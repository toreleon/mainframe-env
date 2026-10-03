//! Shared observation validation; no provider payload interpretation or callbacks.
use crate::{durable, validation};
use mainframe_env_store_api::*;
mod methods;
#[cfg(test)]
mod tests;
pub(crate) use methods::checked_read_methods;

pub(crate) struct Budget {
    bytes: usize,
}
impl Budget {
    pub(crate) fn new(bytes: usize) -> Self {
        Self { bytes }
    }
    pub(crate) fn add(&mut self, bytes: usize) -> Result<(), StoreError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(StoreError::CapacityExceeded)?;
        if self.bytes > MAX_ROOT_PAYLOAD_BYTES {
            Err(StoreError::CapacityExceeded)
        } else {
            Ok(())
        }
    }
    pub(crate) fn row(&mut self, row: &ProviderStateRecord, max: usize) -> Result<(), StoreError> {
        row.validate_write(max)?;
        for bytes in [row.namespace.len(), row.key.len(), row.payload.len()] {
            self.add(bytes)?;
        }
        Ok(())
    }
}

pub(crate) fn replay_validate(
    r: &ProviderReplayAssertion,
    max: usize,
) -> Result<Budget, StoreError> {
    let bytes = r.validate_bounds(max)?;
    let e = &r.effect;
    validation::effect(e)?;
    if e.digest_format != EffectDigestFormat::CanonicalHostV1
        || e.intent.capability.is_none()
        || e.intent.audit_resource.is_none()
        || e.intent.audit_invocation_key.is_none()
        || e.intent.recovery_lease.is_some()
        || e.intent.created_tick == 0
        || e.intent.recovery_after_tick > i64::MAX as u64
        || r.observed_tick == 0
        || r.observed_tick > i64::MAX as u64
        || r.observed_tick < e.intent.created_tick
    {
        return Err(StoreError::LeaseConflict);
    }
    match e.state {
        EffectState::Intent => {
            validation::new_intent(e)?;
            if r.observed_tick >= e.intent.recovery_after_tick {
                return Err(StoreError::LeaseConflict);
            }
        }
        EffectState::Completed => {
            validation::terminal(&e.key, e)?;
            if e.resolved_tick
                .is_none_or(|tick| tick > r.observed_tick || tick > i64::MAX as u64)
            {
                return Err(StoreError::InvalidTransition);
            }
        }
        _ => return Err(StoreError::InvalidTransition),
    }
    Ok(Budget::new(bytes))
}

pub(crate) fn fence(
    e: &EffectRecord,
    x: &ExecutionRecord,
    retained: &EffectRecord,
    current: &ExecutionRecord,
    tick: u64,
    floor: u64,
) -> Result<(), StoreError> {
    if e != retained || x != current {
        return Err(StoreError::Conflict);
    }
    validation::effect_execution(current, retained)?;
    if current.state != ExecutionState::Running
        || current.terminal_tick.is_some()
        || current.version == 0
        || current.version > i64::MAX as u64
    {
        return Err(StoreError::Conflict);
    }
    if floor > tick || current.lease_expiry_tick.is_some_and(|t| t <= tick) {
        return Err(StoreError::LeaseConflict);
    }
    Ok(())
}

// Memory holds typed core records. Bound their actual variable storage before
// allocating the existing codec buffers; JSON escaping has a finite upper bound.
pub(crate) fn memory_core_budget(
    e: &EffectRecord,
    x: &ExecutionRecord,
    max: usize,
    budget: &mut Budget,
) -> Result<(), StoreError> {
    let sizes = [
        e.execution_id.as_str().len(),
        e.run_unit_id.as_str().len(),
        e.key.as_str().len(),
        e.intent.owner.as_str().len(),
        e.intent.capability.as_ref().map_or(0, |v| v.as_str().len()),
        e.intent
            .audit_invocation_key
            .as_ref()
            .map_or(0, |v| v.as_str().len()),
        e.intent
            .recovery_lease
            .as_ref()
            .map_or(0, |v| v.owner.len()),
        x.execution_id.as_str().len(),
        x.run_unit_id.as_str().len(),
        x.principal.as_str().len(),
        x.selector.as_str().len(),
        x.artifact.as_str().len(),
        x.owner_lease.as_ref().map_or(0, String::len),
    ];
    let mut bound = 4096usize;
    for size in sizes {
        if size > max {
            return Err(StoreError::PayloadTooLarge);
        }
        bound = bound
            .checked_add(size.checked_mul(6).ok_or(StoreError::CapacityExceeded)?)
            .ok_or(StoreError::CapacityExceeded)?;
    }
    // Conservative bound charged before codecs allocate; never an unbounded clone.
    budget.add(bound)?;
    for bytes in [durable::encode_effect(e)?, durable::encode_execution(x)?] {
        if bytes.len() > max {
            return Err(StoreError::PayloadTooLarge);
        }
    }
    Ok(())
}

pub(crate) fn identity(d: &TerminalRowDependency) -> (&str, &str) {
    match d {
        TerminalRowDependency::Exact(row) => (&row.namespace, &row.key),
        TerminalRowDependency::Absent { namespace, key } => (namespace, key),
    }
}
pub(crate) fn compare(
    d: &TerminalRowDependency,
    row: Option<&ProviderStateRecord>,
) -> Result<(), StoreError> {
    match (d, row) {
        (TerminalRowDependency::Absent { .. }, None) => Ok(()),
        (TerminalRowDependency::Exact(expected), Some(current)) if expected == current => Ok(()),
        _ => Err(StoreError::Conflict),
    }
}

pub(crate) fn scope(
    doc: Option<&crate::root_terminal::Document>,
    namespace: &str,
    key: &str,
    ns: Option<&ProviderStateRecord>,
    row: Option<&ProviderStateRecord>,
) -> Result<(), StoreError> {
    // Row and namespace ownership cannot overlap, even if root strings agree.
    if ns.is_some() && row.is_some() {
        return Err(StoreError::Conflict);
    }
    match (doc, ns.or(row)) {
        (Some(doc), Some(binding)) => doc.require_scope_index(binding, namespace, key),
        (None, None) => Ok(()),
        _ => Err(StoreError::InvalidTransition),
    }
}
