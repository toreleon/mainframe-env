//! Version-fenced publication from IMS utility staging into the live generic
//! database object row. The stage and receipt are evidence, not data authorities.

use super::*;
use crate::database::{DatabaseDefinition, DatabaseEngine};
use crate::recovery::utilities::load_stage;
use crate::recovery::{
    RecoveryLimits, RecoveryProblem, RecoveryTransition, TrackedResource, TrackedResourceKind,
    UtilityImage, UtilityKind, UtilityPublishReceipt,
};
use mainframe_env_store_api::{EffectState, IdempotencyStore};

const SELECTION_NAMESPACE: &str = "ims-v1-metadata-selection";
const PUBLICATION_NAMESPACE: &str = "ims-recovery-v1-db-publication";
const PUBLICATION_SCHEMA: &str = "mainframe-env.ims-utility-publication@1";
const PUBLICATION_DOMAIN: &str = "mainframe-env.ims-utility-publication-digest@1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    schema_version: String,
    job: String,
    application: String,
    package_identity: String,
    database: String,
    generation: u64,
    image_digest: [u8; 32],
    effect_key: String,
    request_digest: [u8; 32],
    receipt_digest: [u8; 32],
}

impl Publication {
    fn digest(&self) -> [u8; 32] {
        let bytes = serde_json::to_vec(&(
            PUBLICATION_DOMAIN,
            &self.schema_version,
            &self.job,
            &self.application,
            &self.package_identity,
            &self.database,
            self.generation,
            self.image_digest,
            &self.effect_key,
            self.request_digest,
        ))
        .expect("bounded IMS publication receipt is JSON serializable");
        Sha256::digest(bytes).into()
    }
}

impl ImsService {
    /// Owned application adapter publication over the same row/CAS bridge.
    /// No public API accepts a caller-created State or row mutations.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn publish_application_recovery(
        &self,
        invocation: &Invocation,
        request: &mainframe_env_host_api::ImsRecoveryRequest,
        durable: &mut DurableState,
        next: State,
        transition: RecoveryTransition,
        effects: &dyn IdempotencyStore,
        digest: [u8; 32],
    ) -> Result<(), HostProblem> {
        let selected = self
            .selected_metadata_generation(&request.application)?
            .ok_or(HostProblem::NotFound)?;
        if selected.package_identity != request.package_identity
            || next.metadata.as_ref() != Some(&selected.catalog)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let selection = self.selected_metadata_selection_fence(&selected)?;
        validate_state(&next, self.limits)?;
        let mut changes =
            row_changes(&durable.state, &next, &durable.versions, self.limits, false)?;
        // Fence observed GU images as well as real undo release. A concurrent
        // writer cannot invalidate position between observation and publication.
        let psb = next
            .metadata
            .as_ref()
            .and_then(|catalog| catalog.psbs.iter().find(|psb| psb.name == request.psb))
            .ok_or(HostProblem::NotFound)?;
        for pcb in &psb.pcbs {
            if let mainframe_env_host_api::ImsPcbMetadata::Database(pcb) = pcb
                && !changes.iter().any(|change| {
                    change.namespace == GENERIC_DATABASE_NAMESPACE && change.key == pcb.database
                })
            {
                let image = next
                    .generic_databases
                    .get(&pcb.database)
                    .ok_or(HostProblem::NotFound)?;
                changes.push(put_row_change(
                    GENERIC_DATABASE_NAMESPACE,
                    &pcb.database,
                    encode_object_row(&pcb.database, image)?,
                    &durable.versions,
                    self.limits.max_state_bytes,
                )?);
            }
        }
        let mut mutations = changes
            .iter()
            .map(|change| change.mutation.clone())
            .collect::<Vec<_>>();
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                version: selection
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?,
                ..selection.clone()
            },
            expected_version: Some(selection.version),
        }));
        if invocation.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        if let Some(clock) = &self.replay_clock
            && clock.now_tick()? >= invocation.deadline_tick
        {
            return Err(HostProblem::TimedOut);
        }
        let effect = effects
            .effect(&request.mutation.idempotency_key)
            .map_err(store_error)?
            .ok_or(HostProblem::MissingIdempotency)?;
        if effect.intent.recovery_lease.is_some() || effect.state == EffectState::UnknownOutcome {
            return Err(HostProblem::UnknownOutcome);
        }
        transition
            .publish_with_intent(
                &*self.store,
                effects,
                &request.mutation.idempotency_key,
                digest,
                mutations,
            )
            .map_err(host_recovery_error)?;
        for change in changes {
            let identity = (change.namespace, change.key);
            if let Some(version) = change.next_version {
                durable.versions.insert(identity, version);
            } else {
                durable.versions.remove(&identity);
            }
        }
        durable.state = next;
        Ok(())
    }
    /// The live generic database row to capture in a recovery UOW baseline.
    pub fn recovery_database_resource(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
    ) -> Result<TrackedResource, RecoveryProblem> {
        self.utility_binding(
            invocation,
            application,
            package_identity,
            database,
            AccessIntent::Update,
        )?;
        Ok(TrackedResource {
            namespace: GENERIC_DATABASE_NAMESPACE.into(),
            key: normalize(database),
            kind: TrackedResourceKind::Database,
        })
    }

    /// Publish a checkpoint/log/backout proposal through the same fenced UOW
    /// store as the normal database route. A backout image is validated against
    /// selected metadata before any row mutation is submitted.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_database_recovery_transition(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
        transition: RecoveryTransition,
        effects: &dyn IdempotencyStore,
        key: &IdempotencyKey,
        request_digest: [u8; 32],
    ) -> Result<(), RecoveryProblem> {
        let (definition, selection) = self.utility_binding(
            invocation,
            application,
            package_identity,
            database,
            AccessIntent::Update,
        )?;
        let effect = effects
            .effect(key)
            .map_err(read_store_error)?
            .ok_or(RecoveryProblem::InvalidRequest)?;
        if effect.execution_id != invocation.execution_id
            || effect.run_unit_id != invocation.run_unit_id
            || effect.request_digest != request_digest
        {
            return Err(RecoveryProblem::Conflict);
        }
        let name = normalize(database);
        let mut restored_image = None;
        for mutation in transition.resource_mutations() {
            match mutation {
                ProviderStateMutation::Put(write)
                    if write.record.namespace == GENERIC_DATABASE_NAMESPACE
                        && write.record.key == name =>
                {
                    let row: ObjectRow<DatabaseEngineImage> =
                        serde_json::from_slice(&write.record.payload)
                            .map_err(|_| RecoveryProblem::CorruptImage)?;
                    if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != name {
                        return Err(RecoveryProblem::CorruptImage);
                    }
                    let engine =
                        DatabaseEngine::restore(row.value, generic::engine_limits(self.limits))
                            .map_err(|_| RecoveryProblem::CorruptImage)?;
                    if engine.definition() != &definition {
                        return Err(RecoveryProblem::Conflict);
                    }
                    restored_image = Some(engine.image());
                }
                _ => return Err(RecoveryProblem::Unsupported),
            }
        }
        let changed = restored_image.is_some();
        let mut durable = self.lock().map_err(host_error)?;
        if durable.state.metadata.as_ref().is_none_or(|catalog| {
            !catalog.databases.iter().any(|candidate| {
                normalize(&candidate.name) == name
                    && generic::definition(candidate).as_ref() == Ok(&definition)
            })
        }) {
            return Err(RecoveryProblem::Conflict);
        }
        let mut next = durable.state.scoped_snapshot();
        if changed {
            system::reservations::refresh(&*self.store, self.limits, &mut durable)
                .map_err(host_error)?;
            generic::isolation::refresh_undo(&*self.store, self.limits, &mut durable)
                .map_err(host_error)?;
            generic::isolation::ensure_writer(
                &durable.state,
                invocation.run_unit_id.as_str(),
                &name,
            )
            .map_err(host_error)?;
            next = durable.state.scoped_snapshot();
            generic::isolation::publish_backout_image(
                &mut next,
                invocation.run_unit_id.as_str(),
                &name,
                restored_image.ok_or(RecoveryProblem::CorruptImage)?,
                self.limits,
            )
            .map_err(host_error)?;
            generic::reset_positions(&mut next, &name, None);
            validate_state(&next, self.limits).map_err(host_error)?;
        }
        let mut changes = row_changes(&durable.state, &next, &durable.versions, self.limits, false)
            .map_err(host_error)?;
        if changed {
            // The recovery proposal supplies this row's exact CAS publication.
            changes.retain(|change| {
                change.namespace != GENERIC_DATABASE_NAMESPACE || change.key != name
            });
            for related in generic::isolation::dependencies(&next, &name).map_err(host_error)? {
                if related != name
                    && !changes.iter().any(|change| {
                        change.namespace == GENERIC_DATABASE_NAMESPACE && change.key == related
                    })
                {
                    let image = next
                        .generic_databases
                        .get(&related)
                        .ok_or(RecoveryProblem::Conflict)?;
                    changes.push(
                        put_row_change(
                            GENERIC_DATABASE_NAMESPACE,
                            &related,
                            encode_object_row(&related, image).map_err(host_error)?,
                            &durable.versions,
                            self.limits.max_state_bytes,
                        )
                        .map_err(host_error)?,
                    );
                }
            }
        }
        let mut uow_mutations = changes
            .iter()
            .map(|change| change.mutation.clone())
            .collect::<Vec<_>>();
        if !transition.replayed() {
            uow_mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: SELECTION_NAMESPACE.into(),
                    key: selection.key,
                    version: selection
                        .version
                        .checked_add(1)
                        .ok_or(RecoveryProblem::LimitExceeded)?,
                    payload: selection.payload,
                },
                expected_version: Some(selection.version),
            }));
        }
        transition.publish_with_intent(
            &*self.store,
            effects,
            key,
            request_digest,
            uow_mutations,
        )?;
        for change in changes {
            let key = (change.namespace, change.key);
            match change.next_version {
                Some(version) => {
                    durable.versions.insert(key, version);
                }
                None => {
                    durable.versions.remove(&key);
                }
            }
        }
        durable.state = next;
        Ok(())
    }

    fn utility_binding(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
        intent: AccessIntent,
    ) -> Result<(DatabaseDefinition, ProviderStateRecord), RecoveryProblem> {
        let name = normalize(database);
        if let Some(authorizer) = &self.authorizer {
            let resource =
                EnterpriseResource::new(EnterpriseResourceClass::ImsDatabase, name.clone(), intent)
                    .map_err(host_error)?;
            authorizer
                .authorize(invocation.principal.id(), &resource)
                .map_err(host_error)?;
        }
        let selected = self
            .selected_metadata_generation(application)
            .map_err(host_error)?
            .ok_or(RecoveryProblem::NotFound)?;
        if selected.package_identity != package_identity {
            return Err(RecoveryProblem::Conflict);
        }
        if self.lock().map_err(host_error)?.state.metadata.as_ref() != Some(&selected.catalog) {
            return Err(RecoveryProblem::Conflict);
        }
        let metadata = selected
            .catalog
            .databases
            .iter()
            .find(|candidate| normalize(&candidate.name) == name)
            .ok_or(RecoveryProblem::NotFound)?;
        let definition = generic::definition(metadata).map_err(host_error)?;
        let selection = self
            .selected_metadata_selection_fence(&selected)
            .map_err(host_error)?;
        if selection.version == 0 || selection.payload.len() > self.limits.max_state_bytes {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok((definition, selection))
    }

    pub(crate) fn utility_image(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
        limits: RecoveryLimits,
        intent: AccessIntent,
    ) -> Result<(u64, UtilityImage), RecoveryProblem> {
        let (definition, _) =
            self.utility_binding(invocation, application, package_identity, database, intent)?;
        let mut durable = self.lock().map_err(host_error)?;
        generic::refresh_databases(&*self.store, self.limits, &mut durable).map_err(host_error)?;
        if durable.state.metadata.as_ref().is_none_or(|catalog| {
            !catalog.databases.iter().any(|candidate| {
                normalize(&candidate.name) == normalize(database)
                    && generic::definition(candidate).as_ref() == Ok(&definition)
            })
        }) {
            return Err(RecoveryProblem::Conflict);
        }
        let name = normalize(database);
        let version = *durable
            .versions
            .get(&(GENERIC_DATABASE_NAMESPACE.into(), name.clone()))
            .ok_or(RecoveryProblem::NotFound)?;
        let image = durable
            .state
            .generic_databases
            .get(&name)
            .ok_or(RecoveryProblem::NotFound)?;
        let engine =
            DatabaseEngine::restore((**image).clone(), generic::engine_limits(self.limits))
                .map_err(|_| RecoveryProblem::CorruptImage)?;
        let utility = UtilityImage::from_engine(&engine)?;
        utility.validate(limits)?;
        Ok((version, utility))
    }

    pub(crate) fn utility_raw_version(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
        definition: &DatabaseDefinition,
    ) -> Result<u64, RecoveryProblem> {
        let (bound, _) = self.utility_binding(
            invocation,
            application,
            package_identity,
            database,
            AccessIntent::Update,
        )?;
        if bound != *definition {
            return Err(RecoveryProblem::Conflict);
        }
        let row = self
            .store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, &normalize(database))
            .map_err(read_store_error)?
            .ok_or(RecoveryProblem::NotFound)?;
        if row.version == 0 {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(row.version)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish_utility_stage(
        &self,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        effects: &dyn IdempotencyStore,
        key: &IdempotencyKey,
        request_digest: [u8; 32],
        job: &str,
        database: &str,
        limits: RecoveryLimits,
    ) -> Result<UtilityPublishReceipt, RecoveryProblem> {
        let (definition, selection) = self.utility_binding(
            invocation,
            application,
            package_identity,
            database,
            AccessIntent::Update,
        )?;
        let stage = load_stage(&*self.store, job, limits)?;
        if normalize(database) != stage.plan.database {
            return Err(RecoveryProblem::Conflict);
        }
        if definition != stage.image.definition {
            return Err(RecoveryProblem::Conflict);
        }
        let effect = effects
            .effect(key)
            .map_err(read_store_error)?
            .ok_or(RecoveryProblem::InvalidRequest)?;
        if effect.request_digest != request_digest {
            return Err(RecoveryProblem::Conflict);
        }
        if effect.execution_id != invocation.execution_id
            || effect.run_unit_id != invocation.run_unit_id
        {
            return Err(RecoveryProblem::Conflict);
        }
        if effect.state == EffectState::UnknownOutcome {
            return Err(RecoveryProblem::UnknownOutcome);
        }
        let prior = self
            .store
            .get_provider_state(PUBLICATION_NAMESPACE, job)
            .map_err(read_store_error)?;
        if let Some(prior) = prior {
            let receipt = decode_publication(prior, limits)?;
            if receipt.job != job
                || receipt.application != normalize(application)
                || receipt.package_identity != package_identity
                || receipt.database != normalize(database)
                || receipt.image_digest != stage.image_digest
                || receipt.effect_key != key.as_str()
                || receipt.request_digest != request_digest
            {
                return Err(RecoveryProblem::Conflict);
            }
            return match effect.state {
                EffectState::Intent | EffectState::Completed => Ok(UtilityPublishReceipt {
                    generation: receipt.generation,
                    image_digest: receipt.image_digest,
                    replayed: true,
                }),
                _ => Err(RecoveryProblem::Conflict),
            };
        }
        if effect.state != EffectState::Intent {
            return Err(RecoveryProblem::Conflict);
        }
        let mut durable = self.lock().map_err(host_error)?;
        if durable.state.metadata.as_ref().is_none_or(|metadata| {
            !metadata.databases.iter().any(|candidate| {
                normalize(&candidate.name) == normalize(database)
                    && generic::definition(candidate).as_ref() == Ok(&definition)
            })
        }) {
            return Err(RecoveryProblem::Conflict);
        }
        let name = normalize(database);
        let raw = self
            .store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, &name)
            .map_err(read_store_error)?
            .ok_or(RecoveryProblem::NotFound)?;
        if raw.version == 0 {
            return Err(RecoveryProblem::CorruptImage);
        }
        if Some(raw.version) != stage.expected_active_version {
            return Err(RecoveryProblem::Conflict);
        }
        if stage.plan.kind != UtilityKind::DatabaseRecovery {
            generic::refresh_databases(&*self.store, self.limits, &mut durable)
                .map_err(host_error)?;
        } else {
            system::reservations::refresh(&*self.store, self.limits, &mut durable)
                .map_err(host_error)?;
            durable.versions.insert(
                (GENERIC_DATABASE_NAMESPACE.into(), name.clone()),
                raw.version,
            );
            generic::isolation::refresh_undo(&*self.store, self.limits, &mut durable)
                .map_err(host_error)?;
        }
        generic::isolation::ensure_no_pending(&durable.state, &name).map_err(host_error)?;
        let engine = stage.image.validate(limits)?;
        let image = engine.image();
        DatabaseEngine::restore(image.clone(), generic::engine_limits(self.limits))
            .map_err(|_| RecoveryProblem::LimitExceeded)?;
        let mut next = durable.state.scoped_snapshot();
        next.generic_databases.insert(name.clone(), Arc::new(image));
        generic::reset_positions(&mut next, &name, None);
        validate_state(&next, self.limits).map_err(host_error)?;
        durable.versions.insert(
            (GENERIC_DATABASE_NAMESPACE.into(), name.clone()),
            raw.version,
        );
        let changes = row_changes(&durable.state, &next, &durable.versions, self.limits, false)
            .map_err(host_error)?;
        let database_version = raw
            .version
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        let mut publication = Publication {
            schema_version: PUBLICATION_SCHEMA.into(),
            job: job.into(),
            application: normalize(application),
            package_identity: package_identity.into(),
            database: name,
            generation: database_version,
            image_digest: stage.image_digest,
            effect_key: key.as_str().into(),
            request_digest,
            receipt_digest: [0; 32],
        };
        publication.receipt_digest = publication.digest();
        let receipt_payload =
            serde_json::to_vec(&publication).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
        if receipt_payload.len() > limits.max_state_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let mut mutations = changes
            .iter()
            .map(|change| change.mutation.clone())
            .collect::<Vec<_>>();
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: PUBLICATION_NAMESPACE.into(),
                key: job.into(),
                version: 1,
                payload: receipt_payload,
            },
            expected_version: None,
        }));
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: SELECTION_NAMESPACE.into(),
                key: selection.key,
                version: selection
                    .version
                    .checked_add(1)
                    .ok_or(RecoveryProblem::LimitExceeded)?,
                payload: selection.payload,
            },
            expected_version: Some(selection.version),
        }));
        self.store
            .mutate_provider_states_atomic(mutations)
            .map_err(write_store_error)?;
        for change in changes {
            let key = (change.namespace, change.key);
            match change.next_version {
                Some(version) => {
                    durable.versions.insert(key, version);
                }
                None => {
                    durable.versions.remove(&key);
                }
            }
        }
        durable.state = next;
        Ok(UtilityPublishReceipt {
            generation: database_version,
            image_digest: stage.image_digest,
            replayed: false,
        })
    }
}

fn host_recovery_error(problem: RecoveryProblem) -> HostProblem {
    super::application_recovery::recovery_error(problem)
}

fn decode_publication(
    row: ProviderStateRecord,
    limits: RecoveryLimits,
) -> Result<Publication, RecoveryProblem> {
    if row.version != 1 || row.payload.len() > limits.max_state_bytes {
        return Err(RecoveryProblem::CorruptImage);
    }
    let value: Publication =
        serde_json::from_slice(&row.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
    if value.schema_version != PUBLICATION_SCHEMA
        || value.job != row.key
        || value.generation == 0
        || value.receipt_digest != value.digest()
    {
        return Err(RecoveryProblem::CorruptImage);
    }
    Ok(value)
}

fn host_error(problem: HostProblem) -> RecoveryProblem {
    match problem {
        HostProblem::Unauthorized => RecoveryProblem::Unauthorized,
        HostProblem::NotFound => RecoveryProblem::NotFound,
        HostProblem::ResourceExhausted => RecoveryProblem::LimitExceeded,
        HostProblem::Unsupported => RecoveryProblem::Unsupported,
        HostProblem::IdempotencyConflict => RecoveryProblem::Conflict,
        HostProblem::UnknownOutcome => RecoveryProblem::UnknownOutcome,
        _ => RecoveryProblem::InfrastructureFailure,
    }
}

fn read_store_error(_problem: StoreError) -> RecoveryProblem {
    RecoveryProblem::InfrastructureFailure
}

fn write_store_error(problem: StoreError) -> RecoveryProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists => RecoveryProblem::Conflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            RecoveryProblem::LimitExceeded
        }
        StoreError::Infrastructure(_) | StoreError::Poisoned => RecoveryProblem::UnknownOutcome,
        _ => RecoveryProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod isolation_tests;
    mod reservation_tests;
    use crate::recovery::{
        BackoutPointKind, CheckpointKind, CheckpointRequest, LogRequest, RecoveryContext,
        RecoverySession, UtilityDelta, UtilityDeltaChange, UtilityEngine, UtilityPlan,
        UtilityRecord,
    };
    use mainframe_env_execution_api::{AuditRecord, ExecutionId};
    use mainframe_env_execution_api::{CapabilityId, InvocationLimits};
    use mainframe_env_host_api::{ImsOperation, ImsQualifier};
    use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
    use mainframe_env_store_api::{
        AuditSink, EffectDigestFormat, EffectIntentMetadata, EffectRecord, IdempotencyStore,
    };
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::{AtomicU64, Ordering};

    const PACKAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    static NEXT_FILE: AtomicU64 = AtomicU64::new(1);
    static NEXT_READ: AtomicU64 = AtomicU64::new(1000);

    struct CommitThenLoseAck {
        inner: Arc<MemoryStore>,
        lose_ack: AtomicBool,
    }

    struct DenyDatabase;

    impl EnterpriseAuthorizer for DenyDatabase {
        fn authorize(
            &self,
            _principal: &mainframe_env_execution_api::PrincipalId,
            _resource: &EnterpriseResource,
        ) -> Result<(), HostProblem> {
            Err(HostProblem::Unauthorized)
        }
    }

    impl AuditSink for CommitThenLoseAck {
        fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
            self.inner.record_audit(record)
        }
        fn audit_records(
            &self,
            execution_id: &ExecutionId,
            start_effect_sequence: u64,
            max: usize,
        ) -> Result<Vec<AuditRecord>, StoreError> {
            self.inner
                .audit_records(execution_id, start_effect_sequence, max)
        }
    }

    impl ProviderStateStore for CommitThenLoseAck {
        fn get_provider_state(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<ProviderStateRecord>, StoreError> {
            self.inner.get_provider_state(namespace, key)
        }
        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, StoreError> {
            self.inner.list_provider_state(namespace, max)
        }
        fn put_provider_state(
            &self,
            record: ProviderStateRecord,
            expected_version: Option<u64>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_state(record, expected_version)
        }
        fn delete_provider_state(
            &self,
            namespace: &str,
            key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .delete_provider_state(namespace, key, expected_version)
        }
        fn move_provider_state(
            &self,
            record: ProviderStateRecord,
            old_key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .move_provider_state(record, old_key, expected_version)
        }
        fn put_provider_states_atomic(
            &self,
            writes: Vec<ProviderStateWrite>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_states_atomic(writes)
        }
        fn mutate_provider_states_atomic(
            &self,
            mutations: Vec<ProviderStateMutation>,
        ) -> Result<(), StoreError> {
            self.inner.mutate_provider_states_atomic(mutations)?;
            if self.lose_ack.swap(false, Ordering::SeqCst) {
                Err(StoreError::Infrastructure("acknowledgement lost".into()))
            } else {
                Ok(())
            }
        }
    }

    fn install(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let catalog = generic::tests::catalog();
        service.install_metadata(catalog.clone()).unwrap();
        service
            .publish_metadata_generation("APP", 1, PACKAGE, Some(&catalog))
            .unwrap();
        service
    }

    fn intent(
        effects: &dyn IdempotencyStore,
        run: &str,
        label: &str,
        digest: [u8; 32],
    ) -> IdempotencyKey {
        let ids = InvocationLimits::default();
        let invocation = generic::tests::invocation(run);
        let key = IdempotencyKey::new(label, ids).unwrap();
        effects
            .record_intent(EffectRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                sequence: 1,
                key: key.clone(),
                digest_format: EffectDigestFormat::CanonicalHostV1,
                request_digest: digest,
                intent: EffectIntentMetadata {
                    owner: invocation.execution_id,
                    attempt: 1,
                    capability: Some(CapabilityId::new("ims.recovery.utility", ids).unwrap()),
                    audit_resource: None,
                    audit_invocation_key: None,
                    created_tick: 5,
                    recovery_after_tick: 10,
                    epoch: 1,
                    recovery_lease: None,
                },
                state: EffectState::Intent,
                result_digest: None,
                resolved_tick: None,
            })
            .unwrap();
        key
    }

    fn loaded_image(service: &ImsService, run: &str) -> UtilityImage {
        let invocation = generic::tests::invocation(run);
        let mut image = UtilityEngine::extract(
            service,
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        image.records = vec![
            UtilityRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2Y".to_vec(),
            },
            UtilityRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            },
        ];
        image
    }

    fn load(service: &ImsService, effects: &dyn IdempotencyStore, run: &str) -> UtilityImage {
        let invocation = generic::tests::invocation(run);
        let image = loaded_image(service, run);
        let limits = RecoveryLimits::default();
        UtilityEngine::stage_initial_load(
            service,
            &invocation,
            "APP",
            PACKAGE,
            "load-1",
            UtilityPlan {
                kind: UtilityKind::InitialLoad,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: image.records.len(),
            },
            image.clone(),
            limits,
        )
        .unwrap();
        let key = intent(effects, run, "load-effect", [1; 32]);
        UtilityEngine::publish(
            service,
            &invocation,
            "APP",
            PACKAGE,
            effects,
            &key,
            [1; 32],
            "load-1",
            "GENDB",
            limits,
        )
        .unwrap();
        image
    }

    fn read_key(service: &ImsService, run: &str, key: &[u8]) -> ImsResult {
        let mut request = generic::tests::request(
            run,
            ImsOperation::GetUnique,
            NEXT_READ.fetch_add(1, Ordering::Relaxed),
            &["ROOT"],
            b"",
        );
        request.qualifiers.push(ImsQualifier {
            segment: "ROOT".into(),
            field: "ROOTKEY".into(),
            value: key.into(),
        });
        service
            .execute(&generic::tests::invocation(run), &request)
            .unwrap()
    }

    #[test]
    fn memory_publication_reorganization_replay_and_normal_route_share_one_image() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "BRIDGERUN";
        let invocation = generic::tests::invocation(run);
        service
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        let image = loaded_image(&service, run);
        // Fresh navigation advances the image CAS; stage against that version.
        assert_eq!(read_key(&service, run, b"A1").status, "GE");
        UtilityEngine::stage_initial_load(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "load-1",
            UtilityPlan {
                kind: UtilityKind::InitialLoad,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: 2,
            },
            image.clone(),
            RecoveryLimits::default(),
        )
        .unwrap();
        let key = intent(&*store, run, "load-effect", [1; 32]);
        let receipt = UtilityEngine::publish(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            &*store,
            &key,
            [1; 32],
            "load-1",
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        assert!(!receipt.replayed);
        let observed = read_key(&service, run, b"A1");
        assert_eq!(observed.status, "  ", "{observed:?}");
        assert_eq!(observed.segments[0].data, b"A1X");
        assert!(
            store
                .get_provider_state("ims-recovery-v1-db-active", "GENDB")
                .unwrap()
                .is_none()
        );
        let replay = UtilityEngine::publish(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            &*store,
            &key,
            [1; 32],
            "load-1",
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        assert!(replay.replayed);
        let plan = UtilityPlan {
            kind: UtilityKind::Reorganize,
            database: "GENDB".into(),
            expected_input_digest: image.digest(),
            expected_records: 2,
        };
        UtilityEngine::stage_reorganization(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "reorg-1",
            plan,
            RecoveryLimits::default(),
        )
        .unwrap();
        let key = intent(&*store, run, "reorg-effect", [2; 32]);
        UtilityEngine::publish(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            &*store,
            &key,
            [2; 32],
            "reorg-1",
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        let extracted = UtilityEngine::extract(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(extracted.records[0].data, b"A1X");
        drop(service);
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(read_key(&reopened, run, b"B2").segments[0].data, b"B2Y");
    }

    #[test]
    fn sqlite_reopen_stage_publish_and_route_visibility() {
        let path = std::env::temp_dir().join(format!(
            "ims-bridge-{}-{}.sqlite",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let run = "SQLBRIDGE";
        let invocation = generic::tests::invocation(run);
        {
            let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let service = install(store);
            let image = loaded_image(&service, run);
            UtilityEngine::stage_initial_load(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                "sqlite-load",
                UtilityPlan {
                    kind: UtilityKind::InitialLoad,
                    database: "GENDB".into(),
                    expected_input_digest: image.digest(),
                    expected_records: 2,
                },
                image,
                RecoveryLimits::default(),
            )
            .unwrap();
        }
        {
            let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let key = intent(&*store, run, "sqlite-effect", [3; 32]);
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &key,
                [3; 32],
                "sqlite-load",
                "GENDB",
                RecoveryLimits::default(),
            )
            .unwrap();
            service
                .execute(
                    &invocation,
                    &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
                )
                .unwrap();
            assert_eq!(read_key(&service, run, b"A1").segments[0].data, b"A1X");
        }
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let fresh_run = "SQLBRIDGE2";
        service
            .execute(
                &generic::tests::invocation(fresh_run),
                &generic::tests::request(fresh_run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        assert_eq!(
            read_key(&service, fresh_run, b"B2").segments[0].data,
            b"B2Y"
        );
        drop(service);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corruption_conflict_recovery_and_unknown_effect_fail_closed() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "RECOVERBRIDGE";
        let invocation = generic::tests::invocation(run);
        let base = load(&service, &*store, run);
        let delta = UtilityDelta::seal(
            1,
            base.digest(),
            UtilityDeltaChange::Insert(UtilityRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"C3Z".to_vec(),
            }),
        );
        let plan = UtilityPlan {
            kind: UtilityKind::DatabaseRecovery,
            database: "GENDB".into(),
            expected_input_digest: base.digest(),
            expected_records: 3,
        };
        let row = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: row.version + 1,
                    payload: b"corrupt".to_vec(),
                    ..row.clone()
                },
                Some(row.version),
            )
            .unwrap();
        assert_eq!(
            UtilityEngine::extract(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                RecoveryLimits::default()
            ),
            Err(RecoveryProblem::InfrastructureFailure),
        );
        let mut bad = delta.clone();
        bad.digest[0] ^= 1;
        assert_eq!(
            UtilityEngine::stage_database_recovery(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                "bad",
                plan.clone(),
                base.clone(),
                &[bad],
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::CorruptImage),
        );
        UtilityEngine::stage_database_recovery(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "recover",
            plan,
            base,
            &[delta],
            RecoveryLimits::default(),
        )
        .unwrap();
        let key = intent(&*store, run, "recover-effect", [4; 32]);
        let effect = store.effect(&key).unwrap().unwrap();
        store
            .record_result(
                &key,
                EffectRecord {
                    state: EffectState::UnknownOutcome,
                    result_digest: Some([8; 32]),
                    ..effect
                },
            )
            .unwrap();
        assert_eq!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &key,
                [4; 32],
                "recover",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::UnknownOutcome),
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap()
                .payload,
            b"corrupt",
        );
        let key = intent(&*store, run, "recover-effect-2", [5; 32]);
        UtilityEngine::publish(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            &*store,
            &key,
            [5; 32],
            "recover",
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        drop(service);
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        reopened
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        assert_eq!(read_key(&reopened, run, b"C3").segments[0].data, b"C3Z");
        let live = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        let now = UtilityEngine::extract(
            &reopened,
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        UtilityEngine::stage_reorganization(
            &reopened,
            &invocation,
            "APP",
            PACKAGE,
            "stale",
            UtilityPlan {
                kind: UtilityKind::Reorganize,
                database: "GENDB".into(),
                expected_input_digest: now.digest(),
                expected_records: 3,
            },
            RecoveryLimits::default(),
        )
        .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: live.version + 1,
                    ..live.clone()
                },
                Some(live.version),
            )
            .unwrap();
        let key = intent(&*store, run, "stale-effect", [6; 32]);
        assert_eq!(
            UtilityEngine::publish(
                &reopened,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &key,
                [6; 32],
                "stale",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::Conflict),
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap()
                .payload,
            live.payload,
        );
    }

    #[test]
    fn backout_proposal_restores_the_live_database_row() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "BACKOUTBRIDGE";
        let invocation = generic::tests::invocation(run);
        load(&service, &*store, run);
        service
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        let resource = service
            .recovery_database_resource(&invocation, "APP", PACKAGE, "GENDB")
            .unwrap();
        let session = RecoverySession::load(&*store, run, RecoveryLimits::default()).unwrap();
        let proposal = session.begin_uow(&*store, "begin", vec![resource]).unwrap();
        let key = intent(&*store, run, "begin-effect", [9; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                proposal,
                &*store,
                &key,
                [9; 32],
            )
            .unwrap();
        let session = RecoverySession::load(&*store, run, RecoveryLimits::default()).unwrap();
        let sets = session
            .sets(
                &*store,
                "sets",
                BackoutPointKind::Sets,
                Some(*b"SAVE"),
                vec![],
                false,
            )
            .unwrap();
        let key = intent(&*store, run, "sets-effect", [10; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                sets,
                &*store,
                &key,
                [10; 32],
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Insert, 2, &["ROOT"], b"C3Z"),
            )
            .unwrap();
        assert_eq!(read_key(&service, run, b"C3").segments[0].data, b"C3Z");
        let session = RecoverySession::load(&*store, run, RecoveryLimits::default()).unwrap();
        let rols = session.rols(&*store, "rols", *b"SAVE").unwrap();
        let key = intent(&*store, run, "rols-effect", [11; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                rols.transition,
                &*store,
                &key,
                [11; 32],
            )
            .unwrap();
        let observed = read_key(&service, run, b"C3");
        assert_eq!(observed.status, "GE", "{observed:?}");
        assert_eq!(read_key(&service, run, b"A1").segments[0].data, b"A1X");
    }

    #[test]
    fn committed_publication_with_lost_ack_is_unknown_then_fenced_replay() {
        let inner = Arc::new(MemoryStore::new(StoreLimits::default()));
        let wrapper = Arc::new(CommitThenLoseAck {
            inner: inner.clone(),
            lose_ack: AtomicBool::new(false),
        });
        let service = install(wrapper.clone());
        let run = "LOSTACK";
        let invocation = generic::tests::invocation(run);
        let image = loaded_image(&service, run);
        UtilityEngine::stage_initial_load(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "lost-load",
            UtilityPlan {
                kind: UtilityKind::InitialLoad,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: 2,
            },
            image,
            RecoveryLimits::default(),
        )
        .unwrap();
        let key = intent(&*inner, run, "lost-effect", [12; 32]);
        wrapper.lose_ack.store(true, Ordering::SeqCst);
        assert_eq!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*inner,
                &key,
                [12; 32],
                "lost-load",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::UnknownOutcome),
        );
        assert!(
            inner
                .get_provider_state(PUBLICATION_NAMESPACE, "lost-load")
                .unwrap()
                .is_some()
        );
        assert!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*inner,
                &key,
                [12; 32],
                "lost-load",
                "GENDB",
                RecoveryLimits::default(),
            )
            .unwrap()
            .replayed
        );
        drop(service);
        let reopened = ImsService::open(inner, ImsLimits::default()).unwrap();
        let fresh_run = "LOSTACK2";
        reopened
            .execute(
                &generic::tests::invocation(fresh_run),
                &generic::tests::request(fresh_run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        assert_eq!(
            read_key(&reopened, fresh_run, b"A1").segments[0].data,
            b"A1X"
        );
    }

    #[test]
    fn stage_corruption_and_package_mismatch_leave_live_image_untouched() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "STAGEFENCE";
        let invocation = generic::tests::invocation(run);
        let image = loaded_image(&service, run);
        let active = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        UtilityEngine::stage_initial_load(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "tampered-stage",
            UtilityPlan {
                kind: UtilityKind::InitialLoad,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: 2,
            },
            image.clone(),
            RecoveryLimits::default(),
        )
        .unwrap();
        let stage = store
            .get_provider_state("ims-recovery-v1-db-stage", "tampered-stage")
            .unwrap()
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: stage.version + 1,
                    payload: b"corrupt".to_vec(),
                    ..stage
                },
                Some(1),
            )
            .unwrap();
        let key = intent(&*store, run, "stage-effect", [13; 32]);
        assert_eq!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &key,
                [13; 32],
                "tampered-stage",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::CorruptImage),
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap(),
            active,
        );
        assert_eq!(
            UtilityEngine::stage_initial_load(
                &service,
                &invocation,
                "APP",
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "wrong-package",
                UtilityPlan {
                    kind: UtilityKind::InitialLoad,
                    database: "GENDB".into(),
                    expected_input_digest: image.digest(),
                    expected_records: 2,
                },
                image,
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::Conflict),
        );
        assert!(
            store
                .get_provider_state("ims-recovery-v1-db-stage", "wrong-package")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn selected_database_denial_precedes_utility_staging() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let permitted = install(store.clone());
        let image = loaded_image(&permitted, "DENIEDRUN");
        let denied = ImsService::open_authorized(
            store.clone(),
            ImsLimits::default(),
            Arc::new(DenyDatabase),
        )
        .unwrap();
        let invocation = generic::tests::invocation("DENIEDRUN");
        assert_eq!(
            UtilityEngine::stage_initial_load(
                &denied,
                &invocation,
                "APP",
                PACKAGE,
                "denied-load",
                UtilityPlan {
                    kind: UtilityKind::InitialLoad,
                    database: "GENDB".into(),
                    expected_input_digest: image.digest(),
                    expected_records: 2,
                },
                image,
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::Unauthorized),
        );
        assert!(
            store
                .get_provider_state("ims-recovery-v1-db-stage", "denied-load")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn checkpoint_and_log_proposals_bind_the_live_database_digest() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "CHECKBRIDGE";
        load(&service, &*store, run);
        let invocation = generic::tests::invocation(run);
        let image = UtilityEngine::extract(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        let session = RecoverySession::load(&*store, run, RecoveryLimits::default()).unwrap();
        let checkpoint = session
            .checkpoint(
                "checkpoint",
                CheckpointRequest {
                    id: "CHK00001".into(),
                    kind: CheckpointKind::Basic,
                    context: RecoveryContext::Batch,
                    prior_xrst: false,
                    user_areas: Vec::new(),
                    positions: Vec::new(),
                },
                image.digest(),
            )
            .unwrap();
        let key = intent(&*store, run, "checkpoint-effect", [14; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                checkpoint,
                &*store,
                &key,
                [14; 32],
            )
            .unwrap();
        let session = RecoverySession::load(&*store, run, RecoveryLimits::default()).unwrap();
        assert_eq!(session.checkpoint_count(), 1);
        let log = session
            .log(
                "log",
                LogRequest {
                    code: 0xa0,
                    data: b"bridge".to_vec(),
                },
            )
            .unwrap();
        let key = intent(&*store, run, "log-effect", [15; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                log,
                &*store,
                &key,
                [15; 32],
            )
            .unwrap();
        assert_eq!(
            RecoverySession::load(&*store, run, RecoveryLimits::default())
                .unwrap()
                .log_count(),
            1,
        );
    }

    #[test]
    fn foreign_effect_and_corrupt_publication_receipt_cannot_claim_replay() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let service = install(store.clone());
        let run = "RECEIPTFENCE";
        let invocation = generic::tests::invocation(run);
        let image = loaded_image(&service, run);
        UtilityEngine::stage_initial_load(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            "receipt-load",
            UtilityPlan {
                kind: UtilityKind::InitialLoad,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: 2,
            },
            image,
            RecoveryLimits::default(),
        )
        .unwrap();
        let foreign = intent(&*store, "OTHEREXEC", "foreign-effect", [16; 32]);
        assert_eq!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &foreign,
                [16; 32],
                "receipt-load",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::Conflict),
        );
        let key = intent(&*store, run, "receipt-effect", [17; 32]);
        UtilityEngine::publish(
            &service,
            &invocation,
            "APP",
            PACKAGE,
            &*store,
            &key,
            [17; 32],
            "receipt-load",
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        let active = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        let receipt = store
            .get_provider_state(PUBLICATION_NAMESPACE, "receipt-load")
            .unwrap()
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: receipt.version + 1,
                    payload: b"corrupt".to_vec(),
                    ..receipt
                },
                Some(1),
            )
            .unwrap();
        assert_eq!(
            UtilityEngine::publish(
                &service,
                &invocation,
                "APP",
                PACKAGE,
                &*store,
                &key,
                [17; 32],
                "receipt-load",
                "GENDB",
                RecoveryLimits::default(),
            ),
            Err(RecoveryProblem::CorruptImage),
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap(),
            active,
        );
    }
}
