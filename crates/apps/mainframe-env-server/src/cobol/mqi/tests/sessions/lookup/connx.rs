//! CONNX observation transport only, not an installed ABI or lifecycle permit.
use super::*;
use mainframe_env_host_api::mq_raw_layout::{
    MqConnxProfile, MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_interpreter::MqMqiConnxProfile;

struct ConnxLookup {
    original: Invocation,
    calls: Mutex<Vec<Invocation>>,
    panic: bool,
    barriers: Option<(Arc<Barrier>, Arc<Barrier>)>,
}
fn observation() -> MqMqiConnxProfile {
    // The wrapper preserves even a profile the compiled interpreter would
    // refuse; it does not normalize or independently select structure encoding.
    MqMqiConnxProfile {
        profile: MqConnxProfile::OrdinaryOwnedNonshared,
        encoding: MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::ReversedLittleEndian,
            characters: MqRawCharacterEncoding::OwnedCp037,
        },
    }
}
impl MqMqiProgramFrame for ConnxLookup {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        Frame(self.original.clone()).profile(invocation)
    }
    fn connx_profile(&self, invocation: &Invocation) -> Result<MqMqiConnxProfile, HostProblem> {
        self.calls.lock().unwrap().push(invocation.clone());
        if let Some((entered, release)) = &self.barriers {
            entered.wait();
            release.wait();
        }
        assert!(!self.panic, "fixture CONNX callback panic");
        Ok(observation())
    }
}
fn inner(original: &Invocation) -> Arc<ConnxLookup> {
    Arc::new(ConnxLookup {
        original: original.clone(),
        calls: Mutex::new(vec![]),
        panic: false,
        barriers: None,
    })
}

#[test]
fn exact_original_and_explicit_structure_profile_are_forwarded_without_normalization() {
    let (original, _, _) = fixture();
    let inner = inner(&original);
    let (guard, events, aborts) = session_for(inner.clone());
    let frame = guard.frame(&original).unwrap();
    assert_eq!(frame.connx_profile(&original), Ok(observation()));
    let mut changed = original.clone();
    changed.deadline_tick -= 1;
    assert_eq!(
        frame.connx_profile(&changed),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(inner.calls.lock().unwrap().as_slice(), &[original]);
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}

#[test]
fn concurrent_finish_abort_and_drop_suppress_late_connx_observations() {
    for mode in 0..3 {
        let (original, _, _) = fixture();
        let mut inner = inner(&original);
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        Arc::get_mut(&mut inner).unwrap().barriers = Some((entered.clone(), release.clone()));
        let (mut guard, events, aborts) = session_for(inner.clone());
        let frame = guard.frame(&original).unwrap();
        let escaped = frame.clone();
        let caller = original.clone();
        let worker = std::thread::spawn(move || escaped.connx_profile(&caller));
        entered.wait();
        match mode {
            0 => guard.finish(&ExecutionOutcome::Cancelled).unwrap(),
            1 => assert_eq!(guard.abort(HostProblem::Malformed), HostProblem::Malformed),
            _ => {}
        }
        drop(guard);
        release.wait();
        assert_eq!(worker.join().unwrap(), Err(HostProblem::Unauthorized));
        assert_eq!(
            frame.connx_profile(&original),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(inner.calls.lock().unwrap().as_slice(), &[original]);
        assert_eq!(events.lock().unwrap().len(), usize::from(mode == 0));
        assert_eq!(aborts.lock().unwrap().len(), usize::from(mode == 1));
    }
}

#[test]
fn connx_callback_panic_revokes_all_transport_without_a_cleanup_callback() {
    let (original, connection, _) = fixture();
    let mut inner = inner(&original);
    Arc::get_mut(&mut inner).unwrap().panic = true;
    let (guard, events, aborts) = session_for(inner.clone());
    let frame = guard.frame(&original).unwrap();
    assert_eq!(
        frame.connx_profile(&original),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        frame.connx_profile(&original),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(frame.profile(&original), Err(HostProblem::Unauthorized));
    assert_eq!(
        frame.local_unit(&original, connection),
        Err(HostProblem::Unauthorized)
    );
    drop(guard);
    assert_eq!(inner.calls.lock().unwrap().as_slice(), &[original]);
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}

#[test]
fn older_embeddings_remain_explicitly_unsupported_for_connx() {
    let (original, _, _) = fixture();
    let (guard, events, aborts) = session_for(Arc::new(Frame(original.clone())));
    let frame = guard.frame(&original).unwrap();
    assert_eq!(
        frame.connx_profile(&original),
        Err(HostProblem::Unsupported)
    );
    assert!(frame.profile(&original).is_ok());
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}
