use crate::{
    ARTIFACT_CONTRACT, CompileOptions, CompileTarget, CompilerProblem, LEGACY_ARTIFACT_CONTRACT,
    LegalizedMir,
};
use mainframe_env_ir::{
    CodecLimits, LegalModule, LegalityProfile, OperationCatalog, decode_binary, encode_binary,
    verify_legal,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactContentId([u8; 32]);

impl ArtifactContentId {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[must_use]
    pub fn to_reference(self) -> String {
        format!("sha256:{}", self.to_hex())
    }
}

impl fmt::Debug for ArtifactContentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ArtifactContentId")
            .field(&self.to_hex())
            .finish()
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticArtifactId([u8; 32]);

impl SemanticArtifactId {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[must_use]
    pub fn to_reference(self) -> String {
        format!("semantic-sha256:{}", self.to_hex())
    }
}

impl fmt::Debug for SemanticArtifactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SemanticArtifactId")
            .field(&self.to_hex())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactLimits {
    pub max_payload_bytes: usize,
    pub max_generation_bytes: usize,
    pub max_host_interfaces: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 64 * 1024 * 1024,
            max_generation_bytes: 256,
            max_host_interfaces: 128,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactManifest {
    pub compiler_generation: String,
    pub target: CompileTarget,
    pub options: CompileOptions,
    pub host_interfaces: BTreeSet<String>,
    pub ir_contract: String,
    /// Exact executable dialect namespace/major pairs required by the payload.
    ///
    /// Entries use the canonical `namespace@major` form. The publisher derives
    /// the same set from operation identities and refuses a stale or incomplete
    /// manifest, so an artifact cannot silently acquire a different dialect
    /// meaning without changing its semantic identity.
    pub dialect_contracts: BTreeSet<String>,
}

/// Historical version-2 manifest, before executable dialects were declared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactManifestV2 {
    /// Compiler generation that produced the historical payload.
    pub compiler_generation: String,
    /// Executable target selected during compilation.
    pub target: CompileTarget,
    /// Normalized compiler options bound to the artifact identity.
    pub options: CompileOptions,
    /// Host interfaces required to execute the payload.
    pub host_interfaces: BTreeSet<String>,
    /// Binary IR envelope contract used by the payload.
    pub ir_contract: String,
}

/// Manifest shape supplied to the explicit artifact reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VersionedArtifactManifest {
    /// Historical pre-dialect manifest migrated after payload verification.
    V2(ArtifactManifestV2),
    /// Current manifest whose dialect declarations must match the payload.
    V3(ArtifactManifest),
}

impl VersionedArtifactManifest {
    /// Return the artifact contract represented by this manifest shape.
    #[must_use]
    pub const fn contract(&self) -> &'static str {
        match self {
            Self::V2(_) => LEGACY_ARTIFACT_CONTRACT,
            Self::V3(_) => ARTIFACT_CONTRACT,
        }
    }
}

impl ArtifactManifest {
    pub fn validate(&self, limits: ArtifactLimits) -> Result<(), CompilerProblem> {
        validate_common_manifest(
            &self.compiler_generation,
            &self.ir_contract,
            &self.host_interfaces,
            limits,
        )?;
        if self.dialect_contracts.is_empty()
            || self.dialect_contracts.len() > limits.max_host_interfaces
            || self.dialect_contracts.iter().any(|item| {
                let Some((namespace, major)) = item.rsplit_once('@') else {
                    return true;
                };
                namespace.is_empty()
                    || item.len() > limits.max_generation_bytes
                    || major.parse::<u16>().ok().is_none_or(|major| major == 0)
            })
        {
            return Err(CompilerProblem::InvalidGeneration);
        }
        Ok(())
    }
}

impl ArtifactManifestV2 {
    /// Validate the bounded fields retained by a version-2 manifest.
    pub fn validate(&self, limits: ArtifactLimits) -> Result<(), CompilerProblem> {
        validate_common_manifest(
            &self.compiler_generation,
            &self.ir_contract,
            &self.host_interfaces,
            limits,
        )
    }

    fn migrate(self, dialect_contracts: BTreeSet<String>) -> ArtifactManifest {
        ArtifactManifest {
            compiler_generation: self.compiler_generation,
            target: self.target,
            options: self.options,
            host_interfaces: self.host_interfaces,
            ir_contract: self.ir_contract,
            dialect_contracts,
        }
    }
}

fn validate_common_manifest(
    compiler_generation: &str,
    ir_contract: &str,
    host_interfaces: &BTreeSet<String>,
    limits: ArtifactLimits,
) -> Result<(), CompilerProblem> {
    if compiler_generation.is_empty()
        || compiler_generation.len() > limits.max_generation_bytes
        || ir_contract.is_empty()
        || ir_contract.len() > limits.max_generation_bytes
        || host_interfaces.len() > limits.max_host_interfaces
        || host_interfaces
            .iter()
            .any(|item| item.is_empty() || item.len() > limits.max_generation_bytes)
    {
        return Err(CompilerProblem::InvalidGeneration);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedArtifact {
    content_id: ArtifactContentId,
    semantic_id: SemanticArtifactId,
    manifest: ArtifactManifest,
    payload: Vec<u8>,
}

/// An artifact whose binary IR and executable operation profile were verified.
///
/// Reading a version-2 manifest derives its missing dialect set from the legal
/// payload. The original bytes and source contract remain unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedArtifact {
    source_contract: &'static str,
    content_id: ArtifactContentId,
    manifest: ArtifactManifest,
    payload: Vec<u8>,
    legal: LegalModule,
}

impl ValidatedArtifact {
    /// Decode, canonicalize, and legalize a current or historical artifact.
    ///
    /// Version-2 input is migrated only in memory by deriving its executable
    /// dialect set from the verified module.
    pub fn read(
        manifest: VersionedArtifactManifest,
        payload: &[u8],
        catalog: &OperationCatalog,
        profile: &LegalityProfile,
        codec_limits: CodecLimits,
        limits: ArtifactLimits,
    ) -> Result<Self, CompilerProblem> {
        if payload.len() > limits.max_payload_bytes {
            return Err(CompilerProblem::ArtifactLimitExceeded);
        }
        let source_contract = manifest.contract();
        let ir_contract = match &manifest {
            VersionedArtifactManifest::V2(legacy) => {
                legacy.validate(limits)?;
                legacy.ir_contract.as_str()
            }
            VersionedArtifactManifest::V3(current) => {
                current.validate(limits)?;
                current.ir_contract.as_str()
            }
        };
        if ir_contract != mainframe_env_ir::IR_ENVELOPE_CONTRACT {
            return Err(CompilerProblem::InvalidGeneration);
        }
        let module = decode_binary(payload, codec_limits)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        let canonical = encode_binary(&module, codec_limits)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        if canonical != payload {
            return Err(CompilerProblem::Legality(
                "artifact payload is not canonically encoded".into(),
            ));
        }
        let legal = verify_legal(module, catalog, profile)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        let payload_dialects = dialect_contracts(&legal);
        let manifest = match manifest {
            VersionedArtifactManifest::V2(legacy) => legacy.migrate(payload_dialects),
            VersionedArtifactManifest::V3(current) => {
                if current.dialect_contracts != payload_dialects {
                    return Err(CompilerProblem::InvalidGeneration);
                }
                current
            }
        };
        manifest.validate(limits)?;
        Ok(Self {
            source_contract,
            content_id: ArtifactContentId(Sha256::digest(payload).into()),
            manifest,
            payload: payload.to_vec(),
            legal,
        })
    }

    #[must_use]
    /// Return the contract of the bytes supplied to the reader.
    pub const fn source_contract(&self) -> &'static str {
        self.source_contract
    }

    #[must_use]
    /// Return the SHA-256 identity of the unchanged executable payload.
    pub const fn content_id(&self) -> ArtifactContentId {
        self.content_id
    }

    #[must_use]
    /// Return the validated current-shape manifest.
    pub fn manifest(&self) -> &ArtifactManifest {
        &self.manifest
    }

    #[must_use]
    /// Return the original canonical executable bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    /// Return the legal IR proof produced while reading the artifact.
    pub fn legal(&self) -> &LegalModule {
        &self.legal
    }
}

impl PublishedArtifact {
    /// Encodes a consumed legal MIR stage and publishes its two typed identities.
    ///
    /// Arbitrary payload bytes are deliberately not accepted.
    ///
    /// ```compile_fail
    /// # use mainframe_env_compiler_api::{ArtifactLimits, ArtifactManifest, LegalizedMir, PublishedArtifact};
    /// # fn fabricate(mir: LegalizedMir, manifest: ArtifactManifest) {
    /// let _ = PublishedArtifact::publish(mir, manifest, b"unverified".to_vec(), ArtifactLimits::default());
    /// # }
    /// ```
    pub fn publish(
        mir: LegalizedMir,
        manifest: ArtifactManifest,
        codec_limits: CodecLimits,
        limits: ArtifactLimits,
    ) -> Result<Self, CompilerProblem> {
        manifest.validate(limits)?;
        if manifest.ir_contract != mainframe_env_ir::IR_ENVELOPE_CONTRACT {
            return Err(CompilerProblem::InvalidGeneration);
        }
        let payload_dialects = dialect_contracts(mir.legal());
        if manifest.dialect_contracts != payload_dialects {
            return Err(CompilerProblem::InvalidGeneration);
        }
        let payload = encode_binary(mir.legal().module(), codec_limits)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        if payload.len() > limits.max_payload_bytes {
            return Err(CompilerProblem::ArtifactLimitExceeded);
        }
        let payload_digest: [u8; 32] = Sha256::digest(&payload).into();
        let mut semantic = Sha256::new();
        field(&mut semantic, b"mainframe-env.semantic-artifact@3");
        field(&mut semantic, mir.source().as_bytes());
        field(&mut semantic, manifest.compiler_generation.as_bytes());
        field(&mut semantic, manifest.target.as_str().as_bytes());
        field(&mut semantic, manifest.ir_contract.as_bytes());
        for (key, value) in manifest.options.values() {
            field(&mut semantic, key.as_bytes());
            field(&mut semantic, value.as_bytes());
        }
        for interface in &manifest.host_interfaces {
            field(&mut semantic, interface.as_bytes());
        }
        for dialect in &manifest.dialect_contracts {
            field(&mut semantic, dialect.as_bytes());
        }
        let semantic_id = SemanticArtifactId(semantic.finalize().into());
        Ok(Self {
            content_id: ArtifactContentId(payload_digest),
            semantic_id,
            manifest,
            payload,
        })
    }

    #[must_use]
    pub const fn content_id(&self) -> ArtifactContentId {
        self.content_id
    }
    #[must_use]
    /// Return the current artifact contract emitted by the publisher.
    pub const fn contract(&self) -> &'static str {
        ARTIFACT_CONTRACT
    }
    #[must_use]
    pub const fn semantic_id(&self) -> SemanticArtifactId {
        self.semantic_id
    }
    #[must_use]
    pub fn manifest(&self) -> &ArtifactManifest {
        &self.manifest
    }
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn dialect_contracts(legal: &LegalModule) -> BTreeSet<String> {
    legal
        .report()
        .operations
        .iter()
        .map(|identity| format!("{}@{}", identity.namespace(), identity.major()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::tests::{
        CicsPlanFixture, DecimalPlanFixture, cics_boundary_fixture, decimal_boundary_fixture,
    };
    use mainframe_env_ir::{
        CodecLimits, DecimalPlanLimits, IrLimits, LegalityProfile, ModuleBuilder, OperationCatalog,
        OperationIdentity, OperationSchema,
    };
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn catalog_and_profile() -> (OperationCatalog, LegalityProfile) {
        let identity = OperationIdentity::new("test", "return", 1).unwrap();
        let mut schema = OperationSchema::pure(identity.clone(), 0, 0);
        schema.terminator = true;
        let mut catalog = OperationCatalog::default();
        catalog.register(schema).unwrap();
        let profile = LegalityProfile {
            allowed_operations: BTreeSet::from([identity]),
            allowed_runtime_imports: BTreeSet::new(),
        };
        (catalog, profile)
    }

    fn legalized() -> LegalizedMir {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "main.cbl",
            b"x".to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let source =
            SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new("test", "return", 1).unwrap();
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let module = builder.finish().unwrap();
        let (catalog, profile) = catalog_and_profile();
        let hir = crate::VerifiedHir::verify(source.id(), module.clone(), &catalog).unwrap();
        LegalizedMir::legalize(hir.lower(module), &catalog, &profile).unwrap()
    }

    #[test]
    fn semantic_and_content_identities_are_unambiguous() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test@1".into()]),
        };
        let first = PublishedArtifact::publish(
            legalized(),
            manifest.clone(),
            CodecLimits::default(),
            ArtifactLimits::default(),
        )
        .unwrap();
        let mut second_manifest = manifest;
        second_manifest.compiler_generation = "compiler-2".into();
        let second = PublishedArtifact::publish(
            legalized(),
            second_manifest,
            CodecLimits::default(),
            ArtifactLimits::default(),
        )
        .unwrap();
        assert_eq!(first.content_id(), second.content_id());
        assert_ne!(first.semantic_id(), second.semantic_id());
        assert_eq!(first.contract(), ARTIFACT_CONTRACT);
        assert_eq!(
            first.content_id().as_bytes(),
            &<[u8; 32]>::from(Sha256::digest(first.payload()))
        );
        assert!(first.content_id().to_reference().starts_with("sha256:"));
        assert!(
            first
                .semantic_id()
                .to_reference()
                .starts_with("semantic-sha256:")
        );
    }

    #[test]
    fn payload_is_bounded() {
        let limits = ArtifactLimits {
            max_payload_bytes: 1,
            ..ArtifactLimits::default()
        };
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test@1".into()]),
        };
        assert_eq!(
            PublishedArtifact::publish(legalized(), manifest, CodecLimits::default(), limits),
            Err(CompilerProblem::ArtifactLimitExceeded)
        );
    }

    #[test]
    fn manifest_must_name_the_payloads_exact_dialect_versions() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test@2".into()]),
        };
        assert_eq!(
            PublishedArtifact::publish(
                legalized(),
                manifest,
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::InvalidGeneration)
        );
    }

    #[test]
    fn malformed_dialect_contract_is_not_publishable() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test".into()]),
        };
        assert_eq!(
            PublishedArtifact::publish(
                legalized(),
                manifest,
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::InvalidGeneration)
        );
    }

    #[test]
    fn publisher_requires_the_binary_ir_envelope_contract() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_OBJECT_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test@1".into()]),
        };
        assert_eq!(
            PublishedArtifact::publish(
                legalized(),
                manifest,
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::InvalidGeneration)
        );
    }

    #[test]
    fn version_two_reader_derives_dialects_and_preserves_exact_payload_bytes() {
        let mir = legalized();
        let payload = encode_binary(mir.legal().module(), CodecLimits::default()).unwrap();
        let legacy = ArtifactManifestV2 {
            compiler_generation: "compiler-v2".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
        };
        let (catalog, profile) = catalog_and_profile();
        let artifact = ValidatedArtifact::read(
            VersionedArtifactManifest::V2(legacy),
            &payload,
            &catalog,
            &profile,
            CodecLimits::default(),
            ArtifactLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.source_contract(), LEGACY_ARTIFACT_CONTRACT);
        assert_eq!(artifact.payload(), payload.as_slice());
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from(["test@1".into()])
        );
        assert_eq!(artifact.legal().report().operation_count, 1);
        assert_eq!(
            artifact.content_id().as_bytes(),
            &<[u8; 32]>::from(Sha256::digest(&payload))
        );
    }

    #[test]
    fn artifact_reader_rejects_stale_v3_metadata_and_malformed_v2_payloads() {
        let mir = legalized();
        let payload = encode_binary(mir.legal().module(), CodecLimits::default()).unwrap();
        let current = ArtifactManifest {
            compiler_generation: "compiler-v3".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from(["test@2".into()]),
        };
        let legacy = ArtifactManifestV2 {
            compiler_generation: "compiler-v2".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
        };
        let (catalog, profile) = catalog_and_profile();
        assert_eq!(
            ValidatedArtifact::read(
                VersionedArtifactManifest::V3(current),
                &payload,
                &catalog,
                &profile,
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::InvalidGeneration)
        );
        assert!(matches!(
            ValidatedArtifact::read(
                VersionedArtifactManifest::V2(legacy.clone()),
                &payload,
                &catalog,
                &LegalityProfile::default(),
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::Legality(_))
        ));
        let truncated = &payload[..payload.len() - 1];
        assert!(matches!(
            ValidatedArtifact::read(
                VersionedArtifactManifest::V2(legacy),
                truncated,
                &catalog,
                &profile,
                CodecLimits::default(),
                ArtifactLimits::default(),
            ),
            Err(CompilerProblem::Legality(_))
        ));
    }

    #[test]
    fn artifact_reader_enforces_typed_cics_semantics_before_runtime_construction() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-v3".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            dialect_contracts: BTreeSet::from([
                "cics.file@1".into(),
                "mainframe.core.cobol@1".into(),
                "test@1".into(),
            ]),
        };
        let valid = cics_boundary_fixture(CicsPlanFixture::Valid);
        let payload = encode_binary(&valid.module, CodecLimits::default()).unwrap();
        let artifact = ValidatedArtifact::read(
            VersionedArtifactManifest::V3(manifest.clone()),
            &payload,
            &valid.catalog,
            &valid.profile,
            CodecLimits::default(),
            ArtifactLimits::default(),
        )
        .expect("canonical typed CICS artifact should be accepted");
        assert_eq!(artifact.legal().report().operation_count, 4);

        for kind in [
            CicsPlanFixture::Empty,
            CicsPlanFixture::WrongType,
            CicsPlanFixture::NonCanonical,
            CicsPlanFixture::OperationMismatch,
            CicsPlanFixture::MissingLayout,
            CicsPlanFixture::WrongLayoutExtent,
            CicsPlanFixture::ReadOnlyOutput,
        ] {
            let invalid = cics_boundary_fixture(kind);
            let payload = encode_binary(&invalid.module, CodecLimits::default()).unwrap();
            let decoded = decode_binary(&payload, CodecLimits::default()).unwrap();
            assert_eq!(
                encode_binary(&decoded, CodecLimits::default()).unwrap(),
                payload,
                "{kind:?} fixture must retain a canonical outer IR envelope"
            );
            assert!(
                matches!(
                    ValidatedArtifact::read(
                        VersionedArtifactManifest::V3(manifest.clone()),
                        &payload,
                        &invalid.catalog,
                        &invalid.profile,
                        CodecLimits::default(),
                        ArtifactLimits::default(),
                    ),
                    Err(CompilerProblem::Legality(detail)) if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected by the artifact legal boundary"
            );
        }
    }

    #[test]
    fn artifact_reader_enforces_decimal_plan_layout_and_topology_semantics() {
        let legacy_manifest = || {
            VersionedArtifactManifest::V2(ArtifactManifestV2 {
                compiler_generation: "compiler-v2".into(),
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
                host_interfaces: BTreeSet::new(),
                ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
            })
        };
        for kind in [
            DecimalPlanFixture::Valid,
            DecimalPlanFixture::ValidCondition,
        ] {
            let valid = decimal_boundary_fixture(kind);
            let payload = encode_binary(&valid.module, CodecLimits::default()).unwrap();
            ValidatedArtifact::read(
                legacy_manifest(),
                &payload,
                &valid.catalog,
                &valid.profile,
                CodecLimits::default(),
                ArtifactLimits::default(),
            )
            .expect("canonical typed decimal artifact should be accepted");
        }

        for kind in [
            DecimalPlanFixture::EmptyBytes,
            DecimalPlanFixture::WrongType,
            DecimalPlanFixture::Truncated,
            DecimalPlanFixture::NonCanonical,
            DecimalPlanFixture::WrongVersion,
            DecimalPlanFixture::OperationMajorMismatch,
            DecimalPlanFixture::EffectMismatch,
            DecimalPlanFixture::SlotMismatch,
            DecimalPlanFixture::MissingLayout,
            DecimalPlanFixture::NonNumericLayout,
            DecimalPlanFixture::WrongLayoutExtent,
            DecimalPlanFixture::MissingConditionOwner,
            DecimalPlanFixture::OrphanConditionBranch,
            DecimalPlanFixture::InvalidFalseTarget,
            DecimalPlanFixture::UnexpectedTrueEdge,
            DecimalPlanFixture::Oversized,
        ] {
            let invalid = decimal_boundary_fixture(kind);
            let mut codec = CodecLimits::default();
            if matches!(kind, DecimalPlanFixture::Oversized) {
                codec.ir.max_attribute_bytes = DecimalPlanLimits::default().max_encoded_bytes + 1;
            }
            let payload = encode_binary(&invalid.module, codec).unwrap();
            assert!(
                matches!(
                    ValidatedArtifact::read(
                        legacy_manifest(),
                        &payload,
                        &invalid.catalog,
                        &invalid.profile,
                        codec,
                        ArtifactLimits::default(),
                    ),
                    Err(CompilerProblem::Legality(detail)) if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected by the artifact legal boundary"
            );
        }
    }
}
