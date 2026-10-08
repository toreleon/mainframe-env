//! Genuine compiler output with private frame/replies: engine evidence ONLY.
use super::*;
use crate::machine::typed_mq::tests::{Frame, context, reply};
use mainframe_env_host_api::mq_md_value::MqMdValue;
use mainframe_env_host_api::mq_mqi::{
    MqMqiDestinationCount, MqMqiIgnoredCounter, MqMqiMessageContext, MqMqiProduced,
};
use mainframe_env_host_api::{MqDeliveryOutcome, MqHandleRegistry};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

mod supplied_correlation;

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
            3 => panic!("private fixture point recheck panic"),
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
        Some(MqMqiUnitOfWork::Local { unit: 31 })
    }
    fn existing_cursor(&self, _: MqHconn, _: MqHobj) -> Option<u64> {
        None
    }
    fn milliseconds_to_ticks(&self, _: u32) -> Option<u64> {
        None
    }
}
struct PutFrame {
    frame: Frame,
    scope: Arc<MqMqiAbiScope>,
    signals: Arc<Signals>,
}
impl MqMqiProgramFrame for PutFrame {
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
        if !matches!(
            call,
            MqMqiCall::Open | MqMqiCall::Close | MqMqiCall::Put | MqMqiCall::PutOne
        ) || self.signals.mode.load(Ordering::SeqCst) == 1
        {
            return Err(HostProblem::Unsupported);
        }
        Ok(Arc::new(Native(self.signals.clone())))
    }
}
fn text(one: bool, v2: bool) -> &'static str {
    match (one, v2) {
        (false, false) => include_str!("tests/put.mir"),
        (false, true) => include_str!("tests/put2.mir"),
        (true, false) => include_str!("tests/put1.mir"),
        (true, true) => include_str!("tests/put12.mir"),
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
    one: bool,
    v2: bool,
) -> (
    ReferenceMachine,
    Arc<PutFrame>,
    MqHandleRegistry,
    MqHconn,
    Option<MqHobj>,
) {
    let mut m = crate::machine::typed_mq::tests::connx::compiled_text(text(one, v2));
    m.invocation.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
    let frame = Arc::new(PutFrame {
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
    let o = if one {
        None
    } else {
        let e = effect(&mut m);
        let o = registry.create_object(context().owner, c).unwrap();
        reply(
            &mut m,
            &e,
            ok(MqMqiOutput::Opened {
                object: o,
                dynamic: None,
            }),
        )
        .unwrap();
        Some(o)
    };
    (m, frame, registry, c, o)
}
fn request(e: &EffectRequest) -> &mainframe_env_host_api::mq_mqi::MqMqiFullPut {
    let HostRequest::MqMqi(r) = &e.request else {
        panic!()
    };
    match &r.envelope.request {
        MqMqiRequest::FullPut { put, .. } | MqMqiRequest::FullPutOne { put, .. } => put,
        _ => panic!("complete producer request required"),
    }
}
fn produced(e: &EffectRequest) -> MqMqiProduced {
    let p = request(e);
    let mut md = p.message.descriptor.clone();
    let f = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    if p.context == MqMqiMessageContext::NoContext {
        f.user_identifier = [b' '; 12];
        f.accounting_token = [0; 32];
        f.appl_identity_data = [b' '; 32];
        f.put_appl_type = 0;
        f.put_appl_name = [b' '; 28];
        f.put_date = [b' '; 8];
        f.put_time = [b' '; 8];
        f.appl_origin_data = [b' '; 4];
    } else {
        f.user_identifier = *b"PRODUCER    ";
        f.accounting_token = [0x92; 32];
        f.appl_identity_data = [b' '; 32];
        f.put_appl_type = 2;
        f.put_appl_name = [b'J'; 28];
        f.put_date = *b"20261003";
        f.put_time = *b"11223344";
        f.appl_origin_data = [b' '; 4];
    }
    MqMqiProduced {
        descriptor: md,
        outcome: if matches!(p.unit, MqMqiUnitOfWork::Local { .. }) {
            MqDeliveryOutcome::Pending
        } else {
            MqDeliveryOutcome::Accepted
        },
        resolved_queue: [b'Q'; 48],
        resolved_manager: [b'M'; 48],
        known_dest_count: MqMqiDestinationCount::UndefinedZos,
        unknown_dest_count: MqMqiDestinationCount::UndefinedZos,
        invalid_dest_count: MqMqiDestinationCount::UndefinedZos,
        backout_count: MqMqiIgnoredCounter::PreservedIgnoredInput,
    }
}
fn call(m: &mut ReferenceMachine, one: bool) -> Result<EffectRequest, MachineProblem> {
    crate::machine::typed_mq::tests::call(
        m,
        &[
            if one { "MQPUT1" } else { "MQPUT" },
            "USING",
            "HCONN",
            if one { "OBJECT-DESC" } else { "HOBJ" },
            "MESSAGE-DESC",
            "PUT-OPTS",
            "BUFFER-LENGTH",
            "MESSAGE-BUFFER",
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
fn compiled_put_and_put1_md1_md2_copy_groups_original_context_and_joined_writeback() {
    for one in [false, true] {
        for v2 in [false, true] {
            for context_option in [16384, 32] {
                for sync in [4, 2] {
                    let (mut m, frame, _, c, o) = started(one, v2);
                    m.write(
                        "PMO-OPTIONS",
                        &(131072_i32 + context_option + sync).to_be_bytes(),
                    )
                    .unwrap();
                    // Exact bytes IDs include binary zero/high values, never trim/re-encode.
                    m.write("MD-MSGID", &[0x9a; 24]).unwrap();
                    m.write("MD-CORRELID", &[0xff; 24]).unwrap();
                    let original_md = m.read("MESSAGE-DESC").unwrap();
                    let original_pmo = m.read("PUT-OPTS").unwrap();
                    let body = m.read("MESSAGE-BUFFER").unwrap();
                    let od = m.read("OBJECT-DESC").unwrap();
                    let e = effect(&mut m);
                    assert_eq!(e.sequence, if one { 2 } else { 3 });
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
                    match &host.envelope.request {
                        MqMqiRequest::FullPut {
                            connection, object, ..
                        } => {
                            assert_eq!(*connection, c);
                            assert_eq!(Some(*object), o);
                        }
                        MqMqiRequest::FullPutOne {
                            connection,
                            lookup,
                            alternate_user,
                            ..
                        } => {
                            assert_eq!(*connection, c);
                            assert!(alternate_user.is_none());
                            assert_eq!(
                                *lookup,
                                MqRouteLookup::Queue {
                                    name: MqRouteName::new("ORDINARY.Q").unwrap(),
                                    manager: None,
                                    dynamic_pattern: None
                                }
                            );
                        }
                        _ => panic!(),
                    }
                    assert_eq!(request(&e).message.body, b"HELLO");
                    assert_eq!(
                        request(&e).message.descriptor.version(),
                        if v2 { 2 } else { 1 }
                    );
                    assert_eq!(
                        request(&e).unit,
                        if sync == 2 {
                            MqMqiUnitOfWork::Local { unit: 31 }
                        } else {
                            MqMqiUnitOfWork::NoSyncpoint
                        }
                    );
                    let out = produced(&e);
                    let expected_md = out.descriptor.clone();
                    reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).unwrap();
                    assert_eq!(m.read("CC").unwrap(), 0_i32.to_be_bytes());
                    assert_eq!(m.read("REASON").unwrap(), 0_i32.to_be_bytes());
                    let (md, _) = m
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
                    assert_eq!(md.to_full_md_value().unwrap(), expected_md);
                    assert_eq!(m.read("MD-BACKOUTCOUNT").unwrap(), 73_i32.to_be_bytes());
                    assert_eq!(m.read("MD-MSGID").unwrap(), [0x9a; 24]);
                    assert_eq!(m.read("MD-CORRELID").unwrap(), [0xff; 24]);
                    assert_eq!(m.read("MD-SUFFIX").unwrap(), b"TAIL0123");
                    assert_eq!(m.read("PMO-RESOLVEDQNAME").unwrap(), [b'Q'; 48]);
                    assert_eq!(m.read("PMO-RESOLVEDQMGRNAME").unwrap(), [b'M'; 48]);
                    for (name, value) in [
                        ("PMO-KNOWNDESTCOUNT", 111_i32),
                        ("PMO-UNKNOWNDESTCOUNT", 222),
                        ("PMO-INVALIDDESTCOUNT", 333),
                        ("PMO-TIMEOUT", -77),
                    ] {
                        assert_eq!(m.read(name).unwrap(), value.to_be_bytes());
                    }
                    assert_eq!(m.read("PMO-SUFFIX").unwrap(), b"TAIL0123");
                    assert_eq!(m.read("MESSAGE-BUFFER").unwrap(), body);
                    assert_eq!(m.read("OBJECT-DESC").unwrap(), od);
                    // Unowned bytes in complete captured groups are exact, independently
                    // identify changed field ranges from the sole reviewed descriptors.
                    for (kind, before, after, changed) in [
                        (
                            if v2 {
                                MqRawLayoutKind::Md2
                            } else {
                                MqRawLayoutKind::Md1
                            },
                            original_md,
                            m.read("MESSAGE-DESC").unwrap(),
                            vec![
                                "UserIdentifier",
                                "AccountingToken",
                                "ApplIdentityData",
                                "PutApplType",
                                "PutApplName",
                                "PutDate",
                                "PutTime",
                                "ApplOriginData",
                            ],
                        ),
                        (
                            MqRawLayoutKind::Pmo1,
                            original_pmo,
                            m.read("PUT-OPTS").unwrap(),
                            vec!["ResolvedQName", "ResolvedQMgrName"],
                        ),
                    ] {
                        for (i, (&a, &b)) in before.iter().zip(&after).enumerate() {
                            if !mq_raw_layout(kind).fields.iter().any(|f| {
                                changed.contains(&f.name)
                                    && (f.offset..f.offset + f.width).contains(&i)
                            }) {
                                assert_eq!(a, b, "unowned byte {i}");
                            }
                        }
                    }
                    assert_eq!(frame.scope.connection(1), Ok(c));
                    if let Some(o) = o {
                        assert_eq!(frame.scope.object(2, c), Ok(o));
                        let e = effect(&mut m);
                        reply(&mut m, &e, ok(MqMqiOutput::NoOutput)).unwrap();
                        assert_eq!(frame.scope.object(2, c), Err(HostProblem::Malformed));
                    }
                    let e = effect(&mut m);
                    reply(&mut m, &e, ok(MqMqiOutput::NoOutput)).unwrap();
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
}

#[test]
fn known_failed_puts_only_exact_status_without_descriptor_pmo_body_or_alias_mutation() {
    for one in [false, true] {
        let (mut m, frame, _, c, o) = started(one, false);
        let before = (
            m.read("MESSAGE-DESC").unwrap(),
            m.read("PUT-OPTS").unwrap(),
            m.read("MESSAGE-BUFFER").unwrap(),
        );
        let e = effect(&mut m);
        let call = if one {
            MqMqiCall::PutOne
        } else {
            MqMqiCall::Put
        };
        reply(
            &mut m,
            &e,
            MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_symbols(
                    call,
                    "MQCC_FAILED",
                    "MQRC_BUFFER_LENGTH_ERROR",
                )
                .unwrap(),
            },
        )
        .unwrap();
        assert_eq!(m.read("CC").unwrap(), 2_i32.to_be_bytes());
        assert_eq!(m.read("REASON").unwrap(), 2005_i32.to_be_bytes());
        assert_eq!(
            (
                m.read("MESSAGE-DESC").unwrap(),
                m.read("PUT-OPTS").unwrap(),
                m.read("MESSAGE-BUFFER").unwrap()
            ),
            before
        );
        assert_eq!(frame.scope.connection(1), Ok(c));
        if let Some(o) = o {
            assert_eq!(frame.scope.object(2, c), Ok(o));
        }
    }
}

#[test]
fn exact_buffer_prefix_zero_and_capacity_above_old_cno_bound_are_admitted() {
    for len in [0_i32, 1, 2048] {
        let (mut m, _, _, _, _) = started(true, false);
        m.write("BUFFER-LENGTH", &len.to_be_bytes()).unwrap();
        let body = m.read("MESSAGE-BUFFER").unwrap();
        let e = effect(&mut m);
        assert_eq!(request(&e).message.body, &body[..len as usize]);
        reply(&mut m, &e, ok(MqMqiOutput::Produced(produced(&e)))).unwrap();
        assert_eq!(m.read("MESSAGE-BUFFER").unwrap(), body);
    }
}

#[test]
fn invalid_complete_groups_controls_profiles_and_body_bounds_refuse_before_dispatch() {
    for one in [false, true] {
        for defect in 0..28 {
            let (mut m, frame, _, _, _) = started(one, false);
            match defect {
                0 => m.write("BUFFER-LENGTH", &(-1_i32).to_be_bytes()).unwrap(),
                1 => m.write("BUFFER-LENGTH", &2049_i32.to_be_bytes()).unwrap(),
                2 => m.write("PMO-OPTIONS", &0_i32.to_be_bytes()).unwrap(),
                3 => m.write("PMO-OPTIONS", &(-1_i32).to_be_bytes()).unwrap(),
                4 => m.write("PMO-OPTIONS", &147462_i32.to_be_bytes()).unwrap(),
                5 => m.write("PMO-VERSION", &2_i32.to_be_bytes()).unwrap(),
                6 => m.write("MD-VERSION", &2_i32.to_be_bytes()).unwrap(),
                7 => m.write("MD-STRUCID", b"BAD ").unwrap(),
                8 => m.write("MD-MSGID", &[0; 24]).unwrap(),
                // Reviewed NEW_CORREL_ID128 remains unrepresented, even with
                // otherwise supported synchronous/no-context/no-syncpoint bits.
                9 => m.write("PMO-OPTIONS", &147588_i32.to_be_bytes()).unwrap(),
                10 => m.write("MD-PRIORITY", &(-2_i32).to_be_bytes()).unwrap(),
                11 => m.write("MD-EXPIRY", &1_i32.to_be_bytes()).unwrap(),
                12 => m.write("MD-PERSISTENCE", &3_i32.to_be_bytes()).unwrap(),
                13 => m
                    .write("MD-CODEDCHARSETID", &1208_i32.to_be_bytes())
                    .unwrap(),
                14 => m.write("MD-FORMAT", b"MQHRF2  ").unwrap(),
                15 => {
                    frame.signals.maximum.store(2047, Ordering::SeqCst);
                }
                16 => {
                    frame.signals.version.store(2, Ordering::SeqCst);
                }
                17 => {
                    member_mut(&mut m, "MD-MSGID").occurs_clause = true;
                }
                18 => {
                    member_mut(&mut m, "PMO-OPTIONS").native_binary = true;
                }
                19 => {
                    member_mut(&mut m, "MD-EXPIRY").digits = 8;
                }
                20 => {
                    let key = m.layout("MD-MSGID").unwrap().name.clone();
                    m.views.get_mut(&key).unwrap().offset += 1;
                }
                21 => {
                    member_mut(&mut m, "MESSAGE-BUFFER").alias_of = Some("CC".into());
                }
                22 => {
                    member_mut(&mut m, "BUFFER-LENGTH").native_binary = true;
                }
                23 => {
                    member_mut(&mut m, "MD-MSGID").parent = Some("MD-FORMAT".into());
                }
                24 => {
                    member_mut(&mut m, "MESSAGE-BUFFER").category = LayoutCategory::Group;
                }
                25 => m.write("MD-ENCODING", &i32::MAX.to_be_bytes()).unwrap(),
                26 => {
                    let key = m.layout("MESSAGE-BUFFER").unwrap().name.clone();
                    let cc = m.views[&m.layout("CC").unwrap().name].clone();
                    *m.views.get_mut(&key).unwrap() = cc;
                }
                27 => {
                    member_mut(&mut m, "MD-SUFFIX").occurs_clause = true;
                }
                _ => unreachable!(),
            }
            let before = (m.bases.clone(), m.effect_sequence);
            assert!(call(&mut m, one).is_err(), "defect {defect}, one {one}");
            assert_eq!((m.bases.clone(), m.effect_sequence), before);
            frame.scope.require_context(context()).unwrap();
        }
    }
}

#[test]
fn structure_precedes_decode_and_old_missing_getter_wrong_encoding_or_panic_is_closed() {
    for mode in 1..=7 {
        let (mut m, frame, _, _, _) = started(true, false);
        frame.signals.mode.store(mode, Ordering::SeqCst);
        m.write("MD-STRUCID", b"BAD ").unwrap();
        let before = m.effect_sequence;
        assert!(call(&mut m, true).is_err());
        assert_eq!(m.effect_sequence, before);
        assert_eq!(frame.signals.captures.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn every_late_argument_prefix_suffix_member_and_view_drift_fences_without_partial_write() {
    for one in [false, true] {
        for name in [
            "HCONN",
            if one { "OBJECT-DESC" } else { "HOBJ" },
            "MESSAGE-DESC",
            "PUT-OPTS",
            "BUFFER-LENGTH",
            "MESSAGE-BUFFER",
            "CC",
            "REASON",
            "MD-SUFFIX",
            "PMO-SUFFIX",
        ] {
            let (mut m, frame, _, c, _) = started(one, false);
            let e = effect(&mut m);
            let out = produced(&e);
            let mut bytes = m.read(name).unwrap();
            bytes[0] ^= 1;
            m.write(name, &bytes).unwrap();
            let before = m.bases.clone();
            assert_eq!(
                reply(&mut m, &e, ok(MqMqiOutput::Produced(out))),
                Err(MachineProblem::Host(HostProblem::UnknownOutcome))
            );
            assert_eq!(m.bases, before);
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
            let _ = c;
        }
    }
    for defect in 0..4 {
        let (mut m, frame, _, _, _) = started(false, true);
        let e = effect(&mut m);
        let out = produced(&e);
        match defect {
            0 => member_mut(&mut m, "MD-MSGID").parent = Some("MD-FORMAT".into()),
            1 => member_mut(&mut m, "PMO-OPTIONS").signed = false,
            2 => {
                let key = m.layout("MESSAGE-BUFFER").unwrap().name.clone();
                m.views.get_mut(&key).unwrap().length -= 1;
            }
            _ => member_mut(&mut m, "MD-SUFFIX").parent = Some("OD-STRUCID".into()),
        }
        let before = m.bases.clone();
        assert!(reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).is_err());
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn request_bound_produced_only_unknown_wrong_shape_status_and_changed_opaque_facts_fence() {
    for one in [false, true] {
        for defect in 0..12 {
            let (mut m, frame, _, _, _) = started(one, false);
            let e = effect(&mut m);
            let mut out = produced(&e);
            let outcome = match defect {
                0 => MqMqiOutcome::UnknownOutcome,
                1 => MqMqiOutcome::DuplicatePossible,
                2 => MqMqiOutcome::ReviewedStatus {
                    status: MqReviewedStatus::from_symbols(
                        if one {
                            MqMqiCall::PutOne
                        } else {
                            MqMqiCall::Put
                        },
                        "MQCC_OK",
                        "MQRC_NONE",
                    )
                    .unwrap(),
                },
                3 => ok(MqMqiOutput::NoOutput),
                4 => {
                    out.outcome = MqDeliveryOutcome::Pending;
                    ok(MqMqiOutput::Produced(out))
                }
                5 => {
                    match &mut out.descriptor {
                        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                            fields.msg_id = [0; 24]
                        }
                    };
                    ok(MqMqiOutput::Produced(out))
                }
                6 => {
                    out.resolved_queue = [b' '; 48];
                    ok(MqMqiOutput::Produced(out))
                }
                7 => {
                    frame.signals.version.store(2, Ordering::SeqCst);
                    ok(MqMqiOutput::Produced(out))
                }
                8 => {
                    frame.signals.maximum.store(8192, Ordering::SeqCst);
                    ok(MqMqiOutput::Produced(out))
                }
                9 => {
                    frame.signals.mode.store(3, Ordering::SeqCst);
                    ok(MqMqiOutput::Produced(out))
                }
                10 => {
                    frame.frame.changed.store(true, Ordering::SeqCst);
                    ok(MqMqiOutput::Produced(out))
                }
                11 => {
                    match &mut out.descriptor {
                        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                            fields.put_date = *b"20261003"
                        }
                    };
                    ok(MqMqiOutput::Produced(out))
                }
                _ => unreachable!(),
            };
            let before = m.bases.clone();
            assert_eq!(
                reply(&mut m, &e, outcome),
                Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
                "defect {defect}"
            );
            assert_eq!(m.bases, before);
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
        }
    }
}

#[test]
fn final_callback_refusal_cancel_and_drop_preserve_all_bytes_and_fence() {
    for one in [false, true] {
        for defect in 0..3 {
            let (mut m, frame, _, _, _) = started(one, false);
            let e = effect(&mut m);
            let out = produced(&e);
            let before = m.bases.clone();
            match defect {
                0 => {
                    frame.signals.refuse_at.store(
                        frame.signals.checks.load(Ordering::SeqCst) + 4,
                        Ordering::SeqCst,
                    );
                }
                1 => m.invocation.cancellation_probe.as_ref().unwrap().request(),
                _ => {
                    drop(m);
                    assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
                    continue;
                }
            }
            assert!(reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).is_err());
            assert_eq!(m.bases, before);
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
        }
    }
}

#[test]
fn late_parent_object_retirement_and_wrong_family_never_reconstruct_handles() {
    let (mut m, frame, mut registry, c, o) = started(false, false);
    let e = effect(&mut m);
    let out = produced(&e);
    registry
        .release(
            context().owner,
            c,
            o.unwrap().into(),
            mainframe_env_host_api::MqHandleKind::Object,
        )
        .unwrap();
    // Simulate another SAME TASK known CLOSE's ABI retirement; no numeric/serde revival.
    let plan = frame.scope.object_plan(1, c, None, None, Some(2)).unwrap();
    let mut guard = plan.guard(&frame.scope).unwrap();
    plan.commit(&mut guard);
    drop(guard);
    let before = m.bases.clone();
    assert!(reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).is_err());
    assert_eq!(m.bases, before);
    assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    for one in [false, true] {
        for bad in [0_i32, 1, 3, 999999999] {
            let (mut m, _, _, _, _) = started(one, false);
            m.write(if one { "HCONN" } else { "HOBJ" }, &bad.to_be_bytes())
                .unwrap();
            if one && bad == 1 {
                continue;
            }
            let before = (m.bases.clone(), m.effect_sequence);
            assert!(call(&mut m, one).is_err());
            assert_eq!((m.bases.clone(), m.effect_sequence), before);
        }
    }
}

#[test]
fn separately_compiled_same_task_child_put_uses_exact_parent_aliases_and_original_occurrence() {
    let (parent, frame, _, c, object) = started(false, false);
    let mut child =
        crate::machine::typed_mq::tests::connx::compiled_text(include_str!("tests/child.mir"));
    child.invocation.execution_id =
        mainframe_env_execution_api::ExecutionId::new("same-task-put-child", Default::default())
            .unwrap();
    child.invocation.parent_execution_id = Some(parent.invocation.execution_id.clone());
    child.invocation.cancellation_probe =
        Some(mainframe_env_execution_api::CancellationProbe::new());
    let child_frame = Arc::new(PutFrame {
        frame: Frame {
            invocation: child.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: frame.scope.clone(),
        signals: frame.signals.clone(),
    });
    child.bind_mqi_program_frame(child_frame).unwrap();
    let e = effect(&mut child);
    let HostRequest::MqMqi(host) = &e.request else {
        panic!()
    };
    let MqMqiRequest::FullPut {
        connection,
        object: observed,
        ..
    } = host.envelope.request
    else {
        panic!()
    };
    assert_eq!(connection, c);
    assert_eq!(Some(observed), object);
    assert_eq!(e.sequence, 1);
    assert_eq!(e.run_unit, child.invocation.run_unit_id);
    assert_eq!(host.mutation.sequence, e.sequence);
    reply(&mut child, &e, ok(MqMqiOutput::Produced(produced(&e)))).unwrap();
    assert_eq!(
        frame.scope.object(2, c),
        object.ok_or(HostProblem::Malformed)
    );
    assert_eq!(parent.read("HOBJ").unwrap(), 2_i32.to_be_bytes());
    assert!(matches!(
        child.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()),
        MachineDrive::Completed(_)
    ));
    drop(child);
    frame.scope.require_context(context()).unwrap();
}

#[test]
fn every_generated_md_pmo_member_layout_drift_is_captured_and_status_envelopes_are_bound() {
    for kind in [MqRawLayoutKind::Md2, MqRawLayoutKind::Pmo1] {
        for field in mq_raw_layout(kind).fields {
            let (mut m, frame, _, _, _) = started(false, true);
            let e = effect(&mut m);
            let out = produced(&e);
            let name = format!(
                "{}-{}",
                if kind == MqRawLayoutKind::Pmo1 {
                    "PMO"
                } else {
                    "MD"
                },
                field.name.to_uppercase()
            );
            member_mut(&mut m, &name).length += 1;
            let before = m.bases.clone();
            assert!(
                reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).is_err(),
                "{name}"
            );
            assert_eq!(m.bases, before);
            assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
        }
    }
    for defect in 0..3 {
        let (mut m, frame, _, _, _) = started(true, false);
        let e = effect(&mut m);
        let out = produced(&e);
        let HostRequest::MqMqi(host) = &e.request else {
            panic!()
        };
        let mut limits = host.envelope.limits;
        if defect == 0 {
            limits.message.body_bytes -= 1;
        }
        let result = EffectResult {
            sequence: if defect == 2 {
                e.sequence + 1
            } else {
                e.sequence
            },
            outcome: Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
                limits,
                result: MqMqiResult {
                    call: if defect == 1 {
                        MqMqiCall::Put
                    } else {
                        MqMqiCall::PutOne
                    },
                    outcome: ok(MqMqiOutput::Produced(out)),
                },
            }))),
        };
        let before = m.bases.clone();
        assert!(m.resume_host(result).is_err());
        assert_eq!(m.bases, before);
        assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn both_positive_reviewed_ccsid_and_persistence_values_remain_complete_observations() {
    for ccsid in [37_i32, 819] {
        for persistence in [0_i32, 1] {
            let (mut m, _, _, _, _) = started(true, false);
            m.write("MD-CODEDCHARSETID", &ccsid.to_be_bytes()).unwrap();
            m.write("MD-PERSISTENCE", &persistence.to_be_bytes())
                .unwrap();
            let e = effect(&mut m);
            assert_eq!(
                request(&e).message.descriptor.fields().coded_char_set_id,
                ccsid
            );
            assert_eq!(
                request(&e).message.descriptor.fields().persistence,
                persistence
            );
            reply(&mut m, &e, ok(MqMqiOutput::Produced(produced(&e)))).unwrap();
            assert_eq!(m.read("MD-CODEDCHARSETID").unwrap(), ccsid.to_be_bytes());
            assert_eq!(m.read("MD-PERSISTENCE").unwrap(), persistence.to_be_bytes());
        }
    }
}

#[test]
fn compiled_queue_policy_sentinels_default_response_and_input_only_fields_stay_exact() {
    for one in [false, true] {
        for v2 in [false, true] {
            for response in [0, 131072] {
                let (mut m, _, _, _, _) = started(one, v2);
                m.write("MD-PRIORITY", &(-1_i32).to_be_bytes()).unwrap();
                m.write("MD-PERSISTENCE", &2_i32.to_be_bytes()).unwrap();
                m.write("PMO-OPTIONS", &(response + 16384_i32 + 4).to_be_bytes())
                    .unwrap();
                let e = effect(&mut m);
                assert_eq!(request(&e).message.descriptor.fields().priority, -1);
                assert_eq!(request(&e).message.descriptor.fields().persistence, 2);
                reply(&mut m, &e, ok(MqMqiOutput::Produced(produced(&e)))).unwrap();
                assert_eq!(m.read("MD-PRIORITY").unwrap(), (-1_i32).to_be_bytes());
                assert_eq!(m.read("MD-PERSISTENCE").unwrap(), 2_i32.to_be_bytes());
            }
        }
        for missing in [false, true] {
            let (mut m, frame, _, _, _) = started(one, false);
            m.write(
                "PMO-OPTIONS",
                &(16384_i32 + if missing { 4 } else { 2 }).to_be_bytes(),
            )
            .unwrap();
            if missing {
                frame.signals.mode.store(8, Ordering::SeqCst);
            }
            if missing || one {
                assert!(!matches!(
                    m.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap()),
                    MachineDrive::HostCall(_)
                ));
            } else {
                effect(&mut m);
            }
        }
    }
}
