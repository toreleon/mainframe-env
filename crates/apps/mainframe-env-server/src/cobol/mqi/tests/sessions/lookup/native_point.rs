//! Transport containment fixtures only; genuine selected routes are tested separately.
use super::*;
use mainframe_env_host_api::mq_mqi::MqMqiCall;
use mainframe_env_host_api::mq_object_route::{MqRouteLookup, MqRouteName, MqRouteOpenAccess};
use mainframe_env_host_api::mq_raw_layout::{
    MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_host_api::mq_wire_options::{MqWireBindings, MqWireQueueManagerPlatform};
use mainframe_env_interpreter::{MqMqiNativePoint, MqMqiNativePointTarget, MqMqiNativeStructure};
use std::sync::atomic::AtomicBool;

#[derive(Default)]
struct Signals {
    calls: Mutex<Vec<&'static str>>,
    panic: AtomicBool,
    barriers: Mutex<Option<(Arc<Barrier>, Arc<Barrier>)>>,
}
impl Signals {
    fn record(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
    fn check(&self, name: &'static str) -> Result<(), HostProblem> {
        self.record(name);
        if let Some((entered, release)) = self.barriers.lock().unwrap().take() {
            entered.wait();
            release.wait();
        }
        assert!(
            !self.panic.load(Ordering::Acquire),
            "native observation panic"
        );
        Ok(())
    }
}
struct Observation(Arc<Signals>);
impl MqMqiNativeStructure for Observation {
    fn encoding(&self) -> MqRawStructureEncoding {
        self.0.record("encoding");
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::NormalBigEndian,
            characters: MqRawCharacterEncoding::AsciiCompatible,
        }
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        self.0.check("structure-check")
    }
    fn point(&self, _: &MqMqiNativePointTarget) -> Result<Arc<dyn MqMqiNativePoint>, HostProblem> {
        self.0.record("point");
        Ok(Arc::new(Observation(self.0.clone())))
    }
}
impl MqMqiNativePoint for Observation {
    fn recheck(&self) -> Result<(), HostProblem> {
        self.0.check("point-check")
    }
}
impl MqWireBindings for Observation {
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        self.0.record("platform");
        MqWireQueueManagerPlatform::Zos
    }
    fn queue_defaults_are_represented(
        &self,
        _: MqHconn,
        _: Option<mainframe_env_host_api::MqHobj>,
        _: Option<&MqRouteLookup>,
    ) -> bool {
        self.0.record("defaults");
        true
    }
    fn admitted_unit(&self, _: MqHconn) -> Option<MqMqiUnitOfWork> {
        self.0.record("unit");
        Some(MqMqiUnitOfWork::Local { unit: 73 })
    }
    fn existing_cursor(&self, _: MqHconn, _: mainframe_env_host_api::MqHobj) -> Option<u64> {
        self.0.record("cursor");
        None
    }
    fn milliseconds_to_ticks(&self, _: u32) -> Option<u64> {
        self.0.record("ticks");
        None
    }
}
struct NativeFrame {
    original: Invocation,
    signals: Arc<Signals>,
}
impl MqMqiProgramFrame for NativeFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        Frame(self.original.clone()).profile(invocation)
    }
    fn native_structure(
        &self,
        _: &Invocation,
        _: MqMqiCall,
        _: MqHconn,
    ) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
        self.signals.record("capture");
        Ok(Arc::new(Observation(self.signals.clone())))
    }
}
fn target() -> MqMqiNativePointTarget {
    MqMqiNativePointTarget::Open {
        lookup: MqRouteLookup::Queue {
            name: MqRouteName::new("Q").unwrap(),
            manager: None,
            dynamic_pattern: None,
        },
        access: MqRouteOpenAccess::Output,
    }
}

#[test]
fn escaped_native_profiles_revoke_on_finish_abort_and_drop_without_callbacks() {
    for mode in 0..3 {
        let (original, connection, _) = fixture();
        let signals = Arc::new(Signals::default());
        let (mut guard, events, aborts) = session_for(Arc::new(NativeFrame {
            original: original.clone(),
            signals: signals.clone(),
        }));
        let frame = guard.frame(&original).unwrap();
        let mut changed = original.clone();
        changed.deadline_tick -= 1;
        assert!(matches!(
            frame.native_structure(&changed, MqMqiCall::Open, connection),
            Err(HostProblem::Unauthorized)
        ));
        assert!(signals.calls.lock().unwrap().is_empty());
        let structure = frame
            .native_structure(&original, MqMqiCall::Open, connection)
            .unwrap();
        let point = structure.point(&target()).unwrap();
        assert_eq!(
            signals.calls.lock().unwrap().as_slice(),
            &[
                "capture",
                "encoding",
                "structure-check",
                "point",
                "platform",
                "point-check"
            ]
        );
        match mode {
            0 => guard.finish(&ExecutionOutcome::Cancelled).unwrap(),
            1 => assert_eq!(guard.abort(HostProblem::Malformed), HostProblem::Malformed),
            _ => {}
        }
        drop(guard);
        let before = signals.calls.lock().unwrap().clone();
        // Immutable scalars are cached observations, never authorization.
        assert_eq!(
            structure.encoding().characters,
            MqRawCharacterEncoding::AsciiCompatible
        );
        assert_eq!(
            point.queue_manager_platform(),
            MqWireQueueManagerPlatform::Zos
        );
        assert_eq!(structure.recheck(), Err(HostProblem::Unauthorized));
        assert_eq!(point.recheck(), Err(HostProblem::Unauthorized));
        assert!(!point.queue_defaults_are_represented(connection, None, None));
        assert!(point.admitted_unit(connection).is_none());
        assert!(point.milliseconds_to_ticks(1).is_none());
        assert!(matches!(
            structure.point(&target()),
            Err(HostProblem::Unauthorized)
        ));
        assert_eq!(*signals.calls.lock().unwrap(), before);
        assert_eq!(events.lock().unwrap().len(), usize::from(mode == 0));
        assert_eq!(aborts.lock().unwrap().len(), usize::from(mode == 1));
    }
}

#[test]
fn native_profile_panic_revokes_transport_without_cleanup_or_retry() {
    let (original, connection, _) = fixture();
    let signals = Arc::new(Signals::default());
    let (guard, events, aborts) = session_for(Arc::new(NativeFrame {
        original: original.clone(),
        signals: signals.clone(),
    }));
    let frame = guard.frame(&original).unwrap();
    let structure = frame
        .native_structure(&original, MqMqiCall::Open, connection)
        .unwrap();
    let point = structure.point(&target()).unwrap();
    signals.panic.store(true, Ordering::Release);
    assert_eq!(point.recheck(), Err(HostProblem::UnknownOutcome));
    assert_eq!(point.recheck(), Err(HostProblem::Unauthorized));
    assert_eq!(structure.recheck(), Err(HostProblem::Unauthorized));
    assert_eq!(frame.profile(&original), Err(HostProblem::Unauthorized));
    drop(guard);
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}

#[test]
fn in_flight_point_observation_cannot_escape_concurrent_transport_drop() {
    let (original, connection, _) = fixture();
    let signals = Arc::new(Signals::default());
    let (guard, events, aborts) = session_for(Arc::new(NativeFrame {
        original: original.clone(),
        signals: signals.clone(),
    }));
    let frame = guard.frame(&original).unwrap();
    let structure = frame
        .native_structure(&original, MqMqiCall::Open, connection)
        .unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    *signals.barriers.lock().unwrap() = Some((entered.clone(), release.clone()));
    let worker = std::thread::spawn(move || structure.point(&target()).map(|_| ()));
    entered.wait();
    drop(guard);
    release.wait();
    assert_eq!(worker.join().unwrap(), Err(HostProblem::Unauthorized));
    assert_eq!(frame.profile(&original), Err(HostProblem::Unauthorized));
    assert!(events.lock().unwrap().is_empty());
    assert!(aborts.lock().unwrap().is_empty());
}
