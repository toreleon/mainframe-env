use super::*;

pub(super) fn encode_call_values(
    names: &[String],
    values: &[Vec<u8>],
) -> Result<BoundedPayload, MachineProblem> {
    if names.len() != values.len() {
        return Err(MachineProblem::InvalidOperation);
    }
    let mut bytes = u32::try_from(values.len())
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .to_be_bytes()
        .to_vec();
    for (name, value) in names.iter().zip(values) {
        push_host_field(&mut bytes, name.as_bytes())?;
        bytes.push(1);
        push_host_field(&mut bytes, value)?;
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| MachineProblem::ResourceExhausted)
}

pub(super) fn decode_call_arguments(
    payload: &BoundedPayload,
) -> Result<Vec<Vec<u8>>, MachineProblem> {
    if !matches!(
        payload.schema(),
        "mainframe-env.cobol.call@1" | "mainframe-env.cobol.batch-main@1"
    ) {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let mut input = SnapshotInput::new(payload.bytes());
    let count = usize::try_from(input.u32()?).map_err(|_| MachineProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let _name = input.bytes(4096)?;
        if input.take(1)? != [1] {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        values.push(input.bytes(InvocationLimits::default().max_payload_bytes)?);
    }
    if !input.finished() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(values)
}

pub fn encode_cobol_call_result(values: &[Vec<u8>]) -> Result<BoundedPayload, MachineProblem> {
    let mut bytes = u32::try_from(values.len())
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .to_be_bytes()
        .to_vec();
    for value in values {
        push_host_field(&mut bytes, value)?;
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call-result@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| MachineProblem::ResourceExhausted)
}

pub(super) fn ims_option_groups(
    args: &[String],
) -> Result<Vec<(String, Vec<String>)>, MachineProblem> {
    let mut groups = Vec::new();
    for (index, token) in args.iter().enumerate().filter(|(_, token)| {
        matches!(
            token.as_str(),
            "PCB" | "SEGMENT" | "INTO" | "FROM" | "WHERE" | "PSB" | "ID" | "SEGLENGTH"
        )
    }) {
        let open = index + 1;
        if args.get(open).is_none_or(|token| token != "(") {
            continue;
        }
        let close = matching_close(args, open).ok_or(MachineProblem::InvalidOperation)?;
        groups.push((token.clone(), args[open + 1..close].to_vec()));
    }
    Ok(groups)
}

pub(super) fn push_host_field(output: &mut Vec<u8>, value: &[u8]) -> Result<(), MachineProblem> {
    output.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| MachineProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
    Ok(())
}

pub(super) fn decode_call_values(payload: &BoundedPayload) -> Result<Vec<Vec<u8>>, MachineProblem> {
    if payload.schema() != "mainframe-env.cobol.call-result@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let mut input = SnapshotInput::new(payload.bytes());
    let count = usize::try_from(input.u32()?).map_err(|_| MachineProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(input.bytes(InvocationLimits::default().max_payload_bytes)?);
    }
    if !input.finished() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(values)
}

impl ReferenceMachine {
    // Option groups retain source order; WHERE qualifies its preceding SEGMENT.
    pub(super) fn ims_where_qualifiers(
        &self,
        groups: &[(String, Vec<String>)],
    ) -> Result<Vec<ImsQualifier>, MachineProblem> {
        let mut segment = None;
        let mut qualifiers = Vec::new();
        for (name, values) in groups {
            if name == "SEGMENT" {
                segment = if let [name] = values.as_slice() {
                    let name = normalize(name);
                    (!name.is_empty()).then_some(name)
                } else {
                    None
                };
            } else if name == "WHERE" {
                let segment = segment.clone().ok_or(MachineProblem::InvalidOperation)?;
                let Some(equal) = values.iter().position(|token| token == "=") else {
                    return Err(MachineProblem::UnsupportedForm);
                };
                let field = values
                    .get(equal.saturating_sub(1))
                    .ok_or(MachineProblem::InvalidOperation)?;
                let value_name = values
                    .get(equal + 1)
                    .ok_or(MachineProblem::InvalidOperation)?;
                qualifiers.push(ImsQualifier {
                    segment,
                    field: normalize(field),
                    value: self.resolve(value_name)?,
                });
            }
        }
        Ok(qualifiers)
    }
}
