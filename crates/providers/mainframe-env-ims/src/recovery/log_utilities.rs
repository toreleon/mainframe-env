//! Typed bounded projection of DFSULTR0-style log recovery transitions.
//! This is not a parser for licensed OLDS/SLDS binary layouts.

use super::contracts::{RecoveryLimits, RecoveryProblem, UtilityKind, UtilityPlan};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const ACTIVE_NAMESPACE: &str = "ims-recovery-v1-log-active";
const STAGE_NAMESPACE: &str = "ims-recovery-v1-log-stage";
const ACTIVE_SCHEMA: &str = "mainframe-env.ims-recovered-log@1";
const STAGE_SCHEMA: &str = "mainframe-env.ims-recovered-log-stage@1";
const STREAM_DOMAIN: &str = "mainframe-env.ims-log-stream@1";
const BLOCK_DOMAIN: &str = "mainframe-env.ims-log-block@1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogDatasetKind {
    Online,
    Batch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogPayload {
    Data(Vec<u8>),
    PsbStart(String),
    PsbEnd(String),
    /// Interim error-ID marker: never a usable log record.
    Unreadable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogBlock {
    pub sequence: u64,
    pub declared_length: usize,
    pub payload: LogPayload,
    pub digest: [u8; 32],
}

impl LogBlock {
    pub fn seal(sequence: u64, payload: LogPayload) -> Self {
        let declared_length = payload_bytes(&payload).len();
        let digest = block_digest(sequence, declared_length, &payload);
        Self {
            sequence,
            declared_length,
            payload,
            digest,
        }
    }

    fn valid(&self, sequence: u64) -> bool {
        self.sequence == sequence
            && self.declared_length == payload_bytes(&self.payload).len()
            && self.digest == block_digest(self.sequence, self.declared_length, &self.payload)
            && !matches!(self.payload, LogPayload::Unreadable)
    }

    fn marker(&self, sequence: u64) -> bool {
        self.sequence == sequence
            && self.declared_length == payload_bytes(&self.payload).len()
            && self.digest == block_digest(self.sequence, self.declared_length, &self.payload)
            && matches!(self.payload, LogPayload::Unreadable)
    }
}

fn payload_bytes(payload: &LogPayload) -> Vec<u8> {
    serde_json::to_vec(payload).expect("typed log payload is JSON serializable")
}

fn block_digest(sequence: u64, length: usize, payload: &LogPayload) -> [u8; 32] {
    let material = serde_json::to_vec(&(BLOCK_DOMAIN, sequence, length, payload))
        .expect("typed log block is JSON serializable");
    Sha256::digest(material).into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogDataset {
    pub name: String,
    pub kind: LogDatasetKind,
    pub closed: bool,
    pub interim: bool,
    pub blocks: Vec<LogBlock>,
}

impl LogDataset {
    pub fn digest(&self) -> [u8; 32] {
        let material = serde_json::to_vec(&(
            STREAM_DOMAIN,
            &self.name,
            self.kind,
            self.closed,
            self.interim,
            &self.blocks,
        ))
        .expect("typed log stream is JSON serializable");
        Sha256::digest(material).into()
    }

    fn bounds(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if !valid_name(&self.name) {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.blocks.len() > limits.max_log_records {
            return Err(RecoveryProblem::LimitExceeded);
        }
        if serde_json::to_vec(self)
            .map_err(|_| RecoveryProblem::InvalidRequest)?
            .len()
            > limits.max_utility_bytes
        {
            return Err(RecoveryProblem::LimitExceeded);
        }
        Ok(())
    }

    fn validate_interim(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        self.bounds(limits)?;
        if !self.interim || self.closed {
            return Err(RecoveryProblem::CorruptImage);
        }
        for (index, block) in self.blocks.iter().enumerate() {
            let sequence = (index as u64) + 1;
            if !block.valid(sequence) && !block.marker(sequence) {
                return Err(RecoveryProblem::CorruptImage);
            }
        }
        Ok(())
    }

    pub fn validate_usable(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        self.bounds(limits)?;
        if self.interim
            || !self.closed
            || self
                .blocks
                .iter()
                .enumerate()
                .any(|(index, block)| !block.valid((index as u64) + 1))
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }

    fn validate_active(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.interim {
            self.validate_interim(limits)
        } else {
            self.validate_usable(limits)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogReplacement {
    pub sequence: u64,
    pub payload: LogPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ActiveLog {
    schema_version: String,
    name: String,
    generation: u64,
    last_job_id: String,
    stream: LogDataset,
    stream_digest: [u8; 32],
    row_digest: [u8; 32],
}

impl ActiveLog {
    fn digest(&self) -> [u8; 32] {
        row_digest(
            ACTIVE_SCHEMA,
            &(
                &self.schema_version,
                &self.name,
                self.generation,
                &self.last_job_id,
                &self.stream,
                self.stream_digest,
            ),
        )
    }
    fn verify(&self, name: &str, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.schema_version != ACTIVE_SCHEMA
            || self.name != name
            || self.generation == 0
            || self.stream.name != name
            || self.stream_digest != self.stream.digest()
            || self.row_digest != self.digest()
            || self.stream.validate_active(limits).is_err()
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StagedLog {
    schema_version: String,
    job_id: String,
    plan: UtilityPlan,
    expected_active_version: Option<u64>,
    generation: u64,
    stream: LogDataset,
    stream_digest: [u8; 32],
    row_digest: [u8; 32],
}

impl StagedLog {
    fn digest(&self) -> [u8; 32] {
        row_digest(
            STAGE_SCHEMA,
            &(
                &self.schema_version,
                &self.job_id,
                &self.plan,
                self.expected_active_version,
                self.generation,
                &self.stream,
                self.stream_digest,
            ),
        )
    }
    fn verify(&self, job: &str, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.schema_version != STAGE_SCHEMA
            || self.job_id != job
            || self.generation == 0
            || self.plan.kind != UtilityKind::LogRecovery
            || self.plan.database != self.stream.name
            || self.plan.expected_records != self.stream.blocks.len()
            || self.stream_digest != self.stream.digest()
            || self.row_digest != self.digest()
            || self.stream.validate_active(limits).is_err()
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }
}

fn row_digest<T: Serialize>(domain: &str, value: &T) -> [u8; 32] {
    let bytes = serde_json::to_vec(&(domain, value)).expect("typed log row is JSON serializable");
    Sha256::digest(bytes).into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogStageReceipt {
    pub stream_digest: [u8; 32],
    pub error_markers: usize,
    pub replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogPublishReceipt {
    pub generation: u64,
    pub stream_digest: [u8; 32],
    pub replayed: bool,
}

pub struct LogRecoveryEngine;

impl LogRecoveryEngine {
    /// DUP creates a bounded interim with error markers, or a closed output
    /// when every block is valid. An explicit LSN permits a safe truncation;
    /// otherwise a damaged stream requires a subsequent REP plan.
    pub fn stage_duplicate(
        store: &dyn ProviderStateStore,
        job: &str,
        plan: UtilityPlan,
        input: LogDataset,
        replacement_planned: bool,
        error_lsn: Option<u64>,
        limits: RecoveryLimits,
    ) -> Result<LogStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::LogRecovery
            || plan.database != input.name
            || plan.expected_input_digest != input.digest()
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        input.bounds(limits)?;
        if error_lsn == Some(0) {
            return Err(RecoveryProblem::Unsupported);
        }
        let mut output = input.clone();
        output.blocks.clear();
        let mut markers = 0;
        for (index, block) in input.blocks.into_iter().enumerate() {
            let sequence = (index as u64) + 1;
            if error_lsn.is_some_and(|lsn| sequence >= lsn) {
                break;
            }
            if block.valid(sequence) {
                output.blocks.push(block);
            } else {
                output
                    .blocks
                    .push(LogBlock::seal(sequence, LogPayload::Unreadable));
                markers += 1;
            }
        }
        output.interim = markers > 0;
        output.closed = !output.interim;
        if markers > 0 && !replacement_planned && error_lsn.is_none() {
            return Err(RecoveryProblem::Unsupported);
        }
        if output.blocks.len() != plan.expected_records {
            return Err(RecoveryProblem::InvalidRequest);
        }
        output.validate_active(limits)?;
        stage_log(
            store,
            job,
            plan,
            output,
            limits,
            raw_active_version(store, &input.name)?,
            markers,
        )
    }

    pub fn stage_replace(
        store: &dyn ProviderStateStore,
        job: &str,
        plan: UtilityPlan,
        replacements: Vec<LogReplacement>,
        limits: RecoveryLimits,
    ) -> Result<LogStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::LogRecovery {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let (version, active) =
            load_active(store, &plan.database, limits)?.ok_or(RecoveryProblem::NotFound)?;
        if active.stream_digest != plan.expected_input_digest || !active.stream.interim {
            return Err(RecoveryProblem::Conflict);
        }
        let mut supplied = BTreeMap::new();
        for replacement in replacements {
            if replacement.sequence == 0
                || matches!(replacement.payload, LogPayload::Unreadable)
                || supplied
                    .insert(replacement.sequence, replacement.payload)
                    .is_some()
            {
                return Err(RecoveryProblem::InvalidRequest);
            }
        }
        let mut output = active.stream;
        for block in &mut output.blocks {
            if block.marker(block.sequence) {
                let payload = supplied
                    .remove(&block.sequence)
                    .ok_or(RecoveryProblem::InvalidRequest)?;
                *block = LogBlock::seal(block.sequence, payload);
            }
        }
        if !supplied.is_empty() {
            return Err(RecoveryProblem::InvalidRequest);
        }
        output.interim = false;
        output.closed = true;
        output.validate_usable(limits)?;
        stage_log(store, job, plan, output, limits, Some(version), 0)
    }

    /// CLS accepts a verified external OLDS/WADS projection and stages a
    /// closed output. It does not silently repair damaged blocks.
    pub fn stage_close(
        store: &dyn ProviderStateStore,
        job: &str,
        plan: UtilityPlan,
        input: LogDataset,
        limits: RecoveryLimits,
    ) -> Result<LogStageReceipt, RecoveryProblem> {
        if plan.kind != UtilityKind::LogRecovery {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if input.name != plan.database
            || input.kind != LogDatasetKind::Online
            || input.closed
            || input.interim
            || input.digest() != plan.expected_input_digest
        {
            return Err(RecoveryProblem::Unsupported);
        }
        input.bounds(limits)?;
        if input
            .blocks
            .iter()
            .enumerate()
            .any(|(index, block)| !block.valid((index as u64) + 1))
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        let mut output = input;
        output.closed = true;
        let version = raw_active_version(store, &plan.database)?;
        stage_log(store, job, plan, output, limits, version, 0)
    }

    /// PSB mode is a bounded read-only report over explicit PSB markers.
    pub fn active_psbs(
        store: &dyn ProviderStateStore,
        name: &str,
        limits: RecoveryLimits,
    ) -> Result<Vec<String>, RecoveryProblem> {
        let (_, active) = load_active(store, name, limits)?.ok_or(RecoveryProblem::NotFound)?;
        if active.stream.interim {
            return Err(RecoveryProblem::Unsupported);
        }
        let mut names = BTreeSet::new();
        for block in &active.stream.blocks {
            match &block.payload {
                LogPayload::PsbStart(name) if valid_name(name) => {
                    names.insert(name.clone());
                }
                LogPayload::PsbEnd(name) if valid_name(name) => {
                    names.remove(name);
                }
                LogPayload::PsbStart(_) | LogPayload::PsbEnd(_) => {
                    return Err(RecoveryProblem::CorruptImage);
                }
                _ => {}
            }
        }
        Ok(names.into_iter().collect())
    }

    pub fn active(
        store: &dyn ProviderStateStore,
        name: &str,
        limits: RecoveryLimits,
    ) -> Result<LogDataset, RecoveryProblem> {
        load_active(store, name, limits)?
            .map(|(_, row)| row.stream)
            .ok_or(RecoveryProblem::NotFound)
    }

    pub fn publish(
        store: &dyn ProviderStateStore,
        job: &str,
        name: &str,
        limits: RecoveryLimits,
    ) -> Result<LogPublishReceipt, RecoveryProblem> {
        valid_job(job)?;
        let stage = store
            .get_provider_state(STAGE_NAMESPACE, job)
            .map_err(read_error)?;
        let Some(stage) = stage else {
            let (_, active) = load_active(store, name, limits)?.ok_or(RecoveryProblem::NotFound)?;
            if active.last_job_id != job {
                return Err(RecoveryProblem::NotFound);
            }
            return Ok(LogPublishReceipt {
                generation: active.generation,
                stream_digest: active.stream_digest,
                replayed: true,
            });
        };
        if stage.version != 1 || stage.payload.len() > limits.max_state_bytes {
            return Err(RecoveryProblem::CorruptImage);
        }
        let staged: StagedLog =
            serde_json::from_slice(&stage.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
        staged.verify(job, limits)?;
        if staged.plan.database != name
            || raw_active_version(store, name)? != staged.expected_active_version
        {
            return Err(RecoveryProblem::Conflict);
        }
        let mut active = ActiveLog {
            schema_version: ACTIVE_SCHEMA.into(),
            name: name.into(),
            generation: staged.generation,
            last_job_id: job.into(),
            stream: staged.stream,
            stream_digest: staged.stream_digest,
            row_digest: [0; 32],
        };
        active.row_digest = active.digest();
        let payload =
            serde_json::to_vec(&active).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
        if payload.len() > limits.max_state_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        store
            .mutate_provider_states_atomic(vec![
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: ACTIVE_NAMESPACE.into(),
                        key: name.into(),
                        version: staged.generation,
                        payload,
                    },
                    expected_version: staged.expected_active_version,
                }),
                ProviderStateMutation::Delete {
                    namespace: STAGE_NAMESPACE.into(),
                    key: job.into(),
                    expected_version: stage.version,
                },
            ])
            .map_err(write_error)?;
        Ok(LogPublishReceipt {
            generation: staged.generation,
            stream_digest: active.stream_digest,
            replayed: false,
        })
    }
}

fn stage_log(
    store: &dyn ProviderStateStore,
    job: &str,
    plan: UtilityPlan,
    stream: LogDataset,
    limits: RecoveryLimits,
    expected_version: Option<u64>,
    markers: usize,
) -> Result<LogStageReceipt, RecoveryProblem> {
    valid_job(job)?;
    plan.validate(limits)?;
    if plan.kind != UtilityKind::LogRecovery
        || plan.database != stream.name
        || plan.expected_records != stream.blocks.len()
    {
        return Err(RecoveryProblem::InvalidRequest);
    }
    stream.validate_active(limits)?;
    if raw_active_version(store, &stream.name)? != expected_version {
        return Err(RecoveryProblem::Conflict);
    }
    let generation = expected_version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(RecoveryProblem::LimitExceeded)?;
    let stream_digest = stream.digest();
    let mut staged = StagedLog {
        schema_version: STAGE_SCHEMA.into(),
        job_id: job.into(),
        plan,
        expected_active_version: expected_version,
        generation,
        stream,
        stream_digest,
        row_digest: [0; 32],
    };
    staged.row_digest = staged.digest();
    let payload =
        serde_json::to_vec(&staged).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
    if payload.len() > limits.max_state_bytes {
        return Err(RecoveryProblem::LimitExceeded);
    }
    if let Some(row) = store
        .get_provider_state(STAGE_NAMESPACE, job)
        .map_err(read_error)?
    {
        if row.version != 1 {
            return Err(RecoveryProblem::CorruptImage);
        }
        let old: StagedLog =
            serde_json::from_slice(&row.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
        old.verify(job, limits)?;
        if old != staged {
            return Err(RecoveryProblem::Conflict);
        }
        return Ok(LogStageReceipt {
            stream_digest,
            error_markers: markers,
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
    Ok(LogStageReceipt {
        stream_digest,
        error_markers: markers,
        replayed: false,
    })
}

fn load_active(
    store: &dyn ProviderStateStore,
    name: &str,
    limits: RecoveryLimits,
) -> Result<Option<(u64, ActiveLog)>, RecoveryProblem> {
    store
        .get_provider_state(ACTIVE_NAMESPACE, name)
        .map_err(read_error)?
        .map(|row| {
            if row.version == 0 || row.payload.len() > limits.max_state_bytes {
                return Err(RecoveryProblem::CorruptImage);
            }
            let active: ActiveLog =
                serde_json::from_slice(&row.payload).map_err(|_| RecoveryProblem::CorruptImage)?;
            active.verify(name, limits)?;
            if active.generation != row.version {
                return Err(RecoveryProblem::CorruptImage);
            }
            Ok((row.version, active))
        })
        .transpose()
}

fn raw_active_version(
    store: &dyn ProviderStateStore,
    name: &str,
) -> Result<Option<u64>, RecoveryProblem> {
    store
        .get_provider_state(ACTIVE_NAMESPACE, name)
        .map_err(read_error)?
        .map(|row| {
            if row.version == 0 {
                Err(RecoveryProblem::CorruptImage)
            } else {
                Ok(row.version)
            }
        })
        .transpose()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
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
