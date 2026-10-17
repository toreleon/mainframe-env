use super::*;
use crate::{CodecLimits, ModuleBuilder, StorageReference, decode_binary, encode_binary};

const TWO_MIB: usize = 2 * 1024 * 1024;

fn module(bytes: &[u8]) -> Module {
    let limits = IrLimits::default();
    let mut builder = ModuleBuilder::new(limits);
    let storage = builder
        .add_storage("AREA", bytes.len() as u64, None)
        .unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            cobol_chunked_initialization_identity(),
            Vec::new(),
            0,
            cobol_chunked_initialization_attributes("AREA", 0, bytes.len() as u64, bytes, limits)
                .unwrap(),
            vec![Effect::MemoryWrite],
            vec![StorageReference {
                storage,
                offset: 0,
                length: bytes.len() as u64,
            }],
            None,
        )
        .unwrap();
    builder.finish().unwrap()
}

fn first(module: &Module) -> &Operation {
    &module.regions()[0].blocks[0].operations[0]
}

fn patterned() -> Vec<u8> {
    (0..TWO_MIB)
        .map(|ordinal| ((ordinal / MAX_CHUNK_BYTES) * 113 + ordinal % 251) as u8)
        .collect()
}

#[test]
fn complete_two_mib_space_oracle_survives_original_binary_codec_limits() {
    let expected = vec![0x20; TWO_MIB];
    let module = module(&expected);
    validate_cobol_chunked_initialization(&module, first(&module)).unwrap();
    let bytes = encode_binary(&module, CodecLimits::default()).unwrap();
    let decoded = decode_binary(&bytes, CodecLimits::default()).unwrap();
    assert_eq!(
        cobol_initialization_bytes(first(&decoded)).unwrap(),
        expected
    );
    assert_eq!(first(&decoded).attributes.len(), 8);
    assert!(
        first(&decoded)
            .attributes
            .values()
            .all(|value| match value {
                Attribute::Bytes(bytes) => bytes.len() <= IrLimits::default().max_attribute_bytes,
                _ => true,
            })
    );
}

#[test]
fn complete_nonuniform_oracle_preserves_every_byte_across_chunk_boundary() {
    let expected = patterned();
    let module = module(&expected);
    assert_eq!(
        cobol_initialization_bytes(first(&module)).unwrap(),
        expected
    );
}

#[test]
fn missing_middle_ordinal_is_refused_before_full_vector_allocation() {
    let module = module(&patterned());
    let mut operation = first(&module).clone();
    let part = operation.attributes.remove(&chunk_name(1)).unwrap();
    operation.attributes.insert(chunk_name(2), part);
    assert!(cobol_initialization_bytes(&operation).is_err());
}

#[test]
fn overlapping_offset_vocabulary_is_refused() {
    let module = module(&patterned());
    let mut operation = first(&module).clone();
    operation
        .attributes
        .insert("initial_chunk_offset.000001".into(), Attribute::Integer(0));
    assert!(cobol_initialization_bytes(&operation).is_err());
}

#[test]
fn reversed_complete_payload_fails_the_independent_order_oracle() {
    let expected = patterned();
    let module = module(&expected);
    let mut operation = first(&module).clone();
    let first = operation.attributes.remove(&chunk_name(0)).unwrap();
    let second = operation.attributes.remove(&chunk_name(1)).unwrap();
    operation.attributes.insert(chunk_name(0), second);
    operation.attributes.insert(chunk_name(1), first);
    // Reversed raw values are structurally valid, but are different initial data.
    // The independent whole-vector oracle, not a self-digest, catches that error.
    assert_ne!(cobol_initialization_bytes(&operation).unwrap(), expected);
}

#[test]
fn truncation_extra_part_and_mixed_legacy_bytes_are_refused() {
    let module = module(&patterned());
    for mutation in 0..3 {
        let mut operation = first(&module).clone();
        match mutation {
            0 => {
                let Attribute::Bytes(bytes) = operation.attributes.get_mut(&chunk_name(1)).unwrap()
                else {
                    panic!("bytes part")
                };
                bytes.pop();
            }
            1 => {
                operation
                    .attributes
                    .insert(chunk_name(2), Attribute::Bytes(vec![0x20]));
            }
            _ => {
                operation
                    .attributes
                    .insert("initial".into(), Attribute::Bytes(vec![0x20]));
            }
        }
        assert!(cobol_initialization_bytes(&operation).is_err());
    }
}

#[test]
fn invalid_counts_sizes_total_and_wrong_chunk_types_are_refused() {
    let module = module(&patterned());
    for (name, value) in [
        ("initial_chunk_count", Attribute::Integer(i64::MAX)),
        ("initial_chunk_count", Attribute::Integer(1)),
        ("initial_chunk_count", Attribute::Integer(-1)),
        ("initial_chunk_size", Attribute::Integer(0)),
        (
            "initial_chunk_size",
            Attribute::Integer(MAX_CHUNK_BYTES as i64 + 1),
        ),
        ("initial_byte_count", Attribute::Integer(TWO_MIB as i64 + 1)),
        ("initial_byte_count", Attribute::Text("2097152".into())),
        ("initial_chunk.000001", Attribute::Text("spaces".into())),
    ] {
        let mut operation = first(&module).clone();
        operation.attributes.insert(name.into(), value);
        assert!(cobol_initialization_bytes(&operation).is_err(), "{name}");
    }
}

#[test]
fn partial_target_and_wrong_name_or_effect_are_refused() {
    let module = module(&patterned());
    for mutation in 0..5 {
        let mut operation = first(&module).clone();
        match mutation {
            0 => operation.storage[0].offset = 1,
            1 => operation.storage[0].length -= 1,
            2 => {
                operation
                    .attributes
                    .insert("name".into(), Attribute::Text("OTHER".into()));
            }
            3 => operation.effects.push(Effect::MemoryRead),
            _ => operation.storage.push(operation.storage[0].clone()),
        }
        assert!(validate_cobol_chunked_initialization(&module, &operation).is_err());
    }
}

#[test]
fn existing_attribute_and_count_bounds_remain_effective() {
    let limits = IrLimits::default();
    let initial = vec![0x20; TWO_MIB];
    let mut builder = ModuleBuilder::new(limits);
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    assert!(
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "init", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([("initial".into(), Attribute::Bytes(initial.clone()))]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .is_err()
    );
    let limited = IrLimits {
        max_attributes_per_operation: 7,
        ..limits
    };
    assert!(
        cobol_chunked_initialization_attributes("AREA", 0, TWO_MIB as u64, &initial, limited)
            .is_err()
    );
}

#[test]
fn smaller_existing_attribute_bound_is_honored_without_changing_bytes() {
    let expected = vec![0x20; TWO_MIB];
    let module = module(&expected);
    let mut operation = first(&module).clone();
    let limits = IrLimits {
        max_attribute_bytes: 512 * 1024,
        ..IrLimits::default()
    };
    operation.attributes =
        cobol_chunked_initialization_attributes("AREA", 0, TWO_MIB as u64, &expected, limits)
            .unwrap();
    assert_eq!(cobol_initialization_bytes(&operation).unwrap(), expected);
    assert_eq!(operation.attributes.len(), 10);
}

#[test]
fn legacy_singular_bytes_and_unknown_major_keep_exact_admission() {
    let module = module(&patterned());
    let mut operation = first(&module).clone();
    operation.identity = OperationIdentity::new("mainframe.core.cobol", "init", 1).unwrap();
    operation.attributes = BTreeMap::from([("initial".into(), Attribute::Bytes(b"OLD".to_vec()))]);
    assert_eq!(cobol_initialization_bytes(&operation).unwrap(), b"OLD");
    operation.identity = OperationIdentity::new("mainframe.core.cobol", "init", 3).unwrap();
    assert!(cobol_initialization_bytes(&operation).is_err());
}

fn module_with_compiler_storage_name(bytes: &[u8], stored_name: &str, name: &str) -> Module {
    let limits = IrLimits::default();
    let mut builder = ModuleBuilder::new(limits);
    let storage = builder
        .add_storage(stored_name, bytes.len() as u64, None)
        .unwrap();
    let _unrelated = builder
        .add_storage("UNRELATED", bytes.len() as u64, None)
        .unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            cobol_chunked_initialization_identity(),
            Vec::new(),
            0,
            cobol_chunked_initialization_attributes(name, 0, bytes.len() as u64, bytes, limits)
                .unwrap(),
            vec![Effect::MemoryWrite],
            vec![StorageReference {
                storage,
                offset: 0,
                length: bytes.len() as u64,
            }],
            None,
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn compiler_storage_case_preserves_exact_symbolic_target_and_complete_bytes() {
    let expected = vec![0x20; TWO_MIB];
    for stored_name in ["area", "ArEa", "AREA"] {
        let module = module_with_compiler_storage_name(&expected, stored_name, "AREA");
        let operation = first(&module);
        validate_cobol_chunked_initialization(&module, operation).unwrap();
        let mut catalog = crate::OperationCatalog::default();
        catalog
            .register(cobol_chunked_initialization_schema())
            .unwrap();
        crate::verify_semantic_contracts(&module, &catalog).unwrap();
        assert_eq!(operation.attributes["name"], Attribute::Text("AREA".into()));
        assert_eq!(operation.storage[0].storage, module.storage()[0].id);
        assert_eq!(operation.storage[0].offset, 0);
        assert_eq!(operation.storage[0].length, TWO_MIB as u64);
        assert_eq!(cobol_initialization_bytes(operation).unwrap(), expected);
    }
}

#[test]
fn symbolic_case_does_not_admit_other_ids_partial_views_effects_or_unicode_folding() {
    let expected = vec![0x20; TWO_MIB];
    let module = module_with_compiler_storage_name(&expected, "arEa", "AREA");
    for mutation in 0..6 {
        let mut operation = first(&module).clone();
        match mutation {
            0 => operation.storage[0].storage = module.storage()[1].id,
            1 => operation.storage[0].offset = 1,
            2 => operation.storage[0].length -= 1,
            3 => {
                operation
                    .attributes
                    .insert("name".into(), Attribute::Text("OTHER".into()));
            }
            4 => operation.effects.push(Effect::MemoryRead),
            _ => operation.storage.push(operation.storage[0].clone()),
        }
        assert!(validate_cobol_chunked_initialization(&module, &operation).is_err());
    }
    // Only ASCII presentation differs in compiler storage names. Broader Unicode
    // folding must not reinterpret a different declared symbolic name.
    let unicode_case = module_with_compiler_storage_name(&expected, "café", "CAFÉ");
    assert!(validate_cobol_chunked_initialization(&unicode_case, first(&unicode_case)).is_err());
    let unicode_exact = module_with_compiler_storage_name(&expected, "café", "café");
    validate_cobol_chunked_initialization(&unicode_exact, first(&unicode_exact)).unwrap();
}
