use super::codec::{
    CATALOG_KEY, CATALOG_NAMESPACE, CONVERSATION_NAMESPACE, MESSAGE_NAMESPACE, OUTBOUND_NAMESPACE,
    REPLAY_NAMESPACE, SESSION_NAMESPACE, delete, list, mutate, put, read, store_error,
};
use super::contracts::{
    TmCall, TmConversationAction, TmDefinitionSet, TmDestination, TmInputMessage, TmLimits, TmPcb,
    TmPcbStatus, TmTransactionDefinition,
};
use super::model::{
    CatalogRow, ConversationRow, ConversationStart, MessageRow, OutboundRow, OutputBuffer,
    ReplayResult, ReplayRow, SessionRow, TmCallResult, TmCancelReceipt, TmConversationView,
    TmEnqueueReceipt, TmInstallReceipt, TmMessageState, TmOutboundMessage, TmPcbView,
    TmScheduleReceipt, WorkDisposition, WorkPayload,
};
use super::support::{
    call_replay, cancel_replay, digest, enqueue_replay, find_transaction, hex_digest,
    invocation_digest, output_route, output_row, replay_work, schedule_replay, work_error,
    work_generation,
};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::{AccessIntent, EnterpriseAuthorizer, HostProblem};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateStore, WorkRecord, WorkStore};
use std::collections::BTreeMap;
use std::sync::Arc;
mod conversations;

pub(super) const WORK_PAYLOAD_SCHEMA: &str = "mainframe-env.ims-tm-work@1";

pub struct TmService {
    pub(super) store: Arc<dyn ProviderStateStore>,
    pub(super) work_store: Arc<dyn WorkStore>,
    pub(super) authorizer: Arc<dyn EnterpriseAuthorizer>,
    pub(super) limits: TmLimits,
}

impl TmService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        work_store: Arc<dyn WorkStore>,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        limits: TmLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let service = Arc::new(Self {
            store,
            work_store,
            authorizer,
            limits,
        });
        service.validate_store()?;
        Ok(service)
    }

    pub fn install(&self, definitions: TmDefinitionSet) -> Result<TmInstallReceipt, HostProblem> {
        definitions.validate(self.limits)?;
        let identity = hex_digest(&digest("mainframe-env.ims-tm-definitions@1", &definitions)?);
        if let Some((_, current)) = self.catalog()? {
            if current.definitions != definitions {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(TmInstallReceipt {
                transactions: definitions.transactions.len(),
                identity,
                replayed: true,
            });
        }
        let row = CatalogRow {
            definitions: definitions.clone(),
            next_sequence: 1,
            package_binding: None,
            application: None,
            active: true,
        };
        mutate(
            self.store.as_ref(),
            vec![put(
                CATALOG_NAMESPACE,
                CATALOG_KEY,
                &row,
                None,
                self.limits.max_state_bytes,
            )?],
        )?;
        Ok(TmInstallReceipt {
            transactions: definitions.transactions.len(),
            identity,
            replayed: false,
        })
    }

    pub fn enqueue(
        &self,
        invocation: &Invocation,
        mut message: TmInputMessage,
    ) -> Result<TmEnqueueReceipt, HostProblem> {
        let now = self.check_invocation(invocation)?;
        message.validate(self.limits)?;
        let (catalog_version, mut catalog) = self.catalog()?.ok_or(HostProblem::NotFound)?;
        if !catalog.active {
            return Err(HostProblem::NotFound);
        }
        let request_digest = invocation_digest(
            "mainframe-env.ims-tm-enqueue@1",
            invocation,
            &(catalog.package_binding.as_deref(), &message),
        )?;
        if let Some((_, replay)) = read::<ReplayRow>(
            self.store.as_ref(),
            REPLAY_NAMESPACE,
            invocation.idempotency_key.as_str(),
            self.limits.max_state_bytes,
        )? && replay.request_digest != request_digest
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let transaction = find_transaction(&catalog, &message.transaction)?.clone();
        self.authorize_transaction(invocation, &transaction, AccessIntent::Update)?;
        if let Some(replay) = self.replay(invocation, request_digest)? {
            return enqueue_replay(replay);
        }
        if let Some((_, retained)) = self.message(&message.message_id)? {
            if retained.request_digest != request_digest
                || retained.principal != invocation.principal.id().as_str()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            self.ensure_work(&retained, &transaction)?;
            return self.finish_admission(invocation, request_digest, retained);
        }
        if list::<MessageRow>(
            self.store.as_ref(),
            MESSAGE_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )?
        .len()
            >= self.limits.max_queued_messages
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let (conversation_id, new_conversation) = self.resolve_conversation(
            invocation,
            &transaction,
            message.conversation_id.as_deref(),
            request_digest,
            catalog.package_binding.as_deref(),
        )?;
        message.conversation_id = conversation_id.clone();
        let sequence = catalog.next_sequence;
        catalog.next_sequence = sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let deadline_tick = invocation.deadline_tick.min(
            now.checked_add(transaction.timeout_ticks)
                .ok_or(HostProblem::ResourceExhausted)?,
        );
        if deadline_tick <= now {
            return Err(HostProblem::TimedOut);
        }
        let row = MessageRow {
            work_id: format!("ims-tm:{sequence:020}:{}", message.message_id),
            message,
            sequence,
            enqueue_tick: now,
            deadline_tick,
            principal: invocation.principal.id().as_str().into(),
            state: TmMessageState::AdmissionPending,
            request_digest,
            new_conversation,
            run_unit: None,
            package_binding: catalog.package_binding.clone(),
        };
        mutate(
            self.store.as_ref(),
            vec![
                put(
                    CATALOG_NAMESPACE,
                    CATALOG_KEY,
                    &catalog,
                    Some(catalog_version),
                    self.limits.max_state_bytes,
                )?,
                put(
                    MESSAGE_NAMESPACE,
                    &row.message.message_id,
                    &row,
                    None,
                    self.limits.max_state_bytes,
                )?,
            ],
        )?;
        self.ensure_work(&row, &transaction)?;
        self.finish_admission(invocation, request_digest, row)
    }

    pub fn repair_schedules(&self, max: usize) -> Result<usize, HostProblem> {
        if max == 0 || max > self.limits.max_queued_messages {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut repaired = 0;
        for (_, version, mut message) in list::<MessageRow>(
            self.store.as_ref(),
            MESSAGE_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )? {
            if repaired == max || message.state != TmMessageState::AdmissionPending {
                continue;
            }
            let catalog = self.catalog_for_binding(message.package_binding.as_deref())?;
            let transaction = find_transaction(&catalog, &message.message.transaction)?;
            self.ensure_work(&message, transaction)?;
            message.state = TmMessageState::Scheduled;
            mutate(
                self.store.as_ref(),
                vec![put(
                    MESSAGE_NAMESPACE,
                    &message.message.message_id,
                    &message,
                    Some(version),
                    self.limits.max_state_bytes,
                )?],
            )?;
            repaired += 1;
        }
        Ok(repaired)
    }

    pub fn claim(
        &self,
        transaction: &str,
        worker: &str,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, HostProblem> {
        let (_, catalog) = self.catalog()?.ok_or(HostProblem::NotFound)?;
        let transaction = find_transaction(&catalog, transaction)?;
        let now = self
            .store
            .advance_logical_clock(now_tick)
            .map_err(store_error)?;
        self.work_store
            .claim(
                worker,
                Some(&work_generation(
                    transaction,
                    catalog.package_binding.as_deref(),
                )?),
                now,
                lease_ticks,
            )
            .map_err(work_error)
    }

    pub fn start(
        &self,
        invocation: &Invocation,
        work: &WorkRecord,
    ) -> Result<TmScheduleReceipt, HostProblem> {
        let now = self.check_invocation(invocation)?;
        let current_work = self.claimed_work(work, now)?;
        let payload: WorkPayload = serde_json::from_slice(&current_work.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if payload.schema_version != WORK_PAYLOAD_SCHEMA {
            return Err(HostProblem::InfrastructureFailure);
        }
        let (message_version, mut message) = self
            .message(&payload.message_id)?
            .ok_or(HostProblem::NotFound)?;
        if message.state == TmMessageState::Cancelled || current_work.cancellation_requested {
            return Err(HostProblem::Cancelled);
        }
        if message.deadline_tick <= now {
            return Err(HostProblem::TimedOut);
        }
        if payload.transaction != message.message.transaction {
            return Err(HostProblem::IdempotencyConflict);
        }
        if message.principal != invocation.principal.id().as_str() {
            return Err(HostProblem::Unauthorized);
        }
        let catalog = self.catalog_for_binding(message.package_binding.as_deref())?;
        let transaction = find_transaction(&catalog, &message.message.transaction)?.clone();
        if message.work_id != current_work.work_id
            || current_work.required_generation
                != work_generation(&transaction, message.package_binding.as_deref())?
            || current_work.required_selector.as_str() != transaction.program_selector
            || current_work.artifact.as_str() != transaction.artifact
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.authorize_transaction(invocation, &transaction, AccessIntent::Execute)?;
        let request_digest = invocation_digest(
            "mainframe-env.ims-tm-schedule@1",
            invocation,
            &(message.package_binding.as_deref(), &payload),
        )?;
        if let Some(replay) = self.replay(invocation, request_digest)? {
            return schedule_replay(replay);
        }
        if message.state != TmMessageState::Scheduled {
            return Err(HostProblem::IdempotencyConflict);
        }
        if self.session(invocation.run_unit_id.as_str())?.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let conversation = self.start_conversation(invocation, &transaction, &message)?;
        let spa = conversation
            .as_ref()
            .map(|start| start.spa.clone())
            .unwrap_or_default();
        let alternate_destinations = transaction
            .alternate_pcbs
            .iter()
            .map(|pcb| {
                (
                    pcb.name.clone(),
                    match &pcb.destination {
                        TmDestination::Fixed(value) => Some(value.clone()),
                        TmDestination::Modifiable => None,
                    },
                )
            })
            .collect();
        let session = SessionRow {
            run_unit: invocation.run_unit_id.as_str().into(),
            principal: message.principal.clone(),
            message_id: message.message.message_id.clone(),
            transaction: transaction.code.clone(),
            io_destination: message.message.source.clone(),
            message_sequence: message.sequence,
            user_id: message.message.user_id.clone(),
            group_name: message.message.group_name.clone(),
            input_index: None,
            io_status: TmPcbStatus::SUCCESS,
            alternate_destinations,
            output_buffers: BTreeMap::new(),
            pending_output_ids: Vec::new(),
            conversation_id: message.message.conversation_id.clone(),
            work_id: current_work.work_id.clone(),
            lease_id: current_work
                .lease_id
                .clone()
                .ok_or(HostProblem::IdempotencyConflict)?,
            lease_epoch: current_work.lease_epoch,
            package_binding: message.package_binding.clone(),
        };
        message.state = TmMessageState::InFlight;
        message.run_unit = Some(session.run_unit.clone());
        let receipt = TmScheduleReceipt {
            message_id: message.message.message_id.clone(),
            transaction: transaction.code,
            source: message.message.source.clone(),
            conversation_id: message.message.conversation_id.clone(),
            spa,
            replayed: false,
        };
        let mut mutations = vec![
            put(
                MESSAGE_NAMESPACE,
                &message.message.message_id,
                &message,
                Some(message_version),
                self.limits.max_state_bytes,
            )?,
            put(
                SESSION_NAMESPACE,
                &session.run_unit,
                &session,
                None,
                self.limits.max_state_bytes,
            )?,
            self.replay_put(
                invocation,
                request_digest,
                ReplayResult::Schedule(receipt.clone()),
                None,
            )?,
        ];
        if let Some(conversation) = conversation {
            mutations.push(put(
                CONVERSATION_NAMESPACE,
                session
                    .conversation_id
                    .as_deref()
                    .ok_or(HostProblem::InfrastructureFailure)?,
                &conversation.row,
                conversation.current_version,
                self.limits.max_state_bytes,
            )?);
        }
        mutate(self.store.as_ref(), mutations)?;
        Ok(receipt)
    }

    pub fn call(&self, invocation: &Invocation, call: TmCall) -> Result<TmCallResult, HostProblem> {
        let now = self.check_invocation(invocation)?;
        let session = self.session(invocation.run_unit_id.as_str())?;
        if session.is_none() {
            if let Some((_, candidate)) = read::<ReplayRow>(
                self.store.as_ref(),
                REPLAY_NAMESPACE,
                invocation.idempotency_key.as_str(),
                self.limits.max_state_bytes,
            )? && let Some(work) = candidate.work.as_ref()
            {
                let retained = self
                    .work_store
                    .get_work(&work.work_id)
                    .map_err(work_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                let payload: WorkPayload = serde_json::from_slice(&retained.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if payload.schema_version != WORK_PAYLOAD_SCHEMA {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let (_, message) = self
                    .message(&payload.message_id)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                let digest = invocation_digest(
                    "mainframe-env.ims-tm-call@1",
                    invocation,
                    &(message.package_binding.as_deref(), &call),
                )?;
                let replay = self
                    .replay(invocation, digest)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                if message.principal != invocation.principal.id().as_str()
                    || message.work_id != work.work_id
                {
                    return Err(HostProblem::Unauthorized);
                }
                let catalog = self.catalog_for_binding(message.package_binding.as_deref())?;
                let transaction = find_transaction(&catalog, &message.message.transaction)?;
                call.validate(transaction.context, transaction, self.limits)?;
                self.authorize_transaction(invocation, transaction, AccessIntent::Update)?;
                self.apply_replay_work(replay.work.as_ref(), now)?;
                return call_replay(replay);
            }
            return Err(HostProblem::NotFound);
        }
        let (session_version, mut session) = session.ok_or(HostProblem::NotFound)?;
        if session.principal != invocation.principal.id().as_str() {
            return Err(HostProblem::Unauthorized);
        }
        let current_work = self
            .work_store
            .get_work(&session.work_id)
            .map_err(work_error)?
            .ok_or(HostProblem::NotFound)?;
        if current_work.cancellation_requested {
            return Err(HostProblem::Cancelled);
        }
        if current_work.deadline_tick <= now {
            return Err(HostProblem::TimedOut);
        }
        let catalog = self.catalog_for_binding(session.package_binding.as_deref())?;
        let transaction = find_transaction(&catalog, &session.transaction)?.clone();
        call.validate(transaction.context, &transaction, self.limits)?;
        let intent = if matches!(call, TmCall::GetUnique | TmCall::GetNext) {
            AccessIntent::Read
        } else {
            AccessIntent::Update
        };
        self.authorize_transaction(invocation, &transaction, intent)?;
        let request_digest = invocation_digest(
            "mainframe-env.ims-tm-call@1",
            invocation,
            &(session.package_binding.as_deref(), &call),
        )?;
        if let Some(replay) = self.replay(invocation, request_digest)? {
            self.apply_replay_work(replay.work.as_ref(), now)?;
            return call_replay(replay);
        }
        match call {
            TmCall::GetUnique => {
                self.get_segment(invocation, request_digest, session_version, session, true)
            }
            TmCall::GetNext => {
                self.get_segment(invocation, request_digest, session_version, session, false)
            }
            TmCall::Insert { pcb, segment } => self.insert_segment(
                invocation,
                request_digest,
                session_version,
                &transaction,
                session,
                pcb,
                segment,
            ),
            TmCall::Change { pcb, destination } => {
                self.authorize_destination(invocation, &destination)?;
                if session.output_buffers.contains_key(&pcb) {
                    return self.persist_session_result(
                        invocation,
                        request_digest,
                        session_version,
                        session,
                        TmCallResult::status(TmPcbStatus::INVALID_CALL),
                    );
                }
                session
                    .alternate_destinations
                    .insert(pcb.clone(), Some(destination.clone()));
                let mut result = TmCallResult::status(TmPcbStatus::SUCCESS);
                result.destination = Some(destination.clone());
                result.pcb = Some(TmPcbView::Alternate {
                    name: pcb,
                    destination: Some(destination),
                    status: TmPcbStatus::SUCCESS,
                });
                self.persist_session_result(
                    invocation,
                    request_digest,
                    session_version,
                    session,
                    result,
                )
            }
            TmCall::Purge { pcb } => self.purge(
                invocation,
                request_digest,
                session_version,
                &transaction,
                session,
                pcb,
            ),
            TmCall::Commit { conversation } => self.commit(
                invocation,
                request_digest,
                now,
                session_version,
                &catalog,
                &transaction,
                session,
                conversation,
            ),
            TmCall::Rollback => {
                self.rollback(invocation, request_digest, now, session_version, session)
            }
            TmCall::Terminate => self.commit(
                invocation,
                request_digest,
                now,
                session_version,
                &catalog,
                &transaction,
                session,
                transaction
                    .conversational
                    .then_some(TmConversationAction::End),
            ),
        }
    }

    pub fn cancel(
        &self,
        invocation: &Invocation,
        message_id: &str,
    ) -> Result<TmCancelReceipt, HostProblem> {
        self.check_invocation(invocation)?;
        let (version, mut message) = self.message(message_id)?.ok_or(HostProblem::NotFound)?;
        let catalog = self.catalog_for_binding(message.package_binding.as_deref())?;
        let transaction = find_transaction(&catalog, &message.message.transaction)?;
        self.authorize_transaction(invocation, transaction, AccessIntent::Control)?;
        let request_digest = invocation_digest(
            "mainframe-env.ims-tm-cancel@1",
            invocation,
            &(message.package_binding.as_deref(), message_id),
        )?;
        if let Some(replay) = self.replay(invocation, request_digest)? {
            return cancel_replay(replay);
        }
        if message.state == TmMessageState::Completed {
            return Err(HostProblem::NotFound);
        }
        self.work_store
            .request_cancellation(&message.work_id)
            .map_err(work_error)?;
        message.state = TmMessageState::Cancelled;
        let mut mutations = Vec::new();
        if let Some(run_unit) = message.run_unit.take()
            && let Some((session_version, session)) = self.session(&run_unit)?
        {
            if session.message_id != message_id {
                return Err(HostProblem::InfrastructureFailure);
            }
            for pending in &session.pending_output_ids {
                let (pending_version, output) = read::<OutboundRow>(
                    self.store.as_ref(),
                    OUTBOUND_NAMESPACE,
                    pending,
                    self.limits.max_state_bytes,
                )?
                .ok_or(HostProblem::InfrastructureFailure)?;
                if !output.message.express {
                    mutations.push(delete(OUTBOUND_NAMESPACE, pending, pending_version));
                }
            }
            if message.new_conversation
                && let Some(conversation_id) = session.conversation_id.as_deref()
                && let Some((conversation_version, _)) = read::<ConversationRow>(
                    self.store.as_ref(),
                    CONVERSATION_NAMESPACE,
                    conversation_id,
                    self.limits.max_state_bytes,
                )?
            {
                mutations.push(delete(
                    CONVERSATION_NAMESPACE,
                    conversation_id,
                    conversation_version,
                ));
            }
            mutations.push(delete(SESSION_NAMESPACE, &run_unit, session_version));
        }
        let receipt = TmCancelReceipt {
            message_id: message_id.into(),
            state: TmMessageState::Cancelled,
            replayed: false,
        };
        mutations.extend([
            put(
                MESSAGE_NAMESPACE,
                message_id,
                &message,
                Some(version),
                self.limits.max_state_bytes,
            )?,
            self.replay_put(
                invocation,
                request_digest,
                ReplayResult::Cancel(receipt.clone()),
                None,
            )?,
        ]);
        mutate(self.store.as_ref(), mutations)?;
        Ok(receipt)
    }

    pub fn queued(&self, transaction: &str, max: usize) -> Result<Vec<String>, HostProblem> {
        if max == 0 || max > self.limits.max_queued_messages {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut rows = list::<MessageRow>(
            self.store.as_ref(),
            MESSAGE_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )?
        .into_iter()
        .map(|(_, _, value)| value)
        .filter(|row| {
            row.message.transaction == transaction
                && matches!(
                    row.state,
                    TmMessageState::AdmissionPending | TmMessageState::Scheduled
                )
        })
        .collect::<Vec<_>>();
        rows.sort_by_key(|row| (row.sequence, row.message.message_id.clone()));
        Ok(rows
            .into_iter()
            .take(max)
            .map(|row| row.message.message_id)
            .collect())
    }

    pub fn outbound(
        &self,
        destination: &str,
        max: usize,
    ) -> Result<Vec<TmOutboundMessage>, HostProblem> {
        if max == 0 || max > self.limits.max_outbound_messages {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut rows = list::<OutboundRow>(
            self.store.as_ref(),
            OUTBOUND_NAMESPACE,
            self.limits.max_outbound_messages,
            self.limits.max_state_bytes,
        )?
        .into_iter()
        .map(|(_, _, value)| value)
        .filter(|row| row.available && row.message.destination == destination)
        .map(|row| row.message)
        .collect::<Vec<_>>();
        rows.sort_by_key(|row| (row.sequence, row.message_id.clone()));
        rows.truncate(max);
        Ok(rows)
    }

    pub fn message_state(&self, message_id: &str) -> Result<Option<TmMessageState>, HostProblem> {
        Ok(self.message(message_id)?.map(|(_, row)| row.state))
    }

    pub fn conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<TmConversationView>, HostProblem> {
        Ok(read::<ConversationRow>(
            self.store.as_ref(),
            CONVERSATION_NAMESPACE,
            conversation_id,
            self.limits.max_state_bytes,
        )?
        .map(|(_, row)| row.view()))
    }

    fn finish_admission(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        mut message: MessageRow,
    ) -> Result<TmEnqueueReceipt, HostProblem> {
        let (version, current) = self
            .message(&message.message.message_id)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if current.request_digest != request_digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        message = current;
        message.state = TmMessageState::Scheduled;
        let receipt = TmEnqueueReceipt {
            message_id: message.message.message_id.clone(),
            sequence: message.sequence,
            work_id: message.work_id.clone(),
            conversation_id: message.message.conversation_id.clone(),
            replayed: false,
        };
        mutate(
            self.store.as_ref(),
            vec![
                put(
                    MESSAGE_NAMESPACE,
                    &message.message.message_id,
                    &message,
                    Some(version),
                    self.limits.max_state_bytes,
                )?,
                self.replay_put(
                    invocation,
                    request_digest,
                    ReplayResult::Enqueue(receipt.clone()),
                    None,
                )?,
            ],
        )?;
        Ok(receipt)
    }

    fn get_segment(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        session_version: u64,
        mut session: SessionRow,
        unique: bool,
    ) -> Result<TmCallResult, HostProblem> {
        let (_, message) = self
            .message(&session.message_id)?
            .ok_or(HostProblem::NotFound)?;
        let (status, segment, next) = if unique {
            if session.input_index.is_some() {
                (TmPcbStatus::NO_MORE_MESSAGES, None, session.input_index)
            } else {
                (
                    TmPcbStatus::SUCCESS,
                    message.message.segments.first().cloned(),
                    Some(1),
                )
            }
        } else {
            match session.input_index {
                None => (TmPcbStatus::INVALID_CALL, None, None),
                Some(index) if index < message.message.segments.len() => (
                    TmPcbStatus::SUCCESS,
                    Some(message.message.segments[index].clone()),
                    Some(index + 1),
                ),
                Some(index) => (TmPcbStatus::NO_MORE_SEGMENTS, None, Some(index)),
            }
        };
        session.input_index = next;
        session.io_status = status;
        let mut result = TmCallResult::status(status);
        result.segment = segment;
        result.conversation_id = session.conversation_id.clone();
        result.pcb = Some(TmPcbView::Io {
            logical_terminal: message.message.source,
            status,
            message_sequence: message.sequence,
            user_id: message.message.user_id,
            group_name: message.message.group_name,
        });
        self.persist_session_result(invocation, request_digest, session_version, session, result)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_segment(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        session_version: u64,
        transaction: &TmTransactionDefinition,
        mut session: SessionRow,
        pcb: TmPcb,
        segment: Vec<u8>,
    ) -> Result<TmCallResult, HostProblem> {
        let Some((key, destination, express)) = output_route(&session, transaction, &pcb) else {
            return self.persist_session_result(
                invocation,
                request_digest,
                session_version,
                session,
                TmCallResult::status(TmPcbStatus::INVALID_CALL),
            );
        };
        self.authorize_destination(invocation, &destination)?;
        if session
            .output_buffers
            .get(&key)
            .is_some_and(|buffer| buffer.segments.len() >= self.limits.max_segments_per_message)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let buffer = session.output_buffers.entry(key).or_insert(OutputBuffer {
            destination: destination.clone(),
            express,
            segments: Vec::new(),
        });
        if buffer.destination != destination {
            return Err(HostProblem::IdempotencyConflict);
        }
        buffer.segments.push(segment);
        let mut result = TmCallResult::status(TmPcbStatus::SUCCESS);
        result.destination = Some(destination.clone());
        result.pcb = Some(match pcb {
            TmPcb::Io => TmPcbView::Io {
                logical_terminal: session.io_destination.clone(),
                status: TmPcbStatus::SUCCESS,
                message_sequence: session.message_sequence,
                user_id: session.user_id.clone(),
                group_name: session.group_name.clone(),
            },
            TmPcb::Alternate(name) => TmPcbView::Alternate {
                name,
                destination: Some(destination),
                status: TmPcbStatus::SUCCESS,
            },
        });
        self.persist_session_result(invocation, request_digest, session_version, session, result)
    }

    fn purge(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        session_version: u64,
        transaction: &TmTransactionDefinition,
        mut session: SessionRow,
        pcb: TmPcb,
    ) -> Result<TmCallResult, HostProblem> {
        let Some((key, destination, _)) = output_route(&session, transaction, &pcb) else {
            return self.persist_session_result(
                invocation,
                request_digest,
                session_version,
                session,
                TmCallResult::status(TmPcbStatus::INVALID_CALL),
            );
        };
        self.authorize_destination(invocation, &destination)?;
        let Some(buffer) = session.output_buffers.remove(&key) else {
            return self.persist_session_result(
                invocation,
                request_digest,
                session_version,
                session,
                TmCallResult::status(TmPcbStatus::SUCCESS),
            );
        };
        self.ensure_outbound_capacity(1)?;
        let (output_key, output) =
            output_row(&session, &key, buffer, session.pending_output_ids.len())?;
        if !output.available {
            session.pending_output_ids.push(output_key.clone());
        }
        let mut result = TmCallResult::status(TmPcbStatus::SUCCESS);
        result.destination = Some(destination.clone());
        result.pcb = Some(match pcb {
            TmPcb::Io => TmPcbView::Io {
                logical_terminal: session.io_destination.clone(),
                status: TmPcbStatus::SUCCESS,
                message_sequence: session.message_sequence,
                user_id: session.user_id.clone(),
                group_name: session.group_name.clone(),
            },
            TmPcb::Alternate(name) => TmPcbView::Alternate {
                name,
                destination: Some(destination),
                status: TmPcbStatus::SUCCESS,
            },
        });
        result.output_message_ids.push(output_key.clone());
        mutate(
            self.store.as_ref(),
            vec![
                put(
                    SESSION_NAMESPACE,
                    &session.run_unit,
                    &session,
                    Some(session_version),
                    self.limits.max_state_bytes,
                )?,
                put(
                    OUTBOUND_NAMESPACE,
                    &output_key,
                    &output,
                    None,
                    self.limits.max_state_bytes,
                )?,
                self.replay_put(
                    invocation,
                    request_digest,
                    ReplayResult::Call(result.clone()),
                    None,
                )?,
            ],
        )?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        now: u64,
        session_version: u64,
        catalog: &CatalogRow,
        transaction: &TmTransactionDefinition,
        session: SessionRow,
        conversation: Option<TmConversationAction>,
    ) -> Result<TmCallResult, HostProblem> {
        let (message_version, mut message) = self
            .message(&session.message_id)?
            .ok_or(HostProblem::NotFound)?;
        let mut mutations = Vec::new();
        let mut output_ids = Vec::new();
        self.ensure_outbound_capacity(session.output_buffers.len())?;
        for pending in &session.pending_output_ids {
            let (version, mut row) = read::<OutboundRow>(
                self.store.as_ref(),
                OUTBOUND_NAMESPACE,
                pending,
                self.limits.max_state_bytes,
            )?
            .ok_or(HostProblem::InfrastructureFailure)?;
            self.authorize_destination(invocation, &row.message.destination)?;
            row.available = true;
            output_ids.push(pending.clone());
            mutations.push(put(
                OUTBOUND_NAMESPACE,
                pending,
                &row,
                Some(version),
                self.limits.max_state_bytes,
            )?);
        }
        for (ordinal, (key, buffer)) in session.output_buffers.iter().enumerate() {
            self.authorize_destination(invocation, &buffer.destination)?;
            let (output_key, mut output) = output_row(
                &session,
                key,
                buffer.clone(),
                session.pending_output_ids.len() + ordinal,
            )?;
            output.available = true;
            output_ids.push(output_key.clone());
            mutations.push(put(
                OUTBOUND_NAMESPACE,
                &output_key,
                &output,
                None,
                self.limits.max_state_bytes,
            )?);
        }
        self.conversation_commit(
            invocation,
            catalog,
            transaction,
            &session,
            conversation,
            &mut mutations,
        )?;
        message.state = TmMessageState::Completed;
        message.run_unit = None;
        let mut result = TmCallResult::status(TmPcbStatus::SUCCESS);
        result.output_message_ids = output_ids;
        result.conversation_id = session.conversation_id.clone();
        let work = replay_work(&session, WorkDisposition::Complete);
        mutations.extend([
            put(
                MESSAGE_NAMESPACE,
                &message.message.message_id,
                &message,
                Some(message_version),
                self.limits.max_state_bytes,
            )?,
            delete(SESSION_NAMESPACE, &session.run_unit, session_version),
            self.replay_put(
                invocation,
                request_digest,
                ReplayResult::Call(result.clone()),
                Some(work.clone()),
            )?,
        ]);
        mutate(self.store.as_ref(), mutations)?;
        self.apply_replay_work(Some(&work), now)?;
        Ok(result)
    }

    fn rollback(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        now: u64,
        session_version: u64,
        session: SessionRow,
    ) -> Result<TmCallResult, HostProblem> {
        let (message_version, mut message) = self
            .message(&session.message_id)?
            .ok_or(HostProblem::NotFound)?;
        let mut mutations = Vec::new();
        if message.new_conversation
            && let Some(conversation_id) = session.conversation_id.as_deref()
            && let Some((version, _)) = read::<ConversationRow>(
                self.store.as_ref(),
                CONVERSATION_NAMESPACE,
                conversation_id,
                self.limits.max_state_bytes,
            )?
        {
            mutations.push(delete(CONVERSATION_NAMESPACE, conversation_id, version));
        }
        for pending in &session.pending_output_ids {
            let (version, row) = read::<OutboundRow>(
                self.store.as_ref(),
                OUTBOUND_NAMESPACE,
                pending,
                self.limits.max_state_bytes,
            )?
            .ok_or(HostProblem::InfrastructureFailure)?;
            if row.message.express {
                continue;
            }
            mutations.push(delete(OUTBOUND_NAMESPACE, pending, version));
        }
        message.state = TmMessageState::Scheduled;
        message.run_unit = None;
        let result = TmCallResult::status(TmPcbStatus::SUCCESS);
        let work = replay_work(&session, WorkDisposition::Release);
        mutations.extend([
            put(
                MESSAGE_NAMESPACE,
                &message.message.message_id,
                &message,
                Some(message_version),
                self.limits.max_state_bytes,
            )?,
            delete(SESSION_NAMESPACE, &session.run_unit, session_version),
            self.replay_put(
                invocation,
                request_digest,
                ReplayResult::Call(result.clone()),
                Some(work.clone()),
            )?,
        ]);
        mutate(self.store.as_ref(), mutations)?;
        self.apply_replay_work(Some(&work), now)?;
        Ok(result)
    }

    fn persist_session_result(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        session_version: u64,
        session: SessionRow,
        result: TmCallResult,
    ) -> Result<TmCallResult, HostProblem> {
        mutate(
            self.store.as_ref(),
            vec![
                put(
                    SESSION_NAMESPACE,
                    &session.run_unit,
                    &session,
                    Some(session_version),
                    self.limits.max_state_bytes,
                )?,
                self.replay_put(
                    invocation,
                    request_digest,
                    ReplayResult::Call(result.clone()),
                    None,
                )?,
            ],
        )?;
        Ok(result)
    }
}
