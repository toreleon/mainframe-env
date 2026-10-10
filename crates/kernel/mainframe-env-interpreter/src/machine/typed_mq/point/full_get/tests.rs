//! Actual compiled sources/private frame replies: engine evidence ONLY.
use super::*;
use crate::machine::typed_mq::tests::{Frame, context, reply};
use mainframe_env_host_api::mq_mqi::{MqFullMessage, MqMqiQualifiedGot};
use mainframe_env_host_api::{MqGetDisposition, MqHandleRegistry, MqTruncationDisposition as T};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

struct Signals {
    mode: AtomicUsize,
    checks: AtomicUsize,
    refuse_at: AtomicUsize,
    version: AtomicI32,
    maximum: AtomicUsize,
    captures: AtomicUsize,
    unit_queries: AtomicUsize,
}
impl Signals {
    fn check(&self) -> Result<(), HostProblem> {
        let n = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        match self.mode.load(Ordering::SeqCst) {
            2 => Err(HostProblem::Unsupported),
            3 => panic!("private fixture recheck panic"),
            _ if n >= self.refuse_at.load(Ordering::SeqCst) => Err(HostProblem::Unsupported),
            _ => Ok(()),
        }
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
        Ok(Arc::new(Point(self.0.clone(), target.clone())))
    }
}
struct Point(Arc<Signals>, MqMqiNativePointTarget);
impl MqMqiNativePoint for Point {
    fn recheck(&self) -> Result<(), HostProblem> {
        self.0.check()
    }
    fn descriptor_version(&self) -> Result<i32, HostProblem> {
        if self.0.mode.load(Ordering::SeqCst) == 6 {
            return Err(HostProblem::Unsupported);
        }
        Ok(self.0.version.load(Ordering::SeqCst))
    }
    fn max_message_bytes(&self) -> Result<usize, HostProblem> {
        if self.0.mode.load(Ordering::SeqCst) == 7 {
            return Err(HostProblem::Unsupported);
        }
        Ok(self.0.maximum.load(Ordering::SeqCst))
    }
}
impl MqWireBindings for Point {
    fn queue_defaults_are_represented(
        &self,
        _: MqHconn,
        o: Option<MqHobj>,
        q: Option<&MqRouteLookup>,
    ) -> bool {
        if self.0.mode.load(Ordering::SeqCst) == 8 {
            return false;
        }
        match &self.1 {
            MqMqiNativePointTarget::Open { lookup, .. }
            | MqMqiNativePointTarget::PutOne { lookup } => o.is_none() && q == Some(lookup),
            MqMqiNativePointTarget::Object(object) => o == Some(*object) && q.is_none(),
        }
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        MqWireQueueManagerPlatform::Zos
    }
    fn admitted_unit(&self, _: MqHconn) -> Option<MqMqiUnitOfWork> {
        self.0.unit_queries.fetch_add(1, Ordering::SeqCst);
        if self.0.mode.load(Ordering::SeqCst) == 9 {
            return None;
        }
        Some(MqMqiUnitOfWork::Local { unit: 31 })
    }
    fn existing_cursor(&self, _: MqHconn, _: MqHobj) -> Option<u64> {
        None
    }
    fn milliseconds_to_ticks(&self, _: u32) -> Option<u64> {
        None
    }
}
struct GetFrame {
    frame: Frame,
    scope: Arc<MqMqiAbiScope>,
    signals: Arc<Signals>,
}
impl MqMqiProgramFrame for GetFrame {
    fn profile(&self, i: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.frame.profile(i)
    }
    fn abi_scope(&self, i: &Invocation) -> Result<Option<Arc<MqMqiAbiScope>>, HostProblem> {
        self.frame.profile(i)?;
        Ok(Some(self.scope.clone()))
    }
    fn native_structure(
        &self,
        i: &Invocation,
        call: MqMqiCall,
        _: MqHconn,
    ) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
        self.frame.profile(i)?;
        self.signals.captures.fetch_add(1, Ordering::SeqCst);
        if !matches!(call, MqMqiCall::Open | MqMqiCall::Close | MqMqiCall::Get)
            || self.signals.mode.load(Ordering::SeqCst) == 1
        {
            return Err(HostProblem::Unsupported);
        }
        Ok(Arc::new(Native(self.signals.clone())))
    }
}
fn effect(m: &mut ReferenceMachine) -> EffectRequest {
    match m.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()) {
        MachineDrive::HostCall(e) => e,
        other => panic!("genuine compiled original CALL: {other:?}"),
    }
}
fn ok(output: MqMqiOutput) -> MqMqiOutcome {
    MqMqiOutcome::Completed {
        status: MqMqiStatus::OkNone,
        output,
    }
}
fn started(
    v2: bool,
) -> (
    ReferenceMachine,
    Arc<GetFrame>,
    MqHandleRegistry,
    MqHconn,
    MqHobj,
) {
    let mut m = crate::machine::typed_mq::tests::connx::compiled_text(if v2 {
        include_str!("tests/get2.mir")
    } else {
        include_str!("tests/get.mir")
    });
    m.invocation.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
    let frame = Arc::new(GetFrame {
        frame: Frame {
            invocation: m.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: Arc::new(MqMqiAbiScope::new(context(), 4).unwrap()),
        signals: Arc::new(Signals {
            mode: AtomicUsize::new(0),
            checks: AtomicUsize::new(0),
            refuse_at: AtomicUsize::new(usize::MAX),
            version: AtomicI32::new(if v2 { 2 } else { 1 }),
            maximum: AtomicUsize::new(4096),
            captures: AtomicUsize::new(0),
            unit_queries: AtomicUsize::new(0),
        }),
    });
    m.bind_mqi_program_frame(frame.clone()).unwrap();
    let mut registry = MqHandleRegistry::new(1, 8).unwrap();
    let c = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let e = effect(&mut m);
    reply(&mut m, &e, ok(MqMqiOutput::Connected(c))).unwrap();
    let o = registry.create_object(context().owner, c).unwrap();
    let e = effect(&mut m);
    reply(
        &mut m,
        &e,
        ok(MqMqiOutput::Opened {
            object: o,
            dynamic: None,
        }),
    )
    .unwrap();
    (m, frame, registry, c, o)
}
fn envelope(e: &EffectRequest) -> &MqMqiRequestEnvelope {
    let HostRequest::MqMqi(r) = &e.request else {
        panic!()
    };
    &r.envelope
}
fn request(e: &EffectRequest) -> &mainframe_env_host_api::mq_mqi::MqMqiFullGet {
    let MqMqiRequest::QualifiedFullGet(r) = &envelope(e).request else {
        panic!("qualified original request")
    };
    r
}
fn observed(e: &EffectRequest, case: usize) -> MqMqiQualifiedGot {
    let r = request(e);
    let capacity = r.buffer_capacity;
    let (disposition, length, data, resolved) = match case {
        0 => (
            MqGetDisposition::Message(T::Complete { length: 3 }),
            Some(3),
            vec![0, 0xff, 9],
            Some([b'Q'; 48]),
        ),
        1 => (
            MqGetDisposition::Message(T::AcceptedRemoved {
                required: capacity + 2,
                copied: capacity,
            }),
            Some((capacity + 2) as i32),
            vec![0x8a; capacity],
            Some([b'Q'; 48]),
        ),
        2 => (
            MqGetDisposition::Message(T::RejectedRetained {
                required: capacity + 2,
                copied: capacity,
            }),
            Some((capacity + 2) as i32),
            vec![0x8a; capacity],
            None,
        ),
        3 => (MqGetDisposition::NoMessage, None, vec![], None),
        4 => (
            MqGetDisposition::Message(T::Complete { length: 0 }),
            Some(0),
            vec![],
            Some([b'Q'; 48]),
        ),
        _ => panic!(),
    };
    let mut descriptor = r.descriptor.clone();
    let f = match &mut descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    f.report = 1;
    f.msg_type = 8;
    f.expiry = -1;
    f.feedback = 7;
    f.encoding = 785;
    f.coded_char_set_id = 37;
    f.priority = 9;
    f.persistence = 1;
    f.msg_id = [0x91; 24];
    f.correl_id = [0xfe; 24];
    f.backout_count = 3;
    f.reply_to_q = [b'R'; 48];
    f.reply_to_q_mgr = [b'M'; 48];
    f.user_identifier = [b'U'; 12];
    f.accounting_token = [0x97; 32];
    f.appl_identity_data = [b'I'; 32];
    f.put_appl_type = 2;
    f.put_appl_name = [b'A'; 28];
    f.put_date = *b"20261003";
    f.put_time = *b"11223344";
    f.appl_origin_data = [b'O'; 4];
    MqMqiQualifiedGot {
        characters: descriptor.characters(),
        disposition,
        message: (case != 3).then_some(MqFullMessage {
            descriptor,
            body: data,
            properties: vec![],
        }),
        data_length: length,
        cursor: None,
        resolved_queue: resolved,
    }
}
fn outcome(e: &EffectRequest, case: usize) -> MqMqiOutcome {
    let (cc, rc) = match case {
        0 | 4 => ("MQCC_OK", "MQRC_NONE"),
        1 => ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_ACCEPTED"),
        2 => ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED"),
        3 => ("MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE"),
        _ => panic!(),
    };
    MqMqiOutcome::ReviewedOutput {
        status: MqReviewedStatus::from_symbols(MqMqiCall::Get, cc, rc).unwrap(),
        output: MqMqiOutput::QualifiedFullGot(observed(e, case)),
    }
}
fn call(m: &mut ReferenceMachine) -> Result<EffectRequest, MachineProblem> {
    crate::machine::typed_mq::tests::call(
        m,
        &[
            "MQGET",
            "USING",
            "HCONN",
            "HOBJ",
            "MESSAGE-DESC",
            "GET-OPTS",
            "BUFFER-LENGTH",
            "MESSAGE-BUFFER",
            "DATA-LENGTH",
            "CC",
            "REASON",
        ],
    )
}
fn member_mut<'a>(m: &'a mut ReferenceMachine, name: &str) -> &'a mut LayoutMetadata {
    let key = m.layout(name).unwrap().name.clone();
    m.layouts.get_mut(&key).unwrap()
}

#[test]
fn compiled_md1_md2_complete_accepted_rejected_and_no_message_preserve_exact_definedness() {
    for v2 in [false, true] {
        for case in 0..5 {
            for unit in [0_i32, 2, 4] {
                let (mut m, frame, _, c, o) = started(v2);
                let flags = unit + if case == 1 { 64 } else { 0 };
                m.write("GMO-OPTIONS", &flags.to_be_bytes()).unwrap();
                let md = m.read("MESSAGE-DESC").unwrap();
                let gmo = m.read("GET-OPTS").unwrap();
                let body = m.read("MESSAGE-BUFFER").unwrap();
                let length = m.read("DATA-LENGTH").unwrap();
                let e = effect(&mut m);
                let r = request(&e);
                assert_eq!((r.connection, r.object), (c, o));
                assert_eq!(r.descriptor.version(), if v2 { 2 } else { 1 });
                assert_eq!(r.buffer_capacity, 5);
                assert_eq!(r.descriptor.fields().msg_id, [0; 24]);
                assert_eq!(r.descriptor.fields().correl_id, [0; 24]);
                assert_eq!(
                    r.unit,
                    if unit == 4 {
                        MqMqiUnitOfWork::NoSyncpoint
                    } else {
                        MqMqiUnitOfWork::Local { unit: 31 }
                    }
                );
                assert_eq!(e.sequence, 3);
                assert_eq!(e.run_unit, m.invocation.run_unit_id);
                assert_eq!(e.deadline_tick, m.invocation.deadline_tick);
                let HostRequest::MqMqi(host) = &e.request else {
                    panic!()
                };
                assert_eq!(host.envelope.context, context());
                assert_eq!(host.mutation.sequence, e.sequence);
                assert_eq!(
                    Some(&host.mutation.idempotency_key),
                    e.idempotency_key.as_ref()
                );
                let out = observed(&e, case);
                let expected = out.message.clone();
                reply(&mut m, &e, outcome(&e, case)).unwrap();
                let (cc, rc) = match case {
                    0 | 4 => (0_i32, 0_i32),
                    1 => (1, 2079),
                    2 => (1, 2080),
                    3 => (2, 2033),
                    _ => panic!(),
                };
                assert_eq!(m.read("CC").unwrap(), cc.to_be_bytes());
                assert_eq!(m.read("REASON").unwrap(), rc.to_be_bytes());
                if let Some(message) = expected {
                    let (raw, _) = m
                        .point_group(
                            &m.connx_storage("MESSAGE-DESC").unwrap(),
                            if v2 {
                                MqRawLayoutKind::Md2
                            } else {
                                MqRawLayoutKind::Md1
                            },
                            Native(frame.signals.clone()).encoding(),
                        )
                        .unwrap();
                    assert_eq!(raw.to_full_md_value().unwrap(), message.descriptor);
                    let after = m.read("MESSAGE-BUFFER").unwrap();
                    assert_eq!(&after[..message.body.len()], &message.body);
                    assert_eq!(&after[message.body.len()..], &body[message.body.len()..]);
                    assert_eq!(
                        m.read("DATA-LENGTH").unwrap(),
                        out.data_length.unwrap().to_be_bytes()
                    );
                } else {
                    assert_eq!(m.read("MESSAGE-DESC").unwrap(), md);
                    assert_eq!(m.read("MESSAGE-BUFFER").unwrap(), body);
                    assert_eq!(m.read("DATA-LENGTH").unwrap(), length);
                }
                assert_eq!(m.read("MD-SUFFIX").unwrap(), b"TAIL0123");
                assert_eq!(m.read("GMO-SUFFIX").unwrap(), b"TAIL0123");
                assert_eq!(m.read("GMO-SIGNAL1").unwrap(), 73_i32.to_be_bytes());
                assert_eq!(m.read("GMO-SIGNAL2").unwrap(), (-77_i32).to_be_bytes());
                let after = m.read("GET-OPTS").unwrap();
                for (i, (&before, &after)) in gmo.iter().zip(&after).enumerate() {
                    let changed = out.resolved_queue.is_some()
                        && mq_raw_layout(MqRawLayoutKind::Gmo1).fields.iter().any(|f| {
                            f.name == "ResolvedQName" && (f.offset..f.offset + f.width).contains(&i)
                        });
                    if !changed {
                        assert_eq!(before, after, "unowned GMO {i}");
                    }
                }
                if let Some(name) = out.resolved_queue {
                    assert_eq!(m.read("GMO-RESOLVEDQNAME").unwrap(), name);
                }
                assert_eq!(frame.scope.object(2, c), Ok(o));
                let close = effect(&mut m);
                reply(&mut m, &close, ok(MqMqiOutput::NoOutput)).unwrap();
                let disc = effect(&mut m);
                reply(&mut m, &disc, ok(MqMqiOutput::NoOutput)).unwrap();
                assert!(matches!(
                    m.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()),
                    MachineDrive::Completed(_)
                ));
                assert_eq!(m.snapshot().schema_version, 0);
                assert!(m.checkpoint().is_none());
            }
        }
    }
}

#[test]
fn exact_body_zero_full_capacity_and_known_completed_envelope_are_supported() {
    for capacity in [0_i32, 1, 2048] {
        for accepted in [false, true] {
            let (mut m, _, _, _, _) = started(true);
            m.write("BUFFER-LENGTH", &capacity.to_be_bytes()).unwrap();
            if accepted {
                m.write("GMO-OPTIONS", &68_i32.to_be_bytes()).unwrap();
            }
            let before = m.read("MESSAGE-BUFFER").unwrap();
            let e = effect(&mut m);
            assert_eq!(request(&e).buffer_capacity, capacity as usize);
            let case = if accepted { 1 } else { 4 };
            reply(&mut m, &e, outcome(&e, case)).unwrap();
            assert_eq!(
                &m.read("MESSAGE-BUFFER").unwrap()[capacity as usize..],
                &before[capacity as usize..]
            );
        }
    }
    let (mut m, _, _, _, _) = started(false);
    let e = effect(&mut m);
    reply(
        &mut m,
        &e,
        ok(MqMqiOutput::QualifiedFullGot(observed(&e, 0))),
    )
    .unwrap();
}

#[test]
fn wrong_fields_versions_options_layout_bounds_and_pointer_forms_refuse_before_dispatch() {
    for defect in 0..30 {
        let (mut m, frame, _, _, _) = started(true);
        match defect {
            0 => m.write("BUFFER-LENGTH", &(-1_i32).to_be_bytes()).unwrap(),
            1 => m.write("BUFFER-LENGTH", &2049_i32.to_be_bytes()).unwrap(),
            2 => m.write("GMO-OPTIONS", &(-1_i32).to_be_bytes()).unwrap(),
            3 => m.write("GMO-OPTIONS", &6_i32.to_be_bytes()).unwrap(),
            4 => m.write("GMO-OPTIONS", &1_i32.to_be_bytes()).unwrap(),
            5 => m.write("GMO-OPTIONS", &16_i32.to_be_bytes()).unwrap(),
            6 => m.write("GMO-OPTIONS", &16384_i32.to_be_bytes()).unwrap(),
            7 => m.write("GMO-OPTIONS", &8_i32.to_be_bytes()).unwrap(),
            8 => m.write("GMO-VERSION", &2_i32.to_be_bytes()).unwrap(),
            9 => m.write("GMO-STRUCID", b"BAD ").unwrap(),
            10 => m.write("GMO-WAITINTERVAL", &1_i32.to_be_bytes()).unwrap(),
            11 => m.write("MD-VERSION", &1_i32.to_be_bytes()).unwrap(),
            12 => m.write("MD-STRUCID", b"BAD ").unwrap(),
            13 => m.write("MD-MSGID", &[1; 24]).unwrap(),
            14 => m.write("MD-CORRELID", &[1; 24]).unwrap(),
            15 => m.write("MD-FORMAT", b"MQHRF2  ").unwrap(),
            16 => m.write("MD-GROUPID", &[1; 24]).unwrap(),
            17 => m.write("MD-MSGFLAGS", &1_i32.to_be_bytes()).unwrap(),
            18 => m.write("MD-ENCODING", &i32::MAX.to_be_bytes()).unwrap(),
            19 => {
                frame.signals.maximum.store(2047, Ordering::SeqCst);
            }
            20 => {
                frame.signals.version.store(1, Ordering::SeqCst);
            }
            21 => member_mut(&mut m, "GMO-SIGNAL1").native_binary = true,
            22 => member_mut(&mut m, "MD-MSGID").occurs_clause = true,
            23 => member_mut(&mut m, "MESSAGE-BUFFER").alias_of = Some("CC".into()),
            24 => member_mut(&mut m, "DATA-LENGTH").digits = 8,
            25 => {
                let key = m.layout("MD-MSGID").unwrap().name.clone();
                m.views.get_mut(&key).unwrap().offset += 1;
            }
            26 => member_mut(&mut m, "MD-MSGID").parent = Some("MD-FORMAT".into()),
            27 => member_mut(&mut m, "MESSAGE-BUFFER").category = LayoutCategory::Group,
            28 => {
                let key = m.layout("MESSAGE-BUFFER").unwrap().name.clone();
                let cc = m.views[&m.layout("CC").unwrap().name].clone();
                *m.views.get_mut(&key).unwrap() = cc;
            }
            29 => member_mut(&mut m, "GMO-SUFFIX").occurs_clause = true,
            _ => panic!(),
        }
        let before = (m.bases.clone(), m.effect_sequence);
        assert!(call(&mut m).is_err(), "defect {defect}");
        assert_eq!((m.bases.clone(), m.effect_sequence), before);
        frame.scope.require_context(context()).unwrap();
    }
}

#[test]
fn structure_precedes_decode_and_unconfigured_refused_changed_or_panicking_profiles_fail_closed() {
    for mode in 1..=9 {
        let (mut m, frame, _, _, _) = started(false);
        frame.signals.mode.store(mode, Ordering::SeqCst);
        m.write("MD-STRUCID", b"BAD ").unwrap();
        let before = m.effect_sequence;
        assert!(call(&mut m).is_err());
        assert_eq!(m.effect_sequence, before);
        assert!(frame.signals.captures.load(Ordering::SeqCst) >= 1);
    }
    let (mut m, frame, _, _, _) = started(false);
    m.write("GMO-OPTIONS", &2_i32.to_be_bytes()).unwrap();
    frame.signals.mode.store(9, Ordering::SeqCst);
    assert!(call(&mut m).is_err());
}

#[test]
fn every_late_nine_argument_suffix_layout_and_generated_member_change_is_atomic_unknown() {
    for name in [
        "HCONN",
        "HOBJ",
        "MESSAGE-DESC",
        "GET-OPTS",
        "BUFFER-LENGTH",
        "MESSAGE-BUFFER",
        "DATA-LENGTH",
        "CC",
        "REASON",
        "MD-SUFFIX",
        "GMO-SUFFIX",
    ] {
        let (mut m, frame, _, _, _) = started(true);
        let e = effect(&mut m);
        let out = outcome(&e, 0);
        let mut b = m.read(name).unwrap();
        b[0] ^= 1;
        m.write(name, &b).unwrap();
        let before = m.bases.clone();
        assert_eq!(
            reply(&mut m, &e, out),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
            "{name}"
        );
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
    for kind in [MqRawLayoutKind::Md2, MqRawLayoutKind::Gmo1] {
        for field in mq_raw_layout(kind).fields {
            let (mut m, frame, _, _, _) = started(true);
            let e = effect(&mut m);
            let out = outcome(&e, 0);
            let name = format!(
                "{}-{}",
                if kind == MqRawLayoutKind::Md2 {
                    "MD"
                } else {
                    "GMO"
                },
                field.name.to_uppercase()
            );
            member_mut(&mut m, &name).length += 1;
            let before = m.bases.clone();
            assert!(reply(&mut m, &e, out).is_err(), "{name}");
            assert_eq!(m.bases, before);
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
        }
    }
    for name in [
        "MD-COPY",
        "GMO-COPY",
        "MD-SUFFIX",
        "GMO-SUFFIX",
        "MESSAGE-BUFFER",
    ] {
        let (mut m, frame, _, _, _) = started(true);
        let e = effect(&mut m);
        let out = outcome(&e, 0);
        let key = m.layout(name).unwrap().name.clone();
        m.views.get_mut(&key).unwrap().offset += 1;
        let before = m.bases.clone();
        assert!(reply(&mut m, &e, out).is_err());
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn wrong_output_class_status_definedness_limits_and_numeric_observations_never_partially_write() {
    for defect in 0..22 {
        let (mut m, frame, _, _, _) = started(true);
        let e = effect(&mut m);
        let mut v = observed(&e, 0);
        let out = match defect {
            0 => MqMqiOutcome::UnknownOutcome,
            1 => MqMqiOutcome::DuplicatePossible,
            2 => MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE")
                    .unwrap(),
            },
            3 => ok(MqMqiOutput::NoOutput),
            4 => ok(MqMqiOutput::FullGot {
                disposition: v.disposition,
                message: v.message,
                data_length: v.data_length,
                cursor: None,
            }),
            5 => MqMqiOutcome::StatusPending {
                output: MqMqiOutput::QualifiedFullGot(v),
            },
            6 => {
                v.resolved_queue = None;
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            7 => {
                v.data_length = Some(4);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            8 => {
                v.message.as_mut().unwrap().body = vec![0; 6];
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            9 => {
                v.cursor = Some(1);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            10 => {
                v.characters =
                    mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding::OwnedCp037;
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            11 => {
                let md = &mut v.message.as_mut().unwrap().descriptor;
                match md {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.backout_count = 256
                    }
                };
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            12 => {
                let md = &mut v.message.as_mut().unwrap().descriptor;
                match md {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.feedback = i32::MAX
                    }
                };
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            13 => {
                let md = &mut v.message.as_mut().unwrap().descriptor;
                match md {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.coded_char_set_id = 0
                    }
                };
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            14 => {
                let MqMdValue::V2 { extension, .. } = &mut v.message.as_mut().unwrap().descriptor
                else {
                    panic!()
                };
                extension.msg_flags = 1;
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            15 => {
                frame.signals.version.store(1, Ordering::SeqCst);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            16 => {
                frame.signals.maximum.store(8192, Ordering::SeqCst);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            17 => {
                frame.signals.mode.store(3, Ordering::SeqCst);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            18 => {
                frame.frame.changed.store(true, Ordering::SeqCst);
                ok(MqMqiOutput::QualifiedFullGot(v))
            }
            19 => {
                let mut v = observed(&e, 2);
                v.resolved_queue = Some([b'Q'; 48]);
                MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_symbols(
                        MqMqiCall::Get,
                        "MQCC_WARNING",
                        "MQRC_TRUNCATED_MSG_FAILED",
                    )
                    .unwrap(),
                    output: MqMqiOutput::QualifiedFullGot(v),
                }
            }
            20 => {
                let mut v = observed(&e, 3);
                v.data_length = Some(0);
                MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_symbols(
                        MqMqiCall::Get,
                        "MQCC_FAILED",
                        "MQRC_NO_MSG_AVAILABLE",
                    )
                    .unwrap(),
                    output: MqMqiOutput::QualifiedFullGot(v),
                }
            }
            21 => MqMqiOutcome::ReviewedOutput {
                status: MqReviewedStatus::from_symbols(
                    MqMqiCall::Get,
                    "MQCC_WARNING",
                    "MQRC_TRUNCATED_MSG_ACCEPTED",
                )
                .unwrap(),
                output: MqMqiOutput::QualifiedFullGot(v),
            },
            _ => panic!(),
        };
        let before = m.bases.clone();
        assert!(reply(&mut m, &e, out).is_err(), "defect {defect}");
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn final_callback_refusal_cancel_drop_and_original_envelope_mismatch_fence() {
    for defect in 0..6 {
        let (mut m, frame, _, _, _) = started(false);
        let e = effect(&mut m);
        let out = outcome(&e, 0);
        let before = m.bases.clone();
        match defect {
            0 => frame.signals.refuse_at.store(
                frame.signals.checks.load(Ordering::SeqCst) + 4,
                Ordering::SeqCst,
            ),
            1 => m.invocation.cancellation_probe.as_ref().unwrap().request(),
            2 => {
                drop(m);
                assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
                continue;
            }
            _ => {
                let mut limits = envelope(&e).limits;
                if defect == 3 {
                    limits.message.body_bytes -= 1;
                }
                let result = EffectResult {
                    sequence: if defect == 4 {
                        e.sequence + 1
                    } else {
                        e.sequence
                    },
                    outcome: Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
                        limits,
                        result: MqMqiResult {
                            call: if defect == 5 {
                                MqMqiCall::Put
                            } else {
                                MqMqiCall::Get
                            },
                            outcome: out,
                        },
                    }))),
                };
                assert!(m.resume_host(result).is_err());
                assert_eq!(m.bases, before);
                assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
                continue;
            }
        }
        assert!(reply(&mut m, &e, out).is_err());
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn stale_wrong_family_wrong_parent_cold_and_late_retired_aliases_never_revive() {
    for bad in [0_i32, 1, 3, 999999999] {
        let (mut m, _, _, _, _) = started(false);
        m.write("HOBJ", &bad.to_be_bytes()).unwrap();
        let before = (m.bases.clone(), m.effect_sequence);
        assert!(call(&mut m).is_err());
        assert_eq!((m.bases.clone(), m.effect_sequence), before);
    }
    let (mut m, frame, _, c, _) = started(false);
    let e = effect(&mut m);
    let out = outcome(&e, 0);
    let plan = frame.scope.object_plan(1, c, None, None, Some(2)).unwrap();
    let mut guard = plan.guard(&frame.scope).unwrap();
    plan.commit(&mut guard);
    drop(guard);
    let before = m.bases.clone();
    assert!(reply(&mut m, &e, out).is_err());
    assert_eq!(m.bases, before);
    assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    let (mut parent, frame, mut registry, c, _) = started(true);
    // New SAME-root connection has a distinct parent; copied positive integers
    // cannot borrow the prior object's authority under it.
    let other = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let conn = crate::machine::typed_mq::tests::call(
        &mut parent,
        &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"],
    )
    .unwrap();
    reply(&mut parent, &conn, ok(MqMqiOutput::Connected(other))).unwrap();
    assert_eq!(parent.read("HCONN").unwrap(), 3_i32.to_be_bytes());
    assert!(call(&mut parent).is_err());
    assert!(frame.scope.object(2, c).is_ok());
}

#[test]
fn separately_compiled_same_task_child_uses_parent_live_aliases_and_exact_original_effect() {
    let (parent, frame, _, c, o) = started(true);
    let mut child =
        crate::machine::typed_mq::tests::connx::compiled_text(include_str!("tests/child.mir"));
    child.invocation.execution_id =
        mainframe_env_execution_api::ExecutionId::new("same-task-get-child", Default::default())
            .unwrap();
    child.invocation.parent_execution_id = Some(parent.invocation.execution_id.clone());
    child.invocation.cancellation_probe =
        Some(mainframe_env_execution_api::CancellationProbe::new());
    let child_frame = Arc::new(GetFrame {
        frame: Frame {
            invocation: child.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: frame.scope.clone(),
        signals: frame.signals.clone(),
    });
    child.bind_mqi_program_frame(child_frame).unwrap();
    let e = effect(&mut child);
    assert_eq!((request(&e).connection, request(&e).object), (c, o));
    assert_eq!(e.sequence, 1);
    assert_eq!(e.run_unit, child.invocation.run_unit_id);
    reply(&mut child, &e, outcome(&e, 0)).unwrap();
    assert_eq!(frame.scope.object(2, c), Ok(o));
    assert_eq!(parent.read("HOBJ").unwrap(), 2_i32.to_be_bytes());
    assert!(matches!(
        child.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()),
        MachineDrive::Completed(_)
    ));
    drop(child);
    frame.scope.require_context(context()).unwrap();
    let mut cold =
        crate::machine::typed_mq::tests::connx::compiled_text(include_str!("tests/child.mir"));
    let cold_frame = Arc::new(GetFrame {
        frame: Frame {
            invocation: cold.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: Arc::new(MqMqiAbiScope::new(context(), 4).unwrap()),
        signals: frame.signals.clone(),
    });
    cold.bind_mqi_program_frame(cold_frame).unwrap();
    assert!(call(&mut cold).is_err());
}

#[test]
fn ignored_signal_bytes_are_not_addresses_and_unusable_output_profiles_stay_unknown() {
    let (mut m, _, _, _, _) = started(false);
    m.write("GMO-SIGNAL1", &[0xff, 0, 0xa1, 0x93]).unwrap();
    let e = effect(&mut m);
    reply(&mut m, &e, outcome(&e, 0)).unwrap();
    assert_eq!(m.read("GMO-SIGNAL1").unwrap(), [0xff, 0, 0xa1, 0x93]);
    for defect in 0..4 {
        let (mut m, frame, _, _, _) = started(false);
        let e = effect(&mut m);
        let mut v = observed(&e, 0);
        let status = if defect == 3 {
            v.message = None;
            v.data_length = None;
            v.resolved_queue = None;
            v.disposition = MqGetDisposition::WaitExpired;
            MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE")
                .unwrap()
        } else {
            match defect {
                0 => v.message.as_mut().unwrap().properties.push(
                    mainframe_env_host_api::MqMessageProperty {
                        name: "ordinary.property".into(),
                        kind: mainframe_env_host_api::MqPropertyType::ByteString,
                        value: vec![0, 255],
                    },
                ),
                1 => {
                    v.disposition = MqGetDisposition::Message(T::Complete { length: 4097 });
                    v.data_length = Some(4097);
                    v.message.as_mut().unwrap().body = vec![0; 4097];
                }
                2 => match &mut v.message.as_mut().unwrap().descriptor {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.struc_id = *b"BAD "
                    }
                },
                _ => unreachable!(),
            }
            MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap()
        };
        let before = m.bases.clone();
        assert!(
            reply(
                &mut m,
                &e,
                MqMqiOutcome::ReviewedOutput {
                    status,
                    output: MqMqiOutput::QualifiedFullGot(v)
                }
            )
            .is_err()
        );
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn equal_context_foreign_root_and_same_task_disc_retirement_prevent_final_writeback() {
    for after_dispatch in [false, true] {
        let (mut m, frame, _, _, _) = started(false);
        let e = after_dispatch.then(|| effect(&mut m));
        m.mqi.as_mut().unwrap().scope = Some(Arc::new(MqMqiAbiScope::new(context(), 4).unwrap()));
        let before = (m.bases.clone(), m.effect_sequence);
        if let Some(e) = e {
            assert!(reply(&mut m, &e, outcome(&e, 0)).is_err());
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
        } else {
            assert!(call(&mut m).is_err());
        }
        assert_eq!((m.bases.clone(), m.effect_sequence), before);
    }
    let (mut parent, frame, _, c, _) = started(true);
    let e = effect(&mut parent);
    let out = outcome(&e, 0);
    let mut sibling =
        crate::machine::typed_mq::tests::connx::compiled_text(include_str!("tests/disc-child.mir"));
    sibling.invocation.execution_id =
        mainframe_env_execution_api::ExecutionId::new("same-task-disc-sibling", Default::default())
            .unwrap();
    sibling.invocation.parent_execution_id = Some(parent.invocation.execution_id.clone());
    let sibling_frame = Arc::new(GetFrame {
        frame: Frame {
            invocation: sibling.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: frame.scope.clone(),
        signals: frame.signals.clone(),
    });
    sibling.bind_mqi_program_frame(sibling_frame).unwrap();
    let disc = effect(&mut sibling);
    reply(&mut sibling, &disc, ok(MqMqiOutput::NoOutput)).unwrap();
    assert!(frame.scope.connection(1).is_err());
    assert!(frame.scope.object(2, c).is_err());
    let before = parent.bases.clone();
    assert!(reply(&mut parent, &e, out).is_err());
    assert_eq!(parent.bases, before);
    assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
}
