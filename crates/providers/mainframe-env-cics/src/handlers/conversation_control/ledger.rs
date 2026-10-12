//! Versioned, CAS-backed allocation and attach-header authority.

use super::{
    ConversationConnectFrame, ConversationDataFrame, ConversationExchangeState, ConversationKind,
    ConversationOwner, ConversationPeerFrame, ConversationProblem, ConversationRecord,
    MAX_PROCESS_BYTES,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONVERSATION_STATE_NAMESPACE: &str = "cics-conversation-v1";
const KEY: &str = "state";
const MAX_ROW_BYTES: usize = 4 * 1024 * 1024;
const MAX_SYSTEMS: usize = 256;
const MAX_CONVERSATIONS: usize = 4096;
const MAX_ATTACH_HEADERS: usize = 4096;
const MAX_SIGNAL_FACILITIES: usize = 4096;
const LEDGER_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignalLuType {
    LuType4,
    LuType61,
    Pipeline3601,
    Interactive3767,
    Batch3770,
    FullFunction3790,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignalFacilityRecord {
    pub owner: ConversationOwner,
    pub lu_type: SignalLuType,
    pub pending: bool,
    pub terminal_error: bool,
    pub last_event_sequence: u64,
}

/// Installed APPC or MRO session group. An APPC group may be selected by
/// either mapped ALLOCATE or basic GDS ALLOCATE.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSystemDefinition {
    pub sysid: String,
    pub kind: ConversationKind,
    pub capacity: u16,
    pub enabled: bool,
}

impl ConversationSystemDefinition {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.sysid.is_empty()
            || self.sysid.len() > 4
            || !self.sysid.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
            })
            || !(1..=1024).contains(&self.capacity)
            || self.kind == ConversationKind::AppcBasic
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

/// Task-local attach FMH fields. A SEND/CONVERSE snapshots the selected header
/// into its outbound frame; BUILD ATTACH may later replace these fields.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationAttachHeader {
    pub owner: ConversationOwner,
    pub name: String,
    pub process: Vec<u8>,
    pub resource: Vec<u8>,
    pub return_process: Vec<u8>,
    pub return_resource: Vec<u8>,
    pub queue: Vec<u8>,
    pub iu_type: u16,
    pub data_stream: u16,
    pub record_format: u16,
}

impl ConversationAttachHeader {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if !self.owner.valid()
            || self.name.is_empty()
            || self.name.len() > 8
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
            })
            || [
                &self.process,
                &self.resource,
                &self.return_process,
                &self.return_resource,
                &self.queue,
            ]
            .iter()
            .any(|value| value.len() > MAX_PROCESS_BYTES)
            || !matches!(self.record_format, 1 | 4)
            || self.iu_type & !0x13 != 0
            || self.iu_type & 0x03 > 1
            || !matches!(self.data_stream >> 4, 0 | 0xc | 0xd | 0xe | 0xf)
            || self.data_stream >> 4 != 0 && self.data_stream & 0x0f != 0
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

/// One durable state row. The provider store owns its CAS, schema and replay
/// lifetime; callers must authorize and audit before attempting a mutation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationLedger {
    schema_version: u16,
    pub version: u64,
    next_token: u32,
    pub systems: BTreeMap<String, ConversationSystemDefinition>,
    pub conversations: BTreeMap<String, ConversationRecord>,
    pub attach_headers: BTreeMap<String, ConversationAttachHeader>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exchanges: BTreeMap<String, ConversationExchangeState>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signal_facilities: BTreeMap<String, SignalFacilityRecord>,
}

impl Default for ConversationLedger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_VERSION,
            version: 0,
            next_token: 1,
            systems: BTreeMap::new(),
            conversations: BTreeMap::new(),
            attach_headers: BTreeMap::new(),
            exchanges: BTreeMap::new(),
            signal_facilities: BTreeMap::new(),
        }
    }
}

impl ConversationLedger {
    pub fn load(store: &dyn ProviderStateStore) -> Result<Self, StoreError> {
        let Some(row) = store.get_provider_state(CONVERSATION_STATE_NAMESPACE, KEY)? else {
            return Ok(Self::default());
        };
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(StoreError::IncompatibleVersion);
        }
        let state: Self =
            serde_json::from_slice(&row.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if state.version != row.version
            || state.validate().is_err()
            || state
                .encode()
                .map_err(|_| StoreError::IncompatibleVersion)?
                != row.payload
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(state)
    }

    pub fn persist(
        &self,
        next: &mut Self,
        store: &dyn ProviderStateStore,
    ) -> Result<bool, StoreError> {
        next.schema_version = LEDGER_VERSION;
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: CONVERSATION_STATE_NAMESPACE.into(),
                key: KEY.into(),
                version: next.version,
                payload,
            },
            (self.version != 0).then_some(self.version),
        ) {
            Ok(()) => Ok(true),
            Err(StoreError::Conflict) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Commit protocol state and its exact request reply in one provider-store
    /// mutation. A stale CAS or reused replay key writes neither row.
    pub fn persist_with_replay(
        &self,
        next: &mut Self,
        replay: &super::ConversationReplay,
        store: &dyn ProviderStateStore,
    ) -> Result<bool, StoreError> {
        self.persist_with_replays(next, std::slice::from_ref(replay), store)
    }

    /// A resumed CONVERSE can have several coordinator effect identities for
    /// the same staged send. Commit one protocol result and every exact reply
    /// alias atomically, so no resumption can dispatch that send twice.
    pub fn persist_with_replays(
        &self,
        next: &mut Self,
        replays: &[super::ConversationReplay],
        store: &dyn ProviderStateStore,
    ) -> Result<bool, StoreError> {
        if replays.is_empty() || replays.len() > super::exchange::MAX_CONVERSE_ATTEMPTS {
            return Err(StoreError::InvalidTransition);
        }
        let mut keys = std::collections::BTreeSet::new();
        if replays
            .iter()
            .any(|replay| !keys.insert(&replay.effect_key))
        {
            return Err(StoreError::InvalidTransition);
        }
        next.schema_version = LEDGER_VERSION;
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let state_payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        let mut writes = vec![ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: CONVERSATION_STATE_NAMESPACE.into(),
                key: KEY.into(),
                version: next.version,
                payload: state_payload,
            },
            expected_version: (self.version != 0).then_some(self.version),
        }];
        for replay in replays {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: super::CONVERSATION_REPLAY_NAMESPACE.into(),
                    key: replay.effect_key.clone(),
                    version: 1,
                    payload: replay.encode()?,
                },
                expected_version: None,
            });
        }
        match store.put_provider_states_atomic(writes) {
            Ok(()) => Ok(true),
            Err(StoreError::Conflict) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn register_system(
        &mut self,
        definition: ConversationSystemDefinition,
    ) -> Result<(), ConversationProblem> {
        definition.validate()?;
        if self.systems.len() >= MAX_SYSTEMS && !self.systems.contains_key(&definition.sysid) {
            return Err(ConversationProblem::Exhausted);
        }
        if self
            .conversations
            .values()
            .any(|conversation| conversation.system == definition.sysid && !conversation.released)
        {
            return Err(ConversationProblem::WrongState);
        }
        self.systems.insert(definition.sysid.clone(), definition);
        Ok(())
    }

    /// Allocate a unique four-byte EIBRSRCE/CONVID token. A full group is
    /// reported as exhausted; the command handler decides NOQUEUE versus
    /// bounded suspension from the source and active HANDLE policy.
    pub fn allocate(
        &mut self,
        sysid: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
    ) -> Result<ConversationRecord, ConversationProblem> {
        let default_profile = if kind == ConversationKind::AppcBasic {
            "DEFAULT"
        } else {
            "DFHCICSA"
        };
        self.allocate_with_profile(sysid, kind, owner, default_profile)
    }

    pub fn allocate_with_profile(
        &mut self,
        sysid: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
        processing_profile: &str,
    ) -> Result<ConversationRecord, ConversationProblem> {
        let system = self
            .systems
            .get(sysid)
            .ok_or(ConversationProblem::Malformed)?;
        if !system.enabled {
            return Err(ConversationProblem::WrongState);
        }
        let compatible = match system.kind {
            ConversationKind::AppcMapped => {
                matches!(
                    kind,
                    ConversationKind::AppcMapped | ConversationKind::AppcBasic
                )
            }
            ConversationKind::Mro => kind == ConversationKind::Mro,
            ConversationKind::LuType61 => kind == ConversationKind::LuType61,
            ConversationKind::AppcBasic => false,
        };
        if !compatible {
            return Err(ConversationProblem::WrongKind);
        }
        let active = self
            .conversations
            .values()
            .filter(|record| record.system == sysid && !record.released)
            .count();
        if active >= usize::from(system.capacity) || self.conversations.len() >= MAX_CONVERSATIONS {
            return Err(ConversationProblem::Exhausted);
        }
        let token = self.next_token.to_be_bytes();
        let token_key = self.next_token.to_string();
        if self.next_token == u32::MAX || self.conversations.contains_key(&token_key) {
            return Err(ConversationProblem::Exhausted);
        }
        let record = ConversationRecord::allocate_with_profile(
            token,
            sysid,
            kind,
            owner,
            false,
            processing_profile,
        )?;
        self.next_token += 1;
        self.conversations.insert(token_key, record.clone());
        Ok(record)
    }

    /// A trusted APPC/MRO ingress installs the task's one principal facility.
    /// EXEC CICS ALLOCATE always creates an alternate facility instead.
    pub fn install_principal(
        &mut self,
        sysid: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
    ) -> Result<ConversationRecord, ConversationProblem> {
        if self.conversations.values().any(|record| {
            record.owner.execution == owner.execution
                && record.owner.run_unit == owner.run_unit
                && record.principal_facility
                && !record.released
        }) || self.signal_facilities.contains_key(&signal_key(&owner))
        {
            return Err(ConversationProblem::WrongState);
        }
        let mut record = self.allocate(sysid, kind, owner)?;
        record.principal_facility = true;
        self.conversation_mut(record.token)
            .ok_or(ConversationProblem::Malformed)?
            .principal_facility = true;
        Ok(record)
    }

    pub fn conversation(&self, token: [u8; 4]) -> Option<&ConversationRecord> {
        self.conversations
            .get(&u32::from_be_bytes(token).to_string())
    }

    pub fn conversation_mut(&mut self, token: [u8; 4]) -> Option<&mut ConversationRecord> {
        self.conversations
            .get_mut(&u32::from_be_bytes(token).to_string())
    }

    pub fn offer_peer_frame(
        &mut self,
        token: [u8; 4],
        frame: ConversationPeerFrame,
    ) -> Result<(), ConversationProblem> {
        let key = u32::from_be_bytes(token).to_string();
        let record = self
            .conversations
            .get(&key)
            .ok_or(ConversationProblem::NotOwned)?;
        if record.released
            || matches!(
                record.kind,
                ConversationKind::AppcBasic | ConversationKind::LuType61
            )
        {
            return Err(ConversationProblem::WrongKind);
        }
        if record.kind == ConversationKind::Mro && frame.signal {
            return Err(ConversationProblem::Malformed);
        }
        self.exchanges.entry(key).or_default().offer(frame)
    }

    pub fn exchange_mut(&mut self, token: [u8; 4]) -> Option<&mut ConversationExchangeState> {
        self.exchanges
            .get_mut(&u32::from_be_bytes(token).to_string())
    }

    pub fn exchange(&self, token: [u8; 4]) -> Option<&ConversationExchangeState> {
        self.exchanges.get(&u32::from_be_bytes(token).to_string())
    }

    pub fn ensure_exchange(&mut self, token: [u8; 4]) -> &mut ConversationExchangeState {
        self.exchanges
            .entry(u32::from_be_bytes(token).to_string())
            .or_default()
    }

    pub fn remove_exchange(&mut self, token: [u8; 4]) {
        self.exchanges
            .remove(&u32::from_be_bytes(token).to_string());
    }

    pub fn bind_mro_session(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
        name: &str,
    ) -> Result<(), ConversationProblem> {
        if name.is_empty()
            || name.len() > 4
            || !name.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
            })
        {
            return Err(ConversationProblem::Malformed);
        }
        if self.conversations.values().any(|record| {
            record.token != token
                && !record.released
                && record.owner.execution == owner.execution
                && record.owner.run_unit == owner.run_unit
                && record.mro_session_name.as_deref() == Some(name)
        }) {
            return Err(ConversationProblem::WrongState);
        }
        let record = self
            .conversation_mut(token)
            .ok_or(ConversationProblem::NotOwned)?;
        record.check_owner(owner, context)?;
        if record.kind != ConversationKind::Mro {
            return Err(ConversationProblem::WrongKind);
        }
        if record.mro_session_name.as_deref() == Some(name) {
            return Ok(());
        }
        if record.mro_session_name.is_some() {
            return Err(ConversationProblem::WrongState);
        }
        record.next_sequence()?;
        record.mro_session_name = Some(name.into());
        Ok(())
    }

    pub fn receive_mapped_peer_frame(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
        max_length: usize,
        retain_remainder: bool,
    ) -> Result<Option<super::ConversationDataReply>, ConversationProblem> {
        let record = self
            .conversation(token)
            .ok_or(ConversationProblem::NotOwned)?;
        record.check_owner(owner, context)?;
        if record.kind == ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        if record.data.terminal_error() || record.state != super::ConversationState::Receive {
            return Err(ConversationProblem::WrongState);
        }
        let kind = record.kind;
        let key = u32::from_be_bytes(token).to_string();
        let Some(exchange) = self.exchanges.get_mut(&key) else {
            return Ok(None);
        };
        let reply = exchange.receive_mapped(kind, max_length, retain_remainder)?;
        if let Some(ref reply) = reply {
            let record = self
                .conversation_mut(token)
                .ok_or(ConversationProblem::Malformed)?;
            record.next_sequence()?;
            record.state = reply.state;
        }
        Ok(reply)
    }

    pub fn consume_mapped_signal(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
    ) -> Result<bool, ConversationProblem> {
        let record = self
            .conversation(token)
            .ok_or(ConversationProblem::NotOwned)?;
        record.check_owner(owner, context)?;
        if record.kind == ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        let Some(exchange) = self.exchange_mut(token) else {
            return Ok(false);
        };
        if !exchange.take_signal() {
            return Ok(false);
        }
        self.conversation_mut(token)
            .ok_or(ConversationProblem::Malformed)?
            .next_sequence()?;
        Ok(true)
    }

    /// The sealed CONNECT PROCESS route records local state. The first SEND or
    /// WAIT that actually transmits data stages its process header in the same
    /// peer-frame ledger; CONVERSE continues to own its existing exchange path.
    pub fn ensure_mapped_connect_staged(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
    ) -> Result<bool, ConversationProblem> {
        let record = self
            .conversation(token)
            .ok_or(ConversationProblem::NotOwned)?;
        record.check_owner(owner, context)?;
        if record.kind != ConversationKind::AppcMapped {
            return Ok(false);
        }
        let Some(process) = record.process.clone() else {
            return Ok(false);
        };
        let pip = record.pip.clone();
        let sync_level = record.sync_level.ok_or(ConversationProblem::Malformed)?;
        let key = u32::from_be_bytes(token).to_string();
        if self.exchanges.get(&key).is_some_and(|exchange| {
            exchange.next_send_id != 0
                || !exchange.outbound.is_empty()
                || exchange.pending_converse.is_some()
        }) {
            return Ok(false);
        }
        self.stage_mapped_send(
            token,
            owner,
            context,
            ConversationDataFrame {
                end_structured_field: true,
                connect: Some(ConversationConnectFrame {
                    process,
                    pip,
                    sync_level,
                }),
                ..Default::default()
            },
        )?;
        Ok(true)
    }

    pub fn stage_mapped_send(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
        mut frame: ConversationDataFrame,
    ) -> Result<u64, ConversationProblem> {
        let record = self
            .conversation(token)
            .ok_or(ConversationProblem::NotOwned)?;
        record.check_owner(owner, context)?;
        if record.kind == ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        if record.data.terminal_error()
            || record.state != super::ConversationState::Send
                && !(record.kind == ConversationKind::Mro
                    && record.state == super::ConversationState::Allocated)
            || frame.confirm
                && (record.kind != ConversationKind::AppcMapped
                    || !matches!(record.sync_level, Some(1 | 2)))
            || frame.defresp && record.kind != ConversationKind::Mro
            || frame.connect.as_ref().is_some_and(|connect| {
                record.kind != ConversationKind::AppcMapped
                    || record.process.as_deref() != Some(connect.process.as_slice())
                    || record.pip != connect.pip
                    || record.sync_level != Some(connect.sync_level)
            })
            || frame
                .attach_id
                .as_ref()
                .is_some_and(|name| self.attach(owner, name).is_none())
        {
            return Err(ConversationProblem::WrongState);
        }
        if let Some(name) = frame.attach_id.as_deref() {
            frame.attach_header = Some(
                self.attach(owner, name)
                    .ok_or(ConversationProblem::WrongState)?
                    .clone(),
            );
        }
        let next_state = if frame.end_of_chain {
            super::ConversationState::PendFree
        } else if frame.invite {
            super::ConversationState::Receive
        } else {
            super::ConversationState::Send
        };
        let key = u32::from_be_bytes(token).to_string();
        let id = self
            .exchanges
            .entry(key)
            .or_default()
            .stage_send(frame, next_state)?;
        let record = self
            .conversation_mut(token)
            .ok_or(ConversationProblem::Malformed)?;
        record.next_sequence()?;
        if next_state == super::ConversationState::Receive {
            record.state = if record.kind == ConversationKind::Mro {
                super::ConversationState::Send
            } else {
                super::ConversationState::PendReceive
            };
        } else if next_state == super::ConversationState::PendFree {
            record.state = super::ConversationState::PendFree;
        } else if record.state == super::ConversationState::Allocated {
            record.state = super::ConversationState::Send;
        }
        Ok(id)
    }

    pub fn mark_mapped_send_attempted(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
        send_id: u64,
    ) -> Result<(), ConversationProblem> {
        self.conversation(token)
            .ok_or(ConversationProblem::NotOwned)?
            .check_owner(owner, context)?;
        self.exchange_mut(token)
            .ok_or(ConversationProblem::WrongState)?
            .mark_send_attempted(send_id)?;
        self.conversation_mut(token)
            .ok_or(ConversationProblem::Malformed)?
            .next_sequence()
    }

    pub fn acknowledge_mapped_send(
        &mut self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: super::ConversationContext,
        send_id: u64,
    ) -> Result<(), ConversationProblem> {
        self.conversation(token)
            .ok_or(ConversationProblem::NotOwned)?
            .check_owner(owner, context)?;
        let exchange = self
            .exchange_mut(token)
            .ok_or(ConversationProblem::WrongState)?;
        let confirmed_state = exchange.acknowledge_send(send_id)?;
        let pending_state = exchange.pending_sends.last().map(|send| send.next_state);
        let record = self
            .conversation_mut(token)
            .ok_or(ConversationProblem::Malformed)?;
        record.next_sequence()?;
        record.state = if record.state == super::ConversationState::Receive
            && matches!(
                confirmed_state,
                super::ConversationState::Send | super::ConversationState::Receive
            ) {
            super::ConversationState::Receive
        } else {
            match pending_state {
                Some(super::ConversationState::Receive) => {
                    if record.kind == ConversationKind::Mro {
                        super::ConversationState::Send
                    } else {
                        super::ConversationState::PendReceive
                    }
                }
                Some(super::ConversationState::PendFree) => super::ConversationState::PendFree,
                _ => confirmed_state,
            }
        };
        Ok(())
    }

    pub fn install_signal_facility(
        &mut self,
        owner: ConversationOwner,
        lu_type: SignalLuType,
    ) -> Result<(), ConversationProblem> {
        if !owner.valid()
            || self.conversations.values().any(|record| {
                record.owner.execution == owner.execution
                    && record.owner.run_unit == owner.run_unit
                    && record.principal_facility
                    && !record.released
            })
        {
            return Err(ConversationProblem::WrongState);
        }
        let key = signal_key(&owner);
        if let Some(existing) = self.signal_facilities.get(&key) {
            return if existing.owner == owner && existing.lu_type == lu_type {
                Ok(())
            } else {
                Err(ConversationProblem::WrongState)
            };
        }
        if self.signal_facilities.len() >= MAX_SIGNAL_FACILITIES {
            return Err(ConversationProblem::Exhausted);
        }
        self.signal_facilities.insert(
            key,
            SignalFacilityRecord {
                owner,
                lu_type,
                pending: false,
                terminal_error: false,
                last_event_sequence: 0,
            },
        );
        Ok(())
    }

    pub fn post_signal(
        &mut self,
        owner: &ConversationOwner,
        event_sequence: u64,
    ) -> Result<(), ConversationProblem> {
        let facility = self.signal_facility_mut(owner)?;
        if event_sequence == facility.last_event_sequence && event_sequence != 0 {
            return Ok(());
        }
        if facility.terminal_error
            || event_sequence != facility.last_event_sequence.saturating_add(1)
        {
            return Err(ConversationProblem::WrongState);
        }
        facility.last_event_sequence = event_sequence;
        facility.pending = true;
        Ok(())
    }

    pub fn fail_signal_facility(
        &mut self,
        owner: &ConversationOwner,
        event_sequence: u64,
    ) -> Result<(), ConversationProblem> {
        let facility = self.signal_facility_mut(owner)?;
        if facility.terminal_error && event_sequence == facility.last_event_sequence {
            return Ok(());
        }
        if facility.terminal_error
            || event_sequence != facility.last_event_sequence.saturating_add(1)
        {
            return Err(ConversationProblem::WrongState);
        }
        facility.last_event_sequence = event_sequence;
        facility.pending = false;
        facility.terminal_error = true;
        Ok(())
    }

    pub fn signal_pending(&self, owner: &ConversationOwner) -> Result<bool, ConversationProblem> {
        let facility = self.signal_facility(owner)?;
        if facility.terminal_error {
            return Err(ConversationProblem::WrongState);
        }
        Ok(facility.pending)
    }

    pub fn consume_signal(
        &mut self,
        owner: &ConversationOwner,
    ) -> Result<bool, ConversationProblem> {
        let facility = self.signal_facility_mut(owner)?;
        if facility.terminal_error {
            return Err(ConversationProblem::WrongState);
        }
        let pending = facility.pending;
        facility.pending = false;
        Ok(pending)
    }

    fn signal_facility(
        &self,
        owner: &ConversationOwner,
    ) -> Result<&SignalFacilityRecord, ConversationProblem> {
        let record = self
            .signal_facilities
            .get(&signal_key(owner))
            .ok_or(ConversationProblem::NotOwned)?;
        if record.owner.lease_epoch != owner.lease_epoch {
            return Err(ConversationProblem::StaleOwner);
        }
        Ok(record)
    }

    fn signal_facility_mut(
        &mut self,
        owner: &ConversationOwner,
    ) -> Result<&mut SignalFacilityRecord, ConversationProblem> {
        let record = self
            .signal_facilities
            .get_mut(&signal_key(owner))
            .ok_or(ConversationProblem::NotOwned)?;
        if record.owner.lease_epoch != owner.lease_epoch {
            return Err(ConversationProblem::StaleOwner);
        }
        Ok(record)
    }

    pub fn set_attach(
        &mut self,
        header: ConversationAttachHeader,
    ) -> Result<(), ConversationProblem> {
        header.validate()?;
        let key = attach_key(&header.owner, &header.name);
        if self.attach_headers.len() >= MAX_ATTACH_HEADERS
            && !self.attach_headers.contains_key(&key)
        {
            return Err(ConversationProblem::Exhausted);
        }
        self.attach_headers.insert(key, header);
        Ok(())
    }

    pub fn attach(
        &self,
        owner: &ConversationOwner,
        name: &str,
    ) -> Option<&ConversationAttachHeader> {
        self.attach_headers
            .get(&attach_key(owner, name))
            .filter(|header| &header.owner == owner)
    }

    /// Task termination frees every owned session and attach header. The
    /// returned count lets the caller audit exactly what was released.
    pub fn release_task(
        &mut self,
        owner: &ConversationOwner,
    ) -> Result<usize, ConversationProblem> {
        if self
            .conversations
            .values()
            .any(|record| &record.owner == owner && !record.released && record.sequence == u64::MAX)
        {
            return Err(ConversationProblem::Exhausted);
        }
        if self.conversations.values().any(|record| {
            &record.owner == owner
                && !record.released
                && record
                    .pending_issue
                    .as_ref()
                    .is_some_and(|pending| pending.attempted)
        }) {
            return Err(ConversationProblem::WrongState);
        }
        let mut released = 0;
        for record in self.conversations.values_mut() {
            if &record.owner == owner && !record.released {
                record.released = true;
                record.state = super::ConversationState::Free;
                record.pending_issue = None;
                record.data = super::ConversationDataState::default();
                record.sequence += 1;
                released += 1;
            }
        }
        self.attach_headers
            .retain(|_, header| &header.owner != owner);
        self.exchanges.retain(|key, _| {
            self.conversations
                .get(key)
                .is_some_and(|record| !record.released)
        });
        let prior_signals = self.signal_facilities.len();
        self.signal_facilities
            .retain(|_, facility| &facility.owner != owner);
        released += prior_signals - self.signal_facilities.len();
        Ok(released)
    }

    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if !matches!(self.schema_version, 1 | LEDGER_VERSION)
            || self.schema_version == 1 && !self.exchanges.is_empty()
            || self.schema_version == 1 && !self.signal_facilities.is_empty()
            || self.next_token == 0
            || self.systems.len() > MAX_SYSTEMS
            || self.conversations.len() > MAX_CONVERSATIONS
            || self.attach_headers.len() > MAX_ATTACH_HEADERS
            || self.exchanges.len() > MAX_CONVERSATIONS
            || self.signal_facilities.len() > MAX_SIGNAL_FACILITIES
        {
            return Err(ConversationProblem::Malformed);
        }
        for (key, definition) in &self.systems {
            definition.validate()?;
            if key != &definition.sysid {
                return Err(ConversationProblem::Malformed);
            }
        }
        for (key, record) in &self.conversations {
            record.validate()?;
            if key != &u32::from_be_bytes(record.token).to_string()
                || !self.systems.contains_key(&record.system)
            {
                return Err(ConversationProblem::Malformed);
            }
        }
        if self.schema_version == LEDGER_VERSION {
            let mut principals = std::collections::BTreeSet::new();
            let mut mro_sessions = std::collections::BTreeSet::new();
            for record in self
                .conversations
                .values()
                .filter(|record| !record.released)
            {
                if let Some(name) = record.mro_session_name.as_deref()
                    && !mro_sessions.insert((&record.owner.execution, &record.owner.run_unit, name))
                {
                    return Err(ConversationProblem::Malformed);
                }
            }
            for record in self
                .conversations
                .values()
                .filter(|record| record.principal_facility && !record.released)
            {
                if !principals.insert((&record.owner.execution, &record.owner.run_unit)) {
                    return Err(ConversationProblem::Malformed);
                }
            }
        }
        for definition in self.systems.values() {
            if self
                .conversations
                .values()
                .filter(|record| record.system == definition.sysid && !record.released)
                .count()
                > usize::from(definition.capacity)
            {
                return Err(ConversationProblem::Malformed);
            }
        }
        for (key, header) in &self.attach_headers {
            header.validate()?;
            if key != &attach_key(&header.owner, &header.name) {
                return Err(ConversationProblem::Malformed);
            }
        }
        for (key, exchange) in &self.exchanges {
            exchange.validate()?;
            let Some(record) = self.conversations.get(key) else {
                return Err(ConversationProblem::Malformed);
            };
            if record.released
                || matches!(
                    record.kind,
                    ConversationKind::AppcBasic | ConversationKind::LuType61
                )
                || exchange.pending_converse.as_ref().is_some_and(|pending| {
                    pending.owner_epoch != record.owner.lease_epoch
                        || pending
                            .outbound
                            .attach_header
                            .as_ref()
                            .is_some_and(|header| header.owner != record.owner)
                })
                || exchange.outbound.iter().any(|frame| {
                    frame
                        .attach_header
                        .as_ref()
                        .is_some_and(|header| header.owner != record.owner)
                })
            {
                return Err(ConversationProblem::Malformed);
            }
        }
        for (key, facility) in &self.signal_facilities {
            if !facility.owner.valid()
                || key != &signal_key(&facility.owner)
                || facility.pending && facility.last_event_sequence == 0
                || facility.terminal_error
                    && (facility.pending || facility.last_event_sequence == 0)
                || self.conversations.values().any(|record| {
                    record.owner.execution == facility.owner.execution
                        && record.owner.run_unit == facility.owner.run_unit
                        && record.principal_facility
                        && !record.released
                })
            {
                return Err(ConversationProblem::Malformed);
            }
        }
        Ok(())
    }

    fn encode(&self) -> Result<Vec<u8>, ConversationProblem> {
        self.validate()?;
        let payload = serde_json::to_vec(self).map_err(|_| ConversationProblem::Malformed)?;
        if payload.len() > MAX_ROW_BYTES {
            return Err(ConversationProblem::Exhausted);
        }
        Ok(payload)
    }
}

fn attach_key(owner: &ConversationOwner, name: &str) -> String {
    format!("{}\0{}\0{name}", owner.execution, owner.run_unit)
}

fn signal_key(owner: &ConversationOwner) -> String {
    format!("{}\0{}", owner.execution, owner.run_unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};

    fn replay(token: [u8; 4]) -> super::super::ConversationReplay {
        super::super::ConversationReplay {
            schema_version: 1,
            effect_key: "alloc-effect".into(),
            owner_execution: "execution".into(),
            owner_run_unit: "run".into(),
            owner_principal: "user".into(),
            owner_epoch: 1,
            mutation_sequence: 1,
            request_digest: [7; 32],
            deadline_tick: 100,
            retain_until_tick: 200,
            reply: super::super::ConversationReply {
                condition: "NORMAL".into(),
                response: 0,
                response2: 0,
                state: Some(super::super::ConversationState::Allocated),
                token: Some(token),
                outputs: BTreeMap::from([("CONVID".into(), token.to_vec())]),
            },
        }
    }

    fn owner() -> ConversationOwner {
        ConversationOwner {
            execution: "execution".into(),
            run_unit: "run".into(),
            lease_epoch: 1,
        }
    }

    #[test]
    fn allocation_is_bounded_and_fenced_by_store_version() {
        let store = MemoryStore::new(Default::default());
        let initial = ConversationLedger::load(&store).unwrap();
        let mut next = initial.clone();
        next.register_system(ConversationSystemDefinition {
            sysid: "SYS1".into(),
            kind: ConversationKind::AppcMapped,
            capacity: 1,
            enabled: true,
        })
        .unwrap();
        assert!(initial.persist(&mut next, &store).unwrap());
        let mut competing = initial.clone();
        competing
            .register_system(ConversationSystemDefinition {
                sysid: "SYS2".into(),
                kind: ConversationKind::Mro,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        assert!(!initial.persist(&mut competing, &store).unwrap());
        let current = ConversationLedger::load(&store).unwrap();
        let mut allocated = current.clone();
        let record = allocated
            .allocate("SYS1", ConversationKind::AppcBasic, owner())
            .unwrap();
        assert_eq!(record.token, [0, 0, 0, 1]);
        assert_eq!(
            allocated.allocate("SYS1", ConversationKind::AppcMapped, owner()),
            Err(ConversationProblem::Exhausted)
        );
        assert!(current.persist(&mut allocated, &store).unwrap());
        let reopened = ConversationLedger::load(&store).unwrap();
        assert_eq!(reopened.conversation(record.token), Some(&record));
    }

    #[test]
    fn principal_is_unique_per_task_and_survives_reopen() {
        let store = MemoryStore::new(Default::default());
        let mut initial = ConversationLedger::default();
        initial
            .register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 3,
                enabled: true,
            })
            .unwrap();
        let principal = initial
            .install_principal("SYS1", ConversationKind::AppcBasic, owner())
            .unwrap();
        assert!(principal.principal_facility);
        assert_eq!(
            initial.install_principal("SYS1", ConversationKind::AppcBasic, owner()),
            Err(ConversationProblem::WrongState)
        );
        assert!(
            !initial
                .allocate("SYS1", ConversationKind::AppcBasic, owner())
                .unwrap()
                .principal_facility
        );
        let mut persisted = initial.clone();
        assert!(
            ConversationLedger::default()
                .persist(&mut persisted, &store)
                .unwrap()
        );
        let reopened = ConversationLedger::load(&store).unwrap();
        assert_eq!(reopened.conversation(principal.token), Some(&principal));
    }

    #[test]
    fn v1_ledger_reopens_and_upgrades_atomically_for_peer_frames() {
        let store = MemoryStore::new(Default::default());
        let mut old = ConversationLedger {
            schema_version: 1,
            version: 1,
            ..ConversationLedger::default()
        };
        old.register_system(ConversationSystemDefinition {
            sysid: "SYS1".into(),
            kind: ConversationKind::AppcMapped,
            capacity: 1,
            enabled: true,
        })
        .unwrap();
        let token = old
            .allocate("SYS1", ConversationKind::AppcMapped, owner())
            .unwrap()
            .token;
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONVERSATION_STATE_NAMESPACE.into(),
                    key: KEY.into(),
                    version: 1,
                    payload: old.encode().unwrap(),
                },
                None,
            )
            .unwrap();
        let current = ConversationLedger::load(&store).unwrap();
        assert_eq!(current.schema_version, 1);
        let mut impossible = current.clone();
        impossible.signal_facilities.insert(
            signal_key(&owner()),
            SignalFacilityRecord {
                owner: owner(),
                lu_type: SignalLuType::LuType4,
                pending: false,
                terminal_error: false,
                last_event_sequence: 0,
            },
        );
        assert_eq!(impossible.validate(), Err(ConversationProblem::Malformed));
        let mut next = current.clone();
        next.offer_peer_frame(
            token,
            ConversationPeerFrame {
                data: b"REPLY".to_vec(),
                next_state: super::super::ConversationState::Receive,
                end_of_chain: false,
                inbound_fmh: false,
                signal: false,
            },
        )
        .unwrap();
        assert!(current.persist(&mut next, &store).unwrap());
        let reopened = ConversationLedger::load(&store).unwrap();
        assert_eq!(reopened.schema_version, LEDGER_VERSION);
        assert_eq!(
            reopened.exchanges[&u32::from_be_bytes(token).to_string()].inbound[0].data,
            b"REPLY"
        );
    }

    #[test]
    fn v1_extract_indicator_and_lu61_state_survive_v2_upgrade() {
        let store = MemoryStore::new(Default::default());
        let mut old = ConversationLedger {
            schema_version: 1,
            version: 1,
            ..ConversationLedger::default()
        };
        old.register_system(ConversationSystemDefinition {
            sysid: "LU61".into(),
            kind: ConversationKind::LuType61,
            capacity: 1,
            enabled: true,
        })
        .unwrap();
        let token = old
            .allocate("LU61", ConversationKind::LuType61, owner())
            .unwrap()
            .token;
        let record = old.conversation_mut(token).unwrap();
        record.version = 1;
        record.processing_profile = None;
        record.state = super::super::ConversationState::PendReceive;
        record.indicators.receive_required = true;
        record.sequence = 1;
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONVERSATION_STATE_NAMESPACE.into(),
                    key: KEY.into(),
                    version: 1,
                    payload: old.encode().unwrap(),
                },
                None,
            )
            .unwrap();
        let current = ConversationLedger::load(&store).unwrap();
        assert_eq!(
            current.conversation(token).unwrap().indicators.convdata()[3],
            0xff
        );
        let mut upgraded = current.clone();
        assert!(current.persist(&mut upgraded, &store).unwrap());
        let reopened = ConversationLedger::load(&store).unwrap();
        assert_eq!(reopened.schema_version, LEDGER_VERSION);
        assert_eq!(reopened.conversation(token), current.conversation(token));
    }

    #[test]
    fn sqlite_reopen_preserves_explicit_peer_frame() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-conversation-exchange-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let first = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let current = ConversationLedger::load(&first).unwrap();
        let mut next = current.clone();
        next.register_system(ConversationSystemDefinition {
            sysid: "SYS1".into(),
            kind: ConversationKind::AppcMapped,
            capacity: 1,
            enabled: true,
        })
        .unwrap();
        let token = next
            .allocate("SYS1", ConversationKind::AppcMapped, owner())
            .unwrap()
            .token;
        next.offer_peer_frame(
            token,
            ConversationPeerFrame {
                data: b"RESPONSE".to_vec(),
                next_state: super::super::ConversationState::Receive,
                end_of_chain: true,
                inbound_fmh: false,
                signal: false,
            },
        )
        .unwrap();
        assert!(current.persist(&mut next, &first).unwrap());
        drop(first);
        let reopened = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let ledger = ConversationLedger::load(&reopened).unwrap();
        assert_eq!(
            ledger.exchanges[&u32::from_be_bytes(token).to_string()].inbound[0].data,
            b"RESPONSE"
        );
        assert_eq!(ledger.schema_version, LEDGER_VERSION);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attach_header_is_task_owned_and_task_cleanup_releases_all() {
        let mut ledger = ConversationLedger::default();
        ledger
            .register_system(ConversationSystemDefinition {
                sysid: "MRO1".into(),
                kind: ConversationKind::Mro,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        ledger
            .allocate("MRO1", ConversationKind::Mro, owner())
            .unwrap();
        let header = ConversationAttachHeader {
            owner: owner(),
            name: "HDR".into(),
            process: b"TRAN".to_vec(),
            resource: vec![],
            return_process: vec![],
            return_resource: vec![],
            queue: vec![],
            iu_type: 0,
            data_stream: 0,
            record_format: 4,
        };
        ledger.set_attach(header.clone()).unwrap();
        assert_eq!(ledger.attach(&owner(), "HDR"), Some(&header));
        assert_eq!(ledger.release_task(&owner()), Ok(1));
        assert!(ledger.attach(&owner(), "HDR").is_none());
        assert!(ledger.conversation([0, 0, 0, 1]).unwrap().released);
    }

    #[test]
    fn sqlite_reopen_preserves_allocation_and_release() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-conversation-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let first = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let initial = ConversationLedger::load(&first).unwrap();
        let mut installed = initial.clone();
        installed
            .register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        assert!(initial.persist(&mut installed, &first).unwrap());
        let mut allocated = installed.clone();
        let token = allocated
            .install_principal("SYS1", ConversationKind::AppcBasic, owner())
            .unwrap()
            .token;
        assert!(
            installed
                .persist_with_replay(&mut allocated, &replay(token), &first)
                .unwrap()
        );
        drop(first);
        let reopened = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let current = ConversationLedger::load(&reopened).unwrap();
        assert_eq!(current.conversation(token).unwrap().owner, owner());
        assert!(current.conversation(token).unwrap().principal_facility);
        let saved = super::super::load_conversation_replay(&reopened, "alloc-effect")
            .unwrap()
            .unwrap();
        assert_eq!(saved.reply.token, Some(token));
        let mut released = current.clone();
        assert_eq!(released.release_task(&owner()), Ok(1));
        assert!(current.persist(&mut released, &reopened).unwrap());
        assert!(
            ConversationLedger::load(&reopened)
                .unwrap()
                .conversation(token)
                .unwrap()
                .released
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_replay_write_rejects_stale_state_without_partial_receipt() {
        let store = MemoryStore::new(Default::default());
        let initial = ConversationLedger::load(&store).unwrap();
        let mut installed = initial.clone();
        installed
            .register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        assert!(initial.persist(&mut installed, &store).unwrap());
        let mut stale = initial.clone();
        stale
            .register_system(ConversationSystemDefinition {
                sysid: "SYS2".into(),
                kind: ConversationKind::Mro,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        assert!(
            !initial
                .persist_with_replay(&mut stale, &replay([0, 0, 0, 1]), &store)
                .unwrap()
        );
        assert!(
            super::super::load_conversation_replay(&store, "alloc-effect")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn multiple_converse_replays_commit_with_state_or_not_at_all() {
        let store = MemoryStore::new(Default::default());
        let initial = ConversationLedger::load(&store).unwrap();
        let mut installed = initial.clone();
        installed
            .register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::Mro,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        assert!(initial.persist(&mut installed, &store).unwrap());
        let current = ConversationLedger::load(&store).unwrap();
        let existing = replay([0, 0, 0, 1]);
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: super::super::CONVERSATION_REPLAY_NAMESPACE.into(),
                    key: existing.effect_key.clone(),
                    version: 1,
                    payload: existing.encode().unwrap(),
                },
                None,
            )
            .unwrap();
        let mut alias = existing.clone();
        alias.effect_key = "continuation-effect".into();
        alias.mutation_sequence = 2;
        let mut next = current.clone();
        next.register_system(ConversationSystemDefinition {
            sysid: "SYS2".into(),
            kind: ConversationKind::Mro,
            capacity: 1,
            enabled: true,
        })
        .unwrap();
        assert!(
            !current
                .persist_with_replays(&mut next, &[alias, existing], &store)
                .unwrap()
        );
        let reopened = ConversationLedger::load(&store).unwrap();
        assert!(!reopened.systems.contains_key("SYS2"));
        assert!(
            super::super::load_conversation_replay(&store, "continuation-effect")
                .unwrap()
                .is_none()
        );
    }
}
