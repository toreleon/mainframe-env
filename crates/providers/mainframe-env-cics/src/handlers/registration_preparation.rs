//! Genuine idle CICS definition preparation for one shared installer transaction.
use super::*;
use mainframe_env_store_api::ProviderCompositionCommit;
use std::sync::MutexGuard;

/// Opaque registration plan holding the actual CICS state owner until publication.
pub struct PreparedCicsRegistration<'a> {
    service: &'a CicsService,
    state: MutexGuard<'a, State>,
    programs: BTreeSet<String>,
    maps: BTreeMap<(String, String), BmsMapDefinition>,
    files: BTreeMap<String, CicsFileDefinition>,
    writes: Vec<ProviderStateWrite>,
}
impl CicsService {
    pub fn register_transform_definition(
        &self,
        definition: CicsTransformDefinition,
    ) -> Result<(), HostProblem> {
        handlers::register_transform_definition(self, definition)
    }
    /// Encode the exact existing map persistence representation after ordinary validation.
    /// This does not compile BMS source or supply an IBM default.
    pub fn composition_map_payload(
        &self,
        definition: &BmsMapDefinition,
    ) -> Result<Vec<u8>, HostProblem> {
        validate_map(definition, self.limits)?;
        let mut bytes = definition
            .mapset
            .len()
            .checked_add(definition.map.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        for field in &definition.fields {
            for count in [
                field.name.len(),
                field.initial.len(),
                field.color.as_ref().map_or(0, String::len),
                field.highlight.as_ref().map_or(0, String::len),
            ] {
                bytes = bytes
                    .checked_add(count)
                    .ok_or(HostProblem::ResourceExhausted)?;
            }
        }
        if bytes > mainframe_env_store_api::MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut normalized = definition.clone();
        normalized.mapset = normalized.mapset.to_ascii_uppercase();
        normalized.map = normalized.map.to_ascii_uppercase();
        encode_map(&normalized)
    }
    /// Encode the actual explicit FILE codec, including the full target name and CCSID.
    pub fn composition_file_payload(
        &self,
        definition: &CicsFileDefinition,
    ) -> Result<Vec<u8>, HostProblem> {
        encode_file_definition(definition)
    }
    /// Prepare full definitions without any write. Existing foreign definitions cannot be replaced.
    /// An exact prior owned row is required for each changed map/file; active tasks refuse.
    pub fn prepare_registration(
        &self,
        programs: &BTreeSet<String>,
        maps: &[BmsMapDefinition],
        files: &BTreeMap<String, CicsFileDefinition>,
        previous: &[ProviderStateRecord],
    ) -> Result<PreparedCicsRegistration<'_>, HostProblem> {
        let state = self.lock()?;
        if !state.runs.is_empty() || state.task_dispatch.absent_runs(&state.runs) != 0 {
            return Err(HostProblem::IdempotencyConflict);
        }
        for session in state.sessions.keys() {
            state.task_dispatch.require_idle_session(session)?;
        }
        let normalized = programs
            .iter()
            .map(|name| normalize_terminal_name(name, 128))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if normalized.len() != programs.len() || normalized.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut normalized_maps = BTreeMap::new();
        for definition in maps {
            // The same finite preflight runs before cloning any map string or initial bytes.
            self.composition_map_payload(definition)?;
            let mut definition = definition.clone();
            definition.mapset = definition.mapset.to_ascii_uppercase();
            definition.map = definition.map.to_ascii_uppercase();
            let key = (definition.mapset.clone(), definition.map.clone());
            if normalized_maps.insert(key, definition).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let mut normalized_files = BTreeMap::new();
        for (name, definition) in files {
            let name = normalize_terminal_name(name, 16)?;
            if normalized_files.insert(name, definition.clone()).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        if state
            .programs
            .len()
            .checked_add(normalized.difference(&state.programs).count())
            .is_none_or(|n| n > self.limits.max_programs)
            || state
                .maps
                .len()
                .checked_add(
                    normalized_maps
                        .keys()
                        .filter(|key| !state.maps.contains_key(*key))
                        .count(),
                )
                .is_none_or(|n| n > self.limits.max_maps)
            || state
                .file_aliases
                .len()
                .checked_add(
                    normalized_files
                        .keys()
                        .filter(|key| !state.file_aliases.contains_key(*key))
                        .count(),
                )
                .is_none_or(|n| n > self.limits.max_file_aliases)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for name in &normalized {
            self.prepare_registration_row(
                &mut writes,
                previous,
                "cics-program",
                name,
                Vec::new(),
                state.programs.contains(name),
            )?;
        }
        for ((mapset, map), definition) in &normalized_maps {
            let current = state.maps.get(&(mapset.clone(), map.clone()));
            if let Some(current) = current {
                let row = self
                    .store
                    .get_provider_state("cics-map", &map_key(mapset, map))
                    .map_err(store_error)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if row.payload != encode_map(current)? {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
            self.prepare_registration_row(
                &mut writes,
                previous,
                "cics-map",
                &map_key(mapset, map),
                encode_map(definition)?,
                current.is_some(),
            )?;
        }
        for (name, definition) in &normalized_files {
            let current = state.file_aliases.get(name);
            if let Some(current) = current {
                let row = self
                    .store
                    .get_provider_state("cics-file-alias", name)
                    .map_err(store_error)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if decode_file_definition(&row.payload)? != *current {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
            self.prepare_registration_row(
                &mut writes,
                previous,
                "cics-file-alias",
                name,
                encode_file_definition(definition)?,
                current.is_some(),
            )?;
        }
        Ok(PreparedCicsRegistration {
            service: self,
            state,
            programs: normalized,
            maps: normalized_maps,
            files: normalized_files,
            writes,
        })
    }
    fn prepare_registration_row(
        &self,
        writes: &mut Vec<ProviderStateWrite>,
        previous: &[ProviderStateRecord],
        namespace: &str,
        key: &str,
        payload: Vec<u8>,
        cached: bool,
    ) -> Result<(), HostProblem> {
        let current = self
            .store
            .get_provider_state(namespace, key)
            .map_err(store_error)?;
        if cached != current.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        if let Some(row) = &current
            && !previous.contains(row)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let version = current
            .as_ref()
            .map_or(Some(1), |row| row.version.checked_add(1))
            .ok_or(HostProblem::ResourceExhausted)?;
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version,
                payload,
            },
            expected_version: current.map(|row| row.version),
        });
        Ok(())
    }
}
impl PreparedCicsRegistration<'_> {
    /// Exact definitions prepared under the actual state owner.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Check the genuine successful same-store CAS before touching any cache.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Fence the live service until reopen after an unknown shared commit.
    pub fn fence_unknown(&self) {
        self.service
            .composition_uncertain
            .store(true, Ordering::SeqCst);
    }
    /// Install cache fields while the service owner is held; finish only accepts actual committed bytes.
    pub fn finish(mut self, commit: &ProviderCompositionCommit) -> Result<(), HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        self.state
            .programs
            .extend(std::mem::take(&mut self.programs));
        self.state.maps.extend(std::mem::take(&mut self.maps));
        self.state
            .file_aliases
            .extend(std::mem::take(&mut self.files));
        Ok(())
    }
}

impl CicsService {
    pub fn register_file_aliases(
        &self,
        aliases: &BTreeMap<String, DatasetName>,
    ) -> Result<(), HostProblem> {
        self.register_file_definitions(
            &aliases
                .iter()
                .map(|(name, dataset)| {
                    (
                        name.clone(),
                        CicsFileDefinition {
                            dataset: dataset.clone(),
                            ccsid: None,
                        },
                    )
                })
                .collect(),
        )
    }

    pub fn register_file_definitions(
        &self,
        aliases: &BTreeMap<String, CicsFileDefinition>,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let new_aliases = aliases
            .keys()
            .filter(|name| !state.file_aliases.contains_key(*name))
            .count();
        if state
            .file_aliases
            .len()
            .checked_add(new_aliases)
            .is_none_or(|total| total > self.limits.max_file_aliases)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for (name, definition) in aliases {
            let name = name.trim().to_ascii_uppercase();
            if name.is_empty()
                || name.len() > 16
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(HostProblem::Malformed);
            }
            if let Some(existing) = state.file_aliases.get(&name) {
                if existing != definition {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-file-alias".into(),
                    key: name,
                    version: 1,
                    payload: encode_file_definition(definition)?,
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for (name, definition) in aliases {
            state
                .file_aliases
                .insert(name.trim().to_ascii_uppercase(), definition.clone());
        }
        Ok(())
    }

    pub(super) fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, HostProblem> {
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self.composition_uncertain.load(Ordering::SeqCst) {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(state)
    }
}
