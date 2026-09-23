mod bms_map;
mod condition;
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
mod journal_control;
mod limits;
mod program_control;
mod queue_control;
mod recovery;
mod signal_event;
mod spool_control;
mod start_task;
mod storage_control;
mod task_context;
mod task_control;
mod task_enqueue;
mod task_return;
mod task_wait;
mod terminal_control;
mod terminal_run;
mod time;
mod transform_control;
pub(in crate::service) mod transient_data;
mod web_control;
mod web_service_control;

use super::{CicsService, Run};
use crate::generated::CicsCommandFamily;
use mainframe_env_host_api::{AccessIntent, CicsRequest, CicsResponse, HostProblem};
use mainframe_env_store_api::StoreError;
use std::collections::BTreeMap;

pub(super) fn assert_descriptor(
    descriptor: &crate::generated::CicsCommandDescriptor,
    request: &CicsRequest,
) {
    debug_assert_eq!(descriptor.operation, request.operation);
    debug_assert_eq!(descriptor.mutating, request.operation.is_mutating());
    debug_assert!(!descriptor.syntax.is_empty() && !descriptor.official_row.is_empty());
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
    let descriptor = crate::generated::command_descriptor(request.operation);
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
pub(super) use condition::respond as condition;
pub(super) use counter_control::invoke as invoke_counter;
pub(super) use diagnostics::invoke as invoke_diagnostics;
pub use diagnostics::{
    CicsDiagnosticDumpRecord, CicsDiagnosticSnapshot, CicsDiagnosticTraceRecord,
    CicsDumpCodeDefinition, CicsMonitorAction, CicsMonitorPointDefinition, CicsTraceConfiguration,
};
pub(super) use document_control::{
    DocumentRecord, invoke as invoke_document_control, load_authority as load_document_authority,
};
pub(super) use file_control::{DurableFileStatus, invoke as invoke_file_control};
pub(super) use file_tokens::FileUpdateState;
pub(super) use handle_state::{
    AbendExit, AbendRecord, HandleFrame, HandleState, decode_session_tail, session_schema_version,
};
pub use interval::{CicsIntervalError, CicsIntervalMode, CicsIntervalTime};
#[cfg(test)]
pub(super) use interval_control::IntervalStartState;
pub(super) use interval_control::load as load_interval_records;
pub use interval_control::{CICS_DELAY_WORK_GENERATION, CICS_START_WORK_GENERATION};
pub(super) use interval_control::{IntervalStartRecord, invoke as invoke_interval_control};
pub(super) use journal_control::{JournalRecord, load as load_journals};
pub(super) use program_control::invoke as invoke_program_control;
pub use program_control::{CicsApplicationEntryDefinition, CicsJavaStatus, CicsProgramDefinition};
pub(super) use program_control::{
    ProgramLoadState, load_application_entries, load_program_definitions, load_program_loads,
    validate_application_catalog,
};
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
pub(super) use task_enqueue::{
    load_enqueue_models, release_uow as release_uow_enqueues,
    validate_store as validate_enqueue_store,
};
pub use terminal_control::{
    CicsBmsControlSnapshot, CicsOutboardDestinationDefinition, CicsOutboardKind,
    CicsOutboardRecord, CicsOutboardSnapshot, CicsPartitionDefinition, CicsPartitionSetDefinition,
};
pub(super) use terminal_control::{
    TerminalInput, invoke as invoke_terminal_control, release_bms_message_for_task,
    release_outboard_task, release_partition_set_for_task, valid_aid as valid_terminal_aid,
};
pub(in crate::service) use terminal_run::terminal_secret_digest;
pub(super) use time::invoke as invoke_time;
pub use web_service_control::CicsWebServiceDefinition;
pub(super) fn invoke_extended_control(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    family: crate::generated::CicsCommandFamily,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match family {
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
        crate::generated::CicsCommandFamily::Diagnostics => {
            invoke_diagnostics(service, run, request)
        }
        crate::generated::CicsCommandFamily::WebControl => {
            web_control::invoke(service, run, request)
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
    task_enqueue::release_task(service, run)?;
    task_wait::release_task(service, run)?;
    document_control::release_task(service, run)?;
    release_bms_message_for_task(service, run)?;
    release_outboard_task(service, run)?;
    release_partition_set_for_task(service, run)?;
    interval_control::release_task(service, run)?;
    program_control::release_task_program_loads(service, run)?;
    web_service_control::release_task(service, run)
}

pub(super) fn rollback_task(
    service: &CicsService,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    run: &Run,
) -> Result<(), HostProblem> {
    interval_control::discard_protected_start_records(
        service.store.as_ref(),
        records,
        run.invocation.run_unit_id.as_str(),
    )?;
    release_task_state(service, run)
}
