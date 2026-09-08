//! Bounded deterministic in-memory implementations of owned store contracts.

#![forbid(unsafe_code)]

mod durable;
mod local_artifact;
mod memory;
mod postgres;
mod postgres_artifact;
mod runtime;
mod sqlite;
mod validation;

pub use local_artifact::LocalArtifactStore;
pub use memory::{MemoryStore, StoreLimits};
pub use postgres::PostgresStateStore;
pub use postgres_artifact::PostgresArtifactStore;
pub use sqlite::SqliteStateStore;

pub const SQL_MIGRATION_HEAD: &str = "0001-durable-state";
