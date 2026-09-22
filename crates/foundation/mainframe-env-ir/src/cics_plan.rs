//! Canonical executable plans for the typed CICS file/unit-of-work slice.

use crate::StorageId;
use std::collections::BTreeSet;
use std::fmt;

mod assign;
mod browse;
mod codec_tags;
mod file_mutation;
mod handle_abend;
mod identities;
mod interval_control;
mod option_shape;
mod output_shape;
mod program_control;
mod queue_control;
mod storage_control;
mod terminal_control;

pub use assign::{CICS_ASSIGN_OUTPUT_NAMES, CicsAssignOutput};
pub use identities::{CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption};

use codec_tags::{
    operand_from_tag, operand_tag, operation_from_tag, operation_tag, option_from_tag, option_tag,
    output_from_tag, output_tag,
};

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

/// A resolved storage slot in the containing IR module.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CicsStorageSlot {
    /// Stable storage arena identity.
    pub storage: StorageId,
    /// Canonical qualified COBOL layout name used for cross-checking.
    pub qualified_layout_name: String,
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
    /// Runtime byte length of a resolved storage slot.
    LengthOf(CicsStorageSlot),
}

/// One typed named input operand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsNamedOperand {
    /// Semantic operand name.
    pub name: CicsOperandName,
    /// Resolved literal or storage value.
    pub value: CicsOperandValue,
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
            CicsOperandValue::LengthOf(slot) => {
                writer.byte(3)?;
                encode_slot(&mut writer, slot, limits)?;
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
            3 => CicsOperandValue::LengthOf(decode_slot(&mut reader, limits)?),
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
        let numeric_length = matches!(
            operand.name,
            CicsOperandName::Length | CicsOperandName::KeyLength
        );
        if (numeric_length && matches!(&operand.value, CicsOperandValue::Literal(_)))
            || (!numeric_length && matches!(&operand.value, CicsOperandValue::LengthOf(_)))
        {
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
            CicsOperandValue::Storage(slot) | CicsOperandValue::LengthOf(slot) => {
                validate_slot(slot, limits)?
            }
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
    let scheduling_options = option_shape::has_unsupported(plan);
    let unexpected_output = outputs
        .iter()
        .any(|output| !output_shape::allowed(plan.operation, *output));
    let malformed = match plan.operation {
        CicsPlanOperation::Abend => handle_abend::invalid_abend_shape(plan, inputs, outputs),
        CicsPlanOperation::AddressSet => {
            let pointer_from_data = inputs.len() == 2
                && inputs.contains(&CicsOperandName::SetPointer)
                && inputs.contains(&CicsOperandName::UsingAddress);
            let data_from_pointer = inputs.len() == 2
                && inputs.contains(&CicsOperandName::SetAddress)
                && inputs.contains(&CicsOperandName::UsingPointer);
            (!pointer_from_data && !data_from_pointer)
                || plan
                    .operands
                    .iter()
                    .any(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::AsktimeEib => {
            !inputs.is_empty() || scheduling_options || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Asktime => {
            !inputs.is_empty()
                || scheduling_options
                || !outputs.contains(&CicsOutputName::Abstime)
        }
        CicsPlanOperation::FormatTime => {
            let allowed_inputs = BTreeSet::from([
                CicsOperandName::Abstime,
                CicsOperandName::DateSep,
                CicsOperandName::TimeSep,
            ]);
            !inputs.contains(&CicsOperandName::Abstime)
                || !inputs.is_subset(&allowed_inputs)
                || plan.operands.iter().any(|operand| match operand.name {
                    CicsOperandName::Abstime => {
                        !matches!(operand.value, CicsOperandValue::Storage(_))
                    }
                    CicsOperandName::DateSep | CicsOperandName::TimeSep => !matches!(
                        operand.value,
                        CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                    ),
                    _ => true,
                })
                || scheduling_options
        }
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
        CicsPlanOperation::PopHandle | CicsPlanOperation::PushHandle => {
            !inputs.is_empty() || scheduling_options || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::HandleCondition => {
            inputs.len() != 1
                || !inputs.contains(&CicsOperandName::Conditions)
                || plan.operands.iter().any(|operand| {
                    operand.name != CicsOperandName::Conditions
                        || !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_condition_handlers(bytes))
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::HandleAid => {
            inputs.len() != 1
                || !inputs.contains(&CicsOperandName::Aids)
                || plan.operands.iter().any(|operand| {
                    operand.name != CicsOperandName::Aids
                        || !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_aid_handlers(bytes))
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::HandleAbend => handle_abend::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::IgnoreCondition => {
            inputs.len() != 1
                || !inputs.contains(&CicsOperandName::Conditions)
                || plan.operands.iter().any(|operand| {
                    operand.name != CicsOperandName::Conditions
                        || !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_condition_list(bytes))
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Link => program_control::invalid_link_shape(plan, inputs, outputs),
        CicsPlanOperation::Xctl => program_control::invalid_xctl_shape(plan, inputs, outputs),
        CicsPlanOperation::Return => program_control::invalid_return_shape(plan, inputs, outputs),
        CicsPlanOperation::StartBrowse
        | CicsPlanOperation::ReadNext
        | CicsPlanOperation::ReadPrev
        | CicsPlanOperation::EndBrowse => browse::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::Read => {
            resources != 1
                || !inputs.is_subset(&BTreeSet::from([
                    CicsOperandName::File,
                    CicsOperandName::Dataset,
                    CicsOperandName::Ridfld,
                    CicsOperandName::Length,
                    CicsOperandName::KeyLength,
                ]))
                || !inputs.contains(&CicsOperandName::Ridfld)
                || inputs.contains(&CicsOperandName::From)
                || !outputs.contains(&CicsOutputName::Into)
                || (plan.options.contains(&CicsPlanOption::Generic)
                    && !inputs.contains(&CicsOperandName::KeyLength))
                || plan.operands.iter().any(|operand| {
                    operand.name == CicsOperandName::KeyLength
                        && match operand.value {
                            CicsOperandValue::Integer(0) => {
                                !plan.options.contains(&CicsPlanOption::Gteq)
                                    || plan.options.contains(&CicsPlanOption::Generic)
                            }
                            CicsOperandValue::Integer(1..=32_767)
                            | CicsOperandValue::Storage(_)
                            | CicsOperandValue::LengthOf(_) => false,
                            _ => true,
                        }
                })
                || match (
                    operand_value(plan, CicsOperandName::Ridfld),
                    operand_value(plan, CicsOperandName::KeyLength),
                ) {
                    (
                        Some(CicsOperandValue::Storage(ridfld)),
                        Some(CicsOperandValue::LengthOf(length)),
                    ) => ridfld != length,
                    (Some(_), Some(CicsOperandValue::LengthOf(_))) => true,
                    _ => false,
                }
                || plan.options.iter().any(|option| {
                    !matches!(
                        option,
                        CicsPlanOption::Generic
                            | CicsPlanOption::Gteq
                            | CicsPlanOption::NoHandle
                            | CicsPlanOption::Update
                    )
                })
                || !matches!(
                    operand_value(plan, CicsOperandName::Length),
                    None | Some(CicsOperandValue::Storage(_))
                )
                || outputs.contains(&CicsOutputName::Length)
                    != matches!(
                        operand_value(plan, CicsOperandName::Length),
                        Some(CicsOperandValue::Storage(_))
                    )
                || match operand_value(plan, CicsOperandName::Length) {
                    Some(CicsOperandValue::Storage(slot)) => {
                        output_target(&plan.outputs, CicsOutputName::Length) != Some(slot)
                    }
                    _ => false,
                }
        }
        CicsPlanOperation::Delete
        | CicsPlanOperation::Write
        | CicsPlanOperation::Rewrite => {
            file_mutation::invalid_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WriteTransientData => {
            queue_control::invalid_write_transient_data_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DeleteTransientData => {
            queue_control::invalid_delete_transient_data_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Getmain => storage_control::invalid_getmain_shape(plan, inputs, outputs),
        CicsPlanOperation::Freemain => {
            storage_control::invalid_freemain_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ReceiveMap
        | CicsPlanOperation::SendMap
        | CicsPlanOperation::SendText => terminal_control::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::Syncpoint => {
            !inputs.is_empty()
                || plan.options.iter().any(|option| {
                    !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Rollback)
                })
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
        CicsPlanOperation::Assign => {
            !inputs.is_empty()
                || scheduling_options
                || outputs
                    .iter()
                    .any(|output| {
                        !matches!(
                            output,
                            CicsOutputName::Assign(_)
                                | CicsOutputName::Resp
                                | CicsOutputName::Resp2
                        )
                    })
        }
        CicsPlanOperation::PurgeMessage => {
            !inputs.is_empty() || scheduling_options || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Cancel
        | CicsPlanOperation::Delay
        | CicsPlanOperation::Start
        | CicsPlanOperation::Retrieve => {
            interval_control::invalid_shape(plan, inputs, outputs, scheduling_options)
        }
    };
    if unexpected_output
        || malformed
        || (outputs.contains(&CicsOutputName::Resp2) && !outputs.contains(&CicsOutputName::Resp))
    {
        Err(CicsPlanCodecProblem::Malformed)
    } else {
        Ok(())
    }
}

pub(super) fn operand_value(
    plan: &CicsEffectPlan,
    name: CicsOperandName,
) -> Option<&CicsOperandValue> {
    plan.operands
        .iter()
        .find(|operand| operand.name == name)
        .map(|operand| &operand.value)
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

pub(super) fn output_target(
    outputs: &[CicsOutputBinding],
    name: CicsOutputName,
) -> Option<&CicsStorageSlot> {
    outputs
        .iter()
        .find(|output| output.name == name)
        .map(|output| &output.target)
}

fn valid_condition_list(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let names = text.split('\n').collect::<Vec<_>>();
    let unique = names.iter().copied().collect::<BTreeSet<_>>();
    matches!(names.len(), 1..=16)
        && unique.len() == names.len()
        && names.iter().all(|name| {
            crate::CICS_APPLICATION_CONDITION_NAMES
                .binary_search(name)
                .is_ok()
        })
}

fn valid_condition_handlers(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let entries = text
        .split('\n')
        .map(|entry| entry.split_once('\t'))
        .collect::<Option<Vec<_>>>();
    let Some(entries) = entries else {
        return false;
    };
    let unique = entries
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    matches!(entries.len(), 1..=16)
        && unique.len() == entries.len()
        && entries.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && entries.iter().all(|(name, label)| {
            crate::CICS_APPLICATION_CONDITION_NAMES
                .binary_search(name)
                .is_ok()
                && (label.is_empty()
                    || label.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'
                    }))
        })
}

fn valid_aid_handlers(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let entries = if text.is_empty() {
        Vec::new()
    } else {
        let entries = text
            .split('\n')
            .map(|entry| entry.split_once('\t'))
            .collect::<Option<Vec<_>>>();
        let Some(entries) = entries else {
            return false;
        };
        entries
    };
    let unique = entries
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    entries.len() <= 16
        && unique.len() == entries.len()
        && entries.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && entries.iter().all(|(name, label)| {
            crate::CICS_APPLICATION_AID_NAMES
                .binary_search(name)
                .is_ok()
                && (label.is_empty()
                    || label.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'
                    }))
        })
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
                    value: CicsOperandValue::Storage(slot(5, "REQUEST.KEY")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Storage(slot(4, "RESULT.LENGTH")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::KeyLength,
                    value: CicsOperandValue::LengthOf(slot(5, "REQUEST.KEY")),
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
                CicsOutputBinding {
                    name: CicsOutputName::Length,
                    target: slot(4, "RESULT.LENGTH"),
                },
            ],
            condition: CicsCondition::Respond {
                response,
                response2: Some(response2),
            },
        }
    }

    /// Issue #212: unrelated file and UOW plans reject extension flags.
    #[test]
    fn unrelated_operations_reject_cics_extension_flags() {
        let read = read_plan();
        let rewrite = CicsEffectPlan {
            operation: CicsPlanOperation::Rewrite,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
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
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut gteq_read = read.clone();
        gteq_read.options.insert(CicsPlanOption::Gteq);
        assert!(encode_cics_effect_plan(&gteq_read, CicsPlanLimits::default()).is_ok());
        for base in [read, rewrite, syncpoint] {
            assert!(encode_cics_effect_plan(&base, CicsPlanLimits::default()).is_ok());
            for option in [
                CicsPlanOption::Gteq,
                CicsPlanOption::Erase,
                CicsPlanOption::Cursor,
                CicsPlanOption::FreeKb,
                CicsPlanOption::DateSep,
                CicsPlanOption::TimeSep,
                CicsPlanOption::Wait,
                CicsPlanOption::MapOnly,
                CicsPlanOption::DataOnly,
            ] {
                if base.operation == CicsPlanOperation::Read && option == CicsPlanOption::Gteq {
                    continue;
                }
                let mut invalid = base.clone();
                invalid.options.insert(option);
                assert_eq!(
                    encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
                    Err(CicsPlanCodecProblem::Malformed),
                    "{:?} must reject {option:?}",
                    base.operation,
                );
            }
        }
    }

    /// Issues #202-#208: merged typed-CICS tags round-trip and unknown tags fail.
    #[test]
    fn carddemo_extension_codec_tags_round_trip_and_unknown_tags_fail() {
        for (option, tag) in [
            (CicsPlanOption::Erase, 9),
            (CicsPlanOption::Cursor, 10),
            (CicsPlanOption::DateSep, 11),
            (CicsPlanOption::TimeSep, 12),
            (CicsPlanOption::FreeKb, 13),
            (CicsPlanOption::Gteq, 14),
        ] {
            assert_eq!(option_tag(option), tag);
            assert_eq!(option_from_tag(tag), Ok(option));
        }
        assert_eq!(operand_tag(CicsOperandName::KeyLength), 26);
        assert_eq!(operand_from_tag(26), Ok(CicsOperandName::KeyLength));
        assert_eq!(operand_tag(CicsOperandName::UserId), 32);
        assert_eq!(operand_from_tag(32), Ok(CicsOperandName::UserId));
        assert_eq!(operand_tag(CicsOperandName::Hours), 33);
        assert_eq!(operand_from_tag(33), Ok(CicsOperandName::Hours));
        assert_eq!(operand_tag(CicsOperandName::Minutes), 34);
        assert_eq!(operand_from_tag(34), Ok(CicsOperandName::Minutes));
        assert_eq!(operand_tag(CicsOperandName::Seconds), 35);
        assert_eq!(operand_from_tag(35), Ok(CicsOperandName::Seconds));
        assert_eq!(operand_tag(CicsOperandName::Milliseconds), 36);
        assert_eq!(operand_from_tag(36), Ok(CicsOperandName::Milliseconds));
        assert_eq!(operand_tag(CicsOperandName::DataLength), 37);
        assert_eq!(operand_from_tag(37), Ok(CicsOperandName::DataLength));
        assert_eq!(output_tag(CicsOutputName::Length), 91);
        assert_eq!(output_from_tag(91), Ok(CicsOutputName::Length));
        assert_eq!(output_tag(CicsOutputName::SetPointer), 95);
        assert_eq!(output_from_tag(95), Ok(CicsOutputName::SetPointer));
        assert_eq!(option_tag(CicsPlanOption::Protect), 16);
        assert_eq!(option_from_tag(16), Ok(CicsPlanOption::Protect));
        assert_eq!(option_tag(CicsPlanOption::Wait), 17);
        assert_eq!(option_from_tag(17), Ok(CicsPlanOption::Wait));
        assert_eq!(option_tag(CicsPlanOption::After), 18);
        assert_eq!(option_from_tag(18), Ok(CicsPlanOption::After));
        assert_eq!(option_tag(CicsPlanOption::At), 19);
        assert_eq!(option_from_tag(19), Ok(CicsPlanOption::At));
        assert_eq!(option_tag(CicsPlanOption::For), 20);
        assert_eq!(option_from_tag(20), Ok(CicsPlanOption::For));
        assert_eq!(option_tag(CicsPlanOption::Until), 21);
        assert_eq!(option_from_tag(21), Ok(CicsPlanOption::Until));
        assert_eq!(option_tag(CicsPlanOption::NoCheck), 22);
        assert_eq!(option_from_tag(22), Ok(CicsPlanOption::NoCheck));
        assert_eq!(option_tag(CicsPlanOption::MapOnly), 23);
        assert_eq!(option_from_tag(23), Ok(CicsPlanOption::MapOnly));
        assert_eq!(option_tag(CicsPlanOption::DataOnly), 24);
        assert_eq!(option_from_tag(24), Ok(CicsPlanOption::DataOnly));
        assert_eq!(option_tag(CicsPlanOption::Generic), 25);
        assert_eq!(option_from_tag(25), Ok(CicsPlanOption::Generic));

        let plan = read_plan();
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        let decoded = decode_cics_effect_plan(&bytes, CicsPlanLimits::default()).unwrap();
        assert!(decoded.operands.iter().any(|operand| {
            operand.name == CicsOperandName::KeyLength
                && matches!(operand.value, CicsOperandValue::LengthOf(_))
        }));
        assert!(
            decoded
                .outputs
                .iter()
                .any(|output| output.name == CicsOutputName::Length)
        );

        assert_eq!(
            operation_from_tag(u8::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            operand_from_tag(u8::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            option_from_tag(u8::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            output_from_tag(u8::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut unknown_value_tag = bytes;
        unknown_value_tag[12] = u8::MAX;
        assert_eq!(
            decode_cics_effect_plan(&unknown_value_tag, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
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
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::LengthOf(slot(1, "REQUEST.RECORD")),
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
        let asktime_eib = CicsEffectPlan {
            operation: CicsPlanOperation::AsktimeEib,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let asktime = CicsEffectPlan {
            operation: CicsPlanOperation::Asktime,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Abstime,
                target: slot(4, "RESULT.ABSTIME"),
            }],
            condition: CicsCondition::Default,
        };
        let mut missing_absolute = asktime.clone();
        missing_absolute.outputs.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing_absolute, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut bare_with_absolute = asktime.clone();
        bare_with_absolute.operation = CicsPlanOperation::AsktimeEib;
        assert_eq!(
            encode_cics_effect_plan(&bare_with_absolute, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let format_time = CicsEffectPlan {
            operation: CicsPlanOperation::FormatTime,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Abstime,
                    value: CicsOperandValue::Storage(slot(5, "REQUEST.ABSTIME")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DateSep,
                    value: CicsOperandValue::Literal(b"-".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Milliseconds,
                    target: slot(6, "RESULT.MILLISECONDS"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Yyyymmdd,
                    target: slot(7, "RESULT.YYYYMMDD"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let abend = CicsEffectPlan {
            operation: CicsPlanOperation::Abend,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Abcode,
                value: CicsOperandValue::Literal(b"B001".to_vec()),
            }],
            options: BTreeSet::from([CicsPlanOption::Cancel, CicsPlanOption::NoDump]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut invalid_abend = abend.clone();
        invalid_abend.operands[0].value = CicsOperandValue::Literal(Vec::new());
        assert_eq!(
            encode_cics_effect_plan(&invalid_abend, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let handle_abend = CicsEffectPlan {
            operation: CicsPlanOperation::HandleAbend,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Program,
                value: CicsOperandValue::Literal(b"ABEXIT".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut conflicting_handle = handle_abend.clone();
        conflicting_handle.options.insert(CicsPlanOption::Reset);
        assert_eq!(
            encode_cics_effect_plan(&conflicting_handle, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let commarea = slot(8, "REQUEST.COMMAREA");
        let link = CicsEffectPlan {
            operation: CicsPlanOperation::Link,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Program,
                    value: CicsOperandValue::Literal(b"CHILD".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Commarea,
                    value: CicsOperandValue::Storage(commarea.clone()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::LengthOf(commarea.clone()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DataLength,
                    value: CicsOperandValue::Integer(1),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Commarea,
                target: commarea,
            }],
            condition: CicsCondition::Default,
        };
        let mut missing_link_output = link.clone();
        missing_link_output.outputs.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing_link_output, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut mismatched_link_length = link.clone();
        mismatched_link_length.operands[2].value =
            CicsOperandValue::LengthOf(slot(9, "REQUEST.OTHER"));
        assert_eq!(
            encode_cics_effect_plan(&mismatched_link_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut xctl = CicsEffectPlan {
            operation: CicsPlanOperation::Xctl,
            outputs: Vec::new(),
            ..link.clone()
        };
        xctl.operands
            .retain(|operand| operand.name != CicsOperandName::DataLength);
        let mut returning_xctl = xctl.clone();
        returning_xctl.outputs = link.outputs.clone();
        assert_eq!(
            encode_cics_effect_plan(&returning_xctl, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let return_area = slot(9, "REQUEST.RETURN-AREA");
        let return_plan = CicsEffectPlan {
            operation: CicsPlanOperation::Return,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::TransId,
                    value: CicsOperandValue::Literal(b"NEXT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Commarea,
                    value: CicsOperandValue::Storage(return_area.clone()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::LengthOf(return_area),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut invalid_return = return_plan.clone();
        invalid_return.operands[0].value = CicsOperandValue::Literal(b"TOOLONG".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&invalid_return, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let browse_key = slot(10, "BROWSE.KEY");
        let start_browse = CicsEffectPlan {
            operation: CicsPlanOperation::StartBrowse,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(browse_key.clone()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let read_next = CicsEffectPlan {
            operation: CicsPlanOperation::ReadNext,
            operands: start_browse.operands.clone(),
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Into,
                    target: slot(11, "BROWSE.RECORD"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Ridfld,
                    target: browse_key,
                },
            ],
            condition: CicsCondition::Default,
        };
        let read_prev = CicsEffectPlan {
            operation: CicsPlanOperation::ReadPrev,
            ..read_next.clone()
        };
        let end_browse = CicsEffectPlan {
            operation: CicsPlanOperation::EndBrowse,
            operands: vec![start_browse.operands[0].clone()],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mutation_key = slot(12, "FILE.KEY");
        let delete = CicsEffectPlan {
            operation: CicsPlanOperation::Delete,
            operands: vec![
                start_browse.operands[0].clone(),
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(mutation_key.clone()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let write = CicsEffectPlan {
            operation: CicsPlanOperation::Write,
            operands: vec![
                start_browse.operands[0].clone(),
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(13, "FILE.RECORD")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(mutation_key),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let write_transient = CicsEffectPlan {
            operation: CicsPlanOperation::WriteTransientData,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Queue,
                    value: CicsOperandValue::Literal(b"OUTQ".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(14, "TDQ.RECORD")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let receive_map = CicsEffectPlan {
            operation: CicsPlanOperation::ReceiveMap,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Map,
                    value: CicsOperandValue::Literal(b"MENU".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Mapset,
                    value: CicsOperandValue::Literal(b"MAIN".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: slot(15, "BMS.INPUT"),
            }],
            condition: CicsCondition::Default,
        };
        let send_map = CicsEffectPlan {
            operation: CicsPlanOperation::SendMap,
            operands: vec![
                receive_map.operands[0].clone(),
                receive_map.operands[1].clone(),
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(16, "BMS.OUTPUT")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Erase, CicsPlanOption::Cursor]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let send_text = CicsEffectPlan {
            operation: CicsPlanOperation::SendText,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(17, "BMS.TEXT")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let assign = CicsEffectPlan {
            operation: CicsPlanOperation::Assign,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Assign(
                    CicsAssignOutput::from_name("APPLID").expect("ASSIGN output"),
                ),
                target: slot(18, "ASSIGN.APPLID"),
            }],
            condition: CicsCondition::Default,
        };
        let purge_message = CicsEffectPlan {
            operation: CicsPlanOperation::PurgeMessage,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut current_record_delete = delete.clone();
        current_record_delete
            .operands
            .retain(|operand| operand.name != CicsOperandName::Ridfld);
        assert!(encode_cics_effect_plan(&current_record_delete, CicsPlanLimits::default()).is_ok());
        let mut literal_write_record = write.clone();
        literal_write_record.operands[1].value = CicsOperandValue::Literal(b"DATA".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&literal_write_record, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut write_with_length = write.clone();
        write_with_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Integer(4),
        });
        assert!(encode_cics_effect_plan(&write_with_length, CicsPlanLimits::default()).is_ok());
        let mut delete_with_length = delete.clone();
        delete_with_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Integer(4),
        });
        assert_eq!(
            encode_cics_effect_plan(&delete_with_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut mismatched_write_length = write.clone();
        mismatched_write_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::LengthOf(slot(16, "OTHER.RECORD")),
        });
        assert_eq!(
            encode_cics_effect_plan(&mismatched_write_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut rewrite_with_literal_length = rewrite.clone();
        rewrite_with_literal_length.operands[2].value = CicsOperandValue::Integer(5);
        assert!(
            encode_cics_effect_plan(&rewrite_with_literal_length, CicsPlanLimits::default())
                .is_ok()
        );
        let mut rewrite_with_mismatched_length = rewrite.clone();
        rewrite_with_mismatched_length.operands[2].value =
            CicsOperandValue::LengthOf(slot(16, "OTHER.RECORD"));
        assert_eq!(
            encode_cics_effect_plan(&rewrite_with_mismatched_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut rewrite_with_ridfld = rewrite.clone();
        rewrite_with_ridfld.operands.push(CicsNamedOperand {
            name: CicsOperandName::Ridfld,
            value: CicsOperandValue::Storage(slot(16, "FILE.KEY")),
        });
        assert_eq!(
            encode_cics_effect_plan(&rewrite_with_ridfld, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut read_with_zero_key_length = read.clone();
        read_with_zero_key_length
            .operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::KeyLength)
            .unwrap()
            .value = CicsOperandValue::Integer(0);
        assert_eq!(
            encode_cics_effect_plan(&read_with_zero_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut gteq_read_with_zero_key_length = read_with_zero_key_length.clone();
        gteq_read_with_zero_key_length
            .options
            .insert(CicsPlanOption::Gteq);
        assert!(
            encode_cics_effect_plan(&gteq_read_with_zero_key_length, CicsPlanLimits::default())
                .is_ok()
        );
        gteq_read_with_zero_key_length
            .options
            .insert(CicsPlanOption::Generic);
        assert_eq!(
            encode_cics_effect_plan(&gteq_read_with_zero_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut read_with_mismatched_key_length = read.clone();
        read_with_mismatched_key_length
            .operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::KeyLength)
            .unwrap()
            .value = CicsOperandValue::LengthOf(slot(16, "OTHER.KEY"));
        assert_eq!(
            encode_cics_effect_plan(&read_with_mismatched_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut read_with_literal_length = read.clone();
        read_with_literal_length
            .operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::Length)
            .unwrap()
            .value = CicsOperandValue::Integer(8);
        read_with_literal_length
            .outputs
            .retain(|output| output.name != CicsOutputName::Length);
        assert_eq!(
            encode_cics_effect_plan(&read_with_literal_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut generic_read = read.clone();
        generic_read.options.insert(CicsPlanOption::Generic);
        assert!(encode_cics_effect_plan(&generic_read, CicsPlanLimits::default()).is_ok());
        let mut generic_read_without_key_length = generic_read.clone();
        generic_read_without_key_length
            .operands
            .retain(|operand| operand.name != CicsOperandName::KeyLength);
        assert_eq!(
            encode_cics_effect_plan(&generic_read_without_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut generic_write = write.clone();
        generic_write.options.insert(CicsPlanOption::Generic);
        assert_eq!(
            encode_cics_effect_plan(&generic_write, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut write_with_key_length = write.clone();
        write_with_key_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::Integer(3),
        });
        assert!(encode_cics_effect_plan(&write_with_key_length, CicsPlanLimits::default()).is_ok());
        let mut zero_write_key_length = write.clone();
        zero_write_key_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::Integer(0),
        });
        assert_eq!(
            encode_cics_effect_plan(&zero_write_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut mismatched_write_key_length = write.clone();
        mismatched_write_key_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::LengthOf(slot(16, "OTHER.KEY")),
        });
        assert_eq!(
            encode_cics_effect_plan(&mismatched_write_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut delete_with_key_length = delete.clone();
        delete_with_key_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::Integer(3),
        });
        assert!(
            encode_cics_effect_plan(&delete_with_key_length, CicsPlanLimits::default()).is_ok()
        );
        let mut current_delete_with_key_length = current_record_delete.clone();
        current_delete_with_key_length
            .operands
            .push(CicsNamedOperand {
                name: CicsOperandName::KeyLength,
                value: CicsOperandValue::Integer(3),
            });
        assert_eq!(
            encode_cics_effect_plan(&current_delete_with_key_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut literal_transient_record = write_transient.clone();
        literal_transient_record.operands[1].value = CicsOperandValue::Literal(b"DATA".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&literal_transient_record, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut send_map_without_from = send_map.clone();
        send_map_without_from
            .operands
            .retain(|operand| operand.name != CicsOperandName::From);
        assert_eq!(
            encode_cics_effect_plan(&send_map_without_from, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut send_map_map_only = send_map.clone();
        send_map_map_only.operands.retain(|operand| {
            !matches!(
                operand.name,
                CicsOperandName::From | CicsOperandName::Length
            )
        });
        send_map_map_only.options.insert(CicsPlanOption::MapOnly);
        assert!(encode_cics_effect_plan(&send_map_map_only, CicsPlanLimits::default()).is_ok());
        let mut map_only_with_from = send_map_map_only.clone();
        map_only_with_from.operands.push(CicsNamedOperand {
            name: CicsOperandName::From,
            value: CicsOperandValue::Storage(slot(16, "BMS.OUTPUT")),
        });
        assert_eq!(
            encode_cics_effect_plan(&map_only_with_from, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut send_map_data_only = send_map.clone();
        send_map_data_only.options.insert(CicsPlanOption::DataOnly);
        assert!(encode_cics_effect_plan(&send_map_data_only, CicsPlanLimits::default()).is_ok());
        let mut data_only_without_from = send_map_data_only.clone();
        data_only_without_from
            .operands
            .retain(|operand| operand.name != CicsOperandName::From);
        assert_eq!(
            encode_cics_effect_plan(&data_only_without_from, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut missing_browse_key_output = read_next.clone();
        missing_browse_key_output
            .outputs
            .retain(|output| output.name != CicsOutputName::Ridfld);
        assert_eq!(
            encode_cics_effect_plan(&missing_browse_key_output, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        for plan in [
            read,
            rewrite,
            syncpoint,
            asktime_eib,
            asktime,
            format_time,
            abend,
            handle_abend,
            link,
            xctl,
            return_plan,
            start_browse,
            read_next,
            read_prev,
            end_browse,
            delete,
            delete_with_key_length,
            current_record_delete,
            write,
            write_with_length,
            write_with_key_length,
            write_transient,
            receive_map,
            send_map,
            send_map_map_only,
            send_map_data_only,
            send_text,
            assign,
            purge_message,
        ] {
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
            decoded
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Ridfld)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Storage(slot))
                if slot.qualified_layout_name == "REQUEST.KEY"
        ));
        assert_eq!(CICS_ASSIGN_OUTPUT_NAMES.len(), 92);
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[..78]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[78..83]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[83..85]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[85..86]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[86..87]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[87..88]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[88..89]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[89..91]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(
            CICS_ASSIGN_OUTPUT_NAMES[91..]
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert_eq!(
            CICS_ASSIGN_OUTPUT_NAMES
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            CICS_ASSIGN_OUTPUT_NAMES.len()
        );
        for (name, tag) in [
            ("DESTCOUNT", 96),
            ("LDCMNEM", 97),
            ("LDCNUM", 98),
            ("PAGENUM", 99),
            ("PARTNPAGE", 100),
            ("RETURNPROG", 101),
            ("TERMPRIORITY", 102),
            ("LANGINUSE", 103),
            ("INPUTMSGLEN", 104),
            ("INVOKINGPROG", 105),
            ("INPARTN", 106),
            ("FACILITY", 107),
            ("NETNAME", 108),
            ("TNADDR", 109),
        ] {
            let output = CicsAssignOutput::from_name(name).unwrap();
            assert_eq!(output_tag(CicsOutputName::Assign(output)), tag);
            assert_eq!(output_from_tag(tag), Ok(CicsOutputName::Assign(output)));
        }
        for name in CICS_ASSIGN_OUTPUT_NAMES {
            let output = CicsAssignOutput::from_name(name).expect("canonical ASSIGN output");
            assert_eq!(output.name(), *name);
            let plan = CicsEffectPlan {
                operation: CicsPlanOperation::Assign,
                operands: Vec::new(),
                options: BTreeSet::new(),
                outputs: vec![CicsOutputBinding {
                    name: CicsOutputName::Assign(output),
                    target: slot(19, "ASSIGN.OUTPUT"),
                }],
                condition: CicsCondition::Default,
            };
            let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(
                decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
                plan
            );
        }
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
    fn handle_stack_plans_round_trip_and_reject_unowned_options() {
        for operation in [CicsPlanOperation::PushHandle, CicsPlanOperation::PopHandle] {
            let plan = CicsEffectPlan {
                operation,
                operands: Vec::new(),
                options: BTreeSet::new(),
                outputs: Vec::new(),
                condition: CicsCondition::Default,
            };
            let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(
                decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
                plan
            );
            let mut malformed = plan;
            malformed.options.insert(CicsPlanOption::Rollback);
            assert_eq!(
                encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
    }

    #[test]
    fn handle_aid_plan_requires_canonical_bounded_unique_specifications() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::HandleAid,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Aids,
                value: CicsOperandValue::Literal(
                    b"ANYKEY\tANY-HANDLER\nENTER\t\nPF10\tPF-HANDLER".to_vec(),
                ),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut bare = plan.clone();
        bare.operands[0].value = CicsOperandValue::Literal(Vec::new());
        assert!(encode_cics_effect_plan(&bare, CicsPlanLimits::default()).is_ok());
        for specifications in [
            b"PF2\tTWO\nPF1\tONE".to_vec(),
            b"PF1\tONE\nPF1\tTWO".to_vec(),
            b"PF25\tHANDLER".to_vec(),
            b"ENTER\tlower-case".to_vec(),
            crate::CICS_APPLICATION_AID_NAMES[..17]
                .iter()
                .map(|name| format!("{name}\t"))
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ] {
            let mut malformed = plan.clone();
            malformed.operands[0].value = CicsOperandValue::Literal(specifications);
            assert_eq!(
                encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
    }

    #[test]
    fn handle_condition_plan_requires_canonical_bounded_unique_specifications() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::HandleCondition,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Conditions,
                value: CicsOperandValue::Literal(b"ERROR\tERR-HANDLER\nLENGERR\t".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        for specifications in [
            Vec::new(),
            b"ERROR\tONE\nERROR\tTWO".to_vec(),
            b"LENGERR\tTWO\nERROR\tONE".to_vec(),
            b"NOT-A-REVIEWED-CONDITION\tHANDLER".to_vec(),
            b"ERROR\tlower-case".to_vec(),
            crate::CICS_APPLICATION_CONDITION_NAMES[..17]
                .iter()
                .map(|name| format!("{name}\t"))
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ] {
            let mut malformed = plan.clone();
            malformed.operands[0].value = CicsOperandValue::Literal(specifications);
            assert_eq!(
                encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
    }

    #[test]
    fn ignore_condition_plan_requires_one_to_sixteen_reviewed_unique_names() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::IgnoreCondition,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Conditions,
                value: CicsOperandValue::Literal(b"ERROR\nLENGERR".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        for names in [
            Vec::new(),
            b"ERROR\nERROR".to_vec(),
            b"NOT-A-REVIEWED-CONDITION".to_vec(),
            crate::CICS_APPLICATION_CONDITION_NAMES[..17]
                .join("\n")
                .into_bytes(),
        ] {
            let mut malformed = plan.clone();
            malformed.operands[0].value = CicsOperandValue::Literal(names);
            assert_eq!(
                encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
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
    fn address_set_plans_require_one_pointer_and_one_address_role() {
        let pointer_from_data = CicsEffectPlan {
            operation: CicsPlanOperation::AddressSet,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SetPointer,
                    value: CicsOperandValue::Storage(slot(1, "PTR-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::UsingAddress,
                    value: CicsOperandValue::Storage(slot(2, "DATA-X")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded =
            encode_cics_effect_plan(&pointer_from_data, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            pointer_from_data
        );
        let mut data_from_pointer = pointer_from_data.clone();
        data_from_pointer.operands[0].name = CicsOperandName::SetAddress;
        data_from_pointer.operands[1].name = CicsOperandName::UsingPointer;
        assert!(encode_cics_effect_plan(&data_from_pointer, CicsPlanLimits::default()).is_ok());
        data_from_pointer.operands[0].value = CicsOperandValue::Literal(b"PTR-X".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&data_from_pointer, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn getmain_requires_one_length_form_and_one_set_pointer_output() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Getmain,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Flength,
                    value: CicsOperandValue::Integer(16),
                },
                CicsNamedOperand {
                    name: CicsOperandName::InitImage,
                    value: CicsOperandValue::Storage(slot(1, "INIT-X")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::NoSuspend]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SetPointer,
                target: slot(2, "PTR-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        let mut length = plan.clone();
        length.operands[0].name = CicsOperandName::Length;
        let bytes = encode_cics_effect_plan(&length, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), length);

        let mut both = plan.clone();
        both.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Integer(16),
        });
        assert_eq!(
            encode_cics_effect_plan(&both, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );

        let mut missing_set = plan.clone();
        missing_set.outputs.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing_set, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut literal_image = plan;
        literal_image.operands[1].value = CicsOperandValue::Literal(vec![b'Z']);
        assert_eq!(
            encode_cics_effect_plan(&literal_image, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn freemain_requires_one_storage_backed_data_form() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Freemain,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::DataPointer,
                value: CicsOperandValue::Storage(slot(1, "PTR-X")),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        let mut data = plan.clone();
        data.operands[0].name = CicsOperandName::DataArea;
        let bytes = encode_cics_effect_plan(&data, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), data);

        let mut both = plan.clone();
        both.operands.push(CicsNamedOperand {
            name: CicsOperandName::DataArea,
            value: CicsOperandValue::Storage(slot(2, "LINK-X")),
        });
        assert_eq!(
            encode_cics_effect_plan(&both, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );

        let mut missing = plan.clone();
        missing.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );

        let mut literal = plan;
        literal.operands[0].value = CicsOperandValue::Literal(vec![0; 8]);
        assert_eq!(
            encode_cics_effect_plan(&literal, limits),
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

    #[test]
    fn retrieve_set_requires_a_pointer_output_and_length_without_an_input_maximum() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Retrieve,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::SetPointer,
                    target: slot(1, "RESULT.POINTER"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Length,
                    target: slot(2, "RESULT.LENGTH"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        let mut with_input_maximum = plan.clone();
        with_input_maximum.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Storage(slot(2, "RESULT.LENGTH")),
        });
        assert_eq!(
            encode_cics_effect_plan(&with_input_maximum, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut both_destinations = plan;
        both_destinations.outputs.push(CicsOutputBinding {
            name: CicsOutputName::Into,
            target: slot(3, "RESULT.DATA"),
        });
        assert_eq!(
            encode_cics_effect_plan(&both_destinations, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn retrieve_wait_round_trips_only_on_the_retrieve_shape() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::Retrieve,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Length,
                value: CicsOperandValue::Storage(slot(1, "RESULT.LENGTH")),
            }],
            options: BTreeSet::from([CicsPlanOption::Wait]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Into,
                    target: slot(2, "RESULT.DATA"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Length,
                    target: slot(1, "RESULT.LENGTH"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        plan.operation = CicsPlanOperation::Read;
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn delay_explicit_units_require_exactly_one_for_or_until_mode() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::Delay,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Minutes,
                    value: CicsOperandValue::Storage(slot(1, "DELAY.MINUTES")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Seconds,
                    value: CicsOperandValue::Integer(3),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::For]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        plan.options.insert(CicsPlanOption::Until);
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.options.clear();
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );

        plan.operands = vec![CicsNamedOperand {
            name: CicsOperandName::StartTime,
            value: CicsOperandValue::Storage(slot(2, "DELAY.TIME")),
        }];
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        plan.operands = vec![CicsNamedOperand {
            name: CicsOperandName::Milliseconds,
            value: CicsOperandValue::Storage(slot(3, "DELAY.MILLISECONDS")),
        }];
        plan.options.insert(CicsPlanOption::For);
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);
        plan.options = BTreeSet::from([CicsPlanOption::Until]);
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn start_plan_allows_runtime_generated_request_identity() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Start,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::TransId,
                    value: CicsOperandValue::Literal(b"NEXT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(1, "REQUEST.DATA")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::UserId,
                    value: CicsOperandValue::Literal(b"TARGET".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Minutes,
                    value: CicsOperandValue::Storage(slot(2, "REQUEST.MINUTES")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Protect, CicsPlanOption::After]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);

        let mut no_data = plan.clone();
        no_data
            .operands
            .retain(|operand| operand.name != CicsOperandName::From);
        let bytes = encode_cics_effect_plan(&no_data, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), no_data);
        let mut invalid_length = no_data;
        invalid_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Integer(1),
        });
        assert_eq!(
            encode_cics_effect_plan(&invalid_length, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );

        let mut invalid_user = plan;
        invalid_user
            .operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::UserId)
            .unwrap()
            .value = CicsOperandValue::Integer(1);
        assert_eq!(
            encode_cics_effect_plan(&invalid_user, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    proptest! {
        #[test]
        fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = decode_cics_effect_plan(&bytes, CicsPlanLimits::default());
        }
    }
}
