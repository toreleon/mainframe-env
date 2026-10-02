use super::*;
use crate::mqi_lifecycle::{LifecycleLimits, MqLifecycleDirectory};

fn binding(bytes: &[u8], schema: &str) -> BoundedPayload {
    BoundedPayload::new(schema, bytes.to_vec(), Default::default()).unwrap()
}

#[test]
fn explicit_context_rejects_all_present_conflicts_and_keeps_old_routes_binding_only() {
    let f = unbound(false);
    let mut directory = MqLifecycleDirectory::new(LifecycleLimits::default()).unwrap();
    assert!(directory.mint_process(&f.inv, 20).is_err());
    let process = directory
        .mint_process_explicit(&f.inv, context(), 20)
        .unwrap();
    assert!(directory.bind_root(process, &f.inv, 20).is_err());
    let frame = directory.bind_root_explicit(process, &f.inv, 20).unwrap();
    assert!(directory.owner_for(frame, &f.inv, 20).is_ok());
    let mut matching = f.inv.clone();
    matching.bindings.insert(
        "mq.host-context".into(),
        binding(
            b"zos-batch|queue-manager",
            "mainframe-env.mq.host-context@1",
        ),
    );
    assert!(directory.owner_for(frame, &matching, 20).is_err());
    let mut bound = MqLifecycleDirectory::new(LifecycleLimits::default()).unwrap();
    let p = bound.mint_process(&matching, 20).unwrap();
    assert!(bound.bind_root_explicit(p, &matching, 20).is_err());
    let p = bound
        .mint_process_explicit(&matching, context(), 20)
        .unwrap();
    let matched = bound.bind_root_explicit(p, &matching, 20).unwrap();
    assert!(bound.owner_for(matched, &matching, 20).is_ok());
    for bytes in [
        b"zos-batch|host-coordinator".as_slice(),
        b"zos-ims-batch-dli|queue-manager",
        b"zos-ims|host-coordinator",
        b"zos-cics|host-coordinator",
        b"mqi-client|queue-manager",
        b"other-bindings|queue-manager",
        b"malformed",
    ] {
        let mut conflicting = f.inv.clone();
        conflicting.bindings.insert(
            "mq.host-context".into(),
            binding(bytes, "mainframe-env.mq.host-context@1"),
        );
        assert!(
            directory
                .mint_process_explicit(&conflicting, context(), 20)
                .is_err()
        );
    }
    for (name, schema, bytes) in [
        (
            "mq.host-context",
            "wrong@1",
            b"zos-batch|queue-manager".as_slice(),
        ),
        (
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            b"local",
        ),
        (
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            b"bad",
        ),
        ("cics.execution-context", "bad@1", b"local"),
        (
            crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING,
            "bad@1",
            b"bad",
        ),
        (
            crate::retention::CICS_OUTER_EFFECT_ORIGIN_BINDING,
            "bad@1",
            b"bad",
        ),
    ] {
        let mut conflicting = f.inv.clone();
        conflicting
            .bindings
            .insert(name.into(), binding(bytes, schema));
        assert!(
            directory
                .mint_process_explicit(&conflicting, context(), 20)
                .is_err()
        );
    }
    let mut cics = f.inv.clone();
    cics.bindings.insert(
        "cics.execution-context".into(),
        binding(b"local", "mainframe-env.cics.execution-context@1"),
    );
    for name in [
        crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING,
        crate::retention::CICS_OUTER_EFFECT_ORIGIN_BINDING,
    ] {
        cics.bindings.insert(name.into(), binding(b"bad", "bad@1"));
    }
    assert!(
        directory
            .mint_process_explicit(&cics, context(), 20)
            .is_err()
    );
    let mut unsupported = context();
    unsupported.owner = MqSyncpointOwner::HostCoordinator;
    assert!(
        directory
            .mint_process_explicit(&f.inv, unsupported, 20)
            .is_err()
    );
}

#[test]
fn memory_sqlite_explicit_scope_still_requires_exact_original_directory_proof_and_envelope() {
    for sqlite in [false, true] {
        let f = unbound(sqlite);
        let child = Child::new(&f);
        assert!(f.service.mint_selected_process(&f.inv).is_err());
        assert!(
            f.service
                .mint_selected_process_explicit(&child.inv, context())
                .is_err()
        );
        let e = child.effect(&f, 10, connect_request());
        child.seed(&f, &e);
        let original = e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap();
        let scope = MqMqiServiceScope::for_host_dispatch(
            &child.inv,
            f.owner,
            original,
            &f.provider,
            HostLimits::default(),
        );
        assert!(admit_mqi(&scope, &child.inv, 20).is_err());
        let proof = {
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            s.runtime
                .as_ref()
                .unwrap()
                .directory
                .context_for(child.binding.frame(), &child.inv, 20)
                .unwrap()
        };
        let mut forged = child.inv.clone();
        forged.audit_correlation = "changed".into();
        let scope = MqMqiServiceScope::for_directory_dispatch(
            &forged,
            f.owner,
            e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
            &f.provider,
            HostLimits::default(),
            proof,
        );
        assert!(admit_mqi(&scope, &forged, 20).is_err());
        let rows = f.rows();
        let mut conflicting = e.clone();
        let HostRequest::MqMqi(h) = &mut conflicting.request else {
            panic!()
        };
        h.envelope.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
        assert!(child.execute(&f, &conflicting).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn memory_sqlite_explicit_same_task_child_refuses_foreign_stale_probe_and_parent_intent() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = unbound(sqlite);
            let child = Child::new(&f);
            let rows = f.rows();
            if case == 0 {
                let foreign = unbound(sqlite);
                assert!(
                    foreign
                        .service
                        .prepare_selected_batch_child(
                            f.frame,
                            &f.inv,
                            &child.inv,
                            InstalledBatchRelationship::SameTaskCall
                        )
                        .is_err()
                );
            } else if case == 1 {
                f.service.abort_selected_batch_child(child.binding).unwrap();
                assert!(
                    f.service
                        .selected_batch_owner(child.binding.frame(), &child.inv)
                        .is_err()
                );
            } else if case == 2 {
                let mut changed = child.inv.clone();
                changed.cancellation_probe = Some(CancellationProbe::new());
                assert!(
                    f.service
                        .prepare_selected_batch_child(
                            f.frame,
                            &f.inv,
                            &changed,
                            InstalledBatchRelationship::SameTaskCall
                        )
                        .is_err()
                );
            } else if case == 3 {
                let e = child.effect(&f, 10, connect_request());
                f.seed(&e);
                assert!(child.execute(&f, &e).is_err());
            } else {
                f.service.abort_selected_batch_child(child.binding).unwrap();
                let mut guard = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &mut *guard else {
                    panic!()
                };
                let runtime = s.runtime.as_mut().unwrap();
                runtime
                    .directory
                    .retire_frame(f.frame, &mut runtime.handles.handles_mut())
                    .unwrap();
                drop(guard);
                assert!(
                    f.service
                        .prepare_selected_batch_child(
                            f.frame,
                            &f.inv,
                            &child.inv,
                            InstalledBatchRelationship::SameTaskCall
                        )
                        .is_err()
                );
            }
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn memory_sqlite_explicit_child_pending_work_survives_late_cancel_cas_audit_and_control_failure() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = unbound(sqlite);
            let child = Child::new(&f);
            let c = child_connect(&f, &child);
            let o = f.open(c);
            let unit = f.unit();
            child.call(
                &f,
                12,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::Local { unit }),
                },
            );
            let e = child.effect(
                &f,
                13,
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
                2 | 3 => {
                    let store = f.store.clone();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                        let (namespace, key) = if case == 2 {
                            (CATALOG_NAMESPACE, CATALOG_KEY)
                        } else {
                            (ownership::CONTROL_NAMESPACE, ownership::CONTROL_KEY)
                        };
                        let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
                        let version = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(version)).unwrap();
                    }));
                }
                _ => super::super::super::bounds::fill_audits(&f),
            }
            let mut rows = f.rows();
            if case == 2 || case == 3 {
                let namespace = if case == 2 {
                    CATALOG_NAMESPACE
                } else {
                    ownership::CONTROL_NAMESPACE
                };
                rows.iter_mut()
                    .find(|r| r.namespace == namespace)
                    .unwrap()
                    .version += 1;
            }
            let audit = f
                .store
                .audit_records(&child.inv.execution_id, 0, 256)
                .unwrap();
            assert!(child.execute(&f, &e).is_err());
            assert_eq!(f.rows(), rows);
            if case != 0 {
                assert_eq!(
                    f.store
                        .audit_records(&child.inv.execution_id, 0, 256)
                        .unwrap(),
                    audit
                );
            }
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
