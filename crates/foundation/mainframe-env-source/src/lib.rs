//! Exact, bounded, deterministic source inputs and provenance.

#![forbid(unsafe_code)]

mod bundle;
mod identity;

pub use bundle::{
    ProvenanceEdge, ProvenanceEdgeInput, ProvenanceKind, SourceBundle, SourceEncoding, SourceFile,
    SourceFormat, SourceLimits, SourceRange,
};
pub use identity::{FileId, LogicalPath, SourceId, SourceProblem};

/// Stable source-bundle contract identity.
pub const SOURCE_CONTRACT: &str = "mainframe-env.source@1";
