use super::*;

impl SqliteStateStore {
    pub(super) fn bounded_provider_page(
        &self,
        namespace: &str,
        max: usize,
        max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        crate::provider_scan::limits(namespace, max, max_bytes)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = self
                .provider_page_in_snapshot(&mut transaction, namespace, max, max_bytes)
                .await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    async fn provider_page_in_snapshot(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        max: usize,
        max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        let limit = i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?;
        // Only fixed-size projections cross into Rust before the byte preflight.
        // BLOB casts measure UTF-8 bytes rather than SQLite character counts.
        let metadata = sqlx::query(
            "SELECT length(CAST(namespace AS BLOB)),length(CAST(key AS BLOB)),\
             length(CAST(payload AS BLOB)),\
             CASE WHEN typeof(namespace)='text' AND typeof(key)='text' AND \
             typeof(payload)='blob' AND typeof(version)='integer' THEN 1 ELSE 0 END,\
             CASE WHEN typeof(version)='integer' AND version>0 THEN version ELSE 0 END \
             FROM provider_state WHERE namespace=? ORDER BY key LIMIT ?",
        )
        .bind(namespace)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        let count = metadata.len();
        if count > max {
            return Err(StoreError::CapacityExceeded);
        }
        let mut bytes = 0;
        // Budget refusal precedes decoding even a malformed physical version.
        for row in &metadata {
            let lengths = [0, 1, 2].map(|index| {
                row.try_get::<i64, _>(index)
                    .map_err(|_| StoreError::IncompatibleVersion)
                    .and_then(|value| {
                        usize::try_from(value).map_err(|_| StoreError::IncompatibleVersion)
                    })
            });
            crate::provider_scan::charge(
                &mut bytes,
                lengths[0].clone()?,
                lengths[1].clone()?,
                lengths[2].clone()?,
                max_bytes,
            )?;
        }
        for row in &metadata {
            let namespace_bytes: i64 = row.try_get(0).map_err(infrastructure)?;
            let key_bytes: i64 = row.try_get(1).map_err(infrastructure)?;
            let payload_bytes: i64 = row.try_get(2).map_err(infrastructure)?;
            let shape: i64 = row.try_get(3).map_err(infrastructure)?;
            let version: i64 = row.try_get(4).map_err(infrastructure)?;
            if namespace_bytes != namespace.len() as i64
                || key_bytes <= 0
                || key_bytes > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES as i64
                || shape != 1
                || version <= 0
            {
                return Err(StoreError::IncompatibleVersion);
            }
            if payload_bytes as usize > self.max_payload_bytes {
                return Err(StoreError::PayloadTooLarge);
            }
        }
        drop(metadata);
        // Validate all key bytes (including UTF-8) without fetching payloads.
        let key_rows = sqlx::query(
            "SELECT CAST(key AS BLOB) FROM provider_state WHERE namespace=? ORDER BY key LIMIT ?",
        )
        .bind(namespace)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(count)
            .map_err(|_| StoreError::CapacityExceeded)?;
        for row in key_rows {
            let key: Vec<u8> = row.try_get(0).map_err(infrastructure)?;
            let key = String::from_utf8(key).map_err(|_| StoreError::IncompatibleVersion)?;
            if keys.last().is_some_and(|previous| previous >= &key) {
                return Err(StoreError::IncompatibleVersion);
            }
            keys.push(key);
        }
        if keys.len() != count {
            return Err(StoreError::IncompatibleVersion);
        }
        let physical = sqlx::query(
            "SELECT key,version,payload FROM provider_state WHERE namespace=? ORDER BY key LIMIT ?",
        )
        .bind(namespace)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        if physical.len() != count {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| StoreError::CapacityExceeded)?;
        for (row, key) in physical.into_iter().zip(keys) {
            let decoded_key: String = row.try_get(0).map_err(infrastructure)?;
            if decoded_key != key {
                return Err(StoreError::IncompatibleVersion);
            }
            let record = ProviderStateRecord {
                namespace: namespace.into(),
                key,
                version: u64::try_from(row.try_get::<i64, _>(1).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                payload: row.try_get(2).map_err(infrastructure)?,
            };
            record.validate_write(self.max_payload_bytes)?;
            result.push(record);
        }
        Ok(result)
    }

    pub(super) fn legacy_provider_page(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self.run(
            sqlx::query(
                "SELECT key,version,payload FROM provider_state \
                 WHERE namespace=? ORDER BY key LIMIT ?",
            )
            .bind(namespace)
            .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
            .fetch_all(&self.pool),
        )?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderStateRecord {
                    namespace: namespace.into(),
                    key: row
                        .try_get(0)
                        .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
                    version: u64::try_from(
                        row.try_get::<i64, _>(1)
                            .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
                    )
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                    payload: row
                        .try_get(2)
                        .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
