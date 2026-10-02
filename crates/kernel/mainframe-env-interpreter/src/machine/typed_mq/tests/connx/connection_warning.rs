use super::*;
use mainframe_env_execution_api::ExecutionId;
use mainframe_env_host_api::{
    MqHandleObservation, canonical_request_digest, canonical_result_digest,
};

fn warning(call: MqMqiCall, connection: MqHconn) -> MqMqiOutcome {
    MqMqiOutcome::ReviewedOutput {
        status: MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap(),
        output: MqMqiOutput::Connected(connection),
    }
}
fn setup(call: MqMqiCall, child: bool) -> (ReferenceMachine, Arc<ConnxFrame>) {
    let text = match call {
        MqMqiCall::Connect => include_str!("../conn.mir"),
        MqMqiCall::ConnectExtended => include_str!("../connx.mir"),
        _ => panic!("connection only"),
    };
    let mut machine = compiled_text(text);
    if child {
        machine.invocation.parent_execution_id = Some(
            ExecutionId::new(
                "surviving-parent",
                mainframe_env_execution_api::InvocationLimits::default(),
            )
            .unwrap(),
        );
    }
    bind_fixture(machine, true)
}
fn dispatch(machine: &mut ReferenceMachine, call: MqMqiCall) -> EffectRequest {
    match call {
        MqMqiCall::Connect => connect(machine),
        MqMqiCall::ConnectExtended => connx(machine).unwrap(),
        _ => panic!("connection only"),
    }
}
fn assert_warning(machine: &ReferenceMachine, token: MqHconn) {
    assert_eq!(machine.read("HCONN").unwrap(), 1_i32.to_be_bytes());
    assert_eq!(machine.read("CC").unwrap(), 1_i32.to_be_bytes());
    assert_eq!(machine.read("REASON").unwrap(), 2002_i32.to_be_bytes());
    let state = machine.mqi.as_ref().unwrap();
    assert_eq!(state.connections.len(), 1);
    assert_eq!(state.connections.get(&1), Some(&token));
    assert_eq!(state.next_connection, 2);
}

#[test]
fn actual_compiled_calls_first_child_warning_preserve_original_effect_and_result() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let (mut machine, frame) = setup(call, true);
        let invocation = machine.invocation.clone();
        let input = machine.bases.clone();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap())
        else {
            panic!("actual compiled connection CALL")
        };
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
        let HostRequest::MqMqi(request) = &effect.request else {
            panic!("original typed effect")
        };
        assert_eq!(request.envelope.request.call(), call);
        assert_eq!(request.envelope.context, context());
        assert_eq!(request.mutation.sequence, effect.sequence);
        assert_eq!(
            Some(&request.mutation.idempotency_key),
            effect.idempotency_key.as_ref()
        );
        assert_eq!(effect.sequence, 1);
        assert_eq!(effect.run_unit, invocation.run_unit_id);
        assert_eq!(effect.deadline_tick, invocation.deadline_tick);
        assert_eq!(frame.frame.invocation, invocation);
        let request_digest = canonical_request_digest(&effect.request).unwrap();
        let result = EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                limits: request.envelope.limits,
                result: MqMqiResult {
                    call,
                    outcome: warning(call, frame.connection),
                },
            })),
        };
        let result_digest = canonical_result_digest(&result.outcome).unwrap();
        let original = result.clone();
        machine.resume_host(result).unwrap();
        assert_warning(&machine, frame.connection);
        assert_eq!(machine.invocation, invocation);
        assert_eq!(
            canonical_request_digest(&effect.request).unwrap(),
            request_digest
        );
        assert_eq!(
            canonical_result_digest(&original.outcome).unwrap(),
            result_digest
        );
        let ok = Ok(HostResult::MqMqi(MqMqiHostResult {
            limits: request.envelope.limits,
            result: MqMqiResult {
                call,
                outcome: success(frame.connection),
            },
        }));
        assert_ne!(canonical_result_digest(&ok).unwrap(), result_digest);
        // No provider registry or UOW operation occurred in the adapter. The
        // independent fixture token predates this child's first observation.
        assert_eq!(frame.unit.load(Ordering::SeqCst), 31);
        assert_eq!(machine.read("MANAGER").unwrap(), vec![b' '; 48]);
        if call == MqMqiCall::ConnectExtended {
            let layout = machine.layout("CONNECT-OPTS").unwrap();
            let view = &machine.views[&layout.name];
            assert_eq!(
                machine.read("CONNECT-OPTS").unwrap(),
                input[view.base][view.offset..view.offset + view.length]
            );
        }
    }
}

#[test]
fn both_calls_reuse_exact_prior_alias_without_new_token_or_unit() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        for first_warning in [false, true] {
            let (mut machine, frame) = setup(call, false);
            let effect = dispatch(&mut machine, call);
            let first = if first_warning {
                warning(call, frame.connection)
            } else {
                success(frame.connection)
            };
            reply(&mut machine, &effect, first).unwrap();
            for _ in 0..2 {
                let effect = dispatch(&mut machine, call);
                reply(&mut machine, &effect, warning(call, frame.connection)).unwrap();
                assert_warning(&machine, frame.connection);
            }
            assert_eq!(frame.unit.load(Ordering::SeqCst), 31);
        }
    }
}

#[test]
fn both_calls_refuse_warning_without_exact_nonhistorical_issued_output() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        for mode in 0..11 {
            let (mut machine, frame) = setup(call, true);
            let effect = dispatch(&mut machine, call);
            let historical = MqHandleObservation::capture_connection(frame.connection)
                .unwrap()
                .historical_connection()
                .unwrap();
            let outcome = match mode {
                0 => MqMqiOutcome::ReviewedStatus {
                    status: MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap(),
                },
                1 => warning(call, MqHconn::Default),
                2 => warning(call, MqHconn::Unassociated),
                3 => warning(call, historical),
                4 => MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap(),
                    output: MqMqiOutput::UnitOfWork { unit: 31 },
                },
                5 => MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_wire_pair(call, 2, 2035).unwrap(),
                    output: MqMqiOutput::Connected(frame.connection),
                },
                6 => MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_wire_pair(call, 1, 2391).unwrap(),
                    output: MqMqiOutput::Connected(frame.connection),
                },
                7 => warning(
                    if call == MqMqiCall::Connect {
                        MqMqiCall::ConnectExtended
                    } else {
                        MqMqiCall::Connect
                    },
                    frame.connection,
                ),
                8 => MqMqiOutcome::DuplicatePossible,
                9 => MqMqiOutcome::StatusPending {
                    output: MqMqiOutput::Connected(frame.connection),
                },
                _ => MqMqiOutcome::UnknownOutcome,
            };
            let before = machine.bases.clone();
            assert_eq!(
                reply(&mut machine, &effect, outcome),
                Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
                "{call:?} mode{mode}"
            );
            assert_eq!(machine.bases, before);
            assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
            assert_eq!(machine.mqi.as_ref().unwrap().next_connection, 1);
        }
    }
}

#[test]
fn both_warning_calls_require_exact_result_call_limits_and_sequence() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        for mode in 0..3 {
            let (mut machine, frame) = setup(call, false);
            let effect = dispatch(&mut machine, call);
            let before = machine.bases.clone();
            let mut limits = MqMqiLimits::default();
            if mode == 0 {
                limits.canonical_bytes -= 1;
            }
            let result = EffectResult {
                sequence: effect.sequence + u64::from(mode == 1),
                outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                    limits,
                    result: MqMqiResult {
                        call: if mode == 2 {
                            MqMqiCall::Disconnect
                        } else {
                            call
                        },
                        outcome: warning(call, frame.connection),
                    },
                })),
            };
            assert_eq!(
                machine.resume_host(result),
                Err(MachineProblem::Host(HostProblem::UnknownOutcome))
            );
            assert_eq!(machine.bases, before);
            assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
        }
    }
}

#[test]
fn connx_warning_keeps_options32_wrapped_prefix_and_suffix_unchanged() {
    let (mut machine, frame) =
        bind_fixture(compiled_text(include_str!("../connx-wrapped.mir")), true);
    let before = machine.read("CONNECT-OPTS").unwrap();
    let effect = dispatch(&mut machine, MqMqiCall::ConnectExtended);
    reply(
        &mut machine,
        &effect,
        warning(MqMqiCall::ConnectExtended, frame.connection),
    )
    .unwrap();
    assert_warning(&machine, frame.connection);
    assert_eq!(machine.read("CONNECT-OPTS").unwrap(), before);
    assert_eq!(machine.read("CNO-OPTIONS").unwrap(), 32_i32.to_be_bytes());
}

#[test]
fn connx_warning_stale_suffix_or_independent_abi_profile_is_atomic_unknown() {
    for mode in 0..3 {
        let (mut machine, frame) = setup(MqMqiCall::ConnectExtended, false);
        let effect = dispatch(&mut machine, MqMqiCall::ConnectExtended);
        match mode {
            0 => machine.write("CNO-SUFFIX", b"DIFFERENT").unwrap(),
            1 => frame.mode.store(4, Ordering::SeqCst),
            _ => frame
                .change_at
                .store(frame.lookups.load(Ordering::SeqCst) + 3, Ordering::SeqCst),
        }
        let before = machine.bases.clone();
        assert_eq!(
            reply(
                &mut machine,
                &effect,
                warning(MqMqiCall::ConnectExtended, frame.connection)
            ),
            Err(MachineProblem::Host(HostProblem::UnknownOutcome))
        );
        assert_eq!(machine.bases, before);
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    }
}

#[test]
fn both_calls_warning_stale_storage_profile_refusal_and_panic_write_nothing() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        for mode in 0..8 {
            let (mut machine, frame) = setup(call, false);
            let effect = dispatch(&mut machine, call);
            match mode {
                0 => frame.frame.changed.store(true, Ordering::SeqCst),
                1 => frame.panic_profile.store(true, Ordering::SeqCst),
                2 => frame.profile_refuse_at.store(0, Ordering::SeqCst),
                3 => {
                    let count = frame.profile_lookups.load(Ordering::SeqCst);
                    frame.profile_refuse_at.store(
                        count + if call == MqMqiCall::Connect { 3 } else { 4 },
                        Ordering::SeqCst,
                    );
                }
                4 => machine.write("CC", &29_i32.to_be_bytes()).unwrap(),
                5 => machine.write("MANAGER", b"CHANGED").unwrap(),
                6 => {
                    let name = machine.layout("REASON").unwrap().name.clone();
                    machine.layouts.get_mut(&name).unwrap().native_binary = true;
                }
                _ => {
                    let name = machine.layout("REASON").unwrap().name.clone();
                    machine.views.get_mut(&name).unwrap().offset += 1;
                }
            }
            let before = machine.bases.clone();
            assert_eq!(
                reply(&mut machine, &effect, warning(call, frame.connection)),
                Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
                "{call:?} mode{mode}"
            );
            assert_eq!(machine.bases, before);
            assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
        }
    }
}

#[test]
fn both_failed_calls_keep_undefined_handle_and_original_status() {
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let (mut machine, _) = setup(call, false);
        let effect = dispatch(&mut machine, call);
        let handle = machine.read("HCONN").unwrap();
        reply(
            &mut machine,
            &effect,
            MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_wire_pair(call, 2, 2059).unwrap(),
            },
        )
        .unwrap();
        assert_eq!(machine.read("HCONN").unwrap(), handle);
        assert_eq!(machine.read("CC").unwrap(), 2_i32.to_be_bytes());
        assert_eq!(machine.read("REASON").unwrap(), 2059_i32.to_be_bytes());
        assert!(machine.mqi.as_ref().unwrap().connections.is_empty());
    }
}

#[test]
fn conn_preflight_catches_panic_native_storage_and_manager_output_overlap() {
    for mode in 0..3 {
        let (mut machine, frame) = setup(MqMqiCall::Connect, false);
        match mode {
            0 => frame.panic_profile.store(true, Ordering::SeqCst),
            1 => {
                let name = machine.layout("REASON").unwrap().name.clone();
                machine.layouts.get_mut(&name).unwrap().native_binary = true;
            }
            _ => {
                let manager = machine.layout("MANAGER").unwrap().name.clone();
                let handle = machine.layout("HCONN").unwrap().name.clone();
                let view = machine.views[&manager].clone();
                machine
                    .views
                    .insert(handle, StorageView { length: 4, ..view });
            }
        }
        let before = machine.bases.clone();
        assert!(
            call(
                &mut machine,
                &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
            )
            .is_err()
        );
        assert_eq!(machine.effect_sequence, 0);
        assert_eq!(machine.bases, before);
    }
}
