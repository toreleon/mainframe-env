//! Actual compiled new root/CALL controls, separate from original cold fixture.
//! Same physical store Arc only: no fresh-file reopen or pending PUT/GET credit.
#[cfg(unix)]
mod physical_file;
use super::*;
use crate::cobol::{DefaultProgramRouter, default_program_router};
use mainframe_env_execution_api::{IdempotencyKey, RunUnitId, Selector};
use mainframe_env_host_api::{HostProvider, RegistrySnapshot, ScopedHostService};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, ROOT_DRIVER_NAMESPACE, RootCustodyCutoff};
use std::sync::atomic::AtomicBool;

pub(super) const COLD: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQCOLD. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'DONE' TO OUTCOME. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME END-IF. GOBACK.";
pub(super) const NEXT: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQNEXT. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUTCOME PIC X(8) VALUE SPACES. PROCEDURE DIVISION. CALL 'MQCOLD' USING OUTCOME. DISPLAY OUTCOME. GOBACK.";

pub(super) fn router_for(
    mq: &Arc<ConfiguredInstalledMqHost>,
    store: Arc<dyn mainframe_env_store_api::PlatformStore>,
    artifacts: Arc<LocalArtifactStore>,
) -> (
    Arc<DefaultProgramRouter>,
    Arc<dyn HostProvider>,
    Arc<RecordingFactory>,
) {
    let router = default_program_router();
    let factory = Arc::new(RecordingFactory {
        host: mq.clone(),
        children: Mutex::new(vec![]),
        observations: Mutex::new(vec![]),
        hook: Mutex::new(None),
        nested_hook: Mutex::new(None),
        override_host: Mutex::new(None),
        fail_preparation: AtomicBool::new(false),
        drop_at_admission: AtomicBool::new(false),
    });
    router
        .bind_execution_control(mq.execution_control())
        .unwrap();
    router.bind_mqi_program_host(factory.clone()).unwrap();
    let program = router.native_root_program_provider().unwrap();
    router.bind_native_mq_root_host(mq).unwrap();
    let selected: Arc<dyn HostProvider> = mq.clone();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, vec![selected, program.clone()], Default::default()).unwrap(),
        ),
        Default::default(),
    ));
    router.bind_runtime(host, store, artifacts).unwrap();
    (router, program, factory)
}
type OriginalCore = (
    Vec<Option<mainframe_env_store_api::ExecutionRecord>>,
    Vec<Option<mainframe_env_store_api::EffectRecord>>,
    Vec<mainframe_env_execution_api::LifecycleEvent>,
    Vec<mainframe_env_store_api::OutboxRecord>,
    Vec<AuditSubjectRecord>,
);

pub(super) fn original_core(
    store: &dyn mainframe_env_store_api::PlatformStore,
    cutoff: &RootCustodyCutoff,
) -> OriginalCore {
    let mut actors = vec![];
    let mut effects = vec![];
    let mut events = vec![];
    let mut audits = vec![];
    for actor in &cutoff.preserved.actors {
        let id = &actor.execution.execution_id;
        actors.push(store.get_execution(id).unwrap());
        for effect in &actor.effects {
            effects.push(store.effect(&effect.key).unwrap());
        }
        events.extend(store.events(id, 1, 256).unwrap());
        audits.extend(store.audit_subject_records(id, 256).unwrap());
    }
    let outbox = store
        .pending_notifications(256)
        .unwrap()
        .into_iter()
        .filter(|row| {
            cutoff
                .preserved
                .actors
                .iter()
                .any(|actor| actor.execution.execution_id == row.execution_id)
        })
        .collect();
    (actors, effects, events, outbox, audits)
}
// Source: ObjectRow<Control> restore (rows.rs130-136), not a bare Control.
pub(super) fn control_is_exact_next_incarnation(
    retained: &serde_json::Value,
    advanced: &serde_json::Value,
) -> bool {
    for outer in [retained, advanced] {
        if outer["schema_version"] != "mainframe-env.mq-object-row@1"
            || outer["object_key"] != "state"
            || outer["value"]["schema_version"] != "mainframe-env.mq-selected-control@1"
        {
            return false;
        }
    }
    let old = &retained["value"];
    let new = &advanced["value"];
    let positive =
        |value: &serde_json::Value| value.as_u64().filter(|v| *v > 0 && *v <= i64::MAX as u64);
    let Some(epoch) = positive(&old["registry_epoch"]) else {
        return false;
    };
    let Some(expected) = epoch.checked_add(1).filter(|v| *v <= i64::MAX as u64) else {
        return false;
    };
    for key in ["generation", "fence"] {
        if positive(&old[key]).is_none() || positive(&new[key]).is_none() || new[key] != old[key] {
            return false;
        }
    }
    positive(&new["registry_epoch"]) == Some(expected)
}
#[test]
fn cold_control_oracle_refuses_bare_control_wrong_wrapper_and_wrong_incarnation() {
    // Source-shaped independently literal ObjectRow and Control, not observed output.
    let old = serde_json::json!({"schema_version":"mainframe-env.mq-object-row@1","object_key":"state","value":{"schema_version":"mainframe-env.mq-selected-control@1","generation":1,"fence":1,"registry_epoch":1,"next_unit":2}});
    let new = serde_json::json!({"schema_version":"mainframe-env.mq-object-row@1","object_key":"state","value":{"schema_version":"mainframe-env.mq-selected-control@1","generation":1,"fence":1,"registry_epoch":2,"next_unit":3}});
    assert!(control_is_exact_next_incarnation(&old, &new));
    assert!(!control_is_exact_next_incarnation(
        &old["value"],
        &new["value"]
    ));
    let mut max_old = old.clone();
    let mut max_new = new.clone();
    max_old["value"]["registry_epoch"] = (i64::MAX as u64 - 1).into();
    max_new["value"]["registry_epoch"] = (i64::MAX as u64).into();
    assert!(control_is_exact_next_incarnation(&max_old, &max_new));
    max_old["value"]["registry_epoch"] = (i64::MAX as u64).into();
    max_new["value"]["registry_epoch"] = (i64::MAX as u64 + 1).into();
    assert!(!control_is_exact_next_incarnation(&max_old, &max_new));
    for key in ["generation", "fence", "registry_epoch"] {
        for invalid in [
            serde_json::Value::Null,
            "1".into(),
            true.into(),
            0.into(),
            (i64::MAX as u64 + 1).into(),
            u64::MAX.into(),
        ] {
            for replace_old in [false, true] {
                let mut left = old.clone();
                let mut right = new.clone();
                {
                    let row = if replace_old { &mut left } else { &mut right };
                    row["value"][key] = invalid.clone();
                }
                assert!(
                    !control_is_exact_next_incarnation(&left, &right),
                    "{key}/{invalid}/{replace_old}"
                );
                {
                    let row = if replace_old { &mut left } else { &mut right };
                    row["value"].as_object_mut().unwrap().remove(key);
                }
                assert!(
                    !control_is_exact_next_incarnation(&left, &right),
                    "missing {key}/{replace_old}"
                );
            }
        }
    }
    // Refuse the previous None==None bug, including missing/malformed new epoch.
    let mut overflow = old.clone();
    overflow["value"]["registry_epoch"] = u64::MAX.into();
    for invalid in [serde_json::Value::Null, "2".into(), true.into()] {
        let mut right = new.clone();
        right["value"]["registry_epoch"] = invalid;
        assert!(!control_is_exact_next_incarnation(&overflow, &right));
        right["value"]
            .as_object_mut()
            .unwrap()
            .remove("registry_epoch");
        assert!(!control_is_exact_next_incarnation(&overflow, &right));
    }
    for changed in [
        "outer-schema",
        "outer-key",
        "inner-schema",
        "generation",
        "fence",
        "epoch-same",
        "epoch-skip",
    ] {
        for replace_old in [false, true] {
            let mut left = old.clone();
            let mut right = new.clone();
            let row = if replace_old { &mut left } else { &mut right };
            match changed {
                "outer-schema" => row["schema_version"] = "mainframe-env.mq-object-row@2".into(),
                "outer-key" => row["object_key"] = "other".into(),
                "inner-schema" => {
                    row["value"]["schema_version"] = "mainframe-env.mq-selected-control@2".into()
                }
                "generation" => row["value"]["generation"] = 2.into(),
                "fence" => row["value"]["fence"] = 2.into(),
                "epoch-same" => {
                    row["value"]["registry_epoch"] = if replace_old { 2.into() } else { 1.into() }
                }
                "epoch-skip" => row["value"]["registry_epoch"] = 4.into(),
                _ => unreachable!(),
            }
            assert!(
                !control_is_exact_next_incarnation(&left, &right),
                "{changed}/{replace_old}"
            );
        }
    }
}

#[test]
fn actual_manager_admits_distinct_compiled_root_and_cold_child_preserving_unknown_predecessor() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install("MQROOT", include_str!("../point.cbl"));
        // Freeze all genuine artifacts/catalogs before the old root starts.
        f.install("MQCOLD", COLD);
        f.install("MQNEXT", NEXT);
        let original = super::super::native_root::original(&f);
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
            let fresh = manager.prepare_successor_host(custody).unwrap();
            assert!(!Arc::ptr_eq(&fresh, &old_host));
            assert!(Arc::ptr_eq(&fresh.store, &old_host.store));
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
            *slot.lock().unwrap() = Some((capture, before, next, child, provider_epoch));
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
            let (capture, before, _, _, provider_epoch) =
                observation.as_ref().expect("actual cold control completed");
            assert_eq!(&original_core(&*f.store, capture), before);
            assert_eq!(
                f.store.provider_state_retention_epoch().unwrap(),
                *provider_epoch
            );
            assert_eq!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                f.store
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
