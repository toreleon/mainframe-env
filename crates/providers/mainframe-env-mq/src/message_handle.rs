//! Private bounded message-handle kernel. MQI routing and wire encoding remain pending.

use mainframe_env_host_api::{
    MqExpiry, MqHandle, MqHandleKind, MqHandleOwner, MqHandleProblem, MqHandleRegistry,
    MqHandleSharing, MqHconn, MqHmsg, MqHobj, MqHostEnvironment, MqMessage, MqMessageDescriptor,
    MqMessageIdentifiers, MqMessageLimits, MqMessageOrdering, MqMessageProblem, MqMessageProperty,
    MqPersistence, MqPriority, MqPropertyQuery, MqPropertyType,
};

const MAGIC: &[u8; 4] = b"MHK1";
const MAX_KERNEL_PROPERTY_BYTES: usize = 16 * 1024 * 1024;

/// Only the contract-default option is executable. IBM option bits are pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandleKernelOption {
    Default,
    PendingWire,
}

/// `KernelV1` is a private deterministic frame, never an MQRFH2 representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqBufferCodec {
    KernelV1,
    MqRfh2Pending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHandleKernelProblem {
    Handle(MqHandleProblem),
    Message(MqMessageProblem),
    InvalidOption,
    UnsupportedWire,
    NotFound,
    BufferTooSmall { required: usize },
    MalformedBuffer,
    Capacity,
}

impl From<MqHandleProblem> for MqHandleKernelProblem {
    fn from(value: MqHandleProblem) -> Self {
        Self::Handle(value)
    }
}

impl From<MqMessageProblem> for MqHandleKernelProblem {
    fn from(value: MqMessageProblem) -> Self {
        Self::Message(value)
    }
}

#[derive(Debug)]
struct Properties {
    handle: MqHmsg,
    owner: MqHandleOwner,
    connection: MqHconn,
    values: Vec<MqMessageProperty>,
}

/// Volatile properties keyed by registry-issued HMSG. The registry alone decides
/// token lifetime, kind, connection, owner and in-use state. No snapshot API exists.
#[derive(Debug)]
pub struct MqHandleKernel {
    registry: MqHandleRegistry,
    limits: MqMessageLimits,
    properties: Vec<Properties>,
}

impl MqHandleKernel {
    pub fn new(
        epoch: u64,
        max_slots: usize,
        limits: MqMessageLimits,
    ) -> Result<Self, MqHandleKernelProblem> {
        limits.validate()?;
        Ok(Self {
            registry: MqHandleRegistry::new(epoch, max_slots)?,
            limits,
            properties: Vec::new(),
        })
    }

    pub fn connect(
        &mut self,
        owner: MqHandleOwner,
        sharing: MqHandleSharing,
    ) -> Result<MqHconn, MqHandleKernelProblem> {
        Ok(self.registry.connect(owner, sharing)?)
    }

    pub fn bind_cics_default(
        &mut self,
        owner: MqHandleOwner,
    ) -> Result<MqHconn, MqHandleKernelProblem> {
        Ok(self.registry.bind_cics_default(owner)?)
    }

    pub fn create_object(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
    ) -> Result<MqHobj, MqHandleKernelProblem> {
        Ok(self.registry.create_object(owner, connection)?)
    }

    pub fn create(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        option: MqHandleKernelOption,
    ) -> Result<MqHmsg, MqHandleKernelProblem> {
        check_option(option)?;
        // The registry allocation is the only handle creation point.
        let handle = self.registry.create_message(owner, connection)?;
        self.properties.push(Properties {
            handle,
            owner,
            connection,
            values: Vec::new(),
        });
        Ok(handle)
    }

    pub fn delete(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        option: MqHandleKernelOption,
    ) -> Result<(), MqHandleKernelProblem> {
        check_option(option)?;
        self.registry
            .release(owner, connection, handle, MqHandleKind::Message)?;
        self.properties
            .retain(|entry| MqHandle::Message(entry.handle) != handle);
        Ok(())
    }

    pub fn disconnect(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
    ) -> Result<(), MqHandleKernelProblem> {
        self.registry.disconnect(owner, connection)?;
        if connection != MqHconn::Default {
            self.properties
                .retain(|entry| entry.connection != connection);
        }
        Ok(())
    }

    pub fn advance_epoch(&mut self, epoch: u64) -> Result<(), MqHandleKernelProblem> {
        self.registry.advance_epoch(epoch)?;
        self.properties.clear();
        Ok(())
    }

    pub fn begin_io(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHmsg,
    ) -> Result<(), MqHandleKernelProblem> {
        Ok(self.registry.begin_message_io(owner, connection, handle)?)
    }

    pub fn end_io(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHmsg,
    ) -> Result<(), MqHandleKernelProblem> {
        Ok(self.registry.end_message_io(owner, connection, handle)?)
    }

    /// End a unit in the registry, then discard only property rows whose tokens
    /// the registry now reports stale. Missing connections can be transient for
    /// an unassociated handle and never authorize deleting its properties.
    pub fn end_processing_unit(
        &mut self,
        owner: MqHandleOwner,
    ) -> Result<(), MqHandleKernelProblem> {
        self.registry.end_processing_unit(owner)?;
        self.properties.retain(|entry| {
            // Reclaim special-handle data after the registry retires its token.
            // Without a connection, validation may stop at MissingConnection.
            if matches!(entry.connection, MqHconn::Unassociated | MqHconn::Default)
                && same_processing_unit(entry.owner, owner)
            {
                return false;
            }
            self.registry.validate_message_property(
                entry.owner,
                entry.connection,
                entry.handle.into(),
            ) != Err(MqHandleProblem::Stale)
        });
        Ok(())
    }

    pub fn set(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        property: MqMessageProperty,
        option: MqHandleKernelOption,
    ) -> Result<(), MqHandleKernelProblem> {
        self.validate(owner, connection, handle)?;
        check_option(option)?;
        let index = self.index(handle)?;
        let mut next = self.properties[index].values.clone();
        match next.binary_search_by(|existing| existing.name.cmp(&property.name)) {
            Ok(position) => next[position] = property,
            Err(position) => next.insert(position, property),
        }
        validate_properties(&next, self.limits)?;
        self.check_total(index, &next)?;
        self.properties[index].values = next;
        Ok(())
    }

    pub fn delete_property(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        query: &MqPropertyQuery,
        option: MqHandleKernelOption,
    ) -> Result<bool, MqHandleKernelProblem> {
        self.validate(owner, connection, handle)?;
        check_option(option)?;
        let MqPropertyQuery::Exact(name) = query else {
            return Err(MqHandleKernelProblem::InvalidOption);
        };
        query.validate(self.limits)?;
        let index = self.index(handle)?;
        if let Ok(position) = self.properties[index]
            .values
            .binary_search_by(|item| item.name.cmp(name))
        {
            self.properties[index].values.remove(position);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn inquire(
        &self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        query: &MqPropertyQuery,
        after: Option<&str>,
        capacity: usize,
        option: MqHandleKernelOption,
    ) -> Result<MqMessageProperty, MqHandleKernelProblem> {
        self.validate(owner, connection, handle)?;
        check_option(option)?;
        query.validate(self.limits)?;
        if capacity > self.limits.property_value_bytes {
            return Err(MqHandleKernelProblem::InvalidOption);
        }
        if let Some(name) = after {
            MqPropertyQuery::Exact(name.into()).validate(self.limits)?;
        }
        let item = self.properties[self.index(handle)?]
            .values
            .iter()
            .find(|item| {
                matches_query(query, &item.name)
                    && after.is_none_or(|name| item.name.as_str() > name)
            })
            .ok_or(MqHandleKernelProblem::NotFound)?;
        if item.value.len() > capacity {
            return Err(MqHandleKernelProblem::BufferTooSmall {
                required: item.value.len(),
            });
        }
        Ok(item.clone())
    }

    pub fn to_buffer(
        &self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        query: &MqPropertyQuery,
        capacity: usize,
        codec: MqBufferCodec,
    ) -> Result<Vec<u8>, MqHandleKernelProblem> {
        self.validate(owner, connection, handle)?;
        check_codec(codec)?;
        query.validate(self.limits)?;
        let selected: Vec<_> = self.properties[self.index(handle)?]
            .values
            .iter()
            .filter(|item| matches_query(query, &item.name))
            .cloned()
            .collect();
        if selected.is_empty() {
            return Err(MqHandleKernelProblem::NotFound);
        }
        let frame = encode(&selected);
        if frame.len() > capacity {
            return Err(MqHandleKernelProblem::BufferTooSmall {
                required: frame.len(),
            });
        }
        Ok(frame)
    }

    pub fn from_buffer(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
        buffer: &[u8],
        strip_properties: bool,
        codec: MqBufferCodec,
    ) -> Result<Vec<u8>, MqHandleKernelProblem> {
        self.validate(owner, connection, handle)?;
        check_codec(codec)?;
        let maximum = self
            .limits
            .property_total_bytes
            .saturating_add(self.limits.body_bytes)
            .saturating_add(self.limits.properties.saturating_mul(7))
            .saturating_add(10);
        if buffer.len() > maximum {
            return Err(MqHandleKernelProblem::Capacity);
        }
        if buffer.is_empty() {
            return Ok(Vec::new());
        }
        let (incoming, body) = decode(buffer, self.limits)?;
        if body.len() > self.limits.body_bytes {
            return Err(MqHandleKernelProblem::Message(
                MqMessageProblem::BodyTooLong,
            ));
        }
        let index = self.index(handle)?;
        let mut next = self.properties[index].values.clone();
        for property in incoming {
            match next.binary_search_by(|existing| existing.name.cmp(&property.name)) {
                Ok(position) => next[position] = property,
                Err(position) => next.insert(position, property),
            }
        }
        validate_properties(&next, self.limits)?;
        self.check_total(index, &next)?;
        let result = if strip_properties {
            body.to_vec()
        } else {
            buffer.to_vec()
        };
        self.properties[index].values = next;
        Ok(result)
    }

    fn validate(
        &self,
        owner: MqHandleOwner,
        connection: MqHconn,
        handle: MqHandle,
    ) -> Result<(), MqHandleKernelProblem> {
        Ok(self
            .registry
            .validate_message_property(owner, connection, handle)?)
    }

    fn index(&self, handle: MqHandle) -> Result<usize, MqHandleKernelProblem> {
        let MqHandle::Message(hmsg) = handle else {
            return Err(MqHandleKernelProblem::Handle(MqHandleProblem::WrongKind));
        };
        self.properties
            .iter()
            .position(|entry| entry.handle == hmsg)
            .ok_or(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
    }

    fn check_total(
        &self,
        replaced: usize,
        next: &[MqMessageProperty],
    ) -> Result<(), MqHandleKernelProblem> {
        let total = self
            .properties
            .iter()
            .enumerate()
            .try_fold(0usize, |total, (index, entry)| {
                let values: &[MqMessageProperty] = if index == replaced {
                    next
                } else {
                    &entry.values
                };
                values.iter().try_fold(total, |sum, item| {
                    sum.checked_add(item.name.len())?
                        .checked_add(item.value.len())
                })
            })
            .ok_or(MqHandleKernelProblem::Capacity)?;
        if total > MAX_KERNEL_PROPERTY_BYTES {
            return Err(MqHandleKernelProblem::Capacity);
        }
        Ok(())
    }
}

fn check_option(option: MqHandleKernelOption) -> Result<(), MqHandleKernelProblem> {
    if option == MqHandleKernelOption::Default {
        Ok(())
    } else {
        Err(MqHandleKernelProblem::InvalidOption)
    }
}

fn check_codec(codec: MqBufferCodec) -> Result<(), MqHandleKernelProblem> {
    if codec == MqBufferCodec::KernelV1 {
        Ok(())
    } else {
        Err(MqHandleKernelProblem::UnsupportedWire)
    }
}

fn matches_query(query: &MqPropertyQuery, name: &str) -> bool {
    match query {
        MqPropertyQuery::Exact(exact) => exact == name,
        MqPropertyQuery::Prefix(prefix) => name.starts_with(prefix),
    }
}

fn same_processing_unit(left: MqHandleOwner, right: MqHandleOwner) -> bool {
    if left.environment != right.environment
        || left.host_id != right.host_id
        || left.process_id != right.process_id
    {
        return false;
    }
    match left.environment {
        MqHostEnvironment::ZosCics
        | MqHostEnvironment::ZosBatch
        | MqHostEnvironment::ZosImsBatchDli => left.task_id == right.task_id,
        MqHostEnvironment::ZosIms => {
            left.task_id == right.task_id && left.syncpoint_epoch == right.syncpoint_epoch
        }
        MqHostEnvironment::MqiClient | MqHostEnvironment::OtherBindings => {
            left.thread_id == right.thread_id
        }
    }
}

fn validate_properties(
    values: &[MqMessageProperty],
    limits: MqMessageLimits,
) -> Result<(), MqHandleKernelProblem> {
    let message = MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: MqMessageIdentifiers::default(),
            format: None,
            expiry: MqExpiry::Unlimited,
            persistence: MqPersistence::QueueDefault,
            priority: MqPriority::QueueDefault,
            ordering: MqMessageOrdering::default(),
        },
        body: Vec::new(),
        properties: values.to_vec(),
    };
    Ok(message.validate(limits)?)
}

fn type_tag(kind: MqPropertyType) -> u8 {
    match kind {
        MqPropertyType::Boolean => 0,
        MqPropertyType::ByteString => 1,
        MqPropertyType::Int8 => 2,
        MqPropertyType::Int16 => 3,
        MqPropertyType::Int32 => 4,
        MqPropertyType::Int64 => 5,
        MqPropertyType::Float32 => 6,
        MqPropertyType::Float64 => 7,
        MqPropertyType::String => 8,
        MqPropertyType::Null => 9,
    }
}

fn property_type(tag: u8) -> Result<MqPropertyType, MqHandleKernelProblem> {
    match tag {
        0 => Ok(MqPropertyType::Boolean),
        1 => Ok(MqPropertyType::ByteString),
        2 => Ok(MqPropertyType::Int8),
        3 => Ok(MqPropertyType::Int16),
        4 => Ok(MqPropertyType::Int32),
        5 => Ok(MqPropertyType::Int64),
        6 => Ok(MqPropertyType::Float32),
        7 => Ok(MqPropertyType::Float64),
        8 => Ok(MqPropertyType::String),
        9 => Ok(MqPropertyType::Null),
        _ => Err(MqHandleKernelProblem::MalformedBuffer),
    }
}

fn encode(values: &[MqMessageProperty]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for item in values {
        bytes.extend_from_slice(&(item.name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(item.name.as_bytes());
        bytes.push(type_tag(item.kind));
        bytes.extend_from_slice(&(item.value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&item.value);
    }
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes
}

struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], MqHandleKernelProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(MqHandleKernelProblem::MalformedBuffer)?;
        let part = self
            .input
            .get(self.offset..end)
            .ok_or(MqHandleKernelProblem::MalformedBuffer)?;
        self.offset = end;
        Ok(part)
    }

    fn u8(&mut self) -> Result<u8, MqHandleKernelProblem> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<usize, MqHandleKernelProblem> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize)
    }
    fn u32(&mut self) -> Result<usize, MqHandleKernelProblem> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }
}

fn decode<'a>(
    buffer: &'a [u8],
    limits: MqMessageLimits,
) -> Result<(Vec<MqMessageProperty>, &'a [u8]), MqHandleKernelProblem> {
    let mut reader = Reader {
        input: buffer,
        offset: 0,
    };
    if reader.take(4)? != MAGIC {
        return Err(MqHandleKernelProblem::MalformedBuffer);
    }
    let count = reader.u16()?;
    if count > limits.properties {
        return Err(MqHandleKernelProblem::Message(
            MqMessageProblem::PropertyCount,
        ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let name_length = reader.u16()?;
        if name_length > limits.property_name_bytes {
            return Err(MqHandleKernelProblem::Message(
                MqMessageProblem::PropertyName,
            ));
        }
        let name = std::str::from_utf8(reader.take(name_length)?)
            .map_err(|_| MqHandleKernelProblem::MalformedBuffer)?
            .to_owned();
        let kind = property_type(reader.u8()?)?;
        let value_length = reader.u32()?;
        if value_length > limits.property_value_bytes {
            return Err(MqHandleKernelProblem::Message(
                MqMessageProblem::PropertyValueLength,
            ));
        }
        let value = reader.take(value_length)?.to_vec();
        values.push(MqMessageProperty { name, kind, value });
    }
    let body_length = reader.u32()?;
    if body_length > limits.body_bytes {
        return Err(MqHandleKernelProblem::Message(
            MqMessageProblem::BodyTooLong,
        ));
    }
    let body = reader.take(body_length)?;
    if reader.offset != buffer.len() {
        return Err(MqHandleKernelProblem::MalformedBuffer);
    }
    values.sort_by(|left, right| left.name.cmp(&right.name));
    validate_properties(&values, limits)?;
    Ok((values, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::{
        MqHandle, MqHandleOwner, MqHandleProblem, MqHandleSharing, MqHostEnvironment,
        MqMessageLimits, MqMessageProperty, MqPropertyQuery, MqPropertyType,
    };

    fn owner(thread_id: u64) -> MqHandleOwner {
        MqHandleOwner {
            environment: MqHostEnvironment::MqiClient,
            host_id: 1,
            process_id: 2,
            thread_id,
            task_id: 4,
            syncpoint_epoch: 5,
        }
    }

    fn property(name: &str, value: &[u8]) -> MqMessageProperty {
        MqMessageProperty {
            name: name.into(),
            kind: MqPropertyType::ByteString,
            value: value.into(),
        }
    }

    #[test]
    fn ordered_typed_properties_and_private_buffer_round_trip() {
        let mut kernel = MqHandleKernel::new(1, 8, MqMessageLimits::default()).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let hmsg = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                hmsg.into(),
                property("usr.Z", b"z"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        kernel
            .set(
                who,
                conn,
                hmsg.into(),
                property("usr.A", b"a"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        let query = MqPropertyQuery::Prefix("usr.".into());
        let first = kernel
            .inquire(
                who,
                conn,
                hmsg.into(),
                &query,
                None,
                8,
                MqHandleKernelOption::Default,
            )
            .unwrap();
        assert_eq!(first.name, "usr.A");
        let second = kernel
            .inquire(
                who,
                conn,
                hmsg.into(),
                &query,
                Some("usr.A"),
                8,
                MqHandleKernelOption::Default,
            )
            .unwrap();
        assert_eq!(second.name, "usr.Z");
        let bytes = kernel
            .to_buffer(
                who,
                conn,
                hmsg.into(),
                &query,
                1024,
                MqBufferCodec::KernelV1,
            )
            .unwrap();
        let other = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        assert_eq!(
            kernel
                .from_buffer(
                    who,
                    conn,
                    other.into(),
                    &bytes,
                    true,
                    MqBufferCodec::KernelV1
                )
                .unwrap(),
            Vec::<u8>::new()
        );
        assert_eq!(
            kernel
                .to_buffer(
                    who,
                    conn,
                    other.into(),
                    &query,
                    1024,
                    MqBufferCodec::KernelV1
                )
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn failures_leave_properties_and_handle_lifetime_unchanged() {
        let limits = MqMessageLimits {
            properties: 1,
            property_total_bytes: 8,
            property_value_bytes: 4,
            ..MqMessageLimits::default()
        };
        let mut kernel = MqHandleKernel::new(1, 4, limits).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let hmsg = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                hmsg.into(),
                property("a", b"1"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        let before = kernel
            .inquire(
                who,
                conn,
                hmsg.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default,
            )
            .unwrap();
        assert_eq!(
            kernel.set(
                who,
                conn,
                hmsg.into(),
                property("b", b"2"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Message(
                mainframe_env_host_api::MqMessageProblem::PropertyCount
            ))
        );
        assert_eq!(
            kernel.set(
                who,
                conn,
                hmsg.into(),
                property("a", b"12345"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Message(
                mainframe_env_host_api::MqMessageProblem::PropertyValueLength
            ))
        );
        assert_eq!(
            kernel.set(
                owner(9),
                conn,
                hmsg.into(),
                property("a", b"2"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::CrossOwner))
        );
        let object = kernel.create_object(who, conn).unwrap();
        assert_eq!(
            kernel.set(
                who,
                conn,
                MqHandle::Object(object),
                property("a", b"2"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::WrongKind))
        );
        assert_eq!(
            kernel.set(
                who,
                conn,
                hmsg.into(),
                property("a", b"2"),
                MqHandleKernelOption::PendingWire
            ),
            Err(MqHandleKernelProblem::InvalidOption)
        );
        assert_eq!(
            kernel
                .inquire(
                    who,
                    conn,
                    hmsg.into(),
                    &MqPropertyQuery::Exact("a".into()),
                    None,
                    4,
                    MqHandleKernelOption::Default
                )
                .unwrap(),
            before
        );
        kernel.begin_io(who, conn, hmsg).unwrap();
        assert_eq!(
            kernel.delete(who, conn, hmsg.into(), MqHandleKernelOption::Default),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::InUse))
        );
        kernel.end_io(who, conn, hmsg).unwrap();
        assert_eq!(
            kernel
                .inquire(
                    who,
                    conn,
                    hmsg.into(),
                    &MqPropertyQuery::Exact("a".into()),
                    None,
                    4,
                    MqHandleKernelOption::Default
                )
                .unwrap(),
            before
        );
    }

    #[test]
    fn epoch_and_reopen_cannot_resurrect_properties() {
        let mut kernel = MqHandleKernel::new(7, 3, MqMessageLimits::default()).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let hmsg = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                hmsg.into(),
                property("a", b"1"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        kernel.advance_epoch(8).unwrap();
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                hmsg.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
        let mut reopened = MqHandleKernel::new(8, 3, MqMessageLimits::default()).unwrap();
        let new_conn = reopened.connect(who, MqHandleSharing::NonShared).unwrap();
        assert_eq!(
            reopened.inquire(
                who,
                new_conn,
                hmsg.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
        let new_hmsg = reopened
            .create(who, new_conn, MqHandleKernelOption::Default)
            .unwrap();
        assert_eq!(
            reopened.inquire(
                who,
                new_conn,
                new_hmsg.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::NotFound)
        );
    }

    #[test]
    fn delete_reuse_and_connection_teardown_reject_stale_tokens() {
        let mut kernel = MqHandleKernel::new(1, 3, MqMessageLimits::default()).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let old = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                old.into(),
                property("a", b"old"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        kernel
            .delete(who, conn, old.into(), MqHandleKernelOption::Default)
            .unwrap();
        let next = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        assert_ne!(old, next);
        assert_eq!(
            kernel.set(
                who,
                conn,
                old.into(),
                property("a", b"bad"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                next.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::NotFound)
        );
        kernel.disconnect(who, conn).unwrap();
        assert_eq!(
            kernel.set(
                who,
                conn,
                next.into(),
                property("a", b"bad"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
        let new_conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let fresh = kernel
            .create(who, new_conn, MqHandleKernelOption::Default)
            .unwrap();
        assert_eq!(
            kernel.inquire(
                who,
                new_conn,
                fresh.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::NotFound)
        );
    }

    #[test]
    fn conversion_boundaries_and_malformed_frames_do_not_mutate() {
        let mut kernel = MqHandleKernel::new(1, 3, MqMessageLimits::default()).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let source = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                source.into(),
                property("a", b"1"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        let exact = MqPropertyQuery::Exact("a".into());
        let frame = kernel
            .to_buffer(
                who,
                conn,
                source.into(),
                &exact,
                128,
                MqBufferCodec::KernelV1,
            )
            .unwrap();
        assert_eq!(
            kernel.to_buffer(
                who,
                conn,
                source.into(),
                &exact,
                frame.len() - 1,
                MqBufferCodec::KernelV1
            ),
            Err(MqHandleKernelProblem::BufferTooSmall {
                required: frame.len()
            })
        );
        assert_eq!(
            kernel.to_buffer(
                who,
                conn,
                source.into(),
                &exact,
                128,
                MqBufferCodec::MqRfh2Pending
            ),
            Err(MqHandleKernelProblem::UnsupportedWire)
        );
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                source.into(),
                &exact,
                None,
                0,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::BufferTooSmall { required: 1 })
        );
        let target = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        for malformed in [
            b"MHK".as_slice(),
            &frame[..frame.len() - 1],
            &[b'X', b'H', b'K', b'1', 0, 0, 0, 0, 0, 0],
        ] {
            assert_eq!(
                kernel.from_buffer(
                    who,
                    conn,
                    target.into(),
                    malformed,
                    true,
                    MqBufferCodec::KernelV1
                ),
                Err(MqHandleKernelProblem::MalformedBuffer)
            );
        }
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                target.into(),
                &exact,
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::NotFound)
        );
        assert_eq!(
            kernel
                .from_buffer(
                    who,
                    conn,
                    target.into(),
                    &frame,
                    false,
                    MqBufferCodec::KernelV1
                )
                .unwrap(),
            frame
        );
        assert_eq!(
            kernel
                .inquire(
                    who,
                    conn,
                    target.into(),
                    &exact,
                    None,
                    4,
                    MqHandleKernelOption::Default
                )
                .unwrap(),
            property("a", b"1")
        );
        assert_eq!(
            kernel.delete_property(
                who,
                conn,
                target.into(),
                &MqPropertyQuery::Prefix("".into()),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::InvalidOption)
        );
        assert!(
            kernel
                .delete_property(
                    who,
                    conn,
                    target.into(),
                    &exact,
                    MqHandleKernelOption::Default
                )
                .unwrap()
        );
        assert!(
            !kernel
                .delete_property(
                    who,
                    conn,
                    target.into(),
                    &exact,
                    MqHandleKernelOption::Default
                )
                .unwrap()
        );
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                target.into(),
                &exact,
                None,
                4,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::NotFound)
        );
    }

    #[test]
    fn handle_slot_and_aggregate_bounds_fail_before_mutation() {
        let limits = MqMessageLimits {
            property_name_bytes: 2,
            property_value_bytes: 4,
            property_total_bytes: 6,
            ..MqMessageLimits::default()
        };
        let mut kernel = MqHandleKernel::new(1, 2, limits).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let hmsg = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        assert_eq!(
            kernel.create(who, conn, MqHandleKernelOption::Default),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Capacity))
        );
        kernel
            .set(
                who,
                conn,
                hmsg.into(),
                property("aa", b"1234"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        assert_eq!(
            kernel.set(
                who,
                conn,
                hmsg.into(),
                property("b", b"1"),
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Message(
                mainframe_env_host_api::MqMessageProblem::PropertyTotalLength
            ))
        );
        assert_eq!(
            kernel
                .inquire(
                    who,
                    conn,
                    hmsg.into(),
                    &MqPropertyQuery::Exact("aa".into()),
                    None,
                    4,
                    MqHandleKernelOption::Default
                )
                .unwrap(),
            property("aa", b"1234")
        );
    }

    #[test]
    fn unassociated_handle_survives_disconnect_until_its_unit_ends() {
        let mut kernel = MqHandleKernel::new(1, 3, MqMessageLimits::default()).unwrap();
        let who = owner(3);
        let conn = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        let handle = kernel
            .create(who, MqHconn::Unassociated, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                MqHconn::Unassociated,
                handle.into(),
                property("a", b"1"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        kernel.disconnect(who, conn).unwrap();
        assert_eq!(
            kernel.inquire(
                who,
                MqHconn::Unassociated,
                handle.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                1,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(
                MqHandleProblem::MissingConnection
            ))
        );
        let _reconnected = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        assert_eq!(
            kernel
                .inquire(
                    who,
                    MqHconn::Unassociated,
                    handle.into(),
                    &MqPropertyQuery::Exact("a".into()),
                    None,
                    1,
                    MqHandleKernelOption::Default
                )
                .unwrap(),
            property("a", b"1")
        );
        kernel.end_processing_unit(who).unwrap();
        assert!(kernel.properties.is_empty());
        let _again = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
        assert_eq!(
            kernel.inquire(
                who,
                MqHconn::Unassociated,
                handle.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                1,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
    }

    #[test]
    fn cics_default_handle_is_reclaimed_at_task_end() {
        let mut kernel = MqHandleKernel::new(1, 3, MqMessageLimits::default()).unwrap();
        let mut who = owner(3);
        who.environment = MqHostEnvironment::ZosCics;
        let conn = kernel.bind_cics_default(who).unwrap();
        let handle = kernel
            .create(who, conn, MqHandleKernelOption::Default)
            .unwrap();
        kernel
            .set(
                who,
                conn,
                handle.into(),
                property("a", b"1"),
                MqHandleKernelOption::Default,
            )
            .unwrap();
        kernel.end_processing_unit(who).unwrap();
        assert!(kernel.properties.is_empty());
        kernel.bind_cics_default(who).unwrap();
        assert_eq!(
            kernel.inquire(
                who,
                conn,
                handle.into(),
                &MqPropertyQuery::Exact("a".into()),
                None,
                1,
                MqHandleKernelOption::Default
            ),
            Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
        );
    }
}
