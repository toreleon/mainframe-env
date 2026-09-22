use super::{Db2Statement, Db2StatementKind, StatementParser};
use crate::{
    Db2AstLimits, Db2HostIdentifier, Db2HostReference, Db2StatementId, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2TokenKind, lex_db2,
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
    let mut parser = StatementParser::new(lexed.tokens(), ast_limits);
    let first = parser.word_at(0).map(str::to_owned).ok_or_else(|| {
        parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "Db2 dynamic statement must begin with PREPARE or EXECUTE",
        )
    })?;
    let (id, kind) = match first.as_str() {
        "PREPARE" => (
            Db2StatementId::SqlPrepare,
            Db2StatementKind::Prepare(parser.parse_prepare()?),
        ),
        "EXECUTE" if parser.word_at(1) == Some("IMMEDIATE") => (
            Db2StatementId::SqlExecuteImmediate,
            Db2StatementKind::ExecuteImmediate(parser.parse_execute_immediate()?),
        ),
        "EXECUTE" => (
            Db2StatementId::SqlExecute,
            Db2StatementKind::Execute(parser.parse_execute()?),
        ),
        _ => {
            return Err(parser.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the Db2 dynamic SQL syntax family",
            ));
        }
    };
    parser.finish()?;
    Ok(Db2Statement {
        id,
        kind,
        span: parser.statement_span(),
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
            self.tokens.get(self.position).map(|token| &token.kind),
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
            self.tokens.get(self.position).map(|token| &token.kind),
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
}
