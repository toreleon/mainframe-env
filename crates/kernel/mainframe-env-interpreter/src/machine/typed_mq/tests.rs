use super::*;
use mainframe_env_host_api::mq_mqi::MqMqiResult;
use mainframe_env_host_api::{MqHandleOwner, MqHandleRegistry};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) mod connx;
mod syncpoint;

pub(super) struct Frame {
    pub(super) invocation: Invocation,
    pub(super) changed: AtomicBool,
}
impl MqMqiProgramFrame for Frame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        if invocation != &self.invocation {
            return Err(HostProblem::Unauthorized);
        }
        if invocation.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        let mut context = context();
        if self.changed.load(Ordering::SeqCst) {
            context.owner.task_id += 1;
        }
        Ok(MqMqiProgramProfile {
            context,
            limits: MqMqiLimits::default(),
        })
    }
}
pub(super) fn context() -> MqMqiContext {
    MqMqiContext {
        owner: MqHandleOwner {
            environment: MqHostEnvironment::ZosBatch,
            host_id: 1,
            process_id: 2,
            thread_id: 3,
            task_id: 4,
            syncpoint_epoch: 1,
        },
        syncpoint_owner: MqSyncpointOwner::QueueManager,
    }
}
fn field(machine: &mut ReferenceMachine, name: &str, bytes: Vec<u8>, numeric: bool) {
    let length = bytes.len();
    let base = machine.bases.len();
    machine.bases.push(bytes);
    machine.views.insert(
        name.into(),
        StorageView {
            base,
            offset: 0,
            length,
        },
    );
    machine
        .simple_layouts
        .insert(name.into(), vec![name.into()]);
    machine.layouts.insert(
        name.into(),
        LayoutMetadata {
            name: name.into(),
            simple_name: name.into(),
            category: if numeric {
                LayoutCategory::Binary
            } else {
                LayoutCategory::Alphanumeric
            },
            picture: if numeric {
                "S9(9)".into()
            } else {
                "X(48)".into()
            },
            digits: if numeric { 9 } else { 0 },
            scale: 0,
            native_binary: false,
            signed: numeric,
            sign_separate: false,
            justified_right: false,
            blank_when_zero: false,
            linkage: false,
            offset: 0,
            length,
            element_length: length,
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: vec![],
            keys: vec![],
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: vec![],
            object_class: None,
        },
    );
}
pub(super) fn unbound_fixture() -> ReferenceMachine {
    let invocation = super::super::tests::invocation();
    let mut machine = ReferenceMachine::from_binary(
        &super::super::tests::binary(),
        invocation,
        CodecLimits::default(),
    )
    .unwrap();
    field(&mut machine, "MANAGER", vec![b' '; 48], false);
    for name in ["HCONN", "CC", "REASON"] {
        field(&mut machine, name, vec![0; 4], true);
    }
    machine
}
fn fixture() -> (ReferenceMachine, Arc<Frame>) {
    let mut machine = unbound_fixture();
    let frame = Arc::new(Frame {
        invocation: machine.invocation.clone(),
        changed: AtomicBool::new(false),
    });
    machine.bind_mqi_program_frame(frame.clone()).unwrap();
    (machine, frame)
}
pub(super) fn call(
    machine: &mut ReferenceMachine,
    args: &[&str],
) -> Result<EffectRequest, MachineProblem> {
    let operation = machine.operations[0].clone();
    let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match machine.program_effect(&operation, "call", &args)? {
        Step::Effect(effect) => Ok(*effect),
        _ => panic!("typed effect required"),
    }
}
pub(super) fn connect(machine: &mut ReferenceMachine) -> EffectRequest {
    call(
        machine,
        &["'MQCONN'", "USING", "MANAGER", "HCONN", "CC", "REASON"],
    )
    .unwrap()
}
pub(super) fn reply(
    machine: &mut ReferenceMachine,
    effect: &EffectRequest,
    outcome: MqMqiOutcome,
) -> Result<(), MachineProblem> {
    let HostRequest::MqMqi(request) = &effect.request else {
        panic!("typed request")
    };
    machine.resume_host(EffectResult {
        sequence: effect.sequence,
        outcome: Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
            limits: request.envelope.limits,
            result: MqMqiResult {
                call: request.envelope.request.call(),
                outcome,
            },
        }))),
    })
}
pub(super) fn issued() -> MqHconn {
    MqHandleRegistry::new(1, 4)
        .unwrap()
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap()
}

#[test]
fn original_typed_connection_effect_preserves_frame_and_core_occurrence() {
    let (mut machine, _) = fixture();
    let effect = connect(&mut machine);
    let HostRequest::MqMqi(value) = &effect.request else {
        panic!("typed request")
    };
    assert_eq!(value.envelope.context, context());
    assert_eq!(effect.run_unit, machine.invocation.run_unit_id);
    assert_eq!(effect.sequence, 1);
    assert_eq!(value.mutation.sequence, effect.sequence);
    assert_eq!(
        Some(&value.mutation.idempotency_key),
        effect.idempotency_key.as_ref()
    );
    assert!(effect.validate(HostLimits::default()).is_ok());
    assert_eq!(
        mainframe_env_host_api::canonical_request_digest(&effect.request),
        mainframe_env_host_api::canonical_request_digest(&effect.clone().request)
    );
    assert!(matches!(
        &value.envelope.request,
        MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            ..
        })
    ));
}

#[test]
fn manager_name_keeps_case_padding_null_and_rejects_embedded_blanks() {
    for (raw, expected) in [
        (b"mixed.QM".as_slice(), "mixed.QM"),
        (b"Case\0ignored".as_slice(), "Case"),
    ] {
        let (mut machine, _) = fixture();
        machine.write("MANAGER", raw).unwrap();
        let effect = connect(&mut machine);
        let HostRequest::MqMqi(value) = effect.request else {
            panic!("typed")
        };
        let MqMqiRequest::Connect(value) = value.envelope.request else {
            panic!("connect")
        };
        assert_eq!(value.manager.unwrap().as_str(), expected);
    }
    let (mut machine, _) = fixture();
    machine.write("MANAGER", b" BAD NAME").unwrap();
    assert!(
        call(
            &mut machine,
            &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
        )
        .is_err()
    );
    assert_eq!(machine.effect_sequence, 0);
}

#[test]
fn issued_tokens_are_only_translated_and_successful_disconnect_retires_alias() {
    let (mut machine, _) = fixture();
    let connection = issued();
    let effect = connect(&mut machine);
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::Connected(connection),
        },
    )
    .unwrap();
    assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 1);
    let effect = call(&mut machine, &["MQDISC", "USING", "HCONN", "CC", "REASON"]).unwrap();
    let HostRequest::MqMqi(value) = &effect.request else {
        panic!("typed")
    };
    assert_eq!(
        value.envelope.request,
        MqMqiRequest::Disconnect { connection }
    );
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::NoOutput,
        },
    )
    .unwrap();
    assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 1);
    assert!(call(&mut machine, &["MQDISC", "USING", "HCONN", "CC", "REASON"]).is_err());
}

#[test]
fn malformed_arity_output_layout_alias_and_foreign_wire_fail_before_effect() {
    for args in [
        vec!["MQCONN", "USING", "MANAGER", "HCONN", "CC"],
        vec!["MQCONN", "USING", "MANAGER", "HCONN", "HCONN", "REASON"],
        vec![
            "MQCONN", "USING", "BY", "VALUE", "MANAGER", "HCONN", "CC", "REASON",
        ],
        vec!["MQDISC", "USING", "HCONN", "CC", "REASON"],
    ] {
        let (mut machine, _) = fixture();
        assert!(call(&mut machine, &args).is_err());
        assert_eq!(machine.effect_sequence, 0);
    }
    let (mut machine, _) = fixture();
    machine.layouts.get_mut("CC").unwrap().signed = false;
    assert!(
        call(
            &mut machine,
            &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
        )
        .is_err()
    );
    assert_eq!(machine.effect_sequence, 0);
}

#[test]
fn changed_frame_is_rejected_before_dispatch_and_unknown_after_dispatch() {
    let (mut machine, frame) = fixture();
    frame.changed.store(true, Ordering::SeqCst);
    assert!(
        call(
            &mut machine,
            &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
        )
        .is_err()
    );
    assert_eq!(machine.effect_sequence, 0);
    frame.changed.store(false, Ordering::SeqCst);
    let effect = connect(&mut machine);
    frame.changed.store(true, Ordering::SeqCst);
    assert_eq!(
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::Connected(issued())
            }
        ),
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 0);
}

#[test]
fn failed_connect_preserves_handle_and_exact_reviewed_status_but_unknown_writes_nothing() {
    let (mut machine, _) = fixture();
    let effect = connect(&mut machine);
    let status = mainframe_env_host_api::mq_status::MqReviewedStatus::from_wire_pair(
        MqMqiCall::Connect,
        2,
        2059,
    )
    .unwrap();
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::ReviewedStatus { status },
    )
    .unwrap();
    assert_eq!(machine.decimal("CC").unwrap().coefficient, 2);
    assert_eq!(machine.decimal("REASON").unwrap().coefficient, 2059);
    assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 0);
    let effect = connect(&mut machine);
    assert_eq!(
        reply(&mut machine, &effect, MqMqiOutcome::UnknownOutcome),
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    assert_eq!(machine.decimal("REASON").unwrap().coefficient, 2059);
}

#[test]
fn typed_frame_cannot_rebind_or_silently_restore_legacy_checkpoint() {
    let (mut machine, frame) = fixture();
    assert!(machine.bind_mqi_program_frame(frame).is_err());
    assert!(machine.checkpoint().is_none());
    assert_eq!(
        machine.restore(machine.snapshot()),
        Err(MachineProblem::IncompatibleSnapshot)
    );
    let legacy = ReferenceMachine::from_binary(
        &super::super::tests::binary(),
        super::super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap();
    assert!(legacy.checkpoint().is_some());
    assert!(is_call("'MQPUT1'"));
    assert!(!is_call("'MQ-USER-PROGRAM'"));
    let (mut machine, _) = fixture();
    assert!(call(&mut machine, &["MQPUT1", "USING", "HCONN"]).is_err());
}

#[test]
fn typed_source_snapshots_refuse_unbound_restore_before_and_after_connect_without_changes() {
    for connected in [false, true] {
        let (mut source, _) = fixture();
        if connected {
            let effect = connect(&mut source);
            reply(
                &mut source,
                &effect,
                MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output: MqMqiOutput::Connected(issued()),
                },
            )
            .unwrap();
            assert_eq!(source.mqi.as_ref().unwrap().connections.len(), 1);
        }
        let snapshot = source.snapshot();
        assert_eq!(snapshot.schema_version, 0);
        assert!(source.checkpoint().is_none());
        let mut destination = unbound_fixture();
        destination.write("HCONN", &[0x33; 4]).unwrap();
        let before = destination.snapshot();
        assert_eq!(
            destination.restore(snapshot.clone()),
            Err(MachineProblem::IncompatibleSnapshot)
        );
        assert_eq!(destination.snapshot(), before);
        assert!(destination.mqi.is_none());
        // The manual binary codec retains the invalid source marker, too. MachineSnapshot
        // has no Serde implementation that could erase it into a valid legacy projection.
        let bytes = snapshot_codec::encode_snapshot(&snapshot).unwrap();
        let diagnostic = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@12",
            bytes,
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            destination.restore_checkpoint(&diagnostic),
            Err(MachineProblem::IncompatibleSnapshot)
        );
        assert_eq!(destination.snapshot(), before);
        assert_eq!(
            source.restore(snapshot),
            Err(MachineProblem::IncompatibleSnapshot)
        );
    }
}

#[test]
fn legacy_cross_instance_snapshot_and_checkpoint_restore_keep_exact_bytes() {
    let mut source = unbound_fixture();
    source.write("HCONN", &[0x22; 4]).unwrap();
    let snapshot = source.snapshot();
    assert_eq!(snapshot.schema_version, 12);
    let checkpoint = source.checkpoint().unwrap();
    let mut destination = unbound_fixture();
    destination.restore(snapshot.clone()).unwrap();
    assert_eq!(destination.snapshot(), snapshot);
    assert_eq!(destination.checkpoint(), Some(checkpoint.clone()));
    let mut reopened = unbound_fixture();
    reopened.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(reopened.snapshot(), snapshot);
    assert_eq!(reopened.checkpoint(), Some(checkpoint));
}

#[test]
fn alias_capacity_is_checked_before_connect_dispatch() {
    let (mut machine, _) = fixture();
    machine.mqi.as_mut().unwrap().next_connection = i32::MAX;
    assert_eq!(
        call(
            &mut machine,
            &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
        ),
        Err(MachineProblem::ResourceExhausted)
    );
    assert_eq!(machine.effect_sequence, 0);
}

#[test]
fn explicit_reference_arguments_preserve_the_default_signature_and_unusable_reply_is_unknown() {
    let (mut machine, _) = fixture();
    let effect = call(
        &mut machine,
        &[
            "MQCONN",
            "USING",
            "BY",
            "REFERENCE",
            "MANAGER",
            "HCONN",
            "BY",
            "REFERENCE",
            "CC",
            "REASON",
            "END-CALL",
        ],
    )
    .unwrap();
    assert!(matches!(effect.request, HostRequest::MqMqi(_)));
    let result = machine.resume_host(EffectResult {
        sequence: effect.sequence,
        outcome: Ok(HostResult::Clock("1".into())),
    });
    assert_eq!(
        result,
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    for name in ["HCONN", "CC", "REASON"] {
        assert_eq!(machine.decimal(name).unwrap().coefficient, 0);
    }
}

#[test]
fn historical_connection_reply_cannot_install_a_live_abi_alias() {
    let (mut machine, _) = fixture();
    let effect = connect(&mut machine);
    let original = issued();
    let historical = mainframe_env_host_api::MqHandleObservation::capture_connection(original)
        .unwrap()
        .historical_connection()
        .unwrap();
    assert!(historical.is_historical());
    assert_eq!(
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::Connected(historical),
            },
        ),
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    for name in ["HCONN", "CC", "REASON"] {
        assert_eq!(machine.decimal(name).unwrap().coefficient, 0);
    }
}

#[test]
fn reviewed_ok_output_installs_only_live_connection_and_disconnect_retires_it() {
    for historical in [false, true] {
        let (mut machine, _) = fixture();
        let effect = connect(&mut machine);
        let live = issued();
        let connection = if historical {
            mainframe_env_host_api::MqHandleObservation::capture_connection(live)
                .unwrap()
                .historical_connection()
                .unwrap()
        } else {
            live
        };
        let status = mainframe_env_host_api::mq_status::MqReviewedStatus::from_symbols(
            MqMqiCall::Connect,
            "MQCC_OK",
            "MQRC_NONE",
        )
        .unwrap();
        let result = reply(
            &mut machine,
            &effect,
            MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Connected(connection),
            },
        );
        if historical {
            assert_eq!(
                result,
                Err(MachineProblem::Host(HostProblem::UnknownOutcome))
            );
            assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
            assert_eq!(machine.decimal("HCONN").unwrap().coefficient, 0);
            continue;
        }
        result.unwrap();
        assert_eq!(
            machine.mqi.as_ref().unwrap().connections.get(&1),
            Some(&live)
        );
        let effect = call(&mut machine, &["MQDISC", "USING", "HCONN", "CC", "REASON"]).unwrap();
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::ReviewedOutput {
                status: mainframe_env_host_api::mq_status::MqReviewedStatus::from_symbols(
                    MqMqiCall::Disconnect,
                    "MQCC_OK",
                    "MQRC_NONE",
                )
                .unwrap(),
                output: MqMqiOutput::NoOutput,
            },
        )
        .unwrap();
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
        assert_eq!(machine.decimal("CC").unwrap().coefficient, 0);
        assert_eq!(machine.decimal("REASON").unwrap().coefficient, 0);
    }
}
