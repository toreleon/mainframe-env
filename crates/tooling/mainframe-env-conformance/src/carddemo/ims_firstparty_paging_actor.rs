//! Paging-local actor/audit attribution; the original six-b71 helper stays byte-exact.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigestFormat, LifecycleEventKind};
use mainframe_env_store_api::{EffectRecord, EffectState, ExecutionState};

fn refusal(
    receipt: &mut serde_json::Value,
    phase: &'static str,
    details: serde_json::Value,
) -> CorpusProblem {
    receipt["paging_actor_failure"] = serde_json::json!({"phase": phase, "details": details});
    // Preserve the original check_actor error category and detail.
    CorpusProblem::new(
        "carddemo.ims.installed_firstparty_readonly_owner",
        "actual source, installed guest, independent bytes or owned teardown differs",
    )
}

pub(super) fn audit_matches(
    audits: &[AuditRecord],
    owner: &ExecutionRecord,
    run: &RunUnitId,
    sequence: u64,
    invocation: &IdempotencyKey,
    resource: &AuditResourceDigest,
    capability: &str,
) -> bool {
    let selected = audits
        .iter()
        .filter(|audit| {
            audit.effect_sequence == sequence
                && audit.capability.as_str() == capability
                && audit.resource == *resource
        })
        .collect::<Vec<_>>();
    matches!(selected.as_slice(), [audit] if audit.execution_id == owner.execution_id
        && audit.run_unit_id == *run && audit.principal == owner.principal
        && audit.attempt == owner.attempt && audit.invocation_key == *invocation
        && audit.resource == *resource && audit.decision == AuditDecision::Success)
}

pub(super) fn check_paging_actor(
    store: &dyn PlatformStore,
    effect: &EffectRecord,
    record: &ArtifactRecord,
    run: &RunUnitId,
    capability: &str,
    receipt: &mut serde_json::Value,
) -> Result<ExecutionRecord, CorpusProblem> {
    let owner = store
        .get_execution(&effect.execution_id)
        .map_err(|_| {
            refusal(
                receipt,
                "owner-read",
                serde_json::json!({"store_error": true}),
            )
        })?
        .ok_or_else(|| {
            refusal(
                receipt,
                "owner-present",
                serde_json::json!({"present": false}),
            )
        })?;
    let identity = owner.selector.as_str() == "program:COPAUS0C"
        && owner.artifact == record.artifact
        && owner.run_unit_id == *run
        && owner.principal.as_str() == "WEBADM"
        && owner.attempt > 0
        && owner.state == ExecutionState::Completed
        && owner.terminal_tick.is_some_and(|tick| tick > 0)
        && owner.owner_lease.is_none()
        && owner.lease_expiry_tick.is_none()
        && effect.execution_id == owner.execution_id
        && effect.run_unit_id == *run
        && effect.intent.owner == owner.execution_id
        && effect.intent.attempt == owner.attempt
        && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some(capability)
        && effect.state == EffectState::Completed
        && effect.result_digest.is_some();
    if !identity {
        return Err(refusal(
            receipt,
            "owner-effect-identity",
            serde_json::json!({
                "selector": owner.selector.as_str() == "program:COPAUS0C",
                "artifact": owner.artifact == record.artifact, "owner_run": owner.run_unit_id == *run,
                "principal": owner.principal.as_str() == "WEBADM", "positive_attempt": owner.attempt > 0,
                "completed": owner.state == ExecutionState::Completed,
                "positive_terminal_tick": owner.terminal_tick.is_some_and(|tick| tick > 0),
                "no_owner_lease": owner.owner_lease.is_none(), "no_lease_expiry": owner.lease_expiry_tick.is_none(),
                "effect_execution": effect.execution_id == owner.execution_id, "effect_run": effect.run_unit_id == *run,
                "intent_owner": effect.intent.owner == owner.execution_id, "intent_attempt": effect.intent.attempt == owner.attempt,
                "capability": effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some(capability),
                "effect_completed": effect.state == EffectState::Completed, "result_digest_present": effect.result_digest.is_some()
            }),
        ));
    }
    let events = store
        .events(&owner.execution_id, owner.version, 2)
        .map_err(|_| {
            refusal(
                receipt,
                "event-read",
                serde_json::json!({"store_error": true}),
            )
        })?;
    let terminal = matches!(events.as_slice(), [event] if event.validate()
        && event.execution_id == owner.execution_id && event.run_unit_id == *run
        && event.sequence == owner.version && event.attempt == owner.attempt
        && Some(event.tick) == owner.terminal_tick && event.tick > 0
        && matches!(event.kind, LifecycleEventKind::Completed { return_code: 0 }));
    if !terminal {
        let singleton = match events.as_slice() {
            [event] => Some(serde_json::json!({
                "valid": event.validate(), "execution": event.execution_id == owner.execution_id,
                "run": event.run_unit_id == *run, "version": event.sequence == owner.version,
                "attempt": event.attempt == owner.attempt, "terminal_tick": Some(event.tick) == owner.terminal_tick,
                "positive_tick": event.tick > 0, "completed_zero": matches!(event.kind, LifecycleEventKind::Completed { return_code: 0 })
            })),
            _ => None,
        };
        return Err(refusal(
            receipt,
            "terminal-event",
            serde_json::json!({
                "event_rows": events.len(), "singleton_predicates": singleton
            }),
        ));
    }
    let resource = effect.intent.audit_resource.as_ref().ok_or_else(|| {
        refusal(
            receipt,
            "audit-resource-present",
            serde_json::json!({"present": false}),
        )
    })?;
    let invocation = effect.intent.audit_invocation_key.as_ref().ok_or_else(|| {
        refusal(
            receipt,
            "audit-invocation-present",
            serde_json::json!({"present": false}),
        )
    })?;
    if resource.format != AuditResourceDigestFormat::CanonicalHostResourceV1 {
        return Err(refusal(
            receipt,
            "audit-resource-format",
            serde_json::json!({"canonical": false}),
        ));
    }
    let audits = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| {
            refusal(
                receipt,
                "audit-read",
                serde_json::json!({"store_error": true}),
            )
        })?;
    if audits.len() > MAX_ROWS {
        return Err(refusal(
            receipt,
            "audit-limit",
            serde_json::json!({"audit_rows": audits.len(), "maximum": MAX_ROWS}),
        ));
    }
    if !audit_matches(
        &audits,
        &owner,
        run,
        effect.sequence,
        invocation,
        resource,
        capability,
    ) {
        let coarse = audits
            .iter()
            .filter(|audit| {
                audit.effect_sequence == effect.sequence && audit.capability.as_str() == capability
            })
            .collect::<Vec<_>>();
        let exact = coarse
            .iter()
            .filter(|audit| audit.resource == *resource)
            .collect::<Vec<_>>();
        let singleton = match exact.as_slice() {
            [audit] => Some(serde_json::json!({
                "execution": audit.execution_id == owner.execution_id, "run": audit.run_unit_id == *run,
                "principal": audit.principal == owner.principal, "attempt": audit.attempt == owner.attempt,
                "invocation_key": audit.invocation_key == *invocation, "resource": audit.resource == *resource,
                "success": audit.decision == AuditDecision::Success
            })),
            _ => None,
        };
        return Err(refusal(
            receipt,
            "audit-match",
            serde_json::json!({
                "audit_rows": audits.len(), "sequence_capability_rows": coarse.len(),
                "canonical_resource_rows": exact.len(), "singleton_predicates": singleton
            }),
        ));
    }
    Ok(owner)
}
