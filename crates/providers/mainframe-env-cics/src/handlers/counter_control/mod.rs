//! Durable local authority for IBM CICS named-counter command shapes.

mod define;
mod state;

use super::super::{CicsService, Run};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use state::{CounterKind, CounterRecord, CounterReplay, CounterReply};

const MAX_CAS_RETRIES: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
struct CounterKey(String);

impl CounterKey {
    pub fn new(pool: &str, name: &str) -> Self {
        Self(format!("{pool}/{name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::DefineCounter | CicsOperation::DefineDCounter => {
            define::invoke(service, run, request)
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

/// Mark a local named-counter pool unavailable during a modeled rebuild.
/// The flag is versioned with the counters so concurrent workers see the same state.
impl CicsService {
    pub fn set_counter_pool_rebuilding(
        &self,
        pool: &str,
        rebuilding: bool,
    ) -> Result<(), HostProblem> {
        let pool = normalize_pool(pool.as_bytes())?;
        for _ in 0..MAX_CAS_RETRIES {
            let current = state::load(self.store.as_ref(), self.limits)?;
            if current.rebuilding.contains(&pool) == rebuilding {
                return Ok(());
            }
            let mut next = current.clone();
            if rebuilding {
                next.rebuilding.insert(pool.clone());
            } else {
                next.rebuilding.remove(&pool);
            }
            if state::persist(self, &current, &mut next)? {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

fn normalize_name(bytes: &[u8]) -> Result<String, HostProblem> {
    let trimmed = bytes.trim_ascii_end();
    if bytes.is_empty()
        || bytes.len() > 16
        || trimmed.is_empty()
        || matches!(trimmed[0], b'0'..=b'9' | b'_')
        || !trimmed.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"_$#@".contains(byte)
        })
    {
        return Err(invreq(404));
    }
    String::from_utf8(trimmed.to_vec()).map_err(|_| invreq(404))
}

fn normalize_pool(bytes: &[u8]) -> Result<String, HostProblem> {
    let trimmed = bytes.trim_ascii_end();
    if bytes.len() > 8
        || !trimmed.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"_$#@".contains(byte)
        })
    {
        return Err(invreq(403));
    }
    if trimmed.is_empty() {
        Ok("DEFAULT".into())
    } else {
        String::from_utf8(trimmed.to_vec()).map_err(|_| invreq(403))
    }
}

fn selector(request: &CicsRequest) -> Result<(String, CounterKey), HostProblem> {
    let field = if matches!(request.operation, CicsOperation::DefineDCounter) {
        "DCOUNTER"
    } else {
        "COUNTER"
    };
    let name = normalize_name(
        request
            .arguments
            .get(field)
            .ok_or(HostProblem::Malformed)?
            .bytes(),
    )?;
    let pool = request
        .arguments
        .get("POOL")
        .map(|value| normalize_pool(value.bytes()))
        .transpose()?
        .unwrap_or_else(|| "DEFAULT".into());
    let key = CounterKey::new(&pool, &name);
    Ok((pool, key))
}

fn authorize(
    service: &CicsService,
    run: &mut Run,
    key: &CounterKey,
    intent: AccessIntent,
) -> Result<(), HostProblem> {
    service.authorize(
        run,
        "COUNTER",
        &format!("CICS.COUNTER.{}", key.as_str().replace('/', ".")),
        intent,
    )
}

fn unavailable(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.contains_key("OPTION.NOSUSPEND") {
        Err(HostProblem::Condition {
            name: "BUSY".into(),
            response: 128,
            response2: 500,
        })
    } else {
        service.response(
            run,
            CicsDisposition::Suspended,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )
    }
}

fn invreq(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}

fn replay_identity(
    run: &Run,
    request: &CicsRequest,
) -> Result<(String, CounterReplay), HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    Ok((
        mutation.idempotency_key.as_str().into(),
        CounterReplay {
            owner_execution: run.invocation.execution_id.as_str().into(),
            owner_run_unit: run.invocation.run_unit_id.as_str().into(),
            owner_principal: run.invocation.principal.id().as_str().into(),
            request_digest: digest,
            reply: CounterReply::normal(),
        },
    ))
}

fn response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    reply: &CounterReply,
) -> Result<CicsResponse, HostProblem> {
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        &reply.condition,
        reply.response,
        reply.response2,
        None,
        None,
        Vec::new(),
    )?;
    for (name, value) in [
        ("VALUE", reply.value),
        ("MINIMUM", reply.minimum),
        ("MAXIMUM", reply.maximum),
    ] {
        if request.arguments.contains_key(name)
            && let Some(value) = value
        {
            let signed = if matches!(request.operation, CicsOperation::DefineCounter) {
                i128::from(value as u32 as i32)
            } else {
                i128::from(value)
            };
            result.outputs.insert(name.into(), decimal_payload(signed)?);
        }
    }
    Ok(result)
}

fn decimal_payload(value: i128) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(
        "mainframe-env.cics.decimal@1",
        value.to_string().into_bytes(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}
