use super::*;
use mainframe_env_host_api::mq_raw_layout::{
    MqConnxProfile, MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_host_api::mq_status::MqReviewedStatus;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize};

struct ConnxFrame {
    frame: Frame,
    mode: AtomicU8,
    lookups: AtomicUsize,
    change_at: AtomicUsize,
    context_change: AtomicBool,
    panic_profile: AtomicBool,
    connection: MqHconn,
    unit: AtomicU64,
}
fn ordinary() -> MqMqiConnxProfile {
    MqMqiConnxProfile {
        profile: MqConnxProfile::OrdinaryOwnedNonshared,
        encoding: MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::NormalBigEndian,
            characters: MqRawCharacterEncoding::AsciiCompatible,
        },
    }
}
impl MqMqiProgramFrame for ConnxFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        assert!(
            !self.panic_profile.load(Ordering::SeqCst),
            "fixture profile panic"
        );
        self.frame.profile(invocation)
    }
    fn connx_profile(&self, invocation: &Invocation) -> Result<MqMqiConnxProfile, HostProblem> {
        self.frame.profile(invocation)?;
        let observed = self.lookups.fetch_add(1, Ordering::SeqCst) + 1;
        if self.context_change.load(Ordering::SeqCst) {
            self.frame.changed.store(true, Ordering::SeqCst);
        }
        let mut p = ordinary();
        let mode = if observed >= self.change_at.load(Ordering::SeqCst) {
            7
        } else {
            self.mode.load(Ordering::SeqCst)
        };
        match mode {
            1 => return Err(HostProblem::Unsupported),
            2 => panic!("fixture CONNX lookup panic"),
            3 => p.encoding.characters = MqRawCharacterEncoding::OwnedCp037,
            4 => p.encoding.numbers = MqRawNumberEncoding::ReversedLittleEndian,
            5 => p.encoding.numbers = MqRawNumberEncoding::Unsupported,
            6 => p.profile = MqConnxProfile::MtsSharingDefaultPending,
            7 => p.profile = MqConnxProfile::ClientOrFallbackPending,
            8 => p.profile = MqConnxProfile::BindingOrSecurityPending,
            9 => p.profile = MqConnxProfile::ImplicitConnectionPending,
            10 => p.profile = MqConnxProfile::UnknownPending,
            _ => {}
        }
        Ok(p)
    }
    fn local_unit(
        &self,
        invocation: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.frame.profile(invocation)?;
        if connection != self.connection {
            return Err(HostProblem::Unauthorized);
        }
        Ok(MqMqiUnitOfWork::Local {
            unit: self.unit.load(Ordering::SeqCst),
        })
    }
}

// Product compiler output of adjacent connx.cbl, independently regenerated with
// the exact candidate compiler in the external receipt. No handcrafted layout.
fn compiled() -> ReferenceMachine {
    compiled_text(include_str!("connx.mir"))
}
fn compiled_text(text: &str) -> ReferenceMachine {
    let module = mainframe_env_ir::parse_text(text, CodecLimits::default()).unwrap();
    let binary = mainframe_env_ir::encode_binary(&module, CodecLimits::default()).unwrap();
    ReferenceMachine::from_binary(
        &binary,
        super::super::super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap()
}
fn fixture(initialize: bool) -> (ReferenceMachine, Arc<ConnxFrame>) {
    bind_fixture(compiled(), initialize)
}
fn bind_fixture(
    mut machine: ReferenceMachine,
    initialize: bool,
) -> (ReferenceMachine, Arc<ConnxFrame>) {
    let frame = Arc::new(ConnxFrame {
        frame: Frame {
            invocation: machine.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        mode: AtomicU8::new(0),
        lookups: AtomicUsize::new(0),
        change_at: AtomicUsize::new(usize::MAX),
        context_change: AtomicBool::new(false),
        panic_profile: AtomicBool::new(false),
        connection: issued(),
        unit: AtomicU64::new(31),
    });
    machine.bind_mqi_program_frame(frame.clone()).unwrap();
    if initialize {
        while machine.operations[machine.pc].identity.name() != "call" {
            assert!(matches!(
                machine.drive(MachineResume::Start, Quantum::new(1, 65536).unwrap()),
                MachineDrive::Continue
            ));
        }
    }
    (machine, frame)
}
fn connx(machine: &mut ReferenceMachine) -> Result<EffectRequest, MachineProblem> {
    call(
        machine,
        &[
            "MQCONNX",
            "USING",
            "MANAGER",
            "CONNECT-OPTS",
            "HCONN",
            "CC",
            "REASON",
        ],
    )
}
fn success(connection: MqHconn) -> MqMqiOutcome {
    MqMqiOutcome::Completed {
        status: MqMqiStatus::OkNone,
        output: MqMqiOutput::Connected(connection),
    }
}
fn status(call: MqMqiCall, completion: &str, reason: &str) -> MqReviewedStatus {
    MqReviewedStatus::from_symbols(call, completion, reason).unwrap()
}
fn outputs(machine: &ReferenceMachine) -> Vec<Vec<u8>> {
    ["HCONN", "CC", "REASON", "CONNECT-OPTS", "MANAGER"]
        .iter()
        .map(|s| machine.read(s).unwrap())
        .collect()
}

#[test]
fn compiled_cobol_executes_original_connx_commit_back_disconnect_with_issued_token() {
    for (text, options) in [
        (include_str!("connx.mir"), 0),
        (include_str!("connx.mir"), 32),
        (include_str!("connx-wrapped.mir"), 32),
    ] {
        let (mut machine, frame) = bind_fixture(compiled_text(text), true);
        machine
            .write_decimal(
                "CNO-OPTIONS",
                Decimal {
                    coefficient: options,
                    scale: 0,
                },
            )
            .unwrap();
        let before = machine.read("CONNECT-OPTS").unwrap();
        let mut resume = MachineResume::Start;
        for (index, expected) in [
            MqMqiCall::ConnectExtended,
            MqMqiCall::Commit,
            MqMqiCall::Back,
            MqMqiCall::Disconnect,
        ]
        .into_iter()
        .enumerate()
        {
            let MachineDrive::HostCall(effect) =
                machine.drive(resume, Quantum::new(100, 65536).unwrap())
            else {
                panic!("actual compiled CALL must dispatch")
            };
            let HostRequest::MqMqi(req) = &effect.request else {
                panic!("original typed effect")
            };
            assert_eq!(req.envelope.request.call(), expected);
            assert_eq!(effect.sequence, index as u64 + 1);
            assert_eq!(req.envelope.context, context());
            assert_eq!(effect.run_unit, machine.invocation.run_unit_id);
            assert_eq!(frame.frame.invocation, machine.invocation);
            assert_eq!(effect.deadline_tick, machine.invocation.deadline_tick);
            assert_eq!(req.mutation.sequence, effect.sequence);
            assert_eq!(
                Some(&req.mutation.idempotency_key),
                effect.idempotency_key.as_ref()
            );
            effect.validate(HostLimits::default()).unwrap();
            let output = match expected {
                MqMqiCall::ConnectExtended => {
                    assert!(matches!(
                        &req.envelope.request,
                        MqMqiRequest::ConnectExtended(MqMqiConnect {
                            manager: None,
                            sharing: MqHandleSharing::NonShared,
                            options: MqMqiOptions::ContractDefault
                        })
                    ));
                    MqMqiOutput::Connected(frame.connection)
                }
                MqMqiCall::Commit => {
                    assert_eq!(
                        req.envelope.request,
                        MqMqiRequest::Commit {
                            connection: frame.connection,
                            unit: 31
                        }
                    );
                    MqMqiOutput::UnitOfWork { unit: 31 }
                }
                MqMqiCall::Back => {
                    assert_eq!(
                        req.envelope.request,
                        MqMqiRequest::Back {
                            connection: frame.connection,
                            unit: 32
                        }
                    );
                    MqMqiOutput::UnitOfWork { unit: 32 }
                }
                _ => {
                    assert_eq!(
                        req.envelope.request,
                        MqMqiRequest::Disconnect {
                            connection: frame.connection
                        }
                    );
                    MqMqiOutput::NoOutput
                }
            };
            if expected == MqMqiCall::Commit {
                frame.unit.store(32, Ordering::SeqCst);
            }
            resume = MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                    limits: req.envelope.limits,
                    result: MqMqiResult {
                        call: expected,
                        outcome: MqMqiOutcome::Completed {
                            status: MqMqiStatus::OkNone,
                            output,
                        },
                    },
                })),
            });
        }
        assert!(matches!(
            machine.drive(resume, Quantum::new(100, 65536).unwrap()),
            MachineDrive::Completed(_)
        ));
        assert_eq!(machine.read("CONNECT-OPTS").unwrap(), before);
        assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 1);
        assert_eq!(machine.decimal("CC").unwrap().coefficient, 0);
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    }
}

#[test]
fn old_frame_default_refuses_connx_without_occurrence_or_alias() {
    let mut machine = compiled();
    let frame = Arc::new(Frame {
        invocation: machine.invocation.clone(),
        changed: AtomicBool::new(false),
    });
    machine.bind_mqi_program_frame(frame).unwrap();
    assert_eq!(
        connx(&mut machine),
        Err(MachineProblem::Host(HostProblem::Unsupported))
    );
    assert_eq!(machine.effect_sequence, 0);
    assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
}

#[test]
fn exact_manager_rules_and_compiled_prefix_capacity_preserve_all_input_bytes() {
    for (raw, name) in [
        (b"Case.QM  ".as_slice(), Some("Case.QM")),
        (b"Case\0ignored".as_slice(), Some("Case")),
        (b"   ".as_slice(), None),
    ] {
        let (mut machine, frame) = fixture(true);
        machine.write("MANAGER", raw).unwrap();
        let before = outputs(&machine);
        let effect = connx(&mut machine).unwrap();
        let HostRequest::MqMqi(req) = &effect.request else {
            panic!()
        };
        let MqMqiRequest::ConnectExtended(c) = &req.envelope.request else {
            panic!()
        };
        assert_eq!(c.manager.as_ref().map(|n| n.as_str()), name);
        reply(&mut machine, &effect, success(frame.connection)).unwrap();
        assert_eq!(machine.read("CONNECT-OPTS").unwrap(), before[3]);
        assert_eq!(machine.read("MANAGER").unwrap(), before[4]);
    }
    for raw in [
        b" BAD".as_slice(),
        b"BAD NAME".as_slice(),
        b"BAD-".as_slice(),
        &[0xff],
    ] {
        let (mut machine, _) = fixture(true);
        machine.write("MANAGER", raw).unwrap();
        let before = outputs(&machine);
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(outputs(&machine), before);
    }
}

#[test]
fn numeric_flags_versions_id_encoding_and_independent_profiles_fail_before_dispatch() {
    for options in [
        -1_i32,
        i32::MIN,
        i32::MAX,
        1,
        64,
        128,
        96,
        192,
        1024,
        2048,
        3072,
        131072,
        536870912,
    ] {
        let (mut machine, _) = fixture(true);
        machine
            .write("CNO-OPTIONS", &options.to_be_bytes())
            .unwrap();
        let before = outputs(&machine);
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(outputs(&machine), before);
    }
    for version in [-1_i32, 0, 2, 8, i32::MAX] {
        let (mut machine, _) = fixture(true);
        machine
            .write("CNO-VERSION", &version.to_be_bytes())
            .unwrap();
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
    }
    for bytes in [*b"CNO\0", [0xc3, 0xd5, 0xd6, 0x40], *b"CNOX"] {
        let (mut machine, _) = fixture(true);
        machine.write("CNO-ID", &bytes).unwrap();
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
    }
    for mode in 1..=10 {
        let (mut machine, frame) = fixture(true);
        frame.mode.store(mode, Ordering::SeqCst);
        let before = outputs(&machine);
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(outputs(&machine), before);
    }
    let (mut machine, _) = fixture(true);
    machine.write("CNO-VERSION", &1_i32.to_le_bytes()).unwrap();
    assert!(connx(&mut machine).is_err());
    assert_eq!(machine.effect_sequence, 0);
}

#[test]
fn lucky_raw_bytes_wrong_group_member_declaration_capacity_and_overlap_do_not_dispatch() {
    for mode in 0..18 {
        let (mut machine, _) = fixture(true);
        let cno = machine.layout("CONNECT-OPTS").unwrap().name.clone();
        let options = machine.layout("CNO-OPTIONS").unwrap().name.clone();
        let cc = machine.layout("CC").unwrap().name.clone();
        match mode {
            0 => machine.layouts.get_mut(&cno).unwrap().category = LayoutCategory::Alphanumeric,
            1 => {
                machine.layouts.get_mut(&options).unwrap().category = LayoutCategory::NumericDisplay
            }
            2 => machine.layouts.get_mut(&options).unwrap().signed = false,
            3 => machine.layouts.get_mut(&options).unwrap().digits = 8,
            4 => machine.layouts.get_mut(&options).unwrap().offset += 1,
            5 => machine.views.get_mut(&options).unwrap().offset += 1,
            6 => machine.layouts.get_mut(&options).unwrap().native_binary = true,
            7 => machine.layouts.get_mut(&cno).unwrap().length = 11,
            8 => machine.layouts.get_mut(&cno).unwrap().occurs = 2,
            9 => machine.layouts.get_mut(&cno).unwrap().dynamic = true,
            10 => machine.views.get_mut(&cno).unwrap().length = 11,
            11 => {
                let view = machine.views[&cno].clone();
                machine.views.insert(cc, StorageView { length: 4, ..view });
            }
            12 => machine.layouts.get_mut(&cc).unwrap().native_binary = true,
            13 => machine.layouts.get_mut(&options).unwrap().parent = None,
            14 => machine.layouts.get_mut(&cno).unwrap().length = 1025,
            15 => machine.views.get_mut(&cno).unwrap().offset = usize::MAX,
            16 => machine.layouts.get_mut(&cno).unwrap().offset = usize::MAX,
            _ => {
                let mut nested = machine.layouts[&options].clone();
                nested.name = "FORGED-NESTED".into();
                nested.parent = Some(options.clone());
                machine.layouts.insert(nested.name.clone(), nested);
            }
        }
        let before = machine.bases.clone();
        assert!(connx(&mut machine).is_err(), "mutant {mode}");
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(machine.bases, before);
    }
}

#[test]
fn by_reference_arity_and_full_input_output_overlap_are_preflighted() {
    for args in [
        vec!["MQCONNX", "USING", "MANAGER", "CONNECT-OPTS", "HCONN", "CC"],
        vec![
            "MQCONNX",
            "USING",
            "BY",
            "VALUE",
            "MANAGER",
            "CONNECT-OPTS",
            "HCONN",
            "CC",
            "REASON",
        ],
        vec![
            "MQCONNX",
            "USING",
            "BY",
            "CONTENT",
            "MANAGER",
            "CONNECT-OPTS",
            "HCONN",
            "CC",
            "REASON",
        ],
        vec![
            "MQCONNX",
            "USING",
            "MANAGER",
            "CONNECT-OPTS",
            "HCONN",
            "HCONN",
            "REASON",
        ],
        vec![
            "MQCONNX",
            "USING",
            "MANAGER",
            "CONNECT-OPTS",
            "CNO-OPTIONS",
            "CC",
            "REASON",
        ],
        vec![
            "MQCONNX",
            "USING",
            "MANAGER",
            "CONNECT-OPTS",
            "HCONN",
            "CC",
            "REASON",
            "RETURNING",
            "CC",
        ],
    ] {
        let (mut machine, _) = fixture(true);
        let before = outputs(&machine);
        assert!(call(&mut machine, &args).is_err());
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(outputs(&machine), before);
    }
}

#[test]
fn changed_lookup_context_or_profile_panics_cannot_allocate_original_effect() {
    for mode in 0..3 {
        let (mut machine, frame) = fixture(true);
        match mode {
            0 => frame.change_at.store(2, Ordering::SeqCst),
            1 => frame.context_change.store(true, Ordering::SeqCst),
            _ => frame.panic_profile.store(true, Ordering::SeqCst),
        };
        let before = outputs(&machine);
        assert!(connx(&mut machine).is_err());
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(outputs(&machine), before);
    }
}

#[test]
fn failed_connx_preserves_undefined_handle_and_exact_reviewed_status() {
    let (mut machine, _) = fixture(true);
    let before = outputs(&machine);
    let effect = connx(&mut machine).unwrap();
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::ReviewedStatus {
            status: status(
                MqMqiCall::ConnectExtended,
                "MQCC_FAILED",
                "MQRC_Q_MGR_NOT_AVAILABLE",
            ),
        },
    )
    .unwrap();
    assert_eq!(machine.decimal("CC").unwrap().coefficient, 2);
    assert_eq!(machine.decimal("REASON").unwrap().coefficient, 2059);
    assert_eq!(machine.read("HCONN").unwrap(), before[0]);
    assert_eq!(machine.read("CONNECT-OPTS").unwrap(), before[3]);
    assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
}

#[test]
fn exact_issued_token_reuses_alias_but_unrepresented_warning_output_is_unknown() {
    let (mut machine, frame) = fixture(true);
    let e = connx(&mut machine).unwrap();
    reply(&mut machine, &e, success(frame.connection)).unwrap();
    let e = connx(&mut machine).unwrap();
    reply(&mut machine, &e, success(frame.connection)).unwrap();
    assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
    assert_eq!(machine.mqi.as_ref().unwrap().next_connection, 2);
    let e = connx(&mut machine).unwrap();
    let before = outputs(&machine);
    assert_eq!(
        reply(
            &mut machine,
            &e,
            MqMqiOutcome::ReviewedOutput {
                status: status(
                    MqMqiCall::ConnectExtended,
                    "MQCC_WARNING",
                    "MQRC_ALREADY_CONNECTED",
                ),
                output: MqMqiOutput::Connected(frame.connection),
            },
        ),
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    assert_eq!(outputs(&machine), before);
    assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 1);
    assert_eq!(machine.decimal("CC").unwrap().coefficient, 0);
    assert_eq!(machine.decimal("REASON").unwrap().coefficient, 0);
    assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
    assert_eq!(machine.mqi.as_ref().unwrap().next_connection, 2);
}

#[test]
fn postdispatch_profile_refusal_change_panic_and_storage_staleness_are_unknown_atomic() {
    for mode in 0..11 {
        let (mut machine, frame) = fixture(true);
        let effect = connx(&mut machine).unwrap();
        match mode {
            0 => frame.mode.store(1, Ordering::SeqCst),
            1 => frame.mode.store(2, Ordering::SeqCst),
            2 => frame.mode.store(4, Ordering::SeqCst),
            3 => frame.frame.changed.store(true, Ordering::SeqCst),
            4 => frame.panic_profile.store(true, Ordering::SeqCst),
            5 => {
                machine.write("CNO-SUFFIX", b"DIFFERENT").unwrap();
            }
            6 => {
                machine.write("CNO-OPTIONS", &32_i32.to_be_bytes()).unwrap();
            }
            7 => {
                machine.write("MANAGER", b"changed").unwrap();
            }
            8 => {
                let cc = machine.layout("CC").unwrap().name.clone();
                machine.layouts.get_mut(&cc).unwrap().digits = 8;
            }
            9 => {
                let cno = machine.layout("CONNECT-OPTS").unwrap().name.clone();
                machine.views.get_mut(&cno).unwrap().length -= 1;
            }
            _ => frame
                .change_at
                .store(frame.lookups.load(Ordering::SeqCst) + 3, Ordering::SeqCst),
        }
        let before = machine.bases.clone();
        assert_eq!(
            reply(&mut machine, &effect, success(frame.connection)),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
            "mode {mode}"
        );
        assert_eq!(machine.bases, before);
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
        assert_eq!(machine.mqi.as_ref().unwrap().next_connection, 1);
    }
}

#[test]
fn wrong_unusable_or_unproved_reply_and_status_write_nothing() {
    for mode in 0..7 {
        let (mut machine, frame) = fixture(true);
        let effect = connx(&mut machine).unwrap();
        let before = outputs(&machine);
        let result = match mode {
            0 => MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::NoOutput,
            },
            1 => MqMqiOutcome::UnknownOutcome,
            2 => MqMqiOutcome::DuplicatePossible,
            3 => MqMqiOutcome::ReviewedStatus {
                status: status(
                    MqMqiCall::ConnectExtended,
                    "MQCC_WARNING",
                    "MQRC_ALREADY_CONNECTED",
                ),
            },
            4 => MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::Connected(MqHconn::Default),
            },
            5 => MqMqiOutcome::ReviewedStatus {
                status: status(
                    MqMqiCall::Connect,
                    "MQCC_FAILED",
                    "MQRC_Q_MGR_NOT_AVAILABLE",
                ),
            },
            _ => success(frame.connection),
        };
        let answer = if mode == 6 {
            machine.resume_host(EffectResult {
                sequence: effect.sequence + 1,
                outcome: Ok(HostResult::Clock("1".into())),
            })
        } else {
            reply(&mut machine, &effect, result)
        };
        assert_eq!(
            answer,
            Err(MachineProblem::Host(HostProblem::UnknownOutcome))
        );
        assert_eq!(outputs(&machine), before);
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    }
}

#[test]
fn typed_connx_never_serializes_live_tokens_or_accepts_typed_source_checkpoint() {
    let (mut machine, frame) = fixture(true);
    let e = connx(&mut machine).unwrap();
    reply(&mut machine, &e, success(frame.connection)).unwrap();
    assert_eq!(machine.snapshot().schema_version, 0);
    assert!(machine.checkpoint().is_none());
    let snapshot = machine.snapshot();
    let mut legacy = unbound_fixture();
    let before = legacy.snapshot();
    assert_eq!(
        legacy.restore(snapshot),
        Err(MachineProblem::IncompatibleSnapshot)
    );
    assert_eq!(legacy.snapshot(), before);
}
