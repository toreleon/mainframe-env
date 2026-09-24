use super::*;

pub(super) fn for_binding(
    machine: &ReferenceMachine,
    name: CicsOutputName,
    target: &CicsTarget,
) -> Result<Option<(String, BoundedPayload)>, MachineProblem> {
    if matches!(
        name,
        CicsOutputName::ConversationInto
            | CicsOutputName::ConversationToLength
            | CicsOutputName::ConversationToFullLength
    ) {
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let (key, number) = match name {
            CicsOutputName::ConversationInto => (
                "CAPACITY.INTO",
                i64::try_from(resolved_slot(machine, slot)?.length)
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?,
            ),
            CicsOutputName::ConversationToLength => {
                let raw = read_slot(machine, slot)?;
                let [first, second] = raw.as_slice() else {
                    return Err(MachineProblem::UnexpectedHostResult);
                };
                (
                    "CAPACITY.TOLENGTH",
                    i64::from(i16::from_be_bytes([*first, *second])),
                )
            }
            CicsOutputName::ConversationToFullLength => {
                let raw = read_slot(machine, slot)?;
                let [a, b, c, d] = raw.as_slice() else {
                    return Err(MachineProblem::UnexpectedHostResult);
                };
                (
                    "CAPACITY.TOFLENGTH",
                    i64::from(i32::from_be_bytes([*a, *b, *c, *d])),
                )
            }
            _ => unreachable!(),
        };
        return Ok(Some((
            key.into(),
            payload(
                "mainframe-env.cics.decimal@1",
                number.to_string().into_bytes(),
            )?,
        )));
    }
    let source = match name {
        CicsOutputName::WebHost => "HOST.MAXLENGTH",
        CicsOutputName::WebPath => "PATH.MAXLENGTH",
        CicsOutputName::WebQueryString => "QUERYSTRING.MAXLENGTH",
        CicsOutputName::WebHttpMethod => "HTTPMETHOD.MAXLENGTH",
        CicsOutputName::WebHttpVersion => "HTTPVERSION.MAXLENGTH",
        CicsOutputName::WebRealm => "REALM.MAXLENGTH",
        CicsOutputName::WebValue => "VALUE.MAXLENGTH",
        CicsOutputName::WebBrowseName => "BROWSENAME.MAXLENGTH",
        CicsOutputName::WebReceiveInto => "INTO.MAXLENGTH",
        CicsOutputName::WebConverseInto => "INTO.MAXLENGTH",
        CicsOutputName::WebReceiveStatusText => "STATUSTEXT.MAXLENGTH",
        CicsOutputName::WebConverseStatusText => "STATUSTEXT.MAXLENGTH",
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
