//! Bounded physical observations. These DTOs confer no dispatch permission.
use crate::*;
use mainframe_env_execution_api::AuditDecision;
use std::collections::BTreeSet;

/// Read assertions and one insert-only receipt/audit, under the original intent.
/// Exact dependencies are never rewritten. There is no terminal lifecycle here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedProviderReadPublication {
    /// Existing attributed Intent-only publication; success inserts one version1 receipt.
    pub publication: AuditedProviderPublication,
    /// Complete observed current Running execution, compared physically, not a permit.
    pub execution: ExecutionRecord,
    /// Unique exact/absent provider identities, including the receipt's absence.
    pub dependencies: Vec<TerminalRowDependency>,
}

/// Final nonpublishing replay fence; no result or live authority is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReplayAssertion {
    /// Complete original unclaimed canonical Intent or canonical Completed occurrence.
    pub effect: EffectRecord,
    /// Complete current Running execution; historical execution is insufficient.
    pub execution: ExecutionRecord,
    /// Positive SQL-compatible logical tick, not a physical UTC source.
    pub observed_tick: u64,
    /// Identifies the required Exact dependency containing the original retained receipt.
    /// The store checks bytes/version, never parses its provider-specific result.
    pub receipt: ProviderStateIdentity,
    /// Unique bounded exact/absent observations, including the receipt.
    pub dependencies: Vec<TerminalRowDependency>,
}

impl CheckedProviderReadPublication {
    /// Validate finite shape before cloning/encoding. Returns captured input bytes
    /// for the backend's combined 64MiB input/current-record budget.
    /// The 4096-operation limit includes dependencies, receipt writes and the
    /// one existing typed audit emitted by publication, including audit-only Deny.
    /// This does not validate current ownership, clocks, authorization or publication.
    pub fn validate_bounds(&self, max_blob: usize) -> Result<usize, StoreError> {
        let p = &self.publication;
        let mut bytes = core_bytes(&p.intent, &self.execution, max_blob)?;
        for size in [
            p.audit.execution_id.as_str().len(),
            p.audit.run_unit_id.as_str().len(),
            p.audit.principal.as_str().len(),
            p.audit.invocation_key.as_str().len(),
            p.audit.capability.as_str().len(),
        ] {
            add(&mut bytes, size)?;
        }
        let mutations_and_audit = p
            .mutations
            .len()
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        dependencies(
            &self.dependencies,
            mutations_and_audit,
            max_blob,
            &mut bytes,
        )?;
        p.validate_mutations(max_blob)?;
        match (p.audit.decision, p.mutations.as_slice()) {
            (AuditDecision::Deny, []) => {}
            (AuditDecision::Success, [ProviderStateMutation::Put(w)])
                if w.expected_version.is_none() && w.record.version == 1 =>
            {
                if !self.dependencies.iter().any(|d| {
                    matches!(d,
                    TerminalRowDependency::Absent { namespace, key }
                        if namespace == &w.record.namespace && key == &w.record.key)
                }) {
                    return Err(StoreError::InvalidTransition);
                }
                add(&mut bytes, w.record.namespace.len())?;
                add(&mut bytes, w.record.key.len())?;
                add(&mut bytes, w.record.payload.len())?;
            }
            _ => return Err(StoreError::InvalidTransition),
        }
        Ok(bytes)
    }
}

impl ProviderReplayAssertion {
    /// Validate bounded observations and require the receipt as an Exact dependency.
    /// A successful shape check alone never admits replay or recovers an effect.
    pub fn validate_bounds(&self, max_blob: usize) -> Result<usize, StoreError> {
        let mut bytes = core_bytes(&self.effect, &self.execution, max_blob)?;
        dependencies(&self.dependencies, 0, max_blob, &mut bytes)?;
        identity(&self.receipt.namespace, &self.receipt.key)?;
        add(&mut bytes, self.receipt.namespace.len())?;
        add(&mut bytes, self.receipt.key.len())?;
        if !self.dependencies.iter().any(|d| {
            matches!(d, TerminalRowDependency::Exact(r)
            if r.namespace == self.receipt.namespace && r.key == self.receipt.key)
        }) {
            return Err(StoreError::InvalidTransition);
        }
        Ok(bytes)
    }
}

pub(crate) fn core_bytes(
    e: &EffectRecord,
    x: &ExecutionRecord,
    max: usize,
) -> Result<usize, StoreError> {
    let mut bytes = 0;
    for size in [
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
    ] {
        if size > max {
            return Err(StoreError::PayloadTooLarge);
        }
        add(&mut bytes, size)?;
    }
    Ok(bytes)
}

pub(crate) fn dependencies(
    ds: &[TerminalRowDependency],
    extra_operations: usize,
    max: usize,
    bytes: &mut usize,
) -> Result<(), StoreError> {
    if ds
        .len()
        .checked_add(extra_operations)
        .is_none_or(|n| n > MAX_ROOT_OPERATIONS)
    {
        return Err(StoreError::CapacityExceeded);
    }
    // Only borrowed identity references are allocated after the count preflight.
    let mut seen = BTreeSet::new();
    for d in ds {
        let (n, k, payload) = match d {
            TerminalRowDependency::Exact(r) => {
                r.validate_write(max)?;
                (&r.namespace, &r.key, r.payload.len())
            }
            TerminalRowDependency::Absent { namespace, key } => (namespace, key, 0),
        };
        identity(n, k)?;
        if !seen.insert((n, k)) {
            return Err(StoreError::Conflict);
        }
        for size in [n.len(), k.len(), payload] {
            add(bytes, size)?;
        }
    }
    Ok(())
}

fn identity(n: &str, k: &str) -> Result<(), StoreError> {
    if n.is_empty()
        || n.len() > MAX_PROVIDER_NAMESPACE_BYTES
        || k.is_empty()
        || k.len() > MAX_PROVIDER_KEY_BYTES
        || n.starts_with("durable-")
        || (n == "jes-worker-meta" && k == "logical-clock")
    {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}
pub(crate) fn add(bytes: &mut usize, size: usize) -> Result<(), StoreError> {
    *bytes = bytes
        .checked_add(size)
        .ok_or(StoreError::CapacityExceeded)?;
    if *bytes > MAX_ROOT_PAYLOAD_BYTES {
        Err(StoreError::CapacityExceeded)
    } else {
        Ok(())
    }
}
