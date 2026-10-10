//! Reuse the original complete physical cutoff recapture for owned-file reopen.
use super::*;
impl SqliteStateStore {
    pub(in crate::sqlite) async fn root_require_cutoff_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        cutoff: &RootCustodyCutoff,
    ) -> Result<u64, StoreError> {
        cutoff.validate_bounds()?;
        let (epoch, floor) = self.root_lock(tx).await?;
        let old = Document::read(&cutoff.preserved.closing)?;
        old.require_claim(&cutoff.preserved.claim)?;
        if old.phase != Phase::Open {
            return Err(StoreError::InvalidTransition);
        }
        let mut fenced = old.clone();
        fenced.phase = Phase::Uncertain;
        let version = cutoff
            .preserved
            .closing
            .version
            .checked_add(1)
            .ok_or(StoreError::Conflict)?;
        if fenced.row(version, self.max_payload_bytes)? != cutoff.fenced
            || crate::root_terminal::custody::custody_marker(cutoff)? != cutoff.marker
        {
            return Err(StoreError::Conflict);
        }
        if epoch != cutoff.fenced_epoch
            || self.root_row(tx, ROOT_DRIVER_NAMESPACE, &old.root).await? != cutoff.fenced
            || self.root_row(tx, ROOT_CUSTODY_NAMESPACE, &old.root).await? != cutoff.marker
        {
            return Err(StoreError::Conflict);
        }
        let mut actual = self
            .root_capture(
                tx,
                &cutoff.preserved.claim,
                cutoff.preserved.closing.clone(),
                cutoff.preserved.observed_tick,
            )
            .await?;
        for (namespace, key) in crate::root_terminal::custody::custody_index_identities(&old) {
            let retained = self.root_row(tx, &namespace, &key).await?;
            old.require_recorded_index(&retained)?;
            crate::root_terminal::custody::custody_require_row(&actual, &retained)?;
            actual
                .provider_dependencies
                .push(TerminalRowDependency::Exact(retained));
        }
        for actor in &old.actors {
            let id = ExecutionId::new(&actor.execution, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?;
            let prefix = durable::audit_storage_key(&id, "");
            let (operations, bytes) = crate::root_terminal::custody::custody_remaining(&actual)?;
            let rows = self
                .root_rows_bounded(
                    tx,
                    durable::AUDIT_NAMESPACE,
                    Some(&prefix),
                    None,
                    operations,
                    bytes,
                )
                .await?;
            actual.core_records.extend(rows);
        }
        actual.provider_epoch = cutoff.preserved.provider_epoch;
        actual.validate_bounds()?;
        if actual != cutoff.preserved {
            return Err(StoreError::Conflict);
        }
        Ok(floor)
    }
    pub(in crate::sqlite) fn root_verify_cutoff_for_owned_file(
        &self,
        cutoff: &RootCustodyCutoff,
    ) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let result = self
                .root_require_cutoff_in(&mut tx, cutoff)
                .await
                .map(|_| ());
            finish_read_transaction(tx, result).await
        })?
    }
}
