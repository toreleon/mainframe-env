//! Passive first-party conformance capture of actual, successfully committed effects.
//! This adapter never supplies clocks, deadlines, cancellation, leases or host results.
use super::*;
use mainframe_env_execution_api::{AuditRecord, LifecycleEvent, LifecycleEventKind};
use mainframe_env_store_api::*;
use std::sync::Mutex;

#[path = "overlap_store_ports.rs"]
mod overlap_store_ports;

const MAX_COMMITS: usize = 4096;

#[derive(Clone)]
struct Committed {
    ordinal: usize,
    event: LifecycleEvent,
    execution: ExecutionRecord,
    effect: Option<EffectRecord>,
}

#[derive(Default)]
struct Capture {
    armed: bool,
    incomplete: bool,
    commits: Vec<Committed>,
}

pub(super) struct ObservedStore {
    inner: Arc<MemoryStore>,
    capture: Mutex<Capture>,
}

impl ObservedStore {
    pub(super) fn new(inner: Arc<MemoryStore>) -> Self {
        Self {
            inner,
            capture: Mutex::new(Capture::default()),
        }
    }

    /// Arm once, before offering workloads; clearing/rearming cannot erase uncertainty.
    pub(super) fn arm(&self) -> Result<(), CorpusProblem> {
        let mut capture = self.capture.lock().map_err(|_| refused())?;
        if capture.armed || capture.incomplete || !capture.commits.is_empty() {
            return Err(refused());
        }
        capture.armed = true;
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "exact forwarding of the existing journal port"
    )]
    fn observe_commit(
        &self,
        execution_id: &ExecutionId,
        expected_version: u64,
        next_state: Option<ExecutionState>,
        event: LifecycleEvent,
        effect: Option<EffectRecord>,
        audit: Option<AuditRecord>,
        checkpoint: Option<CheckpointRecord>,
        notification: OutboxRecord,
    ) -> Result<ExecutionRecord, StoreError> {
        // The lock covers the real commit and its capture. A post-return-only
        // observer could reorder raced commits into a fictitious interleaving.
        // MemoryStore does not callback into this wrapper. No observer callback,
        // asynchronous wait, sleep, host call or forwarding-port reentry occurs.
        let mut capture = self.capture.lock().ok();
        let selected = capture.as_ref().is_some_and(|capture| capture.armed);
        let retained = selected.then(|| (event.clone(), effect.clone()));
        let result = self.inner.commit_execution_step(
            execution_id,
            expected_version,
            next_state,
            event,
            effect,
            audit,
            checkpoint,
            notification,
        );
        if let Some(capture) = capture.as_mut()
            && capture.armed
        {
            match (&result, retained) {
                (Ok(execution), Some((event, effect))) if capture.commits.len() < MAX_COMMITS => {
                    let ordinal = capture.commits.len() + 1;
                    capture.commits.push(Committed {
                        ordinal,
                        event,
                        execution: execution.clone(),
                        effect,
                    });
                }
                // Errors, overflow and lost capture never change the actual
                // backend result. They only prevent this owner granting proof.
                _ => capture.incomplete = true,
            }
        }
        result
    }

    fn unobserved_lifecycle<T>(
        &self,
        operation: impl FnOnce(&MemoryStore) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        // These ports can change lifecycle state without a captured journal row.
        // Serialize the actual operation with captured commits and conservatively
        // refuse proof after success. Preserve every backend result unchanged.
        let mut capture = self.capture.lock().ok();
        let result = operation(self.inner.as_ref());
        if let Some(capture) = capture.as_mut()
            && capture.armed
            && result.is_ok()
        {
            capture.incomplete = true;
        }
        result
    }

    pub(super) fn batch_effect_seen(&self, artifact: &ArtifactRef) -> Result<bool, CorpusProblem> {
        let capture = self.capture.lock().map_err(|_| refused())?;
        if !capture.armed || capture.incomplete {
            return Err(refused());
        }
        Ok(capture.commits.iter().any(|commit| {
            successful_effect(commit)
                && commit.execution.selector.as_str() == "program:CBCUS01C"
                && &commit.execution.artifact == artifact
                && commit.execution.principal.as_str() == "IBMUSER"
        }))
    }

    pub(super) fn prove_overlap(
        &self,
        batch_artifact: &ArtifactRef,
        online_artifact: &ArtifactRef,
        online_run: &RunUnitId,
    ) -> Result<OverlapWitness, CorpusProblem> {
        let capture = self.capture.lock().map_err(|_| refused())?;
        prove(&capture, batch_artifact, online_artifact, online_run)
    }

    pub(super) fn export(&self) -> Result<serde_json::Value, CorpusProblem> {
        let capture = self.capture.lock().map_err(|_| refused())?;
        Ok(serde_json::json!({
            "armed":capture.armed, "incomplete":capture.incomplete,
            "limit":MAX_COMMITS,
            "commits":capture.commits.iter().map(|commit| serde_json::json!({
                "ordinal":commit.ordinal,
                "event":{
                    "execution":commit.event.execution_id.as_str(),
                    "run":commit.event.run_unit_id.as_str(), "attempt":commit.event.attempt,
                    "sequence":commit.event.sequence, "tick":commit.event.tick,
                    "kind":format!("{:?}", commit.event.kind),
                },
                "execution":{
                    "id":commit.execution.execution_id.as_str(), "run":commit.execution.run_unit_id.as_str(),
                    "attempt":commit.execution.attempt, "version":commit.execution.version,
                    "state":format!("{:?}",commit.execution.state),
                    "selector":commit.execution.selector.as_str(), "artifact":commit.execution.artifact.as_str(),
                    "principal":commit.execution.principal.as_str(),
                },
                "effect":commit.effect.as_ref().map(|effect| serde_json::json!({
                    "execution":effect.execution_id.as_str(), "run":effect.run_unit_id.as_str(),
                    "sequence":effect.sequence, "key":effect.key.as_str(),
                    "owner":effect.intent.owner.as_str(), "attempt":effect.intent.attempt,
                    "capability":effect.intent.capability.as_ref().map(CapabilityId::as_str),
                    "state":format!("{:?}",effect.state), "format":format!("{:?}",effect.digest_format),
                    "request_digest":effect.request_digest, "result_digest":effect.result_digest,
                    "created_tick":effect.intent.created_tick, "resolved_tick":effect.resolved_tick,
                })),
            })).collect::<Vec<_>>(),
        }))
    }
}

#[derive(Debug)]
pub(super) struct OverlapWitness {
    pub(super) batch_execution: String,
    pub(super) batch_run: String,
    pub(super) online_execution: String,
    pub(super) online_run: String,
    pub(super) ordinals: [usize; 3],
    pub(super) batch_effect_sequences: [u64; 2],
    pub(super) online_effect_sequence: u64,
}

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.full.compiled_overlap_unproved",
        "complete ordered effects of exact compiled batch/online actors do not prove overlap",
    )
}

fn successful_effect(commit: &Committed) -> bool {
    let LifecycleEventKind::EffectResult { sequence } = commit.event.kind else {
        return false;
    };
    let Some(effect) = &commit.effect else {
        return false;
    };
    commit.event.validate()
        && commit.execution.state == ExecutionState::Running
        && commit.event.execution_id == commit.execution.execution_id
        && commit.event.run_unit_id == commit.execution.run_unit_id
        && commit.event.attempt == commit.execution.attempt
        && effect.execution_id == commit.execution.execution_id
        && effect.run_unit_id == commit.execution.run_unit_id
        && effect.intent.owner == commit.execution.execution_id
        && effect.intent.attempt == commit.execution.attempt
        && effect.sequence == sequence
        && effect.state == EffectState::Completed
        && effect.digest_format == EffectDigestFormat::CanonicalHostV1
        && effect.result_digest.is_some()
        && effect.resolved_tick.is_some_and(|tick| tick > 0)
}

fn prove(
    capture: &Capture,
    batch_artifact: &ArtifactRef,
    online_artifact: &ArtifactRef,
    online_run: &RunUnitId,
) -> Result<OverlapWitness, CorpusProblem> {
    if !capture.armed
        || capture.incomplete
        || capture.commits.len() > MAX_COMMITS
        || capture
            .commits
            .iter()
            .enumerate()
            .any(|(index, commit)| commit.ordinal != index + 1)
    {
        return Err(refused());
    }
    let batch = |commit: &Committed| {
        successful_effect(commit)
            && &commit.execution.artifact == batch_artifact
            && commit.execution.selector.as_str() == "program:CBCUS01C"
            && commit.execution.principal.as_str() == "IBMUSER"
            && commit.effect.as_ref().is_some_and(|effect| {
                effect.intent.capability.as_ref().map(CapabilityId::as_str)
                    == Some("host.dataset.read")
            })
    };
    let online = |commit: &Committed| {
        successful_effect(commit)
            && &commit.execution.artifact == online_artifact
            && &commit.execution.run_unit_id == online_run
            && commit.execution.selector.as_str() == "program:COACTVWC"
            && commit.execution.principal.as_str() == "WEBUSER"
            && commit.effect.as_ref().is_some_and(|effect| {
                effect.intent.capability.as_ref().map(CapabilityId::as_str)
                    == Some("host.cics.execute")
            })
    };
    if batch_artifact == online_artifact {
        return Err(refused());
    }
    // The offered campaign has sixteen real batch identities. At most sixteen
    // candidates and one retained middle marker per candidate keep this scan
    // bounded by O(commits * 16), independent of the number of online effects.
    let mut candidates = BTreeMap::<String, (&Committed, Option<&Committed>)>::new();
    for commit in &capture.commits {
        let identity = commit.execution.execution_id.as_str();
        if let Some((before, _)) = candidates.get(identity)
            && (commit.execution.state != ExecutionState::Running
                || commit.execution.run_unit_id != before.execution.run_unit_id
                || commit.execution.attempt != before.execution.attempt
                || commit.execution.artifact != *batch_artifact
                || commit.execution.selector != before.execution.selector
                || commit.execution.principal != before.execution.principal
                || matches!(
                    commit.event.kind,
                    LifecycleEventKind::Suspended
                        | LifecycleEventKind::Resumed
                        | LifecycleEventKind::HandoffCompleted
                ))
        {
            candidates.remove(identity);
        }
        if online(commit) {
            for (before, middle) in candidates.values_mut() {
                if before.execution.execution_id != commit.execution.execution_id
                    && before.execution.run_unit_id != *online_run
                {
                    *middle = Some(commit);
                }
            }
        }
        if !batch(commit) {
            continue;
        }
        if let Some((before, Some(middle))) = candidates.get(identity) {
            let before_effect = before.effect.as_ref().ok_or_else(refused)?;
            let after_effect = commit.effect.as_ref().ok_or_else(refused)?;
            if before.execution.run_unit_id == commit.execution.run_unit_id
                && before.execution.attempt == commit.execution.attempt
                && before.event.sequence < commit.event.sequence
                && before_effect.sequence < after_effect.sequence
            {
                return Ok(OverlapWitness {
                    batch_execution: identity.into(),
                    batch_run: before.execution.run_unit_id.as_str().into(),
                    online_execution: middle.execution.execution_id.as_str().into(),
                    online_run: online_run.as_str().into(),
                    ordinals: [before.ordinal, middle.ordinal, commit.ordinal],
                    batch_effect_sequences: [before_effect.sequence, after_effect.sequence],
                    online_effect_sequence: middle.effect.as_ref().ok_or_else(refused)?.sequence,
                });
            }
        }
        if !candidates.contains_key(identity) {
            if candidates.len() == 16 {
                return Err(refused());
            }
            candidates.insert(identity.into(), (commit, None));
        }
    }
    Err(refused())
}

#[cfg(test)]
#[path = "overlap_store_tests.rs"]
mod tests;
