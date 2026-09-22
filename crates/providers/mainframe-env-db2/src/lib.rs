//! Bounded durable static-Db2 authority and host-provider adapters.

#![forbid(unsafe_code)]

mod abi;
mod ast;
mod catalog;
mod create_table_syntax;
mod cursor_syntax;
mod expression_parser;
mod generated_statement_catalog;
mod query_syntax;
mod retention;
mod service;
mod statement;
mod syntax;
mod type_system;

pub use abi::db2_abi_library;
pub use ast::{
    Db2AstError, Db2AstErrorCode, Db2AstLimits, Db2BinaryOperator, Db2BuiltInDataType,
    Db2BuiltInType, Db2DataType, Db2Expression, Db2ExpressionArena, Db2ExpressionId,
    Db2ExpressionKind, Db2HostIdentifier, Db2HostReference, Db2Identifier, Db2Literal,
    Db2QualifiedName, Db2UnaryOperator,
};

pub use catalog::{
    DB2_APPLICATION_CATALOG_CONTRACT, Db2CatalogGeneration, Db2ColumnDefinition, Db2ExtractField,
    Db2ExtractLayout, Db2ForeignKeyDefinition, Db2ResultEncoding, Db2SeedRow, Db2TableDefinition,
    decode_table_definitions_bounded,
};
pub use create_table_syntax::{
    Db2ColumnDefault, Db2CreateTableColumn, Db2CreateTableConstraint, Db2CreateTableStatement,
    Db2DefaultSpelling, Db2ForeignKeyConstraint, Db2OnDeleteAction, Db2TableConstraintKind,
    parse_db2_create_table_statement,
};
pub use cursor_syntax::{
    Db2CursorHoldability, Db2CursorOrientation, Db2CursorReturnTarget, Db2CursorReturnability,
    Db2CursorRowsetPositioning, Db2CursorSensitivity, Db2DeclareCursorPreparedStatement,
    Db2SensitiveCursorKind, parse_db2_declare_cursor_prepared,
};
pub use expression_parser::{Db2ParsedExpression, parse_db2_expression};
pub use generated_statement_catalog::{
    DB2_OFFICIAL_STATEMENT_CATALOG_SHA256, DB2_STATEMENT_DESCRIPTORS, Db2StatementDescriptor,
    Db2StatementId, Db2StatementUnit, db2_statement_descriptor, db2_statement_descriptor_by_row,
};
pub use query_syntax::{
    Db2FetchClause, Db2FetchPosition, Db2NamedTableSource, Db2OffsetClause, Db2OrderByItem,
    Db2OrderDirection, Db2OrderKey, Db2QueryExpression, Db2SelectCore, Db2SelectItem,
    Db2SelectQuantifier, parse_db2_select_core,
};
pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, Db2ReplayDependency,
    Db2ReplayOwnerKind, Db2ReplayRetentionDescriptor, Db2ReplayRetentionError,
    Db2ReplayRetentionState, describe_db2_replay_row,
};
pub use service::{Db2Limits, Db2ReplayClock, Db2Service, db2_providers};
pub use statement::{
    Db2CommitStatement, Db2DescriptorNameMode, Db2ExecuteImmediateStatement, Db2ExecuteStatement,
    Db2ExecuteUsing, Db2PrepareDescriptor, Db2PrepareStatement, Db2RollbackStatement,
    Db2RollbackTarget, Db2SavepointStatement, Db2Statement, Db2StatementKind,
    parse_db2_dynamic_statement, parse_db2_transaction_statement,
};
pub use syntax::{
    Db2LexedStatement, Db2SourceLocation, Db2SourceSpan, Db2StringKind, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2,
};
