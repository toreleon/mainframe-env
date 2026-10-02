//! Bounded MQI handle lifetime and ownership contract.
//!
//! These identities are host-side tokens, not MQ wire integers. An MQI adapter
//! must translate its wire handles and completion/reason codes explicitly.
//! This module does not register or implement any MQI call.

use crate::MqHostEnvironment;
use std::sync::atomic::{AtomicU64, Ordering};

mod observation;
pub use observation::MqHandleObservation;

static NEXT_REGISTRY_ID: AtomicU64 = AtomicU64::new(1);

/// Upper bound on live and reusable handle slots in one registry.
pub const MQ_MAX_HANDLE_SLOTS: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HandleId {
    registry: u64,
    slot: u32,
    generation: u64,
    epoch: u64,
    // Canonical identity is observation, not execution permission. Only the
    // observation factory creates this disposition; no public clearing API.
    historical: bool,
}

/// An MQHCONN identity, including the two distinct IBM special values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHconn {
    /// A connection returned by MQCONN or MQCONNX outside the CICS default case.
    Issued(MqConnectionId),
    /// `MQHC_DEF_HCONN`: the CICS task's default connection.
    Default,
    /// `MQHC_UNASSOCIATED_HCONN`: only valid for message-handle operations.
    Unassociated,
}

/// Opaque issued connection identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqConnectionId(HandleId);

/// `MQHC_DEF_HCONN`, kept distinct from an issued connection.
pub const MQHC_DEF_HCONN: MqHconn = MqHconn::Default;
/// `MQHC_UNASSOCIATED_HCONN`, kept distinct from a default connection.
pub const MQHC_UNASSOCIATED_HCONN: MqHconn = MqHconn::Unassociated;

/// Object access returned by MQOPEN or as MQSUB's destination Hobj.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqHobj(HandleId);
/// Subscription identity returned in MQSUB's Hsub parameter.
///
/// Hsub has the MQHOBJ wire type but a separate permitted-use role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqHsub(HandleId);
/// Message-property identity returned by MQCRTMH.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqHmsg(HandleId);

macro_rules! canonical_identity {
    ($($token:ident),+ $(,)?) => {$(
        impl $token {
            /// Read-only host canonical identity: registry, slot, generation,
            /// epoch. These are not MQ wire values. No reconstruction API is
            /// exposed for live authority; lifetime validation remains registry-owned.
            pub(crate) const fn canonical_parts(self) -> (u64, u32, u64, u64) {
                let HandleId { registry, slot, generation, epoch, .. } = self.0;
                (registry, slot, generation, epoch)
            }

            /// A replay identity that every registry access refuses. Canonical
            /// equality with a live token does not confer authority equality.
            pub const fn is_historical(self) -> bool {
                self.0.historical
            }
        }
    )+};
}
canonical_identity!(MqConnectionId, MqHobj, MqHsub, MqHmsg);

/// Runtime handle tag for an MQI parameter whose expected role is catalogued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandle {
    Object(MqHobj),
    Subscription(MqHsub),
    Message(MqHmsg),
}

impl From<MqHobj> for MqHandle {
    fn from(value: MqHobj) -> Self {
        Self::Object(value)
    }
}
impl From<MqHsub> for MqHandle {
    fn from(value: MqHsub) -> Self {
        Self::Subscription(value)
    }
}
impl From<MqHmsg> for MqHandle {
    fn from(value: MqHmsg) -> Self {
        Self::Message(value)
    }
}

impl MqHandle {
    fn id(self) -> HandleId {
        match self {
            Self::Object(value) => value.0,
            Self::Subscription(value) => value.0,
            Self::Message(value) => value.0,
        }
    }

    fn kind(self) -> MqHandleKind {
        match self {
            Self::Object(_) => MqHandleKind::Object,
            Self::Subscription(_) => MqHandleKind::Subscription,
            Self::Message(_) => MqHandleKind::Message,
        }
    }
}

/// Distinct MQI handle roles. Subscription and object share a wire type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandleKind {
    Object,
    Subscription,
    Message,
}

/// MQCONN and MQCONNX handle sharing choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandleSharing {
    /// MQCONN or MQCNO_HANDLE_SHARE_NONE.
    NonShared,
    /// MQCNO_HANDLE_SHARE_BLOCK.
    SharedBlock,
    /// MQCNO_HANDLE_SHARE_NO_BLOCK.
    SharedNoBlock,
}

/// Caller-attested host and unit identity. Every field must be nonzero.
///
/// For IMS, `syncpoint_epoch` changes at each sync point. The host must assign
/// non-reused IDs within a registry epoch; a new runtime advances that epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqHandleOwner {
    pub environment: MqHostEnvironment,
    pub host_id: u64,
    pub process_id: u64,
    pub thread_id: u64,
    pub task_id: u64,
    pub syncpoint_epoch: u64,
}

impl MqHandleOwner {
    fn valid(self) -> bool {
        self.host_id != 0
            && self.process_id != 0
            && self.thread_id != 0
            && self.task_id != 0
            && self.syncpoint_epoch != 0
    }

    fn same_unit(self, other: Self) -> bool {
        if self.environment != other.environment
            || self.host_id != other.host_id
            || self.process_id != other.process_id
        {
            return false;
        }
        match self.environment {
            MqHostEnvironment::ZosCics
            | MqHostEnvironment::ZosBatch
            | MqHostEnvironment::ZosImsBatchDli => self.task_id == other.task_id,
            MqHostEnvironment::ZosIms => {
                self.task_id == other.task_id && self.syncpoint_epoch == other.syncpoint_epoch
            }
            MqHostEnvironment::MqiClient | MqHostEnvironment::OtherBindings => {
                self.thread_id == other.thread_id
            }
        }
    }

    fn permits(self, other: Self, sharing: MqHandleSharing) -> bool {
        if self.environment != other.environment
            || self.host_id != other.host_id
            || self.process_id != other.process_id
        {
            return false;
        }
        sharing != MqHandleSharing::NonShared || self.same_unit(other)
    }
}

/// A rejected registry operation. No rejected operation changes live handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandleProblem {
    InvalidConfiguration,
    InvalidOwner,
    Capacity,
    Stale,
    WrongKind,
    CrossOwner,
    CrossConnection,
    MissingConnection,
    SpecialConnection,
    AlreadyConnected,
    InUse,
    EpochNotAdvanced,
    /// A stored identity is never directly executable, even for a live slot.
    Historical,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    kind: EntryKind,
    owner: MqHandleOwner,
    parent: Option<HandleId>,
    sharing: MqHandleSharing,
    in_use: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Connection,
    Object,
    Subscription,
    Message,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    generation: u64,
    entry: Option<Entry>,
}

/// A finite registry whose opaque tokens carry both slot generation and epoch.
///
/// Methods take `&mut self` for mutations; a provider may serialize access using
/// its existing queue-manager authority. No provider state is stored here.
#[derive(Debug)]
pub struct MqHandleRegistry {
    registry_id: u64,
    epoch: u64,
    max_slots: usize,
    slots: Vec<Slot>,
    active: usize,
    defaults: Vec<(MqHandleOwner, HandleId)>,
}

impl MqHandleRegistry {
    pub fn new(epoch: u64, max_slots: usize) -> Result<Self, MqHandleProblem> {
        if epoch == 0 || max_slots == 0 || max_slots > MQ_MAX_HANDLE_SLOTS {
            return Err(MqHandleProblem::InvalidConfiguration);
        }
        let registry_id = NEXT_REGISTRY_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| MqHandleProblem::Capacity)?;
        Ok(Self {
            registry_id,
            epoch,
            max_slots,
            slots: Vec::new(),
            active: 0,
            defaults: Vec::new(),
        })
    }

    #[must_use]
    pub fn active_handles(&self) -> usize {
        self.active
    }

    /// Lifetime-only observation for reclaiming provider data after retirement.
    /// This does not check caller ownership, connection applicability or in-use
    /// state and must never substitute for operation authorization/validation.
    #[must_use]
    pub fn is_live(&self, handle: MqHandle) -> bool {
        self.entry(handle.id()).is_ok_and(|entry| {
            matches!(
                (entry.kind, handle.kind()),
                (EntryKind::Object, MqHandleKind::Object)
                    | (EntryKind::Subscription, MqHandleKind::Subscription)
                    | (EntryKind::Message, MqHandleKind::Message)
            )
        })
    }

    /// A new runtime epoch invalidates every old token, including default and
    /// unassociated handles. Epochs must increase; wraparound fails closed.
    pub fn advance_epoch(&mut self, next: u64) -> Result<(), MqHandleProblem> {
        if next <= self.epoch {
            return Err(MqHandleProblem::EpochNotAdvanced);
        }
        self.epoch = next;
        self.defaults.clear();
        self.active = 0;
        for slot in &mut self.slots {
            Self::retire_slot(slot);
        }
        Ok(())
    }

    /// Register an ordinary MQCONN/MQCONNX connection. The caller has already
    /// resolved the queue manager and option legality.
    pub fn connect(
        &mut self,
        owner: MqHandleOwner,
        sharing: MqHandleSharing,
    ) -> Result<MqHconn, MqHandleProblem> {
        Self::check_owner(owner)?;
        if owner.environment == MqHostEnvironment::ZosCics {
            return Err(MqHandleProblem::SpecialConnection);
        }
        let id = self.allocate(Entry {
            kind: EntryKind::Connection,
            owner,
            parent: None,
            sharing,
            in_use: false,
        })?;
        Ok(MqHconn::Issued(MqConnectionId(id)))
    }

    /// Bind the automatically available CICS task connection. MQDISC on this
    /// special handle is a no-op; ending the task invalidates it.
    pub fn bind_cics_default(&mut self, owner: MqHandleOwner) -> Result<MqHconn, MqHandleProblem> {
        Self::check_owner(owner)?;
        if owner.environment != MqHostEnvironment::ZosCics {
            return Err(MqHandleProblem::SpecialConnection);
        }
        if self
            .defaults
            .iter()
            .any(|(candidate, _)| candidate.same_unit(owner))
        {
            return Err(MqHandleProblem::AlreadyConnected);
        }
        let id = self.allocate(Entry {
            kind: EntryKind::Connection,
            owner,
            parent: None,
            sharing: MqHandleSharing::NonShared,
            in_use: false,
        })?;
        self.defaults.push((owner, id));
        Ok(MQHC_DEF_HCONN)
    }

    /// Validate an HCONN, resolving the CICS default only for its owning task.
    pub fn validate_connection(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<(), MqHandleProblem> {
        self.connection_id(owner, hconn).map(|_| ())
    }

    /// Create an MQOPEN/MQSUB destination object handle.
    pub fn create_object(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<MqHobj, MqHandleProblem> {
        self.create_child(owner, hconn, EntryKind::Object)
            .map(MqHobj)
    }

    /// Create an MQSUB Hsub. It is not interchangeable with Hobj.
    pub fn create_subscription(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<MqHsub, MqHandleProblem> {
        self.create_child(owner, hconn, EntryKind::Subscription)
            .map(MqHsub)
    }

    /// Create an associated Hmsg or use MQHC_UNASSOCIATED_HCONN. An unassociated
    /// handle requires a live connection in the creator's processing unit.
    pub fn create_message(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<MqHmsg, MqHandleProblem> {
        Self::check_owner(owner)?;
        let parent = match hconn {
            MqHconn::Unassociated => {
                self.require_unit_connection(owner)?;
                None
            }
            _ => Some(self.connection_id(owner, hconn)?),
        };
        let sharing = match parent {
            Some(id) => self.entry(id)?.sharing,
            None => MqHandleSharing::NonShared,
        };
        self.allocate(Entry {
            kind: EntryKind::Message,
            owner,
            parent,
            sharing,
            in_use: false,
        })
        .map(MqHmsg)
    }

    /// Validate an Hobj, Hsub, or associated Hmsg for the expected MQI role.
    /// Use `validate_message_property` for unassociated Hmsg instead.
    pub fn validate(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        handle: MqHandle,
        expected: MqHandleKind,
    ) -> Result<(), MqHandleProblem> {
        self.child_id(owner, hconn, handle, expected).map(|_| ())
    }

    /// Validate a message-property call. Its Hconn must be exactly the value
    /// used at MQCRTMH, including the unassociated special value.
    pub fn validate_message_property(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        handle: MqHandle,
    ) -> Result<(), MqHandleProblem> {
        if handle.kind() != MqHandleKind::Message {
            return Err(MqHandleProblem::WrongKind);
        }
        if hconn != MqHconn::Unassociated {
            return self.validate(owner, hconn, handle, MqHandleKind::Message);
        }
        Self::check_owner(owner)?;
        self.require_unit_connection(owner)?;
        let entry = self.entry(handle.id())?;
        if entry.kind != EntryKind::Message {
            return Err(MqHandleProblem::WrongKind);
        }
        if entry.parent.is_some() {
            return Err(MqHandleProblem::CrossConnection);
        }
        if !entry.owner.same_unit(owner) {
            return Err(MqHandleProblem::CrossOwner);
        }
        if entry.in_use {
            return Err(MqHandleProblem::InUse);
        }
        Ok(())
    }

    /// Reserve a message handle for MQGET/MQPUT/MQPUT1. An unassociated Hmsg
    /// may use any valid connection in its processing unit, one call at a time.
    pub fn begin_message_io(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hmsg: MqHmsg,
    ) -> Result<(), MqHandleProblem> {
        let connection = self.connection_id(owner, hconn)?;
        let entry = self.entry(hmsg.0)?;
        if entry.kind != EntryKind::Message {
            return Err(MqHandleProblem::WrongKind);
        }
        if let Some(parent) = entry.parent {
            if parent != connection {
                return Err(MqHandleProblem::CrossConnection);
            }
        } else if !entry.owner.same_unit(owner) {
            return Err(MqHandleProblem::CrossOwner);
        }
        if entry.in_use {
            return Err(MqHandleProblem::InUse);
        }
        self.entry_mut(hmsg.0)?.in_use = true;
        Ok(())
    }

    pub fn end_message_io(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hmsg: MqHmsg,
    ) -> Result<(), MqHandleProblem> {
        let connection = self.connection_id(owner, hconn)?;
        let entry = self.entry(hmsg.0)?;
        if entry.kind != EntryKind::Message {
            return Err(MqHandleProblem::WrongKind);
        }
        if let Some(parent) = entry.parent {
            if parent != connection {
                return Err(MqHandleProblem::CrossConnection);
            }
        } else if !entry.owner.same_unit(owner) {
            return Err(MqHandleProblem::CrossOwner);
        }
        if !entry.in_use {
            return Err(MqHandleProblem::Stale);
        }
        self.entry_mut(hmsg.0)?.in_use = false;
        Ok(())
    }

    /// Close an Hobj/Hsub or delete an Hmsg after its call succeeds. An
    /// unassociated Hmsg must be released with the unassociated Hconn value.
    pub fn release(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        handle: MqHandle,
        expected: MqHandleKind,
    ) -> Result<(), MqHandleProblem> {
        if handle.kind() != expected {
            return Err(MqHandleProblem::WrongKind);
        }
        let id = if hconn == MqHconn::Unassociated {
            if expected != MqHandleKind::Message {
                return Err(MqHandleProblem::SpecialConnection);
            }
            self.validate_message_property(owner, hconn, handle)?;
            handle.id()
        } else {
            self.child_id(owner, hconn, handle, expected)?
        };
        if self.entry(id)?.in_use {
            return Err(MqHandleProblem::InUse);
        }
        self.retire(id);
        Ok(())
    }

    /// Disconnect an issued connection and invalidate every associated Hobj,
    /// Hsub, and Hmsg. An unassociated Hmsg survives for explicit deletion.
    pub fn disconnect(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<(), MqHandleProblem> {
        let id = self.connection_id(owner, hconn)?;
        if hconn == MqHconn::Default {
            return Ok(());
        }
        if self.slots.iter().any(|slot| {
            slot.entry
                .is_some_and(|entry| entry.parent == Some(id) && entry.in_use)
        }) {
            return Err(MqHandleProblem::InUse);
        }
        self.invalidate_connection(id);
        Ok(())
    }

    /// End a nonshared task/thread or IMS syncpoint scope. Shared connections
    /// survive thread end; the process/runtime owner must end them separately.
    pub fn end_processing_unit(&mut self, owner: MqHandleOwner) -> Result<(), MqHandleProblem> {
        Self::check_owner(owner)?;
        let ids: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                let entry = item.entry?;
                (entry.kind == EntryKind::Connection
                    && entry.sharing == MqHandleSharing::NonShared
                    && entry.owner.same_unit(owner))
                .then_some(HandleId {
                    registry: self.registry_id,
                    slot: slot as u32,
                    generation: item.generation,
                    epoch: self.epoch,
                    historical: false,
                })
            })
            .collect();
        for id in ids {
            self.invalidate_connection(id);
        }
        let messages: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                let entry = item.entry?;
                (entry.kind == EntryKind::Message
                    && entry.parent.is_none()
                    && entry.owner.same_unit(owner))
                .then_some(HandleId {
                    registry: self.registry_id,
                    slot: slot as u32,
                    generation: item.generation,
                    epoch: self.epoch,
                    historical: false,
                })
            })
            .collect();
        for id in messages {
            self.retire(id);
        }
        Ok(())
    }

    /// Host-owned process termination, not MQDISC or application authorization.
    /// Retires shared connections and every volatile child/unassociated handle
    /// in this exact environment/host/process, including in-flight handles.
    /// The service's existing UOW coordinator must resolve durable work separately.
    pub fn end_process(&mut self, owner: MqHandleOwner) -> Result<(), MqHandleProblem> {
        Self::check_owner(owner)?;
        let belongs = |candidate: MqHandleOwner| {
            candidate.environment == owner.environment
                && candidate.host_id == owner.host_id
                && candidate.process_id == owner.process_id
        };
        for slot in &mut self.slots {
            if slot.entry.is_some_and(|entry| belongs(entry.owner)) {
                Self::retire_slot(slot);
                self.active -= 1;
            }
        }
        self.defaults.retain(|(candidate, _)| !belongs(*candidate));
        Ok(())
    }

    fn check_owner(owner: MqHandleOwner) -> Result<(), MqHandleProblem> {
        if owner.valid() {
            Ok(())
        } else {
            Err(MqHandleProblem::InvalidOwner)
        }
    }

    fn create_child(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        kind: EntryKind,
    ) -> Result<HandleId, MqHandleProblem> {
        let parent = self.connection_id(owner, hconn)?;
        let sharing = self.entry(parent)?.sharing;
        self.allocate(Entry {
            kind,
            owner,
            parent: Some(parent),
            sharing,
            in_use: false,
        })
    }

    fn connection_id(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<HandleId, MqHandleProblem> {
        Self::check_owner(owner)?;
        let id = match hconn {
            MqHconn::Issued(value) => value.0,
            MqHconn::Default => {
                if owner.environment != MqHostEnvironment::ZosCics {
                    return Err(MqHandleProblem::SpecialConnection);
                }
                self.defaults
                    .iter()
                    .find(|(candidate, _)| candidate.same_unit(owner))
                    .map(|(_, id)| *id)
                    .ok_or(MqHandleProblem::MissingConnection)?
            }
            MqHconn::Unassociated => return Err(MqHandleProblem::SpecialConnection),
        };
        let entry = self.entry(id)?;
        if entry.kind != EntryKind::Connection {
            return Err(MqHandleProblem::WrongKind);
        }
        if !entry.owner.permits(owner, entry.sharing) {
            return Err(MqHandleProblem::CrossOwner);
        }
        Ok(id)
    }

    fn require_unit_connection(&self, owner: MqHandleOwner) -> Result<(), MqHandleProblem> {
        let found = self.slots.iter().any(|slot| {
            slot.entry.is_some_and(|entry| {
                entry.kind == EntryKind::Connection
                    && entry.owner.thread_id == owner.thread_id
                    && entry.owner.same_unit(owner)
            })
        });
        if found {
            Ok(())
        } else {
            Err(MqHandleProblem::MissingConnection)
        }
    }

    fn child_id(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        handle: MqHandle,
        expected: MqHandleKind,
    ) -> Result<HandleId, MqHandleProblem> {
        if handle.kind() != expected {
            return Err(MqHandleProblem::WrongKind);
        }
        let parent = self.connection_id(owner, hconn)?;
        let entry = self.entry(handle.id())?;
        let actual = match entry.kind {
            EntryKind::Connection => return Err(MqHandleProblem::WrongKind),
            EntryKind::Object => MqHandleKind::Object,
            EntryKind::Subscription => MqHandleKind::Subscription,
            EntryKind::Message => MqHandleKind::Message,
        };
        if actual != expected {
            return Err(MqHandleProblem::WrongKind);
        }
        if entry.parent != Some(parent) {
            return Err(MqHandleProblem::CrossConnection);
        }
        if !entry.owner.permits(owner, entry.sharing) {
            return Err(MqHandleProblem::CrossOwner);
        }
        if entry.in_use {
            return Err(MqHandleProblem::InUse);
        }
        Ok(handle.id())
    }

    fn entry(&self, id: HandleId) -> Result<&Entry, MqHandleProblem> {
        if id.historical {
            return Err(MqHandleProblem::Historical);
        }
        if id.registry != self.registry_id || id.epoch != self.epoch {
            return Err(MqHandleProblem::Stale);
        }
        let slot = self
            .slots
            .get(id.slot as usize)
            .ok_or(MqHandleProblem::Stale)?;
        if slot.generation != id.generation {
            return Err(MqHandleProblem::Stale);
        }
        slot.entry.as_ref().ok_or(MqHandleProblem::Stale)
    }

    fn entry_mut(&mut self, id: HandleId) -> Result<&mut Entry, MqHandleProblem> {
        if id.historical {
            return Err(MqHandleProblem::Historical);
        }
        if id.registry != self.registry_id || id.epoch != self.epoch {
            return Err(MqHandleProblem::Stale);
        }
        let slot = self
            .slots
            .get_mut(id.slot as usize)
            .ok_or(MqHandleProblem::Stale)?;
        if slot.generation != id.generation {
            return Err(MqHandleProblem::Stale);
        }
        slot.entry.as_mut().ok_or(MqHandleProblem::Stale)
    }

    fn allocate(&mut self, entry: Entry) -> Result<HandleId, MqHandleProblem> {
        if self.active == self.max_slots {
            return Err(MqHandleProblem::Capacity);
        }
        let index = if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.entry.is_none() && slot.generation != 0)
        {
            index
        } else {
            if self.slots.len() == self.max_slots {
                return Err(MqHandleProblem::Capacity);
            }
            self.slots.push(Slot {
                generation: 1,
                entry: None,
            });
            self.slots.len() - 1
        };
        let slot = &mut self.slots[index];
        slot.entry = Some(entry);
        self.active += 1;
        Ok(HandleId {
            registry: self.registry_id,
            slot: index as u32,
            generation: slot.generation,
            epoch: self.epoch,
            historical: false,
        })
    }

    fn retire_slot(slot: &mut Slot) {
        slot.entry = None;
        slot.generation = slot.generation.checked_add(1).unwrap_or(0);
    }

    fn retire(&mut self, id: HandleId) {
        let slot = &mut self.slots[id.slot as usize];
        debug_assert!(slot.entry.is_some() && slot.generation == id.generation);
        Self::retire_slot(slot);
        self.active -= 1;
    }

    fn invalidate_connection(&mut self, id: HandleId) {
        let children: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                item.entry
                    .filter(|entry| entry.parent == Some(id))
                    .map(|_| HandleId {
                        registry: self.registry_id,
                        slot: slot as u32,
                        generation: item.generation,
                        epoch: self.epoch,
                        historical: false,
                    })
            })
            .collect();
        for child in children {
            self.retire(child);
        }
        self.defaults.retain(|(_, candidate)| *candidate != id);
        self.retire(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_mqi_contract_by_label;

    fn owner(environment: MqHostEnvironment) -> MqHandleOwner {
        MqHandleOwner {
            environment,
            host_id: 1,
            process_id: 2,
            thread_id: 3,
            task_id: 4,
            syncpoint_epoch: 5,
        }
    }

    fn registry(max: usize) -> MqHandleRegistry {
        MqHandleRegistry::new(7, max).unwrap()
    }

    #[test]
    fn lifetime_observation_is_not_operation_permission() {
        let mut registry = registry(8);
        let who = owner(MqHostEnvironment::ZosBatch);
        let conn = registry.connect(who, MqHandleSharing::NonShared).unwrap();
        let associated = registry.create_message(who, conn).unwrap();
        let unassociated = registry.create_message(who, MqHconn::Unassociated).unwrap();
        registry.begin_message_io(who, conn, associated).unwrap();
        assert!(registry.is_live(associated.into()));
        assert_eq!(
            registry.validate_message_property(who, conn, associated.into()),
            Err(MqHandleProblem::InUse)
        );
        registry.end_message_io(who, conn, associated).unwrap();
        registry.disconnect(who, conn).unwrap();
        assert!(!registry.is_live(associated.into()));
        assert!(registry.is_live(unassociated.into()));
        assert_eq!(
            registry.validate_message_property(who, MqHconn::Unassociated, unassociated.into()),
            Err(MqHandleProblem::MissingConnection)
        );
        let other = MqHandleRegistry::new(7, 8).unwrap();
        assert!(!other.is_live(unassociated.into()));
        registry.advance_epoch(8).unwrap();
        assert!(!registry.is_live(unassociated.into()));
    }

    #[test]
    fn pinned_calls_bind_handle_creation_use_and_release_roles() {
        for label in [
            "MQCONN", "MQCONNX", "MQDISC", "MQOPEN", "MQCLOSE", "MQCRTMH", "MQDLTMH", "MQSUB",
            "MQSUBRQ", "MQCB", "MQCTL",
        ] {
            let call = mq_mqi_contract_by_label(label).unwrap();
            assert_eq!(
                call.official_row.split(':').next(),
                Some("ibm-mq-9.4-mqi-2026-08-31")
            );
            assert!(
                call.parameters
                    .iter()
                    .any(|parameter| parameter.handle_role.is_some())
            );
        }
        let sub = mq_mqi_contract_by_label("MQSUB").unwrap();
        let hsub = sub.parameter("Hsub").unwrap();
        assert_eq!(hsub.data_type, "MQHOBJ");
        assert_eq!(hsub.handle_role, Some(crate::MqMqiHandleRole::Subscription));
    }

    #[test]
    fn typed_roles_and_connection_association_reject_wrong_use_without_mutation() {
        let mut r = registry(8);
        let a = owner(MqHostEnvironment::MqiClient);
        let first = r.connect(a, MqHandleSharing::NonShared).unwrap();
        let second = r.connect(a, MqHandleSharing::NonShared).unwrap();
        let object = r.create_object(a, first).unwrap();
        let sub = r.create_subscription(a, first).unwrap();
        let message = r.create_message(a, first).unwrap();
        assert_eq!(
            r.validate(a, first, object.into(), MqHandleKind::Subscription),
            Err(MqHandleProblem::WrongKind)
        );
        assert_eq!(
            r.validate(a, first, sub.into(), MqHandleKind::Object),
            Err(MqHandleProblem::WrongKind)
        );
        assert_eq!(
            r.validate(a, second, object.into(), MqHandleKind::Object),
            Err(MqHandleProblem::CrossConnection)
        );
        assert_eq!(
            r.validate_message_property(a, second, message.into()),
            Err(MqHandleProblem::CrossConnection)
        );
        assert_eq!(
            r.release(a, second, sub.into(), MqHandleKind::Subscription),
            Err(MqHandleProblem::CrossConnection)
        );
        assert_eq!(r.active_handles(), 5);
        for (handle, kind) in [
            (object.into(), MqHandleKind::Object),
            (sub.into(), MqHandleKind::Subscription),
            (message.into(), MqHandleKind::Message),
        ] {
            r.validate(a, first, handle, kind).unwrap();
            r.release(a, first, handle, kind).unwrap();
            assert_eq!(
                r.validate(a, first, handle, kind),
                Err(MqHandleProblem::Stale)
            );
        }
    }

    #[test]
    fn owner_scope_matches_host_process_task_thread_and_ims_syncpoint() {
        let mut r = registry(16);
        let base = owner(MqHostEnvironment::MqiClient);
        let nonshared = r.connect(base, MqHandleSharing::NonShared).unwrap();
        let shared = r.connect(base, MqHandleSharing::SharedNoBlock).unwrap();
        let object = r.create_object(base, shared).unwrap();
        let mut other_thread = base;
        other_thread.thread_id += 1;
        assert_eq!(
            r.validate_connection(other_thread, nonshared),
            Err(MqHandleProblem::CrossOwner)
        );
        r.validate_connection(other_thread, shared).unwrap();
        r.validate(other_thread, shared, object.into(), MqHandleKind::Object)
            .unwrap();
        r.end_processing_unit(base).unwrap();
        assert_eq!(
            r.validate_connection(base, nonshared),
            Err(MqHandleProblem::Stale)
        );
        r.validate_connection(other_thread, shared).unwrap();
        let mut other_process = other_thread;
        other_process.process_id += 1;
        assert_eq!(
            r.validate_connection(other_process, shared),
            Err(MqHandleProblem::CrossOwner)
        );
        let mut other_host = other_thread;
        other_host.host_id += 1;
        assert_eq!(
            r.validate_connection(other_host, shared),
            Err(MqHandleProblem::CrossOwner)
        );

        let batch = owner(MqHostEnvironment::ZosBatch);
        let task = r.connect(batch, MqHandleSharing::NonShared).unwrap();
        let mut batch_thread = batch;
        batch_thread.thread_id += 1;
        r.validate_connection(batch_thread, task).unwrap();
        batch_thread.task_id += 1;
        assert_eq!(
            r.validate_connection(batch_thread, task),
            Err(MqHandleProblem::CrossOwner)
        );

        let ims = owner(MqHostEnvironment::ZosIms);
        let ims_handle = r.connect(ims, MqHandleSharing::NonShared).unwrap();
        let mut after_syncpoint = ims;
        after_syncpoint.syncpoint_epoch += 1;
        assert_eq!(
            r.validate_connection(after_syncpoint, ims_handle),
            Err(MqHandleProblem::CrossOwner)
        );
        r.end_processing_unit(ims).unwrap();
        assert_eq!(
            r.validate_connection(ims, ims_handle),
            Err(MqHandleProblem::Stale)
        );
    }

    #[test]
    fn disconnect_cascades_but_unassociated_message_survives() {
        let mut r = registry(8);
        let a = owner(MqHostEnvironment::OtherBindings);
        assert_eq!(
            r.create_message(a, MQHC_UNASSOCIATED_HCONN),
            Err(MqHandleProblem::MissingConnection)
        );
        let connection = r.connect(a, MqHandleSharing::NonShared).unwrap();
        let object = r.create_object(a, connection).unwrap();
        let sub = r.create_subscription(a, connection).unwrap();
        let associated = r.create_message(a, connection).unwrap();
        let unassociated = r.create_message(a, MQHC_UNASSOCIATED_HCONN).unwrap();
        assert_eq!(
            r.validate_message_property(a, connection, unassociated.into()),
            Err(MqHandleProblem::CrossConnection)
        );
        assert_eq!(
            r.validate_message_property(a, MQHC_UNASSOCIATED_HCONN, associated.into()),
            Err(MqHandleProblem::CrossConnection)
        );
        let mut other_thread = a;
        other_thread.thread_id += 1;
        assert_eq!(
            r.validate_message_property(other_thread, MQHC_UNASSOCIATED_HCONN, unassociated.into()),
            Err(MqHandleProblem::MissingConnection)
        );
        r.disconnect(a, connection).unwrap();
        assert_eq!(r.active_handles(), 1);
        assert_eq!(
            r.validate_connection(a, connection),
            Err(MqHandleProblem::Stale)
        );
        for (handle, kind) in [
            (object.into(), MqHandleKind::Object),
            (sub.into(), MqHandleKind::Subscription),
            (associated.into(), MqHandleKind::Message),
        ] {
            let new_connection = r.connect(a, MqHandleSharing::NonShared).unwrap();
            assert_eq!(
                r.validate(a, new_connection, handle, kind),
                Err(MqHandleProblem::Stale)
            );
            r.disconnect(a, new_connection).unwrap();
        }
        assert_eq!(
            r.validate_message_property(a, MQHC_UNASSOCIATED_HCONN, unassociated.into()),
            Err(MqHandleProblem::MissingConnection)
        );
        let new_connection = r.connect(a, MqHandleSharing::NonShared).unwrap();
        r.begin_message_io(a, new_connection, unassociated).unwrap();
        assert_eq!(
            r.begin_message_io(a, new_connection, unassociated),
            Err(MqHandleProblem::InUse)
        );
        assert_eq!(
            r.release(
                a,
                MQHC_UNASSOCIATED_HCONN,
                unassociated.into(),
                MqHandleKind::Message
            ),
            Err(MqHandleProblem::InUse)
        );
        r.end_message_io(a, new_connection, unassociated).unwrap();
        r.release(
            a,
            MQHC_UNASSOCIATED_HCONN,
            unassociated.into(),
            MqHandleKind::Message,
        )
        .unwrap();
    }

    #[test]
    fn cics_default_is_task_bound_and_disc_does_not_drop_it() {
        let mut r = registry(4);
        let cics = owner(MqHostEnvironment::ZosCics);
        assert_eq!(
            r.connect(cics, MqHandleSharing::NonShared),
            Err(MqHandleProblem::SpecialConnection)
        );
        assert_eq!(
            r.bind_cics_default(owner(MqHostEnvironment::ZosBatch)),
            Err(MqHandleProblem::SpecialConnection)
        );
        assert_eq!(
            r.validate_connection(cics, MQHC_DEF_HCONN),
            Err(MqHandleProblem::MissingConnection)
        );
        assert_eq!(r.bind_cics_default(cics), Ok(MQHC_DEF_HCONN));
        assert_eq!(
            r.bind_cics_default(cics),
            Err(MqHandleProblem::AlreadyConnected)
        );
        let object = r.create_object(cics, MQHC_DEF_HCONN).unwrap();
        let mut another_task = cics;
        another_task.task_id += 1;
        assert_eq!(
            r.validate_connection(another_task, MQHC_DEF_HCONN),
            Err(MqHandleProblem::MissingConnection)
        );
        r.disconnect(cics, MQHC_DEF_HCONN).unwrap();
        r.validate(cics, MQHC_DEF_HCONN, object.into(), MqHandleKind::Object)
            .unwrap();
        r.end_processing_unit(cics).unwrap();
        assert_eq!(
            r.validate_connection(cics, MQHC_DEF_HCONN),
            Err(MqHandleProblem::MissingConnection)
        );
    }

    #[test]
    fn capacity_generation_and_epoch_fail_closed() {
        assert!(matches!(
            MqHandleRegistry::new(0, 1),
            Err(MqHandleProblem::InvalidConfiguration)
        ));
        assert!(matches!(
            MqHandleRegistry::new(1, MQ_MAX_HANDLE_SLOTS + 1),
            Err(MqHandleProblem::InvalidConfiguration)
        ));
        let mut r = registry(2);
        let a = owner(MqHostEnvironment::MqiClient);
        let connection = r.connect(a, MqHandleSharing::NonShared).unwrap();
        let old = r.create_object(a, connection).unwrap();
        assert_eq!(
            r.create_object(a, connection),
            Err(MqHandleProblem::Capacity)
        );
        r.release(a, connection, old.into(), MqHandleKind::Object)
            .unwrap();
        let fresh = r.create_object(a, connection).unwrap();
        assert_ne!(old, fresh);
        assert_eq!(
            r.validate(a, connection, old.into(), MqHandleKind::Object),
            Err(MqHandleProblem::Stale)
        );
        assert_eq!(r.advance_epoch(7), Err(MqHandleProblem::EpochNotAdvanced));
        r.advance_epoch(8).unwrap();
        assert_eq!(r.active_handles(), 0);
        assert_eq!(
            r.validate_connection(a, connection),
            Err(MqHandleProblem::Stale)
        );
        let replacement = r.connect(a, MqHandleSharing::NonShared).unwrap();
        assert_ne!(connection, replacement);
        assert_eq!(
            r.validate(a, replacement, fresh.into(), MqHandleKind::Object),
            Err(MqHandleProblem::Stale)
        );
        let other = registry(2);
        assert_eq!(
            other.validate_connection(a, replacement),
            Err(MqHandleProblem::Stale)
        );
    }

    #[test]
    fn active_message_io_blocks_disconnect_without_partial_invalidation() {
        let mut r = registry(3);
        let a = owner(MqHostEnvironment::MqiClient);
        let connection = r.connect(a, MqHandleSharing::NonShared).unwrap();
        let object = r.create_object(a, connection).unwrap();
        let message = r.create_message(a, connection).unwrap();
        r.begin_message_io(a, connection, message).unwrap();
        assert_eq!(r.disconnect(a, connection), Err(MqHandleProblem::InUse));
        assert_eq!(r.active_handles(), 3);
        r.validate(a, connection, object.into(), MqHandleKind::Object)
            .unwrap();
        r.end_message_io(a, connection, message).unwrap();
        r.disconnect(a, connection).unwrap();
        assert_eq!(r.active_handles(), 0);
    }
}
