//! Bounded deterministic in-memory implementations of owned store contracts.

#![forbid(unsafe_code)]

mod checked_read;
mod durable;
mod durable_retention;
mod local_artifact;
mod memory;
mod postgres;
mod postgres_artifact;
mod publication;
mod retention;
mod root_terminal;
mod runtime;
mod sqlite;
mod validation;

pub use local_artifact::LocalArtifactStore;
pub use memory::{MemoryStore, StoreLimits};
pub use postgres::PostgresStateStore;
pub use postgres_artifact::PostgresArtifactStore;
pub use sqlite::SqliteStateStore;

pub const SQLITE_MIGRATION_HEAD: &str = "0002-retention-lifecycle";
pub const POSTGRES_MIGRATION_HEAD: &str = "0003-executable-artifact-metadata";
