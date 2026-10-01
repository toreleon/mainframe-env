mod bms_map;
mod bridge_abi;
mod bridge_definition;
mod bridge_profile;
mod bridge_runtime;
mod bridge_start;
mod bridge_terminal;
pub mod bts_browse;
mod bts_child_link;
mod bts_container;
pub mod bts_lifecycle;
mod bts_link;
mod builtin_function;
mod condition;
mod conversation_extract;
pub(in crate::service) use conversation_extract::publish_pass_logon;
#[cfg(test)]
pub(in crate::service) use conversation_extract::{ExtractMetadata, LuName, publish_metadata};
mod conversation_control;
mod counter_control;
mod diagnostics;
mod document_control;
mod event_control;
mod file_control;
mod file_tokens;
mod file_unlock;
mod handle_state;
mod host_boundary;
mod interval;
mod interval_control;
mod issue_device;
pub use issue_device::IssuePassTransfer;
#[cfg(test)]
pub(in crate::service) use issue_device::{
    IssueDeviceDefinition, IssueDeviceKind, IssueDeviceRecord, invoke as invoke_issue_device,
};
mod journal_control;
mod limits;
mod network_context;
mod network_control;
mod operator_control;
mod program_control;
mod queue_control;
mod recovery;
mod security_control;
pub use security_control::{
    CicsCredentialChangeRequest, CicsCredentialDetails, CicsCredentialFailure, CicsCredentialKind,
    CicsCredentialRequest, CicsCredentialVerification, CicsPassTicketFailure,
    CicsPassTicketOutcome, CicsPassTicketRequest, CicsSecurityAccess, CicsSecurityAccessReason,
    CicsSecurityAuthority, CicsSecurityTokenKind, CicsTokenFailure, CicsTokenVerification,
    CicsTokenVerificationRequest,
};
pub(in crate::service) use security_control::{
    TerminalIdentity, decode_terminal_identity, encode_terminal_identity,
    validate_terminal_identity,
};
mod signal_event;
mod spool_control;
mod start_brexit;
mod start_task;
mod storage_control;
mod task_context;
mod task_control;
mod task_enqueue;
mod task_return;
mod task_wait;
mod trace;
pub use trace::CicsTraceEntry;
mod terminal_control;
mod terminal_lifecycle;
mod terminal_run;
mod time;
mod transaction_definition;
mod transform_control;
pub(in crate::service) mod transient_data;
mod web_control;
mod web_service_control;

use super::{CicsService, Run};
use crate::generated::CicsCommandFamily;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostResult, canonical_result_digest,
};
use mainframe_env_store_api::StoreError;
use std::collections::BTreeMap;

pub(super) fn deferred_converse(operation: CicsOperation, response: &CicsResponse) -> bool {
    response.disposition == CicsDisposition::Suspended
        && matches!(
            operation,
            CicsOperation::Converse
                | CicsOperation::ReceiveConversation
                | CicsOperation::GdsReceiveConversation
                | CicsOperation::SendConversation
                | CicsOperation::GdsWaitConversation
                | CicsOperation::WaitConvid
                | CicsOperation::WaitSignal
                | CicsOperation::WaitTerminal
        )
}

pub(super) fn cics_result_digest(response: &CicsResponse) -> Result<[u8; 32], HostProblem> {
    canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn originating_task_for(
    session: &super::Session,
    invocation: &mainframe_env_execution_api::Invocation,
) -> String {
    if session.run_unit.is_empty() {
        invocation.run_unit_id.as_str().to_string()
    } else {
        session.run_unit.clone()
    }
}

pub(super) fn assert_descriptor(
    descriptor: &crate::generated::CicsCommandDescriptor,
    request: &CicsRequest,
) {
    debug_assert_eq!(descriptor.operation, request.operation);
    debug_assert_eq!(descriptor.mutating, request.operation.is_mutating());
    debug_assert!(!descriptor.syntax.is_empty() && !descriptor.official_row.is_empty());
}

fn bts_live(service: &CicsService, run: &Run, retention_tick: u64) -> Result<(), HostProblem> {
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let tick = match &service.replay_clock {
        Some(clock) => clock.now_tick()?,
        None => retention_tick.saturating_sub(1),
    };
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

/// A selected program LINK retains its caller's run unit on nested RETURN.
fn selected_link_return(run: &Run) -> bool {
    run.invocation.parent_execution_id.is_some()
        && run.invocation.bindings.contains_key("cobol.call.arguments")
}

pub(super) fn authorize_and_describe(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<&'static crate::generated::CicsCommandDescriptor, HostProblem> {
    service.authorize(
        run,
        "TCICSTRN",
        &format!("CICS.{}", run.transaction),
        AccessIntent::Execute,
    )?;
    let descriptor =
        crate::generated::command_descriptor(request.operation).ok_or(HostProblem::Unsupported)?;
    assert_descriptor(descriptor, request);
    Ok(descriptor)
}

pub(super) fn argument_bytes(request: &CicsRequest, name: &str) -> Option<Vec<u8>> {
    request
        .arguments
        .get(name)
        .map(|value| value.bytes().to_vec())
}

pub(super) fn argument_optional(request: &CicsRequest, name: &str) -> Option<String> {
    argument_bytes(request, name).map(|value| String::from_utf8_lossy(&value).into_owned())
}

pub(super) fn argument_text(request: &CicsRequest, name: &str) -> Result<String, HostProblem> {
    let value = argument_bytes(request, name).ok_or(HostProblem::Malformed)?;
    String::from_utf8(value).map_err(|_| HostProblem::Malformed)
}

pub(crate) fn field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value);
    Ok(())
}

pub(crate) fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

pub use bms_map::{BmsFieldDefinition, BmsMapDefinition};
pub(super) use bms_map::{
    decode_terminal_address, encode_terminal_address, terminal_field_address, validate_map,
};
pub use bridge_abi::{BrxaBindFrame, BrxaBindReply, BrxaEndFrame, BrxaInitFrame, BrxaInitReply};
pub use bridge_definition::{CicsBridgeExitDefault, CicsBridgeExitSelection};
pub use bridge_profile::{CicsBridgeAbiProfile, CicsBridgeAbiSelection};
pub use bridge_runtime::CicsBridgeRuntime;
pub use bridge_start::{CICS_BRIDGE_START_WORK_GENERATION, CicsBridgeStartIntent};
pub use bts_child_link::CicsBtsChildCompletion;
pub use bts_link::CicsBtsLinkContext;
pub(super) use condition::respond as condition;
pub(super) fn condition_for_request(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    problem: HostProblem,
) -> Result<CicsResponse, HostProblem> {
    if matches!(
        request.operation,
        mainframe_env_host_api::CicsOperation::GdsExtractAttributes
            | mainframe_env_host_api::CicsOperation::GdsExtractProcess
    ) || request.operation == mainframe_env_host_api::CicsOperation::ExtractAttributes
        && problem == HostProblem::Unsupported
    {
        return Err(problem);
    }
    condition::respond(service, run, &request.condition_policy, problem)
}
pub use conversation_control::{
    CONVERSATION_RECORD_VERSION, CONVERSATION_REPLAY_NAMESPACE, CONVERSATION_STATE_NAMESPACE,
    CicsConversationTransport, ConversationAttachHeader, ConversationConnectFrame,
    ConversationContext, ConversationDataFrame, ConversationDataReply, ConversationDataState,
    ConversationExchangeState, ConversationKind, ConversationLedger, ConversationOutboundFrame,
    ConversationOwner, ConversationPartnerDefinition, ConversationPartnerProcessDefinition,
    ConversationPeerFrame, ConversationProblem, ConversationProfileDefinition, ConversationRecord,
    ConversationReplay, ConversationReply, ConversationState, ConversationSystemDefinition,
    ConversationTransmitOutcome, DataCondition, GdsAllocateFailure, GdsAssignFailure,
    GdsConnectFailure, GdsFreeFailure, GdsIssueFailure, GdsIssueFlow, GdsReceiveFailure,
    GdsReturnCode, GdsWaitFailure, IssuePendingControl, IssueRequestIdentity,
    IssueValidationProblem, MAX_BASIC_PIP_BYTES, MAX_EXCHANGE_FRAME_BYTES, MAX_PENDING_PEER_FRAMES,
    MAX_PIP_BYTES, MAX_PROCESS_BYTES, MAX_RECORDED_OUTBOUND_FRAMES, SignalFacilityRecord,
    SignalLuType, load_conversation_replay, prune_conversation_replays,
};
#[cfg(test)]
pub(in crate::service) use conversation_control::{
    confirm_issue_control, mark_issue_control_attempted,
};
pub(super) use conversation_control::{
    context as conversation_context, deadline as conversation_deadline,
};

pub(super) fn preflight_conversation(
    service: &CicsService,
    run: &Run,
    family: CicsCommandFamily,
) -> Result<(), HostProblem> {
    if family == CicsCommandFamily::ConversationControl {
        conversation_context(run)?;
        conversation_deadline(service, run)?;
    }
    Ok(())
}
pub(super) use counter_control::invoke as invoke_counter;
pub(super) use diagnostics::invoke as invoke_diagnostics;
pub use diagnostics::{
    CicsDiagnosticDumpRecord, CicsDiagnosticSnapshot, CicsDiagnosticTraceRecord,
    CicsDumpCodeDefinition, CicsMonitorAction, CicsMonitorPointDefinition, CicsTraceConfiguration,
};
pub(super) use document_control::{
    DocumentRecord, invoke as invoke_document_control, load_authority as load_document_authority,
};
pub use file_control::CicsFileDefinition;
pub(super) use file_control::{DurableFileStatus, invoke as invoke_file_control};
pub(super) use file_tokens::FileUpdateState;
pub use network_context::{
    CicsCertificateName, CicsClientCertificate, CicsTcpipAuthenticate, CicsTcpipContext,
    CicsTcpipPrivacy, CicsTcpipSslType,
};
pub(super) use network_control::invoke as invoke_network;
pub use operator_control::CICS_OPERATOR_WORK_GENERATION;
pub use operator_control::CicsOperatorMessageView;
pub(super) use operator_control::invoke as invoke_operator;
pub(super) use time::invoke as invoke_time;
pub(super) fn validate_owned_stores(
    store: &dyn mainframe_env_store_api::ProviderStateStore,
    limits: super::CicsLimits,
) -> Result<(), HostProblem> {
    task_enqueue::validate_store(store, limits)?;
    operator_control::load_operator_messages(store, limits)?;
    operator_control::validate_active_operator_commands(store, limits)?;
    network_context::validate_store(store, limits)?;
    bridge_definition::validate_store(store, limits)?;
    bridge_profile::validate_store(store, limits)?;
    bridge_runtime::validate_store(store, limits)?;
    bridge_start::validate_store(store, limits)?;
    Ok(())
}
pub(super) use handle_state::{
    AbendExit, AbendRecord, HandleFrame, HandleState, decode_session_tail, session_schema_version,
};
pub use interval::{CicsIntervalError, CicsIntervalMode, CicsIntervalTime};
#[cfg(test)]
pub(super) use interval_control::IntervalStartState;
pub(super) use interval_control::load as load_interval_records;
pub use interval_control::{
    CICS_DELAY_WORK_GENERATION, CICS_POST_WORK_GENERATION, CICS_START_WORK_GENERATION,
};
pub(super) fn post_event_outputs(
    service: &CicsService,
    run: &Run,
) -> Result<BTreeMap<String, mainframe_env_execution_api::BoundedPayload>, HostProblem> {
    let Some(event) = interval_control::post_ready_event(service, run)? else {
        return Ok(BTreeMap::new());
    };
    Ok(BTreeMap::from([(
        "POST.EVENT".into(),
        mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.cics.post-event@1",
            event,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    )]))
}
pub(super) use interval_control::{IntervalStartRecord, invoke as invoke_interval_control};
pub(super) use journal_control::{JournalRecord, load as load_journals};
pub use program_control::{CicsApplicationEntryDefinition, CicsJavaStatus, CicsProgramDefinition};
pub(super) use program_control::{
    ProgramLoadState, load_application_entries, load_program_definitions, load_program_loads,
    validate_application_catalog,
};
pub(super) use program_control::{invoke as invoke_program_control, validate_replay_response};
pub(super) use queue_control::invoke as invoke_queue_control;
pub(super) use recovery::invoke as invoke_recovery;
pub use signal_event::{CicsSignalCaptureSpec, CicsSignalEmission};
pub(super) use spool_control::invoke as invoke_spool_control;
pub(super) use spool_control::{
    SpoolRecord, SpoolRecordMode, SpoolReport, SpoolReportState, SpoolState, load_spool_state,
    persist_spool_state,
};
pub use start_task::{CicsStartTask, CicsStartTerminal};
pub(super) use task_context::{
    CurrentProgramFrame, allocate_terminal_input, synchronize_current_program,
};
pub(super) use task_control::{
    RunSeed, decode_session_flags, encode_session, invoke as invoke_task_control, new_run,
    new_run_with_state,
};
pub use task_enqueue::CicsEnqueueModelDefinition;
pub(super) use task_enqueue::{load_enqueue_models, release_uow as release_uow_enqueues};
pub use terminal_control::{
    CicsBmsControlSnapshot, CicsOutboardDestinationDefinition, CicsOutboardKind,
    CicsOutboardRecord, CicsOutboardSnapshot, CicsPartitionDefinition, CicsPartitionSetDefinition,
};
pub(super) use terminal_control::{
    TerminalInput, invoke as invoke_terminal_control, release_bms_message_for_task,
    release_outboard_task, release_partition_set_for_task, valid_aid as valid_terminal_aid,
};
pub(in crate::service) use terminal_run::terminal_secret_digest;
pub use web_control::{
    CicsWebEndpoint, CicsWebInboundRequest, CicsWebRequest, CicsWebResponse, CicsWebServerResponse,
    CicsWebTransport, CicsWebUriMapDefinition, CicsWebVersion,
};
pub(in crate::service) use web_control::{WebState, load_web_state};
pub use web_service_control::CicsWebServiceDefinition;
pub(super) fn invoke_extended_control(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    family: crate::generated::CicsCommandFamily,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match family {
        crate::generated::CicsCommandFamily::ConversationControl => {
            if matches!(
                request.operation,
                CicsOperation::ExtractAttach
                    | CicsOperation::ExtractAttributes
                    | CicsOperation::GdsExtractAttributes
                    | CicsOperation::ExtractLogonMsg
                    | CicsOperation::ExtractProcess
                    | CicsOperation::GdsExtractProcess
                    | CicsOperation::ExtractTct
                    | CicsOperation::Point
            ) {
                conversation_extract::invoke(service, run, request)
            } else {
                conversation_control::invoke(service, run, request, retention_tick)
            }
        }
        crate::generated::CicsCommandFamily::TransformControl => {
            transform_control::invoke(service, run, request)
        }
        crate::generated::CicsCommandFamily::JournalControl => {
            journal_control::invoke(service, run, request)
        }
        crate::generated::CicsCommandFamily::WebServiceControl => {
            web_service_control::invoke(service, run, request, retention_tick)
        }
        crate::generated::CicsCommandFamily::EventControl => {
            if request.operation == mainframe_env_host_api::CicsOperation::SignalEvent {
                signal_event::invoke(service, run, request)
            } else {
                event_control::invoke(service, run, request)
            }
        }
        crate::generated::CicsCommandFamily::BtsControl => match request.operation {
            mainframe_env_host_api::CicsOperation::BtsEndBrowseContainer
            | mainframe_env_host_api::CicsOperation::BtsGetNextContainer
            | mainframe_env_host_api::CicsOperation::BtsInquireContainer
            | mainframe_env_host_api::CicsOperation::BtsStartBrowseContainer
            | mainframe_env_host_api::CicsOperation::BtsEndBrowseEvent
            | mainframe_env_host_api::CicsOperation::BtsGetNextEvent
            | mainframe_env_host_api::CicsOperation::BtsInquireEvent
            | mainframe_env_host_api::CicsOperation::BtsStartBrowseEvent
            | mainframe_env_host_api::CicsOperation::BtsEndBrowseTimer
            | mainframe_env_host_api::CicsOperation::BtsInquireTimer
            | mainframe_env_host_api::CicsOperation::BtsStartBrowseTimer
            | mainframe_env_host_api::CicsOperation::BtsStartBrowseActivity
            | mainframe_env_host_api::CicsOperation::BtsGetNextActivity
            | mainframe_env_host_api::CicsOperation::BtsEndBrowseActivity
            | mainframe_env_host_api::CicsOperation::BtsInquireActivity
            | mainframe_env_host_api::CicsOperation::BtsStartBrowseProcess
            | mainframe_env_host_api::CicsOperation::BtsGetNextProcess
            | mainframe_env_host_api::CicsOperation::BtsEndBrowseProcess
            | mainframe_env_host_api::CicsOperation::BtsInquireProcess => {
                bts_browse::invoke(service, run, request)
            }
            CicsOperation::DeleteChannel
            | CicsOperation::DeleteContainer
            | CicsOperation::GetContainer
            | CicsOperation::GetContainer64
            | CicsOperation::MoveContainer
            | CicsOperation::PutContainer
            | CicsOperation::PutContainer64
            | CicsOperation::QueryChannel => {
                bts_container::invoke_channel_container(service, run, request)
            }
            mainframe_env_host_api::CicsOperation::FetchAny
            | mainframe_env_host_api::CicsOperation::FetchChild
            | mainframe_env_host_api::CicsOperation::FreeChild => {
                bts_child_link::invoke_child(service, run, request, retention_tick)
            }
            mainframe_env_host_api::CicsOperation::LinkAcqActivity
            | mainframe_env_host_api::CicsOperation::LinkAcqProcess
            | mainframe_env_host_api::CicsOperation::LinkActivity => {
                bts_link::invoke(service, run, request, retention_tick)
            }
            _ => bts_lifecycle::invoke(service, run, request),
        },
        crate::generated::CicsCommandFamily::Diagnostics => {
            invoke_diagnostics(service, run, request)
        }
        crate::generated::CicsCommandFamily::WebControl => {
            web_control::invoke(service, run, request)
        }
        crate::generated::CicsCommandFamily::SecurityControl => {
            security_control::invoke(service, run, request, retention_tick)
        }
        crate::generated::CicsCommandFamily::BuiltinFunctionControl => {
            builtin_function::invoke(service, run, request)
        }
        _ => unreachable!("only extended control families delegate here"),
    }
}

#[allow(unused_imports)]
pub use transform_control::{
    CicsTransformContainerMode, CicsTransformDefinition, CicsTransformFieldDefinition,
    CicsTransformFieldKind, CicsTransformFormat, CicsXmlTransformMetadata,
};
pub(crate) use transform_control::{
    TransformContainer, container as transform_container,
    load_containers as load_transform_containers, load_resources as load_transform_resources,
    put_container as put_transform_container, register_definition as register_transform_definition,
};
#[allow(unused_imports)]
pub use transient_data::{
    CicsTransientDataQueueDefinition, CicsTransientDataQueueKind, CicsTransientDataQueueOpen,
};
pub(in crate::service) use transient_data::{
    TransientDataState, load as load_transient_data, register as register_transient_data,
};

pub(super) fn invoke_interval_or_spool_control(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    family: CicsCommandFamily,
) -> Result<CicsResponse, HostProblem> {
    match family {
        CicsCommandFamily::IntervalControl => invoke_interval_control(service, run, request),
        CicsCommandFamily::SpoolControl => invoke_spool_control(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn release_task_state(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    bts_browse::release_task(service, run)?;
    bts_child_link::release_task(service, run)?;
    bts_link::release_task(service, run)?;
    task_enqueue::release_task(service, run)?;
    task_wait::release_task(service, run)?;
    conversation_control::release_task(service, run)?;
    document_control::release_task(service, run)?;
    release_bms_message_for_task(service, run)?;
    release_outboard_task(service, run)?;
    release_partition_set_for_task(service, run)?;
    web_control::release_task(service, run)?;
    interval_control::release_task(service, run)?;
    program_control::release_task_program_loads(service, run)?;
    security_control::release_task_token_key(service, run)?;
    network_context::release_task(service, run)?;
    web_service_control::release_task(service, run)
}

pub(super) fn discard_task_starts(
    service: &CicsService,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    run: &Run,
) -> Result<(), HostProblem> {
    interval_control::discard_protected_start_records(
        service.store.as_ref(),
        records,
        run.invocation.run_unit_id.as_str(),
    )
}
