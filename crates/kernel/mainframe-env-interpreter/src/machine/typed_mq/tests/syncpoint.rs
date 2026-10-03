use super::*;
use mainframe_env_host_api::mq_status::{MqStatusReview, mq_status_call};
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize};

struct UnitFrame {
    frame: Frame,
    connection: MqHconn,
    unit: AtomicU64,
    mode: AtomicU8,
    changed_during_lookup: AtomicBool,
    lookups: AtomicUsize,
}
impl MqMqiProgramFrame for UnitFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.frame.profile(invocation)
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
        self.lookups.fetch_add(1, Ordering::SeqCst);
        if self.changed_during_lookup.load(Ordering::SeqCst) {
            self.frame.changed.store(true, Ordering::SeqCst);
        }
        let unit = self.unit.load(Ordering::SeqCst);
        match self.mode.load(Ordering::SeqCst) {
            0 => Ok(MqMqiUnitOfWork::Local { unit }),
            1 => Ok(MqMqiUnitOfWork::NoSyncpoint),
            2 => Ok(MqMqiUnitOfWork::ExternalPending { unit }),
            _ => Err(HostProblem::UnknownOutcome),
        }
    }
}
fn connected_fixture() -> (ReferenceMachine, Arc<UnitFrame>) {
    let mut machine = unbound_fixture();
    let frame = Arc::new(UnitFrame {
        frame: Frame {
            invocation: machine.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        connection: issued(),
        unit: AtomicU64::new(31),
        mode: AtomicU8::new(0),
        changed_during_lookup: AtomicBool::new(false),
        lookups: AtomicUsize::new(0),
    });
    machine.bind_mqi_program_frame(frame.clone()).unwrap();
    let effect = connect(&mut machine);
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::Connected(frame.connection),
        },
    )
    .unwrap();
    (machine, frame)
}

#[test]
fn original_syncpoint_effect_uses_live_connection_and_fresh_provider_unit() {
    let (mut machine, frame) = connected_fixture();
    let handle_bytes = machine.read("HCONN").unwrap();
    for (name, call_id, unit) in [
        ("MQCMIT", MqMqiCall::Commit, 31),
        ("MQBACK", MqMqiCall::Back, 32),
    ] {
        frame.unit.store(unit, Ordering::SeqCst);
        let effect = call(&mut machine, &[name, "USING", "HCONN", "CC", "REASON"]).unwrap();
        let HostRequest::MqMqi(value) = &effect.request else {
            panic!("original typed request")
        };
        assert_eq!(effect.run_unit, machine.invocation.run_unit_id);
        assert_eq!(value.envelope.context, context());
        assert_eq!(value.mutation.sequence, effect.sequence);
        assert_eq!(
            Some(&value.mutation.idempotency_key),
            effect.idempotency_key.as_ref()
        );
        assert_eq!(value.envelope.request.call(), call_id);
        assert_eq!(
            value.envelope.request,
            match call_id {
                MqMqiCall::Commit => MqMqiRequest::Commit {
                    connection: frame.connection,
                    unit
                },
                _ => MqMqiRequest::Back {
                    connection: frame.connection,
                    unit
                },
            }
        );
        effect.validate(HostLimits::default()).unwrap();
        let digest = mainframe_env_host_api::canonical_request_digest(&effect.request);
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::UnitOfWork { unit },
            },
        )
        .unwrap();
        assert_eq!(
            digest,
            mainframe_env_host_api::canonical_request_digest(&effect.request)
        );
        assert_eq!(machine.read("HCONN").unwrap(), handle_bytes);
        assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
        assert_eq!(machine.decimal("CC").unwrap().coefficient, 0);
        assert_eq!(machine.decimal("REASON").unwrap().coefficient, 0);
    }
    assert_eq!(frame.lookups.load(Ordering::SeqCst), 2);
}

#[test]
fn every_admitted_syncpoint_status_preserves_exact_wire_pair_without_deciding_uow() {
    for (name, call_id) in [("MQCMIT", MqMqiCall::Commit), ("MQBACK", MqMqiCall::Back)] {
        let mut checked = 0;
        for pair in mq_status_call(call_id)
            .pairs()
            .filter(|p| p.review == MqStatusReview::Admitted)
        {
            let status = mainframe_env_host_api::mq_status::MqReviewedStatus::from_symbols(
                call_id,
                pair.completion.symbol(),
                pair.reason_symbol,
            )
            .unwrap();
            let (mut machine, frame) = connected_fixture();
            let effect = call(&mut machine, &[name, "USING", "HCONN", "CC", "REASON"]).unwrap();
            let before = machine.read("HCONN").unwrap();
            reply(
                &mut machine,
                &effect,
                MqMqiOutcome::ReviewedStatus { status },
            )
            .unwrap();
            let (completion, reason) = status.wire_pair();
            assert_eq!(
                machine.decimal("CC").unwrap().coefficient,
                i128::from(completion)
            );
            assert_eq!(
                machine.decimal("REASON").unwrap().coefficient,
                i128::from(reason)
            );
            assert_eq!(machine.read("HCONN").unwrap(), before);
            assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
            assert_eq!(frame.unit.load(Ordering::SeqCst), 31);
            checked += 1;
        }
        assert_eq!(checked, mq_status_call(call_id).pairs().count());
    }
}

#[test]
fn lookup_is_not_a_binding_grant_and_unrepresented_units_fail_before_effect() {
    for mode in [0, 1, 2, 3] {
        let (mut machine, frame) = connected_fixture();
        frame.mode.store(mode, Ordering::SeqCst);
        if mode == 0 {
            frame.unit.store(0, Ordering::SeqCst);
        }
        let before = machine.snapshot();
        assert!(call(&mut machine, &["MQCMIT", "USING", "HCONN", "CC", "REASON"]).is_err());
        assert_eq!(machine.snapshot(), before);
        assert_eq!(machine.effect_sequence, 1);
    }
    let (mut machine, _) = fixture();
    let effect = connect(&mut machine);
    reply(
        &mut machine,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::Connected(issued()),
        },
    )
    .unwrap();
    assert_eq!(
        call(&mut machine, &["MQCMIT", "USING", "HCONN", "CC", "REASON"]),
        Err(MachineProblem::Host(HostProblem::Unsupported))
    );
    assert_eq!(machine.effect_sequence, 1);
}

#[test]
fn arity_alias_foreign_handle_and_changed_lookup_cannot_allocate_an_occurrence() {
    for args in [
        vec!["MQCMIT", "USING", "HCONN", "CC"],
        vec!["MQBACK", "USING", "HCONN", "HCONN", "REASON"],
        vec!["MQCMIT", "USING", "BY", "CONTENT", "HCONN", "CC", "REASON"],
    ] {
        let (mut machine, frame) = connected_fixture();
        assert!(call(&mut machine, &args).is_err());
        assert_eq!(machine.effect_sequence, 1);
        assert_eq!(frame.lookups.load(Ordering::SeqCst), 0);
    }
    let (mut machine, frame) = connected_fixture();
    machine.write("HCONN", &77_i32.to_be_bytes()).unwrap();
    assert!(call(&mut machine, &["MQBACK", "USING", "HCONN", "CC", "REASON"]).is_err());
    assert_eq!(frame.lookups.load(Ordering::SeqCst), 0);
    machine.write("HCONN", &1_i32.to_be_bytes()).unwrap();
    frame.changed_during_lookup.store(true, Ordering::SeqCst);
    assert!(call(&mut machine, &["MQBACK", "USING", "HCONN", "CC", "REASON"]).is_err());
    assert_eq!(machine.effect_sequence, 1);
}

#[test]
fn mismatched_unit_changed_frame_unknown_and_malformed_reply_write_nothing() {
    for mode in [0, 1, 2, 3, 4] {
        let (mut machine, frame) = connected_fixture();
        let effect = call(&mut machine, &["MQCMIT", "USING", "HCONN", "CC", "REASON"]).unwrap();
        machine.write("CC", &9_i32.to_be_bytes()).unwrap();
        machine.write("REASON", &17_i32.to_be_bytes()).unwrap();
        let before = [
            machine.read("HCONN").unwrap(),
            machine.read("CC").unwrap(),
            machine.read("REASON").unwrap(),
        ];
        let outcome = match mode {
            0 => MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::UnitOfWork { unit: 32 },
            },
            1 => {
                frame.frame.changed.store(true, Ordering::SeqCst);
                MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output: MqMqiOutput::UnitOfWork { unit: 31 },
                }
            }
            2 => MqMqiOutcome::UnknownOutcome,
            3 => MqMqiOutcome::DuplicatePossible,
            _ => MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::NoOutput,
            },
        };
        assert_eq!(
            reply(&mut machine, &effect, outcome),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome))
        );
        assert_eq!(
            [
                machine.read("HCONN").unwrap(),
                machine.read("CC").unwrap(),
                machine.read("REASON").unwrap()
            ],
            before
        );
        assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
    }
}

#[test]
fn unusable_typed_envelope_is_unknown_while_legacy_validation_keeps_its_error() {
    let (mut machine, _) = connected_fixture();
    let effect = call(&mut machine, &["MQCMIT", "USING", "HCONN", "CC", "REASON"]).unwrap();
    let result = EffectResult {
        sequence: effect.sequence + 1,
        outcome: Ok(HostResult::Clock("1".into())),
    };
    let legacy = Pending {
        sequence: effect.sequence,
        kind: PendingKind::Ignore,
    };
    assert_eq!(
        validate_reply(&legacy, &result),
        result
            .validate(effect.sequence, HostLimits::default())
            .map_err(MachineProblem::Host)
    );
    let before = machine.read("CC").unwrap();
    assert_eq!(
        machine.resume_host(result),
        Err(MachineProblem::Host(HostProblem::UnknownOutcome))
    );
    assert_eq!(machine.read("CC").unwrap(), before);
}

#[test]
fn existing_failed_environment_outcome_uses_reviewed_numbers_without_retiring_alias() {
    for name in ["MQCMIT", "MQBACK"] {
        let (mut machine, _) = connected_fixture();
        let effect = call(&mut machine, &[name, "USING", "HCONN", "CC", "REASON"]).unwrap();
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            },
        )
        .unwrap();
        assert_eq!(machine.decimal("CC").unwrap().coefficient, 2);
        assert_eq!(machine.decimal("REASON").unwrap().coefficient, 2012);
        assert_eq!(machine.mqi.as_ref().unwrap().connections.len(), 1);
    }
}
