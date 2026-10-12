//! Explicit same-store observation of one known committed local PUT1.
//! No trigger eligibility, native MQ or syncpoint/reconciliation semantics are inferred.
use super::*;
use mainframe_env_store_api::{EffectRecord, ExecutionState, PlatformStore};

/// Inputs are observations, never authority minted from copied IDs or rows.
pub struct MqCommittedPutOffer<'a> {
    pub source: &'a Invocation,
    pub request: &'a MqRequest,
    pub catalog: &'a MqCatalogProjection,
}

/// Holds the genuine legacy owner until the exact source-fencing CAS is acknowledged.
/// No constructor, Clone or serde implementation is provided.
pub struct PreparedMqCommittedPut<'a> {
    service: &'a MqService,
    durable: LegacyAccess<'a>,
    writes: Vec<ProviderStateWrite>,
    source_row: ProviderStateRecord,
    effect: EffectRecord,
    queue: String,
    process: String,
}

impl MqService {
    pub fn prepare_committed_local_put<'a>(
        &'a self,
        store: &Arc<dyn PlatformStore>,
        offer: MqCommittedPutOffer<'_>,
    ) -> Result<PreparedMqCommittedPut<'a>, HostProblem> {
        if Arc::as_ptr(store).cast::<()>() != Arc::as_ptr(&self.store).cast::<()>() {
            return Err(HostProblem::Unsupported);
        }
        let request = offer.request;
        if request.operation != MqOperation::PutOne
            || request.options != 0
            || request.handle.is_some()
            || request.wait_ticks != 0
            || request
                .queue
                .as_deref()
                .is_none_or(|name| !is_canonical_name(name))
        {
            return Err(HostProblem::Unsupported);
        }
        let mutation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?;
        if mutation.transaction.is_some() || mutation.sequence == 0 {
            return Err(HostProblem::Unsupported);
        }
        let execution = store
            .get_execution(&offer.source.execution_id)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        if execution.state != ExecutionState::Completed
            || execution.version == 0
            || execution.terminal_tick.is_none_or(|tick| tick == 0)
            || execution.execution_id != offer.source.execution_id
            || execution.run_unit_id != offer.source.run_unit_id
            || execution.selector != offer.source.selector
            || execution.artifact != offer.source.artifact
            || execution.principal != *offer.source.principal.id()
            || execution.attempt != offer.source.attempt
        {
            return Err(HostProblem::Unauthorized);
        }
        let effect = store
            .effect(&mutation.idempotency_key)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let durable = self.lock()?;
        verify_actual_cache(self, &durable)?;
        verify_projection(&durable.state, offer.catalog, self.limits)?;
        let selection = read_selection(self)?.ok_or(HostProblem::NotFound)?;
        if decode_selection(&selection)? != *offer.catalog || !durable.state.pending.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let queue = request.queue.as_deref().ok_or(HostProblem::Malformed)?;
        let actual = durable
            .state
            .queues
            .get(queue)
            .ok_or(HostProblem::NotFound)?;
        let process = actual
            .trigger_program
            .clone()
            .ok_or(HostProblem::Unsupported)?;
        let row = self
            .store
            .get_provider_state(REPLAY_NAMESPACE, mutation.idempotency_key.as_str())
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let descriptor = crate::retention::describe_mq_replay_row(&row, Some(&effect), self.limits)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        if descriptor.retention != crate::retention::MqReplayRetentionState::Terminal
            || descriptor.dependency != Some(crate::retention::MqReplayDependency::CoreEffect)
            || descriptor.request_digest != request_digest(request)?
            || descriptor.owner_execution.as_deref() != Some(offer.source.execution_id.as_str())
            || descriptor.owner_run_unit.as_deref() != Some(offer.source.run_unit_id.as_str())
            || descriptor.sequence != Some(mutation.sequence)
            || effect.intent.attempt != offer.source.attempt
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let recorded = durable
            .state
            .replay
            .get(mutation.idempotency_key.as_str())
            .ok_or(HostProblem::NotFound)?;
        if recorded.completion_code != 0
            || recorded.reason_code != 0
            || recorded.trigger_program.as_deref() != Some(process.as_str())
            || recorded.message_id.as_ref().is_none_or(|id| id.len() != 24)
            || recorded
                .correlation_id
                .as_ref()
                .is_none_or(|id| id.len() != 24)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let message_id = recorded.message_id.as_ref().ok_or(HostProblem::Malformed)?;
        let correlation = recorded
            .correlation_id
            .as_ref()
            .ok_or(HostProblem::Malformed)?;
        if actual
            .messages
            .iter()
            .filter(|message| {
                message.message_id == *message_id
                    && message.correlation_id == *correlation
                    && message.data == request.message
            })
            .count()
            != 1
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut writes = Vec::new();
        let mut bytes = 0usize;
        // Exact original rows, not reconstructed semantic subsets. The held owner
        // plus real version CAS protects against another live service adapter.
        for (namespace, key) in [
            (STATE_NAMESPACE, STATE_KEY),
            (QUEUE_NAMESPACE, queue),
            (REPLAY_NAMESPACE, mutation.idempotency_key.as_str()),
        ] {
            let current = self
                .store
                .get_provider_state(namespace, key)
                .map_err(store_error)?
                .ok_or(HostProblem::IdempotencyConflict)?;
            if durable.versions.get(&(namespace.into(), key.into())) != Some(&current.version) {
                return Err(HostProblem::IdempotencyConflict);
            }
            bytes = bytes
                .checked_add(current.namespace.len())
                .and_then(|n| n.checked_add(current.key.len()))
                .and_then(|n| n.checked_add(current.payload.len()))
                .ok_or(HostProblem::ResourceExhausted)?;
            if bytes
                .checked_mul(3)
                .is_none_or(|n| n > MAX_PROVIDER_COMPOSITION_BYTES / 4)
            {
                return Err(HostProblem::ResourceExhausted);
            }
            let version = current
                .version
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                expected_version: Some(current.version),
                record: ProviderStateRecord { version, ..current },
            });
        }
        Ok(PreparedMqCommittedPut {
            service: self,
            durable,
            writes,
            source_row: row,
            effect,
            queue: queue.into(),
            process,
        })
    }
}

impl PreparedMqCommittedPut<'_> {
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    pub fn source_row(&self) -> &ProviderStateRecord {
        &self.source_row
    }
    pub fn source_effect(&self) -> &EffectRecord {
        &self.effect
    }
    pub fn queue(&self) -> &str {
        &self.queue
    }
    pub fn process(&self) -> &str {
        &self.process
    }
    pub fn fence_unknown(&self) {
        self.service.revoke_selected_authority();
    }
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    pub fn finish(mut self, commit: &ProviderCompositionCommit) -> Result<(), HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        for write in &self.writes {
            self.durable.versions.insert(
                (write.record.namespace.clone(), write.record.key.clone()),
                write.record.version,
            );
        }
        Ok(())
    }
}
