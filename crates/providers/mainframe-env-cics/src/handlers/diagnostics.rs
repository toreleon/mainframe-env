//! Bounded durable diagnostic records and command control.

mod state;
mod trace_number;

pub use state::{
    CicsDiagnosticDumpRecord, CicsDiagnosticSnapshot, CicsDiagnosticTraceRecord,
    CicsDumpCodeDefinition, CicsMonitorAction, CicsMonitorPointDefinition, CicsTraceConfiguration,
};

use crate::service::{CicsService, Run, bounded};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use state::{load, persist};
use std::collections::BTreeMap;

impl CicsService {
    /// Read the retained local diagnostic records and trace configuration.
    pub fn diagnostic_snapshot(&self) -> Result<CicsDiagnosticSnapshot, HostProblem> {
        let state = load(self)?;
        Ok(state.snapshot())
    }

    /// Install the local trace destinations and user-trace flag.
    ///
    /// This trusted region-configuration API is distinct from an application
    /// `EXEC CICS TRACE` request. An application must pass SAF before changing
    /// its diagnostic state.
    pub fn configure_diagnostic_trace(
        &self,
        configuration: CicsTraceConfiguration,
    ) -> Result<(), HostProblem> {
        let current = load(self)?;
        if current.configuration == configuration {
            return Ok(());
        }
        let mut next = current.clone();
        next.configuration = configuration;
        persist(self, current.version, &mut next)
    }

    /// Register local dump-table and user event-monitoring definitions.
    /// Existing definitions are immutable through this additive API.
    pub fn register_diagnostic_resources(
        &self,
        dump_codes: &[CicsDumpCodeDefinition],
        monitor_points: &[CicsMonitorPointDefinition],
    ) -> Result<(), HostProblem> {
        let dumps = dump_codes
            .iter()
            .map(|definition| {
                let mut definition = definition.clone();
                definition.code = diagnostic_name(&definition.code, 4)?;
                Ok((definition.code.clone(), definition))
            })
            .collect::<Result<BTreeMap<_, _>, HostProblem>>()?;
        let points = monitor_points
            .iter()
            .map(|definition| {
                let mut definition = definition.clone();
                definition.entry_name = diagnostic_name(&definition.entry_name, 8)?;
                if !(1..=199).contains(&definition.point) {
                    return Err(HostProblem::Malformed);
                }
                match &definition.action {
                    CicsMonitorAction::AddCounter { slot }
                    | CicsMonitorAction::SubtractCounter { slot }
                    | CicsMonitorAction::OrCounter { slot }
                    | CicsMonitorAction::StartClock { slot }
                    | CicsMonitorAction::StopClock { slot }
                        if *slot > 255 =>
                    {
                        return Err(HostProblem::Malformed);
                    }
                    CicsMonitorAction::Move {
                        offset,
                        maximum_length,
                    } if *maximum_length == 0
                        || usize::from(*offset) + usize::from(*maximum_length) > 8192 =>
                    {
                        return Err(HostProblem::Malformed);
                    }
                    _ => {}
                }
                Ok((
                    format!("{}:{:03}", definition.entry_name, definition.point),
                    definition,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, HostProblem>>()?;
        if dumps.len() != dump_codes.len() || points.len() != monitor_points.len() {
            return Err(HostProblem::Malformed);
        }
        let current = load(self)?;
        let mut next = current.clone();
        for (name, definition) in dumps {
            if next
                .dump_definitions
                .get(&name)
                .is_some_and(|old| old != &definition)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            next.dump_definitions.insert(name, definition);
        }
        for (key, definition) in points {
            if next
                .monitor_definitions
                .get(&key)
                .is_some_and(|old| old != &definition)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            next.monitor_definitions.insert(key, definition);
        }
        if next == current {
            return Ok(());
        }
        persist(self, current.version, &mut next)
    }
}

fn diagnostic_name(value: &str, maximum: usize) -> Result<String, HostProblem> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$'))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(value)
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::EnterTraceNum => trace_number::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: state::DiagnosticReply,
) -> Result<CicsResponse, HostProblem> {
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        &reply.condition,
        reply.response,
        reply.response2,
        None,
        None,
        Vec::new(),
    )?;
    for (name, value) in reply.outputs {
        result.outputs.insert(name, bounded(value)?);
    }
    Ok(result)
}
