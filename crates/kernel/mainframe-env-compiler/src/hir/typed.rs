use super::{HirProblem, HirStatement, StatementKind, StatementOption, StatementOptionKind};
use crate::{CobolLayout, CobolUsage, DataCategory, LosslessSyntax, SemanticModel, SourceSpan};
use mainframe_env_diagnostics::SourceSpan as IrSourceSpan;
use mainframe_env_source::SourceBundle;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirResolvedStatement {
    Add(HirAddStatement),
    Compute(HirComputeStatement),
    Cics(HirCicsStatement),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirDataReference {
    pub qualified_name: String,
    pub offset: usize,
    pub length: usize,
    pub category: DataCategory,
    pub usage: CobolUsage,
    pub digits: usize,
    pub scale: usize,
    pub signed: bool,
    pub dynamic: bool,
    pub dynamic_limit: Option<usize>,
    pub allocated: bool,
}

impl From<&CobolLayout> for HirDataReference {
    fn from(layout: &CobolLayout) -> Self {
        Self {
            qualified_name: layout.qualified_name.clone(),
            offset: layout.offset,
            length: layout.length,
            category: layout.category,
            usage: layout.usage,
            digits: layout.digits,
            scale: layout.scale,
            signed: layout.signed,
            dynamic: layout.dynamic,
            dynamic_limit: layout.dynamic_limit,
            allocated: layout.allocated,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirNumericLiteral {
    pub negative: bool,
    pub digits: String,
    pub scale: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirUnaryOperator {
    Plus,
    Negate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirBinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirNumericExpression {
    Literal(HirNumericLiteral),
    Data(HirDataReference),
    LengthOf(HirDataReference),
    Unary {
        operator: HirUnaryOperator,
        operand: Box<Self>,
    },
    Binary {
        operator: HirBinaryOperator,
        left: Box<Self>,
        right: Box<Self>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirRounding {
    Truncate,
    Rounded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirArithmeticReceiver {
    pub target: HirDataReference,
    pub rounding: HirRounding,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HirSizeErrorPolicy {
    pub on_size_error: bool,
    pub not_on_size_error: bool,
    pub explicit_terminator: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirCorrespondingPair {
    pub source: HirDataReference,
    pub receiver: HirArithmeticReceiver,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirAddMode {
    To {
        sources: Vec<HirNumericExpression>,
        receivers: Vec<HirArithmeticReceiver>,
    },
    Giving {
        sources: Vec<HirNumericExpression>,
        augend: Option<HirNumericExpression>,
        receivers: Vec<HirArithmeticReceiver>,
    },
    Corresponding {
        source_group: HirDataReference,
        target_group: HirDataReference,
        pairs: Vec<HirCorrespondingPair>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirAddStatement {
    pub mode: HirAddMode,
    pub size_error: HirSizeErrorPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirComputeStatement {
    pub expression: HirNumericExpression,
    pub receivers: Vec<HirArithmeticReceiver>,
    pub size_error: HirSizeErrorPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirCicsOperation {
    Read,
    Rewrite,
    Syncpoint,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOperandName {
    File,
    Dataset,
    From,
    Ridfld,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirCicsValue {
    Literal(String),
    Data(HirDataReference),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirCicsNamedOperand {
    pub name: HirCicsOperandName,
    pub value: HirCicsValue,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOption {
    Update,
    Rollback,
    NoHandle,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOutputName {
    Into,
    Resp,
    Resp2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirCicsOutputBinding {
    pub name: HirCicsOutputName,
    pub target: HirDataReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirCicsConditionPolicy {
    Default,
    NoHandle,
    Respond {
        response: HirDataReference,
        response2: Option<HirDataReference>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirCicsStatement {
    pub operation: HirCicsOperation,
    pub operands: Vec<HirCicsNamedOperand>,
    pub options: BTreeSet<HirCicsOption>,
    pub outputs: Vec<HirCicsOutputBinding>,
    pub condition_policy: HirCicsConditionPolicy,
}

enum ResolutionFailure {
    Unsupported,
    Invalid(String),
}

type Resolution<T> = Result<T, ResolutionFailure>;
type CicsClauses = (BTreeMap<String, Vec<String>>, Vec<String>);
const MAX_TYPED_EXPRESSION_DEPTH: usize = 128;

pub(super) fn resolve_statements(
    statements: &mut [HirStatement],
    semantic: &SemanticModel,
) -> Result<(), HirProblem> {
    for statement in statements {
        statement.resolved = resolve_statement(statement, semantic)?;
    }
    Ok(())
}

pub(super) fn attach_locations(statements: &mut [HirStatement], source: &SourceBundle) {
    for statement in statements {
        statement.location = statement.source.first().and_then(|span| {
            let file = source
                .files()
                .iter()
                .find(|file| file.path() == &span.source)?;
            IrSourceSpan::new(file.id(), span.source_start..span.source_end).ok()
        });
    }
}

pub(super) fn source_spans_for_range(
    syntax: &LosslessSyntax,
    procedure_offset: usize,
    range: &Range<usize>,
) -> Vec<SourceSpan> {
    if range.start >= range.end {
        return Vec::new();
    }
    crate::syntax::source_spans(
        syntax.semantic_origins(),
        procedure_offset + range.start..procedure_offset + range.end,
    )
}

fn resolve_statement(
    statement: &HirStatement,
    semantic: &SemanticModel,
) -> Result<Option<HirResolvedStatement>, HirProblem> {
    let resolved = match statement.kind {
        StatementKind::Add => resolve_add(&statement.arguments, &statement.options, semantic)
            .map(HirResolvedStatement::Add),
        StatementKind::Compute => {
            resolve_compute(&statement.arguments, &statement.options, semantic)
                .map(HirResolvedStatement::Compute)
        }
        StatementKind::ExecCics => {
            resolve_cics(&statement.arguments, semantic).map(HirResolvedStatement::Cics)
        }
        _ => return Ok(None),
    };
    match resolved {
        Ok(resolved) => Ok(Some(resolved)),
        Err(ResolutionFailure::Unsupported) => Ok(None),
        Err(ResolutionFailure::Invalid(detail)) => Err(HirProblem::InvalidResolvedStatement(
            statement.kind,
            statement.line,
            detail,
        )),
    }
}

fn resolve_add(
    tokens: &[String],
    options: &[StatementOption],
    semantic: &SemanticModel,
) -> Resolution<HirAddStatement> {
    if tokens
        .first()
        .is_some_and(|token| matches!(token.as_str(), "CORRESPONDING" | "CORR"))
    {
        return resolve_add_corresponding(tokens, options, semantic);
    }
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "(" | ")"))
    {
        return Err(ResolutionFailure::Unsupported);
    }
    let to = tokens.iter().position(|token| token == "TO");
    let giving = tokens.iter().position(|token| token == "GIVING");
    let source_end = to
        .or(giving)
        .ok_or_else(|| ResolutionFailure::Invalid("ADD has no TO or GIVING boundary".into()))?;
    let sources = parse_numeric_items(&tokens[..source_end], semantic)?;
    if sources.is_empty() {
        return Err(ResolutionFailure::Invalid(
            "ADD has no source operand".into(),
        ));
    }
    // The executable decimal plan preserves COBOL's left-to-right arithmetic
    // context, so an ADD source list becomes a left-deep expression. Keep that
    // tree inside the same codec/runtime depth bound before lowering; larger
    // valid forms remain on the bounded legacy compatibility route.
    if sources.len().saturating_add(2) > MAX_TYPED_EXPRESSION_DEPTH {
        return Err(ResolutionFailure::Unsupported);
    }
    let mode = if let Some(giving) = giving {
        let augend = if let Some(to) = to {
            let values = parse_numeric_items(&tokens[to + 1..giving], semantic)?;
            match values.as_slice() {
                [value] => Some(value.clone()),
                _ => {
                    return Err(ResolutionFailure::Invalid(
                        "ADD GIVING requires one optional augend".into(),
                    ));
                }
            }
        } else {
            None
        };
        HirAddMode::Giving {
            sources,
            augend,
            receivers: parse_receivers(&tokens[giving + 1..], semantic)?,
        }
    } else {
        let to = to.expect("source boundary established by TO");
        HirAddMode::To {
            sources,
            receivers: parse_receivers(&tokens[to + 1..], semantic)?,
        }
    };
    Ok(HirAddStatement {
        mode,
        size_error: size_error_policy(options),
    })
}

fn resolve_add_corresponding(
    tokens: &[String],
    options: &[StatementOption],
    semantic: &SemanticModel,
) -> Resolution<HirAddStatement> {
    let (source_group, source_end) = data_reference_at(tokens, 1, semantic)?;
    if tokens.get(source_end).is_none_or(|token| token != "TO") {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING has no TO boundary".into(),
        ));
    }
    let (target_group, target_end) = data_reference_at(tokens, source_end + 1, semantic)?;
    let rounded = match tokens.get(target_end..).unwrap_or_default() {
        [] => HirRounding::Truncate,
        [word] if word == "ROUNDED" => HirRounding::Rounded,
        _ => return Err(ResolutionFailure::Unsupported),
    };
    if !is_group(source_group.category) || !is_group(target_group.category) {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING operands must be groups".into(),
        ));
    }
    let sources = semantic
        .layouts
        .iter()
        .filter(|layout| {
            layout.name != "FILLER"
                && is_numeric(layout.category)
                && descendant_of(layout, &source_group.qualified_name, &semantic.layouts)
        })
        .collect::<Vec<_>>();
    let pairs = semantic
        .layouts
        .iter()
        .filter(|layout| {
            layout.name != "FILLER"
                && is_numeric(layout.category)
                && descendant_of(layout, &target_group.qualified_name, &semantic.layouts)
        })
        .filter_map(|target| {
            let matches = sources
                .iter()
                .filter(|source| source.name == target.name)
                .copied()
                .collect::<Vec<_>>();
            (matches.len() == 1).then(|| HirCorrespondingPair {
                source: matches[0].into(),
                receiver: HirArithmeticReceiver {
                    target: target.into(),
                    rounding: rounded,
                },
            })
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING has no unique numeric pair".into(),
        ));
    }
    Ok(HirAddStatement {
        mode: HirAddMode::Corresponding {
            source_group,
            target_group,
            pairs,
        },
        size_error: size_error_policy(options),
    })
}

fn resolve_compute(
    tokens: &[String],
    options: &[StatementOption],
    semantic: &SemanticModel,
) -> Resolution<HirComputeStatement> {
    let equals = tokens
        .iter()
        .position(|token| token == "=")
        .ok_or_else(|| ResolutionFailure::Invalid("COMPUTE has no equals boundary".into()))?;
    let receivers = parse_receivers(&tokens[..equals], semantic)?;
    let mut parser = ExpressionParser {
        tokens: &tokens[equals + 1..],
        position: 0,
        semantic,
    };
    let expression = parser.expression()?;
    if parser.position != parser.tokens.len() {
        return Err(ResolutionFailure::Unsupported);
    }
    Ok(HirComputeStatement {
        expression,
        receivers,
        size_error: size_error_policy(options),
    })
}

fn parse_numeric_items(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<Vec<HirNumericExpression>> {
    let mut result = Vec::new();
    let mut position = 0;
    while position < tokens.len() {
        if tokens[position] == "," {
            position += 1;
            continue;
        }
        let (value, end) = numeric_atom_at(tokens, position, semantic)?;
        result.push(value);
        position = end;
    }
    Ok(result)
}

fn parse_receivers(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<Vec<HirArithmeticReceiver>> {
    let mut receivers = Vec::new();
    let mut position = 0;
    while position < tokens.len() {
        if tokens[position] == "," {
            position += 1;
            continue;
        }
        let (target, end) = data_reference_at(tokens, position, semantic)?;
        require_numeric(&target)?;
        require_writable(&target)?;
        let rounded = tokens.get(end).is_some_and(|token| token == "ROUNDED");
        receivers.push(HirArithmeticReceiver {
            target,
            rounding: if rounded {
                HirRounding::Rounded
            } else {
                HirRounding::Truncate
            },
        });
        position = end + usize::from(rounded);
    }
    if receivers.is_empty() {
        Err(ResolutionFailure::Invalid(
            "arithmetic statement has no receiver".into(),
        ))
    } else {
        Ok(receivers)
    }
}

fn numeric_atom_at(
    tokens: &[String],
    position: usize,
    semantic: &SemanticModel,
) -> Resolution<(HirNumericExpression, usize)> {
    if tokens.get(position).is_some_and(|token| token == "LENGTH")
        && tokens.get(position + 1).is_some_and(|token| token == "OF")
    {
        let (reference, end) = data_reference_at(tokens, position + 2, semantic)?;
        return Ok((HirNumericExpression::LengthOf(reference), end));
    }
    if let Some(literal) = tokens
        .get(position)
        .and_then(|token| numeric_literal(token))
    {
        return Ok((HirNumericExpression::Literal(literal), position + 1));
    }
    if tokens
        .get(position)
        .is_some_and(|token| token == "FUNCTION")
    {
        return Err(ResolutionFailure::Unsupported);
    }
    let (reference, end) = data_reference_at(tokens, position, semantic)?;
    require_numeric(&reference)?;
    Ok((HirNumericExpression::Data(reference), end))
}

struct ExpressionParser<'a> {
    tokens: &'a [String],
    position: usize,
    semantic: &'a SemanticModel,
}

struct ParsedNumericExpression {
    expression: HirNumericExpression,
    depth: usize,
}

impl ExpressionParser<'_> {
    fn expression(&mut self) -> Resolution<HirNumericExpression> {
        self.additive(1).map(|parsed| parsed.expression)
    }

    fn additive(&mut self, source_depth: usize) -> Resolution<ParsedNumericExpression> {
        self.require_source_depth(source_depth)?;
        let mut value = self.multiplicative(source_depth)?;
        while let Some(operator) =
            self.tokens
                .get(self.position)
                .and_then(|token| match token.as_str() {
                    "+" => Some(HirBinaryOperator::Add),
                    "-" => Some(HirBinaryOperator::Subtract),
                    _ => None,
                })
        {
            self.position += 1;
            let right = self.multiplicative(source_depth)?;
            value = self.binary(operator, value, right)?;
        }
        Ok(value)
    }

    fn multiplicative(&mut self, source_depth: usize) -> Resolution<ParsedNumericExpression> {
        self.require_source_depth(source_depth)?;
        let mut value = self.factor(source_depth)?;
        while let Some(operator) =
            self.tokens
                .get(self.position)
                .and_then(|token| match token.as_str() {
                    "*" => Some(HirBinaryOperator::Multiply),
                    "/" => Some(HirBinaryOperator::Divide),
                    _ => None,
                })
        {
            self.position += 1;
            let right = self.factor(source_depth)?;
            value = self.binary(operator, value, right)?;
        }
        Ok(value)
    }

    fn factor(&mut self, source_depth: usize) -> Resolution<ParsedNumericExpression> {
        self.require_source_depth(source_depth)?;
        if let Some(operator) =
            self.tokens
                .get(self.position)
                .and_then(|token| match token.as_str() {
                    "+" => Some(HirUnaryOperator::Plus),
                    "-" => Some(HirUnaryOperator::Negate),
                    _ => None,
                })
        {
            self.position += 1;
            let operand = self.factor(source_depth.saturating_add(1))?;
            let depth = self.checked_node_depth(operand.depth)?;
            return Ok(ParsedNumericExpression {
                expression: HirNumericExpression::Unary {
                    operator,
                    operand: Box::new(operand.expression),
                },
                depth,
            });
        }
        if self
            .tokens
            .get(self.position)
            .is_some_and(|token| token == "(")
        {
            self.position += 1;
            let value = self.additive(source_depth.saturating_add(1))?;
            if self
                .tokens
                .get(self.position)
                .is_none_or(|token| token != ")")
            {
                return Err(ResolutionFailure::Unsupported);
            }
            self.position += 1;
            return Ok(value);
        }
        let (value, end) = numeric_atom_at(self.tokens, self.position, self.semantic)?;
        if self.tokens.get(end).is_some_and(|token| token == "(") {
            return Err(ResolutionFailure::Unsupported);
        }
        self.position = end;
        Ok(ParsedNumericExpression {
            expression: value,
            depth: 1,
        })
    }

    fn binary(
        &self,
        operator: HirBinaryOperator,
        left: ParsedNumericExpression,
        right: ParsedNumericExpression,
    ) -> Resolution<ParsedNumericExpression> {
        let depth = self.checked_node_depth(left.depth.max(right.depth))?;
        Ok(ParsedNumericExpression {
            expression: HirNumericExpression::Binary {
                operator,
                left: Box::new(left.expression),
                right: Box::new(right.expression),
            },
            depth,
        })
    }

    fn checked_node_depth(&self, child_depth: usize) -> Resolution<usize> {
        let depth = child_depth.checked_add(1).ok_or_else(|| {
            ResolutionFailure::Invalid("COMPUTE expression tree depth overflowed".into())
        })?;
        if depth > MAX_TYPED_EXPRESSION_DEPTH {
            Err(ResolutionFailure::Invalid(
                "COMPUTE expression tree depth exceeds the typed limit".into(),
            ))
        } else {
            Ok(depth)
        }
    }

    fn require_source_depth(&self, depth: usize) -> Resolution<()> {
        if depth > MAX_TYPED_EXPRESSION_DEPTH {
            Err(ResolutionFailure::Invalid(
                "COMPUTE expression nesting exceeds the typed limit".into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn numeric_literal(token: &str) -> Option<HirNumericLiteral> {
    if matches!(token, "ZERO" | "ZEROS" | "ZEROES") {
        return Some(HirNumericLiteral {
            negative: false,
            digits: "0".into(),
            scale: 0,
        });
    }
    let (negative, unsigned) = token.strip_prefix('-').map_or_else(
        || (false, token.strip_prefix('+').unwrap_or(token)),
        |value| (true, value),
    );
    let mut parts = unsigned.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let joined = format!("{whole}{fraction}");
    let digits = joined.trim_start_matches('0');
    Some(HirNumericLiteral {
        negative,
        digits: if digits.is_empty() { "0" } else { digits }.into(),
        scale: fraction.len(),
    })
}

fn data_reference_at(
    tokens: &[String],
    start: usize,
    semantic: &SemanticModel,
) -> Resolution<(HirDataReference, usize)> {
    let first = tokens
        .get(start)
        .ok_or_else(|| ResolutionFailure::Invalid("data reference is missing".into()))?;
    if matches!(first.as_str(), "(" | ")" | "+" | "-" | "*" | "/") || first.starts_with(['\'', '"'])
    {
        return Err(ResolutionFailure::Unsupported);
    }
    let mut end = start + 1;
    while tokens
        .get(end)
        .is_some_and(|token| matches!(token.as_str(), "OF" | "IN"))
    {
        if tokens.get(end + 1).is_none() {
            return Err(ResolutionFailure::Invalid(
                "qualified data reference has no qualifier".into(),
            ));
        }
        end += 2;
    }
    if tokens.get(end).is_some_and(|token| token == "(") {
        return Err(ResolutionFailure::Unsupported);
    }
    if end == start + 1 && crate::special_register_named(first).is_some() {
        return Err(ResolutionFailure::Unsupported);
    }
    let spelling = tokens[start..end].join(" ");
    let layout = semantic.resolve(&spelling).map_err(|problem| {
        ResolutionFailure::Invalid(format!("cannot resolve {spelling}: {problem:?}"))
    })?;
    Ok((layout.into(), end))
}

fn require_numeric(reference: &HirDataReference) -> Resolution<()> {
    if is_numeric(reference.category) {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "{} is not numeric",
            reference.qualified_name
        )))
    }
}

fn require_writable(reference: &HirDataReference) -> Resolution<()> {
    if reference.allocated
        && !matches!(
            reference.category,
            DataCategory::Condition | DataCategory::Rename
        )
    {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "{} is not receiving storage",
            reference.qualified_name
        )))
    }
}

fn size_error_policy(options: &[StatementOption]) -> HirSizeErrorPolicy {
    HirSizeErrorPolicy {
        on_size_error: has_option(options, StatementOptionKind::OnSizeError),
        not_on_size_error: has_option(options, StatementOptionKind::NotOnSizeError),
        explicit_terminator: has_option(options, StatementOptionKind::ExplicitTerminator),
    }
}

fn has_option(options: &[StatementOption], expected: StatementOptionKind) -> bool {
    options.iter().any(|option| option.kind == expected)
}

fn descendant_of(layout: &CobolLayout, root: &str, layouts: &[CobolLayout]) -> bool {
    let mut parent = layout.parent.as_deref();
    while let Some(name) = parent {
        if name == root {
            return true;
        }
        parent = layouts
            .iter()
            .find(|candidate| candidate.qualified_name == name)
            .and_then(|candidate| candidate.parent.as_deref());
    }
    false
}

const fn is_numeric(category: DataCategory) -> bool {
    matches!(
        category,
        DataCategory::NumericDisplay
            | DataCategory::NumericEdited
            | DataCategory::PackedDecimal
            | DataCategory::Binary
            | DataCategory::FloatShort
            | DataCategory::FloatLong
    )
}

const fn is_group(category: DataCategory) -> bool {
    matches!(
        category,
        DataCategory::Group | DataCategory::NationalGroup | DataCategory::Utf8Group
    )
}

fn resolve_cics(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsStatement> {
    let mut body = tokens;
    if body.first().is_some_and(|token| token == "CICS") {
        body = &body[1..];
    }
    if body.last().is_some_and(|token| token == "END-EXEC") {
        body = &body[..body.len() - 1];
    }
    let operation = match body.first().map(String::as_str) {
        Some("READ") => HirCicsOperation::Read,
        Some("REWRITE") => HirCicsOperation::Rewrite,
        Some("SYNCPOINT") => HirCicsOperation::Syncpoint,
        _ => return Err(ResolutionFailure::Unsupported),
    };
    let (clauses, raw_options) = cics_clauses(&body[1..])?;
    let allowed_clauses: &[&str] = match operation {
        HirCicsOperation::Read => &["FILE", "DATASET", "RIDFLD", "INTO", "RESP", "RESP2"],
        HirCicsOperation::Rewrite => &["FILE", "DATASET", "FROM", "RESP", "RESP2"],
        HirCicsOperation::Syncpoint => &["RESP", "RESP2"],
    };
    let allowed_options: &[&str] = match operation {
        HirCicsOperation::Read => &["UPDATE", "NOHANDLE"],
        HirCicsOperation::Rewrite => &["NOHANDLE"],
        HirCicsOperation::Syncpoint => &["ROLLBACK", "NOHANDLE"],
    };
    if clauses
        .keys()
        .any(|name| !allowed_clauses.contains(&name.as_str()))
        || raw_options
            .iter()
            .any(|name| !allowed_options.contains(&name.as_str()))
    {
        return Err(ResolutionFailure::Unsupported);
    }
    let resources =
        usize::from(clauses.contains_key("FILE")) + usize::from(clauses.contains_key("DATASET"));
    if !matches!(operation, HirCicsOperation::Syncpoint) && resources != 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS file command requires exactly one FILE or DATASET".into(),
        ));
    }
    for required in match operation {
        HirCicsOperation::Read => &["RIDFLD", "INTO"][..],
        HirCicsOperation::Rewrite => &["FROM"][..],
        HirCicsOperation::Syncpoint => &[][..],
    } {
        if !clauses.contains_key(*required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires {required}"
            )));
        }
    }
    let mut operands = Vec::new();
    for (name, identity) in [
        ("FILE", HirCicsOperandName::File),
        ("DATASET", HirCicsOperandName::Dataset),
        ("FROM", HirCicsOperandName::From),
        ("RIDFLD", HirCicsOperandName::Ridfld),
    ] {
        if let Some(value) = clauses.get(name) {
            operands.push(HirCicsNamedOperand {
                name: identity,
                value: cics_value(value, semantic)?,
            });
        }
    }
    let mut outputs = Vec::new();
    for (name, identity) in [
        ("INTO", HirCicsOutputName::Into),
        ("RESP", HirCicsOutputName::Resp),
        ("RESP2", HirCicsOutputName::Resp2),
    ] {
        if let Some(value) = clauses.get(name) {
            let target = complete_data_reference(value, semantic)?;
            require_writable(&target)?;
            if matches!(identity, HirCicsOutputName::Resp | HirCicsOutputName::Resp2) {
                require_numeric(&target)?;
            }
            outputs.push(HirCicsOutputBinding {
                name: identity,
                target,
            });
        }
    }
    let options = raw_options
        .iter()
        .map(|option| match option.as_str() {
            "UPDATE" => HirCicsOption::Update,
            "ROLLBACK" => HirCicsOption::Rollback,
            "NOHANDLE" => HirCicsOption::NoHandle,
            _ => unreachable!("allowed CICS option"),
        })
        .collect::<BTreeSet<_>>();
    let response = output(&outputs, HirCicsOutputName::Resp).cloned();
    let response2 = output(&outputs, HirCicsOutputName::Resp2).cloned();
    if response.is_none() && response2.is_some() {
        return Err(ResolutionFailure::Invalid(
            "CICS RESP2 requires RESP".into(),
        ));
    }
    let condition_policy = if options.contains(&HirCicsOption::NoHandle) {
        HirCicsConditionPolicy::NoHandle
    } else if let Some(response) = response {
        HirCicsConditionPolicy::Respond {
            response,
            response2,
        }
    } else {
        HirCicsConditionPolicy::Default
    };
    Ok(HirCicsStatement {
        operation,
        operands,
        options,
        outputs,
        condition_policy,
    })
}

fn cics_clauses(tokens: &[String]) -> Resolution<CicsClauses> {
    let mut clauses = BTreeMap::new();
    let mut options = Vec::new();
    let mut position = 0;
    while position < tokens.len() {
        let name = tokens[position].clone();
        if tokens.get(position + 1).is_some_and(|token| token == "(") {
            let close = matching_close(tokens, position + 1)?;
            if close == position + 2
                || clauses
                    .insert(name, tokens[position + 2..close].to_vec())
                    .is_some()
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS operand is empty or duplicated".into(),
                ));
            }
            position = close + 1;
        } else {
            if options.contains(&name) {
                return Err(ResolutionFailure::Invalid(
                    "CICS option is duplicated".into(),
                ));
            }
            options.push(name);
            position += 1;
        }
    }
    Ok((clauses, options))
}

fn matching_close(tokens: &[String], open: usize) -> Resolution<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1).ok_or(ResolutionFailure::Unsupported)?;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(ResolutionFailure::Unsupported)
}

fn cics_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if let [value] = tokens
        && value.len() >= 2
        && value.starts_with(['\'', '"'])
        && value.as_bytes().first() == value.as_bytes().last()
    {
        return Ok(HirCicsValue::Literal(value[1..value.len() - 1].into()));
    }
    if matches!(tokens, [value] if numeric_literal(value).is_some()) {
        return Err(ResolutionFailure::Unsupported);
    }
    complete_data_reference(tokens, semantic).map(HirCicsValue::Data)
}

fn complete_data_reference(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirDataReference> {
    let (reference, end) = data_reference_at(tokens, 0, semantic)?;
    if end == tokens.len() {
        Ok(reference)
    } else {
        Err(ResolutionFailure::Unsupported)
    }
}

fn output(outputs: &[HirCicsOutputBinding], name: HirCicsOutputName) -> Option<&HirDataReference> {
    outputs
        .iter()
        .find(|output| output.name == name)
        .map(|output| &output.target)
}

pub(super) fn has_multiple_arithmetic_receivers(statement: &HirStatement) -> bool {
    let separator = match statement.kind {
        StatementKind::Add => statement
            .arguments
            .iter()
            .position(|argument| argument == "TO")
            .or_else(|| {
                statement
                    .arguments
                    .iter()
                    .position(|argument| argument == "GIVING")
            }),
        StatementKind::Subtract => statement
            .arguments
            .iter()
            .position(|argument| argument == "FROM"),
        _ => None,
    };
    let Some(separator) = separator else {
        return false;
    };
    let giving = statement
        .arguments
        .iter()
        .enumerate()
        .skip(separator + 1)
        .find_map(|(index, argument)| (argument == "GIVING").then_some(index));
    let primary_start = separator + 1;
    let primary_end = giving.unwrap_or(statement.arguments.len());
    arithmetic_receiver_count(&statement.arguments[primary_start..primary_end]) > 1
        || giving
            .is_some_and(|giving| arithmetic_receiver_count(&statement.arguments[giving + 1..]) > 1)
}

fn arithmetic_receiver_count(tokens: &[String]) -> usize {
    let mut count = 0usize;
    let mut position = 0usize;
    while position < tokens.len() {
        if matches!(tokens[position].as_str(), "," | "ROUNDED") {
            position += 1;
            continue;
        }
        count += 1;
        position += 1;
        while position < tokens.len() {
            if tokens[position] == "(" {
                let mut depth = 1usize;
                position += 1;
                while position < tokens.len() && depth > 0 {
                    match tokens[position].as_str() {
                        "(" => depth += 1,
                        ")" => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    position += 1;
                }
            } else if matches!(tokens[position].as_str(), "OF" | "IN")
                && position + 1 < tokens.len()
            {
                position += 2;
            } else {
                break;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CobolCompiler;
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };

    fn analyze(source: &str) -> crate::CobolAnalysis {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("typed.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "typed.cbl",
            source.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let bundle =
            SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
        CobolCompiler::default().analyze(&bundle)
    }

    #[test]
    fn add_and_compute_resolve_values_receivers_pairs_and_length() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. TYPED. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 1. 01 B PIC 99 VALUE 2. 01 C PIC 99 VALUE 3. 01 LEN-X PIC 9. 01 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 8. 01 SOURCE-G. 05 COUNT-X PIC 99 VALUE 2. 05 NESTED-G. 10 AMOUNT-X PIC 99 VALUE 3. 01 TARGET-G. 05 COUNT-X PIC 99 VALUE 10. 05 NESTED-G. 10 AMOUNT-X PIC 99 VALUE 20. PROCEDURE DIVISION. ADD A TO B C ROUNDED ON SIZE ERROR CONTINUE NOT ON SIZE ERROR CONTINUE END-ADD. ADD A TO B GIVING C ROUNDED. ADD COUNT-X OF SOURCE-G TO COUNT-X OF TARGET-G. ADD CORRESPONDING SOURCE-G TO TARGET-G ROUNDED. COMPUTE C ROUNDED = ( A + B ) * 2 / 3. COMPUTE LEN-X = LENGTH OF DYN-X. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis.hir.expect("typed HIR");
        let resolved = hir
            .statements
            .iter()
            .filter_map(|statement| statement.resolved.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(resolved.len(), 6);
        let HirResolvedStatement::Add(HirAddStatement {
            mode: HirAddMode::To { receivers, .. },
            ..
        }) = resolved[0]
        else {
            panic!("typed ADD TO")
        };
        assert_eq!(receivers.len(), 2);
        assert_eq!(receivers[1].rounding, HirRounding::Rounded);
        let HirResolvedStatement::Add(add) = resolved[0] else {
            unreachable!()
        };
        assert_eq!(
            add.size_error,
            HirSizeErrorPolicy {
                on_size_error: true,
                not_on_size_error: true,
                explicit_terminator: true,
            }
        );
        let HirResolvedStatement::Add(HirAddStatement {
            mode: HirAddMode::Corresponding { pairs, .. },
            ..
        }) = resolved[3]
        else {
            panic!("typed ADD CORRESPONDING")
        };
        assert_eq!(pairs.len(), 2);
        assert!(
            pairs
                .iter()
                .all(|pair| pair.receiver.rounding == HirRounding::Rounded)
        );
        let HirResolvedStatement::Compute(compute) = resolved[5] else {
            panic!("typed COMPUTE LENGTH OF")
        };
        assert!(matches!(
            compute.expression,
            HirNumericExpression::LengthOf(HirDataReference { dynamic: true, .. })
        ));
    }

    #[test]
    fn typed_arithmetic_accepts_grammar_valid_receiver_and_operand_commas() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. COMMAS. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 1. 01 B PIC 99 VALUE 2. 01 C PIC 99 VALUE 3. 01 D PIC 99 VALUE 4. PROCEDURE DIVISION. ADD A, B TO C, D. COMPUTE C, D ROUNDED = A + B. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis.hir.expect("comma-separated typed arithmetic HIR");
        let resolved = hir
            .statements
            .iter()
            .filter_map(|statement| statement.resolved.as_ref())
            .collect::<Vec<_>>();
        let HirResolvedStatement::Add(HirAddStatement {
            mode: HirAddMode::To { sources, receivers },
            ..
        }) = resolved[0]
        else {
            panic!("typed ADD")
        };
        assert_eq!(sources.len(), 2);
        assert_eq!(receivers.len(), 2);
        let HirResolvedStatement::Compute(compute) = resolved[1] else {
            panic!("typed COMPUTE")
        };
        assert_eq!(compute.receivers.len(), 2);
        assert_eq!(compute.receivers[1].rounding, HirRounding::Rounded);
    }

    #[test]
    fn add_plan_preserves_the_legacy_arithmetic_context_zero_seed() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ZEROSEED. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9(20) VALUE 1. 01 B PIC 9(20). PROCEDURE DIVISION. ADD A GIVING B. STOP RUN.";
        let hir = analyze(source).hir.expect("typed ADD HIR");
        let operation = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find(|operation| {
                operation.identity.namespace() == "cobol.hir"
                    && operation.identity.name() == "add"
                    && operation.identity.major() == 2
            })
            .expect("typed ADD operation");
        let mainframe_env_ir::Attribute::Bytes(bytes) = &operation.attributes["assignment_plan"]
        else {
            panic!("typed assignment plan")
        };
        let plan = mainframe_env_ir::decode_decimal_assignment_plan(
            bytes,
            mainframe_env_ir::DecimalPlanLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            plan.assignments[0].expression,
            mainframe_env_ir::DecimalExpression::Add {
                ref left,
                ..
            } if matches!(
                left.as_ref(),
                mainframe_env_ir::DecimalExpression::Literal {
                    coefficient: 0,
                    scale: 0,
                }
            )
        ));
    }

    #[test]
    fn usage_index_arithmetic_fails_before_an_unexecutable_plan_is_published() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INDEXED. DATA DIVISION. WORKING-STORAGE SECTION. 01 INDEX-X INDEX. PROCEDURE DIVISION. ADD 1 TO INDEX-X. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.public_message().contains("is not numeric"))
        );
    }

    #[test]
    fn hostile_compute_nesting_fails_at_the_typed_depth_boundary() {
        let nesting = MAX_TYPED_EXPRESSION_DEPTH + 1;
        let expression = format!("{}A{}", "(".repeat(nesting), ")".repeat(nesting));
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. DEEP. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. PROCEDURE DIVISION. COMPUTE A = {expression}. STOP RUN."
        );
        let analysis = analyze(&source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("COMPUTE expression is malformed")
        }));
    }

    #[test]
    fn hostile_flat_compute_chain_fails_before_building_an_overdeep_tree() {
        let expression = std::iter::repeat_n("A", MAX_TYPED_EXPRESSION_DEPTH + 1)
            .collect::<Vec<_>>()
            .join(" + ");
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. FLAT. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. PROCEDURE DIVISION. COMPUTE A = {expression}. STOP RUN."
        );
        let analysis = analyze(&source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("expression tree depth exceeds the typed limit")
        }));
    }

    #[test]
    fn oversized_add_source_list_stays_on_the_bounded_legacy_route() {
        let sources = std::iter::repeat_n("1", MAX_TYPED_EXPRESSION_DEPTH - 1)
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. DEEPADD. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. PROCEDURE DIVISION. ADD {sources} GIVING A. STOP RUN."
        );
        let hir = analyze(&source).hir.expect("bounded legacy-compatible HIR");
        let add = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .expect("ADD statement");
        assert!(add.resolved.is_none());
    }

    #[test]
    fn scalar_arithmetic_type_errors_fail_during_hir_construction() {
        for procedure in ["ADD MISSING TO A.", "ADD 1 TO TEXT-X."] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9. 01 TEXT-X PIC X. PROCEDURE DIVISION. {procedure} STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.semantic.is_some());
            assert!(analysis.hir.is_none());
            assert!(
                analysis.diagnostics[0]
                    .public_message()
                    .contains("InvalidResolvedStatement")
            );
        }
    }

    #[test]
    fn dynamic_selector_remains_explicitly_unmigrated() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. TABLED. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-G. 05 T PIC 9 OCCURS 2 TIMES. PROCEDURE DIVISION. ADD 1 TO T(1). STOP RUN.";
        let analysis = analyze(source);
        let messages = analysis
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.public_message())
            .collect::<Vec<_>>();
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("legacy-compatible HIR: {messages:?}"));
        let add = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .unwrap();
        assert!(add.resolved.is_none());
        assert_eq!(add.arguments, ["1", "TO", "T", "(", "1", ")"]);
    }

    #[test]
    fn special_register_and_intrinsic_arithmetic_keep_the_legacy_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LEGACY. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X VALUE '1'. PROCEDURE DIVISION. COMPUTE RETURN-CODE = 1. COMPUTE RETURN-CODE = FUNCTION NUMVAL(TEXT-X). STOP RUN.";
        let hir = analyze(source).hir.expect("legacy-compatible HIR");
        let computes = hir
            .statements
            .iter()
            .filter(|statement| statement.kind == StatementKind::Compute)
            .collect::<Vec<_>>();
        assert_eq!(computes.len(), 2);
        assert!(
            computes
                .iter()
                .all(|statement| statement.resolved.is_none())
        );
    }

    #[test]
    fn cics_pilot_commands_resolve_named_inputs_outputs_options_and_policy() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICST. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(4). 01 RESP-X PIC 999. 01 RESP2-X PIC 999. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS REWRITE DATASET('ACCTDAT') FROM('AA22') RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS SYNCPOINT ROLLBACK RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed CICS HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].operation, HirCicsOperation::Read);
        assert!(commands[0].options.contains(&HirCicsOption::Update));
        assert!(
            commands[0]
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Into)
        );
        assert!(matches!(
            commands[0].condition_policy,
            HirCicsConditionPolicy::Respond {
                response2: Some(_),
                ..
            }
        ));
        assert_eq!(commands[1].operation, HirCicsOperation::Rewrite);
        assert!(
            commands[1]
                .operands
                .iter()
                .any(|operand| operand.name == HirCicsOperandName::Dataset)
        );
        assert_eq!(commands[2].operation, HirCicsOperation::Syncpoint);
        assert!(commands[2].options.contains(&HirCicsOption::Rollback));
    }

    #[test]
    fn cics_numeric_lexemes_do_not_enter_the_typed_route_lossily() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSK. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(4). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(003) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("legacy-compatible CICS HIR");
        let read = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .unwrap();
        assert!(read.resolved.is_none());
        assert!(read.arguments.iter().any(|argument| argument == "003"));
    }

    #[test]
    fn typed_cics_read_requires_the_modeled_into_output() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSNOOUT. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(2) VALUE 'AA'. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') RIDFLD(KEY-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("CICS Read requires INTO")
        }));
    }
}
