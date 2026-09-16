//! Single-node mainframe-env product composition.

#![forbid(unsafe_code)]

mod cobol;
mod config;
#[allow(dead_code, reason = "R-11 product integration seam")]
mod console_retention;
mod environment_secrets;
mod jes_admission;
mod jes_worker;
mod product;
#[cfg(test)]
mod recovery_tests;
mod retention_maintenance;

pub use cobol::{
    DefaultProgramRouter, ProgramExecutionControl, compatible_system_services,
    default_program_router,
};
pub use config::{
    ArtifactProfile, BootstrapConfig, ConfigOverrides, RetentionConfig, ServerConfig, StoreProfile,
    TlsConfig,
};
pub use environment_secrets::EnvironmentSecretResolver;
pub use product::{
    ApplicationPublicationReceipt, BatchInstallReceipt, BatchProgramDefinition,
    HmacSha256PackageTrust, OnlineApplicationDefinition, OnlineInstallReceipt,
    OnlineProgramDefinition, ProductCapacityStatus, ProductMetrics, ProductReadiness,
    ProductServer,
};
pub use retention_maintenance::{RetentionMaintenance, RetentionMaintenancePass};

pub const SERVER_GENERATION: &str = "mainframe-env-server@1";
