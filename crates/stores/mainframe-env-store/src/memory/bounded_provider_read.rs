use super::*;

impl MemoryStore {
    pub(super) fn bounded_provider_page(
        &self,
        namespace: &str,
        max: usize,
        max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        crate::provider_scan::limits(namespace, max, max_bytes)?;
        let state = self.lock()?;
        let mut count = 0;
        let mut bytes = 0;
        for ((stored_namespace, key), row) in state
            .provider_state
            .iter()
            .filter(|((candidate, _), _)| candidate == namespace)
            .take(max)
        {
            crate::provider_scan::charge(
                &mut bytes,
                row.namespace.len(),
                row.key.len(),
                row.payload.len(),
                max_bytes,
            )?;
            row.validate_write(self.limits.max_blob_bytes)?;
            if stored_namespace != &row.namespace || key != &row.key {
                return Err(StoreError::IncompatibleVersion);
            }
            count += 1;
        }
        // No owned output or payload clones until the entire borrowed page passes.
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| StoreError::CapacityExceeded)?;
        rows.extend(
            state
                .provider_state
                .iter()
                .filter(|((candidate, _), _)| candidate == namespace)
                .take(max)
                .map(|(_, row)| row.clone()),
        );
        Ok(rows)
    }

    pub(super) fn legacy_provider_page(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .provider_state
            .iter()
            .filter(|((candidate, _), _)| candidate == namespace)
            .take(max)
            .map(|(_, record)| record.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests;
