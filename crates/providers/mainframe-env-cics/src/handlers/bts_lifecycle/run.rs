//! Durable RUN activation outbox and work admission for context switching.

use super::*;
use crate::service::CicsService;
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, InvocationLimits, Selector};
use mainframe_env_store_api::{WorkRecord, WorkState};

const RUN_NAMESPACE: &str = "cics-bts-run-request-v1";
const OUTBOX_NAMESPACE: &str = "cics-bts-run-outbox-v1";
const RUN_SCHEMA: &str = "mainframe-env.cics.bts-run-request@1";
const OUTBOX_SCHEMA: &str = "mainframe-env.cics.bts-run-outbox@1";
const MAX_RUN_BYTES: usize = 4096;
const MAX_PENDING_RUNS: usize = 4096;
const RUN_LIFETIME_TICKS: u64 = 86_400_000;

/// Work generation claimed by the server's CICS task worker.
pub const BTS_RUN_WORK_GENERATION: &str = "cics-bts-run-v1";

mod worker;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BtsRunState {
    Pending,
    Attached,
    Finished,
}

/// One RUN effect and the exact task to attach in a separate UOW.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsRunRecord {
    pub schema_version: String,
    pub run_id: String,
    pub work_id: String,
    pub owner_run_unit: String,
    pub owner_execution: String,
    pub owner_principal: String,
    pub statement_id: String,
    pub effect_key: String,
    pub request_digest: [u8; 32],
    pub request_shape_digest: [u8; 32],
    pub process_type: String,
    pub process_name: String,
    pub activity_id: String,
    pub activation_epoch: u64,
    pub transaction: String,
    pub program: String,
    pub userid: String,
    pub input_event: String,
    pub synchronous: bool,
    pub facility_token: Option<[u8; 8]>,
    pub scheduled_tick: u64,
    pub priority: u8,
    pub state: BtsRunState,
    pub completion: Option<BtsCompletion>,
    pub abcode: Option<String>,
    #[serde(skip)]
    pub row_version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BtsRunOutbox {
    schema_version: String,
    pending: BTreeSet<String>,
    #[serde(skip)]
    row_version: u64,
}

impl BtsRunOutbox {
    fn empty() -> Self {
        Self {
            schema_version: OUTBOX_SCHEMA.into(),
            pending: BTreeSet::new(),
            row_version: 0,
        }
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != OUTBOX_SCHEMA
            || self.pending.len() > MAX_PENDING_RUNS
            || self
                .pending
                .iter()
                .any(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }
}

impl BtsRunRecord {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != RUN_SCHEMA
            || self.run_id.len() != 64
            || !self.run_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.work_id != format!("cics-bts-run:{}", self.run_id)
            || validate_identifier(&self.owner_run_unit, 256).is_err()
            || validate_identifier(&self.owner_execution, 256).is_err()
            || validate_identifier(&self.owner_principal, 256).is_err()
            || self.statement_id.len() > 256
            || !self
                .statement_id
                .strip_prefix(&format!("{}:", self.owner_run_unit))
                .is_some_and(|position| {
                    !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit())
                })
            || validate_identifier(&self.effect_key, 256).is_err()
            || validate_name(&self.process_type, 8, true).is_err()
            || validate_name(&self.process_name, 36, true).is_err()
            || validate_activity_id(&self.activity_id).is_err()
            || self.activation_epoch == 0
            || validate_identifier(&self.transaction, 4).is_err()
            || validate_identifier(&self.program, 8).is_err()
            || validate_identifier(&self.userid, 8).is_err()
            || self.input_event != "DFHINITIAL"
                && super::super::event_control::event_name(&self.input_event).is_err()
            || self.scheduled_tick == 0
            || self.state == BtsRunState::Finished && self.completion.is_none()
            || self.state != BtsRunState::Finished && self.completion.is_some()
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
            execution_id: ExecutionId::new(format!("cics-bts-{}", &self.run_id[..32]), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_selector: Selector::new("cics:bts-run", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_generation: BTS_RUN_WORK_GENERATION.into(),
            artifact: ArtifactRef::new("artifact:none", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state: WorkState::Queued,
            priority: self.priority,
            attempt: 0,
            max_attempts: 86_400,
            available_tick: self.scheduled_tick,
            deadline_tick: self
                .scheduled_tick
                .checked_add(RUN_LIFETIME_TICKS)
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

impl<'a> BtsLifecycleStore<'a> {
    fn load_run_outbox(&self) -> Result<BtsRunOutbox, HostProblem> {
        let Some(row) = self
            .store
            .get_provider_state(OUTBOX_NAMESPACE, "pending")
            .map_err(store_error)?
        else {
            return Ok(BtsRunOutbox::empty());
        };
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut outbox: BtsRunOutbox =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        outbox.row_version = row.version;
        outbox.validate()?;
        Ok(outbox)
    }

    pub fn load_run(&self, run_id: &str) -> Result<Option<BtsRunRecord>, HostProblem> {
        if run_id.len() != 64 || !run_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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
        let mut record: BtsRunRecord =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        record.row_version = row.version;
        record.validate()?;
        if record.run_id != run_id {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(record))
    }

    /// Move an INITIAL or DORMANT activity to ACTIVE with its exact outbox row
    /// and a reattachment input event in one store transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn start_run(
        &self,
        process_type: &str,
        process_name: &str,
        activity_id: &str,
        input_event: Option<&str>,
        synchronous: bool,
        facility_token: Option<[u8; 8]>,
        owner_run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        statement_id: &str,
        effect_key: &str,
        request_digest: [u8; 32],
        request_shape_digest: [u8; 32],
        scheduled_tick: u64,
        priority: u8,
    ) -> Result<BtsRunRecord, HostProblem> {
        validate_identifier(owner_run_unit, 256)?;
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        validate_identifier(statement_id, 256)?;
        validate_identifier(effect_key, 256)?;
        validate_activity_id(activity_id)?;
        if scheduled_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        let run_id = run_id(owner_run_unit, owner_execution, statement_id);
        let key = Self::process_key(process_type, process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            if let Some(saved) = self.load_run(&run_id)? {
                if saved.owner_run_unit != owner_run_unit
                    || saved.owner_execution != owner_execution
                    || saved.owner_principal != owner_principal
                    || saved.statement_id != statement_id
                    || saved.request_shape_digest != request_shape_digest
                    || saved.effect_key == effect_key && saved.request_digest != request_digest
                    || saved.process_type != process_type
                    || saved.process_name != process_name
                    || saved.activity_id != activity_id
                    || saved.synchronous != synchronous
                    || saved.facility_token != facility_token
                    || saved.input_event != input_event.unwrap_or("DFHINITIAL")
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(saved);
            }
            let mut outbox = self.load_run_outbox()?;
            if outbox.pending.len() >= MAX_PENDING_RUNS {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut process = self
                .load_process(process_type, process_name)?
                .ok_or(HostProblem::NotFound)?;
            if !process.visible_to(owner_run_unit) {
                return Err(HostProblem::NotFound);
            }
            if process.replays.len() >= MAX_REPLAYS || process.replays.contains_key(effect_key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let old_version = process.row_version;
            let ticket = process.start(activity_id, input_event, synchronous)?;
            let mut record = BtsRunRecord {
                schema_version: RUN_SCHEMA.into(),
                run_id: run_id.clone(),
                work_id: format!("cics-bts-run:{run_id}"),
                owner_run_unit: owner_run_unit.into(),
                owner_execution: owner_execution.into(),
                owner_principal: owner_principal.into(),
                statement_id: statement_id.into(),
                effect_key: effect_key.into(),
                request_digest,
                request_shape_digest,
                process_type: process_type.into(),
                process_name: process_name.into(),
                activity_id: activity_id.into(),
                activation_epoch: ticket.activation_epoch,
                transaction: ticket.transid,
                program: ticket.program,
                userid: ticket.userid,
                input_event: ticket.input_event,
                synchronous,
                facility_token,
                scheduled_tick,
                priority,
                state: BtsRunState::Pending,
                completion: None,
                abcode: None,
                row_version: 0,
            };
            record.validate()?;
            let mut writes = Vec::with_capacity(3);
            if record.input_event != "DFHINITIAL" {
                writes.push(super::super::event_control::prepare_run_input_event(
                    self.store,
                    activity_id,
                    &record.input_event,
                )?);
            }
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            process.replays.insert(
                effect_key.into(),
                BtsReplay {
                    owner_execution: owner_execution.into(),
                    owner_run_unit: owner_run_unit.into(),
                    owner_principal: owner_principal.into(),
                    request_digest,
                    condition: "NORMAL".into(),
                    response: 0,
                    response2: 0,
                    outputs: BTreeMap::from([("BTS.RUNID".into(), run_id.as_bytes().to_vec())]),
                },
            );
            writes.push(put_process(&key, &process, Some(old_version))?);
            writes.push(put_run(&record, None)?);
            outbox.pending.insert(run_id.clone());
            writes.push(put_outbox(&outbox)?);
            match self.store.mutate_provider_states_atomic(writes) {
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
}

impl CicsService {
    /// Re-admit any task whose lifecycle activation committed before its work
    /// enqueue. This bounded pass is safe on startup and after an uncertain RUN.
    pub fn recover_bts_run_work(&self) -> Result<usize, HostProblem> {
        let work = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let outbox = authority.load_run_outbox()?;
        let mut enqueued = 0;
        for run_id in outbox.pending {
            let record = authority
                .load_run(&run_id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if record.state == BtsRunState::Finished {
                continue;
            }
            enqueue_exact(work.as_ref(), &record)?;
            enqueued += 1;
        }
        Ok(enqueued)
    }

    pub fn enqueue_bts_run_work(&self, record: &BtsRunRecord) -> Result<(), HostProblem> {
        let work = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        enqueue_exact(work.as_ref(), record)
    }
}

fn put_outbox(outbox: &BtsRunOutbox) -> Result<ProviderStateMutation, HostProblem> {
    outbox.validate()?;
    let payload = serde_json::to_vec(outbox).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_ROW_BYTES {
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

fn put_run(
    record: &BtsRunRecord,
    expected_version: Option<u64>,
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
            version: expected_version
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload,
        },
        expected_version,
    }))
}

fn enqueue_exact(
    work_store: &dyn mainframe_env_store_api::WorkStore,
    record: &BtsRunRecord,
) -> Result<(), HostProblem> {
    let expected = record.work_record()?;
    match work_store.enqueue(expected.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict) => {
            let current = work_store
                .get_work(&expected.work_id)
                .map_err(store_error)?
                .ok_or(HostProblem::IdempotencyConflict)?;
            if !same_work(&current, &expected) {
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

fn same_work(current: &WorkRecord, expected: &WorkRecord) -> bool {
    current.work_id == expected.work_id
        && current.execution_id == expected.execution_id
        && current.required_selector == expected.required_selector
        && current.required_generation == expected.required_generation
        && current.artifact == expected.artifact
        && current.priority == expected.priority
        && current.available_tick >= expected.available_tick
        && current.deadline_tick == expected.deadline_tick
        && current.payload == expected.payload
}

fn run_id(run_unit: &str, execution: &str, statement_id: &str) -> String {
    let mut digest = Sha256::new();
    for field in [run_unit, execution, statement_id] {
        digest.update((field.len() as u32).to_be_bytes());
        digest.update(field.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::handlers::event_control::{self, EventKind, EventRecord};
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::WorkStore;

    fn published_process<'a>(authority: &BtsLifecycleStore<'a>) -> String {
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap(),
                "UOW1",
                "EXEC1",
                "USER",
            )
            .unwrap();
        authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        authority
            .acquire("UOW2", "EXEC2", "USER", "TYPE", "ORDER", &root)
            .unwrap();
        root
    }

    #[test]
    fn run_admission_and_work_identity_survive_reopen_exactly() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = published_process(&authority);
        let first = authority
            .start_run(
                "TYPE", "ORDER", &root, None, true, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                "run", [3; 32], [3; 32], 1000, 5,
            )
            .unwrap();
        assert_eq!(first.activation_epoch, 1);
        assert_eq!(first.input_event, "DFHINITIAL");
        assert_eq!(first.state, BtsRunState::Pending);
        let work = first.work_record().unwrap();
        assert_eq!(work.required_generation, BTS_RUN_WORK_GENERATION);
        assert_eq!(work.payload, first.run_id.as_bytes());
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .mode,
            BtsMode::Active,
        );
        let reopened = BtsLifecycleStore::new(&memory);
        assert_eq!(reopened.load_run(&first.run_id).unwrap().unwrap(), first);
        assert_eq!(
            reopened.start_run(
                "TYPE", "ORDER", &root, None, true, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                "run", [3; 32], [3; 32], 2000, 5,
            ),
            Ok(first.clone())
        );
        assert_eq!(
            reopened.start_run(
                "TYPE", "ORDER", &root, None, true, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                "run", [4; 32], [3; 32], 2000, 5,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        memory.enqueue(first.work_record().unwrap()).unwrap();
        let claimed = memory
            .claim("worker", Some(BTS_RUN_WORK_GENERATION), 1000, 30_000)
            .unwrap()
            .unwrap();
        memory
            .dead_letter(
                &claimed.work_id,
                claimed.lease_id.as_deref().unwrap(),
                claimed.lease_epoch,
                1001,
            )
            .unwrap();
        assert_eq!(
            enqueue_exact(&memory, &first),
            Err(HostProblem::UnknownOutcome)
        );
    }

    #[test]
    fn dormant_run_fires_exact_input_with_activity_transition() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = published_process(&authority);
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "dormant",
                [1; 32],
                |process| {
                    process.start(&root, None, true)?;
                    process.finish(&root, 1, 1, BtsCompletion::Incomplete, None, None)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let failed = authority.start_run(
            "TYPE",
            "ORDER",
            &root,
            Some("READY"),
            false,
            None,
            "UOW2",
            "EXEC2",
            "USER",
            "UOW2:43",
            "run",
            [3; 32],
            [3; 32],
            1000,
            5,
        );
        assert!(
            matches!(failed, Err(HostProblem::Condition { name, response: 111, response2: 7 }) if name == "EVENTERR")
        );
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .mode,
            BtsMode::Dormant
        );
        let mut pool = event_control::ActivityState::default();
        pool.events.insert(
            "READY".into(),
            EventRecord {
                kind: EventKind::Input,
                fired: false,
                parent: None,
            },
        );
        memory
            .mutate_provider_states_atomic(vec![
                event_control::activity_mutation(&root, &pool).unwrap(),
            ])
            .unwrap();
        let run = authority
            .start_run(
                "TYPE",
                "ORDER",
                &root,
                Some("READY"),
                false,
                None,
                "UOW2",
                "EXEC2",
                "USER",
                "UOW2:43",
                "run",
                [3; 32],
                [3; 32],
                1000,
                5,
            )
            .unwrap();
        assert_eq!(run.input_event, "READY");
        assert!(
            event_control::load_activity_from_store(&memory, &root)
                .unwrap()
                .events["READY"]
                .fired
        );
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .mode,
            BtsMode::Active
        );
    }
}
