//! Engine-only compiled fixtures; no installed provider/SAF/root-terminal credit.
use super::super::tests::{Frame, context, reply};
use super::*;
use mainframe_env_host_api::{MqHandleObservation, MqHandleRegistry};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

struct Signals {
    mode: AtomicU8,
    checks: AtomicUsize,
    refuse_at: AtomicUsize,
    captures: AtomicUsize,
    targets: AtomicUsize,
}
impl Signals {
    fn check(&self) -> Result<(), HostProblem> {
        let n = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        if self.mode.load(Ordering::SeqCst) == 3 {
            panic!("private fixture callback panic");
        }
        if self.mode.load(Ordering::SeqCst) == 2 || n >= self.refuse_at.load(Ordering::SeqCst) {
            return Err(HostProblem::Unsupported);
        }
        Ok(())
    }
}
struct Native(Arc<Signals>);
impl MqMqiNativeStructure for Native {
    fn encoding(&self) -> MqRawStructureEncoding {
        MqRawStructureEncoding {
            numbers: if self.0.mode.load(Ordering::SeqCst) == 4 {
                MqRawNumberEncoding::ReversedLittleEndian
            } else {
                MqRawNumberEncoding::NormalBigEndian
            },
            characters: if self.0.mode.load(Ordering::SeqCst) == 5 {
                MqRawCharacterEncoding::OwnedCp037
            } else {
                MqRawCharacterEncoding::AsciiCompatible
            },
        }
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        self.0.check()
    }
    fn point(
        &self,
        target: &MqMqiNativePointTarget,
    ) -> Result<Arc<dyn MqMqiNativePoint>, HostProblem> {
        self.0.check()?;
        self.0.targets.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Point {
            signals: self.0.clone(),
            target: target.clone(),
        }))
    }
}
struct Point {
    signals: Arc<Signals>,
    target: MqMqiNativePointTarget,
}
impl MqMqiNativePoint for Point {
    fn recheck(&self) -> Result<(), HostProblem> {
        self.signals.check()
    }
}
impl MqWireBindings for Point {
    fn queue_defaults_are_represented(
        &self,
        _: MqHconn,
        o: Option<MqHobj>,
        q: Option<&MqRouteLookup>,
    ) -> bool {
        match &self.target {
            MqMqiNativePointTarget::Open { lookup, .. } => o.is_none() && q == Some(lookup),
            MqMqiNativePointTarget::Object(object) => o == Some(*object) && q.is_none(),
        }
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        MqWireQueueManagerPlatform::Zos
    }
    fn admitted_unit(&self, _: MqHconn) -> Option<MqMqiUnitOfWork> {
        Some(MqMqiUnitOfWork::Local { unit: 31 })
    }
    fn existing_cursor(&self, _: MqHconn, _: MqHobj) -> Option<u64> {
        None
    }
    fn milliseconds_to_ticks(&self, _: u32) -> Option<u64> {
        None
    }
}
struct PointFrame {
    frame: Frame,
    scope: Arc<MqMqiAbiScope>,
    signals: Arc<Signals>,
}
impl MqMqiProgramFrame for PointFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.frame.profile(invocation)
    }
    fn abi_scope(
        &self,
        invocation: &Invocation,
    ) -> Result<Option<Arc<MqMqiAbiScope>>, HostProblem> {
        self.frame.profile(invocation)?;
        Ok(Some(self.scope.clone()))
    }
    fn native_structure(
        &self,
        invocation: &Invocation,
        call: MqMqiCall,
        _: MqHconn,
    ) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
        self.frame.profile(invocation)?;
        self.signals.captures.fetch_add(1, Ordering::SeqCst);
        if !matches!(call, MqMqiCall::Open | MqMqiCall::Close)
            || self.signals.mode.load(Ordering::SeqCst) == 1
        {
            return Err(HostProblem::Unsupported);
        }
        Ok(Arc::new(Native(self.signals.clone())))
    }
}
fn compiled() -> ReferenceMachine {
    super::super::tests::connx::compiled_text(include_str!("tests/point.mir"))
}
fn bind(mut m: ReferenceMachine, scope: Arc<MqMqiAbiScope>) -> (ReferenceMachine, Arc<PointFrame>) {
    m.invocation.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
    let frame = Arc::new(PointFrame {
        frame: Frame {
            invocation: m.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope,
        signals: Arc::new(Signals {
            mode: AtomicU8::new(0),
            checks: AtomicUsize::new(0),
            refuse_at: AtomicUsize::new(usize::MAX),
            captures: AtomicUsize::new(0),
            targets: AtomicUsize::new(0),
        }),
    });
    m.bind_mqi_program_frame(frame.clone()).unwrap();
    (m, frame)
}
fn effect(m: &mut ReferenceMachine) -> EffectRequest {
    match m.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()) {
        MachineDrive::HostCall(effect) => effect,
        other => panic!("compiled original CALL expected {other:?}"),
    }
}
fn ok(output: MqMqiOutput) -> MqMqiOutcome {
    MqMqiOutcome::Completed {
        status: MqMqiStatus::OkNone,
        output,
    }
}
fn started(capacity: usize) -> (ReferenceMachine, Arc<PointFrame>, MqHandleRegistry, MqHconn) {
    let shared = Arc::new(MqMqiAbiScope::new(context(), capacity).unwrap());
    let (mut m, frame) = bind(compiled(), shared);
    let mut registry = MqHandleRegistry::new(1, 8).unwrap();
    let connection = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let first = effect(&mut m);
    reply(&mut m, &first, ok(MqMqiOutput::Connected(connection))).unwrap();
    (m, frame, registry, connection)
}
fn open_reply(
    m: &mut ReferenceMachine,
    e: &EffectRequest,
    object: MqHobj,
) -> Result<(), MachineProblem> {
    reply(
        m,
        e,
        ok(MqMqiOutput::Opened {
            object,
            dynamic: None,
        }),
    )
}

fn member_mut<'a>(m: &'a mut ReferenceMachine, name: &str) -> &'a mut LayoutMetadata {
    let key = m.layout(name).unwrap().name.clone();
    m.layouts.get_mut(&key).unwrap()
}

#[test]
fn genuine_compiled_conn_open_close_disc_preserves_original_effect_and_undefined_fields() {
    for (options, access) in [
        (16_i32, MqRouteOpenAccess::Output),
        (2, MqRouteOpenAccess::InputShared),
    ] {
        let (mut m, frame, mut registry, connection) = started(4);
        m.write("OPTIONS", &options.to_be_bytes()).unwrap();
        let od = m.read("OBJECT-DESC").unwrap();
        let e = effect(&mut m);
        assert_eq!(e.sequence, 2);
        assert_eq!(e.run_unit, m.invocation.run_unit_id);
        assert_eq!(e.deadline_tick, m.invocation.deadline_tick);
        let HostRequest::MqMqi(request) = &e.request else {
            panic!()
        };
        assert_eq!(request.mutation.sequence, e.sequence);
        assert_eq!(
            Some(&request.mutation.idempotency_key),
            e.idempotency_key.as_ref()
        );
        let MqMqiRequest::Open(open) = &request.envelope.request else {
            panic!()
        };
        assert_eq!(open.connection(), connection);
        assert_eq!(open.access(), &[access]);
        assert_eq!(
            open.lookup(),
            &MqRouteLookup::Queue {
                name: MqRouteName::new("ORDINARY.Q").unwrap(),
                manager: None,
                dynamic_pattern: None
            }
        );
        let object = registry.create_object(context().owner, connection).unwrap();
        assert_eq!(
            frame.scope.object(2, connection),
            Err(HostProblem::Malformed)
        );
        open_reply(&mut m, &e, object).unwrap();
        assert_eq!(m.read("HOBJ").unwrap(), 2_i32.to_be_bytes());
        assert_eq!(m.read("OBJECT-DESC").unwrap(), od);
        let hobj_bytes = m.read("HOBJ").unwrap();
        let close = effect(&mut m);
        let HostRequest::MqMqi(request) = &close.request else {
            panic!()
        };
        let MqMqiRequest::Close(value) = request.envelope.request else {
            panic!()
        };
        assert_eq!(value.connection(), connection);
        assert_eq!(
            value.target(),
            MqRouteCloseTarget::Object {
                handle: object,
                lifecycle: MqRouteCloseLifecycle::Predefined
            }
        );
        reply(&mut m, &close, ok(MqMqiOutput::NoOutput)).unwrap();
        assert_eq!(m.read("HOBJ").unwrap(), hobj_bytes);
        assert_eq!(
            frame.scope.object(2, connection),
            Err(HostProblem::Malformed)
        );
        assert_eq!(frame.scope.connection(1), Ok(connection));
        let disc = effect(&mut m);
        reply(&mut m, &disc, ok(MqMqiOutput::NoOutput)).unwrap();
        assert_eq!(frame.scope.connection(1), Err(HostProblem::Malformed));
        assert_eq!(m.read("OBJECT-DESC").unwrap(), od);
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()),
            MachineDrive::Completed(_)
        ));
        assert!(m.checkpoint().is_none());
        assert_eq!(m.snapshot().schema_version, 0);
    }
}

#[test]
fn real_compiler_group_is_required_before_original_allocation() {
    for defect in 0..12 {
        let (mut m, frame, _, _) = started(4);
        match defect {
            0 => {
                m.write("OD-ID", b"BAD ").unwrap();
            }
            1 => {
                m.write("OD-VERSION", &2_i32.to_be_bytes()).unwrap();
            }
            2 => {
                m.write("OPTIONS", &0_i32.to_be_bytes()).unwrap();
            }
            3 => {
                m.write("OPTIONS", &(-1_i32).to_be_bytes()).unwrap();
            }
            4 => {
                m.write("OPTIONS", &18_i32.to_be_bytes()).unwrap();
            }
            5 => {
                member_mut(&mut m, "OD-NAME").occurs_clause = true;
            }
            6 => {
                member_mut(&mut m, "OD-TYPE").native_binary = true;
            }
            7 => {
                member_mut(&mut m, "OD-TYPE").digits = 8;
            }
            8 => {
                let key = m.layout("OD-USER").unwrap().name.clone();
                m.views.get_mut(&key).unwrap().offset -= 1;
            }
            9 => {
                m.write("OD-MANAGER", b"FOREIGN").unwrap();
            }
            10 => {
                m.write("OD-TYPE", &2_i32.to_be_bytes()).unwrap();
            }
            11 => {
                m.write("OD-USER", b"USER").unwrap();
            }
            _ => unreachable!(),
        }
        let before = (m.bases.clone(), m.effect_sequence);
        assert!(
            super::super::tests::call(
                &mut m,
                &[
                    "MQOPEN",
                    "USING",
                    "HCONN",
                    "OBJECT-DESC",
                    "OPTIONS",
                    "HOBJ",
                    "CC",
                    "REASON"
                ]
            )
            .is_err(),
            "defect {defect}"
        );
        assert_eq!((m.bases.clone(), m.effect_sequence), before);
        frame.scope.require_context(context()).unwrap();
    }
}

#[test]
fn structure_port_precedes_raw_decode_and_unconfigured_or_foreign_encoding_refuses() {
    for mode in 1..=5 {
        let (mut m, frame, _, _) = started(4);
        frame.signals.mode.store(mode, Ordering::SeqCst);
        m.write("OD-ID", b"BAD ").unwrap();
        assert!(
            super::super::tests::call(
                &mut m,
                &[
                    "MQOPEN",
                    "USING",
                    "HCONN",
                    "OBJECT-DESC",
                    "OPTIONS",
                    "HOBJ",
                    "CC",
                    "REASON"
                ]
            )
            .is_err()
        );
        assert_eq!(m.effect_sequence, 1);
        assert_eq!(frame.signals.captures.load(Ordering::SeqCst), 1);
        assert_eq!(frame.signals.targets.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn every_late_argument_or_member_drift_fences_without_partial_bytes_or_adoption() {
    for target in [
        "HCONN",
        "OBJECT-DESC",
        "OPTIONS",
        "HOBJ",
        "CC",
        "REASON",
        "OD-SUFFIX",
    ] {
        let (mut m, frame, mut registry, c) = started(4);
        let e = effect(&mut m);
        let object = registry.create_object(context().owner, c).unwrap();
        let mut bytes = m.read(target).unwrap();
        bytes[0] ^= 1;
        m.write(target, &bytes).unwrap();
        let before = m.bases.clone();
        assert_eq!(
            open_reply(&mut m, &e, object),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome))
        );
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.object(2, c), Err(HostProblem::UnknownOutcome));
    }
    let (mut m, frame, mut registry, c) = started(4);
    let e = effect(&mut m);
    member_mut(&mut m, "OD-NAME").parent = Some("OD-ID".into());
    let before = m.bases.clone();
    assert!(
        open_reply(
            &mut m,
            &e,
            registry.create_object(context().owner, c).unwrap()
        )
        .is_err()
    );
    assert_eq!(m.bases, before);
    assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
}

#[test]
fn capacity_failure_is_predispatch_and_failed_open_burns_only_reservation() {
    let (mut m, _, _, _) = started(1);
    assert_eq!(
        super::super::tests::call(
            &mut m,
            &[
                "MQOPEN",
                "USING",
                "HCONN",
                "OBJECT-DESC",
                "OPTIONS",
                "HOBJ",
                "CC",
                "REASON"
            ]
        ),
        Err(MachineProblem::Host(HostProblem::ResourceExhausted))
    );
    assert_eq!(m.effect_sequence, 1);
    let (mut m, frame, mut registry, c) = started(2);
    let before = m.read("HOBJ").unwrap();
    let e = effect(&mut m);
    reply(
        &mut m,
        &e,
        MqMqiOutcome::ReviewedStatus {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Open, 2, 2085).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(m.read("HOBJ").unwrap(), before);
    assert_eq!(m.read("REASON").unwrap(), 2085_i32.to_be_bytes());
    assert_eq!(frame.scope.object(2, c), Err(HostProblem::Malformed));
    let e = super::super::tests::call(
        &mut m,
        &[
            "MQOPEN",
            "USING",
            "HCONN",
            "OBJECT-DESC",
            "OPTIONS",
            "HOBJ",
            "CC",
            "REASON",
        ],
    )
    .unwrap();
    open_reply(
        &mut m,
        &e,
        registry.create_object(context().owner, c).unwrap(),
    )
    .unwrap();
    assert_eq!(m.read("HOBJ").unwrap(), 3_i32.to_be_bytes());
}

#[test]
fn unknown_historical_wrong_output_and_postdispatch_callback_failure_fence() {
    for defect in 0..7 {
        let (mut m, frame, mut registry, c) = started(4);
        let e = effect(&mut m);
        let live = registry.create_object(context().owner, c).unwrap();
        let outcome = match defect {
            0 => MqMqiOutcome::UnknownOutcome,
            1 => ok(MqMqiOutput::Opened {
                object: MqHandleObservation::from(mainframe_env_host_api::MqHandle::Object(live))
                    .historical_object()
                    .unwrap(),
                dynamic: None,
            }),
            2 => ok(MqMqiOutput::Connected(c)),
            3 => MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_wire_pair(MqMqiCall::Open, 0, 0).unwrap(),
            },
            4 => {
                frame.signals.mode.store(2, Ordering::SeqCst);
                ok(MqMqiOutput::Opened {
                    object: live,
                    dynamic: None,
                })
            }
            5 => {
                frame.signals.mode.store(3, Ordering::SeqCst);
                ok(MqMqiOutput::Opened {
                    object: live,
                    dynamic: None,
                })
            }
            6 => {
                frame.frame.changed.store(true, Ordering::SeqCst);
                ok(MqMqiOutput::Opened {
                    object: live,
                    dynamic: None,
                })
            }
            _ => unreachable!(),
        };
        let before = m.bases.clone();
        assert_eq!(
            reply(&mut m, &e, outcome),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome))
        );
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn final_profile_refusal_aborted_close_and_cancel_or_drop_never_retire_or_write() {
    for defect in 0..4 {
        let (mut m, frame, mut registry, c) = started(4);
        let e = effect(&mut m);
        let object = registry.create_object(context().owner, c).unwrap();
        open_reply(&mut m, &e, object).unwrap();
        let close = effect(&mut m);
        let before = m.bases.clone();
        if defect == 0 {
            // Fail at the final point recheck, after encoding all writes.
            frame.signals.refuse_at.store(
                frame.signals.checks.load(Ordering::SeqCst) + 4,
                Ordering::SeqCst,
            );
        } else if defect == 1 {
            m.invocation.cancellation_probe.as_ref().unwrap().request();
        } else if defect == 2 {
            drop(m);
            assert_eq!(frame.scope.object(2, c), Err(HostProblem::UnknownOutcome));
            continue;
        } else {
            m.write("CLOSE-OPTIONS", &1_i32.to_be_bytes()).unwrap();
        }
        let preserved = m.bases.clone();
        assert!(reply(&mut m, &close, ok(MqMqiOutput::NoOutput)).is_err());
        assert_eq!(m.bases, preserved);
        if defect != 3 {
            assert_eq!(m.bases, before);
        }
        assert_eq!(frame.scope.object(2, c), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn separately_compiled_same_task_child_closes_parent_object_without_own_connection() {
    let (mut parent, frame, mut registry, c) = started(4);
    let e = effect(&mut parent);
    let object = registry.create_object(context().owner, c).unwrap();
    open_reply(&mut parent, &e, object).unwrap();
    let mut child = super::super::tests::connx::compiled_text(include_str!("tests/child.mir"));
    child.invocation.execution_id =
        mainframe_env_execution_api::ExecutionId::new("same-task-point-child", Default::default())
            .unwrap();
    child.invocation.parent_execution_id = Some(parent.invocation.execution_id.clone());
    let (mut child, _) = bind(child, frame.scope.clone());
    let e = effect(&mut child);
    let HostRequest::MqMqi(request) = &e.request else {
        panic!()
    };
    let MqMqiRequest::Close(close) = request.envelope.request else {
        panic!()
    };
    assert_eq!(close.connection(), c);
    assert_eq!(
        close.target(),
        MqRouteCloseTarget::Object {
            handle: object,
            lifecycle: MqRouteCloseLifecycle::Predefined
        }
    );
    assert_eq!(e.run_unit, child.invocation.run_unit_id);
    reply(&mut child, &e, ok(MqMqiOutput::NoOutput)).unwrap();
    assert_eq!(frame.scope.object(2, c), Err(HostProblem::Malformed));
    assert_eq!(frame.scope.connection(1), Ok(c));
    assert_eq!(parent.read("HOBJ").unwrap(), 2_i32.to_be_bytes());
    drop(child);
    frame.scope.require_context(context()).unwrap();
    assert!(
        super::super::tests::call(
            &mut parent,
            &[
                "MQCLOSE",
                "USING",
                "HCONN",
                "HOBJ",
                "CLOSE-OPTIONS",
                "CC",
                "REASON"
            ]
        )
        .is_err()
    );
}

#[test]
fn close_failure_preserves_alias_and_bad_reference_or_family_never_dispatches() {
    let (mut m, frame, mut registry, c) = started(4);
    let e = effect(&mut m);
    let object = registry.create_object(context().owner, c).unwrap();
    open_reply(&mut m, &e, object).unwrap();
    let e = effect(&mut m);
    let before = m.read("HOBJ").unwrap();
    reply(
        &mut m,
        &e,
        MqMqiOutcome::ReviewedStatus {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Close, 2, 2019).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(m.read("HOBJ").unwrap(), before);
    assert_eq!(frame.scope.object(2, c), Ok(object));
    for args in [
        vec![
            "MQCLOSE",
            "USING",
            "BY",
            "VALUE",
            "HCONN",
            "HOBJ",
            "CLOSE-OPTIONS",
            "CC",
            "REASON",
        ],
        vec![
            "MQCLOSE",
            "USING",
            "HCONN",
            "HCONN",
            "CLOSE-OPTIONS",
            "CC",
            "REASON",
        ],
        vec![
            "MQOPEN",
            "USING",
            "HOBJ",
            "OBJECT-DESC",
            "OPTIONS",
            "HCONN",
            "CC",
            "REASON",
        ],
        vec![
            "MQOPEN",
            "USING",
            "HCONN",
            "OBJECT-DESC",
            "OPTIONS",
            "HOBJ",
            "HOBJ",
            "REASON",
        ],
    ] {
        let before = (m.bases.clone(), m.effect_sequence);
        assert!(super::super::tests::call(&mut m, &args).is_err());
        assert_eq!((m.bases.clone(), m.effect_sequence), before);
    }
}

#[test]
fn genuine_compiled_copy_wrapper_keeps_prefix_suffix_and_adoption_atomic() {
    let shared = Arc::new(MqMqiAbiScope::new(context(), 3).unwrap());
    let (mut m, frame) = bind(
        super::super::tests::connx::compiled_text(include_str!("tests/wrapped.mir")),
        shared,
    );
    let mut registry = MqHandleRegistry::new(1, 4).unwrap();
    let connection = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let e = effect(&mut m);
    reply(&mut m, &e, ok(MqMqiOutput::Connected(connection))).unwrap();
    let before = m.read("OBJECT-DESC").unwrap();
    let e = effect(&mut m);
    let object = registry.create_object(context().owner, connection).unwrap();
    open_reply(&mut m, &e, object).unwrap();
    assert_eq!(m.read("OBJECT-DESC").unwrap(), before);
    assert_eq!(m.read("OD-SUFFIX").unwrap(), b"TAIL0123");
    assert_eq!(frame.scope.object(2, connection), Ok(object));
}
