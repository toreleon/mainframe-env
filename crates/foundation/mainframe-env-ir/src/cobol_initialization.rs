//! Local, versioned representation of an already-computed COBOL initializer.
//!
//! This contract splits bytes only. It does not select VALUE, OCCURS, padding,
//! truncation, character encoding, entry-context, or retained-state semantics.

use crate::{
    Attribute, Effect, IrLimits, Module, Operation, OperationIdentity, OperationSchema,
    OperationSemanticContract,
};
use std::collections::BTreeMap;

const METADATA_ATTRIBUTES: usize = 6;
const MAX_CHUNK_BYTES: usize = 1024 * 1024;

/// Identity intentionally differs from the existing singular-byte initializer.
pub fn cobol_chunked_initialization_identity() -> OperationIdentity {
    OperationIdentity::new("mainframe.core.cobol", "init", 2)
        .expect("static chunked COBOL initialization identity")
}

pub fn cobol_chunked_initialization_schema() -> OperationSchema {
    let mut schema = OperationSchema::pure(cobol_chunked_initialization_identity(), 0, 0);
    schema.allowed_effects.insert(Effect::MemoryWrite);
    schema.semantic_contract = OperationSemanticContract::CobolChunkedInitialization;
    schema
}

/// Encode the exact offered vector without modifying or interpreting its bytes.
/// Every part remains within the caller's original per-attribute bound.
pub fn cobol_chunked_initialization_attributes(
    name: &str,
    offset: u64,
    length: u64,
    initial: &[u8],
    limits: IrLimits,
) -> Result<BTreeMap<String, Attribute>, &'static str> {
    let chunk_size = limits.max_attribute_bytes.min(MAX_CHUNK_BYTES);
    if chunk_size == 0 || initial.len() <= chunk_size {
        return Err("chunked initializer requires multiple bounded chunks");
    }
    let chunk_count = initial.len().div_ceil(chunk_size);
    if chunk_count
        .checked_add(METADATA_ATTRIBUTES)
        .is_none_or(|count| count > limits.max_attributes_per_operation)
    {
        return Err("chunked initializer exceeds the existing attribute-count bound");
    }
    let integer = |value| {
        i64::try_from(value)
            .map(Attribute::Integer)
            .map_err(|_| "chunked initializer integer exceeds its existing wire width")
    };
    let mut attributes = BTreeMap::from([
        ("name".into(), Attribute::Text(name.into())),
        ("offset".into(), integer(offset)?),
        ("length".into(), integer(length)?),
        ("initial_byte_count".into(), integer(initial.len() as u64)?),
        ("initial_chunk_size".into(), integer(chunk_size as u64)?),
        ("initial_chunk_count".into(), integer(chunk_count as u64)?),
    ]);
    for (ordinal, chunk) in initial.chunks(chunk_size).enumerate() {
        attributes.insert(chunk_name(ordinal), Attribute::Bytes(chunk.to_vec()));
    }
    Ok(attributes)
}

fn chunk_name(ordinal: usize) -> String {
    format!("initial_chunk.{ordinal:06}")
}

fn integer(operation: &Operation, name: &str) -> Result<usize, &'static str> {
    match operation.attributes.get(name) {
        Some(Attribute::Integer(value)) => usize::try_from(*value)
            .map_err(|_| "chunked initializer requires nonnegative bounded integers"),
        _ => Err("chunked initializer integer is absent or has the wrong type"),
    }
}

/// Borrow all parts after proving exact canonical coverage. Offsets are inferred
/// from ordinal * chunk_size; arbitrary gaps, overlaps, and alternative ordering
/// cannot be declared by this wire representation.
fn chunks(operation: &Operation) -> Result<(usize, Vec<&[u8]>), &'static str> {
    if operation.identity != cobol_chunked_initialization_identity()
        || !operation.operands.is_empty()
        || !operation.results.is_empty()
        || operation.effects != [Effect::MemoryWrite]
    {
        return Err("chunked initializer identity or operation shape is invalid");
    }
    match operation.attributes.get("name") {
        Some(Attribute::Text(name)) if !name.is_empty() => {}
        _ => return Err("chunked initializer storage name is absent or invalid"),
    }
    integer(operation, "offset")?;
    if integer(operation, "length")? == 0 {
        return Err("chunked initializer target length is empty");
    }
    let total = integer(operation, "initial_byte_count")?;
    let size = integer(operation, "initial_chunk_size")?;
    let count = integer(operation, "initial_chunk_count")?;
    if size == 0
        || size > MAX_CHUNK_BYTES
        || count < 2
        || count > IrLimits::default().max_attributes_per_operation - METADATA_ATTRIBUTES
        || total <= size
        || total.div_ceil(size) != count
        || operation.attributes.len() != count + METADATA_ATTRIBUTES
    {
        return Err("chunked initializer count, extent, or vocabulary is invalid");
    }
    let mut parts = Vec::with_capacity(count);
    let mut remaining = total;
    for ordinal in 0..count {
        let Some(Attribute::Bytes(bytes)) = operation.attributes.get(&chunk_name(ordinal)) else {
            return Err("chunked initializer is missing a canonical part");
        };
        let expected = remaining.min(size);
        if bytes.len() != expected {
            return Err("chunked initializer part extent is incomplete or overlaps");
        }
        remaining -= expected;
        parts.push(bytes.as_slice());
    }
    if remaining != 0 {
        return Err("chunked initializer does not cover its complete byte extent");
    }
    Ok((total, parts))
}

/// Verify exact target-view binding as well as canonical byte-part coverage.
pub fn validate_cobol_chunked_initialization(
    module: &Module,
    operation: &Operation,
) -> Result<(), &'static str> {
    chunks(operation)?;
    let [reference] = operation.storage.as_slice() else {
        return Err("chunked initializer requires exactly one whole target view");
    };
    let storage = module
        .storage()
        .get(reference.storage.get() as usize)
        .ok_or("chunked initializer target storage is absent")?;
    let Some(Attribute::Text(name)) = operation.attributes.get("name") else {
        return Err("chunked initializer storage name is absent");
    };
    if !storage.name.eq_ignore_ascii_case(name)
        || reference.offset != 0
        || reference.length != storage.size
        || integer(operation, "length")? as u64 != storage.size
    {
        return Err("chunked initializer does not bind its exact named target view");
    }
    Ok(())
}

/// Return the exact complete vector for either the legacy or new operation.
/// Consumers continue to perform their existing whole-view fitting/write and
/// retained-state/entry-context precedence after this representation step.
pub fn cobol_initialization_bytes(operation: &Operation) -> Result<Vec<u8>, &'static str> {
    if operation.identity.namespace() == "mainframe.core.cobol"
        && operation.identity.name() == "init"
        && operation.identity.major() == 1
    {
        return match operation.attributes.get("initial") {
            Some(Attribute::Bytes(bytes)) => Ok(bytes.clone()),
            _ => Err("legacy initializer bytes are absent or have the wrong type"),
        };
    }
    let (total, parts) = chunks(operation)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| "chunked initializer cannot allocate its admitted byte extent")?;
    for part in parts {
        bytes.extend_from_slice(part);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
