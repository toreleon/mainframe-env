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

//! The complete physical replay snapshot is still read and compared on every
//! sync. A sorted borrowed-row merge classifies unchanged keys without a fresh
//! tree lookup per row; decoding and publication retain physical scan order.
//!
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
    /// Test-only witness for exact unchanged physical snapshots. No production
    /// counter or alternate store authority is introduced.
    #[cfg(test)]
    unchanged_snapshot_count: usize,
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

        // Exact key-ordered equality proves the complete physical key set is
        // unchanged, including every payload byte. Keep the physical scan above:
        // any missing fingerprint, reordered/replaced/duplicate key or changed
        // bytes falls through to the original fully staged validation below.
        if rows.len() == self.entries.len()
            && rows
                .iter()
                .zip(&self.entries)
                .all(|(row, (key, existing))| {
                    row.key.as_str() == key.as_str()
                        && existing.seen.as_ref().is_some_and(|(version, payload)| {
                            *version == row.version
                                && payload.len() == row.payload.len()
                                && *payload == row.payload
                        })
                })
        {
            #[cfg(test)]
            {
                self.unchanged_snapshot_count += 1;
            }
            return Ok(());
        }

        // Sort borrowed rows once for duplicate detection and a linear merge
        // with the committed index. Preserve physical scan order for decoding:
        // the first invalid changed row must still be the first one observed.
        let mut ordered: Vec<_> = rows.iter().enumerate().collect();
        ordered.sort_unstable_by(|(_, left), (_, right)| left.key.cmp(&right.key));
        if ordered
            .windows(2)
            .any(|pair| pair[0].1.key == pair[1].1.key)
        {
            return Err(HostProblem::InfrastructureFailure);
        }

        let mut unchanged = vec![false; rows.len()];
        {
            let mut committed = self.entries.iter().peekable();
            for (index, row) in &ordered {
                while committed
                    .peek()
                    .is_some_and(|(key, _)| key.as_str() < row.key.as_str())
                {
                    committed.next();
                }
                if let Some((key, existing)) = committed.peek()
                    && key.as_str() == row.key.as_str()
                {
                    unchanged[*index] = existing.seen.as_ref().is_some_and(|(version, payload)| {
                        *version == row.version
                            && payload.len() == row.payload.len()
                            && *payload == row.payload
                    });
                }
            }
        }

        let mut updates: Vec<(String, Entry)> = Vec::new();
        for (row, unchanged) in rows.iter().zip(unchanged) {
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
        // against the duplicate-free `ordered` rows proves nothing was pruned,
        // without a scan.
        if self.entries.len() != ordered.len() {
            self.entries.retain(|key, _| {
                ordered
                    .binary_search_by(|(_, row)| row.key.cmp(key))
                    .is_ok()
            });
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

    struct SnapshotPage {
        rows: Vec<ProviderStateRecord>,
        fail: bool,
        calls: std::sync::Mutex<Vec<(String, usize)>>,
    }

    impl SnapshotPage {
        fn new(rows: Vec<ProviderStateRecord>) -> Self {
            Self {
                rows,
                fail: false,
                calls: Default::default(),
            }
        }
    }

    impl mainframe_env_store_api::AuditSink for SnapshotPage {
        fn record_audit(
            &self,
            _audit: mainframe_env_execution_api::AuditRecord,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not write audits");
        }

        fn audit_records(
            &self,
            _execution: &mainframe_env_execution_api::ExecutionId,
            _start: u64,
            _max: usize,
        ) -> Result<
            Vec<mainframe_env_execution_api::AuditRecord>,
            mainframe_env_store_api::StoreError,
        > {
            panic!("replay sync must not read audit rows");
        }
    }

    impl ProviderStateStore for SnapshotPage {
        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, mainframe_env_store_api::StoreError> {
            self.calls.lock().unwrap().push((namespace.into(), max));
            if self.fail {
                return Err(mainframe_env_store_api::StoreError::Conflict);
            }
            Ok(self.rows.clone())
        }

        fn get_provider_state(
            &self,
            _namespace: &str,
            _key: &str,
        ) -> Result<Option<ProviderStateRecord>, mainframe_env_store_api::StoreError> {
            panic!("replay sync must not replace the complete physical scan with gets");
        }

        fn put_provider_state(
            &self,
            _record: ProviderStateRecord,
            _expected: Option<u64>,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not put rows");
        }

        fn delete_provider_state(
            &self,
            _namespace: &str,
            _key: &str,
            _expected: u64,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not delete physical rows");
        }

        fn move_provider_state(
            &self,
            _record: ProviderStateRecord,
            _old_key: &str,
            _expected: u64,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not move rows");
        }

        fn put_provider_states_atomic(
            &self,
            _writes: Vec<mainframe_env_store_api::ProviderStateWrite>,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not publish rows");
        }

        fn mutate_provider_states_atomic(
            &self,
            _mutations: Vec<mainframe_env_store_api::ProviderStateMutation>,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            panic!("replay sync must not mutate rows");
        }
    }

    type CommittedSnapshot = BTreeMap<String, (Replay, Option<(u64, Vec<u8>)>)>;

    fn committed_snapshot(index: &ReplayIndex) -> CommittedSnapshot {
        index
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), (entry.replay.clone(), entry.seen.clone())))
            .collect()
    }

    fn pending_record(key: &str, digest: u8) -> ProviderStateRecord {
        let mut payload = b"MEDR1".to_vec();
        payload.extend_from_slice(&[digest; 32]);
        payload.push(0);
        ProviderStateRecord {
            namespace: "dataset-replay".into(),
            key: key.into(),
            version: 1,
            payload,
        }
    }

    // Independent full-reload oracle: no incremental classification or merge.
    fn full_reload(rows: &[ProviderStateRecord]) -> BTreeMap<String, Replay> {
        let mut expected = BTreeMap::new();
        for row in rows {
            describe_dataset_replay_row_with_limits(row, limits()).unwrap();
            assert!(
                expected
                    .insert(row.key.clone(), decode_replay(&row.payload).unwrap())
                    .is_none()
            );
        }
        expected
    }

    #[test]
    fn merge_matches_full_reload_for_interleaved_unordered_changes() {
        let initial: Vec<_> = (0..1_024)
            .map(|key| pending_record(&format!("replay-{key:04}"), 0))
            .collect();
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(initial.clone()), limits())
            .unwrap();
        let mut next = initial;
        next.retain(|row| row.key[7..].parse::<usize>().unwrap() % 7 != 0);
        for row in &mut next {
            if row.key[7..].parse::<usize>().unwrap() % 11 == 0 {
                // Same version and length, different valid digest bytes.
                row.payload[5] = 7;
            }
            if row.key[7..].parse::<usize>().unwrap() % 13 == 0 {
                // A valid pending-to-resolved version and payload-length change.
                row.version = 2;
                row.payload[37] = 1;
                row.payload.extend_from_slice(&2_u64.to_be_bytes());
            }
        }
        next.extend((1_024..1_055).map(|key| pending_record(&format!("replay-{key:04}"), 9)));
        next.reverse();
        next.rotate_left(117);
        let expected = full_reload(&next);
        let page = SnapshotPage::new(next);
        index.sync(&page, limits()).unwrap();
        assert_eq!(index.snapshot(), expected);
        assert_eq!(
            *page.calls.lock().unwrap(),
            vec![("dataset-replay".into(), limits().max_idempotency)]
        );
        // Re-put a physically deleted identity at its old version with new bytes.
        let mut reinserted = page.rows.clone();
        reinserted.push(pending_record("replay-0000", 17));
        let expected = full_reload(&reinserted);
        index
            .sync(&SnapshotPage::new(reinserted), limits())
            .unwrap();
        assert_eq!(index.snapshot(), expected);
    }

    #[test]
    fn merge_unchanged_reverse_page_decodes_and_applies_nothing() {
        let rows: Vec<_> = (0..17)
            .map(|key| pending_record(&format!("stable-{key:02}"), key))
            .collect();
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(rows.clone()), limits())
            .unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let page = SnapshotPage::new(rows.into_iter().rev().collect());
        index.sync(&page, limits()).unwrap();
        assert_eq!(committed_snapshot(&index), before);
        assert_eq!(index.decode_count(), decodes);
        assert_eq!(index.applied_count, applied);
        assert_eq!(page.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn merge_duplicate_page_refuses_before_decode_and_keeps_fingerprints() {
        let mut index = ReplayIndex::default();
        index
            .sync(
                &SnapshotPage::new(vec![pending_record("keep", 1)]),
                limits(),
            )
            .unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let mut corrupt = pending_record("duplicate", 0);
        corrupt.payload.push(0);
        let page = SnapshotPage::new(vec![
            pending_record("z-valid-first", 2),
            corrupt,
            pending_record("duplicate", 3),
        ]);
        assert_eq!(
            index.sync(&page, limits()),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(committed_snapshot(&index), before);
        assert_eq!(index.decode_count(), decodes);
        assert_eq!(index.applied_count, applied);
    }

    #[test]
    fn merge_late_error_keeps_physical_decode_order_and_all_prior_state() {
        let mut index = ReplayIndex::default();
        index
            .sync(
                &SnapshotPage::new(vec![pending_record("retained", 1)]),
                limits(),
            )
            .unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        // Sorting would encounter a-invalid before z-valid; physical order must win.
        let mut bad = pending_record("a-invalid", 2);
        bad.namespace = "wrong-namespace".into();
        let page = SnapshotPage::new(vec![pending_record("z-valid", 3), bad]);
        assert_eq!(
            index.sync(&page, limits()),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(index.decode_count(), decodes + 1);
        assert_eq!(index.applied_count, applied);
        assert_eq!(committed_snapshot(&index), before);
        let retry = SnapshotPage::new(vec![
            pending_record("z-valid", 3),
            pending_record("a-invalid", 2),
        ]);
        index.sync(&retry, limits()).unwrap();
        assert_eq!(index.decode_count(), decodes + 3);
        assert_eq!(index.applied_count, applied + 2);
        assert_eq!(index.snapshot(), full_reload(&retry.rows));
    }

    #[test]
    fn merge_store_error_keeps_index_and_original_single_scan_contract() {
        let mut index = ReplayIndex::default();
        index
            .sync(
                &SnapshotPage::new(vec![pending_record("retained", 1)]),
                limits(),
            )
            .unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let mut page = SnapshotPage::new(vec![pending_record("unobserved", 2)]);
        page.fail = true;
        assert_eq!(
            index.sync(&page, limits()),
            Err(store_error(mainframe_env_store_api::StoreError::Conflict))
        );
        assert_eq!(committed_snapshot(&index), before);
        assert_eq!(index.decode_count(), decodes);
        assert_eq!(index.applied_count, applied);
        assert_eq!(
            *page.calls.lock().unwrap(),
            vec![("dataset-replay".into(), limits().max_idempotency)]
        );
    }

    #[test]
    fn merge_missing_fingerprint_revalidates_then_empty_page_prunes_all() {
        let rows = vec![pending_record("one", 1), pending_record("two", 2)];
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(rows.clone()), limits())
            .unwrap();
        index.entries.get_mut("two").unwrap().seen = None;
        let decodes = index.decode_count();
        let applied = index.applied_count;
        index
            .sync(&SnapshotPage::new(rows.clone()), limits())
            .unwrap();
        assert_eq!(index.snapshot(), full_reload(&rows));
        assert_eq!(index.decode_count(), decodes + 1);
        assert_eq!(index.applied_count, applied + 1);
        assert!(index.entries["two"].seen.is_some());
        index
            .sync(&SnapshotPage::new(Vec::new()), limits())
            .unwrap();
        assert_eq!(index.len(), 0);
        assert!(committed_snapshot(&index).is_empty());
    }

    #[test]
    fn exact_snapshot_unchanged_keeps_authority_and_complete_physical_scan() {
        let rows = vec![pending_record("one", 1), pending_record("two", 2)];
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(rows.clone()), limits())
            .unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let page = SnapshotPage::new(rows);
        index.sync(&page, limits()).unwrap();
        assert_eq!(index.unchanged_snapshot_count, 1);
        assert_eq!(committed_snapshot(&index), before);
        assert_eq!(index.snapshot(), full_reload(&page.rows));
        assert_eq!(index.decode_count(), decodes);
        assert_eq!(index.applied_count, applied);
        assert_eq!(
            *page.calls.lock().unwrap(),
            vec![("dataset-replay".into(), limits().max_idempotency)]
        );
        let mut empty = ReplayIndex::default();
        let page = SnapshotPage::new(Vec::new());
        empty.sync(&page, limits()).unwrap();
        assert_eq!(empty.unchanged_snapshot_count, 1);
        assert!(committed_snapshot(&empty).is_empty());
        assert_eq!(page.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn exact_snapshot_same_count_replacement_publishes_new_key_and_prunes_old_key() {
        let initial = vec![
            pending_record("a", 1),
            pending_record("b", 2),
            pending_record("c", 3),
        ];
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(initial.clone()), limits())
            .unwrap();
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let page = SnapshotPage::new(vec![
            initial[0].clone(),
            initial[1].clone(),
            pending_record("d", 4),
        ]);
        index.sync(&page, limits()).unwrap();
        assert_eq!(index.unchanged_snapshot_count, 0);
        assert_eq!(index.len(), 3);
        assert!(index.get("c").is_none());
        assert!(index.get("d").is_some());
        assert_eq!(index.snapshot(), full_reload(&page.rows));
        assert_eq!(index.decode_count(), decodes + 1);
        assert_eq!(index.applied_count, applied + 1);
        assert_eq!(page.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn exact_snapshot_late_digest_byte_at_same_version_and_length_is_revalidated() {
        let rows = vec![pending_record("a", 1), pending_record("z", 2)];
        let mut index = ReplayIndex::default();
        index
            .sync(&SnapshotPage::new(rows.clone()), limits())
            .unwrap();
        let before = committed_snapshot(&index);
        let before_replays = index.snapshot();
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let mut changed = rows;
        // Byte36 is the last digest byte, not the result tag. Keep version,
        // length and every earlier byte unchanged in the final physical row.
        changed[1].payload[36] ^= 0x5a;
        assert_eq!(changed[1].version, 1);
        assert_eq!(
            changed[1].payload.len(),
            before["z"].1.as_ref().unwrap().1.len()
        );
        let page = SnapshotPage::new(changed);
        index.sync(&page, limits()).unwrap();
        assert_eq!(index.unchanged_snapshot_count, 0);
        assert_eq!(index.snapshot(), full_reload(&page.rows));
        assert_ne!(index.snapshot(), before_replays);
        assert_eq!(index.decode_count(), decodes + 1);
        assert_eq!(index.applied_count, applied + 1);
        assert_ne!(committed_snapshot(&index), before);
    }

    #[test]
    fn exact_snapshot_same_count_duplicate_refuses_before_any_physical_decode() {
        let initial: Vec<_> = (0..4)
            .map(|key| pending_record(&format!("retained-{key}"), key))
            .collect();
        let mut index = ReplayIndex::default();
        index.sync(&SnapshotPage::new(initial), limits()).unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let mut malformed = pending_record("duplicate", 0);
        malformed.payload.push(0);
        // Both snapshots retain the same count as the committed index. Cover
        // malformed-first, then valid-first: decoding that valid row before
        // duplicate refusal would increment the successful decode witness.
        let malformed_first = vec![
            malformed.clone(),
            pending_record("duplicate", 3),
            pending_record("valid-one", 4),
            pending_record("valid-two", 5),
        ];
        let valid_first = vec![
            pending_record("valid-one", 4),
            malformed,
            pending_record("duplicate", 3),
            pending_record("valid-two", 5),
        ];
        for rows in [malformed_first, valid_first] {
            let page = SnapshotPage::new(rows);
            assert_eq!(
                index.sync(&page, limits()),
                Err(HostProblem::InfrastructureFailure)
            );
            assert_eq!(index.unchanged_snapshot_count, 0);
            assert_eq!(index.decode_count(), decodes);
            assert_eq!(index.applied_count, applied);
            assert_eq!(committed_snapshot(&index), before);
            assert_eq!(page.calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn exact_snapshot_same_count_late_failure_keeps_physical_order_and_prior_state() {
        let initial = vec![
            pending_record("retained-a", 1),
            pending_record("retained-b", 2),
        ];
        let mut index = ReplayIndex::default();
        index.sync(&SnapshotPage::new(initial), limits()).unwrap();
        let before = committed_snapshot(&index);
        let decodes = index.decode_count();
        let applied = index.applied_count;
        let mut malformed = pending_record("a-invalid", 3);
        malformed.namespace = "wrong-namespace".into();
        let page = SnapshotPage::new(vec![pending_record("z-valid", 4), malformed]);
        assert_eq!(
            index.sync(&page, limits()),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(index.unchanged_snapshot_count, 0);
        assert_eq!(index.decode_count(), decodes + 1);
        assert_eq!(index.applied_count, applied);
        assert_eq!(committed_snapshot(&index), before);
        let page = SnapshotPage::new(vec![
            pending_record("z-valid", 4),
            pending_record("a-invalid", 3),
        ]);
        index.sync(&page, limits()).unwrap();
        assert_eq!(index.unchanged_snapshot_count, 0);
        assert_eq!(index.snapshot(), full_reload(&page.rows));
        assert_eq!(index.decode_count(), decodes + 3);
        assert_eq!(index.applied_count, applied + 2);
    }
}
