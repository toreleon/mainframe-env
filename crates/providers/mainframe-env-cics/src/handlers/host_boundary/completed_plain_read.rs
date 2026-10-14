//! Finite recovery of a completed non-update READ, never a live browse reply.
use super::*;
use mainframe_env_store_api::{
    EffectDigestFormat, EffectRecord, EffectState, ExecutionRecord, ExecutionState, PlatformStore,
    ProviderReplayAssertion, ProviderStateIdentity, TerminalRowDependency,
};
use std::sync::Weak;

/// Privately minted same-service/store observations, not a native-root permit.
/// No public constructor, Clone, Debug or serialization is provided.
pub struct PreparedCicsPlainRead {
    service: Weak<CicsService>,
    store: Arc<dyn PlatformStore>,
    original: Invocation,
    effect: EffectRequest,
    completed: EffectRecord,
    execution: ExecutionRecord,
    observed_tick: u64,
}

impl PreparedCicsPlainRead {
    fn require_core(&self) -> Result<(), HostProblem> {
        self.require_core_at(self.observed_tick)
    }

    fn require_core_at(&self, observed_tick: u64) -> Result<(), HostProblem> {
        let original = &self.original;
        let completed = &self.completed;
        if !matches!(&self.effect.request, HostRequest::Cics(request)
            if request.is_completed_plain_read_replay())
            || original.cancellation_requested()
            || observed_tick < self.observed_tick
            || observed_tick == 0
            || observed_tick >= original.deadline_tick.min(self.effect.deadline_tick)
            || completed.state != EffectState::Completed
            || completed.result_digest.is_none()
            || completed
                .resolved_tick
                .is_none_or(|tick| tick == 0 || tick > observed_tick)
            || completed.intent.recovery_lease.is_some()
            || completed.execution_id != original.execution_id
            || completed.run_unit_id != original.run_unit_id
            || completed.sequence != self.effect.sequence
            || self.effect.run_unit != original.run_unit_id
            || self.effect.idempotency_key.as_ref() != Some(&completed.key)
            || completed.digest_format != EffectDigestFormat::CanonicalHostV1
            || completed.request_digest != canonical_request_digest(&self.effect.request)?
            || completed.intent.owner != original.execution_id
            || completed.intent.attempt != original.attempt
            || completed.intent.audit_invocation_key.as_ref() != Some(&original.idempotency_key)
            || completed.intent.capability.as_ref()
                != Some(
                    &self
                        .effect
                        .request
                        .required_capability(InvocationLimits::default()),
                )
            || completed.intent.audit_resource
                != Some(mainframe_env_host_api::canonical_audit_resource_digest(
                    &self.effect.request,
                ))
            || self.execution.state != ExecutionState::Running
            || self.execution.terminal_tick.is_some()
            || self.execution.execution_id != original.execution_id
            || self.execution.run_unit_id != original.run_unit_id
            || self.execution.selector != original.selector
            || self.execution.artifact != original.artifact
            || self.execution.principal != *original.principal.id()
            || self.execution.attempt != original.attempt
            || self
                .store
                .get_execution(&original.execution_id)
                .map_err(|_| HostProblem::UnknownOutcome)?
                .as_ref()
                != Some(&self.execution)
            || self
                .store
                .effect(&completed.key)
                .map_err(|_| HostProblem::UnknownOutcome)?
                .as_ref()
                != Some(completed)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

impl CicsService {
    /// Prepare only from actual Completed/core observations retained by a trusted drive.
    /// The receiver still reacquires the current task lease and atomic store fence.
    pub fn prepare_completed_plain_read(
        self: &Arc<Self>,
        store: Arc<dyn PlatformStore>,
        original: Invocation,
        effect: EffectRequest,
        completed: EffectRecord,
        execution: ExecutionRecord,
        observed_tick: u64,
    ) -> Result<PreparedCicsPlainRead, HostProblem> {
        if Arc::as_ptr(&self.store) as *const () != Arc::as_ptr(&store) as *const () {
            return Err(HostProblem::UnknownOutcome);
        }
        reject_reserved_nested_origin(&original)?;
        let prepared = PreparedCicsPlainRead {
            service: Arc::downgrade(self),
            store,
            original,
            effect,
            completed,
            execution,
            observed_tick,
        };
        prepared.require_core()?;
        let mut state = self.lock()?;
        // Match ordinary ensure_run's existing-frame synchronization, but never mint a Run.
        if !state.runs.contains_key(&prepared.original.run_unit_id)
            || !super::super::task_context::synchronize_current_program(
                &mut state,
                &prepared.original,
            )?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(prepared)
    }

    fn consume_completed_plain_read(
        &self,
        original: &Invocation,
        effect: &EffectRequest,
        digest: [u8; 32],
        observed_tick: u64,
        prepared: &PreparedCicsPlainRead,
    ) -> Result<CicsResponse, HostProblem> {
        let owner = prepared
            .service
            .upgrade()
            .ok_or(HostProblem::UnknownOutcome)?;
        if !std::ptr::eq(self, owner.as_ref())
            || Arc::as_ptr(&self.store) as *const () != Arc::as_ptr(&prepared.store) as *const ()
            || &prepared.original != original
            || &prepared.effect != effect
            || prepared.observed_tick != observed_tick
            || prepared.completed.result_digest != Some(digest)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        prepared.require_core()?;
        let HostRequest::Cics(request) = &effect.request else {
            return Err(HostProblem::Unsupported);
        };
        let mut run =
            CommandLease::acquire_with_origin(self, &effect.run_unit, Some(CicsOperation::Read))?;
        if run.current_program.effect_invocation != *original
            || run.invocation.run_unit_id != original.run_unit_id
            || !run.browses.is_empty()
            || !run.initial_browse_positions.is_empty()
            || !run.current_records.is_empty()
            || !run.undo.is_empty()
            || run.undo_version.is_some()
            || !run.file_updates.file_tokens.is_empty()
            || !run.file_updates.task_browses.is_empty()
            || !run.file_updates.current_record_values.is_empty()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let row = self
            .store
            .get_provider_state("cics-effect-replay-v1", prepared.completed.key.as_str())
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if row.version != 2 || row.payload.get(..8) != Some(b"MECER004") {
            return Err(HostProblem::UnknownOutcome);
        }
        let replay = decode_cics_effect_replay(&row.payload, self.limits)?;
        validate_cics_effect_replay_identity(
            &replay,
            prepared.completed.key.as_str(),
            original.execution_id.as_str(),
            original.run_unit_id.as_str(),
            effect.sequence,
            prepared.completed.request_digest,
        )?;
        // The completed occurrence retains its original bound. The new
        // effect/Invocation deadlines remain the live request budgets checked
        // above and by ScopedHostService; no expired dispatch is authorized.
        if replay.deadline_tick != Some(prepared.completed.intent.recovery_after_tick) {
            if std::env::var("MAINFRAME_ENV_READ_REPLAY_DIAGNOSTICS")
                .is_ok_and(|value| value == "1")
            {
                eprintln!(
                    "CICS completed READ replay refusal stage=receipt-deadline current_deadline={} receipt_deadline={} occurrence_deadline={}",
                    effect.deadline_tick,
                    replay.deadline_tick.unwrap_or(0),
                    prepared.completed.intent.recovery_after_tick
                );
            }
            return Err(HostProblem::UnknownOutcome);
        }
        if replay.resolution_tick.is_none()
            || replay.result_digest != Some(digest)
            || replay.response.disposition != CicsDisposition::Complete
            || replay.response.condition != "NORMAL"
            || replay.response.response != 0
            || replay.response.response2 != 0
            || replay.response.target.is_some()
            || replay.response.next_transaction.is_some()
            || replay.response.unit_of_work.is_some()
            || replay.response.applid != run.applid
            || replay.response.sysid != run.sysid
            || replay.response.transaction != run.transaction
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let alias = replay
            .read_alias
            .as_ref()
            .ok_or(HostProblem::UnknownOutcome)?;
        let logical = argument_text(request, "DATASET")?
            .trim()
            .to_ascii_uppercase();
        if alias.namespace != "cics-file-alias" || alias.key != logical {
            return Err(HostProblem::UnknownOutcome);
        }
        let definition = decode_file_definition(&alias.payload)?;
        let session_row = self
            .store
            .get_provider_state("cics-session", &run.session)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let status_row = self
            .store
            .get_provider_state("cics-file-status", &logical)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        {
            let state = self.lock()?;
            let session = state
                .sessions
                .get(&run.session)
                .ok_or(HostProblem::UnknownOutcome)?;
            if !session.connected
                || session.expires_at_tick <= observed_tick
                || session_row.version != session.version
                || session_row.payload != handlers::encode_session(session)?
                || replay.response.aid != 0
                || state.file_aliases.get(&logical) != Some(&definition)
                || !matches!(
                    state.file_statuses.get(&logical),
                    None | Some(DurableFileStatus {
                        status: CicsFileStatus::Open,
                        ..
                    })
                )
                || status_row.as_ref().is_some_and(|current| {
                    state.file_statuses.get(&logical).is_none_or(|status| {
                        current.version != status.version
                            || current.payload != encode_file_status(status.status)
                    })
                })
                || status_row.is_none() && state.file_statuses.contains_key(&logical)
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        // Actual current command and dataset SAF checks; never invoke a dataset read.
        handlers::authorize_and_describe(self, &mut run, request)?;
        self.authorize(
            &mut run,
            "DATASET",
            definition.dataset.as_str(),
            AccessIntent::Read,
        )?;
        // SAF can advance the shared store clock after the coordinator's observation.
        // Sample the configured clock once, then retain all current expiry and row fences.
        let replay_tick = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::UnknownOutcome)?
            .now_tick()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        prepared.require_core_at(replay_tick)?;
        if self
            .lock()?
            .sessions
            .get(&run.session)
            .is_none_or(|session| !session.connected || session.expires_at_tick <= replay_tick)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let response = handlers::validate_replay_response(request, replay.response)?;
        prepared
            .store
            .assert_provider_replay(ProviderReplayAssertion {
                effect: prepared.completed.clone(),
                execution: prepared.execution.clone(),
                observed_tick: replay_tick,
                receipt: ProviderStateIdentity {
                    namespace: row.namespace.clone(),
                    key: row.key.clone(),
                },
                dependencies: vec![
                    TerminalRowDependency::Exact(row),
                    TerminalRowDependency::Exact(alias.clone()),
                    TerminalRowDependency::Exact(session_row),
                    match status_row {
                        Some(status) => TerminalRowDependency::Exact(status),
                        None => TerminalRowDependency::Absent {
                            namespace: "cics-file-status".into(),
                            key: logical,
                        },
                    },
                    TerminalRowDependency::Absent {
                        namespace: "cics-uow".into(),
                        key: original.run_unit_id.to_string(),
                    },
                    TerminalRowDependency::Absent {
                        namespace: "cics-uow-undo".into(),
                        key: original.run_unit_id.to_string(),
                    },
                ],
            })
            .map_err(|_| HostProblem::UnknownOutcome)?;
        run.finish().map_err(|_| HostProblem::UnknownOutcome)?;
        Ok(response)
    }
}

/// Capture the actual configured file alias for the pinned DATASET spelling.
/// A direct physical Dataset name without this configured CICS alias stays unproved.
/// Missing aliases retain ordinary first-dispatch behavior and cannot become replay proof.
pub(super) fn read_alias(
    service: &CicsService,
    request: &CicsRequest,
) -> Result<Option<ProviderStateRecord>, HostProblem> {
    if !request.is_completed_plain_read_replay() {
        return Ok(None);
    }
    let logical = argument_text(request, "DATASET")?
        .trim()
        .to_ascii_uppercase();
    let Some(row) = service
        .store
        .get_provider_state("cics-file-alias", &logical)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    let definition = decode_file_definition(&row.payload)?;
    if service.lock()?.file_aliases.get(&logical) != Some(&definition) {
        return Err(HostProblem::UnknownOutcome);
    }
    row.validate_write(InvocationLimits::default().max_payload_bytes)
        .map_err(store_error)?;
    Ok(Some(row))
}

pub(super) fn validate_alias(row: &ProviderStateRecord) -> Result<(), HostProblem> {
    if row.namespace != "cics-file-alias" || normalize_terminal_name(&row.key, 16)? != row.key {
        return Err(HostProblem::InfrastructureFailure);
    }
    row.validate_write(InvocationLimits::default().max_payload_bytes)
        .map_err(store_error)?;
    decode_file_definition(&row.payload)?;
    Ok(())
}

pub(super) fn encode_alias(
    out: &mut Vec<u8>,
    row: &ProviderStateRecord,
) -> Result<(), HostProblem> {
    validate_alias(row)?;
    field(out, row.key.as_bytes())?;
    out.extend_from_slice(&row.version.to_be_bytes());
    field(out, &row.payload)
}

pub(super) fn decode_alias(reader: &mut Reader<'_>) -> Result<ProviderStateRecord, HostProblem> {
    let row = ProviderStateRecord {
        namespace: "cics-file-alias".into(),
        key: String::from_utf8(reader.field(16)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        version: u64::from_be_bytes(
            reader
                .take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ),
        payload: reader.field(512)?,
    };
    validate_alias(&row)?;
    Ok(row)
}

// Require unchanged full CAS identity after the original physical READ.
// A rename/replacement followed by restoration still changes the positive version.
pub(super) fn confirm_alias(
    service: &CicsService,
    alias: Option<&ProviderStateRecord>,
) -> Result<(), HostProblem> {
    if let Some(alias) = alias {
        if service
            .store
            .get_provider_state(&alias.namespace, &alias.key)
            .map_err(store_error)?
            .as_ref()
            != Some(alias)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let definition = decode_file_definition(&alias.payload)?;
        if service.lock()?.file_aliases.get(&alias.key) != Some(&definition) {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    Ok(())
}

impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        if let Err(problem) = reject_reserved_nested_origin(invocation) {
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem),
            };
        }
        if let Err(problem) = self.service.ensure_run(invocation) {
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem),
            };
        }
        let sequence = effect.sequence;
        let request = match effect.request.clone() {
            HostRequest::Cics(request) => request,
            _ => {
                return EffectResult {
                    sequence,
                    outcome: Err(HostProblem::Malformed),
                };
            }
        };
        EffectResult {
            sequence,
            outcome: self.service.invoke(&effect, request).map(HostResult::Cics),
        }
    }

    fn replay_completed_plain_read(
        &self,
        original: &Invocation,
        effect: EffectRequest,
        digest: [u8; 32],
        observed_tick: u64,
        context: &(dyn std::any::Any + Send + Sync),
    ) -> EffectResult {
        let sequence = effect.sequence;
        let outcome = context
            .downcast_ref::<PreparedCicsPlainRead>()
            .ok_or(HostProblem::UnknownOutcome)
            .and_then(|prepared| {
                self.service.consume_completed_plain_read(
                    original,
                    &effect,
                    digest,
                    observed_tick,
                    prepared,
                )
            })
            .map(HostResult::Cics);
        EffectResult { sequence, outcome }
    }
}

pub(crate) fn encode_cics_effect_replay(replay: &CicsEffectReplay) -> Result<Vec<u8>, HostProblem> {
    if replay.read_alias.is_some() && replay.effect_key.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let response = &replay.response;
    let owner = replay
        .owner_execution
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let deadline_tick = replay
        .deadline_tick
        .filter(|tick| *tick != 0)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut out = if let Some(effect_key) = replay.effect_key.as_deref() {
        let owner_run_unit = replay
            .owner_run_unit
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let sequence = replay
            .sequence
            .filter(|sequence| *sequence != 0)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let result_digest = replay
            .result_digest
            .ok_or(HostProblem::InfrastructureFailure)?;
        let binding_digest = replay
            .binding_digest
            .ok_or(HostProblem::InfrastructureFailure)?;
        if replay.resolution_tick == Some(0)
            || replay
                .resolution_tick
                .is_some_and(|tick| tick < deadline_tick)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut out = if replay.read_alias.is_some() {
            b"MECER004".to_vec()
        } else {
            b"MECER003".to_vec()
        };
        field(&mut out, effect_key.as_bytes())?;
        field(&mut out, owner.as_bytes())?;
        field(&mut out, owner_run_unit.as_bytes())?;
        out.extend_from_slice(&sequence.to_be_bytes());
        out.extend_from_slice(&deadline_tick.to_be_bytes());
        out.extend_from_slice(&replay.resolution_tick.unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&replay.request_digest);
        out.extend_from_slice(&result_digest);
        out.extend_from_slice(&binding_digest);
        out
    } else {
        let mut out = b"MECER002".to_vec();
        field(&mut out, owner.as_bytes())?;
        out.extend_from_slice(&deadline_tick.to_be_bytes());
        out.extend_from_slice(&replay.request_digest);
        out
    };
    out.push(match response.disposition {
        CicsDisposition::Complete => 1,
        CicsDisposition::Suspended => 2,
        CicsDisposition::Transfer => 3,
        CicsDisposition::Handler => 4,
        CicsDisposition::Returned => 5,
        CicsDisposition::Abended => 6,
        CicsDisposition::Ignored => 7,
    });
    field(&mut out, response.condition.as_bytes())?;
    out.extend_from_slice(&response.response.to_be_bytes());
    out.extend_from_slice(&response.response2.to_be_bytes());
    field(&mut out, response.applid.as_bytes())?;
    field(&mut out, response.sysid.as_bytes())?;
    field(&mut out, response.transaction.as_bytes())?;
    out.push(response.aid);
    for value in [&response.target, &response.next_transaction] {
        match value {
            Some(value) => {
                out.push(1);
                field(&mut out, value.as_bytes())?;
            }
            None => out.push(0),
        }
    }
    field(&mut out, response.payload.schema().as_bytes())?;
    field(&mut out, response.payload.bytes())?;
    out.extend_from_slice(
        &u32::try_from(response.outputs.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &response.outputs {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value.schema().as_bytes())?;
        field(&mut out, value.bytes())?;
    }
    out.push(match response.unit_of_work {
        None => 0,
        Some(CicsUnitOfWorkOutcome::Committed) => 1,
        Some(CicsUnitOfWorkOutcome::RolledBack) => 2,
    });
    if let Some(alias) = &replay.read_alias {
        encode_alias(&mut out, alias)?;
    }
    Ok(out)
}

pub(crate) fn decode_cics_effect_replay(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsEffectReplay, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    let version = match reader.take(8)? {
        b"MECER001" => 1,
        b"MECER002" => 2,
        b"MECER003" => 3,
        b"MECER004" => 4,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let (effect_key, owner_execution, owner_run_unit, sequence, deadline_tick, resolution_tick) =
        if version >= 3 {
            let effect_key =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let owner_execution =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let owner_run_unit =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let sequence = u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
            let deadline_tick = u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
            let resolution_tick = match u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ) {
                0 => None,
                tick => Some(tick),
            };
            (
                Some(effect_key),
                Some(owner_execution),
                Some(owner_run_unit),
                Some(sequence),
                Some(deadline_tick),
                resolution_tick,
            )
        } else if version == 2 {
            (
                None,
                Some(
                    String::from_utf8(
                        reader.field(InvocationLimits::default().max_identity_bytes)?,
                    )
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                ),
                None,
                None,
                Some(u64::from_be_bytes(
                    reader
                        .take(8)?
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )),
                None,
            )
        } else {
            (None, None, None, None, None, None)
        };
    if owner_execution
        .as_ref()
        .is_some_and(|owner| ExecutionId::new(owner, InvocationLimits::default()).is_err())
        || owner_run_unit
            .as_ref()
            .is_some_and(|owner| RunUnitId::new(owner, InvocationLimits::default()).is_err())
        || effect_key
            .as_ref()
            .is_some_and(|key| IdempotencyKey::new(key, InvocationLimits::default()).is_err())
        || sequence == Some(0)
        || deadline_tick == Some(0)
        || resolution_tick == Some(0)
        || resolution_tick
            .zip(deadline_tick)
            .is_some_and(|(resolution, deadline)| resolution < deadline)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let request_digest = reader
        .take(32)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let (result_digest, binding_digest) = if version >= 3 {
        (
            Some(
                reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            Some(
                reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
        )
    } else {
        (None, None)
    };
    let disposition = match reader.take(1)?[0] {
        1 => CicsDisposition::Complete,
        2 => CicsDisposition::Suspended,
        3 => CicsDisposition::Transfer,
        4 => CicsDisposition::Handler,
        5 => CicsDisposition::Returned,
        6 => CicsDisposition::Abended,
        7 => CicsDisposition::Ignored,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let condition =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let response = i32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let response2 = i32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let applid =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let sysid =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let transaction =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let aid = reader.take(1)?[0];
    let mut optional = || -> Result<Option<String>, HostProblem> {
        match reader.take(1)?[0] {
            0 => Ok(None),
            1 => String::from_utf8(reader.field(128)?)
                .map(Some)
                .map_err(|_| HostProblem::InfrastructureFailure),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    };
    let target = optional()?;
    let next_transaction = optional()?;
    let payload_schema =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let payload = BoundedPayload::new(
        payload_schema,
        reader.field(limits.max_screen_bytes)?,
        InvocationLimits {
            max_payload_bytes: limits.max_screen_bytes,
            ..InvocationLimits::default()
        },
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let output_count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if output_count > limits.max_fields {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut outputs = BTreeMap::new();
    for _ in 0..output_count {
        let name = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let schema = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = BoundedPayload::new(
            schema,
            reader.field(limits.max_screen_bytes)?,
            InvocationLimits {
                max_payload_bytes: limits.max_screen_bytes,
                ..InvocationLimits::default()
            },
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if outputs.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let unit_of_work = match reader.take(1)?[0] {
        0 => None,
        1 => Some(CicsUnitOfWorkOutcome::Committed),
        2 => Some(CicsUnitOfWorkOutcome::RolledBack),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let read_alias = if version == 4 {
        Some(decode_alias(&mut reader)?)
    } else {
        None
    };
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsEffectReplay {
        effect_key,
        owner_execution,
        owner_run_unit,
        sequence,
        deadline_tick,
        resolution_tick,
        request_digest,
        result_digest,
        binding_digest,
        read_alias,
        response: CicsResponse {
            disposition,
            condition,
            response,
            response2,
            applid,
            sysid,
            transaction,
            aid,
            target,
            next_transaction,
            payload,
            outputs,
            unit_of_work,
        },
    })
}

pub(crate) fn cics_effect_replay_binding_digest(replay: &CicsEffectReplay) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cics-effect-replay-binding@1\0");
    for field in [
        replay.effect_key.as_deref().unwrap_or_default().as_bytes(),
        replay
            .owner_execution
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        replay
            .owner_run_unit
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update(replay.sequence.unwrap_or(0).to_be_bytes());
    hash.update(replay.deadline_tick.unwrap_or(0).to_be_bytes());
    hash.update(replay.resolution_tick.unwrap_or(0).to_be_bytes());
    hash.update(replay.request_digest);
    hash.update(replay.result_digest.unwrap_or([0; 32]));
    if let Some(alias) = &replay.read_alias {
        hash.update(b"mainframe-env.cics-plain-read-alias@1\0");
        for bytes in [
            alias.namespace.as_bytes(),
            alias.key.as_bytes(),
            alias.payload.as_slice(),
        ] {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        hash.update(alias.version.to_be_bytes());
    }
    hash.finalize().into()
}

#[cfg(test)]
#[path = "completed_plain_read_codec_tests.rs"]
mod tests;
