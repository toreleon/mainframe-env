//! Versioned terminal subjects under the existing audit namespace and ordering.
use super::*;
pub(crate) mod routing;
use mainframe_env_execution_api::{
    RootTerminalAudit, RootTerminalAuditRole, RootTerminalResourceDigest,
};

pub(crate) fn encode_terminal_audit(record: &RootTerminalAudit) -> Result<Vec<u8>, StoreError> {
    crate::root_terminal::validate_audit(record)?;
    encode(json!({ "schema": "mainframe-env.root-terminal-audit@1",
        "role": match record.role { RootTerminalAuditRole::ProviderSettlement => "provider-settlement", RootTerminalAuditRole::CoreClosure => "core-closure" },
        "execution": record.execution_id.as_str(), "run": record.run_unit_id.as_str(),
        "attempt": record.attempt, "lifecycle_sequence": record.lifecycle_sequence,
        "tick": record.observed_tick, "principal": record.principal.as_str(),
        "invocation_key": record.invocation_key.as_str(), "capability": record.capability.as_str(),
        "resource_digest": hex(&record.resource.value), "decision": audit_decision(record.decision) }))
}

pub(crate) fn decode_terminal_audit(bytes: &[u8]) -> Result<RootTerminalAudit, StoreError> {
    let value = decode(bytes)?;
    if string(&value, "schema")? != "mainframe-env.root-terminal-audit@1" {
        return Err(StoreError::IncompatibleVersion);
    }
    let limits = InvocationLimits::default();
    let record = RootTerminalAudit {
        role: match string(&value, "role")? {
            "provider-settlement" => RootTerminalAuditRole::ProviderSettlement,
            "core-closure" => RootTerminalAuditRole::CoreClosure,
            _ => return Err(StoreError::IncompatibleVersion),
        },
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        run_unit_id: RunUnitId::new(string(&value, "run")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        principal: PrincipalId::new(string(&value, "principal")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        invocation_key: IdempotencyKey::new(string(&value, "invocation_key")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        capability: CapabilityId::new(string(&value, "capability")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        attempt: u32::try_from(number(&value, "attempt")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        lifecycle_sequence: number(&value, "lifecycle_sequence")?,
        observed_tick: number(&value, "tick")?,
        resource: RootTerminalResourceDigest {
            value: digest_back(string(&value, "resource_digest")?)?,
        },
        decision: audit_decision_back(string(&value, "decision")?)?,
    };
    crate::root_terminal::validate_audit(&record)?;
    // Full canonical storage equality also rejects unknown/duplicate fields.
    if encode_terminal_audit(&record)? != bytes {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(record)
}
