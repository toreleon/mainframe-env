use super::super::{CicsService, Run};
use mainframe_env_host_api::{CicsConditionPolicy, CicsDisposition, CicsResponse, HostProblem};

pub(crate) fn respond(
    service: &CicsService,
    run: &Run,
    policy: &CicsConditionPolicy,
    problem: HostProblem,
) -> Result<CicsResponse, HostProblem> {
    if matches!(
        problem,
        HostProblem::UnknownOutcome
            | HostProblem::InfrastructureFailure
            | HostProblem::ProviderFailure
            | HostProblem::TimedOut
            | HostProblem::Cancelled
    ) {
        return Err(problem);
    }
    let (name, response, response2) = condition_for(&problem);
    match policy {
        CicsConditionPolicy::NoHandle | CicsConditionPolicy::Respond { .. } => service.response(
            run,
            CicsDisposition::Complete,
            name,
            response,
            response2,
            None,
            None,
            Vec::new(),
        ),
        CicsConditionPolicy::Default => {
            if run.ignored_conditions.contains(name) {
                service.response(
                    run,
                    CicsDisposition::Ignored,
                    name,
                    response,
                    response2,
                    None,
                    None,
                    Vec::new(),
                )
            } else if let Some(target) = run.handlers.get(name) {
                service.response(
                    run,
                    CicsDisposition::Handler,
                    name,
                    response,
                    response2,
                    Some(target.clone()),
                    None,
                    Vec::new(),
                )
            } else if name == "ENQBUSY" || run.ignored_conditions.contains("ERROR") {
                service.response(
                    run,
                    CicsDisposition::Ignored,
                    name,
                    response,
                    response2,
                    None,
                    None,
                    Vec::new(),
                )
            } else if let Some(target) = run.handlers.get("ERROR") {
                service.response(
                    run,
                    CicsDisposition::Handler,
                    name,
                    response,
                    response2,
                    Some(target.clone()),
                    None,
                    Vec::new(),
                )
            } else {
                Err(problem)
            }
        }
    }
}

fn condition_for(problem: &HostProblem) -> (&'static str, i32, i32) {
    match problem {
        HostProblem::NotFound => ("NOTFND", 13, 0),
        HostProblem::Unauthorized => ("NOTAUTH", 70, 0),
        HostProblem::ResourceExhausted => ("ERROR", 1, 101),
        HostProblem::Condition {
            name,
            response,
            response2,
        } => (condition_name(name), *response, *response2),
        _ => ("ERROR", 1, 0),
    }
}

fn condition_name(name: &str) -> &'static str {
    match name {
        "DUPREC" => "DUPREC",
        "INVREQ" => "INVREQ",
        "LENGERR" => "LENGERR",
        "ENDFILE" => "ENDFILE",
        "ENQBUSY" => "ENQBUSY",
        "PGMIDERR" => "PGMIDERR",
        "NOTAUTH" => "NOTAUTH",
        "NOTFND" => "NOTFND",
        "NOTOPEN" => "NOTOPEN",
        "IOERR" => "IOERR",
        "LOCKED" => "LOCKED",
        "RECORDBUSY" => "RECORDBUSY",
        "ROLLEDBACK" => "ROLLEDBACK",
        _ => "ERROR",
    }
}
