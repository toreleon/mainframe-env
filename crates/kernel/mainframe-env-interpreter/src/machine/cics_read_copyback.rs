//! Opt-in passive postapplication storage observation, excluded from checkpoints.
//! Digests attest provenance; only the copied machine cells attest storage.
use super::*;
use mainframe_env_execution_api::{
    ArtifactRef, AuditResourceDigest, ExecutionId, PrincipalId, RunUnitId,
};
use mainframe_env_host_api::{
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_ir::CicsStorageSlot;

#[cfg(test)]
mod tests;

const MAX_READS: usize = 64;
const MAX_CELLS: usize = 16;
const MAX_BYTES: usize = 64 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_EIB_VALUE_BYTES: usize = 16 + 4;
const EIB: [&str; 6] = [
    "EIBRESP", "EIBRESP2", "EIBFN", "EIBAID", "EIBCALEN", "EIBTRNID",
];

/// Value-free diagnostic refusal. This never changes the ordinary machine outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsReadCopybackProblem {
    Scope,
    AlreadyEnabled,
    Layout,
    Order,
    Bounds,
    Application,
    Restoration,
    Incomplete,
}

/// Actual selected cell, with its admitted resolved slot and physical view.
/// No caller constructors, deserialization, mutation or whole-machine dump.
pub struct CicsReadCopybackCell {
    slot: CicsStorageSlot,
    base: usize,
    offset: usize,
    bytes: Vec<u8>,
}

impl CicsReadCopybackCell {
    pub fn slot(&self) -> &CicsStorageSlot {
        &self.slot
    }
    pub fn base(&self) -> usize {
        self.base
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Exact local implicit cell; this is not a binary DFHEIBLK memory assertion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CicsReadCopybackValue {
    Bytes(Vec<u8>),
    Decimal { coefficient: i128, scale: u32 },
}

/// Identity copied only from the machine's actual Invocation. No payload bindings.
pub struct CicsReadCopybackOrigin {
    execution: ExecutionId,
    run: RunUnitId,
    selector: Selector,
    artifact: ArtifactRef,
    principal: PrincipalId,
    invocation_key: IdempotencyKey,
    attempt: u32,
    deadline: u64,
}

impl CicsReadCopybackOrigin {
    pub fn execution(&self) -> &ExecutionId {
        &self.execution
    }
    pub fn run(&self) -> &RunUnitId {
        &self.run
    }
    pub fn selector(&self) -> &Selector {
        &self.selector
    }
    pub fn artifact(&self) -> &ArtifactRef {
        &self.artifact
    }
    pub fn principal(&self) -> &PrincipalId {
        &self.principal
    }
    pub fn invocation_key(&self) -> &IdempotencyKey {
        &self.invocation_key
    }
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
}

/// Actual offered effect metadata. READ keeps its original dispatch key without mutation metadata.
pub struct CicsReadCopybackEffect {
    sequence: u64,
    program_counter: usize,
    request_digest: [u8; 32],
    resource: AuditResourceDigest,
    result_digest: [u8; 32],
    key: Option<IdempotencyKey>,
}

impl CicsReadCopybackEffect {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn program_counter(&self) -> usize {
        self.program_counter
    }
    pub fn request_digest(&self) -> &[u8; 32] {
        &self.request_digest
    }
    pub fn resource(&self) -> AuditResourceDigest {
        self.resource
    }
    pub fn result_digest(&self) -> &[u8; 32] {
        &self.result_digest
    }
    pub fn key(&self) -> Option<&IdempotencyKey> {
        self.key.as_ref()
    }
}

/// A successful READ's raw cells at the postapplication boundary.
pub struct CicsReadCopybackRecord {
    effect: CicsReadCopybackEffect,
    cells: Vec<CicsReadCopybackCell>,
    eib: Vec<(&'static str, CicsReadCopybackValue)>,
}

impl CicsReadCopybackRecord {
    pub fn effect(&self) -> &CicsReadCopybackEffect {
        &self.effect
    }
    /// Ordered INTO, RESP, RESP2 cells, without re-encoding or normalization.
    pub fn cells(&self) -> &[CicsReadCopybackCell] {
        &self.cells
    }
    pub fn eib(&self) -> &[(&'static str, CicsReadCopybackValue)] {
        &self.eib
    }
}

/// Opaque live archive. This alone is neither durable terminal nor cleanup proof.
/// The server owns that separate qualification; checkpoints cannot create this.
pub struct CicsReadCopybackArchive {
    origin: CicsReadCopybackOrigin,
    reads: Vec<CicsReadCopybackRecord>,
    terminal: CicsReadCopybackEffect,
}

impl CicsReadCopybackArchive {
    pub fn origin(&self) -> &CicsReadCopybackOrigin {
        &self.origin
    }
    pub fn reads(&self) -> &[CicsReadCopybackRecord] {
        &self.reads
    }
    pub fn terminal(&self) -> &CicsReadCopybackEffect {
        &self.terminal
    }
}

/// Bounded structural assertion about one resolved admitted storage slot.
/// It carries no expected bytes and confers no dispatch or storage authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsReadCopybackSlotSelection {
    descriptor: CicsStorageSlot,
    length: usize,
}

impl CicsReadCopybackSlotSelection {
    pub fn new(
        descriptor: CicsStorageSlot,
        length: usize,
    ) -> Result<Self, CicsReadCopybackProblem> {
        if descriptor.qualified_layout_name.is_empty()
            || descriptor.qualified_layout_name.len() > 1024
            || length == 0
            || length > MAX_BYTES
        {
            return Err(CicsReadCopybackProblem::Bounds);
        }
        Ok(Self { descriptor, length })
    }
    pub fn descriptor(&self) -> &CicsStorageSlot {
        &self.descriptor
    }
    pub fn length(&self) -> usize {
        self.length
    }
}

/// One ordered READ's actual typed destinations selected by the operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsReadCopybackSelection {
    into: CicsReadCopybackSlotSelection,
    response: Option<CicsReadCopybackSlotSelection>,
    response2: Option<CicsReadCopybackSlotSelection>,
}

impl CicsReadCopybackSelection {
    pub fn new(
        into: CicsReadCopybackSlotSelection,
        response: Option<CicsReadCopybackSlotSelection>,
        response2: Option<CicsReadCopybackSlotSelection>,
    ) -> Self {
        Self {
            into,
            response,
            response2,
        }
    }
}

/// Bounded generic observation plan. Application names/source choices belong to
/// the assurance owner, not this kernel. No callbacks, values or whole-machine snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsReadCopybackPlan {
    reads: Vec<CicsReadCopybackSelection>,
}

impl CicsReadCopybackPlan {
    pub fn read_count(&self) -> usize {
        self.reads.len()
    }
    pub fn new(reads: Vec<CicsReadCopybackSelection>) -> Result<Self, CicsReadCopybackProblem> {
        if reads.is_empty() || reads.len() > MAX_READS {
            return Err(CicsReadCopybackProblem::Bounds);
        }
        let mut bytes = 0;
        let mut metadata = 0;
        for read in &reads {
            charge(&mut bytes, EIB.len() * MAX_EIB_VALUE_BYTES, MAX_BYTES)?;
            for slot in [&read.into]
                .into_iter()
                .chain(read.response.iter())
                .chain(read.response2.iter())
            {
                charge(&mut bytes, slot.length, MAX_BYTES)?;
                charge(
                    &mut metadata,
                    slot.descriptor.qualified_layout_name.len() + 64,
                    MAX_METADATA_BYTES,
                )?;
            }
        }
        Ok(Self { reads })
    }
}

struct Slot {
    descriptor: CicsStorageSlot,
    view: StorageView,
}

struct Selection {
    into: Slot,
    response: Option<Slot>,
    response2: Option<Slot>,
}

struct PendingOrigin {
    operation: CicsOperation,
    sequence: u64,
    pc: usize,
    request_digest: [u8; 32],
    resource: AuditResourceDigest,
    key: Option<IdempotencyKey>,
    result_digest: Option<[u8; 32]>,
    eligible: bool,
}

pub(super) struct Collector {
    origin: CicsReadCopybackOrigin,
    selections: Vec<Selection>,
    reads: Vec<CicsReadCopybackRecord>,
    terminal: Option<CicsReadCopybackEffect>,
    pending: Option<PendingOrigin>,
    bytes: usize,
    metadata: usize,
    incomplete: Option<CicsReadCopybackProblem>,
}

fn slot(
    machine: &ReferenceMachine,
    selected: &CicsReadCopybackSlotSelection,
) -> Result<Slot, CicsReadCopybackProblem> {
    let descriptor = &selected.descriptor;
    let name = &descriptor.qualified_layout_name;
    let view = machine
        .views
        .get(name)
        .ok_or(CicsReadCopybackProblem::Layout)?;
    let layout = machine
        .layouts
        .get(name)
        .ok_or(CicsReadCopybackProblem::Layout)?;
    if machine.storage_names_by_id.get(&descriptor.storage) != Some(name)
        || machine.views_by_id.get(&descriptor.storage) != Some(view)
        || view.length != selected.length
        || layout.dynamic
        || layout.unbounded
    {
        return Err(CicsReadCopybackProblem::Layout);
    }
    Ok(Slot {
        descriptor: descriptor.clone(),
        view: view.clone(),
    })
}

fn target_matches(target: Option<&typed_cics::CicsTarget>, expected: Option<&Slot>) -> bool {
    match (target, expected) {
        (Some(typed_cics::CicsTarget::Resolved(actual)), Some(expected)) => {
            actual == &expected.descriptor
        }
        (None, None) => true,
        _ => false,
    }
}

fn charge(current: &mut usize, amount: usize, cap: usize) -> Result<(), CicsReadCopybackProblem> {
    let next = current
        .checked_add(amount)
        .ok_or(CicsReadCopybackProblem::Bounds)?;
    if next > cap {
        return Err(CicsReadCopybackProblem::Bounds);
    }
    *current = next;
    Ok(())
}

impl Collector {
    fn new(
        machine: &ReferenceMachine,
        plan: &CicsReadCopybackPlan,
    ) -> Result<Self, CicsReadCopybackProblem> {
        let invocation = &machine.invocation;
        if invocation.attempt == 0 || machine.pending.is_some() {
            return Err(CicsReadCopybackProblem::Scope);
        }
        let identities = [
            invocation.execution_id.as_str(),
            invocation.run_unit_id.as_str(),
            invocation.selector.as_str(),
            invocation.artifact.as_str(),
            invocation.principal.id().as_str(),
            invocation.idempotency_key.as_str(),
        ];
        let mut metadata = 0;
        for identity in identities {
            if identity.is_empty()
                || identity.len() > InvocationLimits::default().max_identity_bytes
            {
                return Err(CicsReadCopybackProblem::Bounds);
            }
            charge(&mut metadata, identity.len(), MAX_METADATA_BYTES)?;
        }
        let mut selections = Vec::with_capacity(plan.reads.len());
        for selected in &plan.reads {
            let into = slot(machine, &selected.into)?;
            let response = selected
                .response
                .as_ref()
                .map(|selected| slot(machine, selected))
                .transpose()?;
            let response2 = selected
                .response2
                .as_ref()
                .map(|selected| slot(machine, selected))
                .transpose()?;
            for slot in [&into]
                .into_iter()
                .chain(response.iter())
                .chain(response2.iter())
            {
                charge(
                    &mut metadata,
                    slot.descriptor.qualified_layout_name.len() + 64,
                    MAX_METADATA_BYTES,
                )?;
            }
            selections.push(Selection {
                into,
                response,
                response2,
            });
        }
        Ok(Self {
            origin: CicsReadCopybackOrigin {
                execution: invocation.execution_id.clone(),
                run: invocation.run_unit_id.clone(),
                selector: invocation.selector.clone(),
                artifact: invocation.artifact.clone(),
                principal: invocation.principal.id().clone(),
                invocation_key: invocation.idempotency_key.clone(),
                attempt: invocation.attempt,
                deadline: invocation.deadline_tick,
            },
            reads: Vec::with_capacity(selections.len()),
            selections,
            terminal: None,
            pending: None,
            bytes: 0,
            metadata,
            incomplete: None,
        })
    }

    pub(super) fn refuse(&mut self, problem: CicsReadCopybackProblem) {
        self.incomplete.get_or_insert(problem);
        self.pending = None;
    }

    fn offered(&mut self, machine: &ReferenceMachine, effect: &EffectRequest, kind: &PendingKind) {
        if self.incomplete.is_some() {
            return;
        }
        let PendingKind::Cics {
            operation,
            into,
            outputs,
            response,
            response2,
            ..
        } = kind
        else {
            return;
        };
        if !matches!(operation, CicsOperation::Read | CicsOperation::Return) {
            return;
        }
        let result = (|| {
            if self.pending.is_some() || self.terminal.is_some() {
                return Err(CicsReadCopybackProblem::Order);
            }
            if *operation == CicsOperation::Read {
                let selected = self
                    .selections
                    .get(self.reads.len())
                    .ok_or(CicsReadCopybackProblem::Order)?;
                let key = machine
                    .effect_key()
                    .map_err(|_| CicsReadCopybackProblem::Bounds)?;
                if self.reads.len() >= MAX_READS
                    || !target_matches(into.as_ref(), Some(&selected.into))
                    || !target_matches(response.as_ref(), selected.response.as_ref())
                    || !target_matches(response2.as_ref(), selected.response2.as_ref())
                    || !outputs.is_empty()
                    || effect.idempotency_key.as_ref() != Some(&key)
                {
                    return Err(CicsReadCopybackProblem::Layout);
                }
            }
            if effect.sequence == 0
                || effect.run_unit != self.origin.run
                || effect.deadline_tick != self.origin.deadline
                || !matches!(&effect.request, HostRequest::Cics(actual) if actual.operation == *operation)
            {
                return Err(CicsReadCopybackProblem::Scope);
            }
            let request_digest = canonical_request_digest(&effect.request)
                .map_err(|_| CicsReadCopybackProblem::Bounds)?;
            let resource = canonical_audit_resource_digest(&effect.request);
            if resource.format
                != mainframe_env_execution_api::AuditResourceDigestFormat::CanonicalHostResourceV1
            {
                return Err(CicsReadCopybackProblem::Bounds);
            }
            charge(&mut self.metadata, 256, MAX_METADATA_BYTES)?;
            self.pending = Some(PendingOrigin {
                operation: *operation,
                sequence: effect.sequence,
                pc: machine.pc,
                request_digest,
                resource,
                key: effect.idempotency_key.clone(),
                result_digest: None,
                eligible: false,
            });
            Ok(())
        })();
        if let Err(problem) = result {
            self.refuse(problem);
        }
    }

    pub(super) fn resumed(&mut self, result: &EffectResult) {
        if self.incomplete.is_some() || self.pending.is_none() {
            return;
        }
        let success = (|| {
            let pending = self
                .pending
                .as_mut()
                .ok_or(CicsReadCopybackProblem::Incomplete)?;
            let Ok(HostResult::Cics(response)) = &result.outcome else {
                return Err(CicsReadCopybackProblem::Application);
            };
            let expected = if pending.operation == CicsOperation::Read {
                CicsDisposition::Complete
            } else {
                CicsDisposition::Returned
            };
            if result.sequence != pending.sequence
                || response.response != 0
                || response.response2 != 0
                || (pending.operation == CicsOperation::Read
                    && (!matches!(
                        response.payload.schema(),
                        "mainframe-env.cics.into@1" | "mainframe-env.cics.payload@1"
                    ) || !response.outputs.is_empty()))
                || (pending.operation == CicsOperation::Read
                    && response.payload.bytes().len()
                        != self
                            .selections
                            .get(self.reads.len())
                            .ok_or(CicsReadCopybackProblem::Order)?
                            .into
                            .view
                            .length)
                || response.disposition != expected
                || response.condition != "NORMAL"
            {
                return Err(CicsReadCopybackProblem::Application);
            }
            pending.result_digest = Some(
                canonical_result_digest(&result.outcome)
                    .map_err(|_| CicsReadCopybackProblem::Bounds)?,
            );
            pending.eligible = true;
            Ok(())
        })();
        if let Err(problem) = success {
            self.refuse(problem);
        }
    }

    fn applied(&mut self, machine: &ReferenceMachine, operation: CicsOperation) {
        if self.incomplete.is_some()
            || !matches!(operation, CicsOperation::Read | CicsOperation::Return)
        {
            return;
        }
        let result = (|| {
            let pending = self
                .pending
                .take()
                .ok_or(CicsReadCopybackProblem::Incomplete)?;
            if pending.operation != operation
                || pending.sequence != machine.effect_sequence
                || !pending.eligible
            {
                return Err(CicsReadCopybackProblem::Application);
            }
            let effect = CicsReadCopybackEffect {
                sequence: pending.sequence,
                program_counter: pending.pc,
                request_digest: pending.request_digest,
                resource: pending.resource,
                result_digest: pending
                    .result_digest
                    .ok_or(CicsReadCopybackProblem::Incomplete)?,
                key: pending.key,
            };
            if operation == CicsOperation::Return {
                if self.reads.len() != self.selections.len() || effect.key.is_none() {
                    return Err(CicsReadCopybackProblem::Incomplete);
                }
                self.terminal = Some(effect);
                return Ok(());
            }
            let selected = self
                .selections
                .get(self.reads.len())
                .ok_or(CicsReadCopybackProblem::Order)?;
            let count = 1
                + usize::from(selected.response.is_some())
                + usize::from(selected.response2.is_some());
            if self.reads.len() >= MAX_READS || count + EIB.len() > MAX_CELLS {
                return Err(CicsReadCopybackProblem::Bounds);
            }
            let sources = [&selected.into]
                .into_iter()
                .chain(selected.response.iter())
                .chain(selected.response2.iter());
            let raw_length = sources.clone().try_fold(0usize, |sum, slot| {
                sum.checked_add(slot.view.length)
                    .ok_or(CicsReadCopybackProblem::Bounds)
            })?;
            // Charge the actual accepted typed values before any storage/EIB
            // copy. Ordinary application can preserve a Decimal EIBTRNID after
            // an admitted numeric write; a fixed normal-layout sum is insufficient.
            let eib_length = EIB.into_iter().try_fold(0usize, |sum, name| {
                // Physical EIB layouts need a separate coherence profile.
                if machine.views.contains_key(name) {
                    return Err(CicsReadCopybackProblem::Layout);
                }
                let length = match machine.implicit.get(name) {
                    Some(CobolValue::Bytes(bytes)) if bytes.len() <= 4 => bytes.len(),
                    Some(CobolValue::Decimal(_)) => MAX_EIB_VALUE_BYTES,
                    _ => return Err(CicsReadCopybackProblem::Layout),
                };
                sum.checked_add(length)
                    .ok_or(CicsReadCopybackProblem::Bounds)
            })?;
            let capture_length = raw_length
                .checked_add(eib_length)
                .ok_or(CicsReadCopybackProblem::Bounds)?;
            charge(&mut self.bytes, capture_length, MAX_BYTES)?;
            let mut cells = Vec::with_capacity(count);
            for source in sources {
                if machine.views_by_id.get(&source.descriptor.storage) != Some(&source.view)
                    || machine.views.get(&source.descriptor.qualified_layout_name)
                        != Some(&source.view)
                {
                    return Err(CicsReadCopybackProblem::Layout);
                }
                charge(
                    &mut self.metadata,
                    source.descriptor.qualified_layout_name.len() + 32,
                    MAX_METADATA_BYTES,
                )?;
                let end = source
                    .view
                    .offset
                    .checked_add(source.view.length)
                    .ok_or(CicsReadCopybackProblem::Bounds)?;
                let bytes = machine
                    .bases
                    .get(source.view.base)
                    .and_then(|base| base.get(source.view.offset..end))
                    .ok_or(CicsReadCopybackProblem::Layout)?
                    .to_vec();
                cells.push(CicsReadCopybackCell {
                    slot: source.descriptor.clone(),
                    base: source.view.base,
                    offset: source.view.offset,
                    bytes,
                });
            }
            let mut eib = Vec::with_capacity(EIB.len());
            for name in EIB {
                // A physical EIB layout needs a separate profile with coherence checks.
                if machine.views.contains_key(name) {
                    return Err(CicsReadCopybackProblem::Layout);
                }
                let value = match machine.implicit.get(name) {
                    Some(CobolValue::Bytes(bytes)) if bytes.len() <= 4 => {
                        CicsReadCopybackValue::Bytes(bytes.clone())
                    }
                    Some(CobolValue::Decimal(value)) => CicsReadCopybackValue::Decimal {
                        coefficient: value.coefficient,
                        scale: value.scale,
                    },
                    _ => return Err(CicsReadCopybackProblem::Layout),
                };
                eib.push((name, value));
            }
            self.reads
                .push(CicsReadCopybackRecord { effect, cells, eib });
            Ok(())
        })();
        if let Err(problem) = result {
            self.refuse(problem);
        }
    }
}

impl ReferenceMachine {
    /// Arm one bounded typed slot plan; no application names or expected bytes enter runtime.
    /// This does not create durable success, grants, bindings, cleanup or an extra drive.
    pub fn enable_read_copyback(
        &mut self,
        plan: CicsReadCopybackPlan,
    ) -> Result<(), CicsReadCopybackProblem> {
        if self.cics_read_copyback.is_some() {
            return Err(CicsReadCopybackProblem::AlreadyEnabled);
        }
        self.cics_read_copyback = Some(Collector::new(self, &plan)?);
        Ok(())
    }

    pub(super) fn observe_read_copyback_offer(
        &mut self,
        effect: &EffectRequest,
        kind: &PendingKind,
    ) {
        if let Some(mut collector) = self.cics_read_copyback.take() {
            collector.offered(self, effect, kind);
            self.cics_read_copyback = Some(collector);
        }
    }

    pub(super) fn observe_read_copyback_applied(&mut self, operation: CicsOperation) {
        if let Some(mut collector) = self.cics_read_copyback.take() {
            collector.applied(self, operation);
            self.cics_read_copyback = Some(collector);
        }
    }

    /// Consume only a live successful selected-READ/RETURN collector. This is still
    /// provisional until the server verifies ordinary core completion and cleanup.
    pub fn take_read_copyback(
        &mut self,
    ) -> Option<Result<CicsReadCopybackArchive, CicsReadCopybackProblem>> {
        let collector = self.cics_read_copyback.take()?;
        Some((|| {
            if let Some(problem) = collector.incomplete {
                return Err(problem);
            }
            if collector.pending.is_some()
                || self.pending.is_some()
                || collector.reads.len() != collector.selections.len()
            {
                return Err(CicsReadCopybackProblem::Incomplete);
            }
            let terminal = collector
                .terminal
                .ok_or(CicsReadCopybackProblem::Incomplete)?;
            if terminal.sequence != self.effect_sequence
                || collector
                    .reads
                    .windows(2)
                    .any(|pair| pair[0].effect.sequence >= pair[1].effect.sequence)
                || collector
                    .reads
                    .last()
                    .is_none_or(|read| read.effect.sequence >= terminal.sequence)
            {
                return Err(CicsReadCopybackProblem::Order);
            }
            Ok(CicsReadCopybackArchive {
                origin: collector.origin,
                reads: collector.reads,
                terminal,
            })
        })())
    }
}
