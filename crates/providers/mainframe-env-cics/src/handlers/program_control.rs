use super::super::{CicsService, Run, argument_bytes, argument_text, bounded};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, HostResult, ProgramName, ProgramRequest,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::Inquire => inquire(service, run, request),
        CicsOperation::Link | CicsOperation::Xctl => transfer(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn inquire(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let target = argument_text(request, "PROGRAM")?
        .trim()
        .to_ascii_uppercase();
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.PROGRAM.{target}"),
        AccessIntent::Execute,
    )?;
    if service.lock()?.programs.contains(&target) {
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    let program = ProgramName::new(target, 128).map_err(|_| HostProblem::Malformed)?;
    match service.nested(
        run,
        HostRequest::Program(ProgramRequest::Inquire { program }),
    ) {
        Err(HostProblem::NotFound) => Err(HostProblem::Condition {
            name: "PGMIDERR".into(),
            response: 27,
            response2: 0,
        }),
        Err(problem) => Err(problem),
        Ok(HostResult::Program(_)) => service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        ),
        Ok(_) => Err(HostProblem::ProviderFailure),
    }
}

fn transfer(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let target = argument_text(request, "PROGRAM")?
        .trim()
        .to_ascii_uppercase();
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.PROGRAM.{target}"),
        AccessIntent::Execute,
    )?;
    let program = ProgramName::new(target.clone(), 128).map_err(|_| HostProblem::Malformed)?;
    let payload = bounded(argument_bytes(request, "COMMAREA").unwrap_or_default())?;
    if request.operation == CicsOperation::Xctl && service.lock()?.programs.contains(&target) {
        return service.response(
            run,
            CicsDisposition::Transfer,
            "NORMAL",
            0,
            0,
            Some(target),
            None,
            payload.bytes().to_vec(),
        );
    }
    let host_request = if request.operation == CicsOperation::Link {
        HostRequest::Program(ProgramRequest::Link { program, payload })
    } else {
        HostRequest::Program(ProgramRequest::Xctl { program, payload })
    };
    let payload = match service.nested(run, host_request)? {
        HostResult::Program(payload) => payload.bytes().to_vec(),
        _ => return Err(HostProblem::ProviderFailure),
    };
    let mut response = service.response(
        run,
        if request.operation == CicsOperation::Link {
            CicsDisposition::Complete
        } else {
            CicsDisposition::Transfer
        },
        "NORMAL",
        0,
        0,
        Some(target),
        None,
        payload.clone(),
    )?;
    if request.operation == CicsOperation::Link {
        response
            .outputs
            .insert("COMMAREA".into(), bounded(payload)?);
    }
    Ok(response)
}
