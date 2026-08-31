//! Fail-closed RACF/SAF authority and host-provider adapters.

#![forbid(unsafe_code)]

mod service;

pub use service::{
    MemorySecretResolver, RacfInstallReceipt, RacfLimits, RacfManifest, RacfProfileDefinition,
    RacfService, RacfUserDefinition, ResolvedSecret, SecretResolver, racf_providers,
};
