use super::codec::{
    CATALOG_KEY, CATALOG_NAMESPACE, CONVERSATION_NAMESPACE, MESSAGE_NAMESPACE, OUTBOUND_NAMESPACE,
    PACKAGE_NAMESPACE, REPLAY_NAMESPACE, SESSION_NAMESPACE, list, put, read, store_error,
};
use super::contracts::{TmPcb, TmTransactionDefinition};
use super::model::{
    CatalogRow, ConversationRow, MessageRow, OutboundRow, OutputBuffer, PackageDefinitionsRow,
    ReplayResult, ReplayRow, ReplayWork, SessionRow, TmCallResult, TmCancelReceipt,
    TmEnqueueReceipt, TmMessageState, TmOutboundMessage, TmScheduleReceipt, WorkDisposition,
    WorkPayload,
};
use super::service::{TmService, WORK_PAYLOAD_SCHEMA};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, Invocation, InvocationLimits, Selector,
};
use mainframe_env_host_api::{
    AccessIntent, EnterpriseResource, EnterpriseResourceClass, HostProblem,
};
use mainframe_env_store_api::{ProviderStateMutation, StoreError, WorkRecord, WorkState};
use serde::Serialize;
use sha2::{Digest, Sha256};

impl TmService {
    pub(super) fn ensure_outbound_capacity(&self, additional: usize) -> Result<(), HostProblem> {
        let current = list::<OutboundRow>(
            self.store.as_ref(),
            OUTBOUND_NAMESPACE,
            self.limits.max_outbound_messages,
            self.limits.max_state_bytes,
        )?
        .len();
        if current
            .checked_add(additional)
            .is_none_or(|total| total > self.limits.max_outbound_messages)
        {
            Err(HostProblem::ResourceExhausted)
        } else {
            Ok(())
        }
    }

    pub(super) fn replay_put(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
        result: ReplayResult,
        work: Option<ReplayWork>,
    ) -> Result<ProviderStateMutation, HostProblem> {
        if list::<ReplayRow>(
            self.store.as_ref(),
            REPLAY_NAMESPACE,
            self.limits.max_replays,
            self.limits.max_state_bytes,
        )?
        .len()
            >= self.limits.max_replays
        {
            return Err(HostProblem::ResourceExhausted);
        }
        put(
            REPLAY_NAMESPACE,
            invocation.idempotency_key.as_str(),
            &ReplayRow {
                request_digest,
                result,
                work,
            },
            None,
            self.limits.max_state_bytes,
        )
    }

    pub(super) fn replay(
        &self,
        invocation: &Invocation,
        request_digest: [u8; 32],
    ) -> Result<Option<ReplayRow>, HostProblem> {
        let replay = read::<ReplayRow>(
            self.store.as_ref(),
            REPLAY_NAMESPACE,
            invocation.idempotency_key.as_str(),
            self.limits.max_state_bytes,
        )?;
        match replay {
            Some((_, replay)) if replay.request_digest == request_digest => Ok(Some(replay)),
            Some(_) => Err(HostProblem::IdempotencyConflict),
            None => Ok(None),
        }
    }

    pub(super) fn apply_replay_work(
        &self,
        work: Option<&ReplayWork>,
        now: u64,
    ) -> Result<(), HostProblem> {
        let Some(work) = work else {
            return Ok(());
        };
        let current = self
            .work_store
            .get_work(&work.work_id)
            .map_err(work_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        match work.disposition {
            WorkDisposition::Complete if current.state == WorkState::Completed => Ok(()),
            WorkDisposition::Release if current.state == WorkState::Queued => Ok(()),
            WorkDisposition::Complete => self
                .work_store
                .complete(&work.work_id, &work.lease_id, work.lease_epoch, now)
                .map_err(|_| HostProblem::UnknownOutcome),
            WorkDisposition::Release => self
                .work_store
                .release(&work.work_id, &work.lease_id, work.lease_epoch, now, now)
                .map(|_| ())
                .map_err(|_| HostProblem::UnknownOutcome),
        }
    }

    pub(super) fn ensure_work(
        &self,
        message: &MessageRow,
        transaction: &TmTransactionDefinition,
    ) -> Result<(), HostProblem> {
        let limits = InvocationLimits::default();
        let payload = serde_json::to_vec(&WorkPayload {
            schema_version: WORK_PAYLOAD_SCHEMA.into(),
            message_id: message.message.message_id.clone(),
            transaction: transaction.code.clone(),
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let execution_digest = digest("mainframe-env.ims-tm-work-identity@1", &payload)?;
        let work = WorkRecord {
            work_id: message.work_id.clone(),
            execution_id: ExecutionId::new(
                format!("ims-tm-{}", &hex_digest(&execution_digest)[..24]),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_selector: Selector::new(transaction.program_selector.clone(), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_generation: work_generation(transaction, message.package_binding.as_deref())?,
            artifact: ArtifactRef::new(transaction.artifact.clone(), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state: WorkState::Queued,
            priority: transaction.priority,
            attempt: 0,
            max_attempts: 3,
            available_tick: message.enqueue_tick,
            deadline_tick: message.deadline_tick,
            cancellation_requested: false,
            worker_id: None,
            lease_id: None,
            lease_epoch: 0,
            lease_expiry_tick: None,
            heartbeat_tick: None,
            terminal_tick: None,
            checkpoint_id: None,
            effect_sequence: message.sequence,
            payload,
        };
        match self.work_store.enqueue(work.clone()) {
            Ok(()) => Ok(()),
            Err(StoreError::AlreadyExists | StoreError::Conflict) => {
                let current = self
                    .work_store
                    .get_work(&work.work_id)
                    .map_err(work_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                if same_work_identity(&current, &work) {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                }
            }
            Err(StoreError::Infrastructure(_)) => Err(HostProblem::UnknownOutcome),
            Err(problem) => Err(work_error(problem)),
        }
    }

    pub(super) fn claimed_work(
        &self,
        supplied: &WorkRecord,
        now: u64,
    ) -> Result<WorkRecord, HostProblem> {
        let current = self
            .work_store
            .get_work(&supplied.work_id)
            .map_err(work_error)?
            .ok_or(HostProblem::NotFound)?;
        if current.state != WorkState::Claimed
            || current.lease_id != supplied.lease_id
            || current.lease_epoch != supplied.lease_epoch
            || current.lease_expiry_tick.is_none_or(|expiry| expiry <= now)
            || current.deadline_tick <= now
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(current)
    }

    pub(super) fn authorize_transaction(
        &self,
        invocation: &Invocation,
        transaction: &TmTransactionDefinition,
        intent: AccessIntent,
    ) -> Result<(), HostProblem> {
        for resource in [
            EnterpriseResource::new(
                EnterpriseResourceClass::ImsPsb,
                transaction.psb.clone(),
                AccessIntent::Execute,
            )?,
            EnterpriseResource::new(
                EnterpriseResourceClass::ImsUnitOfWork,
                transaction.code.clone(),
                intent,
            )?,
        ] {
            self.authorizer
                .authorize(invocation.principal.id(), &resource)?;
        }
        Ok(())
    }

    pub(super) fn authorize_destination(
        &self,
        invocation: &Invocation,
        destination: &str,
    ) -> Result<(), HostProblem> {
        self.authorizer.authorize(
            invocation.principal.id(),
            &EnterpriseResource::new(
                EnterpriseResourceClass::ImsUnitOfWork,
                format!("DEST.{destination}"),
                AccessIntent::Update,
            )?,
        )
    }

    pub(super) fn check_invocation(&self, invocation: &Invocation) -> Result<u64, HostProblem> {
        if invocation.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        let now = self.store.advance_logical_clock(1).map_err(store_error)?;
        if invocation.deadline_tick <= now {
            Err(HostProblem::TimedOut)
        } else {
            Ok(now)
        }
    }

    pub(super) fn catalog(&self) -> Result<Option<(u64, CatalogRow)>, HostProblem> {
        read(
            self.store.as_ref(),
            CATALOG_NAMESPACE,
            CATALOG_KEY,
            self.limits.max_state_bytes,
        )
    }

    pub(super) fn catalog_for_binding(
        &self,
        binding: Option<&str>,
    ) -> Result<CatalogRow, HostProblem> {
        let (_, mut catalog) = self.catalog()?.ok_or(HostProblem::NotFound)?;
        if let Some(binding) = binding {
            let (_, retained) = read::<PackageDefinitionsRow>(
                self.store.as_ref(),
                PACKAGE_NAMESPACE,
                binding,
                self.limits.max_state_bytes,
            )?
            .ok_or(HostProblem::InfrastructureFailure)?;
            if binding != format!("{}:{}", retained.application, retained.generation) {
                return Err(HostProblem::InfrastructureFailure);
            }
            retained.definitions.validate(self.limits)?;
            catalog.definitions = retained.definitions;
            catalog.package_binding = Some(binding.into());
            catalog.application = Some(retained.application);
        } else if catalog.package_binding.is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(catalog)
    }

    pub(super) fn message(&self, id: &str) -> Result<Option<(u64, MessageRow)>, HostProblem> {
        read(
            self.store.as_ref(),
            MESSAGE_NAMESPACE,
            id,
            self.limits.max_state_bytes,
        )
    }

    pub(super) fn session(&self, run: &str) -> Result<Option<(u64, SessionRow)>, HostProblem> {
        read(
            self.store.as_ref(),
            SESSION_NAMESPACE,
            run,
            self.limits.max_state_bytes,
        )
    }

    pub(super) fn validate_store(&self) -> Result<(), HostProblem> {
        let catalog = self.catalog()?;
        let messages = list::<MessageRow>(
            self.store.as_ref(),
            MESSAGE_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )?;
        let sessions = list::<SessionRow>(
            self.store.as_ref(),
            SESSION_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )?;
        let conversations = list::<ConversationRow>(
            self.store.as_ref(),
            CONVERSATION_NAMESPACE,
            self.limits.max_queued_messages,
            self.limits.max_state_bytes,
        )?;
        let outbound = list::<OutboundRow>(
            self.store.as_ref(),
            OUTBOUND_NAMESPACE,
            self.limits.max_outbound_messages,
            self.limits.max_state_bytes,
        )?;
        let replay = list::<ReplayRow>(
            self.store.as_ref(),
            REPLAY_NAMESPACE,
            self.limits.max_replays,
            self.limits.max_state_bytes,
        )?;
        if catalog.is_none()
            && (!messages.is_empty()
                || !sessions.is_empty()
                || !conversations.is_empty()
                || !outbound.is_empty()
                || !replay.is_empty())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some((_, catalog)) = catalog {
            catalog.definitions.validate(self.limits)?;
            if catalog.active && catalog.package_binding.is_some() {
                let selected = self.catalog_for_binding(catalog.package_binding.as_deref())?;
                if selected.definitions != catalog.definitions
                    || selected.application != catalog.application
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            if catalog.next_sequence == 0 {
                return Err(HostProblem::InfrastructureFailure);
            }
            for (_, _, message) in messages {
                message.message.validate(self.limits)?;
                let bound = self.catalog_for_binding(message.package_binding.as_deref())?;
                find_transaction(&bound, &message.message.transaction)?;
                if message.sequence == 0
                    || message.deadline_tick == 0
                    || message.enqueue_tick == 0
                    || message.enqueue_tick >= message.deadline_tick
                    || (message.state == TmMessageState::InFlight) != message.run_unit.is_some()
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            for (_, _, session) in sessions {
                let bound = self.catalog_for_binding(session.package_binding.as_deref())?;
                find_transaction(&bound, &session.transaction)?;
                if !session.io_status.valid()
                    || self.message(&session.message_id)?.is_none()
                    || session.lease_epoch == 0
                    || session.lease_id.is_empty()
                    || session.output_buffers.values().any(|buffer| {
                        buffer.segments.len() > self.limits.max_segments_per_message
                            || buffer
                                .segments
                                .iter()
                                .any(|segment| segment.len() > self.limits.max_segment_bytes)
                    })
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            for (_, _, conversation) in conversations {
                let bound = self.catalog_for_binding(conversation.package_binding.as_deref())?;
                find_transaction(&bound, &conversation.next_transaction)?;
                if conversation.spa.len() > self.limits.max_spa_bytes {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            for (_, _, output) in outbound {
                if output.message.segments.len() > self.limits.max_segments_per_message
                    || output
                        .message
                        .segments
                        .iter()
                        .any(|segment| segment.len() > self.limits.max_segment_bytes)
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            for (_, _, replay) in replay {
                if !replay.result.valid() {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
        }
        Ok(())
    }
}

pub(super) fn find_transaction<'a>(
    catalog: &'a CatalogRow,
    code: &str,
) -> Result<&'a TmTransactionDefinition, HostProblem> {
    catalog
        .definitions
        .transactions
        .iter()
        .find(|transaction| transaction.code == code)
        .ok_or(HostProblem::NotFound)
}

pub(super) fn output_route(
    session: &SessionRow,
    transaction: &TmTransactionDefinition,
    pcb: &TmPcb,
) -> Option<(String, String, bool)> {
    match pcb {
        TmPcb::Io => Some(("IO".into(), session_destination(session), false)),
        TmPcb::Alternate(name) => {
            let definition = transaction.alternate(name)?;
            let destination = session.alternate_destinations.get(name)?.clone()?;
            Some((name.clone(), destination, definition.express))
        }
    }
}

pub(super) fn session_destination(session: &SessionRow) -> String {
    session.io_destination.clone()
}

pub(super) fn output_row(
    session: &SessionRow,
    pcb: &str,
    buffer: OutputBuffer,
    ordinal: usize,
) -> Result<(String, OutboundRow), HostProblem> {
    let digest = digest(
        "mainframe-env.ims-tm-output@1",
        &(session.run_unit.as_str(), pcb, ordinal),
    )?;
    let key = format!("out-{}", &hex_digest(&digest)[..32]);
    let sequence = u64::try_from(ordinal)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok((
        key.clone(),
        OutboundRow {
            message: TmOutboundMessage {
                message_id: key,
                destination: buffer.destination,
                segments: buffer.segments,
                express: buffer.express,
                sequence,
            },
            available: buffer.express,
        },
    ))
}

pub(super) fn replay_work(session: &SessionRow, disposition: WorkDisposition) -> ReplayWork {
    ReplayWork {
        work_id: session.work_id.clone(),
        lease_id: session.lease_id.clone(),
        lease_epoch: session.lease_epoch,
        disposition,
    }
}

pub(super) fn work_generation(
    transaction: &TmTransactionDefinition,
    package_binding: Option<&str>,
) -> Result<String, HostProblem> {
    let digest = if let Some(binding) = package_binding {
        digest(
            "mainframe-env.ims-tm-package-work-generation@1",
            &(transaction.required_generation.as_str(), binding),
        )?
    } else {
        digest(
            "mainframe-env.ims-tm-work-generation@1",
            &transaction.required_generation,
        )?
    };
    Ok(format!(
        "ims-tm@1:{}:{}",
        transaction.code,
        &hex_digest(&digest)[..16]
    ))
}

pub(super) fn same_work_identity(left: &WorkRecord, right: &WorkRecord) -> bool {
    left.work_id == right.work_id
        && left.execution_id == right.execution_id
        && left.required_selector == right.required_selector
        && left.required_generation == right.required_generation
        && left.artifact == right.artifact
        && left.priority == right.priority
        && left.available_tick == right.available_tick
        && left.deadline_tick == right.deadline_tick
        && left.effect_sequence == right.effect_sequence
        && left.payload == right.payload
}

pub(super) fn invocation_digest<T: Serialize>(
    domain: &str,
    invocation: &Invocation,
    value: &T,
) -> Result<[u8; 32], HostProblem> {
    digest(
        domain,
        &(
            invocation.principal.id().as_str(),
            invocation.run_unit_id.as_str(),
            value,
        ),
    )
}

pub(super) fn digest<T: Serialize>(domain: &str, value: &T) -> Result<[u8; 32], HostProblem> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
    Ok(digest.finalize().into())
}

pub(super) fn hex_digest(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn work_error(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::NotFound => HostProblem::NotFound,
        StoreError::AlreadyExists | StoreError::Conflict | StoreError::LeaseConflict => {
            HostProblem::IdempotencyConflict
        }
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

pub(super) fn enqueue_replay(replay: ReplayRow) -> Result<TmEnqueueReceipt, HostProblem> {
    match replay.result {
        ReplayResult::Enqueue(mut receipt) => {
            receipt.replayed = true;
            Ok(receipt)
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}

pub(super) fn schedule_replay(replay: ReplayRow) -> Result<TmScheduleReceipt, HostProblem> {
    match replay.result {
        ReplayResult::Schedule(mut receipt) => {
            receipt.replayed = true;
            Ok(receipt)
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}

pub(super) fn call_replay(replay: ReplayRow) -> Result<TmCallResult, HostProblem> {
    match replay.result {
        ReplayResult::Call(mut result) => {
            result.replayed = true;
            Ok(result)
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}

pub(super) fn cancel_replay(replay: ReplayRow) -> Result<TmCancelReceipt, HostProblem> {
    match replay.result {
        ReplayResult::Cancel(mut result) => {
            result.replayed = true;
            Ok(result)
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}
