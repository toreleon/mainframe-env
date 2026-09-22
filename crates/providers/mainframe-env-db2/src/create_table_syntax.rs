//! Owned bounded syntax for the declared common `CREATE TABLE` slice.
//!
//! This parser is intentionally disconnected from statement recognition,
//! binding, execution, authorization, and persistence. It accepts only the
//! source-reviewed common subset below and never returns raw tokens or a
//! generic-success node.

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

    fn column_definition(&mut self) -> Result<Db2CreateTableColumn, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        let name = self.identifier("column name")?;
        let data_type = self.data_type()?;
        let mut not_null = false;
        let mut default = None;

        while !self.at_element_end() {
            if self.take_word("NOT") {
                self.expect_word("NULL")?;
                if not_null {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        "column NOT NULL is specified more than once",
                    ));
                }
                if default.as_ref().is_some_and(|clause: &Db2ColumnDefault| {
                    matches!(clause.value(), Db2Literal::Null)
                }) {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "NOT NULL conflicts with DEFAULT NULL",
                    ));
                }
                not_null = true;
                continue;
            }

            let spelling = if self.take_word("WITH") {
                self.expect_word("DEFAULT")?;
                Some(Db2DefaultSpelling::WithDefault)
            } else if self.take_word("DEFAULT") {
                Some(Db2DefaultSpelling::Default)
            } else {
                None
            };
            if let Some(spelling) = spelling {
                if default.is_some() {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        "column DEFAULT is specified more than once",
                    ));
                }
                let value = self.default_value()?;
                if not_null && matches!(value, Db2Literal::Null) {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "DEFAULT NULL conflicts with NOT NULL",
                    ));
                }
                default = Some(Db2ColumnDefault { spelling, value });
                continue;
            }

            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "column clause is outside NOT NULL and constant/NULL DEFAULT syntax",
            ));
        }

        Ok(Db2CreateTableColumn {
            name,
            data_type,
            not_null,
            default,
            span: Db2SourceSpan {
                start,
                end: self.previous_end(),
            },
        })
    }

    fn default_value(&mut self) -> Result<Db2Literal, Db2SyntaxDiagnostic> {
        if self.take_word("NULL") {
            return Ok(Db2Literal::Null);
        }
        let sign = if self.take_symbol(Db2Symbol::Plus) {
            Some('+')
        } else if self.take_symbol(Db2Symbol::Minus) {
            Some('-')
        } else {
            None
        };
        let Some(token) = self.tokens.get(self.position) else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "DEFAULT requires a constant or NULL operand",
            ));
        };
        let literal = match &token.kind {
            Db2TokenKind::Number(value) => {
                let mut value = value.clone();
                if let Some(sign) = sign {
                    value.insert(0, sign);
                }
                Db2Literal::Number(value)
            }
            Db2TokenKind::String { kind, value } if sign.is_none() => Db2Literal::String {
                kind: *kind,
                value: value.clone(),
            },
            _ => {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "DEFAULT operand must be a numeric/string constant or NULL",
                ));
            }
        };
        let literal_bytes = match &literal {
            Db2Literal::Number(value) | Db2Literal::String { value, .. } => value.len(),
            Db2Literal::Null | Db2Literal::Boolean(_) => 0,
        };
        if literal_bytes > self.limits.max_literal_bytes {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "DEFAULT constant exceeds the configured literal byte limit",
            ));
        }
        self.position += 1;
        Ok(literal)
    }

    fn table_constraint(&mut self) -> Result<Db2CreateTableConstraint, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        let name = if self.take_word("CONSTRAINT") {
            Some(self.identifier("constraint name")?)
        } else {
            None
        };
        let kind = if self.take_word("PRIMARY") {
            self.expect_word("KEY")?;
            Db2TableConstraintKind::PrimaryKey(self.identifier_list("primary-key column")?)
        } else if self.take_word("UNIQUE") {
            Db2TableConstraintKind::Unique(self.identifier_list("unique-key column")?)
        } else if self.take_word("FOREIGN") {
            self.expect_word("KEY")?;
            let columns = self.identifier_list("foreign-key column")?;
            self.expect_word("REFERENCES")?;
            let referenced_table = self.qualified_name("referenced table name")?;
            let referenced_columns = if self.peek_symbol(Db2Symbol::LeftParenthesis) {
                Some(self.identifier_list("referenced column")?)
            } else {
                None
            };
            let on_delete = if self.take_word("ON") {
                self.expect_word("DELETE")?;
                Some(self.on_delete_action()?)
            } else {
                None
            };
            if on_delete.is_some() && self.word() == Some("ON") {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "FOREIGN KEY ON DELETE is specified more than once",
                ));
            }
            Db2TableConstraintKind::ForeignKey(Db2ForeignKeyConstraint {
                columns,
                referenced_table,
                referenced_columns,
                on_delete,
            })
        } else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "only table PRIMARY KEY, UNIQUE, and FOREIGN KEY constraints are supported",
            ));
        };
        Ok(Db2CreateTableConstraint {
            name,
            kind,
            span: Db2SourceSpan {
                start,
                end: self.previous_end(),
            },
        })
    }

    fn on_delete_action(&mut self) -> Result<Db2OnDeleteAction, Db2SyntaxDiagnostic> {
        if self.take_word("RESTRICT") {
            Ok(Db2OnDeleteAction::Restrict)
        } else if self.take_word("NO") {
            self.expect_word("ACTION")?;
            Ok(Db2OnDeleteAction::NoAction)
        } else if self.take_word("CASCADE") {
            Ok(Db2OnDeleteAction::Cascade)
        } else if self.take_word("SET") {
            self.expect_word("NULL")?;
            Ok(Db2OnDeleteAction::SetNull)
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "ON DELETE requires RESTRICT, NO ACTION, CASCADE, or SET NULL",
            ))
        }
    }

    fn identifier_list(&mut self, label: &str) -> Result<Vec<Db2Identifier>, Db2SyntaxDiagnostic> {
        self.expect_symbol(Db2Symbol::LeftParenthesis)?;
        if self.take_symbol(Db2Symbol::RightParenthesis) {
            return Err(self.diagnostic_previous(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("{label} list must not be empty"),
            ));
        }
        let mut names = Vec::new();
        loop {
            if names.len() >= self.limits.max_list_items {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    format!("{label} list exceeds the configured limit"),
                ));
            }
            let name = self.identifier(label)?;
            if names
                .iter()
                .any(|existing| same_identifier(existing, &name))
            {
                return Err(self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    format!("{label} is specified more than once"),
                ));
            }
            names.push(name);
            if self.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            self.expect_symbol(Db2Symbol::Comma)?;
            if self.at_end() || self.peek_symbol(Db2Symbol::RightParenthesis) {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::MissingToken,
                    format!("missing {label} after comma"),
                ));
            }
        }
        Ok(names)
    }

    fn data_type(&mut self) -> Result<Db2DataType, Db2SyntaxDiagnostic> {
        let first = self.word().map(str::to_owned).ok_or_else(|| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "column definition requires a Db2 data type",
            )
        })?;
        let kind = match first.as_str() {
            "SMALLINT" => Some(Db2BuiltInType::SmallInt),
            "INTEGER" | "INT" => Some(Db2BuiltInType::Integer),
            "BIGINT" => Some(Db2BuiltInType::BigInt),
            "DECIMAL" | "DEC" | "NUMERIC" => Some(Db2BuiltInType::Decimal),
            "FLOAT" => Some(Db2BuiltInType::Float),
            "REAL" => Some(Db2BuiltInType::Real),
            "DOUBLE" => Some(Db2BuiltInType::Double),
            "DECFLOAT" => Some(Db2BuiltInType::DecFloat),
            "CHAR" | "CHARACTER" => Some(Db2BuiltInType::Character),
            "VARCHAR" => Some(Db2BuiltInType::VarChar),
            "CLOB" => Some(Db2BuiltInType::Clob),
            "GRAPHIC" => Some(Db2BuiltInType::Graphic),
            "VARGRAPHIC" => Some(Db2BuiltInType::VarGraphic),
            "DBCLOB" => Some(Db2BuiltInType::DbClob),
            "BINARY" => Some(Db2BuiltInType::Binary),
            "VARBINARY" => Some(Db2BuiltInType::VarBinary),
            "BLOB" => Some(Db2BuiltInType::Blob),
            "DATE" => Some(Db2BuiltInType::Date),
            "TIME" => Some(Db2BuiltInType::Time),
            "TIMESTAMP" => Some(Db2BuiltInType::Timestamp),
            "ROWID" => Some(Db2BuiltInType::RowId),
            "XML" => Some(Db2BuiltInType::Xml),
            _ => None,
        };
        if let Some(mut kind) = kind {
            self.position += 1;
            if kind == Db2BuiltInType::Double {
                self.take_word("PRECISION");
            } else if kind == Db2BuiltInType::Character && self.take_word("VARYING") {
                kind = Db2BuiltInType::VarChar;
            } else if kind == Db2BuiltInType::Binary && self.take_word("VARYING") {
                kind = Db2BuiltInType::VarBinary;
            }
            let mut arguments = Vec::new();
            if self.take_symbol(Db2Symbol::LeftParenthesis) {
                loop {
                    if arguments.len() >= 2 {
                        return Err(self.diagnostic_here(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            "Db2 data type has too many numeric arguments",
                        ));
                    }
                    arguments.push(self.unsigned_type_argument()?);
                    if self.take_symbol(Db2Symbol::RightParenthesis) {
                        break;
                    }
                    self.expect_symbol(Db2Symbol::Comma)?;
                }
            }
            let with_time_zone = if self.word() == Some("WITH")
                && self.word_at(self.position + 1) == Some("TIME")
            {
                self.position += 1;
                self.expect_word("TIME")?;
                self.expect_word("ZONE")?;
                true
            } else {
                if self.word() == Some("WITHOUT") && self.word_at(self.position + 1) == Some("TIME")
                {
                    self.position += 1;
                    self.expect_word("TIME")?;
                    self.expect_word("ZONE")?;
                }
                false
            };
            return Db2BuiltInDataType::new(kind, arguments, with_time_zone, self.limits)
                .map(Db2DataType::BuiltIn)
                .map_err(|problem| {
                    self.diagnostic_here(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        problem.message,
                    )
                });
        }
        self.qualified_name("distinct type name")
            .map(Db2DataType::Distinct)
    }

    fn unsigned_type_argument(&mut self) -> Result<u32, Db2SyntaxDiagnostic> {
        let Some(Db2Token {
            kind: Db2TokenKind::Number(value),
            ..
        }) = self.tokens.get(self.position)
        else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "Db2 data-type argument must be an unsigned integer",
            ));
        };
        let parsed = value.parse::<u32>().map_err(|_| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "Db2 data-type argument exceeds u32",
            )
        })?;
        self.position += 1;
        Ok(parsed)
    }

    fn validate_constraints(
        &self,
        columns: &[Db2CreateTableColumn],
        constraints: &[Db2CreateTableConstraint],
    ) -> Result<(), Db2SyntaxDiagnostic> {
        let mut primary_seen = false;
        let mut unique_signatures: Vec<Vec<String>> = Vec::new();
        let mut foreign_signatures: Vec<(Vec<String>, Vec<String>)> = Vec::new();

        for constraint in constraints {
            match constraint.kind() {
                Db2TableConstraintKind::PrimaryKey(names) => {
                    if primary_seen {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::DuplicateClause,
                            constraint.span.start,
                            "CREATE TABLE has more than one PRIMARY KEY",
                        ));
                    }
                    primary_seen = true;
                    self.validate_key_columns(columns, names, true, constraint.span.start)?;
                    let signature = identifier_signature(names);
                    if unique_signatures.contains(&signature) {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::DuplicateClause,
                            constraint.span.start,
                            "PRIMARY KEY duplicates a UNIQUE key",
                        ));
                    }
                    unique_signatures.push(signature);
                }
                Db2TableConstraintKind::Unique(names) => {
                    self.validate_key_columns(columns, names, true, constraint.span.start)?;
                    let signature = identifier_signature(names);
                    if unique_signatures.contains(&signature) {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::DuplicateClause,
                            constraint.span.start,
                            "UNIQUE constraint duplicates an existing key",
                        ));
                    }
                    unique_signatures.push(signature);
                }
                Db2TableConstraintKind::ForeignKey(foreign) => {
                    self.validate_key_columns(
                        columns,
                        foreign.columns(),
                        false,
                        constraint.span.start,
                    )?;
                    if let Some(parent_columns) = foreign.referenced_columns()
                        && parent_columns.len() != foreign.columns().len()
                    {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            constraint.span.start,
                            "FOREIGN KEY and referenced column counts differ",
                        ));
                    }
                    if foreign.on_delete() == Some(Db2OnDeleteAction::SetNull)
                        && !foreign.columns().iter().any(|name| {
                            column_named(columns, name).is_some_and(|column| !column.not_null)
                        })
                    {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            constraint.span.start,
                            "ON DELETE SET NULL requires a nullable foreign-key column",
                        ));
                    }
                    let signature = (
                        identifier_signature(foreign.columns()),
                        qualified_signature(foreign.referenced_table()),
                    );
                    if foreign_signatures.contains(&signature) {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::DuplicateClause,
                            constraint.span.start,
                            "FOREIGN KEY duplicates an existing child-key/parent-table pair",
                        ));
                    }
                    foreign_signatures.push(signature);
                }
            }
        }
        Ok(())
    }

    fn validate_key_columns(
        &self,
        columns: &[Db2CreateTableColumn],
        names: &[Db2Identifier],
        require_not_null: bool,
        location: Db2SourceLocation,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        for name in names {
            let Some(column) = column_named(columns, name) else {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    location,
                    "table constraint identifies an undeclared column",
                ));
            };
            if require_not_null && !column.not_null {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    location,
                    "PRIMARY KEY and UNIQUE columns must be declared NOT NULL",
                ));
            }
        }
        Ok(())
    }

    fn starts_table_constraint(&self) -> bool {
        matches!(
            self.word(),
            Some("CONSTRAINT" | "PRIMARY" | "UNIQUE" | "FOREIGN")
        )
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
                    problem.message,
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
        Db2SyntaxDiagnostic::new(code, self.current_start(), message)
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
        Db2SyntaxDiagnostic::new(code, location, message)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2CreateTableStatement, Db2SyntaxDiagnostic> {
        parse_db2_create_table_statement(
            source,
            Db2SyntaxLimits::default(),
            Db2AstLimits::default(),
        )
    }

    fn foreign(constraint: &Db2CreateTableConstraint) -> &Db2ForeignKeyConstraint {
        let Db2TableConstraintKind::ForeignKey(foreign) = constraint.kind() else {
            panic!("expected FOREIGN KEY")
        };
        foreign
    }

    #[test]
    fn create_table_builds_an_owned_bounded_ast() {
        let statement = parse(
            "CREATE TABLE app.orders (\
             id INTEGER NOT NULL, \
             amount DECIMAL(9,2) WITH DEFAULT -12.5, \
             state app.order_state WITH DEFAULT 'new', \
             note VARCHAR(20) DEFAULT NULL)",
        )
        .unwrap();
        assert_eq!(statement.table_name().parts()[0].value(), "APP");
        assert_eq!(statement.table_name().parts()[1].value(), "ORDERS");
        assert_eq!(statement.columns().len(), 4);
        assert!(statement.columns()[0].is_not_null());
        let Db2DataType::BuiltIn(decimal) = statement.columns()[1].data_type() else {
            panic!("expected DECIMAL")
        };
        assert_eq!(decimal.kind(), Db2BuiltInType::Decimal);
        assert_eq!(decimal.arguments(), &[9, 2]);
        assert_eq!(
            statement.columns()[1].default().unwrap().value(),
            &Db2Literal::Number("-12.5".into())
        );
        assert_eq!(
            statement.columns()[1].default().unwrap().spelling(),
            Db2DefaultSpelling::WithDefault
        );
        assert!(matches!(
            statement.columns()[2].data_type(),
            Db2DataType::Distinct(name) if name.parts().len() == 2
        ));
        assert_eq!(
            statement.columns()[2].default().unwrap().spelling(),
            Db2DefaultSpelling::WithDefault
        );
        assert_eq!(
            statement.columns()[3].default().unwrap().value(),
            &Db2Literal::Null
        );
        assert!(statement.constraints().is_empty());
    }

    #[test]
    fn built_in_aliases_and_time_zone_shape_match_owned_type_syntax() {
        let statement = parse(
            "CREATE TABLE types (\
             a SMALLINT, b INT, c BIGINT, d NUMERIC(7,3), e FLOAT(24), \
             f REAL, g DOUBLE PRECISION, h DECFLOAT(16), \
             i CHARACTER VARYING(12), j BINARY VARYING(8), \
             k TIMESTAMP(9) WITH TIME ZONE, l TIME WITHOUT TIME ZONE, \
             m CLOB(20), n GRAPHIC(4), o VARGRAPHIC(5), p DBCLOB(6), \
             q VARBINARY(7), r BLOB(8), s DATE, t ROWID, u XML)",
        )
        .unwrap();
        assert_eq!(statement.columns().len(), 21);
        let Db2DataType::BuiltIn(varying) = statement.columns()[8].data_type() else {
            panic!("expected varying character")
        };
        assert_eq!(varying.kind(), Db2BuiltInType::VarChar);
        let Db2DataType::BuiltIn(timestamp) = statement.columns()[10].data_type() else {
            panic!("expected timestamp")
        };
        assert!(timestamp.with_time_zone());
        let Db2DataType::BuiltIn(time) = statement.columns()[11].data_type() else {
            panic!("expected time")
        };
        assert!(!time.with_time_zone());
    }

    #[test]
    fn named_table_constraints_and_references_are_preserved() {
        let statement = parse(
            "CREATE TABLE app.child (\
             id INTEGER NOT NULL, code CHAR(4) NOT NULL, parent_id INTEGER NOT NULL, \
             optional_parent INTEGER, \
             CONSTRAINT pk_child PRIMARY KEY (id), \
             CONSTRAINT uq_child UNIQUE (code), \
             CONSTRAINT fk_child FOREIGN KEY (parent_id) \
               REFERENCES owner.parent (id) ON DELETE CASCADE, \
             FOREIGN KEY (optional_parent) REFERENCES parent ON DELETE SET NULL)",
        )
        .unwrap();
        assert_eq!(statement.constraints().len(), 4);
        assert_eq!(
            statement.constraints()[0].name().unwrap().value(),
            "PK_CHILD"
        );
        assert!(matches!(
            statement.constraints()[0].kind(),
            Db2TableConstraintKind::PrimaryKey(columns) if columns[0].value() == "ID"
        ));
        assert!(matches!(
            statement.constraints()[1].kind(),
            Db2TableConstraintKind::Unique(columns) if columns[0].value() == "CODE"
        ));
        let first = foreign(&statement.constraints()[2]);
        assert_eq!(first.referenced_table().parts().len(), 2);
        assert_eq!(first.referenced_columns().unwrap()[0].value(), "ID");
        assert_eq!(first.on_delete(), Some(Db2OnDeleteAction::Cascade));
        let second = foreign(&statement.constraints()[3]);
        assert!(second.referenced_columns().is_none());
        assert_eq!(second.on_delete(), Some(Db2OnDeleteAction::SetNull));
    }

    #[test]
    fn every_common_on_delete_action_is_typed() {
        for (spelling, expected) in [
            ("RESTRICT", Db2OnDeleteAction::Restrict),
            ("NO ACTION", Db2OnDeleteAction::NoAction),
            ("CASCADE", Db2OnDeleteAction::Cascade),
            ("SET NULL", Db2OnDeleteAction::SetNull),
        ] {
            let source = format!(
                "CREATE TABLE child (id INT, FOREIGN KEY (id) REFERENCES parent (id) ON DELETE {spelling})"
            );
            let statement = parse(&source).unwrap();
            assert_eq!(
                foreign(&statement.constraints()[0]).on_delete(),
                Some(expected)
            );
        }
        let statement =
            parse("CREATE TABLE child (id INT, FOREIGN KEY (id) REFERENCES parent (id))").unwrap();
        assert_eq!(foreign(&statement.constraints()[0]).on_delete(), None);
    }

    #[test]
    fn unsupported_column_and_table_forms_fail_explicitly() {
        for source in [
            "CREATE TABLE t (a INT GENERATED ALWAYS AS IDENTITY)",
            "CREATE TABLE t (a CHAR(8) NOT NULL DEFAULT 'x' AS SECURITY LABEL)",
            "CREATE TABLE t (a INT IMPLICITLY HIDDEN)",
            "CREATE TABLE t (a INT FIELDPROC p)",
            "CREATE TABLE t (a INT INLINE LENGTH 4)",
            "CREATE TABLE t (a INT PRIMARY KEY)",
            "CREATE TABLE t (a INT UNIQUE)",
            "CREATE TABLE t (a INT REFERENCES parent(id))",
            "CREATE TABLE t (a INT, CHECK (a > 0))",
            "CREATE TABLE t (a DATE, b DATE, PERIOD FOR BUSINESS_TIME (a,b))",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "{source}"
            );
        }
        for source in [
            "CREATE TABLE t LIKE source",
            "CREATE TABLE t (a INT) AS (SELECT 1) WITH NO DATA",
            "CREATE TABLE t (a INT) MATERIALIZED QUERY",
            "CREATE TABLE t (a INT) IN db.ts",
            "CREATE TABLE t (a INT) PARTITION BY SIZE",
            "CREATE TABLE t (a INT) ORGANIZE BY HASH a",
            "CREATE TABLE t (a INT) COMPRESS YES",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "{source}"
            );
        }
    }

    #[test]
    fn duplicate_columns_options_names_and_keys_are_rejected() {
        for source in [
            "CREATE TABLE t (a INT, A INT)",
            "CREATE TABLE t (a INT NOT NULL NOT NULL)",
            "CREATE TABLE t (a INT DEFAULT 1 WITH DEFAULT 2)",
            "CREATE TABLE t (a INT NOT NULL, CONSTRAINT p PRIMARY KEY(a), CONSTRAINT p UNIQUE(a))",
            "CREATE TABLE t (a INT NOT NULL, PRIMARY KEY(a), PRIMARY KEY(a))",
            "CREATE TABLE t (a INT NOT NULL, UNIQUE(a), UNIQUE(a))",
            "CREATE TABLE t (a INT, UNIQUE(a,a))",
            "CREATE TABLE t (a INT, FOREIGN KEY(a) REFERENCES p(id), FOREIGN KEY(a) REFERENCES p(other))",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "{source}"
            );
        }
    }

    #[test]
    fn semantic_conflicts_visible_inside_the_definition_are_rejected() {
        for source in [
            "CREATE TABLE t (a INT NOT NULL DEFAULT NULL)",
            "CREATE TABLE t (a INT DEFAULT NULL NOT NULL)",
            "CREATE TABLE t (a INT, PRIMARY KEY(a))",
            "CREATE TABLE t (a INT, UNIQUE(a))",
            "CREATE TABLE t (a INT, FOREIGN KEY(missing) REFERENCES p(id))",
            "CREATE TABLE t (a INT, FOREIGN KEY(a) REFERENCES p(id, other))",
            "CREATE TABLE t (a INT NOT NULL, FOREIGN KEY(a) REFERENCES p(id) ON DELETE SET NULL)",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{source}"
            );
        }
    }

    #[test]
    fn malformed_and_missing_operands_fail_closed() {
        for source in [
            "CREATE",
            "CREATE TABLE",
            "CREATE TABLE t",
            "CREATE TABLE t ()",
            "CREATE TABLE t (,a INT)",
            "CREATE TABLE t (a INT,)",
            "CREATE TABLE t (a)",
            "CREATE TABLE t (a INT b INT)",
            "CREATE TABLE t (a INT DEFAULT)",
            "CREATE TABLE t (a INT WITH DEFAULT)",
            "CREATE TABLE t (a INT DEFAULT CURRENT_DATE)",
            "CREATE TABLE t (a INT, CONSTRAINT c)",
            "CREATE TABLE t (a INT, PRIMARY (a))",
            "CREATE TABLE t (a INT, PRIMARY KEY ())",
            "CREATE TABLE t (a INT, FOREIGN KEY (a))",
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES)",
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ())",
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ON DELETE)",
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ON DELETE SET DEFAULT)",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn extra_and_multiple_statements_are_never_accepted() {
        assert_eq!(
            parse("CREATE TABLE t (a INT); CREATE TABLE u (b INT)")
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::UnexpectedToken
        );
        assert_eq!(
            parse("CREATE TABLE t (a INT);;").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnexpectedToken
        );
        assert_eq!(
            parse("SELECT 1").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement
        );
        assert_eq!(
            parse("CREATE VIEW v AS SELECT 1").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement
        );
    }

    #[test]
    fn unicode_identifiers_and_multiline_spans_are_preserved() {
        let statement =
            parse("CREATE TABLE \"模式\".\"表\" (\n  \"列\" VARCHAR(4) NOT NULL\n);").unwrap();
        assert_eq!(statement.table_name().parts()[0].value(), "模式");
        assert_eq!(statement.table_name().parts()[1].value(), "表");
        assert_eq!(statement.span().start, Db2SourceLocation::START);
        assert_eq!(
            statement.columns()[0].span().start,
            Db2SourceLocation { line: 2, column: 3 }
        );
        assert_eq!(statement.columns()[0].span().end.line, 2);
        assert_eq!(statement.span().end.line, 3);
    }

    #[test]
    fn identifier_element_key_and_literal_limits_are_enforced() {
        let syntax = Db2SyntaxLimits::default();

        let mut limits = Db2AstLimits::default();
        limits.max_name_parts = 1;
        assert_eq!(
            parse_db2_create_table_statement("CREATE TABLE s.t (a INT)", syntax, limits)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );

        let mut limits = Db2AstLimits::default();
        limits.max_list_items = 2;
        for source in [
            "CREATE TABLE t (a INT, b INT, c INT)",
            "CREATE TABLE t (a INT NOT NULL, b INT NOT NULL, c INT NOT NULL, UNIQUE(a,b,c))",
        ] {
            assert_eq!(
                parse_db2_create_table_statement(source, syntax, limits)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{source}"
            );
        }

        let mut limits = Db2AstLimits::default();
        limits.max_identifier_bytes = 3;
        assert_eq!(
            parse_db2_create_table_statement("CREATE TABLE long (a INT)", syntax, limits)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );

        let mut limits = Db2AstLimits::default();
        limits.max_literal_bytes = 3;
        assert_eq!(
            parse_db2_create_table_statement(
                "CREATE TABLE t (a VARCHAR(10) DEFAULT 'four')",
                syntax,
                limits,
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
    }

    #[test]
    fn lexer_resource_bounds_are_propagated() {
        let mut syntax = Db2SyntaxLimits::default();
        syntax.max_statement_bytes = 12;
        assert_eq!(
            parse_db2_create_table_statement(
                "CREATE TABLE t (a INT)",
                syntax,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );

        let mut syntax = Db2SyntaxLimits::default();
        syntax.max_tokens = 4;
        assert_eq!(
            parse_db2_create_table_statement(
                "CREATE TABLE t (a INT)",
                syntax,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );
    }
}
