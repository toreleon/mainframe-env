use super::*;
use mainframe_env_store::LocalArtifactStore;
use std::sync::{Barrier, mpsc};
use std::time::Duration;

fn runtime(
    root: &super::super::super::hardening::TestRoot,
) -> (
    Arc<ScopedHostService>,
    Arc<dyn PlatformStore>,
    Arc<dyn ArtifactStore>,
) {
    let registry =
        mainframe_env_host_api::RegistrySnapshot::new(1, vec![], InvocationLimits::default())
            .unwrap();
    (
        Arc::new(ScopedHostService::new(
            Arc::new(registry),
            Default::default(),
        )),
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(LocalArtifactStore::open(&root.0, 64 * 1024 * 1024).unwrap()),
    )
}

fn control() -> Arc<dyn ProgramExecutionControl> {
    Arc::new(|_: &Invocation| {
        Ok(ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        })
    })
}

#[test]
fn factory_first_serializes_runtime_publication_and_freezes_typed_control() {
    let root = super::super::super::hardening::TestRoot::new();
    let router = default_program_router();
    router.bind_execution_control(control()).unwrap();
    let (host, store, artifacts) = runtime(&root);
    let barrier = Arc::new(Barrier::new(2));
    let (sent, received) = mpsc::channel();
    // Hold the exact guard used by the factory's check/install, not a second test lock.
    let setup = router.cobol.setup.lock().unwrap();
    std::thread::scope(|scope| {
        let other = router.clone();
        let started = barrier.clone();
        scope.spawn(move || {
            started.wait();
            sent.send(other.bind_runtime(host, store, artifacts))
                .unwrap();
        });
        barrier.wait();
        assert_eq!(
            received.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(router.cobol.host.get().is_none());
        router
            .cobol
            .bind_mqi_host_locked(&setup, admission(false))
            .unwrap();
        drop(setup);
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );
    });
    assert_eq!(
        router.bind_mqi_program_host(admission(false)),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        router.bind_execution_control(control()),
        Err(HostProblem::IdempotencyConflict)
    );
    let mut invocation = super::super::super::hardening::parent();
    assert!(
        router
            .cobol
            .admit_batch_mqi(&mut invocation)
            .unwrap()
            .is_some()
    );
}

#[test]
fn runtime_first_blocks_factory_then_refuses_late_profile_but_keeps_legacy_control() {
    let root = super::super::super::hardening::TestRoot::new();
    let router = default_program_router();
    let (host, store, artifacts) = runtime(&root);
    let barrier = Arc::new(Barrier::new(2));
    let (sent, received) = mpsc::channel();
    let setup = router.cobol.setup.lock().unwrap();
    std::thread::scope(|scope| {
        let other = router.clone();
        let started = barrier.clone();
        scope.spawn(move || {
            started.wait();
            sent.send(other.bind_mqi_program_host(admission(false)))
                .unwrap();
        });
        barrier.wait();
        assert_eq!(
            received.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(router.cobol.mqi_host.get().is_none());
        router
            .cobol
            .bind_runtime_locked(&setup, host, store, artifacts)
            .unwrap();
        drop(setup);
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Err(HostProblem::IdempotencyConflict)
        );
    });
    router.bind_execution_control(control()).unwrap();
    let mut invocation = super::super::super::hardening::parent();
    let before = invocation.clone();
    assert!(
        router
            .cobol
            .admit_batch_mqi(&mut invocation)
            .unwrap()
            .is_none()
    );
    assert_eq!(invocation, before);
}

#[test]
fn first_late_control_is_refused_on_typed_runtime_and_repeated_runtime_is_unchanged() {
    let root = super::super::super::hardening::TestRoot::new();
    let router = default_program_router();
    router.bind_mqi_program_host(admission(false)).unwrap();
    let (host, store, artifacts) = runtime(&root);
    router
        .bind_runtime(host.clone(), store.clone(), artifacts.clone())
        .unwrap();
    assert_eq!(
        router.bind_execution_control(control()),
        Err(HostProblem::IdempotencyConflict)
    );
    assert!(router.cobol.control.get().is_none());
    let (other_host, other_store, other_artifacts) = runtime(&root);
    assert_eq!(
        router.bind_runtime(other_host, other_store, other_artifacts),
        Err(HostProblem::IdempotencyConflict)
    );
    assert!(Arc::ptr_eq(router.cobol.host.get().unwrap(), &host));
    assert!(Arc::ptr_eq(router.cobol.store.get().unwrap(), &store));
    assert!(Arc::ptr_eq(
        router.cobol.artifacts.get().unwrap(),
        &artifacts
    ));
}

#[test]
fn partial_setup_conflict_publishes_no_host_or_artifacts() {
    let root = super::super::super::hardening::TestRoot::new();
    let router = default_program_router();
    let occupied: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
    assert!(router.cobol.store.set(occupied.clone()).is_ok());
    let (host, store, artifacts) = runtime(&root);
    assert_eq!(
        router.bind_runtime(host, store, artifacts),
        Err(HostProblem::IdempotencyConflict)
    );
    assert!(router.cobol.host.get().is_none());
    assert!(router.cobol.artifacts.get().is_none());
    assert!(Arc::ptr_eq(router.cobol.store.get().unwrap(), &occupied));
}

struct ReentrantAdmission(std::sync::Weak<DefaultProgramRouter>);
impl ProgramMqHostAdmission for ReentrantAdmission {
    fn admit_installed_batch(
        &self,
        invocation: &Invocation,
        _: &dyn PlatformStore,
    ) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        let router = self.0.upgrade().unwrap();
        assert!(
            router.cobol.setup.try_lock().is_ok(),
            "factory callback holds no setup lock"
        );
        assert_eq!(
            router.bind_execution_control(control()),
            Err(HostProblem::IdempotencyConflict)
        );
        Ok(Arc::new(Frame(invocation.clone())))
    }
}

#[test]
fn external_factory_runs_outside_setup_guard() {
    let root = super::super::super::hardening::TestRoot::new();
    let router = default_program_router();
    router
        .bind_mqi_program_host(Arc::new(ReentrantAdmission(Arc::downgrade(&router))))
        .unwrap();
    let (host, store, artifacts) = runtime(&root);
    router.bind_runtime(host, store, artifacts).unwrap();
    let mut invocation = super::super::super::hardening::parent();
    assert!(
        router
            .cobol
            .admit_batch_mqi(&mut invocation)
            .unwrap()
            .is_some()
    );
}
