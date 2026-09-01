//! Exact, bounded, deterministic source inputs and provenance.

#![forbid(unsafe_code)]

mod abi;
mod bundle;
mod identity;

pub use abi::{
    HOST_ABI_SOURCE_LIBRARY_CONTRACT, HOST_ABI_SOURCE_LICENSE, HOST_ABI_SOURCE_ORIGIN,
    HostAbiLibraryDefinition, HostAbiMember, HostAbiProblem, HostAbiSubsystem,
    MaterializedHostAbiLibraries, materialize_host_abi_libraries,
};

pub use bundle::{
    LibraryProblem, ProvenanceEdge, ProvenanceEdgeInput, ProvenanceKind, SourceBundle,
    SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits, SourceRange,
};
pub use identity::{FileId, LogicalPath, SourceId, SourceProblem};

/// Stable source-bundle contract identity.
pub const SOURCE_CONTRACT: &str = "mainframe-env.source@1";

/// Additive ordered-library identity used only by explicit source closures.
pub const SOURCE_LIBRARY_CONTRACT: &str = "mainframe-env.source-libraries@1";
