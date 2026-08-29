//! Bounded dataset/catalog authority and host-provider adapters.

#![forbid(unsafe_code)]

mod codec;
mod service;

pub use service::{DatasetLimits, DatasetService, dataset_providers};
