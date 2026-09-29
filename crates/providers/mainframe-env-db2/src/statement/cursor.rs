//! Owned syntax for the common prepared DECLARE CURSOR slice.

use super::{Db2Statement, Db2StatementKind};
use crate::{
    Db2AstLimits, Db2Identifier, Db2SourceLocation, Db2SourceSpan, Db2StatementId, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2,
};

const RETURNING_CURSOR_NAME_BYTES: usize = 30;

/// Scrollability and, when scrollable, the explicitly requested sensitivity.
/// Omitted keywords remain distinct from their effective defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorOrientation {
    DefaultNoScroll,
    NoScroll,
    Scroll(Db2CursorSensitivity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorSensitivity {
    DefaultAsensitive,
    Asensitive,
    Insensitive,
    Sensitive(Db2SensitiveCursorKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SensitiveCursorKind {
    DefaultDynamic,
    Dynamic,
    Static,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorHoldability {
    DefaultWithoutHold,
    WithoutHold,
    WithHold,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorReturnTarget {
    DefaultCaller,
    Caller,
    Client,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorReturnability {
    DefaultFromPreparedStatement,
    WithoutReturn,
    WithReturn(Db2CursorReturnTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CursorRowsetPositioning {
    DefaultWithoutRowsetPositioning,
    WithoutRowsetPositioning,
    WithRowsetPositioning,
}

/// A prepared-statement-name DECLARE CURSOR. Inline select syntax has no AST
/// representation in this deliberately partial family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DeclareCursorPreparedStatement {
    cursor_name: Db2Identifier,
    orientation: Db2CursorOrientation,
    holdability: Db2CursorHoldability,
    returnability: Db2CursorReturnability,
    rowset_positioning: Db2CursorRowsetPositioning,
    statement_name: Db2Identifier,
    span: Db2SourceSpan,
}

impl Db2DeclareCursorPreparedStatement {
    #[must_use]
    pub const fn cursor_name(&self) -> &Db2Identifier {
        &self.cursor_name
    }

    #[must_use]
    pub const fn orientation(&self) -> Db2CursorOrientation {
        self.orientation
    }

    #[must_use]
    pub const fn holdability(&self) -> Db2CursorHoldability {
        self.holdability
    }

    #[must_use]
    pub const fn returnability(&self) -> Db2CursorReturnability {
        self.returnability
    }

    #[must_use]
    pub const fn rowset_positioning(&self) -> Db2CursorRowsetPositioning {
        self.rowset_positioning
    }

    #[must_use]
    pub const fn statement_name(&self) -> &Db2Identifier {
        &self.statement_name
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one DECLARE CURSOR whose source is a prepared statement name.
/// Other DECLARE families, inline SELECT, and trailing statements fail closed.
pub fn parse_db2_declare_cursor_prepared(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2DeclareCursorPreparedStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let mut parser = CursorParser::new(lexed.tokens(), ast_limits);
    parser.expect_word("DECLARE")?;
    if parser.at_end_or_semicolon() || parser.cursor_name_is_missing() {
        return Err(parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::MissingToken,
            "missing Db2 cursor name",
        ));
    }
    let cursor_name = parser.identifier("cursor name")?;
    let orientation = parser.orientation()?;
    parser.expect_word("CURSOR")?;

    let mut holdability = None;
    let mut returnability = None;
    let mut rowset_positioning = None;
    while !parser.at_end_or_semicolon() && parser.word_at(parser.position) != Some("FOR") {
        if parser.take_word("WITH") {
            parser.with_modifier(
                &mut holdability,
                &mut returnability,
                &mut rowset_positioning,
            )?;
        } else if parser.take_word("WITHOUT") {
            parser.without_modifier(
                &mut holdability,
                &mut returnability,
                &mut rowset_positioning,
            )?;
        } else {
            return Err(parser.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "expected WITH, WITHOUT, or FOR after DECLARE CURSOR",
            ));
        }
    }

    parser.expect_word("FOR")?;
    if parser
        .word_at(parser.position)
        .is_some_and(|word| matches!(word, "SELECT" | "WITH" | "VALUES" | "TABLE"))
    {
        return Err(parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            "inline query syntax is pending the Db2 SELECT slice",
        ));
    }
    let statement_name = parser.identifier("prepared statement name")?;
    parser.finish()?;

    let returnability =
        returnability.unwrap_or(Db2CursorReturnability::DefaultFromPreparedStatement);
    if matches!(returnability, Db2CursorReturnability::WithReturn(_))
        && cursor_name.value().len() > RETURNING_CURSOR_NAME_BYTES
    {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            parser.tokens[1].span.start,
            "WITH RETURN cursor name exceeds the 30-byte slice limit",
        ));
    }

    Ok(Db2DeclareCursorPreparedStatement {
        cursor_name,
        orientation,
        holdability: holdability.unwrap_or(Db2CursorHoldability::DefaultWithoutHold),
        returnability,
        rowset_positioning: rowset_positioning
            .unwrap_or(Db2CursorRowsetPositioning::DefaultWithoutRowsetPositioning),
        statement_name,
        span: parser.statement_span(),
    })
}

/// Parse the prepared DECLARE CURSOR form as a Db2 statement node.
pub fn parse_db2_cursor_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2Statement, Db2SyntaxDiagnostic> {
    let cursor = parse_db2_declare_cursor_prepared(source, syntax_limits, ast_limits)?;
    Ok(Db2Statement {
        id: Db2StatementId::SqlDeclareCursor,
        span: cursor.span(),
        kind: Db2StatementKind::DeclareCursorPrepared(cursor),
    })
}

struct CursorParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    ast_limits: Db2AstLimits,
}

impl<'a> CursorParser<'a> {
    fn new(tokens: &'a [Db2Token], ast_limits: Db2AstLimits) -> Self {
        Self {
            tokens,
            position: 0,
            ast_limits,
        }
    }

    fn orientation(&mut self) -> Result<Db2CursorOrientation, Db2SyntaxDiagnostic> {
        let orientation = if self.take_word("NO") {
            self.expect_word("SCROLL")?;
            Db2CursorOrientation::NoScroll
        } else if self.take_word("SCROLL") {
            Db2CursorOrientation::Scroll(Db2CursorSensitivity::DefaultAsensitive)
        } else if self.take_word("ASENSITIVE") {
            self.expect_word("SCROLL")?;
            Db2CursorOrientation::Scroll(Db2CursorSensitivity::Asensitive)
        } else if self.take_word("INSENSITIVE") {
            self.expect_word("SCROLL")?;
            Db2CursorOrientation::Scroll(Db2CursorSensitivity::Insensitive)
        } else if self.take_word("SENSITIVE") {
            let kind = if self.take_word("DYNAMIC") {
                Db2SensitiveCursorKind::Dynamic
            } else if self.take_word("STATIC") {
                Db2SensitiveCursorKind::Static
            } else {
                Db2SensitiveCursorKind::DefaultDynamic
            };
            self.expect_word("SCROLL")?;
            Db2CursorOrientation::Scroll(Db2CursorSensitivity::Sensitive(kind))
        } else {
            Db2CursorOrientation::DefaultNoScroll
        };
        if self.word_at(self.position).is_some_and(is_orientation_word) {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "duplicate or conflicting DECLARE CURSOR scroll/sensitivity clause",
            ));
        }
        Ok(orientation)
    }

    fn with_modifier(
        &mut self,
        holdability: &mut Option<Db2CursorHoldability>,
        returnability: &mut Option<Db2CursorReturnability>,
        rowset_positioning: &mut Option<Db2CursorRowsetPositioning>,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word("HOLD") {
            Self::set_once(
                holdability,
                Db2CursorHoldability::WithHold,
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR holdability is specified more than once",
                ),
            )
        } else if self.take_word("RETURN") {
            let target = if self.take_word("TO") {
                if self.take_word("CALLER") {
                    Db2CursorReturnTarget::Caller
                } else if self.take_word("CLIENT") {
                    Db2CursorReturnTarget::Client
                } else {
                    return Err(self.diagnostic_here(
                        Db2SyntaxDiagnosticCode::MissingToken,
                        "WITH RETURN TO requires CALLER or CLIENT",
                    ));
                }
            } else {
                Db2CursorReturnTarget::DefaultCaller
            };
            Self::set_once(
                returnability,
                Db2CursorReturnability::WithReturn(target),
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR returnability is specified more than once",
                ),
            )
        } else if self.take_word("ROWSET") {
            self.expect_word("POSITIONING")?;
            Self::set_once(
                rowset_positioning,
                Db2CursorRowsetPositioning::WithRowsetPositioning,
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR rowset positioning is specified more than once",
                ),
            )
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "WITH requires HOLD, RETURN, or ROWSET POSITIONING",
            ))
        }
    }

    fn without_modifier(
        &mut self,
        holdability: &mut Option<Db2CursorHoldability>,
        returnability: &mut Option<Db2CursorReturnability>,
        rowset_positioning: &mut Option<Db2CursorRowsetPositioning>,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word("HOLD") {
            Self::set_once(
                holdability,
                Db2CursorHoldability::WithoutHold,
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR holdability is specified more than once",
                ),
            )
        } else if self.take_word("RETURN") {
            if self.word_at(self.position) == Some("TO") {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "WITHOUT RETURN cannot specify a return target",
                ));
            }
            Self::set_once(
                returnability,
                Db2CursorReturnability::WithoutReturn,
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR returnability is specified more than once",
                ),
            )
        } else if self.take_word("ROWSET") {
            self.expect_word("POSITIONING")?;
            Self::set_once(
                rowset_positioning,
                Db2CursorRowsetPositioning::WithoutRowsetPositioning,
                self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "DECLARE CURSOR rowset positioning is specified more than once",
                ),
            )
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "WITHOUT requires HOLD, RETURN, or ROWSET POSITIONING",
            ))
        }
    }

    fn set_once<T>(
        slot: &mut Option<T>,
        value: T,
        duplicate: Db2SyntaxDiagnostic,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        if slot.is_some() {
            Err(duplicate)
        } else {
            *slot = Some(value);
            Ok(())
        }
    }

    fn cursor_name_is_missing(&self) -> bool {
        self.word_at(self.position)
            .is_some_and(|word| word == "CURSOR" || word == "NO" || is_orientation_word(word))
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
                    &problem.message,
                )
            })?;
        self.position += 1;
        Ok(identifier)
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
                "unexpected token after prepared DECLARE CURSOR",
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
        let location = self.tokens.get(self.position).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message.as_ref())
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

fn is_orientation_word(word: &str) -> bool {
    matches!(
        word,
        "NO" | "SCROLL" | "ASENSITIVE" | "INSENSITIVE" | "SENSITIVE" | "DYNAMIC" | "STATIC"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2DeclareCursorPreparedStatement, Db2SyntaxDiagnostic> {
        parse_db2_declare_cursor_prepared(
            source,
            Db2SyntaxLimits::default(),
            Db2AstLimits::default(),
        )
    }

    #[test]
    fn minimal_prepared_form_preserves_all_defaults() {
        let statement = parse("declare cursor_one cursor for statement_one").unwrap();
        assert_eq!(statement.cursor_name().value(), "CURSOR_ONE");
        assert_eq!(
            statement.orientation(),
            Db2CursorOrientation::DefaultNoScroll
        );
        assert_eq!(
            statement.holdability(),
            Db2CursorHoldability::DefaultWithoutHold
        );
        assert_eq!(
            statement.returnability(),
            Db2CursorReturnability::DefaultFromPreparedStatement
        );
        assert_eq!(
            statement.rowset_positioning(),
            Db2CursorRowsetPositioning::DefaultWithoutRowsetPositioning
        );
        assert_eq!(statement.statement_name().value(), "STATEMENT_ONE");
        assert_eq!(statement.span().start, Db2SourceLocation::START);
    }

    #[test]
    fn scroll_and_sensitivity_forms_are_typed_without_losing_defaults() {
        let cases = [
            (
                "DECLARE C NO SCROLL CURSOR FOR S",
                Db2CursorOrientation::NoScroll,
            ),
            (
                "DECLARE C SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::DefaultAsensitive),
            ),
            (
                "DECLARE C ASENSITIVE SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::Asensitive),
            ),
            (
                "DECLARE C INSENSITIVE SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::Insensitive),
            ),
            (
                "DECLARE C SENSITIVE SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::Sensitive(
                    Db2SensitiveCursorKind::DefaultDynamic,
                )),
            ),
            (
                "DECLARE C SENSITIVE DYNAMIC SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::Sensitive(
                    Db2SensitiveCursorKind::Dynamic,
                )),
            ),
            (
                "DECLARE C SENSITIVE STATIC SCROLL CURSOR FOR S",
                Db2CursorOrientation::Scroll(Db2CursorSensitivity::Sensitive(
                    Db2SensitiveCursorKind::Static,
                )),
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(parse(source).unwrap().orientation(), expected, "{source}");
        }
    }

    #[test]
    fn modifier_families_accept_every_source_defined_order() {
        let clauses = [
            "WITH HOLD WITH RETURN TO CLIENT WITH ROWSET POSITIONING",
            "WITH HOLD WITH ROWSET POSITIONING WITH RETURN TO CLIENT",
            "WITH RETURN TO CLIENT WITH HOLD WITH ROWSET POSITIONING",
            "WITH RETURN TO CLIENT WITH ROWSET POSITIONING WITH HOLD",
            "WITH ROWSET POSITIONING WITH HOLD WITH RETURN TO CLIENT",
            "WITH ROWSET POSITIONING WITH RETURN TO CLIENT WITH HOLD",
        ];
        for clause in clauses {
            let statement = parse(&format!("DECLARE C CURSOR {clause} FOR S")).unwrap();
            assert_eq!(statement.holdability(), Db2CursorHoldability::WithHold);
            assert_eq!(
                statement.returnability(),
                Db2CursorReturnability::WithReturn(Db2CursorReturnTarget::Client)
            );
            assert_eq!(
                statement.rowset_positioning(),
                Db2CursorRowsetPositioning::WithRowsetPositioning
            );
        }
    }

    #[test]
    fn explicit_negative_and_default_return_target_forms_are_preserved() {
        let negative =
            parse("DECLARE C CURSOR WITHOUT RETURN WITHOUT ROWSET POSITIONING WITHOUT HOLD FOR S")
                .unwrap();
        assert_eq!(negative.holdability(), Db2CursorHoldability::WithoutHold);
        assert_eq!(
            negative.returnability(),
            Db2CursorReturnability::WithoutReturn
        );
        assert_eq!(
            negative.rowset_positioning(),
            Db2CursorRowsetPositioning::WithoutRowsetPositioning
        );

        let implicit_caller = parse("DECLARE C CURSOR WITH RETURN FOR S").unwrap();
        assert_eq!(
            implicit_caller.returnability(),
            Db2CursorReturnability::WithReturn(Db2CursorReturnTarget::DefaultCaller)
        );
        let caller = parse("DECLARE C CURSOR WITH RETURN TO CALLER FOR S").unwrap();
        assert_eq!(
            caller.returnability(),
            Db2CursorReturnability::WithReturn(Db2CursorReturnTarget::Caller)
        );
    }

    #[test]
    fn duplicate_and_conflicting_modifier_clauses_fail_closed() {
        for source in [
            "DECLARE C CURSOR WITH HOLD WITH HOLD FOR S",
            "DECLARE C CURSOR WITH HOLD WITHOUT HOLD FOR S",
            "DECLARE C CURSOR WITH RETURN WITHOUT RETURN FOR S",
            "DECLARE C CURSOR WITH RETURN TO CALLER WITH RETURN TO CLIENT FOR S",
            "DECLARE C CURSOR WITH ROWSET POSITIONING WITHOUT ROWSET POSITIONING FOR S",
            "DECLARE C CURSOR WITHOUT ROWSET POSITIONING WITHOUT ROWSET POSITIONING FOR S",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "{source}"
            );
        }
    }

    #[test]
    fn illegal_scroll_and_sensitivity_combinations_are_rejected() {
        for source in [
            "DECLARE C NO SCROLL SCROLL CURSOR FOR S",
            "DECLARE C SCROLL ASENSITIVE CURSOR FOR S",
            "DECLARE C ASENSITIVE NO SCROLL CURSOR FOR S",
            "DECLARE C INSENSITIVE CURSOR FOR S",
            "DECLARE C DYNAMIC SCROLL CURSOR FOR S",
            "DECLARE C SENSITIVE DYNAMIC STATIC SCROLL CURSOR FOR S",
            "DECLARE C SENSITIVE STATIC NO SCROLL CURSOR FOR S",
        ] {
            assert!(parse(source).is_err(), "unexpected success: {source}");
        }
    }

    #[test]
    fn missing_and_malformed_operands_are_rejected() {
        for source in [
            "DECLARE",
            "DECLARE CURSOR FOR S",
            "DECLARE C",
            "DECLARE C NO CURSOR FOR S",
            "DECLARE C CURSOR",
            "DECLARE C CURSOR WITH FOR S",
            "DECLARE C CURSOR WITHOUT FOR S",
            "DECLARE C CURSOR WITH ROWSET FOR S",
            "DECLARE C CURSOR WITHOUT ROWSET FOR S",
            "DECLARE C CURSOR WITH RETURN TO FOR S",
            "DECLARE C CURSOR WITH RETURN TO SERVER FOR S",
            "DECLARE C CURSOR WITHOUT RETURN TO CALLER FOR S",
            "DECLARE C CURSOR FOR",
            "DECLARE C CURSOR FOR 7",
        ] {
            assert!(parse(source).is_err(), "unexpected success: {source}");
        }
    }

    #[test]
    fn inline_queries_extra_tokens_and_multiple_statements_are_rejected() {
        for source in [
            "DECLARE C CURSOR FOR SELECT C1 FROM T",
            "DECLARE C CURSOR FOR WITH Q AS (SELECT C1 FROM T) SELECT C1 FROM Q",
            "DECLARE C CURSOR FOR VALUES",
            "DECLARE C CURSOR FOR TABLE",
            "DECLARE C CURSOR FOR S EXTRA",
            "DECLARE C CURSOR FOR S; DECLARE D CURSOR FOR T",
            "DECLARE C CURSOR FOR S;;",
            "SELECT C1 FROM T",
        ] {
            assert!(parse(source).is_err(), "unexpected success: {source}");
        }
        assert_eq!(
            parse("SELECT C1 FROM T").unwrap_err().code,
            Db2SyntaxDiagnosticCode::MissingToken
        );
    }

    #[test]
    fn rejected_forms_report_the_offending_location() {
        let duplicate = parse("DECLARE C CURSOR WITH HOLD\nWITH HOLD FOR S").unwrap_err();
        assert_eq!(duplicate.code, Db2SyntaxDiagnosticCode::DuplicateClause);
        assert_eq!(duplicate.location, Db2SourceLocation { line: 2, column: 6 });

        let misplaced = parse("DECLARE C CURSOR FOR S WITH HOLD").unwrap_err();
        assert_eq!(misplaced.code, Db2SyntaxDiagnosticCode::UnexpectedToken);
        assert_eq!(
            misplaced.location,
            Db2SourceLocation {
                line: 1,
                column: 24
            }
        );

        let inline = parse("DECLARE C CURSOR FOR SELECT C1 FROM T").unwrap_err();
        assert_eq!(
            inline.code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        assert_eq!(
            inline.location,
            Db2SourceLocation {
                line: 1,
                column: 22
            }
        );
        assert!(inline.message.contains("SELECT slice"));
    }

    #[test]
    fn delimited_unicode_names_and_multiline_span_are_owned() {
        let statement = parse(
            "DECLARE \"游標😀\" SENSITIVE STATIC SCROLL CURSOR\n  WITH RETURN TO CLIENT FOR \"準備😀\";",
        )
        .unwrap();
        assert_eq!(statement.cursor_name().value(), "游標😀");
        assert!(statement.cursor_name().is_delimited());
        assert_eq!(statement.statement_name().value(), "準備😀");
        assert!(statement.statement_name().is_delimited());
        assert_eq!(statement.span().start, Db2SourceLocation::START);
        assert_eq!(statement.span().end.line, 2);
        assert!(statement.span().end.column > 1);

        // D2's pinned lexer accepts Unicode in delimited identifiers only.
        assert_eq!(
            parse("DECLARE 游標 CURSOR FOR 準備").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidCharacter
        );
    }

    #[test]
    fn returning_cursor_name_uses_the_source_specific_bound() {
        assert!(
            parse(&format!(
                "DECLARE {} CURSOR WITH RETURN FOR S",
                "C".repeat(30)
            ))
            .is_ok()
        );
        let long_name = "C".repeat(31);
        assert_eq!(
            parse(&format!("DECLARE {long_name} CURSOR WITH RETURN FOR S"))
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        assert!(parse(&format!("DECLARE {long_name} CURSOR WITHOUT RETURN FOR S")).is_ok());
        assert_eq!(
            parse(&format!(
                "DECLARE \"{}\" CURSOR WITH RETURN FOR S",
                "游".repeat(11)
            ))
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
    }

    #[test]
    fn statement_surface_keeps_catalog_identity_and_byte_span() {
        let source = "DECLARE C CURSOR FOR S;";
        let statement =
            parse_db2_cursor_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
                .unwrap();
        assert_eq!(statement.id(), Db2StatementId::SqlDeclareCursor);
        assert_eq!(statement.span().start_byte, 0);
        assert_eq!(statement.span().end_byte, source.len());
        assert!(matches!(
            statement.kind(),
            Db2StatementKind::DeclareCursorPrepared(_)
        ));
    }

    #[test]
    fn invalid_ast_limits_fail_before_parsing() {
        let invalid = Db2AstLimits {
            max_identifier_bytes: 0,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(
                "DECLARE C CURSOR FOR S",
                Db2SyntaxLimits::default(),
                invalid,
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
    }

    #[test]
    fn lexical_and_ast_resource_limits_fail_before_ast_publication() {
        let source = "DECLARE CURSOR_ONE CURSOR FOR STATEMENT_ONE";
        let too_small_statement = Db2SyntaxLimits {
            max_statement_bytes: source.len() - 1,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(source, too_small_statement, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );

        let too_few_tokens = Db2SyntaxLimits {
            max_tokens: 4,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(source, too_few_tokens, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );

        let shallow_nesting = Db2SyntaxLimits {
            max_nesting: 1,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(
                "/* outer /* inner */ */ DECLARE C CURSOR FOR S",
                shallow_nesting,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );

        let tiny_token = Db2SyntaxLimits {
            max_token_bytes: 6,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(source, tiny_token, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::TokenTooLarge
        );

        let tiny_identifier = Db2AstLimits {
            max_identifier_bytes: 4,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            parse_db2_declare_cursor_prepared(source, Db2SyntaxLimits::default(), tiny_identifier)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
    }
}
