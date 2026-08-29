//! Exact, bounded, deterministic source inputs and provenance.

#![forbid(unsafe_code)]

mod bundle;
mod identity;

pub use bundle::{
    LibraryProblem, ProvenanceEdge, ProvenanceEdgeInput, ProvenanceKind, SourceBundle,
    SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits, SourceRange,
};
pub use identity::{FileId, LogicalPath, SourceId, SourceProblem};

/// Stable source-bundle contract identity.
pub const SOURCE_CONTRACT: &str = "mainframe-env.source@1";

/// Additive ordered-library identity used only by explicit source closures.
pub const SOURCE_LIBRARY_CONTRACT: &str = "mainframe-env.source-libraries@1";
