//! Owned bounded syntax for the declared common `CREATE TABLE` slice.
//!
//! This parser is intentionally disconnected from statement recognition,
//! binding, execution, authorization, and persistence. It accepts only the
//! source-reviewed common subset below and never returns raw tokens or a
//! generic-success node.

mod columns;
mod constraints;

#[cfg(test)]
mod tests;

use crate::{
    Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2DataType, Db2Identifier, Db2Literal,
    Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol, Db2SyntaxDiagnostic,
    Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2,
};

/// One bounded named-table definition from the common `CREATE TABLE` subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateTableStatement {
    table_name: Db2QualifiedName,
    columns: Vec<Db2CreateTableColumn>,
    constraints: Vec<Db2CreateTableConstraint>,
    span: Db2SourceSpan,
}

impl Db2CreateTableStatement {
    #[must_use]
    pub const fn table_name(&self) -> &Db2QualifiedName {
        &self.table_name
    }

    #[must_use]
    pub fn columns(&self) -> &[Db2CreateTableColumn] {
        &self.columns
    }

    #[must_use]
    pub fn constraints(&self) -> &[Db2CreateTableConstraint] {
        &self.constraints
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateTableColumn {
    name: Db2Identifier,
    data_type: Db2DataType,
    not_null: bool,
    default: Option<Db2ColumnDefault>,
    span: Db2SourceSpan,
}

impl Db2CreateTableColumn {
    #[must_use]
    pub const fn name(&self) -> &Db2Identifier {
        &self.name
    }

    #[must_use]
    pub const fn data_type(&self) -> &Db2DataType {
        &self.data_type
    }

    #[must_use]
    pub const fn is_not_null(&self) -> bool {
        self.not_null
    }

    #[must_use]
    pub const fn default(&self) -> Option<&Db2ColumnDefault> {
        self.default.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Whether the source used `DEFAULT` or the equivalent `WITH DEFAULT` spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DefaultSpelling {
    Default,
    WithDefault,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ColumnDefault {
    spelling: Db2DefaultSpelling,
    value: Db2Literal,
}

impl Db2ColumnDefault {
    #[must_use]
    pub const fn spelling(&self) -> Db2DefaultSpelling {
        self.spelling
    }

    #[must_use]
    pub const fn value(&self) -> &Db2Literal {
        &self.value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateTableConstraint {
    name: Option<Db2Identifier>,
    kind: Db2TableConstraintKind,
    span: Db2SourceSpan,
}

impl Db2CreateTableConstraint {
    #[must_use]
    pub const fn name(&self) -> Option<&Db2Identifier> {
        self.name.as_ref()
    }

    #[must_use]
    pub const fn kind(&self) -> &Db2TableConstraintKind {
        &self.kind
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2TableConstraintKind {
    PrimaryKey(Vec<Db2Identifier>),
    Unique(Vec<Db2Identifier>),
    ForeignKey(Db2ForeignKeyConstraint),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ForeignKeyConstraint {
    columns: Vec<Db2Identifier>,
    referenced_table: Db2QualifiedName,
    referenced_columns: Option<Vec<Db2Identifier>>,
    on_delete: Option<Db2OnDeleteAction>,
}

impl Db2ForeignKeyConstraint {
    #[must_use]
    pub fn columns(&self) -> &[Db2Identifier] {
        &self.columns
    }

    #[must_use]
    pub const fn referenced_table(&self) -> &Db2QualifiedName {
        &self.referenced_table
    }

    /// `None` preserves omission of the parent column list; Db2 then selects
    /// the parent primary key during later binding.
    #[must_use]
    pub fn referenced_columns(&self) -> Option<&[Db2Identifier]> {
        self.referenced_columns.as_deref()
    }

    #[must_use]
    pub const fn on_delete(&self) -> Option<Db2OnDeleteAction> {
        self.on_delete
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2OnDeleteAction {
    Restrict,
    NoAction,
    Cascade,
    SetNull,
}

/// Parse exactly one common named-table definition. Advanced column clauses,
/// CHECK/period constraints, LIKE/AS forms, materialized definitions, and
/// physical table clauses fail explicitly and remain pending.
pub fn parse_db2_create_table_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2CreateTableStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let mut parser = CreateTableParser::new(lexed.tokens(), ast_limits);
    parser.parse()
}

struct CreateTableParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    limits: Db2AstLimits,
}

impl<'a> CreateTableParser<'a> {
    const fn new(tokens: &'a [Db2Token], limits: Db2AstLimits) -> Self {
        Self {
            tokens,
            position: 0,
            limits,
        }
    }

    fn parse(&mut self) -> Result<Db2CreateTableStatement, Db2SyntaxDiagnostic> {
        if !self.take_word("CREATE") {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the common Db2 CREATE TABLE syntax family",
            ));
        }
        if !self.take_word("TABLE") {
            let code = if self.at_end() {
                Db2SyntaxDiagnosticCode::MissingToken
            } else {
                Db2SyntaxDiagnosticCode::UnsupportedStatement
            };
            return Err(self.diagnostic_here(code, "CREATE must be followed by TABLE"));
        }
        let table_name = self.qualified_name("table name")?;
        if !self.take_symbol(Db2Symbol::LeftParenthesis) {
            let code = if self.word().is_some() {
                Db2SyntaxDiagnosticCode::UnsupportedStatement
            } else {
                Db2SyntaxDiagnosticCode::MissingToken
            };
            return Err(self.diagnostic_here(
                code,
                "common CREATE TABLE requires a parenthesized named-table definition",
            ));
        }
        if self.take_symbol(Db2Symbol::RightParenthesis) {
            return Err(self.diagnostic_previous(
                Db2SyntaxDiagnosticCode::MissingToken,
                "CREATE TABLE requires at least one named column",
            ));
        }

        let mut columns = Vec::new();
        let mut constraints = Vec::new();
        let mut element_count = 0_usize;
        loop {
            if element_count >= self.limits.max_list_items {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "CREATE TABLE elements exceed the configured list limit",
                ));
            }
            if self.starts_table_constraint() {
                let constraint = self.table_constraint()?;
                if let Some(name) = constraint.name()
                    && constraints
                        .iter()
                        .any(|existing: &Db2CreateTableConstraint| {
                            existing
                                .name()
                                .is_some_and(|other| same_identifier(name, other))
                        })
                {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        constraint.span.start,
                        "CREATE TABLE constraint name is specified more than once",
                    ));
                }
                constraints.push(constraint);
            } else if matches!(self.word(), Some("CHECK" | "PERIOD")) {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "CHECK and PERIOD definitions are outside the declared common subset",
                ));
            } else {
                let column = self.column_definition()?;
                if columns.iter().any(|existing: &Db2CreateTableColumn| {
                    same_identifier(existing.name(), column.name())
                }) {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        column.span.start,
                        "CREATE TABLE column name is specified more than once",
                    ));
                }
                columns.push(column);
            }
            element_count += 1;

            if self.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            self.expect_symbol(Db2Symbol::Comma)?;
            if self.at_end()
                || self.peek_symbol(Db2Symbol::Semicolon)
                || self.peek_symbol(Db2Symbol::RightParenthesis)
            {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "missing CREATE TABLE element after comma",
                ));
            }
        }
        if columns.is_empty() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "CREATE TABLE requires at least one named column",
            ));
        }

        self.validate_constraints(&columns, &constraints)?;
        self.finish()?;
        Ok(Db2CreateTableStatement {
            table_name,
            columns,
            constraints,
            span: self.statement_span(),
        })
    }

    fn qualified_name(&mut self, label: &str) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
        let mut parts = Vec::new();
        loop {
            if parts.len() >= self.limits.max_name_parts {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    format!("Db2 {label} exceeds the configured part limit"),
                ));
            }
            parts.push(self.identifier(label)?);
            if !self.take_symbol(Db2Symbol::Period) {
                break;
            }
        }
        Db2QualifiedName::new(parts, self.limits).map_err(|problem| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                problem.message,
            )
        })
    }

    fn identifier(&mut self, label: &str) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.tokens.get(self.position) else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("missing Db2 {label}"),
            ));
        };
        let Db2TokenKind::Word { value, delimited } = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                format!("Db2 {label} must be an identifier"),
            ));
        };
        let identifier =
            Db2Identifier::new(value.clone(), *delimited, self.limits).map_err(|problem| {
                Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    token.span.start,
                    &problem.message,
                )
            })?;
        self.position += 1;
        Ok(identifier)
    }

    fn finish(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(Db2Symbol::Semicolon) {
            if !self.at_end() {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "only one Db2 statement is allowed",
                ));
            }
            return Ok(());
        }
        if !self.at_end() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "trailing CREATE TABLE clause is outside the declared common subset",
            ));
        }
        Ok(())
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word(expected) {
            Ok(())
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("expected Db2 keyword {expected}"),
            ))
        }
    }

    fn take_word(&mut self, expected: &str) -> bool {
        if self.word() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn word(&self) -> Option<&str> {
        self.word_at(self.position)
    }

    fn word_at(&self, position: usize) -> Option<&str> {
        match self.tokens.get(position).map(|token| &token.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) => Some(value),
            _ => None,
        }
    }

    fn expect_symbol(&mut self, expected: Db2Symbol) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(expected) {
            Ok(())
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("expected Db2 symbol {expected:?}"),
            ))
        }
    }

    fn take_symbol(&mut self, expected: Db2Symbol) -> bool {
        if self.peek_symbol(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek_symbol(&self, expected: Db2Symbol) -> bool {
        matches!(
            self.tokens.get(self.position).map(|token| &token.kind),
            Some(Db2TokenKind::Symbol(symbol)) if *symbol == expected
        )
    }

    fn at_element_end(&self) -> bool {
        self.at_end()
            || self.peek_symbol(Db2Symbol::Comma)
            || self.peek_symbol(Db2Symbol::RightParenthesis)
            || self.peek_symbol(Db2Symbol::Semicolon)
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
    }

    fn current_start(&self) -> Db2SourceLocation {
        self.tokens
            .get(self.position)
            .map_or_else(|| self.previous_end(), |token| token.span.start)
    }

    fn previous_end(&self) -> Db2SourceLocation {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(Db2SourceLocation::START, |token| token.span.end)
    }

    fn statement_span(&self) -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: self.tokens.first().map_or(0, |token| token.span.start_byte),
            end_byte: self.tokens.last().map_or(0, |token| token.span.end_byte),
            start: self
                .tokens
                .first()
                .map_or(Db2SourceLocation::START, |token| token.span.start),
            end: self
                .tokens
                .last()
                .map_or(Db2SourceLocation::START, |token| token.span.end),
        }
    }

    fn diagnostic_here(
        &self,
        code: Db2SyntaxDiagnosticCode,
        message: impl AsRef<str>,
    ) -> Db2SyntaxDiagnostic {
        Db2SyntaxDiagnostic::new(code, self.current_start(), message.as_ref())
    }

    fn diagnostic_previous(
        &self,
        code: Db2SyntaxDiagnosticCode,
        message: impl AsRef<str>,
    ) -> Db2SyntaxDiagnostic {
        let location = self
            .position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(Db2SourceLocation::START, |token| token.span.start);
        Db2SyntaxDiagnostic::new(code, location, message.as_ref())
    }
}

fn same_identifier(left: &Db2Identifier, right: &Db2Identifier) -> bool {
    left.value() == right.value()
}

fn identifier_signature(names: &[Db2Identifier]) -> Vec<String> {
    names.iter().map(|name| name.value().to_owned()).collect()
}

fn qualified_signature(name: &Db2QualifiedName) -> Vec<String> {
    identifier_signature(name.parts())
}

fn column_named<'a>(
    columns: &'a [Db2CreateTableColumn],
    name: &Db2Identifier,
) -> Option<&'a Db2CreateTableColumn> {
    columns
        .iter()
        .find(|column| same_identifier(column.name(), name))
}
