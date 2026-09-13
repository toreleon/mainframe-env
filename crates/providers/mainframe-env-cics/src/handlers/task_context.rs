use super::super::{CicsService, Run, bounded, decimal_payload};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};

pub(in crate::service) fn assign(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_assign_request(request)?;
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for (name, value) in [
        ("APPLID", run.applid.as_bytes()),
        ("SYSID", run.sysid.as_bytes()),
        ("USERID", run.invocation.principal.id().as_str().as_bytes()),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(value.to_vec())?);
        }
    }
    if request.arguments.contains_key("TASKPRIORITY") {
        response.outputs.insert(
            "TASKPRIORITY".into(),
            decimal_payload(i64::from(run.invocation.priority))?,
        );
    }
    Ok(response)
}

fn validate_assign_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "APPLID",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
        "SYSID",
        "TASKPRIORITY",
        "USERID",
    ];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || if name == "OPTION.NOHANDLE" {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else {
                value.schema() != "mainframe-env.cics.argument@1"
            }
    }) {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
