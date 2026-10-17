//! Existing physical SQLite transaction; no callback or sequential fallback.
use super::*;

impl SqliteStateStore {
    pub(crate) fn root_capture_cutoff(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<RootCustodyCutoff, StoreError> {
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
                doc.require_live(tick, floor)?;
                let actor = self
                    .root_row(&mut tx, "durable-execution", &doc.root)
                    .await?;
                if doc.phase != Phase::Open
                    || execution.state != ExecutionState::Running
                    || execution.execution_id.as_str() != doc.root
                    || durable::decode_execution(&actor.payload, actor.version)? != *execution
                    || self
                        .root_optional_row(&mut tx, ROOT_CUSTODY_NAMESPACE, &doc.root)
                        .await?
                        .is_some()
                {
                    return Err(StoreError::Conflict);
                }
                let mut preserved = self
                    .root_capture(&mut tx, claim, current.clone(), tick)
                    .await?;
                for (namespace, key) in
                    crate::root_terminal::custody::custody_index_identities(&doc)
                {
                    let retained = self.root_row(&mut tx, &namespace, &key).await?;
                    doc.require_recorded_index(&retained)?;
                    crate::root_terminal::custody::custody_require_row(&preserved, &retained)?;
                    preserved
                        .provider_dependencies
                        .push(TerminalRowDependency::Exact(retained));
                    preserved.validate_bounds()?;
                }
                for actor in &doc.actors {
                    let id = ExecutionId::new(&actor.execution, InvocationLimits::default())
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                    let prefix = durable::audit_storage_key(&id, "");
                    let (operations, bytes) =
                        crate::root_terminal::custody::custody_remaining(&preserved)?;
                    let rows = self
                        .root_rows_bounded(
                            &mut tx,
                            durable::AUDIT_NAMESPACE,
                            Some(&prefix),
                            None,
                            operations,
                            bytes,
                        )
                        .await?;
                    for row in rows {
                        preserved.core_records.push(row);
                        preserved.validate_bounds()?;
                    }
                }
                doc.phase = Phase::Uncertain;
                let fenced = doc.row(
                    current.version.checked_add(1).ok_or(StoreError::Conflict)?,
                    self.max_payload_bytes,
                )?;
                let mut result = RootCustodyCutoff {
                    preserved,
                    fenced,
                    marker: ProviderStateRecord {
                        namespace: ROOT_CUSTODY_NAMESPACE.into(),
                        key: doc.root,
                        version: 1,
                        payload: Vec::new(),
                    },
                    fenced_epoch: 0,
                };
                result.marker = crate::root_terminal::custody::custody_marker(&result)?;
                result.validate_bounds()?;
                self.apply_root_mutations_in(
                    &mut tx,
                    vec![
                        put(result.fenced.clone(), Some(current.version)),
                        put(result.marker.clone(), None),
                    ],
                )
                .await?;
                self.root_clock(&mut tx, tick).await?;
                result.fenced_epoch = self.root_lock(&mut tx).await?.0;
                Ok(result)
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
