//! Column-only default admission from one bounded original CREATE TABLE source.
//!
//! SQL0050, ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0:
//! sqlref/src/tpc/db2z_sql_createtable.html, 874327 bytes,
//! SHA-256 104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6,
//! DEFAULT/table rules at plain-text lines 400..505. Line 444's WITH DEFAULT
//! empty-string description is unresolved against the generic system-default
//! rules; every such spelling remains source-pending here, not invalid Db2.
//!
//! Constant assignment delegates to the existing numeric/string proof owners
//! and their pinned constants, numeric assignment, string assignment and type
//! sources. Character/X defaults have a decoded UTF-8 ceiling of 1536 bytes
//! before storage trimming or padding; BX remains binary. Special registers,
//! distinct casts, graphic constants and other ordinary conversions stay pending.
//!
//! The statement is parsed once here, so its components and assigned proofs
//! belong to the retained original source. Only column types/defaults are bound:
//! names, constraints, installed catalog, authorization and execution are not.
//! System producers are plans for insertion/update/LOAD, never materialized SQL
//! scalars/cells or literal proofs. A runtime value executor is still required.
//! No clock, durable/schema identity, SQLCA or official row credit is introduced.

use super::{Db2CreateTableStatement, Db2DefaultSpelling, parse_db2_create_table_statement};
use crate::{
    Db2AssignedNumericConstant, Db2AstLimits, Db2Literal, Db2Nullability,
    Db2NumericAssignmentErrorCode, Db2NumericConstantLimits, Db2NumericConstantValueErrorCode,
    Db2ResolvedType, Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2StoredStringConstant,
    Db2StringConstantContext, Db2StringConstantErrorCode, Db2StringConstantLimits,
    Db2StringConstantValue, Db2StringStorageContext, Db2StringStorageErrorCode,
    Db2StringStorageLimits, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2TimeZone,
    Db2TypeErrorCode, assign_db2_numeric_constant, materialize_db2_located_numeric_operand,
    materialize_db2_string_constant, resolve_db2_type, store_db2_string_constant,
};
use std::fmt;

/// All budgets retain their existing owners, including both numeric original
/// source ceilings and original string spelling/span budgets. Selected operand
/// lexing is independent of the complete statement's token budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ColumnDefaultBindingLimits {
    pub syntax: Db2SyntaxLimits,
    pub ast: Db2AstLimits,
    pub numeric: Db2NumericConstantLimits,
    pub numeric_syntax: Db2SyntaxLimits,
    pub string: Db2StringConstantLimits,
    pub storage: Db2StringStorageLimits,
}

impl Default for Db2ColumnDefaultBindingLimits {
    fn default() -> Self {
        Self {
            syntax: Db2SyntaxLimits::default(),
            ast: Db2AstLimits::default(),
            numeric: Db2NumericConstantLimits::default(),
            numeric_syntax: Db2SyntaxLimits::default(),
            string: Db2StringConstantLimits::default(),
            storage: Db2StringStorageLimits {
                max_output_bytes: 32_704,
            },
        }
    }
}

/// Caller-selected source and target contexts. No host/catalog encoding is
/// inferred. Target families select the corresponding explicit context; the
/// storage owner decides compatibility and whether conversion is implemented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ColumnDefaultStringContexts {
    pub source: Db2StringConstantContext,
    pub character_target: Db2StringStorageContext,
    pub binary_target: Db2StringStorageContext,
}

/// Closed symbolic nonnull system producers. Lengths are declared target units;
/// graphic blanks are deliberately not fabricated encoded bytes. Numeric zero
/// includes resolved floating/DECFLOAT families without claiming their values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SystemDefaultProducer {
    NumericZero,
    FixedCharacterBlanks {
        length: u32,
    },
    FixedGraphicBlanks {
        length: u32,
    },
    FixedBinaryZeros {
        length: u32,
    },
    VaryingEmpty,
    CurrentDate,
    CurrentTime,
    CurrentTimestamp {
        precision: u32,
        time_zone: Db2TimeZone,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2BoundColumnDefault {
    ImplicitNull,
    /// A value must be supplied at insertion/update/LOAD; this is never NULL.
    MissingDefaultObligation,
    ExplicitNull,
    System(Db2SystemDefaultProducer),
    Numeric(Db2AssignedNumericConstant),
    String(Db2StoredStringConstant),
}

/// Immutable owned result for the column at the same index in statement().
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2BoundColumnDefaultResult {
    target: Db2ResolvedType,
    default: Db2BoundColumnDefault,
}

impl Db2BoundColumnDefaultResult {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.target
    }

    #[must_use]
    pub const fn default(&self) -> &Db2BoundColumnDefault {
        &self.default
    }
}

/// Opaque owned source/parse/column pairing. No caller AST is admitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2BoundCreateTableColumnDefaults {
    source: String,
    statement: Db2CreateTableStatement,
    columns: Vec<Db2BoundColumnDefaultResult>,
}

impl Db2BoundCreateTableColumnDefaults {
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Syntax only for names/constraints; this does not certify their binding.
    #[must_use]
    pub const fn statement(&self) -> &Db2CreateTableStatement {
        &self.statement
    }

    #[must_use]
    pub fn columns(&self) -> &[Db2BoundColumnDefaultResult] {
        &self.columns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ColumnDefaultBindingErrorCode {
    Syntax(Db2SyntaxDiagnosticCode),
    Type(Db2TypeErrorCode),
    NumericSource(Db2NumericConstantValueErrorCode),
    NumericAssignment(Db2NumericAssignmentErrorCode),
    StringSource(Db2StringConstantErrorCode),
    StringStorage(Db2StringStorageErrorCode),
    CharacterDefaultTooLong,
    WithDefaultSourcePending,
    UnsupportedDefault,
}

/// Fixed-size owner-mapped failure. Parser diagnostics provide a location only;
/// later owners retain their original span without inventing byte coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ColumnDefaultBindingError {
    pub code: Db2ColumnDefaultBindingErrorCode,
    pub location: Db2SourceLocation,
    pub span: Option<Db2SourceSpan>,
    pub message: &'static str,
}

impl fmt::Display for Db2ColumnDefaultBindingError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.location.line, self.location.column, self.message
        )
    }
}

impl std::error::Error for Db2ColumnDefaultBindingError {}

fn located(
    code: Db2ColumnDefaultBindingErrorCode,
    span: Db2SourceSpan,
    message: &'static str,
) -> Db2ColumnDefaultBindingError {
    Db2ColumnDefaultBindingError {
        code,
        location: span.start,
        span: Some(span),
        message,
    }
}

/// Reparse the complete bounded original source exactly once using the current
/// CREATE TABLE owner, then bind only each column's type and default. Selected
/// literal proof constructors may lex the actual original operand again; they
/// never parse invented SQL or trust the AST's normalized numeric/string text.
/// Per-family proof/storage budgets apply when that family is consumed.
pub fn bind_db2_create_table_column_defaults(
    source: &str,
    limits: Db2ColumnDefaultBindingLimits,
    contexts: Db2ColumnDefaultStringContexts,
) -> Result<Db2BoundCreateTableColumnDefaults, Db2ColumnDefaultBindingError> {
    use Db2ColumnDefaultBindingErrorCode as Code;
    let statement =
        parse_db2_create_table_statement(source, limits.syntax, limits.ast).map_err(|error| {
            Db2ColumnDefaultBindingError {
                code: Code::Syntax(error.code),
                location: error.location,
                span: None,
                message: "original CREATE TABLE source is outside admitted syntax or limits",
            }
        })?;
    let mut columns = Vec::with_capacity(statement.columns().len());
    for column in statement.columns() {
        // Every WITH DEFAULT spelling is source-pending, independently of its
        // operand or target. Preserve the parser's spelling and clause span.
        if let Some(clause) = column.default()
            && clause.spelling() == Db2DefaultSpelling::WithDefault
        {
            return Err(located(
                Code::WithDefaultSourcePending,
                clause.span(),
                "WITH DEFAULT binding awaits source-rule disambiguation",
            ));
        }
        let nullability = if column.is_not_null() {
            Db2Nullability::NotNull
        } else {
            Db2Nullability::Nullable
        };
        let target = resolve_db2_type(column.data_type(), nullability)
            .map_err(|error| located(Code::Type(error.code), column.span(), error.message))?;
        let default = if let Some(clause) = column.default() {
            match clause.value() {
                None => Db2BoundColumnDefault::System(system_producer(&target)),
                Some(Db2Literal::Null) => Db2BoundColumnDefault::ExplicitNull,
                Some(Db2Literal::Number(_)) => {
                    let (Some(operand), Some(number)) =
                        (clause.value_span(), clause.numeric_token_span())
                    else {
                        return Err(located(
                            Code::UnsupportedDefault,
                            clause.span(),
                            "numeric default lacks original component locations",
                        ));
                    };
                    let natural = materialize_db2_located_numeric_operand(
                        source,
                        operand,
                        clause.numeric_sign_span(),
                        number,
                        limits.numeric,
                        limits.numeric_syntax,
                    )
                    .map_err(|error| {
                        located(Code::NumericSource(error.code), error.span, error.message)
                    })?;
                    let assigned =
                        assign_db2_numeric_constant(&natural, &target).map_err(|error| {
                            located(
                                Code::NumericAssignment(error.code),
                                error.span,
                                error.message,
                            )
                        })?;
                    Db2BoundColumnDefault::Numeric(assigned)
                }
                Some(Db2Literal::String { .. }) => {
                    let Some(operand) = clause.value_span() else {
                        return Err(located(
                            Code::UnsupportedDefault,
                            clause.span(),
                            "string default lacks its original location",
                        ));
                    };
                    let natural = materialize_db2_string_constant(
                        source,
                        operand,
                        limits.string,
                        contexts.source,
                    )
                    .map_err(|error| {
                        located(Code::StringSource(error.code), error.span, error.message)
                    })?;
                    if let Db2StringConstantValue::Character(text) = natural.value()
                        && text.len() > 1536
                    {
                        return Err(located(
                            Code::CharacterDefaultTooLong,
                            operand,
                            "character default exceeds 1536 decoded UTF-8 bytes before storage assignment",
                        ));
                    }
                    let context = if matches!(
                        target.scalar(),
                        Db2ScalarType::Binary { .. } | Db2ScalarType::VarBinary { .. }
                    ) {
                        contexts.binary_target
                    } else {
                        contexts.character_target
                    };
                    let stored =
                        store_db2_string_constant(&natural, &target, context, limits.storage)
                            .map_err(|error| {
                                located(Code::StringStorage(error.code), error.span, error.message)
                            })?;
                    Db2BoundColumnDefault::String(stored)
                }
                Some(Db2Literal::Boolean(_)) => {
                    return Err(located(
                        Code::UnsupportedDefault,
                        clause.span(),
                        "Boolean default binding is not implemented",
                    ));
                }
            }
        } else if column.is_not_null() {
            Db2BoundColumnDefault::MissingDefaultObligation
        } else {
            Db2BoundColumnDefault::ImplicitNull
        };
        columns.push(Db2BoundColumnDefaultResult { target, default });
    }
    Ok(Db2BoundCreateTableColumnDefaults {
        source: source.to_owned(),
        statement,
        columns,
    })
}

fn system_producer(target: &Db2ResolvedType) -> Db2SystemDefaultProducer {
    use Db2ScalarType as Type;
    use Db2SystemDefaultProducer as Producer;
    match target.scalar() {
        Type::SmallInt
        | Type::Integer
        | Type::BigInt
        | Type::Decimal { .. }
        | Type::Float { .. }
        | Type::Real
        | Type::Double
        | Type::DecFloat { .. } => Producer::NumericZero,
        Type::Character { length } => Producer::FixedCharacterBlanks { length: *length },
        Type::Graphic { length } => Producer::FixedGraphicBlanks { length: *length },
        Type::Binary { length } => Producer::FixedBinaryZeros { length: *length },
        Type::VarChar { .. } | Type::VarGraphic { .. } | Type::VarBinary { .. } => {
            Producer::VaryingEmpty
        }
        Type::Date => Producer::CurrentDate,
        Type::Time => Producer::CurrentTime,
        Type::Timestamp {
            precision,
            time_zone,
        } => Producer::CurrentTimestamp {
            precision: *precision,
            time_zone: *time_zone,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Db2AssignedNumericValue, Db2MixedData, Db2NumericConstantErrorCode};

    type Code = Db2ColumnDefaultBindingErrorCode;

    fn bind(source: &str) -> Db2BoundCreateTableColumnDefaults {
        bind_db2_create_table_column_defaults(
            source,
            Db2ColumnDefaultBindingLimits::default(),
            contexts(),
        )
        .unwrap()
    }

    fn rejected(source: &str, expected: Code) -> Db2ColumnDefaultBindingError {
        let error = bind_db2_create_table_column_defaults(
            source,
            Db2ColumnDefaultBindingLimits::default(),
            contexts(),
        )
        .unwrap_err();
        assert_eq!(error.code, expected, "{source:?}");
        assert!(!error.message.is_empty());
        error
    }

    fn numeric(
        result: &Db2BoundCreateTableColumnDefaults,
        index: usize,
    ) -> &Db2AssignedNumericConstant {
        let Db2BoundColumnDefault::Numeric(value) = result.columns()[index].default() else {
            panic!("expected numeric default")
        };
        value
    }

    fn string(
        result: &Db2BoundCreateTableColumnDefaults,
        index: usize,
    ) -> &Db2StoredStringConstant {
        let Db2BoundColumnDefault::String(value) = result.columns()[index].default() else {
            panic!("expected string default")
        };
        value
    }

    fn contexts() -> Db2ColumnDefaultStringContexts {
        Db2ColumnDefaultStringContexts {
            source: Db2StringConstantContext::UnicodeUtf8 {
                mixed_data: Db2MixedData::No,
            },
            character_target: Db2StringStorageContext::UnicodeUtf8Mixed,
            binary_target: Db2StringStorageContext::Binary,
        }
    }

    #[test]
    fn default_binding_four_intents_are_distinct() {
        let result = bind_db2_create_table_column_defaults(
            "CREATE TABLE T (A INTEGER, B INTEGER NOT NULL, C INTEGER DEFAULT NULL, D INTEGER DEFAULT)",
            Db2ColumnDefaultBindingLimits::default(),
            contexts(),
        ).unwrap();
        assert_eq!(result.columns().len(), 4);
        assert_eq!(
            result.columns()[0].default(),
            &Db2BoundColumnDefault::ImplicitNull
        );
        assert_eq!(
            result.columns()[1].default(),
            &Db2BoundColumnDefault::MissingDefaultObligation
        );
        assert_eq!(
            result.columns()[2].default(),
            &Db2BoundColumnDefault::ExplicitNull
        );
        assert_eq!(
            result.columns()[3].default(),
            &Db2BoundColumnDefault::System(Db2SystemDefaultProducer::NumericZero)
        );
    }

    #[test]
    fn exact_assigned_integer_bounds_after_fractional_truncation() {
        for (target, spelling, expected) in [
            (
                "SMALLINT",
                "-32768.99",
                Db2AssignedNumericValue::SmallInt(-32768),
            ),
            (
                "SMALLINT",
                "+32767.99",
                Db2AssignedNumericValue::SmallInt(32767),
            ),
            (
                "INTEGER",
                "-2147483648.99",
                Db2AssignedNumericValue::Integer(i32::MIN),
            ),
            (
                "INTEGER",
                "2147483647.99",
                Db2AssignedNumericValue::Integer(i32::MAX),
            ),
            (
                "BIGINT",
                "-9223372036854775808.99",
                Db2AssignedNumericValue::BigInt(i64::MIN),
            ),
            (
                "BIGINT",
                "9223372036854775807.99",
                Db2AssignedNumericValue::BigInt(i64::MAX),
            ),
        ] {
            let source = format!("CREATE TABLE T (A {target} NOT NULL DEFAULT {spelling})");
            let result = bind(&source);
            let assigned = numeric(&result, 0);
            assert_eq!(*assigned.value(), expected);
            assert_eq!(assigned.conversion().discarded_fractional_digits(), 2);
            assert!(assigned.conversion().discarded_nonzero());
            assert_eq!(
                assigned.resolved_type(),
                result.columns()[0].resolved_type()
            );
            assert_eq!(
                assigned.resolved_type().nullability(),
                Db2Nullability::NotNull
            );
        }
        for (target, spelling) in [
            ("SMALLINT", "32768.0"),
            ("SMALLINT", "-32769.0"),
            ("INTEGER", "2147483648.0"),
            ("INTEGER", "-2147483649.0"),
            ("BIGINT", "9223372036854775808.0"),
            ("BIGINT", "-9223372036854775809.0"),
        ] {
            let source = format!("CREATE TABLE T (A {target} DEFAULT {spelling})");
            let error = rejected(
                &source,
                Code::NumericAssignment(Db2NumericAssignmentErrorCode::ValueOutOfRange),
            );
            let span = error.span.unwrap();
            assert_eq!(&source[span.start_byte..span.end_byte], spelling);
        }
    }

    #[test]
    fn decimal_precision_scale_loss_and_temporary_attributes() {
        for (target, spelling, coefficient, scale, discarded, nonzero) in [
            ("DECIMAL(5,2)", "-12.3499", -1234, 2, 2, true),
            ("DECIMAL(5,2)", "12.3400", 1234, 2, 2, false),
            (
                "DECIMAL(31,31)",
                ".1234567890123456789012345678901",
                1234567890123456789012345678901,
                31,
                0,
                false,
            ),
            (
                "DECIMAL(31,0)",
                "9999999999999999999999999999999",
                9999999999999999999999999999999,
                0,
                0,
                false,
            ),
            ("DECIMAL(4,2)", "-0.00", 0, 2, 0, false),
            ("DECIMAL(5,2)", "1", 100, 2, 0, false),
        ] {
            let result = bind(&format!("CREATE TABLE T (A {target} DEFAULT {spelling})"));
            let assigned = numeric(&result, 0);
            let Db2AssignedNumericValue::Decimal(value) = assigned.value() else {
                panic!("expected decimal")
            };
            assert_eq!(value.coefficient(), coefficient);
            assert_eq!(value.scale(), scale);
            assert_eq!(
                assigned.conversion().discarded_fractional_digits(),
                discarded
            );
            assert_eq!(assigned.conversion().discarded_nonzero(), nonzero);
            assert_eq!(
                assigned.resolved_type().nullability(),
                Db2Nullability::Nullable
            );
        }
        for (target, spelling) in [
            ("DECIMAL(4,2)", "100.00"),
            ("DECIMAL(31,31)", "1"),
            ("DECIMAL(31,30)", "9999999999999999999999999999999"),
        ] {
            rejected(
                &format!("CREATE TABLE T (A {target} DEFAULT {spelling})"),
                Code::NumericAssignment(Db2NumericAssignmentErrorCode::ValueOutOfRange),
            );
        }
        let result =
            bind("CREATE TABLE T (A DECIMAL(31,2) DEFAULT 1, B DECIMAL(31,2) DEFAULT 2147483648)");
        for (index, precision) in [(0, 11), (1, 19)] {
            assert_eq!(
                numeric(&result, index)
                    .conversion()
                    .temporary_decimal()
                    .unwrap()
                    .scalar(),
                &Db2ScalarType::Decimal {
                    precision,
                    scale: 0
                }
            );
        }
    }

    #[test]
    fn signed_nested_trivia_utf8_crlf_components_and_owned_contexts() {
        let result = {
            let source = String::from(
                "/*é*/\r\nCREATE TABLE \"t\" (\r\n \"n\" DECIMAL(5,2) DEFAULT -/*外/*n*/x*/\r\n 001.2300 NOT NULL,\r\n \"s\" CHAR(4) DEFAULT 'é'''\r\n)",
            );
            let limits = Db2ColumnDefaultBindingLimits::default();
            let context = contexts();
            bind_db2_create_table_column_defaults(&source, limits, context)
                .unwrap()
                .clone()
        };
        let span = |start_byte, end_byte, line, column, end_line, end_column| Db2SourceSpan {
            start_byte,
            end_byte,
            start: Db2SourceLocation { line, column },
            end: Db2SourceLocation {
                line: end_line,
                column: end_column,
            },
        };
        let clause = result.statement().columns()[0].default().unwrap();
        assert_eq!(clause.span(), span(46, 79, 3, 19, 4, 10));
        assert_eq!(clause.value_span(), Some(span(54, 79, 3, 27, 4, 10)));
        assert_eq!(clause.numeric_sign_span(), Some(span(54, 55, 3, 27, 3, 28)));
        assert_eq!(clause.numeric_token_span(), Some(span(71, 79, 4, 2, 4, 10)));
        assert_eq!(numeric(&result, 0).span(), span(54, 79, 3, 27, 4, 10));
        let Db2AssignedNumericValue::Decimal(value) = numeric(&result, 0).value() else {
            panic!("expected decimal")
        };
        assert_eq!((value.coefficient(), value.scale()), (-123, 2));
        assert_eq!(string(&result, 1).span(), span(112, 118, 5, 22, 5, 27));
        assert_eq!(
            string(&result, 1).source().span(),
            span(112, 118, 5, 22, 5, 27)
        );
        assert_eq!(string(&result, 1).value().bytes(), "é' ".as_bytes());
        assert_eq!(
            string(&result, 1).context(),
            Db2StringStorageContext::UnicodeUtf8Mixed
        );
        assert_eq!(result.statement().columns()[0].name().value(), "n");
        assert_eq!(&result.source()[54..79], "-/*外/*n*/x*/\r\n 001.2300");
    }

    #[test]
    fn character_binary_empty_padding_excess_and_nul() {
        let result = bind(
            "CREATE TABLE T (A CHAR(5) DEFAULT 'a''b', B CHAR(2) DEFAULT 'ab  ', C VARCHAR(5) NOT NULL DEFAULT '', D BINARY(4) DEFAULT BX'FF00', E VARBINARY(5) DEFAULT BX'', F CHAR(2) DEFAULT X'00', G VARCHAR(4) DEFAULT 'é')",
        );
        for (index, bytes) in [
            (0, b"a'b  ".as_slice()),
            (1, b"ab"),
            (2, b""),
            (3, &[255, 0, 0, 0]),
            (4, b""),
            (5, &[0, 32]),
            (6, "é".as_bytes()),
        ] {
            assert_eq!(string(&result, index).value().bytes(), bytes);
            assert_eq!(
                string(&result, index).resolved_type(),
                result.columns()[index].resolved_type()
            );
        }
        assert_eq!(string(&result, 0).padded_bytes(), 2);
        assert_eq!(string(&result, 1).truncated_bytes(), 2);
        assert_eq!(
            string(&result, 2).resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(
            string(&result, 2).source().resolved_type().scalar(),
            &Db2ScalarType::VarChar { length: 0 }
        );
        rejected(
            "CREATE TABLE T (A CHAR(2) DEFAULT 'ab x')",
            Code::StringStorage(Db2StringStorageErrorCode::NonBlankExcess),
        );
        rejected(
            "CREATE TABLE T (A CHAR(1) DEFAULT 'é')",
            Code::StringStorage(Db2StringStorageErrorCode::NonBlankExcess),
        );
        rejected(
            "CREATE TABLE T (A BINARY(1) DEFAULT BX'0000')",
            Code::StringStorage(Db2StringStorageErrorCode::BinaryTooLong),
        );
        rejected(
            "CREATE TABLE T (A VARCHAR(2) DEFAULT BX'00')",
            Code::StringStorage(Db2StringStorageErrorCode::IncompatibleTypes),
        );
        rejected(
            "CREATE TABLE T (A VARBINARY(2) DEFAULT X'00')",
            Code::StringStorage(Db2StringStorageErrorCode::IncompatibleTypes),
        );
    }

    #[test]
    fn character_default_ceiling_is_pre_trim_and_counts_utf8_bytes() {
        for (body, admitted) in [
            (" ".repeat(1536), true),
            (" ".repeat(1537), false),
            ("é".repeat(768), true),
            (format!("{} ", "é".repeat(768)), false),
        ] {
            let source = format!("CREATE TABLE T (A VARCHAR(1536) DEFAULT '{body}')");
            if admitted {
                assert!(matches!(
                    bind(&source).columns()[0].default(),
                    Db2BoundColumnDefault::String(_)
                ));
            } else {
                rejected(&source, Code::CharacterDefaultTooLong);
            }
        }
        for size in [1536, 1537] {
            let source = format!("CREATE TABLE T (A CHAR(1) DEFAULT '{}')", " ".repeat(size));
            if size == 1536 {
                assert_eq!(string(&bind(&source), 0).value().bytes(), b" ");
            } else {
                rejected(&source, Code::CharacterDefaultTooLong);
            }
            let hex = format!(
                "CREATE TABLE T (A CHAR(1) DEFAULT X'{}')",
                "20".repeat(size)
            );
            if size == 1536 {
                assert_eq!(string(&bind(&hex), 0).truncated_bytes(), 1535);
            } else {
                rejected(&hex, Code::CharacterDefaultTooLong);
            }
        }
        let binary = format!(
            "CREATE TABLE T (A VARBINARY(1537) DEFAULT BX'{}')",
            "00".repeat(1537)
        );
        assert_eq!(string(&bind(&binary), 0).value().bytes().len(), 1537);
        let escaped = format!(
            "CREATE TABLE T (A VARCHAR(1536) DEFAULT '{}')",
            "''".repeat(1536)
        );
        assert_eq!(
            string(&bind(&escaped), 0).source().value().bytes().len(),
            1536
        );
    }

    #[test]
    fn all_resolved_families_keep_nullable_missing_null_and_system_intents() {
        use Db2SystemDefaultProducer as P;
        for (target, producer) in [
            ("SMALLINT", P::NumericZero),
            ("INTEGER", P::NumericZero),
            ("BIGINT", P::NumericZero),
            ("DECIMAL(31,31)", P::NumericZero),
            ("REAL", P::NumericZero),
            ("DOUBLE", P::NumericZero),
            ("FLOAT(1)", P::NumericZero),
            ("FLOAT(53)", P::NumericZero),
            ("DECFLOAT(16)", P::NumericZero),
            ("DECFLOAT(34)", P::NumericZero),
            ("CHAR(7)", P::FixedCharacterBlanks { length: 7 }),
            ("GRAPHIC(3)", P::FixedGraphicBlanks { length: 3 }),
            ("BINARY(5)", P::FixedBinaryZeros { length: 5 }),
            ("VARCHAR(7)", P::VaryingEmpty),
            ("VARGRAPHIC(3)", P::VaryingEmpty),
            ("VARBINARY(5)", P::VaryingEmpty),
            ("DATE", P::CurrentDate),
            ("TIME", P::CurrentTime),
            (
                "TIMESTAMP",
                P::CurrentTimestamp {
                    precision: 6,
                    time_zone: Db2TimeZone::WithoutTimeZone,
                },
            ),
            (
                "TIMESTAMP(0) WITHOUT TIME ZONE",
                P::CurrentTimestamp {
                    precision: 0,
                    time_zone: Db2TimeZone::WithoutTimeZone,
                },
            ),
            (
                "TIMESTAMP(12) WITH TIME ZONE",
                P::CurrentTimestamp {
                    precision: 12,
                    time_zone: Db2TimeZone::WithTimeZone,
                },
            ),
        ] {
            let result = bind(&format!(
                "CREATE TABLE T (A {target}, B {target} NOT NULL, C {target} DEFAULT NULL, D {target} DEFAULT, E {target} DEFAULT NOT NULL)"
            ));
            assert_eq!(
                result.columns()[0].default(),
                &Db2BoundColumnDefault::ImplicitNull
            );
            assert_eq!(
                result.columns()[1].default(),
                &Db2BoundColumnDefault::MissingDefaultObligation
            );
            assert_eq!(
                result.columns()[2].default(),
                &Db2BoundColumnDefault::ExplicitNull
            );
            for index in [3, 4] {
                assert_eq!(
                    result.columns()[index].default(),
                    &Db2BoundColumnDefault::System(producer)
                );
            }
            for index in [0, 2, 3] {
                assert_eq!(
                    result.columns()[index].resolved_type().nullability(),
                    Db2Nullability::Nullable
                );
            }
            for index in [1, 4] {
                assert_eq!(
                    result.columns()[index].resolved_type().nullability(),
                    Db2Nullability::NotNull
                );
            }
        }
    }

    #[test]
    fn with_default_is_always_fixed_located_source_pending() {
        for tail in ["", " NULL", " 1", " ''", " NOT NULL"] {
            let source = format!("CREATE TABLE T (A INTEGER WITH DEFAULT{tail})");
            let syntax = parse_db2_create_table_statement(
                &source,
                Db2SyntaxLimits::default(),
                Db2AstLimits::default(),
            )
            .unwrap();
            assert_eq!(
                syntax.columns()[0].default().unwrap().spelling(),
                Db2DefaultSpelling::WithDefault
            );
            let error = rejected(&source, Code::WithDefaultSourcePending);
            assert_eq!(
                error.span,
                Some(syntax.columns()[0].default().unwrap().span())
            );
            assert_eq!(
                error.location,
                Db2SourceLocation {
                    line: 1,
                    column: 27
                }
            );
            assert_eq!(
                error.message,
                "WITH DEFAULT binding awaits source-rule disambiguation"
            );
        }
    }

    #[test]
    fn unsupported_types_constants_and_contexts_keep_owner_codes() {
        for (target, code) in [
            ("S.MYTYPE", Db2TypeErrorCode::UnsupportedDistinct),
            ("CLOB(5)", Db2TypeErrorCode::UnsupportedLob),
            ("ROWID", Db2TypeErrorCode::UnsupportedRowId),
            ("XML", Db2TypeErrorCode::UnsupportedXml),
            ("CHAR(0)", Db2TypeErrorCode::InvalidLength),
            ("TIME WITH TIME ZONE", Db2TypeErrorCode::InvalidTimeZone),
        ] {
            rejected(
                &format!("CREATE TABLE T (A {target} DEFAULT)"),
                Code::Type(code),
            );
        }
        for (target, code) in [
            (
                "REAL",
                Db2NumericAssignmentErrorCode::UnsupportedFloatingTarget,
            ),
            (
                "DECFLOAT",
                Db2NumericAssignmentErrorCode::UnsupportedDecFloatTarget,
            ),
            (
                "CHAR(5)",
                Db2NumericAssignmentErrorCode::UnsupportedNonNumericTarget,
            ),
        ] {
            rejected(
                &format!("CREATE TABLE T (A {target} DEFAULT 1)"),
                Code::NumericAssignment(code),
            );
        }
        for (target, code) in [
            (
                "GRAPHIC(5)",
                Db2StringStorageErrorCode::UnsupportedGraphicTarget,
            ),
            ("DATE", Db2StringStorageErrorCode::UnsupportedDatetimeTarget),
        ] {
            rejected(
                &format!("CREATE TABLE T (A {target} DEFAULT 'a')"),
                Code::StringStorage(code),
            );
        }
        let source = "CREATE TABLE T (A CHAR(2) DEFAULT 'a')";
        for source_context in [
            Db2StringConstantContext::Ascii,
            Db2StringConstantContext::Ebcdic,
            Db2StringConstantContext::UnicodeUtf16,
        ] {
            let context = Db2ColumnDefaultStringContexts {
                source: source_context,
                ..contexts()
            };
            let error = bind_db2_create_table_column_defaults(
                source,
                Db2ColumnDefaultBindingLimits::default(),
                context,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Code::StringSource(Db2StringConstantErrorCode::UnsupportedContext)
            );
        }
        for (target_context, code) in [
            (
                Db2StringStorageContext::Ascii,
                Db2StringStorageErrorCode::UnsupportedCharacterConversion,
            ),
            (
                Db2StringStorageContext::UnicodeUtf16,
                Db2StringStorageErrorCode::UnsupportedCharacterConversion,
            ),
            (
                Db2StringStorageContext::BitData,
                Db2StringStorageErrorCode::UnsupportedBitData,
            ),
            (
                Db2StringStorageContext::Binary,
                Db2StringStorageErrorCode::TargetEncodingMismatch,
            ),
        ] {
            let context = Db2ColumnDefaultStringContexts {
                character_target: target_context,
                ..contexts()
            };
            let error = bind_db2_create_table_column_defaults(
                source,
                Db2ColumnDefaultBindingLimits::default(),
                context,
            )
            .unwrap_err();
            assert_eq!(error.code, Code::StringStorage(code));
        }
    }

    #[test]
    fn complete_original_source_must_parse_and_not_null_conflicts_remain_rejected() {
        for source in [
            "",
            "CREATE TABLE T ()",
            "CREATE TABLE T (A INTEGER DEFAULT 1) garbage",
            "CREATE TABLE T (A INTEGER); CREATE TABLE U (B INTEGER)",
            "CREATE TABLE T (A INTEGER DEFAULT NULL NOT NULL)",
            "CREATE TABLE T (A INTEGER NOT NULL DEFAULT NULL)",
            "CREATE TABLE T (A INTEGER DEFAULT CURRENT DATE)",
            "CREATE TABLE T (A INTEGER DEFAULT USER)",
            "CREATE TABLE T (A INTEGER DEFAULT S.CAST(1))",
            "CREATE TABLE T (A GRAPHIC(1) DEFAULT GX'0041')",
            "CREATE TABLE T (A INTEGER DEFAULT 1 + 2)",
            "CREATE TABLE T (A INTEGER DEFAULT 1e2)",
            "CREATE TABLE T (A INTEGER DEFAULT, A INTEGER)",
        ] {
            let error = bind_db2_create_table_column_defaults(
                source,
                Db2ColumnDefaultBindingLimits::default(),
                contexts(),
            )
            .unwrap_err();
            assert!(
                matches!(error.code, Code::Syntax(_)),
                "{source:?}: {error:?}"
            );
            assert!(error.span.is_none());
        }
    }

    fn with_limits(
        source: &str,
        limits: Db2ColumnDefaultBindingLimits,
    ) -> Result<Db2BoundCreateTableColumnDefaults, Db2ColumnDefaultBindingError> {
        bind_db2_create_table_column_defaults(source, limits, contexts())
    }

    #[test]
    fn full_statement_source_token_ast_and_column_order_bounds() {
        let source = "CREATE TABLE T (B INTEGER DEFAULT 1, A INTEGER NOT NULL)";
        let limits = Db2ColumnDefaultBindingLimits {
            syntax: Db2SyntaxLimits {
                max_statement_bytes: source.len(),
                ..Db2SyntaxLimits::default()
            },
            ast: Db2AstLimits {
                max_list_items: 2,
                ..Db2AstLimits::default()
            },
            ..Db2ColumnDefaultBindingLimits::default()
        };
        let result = with_limits(source, limits).unwrap();
        assert_eq!(
            result
                .statement()
                .columns()
                .iter()
                .map(|column| column.name().value())
                .collect::<Vec<_>>(),
            ["B", "A"]
        );
        assert!(matches!(
            result.columns()[0].default(),
            Db2BoundColumnDefault::Numeric(_)
        ));
        assert_eq!(
            result.columns()[1].default(),
            &Db2BoundColumnDefault::MissingDefaultObligation
        );
        let mut one_beyond = limits;
        one_beyond.syntax.max_statement_bytes -= 1;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::Syntax(Db2SyntaxDiagnosticCode::StatementTooLarge)
        );
        one_beyond = limits;
        one_beyond.ast.max_list_items = 1;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::Syntax(Db2SyntaxDiagnosticCode::InvalidStatementOperand)
        );
        let source = "CREATE TABLE T (A INTEGER DEFAULT 1)";
        let limits = Db2ColumnDefaultBindingLimits {
            syntax: Db2SyntaxLimits {
                max_tokens: 9,
                max_token_bytes: 7,
                ..Db2SyntaxLimits::default()
            },
            ..Db2ColumnDefaultBindingLimits::default()
        };
        assert!(with_limits(source, limits).is_ok());
        let mut one_beyond = limits;
        one_beyond.syntax.max_tokens = 8;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::Syntax(Db2SyntaxDiagnosticCode::TooManyTokens)
        );
        one_beyond = limits;
        one_beyond.syntax.max_token_bytes = 6;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::Syntax(Db2SyntaxDiagnosticCode::TokenTooLarge)
        );
        let source = "CREATE TABLE T (A INTEGER DEFAULT -12)";
        let limits = Db2ColumnDefaultBindingLimits {
            ast: Db2AstLimits {
                max_literal_bytes: 3,
                ..Db2AstLimits::default()
            },
            ..Db2ColumnDefaultBindingLimits::default()
        };
        assert!(with_limits(source, limits).is_ok());
        let mut one_beyond = limits;
        one_beyond.ast.max_literal_bytes = 2;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::Syntax(Db2SyntaxDiagnosticCode::InvalidStatementOperand)
        );
    }

    #[test]
    fn both_numeric_original_source_budgets_and_original_combined_span_budget() {
        let source = "CREATE TABLE T (A INTEGER DEFAULT -/*a/*b*/c*/ 12)";
        let limits = Db2ColumnDefaultBindingLimits {
            numeric: Db2NumericConstantLimits {
                max_source_bytes: source.len(),
                ast: Db2AstLimits {
                    max_literal_bytes: 15,
                    ..Db2AstLimits::default()
                },
            },
            numeric_syntax: Db2SyntaxLimits {
                max_statement_bytes: source.len(),
                max_tokens: 2,
                max_token_bytes: 2,
                max_nesting: 2,
            },
            ..Db2ColumnDefaultBindingLimits::default()
        };
        assert_eq!(
            *numeric(&with_limits(source, limits).unwrap(), 0).value(),
            Db2AssignedNumericValue::Integer(-12)
        );
        for budget in [0, 1] {
            let mut one_beyond = limits;
            if budget == 0 {
                one_beyond.numeric.max_source_bytes -= 1;
            } else {
                one_beyond.numeric_syntax.max_statement_bytes -= 1;
            }
            assert_eq!(
                with_limits(source, one_beyond).unwrap_err().code,
                Code::NumericSource(Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::SourceTooLarge
                ))
            );
        }
        let mut one_beyond = limits;
        one_beyond.numeric.ast.max_literal_bytes = 14;
        let error = with_limits(source, one_beyond).unwrap_err();
        assert_eq!(
            error.code,
            Code::NumericSource(Db2NumericConstantValueErrorCode::Classification(
                Db2NumericConstantErrorCode::ConstantTooLarge
            ))
        );
        assert_eq!(
            error.span.unwrap().end_byte - error.span.unwrap().start_byte,
            15
        );
        for (field, code) in [
            (0, Db2SyntaxDiagnosticCode::TooManyTokens),
            (1, Db2SyntaxDiagnosticCode::TokenTooLarge),
            (2, Db2SyntaxDiagnosticCode::UnbalancedDelimiter),
        ] {
            let mut one_beyond = limits;
            match field {
                0 => one_beyond.numeric_syntax.max_tokens = 1,
                1 => one_beyond.numeric_syntax.max_token_bytes = 1,
                _ => one_beyond.numeric_syntax.max_nesting = 1,
            }
            assert_eq!(
                with_limits(source, one_beyond).unwrap_err().code,
                Code::NumericSource(Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::Syntax(code)
                ))
            );
        }
    }

    #[test]
    fn string_original_source_spelling_token_decoded_and_storage_budgets() {
        let source = "CREATE TABLE T (A CHAR(5) DEFAULT 'a''b')";
        let limits = Db2ColumnDefaultBindingLimits {
            string: Db2StringConstantLimits {
                syntax: Db2SyntaxLimits {
                    max_statement_bytes: source.len(),
                    max_token_bytes: 6,
                    max_tokens: 1,
                    ..Db2SyntaxLimits::default()
                },
                ast: Db2AstLimits {
                    max_literal_bytes: 6,
                    ..Db2AstLimits::default()
                },
                max_value_bytes: 3,
            },
            storage: Db2StringStorageLimits {
                max_output_bytes: 5,
            },
            ..Db2ColumnDefaultBindingLimits::default()
        };
        assert_eq!(
            string(&with_limits(source, limits).unwrap(), 0)
                .value()
                .bytes(),
            b"a'b  "
        );
        for (field, code) in [
            (0, Db2StringConstantErrorCode::SourceTooLarge),
            (1, Db2StringConstantErrorCode::SpellingTooLarge),
            (2, Db2StringConstantErrorCode::TokenTooLarge),
            (3, Db2StringConstantErrorCode::ValueTooLarge),
        ] {
            let mut one_beyond = limits;
            match field {
                0 => one_beyond.string.syntax.max_statement_bytes -= 1,
                1 => one_beyond.string.ast.max_literal_bytes = 5,
                2 => one_beyond.string.syntax.max_token_bytes = 5,
                _ => one_beyond.string.max_value_bytes = 2,
            }
            assert_eq!(
                with_limits(source, one_beyond).unwrap_err().code,
                Code::StringSource(code)
            );
        }
        let mut one_beyond = limits;
        one_beyond.storage.max_output_bytes = 4;
        assert_eq!(
            with_limits(source, one_beyond).unwrap_err().code,
            Code::StringStorage(Db2StringStorageErrorCode::OutputTooLarge)
        );
    }

    #[test]
    fn invalid_and_compiled_one_beyond_limits_keep_static_owner_failures() {
        let numeric_source = "CREATE TABLE T (A INTEGER DEFAULT 1)";
        let string_source = "CREATE TABLE T (A CHAR(2) DEFAULT 'a')";
        for (field, expected) in [
            (0, Code::Syntax(Db2SyntaxDiagnosticCode::InvalidLimits)),
            (1, Code::Syntax(Db2SyntaxDiagnosticCode::InvalidLimits)),
            (
                2,
                Code::NumericSource(Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::InvalidLimits,
                )),
            ),
            (
                3,
                Code::NumericSource(Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::InvalidLimits,
                )),
            ),
            (
                4,
                Code::StringSource(Db2StringConstantErrorCode::InvalidLimits),
            ),
            (
                5,
                Code::StringStorage(Db2StringStorageErrorCode::InvalidLimits),
            ),
        ] {
            for invalid in [0, 1] {
                let mut limits = Db2ColumnDefaultBindingLimits::default();
                match field {
                    0 => {
                        limits.syntax.max_statement_bytes =
                            if invalid == 0 { 0 } else { 8 * 1024 * 1024 + 1 }
                    }
                    1 => limits.ast.max_identifier_bytes = if invalid == 0 { 0 } else { 1025 },
                    2 => {
                        limits.numeric.max_source_bytes =
                            if invalid == 0 { 0 } else { 8 * 1024 * 1024 + 1 }
                    }
                    3 => {
                        limits.numeric_syntax.max_statement_bytes =
                            if invalid == 0 { 0 } else { 8 * 1024 * 1024 + 1 }
                    }
                    4 => limits.string.max_value_bytes = if invalid == 0 { 0 } else { 32705 },
                    _ => limits.storage.max_output_bytes = if invalid == 0 { 0 } else { 32705 },
                }
                let error = with_limits(
                    if field >= 4 {
                        string_source
                    } else {
                        numeric_source
                    },
                    limits,
                )
                .unwrap_err();
                assert_eq!(error.code, expected);
                assert!(!error.message.is_empty());
            }
        }
    }
}
