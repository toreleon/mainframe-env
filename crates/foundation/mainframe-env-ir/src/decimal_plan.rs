//! Canonical executable plans for resolved decimal assignments.

use crate::StorageId;
use std::fmt;

/// Stable wire identity written for a canonical decimal assignment plan.
pub const DECIMAL_ASSIGNMENT_PLAN_CONTRACT: &str = "mainframe-env.decimal-assignment-plan@2";
/// Historical plan identity whose execution policy was implicit in COBOL module state.
pub const LEGACY_DECIMAL_ASSIGNMENT_PLAN_CONTRACT: &str = "mainframe-env.decimal-assignment-plan@1";

const MAGIC: &[u8; 4] = b"MDAP";
const LEGACY_VERSION: u16 = 1;
const VERSION: u16 = 2;
const MIN_ENCODED_ASSIGNMENT_BYTES: usize = 16;

/// Version selected by the canonical decimal-plan wire header.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalPlanWireVersion {
    /// Historical plan whose arithmetic context came from COBOL module config.
    LegacyV1,
    /// Current plan carrying every execution policy explicitly.
    PolicyV2,
}

impl DecimalPlanWireVersion {
    /// Stable contract identity represented by this wire version.
    #[must_use]
    pub const fn contract(self) -> &'static str {
        match self {
            Self::LegacyV1 => LEGACY_DECIMAL_ASSIGNMENT_PLAN_CONTRACT,
            Self::PolicyV2 => DECIMAL_ASSIGNMENT_PLAN_CONTRACT,
        }
    }
}

/// Precision and arithmetic-context policy for expression evaluation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalArithmeticContext {
    /// Historical v1 behavior: select 18 or 34 digits from COBOL module config.
    /// This value is read-only compatibility state and cannot be emitted in v2.
    LegacyCobolModuleV1,
    /// 18 digits, exponent range -9999..=9999, primitive truncation, and nine
    /// guard places for division.
    Decimal18V1,
    /// 34 digits, exponent range -9999..=9999, primitive truncation, and nine
    /// guard places for division.
    Decimal34V1,
}

/// ABI used to decode and encode storage referenced by a decimal plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalStorageAbi {
    /// Version 1 COBOL numeric layout metadata and physical encodings.
    CobolNumericV1,
}

/// Visibility and commit policy for assignment receivers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalReceiverUpdatePolicy {
    /// Capture and convert every receiver, then commit the whole batch atomically.
    CapturedOperandsAtomicV1,
    /// Capture every operand first and commit receiver-local results. A COBOL
    /// adapter preserves a conversion-failed receiver when `ON SIZE ERROR` is
    /// declared, or stores its truncated result when that handler is absent.
    CapturedOperandsReceiverLocalV1,
}

/// Condition contract raised by decimal evaluation and receiver conversion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalConditionPolicy {
    /// COBOL `arithmetic-size-error@1`, including handler-dependent receiver
    /// preservation/truncation and condition timing after all receiver work.
    CobolSizeErrorV1,
}

/// Complete, versioned behavior selected by a decimal executable plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DecimalExecutionPolicy {
    /// Precision, exponent, and division context.
    pub arithmetic_context: DecimalArithmeticContext,
    /// Storage metadata and byte encoding.
    pub storage_abi: DecimalStorageAbi,
    /// Receiver capture and update behavior.
    pub receiver_update: DecimalReceiverUpdatePolicy,
    /// Failure-to-condition mapping.
    pub condition: DecimalConditionPolicy,
}

impl DecimalExecutionPolicy {
    /// Current explicit policy matching COBOL `ARITH(COMPAT)` arithmetic.
    #[must_use]
    pub const fn decimal18_v1() -> Self {
        Self {
            arithmetic_context: DecimalArithmeticContext::Decimal18V1,
            storage_abi: DecimalStorageAbi::CobolNumericV1,
            receiver_update: DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1,
            condition: DecimalConditionPolicy::CobolSizeErrorV1,
        }
    }

    /// Current explicit policy matching COBOL `ARITH(EXTEND)` arithmetic.
    #[must_use]
    pub const fn decimal34_v1() -> Self {
        Self {
            arithmetic_context: DecimalArithmeticContext::Decimal34V1,
            storage_abi: DecimalStorageAbi::CobolNumericV1,
            receiver_update: DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1,
            condition: DecimalConditionPolicy::CobolSizeErrorV1,
        }
    }

    const fn legacy_cobol_v1() -> Self {
        Self {
            arithmetic_context: DecimalArithmeticContext::LegacyCobolModuleV1,
            storage_abi: DecimalStorageAbi::CobolNumericV1,
            receiver_update: DecimalReceiverUpdatePolicy::CapturedOperandsAtomicV1,
            condition: DecimalConditionPolicy::CobolSizeErrorV1,
        }
    }
}

/// Resource limits applied while encoding and decoding decimal assignment plans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecimalPlanLimits {
    /// Maximum size of the complete encoded plan.
    pub max_encoded_bytes: usize,
    /// Maximum number of assignments in one bounded execution batch.
    pub max_assignments: usize,
    /// Maximum total expression nodes across the batch.
    pub max_expression_nodes: usize,
    /// Maximum nesting depth of one expression, counting its root as depth one.
    pub max_expression_depth: usize,
    /// Maximum UTF-8 byte length of the semantic-origin identity.
    pub max_semantic_origin_bytes: usize,
    /// Maximum UTF-8 byte length of a qualified layout name.
    pub max_qualified_name_bytes: usize,
    /// Maximum admitted literal scale.
    pub max_literal_scale: u32,
}

impl Default for DecimalPlanLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 1024 * 1024,
            max_assignments: 4_096,
            max_expression_nodes: 65_536,
            max_expression_depth: 128,
            max_semantic_origin_bytes: 256,
            max_qualified_name_bytes: 1_024,
            max_literal_scale: 9_999,
        }
    }
}

/// A storage slot resolved by the frontend before executable-plan publication.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DecimalStorageSlot {
    /// Stable storage arena slot in the containing IR module.
    pub storage: StorageId,
    /// Canonical, fully qualified language-layout name retained for verification.
    pub qualified_layout_name: String,
}

/// Rounding policy applied when a decimal value is encoded into a receiver.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecimalRoundingPolicy {
    /// Discard digits beyond the receiver scale.
    Truncation,
    /// Round away from zero whenever discarded digits are nonzero.
    AwayFromZero,
    /// Round to the nearest value, resolving ties away from zero.
    NearestAwayFromZero,
    /// Round to the nearest value, resolving ties to an even digit.
    NearestEven,
    /// Reject an assignment that would require rounding.
    Prohibited,
    /// Round toward positive infinity.
    TowardGreater,
    /// Round toward negative infinity.
    TowardLesser,
}

/// A resolved decimal assignment destination and its write policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalReceiver {
    /// Resolved destination storage slot and qualified layout identity.
    pub target: DecimalStorageSlot,
    /// Rounding behavior used when storing the result.
    pub rounding: DecimalRoundingPolicy,
}

/// A typed decimal expression whose static grammar and storage names are resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecimalExpression {
    /// An exact base-ten value represented as `coefficient * 10^-scale`.
    Literal {
        /// Signed integer coefficient.
        coefficient: i128,
        /// Count of fractional decimal digits.
        scale: u32,
    },
    /// Read the decimal value in a resolved storage slot.
    Storage(DecimalStorageSlot),
    /// Read the current byte length of a resolved storage slot as an integer.
    Length(DecimalStorageSlot),
    /// Negate one expression.
    Negate(Box<Self>),
    /// Add two expressions.
    Add {
        /// Left operand.
        left: Box<Self>,
        /// Right operand.
        right: Box<Self>,
    },
    /// Subtract the right expression from the left expression.
    Subtract {
        /// Left operand.
        left: Box<Self>,
        /// Right operand.
        right: Box<Self>,
    },
    /// Multiply two expressions.
    Multiply {
        /// Left operand.
        left: Box<Self>,
        /// Right operand.
        right: Box<Self>,
    },
    /// Divide the left expression by the right expression.
    Divide {
        /// Dividend.
        left: Box<Self>,
        /// Divisor.
        right: Box<Self>,
    },
}

/// One expression and the resolved receiver that consumes its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalAssignment {
    /// Value computed before the batch commits its receiving-field writes.
    pub expression: DecimalExpression,
    /// Destination and rounding policy for the computed value.
    pub receiver: DecimalReceiver,
}

/// An ordered, bounded batch of typed decimal assignments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalAssignmentPlan {
    /// Stable identity of the frontend rule that produced the plan; provenance only.
    pub semantic_origin: String,
    /// Versioned execution behavior, independent of producer identity.
    pub policy: DecimalExecutionPolicy,
    /// Assignments evaluated and committed according to `policy`.
    pub assignments: Vec<DecimalAssignment>,
}

/// Encode a decimal assignment plan into its canonical policy-bearing v2 wire form.
pub fn encode_decimal_assignment_plan(
    plan: &DecimalAssignmentPlan,
    limits: DecimalPlanLimits,
) -> Result<Vec<u8>, DecimalPlanCodecProblem> {
    validate_plan(plan, limits)?;
    let mut writer = Writer::new(limits.max_encoded_bytes);
    writer.extend(MAGIC)?;
    writer.u16(VERSION)?;
    encode_policy(&mut writer, plan.policy)?;
    writer.string(&plan.semantic_origin, limits.max_semantic_origin_bytes)?;
    writer.count(plan.assignments.len())?;
    let mut nodes = 0usize;
    for assignment in &plan.assignments {
        encode_slot(&mut writer, &assignment.receiver.target, limits)?;
        writer.byte(rounding_tag(assignment.receiver.rounding))?;
        encode_expression(&mut writer, &assignment.expression, limits, &mut nodes, 1)?;
    }
    Ok(writer.finish())
}

fn encode_legacy_decimal_assignment_plan(
    plan: &DecimalAssignmentPlan,
    limits: DecimalPlanLimits,
) -> Result<Vec<u8>, DecimalPlanCodecProblem> {
    validate_plan_for_version(plan, limits, DecimalPlanWireVersion::LegacyV1)?;
    let mut writer = Writer::new(limits.max_encoded_bytes);
    writer.extend(MAGIC)?;
    writer.u16(LEGACY_VERSION)?;
    writer.string(&plan.semantic_origin, limits.max_semantic_origin_bytes)?;
    writer.count(plan.assignments.len())?;
    let mut nodes = 0usize;
    for assignment in &plan.assignments {
        encode_slot(&mut writer, &assignment.receiver.target, limits)?;
        writer.byte(rounding_tag(assignment.receiver.rounding))?;
        encode_expression(&mut writer, &assignment.expression, limits, &mut nodes, 1)?;
    }
    Ok(writer.finish())
}

/// Decode and validate one canonical v1 or v2 decimal assignment plan.
pub fn decode_decimal_assignment_plan(
    bytes: &[u8],
    limits: DecimalPlanLimits,
) -> Result<DecimalAssignmentPlan, DecimalPlanCodecProblem> {
    if bytes.len() > limits.max_encoded_bytes {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(DecimalPlanCodecProblem::BadMagic);
    }
    let version = version_from_u16(reader.u16()?)?;
    let policy = match version {
        DecimalPlanWireVersion::LegacyV1 => DecimalExecutionPolicy::legacy_cobol_v1(),
        DecimalPlanWireVersion::PolicyV2 => decode_policy(&mut reader)?,
    };
    let semantic_origin = reader.string(limits.max_semantic_origin_bytes)?;
    validate_semantic_origin(&semantic_origin, limits)?;
    let count = reader.count(limits.max_assignments)?;
    if count == 0 && version == DecimalPlanWireVersion::LegacyV1 {
        return Err(DecimalPlanCodecProblem::Malformed);
    }
    if count > reader.remaining().len() / MIN_ENCODED_ASSIGNMENT_BYTES {
        return Err(DecimalPlanCodecProblem::Truncated);
    }
    let mut assignments = Vec::with_capacity(count);
    let mut nodes = 0usize;
    for _ in 0..count {
        let target = decode_slot(&mut reader, limits)?;
        let rounding = rounding_from_tag(reader.byte()?)?;
        let expression = decode_expression(&mut reader, limits, &mut nodes, 1)?;
        assignments.push(DecimalAssignment {
            expression,
            receiver: DecimalReceiver { target, rounding },
        });
    }
    if !reader.remaining().is_empty() {
        return Err(DecimalPlanCodecProblem::TrailingData);
    }
    let plan = DecimalAssignmentPlan {
        semantic_origin,
        policy,
        assignments,
    };
    validate_plan_for_version(&plan, limits, version)?;
    let canonical = match version {
        DecimalPlanWireVersion::LegacyV1 => encode_legacy_decimal_assignment_plan(&plan, limits)?,
        DecimalPlanWireVersion::PolicyV2 => encode_decimal_assignment_plan(&plan, limits)?,
    };
    if canonical != bytes {
        return Err(DecimalPlanCodecProblem::NonCanonical);
    }
    Ok(plan)
}

/// Return the declared decimal-plan wire version without decoding its payload.
pub fn decimal_assignment_plan_wire_version(
    bytes: &[u8],
) -> Result<DecimalPlanWireVersion, DecimalPlanCodecProblem> {
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(DecimalPlanCodecProblem::BadMagic);
    }
    version_from_u16(reader.u16()?)
}

fn validate_plan(
    plan: &DecimalAssignmentPlan,
    limits: DecimalPlanLimits,
) -> Result<(), DecimalPlanCodecProblem> {
    validate_plan_for_version(plan, limits, DecimalPlanWireVersion::PolicyV2)
}

fn validate_plan_for_version(
    plan: &DecimalAssignmentPlan,
    limits: DecimalPlanLimits,
    version: DecimalPlanWireVersion,
) -> Result<(), DecimalPlanCodecProblem> {
    validate_semantic_origin(&plan.semantic_origin, limits)?;
    validate_policy(plan.policy, version)?;
    if plan.assignments.is_empty() && version == DecimalPlanWireVersion::LegacyV1 {
        return Err(DecimalPlanCodecProblem::Malformed);
    }
    if plan.assignments.len() > limits.max_assignments
        || u32::try_from(plan.assignments.len()).is_err()
    {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    let mut nodes = 0usize;
    for assignment in &plan.assignments {
        validate_slot(&assignment.receiver.target, limits)?;
        validate_expression(&assignment.expression, limits, &mut nodes, 1)?;
    }
    Ok(())
}

fn validate_policy(
    policy: DecimalExecutionPolicy,
    version: DecimalPlanWireVersion,
) -> Result<(), DecimalPlanCodecProblem> {
    match (version, policy.arithmetic_context) {
        (DecimalPlanWireVersion::LegacyV1, DecimalArithmeticContext::LegacyCobolModuleV1)
            if policy.storage_abi == DecimalStorageAbi::CobolNumericV1
                && policy.receiver_update
                    == DecimalReceiverUpdatePolicy::CapturedOperandsAtomicV1
                && policy.condition == DecimalConditionPolicy::CobolSizeErrorV1 =>
        {
            Ok(())
        }
        (
            DecimalPlanWireVersion::PolicyV2,
            DecimalArithmeticContext::Decimal18V1 | DecimalArithmeticContext::Decimal34V1,
        ) if policy.storage_abi == DecimalStorageAbi::CobolNumericV1
            && policy.receiver_update
                == DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1
            && policy.condition == DecimalConditionPolicy::CobolSizeErrorV1 =>
        {
            Ok(())
        }
        _ => Err(DecimalPlanCodecProblem::UnsupportedPolicy),
    }
}

const fn version_from_u16(version: u16) -> Result<DecimalPlanWireVersion, DecimalPlanCodecProblem> {
    match version {
        LEGACY_VERSION => Ok(DecimalPlanWireVersion::LegacyV1),
        VERSION => Ok(DecimalPlanWireVersion::PolicyV2),
        _ => Err(DecimalPlanCodecProblem::UnsupportedVersion),
    }
}

fn encode_policy(
    writer: &mut Writer,
    policy: DecimalExecutionPolicy,
) -> Result<(), DecimalPlanCodecProblem> {
    validate_policy(policy, DecimalPlanWireVersion::PolicyV2)?;
    writer.byte(1)?;
    writer.byte(match policy.arithmetic_context {
        DecimalArithmeticContext::Decimal18V1 => 1,
        DecimalArithmeticContext::Decimal34V1 => 2,
        DecimalArithmeticContext::LegacyCobolModuleV1 => {
            return Err(DecimalPlanCodecProblem::UnsupportedPolicy);
        }
    })?;
    writer.byte(match policy.storage_abi {
        DecimalStorageAbi::CobolNumericV1 => 1,
    })?;
    writer.byte(match policy.receiver_update {
        DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1 => 1,
        DecimalReceiverUpdatePolicy::CapturedOperandsAtomicV1 => {
            return Err(DecimalPlanCodecProblem::UnsupportedPolicy);
        }
    })?;
    writer.byte(match policy.condition {
        DecimalConditionPolicy::CobolSizeErrorV1 => 1,
    })
}

fn decode_policy(
    reader: &mut Reader<'_>,
) -> Result<DecimalExecutionPolicy, DecimalPlanCodecProblem> {
    if reader.byte()? != 1 {
        return Err(DecimalPlanCodecProblem::UnsupportedPolicy);
    }
    let arithmetic_context = match reader.byte()? {
        1 => DecimalArithmeticContext::Decimal18V1,
        2 => DecimalArithmeticContext::Decimal34V1,
        _ => return Err(DecimalPlanCodecProblem::UnsupportedPolicy),
    };
    let storage_abi = match reader.byte()? {
        1 => DecimalStorageAbi::CobolNumericV1,
        _ => return Err(DecimalPlanCodecProblem::UnsupportedPolicy),
    };
    let receiver_update = match reader.byte()? {
        1 => DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1,
        _ => return Err(DecimalPlanCodecProblem::UnsupportedPolicy),
    };
    let condition = match reader.byte()? {
        1 => DecimalConditionPolicy::CobolSizeErrorV1,
        _ => return Err(DecimalPlanCodecProblem::UnsupportedPolicy),
    };
    Ok(DecimalExecutionPolicy {
        arithmetic_context,
        storage_abi,
        receiver_update,
        condition,
    })
}

fn validate_semantic_origin(
    value: &str,
    limits: DecimalPlanLimits,
) -> Result<(), DecimalPlanCodecProblem> {
    if value.len() > limits.max_semantic_origin_bytes || value.len() > usize::from(u16::MAX) {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    if value.is_empty()
        || !value.is_ascii()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'@' | b'/')
        })
    {
        return Err(DecimalPlanCodecProblem::NonCanonical);
    }
    Ok(())
}

fn validate_slot(
    slot: &DecimalStorageSlot,
    limits: DecimalPlanLimits,
) -> Result<(), DecimalPlanCodecProblem> {
    let name = slot.qualified_layout_name.as_str();
    if name.len() > limits.max_qualified_name_bytes || name.len() > usize::from(u16::MAX) {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    if name.is_empty()
        || name.bytes().any(|byte| byte.is_ascii_lowercase())
        || name.split('.').any(|part| part.is_empty())
        || !name.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@' | b'.')
        })
    {
        return Err(DecimalPlanCodecProblem::NonCanonical);
    }
    Ok(())
}

fn validate_expression(
    expression: &DecimalExpression,
    limits: DecimalPlanLimits,
    nodes: &mut usize,
    depth: usize,
) -> Result<(), DecimalPlanCodecProblem> {
    consume_node(nodes, depth, limits)?;
    match expression {
        DecimalExpression::Literal { scale, .. } => {
            if *scale > limits.max_literal_scale {
                Err(DecimalPlanCodecProblem::LimitExceeded)
            } else {
                Ok(())
            }
        }
        DecimalExpression::Storage(slot) | DecimalExpression::Length(slot) => {
            validate_slot(slot, limits)
        }
        DecimalExpression::Negate(value) => {
            validate_expression(value, limits, nodes, depth.saturating_add(1))
        }
        DecimalExpression::Add { left, right }
        | DecimalExpression::Subtract { left, right }
        | DecimalExpression::Multiply { left, right }
        | DecimalExpression::Divide { left, right } => {
            validate_expression(left, limits, nodes, depth.saturating_add(1))?;
            validate_expression(right, limits, nodes, depth.saturating_add(1))
        }
    }
}

fn consume_node(
    nodes: &mut usize,
    depth: usize,
    limits: DecimalPlanLimits,
) -> Result<(), DecimalPlanCodecProblem> {
    if depth == 0 || depth > limits.max_expression_depth {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or(DecimalPlanCodecProblem::LimitExceeded)?;
    if *nodes > limits.max_expression_nodes {
        return Err(DecimalPlanCodecProblem::LimitExceeded);
    }
    Ok(())
}

fn encode_slot(
    writer: &mut Writer,
    slot: &DecimalStorageSlot,
    limits: DecimalPlanLimits,
) -> Result<(), DecimalPlanCodecProblem> {
    validate_slot(slot, limits)?;
    writer.u32(slot.storage.get())?;
    writer.string(&slot.qualified_layout_name, limits.max_qualified_name_bytes)
}

fn decode_slot(
    reader: &mut Reader<'_>,
    limits: DecimalPlanLimits,
) -> Result<DecimalStorageSlot, DecimalPlanCodecProblem> {
    let storage = StorageId::from_index(reader.u32()? as usize)
        .map_err(|_| DecimalPlanCodecProblem::Malformed)?;
    let qualified_layout_name = reader.string(limits.max_qualified_name_bytes)?;
    let slot = DecimalStorageSlot {
        storage,
        qualified_layout_name,
    };
    validate_slot(&slot, limits)?;
    Ok(slot)
}

fn encode_expression(
    writer: &mut Writer,
    expression: &DecimalExpression,
    limits: DecimalPlanLimits,
    nodes: &mut usize,
    depth: usize,
) -> Result<(), DecimalPlanCodecProblem> {
    consume_node(nodes, depth, limits)?;
    match expression {
        DecimalExpression::Literal { coefficient, scale } => {
            if *scale > limits.max_literal_scale {
                return Err(DecimalPlanCodecProblem::LimitExceeded);
            }
            writer.byte(0)?;
            writer.i128(*coefficient)?;
            writer.u32(*scale)
        }
        DecimalExpression::Storage(slot) => {
            writer.byte(1)?;
            encode_slot(writer, slot, limits)
        }
        DecimalExpression::Length(slot) => {
            writer.byte(2)?;
            encode_slot(writer, slot, limits)
        }
        DecimalExpression::Negate(value) => {
            writer.byte(3)?;
            encode_expression(writer, value, limits, nodes, depth.saturating_add(1))
        }
        DecimalExpression::Add { left, right } => {
            writer.byte(4)?;
            encode_binary_expression(writer, left, right, limits, nodes, depth)
        }
        DecimalExpression::Subtract { left, right } => {
            writer.byte(5)?;
            encode_binary_expression(writer, left, right, limits, nodes, depth)
        }
        DecimalExpression::Multiply { left, right } => {
            writer.byte(6)?;
            encode_binary_expression(writer, left, right, limits, nodes, depth)
        }
        DecimalExpression::Divide { left, right } => {
            writer.byte(7)?;
            encode_binary_expression(writer, left, right, limits, nodes, depth)
        }
    }
}

fn encode_binary_expression(
    writer: &mut Writer,
    left: &DecimalExpression,
    right: &DecimalExpression,
    limits: DecimalPlanLimits,
    nodes: &mut usize,
    depth: usize,
) -> Result<(), DecimalPlanCodecProblem> {
    encode_expression(writer, left, limits, nodes, depth.saturating_add(1))?;
    encode_expression(writer, right, limits, nodes, depth.saturating_add(1))
}

fn decode_expression(
    reader: &mut Reader<'_>,
    limits: DecimalPlanLimits,
    nodes: &mut usize,
    depth: usize,
) -> Result<DecimalExpression, DecimalPlanCodecProblem> {
    consume_node(nodes, depth, limits)?;
    Ok(match reader.byte()? {
        0 => {
            let coefficient = reader.i128()?;
            let scale = reader.u32()?;
            if scale > limits.max_literal_scale {
                return Err(DecimalPlanCodecProblem::LimitExceeded);
            }
            DecimalExpression::Literal { coefficient, scale }
        }
        1 => DecimalExpression::Storage(decode_slot(reader, limits)?),
        2 => DecimalExpression::Length(decode_slot(reader, limits)?),
        3 => DecimalExpression::Negate(Box::new(decode_expression(
            reader,
            limits,
            nodes,
            depth.saturating_add(1),
        )?)),
        4 => decode_binary(reader, limits, nodes, depth, |left, right| {
            DecimalExpression::Add { left, right }
        })?,
        5 => decode_binary(reader, limits, nodes, depth, |left, right| {
            DecimalExpression::Subtract { left, right }
        })?,
        6 => decode_binary(reader, limits, nodes, depth, |left, right| {
            DecimalExpression::Multiply { left, right }
        })?,
        7 => decode_binary(reader, limits, nodes, depth, |left, right| {
            DecimalExpression::Divide { left, right }
        })?,
        _ => return Err(DecimalPlanCodecProblem::Malformed),
    })
}

fn decode_binary(
    reader: &mut Reader<'_>,
    limits: DecimalPlanLimits,
    nodes: &mut usize,
    depth: usize,
    build: impl FnOnce(Box<DecimalExpression>, Box<DecimalExpression>) -> DecimalExpression,
) -> Result<DecimalExpression, DecimalPlanCodecProblem> {
    let child_depth = depth.saturating_add(1);
    let left = Box::new(decode_expression(reader, limits, nodes, child_depth)?);
    let right = Box::new(decode_expression(reader, limits, nodes, child_depth)?);
    Ok(build(left, right))
}

const fn rounding_tag(rounding: DecimalRoundingPolicy) -> u8 {
    match rounding {
        DecimalRoundingPolicy::Truncation => 0,
        DecimalRoundingPolicy::AwayFromZero => 1,
        DecimalRoundingPolicy::NearestAwayFromZero => 2,
        DecimalRoundingPolicy::NearestEven => 3,
        DecimalRoundingPolicy::Prohibited => 4,
        DecimalRoundingPolicy::TowardGreater => 5,
        DecimalRoundingPolicy::TowardLesser => 6,
    }
}

fn rounding_from_tag(tag: u8) -> Result<DecimalRoundingPolicy, DecimalPlanCodecProblem> {
    Ok(match tag {
        0 => DecimalRoundingPolicy::Truncation,
        1 => DecimalRoundingPolicy::AwayFromZero,
        2 => DecimalRoundingPolicy::NearestAwayFromZero,
        3 => DecimalRoundingPolicy::NearestEven,
        4 => DecimalRoundingPolicy::Prohibited,
        5 => DecimalRoundingPolicy::TowardGreater,
        6 => DecimalRoundingPolicy::TowardLesser,
        _ => return Err(DecimalPlanCodecProblem::Malformed),
    })
}

struct Writer {
    bytes: Vec<u8>,
    max: usize,
}

impl Writer {
    fn new(max: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max,
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn byte(&mut self, value: u8) -> Result<(), DecimalPlanCodecProblem> {
        self.extend(&[value])
    }

    fn u16(&mut self, value: u16) -> Result<(), DecimalPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), DecimalPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }

    fn i128(&mut self, value: i128) -> Result<(), DecimalPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }

    fn count(&mut self, value: usize) -> Result<(), DecimalPlanCodecProblem> {
        self.u32(u32::try_from(value).map_err(|_| DecimalPlanCodecProblem::LimitExceeded)?)
    }

    fn string(&mut self, value: &str, max: usize) -> Result<(), DecimalPlanCodecProblem> {
        if value.len() > max {
            return Err(DecimalPlanCodecProblem::LimitExceeded);
        }
        let length =
            u16::try_from(value.len()).map_err(|_| DecimalPlanCodecProblem::LimitExceeded)?;
        self.u16(length)?;
        self.extend(value.as_bytes())
    }

    fn extend(&mut self, value: &[u8]) -> Result<(), DecimalPlanCodecProblem> {
        let next = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or(DecimalPlanCodecProblem::LimitExceeded)?;
        if next > self.max {
            return Err(DecimalPlanCodecProblem::LimitExceeded);
        }
        self.bytes.extend_from_slice(value);
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], DecimalPlanCodecProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(DecimalPlanCodecProblem::LimitExceeded)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(DecimalPlanCodecProblem::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, DecimalPlanCodecProblem> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DecimalPlanCodecProblem> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| DecimalPlanCodecProblem::Truncated)?,
        ))
    }

    fn u32(&mut self) -> Result<u32, DecimalPlanCodecProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| DecimalPlanCodecProblem::Truncated)?,
        ))
    }

    fn i128(&mut self) -> Result<i128, DecimalPlanCodecProblem> {
        Ok(i128::from_be_bytes(
            self.take(16)?
                .try_into()
                .map_err(|_| DecimalPlanCodecProblem::Truncated)?,
        ))
    }

    fn count(&mut self, max: usize) -> Result<usize, DecimalPlanCodecProblem> {
        let value =
            usize::try_from(self.u32()?).map_err(|_| DecimalPlanCodecProblem::LimitExceeded)?;
        if value > max {
            Err(DecimalPlanCodecProblem::LimitExceeded)
        } else {
            Ok(value)
        }
    }

    fn string(&mut self, max: usize) -> Result<String, DecimalPlanCodecProblem> {
        let length = usize::from(self.u16()?);
        if length > max {
            return Err(DecimalPlanCodecProblem::LimitExceeded);
        }
        std::str::from_utf8(self.take(length)?)
            .map(str::to_owned)
            .map_err(|_| DecimalPlanCodecProblem::Malformed)
    }
}

/// Failure returned by the canonical decimal assignment-plan codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecimalPlanCodecProblem {
    /// The input does not carry the decimal-plan magic bytes.
    BadMagic,
    /// The input uses an unsupported decimal-plan wire version.
    UnsupportedVersion,
    /// The plan selects an unsupported or version-incompatible execution policy.
    UnsupportedPolicy,
    /// The input ends before a declared value is complete.
    Truncated,
    /// Bytes remain after the one complete plan.
    TrailingData,
    /// The input has an unknown tag or structurally invalid value.
    Malformed,
    /// A textual identity or wire representation is not canonical.
    NonCanonical,
    /// A configured size, count, depth, or scale bound was exceeded.
    LimitExceeded,
}

impl fmt::Display for DecimalPlanCodecProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "decimal assignment plan codec failed: {self:?}")
    }
}

impl std::error::Error for DecimalPlanCodecProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn slot(index: usize, name: &str) -> DecimalStorageSlot {
        DecimalStorageSlot {
            storage: StorageId::from_index(index).unwrap(),
            qualified_layout_name: name.into(),
        }
    }

    fn assignment(
        index: usize,
        rounding: DecimalRoundingPolicy,
        expression: DecimalExpression,
    ) -> DecimalAssignment {
        DecimalAssignment {
            expression,
            receiver: DecimalReceiver {
                target: slot(index, &format!("RESULT-{index}")),
                rounding,
            },
        }
    }

    fn complete_plan() -> DecimalAssignmentPlan {
        let left = DecimalExpression::Add {
            left: Box::new(DecimalExpression::Literal {
                coefficient: -125,
                scale: 2,
            }),
            right: Box::new(DecimalExpression::Storage(slot(0, "INPUT.GROUP"))),
        };
        let right = DecimalExpression::Subtract {
            left: Box::new(DecimalExpression::Length(slot(1, "TEXT.GROUP"))),
            right: Box::new(DecimalExpression::Negate(Box::new(
                DecimalExpression::Literal {
                    coefficient: 3,
                    scale: 0,
                },
            ))),
        };
        let expression = DecimalExpression::Divide {
            left: Box::new(DecimalExpression::Multiply {
                left: Box::new(left),
                right: Box::new(right),
            }),
            right: Box::new(DecimalExpression::Literal {
                coefficient: 10,
                scale: 0,
            }),
        };
        let policies = [
            DecimalRoundingPolicy::Truncation,
            DecimalRoundingPolicy::AwayFromZero,
            DecimalRoundingPolicy::NearestAwayFromZero,
            DecimalRoundingPolicy::NearestEven,
            DecimalRoundingPolicy::Prohibited,
            DecimalRoundingPolicy::TowardGreater,
            DecimalRoundingPolicy::TowardLesser,
        ];
        DecimalAssignmentPlan {
            semantic_origin: "cobol.add-assign@1".into(),
            policy: DecimalExecutionPolicy::decimal34_v1(),
            assignments: policies
                .into_iter()
                .enumerate()
                .map(|(index, policy)| assignment(index + 2, policy, expression.clone()))
                .collect(),
        }
    }

    fn first_expression_offset(bytes: &[u8]) -> usize {
        let origin_length = usize::from(u16::from_be_bytes(bytes[11..13].try_into().unwrap()));
        let assignment = 13 + origin_length + 4;
        let name_length = usize::from(u16::from_be_bytes(
            bytes[assignment + 4..assignment + 6].try_into().unwrap(),
        ));
        assignment + 6 + name_length + 1
    }

    #[test]
    fn every_expression_and_rounding_policy_round_trips_canonically() {
        let plan = complete_plan();
        let limits = DecimalPlanLimits::default();
        let encoded = encode_decimal_assignment_plan(&plan, limits).unwrap();
        assert_eq!(&encoded[..4], MAGIC);
        assert_eq!(decode_decimal_assignment_plan(&encoded, limits), Ok(plan));
    }

    #[test]
    fn malformed_trailing_noncanonical_and_wrong_version_inputs_fail_closed() {
        let limits = DecimalPlanLimits::default();
        let encoded = encode_decimal_assignment_plan(&complete_plan(), limits).unwrap();

        let mut bad_magic = encoded.clone();
        bad_magic[0] ^= 1;
        assert_eq!(
            decode_decimal_assignment_plan(&bad_magic, limits),
            Err(DecimalPlanCodecProblem::BadMagic)
        );

        let mut wrong_version = encoded.clone();
        wrong_version[4..6].copy_from_slice(&3u16.to_be_bytes());
        assert_eq!(
            decode_decimal_assignment_plan(&wrong_version, limits),
            Err(DecimalPlanCodecProblem::UnsupportedVersion)
        );

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_decimal_assignment_plan(&trailing, limits),
            Err(DecimalPlanCodecProblem::TrailingData)
        );

        assert_eq!(
            decode_decimal_assignment_plan(&encoded[..encoded.len() - 1], limits),
            Err(DecimalPlanCodecProblem::Truncated)
        );

        let mut malformed = encoded.clone();
        let expression = first_expression_offset(&malformed);
        malformed[expression] = u8::MAX;
        assert_eq!(
            decode_decimal_assignment_plan(&malformed, limits),
            Err(DecimalPlanCodecProblem::Malformed)
        );

        let mut noncanonical = encoded;
        let at = noncanonical
            .windows("RESULT-2".len())
            .position(|window| window == b"RESULT-2")
            .unwrap();
        noncanonical[at] = b'r';
        assert_eq!(
            decode_decimal_assignment_plan(&noncanonical, limits),
            Err(DecimalPlanCodecProblem::NonCanonical)
        );
    }

    #[test]
    fn count_depth_scale_and_total_byte_limits_are_enforced() {
        let plan = complete_plan();
        let encoded = encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default()).unwrap();

        let one_assignment = DecimalPlanLimits {
            max_assignments: 1,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            encode_decimal_assignment_plan(&plan, one_assignment),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, one_assignment),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );

        let shallow = DecimalPlanLimits {
            max_expression_depth: 2,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, shallow),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );

        let too_few_nodes = DecimalPlanLimits {
            max_expression_nodes: 7,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, too_few_nodes),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );

        let integer_only = DecimalPlanLimits {
            max_literal_scale: 1,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, integer_only),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );

        let too_small = DecimalPlanLimits {
            max_encoded_bytes: encoded.len() - 1,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, too_small),
            Err(DecimalPlanCodecProblem::LimitExceeded)
        );
    }

    #[test]
    fn impossible_hostile_count_is_rejected_before_batch_allocation() {
        let origin = b"test.hostile@1";
        let mut encoded = Vec::new();
        encoded.extend_from_slice(MAGIC);
        encoded.extend_from_slice(&VERSION.to_be_bytes());
        encoded.extend_from_slice(&[1, 2, 1, 1, 1]);
        encoded.extend_from_slice(&(origin.len() as u16).to_be_bytes());
        encoded.extend_from_slice(origin);
        encoded.extend_from_slice(&u32::MAX.to_be_bytes());
        let limits = DecimalPlanLimits {
            max_encoded_bytes: encoded.len(),
            max_assignments: u32::MAX as usize,
            ..DecimalPlanLimits::default()
        };
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, limits),
            Err(DecimalPlanCodecProblem::Truncated)
        );
    }

    #[test]
    fn encoder_rejects_noncanonical_semantic_and_layout_identities() {
        let mut plan = complete_plan();
        plan.semantic_origin = "cobol add".into();
        assert_eq!(
            encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default()),
            Err(DecimalPlanCodecProblem::NonCanonical)
        );
        plan.semantic_origin = "cobol.add@1".into();
        plan.assignments[0].receiver.target.qualified_layout_name = "group.result".into();
        assert_eq!(
            encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default()),
            Err(DecimalPlanCodecProblem::NonCanonical)
        );
    }

    #[test]
    fn policy_tags_and_legacy_only_policy_fail_closed() {
        let limits = DecimalPlanLimits::default();
        let encoded = encode_decimal_assignment_plan(&complete_plan(), limits).unwrap();
        for offset in 6..=10 {
            let mut unsupported = encoded.clone();
            unsupported[offset] = u8::MAX;
            assert_eq!(
                decode_decimal_assignment_plan(&unsupported, limits),
                Err(DecimalPlanCodecProblem::UnsupportedPolicy)
            );
        }

        let mut legacy_only = complete_plan();
        legacy_only.policy = DecimalExecutionPolicy::legacy_cobol_v1();
        assert_eq!(
            encode_decimal_assignment_plan(&legacy_only, limits),
            Err(DecimalPlanCodecProblem::UnsupportedPolicy)
        );
    }

    #[test]
    fn historical_v1_plan_decodes_canonically_without_becoming_v2() {
        let limits = DecimalPlanLimits::default();
        let mut historical = complete_plan();
        historical.policy = DecimalExecutionPolicy::legacy_cobol_v1();
        historical.semantic_origin = "cobol.compute@1".into();
        let encoded = encode_legacy_decimal_assignment_plan(&historical, limits).unwrap();
        assert_eq!(
            decimal_assignment_plan_wire_version(&encoded),
            Ok(DecimalPlanWireVersion::LegacyV1)
        );
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, limits),
            Ok(historical)
        );
        assert_eq!(
            decimal_assignment_plan_wire_version(
                &encode_decimal_assignment_plan(&complete_plan(), limits).unwrap()
            ),
            Ok(DecimalPlanWireVersion::PolicyV2)
        );
    }

    #[test]
    fn policy_v2_admits_an_explicit_noop_while_legacy_v1_does_not() {
        let limits = DecimalPlanLimits::default();
        let mut noop = complete_plan();
        noop.semantic_origin = "cobol.add@1".into();
        noop.assignments.clear();
        let encoded = encode_decimal_assignment_plan(&noop, limits).unwrap();
        assert_eq!(
            decode_decimal_assignment_plan(&encoded, limits),
            Ok(noop.clone())
        );

        noop.policy = DecimalExecutionPolicy::legacy_cobol_v1();
        assert_eq!(
            encode_legacy_decimal_assignment_plan(&noop, limits),
            Err(DecimalPlanCodecProblem::Malformed)
        );
    }

    proptest! {
        #[test]
        fn literal_plans_round_trip(
            coefficient in any::<i128>(),
            scale in 0u32..=100,
            slot_index in 0u16..=u16::MAX,
        ) {
            let plan = DecimalAssignmentPlan {
                semantic_origin: "test.literal@1".into(),
                policy: DecimalExecutionPolicy::decimal18_v1(),
                assignments: vec![assignment(
                    usize::from(slot_index),
                    DecimalRoundingPolicy::NearestEven,
                    DecimalExpression::Literal { coefficient, scale },
                )],
            };
            let limits = DecimalPlanLimits::default();
            let encoded = encode_decimal_assignment_plan(&plan, limits).unwrap();
            prop_assert_eq!(decode_decimal_assignment_plan(&encoded, limits), Ok(plan));
        }

        #[test]
        fn arbitrary_bounded_bytes_never_panic(
            bytes in proptest::collection::vec(any::<u8>(), 0..4096),
        ) {
            let limits = DecimalPlanLimits {
                max_encoded_bytes: 4096,
                ..DecimalPlanLimits::default()
            };
            let _ = decode_decimal_assignment_plan(&bytes, limits);
        }
    }
}
