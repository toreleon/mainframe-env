use mainframe_env_db2::{
    Db2AstErrorCode, Db2AstLimits, Db2ExpressionArena, Db2ExpressionKind, Db2Identifier,
    Db2Literal, Db2SyntaxLimits, Db2TokenKind, lex_db2,
};

#[test]
fn owned_ast_keeps_d2_token_span_after_tokens_are_dropped() {
    let lexed = lex_db2("Amount", Db2SyntaxLimits::default()).unwrap();
    let token = &lexed.tokens()[0];
    let Db2TokenKind::Word { value, delimited } = &token.kind else {
        panic!("expected an identifier token");
    };
    let identifier =
        Db2Identifier::new(value.clone(), *delimited, Db2AstLimits::default()).unwrap();
    let span = token.span;
    drop(lexed);

    let mut arena = Db2ExpressionArena::new(Db2AstLimits::default()).unwrap();
    let id = arena
        .push(
            Db2ExpressionKind::Literal(Db2Literal::Number("1".into())),
            span,
        )
        .unwrap();
    assert_eq!(identifier.value(), "AMOUNT");
    assert_eq!(arena.get(id).unwrap().span(), span);
    assert_eq!(span.start_byte, 0);
    assert_eq!(span.end_byte, 6);
}

#[test]
fn invalid_ast_limits_fail_before_an_arena_is_created() {
    let limits = Db2AstLimits {
        max_expression_nodes: 0,
        ..Db2AstLimits::default()
    };
    assert_eq!(
        Db2ExpressionArena::new(limits).unwrap_err().code,
        Db2AstErrorCode::InvalidLimits
    );
}
