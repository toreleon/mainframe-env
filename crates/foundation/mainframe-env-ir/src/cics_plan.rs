//! Canonical executable plans for the typed CICS file/unit-of-work slice.

use crate::StorageId;
use std::collections::BTreeSet;
use std::fmt;

/// Stable wire identity for a typed CICS effect plan.
pub const CICS_EFFECT_PLAN_CONTRACT: &str = "mainframe-env.cics-effect-plan@1";

const MAGIC: &[u8; 4] = b"MCEP";
const VERSION: u16 = 1;

/// Resource limits for CICS effect-plan encoding and decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsPlanLimits {
    /// Maximum encoded bytes for one plan.
    pub max_encoded_bytes: usize,
    /// Maximum number of named input operands.
    pub max_operands: usize,
    /// Maximum number of flag options.
    pub max_options: usize,
    /// Maximum number of output bindings.
    pub max_outputs: usize,
    /// Maximum aggregate literal bytes.
    pub max_literal_bytes: usize,
    /// Maximum bytes in one qualified layout name.
    pub max_qualified_name_bytes: usize,
}

impl Default for CicsPlanLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 1024 * 1024,
            max_operands: 32,
            max_options: 16,
            max_outputs: 16,
            max_literal_bytes: 1024 * 1024,
            max_qualified_name_bytes: 1024,
        }
    }
}

/// CICS operation selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOperation {
    /// Change the issuing task's dispatch priority and optionally yield.
    ChangeTask,
    /// Release one task-owned enqueue.
    Deq,
    /// Acquire one task-owned enqueue.
    Enq,
    /// Read one file record.
    Read,
    /// Rewrite the record held by the current update context.
    Rewrite,
    /// Commit or roll back the current unit of work.
    Syncpoint,
    /// Overwrite the originating task's bounded user correlator data.
    SetAssociationUserCorrData,
    /// Yield the issuing task once for redispatch.
    Suspend,
}

/// A resolved storage slot in the containing IR module.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CicsStorageSlot {
    /// Stable storage arena identity.
    pub storage: StorageId,
    /// Canonical qualified COBOL layout name used for cross-checking.
    pub qualified_layout_name: String,
}

/// Named input accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOperandName {
    /// `FILE(...)` resource binding.
    File,
    /// `DATASET(...)` resource alias.
    Dataset,
    /// `FROM(...)` record bytes.
    From,
    /// `RIDFLD(...)` record identifier.
    Ridfld,
    /// `RESOURCE(...)` enqueue identity.
    Resource,
    /// `LENGTH(...)` content-identity length.
    Length,
    /// `MAXLIFETIME(...)` dynamic CVDA value.
    MaxLifetime,
    /// `PRIORITY(...)` task dispatch value.
    Priority,
    /// `USERCORRDATA(...)` task association value.
    UserCorrData,
}

/// Literal bytes or a runtime read from resolved storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CicsOperandValue {
    /// Exact literal bytes after source delimiters are removed.
    Literal(Vec<u8>),
    /// Runtime bytes from a resolved storage slot.
    Storage(CicsStorageSlot),
    /// Exact signed integer value resolved by the frontend.
    Integer(i64),
}

/// One typed named input operand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsNamedOperand {
    /// Semantic operand name.
    pub name: CicsOperandName,
    /// Resolved literal or storage value.
    pub value: CicsOperandValue,
}

/// Flag option accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOption {
    /// Establish a read-for-update context.
    Update,
    /// Roll back rather than commit at syncpoint.
    Rollback,
    /// Suppress default condition handling.
    NoHandle,
    /// Keep an enqueue until task termination.
    Task,
    /// Keep an enqueue until the current unit of work ends.
    Uow,
    /// Return `ENQBUSY` rather than suspending for a contended resource.
    NoSuspend,
}

/// Named result binding written after the host result arrives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOutputName {
    /// Record payload destination.
    Into,
    /// Primary response code destination.
    Resp,
    /// Secondary response code destination.
    Resp2,
}

/// One pre-resolved result binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsOutputBinding {
    /// Semantic output name.
    pub name: CicsOutputName,
    /// Resolved receiving storage.
    pub target: CicsStorageSlot,
}

/// CICS condition policy selected by the frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CicsCondition {
    /// Use normal condition propagation.
    Default,
    /// Suppress normal condition propagation.
    NoHandle,
    /// Return response codes through resolved storage.
    Respond {
        /// Primary response-code receiver.
        response: CicsStorageSlot,
        /// Optional secondary response-code receiver.
        response2: Option<CicsStorageSlot>,
    },
}

/// A typed CICS request plan with no source-statement grammar remaining.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsEffectPlan {
    /// Exact operation carried by the executable dialect identity.
    pub operation: CicsPlanOperation,
    /// Named inputs; encoding canonicalizes their order.
    pub operands: Vec<CicsNamedOperand>,
    /// Unique flag options.
    pub options: BTreeSet<CicsPlanOption>,
    /// Named outputs; encoding canonicalizes their order.
    pub outputs: Vec<CicsOutputBinding>,
    /// Explicit condition behavior.
    pub condition: CicsCondition,
}

/// Encode one validated CICS effect plan in canonical form.
pub fn encode_cics_effect_plan(
    plan: &CicsEffectPlan,
    limits: CicsPlanLimits,
) -> Result<Vec<u8>, CicsPlanCodecProblem> {
    validate_plan(plan, limits)?;
    let mut writer = Writer::new(limits.max_encoded_bytes);
    writer.extend(MAGIC)?;
    writer.u16(VERSION)?;
    writer.byte(operation_tag(plan.operation))?;

    let mut operands = plan.operands.iter().collect::<Vec<_>>();
    operands.sort_by_key(|operand| operand.name);
    writer.count(operands.len())?;
    for operand in operands {
        writer.byte(operand_tag(operand.name))?;
        match &operand.value {
            CicsOperandValue::Literal(bytes) => {
                writer.byte(0)?;
                writer.bytes(bytes, limits.max_literal_bytes)?;
            }
            CicsOperandValue::Storage(slot) => {
                writer.byte(1)?;
                encode_slot(&mut writer, slot, limits)?;
            }
            CicsOperandValue::Integer(value) => {
                writer.byte(2)?;
                writer.i64(*value)?;
            }
        }
    }

    writer.count(plan.options.len())?;
    for option in &plan.options {
        writer.byte(option_tag(*option))?;
    }

    let mut outputs = plan.outputs.iter().collect::<Vec<_>>();
    outputs.sort_by_key(|output| output.name);
    writer.count(outputs.len())?;
    for output in outputs {
        writer.byte(output_tag(output.name))?;
        encode_slot(&mut writer, &output.target, limits)?;
    }
    encode_condition(&mut writer, &plan.condition, limits)?;
    Ok(writer.finish())
}

/// Decode and validate one canonical CICS effect plan.
pub fn decode_cics_effect_plan(
    bytes: &[u8],
    limits: CicsPlanLimits,
) -> Result<CicsEffectPlan, CicsPlanCodecProblem> {
    if bytes.len() > limits.max_encoded_bytes {
        return Err(CicsPlanCodecProblem::LimitExceeded);
    }
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(CicsPlanCodecProblem::BadMagic);
    }
    if reader.u16()? != VERSION {
        return Err(CicsPlanCodecProblem::UnsupportedVersion);
    }
    let operation = operation_from_tag(reader.byte()?)?;

    let operand_count = reader.count(limits.max_operands)?;
    let mut operands = Vec::with_capacity(operand_count);
    let mut last_operand = None;
    let mut literal_bytes = 0usize;
    for _ in 0..operand_count {
        let name = operand_from_tag(reader.byte()?)?;
        require_order(last_operand, name)?;
        last_operand = Some(name);
        let value = match reader.byte()? {
            0 => {
                let bytes = reader.bytes(limits.max_literal_bytes)?;
                literal_bytes = literal_bytes
                    .checked_add(bytes.len())
                    .ok_or(CicsPlanCodecProblem::LimitExceeded)?;
                if literal_bytes > limits.max_literal_bytes {
                    return Err(CicsPlanCodecProblem::LimitExceeded);
                }
                CicsOperandValue::Literal(bytes)
            }
            1 => CicsOperandValue::Storage(decode_slot(&mut reader, limits)?),
            2 => CicsOperandValue::Integer(reader.i64()?),
            _ => return Err(CicsPlanCodecProblem::Malformed),
        };
        operands.push(CicsNamedOperand { name, value });
    }

    let option_count = reader.count(limits.max_options)?;
    let mut options = BTreeSet::new();
    let mut last_option = None;
    for _ in 0..option_count {
        let option = option_from_tag(reader.byte()?)?;
        require_order(last_option, option)?;
        last_option = Some(option);
        if !options.insert(option) {
            return Err(CicsPlanCodecProblem::Malformed);
        }
    }

    let output_count = reader.count(limits.max_outputs)?;
    let mut outputs = Vec::with_capacity(output_count);
    let mut last_output = None;
    for _ in 0..output_count {
        let name = output_from_tag(reader.byte()?)?;
        require_order(last_output, name)?;
        last_output = Some(name);
        outputs.push(CicsOutputBinding {
            name,
            target: decode_slot(&mut reader, limits)?,
        });
    }
    let condition = decode_condition(&mut reader, limits)?;
    if !reader.remaining().is_empty() {
        return Err(CicsPlanCodecProblem::TrailingData);
    }
    let plan = CicsEffectPlan {
        operation,
        operands,
        options,
        outputs,
        condition,
    };
    validate_plan(&plan, limits)?;
    if encode_cics_effect_plan(&plan, limits)? != bytes {
        return Err(CicsPlanCodecProblem::NonCanonical);
    }
    Ok(plan)
}

fn validate_plan(
    plan: &CicsEffectPlan,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    bounded_count(plan.operands.len(), limits.max_operands)?;
    bounded_count(plan.options.len(), limits.max_options)?;
    bounded_count(plan.outputs.len(), limits.max_outputs)?;
    let mut operand_names = BTreeSet::new();
    let mut total_literal_bytes = 0usize;
    for operand in &plan.operands {
        if !operand_names.insert(operand.name) {
            return Err(CicsPlanCodecProblem::Malformed);
        }
        match &operand.value {
            CicsOperandValue::Literal(bytes) => {
                total_literal_bytes = total_literal_bytes
                    .checked_add(bytes.len())
                    .ok_or(CicsPlanCodecProblem::LimitExceeded)?;
                if total_literal_bytes > limits.max_literal_bytes
                    || u32::try_from(bytes.len()).is_err()
                {
                    return Err(CicsPlanCodecProblem::LimitExceeded);
                }
            }
            CicsOperandValue::Storage(slot) => validate_slot(slot, limits)?,
            CicsOperandValue::Integer(_) => {}
        }
    }
    if matches!(
        plan.operation,
        CicsPlanOperation::Deq | CicsPlanOperation::Enq
    ) {
        let resource = plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::Resource)
            .ok_or(CicsPlanCodecProblem::Malformed)?;
        let content_mode = plan
            .operands
            .iter()
            .any(|operand| operand.name == CicsOperandName::Length);
        if (!content_mode && !matches!(resource.value, CicsOperandValue::Storage(_)))
            || plan.operands.iter().any(|operand| {
                matches!(
                    operand.name,
                    CicsOperandName::Length | CicsOperandName::MaxLifetime
                ) && matches!(operand.value, CicsOperandValue::Literal(_))
            })
        {
            return Err(CicsPlanCodecProblem::Malformed);
        }
    }
    if plan.operation == CicsPlanOperation::ChangeTask
        && plan.operands.iter().any(|operand| {
            operand.name == CicsOperandName::Priority
                && matches!(operand.value, CicsOperandValue::Literal(_))
        })
    {
        return Err(CicsPlanCodecProblem::Malformed);
    }
    let mut output_names = BTreeSet::new();
    for output in &plan.outputs {
        if !output_names.insert(output.name) {
            return Err(CicsPlanCodecProblem::Malformed);
        }
        validate_slot(&output.target, limits)?;
    }
    validate_operation_shape(plan, &operand_names, &output_names)?;
    validate_condition(plan, limits)
}

fn validate_operation_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> Result<(), CicsPlanCodecProblem> {
    let resources = usize::from(inputs.contains(&CicsOperandName::File))
        + usize::from(inputs.contains(&CicsOperandName::Dataset));
    let enqueue_lifetimes = usize::from(inputs.contains(&CicsOperandName::MaxLifetime))
        + usize::from(plan.options.contains(&CicsPlanOption::Task))
        + usize::from(plan.options.contains(&CicsPlanOption::Uow));
    let enqueue_inputs = [
        CicsOperandName::Resource,
        CicsOperandName::Length,
        CicsOperandName::MaxLifetime,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let scheduling_options = plan
        .options
        .iter()
        .any(|option| !matches!(option, CicsPlanOption::NoHandle));
    let malformed = match plan.operation {
        CicsPlanOperation::ChangeTask => {
            !inputs.is_subset(&BTreeSet::from([CicsOperandName::Priority]))
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Deq | CicsPlanOperation::Enq => {
            !inputs.contains(&CicsOperandName::Resource)
                || !inputs.is_subset(&enqueue_inputs)
                || enqueue_lifetimes > 1
                || plan.options.contains(&CicsPlanOption::Update)
                || plan.options.contains(&CicsPlanOption::Rollback)
                || (plan.operation == CicsPlanOperation::Deq
                    && plan.options.contains(&CicsPlanOption::NoSuspend))
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Read => {
            resources != 1
                || !inputs.contains(&CicsOperandName::Ridfld)
                || inputs.contains(&CicsOperandName::From)
                || !outputs.contains(&CicsOutputName::Into)
                || plan.options.contains(&CicsPlanOption::Rollback)
                || plan.options.contains(&CicsPlanOption::Task)
                || plan.options.contains(&CicsPlanOption::Uow)
                || plan.options.contains(&CicsPlanOption::NoSuspend)
        }
        CicsPlanOperation::Rewrite => {
            resources != 1
                || !inputs.contains(&CicsOperandName::From)
                || inputs.contains(&CicsOperandName::Ridfld)
                || plan.options.contains(&CicsPlanOption::Update)
                || plan.options.contains(&CicsPlanOption::Rollback)
                || plan.options.contains(&CicsPlanOption::Task)
                || plan.options.contains(&CicsPlanOption::Uow)
                || plan.options.contains(&CicsPlanOption::NoSuspend)
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Syncpoint => {
            !inputs.is_empty()
                || plan.options.contains(&CicsPlanOption::Update)
                || plan.options.contains(&CicsPlanOption::Task)
                || plan.options.contains(&CicsPlanOption::Uow)
                || plan.options.contains(&CicsPlanOption::NoSuspend)
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::SetAssociationUserCorrData => {
            (inputs.len() != 1 || !inputs.contains(&CicsOperandName::UserCorrData))
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Suspend => {
            !inputs.is_empty() || scheduling_options || outputs.contains(&CicsOutputName::Into)
        }
    };
    if malformed
        || (outputs.contains(&CicsOutputName::Resp2) && !outputs.contains(&CicsOutputName::Resp))
    {
        Err(CicsPlanCodecProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_condition(
    plan: &CicsEffectPlan,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    let response = output_target(&plan.outputs, CicsOutputName::Resp);
    let response2 = output_target(&plan.outputs, CicsOutputName::Resp2);
    let no_handle = plan.options.contains(&CicsPlanOption::NoHandle);
    match &plan.condition {
        CicsCondition::Default if !no_handle && response.is_none() => Ok(()),
        CicsCondition::NoHandle if no_handle => Ok(()),
        CicsCondition::Respond {
            response: expected,
            response2: expected2,
        } if !no_handle && response == Some(expected) && response2 == expected2.as_ref() => {
            validate_slot(expected, limits)?;
            if let Some(expected2) = expected2 {
                validate_slot(expected2, limits)?;
            }
            Ok(())
        }
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

fn output_target(outputs: &[CicsOutputBinding], name: CicsOutputName) -> Option<&CicsStorageSlot> {
    outputs
        .iter()
        .find(|output| output.name == name)
        .map(|output| &output.target)
}

fn validate_slot(
    slot: &CicsStorageSlot,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    let name = slot.qualified_layout_name.as_str();
    if name.len() > limits.max_qualified_name_bytes || name.len() > usize::from(u16::MAX) {
        return Err(CicsPlanCodecProblem::LimitExceeded);
    }
    if name.is_empty()
        || name.bytes().any(|byte| byte.is_ascii_lowercase())
        || name.split('.').any(str::is_empty)
        || !name.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@' | b'.')
        })
    {
        return Err(CicsPlanCodecProblem::NonCanonical);
    }
    Ok(())
}

fn bounded_count(value: usize, maximum: usize) -> Result<(), CicsPlanCodecProblem> {
    if value > maximum || u32::try_from(value).is_err() {
        Err(CicsPlanCodecProblem::LimitExceeded)
    } else {
        Ok(())
    }
}

fn encode_slot(
    writer: &mut Writer,
    slot: &CicsStorageSlot,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    validate_slot(slot, limits)?;
    writer.u32(slot.storage.get())?;
    writer.string(&slot.qualified_layout_name, limits.max_qualified_name_bytes)
}

fn decode_slot(
    reader: &mut Reader<'_>,
    limits: CicsPlanLimits,
) -> Result<CicsStorageSlot, CicsPlanCodecProblem> {
    let storage = StorageId::from_index(reader.u32()? as usize)
        .map_err(|_| CicsPlanCodecProblem::Malformed)?;
    let slot = CicsStorageSlot {
        storage,
        qualified_layout_name: reader.string(limits.max_qualified_name_bytes)?,
    };
    validate_slot(&slot, limits)?;
    Ok(slot)
}

fn encode_condition(
    writer: &mut Writer,
    condition: &CicsCondition,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    match condition {
        CicsCondition::Default => writer.byte(0),
        CicsCondition::NoHandle => writer.byte(1),
        CicsCondition::Respond {
            response,
            response2,
        } => {
            writer.byte(2)?;
            encode_slot(writer, response, limits)?;
            writer.byte(u8::from(response2.is_some()))?;
            if let Some(response2) = response2 {
                encode_slot(writer, response2, limits)?;
            }
            Ok(())
        }
    }
}

fn decode_condition(
    reader: &mut Reader<'_>,
    limits: CicsPlanLimits,
) -> Result<CicsCondition, CicsPlanCodecProblem> {
    Ok(match reader.byte()? {
        0 => CicsCondition::Default,
        1 => CicsCondition::NoHandle,
        2 => {
            let response = decode_slot(reader, limits)?;
            let response2 = match reader.byte()? {
                0 => None,
                1 => Some(decode_slot(reader, limits)?),
                _ => return Err(CicsPlanCodecProblem::Malformed),
            };
            CicsCondition::Respond {
                response,
                response2,
            }
        }
        _ => return Err(CicsPlanCodecProblem::Malformed),
    })
}

fn require_order<T: Copy + Ord>(
    previous: Option<T>,
    current: T,
) -> Result<(), CicsPlanCodecProblem> {
    match previous {
        Some(previous) if previous == current => Err(CicsPlanCodecProblem::Malformed),
        Some(previous) if previous > current => Err(CicsPlanCodecProblem::NonCanonical),
        _ => Ok(()),
    }
}

const fn operation_tag(value: CicsPlanOperation) -> u8 {
    match value {
        CicsPlanOperation::Read => 0,
        CicsPlanOperation::Rewrite => 1,
        CicsPlanOperation::Syncpoint => 2,
        CicsPlanOperation::Deq => 3,
        CicsPlanOperation::Enq => 4,
        CicsPlanOperation::ChangeTask => 5,
        CicsPlanOperation::Suspend => 6,
        CicsPlanOperation::SetAssociationUserCorrData => 7,
    }
}

fn operation_from_tag(value: u8) -> Result<CicsPlanOperation, CicsPlanCodecProblem> {
    match value {
        0 => Ok(CicsPlanOperation::Read),
        1 => Ok(CicsPlanOperation::Rewrite),
        2 => Ok(CicsPlanOperation::Syncpoint),
        3 => Ok(CicsPlanOperation::Deq),
        4 => Ok(CicsPlanOperation::Enq),
        5 => Ok(CicsPlanOperation::ChangeTask),
        6 => Ok(CicsPlanOperation::Suspend),
        7 => Ok(CicsPlanOperation::SetAssociationUserCorrData),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

const fn operand_tag(value: CicsOperandName) -> u8 {
    match value {
        CicsOperandName::File => 0,
        CicsOperandName::Dataset => 1,
        CicsOperandName::From => 2,
        CicsOperandName::Ridfld => 3,
        CicsOperandName::Resource => 4,
        CicsOperandName::Length => 5,
        CicsOperandName::MaxLifetime => 6,
        CicsOperandName::Priority => 7,
        CicsOperandName::UserCorrData => 8,
    }
}

fn operand_from_tag(value: u8) -> Result<CicsOperandName, CicsPlanCodecProblem> {
    match value {
        0 => Ok(CicsOperandName::File),
        1 => Ok(CicsOperandName::Dataset),
        2 => Ok(CicsOperandName::From),
        3 => Ok(CicsOperandName::Ridfld),
        4 => Ok(CicsOperandName::Resource),
        5 => Ok(CicsOperandName::Length),
        6 => Ok(CicsOperandName::MaxLifetime),
        7 => Ok(CicsOperandName::Priority),
        8 => Ok(CicsOperandName::UserCorrData),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

const fn option_tag(value: CicsPlanOption) -> u8 {
    match value {
        CicsPlanOption::Update => 0,
        CicsPlanOption::Rollback => 1,
        CicsPlanOption::NoHandle => 2,
        CicsPlanOption::Task => 3,
        CicsPlanOption::Uow => 4,
        CicsPlanOption::NoSuspend => 5,
    }
}

fn option_from_tag(value: u8) -> Result<CicsPlanOption, CicsPlanCodecProblem> {
    match value {
        0 => Ok(CicsPlanOption::Update),
        1 => Ok(CicsPlanOption::Rollback),
        2 => Ok(CicsPlanOption::NoHandle),
        3 => Ok(CicsPlanOption::Task),
        4 => Ok(CicsPlanOption::Uow),
        5 => Ok(CicsPlanOption::NoSuspend),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

const fn output_tag(value: CicsOutputName) -> u8 {
    match value {
        CicsOutputName::Into => 0,
        CicsOutputName::Resp => 1,
        CicsOutputName::Resp2 => 2,
    }
}

fn output_from_tag(value: u8) -> Result<CicsOutputName, CicsPlanCodecProblem> {
    match value {
        0 => Ok(CicsOutputName::Into),
        1 => Ok(CicsOutputName::Resp),
        2 => Ok(CicsOutputName::Resp2),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

struct Writer {
    bytes: Vec<u8>,
    maximum: usize,
}

impl Writer {
    fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
        }
    }
    fn finish(self) -> Vec<u8> {
        self.bytes
    }
    fn byte(&mut self, value: u8) -> Result<(), CicsPlanCodecProblem> {
        self.extend(&[value])
    }
    fn u16(&mut self, value: u16) -> Result<(), CicsPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }
    fn u32(&mut self, value: u32) -> Result<(), CicsPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }
    fn i64(&mut self, value: i64) -> Result<(), CicsPlanCodecProblem> {
        self.extend(&value.to_be_bytes())
    }
    fn count(&mut self, value: usize) -> Result<(), CicsPlanCodecProblem> {
        self.u32(u32::try_from(value).map_err(|_| CicsPlanCodecProblem::LimitExceeded)?)
    }
    fn string(&mut self, value: &str, maximum: usize) -> Result<(), CicsPlanCodecProblem> {
        if value.len() > maximum {
            return Err(CicsPlanCodecProblem::LimitExceeded);
        }
        self.u16(u16::try_from(value.len()).map_err(|_| CicsPlanCodecProblem::LimitExceeded)?)?;
        self.extend(value.as_bytes())
    }
    fn bytes(&mut self, value: &[u8], maximum: usize) -> Result<(), CicsPlanCodecProblem> {
        if value.len() > maximum {
            return Err(CicsPlanCodecProblem::LimitExceeded);
        }
        self.count(value.len())?;
        self.extend(value)
    }
    fn extend(&mut self, value: &[u8]) -> Result<(), CicsPlanCodecProblem> {
        let next = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or(CicsPlanCodecProblem::LimitExceeded)?;
        if next > self.maximum {
            return Err(CicsPlanCodecProblem::LimitExceeded);
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
    fn take(&mut self, length: usize) -> Result<&'a [u8], CicsPlanCodecProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CicsPlanCodecProblem::LimitExceeded)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(CicsPlanCodecProblem::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, CicsPlanCodecProblem> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, CicsPlanCodecProblem> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| CicsPlanCodecProblem::Truncated)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, CicsPlanCodecProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| CicsPlanCodecProblem::Truncated)?,
        ))
    }
    fn i64(&mut self) -> Result<i64, CicsPlanCodecProblem> {
        Ok(i64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| CicsPlanCodecProblem::Truncated)?,
        ))
    }
    fn count(&mut self, maximum: usize) -> Result<usize, CicsPlanCodecProblem> {
        let count =
            usize::try_from(self.u32()?).map_err(|_| CicsPlanCodecProblem::LimitExceeded)?;
        if count > maximum || count > self.remaining().len() {
            Err(CicsPlanCodecProblem::LimitExceeded)
        } else {
            Ok(count)
        }
    }
    fn string(&mut self, maximum: usize) -> Result<String, CicsPlanCodecProblem> {
        let length = usize::from(self.u16()?);
        if length > maximum {
            return Err(CicsPlanCodecProblem::LimitExceeded);
        }
        std::str::from_utf8(self.take(length)?)
            .map(str::to_owned)
            .map_err(|_| CicsPlanCodecProblem::Malformed)
    }
    fn bytes(&mut self, maximum: usize) -> Result<Vec<u8>, CicsPlanCodecProblem> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| CicsPlanCodecProblem::LimitExceeded)?;
        if length > maximum {
            return Err(CicsPlanCodecProblem::LimitExceeded);
        }
        Ok(self.take(length)?.to_vec())
    }
}

/// Failure returned by the CICS effect-plan codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsPlanCodecProblem {
    /// The plan magic is absent.
    BadMagic,
    /// The encoded plan uses an unsupported version.
    UnsupportedVersion,
    /// The input ends before a declared field is complete.
    Truncated,
    /// Bytes remain after the complete plan.
    TrailingData,
    /// A tag, duplicate, or operation shape is invalid.
    Malformed,
    /// Text or field ordering is not canonical.
    NonCanonical,
    /// A configured resource limit was exceeded.
    LimitExceeded,
}

impl fmt::Display for CicsPlanCodecProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CICS effect plan codec failed: {self:?}")
    }
}

impl std::error::Error for CicsPlanCodecProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn slot(index: usize, name: &str) -> CicsStorageSlot {
        CicsStorageSlot {
            storage: StorageId::from_index(index).unwrap(),
            qualified_layout_name: name.into(),
        }
    }

    fn read_plan() -> CicsEffectPlan {
        let response = slot(2, "RESULT.RESP");
        let response2 = slot(3, "RESULT.RESP2");
        CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Literal(b"003".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Update]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Resp2,
                    target: response2.clone(),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Into,
                    target: slot(1, "RESULT.RECORD"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Resp,
                    target: response.clone(),
                },
            ],
            condition: CicsCondition::Respond {
                response,
                response2: Some(response2),
            },
        }
    }

    #[test]
    fn all_pilot_operations_round_trip_and_literal_bytes_are_exact() {
        let read = read_plan();
        let rewrite = CicsEffectPlan {
            operation: CicsPlanOperation::Rewrite,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Dataset,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(1, "REQUEST.RECORD")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let syncpoint = CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::Rollback, CicsPlanOption::NoHandle]),
            outputs: Vec::new(),
            condition: CicsCondition::NoHandle,
        };
        for plan in [read, rewrite, syncpoint] {
            let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
            let decoded = decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap();
            assert_eq!(decoded.operation, plan.operation);
            assert_eq!(decoded.options, plan.options);
            assert_eq!(decoded.condition, plan.condition);
            assert_eq!(
                encode_cics_effect_plan(&decoded, CicsPlanLimits::default()).unwrap(),
                encoded
            );
        }
        let decoded = decode_cics_effect_plan(
            &encode_cics_effect_plan(&read_plan(), CicsPlanLimits::default()).unwrap(),
            CicsPlanLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            decoded.operands[1].value,
            CicsOperandValue::Literal(ref bytes) if bytes == b"003"
        ));
    }

    #[test]
    fn enqueue_plans_round_trip_integer_lengths_and_lifetime_exclusion() {
        let enq = CicsEffectPlan {
            operation: CicsPlanOperation::Enq,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Resource,
                    value: CicsOperandValue::Storage(slot(1, "REQUEST.LOCK-NAME")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(9),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Uow, CicsPlanOption::NoSuspend]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&enq, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()).unwrap(),
            enq
        );

        let mut conflicting = enq.clone();
        conflicting.options.insert(CicsPlanOption::Task);
        assert_eq!(
            encode_cics_effect_plan(&conflicting, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid_deq = enq;
        invalid_deq.operation = CicsPlanOperation::Deq;
        assert_eq!(
            encode_cics_effect_plan(&invalid_deq, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn task_scheduling_plans_round_trip_and_reject_cross_command_operands() {
        let change = CicsEffectPlan {
            operation: CicsPlanOperation::ChangeTask,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Priority,
                value: CicsOperandValue::Integer(200),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&change, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            change
        );
        let mut suspend = change;
        suspend.operation = CicsPlanOperation::Suspend;
        assert_eq!(
            encode_cics_effect_plan(&suspend, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        suspend.operands.clear();
        assert_eq!(
            decode_cics_effect_plan(
                &encode_cics_effect_plan(&suspend, CicsPlanLimits::default()).unwrap(),
                CicsPlanLimits::default(),
            )
            .unwrap(),
            suspend
        );
    }

    #[test]
    fn task_association_requires_one_bounded_value_operand() {
        let association = CicsEffectPlan {
            operation: CicsPlanOperation::SetAssociationUserCorrData,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::UserCorrData,
                value: CicsOperandValue::Literal(vec![b'A'; 80]),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&association, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            association
        );
        let mut missing = association;
        missing.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn shape_duplicates_versions_and_trailing_data_fail_closed() {
        let limits = CicsPlanLimits::default();
        let mut duplicate = read_plan();
        duplicate.operands.push(duplicate.operands[0].clone());
        assert_eq!(
            encode_cics_effect_plan(&duplicate, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut conflicting = read_plan();
        conflicting.operands.push(CicsNamedOperand {
            name: CicsOperandName::Dataset,
            value: CicsOperandValue::Literal(b"OTHER".to_vec()),
        });
        assert_eq!(
            encode_cics_effect_plan(&conflicting, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut bytes = encode_cics_effect_plan(&read_plan(), limits).unwrap();
        bytes[4..6].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&bytes, limits),
            Err(CicsPlanCodecProblem::UnsupportedVersion)
        );
        let mut trailing = encode_cics_effect_plan(&read_plan(), limits).unwrap();
        trailing.push(0);
        assert_eq!(
            decode_cics_effect_plan(&trailing, limits),
            Err(CicsPlanCodecProblem::TrailingData)
        );
    }

    #[test]
    fn response_policy_and_operation_requirements_are_checked() {
        let limits = CicsPlanLimits::default();
        let mut missing_key = read_plan();
        missing_key
            .operands
            .retain(|operand| operand.name != CicsOperandName::Ridfld);
        assert!(encode_cics_effect_plan(&missing_key, limits).is_err());
        let mut missing_into = read_plan();
        missing_into
            .outputs
            .retain(|output| output.name != CicsOutputName::Into);
        assert!(encode_cics_effect_plan(&missing_into, limits).is_err());
        let mut bad_policy = read_plan();
        bad_policy.condition = CicsCondition::Default;
        assert!(encode_cics_effect_plan(&bad_policy, limits).is_err());
        let mut bad_response = read_plan();
        bad_response
            .outputs
            .retain(|output| output.name != CicsOutputName::Resp);
        assert!(encode_cics_effect_plan(&bad_response, limits).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = decode_cics_effect_plan(&bytes, CicsPlanLimits::default());
        }
    }
}
