//! Incremental in-memory sync for the `dataset-replay` provider-state index.
//!
//! `DatasetService` used to reload and fully decode every `dataset-replay`
//! row on each request (`fix(#194)`). `ReplayIndex` instead keeps one
//! `(version, payload digest)` fingerprint per key and only re-decodes a row
//! whose fingerprint changed since the last sync, matching `a5fbc43` in
//! every observable outcome.

use crate::service::{
    DatasetLimits, Replay, decode_replay, describe_dataset_replay_row_with_limits, store_error,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateStore;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Entry {
    replay: Replay,
    seen: Option<(u64, [u8; 32])>,
}

/// The live, incrementally synced view of the `dataset-replay` namespace.
#[derive(Default)]
pub(crate) struct ReplayIndex {
    entries: BTreeMap<String, Entry>,
    /// Counts rows fully re-decoded by [`Self::sync`]. Per-index, so
    /// parallel tests never see each other's decodes.
    #[cfg(test)]
    decode_count: usize,
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
        let digest: [u8; 32] = Sha256::digest(payload).into();
        self.entries.insert(
            key.into(),
            Entry {
                replay,
                seen: Some((version, digest)),
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
    /// call. A listed row is re-decoded only when its `(version, payload
    /// digest)` fingerprint differs from what was last seen; every other
    /// row is kept from the current index. Keys no longer listed are
    /// dropped. On any error the index is left unchanged.
    pub(crate) fn sync(
        &mut self,
        store: &dyn ProviderStateStore,
        limits: DatasetLimits,
    ) -> Result<(), HostProblem> {
        let rows = store
            .list_provider_state("dataset-replay", limits.max_idempotency)
            .map_err(store_error)?;
        let mut staged = BTreeMap::new();
        for row in rows {
            if staged.contains_key(&row.key) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let digest: [u8; 32] = Sha256::digest(&row.payload).into();
            let fingerprint = Some((row.version, digest));
            let entry = match self.entries.get(&row.key) {
                Some(existing) if existing.seen == fingerprint => existing.clone(),
                _ => self.decode_row(&row, digest, limits)?,
            };
            staged.insert(row.key, entry);
        }
        self.entries = staged;
        Ok(())
    }

    fn decode_row(
        &mut self,
        row: &mainframe_env_store_api::ProviderStateRecord,
        digest: [u8; 32],
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
            seen: Some((row.version, digest)),
        })
    }

    #[cfg(test)]
    pub(crate) fn decode_count(&self) -> usize {
        self.decode_count
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
        index.sync(&store, limits()).unwrap();
        assert_eq!(
            index.decode_count(),
            3,
            "a second sync with no external change must decode 0 additional rows"
        );
        assert_eq!(index.len(), 3);
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
