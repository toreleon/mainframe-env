//! Root-wide scoped member index, before enabling the serialized runtime writer.
//! Integrity checks cannot reconstruct a live source lease or known outcome.
#![allow(
    dead_code,
    reason = "strict scoped readers precede manager-owned writer rollout"
)]
use super::*;
use crate::cobol::storage_scope::Entry;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

const MAX_SCOPES: usize = 16;
const MAX_CLOSE_METADATA_BYTES: u64 = 128 * 1024;
const MAX_SCOPED_ROW_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Member {
    scope: String,
    program: String,
    artifact: String,
    row_version: u64,
    payload_digest: String,
    charged_bytes: u64,
    busy: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cobol) struct ScopedRun {
    schema_version: u32,
    root: Entry,
    max_member_bytes: u64,
    max_scopes: u32,
    max_calls: u64,
    max_receipt_bytes: u64,
    calls: std::collections::BTreeSet<String>,
    receipt_charge: u64,
    root_charge: u64,
    active: usize,
    charged_bytes: u64,
    scopes: BTreeMap<String, Entry>,
    members: BTreeMap<String, Member>,
    ended_tick: Option<u64>,
    metadata_digest: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cobol) struct ScopedInstance {
    schema_version: u32,
    run_key: String,
    scope_entry: Entry,
    max_state_bytes: u64,
    program: String,
    artifact: String,
    owner: Option<Entry>,
    initial: bool,
    state: Option<Vec<u8>>,
    metadata_digest: String,
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(value).map_err(|_| HostProblem::UnknownOutcome)
}

fn payload_digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn metadata<T: Serialize>(domain: &[u8], key: &str, value: &T) -> Result<String, HostProblem> {
    Ok(super::super::replay::digest(&[
        domain,
        key.as_bytes(),
        &canonical(value)?,
    ]))
}

fn checked_row(record: &ProviderStateRecord, namespace: &str) -> Result<(), HostProblem> {
    if record.namespace != namespace
        || !valid_digest(&record.key)
        || record.version == 0
        || record.version > i64::MAX as u64
        || record.payload.len() > MAX_SCOPED_ROW_BYTES
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

pub(super) fn is_scoped(record: &ProviderStateRecord) -> bool {
    // Canonical V3 starts with this field. Reordered/malformed V3 fails the
    // existing legacy decoder; it never gains a fallback or migration path.
    record.payload.starts_with(b"{\"schema_version\":3,")
}

pub(super) fn describe_run(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    let value =
        ScopedRun::decode(record).map_err(|_| CobolRetentionValidationError::InconsistentState)?;
    let (owner, run, _, _) = value.root.creation_identity();
    let mut dependencies = owner_dependencies(owner, run);
    dependencies.push(provider_dependency(
        CALL_PROTOCOL_NAMESPACE,
        protocol_key(run),
    ));
    for scope in value.scopes.values() {
        let (execution, _, _, _) = scope.actor_identity();
        if execution != owner {
            dependencies.push(
                super::super::retention::CobolRetentionDependency::Execution(execution.into()),
            );
        }
        if let Some(call) = scope.call_key() {
            dependencies.push(provider_dependency(
                super::super::retention::CALL_REPLAY_NAMESPACE,
                call,
            ));
        }
    }
    Ok(CobolRetentionRowDescriptor {
        namespace: record.namespace.clone(),
        key: record.key.clone(),
        row_version: record.version,
        kind: CobolRetentionRowKind::RunState,
        state: if value.ended_tick.is_some() {
            CobolRetentionState::Terminal
        } else {
            CobolRetentionState::Active
        },
        owner_execution: Some(owner.into()),
        owner_run_unit: Some(run.into()),
        terminal_tick: value.ended_tick,
        dependencies,
    })
}

pub(super) fn describe_instance(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    let value = ScopedInstance::decode(record)
        .map_err(|_| CobolRetentionValidationError::InconsistentState)?;
    let (owner, run, _, _) = value.scope_entry.creation_identity();
    let mut dependencies = owner_dependencies(owner, run);
    dependencies.push(provider_dependency(RUN_STATE_NAMESPACE, &value.run_key));
    let creator = value.scope_entry.actor_identity().0;
    if creator != owner {
        dependencies
            .push(super::super::retention::CobolRetentionDependency::Execution(creator.into()));
    }
    if let Some(entry) = &value.owner {
        dependencies.push(
            super::super::retention::CobolRetentionDependency::Execution(
                entry.actor_identity().0.into(),
            ),
        );
        if let Some(call) = entry.call_key() {
            dependencies.push(provider_dependency(
                super::super::retention::CALL_REPLAY_NAMESPACE,
                call,
            ));
        }
    }
    Ok(CobolRetentionRowDescriptor {
        namespace: record.namespace.clone(),
        key: record.key.clone(),
        row_version: record.version,
        kind: CobolRetentionRowKind::Instance,
        state: CobolRetentionState::Active,
        owner_execution: Some(owner.into()),
        owner_run_unit: Some(run.into()),
        terminal_tick: None,
        dependencies,
    })
}

impl ScopedRun {
    pub(in crate::cobol) fn fresh(root: &Invocation) -> Result<Self, HostProblem> {
        let entry = Entry::root(root)?;
        let cap = root.limits.max_storage_bytes;
        let mut value = Self {
            schema_version: 3,
            root: entry.clone(),
            max_member_bytes: cap,
            max_scopes: root.limits.max_frames.min(MAX_SCOPES as u32),
            max_calls: root.limits.max_effects,
            max_receipt_bytes: root
                .limits
                .max_output_bytes
                .checked_add(MAX_CLOSE_METADATA_BYTES)
                .ok_or(HostProblem::ResourceExhausted)?,
            calls: std::collections::BTreeSet::new(),
            receipt_charge: 0,
            // The top machine is outside installed-member ownership. Charge its
            // full declared storage allowance rather than assuming zero bytes.
            root_charge: cap,
            active: 0,
            charged_bytes: cap,
            scopes: BTreeMap::from([(entry.scope_id().into(), entry)]),
            members: BTreeMap::new(),
            ended_tick: None,
            metadata_digest: String::new(),
        };
        value.refresh(&run_key(root))?;
        value.validate(&run_key(root))?;
        Ok(value)
    }

    fn expected_digest(&self, key: &str) -> Result<String, HostProblem> {
        let mut value = self.clone();
        value.metadata_digest.clear();
        metadata(b"scoped-run-metadata@3", key, &value)
    }

    fn refresh(&mut self, key: &str) -> Result<(), HostProblem> {
        self.metadata_digest = self.expected_digest(key)?;
        Ok(())
    }

    fn validate(&self, key: &str) -> Result<(), HostProblem> {
        self.root.validate_stored()?;
        let (_, run, principal, parent) = self.root.creation_identity();
        if self.schema_version != 3
            || parent.is_some()
            || !self.root.is_creator()
            || key != super::super::retention::run_state_key(run, principal)
            || self.max_member_bytes == 0
            || self
                .max_member_bytes
                .checked_mul(MAX_INSTANCES as u64 + 1)
                .is_none_or(|n| n > i64::MAX as u64)
            || !(1..=MAX_SCOPES as u32).contains(&self.max_scopes)
            || self.max_calls == 0
            || self
                .max_member_bytes
                .checked_mul(MAX_INSTANCES as u64 + 1)
                .and_then(|storage| {
                    self.max_receipt_bytes
                        .checked_mul(self.max_calls)
                        .and_then(|receipts| storage.checked_add(receipts))
                })
                .is_none_or(|total| total > i64::MAX as u64)
            || self.max_receipt_bytes <= MAX_CLOSE_METADATA_BYTES
            || self
                .max_receipt_bytes
                .checked_mul(self.max_calls)
                .is_none_or(|n| n > i64::MAX as u64)
            || self.calls.len() as u64 > self.max_calls
            || self.calls.iter().any(|call| !valid_digest(call))
            || self.max_receipt_bytes.checked_mul(self.calls.len() as u64)
                != Some(self.receipt_charge)
            || self.members.len() > MAX_INSTANCES
            || self.scopes.len() > self.max_scopes as usize
            || self.metadata_digest != self.expected_digest(key)?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        if let Some(tick) = self.ended_tick {
            return if tick > 0
                && self.scopes.is_empty()
                && self.members.is_empty()
                && self.active == 0
                && self.root_charge == 0
                && self.charged_bytes == 0
            {
                Ok(())
            } else {
                Err(HostProblem::UnknownOutcome)
            };
        }
        if self.root_charge != self.max_member_bytes
            || self
                .scopes
                .get(self.root.scope_id())
                .is_none_or(|s| !s.same_scope(&self.root))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        for (id, scope) in &self.scopes {
            scope.validate_stored()?;
            let (owner, run, principal, parent) = scope.creation_identity();
            if !scope.is_creator()
                || scope.scope_id() != id
                || (owner, run, principal) != {
                    let (o, r, p, _) = self.root.creation_identity();
                    (o, r, p)
                }
                || scope.logical_level() > self.max_scopes
            {
                return Err(HostProblem::UnknownOutcome);
            }
            if scope
                .call_key()
                .is_some_and(|call| !self.calls.contains(call))
            {
                return Err(HostProblem::UnknownOutcome);
            }
            if parent.is_some() {
                let (_, program, artifact, _) = scope.actor_identity();
                let member = self
                    .members
                    .get(&scope.member_key(program)?)
                    .ok_or(HostProblem::UnknownOutcome)?;
                if !member.busy || member.artifact != artifact || member.scope != *id {
                    return Err(HostProblem::UnknownOutcome);
                }
            }
            match parent {
                None if id == self.root.scope_id() && scope.logical_level() == 1 => {}
                Some(parent)
                    if self.scopes.get(parent).is_some_and(|p| {
                        p.logical_level().checked_add(1) == Some(scope.logical_level())
                    }) => {}
                _ => return Err(HostProblem::UnknownOutcome),
            }
        }
        let mut active = 0_usize;
        let mut charged = self.root_charge;
        for (id, member) in &self.members {
            let scope = self
                .scopes
                .get(&member.scope)
                .ok_or(HostProblem::UnknownOutcome)?;
            if scope.member_key(&member.program)? != *id
                || !valid_identity(&member.artifact)
                || !member
                    .artifact
                    .strip_prefix("sha256:")
                    .is_some_and(valid_digest)
                || member.row_version == 0
                || member.row_version > i64::MAX as u64
                || !valid_digest(&member.payload_digest)
                || member.charged_bytes > self.max_member_bytes
                || member.busy && member.charged_bytes != self.max_member_bytes
            {
                return Err(HostProblem::UnknownOutcome);
            }
            active += usize::from(member.busy);
            charged = charged
                .checked_add(member.charged_bytes)
                .ok_or(HostProblem::UnknownOutcome)?;
        }
        if self
            .charged_bytes
            .checked_add(self.receipt_charge)
            .is_none_or(|n| n > i64::MAX as u64)
            || self.active != active
            || self.charged_bytes != charged
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }

    pub(in crate::cobol) fn decode(record: &ProviderStateRecord) -> Result<Self, HostProblem> {
        checked_row(record, RUN_STATE_NAMESPACE)?;
        let value: Self =
            serde_json::from_slice(&record.payload).map_err(|_| HostProblem::UnknownOutcome)?;
        value.validate(&record.key)?;
        if canonical(&value)? != record.payload {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(value)
    }

    /// A checksum is not permission to widen the trusted root's resource limits.
    pub(in crate::cobol) fn validate_live_limits(
        &self,
        actor: &Invocation,
    ) -> Result<(), HostProblem> {
        self.root.validate_for(actor)?;
        if self.max_member_bytes != actor.limits.max_storage_bytes
            || self.max_scopes != actor.limits.max_frames.min(MAX_SCOPES as u32)
            || self.max_calls != actor.limits.max_effects
            || Some(self.max_receipt_bytes)
                != actor
                    .limits
                    .max_output_bytes
                    .checked_add(MAX_CLOSE_METADATA_BYTES)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        self.validate(&run_key(actor))
    }

    /// Prepare a root-wide CALL reservation in memory; the original pending
    /// receipt and member/source CAS writes must publish this charge atomically.
    /// Scope close never replenishes the historical receipt allowance.
    pub(in crate::cobol) fn reserve_call_charge(
        &mut self,
        key: &str,
        call: &str,
    ) -> Result<(), HostProblem> {
        self.validate(key)?;
        if self.ended_tick.is_some() || self.calls.len() as u64 >= self.max_calls {
            return Err(HostProblem::ResourceExhausted);
        }
        if !valid_digest(call) {
            return Err(HostProblem::Malformed);
        }
        if self.calls.contains(call) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = self.clone();
        next.calls.insert(call.into());
        next.receipt_charge = next
            .max_receipt_bytes
            .checked_mul(next.calls.len() as u64)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.refresh(key)?;
        next.validate(key)?;
        *self = next;
        Ok(())
    }

    /// Verify the complete bounded member set, not a plausible prefix or count.
    pub(in crate::cobol) fn validate_members(
        &self,
        key: &str,
        rows: &[ProviderStateRecord],
    ) -> Result<(), HostProblem> {
        self.validate(key)?;
        if rows.len() != self.members.len() {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            if !seen.insert(&row.key) {
                return Err(HostProblem::UnknownOutcome);
            }
            let member = self
                .members
                .get(&row.key)
                .ok_or(HostProblem::UnknownOutcome)?;
            let value = ScopedInstance::decode(row)?;
            if value.owner.as_ref().is_some_and(|entry| {
                entry
                    .call_key()
                    .is_none_or(|call| !self.calls.contains(call))
            }) {
                return Err(HostProblem::UnknownOutcome);
            }
            let scope = self
                .scopes
                .get(value.scope_entry.scope_id())
                .ok_or(HostProblem::UnknownOutcome)?;
            if scope.creation_identity().3.is_some()
                && scope.member_key(scope.actor_identity().1)? == row.key
                && value.owner.as_ref() != Some(scope)
            {
                return Err(HostProblem::UnknownOutcome);
            }
            if member.row_version != row.version
                || member.payload_digest != payload_digest(&row.payload)
                || member.scope != value.scope_entry.scope_id()
                || !value.scope_entry.same_scope(scope)
                || value.max_state_bytes != self.max_member_bytes
                || member.program != value.program
                || member.artifact != value.artifact
                || member.busy != value.owner.is_some()
                || member.charged_bytes
                    != if member.busy {
                        self.max_member_bytes
                    } else {
                        value.state.as_ref().map_or(0, |v| v.len() as u64)
                    }
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        Ok(())
    }
}

impl ScopedInstance {
    fn expected_digest(&self, key: &str) -> Result<String, HostProblem> {
        let mut value = self.clone();
        value.metadata_digest.clear();
        metadata(b"scoped-instance-metadata@3", key, &value)
    }

    fn decode(row: &ProviderStateRecord) -> Result<Self, HostProblem> {
        if row.payload.len() > MAX_SCOPED_ROW_BYTES {
            return Err(HostProblem::UnknownOutcome);
        }
        let value: Self =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::UnknownOutcome)?;
        checked_row(row, &namespace(&value.run_key))?;
        let scope = &value.scope_entry;
        scope.validate_stored()?;
        let (_, run, principal, _) = scope.creation_identity();
        if value.schema_version != 3
            || !scope.is_creator()
            || value.run_key != super::super::retention::run_state_key(run, principal)
            || value.max_state_bytes == 0
            || value
                .max_state_bytes
                .checked_mul(MAX_INSTANCES as u64 + 1)
                .is_none_or(|n| n > i64::MAX as u64)
            || scope.member_key(&value.program)? != row.key
            || !value
                .artifact
                .strip_prefix("sha256:")
                .is_some_and(valid_digest)
            || value
                .state
                .as_ref()
                .is_some_and(|v| v.len() as u64 > value.max_state_bytes)
            || value.initial && value.state.is_some()
            || value.metadata_digest != value.expected_digest(&row.key)?
            || canonical(&value)? != row.payload
        {
            return Err(HostProblem::UnknownOutcome);
        }
        if let Some(owner) = &value.owner {
            owner.validate_stored()?;
            let (_, program, artifact, _) = owner.actor_identity();
            if !owner.same_scope(scope)
                || owner.is_creator() && owner.creation_identity().3.is_none()
                || program != value.program
                || artifact != value.artifact
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        Ok(value)
    }
}
