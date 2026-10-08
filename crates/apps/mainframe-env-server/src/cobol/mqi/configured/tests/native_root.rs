//! Actual compiled root tests; setup rows are the existing provider fixture only.
//! Connection-only proof earns no pending PUT/native-complete PUT credit.
use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_store_api::*;

const ROOT: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 DISPLAY 'BAD-CONN' END-IF. DISPLAY 'ROOT-DONE'. GOBACK.";

pub(super) fn original(f: &Fixture) -> Invocation {
    let admitted = f
        .router
        .cobol
        .preflight_installed_program("MQROOT", true)
        .unwrap();
    let v = &f.parent;
    let limits = InvocationLimits::default();
    let mut original = Invocation::new(
        v.request_id.clone(),
        v.execution_id.clone(),
        v.run_unit_id.clone(),
        None,
        Selector::new("program:MQROOT", limits).unwrap(),
        admitted.artifact,
        v.principal.clone(),
        v.service_class,
        v.priority,
        v.deadline_tick,
        v.trace_id.clone(),
        v.idempotency_key.clone(),
        v.attempt,
        v.limits,
        v.bindings.clone(),
        limits,
    )
    .unwrap()
    .with_provider_generations(v.provider_generations.clone(), limits)
    .unwrap();
    original.cancellation_probe = v.cancellation_probe.clone();
    // Host setup precedes freezing; the producer never modifies this original.
    crate::cobol::bind_compatible_runtime_services(&mut original).unwrap();
    original
}

#[test]
fn compiled_native_root_connection_normal_and_known_abend_publish_both_terminal_audits() {
    for sqlite in [false, true] {
        for abnormal in [false, true] {
            let f = Fixture::new(sqlite);
            let source = if abnormal {
                ROOT.replace("DISPLAY 'ROOT-DONE'. GOBACK.", "CALL 'CEE3ABD'. GOBACK.")
            } else {
                ROOT.into()
            };
            f.install("MQROOT", &source);
            let original = original(&f);
            let before = original.clone();
            let outcome = f
                .router
                .execute_native_mq_root(&f.mq, &original, "MQROOT")
                .unwrap();
            assert!(
                if abnormal {
                    matches!(outcome, ExecutionOutcome::Abend(_))
                } else {
                    matches!(outcome, ExecutionOutcome::Completed(_))
                },
                "{outcome:?}"
            );
            assert_eq!(original, before);
            assert!(!original.bindings.contains_key("mq.host-context"));
            let execution = f
                .store
                .get_execution(&original.execution_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                execution.state,
                if abnormal {
                    ExecutionState::Failed
                } else {
                    ExecutionState::Completed
                }
            );
            let events = f.store.events(&original.execution_id, 1, 128).unwrap();
            assert_eq!(events.last().unwrap().sequence, execution.version);
            assert!(if abnormal {
                matches!(events.last().unwrap().kind, LifecycleEventKind::Abend)
            } else {
                matches!(
                    events.last().unwrap().kind,
                    LifecycleEventKind::Completed { return_code: 0 }
                )
            });
            let notifications = f.store.pending_notifications(128).unwrap();
            assert_eq!(notifications.len(), events.len());
            for event in &events {
                let found: Vec<_> = notifications
                    .iter()
                    .filter(|n| {
                        n.execution_id == event.execution_id && n.sequence == event.sequence
                    })
                    .collect();
                assert_eq!(found.len(), 1);
                assert_eq!(
                    found[0].payload,
                    lifecycle_notification_payload(&event.kind)
                );
            }
            let subjects = f
                .store
                .audit_subject_records(&original.execution_id, 128)
                .unwrap();
            let terminal: Vec<_> = subjects
                .iter()
                .filter_map(|s| match s {
                    AuditSubjectRecord::RootTerminal(a) => Some(a),
                    _ => None,
                })
                .collect();
            assert_eq!(terminal.len(), 2);
            assert_eq!(terminal[0].role, RootTerminalAuditRole::ProviderSettlement);
            assert_eq!(terminal[1].role, RootTerminalAuditRole::CoreClosure);
            assert_eq!(terminal[0].resource, terminal[1].resource);
            assert!(terminal.iter().all(|a| a.observed_tick == 20
                && a.principal == *original.principal.id()
                && a.decision == AuditDecision::Success));
            assert!(
                subjects
                    .iter()
                    .filter(|s| matches!(s, AuditSubjectRecord::Effect(_)))
                    .count()
                    >= 2
            );
            let owners = f
                .store
                .list_provider_state("mq-selected-v1-uow-owner", 128)
                .unwrap();
            assert_eq!(owners.len(), 1, "no post-terminal fresh unit");
            let owner: serde_json::Value = serde_json::from_slice(&owners[0].payload).unwrap();
            assert_eq!(owner["value"]["execution"], original.execution_id.as_str());
            assert_eq!(
                owner["value"]["state"],
                if abnormal { "rolled-back" } else { "committed" }
            );
            assert!(f.saf.resources.lock().unwrap().len() >= 2);
            let root = {
                let map = f.mq.topology.lock().unwrap();
                match map.roots.get(&original.execution_id).unwrap() {
                    RootEntry::Retained {
                        root,
                        native: Some(_),
                        ..
                    } => root.clone(),
                    _ => panic!("native root"),
                }
            };
            assert!(root.frame().context().is_err());
            let frozen = (f.rows(), events, notifications, subjects);
            assert!(
                matches!(f.router.execute_native_mq_root(&f.mq, &original, "MQROOT"),
                Ok(ExecutionOutcome::ProviderFailure(ref problem)) if problem.has_unknown_outcome())
            );
            assert_eq!(
                (
                    f.rows(),
                    f.store.events(&original.execution_id, 1, 128).unwrap(),
                    f.store.pending_notifications(128).unwrap(),
                    f.store
                        .audit_subject_records(&original.execution_id, 128)
                        .unwrap()
                ),
                frozen
            );
            if sqlite {
                let reopened =
                    mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
                assert_eq!(
                    reopened.get_execution(&original.execution_id).unwrap(),
                    Some(execution)
                );
                assert_eq!(
                    reopened
                        .audit_subject_records(&original.execution_id, 128)
                        .unwrap(),
                    frozen.3
                );
            }
        }
    }
}

#[test]
fn compiled_native_root_enrolls_original_call_child_and_closes_cobol_with_same_mq_owner() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install(
            "MQLEAF",
            &ROOT.replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
        );
        f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. DISPLAY 'PARENT-DONE'. GOBACK.");
        let original = original(&f);
        let frozen = original.clone();
        let result = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(result, ExecutionOutcome::Completed(_)),
            "{result:?}"
        );
        assert_eq!(original, frozen);
        let children = f.factory.children.lock().unwrap();
        assert_eq!(children.len(), 1);
        let child = children[0].clone();
        assert_eq!(
            child.parent_execution_id.as_ref(),
            Some(&original.execution_id)
        );
        assert_eq!(
            f.store
                .get_execution(&child.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let rows = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 128)
            .unwrap();
        assert_eq!(rows.len(), 1);
        let receipt: serde_json::Value = serde_json::from_slice(&rows[0].payload).unwrap();
        assert_eq!(receipt["value"]["execution"], child.execution_id.as_str());
        let owner: serde_json::Value = serde_json::from_slice(
            &f.store
                .list_provider_state("mq-selected-v1-uow-owner", 128)
                .unwrap()[0]
                .payload,
        )
        .unwrap();
        assert_eq!(owner["value"]["execution"], original.execution_id.as_str());
        assert_eq!(owner["value"]["state"], "committed");
        assert_eq!(owner["value"]["connection_key"], receipt["value"]["key"]);
        let run = f
            .store
            .get_provider_state(
                crate::cobol::retention::RUN_STATE_NAMESPACE,
                &crate::cobol::instance::run_key(&original),
            )
            .unwrap()
            .unwrap();
        let run: serde_json::Value = serde_json::from_slice(&run.payload).unwrap();
        assert_eq!(run["ended"], true);
        assert_eq!(run["active"], 0);
        assert_eq!(run["instances"], 0);
        let instance_scope = format!(
            "{}{}",
            crate::cobol::retention::INSTANCE_NAMESPACE_PREFIX,
            crate::cobol::instance::run_key(&original)
        );
        assert!(
            f.store
                .list_provider_state(&instance_scope, 128)
                .unwrap()
                .is_empty()
        );
        let subjects = f
            .store
            .audit_subject_records(&original.execution_id, 128)
            .unwrap();
        assert_eq!(
            subjects
                .iter()
                .filter(|s| matches!(s, AuditSubjectRecord::RootTerminal(_)))
                .count(),
            2
        );
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .profile(&child)
                .is_err()
        );
        if sqlite {
            let db = mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                db.audit_subject_records(&original.execution_id, 128)
                    .unwrap(),
                subjects
            );
            assert!(
                db.list_provider_state(&instance_scope, 128)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[test]
fn compiled_native_root_terminal_cancel_or_physical_epoch_race_retains_without_settlement() {
    for sqlite in [false, true] {
        for cancel in [false, true] {
            let f = Fixture::new(sqlite);
            f.install("MQROOT", ROOT);
            let original = original(&f);
            let probe = original.cancellation_probe.clone().unwrap();
            let store = f.store.clone();
            let execution = original.execution_id.clone();
            let mut triggered = false;
            *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                let row = store
                    .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.as_str())
                    .unwrap()
                    .unwrap();
                let ownership: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                if triggered || ownership["phase"] != 1 {
                    return;
                }
                triggered = true;
                if cancel {
                    probe.request();
                } else {
                    store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "test-unrelated-terminal-race".into(),
                                key: "changed".into(),
                                version: 1,
                                payload: vec![7],
                            },
                            None,
                        )
                        .unwrap();
                }
            }));
            let result = f
                .router
                .execute_native_mq_root(&f.mq, &original, "MQROOT")
                .unwrap();
            assert!(
                matches!(result, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
                "{result:?}"
            );
            let execution = f
                .store
                .get_execution(&original.execution_id)
                .unwrap()
                .unwrap();
            assert_eq!(execution.state, ExecutionState::Running);
            let root = f
                .store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
                .unwrap()
                .unwrap();
            let root: serde_json::Value = serde_json::from_slice(&root.payload).unwrap();
            assert_eq!(root["phase"], 3);
            let owners = f
                .store
                .list_provider_state("mq-selected-v1-uow-owner", 128)
                .unwrap();
            assert_eq!(owners.len(), 1);
            let owner: serde_json::Value = serde_json::from_slice(&owners[0].payload).unwrap();
            assert_eq!(owner["value"]["state"], "pending");
            let subjects = f
                .store
                .audit_subject_records(&original.execution_id, 128)
                .unwrap();
            assert!(
                subjects
                    .iter()
                    .all(|subject| !matches!(subject, AuditSubjectRecord::RootTerminal(_)))
            );
            let root = {
                let map = f.mq.topology.lock().unwrap();
                match map.roots.get(&original.execution_id).unwrap() {
                    RootEntry::Retained { root, .. } => root.clone(),
                    _ => panic!("actual retained root"),
                }
            };
            assert!(root.frame().context().is_err());
            let frozen = (
                f.rows(),
                f.store.events(&original.execution_id, 1, 128).unwrap(),
                f.store.pending_notifications(128).unwrap(),
                subjects,
            );
            let repeated = f.router.execute_native_mq_root(&f.mq, &original, "MQROOT");
            assert!(
                match &repeated {
                    Err(HostProblem::Unauthorized) if cancel => true,
                    Ok(ExecutionOutcome::ProviderFailure(problem)) => problem.has_unknown_outcome(),
                    _ => false,
                },
                "{repeated:?}"
            );
            assert_eq!(
                (
                    f.rows(),
                    f.store.events(&original.execution_id, 1, 128).unwrap(),
                    f.store.pending_notifications(128).unwrap(),
                    f.store
                        .audit_subject_records(&original.execution_id, 128)
                        .unwrap()
                ),
                frozen
            );
            if sqlite {
                let reopened =
                    mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
                assert_eq!(
                    reopened.get_execution(&original.execution_id).unwrap(),
                    Some(execution)
                );
                assert_eq!(
                    reopened
                        .get_provider_state("mq-selected-v1-uow-owner", &owners[0].key)
                        .unwrap(),
                    Some(owners[0].clone())
                );
            }
        }
    }
}

#[test]
fn compiled_native_root_terminal_saf_denial_retains_original_unit_without_terminal_publication() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQROOT", ROOT);
        let original = original(&f);
        let frozen = original.clone();
        let store = f.store.clone();
        let execution = original.execution_id.clone();
        let saf = Arc::downgrade(&f.saf);
        *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
            let row = store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.as_str())
                .unwrap()
                .unwrap();
            let ownership: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            if ownership["phase"] == 1 {
                saf.upgrade()
                    .unwrap()
                    .deny
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }));
        let result = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(result, ExecutionOutcome::ProviderFailure(ref problem)
            if problem.has_unknown_outcome()),
            "{result:?}"
        );
        assert_eq!(original, frozen);
        assert!(f.saf.deny.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            f.saf
                .resources
                .lock()
                .unwrap()
                .iter()
                .any(
                    |resource| resource.class == EnterpriseResourceClass::MqUnitOfWork
                        && resource.name.as_str() == "CURRENT"
                )
        );
        let execution = f
            .store
            .get_execution(&original.execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.state, ExecutionState::Running);
        let owners = f
            .store
            .list_provider_state("mq-selected-v1-uow-owner", 128)
            .unwrap();
        assert_eq!(owners.len(), 1);
        let owner: serde_json::Value = serde_json::from_slice(&owners[0].payload).unwrap();
        assert_eq!(owner["value"]["state"], "pending");
        let subjects = f
            .store
            .audit_subject_records(&original.execution_id, 128)
            .unwrap();
        assert!(
            subjects
                .iter()
                .all(|subject| !matches!(subject, AuditSubjectRecord::RootTerminal(_)))
        );
        let root = f
            .store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
            .unwrap()
            .unwrap();
        let document: serde_json::Value = serde_json::from_slice(&root.payload).unwrap();
        assert_eq!(document["phase"], 3);
        let notifications = f.store.pending_notifications(128).unwrap();
        assert!(
            notifications
                .iter()
                .all(|notification| notification.sequence <= execution.version)
        );
        if sqlite {
            let reopened =
                mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened.get_execution(&original.execution_id).unwrap(),
                Some(execution)
            );
            assert_eq!(
                reopened
                    .list_provider_state("mq-selected-v1-uow-owner", 128)
                    .unwrap(),
                owners
            );
            assert_eq!(
                reopened
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap(),
                subjects
            );
            assert_eq!(reopened.pending_notifications(128).unwrap(), notifications);
        }
    }
}

#[test]
fn compiled_native_root_post_commit_cancellation_fences_reply_without_second_publication() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQROOT", ROOT);
        let original = original(&f);
        let store = f.store.clone();
        let execution = original.execution_id.clone();
        let probe = original.cancellation_probe.clone().unwrap();
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if let Some(row) = store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.as_str())
                .unwrap()
            {
                let root: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                if root["phase"] == 2 {
                    probe.request();
                }
            }
        }));
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem)
            if problem.has_unknown_outcome()),
            "{outcome:?}"
        );
        let execution = f
            .store
            .get_execution(&original.execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.state, ExecutionState::Completed);
        let subjects = f
            .store
            .audit_subject_records(&original.execution_id, 128)
            .unwrap();
        assert_eq!(
            subjects
                .iter()
                .filter(|subject| matches!(subject, AuditSubjectRecord::RootTerminal(_)))
                .count(),
            2
        );
        let root = f
            .store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
            .unwrap()
            .unwrap();
        let document: serde_json::Value = serde_json::from_slice(&root.payload).unwrap();
        assert_eq!(document["phase"], 2);
        let owners = f
            .store
            .list_provider_state("mq-selected-v1-uow-owner", 128)
            .unwrap();
        assert_eq!(owners.len(), 1);
        let owner: serde_json::Value = serde_json::from_slice(&owners[0].payload).unwrap();
        assert_eq!(owner["value"]["state"], "committed");
        let frozen = (
            f.rows(),
            f.store.events(&original.execution_id, 1, 128).unwrap(),
            f.store.pending_notifications(128).unwrap(),
            subjects,
        );
        assert_eq!(
            f.router.execute_native_mq_root(&f.mq, &original, "MQROOT"),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            (
                f.rows(),
                f.store.events(&original.execution_id, 1, 128).unwrap(),
                f.store.pending_notifications(128).unwrap(),
                f.store
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap()
            ),
            frozen
        );
        let retained = {
            let map = f.mq.topology.lock().unwrap();
            match map.roots.get(&original.execution_id).unwrap() {
                RootEntry::Retained { root, .. } => root.clone(),
                _ => panic!("retained original root"),
            }
        };
        assert!(retained.frame().context().is_err());
        if sqlite {
            let reopened =
                mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened.get_execution(&original.execution_id).unwrap(),
                Some(execution)
            );
            assert_eq!(
                reopened
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap(),
                frozen.3
            );
        }
    }
}

#[test]
fn compiled_native_root_unrepresented_child_open_is_unknown_not_known_abnormal() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let leaf = ROOT
            .replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF")
            .replace(
                "DISPLAY 'ROOT-DONE'.",
                "CALL 'MQOPEN' USING HC QM HC CC RC.",
            );
        f.install("MQLEAF", &leaf);
        f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. GOBACK.");
        let original = original(&f);
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem)
            if problem.has_unknown_outcome()),
            "{outcome:?}"
        );
        assert_eq!(f.factory.children.lock().unwrap().len(), 1);
        assert_eq!(
            f.store
                .get_execution(&original.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Running
        );
        let subjects = f
            .store
            .audit_subject_records(&original.execution_id, 128)
            .unwrap();
        assert!(
            subjects
                .iter()
                .all(|subject| !matches!(subject, AuditSubjectRecord::RootTerminal(_)))
        );
        let owners = f
            .store
            .list_provider_state("mq-selected-v1-uow-owner", 128)
            .unwrap();
        assert_eq!(owners.len(), 1);
        let owner: serde_json::Value = serde_json::from_slice(&owners[0].payload).unwrap();
        assert_eq!(owner["value"]["state"], "pending");
        if sqlite {
            let reopened =
                mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened
                    .list_provider_state("mq-selected-v1-uow-owner", 128)
                    .unwrap(),
                owners
            );
            assert_eq!(
                reopened
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap(),
                subjects
            );
        }
    }
}

#[test]
fn compiled_native_root_foreign_physical_setup_and_child_relabel_refuse_before_admission() {
    for sqlite in [false, true] {
        for (wrapped, foreign_control) in [(true, false), (false, true)] {
            let f = Fixture::registered(sqlite, 16, wrapped, foreign_control);
            f.install("MQROOT", ROOT);
            let original = original(&f);
            let frozen = f.rows();
            assert!(
                f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .is_err()
            );
            assert_eq!(f.rows(), frozen);
            assert_eq!(f.store.get_execution(&original.execution_id).unwrap(), None);
            assert!(f.saf.resources.lock().unwrap().is_empty());
        }
        let substituted_program = Fixture::foreign_program(sqlite);
        substituted_program.install("MQROOT", ROOT);
        let supplied = original(&substituted_program);
        let before = substituted_program.rows();
        assert_eq!(
            substituted_program.router.execute_native_mq_root(
                &substituted_program.mq,
                &supplied,
                "MQROOT"
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(substituted_program.rows(), before);
        assert_eq!(
            substituted_program
                .store
                .get_execution(&supplied.execution_id)
                .unwrap(),
            None
        );
        assert!(substituted_program.saf.resources.lock().unwrap().is_empty());
        let f = Fixture::new(sqlite);
        let foreign = Fixture::new(sqlite);
        f.install("MQROOT", ROOT);
        let original = original(&f);
        let frozen = f.rows();
        let other = foreign.rows();
        assert_eq!(
            f.router
                .execute_native_mq_root(&foreign.mq, &original, "MQROOT"),
            Err(HostProblem::Unauthorized)
        );
        let mut child = original.clone();
        child.parent_execution_id =
            Some(ExecutionId::new("real-parent", InvocationLimits::default()).unwrap());
        assert_eq!(
            f.router.execute_native_mq_root(&f.mq, &child, "MQROOT"),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(f.rows(), frozen);
        assert_eq!(foreign.rows(), other);
        assert_eq!(f.store.get_execution(&original.execution_id).unwrap(), None);
        assert_eq!(
            foreign.store.get_execution(&original.execution_id).unwrap(),
            None
        );
        assert!(f.saf.resources.lock().unwrap().is_empty());
        assert!(foreign.saf.resources.lock().unwrap().is_empty());
    }
}
