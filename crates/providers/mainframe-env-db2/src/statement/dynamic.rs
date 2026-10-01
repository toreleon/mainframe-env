use super::{Db2Statement, Db2StatementKind, StatementParser};
use crate::{
    Db2AstLimits, Db2HostIdentifier, Db2HostReference, Db2SourceLocation, Db2SourceSpan,
    Db2StatementId, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    Db2TokenKind, lex_db2,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DescriptorNameMode {
    Names,
    Labels,
    Any,
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2PrepareDescriptor {
    name: Db2HostIdentifier,
    mode: Db2DescriptorNameMode,
}

impl Db2PrepareDescriptor {
    #[must_use]
    pub const fn name(&self) -> &Db2HostIdentifier {
        &self.name
    }

    #[must_use]
    pub const fn mode(&self) -> Db2DescriptorNameMode {
        self.mode
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2PrepareStatement {
    name: crate::Db2Identifier,
    descriptor: Option<Db2PrepareDescriptor>,
    attributes: Option<Db2HostReference>,
    source: Db2HostIdentifier,
}

impl Db2PrepareStatement {
    #[must_use]
    pub const fn name(&self) -> &crate::Db2Identifier {
        &self.name
    }

    #[must_use]
    pub const fn descriptor(&self) -> Option<&Db2PrepareDescriptor> {
        self.descriptor.as_ref()
    }

    #[must_use]
    pub const fn attributes(&self) -> Option<&Db2HostReference> {
        self.attributes.as_ref()
    }

    #[must_use]
    pub const fn source(&self) -> &Db2HostIdentifier {
        &self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ExecuteUsing {
    None,
    Variables(Vec<Db2HostReference>),
    Descriptor(Db2HostIdentifier),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ExecuteStatement {
    name: crate::Db2Identifier,
    using: Db2ExecuteUsing,
}

impl Db2ExecuteStatement {
    #[must_use]
    pub const fn name(&self) -> &crate::Db2Identifier {
        &self.name
    }

    #[must_use]
    pub const fn using(&self) -> &Db2ExecuteUsing {
        &self.using
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ExecuteImmediateStatement {
    source: Db2HostIdentifier,
}

impl Db2ExecuteImmediateStatement {
    #[must_use]
    pub const fn source(&self) -> &Db2HostIdentifier {
        &self.source
    }
}

/// Parse the bounded common static-host subset of PREPARE, EXECUTE, and
/// EXECUTE IMMEDIATE. Unsupported PL/I expressions, array elements, and
/// multi-row buffers fail explicitly and do not complete their row obligations.
pub fn parse_db2_dynamic_statement(
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
    let words = lexed.tokens();
    let first = words.first().ok_or_else(|| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            Db2SourceLocation::START,
            "Db2 dynamic statement must begin with PREPARE or EXECUTE",
        )
    })?;
    let (id, kind) = if parser.peek_word("PREPARE") {
        (
            Db2StatementId::SqlPrepare,
            Db2StatementKind::Prepare(parser.parse_prepare()?),
        )
    } else if parser.peek_word("EXECUTE")
        && matches!(
            words.get(1).map(|token| &token.kind),
            Some(Db2TokenKind::Word { value, delimited: false }) if value == "IMMEDIATE"
        )
    {
        (
            Db2StatementId::SqlExecuteImmediate,
            Db2StatementKind::ExecuteImmediate(parser.parse_execute_immediate()?),
        )
    } else if parser.peek_word("EXECUTE") {
        (
            Db2StatementId::SqlExecute,
            Db2StatementKind::Execute(parser.parse_execute()?),
        )
    } else {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            first.span.start,
            "statement is outside the Db2 dynamic SQL syntax family",
        ));
    };
    parser.finish()?;
    let last = &words[words.len() - 1].span;
    Ok(Db2Statement {
        id,
        kind,
        span: Db2SourceSpan {
            start_byte: first.span.start_byte,
            end_byte: last.end_byte,
            start: first.span.start,
            end: last.end,
        },
    })
}

impl StatementParser<'_> {
    fn parse_prepare(&mut self) -> Result<Db2PrepareStatement, Db2SyntaxDiagnostic> {
        self.expect_word("PREPARE")?;
        let name = self.identifier("prepared statement name")?;
        let descriptor = if self.take_word("INTO") {
            let descriptor_name = self.host_identifier("SQLDA descriptor name")?;
            let mode = if self.take_word("USING") {
                if self.take_word("NAMES") {
                    Db2DescriptorNameMode::Names
                } else if self.take_word("LABELS") {
                    Db2DescriptorNameMode::Labels
                } else if self.take_word("ANY") {
                    Db2DescriptorNameMode::Any
                } else if self.take_word("BOTH") {
                    Db2DescriptorNameMode::Both
                } else {
                    return Err(self.diagnostic_here(
                        Db2SyntaxDiagnosticCode::MissingToken,
                        "PREPARE INTO USING requires NAMES, LABELS, ANY, or BOTH",
                    ));
                }
            } else {
                Db2DescriptorNameMode::Names
            };
            Some(Db2PrepareDescriptor {
                name: descriptor_name,
                mode,
            })
        } else {
            None
        };
        let attributes = if self.take_word("ATTRIBUTES") {
            Some(self.host_reference("PREPARE attribute host variable")?)
        } else {
            None
        };
        self.expect_word("FROM")?;
        let source = self.host_identifier("PREPARE source host variable")?;
        if matches!(
            self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::HostVariable(_))
        ) || self.take_word("INDICATOR")
        {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "PREPARE source host variable cannot have an indicator",
            ));
        }
        Ok(Db2PrepareStatement {
            name,
            descriptor,
            attributes,
            source,
        })
    }

    fn parse_execute(&mut self) -> Result<Db2ExecuteStatement, Db2SyntaxDiagnostic> {
        self.expect_word("EXECUTE")?;
        let name = self.identifier("prepared statement name")?;
        let using = if self.take_word("USING") {
            if self.take_word("DESCRIPTOR") {
                Db2ExecuteUsing::Descriptor(self.host_identifier("SQLDA descriptor name")?)
            } else {
                let mut variables = Vec::new();
                loop {
                    if variables.len() >= self.ast_limits.max_list_items {
                        return Err(self.diagnostic_here(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            "EXECUTE USING exceeds the configured variable limit",
                        ));
                    }
                    variables.push(self.host_reference("EXECUTE USING variable")?);
                    if !self.take_symbol(Db2Symbol::Comma) {
                        break;
                    }
                }
                Db2ExecuteUsing::Variables(variables)
            }
        } else {
            Db2ExecuteUsing::None
        };
        Ok(Db2ExecuteStatement { name, using })
    }

    fn parse_execute_immediate(
        &mut self,
    ) -> Result<Db2ExecuteImmediateStatement, Db2SyntaxDiagnostic> {
        self.expect_word("EXECUTE")?;
        self.expect_word("IMMEDIATE")?;
        let source = self.host_identifier("EXECUTE IMMEDIATE source host variable")?;
        if matches!(
            self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::HostVariable(_))
        ) || self.take_word("INDICATOR")
        {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "EXECUTE IMMEDIATE source cannot have an indicator",
            ));
        }
        Ok(Db2ExecuteImmediateStatement { source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2Statement, Db2SyntaxDiagnostic> {
        parse_db2_dynamic_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    #[test]
    fn prepare_common_host_forms_preserve_descriptor_attributes_and_source() {
        let statement = parse(
            "PREPARE S1 INTO :Sql-Da USING BOTH ATTRIBUTES :Attrs INDICATOR :Attrs-Ind FROM :Sql-Text;",
        )
        .unwrap();
        assert_eq!(statement.id(), Db2StatementId::SqlPrepare);
        let Db2StatementKind::Prepare(prepare) = statement.kind() else {
            panic!("expected PREPARE AST")
        };
        assert_eq!(prepare.name().value(), "S1");
        let descriptor = prepare.descriptor().unwrap();
        assert_eq!(descriptor.name().value(), "Sql-Da");
        assert_eq!(descriptor.mode(), Db2DescriptorNameMode::Both);
        let attributes = prepare.attributes().unwrap();
        assert_eq!(attributes.variable().value(), "Attrs");
        assert_eq!(attributes.indicator().unwrap().value(), "Attrs-Ind");
        assert_eq!(prepare.source().value(), "Sql-Text");

        let minimal = parse("PREPARE S2 FROM :Text").unwrap();
        let Db2StatementKind::Prepare(prepare) = minimal.kind() else {
            panic!("expected PREPARE AST")
        };
        assert!(prepare.descriptor().is_none());
        assert!(prepare.attributes().is_none());
    }

    #[test]
    fn prepare_rejects_missing_modes_non_host_sources_and_source_indicators() {
        for source in [
            "PREPARE S INTO :D USING FROM :TEXT",
            "PREPARE S FROM 'SELECT 1'",
            "PREPARE S FROM :TEXT :IND",
            "PREPARE S FROM :TEXT INDICATOR :IND",
            "PREPARE S INTO D FROM :TEXT",
        ] {
            assert!(
                parse(source).is_err(),
                "unexpected PREPARE success: {source}"
            );
        }
    }

    #[test]
    fn execute_common_host_and_descriptor_forms_are_typed() {
        let plain = parse("EXECUTE S1").unwrap();
        let Db2StatementKind::Execute(execute) = plain.kind() else {
            panic!("expected EXECUTE AST")
        };
        assert_eq!(execute.name().value(), "S1");
        assert_eq!(execute.using(), &Db2ExecuteUsing::None);

        let variables = parse("EXECUTE S1 USING :A :A-IND, :B INDICATOR :B-IND").unwrap();
        let Db2StatementKind::Execute(execute) = variables.kind() else {
            panic!("expected EXECUTE AST")
        };
        let Db2ExecuteUsing::Variables(variables) = execute.using() else {
            panic!("expected variables")
        };
        assert_eq!(variables.len(), 2);
        assert_eq!(variables[0].variable().value(), "A");
        assert_eq!(variables[0].indicator().unwrap().value(), "A-IND");
        assert_eq!(variables[1].variable().value(), "B");
        assert_eq!(variables[1].indicator().unwrap().value(), "B-IND");

        let descriptor = parse("EXECUTE S1 USING DESCRIPTOR :Sql-Da").unwrap();
        let Db2StatementKind::Execute(execute) = descriptor.kind() else {
            panic!("expected EXECUTE AST")
        };
        let Db2ExecuteUsing::Descriptor(descriptor) = execute.using() else {
            panic!("expected descriptor")
        };
        assert_eq!(descriptor.value(), "Sql-Da");
    }

    #[test]
    fn execute_lists_are_bounded_and_malformed_forms_fail_closed() {
        let limits = Db2AstLimits {
            max_list_items: 1,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            parse_db2_dynamic_statement(
                "EXECUTE S USING :A, :B",
                Db2SyntaxLimits::default(),
                limits
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        for source in [
            "EXECUTE S USING",
            "EXECUTE S USING :A,",
            "EXECUTE S USING DESCRIPTOR D",
            "EXECUTE S USING DESCRIPTOR :D, :A",
        ] {
            assert!(
                parse(source).is_err(),
                "unexpected EXECUTE success: {source}"
            );
        }
    }

    #[test]
    fn execute_immediate_requires_one_non_indicated_host_string() {
        let statement = parse("EXECUTE IMMEDIATE :Sql-Text").unwrap();
        assert_eq!(statement.id(), Db2StatementId::SqlExecuteImmediate);
        let Db2StatementKind::ExecuteImmediate(immediate) = statement.kind() else {
            panic!("expected EXECUTE IMMEDIATE AST")
        };
        assert_eq!(immediate.source().value(), "Sql-Text");
        for source in [
            "EXECUTE IMMEDIATE",
            "EXECUTE IMMEDIATE 'DELETE FROM T'",
            "EXECUTE IMMEDIATE :TEXT :IND",
            "EXECUTE IMMEDIATE :TEXT INDICATOR :IND",
            "EXECUTE IMMEDIATE :TEXT EXTRA",
        ] {
            assert!(
                parse(source).is_err(),
                "unexpected EXECUTE IMMEDIATE success: {source}"
            );
        }
    }

    #[test]
    fn dynamic_parser_rejects_other_families_and_multiple_statements() {
        assert_eq!(
            parse("SELECT 1").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement
        );
        assert_eq!(
            parse("EXECUTE S; PREPARE X FROM :T").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnexpectedToken
        );
    }

    #[test]
    fn prepare_diagram_options_are_independent_and_ordered() {
        for (source, mode) in [
            ("PREPARE S INTO :D FROM :T", Db2DescriptorNameMode::Names),
            (
                "PREPARE S INTO :D USING NAMES FROM :T",
                Db2DescriptorNameMode::Names,
            ),
            (
                "PREPARE S INTO :D USING LABELS FROM :T",
                Db2DescriptorNameMode::Labels,
            ),
            (
                "PREPARE S INTO :D USING ANY FROM :T",
                Db2DescriptorNameMode::Any,
            ),
            (
                "PREPARE S INTO :D USING BOTH FROM :T",
                Db2DescriptorNameMode::Both,
            ),
        ] {
            let statement = parse(source).unwrap();
            let Db2StatementKind::Prepare(prepare) = statement.kind() else {
                panic!("expected PREPARE AST")
            };
            assert_eq!(prepare.descriptor().unwrap().mode(), mode);
            assert!(prepare.attributes().is_none());
        }
        let statement = parse("PREPARE S ATTRIBUTES :A FROM :T").unwrap();
        let Db2StatementKind::Prepare(prepare) = statement.kind() else {
            panic!("expected PREPARE AST")
        };
        assert!(prepare.descriptor().is_none());
        assert!(prepare.attributes().unwrap().indicator().is_none());

        let statement = parse("PREPARE S INTO :D ATTRIBUTES :A :I FROM :T").unwrap();
        let Db2StatementKind::Prepare(prepare) = statement.kind() else {
            panic!("expected PREPARE AST")
        };
        assert_eq!(
            prepare.attributes().unwrap().indicator().unwrap().value(),
            "I"
        );
    }

    #[test]
    fn execute_diagram_options_allow_single_and_non_indicated_variables() {
        let statement = parse("EXECUTE S USING :A").unwrap();
        let Db2StatementKind::Execute(execute) = statement.kind() else {
            panic!("expected EXECUTE AST")
        };
        let Db2ExecuteUsing::Variables(variables) = execute.using() else {
            panic!("expected variables")
        };
        assert_eq!(variables.len(), 1);
        assert!(variables[0].indicator().is_none());
        assert!(parse("EXECUTE S USING DESCRIPTOR :D").is_ok());
        assert!(parse("EXECUTE S").is_ok());
    }

    #[test]
    fn misplaced_duplicate_and_unsupported_dynamic_forms_fail_locally() {
        for source in [
            "PREPARE S INTO :D INTO :E FROM :T",
            "PREPARE S INTO :D USING BOTH USING NAMES FROM :T",
            "PREPARE S ATTRIBUTES :A ATTRIBUTES :B FROM :T",
            "PREPARE S ATTRIBUTES :A INTO :D FROM :T",
            "PREPARE S FROM :T ATTRIBUTES :A",
            "PREPARE S INTO :D :I FROM :T",
            "PREPARE S INTO :D INDICATOR :I FROM :T",
            "PREPARE S FROM SQL_TEXT",
            "PREPARE S FROM STRING_EXPR || OTHER_EXPR",
            "PREPARE S FROM :T FOR MULTIPLE ROWS",
            "EXECUTE S USING :A USING :B",
            "EXECUTE S USING DESCRIPTOR :D :I",
            "EXECUTE S USING :A[1]",
            "EXECUTE S USING SQL_VAR",
            "EXECUTE S FOR 2 ROWS",
            "EXECUTE IMMEDIATE SQL_TEXT",
            "EXECUTE IMMEDIATE :T FOR 2 ROWS",
            "EXECUTE IMMEDIATE :T; EXECUTE S",
        ] {
            let problem = parse(source).expect_err(source);
            assert!(problem.location.line >= 1, "{source}");
            assert!(problem.location.column >= 1, "{source}");
        }
        let problem = parse("PREPARE S FROM :T :I").unwrap_err();
        assert_eq!(
            problem.code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        assert_eq!(problem.location.column, 19);
    }
}
