mod bms_map;
mod condition;
mod file_control;
mod handle_state;
mod interval;
mod interval_control;
mod program_control;
mod queue_control;
mod recovery;
mod start_task;
mod storage_control;
mod task_context;
mod task_control;
mod task_enqueue;
mod task_return;
mod terminal_control;
mod terminal_run;
mod time;

use super::{CicsService, Run};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::StoreError;
use std::collections::BTreeMap;

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

pub(super) use bms_map::{
    decode_terminal_address, encode_terminal_address, terminal_field_address, validate_map,
};
pub(super) use condition::respond as condition;
pub(super) use file_control::invoke as invoke_file_control;
pub(super) use handle_state::{
    AbendExit, AbendRecord, HandleFrame, HandleState, decode_session_tail, session_schema_version,
};
pub use interval::{CicsIntervalError, CicsIntervalMode, CicsIntervalTime};
#[cfg(test)]
pub(super) use interval_control::IntervalStartState;
pub(super) use interval_control::load as load_interval_records;
pub use interval_control::{CICS_DELAY_WORK_GENERATION, CICS_START_WORK_GENERATION};
pub(super) use interval_control::{IntervalStartRecord, invoke as invoke_interval_control};
pub(super) use program_control::invoke as invoke_program_control;
pub(super) use queue_control::invoke as invoke_queue_control;
pub(super) use recovery::invoke as invoke_recovery;
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
pub(super) use terminal_control::{
    TerminalInput, invoke as invoke_terminal_control, valid_aid as valid_terminal_aid,
};
pub(super) use time::invoke as invoke_time;

pub(super) fn release_task_state(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    task_enqueue::release_task(service, run)?;
    interval_control::release_task(service, run)
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
