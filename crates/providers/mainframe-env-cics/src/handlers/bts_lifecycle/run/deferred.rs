//! Atomic release of one suspended asynchronous RUN reservation.

use super::*;

impl BtsLifecycleStore<'_> {
    /// Retire deferred RUN rows whose activity is forced or removed by the
    /// caller's process transition. The caller includes these writes in the
    /// same CAS batch as the process and event-pool changes.
    pub(in crate::service) fn retire_deferred_for_ids(
        &self,
        process: &BtsProcess,
        activity_ids: &[String],
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        let targets = activity_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let mut outbox = self.load_run_outbox()?;
        let mut writes = Vec::new();
        for id in outbox.deferred.clone() {
            let mut record = self
                .load_run(&id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if record.process_type != process.process_type
                || record.process_name != process.name
                || !targets.contains(record.activity_id.as_str())
            {
                continue;
            }
            let activity = process
                .activities
                .get(&record.activity_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if record.state != BtsRunState::Deferred
                || record.activation_epoch != activity.activation_epoch
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let old_version = record.row_version;
            record.state = BtsRunState::Finished;
            record.completion = Some(BtsCompletion::Forced);
            record.abcode = None;
            writes.push(put_run(&record, Some(old_version))?);
            outbox.deferred.remove(&id);
        }
        if !writes.is_empty() {
            writes.push(put_outbox(&outbox)?);
        }
        Ok(writes)
    }

    fn deferred_for_activity(
        &self,
        process: &BtsProcess,
        activity_id: &str,
        outbox: &BtsRunOutbox,
    ) -> Result<Option<BtsRunRecord>, HostProblem> {
        let activity = process
            .activities
            .get(activity_id)
            .ok_or(HostProblem::NotFound)?;
        let mut found = None;
        for id in &outbox.deferred {
            let record = self
                .load_run(id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if record.process_type != process.process_type
                || record.process_name != process.name
                || record.activity_id != activity_id
            {
                continue;
            }
            if record.state != BtsRunState::Deferred
                || record.activation_epoch != activity.activation_epoch
                || found.is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            found = Some(record);
        }
        Ok(found)
    }

    /// Resume the subject and atomically release its reserved RUN, if any.
    /// A pending outbox entry survives a failure between this commit and work
    /// enqueue, so startup recovery can admit the same task.
    #[allow(clippy::too_many_arguments)]
    pub fn resume_deferred(
        &self,
        process_type: &str,
        process_name: &str,
        activity_id: &str,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        replay_key: &str,
        request_digest: [u8; 32],
        scheduled_tick: Option<u64>,
        priority: u8,
    ) -> Result<(BtsReply, Option<BtsRunRecord>), HostProblem> {
        validate_activity_id(activity_id)?;
        validate_identifier(run_unit, 256)?;
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        validate_identifier(replay_key, 256)?;
        let key = Self::process_key(process_type, process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut process = self
                .load_process(process_type, process_name)?
                .ok_or(HostProblem::NotFound)?;
            if !process.visible_to(run_unit) {
                return Err(HostProblem::NotFound);
            }
            if let Some(replay) = process.replays.get(replay_key) {
                if replay.owner_run_unit != run_unit
                    || replay.owner_execution != owner_execution
                    || replay.owner_principal != owner_principal
                    || replay.request_digest != request_digest
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let record = replay
                    .outputs
                    .get("BTS.RUNID")
                    .map(|bytes| {
                        let id = std::str::from_utf8(bytes)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        self.load_run(id)?.ok_or(HostProblem::InfrastructureFailure)
                    })
                    .transpose()?;
                return Ok((BtsReply::normal(), record));
            }
            if process.replays.len() >= MAX_REPLAYS {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut outbox = self.load_run_outbox()?;
            let deferred = self.deferred_for_activity(&process, activity_id, &outbox)?;
            let was_suspended = process
                .activities
                .get(activity_id)
                .ok_or(HostProblem::NotFound)?
                .suspended;
            if deferred.is_some() && !was_suspended {
                return Err(HostProblem::InfrastructureFailure);
            }
            let old_process_version = process.row_version;
            process.set_suspended(activity_id, false)?;
            let mut writes = Vec::with_capacity(5);
            let queued = if was_suspended {
                let (queued, event_write) =
                    super::super::super::event_control::prepare_resume_event_fence(
                        self.store,
                        activity_id,
                    )?;
                writes.push(event_write);
                queued
            } else {
                Vec::new()
            };
            let mut released = None;
            if let Some(mut record) = deferred {
                let activity = process
                    .activities
                    .get_mut(activity_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if !matches!(activity.mode, BtsMode::Initial | BtsMode::Dormant)
                    || activity.activation_epoch != record.activation_epoch
                    || !outbox.deferred.remove(&record.run_id)
                    || record.input_event != "DFHINITIAL"
                        && !queued.iter().any(|event| event == &record.input_event)
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                activity.mode = BtsMode::Active;
                let old_run_version = record.row_version;
                record.state = BtsRunState::Pending;
                record.scheduled_tick = scheduled_tick
                    .filter(|tick| *tick != 0)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                outbox.pending.insert(record.run_id.clone());
                writes.push(put_run(&record, Some(old_run_version))?);
                writes.push(put_outbox(&outbox)?);
                released = Some(record);
            } else if let Some(event) = queued.first() {
                let activity = process
                    .activities
                    .get(activity_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if activity.mode != BtsMode::Active {
                    if activity.mode != BtsMode::Dormant {
                        return Err(HostProblem::Unsupported);
                    }
                    if outbox.pending.len() + outbox.deferred.len() >= MAX_PENDING_RUNS {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    let tick = scheduled_tick
                        .filter(|tick| *tick != 0)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    let statement_id = format!("{run_unit}:0");
                    let id = run_id(run_unit, owner_execution, &statement_id, replay_key);
                    let ticket = process.start(activity_id, Some(event), false)?;
                    let record = BtsRunRecord {
                        schema_version: RUN_SCHEMA.into(),
                        run_id: id.clone(),
                        work_id: format!("cics-bts-run:{id}"),
                        owner_run_unit: run_unit.into(),
                        owner_execution: owner_execution.into(),
                        owner_principal: owner_principal.into(),
                        statement_id,
                        effect_key: replay_key.into(),
                        request_digest,
                        request_shape_digest: request_digest,
                        process_type: process_type.into(),
                        process_name: process_name.into(),
                        activity_id: activity_id.into(),
                        activation_epoch: ticket.activation_epoch,
                        transaction: ticket.transid,
                        program: ticket.program,
                        userid: ticket.userid,
                        input_event: ticket.input_event,
                        synchronous: false,
                        facility_token: None,
                        scheduled_tick: tick,
                        priority,
                        state: BtsRunState::Pending,
                        completion: None,
                        abcode: None,
                        row_version: 0,
                    };
                    record.validate()?;
                    outbox.pending.insert(id);
                    writes.push(put_run(&record, None)?);
                    writes.push(put_outbox(&outbox)?);
                    released = Some(record);
                }
            }
            let reply = BtsReply::normal();
            process.replays.insert(
                replay_key.into(),
                BtsReplay {
                    owner_execution: owner_execution.into(),
                    owner_run_unit: run_unit.into(),
                    owner_principal: owner_principal.into(),
                    request_digest,
                    condition: reply.condition.clone(),
                    response: reply.response,
                    response2: reply.response2,
                    outputs: released.as_ref().map_or_else(BTreeMap::new, |record| {
                        BTreeMap::from([("BTS.RUNID".into(), record.run_id.as_bytes().to_vec())])
                    }),
                },
            );
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(put_process(&key, &process, Some(old_process_version))?);
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => {
                    if let Some(record) = released.as_mut() {
                        record.row_version = record
                            .row_version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?;
                    }
                    return Ok((reply, released));
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::handlers::event_control::{self, EventKind, EventRecord};
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::WorkStore;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn resume_reattaches_for_a_fired_dormant_event_with_one_work_row() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
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
                    process.set_suspended(&root, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let mut pool = event_control::ActivityState::default();
        for name in ["READY", "SECOND"] {
            pool.events.insert(
                name.into(),
                EventRecord {
                    kind: EventKind::Input,
                    fired: true,
                    parent: None,
                },
            );
            pool.reattach.push_back(name.into());
        }
        memory
            .mutate_provider_states_atomic(vec![
                event_control::activity_mutation(&root, &pool).unwrap(),
            ])
            .unwrap();
        let (reply, record) = authority
            .resume_deferred(
                "TYPE",
                "ORDER",
                &root,
                "UOW2",
                "EXEC2",
                "USER",
                "resume",
                [2; 32],
                Some(1000),
                5,
            )
            .unwrap();
        assert_eq!(reply, BtsReply::normal());
        let record = record.unwrap();
        assert_eq!(record.state, BtsRunState::Pending);
        assert_eq!(record.input_event, "READY");
        assert_eq!(record.activation_epoch, 2);
        assert_eq!(authority.load_run_outbox().unwrap().pending.len(), 1);
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .mode,
            BtsMode::Active
        );
        assert_eq!(
            event_control::load_activity_from_store(&memory, &root)
                .unwrap()
                .reattach
                .front()
                .map(String::as_str),
            Some("READY")
        );
        assert_eq!(
            event_control::load_activity_from_store(&memory, &root)
                .unwrap()
                .reattach
                .len(),
            2
        );
        memory.enqueue(record.work_record().unwrap()).unwrap();
        assert_eq!(
            authority
                .resume_deferred(
                    "TYPE",
                    "ORDER",
                    &root,
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "resume",
                    [2; 32],
                    Some(2000),
                    5,
                )
                .unwrap()
                .1
                .unwrap()
                .run_id,
            record.run_id
        );
        assert_eq!(
            authority.resume_deferred(
                "TYPE",
                "ORDER",
                &root,
                "UOW2",
                "EXEC2",
                "USER",
                "resume",
                [3; 32],
                Some(2000),
                5,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
    }

    #[test]
    fn deferred_release_and_exact_work_survive_sqlite_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-deferred-resume-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let (root, run_id) = {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
            authority
                .define_process(
                    BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1")
                        .unwrap(),
                    "UOW1",
                    "EXEC1",
                    "USER",
                )
                .unwrap();
            authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
            authority
                .acquire("UOW2", "EXEC2", "USER", "TYPE", "ORDER", &root)
                .unwrap();
            authority
                .mutate_process(
                    "TYPE",
                    "ORDER",
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "suspend",
                    [1; 32],
                    |process| {
                        process.set_suspended(&root, true)?;
                        Ok(BtsReply::normal())
                    },
                )
                .unwrap();
            let record = authority
                .start_run(
                    "TYPE", "ORDER", &root, None, false, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                    "run", [2; 32], [2; 32], 1000, 5,
                )
                .unwrap();
            assert_eq!(record.state, BtsRunState::Deferred);
            assert!(sqlite.get_work(&record.work_id).unwrap().is_none());
            (root, record.run_id)
        };
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            let (_, released) = authority
                .resume_deferred(
                    "TYPE",
                    "ORDER",
                    &root,
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "resume",
                    [3; 32],
                    Some(2000),
                    5,
                )
                .unwrap();
            assert_eq!(released.unwrap().state, BtsRunState::Pending);
            let outbox = authority.load_run_outbox().unwrap();
            assert!(outbox.deferred.is_empty());
            assert_eq!(outbox.pending, BTreeSet::from([run_id.clone()]));
        }
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            let record = authority.load_run(&run_id).unwrap().unwrap();
            enqueue_exact(&sqlite, &record).unwrap();
            assert!(sqlite.get_work(&record.work_id).unwrap().is_some());
            assert_eq!(
                authority
                    .resume_deferred(
                        "TYPE",
                        "ORDER",
                        &root,
                        "UOW2",
                        "EXEC2",
                        "USER",
                        "resume",
                        [3; 32],
                        Some(3000),
                        5,
                    )
                    .unwrap()
                    .1
                    .unwrap()
                    .run_id,
                run_id
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancel_and_reset_retire_deferred_rows_without_admitting_work() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
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
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "suspend",
                [1; 32],
                |process| {
                    process.set_suspended(&root, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let first = authority
            .start_run(
                "TYPE", "ORDER", &root, None, false, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                "first", [2; 32], [2; 32], 1000, 5,
            )
            .unwrap();
        authority
            .cancel_with_events(
                "TYPE", "ORDER", &root, "UOW2", "EXEC2", "USER", "cancel", [3; 32],
            )
            .unwrap();
        assert_eq!(
            authority
                .load_run(&first.run_id)
                .unwrap()
                .unwrap()
                .completion,
            Some(BtsCompletion::Forced)
        );
        assert!(authority.load_run_outbox().unwrap().deferred.is_empty());
        assert!(memory.get_work(&first.work_id).unwrap().is_none());
        authority
            .remove_subtree(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "reset-first",
                [4; 32],
                &super::super::super::removal::BtsRemoval::Reset {
                    activity_id: root.clone(),
                },
            )
            .unwrap();
        let second = authority
            .start_run(
                "TYPE", "ORDER", &root, None, false, None, "UOW2", "EXEC2", "USER", "UOW2:42",
                "second", [5; 32], [2; 32], 2000, 5,
            )
            .unwrap();
        assert_eq!(second.state, BtsRunState::Deferred);
        authority
            .remove_subtree(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "reset-second",
                [6; 32],
                &super::super::super::removal::BtsRemoval::Reset { activity_id: root },
            )
            .unwrap();
        assert_eq!(
            authority
                .load_run(&second.run_id)
                .unwrap()
                .unwrap()
                .completion,
            Some(BtsCompletion::Forced)
        );
        assert!(authority.load_run_outbox().unwrap().deferred.is_empty());
        assert!(memory.get_work(&second.work_id).unwrap().is_none());
    }
}
