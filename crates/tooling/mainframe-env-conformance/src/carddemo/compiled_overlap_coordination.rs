//! Active test scheduling, separate from passive capture and the unchanged proof.
//! A release is never evidence of overlap. No production clock or host result changes.
use super::*;
use mainframe_env_execution_api::{AuditRecord, LifecycleEvent, LifecycleEventKind};
use mainframe_env_store_api::*;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

#[path = "compiled_overlap_coordination_ports.rs"]
mod ports;

#[derive(Clone)]
struct Marker {
    execution: String,
    run: String,
    attempt: u32,
    sequence: u64,
}

#[derive(Default)]
struct State {
    deadline: Option<Instant>,
    batch: Option<Marker>,
    online: Option<Marker>,
    released: bool,
    refusal: Option<&'static str>,
}

pub(super) struct Coordination {
    batch_artifact: ArtifactRef,
    online_artifact: ArtifactRef,
    state: Mutex<State>,
    release: Condvar,
    ready: tokio::sync::Notify,
    faulty: AtomicBool,
}

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.full.compiled_overlap_coordination",
        "explicit effect coordination did not finish; original overlap proof remains required",
    )
}

fn marker(
    event: &LifecycleEvent,
    effect: Option<&EffectRecord>,
    execution: &ExecutionRecord,
) -> Option<Marker> {
    let LifecycleEventKind::EffectResult { sequence } = event.kind else {
        return None;
    };
    let effect = effect?;
    (event.validate()
        && execution.state == ExecutionState::Running
        && event.execution_id == execution.execution_id
        && event.run_unit_id == execution.run_unit_id
        && event.attempt == execution.attempt
        && effect.execution_id == execution.execution_id
        && effect.run_unit_id == execution.run_unit_id
        && effect.intent.owner == execution.execution_id
        && effect.intent.attempt == execution.attempt
        && effect.sequence == sequence
        && effect.state == EffectState::Completed
        && effect.digest_format == EffectDigestFormat::CanonicalHostV1
        && effect.result_digest.is_some()
        && effect.resolved_tick.is_some_and(|tick| tick > 0))
    .then(|| Marker {
        execution: execution.execution_id.as_str().into(),
        run: execution.run_unit_id.as_str().into(),
        attempt: execution.attempt,
        sequence,
    })
}

impl Coordination {
    pub(super) fn new(batch_artifact: ArtifactRef, online_artifact: ArtifactRef) -> Arc<Self> {
        Arc::new(Self {
            batch_artifact,
            online_artifact,
            state: Mutex::new(State::default()),
            release: Condvar::new(),
            ready: tokio::sync::Notify::new(),
            faulty: AtomicBool::new(false),
        })
    }

    pub(super) fn arm(&self, original_deadline: Instant) -> Result<(), CorpusProblem> {
        let mut state = self.state.lock().map_err(|_| refused())?;
        if state.deadline.is_some() || state.refusal.is_some() {
            return Err(refused());
        }
        state.deadline = Some(original_deadline);
        Ok(())
    }

    // Called ONLY after the complete passive adapter call returns. It owns no
    // MemoryStore/capture/provider/registry/Batch state guard. The ordinary
    // caller guard proof is retained with this proposal; opt-in paths are unused.
    fn after_commit(
        &self,
        event: &LifecycleEvent,
        effect: Option<&EffectRecord>,
        result: &Result<ExecutionRecord, StoreError>,
    ) {
        let Ok(execution) = result else { return };
        let Ok(mut state) = self.state.lock() else {
            self.faulty.store(true, Ordering::SeqCst);
            self.release.notify_all();
            return;
        };
        let Some(deadline) = state.deadline else {
            return;
        };
        if state.refusal.is_some() || state.released {
            return;
        }
        if state.batch.as_ref().is_some_and(|batch| {
            batch.execution == execution.execution_id.as_str()
                && (execution.state != ExecutionState::Running
                    || batch.run != execution.run_unit_id.as_str()
                    || batch.attempt != execution.attempt)
        }) {
            state.refusal = Some("selected-batch-retired-or-changed");
            state.released = true;
            self.release.notify_all();
            return;
        }
        let Some(candidate) = marker(event, effect, execution) else {
            return;
        };
        let capability = effect.and_then(|value| value.intent.capability.as_ref());
        let batch = execution.artifact == self.batch_artifact
            && execution.selector.as_str() == "program:CBCUS01C"
            && execution.principal.as_str() == "IBMUSER"
            && capability.map(CapabilityId::as_str) == Some("host.dataset.read");
        let online = execution.artifact == self.online_artifact
            && execution.selector.as_str() == "program:COACTVWC"
            && execution.principal.as_str() == "WEBUSER"
            && capability.map(CapabilityId::as_str) == Some("host.cics.execute");
        if online
            && state.batch.as_ref().is_some_and(|batch| {
                batch.execution != candidate.execution && batch.run != candidate.run
            })
        {
            state.online = Some(candidate);
            state.released = true;
            self.release.notify_all();
            return;
        }
        if !batch || state.batch.is_some() {
            return;
        }
        state.batch = Some(candidate);
        self.ready.notify_one();
        // At most ONE real batch thread waits. Condvar releases the sole
        // private scheduling mutex. Other jobs and actual online commits run.
        while !state.released && state.refusal.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.refusal = Some("original-completion-deadline");
                state.released = true;
                break;
            }
            match self.release.wait_timeout(state, remaining) {
                Ok((next, _)) => state = next,
                Err(_) => {
                    self.faulty.store(true, Ordering::SeqCst);
                    return;
                }
            }
        }
    }

    pub(super) async fn wait_for_boundary(
        &self,
        original_deadline: tokio::time::Instant,
    ) -> Result<(), CorpusProblem> {
        loop {
            let notified = self.ready.notified();
            {
                let state = self.state.lock().map_err(|_| refused())?;
                if self.faulty.load(Ordering::SeqCst) || state.refusal.is_some() {
                    return Err(refused());
                }
                if state.batch.is_some() {
                    return Ok(());
                }
            }
            tokio::time::timeout_at(original_deadline, notified)
                .await
                .map_err(|_| refused())?;
        }
    }

    fn abort(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !state.released {
                state.refusal.get_or_insert("driver-error-panic-or-drop");
                state.released = true;
            }
        } else {
            self.faulty.store(true, Ordering::SeqCst);
        }
        self.release.notify_all();
        self.ready.notify_one();
    }

    pub(super) fn release_on_drop(self: &Arc<Self>) -> ReleaseOnDrop {
        ReleaseOnDrop {
            owner: self.clone(),
            armed: true,
        }
    }

    // Same original Future/result, no new task, retry, poll or backend result.
    // Err/unwind/drop releases before the original all-joins interpretation.
    pub(super) fn guard_result<T, E>(
        self: Arc<Self>,
        future: impl Future<Output = Result<T, E>>,
    ) -> impl Future<Output = Result<T, E>> {
        // Eager construction also covers an offered future dropped unpolled.
        let mut release = self.release_on_drop();
        async move {
            let result = future.await;
            if result.is_ok() {
                release.armed = false;
            }
            drop(release);
            result
        }
    }

    pub(super) fn finish(&self) -> Result<(), CorpusProblem> {
        let state = self.state.lock().map_err(|_| refused())?;
        if self.faulty.load(Ordering::SeqCst)
            || state.refusal.is_some()
            || !state.released
            || state.batch.is_none()
            || state.online.is_none()
        {
            return Err(refused());
        }
        Ok(())
    }

    pub(super) fn export(&self) -> serde_json::Value {
        match self.state.lock() {
            Ok(state) => serde_json::json!({
                "schema":"mainframe-env.compiled-overlap-explicit-coordination@1",
                "scope":"Active coordinated schedule, never passive/fairness/performance proof",
                "batch":state.batch.as_ref().map(|value| serde_json::json!({
                    "execution":value.execution,"run":value.run,
                    "attempt":value.attempt,"effect_sequence":value.sequence})),
                "online":state.online.as_ref().map(|value| serde_json::json!({
                    "execution":value.execution,"run":value.run,
                    "attempt":value.attempt,"effect_sequence":value.sequence})),
                "released":state.released,"refusal":state.refusal,
                "faulty":self.faulty.load(Ordering::SeqCst),
                "release_grants_witness":false,"overlap_acceptance_credit":0,
            }),
            Err(_) => {
                serde_json::json!({"coordination_refused":"poisoned","overlap_acceptance_credit":0})
            }
        }
    }
}

pub(super) struct ReleaseOnDrop {
    owner: Arc<Coordination>,
    armed: bool,
}
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.owner.abort();
        }
    }
}

pub(super) struct CoordinatedStore {
    inner: Arc<ObservedStore>,
    coordination: Arc<Coordination>,
}
impl CoordinatedStore {
    pub(super) fn new(inner: Arc<ObservedStore>, coordination: Arc<Coordination>) -> Self {
        Self {
            inner,
            coordination,
        }
    }
    // Exact outer delegation: inner ObservedStore retains its own conservative
    // unobserved-lifecycle refusal. No default-method fallback or new authority.
    fn unobserved_lifecycle<T>(
        &self,
        operation: impl FnOnce(&ObservedStore) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        operation(self.inner.as_ref())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "exact original journal forwarding"
    )]
    fn observe_commit(
        &self,
        execution_id: &ExecutionId,
        expected_version: u64,
        next_state: Option<ExecutionState>,
        event: LifecycleEvent,
        effect: Option<EffectRecord>,
        audit: Option<mainframe_env_execution_api::AuditRecord>,
        checkpoint: Option<CheckpointRecord>,
        notification: OutboxRecord,
    ) -> Result<ExecutionRecord, StoreError> {
        let offered_event = event.clone();
        let offered_effect = effect.clone();
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
        self.coordination
            .after_commit(&offered_event, offered_effect.as_ref(), &result);
        result
    }
}

#[cfg(test)]
#[path = "compiled_overlap_coordination_tests.rs"]
mod tests;
