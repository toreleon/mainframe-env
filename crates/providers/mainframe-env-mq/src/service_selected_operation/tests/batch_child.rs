//! Real same-store coordinator-shaped fixtures, not installed host attestation
//! or a public/participant acceptance proof.
use super::*;
use crate::mqi_lifecycle::{BatchChildBinding, InstalledBatchRelationship};

#[path = "batch_child/first_connect.rs"]
mod first_connect;

struct Child {
    inv: Invocation,
    binding: BatchChildBinding,
}
impl Child {
    fn new(f: &Fixture) -> Self {
        Self::named(f, "child")
    }
    fn named(f: &Fixture, name: &str) -> Self {
        let mut inv = f.inv.clone();
        let l = InvocationLimits::default();
        inv.execution_id = ExecutionId::new(format!("{name}-execution"), l).unwrap();
        inv.request_id = RequestId::new(format!("{name}-request"), l).unwrap();
        inv.idempotency_key = IdempotencyKey::new(format!("{name}-invocation"), l).unwrap();
        inv.parent_execution_id = Some(f.inv.execution_id.clone());
        let binding = f
            .service
            .prepare_selected_batch_child(
                f.frame,
                &f.inv,
                &inv,
                InstalledBatchRelationship::SameTaskCall,
            )
            .unwrap();
        f.store
            .create_execution(ExecutionRecord {
                execution_id: inv.execution_id.clone(),
                run_unit_id: inv.run_unit_id.clone(),
                selector: inv.selector.clone(),
                artifact: inv.artifact.clone(),
                principal: inv.principal.id().clone(),
                state: ExecutionState::Admitted,
                attempt: inv.attempt,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: None,
            })
            .unwrap();
        f.store
            .transition_execution(&inv.execution_id, 1, ExecutionState::Queued, 6)
            .unwrap();
        f.store
            .transition_execution(&inv.execution_id, 2, ExecutionState::Running, 7)
            .unwrap();
        assert_eq!(
            f.service
                .selected_batch_owner(binding.frame(), &inv)
                .unwrap(),
            f.owner
        );
        Self { inv, binding }
    }
    fn effect(&self, f: &Fixture, sequence: u64, request: MqMqiRequest) -> EffectRequest {
        let mut e = f.effect(sequence, request);
        let key =
            IdempotencyKey::new(format!("child-effect-{sequence}"), Default::default()).unwrap();
        e.idempotency_key = Some(key.clone());
        let HostRequest::MqMqi(host) = &mut e.request else {
            panic!()
        };
        host.mutation.idempotency_key = key;
        e
    }
    fn seed(&self, f: &Fixture, e: &EffectRequest) {
        f.store
            .record_intent(EffectRecord {
                execution_id: self.inv.execution_id.clone(),
                run_unit_id: self.inv.run_unit_id.clone(),
                sequence: e.sequence,
                key: e.idempotency_key.clone().unwrap(),
                digest_format: EffectDigestFormat::CanonicalHostV1,
                request_digest: canonical_request_digest(&e.request).unwrap(),
                state: EffectState::Intent,
                result_digest: None,
                resolved_tick: None,
                intent: EffectIntentMetadata {
                    owner: self.inv.execution_id.clone(),
                    attempt: self.inv.attempt,
                    capability: Some(f.provider.capability.clone()),
                    audit_resource: Some(canonical_audit_resource_digest(&e.request)),
                    audit_invocation_key: Some(self.inv.idempotency_key.clone()),
                    created_tick: 8,
                    recovery_after_tick: 900,
                    epoch: 8,
                    recovery_lease: None,
                },
            })
            .unwrap();
    }
    fn execute(&self, f: &Fixture, e: &EffectRequest) -> Result<EffectResult, HostProblem> {
        f.service.execute_selected_mqi(
            self.binding.frame(),
            &self.inv,
            e.mq_mqi_occurrence(HostLimits::default())?
                .ok_or(HostProblem::Malformed)?,
            &f.provider,
            HostLimits::default(),
        )
    }
    fn call(&self, f: &Fixture, n: u64, r: MqMqiRequest) -> EffectResult {
        let e = self.effect(f, n, r);
        self.seed(f, &e);
        let core = f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap();
        let result = self.execute(f, &e).unwrap();
        let rows = f.rows();
        let audits = f
            .store
            .audit_records(&self.inv.execution_id, 0, 128)
            .unwrap();
        assert!(!audits.is_empty());
        assert_eq!(audits.last().unwrap().execution_id, self.inv.execution_id);
        assert_eq!(
            audits.last().unwrap().invocation_key,
            self.inv.idempotency_key
        );
        assert_eq!(
            audits.last().unwrap().resource,
            canonical_audit_resource_digest(&e.request)
        );
        assert_eq!(self.execute(f, &e).unwrap(), result);
        assert_eq!(f.rows(), rows);
        assert_eq!(
            f.store
                .audit_records(&self.inv.execution_id, 0, 128)
                .unwrap(),
            audits
        );
        assert_eq!(
            f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap(),
            core
        );
        let row = f
            .store
            .get_provider_state(
                receipt::NAMESPACE,
                e.idempotency_key.as_ref().unwrap().as_str(),
            )
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(value["value"]["execution"], self.inv.execution_id.as_str());
        result
    }
}

fn parent_pending(f: &Fixture) -> (MqHconn, MqHobj, u64) {
    let c = f.connect();
    let o = f.open(c);
    let unit = f.unit();
    f.call(
        3,
        MqMqiRequest::Put {
            connection: c,
            object: o,
            put: put(MqMqiUnitOfWork::Local { unit }),
        },
    );
    (c, o, unit)
}

fn assert_pending(f: &Fixture, unit: u64) {
    assert_eq!(f.unit(), unit);
    let guard = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*guard else {
        panic!()
    };
    assert_eq!(
        s.ownership.units[&unit].state,
        ownership::UnitState::Pending
    );
    assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
}

#[test]
fn memory_sqlite_same_task_child_get_put_return_preserves_parent_pending_and_handles() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (c, o, unit) = parent_pending(&f);
        // Seed a separately committed message for an actual child GET.
        f.call(
            4,
            MqMqiRequest::PutOne {
                connection: c,
                lookup: lookup(),
                alternate_user: None,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        let child = Child::new(&f);
        assert_eq!(
            f.service
                .selected_local_unit(child.binding.frame(), &child.inv, c)
                .unwrap(),
            MqMqiUnitOfWork::Local { unit }
        );
        let owner_bytes = f
            .store
            .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
            .unwrap();
        assert!(matches!(
            output(child.call(
                &f,
                10,
                get(
                    c,
                    o,
                    MqMqiUnitOfWork::Local { unit },
                    1024,
                    MqGetMode::Remove
                )
            )),
            MqMqiOutput::Got {
                message: Some(_),
                ..
            }
        ));
        child.call(
            &f,
            11,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        assert_pending(&f, unit);
        let owner_after = f
            .store
            .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
            .unwrap();
        assert_eq!(owner_bytes.unwrap().payload, owner_after.unwrap().payload);
        let rows = f.rows();
        f.service
            .return_selected_batch_child(child.binding.frame(), &child.inv)
            .unwrap();
        assert_eq!(f.rows(), rows);
        assert_pending(&f, unit);
        assert!(
            f.service
                .selected_batch_owner(child.binding.frame(), &child.inv)
                .is_err()
        );
        f.call(
            5,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        assert_eq!(f.depth(), 2);
        assert_ne!(f.unit(), unit);
        f.call(
            6,
            get(c, o, MqMqiUnitOfWork::NoSyncpoint, 1024, MqGetMode::Remove),
        );
    }
}

#[test]
fn memory_sqlite_child_commit_back_disc_preserve_original_logical_provenance() {
    for sqlite in [false, true] {
        for choice in 0..3 {
            let f = Fixture::new(sqlite);
            let (c, _, unit) = parent_pending(&f);
            let child = Child::new(&f);
            child.call(
                &f,
                10,
                match choice {
                    0 => MqMqiRequest::Commit {
                        connection: c,
                        unit,
                    },
                    1 => MqMqiRequest::Back {
                        connection: c,
                        unit,
                    },
                    _ => MqMqiRequest::Disconnect { connection: c },
                },
            );
            assert_eq!(f.depth(), usize::from(choice != 1));
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(
                s.ownership.units[&unit].state,
                if choice == 1 {
                    ownership::UnitState::RolledBack
                } else {
                    ownership::UnitState::Committed
                }
            );
            drop(guard);
            if choice < 2 {
                let fresh = f.unit();
                let row = f
                    .store
                    .get_provider_state(ownership::UOW_NAMESPACE, &fresh.to_string())
                    .unwrap()
                    .unwrap();
                let v: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                assert_eq!(v["value"]["execution"], f.inv.execution_id.as_str());
                assert_eq!(v["value"]["connection_key"], "effect-1");
            }
            let rows = f.rows();
            f.service
                .return_selected_batch_child(child.binding.frame(), &child.inv)
                .unwrap();
            assert_eq!(f.rows(), rows);
        }
    }
}

#[test]
fn memory_sqlite_child_rejects_parent_intent_and_receipt_collisions_before_saf() {
    for sqlite in [false, true] {
        for retained in [false, true] {
            let f = Fixture::new(sqlite);
            let (c, o, unit) = parent_pending(&f);
            let child = Child::new(&f);
            let e = f.effect(
                4,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::Local { unit }),
                },
            );
            f.seed(&e);
            if retained {
                f.execute(&e).unwrap();
            }
            let rows = f.rows();
            let calls = f.saf.calls.load(Ordering::SeqCst);
            assert!(child.execute(&f, &e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
            assert_pending(&f, unit);
        }
    }
}

#[test]
fn memory_sqlite_preparation_substitution_rollback_and_already_connected_child_fail_closed() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (c, _, unit) = parent_pending(&f);
        let child = Child::new(&f);
        let rows = f.rows();
        let again = f
            .service
            .prepare_selected_batch_child(
                f.frame,
                &f.inv,
                &child.inv,
                InstalledBatchRelationship::SameTaskCall,
            )
            .unwrap();
        f.service.abort_selected_batch_child(again).unwrap();
        assert_eq!(
            f.service
                .selected_local_unit(child.binding.frame(), &child.inv, c)
                .unwrap(),
            MqMqiUnitOfWork::Local { unit }
        );
        assert_eq!(
            f.service.prepare_selected_batch_child(
                f.frame,
                &f.inv,
                &child.inv,
                InstalledBatchRelationship::SeparateSubtask
            ),
            Err(HostProblem::Unsupported)
        );
        let mut forged = child.inv.clone();
        forged.cancellation_probe = Some(CancellationProbe::new());
        assert!(
            f.service
                .prepare_selected_batch_child(
                    f.frame,
                    &f.inv,
                    &forged,
                    InstalledBatchRelationship::SameTaskCall
                )
                .is_err()
        );
        let e = child.effect(
            &f,
            10,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        child.seed(&f, &e);
        let calls = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(child.execute(&f, &e), Err(HostProblem::Unsupported));
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        f.service.abort_selected_batch_child(child.binding).unwrap();
        assert!(
            f.service
                .selected_batch_owner(child.binding.frame(), &child.inv)
                .is_err()
        );
        assert_eq!(f.rows(), rows);
        assert_pending(&f, unit);
    }
}

#[test]
fn memory_sqlite_child_live_controls_revoked_saf_and_late_cas_audit_rollback() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = Fixture::new(sqlite);
            let (c, _, unit) = parent_pending(&f);
            let child = Child::new(&f);
            let e = child.effect(
                &f,
                10,
                MqMqiRequest::Commit {
                    connection: c,
                    unit,
                },
            );
            child.seed(&f, &e);
            match case {
                0 => f.saf.deny.store(true, Ordering::SeqCst),
                1 => {
                    let probe = child.inv.cancellation_probe.clone().unwrap();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || probe.request()));
                }
                2 => {
                    let clock = f.clock.clone();
                    *f.saf.hook.lock().unwrap() =
                        Some(Box::new(move || clock.0.store(901, Ordering::SeqCst)));
                }
                3 => {
                    let mut row = f
                        .store
                        .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
                        .unwrap()
                        .unwrap();
                    let version = row.version;
                    row.version += 1;
                    f.store.put_provider_state(row, Some(version)).unwrap();
                }
                _ => super::bounds::fill_audits(&f),
            }
            let rows = f.rows();
            let audits = f
                .store
                .audit_records(&child.inv.execution_id, 0, 256)
                .unwrap();
            assert!(child.execute(&f, &e).is_err(), "case {case}");
            assert_eq!(f.rows(), rows);
            if case != 0 {
                assert_eq!(
                    f.store
                        .audit_records(&child.inv.execution_id, 0, 256)
                        .unwrap(),
                    audits
                );
            }
            // A denied decision is audit-only; pending queue/UOW bytes stay exact.
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
            assert_eq!(
                s.ownership.units[&unit].state,
                ownership::UnitState::Pending
            );
            assert_eq!(s.runtime.as_ref().unwrap().connections[0].unit, unit);
        }
    }
}

#[test]
fn memory_sqlite_child_reply_uncertainty_fences_without_redispatch_or_return_decision() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (c, o, unit) = parent_pending(&f);
        let child = Child::new(&f);
        let e = child.effect(
            &f,
            10,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        child.seed(&f, &e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(child.execute(&f, &e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(child.execute(&f, &e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        assert_eq!(
            f.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
        f.service
            .return_selected_batch_child(child.binding.frame(), &child.inv)
            .unwrap();
        assert_eq!(f.rows(), rows);
        let guard = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*guard else {
            panic!()
        };
        assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
        assert_eq!(
            s.ownership.units[&unit].state,
            ownership::UnitState::Pending
        );
    }
}

#[test]
fn memory_sqlite_child_old_incarnation_receipt_is_fenced_before_saf() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (c, o, unit) = parent_pending(&f);
        let child = Child::new(&f);
        let e = child.effect(
            &f,
            10,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        child.seed(&f, &e);
        child.execute(&f, &e).unwrap();
        let cold = MqService::open_selected_mqi(
            f.store.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = cold.mint_selected_process(&f.inv).unwrap();
        let (frame, owner) = cold.bind_selected_root(process, &f.inv).unwrap();
        let mut fresh = f.effect(
            20,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        let HostRequest::MqMqi(host) = &mut fresh.request else {
            panic!()
        };
        host.envelope.context.owner = owner;
        f.seed(&fresh);
        cold.execute_selected_mqi(
            frame,
            &f.inv,
            fresh
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
            &f.provider,
            HostLimits::default(),
        )
        .unwrap();
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(child.execute(&f, &e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        let guard = cold.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*guard else {
            panic!()
        };
        assert_eq!(s.ownership.units[&unit].registry_epoch, 1);
        assert_eq!(s.ownership.control.as_ref().unwrap().registry_epoch, 2);
        assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
    }
}

#[test]
fn sqlite_physical_reopen_preserves_child_receipt_pending_origin_but_no_opaque_lineage() {
    let db = super::restart::Database::new();
    let (parent, child_inv, old_frame, old_conn, effect, rows, audit, core, unit) = {
        let f = Fixture::from_store(db.open());
        let (c, o, unit) = parent_pending(&f);
        let child = Child::new(&f);
        let e = child.effect(
            &f,
            10,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        child.seed(&f, &e);
        child.execute(&f, &e).unwrap();
        (
            f.inv.clone(),
            child.inv.clone(),
            child.binding.frame(),
            c,
            e.clone(),
            f.rows(),
            f.store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap(),
            f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap(),
            unit,
        )
    };
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store
            .audit_records(&child_inv.execution_id, 0, 128)
            .unwrap(),
        audit
    );
    assert_eq!(
        store
            .effect(effect.idempotency_key.as_ref().unwrap())
            .unwrap(),
        core
    );
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(20)));
    let saf = Arc::new(Saf::default());
    let cold =
        MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf.clone(), clock)
            .unwrap();
    assert!(cold.selected_batch_owner(old_frame, &child_inv).is_err());
    let process = cold.mint_selected_process(&parent).unwrap();
    let (frame, _) = cold.bind_selected_root(process, &parent).unwrap();
    let new_child = cold
        .prepare_selected_batch_child(
            frame,
            &parent,
            &child_inv,
            InstalledBatchRelationship::SameTaskCall,
        )
        .unwrap();
    assert_ne!(new_child.frame(), old_frame);
    assert!(
        cold.selected_local_unit(new_child.frame(), &child_inv, old_conn)
            .is_err()
    );
    assert_eq!(saf.calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    let guard = cold.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*guard else {
        panic!()
    };
    assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
    assert_eq!(
        s.ownership.units[&unit].state,
        ownership::UnitState::Pending
    );
}

#[test]
fn memory_sqlite_child_original_context_and_current_uow_cas_are_mandatory() {
    for sqlite in [false, true] {
        for context in [false, true] {
            let f = Fixture::new(sqlite);
            let (c, o, unit) = parent_pending(&f);
            let child = Child::new(&f);
            let mut e = child.effect(
                &f,
                10,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::NoSyncpoint),
                },
            );
            if context {
                let HostRequest::MqMqi(host) = &mut e.request else {
                    panic!()
                };
                host.envelope.context.owner.task_id += 1;
            } else {
                let mut row = f
                    .store
                    .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
                    .unwrap()
                    .unwrap();
                let old = row.version;
                row.version += 1;
                f.store.put_provider_state(row, Some(old)).unwrap();
            }
            child.seed(&f, &e);
            let rows = f.rows();
            let audits = f
                .store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap();
            let calls = f.saf.calls.load(Ordering::SeqCst);
            assert!(child.execute(&f, &e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store
                    .audit_records(&child.inv.execution_id, 0, 128)
                    .unwrap(),
                audits
            );
            if context {
                assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
            }
            assert_pending(&f, unit);
            assert_eq!(f.depth(), 0);
        }
    }
}

#[test]
fn memory_sqlite_child_intent_recovery_or_terminal_actor_after_saf_cannot_publish() {
    for sqlite in [false, true] {
        for terminal in [false, true] {
            let f = Fixture::new(sqlite);
            let (c, o, unit) = parent_pending(&f);
            let child = Child::new(&f);
            let e = child.effect(
                &f,
                10,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::Local { unit }),
                },
            );
            child.seed(&f, &e);
            let store = f.store.clone();
            let execution = child.inv.execution_id.clone();
            let key = e.idempotency_key.clone().unwrap();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                if terminal {
                    store
                        .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                        .unwrap();
                } else {
                    store
                        .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                        .unwrap();
                }
            }));
            let rows = f.rows();
            let audits = f
                .store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap();
            assert!(child.execute(&f, &e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store
                    .audit_records(&child.inv.execution_id, 0, 128)
                    .unwrap(),
                audits
            );
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
            assert_eq!(
                s.ownership.units[&unit].state,
                ownership::UnitState::Pending
            );
        }
    }
}

#[test]
fn root_unit_owner_v1_bytes_remain_exact() {
    let f = Fixture::new(false);
    f.connect();
    let row = f
        .store
        .get_provider_state(ownership::UOW_NAMESPACE, "1")
        .unwrap()
        .unwrap();
    assert_eq!(row.payload, br#"{"schema_version":"mainframe-env.mq-object-row@1","object_key":"1","value":{"schema_version":"mainframe-env.mq-selected-uow-owner@1","coordinator":"queue-manager-local","unit":1,"connection_key":"effect-1","execution":"execution","run":"run","principal":"TEST","generation":3,"fence":5,"registry_epoch":1,"state":"pending","queues":[]}}"#);
}
