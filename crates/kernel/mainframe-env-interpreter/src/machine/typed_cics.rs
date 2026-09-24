use super::*;
use mainframe_env_host_api::CicsResponse;
use mainframe_env_ir::{
    CICS_ASSIGN_OUTPUT_NAMES, CICS_CERTIFICATE_OUTPUT_NAMES, CICS_EXECUTABLE_DESCRIPTORS,
    CicsCertificateOutput, CicsCondition, CicsEffectPlan, CicsExecutableDescriptor,
    CicsOperandName, CicsOperandValue, CicsOperationContract, CicsOutputName, CicsPlanLimits,
    CicsPlanOperation, CicsPlanOption, CicsStorageSlot, CicsTcpipOutput, Effect, Module,
    OperationCatalog, OperationSchema, OperationSemanticContract, cics_executable_descriptor,
    cics_executable_descriptor_for_identity, cobol_layout_definition_identity,
    decode_cics_effect_plan, verify_semantic_contracts,
};

mod address;
mod assign;
mod certificate;
mod convert_time;
mod diagnostics;
mod legacy;
mod names;
mod output_write;
mod post;
mod registry;
mod response;
pub(super) use output_write::write_output;
mod retrieve;
mod runtime_validation;
mod slot_access;
use runtime_validation::validate_machine_slot;
mod spool_control;
mod storage64;
mod task_wait;
mod tcpip;
mod web_control;
mod web_service_control;
pub(super) use address::CicsAddressSet;
use diagnostics::{argument_summary, invalid_plan};
pub(super) use legacy::execute_legacy;
use names::SlotUse;
#[cfg(test)]
use registry::operation_schema;
use registry::{expected_effects, expected_operation};
pub(super) use registry::{operation_identities, validate_module_operations};
pub(super) use response::{drive_response, write_response_state, write_runtime_output};
use runtime_validation::validate_runtime_plan;
use slot_access::{plan_slots, read_integer_slot, read_slot};
pub(super) use storage64::Storage64Intent;

const PLAN_ATTRIBUTE: &str = "cics_plan";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CicsTarget {
    Legacy(String),
    Resolved(CicsStorageSlot),
}

pub(super) fn is_typed(operation: &Operation) -> bool {
    expected_operation(&operation.identity).is_some()
}

pub(super) use runtime_validation::into_payload_schema;

pub(super) fn abend_dump_disposition(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<AbendDumpDisposition, MachineProblem> {
    if operation != CicsOperation::Abend {
        return Ok(AbendDumpDisposition::Unspecified);
    }
    let Some(value) = response.outputs.get("ABEND.DUMP") else {
        // Retained responses written before this metadata was introduced do
        // not claim a dump decision.
        return Ok(AbendDumpDisposition::Unspecified);
    };
    if value.schema() != "mainframe-env.cics.abend-dump@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    match value.bytes() {
        b"requested" => Ok(AbendDumpDisposition::Requested),
        b"suppressed" => Ok(AbendDumpDisposition::Suppressed),
        _ => Err(MachineProblem::UnexpectedHostResult),
    }
}

pub(super) fn abend_outcome(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<Abend, MachineProblem> {
    let code = if operation == CicsOperation::Abend && !response.payload.bytes().is_empty() {
        String::from_utf8(response.payload.bytes().to_vec())
            .map_err(|_| MachineProblem::UnexpectedHostResult)?
    } else {
        response.condition.clone()
    };
    Ok(Abend {
        code,
        reason: Some(format!(
            "EIBRESP={} EIBRESP2={}",
            response.response, response.response2
        )),
        dump: abend_dump_disposition(operation, response)?,
    })
}

pub(super) fn suspension(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    state_bytes: usize,
) -> MachineDrive<EffectRequest> {
    let (kind, reissue) = match operation {
        CicsOperation::Enq => ("cics-enqueue", true),
        CicsOperation::Delay => ("cics-delay", true),
        CicsOperation::Retrieve => ("cics-retrieve", true),
        CicsOperation::WaitEvent | CicsOperation::WaitExternal | CicsOperation::WaitCics => {
            ("cics-event", true)
        }
        CicsOperation::WaitJournalName => ("cics-journal", true),
        CicsOperation::WaitJournalNum => ("cics-journal", true),
        operation if operation.is_counter() => ("cics-counter", true),
        CicsOperation::WriteJournalName => ("cics-journal", true),
        CicsOperation::WriteJournalNum => ("cics-journal", true),
        CicsOperation::WriteOperator => ("cics-operator", true),
        CicsOperation::ChangeTask | CicsOperation::Suspend => ("cics-scheduler", false),
        _ => ("cics-terminal", true),
    };
    if reissue {
        machine.pc = machine.pc.saturating_sub(1);
    }
    MachineDrive::Suspended(Suspension {
        kind: kind.into(),
        resume_token: format!(
            "{}:{}",
            machine.invocation.run_unit_id, machine.effect_sequence
        ),
        state_bytes: state_bytes as u64,
    })
}

pub(super) fn validate_machine(machine: &ReferenceMachine) -> Result<(), MachineProblem> {
    for operation in machine
        .operations
        .iter()
        .filter(|operation| is_typed(operation))
    {
        let plan = plan(operation)?;
        validate_declared_slots(operation, &plan)?;
        for slot in plan_slots(&plan)? {
            validate_machine_slot(machine, operation, &slot, SlotUse::Input)?;
        }
        for operand in &plan.operands {
            if let CicsOperandValue::Storage(slot) | CicsOperandValue::LengthOf(slot) =
                &operand.value
            {
                let slot_use = if matches!(operand.value, CicsOperandValue::Storage(_))
                    && matches!(
                        operand.name,
                        CicsOperandName::Length
                            | CicsOperandName::KeyLength
                            | CicsOperandName::Item
                    ) {
                    if matches!(
                        plan.operation,
                        CicsPlanOperation::DocumentCreate
                            | CicsPlanOperation::DocumentInsert
                            | CicsPlanOperation::DocumentSet
                    ) {
                        SlotUse::FullwordInput
                    } else {
                        SlotUse::HalfwordInput
                    }
                } else {
                    names::input_slot_use(operand.name)
                };
                validate_machine_slot(machine, operation, slot, slot_use)?;
            }
        }
        validate_address_set_slots(machine, operation, &plan)?;
        for output in &plan.outputs {
            let slot_use = names::output_slot_use(output.name);
            validate_machine_slot(machine, operation, &output.target, slot_use)?;
        }
    }
    Ok(())
}

pub(super) fn execute(
    machine: &mut ReferenceMachine,
    operation: &Operation,
) -> Result<Step, MachineProblem> {
    let plan = plan(operation)?;
    validate_declared_slots(operation, &plan)?;
    validate_runtime_plan(machine, operation, &plan)?;
    if plan.operation == CicsPlanOperation::WsaContextGet {
        web_service_control::release_previous_set(machine);
    }
    certificate::release_previous(machine);
    if plan.operation == CicsPlanOperation::ReadTemporaryStorage {
        retrieve::release_temporary_storage_set(machine);
    }
    if plan.operation == CicsPlanOperation::ExtractLogonMsg {
        retrieve::release_logon_message_set(machine);
    }
    if matches!(
        plan.operation,
        CicsPlanOperation::ReceivePartn
            | CicsPlanOperation::ReceiveMap
            | CicsPlanOperation::IssueReceive
    ) {
        retrieve::release_partition_receive_set(machine);
    }
    let host_operation = names::host_operation(plan.operation);
    let address_set = address::action(&plan)?;
    let mut arguments = task_wait::arguments(machine, &plan)?.unwrap_or_default();
    let mut operand_outputs = BTreeMap::new();
    for operand in &plan.operands {
        if matches!(
            operand.name,
            CicsOperandName::EventControlAddress | CicsOperandName::EcbList
        ) {
            continue;
        }
        let (schema, bytes) = match &operand.value {
            CicsOperandValue::Literal(bytes) if operand.name == CicsOperandName::Conditions => (
                if plan.operation == CicsPlanOperation::HandleCondition {
                    "mainframe-env.cics.condition-handlers@1"
                } else {
                    "mainframe-env.cics.condition-list@1"
                },
                bytes.clone(),
            ),
            CicsOperandValue::Literal(bytes) if operand.name == CicsOperandName::Aids => {
                ("mainframe-env.cics.aid-handlers@1", bytes.clone())
            }
            CicsOperandValue::Literal(bytes) => ("mainframe-env.cics.literal@1", bytes.clone()),
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::SecurityPassword
                        | CicsOperandName::SecurityNewPassword
                        | CicsOperandName::SecurityNewPhrase
                        | CicsOperandName::SecurityOidCard
                        | CicsOperandName::SecurityPhrase
                ) =>
            {
                ("mainframe-env.cics.secret@1", read_slot(machine, slot)?)
            }
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::CommareaPointer
                        | CicsOperandName::SetAddress
                        | CicsOperandName::SetPointer
                ) =>
            {
                (
                    "mainframe-env.cics.storage-target@1",
                    format!(
                        "{}:{}:{}",
                        machine.invocation.artifact.as_str(),
                        slot.storage.get(),
                        slot.qualified_layout_name
                    )
                    .into_bytes(),
                )
            }
            CicsOperandValue::Storage(slot) if operand.name == CicsOperandName::DataPointer => {
                retrieve::freemain_argument(machine, slot)?
            }
            CicsOperandValue::Storage(slot) if operand.name == CicsOperandName::DataArea => {
                retrieve::freemain_data_argument(machine, slot)?
            }
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::DataPointer64 | CicsOperandName::DataArea64
                ) =>
            {
                storage64::freemain_argument(machine, slot, operand.name)?
            }
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::LoadSet
                        | CicsOperandName::Entry
                        | CicsOperandName::LoadLength
                        | CicsOperandName::LoadFlength
                ) =>
            {
                let target = CicsTarget::Resolved(slot.clone());
                if matches!(
                    operand.name,
                    CicsOperandName::LoadSet | CicsOperandName::Entry
                ) {
                    arguments.extend(retrieve::allocation_arguments(
                        machine,
                        &target,
                        plan.operation,
                    )?);
                }
                operand_outputs.insert(
                    names::operand_for(plan.operation, operand.name).into(),
                    target,
                );
                (
                    "mainframe-env.cics.argument@1",
                    slot.qualified_layout_name.as_bytes().to_vec(),
                )
            }
            CicsOperandValue::Storage(slot) if operand.name == CicsOperandName::SpoolOutDescr => {
                spool_control::out_descriptor_argument(machine, slot)
            }
            CicsOperandValue::Storage(slot) if operand.name == CicsOperandName::MonitorData1 => {
                if let Some(bytes) = diagnostics::monitor_pointer_bytes(machine, slot)? {
                    arguments.insert(
                        "DATA1.BYTES".into(),
                        payload("mainframe-env.cics.monitor-data@1", bytes)?,
                    );
                }
                (
                    "mainframe-env.cics.storage-value@1",
                    read_slot(machine, slot)?,
                )
            }
            CicsOperandValue::Storage(slot) if operand.name == CicsOperandName::UsingAddress => (
                "mainframe-env.cics.storage-identity@1",
                format!(
                    "{}:{}:{}",
                    machine.invocation.artifact.as_str(),
                    slot.storage.get(),
                    slot.qualified_layout_name
                )
                .into_bytes(),
            ),
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::Length
                        | CicsOperandName::ControlCursor
                        | CicsOperandName::DestIdLength
                        | CicsOperandName::Subaddress
                        | CicsOperandName::VolumeLength
                        | CicsOperandName::NumRec
                        | CicsOperandName::KeyNumber
                        | CicsOperandName::DataLength
                        | CicsOperandName::KeyLength
                        | CicsOperandName::MaxLifetime
                        | CicsOperandName::Priority
                        | CicsOperandName::Abstime
                        | CicsOperandName::Interval
                        | CicsOperandName::StartTime
                        | CicsOperandName::Hours
                        | CicsOperandName::Minutes
                        | CicsOperandName::Seconds
                        | CicsOperandName::TimerDays
                        | CicsOperandName::TimerHours
                        | CicsOperandName::TimerMinutes
                        | CicsOperandName::TimerSeconds
                        | CicsOperandName::TimerYear
                        | CicsOperandName::TimerMonth
                        | CicsOperandName::TimerDayOfMonth
                        | CicsOperandName::TimerDayOfYear
                        | CicsOperandName::SignalFromLength
                        | CicsOperandName::Milliseconds
                        | CicsOperandName::Flength
                        | CicsOperandName::Flength64
                        | CicsOperandName::NumEvents
                        | CicsOperandName::Purgeability
                        | CicsOperandName::Item
                        | CicsOperandName::MajorVersion
                        | CicsOperandName::MinorVersion
                        | CicsOperandName::ListLength
                        | CicsOperandName::MaximumLength
                        | CicsOperandName::ElementNameLength
                        | CicsOperandName::ElementNamespaceLength
                        | CicsOperandName::TypeNameLength
                        | CicsOperandName::TypeNamespaceLength
                        | CicsOperandName::JournalReqId
                        | CicsOperandName::JournalNum
                        | CicsOperandName::JournalFlength
                        | CicsOperandName::JournalPfxLeng
                        | CicsOperandName::Token
                        | CicsOperandName::SpoolRecordLength
                        | CicsOperandName::SpoolMaxFlength
                        | CicsOperandName::SpoolFlength
                        | CicsOperandName::CounterValue
                        | CicsOperandName::CounterMinimum
                        | CicsOperandName::CounterMaximum
                        | CicsOperandName::TraceNum
                        | CicsOperandName::TraceFromLength
                        | CicsOperandName::MonitorPoint
                        | CicsOperandName::DumpLength
                        | CicsOperandName::DumpFlength
                        | CicsOperandName::DumpNumSegments
                        | CicsOperandName::WebUrlLength
                        | CicsOperandName::WebHostLength
                        | CicsOperandName::WebPathLength
                        | CicsOperandName::WebQueryStringLength
                        | CicsOperandName::WebMethodLength
                        | CicsOperandName::WebVersionLength
                        | CicsOperandName::WebRealmLength
                        | CicsOperandName::WebNameLength
                        | CicsOperandName::WebValueLength
                        | CicsOperandName::WebStatusCode
                        | CicsOperandName::WebStatusLength
                        | CicsOperandName::WebFromLength
                        | CicsOperandName::WebReceiveMaxLength
                        | CicsOperandName::WebReceiveStatusLength
                        | CicsOperandName::WebPortNumber
                        | CicsOperandName::BtsTimeout
                ) || web_service_control::numeric_operand(operand.name) =>
            {
                (
                    "mainframe-env.cics.decimal@1",
                    read_integer_slot(machine, slot)?.to_string().into_bytes(),
                )
            }
            CicsOperandValue::Storage(slot)
                if operand.name == CicsOperandName::Resource
                    && !plan
                        .operands
                        .iter()
                        .any(|value| value.name == CicsOperandName::Length) =>
            {
                (
                    "mainframe-env.cics.storage-identity@1",
                    format!(
                        "{}:{}:{}",
                        machine.invocation.artifact.as_str(),
                        slot.storage.get(),
                        slot.qualified_layout_name
                    )
                    .into_bytes(),
                )
            }
            CicsOperandValue::Storage(slot) => (
                if operand.name == CicsOperandName::SecurityTokenData {
                    "mainframe-env.cics.secret@1"
                } else {
                    "mainframe-env.cics.storage-value@1"
                },
                read_slot(machine, slot)?,
            ),
            CicsOperandValue::Integer(value) => (
                "mainframe-env.cics.decimal@1",
                value.to_string().into_bytes(),
            ),
            CicsOperandValue::LengthOf(slot) => (
                "mainframe-env.cics.decimal@1",
                read_slot(machine, slot)?.len().to_string().into_bytes(),
            ),
        };
        arguments.insert(
            names::operand_for(plan.operation, operand.name).into(),
            payload(schema, bytes)?,
        );
    }

    if plan.operation == CicsPlanOperation::DumpTransaction
        && plan
            .operands
            .iter()
            .any(|operand| operand.name == CicsOperandName::DumpSegmentList)
    {
        arguments.insert(
            "SEGMENTS".into(),
            payload(
                "mainframe-env.cics.dump-segments@1",
                diagnostics::dump_segments(machine, &plan)?,
            )?,
        );
    }
    let mut into = None;
    let mut outputs = operand_outputs;
    let mut response = None;
    let mut response2 = None;
    for output in &plan.outputs {
        let key = names::output(output.name);
        if !arguments.contains_key(key) {
            arguments.insert(
                key.into(),
                payload(
                    "mainframe-env.cics.argument@1",
                    output.target.qualified_layout_name.as_bytes().to_vec(),
                )?,
            );
        }
        let target = CicsTarget::Resolved(output.target.clone());
        web_service_control::output_arguments(machine, output.name, &target, &mut arguments)?;
        if let Some((name, capacity)) = web_control::output_capacity(machine, output.name, &target)?
        {
            arguments.insert(name, capacity);
        }
        if matches!(
            output.name,
            CicsOutputName::DigestResult | CicsOutputName::OperatorReply
        ) {
            let name = if output.name == CicsOutputName::DigestResult {
                "RESULT.MAXLENGTH"
            } else {
                "REPLY.MAXLENGTH"
            };
            arguments.insert(
                name.into(),
                payload(
                    "mainframe-env.cics.decimal@1",
                    resolved_slot(machine, &output.target)?
                        .length
                        .to_string()
                        .into_bytes(),
                )?,
            );
        }
        if let CicsOutputName::Tcpip(identity) = output.name {
            tcpip::add_output_arguments(machine, &mut arguments, key, identity, &output.target)?;
        }
        match output.name {
            CicsOutputName::LogonSet | CicsOutputName::PipList => {
                let capacity = retrieve::allocation_capacity(machine, &target)?;
                arguments.insert(
                    format!("{key}.MAXLENGTH"),
                    payload(
                        "mainframe-env.cics.decimal@1",
                        capacity.to_string().into_bytes(),
                    )?,
                );
                outputs.insert(key.into(), target);
            }
            CicsOutputName::LogonInto => {
                let CicsTarget::Resolved(slot) = &target else {
                    return Err(MachineProblem::UnexpectedHostResult);
                };
                arguments.insert(
                    "INTO.MAXLENGTH".into(),
                    payload(
                        "mainframe-env.cics.decimal@1",
                        resolved_slot(machine, slot)?
                            .length
                            .to_string()
                            .into_bytes(),
                    )?,
                );
                outputs.insert(key.into(), target);
            }
            CicsOutputName::ProcessName => {
                let CicsTarget::Resolved(slot) = &target else {
                    return Err(MachineProblem::UnexpectedHostResult);
                };
                arguments.insert(
                    "PROCNAME.MAXLENGTH".into(),
                    payload(
                        "mainframe-env.cics.decimal@1",
                        resolved_slot(machine, slot)?
                            .length
                            .to_string()
                            .into_bytes(),
                    )?,
                );
                outputs.insert(key.into(), target);
            }
            CicsOutputName::AttachProcess
            | CicsOutputName::AttachResource
            | CicsOutputName::AttachReturnProcess
            | CicsOutputName::AttachReturnResource
            | CicsOutputName::AttachQueue => {
                let CicsTarget::Resolved(slot) = &target else {
                    return Err(MachineProblem::UnexpectedHostResult);
                };
                arguments.insert(
                    format!("{key}.MAXLENGTH"),
                    payload(
                        "mainframe-env.cics.decimal@1",
                        resolved_slot(machine, slot)?
                            .length
                            .to_string()
                            .into_bytes(),
                    )?,
                );
                outputs.insert(key.into(), target);
            }
            CicsOutputName::AttachIuType
            | CicsOutputName::AttachDataStream
            | CicsOutputName::AttachRecordFormat
            | CicsOutputName::ConversationState
            | CicsOutputName::ConversationData
            | CicsOutputName::ConversationRetCode
            | CicsOutputName::LogonLength
            | CicsOutputName::ProcessLength
            | CicsOutputName::SyncLevel
            | CicsOutputName::PipLength
            | CicsOutputName::TctSysId
            | CicsOutputName::TctTermId => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::Abstime
            | CicsOutputName::SecurityRead
            | CicsOutputName::SecurityUpdate
            | CicsOutputName::SecurityControl
            | CicsOutputName::SecurityAlter
            | CicsOutputName::SecurityChangeTime
            | CicsOutputName::SecurityDaysLeft
            | CicsOutputName::SecurityEsmReason
            | CicsOutputName::SecurityEsmResp
            | CicsOutputName::SecurityExpiryTime
            | CicsOutputName::SecurityInvalidCount
            | CicsOutputName::SecurityLastUseTime
            | CicsOutputName::SecurityPassTicket
            | CicsOutputName::SecurityIsUserId
            | CicsOutputName::SecurityEncryptKey
            | CicsOutputName::SecurityOutTokenLength
            | CicsOutputName::SecurityEncryptLength
            | CicsOutputName::SecurityLangInUse
            | CicsOutputName::SecurityNatLangInUse
            | CicsOutputName::TimerStatus
            | CicsOutputName::EventName
            | CicsOutputName::BtsAny
            | CicsOutputName::BtsCompStatus
            | CicsOutputName::BtsChannel
            | CicsOutputName::BtsAbcode
            | CicsOutputName::SubEventName
            | CicsOutputName::EventType
            | CicsOutputName::FireStatus
            | CicsOutputName::Commarea
            | CicsOutputName::Milliseconds
            | CicsOutputName::Mmddyy
            | CicsOutputName::Mmddyyyy
            | CicsOutputName::Time
            | CicsOutputName::Yyddd
            | CicsOutputName::Yymmdd
            | CicsOutputName::Yyyymmdd
            | CicsOutputName::ReturnTransId
            | CicsOutputName::ReturnTermId
            | CicsOutputName::Queue
            | CicsOutputName::NumItems
            | CicsOutputName::DocumentToken
            | CicsOutputName::ElementName
            | CicsOutputName::ElementNameLength
            | CicsOutputName::ElementNamespace
            | CicsOutputName::ElementNamespaceLength
            | CicsOutputName::TypeName
            | CicsOutputName::TypeNameLength
            | CicsOutputName::TypeNamespace
            | CicsOutputName::TypeNamespaceLength
            | CicsOutputName::JournalReqId
            | CicsOutputName::Token
            | CicsOutputName::SpoolToken
            | CicsOutputName::SpoolToFlength
            | CicsOutputName::WebAction
            | CicsOutputName::WebMessageId
            | CicsOutputName::WebRelatesUri
            | CicsOutputName::WebRelatesType
            | CicsOutputName::WebEprInto
            | CicsOutputName::WebEprSet
            | CicsOutputName::WebEprLength
            | CicsOutputName::Partn
            | CicsOutputName::DumpId
            | CicsOutputName::WebSchemeName
            | CicsOutputName::WebHost
            | CicsOutputName::WebHostLength
            | CicsOutputName::WebHostType
            | CicsOutputName::WebPortNumber
            | CicsOutputName::WebPath
            | CicsOutputName::WebPathLength
            | CicsOutputName::WebQueryString
            | CicsOutputName::WebQueryStringLength
            | CicsOutputName::WebSessionToken
            | CicsOutputName::WebHttpVNum
            | CicsOutputName::WebHttpRNum
            | CicsOutputName::WebScheme
            | CicsOutputName::WebHttpMethod
            | CicsOutputName::WebMethodLength
            | CicsOutputName::WebHttpVersion
            | CicsOutputName::WebVersionLength
            | CicsOutputName::WebRequestType
            | CicsOutputName::WebUriMap
            | CicsOutputName::WebRealm
            | CicsOutputName::WebRealmLength
            | CicsOutputName::WebValue
            | CicsOutputName::WebValueLength
            | CicsOutputName::WebBrowseName
            | CicsOutputName::WebBrowseNameLength
            | CicsOutputName::WebRetrieveDocumentToken
            | CicsOutputName::WebReceiveInto
            | CicsOutputName::WebReceiveLength
            | CicsOutputName::WebReceiveStatusCode
            | CicsOutputName::WebReceiveStatusText
            | CicsOutputName::WebReceiveStatusLength
            | CicsOutputName::WebReceiveMediaType
            | CicsOutputName::WebReceiveBodyCharset
            | CicsOutputName::WebConverseInto
            | CicsOutputName::WebConverseToLength
            | CicsOutputName::WebConverseStatusCode
            | CicsOutputName::WebConverseStatusText
            | CicsOutputName::WebConverseStatusLength
            | CicsOutputName::WebConverseMediaType
            | CicsOutputName::WebConverseBodyCharset
            | CicsOutputName::Assign(_) => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::Certificate(_) => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::Tcpip(_) => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::Into => {
                if matches!(
                    plan.operation,
                    CicsPlanOperation::Retrieve
                        | CicsPlanOperation::ReadTransientData
                        | CicsPlanOperation::ReceivePartn
                        | CicsPlanOperation::IssueReceive
                        | CicsPlanOperation::DocumentRetrieve
                        | CicsPlanOperation::SpoolRead
                ) {
                    let CicsTarget::Resolved(slot) = &target else {
                        return Err(MachineProblem::UnexpectedHostResult);
                    };
                    arguments.insert(
                        "INTO.MAXLENGTH".into(),
                        payload(
                            "mainframe-env.cics.decimal@1",
                            resolved_slot(machine, slot)?
                                .length
                                .to_string()
                                .into_bytes(),
                        )?,
                    );
                }
                into = Some(target);
            }
            CicsOutputName::SetPointer => {
                arguments.extend(retrieve::allocation_arguments(
                    machine,
                    &target,
                    plan.operation,
                )?);
                outputs.insert(key.into(), target);
            }
            CicsOutputName::SecurityOutToken | CicsOutputName::SecurityEncryptPassTicket => {
                arguments.extend(retrieve::allocation_arguments(
                    machine,
                    &target,
                    plan.operation,
                )?);
                outputs.insert(key.into(), target);
            }
            CicsOutputName::SetPointer64 => {
                arguments.extend(retrieve::allocation64_arguments(
                    machine,
                    &target,
                    arguments.get("LOCATION"),
                )?);
                outputs.insert(key.into(), target);
            }
            CicsOutputName::DigestResult
            | CicsOutputName::Field
            | CicsOutputName::Ridfld
            | CicsOutputName::OperatorReply
            | CicsOutputName::OperatorReplyLength => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::Resp => response = Some(target),
            CicsOutputName::Resp2 => response2 = Some(target),
            CicsOutputName::Length => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::DocumentSize => {
                outputs.insert(key.into(), target);
            }
            CicsOutputName::CounterValue
            | CicsOutputName::CounterMinimum
            | CicsOutputName::CounterMaximum => {
                arguments.insert(
                    key.into(),
                    payload("mainframe-env.cics.output@1", Vec::new())?,
                );
                outputs.insert(key.into(), target);
            }
        }
    }
    if plan.operation == CicsPlanOperation::WriteTemporaryStorage
        && !plan.options.contains(&CicsPlanOption::RewriteTemporary)
        && let Some(CicsOperandValue::Storage(slot)) = plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::Item)
            .map(|operand| &operand.value)
    {
        outputs.insert("ITEM".into(), CicsTarget::Resolved(slot.clone()));
    }
    for option in &plan.options {
        arguments.insert(
            format!("OPTION.{}", names::option(*option)),
            payload("mainframe-env.cics.option@1", Vec::new())?,
        );
    }
    if plan.operation == CicsPlanOperation::Delay {
        arguments.insert(
            "DELAY.ID".into(),
            payload(
                "mainframe-env.cics.delay-id@1",
                format!("{}:{}", machine.invocation.run_unit_id, machine.pc).into_bytes(),
            )?,
        );
    }
    if plan.operation == CicsPlanOperation::WriteOperator {
        arguments.insert(
            "OPERATOR.ID".into(),
            payload(
                "mainframe-env.cics.operator-id@1",
                format!("{}:{}", machine.invocation.run_unit_id, machine.pc).into_bytes(),
            )?,
        );
    }
    if matches!(
        plan.operation,
        CicsPlanOperation::FetchAny | CicsPlanOperation::FetchChild
    ) {
        arguments.insert(
            "FETCH.ID".into(),
            payload(
                "mainframe-env.cics.fetch-id@1",
                format!("{}:{}", machine.invocation.run_unit_id, machine.pc).into_bytes(),
            )?,
        );
    }

    let condition_policy = match &plan.condition {
        CicsCondition::Default => CicsConditionPolicy::Default,
        CicsCondition::NoHandle => CicsConditionPolicy::NoHandle,
        CicsCondition::Respond {
            response,
            response2,
        } => CicsConditionPolicy::Respond {
            response_field: response.qualified_layout_name.clone(),
            response2_field: response2
                .as_ref()
                .map(|slot| slot.qualified_layout_name.clone()),
        },
    };
    let no_handle = matches!(plan.condition, CicsCondition::NoHandle);

    let mut mutation = (host_operation.is_mutating()
        || matches!(
            host_operation,
            CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
        ) && arguments.contains_key("TOKEN"))
    .then(|| machine.mutation())
    .transpose()?;
    if let Some(mutation) = &mut mutation {
        mutation.transaction = Some(
            machine
                .invocation
                .bindings
                .get("cics.transaction")
                .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
                .unwrap_or_else(|| "DEFAULT".into()),
        );
    }
    let argument_summary = argument_summary(&arguments);
    let storage64_intent = storage64::pending_intent(machine, host_operation, &arguments)?;
    machine.effect(
        HostRequest::Cics(CicsRequest {
            operation: host_operation,
            arguments,
            condition_policy,
            mutation,
        }),
        PendingKind::Cics {
            operation: host_operation,
            storage64_intent,
            argument_summary,
            into,
            outputs,
            response,
            response2,
            address_set,
            no_handle,
        },
    )
}

fn legacy_condition_policy(
    args: &[String],
    arguments: &BTreeMap<String, BoundedPayload>,
) -> Result<CicsConditionPolicy, MachineProblem> {
    let response = arguments.get("RESP");
    if response.is_none() && arguments.contains_key("RESP2") {
        return Err(MachineProblem::UnsupportedForm);
    }
    if let Some(response) = response {
        // IBM's common command format defines RESP as implying NOHANDLE while
        // still updating the response area. Preserve that binding when the
        // redundant NOHANDLE keyword is also present.
        Ok(CicsConditionPolicy::Respond {
            response_field: String::from_utf8_lossy(response.bytes()).into_owned(),
            response2_field: arguments
                .get("RESP2")
                .map(|value| String::from_utf8_lossy(value.bytes()).into_owned()),
        })
    } else if args.iter().any(|arg| arg.eq_ignore_ascii_case("NOHANDLE")) {
        Ok(CicsConditionPolicy::NoHandle)
    } else {
        Ok(CicsConditionPolicy::Default)
    }
}

pub(super) fn write_target(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &CobolValue,
) -> Result<(), MachineProblem> {
    match target {
        CicsTarget::Legacy(target) => {
            let tokens = target
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            machine.write_reference_value(&tokens, value)
        }
        CicsTarget::Resolved(slot) => write_resolved(machine, slot, value),
    }
}

fn resolved_slot(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<ResolvedReference, MachineProblem> {
    Ok(ResolvedReference {
        layout: machine
            .layouts
            .get(&slot.qualified_layout_name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?,
        offset: 0,
        length: machine
            .views_by_id
            .get(&slot.storage)
            .ok_or(MachineProblem::UnknownStorage)?
            .length,
    })
}

fn write_resolved(
    machine: &mut ReferenceMachine,
    slot: &CicsStorageSlot,
    value: &CobolValue,
) -> Result<(), MachineProblem> {
    let reference = resolved_slot(machine, slot)?;
    if reference.layout.dynamic {
        let bytes = match value {
            CobolValue::Bytes(bytes) => bytes.clone(),
            CobolValue::Decimal(value) => decimal_string(*value).into_bytes(),
        };
        return machine.write_dynamic(&reference.layout, &bytes);
    }
    let bytes = match value {
        CobolValue::Bytes(bytes)
            if is_pointer_like(reference.layout.category)
                && bytes.iter().all(|byte| *byte == 0) =>
        {
            vec![0; reference.length]
        }
        CobolValue::Bytes(bytes) => FixedValue::fit(
            bytes,
            reference.length,
            reference.length == reference.layout.length && reference.layout.justified_right,
        )
        .bytes()
        .to_vec(),
        CobolValue::Decimal(value)
            if reference.length == reference.layout.length
                && is_numeric(reference.layout.category) =>
        {
            encode_decimal(
                &reference.layout,
                if matches!(
                    reference.layout.category,
                    LayoutCategory::FloatShort | LayoutCategory::FloatLong
                ) {
                    *value
                } else {
                    decimal_rescale(*value, reference.layout.scale)?
                },
            )?
        }
        CobolValue::Decimal(value) => {
            FixedValue::fit(decimal_string(*value).as_bytes(), reference.length, false)
                .bytes()
                .to_vec()
        }
    };
    machine.write_reference(&reference, &bytes)
}

fn plan(operation: &Operation) -> Result<CicsEffectPlan, MachineProblem> {
    let expected = expected_operation(&operation.identity)
        .ok_or_else(|| invalid_plan("operation identity is not a supported typed CICS route"))?;
    if !operation.operands.is_empty()
        || !operation.results.is_empty()
        || operation.effects != expected_effects(expected)
    {
        return Err(invalid_plan("operation signature or effects are invalid"));
    }
    if operation.attributes.contains_key("arguments")
        || operation.attributes.contains_key("control_text")
        || operation
            .attributes
            .keys()
            .any(|name| name.starts_with("arg_"))
    {
        return Err(invalid_plan(
            "legacy arguments and control text attributes are forbidden",
        ));
    }
    let bytes = match operation.attributes.get(PLAN_ATTRIBUTE) {
        Some(Attribute::Bytes(bytes)) => bytes,
        _ => return Err(invalid_plan("missing bytes attribute cics_plan")),
    };
    let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default())
        .map_err(|problem| invalid_plan(&problem.to_string()))?;
    if expected != plan.operation {
        return Err(invalid_plan(
            "operation identity does not match plan operation",
        ));
    }
    Ok(plan)
}

fn validate_declared_slots(
    operation: &Operation,
    plan: &CicsEffectPlan,
) -> Result<(), MachineProblem> {
    let expected = plan_slots(plan)?
        .into_iter()
        .map(|slot| slot.storage)
        .collect::<BTreeSet<_>>();
    let mut declared = BTreeSet::new();
    for reference in &operation.storage {
        if reference.offset != 0 || !declared.insert(reference.storage) {
            return Err(invalid_plan(
                "operation storage declarations must be unique offset-zero slots",
            ));
        }
    }
    if declared != expected {
        return Err(invalid_plan(
            "operation storage declarations do not exactly match plan slots",
        ));
    }
    Ok(())
}

fn validate_address_set_slots(
    machine: &ReferenceMachine,
    operation: &Operation,
    plan: &CicsEffectPlan,
) -> Result<(), MachineProblem> {
    for operand in &plan.operands {
        let CicsOperandValue::Storage(slot) = &operand.value else {
            continue;
        };
        let slot_use = match operand.name {
            CicsOperandName::CommareaPointer => SlotUse::PointerOutput,
            CicsOperandName::SetAddress => SlotUse::AddressOutput,
            CicsOperandName::SetPointer => SlotUse::PointerOutput,
            CicsOperandName::UsingAddress => SlotUse::AddressInput,
            CicsOperandName::UsingPointer => SlotUse::PointerInput,
            _ => continue,
        };
        validate_machine_slot(machine, operation, slot, slot_use)?;
        if operand.name == CicsOperandName::CommareaPointer
            && resolved_slot(machine, slot)?.length != 4
        {
            return Err(invalid_plan(
                "ADDRESS COMMAREA output must be a four-byte pointer",
            ));
        }
    }
    Ok(())
}

fn payload(schema: &str, bytes: Vec<u8>) -> Result<BoundedPayload, MachineProblem> {
    BoundedPayload::new(schema, bytes, InvocationLimits::default())
        .map_err(|_| MachineProblem::ResourceExhausted)
}

#[cfg(test)]
pub(super) use legacy::legacy_arguments;

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_ir::{
        CicsNamedOperand, CicsOutputBinding, IrLimits, Module, ModuleBuilder, StorageReference,
        encode_cics_effect_plan,
    };

    fn syncpoint_plan() -> CicsEffectPlan {
        CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        }
    }

    fn read_plan() -> CicsEffectPlan {
        let mut ids = ModuleBuilder::new(IrLimits::default());
        let into = ids.add_storage("into-x", 1, None).unwrap();
        CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Literal(b"003".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: into,
                    qualified_layout_name: "INTO-X".into(),
                },
            }],
            condition: CicsCondition::Default,
        }
    }

    fn machine_with_alphanumeric_slot(
        name: &str,
        length: usize,
    ) -> (ReferenceMachine, CicsStorageSlot) {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let storage = builder
            .add_storage(name, u64::try_from(length).unwrap(), None)
            .unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text(name.into())),
                    ("simple_name".into(), Attribute::Text(name.into())),
                    ("category".into(), Attribute::Text("alphanumeric".into())),
                    ("picture".into(), Attribute::Text(format!("X({length})"))),
                    ("digits".into(), Attribute::Integer(0)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(length as i64)),
                    ("element_length".into(), Attribute::Integer(length as i64)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        let machine = ReferenceMachine::from_binary(
            &bytes,
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        (
            machine,
            CicsStorageSlot {
                storage,
                qualified_layout_name: name.into(),
            },
        )
    }

    /// Issue #207: compact FORMATTIME output leaves a wider field's suffix unchanged.
    #[test]
    fn compact_formattime_output_preserves_wider_field_suffix() {
        let (mut machine, slot) = machine_with_alphanumeric_slot("DATE-X", 8);
        write_resolved(
            &mut machine,
            &slot,
            &CobolValue::Bytes(b"XXXXXXXX".to_vec()),
        )
        .unwrap();
        write_output(
            &mut machine,
            CicsOperation::FormatTime,
            "MMDDYY",
            &CicsTarget::Resolved(slot.clone()),
            &payload("mainframe-env.cics.payload@1", b"083026".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            machine.read_reference(&resolved_slot(&machine, &slot).unwrap()),
            Ok(b"083026XX".to_vec())
        );

        let (mut compact_machine, compact_slot) = machine_with_alphanumeric_slot("TIME-X", 6);
        write_output(
            &mut compact_machine,
            CicsOperation::FormatTime,
            "TIME",
            &CicsTarget::Resolved(compact_slot.clone()),
            &payload("mainframe-env.cics.payload@1", b"123456".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            compact_machine
                .read_reference(&resolved_slot(&compact_machine, &compact_slot).unwrap()),
            Ok(b"123456".to_vec())
        );
    }

    #[test]
    fn readq_ts_set_allocation_expires_at_the_next_readq() {
        let (mut machine, slot) = machine_with_alphanumeric_slot("PTR-X", 4);
        write_output(
            &mut machine,
            CicsOperation::ReadTemporaryStorage,
            "SET",
            &CicsTarget::Resolved(slot.clone()),
            &payload("mainframe-env.cics.payload@1", b"ITEM".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        let pointer = machine
            .read_reference(&resolved_slot(&machine, &slot).unwrap())
            .unwrap();
        let (base, offset) = machine.decode_address(&pointer).unwrap().unwrap();
        assert_eq!(offset, 0);
        assert!(!machine.freed_allocations.contains(&base));
        retrieve::release_temporary_storage_set(&mut machine);
        assert!(machine.freed_allocations.contains(&base));
    }

    #[test]
    fn extract_logonmsg_set_pointer_expires_at_the_next_extract() {
        let (mut machine, slot) = machine_with_alphanumeric_slot("LOGON-PTR", 4);
        write_output(
            &mut machine,
            CicsOperation::ExtractLogonMsg,
            "SET",
            &CicsTarget::Resolved(slot.clone()),
            &payload("mainframe-env.cics.payload@1", b"LOGON".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        let pointer = machine
            .read_reference(&resolved_slot(&machine, &slot).unwrap())
            .unwrap();
        let (base, offset) = machine.decode_address(&pointer).unwrap().unwrap();
        assert_eq!(offset, 0);
        assert!(!machine.freed_allocations.contains(&base));
        retrieve::release_logon_message_set(&mut machine);
        assert!(machine.freed_allocations.contains(&base));
        write_output(
            &mut machine,
            CicsOperation::ExtractLogonMsg,
            "SET",
            &CicsTarget::Resolved(slot.clone()),
            &payload("mainframe-env.cics.pointer-null@1", Vec::new()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            machine.read_reference(&resolved_slot(&machine, &slot).unwrap()),
            Ok(vec![0; 4])
        );
    }

    #[test]
    fn extract_process_pip_length_uses_full_halfword_and_checks_host_bounds() {
        let (mut machine, slot) = machine_with_alphanumeric_slot("PIP-LEN", 2);
        let target = CicsTarget::Resolved(slot.clone());
        write_output(
            &mut machine,
            CicsOperation::ExtractProcess,
            "PIPLENGTH",
            &target,
            &payload("mainframe-env.cics.decimal@1", b"32763".to_vec()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            machine.read_reference(&resolved_slot(&machine, &slot).unwrap()),
            Ok(vec![0x7f, 0xfb])
        );
        for (operation, value) in [
            (CicsOperation::ExtractProcess, b"32764".as_slice()),
            (CicsOperation::GdsExtractProcess, b"764".as_slice()),
        ] {
            assert_eq!(
                write_output(
                    &mut machine,
                    operation,
                    "PIPLENGTH",
                    &target,
                    &payload("mainframe-env.cics.decimal@1", value.to_vec()).unwrap(),
                    None,
                ),
                Err(MachineProblem::UnexpectedHostResult)
            );
        }
    }

    #[test]
    fn legacy_resp_binding_takes_precedence_over_nohandle() {
        let tokens = [
            "EXEC", "CICS", "ASKTIME", "NOHANDLE", "RESP", "(", "RESP-X", ")", "RESP2", "(",
            "RESP2-X", ")", "END-EXEC",
        ]
        .map(str::to_string);
        let arguments = legacy_arguments(&tokens).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&tokens, &arguments),
            Ok(CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            })
        );

        let nohandle = ["EXEC", "CICS", "ASKTIME", "NOHANDLE", "END-EXEC"].map(str::to_string);
        let arguments = legacy_arguments(&nohandle).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&nohandle, &arguments),
            Ok(CicsConditionPolicy::NoHandle)
        );
    }

    #[test]
    fn legacy_resp2_without_resp_fails_closed() {
        let tokens = [
            "EXEC", "CICS", "ASKTIME", "RESP2", "(", "RESP2-X", ")", "END-EXEC",
        ]
        .map(str::to_string);
        let arguments = legacy_arguments(&tokens).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&tokens, &arguments),
            Err(MachineProblem::UnsupportedForm)
        );
    }

    #[test]
    fn cics_abend_dump_metadata_is_typed_and_backward_compatible() {
        let response = |schema: &str, value: &[u8]| CicsResponse {
            disposition: CicsDisposition::Abended,
            condition: "ERROR".into(),
            response: 27,
            response2: 0,
            applid: "MEAPPL".into(),
            sysid: "MESYS".into(),
            transaction: "MENU".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: payload("mainframe-env.cics.payload@1", b"B001".to_vec()).unwrap(),
            outputs: BTreeMap::from([(
                "ABEND.DUMP".into(),
                payload(schema, value.to_vec()).unwrap(),
            )]),
            unit_of_work: None,
        };
        assert_eq!(
            abend_dump_disposition(
                CicsOperation::Abend,
                &response("mainframe-env.cics.abend-dump@1", b"requested"),
            ),
            Ok(AbendDumpDisposition::Requested)
        );
        assert_eq!(
            abend_outcome(
                CicsOperation::Abend,
                &response("mainframe-env.cics.abend-dump@1", b"requested"),
            ),
            Ok(Abend {
                code: "B001".into(),
                reason: Some("EIBRESP=27 EIBRESP2=0".into()),
                dump: AbendDumpDisposition::Requested,
            })
        );
        assert_eq!(
            abend_dump_disposition(
                CicsOperation::Abend,
                &response("mainframe-env.cics.abend-dump@1", b"suppressed"),
            ),
            Ok(AbendDumpDisposition::Suppressed)
        );
        assert_eq!(
            abend_dump_disposition(
                CicsOperation::Abend,
                &response("mainframe-env.cics.payload@1", b"requested"),
            ),
            Err(MachineProblem::UnexpectedHostResult)
        );
        let mut legacy = response("mainframe-env.cics.abend-dump@1", b"requested");
        legacy.outputs.clear();
        assert_eq!(
            abend_dump_disposition(CicsOperation::Abend, &legacy),
            Ok(AbendDumpDisposition::Unspecified)
        );
    }

    fn module(
        identity: OperationIdentity,
        plan: &CicsEffectPlan,
        effects: Vec<Effect>,
        legacy_arguments: bool,
        extra_storage: bool,
        transform: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> Module {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let storage = extra_storage.then(|| builder.add_storage("extra", 1, None).unwrap());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let bytes = transform(encode_cics_effect_plan(plan, CicsPlanLimits::default()).unwrap());
        let mut attributes = BTreeMap::from([(PLAN_ATTRIBUTE.into(), Attribute::Bytes(bytes))]);
        if legacy_arguments {
            attributes.insert("arguments".into(), Attribute::Bytes(Vec::new()));
        }
        builder
            .add_operation(
                block,
                identity,
                Vec::new(),
                0,
                attributes,
                effects,
                storage
                    .map(|storage| {
                        vec![StorageReference {
                            storage,
                            offset: 0,
                            length: 1,
                        }]
                    })
                    .unwrap_or_default(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[test]
    fn typed_cics_plan_identity_signature_and_storage_are_fail_closed() {
        let syncpoint = syncpoint_plan();
        let identity = operation_identities()[4].clone();
        assert!(
            super::super::validate_module(&module(
                identity.clone(),
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                false,
                |bytes| bytes,
            ))
            .is_ok()
        );
        for invalid in [
            module(
                identity.clone(),
                &read_plan(),
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                false,
                |bytes| bytes,
            ),
            module(
                identity.clone(),
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                true,
                false,
                |bytes| bytes,
            ),
            module(
                identity.clone(),
                &syncpoint,
                vec![Effect::Transaction],
                false,
                false,
                |bytes| bytes,
            ),
            module(
                identity,
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                true,
                |bytes| bytes,
            ),
        ] {
            assert!(matches!(
                super::super::validate_module(&invalid),
                Err(MachineProblem::InvalidArtifact(_))
            ));
        }
    }

    #[test]
    fn malformed_plan_and_unregistered_major_are_rejected() {
        let syncpoint = syncpoint_plan();
        let malformed = module(
            operation_identities()[4].clone(),
            &syncpoint,
            expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
            false,
            false,
            |mut bytes| {
                bytes.push(0);
                bytes
            },
        );
        assert!(matches!(
            super::super::validate_module(&malformed),
            Err(MachineProblem::InvalidArtifact(_))
        ));
        let wrong_major = module(
            OperationIdentity::new("cics.recovery", "syncpoint", 2).unwrap(),
            &syncpoint,
            expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
            false,
            false,
            |bytes| bytes,
        );
        assert!(matches!(
            super::super::validate_module(&wrong_major),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn dialect_descriptors_drive_interpreter_registry_and_contracts() {
        assert_eq!(
            operation_identities().into_iter().collect::<BTreeSet<_>>(),
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.identity())
                .collect()
        );
        for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
            let identity = descriptor.identity();
            assert_eq!(expected_operation(&identity), Some(descriptor.operation));
            assert_eq!(expected_effects(descriptor.operation), descriptor.effects);
            let schema = operation_schema(descriptor);
            assert_eq!(schema.identity, identity);
            assert_eq!(
                schema.allowed_effects,
                descriptor.effects.iter().copied().collect()
            );
            assert_eq!(
                schema.runtime_import.as_deref(),
                Some(descriptor.runtime_import)
            );
            assert_eq!(
                schema.semantic_contract,
                OperationSemanticContract::CicsEffect(CicsOperationContract {
                    plan_attribute: PLAN_ATTRIBUTE.into(),
                    expected_operation: Some(descriptor.operation),
                    layout_definition_operation: Some(
                        OperationIdentity::new("mainframe.core.cobol", "define", 1).unwrap(),
                    ),
                })
            );
        }
    }

    #[test]
    fn selected_unlock_route_emits_a_mutating_typed_host_effect() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Unlock,
            operands: vec![mainframe_env_ir::CicsNamedOperand {
                name: CicsOperandName::File,
                value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
            }],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let descriptor = cics_executable_descriptor(CicsPlanOperation::Unlock);
        let module = module(
            descriptor.identity(),
            &plan,
            descriptor.effects.to_vec(),
            false,
            false,
            |bytes| bytes,
        );
        assert!(super::super::validate_module(&module).is_ok());
        let bytes = mainframe_env_ir::encode_binary(&module, CodecLimits::default()).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            &bytes,
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        let operation = machine
            .operations
            .iter()
            .find(|operation| operation.identity == descriptor.identity())
            .unwrap()
            .clone();
        let Step::Effect(effect) = execute(&mut machine, &operation).unwrap() else {
            panic!("selected UNLOCK route must emit one host effect");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("selected UNLOCK route must use the CICS host boundary");
        };
        assert_eq!(request.operation, CicsOperation::Unlock);
        assert!(request.is_mutating());
        assert!(request.mutation.is_some());
        assert_eq!(request.arguments["FILE"].bytes(), b"ACCTDAT");
    }

    #[test]
    fn selected_retrieve_without_length_sends_destination_capacity() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let storage = builder.add_storage("MSG-X", 684, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("MSG-X".into())),
                    ("simple_name".into(), Attribute::Text("MSG-X".into())),
                    ("category".into(), Attribute::Text("alphanumeric".into())),
                    ("picture".into(), Attribute::Text("X(684)".into())),
                    ("digits".into(), Attribute::Integer(0)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(684)),
                    ("element_length".into(), Attribute::Integer(684)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let descriptor = cics_executable_descriptor(CicsPlanOperation::Retrieve);
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Retrieve,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::NoHandle]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage,
                    qualified_layout_name: "MSG-X".into(),
                },
            }],
            condition: CicsCondition::NoHandle,
        };
        builder
            .add_operation(
                block,
                descriptor.identity(),
                Vec::new(),
                0,
                BTreeMap::from([(
                    PLAN_ATTRIBUTE.into(),
                    Attribute::Bytes(
                        encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                    ),
                )]),
                descriptor.effects.to_vec(),
                vec![StorageReference {
                    storage,
                    offset: 0,
                    length: 684,
                }],
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let module = builder.finish().unwrap();
        assert!(super::super::validate_module(&module).is_ok());
        let bytes = mainframe_env_ir::encode_binary(&module, CodecLimits::default()).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            &bytes,
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        let selected = machine
            .operations
            .iter()
            .find(|operation| operation.identity == descriptor.identity())
            .unwrap()
            .clone();
        let Step::Effect(effect) = execute(&mut machine, &selected).unwrap() else {
            panic!("selected RETRIEVE must emit a host effect");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("selected RETRIEVE must use CICS");
        };
        assert_eq!(request.operation, CicsOperation::Retrieve);
        assert_eq!(request.arguments["INTO.MAXLENGTH"].bytes(), b"684");
        assert!(!request.arguments.contains_key("LENGTH"));
        assert_eq!(request.condition_policy, CicsConditionPolicy::NoHandle);
    }

    #[test]
    fn selected_timer_and_signal_routes_emit_distinct_mutating_host_effects() {
        for (plan, operation, operand, expected) in [
            (
                CicsEffectPlan {
                    operation: CicsPlanOperation::DefineTimer,
                    operands: vec![
                        mainframe_env_ir::CicsNamedOperand {
                            name: CicsOperandName::Timer,
                            value: CicsOperandValue::Literal(b"CLOCK".to_vec()),
                        },
                        mainframe_env_ir::CicsNamedOperand {
                            name: CicsOperandName::TimerSeconds,
                            value: CicsOperandValue::Integer(5),
                        },
                    ],
                    options: BTreeSet::from([CicsPlanOption::TimerAfter]),
                    outputs: Vec::new(),
                    condition: CicsCondition::Default,
                },
                CicsOperation::DefineTimer,
                "TIMER",
                b"CLOCK".as_slice(),
            ),
            (
                CicsEffectPlan {
                    operation: CicsPlanOperation::SignalEvent,
                    operands: vec![mainframe_env_ir::CicsNamedOperand {
                        name: CicsOperandName::Event,
                        value: CicsOperandValue::Literal(b"ORDER:GO".to_vec()),
                    }],
                    options: BTreeSet::new(),
                    outputs: Vec::new(),
                    condition: CicsCondition::Default,
                },
                CicsOperation::SignalEvent,
                "EVENT",
                b"ORDER:GO".as_slice(),
            ),
        ] {
            let descriptor = cics_executable_descriptor(plan.operation);
            let module = module(
                descriptor.identity(),
                &plan,
                descriptor.effects.to_vec(),
                false,
                false,
                |bytes| bytes,
            );
            assert!(super::super::validate_module(&module).is_ok());
            let bytes = mainframe_env_ir::encode_binary(&module, CodecLimits::default()).unwrap();
            let mut machine = ReferenceMachine::from_binary(
                &bytes,
                super::super::tests::invocation(),
                CodecLimits::default(),
            )
            .unwrap();
            let selected = machine
                .operations
                .iter()
                .find(|candidate| candidate.identity == descriptor.identity())
                .unwrap()
                .clone();
            let Step::Effect(effect) = execute(&mut machine, &selected).unwrap() else {
                panic!("selected event route must emit a host effect");
            };
            let HostRequest::Cics(request) = effect.request else {
                panic!("selected event route must use CICS");
            };
            assert_eq!(request.operation, operation);
            assert!(request.is_mutating() && request.mutation.is_some());
            assert_eq!(request.arguments[operand].bytes(), expected);
        }
    }

    #[test]
    fn equal_alias_views_cannot_substitute_a_different_storage_identity() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let backing = builder.add_storage("backing", 3, None).unwrap();
        let alias = |storage| StorageReference {
            storage,
            offset: 0,
            length: 3,
        };
        let wrong = builder.add_storage("a", 3, Some(alias(backing))).unwrap();
        let named = builder.add_storage("b", 3, Some(alias(backing))).unwrap();
        assert_ne!(wrong, named);
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("B".into())),
                    ("simple_name".into(), Attribute::Text("B".into())),
                    ("category".into(), Attribute::Text("alphanumeric".into())),
                    ("picture".into(), Attribute::Text(String::new())),
                    ("digits".into(), Attribute::Integer(0)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(3)),
                    ("element_length".into(), Attribute::Integer(3)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(CicsStorageSlot {
                        storage: wrong,
                        qualified_layout_name: "B".into(),
                    }),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: named,
                    qualified_layout_name: "B".into(),
                },
            }],
            condition: CicsCondition::Default,
        };
        builder
            .add_operation(
                block,
                operation_identities()[2].clone(),
                Vec::new(),
                0,
                BTreeMap::from([(
                    PLAN_ATTRIBUTE.into(),
                    Attribute::Bytes(
                        encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                    ),
                )]),
                expected_effects(CicsPlanOperation::Read).to_vec(),
                vec![alias(wrong), alias(named)],
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        assert!(matches!(
            ReferenceMachine::from_binary(
                &bytes,
                super::super::tests::invocation(),
                CodecLimits::default(),
            ),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn legacy_dataset_alias_reaches_file_control_commands_unchanged() {
        // CardDemo (`59cc6c2f`) writes DATASET(...) where these commands'
        // pinned registry rows spell the option FILE (compiler-side alias in
        // `crates/kernel/mainframe-env-compiler/src/hir/typed/cics_resolution.rs`).
        // `legacy_arguments` is generic over whichever clause name source
        // wrote (see the token loop above in this file), and
        // `crates/providers/mainframe-env-cics/src/handlers/file_control.rs:125-126`
        // already tries DATASET before falling back to FILE, so the legacy
        // execution route needs no change: this asserts that evidence at the
        // interpreter boundary for the STARTBR/WRITE/DELETE families named in
        // the task contract.
        for (label, tokens) in [
            (
                "STARTBR",
                vec![
                    "EXEC",
                    "CICS",
                    "STARTBR",
                    "DATASET",
                    "(",
                    "TRANSACT-FILE",
                    ")",
                    "RIDFLD",
                    "(",
                    "TRAN-ID",
                    ")",
                    "END-EXEC",
                ],
            ),
            (
                "WRITE",
                vec![
                    "EXEC",
                    "CICS",
                    "WRITE",
                    "DATASET",
                    "(",
                    "USRSEC-FILE",
                    ")",
                    "FROM",
                    "(",
                    "SEC-USER-DATA",
                    ")",
                    "END-EXEC",
                ],
            ),
            (
                "DELETE",
                vec![
                    "EXEC",
                    "CICS",
                    "DELETE",
                    "DATASET",
                    "(",
                    "USRSEC-FILE",
                    ")",
                    "RESP",
                    "(",
                    "WS-RESP-CD",
                    ")",
                    "END-EXEC",
                ],
            ),
        ] {
            let tokens = tokens.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert!(
                CicsOperation::from_tokens(&tokens).is_some(),
                "{label}: DATASET spelling must not change operation recognition"
            );
            let arguments = legacy_arguments(&tokens).expect("legacy arguments");
            assert!(
                arguments.contains_key("DATASET"),
                "{label}: expected a DATASET argument key, got {arguments:?}"
            );
            assert!(
                !arguments.contains_key("FILE"),
                "{label}: source wrote DATASET, not FILE"
            );
        }
    }

    #[test]
    fn legacy_startbr_keylength_length_of_resolves_to_decimal() {
        // Issue #184: COCRDLIC.cbl:1129's legacy-routed STARTBR writes
        // `KEYLENGTH(LENGTH OF WS-CARD-RID-CARDNUM)`; this pins that clause
        // resolving to `mainframe-env.cics.decimal@1`, like typed READ/REWRITE.
        let mut builder = ModuleBuilder::new(IrLimits::default());
        builder.add_storage("KEY-X", 3, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("KEY-X".into())),
                    ("simple_name".into(), Attribute::Text("KEY-X".into())),
                    ("category".into(), Attribute::Text("alphanumeric".into())),
                    ("picture".into(), Attribute::Text("X(3)".into())),
                    ("digits".into(), Attribute::Integer(0)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(3)),
                    ("element_length".into(), Attribute::Integer(3)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            &bytes,
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        let tokens = [
            "EXEC",
            "CICS",
            "STARTBR",
            "DATASET",
            "(",
            "'TRANSACT'",
            ")",
            "RIDFLD",
            "(",
            "KEY-X",
            ")",
            "KEYLENGTH",
            "(",
            "LENGTH",
            "OF",
            "KEY-X",
            ")",
            "END-EXEC",
        ]
        .map(str::to_string);
        let step = execute_legacy(&mut machine, &tokens).expect("startbr lowers");
        let Step::Effect(effect) = step else {
            panic!("expected a host effect step");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("expected a CICS request");
        };
        let key_length = request
            .arguments
            .get("KEYLENGTH")
            .expect("KEYLENGTH argument");
        assert_eq!(key_length.schema(), "mainframe-env.cics.decimal@1");
        assert_eq!(key_length.bytes(), b"3");
    }

    #[test]
    fn legacy_startbr_keylength_numeric_literal_resolves_to_decimal() {
        // A bare `KEYLENGTH(16)` isn't quoted, so `legacy_arguments` tags it
        // `argument@1`; `legacy_numeric_operand` resolves an all-digit token
        // directly to `decimal@1`, with no data-name lookup.
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            &bytes,
            super::super::tests::invocation(),
            CodecLimits::default(),
        )
        .unwrap();
        let tokens = [
            "EXEC",
            "CICS",
            "STARTBR",
            "DATASET",
            "(",
            "'TRANSACT'",
            ")",
            "KEYLENGTH",
            "(",
            "16",
            ")",
            "END-EXEC",
        ]
        .map(str::to_string);
        let step = execute_legacy(&mut machine, &tokens).expect("startbr lowers");
        let Step::Effect(effect) = step else {
            panic!("expected a host effect step");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("expected a CICS request");
        };
        let key_length = request
            .arguments
            .get("KEYLENGTH")
            .expect("KEYLENGTH argument");
        assert_eq!(key_length.schema(), "mainframe-env.cics.decimal@1");
        assert_eq!(key_length.bytes(), b"16");
    }
}
