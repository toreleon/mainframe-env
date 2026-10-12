//! Finite fixed IMS metadata participant; actual runtime images and original codecs are retained.
use super::*;
use crate::database::{DatabaseEngine, EngineProblem};
use mainframe_env_store_api::{
    MAX_PROVIDER_COMPOSITION_BYTES, MAX_PROVIDER_STATE_SCAN, ProviderCompositionCommit,
};

/// Exact selected static DBD/PSB catalog and original retained package identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsCatalogProjection {
    /// Normalized application owning the one installed generic catalog.
    pub application: String,
    /// Actual retained package generation, including metadata-only rollback selection.
    pub generation: u64,
    /// Signed original package identity used by the existing metadata publisher.
    pub package_identity: String,
    /// Original validated metadata digest; it is independent of runtime database records.
    pub metadata_identity: String,
    /// Full existing catalog, preserving all database, segment, field, PCB and PSB declarations.
    pub catalog: ImsMetadataCatalog,
}
impl ImsCatalogProjection {
    /// Bound the full existing typed catalog; no IMS ABI, migration or runtime-completion claim.
    pub fn content_payload(&self) -> Result<Vec<u8>, HostProblem> {
        crate::metadata_publication::composition::validate_projection(self)?;
        let raw = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if raw.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(raw)
    }
}

/// Actual IMS owner lock and complete original-codec CAS rows until same-store acknowledgement.
pub struct PreparedImsCatalog<'a> {
    service: &'a ImsService,
    durable: MutexGuard<'a, DurableState>,
    next: State,
    next_versions: RowVersions,
    writes: Vec<ProviderStateWrite>,
    projection: ImsCatalogProjection,
}
impl ImsService {
    // Internal codec wrapper shares the actual runtime owner without exposing
    // private cache state to sibling modules or accepting caller-made rows.
    pub(crate) fn with_catalog_composition_owner<T>(
        &self,
        publish: impl FnOnce() -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        let _owner = self.lock()?;
        publish()
    }

    /// Prepare the fixed full generic catalog and genuine retained generation/selection rows.
    /// Existing generic installation forbids catalog changes; this profile keeps that predicate.
    /// Legacy seed declarations, TM installation and schema migration are separate participants.
    pub fn prepare_catalog_composition(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        catalog: ImsMetadataCatalog,
        previous: Option<&ImsCatalogProjection>,
        rollback: bool,
    ) -> Result<PreparedImsCatalog<'_>, HostProblem> {
        let metadata_identity = validate_ims_metadata(&catalog, ImsMetadataLimits::default())
            .map_err(|_| HostProblem::Malformed)?
            .digest;
        self.validate_secondary_metadata(&catalog)?;
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        if !durable.state.pending_undo.is_empty() || !durable.state.generic_pending_undo.is_empty()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let projection = ImsCatalogProjection {
            application: application.to_ascii_uppercase(),
            generation,
            package_identity: package_identity.into(),
            metadata_identity,
            catalog: catalog.clone(),
        };
        projection.content_payload()?;
        if let Some(prior) = previous {
            prior.content_payload()?;
            if prior.application != projection.application || prior.catalog != catalog {
                return Err(HostProblem::IdempotencyConflict);
            }
            verify_projection(self, &durable.state, prior)?;
        } else if durable.state.metadata.is_some() || !durable.state.generic_databases.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.state.scoped_snapshot();
        if let Some(current) = &next.metadata {
            if current != &catalog {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else {
            if rollback {
                return Err(HostProblem::NotFound);
            }
            if next.definitions.as_ref().is_some_and(|legacy| {
                legacy.databases.iter().any(|db| {
                    catalog
                        .databases
                        .iter()
                        .any(|item| normalize(&item.name) == normalize(&db.name))
                }) || legacy.psbs.iter().any(|psb| {
                    catalog
                        .psbs
                        .iter()
                        .any(|item| normalize(&item.name) == normalize(&psb.name))
                })
            }) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut images = BTreeMap::new();
            for database in &catalog.databases {
                let engine = DatabaseEngine::new(
                    generic::definition(database)?,
                    generic::engine_limits(self.limits),
                )
                .map_err(|problem| match problem {
                    EngineProblem::LimitExceeded => HostProblem::ResourceExhausted,
                    EngineProblem::Unsupported => HostProblem::Unsupported,
                    _ => HostProblem::Malformed,
                })?;
                images.insert(normalize(&database.name), Arc::new(engine.image()));
            }
            next.metadata = Some(catalog);
            next.generic_databases = images;
        }
        validate_state(&next, self.limits)?;
        let current_raw =
            serde_json::to_vec(&durable.state).map_err(|_| HostProblem::InfrastructureFailure)?;
        let next_raw = serde_json::to_vec(&next).map_err(|_| HostProblem::InfrastructureFailure)?;
        if current_raw
            .len()
            .checked_add(next_raw.len())
            .is_none_or(|size| size > MAX_PROVIDER_COMPOSITION_BYTES / 2)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        let mut next_versions = durable.versions.clone();
        for change in row_changes(&durable.state, &next, &durable.versions, self.limits, false)? {
            let ProviderStateMutation::Put(write) = change.mutation else {
                return Err(HostProblem::Unsupported);
            };
            if change.next_version != Some(write.record.version)
                || change.namespace != write.record.namespace
                || change.key != write.record.key
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            next_versions.insert((change.namespace, change.key), write.record.version);
            writes.push(write);
        }
        writes.extend(crate::metadata_publication::composition::prepare_selection(
            self,
            &projection,
            previous,
            rollback,
        )?);
        let total = writes.iter().try_fold(0usize, |sum, write| {
            sum.checked_add(write.record.namespace.len())
                .and_then(|sum| sum.checked_add(write.record.key.len()))
                .and_then(|sum| sum.checked_add(write.record.payload.len()))
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        if total > MAX_PROVIDER_COMPOSITION_BYTES / 4 || writes.len() >= MAX_PROVIDER_STATE_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(PreparedImsCatalog {
            service: self,
            durable,
            next,
            next_versions,
            writes,
            projection,
        })
    }

    /// Validate actual static catalog ownership and retained selection, preserving legitimate runtime records.
    pub fn selected_catalog_composition_matches(
        &self,
        expected: &ImsCatalogProjection,
    ) -> Result<(), HostProblem> {
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        verify_projection(self, &durable.state, expected)
    }
}
fn verify_projection(
    service: &ImsService,
    state: &State,
    expected: &ImsCatalogProjection,
) -> Result<(), HostProblem> {
    expected.content_payload()?;
    if state.metadata.as_ref() != Some(&expected.catalog) {
        return Err(HostProblem::IdempotencyConflict);
    }
    validate_state(state, service.limits)?;
    crate::metadata_publication::composition::matches_selection(service, expected)
}

// Complete bounded observations precede the existing loader. A legacy migration
// is explicitly refused here; the ordinary legacy loader remains unchanged.
fn verify_actual_cache(service: &ImsService, durable: &DurableState) -> Result<(), HostProblem> {
    let mut bytes = 0usize;
    let mut versions = RowVersions::new();
    for namespace in [
        STATE_NAMESPACE,
        DATABASE_NAMESPACE,
        SESSION_NAMESPACE,
        CHECKPOINT_NAMESPACE,
        REPLAY_NAMESPACE,
        PENDING_NAMESPACE,
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SYSTEM_NAMESPACE,
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
                .and_then(|sum| sum.checked_add(row.key.len()))
                .and_then(|sum| sum.checked_add(row.payload.len()))
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
    if serde_json::to_vec(&actual)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .len()
        > MAX_PROVIDER_COMPOSITION_BYTES / 4
    {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}
impl PreparedImsCatalog<'_> {
    /// Full original-codec rows and immediate observed CAS versions under the actual IMS lock.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Exact static catalog selection; runtime rows remain genuine provider state.
    pub fn projection(&self) -> &ImsCatalogProjection {
        &self.projection
    }
    /// Require the genuine same physical store and all exact prepared writes.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Quarantine this live cache after uncertain commit acknowledgement until authoritative reopen.
    pub fn fence_unknown(&self) {
        self.service
            .composition_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Publish only the cache after the actual same-store full-write acknowledgement.
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

// Original public installation body, relocated byte-exact without changing any predicate.
impl ImsService {
    pub fn install(
        &self,
        definition: ImsApplicationDefinition,
    ) -> Result<ImsInstallReceipt, HostProblem> {
        validate_definition(&definition, self.limits)?;
        let identity = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&definition).map_err(|_| HostProblem::ProviderFailure)?
            )
        );
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.definitions {
            if current != &definition {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(install_receipt(&definition, identity, true));
        }
        if durable.state.metadata.as_ref().is_some_and(|metadata| {
            definition.databases.iter().any(|db| {
                metadata
                    .databases
                    .iter()
                    .any(|candidate| normalize(&candidate.name) == normalize(&db.name))
            }) || definition.psbs.iter().any(|psb| {
                metadata
                    .psbs
                    .iter()
                    .any(|candidate| normalize(&candidate.name) == normalize(&psb.name))
            })
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.state.scoped_snapshot();
        for database in &definition.databases {
            next.databases.entry(normalize(&database.name)).or_default();
        }
        next.definitions = Some(definition.clone());
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definition, identity, false))
    }
}
