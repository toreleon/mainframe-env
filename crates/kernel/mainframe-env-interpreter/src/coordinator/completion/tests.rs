use crate::ReferenceMachine;
use mainframe_env_execution_api::*;
use mainframe_env_ir::{Attribute, CodecLimits, IrLimits, ModuleBuilder, OperationIdentity};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn invocation(scoped: bool) -> Invocation {
    let limits = InvocationLimits::default();
    let mut invocation = Invocation::new(
        RequestId::new("request", limits).unwrap(),
        ExecutionId::new("execution", limits).unwrap(),
        RunUnitId::new("run", limits).unwrap(),
        None,
        Selector::new("test", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("USER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new("key", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();

    invocation.attempt = 3;
    if scoped {
        invocation.bindings.insert(
            "cobol.storage-entry".into(),
            BoundedPayload::new(
                "mainframe-env.cobol.storage-entry@1",
                b"opaque opt-in".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
    }
    invocation
}

pub(super) fn machine(invocation: Invocation, terminal: &str, args: &[&str]) -> ReferenceMachine {
    machine_with_lifecycle(invocation, terminal, args, "retained@1")
}

fn machine_with_lifecycle(
    invocation: Invocation,
    terminal: &str,
    args: &[&str],
    lifecycle: &str,
) -> ReferenceMachine {
    machine_with_body(invocation, terminal, args, lifecycle, "continue", &[])
}

fn machine_with_body(
    invocation: Invocation,
    terminal: &str,
    args: &[&str],
    lifecycle: &str,
    body: &str,
    body_args: &[&str],
) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    for (name, attributes) in [
        (
            "config",
            BTreeMap::from([
                ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                (
                    "program_lifecycle".into(),
                    Attribute::Text(lifecycle.into()),
                ),
                ("entry_formals_v1".into(), Attribute::Text(String::new())),
            ]),
        ),
        (
            body,
            body_args
                .iter()
                .enumerate()
                .map(|(i, arg)| (format!("arg_{i:03}"), Attribute::Text((*arg).into())))
                .collect(),
        ),
        (
            terminal,
            args.iter()
                .enumerate()
                .map(|(i, arg)| (format!("arg_{i:03}"), Attribute::Text((*arg).into())))
                .collect(),
        ),
        ("halt", BTreeMap::new()),
    ] {
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", name, 1).unwrap(),
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }
    let bytes = mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
        .unwrap();
    ReferenceMachine::from_binary(&bytes, invocation, CodecLimits::default()).unwrap()
}

#[test]
fn scoped_live_native_returns_supply_exact_terminal_image() {
    for (terminal, args) in [("go_back", &[][..]), ("exit", &["PROGRAM"][..])] {
        let mut m = machine(invocation(true), terminal, args);
        assert!(m.completion_checkpoint().is_none());
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Completed(_)
        ));
        let witness = m.attest_installed_program_return().unwrap();
        assert_eq!(witness.program_counter(), 2);
        assert_eq!(witness.executed_steps(), 3);
        let terminal_image = m
            .completion_checkpoint()
            .expect("live scoped normal return must capture terminal checkpoint");
        assert_eq!(terminal_image, m.checkpoint().unwrap());
        assert_eq!(
            terminal_image.schema(),
            "mainframe-env.reference-machine-checkpoint@12"
        );
        assert_eq!(&terminal_image.bytes()[..8], b"MECP0012");
        assert_eq!(m.snapshot().program_counter, 2);
        assert_eq!(m.snapshot().executed_steps, 3);
        assert_eq!(m.effect_sequence(), 0);
    }
}

fn returned() -> ReferenceMachine {
    let mut m = machine(invocation(true), "go_back", &[]);
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
        MachineDrive::Completed(_)
    ));
    m
}

#[test]
fn unscoped_and_wrong_binding_schema_do_not_capture() {
    for mode in ["absent", "wrong-schema", "wrong-name"] {
        let mut inv = invocation(false);
        if mode != "absent" {
            inv.bindings.insert(
                if mode == "wrong-name" {
                    "foreign.storage-entry"
                } else {
                    "cobol.storage-entry"
                }
                .into(),
                BoundedPayload::new(
                    if mode == "wrong-schema" {
                        "mainframe-env.cobol.storage-entry@2"
                    } else {
                        "mainframe-env.cobol.storage-entry@1"
                    },
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
        }
        let mut m = machine(inv, "go_back", &[]);
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Completed(_)
        ));
        assert!(m.attest_installed_program_return().is_ok());
        assert!(m.completion_checkpoint().is_none(), "{mode}");
    }
}

#[test]
fn constructor_continue_and_cold_restore_cannot_capture() {
    let mut m = machine(invocation(true), "go_back", &[]);
    let constructor = m.checkpoint().unwrap();
    assert!(m.completion_checkpoint().is_none());
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(2, 4096).unwrap()),
        MachineDrive::Continue
    ));
    assert_eq!(m.snapshot().program_counter, 2);
    let before_return = m.checkpoint().unwrap();
    assert!(m.completion_checkpoint().is_none());
    let after_return = returned().completion_checkpoint().unwrap();
    for checkpoint in [constructor, before_return, after_return] {
        let mut cold = machine(invocation(true), "go_back", &[]);
        cold.restore_checkpoint(&checkpoint).unwrap();
        assert!(cold.completion_checkpoint().is_none());
    }
}

#[test]
fn stale_drive_and_non_native_completion_cannot_capture() {
    for resume in [MachineResume::Cancelled, MachineResume::TimedOut] {
        let mut m = returned();
        assert!(matches!(
            m.drive(resume, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Failed(_)
        ));
        assert!(m.completion_checkpoint().is_none());
    }
    for (terminal, args) in [
        ("halt", &[][..]),
        ("stop_run", &[][..]),
        ("exit", &["METHOD"][..]),
        ("exit", &["FUNCTION"][..]),
    ] {
        let mut m = machine(invocation(true), terminal, args);
        let _ = m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap());
        assert!(m.completion_checkpoint().is_none(), "{terminal} {args:?}");
    }
}

#[test]
fn unsupported_resources_and_lifecycle_fence_without_mutation() {
    for resource in ["open-file", "sql", "sort", "extra-base", "lifecycle"] {
        let mut m = if resource == "lifecycle" {
            machine_with_lifecycle(invocation(true), "go_back", &[], "unsupported@1")
        } else {
            machine(invocation(true), "go_back", &[])
        };
        let mut snapshot = m.snapshot();
        match resource {
            "open-file" => {
                snapshot
                    .dataset_cursors
                    .insert("DATA.SET".into(), "cursor".into());
            }
            "sql" => {
                snapshot.sql_cursors.insert("C".into(), Vec::new());
            }
            "sort" => {
                snapshot.sort_workspaces.insert("S".into(), (Vec::new(), 0));
            }
            "extra-base" => snapshot.base_storage.push(Vec::new()),
            _ => {}
        }
        m.restore(snapshot).unwrap();
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Completed(_)
        ));
        let before = m.checkpoint().unwrap();
        assert!(m.completion_checkpoint().is_none(), "{resource}");
        assert_eq!(m.checkpoint().unwrap(), before);
    }
}

#[test]
fn terminal_capture_is_pure_and_invalid_restore_preserves_it() {
    let mut m = returned();
    let image = m.completion_checkpoint().unwrap();
    let snapshot = m.snapshot();
    assert_eq!(m.completion_checkpoint().unwrap(), image);
    assert_eq!(m.snapshot(), snapshot);
    let mut invalid = snapshot.clone();
    invalid.program_counter = usize::MAX;
    assert!(m.restore(invalid).is_err());
    assert_eq!(m.snapshot(), snapshot);
    assert_eq!(m.completion_checkpoint().unwrap(), image);
    m.restore(snapshot).unwrap();
    assert!(m.completion_checkpoint().is_none());
}

#[test]
fn checkpoint_record_preserves_suspend_identity_and_zero_sequence() {
    let inv = invocation(true);
    let image = returned().completion_checkpoint().unwrap();
    let expected_bytes = image.bytes().to_vec();
    let record = super::checkpoint_record(&inv, 0, None, image);
    assert_eq!(record.execution_id, inv.execution_id);
    assert_eq!(record.run_unit_id, inv.run_unit_id);
    assert_eq!(record.artifact, inv.artifact);
    assert_eq!(record.principal, inv.principal.id().clone());
    assert_eq!(record.provider_generation, "mainframe-env-reference@1");
    assert_eq!(
        record.required_host_interfaces,
        BTreeMap::from([
            ("mainframe-env.execution-api".into(), "1".into()),
            ("mainframe-env.host-api".into(), "1".into())
        ])
    );
    assert_eq!(record.schema_version, 1);
    assert_eq!(record.machine_schema_version, 1);
    assert_eq!(record.effect_sequence, 0);
    assert_eq!(record.session_id, None);
    assert_eq!(record.transaction, None);
    assert_eq!(record.security_classification, "application-data");
    assert_eq!(record.encryption_key_reference, None);
    assert_eq!(record.payload_size, expected_bytes.len() as u64);
    assert_eq!(record.payload, expected_bytes);
    use sha2::Digest;
    assert_eq!(
        record.payload_digest,
        <[u8; 32]>::from(sha2::Sha256::digest(&expected_bytes))
    );
    let suspended = super::checkpoint_record(
        &inv,
        7,
        Some("session".into()),
        returned().checkpoint().unwrap(),
    );
    assert_eq!(suspended.effect_sequence, 7);
    assert_eq!(suspended.session_id.as_deref(), Some("session"));
}

#[test]
fn generic_machine_default_is_none_and_local_completion_is_unchanged() {
    struct DefaultMachine;
    impl Machine for DefaultMachine {
        type Effect = mainframe_env_host_api::EffectRequest;
        type EffectResult = mainframe_env_host_api::EffectResult;
        fn drive(
            &mut self,
            _: MachineResume<Self::EffectResult>,
            _: Quantum,
        ) -> MachineDrive<Self::Effect> {
            MachineDrive::Completed(Completion {
                return_code: 37,
                output: BoundedPayload::new("test@1", Vec::new(), InvocationLimits::default())
                    .unwrap(),
            })
        }
    }
    let mut m = DefaultMachine;
    assert!(m.completion_checkpoint().is_none());
    assert!(matches!(
        crate::ExecutionCoordinator::local(crate::CoordinatorLimits::default()).execute(
            &mut m,
            &invocation(false),
            crate::ExecutionControl {
                now_tick: 1,
                cancellation_requested: false
            }
        ),
        ExecutionOutcome::Completed(Completion {
            return_code: 37,
            ..
        })
    ));
}

#[test]
fn returned_cics_result_at_goback_pc_never_supplies_terminal_image() {
    use mainframe_env_host_api::{CicsDisposition, CicsResponse, EffectResult, HostResult};
    let mut m = machine_with_body(
        invocation(true),
        "go_back",
        &[],
        "retained@1",
        "exec_cics",
        &["RETURN"],
    );
    let MachineDrive::HostCall(effect) =
        m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap())
    else {
        panic!("actual CICS RETURN required")
    };
    assert_eq!(m.snapshot().program_counter, 2);
    assert!(m.completion_checkpoint().is_none());
    let response = CicsResponse {
        disposition: CicsDisposition::Returned,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "MEAPPL".into(),
        sysid: "MESYS".into(),
        transaction: "TEST".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        unit_of_work: None,
    };
    assert!(matches!(
        m.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response))
            }),
            Quantum::new(20, 4096).unwrap()
        ),
        MachineDrive::Completed(_)
    ));
    assert_eq!(m.snapshot().program_counter, 2);
    assert_eq!(m.snapshot().executed_steps, 2);
    assert!(m.completion_checkpoint().is_none());
}

#[test]
fn failed_drive_and_zero_quantum_clear_prior_terminal_capture() {
    let mut inv = invocation(true);
    inv.limits.max_steps = 3;
    let mut m = machine(inv, "go_back", &[]);
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
        MachineDrive::Completed(_)
    ));
    assert!(m.completion_checkpoint().is_some());
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
        MachineDrive::Failed(_)
    ));
    assert!(m.completion_checkpoint().is_none());
    let mut m = returned();
    assert!(matches!(
        m.drive(
            MachineResume::Start,
            Quantum {
                max_steps: 0,
                max_allocated_bytes: 4096
            }
        ),
        MachineDrive::Continue
    ));
    assert!(m.completion_checkpoint().is_none());
}
