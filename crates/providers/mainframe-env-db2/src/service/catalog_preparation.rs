//! Finite held catalog participant; existing row codecs and prospective transitions remain authoritative.
use super::*;
use mainframe_env_store_api::{
    MAX_PROVIDER_COMPOSITION_BYTES, MAX_PROVIDER_STATE_SCAN, ProviderCompositionCommit,
};

/// Exact selected static catalog projection; runtime table rows and replay state remain provider-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Db2CatalogProjection {
    /// Actual normalized application selected by the existing catalog codec.
    pub application: String,
    /// Actual retained generation, including genuine rollback selection.
    pub generation: u64,
    /// Actual signed application identity used by the original catalog installer.
    pub identity: String,
    /// Full selected table definitions; widths, encodings, defaults, keys and extract layouts are explicit.
    pub tables: BTreeMap<String, Db2TableDefinition>,
}
impl Db2CatalogProjection {
    /// Bounded canonical participating content; this does not serialize runtime rows or vendor semantics.
    pub fn content_payload(&self) -> Result<Vec<u8>, HostProblem> {
        Db2CatalogGeneration {
            application: self.application.clone(),
            generation: self.generation,
            identity: self.identity.clone(),
            tables: self.tables.values().cloned().collect(),
            rows: Vec::new(),
        }
        .validate(Db2Limits::default())?;
        if self.application != self.application.to_ascii_uppercase()
            || self
                .tables
                .iter()
                .any(|(name, table)| name != &table.normalized_name())
        {
            return Err(HostProblem::Malformed);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if self.application.is_empty()
            || self.generation == 0
            || self.tables.is_empty()
            || self.tables.len() > Db2Limits::default().max_tables
            || bytes.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8
        {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(bytes)
    }
}

/// Real provider lock and complete original-codec CAS writes held through a genuine acknowledgement.
pub struct PreparedDb2Catalog<'a> {
    service: &'a Db2Service,
    durable: MutexGuard<'a, DurableState>,
    next: State,
    next_versions: RowVersions,
    writes: Vec<ProviderStateWrite>,
    projection: Db2CatalogProjection,
}
impl Db2Service {
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, DurableState>, HostProblem> {
        if self
            .composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let owner = self
            .durable
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self
            .composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(owner)
    }

    /// Prepare a signed finite selected catalog without persisting or publishing live cache.
    /// Existing local ownership forbids legacy adoption, foreign selected tables and disappearing names.
    /// Deletions require a separately owned mutation acknowledgement and are not admitted by this Put-only profile.
    pub fn prepare_catalog_composition(
        &self,
        catalog: Db2CatalogGeneration,
        previous: Option<&Db2CatalogProjection>,
        rollback: bool,
    ) -> Result<PreparedDb2Catalog<'_>, HostProblem> {
        catalog.validate(self.limits)?;
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        let application = catalog.application.to_ascii_uppercase();
        let names = catalog
            .tables
            .iter()
            .map(Db2TableDefinition::normalized_name)
            .collect::<BTreeSet<_>>();
        if let Some(prior) = previous {
            if prior.application != application
                || prior.tables.keys().cloned().collect::<BTreeSet<_>>() != names
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            verify_projection(&durable.state, prior)?;
        } else if durable.state.installations.contains_key(&application)
            || names.iter().any(|name| {
                durable.state.tables.contains_key(name) || durable.state.schemas.contains_key(name)
            })
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        // Foreign CURRENT catalog ownership is checked before the original exact-retry fast path.
        for (owner, installed) in &durable.state.installations {
            if owner != &application && installed.tables.iter().any(|name| names.contains(name)) {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let next = if rollback {
            if previous.is_none() {
                return Err(HostProblem::NotFound);
            }
            self.prospective_catalog_rollback(&durable.state, &application, catalog.generation)?
        } else {
            self.prospective_catalog_install(&durable.state, catalog.clone())?
                .unwrap_or_else(|| durable.state.scoped_snapshot())
        };
        let projection = projection(&next, &application)?;
        let offered = catalog
            .tables
            .iter()
            .map(|table| (table.normalized_name(), table.clone()))
            .collect::<BTreeMap<_, _>>();
        if projection.generation != catalog.generation
            || projection.identity != catalog.identity
            || projection.tables != offered
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current_bytes =
            serde_json::to_vec(&durable.state).map_err(|_| HostProblem::InfrastructureFailure)?;
        let next_bytes =
            serde_json::to_vec(&next).map_err(|_| HostProblem::InfrastructureFailure)?;
        if current_bytes
            .len()
            .checked_add(next_bytes.len())
            .is_none_or(|bytes| bytes > MAX_PROVIDER_COMPOSITION_BYTES / 2)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let changes = row_changes(&durable.state, &next, &durable.versions, self.limits, false)?;
        let mut writes = Vec::new();
        let mut next_versions = durable.versions.clone();
        let mut payload_bytes = 0usize;
        for change in changes {
            let ProviderStateMutation::Put(write) = change.mutation else {
                return Err(HostProblem::Unsupported);
            };
            payload_bytes = payload_bytes
                .checked_add(write.record.payload.len())
                .and_then(|bytes| bytes.checked_add(write.record.namespace.len()))
                .and_then(|bytes| bytes.checked_add(write.record.key.len()))
                .ok_or(HostProblem::ResourceExhausted)?;
            if payload_bytes > MAX_PROVIDER_COMPOSITION_BYTES / 4
                || writes.len() >= MAX_PROVIDER_STATE_SCAN - 1
            {
                return Err(HostProblem::ResourceExhausted);
            }
            if change.next_version != Some(write.record.version)
                || change.namespace != write.record.namespace
                || change.key != write.record.key
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            next_versions.insert((change.namespace, change.key), write.record.version);
            writes.push(write);
        }
        projection.content_payload()?;
        Ok(PreparedDb2Catalog {
            service: self,
            durable,
            next,
            next_versions,
            writes,
            projection,
        })
    }

    /// Verify actual selected static ownership under the provider lock; legitimate runtime row edits are retained.
    pub fn selected_catalog_composition_matches(
        &self,
        expected: &Db2CatalogProjection,
    ) -> Result<(), HostProblem> {
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        verify_projection(&durable.state, expected)
    }
}

fn projection(state: &State, application: &str) -> Result<Db2CatalogProjection, HostProblem> {
    let selected = state
        .installations
        .get(application)
        .ok_or(HostProblem::NotFound)?;
    let retained = state
        .catalog_generations
        .get(application)
        .and_then(|items| items.get(&selected.generation))
        .ok_or(HostProblem::InfrastructureFailure)?;
    if selected.identity != retained.identity
        || selected.generation != retained.generation
        || selected.tables != retained.schemas.keys().cloned().collect::<BTreeSet<_>>()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut tables = BTreeMap::new();
    for name in &selected.tables {
        let schema = state
            .schemas
            .get(name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let table = state
            .tables
            .get(name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if retained.schemas.get(name) != Some(schema)
            || table.columns != schema.columns.len()
            || !matches!(state.table_provenance.get(name), Some(TableProvenance::Application { owner, .. }) if owner == application)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        tables.insert(name.clone(), schema.clone());
    }
    Ok(Db2CatalogProjection {
        application: application.into(),
        generation: selected.generation,
        identity: selected.identity.clone(),
        tables,
    })
}
fn verify_projection(state: &State, expected: &Db2CatalogProjection) -> Result<(), HostProblem> {
    expected.content_payload()?;
    if projection(state, &expected.application)? != *expected {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

// Bound complete actual row observations before using the existing loader. It may
// read this same-service snapshot, but cannot perform a legacy migration here.
fn verify_actual_cache(service: &Db2Service, durable: &DurableState) -> Result<(), HostProblem> {
    let mut bytes = 0usize;
    let mut versions = RowVersions::new();
    for namespace in [
        STATE_NAMESPACE,
        TABLE_NAMESPACE,
        SCHEMA_NAMESPACE,
        INSTALLATION_NAMESPACE,
        GENERATION_NAMESPACE,
        PROVENANCE_NAMESPACE,
        LEGACY_SNAPSHOT_NAMESPACE,
        PENDING_NAMESPACE,
        CURSOR_NAMESPACE,
        CURSOR_DECLARATION_NAMESPACE,
        REPLAY_NAMESPACE,
    ] {
        let remaining = (MAX_PROVIDER_COMPOSITION_BYTES / 4)
            .checked_sub(bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
        let rows = service
            .store
            .list_provider_state_bounded(namespace, MAX_PROVIDER_STATE_SCAN, remaining)
            .map_err(store_error)?;
        if rows.len() >= MAX_PROVIDER_STATE_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut last: Option<String> = None;
        for row in rows {
            if row.namespace != namespace
                || row.version == 0
                || last.as_ref().is_some_and(|key| key >= &row.key)
                || versions
                    .insert((namespace.into(), row.key.clone()), row.version)
                    .is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            last = Some(row.key.clone());
            bytes = bytes
                .checked_add(row.namespace.len())
                .and_then(|bytes| bytes.checked_add(row.key.len()))
                .and_then(|bytes| bytes.checked_add(row.payload.len()))
                .ok_or(HostProblem::ResourceExhausted)?;
            if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 4 {
                return Err(HostProblem::ResourceExhausted);
            }
            if namespace == STATE_NAMESPACE {
                let manifest: RowStoreManifest =
                    serde_json::from_slice(&row.payload).map_err(|_| HostProblem::Unsupported)?;
                if row.key != STATE_KEY || manifest.schema_version != ROW_STORE_SCHEMA {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
        }
    }
    let (actual, actual_versions) = load_or_migrate(&*service.store, service.limits)?;
    if actual != durable.state || actual_versions != durable.versions || versions != actual_versions
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    let canonical = serde_json::to_vec(&actual).map_err(|_| HostProblem::InfrastructureFailure)?;
    if canonical.len() > MAX_PROVIDER_COMPOSITION_BYTES / 4 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}
impl PreparedDb2Catalog<'_> {
    /// Original full-codec Put rows and exact CAS versions prepared under the actual lock.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Selected static projection; original table/replay/runtime payloads remain in full provider rows.
    pub fn projection(&self) -> &Db2CatalogProjection {
        &self.projection
    }
    /// Require the actual same store and all exact original prepared writes.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Quarantine this live provider after unknown acknowledgement; authoritative reopen is required.
    pub fn fence_unknown(&self) {
        self.service
            .composition_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Install the prepared provider cache only after the genuine same-store commit acknowledges every row.
    pub fn finish(mut self, commit: &ProviderCompositionCommit) -> Result<(), HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        self.durable.state = self.next;
        self.durable.versions = self.next_versions;
        Ok(())
    }
}
