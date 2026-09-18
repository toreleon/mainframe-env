use super::{
    ONLINE_EXCHANGE_NAMESPACE, OnlineExchangeState, ProductServer, decode_online_exchange,
    encode_online_exchange, normalize_online_name, store_error,
};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, LifecycleEventKind, Machine, Principal, PrincipalId, RequestId, Selector,
    ServiceClass, Suspension, TraceId,
};
use mainframe_env_host_api::{HostProblem, ScopedHostService, SessionId};
use mainframe_env_interpreter::{ExecutionCoordinator, ReferenceMachine};
use mainframe_env_store_api::{ArtifactStore, ExecutionState, ProviderStateRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const ONLINE_PROVIDER_CAPABILITIES: [&str; 12] = [
    "host.security.authorize",
    "host.cics.execute",
    "host.dataset.read",
    "host.dataset.write",
    "host.db2.read",
    "host.db2.write",
    "host.ims.read",
    "host.ims.write",
    "host.mq.read",
    "host.mq.write",
    "host.program.invoke",
    "host.clock",
];

pub(super) struct OnlineMachineContinuation {
    pub(super) program: String,
    pub(super) artifact: ArtifactRef,
    pub(super) provider_generations: BTreeMap<CapabilityId, String>,
    pub(super) priority: Option<u8>,
    pub(super) checkpoint: BoundedPayload,
    pub(super) transfer: Option<PendingOnlineTransfer>,
    pub(super) version: u64,
}

fn online_transfer_exchange(
    current: &OnlineExchangeState,
    program: &str,
    invocation: &Invocation,
    payload: &BoundedPayload,
) -> Result<OnlineExchangeState, HostProblem> {
    let next = OnlineExchangeState {
        schema_version: current.schema_version.clone(),
        program: normalize_online_name(program, 128)?,
        request_id: invocation.request_id.as_str().into(),
        execution_id: invocation.execution_id.as_str().into(),
        run_unit_id: invocation.run_unit_id.as_str().into(),
        selector: invocation.selector.as_str().into(),
        artifact: invocation.artifact.as_str().into(),
        principal: invocation.principal.id().as_str().into(),
        grants: invocation
            .principal
            .grants()
            .iter()
            .map(|capability| capability.as_str().to_string())
            .collect(),
        provider_generations: invocation
            .provider_generations
            .iter()
            .map(|(capability, generation)| (capability.as_str().to_string(), generation.clone()))
            .collect(),
        priority: invocation.priority,
        deadline_tick: invocation.deadline_tick,
        trace_id: invocation.trace_id.as_str().into(),
        idempotency_key: invocation.idempotency_key.as_str().into(),
        attempt: invocation.attempt,
        audit_correlation: invocation.audit_correlation.clone(),
        transaction: current.transaction.clone(),
        commarea: payload.bytes().to_vec(),
        aid: current.aid,
        blocking_effect: None,
        version: current
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?,
    };
    super::validate_online_exchange(&next)?;
    if next.run_unit_id != current.run_unit_id
        || next.principal != current.principal
        || next.transaction != current.transaction
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(next)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingOnlineTransfer {
    pub(super) prior_execution_id: String,
    pub(super) expected_exchange_version: u64,
    pub(super) next_exchange: OnlineExchangeState,
}

impl ProductServer {
    pub(super) fn online_exchange(
        &self,
        session: &SessionId,
    ) -> Result<Option<OnlineExchangeState>, HostProblem> {
        self.store
            .get_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str())
            .map_err(store_error)?
            .map(|record| decode_online_exchange(&record))
            .transpose()
    }

    pub(super) fn persist_online_exchange(
        &self,
        session: &SessionId,
        state: &mut OnlineExchangeState,
    ) -> Result<(), HostProblem> {
        let previous = state.version;
        state.version = previous
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        if let Err(error) = self.store.put_provider_state(
            ProviderStateRecord {
                namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                key: session.as_str().into(),
                version: state.version,
                payload: encode_online_exchange(state)?,
            },
            Some(previous),
        ) {
            state.version = previous;
            return Err(store_error(error));
        }
        Ok(())
    }

    pub(super) fn discard_online_machine_run_if_present(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        preserve_handle_state: bool,
    ) -> Result<(), HostProblem> {
        let trace = if preserve_handle_state {
            self.cics
                .discard_handed_off_terminal_run_if_present(session, principal, now_tick)
        } else {
            self.cics
                .discard_terminal_run_if_present(session, principal, now_tick)
        }?;
        self.online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .entry(session.as_str().into())
            .or_default()
            .extend(trace);
        Ok(())
    }

    fn suspend_online_machine_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let trace = self.cics.terminal_run_trace(session, principal, now_tick)?;
        self.online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .entry(session.as_str().into())
            .or_default()
            .extend(trace);
        self.cics.suspend_terminal_run(session, principal, now_tick)
    }

    pub(super) fn online_machine_continuation(
        &self,
        session: &SessionId,
    ) -> Result<Option<OnlineMachineContinuation>, HostProblem> {
        self.store
            .get_provider_state("online-machine-continuation", session.as_str())
            .map_err(store_error)?
            .map(|record| decode_online_machine_continuation(&record))
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    fn persist_online_machine_continuation(
        &self,
        session: &SessionId,
        program: &str,
        artifact: &ArtifactRef,
        provider_generations: &BTreeMap<CapabilityId, String>,
        priority: u8,
        checkpoint: &BoundedPayload,
        current_version: Option<u64>,
    ) -> Result<u64, HostProblem> {
        let version = current_version
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "online-machine-continuation".into(),
                    key: session.as_str().into(),
                    version,
                    payload: encode_online_machine_continuation_with_transfer(
                        program,
                        artifact,
                        provider_generations,
                        priority,
                        checkpoint,
                        None,
                    )?,
                },
                current_version,
            )
            .map_err(store_error)?;
        Ok(version)
    }

    #[allow(clippy::too_many_arguments)]
    fn persist_pending_online_transfer(
        &self,
        session: &SessionId,
        program: &str,
        artifact: &ArtifactRef,
        provider_generations: &BTreeMap<CapabilityId, String>,
        priority: u8,
        checkpoint: &BoundedPayload,
        transfer: &PendingOnlineTransfer,
        current_version: Option<u64>,
    ) -> Result<u64, HostProblem> {
        let version = current_version
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "online-machine-continuation".into(),
                    key: session.as_str().into(),
                    version,
                    payload: encode_online_machine_continuation_with_transfer(
                        program,
                        artifact,
                        provider_generations,
                        priority,
                        checkpoint,
                        Some(transfer),
                    )?,
                },
                current_version,
            )
            .map_err(store_error)?;
        Ok(version)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn stage_online_program_transfer(
        &self,
        session: &SessionId,
        program: &str,
        artifact: &ArtifactRef,
        previous: &Invocation,
        payload: &BoundedPayload,
        current_version: Option<u64>,
        exchange: &mut OnlineExchangeState,
        coordinator: &ExecutionCoordinator,
        now_tick: u64,
    ) -> Result<(Invocation, BoundedPayload, u64), HostProblem> {
        let next = self.transferred_online_invocation(previous, program, artifact, payload)?;
        let record = self
            .artifacts
            .get_artifact(artifact)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let executable = super::admit_executable_artifact(&record)?;
        let mut checkpoint_invocation = next.clone();
        checkpoint_invocation.idempotency_key = IdempotencyKey::new(
            format!("{}:{program}", next.idempotency_key),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let machine = ReferenceMachine::from_binary(
            executable.payload(),
            checkpoint_invocation,
            mainframe_env_ir::CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        let checkpoint = machine.checkpoint().ok_or(HostProblem::ProviderFailure)?;
        let next_exchange = online_transfer_exchange(exchange, program, &next, payload)?;
        let pending = PendingOnlineTransfer {
            prior_execution_id: previous.execution_id.as_str().into(),
            expected_exchange_version: exchange.version,
            next_exchange,
        };
        let staged_version = self.persist_pending_online_transfer(
            session,
            program,
            artifact,
            &next.provider_generations,
            next.priority,
            &checkpoint,
            &pending,
            current_version,
        )?;
        let control = self
            .program
            .observe_execution_control(previous)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        coordinator
            .complete_suspended_handoff(previous, control.now_tick)
            .map_err(store_error)?;
        let mut persisted_exchange = pending.next_exchange.clone();
        persisted_exchange.version = exchange.version;
        self.persist_online_exchange(session, &mut persisted_exchange)?;
        *exchange = persisted_exchange;
        let settled_version = self.persist_online_machine_continuation(
            session,
            program,
            artifact,
            &next.provider_generations,
            next.priority,
            &checkpoint,
            Some(staged_version),
        )?;
        let trace = self
            .cics
            .terminal_run_trace(session, previous.principal.id(), now_tick)?;
        self.cics.restore_terminal_run(
            next.clone(),
            session,
            &exchange.transaction,
            payload.bytes().to_vec(),
            now_tick,
        )?;
        self.online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .entry(session.as_str().into())
            .or_default()
            .extend(trace);
        Ok((next, checkpoint, settled_version))
    }

    pub(super) fn recover_pending_online_transfer_if_present(
        &self,
        session: &SessionId,
        saved: &mut Option<OnlineMachineContinuation>,
        exchange: &mut Option<OnlineExchangeState>,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        if saved
            .as_ref()
            .is_none_or(|continuation| continuation.transfer.is_none())
        {
            return Ok(());
        }
        self.recover_pending_online_transfer(
            session,
            saved.as_mut().ok_or(HostProblem::InfrastructureFailure)?,
            exchange
                .as_mut()
                .ok_or(HostProblem::InfrastructureFailure)?,
            now_tick,
        )
    }

    fn recover_pending_online_transfer(
        &self,
        session: &SessionId,
        saved: &mut OnlineMachineContinuation,
        exchange: &mut OnlineExchangeState,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let Some(pending) = saved.transfer.clone() else {
            return Ok(());
        };
        let current_is_prior = exchange.execution_id == pending.prior_execution_id
            && exchange.version == pending.expected_exchange_version;
        let current_is_next = exchange == &pending.next_exchange;
        if !current_is_prior && !current_is_next {
            return Err(HostProblem::InfrastructureFailure);
        }
        let prior_id = ExecutionId::new(&pending.prior_execution_id, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let prior = self
            .store
            .get_execution(&prior_id)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if prior.state == ExecutionState::Suspended {
            if !current_is_prior {
                return Err(HostProblem::InfrastructureFailure);
            }
            let invocation = self.online_exchange_invocation(exchange)?;
            ExecutionCoordinator::durable(
                self.host.clone(),
                self.store.clone(),
                Default::default(),
            )
            .complete_suspended_handoff(&invocation, now_tick)
            .map_err(store_error)?;
        } else if prior.state != ExecutionState::Completed {
            return Err(HostProblem::InfrastructureFailure);
        }
        let prior = self
            .store
            .get_execution(&prior_id)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if prior.state != ExecutionState::Completed
            || !matches!(
                self.store
                    .events(&prior_id, prior.version, 1)
                    .map_err(store_error)?
                    .as_slice(),
                [event] if matches!(event.kind, LifecycleEventKind::HandoffCompleted)
            )
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if current_is_prior {
            let mut next = pending.next_exchange;
            next.version = exchange.version;
            self.persist_online_exchange(session, &mut next)?;
            *exchange = next;
        }
        saved.version = self.persist_online_machine_continuation(
            session,
            &saved.program,
            &saved.artifact,
            &saved.provider_generations,
            saved.priority.ok_or(HostProblem::InfrastructureFailure)?,
            &saved.checkpoint,
            Some(saved.version),
        )?;
        saved.transfer = None;
        Ok(())
    }

    fn transferred_online_invocation(
        &self,
        previous: &Invocation,
        program: &str,
        artifact: &ArtifactRef,
        payload: &BoundedPayload,
    ) -> Result<Invocation, HostProblem> {
        let sequence = self.next_sequence()?;
        let limits = InvocationLimits::default();
        let mut bindings = previous.bindings.clone();
        bindings.insert(
            "cics.commarea".into(),
            BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                payload.bytes().to_vec(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        let mut next = Invocation::new(
            RequestId::new(format!("online-transfer-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("online-transfer-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            previous.run_unit_id.clone(),
            None,
            Selector::new(format!("program:{program}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            artifact.clone(),
            Principal::new(
                previous.principal.id().clone(),
                previous.principal.grants().clone(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ServiceClass::Interactive,
            previous.priority,
            previous.deadline_tick,
            TraceId::new(format!("online-transfer-trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("online-transfer-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            previous.attempt,
            previous.limits,
            bindings,
            limits,
        )
        .and_then(|invocation| {
            invocation.with_provider_generations(previous.provider_generations.clone(), limits)
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        next.audit_correlation
            .clone_from(&previous.audit_correlation);
        next.cancellation.clone_from(&previous.cancellation);
        next.cancellation_probe
            .clone_from(&previous.cancellation_probe);
        Ok(next)
    }

    pub(super) fn clear_online_machine_continuation(
        &self,
        session: &SessionId,
        version: Option<u64>,
    ) -> Result<(), HostProblem> {
        if let Some(version) = version {
            self.store
                .delete_provider_state("online-machine-continuation", session.as_str(), version)
                .map_err(store_error)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_online_suspension(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        artifact: &ArtifactRef,
        invocation: &Invocation,
        machine: &ReferenceMachine,
        current_version: Option<u64>,
        suspension: &Suspension,
        coordinator: &ExecutionCoordinator,
        exchange: &OnlineExchangeState,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let checkpoint = machine.checkpoint().ok_or(HostProblem::ProviderFailure)?;
        self.persist_online_machine_continuation(
            session,
            program,
            artifact,
            &invocation.provider_generations,
            machine.invocation_priority(),
            &checkpoint,
            current_version,
        )?;
        match suspension.kind.as_str() {
            "cics-delay" | "cics-enqueue" | "cics-scheduler" => return Ok(()),
            "cics-terminal" => {}
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        let control = self
            .program
            .observe_execution_control(invocation)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        coordinator
            .complete_suspended_handoff(invocation, control.now_tick)
            .map_err(store_error)?;
        self.program.finish_run_unit(invocation)?;
        self.suspend_online_machine_run(session, principal, now_tick)?;
        self.clear_online_exchange(session, exchange)
    }
}

#[cfg(test)]
pub(super) fn encode_online_machine_continuation(
    program: &str,
    artifact: &ArtifactRef,
    provider_generations: &BTreeMap<CapabilityId, String>,
    priority: u8,
    checkpoint: &BoundedPayload,
) -> Result<Vec<u8>, HostProblem> {
    encode_online_machine_continuation_with_transfer(
        program,
        artifact,
        provider_generations,
        priority,
        checkpoint,
        None,
    )
}

pub(super) fn encode_online_machine_continuation_with_transfer(
    program: &str,
    artifact: &ArtifactRef,
    provider_generations: &BTreeMap<CapabilityId, String>,
    priority: u8,
    checkpoint: &BoundedPayload,
    transfer: Option<&PendingOnlineTransfer>,
) -> Result<Vec<u8>, HostProblem> {
    let generations = encode_provider_generations(provider_generations)?;
    let priority = [priority];
    let transfer = transfer
        .map(|transfer| {
            validate_pending_transfer(transfer)?;
            serde_json::to_vec(transfer).map_err(|_| HostProblem::InfrastructureFailure)
        })
        .transpose()?
        .unwrap_or_default();
    let mut encoded = b"MEOM4".to_vec();
    for value in [
        program.as_bytes(),
        artifact.as_str().as_bytes(),
        &generations,
        &priority,
        checkpoint.schema().as_bytes(),
        checkpoint.bytes(),
        &transfer,
    ] {
        encoded.extend_from_slice(
            &u32::try_from(value.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(value);
    }
    Ok(encoded)
}

pub(super) fn decode_online_machine_continuation(
    record: &ProviderStateRecord,
) -> Result<OnlineMachineContinuation, HostProblem> {
    let current = record.payload.starts_with(b"MEOM4");
    let has_priority = current || record.payload.starts_with(b"MEOM3");
    if !has_priority && !record.payload.starts_with(b"MEOM2") {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 5usize;
    let mut next = || -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            record
                .payload
                .get(at..at + 4)
                .ok_or(HostProblem::InfrastructureFailure)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        at += 4;
        let end = at
            .checked_add(length)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = record
            .payload
            .get(at..end)
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_vec();
        at = end;
        Ok(value)
    };
    let program = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let artifact = ArtifactRef::new(
        String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let provider_generations = decode_provider_generations(&next()?)?;
    let priority = if has_priority {
        match next()?.as_slice() {
            [priority] => Some(*priority),
            _ => return Err(HostProblem::InfrastructureFailure),
        }
    } else {
        None
    };
    let schema = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let bytes = next()?;
    let transfer = if current {
        let bytes = next()?;
        if bytes.is_empty() {
            None
        } else {
            let mut transfer: PendingOnlineTransfer =
                serde_json::from_slice(&bytes).map_err(|_| HostProblem::InfrastructureFailure)?;
            if serde_json::to_vec(&transfer).map_err(|_| HostProblem::InfrastructureFailure)?
                != bytes
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            transfer.next_exchange.version = transfer
                .expected_exchange_version
                .checked_add(1)
                .ok_or(HostProblem::InfrastructureFailure)?;
            validate_pending_transfer(&transfer)?;
            Some(transfer)
        }
    } else {
        None
    };
    if transfer.as_ref().is_some_and(|transfer| {
        transfer.next_exchange.program != program
            || transfer.next_exchange.artifact != artifact.as_str()
            || transfer.next_exchange.priority != priority.unwrap_or_default()
            || transfer.next_exchange.provider_generations.len() != provider_generations.len()
            || provider_generations.iter().any(|(capability, generation)| {
                transfer
                    .next_exchange
                    .provider_generations
                    .get(capability.as_str())
                    != Some(generation)
            })
    }) {
        return Err(HostProblem::InfrastructureFailure);
    }
    if at != record.payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(OnlineMachineContinuation {
        program: normalize_online_name(&program, 128)?,
        artifact,
        provider_generations,
        priority,
        checkpoint: BoundedPayload::new(
            schema,
            bytes,
            InvocationLimits {
                max_payload_bytes: 64 * 1024 * 1024,
                ..InvocationLimits::default()
            },
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        transfer,
        version: record.version,
    })
}

fn validate_pending_transfer(transfer: &PendingOnlineTransfer) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    ExecutionId::new(&transfer.prior_execution_id, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if transfer.expected_exchange_version == 0
        || Some(transfer.next_exchange.version) != transfer.expected_exchange_version.checked_add(1)
        || transfer.next_exchange.execution_id == transfer.prior_execution_id
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    super::validate_online_exchange(&transfer.next_exchange)
}

pub(super) fn restore_online_machine_priority(
    invocation: &mut Invocation,
    continuation: Option<&OnlineMachineContinuation>,
) {
    if let Some(priority) = continuation.and_then(|continuation| continuation.priority) {
        invocation.priority = priority;
    }
}

fn encode_provider_generations(
    generations: &BTreeMap<CapabilityId, String>,
) -> Result<Vec<u8>, HostProblem> {
    let limits = InvocationLimits::default();
    if !has_exact_provider_generation_shape(generations)
        || generations
            .values()
            .any(|generation| generation.is_empty() || generation.len() > limits.max_identity_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    serde_json::to_vec(
        &generations
            .iter()
            .map(|(capability, generation)| (capability.as_str().to_string(), generation.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_provider_generations(
    bytes: &[u8],
) -> Result<BTreeMap<CapabilityId, String>, HostProblem> {
    let values: BTreeMap<String, String> =
        serde_json::from_slice(bytes).map_err(|_| HostProblem::InfrastructureFailure)?;
    if serde_json::to_vec(&values).map_err(|_| HostProblem::InfrastructureFailure)? != bytes {
        return Err(HostProblem::InfrastructureFailure);
    }
    let limits = InvocationLimits::default();
    if values.len() > limits.max_capabilities {
        return Err(HostProblem::InfrastructureFailure);
    }
    let generations = values
        .into_iter()
        .map(|(capability, generation)| {
            if generation.is_empty() || generation.len() > limits.max_identity_bytes {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok((
                CapabilityId::new(capability, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                generation,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    if !has_exact_provider_generation_shape(&generations) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(generations)
}

pub(super) fn preflight_provider_generations(
    host: &ScopedHostService,
    generations: &BTreeMap<CapabilityId, String>,
) -> Result<(), HostProblem> {
    if !has_exact_provider_generation_shape(generations) {
        return Err(HostProblem::ProviderFailure);
    }
    host.validate_provider_generations(generations)
}

fn has_exact_provider_generation_shape(generations: &BTreeMap<CapabilityId, String>) -> bool {
    generations.len() == ONLINE_PROVIDER_CAPABILITIES.len()
        && ONLINE_PROVIDER_CAPABILITIES.iter().all(|expected| {
            generations
                .keys()
                .any(|capability| capability.as_str() == *expected)
        })
}
