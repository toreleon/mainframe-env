//! Owned bounded Db2 AST primitives.

use crate::{Db2SourceSpan, Db2StringKind};
use std::fmt;

const MAX_AST_MESSAGE_BYTES: usize = 256;
const MAX_IDENTIFIER_BYTES_CEILING: usize = 1024;
const MAX_NAME_PARTS_CEILING: usize = 16;
const MAX_LITERAL_BYTES_CEILING: usize = 8 * 1024 * 1024;
const MAX_EXPRESSION_NODES_CEILING: usize = 262_144;
const MAX_LIST_ITEMS_CEILING: usize = 65_536;
const MAX_EXPRESSION_DEPTH_CEILING: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2AstLimits {
    pub max_identifier_bytes: usize,
    pub max_name_parts: usize,
    pub max_literal_bytes: usize,
    pub max_expression_nodes: usize,
    pub max_list_items: usize,
    pub max_expression_depth: usize,
}

impl Default for Db2AstLimits {
    fn default() -> Self {
        Self {
            max_identifier_bytes: 128,
            max_name_parts: 3,
            max_literal_bytes: 1024 * 1024,
            max_expression_nodes: 65_536,
            max_list_items: 1024,
            max_expression_depth: 128,
        }
    }
}

impl Db2AstLimits {
    fn validate(self) -> Result<(), Db2AstError> {
        if self.max_identifier_bytes == 0
            || self.max_identifier_bytes > MAX_IDENTIFIER_BYTES_CEILING
            || self.max_name_parts == 0
            || self.max_name_parts > MAX_NAME_PARTS_CEILING
            || self.max_literal_bytes == 0
            || self.max_literal_bytes > MAX_LITERAL_BYTES_CEILING
            || self.max_expression_nodes == 0
            || self.max_expression_nodes > MAX_EXPRESSION_NODES_CEILING
            || self.max_expression_nodes > u32::MAX as usize
            || self.max_list_items == 0
            || self.max_list_items > MAX_LIST_ITEMS_CEILING
            || self.max_expression_depth == 0
            || self.max_expression_depth > MAX_EXPRESSION_DEPTH_CEILING
        {
            return Err(Db2AstError::new(
                Db2AstErrorCode::InvalidLimits,
                "Db2 AST limits are zero or exceed the compiled ceiling",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2AstErrorCode {
    InvalidLimits,
    InvalidIdentifier,
    IdentifierTooLong,
    InvalidQualifiedName,
    InvalidDataType,
    LiteralTooLarge,
    TooManyNodes,
    TooManyListItems,
    ExpressionTooDeep,
    InvalidExpressionReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2AstError {
    pub code: Db2AstErrorCode,
    pub message: String,
}

impl Db2AstError {
    fn new(code: Db2AstErrorCode, message: impl AsRef<str>) -> Self {
        Self {
            code,
            message: bounded_text(message.as_ref(), MAX_AST_MESSAGE_BYTES),
        }
    }
}

impl fmt::Display for Db2AstError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Db2AstError {}

/// A normalized SQL identifier. Ordinary ASCII letters are folded to uppercase;
/// delimited identifiers preserve case and discard insignificant trailing spaces.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Db2Identifier {
    value: String,
    delimited: bool,
}

impl Db2Identifier {
    pub fn new(
        value: impl Into<String>,
        delimited: bool,
        limits: Db2AstLimits,
    ) -> Result<Self, Db2AstError> {
        limits.validate()?;
        let mut value = value.into();
        if delimited {
            let significant_bytes = value.trim_end_matches(' ').len();
            value.truncate(significant_bytes);
        } else {
            value.make_ascii_uppercase();
        }
        if value.is_empty() || value.as_bytes().contains(&0) {
            return Err(Db2AstError::new(
                Db2AstErrorCode::InvalidIdentifier,
                "Db2 identifier is empty or contains NUL",
            ));
        }
        if value.len() > limits.max_identifier_bytes {
            return Err(Db2AstError::new(
                Db2AstErrorCode::IdentifierTooLong,
                "Db2 identifier exceeds the configured UTF-8 byte limit",
            ));
        }
        if !delimited && !ordinary_identifier(&value) {
            return Err(Db2AstError::new(
                Db2AstErrorCode::InvalidIdentifier,
                "Db2 ordinary identifier has an invalid character or initial",
            ));
        }
        Ok(Self { value, delimited })
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn is_delimited(&self) -> bool {
        self.delimited
    }
}

fn ordinary_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || !first.is_ascii())
        && characters.all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || character == '_'
                || !character.is_ascii()
        })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Db2QualifiedName {
    parts: Vec<Db2Identifier>,
}

impl Db2QualifiedName {
    pub fn new(parts: Vec<Db2Identifier>, limits: Db2AstLimits) -> Result<Self, Db2AstError> {
        limits.validate()?;
        if parts.is_empty() || parts.len() > limits.max_name_parts {
            return Err(Db2AstError::new(
                Db2AstErrorCode::InvalidQualifiedName,
                "Db2 qualified name has no parts or exceeds the configured part limit",
            ));
        }
        Ok(Self { parts })
    }

    #[must_use]
    pub fn parts(&self) -> &[Db2Identifier] {
        &self.parts
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2HostReference {
    variable: Db2Identifier,
    indicator: Option<Db2Identifier>,
}

impl Db2HostReference {
    #[must_use]
    pub const fn new(variable: Db2Identifier, indicator: Option<Db2Identifier>) -> Self {
        Self {
            variable,
            indicator,
        }
    }

    #[must_use]
    pub const fn variable(&self) -> &Db2Identifier {
        &self.variable
    }

    #[must_use]
    pub const fn indicator(&self) -> Option<&Db2Identifier> {
        self.indicator.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2BuiltInType {
    SmallInt,
    Integer,
    BigInt,
    Decimal,
    Float,
    Real,
    Double,
    DecFloat,
    Character,
    VarChar,
    Clob,
    Graphic,
    VarGraphic,
    DbClob,
    Binary,
    VarBinary,
    Blob,
    Date,
    Time,
    Timestamp,
    RowId,
    Xml,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2BuiltInDataType {
    kind: Db2BuiltInType,
    arguments: Vec<u32>,
    with_time_zone: bool,
}

impl Db2BuiltInDataType {
    pub fn new(
        kind: Db2BuiltInType,
        arguments: Vec<u32>,
        with_time_zone: bool,
        limits: Db2AstLimits,
    ) -> Result<Self, Db2AstError> {
        limits.validate()?;
        if arguments.len() > 2 || arguments.len() > limits.max_list_items {
            return Err(Db2AstError::new(
                Db2AstErrorCode::InvalidDataType,
                "Db2 built-in type has too many numeric arguments",
            ));
        }
        Ok(Self {
            kind,
            arguments,
            with_time_zone,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> Db2BuiltInType {
        self.kind
    }

    #[must_use]
    pub fn arguments(&self) -> &[u32] {
        &self.arguments
    }

    #[must_use]
    pub const fn with_time_zone(&self) -> bool {
        self.with_time_zone
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2DataType {
    BuiltIn(Db2BuiltInDataType),
    Distinct(Db2QualifiedName),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2Literal {
    Null,
    Boolean(bool),
    Number(String),
    String { kind: Db2StringKind, value: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2UnaryOperator {
    Positive,
    Negative,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Concatenate,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Db2ExpressionId(u32);

impl Db2ExpressionId {
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ExpressionKind {
    Literal(Db2Literal),
    Column(Db2QualifiedName),
    HostVariable(Db2HostReference),
    ParameterMarker,
    Unary {
        operator: Db2UnaryOperator,
        operand: Db2ExpressionId,
    },
    Binary {
        left: Db2ExpressionId,
        operator: Db2BinaryOperator,
        right: Db2ExpressionId,
    },
    Function {
        name: Db2QualifiedName,
        arguments: Vec<Db2ExpressionId>,
    },
    Cast {
        expression: Db2ExpressionId,
        data_type: Db2DataType,
    },
    IsNull {
        expression: Db2ExpressionId,
        negated: bool,
    },
    Case {
        operand: Option<Db2ExpressionId>,
        branches: Vec<(Db2ExpressionId, Db2ExpressionId)>,
        otherwise: Option<Db2ExpressionId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Expression {
    kind: Db2ExpressionKind,
    span: Db2SourceSpan,
}

impl Db2Expression {
    #[must_use]
    pub const fn kind(&self) -> &Db2ExpressionKind {
        &self.kind
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Append-only expression arena. Every node can reference only earlier nodes,
/// preventing cycles and making depth validation deterministic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ExpressionArena {
    limits: Db2AstLimits,
    nodes: Vec<Db2Expression>,
    depths: Vec<usize>,
}

impl Db2ExpressionArena {
    pub fn new(limits: Db2AstLimits) -> Result<Self, Db2AstError> {
        limits.validate()?;
        Ok(Self {
            limits,
            nodes: Vec::new(),
            depths: Vec::new(),
        })
    }

    pub fn push(
        &mut self,
        kind: Db2ExpressionKind,
        span: Db2SourceSpan,
    ) -> Result<Db2ExpressionId, Db2AstError> {
        if self.nodes.len() >= self.limits.max_expression_nodes {
            return Err(Db2AstError::new(
                Db2AstErrorCode::TooManyNodes,
                "Db2 expression arena exceeds the configured node limit",
            ));
        }
        validate_kind_bounds(&kind, self.limits)?;
        let references = expression_references(&kind);
        let mut depth = 1_usize;
        for reference in references {
            let referenced_depth = self.depths.get(reference.0 as usize).ok_or_else(|| {
                Db2AstError::new(
                    Db2AstErrorCode::InvalidExpressionReference,
                    "Db2 expression references a missing or forward node",
                )
            })?;
            depth = depth.max(referenced_depth.saturating_add(1));
        }
        if depth > self.limits.max_expression_depth {
            return Err(Db2AstError::new(
                Db2AstErrorCode::ExpressionTooDeep,
                "Db2 expression exceeds the configured depth limit",
            ));
        }
        let index = u32::try_from(self.nodes.len()).map_err(|_| {
            Db2AstError::new(
                Db2AstErrorCode::TooManyNodes,
                "Db2 expression identity exceeds u32",
            )
        })?;
        self.nodes.push(Db2Expression { kind, span });
        self.depths.push(depth);
        Ok(Db2ExpressionId(index))
    }

    #[must_use]
    pub fn get(&self, id: Db2ExpressionId) -> Option<&Db2Expression> {
        self.nodes.get(id.0 as usize)
    }

    #[must_use]
    pub fn nodes(&self) -> &[Db2Expression] {
        &self.nodes
    }
}

fn validate_kind_bounds(kind: &Db2ExpressionKind, limits: Db2AstLimits) -> Result<(), Db2AstError> {
    match kind {
        Db2ExpressionKind::Literal(Db2Literal::Number(value))
        | Db2ExpressionKind::Literal(Db2Literal::String { value, .. })
            if value.len() > limits.max_literal_bytes =>
        {
            Err(Db2AstError::new(
                Db2AstErrorCode::LiteralTooLarge,
                "Db2 literal exceeds the configured byte limit",
            ))
        }
        Db2ExpressionKind::Function { arguments, .. }
            if arguments.len() > limits.max_list_items =>
        {
            Err(Db2AstError::new(
                Db2AstErrorCode::TooManyListItems,
                "Db2 function has too many arguments",
            ))
        }
        Db2ExpressionKind::Case { branches, .. } if branches.len() > limits.max_list_items => {
            Err(Db2AstError::new(
                Db2AstErrorCode::TooManyListItems,
                "Db2 CASE expression has too many branches",
            ))
        }
        _ => Ok(()),
    }
}

fn expression_references(kind: &Db2ExpressionKind) -> Vec<Db2ExpressionId> {
    match kind {
        Db2ExpressionKind::Literal(_)
        | Db2ExpressionKind::Column(_)
        | Db2ExpressionKind::HostVariable(_)
        | Db2ExpressionKind::ParameterMarker => Vec::new(),
        Db2ExpressionKind::Unary { operand, .. } => vec![*operand],
        Db2ExpressionKind::Binary { left, right, .. } => vec![*left, *right],
        Db2ExpressionKind::Function { arguments, .. } => arguments.clone(),
        Db2ExpressionKind::Cast { expression, .. }
        | Db2ExpressionKind::IsNull { expression, .. } => vec![*expression],
        Db2ExpressionKind::Case {
            operand,
            branches,
            otherwise,
        } => operand
            .iter()
            .chain(branches.iter().flat_map(|(when, then)| [when, then]))
            .chain(otherwise.iter())
            .copied()
            .collect(),
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
    use crate::{Db2SourceLocation, Db2SourceSpan};

    fn span() -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: 0,
            end_byte: 1,
            start: Db2SourceLocation::START,
            end: Db2SourceLocation { line: 1, column: 2 },
        }
    }

    fn identifier(value: &str) -> Db2Identifier {
        Db2Identifier::new(value, false, Db2AstLimits::default()).unwrap()
    }

    fn name(value: &str) -> Db2QualifiedName {
        Db2QualifiedName::new(vec![identifier(value)], Db2AstLimits::default()).unwrap()
    }

    #[test]
    fn identifiers_fold_and_delimited_trailing_spaces_are_insignificant() {
        let ordinary = Db2Identifier::new("mixed_1", false, Db2AstLimits::default()).unwrap();
        assert_eq!(ordinary.value(), "MIXED_1");
        assert!(!ordinary.is_delimited());
        let delimited = Db2Identifier::new(" Mixed  ", true, Db2AstLimits::default()).unwrap();
        assert_eq!(delimited.value(), " Mixed");
        assert!(delimited.is_delimited());
        assert_eq!(
            Db2Identifier::new("1BAD", false, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2AstErrorCode::InvalidIdentifier
        );
        assert_eq!(
            Db2Identifier::new("A".repeat(129), false, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2AstErrorCode::IdentifierTooLong
        );
    }

    #[test]
    fn qualified_names_and_type_arguments_are_bounded() {
        let limits = Db2AstLimits {
            max_name_parts: 2,
            max_list_items: 1,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            Db2QualifiedName::new(
                vec![identifier("A"), identifier("B"), identifier("C")],
                limits
            )
            .unwrap_err()
            .code,
            Db2AstErrorCode::InvalidQualifiedName
        );
        assert_eq!(
            Db2BuiltInDataType::new(Db2BuiltInType::Decimal, vec![9, 2], false, limits)
                .unwrap_err()
                .code,
            Db2AstErrorCode::InvalidDataType
        );
    }

    #[test]
    fn expression_arena_is_owned_acyclic_and_inspectable() {
        let mut arena = Db2ExpressionArena::new(Db2AstLimits::default()).unwrap();
        let column = arena
            .push(Db2ExpressionKind::Column(name("AMOUNT")), span())
            .unwrap();
        let number = arena
            .push(
                Db2ExpressionKind::Literal(Db2Literal::Number("2".into())),
                span(),
            )
            .unwrap();
        let product = arena
            .push(
                Db2ExpressionKind::Binary {
                    left: column,
                    operator: Db2BinaryOperator::Multiply,
                    right: number,
                },
                span(),
            )
            .unwrap();
        let predicate = arena
            .push(
                Db2ExpressionKind::IsNull {
                    expression: product,
                    negated: true,
                },
                span(),
            )
            .unwrap();
        assert_eq!(predicate.index(), 3);
        assert!(matches!(
            arena.get(predicate).unwrap().kind(),
            Db2ExpressionKind::IsNull { negated: true, .. }
        ));
        assert_eq!(arena.nodes().len(), 4);
    }

    #[test]
    fn invalid_references_depth_nodes_literals_and_lists_fail_closed() {
        let limits = Db2AstLimits {
            max_literal_bytes: 2,
            max_expression_nodes: 3,
            max_list_items: 1,
            max_expression_depth: 2,
            ..Db2AstLimits::default()
        };
        let mut arena = Db2ExpressionArena::new(limits).unwrap();
        assert_eq!(
            arena
                .push(
                    Db2ExpressionKind::Literal(Db2Literal::Number("123".into())),
                    span()
                )
                .unwrap_err()
                .code,
            Db2AstErrorCode::LiteralTooLarge
        );
        assert_eq!(
            arena
                .push(
                    Db2ExpressionKind::Unary {
                        operator: Db2UnaryOperator::Negative,
                        operand: Db2ExpressionId(9)
                    },
                    span()
                )
                .unwrap_err()
                .code,
            Db2AstErrorCode::InvalidExpressionReference
        );
        let one = arena
            .push(
                Db2ExpressionKind::Literal(Db2Literal::Number("1".into())),
                span(),
            )
            .unwrap();
        assert_eq!(
            arena
                .push(
                    Db2ExpressionKind::Function {
                        name: name("F"),
                        arguments: vec![one, one]
                    },
                    span()
                )
                .unwrap_err()
                .code,
            Db2AstErrorCode::TooManyListItems
        );
        let unary = arena
            .push(
                Db2ExpressionKind::Unary {
                    operator: Db2UnaryOperator::Negative,
                    operand: one,
                },
                span(),
            )
            .unwrap();
        assert_eq!(
            arena
                .push(
                    Db2ExpressionKind::Unary {
                        operator: Db2UnaryOperator::Negative,
                        operand: unary,
                    },
                    span()
                )
                .unwrap_err()
                .code,
            Db2AstErrorCode::ExpressionTooDeep
        );
        arena
            .push(Db2ExpressionKind::ParameterMarker, span())
            .unwrap();
        assert_eq!(
            arena
                .push(Db2ExpressionKind::ParameterMarker, span())
                .unwrap_err()
                .code,
            Db2AstErrorCode::TooManyNodes
        );
    }
}
