//! Actual single-lock native root publication, using touched-entry rollback.
use super::*;
use crate::root_terminal::{
    ACTOR_NAMESPACE, Document, Phase, ROW_SCOPE_NAMESPACE, RUN_NAMESPACE, SCOPE_NAMESPACE,
    row_scope_key,
};
use mainframe_env_execution_api::{AuditSubjectRecord, InvocationLimits};
use mainframe_env_store_api::{
    MAX_ROOT_OPERATIONS, ROOT_DRIVER_NAMESPACE, RootActorSnapshot, RootChildAdmission,
    RootClosureSnapshot, RootDriverAdmission, RootDriverClaim, RootProviderRowAdmission,
    RootTerminalCommit, RootTerminalPublication, TerminalRowDependency,
};
mod guards;
pub(super) use guards::{
    guard_actor, guard_effect, guard_outbox_delivery, guard_provider, guard_unenrolled, guard_work,
};

fn row<'a>(
    state: &'a State,
    namespace: &str,
    key: &str,
) -> Result<&'a ProviderStateRecord, StoreError> {
    state
        .provider_state
        .get(&(namespace.into(), key.into()))
        .ok_or(StoreError::NotFound)
}
fn index(namespace: &str, key: &str, root: &str) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version: 1,
        payload: root.as_bytes().to_vec(),
    }
}
fn write(
    state: &mut State,
    journal: &mut journal::Journal,
    record: ProviderStateRecord,
    expected: Option<u64>,
    limits: StoreLimits,
) -> Result<(), StoreError> {
    journal.touch_provider_state(state, &(record.namespace.clone(), record.key.clone()));
    journal.touch_blob_bytes(state);
    journal.touch_provider_epoch(state);
    MemoryStore::put_provider_state_locked(state, record, expected, limits)
}
fn admit(
    state: &mut State,
    journal: &mut journal::Journal,
    execution: ExecutionRecord,
    event: LifecycleEvent,
    notification: OutboxRecord,
    limits: StoreLimits,
) -> Result<(), StoreError> {
    validation::admission(&execution, &event, &notification)?;
    MemoryStore::validate_encoded_size(encode_execution(&execution)?, limits)?;
    if state.executions.contains_key(&execution.execution_id) {
        return Err(StoreError::AlreadyExists);
    }
    if state.executions.len() >= limits.max_executions {
        return Err(StoreError::CapacityExceeded);
    }
    journal.touch_execution(state, &execution.execution_id);
    state
        .executions
        .insert(execution.execution_id.clone(), execution);
    journal.touch_events(state, &event.execution_id);
    journal.touch_provider_epoch(state);
    MemoryStore::append_event_locked(state, event, limits)?;
    journal.touch_outbox(state, &notification.notification_id);
    journal.touch_blob_bytes(state);
    journal.touch_provider_epoch(state);
    MemoryStore::append_outbox_locked(state, notification, limits)
}

fn capture(
    state: &State,
    claim: &RootDriverClaim,
    closing: ProviderStateRecord,
    observed_tick: u64,
) -> Result<RootClosureSnapshot, StoreError> {
    let doc = Document::read(&closing)?;
    // Refuse the whole prospective capture before cloning rows or allocating
    // codec buffers. This conservative estimate is a bound, not a codec.
    preflight_capture(state, &doc, claim, &closing)?;
    let mut actors = Vec::with_capacity(doc.actors.len());
    let mut dependencies = Vec::with_capacity(doc.actors.len());
    let mut operations = doc.actors.len() + doc.provider_rows.len() + 2;
    let mut captured_bytes =
        closing.payload.len() + claim.inserted_row().payload.len() + doc.actors.len() * 4096;
    let mut core_records = Vec::new();
    for namespace in &doc.provider_namespaces {
        let count = state
            .provider_state
            .keys()
            .filter(|(n, _)| n == namespace)
            .count();
        operations = operations
            .checked_add(count)
            .ok_or(StoreError::CapacityExceeded)?;
        if operations > MAX_ROOT_OPERATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        for ((n, _), retained) in &state.provider_state {
            if n == namespace {
                captured_bytes = captured_bytes
                    .checked_add(retained.payload.len())
                    .ok_or(StoreError::CapacityExceeded)?;
                if captured_bytes > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES {
                    return Err(StoreError::CapacityExceeded);
                }
                dependencies.push(TerminalRowDependency::Exact(retained.clone()));
            }
        }
    }
    for (namespace, key) in &doc.provider_rows {
        match state.provider_state.get(&(namespace.clone(), key.clone())) {
            Some(retained) => {
                captured_bytes = captured_bytes
                    .checked_add(retained.payload.len())
                    .ok_or(StoreError::CapacityExceeded)?;
                if captured_bytes > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES {
                    return Err(StoreError::CapacityExceeded);
                }
                dependencies.push(TerminalRowDependency::Exact(retained.clone()));
            }
            None => dependencies.push(TerminalRowDependency::Absent {
                namespace: namespace.clone(),
                key: key.clone(),
            }),
        }
    }
    for identity in &doc.actors {
        let execution_id = ExecutionId::new(&identity.execution, InvocationLimits::default())
            .map_err(|_| StoreError::IncompatibleVersion)?;
        let execution = state
            .executions
            .get(&execution_id)
            .ok_or(StoreError::NotFound)?;
        let count = state
            .effects
            .values()
            .filter(|e| e.execution_id == execution_id)
            .count();
        let events = state
            .events
            .get(&execution_id)
            .ok_or(StoreError::NotFound)?;
        let outbox_count = state
            .outbox
            .values()
            .filter(|o| o.execution_id == execution_id)
            .count();
        operations = operations
            .checked_add(count * 2 + events.len() + outbox_count + 1)
            .ok_or(StoreError::CapacityExceeded)?;
        if operations > MAX_ROOT_OPERATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        // This first synchronous compiled profile has no scheduled work or
        // checkpoint transfer. Presence protects the root, never deletes it.
        if state.work.values().any(|w| w.execution_id == execution_id)
            || state.checkpoints.contains_key(&execution_id)
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut effects = Vec::with_capacity(count);
        let outbox_bytes = state
            .outbox
            .values()
            .filter(|o| o.execution_id == execution_id)
            .try_fold(0_usize, |n, o| {
                n.checked_add(o.payload.len().checked_mul(2)?.checked_add(2048)?)
            })
            .ok_or(StoreError::CapacityExceeded)?;
        if outbox_bytes
            > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES.saturating_sub(captured_bytes)
        {
            return Err(StoreError::CapacityExceeded);
        }
        let mut push_core = |namespace: String,
                             key: String,
                             version: u64,
                             payload: Vec<u8>|
         -> Result<(), StoreError> {
            captured_bytes = captured_bytes
                .checked_add(payload.len())
                .and_then(|n| n.checked_add(namespace.len() + key.len()))
                .ok_or(StoreError::CapacityExceeded)?;
            if payload.len() > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES
                || captured_bytes > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES
            {
                return Err(StoreError::CapacityExceeded);
            }
            core_records.push(ProviderStateRecord {
                namespace,
                key,
                version,
                payload,
            });
            Ok(())
        };
        push_core(
            "durable-execution".into(),
            execution_id.as_str().into(),
            execution.version,
            crate::durable::encode_execution(execution)?,
        )?;
        for event in events {
            push_core(
                format!("durable-event:{execution_id}"),
                format!("{:020}", event.sequence),
                1,
                crate::durable::encode_event(event)?,
            )?;
        }
        for effect in state
            .effects
            .values()
            .filter(|e| e.execution_id == execution_id)
        {
            push_core(
                "durable-effect".into(),
                effect.key.as_str().into(),
                1,
                crate::durable::encode_effect(effect)?,
            )?;
            effects.push(effect.clone());
        }
        for notification in state
            .outbox
            .values()
            .filter(|o| o.execution_id == execution_id)
        {
            push_core(
                "durable-outbox".into(),
                notification.notification_id.clone(),
                notification.version,
                crate::durable::encode_outbox(notification)?,
            )?;
        }
        let last_event = state
            .events
            .get(&execution_id)
            .and_then(|events| events.last())
            .ok_or(StoreError::NotFound)?;
        let parent = identity
            .parent
            .as_ref()
            .map(|p| {
                ExecutionId::new(p, InvocationLimits::default())
                    .map_err(|_| StoreError::IncompatibleVersion)
            })
            .transpose()?;
        actors.push(RootActorSnapshot {
            execution: execution.clone(),
            parent,
            call: identity.call.clone(),
            effects,
            checkpoint: None,
            work: Vec::new(),
            last_event: last_event.clone(),
        });
        if let Some(binding) = &identity.call {
            if row(state, &binding.catalog.namespace, &binding.catalog.key)? != &binding.catalog {
                return Err(StoreError::Conflict);
            }
            let call = row(state, &binding.namespace, &binding.key)?;
            captured_bytes = captured_bytes
                .checked_add(call.payload.len())
                .ok_or(StoreError::CapacityExceeded)?;
            if captured_bytes > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES {
                return Err(StoreError::CapacityExceeded);
            }
            dependencies.push(TerminalRowDependency::Exact(call.clone()));
        }
    }
    let snapshot = RootClosureSnapshot {
        claim: claim.clone(),
        closing,
        actors,
        provider_dependencies: dependencies,
        core_records,
        provider_epoch: state.provider_epoch,
        observed_tick,
    };
    snapshot.validate_bounds()?;
    Ok(snapshot)
}

fn preflight_capture(
    state: &State,
    doc: &Document,
    claim: &RootDriverClaim,
    closing: &ProviderStateRecord,
) -> Result<(), StoreError> {
    let mut operations = 2_usize;
    let mut bytes = closing
        .payload
        .len()
        .checked_add(claim.inserted_row().payload.len())
        .ok_or(StoreError::CapacityExceeded)?;
    let mut charge = |count: usize, size: usize| -> Result<(), StoreError> {
        operations = operations
            .checked_add(count)
            .ok_or(StoreError::CapacityExceeded)?;
        bytes = bytes
            .checked_add(size)
            .ok_or(StoreError::CapacityExceeded)?;
        if operations > MAX_ROOT_OPERATIONS
            || bytes > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES
        {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(())
    };
    for namespace in &doc.provider_namespaces {
        for ((n, k), row) in &state.provider_state {
            if n == namespace {
                charge(1, row.payload.len() + n.len() + k.len())?;
            }
        }
    }
    for (namespace, key) in &doc.provider_rows {
        charge(
            1,
            state
                .provider_state
                .get(&(namespace.clone(), key.clone()))
                .map_or(0, |row| row.payload.len())
                + namespace.len()
                + key.len(),
        )?;
    }
    for actor in &doc.actors {
        // Execution/event/effect codecs contain only bounded scalar identities.
        // Outbox JSON can expand each original byte to six bytes.
        charge(2, 8192)?;
        if let Some(call) = &actor.call {
            charge(
                3,
                call.catalog.payload.len()
                    + call.catalog.namespace.len()
                    + call.catalog.key.len()
                    + call.namespace.len()
                    + call.key.len()
                    + row(state, &call.namespace, &call.key)?.payload.len(),
            )?;
        }
        for effect in state
            .effects
            .values()
            .filter(|effect| effect.execution_id.as_str() == actor.execution)
        {
            // Each effect is held as both typed metadata and original codec bytes.
            let _ = effect;
            charge(2, 8192)?;
        }
        if let Some(events) = state
            .events
            .iter()
            .find_map(|(id, events)| (id.as_str() == actor.execution).then_some(events))
        {
            charge(
                events.len(),
                events
                    .len()
                    .checked_mul(4096)
                    .ok_or(StoreError::CapacityExceeded)?,
            )?;
        }
        for notification in state
            .outbox
            .values()
            .filter(|n| n.execution_id.as_str() == actor.execution)
        {
            charge(
                1,
                notification
                    .payload
                    .len()
                    .checked_mul(6)
                    .and_then(|n| n.checked_add(4096))
                    .ok_or(StoreError::CapacityExceeded)?,
            )?;
        }
    }
    Ok(())
}
fn assert_dependency(state: &State, dependency: &TerminalRowDependency) -> Result<(), StoreError> {
    match dependency {
        TerminalRowDependency::Exact(expected)
            if row(state, &expected.namespace, &expected.key)? == expected =>
        {
            Ok(())
        }
        TerminalRowDependency::Absent { namespace, key }
            if !state
                .provider_state
                .contains_key(&(namespace.clone(), key.clone())) =>
        {
            Ok(())
        }
        _ => Err(StoreError::Conflict),
    }
}

impl MemoryStore {
    pub(super) fn root_fence(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        claim.admission().validate()?;
        let mut state = self.lock()?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            claim.admission().execution.execution_id.as_str(),
        )?
        .clone();
        let mut doc = Document::read(&current)?;
        doc.require_claim(claim)?;
        if tick == 0
            || tick > i64::MAX as u64
            || tick < state.logical_tick
            || execution.execution_id.as_str() != doc.root
            || execution.state.terminal()
            || state.executions.get(&execution.execution_id) != Some(execution)
            || doc.phase == Phase::Terminal
        {
            return Err(StoreError::Conflict);
        }
        if doc.phase == Phase::Uncertain {
            return Ok(current);
        }
        doc.phase = Phase::Uncertain;
        let next = doc.row(
            current.version.checked_add(1).ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        journal::journaled(&mut state, |state, journal| {
            write(
                state,
                journal,
                next.clone(),
                Some(current.version),
                self.limits,
            )?;
            journal.touch_logical_tick(state);
            state.logical_tick = tick;
            Ok(next)
        })
    }
    pub(super) fn root_register_row(
        &self,
        admission: RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            admission.claim.admission().execution.execution_id.as_str(),
        )?
        .clone();
        let mut doc = Document::read(&current)?;
        doc.require_live(admission.observed_tick, state.logical_tick)?;
        if state.executions.get(&admission.execution.execution_id) != Some(&admission.execution) {
            return Err(StoreError::Conflict);
        }
        let intent = state
            .effects
            .get(&admission.effect_key)
            .ok_or(StoreError::NotFound)?;
        let was_registered = doc
            .provider_rows
            .iter()
            .any(|(n, k)| n == &admission.identity.namespace && k == &admission.identity.key);
        doc.register_row(&admission, intent)?;
        let index_key = row_scope_key(&admission.identity.namespace, &admission.identity.key);
        if let Some(binding) = state
            .provider_state
            .get(&(ROW_SCOPE_NAMESPACE.into(), index_key.clone()))
        {
            return if was_registered
                && binding.version == 1
                && binding.payload == doc.root.as_bytes()
            {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        let next = doc.row(
            current.version.checked_add(1).ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        journal::journaled(&mut state, |state, journal| {
            write(state, journal, next, Some(current.version), self.limits)?;
            write(
                state,
                journal,
                index(ROW_SCOPE_NAMESPACE, &index_key, &doc.root),
                None,
                self.limits,
            )
        })
    }
    pub(super) fn root_admit(
        &self,
        admission: RootDriverAdmission,
    ) -> Result<RootDriverClaim, StoreError> {
        let doc = Document::new(&admission)?;
        let root_row = doc.row(1, self.limits.max_blob_bytes)?;
        let claim = admission.observe_inserted(&root_row)?;
        let mut state = self.lock()?;
        doc.require_live(admission.event.tick, state.logical_tick)?;
        journal::journaled(&mut state, |state, journal| {
            if state
                .executions
                .values()
                .any(|e| e.run_unit_id == admission.execution.run_unit_id)
                || state
                    .effects
                    .values()
                    .any(|e| e.run_unit_id == admission.execution.run_unit_id)
                || state
                    .checkpoints
                    .contains_key(&admission.execution.execution_id)
                || state
                    .work
                    .values()
                    .any(|w| w.execution_id == admission.execution.execution_id)
            {
                return Err(StoreError::Conflict);
            }
            write(
                state,
                journal,
                index(RUN_NAMESPACE, &doc.run, &doc.root),
                None,
                self.limits,
            )?;
            write(
                state,
                journal,
                index(ACTOR_NAMESPACE, &doc.root, &doc.root),
                None,
                self.limits,
            )?;
            write(state, journal, root_row, None, self.limits)?;
            for namespace in &doc.provider_namespaces {
                write(
                    state,
                    journal,
                    index(SCOPE_NAMESPACE, namespace, &doc.root),
                    None,
                    self.limits,
                )?;
            }
            for (namespace, key) in &doc.provider_rows {
                write(
                    state,
                    journal,
                    index(
                        ROW_SCOPE_NAMESPACE,
                        &row_scope_key(namespace, key),
                        &doc.root,
                    ),
                    None,
                    self.limits,
                )?;
            }
            admit(
                state,
                journal,
                admission.execution,
                admission.event,
                admission.notification,
                self.limits,
            )?;
            journal.touch_logical_tick(state);
            state.logical_tick = claim.admission().event.tick;
            Ok(claim)
        })
    }
    pub(super) fn root_admit_child(&self, admission: RootChildAdmission) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            admission.claim.admission().execution.execution_id.as_str(),
        )?
        .clone();
        let mut doc = Document::read(&current)?;
        doc.require_live(admission.event.tick, state.logical_tick)?;
        if state.executions.get(&admission.parent) != Some(&admission.parent_occurrence.execution) {
            return Err(StoreError::Conflict);
        }
        let intent = state
            .effects
            .get(&admission.parent_occurrence.effect_key)
            .ok_or(StoreError::NotFound)?;
        doc.enroll(&admission, intent)?;
        if row(&state, &admission.call.namespace, &admission.call.key)? != &admission.call {
            return Err(StoreError::Conflict);
        }
        if row(&state, &admission.catalog.namespace, &admission.catalog.key)? != &admission.catalog
        {
            return Err(StoreError::Conflict);
        }
        if state
            .work
            .values()
            .any(|w| w.execution_id == admission.execution.execution_id)
            || state
                .checkpoints
                .contains_key(&admission.execution.execution_id)
        {
            return Err(StoreError::InvalidTransition);
        }
        let next = doc.row(
            current.version.checked_add(1).ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        journal::journaled(&mut state, |state, journal| {
            write(state, journal, next, Some(current.version), self.limits)?;
            write(
                state,
                journal,
                index(
                    ACTOR_NAMESPACE,
                    admission.execution.execution_id.as_str(),
                    &doc.root,
                ),
                None,
                self.limits,
            )?;
            let tick = admission.event.tick;
            admit(
                state,
                journal,
                admission.execution,
                admission.event,
                admission.notification,
                self.limits,
            )?;
            journal.touch_logical_tick(state);
            state.logical_tick = tick;
            Ok(())
        })
    }
    pub(super) fn root_close(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        observed_tick: u64,
    ) -> Result<RootClosureSnapshot, StoreError> {
        let mut state = self.lock()?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            claim.admission().execution.execution_id.as_str(),
        )?
        .clone();
        let mut doc = Document::read(&current)?;
        doc.require_claim(claim)?;
        doc.require_live(observed_tick, state.logical_tick)?;
        if doc.phase != Phase::Open
            || state.executions.get(&execution.execution_id) != Some(execution)
            || execution.execution_id.as_str() != doc.root
            || execution.state != ExecutionState::Running
        {
            return Err(StoreError::Conflict);
        }
        doc.phase = Phase::Closing;
        let closing = doc.row(
            current.version.checked_add(1).ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        let provisional = capture(&state, claim, closing.clone(), observed_tick)?;
        crate::root_terminal::validate_known_closure(&provisional)?;
        journal::journaled(&mut state, |state, journal| {
            write(state, journal, closing, Some(current.version), self.limits)?;
            journal.touch_logical_tick(state);
            state.logical_tick = observed_tick;
            capture(
                state,
                claim,
                row(state, ROOT_DRIVER_NAMESPACE, &doc.root)?.clone(),
                observed_tick,
            )
        })
    }
    pub(super) fn root_commit(
        &self,
        request: RootTerminalPublication,
    ) -> Result<RootTerminalCommit, StoreError> {
        let mut doc =
            crate::root_terminal::validate_publication(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        doc.require_live(request.observed_tick, state.logical_tick)?;
        if row(&state, ROOT_DRIVER_NAMESPACE, &doc.root)? != &request.closure.closing
            || state.provider_epoch != request.closure.provider_epoch
            || capture(
                &state,
                &request.closure.claim,
                request.closure.closing.clone(),
                request.closure.observed_tick,
            )? != request.closure
        {
            return Err(StoreError::Conflict);
        }
        for dependency in &request.dependencies {
            assert_dependency(&state, dependency)?;
        }
        for dependency in &request.closure.provider_dependencies {
            assert_dependency(&state, dependency)?;
        }
        doc.phase = Phase::Terminal;
        doc.winning_resource = Some(request.audits[0].resource.value);
        let winner = doc.row(
            request
                .closure
                .closing
                .version
                .checked_add(1)
                .ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        journal::journaled(&mut state, |state, journal| {
            Self::apply_mutations_locked(state, journal, request.mutations, self.limits)?;
            let id = &request.closure.actors[0].execution.execution_id;
            let mut execution = state
                .executions
                .get(id)
                .cloned()
                .ok_or(StoreError::NotFound)?;
            for step in request.steps {
                validation::execution_step(id, &execution, &step.event, &step.notification)?;
                if !execution.state.can_transition_to(step.next_state)
                    || step.event.sequence != execution.version + 1
                    || step.event.tick != request.observed_tick
                {
                    return Err(StoreError::InvalidTransition);
                }
                execution.state = step.next_state;
                execution.version = execution
                    .version
                    .checked_add(1)
                    .ok_or(StoreError::Conflict)?;
                execution.terminal_tick = step.next_state.terminal().then_some(step.event.tick);
                journal.touch_execution(state, id);
                state.executions.insert(id.clone(), execution.clone());
                journal.touch_events(state, id);
                journal.touch_provider_epoch(state);
                Self::append_event_locked(state, step.event, self.limits)?;
                journal.touch_outbox(state, &step.notification.notification_id);
                journal.touch_blob_bytes(state);
                journal.touch_provider_epoch(state);
                Self::append_outbox_locked(state, step.notification, self.limits)?;
            }
            for audit in request.audits {
                if state.audits.len() + terminal_audit_count(state) >= self.limits.max_audits {
                    return Err(StoreError::CapacityExceeded);
                }
                let payload = crate::durable::root_terminal::encode_terminal_audit(&audit)?;
                journal.touch_next_audit_ordinal(state);
                state.next_audit_ordinal = state
                    .next_audit_ordinal
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?;
                let key = crate::durable::audit_storage_key(
                    &audit.execution_id,
                    &format!("memory:{:020}", state.next_audit_ordinal),
                );
                write(
                    state,
                    journal,
                    ProviderStateRecord {
                        namespace: AUDIT_NAMESPACE.into(),
                        key,
                        version: 1,
                        payload,
                    },
                    None,
                    self.limits,
                )?;
            }
            write(
                state,
                journal,
                winner.clone(),
                Some(request.closure.closing.version),
                self.limits,
            )?;
            journal.touch_logical_tick(state);
            state.logical_tick = request.observed_tick;
            Ok(RootTerminalCommit {
                claim: request.closure.claim,
                execution,
                winner,
            })
        })
    }
    pub(super) fn root_audit_subjects(
        &self,
        execution: &ExecutionId,
        max: usize,
    ) -> Result<Vec<AuditSubjectRecord>, StoreError> {
        if max == 0 || max > self.limits.max_audits {
            return Err(StoreError::CapacityExceeded);
        }
        let state = self.lock()?;
        let prefix = crate::durable::audit_storage_key(execution, "");
        let mut rows = BTreeMap::new();
        for (key, audit) in state
            .audits
            .iter()
            .filter(|(_, a)| &a.execution_id == execution)
        {
            rows.insert(key.clone(), AuditSubjectRecord::Effect(audit.clone()));
        }
        for ((namespace, key), value) in &state.provider_state {
            if namespace == AUDIT_NAMESPACE && key.starts_with(&prefix) {
                rows.insert(
                    key.clone(),
                    AuditSubjectRecord::RootTerminal(
                        crate::durable::root_terminal::decode_terminal_audit(&value.payload)?,
                    ),
                );
            }
        }
        if rows.len() > max {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(rows.into_values().collect())
    }
}
pub(super) fn terminal_audit_count(state: &State) -> usize {
    state
        .provider_state
        .keys()
        .filter(|(namespace, _)| namespace == AUDIT_NAMESPACE)
        .count()
}
