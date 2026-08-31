//! Single-node mainframe-env product composition.

#![forbid(unsafe_code)]

mod cobol;
mod config;
mod product;

pub use cobol::{DefaultProgramRouter, compatible_system_services, default_program_router};
pub use config::{ConfigOverrides, ServerConfig, StoreProfile, TlsConfig};
pub use product::{
    BatchInstallReceipt, BatchProgramDefinition, HmacSha256PackageTrust,
    OnlineApplicationDefinition, OnlineInstallReceipt, OnlineProgramDefinition, ProductMetrics,
    ProductServer,
};

pub const SERVER_GENERATION: &str = "mainframe-env-server@1";
