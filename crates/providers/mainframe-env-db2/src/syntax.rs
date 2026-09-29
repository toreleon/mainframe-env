//! Owned, bounded Db2 SQL tokens for later parser slices.

use std::fmt;

const MAX_DIAGNOSTIC_BYTES: usize = 256;
const MAX_STATEMENT_BYTES_CEILING: usize = 8 * 1024 * 1024;
const MAX_TOKENS_CEILING: usize = 262_144;
const MAX_TOKEN_BYTES_CEILING: usize = 1024 * 1024;
const MAX_NESTING_CEILING: usize = 1024;

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Db2SourceLocation {
    pub line: u32,
    pub column: u32,
}

impl Db2SourceLocation {
    pub const START: Self = Self { line: 1, column: 1 };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start: Db2SourceLocation,
    pub end: Db2SourceLocation,
}

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
    UnterminatedString,
    UnterminatedComment,
    InvalidHex,
    UnsupportedNumericConstant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SyntaxDiagnostic {
    pub code: Db2SyntaxDiagnosticCode,
    pub location: Db2SourceLocation,
    pub message: String,
}

impl Db2SyntaxDiagnostic {
    fn new(code: Db2SyntaxDiagnosticCode, location: Db2SourceLocation, message: &str) -> Self {
        let mut end = message.len().min(MAX_DIAGNOSTIC_BYTES);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            code,
            location,
            message: message[..end].to_owned(),
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
    Graphic,
    Hex,
    UnicodeHex,
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

    #[must_use]
    pub fn cursor(&self) -> Db2TokenCursor<'_> {
        Db2TokenCursor {
            tokens: &self.tokens,
            position: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Db2TokenCursor<'a> {
    tokens: &'a [Db2Token],
    position: usize,
}

impl<'a> Db2TokenCursor<'a> {
    #[must_use]
    pub fn peek(&self) -> Option<&'a Db2Token> {
        self.tokens.get(self.position)
    }
    #[must_use]
    pub fn position(&self) -> usize {
        self.position
    }
}

impl<'a> Iterator for Db2TokenCursor<'a> {
    type Item = &'a Db2Token;
    fn next(&mut self) -> Option<Self::Item> {
        let token = self.peek()?;
        self.position += 1;
        Some(token)
    }
}

/// Lex a statement without accessing a catalog or executing any SQL.
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
    if source.contains('\0') {
        return Err(Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidCharacter,
            Db2SourceLocation::START,
            "Db2 statement contains a NUL byte",
        ));
    }
    let mut lexer = Lexer {
        source,
        limits,
        offset: 0,
        location: Db2SourceLocation::START,
        tokens: Vec::new(),
        nesting: 0,
    };
    lexer.scan()?;
    Ok(Db2LexedStatement {
        tokens: lexer.tokens,
    })
}

struct Lexer<'a> {
    source: &'a str,
    limits: Db2SyntaxLimits,
    offset: usize,
    location: Db2SourceLocation,
    tokens: Vec<Db2Token>,
    nesting: usize,
}

impl Lexer<'_> {
    fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn at(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        Db2SyntaxDiagnostic::new(code, self.location, message)
    }
    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        let crlf_tail =
            ch == '\n' && self.offset >= 2 && self.source.as_bytes()[self.offset - 2] == b'\r';
        if ch == '\r' || (ch == '\n' && !crlf_tail) {
            self.location.line = self.location.line.saturating_add(1);
            self.location.column = 1;
        } else if !crlf_tail {
            self.location.column = self.location.column.saturating_add(1);
        }
        Some(ch)
    }
    fn take(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            for _ in prefix.chars() {
                self.advance();
            }
            true
        } else {
            false
        }
    }
    fn push(
        &mut self,
        kind: Db2TokenKind,
        start: (usize, Db2SourceLocation),
    ) -> Result<(), Db2SyntaxDiagnostic> {
        if self.offset - start.0 > self.limits.max_token_bytes {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::TokenTooLarge,
                start.1,
                "Db2 token exceeds the configured byte limit",
            ));
        }
        if self.tokens.len() >= self.limits.max_tokens {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::TooManyTokens,
                start.1,
                "Db2 token stream exceeds the configured limit",
            ));
        }
        self.tokens.push(Db2Token {
            kind,
            span: Db2SourceSpan {
                start_byte: start.0,
                end_byte: self.offset,
                start: start.1,
                end: self.location,
            },
        });
        Ok(())
    }
    fn scan(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.advance();
                continue;
            }
            if self.take("--") {
                while !matches!(self.peek(), None | Some('\r' | '\n')) {
                    self.advance();
                }
                continue;
            }
            if self.take("/*") {
                self.comment()?;
                continue;
            }
            let start = (self.offset, self.location);
            if ch == '\0' || !ch.is_ascii() {
                return Err(self.at(
                    Db2SyntaxDiagnosticCode::InvalidCharacter,
                    "invalid Db2 character",
                ));
            }
            if ch == '"' {
                let value = self.quoted('"', Db2SyntaxDiagnosticCode::UnterminatedString)?;
                if value.is_empty() {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::UnsupportedToken,
                        start.1,
                        "empty delimited Db2 identifier",
                    ));
                }
                self.push(
                    Db2TokenKind::Word {
                        value,
                        delimited: true,
                    },
                    start,
                )?;
                continue;
            }
            if ch == '\'' {
                let value = self.quoted('\'', Db2SyntaxDiagnosticCode::UnterminatedString)?;
                self.push(
                    Db2TokenKind::String {
                        kind: Db2StringKind::Character,
                        value,
                    },
                    start,
                )?;
                continue;
            }
            if ch.is_ascii_alphabetic() {
                self.word_or_prefixed_string(start)?;
                continue;
            }
            if ch.is_ascii_digit()
                || (ch == '.' && self.rest()[1..].starts_with(|c: char| c.is_ascii_digit()))
            {
                self.number(start)?;
                continue;
            }
            if ch == ':' {
                self.advance();
                if self.tokens.last().is_some_and(|token| {
                    token.span.end_byte == start.0
                        && matches!(token.kind, Db2TokenKind::Word { .. })
                }) {
                    self.push(Db2TokenKind::Symbol(Db2Symbol::Colon), start)?;
                } else {
                    self.host_variable(start)?;
                }
                continue;
            }
            if ch == '?' {
                self.advance();
                self.push(Db2TokenKind::ParameterMarker, start)?;
                continue;
            }
            let symbol = if self.take("||") {
                Db2Symbol::Concatenate
            } else if self.take("<=") {
                Db2Symbol::LessOrEqual
            } else if self.take(">=") {
                Db2Symbol::GreaterOrEqual
            } else if self.take("<>") {
                Db2Symbol::NotEqual
            } else {
                self.advance();
                match ch {
                    ',' => Db2Symbol::Comma,
                    '=' => Db2Symbol::Equal,
                    '<' => Db2Symbol::Less,
                    '>' => Db2Symbol::Greater,
                    '+' => Db2Symbol::Plus,
                    '-' => Db2Symbol::Minus,
                    '*' => Db2Symbol::Multiply,
                    '/' => Db2Symbol::Divide,
                    '%' => Db2Symbol::Remainder,
                    '(' => Db2Symbol::LeftParenthesis,
                    ')' => Db2Symbol::RightParenthesis,
                    '.' => Db2Symbol::Period,
                    ';' => Db2Symbol::Semicolon,
                    _ => {
                        return Err(Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::UnsupportedToken,
                            start.1,
                            "token is not part of the owned Db2 lexical core",
                        ));
                    }
                }
            };
            if symbol == Db2Symbol::LeftParenthesis {
                self.nesting += 1;
                if self.nesting > self.limits.max_nesting {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        start.1,
                        "Db2 parenthesis nesting exceeds the configured limit",
                    ));
                }
            } else if symbol == Db2Symbol::RightParenthesis {
                self.nesting = self.nesting.checked_sub(1).ok_or_else(|| {
                    Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        start.1,
                        "Db2 statement has an unmatched closing parenthesis",
                    )
                })?;
            }
            self.push(Db2TokenKind::Symbol(symbol), start)?;
        }
        if self.tokens.is_empty() {
            return Err(self.at(
                Db2SyntaxDiagnosticCode::EmptyStatement,
                "Db2 statement contains no tokens",
            ));
        }
        if self.nesting != 0 {
            return Err(self.at(
                Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                "Db2 statement has an unmatched opening parenthesis",
            ));
        }
        Ok(())
    }
    fn comment(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        let mut depth = 1_usize;
        while self.peek().is_some() {
            if self.take("/*") {
                depth += 1;
                if depth > self.limits.max_nesting {
                    return Err(self.at(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
                        "Db2 comment nesting exceeds the configured limit",
                    ));
                }
            } else if self.take("*/") {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            } else {
                self.advance();
            }
        }
        Err(self.at(
            Db2SyntaxDiagnosticCode::UnterminatedComment,
            "unterminated Db2 bracketed comment",
        ))
    }
    fn quoted(
        &mut self,
        quote: char,
        failure: Db2SyntaxDiagnosticCode,
    ) -> Result<String, Db2SyntaxDiagnostic> {
        self.advance();
        let begin = self.offset;
        loop {
            match self.peek() {
                None => return Err(self.at(failure, "unterminated Db2 quoted token")),
                Some(ch) if ch == quote => {
                    let end = self.offset;
                    self.advance();
                    if self.peek() == Some(quote) {
                        self.advance();
                    } else {
                        return Ok(self.source[begin..end].to_owned());
                    }
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }
    fn word_or_prefixed_string(
        &mut self,
        start: (usize, Db2SourceLocation),
    ) -> Result<(), Db2SyntaxDiagnostic> {
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            self.advance();
        }
        let word = &self.source[start.0..self.offset];
        if self.peek() == Some('\'') {
            let upper = word.to_ascii_uppercase();
            if upper == "DECFLOAT" {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::UnsupportedNumericConstant,
                    start.1,
                    "Db2 float and decfloat constants are source-pending on #350",
                ));
            }
            let kind = match upper.as_str() {
                "X" => Db2StringKind::Hex,
                "UX" => Db2StringKind::UnicodeHex,
                "G" => Db2StringKind::Graphic,
                "N" => Db2StringKind::National,
                _ => {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::UnsupportedToken,
                        start.1,
                        "unsupported Db2 string prefix",
                    ));
                }
            };
            let value = self.quoted('\'', Db2SyntaxDiagnosticCode::UnterminatedString)?;
            if matches!(kind, Db2StringKind::Hex | Db2StringKind::UnicodeHex) {
                let modulus = if kind == Db2StringKind::UnicodeHex {
                    4
                } else {
                    2
                };
                if value.len() % modulus != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(Db2SyntaxDiagnostic::new(
                        Db2SyntaxDiagnosticCode::InvalidHex,
                        start.1,
                        "invalid Db2 hexadecimal string constant",
                    ));
                }
            }
            self.push(Db2TokenKind::String { kind, value }, start)
        } else {
            self.push(
                Db2TokenKind::Word {
                    value: word.to_ascii_uppercase(),
                    delimited: false,
                },
                start,
            )
        }
    }
    fn number(&mut self, start: (usize, Db2SourceLocation)) -> Result<(), Db2SyntaxDiagnostic> {
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.advance();
        }
        if self.peek() == Some('.') && self.rest()[1..].starts_with(|ch: char| ch.is_ascii_digit())
        {
            self.advance();
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.advance();
            }
        }
        if matches!(self.peek(), Some('E' | 'e')) {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::UnsupportedNumericConstant,
                start.1,
                "Db2 float and decfloat constants are source-pending on #350",
            ));
        }
        if self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::UnsupportedNumericConstant,
                start.1,
                "Db2 numeric constant form is source-pending on #350",
            ));
        }
        self.push(
            Db2TokenKind::Number(self.source[start.0..self.offset].to_owned()),
            start,
        )
    }
    fn host_variable(
        &mut self,
        start: (usize, Db2SourceLocation),
    ) -> Result<(), Db2SyntaxDiagnostic> {
        if !self.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            return Err(Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidHostVariable,
                start.1,
                "Db2 host-variable marker must be followed by a name",
            ));
        }
        let begin = self.offset;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            self.advance();
        }
        while self.peek() == Some('-')
            && self.rest()[1..].starts_with(|ch: char| ch.is_ascii_alphabetic())
        {
            self.advance();
            while self
                .peek()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                self.advance();
            }
        }
        self.push(
            Db2TokenKind::HostVariable(self.source[begin..self.offset].to_ascii_uppercase()),
            start,
        )
    }
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
        assert_eq!(statement.tokens()[0].span.start_byte, 0);
        assert_eq!(statement.tokens()[0].span.start, Db2SourceLocation::START);
        assert!(
            statement
                .tokens()
                .windows(2)
                .all(|pair| pair[0].span.end_byte <= pair[1].span.start_byte)
        );
        let mut cursor = statement.cursor();
        assert_eq!(cursor.peek(), statement.tokens().first());
        assert_eq!(cursor.next(), statement.tokens().first());
        assert_eq!(cursor.position(), 1);
    }

    #[test]
    fn host_variables_and_label_colon() {
        let statement = lex("FETCH C1 INTO :HV-ID, :HV_NAME :HV-IND").unwrap();
        let variables: Vec<_> = statement
            .tokens()
            .iter()
            .filter_map(|t| match &t.kind {
                Db2TokenKind::HostVariable(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(variables, ["HV-ID", "HV_NAME", "HV-IND"]);
        assert_eq!(
            lex("LBL: BEGIN").unwrap().tokens()[1].kind,
            Db2TokenKind::Symbol(Db2Symbol::Colon)
        );
        assert_eq!(
            lex(": BAD").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidHostVariable
        );
    }

    #[test]
    fn comments_strings_and_unicode_spans() {
        let statement =
            lex("-- line\nSELECT /* outer /* inner */ end */ G'AB', N'CD', UX'0041', X'0F'")
                .unwrap();
        assert_eq!(
            statement.tokens()[0].span.start,
            Db2SourceLocation { line: 2, column: 1 }
        );
        assert!(statement.tokens().iter().any(|t| matches!(
            t.kind,
            Db2TokenKind::String {
                kind: Db2StringKind::UnicodeHex,
                ..
            }
        )));
        assert_eq!(
            lex("SELECT /* open").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnterminatedComment
        );
        let unterminated = lex("SELECT 'open").unwrap_err();
        assert_eq!(
            unterminated.location,
            Db2SourceLocation {
                line: 1,
                column: 13
            }
        );
        assert_eq!(unterminated.message, "unterminated Db2 quoted token");
        assert_eq!(
            lex("SELECT 'open").unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnterminatedString
        );
        assert_eq!(
            lex("SELECT UX'00ZZ'").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidHex
        );
        assert_eq!(
            lex("SELECT X'F'").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidHex
        );
        assert_eq!(
            lex("SELECT é").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidCharacter
        );
    }

    #[test]
    fn numeric_source_gap_is_explicit() {
        assert_eq!(lex("SELECT 12, .5, 12.50").unwrap().tokens().len(), 6);
        for source in [
            "SELECT 1E3",
            "SELECT 1.2e-4",
            "SELECT 1E+2",
            "SELECT DECFLOAT'NaN'",
        ] {
            let diagnostic = lex(source).unwrap_err();
            assert_eq!(
                diagnostic.code,
                Db2SyntaxDiagnosticCode::UnsupportedNumericConstant
            );
            assert_eq!(
                diagnostic.location,
                Db2SourceLocation { line: 1, column: 8 }
            );
            assert_eq!(
                diagnostic.message,
                "Db2 float and decfloat constants are source-pending on #350"
            );
        }
    }

    #[test]
    fn malformed_inputs_and_bounds_fail_closed() {
        assert_eq!(
            lex(" ").unwrap_err().code,
            Db2SyntaxDiagnosticCode::EmptyStatement
        );
        assert_eq!(
            lex("SELECT `OTHER`").unwrap_err().code,
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
        let invalid = Db2SyntaxLimits {
            max_statement_bytes: 0,
            ..Default::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", invalid).unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        let short = Db2SyntaxLimits {
            max_statement_bytes: 4,
            ..Default::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", short).unwrap_err().code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );
        let few = Db2SyntaxLimits {
            max_tokens: 1,
            ..Default::default()
        };
        assert_eq!(
            lex_db2("SELECT 1", few).unwrap_err().code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );
        let narrow = Db2SyntaxLimits {
            max_token_bytes: 3,
            ..Default::default()
        };
        assert_eq!(
            lex_db2("SELECT", narrow).unwrap_err().code,
            Db2SyntaxDiagnosticCode::TokenTooLarge
        );
        let shallow = Db2SyntaxLimits {
            max_nesting: 1,
            ..Default::default()
        };
        assert_eq!(
            lex_db2("SELECT ((1))", shallow).unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );
        let exact = Db2SyntaxLimits {
            max_statement_bytes: 8,
            max_tokens: 2,
            max_token_bytes: 6,
            ..Default::default()
        };
        assert_eq!(lex_db2("SELECT 1", exact).unwrap().tokens().len(), 2);
        let leading = lex("\nSELECT 1").unwrap();
        assert_eq!(leading.tokens()[0].span.start_byte, 1);
        assert_eq!(
            leading.tokens()[0].span.start,
            Db2SourceLocation { line: 2, column: 1 }
        );
        let crlf = lex("-- line\r\nSELECT 1").unwrap();
        assert_eq!(
            crlf.tokens()[0].span.start,
            Db2SourceLocation { line: 2, column: 1 }
        );
        assert_eq!(
            lex("SELECT '\0'").unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidCharacter
        );
    }
}
