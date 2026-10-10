//! Actual compiled original-root cutoff; no cold successor/pending PUT/GET claim.
use super::*;
use mainframe_env_execution_api::ExecutionOutcome;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, ROOT_DRIVER_NAMESPACE};

#[test]
fn actual_live_compiled_root_cutoff_revokes_old_authority_without_reentering_clock_or_service() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install("MQROOT", include_str!("../point.cbl"));
        let original = super::super::native_root::original(&f);
        let manager = f.mq.install_native_root_manager().unwrap();
        assert!(f.mq.install_native_root_manager().is_err());
        let store = f.store.clone();
        let supplied = original.clone();
        let captured = Arc::new(Mutex::new(None));
        let slot = captured.clone();
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
            let cutoff = manager.cutoff_live_root(&supplied).unwrap();
            assert_eq!(cutoff.original(), &supplied);
            assert_eq!(
                cutoff.captured().preserved.actors[0].execution.state,
                ExecutionState::Running
            );
            assert!(manager.cutoff_live_root(&supplied).is_err());
            *slot.lock().unwrap() = Some(cutoff);
        }));
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        *f.clock.1.lock().unwrap() = None; // Release the manager/clock hook cycle after the owned drive.
        assert!(
            matches!(outcome,ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome()),
            "{outcome:?}"
        );
        let cutoff = captured.lock().unwrap();
        let cutoff = cutoff
            .as_ref()
            .expect("actual same-clock hook reached cutoff");
        assert!(cutoff.same_host(&f.mq));
        let captured = cutoff.captured();
        assert_eq!(
            f.store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
                .unwrap(),
            Some(captured.fenced.clone())
        );
        assert_eq!(
            f.store
                .get_provider_state(ROOT_CUSTODY_NAMESPACE, original.execution_id.as_str())
                .unwrap(),
            Some(captured.marker.clone())
        );
        for dependency in &captured.preserved.provider_dependencies {
            match dependency {
                mainframe_env_store_api::TerminalRowDependency::Exact(row) => assert_eq!(
                    f.store
                        .get_provider_state(&row.namespace, &row.key)
                        .unwrap(),
                    Some(row.clone())
                ),
                mainframe_env_store_api::TerminalRowDependency::Absent { namespace, key } => {
                    assert!(
                        f.store
                            .get_provider_state(namespace, key)
                            .unwrap()
                            .is_none()
                    )
                }
            }
        }
        assert_eq!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .len(),
            1
        );
        assert!(
            !f.saf
                .resources
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.class == mainframe_env_host_api::EnterpriseResourceClass::MqQueue)
        );
        let map = f.mq.topology.lock().unwrap();
        let RootEntry::Retained { root, frame, .. } =
            map.roots.get(&original.execution_id).unwrap()
        else {
            panic!("actual native root");
        };
        assert!(root.native_authority_revoked());
        assert!(root.frame().context().is_err());
        assert!(
            frame::Observation(frame.clone())
                .abi_scope(&original)
                .is_err()
        );
        drop(map);
        let rows = f.rows();
        let audits = f
            .store
            .audit_subject_records(&original.execution_id, 128)
            .unwrap();
        let saf = f.saf.resources.lock().unwrap().clone();
        assert!(
            matches!(f.router.execute_native_mq_root(&f.mq,&original,"MQROOT"),Ok(ExecutionOutcome::ProviderFailure(ref p)) if p.has_unknown_outcome())
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(
            f.store
                .audit_subject_records(&original.execution_id, 128)
                .unwrap(),
            audits
        );
        assert_eq!(*f.saf.resources.lock().unwrap(), saf);
    }
}
