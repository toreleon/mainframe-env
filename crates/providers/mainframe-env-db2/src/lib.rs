//! Bounded durable static-Db2 authority and host-provider adapters.

#![forbid(unsafe_code)]

mod service;

pub use service::{Db2Limits, Db2Service, db2_providers};
