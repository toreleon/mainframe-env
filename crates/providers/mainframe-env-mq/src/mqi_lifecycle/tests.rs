use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::{MqHandleProblem, MqHandleSharing, MqHconn};
use std::collections::BTreeSet;

pub(super) fn invocation(name: &str, context: &[u8]) -> Invocation {
    let limits = InvocationLimits::default();
    let mut bindings = BTreeMap::from([(
        "mq.host-context".into(),
        BoundedPayload::new("mainframe-env.mq.host-context@1", context.to_vec(), limits).unwrap(),
    )]);
    if context == b"zos-cics|host-coordinator" {
        bindings.insert(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                b"local".to_vec(),
                limits,
            )
            .unwrap(),
        );
    }
    Invocation::new(
        RequestId::new(format!("request-{name}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{name}"), limits).unwrap(),
        RunUnitId::new(format!("run-{name}"), limits).unwrap(),
        None,
        Selector::new("mq:test", limits).unwrap(),
        ArtifactRef::new("mq:test", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", limits).unwrap()]),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new(format!("trace-{name}"), limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{name}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        bindings,
        limits,
    )
    .unwrap()
}
pub(super) fn directory() -> MqLifecycleDirectory {
    MqLifecycleDirectory::new(Default::default()).unwrap()
}
pub(super) fn registry() -> MqHandleRegistry {
    MqHandleRegistry::new(1, 64).unwrap()
}
pub(super) fn child(parent: &Invocation, name: &str) -> Invocation {
    let mut child = invocation(name, b"zos-cics|host-coordinator");
    child.parent_execution_id = Some(parent.execution_id.clone());
    child.run_unit_id = parent.run_unit_id.clone();
    child.principal = parent.principal.clone();
    child.provider_generations = parent.provider_generations.clone();
    child.cancellation = parent.cancellation.clone();
    child.cancellation_probe = parent.cancellation_probe.clone();
    child.bindings = parent.bindings.clone();
    child
}

#[test]
fn host_selected_process_sharing_and_non_reused_unit_ids() {
    let mut directory = directory();
    let a = invocation("a", b"mqi-client|queue-manager");
    let b = invocation("b", b"mqi-client|queue-manager");
    let process = directory.mint_process(&a, 1).unwrap();
    let first = directory.bind_root(process, &a, 1).unwrap();
    assert_eq!(directory.bind_root(process, &a, 2), Ok(first));
    let second = directory.bind_root(process, &b, 1).unwrap();
    let ao = directory.owner_for(first, &a, 2).unwrap();
    let bo = directory.owner_for(second, &b, 2).unwrap();
    assert_eq!(ao.process_id, bo.process_id);
    assert_ne!(ao.thread_id, bo.thread_id);
    let mut registry = registry();
    let shared = registry.connect(ao, MqHandleSharing::SharedBlock).unwrap();
    let local = registry.connect(ao, MqHandleSharing::NonShared).unwrap();
    assert_eq!(registry.validate_connection(bo, shared), Ok(()));
    assert_eq!(
        registry.validate_connection(bo, local),
        Err(MqHandleProblem::CrossOwner)
    );
    let other_process = directory.mint_process(&b, 1).unwrap();
    assert!(directory.bind_root(other_process, &b, 1).is_err());
    directory.retire_frame(first, &mut registry).unwrap();
    assert!(directory.owner_for(first, &a, 2).is_err());
    let replacement = directory.bind_root(process, &a, 2).unwrap();
    let replacement_owner = directory.owner_for(replacement, &a, 2).unwrap();
    assert_ne!(replacement_owner.task_id, ao.task_id);
    assert_ne!(replacement_owner.thread_id, ao.thread_id);
}

#[test]
fn invocation_and_process_substitution_fail_closed() {
    let mut directory = directory();
    let original = invocation("a", b"zos-batch|queue-manager");
    let process = directory.mint_process(&original, 1).unwrap();
    let frame = directory.bind_root(process, &original, 1).unwrap();
    for case in 0..8 {
        let mut other = original.clone();
        match case {
            0 => other.attempt += 1,
            1 => other.run_unit_id = RunUnitId::new("alien", Default::default()).unwrap(),
            2 => other.execution_id = ExecutionId::new("alien", Default::default()).unwrap(),
            3 => other.idempotency_key = IdempotencyKey::new("alien", Default::default()).unwrap(),
            4 => other.deadline_tick -= 1,
            5 => {
                other.principal = Principal::new(
                    PrincipalId::new("OTHER", Default::default()).unwrap(),
                    BTreeSet::new(),
                    Default::default(),
                )
                .unwrap()
            }
            6 => other.bindings = invocation("a", b"mqi-client|queue-manager").bindings,
            _ => other
                .provider_generations
                .insert(
                    CapabilityId::new("host.mq.write", Default::default()).unwrap(),
                    "different".into(),
                )
                .map(|_| ())
                .unwrap_or(()),
        }
        assert!(directory.owner_for(frame, &other, 2).is_err());
        assert_eq!(directory.frames.len(), 1);
    }
    let mut other_directory = super::MqLifecycleDirectory::new(Default::default()).unwrap();
    assert!(other_directory.bind_root(process, &original, 1).is_err());
    assert!(other_directory.owner_for(frame, &original, 1).is_err());
    let new_process = other_directory.mint_process(&original, 1).unwrap();
    let new_frame = other_directory
        .bind_root(new_process, &original, 1)
        .unwrap();
    assert_ne!(
        directory.owner_for(frame, &original, 1).unwrap().host_id,
        other_directory
            .owner_for(new_frame, &original, 1)
            .unwrap()
            .host_id
    );
    let mut duplicate = original.clone();
    duplicate.execution_id = ExecutionId::new("new-root-same-run", Default::default()).unwrap();
    assert!(directory.bind_root(process, &duplicate, 1).is_err());
}

#[test]
fn explicit_cics_frame_inheritance_preserves_one_task_until_last_frame_ends() {
    let mut directory = directory();
    let root = invocation("parent", b"zos-cics|host-coordinator");
    let child = child(&root, "child");
    let process = directory.mint_process(&root, 1).unwrap();
    let parent_lease = directory.bind_root(process, &root, 1).unwrap();
    assert!(directory.bind_root(process, &child, 1).is_err());
    let child_lease = directory.bind_cics_child(parent_lease, &child, 1).unwrap();
    assert_eq!(
        directory.bind_cics_child(parent_lease, &child, 2),
        Ok(child_lease)
    );
    let owner = directory.owner_for(parent_lease, &root, 1).unwrap();
    assert_eq!(directory.owner_for(child_lease, &child, 1), Ok(owner));
    let mut registry = registry();
    let connection = registry.bind_cics_default(owner).unwrap();
    let object = registry.create_object(owner, connection).unwrap();
    directory.retire_frame(child_lease, &mut registry).unwrap();
    assert_eq!(registry.validate_connection(owner, connection), Ok(()));
    directory.retire_frame(parent_lease, &mut registry).unwrap();
    assert!(registry.validate_connection(owner, connection).is_err());
    assert!(!registry.is_live(object.into()));
    assert!(directory.bind_cics_child(parent_lease, &child, 2).is_err());
    assert_eq!(directory.retained_bytes, 0);
}

#[test]
fn cics_child_does_not_attest_a_foreign_or_widened_frame() {
    let mut directory = directory();
    let root = invocation("parent", b"zos-cics|host-coordinator");
    let process = directory.mint_process(&root, 1).unwrap();
    let parent = directory.bind_root(process, &root, 1).unwrap();
    let before = directory.retained_bytes;
    for case in 0..9 {
        let mut candidate = child(&root, "child");
        match case {
            0 => candidate.parent_execution_id = None,
            1 => {
                candidate.parent_execution_id =
                    Some(ExecutionId::new("alien", Default::default()).unwrap())
            }
            2 => candidate.run_unit_id = RunUnitId::new("alien", Default::default()).unwrap(),
            3 => candidate.deadline_tick += 1,
            4 => candidate.limits.max_effects += 1,
            5 => candidate.cancellation_probe = Some(CancellationProbe::new()),
            6 => {
                candidate.bindings.remove("cics.execution-context");
            }
            7 => {
                candidate.bindings.insert(
                    "cics.session".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.session@1",
                        b"alien".to_vec(),
                        Default::default(),
                    )
                    .unwrap(),
                );
            }
            _ => {
                candidate.provider_generations.insert(
                    CapabilityId::new("host.mq.write", Default::default()).unwrap(),
                    "different".into(),
                );
            }
        }
        assert!(directory.bind_cics_child(parent, &candidate, 1).is_err());
        assert_eq!(directory.frames.len(), 1);
        assert_eq!(directory.retained_bytes, before);
    }
    let ims = invocation("ims", b"zos-ims|host-coordinator");
    let process = directory.mint_process(&ims, 1).unwrap();
    let parent = directory.bind_root(process, &ims, 1).unwrap();
    assert!(
        directory
            .bind_cics_child(parent, &child(&ims, "child"), 1)
            .is_err()
    );
}

#[test]
fn live_controls_and_strict_context_precede_lifecycle_allocation() {
    let mut directory = directory();
    let mut original = invocation("a", b"zos-batch|queue-manager");
    for case in 0..6 {
        let mut candidate = original.clone();
        match case {
            0 => candidate.deadline_tick = u64::MAX,
            1 => candidate.deadline_tick = 0,
            2 => candidate.attempt = 0,
            3 => candidate.bindings.clear(),
            4 => {
                candidate.bindings.insert(
                    "mq.host-context".into(),
                    BoundedPayload::new(
                        "wrong",
                        b"zos-batch|queue-manager".to_vec(),
                        Default::default(),
                    )
                    .unwrap(),
                );
            }
            _ => candidate.limits.max_events = 0,
        }
        assert!(directory.mint_process(&candidate, 1).is_err());
        assert!(directory.processes.is_empty());
        assert_eq!(directory.next_process, 1);
    }
    assert_eq!(
        directory.mint_process(&original, 100),
        Err(HostProblem::TimedOut)
    );
    let probe = CancellationProbe::new();
    original.cancellation_probe = Some(probe.clone());
    let process = directory.mint_process(&original, 1).unwrap();
    let frame = directory.bind_root(process, &original, 1).unwrap();
    probe.request();
    assert_eq!(
        directory.owner_for(frame, &original, 2),
        Err(HostProblem::Cancelled)
    );
    assert_eq!(
        directory.bind_root(process, &original, 2),
        Err(HostProblem::Cancelled)
    );
    assert_eq!(directory.frames.len(), 1);
}

#[test]
fn finite_directory_budgets_and_counter_exhaustion_do_not_allocate() {
    let invocation = invocation("a", b"zos-batch|queue-manager");
    let mut directory = MqLifecycleDirectory::new(LifecycleLimits {
        processes: 1,
        frames: 1,
        retained_bytes: MAX_RETAINED_BYTES,
    })
    .unwrap();
    let process = directory.mint_process(&invocation, 1).unwrap();
    assert_eq!(
        directory.mint_process(&invocation, 1),
        Err(HostProblem::ResourceExhausted)
    );
    let frame = directory.bind_root(process, &invocation, 1).unwrap();
    let b = super::tests::invocation("b", b"zos-batch|queue-manager");
    assert_eq!(
        directory.bind_root(process, &b, 1),
        Err(HostProblem::ResourceExhausted)
    );
    let mut registry = registry();
    directory.retire_frame(frame, &mut registry).unwrap();
    directory.next_frame = u64::MAX;
    assert_eq!(
        directory.bind_root(process, &invocation, 1),
        Err(HostProblem::ResourceExhausted)
    );
    assert!(directory.frames.is_empty());
    assert_eq!(directory.retained_bytes, 0);
    directory.retire_process(process, &mut registry).unwrap();
    directory.next_process = u64::MAX;
    assert_eq!(
        directory.mint_process(&invocation, 1),
        Err(HostProblem::ResourceExhausted)
    );
    assert!(directory.processes.is_empty());
    let mut tiny = MqLifecycleDirectory::new(LifecycleLimits {
        retained_bytes: 1,
        ..Default::default()
    })
    .unwrap();
    let process = tiny.mint_process(&invocation, 1).unwrap();
    assert_eq!(
        tiny.bind_root(process, &invocation, 1),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(tiny.next_frame, 1);
    let mut huge = invocation.clone();
    huge.bindings.insert(
        "huge".into(),
        BoundedPayload::new("payload", vec![0; MAX_IDENTITY_BYTES], Default::default()).unwrap(),
    );
    assert_eq!(
        tiny.mint_process(&huge, 1),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn ims_syncpoint_retires_only_old_nonshared_and_unassociated_lifetimes() {
    let mut directory = directory();
    let invocation = invocation("ims", b"zos-ims|host-coordinator");
    let process = directory.mint_process(&invocation, 1).unwrap();
    let frame = directory.bind_root(process, &invocation, 1).unwrap();
    let old = directory.owner_for(frame, &invocation, 1).unwrap();
    let mut registry = registry();
    let nonshared = registry.connect(old, MqHandleSharing::NonShared).unwrap();
    let shared = registry.connect(old, MqHandleSharing::SharedBlock).unwrap();
    let unassociated = registry.create_message(old, MqHconn::Unassociated).unwrap();
    let next = directory
        .advance_ims_syncpoint(frame, &mut registry)
        .unwrap();
    assert_eq!(next.syncpoint_epoch, old.syncpoint_epoch + 1);
    assert_eq!(directory.owner_for(frame, &invocation, 2), Ok(next));
    assert!(registry.validate_connection(next, nonshared).is_err());
    assert_eq!(registry.validate_connection(next, shared), Ok(()));
    assert!(!registry.is_live(unassociated.into()));
    directory
        .frames
        .get_mut(&frame.frame)
        .unwrap()
        .owner
        .syncpoint_epoch = u64::MAX;
    let before = registry.active_handles();
    assert_eq!(
        directory.advance_ims_syncpoint(frame, &mut registry),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(registry.active_handles(), before);
}

#[test]
fn process_retirement_is_separate_from_frames_and_cannot_cross_processes() {
    let mut directory = directory();
    let a = invocation("a", b"mqi-client|queue-manager");
    let b = invocation("b", b"mqi-client|queue-manager");
    let ap = directory.mint_process(&a, 1).unwrap();
    let bp = directory.mint_process(&b, 1).unwrap();
    let af = directory.bind_root(ap, &a, 1).unwrap();
    let bf = directory.bind_root(bp, &b, 1).unwrap();
    let ao = directory.owner_for(af, &a, 1).unwrap();
    let bo = directory.owner_for(bf, &b, 1).unwrap();
    let mut registry = registry();
    let shared = registry.connect(ao, MqHandleSharing::SharedBlock).unwrap();
    let object = registry.create_object(ao, shared).unwrap();
    let subscription = registry.create_subscription(ao, shared).unwrap();
    let message = registry.create_message(ao, shared).unwrap();
    registry.begin_message_io(ao, shared, message).unwrap();
    let foreign = registry.connect(bo, MqHandleSharing::SharedBlock).unwrap();
    assert_eq!(
        directory.retire_process(ap, &mut registry),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(registry.validate_connection(ao, shared), Ok(()));
    directory.retire_frame(af, &mut registry).unwrap();
    assert_eq!(registry.validate_connection(ao, shared), Ok(()));
    directory.retire_process(ap, &mut registry).unwrap();
    assert!(registry.validate_connection(ao, shared).is_err());
    for handle in [object.into(), subscription.into(), message.into()] {
        assert!(!registry.is_live(handle));
    }
    assert_eq!(registry.validate_connection(bo, foreign), Ok(()));
    assert!(directory.bind_root(ap, &a, 1).is_err());
    assert_eq!(registry.active_handles(), 1);
    directory.retire_frame(bf, &mut registry).unwrap();
    directory.retire_process(bp, &mut registry).unwrap();
    assert_eq!(registry.active_handles(), 0);
}

#[test]
fn registry_process_retirement_checks_environment_host_and_invalid_owner_first() {
    let mut registry = registry();
    let owner = MqHandleOwner {
        environment: MqHostEnvironment::MqiClient,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    };
    let live = registry
        .connect(owner, MqHandleSharing::SharedBlock)
        .unwrap();
    let other_host = MqHandleOwner {
        host_id: 2,
        ..owner
    };
    let foreign = registry
        .connect(other_host, MqHandleSharing::SharedBlock)
        .unwrap();
    let other_environment = MqHandleOwner {
        environment: MqHostEnvironment::ZosCics,
        ..owner
    };
    let default = registry.bind_cics_default(other_environment).unwrap();
    let message = registry
        .create_message(owner, MqHconn::Unassociated)
        .unwrap();
    assert_eq!(
        registry.end_process(MqHandleOwner {
            process_id: 0,
            ..owner
        }),
        Err(MqHandleProblem::InvalidOwner)
    );
    assert_eq!(registry.active_handles(), 4);
    registry.end_process(owner).unwrap();
    assert!(!registry.is_live(message.into()));
    assert!(registry.validate_connection(owner, live).is_err());
    assert_eq!(registry.validate_connection(other_host, foreign), Ok(()));
    assert_eq!(
        registry.validate_connection(other_environment, default),
        Ok(())
    );
    registry.end_process(other_environment).unwrap();
    assert!(
        registry
            .validate_connection(other_environment, default)
            .is_err()
    );
    assert_eq!(registry.active_handles(), 1);
}

impl MqLifecycleDirectory {
    /// Host-selected process topology only: never infer sharing from principal,
    /// a string hash, an application owner assertion, or equal binding bytes.
    pub(crate) fn mint_process(
        &mut self,
        admitted: &Invocation,
        now: u64,
    ) -> Result<ProcessLease, HostProblem> {
        self.mint_process_in_mode(admitted, now, ContextMode::Binding)
    }

    pub(crate) fn bind_root(
        &mut self,
        process: ProcessLease,
        admitted: &Invocation,
        now: u64,
    ) -> Result<FrameLease, HostProblem> {
        self.bind_root_in_mode(process, admitted, now, ContextMode::Binding)
    }
}
