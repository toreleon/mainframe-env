use super::*;

impl<'a> CreateTableParser<'a> {
    pub(super) fn table_constraint(
        &mut self,
    ) -> Result<Db2CreateTableConstraint, Db2SyntaxDiagnostic> {
        let start = self.current_start();
        let start_byte = self.tokens[self.position].span.start_byte;
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
                start_byte,
                end_byte: self.tokens[self.position - 1].span.end_byte,
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

    pub(super) fn validate_constraints(
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

    pub(super) fn starts_table_constraint(&self) -> bool {
        matches!(
            self.word(),
            Some("CONSTRAINT" | "PRIMARY" | "UNIQUE" | "FOREIGN")
        )
    }
}
