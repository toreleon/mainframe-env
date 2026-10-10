use super::*;
use mainframe_env_ir::{
    cobol_chunked_initialization_identity, cobol_initialization_bytes, verify_legal,
};

const ORIGINAL_SCALE_SOURCE: &str = include_str!("fixtures/memory-scale-32768.cbl");

fn published(source: &str) -> PublishedArtifact {
    let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
        .compile(request(source, CompilationMode::Executable))
        .unwrap()
    else {
        panic!("complete executable fixture")
    };
    artifact
}

#[test]
fn original_32768_by_64_source_publishes_complete_two_mib_space_bytes() {
    let artifact = published(ORIGINAL_SCALE_SOURCE);
    let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let initializers = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .filter(|operation| operation.identity.name() == "init")
        .collect::<Vec<_>>();
    let [initializer] = initializers.as_slice() else {
        panic!("one whole root initializer")
    };
    assert_eq!(
        initializer.identity,
        cobol_chunked_initialization_identity()
    );
    assert_eq!(
        cobol_initialization_bytes(initializer).unwrap(),
        vec![0x20; 2_097_152]
    );
    assert_eq!(initializer.storage.len(), 1);
    assert_eq!(initializer.storage[0].offset, 0);
    assert_eq!(initializer.storage[0].length, 2_097_152);
    assert_eq!(
        artifact.manifest().dialect_contracts,
        BTreeSet::from([
            "mainframe.core.cobol@1".into(),
            "mainframe.core.cobol@2".into()
        ])
    );
}

#[test]
fn legacy_catalog_refuses_the_new_identity_and_small_initializer_stays_legacy() {
    let artifact = published(ORIGINAL_SCALE_SOURCE);
    let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let current = core_mir_catalog();
    let mut old = mainframe_env_ir::OperationCatalog::default();
    for identity in current.identities() {
        if *identity != cobol_chunked_initialization_identity() {
            old.register(current.get(identity).unwrap().clone())
                .unwrap();
        }
    }
    let mut profile = core_mir_profile();
    profile
        .allowed_operations
        .remove(&cobol_chunked_initialization_identity());
    assert!(verify_legal(module, &old, &profile).is_err());
    let old_artifact = published(HELLO);
    let old_module = decode_binary(old_artifact.payload(), CodecLimits::default()).unwrap();
    assert!(verify_legal(old_module.clone(), &old, &profile).is_ok());
    let initial = old_module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .find(|operation| operation.identity.name() == "init")
        .unwrap();
    assert_eq!(initial.identity.major(), 1);
    assert_eq!(initial.attributes.len(), 4);
    assert_eq!(cobol_initialization_bytes(initial).unwrap(), b"HELLO");
}

#[test]
fn mixed_major_artifact_requires_exact_derived_dialect_manifest() {
    use mainframe_env_compiler_api::{ValidatedArtifact, VersionedArtifactManifest};
    let artifact = published(ORIGINAL_SCALE_SOURCE);
    let mut manifest = artifact.manifest().clone();
    assert!(manifest.dialect_contracts.remove("mainframe.core.cobol@2"));
    assert!(
        ValidatedArtifact::read(
            VersionedArtifactManifest::V3(manifest),
            artifact.payload(),
            &core_mir_catalog(),
            &core_mir_profile(),
            CodecLimits::default(),
            ArtifactLimits::default(),
        )
        .is_err()
    );
}
