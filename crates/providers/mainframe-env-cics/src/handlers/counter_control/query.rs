use super::{CounterReply, authorize, invreq, response, selector, state, unavailable};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let selector_name = match request.operation {
        CicsOperation::QueryCounter => "COUNTER",
        CicsOperation::QueryDCounter => "DCOUNTER",
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if request.mutation.is_some()
        || !request.arguments.contains_key(selector_name)
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
                    | "VALUE"
                    | "MINIMUM"
                    | "MAXIMUM"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOSUSPEND"
                    | "OPTION.NOHANDLE"
            ) || name
                == if selector_name == "COUNTER" {
                    "DCOUNTER"
                } else {
                    "COUNTER"
                }
                || matches!(name.as_str(), "VALUE" | "MINIMUM" | "MAXIMUM")
                    && !request.arguments[name].bytes().is_empty()
        })
    {
        return Err(HostProblem::Malformed);
    }
    let (pool, key) = selector(request)?;
    authorize(service, run, &key, AccessIntent::Read)?;
    let current = state::load(service.store.as_ref(), service.limits)?;
    if current.rebuilding.contains(&pool) {
        return unavailable(service, run, request);
    }
    let record = current
        .records
        .get(key.as_str())
        .ok_or_else(|| invreq(201))?;
    let mut reply = CounterReply::normal();
    if request.arguments.contains_key("VALUE") {
        reply.value = Some(record.current_value());
    }
    if request.arguments.contains_key("MINIMUM") {
        reply.minimum = Some(record.minimum);
    }
    if request.arguments.contains_key("MAXIMUM") {
        reply.maximum = Some(record.maximum);
    }
    if request.operation == CicsOperation::QueryCounter {
        let warning = [
            if record.at_limit {
                None
            } else {
                reply.value.and_then(overflow)
            },
            reply.minimum.and_then(overflow),
            reply.maximum.and_then(overflow),
        ]
        .into_iter()
        .flatten()
        .max();
        if let Some(response2) = warning {
            reply.condition = "LENGERR".into();
            reply.response = 22;
            reply.response2 = response2;
        }
    }
    response(service, run, request, &reply)
}

fn overflow(value: u64) -> Option<i32> {
    (value > i32::MAX as u64).then(|| {
        if value <= u32::MAX as u64 {
            1
        } else if value < (1u64 << 33) {
            2
        } else {
            3
        }
    })
}
