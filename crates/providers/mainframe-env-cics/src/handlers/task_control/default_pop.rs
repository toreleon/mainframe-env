//! Source-defined unmatched POP HANDLE default recovery at the current link level.
use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let previous = HandleState::from_run(run);
    let default_abend = matches!(request.condition_policy, CicsConditionPolicy::Default)
        && !["INVREQ", "ERROR"].iter().any(|condition| {
            run.ignored_conditions.contains(*condition) || run.handlers.contains_key(*condition)
        });
    if default_abend {
        super::super::interval_control::discard_protected_starts(service, run)?;
        if run.abend_handler.is_none() && !service.ancestor_abend_exit(run)? {
            super::super::release_task_state(service, run)?;
        }
        // This source establishes default termination, not an IBM ABCODE or dump.
        // Do not reuse a prior explicit ABEND's metadata for a different origin.
        run.latest_abend = None;
    }
    let (disposition, target, payload) = match &request.condition_policy {
        CicsConditionPolicy::NoHandle | CicsConditionPolicy::Respond { .. } => {
            (CicsDisposition::Complete, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.ignored_conditions.contains("INVREQ") => {
            (CicsDisposition::Ignored, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.handlers.contains_key("INVREQ") => (
            CicsDisposition::Handler,
            run.handlers.get("INVREQ").cloned(),
            Vec::new(),
        ),
        CicsConditionPolicy::Default if run.ignored_conditions.contains("ERROR") => {
            (CicsDisposition::Ignored, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.handlers.contains_key("ERROR") => (
            CicsDisposition::Handler,
            run.handlers.get("ERROR").cloned(),
            Vec::new(),
        ),
        CicsConditionPolicy::Default if run.abend_handler.is_some() => {
            let exit = run
                .abend_handler
                .take()
                .ok_or(HostProblem::InfrastructureFailure)?;
            run.cancelled_abend_handler = Some(exit.clone());
            match exit {
                AbendExit::Label(target) => (CicsDisposition::Handler, Some(target), Vec::new()),
                AbendExit::Program(target) => (
                    CicsDisposition::Transfer,
                    Some(target),
                    run.retrieve.clone(),
                ),
            }
        }
        CicsConditionPolicy::Default => (CicsDisposition::Abended, None, Vec::new()),
    };
    if HandleState::from_run(run) != previous {
        persist_handle_state(service, run, previous)?;
    }
    let mut response =
        service.response(run, disposition, "INVREQ", 16, 0, target, None, payload)?;
    if default_abend && disposition == CicsDisposition::Abended {
        response.outputs.insert(
            "ABEND.DEFAULT".into(),
            BoundedPayload::new(
                "mainframe-env.cics.default-abend@1",
                b"POP-HANDLE".to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        if run.current_program.logical_level > 1 {
            run.program_abend = Some(super::super::program_abend::PendingProgramAbend {
                response: response.clone(),
                record: None,
                cancel_exits: false,
            });
        }
    }
    Ok(response)
}
