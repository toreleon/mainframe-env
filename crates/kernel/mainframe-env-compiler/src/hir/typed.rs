use super::{HirProblem, HirStatement, StatementKind, StatementOption, StatementOptionKind};
use crate::{CobolLayout, CobolUsage, DataCategory, LosslessSyntax, SemanticModel, SourceSpan};
use mainframe_env_diagnostics::SourceSpan as IrSourceSpan;
use mainframe_env_ir::CicsApplicationHandlerReadiness;
use mainframe_env_source::SourceBundle;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

mod cics_file_clauses;
mod cics_resolution;
mod corresponding_reference;
use corresponding_reference::corresponding_group_reference_at;

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
    pub source_declaration: Vec<SourceSpan>,
    pub receiver_declaration: Vec<SourceSpan>,
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
    Length,
    KeyLength,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirCicsValue {
    Literal(String),
    Data(HirDataReference),
    LengthOf(HirDataReference),
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
    Length,
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
    let (source_group, source_end) = corresponding_group_reference_at(tokens, 1, semantic)?;
    if tokens.get(source_end).is_none_or(|token| token != "TO") {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING has no TO boundary".into(),
        ));
    }
    let (target_group, target_end) =
        corresponding_group_reference_at(tokens, source_end + 1, semantic)?;
    let rounded = match tokens.get(target_end..).unwrap_or_default() {
        [] => HirRounding::Truncate,
        [word] if word == "ROUNDED" => HirRounding::Rounded,
        _ => return Err(ResolutionFailure::Unsupported),
    };
    if !is_add_corresponding_group(source_group.category)
        || !is_add_corresponding_group(target_group.category)
    {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING operands must be alphanumeric or national groups".into(),
        ));
    }
    let layouts_by_name = semantic
        .layouts
        .iter()
        .map(|layout| (layout.qualified_name.as_str(), layout))
        .collect::<BTreeMap<_, _>>();
    let sources = semantic
        .layouts
        .iter()
        .filter_map(|layout| {
            corresponding_key(layout, &source_group.qualified_name, &layouts_by_name)
                .filter(|_| {
                    corresponding_item_eligible(
                        layout,
                        &source_group.qualified_name,
                        &layouts_by_name,
                    )
                })
                .map(|key| (key, layout))
        })
        .fold(
            BTreeMap::<Vec<String>, Vec<&CobolLayout>>::new(),
            |mut candidates, (key, layout)| {
                candidates.entry(key).or_default().push(layout);
                candidates
            },
        );
    let targets = semantic
        .layouts
        .iter()
        .filter_map(|layout| {
            corresponding_key(layout, &target_group.qualified_name, &layouts_by_name)
                .filter(|_| {
                    corresponding_item_eligible(
                        layout,
                        &target_group.qualified_name,
                        &layouts_by_name,
                    )
                })
                .map(|key| (key, layout))
        })
        .collect::<Vec<_>>();
    let target_counts = targets.iter().fold(
        BTreeMap::<Vec<String>, usize>::new(),
        |mut counts, (key, _)| {
            *counts.entry(key.clone()).or_default() += 1;
            counts
        },
    );
    let mut unsupported_national_pair = false;
    let pairs = targets
        .into_iter()
        .filter_map(|(key, target)| {
            let matches = sources.get(&key)?;
            (matches.len() == 1 && target_counts.get(&key) == Some(&1)).then(|| {
                if matches[0].usage == CobolUsage::National || target.usage == CobolUsage::National
                {
                    unsupported_national_pair = true;
                }
                HirCorrespondingPair {
                    source: matches[0].into(),
                    receiver: HirArithmeticReceiver {
                        target: target.into(),
                        rounding: rounded,
                    },
                    source_declaration: matches[0].source.clone(),
                    receiver_declaration: target.source.clone(),
                }
            })
        })
        .collect::<Vec<_>>();
    if unsupported_national_pair {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING numeric USAGE NATIONAL storage is not supported by the declared decimal ABI"
                .into(),
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

fn corresponding_key(
    layout: &CobolLayout,
    selected_group: &str,
    layouts: &BTreeMap<&str, &CobolLayout>,
) -> Option<Vec<String>> {
    // Store the leaf first and then its implicit qualifiers. The selected
    // sending/receiving group is deliberately the exclusive boundary, so its
    // own name and any outer hierarchy never affect correspondence.
    let mut key = vec![layout.name.clone()];
    let mut parent = layout.parent.as_deref()?;
    while parent != selected_group {
        let qualifier = layouts.get(parent)?;
        // FILLER is not a data-name and therefore cannot be an implicit
        // qualifier. Omitting it also lets the uniqueness check reject two
        // otherwise indistinguishable leaves under separate filler groups.
        if qualifier.name != "FILLER" {
            key.push(qualifier.name.clone());
        }
        parent = qualifier.parent.as_deref()?;
    }
    Some(key)
}

fn corresponding_item_eligible(
    layout: &CobolLayout,
    selected_group: &str,
    layouts: &BTreeMap<&str, &CobolLayout>,
) -> bool {
    if layout.name == "FILLER" || !is_corresponding_numeric(layout.category) {
        return false;
    }
    let mut subordinate = layout;
    loop {
        if matches!(
            subordinate.usage,
            CobolUsage::Index
                | CobolUsage::Pointer
                | CobolUsage::Pointer32
                | CobolUsage::ProcedurePointer
                | CobolUsage::FunctionPointer
                | CobolUsage::ObjectReference
        ) || subordinate.alias_of.is_some()
            || subordinate.occurs_clause
            || matches!(subordinate.category, DataCategory::Rename)
        {
            return false;
        }
        let Some(parent) = subordinate.parent.as_deref() else {
            return false;
        };
        if parent == selected_group {
            return true;
        }
        let Some(parent) = layouts.get(parent) else {
            return false;
        };
        subordinate = parent;
    }
}

const fn is_corresponding_numeric(category: DataCategory) -> bool {
    matches!(
        category,
        DataCategory::NumericDisplay
            | DataCategory::NumericEdited
            | DataCategory::NationalEdited
            | DataCategory::PackedDecimal
            | DataCategory::Binary
            | DataCategory::FloatShort
            | DataCategory::FloatLong
    )
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
    if is_numeric(reference.category) && reference.usage != CobolUsage::National {
        Ok(())
    } else if is_numeric(reference.category) {
        Err(ResolutionFailure::Invalid(format!(
            "{} uses numeric USAGE NATIONAL storage not supported by the declared decimal ABI",
            reference.qualified_name
        )))
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

const fn is_add_corresponding_group(category: DataCategory) -> bool {
    matches!(category, DataCategory::Group | DataCategory::NationalGroup)
}

fn resolve_cics(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsStatement> {
    let mut body = tokens;
    if body
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("CICS"))
    {
        body = &body[1..];
    }
    if body
        .last()
        .is_some_and(|token| token.eq_ignore_ascii_case("END-EXEC"))
    {
        body = &body[..body.len() - 1];
    }
    if cics_resolution::validated_legacy_spi_compatibility(body)?.is_some() {
        return Err(ResolutionFailure::Unsupported);
    }
    let (descriptor, clauses, raw_options) = cics_resolution::validated_command(body, semantic)?;
    match descriptor.readiness {
        CicsApplicationHandlerReadiness::TypedRuntime => {}
        CicsApplicationHandlerReadiness::LegacyCompatibility => {
            return Err(ResolutionFailure::Unsupported);
        }
        CicsApplicationHandlerReadiness::Unready => {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS application command {} is catalog-known but its handler is unready",
                descriptor.label_tokens.join(" ")
            )));
        }
    }
    let operation = match descriptor.label_tokens {
        ["READ"] => HirCicsOperation::Read,
        ["REWRITE"] => HirCicsOperation::Rewrite,
        ["SYNCPOINT"] => HirCicsOperation::Syncpoint,
        _ => return Err(ResolutionFailure::Unsupported),
    };
    let allowed_clauses: &[&str] = match operation {
        HirCicsOperation::Read => &[
            "FILE",
            "DATASET",
            "RIDFLD",
            "INTO",
            "LENGTH",
            "KEYLENGTH",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::Rewrite => &["FILE", "DATASET", "FROM", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::Syncpoint => &["RESP", "RESP2"],
    };
    let allowed_options: &[&str] = match operation {
        HirCicsOperation::Read => &["UPDATE", "NOHANDLE"],
        HirCicsOperation::Rewrite => &["NOHANDLE"],
        HirCicsOperation::Syncpoint => &["ROLLBACK", "NOHANDLE"],
    };
    let unready_clauses = clauses
        .keys()
        .filter(|name| !allowed_clauses.contains(&name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let unready_options = raw_options
        .iter()
        .filter(|name| !allowed_options.contains(&name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unready_clauses.is_empty() || !unready_options.is_empty() {
        let names = unready_clauses
            .into_iter()
            .chain(unready_options)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {} is catalog-known but typed lowering is unready for {names}",
            descriptor.label_tokens.join(" ")
        )));
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
    let (operands, length_output) =
        cics_file_clauses::cics_operands(&clauses, operation, semantic)?;
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
    if let Some(binding) = length_output {
        outputs.push(binding);
    }
    let mut options = raw_options
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
    let no_handle = options.contains(&HirCicsOption::NoHandle);
    let condition_policy = if let Some(response) = response {
        // RESP implies NOHANDLE while retaining the response-area update. The
        // typed plan carries that canonical policy as Respond, so an explicit
        // redundant NOHANDLE flag must not overwrite it.
        options.remove(&HirCicsOption::NoHandle);
        HirCicsConditionPolicy::Respond {
            response,
            response2,
        }
    } else if no_handle {
        HirCicsConditionPolicy::NoHandle
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
    fn add_corresponding_matches_the_exact_relative_qualifier_path() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRQUAL. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 MATCH-X PIC 99 VALUE 1. 05 LEFT-G. 10 AMOUNT PIC 99 VALUE 2. 01 DST-G. 05 MATCH-X PIC 99 VALUE 10. 05 RIGHT-G. 10 AMOUNT PIC 99 VALUE 20. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let hir = analyze(source).hir.expect("qualified CORRESPONDING HIR");
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .expect("ADD CORRESPONDING statement");
        let Some(HirResolvedStatement::Add(HirAddStatement {
            mode:
                HirAddMode::Corresponding {
                    source_group,
                    target_group,
                    pairs,
                },
            ..
        })) = statement.resolved.as_ref()
        else {
            panic!("typed ADD CORRESPONDING")
        };
        assert_eq!(source_group.qualified_name, "SRC-G");
        assert_eq!(target_group.qualified_name, "DST-G");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].source.qualified_name, "SRC-G.MATCH-X");
        assert_eq!(pairs[0].receiver.target.qualified_name, "DST-G.MATCH-X");
        assert!(!statement.source.is_empty());
        for (spans, declaration) in [
            (&pairs[0].source_declaration, "05 MATCH-X PIC 99 VALUE 1"),
            (&pairs[0].receiver_declaration, "05 MATCH-X PIC 99 VALUE 10"),
        ] {
            let [span] = spans.as_slice() else {
                panic!("one exact fixture declaration span")
            };
            let start = source.find(declaration).expect("fixture declaration");
            assert_eq!(span.source.as_str(), "typed.cbl");
            assert_eq!(
                span.source_start..span.source_end,
                start..start + declaration.len()
            );
            assert_eq!(&source[span.source_start..span.source_end], declaration);
        }

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
        assert_eq!(plan.assignments.len(), 1);
        assert_eq!(
            plan.assignments[0].receiver.target.qualified_layout_name,
            "DST-G.MATCH-X"
        );
        let mainframe_env_ir::DecimalExpression::Add { left, right } =
            &plan.assignments[0].expression
        else {
            panic!("corresponding addition")
        };
        assert!(matches!(
            left.as_ref(),
            mainframe_env_ir::DecimalExpression::Storage(slot)
                if slot.qualified_layout_name == "DST-G.MATCH-X"
        ));
        assert!(matches!(
            right.as_ref(),
            mainframe_env_ir::DecimalExpression::Storage(slot)
                if slot.qualified_layout_name == "SRC-G.MATCH-X"
        ));
    }

    #[test]
    fn add_corresponding_uses_qualifiers_to_make_repeated_leaves_unique() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRUNIQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 LEFT-G. 10 AMOUNT PIC 99. 05 RIGHT-G. 10 AMOUNT PIC 99. 01 DST-G. 05 LEFT-G. 10 AMOUNT PIC 99. 05 RIGHT-G. 10 AMOUNT PIC 99. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let hir = analyze(source).hir.expect("qualified duplicate-leaf HIR");
        let pairs = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Add(HirAddStatement {
                    mode: HirAddMode::Corresponding { pairs, .. },
                    ..
                })) => Some(pairs),
                _ => None,
            })
            .expect("resolved pairs");
        assert_eq!(
            pairs
                .iter()
                .map(|pair| (
                    pair.source.qualified_name.as_str(),
                    pair.receiver.target.qualified_name.as_str(),
                ))
                .collect::<Vec<_>>(),
            [
                ("SRC-G.LEFT-G.AMOUNT", "DST-G.LEFT-G.AMOUNT"),
                ("SRC-G.RIGHT-G.AMOUNT", "DST-G.RIGHT-G.AMOUNT"),
            ]
        );
    }

    #[test]
    fn add_corresponding_requires_bilateral_uniqueness_after_filler_qualification() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRAMBIG. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-DUP. 05 FILLER. 10 AMOUNT PIC 99. 05 FILLER. 10 AMOUNT PIC 99. 01 DST-ONE. 05 AMOUNT PIC 99. 01 SRC-ONE. 05 AMOUNT PIC 99. 01 DST-DUP. 05 FILLER. 10 AMOUNT PIC 99. 05 FILLER. 10 AMOUNT PIC 99. PROCEDURE DIVISION. ADD CORRESPONDING SRC-DUP TO DST-ONE. ADD CORRESPONDING SRC-ONE TO DST-DUP. STOP RUN.";
        let hir = analyze(source).hir.expect("ambiguous CORRESPONDING HIR");
        let pair_counts = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Add(HirAddStatement {
                    mode: HirAddMode::Corresponding { pairs, .. },
                    ..
                })) => Some(pairs.len()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pair_counts, [0, 0]);
    }

    #[test]
    fn add_corresponding_applies_documented_item_category_clause_and_usage_exclusions() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRELIG. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 GOOD-X PIC 99. 05 EDIT-X PIC +99. 05 PACK-X PIC S9(3) COMP-3. 05 BINARY-X PIC S9(4) COMP. 05 FLOAT-X COMP-1. 05 TABLE-X PIC 99 OCCURS 1 TIMES. 05 BASE-X PIC 99. 05 REDEF-X REDEFINES BASE-X PIC 99. 05 INDEX-X USAGE INDEX. 05 POINTER-X USAGE POINTER. 05 POINTER32-X USAGE POINTER-32. 05 PROC-X USAGE PROCEDURE-POINTER. 05 FUNC-X USAGE FUNCTION-POINTER. 05 OBJECT-X USAGE OBJECT REFERENCE. 05 FILLER PIC 99. 05 TEXT-X PIC XX. 66 RENAMED-X RENAMES GOOD-X. 01 DST-G. 05 GOOD-X PIC 99. 05 EDIT-X PIC +99. 05 PACK-X PIC S9(3) COMP-3. 05 BINARY-X PIC S9(4) COMP. 05 FLOAT-X COMP-1. 05 TABLE-X PIC 99 OCCURS 1 TIMES. 05 BASE-X PIC 99. 05 REDEF-X REDEFINES BASE-X PIC 99. 05 INDEX-X USAGE INDEX. 05 POINTER-X USAGE POINTER. 05 POINTER32-X USAGE POINTER-32. 05 PROC-X USAGE PROCEDURE-POINTER. 05 FUNC-X USAGE FUNCTION-POINTER. 05 OBJECT-X USAGE OBJECT REFERENCE. 05 FILLER PIC 99. 05 TEXT-X PIC XX. 66 RENAMED-X RENAMES GOOD-X. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let analysis = analyze(source);
        let semantic = analysis.semantic.as_ref().expect("semantic model");
        assert!(semantic.layout("SRC-G.TABLE-X").unwrap().occurs_clause);
        assert!(semantic.layout("DST-G.TABLE-X").unwrap().occurs_clause);
        let hir = analysis.hir.expect("eligible CORRESPONDING HIR");
        let pairs = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Add(HirAddStatement {
                    mode: HirAddMode::Corresponding { pairs, .. },
                    ..
                })) => Some(pairs),
                _ => None,
            })
            .expect("resolved pairs");
        assert_eq!(
            pairs
                .iter()
                .map(|pair| pair.source.qualified_name.as_str())
                .collect::<Vec<_>>(),
            [
                "SRC-G.GOOD-X",
                "SRC-G.EDIT-X",
                "SRC-G.PACK-X",
                "SRC-G.BINARY-X",
                "SRC-G.FLOAT-X",
                "SRC-G.BASE-X",
            ]
        );
        assert!(pairs.iter().all(|pair| {
            ![
                "TABLE-X",
                "REDEF-X",
                "INDEX-X",
                "POINTER-X",
                "POINTER32-X",
                "PROC-X",
                "FUNC-X",
                "OBJECT-X",
                "FILLER",
                "RENAMED-X",
            ]
            .iter()
            .any(|excluded| pair.source.qualified_name.contains(excluded))
        }));
    }

    #[test]
    fn add_corresponding_excludes_descendants_of_forbidden_subordinate_groups() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRNEST. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 GOOD-X PIC 99. 05 TABLE-G OCCURS 1 TIMES. 10 TABLE-X PIC 99. 05 BASE-G. 10 BASE-X PIC 99. 05 REDEF-G REDEFINES BASE-G. 10 REDEF-X PIC 99. 01 DST-G. 05 GOOD-X PIC 99. 05 TABLE-G OCCURS 1 TIMES. 10 TABLE-X PIC 99. 05 BASE-G. 10 BASE-X PIC 99. 05 REDEF-G REDEFINES BASE-G. 10 REDEF-X PIC 99. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let hir = analyze(source)
            .hir
            .expect("CORRESPONDING with nested exclusions");
        let pairs = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Add(HirAddStatement {
                    mode: HirAddMode::Corresponding { pairs, .. },
                    ..
                })) => Some(pairs),
                _ => None,
            })
            .expect("resolved pairs");
        assert_eq!(
            pairs
                .iter()
                .map(|pair| pair.source.qualified_name.as_str())
                .collect::<Vec<_>>(),
            ["SRC-G.GOOD-X", "SRC-G.BASE-G.BASE-X"]
        );
    }

    #[test]
    fn add_corresponding_does_not_apply_subordinate_exclusions_to_selected_groups() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRROOT. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-BASE. 05 VALUE-X PIC 99. 01 SRC-G REDEFINES SRC-BASE. 05 VALUE-X PIC 99. 01 DST-BASE. 05 VALUE-X PIC 99. 01 DST-G REDEFINES DST-BASE. 05 VALUE-X PIC 99. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let hir = analyze(source)
            .hir
            .expect("selected REDEFINES groups remain eligible");
        let Some(HirResolvedStatement::Add(HirAddStatement {
            mode:
                HirAddMode::Corresponding {
                    source_group,
                    target_group,
                    pairs,
                },
            ..
        })) = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .and_then(|statement| statement.resolved.as_ref())
        else {
            panic!("typed ADD CORRESPONDING")
        };
        assert_eq!(source_group.qualified_name, "SRC-G");
        assert_eq!(target_group.qualified_name, "DST-G");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].source.qualified_name, "SRC-G.VALUE-X");
        assert_eq!(pairs[0].receiver.target.qualified_name, "DST-G.VALUE-X");
    }

    #[test]
    fn subscripted_selected_group_uses_the_explicit_compatibility_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRSUB. DATA DIVISION. WORKING-STORAGE SECTION. 01 IDX PIC 9 VALUE 2. 01 SRC-ROOT. 05 SRC-G. 10 VALUE-X PIC 99 VALUE 1. 01 DST-ROOT. 05 DST-G OCCURS 2 TIMES. 10 VALUE-X PIC 99 VALUE 10. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G(IDX). STOP RUN.";
        let hir = analyze(source).hir.expect("subscript compatibility HIR");
        let add = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .expect("ADD CORRESPONDING statement");
        assert!(add.resolved.is_none());
        assert!(add.arguments.iter().any(|argument| argument == "IDX"));
    }

    #[test]
    fn table_group_without_required_subscript_is_rejected() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRNOSUB. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-ROOT. 05 SRC-G. 10 VALUE-X PIC 99 VALUE 1. 01 DST-ROOT. 05 DST-G OCCURS 2 TIMES. 10 VALUE-X PIC 99 VALUE 10. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("table group operand requires subscripting")
        }));
    }

    #[test]
    fn reference_modified_selected_group_fails_closed() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRREFM. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 VALUE-X PIC 99 VALUE 1. 01 DST-G. 05 VALUE-X PIC 99 VALUE 10. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G(1:2) TO DST-G. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("cannot be reference modified")
        }));
    }

    #[test]
    fn unsupported_national_numeric_pairs_are_not_silently_misencoded_or_ignored() {
        for pictures in [("99", "999"), ("+99", "+999")] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CORRNATL. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G GROUP-USAGE NATIONAL. 05 AMOUNT PIC {}. 01 DST-G GROUP-USAGE NATIONAL. 05 AMOUNT PIC {}. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.",
                pictures.0, pictures.1
            );
            let analysis = analyze(&source);
            assert!(analysis.semantic.is_some());
            assert!(analysis.hir.is_none());
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .public_message()
                    .contains("numeric USAGE NATIONAL storage is not supported")
            }));
        }
    }

    #[test]
    fn utf8_groups_are_outside_the_current_typed_add_corresponding_slice() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRUTF8. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G GROUP-USAGE UTF-8. 05 TEXT-X PIC U. 01 DST-G GROUP-USAGE UTF-8. 05 TEXT-X PIC U. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.semantic.is_some());
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("must be alphanumeric or national groups")
        }));
    }

    #[test]
    fn valid_add_corresponding_without_pairs_is_an_explicit_typed_noop() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORRNONE. DATA DIVISION. WORKING-STORAGE SECTION. 01 SRC-G. 05 SOURCE-ONLY PIC 99. 01 DST-G. 05 TARGET-ONLY PIC 99. PROCEDURE DIVISION. ADD CORRESPONDING SRC-G TO DST-G. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.diagnostics.is_empty());
        let hir = analysis.hir.expect("typed no-pair HIR");
        let add = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::Add)
            .expect("ADD statement");
        assert!(matches!(
            add.resolved.as_ref(),
            Some(HirResolvedStatement::Add(HirAddStatement {
                mode: HirAddMode::Corresponding { pairs, .. },
                ..
            })) if pairs.is_empty()
        ));
        assert!(
            hir.module
                .regions()
                .iter()
                .flat_map(|region| &region.blocks)
                .flat_map(|block| &block.operations)
                .any(|operation| {
                    operation.identity.namespace() == "cobol.hir"
                        && operation.identity.name() == "add"
                        && operation.identity.major() == 2
                })
        );
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
    fn numeric_national_typed_slots_fail_at_hir_while_opaque_length_remains_valid() {
        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. NATADD. DATA DIVISION. WORKING-STORAGE SECTION. 01 NATIONAL-X PIC 99 NATIONAL. PROCEDURE DIVISION. ADD 1 TO NATIONAL-X. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. NATRESP. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESPONSE-X PIC 9(4) NATIONAL. PROCEDURE DIVISION. EXEC CICS SYNCPOINT RESP(RESPONSE-X) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.semantic.is_some());
            assert!(analysis.hir.is_none());
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .public_message()
                    .contains("numeric USAGE NATIONAL storage not supported")
            }));
        }

        let length = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. NATLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 NATIONAL-X PIC 99 NATIONAL. 01 LENGTH-X PIC 9(4). PROCEDURE DIVISION. COMPUTE LENGTH-X = LENGTH OF NATIONAL-X. STOP RUN.",
        );
        assert!(length.hir.is_some());
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
    fn cics_file_lengths_lower_as_typed_values_and_read_output() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(8). 01 KEY-X PIC X(3). 01 LEN-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) LENGTH(LEN-X) KEYLENGTH(LENGTH OF KEY-X) END-EXEC. EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) LENGTH(LENGTH OF REC-X) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed CICS length HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::Data(_))
        }));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(operand.value, HirCicsValue::LengthOf(_))
        }));
        assert!(
            commands[0]
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Length)
        );
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::LengthOf(_))
        }));
        assert!(
            !commands[1]
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Length)
        );
    }

    #[test]
    fn cics_file_length_data_items_require_halfword_binary_storage() {
        for command in [
            "READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) LENGTH(TEXT-X)",
            "READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) KEYLENGTH(FULL-X)",
            "REWRITE FILE('ACCTDAT') FROM(REC-X) LENGTH(FULL-X)",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLBAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(8). 01 KEY-X PIC X(3). 01 TEXT-X PIC X(2). 01 FULL-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .public_message()
                    .contains("is not a halfword binary data item")
            }));
        }
    }

    #[test]
    fn cics_resp_binding_wins_over_an_explicit_nohandle_flag() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSRESP. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(4). 01 KEY-X PIC X(3) VALUE '003'. 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) NOHANDLE RESP(RESP-X) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed CICS RESP policy HIR");
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved READ command");
        assert!(matches!(
            command.condition_policy,
            HirCicsConditionPolicy::Respond {
                response2: None,
                ..
            }
        ));
        assert!(!command.options.contains(&HirCicsOption::NoHandle));
    }

    #[test]
    fn cics_registry_prefers_the_longest_catalog_label() {
        let bare = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBASE. PROCEDURE DIVISION. EXEC CICS ASKTIME END-EXEC. STOP RUN.",
        );
        assert!(bare.hir.is_none());
        assert!(bare.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("ASKTIME") && message.contains("handler is unready")
        }));

        let longer = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLONG. DATA DIVISION. WORKING-STORAGE SECTION. 01 TIME-X PIC X(8). PROCEDURE DIVISION. EXEC CICS ASKTIME ABSTIME(TIME-X) END-EXEC. STOP RUN.",
        );
        let hir = longer
            .hir
            .unwrap_or_else(|| panic!("ASKTIME ABSTIME: {:?}", longer.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(statement.resolved.is_none());
    }

    #[test]
    fn cics_shared_heads_resolve_with_valued_discriminators() {
        for (command, expected_label) in [
            ("ACQUIRE ACTIVITYID('A1')", "ACQUIRE ACTIVITYID"),
            (
                "ACQUIRE PROCESS('P1') PROCESSTYPE('PTYPE')",
                "ACQUIRE PROCESS",
            ),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDISC. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    let message = diagnostic.public_message();
                    message.contains(expected_label) && message.contains("handler is unready")
                }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn cics_true_identity_qualifiers_remain_required() {
        for (label, qualifier) in [
            (&["ASKTIME", "ABSTIME"][..], "ABSTIME"),
            (&["DELETE", "CHANNEL"][..], "CHANNEL"),
            (&["ENTER", "TRACENUM"][..], "TRACENUM"),
            (&["EXTRACT", "CERTIFICATE"][..], "CERTIFICATE"),
            (&["QUERY", "CHANNEL"][..], "CHANNEL"),
            (&["REQUEST", "PASSTICKET"][..], "PASSTICKET"),
            (&["SET", "ASSOCIATION", "USERCORRDATA"][..], "USERCORRDATA"),
            (&["VERIFY", "PASSWORD"][..], "PASSWORD"),
        ] {
            let descriptor = mainframe_env_ir::CICS_APPLICATION_REGISTRY
                .iter()
                .find(|descriptor| descriptor.label_tokens == label)
                .unwrap_or_else(|| panic!("missing {} descriptor", label.join(" ")));
            assert!(
                descriptor
                    .required_discriminator_options
                    .contains(&qualifier),
                "{} must require {qualifier}",
                label.join(" ")
            );
        }

        let qualified = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSPASS. PROCEDURE DIVISION. EXEC CICS REQUEST PASSTICKET(PT-X) ESMAPPNAME('APP') END-EXEC. STOP RUN.",
        );
        assert!(qualified.hir.is_none());
        assert!(qualified.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("REQUEST PASSTICKET") && message.contains("handler is unready")
        }));

        let omitted = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSPASS. PROCEDURE DIVISION. EXEC CICS REQUEST ESMAPPNAME('APP') END-EXEC. STOP RUN.",
        );
        assert!(omitted.hir.is_none());
        assert!(omitted.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("missing a required command discriminator")
        }));
    }

    #[test]
    fn cics_gds_wait_alias_is_recognized_but_rejected_for_cobol() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGDS. PROCEDURE DIVISION. EXEC CICS GDS WAIT END-EXEC. STOP RUN.",
        );
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("CICS WAIT is not applicable to COBOL")
        }));
    }

    #[test]
    fn cics_non_cobol_application_forms_fail_closed() {
        for command in ["CICSMESSAGE", "GETMAIN64"] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSCOB. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .public_message()
                        .contains("not applicable to COBOL")
                }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn compiler_cics_registry_exposes_a_candidate_head_for_all_263_rows() {
        assert_eq!(mainframe_env_ir::CICS_APPLICATION_REGISTRY.len(), 263);
        for descriptor in mainframe_env_ir::CICS_APPLICATION_REGISTRY {
            assert!(
                descriptor.recognition_heads.iter().any(|head| {
                    mainframe_env_ir::cics_application_registry_candidates_for_tokens(head).any(
                        |candidate| candidate.descriptor.official_row == descriptor.official_row,
                    )
                }),
                "{}",
                descriptor.label_tokens.join(" ")
            );
        }
    }

    #[test]
    fn cics_application_route_rejects_spi_fepi_and_unknown_labels() {
        for (command, expected) in [
            (
                "INQUIRE FILE('ACCTDAT')",
                "unknown or unreviewed top-level option FILE",
            ),
            (
                "FEPI ALLOCATE POOL('POOL')",
                "unknown CICS application command",
            ),
            ("FROBULATE THING('X')", "unknown CICS application command"),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSISO. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.public_message().contains(expected) }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn cics_registry_rejects_unknown_duplicate_and_malformed_top_level_forms() {
        let cases = [
            (
                "RETURN TRANSID('NEXT') BOGUS('X')",
                "unknown or unreviewed top-level option BOGUS",
            ),
            (
                "RETURN TRANSID('NEXT') TRANSID('OTHER')",
                "top-level option TRANSID is duplicated",
            ),
            ("RETURN TRANSID()", "operand clause is empty"),
            ("RETURN TRANSID('NEXT'", "clause parentheses are malformed"),
        ];
        for (command, expected) in cases {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSNEG. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.public_message().contains(expected)),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn cics_registry_enforces_option_shapes_before_readiness() {
        let cases = [
            ("RETURN TRANSID", "TRANSID requires a parenthesized operand"),
            (
                "RETURN IMMEDIATE('YES')",
                "IMMEDIATE is a flag and rejects a parenthesized operand",
            ),
            ("SEND MAP", "MAP requires a parenthesized operand"),
            ("WRITE FILE", "FILE requires a parenthesized operand"),
            ("ABEND ABCODE", "ABCODE requires a parenthesized operand"),
        ];
        for (command, expected) in cases {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSHAPE. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.public_message().contains(expected) }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    /// Regression coverage for `toreleon/mainframe-env` issue #170: pinned
    /// syntax diagrams (dfhp4_sendmap.html CURSOR(data-value), and
    /// dfhp4_formattime.html DATESEP(data-value)/TIMESEP(data-value)) draw
    /// the parenthesized operand as an independently optional nested group,
    /// so both the bare keyword and the keyword with its operand must
    /// compile. `f8d44ec`/`f1fe39e` regressed this to always requiring the
    /// operand.
    #[test]
    fn cics_registry_accepts_bare_and_valued_optional_operand_options() {
        for command in [
            "SEND MAP('MENU') MAPSET('MAIN') CURSOR",
            "SEND MAP('MENU') MAPSET('MAIN') CURSOR(5)",
            "FORMATTIME ABSTIME(ABS-TIME-X) DATESEP TIMESEP",
            "FORMATTIME ABSTIME(ABS-TIME-X) DATESEP('-') TIMESEP('.')",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSOPT. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(
                analysis.hir.is_some(),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }

        // A bare Value-shape option (its parenthesized operand is fused into
        // the keyword in the pinned diagram, so it is not independently
        // optional) must still be rejected exactly as before.
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSOPTN. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') MAPSET END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("MAPSET requires a parenthesized operand")
        }));
    }

    /// `toreleon/mainframe-env` issue #171: `COPAUS2C.cbl` repeats
    /// `NOHANDLE` on one `ASKTIME` command. The pinned IBM sources cached for
    /// this contract (dfhp4_apiformat.html "Common options for all EXEC CICS
    /// commands", dfhp4_asktime.html) describe NOHANDLE's effect but do not
    /// state whether repeating it is legal, so duplicate detection is left
    /// exactly as strict as before pending that decision (see task
    /// needs_decision). This test freezes that current, unchanged behavior
    /// as a regression guard: it must keep failing, not silently start
    /// passing, until the source question above is resolved.
    #[test]
    fn cics_registry_still_rejects_a_repeated_nohandle_pending_source_review() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUPN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS ASKTIME NOHANDLE ABSTIME(ABS-TIME-X) NOHANDLE END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("option NOHANDLE is duplicated")
        }));
    }

    #[test]
    fn cics_registry_enforces_known_dependencies_and_groups() {
        let cases = [
            ("RETURN RESP2(RESP-X)", "option RESP2 requires RESP"),
            (
                "ACQUIRE PROCESS('P1')",
                "ACQUIRE PROCESS requires option PROCESSTYPE",
            ),
            (
                "ACQUIRE ACTIVITYID('A1') PROCESSTYPE('PTYPE')",
                "ACQUIRE ACTIVITYID has unknown or unreviewed top-level option PROCESSTYPE",
            ),
        ];
        for (command, expected) in cases {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSCON. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.public_message().contains(expected) }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn cics_registry_rejects_used_bounded_option_shapes() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSAMB. PROCEDURE DIVISION. EXEC CICS GET CONTAINER('C1') CONVERTST END-EXEC. STOP RUN.",
        );
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("CONVERTST has a source-bounded operand shape")
        }));
    }

    #[test]
    fn catalog_known_unready_cics_command_fails_before_legacy_lowering() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSWAIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X PIC X(8). PROCEDURE DIVISION. EXEC CICS ADDRESS SET(PTR-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("ADDRESS SET") && message.contains("handler is unready")
        }));
    }

    #[test]
    fn legacy_cics_compatibility_routes_are_explicit_and_not_typed() {
        let legacy = mainframe_env_ir::CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::LegacyCompatibility
            })
            .collect::<Vec<_>>();
        assert_eq!(legacy.len(), 20);
        assert!(
            legacy.iter().all(|descriptor| {
                descriptor.advertised && descriptor.runtime_operation.is_some()
            })
        );
        assert!(legacy.iter().all(|descriptor| {
            !matches!(
                descriptor.label_tokens,
                ["READ"] | ["REWRITE"] | ["SYNCPOINT"]
            )
        }));
    }

    #[test]
    fn legacy_spi_compatibility_is_exactly_inquire_program() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSPI. DATA DIVISION. WORKING-STORAGE SECTION. 01 IDX PIC 9 VALUE 1. 01 NAMES. 05 PGM-NAME PIC X(4) OCCURS 2. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS INQUIRE PROGRAM(PGM-NAME(IDX)) NOHANDLE RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("INQUIRE PROGRAM: {:?}", analysis.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(statement.resolved.is_none());

        for command in [
            "INQUIRE NOHANDLE PROGRAM('P001')",
            "INQUIRE RESP(RESP-X) PROGRAM('P001')",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSPIO. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            let hir = analysis
                .hir
                .unwrap_or_else(|| panic!("{command}: {:?}", analysis.diagnostics));
            let statement = hir
                .statements
                .iter()
                .find(|statement| statement.kind == StatementKind::ExecCics)
                .expect("EXEC CICS statement");
            assert!(statement.resolved.is_none(), "{command}");
        }

        for (command, expected) in [
            ("INQUIRE PROGRAM", "requires a parenthesized operand"),
            ("INQUIRE PROGRAM()", "operand clause is empty"),
            (
                "INQUIRE PROGRAM('P001') PROGRAM('P002')",
                "top-level option PROGRAM is duplicated",
            ),
            (
                "INQUIRE PROGRAM('P001') UNKNOWN",
                "unknown legacy SPI option UNKNOWN",
            ),
            (
                "INQUIRE PROGRAM('P001') RESP2(RESP2-X)",
                "option RESP2 requires RESP",
            ),
            (
                "INQUIRE PROGRAM('P001') NOHANDLE('X')",
                "option NOHANDLE is a flag",
            ),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSPIE. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.public_message().contains(expected)),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }

        for command in ["INQUIRE", "INQUIRE FILE('ACCTDAT')", "SET FILE('ACCTDAT')"] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSPIN. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            assert!(analyze(&source).hir.is_none(), "{command}");
        }

        let reordered_application = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBTSA. DATA DIVISION. WORKING-STORAGE SECTION. 01 PGM-OUT PIC X(8). PROCEDURE DIVISION. EXEC CICS INQUIRE PROGRAM(PGM-OUT) ACTIVITYID('A1') END-EXEC. STOP RUN.",
        );
        assert!(reordered_application.hir.is_none());
        assert!(reordered_application.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("INQUIRE ACTIVITYID") && message.contains("handler is unready")
        }));
    }

    #[test]
    fn existing_legacy_cics_label_tail_clauses_are_not_consumed_as_command_words() {
        for command in [
            "SEND MAP('MENU') MAPSET('MAIN')",
            "RECEIVE MAP('MENU') MAPSET('MAIN')",
            "WRITE FILE('ACCTDAT') FROM('AA11')",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTAIL. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            let hir = analysis
                .hir
                .unwrap_or_else(|| panic!("{command}: {:?}", analysis.diagnostics));
            let statement = hir
                .statements
                .iter()
                .find(|statement| statement.kind == StatementKind::ExecCics)
                .expect("EXEC CICS statement");
            assert!(statement.resolved.is_none(), "{command}");
        }
    }

    #[test]
    fn cics_optional_and_alternate_forms_do_not_become_false_discriminators() {
        let handle = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSHAND. PROCEDURE DIVISION. EXEC CICS HANDLE ABEND PROGRAM('P') END-EXEC. STOP RUN.",
        );
        let hir = handle
            .hir
            .unwrap_or_else(|| panic!("HANDLE ABEND: {:?}", handle.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(statement.resolved.is_none());

        for (command, expected_label) in [
            ("ISSUE ERASEAUP", "ISSUE ERASEAUP"),
            ("SEND PAGE RETAIN", "SEND PAGE"),
            ("TRACE OFF", "TRACE"),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSALT. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    let message = diagnostic.public_message();
                    message.contains(expected_label) && message.contains("handler is unready")
                }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn cics_dynamic_condition_clauses_validate_names_shapes_and_bounds() {
        let handle = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSCOND. PROCEDURE DIVISION. EXEC CICS HANDLE CONDITION ERROR(ERR-HANDLER) LENGERR END-EXEC. STOP RUN.",
        );
        let hir = handle
            .hir
            .unwrap_or_else(|| panic!("HANDLE CONDITION: {:?}", handle.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(statement.resolved.is_none());

        let ignore = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSIGN. PROCEDURE DIVISION. EXEC CICS IGNORE CONDITION ERROR END-EXEC. STOP RUN.",
        );
        assert!(ignore.hir.is_none());
        assert!(ignore.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("IGNORE CONDITION") && message.contains("handler is unready")
        }));

        for (command, expected) in [
            (
                "HANDLE CONDITION MADEUP(ERR-HANDLER)",
                "unknown or unreviewed top-level option MADEUP",
            ),
            (
                "IGNORE CONDITION ERROR(ERR-HANDLER)",
                "condition ERROR forbids a label operand",
            ),
            (
                "HANDLE CONDITION",
                "requires 1..=16 EIBRESP condition clauses",
            ),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSCBAD. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.public_message().contains(expected)),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }

        let seventeen = mainframe_env_ir::CICS_APPLICATION_CONDITION_NAMES[..17].join(" ");
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICS17. PROCEDURE DIVISION. EXEC CICS HANDLE CONDITION {seventeen} END-EXEC. STOP RUN."
        );
        let analysis = analyze(&source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("requires 1..=16 EIBRESP condition clauses, found 17")
        }));
    }

    #[test]
    fn typed_cics_registry_options_never_fall_back_to_lossy_raw_lowering() {
        for (command, option) in [
            ("READ FILE('ACCTDAT') RIDFLD(KEY-X) SET(PTR-X)", "SET"),
            (
                "READ FILE('ACCTDAT') RIDFLD(KEY-X) INTO(REC-X) NOSUSPEND",
                "NOSUSPEND",
            ),
            ("REWRITE FILE('ACCTDAT') FROM(REC-X) NOSUSPEND", "NOSUSPEND"),
            (
                "READ FILE('ACCTDAT') RIDFLD(KEY-X) INTO(REC-X) KEYLENGTH(LENGTH OF KEY-X) GENERIC",
                "GENERIC",
            ),
            (
                "READ FILE('ACCTDAT') RIDFLD(KEY-X) INTO(REC-X) GTEQ",
                "GTEQ",
            ),
            (
                "READ FILE('ACCTDAT') RIDFLD(KEY-X) INTO(REC-X) EQUAL",
                "EQUAL",
            ),
            (
                "READ FILE('ACCTDAT') RIDFLD(KEY-X) INTO(REC-X) SYSID('REM1')",
                "SYSID",
            ),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFORM. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(2). 01 REC-X PIC X(8). 01 PTR-X PIC X(8). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    let message = diagnostic.public_message();
                    message.contains("typed lowering is unready") && message.contains(option)
                }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }
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
