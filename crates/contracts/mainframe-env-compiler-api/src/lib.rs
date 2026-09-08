//! Owned compiler stage, request, result, and artifact contracts.

#![forbid(unsafe_code)]

mod artifact;
mod service;
mod stage;

pub use artifact::{
    ArtifactContentId, ArtifactLimits, ArtifactManifest, PublishedArtifact, SemanticArtifactId,
};
pub use service::{
    CompilationMode, CompileOptions, CompileTarget, CompilerProblem, CompilerRequest,
    CompilerResult, CompilerService,
};
pub use stage::{LegalizedMir, LoweredMir, VerifiedHir};

pub const COMPILER_CONTRACT: &str = "mainframe-env.compiler@1";
pub const ARTIFACT_CONTRACT: &str = "mainframe-env.artifact@2";
