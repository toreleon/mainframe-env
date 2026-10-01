//! Explicit/default ABEND unwinding in the existing synchronous program lease owner.
use super::super::{CicsService, Run, bounded};
use super::handle_state::{AbendExit, AbendRecord, HandleState, persist_handle_state};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{CicsDisposition, CicsResponse, HostProblem, HostResult};

#[derive(Clone)]
pub(in crate::service) struct PendingProgramAbend {
    pub(in crate::service) response: CicsResponse,
    pub(in crate::service) record: Option<AbendRecord>,
    pub(in crate::service) cancel_exits: bool,
}

/// Only an observed source-backed child ABEND and the matching known executor result
/// can unwind. A reserved/unknown installed call never supplies handler success.
pub(in crate::service) fn unwind(
    service: &CicsService,
    run: &mut Run,
    result: &Result<HostResult, HostProblem>,
) -> Result<Option<CicsResponse>, HostProblem> {
    let Some(pending) = run.program_abend.clone() else {
        return Ok(None);
    };
    if !matches!(result, Err(HostProblem::Condition { name, response: -1, response2: 0 })
        if name == "INSTALLED-CALL-ABEND")
    {
        // Never turn post-dispatch uncertainty or a mismatched executor reply
        // into a known ABEND/handler disposition.
        return Err(HostProblem::UnknownOutcome);
    }
    if validate_pending(&pending).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    let previous = HandleState::from_run(run);
    let exit = if pending.cancel_exits {
        None
    } else {
        run.abend_handler.take()
    };
    if let Some(exit) = &exit {
        run.cancelled_abend_handler = Some(exit.clone());
    }
    run.latest_abend = pending.record;
    if HandleState::from_run(run) != previous || run.current_program.logical_level == 1 {
        persist_handle_state(service, run, previous).map_err(|_| HostProblem::UnknownOutcome)?;
    }
    let mut response = pending.response;
    let code = response.payload.bytes().to_vec();
    response.applid = run.applid.clone();
    response.sysid = run.sysid.clone();
    response.transaction = run.transaction.clone();
    response.outputs.insert(
        "ABEND.CODE".into(),
        BoundedPayload::new(
            "mainframe-env.cics.abend-code@1",
            code,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    match exit {
        Some(AbendExit::Label(target)) => {
            response.disposition = CicsDisposition::Handler;
            response.target = Some(target);
            response.payload = bounded(Vec::new())?;
            run.program_abend = None;
        }
        Some(AbendExit::Program(target)) => {
            response.disposition = CicsDisposition::Transfer;
            response.target = Some(target);
            response.payload = run
                .current_program
                .effect_invocation
                .bindings
                .get("cics.commarea")
                .cloned()
                .map(|area| bounded(area.bytes().to_vec()))
                .transpose()?
                .unwrap_or(bounded(Vec::new())?);
            run.program_abend = None;
        }
        None if run.current_program.logical_level == 1 => {
            response.payload = bounded(Vec::new())?;
            run.program_abend = None;
        }
        None => response.payload = bounded(Vec::new())?,
    }
    Ok(Some(response))
}

fn validate_pending(pending: &PendingProgramAbend) -> Result<(), HostProblem> {
    let response = &pending.response;
    if response.disposition != CicsDisposition::Abended
        || response.target.is_some()
        || response.payload.schema() != "mainframe-env.cics.payload@1"
        || response.payload.bytes().len() > 4
        || std::str::from_utf8(response.payload.bytes()).is_err()
        || response.outputs.contains_key("COMMAREA")
    {
        return Err(HostProblem::UnknownOutcome);
    }
    if response.outputs.contains_key("ABEND.DEFAULT") {
        validate_default_pop(response)?;
        if pending.record.is_some() || pending.cancel_exits {
            return Err(HostProblem::UnknownOutcome);
        }
    } else if response.condition != "ERROR"
        || response.response != 27
        || response.response2 != 0
        || response.outputs.get("ABEND.DUMP").is_none_or(|dump| {
            dump.schema() != "mainframe-env.cics.abend-dump@1"
                || !matches!(dump.bytes(), b"requested" | b"suppressed")
        })
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

/// Validate the additive control metadata before treating a retained LINK reply
/// as an ABEND unwind rather than an ordinary condition/COMMAREA result.
pub(super) fn validate_replay(response: &CicsResponse) -> Result<bool, HostProblem> {
    if response.outputs.contains_key("ABEND.DEFAULT") {
        validate_default_pop(response)?;
        if response.outputs.get("ABEND.CODE").is_none() {
            return Err(HostProblem::ProviderFailure);
        }
        return Ok(true);
    }
    let Some(code) = response.outputs.get("ABEND.CODE") else {
        return Ok(false);
    };
    let dump = response
        .outputs
        .get("ABEND.DUMP")
        .ok_or(HostProblem::ProviderFailure)?;
    let has_target = matches!(
        response.disposition,
        CicsDisposition::Handler | CicsDisposition::Transfer
    );
    if !matches!(
        response.disposition,
        CicsDisposition::Handler | CicsDisposition::Transfer | CicsDisposition::Abended
    ) || response.condition != "ERROR"
        || response.response != 27
        || response.response2 != 0
        || code.schema() != "mainframe-env.cics.abend-code@1"
        || code.bytes().len() > 4
        || std::str::from_utf8(code.bytes()).is_err()
        || dump.schema() != "mainframe-env.cics.abend-dump@1"
        || !matches!(dump.bytes(), b"requested" | b"suppressed")
        || response.outputs.contains_key("COMMAREA")
        || response.payload.schema() != "mainframe-env.cics.payload@1"
        || has_target != response.target.is_some()
        || response
            .target
            .as_ref()
            .is_some_and(|target| target.is_empty() || target.len() > 128)
        || response.disposition != CicsDisposition::Transfer && !response.payload.bytes().is_empty()
        || response.payload.bytes().len() > 32_763
    {
        return Err(HostProblem::ProviderFailure);
    }
    Ok(true)
}

/// One reviewed default transition, not a generic guessed condition/abend table.
fn validate_default_pop(response: &CicsResponse) -> Result<(), HostProblem> {
    let origin = response
        .outputs
        .get("ABEND.DEFAULT")
        .ok_or(HostProblem::ProviderFailure)?;
    let has_target = matches!(
        response.disposition,
        CicsDisposition::Handler | CicsDisposition::Transfer
    );
    if origin.schema() != "mainframe-env.cics.default-abend@1"
        || origin.bytes() != b"POP-HANDLE"
        || response.condition != "INVREQ"
        || response.response != 16
        || response.response2 != 0
        || !matches!(
            response.disposition,
            CicsDisposition::Handler | CicsDisposition::Transfer | CicsDisposition::Abended
        )
        || has_target != response.target.is_some()
        || response
            .target
            .as_ref()
            .is_some_and(|target| target.is_empty() || target.len() > 128)
        || response.outputs.contains_key("ABEND.DUMP")
        || response.outputs.contains_key("COMMAREA")
        || response.outputs.get("ABEND.CODE").is_some_and(|code| {
            code.schema() != "mainframe-env.cics.abend-code@1" || !code.bytes().is_empty()
        })
        || response.payload.schema() != "mainframe-env.cics.payload@1"
        || response.disposition != CicsDisposition::Transfer && !response.payload.bytes().is_empty()
        || response.payload.bytes().len() > 32_763
    {
        return Err(HostProblem::ProviderFailure);
    }
    Ok(())
}
