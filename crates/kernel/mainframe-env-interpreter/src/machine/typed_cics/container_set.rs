use super::*;

// Private ownership labels in existing freed bases, not a new snapshot field.
// Historical unmarked BTS areas cannot be identified safely after restore.
const BTS_MARKER: &[u8] = b"MEC-CICS-BTS-SET\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::machine) enum ContainerIdentity {
    Channel(String, Option<String>),
    BtsSet {
        previous: Option<usize>,
        capacity: usize,
    },
}

const MARKER: &[u8] = b"MEC-CICS-CONTAINER-SET\0";

pub(super) fn prepare(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    arguments: &mut BTreeMap<String, BoundedPayload>,
    set_requested: bool,
) -> Result<Option<ContainerIdentity>, MachineProblem> {
    if !matches!(
        operation,
        CicsOperation::GetContainer
            | CicsOperation::DeleteContainer
            | CicsOperation::MoveContainer
            | CicsOperation::DeleteChannel
    ) {
        return Ok(None);
    }
    // Explicit selectors route to BTS in the actual provider even when a
    // current channel exists. Omitted-selector/channel ambiguity is not owned.
    if operation == CicsOperation::GetContainer
        && [
            "ACTIVITY",
            "OPTION.PROCESS",
            "OPTION.ACQPROCESS",
            "OPTION.ACQACTIVITY",
        ]
        .iter()
        .any(|key| arguments.contains_key(*key))
    {
        if !set_requested {
            return Ok(None);
        }
        let previous = active_bts_base(machine);
        let capacity = bts_capacity(machine, previous)?;
        let advertised = arguments
            .get("SET.MAXLENGTH")
            .and_then(|value| std::str::from_utf8(value.bytes()).ok())
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let capacity = capacity.min(advertised);
        arguments.insert(
            "SET.MAXLENGTH".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                capacity.to_string().into_bytes(),
            )?,
        );
        return Ok(Some(ContainerIdentity::BtsSet { previous, capacity }));
    }
    let name = |key| {
        arguments.get(key).and_then(|value| {
            std::str::from_utf8(value.bytes())
                .ok()
                .map(|name| name.trim_end_matches(' ').to_owned())
        })
    };
    let Some(channel) = name("CHANNEL").or_else(|| {
        machine
            .invocation
            .bindings
            .get("cics.channel")
            .and_then(|value| std::str::from_utf8(value.bytes()).ok())
            .map(|name| name.trim_end_matches(' ').to_owned())
    }) else {
        return Ok(None);
    };
    let identity = (channel, name("CONTAINER"));
    on_issue(machine, operation, set_requested, Some(&identity));
    Ok(Some(ContainerIdentity::Channel(identity.0, identity.1)))
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
    identity: Option<&ContainerIdentity>,
) {
    match (operation, identity) {
        (
            CicsOperation::DeleteContainer | CicsOperation::MoveContainer,
            Some(ContainerIdentity::Channel(channel, Some(container))),
        ) => {
            release_container(machine, channel, container);
        }
        (CicsOperation::DeleteChannel, Some(ContainerIdentity::Channel(channel, _))) => {
            release_channel(machine, channel)
        }
        _ => {}
    }
}

fn bts_marker(base: usize) -> Vec<u8> {
    let mut marker = BTS_MARKER.to_vec();
    marker.extend_from_slice(&(base as u64).to_be_bytes());
    marker
}

fn owned_bts_base(machine: &ReferenceMachine, base: usize) -> bool {
    base >= machine.static_base_count
        && !machine.freed_allocations.contains(&base)
        && machine.freed_allocations.contains(&(base + 1))
        && machine.bases.get(base + 1).is_some_and(|marker| {
            marker.strip_prefix(BTS_MARKER) == Some((base as u64).to_be_bytes().as_slice())
        })
}

fn active_bts_base(machine: &ReferenceMachine) -> Option<usize> {
    (machine.static_base_count..machine.bases.len())
        .rev()
        .find(|base| owned_bts_base(machine, *base))
}

fn bts_capacity(
    machine: &ReferenceMachine,
    previous: Option<usize>,
) -> Result<usize, MachineProblem> {
    // The existing checkpoint reader bounds ALL retained bytes/base labels and
    // freed entries. Freed markers are not free capacity for serialized state.
    if machine.bases.len().checked_add(2).is_none_or(|count| {
        count > (machine.invocation.limits.max_frames as usize).saturating_mul(1024)
    }) || machine
        .freed_allocations
        .len()
        .checked_add(1 + usize::from(previous.is_some()))
        .is_none_or(|count| count > machine.invocation.limits.max_frames as usize)
    {
        return Err(MachineProblem::ResourceExhausted);
    }
    let retained = machine
        .bases
        .iter()
        .try_fold(BTS_MARKER.len() + 8, |total, bytes| {
            total.checked_add(bytes.len())
        })
        .ok_or(MachineProblem::ResourceExhausted)?;
    usize::try_from(machine.invocation.limits.max_storage_bytes)
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .checked_sub(retained)
        .ok_or(MachineProblem::ResourceExhausted)
}

pub(in crate::machine) fn observed(
    machine: &mut ReferenceMachine,
    identity: Option<&ContainerIdentity>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    let Some(ContainerIdentity::BtsSet { previous, capacity }) = identity else {
        return Ok(());
    };
    // Pending effect identity was checked by resume_host. Capture at prepare
    // rather than discover here: an older observed result cannot free a newer
    // loan. HostProblem paths never reach this boundary.
    if let Some(base) = previous
        && owned_bts_base(machine, *base)
    {
        machine.freed_allocations.insert(*base);
    }
    if response.response == 0 && response.disposition == CicsDisposition::Complete {
        let value = response
            .outputs
            .get("SET")
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let length = response
            .outputs
            .get("FLENGTH")
            .filter(|value| value.schema() == "mainframe-env.cics.decimal@1")
            .and_then(|value| std::str::from_utf8(value.bytes()).ok())
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if value.schema() != "mainframe-env.cics.payload@1"
            || value.bytes().len() > *capacity
            || length != value.bytes().len()
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    } else if response.outputs.contains_key("SET") || response.outputs.contains_key("FLENGTH") {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(())
}

pub(in crate::machine) fn record_bts(
    machine: &mut ReferenceMachine,
    base: usize,
) -> Result<(), MachineProblem> {
    if base < machine.static_base_count
        || machine.bases.len() != base + 1
        || machine.freed_allocations.contains(&base)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let retained = machine
        .bases
        .iter()
        .try_fold(BTS_MARKER.len() + 8, |total, bytes| {
            total.checked_add(bytes.len())
        })
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    if retained as u64 > machine.invocation.limits.max_storage_bytes
        || machine.freed_allocations.len().saturating_add(1)
            > machine.invocation.limits.max_frames as usize
        || machine.bases.len().saturating_add(1)
            > (machine.invocation.limits.max_frames as usize).saturating_mul(1024)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let marker_base = machine.bases.len();
    machine.bases.push(bts_marker(base));
    machine.freed_allocations.insert(marker_base);
    Ok(())
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
        typed_get_profile(containers, false)
    }

    fn typed_get_profile(containers: &[&str], bts: bool) -> ReferenceMachine {
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
                operands: if bts {
                    vec![CicsNamedOperand {
                        name: CicsOperandName::ContainerName,
                        value: CicsOperandValue::Literal(container.as_bytes().to_vec()),
                    }]
                } else {
                    vec![
                        CicsNamedOperand {
                            name: CicsOperandName::ContainerName,
                            value: CicsOperandValue::Literal(container.as_bytes().to_vec()),
                        },
                        CicsNamedOperand {
                            name: CicsOperandName::BtsChannel,
                            value: CicsOperandValue::Literal(b"WORK".to_vec()),
                        },
                    ]
                },
                options: if bts {
                    BTreeSet::from([CicsPlanOption::ContainerProcess])
                } else {
                    BTreeSet::new()
                },
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
        let response = get_response(data);
        machine
            .resume_host(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            })
            .unwrap();
    }

    fn get_response(data: &[u8]) -> CicsResponse {
        CicsResponse {
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
        }
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
            Some(&ContainerIdentity::Channel(
                "WORK".into(),
                Some("ONE".into()),
            )),
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
            Some(&ContainerIdentity::Channel(
                "WORK".into(),
                Some("ONE".into()),
            )),
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
            Some(&ContainerIdentity::Channel("WORK".into(), None)),
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
    fn issue(machine: &mut ReferenceMachine) -> EffectRequest {
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap())
        else {
            panic!("expected typed GET effect")
        };
        effect
    }

    fn complete(
        machine: &mut ReferenceMachine,
        effect: &EffectRequest,
        response: CicsResponse,
    ) -> Result<(), MachineProblem> {
        machine.resume_host(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Cics(response)),
        })
    }

    fn explicit_arguments(selector: &str) -> BTreeMap<String, BoundedPayload> {
        BTreeMap::from([
            (
                selector.into(),
                payload(
                    if selector == "ACTIVITY" {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.option@1"
                    },
                    if selector == "ACTIVITY" {
                        b"CHILD".to_vec()
                    } else {
                        Vec::new()
                    },
                )
                .unwrap(),
            ),
            (
                "CONTAINER".into(),
                payload("mainframe-env.cics.literal@1", b"TWO".to_vec()).unwrap(),
            ),
            (
                "SET.MAXLENGTH".into(),
                payload("mainframe-env.cics.decimal@1", b"1024".to_vec()).unwrap(),
            ),
        ])
    }

    #[test]
    fn explicit_bts_prepare_is_pure_and_selectors_never_use_channel_markers() {
        let mut machine = typed_get_profile(&["ONE"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let bts = pointer_base(&machine, "PTR-A");
        let channel = saved(&mut machine, "WORK", "ONE");
        machine.invocation.bindings.insert(
            "cics.channel".into(),
            payload("mainframe-env.cics.literal@1", b"WORK".to_vec()).unwrap(),
        );
        let before = machine.snapshot();
        for selector in [
            "ACTIVITY",
            "OPTION.PROCESS",
            "OPTION.ACQPROCESS",
            "OPTION.ACQACTIVITY",
        ] {
            let identity = prepare(
                &mut machine,
                CicsOperation::GetContainer,
                &mut explicit_arguments(selector),
                true,
            )
            .unwrap();
            assert!(
                matches!(identity, Some(ContainerIdentity::BtsSet { previous: Some(base), .. }) if base == bts)
            );
            assert_eq!(machine.snapshot(), before);
        }
        let identity = prepare(
            &mut machine,
            CicsOperation::GetContainer,
            &mut explicit_arguments("OPTION.PROCESS"),
            false,
        )
        .unwrap();
        assert!(identity.is_none());
        assert_eq!(machine.snapshot(), before);
        let identity = prepare(
            &mut machine,
            CicsOperation::GetContainer,
            &mut explicit_arguments("OPTION.PROCESS"),
            true,
        )
        .unwrap();
        observed(&mut machine, identity.as_ref(), &get_response(b"second")).unwrap();
        assert!(machine.freed_allocations.contains(&bts));
        assert!(!machine.freed_allocations.contains(&channel));
    }

    #[test]
    fn omitted_selector_bts_loan_is_not_inferred_from_absent_channel() {
        let mut machine = typed_get_profile(&["ONE"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let before = machine.snapshot();
        let mut arguments = explicit_arguments("OPTION.PROCESS");
        arguments.remove("OPTION.PROCESS");
        assert!(
            prepare(
                &mut machine,
                CicsOperation::GetContainer,
                &mut arguments,
                true
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(machine.snapshot(), before);
    }

    #[test]
    fn explicit_bts_host_failures_and_effect_quota_do_not_expire_a_loan() {
        for problem in [
            HostProblem::UnknownOutcome,
            HostProblem::Unauthorized,
            HostProblem::TimedOut,
            HostProblem::Cancelled,
            HostProblem::InfrastructureFailure,
        ] {
            let mut machine = typed_get_profile(&["ONE", "TWO"], true);
            resume_get(&mut machine, MachineResume::Start, b"first");
            let before = machine.snapshot();
            let effect = issue(&mut machine);
            assert!(
                machine
                    .resume_host(EffectResult {
                        sequence: effect.sequence,
                        outcome: Err(problem)
                    })
                    .is_err()
            );
            assert_eq!(machine.bases, before.base_storage);
            assert_eq!(machine.freed_allocations, before.freed_allocations);
        }
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let before = machine.snapshot();
        machine.invocation.limits.max_effects = machine.effect_sequence;
        assert!(matches!(
            machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
            MachineDrive::Failed(problem) if problem.category == FailureCategory::ResourceExhausted
        ));
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.freed_allocations, before.freed_allocations);
    }

    #[test]
    fn explicit_bts_stale_observed_identity_and_duplicate_resume_preserve_newer_loan() {
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let stale = prepare(
            &mut machine,
            CicsOperation::GetContainer,
            &mut explicit_arguments("OPTION.PROCESS"),
            true,
        )
        .unwrap();
        let effect = issue(&mut machine);
        let response = get_response(b"second");
        complete(&mut machine, &effect, response.clone()).unwrap();
        let newer = pointer_base(&machine, "PTR-B");
        let before = machine.snapshot();
        observed(&mut machine, stale.as_ref(), &response).unwrap();
        assert_eq!(machine.snapshot(), before);
        assert!(!machine.freed_allocations.contains(&newer));
        assert!(matches!(
            complete(&mut machine, &effect, response),
            Err(MachineProblem::UnexpectedResume)
        ));
        assert_eq!(machine.snapshot(), before);
    }

    #[test]
    fn explicit_bts_known_condition_has_no_new_pointer_and_rejects_hidden_set_output() {
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let first = pointer_base(&machine, "PTR-A");
        let pointer = machine.read("PTR-B").unwrap();
        let length = machine.read("LEN-X").unwrap();
        let bases = machine.bases.len();
        let effect = issue(&mut machine);
        let mut response = get_response(b"");
        response.condition = "CONTAINERERR".into();
        response.response = 110;
        response.response2 = 10;
        response.outputs.clear();
        complete(&mut machine, &effect, response).unwrap();
        assert!(machine.freed_allocations.contains(&first));
        assert_eq!(machine.bases.len(), bases);
        assert_eq!(machine.read("PTR-B").unwrap(), pointer);
        assert_eq!(machine.read("LEN-X").unwrap(), length);
        let mut invalid = get_response(b"hidden");
        invalid.response = 110;
        let identity = ContainerIdentity::BtsSet {
            previous: None,
            capacity: 1024,
        };
        assert!(observed(&mut machine, Some(&identity), &invalid).is_err());
        let mut invalid = get_response(b"hidden");
        invalid.outputs.remove("SET");
        assert!(observed(&mut machine, Some(&identity), &invalid).is_err());
        let mut invalid = get_response(b"hidden");
        invalid.outputs.insert(
            "FLENGTH".into(),
            payload("mainframe-env.cics.decimal@1", b"1".to_vec()).unwrap(),
        );
        assert!(observed(&mut machine, Some(&identity), &invalid).is_err());
        let mut invalid = get_response(b"hidden");
        invalid.response = 110;
        invalid.outputs.remove("SET");
        assert!(observed(&mut machine, Some(&identity), &invalid).is_err());
        assert_eq!(machine.bases.len(), bases);
    }

    #[test]
    fn explicit_bts_metadata_capacity_address_and_oversized_response_fail_without_publication() {
        let mut machine = typed_get_profile(&["ONE"], true);
        let used = machine.bases.iter().map(Vec::len).sum::<usize>();
        machine.invocation.limits.max_storage_bytes = (used + BTS_MARKER.len() + 8 + 3) as u64;
        let effect = issue(&mut machine);
        let HostRequest::Cics(request) = &effect.request else {
            panic!()
        };
        assert_eq!(request.arguments["SET.MAXLENGTH"].bytes(), b"3");
        let before = machine.snapshot();
        assert!(complete(&mut machine, &effect, get_response(b"four")).is_err());
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.read("PTR-A").unwrap(), [0; 4]);

        let mut machine = typed_get_profile(&["ONE"], true);
        machine.invocation.limits.max_storage_bytes = (used + BTS_MARKER.len() + 7) as u64;
        let before = machine.snapshot();
        assert!(
            prepare(
                &mut machine,
                CicsOperation::GetContainer,
                &mut explicit_arguments("OPTION.PROCESS"),
                true
            )
            .is_err()
        );
        assert_eq!(machine.snapshot(), before);

        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        machine.invocation.limits.max_frames = 1;
        let before = machine.snapshot();
        assert!(
            prepare(
                &mut machine,
                CicsOperation::GetContainer,
                &mut explicit_arguments("OPTION.PROCESS"),
                true
            )
            .is_err()
        );
        assert_eq!(machine.snapshot(), before);

        let mut machine = typed_get_profile(&["ONE"], true);
        machine.invocation.limits.max_frames = 10_000;
        machine.bases.resize(4095, Vec::new());
        let before = machine.snapshot();
        assert!(matches!(
            machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
            MachineDrive::Failed(problem) if problem.category == FailureCategory::ResourceExhausted
        ));
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.freed_allocations, before.freed_allocations);
    }

    #[test]
    fn explicit_bts_checkpoint_is_pure_and_restores_active_then_expired_owned_bases() {
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let first = pointer_base(&machine, "PTR-A");
        let before = machine.snapshot();
        let checkpoint = machine.checkpoint().unwrap();
        assert_eq!(machine.snapshot(), before);
        let mut restored = typed_get_profile(&["ONE", "TWO"], true);
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.snapshot(), before);
        assert_eq!(active_bts_base(&restored), Some(first));
        resume_get(&mut restored, MachineResume::Start, b"second");
        let second = pointer_base(&restored, "PTR-B");
        let checkpoint = restored.checkpoint().unwrap();
        machine.restore_checkpoint(&checkpoint).unwrap();
        assert!(machine.freed_allocations.contains(&first));
        assert!(!machine.freed_allocations.contains(&second));
        assert_eq!(active_bts_base(&machine), Some(second));
    }

    #[test]
    fn legacy_unmarked_bts_area_is_not_arbitrarily_freed_after_restore() {
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let first = pointer_base(&machine, "PTR-A");
        let label = machine.bases.len() - 1;
        machine.bases.pop();
        machine.freed_allocations.remove(&label);
        let checkpoint = machine.checkpoint().unwrap();
        machine.restore_checkpoint(&checkpoint).unwrap();
        assert!(active_bts_base(&machine).is_none());
        resume_get(&mut machine, MachineResume::Start, b"second");
        assert!(!machine.freed_allocations.contains(&first));
    }
    #[test]
    fn explicit_bts_invalid_effect_and_unobserved_condition_preserve_old_loan() {
        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let before = machine.snapshot();
        machine.invocation.deadline_tick = 0;
        let operation = machine.operations[machine.pc].clone();
        assert!(matches!(
            machine.execute(&operation),
            Err(MachineProblem::Host(HostProblem::Malformed))
        ));
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.freed_allocations, before.freed_allocations);
        assert!(machine.pending.is_none());

        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let before = machine.snapshot();
        let effect = issue(&mut machine);
        assert!(
            machine
                .resume_host(EffectResult {
                    sequence: effect.sequence + 1,
                    outcome: Ok(HostResult::Cics(get_response(b"second")))
                })
                .is_err()
        );
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.freed_allocations, before.freed_allocations);

        let mut machine = typed_get_profile(&["ONE", "TWO"], true);
        resume_get(&mut machine, MachineResume::Start, b"first");
        let before = machine.snapshot();
        let effect = issue(&mut machine);
        // A HostProblem carrying the same numbers is not an observed CicsResponse.
        assert!(
            machine
                .resume_host(EffectResult {
                    sequence: effect.sequence,
                    outcome: Err(HostProblem::Condition {
                        name: "CONTAINERERR".into(),
                        response: 110,
                        response2: 10
                    })
                })
                .is_err()
        );
        assert_eq!(machine.bases, before.base_storage);
        assert_eq!(machine.freed_allocations, before.freed_allocations);
    }
}
