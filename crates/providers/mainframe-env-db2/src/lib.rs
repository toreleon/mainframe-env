//! Bounded durable static-Db2 authority and host-provider adapters.

#![forbid(unsafe_code)]

mod abi;
mod catalog;
mod generated_statement_catalog;
mod retention;
mod service;
mod syntax;

pub use abi::db2_abi_library;

pub use catalog::{
    DB2_APPLICATION_CATALOG_CONTRACT, Db2CatalogGeneration, Db2ColumnDefinition, Db2ExtractField,
    Db2ExtractLayout, Db2ForeignKeyDefinition, Db2ResultEncoding, Db2SeedRow, Db2TableDefinition,
    decode_table_definitions_bounded,
};
pub use generated_statement_catalog::{
    DB2_OFFICIAL_STATEMENT_CATALOG_SHA256, DB2_STATEMENT_DESCRIPTORS, Db2StatementDescriptor,
    Db2StatementId, Db2StatementUnit, db2_statement_descriptor, db2_statement_descriptor_by_row,
};
pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, Db2ReplayDependency,
    Db2ReplayOwnerKind, Db2ReplayRetentionDescriptor, Db2ReplayRetentionError,
    Db2ReplayRetentionState, describe_db2_replay_row,
};
pub use service::{Db2Limits, Db2ReplayClock, Db2Service, db2_providers};
pub use syntax::{
    Db2LexedStatement, Db2SourceLocation, Db2SourceSpan, Db2StringKind, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenCursor,
    Db2TokenKind, lex_db2,
};
