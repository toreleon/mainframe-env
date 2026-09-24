//! Exact request replay records for transport-neutral conversation mutations.

use super::ConversationState;
use mainframe_env_store_api::{ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONVERSATION_REPLAY_NAMESPACE: &str = "cics-conversation-replay-v1";
const MAX_REPLAY_BYTES: usize = 1024 * 1024 + 4096;
const MAX_OUTPUTS: usize = 16;
const MAX_REPLAYS_PER_SCAN: usize = 4096;

/// The bounded, source-visible reply retained with one state transition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationReply {
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub state: Option<ConversationState>,
    pub token: Option<[u8; 4]>,
    pub outputs: BTreeMap<String, Vec<u8>>,
}

impl ConversationReply {
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.condition.is_empty()
            || self.condition.len() > 32
            || !self.condition.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.response < 0
            || self.response2 < 0
            || self.outputs.len() > MAX_OUTPUTS
            || self.outputs.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > 32
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    || value.len() > 1024 * 1024
            })
            || self.outputs.values().map(Vec::len).sum::<usize>() > 1024 * 1024
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
    }
}

/// Same-key reissue may return this receipt only for the same owner, lease,
/// request digest and mutation sequence. It never redispatches an uncertain
/// transport send.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationReplay {
    pub schema_version: u16,
    pub effect_key: String,
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub owner_principal: String,
    pub owner_epoch: u64,
    pub mutation_sequence: u64,
    pub request_digest: [u8; 32],
    pub deadline_tick: u64,
    pub retain_until_tick: u64,
    pub reply: ConversationReply,
}

impl ConversationReplay {
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.schema_version != 1
            || self.effect_key.is_empty()
            || self.effect_key.len() > 256
            || self.owner_execution.is_empty()
            || self.owner_execution.len() > 128
            || self.owner_run_unit.is_empty()
            || self.owner_run_unit.len() > 128
            || self.owner_principal.is_empty()
            || self.owner_principal.len() > 128
            || self.owner_epoch == 0
            || self.mutation_sequence == 0
            || self.deadline_tick == 0
            || self.retain_until_tick < self.deadline_tick
        {
            return Err(StoreError::IncompatibleVersion);
        }
        self.reply.validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>, StoreError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| StoreError::IncompatibleVersion)?;
        if bytes.len() > MAX_REPLAY_BYTES {
            return Err(StoreError::PayloadTooLarge);
        }
        Ok(bytes)
    }

    fn decode(key: &str, bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() > MAX_REPLAY_BYTES {
            return Err(StoreError::IncompatibleVersion);
        }
        let replay: Self =
            serde_json::from_slice(bytes).map_err(|_| StoreError::IncompatibleVersion)?;
        replay.validate()?;
        if replay.effect_key != key || replay.encode()? != bytes {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(replay)
    }

    /// Check the full owner and request identity before returning a prior
    /// reply. A different request under the same key is a hard conflict.
    pub fn matches_request(
        &self,
        execution: &str,
        run_unit: &str,
        principal: &str,
        epoch: u64,
        sequence: u64,
        digest: [u8; 32],
    ) -> Result<&ConversationReply, StoreError> {
        if self.owner_execution != execution
            || self.owner_run_unit != run_unit
            || self.owner_principal != principal
            || self.owner_epoch != epoch
            || self.mutation_sequence != sequence
            || self.request_digest != digest
        {
            return Err(StoreError::Conflict);
        }
        Ok(&self.reply)
    }
}

pub fn load_conversation_replay(
    store: &dyn ProviderStateStore,
    effect_key: &str,
) -> Result<Option<ConversationReplay>, StoreError> {
    let Some(row) = store.get_provider_state(CONVERSATION_REPLAY_NAMESPACE, effect_key)? else {
        return Ok(None);
    };
    if row.version != 1 {
        return Err(StoreError::IncompatibleVersion);
    }
    ConversationReplay::decode(effect_key, &row.payload).map(Some)
}

/// Reclaim only expired receipts older than an externally established safe
/// checkpoint watermark. Live conversation tokens and explicit references
/// protect their replay rows from pruning.
pub fn prune_conversation_replays(
    store: &dyn ProviderStateStore,
    safe_watermark_tick: u64,
    protected_keys: &BTreeSet<String>,
) -> Result<usize, StoreError> {
    if safe_watermark_tick == 0 || protected_keys.len() > MAX_REPLAYS_PER_SCAN {
        return Err(StoreError::InvalidTransition);
    }
    let ledger = super::ConversationLedger::load(store)?;
    let rows = store.list_provider_state(CONVERSATION_REPLAY_NAMESPACE, MAX_REPLAYS_PER_SCAN)?;
    let mut removed = 0;
    for row in rows {
        let replay = ConversationReplay::decode(&row.key, &row.payload)?;
        let active_token = replay.reply.token.is_some_and(|token| {
            ledger
                .conversation(token)
                .is_some_and(|record| !record.released)
        });
        if replay.retain_until_tick <= safe_watermark_tick
            && !active_token
            && !protected_keys.contains(&row.key)
        {
            store.delete_provider_state(CONVERSATION_REPLAY_NAMESPACE, &row.key, row.version)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ConversationKind, ConversationLedger, ConversationOwner, ConversationSystemDefinition,
    };
    use super::*;
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::ProviderStateRecord;

    fn replay() -> ConversationReplay {
        ConversationReplay {
            schema_version: 1,
            effect_key: "effect-1".into(),
            owner_execution: "exec".into(),
            owner_run_unit: "run".into(),
            owner_principal: "user".into(),
            owner_epoch: 2,
            mutation_sequence: 4,
            request_digest: [7; 32],
            deadline_tick: 100,
            retain_until_tick: 200,
            reply: ConversationReply {
                condition: "NORMAL".into(),
                response: 0,
                response2: 0,
                state: Some(ConversationState::Allocated),
                token: Some([0, 0, 0, 1]),
                outputs: BTreeMap::from([("CONVID".into(), vec![0, 0, 0, 1])]),
            },
        }
    }

    #[test]
    fn replay_checks_exact_owner_and_prunes_only_after_safe_watermark() {
        let store = MemoryStore::new(Default::default());
        let saved = replay();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONVERSATION_REPLAY_NAMESPACE.into(),
                    key: saved.effect_key.clone(),
                    version: 1,
                    payload: saved.encode().unwrap(),
                },
                None,
            )
            .unwrap();
        let reopened = load_conversation_replay(&store, "effect-1")
            .unwrap()
            .unwrap();
        assert_eq!(
            reopened.matches_request("exec", "run", "user", 2, 4, [7; 32]),
            Ok(&saved.reply)
        );
        assert_eq!(
            reopened.matches_request("exec", "run", "user", 1, 4, [7; 32]),
            Err(StoreError::Conflict)
        );
        assert_eq!(
            prune_conversation_replays(&store, 199, &BTreeSet::new()),
            Ok(0)
        );
        assert_eq!(
            prune_conversation_replays(&store, 200, &BTreeSet::from(["effect-1".into()])),
            Ok(0)
        );
        assert_eq!(
            prune_conversation_replays(&store, 200, &BTreeSet::new()),
            Ok(1)
        );
    }

    #[test]
    fn active_conversation_protects_allocation_replay() {
        let store = MemoryStore::new(Default::default());
        let original = ConversationLedger::load(&store).unwrap();
        let mut ledger = original.clone();
        ledger
            .register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        let owner = ConversationOwner {
            execution: "exec".into(),
            run_unit: "run".into(),
            lease_epoch: 2,
        };
        let token = ledger
            .allocate("SYS1", ConversationKind::AppcMapped, owner.clone())
            .unwrap()
            .token;
        let mut receipt = replay();
        receipt.reply.token = Some(token);
        assert!(
            original
                .persist_with_replay(&mut ledger, &receipt, &store)
                .unwrap()
        );
        assert_eq!(
            prune_conversation_replays(&store, 200, &BTreeSet::new()),
            Ok(0)
        );
        let mut released = ledger.clone();
        assert_eq!(released.release_task(&owner), Ok(1));
        assert!(ledger.persist(&mut released, &store).unwrap());
        assert_eq!(
            prune_conversation_replays(&store, 200, &BTreeSet::new()),
            Ok(1)
        );
    }
}
