//! Typed explicit and logical-program ABEND outcome metadata.
use super::*;

pub(super) fn abend_dump_disposition(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<AbendDumpDisposition, MachineProblem> {
    if operation != CicsOperation::Abend && propagated_abend_code(operation, response)?.is_none() {
        return Ok(AbendDumpDisposition::Unspecified);
    }
    let Some(value) = response.outputs.get("ABEND.DUMP") else {
        // Retained responses written before this metadata was introduced do
        // not claim a dump decision.
        return Ok(AbendDumpDisposition::Unspecified);
    };
    if value.schema() != "mainframe-env.cics.abend-dump@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    match value.bytes() {
        b"requested" => Ok(AbendDumpDisposition::Requested),
        b"suppressed" => Ok(AbendDumpDisposition::Suppressed),
        _ => Err(MachineProblem::UnexpectedHostResult),
    }
}

pub(in crate::machine) fn abend_outcome(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<Abend, MachineProblem> {
    let code = if let Some(code) = propagated_abend_code(operation, response)? {
        code.to_string()
    } else if operation == CicsOperation::Abend && !response.payload.bytes().is_empty() {
        String::from_utf8(response.payload.bytes().to_vec())
            .map_err(|_| MachineProblem::UnexpectedHostResult)?
    } else {
        response.condition.clone()
    };
    Ok(Abend {
        code,
        reason: Some(format!(
            "EIBRESP={} EIBRESP2={}",
            response.response, response.response2
        )),
        dump: abend_dump_disposition(operation, response)?,
    })
}

fn propagated_abend_code(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<Option<&str>, MachineProblem> {
    let Some(code) = response.outputs.get("ABEND.CODE") else {
        return Ok(None);
    };
    if !matches!(
        operation,
        CicsOperation::Link | CicsOperation::InvokeApplication
    ) || response.disposition != CicsDisposition::Abended
        || code.schema() != "mainframe-env.cics.abend-code@1"
        || code.bytes().len() > 4
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let code =
        std::str::from_utf8(code.bytes()).map_err(|_| MachineProblem::UnexpectedHostResult)?;
    Ok(Some(if code.is_empty() {
        &response.condition
    } else {
        code
    }))
}
