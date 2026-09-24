//! Atomic forced cancellation and child-completion event publication.

use super::*;

impl<'a> BtsLifecycleStore<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn cancel_with_events(
        &self,
        process_type: &str,
        process_name: &str,
        activity_id: &str,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        replay_key: &str,
        request_digest: [u8; 32],
    ) -> Result<BtsReply, HostProblem> {
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
            if let Some(saved) = process.replays.get(replay_key) {
                if saved.owner_execution != owner_execution
                    || saved.owner_run_unit != run_unit
                    || saved.owner_principal != owner_principal
                    || saved.request_digest != request_digest
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(BtsReply {
                    condition: saved.condition.clone(),
                    response: saved.response,
                    response2: saved.response2,
                    outputs: saved.outputs.clone(),
                });
            }
            if process.replays.len() >= MAX_REPLAYS {
                return Err(HostProblem::ResourceExhausted);
            }
            let old = process.clone();
            let ids = process.cancel_subtree(activity_id)?;
            if ids.iter().any(|id| {
                old.activities[id]
                    .acquired_by
                    .as_deref()
                    .is_some_and(|owner| owner != run_unit)
            }) {
                return Err(HostProblem::Condition {
                    name: "LOCKED".into(),
                    response: 100,
                    response2: 0,
                });
            }
            let mut by_parent: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
            for id in ids {
                let activity = old
                    .activities
                    .get(&id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if let Some(parent) = &activity.parent_id {
                    by_parent.entry(parent.clone()).or_default().push((
                        activity
                            .completion_event
                            .clone()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        id,
                    ));
                }
            }
            let mut writes = Vec::with_capacity(by_parent.len() + 1);
            for (parent, children) in by_parent {
                if let Some(event) = super::super::event_control::activity_completion::post_many(
                    self.store, &parent, &children,
                )? {
                    writes.push(event);
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
                    outputs: reply.outputs.clone(),
                },
            );
            process.epoch = old
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(put_process(&key, &process, Some(old.row_version))?);
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(reply),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests {
    use super::super::children::BtsChildDefinition;
    use super::*;
    use crate::service::handlers::event_control;
    use mainframe_env_store::MemoryStore;

    #[test]
    fn cancel_posts_child_completion_atomically_and_replays_exactly() {
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
                "start",
                [1; 32],
                |process| {
                    process.start(&root, None, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let child = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &BtsChildDefinition {
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
        let first = authority
            .cancel_with_events(
                "TYPE", "ORDER", &child, "UOW2", "EXEC2", "USER", "cancel", [3; 32],
            )
            .unwrap();
        assert_eq!(first, BtsReply::normal());
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&child]
                .completion,
            BtsCompletion::Forced,
        );
        let pool = event_control::load_activity_from_store(&memory, &root).unwrap();
        assert!(pool.events["DONE"].fired);
        assert_eq!(
            authority.cancel_with_events(
                "TYPE", "ORDER", &child, "UOW2", "EXEC2", "USER", "cancel", [3; 32],
            ),
            Ok(first)
        );
        assert_eq!(
            authority.cancel_with_events(
                "TYPE", "ORDER", &child, "UOW2", "EXEC2", "USER", "cancel", [4; 32],
            ),
            Err(HostProblem::IdempotencyConflict)
        );
    }
}
