//! Checked pointer-free COBOL MQ prefixes and exact observed field writeback.
//!
//! These observations are not canonical effect payloads, descriptors, handles or
//! operation/SAF authority. Embedding supplies the trusted structure encoding;
//! MQMD.Encoding describes the body and never selects this decoder's encoding.
//! No initialization, expiry scaling, output synthesis or native-endian fallback.

use crate::mq_mqi::MqMqiCall;

mod generated {
    use super::*;
    include!("mq_raw_layout/generated.rs");
}
#[cfg(test)]
mod tests;

pub use generated::MQ_RAW_LAYOUT_PROJECTION_SHA256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawLayoutKind {
    Od1,
    Md1,
    Md2,
    Gmo1,
    Pmo1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawNumberEncoding {
    NormalBigEndian,
    ReversedLittleEndian,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawCharacterEncoding {
    /// ASCII-compatible single-byte structure identifiers. Other chars remain raw.
    AsciiCompatible,
    /// Existing owned CP037 embedding profile; not an MQ-required CCSID assertion.
    OwnedCp037,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawStructureEncoding {
    pub numbers: MqRawNumberEncoding,
    pub characters: MqRawCharacterEncoding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawFieldKind {
    Long,
    Characters,
    Bytes,
    /// A four-byte observed alias, never an executable handle.
    Alias,
    /// Retained exactly; z/OS SET_SIGNAL can interpret this slot as a pointer.
    SignalSlot,
}

/// Reviewed initial observations only, never defaults for missing input fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawInitialValue {
    Long(i32),
    Identifier,
    Blanks,
    Nulls,
    Environment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WritebackPolicy {
    Input,
    Get,
    GetPut,
    DynamicOpen,
    PutCountOther,
    SingleQueuePut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawFieldDescriptor {
    pub name: &'static str,
    pub kind: MqRawFieldKind,
    pub offset: usize,
    pub width: usize,
    pub initial: MqRawInitialValue,
    writeback: WritebackPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawLayoutDescriptor {
    pub kind: MqRawLayoutKind,
    pub version: i32,
    pub prefix_bytes: usize,
    pub fields: &'static [MqRawFieldDescriptor],
    ascii_identifier: [u8; 4],
    cp037_identifier: [u8; 4],
}

pub fn mq_raw_layout(kind: MqRawLayoutKind) -> &'static MqRawLayoutDescriptor {
    generated::LAYOUTS
        .iter()
        .find(|layout| layout.kind == kind)
        .expect("generated layout")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawProblem {
    UnsupportedEncoding,
    Capacity,
    StructureIdentifier,
    Version,
    Field,
    FieldKind,
    FieldWidth,
    CobolLongRange,
    ObservationCount,
    DuplicateField,
    OutputPending,
    StaleCapture,
    /// Complete MQMD fields exceed the current typed descriptor vocabulary.
    DescriptorRepresentationPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawFieldValue<'a> {
    Long(i32),
    Characters(&'a [u8]),
    Bytes(&'a [u8]),
    Alias(i32),
    SignalSlot(&'a [u8]),
}

/// Raw 32-bit observations remain available even outside COBOL PIC S9(9) range.
/// This separate check is required before using a scalar as a COBOL MQLONG input.
pub fn mq_raw_cobol_long(value: i64) -> Result<i32, MqRawProblem> {
    let value = i32::try_from(value).map_err(|_| MqRawProblem::CobolLongRange)?;
    if !(generated::COBOL_LONG_MIN..=generated::COBOL_LONG_MAX).contains(&value) {
        return Err(MqRawProblem::CobolLongRange);
    }
    Ok(value)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRawCapture {
    layout: &'static MqRawLayoutDescriptor,
    encoding: MqRawStructureEncoding,
    capacity: usize,
    prefix: [u8; generated::MAX_PREFIX],
}

impl MqRawCapture {
    pub fn capture(
        kind: MqRawLayoutKind,
        bytes: &[u8],
        encoding: MqRawStructureEncoding,
    ) -> Result<Self, MqRawProblem> {
        let layout = mq_raw_layout(kind);
        let identifier = match encoding.characters {
            MqRawCharacterEncoding::AsciiCompatible => layout.ascii_identifier,
            MqRawCharacterEncoding::OwnedCp037 => layout.cp037_identifier,
            MqRawCharacterEncoding::Unsupported => return Err(MqRawProblem::UnsupportedEncoding),
        };
        if encoding.numbers == MqRawNumberEncoding::Unsupported {
            return Err(MqRawProblem::UnsupportedEncoding);
        }
        if bytes.len() < layout.prefix_bytes {
            return Err(MqRawProblem::Capacity);
        }
        let id = &layout.fields[0];
        if bytes[id.offset..id.offset + id.width] != identifier {
            return Err(MqRawProblem::StructureIdentifier);
        }
        let version = &layout.fields[1];
        if read_long(
            &bytes[version.offset..version.offset + version.width],
            encoding.numbers,
        )? != layout.version
        {
            return Err(MqRawProblem::Version);
        }
        let mut prefix = [0; generated::MAX_PREFIX];
        prefix[..layout.prefix_bytes].copy_from_slice(&bytes[..layout.prefix_bytes]);
        Ok(Self {
            layout,
            encoding,
            capacity: bytes.len(),
            prefix,
        })
    }

    pub fn layout(&self) -> &'static MqRawLayoutDescriptor {
        self.layout
    }
    pub fn encoding(&self) -> MqRawStructureEncoding {
        self.encoding
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn prefix(&self) -> &[u8] {
        &self.prefix[..self.layout.prefix_bytes]
    }

    pub fn field(&self, name: &str) -> Result<MqRawFieldValue<'_>, MqRawProblem> {
        let descriptor = self
            .layout
            .fields
            .iter()
            .find(|field| field.name == name)
            .ok_or(MqRawProblem::Field)?;
        let bytes = &self.prefix[descriptor.offset..descriptor.offset + descriptor.width];
        Ok(match descriptor.kind {
            MqRawFieldKind::Long => MqRawFieldValue::Long(read_long(bytes, self.encoding.numbers)?),
            MqRawFieldKind::Characters => MqRawFieldValue::Characters(bytes),
            MqRawFieldKind::Bytes => MqRawFieldValue::Bytes(bytes),
            MqRawFieldKind::Alias => {
                MqRawFieldValue::Alias(read_long(bytes, self.encoding.numbers)?)
            }
            MqRawFieldKind::SignalSlot => MqRawFieldValue::SignalSlot(bytes),
        })
    }

    /// No subset projection may discard the remainder of an observed MQMD.
    pub fn try_typed_descriptor(&self) -> Result<crate::MqMessageDescriptor, MqRawProblem> {
        if matches!(
            self.layout.kind,
            MqRawLayoutKind::Md1 | MqRawLayoutKind::Md2
        ) {
            Err(MqRawProblem::DescriptorRepresentationPending)
        } else {
            Err(MqRawProblem::FieldKind)
        }
    }

    /// Bounded atomic copy after complete preflight. Actual observations and
    /// trusted applicability come from the caller, never an inferred MQRC/name.
    /// Omitted, unchanged and undefined fields preserve their exact input bytes.
    pub fn writeback(
        &self,
        context: MqRawWritebackContext,
        observations: &[MqRawObservedField<'_>],
        destination: &mut [u8],
    ) -> Result<(), MqRawProblem> {
        if observations.len() > self.layout.fields.len() {
            return Err(MqRawProblem::ObservationCount);
        }
        if destination.len() != self.capacity {
            return Err(MqRawProblem::Capacity);
        }
        if destination[..self.layout.prefix_bytes] != *self.prefix() {
            return Err(MqRawProblem::StaleCapture);
        }
        if !call_matches(self.layout.kind, context.call) {
            return Err(MqRawProblem::OutputPending);
        }
        let mut next = self.prefix;
        for (index, observation) in observations.iter().enumerate() {
            if observations[..index]
                .iter()
                .any(|prior| prior.field == observation.field)
            {
                return Err(MqRawProblem::DuplicateField);
            }
            let field = self
                .layout
                .fields
                .iter()
                .find(|field| field.name == observation.field)
                .ok_or(MqRawProblem::Field)?;
            let MqRawObservation::Observed(value) = observation.observation else {
                continue;
            };
            let permitted = match field.writeback {
                WritebackPolicy::Input => false,
                WritebackPolicy::Get => context.call == MqMqiCall::Get,
                WritebackPolicy::GetPut => matches!(
                    context.call,
                    MqMqiCall::Get | MqMqiCall::Put | MqMqiCall::PutOne
                ),
                WritebackPolicy::DynamicOpen => {
                    context.call == MqMqiCall::Open && context.dynamic_model_open
                }
                WritebackPolicy::PutCountOther => {
                    context.platform == MqRawPlatform::Other
                        && matches!(context.call, MqMqiCall::Put | MqMqiCall::PutOne)
                }
                WritebackPolicy::SingleQueuePut => {
                    context.single_queue
                        && matches!(context.call, MqMqiCall::Put | MqMqiCall::PutOne)
                }
            };
            if !permitted {
                return Err(MqRawProblem::OutputPending);
            }
            if matches!(context.call, MqMqiCall::Put | MqMqiCall::PutOne) && !context.single_queue {
                return Err(MqRawProblem::OutputPending);
            }
            let target = &mut next[field.offset..field.offset + field.width];
            match (field.kind, value) {
                (MqRawFieldKind::Long, MqRawFieldValue::Long(value)) => {
                    if field.writeback == WritebackPolicy::PutCountOther && value < 0 {
                        return Err(MqRawProblem::OutputPending);
                    }
                    mq_raw_cobol_long(i64::from(value))?;
                    target.copy_from_slice(&write_long(value, self.encoding.numbers)?);
                }
                (MqRawFieldKind::Characters, MqRawFieldValue::Characters(bytes))
                | (MqRawFieldKind::Bytes, MqRawFieldValue::Bytes(bytes)) => {
                    if bytes.len() != field.width {
                        return Err(MqRawProblem::FieldWidth);
                    }
                    target.copy_from_slice(bytes);
                }
                _ => return Err(MqRawProblem::FieldKind),
            }
        }
        destination[..self.layout.prefix_bytes].copy_from_slice(&next[..self.layout.prefix_bytes]);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawPlatform {
    Zos,
    Other,
}

/// Trusted applicability observations, not caller permissions or operation results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawWritebackContext {
    pub call: MqMqiCall,
    pub platform: MqRawPlatform,
    pub single_queue: bool,
    pub dynamic_model_open: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawObservation<'a> {
    Unchanged,
    Undefined,
    Observed(MqRawFieldValue<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawObservedField<'a> {
    pub field: &'a str,
    pub observation: MqRawObservation<'a>,
}

fn call_matches(kind: MqRawLayoutKind, call: MqMqiCall) -> bool {
    match kind {
        MqRawLayoutKind::Od1 => matches!(call, MqMqiCall::Open | MqMqiCall::PutOne),
        MqRawLayoutKind::Md1 | MqRawLayoutKind::Md2 => {
            matches!(call, MqMqiCall::Get | MqMqiCall::Put | MqMqiCall::PutOne)
        }
        MqRawLayoutKind::Gmo1 => call == MqMqiCall::Get,
        MqRawLayoutKind::Pmo1 => matches!(call, MqMqiCall::Put | MqMqiCall::PutOne),
    }
}

fn read_long(bytes: &[u8], encoding: MqRawNumberEncoding) -> Result<i32, MqRawProblem> {
    let bytes = <[u8; 4]>::try_from(bytes).map_err(|_| MqRawProblem::FieldWidth)?;
    match encoding {
        MqRawNumberEncoding::NormalBigEndian => Ok(i32::from_be_bytes(bytes)),
        MqRawNumberEncoding::ReversedLittleEndian => Ok(i32::from_le_bytes(bytes)),
        MqRawNumberEncoding::Unsupported => Err(MqRawProblem::UnsupportedEncoding),
    }
}

fn write_long(value: i32, encoding: MqRawNumberEncoding) -> Result<[u8; 4], MqRawProblem> {
    match encoding {
        MqRawNumberEncoding::NormalBigEndian => Ok(value.to_be_bytes()),
        MqRawNumberEncoding::ReversedLittleEndian => Ok(value.to_le_bytes()),
        MqRawNumberEncoding::Unsupported => Err(MqRawProblem::UnsupportedEncoding),
    }
}
