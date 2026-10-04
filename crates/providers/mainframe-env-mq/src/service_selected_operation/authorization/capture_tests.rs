//! Collection-slot tests, not live MQ, host or SAF execution acceptance.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Delegate {
    decision: Result<(), HostProblem>,
    calls: AtomicUsize,
}
impl EnterpriseAuthorizer for Delegate {
    fn authorize(
        &self,
        _: &mainframe_env_execution_api::PrincipalId,
        _: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.decision.clone()
    }
}
fn delegate(decision: Result<(), HostProblem>) -> Delegate {
    Delegate {
        decision,
        calls: AtomicUsize::new(0),
    }
}
fn principal() -> mainframe_env_execution_api::PrincipalId {
    mainframe_env_execution_api::PrincipalId::new("USER", InvocationLimits::default()).unwrap()
}
fn resource() -> EnterpriseResource {
    EnterpriseResource::new(
        EnterpriseResourceClass::MqUnitOfWork,
        "CURRENT",
        AccessIntent::Read,
    )
    .unwrap()
}
#[test]
fn capture_delegate_success_and_error_are_actual_not_inferred() {
    for decision in [
        Ok(()),
        Err(HostProblem::Unauthorized),
        Err(HostProblem::ProviderFailure),
    ] {
        let delegate = delegate(decision.clone());
        let capture = Capture::new(&delegate);
        assert_eq!(capture.authorize(&principal(), &resource()), decision);
        assert_eq!(delegate.calls.load(Ordering::SeqCst), 1);
        let state = capture.resources.lock().unwrap();
        assert_eq!(state.reserved, 0);
        assert_eq!(state.recorded.len(), usize::from(decision.is_ok()));
        drop(state);
        if decision.is_ok() {
            assert_eq!(capture.into_resources().unwrap().len(), 1);
        } else {
            assert_eq!(capture.into_resources(), Err(HostProblem::Malformed));
        }
    }
}
#[test]
fn capture_callback_lock_free_and_nested_collection_is_bounded() {
    let delegate = delegate(Ok(()));
    let capture = Capture::new(&delegate);
    capture
        .capture(&resource(), || {
            let state = capture
                .resources
                .try_lock()
                .expect("callback held capture mutex");
            assert_eq!((state.reserved, state.recorded.len()), (1, 0));
            drop(state);
            capture.capture(&resource(), || {
                let state = capture
                    .resources
                    .try_lock()
                    .expect("nested capture held mutex");
                assert_eq!((state.reserved, state.recorded.len()), (2, 0));
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(capture.into_resources().unwrap().len(), 2);
    // This test exercises the same callback helper as actual authorize above;
    // a collection callback alone is not a delegate SAF decision or MQ permit.
    assert_eq!(delegate.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn capture_callback_panic_returns_reserved_slot_without_poison_or_record() {
    let delegate = delegate(Ok(()));
    let capture = Capture::new(&delegate);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = capture.capture(&resource(), || panic!("bounded collection callback"));
        }))
        .is_err()
    );
    let state = capture.resources.lock().unwrap();
    assert_eq!((state.reserved, state.recorded.len()), (0, 0));
    drop(state);
    capture.authorize(&principal(), &resource()).unwrap();
    assert_eq!(capture.into_resources().unwrap().len(), 1);
}
#[test]
fn capture_concurrent_max_plus_one_refuses_before_delegate_and_no_slot_leak() {
    let delegate = delegate(Ok(()));
    let capture = Capture::new(&delegate);
    for _ in 0..(MAX_RESOURCES - 1) {
        capture.authorize(&principal(), &resource()).unwrap();
    }
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let worker_capture = &capture;
        let worker = scope.spawn(move || {
            worker_capture.capture(&resource(), || {
                entered.send(()).unwrap();
                // Bounded diagnostic only: no selected/frame/backend authority
                // is held, and release never needs the Capture collection lock.
                released.recv_timeout(Duration::from_secs(2)).unwrap();
                Err(HostProblem::Unauthorized)
            })
        });
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let calls = delegate.calls.load(Ordering::SeqCst);
        assert_eq!(
            capture.authorize(&principal(), &resource()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(delegate.calls.load(Ordering::SeqCst), calls);
        assert!(capture.resources.try_lock().is_ok());
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Err(HostProblem::Unauthorized));
    });
    capture.authorize(&principal(), &resource()).unwrap();
    assert_eq!(
        capture.authorize(&principal(), &resource()),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(capture.into_resources().unwrap().len(), MAX_RESOURCES);
}
