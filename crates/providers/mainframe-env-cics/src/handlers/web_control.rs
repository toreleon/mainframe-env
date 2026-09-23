use super::super::{CicsService, Run};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};

mod parse_url;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::WebParseUrl => parse_url::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}
