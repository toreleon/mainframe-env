use super::*;

impl<'a> CreateTableParser<'a> {
    pub(super) fn column_definition(
        &mut self,
    ) -> Result<Db2CreateTableColumn, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        let start_byte = self.tokens[self.position].span.start_byte;
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
                    matches!(clause.value(), Some(Db2Literal::Null))
                }) {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "NOT NULL conflicts with DEFAULT NULL",
                    ));
                }
                not_null = true;
                continue;
            }

            let clause_start = self.tokens[self.position].span;
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
                let clause = self.default_clause(spelling, clause_start)?;
                if not_null && matches!(clause.value(), Some(Db2Literal::Null)) {
                    return Err(self.diagnostic_previous(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        "DEFAULT NULL conflicts with NOT NULL",
                    ));
                }
                default = Some(clause);
                continue;
            }

            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "column clause is outside NOT NULL and type/constant/NULL DEFAULT syntax",
            ));
        }

        Ok(Db2CreateTableColumn {
            name,
            data_type,
            not_null,
            default,
            span: Db2SourceSpan {
                start_byte,
                end_byte: self.tokens[self.position - 1].span.end_byte,
                start,
                end: self.previous_end(),
            },
        })
    }

    fn default_clause(
        &mut self,
        spelling: Db2DefaultSpelling,
        start: Db2SourceSpan,
    ) -> Result<Db2ColumnDefault, Db2SyntaxDiagnostic> {
        let mut clause = Db2ColumnDefault {
            spelling,
            value: None,
            span: Db2SourceSpan {
                end_byte: self.tokens[self.position - 1].span.end_byte,
                end: self.previous_end(),
                ..start
            },
            value_span: None,
            numeric_sign_span: None,
            numeric_token_span: None,
        };
        // Only element boundaries or the next admitted option may end a
        // type-default clause. Duplicate options are diagnosed by the caller.
        if self.at_element_end()
            || matches!(self.word(), Some("NOT" | "DEFAULT"))
            || (self.word() == Some("WITH") && self.word_at(self.position + 1) == Some("DEFAULT"))
        {
            return Ok(clause);
        }
        let operand_start = self.tokens[self.position].span;
        if self.take_word("NULL") {
            clause.value = Some(Db2Literal::Null);
            clause.value_span = Some(operand_start);
            clause.span.end_byte = operand_start.end_byte;
            clause.span.end = operand_start.end;
            return Ok(clause);
        }
        let sign = if self.take_symbol(Db2Symbol::Plus) {
            Some('+')
        } else if self.take_symbol(Db2Symbol::Minus) {
            Some('-')
        } else {
            None
        };
        if sign.is_some() {
            clause.numeric_sign_span = Some(operand_start);
        }
        let Some(token) = self.tokens.get(self.position) else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "DEFAULT requires a constant or NULL operand",
            ));
        };
        let literal = match &token.kind {
            Db2TokenKind::Number(value) => {
                clause.numeric_token_span = Some(token.span);
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
        clause.value = Some(literal);
        clause.value_span = Some(Db2SourceSpan {
            end_byte: token.span.end_byte,
            end: token.span.end,
            ..operand_start
        });
        clause.span.end_byte = token.span.end_byte;
        clause.span.end = token.span.end;
        self.position += 1;
        Ok(clause)
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
            let has_time_zone_clause = (self.word() == Some("WITH")
                && self.word_at(self.position + 1) == Some("TIME"))
                || (self.word() == Some("WITHOUT")
                    && self.word_at(self.position + 1) == Some("TIME"));
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
            let valid_arguments = match kind {
                Db2BuiltInType::SmallInt
                | Db2BuiltInType::Integer
                | Db2BuiltInType::BigInt
                | Db2BuiltInType::Real
                | Db2BuiltInType::Double
                | Db2BuiltInType::Date
                | Db2BuiltInType::Time
                | Db2BuiltInType::RowId
                | Db2BuiltInType::Xml => arguments.is_empty(),
                Db2BuiltInType::VarChar
                | Db2BuiltInType::VarGraphic
                | Db2BuiltInType::VarBinary => arguments.len() == 1,
                Db2BuiltInType::Decimal => arguments.len() <= 2,
                Db2BuiltInType::DecFloat => {
                    arguments.is_empty() || matches!(arguments.as_slice(), [16] | [34])
                }
                _ => arguments.len() <= 1,
            };
            if !valid_arguments
                || (has_time_zone_clause
                    && !matches!(kind, Db2BuiltInType::Timestamp | Db2BuiltInType::Time))
            {
                return Err(self.diagnostic_previous(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 data-type arguments or time-zone clause are outside the common syntax",
                ));
            }
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
}
