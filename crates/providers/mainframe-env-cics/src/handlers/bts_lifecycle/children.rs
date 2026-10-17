//! Atomic child definition and UOW publication over the shared process row.

use super::*;

/// Source-checked DEFINE ACTIVITY input after compiler and SAF validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtsChildDefinition {
    pub name: String,
    pub completion_event: String,
    pub program: String,
    pub transid: String,
    pub userid: String,
}

impl BtsChildDefinition {
    pub fn validate(&self) -> Result<(), HostProblem> {
        validate_name(&self.name, 16, false)?;
        if super::super::event_control::event_name(&self.completion_event)
            .ok()
            .as_deref()
            != Some(self.completion_event.as_str())
        {
            return Err(HostProblem::Malformed);
        }
        validate_identifier(&self.program, 8)?;
        validate_identifier(&self.transid, 4)?;
        validate_identifier(&self.userid, 8)?;
        Ok(())
    }
}

impl<'a> BtsLifecycleStore<'a> {
    /// Add one pending direct child and its exact activity index with replay.
    pub fn define_child(
        &self,
        process_type: &str,
        process_name: &str,
        parent_id: &str,
        definition: &BtsChildDefinition,
        replay: BtsReplayContext<'_>,
    ) -> Result<String, HostProblem> {
        let BtsReplayContext {
            run_unit,
            owner_execution,
            owner_principal,
            replay_key,
            request_digest,
        } = replay;
        definition.validate()?;
        validate_activity_id(parent_id)?;
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
                let id = saved
                    .outputs
                    .get("ACTIVITYID")
                    .ok_or(HostProblem::InfrastructureFailure)?;
                return String::from_utf8(id.clone())
                    .map_err(|_| HostProblem::InfrastructureFailure);
            }
            let parent = process
                .activities
                .get(parent_id)
                .ok_or_else(|| condition("INVREQ", 16, 4))?;
            if parent.mode != BtsMode::Active
                || parent
                    .pending_uow
                    .as_deref()
                    .is_some_and(|owner| owner != run_unit)
            {
                return Err(condition("INVREQ", 16, 4));
            }
            if process.child(parent_id, &definition.name).is_some() {
                return Err(condition("ACTIVITYERR", 109, 3));
            }
            if process.activities.values().any(|activity| {
                activity.parent_id.as_deref() == Some(parent_id)
                    && activity.completion_event.as_deref()
                        == Some(definition.completion_event.as_str())
            }) {
                return Err(condition("EVENTERR", 111, 7));
            }
            if process.activities.len() >= MAX_ACTIVITIES || process.replays.len() >= MAX_REPLAYS {
                return Err(HostProblem::ResourceExhausted);
            }
            let sequence = process.next_child_sequence;
            let id = Self::child_id(process_type, process_name, &process.root_id, sequence)?;
            process.next_child_sequence = sequence
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let activity = BtsActivity {
                id: id.clone(),
                name: definition.name.clone(),
                parent_id: Some(parent_id.into()),
                completion_event: Some(definition.completion_event.clone()),
                program: definition.program.clone(),
                transid: definition.transid.clone(),
                userid: definition.userid.clone(),
                mode: BtsMode::Initial,
                completion: BtsCompletion::Incomplete,
                suspended: false,
                activation_epoch: 0,
                checkpoint: None,
                acquired_by: None,
                pending_uow: Some(run_unit.into()),
                abcode: None,
                abprogram: None,
            };
            process.activities.insert(id.clone(), activity);
            process.replays.insert(
                replay_key.into(),
                BtsReplay {
                    owner_execution: owner_execution.into(),
                    owner_run_unit: run_unit.into(),
                    owner_principal: owner_principal.into(),
                    request_digest,
                    condition: "NORMAL".into(),
                    response: 0,
                    response2: 0,
                    outputs: BTreeMap::from([("ACTIVITYID".into(), id.as_bytes().to_vec())]),
                },
            );
            let index = BtsActivityIndex {
                schema_version: ACTIVITY_INDEX_SCHEMA.into(),
                activity_id: id.clone(),
                process_type: process_type.into(),
                process_name: process_name.into(),
                parent_id: Some(parent_id.into()),
                pending_uow: Some(run_unit.into()),
                row_version: 0,
            };
            let expected = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let completion_event = super::super::event_control::activity_completion::define(
                self.store,
                parent_id,
                &definition.completion_event,
                &id,
            )?;
            match self.store.mutate_provider_states_atomic(vec![
                put_process(&key, &process, Some(expected))?,
                put_activity_index(&index, None)?,
                completion_event,
            ]) {
                Ok(()) => return Ok(id),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// Publish or roll back every child defined by this UOW in one CAS batch.
    /// The caller invokes this from the existing CICS syncpoint participant.
    pub fn finish_child_uow(
        &self,
        process_type: &str,
        process_name: &str,
        run_unit: &str,
        commit: bool,
    ) -> Result<(), HostProblem> {
        validate_identifier(run_unit, 256)?;
        let key = Self::process_key(process_type, process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut process = self
                .load_process(process_type, process_name)?
                .ok_or(HostProblem::NotFound)?;
            let mut writes = self.settle_pending_children(&mut process, run_unit, commit, false)?;
            if writes.is_empty() {
                return Ok(());
            }
            let expected = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(put_process(&key, &process, Some(expected))?);
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub(super) fn settle_pending_children(
        &self,
        process: &mut BtsProcess,
        run_unit: &str,
        commit: bool,
        retire_entire_process: bool,
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        let pending = process
            .activities
            .values()
            .filter(|activity| {
                activity.parent_id.is_some() && activity.pending_uow.as_deref() == Some(run_unit)
            })
            .map(|activity| activity.id.clone())
            .collect::<Vec<_>>();
        let mut writes = if commit {
            Vec::new()
        } else {
            let retired = if retire_entire_process {
                process.activities.keys().cloned().collect::<Vec<_>>()
            } else {
                pending.clone()
            };
            self.retire_deferred_for_ids(process, &retired)?
        };
        let mut removed_events: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for id in pending {
            let index = self
                .load_activity_index(&id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if index.process_type != process.process_type
                || index.process_name != process.name
                || index.pending_uow.as_deref() != Some(run_unit)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if commit {
                process
                    .activities
                    .get_mut(&id)
                    .expect("selected pending child")
                    .pending_uow = None;
                let mut index = index;
                index.pending_uow = None;
                writes.push(put_activity_index(&index, Some(index.row_version))?);
            } else {
                if process.activities[&id].acquired_by.is_some() {
                    return Err(condition("LOCKED", 100, 0));
                }
                let child = process
                    .activities
                    .remove(&id)
                    .expect("selected pending child");
                removed_events
                    .entry(child.parent_id.ok_or(HostProblem::InfrastructureFailure)?)
                    .or_default()
                    .push((
                        child
                            .completion_event
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        id.clone(),
                    ));
                writes.push(ProviderStateMutation::Delete {
                    namespace: ACTIVITY_INDEX_NAMESPACE.into(),
                    key: id,
                    expected_version: index.row_version,
                });
            }
        }
        for (parent, children) in removed_events {
            if let Some(event) = super::super::event_control::activity_completion::delete_many(
                self.store, &parent, &children,
            )? {
                writes.push(event);
            }
        }
        Ok(writes)
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
    use crate::service::handlers::event_control;
    use mainframe_env_store::MemoryStore;

    fn active_process<'a>(authority: &BtsLifecycleStore<'a>) -> String {
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process(process, "UOW1", "EXEC1", "USER")
            .unwrap();
        authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        authority
            .acquire("UOW2", "EXEC2", "USER", "TYPE", "ORDER", &root)
            .unwrap();
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "start",
                    request_digest: [1; 32],
                },
                |process| {
                    process.start(&root, None, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        root
    }

    #[test]
    fn child_definition_replays_and_publishes_index_at_syncpoint() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = active_process(&authority);
        let child = BtsChildDefinition {
            name: "CHILD".into(),
            completion_event: "DONE".into(),
            program: "WORKER".into(),
            transid: "BTS2".into(),
            userid: "USER".into(),
        };
        let id = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &child,
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "define",
                    request_digest: [2; 32],
                },
            )
            .unwrap();
        assert_eq!(id.len(), 52);
        let pool = event_control::load_activity_from_store(&memory, &root).unwrap();
        assert!(matches!(
            &pool.events["DONE"].kind,
            event_control::EventKind::Activity { child_id } if child_id == &id
        ));
        let replay = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &child,
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "define",
                    request_digest: [2; 32],
                },
            )
            .unwrap();
        assert_eq!(replay, id);
        assert!(
            authority
                .define_child(
                    "TYPE",
                    "ORDER",
                    &root,
                    &child,
                    crate::service::handlers::bts_lifecycle::BtsReplayContext {
                        run_unit: "UOW2",
                        owner_execution: "EXEC2",
                        owner_principal: "USER",
                        replay_key: "different",
                        request_digest: [3; 32]
                    }
                )
                .is_err()
        );
        assert_eq!(
            authority
                .load_activity_index(&id)
                .unwrap()
                .unwrap()
                .pending_uow
                .as_deref(),
            Some("UOW2")
        );
        authority.finish_uow("UOW2", "EXEC2", "USER", true).unwrap();
        assert!(
            authority
                .load_activity_index(&id)
                .unwrap()
                .unwrap()
                .pending_uow
                .is_none()
        );
        assert!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&id]
                .pending_uow
                .is_none()
        );
    }

    #[test]
    fn child_rollback_keeps_parent_and_clears_index() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = active_process(&authority);
        let child = BtsChildDefinition {
            name: "CHILD".into(),
            completion_event: "DONE".into(),
            program: "WORKER".into(),
            transid: "BTS2".into(),
            userid: "USER".into(),
        };
        let id = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &child,
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "define",
                    request_digest: [2; 32],
                },
            )
            .unwrap();
        authority
            .finish_uow("UOW2", "EXEC2", "USER", false)
            .unwrap();
        assert!(authority.load_activity_index(&id).unwrap().is_none());
        let pool = event_control::load_activity_from_store(&memory, &root).unwrap();
        assert!(!pool.events.contains_key("DONE"));
        let process = authority.load_process("TYPE", "ORDER").unwrap().unwrap();
        assert_eq!(process.activities.len(), 1);
        assert!(process.activities.contains_key(&root));
    }

    #[test]
    fn child_rollback_retires_deferred_run_with_child_index() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = active_process(&authority);
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
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "define",
                    request_digest: [2; 32],
                },
            )
            .unwrap();
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "suspend-child",
                    request_digest: [3; 32],
                },
                |process| {
                    process.set_suspended(&child, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let run = authority
            .start_run(
                "TYPE",
                "ORDER",
                &child,
                None,
                false,
                None,
                "UOW2",
                "EXEC2",
                "USER",
                "UOW2:42",
                "run-child",
                [4; 32],
                [4; 32],
                1000,
                5,
            )
            .unwrap();
        assert_eq!(run.state, super::super::run::BtsRunState::Deferred);
        authority
            .finish_child_uow("TYPE", "ORDER", "UOW2", false)
            .unwrap();
        assert!(authority.load_activity_index(&child).unwrap().is_none());
        assert_eq!(
            authority.load_run(&run.run_id).unwrap().unwrap().completion,
            Some(BtsCompletion::Forced)
        );
        let outbox = memory
            .get_provider_state("cics-bts-run-outbox-v1", "pending")
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&outbox.payload).unwrap();
        assert!(value["deferred"].as_array().unwrap().is_empty());
        assert!(authority.load_process("TYPE", "ORDER").unwrap().is_some());
    }
}
