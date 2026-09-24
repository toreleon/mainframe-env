//! Lease-fenced attachment and completion of durable BTS RUN work.

use super::*;
use mainframe_env_execution_api::RunUnitId;

impl CicsService {
    /// Close a finished worker's context after its CICS run has returned.
    pub fn close_bts_run_context(&self, work: &WorkRecord) -> Result<(), HostProblem> {
        self.check_work_lease(work)?;
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let record = authority
            .load_run(run_id)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if record.state != BtsRunState::Finished || record.work_id != work.work_id {
            return Err(HostProblem::IdempotencyConflict);
        }
        let run_unit = RunUnitId::new(
            format!("run-{}", work.execution_id),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let context = authority
            .load_context_row(run_unit.as_str())?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if context.process_type != record.process_type
            || context.process_name != record.process_name
            || context.activity_id != record.activity_id
            || context.activation_epoch != record.activation_epoch
            || context.owner_lease_epoch != work.lease_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        authority.close_context(run_unit.as_str())
    }

    /// Read the explicit application ABEND recorded for a registered child run.
    pub fn bts_run_abend(
        &self,
        run_unit: &RunUnitId,
    ) -> Result<Option<(String, String)>, HostProblem> {
        let state = self.lock()?;
        let run = state.runs.get(run_unit).ok_or(HostProblem::NotFound)?;
        let Some(abend) = run.latest_abend.as_ref() else {
            return Ok(None);
        };
        let code = String::from_utf8(abend.code.clone())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if code.len() != 4 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let program = abend
            .program
            .as_deref()
            .or(run.current_program.current.as_deref())
            .ok_or(HostProblem::InfrastructureFailure)?;
        if program.len() > 8 {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some((code, format!("{program:<8}"))))
    }
    /// Bind a claimed work lease to one active BTS activation before launch.
    pub fn promote_bts_run_work(
        &self,
        work: &WorkRecord,
    ) -> Result<Option<BtsRunRecord>, HostProblem> {
        self.check_work_lease(work)?;
        let authority = BtsLifecycleStore::new(self.store.as_ref());
        let record = authority.promote_run(work)?;
        if work.cancellation_requested && record.is_some() {
            authority.finish_run(work, BtsCompletion::Forced, None, None)?;
            return Ok(None);
        }
        Ok(record)
    }

    /// Save terminal child outcome with its process and completion event.
    pub fn complete_bts_run_work(
        &self,
        work: &WorkRecord,
        completion: BtsCompletion,
        abcode: Option<&str>,
        abprogram: Option<&str>,
    ) -> Result<(), HostProblem> {
        self.check_work_lease(work)?;
        BtsLifecycleStore::new(self.store.as_ref()).finish_run(work, completion, abcode, abprogram)
    }

    fn check_work_lease(&self, work: &WorkRecord) -> Result<(), HostProblem> {
        validate_work(work)?;
        let current = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?
            .get_work(&work.work_id)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        if current.state != WorkState::Claimed
            || current.lease_id != work.lease_id
            || current.lease_epoch != work.lease_epoch
            || current.execution_id != work.execution_id
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }
}

impl<'a> BtsLifecycleStore<'a> {
    pub fn promote_run(&self, work: &WorkRecord) -> Result<Option<BtsRunRecord>, HostProblem> {
        validate_work(work)?;
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let key = || HostProblem::InfrastructureFailure;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut record = self.load_run(run_id)?.ok_or_else(key)?;
            if record.work_id != work.work_id || !same_work(work, &record.work_record()?) {
                return Err(key());
            }
            if record.state == BtsRunState::Finished {
                return Ok(None);
            }
            if record.state == BtsRunState::Deferred {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut process = self
                .load_process(&record.process_type, &record.process_name)?
                .ok_or_else(key)?;
            let activity = process
                .activities
                .get(&record.activity_id)
                .ok_or_else(key)?;
            if activity.mode != BtsMode::Active
                || activity.activation_epoch != record.activation_epoch
            {
                self.retire_stale_run(&record, activity.completion)?;
                return Ok(None);
            }
            let old_process = process.row_version;
            let old_record = record.row_version;
            process.checkpoint(
                &record.activity_id,
                record.activation_epoch,
                work.lease_epoch,
                &record.work_id,
            )?;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            record.state = BtsRunState::Attached;
            let process_key = Self::process_key(&record.process_type, &record.process_name)?;
            match self.store.mutate_provider_states_atomic(vec![
                put_process(&process_key, &process, Some(old_process))?,
                put_run(&record, Some(old_record))?,
            ]) {
                Ok(()) => {
                    record.row_version = old_record + 1;
                    return Ok(Some(record));
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub fn finish_run(
        &self,
        work: &WorkRecord,
        completion: BtsCompletion,
        abcode: Option<&str>,
        abprogram: Option<&str>,
    ) -> Result<(), HostProblem> {
        validate_work(work)?;
        let run_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut record = self
                .load_run(run_id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if record.work_id != work.work_id || !same_work(work, &record.work_record()?) {
                return Err(HostProblem::InfrastructureFailure);
            }
            if record.state == BtsRunState::Finished {
                return if record.completion == Some(completion)
                    && record.abcode.as_deref() == abcode
                {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            if record.state != BtsRunState::Attached {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut process = self
                .load_process(&record.process_type, &record.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let old_process = process.row_version;
            let old_record = record.row_version;
            process.finish(
                &record.activity_id,
                record.activation_epoch,
                work.lease_epoch,
                completion,
                abcode,
                abprogram,
            )?;
            let mut cleanup = if completion == BtsCompletion::Incomplete {
                Vec::new()
            } else {
                self.completed_parent_cleanup(&mut process, &record.activity_id)?
            };
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            record.state = BtsRunState::Finished;
            record.completion = Some(completion);
            record.abcode = abcode.map(str::to_string);
            let mut outbox = self.load_run_outbox()?;
            if !outbox.pending.remove(&record.run_id) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let mut writes = vec![
                put_process(
                    &Self::process_key(&record.process_type, &record.process_name)?,
                    &process,
                    Some(old_process),
                )?,
                put_run(&record, Some(old_record))?,
                put_outbox(&outbox)?,
            ];
            writes.append(&mut cleanup);
            if completion != BtsCompletion::Incomplete {
                let activity = process
                    .activities
                    .get(&record.activity_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if let Some(parent) = activity.parent_id.as_deref()
                    && let Some(event) =
                        super::super::super::event_control::activity_completion::post_many(
                            self.store,
                            parent,
                            &[(
                                activity
                                    .completion_event
                                    .clone()
                                    .ok_or(HostProblem::InfrastructureFailure)?,
                                activity.id.clone(),
                            )],
                        )?
                {
                    writes.push(event);
                }
            }
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    fn retire_stale_run(
        &self,
        record: &BtsRunRecord,
        observed: BtsCompletion,
    ) -> Result<(), HostProblem> {
        let mut record = record.clone();
        record.state = BtsRunState::Finished;
        record.completion = Some(if observed == BtsCompletion::Incomplete {
            BtsCompletion::Forced
        } else {
            observed
        });
        let mut outbox = self.load_run_outbox()?;
        if !outbox.pending.remove(&record.run_id) {
            return Err(HostProblem::InfrastructureFailure);
        }
        self.store
            .mutate_provider_states_atomic(vec![
                put_run(&record, Some(record.row_version))?,
                put_outbox(&outbox)?,
            ])
            .map_err(store_error)
    }
}

fn validate_work(work: &WorkRecord) -> Result<(), HostProblem> {
    if work.required_generation != BTS_RUN_WORK_GENERATION
        || work.required_selector.as_str() != "cics:bts-run"
        || work.artifact.as_str() != "artifact:none"
        || work.work_id.strip_prefix("cics-bts-run:") != std::str::from_utf8(&work.payload).ok()
        || work.state != WorkState::Claimed
        || work.lease_id.is_none()
        || work.lease_epoch == 0
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
