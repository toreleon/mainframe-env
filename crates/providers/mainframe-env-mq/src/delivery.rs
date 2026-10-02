//! Pure, bounded point-to-point MQ queue-manager transitions.

pub(crate) mod checkpoint;
pub(crate) mod full_message;
pub(crate) mod replay;
mod restart;
mod source_backout;
use full_message::{Payload, PayloadGet, QueueProfile, QueueState};

use crate::{
    MqObjectCapability, MqObjectCatalog, MqObjectDefinition, MqObjectError, MqObjectLookup,
    MqObjectName, MqResolvedTarget,
};
use mainframe_env_host_api::{
    MqDeliveryOutcome, MqDistributionItemResult, MqDistributionResult, MqExpiry, MqGetContract,
    MqGetDisposition, MqGetMode, MqMessage, MqMessageDescriptor, MqMessageIdentifiers,
    MqMessageLimits, MqMessageOrdering, MqMessageProblem, MqMessageProperty, MqPersistence,
    MqPriority, MqPropertyType, MqTruncation, MqTruncationDisposition, MqWait,
};
use std::collections::BTreeMap;

pub const MQ_DELIVERY_SCHEMA: &str = "mainframe-env.mq-delivery@1";

/// Product resource guards; smaller values may be selected by the owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqDeliveryLimits {
    pub queues: usize,
    pub depth_per_queue: usize,
    pub total_bytes: usize,
    pub pending_operations: usize,
    pub cursors: usize,
    pub finalized_units: usize,
    pub snapshot_bytes: usize,
}

impl Default for MqDeliveryLimits {
    fn default() -> Self {
        Self {
            queues: 4_096,
            depth_per_queue: 10_000,
            total_bytes: 64 * 1024 * 1024,
            pending_operations: 10_000,
            cursors: 4_096,
            finalized_units: 10_000,
            snapshot_bytes: 64 * 1024 * 1024,
        }
    }
}

impl MqDeliveryLimits {
    fn validate(self) -> Result<(), MqDeliveryError> {
        let ceiling = Self::default();
        let values = [
            (self.queues, ceiling.queues),
            (self.depth_per_queue, ceiling.depth_per_queue),
            (self.total_bytes, ceiling.total_bytes),
            (self.pending_operations, ceiling.pending_operations),
            (self.cursors, ceiling.cursors),
            (self.finalized_units, ceiling.finalized_units),
            (self.snapshot_bytes, ceiling.snapshot_bytes),
        ];
        if values
            .iter()
            .any(|(value, maximum)| *value == 0 || value > maximum)
        {
            return Err(MqDeliveryError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqDeliveryError {
    InvalidLimits,
    Message(MqMessageProblem),
    Object(MqObjectError),
    Unsupported,
    UnknownQueue,
    InvalidUnit,
    InvalidCursor,
    Group,
    Segment,
    ResourceExhausted,
    ClockRegression,
    CorruptSnapshot,
    UnsupportedSchema,
}

impl From<MqMessageProblem> for MqDeliveryError {
    fn from(problem: MqMessageProblem) -> Self {
        Self::Message(problem)
    }
}

impl From<MqObjectError> for MqDeliveryError {
    fn from(problem: MqObjectError) -> Self {
        Self::Object(problem)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDeliveryGet {
    pub disposition: MqGetDisposition,
    /// Descriptor and properties are preserved; body contains only copied bytes.
    pub message: Option<MqMessage>,
    pub cursor: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    id: u64,
    message: Payload,
    expires_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Pending {
    Put { queue: MqObjectName, entry: Entry },
    Get { queue: MqObjectName, entry: Entry },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Cursor {
    queue: MqObjectName,
    entry_id: u64,
}

/// This kernel owns only in-process deterministic transitions. The service
/// owner supplies validated handles, SAF decisions, effect intents and CAS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDeliveryKernel {
    manager: MqObjectName,
    queues: BTreeMap<MqObjectName, QueueState>,
    schema_two: bool,
    pending: BTreeMap<u64, Vec<Pending>>,
    finalized: BTreeMap<u64, bool>,
    cursors: BTreeMap<u64, Cursor>,
    next_id: u64,
    next_cursor: u64,
    tick: u64,
    limits: MqDeliveryLimits,
    message_limits: MqMessageLimits,
    default_persistence: MqPersistence,
}

impl MqDeliveryKernel {
    pub fn new(
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        default_persistence: MqPersistence,
    ) -> Result<Self, MqDeliveryError> {
        limits.validate()?;
        message_limits.validate()?;
        if !matches!(
            default_persistence,
            MqPersistence::Persistent | MqPersistence::NonPersistent
        ) {
            return Err(MqDeliveryError::Unsupported);
        }
        let mut queues = BTreeMap::new();
        for definition in catalog.definitions() {
            if let MqObjectDefinition::LocalQueue { name, .. } = definition {
                queues.insert(name.clone(), Vec::new().into());
            }
        }
        for instance in catalog.model_instances() {
            queues.insert(instance.name.clone(), Vec::new().into());
        }
        if queues.len() > limits.queues {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        Ok(Self {
            manager: catalog.queue_manager().name.clone(),
            queues,
            schema_two: false,
            pending: BTreeMap::new(),
            finalized: BTreeMap::new(),
            cursors: BTreeMap::new(),
            next_id: 1,
            next_cursor: 1,
            tick: 0,
            limits,
            message_limits,
            default_persistence,
        })
    }

    #[must_use]
    pub fn depth(&self, queue: &MqObjectName) -> Option<usize> {
        self.queues.get(queue).map(|q| q.len())
    }

    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// An absent decision is unknown, never a request to redispatch mutation.
    #[must_use]
    pub fn unit_outcome(&self, unit: u64) -> MqDeliveryOutcome {
        if self.pending.contains_key(&unit) {
            MqDeliveryOutcome::Pending
        } else {
            match self.finalized.get(&unit) {
                Some(true) => MqDeliveryOutcome::DuplicatePossible,
                Some(false) => MqDeliveryOutcome::Rejected,
                None => MqDeliveryOutcome::UnknownOutcome,
            }
        }
    }

    pub fn put_one(
        &mut self,
        catalog: &MqObjectCatalog,
        destination: &MqObjectName,
        message: MqMessage,
        unit: Option<u64>,
    ) -> Result<MqDeliveryOutcome, MqDeliveryError> {
        let result = self.put(catalog, std::slice::from_ref(destination), message, unit)?;
        Ok(result.items[0].outcome.clone())
    }

    /// All destinations are local resolved queues. Capacity and coherence are
    /// checked on a private candidate before any item becomes visible.
    pub fn put(
        &mut self,
        catalog: &MqObjectCatalog,
        destinations: &[MqObjectName],
        mut message: MqMessage,
        unit: Option<u64>,
    ) -> Result<MqDistributionResult, MqDeliveryError> {
        message.validate(self.message_limits)?;
        validate_supported_message(&message)?;
        validate_unit(self, unit)?;
        if destinations.is_empty() || destinations.len() > self.message_limits.distribution_items {
            return Err(MqDeliveryError::Message(MqMessageProblem::Distribution));
        }
        let targets = destinations
            .iter()
            .map(|name| self.resolve(catalog, name, MqObjectCapability::Output))
            .collect::<Result<Vec<_>, _>>()?;
        if targets
            .iter()
            .any(|q| self.queues[q].profile != QueueProfile::Partial)
        {
            return Err(MqDeliveryError::Unsupported);
        }
        let mut next = self.clone();
        if message.descriptor.identifiers.message_id.is_none() {
            let mut id = vec![0; 24];
            id[16..].copy_from_slice(&next.next_id.to_be_bytes());
            message.descriptor.identifiers.message_id = Some(id);
        }
        let persistence =
            effective_persistence(message.descriptor.persistence, self.default_persistence)?;
        message.descriptor.persistence = persistence;
        let expires_at = match message.descriptor.expiry {
            MqExpiry::RelativeHostTicks(ticks) => Some(
                self.tick
                    .checked_add(ticks)
                    .ok_or(MqDeliveryError::ResourceExhausted)?,
            ),
            MqExpiry::Unlimited => None,
            MqExpiry::PendingSource => return Err(MqDeliveryError::Unsupported),
        };
        for target in targets {
            let id = next.allocate_id()?;
            let entry = Entry {
                id,
                message: Payload::Partial(message.clone()),
                expires_at,
            };
            if let Some(unit) = unit {
                let mut visible = next
                    .queues
                    .get(&target)
                    .ok_or(MqDeliveryError::UnknownQueue)?
                    .clone();
                if let Some(operations) = next.pending.get(&unit) {
                    for operation in operations {
                        if let Pending::Put { queue, entry } = operation
                            && queue == &target
                        {
                            visible.push(entry.clone());
                        }
                    }
                }
                validate_group_and_segment(&visible, &entry, false)?;
                next.pending.entry(unit).or_default().push(Pending::Put {
                    queue: target,
                    entry,
                });
            } else {
                let queue = next
                    .queues
                    .get_mut(&target)
                    .ok_or(MqDeliveryError::UnknownQueue)?;
                validate_group_and_segment(queue, &entry, false)?;
                queue.push(entry);
            }
        }
        next.check_bounds()?;
        if next.schema_two {
            next.encode_live_checkpoint()?;
        }
        let result = MqDistributionResult {
            items: destinations
                .iter()
                .map(|name| MqDistributionItemResult {
                    destination: name.as_str().into(),
                    outcome: if unit.is_some() {
                        MqDeliveryOutcome::Pending
                    } else {
                        MqDeliveryOutcome::Accepted
                    },
                })
                .collect(),
        };
        result.validate(self.message_limits)?;
        *self = next;
        Ok(result)
    }

    pub fn get(
        &mut self,
        catalog: &MqObjectCatalog,
        queue: &MqObjectName,
        request: &MqGetContract,
        unit: Option<u64>,
    ) -> Result<MqDeliveryGet, MqDeliveryError> {
        let got = self.get_payload(catalog, queue, request, unit, QueueProfile::Partial)?;
        Ok(MqDeliveryGet {
            disposition: got.disposition,
            cursor: got.cursor,
            message: got.message.map(|p| match p {
                Payload::Partial(m) => m,
                Payload::Complete(_) => unreachable!("profile checked before mutation"),
            }),
        })
    }

    fn get_payload(
        &mut self,
        catalog: &MqObjectCatalog,
        queue: &MqObjectName,
        request: &MqGetContract,
        unit: Option<u64>,
        profile: QueueProfile,
    ) -> Result<PayloadGet, MqDeliveryError> {
        request.validate(self.message_limits)?;
        validate_unit(self, unit)?;
        let capability = match request.mode {
            MqGetMode::BrowseFirst | MqGetMode::BrowseNext { .. } => MqObjectCapability::Browse,
            _ => MqObjectCapability::Input,
        };
        let target = self.resolve(catalog, queue, capability)?;
        if self.queues[&target].profile != profile {
            return Err(MqDeliveryError::Unsupported);
        }
        let cursor = match request.mode {
            MqGetMode::BrowseNext { cursor } | MqGetMode::RemoveUnderCursor { cursor } => {
                let current = self
                    .cursors
                    .get(&cursor)
                    .ok_or(MqDeliveryError::InvalidCursor)?;
                if current.queue != target {
                    return Err(MqDeliveryError::InvalidCursor);
                }
                Some((cursor, current.entry_id))
            }
            _ => None,
        };
        let entries = self
            .queues
            .get(&target)
            .ok_or(MqDeliveryError::UnknownQueue)?;
        let position = entries.iter().position(|entry| {
            let in_range = match request.mode {
                MqGetMode::BrowseNext { .. } => entry.id > cursor.expect("checked cursor").1,
                MqGetMode::RemoveUnderCursor { .. } => {
                    entry.id == cursor.expect("checked cursor").1
                }
                _ => true,
            };
            in_range && entry.message.matches(&request.selection.identifiers)
        });
        let staged_position = if position.is_none() && matches!(request.mode, MqGetMode::Remove) {
            unit.and_then(|unit| self.pending.get(&unit)).and_then(|operations| operations.iter().position(|operation| {
                matches!(operation, Pending::Put { queue, entry } if queue == &target && entry.message.matches(&request.selection.identifiers))
            }))
        } else {
            None
        };
        if position.is_none() && staged_position.is_none() {
            if matches!(request.mode, MqGetMode::RemoveUnderCursor { .. }) {
                return Err(MqDeliveryError::InvalidCursor);
            }
            return Ok(PayloadGet {
                disposition: if request.wait == MqWait::NoWait {
                    MqGetDisposition::NoMessage
                } else {
                    MqGetDisposition::WaitExpired
                },
                message: None,
                cursor: None,
            });
        }
        let entry = if let Some(position) = position {
            entries[position].clone()
        } else {
            match &self.pending[&unit.expect("staged selection has unit")]
                [staged_position.expect("staged position")]
            {
                Pending::Put { entry, .. } => entry.clone(),
                _ => unreachable!("staged selection is a put"),
            }
        };
        let required = entry.message.body().len();
        let copied = required.min(request.buffer_capacity);
        let truncated = required > request.buffer_capacity;
        let removes = matches!(
            request.mode,
            MqGetMode::Remove | MqGetMode::RemoveUnderCursor { .. }
        );
        let disposition = if truncated {
            if request.truncation == MqTruncation::Reject {
                MqTruncationDisposition::RejectedRetained { required, copied }
            } else if removes {
                MqTruncationDisposition::AcceptedRemoved { required, copied }
            } else {
                MqTruncationDisposition::AcceptedBrowsed { required, copied }
            }
        } else {
            MqTruncationDisposition::Complete { length: required }
        };
        let mut returned = entry.message.clone();
        returned.body_mut().truncate(copied);
        if truncated && request.truncation == MqTruncation::Reject {
            return Ok(PayloadGet {
                disposition: MqGetDisposition::Message(disposition),
                message: Some(returned),
                cursor: None,
            });
        }
        let mut next = self.clone();
        let mut result_cursor = None;
        if removes {
            if let Some(position) = position {
                next.queues
                    .get_mut(&target)
                    .expect("resolved queue")
                    .remove(position);
                if let Some(unit) = unit {
                    next.pending.entry(unit).or_default().push(Pending::Get {
                        queue: target,
                        entry,
                    });
                }
            } else {
                next.pending
                    .get_mut(&unit.expect("staged selection has unit"))
                    .expect("staged unit")
                    .remove(staged_position.expect("staged position"));
            }
        } else {
            let token = match cursor {
                Some((token, _)) => token,
                None => next.allocate_cursor()?,
            };
            next.cursors.insert(
                token,
                Cursor {
                    queue: target,
                    entry_id: entry.id,
                },
            );
            result_cursor = Some(token);
        }
        next.check_bounds()?;
        if next.schema_two {
            next.encode_live_checkpoint()?;
        }
        *self = next;
        Ok(PayloadGet {
            disposition: MqGetDisposition::Message(disposition),
            message: Some(returned),
            cursor: result_cursor,
        })
    }

    pub fn commit(&mut self, unit: u64) -> Result<MqDeliveryOutcome, MqDeliveryError> {
        if unit == 0 {
            return Err(MqDeliveryError::InvalidUnit);
        }
        if let Some(decision) = self.finalized.get(&unit) {
            return Ok(if *decision {
                MqDeliveryOutcome::DuplicatePossible
            } else {
                MqDeliveryOutcome::UnknownOutcome
            });
        }
        let Some(operations) = self.pending.get(&unit) else {
            return Ok(MqDeliveryOutcome::UnknownOutcome);
        };
        let mut next = self.clone();
        for operation in operations {
            if let Pending::Put { queue, entry } = operation {
                if entry.expires_at.is_some_and(|expiry| expiry <= self.tick) {
                    continue;
                }
                let entries = next
                    .queues
                    .get_mut(queue)
                    .ok_or(MqDeliveryError::UnknownQueue)?;
                validate_group_and_segment(entries, entry, false)?;
                entries.push(entry.clone());
                entries.sort_by_key(|entry| entry.id);
            }
        }
        next.pending.remove(&unit);
        next.finalized.insert(unit, true);
        next.check_bounds()?;
        if next.schema_two {
            next.encode_live_checkpoint()?;
        }
        *self = next;
        Ok(MqDeliveryOutcome::Accepted)
    }

    pub fn backout(&mut self, unit: u64) -> Result<MqDeliveryOutcome, MqDeliveryError> {
        if unit == 0 {
            return Err(MqDeliveryError::InvalidUnit);
        }
        if self.finalized.contains_key(&unit) {
            return Ok(self.unit_outcome(unit));
        }
        let Some(_) = self.pending.get(&unit) else {
            return Ok(MqDeliveryOutcome::UnknownOutcome);
        };
        let mut next = self.clone();
        next.apply_backout(unit)?;
        next.check_bounds()?;
        if next.schema_two {
            next.encode_live_checkpoint()?;
        }
        *self = next;
        Ok(MqDeliveryOutcome::Rejected)
    }

    fn apply_backout(&mut self, unit: u64) -> Result<(), MqDeliveryError> {
        for operation in self
            .pending
            .get(&unit)
            .ok_or(MqDeliveryError::InvalidUnit)?
        {
            if let Pending::Get { queue, entry } = operation {
                if entry.expires_at.is_some_and(|expiry| expiry <= self.tick) {
                    continue;
                }
                let entries = self
                    .queues
                    .get_mut(queue)
                    .ok_or(MqDeliveryError::UnknownQueue)?;
                entries.push(entry.clone());
                entries.sort_by_key(|entry| entry.id);
            }
        }
        self.pending.remove(&unit);
        self.finalized.insert(unit, false);
        Ok(())
    }

    /// Only the trusted host advances logical time. Expiry never runs during a
    /// rejected put/get, so those paths leave queues and cursors unchanged.
    pub fn advance_tick(&mut self, tick: u64) -> Result<(), MqDeliveryError> {
        if self.schema_two && tick > i64::MAX as u64 {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        if tick < self.tick {
            return Err(MqDeliveryError::ClockRegression);
        }
        let mut next = self.clone();
        next.tick = tick;
        next.purge_expired();
        if next.schema_two {
            next.check_bounds()?;
            next.encode_live_checkpoint()?;
        }
        *self = next;
        Ok(())
    }

    fn purge_expired(&mut self) {
        let tick = self.tick;
        for entries in self.queues.values_mut() {
            entries.retain(|entry| entry.expires_at.is_none_or(|expiry| expiry > tick));
        }
        for operations in self.pending.values_mut() {
            operations.retain(|operation| match operation {
                Pending::Put { entry, .. } | Pending::Get { entry, .. } => {
                    entry.expires_at.is_none_or(|expiry| expiry > tick)
                }
            });
        }
    }

    fn resolve(
        &self,
        catalog: &MqObjectCatalog,
        name: &MqObjectName,
        capability: MqObjectCapability,
    ) -> Result<MqObjectName, MqDeliveryError> {
        if catalog.queue_manager().name != self.manager {
            return Err(MqDeliveryError::Object(MqObjectError::InvalidReferenceKind));
        }
        match catalog
            .resolve(&MqObjectLookup::Queue(name.clone()), capability)?
            .target
        {
            MqResolvedTarget::Queue { name, .. } if self.queues.contains_key(&name) => Ok(name),
            MqResolvedTarget::Queue { .. } => Err(MqDeliveryError::UnknownQueue),
            _ => Err(MqDeliveryError::Unsupported),
        }
    }

    fn allocate_id(&mut self) -> Result<u64, MqDeliveryError> {
        let id = self.next_id;
        if self.schema_two && id >= i64::MAX as u64 {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        self.next_id = id
            .checked_add(1)
            .ok_or(MqDeliveryError::ResourceExhausted)?;
        Ok(id)
    }

    fn allocate_cursor(&mut self) -> Result<u64, MqDeliveryError> {
        let id = self.next_cursor;
        if self.schema_two && id >= i64::MAX as u64 {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        self.next_cursor = id
            .checked_add(1)
            .ok_or(MqDeliveryError::ResourceExhausted)?;
        Ok(id)
    }

    fn check_bounds(&self) -> Result<(), MqDeliveryError> {
        self.check_profiles()?;
        if self.schema_two
            && ([self.next_id, self.next_cursor]
                .into_iter()
                .any(|v| v == 0 || v > i64::MAX as u64)
                || self.tick > i64::MAX as u64
                || self
                    .pending
                    .keys()
                    .chain(self.finalized.keys())
                    .any(|v| *v == 0 || *v > i64::MAX as u64))
        {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        if self.queues.len() > self.limits.queues
            || self.pending.len() > self.limits.pending_operations
            || self.cursors.len() > self.limits.cursors
            || self.finalized.len() > self.limits.finalized_units
        {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        let mut bytes = 0usize;
        let mut pending_count = 0usize;
        let mut reserved_depth = BTreeMap::<&MqObjectName, usize>::new();
        for (queue, entries) in &self.queues {
            reserved_depth.insert(queue, entries.len());
            for entry in entries.iter() {
                bytes = bytes
                    .checked_add(entry_bytes(entry)?)
                    .ok_or(MqDeliveryError::ResourceExhausted)?;
            }
        }
        for operations in self.pending.values() {
            pending_count = pending_count
                .checked_add(operations.len())
                .ok_or(MqDeliveryError::ResourceExhausted)?;
            for operation in operations {
                let (queue, entry) = match operation {
                    Pending::Put { queue, entry } | Pending::Get { queue, entry } => (queue, entry),
                };
                *reserved_depth
                    .get_mut(queue)
                    .ok_or(MqDeliveryError::UnknownQueue)? += 1;
                bytes = bytes
                    .checked_add(entry_bytes(entry)?)
                    .ok_or(MqDeliveryError::ResourceExhausted)?;
            }
        }
        if pending_count > self.limits.pending_operations
            || bytes > self.limits.total_bytes
            || reserved_depth
                .values()
                .any(|depth| *depth > self.limits.depth_per_queue)
        {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        Ok(())
    }
}

fn validate_unit(kernel: &MqDeliveryKernel, unit: Option<u64>) -> Result<(), MqDeliveryError> {
    if let Some(unit) = unit {
        if unit == 0 || kernel.finalized.contains_key(&unit) {
            return Err(MqDeliveryError::InvalidUnit);
        }
    }
    Ok(())
}

fn validate_supported_message(message: &MqMessage) -> Result<(), MqDeliveryError> {
    if matches!(message.descriptor.expiry, MqExpiry::PendingSource)
        || matches!(message.descriptor.persistence, MqPersistence::PendingSource)
        || matches!(message.descriptor.priority, MqPriority::PendingNumeric(_))
    {
        return Err(MqDeliveryError::Unsupported);
    }
    Ok(())
}

fn effective_persistence(
    requested: MqPersistence,
    default: MqPersistence,
) -> Result<MqPersistence, MqDeliveryError> {
    match requested {
        MqPersistence::QueueDefault => Ok(default),
        MqPersistence::Persistent | MqPersistence::NonPersistent => Ok(requested),
        MqPersistence::PendingSource => Err(MqDeliveryError::Unsupported),
    }
}

fn matches_identifiers(actual: &MqMessageIdentifiers, selection: &MqMessageIdentifiers) -> bool {
    [
        (&actual.message_id, &selection.message_id),
        (&actual.correlation_id, &selection.correlation_id),
        (&actual.group_id, &selection.group_id),
    ]
    .into_iter()
    .all(|(actual, wanted)| {
        wanted
            .as_ref()
            .is_none_or(|wanted| actual.as_ref() == Some(wanted))
    })
}

fn entry_bytes(entry: &Entry) -> Result<usize, MqDeliveryError> {
    let message = match &entry.message {
        Payload::Partial(message) => message,
        Payload::Complete(message) => return full_message::full_bytes(message),
    };
    let mut size = message.body.len() + 128;
    for id in [
        &message.descriptor.identifiers.message_id,
        &message.descriptor.identifiers.correlation_id,
        &message.descriptor.identifiers.group_id,
    ] {
        size = size
            .checked_add(id.as_ref().map_or(0, Vec::len))
            .ok_or(MqDeliveryError::ResourceExhausted)?;
    }
    size = size
        .checked_add(message.descriptor.format.as_ref().map_or(0, String::len))
        .ok_or(MqDeliveryError::ResourceExhausted)?;
    for property in &message.properties {
        size = size
            .checked_add(property.name.len())
            .and_then(|value| value.checked_add(property.value.len()))
            .ok_or(MqDeliveryError::ResourceExhausted)?;
    }
    Ok(size)
}

fn validate_group_and_segment(
    entries: &[Entry],
    incoming: &Entry,
    restart_suffix: bool,
) -> Result<(), MqDeliveryError> {
    let Payload::Partial(message) = &incoming.message else {
        return Ok(());
    };
    let descriptor = &message.descriptor;
    let ids = &descriptor.identifiers;
    let order = &descriptor.ordering;
    if let Some(offset) = order.segment_offset {
        if message.body.is_empty() {
            return Err(MqDeliveryError::Segment);
        }
        let prior = entries.iter().rev().find(|entry| {
            entry
                .partial()
                .expect("homogeneous partial queue")
                .descriptor
                .identifiers
                .message_id
                == ids.message_id
                && entry
                    .partial()
                    .expect("homogeneous partial queue")
                    .descriptor
                    .identifiers
                    .group_id
                    == ids.group_id
                && entry
                    .partial()
                    .expect("homogeneous partial queue")
                    .descriptor
                    .ordering
                    .group_sequence
                    == order.group_sequence
        });
        match prior {
            None if offset != 0 && !restart_suffix => return Err(MqDeliveryError::Segment),
            Some(previous) => {
                let previous_order = &previous.partial()?.descriptor.ordering;
                let expected = previous_order.segment_offset.and_then(|start| {
                    start.checked_add(
                        previous
                            .partial()
                            .expect("homogeneous partial queue")
                            .body
                            .len() as u64,
                    )
                });
                if previous_order.last_segment || expected != Some(offset) {
                    return Err(MqDeliveryError::Segment);
                }
            }
            None => {}
        }
    }
    if let Some(group) = &ids.group_id {
        let prior = entries.iter().rev().find(|entry| {
            entry
                .partial()
                .expect("homogeneous partial queue")
                .descriptor
                .identifiers
                .group_id
                .as_ref()
                == Some(group)
        });
        match prior {
            None if order.group_sequence != Some(1) && !restart_suffix => {
                return Err(MqDeliveryError::Group);
            }
            Some(previous) => {
                let prior_order = &previous.partial()?.descriptor.ordering;
                if prior_order.last_in_group {
                    return Err(MqDeliveryError::Group);
                }
                let same_sequence = prior_order.group_sequence == order.group_sequence;
                if same_sequence {
                    if order.segment_offset.is_none()
                        || prior_order.segment_offset.is_none()
                        || prior_order.last_segment
                        || previous.partial()?.descriptor.identifiers.message_id != ids.message_id
                    {
                        return Err(MqDeliveryError::Group);
                    }
                } else if prior_order
                    .group_sequence
                    .and_then(|value| value.checked_add(1))
                    != order.group_sequence
                    || (prior_order.segment_offset.is_some() && !prior_order.last_segment)
                {
                    return Err(MqDeliveryError::Group);
                }
            }
            None => {}
        }
    }
    if order.last_in_group && order.segment_offset.is_some() && !order.last_segment {
        return Err(MqDeliveryError::Group);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MqLocalQueueUsage, MqObjectDefinition, MqQueueManagerDefinition};
    use mainframe_env_host_api::{
        MqExpiry, MqGetMode, MqMessageDescriptor, MqMessageIdentifiers, MqMessageMatch,
        MqMessageOrdering, MqPriority, MqTruncation, MqWait,
    };

    pub(super) fn name(value: &str) -> MqObjectName {
        MqObjectName::new(value).unwrap()
    }

    pub(super) fn catalog() -> MqObjectCatalog {
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            ["A", "B"]
                .into_iter()
                .map(|value| MqObjectDefinition::LocalQueue {
                    name: name(value),
                    usage: MqLocalQueueUsage::Normal,
                    trigger_process: None,
                })
                .collect(),
            Default::default(),
        )
        .unwrap()
    }

    pub(super) fn kernel(catalog: &MqObjectCatalog) -> MqDeliveryKernel {
        MqDeliveryKernel::new(
            catalog,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap()
    }

    pub(super) fn message(body: &[u8]) -> MqMessage {
        MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers::default(),
                format: None,
                expiry: MqExpiry::Unlimited,
                persistence: MqPersistence::QueueDefault,
                priority: MqPriority::QueueDefault,
                ordering: MqMessageOrdering::default(),
            },
            body: body.to_vec(),
            properties: vec![],
        }
    }

    pub(super) fn request(
        mode: MqGetMode,
        capacity: usize,
        truncation: MqTruncation,
    ) -> MqGetContract {
        MqGetContract {
            selection: MqMessageMatch::default(),
            mode,
            wait: MqWait::NoWait,
            truncation,
            buffer_capacity: capacity,
        }
    }

    #[test]
    fn zero_length_is_a_message_and_truncation_rejection_preserves_state() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        kernel
            .put_one(&objects, &name("A"), message(b""), None)
            .unwrap();
        let result = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::Remove, 0, MqTruncation::Reject),
                None,
            )
            .unwrap();
        assert_eq!(
            result.disposition,
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 0 })
        );
        assert_eq!(result.message.unwrap().body, b"");
        kernel
            .put_one(&objects, &name("A"), message(b"hello"), None)
            .unwrap();
        let before = kernel.clone();
        let result = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::Remove, 2, MqTruncation::Reject),
                None,
            )
            .unwrap();
        assert_eq!(
            result.disposition,
            MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                required: 5,
                copied: 2
            })
        );
        assert_eq!(kernel, before);
    }

    #[test]
    fn syncpoint_backout_restores_get_and_discards_put() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        kernel
            .put_one(&objects, &name("A"), message(b"old"), None)
            .unwrap();
        kernel
            .put_one(&objects, &name("A"), message(b"new"), Some(7))
            .unwrap();
        let got = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::Remove, 10, MqTruncation::Reject),
                Some(7),
            )
            .unwrap();
        assert_eq!(got.message.unwrap().body, b"old");
        kernel.backout(7).unwrap();
        assert_eq!(kernel.depth(&name("A")), Some(1));
        let restored = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::Remove, 10, MqTruncation::Reject),
                None,
            )
            .unwrap();
        assert_eq!(restored.message.unwrap().body, b"old");
    }

    #[test]
    fn restart_keeps_committed_persistent_and_rejects_corruption() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        kernel
            .put_one(&objects, &name("A"), message(b"durable"), None)
            .unwrap();
        let mut volatile = message(b"volatile");
        volatile.descriptor.persistence = MqPersistence::NonPersistent;
        kernel
            .put_one(&objects, &name("A"), volatile, None)
            .unwrap();
        let bytes = kernel.encode().unwrap();
        let restarted = MqDeliveryKernel::decode(
            &bytes,
            &objects,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        assert_eq!(restarted.depth(&name("A")), Some(1));
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["schema_version"] = serde_json::json!("mainframe-env.mq-delivery@2");
        assert_eq!(
            MqDeliveryKernel::decode(
                &serde_json::to_vec(&value).unwrap(),
                &objects,
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            ),
            Err(MqDeliveryError::UnsupportedSchema)
        );
    }

    #[test]
    fn browse_identifier_selection_cursor_and_accepted_truncation() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        let mut first = message(b"first");
        first.descriptor.identifiers.correlation_id = Some(b"one".to_vec());
        kernel.put_one(&objects, &name("A"), first, None).unwrap();
        let mut second = message(b"second");
        second.descriptor.identifiers.correlation_id = Some(b"two".to_vec());
        kernel.put_one(&objects, &name("A"), second, None).unwrap();
        let mut browse = request(MqGetMode::BrowseFirst, 2, MqTruncation::Accept);
        browse.selection.identifiers.correlation_id = Some(b"one".to_vec());
        let first = kernel.get(&objects, &name("A"), &browse, None).unwrap();
        assert_eq!(first.message.unwrap().body, b"fi");
        assert_eq!(
            first.disposition,
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedBrowsed {
                required: 5,
                copied: 2
            })
        );
        let cursor = first.cursor.unwrap();
        let before = kernel.clone();
        assert_eq!(
            kernel.get(
                &objects,
                &name("B"),
                &request(MqGetMode::BrowseNext { cursor }, 10, MqTruncation::Reject),
                None
            ),
            Err(MqDeliveryError::InvalidCursor)
        );
        assert_eq!(kernel, before);
        let next = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseNext { cursor }, 10, MqTruncation::Reject),
                None,
            )
            .unwrap();
        assert_eq!(next.message.unwrap().body, b"second");
        assert_eq!(next.cursor, Some(cursor));
        let removed = kernel
            .get(
                &objects,
                &name("A"),
                &request(
                    MqGetMode::RemoveUnderCursor { cursor },
                    2,
                    MqTruncation::Accept,
                ),
                None,
            )
            .unwrap();
        assert_eq!(
            removed.disposition,
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                required: 6,
                copied: 2
            })
        );
        assert_eq!(kernel.depth(&name("A")), Some(1));
    }

    #[test]
    fn distribution_overload_is_atomic_and_replay_is_explicit() {
        let objects = catalog();
        let mut limits = MqDeliveryLimits::default();
        limits.depth_per_queue = 1;
        let mut kernel = MqDeliveryKernel::new(
            &objects,
            limits,
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        kernel
            .put_one(&objects, &name("B"), message(b"full"), None)
            .unwrap();
        let before = kernel.clone();
        assert_eq!(
            kernel.put(&objects, &[name("A"), name("B")], message(b"x"), None),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(kernel, before);
        assert_eq!(
            kernel.put_one(&objects, &name("MISSING"), message(b"x"), None),
            Err(MqDeliveryError::Object(MqObjectError::UnknownObject))
        );
        assert_eq!(kernel, before);
        let result = kernel
            .put(&objects, &[name("A")], message(b"staged"), Some(9))
            .unwrap();
        assert_eq!(result.items[0].outcome, MqDeliveryOutcome::Pending);
        assert_eq!(kernel.depth(&name("A")), Some(0));
        assert_eq!(kernel.unit_outcome(9), MqDeliveryOutcome::Pending);
        assert_eq!(kernel.commit(9), Ok(MqDeliveryOutcome::Accepted));
        assert_eq!(kernel.commit(9), Ok(MqDeliveryOutcome::DuplicatePossible));
        assert_eq!(kernel.unit_outcome(999), MqDeliveryOutcome::UnknownOutcome);
        let before = kernel.clone();
        assert_eq!(
            kernel.put_one(&objects, &name("A"), message(b"x"), Some(9)),
            Err(MqDeliveryError::InvalidUnit)
        );
        assert_eq!(kernel, before);
    }

    #[test]
    fn group_segment_and_clock_boundaries_leave_rejections_unchanged() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        let mut first = message(b"ab");
        first.descriptor.identifiers.message_id = Some(b"message".to_vec());
        first.descriptor.identifiers.group_id = Some(b"group".to_vec());
        first.descriptor.ordering.group_sequence = Some(1);
        first.descriptor.ordering.segment_offset = Some(0);
        kernel
            .put_one(&objects, &name("A"), first.clone(), None)
            .unwrap();
        let before = kernel.clone();
        let mut gap = first.clone();
        gap.descriptor.ordering.segment_offset = Some(3);
        assert_eq!(
            kernel.put_one(&objects, &name("A"), gap, None),
            Err(MqDeliveryError::Segment)
        );
        assert_eq!(kernel, before);
        let mut last = first;
        last.descriptor.ordering.segment_offset = Some(2);
        last.descriptor.ordering.last_segment = true;
        last.descriptor.ordering.last_in_group = true;
        kernel.put_one(&objects, &name("A"), last, None).unwrap();
        let before = kernel.clone();
        let mut invalid = message(b"more");
        invalid.descriptor.identifiers.group_id = Some(b"group".to_vec());
        invalid.descriptor.ordering.group_sequence = Some(2);
        assert_eq!(
            kernel.put_one(&objects, &name("A"), invalid, None),
            Err(MqDeliveryError::Group)
        );
        assert_eq!(kernel, before);
        assert_eq!(kernel.advance_tick(3), Ok(()));
        let before = kernel.clone();
        assert_eq!(
            kernel.advance_tick(2),
            Err(MqDeliveryError::ClockRegression)
        );
        assert_eq!(kernel, before);
    }

    #[test]
    fn expiry_and_restart_back_out_uncommitted_work() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        let mut expiring = message(b"expires");
        expiring.descriptor.expiry = MqExpiry::RelativeHostTicks(2);
        kernel
            .put_one(&objects, &name("A"), expiring, None)
            .unwrap();
        kernel.advance_tick(1).unwrap();
        assert_eq!(kernel.depth(&name("A")), Some(1));
        kernel.advance_tick(2).unwrap();
        assert_eq!(kernel.depth(&name("A")), Some(0));
        kernel
            .put_one(&objects, &name("A"), message(b"committed"), None)
            .unwrap();
        kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::Remove, 20, MqTruncation::Reject),
                Some(5),
            )
            .unwrap();
        kernel
            .put_one(&objects, &name("A"), message(b"uncommitted"), Some(5))
            .unwrap();
        let restarted = MqDeliveryKernel::decode(
            &kernel.encode().unwrap(),
            &objects,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        assert_eq!(restarted.depth(&name("A")), Some(1));
        assert_eq!(restarted.unit_outcome(5), MqDeliveryOutcome::UnknownOutcome);
    }

    #[test]
    fn snapshot_rejects_unknown_fields_duplicate_ids_and_bad_payload() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        kernel
            .put_one(&objects, &name("A"), message(b"x"), None)
            .unwrap();
        let bytes = kernel.encode().unwrap();
        let decode = |value: serde_json::Value| {
            MqDeliveryKernel::decode(
                &serde_json::to_vec(&value).unwrap(),
                &objects,
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent,
            )
        };
        let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        unknown["extra"] = serde_json::json!(1);
        assert_eq!(decode(unknown), Err(MqDeliveryError::CorruptSnapshot));
        let mut duplicate: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let original = duplicate["queues"][0]["messages"][0].clone();
        duplicate["queues"][0]["messages"]
            .as_array_mut()
            .unwrap()
            .push(original);
        assert_eq!(decode(duplicate), Err(MqDeliveryError::CorruptSnapshot));
        let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        invalid["queues"][0]["messages"][0]["message"]["body"] = serde_json::json!([256]);
        assert_eq!(decode(invalid), Err(MqDeliveryError::CorruptSnapshot));
    }

    #[test]
    fn no_message_wait_and_own_uncommitted_put_are_distinct() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        let mut wait = request(MqGetMode::Remove, 10, MqTruncation::Reject);
        assert_eq!(
            kernel
                .get(&objects, &name("A"), &wait, None)
                .unwrap()
                .disposition,
            MqGetDisposition::NoMessage
        );
        wait.wait = MqWait::BoundedHostTicks(2);
        assert_eq!(
            kernel
                .get(&objects, &name("A"), &wait, None)
                .unwrap()
                .disposition,
            MqGetDisposition::WaitExpired
        );
        kernel
            .put_one(&objects, &name("A"), message(b"own"), Some(11))
            .unwrap();
        assert_eq!(
            kernel
                .get(&objects, &name("A"), &wait, None)
                .unwrap()
                .disposition,
            MqGetDisposition::WaitExpired
        );
        let own = kernel.get(&objects, &name("A"), &wait, Some(11)).unwrap();
        assert_eq!(own.message.unwrap().body, b"own");
        assert_eq!(kernel.commit(11), Ok(MqDeliveryOutcome::Accepted));
        assert_eq!(kernel.depth(&name("A")), Some(0));
    }

    #[test]
    fn pending_capacity_and_cursor_capacity_fail_without_mutation() {
        let objects = catalog();
        let mut limits = MqDeliveryLimits::default();
        limits.pending_operations = 1;
        limits.cursors = 1;
        let mut kernel = MqDeliveryKernel::new(
            &objects,
            limits,
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        kernel
            .put_one(&objects, &name("A"), message(b"x"), Some(1))
            .unwrap();
        let before = kernel.clone();
        assert_eq!(
            kernel.put_one(&objects, &name("B"), message(b"y"), Some(2)),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(kernel, before);
        kernel.commit(1).unwrap();
        kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 10, MqTruncation::Reject),
                None,
            )
            .unwrap();
        let before = kernel.clone();
        assert_eq!(
            kernel.get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 10, MqTruncation::Reject),
                None
            ),
            Err(MqDeliveryError::ResourceExhausted)
        );
        assert_eq!(kernel, before);
    }

    #[test]
    fn restart_invalidates_old_cursor_tokens_without_reusing_them() {
        let objects = catalog();
        let mut kernel = kernel(&objects);
        kernel
            .put_one(&objects, &name("A"), message(b"x"), None)
            .unwrap();
        let old = kernel
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 4, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        let mut restarted = MqDeliveryKernel::decode(
            &kernel.encode().unwrap(),
            &objects,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        let before = restarted.clone();
        assert_eq!(
            restarted.get(
                &objects,
                &name("A"),
                &request(
                    MqGetMode::RemoveUnderCursor { cursor: old },
                    4,
                    MqTruncation::Reject
                ),
                None
            ),
            Err(MqDeliveryError::InvalidCursor)
        );
        assert_eq!(restarted, before);
        let new = restarted
            .get(
                &objects,
                &name("A"),
                &request(MqGetMode::BrowseFirst, 4, MqTruncation::Reject),
                None,
            )
            .unwrap()
            .cursor
            .unwrap();
        assert_ne!(new, old);
    }
}
