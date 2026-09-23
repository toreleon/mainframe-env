//! Canonical executable plans for the typed CICS file/unit-of-work slice.

use crate::StorageId;
use std::collections::BTreeSet;
use std::fmt;

mod address;
mod assign;
mod browse;
mod codec_tags;
mod condition_handlers;
mod counter_control;
mod diagnostics;
mod document_control;
mod event_control;
mod file_mutation;
mod handle_abend;
mod identities;
mod interval_control;
mod journal_control;
mod option_shape;
mod outboard;
mod output_shape;
mod program_control;
mod queue_control;
mod route;
mod security_control;
mod spool_control;
mod storage_control;
mod task_wait;
mod terminal_control;
mod transform_control;
mod web_control;
mod web_service_control;

pub use assign::{CICS_ASSIGN_OUTPUT_NAMES, CicsAssignOutput};
pub use identities::{CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption};

use codec_tags::{
    operand_from_tag, operand_tag, operation_from_tag, operation_tag, option_from_tag, option_tag,
    output_from_tag, output_tag,
};
use condition_handlers::{valid_aid_handlers, valid_condition_handlers, valid_condition_list};

/// Stable wire identity for a typed CICS effect plan.
pub const CICS_EFFECT_PLAN_CONTRACT: &str = "mainframe-env.cics-effect-plan@2";

const MAGIC: &[u8; 4] = b"MCEP";
const LEGACY_VERSION: u16 = 1;
const VERSION: u16 = 2;

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
    encode_cics_effect_plan_version(plan, limits, VERSION)
}

fn encode_cics_effect_plan_version(
    plan: &CicsEffectPlan,
    limits: CicsPlanLimits,
    version: u16,
) -> Result<Vec<u8>, CicsPlanCodecProblem> {
    validate_plan(plan, limits)?;
    if version == LEGACY_VERSION
        && ((91..=104).contains(&operation_tag(plan.operation))
            || (130..=139).contains(&operation_tag(plan.operation))
            || (239..=258).contains(&operation_tag(plan.operation)))
    {
        return Err(CicsPlanCodecProblem::Malformed);
    }
    let mut writer = Writer::new(limits.max_encoded_bytes);
    writer.extend(MAGIC)?;
    writer.u16(version)?;
    writer.tag(operation_tag(plan.operation), version)?;

    let mut operands = plan.operands.iter().collect::<Vec<_>>();
    operands.sort_by_key(|operand| operand.name);
    writer.count(operands.len())?;
    for operand in operands {
        writer.tag(operand_tag(operand.name), version)?;
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
        let tag = option_tag(*option);
        if version == LEGACY_VERSION && (188..=251).contains(&tag) {
            return Err(CicsPlanCodecProblem::Malformed);
        }
        writer.tag(tag, version)?;
    }

    let mut outputs = plan.outputs.iter().collect::<Vec<_>>();
    outputs.sort_by_key(|output| output.name);
    writer.count(outputs.len())?;
    for output in outputs {
        writer.tag(output_tag(output.name), version)?;
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
    let version = reader.u16()?;
    if !matches!(version, LEGACY_VERSION | VERSION) {
        return Err(CicsPlanCodecProblem::UnsupportedVersion);
    }
    let operation_tag = reader.tag(version)?;
    if version == LEGACY_VERSION
        && ((91..=104).contains(&operation_tag)
            || (130..=139).contains(&operation_tag)
            || (239..=258).contains(&operation_tag))
    {
        return Err(CicsPlanCodecProblem::Malformed);
    }
    let operation = operation_from_tag(operation_tag)?;

    let operand_count = reader.count(limits.max_operands)?;
    let mut operands = Vec::with_capacity(operand_count);
    let mut last_operand = None;
    let mut literal_bytes = 0usize;
    for _ in 0..operand_count {
        let name = operand_from_tag(reader.tag(version)?)?;
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
        let tag = reader.tag(version)?;
        if version == LEGACY_VERSION && (188..=251).contains(&tag) {
            return Err(CicsPlanCodecProblem::Malformed);
        }
        let option = option_from_tag(tag)?;
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
        let name = output_from_tag(reader.tag(version)?)?;
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
    if encode_cics_effect_plan_version(&plan, limits, version)? != bytes {
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
            CicsOperandName::Length
                | CicsOperandName::KeyLength
                | CicsOperandName::ListLength
                | CicsOperandName::MaximumLength
                | CicsOperandName::WebReceiveMaxLength
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
        CicsPlanOperation::Address => address::invalid_shape(plan, inputs, outputs),
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
        CicsPlanOperation::InvokeApplication => {
            program_control::invalid_invoke_application_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Load => program_control::invalid_load_shape(plan, inputs, outputs),
        CicsPlanOperation::Release => {
            program_control::invalid_release_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Xctl => program_control::invalid_xctl_shape(plan, inputs, outputs),
        CicsPlanOperation::Return => program_control::invalid_return_shape(plan, inputs, outputs),
        CicsPlanOperation::StartBrowse
        | CicsPlanOperation::ResetBrowse
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
                || (plan.options.contains(&CicsPlanOption::Equal)
                    && plan.options.contains(&CicsPlanOption::Gteq))
                || plan.operands.iter().any(|operand| {
                    operand.name == CicsOperandName::KeyLength
                        && match operand.value {
                            CicsOperandValue::Integer(0) => {
                                !plan.options.contains(&CicsPlanOption::Gteq)
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
                            | CicsPlanOption::Equal
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
        CicsPlanOperation::Unlock => file_mutation::invalid_unlock_shape(plan, inputs, outputs),
        CicsPlanOperation::WriteTransientData => {
            queue_control::invalid_write_transient_data_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ReadTransientData => {
            queue_control::invalid_read_transient_data_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DeleteTransientData | CicsPlanOperation::DeleteTemporaryStorage => {
            queue_control::invalid_delete_transient_data_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ReadTemporaryStorage => {
            queue_control::invalid_read_temporary_storage_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WriteTemporaryStorage => {
            queue_control::invalid_write_temporary_storage_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Getmain => storage_control::invalid_getmain_shape(plan, inputs, outputs),
        CicsPlanOperation::Getmain64 => {
            storage_control::invalid_getmain64_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Freemain => {
            storage_control::invalid_freemain_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Freemain64 => {
            storage_control::invalid_freemain64_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ReceiveMap
        | CicsPlanOperation::ReceivePartn
        | CicsPlanOperation::SendMap
        | CicsPlanOperation::SendText
        | CicsPlanOperation::SendPartnset
        | CicsPlanOperation::SendControl
        | CicsPlanOperation::SendPage => terminal_control::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::IssueAbort
        | CicsPlanOperation::IssueAdd
        | CicsPlanOperation::IssueEnd
        | CicsPlanOperation::IssueErase
        | CicsPlanOperation::IssueNote
        | CicsPlanOperation::IssueQuery
        | CicsPlanOperation::IssueReceive
        | CicsPlanOperation::IssueReplace
        | CicsPlanOperation::IssueSend
        | CicsPlanOperation::IssueWait => outboard::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::Route => route::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::InvokeService
        | CicsPlanOperation::SoapFaultAdd
        | CicsPlanOperation::SoapFaultCreate
        | CicsPlanOperation::SoapFaultDelete
        | CicsPlanOperation::WsaContextBuild
        | CicsPlanOperation::WsaContextDelete
        | CicsPlanOperation::WsaContextGet
        | CicsPlanOperation::WsaEprCreate => web_service_control::invalid_shape(plan, inputs, outputs),
        CicsPlanOperation::TransformDataToJson
        | CicsPlanOperation::TransformDataToXml
        | CicsPlanOperation::TransformJsonToData
        | CicsPlanOperation::TransformXmlToData => {
            transform_control::invalid_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WebParseUrl => web_control::invalid_parse_url_shape(plan, inputs, outputs),
        CicsPlanOperation::WebOpen => web_control::invalid_open_shape(plan, inputs, outputs),
        CicsPlanOperation::WebClose => web_control::invalid_close_shape(plan, inputs, outputs),
        CicsPlanOperation::WebExtract => web_control::invalid_extract_shape(plan, inputs, outputs),
        CicsPlanOperation::ExtractWeb => web_control::invalid_extract_shape(plan, inputs, outputs),
        CicsPlanOperation::WebRead => web_control::invalid_read_shape(plan, inputs, outputs),
        CicsPlanOperation::WebStartBrowse => web_control::invalid_start_browse_shape(plan, inputs, outputs),
        CicsPlanOperation::WebReadNext => web_control::invalid_read_next_shape(plan, inputs, outputs),
        CicsPlanOperation::WebEndBrowse => web_control::invalid_end_browse_shape(plan, inputs, outputs),
        CicsPlanOperation::WebWrite => web_control::invalid_write_shape(plan, inputs, outputs),
        CicsPlanOperation::WebSend => web_control::invalid_send_shape(plan, inputs, outputs),
        CicsPlanOperation::WebRetrieve => {
            web_control::invalid_retrieve_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WebReceive => {
            web_control::invalid_receive_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WebConverse => {
            web_control::invalid_converse_shape(plan, inputs, outputs)
        }
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
        CicsPlanOperation::SpoolClose => {
            spool_control::invalid_close_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::SpoolOpenInput => {
            spool_control::invalid_open_input_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::SpoolOpenOutput => {
            spool_control::invalid_open_output_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::SpoolRead => spool_control::invalid_read_shape(plan, inputs, outputs),
        CicsPlanOperation::SpoolWrite => spool_control::invalid_write_shape(plan, inputs, outputs),
        CicsPlanOperation::DefineCounter | CicsPlanOperation::DefineDCounter => {
            counter_control::invalid_define_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DeleteCounter | CicsPlanOperation::DeleteDCounter => {
            counter_control::invalid_delete_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::GetCounter | CicsPlanOperation::GetDCounter => {
            counter_control::invalid_get_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::QueryCounter | CicsPlanOperation::QueryDCounter => {
            counter_control::invalid_query_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::RewindCounter | CicsPlanOperation::RewindDCounter => {
            counter_control::invalid_rewind_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::UpdateCounter | CicsPlanOperation::UpdateDCounter => {
            counter_control::invalid_update_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::EnterTraceNum => diagnostics::invalid_trace_num_shape(plan, inputs, outputs),
        CicsPlanOperation::Monitor => diagnostics::invalid_monitor_shape(plan, inputs, outputs),
        CicsPlanOperation::DumpTransaction => {
            diagnostics::invalid_dump_transaction_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::Dump => diagnostics::invalid_dump_shape(plan, inputs, outputs),
        CicsPlanOperation::Trace => diagnostics::invalid_trace_shape(plan, inputs, outputs),
        CicsPlanOperation::EnterTraceId => {
            diagnostics::invalid_trace_id_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::QuerySecurity => {
            security_control::invalid_query_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::VerifyPassword => {
            security_control::invalid_verify_password_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::VerifyPhrase => {
            security_control::invalid_verify_phrase_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ChangePassword => {
            security_control::invalid_change_password_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::ChangePhrase => {
            security_control::invalid_change_phrase_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::RequestPassTicket => {
            security_control::invalid_request_passticket_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::VerifyToken => security_control::invalid_verify_token_shape(plan, inputs, outputs),
        CicsPlanOperation::RequestEncryptPassTicket => security_control::invalid_request_encrypt_passticket_shape(plan, inputs, outputs),
        CicsPlanOperation::Signon => security_control::invalid_signon_shape(plan, inputs, outputs),
        CicsPlanOperation::Signoff => security_control::invalid_signoff_shape(plan, inputs, outputs),
        CicsPlanOperation::Suspend => {
            !inputs.is_empty() || scheduling_options || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::WaitEvent => task_wait::invalid_wait_event_shape(plan, inputs, outputs),
        CicsPlanOperation::WaitExternal => {
            task_wait::invalid_wait_external_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WaitJournalName => {
            journal_control::invalid_wait_journal_name_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WaitJournalNum => {
            journal_control::invalid_wait_journal_num_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::WriteJournalName | CicsPlanOperation::WriteJournalNum => {
            journal_control::invalid_write_journal_shape(plan, inputs, outputs)
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
        CicsPlanOperation::DocumentCreate => {
            document_control::invalid_create_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DocumentDelete => {
            document_control::invalid_delete_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DocumentInsert => {
            document_control::invalid_insert_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DocumentRetrieve => {
            document_control::invalid_retrieve_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DocumentSet => {
            document_control::invalid_set_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DefineInputEvent | CicsPlanOperation::DeleteEvent => {
            event_control::invalid_define_input_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DefineCompositeEvent => {
            event_control::invalid_define_composite_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::AddSubevent | CicsPlanOperation::RemoveSubevent => {
            event_control::invalid_membership_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::DefineTimer
        | CicsPlanOperation::CheckTimer
        | CicsPlanOperation::DeleteTimer
        | CicsPlanOperation::ForceTimer => {
            event_control::invalid_timer_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::RetrieveReattachEvent
        | CicsPlanOperation::RetrieveSubevent
        | CicsPlanOperation::TestEvent => {
            event_control::invalid_retrieve_shape(plan, inputs, outputs)
        }
        CicsPlanOperation::SignalEvent => {
            event_control::invalid_signal_shape(plan, inputs, outputs)
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
    fn tag(&mut self, value: u16, version: u16) -> Result<(), CicsPlanCodecProblem> {
        match version {
            LEGACY_VERSION => {
                self.byte(u8::try_from(value).map_err(|_| CicsPlanCodecProblem::Malformed)?)
            }
            VERSION => self.u16(value),
            _ => Err(CicsPlanCodecProblem::UnsupportedVersion),
        }
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
    fn tag(&mut self, version: u16) -> Result<u16, CicsPlanCodecProblem> {
        match version {
            LEGACY_VERSION => Ok(u16::from(self.byte()?)),
            VERSION => self.u16(),
            _ => Err(CicsPlanCodecProblem::UnsupportedVersion),
        }
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

    #[test]
    fn legacy_plan_golden_decodes_and_migrates_to_canonical_v2() {
        let limits = CicsPlanLimits::default();
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let v1_golden = b"MCEP\0\x01\x02\0\0\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(decode_cics_effect_plan(v1_golden, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION).unwrap(),
            v1_golden
        );
        let v2 = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(&v2[..8], b"MCEP\0\x02\0\x02");
        assert_eq!(v2.len(), v1_golden.len() + 1);
        assert_eq!(decode_cics_effect_plan(&v2, limits), Ok(plan));

        let legacy_read =
            encode_cics_effect_plan_version(&read_plan(), limits, LEGACY_VERSION).unwrap();
        let decoded = decode_cics_effect_plan(&legacy_read, limits).unwrap();
        assert_eq!(
            encode_cics_effect_plan_version(&decoded, limits, LEGACY_VERSION).unwrap(),
            legacy_read
        );
        assert_eq!(
            decode_cics_effect_plan(&encode_cics_effect_plan(&decoded, limits).unwrap(), limits),
            Ok(decoded)
        );
    }

    #[test]
    fn wide_identity_tags_are_big_endian_and_unknown_tags_fail_closed() {
        let limits = CicsPlanLimits::default();
        let mut writer = Writer::new(2);
        writer.tag(0x1234, VERSION).unwrap();
        assert_eq!(writer.finish(), [0x12, 0x34]);
        let mut reader = Reader::new(&[0x12, 0x34]);
        assert_eq!(reader.tag(VERSION), Ok(0x1234));
        assert_eq!(reader.tag(VERSION), Err(CicsPlanCodecProblem::Truncated));

        let mut read = encode_cics_effect_plan(&read_plan(), limits).unwrap();
        for offset in [6, 12] {
            let saved = [read[offset], read[offset + 1]];
            read[offset..offset + 2].copy_from_slice(&u16::MAX.to_be_bytes());
            assert_eq!(
                decode_cics_effect_plan(&read, limits),
                Err(CicsPlanCodecProblem::Malformed)
            );
            read[offset..offset + 2].copy_from_slice(&saved);
        }
        let options_plan = CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::Rollback]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut options = encode_cics_effect_plan(&options_plan, limits).unwrap();
        options[16..18].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&options, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut unordered = options_plan;
        unordered.options.insert(CicsPlanOption::NoHandle);
        unordered.condition = CicsCondition::NoHandle;
        let mut unordered = encode_cics_effect_plan(&unordered, limits).unwrap();
        unordered[16..18].copy_from_slice(&2u16.to_be_bytes());
        unordered[18..20].copy_from_slice(&1u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&unordered, limits),
            Err(CicsPlanCodecProblem::NonCanonical)
        );

        let output_plan = CicsEffectPlan {
            operation: CicsPlanOperation::Asktime,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Abstime,
                target: slot(1, "RESULT.ABSTIME"),
            }],
            condition: CicsCondition::Default,
        };
        let mut outputs = encode_cics_effect_plan(&output_plan, limits).unwrap();
        outputs[20..22].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&outputs, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            decode_cics_effect_plan(&outputs[..21], limits),
            Err(CicsPlanCodecProblem::Truncated)
        );
    }

    #[test]
    fn issue_reserved_tags_cross_the_v1_byte_boundary_without_truncation() {
        let mut legacy = Writer::new(8);
        legacy.tag(255, LEGACY_VERSION).unwrap();
        assert_eq!(
            legacy.tag(256, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(legacy.finish(), [255]);

        let mut canonical = Writer::new(8);
        for tag in 256..=258 {
            canonical.tag(tag, VERSION).unwrap();
        }
        assert_eq!(canonical.finish(), [1, 0, 1, 1, 1, 2]);
    }

    #[test]
    fn define_input_event_uses_reserved_v2_tags_without_changing_v1() {
        let limits = CicsPlanLimits::default();
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DefineInputEvent,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Event,
                value: CicsOperandValue::Literal(b"READY".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert_eq!(operation_tag(plan.operation), 108);
        assert_eq!(operand_tag(CicsOperandName::Event), 320);
        assert_eq!(operation_from_tag(108), Ok(plan.operation));
        assert_eq!(operand_from_tag(320), Ok(CicsOperandName::Event));
        let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(&encoded[..8], b"MCEP\0\x02\0l");
        assert_eq!(decode_cics_effect_plan(&encoded, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            decode_cics_effect_plan(b"MCEP\0\x01\x02\0\0\0\0\0\0\0\0\0\0\0\0\0", limits),
            Ok(CicsEffectPlan {
                operation: CicsPlanOperation::Syncpoint,
                operands: Vec::new(),
                options: BTreeSet::new(),
                outputs: Vec::new(),
                condition: CicsCondition::Default,
            })
        );
    }

    #[test]
    fn define_composite_event_tags_and_predicate_choice_are_exact() {
        let limits = CicsPlanLimits::default();
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::DefineCompositeEvent,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Event,
                    value: CicsOperandValue::Literal(b"GROUP".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SubEvent1,
                    value: CicsOperandValue::Literal(b"GO".to_vec()),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::EventOr]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert_eq!(operation_tag(plan.operation), 107);
        assert_eq!(operand_tag(CicsOperandName::SubEvent1), 321);
        assert_eq!(option_tag(CicsPlanOption::EventAnd), 252);
        assert_eq!(option_tag(CicsPlanOption::EventOr), 253);
        assert_eq!(operation_from_tag(107), Ok(plan.operation));
        assert_eq!(operand_from_tag(321), Ok(CicsOperandName::SubEvent1));
        assert_eq!(option_from_tag(253), Ok(CicsPlanOption::EventOr));
        let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&encoded, limits), Ok(plan.clone()));
        plan.options.insert(CicsPlanOption::EventAnd);
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.options.clear();
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn composite_membership_uses_exclusive_v2_operation_and_operand_tags() {
        let limits = CicsPlanLimits::default();
        for (operation, tag) in [
            (CicsPlanOperation::AddSubevent, 105),
            (CicsPlanOperation::RemoveSubevent, 113),
        ] {
            let plan = CicsEffectPlan {
                operation,
                operands: vec![
                    CicsNamedOperand {
                        name: CicsOperandName::Event,
                        value: CicsOperandValue::Literal(b"GROUP".to_vec()),
                    },
                    CicsNamedOperand {
                        name: CicsOperandName::SubEvent,
                        value: CicsOperandValue::Literal(b"GO".to_vec()),
                    },
                ],
                options: BTreeSet::new(),
                outputs: Vec::new(),
                condition: CicsCondition::Default,
            };
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
            assert_eq!(operand_tag(CicsOperandName::SubEvent), 329);
            assert_eq!(operand_from_tag(329), Ok(CicsOperandName::SubEvent));
            let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
            assert_eq!(decode_cics_effect_plan(&encoded, limits), Ok(plan));
        }
    }

    #[test]
    fn delete_event_uses_reserved_v2_operation_tag() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DeleteEvent,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Event,
                value: CicsOperandValue::Literal(b"GO".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert_eq!(operation_tag(plan.operation), 110);
        assert_eq!(operation_from_tag(110), Ok(plan.operation));
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan)
        );
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
        let mut equal_read = read.clone();
        equal_read.options.insert(CicsPlanOption::Equal);
        assert!(encode_cics_effect_plan(&equal_read, CicsPlanLimits::default()).is_ok());
        for base in [read, rewrite, syncpoint] {
            assert!(encode_cics_effect_plan(&base, CicsPlanLimits::default()).is_ok());
            for option in [
                CicsPlanOption::Gteq,
                CicsPlanOption::Equal,
                CicsPlanOption::Erase,
                CicsPlanOption::Cursor,
                CicsPlanOption::FreeKb,
                CicsPlanOption::DateSep,
                CicsPlanOption::TimeSep,
                CicsPlanOption::Wait,
                CicsPlanOption::MapOnly,
                CicsPlanOption::DataOnly,
            ] {
                if base.operation == CicsPlanOperation::Read
                    && matches!(option, CicsPlanOption::Gteq | CicsPlanOption::Equal)
                {
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
        assert_eq!(option_tag(CicsPlanOption::Equal), 26);
        assert_eq!(option_from_tag(26), Ok(CicsPlanOption::Equal));
        assert_eq!(option_tag(CicsPlanOption::Terminal), 27);
        assert_eq!(option_from_tag(27), Ok(CicsPlanOption::Terminal));
        assert_eq!(operation_tag(CicsPlanOperation::DeleteTemporaryStorage), 41);
        assert_eq!(
            operation_from_tag(41),
            Ok(CicsPlanOperation::DeleteTemporaryStorage)
        );
        assert_eq!(operand_tag(CicsOperandName::Qname), 43);
        assert_eq!(operand_from_tag(43), Ok(CicsOperandName::Qname));
        assert_eq!(operand_tag(CicsOperandName::SysId), 44);
        assert_eq!(operand_from_tag(44), Ok(CicsOperandName::SysId));
        assert_eq!(operation_tag(CicsPlanOperation::Address), 42);
        assert_eq!(operation_from_tag(42), Ok(CicsPlanOperation::Address));
        assert_eq!(operand_tag(CicsOperandName::CommareaPointer), 45);
        assert_eq!(operand_from_tag(45), Ok(CicsOperandName::CommareaPointer));
        assert_eq!(operation_tag(CicsPlanOperation::ReadTransientData), 51);
        assert_eq!(
            operation_from_tag(51),
            Ok(CicsPlanOperation::ReadTransientData)
        );
        assert_eq!(operation_tag(CicsPlanOperation::ReadTemporaryStorage), 49);
        assert_eq!(
            operation_from_tag(49),
            Ok(CicsPlanOperation::ReadTemporaryStorage)
        );
        assert_eq!(operand_tag(CicsOperandName::Item), 70);
        assert_eq!(operand_from_tag(70), Ok(CicsOperandName::Item));
        assert_eq!(option_tag(CicsPlanOption::Next), 44);
        assert_eq!(option_from_tag(44), Ok(CicsPlanOption::Next));
        assert_eq!(output_tag(CicsOutputName::NumItems), 200);
        assert_eq!(output_tag(CicsOutputName::JournalReqId), 201);
        assert_eq!(output_from_tag(200), Ok(CicsOutputName::NumItems));
        assert_eq!(operation_tag(CicsPlanOperation::WriteTemporaryStorage), 50);
        assert_eq!(
            operation_from_tag(50),
            Ok(CicsPlanOperation::WriteTemporaryStorage)
        );
        assert_eq!(option_tag(CicsPlanOption::RewriteTemporary), 45);
        assert_eq!(option_from_tag(45), Ok(CicsPlanOption::RewriteTemporary));
        assert_eq!(option_tag(CicsPlanOption::Auxiliary), 46);
        assert_eq!(option_from_tag(46), Ok(CicsPlanOption::Auxiliary));
        assert_eq!(option_tag(CicsPlanOption::Main), 47);
        assert_eq!(option_from_tag(47), Ok(CicsPlanOption::Main));
        assert_eq!(operation_tag(CicsPlanOperation::InvokeApplication), 46);
        assert_eq!(
            operation_from_tag(46),
            Ok(CicsPlanOperation::InvokeApplication)
        );
        for (operand, tag) in [
            (CicsOperandName::Application, 56),
            (CicsOperandName::Platform, 57),
            (CicsOperandName::ApplicationOperation, 58),
            (CicsOperandName::MajorVersion, 59),
            (CicsOperandName::MinorVersion, 60),
            (CicsOperandName::Channel, 61),
        ] {
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        assert_eq!(option_tag(CicsPlanOption::ExactMatch), 36);
        assert_eq!(option_from_tag(36), Ok(CicsPlanOption::ExactMatch));
        assert_eq!(option_tag(CicsPlanOption::Minimum), 37);
        assert_eq!(option_from_tag(37), Ok(CicsPlanOption::Minimum));
        assert_eq!(operation_tag(CicsPlanOperation::Load), 47);
        assert_eq!(operation_from_tag(47), Ok(CicsPlanOperation::Load));
        assert_eq!(operation_tag(CicsPlanOperation::Release), 48);
        assert_eq!(operation_from_tag(48), Ok(CicsPlanOperation::Release));
        for (operand, tag) in [
            (CicsOperandName::LoadSet, 62),
            (CicsOperandName::Entry, 63),
            (CicsOperandName::LoadLength, 64),
            (CicsOperandName::LoadFlength, 65),
        ] {
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        assert_eq!(option_tag(CicsPlanOption::Hold), 38);
        assert_eq!(option_from_tag(38), Ok(CicsPlanOption::Hold));

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
            operation_from_tag(u16::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            operand_from_tag(u16::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            option_from_tag(u16::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        assert_eq!(
            output_from_tag(u16::MAX),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut unknown_value_tag = bytes;
        unknown_value_tag[14] = u8::MAX;
        assert_eq!(
            decode_cics_effect_plan(&unknown_value_tag, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn every_cics_output_tag_is_unique_and_round_trips() {
        let mut outputs = vec![
            CicsOutputName::Into,
            CicsOutputName::SetPointer,
            CicsOutputName::Ridfld,
            CicsOutputName::Commarea,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
            CicsOutputName::Abstime,
            CicsOutputName::Milliseconds,
            CicsOutputName::Mmddyy,
            CicsOutputName::Mmddyyyy,
            CicsOutputName::Time,
            CicsOutputName::Yyddd,
            CicsOutputName::Yymmdd,
            CicsOutputName::Yyyymmdd,
            CicsOutputName::Length,
            CicsOutputName::ReturnTransId,
            CicsOutputName::ReturnTermId,
            CicsOutputName::Queue,
            CicsOutputName::NumItems,
            CicsOutputName::JournalReqId,
        ];
        outputs.extend(CICS_ASSIGN_OUTPUT_NAMES.iter().map(|name| {
            CicsOutputName::Assign(
                CicsAssignOutput::from_name(name).expect("canonical ASSIGN output"),
            )
        }));

        let mut tags = BTreeSet::new();
        for output in outputs {
            let tag = output_tag(output);
            assert!(
                tags.insert(tag),
                "duplicate output tag {tag} for {output:?}"
            );
            assert_eq!(output_from_tag(tag), Ok(output));
        }
    }

    #[test]
    fn readq_ts_plan_requires_exact_identity_destination_and_item_mode() {
        let length = slot(70, "LENGTH-X");
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::ReadTemporaryStorage,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Queue,
                    value: CicsOperandValue::Literal(b"TEMPQ".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Storage(length.clone()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Item,
                    value: CicsOperandValue::Integer(1),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Into,
                    target: slot(71, "DATA-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Length,
                    target: length,
                },
                CicsOutputBinding {
                    name: CicsOutputName::NumItems,
                    target: slot(72, "COUNT-X"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        let decoded = decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            encode_cics_effect_plan(&decoded, CicsPlanLimits::default()),
            Ok(encoded)
        );

        let mut next_and_item = plan.clone();
        next_and_item.options.insert(CicsPlanOption::Next);
        assert_eq!(
            encode_cics_effect_plan(&next_and_item, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut conflicting_identity = plan.clone();
        conflicting_identity.operands.push(CicsNamedOperand {
            name: CicsOperandName::Qname,
            value: CicsOperandValue::Literal(b"LONG-QUEUE".to_vec()),
        });
        assert_eq!(
            encode_cics_effect_plan(&conflicting_identity, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        for item in [0, -1] {
            let mut runtime_item = plan.clone();
            runtime_item
                .operands
                .iter_mut()
                .find(|operand| operand.name == CicsOperandName::Item)
                .unwrap()
                .value = CicsOperandValue::Integer(item);
            assert!(encode_cics_effect_plan(&runtime_item, CicsPlanLimits::default()).is_ok());
        }
        let mut set_without_length = plan;
        set_without_length.outputs[0].name = CicsOutputName::SetPointer;
        set_without_length
            .outputs
            .retain(|output| output.name != CicsOutputName::Length);
        assert_eq!(
            encode_cics_effect_plan(&set_without_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn writeq_ts_plan_requires_exact_append_rewrite_and_placement_shapes() {
        let item = slot(73, "ITEM-X");
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WriteTemporaryStorage,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Queue,
                    value: CicsOperandValue::Literal(b"TEMPQ".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::From,
                    value: CicsOperandValue::Storage(slot(74, "DATA-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Item,
                    value: CicsOperandValue::Storage(item.clone()),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Auxiliary]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::NumItems,
                target: slot(75, "COUNT-X"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        let decoded = decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            encode_cics_effect_plan(&decoded, CicsPlanLimits::default()),
            Ok(encoded)
        );

        let mut conflicting_placement = plan.clone();
        conflicting_placement.options.insert(CicsPlanOption::Main);
        assert_eq!(
            encode_cics_effect_plan(&conflicting_placement, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut rewrite_outputs = plan.clone();
        rewrite_outputs
            .options
            .insert(CicsPlanOption::RewriteTemporary);
        assert_eq!(
            encode_cics_effect_plan(&rewrite_outputs, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        for length in [0, -1, 32_764] {
            let mut runtime_length = plan.clone();
            runtime_length
                .operands
                .iter_mut()
                .find(|operand| operand.name == CicsOperandName::Length)
                .unwrap()
                .value = CicsOperandValue::Integer(length);
            assert!(encode_cics_effect_plan(&runtime_length, CicsPlanLimits::default()).is_ok());
        }
        let mut rewrite_without_item = plan;
        rewrite_without_item
            .options
            .insert(CicsPlanOption::RewriteTemporary);
        rewrite_without_item
            .operands
            .retain(|operand| operand.name != CicsOperandName::Item);
        rewrite_without_item.outputs.clear();
        assert_eq!(
            encode_cics_effect_plan(&rewrite_without_item, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_parse_url_uses_v2_only_reserved_tags_and_round_trips() {
        assert_eq!(operation_tag(CicsPlanOperation::WebParseUrl), 91);
        assert_eq!(operation_from_tag(91), Ok(CicsPlanOperation::WebParseUrl));
        for (name, tag) in [
            (CicsOperandName::WebUrl, 256),
            (CicsOperandName::WebUrlLength, 257),
            (CicsOperandName::WebHostLength, 258),
            (CicsOperandName::WebPathLength, 259),
            (CicsOperandName::WebQueryStringLength, 260),
        ] {
            assert!((256..=319).contains(&tag));
            assert_eq!(operand_tag(name), tag);
            assert_eq!(operand_from_tag(tag), Ok(name));
        }
        for (name, tag) in [
            (CicsOutputName::WebSchemeName, 312),
            (CicsOutputName::WebHost, 313),
            (CicsOutputName::WebHostLength, 314),
            (CicsOutputName::WebHostType, 315),
            (CicsOutputName::WebPortNumber, 316),
            (CicsOutputName::WebPath, 317),
            (CicsOutputName::WebPathLength, 318),
            (CicsOutputName::WebQueryString, 319),
            (CicsOutputName::WebQueryStringLength, 320),
        ] {
            assert!((312..=375).contains(&tag));
            assert_eq!(output_tag(name), tag);
            assert_eq!(output_from_tag(tag), Ok(name));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebParseUrl,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebUrl,
                    value: CicsOperandValue::Literal(b"http://example.com/".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebUrlLength,
                    value: CicsOperandValue::Integer(19),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::WebSchemeName,
                target: slot(1, "SCHEME-X"),
            }],
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_open_uses_reserved_v2_tags_and_rejects_legacy_encoding() {
        assert_eq!(operation_tag(CicsPlanOperation::WebOpen), 92);
        assert_eq!(operation_from_tag(92), Ok(CicsPlanOperation::WebOpen));
        assert_eq!(operation_tag(CicsPlanOperation::WebClose), 93);
        assert_eq!(operation_from_tag(93), Ok(CicsPlanOperation::WebClose));
        assert_eq!(operand_tag(CicsOperandName::WebSessionToken), 267);
        assert_eq!(operand_from_tag(267), Ok(CicsOperandName::WebSessionToken));
        for (name, tag) in [
            (CicsOperandName::WebHost, 261),
            (CicsOperandName::WebPortNumber, 262),
            (CicsOperandName::WebScheme, 263),
            (CicsOperandName::WebUriMap, 264),
            (CicsOperandName::WebCertificate, 265),
            (CicsOperandName::WebCodePage, 266),
        ] {
            assert!((256..=319).contains(&tag));
            assert_eq!(operand_tag(name), tag);
            assert_eq!(operand_from_tag(tag), Ok(name));
        }
        for (name, tag) in [
            (CicsOutputName::WebSessionToken, 321),
            (CicsOutputName::WebHttpVNum, 322),
            (CicsOutputName::WebHttpRNum, 323),
        ] {
            assert!((312..=375).contains(&tag));
            assert_eq!(output_tag(name), tag);
            assert_eq!(output_from_tag(tag), Ok(name));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebOpen,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebHostLength,
                    value: CicsOperandValue::Integer(11),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebHost,
                    value: CicsOperandValue::Literal(b"example.com".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebScheme,
                    value: CicsOperandValue::Literal(b"HTTP".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::WebSessionToken,
                target: slot(1, "TOKEN-X"),
            }],
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_extract_uses_reserved_v2_tags_and_checked_length_pair() {
        assert_eq!(operation_tag(CicsPlanOperation::WebExtract), 94);
        assert_eq!(operation_from_tag(94), Ok(CicsPlanOperation::WebExtract));
        assert_eq!(operation_tag(CicsPlanOperation::ExtractWeb), 95);
        assert_eq!(operation_from_tag(95), Ok(CicsPlanOperation::ExtractWeb));
        for (name, tag) in [
            (CicsOperandName::WebMethodLength, 268),
            (CicsOperandName::WebVersionLength, 269),
            (CicsOperandName::WebRealmLength, 270),
        ] {
            assert!((256..=319).contains(&tag));
            assert_eq!(operand_tag(name), tag);
            assert_eq!(operand_from_tag(tag), Ok(name));
        }
        for (name, tag) in [
            (CicsOutputName::WebScheme, 324),
            (CicsOutputName::WebHttpMethod, 325),
            (CicsOutputName::WebMethodLength, 326),
            (CicsOutputName::WebHttpVersion, 327),
            (CicsOutputName::WebVersionLength, 328),
            (CicsOutputName::WebRequestType, 329),
            (CicsOutputName::WebUriMap, 330),
            (CicsOutputName::WebRealm, 331),
            (CicsOutputName::WebRealmLength, 332),
        ] {
            assert!((312..=375).contains(&tag));
            assert_eq!(output_tag(name), tag);
            assert_eq!(output_from_tag(tag), Ok(name));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebExtract,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::WebHostLength,
                value: CicsOperandValue::Storage(slot(1, "HOST-LEN")),
            }],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebHost,
                    target: slot(2, "HOST-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebHostLength,
                    target: slot(1, "HOST-LEN"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut synonym = plan.clone();
        synonym.operation = CicsPlanOperation::ExtractWeb;
        let encoded = encode_cics_effect_plan(&synonym, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(synonym)
        );
        let mut invalid = plan;
        invalid.outputs.remove(1);
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_read_codec_keeps_selector_and_length_binding_in_v2() {
        assert_eq!(operation_tag(CicsPlanOperation::WebRead), 96);
        assert_eq!(operation_from_tag(96), Ok(CicsPlanOperation::WebRead));
        for (name, tag) in [
            (CicsOperandName::WebHttpHeaderName, 271),
            (CicsOperandName::WebQueryParmName, 272),
            (CicsOperandName::WebFormFieldName, 273),
            (CicsOperandName::WebNameLength, 274),
            (CicsOperandName::WebValueLength, 275),
        ] {
            assert_eq!(operand_tag(name), tag);
            assert_eq!(operand_from_tag(tag), Ok(name));
        }
        for (name, tag) in [
            (CicsOutputName::WebValue, 333),
            (CicsOutputName::WebValueLength, 334),
        ] {
            assert_eq!(output_tag(name), tag);
            assert_eq!(output_from_tag(tag), Ok(name));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebRead,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebHttpHeaderName,
                    value: CicsOperandValue::Literal(b"X-Test".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebNameLength,
                    value: CicsOperandValue::Integer(6),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebValueLength,
                    value: CicsOperandValue::Storage(slot(1, "VALUE-LEN")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebValue,
                    target: slot(2, "VALUE-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebValueLength,
                    target: slot(1, "VALUE-LEN"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut ambiguous = plan;
        ambiguous.operands.push(CicsNamedOperand {
            name: CicsOperandName::WebQueryParmName,
            value: CicsOperandValue::Literal(b"q".to_vec()),
        });
        assert_eq!(
            encode_cics_effect_plan(&ambiguous, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_startbrowse_codec_reserves_kind_options_and_start_name() {
        assert_eq!(operation_tag(CicsPlanOperation::WebStartBrowse), 97);
        assert_eq!(
            operation_from_tag(97),
            Ok(CicsPlanOperation::WebStartBrowse)
        );
        assert_eq!(operand_tag(CicsOperandName::WebBrowseStartName), 276);
        assert_eq!(
            operand_from_tag(276),
            Ok(CicsOperandName::WebBrowseStartName)
        );
        for (option, tag) in [
            (CicsPlanOption::WebBrowseHttpHeader, 188),
            (CicsPlanOption::WebBrowseQueryParm, 189),
            (CicsPlanOption::WebBrowseFormField, 190),
        ] {
            assert_eq!(option_tag(option), tag);
            assert_eq!(option_from_tag(tag), Ok(option));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebStartBrowse,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebNameLength,
                    value: CicsOperandValue::Integer(1),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebBrowseStartName,
                    value: CicsOperandValue::Literal(b"b".to_vec()),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::WebBrowseQueryParm]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut conflict = plan;
        conflict.options.insert(CicsPlanOption::WebBrowseFormField);
        assert_eq!(
            encode_cics_effect_plan(&conflict, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_readnext_codec_checks_cursor_result_bindings() {
        assert_eq!(operation_tag(CicsPlanOperation::WebReadNext), 98);
        assert_eq!(operation_from_tag(98), Ok(CicsPlanOperation::WebReadNext));
        assert_eq!(output_tag(CicsOutputName::WebBrowseName), 335);
        assert_eq!(output_tag(CicsOutputName::WebBrowseNameLength), 336);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebReadNext,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebNameLength,
                    value: CicsOperandValue::Storage(slot(1, "NAME-LEN")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebValueLength,
                    value: CicsOperandValue::Storage(slot(2, "VALUE-LEN")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::WebBrowseQueryParm]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebValue,
                    target: slot(3, "VALUE-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebValueLength,
                    target: slot(2, "VALUE-LEN"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebBrowseName,
                    target: slot(4, "NAME-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebBrowseNameLength,
                    target: slot(1, "NAME-LEN"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid = plan;
        invalid.outputs.remove(3);
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_endbrowse_codec_keeps_kind_and_rejects_client_token_on_query() {
        assert_eq!(operation_tag(CicsPlanOperation::WebEndBrowse), 99);
        assert_eq!(operation_from_tag(99), Ok(CicsPlanOperation::WebEndBrowse));
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebEndBrowse,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::WebBrowseQueryParm]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut legacy = encode_cics_effect_plan_version(
            &read_plan(),
            CicsPlanLimits::default(),
            LEGACY_VERSION,
        )
        .unwrap();
        legacy[6] = 99;
        assert_eq!(
            decode_cics_effect_plan(&legacy, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid = plan;
        invalid.operands.push(CicsNamedOperand {
            name: CicsOperandName::WebSessionToken,
            value: CicsOperandValue::Storage(slot(1, "TOKEN-X")),
        });
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_write_codec_rejects_legacy_and_unpaired_lengths() {
        assert_eq!(operation_tag(CicsPlanOperation::WebWrite), 100);
        assert_eq!(operation_from_tag(100), Ok(CicsPlanOperation::WebWrite));
        assert_eq!(operand_tag(CicsOperandName::WebHeaderValue), 277);
        assert_eq!(operand_from_tag(277), Ok(CicsOperandName::WebHeaderValue));
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebWrite,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebHttpHeaderName,
                    value: CicsOperandValue::Literal(b"X-Test".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebNameLength,
                    value: CicsOperandValue::Integer(6),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebValueLength,
                    value: CicsOperandValue::Integer(5),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebHeaderValue,
                    value: CicsOperandValue::Literal(b"alpha".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid = plan;
        invalid
            .operands
            .retain(|operand| operand.name != CicsOperandName::WebValueLength);
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_send_codec_roundtrips_client_request_and_rejects_v1() {
        assert_eq!(operation_tag(CicsPlanOperation::WebSend), 101);
        assert_eq!(operation_from_tag(101), Ok(CicsPlanOperation::WebSend));
        assert_eq!(operand_tag(CicsOperandName::WebMethod), 278);
        assert_eq!(operand_from_tag(278), Ok(CicsOperandName::WebMethod));
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebSend,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebPathLength,
                    value: CicsOperandValue::Integer(6),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebSessionToken,
                    value: CicsOperandValue::Storage(slot(0, "TOKEN-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebMethod,
                    value: CicsOperandValue::Literal(b"GET".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebPathInput,
                    value: CicsOperandValue::Literal(b"/ready".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_retrieve_codec_requires_document_token_output_in_v2() {
        assert_eq!(operation_tag(CicsPlanOperation::WebRetrieve), 102);
        assert_eq!(operation_from_tag(102), Ok(CicsPlanOperation::WebRetrieve));
        assert_eq!(output_tag(CicsOutputName::WebRetrieveDocumentToken), 337);
        assert_eq!(
            output_from_tag(337),
            Ok(CicsOutputName::WebRetrieveDocumentToken)
        );
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebRetrieve,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::WebRetrieveDocumentToken,
                target: slot(0, "TOKEN-X"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid = plan;
        invalid.outputs.clear();
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_receive_codec_preserves_bounded_body_and_notruncate() {
        assert_eq!(operation_tag(CicsPlanOperation::WebReceive), 103);
        assert_eq!(operation_from_tag(103), Ok(CicsPlanOperation::WebReceive));
        assert_eq!(operand_tag(CicsOperandName::WebReceiveMaxLength), 291);
        assert_eq!(option_tag(CicsPlanOption::WebNotruncate), 191);
        assert_eq!(output_tag(CicsOutputName::WebReceiveInto), 338);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebReceive,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::WebReceiveMaxLength,
                value: CicsOperandValue::Integer(4),
            }],
            options: BTreeSet::from([CicsPlanOption::WebNotruncate]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebReceiveInto,
                    target: slot(0, "BODY-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebReceiveLength,
                    target: slot(1, "LENGTH-X"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn web_converse_codec_uses_last_reserved_web_operation_tag() {
        assert_eq!(operation_tag(CicsPlanOperation::WebConverse), 104);
        assert_eq!(operation_from_tag(104), Ok(CicsPlanOperation::WebConverse));
        assert_eq!(output_tag(CicsOutputName::WebConverseInto), 345);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WebConverse,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::WebSessionToken,
                    value: CicsOperandValue::Storage(slot(0, "TOKEN-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebMethod,
                    value: CicsOperandValue::Literal(b"GET".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WebReceiveMaxLength,
                    value: CicsOperandValue::Integer(8),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::WebNotruncate]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebConverseInto,
                    target: slot(1, "BODY-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebConverseToLength,
                    target: slot(2, "LEN-X"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn document_create_tags_are_unique_reserved_and_round_trip() {
        assert_eq!(operation_tag(CicsPlanOperation::DocumentCreate), 63);
        assert_eq!(
            operation_from_tag(63),
            Ok(CicsPlanOperation::DocumentCreate)
        );
        let operands = [
            (CicsOperandName::DocumentToken, 132),
            (CicsOperandName::Text, 133),
            (CicsOperandName::Binary, 134),
            (CicsOperandName::FromDocument, 135),
            (CicsOperandName::Template, 136),
            (CicsOperandName::SymbolList, 137),
            (CicsOperandName::ListLength, 138),
            (CicsOperandName::Delimiter, 139),
            (CicsOperandName::HostCodePage, 140),
            (CicsOperandName::Bookmark, 141),
            (CicsOperandName::Symbol, 142),
            (CicsOperandName::AtBookmark, 143),
            (CicsOperandName::ToBookmark, 144),
            (CicsOperandName::MaximumLength, 145),
            (CicsOperandName::CharacterSet, 146),
            (CicsOperandName::SymbolValue, 147),
        ];
        let mut operand_tags = BTreeSet::new();
        for (operand, tag) in operands {
            assert!((132..=151).contains(&tag));
            assert!(operand_tags.insert(tag));
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        assert_eq!(option_tag(CicsPlanOption::Unescaped), 84);
        assert_eq!(option_from_tag(84), Ok(CicsPlanOption::Unescaped));
        for (output, tag) in [
            (CicsOutputName::DocumentToken, 216),
            (CicsOutputName::DocumentSize, 217),
        ] {
            assert!((216..=223).contains(&tag));
            assert_eq!(output_tag(output), tag);
            assert_eq!(output_from_tag(tag), Ok(output));
            assert!(!matches!(output, CicsOutputName::Assign(_)));
        }

        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DocumentCreate,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Text,
                    value: CicsOperandValue::Storage(slot(1, "DOCUMENT.TEXT")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::NoHandle]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::DocumentToken,
                    target: slot(2, "DOCUMENT.TOKEN"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::DocumentSize,
                    target: slot(3, "DOCUMENT.SIZE"),
                },
            ],
            condition: CicsCondition::NoHandle,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
    }

    #[test]
    fn document_delete_tag_and_shape_round_trip() {
        assert_eq!(operation_tag(CicsPlanOperation::DocumentDelete), 64);
        assert_eq!(
            operation_from_tag(64),
            Ok(CicsPlanOperation::DocumentDelete)
        );
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DocumentDelete,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::DocumentToken,
                value: CicsOperandValue::Storage(slot(1, "DOCUMENT.TOKEN")),
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
        let mut missing = plan;
        missing.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&missing, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn document_insert_tag_and_shape_round_trip() {
        assert_eq!(operation_tag(CicsPlanOperation::DocumentInsert), 65);
        assert_eq!(
            operation_from_tag(65),
            Ok(CicsPlanOperation::DocumentInsert)
        );
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DocumentInsert,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DocumentToken,
                    value: CicsOperandValue::Storage(slot(1, "DOCUMENT.TOKEN")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Text,
                    value: CicsOperandValue::Storage(slot(2, "DOCUMENT.TEXT")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::DocumentSize,
                target: slot(3, "DOCUMENT.SIZE"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut missing_length = plan;
        missing_length
            .operands
            .retain(|operand| operand.name != CicsOperandName::Length);
        assert_eq!(
            encode_cics_effect_plan(&missing_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn document_retrieve_tag_and_dataonly_round_trip() {
        assert_eq!(operation_tag(CicsPlanOperation::DocumentRetrieve), 66);
        assert_eq!(
            operation_from_tag(66),
            Ok(CicsPlanOperation::DocumentRetrieve)
        );
        assert_eq!(option_tag(CicsPlanOption::DocumentDataOnly), 85);
        assert_eq!(option_from_tag(85), Ok(CicsPlanOption::DocumentDataOnly));
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DocumentRetrieve,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::DocumentToken,
                    value: CicsOperandValue::Storage(slot(1, "DOCUMENT.TOKEN")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::MaximumLength,
                    value: CicsOperandValue::Integer(4),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::DocumentDataOnly]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Into,
                    target: slot(2, "DOCUMENT.INTO"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::Length,
                    target: slot(3, "DOCUMENT.LENGTH"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut missing = plan;
        missing
            .outputs
            .retain(|output| output.name != CicsOutputName::Length);
        assert_eq!(
            encode_cics_effect_plan(&missing, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn document_set_tag_and_symbol_shape_round_trip() {
        assert_eq!(operation_tag(CicsPlanOperation::DocumentSet), 67);
        assert_eq!(operation_from_tag(67), Ok(CicsPlanOperation::DocumentSet));
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DocumentSet,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Length,
                    value: CicsOperandValue::Integer(4),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DocumentToken,
                    value: CicsOperandValue::Storage(slot(1, "DOCUMENT.TOKEN")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Symbol,
                    value: CicsOperandValue::Literal(b"Name".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SymbolValue,
                    value: CicsOperandValue::Storage(slot(2, "DOCUMENT.VALUE")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::Unescaped]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut missing = plan;
        missing
            .operands
            .retain(|operand| operand.name != CicsOperandName::SymbolValue);
        assert_eq!(
            encode_cics_effect_plan(&missing, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn journal_tag_envelopes_remain_disjoint_from_existing_identities() {
        let operations = crate::CICS_EXECUTABLE_DESCRIPTORS
            .iter()
            .map(|descriptor| descriptor.operation)
            .collect::<BTreeSet<_>>();
        let tags = operations
            .iter()
            .copied()
            .map(operation_tag)
            .collect::<BTreeSet<_>>();
        assert_eq!(tags.len(), operations.len());
        assert_eq!(operation_tag(CicsPlanOperation::WaitJournalName), 54);
        assert_eq!(operation_tag(CicsPlanOperation::WaitJournalNum), 55);
        assert_eq!(operation_tag(CicsPlanOperation::WriteJournalName), 56);
        assert_eq!(operation_tag(CicsPlanOperation::WriteJournalNum), 57);
        assert_eq!(operand_tag(CicsOperandName::JournalName), 96);
        assert_eq!(operand_tag(CicsOperandName::JournalReqId), 97);
        assert_eq!(operand_tag(CicsOperandName::JournalNum), 98);
        for tag in 104..=111 {
            assert_eq!(operand_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 60..=71 {
            assert_eq!(option_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        assert_eq!(output_from_tag(201), Ok(CicsOutputName::JournalReqId));
        for tag in 202..=207 {
            assert_eq!(output_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
    }

    #[test]
    fn wait_journal_name_plan_round_trips_only_the_bounded_shape() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WaitJournalName,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::JournalName,
                    value: CicsOperandValue::Literal(b"ACCOUNTS".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalReqId,
                    value: CicsOperandValue::Storage(slot(54, "WAIT.REQID")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()).unwrap(),
            plan
        );
        for malformed in [
            CicsNamedOperand {
                name: CicsOperandName::JournalName,
                value: CicsOperandValue::Literal(Vec::new()),
            },
            CicsNamedOperand {
                name: CicsOperandName::JournalName,
                value: CicsOperandValue::Literal(b"TOO-LONG9".to_vec()),
            },
            CicsNamedOperand {
                name: CicsOperandName::JournalReqId,
                value: CicsOperandValue::Integer(7),
            },
        ] {
            let mut invalid = plan.clone();
            invalid
                .operands
                .retain(|operand| operand.name != malformed.name);
            invalid.operands.push(malformed);
            assert_eq!(
                encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
    }

    #[test]
    fn wait_journal_num_plan_keeps_numeric_identity_distinct() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WaitJournalNum,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::JournalNum,
                    value: CicsOperandValue::Integer(7),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalReqId,
                    value: CicsOperandValue::Storage(slot(55, "WAIT.NUM.REQID")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()).unwrap(),
            plan
        );
        for invalid_number in [0, 100] {
            let mut invalid = plan.clone();
            invalid.operands[0].value = CicsOperandValue::Integer(invalid_number);
            assert_eq!(
                encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
        let mut name_form = plan;
        name_form.operands[0].name = CicsOperandName::JournalName;
        name_form.operands[0].value = CicsOperandValue::Literal(b"DFHJ07".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&name_form, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn write_journal_name_plan_round_trips_payload_and_output_shape() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::WriteJournalName,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::JournalName,
                    value: CicsOperandValue::Literal(b"ACCOUNTS".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalTypeId,
                    value: CicsOperandValue::Literal(b"UR".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalFrom,
                    value: CicsOperandValue::Storage(slot(56, "WRITE.DATA")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalFlength,
                    value: CicsOperandValue::Integer(5),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalPrefix,
                    value: CicsOperandValue::Storage(slot(57, "WRITE.PREFIX")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalPfxLeng,
                    value: CicsOperandValue::Integer(2),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::JournalReqId,
                target: slot(58, "WRITE.REQID"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        plan.options.insert(CicsPlanOption::Wait);
        assert_eq!(
            encode_cics_effect_plan(&plan, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.outputs.clear();
        assert!(encode_cics_effect_plan(&plan, CicsPlanLimits::default()).is_ok());
        plan.operands
            .retain(|operand| operand.name != CicsOperandName::JournalTypeId);
        assert_eq!(
            encode_cics_effect_plan(&plan, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn write_journal_num_plan_reuses_record_fields_with_numeric_selector() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WriteJournalNum,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::JournalNum,
                    value: CicsOperandValue::Integer(7),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalTypeId,
                    value: CicsOperandValue::Literal(b"UR".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::JournalFrom,
                    value: CicsOperandValue::Storage(slot(59, "WRITE.NUM.DATA")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::JournalReqId,
                target: slot(60, "WRITE.NUM.REQID"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut invalid = plan;
        invalid.operands[0].value = CicsOperandValue::Integer(100);
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn spool_control_reserved_tags_are_unique_and_round_trip() {
        let operations = [
            (CicsPlanOperation::SpoolClose, 58),
            (CicsPlanOperation::SpoolOpenInput, 59),
            (CicsPlanOperation::SpoolOpenOutput, 60),
            (CicsPlanOperation::SpoolRead, 61),
            (CicsPlanOperation::SpoolWrite, 62),
        ];
        let operands = [
            (CicsOperandName::SpoolToken, 112),
            (CicsOperandName::SpoolUserId, 113),
            (CicsOperandName::SpoolClass, 114),
            (CicsOperandName::SpoolNode, 115),
            (CicsOperandName::SpoolRecordLength, 116),
            (CicsOperandName::SpoolOutDescr, 117),
            (CicsOperandName::SpoolMaxFlength, 118),
            (CicsOperandName::SpoolFrom, 119),
            (CicsOperandName::SpoolFlength, 120),
        ];
        let options = [
            (CicsPlanOption::SpoolKeep, 72),
            (CicsPlanOption::SpoolDelete, 73),
            (CicsPlanOption::SpoolNoCc, 74),
            (CicsPlanOption::SpoolAsa, 75),
            (CicsPlanOption::SpoolMcc, 76),
            (CicsPlanOption::SpoolPrint, 77),
            (CicsPlanOption::SpoolPunch, 78),
            (CicsPlanOption::SpoolLine, 79),
            (CicsPlanOption::SpoolPage, 80),
        ];
        assert_eq!(
            operations
                .iter()
                .map(|(_, tag)| *tag)
                .collect::<BTreeSet<_>>()
                .len(),
            operations.len()
        );
        assert_eq!(
            operands
                .iter()
                .map(|(_, tag)| *tag)
                .collect::<BTreeSet<_>>()
                .len(),
            operands.len()
        );
        assert_eq!(
            options
                .iter()
                .map(|(_, tag)| *tag)
                .collect::<BTreeSet<_>>()
                .len(),
            options.len()
        );
        let outputs = [
            (CicsOutputName::SpoolToken, 208),
            (CicsOutputName::SpoolToFlength, 209),
        ];
        assert_eq!(
            outputs
                .iter()
                .map(|(_, tag)| *tag)
                .collect::<BTreeSet<_>>()
                .len(),
            outputs.len()
        );
        for (operation, tag) in operations {
            assert!((58..=62).contains(&tag));
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        for (operand, tag) in operands {
            assert!((112..=131).contains(&tag));
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        for (option, tag) in options {
            assert!((72..=83).contains(&tag));
            assert_eq!(option_tag(option), tag);
            assert_eq!(option_from_tag(tag), Ok(option));
        }
        for (output, tag) in outputs {
            assert!((208..=215).contains(&tag));
            assert_eq!(output_tag(output), tag);
            assert_eq!(output_from_tag(tag), Ok(output));
        }

        let token = CicsStorageSlot {
            storage: StorageId::from_index(0).unwrap(),
            qualified_layout_name: "TOKEN-X".into(),
        };
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::SpoolClose,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::SpoolToken,
                value: CicsOperandValue::Storage(token),
            }],
            options: BTreeSet::from([CicsPlanOption::NoHandle, CicsPlanOption::SpoolKeep]),
            outputs: Vec::new(),
            condition: CicsCondition::NoHandle,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan)
        );
        let output = CicsEffectPlan {
            operation: CicsPlanOperation::SpoolOpenOutput,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SpoolUserId,
                    value: CicsOperandValue::Literal(b"DESTUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SpoolNode,
                    value: CicsOperandValue::Literal(b"LOCAL".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SpoolRecordLength,
                    value: CicsOperandValue::Storage(slot(1, "RECORD-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SpoolOutDescr,
                    value: CicsOperandValue::Storage(slot(2, "DESC-PTR")),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::SpoolAsa, CicsPlanOption::SpoolPunch]),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::Resp,
                    target: slot(3, "RESP-X"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::SpoolToken,
                    target: slot(0, "TOKEN-X"),
                },
            ],
            condition: CicsCondition::Respond {
                response: slot(3, "RESP-X"),
                response2: None,
            },
        };
        let encoded = encode_cics_effect_plan(&output, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(output)
        );
    }

    #[test]
    fn enter_tracenum_uses_reserved_v2_tags_and_checked_shape() {
        assert_eq!(operation_tag(CicsPlanOperation::EnterTraceNum), 151);
        assert_eq!(operand_tag(CicsOperandName::TraceNum), 576);
        assert_eq!(operand_tag(CicsOperandName::TraceFrom), 577);
        assert_eq!(operand_tag(CicsOperandName::TraceFromLength), 578);
        assert_eq!(operand_tag(CicsOperandName::TraceResource), 579);
        assert_eq!(option_tag(CicsPlanOption::TraceException), 508);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::EnterTraceNum,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::TraceNum,
                value: CicsOperandValue::Integer(123),
            }],
            options: BTreeSet::from([CicsPlanOption::TraceException]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(&encoded[..6], b"MCEP\0\x02");
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut invalid = plan;
        invalid.operands[0].name = CicsOperandName::TraceResource;
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn monitor_uses_reserved_v2_tags_and_rejects_missing_point() {
        assert_eq!(operation_tag(CicsPlanOperation::Monitor), 152);
        assert_eq!(operand_tag(CicsOperandName::MonitorPoint), 580);
        assert_eq!(operand_tag(CicsOperandName::MonitorEntryName), 581);
        assert_eq!(operand_tag(CicsOperandName::MonitorData1), 582);
        assert_eq!(operand_tag(CicsOperandName::MonitorData2), 583);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Monitor,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::MonitorPoint,
                value: CicsOperandValue::Integer(11),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        let mut invalid = plan;
        invalid.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn dump_transaction_uses_reserved_v2_tags_and_checked_dependencies() {
        assert_eq!(operation_tag(CicsPlanOperation::DumpTransaction), 149);
        assert_eq!(operand_tag(CicsOperandName::DumpCode), 584);
        assert_eq!(operand_tag(CicsOperandName::DumpFrom), 585);
        assert_eq!(operand_tag(CicsOperandName::DumpNumSegments), 590);
        assert_eq!(option_tag(CicsPlanOption::DumpComplete), 509);
        assert_eq!(option_tag(CicsPlanOption::DumpTrt), 520);
        assert_eq!(output_tag(CicsOutputName::DumpId), 632);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DumpTransaction,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::DumpCode,
                value: CicsOperandValue::Literal(b"ABCD".to_vec()),
            }],
            options: BTreeSet::from([CicsPlanOption::DumpTask]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::DumpId,
                target: slot(1, "DUMP-ID-X"),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        let mut invalid = plan;
        invalid.operands.push(CicsNamedOperand {
            name: CicsOperandName::DumpLength,
            value: CicsOperandValue::Integer(4),
        });
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn dump_uses_reserved_operation_and_dct_tags() {
        assert_eq!(operation_tag(CicsPlanOperation::Dump), 148);
        assert_eq!(option_tag(CicsPlanOption::DumpDct), 521);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Dump,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::DumpDct]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan)
        );
    }

    #[test]
    fn trace_uses_reserved_v2_tags_and_rejects_ambiguous_direction() {
        assert_eq!(operation_tag(CicsPlanOperation::Trace), 153);
        assert_eq!(option_tag(CicsPlanOption::TraceOn), 522);
        assert_eq!(option_tag(CicsPlanOption::TraceSingle), 527);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Trace,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::TraceOn, CicsPlanOption::TraceUser]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        let mut invalid = plan;
        invalid.options.insert(CicsPlanOption::TraceOff);
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn enter_traceid_uses_reserved_v2_tags_and_requires_identifier() {
        assert_eq!(operation_tag(CicsPlanOperation::EnterTraceId), 150);
        assert_eq!(operand_tag(CicsOperandName::TraceId), 591);
        assert_eq!(operand_tag(CicsOperandName::TraceEntryName), 594);
        assert_eq!(option_tag(CicsPlanOption::TraceAccount), 528);
        assert_eq!(option_tag(CicsPlanOption::TracePerform), 530);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::EnterTraceId,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::TraceId,
                value: CicsOperandValue::Literal(b"EV01".to_vec()),
            }],
            options: BTreeSet::from([CicsPlanOption::TraceMonitor]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        let mut invalid = plan;
        invalid.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
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
        let reset_browse = CicsEffectPlan {
            operation: CicsPlanOperation::ResetBrowse,
            ..start_browse.clone()
        };
        let reset_bytes =
            encode_cics_effect_plan(&reset_browse, CicsPlanLimits::default()).unwrap();
        assert_eq!(&reset_bytes[6..8], &74u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&reset_bytes, CicsPlanLimits::default()),
            Ok(reset_browse)
        );
        let mut invalid_reset_length = start_browse.clone();
        invalid_reset_length.operation = CicsPlanOperation::ResetBrowse;
        invalid_reset_length.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Storage(slot(12, "BROWSE.LENGTH")),
        });
        assert_eq!(
            encode_cics_effect_plan(&invalid_reset_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let read_prev = CicsEffectPlan {
            operation: CicsPlanOperation::ReadPrev,
            ..read_next.clone()
        };
        let browse_length = slot(12, "BROWSE.LENGTH");
        let mut length_read_next = read_next.clone();
        length_read_next.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Storage(browse_length.clone()),
        });
        length_read_next.outputs.push(CicsOutputBinding {
            name: CicsOutputName::Length,
            target: browse_length,
        });
        assert!(encode_cics_effect_plan(&length_read_next, CicsPlanLimits::default()).is_ok());
        let mut mismatched_length = length_read_next.clone();
        mismatched_length
            .outputs
            .iter_mut()
            .find(|output| output.name == CicsOutputName::Length)
            .expect("browse LENGTH output")
            .target = slot(13, "OTHER.LENGTH");
        assert_eq!(
            encode_cics_effect_plan(&mismatched_length, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let end_browse = CicsEffectPlan {
            operation: CicsPlanOperation::EndBrowse,
            operands: vec![start_browse.operands[0].clone()],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let mut equal_start_browse = start_browse.clone();
        equal_start_browse.options.insert(CicsPlanOption::Equal);
        assert!(encode_cics_effect_plan(&equal_start_browse, CicsPlanLimits::default()).is_ok());
        equal_start_browse.options.insert(CicsPlanOption::Gteq);
        assert_eq!(
            encode_cics_effect_plan(&equal_start_browse, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut generic_start_browse = start_browse.clone();
        generic_start_browse.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::Integer(2),
        });
        generic_start_browse.options.insert(CicsPlanOption::Generic);
        generic_start_browse.options.insert(CicsPlanOption::Equal);
        assert!(encode_cics_effect_plan(&generic_start_browse, CicsPlanLimits::default()).is_ok());
        generic_start_browse
            .operands
            .retain(|operand| operand.name != CicsOperandName::KeyLength);
        assert_eq!(
            encode_cics_effect_plan(&generic_start_browse, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut first_start_browse = start_browse.clone();
        first_start_browse.operands.push(CicsNamedOperand {
            name: CicsOperandName::KeyLength,
            value: CicsOperandValue::Integer(0),
        });
        assert_eq!(
            encode_cics_effect_plan(&first_start_browse, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        first_start_browse.options.insert(CicsPlanOption::Gteq);
        assert!(encode_cics_effect_plan(&first_start_browse, CicsPlanLimits::default()).is_ok());
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
        let delete_temporary_storage = CicsEffectPlan {
            operation: CicsPlanOperation::DeleteTemporaryStorage,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::Queue,
                value: CicsOperandValue::Literal(b"TEMPQ".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert!(
            encode_cics_effect_plan(&delete_temporary_storage, CicsPlanLimits::default()).is_ok()
        );
        let mut oversized_temporary_queue = delete_temporary_storage.clone();
        oversized_temporary_queue.operands[0].value =
            CicsOperandValue::Literal(b"TOOLONG09".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&oversized_temporary_queue, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut delete_long_temporary_storage = delete_temporary_storage.clone();
        delete_long_temporary_storage.operands[0] = CicsNamedOperand {
            name: CicsOperandName::Qname,
            value: CicsOperandValue::Literal(b"LONG-QUEUE".to_vec()),
        };
        assert!(
            encode_cics_effect_plan(&delete_long_temporary_storage, CicsPlanLimits::default())
                .is_ok()
        );
        delete_long_temporary_storage
            .operands
            .push(delete_temporary_storage.operands[0].clone());
        assert_eq!(
            encode_cics_effect_plan(&delete_long_temporary_storage, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut delete_temporary_at_local_system = delete_temporary_storage.clone();
        delete_temporary_at_local_system
            .operands
            .push(CicsNamedOperand {
                name: CicsOperandName::SysId,
                value: CicsOperandValue::Literal(b"S001".to_vec()),
            });
        assert!(
            encode_cics_effect_plan(&delete_temporary_at_local_system, CicsPlanLimits::default())
                .is_ok()
        );
        delete_temporary_at_local_system.operands[1].value =
            CicsOperandValue::Literal(b"TOOLONG".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&delete_temporary_at_local_system, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
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
        let receive_from = slot(18, "BMS.RECEIVE-FROM");
        let mut receive_map_from = receive_map.clone();
        receive_map_from.operands.extend([
            CicsNamedOperand {
                name: CicsOperandName::From,
                value: CicsOperandValue::Storage(receive_from.clone()),
            },
            CicsNamedOperand {
                name: CicsOperandName::Length,
                value: CicsOperandValue::LengthOf(receive_from),
            },
        ]);
        assert!(encode_cics_effect_plan(&receive_map_from, CicsPlanLimits::default()).is_ok());
        let mut receive_map_terminal = receive_map.clone();
        receive_map_terminal
            .options
            .insert(CicsPlanOption::Terminal);
        assert!(encode_cics_effect_plan(&receive_map_terminal, CicsPlanLimits::default()).is_ok());
        receive_map_terminal.operands.push(CicsNamedOperand {
            name: CicsOperandName::From,
            value: CicsOperandValue::Storage(slot(19, "BMS.TERMINAL-FROM")),
        });
        assert_eq!(
            encode_cics_effect_plan(&receive_map_terminal, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut receive_length_without_from = receive_map.clone();
        receive_length_without_from.operands.push(CicsNamedOperand {
            name: CicsOperandName::Length,
            value: CicsOperandValue::Integer(4),
        });
        assert_eq!(
            encode_cics_effect_plan(&receive_length_without_from, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
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
        assert!(
            encode_cics_effect_plan(&gteq_read_with_zero_key_length, CicsPlanLimits::default())
                .is_ok()
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
        generic_read.options.insert(CicsPlanOption::Equal);
        assert!(encode_cics_effect_plan(&generic_read, CicsPlanLimits::default()).is_ok());
        let mut conflicting_relation = generic_read.clone();
        conflicting_relation.options.insert(CicsPlanOption::Gteq);
        assert_eq!(
            encode_cics_effect_plan(&conflicting_relation, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
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
            delete_temporary_storage,
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
    fn wait_event_plan_freezes_tags_and_rejects_cross_command_shape() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WaitEvent,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::EventControlAddress,
                    value: CicsOperandValue::Storage(slot(1, "WAIT.ECB-POINTER")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::WaitName,
                    value: CicsOperandValue::Literal(b"EVENT001".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert_eq!(operation_tag(CicsPlanOperation::WaitEvent), 43);
        assert_eq!(operand_tag(CicsOperandName::EventControlAddress), 46);
        assert_eq!(operand_tag(CicsOperandName::WaitName), 47);
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(&encoded[6..8], &43u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut malformed = plan;
        malformed.operands[0].value = CicsOperandValue::Integer(1);
        assert_eq!(
            encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn wait_external_plan_freezes_reserved_tags_and_purgeability_shape() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WaitExternal,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::EcbList,
                    value: CicsOperandValue::Storage(slot(1, "WAIT.ECB-LIST-POINTER")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::NumEvents,
                    value: CicsOperandValue::Integer(2),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Purgeability,
                    value: CicsOperandValue::Literal(b"NOTPURGEABLE".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        assert_eq!(operation_tag(CicsPlanOperation::WaitExternal), 44);
        assert_eq!(operand_tag(CicsOperandName::EcbList), 48);
        assert_eq!(operand_tag(CicsOperandName::NumEvents), 49);
        assert_eq!(operand_tag(CicsOperandName::Purgeability), 50);
        assert_eq!(option_tag(CicsPlanOption::Purgeable), 28);
        assert_eq!(option_tag(CicsPlanOption::NotPurgeable), 29);
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(&encoded[6..8], &44u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut malformed = plan;
        malformed.options = BTreeSet::from([CicsPlanOption::Purgeable]);
        assert_eq!(
            encode_cics_effect_plan(&malformed, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
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
    fn address_commarea_plan_requires_pointer_and_optional_source_area() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Address,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::CommareaPointer,
                    value: CicsOperandValue::Storage(slot(1, "PTR-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::UsingAddress,
                    value: CicsOperandValue::Storage(slot(2, "DFHCOMMAREA")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );
        let mut absent = plan.clone();
        absent
            .operands
            .retain(|operand| operand.name != CicsOperandName::UsingAddress);
        assert!(encode_cics_effect_plan(&absent, CicsPlanLimits::default()).is_ok());
        absent.operands.clear();
        assert_eq!(
            encode_cics_effect_plan(&absent, CicsPlanLimits::default()),
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
    fn getmain64_uses_disjoint_append_only_tags_and_checked_plan_shape() {
        for tag in 0..=u16::from(u8::MAX) {
            if let Ok(value) = operation_from_tag(tag) {
                assert_eq!(operation_tag(value), tag, "operation tag {tag}");
            }
            if let Ok(value) = operand_from_tag(tag) {
                assert_eq!(operand_tag(value), tag, "operand tag {tag}");
            }
            if let Ok(value) = option_from_tag(tag) {
                assert_eq!(option_tag(value), tag, "option tag {tag}");
            }
            if let Ok(value) = output_from_tag(tag) {
                assert_eq!(output_tag(value), tag, "output tag {tag}");
            }
        }
        assert_eq!(operation_tag(CicsPlanOperation::Getmain64), 72);
        assert_eq!(operation_tag(CicsPlanOperation::Freemain64), 73);
        assert_eq!(operation_from_tag(73), Ok(CicsPlanOperation::Freemain64));
        assert_eq!(operand_tag(CicsOperandName::Flength64), 172);
        assert_eq!(operand_tag(CicsOperandName::Location64), 173);
        assert_eq!(operand_tag(CicsOperandName::Abi64), 174);
        assert_eq!(operand_tag(CicsOperandName::DataPointer64), 175);
        assert_eq!(operand_tag(CicsOperandName::DataArea64), 176);
        assert_eq!(option_tag(CicsPlanOption::CicsDataKey64), 108);
        assert_eq!(option_tag(CicsPlanOption::UserDataKey64), 109);
        assert_eq!(option_tag(CicsPlanOption::Shared64), 110);
        assert_eq!(option_tag(CicsPlanOption::Executable64), 111);
        assert_eq!(output_tag(CicsOutputName::SetPointer64), 232);
        for tag in 233..=239 {
            assert_eq!(output_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Getmain64,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Flength64,
                    value: CicsOperandValue::Integer(32),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Location64,
                    value: CicsOperandValue::Literal(b"LOC31".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Abi64,
                    value: CicsOperandValue::Literal(
                        b"mainframe-env.cics-amode64-nonle@1".to_vec(),
                    ),
                },
            ],
            options: BTreeSet::from([CicsPlanOption::NoSuspend, CicsPlanOption::Executable64]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SetPointer64,
                target: slot(2, "PTR-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&encoded, limits).unwrap(), plan);
        let mut missing_abi = plan.clone();
        missing_abi
            .operands
            .retain(|value| value.name != CicsOperandName::Abi64);
        assert_eq!(
            encode_cics_effect_plan(&missing_abi, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut conflicting_keys = plan.clone();
        conflicting_keys
            .options
            .insert(CicsPlanOption::CicsDataKey64);
        conflicting_keys
            .options
            .insert(CicsPlanOption::UserDataKey64);
        assert_eq!(
            encode_cics_effect_plan(&conflicting_keys, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut wrong_pointer = plan;
        wrong_pointer.outputs[0].name = CicsOutputName::SetPointer;
        assert_eq!(
            encode_cics_effect_plan(&wrong_pointer, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn freemain64_requires_one_checked_pointer_or_bound_area() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Freemain64,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Abi64,
                    value: CicsOperandValue::Literal(
                        b"mainframe-env.cics-amode64-nonle@1".to_vec(),
                    ),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DataPointer64,
                    value: CicsOperandValue::Storage(slot(1, "PTR64-X")),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);
        let mut area = plan.clone();
        area.operands[1].name = CicsOperandName::DataArea64;
        let bytes = encode_cics_effect_plan(&area, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), area);
        let mut both = plan.clone();
        both.operands.push(CicsNamedOperand {
            name: CicsOperandName::DataArea64,
            value: CicsOperandValue::Storage(slot(2, "AREA-X")),
        });
        assert_eq!(
            encode_cics_effect_plan(&both, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut old_pointer = plan.clone();
        old_pointer.operands[1].name = CicsOperandName::DataPointer;
        assert_eq!(
            encode_cics_effect_plan(&old_pointer, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut missing_abi = plan;
        missing_abi.operands.remove(0);
        assert_eq!(
            encode_cics_effect_plan(&missing_abi, limits),
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
        bytes[4..6].copy_from_slice(&3u16.to_be_bytes());
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

    #[test]
    fn transform_tags_are_unique_reserved_and_round_trip() {
        let operations = [
            CicsPlanOperation::TransformDataToJson,
            CicsPlanOperation::TransformDataToXml,
            CicsPlanOperation::TransformJsonToData,
            CicsPlanOperation::TransformXmlToData,
        ];
        assert_eq!(operations.map(operation_tag), [68, 69, 70, 71]);
        for operation in operations {
            let tag = operation_tag(operation);
            assert!(codec_tags::TRANSFORM_OPERATION_TAGS.contains(&tag));
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        let operands = [
            CicsOperandName::Channel,
            CicsOperandName::InContainer,
            CicsOperandName::OutContainer,
            CicsOperandName::Transformer,
            CicsOperandName::DataContainer,
            CicsOperandName::XmlContainer,
            CicsOperandName::XmlTransform,
            CicsOperandName::NsContainer,
            CicsOperandName::ElementName,
            CicsOperandName::ElementNameLength,
            CicsOperandName::ElementNamespace,
            CicsOperandName::ElementNamespaceLength,
            CicsOperandName::TypeName,
            CicsOperandName::TypeNameLength,
            CicsOperandName::TypeNamespace,
            CicsOperandName::TypeNamespaceLength,
        ];
        let tags = operands.map(operand_tag);
        assert_eq!(
            tags,
            [
                61, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167
            ]
        );
        assert!(
            tags.iter()
                .all(|tag| *tag == 61 || codec_tags::TRANSFORM_OPERAND_TAGS.contains(tag))
        );
        assert_eq!(tags.into_iter().collect::<BTreeSet<_>>().len(), tags.len());
        for (name, tag) in operands.into_iter().zip(tags) {
            assert_eq!(operand_from_tag(tag), Ok(name));
        }
        assert_eq!(codec_tags::TRANSFORM_OPTION_TAGS, 96..=107);
        assert_eq!(codec_tags::TRANSFORM_OUTPUT_TAGS, 224..=231);
        for tag in 168..=171 {
            assert_eq!(operand_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 96..=107 {
            assert_eq!(option_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for (output, tag) in [
            (CicsOutputName::ElementName, 224),
            (CicsOutputName::ElementNameLength, 225),
            (CicsOutputName::ElementNamespace, 226),
            (CicsOutputName::ElementNamespaceLength, 227),
            (CicsOutputName::TypeName, 228),
            (CicsOutputName::TypeNameLength, 229),
            (CicsOutputName::TypeNamespace, 230),
            (CicsOutputName::TypeNamespaceLength, 231),
        ] {
            assert_eq!(output_tag(output), tag);
            assert_eq!(output_from_tag(tag), Ok(output));
        }

        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::TransformDataToJson,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Channel,
                    value: CicsOperandValue::Literal(b"WORK".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::InContainer,
                    value: CicsOperandValue::Storage(slot(1, "INPUT-CONTAINER")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::OutContainer,
                    value: CicsOperandValue::Literal(b"JSON".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Transformer,
                    value: CicsOperandValue::Literal(b"CUSTOMER".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            plan
        );

        let mut reverse = plan.clone();
        reverse.operation = CicsPlanOperation::TransformJsonToData;
        let encoded = encode_cics_effect_plan(&reverse, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            reverse
        );

        let xml = CicsEffectPlan {
            operation: CicsPlanOperation::TransformDataToXml,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Channel,
                    value: CicsOperandValue::Literal(b"WORK".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::DataContainer,
                    value: CicsOperandValue::Literal(b"DATA".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::XmlContainer,
                    value: CicsOperandValue::Literal(b"XML".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::XmlTransform,
                    value: CicsOperandValue::Literal(b"CUSTOMERXML".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::ElementNameLength,
                    value: CicsOperandValue::Storage(slot(2, "ELEMENT-LENGTH")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::ElementName,
                    target: slot(3, "ELEMENT-NAME"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::ElementNameLength,
                    target: slot(2, "ELEMENT-LENGTH"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&xml, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            xml
        );

        let query = CicsEffectPlan {
            operation: CicsPlanOperation::TransformXmlToData,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::Channel,
                    value: CicsOperandValue::Literal(b"WORK".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::XmlContainer,
                    value: CicsOperandValue::Literal(b"XML".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::ElementName,
                    value: CicsOperandValue::Storage(slot(3, "ELEMENT-NAME")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::ElementNameLength,
                    value: CicsOperandValue::Storage(slot(2, "ELEMENT-LENGTH")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::ElementName,
                    target: slot(3, "ELEMENT-NAME"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::ElementNameLength,
                    target: slot(2, "ELEMENT-LENGTH"),
                },
            ],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&query, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
            query
        );
    }

    #[test]
    fn all_cics_wire_tag_spaces_are_unique_and_round_trip() {
        let mut operations = BTreeSet::new();
        let mut operands = BTreeSet::new();
        let mut options = BTreeSet::new();
        let mut outputs = BTreeSet::new();
        for tag in u16::MIN..=u16::MAX {
            if let Ok(value) = operation_from_tag(tag) {
                assert!(operations.insert(value), "duplicate operation tag {tag}");
                assert_eq!(operation_tag(value), tag);
            }
            if let Ok(value) = operand_from_tag(tag) {
                assert!(operands.insert(value), "duplicate operand tag {tag}");
                assert_eq!(operand_tag(value), tag);
            }
            if let Ok(value) = option_from_tag(tag) {
                assert!(options.insert(value), "duplicate option tag {tag}");
                assert_eq!(option_tag(value), tag);
            }
            if let Ok(value) = output_from_tag(tag) {
                assert!(outputs.insert(value), "duplicate output tag {tag}");
                assert_eq!(output_tag(value), tag);
            }
        }
        assert_eq!(operation_tag(CicsPlanOperation::ResetBrowse), 74);
        assert_eq!(operation_from_tag(74), Ok(CicsPlanOperation::ResetBrowse));
        assert_eq!(operation_tag(CicsPlanOperation::Unlock), 75);
        assert_eq!(operation_tag(CicsPlanOperation::QuerySecurity), 132);
        assert_eq!(operation_tag(CicsPlanOperation::VerifyPassword), 137);
        assert_eq!(operation_tag(CicsPlanOperation::ChangePassword), 130);
        assert_eq!(operation_tag(CicsPlanOperation::ChangePhrase), 131);
        assert_eq!(operation_tag(CicsPlanOperation::RequestPassTicket), 134);
        assert_eq!(operation_tag(CicsPlanOperation::Signon), 136);
        assert_eq!(operation_tag(CicsPlanOperation::Signoff), 135);
        assert_eq!(operation_tag(CicsPlanOperation::VerifyPhrase), 138);
        assert_eq!(operand_tag(CicsOperandName::ResClass), 448);
        assert_eq!(operand_tag(CicsOperandName::LogMessage), 452);
        assert_eq!(operand_tag(CicsOperandName::SecurityUserId), 453);
        assert_eq!(operand_tag(CicsOperandName::SecurityPassword), 455);
        assert_eq!(operand_tag(CicsOperandName::SecurityNewPassword), 458);
        assert_eq!(operand_tag(CicsOperandName::SecurityNewPhrase), 459);
        assert_eq!(operand_tag(CicsOperandName::SecurityNewPhraseLen), 460);
        assert_eq!(operand_tag(CicsOperandName::SecurityEsmAppName), 461);
        assert_eq!(operand_tag(CicsOperandName::SecurityLanguageCode), 462);
        assert_eq!(operand_tag(CicsOperandName::SecurityNatLang), 463);
        assert_eq!(operand_tag(CicsOperandName::SecurityOidCard), 464);
        assert_eq!(operand_tag(CicsOperandName::SecurityPhrase), 456);
        assert_eq!(operand_tag(CicsOperandName::SecurityPhraseLen), 457);
        assert_eq!(output_tag(CicsOutputName::SecurityRead), 504);
        assert_eq!(output_tag(CicsOutputName::SecurityAlter), 507);
        assert_eq!(output_tag(CicsOutputName::SecurityInvalidCount), 513);
        assert_eq!(output_tag(CicsOutputName::SecurityPassTicket), 515);
        assert_eq!(output_tag(CicsOutputName::SecurityLangInUse), 516);
        assert_eq!(output_tag(CicsOutputName::SecurityNatLangInUse), 517);
        assert_eq!(operation_from_tag(75), Ok(CicsPlanOperation::Unlock));
        assert_eq!(operation_tag(CicsPlanOperation::SendPartnset), 90);
        assert_eq!(operation_from_tag(90), Ok(CicsPlanOperation::SendPartnset));
        assert_eq!(operation_tag(CicsPlanOperation::ReceivePartn), 86);
        assert_eq!(operation_from_tag(86), Ok(CicsPlanOperation::ReceivePartn));
        assert_eq!(operation_tag(CicsPlanOperation::SendControl), 88);
        assert_eq!(operation_from_tag(88), Ok(CicsPlanOperation::SendControl));
        assert_eq!(operation_tag(CicsPlanOperation::SendPage), 89);
        assert_eq!(operation_from_tag(89), Ok(CicsPlanOperation::SendPage));
        for (operation, tag) in [
            (CicsPlanOperation::IssueAbort, 76),
            (CicsPlanOperation::IssueAdd, 77),
            (CicsPlanOperation::IssueEnd, 78),
            (CicsPlanOperation::IssueErase, 79),
            (CicsPlanOperation::IssueNote, 80),
            (CicsPlanOperation::IssueQuery, 81),
            (CicsPlanOperation::IssueReceive, 82),
            (CicsPlanOperation::IssueReplace, 83),
            (CicsPlanOperation::IssueSend, 84),
            (CicsPlanOperation::Route, 87),
            (CicsPlanOperation::IssueWait, 85),
        ] {
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        assert_eq!(operand_tag(CicsOperandName::Token), 182);
        assert_eq!(operand_from_tag(182), Ok(CicsOperandName::Token));
        assert_eq!(operand_tag(CicsOperandName::Partnset), 192);
        assert_eq!(operand_from_tag(192), Ok(CicsOperandName::Partnset));
        assert_eq!(output_tag(CicsOutputName::Token), 240);
        assert_eq!(output_from_tag(240), Ok(CicsOutputName::Token));
        for (operation, tag) in [
            (CicsPlanOperation::CheckTimer, 106),
            (CicsPlanOperation::DefineTimer, 109),
            (CicsPlanOperation::DeleteTimer, 111),
            (CicsPlanOperation::ForceTimer, 112),
            (CicsPlanOperation::RetrieveReattachEvent, 114),
            (CicsPlanOperation::RetrieveSubevent, 115),
            (CicsPlanOperation::TestEvent, 117),
            (CicsPlanOperation::SignalEvent, 116),
        ] {
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        assert_eq!(operand_tag(CicsOperandName::Timer), 330);
        assert_eq!(operand_from_tag(330), Ok(CicsOperandName::Timer));
        assert_eq!(operand_tag(CicsOperandName::SignalFrom), 339);
        assert_eq!(operand_from_tag(339), Ok(CicsOperandName::SignalFrom));
        assert_eq!(operand_tag(CicsOperandName::SignalFromLength), 340);
        assert_eq!(operand_from_tag(340), Ok(CicsOperandName::SignalFromLength));
        assert_eq!(operand_tag(CicsOperandName::SignalFromChannel), 341);
        assert_eq!(
            operand_from_tag(341),
            Ok(CicsOperandName::SignalFromChannel)
        );
        assert_eq!(option_tag(CicsPlanOption::TimerAfter), 254);
        assert_eq!(option_from_tag(254), Ok(CicsPlanOption::TimerAfter));
        assert_eq!(output_tag(CicsOutputName::TimerStatus), 376);
        assert_eq!(output_from_tag(376), Ok(CicsOutputName::TimerStatus));
        assert_eq!(output_tag(CicsOutputName::EventName), 377);
        assert_eq!(output_from_tag(377), Ok(CicsOutputName::EventName));
        assert_eq!(output_tag(CicsOutputName::SubEventName), 378);
        assert_eq!(output_from_tag(378), Ok(CicsOutputName::SubEventName));
        assert_eq!(output_tag(CicsOutputName::EventType), 379);
        assert_eq!(output_from_tag(379), Ok(CicsOutputName::EventType));
        assert_eq!(output_tag(CicsOutputName::FireStatus), 380);
        assert_eq!(output_from_tag(380), Ok(CicsOutputName::FireStatus));
        assert_eq!(operations.len(), crate::CICS_EXECUTABLE_DESCRIPTORS.len());
        for tag in 183..=191 {
            assert_eq!(operand_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        assert_eq!(option_tag(CicsPlanOption::AsIs), 124);
        assert_eq!(option_from_tag(124), Ok(CicsPlanOption::AsIs));
        assert_eq!(output_tag(CicsOutputName::Partn), 248);
        assert_eq!(output_from_tag(248), Ok(CicsOutputName::Partn));
        for tag in [u16::MAX] {
            assert_eq!(
                operation_from_tag(tag),
                Err(CicsPlanCodecProblem::Malformed)
            );
        }
        for tag in 116..=123 {
            assert_eq!(option_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 241..=247 {
            assert_eq!(output_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
    }

    #[test]
    fn verify_password_plan_keeps_secret_in_storage_and_requires_v2_tags() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::VerifyPassword,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SecurityUserId,
                    value: CicsOperandValue::Literal(b"IBMUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPassword,
                    value: CicsOperandValue::Storage(slot(48, "PASS-X")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityInvalidCount,
                target: slot(49, "COUNT-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands[1].value = CicsOperandValue::Literal(b"PASSWORD".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn change_password_plan_keeps_both_secrets_in_storage_and_requires_v2() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::ChangePassword,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SecurityUserId,
                    value: CicsOperandValue::Literal(b"IBMUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPassword,
                    value: CicsOperandValue::Storage(slot(48, "OLD-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityNewPassword,
                    value: CicsOperandValue::Storage(slot(49, "NEW-X")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityEsmResp,
                target: slot(50, "ESM-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands[2].value = CicsOperandValue::Literal(b"NEWPASS1".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn change_phrase_plan_keeps_both_secrets_in_storage_and_requires_v2() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::ChangePhrase,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SecurityUserId,
                    value: CicsOperandValue::Literal(b"PHUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPhrase,
                    value: CicsOperandValue::Storage(slot(48, "OLD-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPhraseLen,
                    value: CicsOperandValue::Integer(16),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityNewPhrase,
                    value: CicsOperandValue::Storage(slot(49, "NEW-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityNewPhraseLen,
                    value: CicsOperandValue::Integer(20),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityEsmResp,
                target: slot(50, "ESM-X"),
            }],
            condition: CicsCondition::Default,
        };
        plan.operands.sort_by_key(|operand| operand.name);
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::SecurityNewPhrase)
            .unwrap()
            .value = CicsOperandValue::Literal(b"NEW-LONG-PHRASE-5678".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn passticket_plan_requires_storage_application_and_writable_output_in_v2() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::RequestPassTicket,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::SecurityEsmAppName,
                value: CicsOperandValue::Storage(slot(48, "APP-X")),
            }],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityPassTicket,
                target: slot(49, "TICKET-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands[0].value = CicsOperandValue::Literal(b"APP1".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn signon_plan_requires_one_storage_secret_and_v2_tags() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::Signon,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SecurityUserId,
                    value: CicsOperandValue::Literal(b"PHUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPassword,
                    value: CicsOperandValue::Storage(slot(48, "PASS-X")),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityLangInUse,
                target: slot(49, "LANG-X"),
            }],
            condition: CicsCondition::Default,
        };
        plan.operands.sort_by_key(|operand| operand.name);
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::SecurityPassword)
            .unwrap()
            .value = CicsOperandValue::Literal(b"PASSWORD".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn signoff_plan_has_no_data_operands_and_requires_v2() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::Signoff,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Resp,
                target: slot(48, "RESP-X"),
            }],
            condition: CicsCondition::Respond {
                response: slot(48, "RESP-X"),
                response2: None,
            },
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands.push(CicsNamedOperand {
            name: CicsOperandName::SecurityUserId,
            value: CicsOperandValue::Literal(b"IBMUSER".to_vec()),
        });
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn verify_phrase_plan_keeps_secret_in_storage_and_requires_v2_tags() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::VerifyPhrase,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::SecurityUserId,
                    value: CicsOperandValue::Literal(b"IBMUSER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPhrase,
                    value: CicsOperandValue::Storage(slot(48, "PHRASE-X")),
                },
                CicsNamedOperand {
                    name: CicsOperandName::SecurityPhraseLen,
                    value: CicsOperandValue::Integer(16),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityInvalidCount,
                target: slot(49, "COUNT-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        plan.operands[1].value = CicsOperandValue::Literal(b"LONG-PHRASE-1234".to_vec());
        assert_eq!(
            encode_cics_effect_plan(&plan, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn query_security_v2_plan_round_trips_and_cannot_be_encoded_as_v1() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::QuerySecurity,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::ResClass,
                    value: CicsOperandValue::Literal(b"FACILITY".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::ResId,
                    value: CicsOperandValue::Literal(b"ITEM".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::ResIdLength,
                    value: CicsOperandValue::Integer(4),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::SecurityRead,
                target: slot(44, "READ-X"),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(&encoded[..6], b"MCEP\0\x02");
        assert_eq!(decode_cics_effect_plan(&encoded, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut malformed = encoded;
        malformed[6..8].copy_from_slice(&140_u16.to_be_bytes());
        assert_eq!(
            decode_cics_effect_plan(&malformed, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn unlock_token_plan_round_trips_and_rejects_forged_token_shape() {
        let token = slot(15, "FILE.TOKEN");
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Unlock,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Token,
                    value: CicsOperandValue::Storage(token),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        let legacy =
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION)
                .unwrap();
        assert_eq!(legacy[6], 75);
        let migrated = decode_cics_effect_plan(&legacy, CicsPlanLimits::default()).unwrap();
        assert_eq!(migrated, plan);
        assert_eq!(
            encode_cics_effect_plan(&migrated, CicsPlanLimits::default()),
            Ok(bytes)
        );
        let mut read = read_plan();
        read.outputs.push(CicsOutputBinding {
            name: CicsOutputName::Token,
            target: slot(15, "FILE.TOKEN"),
        });
        let read_bytes = encode_cics_effect_plan(&read, CicsPlanLimits::default()).unwrap();
        let decoded = decode_cics_effect_plan(&read_bytes, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            encode_cics_effect_plan(&decoded, CicsPlanLimits::default()),
            Ok(read_bytes)
        );
        let legacy_read =
            encode_cics_effect_plan_version(&read, CicsPlanLimits::default(), LEGACY_VERSION)
                .unwrap();
        let migrated_read =
            decode_cics_effect_plan(&legacy_read, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            encode_cics_effect_plan(&migrated_read, CicsPlanLimits::default()),
            encode_cics_effect_plan(&read, CicsPlanLimits::default())
        );
        let mut forged = plan;
        forged.operands[1].value = CicsOperandValue::Integer(1);
        assert_eq!(
            encode_cics_effect_plan(&forged, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    proptest! {
        #[test]
        fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = decode_cics_effect_plan(&bytes, CicsPlanLimits::default());
        }
    }
    #[test]
    fn web_service_tag_envelopes_are_unique_round_trip_and_v1_safe() {
        let operations = [
            CicsPlanOperation::InvokeService,
            CicsPlanOperation::SoapFaultAdd,
            CicsPlanOperation::SoapFaultCreate,
            CicsPlanOperation::SoapFaultDelete,
            CicsPlanOperation::WsaContextBuild,
            CicsPlanOperation::WsaContextDelete,
            CicsPlanOperation::WsaContextGet,
            CicsPlanOperation::WsaEprCreate,
        ];
        for (offset, operation) in operations.into_iter().enumerate() {
            let tag = 140 + offset as u16;
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        let operands = [
            CicsOperandName::Service,
            CicsOperandName::ServiceOperation,
            CicsOperandName::Uri,
            CicsOperandName::UriMap,
            CicsOperandName::Scope,
            CicsOperandName::ScopeLen,
            CicsOperandName::FaultCode,
            CicsOperandName::FaultCodeStr,
            CicsOperandName::FaultCodeLen,
            CicsOperandName::FaultString,
            CicsOperandName::FaultStrLen,
            CicsOperandName::NatLang,
            CicsOperandName::SoapRole,
            CicsOperandName::RoleLength,
            CicsOperandName::FaultActor,
            CicsOperandName::FaultActLen,
            CicsOperandName::Detail,
            CicsOperandName::DetailLength,
            CicsOperandName::FromCcsid,
            CicsOperandName::SubcodeStr,
            CicsOperandName::SubcodeLen,
            CicsOperandName::ContextType,
            CicsOperandName::Action,
            CicsOperandName::MessageId,
            CicsOperandName::RelatesUri,
            CicsOperandName::RelatesType,
            CicsOperandName::RelatesIndex,
            CicsOperandName::EprType,
            CicsOperandName::EprField,
            CicsOperandName::EprFrom,
            CicsOperandName::EprLength,
            CicsOperandName::FromCodepage,
            CicsOperandName::IntoCcsid,
            CicsOperandName::IntoCodepage,
            CicsOperandName::Address,
            CicsOperandName::RefParms,
            CicsOperandName::RefParmsLen,
            CicsOperandName::Metadata,
            CicsOperandName::MetadataLen,
        ];
        for (offset, operand) in operands.into_iter().enumerate() {
            let tag = 512 + offset as u16;
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        for tag in 512 + operands.len() as u16..=575 {
            assert_eq!(operand_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        let outputs = [
            CicsOutputName::WebAction,
            CicsOutputName::WebMessageId,
            CicsOutputName::WebRelatesUri,
            CicsOutputName::WebRelatesType,
            CicsOutputName::WebEprInto,
            CicsOutputName::WebEprSet,
            CicsOutputName::WebEprLength,
        ];
        for (offset, output) in outputs.into_iter().enumerate() {
            let tag = 568 + offset as u16;
            assert_eq!(output_tag(output), tag);
            assert_eq!(output_from_tag(tag), Ok(output));
        }
        for tag in 568 + outputs.len() as u16..=631 {
            assert_eq!(output_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 444..=507 {
            assert_eq!(option_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        let length = slot(3, "EPR-LEN");
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::WsaEprCreate,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::EprLength,
                    value: CicsOperandValue::Storage(length.clone()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Address,
                    value: CicsOperandValue::Literal(b"http://example.invalid".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![
                CicsOutputBinding {
                    name: CicsOutputName::WebEprInto,
                    target: slot(4, "EPR-OUT"),
                },
                CicsOutputBinding {
                    name: CicsOutputName::WebEprLength,
                    target: length,
                },
            ],
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(&bytes[..6], b"MCEP\0\x02");
        assert_eq!(
            decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut forged = plan;
        forged.outputs.push(CicsOutputBinding {
            name: CicsOutputName::WebEprSet,
            target: slot(5, "EPR-POINTER"),
        });
        assert_eq!(
            encode_cics_effect_plan(&forged, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn counter_v2_tags_round_trip_without_changing_v1() {
        for (operand, tag) in [
            (CicsOperandName::CounterName, 384),
            (CicsOperandName::CounterPool, 385),
            (CicsOperandName::CounterValue, 386),
            (CicsOperandName::CounterMinimum, 387),
            (CicsOperandName::CounterMaximum, 388),
            (CicsOperandName::CounterIncrement, 389),
            (CicsOperandName::CounterCompareMin, 390),
            (CicsOperandName::CounterCompareMax, 391),
        ] {
            assert_eq!(operand_tag(operand), tag);
            assert_eq!(operand_from_tag(tag), Ok(operand));
        }
        for (option, tag) in [
            (CicsPlanOption::CounterNoSuspend, 316),
            (CicsPlanOption::CounterReduce, 317),
            (CicsPlanOption::CounterWrap, 318),
        ] {
            assert_eq!(option_tag(option), tag);
            assert_eq!(option_from_tag(tag), Ok(option));
        }
        for (output, tag) in [
            (CicsOutputName::CounterValue, 440),
            (CicsOutputName::CounterMinimum, 441),
            (CicsOutputName::CounterMaximum, 442),
        ] {
            assert_eq!(output_tag(output), tag);
            assert_eq!(output_from_tag(tag), Ok(output));
        }
        for tag in 392..=447 {
            assert_eq!(operand_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 319..=379 {
            assert_eq!(option_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        for tag in 443..=503 {
            assert_eq!(output_from_tag(tag), Err(CicsPlanCodecProblem::Malformed));
        }
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::DefineCounter,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::CounterName,
                value: CicsOperandValue::Literal(b"TICKET".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let encoded = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(&encoded[..6], b"MCEP\0\x02");
        assert_eq!(decode_cics_effect_plan(&encoded, limits), Ok(plan.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&plan, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }
}
