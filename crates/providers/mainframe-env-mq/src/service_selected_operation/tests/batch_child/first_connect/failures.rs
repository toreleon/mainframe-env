use super::*;

fn assert_unpublished(f: &Fixture) {
    let mut guard = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &mut *guard else {
        panic!()
    };
    assert!(s.ownership.control.is_none());
    assert!(s.ownership.units.is_empty());
    assert!(s.receipts.is_empty());
    let runtime = s.runtime.as_mut().unwrap();
    assert!(runtime.connections.is_empty());
    assert!(runtime.objects.is_empty());
    assert!(runtime.replies.is_empty());
    assert_eq!(runtime.control.next_unit(), 1);
    assert_eq!(runtime.handles.handles_mut().active_handles(), 0);
}

#[test]
fn memory_sqlite_child_first_connect_rejects_parent_intent_context_and_frozen_frame_substitution() {
    for sqlite in [false, true] {
        for case in 0..4 {
            let f = Fixture::new(sqlite);
            let child = Child::new(&f);
            let mut e = child.effect(&f, 10, request());
            if case == 0 {
                f.seed(&e);
            } else {
                if case == 1 {
                    let HostRequest::MqMqi(h) = &mut e.request else {
                        panic!()
                    };
                    h.envelope.context.owner.thread_id += 1;
                }
                child.seed(&f, &e);
            }
            let rows = f.rows();
            let core = f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap();
            let mut inv = child.inv.clone();
            if case == 2 {
                inv.parent_execution_id = None;
            }
            let frame = if case == 3 {
                f.frame
            } else {
                child.binding.frame()
            };
            assert!(
                f.service
                    .execute_selected_mqi(
                        frame,
                        &inv,
                        e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                        &f.provider,
                        HostLimits::default()
                    )
                    .is_err(),
                "case {case}"
            );
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap(),
                core
            );
            assert_unpublished(&f);
        }
    }
}

#[test]
fn memory_sqlite_child_first_connect_needs_same_task_live_parent_and_physical_probe() {
    for sqlite in [false, true] {
        for case in 0..6 {
            let f = Fixture::new(sqlite);
            let child = Child::new(&f);
            f.service.abort_selected_batch_child(child.binding).unwrap();
            let mut inv = child.inv.clone();
            let mut parent = f.inv.clone();
            let relation = if case == 0 {
                InstalledBatchRelationship::SeparateSubtask
            } else {
                InstalledBatchRelationship::SameTaskCall
            };
            match case {
                1 => inv.cancellation_probe = Some(CancellationProbe::new()),
                2 => parent.attempt += 1,
                3 => {
                    inv.deadline_tick = 1001;
                }
                4 => {
                    f.inv.cancellation_probe.as_ref().unwrap().request();
                }
                5 => {
                    let mut guard = f.service.lock_selected().unwrap();
                    let rich_state::StoredAuthority::Rich(s) = &mut *guard else {
                        panic!()
                    };
                    let runtime = s.runtime.as_mut().unwrap();
                    runtime
                        .directory
                        .retire_frame(f.frame, &mut runtime.handles.handles_mut())
                        .unwrap();
                }
                _ => {}
            }
            let rows = f.rows();
            assert!(
                f.service
                    .prepare_selected_batch_child(f.frame, &parent, &inv, relation)
                    .is_err()
            );
            let e = child.effect(&f, 10, request());
            child.seed(&f, &e);
            assert!(child.execute(&f, &e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
            assert_unpublished(&f);
        }
    }
}

#[test]
fn memory_sqlite_child_first_connect_late_controls_cas_audit_quota_and_core_race_rollback() {
    for sqlite in [false, true] {
        for case in 0..7 {
            let f = Fixture::new(sqlite);
            let child = Child::new(&f);
            let e = child.effect(&f, 10, request());
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
                    let store = f.store.clone();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                        let mut row = store
                            .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
                            .unwrap()
                            .unwrap();
                        let version = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(version)).unwrap();
                    }));
                }
                4 => super::super::super::bounds::fill_audits(&f),
                _ => {
                    let store = f.store.clone();
                    let execution = child.inv.execution_id.clone();
                    let key = e.idempotency_key.clone().unwrap();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                        if case == 5 {
                            store
                                .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                                .unwrap();
                        } else {
                            store
                                .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                                .unwrap();
                        }
                    }));
                }
            }
            let mut expected = f.rows();
            if case == 3 {
                expected
                    .iter_mut()
                    .find(|r| r.namespace == CATALOG_NAMESPACE && r.key == CATALOG_KEY)
                    .unwrap()
                    .version += 1;
            }
            let audits = f
                .store
                .audit_records(&child.inv.execution_id, 0, 256)
                .unwrap();
            assert!(child.execute(&f, &e).is_err(), "case {case}");
            assert_eq!(f.rows(), expected, "case {case}");
            if case != 0 {
                assert_eq!(
                    f.store
                        .audit_records(&child.inv.execution_id, 0, 256)
                        .unwrap(),
                    audits
                );
            }
            assert_unpublished(&f);
        }
    }
}
