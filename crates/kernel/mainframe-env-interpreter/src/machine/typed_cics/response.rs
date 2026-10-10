use super::*;

pub(in crate::machine) fn apply_payload_outputs(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    into: Option<&CicsTarget>,
    outputs: &BTreeMap<String, CicsTarget>,
    response: &CicsResponse,
    load_base: Option<usize>,
) -> Result<Option<usize>, MachineProblem> {
    let symbolic = project_receive_map(machine, operation, into, response)?;
    if !symbolic
        && let Some(target) = into
        && matches!(
            into_payload_schema(operation, response),
            Some("mainframe-env.cics.into@1" | "mainframe-env.cics.payload@1")
        )
    {
        write_target(
            machine,
            target,
            &CobolValue::Bytes(response.payload.bytes().to_vec()),
        )?;
    }
    let mut container_set_base = None;
    for (name, value) in &response.outputs {
        if write_runtime_output(machine, operation, name, value)? {
            continue;
        }
        if let Some(field) = name.strip_prefix("BMS.") {
            if !symbolic {
                write_legacy_bms_field(machine, field, value)?;
            }
            continue;
        }
        let Some(target) = outputs.get(name) else {
            continue;
        };
        let base = machine.bases.len();
        write_output(machine, operation, name, target, value, load_base)?;
        if operation == CicsOperation::GetContainer
            && name == "SET"
            && value.schema() == "mainframe-env.cics.payload@1"
            && machine.bases.len() == base + 1
        {
            container_set_base = Some(base);
        }
    }
    Ok(container_set_base)
}

fn write_legacy_bms_field(
    machine: &mut ReferenceMachine,
    field: &str,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    if let Some(field) = field.strip_suffix(".LENGTH") {
        let target = format!("{field}L");
        if machine.layout(&target).is_some() {
            let coefficient = String::from_utf8_lossy(value.bytes())
                .parse::<i128>()
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            machine.write_decimal(
                &target,
                Decimal {
                    coefficient,
                    scale: 0,
                },
            )?;
        }
    } else {
        let target = format!("{field}I");
        if machine.layout(&target).is_some() {
            machine.write(&target, value.bytes())?;
        }
    }
    Ok(())
}

fn project_receive_map(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    into: Option<&CicsTarget>,
    response: &CicsResponse,
) -> Result<bool, MachineProblem> {
    // Legacy targets and other operations retain their original raw INTO route.
    let (CicsOperation::ReceiveMap, Some(CicsTarget::Resolved(slot))) = (operation, into) else {
        return Ok(false);
    };
    let group = resolved_slot(machine, slot)?;
    if group.layout.category != LayoutCategory::Group
        || group.layout.dynamic
        || group.layout.occurs != 1
    {
        return Ok(false);
    }
    read_slot(machine, slot)?;
    let mut fields = BTreeMap::new();
    for layout in machine.layouts.values() {
        let Some(field) = layout.simple_name.strip_suffix('I') else {
            continue;
        };
        if !within_map_group(machine, layout, &group.layout.name) {
            continue;
        }
        let Some(length) = map_member(machine, &group, &format!("{field}L"))? else {
            continue;
        };
        if length.layout.category != LayoutCategory::Binary
            || length.length != 2
            || length.layout.scale != 0
            || !length.layout.signed
            || !(matches!(
                layout.category,
                LayoutCategory::Alphanumeric | LayoutCategory::Alphabetic
            ) || (layout.category == LayoutCategory::NumericDisplay
                && !layout.signed
                && layout.scale == 0))
        {
            continue;
        }
        let data = map_member(machine, &group, &layout.simple_name)?
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if fields.insert(field.to_string(), (data, length)).is_some() {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    }
    if fields.is_empty() {
        return Ok(false);
    }
    // Prepare every selected write before mutation; transport bytes never enter this group.
    let mut writes = Vec::new();
    for (name, value) in &response.outputs {
        let Some(field) = name.strip_prefix("BMS.") else {
            continue;
        };
        let (field, is_length) = field
            .strip_suffix(".LENGTH")
            .map_or((field, false), |field| (field, true));
        let (data, length) = fields
            .get(field)
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if is_length {
            if value.schema() != "mainframe-env.cics.decimal@1" {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            let count = std::str::from_utf8(value.bytes())
                .ok()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|count| usize::from(*count) <= data.length && *count <= i16::MAX as u16)
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            let number = Decimal {
                coefficient: i128::from(count),
                scale: 0,
            };
            writes.push((length.clone(), encode_decimal(&length.layout, number)?));
        } else {
            if value.schema() != "mainframe-env.cics.payload@1" || value.bytes().len() > data.length
            {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            // Numeric DISPLAY receives terminal bytes, not a COBOL numeric assignment.
            // A short prefix leaves surplus storage intact (DFHMDI input-field note).
            // Existing text fields retain their fixed-field fitting contract.
            let bytes = if data.layout.category == LayoutCategory::NumericDisplay {
                let mut bytes = machine.read_reference(data)?;
                bytes[..value.bytes().len()].copy_from_slice(value.bytes());
                bytes
            } else {
                FixedValue::fit(value.bytes(), data.length, data.layout.justified_right)
                    .bytes()
                    .to_vec()
            };
            writes.push((data.clone(), bytes));
        }
    }
    for (reference, bytes) in writes {
        machine.write_reference(&reference, &bytes)?;
    }
    Ok(true)
}

fn within_map_group(machine: &ReferenceMachine, layout: &LayoutMetadata, group: &str) -> bool {
    let mut parent = layout.parent.as_deref();
    for _ in 0..machine.layouts.len() {
        let Some(name) = parent else {
            return false;
        };
        if name == group {
            return true;
        }
        parent = machine
            .layouts
            .get(name)
            .and_then(|layout| layout.parent.as_deref());
    }
    false
}

fn map_member(
    machine: &ReferenceMachine,
    group: &ResolvedReference,
    simple_name: &str,
) -> Result<Option<ResolvedReference>, MachineProblem> {
    let Some(names) = machine.simple_layouts.get(simple_name) else {
        return Ok(None);
    };
    let mut members = names
        .iter()
        .filter_map(|name| machine.layouts.get(name))
        .filter(|layout| within_map_group(machine, layout, &group.layout.name));
    let Some(layout) = members.next() else {
        return Ok(None);
    };
    if members.next().is_some() || layout.dynamic || layout.occurs != 1 {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let reference = machine.reference(std::slice::from_ref(&layout.name))?;
    let group_view = machine.storage_view(&group.layout.name)?;
    let view = machine.storage_view(&layout.name)?;
    let group_end = group_view.offset.checked_add(group.length);
    let end = view.offset.checked_add(reference.length);
    if view.base != group_view.base
        || view.offset < group_view.offset
        || !matches!((end, group_end), (Some(end), Some(group_end)) if end <= group_end)
        || reference.length != layout.length
        || reference.length != view.length
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    machine.read_reference(&reference)?;
    Ok(Some(reference))
}

pub(in crate::machine) fn finish(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    container_identity: Option<&container_set::ContainerIdentity>,
    container_set_base: Option<usize>,
    response: CicsResponse,
    responded: bool,
) -> Result<(), MachineProblem> {
    if response.response == 0 && response.disposition == CicsDisposition::Complete {
        container_set::on_success(machine, operation, container_identity);
        if operation == CicsOperation::GetContainer
            && let Some(base) = container_set_base
        {
            match container_identity {
                Some(container_set::ContainerIdentity::Channel(channel, Some(container)))
                    if machine.bases.len() == base + 1 =>
                {
                    container_set::record(machine, channel, container)
                }
                Some(container_set::ContainerIdentity::BtsSet { .. }) => {
                    container_set::record_bts(machine, base)?
                }
                _ => {}
            }
        }
    }
    eib::write_context(machine, operation, &response)?;
    machine.deferred_drive = drive_response(machine, operation, response, responded)?;
    Ok(())
}

// Private call context keeps the observed hook with output preparation without
// growing the frozen machine facade. No serialized or host ABI type changes.
pub(in crate::machine) struct ResponseCommand<'a>(
    CicsOperation,
    Option<&'a container_set::ContainerIdentity>,
);

impl<'a> From<(CicsOperation, Option<&'a container_set::ContainerIdentity>)>
    for ResponseCommand<'a>
{
    fn from(command: (CicsOperation, Option<&'a container_set::ContainerIdentity>)) -> Self {
        Self(command.0, command.1)
    }
}

// Existing direct response-conversion tests supply no pending loan context.
#[cfg(test)]
impl From<CicsOperation> for ResponseCommand<'_> {
    fn from(operation: CicsOperation) -> Self {
        Self(operation, None)
    }
}

pub(in crate::machine) fn write_response_state<'a>(
    machine: &mut ReferenceMachine,
    command: impl Into<ResponseCommand<'a>>,
    storage64_intent: Option<Storage64Intent>,
    response_target: Option<&CicsTarget>,
    response2_target: Option<&CicsTarget>,
    address_set: Option<&CicsAddressSet>,
    outputs: &BTreeMap<String, CicsTarget>,
    response: &CicsResponse,
) -> Result<Option<usize>, MachineProblem> {
    let ResponseCommand(operation, container_identity) = command.into();
    container_set::observed(machine, container_identity, response)?;
    storage64::validate_response(operation, storage64_intent, response)?;
    if let (CicsOperation::GetContainer64, Some(Storage64Intent::GetContainer(Some((address, _))))) =
        (operation, storage64_intent)
        && let Some(value) = response.outputs.get("INTO")
    {
        machine
            .write_storage64(address, 0, value.bytes())
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    }
    web_service_control::validate_response(operation, outputs, response)?;
    for (target, value) in [
        (response_target, response.response),
        (response2_target, response.response2),
    ] {
        if let Some(target) = target {
            write_target(
                machine,
                target,
                &CobolValue::Decimal(Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                }),
            )?;
        }
    }
    if response.disposition == CicsDisposition::Complete
        && response.response == 0
        && let Some(action) = address_set
    {
        address::apply(machine, action)?;
    }
    let load_base = retrieve::prepare_load_allocation(machine, operation, response, outputs)?;
    certificate::apply_outputs(machine, operation, outputs, response)?;
    Ok(load_base)
}

pub(in crate::machine) fn write_runtime_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if post::apply_event(machine, name, value)? {
        return Ok(true);
    }
    if retrieve::release_output(machine, operation, name, value)? {
        return Ok(true);
    }
    if operation == CicsOperation::GetContainer64 && name == "INTO" {
        return Ok(true);
    }
    if storage64::release_output(machine, operation, name, value)? {
        return Ok(true);
    }
    if task_wait::apply_posted_output(machine, operation, name, value)? {
        return Ok(true);
    }
    if operation == CicsOperation::ExtractCertificate
        && CicsCertificateOutput::from_name(name).is_some_and(CicsCertificateOutput::pointer)
    {
        return Ok(true);
    }
    if name != "TASK.PRIORITY" {
        return Ok(false);
    }
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    machine.invocation.priority = String::from_utf8_lossy(value.bytes())
        .parse::<u8>()
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    Ok(true)
}

pub(in crate::machine) fn drive_response(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    response: CicsResponse,
    responded: bool,
) -> Result<Option<MachineDrive<EffectRequest>>, MachineProblem> {
    Ok(match response.disposition {
        CicsDisposition::Complete => {
            (response.response != 0 && !responded).then_some(MachineDrive::Condition(Condition {
                name: response.condition,
                response: response.response,
                response2: response.response2,
                handled: false,
            }))
        }
        CicsDisposition::Ignored => None,
        CicsDisposition::Suspended => Some(suspension(
            machine,
            operation,
            response.payload.bytes().len(),
        )),
        CicsDisposition::Transfer => {
            let target = response
                .target
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            Some(MachineDrive::Transfer(Transfer {
                selector: Selector::new(target, InvocationLimits::default())
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?,
                payload: response.payload,
                replace_frame: true,
            }))
        }
        CicsDisposition::Handler => {
            let target = response
                .target
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            machine.pc = machine
                .labels
                .get(&normalize(&target))
                .copied()
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            None
        }
        CicsDisposition::Returned => Some(MachineDrive::Completed(machine.complete()?)),
        CicsDisposition::Abended => {
            machine.release_storage64_task();
            Some(MachineDrive::Abend(abend_outcome(operation, &response)?))
        }
    })
}
