//! Live service checkpoint projection, distinct from cold restart/backout.

use super::restart::SnapshotMessage;
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{self, Write};

pub(crate) mod rows;

impl MqDeliveryKernel {
    pub const LIVE_CHECKPOINT_SCHEMA: &str = "mainframe-env.mq-delivery-live@1";

    /// Retains queued and pending persistent AND nonpersistent messages, UOW
    /// decisions and browse cursors for a live, fenced service resume. Expired
    /// entries are purged at the stored tick on a private candidate; an empty
    /// pending unit remains pending until explicitly finalized.
    ///
    /// This is an internal storage projection, not MQ wire data or a store.
    /// The service must bind it to catalog generation, run/connection identity,
    /// clock, SAF, canonical effect/replay records and atomic row CAS. It must
    /// never resume an older checkpoint after a newer fenced publication.
    /// For cold restart's persistent-only backout policy use `encode`/`decode`.
    pub fn encode_live_checkpoint(&self) -> Result<Vec<u8>, MqDeliveryError> {
        encode_projection(&self.live_projection()?, self.limits.snapshot_bytes)
    }

    fn live_projection(&self) -> Result<Checkpoint, MqDeliveryError> {
        let mut candidate = self.clone();
        candidate.advance_tick(self.tick)?;
        candidate.check_bounds()?;
        Ok(Checkpoint {
            schema_version: Self::LIVE_CHECKPOINT_SCHEMA.into(),
            manager: candidate.manager.clone(),
            default_persistent: candidate.default_persistence == MqPersistence::Persistent,
            tick: candidate.tick,
            next_id: candidate.next_id,
            next_cursor: candidate.next_cursor,
            queues: candidate
                .queues
                .iter()
                .map(|(name, entries)| {
                    Ok(LiveQueue {
                        name: name.clone(),
                        messages: entries
                            .iter()
                            .map(LiveEntry::from_entry)
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, MqDeliveryError>>()?,
            pending: candidate
                .pending
                .iter()
                .map(|(unit, operations)| {
                    let mut operations: Vec<_> = operations
                        .iter()
                        .map(|operation| {
                            let (queue, entry, put) = match operation {
                                Pending::Put { queue, entry } => (queue, entry, true),
                                Pending::Get { queue, entry } => (queue, entry, false),
                            };
                            Ok(LiveOperation {
                                queue: queue.clone(),
                                put,
                                entry: LiveEntry::from_entry(entry)?,
                            })
                        })
                        .collect::<Result<_, MqDeliveryError>>()?;
                    // Transitions use entry identity order; historical call order
                    // is not an additional log or replay authority.
                    operations.sort_by_key(|operation| operation.entry.id);
                    Ok(LiveUnit {
                        unit: *unit,
                        operations,
                    })
                })
                .collect::<Result<_, MqDeliveryError>>()?,
            finalized: candidate
                .finalized
                .iter()
                .map(|(unit, committed)| LiveFinalized {
                    unit: *unit,
                    committed: *committed,
                })
                .collect(),
            cursors: candidate
                .cursors
                .iter()
                .map(|(token, cursor)| LiveCursor {
                    token: *token,
                    queue: cursor.queue.clone(),
                    entry_id: cursor.entry_id,
                })
                .collect(),
        })
    }

    /// Strict candidate-only restore: no current authority is mutated on error.
    /// Limits may narrow on restore, never exceed the frozen product ceilings.
    /// The exact queue-manager/local/model-instance queue set and default
    /// persistence must match. Aliases/remote queues cannot replace queue rows.
    /// Service generation/CAS fencing supplies identity beyond these queues.
    pub fn decode_live_checkpoint(
        bytes: &[u8],
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        default_persistence: MqPersistence,
    ) -> Result<Self, MqDeliveryError> {
        let kernel = Self::new(catalog, limits, message_limits, default_persistence)?;
        if bytes.len() > limits.snapshot_bytes {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        let snapshot: Checkpoint =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        Self::restore_projection(snapshot, kernel)
    }

    fn restore_projection(snapshot: Checkpoint, mut kernel: Self) -> Result<Self, MqDeliveryError> {
        let limits = kernel.limits;
        let default_persistence = kernel.default_persistence;
        if snapshot.schema_version != Self::LIVE_CHECKPOINT_SCHEMA {
            return Err(MqDeliveryError::UnsupportedSchema);
        }
        if snapshot.manager != kernel.manager
            || snapshot.default_persistent != (default_persistence == MqPersistence::Persistent)
            || snapshot.next_id == 0
            || snapshot.next_cursor == 0
            || snapshot.queues.len() != kernel.queues.len()
            || snapshot.pending.len() > limits.pending_operations
            || snapshot.finalized.len() > limits.finalized_units
            || snapshot.cursors.len() > limits.cursors
        {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        kernel.tick = snapshot.tick;
        kernel.next_id = snapshot.next_id;
        kernel.next_cursor = snapshot.next_cursor;
        let mut ids = BTreeMap::new();
        let expected: Vec<_> = kernel.queues.keys().cloned().collect();
        for (row, expected) in snapshot.queues.into_iter().zip(expected) {
            if row.name != expected || row.messages.len() > limits.depth_per_queue {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            let mut prior = 0;
            let mut entries = Vec::new();
            for item in row.messages {
                let entry = restore_entry(item, &kernel, &row.name, &mut prior, &mut ids)?;
                entries.push(entry);
            }
            kernel.queues.insert(row.name, entries);
        }
        let mut prior_unit = 0;
        let mut operation_count = 0usize;
        let mut pending_put_ids = BTreeSet::new();
        for row in snapshot.pending {
            if row.unit <= prior_unit {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            prior_unit = row.unit;
            operation_count = operation_count
                .checked_add(row.operations.len())
                .ok_or(MqDeliveryError::CorruptSnapshot)?;
            if operation_count > limits.pending_operations {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            let mut prior = 0;
            let mut operations = Vec::new();
            for operation in row.operations {
                if !kernel.queues.contains_key(&operation.queue) {
                    return Err(MqDeliveryError::CorruptSnapshot);
                }
                let entry = restore_entry(
                    operation.entry,
                    &kernel,
                    &operation.queue,
                    &mut prior,
                    &mut ids,
                )?;
                if operation.put {
                    pending_put_ids.insert(entry.id);
                }
                operations.push(if operation.put {
                    Pending::Put {
                        queue: operation.queue,
                        entry,
                    }
                } else {
                    Pending::Get {
                        queue: operation.queue,
                        entry,
                    }
                });
            }
            kernel.pending.insert(row.unit, operations);
        }
        let mut prior = 0;
        for row in snapshot.finalized {
            if row.unit <= prior || kernel.pending.contains_key(&row.unit) {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            prior = row.unit;
            kernel.finalized.insert(row.unit, row.committed);
        }
        let mut prior = 0;
        for row in snapshot.cursors {
            if row.token <= prior
                || row.token >= kernel.next_cursor
                || row.entry_id == 0
                || row.entry_id >= kernel.next_id
                || !kernel.queues.contains_key(&row.queue)
                || pending_put_ids.contains(&row.entry_id)
                || ids
                    .get(&row.entry_id)
                    .is_some_and(|queue| queue != &row.queue)
            {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            // A removed/expired entry leaves a legitimate stale browse position.
            prior = row.token;
            kernel.cursors.insert(
                row.token,
                Cursor {
                    queue: row.queue,
                    entry_id: row.entry_id,
                },
            );
        }
        kernel
            .check_bounds()
            .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        Ok(kernel)
    }

    /// Explicit live recovery policy: atomically back out every pending unit,
    /// including nonpersistent gets, using the same authority as `backout`.
    /// Existing finalized decisions and cursor epochs remain unchanged. Expiry
    /// uses the current trusted tick. This does not compensate committed work.
    ///
    /// Decisions are retained for this kernel's entire lifetime: there is no
    /// implicit eviction/reuse. Finalized-unit saturation rejects this complete
    /// transition unchanged; the service must preserve replay references before
    /// replacing/pruning an authority under its own retention contract.
    pub fn recover_backout(&mut self) -> Result<(), MqDeliveryError> {
        if self
            .finalized
            .len()
            .checked_add(self.pending.len())
            .is_none_or(|count| count > self.limits.finalized_units)
        {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        let mut candidate = self.clone();
        for unit in self.pending.keys() {
            candidate.apply_backout(*unit)?;
        }
        candidate.check_bounds()?;
        *self = candidate;
        Ok(())
    }
}

fn encode_projection(snapshot: &Checkpoint, limit: usize) -> Result<Vec<u8>, MqDeliveryError> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
        full: false,
    };
    if serde_json::to_writer(&mut writer, snapshot).is_err() {
        return Err(if writer.full {
            MqDeliveryError::ResourceExhausted
        } else {
            MqDeliveryError::CorruptSnapshot
        });
    }
    Ok(writer.bytes)
}

fn restore_entry(
    item: LiveEntry,
    kernel: &MqDeliveryKernel,
    queue: &MqObjectName,
    prior: &mut u64,
    ids: &mut BTreeMap<u64, MqObjectName>,
) -> Result<Entry, MqDeliveryError> {
    if item.id <= *prior
        || item.id >= kernel.next_id
        || item.expires_at.is_some_and(|expiry| expiry <= kernel.tick)
        || ids.insert(item.id, queue.clone()).is_some()
    {
        return Err(MqDeliveryError::CorruptSnapshot);
    }
    *prior = item.id;
    let mut message = item.message.into_message()?;
    message.descriptor.persistence = if item.persistent {
        MqPersistence::Persistent
    } else {
        MqPersistence::NonPersistent
    };
    message
        .validate(kernel.message_limits)
        .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
    if message.descriptor.identifiers.message_id.is_none() {
        return Err(MqDeliveryError::CorruptSnapshot);
    }
    validate_supported_message(&message).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
    // Queue subsets can have holes after selection/expiry. Per-entry shape is
    // checked, without inventing missing handle-owned group history.
    let entry = Entry {
        id: item.id,
        message,
        expires_at: item.expires_at,
    };
    validate_group_and_segment(&[], &entry, true).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
    match (entry.message.descriptor.expiry, entry.expires_at) {
        (MqExpiry::Unlimited, None) => {}
        (MqExpiry::RelativeHostTicks(duration), Some(expiry))
            if expiry
                .checked_sub(duration)
                .is_some_and(|created| created <= kernel.tick) => {}
        _ => return Err(MqDeliveryError::CorruptSnapshot),
    }
    Ok(entry)
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    full: bool,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.full = true;
            return Err(io::Error::other("checkpoint byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema_version: String,
    manager: MqObjectName,
    default_persistent: bool,
    tick: u64,
    next_id: u64,
    next_cursor: u64,
    queues: Vec<LiveQueue>,
    pending: Vec<LiveUnit>,
    finalized: Vec<LiveFinalized>,
    cursors: Vec<LiveCursor>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveQueue {
    name: MqObjectName,
    messages: Vec<LiveEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveUnit {
    unit: u64,
    operations: Vec<LiveOperation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveOperation {
    queue: MqObjectName,
    put: bool,
    entry: LiveEntry,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveEntry {
    id: u64,
    #[serde(deserialize_with = "required_option")]
    expires_at: Option<u64>,
    persistent: bool,
    #[serde(with = "super::restart::LiveMessage")]
    message: SnapshotMessage,
}

fn required_option<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Option::deserialize(deserializer)
}

impl LiveEntry {
    fn from_entry(entry: &Entry) -> Result<Self, MqDeliveryError> {
        Ok(Self {
            id: entry.id,
            expires_at: entry.expires_at,
            persistent: entry.message.descriptor.persistence == MqPersistence::Persistent,
            message: SnapshotMessage::from_live_message(&entry.message)?,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveFinalized {
    unit: u64,
    committed: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveCursor {
    token: u64,
    queue: MqObjectName,
    entry_id: u64,
}

#[cfg(test)]
mod tests {
    use super::super::tests::{catalog, kernel, message, name, request};
    use super::*;
    use serde_json::{Value, json};

    fn restore(
        bytes: &[u8],
        objects: &MqObjectCatalog,
    ) -> Result<MqDeliveryKernel, MqDeliveryError> {
        MqDeliveryKernel::decode_live_checkpoint(
            bytes,
            objects,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
    }

    fn remove(
        kernel: &mut MqDeliveryKernel,
        objects: &MqObjectCatalog,
        unit: Option<u64>,
    ) -> MqDeliveryGet {
        kernel
            .get(
                objects,
                &name("A"),
                &request(MqGetMode::Remove, 100, MqTruncation::Reject),
                unit,
            )
            .unwrap()
    }

    fn mixed(objects: &MqObjectCatalog) -> MqDeliveryKernel {
        let mut state = kernel(objects);
        state
            .put_one(objects, &name("A"), message(b"old"), None)
            .unwrap();
        state
            .put_one(objects, &name("A"), message(b"keep"), None)
            .unwrap();
        state
            .put_one(objects, &name("A"), message(b"new"), Some(7))
            .unwrap();
        assert_eq!(
            remove(&mut state, objects, Some(7)).message.unwrap().body,
            b"old"
        );
        state
    }

    #[test]
    fn pending_put_get_roundtrip_commit_replay_never_duplicates_delivery() {
        let objects = catalog();
        let state = mixed(&objects);
        let bytes = state.encode_live_checkpoint().unwrap();
        let mut resumed = restore(&bytes, &objects).unwrap();
        assert_eq!(resumed.encode_live_checkpoint().unwrap(), bytes);
        assert_eq!(resumed.unit_outcome(7), MqDeliveryOutcome::Pending);
        assert_eq!(resumed.depth(&name("A")), Some(1));
        assert_eq!(resumed.commit(7), Ok(MqDeliveryOutcome::Accepted));
        resumed = restore(&resumed.encode_live_checkpoint().unwrap(), &objects).unwrap();
        let before = resumed.clone();
        assert_eq!(resumed.commit(7), Ok(MqDeliveryOutcome::DuplicatePossible));
        assert_eq!(resumed.backout(7), Ok(MqDeliveryOutcome::DuplicatePossible));
        assert_eq!(resumed, before);
        assert_eq!(
            remove(&mut resumed, &objects, None).message.unwrap().body,
            b"keep"
        );
        assert_eq!(
            remove(&mut resumed, &objects, None).message.unwrap().body,
            b"new"
        );
        assert_eq!(
            remove(&mut resumed, &objects, None).disposition,
            MqGetDisposition::NoMessage
        );
        let before = resumed.clone();
        assert_eq!(
            resumed.put_one(&objects, &name("A"), message(b"replay"), Some(7)),
            Err(MqDeliveryError::InvalidUnit)
        );
        assert_eq!(resumed, before);
    }

    #[test]
    fn pending_put_get_backout_replay_restores_original_order_once() {
        let objects = catalog();
        let mut resumed =
            restore(&mixed(&objects).encode_live_checkpoint().unwrap(), &objects).unwrap();
        assert_eq!(resumed.backout(7), Ok(MqDeliveryOutcome::Rejected));
        resumed = restore(&resumed.encode_live_checkpoint().unwrap(), &objects).unwrap();
        let before = resumed.clone();
        assert_eq!(resumed.backout(7), Ok(MqDeliveryOutcome::Rejected));
        assert_eq!(resumed.commit(7), Ok(MqDeliveryOutcome::UnknownOutcome));
        assert_eq!(resumed, before);
        assert_eq!(
            remove(&mut resumed, &objects, None).message.unwrap().body,
            b"old"
        );
        assert_eq!(
            remove(&mut resumed, &objects, None).message.unwrap().body,
            b"keep"
        );
        assert_eq!(
            remove(&mut resumed, &objects, None).disposition,
            MqGetDisposition::NoMessage
        );
    }

    #[test]
    fn own_put_get_empty_unit_remains_pending_and_bounded() {
        let objects = catalog();
        let mut state = kernel(&objects);
        state.limits.pending_operations = 1;
        state
            .put_one(&objects, &name("A"), message(b"own"), Some(3))
            .unwrap();
        assert_eq!(
            remove(&mut state, &objects, Some(3)).message.unwrap().body,
            b"own"
        );
        let bytes = state.encode_live_checkpoint().unwrap();
        let mut resumed = MqDeliveryKernel::decode_live_checkpoint(
            &bytes,
            &objects,
            state.limits,
            state.message_limits,
            state.default_persistence,
        )
        .unwrap();
        assert_eq!(resumed.unit_outcome(3), MqDeliveryOutcome::Pending);
        let before = resumed.clone();
        assert_eq!(
            resumed.put_one(&objects, &name("A"), message(b"other"), Some(4)),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(resumed, before);
        assert_eq!(resumed.commit(3), Ok(MqDeliveryOutcome::Accepted));
        assert_eq!(resumed.depth(&name("A")), Some(0));
    }

    #[test]
    fn live_nonpersistent_and_expiry_policy_differs_explicitly_from_cold_restart() {
        let objects = catalog();
        let mut state = kernel(&objects);
        let mut volatile = message(b"volatile");
        volatile.descriptor.persistence = MqPersistence::NonPersistent;
        state
            .put_one(&objects, &name("A"), volatile.clone(), None)
            .unwrap();
        remove(&mut state, &objects, Some(1));
        state
            .put_one(&objects, &name("A"), volatile, Some(1))
            .unwrap();
        let mut expired = message(b"expired");
        expired.descriptor.expiry = MqExpiry::RelativeHostTicks(1);
        state
            .put_one(&objects, &name("B"), expired, Some(2))
            .unwrap();
        let mut expires = message(b"later");
        expires.descriptor.expiry = MqExpiry::RelativeHostTicks(2);
        state
            .put_one(&objects, &name("A"), expires, Some(2))
            .unwrap();
        state.advance_tick(1).unwrap();
        let before = state.clone();
        let mut resumed = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        assert_eq!(state, before);
        assert_eq!(resumed.pending[&2].len(), 1);
        assert_eq!(resumed.unit_outcome(1), MqDeliveryOutcome::Pending);
        resumed.recover_backout().unwrap();
        assert_eq!(
            remove(&mut resumed, &objects, None).message.unwrap().body,
            b"volatile"
        );
        assert_eq!(resumed.unit_outcome(2), MqDeliveryOutcome::Rejected);
        let cold = MqDeliveryKernel::decode(
            &state.encode().unwrap(),
            &objects,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        assert_eq!(cold.depth(&name("A")), Some(0));
        assert_eq!(cold.unit_outcome(1), MqDeliveryOutcome::UnknownOutcome);
        let mut resumed = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        resumed.advance_tick(2).unwrap();
        assert_eq!(resumed.unit_outcome(2), MqDeliveryOutcome::Pending);
        resumed.commit(2).unwrap();
        assert_eq!(resumed.depth(&name("A")), Some(0));
        resumed.backout(1).unwrap();
        assert_eq!(resumed.depth(&name("A")), Some(1));
    }

    #[test]
    fn live_cursor_positions_and_epochs_survive_removed_and_expired_entries() {
        let objects = catalog();
        let mut state = kernel(&objects);
        state
            .put_one(&objects, &name("A"), message(b"first"), None)
            .unwrap();
        let cursor = state
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        remove(&mut state, &objects, Some(8));
        let mut resumed = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        resumed.backout(8).unwrap();
        assert_eq!(
            resumed
                .get(
                    &objects,
                    &name("A"),
                    &request(
                        MqGetMode::RemoveUnderCursor { cursor },
                        100,
                        MqTruncation::Reject
                    ),
                    None
                )
                .unwrap()
                .message
                .unwrap()
                .body,
            b"first"
        );
        resumed = restore(&resumed.encode_live_checkpoint().unwrap(), &objects).unwrap();
        resumed
            .put_one(&objects, &name("A"), message(b"next"), None)
            .unwrap();
        assert_eq!(
            resumed
                .get(
                    &objects,
                    &name("A"),
                    &request(MqGetMode::BrowseNext { cursor }, 100, MqTruncation::Reject),
                    None
                )
                .unwrap()
                .message
                .unwrap()
                .body,
            b"next"
        );
        let new_cursor = resumed
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        assert!(new_cursor > cursor);
        let cold = MqDeliveryKernel::decode(
            &resumed.encode().unwrap(),
            &objects,
            resumed.limits,
            resumed.message_limits,
            resumed.default_persistence,
        )
        .unwrap();
        assert!(cold.cursors.is_empty());
        assert_eq!(cold.next_cursor, resumed.next_cursor);
    }

    #[test]
    fn recovery_all_units_is_atomic_on_finalization_saturation() {
        let objects = catalog();
        let mut state = mixed(&objects);
        state
            .put_one(&objects, &name("B"), message(b"other"), Some(8))
            .unwrap();
        state.limits.finalized_units = 1;
        let mut resumed = MqDeliveryKernel::decode_live_checkpoint(
            &state.encode_live_checkpoint().unwrap(),
            &objects,
            state.limits,
            state.message_limits,
            state.default_persistence,
        )
        .unwrap();
        let before = resumed.clone();
        assert_eq!(
            resumed.recover_backout(),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(resumed, before);
        resumed.backout(7).unwrap();
        let before = resumed.clone();
        assert_eq!(resumed.commit(8), Err(MqDeliveryError::ResourceExhausted));
        assert_eq!(resumed.backout(8), Err(MqDeliveryError::ResourceExhausted));
        assert_eq!(resumed, before);
        resumed = MqDeliveryKernel::decode_live_checkpoint(
            &resumed.encode_live_checkpoint().unwrap(),
            &objects,
            state.limits,
            state.message_limits,
            state.default_persistence,
        )
        .unwrap();
        assert_eq!(resumed.unit_outcome(7), MqDeliveryOutcome::Rejected);
        assert_eq!(resumed.unit_outcome(8), MqDeliveryOutcome::Pending);
        resumed.limits.finalized_units = 2;
        resumed.recover_backout().unwrap();
        let before = resumed.clone();
        resumed.recover_backout().unwrap();
        assert_eq!(resumed, before);
        assert_eq!(resumed.depth(&name("A")), Some(2));
        assert_eq!(resumed.depth(&name("B")), Some(0));
    }

    #[test]
    fn malformed_checkpoints_fail_without_touching_the_live_authority() {
        let objects = catalog();
        let mut state = mixed(&objects);
        state
            .put_one(&objects, &name("B"), message(b"second-unit"), Some(8))
            .unwrap();
        let cursor = state
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        let before = state.clone();
        let original: Value =
            serde_json::from_slice(&state.encode_live_checkpoint().unwrap()).unwrap();
        let mut cases = Vec::new();
        macro_rules! bad {
            ($pointer:literal, $value:expr) => {{
                let mut value = original.clone();
                *value.pointer_mut($pointer).unwrap() = $value;
                cases.push(value);
            }};
        }
        bad!("/manager", json!("OTHER"));
        bad!("/default_persistent", json!(false));
        bad!("/next_id", json!(0));
        bad!("/next_id", json!(4));
        bad!("/next_cursor", json!(cursor));
        bad!("/queues/0/name", json!("B"));
        bad!("/queues/0/name", json!("A "));
        bad!("/queues/0/messages/0/id", json!(0));
        bad!("/pending/0/unit", json!(0));
        bad!("/pending/1/unit", json!(7));
        bad!("/pending/0/operations/0/queue", json!("ALIAS"));
        bad!(
            "/pending/0/operations/0/entry/id",
            original["queues"][0]["messages"][0]["id"].clone()
        );
        bad!(
            "/pending/1/operations/0/entry/id",
            original["pending"][0]["operations"][0]["entry"]["id"].clone()
        );
        bad!("/pending/0/operations/1/entry/id", json!(1));
        bad!("/pending/0/operations/0/entry/expires_at", json!(0));
        bad!("/pending/0/operations/0/entry/expires_at", json!(10));
        bad!(
            "/pending/0/operations/0/entry/message/expiry_ticks",
            json!(2)
        );
        bad!(
            "/pending/0/operations/0/entry/message/message_id",
            json!(null)
        );
        bad!("/pending/0/operations/0/entry/message/body", json!([256]));
        bad!(
            "/pending/0/operations/0/entry/message/group_sequence",
            json!(0)
        );
        bad!("/cursors/0/token", json!(0));
        bad!("/cursors/0/entry_id", json!(0));
        bad!("/cursors/0/entry_id", json!(999));
        bad!(
            "/cursors/0/entry_id",
            original["pending"][0]["operations"][1]["entry"]["id"].clone()
        );
        bad!("/cursors/0/queue", json!("B"));
        bad!("/finalized", json!([{ "unit":7, "committed":true }]));
        bad!("/finalized", json!([{ "unit":0, "committed":true }]));
        bad!(
            "/finalized",
            json!([{ "unit":2, "committed":true }, { "unit":1, "committed":false }])
        );
        let mut extra = original.clone();
        extra["pending"][0]["operations"][0]["entry"]["message"]["priority"] = json!(3);
        cases.push(extra);
        let mut extra = original.clone();
        extra["extra"] = json!(1);
        cases.push(extra);
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove("pending");
        cases.push(missing);
        let mut missing = original.clone();
        missing["pending"][0]["operations"][0]["entry"]
            .as_object_mut()
            .unwrap()
            .remove("expires_at");
        cases.push(missing);
        for field in [
            "message_id",
            "correlation_id",
            "group_id",
            "format",
            "expiry_ticks",
            "group_sequence",
            "segment_offset",
        ] {
            let mut missing = original.clone();
            missing["pending"][0]["operations"][0]["entry"]["message"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            cases.push(missing);
        }
        let mut swapped = original.clone();
        swapped["queues"].as_array_mut().unwrap().reverse();
        cases.push(swapped);
        let mut swapped = original.clone();
        swapped["pending"][0]["operations"]
            .as_array_mut()
            .unwrap()
            .reverse();
        cases.push(swapped);
        for (index, value) in cases.into_iter().enumerate() {
            assert_eq!(
                restore(&serde_json::to_vec(&value).unwrap(), &objects),
                Err(MqDeliveryError::CorruptSnapshot),
                "malformed case {index}"
            );
            assert_eq!(state, before);
        }
        let mut unsupported = original;
        unsupported["schema_version"] = json!("mainframe-env.mq-delivery-live@2");
        assert_eq!(
            restore(&serde_json::to_vec(&unsupported).unwrap(), &objects),
            Err(MqDeliveryError::UnsupportedSchema)
        );
        assert!(
            restore(
                b"{\"schema_version\":\"x\",\"schema_version\":\"y\"}",
                &objects
            )
            .is_err()
        );
        assert!(restore(b"{} trailing", &objects).is_err());
        let text = String::from_utf8(state.encode_live_checkpoint().unwrap()).unwrap();
        let duplicate = text.replacen(
            "\"persistent\":true",
            "\"persistent\":true,\"persistent\":false",
            1,
        );
        assert_eq!(
            restore(duplicate.as_bytes(), &objects),
            Err(MqDeliveryError::CorruptSnapshot)
        );
        assert!(restore(&state.encode().unwrap(), &objects).is_err());
    }

    #[test]
    fn conflicting_group_commit_after_restore_rejects_without_partial_publication() {
        let objects = catalog();
        let mut state = kernel(&objects);
        let mut grouped = message(b"group");
        grouped.descriptor.identifiers.group_id = Some(b"G".to_vec());
        grouped.descriptor.ordering.group_sequence = Some(1);
        state
            .put_one(&objects, &name("A"), grouped.clone(), Some(1))
            .unwrap();
        // A second destination verifies that a late conflict cannot publish
        // an earlier staged put from the same commit candidate.
        state
            .put_one(&objects, &name("B"), message(b"discard"), Some(2))
            .unwrap();
        state
            .put_one(&objects, &name("A"), grouped, Some(2))
            .unwrap();
        state = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        state.commit(1).unwrap();
        state = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        let before = state.clone();
        assert_eq!(state.commit(2), Err(MqDeliveryError::Group));
        assert_eq!(state, before);
        state.backout(2).unwrap();
        assert_eq!(state.depth(&name("A")), Some(1));
        assert_eq!(state.depth(&name("B")), Some(0));
    }

    #[test]
    fn live_projection_keeps_properties_and_group_holes_from_selected_gets() {
        let objects = catalog();
        let mut state = kernel(&objects);
        for sequence in 1..=3 {
            let mut grouped = message(&[sequence as u8]);
            grouped.descriptor.identifiers.message_id = Some(vec![sequence as u8]);
            grouped.descriptor.identifiers.group_id = Some(b"G".to_vec());
            grouped.descriptor.ordering.group_sequence = Some(sequence);
            grouped.descriptor.ordering.last_in_group = sequence == 3;
            grouped.descriptor.format = Some("opaque".into());
            grouped.properties.push(MqMessageProperty {
                name: "p".into(),
                kind: MqPropertyType::ByteString,
                value: vec![0, 128, 255],
            });
            state.put_one(&objects, &name("A"), grouped, None).unwrap();
        }
        let mut selected = request(MqGetMode::Remove, 100, MqTruncation::Reject);
        selected.selection.identifiers.message_id = Some(vec![2]);
        state.get(&objects, &name("A"), &selected, Some(5)).unwrap();
        let bytes = state.encode_live_checkpoint().unwrap();
        let mut resumed = restore(&bytes, &objects).unwrap();
        assert_eq!(resumed, state);
        resumed.backout(5).unwrap();
        for sequence in 1..=3 {
            let got = remove(&mut resumed, &objects, None).message.unwrap();
            assert_eq!(got.body, vec![sequence]);
            assert_eq!(got.properties[0].value, vec![0, 128, 255]);
            assert_eq!(got.descriptor.format.as_deref(), Some("opaque"));
        }
    }

    #[test]
    fn allocation_exhaustion_and_expired_get_backout_are_atomic_after_restore() {
        let objects = catalog();
        let mut state = kernel(&objects);
        let mut expiring = message(b"old");
        expiring.descriptor.expiry = MqExpiry::RelativeHostTicks(2);
        state.put_one(&objects, &name("A"), expiring, None).unwrap();
        let token = state
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        remove(&mut state, &objects, Some(1));
        state.next_id = u64::MAX;
        state.next_cursor = u64::MAX;
        let mut resumed = restore(&state.encode_live_checkpoint().unwrap(), &objects).unwrap();
        let before = resumed.clone();
        assert_eq!(
            resumed.put_one(&objects, &name("A"), message(b"overflow"), None),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(resumed, before);
        let mut cursor_probe = resumed.clone();
        cursor_probe.backout(1).unwrap();
        let before_probe = cursor_probe.clone();
        assert_eq!(
            cursor_probe.get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                None
            ),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(cursor_probe, before_probe);
        resumed.advance_tick(2).unwrap();
        resumed = restore(&resumed.encode_live_checkpoint().unwrap(), &objects).unwrap();
        resumed.backout(1).unwrap();
        assert_eq!(resumed.depth(&name("A")), Some(0));
        let before = resumed.clone();
        assert_eq!(
            resumed.get(
                &objects,
                &name("A"),
                &request(
                    MqGetMode::RemoveUnderCursor { cursor: token },
                    100,
                    MqTruncation::Reject
                ),
                None
            ),
            Err(MqDeliveryError::InvalidCursor)
        );
        assert_eq!(resumed, before);
        assert_eq!(resumed.next_cursor, u64::MAX);
    }

    #[test]
    fn checkpoint_limits_cover_depth_bytes_pending_cursors_and_outcomes() {
        let objects = catalog();
        let mut state = mixed(&objects);
        state
            .put_one(&objects, &name("B"), message(b"finalize"), Some(9))
            .unwrap();
        state.commit(9).unwrap();
        state
            .put_one(&objects, &name("B"), message(b"finalize"), Some(10))
            .unwrap();
        state.backout(10).unwrap();
        for _ in 0..2 {
            state
                .get(
                    &objects,
                    &name("A"),
                    &request(MqGetMode::BrowseFirst, 100, MqTruncation::Reject),
                    None,
                )
                .unwrap();
        }
        let bytes = state.encode_live_checkpoint().unwrap();
        let mut cases = Vec::new();
        for field in 0..6 {
            let mut limits = state.limits;
            match field {
                0 => limits.queues = 1,
                1 => limits.depth_per_queue = 2,
                2 => limits.total_bytes = 1,
                3 => limits.pending_operations = 1,
                4 => limits.cursors = 1,
                _ => limits.finalized_units = 1,
            }
            cases.push(limits);
        }
        for limits in cases {
            assert!(
                MqDeliveryKernel::decode_live_checkpoint(
                    &bytes,
                    &objects,
                    limits,
                    state.message_limits,
                    state.default_persistence
                )
                .is_err()
            );
        }
        let mut limits = state.limits;
        limits.snapshot_bytes = bytes.len() - 1;
        assert_eq!(
            MqDeliveryKernel::decode_live_checkpoint(
                &bytes,
                &objects,
                limits,
                state.message_limits,
                state.default_persistence
            ),
            Err(MqDeliveryError::ResourceExhausted)
        );
        state.limits.snapshot_bytes = bytes.len();
        assert_eq!(state.encode_live_checkpoint().unwrap(), bytes);
        state.limits.snapshot_bytes -= 1;
        let before = state.clone();
        assert_eq!(
            state.encode_live_checkpoint(),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(state, before);
        let mut messages = state.message_limits;
        messages.body_bytes = 1;
        assert_eq!(
            MqDeliveryKernel::decode_live_checkpoint(
                &bytes,
                &objects,
                MqDeliveryLimits::default(),
                messages,
                state.default_persistence
            ),
            Err(MqDeliveryError::CorruptSnapshot)
        );
    }
}
