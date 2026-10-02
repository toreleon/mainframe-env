//! Actual compiler/CALL/coordinator/selected-provider tests, not licensed credit.
use super::*;
mod bounds;
mod flows;
mod lifecycle;
mod nested;
mod refusals;
mod setup;

#[test]
fn rejects_absent_or_legacy_selected_state_without_initialization_on_memory_sqlite() {
    struct Clock;
    impl MqReplayClock for Clock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            Ok(1)
        }
    }
    struct Saf;
    impl EnterpriseAuthorizer for Saf {
        fn authorize(
            &self,
            _: &mainframe_env_execution_api::PrincipalId,
            _: &mainframe_env_host_api::EnterpriseResource,
        ) -> Result<(), HostProblem> {
            Ok(())
        }
    }
    let d = CapabilityDescriptor {
        capability: mainframe_env_execution_api::CapabilityId::new(
            "host.mq.write",
            Default::default(),
        )
        .unwrap(),
        provider_id: "mainframe-env-mq".into(),
        generation: "configured-test".into(),
        request_schema: "mainframe-env.host-request@1".into(),
        result_schema: "mainframe-env.host-result@1".into(),
        max_request_bytes: 8 << 20,
        max_result_bytes: 8 << 20,
        ready: true,
    };
    for sqlite in [false, true] {
        for legacy in [false, true] {
            let root = crate::cobol::hardening::TestRoot::new();
            let store: Arc<dyn PlatformStore> = if sqlite {
                Arc::new(
                    mainframe_env_store::SqliteStateStore::open(
                        &format!("sqlite://{}?mode=rwc", root.0.join("strict.db").display()),
                        64 << 20,
                        65536,
                    )
                    .unwrap(),
                )
            } else {
                Arc::new(mainframe_env_store::MemoryStore::new(Default::default()))
            };
            if legacy {
                setup::legacy_fixture(store.as_ref());
            }
            let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
            assert!(
                ConfiguredInstalledMqHost::open(
                    store.clone(),
                    Arc::new(Saf),
                    Arc::new(Clock),
                    d.clone(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    3,
                    5,
                    InstalledMqHostBounds {
                        max_roots: 4,
                        max_frames: 8
                    }
                )
                .is_err()
            );
            assert_eq!(
                store.list_provider_state_prefix("mq-", 4096).unwrap(),
                before
            );
        }
    }
}

#[test]
fn strict_existing_rich_generation_and_fence_mismatch_refuse_without_writes_or_saf() {
    for sqlite in [false, true] {
        let f = setup::Fixture::new(sqlite);
        let before = f.rows();
        for (generation, fence) in [(4, 5), (3, 6)] {
            assert!(
                ConfiguredInstalledMqHost::open(
                    f.store.clone(),
                    f.saf.clone(),
                    f.clock.clone(),
                    setup::descriptor(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    generation,
                    fence,
                    InstalledMqHostBounds {
                        max_roots: 8,
                        max_frames: 16
                    },
                )
                .is_err()
            );
            assert_eq!(f.rows(), before);
            assert!(f.saf.resources.lock().unwrap().is_empty());
        }
    }
}
