//! RUN TRANSID local child request, source conditions, and token response.

use super::*;
use crate::service::handlers::bts_container::ContainerDatatype;
use crate::service::handlers::bts_container::ContainerReadError;
use crate::service::handlers::bts_container::{ContainerSelector, ReadReply, ReadRequest};
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
    let containers = if let Some(channel) = channel.as_deref() {
        channel_snapshot(service, run, channel)?
    } else {
        BTreeMap::new()
    };
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
    let record = authority.start_transid_with_channel_access(
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
        mutation.idempotency_key.as_str(),
        digest,
        &transaction,
        &definition.program,
        channel.as_deref(),
        false,
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

fn channel_snapshot(
    service: &CicsService,
    run: &mut Run,
    channel: &str,
) -> Result<BTreeMap<String, BtsTransidContainer>, HostProblem> {
    let before = service
        .store
        .get_provider_state("cics-container-capacity-v1", "global")
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let names = match service.read_bts_container(
        run,
        ContainerSelector::Channel(channel),
        ReadRequest::Names { max: 256 },
    ) {
        Ok(ReadReply::Names(names)) => names,
        Ok(_) => return Err(HostProblem::InfrastructureFailure),
        Err(error) => return Err(channel_read_problem(error)),
    };
    let mut containers = BTreeMap::new();
    for name in names {
        let value = match service.read_bts_container(
            run,
            ContainerSelector::Channel(channel),
            ReadRequest::Value(&name),
        ) {
            Ok(ReadReply::Value(Some(value))) => value,
            Ok(ReadReply::Value(None)) => return Err(HostProblem::UnknownOutcome),
            Ok(_) => return Err(HostProblem::InfrastructureFailure),
            Err(error) => return Err(channel_read_problem(error)),
        };
        containers.insert(
            name,
            BtsTransidContainer {
                character: value.datatype == ContainerDatatype::Character,
                ccsid: value.ccsid,
                read_only: value.read_only,
                bytes: value.bytes,
            },
        );
    }
    let after = service
        .store
        .get_provider_state("cics-container-capacity-v1", "global")
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if before != after {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(containers)
}

fn channel_read_problem(error: ContainerReadError) -> HostProblem {
    match error {
        ContainerReadError::NotFound => condition("CHANNELERR", 122, 2),
        ContainerReadError::Unauthorized => condition("CHANNELERR", 122, 6),
        ContainerReadError::Bounds => condition("CHANNELERR", 122, 1),
        ContainerReadError::Changed => HostProblem::UnknownOutcome,
        ContainerReadError::StaleEpoch => condition("INVREQ", 16, 1),
        ContainerReadError::Backend(problem) => problem,
    }
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
