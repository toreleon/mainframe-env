//! Versioned at-most-once dispatch for installed COBOL calls. Pending != retryable.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const NAMESPACE: &str = "cobol-call-replay@1";
const RUN_NAMESPACE: &str = "cobol-call-protocol@1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    fingerprint: String,
    child_execution: String,
    reply: Option<Reply>,
}

pub(super) fn digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.installed-call@1\0");
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

fn identity(parent: &Invocation, effect: &EffectRequest) -> Result<String, HostProblem> {
    let key = effect
        .idempotency_key
        .as_ref()
        .ok_or(HostProblem::Malformed)?;
    if effect.run_unit != parent.run_unit_id {
        return Err(HostProblem::Malformed);
    }
    // A retry/replay is the SAME logical call; a new occurrence needs a new key.
    Ok(digest(&[
        b"identity",
        parent.run_unit_id.as_str().as_bytes(),
        parent.execution_id.as_str().as_bytes(),
        key.as_str().as_bytes(),
    ]))
}

fn fingerprint(
    parent: &Invocation,
    effect: &EffectRequest,
    program: &str,
    payload: &BoundedPayload,
) -> Result<String, HostProblem> {
    let grants: Vec<_> = parent
        .principal
        .grants()
        .iter()
        .map(|v| v.as_str())
        .collect();
    let generations: Vec<_> = parent
        .provider_generations
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let bindings: Vec<_> = parent
        .bindings
        .iter()
        .map(|(k, v)| (k.as_str(), v.schema(), v.bytes()))
        .collect();
    let bytes = serde_json::to_vec(&(
        1_u32,
        parent.principal.id().as_str(),
        grants,
        parent.artifact.as_str(),
        bindings,
        generations,
        effect.sequence,
        program.to_ascii_uppercase(),
        payload.schema(),
        payload.bytes(),
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(digest(&[b"fingerprint", &bytes]))
}

fn previous(record: ProviderStateRecord, expected: &str) -> Result<BoundedPayload, HostProblem> {
    let receipt: Receipt =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::UnknownOutcome)?;
    if receipt.schema_version != 1 {
        return Err(HostProblem::UnknownOutcome);
    }
    if receipt.fingerprint != expected {
        return Err(HostProblem::IdempotencyConflict);
    }
    match (record.version, receipt.reply) {
        (2, Some(reply)) => {
            BoundedPayload::new(reply.schema, reply.bytes, InvocationLimits::default())
                .map_err(|_| HostProblem::UnknownOutcome)
        }
        _ => Err(HostProblem::UnknownOutcome),
    }
}

impl CobolProgram {
    fn ensure_call_protocol(&self, parent: &Invocation) -> Result<(), HostProblem> {
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let key = digest(&[b"run-protocol", parent.run_unit_id.as_str().as_bytes()]);
        match store
            .get_provider_state(RUN_NAMESPACE, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            Some(record) if record.version == 1 && record.payload == b"installed-call@1" => Ok(()),
            Some(_) => Err(HostProblem::UnknownOutcome),
            None if parent.attempt != 1 => Err(HostProblem::UnknownOutcome),
            None => {
                let record = ProviderStateRecord {
                    namespace: RUN_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    payload: b"installed-call@1".to_vec(),
                };
                match store.put_provider_state(record, None) {
                    Ok(()) => Ok(()),
                    Err(
                        mainframe_env_store_api::StoreError::Conflict
                        | mainframe_env_store_api::StoreError::AlreadyExists,
                    ) => match store.get_provider_state(RUN_NAMESPACE, &key) {
                        Ok(Some(record))
                            if record.version == 1 && record.payload == b"installed-call@1" =>
                        {
                            Ok(())
                        }
                        _ => Err(HostProblem::UnknownOutcome),
                    },
                    Err(_) => Err(HostProblem::InfrastructureFailure),
                }
            }
        }
    }

    pub(super) fn execute_installed_effect(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
        program: &str,
        payload: &BoundedPayload,
    ) -> Result<BoundedPayload, HostProblem> {
        if !matches!(
            payload.schema(),
            "mainframe-env.cobol.call@1" | "mainframe-env.program.input@1"
        ) {
            return Err(HostProblem::Malformed);
        }
        self.ensure_call_protocol(parent)?;
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let key = identity(parent, effect)?;
        let fingerprint = fingerprint(parent, effect, program, payload)?;
        if let Some(record) = store
            .get_provider_state(NAMESPACE, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            return previous(record, &fingerprint);
        }
        let prefix = if payload.schema() == "mainframe-env.cobol.call@1" {
            "online-call-execution"
        } else {
            "batch-installed-execution"
        };
        let mut receipt = Receipt {
            schema_version: 1,
            fingerprint,
            child_execution: format!("{prefix}-{key}"),
            reply: None,
        };
        let pending = ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: key.clone(),
            version: 1,
            payload: serde_json::to_vec(&receipt)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        };
        if let Err(problem) = store.put_provider_state(pending, None) {
            if matches!(
                problem,
                mainframe_env_store_api::StoreError::Conflict
                    | mainframe_env_store_api::StoreError::AlreadyExists
            ) {
                return match store.get_provider_state(NAMESPACE, &key) {
                    Ok(Some(record)) => previous(record, &receipt.fingerprint),
                    _ => Err(HostProblem::UnknownOutcome),
                };
            }
            return Err(HostProblem::InfrastructureFailure);
        }
        // No call can dispatch without winning the durable reservation.
        let result = if payload.schema() == "mainframe-env.cobol.call@1" {
            self.execute_installed(parent, program, payload, &key)
        } else {
            self.execute_installed_batch(parent, program, payload, &key)
                .and_then(|output| {
                    serde_json::to_vec(&output).map_err(|_| HostProblem::ProviderFailure)
                })
                .and_then(|bytes| {
                    BoundedPayload::new(
                        "mainframe-env.program.output@1",
                        bytes,
                        InvocationLimits::default(),
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)
                })
        }?;
        receipt.reply = Some(Reply {
            schema: result.schema().into(),
            bytes: result.bytes().to_vec(),
        });
        let payload = serde_json::to_vec(&receipt).map_err(|_| HostProblem::UnknownOutcome)?;
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key,
                    version: 2,
                    payload,
                },
                Some(1),
            )
            .map_err(|_| HostProblem::UnknownOutcome)?;
        Ok(result)
    }
}
