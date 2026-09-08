//! Single-node mainframe-env product composition.

#![forbid(unsafe_code)]

mod cobol;
mod config;
mod environment_secrets;
mod jes_worker;
mod product;
#[cfg(test)]
mod recovery_tests;

pub use cobol::{
    DefaultProgramRouter, ProgramExecutionControl, compatible_system_services,
    default_program_router,
};
pub use config::{ArtifactProfile, ConfigOverrides, ServerConfig, StoreProfile, TlsConfig};
pub use environment_secrets::EnvironmentSecretResolver;
pub use product::{
    ApplicationPublicationReceipt, BatchInstallReceipt, BatchProgramDefinition,
    HmacSha256PackageTrust, OnlineApplicationDefinition, OnlineInstallReceipt,
    OnlineProgramDefinition, ProductMetrics, ProductServer,
};

pub const SERVER_GENERATION: &str = "mainframe-env-server@1";
