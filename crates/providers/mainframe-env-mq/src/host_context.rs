//! Host-attested MQ execution context for direct syncpoint calls.

use crate::retention::{CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_BINDING};
use mainframe_env_execution_api::{BoundedPayload, Invocation};
use mainframe_env_host_api::{
    HostProblem, MqContextDisposition, MqHostEnvironment, MqOperation, MqRequest, MqResult,
    MqSyncpointCall, MqSyncpointOwner, mq_syncpoint_context_disposition,
};

// The host supplies this versioned binding on the trusted Invocation, not on
// the application MQ request. Its payload is one exact environment/owner pair.
const MQ_HOST_CONTEXT_BINDING: &str = "mq.host-context";
const MQ_HOST_CONTEXT_SCHEMA: &str = "mainframe-env.mq.host-context@1";
const CICS_EXECUTION_CONTEXT_BINDING: &str = "cics.execution-context";
const CICS_EXECUTION_CONTEXT_SCHEMA: &str = "mainframe-env.cics.execution-context@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AttestedHostContext {
    environment: MqHostEnvironment,
    owner: MqSyncpointOwner,
}

impl AttestedHostContext {
    fn decode(binding: &BoundedPayload) -> Result<Self, HostProblem> {
        if binding.schema() != MQ_HOST_CONTEXT_SCHEMA {
            return Err(HostProblem::Malformed);
        }
        let (environment, owner) = match binding.bytes() {
            b"zos-batch|queue-manager" => {
                (MqHostEnvironment::ZosBatch, MqSyncpointOwner::QueueManager)
            }
            b"zos-batch|host-coordinator" => (
                MqHostEnvironment::ZosBatch,
                MqSyncpointOwner::HostCoordinator,
            ),
            b"zos-ims-batch-dli|queue-manager" => (
                MqHostEnvironment::ZosImsBatchDli,
                MqSyncpointOwner::QueueManager,
            ),
            b"zos-ims-batch-dli|host-coordinator" => (
                MqHostEnvironment::ZosImsBatchDli,
                MqSyncpointOwner::HostCoordinator,
            ),
            b"zos-cics|host-coordinator" => (
                MqHostEnvironment::ZosCics,
                MqSyncpointOwner::HostCoordinator,
            ),
            b"zos-ims|host-coordinator" => {
                (MqHostEnvironment::ZosIms, MqSyncpointOwner::HostCoordinator)
            }
            b"mqi-client|queue-manager" => {
                (MqHostEnvironment::MqiClient, MqSyncpointOwner::QueueManager)
            }
            b"mqi-client|host-coordinator" => (
                MqHostEnvironment::MqiClient,
                MqSyncpointOwner::HostCoordinator,
            ),
            b"other-bindings|queue-manager" => (
                MqHostEnvironment::OtherBindings,
                MqSyncpointOwner::QueueManager,
            ),
            b"other-bindings|host-coordinator" => (
                MqHostEnvironment::OtherBindings,
                MqSyncpointOwner::HostCoordinator,
            ),
            _ => return Err(HostProblem::Malformed),
        };
        Ok(Self { environment, owner })
    }
}

pub(crate) fn reject_host_owned_syncpoint(
    invocation: &Invocation,
    request: &MqRequest,
) -> Result<Option<MqResult>, HostProblem> {
    let context = invocation
        .bindings
        .get(MQ_HOST_CONTEXT_BINDING)
        .map(AttestedHostContext::decode)
        .transpose()?;
    let cics = invocation
        .bindings
        .get(CICS_EXECUTION_CONTEXT_BINDING)
        .map(|binding| {
            if binding.schema() != CICS_EXECUTION_CONTEXT_SCHEMA
                || !matches!(
                    binding.bytes(),
                    b"local"
                        | b"dpl-synconreturn"
                        | b"dpl-without-synconreturn"
                        | b"dpl-executionset-subset"
                )
            {
                Err(HostProblem::Malformed)
            } else {
                Ok(())
            }
        })
        .transpose()?
        .is_some();
    if cics
        && context.is_some_and(|value| {
            value.environment != MqHostEnvironment::ZosCics
                || value.owner != MqSyncpointOwner::HostCoordinator
        })
    {
        return Err(HostProblem::Malformed);
    }
    let nested = invocation
        .bindings
        .contains_key(CICS_NESTED_EFFECT_ORIGIN_BINDING);
    let outer = invocation
        .bindings
        .contains_key(CICS_OUTER_EFFECT_ORIGIN_BINDING);
    if nested != outer || (nested && !cics) {
        return Err(HostProblem::Malformed);
    }
    let call = match request.operation {
        MqOperation::Commit => MqSyncpointCall::Commit,
        MqOperation::Rollback => MqSyncpointCall::Back,
        _ => return Ok(None),
    };
    if nested {
        // The owning CICS coordinator dispatches this internal participant
        // action. Retention checks its exact nested and outer effect origins.
        return Ok(None);
    }
    let context = context
        .or_else(|| {
            cics.then_some(AttestedHostContext {
                environment: MqHostEnvironment::ZosCics,
                owner: MqSyncpointOwner::HostCoordinator,
            })
        })
        .ok_or(HostProblem::Malformed)?;
    match mq_syncpoint_context_disposition(call, context.environment, context.owner) {
        MqContextDisposition::Allowed => Ok(None),
        MqContextDisposition::Rejected {
            completion_code,
            reason_code,
        } => Ok(Some(MqResult {
            completion_code,
            reason_code,
            handle: None,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            trigger_program: None,
        })),
    }
}

#[cfg(test)]
pub(crate) const fn host_context_contract() -> (&'static str, &'static str) {
    (MQ_HOST_CONTEXT_BINDING, MQ_HOST_CONTEXT_SCHEMA)
}

#[cfg(test)]
pub(crate) const fn cics_execution_context_contract() -> (&'static str, &'static str) {
    (
        CICS_EXECUTION_CONTEXT_BINDING,
        CICS_EXECUTION_CONTEXT_SCHEMA,
    )
}
