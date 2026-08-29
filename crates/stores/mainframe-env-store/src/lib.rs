//! Bounded deterministic in-memory implementations of owned store contracts.

#![forbid(unsafe_code)]

mod memory;

pub use memory::{MemoryStore, StoreLimits};
