//! Optional same-server typed SQL storage witness; no HTTP or persisted archive.
//! The observer has no callbacks, authority grants, drive, retry or cleanup methods.
use super::*;
use mainframe_env_db2::{
    Db2Limits, Db2ReplayDependency, Db2ReplayRetentionDescriptor, Db2ReplayRetentionState,
    describe_db2_replay_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, AuditResourceDigest, LifecycleEvent,
};
use mainframe_env_host_api::Db2Operation;
use mainframe_env_interpreter::{Db2CopybackArchive, Db2CopybackPlan, Db2CopybackRecord};
use mainframe_env_store_api::{EffectRecord, ExecutionRecord};
use std::sync::OnceLock;

#[cfg(test)]
mod tests;

const MAX_AUDITS: usize = 512;
const MAX_EVENTS: usize = 1024;

/// Fixed reason codes only; no raw customer records, keys, bodies or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ObservationProblem {
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
pub struct SourceBoundDb2CopybackProfile {
    session: SessionId,
    selector: Selector,
    principal: PrincipalId,
    plan: Db2CopybackPlan,
    artifact: ArtifactRef,
    semantic_identity: String,
    source_digest: [u8; 32],
}

impl SourceBoundDb2CopybackProfile {
    pub fn new(
        session: SessionId,
        selector: Selector,
        principal: PrincipalId,
        artifact: ArtifactRef,
        semantic_identity: String,
        source_digest: [u8; 32],
        plan: Db2CopybackPlan,
    ) -> Result<Self, Db2ObservationProblem> {
        if semantic_identity.is_empty()
            || semantic_identity.len() > 128
            || selector
                .as_str()
                .strip_prefix("program:")
                .is_none_or(str::is_empty)
        {
            return Err(Db2ObservationProblem::Profile);
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
pub struct CompletedDb2Copyback {
    archive: Db2CopybackArchive,
    session: SessionId,
    source_digest: [u8; 32],
    payload_digest: [u8; 32],
    semantic_identity: String,
    terminal_version: u64,
    terminal_tick: u64,
}

impl CompletedDb2Copyback {
    pub fn archive(&self) -> &Db2CopybackArchive {
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
    profile: SourceBoundDb2CopybackProfile,
    owner: OnceLock<(ExecutionId, RunUnitId, IdempotencyKey, u32)>,
    result: OnceLock<Result<CompletedDb2Copyback, Db2ObservationProblem>>,
}

/// Same exact server retained, never reconstructed from strings or persisted rows.
pub struct Db2ObserverManager {
    server: Arc<ProductServer>,
}

/// Pure completed observation handle; no resume or re-dispatch operation.
pub struct ArmedDb2CopybackObservation {
    state: Arc<ArmState>,
}

impl ArmedDb2CopybackObservation {
    pub fn completed(&self) -> Result<&CompletedDb2Copyback, Db2ObservationProblem> {
        match self.state.result.get() {
            Some(Ok(complete)) => Ok(complete),
            Some(Err(problem)) => Err(*problem),
            None => Err(Db2ObservationProblem::Pending),
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
    pub fn install_db2_copyback_observer(
        self: &Arc<Self>,
    ) -> Result<Db2ObserverManager, Db2ObservationProblem> {
        let mut registry = self
            .db2_copyback
            .lock()
            .map_err(|_| Db2ObservationProblem::Ownership)?;
        if registry.machine_binding_started {
            return Err(Db2ObservationProblem::LateInstall);
        }
        if registry.installed {
            return Err(Db2ObservationProblem::AlreadyInstalled);
        }
        registry.installed = true;
        Ok(Db2ObserverManager {
            server: self.clone(),
        })
    }

    pub(super) fn bind_db2_copyback_observation(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        record: &ArtifactRecord,
        invocation: &Invocation,
    ) -> Option<BoundObservation> {
        // Clone selection under one short lock. No callbacks/store/drive/clock run here.
        let state = {
            let mut registry = self.db2_copyback.lock().ok()?;
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
        let valid = principal == &state.profile.principal
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
            let _ = state.result.set(Err(Db2ObservationProblem::Profile));
            return None;
        }
        let owner = (
            invocation.execution_id.clone(),
            invocation.run_unit_id.clone(),
            invocation.idempotency_key.clone(),
            invocation.attempt,
        );
        if state.owner.set(owner).is_err() {
            let _ = state.result.set(Err(Db2ObservationProblem::Ambiguous));
            return None;
        }
        Some(BoundObservation {
            state,
            finished: false,
        })
    }
}

impl Db2ObserverManager {
    /// Arm one next actual matching exchange. Caller data remains assertions only.
    pub fn arm(
        &self,
        profile: SourceBoundDb2CopybackProfile,
    ) -> Result<ArmedDb2CopybackObservation, Db2ObservationProblem> {
        // Do all ordinary read-only artifact validation outside the registry lock.
        let selected = self
            .server
            .online_programs
            .lock()
            .map_err(|_| Db2ObservationProblem::Ownership)?
            .get(
                profile
                    .selector
                    .as_str()
                    .strip_prefix("program:")
                    .ok_or(Db2ObservationProblem::Profile)?,
            )
            .cloned()
            .ok_or(Db2ObservationProblem::Profile)?;
        let record = self
            .server
            .artifacts
            .get_artifact(&selected)
            .map_err(|_| Db2ObservationProblem::Ownership)?
            .ok_or(Db2ObservationProblem::Profile)?;
        if selected != profile.artifact
            || record.artifact != selected
            || admit_executable_artifact(&record).is_err()
            || record
                .executable
                .as_ref()
                .is_none_or(|metadata| metadata.semantic_identity != profile.semantic_identity)
        {
            return Err(Db2ObservationProblem::Profile);
        }
        let state = Arc::new(ArmState {
            profile,
            owner: OnceLock::new(),
            result: OnceLock::new(),
        });
        let mut registry = self
            .server
            .db2_copyback
            .lock()
            .map_err(|_| Db2ObservationProblem::Ownership)?;
        if !registry.installed || registry.arm.is_some() {
            return Err(Db2ObservationProblem::AlreadyArmed);
        }
        registry.arm = Some(state.clone());
        Ok(ArmedDb2CopybackObservation { state })
    }
}

pub(super) struct BoundObservation {
    state: Arc<ArmState>,
    finished: bool,
}

impl Drop for BoundObservation {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.state.result.set(Err(Db2ObservationProblem::Cleanup));
        }
    }
}

impl BoundObservation {
    pub(super) fn enable(&self, machine: &mut ReferenceMachine) {
        if machine
            .enable_db2_copyback(self.state.profile.plan.clone())
            .is_err()
        {
            let _ = self
                .state
                .result
                .set(Err(Db2ObservationProblem::Application));
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
            .take_db2_copyback()
            .ok_or(Db2ObservationProblem::Application)
            .and_then(|archive| archive.map_err(|_| Db2ObservationProblem::Application))
            .and_then(|archive| qualify(server, invocation, &self.state, archive));
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

#[derive(Clone, Copy)]
struct StatementIdentity<'a> {
    operation: Db2Operation,
    sequence: u64,
    resource: AuditResourceDigest,
    key: Option<&'a IdempotencyKey>,
    request: &'a [u8; 32],
    result: &'a [u8; 32],
}

impl<'a> From<&'a Db2CopybackRecord> for StatementIdentity<'a> {
    fn from(record: &'a Db2CopybackRecord) -> Self {
        Self {
            operation: record.operation(),
            sequence: record.sequence(),
            resource: record.resource(),
            key: record.key(),
            request: record.request_digest(),
            result: record.result_digest(),
        }
    }
}

fn capability(operation: Db2Operation) -> &'static str {
    if operation.is_mutating() {
        "host.db2.write"
    } else {
        "host.db2.read"
    }
}

fn events_match(events: &[LifecycleEvent], execution: &ExecutionRecord, selected: &[u64]) -> bool {
    if events.is_empty()
        || events.len() > MAX_EVENTS
        || selected.is_empty()
        || selected.len() > 64
        || selected[0] == 0
        || selected.windows(2).any(|pair| pair[0] >= pair[1])
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
    selected.iter().all(|sequence| {
        let intents = events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.kind
                    == (LifecycleEventKind::EffectIntent {
                        sequence: *sequence,
                    })
            })
            .collect::<Vec<_>>();
        let results = events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.kind
                    == (LifecycleEventKind::EffectResult {
                        sequence: *sequence,
                    })
            })
            .collect::<Vec<_>>();
        matches!((intents.as_slice(), results.as_slice()), ([(i, _)], [(r, _)])
            if i < r && *r < events.len() - 1)
    })
}

fn audits_match(
    audits: &[AuditRecord],
    invocation: &Invocation,
    statements: &[StatementIdentity<'_>],
    events: &[LifecycleEvent],
) -> bool {
    if audits.is_empty()
        || audits.len() > MAX_AUDITS
        || audits.iter().any(|audit| {
            audit.execution_id != invocation.execution_id
                || audit.run_unit_id != invocation.run_unit_id
                || audit.principal != *invocation.principal.id()
                || audit.attempt != invocation.attempt
                || audit.invocation_key != invocation.idempotency_key
                || audit.observed_tick == 0
                || audit.decision != AuditDecision::Success
        })
    {
        return false;
    }
    statements.iter().all(|statement| {
        // SQL capabilities share one domain; incidental CICS/dataset domains remain separate.
        let selected = audits
            .iter()
            .filter(|audit| {
                matches!(
                    audit.capability.as_str(),
                    "host.db2.read" | "host.db2.write"
                ) && audit.effect_sequence == statement.sequence
            })
            .collect::<Vec<_>>();
        let [audit] = selected.as_slice() else {
            return false;
        };
        let intent = events.iter().find(|event| {
            event.kind
                == (LifecycleEventKind::EffectIntent {
                    sequence: statement.sequence,
                })
        });
        let result = events.iter().find(|event| {
            event.kind
                == (LifecycleEventKind::EffectResult {
                    sequence: statement.sequence,
                })
        });
        matches!((intent, result), (Some(intent), Some(result))
            if audit.capability.as_str() == capability(statement.operation)
                && audit.resource == statement.resource
                && audit.observed_tick >= intent.tick && audit.observed_tick <= result.tick)
    })
}

fn core_matches(
    core: &EffectRecord,
    invocation: &Invocation,
    statement: &StatementIdentity<'_>,
    terminal_tick: u64,
) -> bool {
    statement.operation.is_mutating()
        && statement.key == Some(&core.key)
        && core.execution_id == invocation.execution_id
        && core.run_unit_id == invocation.run_unit_id
        && core.sequence == statement.sequence
        && core.digest_format == EffectDigestFormat::CanonicalHostV1
        && core.request_digest == *statement.request
        && core.result_digest == Some(*statement.result)
        && core.state == EffectState::Completed
        && core.intent.owner == invocation.execution_id
        && core.intent.attempt == invocation.attempt
        && core.intent.audit_invocation_key.as_ref() == Some(&invocation.idempotency_key)
        && core.intent.audit_resource == Some(statement.resource)
        && core
            .intent
            .capability
            .as_ref()
            .is_some_and(|cap| cap.as_str() == capability(statement.operation))
        && core.intent.recovery_lease.is_none()
        && core.intent.created_tick > 0
        && core
            .resolved_tick
            .is_some_and(|tick| tick >= core.intent.created_tick && tick <= terminal_tick)
}

fn replay_matches(
    replay: &Db2ReplayRetentionDescriptor,
    invocation: &Invocation,
    statement: &StatementIdentity<'_>,
) -> bool {
    statement.operation.is_mutating()
        && statement.key.is_some_and(|key| key.as_str() == replay.key)
        && replay.namespace == "db2-v1-replay" && replay.row_version > 0
        && replay.retention == Db2ReplayRetentionState::Terminal
        && replay.dependency == Some(Db2ReplayDependency::CoreEffect)
        && replay.outer_effect_key.is_none()
        && replay.owner_execution.as_deref() == Some(invocation.execution_id.as_str())
        && replay.owner_run_unit.as_deref() == Some(invocation.run_unit_id.as_str())
        && replay.sequence == Some(statement.sequence)
        && replay.request_digest == *statement.request && replay.result_digest == *statement.result
        && replay.recorded_deadline_tick == Some(invocation.deadline_tick)
        // Existing provider deadline projection is separate from actual core/actor ticks.
        && replay.resolution_tick.is_some_and(|tick| tick > 0 && tick >= invocation.deadline_tick)
        && replay.terminal_tick == replay.resolution_tick
}

fn qualify(
    server: &ProductServer,
    invocation: &Invocation,
    state: &ArmState,
    archive: Db2CopybackArchive,
) -> Result<CompletedDb2Copyback, Db2ObservationProblem> {
    let refuse = || Db2ObservationProblem::Ownership;
    let origin = archive.origin();
    if origin.execution() != &invocation.execution_id
        || origin.run() != &invocation.run_unit_id
        || origin.selector() != &invocation.selector
        || origin.artifact() != &invocation.artifact
        || origin.principal() != invocation.principal.id()
        || origin.attempt() != invocation.attempt
        || origin.invocation_key() != &invocation.idempotency_key
        || origin.deadline() != invocation.deadline_tick
        || state.owner.get()
            != Some(&(
                invocation.execution_id.clone(),
                invocation.run_unit_id.clone(),
                invocation.idempotency_key.clone(),
                invocation.attempt,
            ))
        || archive.records().is_empty()
        || archive.records().len() > 64
        || archive.records().iter().any(|record| {
            record.operation().is_mutating() != record.key().is_some()
                || record.sequence() > archive.final_effect_sequence()
        })
    {
        return Err(refuse());
    }
    let artifact = server
        .artifacts
        .get_artifact(&invocation.artifact)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    let metadata = artifact.executable.as_ref().ok_or_else(refuse)?;
    if artifact.artifact != state.profile.artifact
        || admit_executable_artifact(&artifact).is_err()
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
    let statements = archive
        .records()
        .iter()
        .map(StatementIdentity::from)
        .collect::<Vec<_>>();
    let sequences = statements
        .iter()
        .map(|statement| statement.sequence)
        .collect::<Vec<_>>();
    let events = server
        .store
        .events(&invocation.execution_id, 0, MAX_EVENTS + 1)
        .map_err(|_| refuse())?;
    if !events_match(&events, &execution, &sequences) {
        return Err(refuse());
    }
    let audits = server
        .store
        .audit_records(&invocation.execution_id, 0, MAX_AUDITS + 1)
        .map_err(|_| refuse())?;
    if !audits_match(&audits, invocation, &statements, &events) {
        return Err(refuse());
    }
    let mut retained = Vec::with_capacity(statements.len());
    for statement in &statements {
        let Some(key) = statement.key else {
            // SELECT/COUNT/EXTRACT have real events/audits, not fabricated core receipts.
            continue;
        };
        let core = server
            .store
            .effect(key)
            .map_err(|_| refuse())?
            .ok_or_else(refuse)?;
        if !core_matches(&core, invocation, statement, tick) {
            return Err(refuse());
        }
        let row = server
            .store
            .get_provider_state("db2-v1-replay", key.as_str())
            .map_err(|_| refuse())?
            .ok_or_else(refuse)?;
        let replay = describe_db2_replay_row(&row, Some(&core), Db2Limits::default())
            .map_err(|_| refuse())?;
        if !replay_matches(&replay, invocation, statement) {
            return Err(refuse());
        }
        retained.push((core, row));
    }
    // Bounded non-atomic observation, never an authorization/ownership lease.
    for (core, row) in &retained {
        if server
            .store
            .effect(&core.key)
            .map_err(|_| refuse())?
            .as_ref()
            != Some(core)
            || server
                .store
                .get_provider_state(&row.namespace, &row.key)
                .map_err(|_| refuse())?
                .as_ref()
                != Some(row)
        {
            return Err(refuse());
        }
    }
    if server
        .store
        .get_execution(&invocation.execution_id)
        .map_err(|_| refuse())?
        != Some(execution.clone())
        || server
            .store
            .events(&invocation.execution_id, execution.version, 2)
            .map_err(|_| refuse())?
            != vec![events.last().ok_or_else(refuse)?.clone()]
        || server
            .artifacts
            .get_artifact(&invocation.artifact)
            .map_err(|_| refuse())?
            != Some(artifact.clone())
    {
        return Err(refuse());
    }
    Ok(CompletedDb2Copyback {
        session: state.profile.session.clone(),
        source_digest: state.profile.source_digest,
        payload_digest: artifact.payload_digest,
        semantic_identity: metadata.semantic_identity.clone(),
        terminal_version: execution.version,
        terminal_tick: tick,
        archive,
    })
}
