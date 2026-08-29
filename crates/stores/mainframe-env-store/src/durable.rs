use crate::{PostgresStateStore, SqliteStateStore};
use base64::Engine;
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, InvocationLimits, LifecycleEvent, LifecycleEventKind,
    PrincipalId, RunUnitId, Selector,
};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, CheckpointRecord, CheckpointStore, EffectRecord, EffectState,
    EventStore, ExecutionRecord, ExecutionState, ExecutionStore, GenerationRecord, GenerationStore,
    IdempotencyStore, JournalStore, OutboxRecord, OutboxStore, ProviderStateRecord,
    ProviderStateStore, ProviderStateWrite, SessionRecord, SessionStore, StoreError, WorkRecord,
    WorkState, WorkStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

macro_rules! durable_implementations {
    ($store:ty) => {
        impl ExecutionStore for $store {
            fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
                if record.version != 1
                    || record.attempt == 0
                    || record.state != ExecutionState::Admitted
                {
                    return Err(StoreError::InvalidTransition);
                }
                self.put_provider_state(
                    state_record(
                        "durable-execution",
                        record.execution_id.as_str(),
                        1,
                        encode_execution(&record)?,
                    ),
                    None,
                )
                .map_err(already_exists)
            }

            fn get_execution(
                &self,
                id: &ExecutionId,
            ) -> Result<Option<ExecutionRecord>, StoreError> {
                self.get_provider_state("durable-execution", id.as_str())?
                    .map(|row| decode_execution(&row.payload, row.version))
                    .transpose()
            }

            fn transition_execution(
                &self,
                id: &ExecutionId,
                expected_version: u64,
                next: ExecutionState,
            ) -> Result<ExecutionRecord, StoreError> {
                let current = self.get_execution(id)?.ok_or(StoreError::NotFound)?;
                if current.version != expected_version {
                    return Err(StoreError::Conflict);
                }
                if !current.state.can_transition_to(next) {
                    return Err(StoreError::InvalidTransition);
                }
                let mut updated = current;
                updated.state = next;
                updated.version = updated.version.checked_add(1).ok_or(StoreError::Conflict)?;
                self.put_provider_state(
                    state_record(
                        "durable-execution",
                        id.as_str(),
                        updated.version,
                        encode_execution(&updated)?,
                    ),
                    Some(expected_version),
                )?;
                Ok(updated)
            }
        }

        impl EventStore for $store {
            fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError> {
                if !event.validate() {
                    return Err(StoreError::InvalidSequence);
                }
                let namespace = format!("durable-event:{}", event.execution_id);
                let events = self.list_provider_state(&namespace, 65536)?;
                let expected = events.last().map_or(Ok(1), |row| {
                    row.key
                        .parse::<u64>()
                        .map_err(|_| StoreError::IncompatibleVersion)
                        .and_then(|sequence| {
                            sequence.checked_add(1).ok_or(StoreError::InvalidSequence)
                        })
                })?;
                if event.sequence != expected {
                    return Err(StoreError::InvalidSequence);
                }
                self.put_provider_state(
                    state_record(
                        &namespace,
                        &format!("{:020}", event.sequence),
                        1,
                        encode_event(&event)?,
                    ),
                    None,
                )
                .map_err(|error| match error {
                    StoreError::Conflict => StoreError::InvalidSequence,
                    other => other,
                })
            }

            fn events(
                &self,
                id: &ExecutionId,
                start_sequence: u64,
                max: usize,
            ) -> Result<Vec<LifecycleEvent>, StoreError> {
                if max == 0 || max > 65536 {
                    return Err(StoreError::CapacityExceeded);
                }
                self.list_provider_state(&format!("durable-event:{id}"), 65536)?
                    .into_iter()
                    .filter(|row| {
                        row.key
                            .parse::<u64>()
                            .is_ok_and(|value| value >= start_sequence)
                    })
                    .take(max)
                    .map(|row| decode_event(&row.payload))
                    .collect()
            }
        }

        impl WorkStore for $store {
            fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
                if work.work_id.is_empty()
                    || work.state != WorkState::Queued
                    || work.attempt != 0
                    || work.max_attempts == 0
                    || work.deadline_tick == 0
                    || work.required_generation.is_empty()
                {
                    return Err(StoreError::InvalidTransition);
                }
                self.put_provider_state(
                    state_record("durable-work", &work.work_id, 1, encode_work(&work)?),
                    None,
                )
                .map_err(already_exists)
            }

            fn claim(
                &self,
                worker: &str,
                now_tick: u64,
                lease_ticks: u64,
            ) -> Result<Option<WorkRecord>, StoreError> {
                if worker.is_empty() || lease_ticks == 0 {
                    return Err(StoreError::LeaseConflict);
                }
                for row in self.list_provider_state("durable-work", 65536)? {
                    let mut work = decode_work(&row.payload)?;
                    if work.state == WorkState::Claimed
                        && work
                            .lease_expiry_tick
                            .is_some_and(|expiry| expiry <= now_tick)
                    {
                        work.state = if work.cancellation_requested {
                            WorkState::Cancelled
                        } else if work.attempt >= work.max_attempts
                            || work.deadline_tick <= now_tick
                        {
                            WorkState::DeadLetter
                        } else {
                            WorkState::Queued
                        };
                        work.worker_id = None;
                        work.lease_id = None;
                        work.lease_expiry_tick = None;
                        work.heartbeat_tick = None;
                        self.put_provider_state(
                            state_record(
                                "durable-work",
                                &work.work_id,
                                row.version + 1,
                                encode_work(&work)?,
                            ),
                            Some(row.version),
                        )?;
                        continue;
                    }
                    if work.state == WorkState::Queued && work.available_tick <= now_tick {
                        work.attempt = work.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
                        work.state = WorkState::Claimed;
                        work.worker_id = Some(worker.into());
                        work.lease_id = Some(format!("{worker}:{}", work.attempt));
                        work.lease_expiry_tick = Some(
                            now_tick
                                .checked_add(lease_ticks)
                                .ok_or(StoreError::Conflict)?,
                        );
                        work.heartbeat_tick = Some(now_tick);
                        match self.put_provider_state(
                            state_record(
                                "durable-work",
                                &work.work_id,
                                row.version + 1,
                                encode_work(&work)?,
                            ),
                            Some(row.version),
                        ) {
                            Ok(()) => return Ok(Some(work)),
                            Err(StoreError::Conflict) => continue,
                            Err(error) => return Err(error),
                        }
                    }
                }
                Ok(None)
            }

            fn heartbeat(
                &self,
                work_id: &str,
                lease_id: &str,
                now_tick: u64,
                lease_ticks: u64,
            ) -> Result<WorkRecord, StoreError> {
                if lease_ticks == 0 {
                    return Err(StoreError::LeaseConflict);
                }
                let row = self
                    .get_provider_state("durable-work", work_id)?
                    .ok_or(StoreError::NotFound)?;
                let mut work = decode_work(&row.payload)?;
                if work.state != WorkState::Claimed
                    || work.lease_id.as_deref() != Some(lease_id)
                    || work
                        .lease_expiry_tick
                        .is_none_or(|expiry| expiry <= now_tick)
                {
                    return Err(StoreError::LeaseConflict);
                }
                work.heartbeat_tick = Some(now_tick);
                work.lease_expiry_tick = Some(
                    now_tick
                        .checked_add(lease_ticks)
                        .ok_or(StoreError::LeaseConflict)?,
                );
                self.put_provider_state(
                    state_record(
                        "durable-work",
                        work_id,
                        row.version + 1,
                        encode_work(&work)?,
                    ),
                    Some(row.version),
                )?;
                Ok(work)
            }

            fn release(
                &self,
                work_id: &str,
                lease_id: &str,
                available_tick: u64,
            ) -> Result<WorkRecord, StoreError> {
                let row = self
                    .get_provider_state("durable-work", work_id)?
                    .ok_or(StoreError::NotFound)?;
                let mut work = decode_work(&row.payload)?;
                valid_lease(&work, lease_id)?;
                work.state = if work.cancellation_requested {
                    WorkState::Cancelled
                } else if work.attempt >= work.max_attempts {
                    WorkState::DeadLetter
                } else {
                    WorkState::Queued
                };
                work.available_tick = available_tick;
                clear_lease(&mut work);
                self.put_provider_state(
                    state_record(
                        "durable-work",
                        work_id,
                        row.version + 1,
                        encode_work(&work)?,
                    ),
                    Some(row.version),
                )?;
                Ok(work)
            }

            fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
                let row = self
                    .get_provider_state("durable-work", work_id)?
                    .ok_or(StoreError::NotFound)?;
                let mut work = decode_work(&row.payload)?;
                work.cancellation_requested = true;
                if work.state == WorkState::Queued {
                    work.state = WorkState::Cancelled;
                }
                self.put_provider_state(
                    state_record(
                        "durable-work",
                        work_id,
                        row.version + 1,
                        encode_work(&work)?,
                    ),
                    Some(row.version),
                )?;
                Ok(work)
            }

            fn dead_letter(&self, work_id: &str, lease_id: &str) -> Result<WorkRecord, StoreError> {
                let row = self
                    .get_provider_state("durable-work", work_id)?
                    .ok_or(StoreError::NotFound)?;
                let mut work = decode_work(&row.payload)?;
                valid_lease(&work, lease_id)?;
                work.state = WorkState::DeadLetter;
                clear_lease(&mut work);
                self.put_provider_state(
                    state_record(
                        "durable-work",
                        work_id,
                        row.version + 1,
                        encode_work(&work)?,
                    ),
                    Some(row.version),
                )?;
                Ok(work)
            }

            fn complete(&self, work_id: &str, lease_id: &str) -> Result<(), StoreError> {
                let row = self
                    .get_provider_state("durable-work", work_id)?
                    .ok_or(StoreError::NotFound)?;
                let mut work = decode_work(&row.payload)?;
                valid_lease(&work, lease_id)?;
                work.state = WorkState::Completed;
                clear_lease(&mut work);
                self.put_provider_state(
                    state_record(
                        "durable-work",
                        work_id,
                        row.version + 1,
                        encode_work(&work)?,
                    ),
                    Some(row.version),
                )
            }
        }

        impl CheckpointStore for $store {
            fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError> {
                if record.schema_version != 1
                    || record.machine_schema_version == 0
                    || record.provider_generation.is_empty()
                    || record.security_classification.is_empty()
                    || record.payload.is_empty()
                    || record.payload_size != record.payload.len() as u64
                    || Sha256::digest(&record.payload).as_slice() != record.payload_digest
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                let existing =
                    self.get_provider_state("durable-checkpoint", record.execution_id.as_str())?;
                let (version, expected) =
                    existing.map_or((1, None), |row| (row.version + 1, Some(row.version)));
                self.put_provider_state(
                    state_record(
                        "durable-checkpoint",
                        record.execution_id.as_str(),
                        version,
                        encode_checkpoint(&record)?,
                    ),
                    expected,
                )
            }

            fn get_checkpoint(
                &self,
                id: &ExecutionId,
            ) -> Result<Option<CheckpointRecord>, StoreError> {
                self.get_provider_state("durable-checkpoint", id.as_str())?
                    .map(|row| decode_checkpoint(&row.payload))
                    .transpose()
            }
        }

        impl SessionStore for $store {
            fn put_session(
                &self,
                record: SessionRecord,
                expected_version: Option<u64>,
            ) -> Result<(), StoreError> {
                if record.schema_version != 1 || record.version == 0 || record.payload.is_empty() {
                    return Err(StoreError::IncompatibleVersion);
                }
                self.put_provider_state(
                    state_record(
                        "durable-session",
                        &record.session_id,
                        record.version,
                        encode_session(&record)?,
                    ),
                    expected_version,
                )
            }

            fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
                self.get_provider_state("durable-session", id)?
                    .map(|row| decode_session(&row.payload, row.version))
                    .transpose()
            }
        }

        impl ArtifactStore for $store {
            fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
                let digest: [u8; 32] = Sha256::digest(&record.payload).into();
                if record.payload_digest != digest
                    || record.artifact.as_str() != format!("sha256:{}", hex(&digest))
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                if let Some(existing) = self.get_artifact(&record.artifact)? {
                    return if existing == record {
                        Ok(())
                    } else {
                        Err(StoreError::Conflict)
                    };
                }
                self.put_provider_state(
                    state_record(
                        "durable-artifact",
                        record.artifact.as_str(),
                        1,
                        encode_artifact(&record)?,
                    ),
                    None,
                )
            }

            fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
                self.get_provider_state("durable-artifact", id.as_str())?
                    .map(|row| decode_artifact(id, &row.payload))
                    .transpose()
            }
        }

        impl GenerationStore for $store {
            fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError> {
                if record.provider.is_empty() || record.generation.is_empty() || record.version == 0
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                let existing = self.get_provider_state("durable-generation", &record.provider)?;
                if existing
                    .as_ref()
                    .is_some_and(|row| row.version >= record.version)
                {
                    return Err(StoreError::Conflict);
                }
                self.put_provider_state(
                    state_record(
                        "durable-generation",
                        &record.provider,
                        record.version,
                        encode_generation(&record)?,
                    ),
                    existing.map(|row| row.version),
                )
            }

            fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError> {
                self.get_provider_state("durable-generation", provider)?
                    .map(|row| decode_generation(provider, &row.payload, row.version))
                    .transpose()
            }
        }

        impl IdempotencyStore for $store {
            fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError> {
                if record.sequence == 0
                    || record.state != EffectState::Intent
                    || record.result_digest.is_some()
                {
                    return Err(StoreError::InvalidTransition);
                }
                if let Some(existing) = self.effect(&record.key)? {
                    return if existing == record {
                        Ok(())
                    } else {
                        Err(StoreError::Conflict)
                    };
                }
                self.put_provider_state(
                    state_record(
                        "durable-effect",
                        record.key.as_str(),
                        1,
                        encode_effect(&record)?,
                    ),
                    None,
                )
            }

            fn record_result(
                &self,
                key: &IdempotencyKey,
                record: EffectRecord,
            ) -> Result<(), StoreError> {
                if &record.key != key
                    || !matches!(
                        record.state,
                        EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome
                    )
                {
                    return Err(StoreError::InvalidTransition);
                }
                let intent = self.effect(key)?.ok_or(StoreError::NotFound)?;
                if intent.execution_id != record.execution_id
                    || intent.run_unit_id != record.run_unit_id
                    || intent.sequence != record.sequence
                    || intent.request_digest != record.request_digest
                    || intent.state != EffectState::Intent
                {
                    return Err(StoreError::Conflict);
                }
                self.put_provider_state(
                    state_record("durable-effect", key.as_str(), 2, encode_effect(&record)?),
                    Some(1),
                )
            }

            fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError> {
                self.get_provider_state("durable-effect", key.as_str())?
                    .map(|row| decode_effect(key, &row.payload))
                    .transpose()
            }

            fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError> {
                if max == 0 || max > 65536 {
                    return Err(StoreError::CapacityExceeded);
                }
                let rows = self.list_provider_state("durable-effect", 65536)?;
                let mut records = Vec::new();
                for row in rows {
                    let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                    let record = decode_effect(&key, &row.payload)?;
                    if record.state == EffectState::UnknownOutcome {
                        records.push(record);
                        if records.len() == max {
                            break;
                        }
                    }
                }
                Ok(records)
            }

            fn reconcile_unknown(
                &self,
                key: &IdempotencyKey,
                final_state: EffectState,
                result_digest: [u8; 32],
            ) -> Result<EffectRecord, StoreError> {
                if !matches!(final_state, EffectState::Completed | EffectState::Failed) {
                    return Err(StoreError::InvalidTransition);
                }
                let row = self
                    .get_provider_state("durable-effect", key.as_str())?
                    .ok_or(StoreError::NotFound)?;
                if row.version != 2 {
                    return Err(StoreError::Conflict);
                }
                let mut record = decode_effect(key, &row.payload)?;
                if record.state != EffectState::UnknownOutcome {
                    return Err(StoreError::InvalidTransition);
                }
                record.state = final_state;
                record.result_digest = Some(result_digest);
                self.put_provider_state(
                    state_record("durable-effect", key.as_str(), 3, encode_effect(&record)?),
                    Some(2),
                )?;
                Ok(record)
            }
        }

        impl OutboxStore for $store {
            fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError> {
                if record.notification_id.is_empty()
                    || record.topic.is_empty()
                    || record.sequence == 0
                    || record.attempt != 0
                    || record.delivered
                    || record.version != 1
                {
                    return Err(StoreError::InvalidTransition);
                }
                if let Some(existing) = self
                    .get_provider_state("durable-outbox", &record.notification_id)?
                    .map(|row| decode_outbox(&row.payload, row.version))
                    .transpose()?
                {
                    return if existing == record {
                        Ok(())
                    } else {
                        Err(StoreError::Conflict)
                    };
                }
                self.put_provider_state(
                    state_record(
                        "durable-outbox",
                        &record.notification_id,
                        1,
                        encode_outbox(&record)?,
                    ),
                    None,
                )
            }

            fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError> {
                if max == 0 || max > 65536 {
                    return Err(StoreError::CapacityExceeded);
                }
                let records = self
                    .list_provider_state("durable-outbox", 65536)?
                    .into_iter()
                    .map(|row| decode_outbox(&row.payload, row.version))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(records
                    .into_iter()
                    .filter(|record| !record.delivered)
                    .take(max)
                    .collect())
            }

            fn mark_notification_delivered(
                &self,
                notification_id: &str,
                expected_version: u64,
            ) -> Result<OutboxRecord, StoreError> {
                let row = self
                    .get_provider_state("durable-outbox", notification_id)?
                    .ok_or(StoreError::NotFound)?;
                if row.version != expected_version {
                    return Err(StoreError::Conflict);
                }
                let mut record = decode_outbox(&row.payload, row.version)?;
                if record.delivered {
                    return Err(StoreError::Conflict);
                }
                record.delivered = true;
                record.attempt = record.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
                record.version = record.version.checked_add(1).ok_or(StoreError::Conflict)?;
                self.put_provider_state(
                    state_record(
                        "durable-outbox",
                        notification_id,
                        record.version,
                        encode_outbox(&record)?,
                    ),
                    Some(expected_version),
                )?;
                Ok(record)
            }
        }

        impl JournalStore for $store {
            fn admit_execution(
                &self,
                execution: ExecutionRecord,
                event: LifecycleEvent,
                notification: OutboxRecord,
            ) -> Result<(), StoreError> {
                if execution.version != 1
                    || execution.attempt == 0
                    || execution.state != ExecutionState::Admitted
                    || execution.execution_id != event.execution_id
                    || !event.validate()
                    || event.execution_id != notification.execution_id
                    || event.sequence != notification.sequence
                    || notification.version != 1
                    || notification.delivered
                {
                    return Err(StoreError::InvalidSequence);
                }
                let namespace = format!("durable-event:{}", event.execution_id);
                if event.sequence != 1 {
                    return Err(StoreError::InvalidSequence);
                }
                self.put_provider_states_atomic(vec![
                    ProviderStateWrite {
                        record: state_record(
                            "durable-execution",
                            execution.execution_id.as_str(),
                            1,
                            encode_execution(&execution)?,
                        ),
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: state_record(
                            &namespace,
                            &format!("{:020}", event.sequence),
                            1,
                            encode_event(&event)?,
                        ),
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: state_record(
                            "durable-outbox",
                            &notification.notification_id,
                            1,
                            encode_outbox(&notification)?,
                        ),
                        expected_version: None,
                    },
                ])
            }

            fn commit_execution_step(
                &self,
                execution_id: &ExecutionId,
                expected_version: u64,
                next_state: Option<ExecutionState>,
                event: LifecycleEvent,
                effect: Option<EffectRecord>,
                checkpoint: Option<CheckpointRecord>,
                notification: OutboxRecord,
            ) -> Result<ExecutionRecord, StoreError> {
                if &event.execution_id != execution_id
                    || event.execution_id != notification.execution_id
                    || event.sequence != notification.sequence
                    || notification.version != 1
                    || notification.delivered
                {
                    return Err(StoreError::InvalidSequence);
                }
                let mut execution = self
                    .get_execution(execution_id)?
                    .ok_or(StoreError::NotFound)?;
                if execution.version != expected_version {
                    return Err(StoreError::Conflict);
                }
                if let Some(next) = next_state {
                    if !execution.state.can_transition_to(next) {
                        return Err(StoreError::InvalidTransition);
                    }
                    execution.state = next;
                }
                execution.version = execution
                    .version
                    .checked_add(1)
                    .ok_or(StoreError::Conflict)?;
                let namespace = format!("durable-event:{}", event.execution_id);
                let events = self.list_provider_state(&namespace, 65536)?;
                let next_sequence = events.last().map_or(Ok(1), |row| {
                    row.key
                        .parse::<u64>()
                        .map_err(|_| StoreError::IncompatibleVersion)
                        .and_then(|sequence| {
                            sequence.checked_add(1).ok_or(StoreError::InvalidSequence)
                        })
                })?;
                if event.sequence != next_sequence {
                    return Err(StoreError::InvalidSequence);
                }
                let mut writes = vec![
                    ProviderStateWrite {
                        record: state_record(
                            "durable-execution",
                            execution_id.as_str(),
                            execution.version,
                            encode_execution(&execution)?,
                        ),
                        expected_version: Some(expected_version),
                    },
                    ProviderStateWrite {
                        record: state_record(
                            &namespace,
                            &format!("{:020}", event.sequence),
                            1,
                            encode_event(&event)?,
                        ),
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: state_record(
                            "durable-outbox",
                            &notification.notification_id,
                            1,
                            encode_outbox(&notification)?,
                        ),
                        expected_version: None,
                    },
                ];
                if let Some(effect) = effect {
                    if effect.execution_id != *execution_id {
                        return Err(StoreError::Conflict);
                    }
                    let (version, expected) = match effect.state {
                        EffectState::Intent => (1, None),
                        EffectState::Completed
                        | EffectState::Failed
                        | EffectState::UnknownOutcome => (2, Some(1)),
                    };
                    writes.push(ProviderStateWrite {
                        record: state_record(
                            "durable-effect",
                            effect.key.as_str(),
                            version,
                            encode_effect(&effect)?,
                        ),
                        expected_version: expected,
                    });
                }
                if let Some(checkpoint) = checkpoint {
                    if checkpoint.execution_id != *execution_id
                        || checkpoint.payload_size != checkpoint.payload.len() as u64
                    {
                        return Err(StoreError::IncompatibleVersion);
                    }
                    let existing =
                        self.get_provider_state("durable-checkpoint", execution_id.as_str())?;
                    let (version, expected) =
                        existing.map_or((1, None), |row| (row.version + 1, Some(row.version)));
                    writes.push(ProviderStateWrite {
                        record: state_record(
                            "durable-checkpoint",
                            execution_id.as_str(),
                            version,
                            encode_checkpoint(&checkpoint)?,
                        ),
                        expected_version: expected,
                    });
                }
                self.put_provider_states_atomic(writes)?;
                Ok(execution)
            }
        }
    };
}

durable_implementations!(SqliteStateStore);
durable_implementations!(PostgresStateStore);

fn state_record(namespace: &str, key: &str, version: u64, payload: Vec<u8>) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version,
        payload,
    }
}

fn encode(value: Value) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(&value).map_err(|error| StoreError::Infrastructure(error.to_string()))
}

fn decode(bytes: &[u8]) -> Result<Value, StoreError> {
    serde_json::from_slice(bytes).map_err(|_| StoreError::IncompatibleVersion)
}

fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, StoreError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or(StoreError::IncompatibleVersion)
}

fn number(value: &Value, name: &str) -> Result<u64, StoreError> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or(StoreError::IncompatibleVersion)
}

fn optional_string(value: &Value, name: &str) -> Result<Option<String>, StoreError> {
    match value.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(StoreError::IncompatibleVersion),
    }
}

fn binary(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes)
}

fn binary_back(value: &str) -> Result<Vec<u8>, StoreError> {
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(value)
        .map_err(|_| StoreError::IncompatibleVersion)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest_back(value: &str) -> Result<[u8; 32], StoreError> {
    if value.len() != 64 {
        return Err(StoreError::IncompatibleVersion);
    }
    let mut output = [0; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| StoreError::IncompatibleVersion)?;
    }
    Ok(output)
}

fn encode_execution(record: &ExecutionRecord) -> Result<Vec<u8>, StoreError> {
    encode(json!({
        "schema":1,"execution":record.execution_id.as_str(),"run":record.run_unit_id.as_str(),
        "selector":record.selector.as_str(),"artifact":record.artifact.as_str(),
        "principal":record.principal.as_str(),"state":execution_state(record.state),
        "attempt":record.attempt,"lease":record.owner_lease,"expiry":record.lease_expiry_tick
    }))
}

fn decode_execution(bytes: &[u8], version: u64) -> Result<ExecutionRecord, StoreError> {
    let value = decode(bytes)?;
    let limits = InvocationLimits::default();
    Ok(ExecutionRecord {
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        run_unit_id: RunUnitId::new(string(&value, "run")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        selector: Selector::new(string(&value, "selector")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        artifact: ArtifactRef::new(string(&value, "artifact")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        principal: PrincipalId::new(string(&value, "principal")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        state: execution_state_back(string(&value, "state")?)?,
        attempt: u32::try_from(number(&value, "attempt")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        version,
        owner_lease: optional_string(&value, "lease")?,
        lease_expiry_tick: value.get("expiry").and_then(Value::as_u64),
    })
}

fn execution_state(value: ExecutionState) -> &'static str {
    match value {
        ExecutionState::Admitted => "admitted",
        ExecutionState::Queued => "queued",
        ExecutionState::Running => "running",
        ExecutionState::Suspended => "suspended",
        ExecutionState::Completing => "completing",
        ExecutionState::Completed => "completed",
        ExecutionState::Failed => "failed",
        ExecutionState::Cancelled => "cancelled",
        ExecutionState::TimedOut => "timed-out",
        ExecutionState::DeadLetter => "dead-letter",
    }
}

fn execution_state_back(value: &str) -> Result<ExecutionState, StoreError> {
    Ok(match value {
        "admitted" => ExecutionState::Admitted,
        "queued" => ExecutionState::Queued,
        "running" => ExecutionState::Running,
        "suspended" => ExecutionState::Suspended,
        "completing" => ExecutionState::Completing,
        "completed" => ExecutionState::Completed,
        "failed" => ExecutionState::Failed,
        "cancelled" => ExecutionState::Cancelled,
        "timed-out" => ExecutionState::TimedOut,
        "dead-letter" => ExecutionState::DeadLetter,
        _ => return Err(StoreError::IncompatibleVersion),
    })
}

fn encode_event(event: &LifecycleEvent) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":1,"execution":event.execution_id.as_str(),"run":event.run_unit_id.as_str(),
        "sequence":event.sequence,"attempt":event.attempt,"tick":event.tick,"kind":event_kind(&event.kind)}),
    )
}

fn decode_event(bytes: &[u8]) -> Result<LifecycleEvent, StoreError> {
    let value = decode(bytes)?;
    let limits = InvocationLimits::default();
    Ok(LifecycleEvent {
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        run_unit_id: RunUnitId::new(string(&value, "run")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        sequence: number(&value, "sequence")?,
        attempt: u32::try_from(number(&value, "attempt")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        tick: number(&value, "tick")?,
        kind: event_kind_back(string(&value, "kind")?)?,
    })
}

fn event_kind(value: &LifecycleEventKind) -> String {
    match value {
        LifecycleEventKind::EffectIntent { sequence } => format!("effect-intent:{sequence}"),
        LifecycleEventKind::EffectResult { sequence } => format!("effect-result:{sequence}"),
        LifecycleEventKind::Completed { return_code } => format!("completed:{return_code}"),
        other => format!("{other:?}").to_ascii_lowercase().replace('_', "-"),
    }
}

fn event_kind_back(value: &str) -> Result<LifecycleEventKind, StoreError> {
    if let Some(sequence) = value.strip_prefix("effect-intent:") {
        return Ok(LifecycleEventKind::EffectIntent {
            sequence: sequence
                .parse()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        });
    }
    if let Some(sequence) = value.strip_prefix("effect-result:") {
        return Ok(LifecycleEventKind::EffectResult {
            sequence: sequence
                .parse()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        });
    }
    if let Some(code) = value.strip_prefix("completed:") {
        return Ok(LifecycleEventKind::Completed {
            return_code: code.parse().map_err(|_| StoreError::IncompatibleVersion)?,
        });
    }
    Ok(match value {
        "admitted" => LifecycleEventKind::Admitted,
        "queued" => LifecycleEventKind::Queued,
        "claimed" => LifecycleEventKind::Claimed,
        "started" => LifecycleEventKind::Started,
        "completing" => LifecycleEventKind::Completing,
        "suspended" => LifecycleEventKind::Suspended,
        "resumed" => LifecycleEventKind::Resumed,
        "cancellationrequested" | "cancellation-requested" => {
            LifecycleEventKind::CancellationRequested
        }
        "cancelled" => LifecycleEventKind::Cancelled,
        "timedout" | "timed-out" => LifecycleEventKind::TimedOut,
        "condition" => LifecycleEventKind::Condition,
        "abend" => LifecycleEventKind::Abend,
        "failed" => LifecycleEventKind::Failed,
        _ => return Err(StoreError::IncompatibleVersion),
    })
}

fn encode_work(work: &WorkRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":1,"id":work.work_id,"execution":work.execution_id.as_str(),
        "selector":work.required_selector.as_str(),"generation":work.required_generation,
        "artifact":work.artifact.as_str(),"state":work_state(work.state),"attempt":work.attempt,
        "max_attempts":work.max_attempts,"available":work.available_tick,"deadline":work.deadline_tick,
        "cancel":work.cancellation_requested,"worker":work.worker_id,"lease":work.lease_id,
        "expiry":work.lease_expiry_tick,"heartbeat":work.heartbeat_tick,
        "checkpoint":work.checkpoint_id,"effect":work.effect_sequence,"payload":binary(&work.payload)}),
    )
}
fn decode_work(bytes: &[u8]) -> Result<WorkRecord, StoreError> {
    let value = decode(bytes)?;
    let limits = InvocationLimits::default();
    Ok(WorkRecord {
        work_id: string(&value, "id")?.into(),
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        required_selector: Selector::new(string(&value, "selector")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        required_generation: string(&value, "generation")?.into(),
        artifact: ArtifactRef::new(string(&value, "artifact")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        state: work_state_back(string(&value, "state")?)?,
        attempt: u32::try_from(number(&value, "attempt")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        max_attempts: u32::try_from(number(&value, "max_attempts")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        available_tick: number(&value, "available")?,
        deadline_tick: number(&value, "deadline")?,
        cancellation_requested: value
            .get("cancel")
            .and_then(Value::as_bool)
            .ok_or(StoreError::IncompatibleVersion)?,
        worker_id: optional_string(&value, "worker")?,
        lease_id: optional_string(&value, "lease")?,
        lease_expiry_tick: value.get("expiry").and_then(Value::as_u64),
        heartbeat_tick: value.get("heartbeat").and_then(Value::as_u64),
        checkpoint_id: optional_string(&value, "checkpoint")?,
        effect_sequence: number(&value, "effect")?,
        payload: binary_back(string(&value, "payload")?)?,
    })
}
fn work_state(value: WorkState) -> &'static str {
    match value {
        WorkState::Queued => "queued",
        WorkState::Claimed => "claimed",
        WorkState::Completed => "completed",
        WorkState::Cancelled => "cancelled",
        WorkState::DeadLetter => "dead-letter",
    }
}
fn work_state_back(value: &str) -> Result<WorkState, StoreError> {
    Ok(match value {
        "queued" => WorkState::Queued,
        "claimed" => WorkState::Claimed,
        "completed" => WorkState::Completed,
        "cancelled" => WorkState::Cancelled,
        "dead-letter" => WorkState::DeadLetter,
        _ => return Err(StoreError::IncompatibleVersion),
    })
}

fn encode_checkpoint(record: &CheckpointRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":record.schema_version,"machine_schema":record.machine_schema_version,
        "execution":record.execution_id.as_str(),"run":record.run_unit_id.as_str(),
        "session":record.session_id,"artifact":record.artifact.as_str(),
        "generation":record.provider_generation,"interfaces":record.required_host_interfaces,
        "effect":record.effect_sequence,"transaction":record.transaction,
        "principal":record.principal.as_str(),"classification":record.security_classification,
        "encryption_key":record.encryption_key_reference,"size":record.payload_size,
        "digest":hex(&record.payload_digest),"payload":binary(&record.payload)}),
    )
}
fn decode_checkpoint(bytes: &[u8]) -> Result<CheckpointRecord, StoreError> {
    let value = decode(bytes)?;
    let limits = InvocationLimits::default();
    let required_host_interfaces = value
        .get("interfaces")
        .and_then(Value::as_object)
        .ok_or(StoreError::IncompatibleVersion)?
        .iter()
        .map(|(name, version)| {
            version
                .as_str()
                .map(|version| (name.clone(), version.to_string()))
                .ok_or(StoreError::IncompatibleVersion)
        })
        .collect::<Result<_, _>>()?;
    let record = CheckpointRecord {
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        run_unit_id: RunUnitId::new(string(&value, "run")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        session_id: optional_string(&value, "session")?,
        schema_version: u32::try_from(number(&value, "schema")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        machine_schema_version: u32::try_from(number(&value, "machine_schema")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        artifact: ArtifactRef::new(string(&value, "artifact")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        provider_generation: string(&value, "generation")?.into(),
        required_host_interfaces,
        effect_sequence: number(&value, "effect")?,
        transaction: optional_string(&value, "transaction")?,
        principal: PrincipalId::new(string(&value, "principal")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        security_classification: string(&value, "classification")?.into(),
        encryption_key_reference: optional_string(&value, "encryption_key")?,
        payload_size: number(&value, "size")?,
        payload_digest: digest_back(string(&value, "digest")?)?,
        payload: binary_back(string(&value, "payload")?)?,
    };
    if record.payload_size != record.payload.len() as u64
        || Sha256::digest(&record.payload).as_slice() != record.payload_digest
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(record)
}

fn encode_session(record: &SessionRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":record.schema_version,"id":record.session_id,"principal":record.principal.as_str(),"payload":binary(&record.payload)}),
    )
}
fn decode_session(bytes: &[u8], version: u64) -> Result<SessionRecord, StoreError> {
    let value = decode(bytes)?;
    Ok(SessionRecord {
        session_id: string(&value, "id")?.into(),
        principal: PrincipalId::new(string(&value, "principal")?, InvocationLimits::default())
            .map_err(|_| StoreError::IncompatibleVersion)?,
        schema_version: u32::try_from(number(&value, "schema")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        version,
        payload: binary_back(string(&value, "payload")?)?,
    })
}

fn encode_artifact(record: &ArtifactRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":1,"media":record.media_type,"digest":hex(&record.payload_digest),"payload":binary(&record.payload)}),
    )
}
fn decode_artifact(id: &ArtifactRef, bytes: &[u8]) -> Result<ArtifactRecord, StoreError> {
    let value = decode(bytes)?;
    let record = ArtifactRecord {
        artifact: id.clone(),
        media_type: string(&value, "media")?.into(),
        payload_digest: digest_back(string(&value, "digest")?)?,
        payload: binary_back(string(&value, "payload")?)?,
    };
    if Sha256::digest(&record.payload).as_slice() != record.payload_digest {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(record)
}

fn encode_generation(record: &GenerationRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":1,"generation":record.generation,"ready":record.ready,"draining":record.draining}),
    )
}
fn decode_generation(
    provider: &str,
    bytes: &[u8],
    version: u64,
) -> Result<GenerationRecord, StoreError> {
    let value = decode(bytes)?;
    Ok(GenerationRecord {
        provider: provider.into(),
        generation: string(&value, "generation")?.into(),
        ready: value
            .get("ready")
            .and_then(Value::as_bool)
            .ok_or(StoreError::IncompatibleVersion)?,
        draining: value
            .get("draining")
            .and_then(Value::as_bool)
            .ok_or(StoreError::IncompatibleVersion)?,
        version,
    })
}

fn encode_effect(record: &EffectRecord) -> Result<Vec<u8>, StoreError> {
    encode(
        json!({"schema":1,"execution":record.execution_id.as_str(),"run":record.run_unit_id.as_str(),"sequence":record.sequence,"request":hex(&record.request_digest),"state":effect_state(record.state),"result":record.result_digest.map(|value|hex(&value))}),
    )
}
fn decode_effect(key: &IdempotencyKey, bytes: &[u8]) -> Result<EffectRecord, StoreError> {
    let value = decode(bytes)?;
    let limits = InvocationLimits::default();
    Ok(EffectRecord {
        execution_id: ExecutionId::new(string(&value, "execution")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        run_unit_id: RunUnitId::new(string(&value, "run")?, limits)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        sequence: number(&value, "sequence")?,
        key: key.clone(),
        request_digest: digest_back(string(&value, "request")?)?,
        state: effect_state_back(string(&value, "state")?)?,
        result_digest: match value.get("result") {
            Some(Value::String(value)) => Some(digest_back(value)?),
            Some(Value::Null) | None => None,
            _ => return Err(StoreError::IncompatibleVersion),
        },
    })
}
fn effect_state(value: EffectState) -> &'static str {
    match value {
        EffectState::Intent => "intent",
        EffectState::Completed => "completed",
        EffectState::Failed => "failed",
        EffectState::UnknownOutcome => "unknown-outcome",
    }
}
fn effect_state_back(value: &str) -> Result<EffectState, StoreError> {
    Ok(match value {
        "intent" => EffectState::Intent,
        "completed" => EffectState::Completed,
        "failed" => EffectState::Failed,
        "unknown-outcome" => EffectState::UnknownOutcome,
        _ => return Err(StoreError::IncompatibleVersion),
    })
}

fn encode_outbox(record: &OutboxRecord) -> Result<Vec<u8>, StoreError> {
    encode(json!({"schema":1,"id":record.notification_id,
        "execution":record.execution_id.as_str(),"sequence":record.sequence,
        "topic":record.topic,"payload":binary(&record.payload),"attempt":record.attempt,
        "delivered":record.delivered}))
}

fn decode_outbox(bytes: &[u8], version: u64) -> Result<OutboxRecord, StoreError> {
    let value = decode(bytes)?;
    Ok(OutboxRecord {
        notification_id: string(&value, "id")?.into(),
        execution_id: ExecutionId::new(string(&value, "execution")?, InvocationLimits::default())
            .map_err(|_| StoreError::IncompatibleVersion)?,
        sequence: number(&value, "sequence")?,
        topic: string(&value, "topic")?.into(),
        payload: binary_back(string(&value, "payload")?)?,
        attempt: u32::try_from(number(&value, "attempt")?)
            .map_err(|_| StoreError::IncompatibleVersion)?,
        delivered: value
            .get("delivered")
            .and_then(Value::as_bool)
            .ok_or(StoreError::IncompatibleVersion)?,
        version,
    })
}

fn valid_lease(work: &WorkRecord, lease_id: &str) -> Result<(), StoreError> {
    if work.state == WorkState::Claimed && work.lease_id.as_deref() == Some(lease_id) {
        Ok(())
    } else {
        Err(StoreError::LeaseConflict)
    }
}

fn clear_lease(work: &mut WorkRecord) {
    work.worker_id = None;
    work.lease_id = None;
    work.lease_expiry_tick = None;
    work.heartbeat_tick = None;
}

fn already_exists(error: StoreError) -> StoreError {
    if error == StoreError::Conflict {
        StoreError::AlreadyExists
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store_api::{
        ArtifactStore, EventStore, ExecutionStore, JournalStore, OutboxStore, WorkStore,
    };

    #[test]
    fn sqlite_durable_traits_survive_reopen_and_enforce_integrity() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-durable-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("durable.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let limits = InvocationLimits::default();
        let execution = ExecutionId::new("exec", limits).unwrap();
        let journal_execution = ExecutionId::new("journal-exec", limits).unwrap();
        {
            let store = SqliteStateStore::open(&url, 8 * 1024 * 1024, 262144).unwrap();
            store
                .create_execution(ExecutionRecord {
                    execution_id: execution.clone(),
                    run_unit_id: RunUnitId::new("run", limits).unwrap(),
                    selector: Selector::new("program:HELLO", limits).unwrap(),
                    artifact: ArtifactRef::new("artifact", limits).unwrap(),
                    principal: PrincipalId::new("IBMUSER", limits).unwrap(),
                    state: ExecutionState::Admitted,
                    attempt: 1,
                    version: 1,
                    owner_lease: None,
                    lease_expiry_tick: None,
                })
                .unwrap();
            store
                .enqueue(WorkRecord {
                    work_id: "work".into(),
                    execution_id: execution.clone(),
                    required_selector: Selector::new("program:HELLO", limits).unwrap(),
                    required_generation: "mainframe-env-reference@1".into(),
                    artifact: ArtifactRef::new("artifact", limits).unwrap(),
                    state: WorkState::Queued,
                    attempt: 0,
                    max_attempts: 3,
                    available_tick: 0,
                    deadline_tick: 100,
                    cancellation_requested: false,
                    worker_id: None,
                    lease_id: None,
                    lease_expiry_tick: None,
                    heartbeat_tick: None,
                    checkpoint_id: None,
                    effect_sequence: 0,
                    payload: vec![1],
                })
                .unwrap();
            let payload = b"artifact".to_vec();
            let digest: [u8; 32] = Sha256::digest(&payload).into();
            store
                .put_artifact(ArtifactRecord {
                    artifact: ArtifactRef::new(format!("sha256:{}", hex(&digest)), limits).unwrap(),
                    media_type: "application/octet-stream".into(),
                    payload_digest: digest,
                    payload,
                })
                .unwrap();
            let event = LifecycleEvent {
                execution_id: journal_execution.clone(),
                run_unit_id: RunUnitId::new("journal-run", limits).unwrap(),
                sequence: 1,
                attempt: 1,
                tick: 1,
                kind: LifecycleEventKind::Admitted,
            };
            store
                .admit_execution(
                    ExecutionRecord {
                        execution_id: journal_execution.clone(),
                        run_unit_id: event.run_unit_id.clone(),
                        selector: Selector::new("program:JOURNAL", limits).unwrap(),
                        artifact: ArtifactRef::new("journal-artifact", limits).unwrap(),
                        principal: PrincipalId::new("IBMUSER", limits).unwrap(),
                        state: ExecutionState::Admitted,
                        attempt: 1,
                        version: 1,
                        owner_lease: None,
                        lease_expiry_tick: None,
                    },
                    event,
                    OutboxRecord {
                        notification_id: "journal-exec:1".into(),
                        execution_id: journal_execution.clone(),
                        sequence: 1,
                        topic: "execution.lifecycle".into(),
                        payload: b"admitted".to_vec(),
                        attempt: 0,
                        delivered: false,
                        version: 1,
                    },
                )
                .unwrap();
        }
        {
            let store = SqliteStateStore::open(&url, 8 * 1024 * 1024, 262144).unwrap();
            assert_eq!(
                store.get_execution(&execution).unwrap().unwrap().state,
                ExecutionState::Admitted
            );
            let claimed = store.claim("worker", 1, 10).unwrap().unwrap();
            assert_eq!(claimed.attempt, 1);
            assert_eq!(
                store
                    .get_execution(&journal_execution)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Admitted
            );
            assert_eq!(store.events(&journal_execution, 1, 8).unwrap().len(), 1);
            assert!(
                store
                    .pending_notifications(8)
                    .unwrap()
                    .iter()
                    .any(|record| record.notification_id == "journal-exec:1")
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    #[ignore = "requires MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn postgres18_migration_and_durable_contracts() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").unwrap();
        let store = PostgresStateStore::open(&url, 8 * 1024 * 1024, 262144).unwrap();
        let limits = InvocationLimits::default();
        let suffix = std::process::id();
        let execution = ExecutionId::new(format!("pg-exec-{suffix}"), limits).unwrap();
        store
            .create_execution(ExecutionRecord {
                execution_id: execution.clone(),
                run_unit_id: RunUnitId::new(format!("pg-run-{suffix}"), limits).unwrap(),
                selector: Selector::new("program:HELLO", limits).unwrap(),
                artifact: ArtifactRef::new("artifact", limits).unwrap(),
                principal: PrincipalId::new("IBMUSER", limits).unwrap(),
                state: ExecutionState::Admitted,
                attempt: 1,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
            })
            .unwrap();
        assert_eq!(
            store
                .transition_execution(&execution, 1, ExecutionState::Queued)
                .unwrap()
                .state,
            ExecutionState::Queued
        );
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: format!("pg-test-{suffix}"),
                    key: "one".into(),
                    version: 1,
                    payload: b"postgres-18".to_vec(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .get_provider_state(&format!("pg-test-{suffix}"), "one")
                .unwrap()
                .unwrap()
                .payload,
            b"postgres-18"
        );
    }
}
