//! Owned Db2 statement AST and source-reviewed family parsers.

mod dynamic;

pub use dynamic::{
    Db2DescriptorNameMode, Db2ExecuteImmediateStatement, Db2ExecuteStatement, Db2ExecuteUsing,
    Db2PrepareDescriptor, Db2PrepareStatement, parse_db2_dynamic_statement,
};

use crate::{
    Db2AstLimits, Db2HostIdentifier, Db2HostReference, Db2Identifier, Db2SourceLocation,
    Db2SourceSpan, Db2StatementId, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2,
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

/// Parse one transaction-control statement. This source-reviewed family parser
/// intentionally rejects every other statement instead of returning a raw or
/// generic-success node.
pub fn parse_db2_transaction_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2Statement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    let mut parser = StatementParser::new(lexed.tokens(), ast_limits);
    let first = parser.word_at(0).map(str::to_owned).ok_or_else(|| {
        parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "Db2 transaction statement must begin with COMMIT, ROLLBACK, or SAVEPOINT",
        )
    })?;
    let (id, kind) = match first.as_str() {
        "COMMIT" => (
            Db2StatementId::SqlCommit,
            Db2StatementKind::Commit(parser.parse_commit()?),
        ),
        "ROLLBACK" => (
            Db2StatementId::SqlRollback,
            Db2StatementKind::Rollback(parser.parse_rollback()?),
        ),
        "SAVEPOINT" => (
            Db2StatementId::SqlSavepoint,
            Db2StatementKind::Savepoint(parser.parse_savepoint()?),
        ),
        _ => {
            return Err(parser.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the Db2 transaction syntax family",
            ));
        }
    };
    parser.finish()?;
    let span = parser.statement_span();
    Ok(Db2Statement { id, kind, span })
}

struct StatementParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    ast_limits: Db2AstLimits,
}

impl<'a> StatementParser<'a> {
    fn new(tokens: &'a [Db2Token], ast_limits: Db2AstLimits) -> Self {
        Self {
            tokens,
            position: 0,
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
        let mut unique = false;
        let mut retain_cursors = false;
        let mut retain_locks = false;
        while !self.at_end_or_semicolon() {
            if self.take_word("UNIQUE") {
                if unique {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::DuplicateClause,
                        "SAVEPOINT UNIQUE is specified more than once",
                    ));
                }
                unique = true;
                continue;
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
        Ok(Db2SavepointStatement {
            name,
            unique,
            retain_cursors,
            retain_locks,
        })
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
        if self.word_at(self.position) == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
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
            Db2Identifier::new(value.clone(), *delimited, self.ast_limits).map_err(|problem| {
                Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    token.span.start,
                    problem.message,
                )
            })?;
        self.position += 1;
        Ok(identifier)
    }

    fn host_identifier(&mut self, label: &str) -> Result<Db2HostIdentifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.tokens.get(self.position) else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("missing Db2 {label}"),
            ));
        };
        let Db2TokenKind::HostVariable(value) = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                format!("Db2 {label} must be a host identifier preceded by colon"),
            ));
        };
        let identifier =
            Db2HostIdentifier::new(value.clone(), self.ast_limits).map_err(|problem| {
                Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    token.span.start,
                    problem.message,
                )
            })?;
        self.position += 1;
        Ok(identifier)
    }

    fn host_reference(&mut self, label: &str) -> Result<Db2HostReference, Db2SyntaxDiagnostic> {
        let variable = self.host_identifier(label)?;
        let indicator = if self.take_word("INDICATOR") {
            Some(self.host_identifier("indicator variable")?)
        } else if matches!(
            self.tokens.get(self.position).map(|token| &token.kind),
            Some(Db2TokenKind::HostVariable(_))
        ) {
            Some(self.host_identifier("indicator variable")?)
        } else {
            None
        };
        Ok(Db2HostReference::new(variable, indicator))
    }

    fn finish(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(Db2Symbol::Semicolon) && !self.at_end() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "only one Db2 statement is allowed",
            ));
        }
        if !self.at_end() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "unexpected token after Db2 transaction statement",
            ));
        }
        Ok(())
    }

    fn take_symbol(&mut self, expected: Db2Symbol) -> bool {
        if matches!(
            self.tokens.get(self.position).map(|token| &token.kind),
            Some(Db2TokenKind::Symbol(symbol)) if *symbol == expected
        ) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn at_end_or_semicolon(&self) -> bool {
        self.at_end()
            || matches!(
                self.tokens.get(self.position).map(|token| &token.kind),
                Some(Db2TokenKind::Symbol(Db2Symbol::Semicolon))
            )
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
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
        let location = self.tokens.get(self.position).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2Statement, Db2SyntaxDiagnostic> {
        parse_db2_transaction_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    #[test]
    fn commit_forms_are_exact() {
        for (source, work) in [("COMMIT", false), ("commit work;", true)] {
            let statement = parse(source).unwrap();
            assert_eq!(statement.id(), Db2StatementId::SqlCommit);
            let Db2StatementKind::Commit(commit) = statement.kind() else {
                panic!("expected COMMIT AST")
            };
            assert_eq!(commit.has_work_keyword(), work);
        }
        assert_eq!(
            parse("COMMIT WORK WORK").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnexpectedToken
        );
        assert_eq!(
            parse("COMMIT WORK WORK").unwrap_err().location,
            Db2SourceLocation {
                line: 1,
                column: 13
            }
        );
    }

    #[test]
    fn rollback_unit_and_savepoint_forms_are_exact() {
        for source in ["ROLLBACK", "ROLLBACK WORK"] {
            let statement = parse(source).unwrap();
            assert_eq!(statement.id(), Db2StatementId::SqlRollback);
            let Db2StatementKind::Rollback(rollback) = statement.kind() else {
                panic!("expected ROLLBACK AST")
            };
            assert_eq!(rollback.has_work_keyword(), source.ends_with("WORK"));
            assert_eq!(rollback.target(), &Db2RollbackTarget::UnitOfWork);
        }
        let unnamed = parse("ROLLBACK TO SAVEPOINT").unwrap();
        let Db2StatementKind::Rollback(rollback) = unnamed.kind() else {
            panic!("expected ROLLBACK AST")
        };
        assert_eq!(rollback.target(), &Db2RollbackTarget::Savepoint(None));
        let named = parse("ROLLBACK WORK TO SAVEPOINT S1").unwrap();
        let Db2StatementKind::Rollback(rollback) = named.kind() else {
            panic!("expected ROLLBACK AST")
        };
        let Db2RollbackTarget::Savepoint(Some(name)) = rollback.target() else {
            panic!("expected named savepoint")
        };
        assert_eq!(name.value(), "S1");
        assert_eq!(
            parse("ROLLBACK TO OTHER").unwrap_err().code,
            Db2SyntaxDiagnosticCode::MissingToken
        );
    }

    #[test]
    fn savepoint_clauses_accept_either_order_and_reject_duplicates() {
        for source in [
            "SAVEPOINT S1 UNIQUE ON ROLLBACK RETAIN CURSORS ON ROLLBACK RETAIN LOCKS",
            "SAVEPOINT S1 ON ROLLBACK RETAIN LOCKS ON ROLLBACK RETAIN CURSORS UNIQUE",
        ] {
            let statement = parse(source).unwrap();
            assert_eq!(statement.id(), Db2StatementId::SqlSavepoint);
            let Db2StatementKind::Savepoint(savepoint) = statement.kind() else {
                panic!("expected SAVEPOINT AST")
            };
            assert_eq!(savepoint.name().value(), "S1");
            assert!(savepoint.is_unique());
            assert!(savepoint.retains_cursors());
            assert!(savepoint.retains_locks());
        }
        for source in [
            "SAVEPOINT S1 UNIQUE UNIQUE",
            "SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS ON ROLLBACK RETAIN CURSORS",
            "SAVEPOINT S1 ON ROLLBACK RETAIN LOCKS ON ROLLBACK RETAIN LOCKS",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::DuplicateClause
            );
        }
    }

    #[test]
    fn transaction_syntax_rejects_invalid_names_clauses_and_multiple_statements() {
        assert_eq!(
            parse("SAVEPOINT SYSPOINT").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        assert_eq!(
            parse("SAVEPOINT S1 ON ROLLBACK RETAIN ROWS")
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::MissingToken
        );
        assert_eq!(
            parse("SAVEPOINT").unwrap_err().code,
            Db2SyntaxDiagnosticCode::MissingToken
        );
        assert_eq!(
            parse("COMMIT; ROLLBACK").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnexpectedToken
        );
        assert_eq!(
            parse("SELECT 1").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement
        );
    }
}
