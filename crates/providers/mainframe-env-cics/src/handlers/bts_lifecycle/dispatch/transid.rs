//! RUN TRANSID local child request, source conditions, and token response.

use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    if service.work_store.is_none() || service.replay_clock.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let transaction =
        argument_name(request, "TRANSID", 4).map_err(|_| condition("TRANSIDERR", 28, 1))?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let definition = authority
        .load_transaction(&transaction)?
        .ok_or_else(|| condition("TRANSIDERR", 28, 1))?;
    if definition.remote {
        return Err(condition("TRANSIDERR", 28, 11));
    }
    if !definition.enabled {
        return Err(condition("DISABLED", 84, 50));
    }
    let channel = request
        .arguments
        .get("CHANNEL")
        .map(|_| channel_name(request))
        .transpose()?;
    service
        .authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{transaction}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 101),
            other => other,
        })?;
    // The transform-container map is shared by channel name and cannot prove
    // task ownership. The sibling BTS channel authority must supply the
    // issue-time snapshot before this option can be admitted.
    if channel.is_some() {
        return Err(HostProblem::Unsupported);
    }
    let containers = BTreeMap::new();
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let tick = service
        .replay_clock
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?
        .now_tick()?;
    if tick == 0 {
        return Err(HostProblem::UnknownOutcome);
    }
    let record = authority.start_transid(
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
        mutation.idempotency_key.as_str(),
        digest,
        &transaction,
        &definition.program,
        channel.as_deref(),
        containers,
        tick,
        run.invocation.priority,
    )?;
    service
        .register_bts_transid_child_inflight(run, &record)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    service
        .enqueue_bts_transid_work(&record)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    response(
        service,
        run,
        BTreeMap::from([(
            "CHILD".into(),
            ("mainframe-env.cics.payload@1", record.token.to_vec()),
        )]),
    )
}

fn channel_name(request: &CicsRequest) -> Result<String, HostProblem> {
    let payload = request
        .arguments
        .get("CHANNEL")
        .ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(payload.bytes()).map_err(|_| condition("CHANNELERR", 122, 1))?;
    let text = text.trim_end_matches(' ');
    if text.is_empty()
        || text.chars().count() > 16
        || text.chars().any(|character| {
            !(character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '$' | '@'
                        | '#'
                        | '.'
                        | '/'
                        | '-'
                        | '_'
                        | '%'
                        | '&'
                        | '?'
                        | '!'
                        | ':'
                        | '|'
                        | '"'
                        | '='
                        | '¬'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                ))
        })
    {
        return Err(condition("CHANNELERR", 122, 1));
    }
    Ok(text.into())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::RunTransId
        || !request.arguments.contains_key("TRANSID")
        || !request.arguments.contains_key("CHILD")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        match name.as_str() {
            "TRANSID" | "CHANNEL"
                if matches!(
                    value.schema(),
                    "mainframe-env.cics.argument@1"
                        | "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                ) => {}
            "CHILD" | "RESP" | "RESP2" if value.schema() == "mainframe-env.cics.argument@1" => {}
            "OPTION.NOHANDLE"
                if value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty() => {}
            _ => return Err(HostProblem::Malformed),
        }
    }
    Ok(())
}
