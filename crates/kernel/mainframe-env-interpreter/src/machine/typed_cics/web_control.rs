use super::*;

pub(super) fn output_capacity(
    machine: &ReferenceMachine,
    name: CicsOutputName,
    target: &CicsTarget,
) -> Result<Option<(String, BoundedPayload)>, MachineProblem> {
    let source = match name {
        CicsOutputName::WebHost => "HOST.MAXLENGTH",
        CicsOutputName::WebPath => "PATH.MAXLENGTH",
        CicsOutputName::WebQueryString => "QUERYSTRING.MAXLENGTH",
        _ => return Ok(None),
    };
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    Ok(Some((
        source.into(),
        payload(
            "mainframe-env.cics.decimal@1",
            resolved_slot(machine, slot)?
                .length
                .to_string()
                .into_bytes(),
        )?,
    )))
}
