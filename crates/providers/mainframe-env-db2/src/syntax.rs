//! Bounded Db2 lexical boundary with no third-party public types.

use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Location, Span, Token, TokenWithSpan, Tokenizer, Whitespace};
use std::fmt;

const MAX_DIAGNOSTIC_BYTES: usize = 256;
const MAX_STATEMENT_BYTES_CEILING: usize = 8 * 1024 * 1024;
const MAX_TOKENS_CEILING: usize = 262_144;
const MAX_TOKEN_BYTES_CEILING: usize = 1024 * 1024;
const MAX_NESTING_CEILING: usize = 1024;

/// Resource limits applied before owned Db2 syntax is constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2SyntaxLimits {
    pub max_statement_bytes: usize,
    pub max_tokens: usize,
    pub max_token_bytes: usize,
    pub max_nesting: usize,
}

impl Default for Db2SyntaxLimits {
    fn default() -> Self {
        Self {
            max_statement_bytes: 1024 * 1024,
            max_tokens: 65_536,
            max_token_bytes: 64 * 1024,
            max_nesting: 128,
        }
    }
}

impl Db2SyntaxLimits {
    fn validate(self) -> Result<(), Db2SyntaxDiagnostic> {
        if self.max_statement_bytes == 0
            || self.max_statement_bytes > MAX_STATEMENT_BYTES_CEILING
            || self.max_tokens == 0
            || self.max_tokens > MAX_TOKENS_CEILING
            || self.max_token_bytes == 0
            || self.max_token_bytes > MAX_TOKEN_BYTES_CEILING
            || self.max_nesting == 0
            || self.max_nesting > MAX_NESTING_CEILING
        {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidLimits,
                Db2SourceLocation::START,
                "Db2 syntax limits are zero or exceed the compiled ceiling",
            ));
        }
        Ok(())
    }
}

/// One-based location in SQL source.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Db2SourceLocation {
    pub line: u32,
    pub column: u32,
}

impl Db2SourceLocation {
    pub const START: Self = Self { line: 1, column: 1 };
}

/// Half-open source span in line/column coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2SourceSpan {
    pub start: Db2SourceLocation,
    pub end: Db2SourceLocation,
}

/// Stable lexical failure classes. Exact SQLCA mapping is owned by a later
/// diagnostic slice and is intentionally not inferred from the substrate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SyntaxDiagnosticCode {
    EmptyStatement,
    InvalidLimits,
    StatementTooLarge,
    TooManyTokens,
    TokenTooLarge,
    InvalidCharacter,
    InvalidHostVariable,
    UnsupportedToken,
    UnbalancedDelimiter,
    TokenizerFailure,
    UnsupportedStatement,
    UnexpectedToken,
    MissingToken,
    DuplicateClause,
    InvalidStatementOperand,
}

/// Bounded owned diagnostic returned by the lexer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SyntaxDiagnostic {
    pub code: Db2SyntaxDiagnosticCode,
    pub location: Db2SourceLocation,
    pub message: String,
}

impl Db2SyntaxDiagnostic {
    pub(crate) fn new(
        code: Db2SyntaxDiagnosticCode,
        location: Db2SourceLocation,
        message: impl AsRef<str>,
    ) -> Self {
        Self {
            code,
            location,
            message: bounded_text(message.as_ref(), MAX_DIAGNOSTIC_BYTES),
        }
    }
}

impl fmt::Display for Db2SyntaxDiagnostic {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.location.line, self.location.column, self.message
        )
    }
}

impl std::error::Error for Db2SyntaxDiagnostic {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringKind {
    Character,
    National,
    Hex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2Symbol {
    Comma,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Plus,
    Minus,
    Multiply,
    Divide,
    Remainder,
    Concatenate,
    LeftParenthesis,
    RightParenthesis,
    Period,
    Colon,
    Semicolon,
}

/// Owned token kinds used by subsequent Db2 parser slices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2TokenKind {
    Word { value: String, delimited: bool },
    Number(String),
    String { kind: Db2StringKind, value: String },
    HostVariable(String),
    ParameterMarker,
    Symbol(Db2Symbol),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Token {
    pub kind: Db2TokenKind,
    pub span: Db2SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2LexedStatement {
    tokens: Vec<Db2Token>,
}

impl Db2LexedStatement {
    #[must_use]
    pub fn tokens(&self) -> &[Db2Token] {
        &self.tokens
    }
}

/// Lex one bounded SQL source into owned Db2 tokens.
pub fn lex_db2(
    source: &str,
    limits: Db2SyntaxLimits,
) -> Result<Db2LexedStatement, Db2SyntaxDiagnostic> {
    limits.validate()?;
    if source.len() > limits.max_statement_bytes {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::StatementTooLarge,
            Db2SourceLocation::START,
            "Db2 statement exceeds the configured byte limit",
        ));
    }
    if source.as_bytes().contains(&0) {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidCharacter,
            Db2SourceLocation::START,
            "Db2 statement contains a NUL byte",
        ));
    }

    let dialect = GenericDialect {};
    let external = Tokenizer::new(&dialect, source)
        .with_unescape(false)
        .tokenize_with_location()
        .map_err(|problem| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::TokenizerFailure,
                owned_location(problem.location),
                problem.message,
            )
        })?;
    if external.len() > limits.max_tokens.saturating_mul(3) {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::TooManyTokens,
            Db2SourceLocation::START,
            "Db2 token stream exceeds the configured limit",
        ));
    }

    let mut tokens: Vec<Db2Token> = Vec::with_capacity(external.len().min(limits.max_tokens));
    let mut nesting = 0_usize;
    let mut index = 0_usize;
    while index < external.len() {
        let item = &external[index];
        if matches!(item.token, Token::Whitespace(_)) {
            index += 1;
            continue;
        }
        if tokens.len() >= limits.max_tokens {
            return Err(diagnostic_at(
                Db2SyntaxDiagnosticCode::TooManyTokens,
                item,
                "Db2 token stream exceeds the configured limit",
            ));
        }
        if matches!(item.token, Token::Colon) {
            let location = owned_location(item.span.start);
            if tokens
                .last()
                .is_some_and(|token| token.span.end == location)
            {
                tokens.push(Db2Token {
                    kind: Db2TokenKind::Symbol(Db2Symbol::Colon),
                    span: owned_span(item.span),
                });
                index += 1;
            } else {
                let (token, consumed) = host_variable(&external[index..], limits)?;
                tokens.push(token);
                index += consumed;
            }
            continue;
        }

        let kind = owned_kind(item, limits)?;
        match kind {
            Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => {
                nesting = nesting.checked_add(1).ok_or_else(|| {
                    diagnostic_at(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        item,
                        "Db2 parenthesis depth overflowed",
                    )
                })?;
                if nesting > limits.max_nesting {
                    return Err(diagnostic_at(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        item,
                        "Db2 parenthesis nesting exceeds the configured limit",
                    ));
                }
            }
            Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => {
                nesting = nesting.checked_sub(1).ok_or_else(|| {
                    diagnostic_at(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        item,
                        "Db2 statement has an unmatched closing parenthesis",
                    )
                })?;
            }
            _ => {}
        }
        tokens.push(Db2Token {
            kind,
            span: owned_span(item.span),
        });
        index += 1;
    }
    if tokens.is_empty() {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::EmptyStatement,
            Db2SourceLocation::START,
            "Db2 statement contains no tokens",
        ));
    }
    if nesting != 0 {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
            tokens
                .last()
                .map_or(Db2SourceLocation::START, |token| token.span.end),
            "Db2 statement has an unmatched opening parenthesis",
        ));
    }
    Ok(Db2LexedStatement { tokens })
}

fn host_variable(
    input: &[TokenWithSpan],
    limits: Db2SyntaxLimits,
) -> Result<(Db2Token, usize), Db2SyntaxDiagnostic> {
    let colon = &input[0];
    let mut cursor = 1_usize;
    let Some(first) = input.get(cursor) else {
        return Err(diagnostic_at(
            Db2SyntaxDiagnosticCode::InvalidHostVariable,
            colon,
            "Db2 host-variable marker has no name",
        ));
    };
    let Token::Word(word) = &first.token else {
        return Err(diagnostic_at(
            Db2SyntaxDiagnosticCode::InvalidHostVariable,
            first,
            "Db2 host-variable name must begin with an identifier",
        ));
    };
    if word.quote_style.is_some() {
        return Err(diagnostic_at(
            Db2SyntaxDiagnosticCode::InvalidHostVariable,
            first,
            "Db2 host-variable name cannot be delimited",
        ));
    }
    let mut name = word.value.clone();
    let mut end = first.span;
    cursor += 1;
    loop {
        let Some(minus) = input.get(cursor) else {
            break;
        };
        let Some(word_token) = input.get(cursor + 1) else {
            break;
        };
        let Token::Word(word) = &word_token.token else {
            break;
        };
        if !matches!(minus.token, Token::Minus) || word.quote_style.is_some() {
            break;
        }
        name.push('-');
        name.push_str(&word.value);
        end = word_token.span;
        cursor += 2;
    }
    ensure_token_bound(&name, first, limits)?;
    Ok((
        Db2Token {
            kind: Db2TokenKind::HostVariable(name),
            span: Db2SourceSpan {
                start: owned_location(colon.span.start),
                end: owned_location(end.end),
            },
        },
        cursor,
    ))
}

fn owned_kind(
    item: &TokenWithSpan,
    limits: Db2SyntaxLimits,
) -> Result<Db2TokenKind, Db2SyntaxDiagnostic> {
    let kind = match &item.token {
        Token::Word(word) => {
            ensure_token_bound(&word.value, item, limits)?;
            match word.quote_style {
                None => Db2TokenKind::Word {
                    value: word.value.to_ascii_uppercase(),
                    delimited: false,
                },
                Some('"') => Db2TokenKind::Word {
                    value: word.value.clone(),
                    delimited: true,
                },
                Some(_) => {
                    return Err(diagnostic_at(
                        Db2SyntaxDiagnosticCode::UnsupportedToken,
                        item,
                        "identifier delimiter is not part of the owned Db2 core",
                    ));
                }
            }
        }
        Token::Number(value, _) => {
            ensure_token_bound(value, item, limits)?;
            Db2TokenKind::Number(value.clone())
        }
        Token::SingleQuotedString(value) => {
            string_token(Db2StringKind::Character, value, item, limits)?
        }
        Token::NationalStringLiteral(value) => {
            string_token(Db2StringKind::National, value, item, limits)?
        }
        Token::HexStringLiteral(value) => string_token(Db2StringKind::Hex, value, item, limits)?,
        Token::Placeholder(value) if value == "?" => Db2TokenKind::ParameterMarker,
        Token::Comma => Db2TokenKind::Symbol(Db2Symbol::Comma),
        Token::Eq => Db2TokenKind::Symbol(Db2Symbol::Equal),
        Token::Neq => Db2TokenKind::Symbol(Db2Symbol::NotEqual),
        Token::Lt => Db2TokenKind::Symbol(Db2Symbol::Less),
        Token::LtEq => Db2TokenKind::Symbol(Db2Symbol::LessOrEqual),
        Token::Gt => Db2TokenKind::Symbol(Db2Symbol::Greater),
        Token::GtEq => Db2TokenKind::Symbol(Db2Symbol::GreaterOrEqual),
        Token::Plus => Db2TokenKind::Symbol(Db2Symbol::Plus),
        Token::Minus => Db2TokenKind::Symbol(Db2Symbol::Minus),
        Token::Mul => Db2TokenKind::Symbol(Db2Symbol::Multiply),
        Token::Div => Db2TokenKind::Symbol(Db2Symbol::Divide),
        Token::Mod => Db2TokenKind::Symbol(Db2Symbol::Remainder),
        Token::StringConcat => Db2TokenKind::Symbol(Db2Symbol::Concatenate),
        Token::LParen => Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis),
        Token::RParen => Db2TokenKind::Symbol(Db2Symbol::RightParenthesis),
        Token::Period => Db2TokenKind::Symbol(Db2Symbol::Period),
        Token::SemiColon => Db2TokenKind::Symbol(Db2Symbol::Semicolon),
        Token::Char(character) => {
            return Err(diagnostic_at(
                Db2SyntaxDiagnosticCode::InvalidCharacter,
                item,
                format!("invalid Db2 character {character:?}"),
            ));
        }
        Token::Whitespace(Whitespace::Space | Whitespace::Newline | Whitespace::Tab)
        | Token::Whitespace(Whitespace::SingleLineComment { .. })
        | Token::Whitespace(Whitespace::MultiLineComment(_))
        | Token::EOF => unreachable!("whitespace and EOF do not reach owned token conversion"),
        _ => {
            return Err(diagnostic_at(
                Db2SyntaxDiagnosticCode::UnsupportedToken,
                item,
                "token is not part of the frozen owned Db2 lexical core",
            ));
        }
    };
    Ok(kind)
}

fn string_token(
    kind: Db2StringKind,
    value: &str,
    item: &TokenWithSpan,
    limits: Db2SyntaxLimits,
) -> Result<Db2TokenKind, Db2SyntaxDiagnostic> {
    ensure_token_bound(value, item, limits)?;
    Ok(Db2TokenKind::String {
        kind,
        value: value.to_string(),
    })
}

fn ensure_token_bound(
    value: &str,
    item: &TokenWithSpan,
    limits: Db2SyntaxLimits,
) -> Result<(), Db2SyntaxDiagnostic> {
    if value.len() > limits.max_token_bytes {
        Err(diagnostic_at(
            Db2SyntaxDiagnosticCode::TokenTooLarge,
            item,
            "Db2 token exceeds the configured byte limit",
        ))
    } else {
        Ok(())
    }
}

fn diagnostic_at(
    code: Db2SyntaxDiagnosticCode,
    item: &TokenWithSpan,
    message: impl AsRef<str>,
) -> Db2SyntaxDiagnostic {
    Db2SyntaxDiagnostic::new(code, owned_location(item.span.start), message)
}

fn owned_span(span: Span) -> Db2SourceSpan {
    Db2SourceSpan {
        start: owned_location(span.start),
        end: owned_location(span.end),
    }
}

fn owned_location(location: Location) -> Db2SourceLocation {
    Db2SourceLocation {
        line: u32::try_from(location.line).unwrap_or(u32::MAX),
        column: u32::try_from(location.column).unwrap_or(u32::MAX),
    }
}

fn bounded_text(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> Result<Db2LexedStatement, Db2SyntaxDiagnostic> {
        lex_db2(source, Db2SyntaxLimits::default())
    }

    #[test]
    fn words_literals_symbols_and_spans_are_owned() {
        let statement = lex("SELECT \"MiXeD\", AMOUNT + 1, 'A''B' FROM T;").unwrap();
        assert_eq!(statement.tokens().len(), 11);
        assert_eq!(
            statement.tokens()[0].kind,
            Db2TokenKind::Word {
                value: "SELECT".into(),
                delimited: false
            }
        );
        assert_eq!(
            statement.tokens()[1].kind,
            Db2TokenKind::Word {
                value: "MiXeD".into(),
                delimited: true
            }
        );
        assert_eq!(
            statement.tokens()[7].kind,
            Db2TokenKind::String {
                kind: Db2StringKind::Character,
                value: "A''B".into()
            }
        );
        assert_eq!(statement.tokens()[0].span.start, Db2SourceLocation::START);
        assert!(
            statement
                .tokens()
                .windows(2)
                .all(|pair| pair[0].span.start <= pair[1].span.start)
        );
    }

    #[test]
    fn cobol_host_variables_are_one_owned_token() {
        let statement = lex("FETCH C1 INTO :HV-ID, :HV_NAME :HV-IND").unwrap();
        let variables = statement
            .tokens()
            .iter()
            .filter_map(|token| match &token.kind {
                Db2TokenKind::HostVariable(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(variables, ["HV-ID", "HV_NAME", "HV-IND"]);
        let preserved = lex("VALUES :Mixed-Field INTO :Output-Field").unwrap();
        let variables = preserved
            .tokens()
            .iter()
            .filter_map(|token| match &token.kind {
                Db2TokenKind::HostVariable(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(variables, ["Mixed-Field", "Output-Field"]);
    }

    #[test]
    fn comments_are_not_owned_tokens() {
        assert_eq!(
            lex("-- comment\nSELECT /* middle */ 1")
                .unwrap()
                .tokens()
                .len(),
            2
        );
    }

    #[test]
    fn sql_pl_label_colon_is_not_a_host_variable() {
        let statement = lex("LBL: BEGIN SET V = 1; END").unwrap();
        assert_eq!(
            statement.tokens()[0].kind,
            Db2TokenKind::Word {
                value: "LBL".into(),
                delimited: false
            }
        );
        assert_eq!(
            statement.tokens()[1].kind,
            Db2TokenKind::Symbol(Db2Symbol::Colon)
        );
        assert!(
            statement
                .tokens()
                .iter()
                .all(|token| !matches!(token.kind, Db2TokenKind::HostVariable(_)))
        );
    }

    #[test]
    fn malformed_and_unsupported_inputs_fail_closed() {
        let unterminated = lex("SELECT 'ABC").unwrap_err();
        assert_eq!(unterminated.code, Db2SyntaxDiagnosticCode::TokenizerFailure);
        assert!(unterminated.message.len() <= MAX_DIAGNOSTIC_BYTES);
        assert_eq!(
            lex("SELECT `OTHER` FROM T").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedToken
        );
        assert_eq!(
            lex("SELECT @ FROM T").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedToken
        );
        assert_eq!(
            lex("SELECT (1").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );
        assert_eq!(
            lex("SELECT 1)").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );
        assert_eq!(
            lex("SELECT : FROM T").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidHostVariable
        );
    }

    #[test]
    fn every_resource_limit_is_enforced() {
        let invalid = Db2SyntaxLimits {
            max_statement_bytes: 0,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", invalid).unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        let short = Db2SyntaxLimits {
            max_statement_bytes: 4,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", short).unwrap_err().code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );
        let few = Db2SyntaxLimits {
            max_tokens: 1,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", few).unwrap_err().code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );
        let narrow = Db2SyntaxLimits {
            max_token_bytes: 3,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            lex_db2("SELECT", narrow).unwrap_err().code,
            Db2SyntaxDiagnosticCode::TokenTooLarge
        );
        let shallow = Db2SyntaxLimits {
            max_nesting: 1,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            lex_db2("SELECT ((1))", shallow).unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );
    }
}
