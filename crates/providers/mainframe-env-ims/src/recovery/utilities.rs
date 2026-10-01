//! Bounded database utility transitions over the shared provider-state CAS
//! authority and the existing deterministic IMS database engine.

use super::contracts::{RecoveryLimits, RecoveryProblem, UtilityKind, UtilityPlan};
use crate::ImsService;
use crate::database::{
    DatabaseDefinition, DatabaseEngine, DatabaseOrganization, EngineLimits, EngineProblem,
    InsertRequest, RecordId,
};
use mainframe_env_execution_api::{IdempotencyKey, Invocation};
use mainframe_env_host_api::AccessIntent;
use mainframe_env_store_api::{
    IdempotencyStore, ProviderStateRecord, ProviderStateStore, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const STAGE_NAMESPACE: &str = "ims-recovery-v1-db-stage";
const STAGE_SCHEMA: &str = "mainframe-env.ims-utility-stage@1";
const IMAGE_DOMAIN: &str = "mainframe-env.ims-utility-image@1";
const DELTA_DOMAIN: &str = "mainframe-env.ims-utility-delta@1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UtilityRecord {
    pub segment: String,
    /// Zero-based parent ordinal in the same topologically ordered image.
    pub parent: Option<usize>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UtilityImage {
    pub definition: DatabaseDefinition,
    pub records: Vec<UtilityRecord>,
}

impl UtilityImage {
    pub fn digest(&self) -> [u8; 32] {
        let material = serde_json::to_vec(&(IMAGE_DOMAIN, &self.definition, &self.records))
            .expect("utility image has a JSON representation");
        Sha256::digest(material).into()
    }

    pub fn validate(&self, limits: RecoveryLimits) -> Result<DatabaseEngine, RecoveryProblem> {
        if self.records.len() > limits.max_utility_records {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| RecoveryProblem::InvalidRequest)?;
        if bytes.len() > limits.max_utility_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let mut engine_limits = EngineLimits::default();
        engine_limits.max_records = engine_limits.max_records.min(limits.max_utility_records);
        let mut engine =
            DatabaseEngine::new(self.definition.clone(), engine_limits).map_err(engine_problem)?;
        let mut ids = Vec::with_capacity(self.records.len());
        for (index, record) in self.records.iter().enumerate() {
            let parent = match record.parent {
                Some(parent) if parent < index => Some(ids[parent]),
                Some(_) => return Err(RecoveryProblem::InvalidRequest),
                None => None,
            };
            let view = engine
                .insert(InsertRequest {
                    segment: record.segment.clone(),
                    parent,
                    data: record.data.clone(),
                })
                .map_err(engine_problem)?;
            ids.push(view.id);
        }
        Ok(engine)
    }

    pub fn from_engine(engine: &DatabaseEngine) -> Result<Self, RecoveryProblem> {
        let views = engine.export_records();
        let ordinals = views
            .iter()
            .enumerate()
            .map(|(index, view)| (view.id, index))
            .collect::<BTreeMap<RecordId, usize>>();
        let records = views
            .into_iter()
            .map(|view| {
                Ok(UtilityRecord {
                    segment: view.segment,
                    parent: view
                        .parent
                        .map(|id| {
                            ordinals
                                .get(&id)
                                .copied()
                                .ok_or(RecoveryProblem::CorruptImage)
                        })
                        .transpose()?,
                    data: view.data,
                })
            })
            .collect::<Result<Vec<_>, RecoveryProblem>>()?;
        Ok(Self {
            definition: engine.definition().clone(),
            records,
        })
    }

    pub(crate) fn canonicalized(&self, limits: RecoveryLimits) -> Result<Self, RecoveryProblem> {
        self.validate(limits)?;
        let mut children = vec![Vec::new(); self.records.len()];
        let mut roots = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            match record.parent {
                Some(parent) => children[parent].push(index),
                None => roots.push(index),
            }
        }
        let sort = |indices: &mut Vec<usize>| {
            indices.sort_by(|left, right| {
                (
                    &self.records[*left].segment,
                    &self.records[*left].data,
                    left,
                )
                    .cmp(&(
                        &self.records[*right].segment,
                        &self.records[*right].data,
                        right,
                    ))
            })
        };
        sort(&mut roots);
        for group in &mut children {
            sort(group);
        }
        let mut stack = roots
            .into_iter()
            .rev()
            .map(|index| (index, None))
            .collect::<Vec<_>>();
        let mut records = Vec::with_capacity(self.records.len());
        while let Some((old, parent)) = stack.pop() {
            let new_index = records.len();
            let source = &self.records[old];
            records.push(UtilityRecord {
                segment: source.segment.clone(),
                parent,
                data: source.data.clone(),
            });
            for child in children[old].iter().rev() {
                stack.push((*child, Some(new_index)));
            }
        }
        let image = Self {
            definition: self.definition.clone(),
            records,
        };
        image.validate(limits)?;
        Ok(image)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UtilityDeltaChange {
    Insert(UtilityRecord),
    Replace { ordinal: usize, data: Vec<u8> },
    Delete { ordinal: usize },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UtilityDelta {
    pub sequence: u64,
    pub previous_digest: [u8; 32],
    pub change: UtilityDeltaChange,
    pub digest: [u8; 32],
}

impl UtilityDelta {
    pub fn seal(sequence: u64, previous_digest: [u8; 32], change: UtilityDeltaChange) -> Self {
        let digest = delta_digest(sequence, previous_digest, &change);
        Self {
            sequence,
            previous_digest,
            change,
            digest,
        }
    }

    pub(crate) fn verify(
        &self,
        sequence: u64,
        previous_digest: [u8; 32],
    ) -> Result<(), RecoveryProblem> {
        if self.sequence != sequence
            || self.previous_digest != previous_digest
            || self.digest != delta_digest(sequence, previous_digest, &self.change)
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }
}

fn delta_digest(sequence: u64, previous: [u8; 32], change: &UtilityDeltaChange) -> [u8; 32] {
    let bytes = serde_json::to_vec(&(DELTA_DOMAIN, sequence, previous, change))
        .expect("bounded typed log change is JSON serializable");
    Sha256::digest(bytes).into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StagedDatabase {
    schema_version: String,
    job_id: String,
    pub(crate) plan: UtilityPlan,
    pub(crate) expected_active_version: Option<u64>,
    pub(crate) generation: u64,
    pub(crate) image: UtilityImage,
    pub(crate) image_digest: [u8; 32],
    row_digest: [u8; 32],
}

impl StagedDatabase {
    fn seal(&mut self) {
        self.row_digest = row_digest(
            STAGE_SCHEMA,
            &(
                &self.schema_version,
                &self.job_id,
                &self.plan,
                self.expected_active_version,
                self.generation,
                &self.image,
                self.image_digest,
            ),
        );
    }
    pub(crate) fn verify(&self, job: &str, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.schema_version != STAGE_SCHEMA
            || self.job_id != job
            || self.generation == 0
            || self.plan.validate(limits).is_err()
            || self.plan.database != self.image.definition.name
            || self.image.digest() != self.image_digest
            || self.image.records.len() != self.plan.expected_records
            || self.row_digest
                != row_digest(
                    STAGE_SCHEMA,
                    &(
                        &self.schema_version,
                        &self.job_id,
                        &self.plan,
                        self.expected_active_version,
                        self.generation,
                        &self.image,
                        self.image_digest,
                    ),
                )
            || self.image.validate(limits).is_err()
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }
}

fn row_digest<T: Serialize>(domain: &str, value: &T) -> [u8; 32] {
    let bytes =
        serde_json::to_vec(&(domain, value)).expect("bounded utility row is JSON serializable");
    Sha256::digest(bytes).into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UtilityStageReceipt {
    pub job_id: String,
    pub image_digest: [u8; 32],
    pub records: usize,
    pub replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UtilityPublishReceipt {
    pub generation: u64,
    pub image_digest: [u8; 32],
    pub replayed: bool,
}

pub struct UtilityEngine;

impl UtilityEngine {
    pub fn stage_initial_load(
        service: &ImsService,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        job: &str,
        plan: UtilityPlan,
        image: UtilityImage,
        limits: RecoveryLimits,
    ) -> Result<UtilityStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::InitialLoad || plan.expected_input_digest != image.digest() {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let (version, active) = service.utility_image(
            invocation,
            application,
            package_identity,
            &plan.database,
            limits,
            AccessIntent::Update,
        )?;
        if !active.records.is_empty() || active.definition != image.definition {
            return Err(RecoveryProblem::Conflict);
        }
        stage_image(&*service.store, job, plan, image, limits, Some(version))
    }

    pub fn extract(
        service: &ImsService,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        database: &str,
        limits: RecoveryLimits,
    ) -> Result<UtilityImage, RecoveryProblem> {
        service
            .utility_image(
                invocation,
                application,
                package_identity,
                database,
                limits,
                AccessIntent::Read,
            )
            .map(|(_, image)| image)
    }

    pub fn stage_reorganization(
        service: &ImsService,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        job: &str,
        plan: UtilityPlan,
        limits: RecoveryLimits,
    ) -> Result<UtilityStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::Reorganize {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let (version, active) = service.utility_image(
            invocation,
            application,
            package_identity,
            &plan.database,
            limits,
            AccessIntent::Update,
        )?;
        if active.digest() != plan.expected_input_digest {
            return Err(RecoveryProblem::Conflict);
        }
        if active.definition.organization == DatabaseOrganization::Gsam {
            return Err(RecoveryProblem::Unsupported);
        }
        let reorganized = active.canonicalized(limits)?;
        stage_image(
            &*service.store,
            job,
            plan,
            reorganized,
            limits,
            Some(version),
        )
    }

    /// Rebuild one data set from a verified image copy and contiguous typed
    /// update log. Damaged active bytes may be replaced only at their exact
    /// shared-store version; no application-logic repair is inferred.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_database_recovery(
        service: &ImsService,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        job: &str,
        plan: UtilityPlan,
        mut image_copy: UtilityImage,
        logs: &[UtilityDelta],
        limits: RecoveryLimits,
    ) -> Result<UtilityStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::DatabaseRecovery
            || image_copy.digest() != plan.expected_input_digest
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if image_copy.definition.organization == DatabaseOrganization::Gsam {
            return Err(RecoveryProblem::Unsupported);
        }
        image_copy.validate(limits)?;
        if logs.len() > limits.max_log_records {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let mut previous = image_copy.digest();
        for (index, log) in logs.iter().enumerate() {
            log.verify((index as u64) + 1, previous)?;
            apply_delta(&mut image_copy, &log.change)?;
            image_copy.validate(limits)?;
            previous = log.digest;
        }
        let expected_version = service.utility_raw_version(
            invocation,
            application,
            package_identity,
            &plan.database,
            &image_copy.definition,
        )?;
        stage_image(
            &*service.store,
            job,
            plan,
            image_copy,
            limits,
            Some(expected_version),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        service: &ImsService,
        invocation: &Invocation,
        application: &str,
        package_identity: &str,
        effects: &dyn IdempotencyStore,
        key: &IdempotencyKey,
        canonical_request_digest: [u8; 32],
        job: &str,
        database: &str,
        limits: RecoveryLimits,
    ) -> Result<UtilityPublishReceipt, RecoveryProblem> {
        service.publish_utility_stage(
            invocation,
            application,
            package_identity,
            effects,
            key,
            canonical_request_digest,
            job,
            database,
            limits,
        )
    }
}

fn stage_image(
    store: &dyn ProviderStateStore,
    job: &str,
    plan: UtilityPlan,
    image: UtilityImage,
    limits: RecoveryLimits,
    expected_version: Option<u64>,
) -> Result<UtilityStageReceipt, RecoveryProblem> {
    valid_job(job)?;
    plan.validate(limits)?;
    if plan.database != image.definition.name || plan.expected_records != image.records.len() {
        return Err(RecoveryProblem::InvalidRequest);
    }
    image.validate(limits)?;
    let generation = expected_version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(RecoveryProblem::LimitExceeded)?;
    let image_digest = image.digest();
    let mut stage = StagedDatabase {
        schema_version: STAGE_SCHEMA.into(),
        job_id: job.into(),
        plan,
        expected_active_version: expected_version,
        generation,
        image,
        image_digest,
        row_digest: [0; 32],
    };
    stage.seal();
    let payload = serde_json::to_vec(&stage).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
    if payload.len() > limits.max_state_bytes {
        return Err(RecoveryProblem::LimitExceeded);
    }
    if let Some(previous) = store
        .get_provider_state(STAGE_NAMESPACE, job)
        .map_err(read_error)?
    {
        if previous.version != 1 {
            return Err(RecoveryProblem::CorruptImage);
        }
        let old: StagedDatabase =
            serde_json::from_slice(&previous.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
        old.verify(job, limits)?;
        if old != stage {
            return Err(RecoveryProblem::Conflict);
        }
        return Ok(UtilityStageReceipt {
            job_id: job.into(),
            image_digest,
            records: stage.image.records.len(),
            replayed: true,
        });
    }
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: STAGE_NAMESPACE.into(),
                key: job.into(),
                version: 1,
                payload,
            },
            None,
        )
        .map_err(write_error)?;
    Ok(UtilityStageReceipt {
        job_id: job.into(),
        image_digest,
        records: stage.image.records.len(),
        replayed: false,
    })
}

pub(crate) fn apply_delta(
    image: &mut UtilityImage,
    change: &UtilityDeltaChange,
) -> Result<(), RecoveryProblem> {
    match change {
        UtilityDeltaChange::Insert(record) => {
            if record
                .parent
                .is_some_and(|parent| parent >= image.records.len())
            {
                return Err(RecoveryProblem::InvalidRequest);
            }
            image.records.push(record.clone());
        }
        UtilityDeltaChange::Replace { ordinal, data } => {
            let record = image
                .records
                .get_mut(*ordinal)
                .ok_or(RecoveryProblem::InvalidRequest)?;
            record.data = data.clone();
        }
        UtilityDeltaChange::Delete { ordinal } => {
            if *ordinal >= image.records.len()
                || image
                    .records
                    .iter()
                    .any(|record| record.parent == Some(*ordinal))
            {
                return Err(RecoveryProblem::InvalidRequest);
            }
            image.records.remove(*ordinal);
            for record in &mut image.records {
                if let Some(parent) = &mut record.parent
                    && *parent > *ordinal
                {
                    *parent -= 1;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn load_stage(
    store: &dyn ProviderStateStore,
    job: &str,
    limits: RecoveryLimits,
) -> Result<StagedDatabase, RecoveryProblem> {
    valid_job(job)?;
    let row = store
        .get_provider_state(STAGE_NAMESPACE, job)
        .map_err(read_error)?
        .ok_or(RecoveryProblem::NotFound)?;
    if row.version != 1 || row.payload.len() > limits.max_state_bytes {
        return Err(RecoveryProblem::CorruptImage);
    }
    let stage: StagedDatabase =
        serde_json::from_slice(&row.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
    stage.verify(job, limits)?;
    Ok(stage)
}

fn valid_job(job: &str) -> Result<(), RecoveryProblem> {
    if job.is_empty()
        || job.len() > 128
        || !job
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(RecoveryProblem::InvalidRequest);
    }
    Ok(())
}

fn engine_problem(problem: EngineProblem) -> RecoveryProblem {
    match problem {
        EngineProblem::LimitExceeded => RecoveryProblem::LimitExceeded,
        EngineProblem::Unsupported => RecoveryProblem::Unsupported,
        _ => RecoveryProblem::InvalidRequest,
    }
}

fn read_error(_error: StoreError) -> RecoveryProblem {
    RecoveryProblem::InfrastructureFailure
}
fn write_error(error: StoreError) -> RecoveryProblem {
    match error {
        StoreError::Conflict | StoreError::AlreadyExists => RecoveryProblem::Conflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            RecoveryProblem::LimitExceeded
        }
        StoreError::Infrastructure(_) | StoreError::Poisoned => RecoveryProblem::UnknownOutcome,
        _ => RecoveryProblem::InfrastructureFailure,
    }
}
