//! Owned compiler stage, request, result, and artifact contracts.

#![forbid(unsafe_code)]

mod artifact;
mod service;
mod stage;

pub use artifact::{
    ArtifactContentId, ArtifactLimits, ArtifactManifest, ArtifactManifestV2, PublishedArtifact,
    SemanticArtifactId, ValidatedArtifact, VersionedArtifactManifest,
};
pub use service::{
    CompilationMode, CompileOptions, CompileTarget, CompilerProblem, CompilerRequest,
    CompilerResult, CompilerService,
};
pub use stage::{LegalizedMir, LoweredMir, VerifiedHir};

/// Stable identifier for the compiler request and stage contract.
pub const COMPILER_CONTRACT: &str = "mainframe-env.compiler@1";
/// Historical artifact contract whose manifest did not declare dialects.
pub const LEGACY_ARTIFACT_CONTRACT: &str = "mainframe-env.artifact@2";
/// Current semantic/content artifact contract with exact dialect requirements.
pub const ARTIFACT_CONTRACT: &str = "mainframe-env.artifact@3";
