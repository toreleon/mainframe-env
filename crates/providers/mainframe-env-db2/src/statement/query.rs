//! Owned syntax for one common Db2 SELECT subselect.

use crate::{
    Db2AstLimits, Db2BinaryOperator, Db2ExpressionArena, Db2ExpressionKind, Db2Identifier,
    Db2ParsedExpression, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind,
    Db2UnaryOperator, lex_db2, parse_db2_expression,
};

/// Whether the SELECT clause used its implicit default or an explicit set
/// quantifier. Keeping implicit and explicit ALL distinct preserves syntax
/// without assigning execution semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SelectQuantifier {
    ImplicitAll,
    All,
    Distinct,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2QueryExpression {
    parsed: Db2ParsedExpression,
    span: Db2SourceSpan,
}

impl Db2QueryExpression {
    #[must_use]
    pub const fn parsed(&self) -> &Db2ParsedExpression {
        &self.parsed
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2SelectItem {
    Expression(Db2QueryExpression),
    Wildcard {
        qualifier: Option<Db2QualifiedName>,
        span: Db2SourceSpan,
    },
}

impl Db2SelectItem {
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        match self {
            Self::Expression(expression) => expression.span(),
            Self::Wildcard { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NamedTableSource {
    name: Db2QualifiedName,
    span: Db2SourceSpan,
}

impl Db2NamedTableSource {
    #[must_use]
    pub const fn name(&self) -> &Db2QualifiedName {
        &self.name
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2OrderDirection {
    Unspecified,
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2OrderKey {
    Expression(Db2QueryExpression),
    Ordinal(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2OrderByItem {
    key: Db2OrderKey,
    direction: Db2OrderDirection,
    span: Db2SourceSpan,
}

impl Db2OrderByItem {
    #[must_use]
    pub const fn key(&self) -> &Db2OrderKey {
        &self.key
    }

    #[must_use]
    pub const fn direction(&self) -> Db2OrderDirection {
        self.direction
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2OffsetClause {
    row_count: u64,
    span: Db2SourceSpan,
}

impl Db2OffsetClause {
    #[must_use]
    pub const fn row_count(&self) -> u64 {
        self.row_count
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2FetchPosition {
    First,
    Next,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2FetchClause {
    position: Db2FetchPosition,
    row_count: Option<u64>,
    span: Db2SourceSpan,
}

impl Db2FetchClause {
    #[must_use]
    pub const fn position(&self) -> Db2FetchPosition {
        self.position
    }

    /// `None` preserves the source form whose Db2 default is one row.
    #[must_use]
    pub const fn explicit_row_count(&self) -> Option<u64> {
        self.row_count
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// One bounded SELECT subselect. This type is intentionally disconnected from
/// binding, execution, authorization, SQLCA mapping, and whole-row recognition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SelectCore {
    quantifier: Db2SelectQuantifier,
    items: Vec<Db2SelectItem>,
    sources: Vec<Db2NamedTableSource>,
    where_condition: Option<Db2QueryExpression>,
    group_by: Vec<Db2QueryExpression>,
    having: Option<Db2QueryExpression>,
    order_by: Vec<Db2OrderByItem>,
    offset: Option<Db2OffsetClause>,
    fetch: Option<Db2FetchClause>,
    span: Db2SourceSpan,
}

impl Db2SelectCore {
    #[must_use]
    pub const fn quantifier(&self) -> Db2SelectQuantifier {
        self.quantifier
    }

    #[must_use]
    pub fn items(&self) -> &[Db2SelectItem] {
        &self.items
    }

    #[must_use]
    pub fn sources(&self) -> &[Db2NamedTableSource] {
        &self.sources
    }

    #[must_use]
    pub const fn where_condition(&self) -> Option<&Db2QueryExpression> {
        self.where_condition.as_ref()
    }

    #[must_use]
    pub fn group_by(&self) -> &[Db2QueryExpression] {
        &self.group_by
    }

    #[must_use]
    pub const fn having(&self) -> Option<&Db2QueryExpression> {
        self.having.as_ref()
    }

    #[must_use]
    pub fn order_by(&self) -> &[Db2OrderByItem] {
        &self.order_by
    }

    #[must_use]
    pub const fn offset(&self) -> Option<&Db2OffsetClause> {
        self.offset.as_ref()
    }

    #[must_use]
    pub const fn fetch(&self) -> Option<&Db2FetchClause> {
        self.fetch.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

mod parser;
#[cfg(test)]
mod tests;

pub use parser::parse_db2_select_core;
