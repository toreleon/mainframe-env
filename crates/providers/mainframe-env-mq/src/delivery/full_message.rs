//! Complete stored payloads in the existing kernel. No MQI policy admission.
use super::*;
use mainframe_env_host_api::mq_md_value::{MqMdCharacterEncoding, MqMdValue};
use mainframe_env_host_api::mq_mqi::{MqFullMessage, mq_md_value_size};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum QueueProfile {
    #[default]
    Partial,
    Complete {
        version: i32,
        characters: MqMdCharacterEncoding,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct QueueState {
    pub(super) profile: QueueProfile,
    pub(super) entries: Vec<Entry>,
}
impl From<Vec<Entry>> for QueueState {
    fn from(entries: Vec<Entry>) -> Self {
        Self {
            profile: QueueProfile::Partial,
            entries,
        }
    }
}
impl std::ops::Deref for QueueState {
    type Target = Vec<Entry>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
impl std::ops::DerefMut for QueueState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Payload {
    Partial(MqMessage),
    Complete(MqFullMessage),
}
pub(super) struct PayloadGet {
    pub(super) disposition: MqGetDisposition,
    pub(super) message: Option<Payload>,
    pub(super) cursor: Option<u64>,
}
impl Payload {
    pub(super) fn body(&self) -> &[u8] {
        match self {
            Self::Partial(m) => &m.body,
            Self::Complete(m) => &m.body,
        }
    }
    pub(super) fn body_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Self::Partial(m) => &mut m.body,
            Self::Complete(m) => &mut m.body,
        }
    }
    pub(super) fn matches(&self, selection: &MqMessageIdentifiers) -> bool {
        match self {
            Self::Partial(m) => matches_identifiers(&m.descriptor.identifiers, selection),
            Self::Complete(m) => {
                let ids = m.descriptor.fields();
                selection.message_id.as_ref().is_none_or(|v| v.as_slice() == ids.msg_id)
                    && selection.correlation_id.as_ref().is_none_or(|v| v.as_slice() == ids.correl_id)
                    && selection.group_id.as_ref().is_none_or(|v| matches!(&m.descriptor, MqMdValue::V2 { extension, .. } if v.as_slice() == extension.group_id))
            }
        }
    }
}
pub(super) fn full_bytes(message: &MqFullMessage) -> Result<usize, MqDeliveryError> {
    let mut bytes = mq_md_value_size(&message.descriptor, 2048)
        .map_err(|_| MqDeliveryError::ResourceExhausted)?
        .checked_add(message.body.len())
        .and_then(|n| n.checked_add(32))
        .ok_or(MqDeliveryError::ResourceExhausted)?;
    for p in &message.properties {
        bytes = bytes
            .checked_add(p.name.len())
            .and_then(|n| n.checked_add(p.value.len()))
            .and_then(|n| n.checked_add(32))
            .ok_or(MqDeliveryError::ResourceExhausted)?;
    }
    Ok(bytes)
}
impl Entry {
    pub(super) fn partial(&self) -> Result<&MqMessage, MqDeliveryError> {
        match &self.message {
            Payload::Partial(v) => Ok(v),
            _ => Err(MqDeliveryError::Unsupported),
        }
    }
    pub(super) fn persistent(&self) -> bool {
        match &self.message {
            Payload::Partial(v) => v.descriptor.persistence == MqPersistence::Persistent,
            Payload::Complete(v) => v.descriptor.fields().persistence == 1,
        }
    }
}

pub(super) fn validate_full(
    message: &MqFullMessage,
    limits: MqMessageLimits,
) -> Result<(), MqDeliveryError> {
    message
        .validate(limits)
        .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
    // Pinned q092170_: MQPER_NOT_PERSISTENT=0/MQPER_PERSISTENT=1.
    // q097390_: MQEI_UNLIMITED=-1. Default resolution/time conversion are NOT here.
    if !matches!(message.descriptor.fields().persistence, 0 | 1)
        || message.descriptor.fields().expiry != -1
    {
        return Err(MqDeliveryError::Unsupported);
    }
    Ok(())
}

impl QueueProfile {
    pub(super) fn accepts(self, message: &MqFullMessage) -> bool {
        self == Self::Complete {
            version: message.descriptor.version(),
            characters: message.descriptor.characters(),
        }
    }
}
impl MqDeliveryKernel {
    pub(crate) fn full_queue_profile(
        &self,
        queue: &MqObjectName,
    ) -> Result<QueueProfile, MqDeliveryError> {
        self.queues
            .get(queue)
            .map(|s| s.profile)
            .ok_or(MqDeliveryError::UnknownQueue)
    }
    pub(super) fn check_profiles(&self) -> Result<(), MqDeliveryError> {
        for state in self.queues.values() {
            if !self.schema_two && state.profile != QueueProfile::Partial {
                return Err(MqDeliveryError::UnsupportedSchema);
            }
            if let QueueProfile::Complete { version, .. } = state.profile
                && !matches!(version, 1 | 2)
            {
                return Err(MqDeliveryError::Unsupported);
            }
            for entry in state.entries.iter() {
                self.check_payload(state.profile, entry)?;
            }
        }
        for ops in self.pending.values() {
            for op in ops {
                let (queue, entry) = match op {
                    Pending::Put { queue, entry } | Pending::Get { queue, entry } => (queue, entry),
                };
                self.check_payload(
                    self.queues
                        .get(queue)
                        .ok_or(MqDeliveryError::UnknownQueue)?
                        .profile,
                    entry,
                )?;
            }
        }
        Ok(())
    }
    fn check_payload(&self, profile: QueueProfile, entry: &Entry) -> Result<(), MqDeliveryError> {
        match &entry.message {
            Payload::Partial(_) if profile == QueueProfile::Partial => Ok(()),
            Payload::Complete(m)
                if self.schema_two && profile.accepts(m) && entry.expires_at.is_none() =>
            {
                validate_full(m, self.message_limits)
            }
            _ => Err(MqDeliveryError::Unsupported),
        }
    }
    pub(crate) fn get_full(
        &mut self,
        catalog: &MqObjectCatalog,
        queue: &MqObjectName,
        profile: QueueProfile,
        request: &MqGetContract,
        unit: Option<u64>,
    ) -> Result<(MqGetDisposition, Option<MqFullMessage>, Option<u64>), MqDeliveryError> {
        if !matches!(profile, QueueProfile::Complete { .. }) {
            return Err(MqDeliveryError::Unsupported);
        }
        if unit.is_some_and(|u| u > i64::MAX as u64) {
            return Err(MqDeliveryError::InvalidUnit);
        }
        let got = self.get_payload(catalog, queue, request, unit, profile)?;
        Ok((
            got.disposition,
            got.message.map(|p| match p {
                Payload::Complete(m) => m,
                _ => unreachable!("checked profile"),
            }),
            got.cursor,
        ))
    }
    /// Explicit schema/profile candidate, not operator permission. The owning
    /// service additionally refuses live owners and carries all physical fences.
    pub(crate) fn upgrade_profiles(
        &self,
        profiles: &BTreeMap<MqObjectName, QueueProfile>,
    ) -> Result<Self, MqDeliveryError> {
        if !self.pending.is_empty() || !self.cursors.is_empty() {
            return Err(MqDeliveryError::InvalidUnit);
        }
        let mut next = self.clone();
        for (name, profile) in profiles {
            if let QueueProfile::Complete { version, .. } = profile
                && !matches!(version, 1 | 2)
            {
                return Err(MqDeliveryError::Unsupported);
            }
            let queue = next
                .queues
                .get_mut(name)
                .ok_or(MqDeliveryError::UnknownQueue)?;
            if queue.profile != *profile && !queue.is_empty() {
                return Err(MqDeliveryError::Unsupported);
            }
            queue.profile = *profile;
        }
        next.schema_two = true;
        next.check_bounds()?;
        next.encode_live_checkpoint()?;
        Ok(next)
    }

    /// Stores an already complete observation. This private primitive does not
    /// generate IDs/context/date, interpret flags, or admit a public MQPUT.
    pub(crate) fn put_full(
        &mut self,
        catalog: &MqObjectCatalog,
        queue: &MqObjectName,
        message: MqFullMessage,
        unit: Option<u64>,
    ) -> Result<MqDeliveryOutcome, MqDeliveryError> {
        let target = self.resolve(catalog, queue, MqObjectCapability::Output)?;
        if !self.queues[&target].profile.accepts(&message) {
            return Err(MqDeliveryError::Unsupported);
        }
        validate_full(&message, self.message_limits)?;
        validate_unit(self, unit)?;
        if unit.is_some_and(|u| u > i64::MAX as u64) {
            return Err(MqDeliveryError::InvalidUnit);
        }
        let mut next = self.clone();
        let entry = Entry {
            id: next.allocate_id()?,
            message: Payload::Complete(message),
            expires_at: None,
        };
        if let Some(unit) = unit {
            next.pending.entry(unit).or_default().push(Pending::Put {
                queue: target,
                entry,
            });
        } else {
            next.queues
                .get_mut(&target)
                .expect("resolved queue")
                .push(entry);
        }
        next.check_bounds()?;
        next.encode_live_checkpoint()?;
        *self = next;
        Ok(if unit.is_some() {
            MqDeliveryOutcome::Pending
        } else {
            MqDeliveryOutcome::Accepted
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
