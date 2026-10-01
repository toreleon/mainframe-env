//! Existing online exchange codec and durable COBOL run-owner handoff.
use super::*;

pub(super) const EXCHANGE_WITH_OWNER: &str = "mainframe-env.online-exchange@2";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Immutable original execution and its exchange-specific integrity binding.
pub(super) struct OnlineRunOwner {
    pub(super) execution: String,
    metadata_digest: String,
}

fn owner_digest(state: &OnlineExchangeState, execution: &str) -> Result<String, HostProblem> {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.online-run-owner@2\0");
    for field in [
        execution,
        &state.execution_id,
        &state.run_unit_id,
        &state.principal,
        &state.program,
        &state.selector,
        &state.artifact,
        &state.transaction,
    ] {
        super::digest_online_field(&mut digest, field.as_bytes());
    }
    let scope = serde_json::to_vec(&(
        &state.grants,
        &state.provider_generations,
        state.deadline_tick,
        state.attempt,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    super::digest_online_field(&mut digest, &scope);
    Ok(super::hex_digest(&digest.finalize()))
}

pub(super) fn new_run_owner(
    state: &OnlineExchangeState,
    execution: &str,
) -> Result<OnlineRunOwner, HostProblem> {
    if execution.is_empty()
        || execution.len() > 128
        || !execution.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
        || ExecutionId::new(execution, InvocationLimits::default()).is_err()
    {
        return Err(HostProblem::Malformed);
    }
    Ok(OnlineRunOwner {
        execution: execution.into(),
        metadata_digest: owner_digest(state, execution)?,
    })
}

fn validate_run_owner(state: &OnlineExchangeState) -> Result<(), HostProblem> {
    match (&state.run_owner, state.schema_version.as_str()) {
        (None, ONLINE_EXCHANGE_CONTRACT) => Ok(()),
        (Some(owner), EXCHANGE_WITH_OWNER) => {
            let expected = new_run_owner(state, &owner.execution)?;
            if expected == *owner {
                Ok(())
            } else {
                Err(HostProblem::InfrastructureFailure)
            }
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn verify_run_owner(
    store: &dyn PlatformStore,
    state: &OnlineExchangeState,
) -> Result<(), HostProblem> {
    validate_online_exchange(state)?;
    let Some(owner) = &state.run_owner else {
        return Ok(());
    };
    if owner.execution == state.execution_id {
        return Ok(());
    }
    let id = ExecutionId::new(&owner.execution, InvocationLimits::default())
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let original = store
        .get_execution(&id)
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    if original.execution_id != id
        || original.state != ExecutionState::Completed
        || original.run_unit_id.as_str() != state.run_unit_id
        || original.principal.as_str() != state.principal
        || original.terminal_tick.is_none_or(|tick| tick == 0)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let events = store
        .events(&id, original.version, 1)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    if !matches!(events.as_slice(), [event]
        if event.execution_id == id && event.run_unit_id == original.run_unit_id
            && event.sequence == original.version && event.attempt == original.attempt
            && Some(event.tick) == original.terminal_tick
            && event.kind == mainframe_env_execution_api::LifecycleEventKind::HandoffCompleted)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

pub(super) fn bind_transfer_owner(
    current: &OnlineExchangeState,
    previous: &Invocation,
    next: &mut Invocation,
) -> Result<(), HostProblem> {
    validate_online_exchange(current)?;
    let owner = current.run_owner.as_ref().ok_or(HostProblem::Unsupported)?;
    if previous.execution_id.as_str() != current.execution_id
        || previous.run_unit_id.as_str() != current.run_unit_id
        || previous.principal.id().as_str() != current.principal
        || crate::cobol::program_run_owner(previous)? != owner.execution
    {
        return Err(HostProblem::UnknownOutcome);
    }
    crate::cobol::restore_program_run_owner(next, &owner.execution)
}

/// Enumerate only identities validated by the owning online exchange codec.
pub(crate) fn online_exchange_dependencies(
    record: &ProviderStateRecord,
) -> Result<Vec<ExecutionId>, HostProblem> {
    if record.namespace != ONLINE_EXCHANGE_NAMESPACE || SessionId::new(&record.key, 64).is_err() {
        return Err(HostProblem::Malformed);
    }
    let state = decode_online_exchange(record)?;
    let mut result = vec![
        ExecutionId::new(&state.execution_id, InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?,
    ];
    if let Some(owner) = &state.run_owner {
        let id = ExecutionId::new(&owner.execution, InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        if !result.contains(&id) {
            result.push(id);
        }
    }
    Ok(result)
}

impl ProductServer {
    /// Reuse validated online ownership for CICS task resources, not effect identity.
    pub(super) fn restore_online_cics_run(
        &self,
        state: &OnlineExchangeState,
        actor: Invocation,
        session: &SessionId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        verify_run_owner(self.store.as_ref(), state)?;
        if actor.execution_id.as_str() != state.execution_id
            || actor.run_unit_id.as_str() != state.run_unit_id
            || actor.principal.id().as_str() != state.principal
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut task = actor.clone();
        if let Some(owner) = &state.run_owner {
            if crate::cobol::program_run_owner(&actor)? != owner.execution {
                return Err(HostProblem::UnknownOutcome);
            }
            if owner.execution != actor.execution_id.as_str() {
                let id = ExecutionId::new(&owner.execution, InvocationLimits::default())
                    .map_err(|_| HostProblem::UnknownOutcome)?;
                let original = self
                    .store
                    .get_execution(&id)
                    .map_err(|_| HostProblem::UnknownOutcome)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                task.execution_id = id;
                task.selector = original.selector;
                task.artifact = original.artifact;
            }
        }
        self.cics.restore_terminal_program_run(
            task,
            actor,
            session,
            &state.transaction,
            state.commarea.clone(),
            now_tick,
        )
    }

    pub(super) fn begin_online_exchange(
        &self,
        session: &SessionId,
        program: &str,
        context: &CicsTerminalExecution,
    ) -> Result<OnlineExchangeState, HostProblem> {
        let mut state = OnlineExchangeState {
            schema_version: EXCHANGE_WITH_OWNER.into(),
            run_owner: None,
            program: normalize_online_name(program, 128)?,
            request_id: context.invocation.request_id.as_str().into(),
            execution_id: context.invocation.execution_id.as_str().into(),
            run_unit_id: context.invocation.run_unit_id.as_str().into(),
            selector: context.invocation.selector.as_str().into(),
            artifact: context.invocation.artifact.as_str().into(),
            principal: context.invocation.principal.id().as_str().into(),
            grants: context
                .invocation
                .principal
                .grants()
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect(),
            provider_generations: context
                .invocation
                .provider_generations
                .iter()
                .map(|(capability, generation)| {
                    (capability.as_str().to_string(), generation.clone())
                })
                .collect(),
            priority: context.invocation.priority,
            deadline_tick: context.invocation.deadline_tick,
            trace_id: context.invocation.trace_id.as_str().into(),
            idempotency_key: context.invocation.idempotency_key.as_str().into(),
            attempt: context.invocation.attempt,
            audit_correlation: context.invocation.audit_correlation.clone(),
            transaction: normalize_online_name(&context.transaction, 16)?,
            commarea: context.commarea.clone(),
            aid: context.aid,
            blocking_effect: None,
            version: 1,
        };
        state.run_owner = Some(new_run_owner(
            &state,
            &crate::cobol::program_run_owner(&context.invocation)?,
        )?);
        validate_online_exchange(&state)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: session.as_str().into(),
                    version: state.version,
                    payload: encode_online_exchange(&state)?,
                },
                None,
            )
            .map_err(store_error)?;
        Ok(state)
    }

    pub(super) fn clear_online_exchange(
        &self,
        session: &SessionId,
        state: &OnlineExchangeState,
    ) -> Result<(), HostProblem> {
        self.store
            .delete_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str(), state.version)
            .map_err(store_error)
    }

    pub(super) fn online_exchange_invocation(
        &self,
        state: &OnlineExchangeState,
    ) -> Result<Invocation, HostProblem> {
        validate_online_exchange(state)?;
        verify_run_owner(self.store.as_ref(), state)?;
        let limits = InvocationLimits::default();
        let grants = state
            .grants
            .iter()
            .map(|capability| {
                CapabilityId::new(capability, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let generations = state
            .provider_generations
            .iter()
            .map(|(capability, generation)| {
                Ok((
                    CapabilityId::new(capability, limits)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                    generation.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, HostProblem>>()?;
        let deadline_tick = current_gateway_call_context()
            .map_or(state.deadline_tick, |context| context.deadline_tick());
        let mut invocation = Invocation::new(
            RequestId::new(&state.request_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(&state.execution_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(&state.run_unit_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            None,
            Selector::new(&state.selector, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ArtifactRef::new(&state.artifact, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            Principal::new(
                PrincipalId::new(&state.principal, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                grants,
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ServiceClass::Interactive,
            state.priority,
            deadline_tick,
            TraceId::new(&state.trace_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(&state.idempotency_key, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state.attempt,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .and_then(|invocation| invocation.with_provider_generations(generations, limits))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        invocation
            .audit_correlation
            .clone_from(&state.audit_correlation);
        if let Some(owner) = &state.run_owner {
            crate::cobol::restore_program_run_owner(&mut invocation, &owner.execution)?;
        }
        if let Some(context) = current_gateway_call_context() {
            invocation = invocation.with_cancellation_probe(context.cancellation_probe());
        }
        Ok(invocation)
    }
}

pub(super) fn encode_online_exchange(state: &OnlineExchangeState) -> Result<Vec<u8>, HostProblem> {
    validate_online_exchange(state)?;
    serde_json::to_vec(state).map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn decode_online_exchange(
    record: &ProviderStateRecord,
) -> Result<OnlineExchangeState, HostProblem> {
    let mut state: OnlineExchangeState =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    state.version = record.version;
    validate_online_exchange(&state)?;
    Ok(state)
}

pub(super) fn validate_online_exchange(state: &OnlineExchangeState) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    if !matches!(
        state.schema_version.as_str(),
        ONLINE_EXCHANGE_CONTRACT | EXCHANGE_WITH_OWNER
    ) || state.version == 0
        || normalize_online_name(&state.program, 128)? != state.program
        || normalize_online_name(&state.transaction, 16)? != state.transaction
        || state.deadline_tick == 0
        || state.attempt == 0
        || state.grants.is_empty()
        || state.grants.len() > limits.max_capabilities
        || state.provider_generations.len() > limits.max_capabilities
        || state.commarea.len() > limits.max_payload_bytes
        || state.audit_correlation.is_empty()
        || state.audit_correlation.len() > limits.max_identity_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    RequestId::new(&state.request_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    ExecutionId::new(&state.execution_id, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    RunUnitId::new(&state.run_unit_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Selector::new(&state.selector, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    ArtifactRef::new(&state.artifact, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    PrincipalId::new(&state.principal, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    TraceId::new(&state.trace_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    IdempotencyKey::new(&state.idempotency_key, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    for grant in &state.grants {
        CapabilityId::new(grant, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    for (capability, generation) in &state.provider_generations {
        if !state.grants.contains(capability)
            || generation.is_empty()
            || generation.len() > limits.max_identity_bytes
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        CapabilityId::new(capability, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    if let Some(key) = &state.blocking_effect {
        IdempotencyKey::new(key, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    validate_run_owner(state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retention_maintenance::provider::RetentionPlanner;
    use mainframe_env_execution_api::{LifecycleEvent, LifecycleEventKind};
    use mainframe_env_store_api::{ExecutionRecord, RetentionPolicy};

    fn state() -> OnlineExchangeState {
        let mut state = OnlineExchangeState {
            schema_version: EXCHANGE_WITH_OWNER.into(),
            run_owner: None,
            program: "EXIT".into(),
            request_id: "request".into(),
            execution_id: "exit-execution".into(),
            run_unit_id: "root-run".into(),
            selector: "program:EXIT".into(),
            artifact: "sha256:exit".into(),
            principal: "IBMUSER".into(),
            grants: BTreeSet::from(["host.cics.execute".into()]),
            provider_generations: BTreeMap::from([("host.cics.execute".into(), "cics@1".into())]),
            priority: 100,
            deadline_tick: 10_000,
            trace_id: "trace".into(),
            idempotency_key: "exit-key".into(),
            attempt: 1,
            audit_correlation: "audit".into(),
            transaction: "PE01".into(),
            commarea: b"OWNER123".to_vec(),
            aid: 0,
            blocking_effect: None,
            version: 1,
        };
        state.run_owner = Some(new_run_owner(&state, "root-execution").unwrap());
        state
    }

    fn row(state: &OnlineExchangeState) -> ProviderStateRecord {
        ProviderStateRecord {
            namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
            key: SessionId::new("owner-session", 64).unwrap().as_str().into(),
            version: state.version,
            payload: encode_online_exchange(state).unwrap(),
        }
    }

    fn server() -> Arc<ProductServer> {
        ProductServer::memory(ServerConfig {
            store_profile: crate::StoreProfile::Memory,
            tls: crate::TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            artifact_root: std::env::temp_dir().join(format!(
                "online-owner-tests-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            )),
            ..ServerConfig::default()
        })
        .unwrap()
    }

    fn original(
        store: &dyn PlatformStore,
        run: &str,
        principal: &str,
        kind: Option<LifecycleEventKind>,
    ) {
        let limits = InvocationLimits::default();
        let id = ExecutionId::new("root-execution", limits).unwrap();
        store
            .create_execution(ExecutionRecord {
                execution_id: id.clone(),
                run_unit_id: RunUnitId::new(run, limits).unwrap(),
                selector: Selector::new("program:ROOT", limits).unwrap(),
                artifact: ArtifactRef::new("sha256:root", limits).unwrap(),
                principal: PrincipalId::new(principal, limits).unwrap(),
                state: ExecutionState::Admitted,
                attempt: 1,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: None,
            })
            .unwrap();
        let queued = store
            .transition_execution(&id, 1, ExecutionState::Queued, 1)
            .unwrap();
        let running = store
            .transition_execution(&id, queued.version, ExecutionState::Running, 2)
            .unwrap();
        let done = if kind == Some(LifecycleEventKind::Abend) {
            store
                .transition_execution(&id, running.version, ExecutionState::Failed, 4)
                .unwrap()
        } else {
            let suspended = store
                .transition_execution(&id, running.version, ExecutionState::Suspended, 3)
                .unwrap();
            store
                .transition_execution(&id, suspended.version, ExecutionState::Completed, 4)
                .unwrap()
        };
        if let Some(kind) = kind {
            for sequence in 1..=done.version {
                store
                    .append_event(LifecycleEvent {
                        execution_id: id.clone(),
                        run_unit_id: done.run_unit_id.clone(),
                        sequence,
                        attempt: done.attempt,
                        tick: done.terminal_tick.unwrap(),
                        kind: if sequence == done.version {
                            kind.clone()
                        } else {
                            LifecycleEventKind::Admitted
                        },
                    })
                    .unwrap();
            }
        }
    }

    #[test]
    fn owner_codec_rejects_corruption_without_promoting_legacy_exchange() {
        let state = state();
        assert_eq!(
            state.run_owner.as_ref().unwrap().metadata_digest,
            "f98ef027cc3ba49dbbec22fd9f99f9f4676517cc5c75663b4ea68495d6a7e0c0"
        );
        assert_eq!(decode_online_exchange(&row(&state)).unwrap(), state);
        for field in [
            "execution_id",
            "run_unit_id",
            "principal",
            "program",
            "selector",
            "artifact",
            "transaction",
        ] {
            let mut record = row(&state);
            let mut value: Value = serde_json::from_slice(&record.payload).unwrap();
            value[field] = Value::String("FOREIGN".into());
            record.payload = serde_json::to_vec(&value).unwrap();
            assert!(decode_online_exchange(&record).is_err(), "{field}");
        }
        for case in 0..6 {
            let mut record = row(&state);
            let mut value: Value = serde_json::from_slice(&record.payload).unwrap();
            match case {
                0 => value["schema_version"] = ONLINE_EXCHANGE_CONTRACT.into(),
                1 => {
                    value.as_object_mut().unwrap().remove("run_owner");
                }
                2 => value["run_owner"]["execution"] = "foreign-root".into(),
                3 => value["run_owner"]["metadata_digest"] = "0".repeat(64).into(),
                4 => value["run_owner"]["extra"] = true.into(),
                _ => value["attempt"] = 2.into(),
            }
            record.payload = serde_json::to_vec(&value).unwrap();
            assert!(decode_online_exchange(&record).is_err(), "case {case}");
        }
        for owner in ["", "bad owner", "é", &"a".repeat(129)] {
            assert!(new_run_owner(&state, owner).is_err());
        }
        let mut legacy = state.clone();
        legacy.schema_version = ONLINE_EXCHANGE_CONTRACT.into();
        legacy.run_owner = None;
        let record = row(&legacy);
        assert!(
            !serde_json::from_slice::<Value>(&record.payload)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("run_owner")
        );
        assert_eq!(
            encode_online_exchange(&decode_online_exchange(&record).unwrap()).unwrap(),
            record.payload
        );
        assert_eq!(online_exchange_dependencies(&record).unwrap().len(), 1);
    }

    #[test]
    fn owner_restore_requires_the_exact_original_completed_handoff() {
        for case in 0..7 {
            let store = MemoryStore::new(Default::default());
            if case != 0 {
                original(
                    &store,
                    if case == 2 { "foreign-run" } else { "root-run" },
                    if case == 3 { "FOREIGN" } else { "IBMUSER" },
                    match case {
                        1 => None,
                        4 => Some(LifecycleEventKind::Completed { return_code: 0 }),
                        6 => Some(LifecycleEventKind::Abend),
                        _ => Some(LifecycleEventKind::HandoffCompleted),
                    },
                );
            }
            assert_eq!(
                verify_run_owner(&store, &state()).is_ok(),
                case == 5,
                "case {case}"
            );
        }
        let server = server();
        original(
            server.store.as_ref(),
            "root-run",
            "IBMUSER",
            Some(LifecycleEventKind::HandoffCompleted),
        );
        let invocation = server.online_exchange_invocation(&state()).unwrap();
        assert_eq!(
            crate::cobol::program_run_owner(&invocation).unwrap(),
            "root-execution"
        );
    }

    #[test]
    fn repeated_actor_replacement_preserves_owner_and_rejects_conflicting_binding() {
        let server = server();
        original(
            server.store.as_ref(),
            "root-run",
            "IBMUSER",
            Some(LifecycleEventKind::HandoffCompleted),
        );
        let mut current = state();
        let mut previous = server.online_exchange_invocation(&current).unwrap();
        for name in ["second-execution", "third-execution"] {
            let mut next = previous.clone();
            next.execution_id = ExecutionId::new(name, InvocationLimits::default()).unwrap();
            next.bindings.clear();
            bind_transfer_owner(&current, &previous, &mut next).unwrap();
            assert_eq!(
                crate::cobol::program_run_owner(&next).unwrap(),
                "root-execution"
            );
            current.execution_id = name.into();
            current.run_owner = Some(new_run_owner(&current, "root-execution").unwrap());
            previous = next;
        }
        let mut conflicting = previous.clone();
        conflicting.bindings.clear();
        crate::cobol::restore_program_run_owner(&mut conflicting, "foreign-root").unwrap();
        let before = conflicting.bindings.clone();
        assert_eq!(
            bind_transfer_owner(&current, &previous, &mut conflicting),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(conflicting.bindings, before);
        current.run_owner = None;
        current.schema_version = ONLINE_EXCHANGE_CONTRACT.into();
        assert_eq!(
            bind_transfer_owner(&current, &previous, &mut conflicting),
            Err(HostProblem::Unsupported)
        );
    }

    #[test]
    fn owner_mutation_and_stale_cas_cannot_update_exchange_or_version() {
        let server = server();
        original(
            server.store.as_ref(),
            "root-run",
            "IBMUSER",
            Some(LifecycleEventKind::HandoffCompleted),
        );
        let session = SessionId::new("owner-session", 64).unwrap();
        let original_row = row(&state());
        server
            .store
            .put_provider_state(original_row.clone(), None)
            .unwrap();
        let mut changed = state();
        changed.run_owner = Some(new_run_owner(&changed, "foreign-root").unwrap());
        assert_eq!(
            server.persist_online_exchange(&session, &mut changed),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(changed.version, 1);
        let mut invalid = state();
        invalid.run_owner.as_mut().unwrap().metadata_digest = "0".repeat(64);
        assert!(
            server
                .persist_online_exchange(&session, &mut invalid)
                .is_err()
        );
        assert_eq!(invalid.version, 1);
        assert_eq!(
            server
                .store
                .get_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str())
                .unwrap(),
            Some(original_row)
        );
        let mut updated = state();
        updated.commarea = b"UPDATED!".to_vec();
        server
            .persist_online_exchange(&session, &mut updated)
            .unwrap();
        assert_eq!(updated.version, 2);
        let mut stale = state();
        assert_eq!(
            server.persist_online_exchange(&session, &mut stale),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(stale.version, 1);
        assert_eq!(server.online_exchange(&session).unwrap().unwrap(), updated);
    }

    #[test]
    fn retention_pins_online_owner_and_fences_corrupt_or_orphan_rows() {
        use super::super::continuation::{
            PendingOnlineTransfer, encode_online_machine_continuation_with_transfer,
        };
        for case in 0..4 {
            let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
            if case != 2 {
                let mut exchange = row(&state());
                if case == 1 {
                    exchange.payload = b"corrupt".to_vec();
                }
                store.put_provider_state(exchange, None).unwrap();
            }
            let generations: BTreeMap<CapabilityId, String> =
                super::super::continuation::ONLINE_PROVIDER_CAPABILITIES
                    .iter()
                    .map(|name| {
                        (
                            CapabilityId::new(*name, InvocationLimits::default()).unwrap(),
                            "provider@1".into(),
                        )
                    })
                    .collect();
            let mut next = OnlineExchangeState {
                version: 2,
                ..state()
            };
            next.grants = generations
                .keys()
                .map(|cap| cap.as_str().to_string())
                .collect();
            next.provider_generations = generations
                .iter()
                .map(|(cap, generation)| (cap.as_str().to_string(), generation.clone()))
                .collect();
            next.run_owner = Some(new_run_owner(&next, "root-execution").unwrap());
            let pending = PendingOnlineTransfer {
                prior_execution_id: "prior-execution".into(),
                expected_exchange_version: 1,
                next_exchange: next,
            };
            let checkpoint =
                BoundedPayload::new("test-checkpoint@1", vec![1], InvocationLimits::default())
                    .unwrap();
            let artifact = ArtifactRef::new("sha256:exit", InvocationLimits::default()).unwrap();
            let continuation = ProviderStateRecord {
                namespace: "online-machine-continuation".into(),
                key: SessionId::new("owner-session", 64).unwrap().as_str().into(),
                version: 1,
                payload: if case == 3 {
                    b"corrupt".to_vec()
                } else {
                    encode_online_machine_continuation_with_transfer(
                        "EXIT",
                        &artifact,
                        &generations,
                        100,
                        &checkpoint,
                        Some(&pending),
                    )
                    .unwrap()
                },
            };
            if case == 0 {
                assert!(super::super::online_continuation_dependencies(&continuation).is_ok());
            }
            store.put_provider_state(continuation, None).unwrap();
            let planner = RetentionPlanner::from_existing(
                store,
                RetentionPolicy {
                    lifecycle_ticks: 1,
                    idempotency_ticks: 1,
                    audit_ticks: 1,
                    archive_ticks: 1,
                    low_watermark_percent: 70,
                    high_watermark_percent: 85,
                    max_batch: 8,
                },
                None,
            )
            .unwrap();
            let deps = planner.core_dependencies().unwrap();
            assert_eq!(deps.unowned, case != 0, "case {case}");
            if case == 0 {
                assert_eq!(
                    deps.blocked_executions
                        .iter()
                        .map(ExecutionId::as_str)
                        .collect::<Vec<_>>(),
                    ["exit-execution", "prior-execution", "root-execution"]
                );
            }
        }
    }
}
