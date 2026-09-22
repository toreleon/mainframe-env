use super::{HirProblem, HirStatement, StatementKind, StatementOption, StatementOptionKind};
use crate::{CobolLayout, CobolUsage, DataCategory, LosslessSyntax, SemanticModel, SourceSpan};
use mainframe_env_diagnostics::SourceSpan as IrSourceSpan;
use mainframe_env_ir::CicsAssignOutput;
use mainframe_env_source::SourceBundle;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

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
    Abend,
    AddressSet,
    Asktime,
    AsktimeEib,
    FormatTime,
    Freemain,
    Getmain,
    Cancel,
    Delay,
    ChangeTask,
    Deq,
    Enq,
    HandleAid,
    HandleAbend,
    HandleCondition,
    IgnoreCondition,
    Link,
    Xctl,
    Return,
    StartBrowse,
    ReadNext,
    ReadPrev,
    EndBrowse,
    Delete,
    Write,
    WriteTransientData,
    DeleteTransientData,
    ReceiveMap,
    SendMap,
    SendText,
    Assign,
    PurgeMessage,
    PopHandle,
    PushHandle,
    Read,
    Rewrite,
    SetAssociationUserCorrData,
    Syncpoint,
    Suspend,
    Start,
    Retrieve,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOperandName {
    Abcode,
    Label,
    Program,
    Commarea,
    TransId,
    TermId,
    ReturnTransId,
    ReturnTermId,
    File,
    Dataset,
    From,
    Ridfld,
    Queue,
    Map,
    Mapset,
    Resource,
    Length,
    MaxLifetime,
    Priority,
    UserCorrData,
    SetAddress,
    SetPointer,
    UsingAddress,
    UsingPointer,
    /// Canonical condition specifications for HANDLE or IGNORE.
    Conditions,
    /// Canonical terminal AID handler specifications.
    Aids,
    Abstime,
    DateSep,
    TimeSep,
    KeyLength,
    ReqId,
    Interval,
    StartTime,
    UserId,
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
    DataLength,
    Flength,
    InitImage,
    DataPointer,
    DataArea,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirCicsValue {
    Literal(String),
    Data(HirDataReference),
    Integer(i64),
    LengthOf(HirDataReference),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirCicsNamedOperand {
    pub name: HirCicsOperandName,
    pub value: HirCicsValue,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOption {
    Cancel,
    NoDump,
    Reset,
    Update,
    Rollback,
    NoHandle,
    Task,
    Uow,
    NoSuspend,
    Erase,
    Cursor,
    DateSep,
    TimeSep,
    FreeKb,
    Gteq,
    Fmh,
    Protect,
    Wait,
    After,
    At,
    For,
    Until,
    NoCheck,
    MapOnly,
    DataOnly,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirCicsOutputName {
    Abstime,
    Commarea,
    Into,
    SetPointer,
    Ridfld,
    Milliseconds,
    Mmddyy,
    Mmddyyyy,
    Resp,
    Resp2,
    Time,
    Yyddd,
    Yymmdd,
    Yyyymmdd,
    Assign(CicsAssignOutput),
    Length,
    ReturnTransId,
    ReturnTermId,
    Queue,
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
    cics_resolution::resolve(tokens, semantic)
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
    use mainframe_env_ir::CicsApplicationHandlerReadiness;
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

    fn analyze_fixed(source: &str) -> crate::CobolAnalysis {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("typed.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "typed.cbl",
            source.as_bytes().to_vec(),
            SourceFormat::Fixed,
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
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICST. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(4). 01 REWRITE-X PIC X(4) VALUE 'AA22'. 01 RESP-X PIC 999. 01 RESP2-X PIC 999. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS REWRITE DATASET('ACCTDAT') FROM(REWRITE-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS SYNCPOINT ROLLBACK RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
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
    fn cics_enqueue_commands_resolve_resource_length_cvda_and_flags() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSENQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 LOCK-NAME PIC X(9) VALUE 'EMPLOYEE1'. 01 LOCK-LENGTH PIC S9(4) COMP VALUE 9. 01 LIFE PIC S9(9) COMP VALUE 246. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ENQ RESOURCE(LOCK-NAME) LENGTH(LOCK-LENGTH) MAXLIFETIME(LIFE) NOSUSPEND RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS DEQ RESOURCE(LOCK-NAME) LENGTH(9) UOW END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed enqueue HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].operation, HirCicsOperation::Enq);
        assert!(commands[0].options.contains(&HirCicsOption::NoSuspend));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::MaxLifetime
                && matches!(operand.value, HirCicsValue::Data(_))
        }));
        assert_eq!(commands[1].operation, HirCicsOperation::Deq);
        assert!(commands[1].options.contains(&HirCicsOption::Uow));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length && operand.value == HirCicsValue::Integer(9)
        }));
    }

    #[test]
    fn cics_task_scheduling_resolves_priority_and_one_shot_suspend() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSSCHED. DATA DIVISION. WORKING-STORAGE SECTION. 01 PRIORITY-X PIC S9(4) COMP VALUE 200. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS CHANGE TASK PRIORITY(PRIORITY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS CHANGE TASK PRIORITY(-1) END-EXEC. EXEC CICS SUSPEND NOHANDLE END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed task scheduling HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].operation, HirCicsOperation::ChangeTask);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Priority
                && matches!(operand.value, HirCicsValue::Data(_))
        }));
        assert!(matches!(
            commands[0].condition_policy,
            HirCicsConditionPolicy::Respond {
                response2: Some(_),
                ..
            }
        ));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Priority
                && operand.value == HirCicsValue::Integer(-1)
        }));
        assert_eq!(commands[2].operation, HirCicsOperation::Suspend);
        assert_eq!(
            commands[2].condition_policy,
            HirCicsConditionPolicy::NoHandle
        );
    }

    #[test]
    fn cics_task_association_resolves_one_typed_correlator_input() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSASSOC. DATA DIVISION. WORKING-STORAGE SECTION. 01 CORR-X PIC X(80) VALUE ALL 'A'. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS SET ASSOCIATION USERCORRDATA(CORR-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed task association HIR");
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("task association command");
        assert_eq!(
            command.operation,
            HirCicsOperation::SetAssociationUserCorrData
        );
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::UserCorrData
                && matches!(operand.value, HirCicsValue::Data(_))
        }));
        assert!(matches!(
            command.condition_policy,
            HirCicsConditionPolicy::Respond {
                response2: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn cics_address_set_resolves_both_virtual_pointer_directions() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSADDR. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 PTR-X POINTER. LINKAGE SECTION. 01 LINK-X PIC X(8). PROCEDURE DIVISION USING LINK-X. EXEC CICS ADDRESS SET(PTR-X) USING(ADDRESS OF DATA-X) END-EXEC. EXEC CICS ADDRESS SET(ADDRESS OF LINK-X) USING(PTR-X) NOHANDLE END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed ADDRESS SET HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        assert!(commands.iter().all(|command| {
            command.operation == HirCicsOperation::AddressSet && command.operands.len() == 2
        }));
        assert_eq!(
            commands[0]
                .operands
                .iter()
                .map(|operand| operand.name)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                HirCicsOperandName::SetPointer,
                HirCicsOperandName::UsingAddress,
            ])
        );
        assert_eq!(
            commands[1]
                .operands
                .iter()
                .map(|operand| operand.name)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                HirCicsOperandName::SetAddress,
                HirCicsOperandName::UsingPointer,
            ])
        );
        assert_eq!(
            commands[1].condition_policy,
            HirCicsConditionPolicy::NoHandle
        );

        let invalid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADADDR. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-A POINTER. 01 PTR-B POINTER. PROCEDURE DIVISION. EXEC CICS ADDRESS SET(PTR-A) USING(PTR-B) END-EXEC. STOP RUN.",
        );
        assert!(invalid.hir.is_none());
        assert!(invalid.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("requires one pointer reference and one ADDRESS OF data area")
        }));
        let wrong_category = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADCAT. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X(4). 01 DATA-X PIC X(4). PROCEDURE DIVISION. EXEC CICS ADDRESS SET(TEXT-X) USING(ADDRESS OF DATA-X) END-EXEC. STOP RUN.",
        );
        assert!(wrong_category.hir.is_none());
        assert!(wrong_category.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("pointer operand must use POINTER or POINTER-32")
        }));
    }

    #[test]
    fn cics_handle_stack_commands_resolve_without_unowned_operands() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSHSTK. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS PUSH HANDLE NOHANDLE END-EXEC. EXEC CICS POP HANDLE RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let hir = analyze(source).hir.expect("typed HANDLE stack HIR");
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].operation, HirCicsOperation::PushHandle);
        assert_eq!(commands[1].operation, HirCicsOperation::PopHandle);
        assert!(commands.iter().all(|command| command.operands.is_empty()));
        assert_eq!(
            commands[0].condition_policy,
            HirCicsConditionPolicy::NoHandle
        );
        assert!(matches!(
            commands[1].condition_policy,
            HirCicsConditionPolicy::Respond {
                response2: Some(_),
                ..
            }
        ));

        let invalid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADHSTK. PROCEDURE DIVISION. EXEC CICS PUSH HANDLE RESET END-EXEC. STOP RUN.",
        );
        assert!(invalid.hir.is_none());
        assert!(invalid.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("PUSH HANDLE") && message.contains("RESET")
        }));
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
        let bare_hir = bare
            .hir
            .unwrap_or_else(|| panic!("ASKTIME: {:?}", bare.diagnostics));
        let statement = bare_hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("bare ASKTIME statement");
        assert!(matches!(
            statement.resolved,
            Some(HirResolvedStatement::Cics(HirCicsStatement {
                operation: HirCicsOperation::AsktimeEib,
                ..
            }))
        ));

        let longer = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLONG. DATA DIVISION. WORKING-STORAGE SECTION. 01 TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS ASKTIME ABSTIME(TIME-X) END-EXEC. STOP RUN.",
        );
        let hir = longer
            .hir
            .unwrap_or_else(|| panic!("ASKTIME ABSTIME: {:?}", longer.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(matches!(
            statement.resolved,
            Some(HirResolvedStatement::Cics(HirCicsStatement {
                operation: HirCicsOperation::Asktime,
                ref outputs,
                ..
            })) if outputs.iter().any(|output| {
                output.name == HirCicsOutputName::Abstime
                    && output.target.qualified_name == "TIME-X"
            })
        ));

        let wrong_shape = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 TIME-X PIC S9(14) COMP-3. PROCEDURE DIVISION. EXEC CICS ASKTIME ABSTIME(TIME-X) END-EXEC. STOP RUN.",
        );
        assert!(wrong_shape.hir.is_none());
        assert!(
            wrong_shape
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.public_message().contains("PIC S9(15) COMP-3"))
        );
    }

    #[test]
    fn cics_formattime_resolves_only_the_source_checked_output_subset() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFMT. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3. 01 DATE-X PIC X(10). 01 TIME-X PIC X(8). 01 MS-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS FORMATTIME ABSTIME(ABS-X) DATESEP('-') YYYYMMDD(DATE-X) TIMESEP(':') TIME(TIME-X) MILLISECONDS(MS-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("FORMATTIME: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved FORMATTIME command");
        assert_eq!(command.operation, HirCicsOperation::FormatTime);
        assert_eq!(
            command
                .operands
                .iter()
                .map(|operand| operand.name)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                HirCicsOperandName::Abstime,
                HirCicsOperandName::DateSep,
                HirCicsOperandName::TimeSep,
            ])
        );

        let compact = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFMTC. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3. 01 DATE-X PIC X(6). 01 TIME-X PIC X(6). PROCEDURE DIVISION. EXEC CICS FORMATTIME ABSTIME(ABS-X) YYMMDD(DATE-X) TIME(TIME-X) END-EXEC. STOP RUN.",
        );
        assert!(compact.hir.is_none());
        assert!(compact.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("FORMATTIME output has an invalid field length")
        }));
        assert_eq!(
            command
                .outputs
                .iter()
                .map(|output| output.name)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                HirCicsOutputName::Milliseconds,
                HirCicsOutputName::Time,
                HirCicsOutputName::Yyyymmdd,
            ])
        );

        let short_binary = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADFMT. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3. 01 MS-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS FORMATTIME ABSTIME(ABS-X) MILLISECONDS(MS-X) END-EXEC. STOP RUN.",
        );
        assert!(short_binary.hir.is_none());
        assert!(
            short_binary
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.public_message().contains("fullword binary"))
        );

        let deferred = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. LATERFMT. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3. 01 DAY-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS FORMATTIME ABSTIME(ABS-X) DAYCOUNT(DAY-X) END-EXEC. STOP RUN.",
        );
        assert!(deferred.hir.is_none());
        assert!(deferred.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("FORMATTIME") && message.contains("DAYCOUNT")
        }));
    }

    /// Issue #207: bare DATESEP and TIMESEP select the documented defaults.
    #[test]
    fn cics_formattime_bare_separators_lower_as_default_options() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. FMTSEP. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3. 01 DATE-X PIC X(8). 01 TIME-X PIC X(8). PROCEDURE DIVISION. EXEC CICS FORMATTIME ABSTIME(ABS-X) MMDDYY(DATE-X) DATESEP TIME(TIME-X) TIMESEP NOHANDLE END-EXEC.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("bare separators: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed FORMATTIME");
        assert_eq!(command.operation, HirCicsOperation::FormatTime);
        assert!(command.options.contains(&HirCicsOption::DateSep));
        assert!(command.options.contains(&HirCicsOption::TimeSep));
        assert!(matches!(
            command.condition_policy,
            HirCicsConditionPolicy::NoHandle
        ));
    }

    #[test]
    fn cics_abend_resolves_code_and_dump_control_without_source_text() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSABND. DATA DIVISION. WORKING-STORAGE SECTION. 01 AB-CODE PIC X(4) VALUE 'B001'. PROCEDURE DIVISION. EXEC CICS ABEND ABCODE(AB-CODE) CANCEL NODUMP END-EXEC.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("ABEND: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved ABEND command");
        assert_eq!(command.operation, HirCicsOperation::Abend);
        assert!(matches!(
            command.operands.as_slice(),
            [HirCicsNamedOperand {
                name: HirCicsOperandName::Abcode,
                value: HirCicsValue::Data(reference),
            }] if reference.qualified_name == "AB-CODE"
        ));
        assert_eq!(
            command.options,
            BTreeSet::from([HirCicsOption::Cancel, HirCicsOption::NoDump])
        );

        let wrong_shape = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADABND. DATA DIVISION. WORKING-STORAGE SECTION. 01 AB-CODE PIC 9(4). PROCEDURE DIVISION. EXEC CICS ABEND ABCODE(AB-CODE) END-EXEC.",
        );
        assert!(wrong_shape.hir.is_none());
        assert!(wrong_shape.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("ABCODE requires a 1-4 character value")
        }));
    }

    #[test]
    fn cics_handle_abend_resolves_program_storage_and_rejects_numeric_names() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSHAB. DATA DIVISION. WORKING-STORAGE SECTION. 01 PROGRAM-X PIC X(8) VALUE 'ABEXIT'. PROCEDURE DIVISION. EXEC CICS HANDLE ABEND PROGRAM(PROGRAM-X) END-EXEC.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("HANDLE ABEND: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved HANDLE ABEND command");
        assert_eq!(command.operation, HirCicsOperation::HandleAbend);
        assert!(matches!(
            command.operands.as_slice(),
            [HirCicsNamedOperand {
                name: HirCicsOperandName::Program,
                value: HirCicsValue::Data(reference),
            }] if reference.qualified_name == "PROGRAM-X"
        ));

        let wrong_shape = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADHAB. DATA DIVISION. WORKING-STORAGE SECTION. 01 PROGRAM-X PIC 9(8). PROCEDURE DIVISION. EXEC CICS HANDLE ABEND PROGRAM(PROGRAM-X) END-EXEC.",
        );
        assert!(wrong_shape.hir.is_none());
        assert!(wrong_shape.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("PROGRAM requires a 1-8 character name")
        }));
    }

    #[test]
    fn cics_link_resolves_program_and_one_input_output_commarea() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLINK. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(16) VALUE 'REQUEST'. PROCEDURE DIVISION. EXEC CICS LINK PROGRAM('CHILD') COMMAREA(AREA-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("LINK: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved LINK command");
        assert_eq!(command.operation, HirCicsOperation::Link);
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Program,
                    value: HirCicsValue::Literal(value),
                } if value == "CHILD"
            )
        }));
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Commarea,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "AREA-X"
            )
        }));
        assert!(command.outputs.iter().any(|output| {
            output.name == HirCicsOutputName::Commarea && output.target.qualified_name == "AREA-X"
        }));

        let deferred = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. LATERLNK. PROCEDURE DIVISION. EXEC CICS LINK PROGRAM('CHILD') CHANNEL('DATA') END-EXEC. STOP RUN.",
        );
        assert!(deferred.hir.is_none());
        assert!(deferred.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("LINK") && message.contains("CHANNEL")
        }));
    }

    #[test]
    fn cics_xctl_resolves_program_and_one_input_only_commarea() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSXCTL. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(16) VALUE 'REQUEST'. PROCEDURE DIVISION. EXEC CICS XCTL PROGRAM('CHILD') COMMAREA(AREA-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("XCTL: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved XCTL command");
        assert_eq!(command.operation, HirCicsOperation::Xctl);
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Program,
                    value: HirCicsValue::Literal(value),
                } if value == "CHILD"
            )
        }));
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Commarea,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "AREA-X"
            )
        }));
        assert!(
            !command
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Commarea)
        );

        for clause in ["CHANNEL('DATA')", "INPUTMSG(AREA-X)"] {
            let deferred = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. LATERXCT. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(16). PROCEDURE DIVISION. EXEC CICS XCTL PROGRAM('CHILD') {clause} END-EXEC. STOP RUN."
            ));
            assert!(deferred.hir.is_none());
            assert!(deferred.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains("XCTL") && message.contains(clause.split('(').next().unwrap())
            }));
        }
    }

    #[test]
    fn cics_return_resolves_bounded_transaction_and_copied_commarea() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSRET. DATA DIVISION. WORKING-STORAGE SECTION. 01 TRANS-X PIC X(4) VALUE 'NEXT'. 01 AREA-X PIC X(8) VALUE 'STATE'. 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS RETURN TRANSID(TRANS-X) COMMAREA(AREA-X) RESP(RESP-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("RETURN: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("resolved RETURN command");
        assert_eq!(command.operation, HirCicsOperation::Return);
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::TransId,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "TRANS-X"
            )
        }));
        assert!(command.operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Commarea,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "AREA-X"
            )
        }));
        assert!(
            !command
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Commarea)
        );
        assert!(matches!(
            command.condition_policy,
            HirCicsConditionPolicy::Respond { .. }
        ));

        let bare = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BARERET. PROCEDURE DIVISION. EXEC CICS RETURN END-EXEC.",
        );
        assert!(bare.hir.is_some(), "{:?}", bare.diagnostics);
        let missing_transaction = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRET. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS RETURN COMMAREA(AREA-X) END-EXEC.",
        );
        assert!(missing_transaction.hir.is_none());
        assert!(missing_transaction.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("COMMAREA requires TRANSID")
        }));
        let oversized = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRET. DATA DIVISION. WORKING-STORAGE SECTION. 01 TRANS-X PIC X(5). PROCEDURE DIVISION. EXEC CICS RETURN TRANSID(TRANS-X) END-EXEC.",
        );
        assert!(oversized.hir.is_none());
        assert!(oversized.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("TRANSID requires a 1-4 character name")
        }));
        for option in [
            "CHANNEL('DATA')",
            "INPUTMSG(AREA-X)",
            "IMMEDIATE",
            "ENDACTIVITY",
        ] {
            let deferred = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. LATERRET. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS RETURN TRANSID('NEXT') {option} END-EXEC."
            ));
            assert!(deferred.hir.is_none());
            assert!(deferred.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains("RETURN") && message.contains(option.split('(').next().unwrap())
            }));
        }
    }

    /// Issue #202: RETURN accepts LENGTH(LENGTH OF) for its COMMAREA.
    #[test]
    fn cics_return_length_of_commarea_resolves_typed_length() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. RETLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS RETURN TRANSID('NEXT') COMMAREA(AREA-X) LENGTH(LENGTH OF AREA-X) END-EXEC.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("RETURN LENGTH OF: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed RETURN");
        let commarea = command
            .operands
            .iter()
            .find(|operand| operand.name == HirCicsOperandName::Commarea)
            .expect("COMMAREA");
        let length = command
            .operands
            .iter()
            .find(|operand| operand.name == HirCicsOperandName::Length)
            .expect("LENGTH");
        assert!(matches!(
            (&commarea.value, &length.value),
            (HirCicsValue::Data(area), HirCicsValue::LengthOf(length)) if area == length
        ));
    }

    #[test]
    fn cics_program_transfers_resolve_literal_dynamic_and_length_of_commareas() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. XLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(8). 01 LENGTH-X PIC S9(4) COMP VALUE 4. PROCEDURE DIVISION. EXEC CICS LINK PROGRAM('CHILD') COMMAREA(AREA-X) LENGTH(3) DATALENGTH(2) END-EXEC. EXEC CICS XCTL PROGRAM('CHILD') COMMAREA(AREA-X) LENGTH(LENGTH-X) END-EXEC. EXEC CICS RETURN TRANSID('NEXT') COMMAREA(AREA-X) LENGTH(LENGTH OF AREA-X) END-EXEC.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("program LENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length && operand.value == HirCicsValue::Integer(3)
        }));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::DataLength
                && operand.value == HirCicsValue::Integer(2)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Length,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "LENGTH-X"
            )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Length,
                    value: HirCicsValue::LengthOf(reference),
                } if reference.qualified_name == "AREA-X"
            )
        }));

        for command in ["LINK PROGRAM('CHILD')", "XCTL PROGRAM('CHILD')"] {
            let invalid = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADLEN. PROCEDURE DIVISION. EXEC CICS {command} LENGTH(1) END-EXEC."
            ));
            assert!(invalid.hir.is_none(), "{command}");
        }
        for clauses in [
            "PROGRAM('CHILD') DATALENGTH(1)",
            "PROGRAM('CHILD') COMMAREA(AREA-X) DATALENGTH(1)",
        ] {
            let invalid = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADDATA. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS LINK {clauses} END-EXEC."
            ));
            assert!(invalid.hir.is_none(), "{clauses}");
        }
    }

    #[test]
    fn cics_default_file_browse_resolves_shared_key_input_output_roles() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBROW. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(2) VALUE 'AA'. 01 RECORD-X PIC X(4). PROCEDURE DIVISION. EXEC CICS STARTBR FILE('ACCTDAT') RIDFLD(KEY-X) END-EXEC. EXEC CICS READNEXT FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS READPREV DATASET('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS ENDBR FILE('ACCTDAT') END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("browse: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.operation)
                .collect::<Vec<_>>(),
            [
                HirCicsOperation::StartBrowse,
                HirCicsOperation::ReadNext,
                HirCicsOperation::ReadPrev,
                HirCicsOperation::EndBrowse,
            ]
        );
        for command in &commands[..3] {
            let input = command
                .operands
                .iter()
                .find(|operand| operand.name == HirCicsOperandName::Ridfld)
                .expect("browse RIDFLD input");
            assert!(matches!(
                input.value,
                HirCicsValue::Data(ref reference) if reference.qualified_name == "KEY-X"
            ));
        }
        for command in &commands[1..3] {
            assert!(command.outputs.iter().any(|output| {
                output.name == HirCicsOutputName::Into && output.target.qualified_name == "RECORD-X"
            }));
            assert!(command.outputs.iter().any(|output| {
                output.name == HirCicsOutputName::Ridfld && output.target.qualified_name == "KEY-X"
            }));
        }
        assert!(commands[0].outputs.is_empty());
        assert!(commands[3].outputs.is_empty());

        for (command, expected) in [
            ("STARTBR FILE('ACCTDAT')", "requires RIDFLD"),
            (
                "STARTBR FILE('ACCTDAT') RIDFLD('AA')",
                "RIDFLD requires a data area",
            ),
            ("READNEXT FILE('ACCTDAT') RIDFLD(KEY-X)", "requires INTO"),
            (
                "READPREV FILE('ACCTDAT') DATASET('OTHER') INTO(RECORD-X) RIDFLD(KEY-X)",
                "mutually exclusive",
            ),
            (
                "READNEXT FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) UPDATE",
                "unready for UPDATE",
            ),
            ("ENDBR FILE('ACCTDAT') REQID(KEY-X)", "unready for REQID"),
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADBROW. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(2). 01 RECORD-X PIC X(4). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC."
            ));
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

    /// Issue #204: GTEQ is scoped to typed keyed selection routes.
    #[test]
    fn cics_startbr_gteq_is_typed_and_operation_scoped() {
        let valid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BRGTEQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). PROCEDURE DIVISION. EXEC CICS STARTBR FILE('ACCTDAT') RIDFLD(KEY-X) GTEQ END-EXEC.",
        );
        let hir = valid
            .hir
            .unwrap_or_else(|| panic!("STARTBR GTEQ: {:?}", valid.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed STARTBR");
        assert_eq!(command.operation, HirCicsOperation::StartBrowse);
        assert_eq!(command.options, BTreeSet::from([HirCicsOption::Gteq]));

        let read = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. RDGTEQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 REC-X PIC X(8). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) GTEQ END-EXEC.",
        );
        let hir = read
            .hir
            .unwrap_or_else(|| panic!("READ GTEQ: {:?}", read.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed READ");
        assert_eq!(command.operation, HirCicsOperation::Read);
        assert_eq!(command.options, BTreeSet::from([HirCicsOption::Gteq]));

        let invalid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. WRGTEQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 REC-X PIC X(8). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(REC-X) RIDFLD(KEY-X) GTEQ END-EXEC.",
        );
        assert!(invalid.hir.is_none());
    }

    /// Issue #205: DELETE may select the record held by READ UPDATE.
    #[test]
    fn cics_keyed_file_mutations_require_resolved_record_and_key_inputs() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSMUT. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '003'. 01 RECORD-X PIC X(4) VALUE 'DATA'. PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS DELETE DATASET('ACCTDAT') RIDFLD(KEY-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("file mutations: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.operation)
                .collect::<Vec<_>>(),
            [HirCicsOperation::Write, HirCicsOperation::Delete]
        );
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::From
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference)
                        if reference.qualified_name == "RECORD-X"
                )
        }));
        for command in &commands {
            assert!(command.operands.iter().any(|operand| {
                operand.name == HirCicsOperandName::Ridfld
                    && matches!(
                        operand.value,
                        HirCicsValue::Data(ref reference)
                            if reference.qualified_name == "KEY-X"
                    )
            }));
            assert!(command.outputs.is_empty());
        }

        let current_record_delete = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CURDEL. PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') END-EXEC.",
        );
        let hir = current_record_delete.hir.unwrap_or_else(|| {
            panic!(
                "current-record DELETE: {:?}",
                current_record_delete.diagnostics
            )
        });
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed current-record DELETE");
        assert_eq!(command.operation, HirCicsOperation::Delete);
        assert!(
            !command
                .operands
                .iter()
                .any(|operand| operand.name == HirCicsOperandName::Ridfld)
        );

        for (command, expected) in [
            ("WRITE FILE('ACCTDAT') FROM(RECORD-X)", "requires RIDFLD"),
            ("WRITE FILE('ACCTDAT') RIDFLD(KEY-X)", "requires FROM"),
            (
                "DELETE FILE('ACCTDAT') RIDFLD(KEY-X) TOKEN(KEY-X)",
                "unready for TOKEN",
            ),
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADMUT. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(4). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC."
            ));
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

        for command in [
            "DELETE FILE('ACCTDAT') RIDFLD('003')",
            "WRITE FILE('ACCTDAT') FROM('DATA') RIDFLD(KEY-X)",
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADMUT. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC."
            ));
            assert!(analysis.hir.is_none(), "{command}");
        }
    }

    #[test]
    fn cics_write_file_resolves_bounded_record_lengths() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. WRITELEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '003'. 01 RECORD-X PIC X(8) VALUE '003ABCDE'. 01 LENGTH-X PIC S9(4) COMP VALUE 5. PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(5) END-EXEC. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) END-EXEC. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH OF RECORD-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("WRITE FILE LENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command))
                    if command.operation == HirCicsOperation::Write =>
                {
                    Some(command)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length && operand.value == HirCicsValue::Integer(5)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "LENGTH-X"
                )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(
                    operand.value,
                    HirCicsValue::LengthOf(ref reference)
                        if reference.qualified_name == "RECORD-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(32768) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 LENGTH-X PIC S9(8) COMP. PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 OTHER-X PIC X(4). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH OF OTHER-X) END-EXEC. STOP RUN.",
        ] {
            assert!(analyze(source).hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_rewrite_file_resolves_bounded_record_lengths() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. REWRITELEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(8) VALUE '003ABCDE'. 01 LENGTH-X PIC S9(4) COMP VALUE 5. PROCEDURE DIVISION. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(5) END-EXEC. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(LENGTH-X) END-EXEC. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(LENGTH OF RECORD-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("REWRITE FILE LENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command))
                    if command.operation == HirCicsOperation::Rewrite =>
                {
                    Some(command)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length && operand.value == HirCicsValue::Integer(5)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "LENGTH-X"
                )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(
                    operand.value,
                    HirCicsValue::LengthOf(ref reference)
                        if reference.qualified_name == "RECORD-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(8). PROCEDURE DIVISION. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(32768) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(8). 01 LENGTH-X PIC S9(8) COMP. PROCEDURE DIVISION. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(8). 01 OTHER-X PIC X(4). PROCEDURE DIVISION. EXEC CICS REWRITE FILE('ACCTDAT') FROM(RECORD-X) LENGTH(LENGTH OF OTHER-X) END-EXEC. STOP RUN.",
        ] {
            assert!(analyze(source).hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_write_file_resolves_bounded_key_lengths() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. WRITEKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '003'. 01 RECORD-X PIC X(8) VALUE '003ABCDE'. 01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3. PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(3) END-EXEC. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(LENGTH OF KEY-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("WRITE FILE KEYLENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command))
                    if command.operation == HirCicsOperation::Write =>
                {
                    Some(command)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && operand.value == HirCicsValue::Integer(3)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference)
                        if reference.qualified_name == "KEY-LENGTH-X"
                )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::LengthOf(ref reference)
                        if reference.qualified_name == "KEY-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(0) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 KEY-LENGTH-X PIC S9(8) COMP. PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADWKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 OTHER-X PIC X(2). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(LENGTH OF OTHER-X) END-EXEC. STOP RUN.",
        ] {
            assert!(analyze(source).hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_delete_file_resolves_bounded_key_lengths() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. DELETEKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '003'. 01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3. PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(3) END-EXEC. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(LENGTH OF KEY-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("DELETE FILE KEYLENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command))
                    if command.operation == HirCicsOperation::Delete =>
                {
                    Some(command)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && operand.value == HirCicsValue::Integer(3)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference)
                        if reference.qualified_name == "KEY-LENGTH-X"
                )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::LengthOf(ref reference)
                        if reference.qualified_name == "KEY-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADDKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(0) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADDKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 KEY-LENGTH-X PIC S9(8) COMP. PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADDKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 OTHER-X PIC X(2). PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(LENGTH OF OTHER-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADDKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-LENGTH-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS DELETE FILE('ACCTDAT') KEYLENGTH(KEY-LENGTH-X) END-EXEC. STOP RUN.",
        ] {
            assert!(analyze(source).hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_read_file_resolves_bounded_key_lengths() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. READKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '003'. 01 RECORD-X PIC X(8). 01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(3) END-EXEC. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(LENGTH OF KEY-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("READ FILE KEYLENGTH: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command))
                    if command.operation == HirCicsOperation::Read =>
                {
                    Some(command)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && operand.value == HirCicsValue::Integer(3)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference)
                        if reference.qualified_name == "KEY-LENGTH-X"
                )
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::KeyLength
                && matches!(
                    operand.value,
                    HirCicsValue::LengthOf(ref reference)
                        if reference.qualified_name == "KEY-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(0) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 KEY-LENGTH-X PIC S9(8) COMP. PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADRKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(8). 01 OTHER-X PIC X(2). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(LENGTH OF OTHER-X) END-EXEC. STOP RUN.",
        ] {
            assert!(analyze(source).hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_read_gteq_is_typed_and_operation_scoped() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. READGTEQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3) VALUE '004'. 01 RECORD-X PIC X(7). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) GTEQ END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("READ GTEQ: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed READ GTEQ");
        assert_eq!(command.operation, HirCicsOperation::Read);
        assert!(command.options.contains(&HirCicsOption::Gteq));

        let wrong_operation = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADGTEQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RECORD-X PIC X(7). PROCEDURE DIVISION. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) GTEQ END-EXEC. STOP RUN.",
        );
        assert!(wrong_operation.hir.is_none());
    }

    /// Issue #206: WRITEQ TD accepts the runtime length of its FROM area.
    #[test]
    fn cics_transient_data_write_resolves_queue_record_and_optional_length() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTDQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(6) VALUE 'ABCDEF'. PROCEDURE DIVISION. EXEC CICS WRITEQ TD QUEUE('OUTQ') FROM(DATA-X) LENGTH(3) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("WRITEQ TD: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed WRITEQ TD");
        assert_eq!(command.operation, HirCicsOperation::WriteTransientData);
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Queue
                && operand.value == HirCicsValue::Literal("OUTQ".into())
        }));
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::From
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "DATA-X"
                )
        }));
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length && operand.value == HirCicsValue::Integer(3)
        }));

        let deleted = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTDQD. PROCEDURE DIVISION. EXEC CICS DELETEQ TD QUEUE('OUTQ') END-EXEC. STOP RUN.",
        );
        let deleted = deleted
            .hir
            .unwrap_or_else(|| panic!("DELETEQ TD: {:?}", deleted.diagnostics));
        let deleted = deleted
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed DELETEQ TD");
        assert_eq!(deleted.operation, HirCicsOperation::DeleteTransientData);
        assert!(deleted.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Queue
                && operand.value == HirCicsValue::Literal("OUTQ".into())
        }));

        let length_of = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTDQL. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(6). PROCEDURE DIVISION. EXEC CICS WRITEQ TD QUEUE('OUTQ') FROM(DATA-X) LENGTH(LENGTH OF DATA-X) END-EXEC. STOP RUN.",
        );
        let hir = length_of
            .hir
            .unwrap_or_else(|| panic!("WRITEQ TD LENGTH OF: {:?}", length_of.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed WRITEQ TD LENGTH OF");
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::LengthOf(_))
        }));

        for (command, expected) in [
            ("WRITEQ TD FROM(DATA-X)", "requires QUEUE"),
            ("WRITEQ TD QUEUE('OUTQ')", "requires FROM"),
            (
                "WRITEQ TD QUEUE('TOOLONG') FROM(DATA-X)",
                "QUEUE requires a 1-4 character name",
            ),
            (
                "WRITEQ TD QUEUE('OUTQ') FROM('ABC')",
                "FROM requires a data area",
            ),
            (
                "WRITEQ TD QUEUE('OUTQ') FROM(DATA-X) SYSID('R1')",
                "unready for SYSID",
            ),
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADTDQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(6). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC."
            ));
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
        let remote = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTDQR. PROCEDURE DIVISION. EXEC CICS DELETEQ TD QUEUE('OUTQ') SYSID('R001') END-EXEC. STOP RUN.",
        );
        assert!(remote.hir.is_none());
        assert!(remote.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("typed lowering is unready for SYSID")
        }));
    }

    #[test]
    fn cics_getmain_resolves_bounded_flength_and_length_storage() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC S9(9) COMP VALUE 4. 01 INIT-X PIC X VALUE 'Z'. 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(LEN-X) INITIMG(INIT-X) NOSUSPEND RESP(RESP-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("GETMAIN: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed GETMAIN");
        assert_eq!(command.operation, HirCicsOperation::Getmain);
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Flength
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "LEN-X"
                )
        }));
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::InitImage
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "INIT-X"
                )
        }));
        assert!(command.outputs.iter().any(|output| {
            output.name == HirCicsOutputName::SetPointer && output.target.qualified_name == "PTR-X"
        }));
        assert!(command.options.contains(&HirCicsOption::NoSuspend));

        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC 9(4) COMP VALUE 4. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) LENGTH(LEN-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("GETMAIN LENGTH: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed GETMAIN LENGTH");
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "LEN-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(LEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X PIC X(4). PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(4) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(2147483648) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) LENGTH(LEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC 9(9) COMP. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) LENGTH(LEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) LENGTH(-1) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) LENGTH(65521) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSGET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(4) LENGTH(4) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_freemain_resolves_exactly_one_pointer_or_data_input() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS FREEMAIN DATAPOINTER(PTR-X) RESP(RESP-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("FREEMAIN: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed FREEMAIN");
        assert_eq!(command.operation, HirCicsOperation::Freemain);
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::DataPointer
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "PTR-X"
                )
        }));

        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. LINKAGE SECTION. 01 LINK-X PIC X(4). PROCEDURE DIVISION. EXEC CICS FREEMAIN DATA(LINK-X) RESP(RESP-X) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("FREEMAIN DATA: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed FREEMAIN DATA");
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::DataArea
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "LINK-X"
                )
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X PIC X(8). PROCEDURE DIVISION. EXEC CICS FREEMAIN DATAPOINTER(PTR-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS FREEMAIN END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 DATA-X PIC X(4). PROCEDURE DIVISION. EXEC CICS FREEMAIN DATA(DATA-X) DATAPOINTER(PTR-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSFREE. PROCEDURE DIVISION. EXEC CICS FREEMAIN DATA('ABCD') END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_bms_subset_resolves_explicit_maps_and_storage_areas() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBMS. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUT-X PIC X(4) VALUE 'DATA'. 01 IN-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') MAPSET('MAIN') FROM(OUT-X) END-EXEC. EXEC CICS RECEIVE MAP('MENU') MAPSET('MAIN') INTO(IN-X) END-EXEC. EXEC CICS SEND TEXT FROM(OUT-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("BMS: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.operation)
                .collect::<Vec<_>>(),
            [
                HirCicsOperation::SendMap,
                HirCicsOperation::ReceiveMap,
                HirCicsOperation::SendText,
            ]
        );
        for command in &commands[..2] {
            assert!(command.operands.iter().any(|operand| {
                operand.name == HirCicsOperandName::Map
                    && operand.value == HirCicsValue::Literal("MENU".into())
            }));
            assert!(command.operands.iter().any(|operand| {
                operand.name == HirCicsOperandName::Mapset
                    && operand.value == HirCicsValue::Literal("MAIN".into())
            }));
        }
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::From
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "OUT-X"
                )
        }));
        assert!(commands[1].outputs.iter().any(|output| {
            output.name == HirCicsOutputName::Into && output.target.qualified_name == "IN-X"
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::From
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "OUT-X"
                )
        }));

        let defaults = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMSDEF. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') END-EXEC. EXEC CICS RECEIVE MAP('MENU') END-EXEC.",
        );
        assert!(defaults.hir.is_some(), "{:?}", defaults.diagnostics);

        for (command, expected) in [
            (
                "SEND MAPSET('MAIN')",
                "missing a required command discriminator",
            ),
            (
                "RECEIVE MAPSET('MAIN')",
                "missing a required command discriminator",
            ),
            ("SEND TEXT", "requires FROM"),
            (
                "SEND MAP('TOOLONG8') MAPSET('MAIN')",
                "MAP requires a 1-7 character name",
            ),
            ("SEND TEXT FROM('DATA')", "FROM requires a data area"),
            ("RECEIVE MAP('MENU') SET(PTR-X)", "unready for SET"),
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADBMS. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC."
            ));
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

    /// Issues #203 and #206: CardDemo SEND display controls and bounded lengths stay typed.
    #[test]
    fn cics_send_map_admits_erase_cursor_and_freekb() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. SENDOPT. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 LENGTH-X PIC S9(4) COMP VALUE 4. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') FROM(DATA-X) LENGTH(LENGTH-X) ERASE CURSOR FREEKB END-EXEC. EXEC CICS SEND TEXT FROM(DATA-X) LENGTH(LENGTH OF DATA-X) ERASE FREEKB END-EXEC. EXEC CICS SEND TEXT FROM(DATA-X) LENGTH(LENGTH-X) END-EXEC.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("SEND MAP options: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands[0].options,
            BTreeSet::from([
                HirCicsOption::Erase,
                HirCicsOption::Cursor,
                HirCicsOption::FreeKb,
            ])
        );
        assert_eq!(
            commands[1].options,
            BTreeSet::from([HirCicsOption::Erase, HirCicsOption::FreeKb])
        );
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::Data(ref reference) if reference.qualified_name == "LENGTH-X")
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::LengthOf(ref reference) if reference.qualified_name == "DATA-X")
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Length
                && matches!(operand.value, HirCicsValue::Data(ref reference) if reference.qualified_name == "LENGTH-X")
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADSEND. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') LENGTH(4) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADSEND. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') FROM(DATA-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADSEND. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') FROM(DATA-X) LENGTH(32768) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADSEND. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS SEND TEXT FROM(DATA-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADSEND. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND TEXT FROM(DATA-X) LENGTH(32768) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none(), "{source}");
        }
    }

    #[test]
    fn cics_send_map_maponly_rejects_application_data() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. SENDDEF. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') MAPSET('MAIN') MAPONLY END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("SEND MAP MAPONLY: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed SEND MAP MAPONLY");
        assert_eq!(command.options, BTreeSet::from([HirCicsOption::MapOnly]));
        assert!(command.operands.iter().all(|operand| !matches!(
            operand.name,
            HirCicsOperandName::From | HirCicsOperandName::Length
        )));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADMAPO. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') FROM(DATA-X) MAPONLY END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADMAPO. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') FROM(DATA-X) LENGTH(4) MAPONLY END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none(), "{source}");
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .public_message()
                    .contains("MAPONLY does not accept FROM or LENGTH")
            }));
        }
    }

    #[test]
    fn cics_send_map_dataonly_requires_application_data() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. SENDDATA. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(2). PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') MAPSET('MAIN') FROM(DATA-X) LENGTH(LENGTH OF DATA-X) DATAONLY END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("SEND MAP DATAONLY: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed SEND MAP DATAONLY");
        assert!(command.options.contains(&HirCicsOption::DataOnly));
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::From
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference) if reference.qualified_name == "DATA-X"
                )
        }));

        for (source, expected) in [
            (
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADDATA. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') DATAONLY END-EXEC. STOP RUN.",
                "DATAONLY requires an explicit FROM data area",
            ),
            (
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADDATA. PROCEDURE DIVISION. EXEC CICS SEND MAP('MENU') DATAONLY MAPONLY END-EXEC. STOP RUN.",
                "DATAONLY and MAPONLY are mutually exclusive",
            ),
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none(), "{source}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.public_message().contains(expected) })
            );
        }
    }

    /// Issue #208: an eight-byte MAPSET data area is resolved at run time.
    #[test]
    fn cics_receive_map_accepts_eight_byte_mapset_data_area() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. RECVMSET. DATA DIVISION. WORKING-STORAGE SECTION. 01 MAPSET-X PIC X(8). 01 INPUT-X PIC X(16). PROCEDURE DIVISION. EXEC CICS RECEIVE MAP('MENU') MAPSET(MAPSET-X) INTO(INPUT-X) END-EXEC.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("RECEIVE MAPSET area: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed RECEIVE MAP");
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Mapset
                && matches!(
                    operand.value,
                    HirCicsValue::Data(ref reference)
                        if reference.qualified_name == "MAPSET-X" && reference.length == 8
                )
        }));
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

    #[test]
    fn cics_purge_message_accepts_only_common_condition_controls() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSPURG. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. \
             01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. \
             EXEC CICS PURGE MESSAGE NOHANDLE RESP(RESP-X) RESP2(RESP2-X) END-EXEC. \
             STOP RUN.",
        );
        assert!(analysis.hir.is_some(), "{:?}", analysis.diagnostics);

        let invalid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSPURB. PROCEDURE DIVISION. \
             EXEC CICS PURGE MESSAGE ERASE END-EXEC. STOP RUN.",
        );
        assert!(invalid.hir.is_none());
        assert!(invalid.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("unknown or unreviewed top-level option ERASE")
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
            (
                "ENQ RESOURCE('LOCK') UOW TASK",
                "options TASK, UOW are mutually exclusive",
            ),
            ("DEQ UOW", "DEQ requires option RESOURCE"),
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
    fn cics_start_and_retrieve_lower_the_bounded_local_data_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INTERVAL. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(16) VALUE 'PAYLOAD'. 01 LENGTH-X PIC S9(4) COMP VALUE 7. 01 WHEN-X PIC S9(6) COMP-3 VALUE 0. 01 USER-X PIC X(8) VALUE 'TARGET'. 01 RTRANS-X PIC X(4). 01 RTERM-X PIC X(4). 01 QUEUE-X PIC X(8). 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') REQID('REQ0001') FROM(DATA-X) LENGTH(LENGTH-X) INTERVAL(WHEN-X) RTRANSID('BACK') RTERMID('T001') QUEUE('WORKQ') USERID(USER-X) FMH PROTECT RESP(RESP-X) RESP2(RESP2-X) END-EXEC. EXEC CICS RETRIEVE INTO(DATA-X) LENGTH(LENGTH-X) RTRANSID(RTRANS-X) RTERMID(RTERM-X) QUEUE(QUEUE-X) WAIT RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("START/RETRIEVE: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].operation, HirCicsOperation::Start);
        assert_eq!(commands[1].operation, HirCicsOperation::Retrieve);
        assert!(commands[0].options.contains(&HirCicsOption::Fmh));
        assert!(commands[0].options.contains(&HirCicsOption::Protect));
        assert!(commands[1].options.contains(&HirCicsOption::Wait));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::ReqId
                && operand.value == HirCicsValue::Literal("REQ0001".into())
        }));
        for name in [
            HirCicsOperandName::ReturnTransId,
            HirCicsOperandName::ReturnTermId,
            HirCicsOperandName::Queue,
            HirCicsOperandName::UserId,
        ] {
            assert!(
                commands[0]
                    .operands
                    .iter()
                    .any(|operand| operand.name == name)
            );
        }
        assert!(commands[0].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::UserId,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "USER-X"
            )
        }));
        assert!(commands[0].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Interval,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "WHEN-X"
            )
        }));
        assert!(
            commands[1]
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Into)
        );
        assert!(
            commands[1]
                .outputs
                .iter()
                .any(|output| output.name == HirCicsOutputName::Length)
        );
        for name in [
            HirCicsOutputName::ReturnTransId,
            HirCicsOutputName::ReturnTermId,
            HirCicsOutputName::Queue,
        ] {
            assert!(commands[1].outputs.iter().any(|output| output.name == name));
        }

        let terminal = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. LATER. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') REQID('REQ0002') FROM(DATA-X) TERMID('T001') END-EXEC. STOP RUN.",
        )
        .hir
        .expect("START TERMID must lower");
        assert!(terminal.statements.iter().any(|statement| matches!(
            statement.resolved.as_ref(),
            Some(HirResolvedStatement::Cics(HirCicsStatement { operands, .. }))
                if operands.iter().any(|operand| operand.name == HirCicsOperandName::TermId)
        )));
    }

    #[test]
    fn cics_start_after_and_at_preserve_literal_and_dynamic_unit_forms() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. STARTUNIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8) VALUE 'PAYLOAD'. 01 TIME-X PIC S9(9) COMP VALUE 1. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') REQID('AFTER001') FROM(DATA-X) AFTER HOURS(1) SECONDS(3) END-EXEC. EXEC CICS START TRANSID('NEXT') REQID('AT000001') FROM(DATA-X) AT MINUTES(62) END-EXEC. EXEC CICS START TRANSID('NEXT') REQID('DYN00001') FROM(DATA-X) AFTER MINUTES(TIME-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("START units: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].options.contains(&HirCicsOption::After));
        assert!(commands[1].options.contains(&HirCicsOption::At));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Hours && operand.value == HirCicsValue::Integer(1)
        }));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Seconds && operand.value == HirCicsValue::Integer(3)
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Minutes
                && operand.value == HirCicsValue::Integer(62)
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Minutes,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "TIME-X"
            )
        }));

        for invalid in [
            "AFTER",
            "HOURS(1)",
            "AFTER AT HOURS(1)",
            "INTERVAL(1) AFTER HOURS(1)",
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADUNIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 TIME-X PIC S9(9) COMP VALUE 1. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') FROM(DATA-X) {invalid} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{invalid}");
        }
        let out_of_range = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. RANGEERR. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') FROM(DATA-X) AFTER HOURS(100) END-EXEC. STOP RUN.",
        );
        assert!(
            out_of_range.hir.is_some(),
            "out-of-range values are runtime conditions"
        );
    }

    #[test]
    fn cics_retrieve_set_requires_a_pointer_and_has_output_only_length() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RETSET. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LENGTH-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS RETRIEVE SET(PTR-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("RETRIEVE SET: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed RETRIEVE SET");
        assert_eq!(command.operation, HirCicsOperation::Retrieve);
        assert!(command.operands.is_empty());
        assert!(command.outputs.iter().any(|output| {
            output.name == HirCicsOutputName::SetPointer && output.target.qualified_name == "PTR-X"
        }));
        assert!(command.outputs.iter().any(|output| {
            output.name == HirCicsOutputName::Length && output.target.qualified_name == "LENGTH-X"
        }));

        for invalid in [
            "SET(DATA-X) LENGTH(LENGTH-X)",
            "INTO(DATA-X) SET(PTR-X) LENGTH(LENGTH-X)",
            "SET(PTR-X)",
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADSET. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8). 01 PTR-X POINTER. 01 LENGTH-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS RETRIEVE {invalid} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{invalid}");
        }
    }

    #[test]
    fn cics_start_may_defer_request_identity_generation_to_runtime() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. GENREQ. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(8) VALUE 'PAYLOAD'. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') FROM(DATA-X) PROTECT NOCHECK END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("generated START REQID: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed START");
        assert_eq!(command.operation, HirCicsOperation::Start);
        assert!(command.options.contains(&HirCicsOption::Protect));
        assert!(command.options.contains(&HirCicsOption::NoCheck));
        assert!(
            command
                .operands
                .iter()
                .all(|operand| operand.name != HirCicsOperandName::ReqId)
        );
    }

    #[test]
    fn cics_start_without_data_keeps_from_optional_and_rejects_dependent_options() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. NODATA. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') REQID('NODATA01') AFTER SECONDS(0) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("no-data START: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed START");
        assert_eq!(command.operation, HirCicsOperation::Start);
        assert!(command.options.contains(&HirCicsOption::After));
        assert!(command.operands.iter().all(|operand| {
            !matches!(
                operand.name,
                HirCicsOperandName::From | HirCicsOperandName::Length
            )
        }));

        for invalid in ["LENGTH(1)", "FMH"] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADNODAT. PROCEDURE DIVISION. EXEC CICS START TRANSID('NEXT') {invalid} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{invalid}");
        }
    }

    #[test]
    fn cics_cancel_lowers_only_the_bounded_local_start_form() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CANCELL. DATA DIVISION. WORKING-STORAGE SECTION. 01 REQ-X PIC X(8) VALUE 'REQ0001'. 01 TRANS-X PIC X(4) VALUE 'NEXT'. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS CANCEL REQID(REQ-X) TRANSID(TRANS-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("CANCEL: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed CANCEL");
        assert_eq!(command.operation, HirCicsOperation::Cancel);
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::ReqId
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "REQ-X"
                )
        }));
        assert!(command.operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::TransId
                && matches!(
                    &operand.value,
                    HirCicsValue::Data(reference) if reference.qualified_name == "TRANS-X"
                )
        }));

        for command in [
            "CANCEL TRANSID('NEXT')",
            "CANCEL REQID('REQ0001') SYSID('R001')",
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CANBAD. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{command}");
        }
    }

    #[test]
    fn cics_delay_lowers_literal_intervals_and_bounded_names() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DELAY0. DATA DIVISION. WORKING-STORAGE SECTION. 01 REQ-X PIC X(8) VALUE 'WAIT0002'. 01 WHEN-X PIC S9(6) COMP-3 VALUE 1. PROCEDURE DIVISION. EXEC CICS DELAY END-EXEC. EXEC CICS DELAY INTERVAL(0) END-EXEC. EXEC CICS DELAY INTERVAL(1) END-EXEC. EXEC CICS DELAY INTERVAL(2) REQID('WAIT0001') END-EXEC. EXEC CICS DELAY INTERVAL(3) REQID(REQ-X) END-EXEC. EXEC CICS DELAY INTERVAL(WHEN-X) REQID('WAIT0003') END-EXEC. EXEC CICS DELAY INTERVAL(60) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("DELAY zero: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 7);
        assert!(
            commands
                .iter()
                .all(|command| command.operation == HirCicsOperation::Delay)
        );
        assert!(commands[0].operands.is_empty());
        assert_eq!(
            commands[1].operands,
            [HirCicsNamedOperand {
                name: HirCicsOperandName::Interval,
                value: HirCicsValue::Integer(0),
            }]
        );
        assert_eq!(
            commands[2].operands,
            [HirCicsNamedOperand {
                name: HirCicsOperandName::Interval,
                value: HirCicsValue::Integer(1),
            }]
        );
        for (command, expected) in [(&commands[3], "WAIT0001"), (&commands[4], "REQ-X")] {
            assert!(command.operands.iter().any(|operand| {
                operand.name == HirCicsOperandName::ReqId
                    && match &operand.value {
                        HirCicsValue::Literal(value) => value == expected,
                        HirCicsValue::Data(reference) => reference.qualified_name == expected,
                        _ => false,
                    }
            }));
        }
        assert!(commands[5].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Interval,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "WHEN-X"
            )
        }));
        assert!(commands[6].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Interval
                && operand.value == HirCicsValue::Integer(60)
        }));

        for command in [
            "DELAY FOR",
            "DELAY HOURS(1)",
            "DELAY FOR UNTIL HOURS(1)",
            "DELAY INTERVAL(1) FOR HOURS(1)",
            "DELAY REQID('WAIT0001')",
            "DELAY INTERVAL(0) REQID('WAIT0001')",
        ] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. DELBAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 WHEN-X PIC S9(6) COMP-3 VALUE 0. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{command}");
        }
    }

    #[test]
    fn cics_delay_for_until_preserve_literal_and_dynamic_units() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DELUNIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 TIME-X PIC S9(9) COMP VALUE 3. 01 CLOCK-X PIC S9(6) COMP-3 VALUE 130000. 01 MS-X PIC S9(9) COMP VALUE 250. PROCEDURE DIVISION. EXEC CICS DELAY FOR HOURS(1) SECONDS(TIME-X) END-EXEC. EXEC CICS DELAY UNTIL MINUTES(759) REQID('UNTIL001') END-EXEC. EXEC CICS DELAY TIME(124500) END-EXEC. EXEC CICS DELAY TIME(CLOCK-X) REQID('CLOCK001') END-EXEC. EXEC CICS DELAY FOR MILLISECS(MS-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("DELAY units: {:?}", analysis.diagnostics));
        let commands = hir
            .statements
            .iter()
            .filter_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 5);
        assert!(commands[0].options.contains(&HirCicsOption::For));
        assert!(commands[1].options.contains(&HirCicsOption::Until));
        assert!(commands[0].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Hours && operand.value == HirCicsValue::Integer(1)
        }));
        assert!(commands[0].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Seconds,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "TIME-X"
            )
        }));
        assert!(commands[1].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::Minutes
                && operand.value == HirCicsValue::Integer(759)
        }));
        assert!(commands[2].operands.iter().any(|operand| {
            operand.name == HirCicsOperandName::StartTime
                && operand.value == HirCicsValue::Integer(124_500)
        }));
        assert!(commands[3].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::StartTime,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "CLOCK-X"
            )
        }));
        assert!(commands[4].options.contains(&HirCicsOption::For));
        assert!(commands[4].operands.iter().any(|operand| {
            matches!(
                operand,
                HirCicsNamedOperand {
                    name: HirCicsOperandName::Milliseconds,
                    value: HirCicsValue::Data(reference),
                } if reference.qualified_name == "MS-X"
            )
        }));

        let invalid = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADMS. PROCEDURE DIVISION. EXEC CICS DELAY UNTIL MILLISECS(1) END-EXEC. STOP RUN.",
        );
        assert!(invalid.hir.is_none());
    }

    #[test]
    fn catalog_known_unready_cics_command_fails_before_legacy_lowering() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSWAIT. DATA DIVISION. \
            WORKING-STORAGE SECTION. 01 PTR-X PIC X(8). PROCEDURE DIVISION. \
            EXEC CICS ADDRESS ACEE(PTR-X) END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("ADDRESS") && message.contains("handler is unready")
        }));
    }

    #[test]
    fn application_cics_routes_have_no_remaining_legacy_lowering() {
        let legacy = mainframe_env_ir::CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::LegacyCompatibility
            })
            .collect::<Vec<_>>();
        assert!(legacy.is_empty());
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
    fn cics_optional_and_alternate_forms_do_not_become_false_discriminators() {
        let handle = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSHAND. PROCEDURE DIVISION. EXEC CICS HANDLE ABEND LABEL(ABEND-HANDLER) END-EXEC. STOP RUN. ABEND-HANDLER. STOP RUN.",
        );
        let hir = handle
            .hir
            .unwrap_or_else(|| panic!("HANDLE ABEND: {:?}", handle.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(matches!(
            statement.resolved,
            Some(HirResolvedStatement::Cics(HirCicsStatement {
                operation: HirCicsOperation::HandleAbend,
                ref operands,
                ..
            })) if matches!(
                operands.as_slice(),
                [HirCicsNamedOperand {
                    name: HirCicsOperandName::Label,
                    value: HirCicsValue::Literal(label),
                }] if label == "ABEND-HANDLER"
            )
        ));

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
    fn typed_cics_routes_reject_catalog_options_without_runtime_semantics() {
        let command = "ASSIGN USERNAME(USER-X)";
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLEG. DATA DIVISION. WORKING-STORAGE SECTION. 01 USER-X PIC X(8). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
        );
        let analysis = analyze(&source);
        assert!(analysis.hir.is_none(), "{command}");
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains("catalog-known but typed lowering is unready")
                    && message.contains("USERNAME")
            }),
            "{command}: {:?}",
            analysis.diagnostics
        );

        let conflict = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSCONF. PROCEDURE DIVISION. EXEC CICS HANDLE ABEND CANCEL RESET END-EXEC. STOP RUN.",
        );
        assert!(conflict.hir.is_none());
        assert!(conflict.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("HANDLE ABEND action options are mutually exclusive")
        }));
    }

    #[test]
    fn typed_assign_keeps_only_implemented_output_forms() {
        for command in [
            "ASSIGN APPLID(APPL-X)",
            "ASSIGN ABCODE(ABCODE-X) ABDUMP(ABDUMP-X) ABOFFSET(ABOFFSET-X) ABPROGRAM(ABPROGRAM-X) ASRAINTRPT(ASRA-PSW-X) ASRAPSW(ASRA-PSW-X) ASRAPSW16(ASRA-PSW16-X) ASRAREGS(ASRA-REGS-X) ASRAREGS64(ASRA-REGS64-X) ORGABCODE(ABCODE-X)",
            "ASSIGN APPLICATION(APP-X) CHANNEL(CHANNEL-X) MAJORVERSION(MAJOR-X) MICROVERSION(MICRO-X) MINORVERSION(MINOR-X) OPERATION(OPERATION-X) PLATFORM(PLATFORM-X)",
            "ASSIGN BRIDGE(BRIDGE-X)",
            "ASSIGN CMDSEC(INDICATOR-X) RESSEC(INDICATOR-X)",
            "ASSIGN CWALENG(CWA-LENGTH-X) OPERKEYS(OPERKEYS-X) RESTART(RESTART-X) TWALENG(TWA-LENGTH-X)",
            "ASSIGN ALTSCRNHT(ALTERNATE-HEIGHT-X) ALTSCRNWD(ALTERNATE-WIDTH-X) DEFSCRNHT(DEFAULT-HEIGHT-X) DEFSCRNWD(DEFAULT-WIDTH-X) SCRNHT(SCREEN-HEIGHT-X) SCRNWD(SCREEN-WIDTH-X)",
            "ASSIGN DS3270(DS3270-X) DSSCS(DSSCS-X)",
            "ASSIGN DESTCOUNT(CWA-LENGTH-X) LDCMNEM(KEY-X) LDCNUM(INDICATOR-X) PAGENUM(TWA-LENGTH-X) PARTNPAGE(KEY-X)",
            "ASSIGN RETURNPROG(PROGRAM-X)",
            "ASSIGN INVOKINGPROG(PROGRAM-X)",
            "ASSIGN INPARTN(KEY-X)",
            "ASSIGN FACILITY(BRIDGE-X) NETNAME(PROGRAM-X)",
            "ASSIGN TNADDR(TN-ADDRESS-X)",
            "ASSIGN APLKYBD(INDICATOR-X) APLTEXT(INDICATOR-X) BTRANS(INDICATOR-X) COLOR(INDICATOR-X) EWASUPP(INDICATOR-X) EXTDS(INDICATOR-X) GMMI(INDICATOR-X) HILIGHT(INDICATOR-X)",
            "ASSIGN KATAKANA(INDICATOR-X) MSRCONTROL(INDICATOR-X) OUTLINE(INDICATOR-X) PARTNS(INDICATOR-X) PS(INDICATOR-X) SOSI(INDICATOR-X) TEXTKYBD(INDICATOR-X) TEXTPRINT(INDICATOR-X) UNATTEND(INDICATOR-X) VALIDATION(INDICATOR-X)",
            "ASSIGN FCI(FCI-X)",
            "ASSIGN INITPARM(INITPARM-X) INITPARMLEN(INITPARM-LENGTH-X)",
            "ASSIGN INPUTMSGLEN(INITPARM-LENGTH-X)",
            "ASSIGN LANGINUSE(OPSECURITY-X)",
            "ASSIGN LINKLEVEL(LINK-LEVEL-X)",
            "ASSIGN LOCALCCSID(MAJOR-X)",
            "ASSIGN MAPCOLUMN(CWA-LENGTH-X) MAPHEIGHT(CWA-LENGTH-X) MAPLINE(TWA-LENGTH-X) MAPWIDTH(TWA-LENGTH-X)",
            "ASSIGN NEXTTRANSID(NEXT-TRANS-X)",
            "ASSIGN OPSECURITY(OPSECURITY-X) TCTUALENG(TCTUA-LENGTH-X)",
            "ASSIGN PARTNSET(PARTITION-SET-X)",
            "ASSIGN PRINSYSID(BRIDGE-X)",
            "ASSIGN PROGRAM(PROGRAM-X)",
            "ASSIGN QNAME(BRIDGE-X)",
            "ASSIGN TASKPRIORITY(PRIORITY-X) TERMPRIORITY(PRIORITY-X) USERID(USER-X)",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSLEG. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABCODE-X PIC X(4). 01 ABDUMP-X PIC X. 01 ABOFFSET-X PIC S9(9) COMP. 01 ABPROGRAM-X PIC X(8). 01 ALTERNATE-HEIGHT-X PIC S9(4) COMP. 01 ALTERNATE-WIDTH-X PIC S9(4) COMP. 01 APP-X PIC X(64). 01 APPL-X PIC X(8). 01 ASRA-PSW-X PIC X(8). 01 ASRA-PSW16-X PIC X(16). 01 ASRA-REGS-X PIC X(64). 01 ASRA-REGS64-X PIC X(128). 01 BRIDGE-X PIC X(4). 01 CHANNEL-X PIC X(16). 01 CWA-LENGTH-X PIC S9(4) COMP. 01 DEFAULT-HEIGHT-X PIC S9(4) COMP. 01 DEFAULT-WIDTH-X PIC S9(4) COMP. 01 DS3270-X PIC X. 01 DSSCS-X PIC X. 01 FCI-X PIC X. 01 INDICATOR-X PIC X. 01 INITPARM-X PIC X(60). 01 INITPARM-LENGTH-X PIC S9(4) COMP. 01 LINK-LEVEL-X PIC S9(4) COMP. 01 MAJOR-X PIC S9(9) COMP. 01 MICRO-X PIC S9(9) COMP. 01 MINOR-X PIC S9(9) COMP. 01 NEXT-TRANS-X PIC X(4). 01 OPERATION-X PIC X(64). 01 OPERKEYS-X PIC X(8). 01 OPSECURITY-X PIC X(3). 01 PARTITION-SET-X PIC X(6). 01 PLATFORM-X PIC X(64). 01 PROGRAM-X PIC X(8). 01 RESTART-X PIC X. 01 SCREEN-HEIGHT-X PIC S9(4) COMP. 01 SCREEN-WIDTH-X PIC S9(4) COMP. 01 TN-ADDRESS-X PIC X(39). 01 USER-X PIC X(8). 01 PRIORITY-X PIC S9(4) COMP. 01 TCTUA-LENGTH-X PIC S9(4) COMP. 01 TWA-LENGTH-X PIC S9(4) COMP. 01 KEY-X PIC X(2). 01 REC-X PIC X(8). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN. ABEND-HANDLER. STOP RUN."
            );
            let analysis = analyze(&source);
            let hir = analysis
                .hir
                .unwrap_or_else(|| panic!("{command}: {:?}", analysis.diagnostics));
            let command = hir
                .statements
                .iter()
                .find_map(|statement| match statement.resolved.as_ref() {
                    Some(HirResolvedStatement::Cics(command)) => Some(command),
                    _ => None,
                })
                .expect("typed EXEC CICS ASSIGN statement");
            assert_eq!(command.operation, HirCicsOperation::Assign);
            assert!(
                command
                    .outputs
                    .iter()
                    .all(|output| matches!(output.name, HirCicsOutputName::Assign(_))),
                "{command:?}"
            );
        }

        for (name, width) in [
            ("ACTIVITY", 16),
            ("ACTIVITYID", 52),
            ("PROCESS", 36),
            ("PROCESSTYPE", 8),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BTSASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUTPUT-X PIC X({width}). PROCEDURE DIVISION. EXEC CICS ASSIGN {name}(OUTPUT-X) END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_some(), "{name}: {:?}", analysis.diagnostics);

            let wrong_width = width - 1;
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADBTSASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUTPUT-X PIC X({wrong_width}). PROCEDURE DIVISION. EXEC CICS ASSIGN {name}(OUTPUT-X) END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{name}");
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains(name) && message.contains("exact-width data area")
            }));
        }

        let error_message = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. ERRORASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ERROR-X PIC X(500). 01 ERROR-LENGTH-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN ERRORMSG(ERROR-X) ERRORMSGLEN(ERROR-LENGTH-X) END-EXEC. STOP RUN.",
        );
        assert!(
            error_message.hir.is_some(),
            "{:?}",
            error_message.diagnostics
        );
        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADERRORASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ERROR-X PIC X(499). PROCEDURE DIVISION. EXEC CICS ASSIGN ERRORMSG(ERROR-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADERRORASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ERROR-LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN ERRORMSGLEN(ERROR-LENGTH-X) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none());
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.public_message().contains("CICS ASSIGN"))
            );
        }

        let bdi = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BDIASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DESTINATION-X PIC X(8). 01 DESTINATION-LENGTH-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN DESTID(DESTINATION-X) DESTIDLENG(DESTINATION-LENGTH-X) END-EXEC. STOP RUN.",
        );
        assert!(bdi.hir.is_some(), "{:?}", bdi.diagnostics);
        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADBDIASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DESTINATION-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN DESTID(DESTINATION-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADBDIASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DESTINATION-LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DESTIDLENG(DESTINATION-LENGTH-X) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none());
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.public_message().contains("CICS ASSIGN"))
            );
        }

        let duplicate_resource = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSALIAS. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(2). 01 REC-X PIC X(8). PROCEDURE DIVISION. EXEC CICS READNEXT FILE('A') DATASET('B') RIDFLD(KEY-X) INTO(REC-X) END-EXEC. STOP RUN.",
        );
        assert!(duplicate_resource.hir.is_none());
        assert!(duplicate_resource.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("options DATASET and FILE are aliases and mutually exclusive")
        }));

        for source in [
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABCODE-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN ABCODE(ABCODE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABDUMP-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN ABDUMP(ABDUMP-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABOFFSET-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN ABOFFSET(ABOFFSET-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABPROGRAM-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN ABPROGRAM(ABPROGRAM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ASRA-INTRPT-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN ASRAINTRPT(ASRA-INTRPT-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ASRA-PSW-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN ASRAPSW(ASRA-PSW-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ASRA-PSW16-X PIC X(15). PROCEDURE DIVISION. EXEC CICS ASSIGN ASRAPSW16(ASRA-PSW16-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ASRA-REGS-X PIC X(63). PROCEDURE DIVISION. EXEC CICS ASSIGN ASRAREGS(ASRA-REGS-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ASRA-REGS64-X PIC X(127). PROCEDURE DIVISION. EXEC CICS ASSIGN ASRAREGS64(ASRA-REGS64-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 BRIDGE-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN BRIDGE(BRIDGE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLAG-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN CMDSEC(FLAG-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLAG-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN RESSEC(FLAG-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN ALTSCRNHT(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN ALTSCRNWD(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DEFSCRNHT(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DEFSCRNWD(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN SCRNHT(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SCREEN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN SCRNWD(SCREEN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DS3270-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DS3270(DS3270-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DESTCOUNT-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DESTCOUNT(DESTCOUNT-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LDCMNEM-X PIC X. PROCEDURE DIVISION. EXEC CICS ASSIGN LDCMNEM(LDCMNEM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LDCNUM-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN LDCNUM(LDCNUM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LANGUAGE-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN LANGINUSE(LANGUAGE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PAGENUM-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN PAGENUM(PAGENUM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PARTNPAGE-X PIC X. PROCEDURE DIVISION. EXEC CICS ASSIGN PARTNPAGE(PARTNPAGE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 RETURN-X PIC X(9). PROCEDURE DIVISION. EXEC CICS ASSIGN RETURNPROG(RETURN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 INVOKING-X PIC X(9). PROCEDURE DIVISION. EXEC CICS ASSIGN INVOKINGPROG(INVOKING-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PARTITION-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN INPARTN(PARTITION-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 FACILITY-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN FACILITY(FACILITY-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 NETNAME-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN NETNAME(NETNAME-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 TNADDR-X PIC X(38). PROCEDURE DIVISION. EXEC CICS ASSIGN TNADDR(TNADDR-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 TERM-PRIORITY-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN TERMPRIORITY(TERM-PRIORITY-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 DSSCS-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN DSSCS(DSSCS-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 FCI-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN FCI(FCI-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PRIORITY-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN TASKPRIORITY(PRIORITY-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN CWALENG(LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 INITPARM-X PIC X(59). PROCEDURE DIVISION. EXEC CICS ASSIGN INITPARM(INITPARM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 INITPARM-LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN INITPARMLEN(INITPARM-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 INPUT-LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN INPUTMSGLEN(INPUT-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LINK-LEVEL-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN LINKLEVEL(LINK-LEVEL-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LOCAL-CCSID-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN LOCALCCSID(LOCAL-CCSID-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LOCAL-CCSID-X PIC X(4). PROCEDURE DIVISION. EXEC CICS ASSIGN LOCALCCSID(LOCAL-CCSID-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 LOCAL-CCSID-X PIC S9(7)V9 COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN LOCALCCSID(LOCAL-CCSID-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 MAP-COLUMN-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN MAPCOLUMN(MAP-COLUMN-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 MAP-HEIGHT-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN MAPHEIGHT(MAP-HEIGHT-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 MAP-LINE-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN MAPLINE(MAP-LINE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 MAP-WIDTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN MAPWIDTH(MAP-WIDTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 NEXT-TRANS-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN NEXTTRANSID(NEXT-TRANS-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 VERSION-X PIC S9(4) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN MAJORVERSION(VERSION-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 OPERKEYS-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN OPERKEYS(OPERKEYS-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 OPSECURITY-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN OPSECURITY(OPSECURITY-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ORGABCODE-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN ORGABCODE(ORGABCODE-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PARTITION-SET-X PIC X(5). PROCEDURE DIVISION. EXEC CICS ASSIGN PARTNSET(PARTITION-SET-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PRINCIPAL-SYSTEM-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN PRINSYSID(PRINCIPAL-SYSTEM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PROGRAM-X PIC X(7). PROCEDURE DIVISION. EXEC CICS ASSIGN PROGRAM(PROGRAM-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 QNAME-X PIC X(3). PROCEDURE DIVISION. EXEC CICS ASSIGN QNAME(QNAME-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 TCTUA-LENGTH-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN TCTUALENG(TCTUA-LENGTH-X) END-EXEC. STOP RUN.",
            "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. PROCEDURE DIVISION. EXEC CICS ASSIGN USERID(MISSING-X) END-EXEC. STOP RUN.",
        ] {
            let analysis = analyze(source);
            assert!(analysis.hir.is_none());
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains("CICS ASSIGN") && message.contains("data area")
            }));
        }

        for name in [
            "APLKYBD",
            "APLTEXT",
            "BTRANS",
            "COLOR",
            "EWASUPP",
            "EXTDS",
            "GMMI",
            "HILIGHT",
            "KATAKANA",
            "MSRCONTROL",
            "OUTLINE",
            "PARTNS",
            "PS",
            "SOSI",
            "TEXTKYBD",
            "TEXTPRINT",
            "UNATTEND",
            "VALIDATION",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 INDICATOR-X PIC X(2). PROCEDURE DIVISION. EXEC CICS ASSIGN {name}(INDICATOR-X) END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{name}");
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.public_message();
                message.contains(name) && message.contains("exact-width data area")
            }));
        }

        let too_many = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. MANYASSIGN. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X(64). 01 SHORT-X PIC S9(4) COMP. 01 LONG-X PIC S9(9) COMP. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ASSIGN APPLICATION(TEXT-X) APPLID(TEXT-X) CHANNEL(TEXT-X) CWALENG(SHORT-X) MAJORVERSION(LONG-X) MICROVERSION(LONG-X) MINORVERSION(LONG-X) OPERATION(TEXT-X) OPERKEYS(TEXT-X) PLATFORM(TEXT-X) RESTART(TEXT-X) SYSID(TEXT-X) TASKPRIORITY(SHORT-X) TWALENG(SHORT-X) USERID(TEXT-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.",
        );
        assert!(too_many.hir.is_none());
        assert!(too_many.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("CICS ASSIGN permits at most 16 options, found 17")
        }));
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
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed HANDLE CONDITION");
        assert_eq!(statement.operation, HirCicsOperation::HandleCondition);
        assert_eq!(
            statement.operands,
            vec![HirCicsNamedOperand {
                name: HirCicsOperandName::Conditions,
                value: HirCicsValue::Literal("ERROR\tERR-HANDLER\nLENGERR\t".into()),
            }]
        );

        let ignore = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSIGN. PROCEDURE DIVISION. EXEC CICS IGNORE CONDITION ERROR LENGERR END-EXEC. STOP RUN.",
        );
        let ignore = ignore
            .hir
            .unwrap_or_else(|| panic!("IGNORE CONDITION: {:?}", ignore.diagnostics));
        let command = ignore
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed IGNORE CONDITION");
        assert_eq!(command.operation, HirCicsOperation::IgnoreCondition);
        assert_eq!(
            command.operands,
            vec![HirCicsNamedOperand {
                name: HirCicsOperandName::Conditions,
                value: HirCicsValue::Literal("ERROR\nLENGERR".into()),
            }]
        );

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
    fn cics_handle_aid_clauses_resolve_optional_labels_and_bounds() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSAID. PROCEDURE DIVISION. EXEC CICS HANDLE AID ANYKEY(ANY-HANDLER) ENTER PF10(PF-HANDLER) END-EXEC. STOP RUN.",
        );
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("HANDLE AID: {:?}", analysis.diagnostics));
        let command = hir
            .statements
            .iter()
            .find_map(|statement| match statement.resolved.as_ref() {
                Some(HirResolvedStatement::Cics(command)) => Some(command),
                _ => None,
            })
            .expect("typed HANDLE AID");
        assert_eq!(command.operation, HirCicsOperation::HandleAid);
        assert_eq!(
            command.operands,
            vec![HirCicsNamedOperand {
                name: HirCicsOperandName::Aids,
                value: HirCicsValue::Literal(
                    "ANYKEY\tANY-HANDLER\nENTER\t\nPF10\tPF-HANDLER".into(),
                ),
            }]
        );

        let bare = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. AIDBARE. PROCEDURE DIVISION. EXEC CICS HANDLE AID END-EXEC. STOP RUN.",
        );
        assert!(bare.hir.is_some(), "{:?}", bare.diagnostics);
        for (command, expected) in [
            ("HANDLE AID PF25(HANDLER)", "unknown or unreviewed"),
            ("HANDLE AID PF1(BAD_LABEL)", "requires one label operand"),
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. AIDBAD. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.public_message().contains(expected) })
            );
        }
        let seventeen = mainframe_env_ir::CICS_APPLICATION_AID_NAMES[..17].join(" ");
        let analysis = analyze(&format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. AID17. PROCEDURE DIVISION. EXEC CICS HANDLE AID {seventeen} END-EXEC. STOP RUN."
        ));
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("permits at most 16 AID clauses, found 17")
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

    #[test]
    fn cics_return_tolerates_a_column_seven_comment_between_options() {
        // toreleon/mainframe-env#176: a standard fixed-format comment line
        // (`*` in column 7) between EXEC CICS options must not reach the
        // resolved clause text. Modeled on the AWS CardDemo (`59cc6c2f`)
        // RETURN pattern in CORPT00C.cbl/COTRN02C.cbl, not copied verbatim.
        // IBM Enterprise COBOL 6.5 Language Reference (`rlfmtcom.html`):
        // a column-7 comment line carries no syntax and may appear
        // anywhere in fixed-format source.
        let source = concat!(
            "       IDENTIFICATION DIVISION.\n",
            "       PROGRAM-ID. CICSRTN.\n",
            "       DATA DIVISION.\n",
            "       WORKING-STORAGE SECTION.\n",
            "       01 WS-TRAN-ID PIC X(4).\n",
            "       01 WS-COMM-AREA PIC X(10).\n",
            "       PROCEDURE DIVISION.\n",
            "           EXEC CICS RETURN\n",
            "               TRANSID (WS-TRAN-ID)\n",
            "               COMMAREA (WS-COMM-AREA)\n",
            "      *        LENGTH(LENGTH OF WS-COMM-AREA)\n",
            "           END-EXEC.\n",
            "           STOP RUN.\n",
        );
        let analysis = analyze_fixed(source);
        let hir = analysis
            .hir
            .expect("EXEC CICS RETURN should compile with a column-7 comment between options");
        assert!(
            hir.statements
                .iter()
                .any(|statement| statement.kind == StatementKind::ExecCics)
        );
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

        for invalid in ["LENGTH(8)", "LENGTH(LENGTH OF REC-X)"] {
            let analysis = analyze(&format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. BADRLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(8). 01 KEY-X PIC X(3). PROCEDURE DIVISION. EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD(KEY-X) {invalid} END-EXEC. STOP RUN."
            ));
            assert!(analysis.hir.is_none(), "{invalid}");
        }
    }

    #[test]
    fn cics_dataset_alias_covers_the_browse_family() {
        // CardDemo (`59cc6c2f`) writes DATASET(...) on STARTBR/READNEXT/ENDBR;
        // COBIL00C.cbl:443 and COCRDLIC.cbl:1129 are pinned STARTBR sites.
        // Main routes these commands through typed HIR; the alias must reach
        // that route with the file-control operands intact.
        for command in [
            "STARTBR DATASET('TRANSACT') RIDFLD(KEY-X) KEYLENGTH(LENGTH OF KEY-X) RESP(RESP-X) RESP2(RESP2-X)",
            "READNEXT DATASET('TRANSACT') INTO(REC-X) RIDFLD(KEY-X) RESP(RESP-X) RESP2(RESP2-X)",
            "ENDBR DATASET('TRANSACT') RESP(RESP-X) RESP2(RESP2-X)",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBR. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(8). 01 KEY-X PIC X(3). 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            let hir = analysis
                .hir
                .unwrap_or_else(|| panic!("{command}: {:?}", analysis.diagnostics));
            let statement = hir
                .statements
                .iter()
                .find(|statement| statement.kind == StatementKind::ExecCics)
                .unwrap_or_else(|| panic!("{command}: no EXEC CICS statement"));
            assert!(
                matches!(
                    statement.resolved.as_ref(),
                    Some(HirResolvedStatement::Cics(_))
                ),
                "{command}"
            );
        }

        // RESETBR is a `family: "file-control"` row that declares `FILE` and
        // not `DATASET`, so the alias still applies and the command no
        // longer fails with "unknown or unreviewed top-level option
        // DATASET". It is a pre-existing `Unready` handler even for
        // `FILE(...)`, so it still fails to compile -- for that unrelated,
        // pre-existing reason, which this asserts by name so the DATASET
        // option-acceptance regression cannot hide behind it.
        let resetbr = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBR. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS RESETBR DATASET('TRANSACT') RIDFLD(KEY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.",
        );
        assert!(resetbr.hir.is_none());
        assert!(resetbr.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("RESETBR") && message.contains("handler is unready")
        }));
        assert!(!resetbr.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("unknown or unreviewed top-level option")
        }));
    }

    #[test]
    fn cics_dataset_alias_resolves_write_and_delete() {
        // COUSR01C.cbl:240 (WRITE) and COUSR03C.cbl:306 (DELETE) both write
        // DATASET(...). WRITE shares its head with WRITE JOURNALNAME/
        // JOURNALNUM/OPERATOR and picks the file-control row by which option
        // is present, so this also proves the DATASET alias satisfies that
        // discriminator the same way FILE(...) does.
        let write = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSWR. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(8). 01 KEY-X PIC X(3). 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS WRITE DATASET('USRSEC') FROM(REC-X) LENGTH(LENGTH OF REC-X) RIDFLD(KEY-X) KEYLENGTH(LENGTH OF KEY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.",
        );
        let hir = write
            .hir
            .unwrap_or_else(|| panic!("WRITE DATASET: {:?}", write.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("WRITE EXEC CICS statement");
        assert!(matches!(
            statement.resolved.as_ref(),
            Some(HirResolvedStatement::Cics(_))
        ));

        let delete = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS DELETE DATASET('USRSEC') RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.",
        );
        let hir = delete
            .hir
            .unwrap_or_else(|| panic!("DELETE DATASET: {:?}", delete.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("DELETE EXEC CICS statement");
        assert!(matches!(
            statement.resolved.as_ref(),
            Some(HirResolvedStatement::Cics(_))
        ));
    }

    #[test]
    fn cics_dataset_and_file_together_are_rejected() {
        let both = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBOTH. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS STARTBR FILE('TRANSACT') DATASET('TRANSACT') RIDFLD(KEY-X) RESP(RESP-X) END-EXEC. STOP RUN.",
        );
        assert!(both.hir.is_none());
        assert!(both.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("STARTBR")
                && message.contains("FILE")
                && message.contains("DATASET")
                && message.contains("mutually exclusive")
        }));

        let repeated = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUP. DATA DIVISION. WORKING-STORAGE SECTION. 01 KEY-X PIC X(3). 01 RESP-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS STARTBR DATASET('TRANSACT') DATASET('TRANSACT') RIDFLD(KEY-X) RESP(RESP-X) END-EXEC. STOP RUN.",
        );
        assert!(repeated.hir.is_none());
        assert!(repeated.diagnostics.iter().any(|diagnostic| {
            let message = diagnostic.public_message();
            message.contains("DATASET") && message.contains("is duplicated")
        }));
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
    fn cics_registry_accepts_bare_and_valued_optional_operand_options() {
        let analysis = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSOPT. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. STOP RUN.",
        );
        let semantic = analysis.semantic.as_ref().expect("semantic model");
        for command in [
            vec![
                "SEND", "MAP", "(", "'MENU'", ")", "MAPSET", "(", "'MAIN'", ")", "CURSOR",
            ],
            vec![
                "SEND", "MAP", "(", "'MENU'", ")", "MAPSET", "(", "'MAIN'", ")", "CURSOR", "(",
                "5", ")",
            ],
            vec![
                "FORMATTIME",
                "ABSTIME",
                "(",
                "ABS-TIME-X",
                ")",
                "DATESEP",
                "TIMESEP",
            ],
            vec![
                "FORMATTIME",
                "ABSTIME",
                "(",
                "ABS-TIME-X",
                ")",
                "DATESEP",
                "(",
                "'-'",
                ")",
                "TIMESEP",
                "(",
                "'.'",
                ")",
            ],
        ] {
            let body = command
                .iter()
                .map(|token| (*token).to_string())
                .collect::<Vec<_>>();
            assert!(
                cics_resolution::validated_command(&body, semantic).is_ok(),
                "{command:?}"
            );
        }

        // A bare Value-shape option (its parenthesized operand is fused into
        // the keyword in the pinned diagram, so it is not independently
        // optional) must still be rejected exactly as before.
        let body = ["SEND", "MAP", "(", "'MENU'", ")", "MAPSET"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let Err(ResolutionFailure::Invalid(detail)) =
            cics_resolution::validated_command(&body, semantic)
        else {
            panic!("bare MAPSET must be rejected");
        };
        assert!(detail.contains("MAPSET requires a parenthesized operand"));
    }

    #[test]
    fn cics_registry_accepts_an_exact_repeated_bare_flag_option() {
        let repeated = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUPN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS ASKTIME NOHANDLE ABSTIME(ABS-TIME-X) NOHANDLE END-EXEC. STOP RUN.",
        );
        let repeated_hir = repeated.hir.unwrap_or_else(|| {
            panic!(
                "ASKTIME NOHANDLE ABSTIME NOHANDLE: {:?}",
                repeated.diagnostics
            )
        });
        let repeated_statement = repeated_hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");

        let once = analyze(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUPN. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS ASKTIME NOHANDLE ABSTIME(ABS-TIME-X) END-EXEC. STOP RUN.",
        );
        let once_hir = once
            .hir
            .unwrap_or_else(|| panic!("ASKTIME NOHANDLE ABSTIME: {:?}", once.diagnostics));
        let once_statement = once_hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");

        assert_eq!(repeated_statement.resolved, once_statement.resolved);
    }

    #[test]
    fn cics_registry_still_rejects_other_repeated_options() {
        let cases = [
            // A Value-shape option repeated: still rejected exactly as
            // today (`cics_registry_rejects_unknown_duplicate_and_malformed_top_level_forms`
            // already covers the RETURN TRANSID case; this repeats it here
            // for full-command context).
            (
                "RETURN TRANSID('NEXT') TRANSID('OTHER')",
                "option TRANSID is duplicated",
            ),
            (
                "ASKTIME ABSTIME(ABS-TIME-X) ABSTIME(ABS-TIME-X)",
                "option ABSTIME is duplicated",
            ),
            // Mixed OptionalValue forms: the bare flag and the valued form
            // are different clause shapes, so an exact-repeat carve-out
            // does not apply.
            (
                "SEND MAP('MENU') MAPSET('MAIN') CURSOR CURSOR(5)",
                "option CURSOR is duplicated",
            ),
            // OptionalValue repeated bare: not a `Flag`-shape option, so
            // still rejected.
            (
                "SEND MAP('MENU') MAPSET('MAIN') CURSOR CURSOR",
                "option CURSOR is duplicated",
            ),
        ];
        for (command, expected) in cases {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUPO. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-TIME-X PIC S9(15) COMP-3. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
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
    fn legacy_send_compatibility_is_exactly_bare_send() {
        // toreleon/mainframe-env#177: pinned AWS CardDemo (`59cc6c2f`) issues
        // a bare 3270-logical `SEND FROM(...) LENGTH(...) NOHANDLE ERASE` in
        // its ABEND-ROUTINE paragraphs (e.g. COACTUPC.cbl:4211). Row 0187 is
        // `Unready`; this second compiler-only compatibility descriptor
        // admits exactly that bounded shape to the pre-existing raw
        // `SendText` route the same way `INQUIRE PROGRAM` reaches `Inquire`.
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBSND. DATA DIVISION. WORKING-STORAGE SECTION. 01 WS-DATA PIC X(10). PROCEDURE DIVISION. EXEC CICS SEND FROM(WS-DATA) LENGTH(10) NOHANDLE ERASE END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        let hir = analysis
            .hir
            .unwrap_or_else(|| panic!("bare SEND: {:?}", analysis.diagnostics));
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == StatementKind::ExecCics)
            .expect("EXEC CICS statement");
        assert!(statement.resolved.is_none());

        // A real IBM SEND option outside the bounded compatibility shape, and
        // a bare SEND missing FROM, both fall through to today's behavior:
        // row 0187 is still recognized by the 263-row registry and still
        // `Unready`, so both fail with the pre-existing diagnosis rather than
        // a fabricated "unknown option" from the new compatibility route.
        for command in [
            "SEND CTLCHAR(WS-DATA) FROM(WS-DATA)",
            "SEND LENGTH(10) ERASE",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBSNE. DATA DIVISION. WORKING-STORAGE SECTION. 01 WS-DATA PIC X(10). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            assert!(analysis.hir.is_none(), "{command}");
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    let message = diagnostic.public_message();
                    message.contains("SEND") && message.contains("handler is unready")
                }),
                "{command}: {:?}",
                analysis.diagnostics
            );
        }

        // Main's typed SEND TEXT and SEND MAP routes keep ownership of their
        // catalog labels, so the bare compatibility form never claims them.
        for command in [
            "SEND TEXT FROM(WS-DATA)",
            "SEND MAP('MENU') MAPSET('MAIN')",
            "SEND MAP('MENU') MAPSET('MAIN') NOHANDLE NOHANDLE",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. CICSBSNU. DATA DIVISION. WORKING-STORAGE SECTION. 01 WS-DATA PIC X(10). PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
            );
            let analysis = analyze(&source);
            let hir = analysis
                .hir
                .unwrap_or_else(|| panic!("{command}: {:?}", analysis.diagnostics));
            let statement = hir
                .statements
                .iter()
                .find(|statement| statement.kind == StatementKind::ExecCics)
                .unwrap_or_else(|| panic!("{command}: no EXEC CICS statement"));
            assert!(
                matches!(
                    statement.resolved.as_ref(),
                    Some(HirResolvedStatement::Cics(_))
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn cics_registry_still_rejects_a_repeated_nohandle_pending_source_review() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSDUPN. PROCEDURE DIVISION. EXEC CICS INQUIRE PROGRAM('P001') NOHANDLE NOHANDLE END-EXEC. STOP RUN.";
        let analysis = analyze(source);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .public_message()
                .contains("option NOHANDLE is duplicated")
        }));
    }
}
