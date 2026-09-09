use crate::{CompileOptions, CompileTarget, CompilerProblem, LegalizedMir};
use mainframe_env_ir::{CodecLimits, encode_binary};
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
}

impl ArtifactManifest {
    pub fn validate(&self, limits: ArtifactLimits) -> Result<(), CompilerProblem> {
        if self.compiler_generation.is_empty()
            || self.compiler_generation.len() > limits.max_generation_bytes
            || self.ir_contract.is_empty()
            || self.ir_contract.len() > limits.max_generation_bytes
            || self.host_interfaces.len() > limits.max_host_interfaces
            || self
                .host_interfaces
                .iter()
                .any(|item| item.is_empty() || item.len() > limits.max_generation_bytes)
        {
            return Err(CompilerProblem::InvalidGeneration);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedArtifact {
    content_id: ArtifactContentId,
    semantic_id: SemanticArtifactId,
    manifest: ArtifactManifest,
    payload: Vec<u8>,
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
        let payload = encode_binary(mir.legal().module(), codec_limits)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        if payload.len() > limits.max_payload_bytes {
            return Err(CompilerProblem::ArtifactLimitExceeded);
        }
        let payload_digest: [u8; 32] = Sha256::digest(&payload).into();
        let mut semantic = Sha256::new();
        field(&mut semantic, b"mainframe-env.semantic-artifact@2");
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

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_ir::{
        CodecLimits, IrLimits, LegalityProfile, ModuleBuilder, OperationCatalog, OperationIdentity,
        OperationSchema,
    };
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use std::collections::{BTreeMap, BTreeSet};

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
        let mut schema = OperationSchema::pure(identity.clone(), 0, 0);
        schema.terminator = true;
        let mut catalog = OperationCatalog::default();
        catalog.register(schema).unwrap();
        let hir = crate::VerifiedHir::verify(source.id(), module.clone(), &catalog).unwrap();
        LegalizedMir::legalize(
            hir.lower(module),
            &catalog,
            &LegalityProfile {
                allowed_operations: BTreeSet::from([identity]),
                allowed_runtime_imports: BTreeSet::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn semantic_and_content_identities_are_unambiguous() {
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: "mainframe-env.ir@1".into(),
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
            ir_contract: "mainframe-env.ir@1".into(),
        };
        assert_eq!(
            PublishedArtifact::publish(legalized(), manifest, CodecLimits::default(), limits),
            Err(CompilerProblem::ArtifactLimitExceeded)
        );
    }
}
