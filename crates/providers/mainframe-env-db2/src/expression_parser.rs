//! Bounded owned parser for common Db2 expressions.

use crate::{
    Db2AstLimits, Db2BinaryOperator, Db2BuiltInDataType, Db2BuiltInType, Db2DataType,
    Db2ExpressionArena, Db2ExpressionId, Db2ExpressionKind, Db2HostIdentifier, Db2HostReference,
    Db2Identifier, Db2Literal, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind,
    Db2UnaryOperator, lex_db2,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ParsedExpression {
    arena: Db2ExpressionArena,
    root: Db2ExpressionId,
}

impl Db2ParsedExpression {
    #[must_use]
    pub const fn arena(&self) -> &Db2ExpressionArena {
        &self.arena
    }

    #[must_use]
    pub const fn root(&self) -> Db2ExpressionId {
        self.root
    }
}

/// Parse one complete common Db2 expression into the owned bounded arena.
pub fn parse_db2_expression(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2ParsedExpression, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    let mut parser = ExpressionParser::new(lexed.tokens(), ast_limits)?;
    let root = parser.parse_precedence(0)?;
    if !parser.at_end() {
        return Err(parser.diagnostic_here(
            Db2SyntaxDiagnosticCode::UnexpectedToken,
            "unexpected token after Db2 expression",
        ));
    }
    Ok(Db2ParsedExpression {
        arena: parser.arena,
        root,
    })
}

struct ExpressionParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    limits: Db2AstLimits,
    arena: Db2ExpressionArena,
    contains_parameter: Vec<bool>,
}

impl<'a> ExpressionParser<'a> {
    fn new(tokens: &'a [Db2Token], limits: Db2AstLimits) -> Result<Self, Db2SyntaxDiagnostic> {
        let arena = Db2ExpressionArena::new(limits).map_err(|problem| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                Db2SourceLocation::START,
                problem.message,
            )
        })?;
        Ok(Self {
            tokens,
            position: 0,
            limits,
            arena,
            contains_parameter: Vec::new(),
        })
    }

    fn parse_precedence(&mut self, minimum: u8) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let mut left = if self.take_word("NOT") {
            let start = self.previous_start();
            let operand = self.parse_precedence(3)?;
            let parameter = self.parameter_flag(operand)?;
            self.push(
                Db2ExpressionKind::Unary {
                    operator: Db2UnaryOperator::Not,
                    operand,
                },
                Db2SourceSpan {
                    start,
                    end: self.expression_span(operand)?.end,
                },
                parameter,
            )?
        } else {
            self.parse_unary()?
        };
        loop {
            if self.word() == Some("IS") && 3 >= minimum {
                let start = self.expression_span(left)?.start;
                self.position += 1;
                let negated = self.take_word("NOT");
                self.expect_word("NULL")?;
                if self.parameter_flag(left)? {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "Db2 NULL predicate cannot contain a parameter marker",
                    ));
                }
                let span = Db2SourceSpan {
                    start,
                    end: self.previous_end(),
                };
                left = self.push(
                    Db2ExpressionKind::IsNull {
                        expression: left,
                        negated,
                    },
                    span,
                    false,
                )?;
                continue;
            }
            let Some((operator, precedence)) = self.binary_operator() else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.position += 1;
            let right = self.parse_precedence(precedence + 1)?;
            let span = Db2SourceSpan {
                start: self.expression_span(left)?.start,
                end: self.expression_span(right)?.end,
            };
            let parameter = self.parameter_flag(left)? || self.parameter_flag(right)?;
            left = self.push(
                Db2ExpressionKind::Binary {
                    left,
                    operator,
                    right,
                },
                span,
                parameter,
            )?;
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let operator = if self.take_symbol(Db2Symbol::Plus) {
            Some(Db2UnaryOperator::Positive)
        } else if self.take_symbol(Db2Symbol::Minus) {
            Some(Db2UnaryOperator::Negative)
        } else {
            None
        };
        if let Some(operator) = operator {
            let start = self.previous_start();
            let operand = self.parse_unary()?;
            let parameter = self.parameter_flag(operand)?;
            return self.push(
                Db2ExpressionKind::Unary { operator, operand },
                Db2SourceSpan {
                    start,
                    end: self.expression_span(operand)?.end,
                },
                parameter,
            );
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let Some(token) = self.tokens.get(self.position).cloned() else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "missing Db2 expression operand",
            ));
        };
        match token.kind {
            Db2TokenKind::Number(value) => {
                self.position += 1;
                self.push(
                    Db2ExpressionKind::Literal(Db2Literal::Number(value)),
                    token.span,
                    false,
                )
            }
            Db2TokenKind::String { kind, value } => {
                self.position += 1;
                self.push(
                    Db2ExpressionKind::Literal(Db2Literal::String { kind, value }),
                    token.span,
                    false,
                )
            }
            Db2TokenKind::ParameterMarker => {
                self.position += 1;
                self.push(Db2ExpressionKind::ParameterMarker, token.span, true)
            }
            Db2TokenKind::HostVariable(value) => self.parse_host_reference(value, token.span),
            Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => {
                self.position += 1;
                let expression = self.parse_precedence(0)?;
                self.expect_symbol(Db2Symbol::RightParenthesis)?;
                Ok(expression)
            }
            Db2TokenKind::Word {
                value,
                delimited: false,
            } if value == "NULL" => {
                self.position += 1;
                self.push(
                    Db2ExpressionKind::Literal(Db2Literal::Null),
                    token.span,
                    false,
                )
            }
            Db2TokenKind::Word {
                value,
                delimited: false,
            } if value == "TRUE" || value == "FALSE" => {
                self.position += 1;
                self.push(
                    Db2ExpressionKind::Literal(Db2Literal::Boolean(value == "TRUE")),
                    token.span,
                    false,
                )
            }
            Db2TokenKind::Word {
                value,
                delimited: false,
            } if value == "CASE" => self.parse_case(),
            Db2TokenKind::Word {
                value,
                delimited: false,
            } if value == "CAST" => self.parse_cast(),
            Db2TokenKind::Word { .. } => self.parse_name_or_function(),
            _ => Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "token cannot begin a common Db2 expression",
            )),
        }
    }

    fn parse_host_reference(
        &mut self,
        value: String,
        first_span: Db2SourceSpan,
    ) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        self.position += 1;
        let variable = self.host_identifier(value, first_span.start)?;
        let indicator = if self.take_word("INDICATOR") {
            Some(self.take_host_identifier("indicator variable")?)
        } else if matches!(
            self.tokens.get(self.position).map(|token| &token.kind),
            Some(Db2TokenKind::HostVariable(_))
        ) {
            Some(self.take_host_identifier("indicator variable")?)
        } else {
            None
        };
        let end = self
            .position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(first_span.end, |token| token.span.end);
        self.push(
            Db2ExpressionKind::HostVariable(Db2HostReference::new(variable, indicator)),
            Db2SourceSpan {
                start: first_span.start,
                end,
            },
            false,
        )
    }

    fn parse_name_or_function(&mut self) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        let name = self.qualified_name()?;
        if !self.take_symbol(Db2Symbol::LeftParenthesis) {
            let end = self.previous_end();
            return self.push(
                Db2ExpressionKind::Column(name),
                Db2SourceSpan { start, end },
                false,
            );
        }
        let mut arguments = Vec::new();
        if self.take_symbol(Db2Symbol::Multiply) {
            let wildcard_span = self.tokens[self.position - 1].span;
            arguments.push(self.push(Db2ExpressionKind::Wildcard, wildcard_span, false)?);
            self.expect_symbol(Db2Symbol::RightParenthesis)?;
        } else if !self.take_symbol(Db2Symbol::RightParenthesis) {
            loop {
                if arguments.len() >= self.limits.max_list_items {
                    return Err(self.diagnostic_here(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "Db2 function exceeds the configured argument limit",
                    ));
                }
                arguments.push(self.parse_precedence(0)?);
                if self.take_symbol(Db2Symbol::RightParenthesis) {
                    break;
                }
                self.expect_symbol(Db2Symbol::Comma)?;
            }
        }
        let parameter = arguments
            .iter()
            .copied()
            .map(|argument| self.parameter_flag(argument))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|flag| flag);
        self.push(
            Db2ExpressionKind::Function { name, arguments },
            Db2SourceSpan {
                start,
                end: self.previous_end(),
            },
            parameter,
        )
    }

    fn parse_cast(&mut self) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        self.expect_word("CAST")?;
        self.expect_symbol(Db2Symbol::LeftParenthesis)?;
        let expression = self.parse_precedence(0)?;
        self.expect_word("AS")?;
        let data_type = self.parse_data_type()?;
        self.expect_symbol(Db2Symbol::RightParenthesis)?;
        let parameter = self.parameter_flag(expression)?;
        self.push(
            Db2ExpressionKind::Cast {
                expression,
                data_type,
            },
            Db2SourceSpan {
                start,
                end: self.previous_end(),
            },
            parameter,
        )
    }

    fn parse_case(&mut self) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        self.expect_word("CASE")?;
        let operand = if self.word() == Some("WHEN") {
            None
        } else {
            Some(self.parse_precedence(0)?)
        };
        let mut branches = Vec::new();
        while self.take_word("WHEN") {
            if branches.len() >= self.limits.max_list_items {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 CASE exceeds the configured branch limit",
                ));
            }
            let condition = self.parse_precedence(0)?;
            self.expect_word("THEN")?;
            let result = self.parse_precedence(0)?;
            branches.push((condition, result));
        }
        if branches.is_empty() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "Db2 CASE requires at least one WHEN branch",
            ));
        }
        let otherwise = if self.take_word("ELSE") {
            Some(self.parse_precedence(0)?)
        } else {
            None
        };
        self.expect_word("END")?;
        let mut parameter = operand
            .map(|value| self.parameter_flag(value))
            .transpose()?
            .unwrap_or(false);
        for (condition, result) in &branches {
            parameter |= self.parameter_flag(*condition)? || self.parameter_flag(*result)?;
        }
        if let Some(otherwise) = otherwise {
            parameter |= self.parameter_flag(otherwise)?;
        }
        self.push(
            Db2ExpressionKind::Case {
                operand,
                branches,
                otherwise,
            },
            Db2SourceSpan {
                start,
                end: self.previous_end(),
            },
            parameter,
        )
    }

    fn parse_data_type(&mut self) -> Result<Db2DataType, Db2SyntaxDiagnostic> {
        let first = self.word().map(str::to_owned).ok_or_else(|| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "CAST requires a Db2 data type",
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
            let with_time_zone = if self.take_word("WITH") {
                self.expect_word("TIME")?;
                self.expect_word("ZONE")?;
                true
            } else {
                if self.take_word("WITHOUT") {
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
        self.qualified_name().map(Db2DataType::Distinct)
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

    fn qualified_name(&mut self) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
        let mut parts = Vec::new();
        loop {
            if parts.len() >= self.limits.max_name_parts {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 qualified name exceeds the configured part limit",
                ));
            }
            parts.push(self.identifier("name part")?);
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

    fn take_host_identifier(
        &mut self,
        label: &str,
    ) -> Result<Db2HostIdentifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.tokens.get(self.position) else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("missing Db2 {label}"),
            ));
        };
        let Db2TokenKind::HostVariable(value) = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                format!("Db2 {label} must be preceded by colon"),
            ));
        };
        let identifier = self.host_identifier(value.clone(), token.span.start)?;
        self.position += 1;
        Ok(identifier)
    }

    fn host_identifier(
        &self,
        value: String,
        location: Db2SourceLocation,
    ) -> Result<Db2HostIdentifier, Db2SyntaxDiagnostic> {
        Db2HostIdentifier::new(value, self.limits).map_err(|problem| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                location,
                problem.message,
            )
        })
    }

    fn binary_operator(&self) -> Option<(Db2BinaryOperator, u8)> {
        match self.tokens.get(self.position).map(|token| &token.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) if value == "OR" => Some((Db2BinaryOperator::Or, 1)),
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) if value == "AND" => Some((Db2BinaryOperator::And, 2)),
            Some(Db2TokenKind::Symbol(Db2Symbol::Equal)) => Some((Db2BinaryOperator::Equal, 3)),
            Some(Db2TokenKind::Symbol(Db2Symbol::NotEqual)) => {
                Some((Db2BinaryOperator::NotEqual, 3))
            }
            Some(Db2TokenKind::Symbol(Db2Symbol::Less)) => Some((Db2BinaryOperator::Less, 3)),
            Some(Db2TokenKind::Symbol(Db2Symbol::LessOrEqual)) => {
                Some((Db2BinaryOperator::LessOrEqual, 3))
            }
            Some(Db2TokenKind::Symbol(Db2Symbol::Greater)) => Some((Db2BinaryOperator::Greater, 3)),
            Some(Db2TokenKind::Symbol(Db2Symbol::GreaterOrEqual)) => {
                Some((Db2BinaryOperator::GreaterOrEqual, 3))
            }
            Some(Db2TokenKind::Symbol(Db2Symbol::Plus)) => Some((Db2BinaryOperator::Add, 4)),
            Some(Db2TokenKind::Symbol(Db2Symbol::Minus)) => Some((Db2BinaryOperator::Subtract, 4)),
            Some(Db2TokenKind::Symbol(Db2Symbol::Multiply)) => {
                Some((Db2BinaryOperator::Multiply, 5))
            }
            Some(Db2TokenKind::Symbol(Db2Symbol::Divide)) => Some((Db2BinaryOperator::Divide, 5)),
            Some(Db2TokenKind::Symbol(Db2Symbol::Concatenate)) => {
                Some((Db2BinaryOperator::Concatenate, 5))
            }
            _ => None,
        }
    }

    fn push(
        &mut self,
        kind: Db2ExpressionKind,
        span: Db2SourceSpan,
        contains_parameter: bool,
    ) -> Result<Db2ExpressionId, Db2SyntaxDiagnostic> {
        let id = self.arena.push(kind, span).map_err(|problem| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.start,
                problem.message,
            )
        })?;
        self.contains_parameter.push(contains_parameter);
        Ok(id)
    }

    fn expression_span(&self, id: Db2ExpressionId) -> Result<Db2SourceSpan, Db2SyntaxDiagnostic> {
        self.arena.get(id).map(|node| node.span()).ok_or_else(|| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "Db2 expression identity is missing",
            )
        })
    }

    fn parameter_flag(&self, id: Db2ExpressionId) -> Result<bool, Db2SyntaxDiagnostic> {
        self.contains_parameter
            .get(id.index() as usize)
            .copied()
            .ok_or_else(|| {
                self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 expression parameter metadata is missing",
                )
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
        if self.word() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn word(&self) -> Option<&str> {
        match self.tokens.get(self.position).map(|token| &token.kind) {
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

    fn current_start(&self) -> Db2SourceLocation {
        self.tokens
            .get(self.position)
            .map_or(Db2SourceLocation::START, |token| token.span.start)
    }

    fn previous_start(&self) -> Db2SourceLocation {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(Db2SourceLocation::START, |token| token.span.start)
    }

    fn previous_end(&self) -> Db2SourceLocation {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(Db2SourceLocation::START, |token| token.span.end)
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
        Db2SyntaxDiagnostic::new(code, self.previous_start(), message)
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2ParsedExpression, Db2SyntaxDiagnostic> {
        parse_db2_expression(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    #[test]
    fn arithmetic_concat_comparison_and_boolean_precedence_is_owned() {
        let parsed = parse("A = 1 OR B = 2 + 3 * 4 || 'X' AND C <> 0").unwrap();
        let Db2ExpressionKind::Binary {
            operator: Db2BinaryOperator::Or,
            right,
            ..
        } = parsed.arena().get(parsed.root()).unwrap().kind()
        else {
            panic!("expected OR root")
        };
        assert!(matches!(
            parsed.arena().get(*right).unwrap().kind(),
            Db2ExpressionKind::Binary {
                operator: Db2BinaryOperator::And,
                ..
            }
        ));
    }

    #[test]
    fn not_binds_to_a_predicate_before_boolean_connectives() {
        let parsed = parse("NOT A = 1 AND B = 2").unwrap();
        let Db2ExpressionKind::Binary {
            left,
            operator: Db2BinaryOperator::And,
            ..
        } = parsed.arena().get(parsed.root()).unwrap().kind()
        else {
            panic!("expected AND root")
        };
        let Db2ExpressionKind::Unary {
            operator: Db2UnaryOperator::Not,
            operand,
        } = parsed.arena().get(*left).unwrap().kind()
        else {
            panic!("expected NOT predicate")
        };
        assert!(matches!(
            parsed.arena().get(*operand).unwrap().kind(),
            Db2ExpressionKind::Binary {
                operator: Db2BinaryOperator::Equal,
                ..
            }
        ));
    }

    #[test]
    fn qualified_columns_functions_cast_and_wildcard_are_typed() {
        let parsed = parse("COALESCE(S.T.C, CAST(AMOUNT AS DECIMAL(9,2)), COUNT(*))").unwrap();
        let Db2ExpressionKind::Function { arguments, .. } =
            parsed.arena().get(parsed.root()).unwrap().kind()
        else {
            panic!("expected function")
        };
        assert_eq!(arguments.len(), 3);
        assert!(matches!(
            parsed.arena().get(arguments[1]).unwrap().kind(),
            Db2ExpressionKind::Cast { .. }
        ));
        let Db2ExpressionKind::Function {
            arguments: count_arguments,
            ..
        } = parsed.arena().get(arguments[2]).unwrap().kind()
        else {
            panic!("expected COUNT")
        };
        assert!(matches!(
            parsed.arena().get(count_arguments[0]).unwrap().kind(),
            Db2ExpressionKind::Wildcard
        ));
    }

    #[test]
    fn host_indicators_and_parameter_markers_remain_distinct() {
        let host = parse(":Cobol-Field INDICATOR :Cobol-Ind").unwrap();
        let Db2ExpressionKind::HostVariable(reference) =
            host.arena().get(host.root()).unwrap().kind()
        else {
            panic!("expected host variable")
        };
        assert_eq!(reference.variable().value(), "Cobol-Field");
        assert_eq!(reference.indicator().unwrap().value(), "Cobol-Ind");
        assert!(matches!(
            parse("?").unwrap().arena().nodes()[0].kind(),
            Db2ExpressionKind::ParameterMarker
        ));
    }

    #[test]
    fn null_predicate_rejects_parameter_markers_at_any_depth() {
        let parsed = parse("AMOUNT IS NOT NULL").unwrap();
        assert!(matches!(
            parsed.arena().get(parsed.root()).unwrap().kind(),
            Db2ExpressionKind::IsNull { negated: true, .. }
        ));
        for source in ["? IS NULL", "(? + 1) IS NOT NULL"] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
    }

    #[test]
    fn simple_and_searched_case_are_bounded_owned_nodes() {
        for source in [
            "CASE CODE WHEN 1 THEN 'A' WHEN 2 THEN 'B' ELSE 'X' END",
            "CASE WHEN A IS NULL THEN 0 ELSE A END",
        ] {
            let parsed = parse(source).unwrap();
            assert!(matches!(
                parsed.arena().get(parsed.root()).unwrap().kind(),
                Db2ExpressionKind::Case { .. }
            ));
        }
    }

    #[test]
    fn malformed_and_bounded_expression_inputs_fail_closed() {
        for source in [
            "",
            "1 +",
            "F(1,)",
            "CAST(1 DECIMAL(9,2))",
            "CASE ELSE 1 END",
            "A B",
            "(SELECT 1)",
            "*",
            "1 + *",
            "F(*, 1)",
        ] {
            assert!(
                parse(source).is_err(),
                "unexpected expression success: {source}"
            );
        }
        let limits = Db2AstLimits {
            max_list_items: 1,
            ..Db2AstLimits::default()
        };
        assert!(parse_db2_expression("F(1,2)", Db2SyntaxLimits::default(), limits).is_err());
        let limits = Db2AstLimits {
            max_expression_depth: 2,
            ..Db2AstLimits::default()
        };
        assert!(parse_db2_expression("---1", Db2SyntaxLimits::default(), limits).is_err());
    }
}
