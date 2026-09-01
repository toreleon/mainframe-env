//! Bounded durable static-Db2 authority and host-provider adapters.

#![forbid(unsafe_code)]

mod abi;
mod catalog;
mod service;

pub use abi::db2_abi_library;

pub use catalog::{
    DB2_APPLICATION_CATALOG_CONTRACT, Db2CatalogGeneration, Db2ColumnDefinition, Db2ExtractField,
    Db2ExtractLayout, Db2ForeignKeyDefinition, Db2ResultEncoding, Db2SeedRow, Db2TableDefinition,
    decode_table_definitions_bounded,
};
pub use service::{Db2Limits, Db2Service, db2_providers};
