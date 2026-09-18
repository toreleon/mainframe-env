use super::optional_decimal;
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &["INTERVAL", "OPTION.NOHANDLE", "RESP", "RESP2"];
    if request.arguments.iter().any(|(name, value)| {
        !ALLOWED.contains(&name.as_str())
            || if name == "INTERVAL" {
                value.schema() != "mainframe-env.cics.decimal@1"
            } else if name == "OPTION.NOHANDLE" {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else {
                value.schema() != "mainframe-env.cics.argument@1"
            }
    }) {
        return Err(HostProblem::Malformed);
    }
    if optional_decimal(request, "INTERVAL")?.is_some_and(|value| value != 0) {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        });
    }
    Ok(())
}
