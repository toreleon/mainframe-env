//! Durable local child-task admission for RUN TRANSID.

use super::*;
use crate::service::{CicsBtsChildCompletion, CicsService};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, InvocationLimits, RunUnitId, Selector,
};
use mainframe_env_store_api::{WorkRecord, WorkState, WorkStore};

const RUN_NAMESPACE: &str = "cics-bts-transid-run-v1";
const OUTBOX_NAMESPACE: &str = "cics-bts-transid-outbox-v1";
const RUN_SCHEMA: &str = "mainframe-env.cics.bts-transid-run@1";
const OUTBOX_SCHEMA: &str = "mainframe-env.cics.bts-transid-outbox@1";
const MAX_PENDING: usize = 4096;
const MAX_RUN_BYTES: usize = 1_048_576;
const LIFETIME_TICKS: u64 = 86_400_000;

/// Work generation for local child tasks issued by RUN TRANSID.
pub const BTS_TRANSID_WORK_GENERATION: &str = "cics-bts-transid-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BtsTransidState {
    Pending,
    Attached,
    Finished,
}

/// Exact container bytes at the point where RUN TRANSID was issued.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsTransidContainer {
    pub character: bool,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsTransidRecord {
    pub schema_version: String,
    pub run_id: String,
    pub work_id: String,
    pub parent_run_unit: String,
    pub parent_execution: String,
    pub parent_principal: String,
    pub effect_key: String,
    pub request_digest: [u8; 32],
    pub token: [u8; 16],
    pub transaction: String,
    pub program: String,
    pub channel: Option<String>,
    pub containers: BTreeMap<String, BtsTransidContainer>,
    pub scheduled_tick: u64,
    pub priority: u8,
    pub state: BtsTransidState,
    pub lease_epoch: u64,
    pub completion: Option<CicsBtsChildCompletion>,
    pub abcode: Option<String>,
    #[serde(skip)]
    pub row_version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outbox {
    schema_version: String,
    pending: BTreeSet<String>,
    #[serde(skip)]
    row_version: u64,
}

impl Outbox {
    fn empty() -> Self {
        Self {
            schema_version: OUTBOX_SCHEMA.into(),
            pending: BTreeSet::new(),
            row_version: 0,
        }
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != OUTBOX_SCHEMA
            || self.pending.len() > MAX_PENDING
            || self.pending.iter().any(|id| !valid_run_id(id))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }
}

impl BtsTransidRecord {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != RUN_SCHEMA
            || !valid_run_id(&self.run_id)
            || self.work_id != format!("cics-bts-child:{}", self.run_id)
            || validate_identifier(&self.parent_run_unit, 256).is_err()
            || validate_identifier(&self.parent_execution, 256).is_err()
            || validate_identifier(&self.parent_principal, 256).is_err()
            || validate_identifier(&self.effect_key, 256).is_err()
            || self.token != token_for_id(&self.run_id)
            || validate_identifier(&self.transaction, 4).is_err()
            || validate_identifier(&self.program, 8).is_err()
            || self
                .channel
                .as_deref()
                .is_some_and(|name| name.is_empty() || name.chars().count() > 16)
            || self.channel.is_none() && !self.containers.is_empty()
            || self.containers.len() > 256
            || self
                .containers
                .iter()
                .any(|(name, data)| name.is_empty() || name.len() > 16 || data.bytes.len() > 65_536)
            || self.scheduled_tick == 0
            || self.state == BtsTransidState::Pending && self.lease_epoch != 0
            || self.state == BtsTransidState::Attached && self.lease_epoch == 0
            || self.state == BtsTransidState::Finished && self.completion.is_none()
            || self.state != BtsTransidState::Finished && self.completion.is_some()
            || self.abcode.as_deref().is_some_and(|code| code.len() != 4)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }

    pub fn work_record(&self) -> Result<WorkRecord, HostProblem> {
        self.validate()?;
        let limits = InvocationLimits::default();
        Ok(WorkRecord {
            work_id: self.work_id.clone(),
            execution_id: ExecutionId::new(
                format!("cics-bts-child-{}", &self.run_id[..32]),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_selector: Selector::new("cics:bts-transid", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_generation: BTS_TRANSID_WORK_GENERATION.into(),
            artifact: ArtifactRef::new("artifact:none", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state: WorkState::Queued,
            priority: self.priority,
            attempt: 0,
            max_attempts: 86_400,
            available_tick: self.scheduled_tick,
            deadline_tick: self
                .scheduled_tick
                .checked_add(LIFETIME_TICKS)
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
            payload: self.run_id.as_bytes().to_vec(),
        })
    }
}

fn valid_run_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl<'a> BtsLifecycleStore<'a> {
    fn load_transid_outbox(&self) -> Result<Outbox, HostProblem> {
        let Some(row) = self
            .store
            .get_provider_state(OUTBOX_NAMESPACE, "pending")
            .map_err(store_error)?
        else {
            return Ok(Outbox::empty());
        };
        if row.version == 0 || row.payload.len() > MAX_RUN_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut outbox: Outbox =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        outbox.row_version = row.version;
        outbox.validate()?;
        Ok(outbox)
    }

    pub fn load_transid(&self, run_id: &str) -> Result<Option<BtsTransidRecord>, HostProblem> {
        if !valid_run_id(run_id) {
            return Err(HostProblem::Malformed);
        }
        let Some(row) = self
            .store
            .get_provider_state(RUN_NAMESPACE, run_id)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > MAX_RUN_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut record: BtsTransidRecord =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        record.row_version = row.version;
        record.validate()?;
        if record.run_id != run_id {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(record))
    }

    /// Save the exact request and pending work before child-token registration.
    #[allow(clippy::too_many_arguments)]
    pub fn start_transid(
        &self,
        parent_run_unit: &str,
        parent_execution: &str,
        parent_principal: &str,
        effect_key: &str,
        request_digest: [u8; 32],
        transaction: &str,
        program: &str,
        channel: Option<&str>,
        containers: BTreeMap<String, BtsTransidContainer>,
        scheduled_tick: u64,
        priority: u8,
    ) -> Result<BtsTransidRecord, HostProblem> {
        validate_identifier(parent_run_unit, 256)?;
        validate_identifier(parent_execution, 256)?;
        validate_identifier(parent_principal, 256)?;
        validate_identifier(effect_key, 256)?;
        let run_id = transid_id(parent_run_unit, parent_execution, effect_key);
        for _ in 0..MAX_CAS_ATTEMPTS {
            if let Some(saved) = self.load_transid(&run_id)? {
                return if saved.parent_run_unit == parent_run_unit
                    && saved.parent_execution == parent_execution
                    && saved.parent_principal == parent_principal
                    && saved.effect_key == effect_key
                    && saved.request_digest == request_digest
                    && saved.transaction == transaction
                    && saved.program == program
                    && saved.channel.as_deref() == channel
                    && saved.containers == containers
                    && saved.priority == priority
                {
                    Ok(saved)
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            let mut outbox = self.load_transid_outbox()?;
            if outbox.pending.len() >= MAX_PENDING {
                return Err(HostProblem::ResourceExhausted);
            }
            let token = token_for_id(&run_id);
            let mut record = BtsTransidRecord {
                schema_version: RUN_SCHEMA.into(),
                run_id: run_id.clone(),
                work_id: format!("cics-bts-child:{run_id}"),
                parent_run_unit: parent_run_unit.into(),
                parent_execution: parent_execution.into(),
                parent_principal: parent_principal.into(),
                effect_key: effect_key.into(),
                request_digest,
                token,
                transaction: transaction.into(),
                program: program.into(),
                channel: channel.map(str::to_owned),
                containers: containers.clone(),
                scheduled_tick,
                priority,
                state: BtsTransidState::Pending,
                lease_epoch: 0,
                completion: None,
                abcode: None,
                row_version: 0,
            };
            record.validate()?;
            outbox.pending.insert(run_id.clone());
            match self.store.mutate_provider_states_atomic(vec![
                put_transid(&record, None)?,
                put_outbox(&outbox)?,
            ]) {
                Ok(()) => {
                    record.row_version = 1;
                    return Ok(record);
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub fn promote_transid(
        &self,
        work: &WorkRecord,
    ) -> Result<Option<BtsTransidRecord>, HostProblem> {
        validate_work(work)?;
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut record = self.load_transid(run_id)?.ok_or(HostProblem::NotFound)?;
            if record.work_id != work.work_id {
                return Err(HostProblem::Malformed);
            }
            if record.state == BtsTransidState::Finished {
                return Ok(None);
            }
            if record.lease_epoch > work.lease_epoch {
                return Err(HostProblem::IdempotencyConflict);
            }
            if record.lease_epoch == work.lease_epoch && record.state == BtsTransidState::Attached {
                return Ok(Some(record));
            }
            let expected = record.row_version;
            record.state = BtsTransidState::Attached;
            record.lease_epoch = work.lease_epoch;
            match self
                .store
                .mutate_provider_states_atomic(vec![put_transid(&record, Some(expected))?])
            {
                Ok(()) => {
                    record.row_version = expected + 1;
                    return Ok(Some(record));
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub fn finish_transid(
        &self,
        work: &WorkRecord,
        completion: CicsBtsChildCompletion,
        abcode: Option<&str>,
    ) -> Result<BtsTransidRecord, HostProblem> {
        validate_work(work)?;
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut record = self.load_transid(run_id)?.ok_or(HostProblem::NotFound)?;
            if record.work_id != work.work_id || record.lease_epoch != work.lease_epoch {
                return Err(HostProblem::IdempotencyConflict);
            }
            if record.state == BtsTransidState::Finished {
                return if record.completion == Some(completion)
                    && record.abcode.as_deref() == abcode
                {
                    Ok(record)
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            if record.state != BtsTransidState::Attached {
                return Err(HostProblem::Malformed);
            }
            let mut outbox = self.load_transid_outbox()?;
            if !outbox.pending.remove(run_id) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let expected = record.row_version;
            record.state = BtsTransidState::Finished;
            record.completion = Some(completion);
            record.abcode = abcode.map(str::to_owned);
            record.validate()?;
            match self.store.mutate_provider_states_atomic(vec![
                put_transid(&record, Some(expected))?,
                put_outbox(&outbox)?,
            ]) {
                Ok(()) => {
                    record.row_version = expected + 1;
                    return Ok(record);
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
    /// Close a crash gap only after the sibling token row proves completion.
    fn finish_transid_from_child(
        &self,
        run_id: &str,
        completion: CicsBtsChildCompletion,
        abcode: Option<&str>,
    ) -> Result<BtsTransidRecord, HostProblem> {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut record = self.load_transid(run_id)?.ok_or(HostProblem::NotFound)?;
            if record.state == BtsTransidState::Finished {
                return if record.completion == Some(completion)
                    && record.abcode.as_deref() == abcode
                {
                    Ok(record)
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            let mut outbox = self.load_transid_outbox()?;
            if !outbox.pending.remove(run_id) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let expected = record.row_version;
            record.state = BtsTransidState::Finished;
            record.completion = Some(completion);
            record.abcode = abcode.map(str::to_owned);
            match self.store.mutate_provider_states_atomic(vec![
                put_transid(&record, Some(expected))?,
                put_outbox(&outbox)?,
            ]) {
                Ok(()) => {
                    record.row_version = expected + 1;
                    return Ok(record);
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

impl CicsService {
    pub fn register_bts_transid_child(&self, record: &BtsTransidRecord) -> Result<(), HostProblem> {
        record.validate()?;
        let parent = RunUnitId::new(&record.parent_run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        if self.bts_child_registered(&parent, record.token, record.channel.as_deref())? {
            return Ok(());
        }
        match self.register_bts_child(&parent, record.token, record.channel.as_deref()) {
            Ok(()) => Ok(()),
            Err(HostProblem::IdempotencyConflict)
                if self.bts_child_registered(
                    &parent,
                    record.token,
                    record.channel.as_deref(),
                )? =>
            {
                Ok(())
            }
            Err(problem) => Err(problem),
        }
    }

    pub fn enqueue_bts_transid_work(&self, record: &BtsTransidRecord) -> Result<(), HostProblem> {
        enqueue_exact(
            self.work_store
                .as_ref()
                .ok_or(HostProblem::InfrastructureFailure)?
                .as_ref(),
            record,
        )
    }

    pub fn recover_bts_transid_work(&self) -> Result<usize, HostProblem> {
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let outbox = authority.load_transid_outbox()?;
        let mut admitted = 0;
        for run_id in outbox.pending {
            match self.reconcile_bts_transid_work(&run_id) {
                Ok(Some(_)) => continue,
                Ok(None) => {}
                // Retain the unresolved request and allow independent child
                // work to recover. Its exact reconciliation still reports
                // UnknownOutcome to the owner/operator.
                Err(HostProblem::UnknownOutcome) => continue,
                Err(problem) => return Err(problem),
            }
            let record = authority
                .load_transid(&run_id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            match self.register_bts_transid_child(&record) {
                Ok(()) => {}
                // The request remains pending for an exact parent retry.
                Err(HostProblem::Unauthorized) => continue,
                Err(problem) => return Err(problem),
            }
            self.enqueue_bts_transid_work(&record)?;
            admitted += 1;
        }
        Ok(admitted)
    }

    /// Reconcile a retained child result after a crash between token completion
    /// and closing the RUN TRANSID request/outbox. Never invent an outcome.
    pub fn reconcile_bts_transid_work(
        &self,
        run_id: &str,
    ) -> Result<Option<BtsTransidRecord>, HostProblem> {
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let record = authority
            .load_transid(run_id)?
            .ok_or(HostProblem::NotFound)?;
        let parent = RunUnitId::new(&record.parent_run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let observed = match self.bts_child_outcome(&parent, record.token) {
            Ok(outcome) => outcome,
            Err(HostProblem::NotFound) if record.state != BtsTransidState::Finished => None,
            Err(problem) => return Err(problem),
        };
        if let Some((completion, abcode)) = observed {
            return authority
                .finish_transid_from_child(run_id, completion, abcode.as_deref())
                .map(Some);
        }
        if record.state == BtsTransidState::Finished {
            return Err(HostProblem::InfrastructureFailure);
        }
        let work = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?
            .get_work(&record.work_id)
            .map_err(store_error)?;
        if work.as_ref().is_some_and(|work| work.state.terminal()) {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(None)
    }

    pub fn promote_bts_transid_work(
        &self,
        work: &WorkRecord,
    ) -> Result<Option<BtsTransidRecord>, HostProblem> {
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let record = authority.promote_transid(work)?;
        if let Some(record) = &record {
            let parent = RunUnitId::new(&record.parent_run_unit, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if !self.bts_child_registered(&parent, record.token, record.channel.as_deref())? {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        Ok(record)
    }

    pub fn complete_bts_transid_work(
        &self,
        work: &WorkRecord,
        completion: CicsBtsChildCompletion,
        abcode: Option<&str>,
    ) -> Result<BtsTransidRecord, HostProblem> {
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let record = authority
            .load_transid(run_id)?
            .ok_or(HostProblem::NotFound)?;
        if record.lease_epoch != work.lease_epoch {
            return Err(HostProblem::IdempotencyConflict);
        }
        let parent = RunUnitId::new(&record.parent_run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.complete_bts_child(&parent, record.token, completion, abcode)?;
        authority.finish_transid(work, completion, abcode)
    }
}

fn transid_id(parent: &str, execution: &str, effect_key: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        b"bts-transid-run-v1".as_slice(),
        parent.as_bytes(),
        execution.as_bytes(),
        effect_key.as_bytes(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    format!("{:x}", digest.finalize())
}

fn token_for_id(run_id: &str) -> [u8; 16] {
    let digest = Sha256::digest([b"bts-transid-token-v1".as_slice(), run_id.as_bytes()].concat());
    let mut token = [0; 16];
    token.copy_from_slice(&digest[..16]);
    token
}

fn put_transid(
    record: &BtsTransidRecord,
    expected: Option<u64>,
) -> Result<ProviderStateMutation, HostProblem> {
    record.validate()?;
    let payload = serde_json::to_vec(record).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_RUN_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RUN_NAMESPACE.into(),
            key: record.run_id.clone(),
            version: expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload,
        },
        expected_version: expected,
    }))
}

fn put_outbox(outbox: &Outbox) -> Result<ProviderStateMutation, HostProblem> {
    outbox.validate()?;
    let payload = serde_json::to_vec(outbox).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_RUN_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: OUTBOX_NAMESPACE.into(),
            key: "pending".into(),
            version: outbox
                .row_version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload,
        },
        expected_version: (outbox.row_version != 0).then_some(outbox.row_version),
    }))
}

fn validate_work(work: &WorkRecord) -> Result<(), HostProblem> {
    let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
    if work.required_generation != BTS_TRANSID_WORK_GENERATION
        || work.required_selector.as_str() != "cics:bts-transid"
        || work.artifact.as_str() != "artifact:none"
        || work.work_id != format!("cics-bts-child:{run_id}")
        || !valid_run_id(run_id)
        || work.state != WorkState::Claimed
        || work.lease_epoch == 0
        || work.lease_id.is_none()
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn enqueue_exact(work_store: &dyn WorkStore, record: &BtsTransidRecord) -> Result<(), HostProblem> {
    let expected = record.work_record()?;
    match work_store.enqueue(expected.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict) => {
            let current = work_store
                .get_work(&expected.work_id)
                .map_err(store_error)?
                .ok_or(HostProblem::IdempotencyConflict)?;
            if current.work_id != expected.work_id
                || current.execution_id != expected.execution_id
                || current.required_selector != expected.required_selector
                || current.required_generation != expected.required_generation
                || current.artifact != expected.artifact
                || current.priority != expected.priority
                || current.available_tick != expected.available_tick
                || current.deadline_tick != expected.deadline_tick
                || current.payload != expected.payload
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            if current.state.terminal() {
                return Err(HostProblem::UnknownOutcome);
            }
            Ok(())
        }
        Err(error) => Err(store_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::MemoryStore;

    #[test]
    fn transid_outbox_replays_and_fences_claimed_completion() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let record = authority
            .start_transid(
                "UOW1",
                "EXEC1",
                "USER",
                "run-transid",
                [1; 32],
                "BT01",
                "CHILD",
                Some("REPLY"),
                BTreeMap::from([(
                    "MESSAGE".into(),
                    BtsTransidContainer {
                        character: true,
                        bytes: b"snapshot".to_vec(),
                    },
                )]),
                1000,
                5,
            )
            .unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        assert_eq!(
            reopened.load_transid(&record.run_id).unwrap(),
            Some(record.clone())
        );
        assert_eq!(
            reopened.start_transid(
                "UOW1",
                "EXEC1",
                "USER",
                "run-transid",
                [2; 32],
                "BT01",
                "CHILD",
                Some("REPLY"),
                record.containers.clone(),
                1001,
                5,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        memory.enqueue(record.work_record().unwrap()).unwrap();
        let claimed = memory
            .claim("worker", Some(BTS_TRANSID_WORK_GENERATION), 1000, 30_000)
            .unwrap()
            .unwrap();
        let promoted = reopened.promote_transid(&claimed).unwrap().unwrap();
        assert_eq!(promoted.state, BtsTransidState::Attached);
        assert_eq!(promoted.lease_epoch, claimed.lease_epoch);
        let mut stale = claimed.clone();
        stale.lease_epoch += 1;
        assert_eq!(
            reopened.finish_transid(&stale, CicsBtsChildCompletion::Normal, None),
            Err(HostProblem::IdempotencyConflict)
        );
        let finished = reopened
            .finish_transid(&claimed, CicsBtsChildCompletion::Normal, None)
            .unwrap();
        assert_eq!(finished.state, BtsTransidState::Finished);
        assert_eq!(
            reopened.finish_transid(&claimed, CicsBtsChildCompletion::Normal, None),
            Ok(finished)
        );
        assert!(reopened.load_transid_outbox().unwrap().pending.is_empty());
    }
}
