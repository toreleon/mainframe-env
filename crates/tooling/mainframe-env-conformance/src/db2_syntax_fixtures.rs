//! Independent authored syntax expectations, not an official row/obligation map.
//!
//! Baseline ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! SQL0072 db2z_sql_drop.html: 320964 bytes,
//! 5f75fdde9c96290ba968fb9b2629ca73d9274de9d8b8bd1002e56c341cd446a0;
//! SQL0105 db2z_sql_rename.html: 25184 bytes,
//! ea6063fba847a91a891db54d4b1741f6dc379a7dbf61c8f15069867365f63984.
//! Identifier/naming/qualification context (no separate statement rows):
//! db2z_sqlidentifiers.html: 11729 bytes,
//! d5c99a5640234e19a310d2e44f1abd9bbe3bf9c0fea1cf87c06b24ee9f4a1f8b;
//! db2z_namingconventions.html: 31731 bytes,
//! 98dde222f9aaa559b19956fedefa9badad95242094da2eafedba310813c2c65f;
//! db2z_resolutionofobjnames.html: 14246 bytes,
//! a1e2b49f72cd742e3d5a4866417ba9acf30dccca8091d0b48a1483ec50019f8d.
//! Matching archive bodies were verified after retained paths were absent;
//! ordinary reading is TOC-blocked. No refresh or licensed evidence is inferred.
//!
//! Literal field values and endpoints below are authored independently of the
//! product parser, lexer and qualification APIs. Builders only assemble DTOs.
//! Resource/error fixtures describe product assurance, not IBM SQLCA behavior.

use crate::db2_syntax::{Component, Designator, Kind, Name, Outcome, Route, Span};
use mainframe_env_db2::{Db2AstLimits, Db2SyntaxLimits};

pub(crate) struct Fixture {
    pub id: &'static str,
    pub source: &'static str,
    pub syntax: Db2SyntaxLimits,
    pub ast: Db2AstLimits,
    pub expected: Outcome,
}

pub(crate) fn group(id: &str) -> Option<(Route, Vec<Fixture>)> {
    match id {
        "db2.syntax.drop.structure" => Some((Route::Drop, drop_structure())),
        "db2.syntax.rename.structure" => Some((Route::Rename, rename_structure())),
        "db2.syntax.drop.bounds" => Some((Route::Drop, bounds(Route::Drop))),
        "db2.syntax.rename.bounds" => Some((Route::Rename, bounds(Route::Rename))),
        "db2.syntax.drop.limitations" => Some((Route::Drop, limitations())),
        _ => None,
    }
}

fn span(start_byte: usize, end_byte: usize, start: (u32, u32), end: (u32, u32)) -> Span {
    Span {
        start_byte,
        end_byte,
        start_line: start.0,
        start_column: start.1,
        end_line: end.0,
        end_column: end.1,
    }
}

// Single-line ASCII endpoints; no source inspection or product location helper.
fn ascii(start: usize, end: usize) -> Span {
    span(start, end, (1, start as u32 + 1), (1, end as u32 + 1))
}

fn part(value: &str, delimited: bool, span: Span) -> Component {
    Component {
        value: value.into(),
        delimited,
        span,
    }
}

fn name(components: Vec<Component>, span: Span) -> Name {
    Name { components, span }
}

fn fixture(id: &'static str, source: &'static str, expected: Outcome) -> Fixture {
    Fixture {
        id,
        source,
        syntax: Db2SyntaxLimits::default(),
        ast: Db2AstLimits::default(),
        expected,
    }
}

fn drop_case(
    id: &'static str,
    source: &'static str,
    kind: Kind,
    name: Name,
    designator: Designator,
    span: Span,
) -> Fixture {
    fixture(
        id,
        source,
        Outcome::Drop {
            kind,
            name,
            designator,
            span,
        },
    )
}

fn rename_case(
    id: &'static str,
    source: &'static str,
    kind: Kind,
    source_name: Name,
    destination: Component,
    span: Span,
) -> Fixture {
    fixture(
        id,
        source,
        Outcome::Rename {
            kind,
            source: source_name,
            destination,
            span,
        },
    )
}

fn error(id: &'static str, source: &'static str, code: &str, line: u32, column: u32) -> Fixture {
    fixture(
        id,
        source,
        Outcome::Diagnostic {
            code: code.into(),
            line,
            column,
        },
    )
}

fn drop_structure() -> Vec<Fixture> {
    vec![
        drop_case(
            "table-one",
            "DROP TABLE t",
            Kind::Table,
            name(vec![part("T", false, ascii(11, 12))], ascii(11, 12)),
            Designator::NotAlias,
            ascii(0, 12),
        ),
        drop_case(
            "table-two",
            "DROP TABLE s.t",
            Kind::Table,
            name(
                vec![
                    part("S", false, ascii(11, 12)),
                    part("T", false, ascii(13, 14)),
                ],
                ascii(11, 14),
            ),
            Designator::NotAlias,
            ascii(0, 14),
        ),
        drop_case(
            "table-three",
            "DROP TABLE l.s.t",
            Kind::Table,
            name(
                vec![
                    part("L", false, ascii(11, 12)),
                    part("S", false, ascii(13, 14)),
                    part("T", false, ascii(15, 16)),
                ],
                ascii(11, 16),
            ),
            Designator::NotAlias,
            ascii(0, 16),
        ),
        drop_case(
            "view-one",
            "DROP VIEW v",
            Kind::View,
            name(vec![part("V", false, ascii(10, 11))], ascii(10, 11)),
            Designator::NotAlias,
            ascii(0, 11),
        ),
        drop_case(
            "view-two",
            "DROP VIEW s.v;",
            Kind::View,
            name(
                vec![
                    part("S", false, ascii(10, 11)),
                    part("V", false, ascii(12, 13)),
                ],
                ascii(10, 13),
            ),
            Designator::NotAlias,
            ascii(0, 14),
        ),
        drop_case(
            "view-three",
            "DROP VIEW l.s.v",
            Kind::View,
            name(
                vec![
                    part("L", false, ascii(10, 11)),
                    part("S", false, ascii(12, 13)),
                    part("V", false, ascii(14, 15)),
                ],
                ascii(10, 15),
            ),
            Designator::NotAlias,
            ascii(0, 15),
        ),
        drop_case(
            "index-one",
            "DROP INDEX i",
            Kind::Index,
            name(vec![part("I", false, ascii(11, 12))], ascii(11, 12)),
            Designator::NotAlias,
            ascii(0, 12),
        ),
        drop_case(
            "index-two",
            "DROP INDEX s.i",
            Kind::Index,
            name(
                vec![
                    part("S", false, ascii(11, 12)),
                    part("I", false, ascii(13, 14)),
                ],
                ascii(11, 14),
            ),
            Designator::NotAlias,
            ascii(0, 14),
        ),
        drop_case(
            "alias-omitted",
            "DROP ALIAS a",
            Kind::Alias,
            name(vec![part("A", false, ascii(11, 12))], ascii(11, 12)),
            Designator::Omitted,
            ascii(0, 12),
        ),
        drop_case(
            "alias-table",
            "DROP ALIAS s.a FOR TABLE;",
            Kind::Alias,
            name(
                vec![
                    part("S", false, ascii(11, 12)),
                    part("A", false, ascii(13, 14)),
                ],
                ascii(11, 14),
            ),
            Designator::ForTable {
                span: ascii(15, 24),
            },
            ascii(0, 25),
        ),
        drop_case(
            "alias-three",
            "DROP ALIAS l.s.a",
            Kind::Alias,
            name(
                vec![
                    part("L", false, ascii(11, 12)),
                    part("S", false, ascii(13, 14)),
                    part("A", false, ascii(15, 16)),
                ],
                ascii(11, 16),
            ),
            Designator::Omitted,
            ascii(0, 16),
        ),
        drop_case(
            "quoted-lower",
            "DROP TABLE \"t\"",
            Kind::Table,
            name(vec![part("t", true, ascii(11, 14))], ascii(11, 14)),
            Designator::NotAlias,
            ascii(0, 14),
        ),
        drop_case(
            "leading-escape-trailing",
            "DROP TABLE \" a\"\"B  \"",
            Kind::Table,
            name(vec![part(" a\"B", true, ascii(11, 20))], ascii(11, 20)),
            Designator::NotAlias,
            ascii(0, 20),
        ),
        drop_case(
            "decode-once",
            "DROP TABLE \"a\"\"\"\"b\"",
            Kind::Table,
            name(vec![part("a\"\"b", true, ascii(11, 19))], ascii(11, 19)),
            Designator::NotAlias,
            ascii(0, 19),
        ),
        drop_case(
            "utf8-crlf",
            "/*é*/\r\nDROP ALIAS \"a\"\"b  \" FOR TABLE;\r\n-- tail",
            Kind::Alias,
            name(
                vec![part("a\"b", true, span(19, 27, (2, 12), (2, 20)))],
                span(19, 27, (2, 12), (2, 20)),
            ),
            Designator::ForTable {
                span: span(28, 37, (2, 21), (2, 30)),
            },
            span(8, 38, (2, 1), (2, 31)),
        ),
        drop_case(
            "designator-comment",
            " DROP ALIAS a FOR/*x*/TABLE; ",
            Kind::Alias,
            name(vec![part("A", false, ascii(12, 13))], ascii(12, 13)),
            Designator::ForTable {
                span: ascii(14, 27),
            },
            ascii(1, 28),
        ),
        error("missing-name", "DROP TABLE", "MissingToken", 1, 11),
        error("missing-component", "DROP TABLE s.", "MissingToken", 1, 14),
        error(
            "malformed-qualification",
            "DROP TABLE s..t",
            "UnexpectedToken",
            1,
            14,
        ),
        error(
            "index-three-rejected",
            "DROP INDEX l.s.i",
            "UnsupportedStatement",
            1,
            16,
        ),
        error(
            "table-four-rejected",
            "DROP TABLE l.s.t.u",
            "UnsupportedStatement",
            1,
            18,
        ),
        error(
            "extra-clause",
            "DROP TABLE t CASCADE",
            "UnsupportedStatement",
            1,
            14,
        ),
        error(
            "extra-sql",
            "DROP TABLE t; DROP VIEW v",
            "UnexpectedToken",
            1,
            15,
        ),
        error(
            "extra-semicolon",
            "DROP TABLE t;;",
            "UnexpectedToken",
            1,
            14,
        ),
    ]
}

fn rename_structure() -> Vec<Fixture> {
    // Raise the configured bound so this case isolates the source grammar's
    // three-part ceiling, rather than the narrower configured-name diagnostic.
    let mut table_four = error(
        "table-four-rejected",
        "RENAME TABLE l.s.t.u TO n",
        "UnsupportedStatement",
        1,
        20,
    );
    table_four.ast.max_name_parts = 4;
    vec![
        rename_case(
            "table-one",
            "RENAME TABLE t TO n",
            Kind::Table,
            name(vec![part("T", false, ascii(13, 14))], ascii(13, 14)),
            part("N", false, ascii(18, 19)),
            ascii(0, 19),
        ),
        rename_case(
            "table-two",
            "RENAME TABLE s.t TO n",
            Kind::Table,
            name(
                vec![
                    part("S", false, ascii(13, 14)),
                    part("T", false, ascii(15, 16)),
                ],
                ascii(13, 16),
            ),
            part("N", false, ascii(20, 21)),
            ascii(0, 21),
        ),
        rename_case(
            "table-three",
            "RENAME TABLE l.s.t TO n",
            Kind::Table,
            name(
                vec![
                    part("L", false, ascii(13, 14)),
                    part("S", false, ascii(15, 16)),
                    part("T", false, ascii(17, 18)),
                ],
                ascii(13, 18),
            ),
            part("N", false, ascii(22, 23)),
            ascii(0, 23),
        ),
        rename_case(
            "index-one",
            "RENAME INDEX i TO n",
            Kind::Index,
            name(vec![part("I", false, ascii(13, 14))], ascii(13, 14)),
            part("N", false, ascii(18, 19)),
            ascii(0, 19),
        ),
        rename_case(
            "index-two",
            "RENAME INDEX s.i TO n;",
            Kind::Index,
            name(
                vec![
                    part("S", false, ascii(13, 14)),
                    part("I", false, ascii(15, 16)),
                ],
                ascii(13, 16),
            ),
            part("N", false, ascii(20, 21)),
            ascii(0, 22),
        ),
        rename_case(
            "quoted-case-trailing",
            "RENAME TABLE \"T\" TO \"n  \"",
            Kind::Table,
            name(vec![part("T", true, ascii(13, 16))], ascii(13, 16)),
            part("n", true, ascii(20, 25)),
            ascii(0, 25),
        ),
        rename_case(
            "leading-escape",
            "RENAME TABLE t TO \" n\"\"X  \"",
            Kind::Table,
            name(vec![part("T", false, ascii(13, 14))], ascii(13, 14)),
            part(" n\"X", true, ascii(18, 27)),
            ascii(0, 27),
        ),
        rename_case(
            "utf8-crlf",
            "/*é*/\r\nRENAME TABLE \"é\".t TO \"名\"; -- tail",
            Kind::Table,
            name(
                vec![
                    part("é", true, span(21, 25, (2, 14), (2, 17))),
                    part("T", false, span(26, 27, (2, 18), (2, 19))),
                ],
                span(21, 27, (2, 14), (2, 19)),
            ),
            part("名", true, span(31, 36, (2, 23), (2, 26))),
            span(8, 37, (2, 1), (2, 27)),
        ),
        rename_case(
            "whitespace-comment",
            " \tRENAME/*x*/TABLE t TO n; ",
            Kind::Table,
            name(vec![part("T", false, ascii(19, 20))], ascii(19, 20)),
            part("N", false, ascii(24, 25)),
            ascii(2, 26),
        ),
        error("missing-to", "RENAME TABLE t n", "MissingToken", 1, 16),
        error(
            "missing-destination",
            "RENAME TABLE t TO",
            "MissingToken",
            1,
            18,
        ),
        error("missing-source", "RENAME TABLE", "MissingToken", 1, 13),
        error(
            "malformed-qualification",
            "RENAME TABLE s..t TO n",
            "UnexpectedToken",
            1,
            16,
        ),
        error(
            "qualified-destination",
            "RENAME TABLE t TO s.n",
            "InvalidStatementOperand",
            1,
            20,
        ),
        error(
            "relocated-diagnostic",
            "/*é*/\r\nRENAME TABLE t TO s.n",
            "InvalidStatementOperand",
            2,
            20,
        ),
        error(
            "index-three-rejected",
            "RENAME INDEX l.s.i TO n",
            "UnsupportedStatement",
            1,
            18,
        ),
        table_four,
        error(
            "extra-clause",
            "RENAME TABLE t TO n RESTRICT",
            "UnsupportedStatement",
            1,
            21,
        ),
        error(
            "extra-sql",
            "RENAME TABLE t TO n; DROP TABLE x",
            "UnexpectedToken",
            1,
            22,
        ),
        error(
            "extra-semicolon",
            "RENAME TABLE t TO n;;",
            "UnexpectedToken",
            1,
            21,
        ),
    ]
}

fn simple(route: Route, id: &'static str) -> Fixture {
    match route {
        Route::Drop => drop_case(
            id,
            "DROP TABLE t",
            Kind::Table,
            name(vec![part("T", false, ascii(11, 12))], ascii(11, 12)),
            Designator::NotAlias,
            ascii(0, 12),
        ),
        Route::Rename => rename_case(
            id,
            "RENAME TABLE t TO n",
            Kind::Table,
            name(vec![part("T", false, ascii(13, 14))], ascii(13, 14)),
            part("N", false, ascii(18, 19)),
            ascii(0, 19),
        ),
    }
}

fn bounds(route: Route) -> Vec<Fixture> {
    let (source, bytes, tokens, token_column, name_column) = match route {
        Route::Drop => ("DROP TABLE t", 12, 3, 12, 12),
        Route::Rename => ("RENAME TABLE t TO n", 19, 5, 19, 14),
    };
    let mut input_exact = simple(route, "input-exact");
    input_exact.syntax.max_statement_bytes = bytes;
    let mut input_over = error("input-one-beyond", source, "StatementTooLarge", 1, 1);
    input_over.syntax.max_statement_bytes = bytes - 1;
    let mut tokens_exact = simple(route, "tokens-exact");
    tokens_exact.syntax.max_tokens = tokens;
    let mut tokens_over = error(
        "tokens-one-beyond",
        source,
        "TooManyTokens",
        1,
        token_column,
    );
    tokens_over.syntax.max_tokens = tokens - 1;
    let mut raw_exact = simple(route, "raw-token-exact");
    raw_exact.syntax.max_token_bytes = if route == Route::Drop { 5 } else { 6 };
    let mut raw_over = error(
        "raw-token-one-beyond",
        source,
        "TokenTooLarge",
        1,
        if route == Route::Drop { 6 } else { 1 },
    );
    raw_over.syntax.max_token_bytes = if route == Route::Drop { 4 } else { 5 };
    let mut name_exact = match route {
        Route::Drop => drop_case(
            "effective-name-exact",
            "DROP TABLE \"a\"\"b  \"",
            Kind::Table,
            name(vec![part("a\"b", true, ascii(11, 19))], ascii(11, 19)),
            Designator::NotAlias,
            ascii(0, 19),
        ),
        Route::Rename => rename_case(
            "effective-name-exact",
            "RENAME TABLE \"a\"\"b  \" TO \"n  \"",
            Kind::Table,
            name(vec![part("a\"b", true, ascii(13, 21))], ascii(13, 21)),
            part("n", true, ascii(25, 30)),
            ascii(0, 30),
        ),
    };
    name_exact.ast.max_identifier_bytes = 3;
    let mut name_over = error(
        "effective-name-one-beyond",
        name_exact.source,
        "InvalidStatementOperand",
        1,
        name_column,
    );
    name_over.ast.max_identifier_bytes = 2;
    let mut parts_exact = simple(route, "parts-exact");
    parts_exact.ast.max_name_parts = 1;
    let mut parts_over = error(
        "parts-one-beyond",
        if route == Route::Drop {
            "DROP TABLE s.t"
        } else {
            "RENAME TABLE s.t TO n"
        },
        "InvalidStatementOperand",
        1,
        if route == Route::Drop { 14 } else { 16 },
    );
    parts_over.ast.max_name_parts = 1;
    let mut tiny_unused = simple(route, "no-expression-budgets");
    tiny_unused.ast.max_expression_nodes = 1;
    tiny_unused.ast.max_list_items = 1;
    tiny_unused.ast.max_expression_depth = 1;
    // These parsers construct no expression/list arena: no aggregate expression
    // or backend claim follows from this valid, minimal configuration.
    let mut invalid_syntax = error("invalid-syntax-limits", source, "InvalidLimits", 1, 1);
    invalid_syntax.syntax.max_tokens = 0;
    let mut invalid_ast = error("invalid-ast-limits", source, "InvalidLimits", 1, 1);
    invalid_ast.ast.max_expression_nodes = 0;
    vec![
        input_exact,
        input_over,
        tokens_exact,
        tokens_over,
        raw_exact,
        raw_over,
        name_exact,
        name_over,
        parts_exact,
        parts_over,
        tiny_unused,
        invalid_syntax,
        invalid_ast,
    ]
}

// Officially legal forms outside the current product subset. Their rejection
// is a product limitation, not official recognized/validated/conditioned proof.
fn limitations() -> Vec<Fixture> {
    vec![
        error(
            "sequence",
            "DROP SEQUENCE s.q RESTRICT",
            "UnsupportedStatement",
            1,
            6,
        ),
        error(
            "alias-sequence",
            "DROP ALIAS a FOR SEQUENCE",
            "UnsupportedStatement",
            1,
            18,
        ),
        error(
            "public-alias",
            "DROP PUBLIC ALIAS a FOR SEQUENCE",
            "UnsupportedStatement",
            1,
            6,
        ),
    ]
}
