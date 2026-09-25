use super::{
    MAX_CAS_RETRIES, authorize, invreq, replay_identity, response, selector, state, unavailable,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let selector_name = match request.operation {
        CicsOperation::DeleteCounter => "COUNTER",
        CicsOperation::DeleteDCounter => "DCOUNTER",
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !request.arguments.contains_key(selector_name)
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
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
        })
    {
        return Err(HostProblem::Malformed);
    }
    let (pool, key) = selector(request)?;
    authorize(service, run, &key, AccessIntent::Update)?;
    let (effect_key, replay) = replay_identity(run, request)?;
    for _ in 0..MAX_CAS_RETRIES {
        let current = state::load(service.store.as_ref(), service.limits)?;
        if let Some(saved) = current.replay(
            &effect_key,
            &replay.owner_execution,
            &replay.owner_run_unit,
            &replay.owner_principal,
            replay.request_digest,
        )? {
            return response(service, run, request, &saved);
        }
        if current.rebuilding.contains(&pool) {
            return unavailable(service, run, request);
        }
        if !current.records.contains_key(key.as_str()) {
            return Err(invreq(201));
        }
        let mut next = current.clone();
        next.records.remove(key.as_str());
        next.record_replay(effect_key.clone(), replay.clone(), service.limits)?;
        if state::persist(service, &current, &mut next)? {
            return response(service, run, request, &replay.reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}
