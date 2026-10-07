//! Native root ownership and terminal publication in the existing SQLite TX.
use super::*;
use crate::root_terminal::{
    ACTOR_NAMESPACE, Document, Phase, ROW_SCOPE_NAMESPACE, RUN_NAMESPACE, SCOPE_NAMESPACE,
    row_scope_key,
};
use crate::{durable, validation};
use mainframe_env_execution_api::{AuditSubjectRecord, ExecutionId, InvocationLimits};
use mainframe_env_store_api::*;
mod capture;
mod guards;
mod preparation;
mod provider_writer;

fn put(record: ProviderStateRecord, expected_version: Option<u64>) -> ProviderStateMutation {
    ProviderStateMutation::Put(ProviderStateWrite {
        record,
        expected_version,
    })
}
fn index(namespace: &str, key: &str, root: &str) -> ProviderStateMutation {
    put(
        ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: 1,
            payload: root.as_bytes().to_vec(),
        },
        None,
    )
}
fn admission_rows(
    execution: &ExecutionRecord,
    event: &mainframe_env_execution_api::LifecycleEvent,
    notification: &OutboxRecord,
) -> Result<Vec<ProviderStateMutation>, StoreError> {
    validation::admission(execution, event, notification)?;
    Ok(vec![
        put(
            ProviderStateRecord {
                namespace: "durable-execution".into(),
                key: execution.execution_id.as_str().into(),
                version: execution.version,
                payload: durable::encode_execution(execution)?,
            },
            None,
        ),
        put(
            ProviderStateRecord {
                namespace: format!("durable-event:{}", event.execution_id),
                key: format!("{:020}", event.sequence),
                version: 1,
                payload: durable::encode_event(event)?,
            },
            None,
        ),
        put(
            ProviderStateRecord {
                namespace: "durable-outbox".into(),
                key: notification.notification_id.clone(),
                version: 1,
                payload: durable::encode_outbox(notification)?,
            },
            None,
        ),
    ])
}

impl SqliteStateStore {
    pub(crate) fn root_fence(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        claim.admission().validate()?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                let current = self
                    .root_row(
                        &mut tx,
                        ROOT_DRIVER_NAMESPACE,
                        claim.admission().execution.execution_id.as_str(),
                    )
                    .await?;
                let mut doc = Document::read(&current)?;
                doc.require_claim(claim)?;
                let actor = self
                    .root_row(&mut tx, "durable-execution", &doc.root)
                    .await?;
                if tick == 0
                    || tick > i64::MAX as u64
                    || tick < floor
                    || execution.state.terminal()
                    || execution.execution_id.as_str() != doc.root
                    || doc.phase == Phase::Terminal
                    || durable::decode_execution(&actor.payload, actor.version)? != *execution
                {
                    return Err(StoreError::Conflict);
                }
                if doc.phase == Phase::Uncertain {
                    return Ok(current);
                }
                doc.phase = Phase::Uncertain;
                let next = doc.row(
                    current.version.checked_add(1).ok_or(StoreError::Conflict)?,
                    self.max_payload_bytes,
                )?;
                self.apply_root_mutations_in(
                    &mut tx,
                    vec![put(next.clone(), Some(current.version))],
                )
                .await?;
                self.root_clock(&mut tx, tick).await?;
                Ok(next)
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_admit(
        &self,
        admission: RootDriverAdmission,
    ) -> Result<RootDriverClaim, StoreError> {
        let doc = Document::new(&admission)?;
        let root = doc.row(1, self.max_payload_bytes)?;
        let claim = admission.observe_inserted(&root)?;
        let mut mutations = admission_rows(
            &admission.execution,
            &admission.event,
            &admission.notification,
        )?;
        mutations.extend([
            index(RUN_NAMESPACE, &doc.run, &doc.root),
            index(ACTOR_NAMESPACE, &doc.root, &doc.root),
            put(root, None),
        ]);
        for namespace in &doc.provider_namespaces {
            mutations.push(index(SCOPE_NAMESPACE, namespace, &doc.root));
        }
        for (namespace, key) in &doc.provider_rows {
            mutations.push(index(
                ROW_SCOPE_NAMESPACE,
                &row_scope_key(namespace, key),
                &doc.root,
            ));
        }
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                doc.require_live(admission.event.tick, floor)?;
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-execution' AND json_extract(payload,'$.run')=?")
                    .bind(&doc.run).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                let effects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-effect' AND json_extract(payload,'$.run')=?")
                    .bind(&doc.run).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                if count != 0 || effects != 0 { return Err(StoreError::Conflict); }
                self.root_require_no_work(&mut tx, &admission.execution.execution_id).await?;
                self.apply_root_mutations_in(&mut tx, mutations).await?;
                self.root_clock(&mut tx, admission.event.tick).await?;
                Ok(claim)
            }.await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_admit_child(&self, admission: RootChildAdmission) -> Result<(), StoreError> {
        admission.claim.admission().validate()?;
        let mut mutations = admission_rows(
            &admission.execution,
            &admission.event,
            &admission.notification,
        )?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                let current = self
                    .root_row(
                        &mut tx,
                        ROOT_DRIVER_NAMESPACE,
                        admission.claim.admission().execution.execution_id.as_str(),
                    )
                    .await?;
                let mut doc = Document::read(&current)?;
                doc.require_live(admission.event.tick, floor)?;
                let parent = self
                    .root_row(&mut tx, "durable-execution", admission.parent.as_str())
                    .await?;
                if durable::decode_execution(&parent.payload, parent.version)?
                    != admission.parent_occurrence.execution
                {
                    return Err(StoreError::Conflict);
                }
                let effect = self
                    .root_row(
                        &mut tx,
                        "durable-effect",
                        admission.parent_occurrence.effect_key.as_str(),
                    )
                    .await?;
                let intent = durable::decode_effect(
                    &admission.parent_occurrence.effect_key,
                    &effect.payload,
                )?;
                doc.enroll(&admission, &intent)?;
                self.root_dependency(
                    &mut tx,
                    &TerminalRowDependency::Exact(admission.call.clone()),
                )
                .await?;
                self.root_dependency(
                    &mut tx,
                    &TerminalRowDependency::Exact(admission.catalog.clone()),
                )
                .await?;
                self.root_require_no_work(&mut tx, &admission.execution.execution_id)
                    .await?;
                mutations.push(index(
                    ACTOR_NAMESPACE,
                    admission.execution.execution_id.as_str(),
                    &doc.root,
                ));
                mutations.push(put(
                    doc.row(
                        current.version.checked_add(1).ok_or(StoreError::Conflict)?,
                        self.max_payload_bytes,
                    )?,
                    Some(current.version),
                ));
                self.apply_root_mutations_in(&mut tx, mutations).await?;
                self.root_clock(&mut tx, admission.event.tick).await
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_register_row(
        &self,
        admission: RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        admission.claim.admission().validate()?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                let current = self
                    .root_row(
                        &mut tx,
                        ROOT_DRIVER_NAMESPACE,
                        admission.claim.admission().execution.execution_id.as_str(),
                    )
                    .await?;
                let mut doc = Document::read(&current)?;
                doc.require_live(admission.observed_tick, floor)?;
                let actor_row = self
                    .root_row(
                        &mut tx,
                        "durable-execution",
                        admission.execution.execution_id.as_str(),
                    )
                    .await?;
                if durable::decode_execution(&actor_row.payload, actor_row.version)?
                    != admission.execution
                {
                    return Err(StoreError::Conflict);
                }
                let effect = self
                    .root_row(&mut tx, "durable-effect", admission.effect_key.as_str())
                    .await?;
                let was_registered = doc.provider_rows.iter().any(|(n, k)| {
                    n == &admission.identity.namespace && k == &admission.identity.key
                });
                doc.register_row(
                    &admission,
                    &durable::decode_effect(&admission.effect_key, &effect.payload)?,
                )?;
                let key = row_scope_key(&admission.identity.namespace, &admission.identity.key);
                if let Some(binding) = self
                    .root_optional_row(&mut tx, ROW_SCOPE_NAMESPACE, &key)
                    .await?
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
                self.apply_root_mutations_in(
                    &mut tx,
                    vec![
                        index(ROW_SCOPE_NAMESPACE, &key, &doc.root),
                        put(
                            doc.row(
                                current.version.checked_add(1).ok_or(StoreError::Conflict)?,
                                self.max_payload_bytes,
                            )?,
                            Some(current.version),
                        ),
                    ],
                )
                .await
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_close(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        observed_tick: u64,
    ) -> Result<RootClosureSnapshot, StoreError> {
        claim.admission().validate()?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                let current = self
                    .root_row(
                        &mut tx,
                        ROOT_DRIVER_NAMESPACE,
                        claim.admission().execution.execution_id.as_str(),
                    )
                    .await?;
                let mut doc = Document::read(&current)?;
                doc.require_claim(claim)?;
                doc.require_live(observed_tick, floor)?;
                let retained = self
                    .root_row(&mut tx, "durable-execution", &doc.root)
                    .await?;
                if doc.phase != Phase::Open
                    || durable::decode_execution(&retained.payload, retained.version)? != *execution
                    || execution.execution_id.as_str() != doc.root
                    || execution.state != ExecutionState::Running
                {
                    return Err(StoreError::Conflict);
                }
                doc.phase = Phase::Closing;
                let closing = doc.row(
                    current.version.checked_add(1).ok_or(StoreError::Conflict)?,
                    self.max_payload_bytes,
                )?;
                let provisional = self
                    .root_capture(&mut tx, claim, closing.clone(), observed_tick)
                    .await?;
                crate::root_terminal::validate_known_closure(&provisional)?;
                self.apply_root_mutations_in(
                    &mut tx,
                    vec![put(closing.clone(), Some(current.version))],
                )
                .await?;
                self.root_clock(&mut tx, observed_tick).await?;
                self.root_capture(&mut tx, claim, closing, observed_tick)
                    .await
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_commit(
        &self,
        request: RootTerminalPublication,
    ) -> Result<RootTerminalCommit, StoreError> {
        let mut doc = crate::root_terminal::validate_publication(&request, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (epoch, floor) = self.root_lock(&mut tx).await?;
                doc.require_live(request.observed_tick, floor)?;
                if epoch != request.closure.provider_epoch
                    || self
                        .root_row(&mut tx, ROOT_DRIVER_NAMESPACE, &doc.root)
                        .await?
                        != request.closure.closing
                    || self
                        .root_capture(
                            &mut tx,
                            &request.closure.claim,
                            request.closure.closing.clone(),
                            request.closure.observed_tick,
                        )
                        .await?
                        != request.closure
                {
                    return Err(StoreError::Conflict);
                }
                for dependency in request
                    .dependencies
                    .iter()
                    .chain(&request.closure.provider_dependencies)
                {
                    self.root_dependency(&mut tx, dependency).await?;
                }
                let mut execution = request.closure.actors[0].execution.clone();
                let mut mutations = request.mutations;
                for step in request.steps {
                    validation::execution_step(
                        &execution.execution_id,
                        &execution,
                        &step.event,
                        &step.notification,
                    )?;
                    if !execution.state.can_transition_to(step.next_state)
                        || step.event.sequence
                            != execution
                                .version
                                .checked_add(1)
                                .ok_or(StoreError::InvalidSequence)?
                        || step.event.tick != request.observed_tick
                    {
                        return Err(StoreError::InvalidTransition);
                    }
                    let old_version = execution.version;
                    execution.state = step.next_state;
                    execution.version = step.event.sequence;
                    execution.terminal_tick = step.next_state.terminal().then_some(step.event.tick);
                    mutations.extend([
                        put(
                            ProviderStateRecord {
                                namespace: "durable-execution".into(),
                                key: doc.root.clone(),
                                version: execution.version,
                                payload: durable::encode_execution(&execution)?,
                            },
                            Some(old_version),
                        ),
                        put(
                            ProviderStateRecord {
                                namespace: format!("durable-event:{}", doc.root),
                                key: format!("{:020}", step.event.sequence),
                                version: 1,
                                payload: durable::encode_event(&step.event)?,
                            },
                            None,
                        ),
                        put(
                            ProviderStateRecord {
                                namespace: "durable-outbox".into(),
                                key: step.notification.notification_id.clone(),
                                version: 1,
                                payload: durable::encode_outbox(&step.notification)?,
                            },
                            None,
                        ),
                    ]);
                }
                let resource = request.audits[0].resource.value;
                let mut ordinal = self
                    .root_audit_ordinal(&mut tx, &execution.execution_id)
                    .await?;
                for audit in request.audits {
                    ordinal = ordinal.checked_add(1).ok_or(StoreError::CapacityExceeded)?;
                    mutations.push(put(
                        ProviderStateRecord {
                            namespace: durable::AUDIT_NAMESPACE.into(),
                            key: durable::audit_storage_key(
                                &execution.execution_id,
                                &format!("direct:{ordinal:020}"),
                            ),
                            version: 1,
                            payload: durable::root_terminal::encode_terminal_audit(&audit)?,
                        },
                        None,
                    ));
                }
                doc.phase = Phase::Terminal;
                doc.winning_resource = Some(resource);
                let winner = doc.row(
                    request
                        .closure
                        .closing
                        .version
                        .checked_add(1)
                        .ok_or(StoreError::Conflict)?,
                    self.max_payload_bytes,
                )?;
                mutations.push(put(winner.clone(), Some(request.closure.closing.version)));
                self.apply_root_mutations_in(&mut tx, mutations).await?;
                self.root_clock(&mut tx, request.observed_tick).await?;
                Ok(RootTerminalCommit {
                    claim: request.closure.claim,
                    execution,
                    winner,
                })
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
    pub(crate) fn root_audit_subjects(
        &self,
        execution: &ExecutionId,
        max: usize,
    ) -> Result<Vec<AuditSubjectRecord>, StoreError> {
        if max == 0 || max > MAX_ROOT_OPERATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let prefix = durable::audit_storage_key(execution, "");
                let rows = self
                    .root_rows_matching(&mut tx, durable::AUDIT_NAMESPACE, Some(&prefix), None, max)
                    .await?;
                rows.into_iter()
                    .map(|row| match durable::decode_audit(&row.payload) {
                        Ok(audit) => Ok(AuditSubjectRecord::Effect(audit)),
                        Err(_) => Ok(AuditSubjectRecord::RootTerminal(
                            durable::root_terminal::decode_terminal_audit(&row.payload)?,
                        )),
                    })
                    .collect()
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
