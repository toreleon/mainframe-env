//! Durable recovery metadata proposals. The caller publishes these mutations
//! with its database/TM and UOW writes through the shared atomic row store.

use super::contracts::{
    BackoutPointKind, CheckpointImage, CheckpointKind, CheckpointRequest, LogRequest,
    RecoveryContext, RecoveryLimits, RecoveryProblem, RepositionStatus, RestartPcbStatus,
    RestartSelection, SavedPcbPosition,
};
use mainframe_env_execution_api::IdempotencyKey;
use mainframe_env_store_api::{
    EffectState, IdempotencyStore, ProviderStateMutation, ProviderStateRecord, ProviderStateStore,
    ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const ROW_NAMESPACE: &str = "ims-recovery-v1-session";
const ROW_SCHEMA: &str = "mainframe-env.ims-recovery-session@1";
const LOG_DOMAIN: &str = "mainframe-env.ims-recovery-log@1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoggedRecord {
    pub sequence: u64,
    pub code: u8,
    pub data: Vec<u8>,
    pub previous_digest: [u8; 32],
    pub digest: [u8; 32],
}

impl LoggedRecord {
    fn new(sequence: u64, request: LogRequest, previous_digest: [u8; 32]) -> Self {
        let digest = log_digest(sequence, request.code, &request.data, previous_digest);
        Self {
            sequence,
            code: request.code,
            data: request.data,
            previous_digest,
            digest,
        }
    }

    fn verify(&self, previous_digest: [u8; 32], limits: RecoveryLimits) -> bool {
        self.previous_digest == previous_digest
            && LogRequest {
                code: self.code,
                data: self.data.clone(),
            }
            .validate(limits)
            .is_ok()
            && self.digest == log_digest(self.sequence, self.code, &self.data, previous_digest)
    }
}

fn log_digest(sequence: u64, code: u8, data: &[u8], previous: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(LOG_DOMAIN.as_bytes());
    hasher.update(sequence.to_be_bytes());
    hasher.update(previous);
    hasher.update([code]);
    hasher.update((data.len() as u64).to_be_bytes());
    hasher.update(data);
    hasher.finalize().into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplayRecord {
    request_digest: [u8; 32],
    sequence: u64,
    returned_data: Vec<u8>,
    restart_result: Option<RestartResult>,
}

/// An existing shared provider-state row tracked by the caller's IMS UOW.
/// Rows outside the IMS DL/I and message scope are not eligible for backout.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackedResourceKind {
    Database,
    NonExpressMessage,
    ExpressMessage,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedResource {
    pub namespace: String,
    pub key: String,
    pub kind: TrackedResourceKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CapturedResource {
    resource: TrackedResource,
    version: Option<u64>,
    payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BackoutPoint {
    token: Option<[u8; 4]>,
    kind: BackoutPointKind,
    user_data: Vec<u8>,
    resources: Vec<CapturedResource>,
}

#[derive(Clone, Debug)]
pub struct RolsPlan {
    pub transition: RecoveryTransition,
    pub returned_data: Vec<u8>,
    pub reset_positions: bool,
}

#[derive(Clone, Debug)]
pub struct RollPlan {
    pub transition: RecoveryTransition,
    pub terminated: bool,
    pub reset_positions: bool,
}

/// Observation from the database adapter's qualified GU attempt.
#[derive(Clone, Debug)]
pub struct PositionAttempt {
    pub status: RepositionStatus,
    pub mutation: Option<ProviderStateMutation>,
}

#[derive(Clone, Debug)]
pub struct XrstPlan {
    pub transition: RecoveryTransition,
    pub result: RestartResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredState {
    schema_version: String,
    run: String,
    next_sequence: u64,
    checkpoints: BTreeMap<String, CheckpointImage>,
    logs: Vec<LoggedRecord>,
    baseline: Option<BackoutPoint>,
    points: Vec<BackoutPoint>,
    xrst_generation: u64,
    xrst_result: Option<RestartResult>,
    replays: BTreeMap<String, ReplayRecord>,
    row_digest: [u8; 32],
}

impl StoredState {
    fn new(run: &str) -> Self {
        let mut state = Self {
            schema_version: ROW_SCHEMA.into(),
            run: run.into(),
            next_sequence: 1,
            checkpoints: BTreeMap::new(),
            logs: Vec::new(),
            baseline: None,
            points: Vec::new(),
            xrst_generation: 0,
            xrst_result: None,
            replays: BTreeMap::new(),
            row_digest: [0; 32],
        };
        state.seal();
        state
    }

    fn digest(&self) -> [u8; 32] {
        let bytes = serde_json::to_vec(&(
            ROW_SCHEMA,
            &self.schema_version,
            &self.run,
            self.next_sequence,
            &self.checkpoints,
            &self.logs,
            &self.baseline,
            &self.points,
            self.xrst_generation,
            &self.xrst_result,
            &self.replays,
        ))
        .expect("bounded recovery state is JSON serializable");
        Sha256::digest(bytes).into()
    }

    fn seal(&mut self) {
        self.row_digest = self.digest();
    }

    fn verify(&self, run: &str, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.schema_version != ROW_SCHEMA
            || self.run != run
            || self.next_sequence == 0
            || self.checkpoints.len() > limits.max_checkpoints
            || self.logs.len() > limits.max_log_records
            || self.points.len() > limits.max_backout_points
            || (self.baseline.is_none() && !self.points.is_empty())
            || (self.xrst_generation == 0) != self.xrst_result.is_none()
            || self.replays.len() > limits.max_log_records
            || self
                .replays
                .values()
                .any(|replay| replay.returned_data.len() > limits.max_user_area_bytes)
            || self
                .xrst_result
                .as_ref()
                .is_some_and(|result| result.positions.len() > limits.max_positions)
            || self.replays.values().any(|replay| {
                replay
                    .restart_result
                    .as_ref()
                    .is_some_and(|result| result.positions.len() > limits.max_positions)
            })
            || self.row_digest != self.digest()
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        for (id, checkpoint) in &self.checkpoints {
            if checkpoint.request.id != *id || checkpoint.verify(limits).is_err() {
                return Err(RecoveryProblem::CorruptImage);
            }
        }
        let mut tokens = BTreeSet::new();
        if let Some(baseline) = &self.baseline {
            verify_point(baseline, limits)?;
        }
        for point in &self.points {
            verify_point(point, limits)?;
            if !point.token.is_some_and(|token| tokens.insert(token)) {
                return Err(RecoveryProblem::CorruptImage);
            }
        }
        let mut previous = [0; 32];
        let mut last = 0;
        for log in &self.logs {
            if log.sequence <= last || !log.verify(previous, limits) {
                return Err(RecoveryProblem::CorruptImage);
            }
            previous = log.digest;
            last = log.sequence;
        }
        if self
            .checkpoints
            .values()
            .any(|image| image.sequence >= self.next_sequence)
            || self
                .logs
                .iter()
                .any(|log| log.sequence >= self.next_sequence)
            || self
                .replays
                .values()
                .any(|replay| replay.sequence >= self.next_sequence)
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }
}

/// A version-fenced proposal. It has no private persistence path: consumers
/// add resource/UOW mutations and publish through ProviderStateStore atomically.
#[derive(Clone, Debug)]
pub struct RecoveryTransition {
    mutation: Option<ProviderStateMutation>,
    resource_mutations: Vec<ProviderStateMutation>,
    replayed: bool,
    warning: bool,
    sequence: u64,
}

impl RecoveryTransition {
    pub fn replayed(&self) -> bool {
        self.replayed
    }
    pub fn warning(&self) -> bool {
        self.warning
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn mutations(self) -> Vec<ProviderStateMutation> {
        self.mutation
            .into_iter()
            .chain(self.resource_mutations)
            .collect()
    }

    /// The canonical effect intent must already have been recorded by the
    /// caller. Infrastructure failure after dispatch is never called a miss.
    pub fn publish(
        self,
        store: &dyn ProviderStateStore,
        mut uow_mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), RecoveryProblem> {
        if self.replayed {
            return if uow_mutations.is_empty() {
                Ok(())
            } else {
                Err(RecoveryProblem::Conflict)
            };
        }
        uow_mutations.extend(self.mutations());
        store
            .mutate_provider_states_atomic(uow_mutations)
            .map_err(|error| match error {
                StoreError::Infrastructure(_) | StoreError::Poisoned => {
                    RecoveryProblem::UnknownOutcome
                }
                other => store_error(other),
            })
    }

    /// Bind a provider proposal to the canonical coordinator intent. This
    /// does not finalize that intent; the execution authority still records
    /// the terminal or unknown effect result after provider dispatch.
    pub fn publish_with_intent(
        self,
        store: &dyn ProviderStateStore,
        effects: &dyn IdempotencyStore,
        key: &IdempotencyKey,
        canonical_request_digest: [u8; 32],
        uow_mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), RecoveryProblem> {
        let effect = effects
            .effect(key)
            .map_err(store_error)?
            .ok_or(RecoveryProblem::InvalidRequest)?;
        if effect.request_digest != canonical_request_digest {
            return Err(RecoveryProblem::Conflict);
        }
        match effect.state {
            EffectState::Intent => self.publish(store, uow_mutations),
            EffectState::UnknownOutcome => Err(RecoveryProblem::UnknownOutcome),
            EffectState::Completed if self.replayed && uow_mutations.is_empty() => Ok(()),
            EffectState::Completed | EffectState::Failed => Err(RecoveryProblem::Conflict),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartResult {
    pub checkpoint_id: Option<String>,
    pub user_areas: Vec<Vec<u8>>,
    pub positions: Vec<RestartPcbStatus>,
}

/// Loaded recovery metadata for a single run. It does not own database rows,
/// message rows, the canonical effect journal, or a transaction coordinator.
#[derive(Clone, Debug)]
pub struct RecoverySession {
    version: Option<u64>,
    state: StoredState,
    limits: RecoveryLimits,
}

impl RecoverySession {
    pub fn load(
        store: &dyn ProviderStateStore,
        run: &str,
        limits: RecoveryLimits,
    ) -> Result<Self, RecoveryProblem> {
        if run.is_empty()
            || run.len() > 128
            || !run
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let row = store
            .get_provider_state(ROW_NAMESPACE, run)
            .map_err(store_error)?;
        let (version, state) = match row {
            Some(record) => {
                if record.namespace != ROW_NAMESPACE
                    || record.key != run
                    || record.version == 0
                    || record.payload.len() > limits.max_state_bytes
                {
                    return Err(RecoveryProblem::CorruptImage);
                }
                let state: StoredState = serde_json::from_slice(&record.payload)
                    .map_err(|_| RecoveryProblem::CorruptImage)?;
                (Some(record.version), state)
            }
            None => (None, StoredState::new(run)),
        };
        state.verify(run, limits)?;
        Ok(Self {
            version,
            state,
            limits,
        })
    }

    pub fn log_count(&self) -> usize {
        self.state.logs.len()
    }
    pub fn checkpoint_count(&self) -> usize {
        self.state.checkpoints.len()
    }
    pub fn backout_point_count(&self) -> usize {
        self.state.points.len()
    }

    /// Propose CHKP metadata after the caller has staged its database UOW
    /// commit. The caller must publish this row in that same atomic batch.
    pub fn checkpoint(
        &self,
        effect_id: &str,
        request: CheckpointRequest,
        committed_database_digest: [u8; 32],
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        request.validate(self.limits)?;
        if request.kind == CheckpointKind::Symbolic && self.state.xrst_generation == 0 {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let request_digest = request_digest("checkpoint", &(&request, committed_database_digest));
        if let Some(replay) = self.replay(effect_id, request_digest)? {
            return Ok(replay);
        }
        if self.state.checkpoints.len() >= self.limits.max_checkpoints
            && !self.state.checkpoints.contains_key(&request.id)
        {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let sequence = self.state.next_sequence;
        let image =
            CheckpointImage::seal(sequence, request, committed_database_digest, self.limits)?;
        let mut next = self.state.clone();
        next.checkpoints.insert(image.request.id.clone(), image);
        next.baseline = None;
        next.points.clear();
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        next.replays.insert(
            effect_id.into(),
            ReplayRecord {
                request_digest,
                sequence,
                returned_data: Vec::new(),
                restart_result: None,
            },
        );
        self.propose(next, sequence, Vec::new())
    }

    pub fn log(
        &self,
        effect_id: &str,
        request: LogRequest,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        request.validate(self.limits)?;
        let request_digest = request_digest("log", &request);
        if let Some(replay) = self.replay(effect_id, request_digest)? {
            return Ok(replay);
        }
        if self.state.logs.len() >= self.limits.max_log_records {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let sequence = self.state.next_sequence;
        let previous = self
            .state
            .logs
            .last()
            .map_or([0; 32], |record| record.digest);
        let mut next = self.state.clone();
        next.logs
            .push(LoggedRecord::new(sequence, request, previous));
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        next.replays.insert(
            effect_id.into(),
            ReplayRecord {
                request_digest,
                sequence,
                returned_data: Vec::new(),
                restart_result: None,
            },
        );
        self.propose(next, sequence, Vec::new())
    }

    /// Selects a verified symbolic image. Positions stay NotAttempted until
    /// the database adapter issues its actual GU and supplies PCB statuses.
    pub fn restart(
        &self,
        selection: RestartSelection,
        context: RecoveryContext,
    ) -> Result<RestartResult, RecoveryProblem> {
        selection.validate(self.limits)?;
        if context == RecoveryContext::MessageProcessing {
            return Err(RecoveryProblem::Unsupported);
        }
        let image = match selection {
            RestartSelection::Normal => None,
            RestartSelection::Last => {
                if context != RecoveryContext::MessageDrivenBatch {
                    return Err(RecoveryProblem::Unsupported);
                }
                Some(
                    self.state
                        .checkpoints
                        .values()
                        .filter(|image| image.request.kind == CheckpointKind::Symbolic)
                        .max_by_key(|image| image.sequence)
                        .ok_or(RecoveryProblem::NotFound)?,
                )
            }
            RestartSelection::Id(id) => Some(
                self.state
                    .checkpoints
                    .get(&id)
                    .ok_or(RecoveryProblem::NotFound)?,
            ),
            RestartSelection::Timestamp(_) => return Err(RecoveryProblem::Unsupported),
        };
        let Some(image) = image else {
            return Ok(RestartResult {
                checkpoint_id: None,
                user_areas: Vec::new(),
                positions: Vec::new(),
            });
        };
        image.verify(self.limits)?;
        if image.request.kind != CheckpointKind::Symbolic {
            return Err(RecoveryProblem::Unsupported);
        }
        Ok(RestartResult {
            checkpoint_id: Some(image.request.id.clone()),
            user_areas: image.request.user_areas.clone(),
            positions: image
                .request
                .positions
                .iter()
                .map(|position| RestartPcbStatus {
                    pcb: position.pcb.clone(),
                    status: RepositionStatus::NotAttempted,
                })
                .collect(),
        })
    }

    /// XRST is one call per execution generation. The resolver must only
    /// *propose* each qualified-GU PCB update; all updates and this receipt
    /// publish in the same shared-store batch. Missing/corrupt images do not
    /// invoke the resolver or produce a mutation.
    pub fn xrst<F>(
        &self,
        effect_id: &str,
        generation: u64,
        selection: RestartSelection,
        context: RecoveryContext,
        mut resolve: F,
    ) -> Result<XrstPlan, RecoveryProblem>
    where
        F: FnMut(&SavedPcbPosition) -> Result<PositionAttempt, RecoveryProblem>,
    {
        if generation == 0 {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let digest = request_digest("xrst", &(generation, &selection, context));
        if let Some(replay) = self.replay(effect_id, digest)? {
            let result = self.state.replays[effect_id]
                .restart_result
                .clone()
                .ok_or(RecoveryProblem::CorruptImage)?;
            return Ok(XrstPlan {
                transition: replay,
                result,
            });
        }
        if generation <= self.state.xrst_generation {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let mut result = self.restart(selection, context)?;
        let mut mutations = Vec::new();
        let mut row_keys = BTreeSet::new();
        if let Some(id) = &result.checkpoint_id {
            let image = self
                .state
                .checkpoints
                .get(id)
                .ok_or(RecoveryProblem::CorruptImage)?;
            for (saved, status) in image.request.positions.iter().zip(&mut result.positions) {
                let attempt = resolve(saved)?;
                if attempt.status != RepositionStatus::NotAttempted && attempt.mutation.is_none() {
                    return Err(RecoveryProblem::InvalidRequest);
                }
                if let Some(mutation) = attempt.mutation {
                    let identity = position_mutation_identity(&mutation)
                        .ok_or(RecoveryProblem::InvalidRequest)?;
                    if !row_keys.insert(identity) {
                        return Err(RecoveryProblem::InvalidRequest);
                    }
                    mutations.push(mutation);
                }
                status.status = attempt.status;
            }
        }
        let sequence = self.state.next_sequence;
        let mut next = self.state.clone();
        next.xrst_generation = generation;
        next.xrst_result = Some(result.clone());
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        next.replays.insert(
            effect_id.into(),
            ReplayRecord {
                request_digest: digest,
                sequence,
                returned_data: Vec::new(),
                restart_result: Some(result.clone()),
            },
        );
        Ok(XrstPlan {
            transition: self.propose(next, sequence, mutations)?,
            result,
        })
    }

    /// Capture the caller's explicit shared-store resource set before the
    /// first staged IMS mutation. New resources must be declared before work.
    pub fn begin_uow(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
        resources: Vec<TrackedResource>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        let digest = request_digest("begin-uow", &resources);
        if let Some(replay) = self.replay(effect_id, digest)? {
            return Ok(replay);
        }
        if self.state.baseline.is_some() {
            return Err(RecoveryProblem::Conflict);
        }
        let captured = capture(store, &resources, self.limits)?;
        let mut next = self.state.clone();
        next.baseline = Some(BackoutPoint {
            token: None,
            kind: BackoutPointKind::Sets,
            user_data: Vec::new(),
            resources: captured,
        });
        self.record_op(next, effect_id, digest, Vec::new(), Vec::new())
    }

    /// SETS/SETU. A tokenless call cancels points. Unsupported SETS rejects;
    /// unsupported SETU reports SC without claiming a functional savepoint.
    pub fn sets(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
        kind: BackoutPointKind,
        token: Option<[u8; 4]>,
        user_data: Vec<u8>,
        unsupported_pcb_or_external: bool,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        let digest = request_digest(
            "sets",
            &(kind, token, &user_data, unsupported_pcb_or_external),
        );
        if let Some(replay) = self.replay(effect_id, digest)? {
            return Ok(replay);
        }
        if self.state.baseline.is_none() {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if user_data.len() > self.limits.max_user_area_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        if token.is_none() && !user_data.is_empty() {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if unsupported_pcb_or_external {
            if kind == BackoutPointKind::Sets {
                return Err(RecoveryProblem::Unsupported);
            }
            return Ok(RecoveryTransition {
                mutation: None,
                resource_mutations: Vec::new(),
                replayed: false,
                warning: true,
                sequence: 0,
            });
        }
        let mut next = self.state.clone();
        match token {
            None => next.points.clear(),
            Some(token) => {
                let resources = next
                    .baseline
                    .as_ref()
                    .expect("checked baseline")
                    .resources
                    .iter()
                    .map(|captured| captured.resource.clone())
                    .collect::<Vec<_>>();
                let point = BackoutPoint {
                    token: Some(token),
                    kind,
                    user_data,
                    resources: capture(store, &resources, self.limits)?,
                };
                if let Some(index) = next
                    .points
                    .iter()
                    .position(|point| point.token == Some(token))
                {
                    next.points.truncate(index + 1);
                    next.points[index] = point;
                } else {
                    if next.points.len() >= self.limits.max_backout_points {
                        return Err(RecoveryProblem::LimitExceeded);
                    }
                    next.points.push(point);
                }
            }
        }
        self.record_op(next, effect_id, digest, Vec::new(), Vec::new())
    }

    /// Build an atomic rollback plan. The shared store applies recovery state
    /// and database/non-express message restoration together, or neither.
    pub fn rols(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
        token: [u8; 4],
    ) -> Result<RolsPlan, RecoveryProblem> {
        let digest = request_digest("rols", &token);
        if let Some(replay) = self.replay(effect_id, digest)? {
            return Ok(RolsPlan {
                transition: replay,
                returned_data: self.state.replays[effect_id].returned_data.clone(),
                reset_positions: true,
            });
        }
        let index = self
            .state
            .points
            .iter()
            .position(|point| point.token == Some(token))
            .ok_or(RecoveryProblem::NotFound)?;
        let point = &self.state.points[index];
        let resource_mutations = restore(store, &point.resources)?;
        let returned_data = point.user_data.clone();
        let mut next = self.state.clone();
        next.points.truncate(index + 1);
        let transition = self.record_op(
            next,
            effect_id,
            digest,
            resource_mutations,
            returned_data.clone(),
        )?;
        Ok(RolsPlan {
            transition,
            returned_data,
            reset_positions: true,
        })
    }

    pub fn rolb(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
    ) -> Result<RollPlan, RecoveryProblem> {
        self.rollback_to_commit(store, effect_id, false)
    }

    pub fn roll(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
    ) -> Result<RollPlan, RecoveryProblem> {
        self.rollback_to_commit(store, effect_id, true)
    }

    fn rollback_to_commit(
        &self,
        store: &dyn ProviderStateStore,
        effect_id: &str,
        terminated: bool,
    ) -> Result<RollPlan, RecoveryProblem> {
        let digest = request_digest("rollback", &terminated);
        if let Some(replay) = self.replay(effect_id, digest)? {
            return Ok(RollPlan {
                transition: replay,
                terminated,
                reset_positions: true,
            });
        }
        let baseline = self
            .state
            .baseline
            .as_ref()
            .ok_or(RecoveryProblem::InvalidRequest)?;
        let resource_mutations = restore(store, &baseline.resources)?;
        let mut next = self.state.clone();
        next.baseline = None;
        next.points.clear();
        let transition = self.record_op(next, effect_id, digest, resource_mutations, Vec::new())?;
        Ok(RollPlan {
            transition,
            terminated,
            reset_positions: true,
        })
    }

    fn record_op(
        &self,
        mut next: StoredState,
        effect_id: &str,
        request_digest: [u8; 32],
        resource_mutations: Vec<ProviderStateMutation>,
        returned_data: Vec<u8>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        let sequence = self.state.next_sequence;
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        next.replays.insert(
            effect_id.into(),
            ReplayRecord {
                request_digest,
                sequence,
                returned_data,
                restart_result: None,
            },
        );
        self.propose(next, sequence, resource_mutations)
    }

    fn replay(
        &self,
        effect_id: &str,
        digest: [u8; 32],
    ) -> Result<Option<RecoveryTransition>, RecoveryProblem> {
        if effect_id.is_empty()
            || effect_id.len() > 128
            || !effect_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        match self.state.replays.get(effect_id) {
            Some(record) if record.request_digest == digest => Ok(Some(RecoveryTransition {
                mutation: None,
                resource_mutations: Vec::new(),
                replayed: true,
                warning: false,
                sequence: record.sequence,
            })),
            Some(_) => Err(RecoveryProblem::Conflict),
            None if self.state.replays.len() >= self.limits.max_log_records => {
                Err(RecoveryProblem::LimitExceeded)
            }
            None => Ok(None),
        }
    }

    fn propose(
        &self,
        mut state: StoredState,
        sequence: u64,
        resource_mutations: Vec<ProviderStateMutation>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        state.seal();
        let payload =
            serde_json::to_vec(&state).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
        if payload.len() > self.limits.max_state_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let version = self
            .version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(RecoveryProblem::LimitExceeded)?;
        Ok(RecoveryTransition {
            mutation: Some(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: ROW_NAMESPACE.into(),
                    key: state.run.clone(),
                    version,
                    payload,
                },
                expected_version: self.version,
            })),
            resource_mutations,
            replayed: false,
            warning: false,
            sequence,
        })
    }
}

fn request_digest<T: Serialize>(name: &str, value: &T) -> [u8; 32] {
    let material = serde_json::to_vec(&(ROW_SCHEMA, name, value))
        .expect("bounded recovery request is JSON serializable");
    Sha256::digest(material).into()
}

fn store_error(error: StoreError) -> RecoveryProblem {
    match error {
        StoreError::Conflict | StoreError::AlreadyExists => RecoveryProblem::Conflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            RecoveryProblem::LimitExceeded
        }
        _ => RecoveryProblem::InfrastructureFailure,
    }
}

fn valid_resource(resource: &TrackedResource) -> bool {
    !resource.namespace.is_empty()
        && resource.namespace.starts_with("ims-")
        && resource.namespace != ROW_NAMESPACE
        && resource.namespace.len() <= 256
        && !resource.key.is_empty()
        && resource.key.len() <= 1_024
        && !resource.namespace.chars().any(char::is_control)
        && !resource.key.chars().any(char::is_control)
}

fn position_mutation_identity(mutation: &ProviderStateMutation) -> Option<(String, String)> {
    let (namespace, key) = match mutation {
        ProviderStateMutation::Put(write) => (&write.record.namespace, &write.record.key),
        ProviderStateMutation::Delete { namespace, key, .. } => (namespace, key),
        ProviderStateMutation::Move { .. } => return None,
    };
    if !namespace.starts_with("ims-")
        || namespace == ROW_NAMESPACE
        || key.is_empty()
        || namespace.len() > 256
        || key.len() > 1_024
    {
        return None;
    }
    Some((namespace.clone(), key.clone()))
}

fn verify_point(point: &BackoutPoint, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
    if point.resources.len() > limits.max_positions
        || point.user_data.len() > limits.max_user_area_bytes
    {
        return Err(RecoveryProblem::CorruptImage);
    }
    let mut seen = BTreeSet::new();
    for captured in &point.resources {
        if !valid_resource(&captured.resource)
            || !seen.insert((&captured.resource.namespace, &captured.resource.key))
            || captured.version.is_some() != captured.payload.is_some()
            || captured.version == Some(0)
            || captured
                .payload
                .as_ref()
                .is_some_and(|payload| payload.len() > limits.max_state_bytes)
        {
            return Err(RecoveryProblem::CorruptImage);
        }
    }
    Ok(())
}

fn capture(
    store: &dyn ProviderStateStore,
    resources: &[TrackedResource],
    limits: RecoveryLimits,
) -> Result<Vec<CapturedResource>, RecoveryProblem> {
    if resources.len() > limits.max_positions {
        return Err(RecoveryProblem::LimitExceeded);
    }
    let mut seen = BTreeSet::new();
    let mut captured = Vec::with_capacity(resources.len());
    for resource in resources {
        if !valid_resource(resource) || !seen.insert((&resource.namespace, &resource.key)) {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let row = store
            .get_provider_state(&resource.namespace, &resource.key)
            .map_err(store_error)?;
        if row
            .as_ref()
            .is_some_and(|row| row.version == 0 || row.payload.len() > limits.max_state_bytes)
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        captured.push(CapturedResource {
            resource: resource.clone(),
            version: row.as_ref().map(|row| row.version),
            payload: row.map(|row| row.payload),
        });
    }
    Ok(captured)
}

fn restore(
    store: &dyn ProviderStateStore,
    captured: &[CapturedResource],
) -> Result<Vec<ProviderStateMutation>, RecoveryProblem> {
    let mut mutations = Vec::new();
    for item in captured {
        if item.resource.kind == TrackedResourceKind::ExpressMessage {
            continue;
        }
        let current = store
            .get_provider_state(&item.resource.namespace, &item.resource.key)
            .map_err(store_error)?;
        match (item.payload.as_ref(), current) {
            (Some(payload), Some(row)) if row.payload != *payload => {
                let version = row
                    .version
                    .checked_add(1)
                    .ok_or(RecoveryProblem::LimitExceeded)?;
                mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: item.resource.namespace.clone(),
                        key: item.resource.key.clone(),
                        version,
                        payload: payload.clone(),
                    },
                    expected_version: Some(row.version),
                }));
            }
            (Some(_), Some(_)) | (None, None) => {}
            (Some(_), None) => return Err(RecoveryProblem::Conflict),
            (None, Some(row)) => mutations.push(ProviderStateMutation::Delete {
                namespace: item.resource.namespace.clone(),
                key: item.resource.key.clone(),
                expected_version: row.version,
            }),
        }
    }
    Ok(mutations)
}
