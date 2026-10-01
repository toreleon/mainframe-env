use super::{response, state};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{
    AccessIntent, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    let raw_identifier = request
        .arguments
        .get("TRACEID")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let identifier = if raw_identifier
        .iter()
        .all(|byte| (0x20..=0x7e).contains(byte))
    {
        String::from_utf8(raw_identifier.to_vec()).map_err(|_| HostProblem::Malformed)?
    } else {
        let mut value = String::from("0x");
        for byte in raw_identifier {
            value.push_str(&format!("{byte:02X}"));
        }
        value
    };
    let data = request
        .arguments
        .get("FROM")
        .map(|value| value.bytes().to_vec())
        .unwrap_or_else(|| vec![0; 8]);
    if data.len() > service.limits.max_diagnostic_payload_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let resource = text(request, "RESOURCE", "")?;
    let entry_name = text(request, "ENTRYNAME", "USER")?
        .trim_end()
        .to_ascii_uppercase();
    let account = request.arguments.contains_key("OPTION.ACCOUNT");
    let monitor = request.arguments.contains_key("OPTION.MONITOR");
    let perform = request.arguments.contains_key("OPTION.PERFORM");
    service.authorize(run, "CICSDIAG", "CICS.DIAG.TRACEID", AccessIntent::Update)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let _guard = service.lock()?;
    let current = state::load(service)?;
    if let Some(reply) = current.replay(mutation.idempotency_key.as_str(), digest)? {
        return response(service, run, reply);
    }
    if !current.configuration.user_trace && !current.single_trace {
        return Err(condition(3));
    }
    if !(current.configuration.internal
        || current.configuration.auxiliary
        || current.configuration.system
        || current.single_trace)
    {
        return Err(condition(2));
    }
    if current.traces.len() + current.dumps.len() >= service.limits.max_diagnostic_entries {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut destinations = Vec::new();
    if current.configuration.internal || current.single_trace {
        destinations.push("INTERNAL");
    }
    if current.configuration.auxiliary {
        destinations.push("AUXILIARY");
    }
    if current.configuration.system {
        destinations.push("SYSTEM");
    }
    let flags = [
        ("ACCOUNT", account),
        ("MONITOR", monitor),
        ("PERFORM", perform),
    ]
    .into_iter()
    .filter_map(|(name, enabled)| enabled.then_some(name))
    .collect::<Vec<_>>()
    .join("+");
    let mut next = current.clone();
    let sequence = next.allocate_sequence()?;
    next.traces.push(state::CicsDiagnosticTraceRecord {
        sequence,
        kind: format!(
            "TRACEID:{}:ENTRY={entry_name}:FLAGS={flags}",
            destinations.join("+")
        ),
        identifier: identifier.clone(),
        resource,
        data: data.clone(),
        exception: false,
        run_unit: run.invocation.run_unit_id.as_str().into(),
        principal: run.invocation.principal.id().as_str().into(),
    });
    if current.single_trace {
        next.single_trace = false;
    }
    if monitor {
        if data.len() > 8192 {
            return Err(HostProblem::ResourceExhausted);
        }
        next.monitor_text
            .insert(format!("TRACEID:{entry_name}:{identifier}"), data);
    }
    for (enabled, kind) in [(account, "ACCOUNT"), (perform, "PERFORM")] {
        if enabled {
            let counter = next
                .monitor_counters
                .entry(format!("TRACEID:{kind}:{entry_name}"))
                .or_default();
            *counter = counter
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
        }
    }
    let reply = state::DiagnosticReply::normal();
    next.record_replay(
        mutation.idempotency_key.as_str(),
        digest,
        reply.clone(),
        service.limits,
    )?;
    state::persist(service, current.version, &mut next)?;
    response(service, run, reply)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    if request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "TRACEID" => {
                !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.decimal@1"
                ) || !(1..=8).contains(&value.bytes().len())
            }
            "FROM" => value.schema() != "mainframe-env.cics.storage-value@1",
            "RESOURCE" | "ENTRYNAME" => {
                !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ) || value.bytes().len() != 8
            }
            "OPTION.ACCOUNT" | "OPTION.MONITOR" | "OPTION.PERFORM" | "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            _ => true,
        })
        || !request.arguments.contains_key("TRACEID")
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn text(request: &CicsRequest, name: &str, default: &str) -> Result<String, HostProblem> {
    match request.arguments.get(name) {
        Some(value) => {
            String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed)
        }
        None => Ok(default.into()),
    }
}

fn condition(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}
