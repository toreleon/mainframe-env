//! Owned transaction-statement syntax; parsing has no execution side effects.

mod cursor;
mod dynamic;

pub use cursor::{
    Db2CursorHoldability, Db2CursorOrientation, Db2CursorReturnTarget, Db2CursorReturnability,
    Db2CursorRowsetPositioning, Db2CursorSensitivity, Db2DeclareCursorPreparedStatement,
    Db2SensitiveCursorKind, parse_db2_cursor_statement, parse_db2_declare_cursor_prepared,
};

pub use dynamic::{
    Db2DescriptorNameMode, Db2ExecuteImmediateStatement, Db2ExecuteStatement, Db2ExecuteUsing,
    Db2PrepareDescriptor, Db2PrepareStatement, parse_db2_dynamic_statement,
};

use crate::{
    Db2AstLimits, Db2HostIdentifier, Db2HostReference, Db2Identifier, Db2SourceLocation,
    Db2SourceSpan, Db2StatementId, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2Token, Db2TokenCursor, Db2TokenKind, lex_db2,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Statement {
    id: Db2StatementId,
    kind: Db2StatementKind,
    span: Db2SourceSpan,
}

impl Db2Statement {
    #[must_use]
    pub const fn id(&self) -> Db2StatementId {
        self.id
    }

    #[must_use]
    pub const fn kind(&self) -> &Db2StatementKind {
        &self.kind
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2StatementKind {
    DeclareCursorPrepared(Db2DeclareCursorPreparedStatement),
    Commit(Db2CommitStatement),
    Rollback(Db2RollbackStatement),
    Savepoint(Db2SavepointStatement),
    Prepare(Db2PrepareStatement),
    Execute(Db2ExecuteStatement),
    ExecuteImmediate(Db2ExecuteImmediateStatement),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2CommitStatement {
    work_keyword: bool,
}

impl Db2CommitStatement {
    #[must_use]
    pub const fn has_work_keyword(self) -> bool {
        self.work_keyword
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2RollbackTarget {
    UnitOfWork,
    Savepoint(Option<Db2Identifier>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2RollbackStatement {
    work_keyword: bool,
    target: Db2RollbackTarget,
}

impl Db2RollbackStatement {
    #[must_use]
    pub const fn has_work_keyword(&self) -> bool {
        self.work_keyword
    }

    #[must_use]
    pub const fn target(&self) -> &Db2RollbackTarget {
        &self.target
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SavepointStatement {
    name: Db2Identifier,
    unique: bool,
    retain_cursors: bool,
    retain_locks: bool,
}

impl Db2SavepointStatement {
    #[must_use]
    pub const fn name(&self) -> &Db2Identifier {
        &self.name
    }

    #[must_use]
    pub const fn is_unique(&self) -> bool {
        self.unique
    }

    #[must_use]
    pub const fn retains_cursors(&self) -> bool {
        self.retain_cursors
    }

    #[must_use]
    pub const fn retains_locks(&self) -> bool {
        self.retain_locks
    }
}

/// Parse exactly one source-reviewed COMMIT, ROLLBACK, or SAVEPOINT statement.
pub fn parse_db2_transaction_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2Statement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let mut parser = StatementParser::new(lexed.cursor(), ast_limits);
    let (id, kind) = if parser.peek_word("COMMIT") {
        (
            Db2StatementId::SqlCommit,
            Db2StatementKind::Commit(parser.parse_commit()?),
        )
    } else if parser.peek_word("ROLLBACK") {
        (
            Db2StatementId::SqlRollback,
            Db2StatementKind::Rollback(parser.parse_rollback()?),
        )
    } else if parser.peek_word("SAVEPOINT") {
        (
            Db2StatementId::SqlSavepoint,
            Db2StatementKind::Savepoint(parser.parse_savepoint()?),
        )
    } else {
        return Err(parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "statement is outside the Db2 transaction syntax family",
        ));
    };
    parser.finish()?;
    let first = &lexed.tokens()[0].span;
    let last = &lexed.tokens()[lexed.tokens().len() - 1].span;
    Ok(Db2Statement {
        id,
        kind,
        span: Db2SourceSpan {
            start_byte: first.start_byte,
            end_byte: last.end_byte,
            start: first.start,
            end: last.end,
        },
    })
}

struct StatementParser<'a> {
    cursor: Db2TokenCursor<'a>,
    previous: Option<&'a Db2Token>,
    ast_limits: Db2AstLimits,
}

impl<'a> StatementParser<'a> {
    fn new(cursor: Db2TokenCursor<'a>, ast_limits: Db2AstLimits) -> Self {
        Self {
            cursor,
            previous: None,
            ast_limits,
        }
    }

    fn parse_commit(&mut self) -> Result<Db2CommitStatement, Db2SyntaxDiagnostic> {
        self.expect_word("COMMIT")?;
        Ok(Db2CommitStatement {
            work_keyword: self.take_word("WORK"),
        })
    }

    fn parse_rollback(&mut self) -> Result<Db2RollbackStatement, Db2SyntaxDiagnostic> {
        self.expect_word("ROLLBACK")?;
        let work_keyword = self.take_word("WORK");
        let target = if self.take_word("TO") {
            self.expect_word("SAVEPOINT")?;
            let name = if self.at_end_or_semicolon() {
                None
            } else {
                Some(self.identifier("rollback savepoint name")?)
            };
            Db2RollbackTarget::Savepoint(name)
        } else {
            Db2RollbackTarget::UnitOfWork
        };
        Ok(Db2RollbackStatement {
            work_keyword,
            target,
        })
    }

    fn parse_savepoint(&mut self) -> Result<Db2SavepointStatement, Db2SyntaxDiagnostic> {
        self.expect_word("SAVEPOINT")?;
        let name = self.identifier("savepoint name")?;
        if name.value().to_ascii_uppercase().starts_with("SYS") {
            return Err(self.diagnostic_previous(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "Db2 savepoint name must not begin with SYS",
            ));
        }
        // db2z_sql_savepoint: UNIQUE may only follow the name; ON ROLLBACK
        // RETAIN CURSORS is required; RETAIN LOCKS is optional; the two ON
        // ROLLBACK clauses may appear in either order.
        let unique = self.take_word("UNIQUE");
        let mut retain_cursors = false;
        let mut retain_locks = false;
        while !self.at_end_or_semicolon() {
            if self.take_word("UNIQUE") {
                return Err(self.diagnostic_previous(
                    if unique {
                        Db2SyntaxDiagnosticCode::DuplicateClause
                    } else {
                        Db2SyntaxDiagnosticCode::UnexpectedToken
                    },
                    if unique {
                        "SAVEPOINT UNIQUE is specified more than once"
                    } else {
                        "SAVEPOINT UNIQUE must follow the savepoint name"
                    },
                ));
            }
            self.expect_word("ON")?;
            self.expect_word("ROLLBACK")?;
            self.expect_word("RETAIN")?;
            if self.take_word("CURSORS") {
                if retain_cursors {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        "SAVEPOINT RETAIN CURSORS is specified more than once",
                    ));
                }
                retain_cursors = true;
            } else if self.take_word("LOCKS") {
                if retain_locks {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        "SAVEPOINT RETAIN LOCKS is specified more than once",
                    ));
                }
                retain_locks = true;
            } else {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "SAVEPOINT RETAIN requires CURSORS or LOCKS",
                ));
            }
        }
        if !retain_cursors {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "SAVEPOINT requires ON ROLLBACK RETAIN CURSORS",
            ));
        }
        Ok(Db2SavepointStatement {
            name,
            unique,
            retain_cursors,
            retain_locks,
        })
    }

    fn identifier(&mut self, label: &str) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("missing Db2 {label}"),
            ));
        };
        let Db2TokenKind::Word { value, delimited } = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                &format!("Db2 {label} must be an identifier"),
            ));
        };
        let identifier =
            Db2Identifier::new(value.clone(), *delimited, self.ast_limits).map_err(|problem| {
                Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    token.span.start,
                    &problem.message,
                )
            })?;
        self.advance();
        Ok(identifier)
    }

    fn host_identifier(&mut self, label: &str) -> Result<Db2HostIdentifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("missing Db2 {label}"),
            ));
        };
        let Db2TokenKind::HostVariable(value) = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                &format!("Db2 {label} must be a colon-prefixed host identifier"),
            ));
        };
        let identifier =
            Db2HostIdentifier::new(value.clone(), self.ast_limits).map_err(|problem| {
                Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    token.span.start,
                    &problem.message,
                )
            })?;
        self.advance();
        Ok(identifier)
    }

    fn host_reference(&mut self, label: &str) -> Result<Db2HostReference, Db2SyntaxDiagnostic> {
        let variable = self.host_identifier(label)?;
        let indicator_keyword = self.take_word("INDICATOR");
        let indicator = if indicator_keyword
            || matches!(
                self.cursor.peek().map(|token| &token.kind),
                Some(Db2TokenKind::HostVariable(_))
            ) {
            Some(self.host_identifier("indicator variable")?)
        } else {
            None
        };
        Ok(Db2HostReference::new(variable, indicator))
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word(expected) {
            Ok(())
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("expected Db2 keyword {expected}"),
            ))
        }
    }

    fn peek_word(&self, expected: &str) -> bool {
        matches!(
            self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::Word { value, delimited: false }) if value == expected
        )
    }

    fn take_word(&mut self, expected: &str) -> bool {
        if self.peek_word(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn take_symbol(&mut self, expected: Db2Symbol) -> bool {
        if matches!(
            self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::Symbol(symbol)) if *symbol == expected
        ) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.previous = self.cursor.next();
    }

    fn at_end_or_semicolon(&self) -> bool {
        self.cursor.peek().is_none()
            || matches!(
                self.cursor.peek().map(|token| &token.kind),
                Some(Db2TokenKind::Symbol(Db2Symbol::Semicolon))
            )
    }

    fn finish(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(Db2Symbol::Semicolon) && self.cursor.peek().is_some() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "only one Db2 statement is allowed",
            ));
        }
        if self.cursor.peek().is_some() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "unexpected token after Db2 statement",
            ));
        }
        Ok(())
    }

    fn diagnostic_here(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        let location = self.cursor.peek().map_or_else(
            || {
                self.previous
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message)
    }

    fn diagnostic_previous(
        &self,
        code: Db2SyntaxDiagnosticCode,
        message: &str,
    ) -> Db2SyntaxDiagnostic {
        let location = self
            .previous
            .map_or(Db2SourceLocation::START, |token| token.span.start);
        Db2SyntaxDiagnostic::new(code, location, message)
    }
}
