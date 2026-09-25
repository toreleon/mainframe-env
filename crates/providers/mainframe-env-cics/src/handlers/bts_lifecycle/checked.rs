//! Parent CHECK acknowledgement of a completed child's event.

use super::*;

impl BtsLifecycleStore<'_> {
    /// Return one direct child's status and consume its completion event only
    /// while the same parent activation is still current. The process CAS
    /// fences a concurrent RESET or new completion against the event edit.
    pub(super) fn checked_child_and_ack(
        &self,
        context: &BtsActivityContext,
        child_name: &str,
    ) -> Result<BtsActivity, HostProblem> {
        validate_name(child_name, 16, false)?;
        let key = Self::process_key(&context.process_type, &context.process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut process = self
                .load_process(&context.process_type, &context.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if !process.visible_to(&context.run_unit) {
                return Err(HostProblem::NotFound);
            }
            let parent = process
                .activities
                .get(&context.activity_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if parent.mode != BtsMode::Active
                || parent.activation_epoch != context.activation_epoch
                || parent
                    .checkpoint
                    .as_ref()
                    .is_none_or(|saved| saved.owner_lease_epoch != context.owner_lease_epoch)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let child = process
                .child(&context.activity_id, child_name)
                .filter(|child| {
                    child
                        .pending_uow
                        .as_deref()
                        .is_none_or(|owner| owner == context.run_unit)
                })
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?
                .clone();
            if child.completion == BtsCompletion::Incomplete {
                return Ok(child);
            }
            let event = child
                .completion_event
                .as_deref()
                .ok_or(HostProblem::InfrastructureFailure)?;
            let Some(event_write) = super::super::event_control::activity_completion::acknowledge(
                self.store,
                &context.activity_id,
                event,
                &child.id,
            )?
            else {
                return Ok(child);
            };
            let old_version = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.mutate_provider_states_atomic(vec![
                put_process(&key, &process, Some(old_version))?,
                event_write,
            ]) {
                Ok(()) => return Ok(child),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::SqliteStateStore;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn checked_completion_stays_acknowledged_after_reopen_until_reset() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-checked-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let (context, child) = {
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
                    "start",
                    [1; 32],
                    |process| {
                        process.start(&root, None, true)?;
                        process.checkpoint(&root, 1, 1, "checkpoint")?;
                        Ok(BtsReply::normal())
                    },
                )
                .unwrap();
            let context = BtsActivityContext {
                schema_version: "mainframe-env.cics.bts-activity-context@1".into(),
                run_unit: "UOW2".into(),
                owner_execution: "EXEC2".into(),
                owner_principal: "USER".into(),
                process_type: "TYPE".into(),
                process_name: "ORDER".into(),
                activity_id: root,
                activation_epoch: 1,
                owner_lease_epoch: 1,
                closed: false,
                row_version: 0,
            };
            authority.bind_context(context.clone()).unwrap();
            let child = authority
                .define_child(
                    "TYPE",
                    "ORDER",
                    &context.activity_id,
                    &super::super::children::BtsChildDefinition {
                        name: "CHILD".into(),
                        completion_event: "DONE".into(),
                        program: "WORKER".into(),
                        transid: "BTS2".into(),
                        userid: "USER".into(),
                    },
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "define",
                    [2; 32],
                )
                .unwrap();
            authority
                .finish_child_uow("TYPE", "ORDER", "UOW2", true)
                .unwrap();
            authority
                .cancel_with_events(
                    "TYPE", "ORDER", &child, "UOW2", "EXEC2", "USER", "cancel", [3; 32],
                )
                .unwrap();
            assert_eq!(
                authority
                    .checked_child_and_ack(&context, "CHILD")
                    .unwrap()
                    .completion,
                BtsCompletion::Forced
            );
            (context, child)
        };
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            assert_eq!(
                authority
                    .checked_child_and_ack(&context, "CHILD")
                    .unwrap()
                    .completion,
                BtsCompletion::Forced
            );
            let pool = sqlite
                .get_provider_state("cics-event-activity-v1", &context.activity_id)
                .unwrap()
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&pool.payload).unwrap();
            assert!(value["events"].get("DONE").is_none());
            authority
                .remove_subtree(
                    "TYPE",
                    "ORDER",
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "reset",
                    [4; 32],
                    &super::super::removal::BtsRemoval::Reset { activity_id: child },
                )
                .unwrap();
            let pool = sqlite
                .get_provider_state("cics-event-activity-v1", &context.activity_id)
                .unwrap()
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&pool.payload).unwrap();
            assert_eq!(value["events"]["DONE"]["fired"], false);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
