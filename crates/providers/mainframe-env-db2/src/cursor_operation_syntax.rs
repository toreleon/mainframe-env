//! Bounded OPEN/static-host and single-row FETCH syntax (SQL 0100 / 0078).
//!
//! Source: ibm-db2-for-zos-13-2026-08-13, db2z_sql_open / db2z_sql_fetch,
//! refs2hostvars, sqlidentifiers and identifyingansqldainc. This private kernel
//! has no dispatcher, host binding or cursor state effects. Scalar versus
//! structure/array identity, counts, scrollability, SQLDA contents and SQLCA
//! require binding/runtime context. Explicit structure/array access and C pointer
//! descriptor notation are outside this subset. No expression parser is used.

use crate::{
    Db2AstLimits, Db2HostIdentifier, Db2HostReference, Db2Identifier, Db2SourceLocation,
    Db2SourceSpan, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    Db2Token, Db2TokenCursor, Db2TokenKind, lex_db2,
};
use std::collections::BTreeSet;

/// Omitted orientation has NEXT behavior, while retaining its source spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2FetchOrientation {
    Unspecified,
    Next,
    Prior,
    First,
    Last,
    Current,
}

impl Db2FetchOrientation {
    #[must_use]
    pub const fn effective(self) -> Self {
        match self {
            Self::Unspecified => Self::Next,
            explicit => explicit,
        }
    }
}

/// An owned host leaf with every component located in the original source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CursorHostReference {
    reference: Db2HostReference,
    span: Db2SourceSpan,
    variable_span: Db2SourceSpan,
    indicator_span: Option<Db2SourceSpan>,
    indicator_keyword_span: Option<Db2SourceSpan>,
}

impl Db2CursorHostReference {
    #[must_use]
    pub const fn reference(&self) -> &Db2HostReference {
        &self.reference
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
    #[must_use]
    pub const fn variable_span(&self) -> Db2SourceSpan {
        self.variable_span
    }
    #[must_use]
    pub const fn indicator_span(&self) -> Option<Db2SourceSpan> {
        self.indicator_span
    }
    #[must_use]
    pub const fn indicator_keyword_span(&self) -> Option<Db2SourceSpan> {
        self.indicator_keyword_span
    }
}

/// USING/INTO operands. SQLDA uses a colon-prefixed, non-indicated host name,
/// following the accepted common dynamic-SQL descriptor parsing pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2CursorHostOperands {
    None,
    Variables {
        references: Vec<Db2CursorHostReference>,
        clause_span: Db2SourceSpan,
    },
    Descriptor {
        name: Db2HostIdentifier,
        name_span: Db2SourceSpan,
        clause_span: Db2SourceSpan,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2CursorOperationKind {
    Open {
        using: Db2CursorHostOperands,
    },
    Fetch {
        orientation: Db2FetchOrientation,
        orientation_span: Option<Db2SourceSpan>,
        from_span: Option<Db2SourceSpan>,
        into: Db2CursorHostOperands,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CursorOperationStatement {
    cursor_name: Db2Identifier,
    cursor_span: Db2SourceSpan,
    kind: Db2CursorOperationKind,
    span: Db2SourceSpan,
}

impl Db2CursorOperationStatement {
    #[must_use]
    pub const fn cursor_name(&self) -> &Db2Identifier {
        &self.cursor_name
    }
    #[must_use]
    pub const fn cursor_span(&self) -> Db2SourceSpan {
        self.cursor_span
    }
    #[must_use]
    pub const fn kind(&self) -> &Db2CursorOperationKind {
        &self.kind
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one OPEN or FETCH without binding or executing it. One host
/// list is allowed per statement; its aggregate count uses `max_list_items`.
/// No expression nodes or recursion are allocated. Host names keep case and
/// hyphens; cursor names decode doubled double-quotes exactly once before the
/// owned identifier constructor applies case/trailing-space rules.
pub fn parse_db2_cursor_operation_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2CursorOperationStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    CursorOperationParser {
        cursor: lexed.cursor(),
        previous: None,
        limits: ast_limits,
    }
    .parse()
}

struct CursorOperationParser<'a> {
    cursor: Db2TokenCursor<'a>,
    previous: Option<&'a Db2Token>,
    limits: Db2AstLimits,
}

impl<'a> CursorOperationParser<'a> {
    fn parse(&mut self) -> Result<Db2CursorOperationStatement, Db2SyntaxDiagnostic> {
        let first = self.cursor.peek().expect("lexer rejects empty input").span;
        let open = self.take_word("OPEN").is_some();
        if !open && self.take_word("FETCH").is_none() {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the declared OPEN/FETCH family",
            ));
        }
        let (orientation, orientation_span, from_span) = if open {
            (Db2FetchOrientation::Unspecified, None, None)
        } else {
            let mut orientation = Db2FetchOrientation::Unspecified;
            let mut span = None;
            for (word, value) in [
                ("NEXT", Db2FetchOrientation::Next),
                ("PRIOR", Db2FetchOrientation::Prior),
                ("FIRST", Db2FetchOrientation::First),
                ("LAST", Db2FetchOrientation::Last),
                ("CURRENT", Db2FetchOrientation::Current),
            ] {
                if let Some(found) = self.take_word(word) {
                    orientation = value;
                    span = Some(found);
                    break;
                }
            }
            (orientation, span, self.take_word("FROM"))
        };
        let (cursor_name, cursor_span) = self.cursor_name()?;
        let operands = self.operands(if open { "USING" } else { "INTO" }, !open)?;
        let kind = if open {
            Db2CursorOperationKind::Open { using: operands }
        } else {
            Db2CursorOperationKind::Fetch {
                orientation,
                orientation_span,
                from_span,
                into: operands,
            }
        };
        self.take_symbol(Db2Symbol::Semicolon);
        if self.cursor.peek().is_some() {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "unsupported or trailing OPEN/FETCH syntax; only one statement is allowed",
            ));
        }
        Ok(Db2CursorOperationStatement {
            cursor_name,
            cursor_span,
            kind,
            span: join(first, self.previous.expect("statement has tokens").span),
        })
    }

    fn cursor_name(&mut self) -> Result<(Db2Identifier, Db2SourceSpan), Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.problem(Db2SyntaxDiagnosticCode::MissingToken, "missing cursor name"));
        };
        let Db2TokenKind::Word { value, delimited } = &token.kind else {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "cursor name must be an unqualified SQL identifier",
            ));
        };
        // Do not reinterpret a missing operand or an undeclared FETCH modifier
        // as a cursor. Quoted identifiers retain the identifier interpretation.
        if !delimited
            && matches!(
                value.as_str(),
                "USING"
                    | "INTO"
                    | "DESCRIPTOR"
                    | "FROM"
                    | "NEXT"
                    | "PRIOR"
                    | "FIRST"
                    | "LAST"
                    | "CURRENT"
                    | "BEFORE"
                    | "AFTER"
                    | "ABSOLUTE"
                    | "RELATIVE"
                    | "ROWSET"
                    | "SENSITIVE"
                    | "INSENSITIVE"
                    | "WITH"
                    | "CONTINUE"
                    | "FOR"
            )
        {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "missing cursor name or unsupported cursor modifier",
            ));
        }
        let effective = if *delimited {
            value.replace("\"\"", "\"")
        } else {
            value.clone()
        };
        let name = Db2Identifier::new(effective, *delimited, self.limits).map_err(|problem| {
            self.problem(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        self.advance();
        Ok((name, token.span))
    }

    fn operands(
        &mut self,
        keyword: &str,
        output: bool,
    ) -> Result<Db2CursorHostOperands, Db2SyntaxDiagnostic> {
        let Some(first) = self.take_word(keyword) else {
            return Ok(Db2CursorHostOperands::None);
        };
        if self.take_word("DESCRIPTOR").is_some() {
            let (name, name_span) = self.host_identifier()?;
            return Ok(Db2CursorHostOperands::Descriptor {
                name,
                name_span,
                clause_span: join(first, name_span),
            });
        }
        let mut references = Vec::new();
        let mut targets = BTreeSet::new();
        loop {
            if references.len() >= self.limits.max_list_items {
                return Err(self.problem(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "OPEN/FETCH host list exceeds the configured aggregate item limit",
                ));
            }
            let reference = self.host_reference()?;
            // Exact repeated target spelling is decidable here. Host-language
            // aliases and case equivalence still require binding.
            if output && !targets.insert(reference.reference.variable().value().to_owned()) {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    reference.variable_span.start,
                    "FETCH repeats the same host target spelling",
                ));
            }
            references.push(reference);
            if !self.take_symbol(Db2Symbol::Comma) {
                break;
            }
        }
        Ok(Db2CursorHostOperands::Variables {
            references,
            clause_span: join(first, self.previous.expect("nonempty host list").span),
        })
    }

    fn host_identifier(
        &mut self,
    ) -> Result<(Db2HostIdentifier, Db2SourceSpan), Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.problem(Db2SyntaxDiagnosticCode::MissingToken, "missing host name"));
        };
        let Db2TokenKind::HostVariable(value) = &token.kind else {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "operand requires a colon-prefixed static host name",
            ));
        };
        let name = Db2HostIdentifier::new(value.clone(), self.limits).map_err(|problem| {
            self.problem(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        self.advance();
        Ok((name, token.span))
    }

    fn host_reference(&mut self) -> Result<Db2CursorHostReference, Db2SyntaxDiagnostic> {
        let (variable, variable_span) = self.host_identifier()?;
        let indicator_keyword_span = self.take_word("INDICATOR");
        let (indicator, indicator_span) = if indicator_keyword_span.is_some()
            || matches!(
                self.cursor.peek().map(|token| &token.kind),
                Some(Db2TokenKind::HostVariable(_))
            ) {
            let (name, span) = self.host_identifier()?;
            (Some(name), Some(span))
        } else {
            (None, None)
        };
        Ok(Db2CursorHostReference {
            reference: Db2HostReference::new(variable, indicator),
            span: join(variable_span, indicator_span.unwrap_or(variable_span)),
            variable_span,
            indicator_span,
            indicator_keyword_span,
        })
    }

    fn take_word(&mut self, word: &str) -> Option<Db2SourceSpan> {
        let token = self.cursor.peek()?;
        if matches!(&token.kind, Db2TokenKind::Word { value, delimited: false } if value == word) {
            self.advance();
            Some(token.span)
        } else {
            None
        }
    }

    fn take_symbol(&mut self, symbol: Db2Symbol) -> bool {
        if matches!(self.cursor.peek().map(|token| &token.kind), Some(Db2TokenKind::Symbol(value)) if *value == symbol)
        {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.previous = self.cursor.next();
    }

    fn problem(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        let location = self.cursor.peek().map_or_else(
            || {
                self.previous
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message)
    }
}

fn join(first: Db2SourceSpan, last: Db2SourceSpan) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: first.start_byte,
        end_byte: last.end_byte,
        start: first.start,
        end: last.end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2CursorOperationStatement, Db2SyntaxDiagnostic> {
        parse_db2_cursor_operation_statement(
            source,
            Db2SyntaxLimits::default(),
            Db2AstLimits::default(),
        )
    }

    fn operands(statement: &Db2CursorOperationStatement) -> &Db2CursorHostOperands {
        match statement.kind() {
            Db2CursorOperationKind::Open { using } => using,
            Db2CursorOperationKind::Fetch { into, .. } => into,
        }
    }

    fn references(statement: &Db2CursorOperationStatement) -> &[Db2CursorHostReference] {
        let Db2CursorHostOperands::Variables { references, .. } = operands(statement) else {
            panic!("expected host list")
        };
        references
    }

    #[test]
    fn open_preserves_owned_host_leaves_and_indicator_spelling() {
        let statement = {
            let source =
                String::from("open cur using :Input-A, :Mixed :Ind-X, :Third indicator :Ind-Y;");
            parse(&source).unwrap()
        };
        assert_eq!(statement.cursor_name().value(), "CUR");
        assert!(!statement.cursor_name().is_delimited());
        let hosts = references(&statement);
        assert_eq!(hosts.len(), 3);
        assert_eq!(hosts[0].reference().variable().value(), "Input-A");
        assert!(hosts[0].reference().indicator().is_none());
        assert_eq!(hosts[1].reference().indicator().unwrap().value(), "Ind-X");
        assert!(hosts[1].indicator_keyword_span().is_none());
        assert_eq!(hosts[2].reference().variable().value(), "Third");
        assert_eq!(hosts[2].reference().indicator().unwrap().value(), "Ind-Y");
        assert!(hosts[2].indicator_keyword_span().is_some());
        assert_eq!(
            operands(&parse("OPEN C").unwrap()),
            &Db2CursorHostOperands::None
        );
        // Repeated input host names are legal; only target repetition is fenced.
        assert!(parse("OPEN C USING :A, :A").is_ok());
    }

    #[test]
    fn fetch_orientation_from_and_target_matrix() {
        for (word, expected) in [
            ("", Db2FetchOrientation::Unspecified),
            ("NEXT ", Db2FetchOrientation::Next),
            ("PRIOR ", Db2FetchOrientation::Prior),
            ("FIRST ", Db2FetchOrientation::First),
            ("LAST ", Db2FetchOrientation::Last),
            ("CURRENT ", Db2FetchOrientation::Current),
        ] {
            for from in ["", "FROM "] {
                for target in [
                    "",
                    " INTO :Output-A :Null-A, :Output-B INDICATOR :Null-B",
                    " INTO DESCRIPTOR :Out-Da",
                ] {
                    let source = format!("FETCH {word}{from}C{target};");
                    let statement = parse(&source).unwrap();
                    let Db2CursorOperationKind::Fetch {
                        orientation,
                        orientation_span,
                        from_span,
                        into,
                    } = statement.kind()
                    else {
                        panic!("expected FETCH")
                    };
                    assert_eq!(*orientation, expected);
                    assert_eq!(orientation_span.is_some(), !word.is_empty());
                    assert_eq!(from_span.is_some(), !from.is_empty());
                    assert_eq!(
                        matches!(into, Db2CursorHostOperands::None),
                        target.is_empty()
                    );
                    if target.starts_with(" INTO :") {
                        assert_eq!(references(&statement).len(), 2);
                    }
                }
            }
        }
        assert_eq!(
            Db2FetchOrientation::Unspecified.effective(),
            Db2FetchOrientation::Next
        );
        assert_eq!(
            Db2FetchOrientation::Prior.effective(),
            Db2FetchOrientation::Prior
        );
    }

    #[test]
    fn descriptor_names_retain_case_and_hyphens_without_indicators() {
        for source in [
            "OPEN C USING DESCRIPTOR :Sql-Da",
            "FETCH C INTO DESCRIPTOR :Sql-Da",
        ] {
            let statement = parse(source).unwrap();
            let Db2CursorHostOperands::Descriptor {
                name,
                name_span,
                clause_span,
            } = operands(&statement)
            else {
                panic!("expected descriptor")
            };
            assert_eq!(name.value(), "Sql-Da");
            assert_eq!(&source[name_span.start_byte..name_span.end_byte], ":Sql-Da");
            assert!(source[clause_span.start_byte..clause_span.end_byte].contains("DESCRIPTOR"));
        }
    }

    #[test]
    fn effective_cursor_names_decode_once_preserve_case_and_trim_trailing_spaces() {
        for family in ["OPEN", "FETCH"] {
            for (raw, effective) in [
                ("abc", "ABC"),
                ("\"abc\"", "abc"),
                ("\"ABC  \"", "ABC"),
                ("\" A\"\"B  \"", " A\"B"),
                ("\"A\"\"\"\"B\"", "A\"\"B"),
                ("\"NEXT\"", "NEXT"),
            ] {
                let statement = parse(&format!("{family} {raw}")).unwrap();
                assert_eq!(statement.cursor_name().value(), effective);
                assert_eq!(statement.cursor_name().is_delimited(), raw.starts_with('"'));
            }
            assert_eq!(
                parse(&format!("{family} abc"))
                    .unwrap()
                    .cursor_name()
                    .value(),
                parse(&format!("{family} \"ABC \""))
                    .unwrap()
                    .cursor_name()
                    .value()
            );
        }
    }

    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        let prefix = &source[..byte];
        Db2SourceLocation {
            line: prefix.bytes().filter(|b| *b == b'\n').count() as u32 + 1,
            column: prefix.rsplit('\n').next().unwrap().chars().count() as u32 + 1,
        }
    }

    fn span(source: &str, actual: Db2SourceSpan, text: &str) {
        assert_eq!(&source[actual.start_byte..actual.end_byte], text);
        assert_eq!(actual.start, location(source, actual.start_byte));
        assert_eq!(actual.end, location(source, actual.end_byte));
    }

    #[test]
    fn relocation_matrix_retains_every_leaf_clause_and_statement_span() {
        for prefix in ["", " \n-- lead\n  ", "/* α */\n\n  "] {
            for family in ["OPEN", "FETCH"] {
                let body = if family == "OPEN" {
                    "OPEN /* x */ \"é\"\"C  \"\n USING :Var-A /* i */ INDICATOR\n :Ind-A, :Var-B;"
                } else {
                    "FETCH /* x */ PRIOR\n FROM \"é\"\"C  \"\n INTO :Var-A /* i */ INDICATOR\n :Ind-A, :Var-B;"
                };
                let source = format!("{prefix}{body} -- suffix\n");
                let statement = parse(&source).unwrap();
                span(&source, statement.span(), body);
                span(&source, statement.cursor_span(), "\"é\"\"C  \"");
                assert_eq!(statement.cursor_name().value(), "é\"C");
                let Db2CursorHostOperands::Variables {
                    references,
                    clause_span,
                } = operands(&statement)
                else {
                    panic!()
                };
                span(
                    &source,
                    *clause_span,
                    if family == "OPEN" {
                        "USING :Var-A /* i */ INDICATOR\n :Ind-A, :Var-B"
                    } else {
                        "INTO :Var-A /* i */ INDICATOR\n :Ind-A, :Var-B"
                    },
                );
                span(
                    &source,
                    references[0].span(),
                    ":Var-A /* i */ INDICATOR\n :Ind-A",
                );
                span(&source, references[0].variable_span(), ":Var-A");
                span(&source, references[0].indicator_span().unwrap(), ":Ind-A");
                span(
                    &source,
                    references[0].indicator_keyword_span().unwrap(),
                    "INDICATOR",
                );
                span(&source, references[1].span(), ":Var-B");
                if let Db2CursorOperationKind::Fetch {
                    orientation_span,
                    from_span,
                    ..
                } = statement.kind()
                {
                    span(&source, orientation_span.unwrap(), "PRIOR");
                    span(&source, from_span.unwrap(), "FROM");
                }
            }
            for body in [
                "OPEN C USING DESCRIPTOR\n :Da",
                "FETCH NEXT FROM C INTO DESCRIPTOR\n :Da",
            ] {
                let source = format!("{prefix}{body}");
                let statement = parse(&source).unwrap();
                let Db2CursorHostOperands::Descriptor {
                    name_span,
                    clause_span,
                    ..
                } = operands(&statement)
                else {
                    panic!()
                };
                span(&source, *name_span, ":Da");
                let clause = if body.starts_with("OPEN") {
                    "USING DESCRIPTOR\n :Da"
                } else {
                    "INTO DESCRIPTOR\n :Da"
                };
                span(&source, *clause_span, clause);
            }
        }
    }

    #[test]
    fn malformed_and_undeclared_forms_fail_closed() {
        for source in [
            "",
            "-- only a comment",
            "OPEN",
            "FETCH",
            "FETCH NEXT",
            "FETCH FROM",
            "FETCH NEXT FROM",
            "OPEN USING :A",
            "FETCH INTO :A",
            "OPEN S.C",
            "FETCH S.C",
            "OPEN :C",
            "FETCH :C",
            "OPEN C USING",
            "FETCH C INTO",
            "OPEN C USING :A,",
            "FETCH C INTO , :A",
            "OPEN C USING A",
            "FETCH C INTO A",
            "OPEN C USING ?",
            "FETCH C INTO 1",
            "OPEN C USING (:A)",
            "FETCH C INTO (:A)",
            "OPEN C USING :A :I :J",
            "FETCH C INTO :A INDICATOR",
            "OPEN C USING :A INDICATOR I",
            "FETCH C INTO INDICATOR :I",
            "OPEN C USING DESCRIPTOR",
            "FETCH C INTO DESCRIPTOR",
            "OPEN C USING DESCRIPTOR D",
            "FETCH C INTO DESCRIPTOR D",
            "OPEN C USING DESCRIPTOR :D :I",
            "FETCH C INTO DESCRIPTOR :D INDICATOR :I",
            "OPEN C USING DESCRIPTOR :D, :A",
            "FETCH C INTO DESCRIPTOR :D INTO :A",
            "OPEN C USING :A USING :B",
            "FETCH C INTO :A INTO :B",
            "OPEN C INTO :A",
            "FETCH C USING :A",
            "FETCH NEXT PRIOR C",
            "FETCH FROM NEXT C",
            "FETCH FROM FROM",
            "FETCH CURRENT CONTINUE C",
            "FETCH INSENSITIVE C INTO :A",
            "FETCH SENSITIVE C INTO :A",
            "FETCH WITH CONTINUE C INTO :A",
            "FETCH BEFORE",
            "FETCH BEFORE C",
            "FETCH AFTER C",
            "FETCH ABSOLUTE 1 C INTO :A",
            "FETCH RELATIVE :N FROM C",
            "FETCH NEXT ROWSET C INTO :A",
            "FETCH ROWSET STARTING AT ABSOLUTE 1 C",
            "FETCH C FOR 2 ROWS INTO :A",
            "FETCH C INTO :A FOR 2 ROWS",
            "FETCH C WITH CONTINUE",
            "OPEN C USING GLOBAL_VAR",
            "OPEN C USING G[1]",
            "FETCH C INTO G[1]",
            "OPEN C USING :A[1]",
            "FETCH C INTO :A(1)",
            "OPEN C USING :S.FIELD",
            "FETCH C INTO :S.FIELD",
            "OPEN C USING DESCRIPTOR :*D",
            "FETCH C INTO DESCRIPTOR :*D",
            "OPEN C USING :A + 1",
            "FETCH C INTO :A || :B",
            "FETCH C INTO :A, :A",
            "FETCH C INTO :A :I, :A :J",
            "OPEN C; FETCH C",
            "FETCH C;;",
            "OPEN C EXTRA",
            "CLOSE C",
            "EXECUTE C",
            "SELECT 1",
            "OPEN \"\"",
            "FETCH \"   \"",
            "OPEN \"unterminated",
        ] {
            let error = parse(source).expect_err(source);
            assert!(
                error.location.line >= 1 && error.location.column >= 1,
                "{source}"
            );
            assert!(error.message.len() <= 256);
        }
    }

    #[test]
    fn failures_relocate_to_original_source_locations() {
        for prefix in ["", "\n-- comment\n  ", "/* α */\n "] {
            for (body, bad) in [
                ("OPEN C USING :A, BAD", "BAD"),
                ("FETCH C INTO :A INDICATOR BAD", "BAD"),
                ("FETCH ABSOLUTE 1 C", "ABSOLUTE"),
                ("FETCH C INTO DESCRIPTOR :D :I", ":I"),
                ("OPEN C; FETCH C", "FETCH"),
                ("FETCH C INTO :A, :A", ":A"),
            ] {
                let source = format!("{prefix}{body}");
                let byte = source.rfind(bad).unwrap();
                assert_eq!(
                    parse(&source).unwrap_err().location,
                    location(&source, byte),
                    "{source}"
                );
            }
            let source = format!("{prefix}OPEN C USING");
            assert_eq!(
                parse(&source).unwrap_err().location,
                location(&source, source.len())
            );
        }
    }

    #[test]
    fn aggregate_host_list_bound_counts_references_not_indicator_components() {
        let ast = Db2AstLimits {
            max_list_items: 2,
            max_expression_nodes: 1,
            max_expression_depth: 1,
            ..Db2AstLimits::default()
        };
        for source in [
            "OPEN C USING :A :I, :B INDICATOR :J",
            "FETCH C INTO :A :I, :B INDICATOR :J",
        ] {
            let parsed =
                parse_db2_cursor_operation_statement(source, Db2SyntaxLimits::default(), ast)
                    .unwrap();
            assert_eq!(references(&parsed).len(), 2);
            let over = format!("{source}, :D");
            assert_eq!(
                parse_db2_cursor_operation_statement(&over, Db2SyntaxLimits::default(), ast)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
        let one = Db2AstLimits {
            max_list_items: 1,
            ..ast
        };
        assert!(
            parse_db2_cursor_operation_statement(
                "FETCH C INTO :A :I",
                Db2SyntaxLimits::default(),
                one
            )
            .is_ok()
        );
        assert!(
            parse_db2_cursor_operation_statement(
                "OPEN C USING DESCRIPTOR :D",
                Db2SyntaxLimits::default(),
                one
            )
            .is_ok()
        );
    }

    #[test]
    fn effective_name_and_host_component_exact_and_one_beyond_bounds() {
        let ast = Db2AstLimits {
            max_identifier_bytes: 3,
            ..Db2AstLimits::default()
        };
        for source in [
            "OPEN ABC",
            "FETCH \"A\"\"B   \"",
            "OPEN \"éA\"",
            "OPEN C USING :Abc :Ind",
            "FETCH C INTO :Abc INDICATOR :Ind",
            "FETCH C INTO DESCRIPTOR :Abc",
        ] {
            assert!(
                parse_db2_cursor_operation_statement(source, Db2SyntaxLimits::default(), ast)
                    .is_ok(),
                "{source}"
            );
        }
        for source in [
            "OPEN ABCD",
            "FETCH \"A\"\"BC\"",
            "OPEN \"éAB\"",
            "OPEN C USING :Abcd",
            "FETCH C INTO :A :Indd",
            "FETCH C INTO :A INDICATOR :Indd",
            "OPEN C USING DESCRIPTOR :Abcd",
        ] {
            assert_eq!(
                parse_db2_cursor_operation_statement(source, Db2SyntaxLimits::default(), ast)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{source}"
            );
        }
    }

    #[test]
    fn lexer_limits_and_invalid_configuration_are_enforced() {
        let source = "FETCH NEXT C INTO :A";
        let exact = Db2SyntaxLimits {
            max_statement_bytes: source.len(),
            max_tokens: 5,
            max_token_bytes: 5,
            max_nesting: 1,
        };
        assert!(
            parse_db2_cursor_operation_statement(source, exact, Db2AstLimits::default()).is_ok()
        );
        for (syntax, code) in [
            (
                Db2SyntaxLimits {
                    max_statement_bytes: source.len() - 1,
                    ..exact
                },
                Db2SyntaxDiagnosticCode::StatementTooLarge,
            ),
            (
                Db2SyntaxLimits {
                    max_tokens: 4,
                    ..exact
                },
                Db2SyntaxDiagnosticCode::TooManyTokens,
            ),
            (
                Db2SyntaxLimits {
                    max_token_bytes: 4,
                    ..exact
                },
                Db2SyntaxDiagnosticCode::TokenTooLarge,
            ),
            (
                Db2SyntaxLimits {
                    max_nesting: 0,
                    ..exact
                },
                Db2SyntaxDiagnosticCode::InvalidLimits,
            ),
        ] {
            assert_eq!(
                parse_db2_cursor_operation_statement(source, syntax, Db2AstLimits::default())
                    .unwrap_err()
                    .code,
                code
            );
        }
        let nested = "/* outer /* inner */ */ OPEN C";
        let nesting = Db2SyntaxLimits {
            max_nesting: 2,
            ..Db2SyntaxLimits::default()
        };
        assert!(
            parse_db2_cursor_operation_statement(nested, nesting, Db2AstLimits::default()).is_ok()
        );
        assert!(
            parse_db2_cursor_operation_statement(
                nested,
                Db2SyntaxLimits {
                    max_nesting: 1,
                    ..nesting
                },
                Db2AstLimits::default()
            )
            .is_err()
        );
        for ast in [
            Db2AstLimits {
                max_identifier_bytes: 0,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_list_items: 0,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_expression_nodes: 0,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_expression_depth: 0,
                ..Db2AstLimits::default()
            },
        ] {
            assert_eq!(
                parse_db2_cursor_operation_statement("OPEN C", Db2SyntaxLimits::default(), ast)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidLimits
            );
        }
    }
}
