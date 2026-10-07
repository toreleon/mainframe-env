//! Completed refusal validation and bounded existing codec projection.
use crate::{
    checked_read::{self, Budget},
    durable, validation,
};
use mainframe_env_execution_api::{AuditDecision, LifecycleEventKind};
use mainframe_env_store_api::*;
#[cfg(test)]
pub(crate) mod tests;

pub(crate) fn validate(r: &CheckedReplayRefusalStep, max: usize) -> Result<Budget, StoreError> {
    let mut budget = Budget::new(r.validate_bounds(max)?);
    checked_read::validate_effect_observation(&r.effect, r.event.tick)?;
    if r.effect.state != EffectState::Completed
        || r.audit.decision == AuditDecision::Success
        || r.effect.intent.capability.as_ref() != Some(&r.audit.capability)
        || r.effect.intent.audit_resource != Some(r.audit.resource)
        || r.effect.intent.audit_invocation_key.as_ref() != Some(&r.audit.invocation_key)
        || r.event.sequence
            != r.execution
                .version
                .checked_add(1)
                .ok_or(StoreError::Conflict)?
        || r.event.kind
            != (LifecycleEventKind::EffectResult {
                sequence: r.effect.sequence,
            })
        || r.notification.notification_id
            != format!("{}:{:020}", r.execution.execution_id, r.event.sequence)
        || r.notification.topic != "execution.lifecycle.v1"
        || r.notification.payload
            != mainframe_env_execution_api::lifecycle_notification_payload(&r.event.kind)
    {
        return Err(StoreError::InvalidTransition);
    }
    validation::execution_step(
        &r.execution.execution_id,
        &r.execution,
        &r.event,
        &r.notification,
    )?;
    validation::audit_event(&r.execution, &r.event, Some(&r.effect), Some(&r.audit))?;
    // Charge finite JSON escaping/framing BEFORE event/outbox/audit codecs.
    let mut encoded_bound = 4096usize;
    for size in [
        r.audit.execution_id.as_str().len(),
        r.audit.run_unit_id.as_str().len(),
        r.audit.principal.as_str().len(),
        r.audit.invocation_key.as_str().len(),
        r.audit.capability.as_str().len(),
        r.event.execution_id.as_str().len(),
        r.event.run_unit_id.as_str().len(),
        r.notification.notification_id.len(),
        r.notification.execution_id.as_str().len(),
        r.notification.topic.len(),
        r.notification.payload.len(),
    ] {
        encoded_bound = encoded_bound
            .checked_add(size.checked_mul(6).ok_or(StoreError::CapacityExceeded)?)
            .ok_or(StoreError::CapacityExceeded)?;
    }
    budget.add(encoded_bound)?;
    checked_read::memory_core_budget(&r.effect, &r.execution, max, &mut budget)?;
    Ok(budget)
}

pub(crate) fn plan(
    r: &CheckedReplayRefusalStep,
    max: usize,
    budget: &mut Budget,
) -> Result<(ExecutionRecord, Vec<ProviderStateMutation>), StoreError> {
    let mut updated = r.execution.clone();
    updated.version = updated.version.checked_add(1).ok_or(StoreError::Conflict)?;
    let specifications = [
        (
            "durable-execution".into(),
            r.execution.execution_id.to_string(),
            updated.version,
            Some(r.execution.version),
            durable::encode_execution(&updated)?,
        ),
        (
            format!("durable-event:{}", r.execution.execution_id),
            format!("{:020}", r.event.sequence),
            1,
            None,
            durable::encode_event(&r.event)?,
        ),
        (
            "durable-outbox".into(),
            r.notification.notification_id.clone(),
            1,
            None,
            durable::encode_outbox(&r.notification)?,
        ),
        (
            durable::AUDIT_NAMESPACE.into(),
            durable::audit_storage_key(
                &r.execution.execution_id,
                &format!("journal:{:020}", r.event.sequence),
            ),
            1,
            None,
            durable::encode_audit(&r.audit)?,
        ),
    ];
    let mut mutations = Vec::new();
    for (namespace, key, version, expected_version, payload) in specifications {
        let record = ProviderStateRecord {
            namespace,
            key,
            version,
            payload,
        };
        budget.row(&record, max)?;
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record,
            expected_version,
        }));
    }
    Ok((updated, mutations))
}
