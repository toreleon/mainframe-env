//! Public partial SQL0072/SQL0105 syntax contracts, not catalog or execution proof.

use mainframe_env_db2::{
    Db2AstLimits, Db2DropAliasDesignator, Db2DropObjectKind, Db2RenameObjectKind,
    Db2SourceLocation, Db2SourceSpan, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    parse_db2_drop_statement, parse_db2_rename_statement,
};

fn location(source: &str, byte: usize) -> Db2SourceLocation {
    let mut result = Db2SourceLocation { line: 1, column: 1 };
    let mut previous_cr = false;
    for character in source[..byte].chars() {
        match character {
            '\r' => {
                result.line += 1;
                result.column = 1;
            }
            '\n' if previous_cr => {}
            '\n' => {
                result.line += 1;
                result.column = 1;
            }
            _ => result.column += 1,
        }
        previous_cr = character == '\r';
    }
    result
}

fn assert_span(source: &str, span: Db2SourceSpan, expected: &str) {
    assert_eq!(&source[span.start_byte..span.end_byte], expected);
    assert_eq!(span.start, location(source, span.start_byte));
    assert_eq!(span.end, location(source, span.end_byte));
}

#[test]
fn drop_object_kinds_preserve_source_qualification_and_alias_spelling() {
    for (keyword, kind, name) in [
        ("TABLE", Db2DropObjectKind::Table, "L.S.T"),
        ("VIEW", Db2DropObjectKind::View, "L.S.V"),
        ("INDEX", Db2DropObjectKind::Index, "S.I"),
        ("ALIAS", Db2DropObjectKind::Alias, "L.S.A"),
    ] {
        let source = format!("DROP {keyword} {name};");
        let statement =
            parse_db2_drop_statement(&source, Default::default(), Default::default()).unwrap();
        assert_eq!(statement.object_kind(), kind);
        assert_span(&source, statement.object_name().span(), name);
        assert_span(&source, statement.span(), &source);
        assert_eq!(
            statement.object_name().name().parts().len(),
            statement.object_name().part_spans().len()
        );
        assert_eq!(
            statement.alias_designator(),
            (kind == Db2DropObjectKind::Alias).then_some(Db2DropAliasDesignator::Unspecified)
        );
        assert!(statement.alias_designator_span().is_none());
    }
    let source = "DROP ALIAS S.A FOR /* context not binding */ TABLE";
    let statement =
        parse_db2_drop_statement(source, Default::default(), Default::default()).unwrap();
    assert_eq!(
        statement.alias_designator(),
        Some(Db2DropAliasDesignator::ForTable)
    );
    assert_span(
        source,
        statement.alias_designator_span().unwrap(),
        "FOR /* context not binding */ TABLE",
    );
}

#[test]
fn rename_keeps_source_name_separate_from_unqualified_destination() {
    for (keyword, kind, name) in [
        ("TABLE", Db2RenameObjectKind::Table, "L.S.T"),
        ("INDEX", Db2RenameObjectKind::Index, "S.I"),
    ] {
        let source = format!("RENAME {keyword} {name} TO \"New\";");
        let statement =
            parse_db2_rename_statement(&source, Default::default(), Default::default()).unwrap();
        assert_eq!(statement.object_kind(), kind);
        assert_span(&source, statement.source_span(), name);
        assert_eq!(statement.destination_identifier().value(), "New");
        assert_span(&source, statement.destination_span(), "\"New\"");
        assert_span(&source, statement.span(), &source);
        assert_eq!(
            statement.source_name().parts().len(),
            statement.source_part_spans().len()
        );
    }
    // Syntax must not fabricate a catalog-conflict check from spelling alone.
    assert!(
        parse_db2_rename_statement(
            "RENAME TABLE T TO T",
            Default::default(),
            Default::default()
        )
        .is_ok()
    );
}

#[test]
fn decoded_identifiers_and_every_span_survive_utf8_crlf_relocation() {
    let source = "/*é*/\r\nDROP ALIAS \"Sché\".\"名\"\"x  \" FOR TABLE; -- tail";
    let drop = parse_db2_drop_statement(source, Default::default(), Default::default()).unwrap();
    assert_eq!(drop.object_name().name().parts()[1].value(), "名\"x");
    assert_span(source, drop.object_name().span(), "\"Sché\".\"名\"\"x  \"");
    for (span, text) in drop
        .object_name()
        .part_spans()
        .iter()
        .zip(["\"Sché\"", "\"名\"\"x  \""])
    {
        assert_span(source, *span, text);
    }
    assert_span(source, drop.alias_designator_span().unwrap(), "FOR TABLE");
    assert_span(
        source,
        drop.span(),
        "DROP ALIAS \"Sché\".\"名\"\"x  \" FOR TABLE;",
    );
    let source = "/*é*/\r\nRENAME TABLE \"Sché\".\"名\"\"x  \" TO \"新\"\"名  \"; -- tail";
    let rename =
        parse_db2_rename_statement(source, Default::default(), Default::default()).unwrap();
    assert_eq!(rename.source_name().parts()[1].value(), "名\"x");
    assert_eq!(rename.destination_identifier().value(), "新\"名");
    assert_span(source, rename.source_span(), "\"Sché\".\"名\"\"x  \"");
    for (span, text) in rename
        .source_part_spans()
        .iter()
        .zip(["\"Sché\"", "\"名\"\"x  \""])
    {
        assert_span(source, *span, text);
    }
    assert_span(source, rename.destination_span(), "\"新\"\"名  \"");
    assert_span(
        source,
        rename.span(),
        "RENAME TABLE \"Sché\".\"名\"\"x  \" TO \"新\"\"名  \";",
    );
}

#[test]
fn public_outputs_own_names_after_source_is_dropped() {
    let drop = {
        let source = String::from("DROP ALIAS \"a\"\"b\"");
        parse_db2_drop_statement(&source, Default::default(), Default::default()).unwrap()
    };
    let rename = {
        let source = String::from("RENAME INDEX S.I TO \"a\"\"b\"");
        parse_db2_rename_statement(&source, Default::default(), Default::default()).unwrap()
    };
    assert_eq!(drop.clone().object_name().name().parts()[0].value(), "a\"b");
    assert_eq!(rename.clone().destination_identifier().value(), "a\"b");
}

#[test]
fn public_parsers_reject_deferred_clauses_names_and_extra_statements() {
    for source in [
        "DROP PUBLIC ALIAS A FOR SEQUENCE",
        "DROP ALIAS A FOR SEQUENCE",
        "DROP SEQUENCE S",
        "DROP TABLE IF EXISTS T",
        "DROP TABLE T CASCADE",
        "DROP TABLE T RESTRICT",
        "DROP INDEX L.S.I",
        "DROP TABLE L.S.T.X",
        "DROP TABLE :Host",
        "DROP TABLE T; DROP TABLE U",
        "DROP TABLE T;;",
    ] {
        assert!(
            parse_db2_drop_statement(source, Default::default(), Default::default()).is_err(),
            "{source}"
        );
    }
    for source in [
        "RENAME VIEW V TO N",
        "RENAME ALIAS A TO N",
        "RENAME TABLE T TO S.N",
        "RENAME INDEX L.S.I TO N",
        "RENAME TABLE L.S.T.X TO N",
        "RENAME TABLE T TO :Host",
        "RENAME TABLE T TO N CASCADE",
        "RENAME TABLE T TO N; RENAME TABLE U TO M",
        "RENAME TABLE T TO N;;",
    ] {
        assert!(
            parse_db2_rename_statement(source, Default::default(), Default::default()).is_err(),
            "{source}"
        );
    }
    let source = "/*é*/\r\nRENAME TABLE T TO S.N";
    let error =
        parse_db2_rename_statement(source, Default::default(), Default::default()).unwrap_err();
    assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
    assert_eq!(error.location, location(source, source.find('.').unwrap()));
}

#[test]
fn public_resource_bounds_measure_effective_names_and_original_input() {
    let ast = Db2AstLimits {
        max_identifier_bytes: 4,
        max_name_parts: 2,
        ..Default::default()
    };
    for name in ["ABCD", "\"ab\"\"c  \"", "\"éé\""] {
        assert!(
            parse_db2_drop_statement(&format!("DROP TABLE S.{name}"), Default::default(), ast)
                .is_ok()
        );
        assert!(
            parse_db2_rename_statement(
                &format!("RENAME TABLE S.{name} TO {name}"),
                Default::default(),
                ast
            )
            .is_ok()
        );
    }
    for name in ["ABCDE", "\"ab\"\"cd\"", "\"ééa\""] {
        assert!(
            parse_db2_drop_statement(&format!("DROP TABLE {name}"), Default::default(), ast)
                .is_err()
        );
        assert!(
            parse_db2_rename_statement(
                &format!("RENAME TABLE T TO {name}"),
                Default::default(),
                ast
            )
            .is_err()
        );
    }
    assert!(parse_db2_drop_statement("DROP TABLE L.S.T", Default::default(), ast).is_err());
    assert!(
        parse_db2_rename_statement("RENAME TABLE L.S.T TO N", Default::default(), ast).is_err()
    );
    for (source, tokens, rename) in [
        ("DROP TABLE S.T;", 6, false),
        ("RENAME TABLE S.T TO N;", 8, true),
    ] {
        for (bytes, count, accepted) in [
            (source.len(), tokens, true),
            (source.len() - 1, tokens, false),
            (source.len(), tokens - 1, false),
        ] {
            let limits = Db2SyntaxLimits {
                max_statement_bytes: bytes,
                max_tokens: count,
                ..Default::default()
            };
            let success = if rename {
                parse_db2_rename_statement(source, limits, Default::default()).is_ok()
            } else {
                parse_db2_drop_statement(source, limits, Default::default()).is_ok()
            };
            assert_eq!(success, accepted, "{source}");
        }
    }
}
