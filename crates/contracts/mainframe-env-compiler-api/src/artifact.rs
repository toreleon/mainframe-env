use crate::{CompileOptions, CompileTarget, CompilerProblem, LegalizedMir};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactId([u8; 32]);

impl ArtifactId {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Debug for ArtifactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ArtifactId")
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
    id: ArtifactId,
    payload_digest: [u8; 32],
    manifest: ArtifactManifest,
    payload: Vec<u8>,
}

impl PublishedArtifact {
    pub fn publish(
        mir: &LegalizedMir,
        manifest: ArtifactManifest,
        payload: Vec<u8>,
        limits: ArtifactLimits,
    ) -> Result<Self, CompilerProblem> {
        manifest.validate(limits)?;
        if payload.len() > limits.max_payload_bytes {
            return Err(CompilerProblem::ArtifactLimitExceeded);
        }
        let payload_digest: [u8; 32] = Sha256::digest(&payload).into();
        let mut semantic = Sha256::new();
        field(&mut semantic, b"mainframe-env.artifact@1");
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
        let id = ArtifactId(semantic.finalize().into());
        Ok(Self {
            id,
            payload_digest,
            manifest,
            payload,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ArtifactId {
        self.id
    }
    #[must_use]
    pub const fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
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
    use crate::{ParsedProgram, SemanticProgram};
    use mainframe_env_diagnostics::Completeness;
    use mainframe_env_ir::{
        IrLimits, LegalityProfile, ModuleBuilder, OperationCatalog, OperationIdentity,
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
        let parsed =
            ParsedProgram::validated(source.id(), Vec::new(), Completeness::Complete, 4).unwrap();
        let semantic =
            SemanticProgram::validated(parsed, [1; 32], Vec::new(), Completeness::Complete, 4)
                .unwrap();
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
        let _hir = crate::VerifiedHir::verify(&semantic, module.clone(), &catalog).unwrap();
        LegalizedMir::legalize(
            source.id(),
            module,
            &catalog,
            &LegalityProfile {
                allowed_operations: BTreeSet::from([identity]),
                allowed_runtime_imports: BTreeSet::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn semantic_identity_is_independent_of_payload_codec() {
        let mir = legalized();
        let manifest = ArtifactManifest {
            compiler_generation: "compiler-1".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::new(),
            ir_contract: "mainframe-env.ir@1".into(),
        };
        let first = PublishedArtifact::publish(
            &mir,
            manifest.clone(),
            b"one".to_vec(),
            ArtifactLimits::default(),
        )
        .unwrap();
        let second =
            PublishedArtifact::publish(&mir, manifest, b"two".to_vec(), ArtifactLimits::default())
                .unwrap();
        assert_eq!(first.id(), second.id());
        assert_ne!(first.payload_digest(), second.payload_digest());
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
            PublishedArtifact::publish(&legalized(), manifest, vec![1, 2], limits),
            Err(CompilerProblem::ArtifactLimitExceeded)
        );
    }
}
