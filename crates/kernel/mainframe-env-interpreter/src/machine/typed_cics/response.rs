use super::*;

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
