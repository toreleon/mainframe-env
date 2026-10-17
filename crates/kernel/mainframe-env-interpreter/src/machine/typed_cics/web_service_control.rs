use super::*;

pub(super) const fn numeric_operand(name: CicsOperandName) -> bool {
    matches!(
        name,
        CicsOperandName::ScopeLen
            | CicsOperandName::FaultCodeLen
            | CicsOperandName::FaultStrLen
            | CicsOperandName::RoleLength
            | CicsOperandName::FaultActLen
            | CicsOperandName::DetailLength
            | CicsOperandName::FromCcsid
            | CicsOperandName::SubcodeLen
            | CicsOperandName::RelatesIndex
            | CicsOperandName::EprLength
            | CicsOperandName::IntoCcsid
            | CicsOperandName::RefParmsLen
            | CicsOperandName::MetadataLen
    )
}

pub(super) fn output_arguments(
    machine: &ReferenceMachine,
    name: CicsOutputName,
    target: &CicsTarget,
    arguments: &mut BTreeMap<String, BoundedPayload>,
) -> Result<(), MachineProblem> {
    if name == CicsOutputName::WebEprInto {
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let capacity = resolved_slot(machine, slot)?.length;
        arguments.insert(
            "EPRINTO.MAXLENGTH".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                capacity.to_string().into_bytes(),
            )?,
        );
    }
    if name == CicsOutputName::WebEprSet {
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let pointer = resolved_slot(machine, slot)?;
        if !matches!(
            pointer.layout.category,
            LayoutCategory::Pointer | LayoutCategory::Pointer32
        ) {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let capacity = retrieve::allocation_capacity(machine, target)?;
        let live = (machine.static_base_count..machine.bases.len())
            .filter(|base| !machine.freed_allocations.contains(base))
            .count();
        let capacity = if live + machine.storage64.live_allocations() + 2
            > machine.invocation.limits.max_frames as usize
        {
            0
        } else {
            capacity
        };
        arguments.insert(
            "EPRSET.MAXLENGTH".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                capacity.to_string().into_bytes(),
            )?,
        );
    }
    Ok(())
}

pub(super) fn write_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    target: &CicsTarget,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if !matches!(
        operation,
        CicsOperation::WsaContextGet | CicsOperation::WsaEprCreate
    ) {
        return Ok(false);
    }
    if name == "EPRSET" {
        if value.schema() != "mainframe-env.cics.payload@1" {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let capacity = retrieve::allocation_capacity(machine, target)?;
        if value.bytes().len() > capacity {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let pointer = resolved_slot(machine, slot)?;
        let marker = machine.bases.len();
        let address = machine.address_bytes_for(marker + 3, 0, pointer.length)?;
        machine
            .bases
            .extend([Vec::new(), Vec::new(), Vec::new(), value.bytes().to_vec()]);
        machine.freed_allocations.extend([marker, marker + 2]);
        machine.write_reference(&pointer, &address)?;
        return Ok(true);
    }
    let payload = matches!(
        name,
        "ACTION" | "MESSAGEID" | "RELATESURI" | "RELATESTYPE" | "EPRINTO"
    );
    let decimal = name == "EPRLENGTH";
    if (payload || decimal)
        && value.schema()
            != if payload {
                "mainframe-env.cics.payload@1"
            } else {
                "mainframe-env.cics.decimal@1"
            }
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(false)
}

pub(super) fn validate_response(
    operation: CicsOperation,
    outputs: &BTreeMap<String, CicsTarget>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    if !matches!(
        operation,
        CicsOperation::InvokeService
            | CicsOperation::SoapFaultAdd
            | CicsOperation::SoapFaultCreate
            | CicsOperation::SoapFaultDelete
            | CicsOperation::WsaContextBuild
            | CicsOperation::WsaContextDelete
            | CicsOperation::WsaContextGet
            | CicsOperation::WsaEprCreate
    ) {
        return Ok(());
    }
    if response
        .outputs
        .keys()
        .any(|name| !outputs.contains_key(name))
        || !matches!(
            response.disposition,
            CicsDisposition::Complete | CicsDisposition::Ignored | CicsDisposition::Handler
        )
        || matches!(
            operation,
            CicsOperation::WsaContextGet | CicsOperation::WsaEprCreate
        ) && response.response == 0
            && !response.outputs.contains_key("EPRLENGTH")
            && (outputs.contains_key("EPRINTO") || outputs.contains_key("EPRSET"))
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(())
}

pub(super) fn release_previous_set(machine: &mut ReferenceMachine) {
    let mut releases = Vec::new();
    for marker in machine.static_base_count..machine.bases.len().saturating_sub(3) {
        if machine.bases[marker].is_empty()
            && machine.bases[marker + 1].is_empty()
            && machine.bases[marker + 2].is_empty()
            && machine.freed_allocations.contains(&marker)
            && !machine.freed_allocations.contains(&(marker + 1))
            && machine.freed_allocations.contains(&(marker + 2))
            && !machine.freed_allocations.contains(&(marker + 3))
        {
            releases.push(marker + 3);
        }
    }
    machine.freed_allocations.extend(releases);
}
