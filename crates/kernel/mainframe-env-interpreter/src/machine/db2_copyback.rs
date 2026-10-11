//! Bounded provisional SQL copyback trace; absent from snapshots and default off.
//! Digests attest actual host provenance; copied cells alone attest selected storage.
use super::*;
use mainframe_env_execution_api::{
    ArtifactRef, AuditResourceDigest, ExecutionId, PrincipalId, RunUnitId,
};
use mainframe_env_host_api::{
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
};

#[cfg(test)]
mod tests;

const MAX_STATEMENTS: usize = 64;
const MAX_CELLS: usize = 16;
const MAX_BYTES: usize = 64 * 1024;
const MAX_METADATA: usize = 64 * 1024;
const DIAGNOSTICS: [&str; 2] = ["SQLCODE", "SQLSTATE"];

/// Diagnostic refusal; this never changes the ordinary machine outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CopybackProblem {
    /// The live machine or offered effect does not match the fresh invocation scope.
    Scope,
    /// A collector is already enabled on this machine.
    AlreadyEnabled,
    /// A selected storage identifier, name or whole-view geometry is inconsistent.
    Layout,
    /// An offered operation, reply or sequence differs from the admitted order.
    Order,
    /// A checked dimension, digest representation or aggregate charge exceeds its bound.
    Bounds,
    /// An ordinary response application failed or required local diagnostics are unavailable.
    Application,
    /// Machine restoration invalidated this live-only collector.
    Restoration,
    /// A pending boundary or missing capture prevents a complete trace.
    Incomplete,
}

/// An asserted whole admitted view; no expected bytes, authority or callbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CopybackSlot {
    storage: StorageId,
    name: String,
    length: usize,
}
impl Db2CopybackSlot {
    /// Select an actual admitted identifier/name/width to be checked against the live machine.
    pub fn new(
        storage: StorageId,
        name: String,
        length: usize,
    ) -> Result<Self, Db2CopybackProblem> {
        if name.is_empty() || name.len() > 1024 || length == 0 || length > MAX_BYTES {
            return Err(Db2CopybackProblem::Bounds);
        }
        Ok(Self {
            storage,
            name,
            length,
        })
    }
    /// Borrow the selected storage identifier.
    pub fn storage(&self) -> StorageId {
        self.storage
    }
    /// Borrow the complete admitted name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Borrow the asserted whole-view width.
    pub fn length(&self) -> usize {
        self.length
    }
}

/// Finite SQL operation order and whole-cell selections; no application names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CopybackPlan {
    operations: Vec<Db2Operation>,
    slots: Vec<Db2CopybackSlot>,
}
impl Db2CopybackPlan {
    /// Bound aggregate capture and metadata before copying any live machine cells.
    pub fn new(
        operations: Vec<Db2Operation>,
        slots: Vec<Db2CopybackSlot>,
    ) -> Result<Self, Db2CopybackProblem> {
        if operations.is_empty() || operations.len() > MAX_STATEMENTS || slots.len() > MAX_CELLS {
            return Err(Db2CopybackProblem::Bounds);
        }
        let mut seen = BTreeSet::new();
        let mut bytes = 2 * (16 + 4);
        let mut metadata = 256;
        for slot in &slots {
            if !seen.insert(slot.storage) {
                return Err(Db2CopybackProblem::Layout);
            }
            charge(&mut bytes, slot.length, MAX_BYTES)?;
            charge(&mut metadata, slot.name.len() + 64, MAX_METADATA)?;
        }
        if bytes
            .checked_mul(operations.len())
            .ok_or(Db2CopybackProblem::Bounds)?
            > MAX_BYTES
            || metadata
                .checked_mul(operations.len())
                .ok_or(Db2CopybackProblem::Bounds)?
                > MAX_METADATA
        {
            return Err(Db2CopybackProblem::Bounds);
        }
        Ok(Self { operations, slots })
    }
}

/// Exact raw selected postimage, with no public constructor or serialization.
pub struct Db2CopybackCell {
    selected: Db2CopybackSlot,
    base: usize,
    offset: usize,
    bytes: Vec<u8>,
}
impl Db2CopybackCell {
    /// Borrow the exact admitted selection.
    pub fn selected(&self) -> &Db2CopybackSlot {
        &self.selected
    }
    /// Borrow the actual owning runtime base.
    pub fn base(&self) -> usize {
        self.base
    }
    /// Borrow the actual physical offset.
    pub fn offset(&self) -> usize {
        self.offset
    }
    /// Borrow raw bytes copied after unchanged SQL reply application.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Actual local implicit value; this is explicitly not physical SQLCA storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2CopybackValue {
    /// Exact bytes of the current local implicit diagnostic value.
    Bytes(Vec<u8>),
    /// The current local fixed decimal value, without a physical SQLCA encoding claim.
    Decimal {
        /// Signed coefficient of the actual local decimal value.
        coefficient: i128,
        /// Decimal scale of the actual local decimal value.
        scale: u32,
    },
}

/// Actual invocation origin, captured from the live machine only.
pub struct Db2CopybackOrigin {
    execution: ExecutionId,
    run: RunUnitId,
    selector: Selector,
    artifact: ArtifactRef,
    principal: PrincipalId,
    invocation_key: IdempotencyKey,
    attempt: u32,
    deadline: u64,
}
impl Db2CopybackOrigin {
    /// Borrow actual execution identity.
    pub fn execution(&self) -> &ExecutionId {
        &self.execution
    }
    /// Borrow actual run identity.
    pub fn run(&self) -> &RunUnitId {
        &self.run
    }
    /// Borrow actual program selector.
    pub fn selector(&self) -> &Selector {
        &self.selector
    }
    /// Borrow actual artifact reference.
    pub fn artifact(&self) -> &ArtifactRef {
        &self.artifact
    }
    /// Borrow actual principal identity.
    pub fn principal(&self) -> &PrincipalId {
        &self.principal
    }
    /// Borrow actual invocation key.
    pub fn invocation_key(&self) -> &IdempotencyKey {
        &self.invocation_key
    }
    /// Borrow actual invocation attempt.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
    /// Borrow actual deadline; no clock conversion is implied.
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
}

/// Actual request/reply/copyback boundary, including unsuccessful SQLCODEs.
pub struct Db2CopybackRecord {
    operation: Db2Operation,
    sequence: u64,
    pc: usize,
    request_digest: [u8; 32],
    result_digest: [u8; 32],
    resource: AuditResourceDigest,
    key: Option<IdempotencyKey>,
    input_count: usize,
    present_indicators: usize,
    provider_sqlcode: i32,
    provider_sqlstate: [u8; 5],
    provider_affected_rows: u64,
    provider_rows: usize,
    cells: Vec<Db2CopybackCell>,
    implicit: Vec<(&'static str, Db2CopybackValue)>,
}
impl Db2CopybackRecord {
    /// Borrow actual typed operation.
    pub fn operation(&self) -> Db2Operation {
        self.operation
    }
    /// Borrow actual offered sequence.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Borrow actual program counter at offer.
    pub fn program_counter(&self) -> usize {
        self.pc
    }
    /// Borrow canonical original host request digest.
    pub fn request_digest(&self) -> &[u8; 32] {
        &self.request_digest
    }
    /// Borrow canonical actual host result digest; not a storage proof.
    pub fn result_digest(&self) -> &[u8; 32] {
        &self.result_digest
    }
    /// Borrow canonical resource identity for a future actual audit join.
    pub fn resource(&self) -> AuditResourceDigest {
        self.resource
    }
    /// Borrow actual offered mutation key, if any.
    pub fn key(&self) -> Option<&IdempotencyKey> {
        self.key.as_ref()
    }
    /// Borrow request input count without values.
    pub fn input_count(&self) -> usize {
        self.input_count
    }
    /// Borrow actual Some indicator count; None indicators are never repaired.
    pub fn present_indicators(&self) -> usize {
        self.present_indicators
    }
    /// Borrow provider SQLCODE, separate from raw selected storage.
    pub fn provider_sqlcode(&self) -> i32 {
        self.provider_sqlcode
    }
    /// Borrow exact five provider state bytes.
    pub fn provider_sqlstate(&self) -> &[u8; 5] {
        &self.provider_sqlstate
    }
    /// Borrow provider affected rows; this does not imply a SQLERRD write.
    pub fn provider_affected_rows(&self) -> u64 {
        self.provider_affected_rows
    }
    /// Borrow provider row count without row data.
    pub fn provider_rows(&self) -> usize {
        self.provider_rows
    }
    /// Borrow actual selected raw cells.
    pub fn cells(&self) -> &[Db2CopybackCell] {
        &self.cells
    }
    /// Borrow local implicit SQL diagnostics; these are not physical SQLCA assertions.
    pub fn implicit(&self) -> &[(&'static str, Db2CopybackValue)] {
        &self.implicit
    }
}

/// Provisional live-only archive; no actor completion, replay, source or cleanup proof.
/// A server owner must separately qualify those actual identities before acceptance.
pub struct Db2CopybackArchive {
    origin: Db2CopybackOrigin,
    records: Vec<Db2CopybackRecord>,
    final_effect_sequence: u64,
}
impl Db2CopybackArchive {
    /// Borrow actual machine invocation origin.
    pub fn origin(&self) -> &Db2CopybackOrigin {
        &self.origin
    }
    /// Borrow all ordered captured boundaries.
    pub fn records(&self) -> &[Db2CopybackRecord] {
        &self.records
    }
    /// Borrow effect sequence at consumption; no terminal state is implied.
    pub fn final_effect_sequence(&self) -> u64 {
        self.final_effect_sequence
    }
}

struct Selected {
    slot: Db2CopybackSlot,
    view: StorageView,
}
struct Offered {
    operation: Db2Operation,
    sequence: u64,
    pc: usize,
    request_digest: [u8; 32],
    resource: AuditResourceDigest,
    key: Option<IdempotencyKey>,
    input_count: usize,
    present_indicators: usize,
    result: Option<Reply>,
}
struct Reply {
    digest: [u8; 32],
    sqlcode: i32,
    sqlstate: [u8; 5],
    affected_rows: u64,
    rows: usize,
}
pub(super) struct Collector {
    origin: Db2CopybackOrigin,
    operations: Vec<Db2Operation>,
    selected: Vec<Selected>,
    records: Vec<Db2CopybackRecord>,
    pending: Option<Offered>,
    bytes: usize,
    metadata: usize,
    refusal: Option<Db2CopybackProblem>,
}
fn charge(usage: &mut usize, amount: usize, cap: usize) -> Result<(), Db2CopybackProblem> {
    let next = usage
        .checked_add(amount)
        .ok_or(Db2CopybackProblem::Bounds)?;
    if next > cap {
        return Err(Db2CopybackProblem::Bounds);
    }
    *usage = next;
    Ok(())
}
impl Collector {
    fn new(machine: &ReferenceMachine, plan: &Db2CopybackPlan) -> Result<Self, Db2CopybackProblem> {
        if machine.pending.is_some()
            || machine.pc != 0
            || machine.executed_steps != 0
            || machine.effect_sequence != 0
        {
            return Err(Db2CopybackProblem::Scope);
        }
        let invocation = &machine.invocation;
        if invocation.attempt == 0 || invocation.deadline_tick == 0 {
            return Err(Db2CopybackProblem::Scope);
        }
        let mut metadata = 0;
        for name in [
            invocation.execution_id.as_str(),
            invocation.run_unit_id.as_str(),
            invocation.selector.as_str(),
            invocation.artifact.as_str(),
            invocation.principal.id().as_str(),
            invocation.idempotency_key.as_str(),
        ] {
            if name.is_empty() || name.len() > InvocationLimits::default().max_identity_bytes {
                return Err(Db2CopybackProblem::Bounds);
            }
            charge(&mut metadata, name.len(), MAX_METADATA)?;
        }
        let mut selected = Vec::with_capacity(plan.slots.len());
        for slot in &plan.slots {
            let layout = machine
                .layouts
                .get(&slot.name)
                .ok_or(Db2CopybackProblem::Layout)?;
            let view = machine
                .views
                .get(&slot.name)
                .ok_or(Db2CopybackProblem::Layout)?;
            if layout.dynamic
                || layout.unbounded
                || view.length != slot.length
                || machine.storage_names_by_id.get(&slot.storage) != Some(&slot.name)
                || machine.views_by_id.get(&slot.storage) != Some(view)
            {
                return Err(Db2CopybackProblem::Layout);
            }
            charge(&mut metadata, slot.name.len() + 64, MAX_METADATA)?;
            selected.push(Selected {
                slot: slot.clone(),
                view: view.clone(),
            });
        }
        Ok(Self {
            origin: Db2CopybackOrigin {
                execution: invocation.execution_id.clone(),
                run: invocation.run_unit_id.clone(),
                selector: invocation.selector.clone(),
                artifact: invocation.artifact.clone(),
                principal: invocation.principal.id().clone(),
                invocation_key: invocation.idempotency_key.clone(),
                attempt: invocation.attempt,
                deadline: invocation.deadline_tick,
            },
            operations: plan.operations.clone(),
            selected,
            records: Vec::with_capacity(plan.operations.len()),
            pending: None,
            bytes: 0,
            metadata,
            refusal: None,
        })
    }
    pub(super) fn refuse(&mut self, problem: Db2CopybackProblem) {
        self.refusal.get_or_insert(problem);
        self.pending = None;
    }
    fn offered(&mut self, machine: &ReferenceMachine, effect: &EffectRequest, kind: &PendingKind) {
        if self.refusal.is_some() {
            return;
        }
        let HostRequest::Db2(request) = &effect.request else {
            return;
        };
        let outcome = (|| {
            if !matches!(kind, PendingKind::Db2 { .. })
                || self.pending.is_some()
                || self.operations.get(self.records.len()) != Some(&request.operation)
            {
                return Err(Db2CopybackProblem::Order);
            }
            if effect.sequence == 0
                || effect.run_unit != self.origin.run
                || effect.deadline_tick != self.origin.deadline
            {
                return Err(Db2CopybackProblem::Scope);
            }
            let resource = canonical_audit_resource_digest(&effect.request);
            if resource.format
                != mainframe_env_execution_api::AuditResourceDigestFormat::CanonicalHostResourceV1
            {
                return Err(Db2CopybackProblem::Bounds);
            }
            charge(
                &mut self.metadata,
                256 + effect
                    .idempotency_key
                    .as_ref()
                    .map_or(0, |key| key.as_str().len()),
                MAX_METADATA,
            )?;
            self.pending = Some(Offered {
                operation: request.operation,
                sequence: effect.sequence,
                pc: machine.pc,
                request_digest: canonical_request_digest(&effect.request)
                    .map_err(|_| Db2CopybackProblem::Bounds)?,
                resource,
                key: effect.idempotency_key.clone(),
                input_count: request.inputs.len(),
                present_indicators: request
                    .inputs
                    .values()
                    .filter(|value| value.indicator.is_some())
                    .count(),
                result: None,
            });
            Ok(())
        })();
        if let Err(problem) = outcome {
            self.refuse(problem);
        }
    }
    pub(super) fn resumed(&mut self, result: &EffectResult) {
        if self.refusal.is_some() || self.pending.is_none() {
            return;
        }
        let outcome = (|| {
            let pending = self
                .pending
                .as_mut()
                .ok_or(Db2CopybackProblem::Incomplete)?;
            let Ok(HostResult::Db2(reply)) = &result.outcome else {
                return Err(Db2CopybackProblem::Application);
            };
            if pending.sequence != result.sequence || pending.result.is_some() {
                return Err(Db2CopybackProblem::Order);
            }
            pending.result = Some(Reply {
                digest: canonical_result_digest(&result.outcome)
                    .map_err(|_| Db2CopybackProblem::Bounds)?,
                sqlcode: reply.sqlcode,
                sqlstate: reply
                    .sqlstate
                    .as_bytes()
                    .try_into()
                    .map_err(|_| Db2CopybackProblem::Application)?,
                affected_rows: reply.affected_rows,
                rows: reply.rows.len(),
            });
            Ok(())
        })();
        if let Err(problem) = outcome {
            self.refuse(problem);
        }
    }
    fn applied(&mut self, machine: &ReferenceMachine) {
        if self.refusal.is_some() {
            return;
        }
        let outcome = (|| {
            let pending = self.pending.take().ok_or(Db2CopybackProblem::Incomplete)?;
            let reply = pending.result.ok_or(Db2CopybackProblem::Incomplete)?;
            if pending.sequence != machine.effect_sequence
                || self.operations.get(self.records.len()) != Some(&pending.operation)
            {
                return Err(Db2CopybackProblem::Order);
            }
            // Check every full range and aggregate charge before any raw/implicit clone.
            let mut bytes = 0;
            for selected in &self.selected {
                if machine.views_by_id.get(&selected.slot.storage) != Some(&selected.view)
                    || machine.views.get(&selected.slot.name) != Some(&selected.view)
                {
                    return Err(Db2CopybackProblem::Layout);
                }
                let end = selected
                    .view
                    .offset
                    .checked_add(selected.view.length)
                    .ok_or(Db2CopybackProblem::Bounds)?;
                if machine
                    .bases
                    .get(selected.view.base)
                    .and_then(|base| base.get(selected.view.offset..end))
                    .is_none()
                {
                    return Err(Db2CopybackProblem::Layout);
                }
                charge(&mut bytes, selected.view.length, MAX_BYTES)?;
                charge(
                    &mut self.metadata,
                    selected.slot.name.len() + 64,
                    MAX_METADATA,
                )?;
            }
            for name in DIAGNOSTICS {
                let length = match machine.implicit.get(name) {
                    Some(CobolValue::Bytes(value)) => value.len(),
                    Some(CobolValue::Decimal(_)) => 16 + 4,
                    None => return Err(Db2CopybackProblem::Application),
                };
                charge(&mut bytes, length, MAX_BYTES)?;
            }
            charge(&mut self.bytes, bytes, MAX_BYTES)?;
            let mut cells = Vec::with_capacity(self.selected.len());
            for selected in &self.selected {
                let view = &selected.view;
                cells.push(Db2CopybackCell {
                    selected: selected.slot.clone(),
                    base: view.base,
                    offset: view.offset,
                    bytes: machine.bases[view.base][view.offset..view.offset + view.length]
                        .to_vec(),
                });
            }
            let mut implicit = Vec::with_capacity(DIAGNOSTICS.len());
            for name in DIAGNOSTICS {
                let value = match machine.implicit.get(name) {
                    Some(CobolValue::Bytes(value)) => Db2CopybackValue::Bytes(value.clone()),
                    Some(CobolValue::Decimal(value)) => Db2CopybackValue::Decimal {
                        coefficient: value.coefficient,
                        scale: value.scale,
                    },
                    None => return Err(Db2CopybackProblem::Application),
                };
                implicit.push((name, value));
            }
            self.records.push(Db2CopybackRecord {
                operation: pending.operation,
                sequence: pending.sequence,
                pc: pending.pc,
                request_digest: pending.request_digest,
                result_digest: reply.digest,
                resource: pending.resource,
                key: pending.key,
                input_count: pending.input_count,
                present_indicators: pending.present_indicators,
                provider_sqlcode: reply.sqlcode,
                provider_sqlstate: reply.sqlstate,
                provider_affected_rows: reply.affected_rows,
                provider_rows: reply.rows,
                cells,
                implicit,
            });
            Ok(())
        })();
        if let Err(problem) = outcome {
            self.refuse(problem);
        }
    }
}
impl ReferenceMachine {
    /// Explicit fresh-machine setup; no source expectations, dispatch, grants or callbacks.
    pub fn enable_db2_copyback(&mut self, plan: Db2CopybackPlan) -> Result<(), Db2CopybackProblem> {
        if self.db2_copyback.is_some() {
            return Err(Db2CopybackProblem::AlreadyEnabled);
        }
        self.db2_copyback = Some(Collector::new(self, &plan)?);
        Ok(())
    }
    pub(super) fn observe_db2_copyback_offer(
        &mut self,
        effect: &EffectRequest,
        kind: &PendingKind,
    ) {
        if let Some(mut collector) = self.db2_copyback.take() {
            collector.offered(self, effect, kind);
            self.db2_copyback = Some(collector);
        }
    }
    pub(super) fn observe_db2_copyback_applied(&mut self) {
        if let Some(mut collector) = self.db2_copyback.take() {
            collector.applied(self);
            self.db2_copyback = Some(collector);
        }
    }
    /// Consume a complete live trace. The owning actor still needs separate qualification.
    pub fn take_db2_copyback(&mut self) -> Option<Result<Db2CopybackArchive, Db2CopybackProblem>> {
        let collector = self.db2_copyback.take()?;
        Some((|| {
            if let Some(problem) = collector.refusal {
                return Err(problem);
            }
            if self.pending.is_some()
                || collector.pending.is_some()
                || collector.records.len() != collector.operations.len()
            {
                return Err(Db2CopybackProblem::Incomplete);
            }
            if collector
                .records
                .windows(2)
                .any(|pair| pair[0].sequence >= pair[1].sequence)
            {
                return Err(Db2CopybackProblem::Order);
            }
            Ok(Db2CopybackArchive {
                origin: collector.origin,
                records: collector.records,
                final_effect_sequence: self.effect_sequence,
            })
        })())
    }
}
