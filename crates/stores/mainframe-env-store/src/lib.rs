//! Bounded deterministic in-memory implementations of owned store contracts.

#![forbid(unsafe_code)]

mod memory;
mod sqlite;

pub use memory::{MemoryStore, StoreLimits};
pub use sqlite::SqliteStateStore;
