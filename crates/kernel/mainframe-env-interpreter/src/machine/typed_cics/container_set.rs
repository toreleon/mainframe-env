use super::*;

const MARKER: &[u8] = b"MEC-CICS-CONTAINER-SET\0";

pub(super) fn prepare(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    arguments: &BTreeMap<String, BoundedPayload>,
    set_requested: bool,
) -> Option<(String, Option<String>)> {
    if !matches!(
        operation,
        CicsOperation::GetContainer
            | CicsOperation::DeleteContainer
            | CicsOperation::MoveContainer
            | CicsOperation::DeleteChannel
    ) {
        return None;
    }
    let name = |key| {
        arguments.get(key).and_then(|value| {
            std::str::from_utf8(value.bytes())
                .ok()
                .map(|name| name.trim_end_matches(' ').to_owned())
        })
    };
    let channel = name("CHANNEL").or_else(|| {
        machine
            .invocation
            .bindings
            .get("cics.channel")
            .and_then(|value| std::str::from_utf8(value.bytes()).ok())
            .map(|name| name.trim_end_matches(' ').to_owned())
    })?;
    let identity = (channel, name("CONTAINER"));
    on_issue(machine, operation, set_requested, Some(&identity));
    Some(identity)
}

fn marker(channel: &str, container: &str) -> Vec<u8> {
    let mut bytes = MARKER.to_vec();
    bytes.extend_from_slice(channel.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(container.as_bytes());
    bytes
}

pub(in crate::machine) fn record(machine: &mut ReferenceMachine, channel: &str, container: &str) {
    let base = machine.bases.len();
    machine.bases.push(marker(channel, container));
    machine.freed_allocations.insert(base);
}

pub(in crate::machine) fn release_container(
    machine: &mut ReferenceMachine,
    channel: &str,
    container: &str,
) {
    release(machine, |saved| saved == marker(channel, container));
}

pub(in crate::machine) fn release_channel(machine: &mut ReferenceMachine, channel: &str) {
    let mut prefix = MARKER.to_vec();
    prefix.extend_from_slice(channel.as_bytes());
    prefix.push(0);
    release(machine, |saved| saved.starts_with(&prefix));
}

pub(in crate::machine) fn release_all(machine: &mut ReferenceMachine) {
    release(machine, |saved| saved.starts_with(MARKER));
}

pub(super) fn on_issue(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    set_requested: bool,
    identity: Option<&(String, Option<String>)>,
) {
    if operation == CicsOperation::GetContainer
        && set_requested
        && let Some((channel, Some(container))) = identity
    {
        release_container(machine, channel, container);
    }
}

pub(in crate::machine) fn on_success(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    identity: Option<&(String, Option<String>)>,
) {
    match (operation, identity) {
        (
            CicsOperation::DeleteContainer | CicsOperation::MoveContainer,
            Some((channel, Some(container))),
        ) => {
            release_container(machine, channel, container);
        }
        (CicsOperation::DeleteChannel, Some((channel, _))) => release_channel(machine, channel),
        _ => {}
    }
}

fn release(machine: &mut ReferenceMachine, selected: impl Fn(&[u8]) -> bool) {
    let releases = (machine.static_base_count + 1..machine.bases.len())
        .filter(|marker_base| {
            machine.freed_allocations.contains(marker_base)
                && !machine.freed_allocations.contains(&(marker_base - 1))
                && selected(&machine.bases[*marker_base])
        })
        .map(|marker_base| marker_base - 1)
        .collect::<Vec<_>>();
    machine.freed_allocations.extend(releases);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_ir::{
        CicsNamedOperand, CicsOutputBinding, IrLimits, ModuleBuilder, StorageReference,
        encode_cics_effect_plan,
    };

    fn typed_get_machine(containers: &[&str]) -> ReferenceMachine {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let fields = [
            ("PTR-A", "pointer", "", 4),
            ("PTR-B", "pointer", "", 4),
            ("LEN-X", "binary", "S9(9)", 4),
        ];
        let slots = fields
            .iter()
            .map(|(name, _, _, length)| builder.add_storage(*name, *length, None).unwrap())
            .collect::<Vec<_>>();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        for ((name, category, picture, length), storage) in fields.iter().zip(&slots) {
            builder
                .add_operation(
                    block,
                    OperationIdentity::new(super::super::super::NAMESPACE, "define", 1).unwrap(),
                    Vec::new(),
                    0,
                    BTreeMap::from([
                        ("name".into(), Attribute::Text((*name).into())),
                        ("simple_name".into(), Attribute::Text((*name).into())),
                        ("category".into(), Attribute::Text((*category).into())),
                        ("picture".into(), Attribute::Text((*picture).into())),
                        (
                            "digits".into(),
                            Attribute::Integer(if *category == "binary" { 9 } else { 0 }),
                        ),
                        ("scale".into(), Attribute::Integer(0)),
                        (
                            "signed".into(),
                            Attribute::Integer(if *category == "binary" { 1 } else { 0 }),
                        ),
                        ("sign_separate".into(), Attribute::Integer(0)),
                        ("section".into(), Attribute::Text("working".into())),
                        ("offset".into(), Attribute::Integer(0)),
                        ("length".into(), Attribute::Integer(*length as i64)),
                        ("element_length".into(), Attribute::Integer(*length as i64)),
                        ("occurs".into(), Attribute::Integer(1)),
                        ("parent".into(), Attribute::Text(String::new())),
                        ("condition_values".into(), Attribute::Text(String::new())),
                    ]),
                    Vec::new(),
                    vec![StorageReference {
                        storage: *storage,
                        offset: 0,
                        length: *length,
                    }],
                    None,
                )
                .unwrap();
        }
        let descriptor = cics_executable_descriptor(CicsPlanOperation::GetContainer);
        for (index, container) in containers.iter().enumerate() {
            let plan = CicsEffectPlan {
                operation: CicsPlanOperation::GetContainer,
                operands: vec![
                    CicsNamedOperand {
                        name: CicsOperandName::ContainerName,
                        value: CicsOperandValue::Literal(container.as_bytes().to_vec()),
                    },
                    CicsNamedOperand {
                        name: CicsOperandName::BtsChannel,
                        value: CicsOperandValue::Literal(b"WORK".to_vec()),
                    },
                ],
                options: BTreeSet::new(),
                outputs: vec![
                    CicsOutputBinding {
                        name: CicsOutputName::ContainerSet,
                        target: CicsStorageSlot {
                            storage: slots[index],
                            qualified_layout_name: fields[index].0.into(),
                        },
                    },
                    CicsOutputBinding {
                        name: CicsOutputName::ContainerLength,
                        target: CicsStorageSlot {
                            storage: slots[2],
                            qualified_layout_name: "LEN-X".into(),
                        },
                    },
                ],
                condition: CicsCondition::Default,
            };
            builder
                .add_operation(
                    block,
                    descriptor.identity(),
                    Vec::new(),
                    0,
                    BTreeMap::from([(
                        PLAN_ATTRIBUTE.into(),
                        Attribute::Bytes(
                            encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                        ),
                    )]),
                    descriptor.effects.to_vec(),
                    vec![
                        StorageReference {
                            storage: slots[index],
                            offset: 0,
                            length: 4,
                        },
                        StorageReference {
                            storage: slots[2],
                            offset: 0,
                            length: 4,
                        },
                    ],
                    None,
                )
                .unwrap();
        }
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        ReferenceMachine::from_binary(
            &bytes,
            super::super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap()
    }

    fn resume_get(
        machine: &mut ReferenceMachine,
        resume: MachineResume<EffectResult>,
        data: &[u8],
    ) {
        let MachineDrive::HostCall(effect) =
            machine.drive(resume, Quantum::new(100, 1024).unwrap())
        else {
            panic!("typed GET CONTAINER must issue a host effect");
        };
        let HostRequest::Cics(request) = &effect.request else {
            panic!("expected CICS request")
        };
        assert_eq!(request.operation, CicsOperation::GetContainer);
        let response = CicsResponse {
            disposition: CicsDisposition::Complete,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            applid: "MEAPPL".into(),
            sysid: "MESYS".into(),
            transaction: "TEST".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: payload("mainframe-env.cics.payload@1", Vec::new()).unwrap(),
            outputs: BTreeMap::from([
                (
                    "SET".into(),
                    payload("mainframe-env.cics.payload@1", data.to_vec()).unwrap(),
                ),
                (
                    "FLENGTH".into(),
                    payload(
                        "mainframe-env.cics.decimal@1",
                        data.len().to_string().into_bytes(),
                    )
                    .unwrap(),
                ),
            ]),
            unit_of_work: None,
        };
        machine
            .resume_host(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            })
            .unwrap();
    }

    fn pointer_base(machine: &ReferenceMachine, name: &str) -> usize {
        let address = machine.read(name).unwrap();
        machine.decode_address(&address).unwrap().unwrap().0
    }

    #[test]
    fn typed_get_container_set_twice_releases_first_area() {
        let mut machine = typed_get_machine(&["ONE", "ONE"]);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let first = machine.read("PTR-A").unwrap();
        let first_base = pointer_base(&machine, "PTR-A");
        resume_get(&mut machine, MachineResume::Start, b"");
        assert!(matches!(
            retrieve::release_pointer(
                &mut machine,
                &payload("mainframe-env.cics.allocated-pointer@1", first).unwrap()
            ),
            Err(MachineProblem::UnexpectedHostResult)
        ));
        let second_base = pointer_base(&machine, "PTR-B");
        assert!(machine.freed_allocations.contains(&first_base));
        assert!(!machine.freed_allocations.contains(&second_base));
    }

    #[test]
    fn typed_get_container_set_survives_other_container_and_expires_at_program_completion() {
        let mut machine = typed_get_machine(&["ONE", "TWO"]);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let first_base = pointer_base(&machine, "PTR-A");
        resume_get(&mut machine, MachineResume::Start, b"second");
        let second_base = pointer_base(&machine, "PTR-B");
        assert!(!machine.freed_allocations.contains(&first_base));
        assert!(!machine.freed_allocations.contains(&second_base));
        assert!(matches!(
            machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
            MachineDrive::Completed(_)
        ));
        assert!(machine.freed_allocations.contains(&first_base));
        assert!(machine.freed_allocations.contains(&second_base));
    }

    fn saved(machine: &mut ReferenceMachine, channel: &str, container: &str) -> usize {
        let base = machine.bases.len();
        machine.bases.push(b"value".to_vec());
        record(machine, channel, container);
        base
    }

    #[test]
    fn channel_get_container_set_expires_after_next_get_of_same_container() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        on_issue(
            &mut machine,
            CicsOperation::GetContainer,
            true,
            Some(&("WORK".into(), Some("ONE".into()))),
        );
        assert!(machine.freed_allocations.contains(&base));
    }

    #[test]
    fn channel_get_container_set_survives_get_of_a_different_container() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        let other = saved(&mut machine, "WORK", "TWO");
        on_issue(
            &mut machine,
            CicsOperation::GetContainer,
            true,
            Some(&("WORK".into(), Some("TWO".into()))),
        );
        assert!(!machine.freed_allocations.contains(&base));
        assert!(machine.freed_allocations.contains(&other));
    }

    #[test]
    fn channel_get_container_set_expires_after_delete_container() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        on_success(
            &mut machine,
            CicsOperation::DeleteContainer,
            Some(&("WORK".into(), Some("ONE".into()))),
        );
        assert!(machine.freed_allocations.contains(&base));
    }

    #[test]
    fn channel_get_container_set_expires_after_move_container() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        on_success(
            &mut machine,
            CicsOperation::MoveContainer,
            Some(&("WORK".into(), Some("ONE".into()))),
        );
        assert!(machine.freed_allocations.contains(&base));
    }

    #[test]
    fn channel_get_container_set_expires_after_delete_channel() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        on_success(
            &mut machine,
            CicsOperation::DeleteChannel,
            Some(&("WORK".into(), None)),
        );
        assert!(machine.freed_allocations.contains(&base));
    }

    #[test]
    fn expired_channel_set_stays_released_after_restore() {
        let (mut machine, _) = super::super::tests::machine_with_alphanumeric_slot("PTR", 4);
        let base = saved(&mut machine, "WORK", "ONE");
        let address = machine.address_bytes_for(base, 0, 4).unwrap();
        release_container(&mut machine, "WORK", "ONE");
        let snapshot = machine.snapshot();
        machine.restore(snapshot).unwrap();
        assert!(machine.freed_allocations.contains(&base));
        assert!(matches!(
            retrieve::release_pointer(
                &mut machine,
                &payload("mainframe-env.cics.allocated-pointer@1", address).unwrap()
            ),
            Err(MachineProblem::UnexpectedHostResult)
        ));
    }
}
