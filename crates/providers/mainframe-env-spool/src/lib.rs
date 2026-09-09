//! Durable JES spool metadata and artifact-backed payload authority.

#![forbid(unsafe_code)]

mod retention;
mod service;

pub use retention::{
    SPOOL_STATE_CONTRACT, SpoolRetentionDescriptor, SpoolRetentionState,
    SpoolRetentionValidationError, SpoolRowCodecVersion, SpoolRowState,
    describe_spool_retention_row,
};
pub use service::{
    ProviderArtifactStore, SpoolLimits, SpoolRetentionClock, SpoolService, spool_providers,
};
