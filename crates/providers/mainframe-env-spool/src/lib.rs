//! Durable JES spool metadata and artifact-backed payload authority.

#![forbid(unsafe_code)]

mod service;

pub use service::{
    ProviderArtifactStore, SPOOL_STATE_CONTRACT, SpoolLimits, SpoolService, spool_providers,
};
