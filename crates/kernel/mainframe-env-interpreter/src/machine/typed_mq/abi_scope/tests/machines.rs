use super::super::super::tests::{Frame, call, connect, reply, unbound_fixture};
use super::*;
use std::sync::atomic::AtomicBool;

struct SharedFrame {
    frame: Frame,
    scope: Mutex<Option<Arc<MqMqiAbiScope>>>,
    connection: MqHconn,
}
impl MqMqiProgramFrame for SharedFrame {
    fn profile(&self, original: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.frame.profile(original)
    }
    fn abi_scope(&self, original: &Invocation) -> Result<Option<Arc<MqMqiAbiScope>>, HostProblem> {
        self.frame.profile(original)?;
        Ok(self.scope.lock().unwrap().clone())
    }
    fn connx_profile(&self, original: &Invocation) -> Result<MqMqiConnxProfile, HostProblem> {
        self.frame.profile(original)?;
        Ok(super::super::super::tests::connx::ordinary())
    }
    fn local_unit(
        &self,
        original: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.frame.profile(original)?;
        if connection != self.connection {
            return Err(HostProblem::Unauthorized);
        }
        Ok(MqMqiUnitOfWork::Local { unit: 31 })
    }
}
fn bind(
    mut machine: ReferenceMachine,
    scope: Arc<MqMqiAbiScope>,
    token: MqHconn,
) -> (ReferenceMachine, Arc<SharedFrame>) {
    let frame = Arc::new(SharedFrame {
        frame: Frame {
            invocation: machine.invocation.clone(),
            changed: AtomicBool::new(false),
        },
        scope: Mutex::new(Some(scope)),
        connection: token,
    });
    machine.bind_mqi_program_frame(frame.clone()).unwrap();
    (machine, frame)
}
fn success(token: MqHconn) -> MqMqiOutcome {
    MqMqiOutcome::Completed {
        status: MqMqiStatus::OkNone,
        output: MqMqiOutput::Connected(token),
    }
}

#[test]
fn same_scope_child_uses_parent_alias_and_return_does_not_retire_it() {
    let scope = scope(4);
    let token = issued();
    let (mut parent, _) = bind(unbound_fixture(), scope.clone(), token);
    let first = connect(&mut parent);
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
    reply(&mut parent, &first, success(token)).unwrap();
    assert_eq!(parent.read("HCONN").unwrap(), 1_i32.to_be_bytes());
    let mut child = unbound_fixture();
    child.invocation.execution_id =
        mainframe_env_execution_api::ExecutionId::new("same-task-child", Default::default())
            .unwrap();
    child.invocation.parent_execution_id = Some(parent.invocation.execution_id.clone());
    let (mut child, _) = bind(child, scope.clone(), token);
    child
        .write("HCONN", &parent.read("HCONN").unwrap())
        .unwrap();
    let effect = call(&mut child, &["MQCMIT", "USING", "HCONN", "CC", "REASON"]).unwrap();
    let HostRequest::MqMqi(request) = &effect.request else {
        panic!()
    };
    assert_eq!(
        request.envelope.request,
        MqMqiRequest::Commit {
            connection: token,
            unit: 31
        }
    );
    assert_eq!(effect.run_unit, child.invocation.run_unit_id);
    reply(
        &mut child,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::UnitOfWork { unit: 31 },
        },
    )
    .unwrap();
    drop(child);
    assert_eq!(scope.connection(1), Ok(token));
    assert!(parent.mqi.as_ref().unwrap().connections.is_empty());
    let back = call(&mut parent, &["MQBACK", "USING", "HCONN", "CC", "REASON"]).unwrap();
    reply(
        &mut parent,
        &back,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::UnitOfWork { unit: 31 },
        },
    )
    .unwrap();
}
#[test]
fn warning_reuse_on_another_machine_keeps_one_root_alias() {
    let scope = scope(3);
    let token = issued();
    for warning in [false, true] {
        let (mut machine, _) = bind(unbound_fixture(), scope.clone(), token);
        let effect = connect(&mut machine);
        let outcome = if warning {
            MqMqiOutcome::ReviewedOutput {
                status: mainframe_env_host_api::mq_status::MqReviewedStatus::from_wire_pair(
                    MqMqiCall::Connect,
                    1,
                    2002,
                )
                .unwrap(),
                output: MqMqiOutput::Connected(token),
            }
        } else {
            success(token)
        };
        reply(&mut machine, &effect, outcome).unwrap();
        assert_eq!(machine.read("HCONN").unwrap(), 1_i32.to_be_bytes());
        assert_eq!(
            machine.read("CC").unwrap(),
            (if warning { 1_i32 } else { 0 }).to_be_bytes()
        );
    }
    assert_eq!(scope.connection(1), Ok(token));
    assert_eq!(scope.connection(2), Err(HostProblem::Malformed));
}
#[test]
fn disconnect_revokes_alias_for_all_frames_but_leaves_undefined_zos_bytes() {
    let scope = scope(2);
    let token = issued();
    let (mut first, _) = bind(unbound_fixture(), scope.clone(), token);
    let effect = connect(&mut first);
    reply(&mut first, &effect, success(token)).unwrap();
    let before = first.read("HCONN").unwrap();
    let effect = call(&mut first, &["MQDISC", "USING", "HCONN", "CC", "REASON"]).unwrap();
    reply(
        &mut first,
        &effect,
        MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output: MqMqiOutput::NoOutput,
        },
    )
    .unwrap();
    assert_eq!(first.read("HCONN").unwrap(), before);
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
    let effect = connect(&mut first);
    let new = issued();
    reply(&mut first, &effect, success(new)).unwrap();
    assert_eq!(first.read("HCONN").unwrap(), 2_i32.to_be_bytes());
}
#[test]
fn swapped_equal_scope_and_late_storage_changes_never_partially_write_or_adopt() {
    for swap in [false, true] {
        let scope = scope(2);
        let token = issued();
        let (mut machine, frame) = bind(unbound_fixture(), scope.clone(), token);
        let effect = connect(&mut machine);
        if swap {
            *frame.scope.lock().unwrap() = Some(super::scope(2));
        } else {
            machine.write("REASON", &17_i32.to_be_bytes()).unwrap();
        }
        let before = machine.bases.clone();
        assert!(reply(&mut machine, &effect, success(token)).is_err());
        assert_eq!(machine.bases, before);
        assert_eq!(scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}
#[test]
fn unusable_sequence_and_unknown_reply_fence_shared_aliases_without_provider_cleanup() {
    for unknown in [false, true] {
        let scope = scope(2);
        let token = issued();
        let (mut machine, _) = bind(unbound_fixture(), scope.clone(), token);
        let effect = connect(&mut machine);
        let before = machine.bases.clone();
        let result = EffectResult {
            sequence: effect.sequence + u64::from(!unknown),
            outcome: if unknown {
                Err(HostProblem::UnknownOutcome)
            } else {
                Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
                    limits: Default::default(),
                    result: mainframe_env_host_api::mq_mqi::MqMqiResult {
                        call: MqMqiCall::Connect,
                        outcome: success(token),
                    },
                })))
            },
        };
        assert!(machine.resume_host(result).is_err());
        assert_eq!(machine.bases, before);
        assert_eq!(scope.connection(1), Err(HostProblem::UnknownOutcome));
    }
}
#[test]
fn compiled_conn_and_connx_use_reserved_aliases_and_preserve_original_call_identity() {
    for (text, call) in [
        (include_str!("../../tests/conn.mir"), MqMqiCall::Connect),
        (
            include_str!("../../tests/connx.mir"),
            MqMqiCall::ConnectExtended,
        ),
    ] {
        let scope = scope(2);
        let token = issued();
        let machine = super::super::super::tests::connx::compiled_text(text);
        let (mut machine, _) = bind(machine, scope.clone(), token);
        let original = machine.invocation.clone();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(100, 65536).unwrap())
        else {
            panic!("compiled call")
        };
        let HostRequest::MqMqi(request) = &effect.request else {
            panic!()
        };
        assert_eq!(request.envelope.request.call(), call);
        assert_eq!(request.envelope.context, context());
        assert_eq!(effect.run_unit, original.run_unit_id);
        assert_eq!(effect.deadline_tick, original.deadline_tick);
        assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
        reply(&mut machine, &effect, success(token)).unwrap();
        assert_eq!(machine.read("HCONN").unwrap(), 1_i32.to_be_bytes());
        assert_eq!(scope.connection(1), Ok(token));
        assert!(machine.checkpoint().is_none());
        assert_eq!(machine.snapshot().schema_version, 0);
    }
}

#[test]
fn dropped_dispatched_call_fences_root_but_unused_frame_drop_does_not() {
    let shared = scope(2);
    let token = issued();
    let (unused, _) = bind(unbound_fixture(), shared.clone(), token);
    drop(unused);
    shared.require_context(context()).unwrap();
    let (mut dispatched, _) = bind(unbound_fixture(), shared.clone(), token);
    let _effect = connect(&mut dispatched);
    drop(dispatched);
    assert_eq!(
        shared.require_context(context()),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(shared.connection(1), Err(HostProblem::UnknownOutcome));
}

#[test]
fn capacity_refusal_is_predispatch_and_known_failure_releases_only_reservation() {
    let shared = scope(1);
    let token = issued();
    let (mut first, _) = bind(unbound_fixture(), shared.clone(), token);
    let effect = connect(&mut first);
    let (mut second, _) = bind(unbound_fixture(), shared.clone(), token);
    let before = (second.bases.clone(), second.effect_sequence);
    assert_eq!(
        call(
            &mut second,
            &["MQCONN", "USING", "MANAGER", "HCONN", "CC", "REASON"]
        ),
        Err(MachineProblem::Host(HostProblem::ResourceExhausted))
    );
    assert_eq!((second.bases.clone(), second.effect_sequence), before);
    reply(
        &mut first,
        &effect,
        MqMqiOutcome::ReviewedStatus {
            status: mainframe_env_host_api::mq_status::MqReviewedStatus::from_wire_pair(
                MqMqiCall::Connect,
                2,
                2058,
            )
            .unwrap(),
        },
    )
    .unwrap();
    shared.require_context(context()).unwrap();
    assert_eq!(first.read("HCONN").unwrap(), 0_i32.to_be_bytes());
    let effect = connect(&mut second);
    reply(&mut second, &effect, success(token)).unwrap();
    assert_eq!(second.read("HCONN").unwrap(), 2_i32.to_be_bytes());
}
