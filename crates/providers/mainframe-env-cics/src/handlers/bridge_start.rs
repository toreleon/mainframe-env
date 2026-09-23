//! Durable, replayable admission for local START BREXIT work.
//!
//! Admission fixes the target and exit artifact before a worker can run. The
//! bridge callback ABI and terminal interception are separate execution work;
//! no public CICS command dispatch reaches this queue until they are bound.

use super::super::{CicsLimits, CicsService, store_error};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, InvocationLimits, PrincipalId, Selector,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState, WorkStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const NAMESPACE: &str = "cics-bridge-start-v1";
const SCHEMA: &str = "mainframe-env.cics.bridge-start@1";
const SELECTOR: &str = "cics:bridge-start";

/// Work generation reserved for local bridge task launches.
pub const CICS_BRIDGE_START_WORK_GENERATION: &str = "cics-bridge-start-v1";

/// Immutable START BREXIT request selected before worker dispatch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsBridgeStartIntent {
    schema: String,
    /// Stable private work identity; never exposed as EIBREQID.
    pub request_id: String,
    /// Local target transaction.
    pub transaction: String,
    /// User-written exit program.
    pub exit: String,
    /// Exact installed exit generation selected at admission.
    pub exit_artifact: String,
    /// Principal under which the started transaction runs.
    pub principal: String,
    /// Copied initial data addressed to the bridge exit.
    pub data: Vec<u8>,
    /// Priority fixed with the admitted request for replay.
    pub priority: u8,
    /// Durable tick at which the local request was admitted.
    pub admitted_tick: u64,
    /// Original effect identity and canonical request digest.
    pub producer_key: String,
    pub producer_digest: [u8; 32],
}

impl CicsService {
    /// Record a bounded bridge launch with exact replay after interrupted enqueue.
    ///
    /// This is kept outside CICS request dispatch until the BRXA callback and
    /// terminal interception can consume the resulting work generation.
    #[allow(dead_code, reason = "START BREXIT dispatch is the next bridge slice")]
    pub(in crate::service) fn schedule_bridge_start(
        &self,
        transaction: &str,
        explicit_exit: Option<&str>,
        issuer_principal: &str,
        user: Option<&str>,
        data: Option<&[u8]>,
        length: Option<usize>,
        producer_key: &str,
        producer_digest: [u8; 32],
        priority: u8,
    ) -> Result<CicsBridgeStartIntent, HostProblem> {
        IdempotencyKey::new(producer_key, InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        let data = match (data, length) {
            (None, None) => Vec::new(),
            (Some(bytes), Some(length)) if length > 0 && length <= bytes.len() => {
                bytes[..length].to_vec()
            }
            _ => return Err(length_error()),
        };
        if data.len() > self.limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let principal = user.unwrap_or(issuer_principal);
        PrincipalId::new(principal, InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        let selection = self.resolve_bridge_exit(transaction, explicit_exit)?;
        let tick = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?
            .now_tick()?;
        if tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let request_id = request_id(producer_key, &producer_digest);
        let intent = CicsBridgeStartIntent {
            schema: SCHEMA.into(),
            request_id,
            transaction: selection.transaction,
            exit: selection.exit,
            exit_artifact: selection.artifact.as_str().into(),
            principal: principal.into(),
            data,
            priority,
            admitted_tick: tick,
            producer_key: producer_key.into(),
            producer_digest,
        };
        let work_store = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let current = self
            .store
            .get_provider_state(NAMESPACE, &intent.request_id)
            .map_err(store_error)?;
        let admitted = if let Some(row) = current {
            replayed(&row, &intent, self.limits)?
        } else {
            check_capacity(self.store.as_ref(), self.limits, intent.data.len())?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: intent.request_id.clone(),
                    version: 1,
                    payload: encode(&intent, self.limits)?,
                },
                None,
            ) {
                Ok(()) => intent,
                Err(StoreError::AlreadyExists | StoreError::Conflict) => {
                    let row = self
                        .store
                        .get_provider_state(NAMESPACE, &intent.request_id)
                        .map_err(store_error)?
                        .ok_or(HostProblem::UnknownOutcome)?;
                    replayed(&row, &intent, self.limits)?
                }
                Err(problem) => return Err(store_error(problem)),
            }
        };
        enqueue(work_store.as_ref(), &admitted)?;
        Ok(admitted)
    }

    /// Repair an interrupted enqueue after opening the durable runtime.
    #[allow(dead_code, reason = "START BREXIT worker is the next bridge slice")]
    pub(in crate::service) fn recover_bridge_starts(&self) -> Result<(), HostProblem> {
        let work_store = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let rows = self
            .store
            .list_provider_state(NAMESPACE, self.limits.max_queue_records.saturating_add(1))
            .map_err(store_error)?;
        if rows.len() > self.limits.max_queue_records {
            return Err(HostProblem::ResourceExhausted);
        }
        for row in rows {
            let intent = decode(&row, self.limits)?;
            enqueue(work_store.as_ref(), &intent)?;
        }
        Ok(())
    }

    /// Decode one claimed bridge request without replacing its immutable intent.
    #[allow(dead_code, reason = "START BREXIT worker is the next bridge slice")]
    pub(in crate::service) fn promote_bridge_start(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<CicsBridgeStartIntent, HostProblem> {
        if work.required_generation != CICS_BRIDGE_START_WORK_GENERATION
            || work.required_selector.as_str() != SELECTOR
            || work.state != WorkState::Claimed
            || work.lease_id.is_none()
            || work.lease_epoch == 0
            || now_tick < work.available_tick
        {
            return Err(HostProblem::Malformed);
        }
        let request_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let row = self
            .store
            .get_provider_state(NAMESPACE, request_id)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let intent = decode(&row, self.limits)?;
        if !same_work(work, &work_record(&intent)?) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(intent)
    }
}

pub(in crate::service) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total = 0usize;
    for row in rows {
        let intent = decode(&row, limits)?;
        total = total
            .checked_add(intent.data.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if total > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(())
}

fn check_capacity(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
    incoming_bytes: usize,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() >= limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total = incoming_bytes;
    for row in rows {
        total = total
            .checked_add(decode(&row, limits)?.data.len())
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if total > limits.max_queue_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn enqueue(store: &dyn WorkStore, intent: &CicsBridgeStartIntent) -> Result<(), HostProblem> {
    let work = work_record(intent)?;
    match store.enqueue(work.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict)
            if store
                .get_work(&work.work_id)
                .map_err(store_error)?
                .as_ref()
                .is_some_and(|existing| same_work(existing, &work)) =>
        {
            Ok(())
        }
        Err(problem) => Err(store_error(problem)),
    }
}

fn work_record(intent: &CicsBridgeStartIntent) -> Result<WorkRecord, HostProblem> {
    let limits = InvocationLimits::default();
    let work = WorkRecord {
        work_id: format!("cics-bridge-start:{}", intent.request_id),
        execution_id: ExecutionId::new(format!("cics-bridge-{}", intent.request_id), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        required_selector: Selector::new(SELECTOR, limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        required_generation: CICS_BRIDGE_START_WORK_GENERATION.into(),
        artifact: ArtifactRef::new(&intent.exit_artifact, limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        state: WorkState::Queued,
        priority: intent.priority,
        attempt: 0,
        max_attempts: 3,
        available_tick: intent.admitted_tick,
        deadline_tick: intent
            .admitted_tick
            .checked_add(86_400_000)
            .ok_or(HostProblem::ResourceExhausted)?,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: intent.request_id.as_bytes().to_vec(),
    };
    Ok(work)
}

fn same_work(left: &WorkRecord, right: &WorkRecord) -> bool {
    left.work_id == right.work_id
        && left.execution_id == right.execution_id
        && left.required_selector == right.required_selector
        && left.required_generation == right.required_generation
        && left.artifact == right.artifact
        && left.priority == right.priority
        && left.max_attempts == right.max_attempts
        && left.available_tick == right.available_tick
        && left.deadline_tick == right.deadline_tick
        && left.payload == right.payload
}

fn replayed(
    row: &ProviderStateRecord,
    candidate: &CicsBridgeStartIntent,
    limits: CicsLimits,
) -> Result<CicsBridgeStartIntent, HostProblem> {
    let admitted = decode(row, limits)?;
    if admitted.request_id != candidate.request_id
        || admitted.transaction != candidate.transaction
        || admitted.exit != candidate.exit
        || admitted.exit_artifact != candidate.exit_artifact
        || admitted.principal != candidate.principal
        || admitted.data != candidate.data
        || admitted.priority != candidate.priority
        || admitted.producer_key != candidate.producer_key
        || admitted.producer_digest != candidate.producer_digest
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(admitted)
}

fn encode(intent: &CicsBridgeStartIntent, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    validate(intent, limits)?;
    serde_json::to_vec(intent).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(
    row: &ProviderStateRecord,
    limits: CicsLimits,
) -> Result<CicsBridgeStartIntent, HostProblem> {
    if row.namespace != NAMESPACE
        || row.version != 1
        || row.payload.len()
            > limits
                .max_queue_bytes
                .saturating_mul(5)
                .saturating_add(2048)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let intent: CicsBridgeStartIntent =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if row.key != intent.request_id
        || validate(&intent, limits).is_err()
        || encode(&intent, limits)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(intent)
}

fn validate(intent: &CicsBridgeStartIntent, limits: CicsLimits) -> Result<(), HostProblem> {
    if intent.schema != SCHEMA
        || intent.request_id != request_id(&intent.producer_key, &intent.producer_digest)
        || intent.transaction.is_empty()
        || intent.transaction.len() > 4
        || intent.exit.is_empty()
        || intent.exit.len() > 8
        || !intent
            .transaction
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        || !intent
            .exit
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        || intent.exit_artifact.len() != 71
        || !intent.exit_artifact.starts_with("sha256:")
        || !intent.exit_artifact[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || PrincipalId::new(&intent.principal, InvocationLimits::default()).is_err()
        || IdempotencyKey::new(&intent.producer_key, InvocationLimits::default()).is_err()
        || intent.data.len() > limits.max_queue_bytes
        || intent.admitted_tick == 0
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn request_id(producer_key: &str, producer_digest: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.cics.bridge-start-id@1\0");
    digest.update(producer_key.as_bytes());
    digest.update(producer_digest);
    format!("{:x}", digest.finalize())[..24].to_string()
}

fn length_error() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 0,
    }
}
