use super::*;

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
        CicsDisposition::Abended => Some(MachineDrive::Abend(abend_outcome(operation, &response)?)),
    })
}
