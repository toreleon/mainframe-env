//! Existing SQLite writers share the exact native-root Closing gate.
use super::*;
impl SqliteStateStore {
    async fn root_guard_actor(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        execution: &ExecutionId,
        terminal: bool,
    ) -> Result<(), StoreError> {
        let Some(binding) = self
            .root_optional_row(tx, ACTOR_NAMESPACE, execution.as_str())
            .await?
        else {
            return Ok(());
        };
        let root =
            std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        let doc = Document::read(&self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?)?;
        if !doc.actors.iter().any(|a| a.execution == execution.as_str()) {
            return Err(StoreError::IncompatibleVersion);
        }
        if doc.phase != Phase::Open || (doc.root == execution.as_str() && terminal) {
            return Err(StoreError::InvalidTransition);
        }
        Ok(())
    }
    pub(in crate::sqlite) async fn root_guard_mutations(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        mutations: &[ProviderStateMutation],
    ) -> Result<(), StoreError> {
        for mutation in mutations {
            let (namespace, key, proposed) = match mutation {
                ProviderStateMutation::Put(w) => {
                    (&w.record.namespace, &w.record.key, Some(&w.record.payload))
                }
                ProviderStateMutation::Delete { namespace, key, .. } => (namespace, key, None),
                ProviderStateMutation::Move {
                    record, old_key, ..
                } => {
                    self.root_guard_identity(tx, &record.namespace, old_key, None)
                        .await?;
                    (&record.namespace, &record.key, Some(&record.payload))
                }
            };
            self.root_guard_identity(tx, namespace, key, proposed.map(Vec::as_slice))
                .await?;
        }
        Ok(())
    }
    async fn root_guard_identity(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
        proposed: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        if namespace.starts_with("durable-root-") {
            return Err(StoreError::InvalidTransition);
        }
        for (n, k) in [
            (SCOPE_NAMESPACE, namespace.to_string()),
            (ROW_SCOPE_NAMESPACE, row_scope_key(namespace, key)),
        ] {
            if let Some(binding) = self.root_optional_row(tx, n, &k).await? {
                let root = std::str::from_utf8(&binding.payload)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                let doc = Document::read(&self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?)?;
                if doc.phase != Phase::Open {
                    return Err(StoreError::InvalidTransition);
                }
            }
        }
        let roots: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace=?")
                .bind(RUN_NAMESPACE)
                .fetch_one(&mut **tx)
                .await
                .map_err(infrastructure)?;
        if roots == 0 {
            return Ok(());
        }
        if namespace == "durable-execution" || namespace == "durable-checkpoint" {
            let id = ExecutionId::new(key, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?;
            self.root_guard_actor(tx, &id, false).await?;
        } else if let Some(id) = namespace.strip_prefix("durable-event:") {
            let id = ExecutionId::new(id, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?;
            self.root_guard_actor(tx, &id, false).await?;
        }
        let retained = self.root_optional_row(tx, namespace, key).await?;
        for bytes in retained
            .as_ref()
            .map(|r| r.payload.as_slice())
            .into_iter()
            .chain(proposed)
        {
            if namespace == "durable-execution" {
                let value: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| StoreError::IncompatibleVersion)?;
                let version = value
                    .get("version")
                    .and_then(serde_json::Value::as_u64)
                    .or_else(|| retained.as_ref().map(|r| r.version))
                    .ok_or(StoreError::IncompatibleVersion)?;
                let execution = durable::decode_execution(bytes, version)?;
                if execution.execution_id.as_str() != key {
                    return Err(StoreError::IncompatibleVersion);
                }
                if retained.is_none()
                    && self
                        .root_optional_row(tx, RUN_NAMESPACE, execution.run_unit_id.as_str())
                        .await?
                        .is_some()
                {
                    return Err(StoreError::InvalidTransition);
                }
                self.root_guard_actor(
                    tx,
                    &execution.execution_id,
                    execution.state.terminal() || execution.state == ExecutionState::Completing,
                )
                .await?;
            } else if namespace == "durable-effect" {
                let effect_key = mainframe_env_execution_api::IdempotencyKey::new(
                    key,
                    InvocationLimits::default(),
                )
                .map_err(|_| StoreError::IncompatibleVersion)?;
                let effect = durable::decode_effect(&effect_key, bytes)?;
                if self
                    .root_optional_row(tx, RUN_NAMESPACE, effect.run_unit_id.as_str())
                    .await?
                    .is_some()
                    && self
                        .root_optional_row(tx, ACTOR_NAMESPACE, effect.execution_id.as_str())
                        .await?
                        .is_none()
                {
                    return Err(StoreError::InvalidTransition);
                }
                if let Some(binding) = self
                    .root_optional_row(tx, ACTOR_NAMESPACE, effect.execution_id.as_str())
                    .await?
                {
                    let root = std::str::from_utf8(&binding.payload)
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                    if Document::read(&self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?)?.run
                        != effect.run_unit_id.as_str()
                    {
                        return Err(StoreError::InvalidTransition);
                    }
                }
                self.root_guard_actor(tx, &effect.execution_id, false)
                    .await?;
            } else if namespace.starts_with("durable-event:") {
                let event = durable::decode_event(bytes)?;
                if namespace != format!("durable-event:{}", event.execution_id) {
                    return Err(StoreError::IncompatibleVersion);
                }
                self.root_guard_actor(
                    tx,
                    &event.execution_id,
                    matches!(
                        event.kind,
                        mainframe_env_execution_api::LifecycleEventKind::Completing
                            | mainframe_env_execution_api::LifecycleEventKind::Completed { .. }
                            | mainframe_env_execution_api::LifecycleEventKind::Abend
                            | mainframe_env_execution_api::LifecycleEventKind::Failed
                            | mainframe_env_execution_api::LifecycleEventKind::Cancelled
                            | mainframe_env_execution_api::LifecycleEventKind::TimedOut
                    ),
                )
                .await?;
            } else if namespace == "durable-outbox" {
                let version = retained.as_ref().map_or(1, |r| r.version);
                let notification = durable::decode_outbox(bytes, version)?;
                if let Some(binding) = self
                    .root_optional_row(tx, ACTOR_NAMESPACE, notification.execution_id.as_str())
                    .await?
                {
                    let root = std::str::from_utf8(&binding.payload)
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                    let document =
                        Document::read(&self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?)?;
                    if document.phase == Phase::Closing
                        || (document.phase == Phase::Terminal && proposed.is_none())
                    {
                        return Err(StoreError::InvalidTransition);
                    }
                    if document.phase == Phase::Terminal && Some(bytes) == proposed {
                        let old = durable::decode_outbox(
                            &retained.as_ref().ok_or(StoreError::NotFound)?.payload,
                            version,
                        )?;
                        if !notification.delivered
                            || old.notification_id != notification.notification_id
                            || old.execution_id != notification.execution_id
                            || old.sequence != notification.sequence
                            || old.topic != notification.topic
                            || old.payload != notification.payload
                        {
                            return Err(StoreError::InvalidTransition);
                        }
                    }
                }
            } else if namespace == "durable-checkpoint" || namespace == "durable-work" {
                let id = if namespace == "durable-work" {
                    durable::decode_work(bytes)?.execution_id
                } else {
                    durable::decode_checkpoint(bytes)?.execution_id
                };
                if self
                    .root_optional_row(tx, ACTOR_NAMESPACE, id.as_str())
                    .await?
                    .is_some()
                {
                    return Err(StoreError::InvalidTransition);
                }
            } else if matches!(
                namespace,
                "cobol-call-replay@1"
                    | "cobol-call-protocol@2"
                    | "cobol-run-state@1"
                    | "cobol-cancel@1"
            ) {
                // Exact enrolled identities were checked first. This header
                // fences an enrolled run; it grants no CALL/schema legality.
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                    if let Some(run) = value
                        .get("owner_run_unit")
                        .and_then(serde_json::Value::as_str)
                    {
                        if let Some(binding) =
                            self.root_optional_row(tx, RUN_NAMESPACE, run).await?
                        {
                            let root = std::str::from_utf8(&binding.payload)
                                .map_err(|_| StoreError::IncompatibleVersion)?;
                            if Document::read(
                                &self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?,
                            )?
                            .phase
                                != Phase::Open
                            {
                                return Err(StoreError::InvalidTransition);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
