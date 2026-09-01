use std::collections::BTreeSet;
use std::fmt;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandDomain {
    Group,
    Dataset,
    User,
    Connection,
    Resource,
    Query,
    Policy,
    Identity,
    Operations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    family: CommandFamily,
    row_id: &'static str,
    keyword: &'static str,
    aliases: &'static [&'static str],
    domain: CommandDomain,
    work_package: &'static str,
    mutating: bool,
    command_direction: bool,
    min_positionals: u64,
    max_positionals: u64,
    operands: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuppliedClassDescriptor {
    pub name: &'static str,
    pub active: bool,
    pub generic_allowed: bool,
    pub generic_active: bool,
    pub discrete_allowed: bool,
    pub raclist: bool,
    pub max_profile_name_bytes: usize,
    pub posit: Option<u16>,
}

mod generated {
    use super::{CommandDescriptor, CommandDomain, SuppliedClassDescriptor};
    include!("generated/racf_command_catalog.rs");
}

use generated::COMMAND_DESCRIPTORS;
pub use generated::CommandFamily;
use generated::SUPPLIED_CLASS_DESCRIPTORS;

impl CommandDescriptor {
    #[must_use]
    pub const fn family(self) -> CommandFamily {
        self.family
    }

    #[must_use]
    pub const fn row_id(self) -> &'static str {
        self.row_id
    }

    #[must_use]
    pub const fn keyword(self) -> &'static str {
        self.keyword
    }

    #[must_use]
    pub const fn aliases(self) -> &'static [&'static str] {
        self.aliases
    }

    #[must_use]
    pub const fn domain(self) -> CommandDomain {
        self.domain
    }

    #[must_use]
    pub const fn work_package(self) -> &'static str {
        self.work_package
    }

    #[must_use]
    pub const fn mutating(self) -> bool {
        self.mutating
    }

    #[must_use]
    pub const fn command_direction(self) -> bool {
        self.command_direction
    }

    #[must_use]
    pub const fn positional_bounds(self) -> (u64, u64) {
        (self.min_positionals, self.max_positionals)
    }

    #[must_use]
    pub const fn operands(self) -> &'static [&'static str] {
        self.operands
    }
}

#[must_use]
pub const fn command_descriptors() -> &'static [CommandDescriptor] {
    COMMAND_DESCRIPTORS
}

#[must_use]
pub const fn supplied_class_descriptors() -> &'static [SuppliedClassDescriptor] {
    SUPPLIED_CLASS_DESCRIPTORS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLanguageLimits {
    pub max_input_bytes: usize,
    pub max_tokens: usize,
    pub max_token_bytes: usize,
    pub max_positionals: usize,
    pub max_operands: usize,
    pub max_values_per_operand: usize,
    pub max_parenthesis_depth: usize,
}

impl Default for CommandLanguageLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 32 * 1024,
            max_tokens: 2048,
            max_token_bytes: 4096,
            max_positionals: 256,
            max_operands: 256,
            max_values_per_operand: 512,
            max_parenthesis_depth: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandDiagnosticCode {
    Empty,
    InputLimit,
    TokenLimit,
    TokenLength,
    UnterminatedQuote,
    InvalidCharacter,
    UnknownFamily,
    UnexpectedToken,
    UnterminatedOperand,
    ParenthesisDepth,
    PositionalCount,
    UnknownOperand,
    DuplicateOperand,
    OperandValueLimit,
    UnsupportedFamily,
    InvalidValue,
    Unauthorized,
    NotFound,
    Conflict,
    ResourceExhausted,
    ProviderFailure,
}

impl CommandDiagnosticCode {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Empty => "MERSEC1001E",
            Self::InputLimit => "MERSEC1002E",
            Self::TokenLimit => "MERSEC1003E",
            Self::TokenLength => "MERSEC1004E",
            Self::UnterminatedQuote => "MERSEC1005E",
            Self::InvalidCharacter => "MERSEC1006E",
            Self::UnknownFamily => "MERSEC1007E",
            Self::UnexpectedToken => "MERSEC1008E",
            Self::UnterminatedOperand => "MERSEC1009E",
            Self::ParenthesisDepth => "MERSEC1010E",
            Self::PositionalCount => "MERSEC1011E",
            Self::UnknownOperand => "MERSEC1012E",
            Self::DuplicateOperand => "MERSEC1013E",
            Self::OperandValueLimit => "MERSEC1014E",
            Self::UnsupportedFamily => "MERSEC2001E",
            Self::InvalidValue => "MERSEC2002E",
            Self::Unauthorized => "MERSEC2003E",
            Self::NotFound => "MERSEC2004E",
            Self::Conflict => "MERSEC2005E",
            Self::ResourceExhausted => "MERSEC2006E",
            Self::ProviderFailure => "MERSEC2007E",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDiagnostic {
    pub code: CommandDiagnosticCode,
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for CommandDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at byte {}: {}",
            self.code.id(),
            self.offset,
            self.message
        )
    }
}

impl std::error::Error for CommandDiagnostic {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCommand {
    pub family: CommandFamily,
    pub row_id: &'static str,
    pub keyword: &'static str,
    pub mutating: bool,
    pub positional_count: usize,
    pub operands: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Word,
    Quoted,
    LeftParenthesis,
    RightParenthesis,
    Comma,
}

struct Token {
    kind: TokenKind,
    text: Zeroizing<String>,
    offset: usize,
}

impl Token {
    fn upper(&self) -> String {
        self.text.to_ascii_uppercase()
    }
}

pub(crate) struct ParsedOperand {
    pub(crate) name: String,
    pub(crate) values: Vec<Zeroizing<String>>,
    pub(crate) offset: usize,
}

pub(crate) struct ParsedCommand {
    pub(crate) descriptor: CommandDescriptor,
    pub(crate) positionals: Vec<Zeroizing<String>>,
    pub(crate) operands: Vec<ParsedOperand>,
}

impl ParsedCommand {
    pub(crate) fn positional(&self, index: usize) -> Option<&str> {
        self.positionals.get(index).map(|value| value.as_str())
    }

    pub(crate) fn operand(&self, name: &str) -> Option<&ParsedOperand> {
        self.operands.iter().find(|operand| operand.name == name)
    }

    pub(crate) fn has_operand(&self, name: &str) -> bool {
        self.operand(name).is_some()
    }
}

impl ParsedOperand {
    pub(crate) fn first(&self) -> Option<&str> {
        self.values.first().map(|value| value.as_str())
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &str> {
        self.values.iter().map(|value| value.as_str())
    }
}

pub fn recognize_command(
    input: &str,
    limits: CommandLanguageLimits,
) -> Result<CommandFamily, CommandDiagnostic> {
    let tokens = lex(input, limits)?;
    let first = tokens
        .first()
        .ok_or_else(|| diagnostic(CommandDiagnosticCode::Empty, 0))?;
    if first.kind != TokenKind::Word {
        return Err(diagnostic(
            CommandDiagnosticCode::UnknownFamily,
            first.offset,
        ));
    }
    descriptor(&first.upper())
        .map(|descriptor| descriptor.family)
        .ok_or_else(|| diagnostic(CommandDiagnosticCode::UnknownFamily, first.offset))
}

pub fn validate_command(
    input: &str,
    limits: CommandLanguageLimits,
) -> Result<ValidatedCommand, CommandDiagnostic> {
    let parsed = parse_command(input, limits)?;
    Ok(ValidatedCommand {
        family: parsed.descriptor.family,
        row_id: parsed.descriptor.row_id,
        keyword: parsed.descriptor.keyword,
        mutating: parsed.descriptor.mutating,
        positional_count: parsed.positionals.len(),
        operands: parsed
            .operands
            .iter()
            .map(|operand| operand.name.clone())
            .collect(),
    })
}

pub(crate) fn parse_command(
    input: &str,
    limits: CommandLanguageLimits,
) -> Result<ParsedCommand, CommandDiagnostic> {
    let tokens = lex(input, limits)?;
    let first = tokens
        .first()
        .ok_or_else(|| diagnostic(CommandDiagnosticCode::Empty, 0))?;
    if first.kind != TokenKind::Word {
        return Err(diagnostic(
            CommandDiagnosticCode::UnknownFamily,
            first.offset,
        ));
    }
    let descriptor = descriptor(&first.upper())
        .ok_or_else(|| diagnostic(CommandDiagnosticCode::UnknownFamily, first.offset))?;
    let mut positionals = Vec::new();
    let mut operands = Vec::new();
    let mut operand_names = BTreeSet::new();
    let mut index = 1;
    while index < tokens.len() {
        if tokens[index].kind == TokenKind::Comma {
            index += 1;
            continue;
        }
        let token = &tokens[index];
        if !matches!(token.kind, TokenKind::Word | TokenKind::Quoted) {
            return Err(diagnostic(
                CommandDiagnosticCode::UnexpectedToken,
                token.offset,
            ));
        }
        let required_positionals = usize::try_from(descriptor.min_positionals)
            .map_err(|_| diagnostic(CommandDiagnosticCode::PositionalCount, token.offset))?;
        let candidate = (token.kind == TokenKind::Word).then(|| token.upper());
        let allowed_operand = candidate.as_deref().is_some_and(|name| {
            descriptor.operands.contains(&name)
                || descriptor.command_direction && matches!(name, "AT" | "ONLYAT")
        });
        if positionals.len() >= required_positionals && allowed_operand {
            if operands.len() >= limits.max_operands {
                return Err(diagnostic(
                    CommandDiagnosticCode::OperandValueLimit,
                    token.offset,
                ));
            }
            let name = candidate.expect("allowed operand has a word name");
            if !operand_names.insert(name.clone()) {
                return Err(diagnostic(
                    CommandDiagnosticCode::DuplicateOperand,
                    token.offset,
                ));
            }
            let offset = token.offset;
            index += 1;
            let mut values = Vec::new();
            if tokens
                .get(index)
                .is_some_and(|token| token.kind == TokenKind::LeftParenthesis)
            {
                index += 1;
                let mut depth = 1usize;
                while index < tokens.len() && depth > 0 {
                    let value = &tokens[index];
                    match value.kind {
                        TokenKind::LeftParenthesis => {
                            depth += 1;
                            if depth > limits.max_parenthesis_depth {
                                return Err(diagnostic(
                                    CommandDiagnosticCode::ParenthesisDepth,
                                    value.offset,
                                ));
                            }
                        }
                        TokenKind::RightParenthesis => depth -= 1,
                        TokenKind::Word | TokenKind::Quoted if depth > 0 => {
                            if values.len() >= limits.max_values_per_operand {
                                return Err(diagnostic(
                                    CommandDiagnosticCode::OperandValueLimit,
                                    value.offset,
                                ));
                            }
                            values.push(Zeroizing::new(value.text.to_string()));
                        }
                        TokenKind::Comma => {}
                        _ => {}
                    }
                    index += 1;
                }
                if depth != 0 {
                    return Err(diagnostic(
                        CommandDiagnosticCode::UnterminatedOperand,
                        offset,
                    ));
                }
            }
            operands.push(ParsedOperand {
                name,
                values,
                offset,
            });
            continue;
        }
        if candidate.is_some()
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.kind == TokenKind::LeftParenthesis)
        {
            return Err(diagnostic(
                CommandDiagnosticCode::UnknownOperand,
                token.offset,
            ));
        }
        let max_positionals = usize::try_from(descriptor.max_positionals)
            .map_err(|_| diagnostic(CommandDiagnosticCode::PositionalCount, token.offset))?
            .min(limits.max_positionals);
        if positionals.len() >= max_positionals {
            return Err(diagnostic(
                CommandDiagnosticCode::PositionalCount,
                token.offset,
            ));
        }
        positionals.push(Zeroizing::new(token.text.to_string()));
        index += 1;
    }
    let min = usize::try_from(descriptor.min_positionals)
        .map_err(|_| diagnostic(CommandDiagnosticCode::PositionalCount, 0))?;
    let max = usize::try_from(descriptor.max_positionals)
        .map_err(|_| diagnostic(CommandDiagnosticCode::PositionalCount, 0))?
        .min(limits.max_positionals);
    if positionals.len() < min || positionals.len() > max {
        return Err(diagnostic(
            CommandDiagnosticCode::PositionalCount,
            input.len(),
        ));
    }
    Ok(ParsedCommand {
        descriptor,
        positionals,
        operands,
    })
}

fn descriptor(selector: &str) -> Option<CommandDescriptor> {
    COMMAND_DESCRIPTORS
        .iter()
        .copied()
        .find(|descriptor| descriptor.keyword == selector || descriptor.aliases.contains(&selector))
}

fn lex(input: &str, limits: CommandLanguageLimits) -> Result<Vec<Token>, CommandDiagnostic> {
    if input.is_empty() || input.trim().is_empty() {
        return Err(diagnostic(CommandDiagnosticCode::Empty, 0));
    }
    if input.len() > limits.max_input_bytes {
        return Err(diagnostic(
            CommandDiagnosticCode::InputLimit,
            limits.max_input_bytes,
        ));
    }
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if tokens.len() >= limits.max_tokens {
            return Err(diagnostic(CommandDiagnosticCode::TokenLimit, at));
        }
        let offset = at;
        let (kind, text) = match bytes[at] {
            b'(' => {
                at += 1;
                (TokenKind::LeftParenthesis, String::new())
            }
            b')' => {
                at += 1;
                (TokenKind::RightParenthesis, String::new())
            }
            b',' => {
                at += 1;
                (TokenKind::Comma, String::new())
            }
            b'\'' => {
                at += 1;
                let mut value = String::new();
                let mut closed = false;
                while at < bytes.len() {
                    if bytes[at] == b'\'' {
                        if bytes.get(at + 1) == Some(&b'\'') {
                            value.push('\'');
                            at += 2;
                        } else {
                            at += 1;
                            closed = true;
                            break;
                        }
                    } else if bytes[at].is_ascii_control() {
                        return Err(diagnostic(CommandDiagnosticCode::InvalidCharacter, at));
                    } else {
                        value.push(char::from(bytes[at]));
                        at += 1;
                    }
                    if value.len() > limits.max_token_bytes {
                        return Err(diagnostic(CommandDiagnosticCode::TokenLength, offset));
                    }
                }
                if !closed {
                    return Err(diagnostic(CommandDiagnosticCode::UnterminatedQuote, offset));
                }
                (TokenKind::Quoted, value)
            }
            byte if byte.is_ascii_control() || matches!(byte, b';' | b'`') => {
                return Err(diagnostic(CommandDiagnosticCode::InvalidCharacter, at));
            }
            _ => {
                let start = at;
                while at < bytes.len()
                    && !bytes[at].is_ascii_whitespace()
                    && !matches!(bytes[at], b'(' | b')' | b',')
                {
                    if bytes[at].is_ascii_control() || matches!(bytes[at], b';' | b'`' | b'\'') {
                        return Err(diagnostic(CommandDiagnosticCode::InvalidCharacter, at));
                    }
                    at += 1;
                }
                if at - start > limits.max_token_bytes {
                    return Err(diagnostic(CommandDiagnosticCode::TokenLength, start));
                }
                (
                    TokenKind::Word,
                    String::from_utf8(bytes[start..at].to_vec())
                        .map_err(|_| diagnostic(CommandDiagnosticCode::InvalidCharacter, start))?,
                )
            }
        };
        tokens.push(Token {
            kind,
            text: Zeroizing::new(text),
            offset,
        });
    }
    Ok(tokens)
}

pub(crate) fn diagnostic(code: CommandDiagnosticCode, offset: usize) -> CommandDiagnostic {
    let message = match code {
        CommandDiagnosticCode::Empty => "command input is empty",
        CommandDiagnosticCode::InputLimit => "command input exceeds its byte limit",
        CommandDiagnosticCode::TokenLimit => "command token count exceeds its limit",
        CommandDiagnosticCode::TokenLength => "command token exceeds its byte limit",
        CommandDiagnosticCode::UnterminatedQuote => "quoted value is not terminated",
        CommandDiagnosticCode::InvalidCharacter => "command contains an invalid character",
        CommandDiagnosticCode::UnknownFamily => "command family is not recognized",
        CommandDiagnosticCode::UnexpectedToken => "command token is out of place",
        CommandDiagnosticCode::UnterminatedOperand => "operand value is not terminated",
        CommandDiagnosticCode::ParenthesisDepth => "operand nesting exceeds its limit",
        CommandDiagnosticCode::PositionalCount => "command positional count is invalid",
        CommandDiagnosticCode::UnknownOperand => "operand is not supported by this command",
        CommandDiagnosticCode::DuplicateOperand => "operand occurs more than once",
        CommandDiagnosticCode::OperandValueLimit => "operand values exceed their limit",
        CommandDiagnosticCode::UnsupportedFamily => "command execution belongs to a later package",
        CommandDiagnosticCode::InvalidValue => "command operand value is invalid",
        CommandDiagnosticCode::Unauthorized => "command authority check denied",
        CommandDiagnosticCode::NotFound => "command target was not found",
        CommandDiagnosticCode::Conflict => "command conflicts with current security state",
        CommandDiagnosticCode::ResourceExhausted => "security authority capacity is exhausted",
        CommandDiagnosticCode::ProviderFailure => "security authority is unavailable",
    };
    CommandDiagnostic {
        code,
        offset,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn generated_catalog_recognizes_all_34_families_and_phrase_alias() {
        assert_eq!(command_descriptors().len(), 34);
        let families = command_descriptors()
            .iter()
            .map(|descriptor| recognize_command(descriptor.keyword(), Default::default()).unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(families.len(), 34);
        assert_eq!(
            recognize_command("PHRASE", Default::default()).unwrap(),
            CommandFamily::Password
        );
    }

    #[test]
    fn supplied_class_metadata_is_generated_and_unique() {
        let classes = supplied_class_descriptors();
        assert_eq!(classes.len(), 24);
        assert_eq!(
            classes
                .iter()
                .map(|descriptor| descriptor.name)
                .collect::<BTreeSet<_>>()
                .len(),
            classes.len()
        );
        let dataset = classes
            .iter()
            .find(|descriptor| descriptor.name == "DATASET")
            .unwrap();
        assert!(dataset.active && dataset.generic_allowed);
        assert!(
            classes
                .iter()
                .all(|descriptor| descriptor.name != "CUSTOMCLS")
        );
    }

    #[test]
    fn core_forms_validate_without_retaining_secret_values_in_public_dto() {
        let forms = [
            "ADDGROUP OPER OWNER(SYS1) SUPGROUP(SYS1)",
            "ADDUSER USER1 DFLTGRP(OPER) PASSWORD('Mixed Case Secret') OMVS(UID(1001))",
            "ADDSD 'USER1.**' GENERIC OWNER(USER1) UACC(READ)",
            "CONNECT USER1 GROUP(OPER) AUTHORITY(USE)",
            "PERMIT 'USER1.**' CLASS(DATASET) ID(USER1) ACCESS(UPDATE)",
            "RDEFINE FACILITY APP.RESOURCE OWNER(USER1) UACC(NONE)",
            "RLIST FACILITY APP.RESOURCE ALL",
            "SEARCH CLASS(DATASET) MASK(USER1)",
        ];
        for form in forms {
            validate_command(form, Default::default())
                .unwrap_or_else(|problem| panic!("form failed with redacted diagnostic {problem}"));
        }
        let shown = format!(
            "{:?}",
            validate_command(forms[1], Default::default()).unwrap()
        );
        assert!(!shown.contains("Mixed Case Secret"));
    }

    #[test]
    fn malformed_unknown_duplicate_and_bounds_are_exact_and_redacted() {
        for (input, code) in [
            ("", CommandDiagnosticCode::Empty),
            ("NOTRACF X", CommandDiagnosticCode::UnknownFamily),
            (
                "ADDUSER USER1 UNKNOWN(value)",
                CommandDiagnosticCode::UnknownOperand,
            ),
            (
                "ADDUSER USER1 OWNER(A) OWNER(B)",
                CommandDiagnosticCode::DuplicateOperand,
            ),
            (
                "ADDUSER USER1 PASSWORD('never-shown)",
                CommandDiagnosticCode::UnterminatedQuote,
            ),
            ("RDEFINE FACILITY", CommandDiagnosticCode::PositionalCount),
        ] {
            let problem = validate_command(input, Default::default()).unwrap_err();
            assert_eq!(problem.code, code);
            assert!(!format!("{problem}").contains("never-shown"));
        }
        let limits = CommandLanguageLimits {
            max_input_bytes: 4,
            ..Default::default()
        };
        assert_eq!(
            validate_command("ADDUSER", limits).unwrap_err().code,
            CommandDiagnosticCode::InputLimit
        );
    }

    #[test]
    fn parser_handles_nested_segment_values_and_doubled_quotes() {
        let parsed = parse_command(
            "ADDUSER USER1 NAME('O''BRIEN') OMVS(UID(1001) HOME('/u/user1'))",
            Default::default(),
        )
        .unwrap();
        assert_eq!(parsed.operand("NAME").unwrap().first(), Some("O'BRIEN"));
        assert_eq!(
            parsed.operand("OMVS").unwrap().values().collect::<Vec<_>>(),
            ["UID", "1001", "HOME", "/u/user1"]
        );
    }

    proptest! {
        #[test]
        fn bounded_command_parser_never_panics(input in ".{0,2048}") {
            let _ = recognize_command(&input, Default::default());
            let _ = validate_command(&input, Default::default());
        }
    }
}
