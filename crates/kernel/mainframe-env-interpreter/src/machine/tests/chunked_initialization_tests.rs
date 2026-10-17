use super::*;
use mainframe_env_ir::{
    StorageReference, cobol_chunked_initialization_attributes,
    cobol_chunked_initialization_identity,
};

const TWO_MIB: usize = 2 * 1024 * 1024;

fn machine(initial: &[u8], target_length: usize) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let storage = builder
        .add_storage("AREA", target_length as u64, None)
        .unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            cobol_chunked_initialization_identity(),
            Vec::new(),
            0,
            cobol_chunked_initialization_attributes(
                "AREA",
                0,
                target_length as u64,
                initial,
                IrLimits::default(),
            )
            .unwrap(),
            vec![Effect::MemoryWrite],
            vec![StorageReference {
                storage,
                offset: 0,
                length: target_length as u64,
            }],
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "halt", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
    let binary =
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
            .unwrap();
    ReferenceMachine::from_binary(&binary, invocation(), CodecLimits::default()).unwrap()
}

#[test]
fn complete_two_mib_spaces_are_written_to_one_whole_view() {
    let expected = vec![0x20; TWO_MIB];
    let mut machine = machine(&expected, TWO_MIB);
    let operation = machine.operations[0].clone();
    machine.execute(&operation).unwrap();
    assert_eq!(machine.bases, vec![expected]);
    assert!(supported_operations().contains(&cobol_chunked_initialization_identity()));
    assert!(
        !supported_operations().contains(&OperationIdentity::new(NAMESPACE, "init", 3).unwrap())
    );
}

#[test]
fn full_retained_entry_override_keeps_precedence_over_all_chunks() {
    let original = vec![0x20; TWO_MIB];
    let retained = (0..TWO_MIB)
        .map(|ordinal| (ordinal % 251) as u8)
        .collect::<Vec<_>>();
    let mut machine = machine(&original, TWO_MIB);
    let operation = machine.operations[0].clone();
    machine
        .entry_initials
        .insert(operation.storage[0].storage, retained.clone());
    machine.execute(&operation).unwrap();
    assert_eq!(machine.bases, vec![retained]);
}

#[test]
fn whole_view_padding_and_truncation_continue_after_full_assembly() {
    let original = vec![0x41; TWO_MIB];
    for target_length in [TWO_MIB - 1, TWO_MIB + 1] {
        let mut machine = machine(&original, target_length);
        let operation = machine.operations[0].clone();
        machine.execute(&operation).unwrap();
        let mut expected = vec![0x41; target_length];
        if target_length > TWO_MIB {
            expected[TWO_MIB] = 0x20;
        }
        assert_eq!(machine.bases, vec![expected]);
    }
}

#[test]
fn complete_nonuniform_initialized_view_is_preserved_through_snapshot_replay() {
    let expected = (0..TWO_MIB)
        .map(|ordinal| (ordinal % 251) as u8)
        .collect::<Vec<_>>();
    let mut first = machine(&expected, TWO_MIB);
    let operation = first.operations[0].clone();
    first.execute(&operation).unwrap();
    let snapshot = first.snapshot();
    let mut reopened = machine(&expected, TWO_MIB);
    reopened.restore(snapshot).unwrap();
    assert_eq!(reopened.bases, vec![expected]);
}
