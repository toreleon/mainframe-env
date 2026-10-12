//! Fixed local queue/process catalog in the genuine legacy service and original row codecs.
use super::*;
#[path = "service_catalog_composition/committed_put.rs"]
mod committed_put;
pub use committed_put::{MqCommittedPutOffer, PreparedMqCommittedPut};
use mainframe_env_store_api::{
    MAX_PROVIDER_COMPOSITION_BYTES, MAX_PROVIDER_STATE_SCAN, ProviderCompositionCommit,
};

const SELECTION_NAMESPACE: &str = "application-mq-catalog-selection";
const SELECTION_KEY: &str = "catalog";

// Private admission mode carries the existing rollback bit without widening public signatures.
enum CatalogPreparation {
    QueueOnly(bool),
    ProcessGraph(bool),
}

/// Signed static queue catalog owned by one retained local application selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqCatalogProjection {
    /// Canonical package name; it is not a queue-manager or native connection name.
    pub application: String,
    /// Actual retained package generation, including rollback to retained bytes.
    pub generation: u64,
    /// Actual signed package identity.
    pub package_identity: String,
    /// Full ordered local queues and process names; launching and completion are separate owners.
    pub definitions: Vec<MqQueueDefinition>,
    /// Digest of the full original typed catalog encoding, including its manager metadata.
    pub catalog_identity: String,
}

impl MqCatalogProjection {
    /// Validate and bound the complete fixed local declaration projection.
    pub fn content_payload(&self) -> Result<Vec<u8>, HostProblem> {
        if self.application.is_empty()
            || self.application.len() > 128
            || self.application != self.application.to_ascii_uppercase()
            || !self
                .application
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || self.generation == 0
            || self.generation > i64::MAX as u64
            || !valid_identity(&self.package_identity)
        {
            return Err(HostProblem::Malformed);
        }
        let catalog = fixed_catalog(&self.definitions, MqLimits::default(), true)?;
        if catalog_identity(&catalog)? != self.catalog_identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        let raw = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if raw.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(raw)
    }
}

/// Holds the actual MQ durable owner until a same-store, exact-write acknowledgement.
pub struct PreparedMqCatalog<'a> {
    service: &'a MqService,
    durable: LegacyAccess<'a>,
    next: State,
    next_versions: RowVersions,
    writes: Vec<ProviderStateWrite>,
    projection: MqCatalogProjection,
}

impl MqService {
    /// Prepare a signed fixed queue catalog without publishing any provider or server cache.
    /// The existing one-catalog installation predicate remains in force across generations.
    pub fn prepare_catalog_composition(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        definitions: Vec<MqQueueDefinition>,
        previous: Option<&MqCatalogProjection>,
        rollback: bool,
    ) -> Result<PreparedMqCatalog<'_>, HostProblem> {
        self.prepare_catalog_composition_inner(
            application,
            generation,
            package_identity,
            definitions,
            previous,
            CatalogPreparation::QueueOnly(rollback),
        )
    }

    /// Explicit local process-graph catalog preparation, separate from the old queue-only port.
    /// This borrows the genuine owner/CAS plan; it grants no launch or IBM trigger authority.
    /// The application caller must separately authenticate its complete signed graph/controller.
    pub fn prepare_process_graph_composition(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        definitions: Vec<MqQueueDefinition>,
        previous: Option<&MqCatalogProjection>,
        rollback: bool,
    ) -> Result<PreparedMqCatalog<'_>, HostProblem> {
        if !definitions
            .iter()
            .any(|definition| definition.trigger_program.is_some())
        {
            return Err(HostProblem::Malformed);
        }
        self.prepare_catalog_composition_inner(
            application,
            generation,
            package_identity,
            definitions,
            previous,
            CatalogPreparation::ProcessGraph(rollback),
        )
    }

    fn prepare_catalog_composition_inner(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        definitions: Vec<MqQueueDefinition>,
        previous: Option<&MqCatalogProjection>,
        mode: CatalogPreparation,
    ) -> Result<PreparedMqCatalog<'_>, HostProblem> {
        let (rollback, allow_process) = match mode {
            CatalogPreparation::QueueOnly(rollback) => (rollback, false),
            CatalogPreparation::ProcessGraph(rollback) => (rollback, true),
        };
        let catalog = fixed_catalog(&definitions, self.limits, allow_process)?;
        let projection = MqCatalogProjection {
            application: application.into(),
            generation,
            package_identity: package_identity.into(),
            catalog_identity: catalog_identity(&catalog)?,
            definitions,
        };
        projection.content_payload()?;
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        if !durable.state.pending.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current_selection = read_selection(self)?;
        if let Some(prior) = previous {
            prior.content_payload()?;
            if prior.application != projection.application
                || prior.definitions != projection.definitions
                || prior.catalog_identity != projection.catalog_identity
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            verify_projection(&durable.state, prior, self.limits)?;
            let row = current_selection
                .as_ref()
                .ok_or(HostProblem::IdempotencyConflict)?;
            if decode_selection(row)? != *prior {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else if current_selection.is_some() || durable.state.catalog.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.state.scoped_snapshot();
        if let Some(current) = &next.catalog {
            if **current != catalog {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else {
            if rollback {
                return Err(HostProblem::NotFound);
            }
            for definition in &projection.definitions {
                next.queues.insert(
                    definition.name.clone(),
                    Arc::new(Queue {
                        trigger_program: definition.trigger_program.clone(),
                        messages: Vec::new(),
                    }),
                );
            }
            next.catalog = Some(Arc::new(catalog));
        }
        validate_state(&next, self.limits)?;
        let mut next_versions = durable.versions.clone();
        let mut writes = Vec::new();
        // Every existing runtime mutation advances this original manifest.
        // The held service owner plus that same CAS dependency prevents an
        // unrelated live service from changing runtime state before publication.
        for change in row_changes(&durable.state, &next, &durable.versions, self.limits, true)? {
            let write = change.into_prepared_write()?;
            next_versions.insert(
                (write.record.namespace.clone(), write.record.key.clone()),
                write.record.version,
            );
            writes.push(write);
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: SELECTION_NAMESPACE.into(),
                key: SELECTION_KEY.into(),
                version: current_selection
                    .as_ref()
                    .map_or(Some(1), |row| row.version.checked_add(1))
                    .ok_or(HostProblem::ResourceExhausted)?,
                payload: projection.content_payload()?,
            },
            expected_version: current_selection.as_ref().map(|row| row.version),
        });
        let bytes = writes.iter().try_fold(0usize, |sum, write| {
            sum.checked_add(write.record.namespace.len())
                .and_then(|sum| sum.checked_add(write.record.key.len()))
                .and_then(|sum| sum.checked_add(write.record.payload.len()))
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 4 || writes.len() >= MAX_PROVIDER_STATE_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(PreparedMqCatalog {
            service: self,
            durable,
            next,
            next_versions,
            writes,
            projection,
        })
    }

    /// Compare genuine selected ownership and catalog while retaining provider-owned runtime data.
    pub fn selected_catalog_composition_matches(
        &self,
        expected: &MqCatalogProjection,
    ) -> Result<(), HostProblem> {
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        verify_projection(&durable.state, expected, self.limits)?;
        let row = read_selection(self)?.ok_or(HostProblem::NotFound)?;
        if decode_selection(&row)? != *expected {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }
}

fn valid_identity(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
fn fixed_catalog(
    definitions: &[MqQueueDefinition],
    limits: MqLimits,
    allow_process: bool,
) -> Result<MqObjectCatalog, HostProblem> {
    validate_definitions(definitions, limits)?;
    if definitions.is_empty()
        || definitions.iter().any(|definition| {
            (!allow_process && definition.trigger_program.is_some())
                || !is_canonical_name(&definition.name)
        })
        || definitions
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
    {
        return Err(HostProblem::Malformed);
    }
    legacy_catalog(definitions, limits)
}
fn catalog_identity(catalog: &MqObjectCatalog) -> Result<String, HostProblem> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(
            catalog
                .encode()
                .map_err(crate::object_service::object_problem)?
        )
    ))
}
fn read_selection(service: &MqService) -> Result<Option<ProviderStateRecord>, HostProblem> {
    let rows = service
        .store
        .list_provider_state_bounded(SELECTION_NAMESPACE, 2, MAX_PROVIDER_COMPOSITION_BYTES / 8)
        .map_err(store_error)?;
    if rows.len() > 1 {
        return Err(HostProblem::IdempotencyConflict);
    }
    let row = rows.into_iter().next();
    if let Some(row) = &row {
        decode_selection(row)?;
    }
    Ok(row)
}
fn decode_selection(row: &ProviderStateRecord) -> Result<MqCatalogProjection, HostProblem> {
    row.validate_write(MAX_PROVIDER_COMPOSITION_BYTES / 8)
        .map_err(store_error)?;
    if row.namespace != SELECTION_NAMESPACE
        || row.key != SELECTION_KEY
        || row.version == 0
        || row.payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let value: MqCatalogProjection =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if value.content_payload()? != row.payload {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(value)
}
fn verify_projection(
    state: &State,
    expected: &MqCatalogProjection,
    limits: MqLimits,
) -> Result<(), HostProblem> {
    expected.content_payload()?;
    let catalog = fixed_catalog(&expected.definitions, limits, true)?;
    if state.catalog.as_deref() != Some(&catalog) {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

// Rebuild the complete expected rows with the original codecs. No loader,
// migration, runtime call, replay refresh or write occurs during this check.
fn verify_actual_cache(service: &MqService, durable: &DurableState) -> Result<(), HostProblem> {
    if durable.state.definitions.is_some() {
        return Err(HostProblem::Unsupported);
    }
    validate_state(&durable.state, service.limits)?;
    let mut actual = Vec::new();
    let mut bytes = 0usize;
    for namespace in [
        STATE_NAMESPACE,
        CATALOG_NAMESPACE,
        QUEUE_NAMESPACE,
        HANDLE_NAMESPACE,
        PENDING_NAMESPACE,
        REPLAY_NAMESPACE,
    ] {
        let remaining = (MAX_PROVIDER_COMPOSITION_BYTES / 4)
            .checked_sub(bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
        let rows = service
            .store
            .list_provider_state_bounded(namespace, MAX_PROVIDER_STATE_SCAN, remaining)
            .map_err(store_error)?;
        if actual
            .len()
            .checked_add(rows.len())
            .is_none_or(|count| count >= MAX_PROVIDER_STATE_SCAN)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut last: Option<&str> = None;
        for row in &rows {
            row.validate_write(service.limits.max_state_bytes)
                .map_err(store_error)?;
            if row.namespace != namespace || last.is_some_and(|key| key >= row.key.as_str()) {
                return Err(HostProblem::InfrastructureFailure);
            }
            last = Some(&row.key);
            bytes = bytes
                .checked_add(row.namespace.len())
                .and_then(|sum| sum.checked_add(row.key.len()))
                .and_then(|sum| sum.checked_add(row.payload.len()))
                .ok_or(HostProblem::ResourceExhausted)?;
            if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 4 {
                return Err(HostProblem::ResourceExhausted);
            }
        }
        actual.extend(rows);
    }
    let mut expected = Vec::new();
    if !durable.versions.is_empty() {
        add_expected(
            &mut expected,
            &durable.versions,
            STATE_NAMESPACE,
            STATE_KEY,
            serde_json::to_vec(&RowStoreManifest {
                schema_version: ROW_STORE_SCHEMA.into(),
                definitions: None,
                next_handle: durable.state.next_handle,
            })
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        )?;
    }
    if let Some(catalog) = &durable.state.catalog {
        add_expected(
            &mut expected,
            &durable.versions,
            CATALOG_NAMESPACE,
            CATALOG_KEY,
            encode_catalog_row(catalog)?,
        )?;
    }
    add_map(
        &mut expected,
        &durable.versions,
        QUEUE_NAMESPACE,
        &durable.state.queues,
    )?;
    add_map(
        &mut expected,
        &durable.versions,
        HANDLE_NAMESPACE,
        &durable.state.handles,
    )?;
    add_map(
        &mut expected,
        &durable.versions,
        PENDING_NAMESPACE,
        &durable.state.pending,
    )?;
    add_map(
        &mut expected,
        &durable.versions,
        REPLAY_NAMESPACE,
        &durable.state.replay,
    )?;
    if expected.len() != durable.versions.len() {
        return Err(HostProblem::IdempotencyConflict);
    }
    expected.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    actual.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    if expected != actual {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
fn add_expected(
    rows: &mut Vec<ProviderStateRecord>,
    versions: &RowVersions,
    namespace: &str,
    key: &str,
    payload: Vec<u8>,
) -> Result<(), HostProblem> {
    let version = versions
        .get(&(namespace.into(), key.into()))
        .copied()
        .ok_or(HostProblem::IdempotencyConflict)?;
    rows.push(ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version,
        payload,
    });
    Ok(())
}
fn add_map<T: Serialize>(
    rows: &mut Vec<ProviderStateRecord>,
    versions: &RowVersions,
    namespace: &str,
    values: &BTreeMap<String, Arc<T>>,
) -> Result<(), HostProblem> {
    for (key, value) in values {
        add_expected(
            rows,
            versions,
            namespace,
            key,
            encode_object_row(key, value)?,
        )?;
    }
    Ok(())
}
impl PreparedMqCatalog<'_> {
    /// Actual original-codec rows prepared under the held MQ owner.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Full fixed catalog and exact retained package selection.
    pub fn projection(&self) -> &MqCatalogProjection {
        &self.projection
    }
    /// Require the same physical store allocation and every exact prepared write.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Refuse further live legacy runtime operations after uncertain acknowledgement.
    pub fn fence_unknown(&self) {
        self.service.revoke_selected_authority();
    }
    /// Publish only the owned cache after the actual store acknowledgement.
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

// Existing public installer body relocated byte-exact to keep the original module below its ceiling.
impl MqService {
    pub fn install(
        &self,
        definitions: Vec<MqQueueDefinition>,
    ) -> Result<MqInstallReceipt, HostProblem> {
        let definitions = (definitions.into_iter().map(canonical_definition))
            .collect::<Result<Vec<_>, HostProblem>>()?;
        validate_definitions(&definitions, self.limits)?;
        let identity = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&definitions).map_err(|_| HostProblem::ProviderFailure)?
            )
        );
        let catalog = legacy_catalog(&definitions, self.limits)?;
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.catalog {
            return if *current.as_ref() == catalog {
                Ok(install_receipt(&definitions, identity, true))
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let mut next = durable.state.scoped_snapshot();
        for definition in &definitions {
            next.queues.insert(
                definition.name.clone(),
                Arc::new(Queue {
                    trigger_program: definition.trigger_program.clone(),
                    messages: Vec::new(),
                }),
            );
        }
        next.catalog = Some(Arc::new(catalog));
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definitions, identity, false))
    }
}
