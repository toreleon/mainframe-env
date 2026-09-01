//! Bounded dataset/catalog authority and host-provider adapters.

#![forbid(unsafe_code)]

mod codec;
mod dependency;
mod service;

pub use service::{
    DatasetLimits, DatasetSeedObject, DatasetService, SeedInstallReceipt, dataset_providers,
};
