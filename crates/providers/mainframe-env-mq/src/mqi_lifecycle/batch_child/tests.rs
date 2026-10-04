use super::*;
use crate::mqi_lifecycle::tests::{child, directory, invocation, registry};
use mainframe_env_execution_api::CancellationProbe;
use mainframe_env_host_api::MqHandleSharing;

fn parent() -> Invocation {
    invocation("batch-parent", b"zos-batch|queue-manager")
        .with_cancellation_probe(CancellationProbe::new())
}
fn bind(d: &mut MqLifecycleDirectory, p: &Invocation) -> FrameLease {
    let process = d.mint_process(p, 1).unwrap();
    d.bind_root(process, p, 1).unwrap()
}
fn prepare(
    d: &mut MqLifecycleDirectory,
    frame: FrameLease,
    p: &Invocation,
    c: &Invocation,
) -> Result<BatchChildBinding, HostProblem> {
    d.bind_installed_batch_child(frame, p, c, InstalledBatchRelationship::SameTaskCall, 2)
}

#[test]
fn same_task_origin_survives_frame_return_without_registry_retirement() {
    let mut d = directory();
    let p = parent();
    let root = bind(&mut d, &p);
    let c = child(&p, "batch-child");
    let binding = prepare(&mut d, root, &p, &c).unwrap();
    let owner = d.owner_for(root, &p, 2).unwrap();
    let proof = d.logical_batch_owner(binding.frame(), &c, 2).unwrap();
    assert_eq!(proof.owner(), owner);
    assert_eq!(proof.execution(), p.execution_id.as_str());
    assert!(proof.is_child());
    let mut r = registry();
    let conn = r.connect(owner, MqHandleSharing::NonShared).unwrap();
    // Removing the root reference cannot erase the still-live child's origin.
    d.retire_frame(root, &mut r).unwrap();
    assert_eq!(r.validate_connection(proof.owner(), conn), Ok(()));
    assert_eq!(
        d.logical_batch_owner(binding.frame(), &c, 3)
            .unwrap()
            .execution(),
        proof.execution()
    );
    assert_eq!(
        d.return_batch_child(binding.frame(), &c),
        Err(HostProblem::Unsupported)
    );
    let grandchild = child(&c, "batch-grandchild");
    let grand = prepare(&mut d, binding.frame(), &c, &grandchild).unwrap();
    d.return_batch_child(binding.frame(), &c).unwrap();
    assert_eq!(
        r.validate_connection(d.owner_for(grand.frame(), &grandchild, 3).unwrap(), conn),
        Ok(())
    );
    assert_eq!(
        d.logical_batch_owner(grand.frame(), &grandchild, 3)
            .unwrap()
            .execution(),
        p.execution_id.as_str()
    );
    assert!(d.owner_for(binding.frame(), &c, 3).is_err());
}

#[test]
fn forged_child_and_parent_claims_never_allocate_or_change_counters() {
    let mut d = directory();
    let p = parent();
    let root = bind(&mut d, &p);
    let c = child(&p, "batch-child");
    for case in 0..12 {
        let mut changed = c.clone();
        match case {
            0 => changed.parent_execution_id = None,
            1 => changed.execution_id = p.execution_id.clone(),
            2 => changed.run_unit_id = RunUnitId::new("foreign", Default::default()).unwrap(),
            3 => {
                changed.principal = mainframe_env_execution_api::Principal::new(
                    p.principal.id().clone(),
                    Default::default(),
                    Default::default(),
                )
                .unwrap()
            }
            4 => changed.attempt += 1,
            5 => changed.cancellation_probe = Some(CancellationProbe::new()),
            6 => changed.cancellation_probe = None,
            7 => changed.deadline_tick += 1,
            8 => changed.limits.max_frames += 1,
            9 => changed.bindings = invocation("ims", b"zos-ims|host-coordinator").bindings,
            10 => {
                changed.provider_generations.insert(
                    mainframe_env_execution_api::CapabilityId::new(
                        "host.mq.write",
                        Default::default(),
                    )
                    .unwrap(),
                    "changed".into(),
                );
            }
            _ => changed.deadline_tick = 2,
        }
        let before = (d.frames.len(), d.next_frame, d.retained_bytes);
        assert!(prepare(&mut d, root, &p, &changed).is_err(), "case {case}");
        assert_eq!((d.frames.len(), d.next_frame, d.retained_bytes), before);
    }
    let mut altered_parent = p.clone();
    altered_parent.idempotency_key =
        mainframe_env_execution_api::IdempotencyKey::new("forged", Default::default()).unwrap();
    assert!(prepare(&mut d, root, &altered_parent, &c).is_err());
    assert_eq!(
        d.bind_installed_batch_child(root, &p, &c, InstalledBatchRelationship::SeparateSubtask, 2),
        Err(HostProblem::Unsupported)
    );
    let mut foreign = directory();
    assert!(prepare(&mut foreign, root, &p, &c).is_err());
    assert!(d.mint_process(&c, 2).is_err());
    let process = ProcessLease {
        directory: d.identity,
        process: root.process,
    };
    assert!(d.bind_root(process, &c, 2).is_err());
    p.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(prepare(&mut d, root, &p, &c), Err(HostProblem::Cancelled));
}

#[test]
fn bounded_preparation_abort_only_new_child_and_never_reuses_frame_ids() {
    let mut d = directory();
    let p = parent();
    let root = bind(&mut d, &p);
    let c = child(&p, "batch-child");
    let before = d.retained_bytes;
    let b = prepare(&mut d, root, &p, &c).unwrap();
    let repeat = prepare(&mut d, root, &p, &c).unwrap();
    d.abort_batch_child(repeat).unwrap();
    assert!(d.owner_for(b.frame(), &c, 2).is_ok());
    d.abort_batch_child(b).unwrap();
    assert_eq!(d.retained_bytes, before);
    assert!(d.owner_for(root, &p, 2).is_ok());
    let replacement = prepare(&mut d, root, &p, &c).unwrap();
    assert_ne!(replacement.frame(), b.frame());
    d.return_batch_child(replacement.frame(), &c).unwrap();
    for limit in [0, 1, 2] {
        let before = (d.frames.len(), d.next_frame, d.retained_bytes);
        match limit {
            0 => d.limits.frames = 1,
            1 => {
                d.limits.frames = 4096;
                d.limits.retained_bytes = d.retained_bytes;
            }
            _ => {
                d.limits.retained_bytes = MAX_RETAINED_BYTES;
                d.next_frame = u64::MAX;
            }
        }
        let before = (before.0, d.next_frame, before.2);
        assert_eq!(
            prepare(&mut d, root, &p, &c),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!((d.frames.len(), d.next_frame, d.retained_bytes), before);
    }
}
