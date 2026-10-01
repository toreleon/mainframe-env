//! Index-safe RESET and DELETE transitions for the shared BTS activity tree.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BtsRemoval {
    Reset {
        activity_id: String,
    },
    Delete {
        parent_id: String,
        child_name: String,
    },
}

impl<'a> BtsLifecycleStore<'a> {
    /// Prepare atomic deletion of settled descendants when a parent finishes.
    /// Live, acquired, or pending descendants require explicit reconciliation.
    pub(super) fn completed_parent_cleanup(
        &self,
        process: &mut BtsProcess,
        parent_id: &str,
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        let removed = process
            .subtree_ids(parent_id)
            .into_iter()
            .filter(|id| id != parent_id)
            .collect::<Vec<_>>();
        if removed.iter().any(|id| {
            let child = &process.activities[id];
            !matches!(child.mode, BtsMode::Initial | BtsMode::Complete)
                || child.acquired_by.is_some()
                || child.pending_uow.is_some()
        }) {
            return Err(HostProblem::UnknownOutcome);
        }
        let direct = process
            .activities
            .values()
            .filter(|activity| activity.parent_id.as_deref() == Some(parent_id))
            .map(|child| {
                Ok((
                    child
                        .completion_event
                        .clone()
                        .ok_or(HostProblem::InfrastructureFailure)?,
                    child.id.clone(),
                ))
            })
            .collect::<Result<Vec<_>, HostProblem>>()?;
        let mut writes = Vec::new();
        let mut cleanup_ids = removed.clone();
        cleanup_ids.push(parent_id.to_owned());
        writes.extend(super::super::bts_container::cleanup_bts_containers(
            self.store,
            process,
            &cleanup_ids,
            parent_id == process.root_id,
        )?);
        if !direct.is_empty()
            && let Some(event) = super::super::event_control::activity_completion::delete_many(
                self.store, parent_id, &direct,
            )?
        {
            writes.push(event);
        }
        for id in &removed {
            let child = &process.activities[id];
            let index = self
                .load_activity_index(id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if index.process_type != process.process_type
                || index.process_name != process.name
                || index.parent_id != child.parent_id
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            writes.push(ProviderStateMutation::Delete {
                namespace: ACTIVITY_INDEX_NAMESPACE.into(),
                key: id.clone(),
                expected_version: index.row_version,
            });
            if let Some(pool) =
                super::super::event_control::activity_completion::delete_pool(self.store, id)?
            {
                writes.push(pool);
            }
        }
        for id in removed {
            process.activities.remove(&id);
        }
        Ok(writes)
    }

    /// Atomically remove descendant indexes with RESET or DELETE and save the
    /// effect reply. The caller has already checked BTS scope and SAF.
    pub fn remove_subtree(
        &self,
        process_type: &str,
        process_name: &str,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        replay_key: &str,
        request_digest: [u8; 32],
        removal: &BtsRemoval,
    ) -> Result<BtsReply, HostProblem> {
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
            let original = &old.activities;
            let removed = match removal {
                BtsRemoval::Reset { activity_id } => {
                    validate_activity_id(activity_id)?;
                    process.reset_subtree(activity_id)?
                }
                BtsRemoval::Delete {
                    parent_id,
                    child_name,
                } => {
                    validate_activity_id(parent_id)?;
                    validate_name(child_name, 16, false)?;
                    if process
                        .activities
                        .get(parent_id)
                        .is_none_or(|parent| parent.mode != BtsMode::Active)
                    {
                        return Err(condition("INVREQ", 16, 4));
                    }
                    process.delete_child(parent_id, child_name)?
                }
            };
            let foreign_pending = |id: &str| {
                original[id]
                    .pending_uow
                    .as_deref()
                    .is_some_and(|owner| owner != run_unit)
            };
            if removed.iter().any(|id| foreign_pending(id))
                || matches!(
                    removal,
                    BtsRemoval::Reset { activity_id } if foreign_pending(activity_id)
                )
            {
                return Err(condition("LOCKED", 100, 0));
            }
            let mut retire_ids = removed.clone();
            if let BtsRemoval::Reset { activity_id } = removal {
                retire_ids.push(activity_id.clone());
            }
            let mut writes = self.retire_deferred_for_ids(&old, &retire_ids)?;
            writes.extend(super::super::bts_container::cleanup_bts_containers(
                self.store,
                &old,
                &retire_ids,
                matches!(removal, BtsRemoval::Reset { activity_id } if activity_id == &old.root_id),
            )?);
            match removal {
                BtsRemoval::Reset { activity_id } => {
                    let subject = original
                        .get(activity_id)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    if let Some(parent) = subject.parent_id.as_deref()
                        && let Some(event) =
                            super::super::event_control::activity_completion::reset(
                                self.store,
                                parent,
                                subject
                                    .completion_event
                                    .as_deref()
                                    .ok_or(HostProblem::InfrastructureFailure)?,
                                activity_id,
                            )?
                    {
                        writes.push(event);
                    }
                    let children = original
                        .values()
                        .filter(|child| {
                            child.parent_id.as_deref() == Some(activity_id.as_str())
                                && !process.activities.contains_key(&child.id)
                        })
                        .map(|child| {
                            Ok((
                                child
                                    .completion_event
                                    .clone()
                                    .ok_or(HostProblem::InfrastructureFailure)?,
                                child.id.clone(),
                            ))
                        })
                        .collect::<Result<Vec<_>, HostProblem>>()?;
                    if !children.is_empty()
                        && let Some(event) =
                            super::super::event_control::activity_completion::delete_many(
                                self.store,
                                activity_id,
                                &children,
                            )?
                    {
                        writes.push(event);
                    }
                }
                BtsRemoval::Delete {
                    parent_id,
                    child_name,
                } => {
                    let child = original
                        .values()
                        .find(|child| {
                            child.parent_id.as_deref() == Some(parent_id)
                                && child.name == *child_name
                        })
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    if let Some(event) = super::super::event_control::activity_completion::delete(
                        self.store,
                        parent_id,
                        child
                            .completion_event
                            .as_deref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        &child.id,
                    )? {
                        writes.push(event);
                    }
                }
            }
            for id in removed {
                let index = self
                    .load_activity_index(&id)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let old = original
                    .get(&id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if index.process_type != process_type
                    || index.process_name != process_name
                    || index.parent_id != old.parent_id
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                writes.push(ProviderStateMutation::Delete {
                    namespace: ACTIVITY_INDEX_NAMESPACE.into(),
                    key: id.clone(),
                    expected_version: index.row_version,
                });
                if let Some(pool) =
                    super::super::event_control::activity_completion::delete_pool(self.store, &id)?
                {
                    writes.push(pool);
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
            let expected = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(put_process(&key, &process, Some(expected))?);
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(reply),
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
    use super::super::children::BtsChildDefinition;
    use super::*;
    use mainframe_env_store::MemoryStore;

    fn parent_and_child<'a>(authority: &BtsLifecycleStore<'a>) -> (String, String) {
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
                |p| {
                    p.start(&root, None, true)?;
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
        (root, child)
    }

    #[test]
    fn delete_removes_child_and_index_in_one_transition() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let (root, child) = parent_and_child(&authority);
        let removal = BtsRemoval::Delete {
            parent_id: root.clone(),
            child_name: "CHILD".into(),
        };
        let reply = authority
            .remove_subtree(
                "TYPE", "ORDER", "UOW2", "EXEC2", "USER", "delete", [3; 32], &removal,
            )
            .unwrap();
        assert_eq!(reply, BtsReply::normal());
        assert!(authority.load_activity_index(&child).unwrap().is_none());
        assert!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities
                .get(&child)
                .is_none()
        );
        assert_eq!(
            authority
                .remove_subtree(
                    "TYPE", "ORDER", "UOW2", "EXEC2", "USER", "delete", [3; 32], &removal
                )
                .unwrap(),
            reply
        );
    }

    #[test]
    fn reset_removes_descendant_index_and_rejects_active_target() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let (root, child) = parent_and_child(&authority);
        let removal = BtsRemoval::Reset {
            activity_id: root.clone(),
        };
        assert!(
            authority
                .remove_subtree(
                    "TYPE", "ORDER", "UOW2", "EXEC2", "USER", "reset", [4; 32], &removal
                )
                .is_err()
        );
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "finish",
                [5; 32],
                |p| {
                    p.finish(&root, 1, 1, BtsCompletion::Normal, None, None)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        authority
            .remove_subtree(
                "TYPE", "ORDER", "UOW2", "EXEC2", "USER", "reset", [4; 32], &removal,
            )
            .unwrap();
        assert!(authority.load_activity_index(&child).unwrap().is_none());
        let process = authority.load_process("TYPE", "ORDER").unwrap().unwrap();
        assert_eq!(process.activities.len(), 1);
        assert_eq!(process.activities[&root].mode, BtsMode::Initial);
    }

    #[test]
    fn delete_keeps_another_uows_pending_child_and_index() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let (root, _) = parent_and_child(&authority);
        let pending = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &BtsChildDefinition {
                    name: "PENDING".into(),
                    completion_event: "PENDINGEV".into(),
                    program: "WORKER".into(),
                    transid: "BTS2".into(),
                    userid: "USER".into(),
                },
                "UOW3",
                "EXEC3",
                "USER",
                "define-pending",
                [6; 32],
            )
            .unwrap();
        let before = authority.load_process("TYPE", "ORDER").unwrap().unwrap();
        assert_eq!(
            authority.remove_subtree(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "delete-pending",
                [7; 32],
                &BtsRemoval::Delete {
                    parent_id: root,
                    child_name: "PENDING".into(),
                },
            ),
            Err(condition("LOCKED", 100, 0))
        );
        assert_eq!(
            authority.load_process("TYPE", "ORDER").unwrap(),
            Some(before.clone())
        );
        assert!(authority.load_activity_index(&pending).unwrap().is_some());
        assert_eq!(
            authority.remove_subtree(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "reset-pending",
                [8; 32],
                &BtsRemoval::Reset {
                    activity_id: pending.clone(),
                },
            ),
            Err(condition("LOCKED", 100, 0))
        );
        assert_eq!(
            authority.load_process("TYPE", "ORDER").unwrap(),
            Some(before)
        );
    }

    #[test]
    fn delete_retires_a_deferred_child_in_the_same_atomic_batch() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let (root, child) = parent_and_child(&authority);
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "suspend-child",
                [9; 32],
                |process| {
                    process.set_suspended(&child, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let deferred = authority
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
                "deferred-child",
                [10; 32],
                [10; 32],
                1000,
                5,
            )
            .unwrap();
        assert_eq!(deferred.state, super::super::run::BtsRunState::Deferred);
        authority
            .remove_subtree(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "delete-deferred",
                [11; 32],
                &BtsRemoval::Delete {
                    parent_id: root,
                    child_name: "CHILD".into(),
                },
            )
            .unwrap();
        assert!(authority.load_activity_index(&child).unwrap().is_none());
        let retired = authority.load_run(&deferred.run_id).unwrap().unwrap();
        assert_eq!(retired.state, super::super::run::BtsRunState::Finished);
        assert_eq!(retired.completion, Some(BtsCompletion::Forced));
        let outbox = memory
            .get_provider_state("cics-bts-run-outbox-v1", "pending")
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&outbox.payload).unwrap();
        assert!(value["deferred"].as_array().unwrap().is_empty());
    }
}
