//! Additive native root ownership and atomic terminal-step contracts.
//!
//! These are bounded storage observations, not host admission, a final flag,
//! SAF permission or an application effect. Backends recheck every observation
//! under the same physical lock/transaction. Original execution/effect bytes
//! and the existing journal transaction keep their meanings.
use crate::{
    CheckpointRecord, EffectRecord, ExecutionRecord, OutboxRecord, ProviderStateMutation,
    ProviderStateRecord, StoreError, WorkRecord,
};
use mainframe_env_execution_api::{
    ExecutionId, IdempotencyKey, LifecycleEvent, RootTerminalAudit, RootTerminalDisposition,
};

/// Maximum complete actor set in a native root; max+1 refuses, never truncates.
pub const MAX_ROOT_ACTORS: usize = 256;
/// Maximum combined captured dependencies and physical terminal operations.
pub const MAX_ROOT_OPERATIONS: usize = 4096;
/// Shared encoded/captured ceiling, further narrowed by actual store quotas.
pub const MAX_ROOT_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
/// Core-owned namespace; providers cannot write these ownership records.
pub const ROOT_DRIVER_NAMESPACE: &str = "durable-root-driver-v1";

/// Original compiled root admission data from its trusted configured driver.
/// Structural validation does not attest compilation or the physical host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootDriverAdmission {
    /// Original parentNone root's admitted execution record.
    pub execution: ExecutionRecord,
    /// Exact original root invocation key; not an MQ effect identity.
    pub invocation_key: IdempotencyKey,
    /// Immutable configured physical setup/canonical admission identity.
    pub configuration_digest: [u8; 32],
    /// Original finite root deadline in the shared logical clock domain.
    pub deadline_tick: u64,
    /// Exact run-specific lifecycle namespaces supplied by the genuine host.
    /// They are storage scopes, not topology or permission from application IDs.
    pub provider_namespaces: Vec<String>,
    /// Exact shared-namespace lifecycle identities owned by this root/run.
    pub provider_rows: Vec<crate::ProviderStateIdentity>,
    /// Actual root admission event.
    pub event: LifecycleEvent,
    /// Existing exact notification for that event.
    pub notification: OutboxRecord,
}

/// Opaque observation of the actual inserted root ownership row.
/// No numeric lease constructor, mutable projection or Serde implementation.
/// It is not an attestation: publication compares its exact admission and row
/// identity to the current core-owned record inside the physical transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootDriverClaim {
    admission: RootDriverAdmission,
    inserted: ProviderStateRecord,
}
impl RootDriverAdmission {
    /// Validate finite original structural bounds before cloning or encoding.
    pub fn validate(&self) -> Result<(), StoreError> {
        use crate::ExecutionState;
        if self.execution.state != ExecutionState::Admitted
            || self.execution.version != 1
            || self.execution.attempt == 0
            || self.execution.terminal_tick.is_some()
            || self.execution.owner_lease.is_some()
            || self.execution.lease_expiry_tick.is_some()
            || self.event.execution_id != self.execution.execution_id
            || self.event.run_unit_id != self.execution.run_unit_id
            || self.event.attempt != self.execution.attempt
            || self.event.sequence != 1
            || !self.event.validate()
            || !matches!(
                self.event.kind,
                mainframe_env_execution_api::LifecycleEventKind::Admitted
            )
            || self.event.tick == 0
            || self.deadline_tick <= self.event.tick
            || self.deadline_tick > i64::MAX as u64
            || self.notification.execution_id != self.execution.execution_id
            || self.notification.sequence != self.event.sequence
            || self.notification.topic != "execution.lifecycle.v1"
            || self.notification.notification_id
                != format!("{}:{:020}", self.event.execution_id, self.event.sequence)
            || self.notification.payload
                != mainframe_env_execution_api::lifecycle_notification_payload(&self.event.kind)
            || self.notification.payload.len() > MAX_ROOT_PAYLOAD_BYTES
            || self.provider_namespaces.len() > 16
            || self.provider_rows.len() > 16
        {
            return Err(StoreError::InvalidTransition);
        }
        for (i, namespace) in self.provider_namespaces.iter().enumerate() {
            if namespace.is_empty()
                || namespace.len() > crate::MAX_PROVIDER_NAMESPACE_BYTES
                || namespace.starts_with("durable-")
                || self.provider_namespaces[..i].contains(namespace)
            {
                return Err(StoreError::InvalidTransition);
            }
        }
        for (i, identity) in self.provider_rows.iter().enumerate() {
            if identity.namespace.is_empty()
                || identity.namespace.len() > crate::MAX_PROVIDER_NAMESPACE_BYTES
                || identity.namespace.starts_with("durable-")
                || identity.key.is_empty()
                || identity.key.len() > crate::MAX_PROVIDER_KEY_BYTES
                || self.provider_rows[..i].contains(identity)
            {
                return Err(StoreError::InvalidTransition);
            }
        }
        Ok(())
    }

    /// Decode an inserted-row observation after admission. This is structural
    /// data, never host attestation or permission to settle a root. The backend
    /// must verify the row's complete codec and retained equality again.
    pub fn observe_inserted(
        &self,
        inserted: &ProviderStateRecord,
    ) -> Result<RootDriverClaim, StoreError> {
        self.validate()?;
        inserted.validate_write(MAX_ROOT_PAYLOAD_BYTES)?;
        if inserted.namespace != ROOT_DRIVER_NAMESPACE
            || inserted.key != self.execution.execution_id.as_str()
            || inserted.version != 1
        {
            return Err(StoreError::Conflict);
        }
        Ok(RootDriverClaim {
            admission: self.clone(),
            inserted: inserted.clone(),
        })
    }
}
impl RootDriverClaim {
    /// Original admission, without substitutable execution or configuration.
    pub fn admission(&self) -> &RootDriverAdmission {
        &self.admission
    }
    /// Exact physical row observed at admission, not a caller-selected epoch.
    pub fn inserted_row(&self) -> &ProviderStateRecord {
        &self.inserted
    }
}

/// Actual compiled child admission plus the exact winning original CALL row.
/// Enrollment and execution admission are one transaction while the root is Open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootChildAdmission {
    /// Opaque observation of this same root's actual admission.
    pub claim: RootDriverClaim,
    /// Already enrolled exact parent execution.
    pub parent: ExecutionId,
    /// Original live parent CALL occurrence. Its execution/intent are read
    /// again inside enrollment's physical transaction; no record is supplied.
    pub parent_occurrence: RootProviderRowAdmission,
    /// Original child execution, never relabeled parentNone.
    pub execution: ExecutionRecord,
    /// Actual child admission event.
    pub event: LifecycleEvent,
    /// Original child admission notification.
    pub notification: OutboxRecord,
    /// Exact retained winning original CALL reservation, not a new intent.
    pub call: ProviderStateRecord,
    /// Exact original compiled child catalog, not a post-completion selection.
    pub catalog: ProviderStateRecord,
}

/// Register an original lifecycle row before its first write under the actual
/// enrolled actor's retained canonical intent. No EffectRecord is supplied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootProviderRowAdmission {
    /// Actual opaque root observation retained by the genuine host driver.
    pub claim: RootDriverClaim,
    /// Exact running original parent record observed for this occurrence.
    pub execution: ExecutionRecord,
    /// Original core effect key, read by the backend under the same lock/TX.
    pub effect_key: IdempotencyKey,
    /// Original effect occurrence sequence.
    pub effect_sequence: u64,
    /// Full shared canonical HostRequest identity of that original occurrence.
    pub request_digest: [u8; 32],
    /// Exact provider lifecycle row identity; not a wildcard namespace grant.
    pub identity: crate::ProviderStateIdentity,
    /// Finite original live observation, before intent recovery and root expiry.
    pub observed_tick: u64,
}

/// Exact or absent provider read dependency, distinct from row mutations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalRowDependency {
    /// The entire record must equal its original captured bytes and version.
    Exact(ProviderStateRecord),
    /// No row may exist at this bounded identity.
    Absent {
        /// Exact provider namespace.
        namespace: String,
        /// Exact provider key.
        key: String,
    },
}

/// One complete enrolled actor snapshot taken inside the root Closing gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootActorSnapshot {
    /// Exact current actor, including run/principal/attempt/lease/version/state.
    pub execution: ExecutionRecord,
    /// Root is None; child parent is the actual enrolled parent.
    pub parent: Option<ExecutionId>,
    /// Frozen original CALL/core/catalog relationship for a genuine enrolled child.
    pub call: Option<RootCallBinding>,
    /// Complete actor-indexed effects, never a global truncated prefix.
    pub effects: Vec<EffectRecord>,
    /// Exact current checkpoint; unsupported live/suspended ownership refuses.
    pub checkpoint: Option<CheckpointRecord>,
    /// Complete associated durable work/recovery controls.
    pub work: Vec<WorkRecord>,
    /// Actual event high-water mark observed in the same capture.
    pub last_event: LifecycleEvent,
}

/// Original compiled child CALL attribution retained by the core membership.
/// Structural observations do not attest compilation, host topology or finality.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootCallBinding {
    /// Exact existing CALL reservation namespace.
    pub namespace: String,
    /// Exact existing CALL reservation identity.
    pub key: String,
    /// Original parent effect key, not the child invocation key.
    pub effect_key: IdempotencyKey,
    /// Full shared canonical original parent CALL request digest.
    pub request_digest: [u8; 32],
    /// Original compiled child batch catalog observation, never reselected.
    pub catalog: ProviderStateRecord,
}

/// Complete bounded Closing observation. Fields are source observations rather
/// than a host winner. Backend captures/rechecks scoped actor completeness and
/// protected provider epoch in its owning physical transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootClosureSnapshot {
    /// Original opaque root admission observation.
    pub claim: RootDriverClaim,
    /// Exact current core-owned Closing row and version.
    pub closing: ProviderStateRecord,
    /// Complete enrolled actor set in immutable enrollment order, root first.
    pub actors: Vec<RootActorSnapshot>,
    /// Exact original registered CALL and other lifecycle rows.
    pub provider_dependencies: Vec<TerminalRowDependency>,
    /// Exact existing core codec bytes for the complete actor/event/effect/
    /// outbox set. These borrowed-source observations feed the sole canonical
    /// terminal resource; they are never application effects or a second log.
    pub core_records: Vec<ProviderStateRecord>,
    /// Provider mutation epoch captured under the same lock/transaction.
    pub provider_epoch: u64,
    /// Actual finite capture tick, before original deadline.
    pub observed_tick: u64,
}
impl RootClosureSnapshot {
    /// Validate aggregate bounds before copying any captured payload.
    pub fn validate_bounds(&self) -> Result<(), StoreError> {
        self.budget().map(|_| ())
    }
    fn budget(&self) -> Result<RootBudget, StoreError> {
        if self.actors.is_empty()
            || self.actors.len() > MAX_ROOT_ACTORS
            || self.observed_tick == 0
            || self.observed_tick >= self.claim.admission.deadline_tick
            || self.closing.payload.len() > MAX_ROOT_PAYLOAD_BYTES
        {
            return Err(StoreError::CapacityExceeded);
        }
        let mut budget = RootBudget::default();
        budget.row(&self.closing)?;
        budget.row(self.claim.inserted_row())?;
        for actor in &self.actors {
            if actor.checkpoint.is_some()
                || !actor.work.is_empty()
                || actor.execution.owner_lease.is_some()
                || actor.execution.lease_expiry_tick.is_some()
                || actor
                    .effects
                    .iter()
                    .any(|effect| effect.intent.recovery_lease.is_some())
            {
                // Scheduled/restorable/recovery-owned roots have no accepted
                // synchronous terminal profile; refuse before a deep copy.
                return Err(StoreError::InvalidTransition);
            }
            // Bounded typed metadata is charged in addition to retained codec
            // bytes because both are held by this immutable snapshot.
            budget.add(1, 4096)?;
            if let Some(call) = &actor.call {
                budget.row(&call.catalog)?;
                budget.identity(&call.namespace, &call.key)?;
            }
            budget.add(
                actor.effects.len(),
                actor
                    .effects
                    .len()
                    .checked_mul(4096)
                    .ok_or(StoreError::CapacityExceeded)?,
            )?;
            for work in &actor.work {
                budget.add(
                    1,
                    work.payload
                        .len()
                        .checked_add(4096)
                        .ok_or(StoreError::CapacityExceeded)?,
                )?;
            }
            if let Some(checkpoint) = &actor.checkpoint {
                budget.add(
                    1,
                    checkpoint
                        .payload
                        .len()
                        .checked_add(4096)
                        .ok_or(StoreError::CapacityExceeded)?,
                )?;
            }
        }
        for dependency in &self.provider_dependencies {
            budget.dependency(dependency)?;
        }
        for row in &self.core_records {
            budget.row(row)?;
        }
        Ok(budget)
    }
}

/// One existing legal lifecycle step and exact outbox notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootTerminalStep {
    /// Actual core event, including sequence/tick/attempt.
    pub event: LifecycleEvent,
    /// Existing legal next lifecycle state, not a new execution state.
    pub next_state: crate::ExecutionState,
    /// Exact existing lifecycle outbox bytes.
    pub notification: OutboxRecord,
}

/// Atomic terminal publication composed by the exclusive real root winner.
/// This structure is not a permit; the backend owns all final validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootTerminalPublication {
    /// Complete exact Closing snapshot and original claim.
    pub closure: RootClosureSnapshot,
    /// Source-backed known native disposition; Unknown never enters this API.
    pub disposition: RootTerminalDisposition,
    /// Actual final finite publication observation. It cannot precede capture
    /// and is separately bound from the immutable Closing snapshot tick.
    pub observed_tick: u64,
    /// Normal Completing/Completed pair or genuine Failed/Abend single step.
    pub steps: Vec<RootTerminalStep>,
    /// Exact additional read dependencies, not independently substituted state.
    pub dependencies: Vec<TerminalRowDependency>,
    /// Bounded actual provider settlement; no core durable namespace mutation.
    pub mutations: Vec<ProviderStateMutation>,
    /// Provider decision and core terminal observation, distinct actual records.
    pub audits: Vec<RootTerminalAudit>,
}
impl RootTerminalPublication {
    /// Refuse the complete combined snapshot/plan before clone or encoding.
    /// Backend quotas and semantic/CAS validation can further narrow this bound.
    pub fn validate_bounds(&self) -> Result<(), StoreError> {
        let mut budget = self.closure.budget()?;
        for dependency in &self.dependencies {
            budget.dependency(dependency)?;
        }
        for mutation in &self.mutations {
            match mutation {
                ProviderStateMutation::Put(write) => budget.row(&write.record)?,
                ProviderStateMutation::Move {
                    record, old_key, ..
                } => {
                    budget.row(record)?;
                    budget.add(0, old_key.len())?;
                }
                ProviderStateMutation::Delete { namespace, key, .. } => {
                    budget.identity(namespace, key)?
                }
            }
        }
        for step in &self.steps {
            budget.add(
                3,
                step.notification
                    .payload
                    .len()
                    .checked_add(4096)
                    .ok_or(StoreError::CapacityExceeded)?,
            )?;
        }
        budget.add(
            self.audits.len(),
            self.audits
                .len()
                .checked_mul(4096)
                .ok_or(StoreError::CapacityExceeded)?,
        )
    }
}

#[derive(Default)]
struct RootBudget {
    operations: usize,
    bytes: usize,
}
impl RootBudget {
    fn add(&mut self, operations: usize, bytes: usize) -> Result<(), StoreError> {
        self.operations = self
            .operations
            .checked_add(operations)
            .ok_or(StoreError::CapacityExceeded)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(StoreError::CapacityExceeded)?;
        if self.operations > MAX_ROOT_OPERATIONS || self.bytes > MAX_ROOT_PAYLOAD_BYTES {
            Err(StoreError::CapacityExceeded)
        } else {
            Ok(())
        }
    }
    fn identity(&mut self, namespace: &str, key: &str) -> Result<(), StoreError> {
        if namespace.is_empty()
            || namespace.len() > crate::MAX_PROVIDER_NAMESPACE_BYTES
            || key.is_empty()
            || key.len() > crate::MAX_PROVIDER_KEY_BYTES
        {
            return Err(StoreError::IncompatibleVersion);
        }
        self.add(1, namespace.len() + key.len())
    }
    fn row(&mut self, row: &ProviderStateRecord) -> Result<(), StoreError> {
        row.validate_write(MAX_ROOT_PAYLOAD_BYTES)?;
        self.identity(&row.namespace, &row.key)?;
        self.add(0, row.payload.len())
    }
    fn dependency(&mut self, dependency: &TerminalRowDependency) -> Result<(), StoreError> {
        match dependency {
            TerminalRowDependency::Exact(row) => self.row(row),
            TerminalRowDependency::Absent { namespace, key } => self.identity(namespace, key),
        }
    }
}

/// Exact stored terminal winner observed after one physical commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootTerminalCommit {
    /// Original admission identity, for exact duplicate/winner comparison.
    pub claim: RootDriverClaim,
    /// Final core execution record from this publication.
    pub execution: ExecutionRecord,
    /// Exact core-owned terminal winner record; no live tokens are restored.
    pub winner: ProviderStateRecord,
}
