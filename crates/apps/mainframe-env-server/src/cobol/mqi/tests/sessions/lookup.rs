//! Read-only frame fixtures, never selected-service/UOW permission evidence.
use super::*;
use mainframe_env_host_api::{MqHconn, mq_mqi::MqMqiUnitOfWork};
use std::sync::{Barrier, atomic::AtomicUsize};
mod connx;

struct LookupFrame {
    invocation: Invocation,
    calls: Arc<Mutex<Vec<(Invocation, MqHconn)>>>,
    profiles: AtomicUsize,
    panic_profile: bool,
    panic_lookup: bool,
    barriers: Option<(Arc<Barrier>, Arc<Barrier>)>,
}
impl LookupFrame {
    fn block(&self) {
        if let Some((entered, release)) = &self.barriers {
            entered.wait();
            release.wait();
        }
    }
}
impl MqMqiProgramFrame for LookupFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.profiles.fetch_add(1, Ordering::SeqCst);
        self.block();
        assert!(!self.panic_profile, "profile fixture panic");
        Frame(self.invocation.clone()).profile(invocation)
    }
    fn local_unit(
        &self,
        invocation: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.calls
            .lock()
            .unwrap()
            .push((invocation.clone(), connection));
        self.block();
        assert!(!self.panic_lookup, "lookup fixture panic");
        Ok(MqMqiUnitOfWork::Local { unit: 73 })
    }
}
fn session_for(
    frame: Arc<dyn MqMqiProgramFrame>,
) -> (
    SessionGuard,
    Arc<Mutex<Vec<ExecutionOutcome>>>,
    Arc<Mutex<Vec<HostProblem>>>,
) {
    let events = Arc::new(Mutex::new(vec![]));
    let aborts = Arc::new(Mutex::new(vec![]));
    (
        SessionGuard::new(Box::new(Session {
            frame,
            events: events.clone(),
            aborts: aborts.clone(),
            store: Arc::new(MemoryStore::new(Default::default())),
            control: super::super::setup::control(),
        })),
        events,
        aborts,
    )
}
fn fixture() -> (Invocation, MqHconn, Arc<LookupFrame>) {
    let invocation = super::super::super::super::hardening::parent();
    let owner = Frame(invocation.clone())
        .profile(&invocation)
        .unwrap()
        .context
        .owner;
    let connection = mainframe_env_host_api::MqHandleRegistry::new(1, 2)
        .unwrap()
        .connect(owner, mainframe_env_host_api::MqHandleSharing::NonShared)
        .unwrap();
    let frame = Arc::new(LookupFrame {
        invocation: invocation.clone(),
        calls: Arc::new(Mutex::new(vec![])),
        profiles: AtomicUsize::new(0),
        panic_profile: false,
        panic_lookup: false,
        barriers: None,
    });
    (invocation, connection, frame)
}
#[test]
fn exact_invocation_live_token_and_returned_observation_are_forwarded_unchanged() {
    let (invocation, connection, inner) = fixture();
    let (guard, events, aborts) = session_for(inner.clone());
    let frame = guard.frame(&invocation).unwrap();
    assert_eq!(
        frame.local_unit(&invocation, connection),
        Ok(MqMqiUnitOfWork::Local { unit: 73 })
    );
    assert_eq!(
        inner.calls.lock().unwrap().as_slice(),
        &[(invocation.clone(), connection)]
    );
    assert_eq!(
        frame.profile(&invocation),
        Frame(invocation.clone()).profile(&invocation)
    );
    let mut changed = invocation.clone();
    changed.deadline_tick -= 1;
    assert_eq!(
        frame.local_unit(&changed, connection),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(frame.profile(&changed), Err(HostProblem::Unauthorized));
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    assert_eq!(inner.profiles.load(Ordering::SeqCst), 1);
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}
#[test]
fn finish_abort_and_drop_revoke_escaped_lookup_without_extra_callbacks() {
    for mode in 0..3 {
        let (invocation, connection, inner) = fixture();
        let (mut guard, events, aborts) = session_for(inner.clone());
        let frame = guard.frame(&invocation).unwrap();
        match mode {
            0 => guard.finish(&ExecutionOutcome::Cancelled).unwrap(),
            1 => {
                assert_eq!(guard.abort(HostProblem::Malformed), HostProblem::Malformed);
            }
            _ => {}
        }
        drop(guard);
        assert_eq!(
            frame.local_unit(&invocation, connection),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(frame.profile(&invocation), Err(HostProblem::Unauthorized));
        assert!(inner.calls.lock().unwrap().is_empty());
        assert_eq!(inner.profiles.load(Ordering::SeqCst), 0);
        assert_eq!(events.lock().unwrap().len(), usize::from(mode == 0));
        assert_eq!(aborts.lock().unwrap().len(), usize::from(mode == 1));
    }
}
#[test]
fn synchronous_invalidation_during_both_callbacks_suppresses_returned_authority() {
    for lookup in [false, true] {
        for mode in 0..3 {
            let (invocation, connection, mut inner) = fixture();
            let entered = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            Arc::get_mut(&mut inner).unwrap().barriers = Some((entered.clone(), release.clone()));
            let (mut guard, events, aborts) = session_for(inner.clone());
            let frame = guard.frame(&invocation).unwrap();
            let escaped = frame.clone();
            let caller = invocation.clone();
            let worker = std::thread::spawn(move || {
                if lookup {
                    escaped.local_unit(&caller, connection).map(|_| ())
                } else {
                    escaped.profile(&caller).map(|_| ())
                }
            });
            entered.wait();
            match mode {
                0 => guard.finish(&ExecutionOutcome::Cancelled).unwrap(),
                1 => {
                    assert_eq!(guard.abort(HostProblem::Malformed), HostProblem::Malformed);
                }
                _ => {}
            }
            drop(guard);
            release.wait();
            assert_eq!(worker.join().unwrap(), Err(HostProblem::Unauthorized));
            assert_eq!(
                frame.local_unit(&invocation, connection),
                Err(HostProblem::Unauthorized)
            );
            assert_eq!(frame.profile(&invocation), Err(HostProblem::Unauthorized));
            assert_eq!(inner.calls.lock().unwrap().len(), usize::from(lookup));
            assert_eq!(inner.profiles.load(Ordering::SeqCst), usize::from(!lookup));
            assert_eq!(events.lock().unwrap().len(), usize::from(mode == 0));
            assert_eq!(aborts.lock().unwrap().len(), usize::from(mode == 1));
        }
    }
}
#[test]
fn callback_panic_is_unknown_revokes_transport_and_never_notifies_cleanup_itself() {
    for lookup in [false, true] {
        let (invocation, connection, mut inner) = fixture();
        let configured = Arc::get_mut(&mut inner).unwrap();
        configured.panic_lookup = lookup;
        configured.panic_profile = !lookup;
        let (mut guard, events, aborts) = session_for(inner.clone());
        let frame = guard.frame(&invocation).unwrap();
        let result = if lookup {
            frame.local_unit(&invocation, connection).map(|_| ())
        } else {
            frame.profile(&invocation).map(|_| ())
        };
        assert_eq!(result, Err(HostProblem::UnknownOutcome));
        assert_eq!(
            frame.local_unit(&invocation, connection),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(frame.profile(&invocation), Err(HostProblem::Unauthorized));
        assert!(events.lock().unwrap().is_empty());
        assert!(aborts.lock().unwrap().is_empty());
        let raw = outcomes().into_iter().find(|raw| matches!(raw, ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome())).unwrap();
        guard.finish(&raw).unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), &[raw]);
        assert_eq!(
            guard.finish(&ExecutionOutcome::Cancelled),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(inner.calls.lock().unwrap().len(), usize::from(lookup));
        assert_eq!(inner.profiles.load(Ordering::SeqCst), usize::from(!lookup));
        assert!(aborts.lock().unwrap().is_empty());
    }
}
#[test]
fn older_frame_default_lookup_stays_unsupported() {
    let (invocation, connection, _) = fixture();
    let (guard, events, aborts) = session_for(Arc::new(Frame(invocation.clone())));
    let frame = guard.frame(&invocation).unwrap();
    assert_eq!(
        frame.local_unit(&invocation, connection),
        Err(HostProblem::Unsupported)
    );
    assert!(frame.profile(&invocation).is_ok());
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}
