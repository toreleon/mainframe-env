//! Separate revised input: actual born-owned SQLite driver, not the historical
//! handwritten outer Caller or a reopened filename as authority. Unrun0credit.
use super::*;
#[test]
fn actual_owned_file_manager_physically_reopens_and_admits_distinct_compiled_cold_root() {
    {
        let (f, source) =
            Fixture::owned_native_points(include_bytes!("../../native_rich_md1.json"));
        f.install("MQROOT", include_str!("../../point.cbl"));
        // Freeze all genuine artifacts/catalogs before the old root starts.
        f.install("MQCOLD", COLD);
        f.install("MQNEXT", NEXT);
        let original = super::super::super::native_root::original(&f);
        let old = original.clone();
        let store = f.store.clone();
        let artifacts = f.artifacts.clone();
        let old_host = f.mq.clone();
        let manager = f.mq.install_native_root_manager().unwrap();
        let recorded = Arc::new(Mutex::new(None));
        let slot = recorded.clone();
        let mut once = false;
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if once
                || store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .is_empty()
            {
                return;
            }
            once = true;
            let custody = manager.cutoff_live_root(&old).unwrap();
            let capture = custody.captured().clone();
            let before = original_core(&*store, &capture);
            let fresh = manager
                .prepare_owned_sqlite_successor(custody, source.clone())
                .unwrap();
            assert!(
                source.get_execution(&old.execution_id).is_err(),
                "actual originating SQLx pool closed"
            );
            let store = fresh.store.clone();
            assert!(!Arc::ptr_eq(&fresh, &old_host));
            assert!(!Arc::ptr_eq(&fresh.store, &old_host.store));
            assert!(Arc::ptr_eq(&fresh.authorizer, &old_host.authorizer));
            assert!(!Arc::ptr_eq(&fresh.control, &old_host.control));
            let (router, program, factory) = router_for(&fresh, store.clone(), artifacts.clone());
            let admitted = router
                .cobol
                .preflight_installed_program("MQNEXT", true)
                .unwrap();
            let mut next = old.clone();
            next.execution_id =
                ExecutionId::new("actual-cold-successor", Default::default()).unwrap();
            next.run_unit_id =
                RunUnitId::new("actual-cold-successor-run", Default::default()).unwrap();
            next.idempotency_key =
                IdempotencyKey::new("actual-cold-successor-key", Default::default()).unwrap();
            next.selector = Selector::new("program:MQNEXT", Default::default()).unwrap();
            next.artifact = admitted.artifact;
            let exact = next.clone();
            let outcome = router
                .execute_native_mq_root(&fresh, &next, "MQNEXT")
                .unwrap();
            let ExecutionOutcome::Completed(done) = outcome else {
                panic!("actual independent compiled successor: {outcome:?}")
            };
            assert_eq!(done.output.bytes(), b"DONE    \n");
            assert_eq!(next, exact);
            assert_eq!(original_core(&*store, &capture), before);
            assert_eq!(factory.children.lock().unwrap().len(), 1);
            let child = factory.children.lock().unwrap()[0].clone();
            assert_eq!(child.parent_execution_id.as_ref(), Some(&next.execution_id));
            assert_eq!(child.run_unit_id, next.run_unit_id);
            assert_eq!(child.selector.as_str(), "program:MQCOLD");
            assert_eq!(
                store
                    .get_execution(&child.execution_id)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Completed
            );
            assert_eq!(
                store
                    .get_execution(&next.execution_id)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Completed
            );
            let root = store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, next.execution_id.as_str())
                .unwrap()
                .unwrap();
            let doc: serde_json::Value = serde_json::from_slice(&root.payload).unwrap();
            assert_eq!(doc["schema"], "mainframe-env.core-root-ownership@2");
            assert_eq!(doc["phase"], 2);
            assert_eq!(doc["actors"].as_array().unwrap().len(), 2);
            assert_eq!(
                store
                    .get_provider_state(ROOT_DRIVER_NAMESPACE, old.execution_id.as_str())
                    .unwrap(),
                Some(capture.fenced.clone())
            );
            assert_eq!(
                store
                    .get_provider_state(ROOT_CUSTODY_NAMESPACE, old.execution_id.as_str())
                    .unwrap()
                    .unwrap()
                    .version,
                2
            );
            let handoff = old_host.runtime.native_custody_handoff_rows();
            for dependency in &capture.preserved.provider_dependencies {
                let mainframe_env_store_api::TerminalRowDependency::Exact(row) = dependency else {
                    continue;
                };
                let actual = store
                    .get_provider_state(&row.namespace, &row.key)
                    .unwrap()
                    .unwrap();
                if row.namespace == "durable-root-scope-v1" {
                    if actual != *row {
                        assert_eq!(actual.version, row.version + 1);
                        assert_eq!(actual.payload, next.execution_id.as_str().as_bytes());
                    }
                } else if let Some(index) = handoff.iter().position(|identity| {
                    identity.namespace == row.namespace && identity.key == row.key
                }) {
                    assert!(actual.version > row.version);
                    if index != 0 {
                        assert_eq!(actual.payload, row.payload);
                    } else {
                        // Source-defined cold SelectedRuntime::new advances
                        // precisely one registry epoch; identity stays exact.
                        let retained: serde_json::Value =
                            serde_json::from_slice(&row.payload).unwrap();
                        let advanced: serde_json::Value =
                            serde_json::from_slice(&actual.payload).unwrap();
                        assert!(control_is_exact_next_incarnation(&retained, &advanced));
                    }
                } else {
                    assert_eq!(actual, *row);
                }
            }
            let receipts = store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap();
            assert_eq!(
                receipts.len(),
                2,
                "one actual old and one actual new cold CONNECT only"
            );
            let values: Vec<serde_json::Value> = receipts
                .iter()
                .map(|row| {
                    serde_json::from_slice::<serde_json::Value>(&row.payload).unwrap()["value"]
                        .clone()
                })
                .collect();
            assert_eq!(
                values
                    .iter()
                    .filter(|row| row["execution"] == old.execution_id.as_str())
                    .count(),
                1
            );
            assert_eq!(
                values
                    .iter()
                    .filter(|row| row["execution"] == child.execution_id.as_str())
                    .count(),
                1
            );
            let call = store
                .list_provider_state("cobol-call-replay@1", 128)
                .unwrap();
            assert_eq!(call.len(), 1);
            let physical: serde_json::Value = serde_json::from_slice(&call[0].payload).unwrap();
            assert_eq!(physical["schema_version"], 2);
            assert_eq!(physical["child_execution"], child.execution_id.as_str());
            assert_eq!(physical["owner_execution"], next.execution_id.as_str());
            assert_eq!(physical["owner_run_unit"], next.run_unit_id.as_str());
            let expected =
                mainframe_env_interpreter::encode_cobol_call_result(&[b"DONE    ".to_vec()])
                    .unwrap();
            assert_eq!(
                physical["reply"],
                serde_json::json!({"schema": expected.schema(), "bytes": expected.bytes()})
            );
            drop(program);
            let provider_epoch = store.provider_state_retention_epoch().unwrap();
            *slot.lock().unwrap() = Some((capture, before, next, child, provider_epoch, store));
        }));
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        *f.clock.1.lock().unwrap() = None; // Drop the owned manager/clock hook cycle.
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
            "{outcome:?}"
        );
        {
            let observation = recorded.lock().unwrap();
            let (capture, before, _, _, provider_epoch, store) =
                observation.as_ref().expect("actual cold control completed");
            assert_eq!(&original_core(&**store, capture), before);
            assert_eq!(
                store.provider_state_retention_epoch().unwrap(),
                *provider_epoch
            );
            assert_eq!(
                store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                store
                    .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
                    .unwrap(),
                Some(capture.fenced.clone())
            );
        }
        assert!(
            !f.saf
                .resources
                .lock()
                .unwrap()
                .iter()
                .any(|resource| resource.class
                    == mainframe_env_host_api::EnterpriseResourceClass::MqQueue)
        );
        let map = f.mq.topology.lock().unwrap();
        let RootEntry::Retained { root, frame, .. } =
            map.roots.get(&original.execution_id).unwrap()
        else {
            panic!("actual old root");
        };
        assert!(root.native_authority_revoked());
        assert!(root.frame().context().is_err());
        assert!(
            frame::Observation(frame.clone())
                .abi_scope(&original)
                .is_err()
        );
    }
}

fn all_fixture_rows(
    store: &dyn mainframe_env_store_api::PlatformStore,
) -> Vec<mainframe_env_store_api::ProviderStateRecord> {
    let mut rows = vec![];
    // Complete source-created provider namespaces of this fixed native fixture.
    for prefix in ["batch-", "cobol-", "durable-", "mq-"] {
        rows.extend(store.list_provider_state_prefix(prefix, 4096).unwrap());
    }
    rows
}
#[test]
fn actual_manager_refuses_same_filename_copied_rows_and_other_owned_driver() {
    for mode in 0..3 {
        let (f, source) =
            Fixture::owned_native_points(include_bytes!("../../native_rich_md1.json"));
        f.install("MQROOT", include_str!("../../point.cbl"));
        let original = super::super::super::native_root::original(&f);
        let old = original.clone();
        let store = f.store.clone();
        let path = f.root.0.join("configured.db");
        let manager = f.mq.install_native_root_manager().unwrap();
        let saved = Arc::new(Mutex::new(None));
        let slot = saved.clone();
        let mut once = false;
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if once
                || store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .is_empty()
            {
                return;
            }
            once = true;
            let custody = manager.cutoff_live_root(&old).unwrap();
            let capture = custody.captured().clone();
            let before = original_core(&*store, &capture);
            let rows = all_fixture_rows(&*store);
            let epoch = store.provider_state_retention_epoch().unwrap();
            let target = path.with_file_name("substitute.db");
            let foreign = match mode {
                0 => Arc::new(
                    mainframe_env_store::SqliteStateStore::open(
                        &format!("sqlite://{}?mode=rwc", path.display()),
                        64 << 20,
                        65536,
                    )
                    .unwrap(),
                ),
                1 => {
                    std::fs::copy(&path, &target).unwrap();
                    Arc::new(
                        mainframe_env_store::SqliteStateStore::open(
                            &format!("sqlite://{}?mode=rwc", target.display()),
                            64 << 20,
                            65536,
                        )
                        .unwrap(),
                    )
                }
                2 => Arc::new(
                    mainframe_env_store::SqliteStateStore::create_owned_file(
                        &target,
                        64 << 20,
                        65536,
                    )
                    .unwrap(),
                ),
                _ => unreachable!(),
            };
            let foreign_before = all_fixture_rows(&*foreign);
            if mode != 2 {
                assert_eq!(
                    foreign_before, rows,
                    "identical physical rows are insufficient custody"
                );
            }
            let result = manager.prepare_owned_sqlite_successor(custody, foreign.clone());
            assert!(matches!(result, Err(HostProblem::Unauthorized)));
            assert_eq!(all_fixture_rows(&*foreign), foreign_before);
            assert_eq!(all_fixture_rows(&*store), rows);
            assert_eq!(original_core(&*store, &capture), before);
            assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
            assert!(
                source.get_execution(&old.execution_id).is_ok(),
                "refused substituted owner never closes actual pool"
            );
            *slot.lock().unwrap() = Some((capture, before, rows, epoch));
        }));
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        *f.clock.1.lock().unwrap() = None;
        assert!(
            matches!(outcome,ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome()),
            "{outcome:?}"
        );
        let saved = saved.lock().unwrap();
        let (capture, before, rows, epoch) = saved.as_ref().unwrap();
        assert_eq!(&original_core(&*f.store, capture), before);
        assert_eq!(&all_fixture_rows(&*f.store), rows);
        assert_eq!(f.store.provider_state_retention_epoch().unwrap(), *epoch);
        assert_eq!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .len(),
            1
        );
        let map = f.mq.topology.lock().unwrap();
        let RootEntry::Retained { root, frame, .. } =
            map.roots.get(&original.execution_id).unwrap()
        else {
            panic!("actual retained old native owner");
        };
        assert!(root.native_authority_revoked());
        assert!(root.frame().context().is_err());
        assert!(
            frame::Observation(frame.clone())
                .abi_scope(&original)
                .is_err()
        );
    }
}
