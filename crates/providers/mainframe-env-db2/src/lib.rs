//! Bounded durable static-Db2 authority and host-provider adapters.

#![forbid(unsafe_code)]

mod abi;
mod arithmetic_types;
mod ast;
mod catalog;
mod create_index_syntax;
mod create_view_syntax;
mod cursor_operation_syntax;
mod delete_syntax;
mod drop_syntax;
mod expression_parser;
mod generated_statement_catalog;
mod insert_syntax;
mod name_resolution;
mod numeric_constant_types;
mod numeric_constant_values;
mod rename_syntax;
mod result_combination_types;
mod retention;
mod service;
mod statement;
mod syntax;
mod type_system;
mod update_syntax;

pub use abi::db2_abi_library;
pub use arithmetic_types::{
    Db2ArithmeticConstantError, Db2ArithmeticConstantLimits, Db2ArithmeticContext,
    Db2ArithmeticError, Db2ArithmeticErrorCode, Db2ArithmeticObligations, Db2ArithmeticOperand,
    Db2ArithmeticRangeCheck, Db2ArithmeticResult, Db2ArithmeticSide, Db2ArithmeticSignRule,
    Db2ArithmeticZeroCheck, Db2DecimalArithmetic, Db2DecimalMultiplyCheck, Db2DecimalTruncation,
    classify_db2_arithmetic_constants, resolve_db2_binary_arithmetic, resolve_db2_unary_arithmetic,
};
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
pub use create_index_syntax::{
    Db2CreateIndexKey, Db2CreateIndexStatement, Db2IndexKeyOrder, Db2IndexUniqueness,
    parse_db2_create_index_statement,
};
pub use create_view_syntax::{
    Db2CreateViewStatement, Db2ViewCheckMode, Db2ViewExpression, Db2ViewLocated, Db2ViewOrderKey,
    Db2ViewSelectCore, parse_db2_create_view_statement,
};
pub use cursor_operation_syntax::{
    Db2CursorHostOperands, Db2CursorHostReference, Db2CursorOperationKind,
    Db2CursorOperationStatement, Db2FetchOrientation, parse_db2_cursor_operation_statement,
};
pub use delete_syntax::{
    Db2DeleteLocated, Db2DeleteSearchCondition, Db2SearchedDeleteStatement,
    parse_db2_searched_delete,
};
pub use drop_syntax::{
    Db2DropAliasDesignator, Db2DropObjectKind, Db2DropObjectName, Db2DropStatement,
    parse_db2_drop_statement,
};
pub use expression_parser::{Db2ParsedExpression, parse_db2_expression};
pub use generated_statement_catalog::{
    DB2_OFFICIAL_STATEMENT_CATALOG_SHA256, DB2_STATEMENT_DESCRIPTORS, Db2StatementDescriptor,
    Db2StatementId, Db2StatementUnit, db2_statement_descriptor, db2_statement_descriptor_by_row,
};
pub use insert_syntax::{
    Db2InsertExpression, Db2InsertValue, Db2InsertValuesRow, Db2InsertValuesStatement,
    parse_db2_insert_values,
};
pub use name_resolution::{
    Db2DynamicQualificationContext, Db2QualificationCandidate, Db2QualificationContext,
    Db2QualificationError, Db2QualificationErrorCode, Db2QualificationObject,
    Db2QualificationOrigin, Db2QualificationRequest, Db2QualificationStatus, Db2QualificationUse,
    Db2SynonymCheck, qualify_db2_name,
};
pub use numeric_constant_types::{
    Db2NumericConstantError, Db2NumericConstantErrorCode, Db2NumericConstantLimits,
    Db2NumericConstantType, classify_db2_located_numeric_operand, classify_db2_numeric_constant,
};
pub use numeric_constant_values::{
    Db2AssignedNumericConstant, Db2AssignedNumericValue, Db2MaterializedNumericConstant,
    Db2NumericAssignmentConversion, Db2NumericAssignmentError, Db2NumericAssignmentErrorCode,
    Db2NumericConstantValue, Db2NumericConstantValueError, Db2NumericConstantValueErrorCode,
    assign_db2_numeric_constant, materialize_db2_located_numeric_operand,
    materialize_db2_numeric_constant,
};
pub use rename_syntax::{Db2RenameObjectKind, Db2RenameStatement, parse_db2_rename_statement};
pub use result_combination_types::{
    Db2CaseElse, Db2CombinedResultOperand, Db2CombinedResultType, Db2ResultCombinationContext,
    Db2ResultCombinationError, Db2ResultCombinationErrorCode, Db2ResultCombinationLimits,
    Db2ResultCombinationStep, Db2ResultConversionObligation, Db2ResultTypeOperand,
    combine_db2_result_types,
};
pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, Db2ReplayDependency,
    Db2ReplayOwnerKind, Db2ReplayRetentionDescriptor, Db2ReplayRetentionError,
    Db2ReplayRetentionState, describe_db2_replay_row,
};
pub use service::{Db2Limits, Db2ReplayClock, Db2Service, db2_providers};
pub use statement::{
    Db2ColumnDefault, Db2CommitStatement, Db2CreateTableColumn, Db2CreateTableConstraint,
    Db2CreateTableStatement, Db2CursorHoldability, Db2CursorOrientation, Db2CursorReturnTarget,
    Db2CursorReturnability, Db2CursorRowsetPositioning, Db2CursorSensitivity,
    Db2DeclareCursorPreparedStatement, Db2DefaultSpelling, Db2DescriptorNameMode,
    Db2ExecuteImmediateStatement, Db2ExecuteStatement, Db2ExecuteUsing, Db2FetchClause,
    Db2FetchPosition, Db2ForeignKeyConstraint, Db2NamedTableSource, Db2OffsetClause,
    Db2OnDeleteAction, Db2OrderByItem, Db2OrderDirection, Db2OrderKey, Db2PrepareDescriptor,
    Db2PrepareStatement, Db2QueryExpression, Db2RollbackStatement, Db2RollbackTarget,
    Db2SavepointStatement, Db2SelectCore, Db2SelectItem, Db2SelectQuantifier,
    Db2SensitiveCursorKind, Db2Statement, Db2StatementKind, Db2TableConstraintKind,
    parse_db2_create_table_statement, parse_db2_cursor_statement,
    parse_db2_declare_cursor_prepared, parse_db2_dynamic_statement, parse_db2_select_core,
    parse_db2_transaction_statement,
};
pub use syntax::{
    Db2LexedStatement, Db2SourceLocation, Db2SourceSpan, Db2StringKind, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenCursor,
    Db2TokenKind, lex_db2, parse_db2_host_reference,
};
pub use type_system::string_constants::storage_assignment::{
    Db2StoredStringConstant, Db2StringStorageContext, Db2StringStorageError,
    Db2StringStorageErrorCode, Db2StringStorageLimits, store_db2_string_constant,
};
pub use type_system::string_constants::{
    Db2MixedData, Db2StringConstant, Db2StringConstantContext, Db2StringConstantEncoding,
    Db2StringConstantError, Db2StringConstantErrorCode, Db2StringConstantLimits,
    Db2StringConstantValue, materialize_db2_string_constant,
};
pub use type_system::truth::Db2TruthValue;
pub use type_system::{
    Db2AssignmentCompatibility, Db2AssignmentContext, Db2AssignmentNullability,
    Db2ComparisonCompatibility, Db2ComparisonContext, Db2ConversionKind, Db2Nullability,
    Db2ResolvedType, Db2ScalarType, Db2TimeZone, Db2TypeAttributes, Db2TypeError, Db2TypeErrorCode,
    classify_db2_assignment, classify_db2_comparison, resolve_db2_type,
    resolve_db2_type_with_attributes,
};
pub use update_syntax::{
    Db2UpdateAssignment, Db2UpdateExpression, Db2UpdateStatement, Db2UpdateValue,
    parse_db2_searched_update,
};
