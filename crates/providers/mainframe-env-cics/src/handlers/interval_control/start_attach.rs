//! Immediate, noncancelable no-data START ATTACH route.

use super::{CicsService, Run, start};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    start(service, run, request, true)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::StartAttach
        || !request.arguments.contains_key("TRANSID")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    // The source passes a live address, not a copied record. The shared arena
    // must own this path before data-bearing forms become executable.
    if request.arguments.contains_key("FROM") || request.arguments.contains_key("LENGTH") {
        return Err(HostProblem::Unsupported);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "TRANSID" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
                    | "mainframe-env.cics.argument@1"
            ),
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
