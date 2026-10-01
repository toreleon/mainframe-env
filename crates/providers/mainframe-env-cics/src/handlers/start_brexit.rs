//! Local START BREXIT admission through the durable bridge work authority.

use super::super::{CicsService, Run};
use super::interval_control::{
    name_argument, optional_decimal, optional_name_argument, validate_start_principal,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let transaction = name_argument(request, "TRANSID", 4)?;
    let explicit_exit = optional_name_argument(request, "BREXIT", 8)?;
    let user = optional_name_argument(request, "USERID", 8)?;
    let length = optional_decimal(request, "BRDATALENGTH")?
        .map(|value| usize::try_from(value).map_err(|_| length_error()))
        .transpose()?;
    let data = request.arguments.get("BRDATA").map(|value| value.bytes());
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;

    service
        .authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{transaction}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 7,
            },
            other => other,
        })?;
    if let Some(user) = user.as_deref() {
        validate_start_principal(service, run, user)?;
        service
            .authorize(
                run,
                "SURROGAT",
                &format!("{user}.DFHSTART"),
                AccessIntent::Read,
            )
            .map_err(|problem| match problem {
                HostProblem::Unauthorized => HostProblem::Condition {
                    name: "NOTAUTH".into(),
                    response: 70,
                    response2: 9,
                },
                other => other,
            })?;
    }
    service.schedule_bridge_start(
        &transaction,
        &run.transaction,
        explicit_exit.as_deref(),
        run.invocation.principal.id().as_str(),
        user.as_deref(),
        data,
        length,
        mutation.idempotency_key.as_str(),
        digest,
        run.invocation.priority,
    )?;
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

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::StartBrexit
        || !request.arguments.contains_key("TRANSID")
        || request.arguments.contains_key("BRDATA")
            != request.arguments.contains_key("BRDATALENGTH")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "TRANSID" | "BREXIT" | "USERID" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
                    | "mainframe-env.cics.argument@1"
            ),
            "BRDATA" => matches!(
                value.schema(),
                "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.argument@1"
            ),
            "BRDATALENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
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

fn length_error() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 0,
    }
}
