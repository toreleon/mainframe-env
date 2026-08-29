use crate::{
    Attribute, BlockId, Effect, IrLimits, Module, ModuleBuilder, OperationIdentity, StorageId,
    StorageReference, TypeIdentity, ValueId,
};
use mainframe_env_diagnostics::SourceSpan;
use mainframe_env_source::FileId;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const MAGIC: &[u8; 4] = b"MEIR";
const VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodecLimits {
    pub max_envelope_bytes: usize,
    pub max_payload_bytes: usize,
    pub max_string_bytes: usize,
    pub ir: IrLimits,
}

impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            max_envelope_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 64 * 1024 * 1024,
            max_string_bytes: 4096,
            ir: IrLimits::default(),
        }
    }
}

pub fn encode_binary(module: &Module, limits: CodecLimits) -> Result<Vec<u8>, IrCodecProblem> {
    let mut payload = Vec::new();
    write_u32(&mut payload, module.value_count());
    write_count(&mut payload, module.storage().len())?;
    for storage in module.storage() {
        write_u32(&mut payload, storage.id.get());
        write_string(&mut payload, &storage.name, limits)?;
        write_u64(&mut payload, storage.size);
        match &storage.alias_of {
            Some(alias) => {
                payload.push(1);
                write_reference(&mut payload, alias);
            }
            None => payload.push(0),
        }
    }
    write_count(&mut payload, module.regions().len())?;
    for region in module.regions() {
        write_u32(&mut payload, region.id.get());
        write_count(&mut payload, region.blocks.len())?;
        for block in &region.blocks {
            write_u32(&mut payload, block.id.get());
            write_count(&mut payload, block.operations.len())?;
            for operation in &block.operations {
                write_u32(&mut payload, operation.id.get());
                write_string(&mut payload, operation.identity.namespace(), limits)?;
                write_string(&mut payload, operation.identity.name(), limits)?;
                write_u16(&mut payload, operation.identity.major());
                write_ids(&mut payload, &operation.operands, ValueId::get)?;
                write_ids(&mut payload, &operation.results, ValueId::get)?;
                write_count(&mut payload, operation.attributes.len())?;
                for (name, value) in &operation.attributes {
                    write_string(&mut payload, name, limits)?;
                    write_attribute(&mut payload, value, limits)?;
                }
                write_count(&mut payload, operation.effects.len())?;
                for effect in &operation.effects {
                    payload.push(effect_tag(*effect));
                }
                write_count(&mut payload, operation.storage.len())?;
                for reference in &operation.storage {
                    write_reference(&mut payload, reference);
                }
                match &operation.location {
                    Some(location) => {
                        payload.push(1);
                        write_u32(&mut payload, location.file.get());
                        write_u64(&mut payload, to_u64(location.bytes.start)?);
                        write_u64(&mut payload, to_u64(location.bytes.end)?);
                    }
                    None => payload.push(0),
                }
            }
        }
    }
    if payload.len() > limits.max_payload_bytes {
        return Err(IrCodecProblem::LimitExceeded);
    }
    let digest: [u8; 32] = Sha256::digest(&payload).into();
    let mut envelope = Vec::with_capacity(4 + 2 + 4 + 32 + payload.len());
    envelope.extend_from_slice(MAGIC);
    write_u16(&mut envelope, VERSION);
    write_u32(
        &mut envelope,
        u32::try_from(payload.len()).map_err(|_| IrCodecProblem::LimitExceeded)?,
    );
    envelope.extend_from_slice(&digest);
    envelope.extend_from_slice(&payload);
    if envelope.len() > limits.max_envelope_bytes {
        return Err(IrCodecProblem::LimitExceeded);
    }
    Ok(envelope)
}

pub fn decode_binary(bytes: &[u8], limits: CodecLimits) -> Result<Module, IrCodecProblem> {
    if bytes.len() > limits.max_envelope_bytes || bytes.len() < 42 {
        return Err(IrCodecProblem::LimitExceeded);
    }
    let mut envelope = Reader::new(bytes);
    if envelope.take(4)? != MAGIC {
        return Err(IrCodecProblem::BadMagic);
    }
    if envelope.u16()? != VERSION {
        return Err(IrCodecProblem::UnsupportedVersion);
    }
    let payload_len =
        usize::try_from(envelope.u32()?).map_err(|_| IrCodecProblem::LimitExceeded)?;
    if payload_len > limits.max_payload_bytes {
        return Err(IrCodecProblem::LimitExceeded);
    }
    let expected_digest = envelope.take(32)?;
    let payload = envelope.take(payload_len)?;
    if !envelope.remaining().is_empty() {
        return Err(IrCodecProblem::TrailingData);
    }
    let actual_digest: [u8; 32] = Sha256::digest(payload).into();
    if expected_digest != actual_digest {
        return Err(IrCodecProblem::IntegrityMismatch);
    }
    decode_payload(payload, limits)
}

pub fn to_text(module: &Module, limits: CodecLimits) -> Result<String, IrCodecProblem> {
    let binary = encode_binary(module, limits)?;
    let mut output = String::from("mainframe-env.ir-text@1\n");
    for region in module.regions() {
        writeln!(output, "region r{}", region.id.get())
            .map_err(|_| IrCodecProblem::LimitExceeded)?;
        for block in &region.blocks {
            writeln!(output, "  block b{}", block.id.get())
                .map_err(|_| IrCodecProblem::LimitExceeded)?;
            for operation in &block.operations {
                writeln!(
                    output,
                    "    o{} = {} operands={} results={} effects={:?}",
                    operation.id.get(),
                    operation.identity,
                    operation.operands.len(),
                    operation.results.len(),
                    operation.effects
                )
                .map_err(|_| IrCodecProblem::LimitExceeded)?;
            }
        }
    }
    output.push_str("payload ");
    for byte in binary {
        write!(output, "{byte:02x}").map_err(|_| IrCodecProblem::LimitExceeded)?;
    }
    output.push('\n');
    if output.len() > limits.max_envelope_bytes.saturating_mul(3) {
        return Err(IrCodecProblem::LimitExceeded);
    }
    Ok(output)
}

pub fn parse_text(text: &str, limits: CodecLimits) -> Result<Module, IrCodecProblem> {
    if text.len() > limits.max_envelope_bytes.saturating_mul(3)
        || !text.starts_with("mainframe-env.ir-text@1\n")
    {
        return Err(IrCodecProblem::UnsupportedVersion);
    }
    let payload = text.lines().last().ok_or(IrCodecProblem::Malformed)?;
    let hex = payload
        .strip_prefix("payload ")
        .ok_or(IrCodecProblem::Malformed)?;
    if hex.len() % 2 != 0 || hex.len() / 2 > limits.max_envelope_bytes {
        return Err(IrCodecProblem::LimitExceeded);
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().as_chunks::<2>().0 {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        bytes.push((high << 4) | low);
    }
    decode_binary(&bytes, limits)
}

fn decode_payload(payload: &[u8], limits: CodecLimits) -> Result<Module, IrCodecProblem> {
    let mut reader = Reader::new(payload);
    let declared_values = reader.u32()?;
    let storage_count = reader.count(limits.ir.max_storage_regions)?;
    let mut storage_inputs = Vec::with_capacity(storage_count);
    for expected in 0..storage_count {
        if reader.u32()? as usize != expected {
            return Err(IrCodecProblem::NonCanonicalId);
        }
        let name = reader.string(limits.max_string_bytes)?;
        let size = reader.u64()?;
        let alias = if reader.flag()? {
            Some(reader.reference()?)
        } else {
            None
        };
        storage_inputs.push((name, size, alias));
    }
    let mut builder = ModuleBuilder::new(limits.ir);
    for (name, size, alias) in storage_inputs {
        builder
            .add_storage(name, size, alias)
            .map_err(IrCodecProblem::Model)?;
    }
    let region_count = reader.count(limits.ir.max_regions)?;
    let mut block_index = 0usize;
    let mut operation_index = 0usize;
    for expected_region in 0..region_count {
        if reader.u32()? as usize != expected_region {
            return Err(IrCodecProblem::NonCanonicalId);
        }
        let region = builder.add_region().map_err(IrCodecProblem::Model)?;
        let block_count = reader.count(limits.ir.max_blocks.saturating_sub(block_index))?;
        for _ in 0..block_count {
            if reader.u32()? as usize != block_index {
                return Err(IrCodecProblem::NonCanonicalId);
            }
            let block = builder.add_block(region).map_err(IrCodecProblem::Model)?;
            block_index += 1;
            let operation_count =
                reader.count(limits.ir.max_operations.saturating_sub(operation_index))?;
            for _ in 0..operation_count {
                if reader.u32()? as usize != operation_index {
                    return Err(IrCodecProblem::NonCanonicalId);
                }
                let identity = OperationIdentity::new(
                    reader.string(limits.max_string_bytes)?,
                    reader.string(limits.max_string_bytes)?,
                    reader.u16()?,
                )
                .map_err(IrCodecProblem::Model)?;
                let operands = reader.value_ids(limits.ir.max_operands_per_operation)?;
                let encoded_results = reader.value_ids(limits.ir.max_results_per_operation)?;
                let attribute_count = reader.count(limits.ir.max_attributes_per_operation)?;
                let mut attributes = BTreeMap::new();
                for _ in 0..attribute_count {
                    let name = reader.string(limits.max_string_bytes)?;
                    let value = reader.attribute(limits)?;
                    if attributes.insert(name, value).is_some() {
                        return Err(IrCodecProblem::Malformed);
                    }
                }
                let effect_count = reader.count(limits.ir.max_effects_per_operation)?;
                let mut effects = Vec::with_capacity(effect_count);
                for _ in 0..effect_count {
                    effects.push(effect_from_tag(reader.byte()?)?);
                }
                let reference_count = reader.count(limits.ir.max_references_per_operation)?;
                let mut references = Vec::with_capacity(reference_count);
                for _ in 0..reference_count {
                    references.push(reader.reference()?);
                }
                let location = if reader.flag()? {
                    let file = FileId::new(reader.u32()?).map_err(|_| IrCodecProblem::Malformed)?;
                    let start = reader.usize()?;
                    let end = reader.usize()?;
                    Some(SourceSpan::new(file, start..end).map_err(|_| IrCodecProblem::Malformed)?)
                } else {
                    None
                };
                let operation = builder
                    .add_operation(
                        block,
                        identity,
                        operands,
                        encoded_results.len(),
                        attributes,
                        effects,
                        references,
                        location,
                    )
                    .map_err(IrCodecProblem::Model)?;
                if operation.get() as usize != operation_index {
                    return Err(IrCodecProblem::NonCanonicalId);
                }
                let built_results = operation_results(&builder, block, operation)?;
                if built_results != encoded_results {
                    return Err(IrCodecProblem::NonCanonicalId);
                }
                operation_index += 1;
            }
        }
    }
    if !reader.remaining().is_empty() {
        return Err(IrCodecProblem::TrailingData);
    }
    let module = builder.finish().map_err(IrCodecProblem::Model)?;
    if module.value_count() != declared_values {
        return Err(IrCodecProblem::NonCanonicalId);
    }
    Ok(module)
}

// The builder intentionally has no general mutable access. This narrow read is
// implemented through the operation's deterministic position after insertion.
fn operation_results(
    builder: &ModuleBuilder,
    block: BlockId,
    operation: crate::OperationId,
) -> Result<Vec<ValueId>, IrCodecProblem> {
    builder
        .operation_results(block, operation)
        .map(ToOwned::to_owned)
        .ok_or(IrCodecProblem::Malformed)
}

fn write_attribute(
    output: &mut Vec<u8>,
    value: &Attribute,
    limits: CodecLimits,
) -> Result<(), IrCodecProblem> {
    match value {
        Attribute::Integer(value) => {
            output.push(0);
            output.extend_from_slice(&value.to_be_bytes());
        }
        Attribute::Boolean(value) => {
            output.push(1);
            output.push(u8::from(*value));
        }
        Attribute::Text(value) => {
            output.push(2);
            write_string(output, value, limits)?;
        }
        Attribute::Bytes(value) => {
            output.push(3);
            if value.len() > limits.ir.max_attribute_bytes {
                return Err(IrCodecProblem::LimitExceeded);
            }
            write_count(output, value.len())?;
            output.extend_from_slice(value);
        }
        Attribute::Type(value) => {
            output.push(4);
            write_string(output, value.namespace(), limits)?;
            write_string(output, value.name(), limits)?;
            write_u16(output, value.major());
        }
    }
    Ok(())
}

fn write_ids<T: Copy>(
    output: &mut Vec<u8>,
    ids: &[T],
    get: fn(T) -> u32,
) -> Result<(), IrCodecProblem> {
    write_count(output, ids.len())?;
    for id in ids {
        write_u32(output, get(*id));
    }
    Ok(())
}

fn write_reference(output: &mut Vec<u8>, reference: &StorageReference) {
    write_u32(output, reference.storage.get());
    write_u64(output, reference.offset);
    write_u64(output, reference.length);
}

fn write_string(
    output: &mut Vec<u8>,
    value: &str,
    limits: CodecLimits,
) -> Result<(), IrCodecProblem> {
    if value.len() > limits.max_string_bytes || value.len() > u16::MAX as usize {
        return Err(IrCodecProblem::LimitExceeded);
    }
    write_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_count(output: &mut Vec<u8>, count: usize) -> Result<(), IrCodecProblem> {
    write_u32(
        output,
        u32::try_from(count).map_err(|_| IrCodecProblem::LimitExceeded)?,
    );
    Ok(())
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}
fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}
fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}
fn to_u64(value: usize) -> Result<u64, IrCodecProblem> {
    u64::try_from(value).map_err(|_| IrCodecProblem::LimitExceeded)
}

fn effect_tag(effect: Effect) -> u8 {
    match effect {
        Effect::MemoryRead => 0,
        Effect::MemoryWrite => 1,
        Effect::DatasetRead => 2,
        Effect::DatasetWrite => 3,
        Effect::TerminalRead => 4,
        Effect::TerminalWrite => 5,
        Effect::ProgramControl => 6,
        Effect::Security => 7,
        Effect::Spool => 8,
        Effect::Clock => 9,
        Effect::Audit => 10,
        Effect::Suspension => 11,
        Effect::Condition => 12,
        Effect::Transaction => 13,
    }
}

fn effect_from_tag(tag: u8) -> Result<Effect, IrCodecProblem> {
    Ok(match tag {
        0 => Effect::MemoryRead,
        1 => Effect::MemoryWrite,
        2 => Effect::DatasetRead,
        3 => Effect::DatasetWrite,
        4 => Effect::TerminalRead,
        5 => Effect::TerminalWrite,
        6 => Effect::ProgramControl,
        7 => Effect::Security,
        8 => Effect::Spool,
        9 => Effect::Clock,
        10 => Effect::Audit,
        11 => Effect::Suspension,
        12 => Effect::Condition,
        13 => Effect::Transaction,
        _ => return Err(IrCodecProblem::Malformed),
    })
}

fn hex_digit(byte: u8) -> Result<u8, IrCodecProblem> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(IrCodecProblem::Malformed),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], IrCodecProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(IrCodecProblem::LimitExceeded)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(IrCodecProblem::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, IrCodecProblem> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, IrCodecProblem> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(IrCodecProblem::Malformed),
        }
    }
    fn u16(&mut self) -> Result<u16, IrCodecProblem> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| IrCodecProblem::Truncated)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, IrCodecProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| IrCodecProblem::Truncated)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, IrCodecProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| IrCodecProblem::Truncated)?,
        ))
    }
    fn usize(&mut self) -> Result<usize, IrCodecProblem> {
        usize::try_from(self.u64()?).map_err(|_| IrCodecProblem::LimitExceeded)
    }
    fn count(&mut self, max: usize) -> Result<usize, IrCodecProblem> {
        let count = usize::try_from(self.u32()?).map_err(|_| IrCodecProblem::LimitExceeded)?;
        if count > max {
            Err(IrCodecProblem::LimitExceeded)
        } else {
            Ok(count)
        }
    }
    fn string(&mut self, max: usize) -> Result<String, IrCodecProblem> {
        let length = usize::from(self.u16()?);
        if length > max {
            return Err(IrCodecProblem::LimitExceeded);
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| IrCodecProblem::Malformed)
    }
    fn value_ids(&mut self, max: usize) -> Result<Vec<ValueId>, IrCodecProblem> {
        let count = self.count(max)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(ValueId::from_index(self.u32()? as usize).map_err(IrCodecProblem::Model)?);
        }
        Ok(values)
    }
    fn reference(&mut self) -> Result<StorageReference, IrCodecProblem> {
        Ok(StorageReference {
            storage: StorageId::from_index(self.u32()? as usize).map_err(IrCodecProblem::Model)?,
            offset: self.u64()?,
            length: self.u64()?,
        })
    }
    fn attribute(&mut self, limits: CodecLimits) -> Result<Attribute, IrCodecProblem> {
        Ok(match self.byte()? {
            0 => Attribute::Integer(i64::from_be_bytes(
                self.take(8)?
                    .try_into()
                    .map_err(|_| IrCodecProblem::Truncated)?,
            )),
            1 => Attribute::Boolean(self.flag()?),
            2 => Attribute::Text(self.string(limits.max_string_bytes)?),
            3 => {
                let length = self.count(limits.ir.max_attribute_bytes)?;
                Attribute::Bytes(self.take(length)?.to_vec())
            }
            4 => Attribute::Type(
                TypeIdentity::new(
                    self.string(limits.max_string_bytes)?,
                    self.string(limits.max_string_bytes)?,
                    self.u16()?,
                )
                .map_err(IrCodecProblem::Model)?,
            ),
            _ => return Err(IrCodecProblem::Malformed),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IrCodecProblem {
    BadMagic,
    UnsupportedVersion,
    Truncated,
    TrailingData,
    IntegrityMismatch,
    Malformed,
    NonCanonicalId,
    LimitExceeded,
    Model(crate::IrProblem),
}

impl std::fmt::Display for IrCodecProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "IR codec failed: {self:?}")
    }
}
impl std::error::Error for IrCodecProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn module() -> Module {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let storage = builder.add_storage("working", 16, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core", "return", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                vec![Effect::ProgramControl],
                vec![StorageReference {
                    storage,
                    offset: 0,
                    length: 1,
                }],
                None,
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[test]
    fn binary_and_text_roundtrip() {
        let module = module();
        let limits = CodecLimits::default();
        let binary = encode_binary(&module, limits).unwrap();
        assert_eq!(decode_binary(&binary, limits).unwrap(), module);
        let text = to_text(&module, limits).unwrap();
        assert!(text.contains("mainframe.core@1.return"));
        assert_eq!(parse_text(&text, limits).unwrap(), module);
    }

    #[test]
    fn corruption_fails_integrity() {
        let limits = CodecLimits::default();
        let mut binary = encode_binary(&module(), limits).unwrap();
        *binary.last_mut().unwrap() ^= 1;
        assert_eq!(
            decode_binary(&binary, limits),
            Err(IrCodecProblem::IntegrityMismatch)
        );
    }

    #[test]
    fn hostile_declared_length_fails_before_allocation() {
        let mut binary = vec![0; 42];
        binary[..4].copy_from_slice(MAGIC);
        binary[4..6].copy_from_slice(&VERSION.to_be_bytes());
        binary[6..10].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            decode_binary(&binary, CodecLimits::default()),
            Err(IrCodecProblem::LimitExceeded)
        );
    }

    proptest! {
        #[test]
        fn arbitrary_bounded_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let limits = CodecLimits { max_envelope_bytes: 4096, max_payload_bytes: 4096, ..CodecLimits::default() };
            let _ = decode_binary(&bytes, limits);
        }
    }
}
