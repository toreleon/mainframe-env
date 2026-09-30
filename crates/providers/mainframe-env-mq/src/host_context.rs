use crate::retention::{CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_BINDING};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::{
    HostProblem, MqContextDisposition, MqHostEnvironment, MqOperation, MqRequest, MqResult,
    MqSyncpointCall, MqSyncpointOwner, mq_syncpoint_context_disposition,
};

const CICS_EXECUTION_CONTEXT_BINDING: &str = "cics.execution-context";
const CICS_EXECUTION_CONTEXT_SCHEMA: &str = "mainframe-env.cics.execution-context@1";

pub(crate) fn reject_host_owned_syncpoint(
    invocation: &Invocation,
    request: &MqRequest,
) -> Result<Option<MqResult>, HostProblem> {
    if !matches!(
        request.operation,
        MqOperation::Commit | MqOperation::Rollback
    ) {
        return Ok(None);
    }
    let Some(context) = invocation.bindings.get(CICS_EXECUTION_CONTEXT_BINDING) else {
        return Ok(None);
    };
    if context.schema() != CICS_EXECUTION_CONTEXT_SCHEMA
        || !matches!(
            context.bytes(),
            b"local"
                | b"dpl-synconreturn"
                | b"dpl-without-synconreturn"
                | b"dpl-executionset-subset"
        )
    {
        return Err(HostProblem::Malformed);
    }
    let nested = invocation
        .bindings
        .contains_key(CICS_NESTED_EFFECT_ORIGIN_BINDING);
    let outer = invocation
        .bindings
        .contains_key(CICS_OUTER_EFFECT_ORIGIN_BINDING);
    if nested != outer {
        return Err(HostProblem::Malformed);
    }
    if nested {
        // CICS owns the application syncpoint and dispatches this internal MQ
        // participant operation. Retention validates the exact nested/outer
        // provenance before any result is persisted.
        Ok(None)
    } else {
        let call = match request.operation {
            MqOperation::Commit => MqSyncpointCall::Commit,
            MqOperation::Rollback => MqSyncpointCall::Back,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        match mq_syncpoint_context_disposition(
            call,
            MqHostEnvironment::ZosCics,
            MqSyncpointOwner::HostCoordinator,
        ) {
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
            MqContextDisposition::Allowed => Err(HostProblem::InfrastructureFailure),
        }
    }
}

#[cfg(test)]
pub(crate) const fn cics_execution_context_contract() -> (&'static str, &'static str) {
    (
        CICS_EXECUTION_CONTEXT_BINDING,
        CICS_EXECUTION_CONTEXT_SCHEMA,
    )
}
