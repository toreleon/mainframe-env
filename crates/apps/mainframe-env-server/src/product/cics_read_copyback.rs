//! Optional same-server typed READ storage witness; no HTTP or persisted archive.
//! The observer has no callbacks, authority grants, drive, retry or cleanup methods.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, describe_cics_replay_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, AuditResourceDigest, LifecycleEvent,
};
use mainframe_env_host_api::{SecurityRequest, canonical_audit_resource_digest};
use mainframe_env_interpreter::{
    CicsReadCopybackArchive, CicsReadCopybackEffect, CicsReadCopybackPlan,
};
use mainframe_env_store_api::{EffectRecord, ExecutionRecord};
use std::sync::OnceLock;

#[cfg(test)]
mod tests;

const MAX_AUDITS: usize = 512;
const MAX_EVENTS: usize = 1024;

/// Fixed reason codes only; no raw customer records, keys, bodies or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsReadObservationProblem {
    LateInstall,
    AlreadyInstalled,
    AlreadyArmed,
    Profile,
    Ambiguous,
    Pending,
    Application,
    Ownership,
    Bounds,
    Cleanup,
}

/// Bounded assertions selecting an actual source/artifact/principal and typed slot plan.
/// These assertions cannot create live authority or manufacture source binding.
pub struct SourceBoundReadCopybackProfile {
    session: SessionId,
    selector: Selector,
    principal: PrincipalId,
    plan: CicsReadCopybackPlan,
    artifact: ArtifactRef,
    semantic_identity: String,
    source_digest: [u8; 32],
}

impl SourceBoundReadCopybackProfile {
    pub fn new(
        session: SessionId,
        selector: Selector,
        principal: PrincipalId,
        artifact: ArtifactRef,
        semantic_identity: String,
        source_digest: [u8; 32],
        plan: CicsReadCopybackPlan,
    ) -> Result<Self, CicsReadObservationProblem> {
        if semantic_identity.is_empty()
            || semantic_identity.len() > 128
            || selector
                .as_str()
                .strip_prefix("program:")
                .is_none_or(str::is_empty)
        {
            return Err(CicsReadObservationProblem::Profile);
        }
        Ok(Self {
            session,
            selector,
            principal,
            artifact,
            semantic_identity,
            source_digest,
            plan,
        })
    }
}

/// Opaque success after the actual machine, actor receipts and existing cleanup.
/// No Clone/Debug/Serde or caller constructor; raw storage is read-only and private.
pub struct CompletedReadCopyback {
    archive: CicsReadCopybackArchive,
    session: SessionId,
    source_digest: [u8; 32],
    payload_digest: [u8; 32],
    semantic_identity: String,
    terminal_version: u64,
    terminal_tick: u64,
}

impl CompletedReadCopyback {
    pub fn archive(&self) -> &CicsReadCopybackArchive {
        &self.archive
    }
    pub fn session(&self) -> &SessionId {
        &self.session
    }
    pub fn asserted_source_digest(&self) -> &[u8; 32] {
        &self.source_digest
    }
    pub fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }
    pub fn semantic_identity(&self) -> &str {
        &self.semantic_identity
    }
    pub fn terminal_version(&self) -> u64 {
        self.terminal_version
    }
    pub fn terminal_tick(&self) -> u64 {
        self.terminal_tick
    }
}

struct ArmState {
    profile: SourceBoundReadCopybackProfile,
    owner: OnceLock<(ExecutionId, RunUnitId, IdempotencyKey, u32)>,
    result: OnceLock<Result<CompletedReadCopyback, CicsReadObservationProblem>>,
}

/// Same exact server retained, never reconstructed from strings or persisted rows.
pub struct CicsReadObserverManager {
    server: Arc<ProductServer>,
}

/// Pure completed observation handle; no resume or re-dispatch operation.
pub struct ArmedReadCopybackObservation {
    state: Arc<ArmState>,
}

impl ArmedReadCopybackObservation {
    pub fn completed(&self) -> Result<&CompletedReadCopyback, CicsReadObservationProblem> {
        match self.state.result.get() {
            Some(Ok(complete)) => Ok(complete),
            Some(Err(problem)) => Err(*problem),
            None => Err(CicsReadObservationProblem::Pending),
        }
    }
}

#[derive(Default)]
pub(super) struct Registry {
    installed: bool,
    machine_binding_started: bool,
    arm: Option<Arc<ArmState>>,
}

impl ProductServer {
    /// Explicit privileged Rust setup before the first online machine binding.
    /// Default empty registry preserves ordinary behavior; there is no public endpoint.
    pub fn install_cics_read_observer(
        self: &Arc<Self>,
    ) -> Result<CicsReadObserverManager, CicsReadObservationProblem> {
        let mut registry = self
            .cics_read_copyback
            .lock()
            .map_err(|_| CicsReadObservationProblem::Ownership)?;
        if registry.machine_binding_started {
            return Err(CicsReadObservationProblem::LateInstall);
        }
        if registry.installed {
            return Err(CicsReadObservationProblem::AlreadyInstalled);
        }
        registry.installed = true;
        Ok(CicsReadObserverManager {
            server: self.clone(),
        })
    }

    pub(super) fn bind_cics_read_observation(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        record: &ArtifactRecord,
        invocation: &Invocation,
        terminal_admission: Option<&TerminalAdmission>,
    ) -> Option<BoundObservation> {
        // Clone selection under one short lock. No callbacks/store/drive/clock run here.
        let state = {
            let mut registry = self.cics_read_copyback.lock().ok()?;
            registry.machine_binding_started = true;
            let state = registry.arm.as_ref()?;
            if state.profile.session != *session
                || state.profile.selector.as_str().strip_prefix("program:") != Some(program)
                || state.result.get().is_some()
            {
                return None;
            }
            state.clone()
        };
        let valid = terminal_admission.is_none_or(|admission| admission.matches_actor(invocation))
            && principal == &state.profile.principal
            && invocation.principal.id() == principal
            && invocation.selector == state.profile.selector
            && invocation.artifact == state.profile.artifact
            && record.artifact == invocation.artifact
            && invocation.artifact.as_str()
                == format!("sha256:{}", super::hex_digest(&record.payload_digest))
            && record.executable.as_ref().is_some_and(|metadata| {
                metadata.semantic_identity == state.profile.semantic_identity
                    && metadata.validates_payload(&record.payload_digest)
            });
        if !valid {
            let _ = state.result.set(Err(CicsReadObservationProblem::Profile));
            return None;
        }
        let owner = (
            invocation.execution_id.clone(),
            invocation.run_unit_id.clone(),
            invocation.idempotency_key.clone(),
            invocation.attempt,
        );
        if state.owner.set(owner).is_err() {
            let _ = state.result.set(Err(CicsReadObservationProblem::Ambiguous));
            return None;
        }
        Some(BoundObservation {
            state,
            terminal_admission: terminal_admission.cloned(),
            finished: false,
        })
    }
}

impl CicsReadObserverManager {
    /// Arm one next actual matching exchange. Caller data remains assertions only.
    pub fn arm(
        &self,
        profile: SourceBoundReadCopybackProfile,
    ) -> Result<ArmedReadCopybackObservation, CicsReadObservationProblem> {
        // Do all ordinary read-only artifact validation outside the registry lock.
        let selected = self
            .server
            .online_programs
            .lock()
            .map_err(|_| CicsReadObservationProblem::Ownership)?
            .get(
                profile
                    .selector
                    .as_str()
                    .strip_prefix("program:")
                    .ok_or(CicsReadObservationProblem::Profile)?,
            )
            .cloned()
            .ok_or(CicsReadObservationProblem::Profile)?;
        let record = self
            .server
            .artifacts
            .get_artifact(&selected)
            .map_err(|_| CicsReadObservationProblem::Ownership)?
            .ok_or(CicsReadObservationProblem::Profile)?;
        if selected != profile.artifact
            || record.artifact != selected
            || admit_executable_artifact(&record).is_err()
            || record
                .executable
                .as_ref()
                .is_none_or(|metadata| metadata.semantic_identity != profile.semantic_identity)
        {
            return Err(CicsReadObservationProblem::Profile);
        }
        let state = Arc::new(ArmState {
            profile,
            owner: OnceLock::new(),
            result: OnceLock::new(),
        });
        let mut registry = self
            .server
            .cics_read_copyback
            .lock()
            .map_err(|_| CicsReadObservationProblem::Ownership)?;
        if !registry.installed || registry.arm.is_some() {
            return Err(CicsReadObservationProblem::AlreadyArmed);
        }
        registry.arm = Some(state.clone());
        Ok(ArmedReadCopybackObservation { state })
    }
}

// Captured only from the actual provider-returned pre-program task invocation.
// The separate resumed transaction binding may already name the continuation;
// the root selector retains the transaction authorize_terminal actually checked.
#[derive(Clone)]
pub(super) struct TerminalAdmission {
    execution: ExecutionId,
    run: RunUnitId,
    principal: PrincipalId,
    attempt: u32,
    invocation_key: IdempotencyKey,
    resource: AuditResourceDigest,
}

impl TerminalAdmission {
    pub(super) fn capture(context: &Invocation) -> Option<Self> {
        let transaction = context.selector.as_str().strip_prefix("cics:")?;
        if transaction.is_empty()
            || transaction.len() > 16
            || !transaction
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return None;
        }
        let resource =
            ResourceName::new(format!("CICS.{}", transaction.to_ascii_uppercase()), 246).ok()?;
        let request = HostRequest::Security(SecurityRequest::Authorize {
            principal: context.principal.id().clone(),
            class: "TCICSTRN".into(),
            resource,
            intent: AccessIntent::Execute,
        });
        Some(Self {
            execution: context.execution_id.clone(),
            run: context.run_unit_id.clone(),
            principal: context.principal.id().clone(),
            attempt: context.attempt,
            invocation_key: context.idempotency_key.clone(),
            resource: canonical_audit_resource_digest(&request),
        })
    }

    fn matches_actor(&self, invocation: &Invocation) -> bool {
        let Some(program) = invocation.selector.as_str().strip_prefix("program:") else {
            return false;
        };
        !program.is_empty()
            && self.execution == invocation.execution_id
            && self.run == invocation.run_unit_id
            && self.principal == *invocation.principal.id()
            && self.attempt == invocation.attempt
            && invocation.idempotency_key.as_str()
                == format!("{}:{program}", self.invocation_key.as_str())
    }
}

pub(super) struct BoundObservation {
    state: Arc<ArmState>,
    terminal_admission: Option<TerminalAdmission>,
    finished: bool,
}

impl Drop for BoundObservation {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .state
                .result
                .set(Err(CicsReadObservationProblem::Cleanup));
        }
    }
}

impl BoundObservation {
    pub(super) fn enable(&self, machine: &mut ReferenceMachine) {
        if machine
            .enable_read_copyback(self.state.profile.plan.clone())
            .is_err()
        {
            let _ = self
                .state
                .result
                .set(Err(CicsReadObservationProblem::Application));
        }
    }

    /// Called only after all original ordinary-completion cleanup has succeeded.
    /// Qualification failure changes diagnostic state, never the product result.
    pub(super) fn finish(
        mut self,
        server: &ProductServer,
        invocation: &Invocation,
        machine: &mut ReferenceMachine,
    ) {
        let result = machine
            .take_read_copyback()
            .ok_or(CicsReadObservationProblem::Application)
            .and_then(|archive| archive.map_err(|_| CicsReadObservationProblem::Application))
            .and_then(|archive| {
                qualify(
                    server,
                    invocation,
                    &self.state,
                    self.terminal_admission.as_ref(),
                    archive,
                )
            });
        let _ = self.state.result.set(result);
        self.finished = true;
    }
}

fn owner_matches(execution: &ExecutionRecord, invocation: &Invocation) -> bool {
    execution.execution_id == invocation.execution_id
        && execution.run_unit_id == invocation.run_unit_id
        && execution.principal == *invocation.principal.id()
        && execution.artifact == invocation.artifact
        && execution.selector == invocation.selector
        && execution.attempt == invocation.attempt
        && execution.state == ExecutionState::Completed
        && execution.version > 0
        && execution.owner_lease.is_none()
        && execution.lease_expiry_tick.is_none()
        && execution
            .terminal_tick
            .is_some_and(|tick| tick > 0 && tick <= invocation.deadline_tick)
}

fn events_match(
    events: &[LifecycleEvent],
    execution: &ExecutionRecord,
    reads: &[u64],
    terminal_sequence: u64,
) -> bool {
    if events.is_empty()
        || events.len() > MAX_EVENTS
        || events.iter().any(|event| {
            !event.validate()
                || event.execution_id != execution.execution_id
                || event.run_unit_id != execution.run_unit_id
                || event.attempt != execution.attempt
        })
        || events
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence || pair[0].tick > pair[1].tick)
    {
        return false;
    }
    let Some(terminal) = events.last() else {
        return false;
    };
    if terminal.sequence != execution.version
        || Some(terminal.tick) != execution.terminal_tick
        || terminal.kind != (LifecycleEventKind::Completed { return_code: 0 })
    {
        return false;
    }
    for sequence in reads.iter().copied().chain([terminal_sequence]) {
        let intent = events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.kind == (LifecycleEventKind::EffectIntent { sequence }))
            .collect::<Vec<_>>();
        let result = events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.kind == (LifecycleEventKind::EffectResult { sequence }))
            .collect::<Vec<_>>();
        if !matches!((intent.as_slice(), result.as_slice()), ([(i, _)], [(r, _)])
            if i < r && *r < events.len() - 1)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
fn audits_match(
    audits: &[AuditRecord],
    invocation: &Invocation,
    effects: &[(u64, AuditResourceDigest)],
    events: &[LifecycleEvent],
) -> bool {
    audits_match_with_terminal_admission(audits, invocation, effects, events, None)
}

fn audits_match_with_terminal_admission(
    audits: &[AuditRecord],
    invocation: &Invocation,
    effects: &[(u64, AuditResourceDigest)],
    events: &[LifecycleEvent],
    terminal_admission: Option<&TerminalAdmission>,
) -> bool {
    if terminal_admission.is_some_and(|admission| !admission.matches_actor(invocation)) {
        return false;
    }
    let mut admissions = 0;
    if audits.is_empty()
        || audits.len() > MAX_AUDITS
        || audits.iter().any(|audit| {
            audit.execution_id != invocation.execution_id
                || audit.run_unit_id != invocation.run_unit_id
                || audit.principal != *invocation.principal.id()
                || audit.attempt != invocation.attempt
                || audit.decision != AuditDecision::Success
                || if audit.invocation_key == invocation.idempotency_key {
                    // Every ordinary program audit retains the original key and
                    // positive-tick predicate, including nonselected capabilities.
                    audit.observed_tick == 0
                } else if terminal_admission.is_some_and(|admission| {
                    audit.invocation_key == admission.invocation_key
                        && audit.capability.as_str() == "host.security.authorize"
                        && audit.resource == admission.resource
                        && audit.effect_sequence == 1
                        && audit.observed_tick == 0
                }) {
                    admissions += 1;
                    false
                } else {
                    true
                }
        })
        || admissions != usize::from(terminal_admission.is_some())
    {
        return false;
    }
    effects.iter().all(|(sequence, resource)| {
        // Outer CICS effects and nested dataset audit sequence domains are separate.
        // Refuse duplicate/conflicting outer receipts rather than filtering them away.
        let selected = audits
            .iter()
            .filter(|audit| {
                audit.capability.as_str() == "host.cics.execute"
                    && audit.effect_sequence == *sequence
            })
            .collect::<Vec<_>>();
        let [audit] = selected.as_slice() else {
            return false;
        };
        let intent = events.iter().find(|event| {
            event.kind
                == (LifecycleEventKind::EffectIntent {
                    sequence: *sequence,
                })
        });
        let result = events.iter().find(|event| {
            event.kind
                == (LifecycleEventKind::EffectResult {
                    sequence: *sequence,
                })
        });
        matches!((intent, result), (Some(intent), Some(result))
            if audit.resource == *resource && audit.observed_tick >= intent.tick
                && audit.observed_tick <= result.tick)
    })
}

fn terminal_effect_matches(
    effect: &EffectRecord,
    invocation: &Invocation,
    terminal: &CicsReadCopybackEffect,
    terminal_tick: u64,
) -> bool {
    effect.execution_id == invocation.execution_id
        && effect.run_unit_id == invocation.run_unit_id
        && effect.sequence == terminal.sequence()
        && Some(&effect.key) == terminal.key()
        && effect.digest_format == EffectDigestFormat::CanonicalHostV1
        && effect.request_digest == *terminal.request_digest()
        && effect.result_digest == Some(*terminal.result_digest())
        && effect.state == EffectState::Completed
        && effect.intent.owner == invocation.execution_id
        && effect.intent.attempt == invocation.attempt
        && effect.intent.audit_invocation_key.as_ref() == Some(&invocation.idempotency_key)
        && effect.intent.audit_resource == Some(terminal.resource())
        && effect
            .intent
            .capability
            .as_ref()
            .is_some_and(|cap| cap.as_str() == "host.cics.execute")
        && effect.intent.recovery_lease.is_none()
        && effect.intent.created_tick > 0
        && effect
            .resolved_tick
            .is_some_and(|tick| tick >= effect.intent.created_tick && tick <= terminal_tick)
}

fn qualify(
    server: &ProductServer,
    invocation: &Invocation,
    state: &ArmState,
    terminal_admission: Option<&TerminalAdmission>,
    archive: CicsReadCopybackArchive,
) -> Result<CompletedReadCopyback, CicsReadObservationProblem> {
    let refuse = || CicsReadObservationProblem::Ownership;
    let origin = archive.origin();
    if origin.execution() != &invocation.execution_id
        || origin.run() != &invocation.run_unit_id
        || origin.selector() != &invocation.selector
        || origin.artifact() != &invocation.artifact
        || origin.principal() != invocation.principal.id()
        || origin.attempt() != invocation.attempt
        || origin.invocation_key() != &invocation.idempotency_key
        || origin.deadline() != invocation.deadline_tick
        || archive.reads().len() != state.profile.plan.read_count()
        || archive
            .reads()
            .iter()
            .any(|read| read.effect().key().is_some())
        || state.owner.get()
            != Some(&(
                invocation.execution_id.clone(),
                invocation.run_unit_id.clone(),
                invocation.idempotency_key.clone(),
                invocation.attempt,
            ))
    {
        return Err(refuse());
    }
    let record = server
        .artifacts
        .get_artifact(&invocation.artifact)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    let metadata = record.executable.as_ref().ok_or_else(refuse)?;
    if record.artifact != state.profile.artifact
        || admit_executable_artifact(&record).is_err()
        || metadata.semantic_identity != state.profile.semantic_identity
    {
        return Err(refuse());
    }
    let execution = server
        .store
        .get_execution(&invocation.execution_id)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    if !owner_matches(&execution, invocation) {
        return Err(refuse());
    }
    let tick = execution.terminal_tick.ok_or_else(refuse)?;
    let terminal = archive.terminal();
    let effect = server
        .store
        .effect(terminal.key().ok_or_else(refuse)?)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    if !terminal_effect_matches(&effect, invocation, terminal, tick) {
        return Err(refuse());
    }
    let row = server
        .store
        .get_provider_state("cics-effect-replay-v1", effect.key.as_str())
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    let replay = describe_cics_replay_row(&row, Some(&effect), CicsLimits::default())
        .map_err(|_| refuse())?;
    if replay.codec != CicsReplayCodecVersion::RetentionV3
        || replay.retention != CicsReplayRetentionState::Terminal
        || replay.owner_execution.as_deref() != Some(invocation.execution_id.as_str())
        || replay.owner_run_unit.as_deref() != Some(invocation.run_unit_id.as_str())
        || replay.sequence != Some(terminal.sequence())
        || replay.terminal_tick != replay.resolution_tick
        || replay.resolution_tick.is_none_or(|resolution| {
            resolution == 0
                || replay
                    .deadline_tick
                    .is_none_or(|lower| lower == 0 || lower > resolution)
        })
        || replay.request_digest != *terminal.request_digest()
        || replay.result_digest != *terminal.result_digest()
    {
        return Err(refuse());
    }
    let events = server
        .store
        .events(&invocation.execution_id, 0, MAX_EVENTS + 1)
        .map_err(|_| refuse())?;
    let sequences = archive
        .reads()
        .iter()
        .map(|read| read.effect().sequence())
        .collect::<Vec<_>>();
    if !events_match(&events, &execution, &sequences, terminal.sequence()) {
        return Err(refuse());
    }
    let audits = server
        .store
        .audit_records(&invocation.execution_id, 0, MAX_AUDITS + 1)
        .map_err(|_| refuse())?;
    let effects = archive
        .reads()
        .iter()
        .map(|read| read.effect())
        .chain([terminal])
        .map(|effect| (effect.sequence(), effect.resource()))
        .collect::<Vec<_>>();
    if !audits_match_with_terminal_admission(
        &audits,
        invocation,
        &effects,
        &events,
        terminal_admission,
    ) {
        return Err(refuse());
    }
    // A bounded non-atomic observation, never a durable ownership/authorization lease.
    // Refuse changed/disappearing current rows before exposing this ephemeral witness.
    if server
        .store
        .get_execution(&invocation.execution_id)
        .map_err(|_| refuse())?
        != Some(execution.clone())
        || server.store.effect(&effect.key).map_err(|_| refuse())? != Some(effect.clone())
        || server
            .store
            .get_provider_state(&row.namespace, &row.key)
            .map_err(|_| refuse())?
            != Some(row)
        || server
            .store
            .events(&invocation.execution_id, execution.version, 2)
            .map_err(|_| refuse())?
            != vec![events.last().ok_or_else(refuse)?.clone()]
    {
        return Err(refuse());
    }
    Ok(CompletedReadCopyback {
        session: state.profile.session.clone(),
        source_digest: state.profile.source_digest,
        payload_digest: record.payload_digest,
        semantic_identity: metadata.semantic_identity.clone(),
        terminal_version: execution.version,
        terminal_tick: tick,
        archive,
    })
}
