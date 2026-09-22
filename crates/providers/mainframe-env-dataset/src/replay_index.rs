//! Incremental in-memory sync for the `dataset-replay` provider-state index.
//!
//! `DatasetService` used to reload and fully decode every `dataset-replay`
//! row on each request (`fix(#194)`). `ReplayIndex` instead keeps one
//! `(version, payload)` fingerprint per key and only re-decodes a row whose
//! fingerprint changed since the last sync, matching `a5fbc43` in every
//! observable outcome.
//!
//! `sync` also leaves every unchanged row untouched: no clone, no map
//! rebuild. A profile of the application-operator submission gate found the
//! per-row cost of the first incremental-sync change was dominated by
//! recomputing a SHA-256 digest for every row on every sync, including
//! unchanged ones (~10 of ~13.4 us/row); cloning unchanged entries into a
//! freshly rebuilt map cost another ~2.8 us/row. Comparing the committed
//! `version`, then payload length, then payload bytes is cheaper than a
//! digest for these small payloads and needs no extra dependency; keeping
//! `self.entries` in place instead of rebuilding it removes the clone.

use crate::service::{
    DatasetLimits, Replay, decode_replay, describe_dataset_replay_row_with_limits, store_error,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateStore;
use std::collections::BTreeMap;

struct Entry {
    replay: Replay,
    /// The row `version` and payload committed the last time this entry was
    /// validated. Every current producer knows those bytes; `None` would
    /// force the next sync to re-decode the row once.
    seen: Option<(u64, Vec<u8>)>,
}

/// The live, incrementally synced view of the `dataset-replay` namespace.
#[derive(Default)]
pub(crate) struct ReplayIndex {
    entries: BTreeMap<String, Entry>,
    /// Counts rows fully re-decoded by [`Self::sync`]. Per-index, so
    /// parallel tests never see each other's decodes.
    #[cfg(test)]
    decode_count: usize,
    /// Counts entries actually inserted or replaced by [`Self::sync`]
    /// (new or changed keys only). Zero on an unchanged sync proves it
    /// touches, clones or rebuilds nothing for rows that didn't change.
    #[cfg(test)]
    applied_count: usize,
}

impl ReplayIndex {
    pub(crate) fn get(&self, key: &str) -> Option<&Replay> {
        self.entries.get(key).map(|entry| &entry.replay)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Record a replay row this request just wrote at `version`/`payload`,
    /// which the store write confirmed as committed. The next `sync` can
    /// skip decoding it while its fingerprint stays unchanged.
    pub(crate) fn record_committed(
        &mut self,
        key: &str,
        version: u64,
        payload: &[u8],
        replay: Replay,
    ) {
        self.entries.insert(
            key.into(),
            Entry {
                replay,
                seen: Some((version, payload.to_vec())),
            },
        );
    }

    /// [`Self::record_committed`], keyed by `mutation`'s idempotency key.
    pub(crate) fn record_mutation(
        &mut self,
        mutation: &mainframe_env_host_api::Mutation,
        version: u64,
        payload: &[u8],
        replay: Replay,
    ) {
        self.record_committed(mutation.idempotency_key.as_str(), version, payload, replay);
    }

    /// Sync the index against `store` with exactly one `list_provider_state`
    /// call. A listed row is re-decoded only when its version, payload
    /// length or payload bytes differ from what was last committed for that
    /// key; every other row is left untouched in the index (no clone, no
    /// digest, no map rebuild). Keys no longer listed are dropped. Every row
    /// is checked, and every changed or new one decoded, before any change
    /// is applied, so an error leaves the index exactly as it was.
    pub(crate) fn sync(
        &mut self,
        store: &dyn ProviderStateStore,
        limits: DatasetLimits,
    ) -> Result<(), HostProblem> {
        let rows = store
            .list_provider_state("dataset-replay", limits.max_idempotency)
            .map_err(store_error)?;

        // Duplicate-key check over borrowed keys: a sort with no per-row
        // clone or map insertion, instead of probing a map being built.
        let mut keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(HostProblem::InfrastructureFailure);
        }

        let mut updates: Vec<(String, Entry)> = Vec::new();
        for row in &rows {
            let unchanged = self.entries.get(row.key.as_str()).is_some_and(|existing| {
                existing.seen.as_ref().is_some_and(|(version, payload)| {
                    *version == row.version
                        && payload.len() == row.payload.len()
                        && *payload == row.payload
                })
            });
            if !unchanged {
                let entry = self.decode_row(row, limits)?;
                updates.push((row.key.clone(), entry));
            }
        }

        // Every row validated: apply the staged changes. `updates` holds
        // only new or changed keys, so unchanged entries are never touched.
        #[cfg(test)]
        {
            self.applied_count += updates.len();
        }
        for (key, entry) in updates {
            self.entries.insert(key, entry);
        }
        // `self.entries` can only hold extra (now-unlisted) keys at this
        // point; it can never be short one, since every listed row was
        // either already present or just inserted. So an exact length match
        // against the duplicate-free `keys` proves nothing was pruned,
        // without a scan.
        if self.entries.len() != keys.len() {
            self.entries
                .retain(|key, _| keys.binary_search(&key.as_str()).is_ok());
        }
        Ok(())
    }

    fn decode_row(
        &mut self,
        row: &mainframe_env_store_api::ProviderStateRecord,
        limits: DatasetLimits,
    ) -> Result<Entry, HostProblem> {
        describe_dataset_replay_row_with_limits(row, limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let decoded =
            decode_replay(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        #[cfg(test)]
        {
            self.decode_count += 1;
        }
        Ok(Entry {
            replay: decoded,
            seen: Some((row.version, row.payload.clone())),
        })
    }

    #[cfg(test)]
    pub(crate) fn decode_count(&self) -> usize {
        self.decode_count
    }

    #[cfg(test)]
    pub(crate) fn applied_count(&self) -> usize {
        self.applied_count
    }

    /// A copy of the currently synced `Replay` values, for equivalence
    /// checks against a from-scratch decode of the same store.
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> BTreeMap<String, Replay> {
        self.entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.replay.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};

    fn limits() -> DatasetLimits {
        DatasetLimits::default()
    }

    fn put_pending_row(store: &dyn ProviderStateStore, key: &str) {
        // A pending replay row ("MEDR1" + 32-byte digest + result tag 0) is
        // the smallest payload `decode_replay` accepts.
        let mut payload = b"MEDR1".to_vec();
        payload.extend_from_slice(&[0u8; 32]);
        payload.push(0);
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset-replay".into(),
                    key: key.into(),
                    version: 1,
                    payload,
                },
                None,
            )
            .unwrap();
    }

    #[test]
    fn second_sync_with_no_external_change_decodes_nothing() {
        let store = MemoryStore::new(Default::default());
        put_pending_row(&store, "id-1");
        put_pending_row(&store, "id-2");
        put_pending_row(&store, "id-3");
        let mut index = ReplayIndex::default();
        index.sync(&store, limits()).unwrap();
        assert_eq!(index.decode_count(), 3);
        assert_eq!(index.applied_count(), 3);
        index.sync(&store, limits()).unwrap();
        assert_eq!(
            index.decode_count(),
            3,
            "a second sync with no external change must decode 0 additional rows"
        );
        assert_eq!(
            index.applied_count(),
            3,
            "a second sync with no external change must clone or rebuild 0 entries"
        );
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn resynced_row_at_same_version_and_length_with_different_bytes_is_detected() {
        let store = MemoryStore::new(Default::default());
        put_pending_row(&store, "id-1");
        let mut index = ReplayIndex::default();
        index.sync(&store, limits()).unwrap();
        assert_eq!(index.decode_count(), 1);

        // Delete and re-put at the same version (1), same payload length,
        // but different bytes. The identity check must not mistake this for
        // "unchanged" just because version and length match.
        store
            .delete_provider_state("dataset-replay", "id-1", 1)
            .unwrap();
        let mut payload = b"MEDR1".to_vec();
        payload.extend_from_slice(&[7u8; 32]);
        payload.push(0);
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset-replay".into(),
                    key: "id-1".into(),
                    version: 1,
                    payload,
                },
                None,
            )
            .unwrap();

        index.sync(&store, limits()).unwrap();
        assert_eq!(
            index.decode_count(),
            2,
            "a same-version, same-length, different-bytes re-put must still be re-decoded"
        );
    }

    #[test]
    fn failed_sync_leaves_the_index_unchanged() {
        let store = MemoryStore::new(Default::default());
        put_pending_row(&store, "id-1");
        put_pending_row(&store, "id-2");
        let mut index = ReplayIndex::default();
        index.sync(&store, limits()).unwrap();
        let before = index.snapshot();
        assert_eq!(before.len(), 2);
        // Corrupt id-2 at a new version so the next sync must re-decode it.
        let healthy = store
            .get_provider_state("dataset-replay", "id-2")
            .unwrap()
            .unwrap();
        let mut corrupt = healthy.clone();
        corrupt.version += 1;
        corrupt.payload.push(0);
        store
            .put_provider_state(corrupt, Some(healthy.version))
            .unwrap();
        assert_eq!(
            index.sync(&store, limits()),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            index.snapshot(),
            before,
            "a failed sync must not stage any of its partial decode work"
        );
    }

    #[test]
    fn sync_after_k_new_rows_decodes_exactly_k() {
        let store = MemoryStore::new(Default::default());
        put_pending_row(&store, "id-1");
        let mut index = ReplayIndex::default();
        index.sync(&store, limits()).unwrap();
        assert_eq!(index.decode_count(), 1);
        put_pending_row(&store, "id-2");
        put_pending_row(&store, "id-3");
        put_pending_row(&store, "id-4");
        index.sync(&store, limits()).unwrap();
        assert_eq!(
            index.decode_count(),
            4,
            "a sync after K new out-of-band rows must decode exactly K more"
        );
        assert_eq!(index.len(), 4);
    }
}
