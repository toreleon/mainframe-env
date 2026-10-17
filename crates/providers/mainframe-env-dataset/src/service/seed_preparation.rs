//! Genuine seed preparation under the existing provider state owner.
use super::*;
use mainframe_env_store_api::ProviderCompositionCommit;
use std::sync::MutexGuard;

/// A seed selection prepared by the actual dataset owner, with its mutex held.
/// Dropping before commit has no store/cache effect; unknown commit must fence before release.
pub struct PreparedSeedSelection<'a> {
    service: &'a DatasetService,
    state: MutexGuard<'a, State>,
    planned: SeedGeneration,
    installed: BTreeMap<String, Entry>,
    indexes: Vec<(String, AlternateIndex)>,
    dependencies: Option<DependencyGraph>,
    selection: Option<SeedSelection>,
    writes: Vec<ProviderStateWrite>,
    retain_new: bool,
    replayed: bool,
}
impl DatasetService {
    pub fn selected_seed_generation(&self, package: &str) -> Result<Option<String>, HostProblem> {
        validate_seed_label(package)?;
        Ok(self
            .lock_state()?
            .seed_selections
            .get(&package.to_ascii_uppercase())
            .map(|selection| selection.generation.clone()))
    }

    /// Prepare exact existing seed install or retained rollback without writing any provider row.
    /// A nonidle cursor/lock/unit refuses rather than overwrite live dataset ownership.
    pub fn prepare_seed_selection(
        &self,
        package: &str,
        generation: &str,
        objects: Vec<DatasetSeedObject>,
        retain_new: bool,
    ) -> Result<PreparedSeedSelection<'_>, HostProblem> {
        let state = self.lock_state()?;
        if !state.cursors.is_empty() || !state.locks.is_empty() || !state.tvs_units.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let planned = if retain_new {
            build_seed_generation(package, generation, objects, self.limits)?
        } else {
            if !objects.is_empty() {
                return Err(HostProblem::Malformed);
            }
            validate_seed_label(package)?;
            validate_seed_label(generation)?;
            state
                .seed_generations
                .get(&(package.to_ascii_uppercase(), generation.to_string()))
                .cloned()
                .ok_or(HostProblem::NotFound)?
        };
        let package = planned.package.clone();
        let generation = planned.generation.clone();
        let key = (package.clone(), generation.clone());
        if let Some(existing) = state.seed_generations.get(&key)
            && existing != &planned
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.validate_composition_seed_owners(&state, &planned)?;
        let current_selection = state.seed_selections.get(&package).cloned();
        if current_selection
            .as_ref()
            .is_some_and(|selection| selection.generation == generation)
        {
            return Ok(PreparedSeedSelection {
                service: self,
                state,
                planned,
                installed: BTreeMap::new(),
                indexes: Vec::new(),
                dependencies: None,
                selection: None,
                writes: Vec::new(),
                retain_new,
                replayed: true,
            });
        }
        let previous_entries = current_selection
            .as_ref()
            .and_then(|selection| {
                state
                    .seed_generations
                    .get(&(package.clone(), selection.generation.clone()))
            })
            .map(|generation| generation.entries.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let next_entries = planned.entries.keys().cloned().collect::<Vec<_>>();
        if !previous_entries.is_empty() && previous_entries != next_entries {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut installed = BTreeMap::new();
        let mut writes = Vec::new();
        for (name, snapshot) in &planned.entries {
            let current = state.entries.get(name);
            if current_selection.is_none()
                && current.is_some_and(|entry| {
                    entry.attributes != snapshot.attributes
                        || !entry.records.is_empty()
                        || !entry.members.is_empty()
                        || !entry.relative_records.is_empty()
                        || !entry.member_generations.is_empty()
                        || !entry.member_aliases.is_empty()
                })
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut entry = snapshot.clone();
            entry.version = current.map_or(1, |entry| entry.version.saturating_add(1));
            validate_entry_shape(&entry, self.limits)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: name.clone(),
                    version: entry.version,
                    payload: encode(&entry).map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: current.map(|entry| entry.version),
            });
            installed.insert(name.clone(), entry);
        }
        if state
            .entries
            .len()
            .checked_add(
                installed
                    .keys()
                    .filter(|name| !state.entries.contains_key(*name))
                    .count(),
            )
            .and_then(|total| total.checked_add(state.alternate_indexes.len()))
            .and_then(|total| total.checked_add(state.generation_groups.len()))
            .is_none_or(|total| total > self.limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in &state.alternate_indexes {
            let Some(entry) = installed.get(&index.base) else {
                continue;
            };
            validate_alternate_index(entry, index)?;
            let mut updated = index.clone();
            if updated.upgrade {
                updated.identities = build_alternate_identities(entry, &updated)?;
            }
            validate_index_identities(entry, &updated)?;
            updated.version = updated
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-aix".into(),
                    key: name.clone(),
                    version: updated.version,
                    payload: encode_alternate_index(&updated)?,
                },
                expected_version: Some(index.version),
            });
            updated_indexes.push((name.clone(), updated));
        }
        if retain_new && !state.seed_generations.contains_key(&key) {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-seed-generation".into(),
                    key: seed_generation_key(&package, &generation),
                    version: 1,
                    payload: encode_seed_generation(&planned)?,
                },
                expected_version: None,
            });
        }
        let next_selection = SeedSelection {
            generation: generation.clone(),
            version: current_selection
                .as_ref()
                .map_or(1, |selection| selection.version.saturating_add(1)),
        };
        let mut dependencies = state.dependencies.clone();
        for (name, entry) in &installed {
            dependencies.set_direct_dependencies(
                name,
                entry
                    .catalog
                    .catalog
                    .iter()
                    .map(|catalog| catalog.as_str().to_string()),
                dependency_limits(self.limits),
            )?;
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-seed-selection".into(),
                key: package.clone(),
                version: next_selection.version,
                payload: encode_seed_selection(&next_selection)?,
            },
            expected_version: current_selection
                .as_ref()
                .map(|selection| selection.version),
        });

        Ok(PreparedSeedSelection {
            service: self,
            state,
            planned,
            installed,
            indexes: updated_indexes,
            dependencies: Some(dependencies),
            selection: Some(next_selection),
            writes,
            retain_new,
            replayed: false,
        })
    }
}
impl DatasetService {
    fn validate_composition_seed_owners(
        &self,
        state: &State,
        planned: &SeedGeneration,
    ) -> Result<(), HostProblem> {
        use mainframe_env_store_api::{MAX_PROVIDER_COMPOSITION_BYTES, MAX_PROVIDER_STATE_SCAN};
        // These pages are bounded observations under the actual service owner,
        // not an acknowledgement or exclusion of trusted direct store writers.
        // The new finite composition profile charges observed bytes plus one
        // canonical comparison copy; unchanged legacy selection does not call it.
        let mut remaining = MAX_PROVIDER_COMPOSITION_BYTES / 2;
        let mut observe = |namespace: &str,
                           cap: usize|
         -> Result<Vec<ProviderStateRecord>, HostProblem> {
            let probe = cap
                .checked_add(1)
                .filter(|probe| *probe <= MAX_PROVIDER_STATE_SCAN)
                .ok_or(HostProblem::ResourceExhausted)?;
            if remaining == 0 {
                return Err(HostProblem::ResourceExhausted);
            }
            let rows = self
                .store
                .list_provider_state_bounded(namespace, probe, remaining)
                .map_err(store_error)?;
            if rows.len() > cap {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut previous: Option<&str> = None;
            for row in &rows {
                row.validate_write(remaining).map_err(store_error)?;
                if row.namespace != namespace || previous.is_some_and(|key| key >= row.key.as_str())
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let charged = row
                    .namespace
                    .len()
                    .checked_add(row.key.len())
                    .and_then(|size| size.checked_add(row.payload.len()))
                    .ok_or(HostProblem::ResourceExhausted)?;
                remaining = remaining
                    .checked_sub(charged)
                    .ok_or(HostProblem::ResourceExhausted)?;
                previous = Some(&row.key);
            }
            Ok(rows)
        };
        if state.seed_selections.len() > self.limits.max_datasets
            || state.seed_generations.len() > self.limits.max_idempotency
            || state.entries.len() > self.limits.max_datasets
        {
            return Err(HostProblem::ResourceExhausted);
        }
        if !state
            .seed_generations
            .contains_key(&(planned.package.clone(), planned.generation.clone()))
            && state
                .seed_generations
                .len()
                .checked_add(1)
                .is_none_or(|count| count > self.limits.max_idempotency)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let selections = observe("dataset-seed-selection", self.limits.max_datasets)?;
        let generations = observe("dataset-seed-generation", self.limits.max_idempotency)?;
        let datasets = observe("dataset", self.limits.max_datasets)?;
        if selections.len() != state.seed_selections.len()
            || generations.len() != state.seed_generations.len()
            || datasets.len() != state.entries.len()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        for row in &selections {
            validate_seed_label(&row.key).map_err(|_| HostProblem::InfrastructureFailure)?;
            let actual = decode_seed_selection(&row.payload, row.version)?;
            let retained = state
                .seed_selections
                .get(&row.key)
                .ok_or(HostProblem::UnknownOutcome)?;
            if &actual != retained || encode_seed_selection(retained)? != row.payload {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        // Key order differs from tuple order at prefix boundaries. Resolve by
        // exact canonical key, without assuming that either order is identical.
        let mut retained_keys = BTreeMap::new();
        for ((package, generation), retained) in &state.seed_generations {
            if package != &retained.package
                || generation != &retained.generation
                || retained_keys
                    .insert(seed_generation_key(package, generation), retained)
                    .is_some()
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        for row in &generations {
            let retained = retained_keys
                .get(&row.key)
                .ok_or(HostProblem::UnknownOutcome)?;
            if row.version != 1 || encode_seed_generation(retained)? != row.payload {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        for (package, selected) in &state.seed_selections {
            let retained = state
                .seed_generations
                .get(&(package.clone(), selected.generation.clone()))
                .ok_or(HostProblem::UnknownOutcome)?;
            if package != &planned.package
                && planned
                    .entries
                    .keys()
                    .any(|name| retained.entries.contains_key(name))
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        for name in planned.entries.keys() {
            let actual = datasets.iter().find(|row| &row.key == name);
            match (state.entries.get(name), actual) {
                (Some(retained), Some(row))
                    if retained.version == row.version
                        && encode(retained).map_err(|_| HostProblem::InfrastructureFailure)?
                            == row.payload => {}
                (None, None) => {}
                _ => return Err(HostProblem::UnknownOutcome),
            }
        }
        Ok(())
    }

    /// Match the complete retained seed snapshot and selected generation under the actual owner.
    pub fn selected_seed_matches(
        &self,
        package: &str,
        generation: &str,
        identity: &str,
        snapshot: &str,
    ) -> Result<bool, HostProblem> {
        let state = self.lock_state()?;
        let package = package.to_ascii_uppercase();
        let Some(selected) = state.seed_selections.get(&package) else {
            return Ok(false);
        };
        let Some(retained) = state
            .seed_generations
            .get(&(package, generation.to_string()))
        else {
            return Ok(false);
        };
        Ok(selected.generation == generation
            && retained.identity == identity
            && format!(
                "sha256:{:x}",
                Sha256::digest(encode_seed_generation(retained)?)
            ) == snapshot)
    }
}
impl PreparedSeedSelection<'_> {
    /// Borrow the owner-validated exact writes; callers cannot alter this held preparation.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Exact retained source-owned seed identity selected by this preparation.
    pub fn receipt(&self) -> SeedInstallReceipt {
        seed_receipt(&self.planned, self.replayed)
    }
    /// Bind complete original seed attributes, record geometry and full bytes, not only raw identity.
    pub fn snapshot_digest(&self) -> Result<String, HostProblem> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(encode_seed_generation(&self.planned)?)
        ))
    }
    /// Validate the actual same-store acknowledgement before any live cache update.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Keep the live owner fenced after an unknown shared commit until authoritative reopen.
    pub fn fence_unknown(&self) {
        self.service
            .composition_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Publish cache fields under the held owner after the exact commit has been checked.
    pub fn finish(
        mut self,
        commit: &ProviderCompositionCommit,
    ) -> Result<SeedInstallReceipt, HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        self.state
            .entries
            .extend(std::mem::take(&mut self.installed));
        if let Some(dependencies) = self.dependencies.take() {
            self.state.dependencies = dependencies;
        }
        for (name, index) in std::mem::take(&mut self.indexes) {
            self.state.alternate_indexes.insert(name, index);
        }
        if self.retain_new {
            self.state.seed_generations.insert(
                (
                    self.planned.package.clone(),
                    self.planned.generation.clone(),
                ),
                self.planned.clone(),
            );
        }
        if let Some(selection) = self.selection.take() {
            self.state
                .seed_selections
                .insert(self.planned.package.clone(), selection);
        }
        Ok(self.receipt())
    }
}

impl DatasetService {
    pub(super) fn select_seed_generation(
        &self,
        planned: SeedGeneration,
        retain_new: bool,
    ) -> Result<SeedInstallReceipt, HostProblem> {
        let mut state = self.lock_state()?;
        let package = planned.package.clone();
        let generation = planned.generation.clone();
        let key = (package.clone(), generation.clone());
        if let Some(existing) = state.seed_generations.get(&key)
            && existing != &planned
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current_selection = state.seed_selections.get(&package).cloned();
        if current_selection
            .as_ref()
            .is_some_and(|selection| selection.generation == generation)
        {
            return Ok(seed_receipt(&planned, true));
        }
        let previous_entries = current_selection
            .as_ref()
            .and_then(|selection| {
                state
                    .seed_generations
                    .get(&(package.clone(), selection.generation.clone()))
            })
            .map(|generation| generation.entries.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let next_entries = planned.entries.keys().cloned().collect::<Vec<_>>();
        if !previous_entries.is_empty() && previous_entries != next_entries {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut installed = BTreeMap::new();
        let mut writes = Vec::new();
        for (name, snapshot) in &planned.entries {
            let current = state.entries.get(name);
            if current_selection.is_none()
                && current.is_some_and(|entry| {
                    entry.attributes != snapshot.attributes
                        || !entry.records.is_empty()
                        || !entry.members.is_empty()
                        || !entry.relative_records.is_empty()
                        || !entry.member_generations.is_empty()
                        || !entry.member_aliases.is_empty()
                })
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut entry = snapshot.clone();
            entry.version = current.map_or(1, |entry| entry.version.saturating_add(1));
            validate_entry_shape(&entry, self.limits)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: name.clone(),
                    version: entry.version,
                    payload: encode(&entry).map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: current.map(|entry| entry.version),
            });
            installed.insert(name.clone(), entry);
        }
        if state
            .entries
            .len()
            .checked_add(
                installed
                    .keys()
                    .filter(|name| !state.entries.contains_key(*name))
                    .count(),
            )
            .and_then(|total| total.checked_add(state.alternate_indexes.len()))
            .and_then(|total| total.checked_add(state.generation_groups.len()))
            .is_none_or(|total| total > self.limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in &state.alternate_indexes {
            let Some(entry) = installed.get(&index.base) else {
                continue;
            };
            validate_alternate_index(entry, index)?;
            let mut updated = index.clone();
            if updated.upgrade {
                updated.identities = build_alternate_identities(entry, &updated)?;
            }
            validate_index_identities(entry, &updated)?;
            updated.version = updated
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-aix".into(),
                    key: name.clone(),
                    version: updated.version,
                    payload: encode_alternate_index(&updated)?,
                },
                expected_version: Some(index.version),
            });
            updated_indexes.push((name.clone(), updated));
        }
        if retain_new && !state.seed_generations.contains_key(&key) {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-seed-generation".into(),
                    key: seed_generation_key(&package, &generation),
                    version: 1,
                    payload: encode_seed_generation(&planned)?,
                },
                expected_version: None,
            });
        }
        let next_selection = SeedSelection {
            generation: generation.clone(),
            version: current_selection
                .as_ref()
                .map_or(1, |selection| selection.version.saturating_add(1)),
        };
        let mut dependencies = state.dependencies.clone();
        for (name, entry) in &installed {
            dependencies.set_direct_dependencies(
                name,
                entry
                    .catalog
                    .catalog
                    .iter()
                    .map(|catalog| catalog.as_str().to_string()),
                dependency_limits(self.limits),
            )?;
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-seed-selection".into(),
                key: package.clone(),
                version: next_selection.version,
                payload: encode_seed_selection(&next_selection)?,
            },
            expected_version: current_selection
                .as_ref()
                .map(|selection| selection.version),
        });
        if let Err(error) = self.store.put_provider_states_atomic(writes) {
            if matches!(
                error,
                StoreError::CapacityExceeded | StoreError::PayloadTooLarge
            ) {
                return Err(HostProblem::ResourceExhausted);
            }
            if error == StoreError::Conflict {
                return Err(HostProblem::IdempotencyConflict);
            }
            let selected = self
                .store
                .get_provider_state("dataset-seed-selection", &package)
                .map_err(store_error)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if decode_seed_selection(&selected.payload, selected.version)? != next_selection {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        state.entries.extend(installed);
        state.dependencies = dependencies;
        for (name, index) in updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        if retain_new {
            state.seed_generations.insert(key, planned.clone());
        }
        state.seed_selections.insert(package, next_selection);
        Ok(seed_receipt(&planned, false))
    }

    pub(super) fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, State>, HostProblem> {
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self
            .composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(state)
    }
}
