//! Bounded DL/I system and GSAM-adjacent calls on the IMS row/replay authority.

use super::*;
use mainframe_env_execution_api::ServiceClass;
use mainframe_env_host_api::{
    ImsAcceptRow, ImsBufferPoolKind, ImsBufferStatistics, ImsCallSite, ImsCallSyntax,
    ImsDatabaseOrganization, ImsDedbAreaDefinition, ImsExecutionContext, ImsPcbAvailability,
    ImsPcbKind, ImsPositionArea, ImsPositionKeyword, ImsProcessingOptionClass, ImsQClass,
    ImsSsaForm, ImsStatisticsFamily, ImsStatusContext, ImsSystemCall, ImsSystemRequest,
    ImsSystemResult, ImsSystemRuntimeDefinition, resolve_ims_status, validate_ims_call_site,
};

const ROW_KEY: &str = "runtime";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct SystemSession {
    accepted: Option<ImsStatusGroup>,
    statuses: BTreeMap<u16, String>,
    #[serde(default)]
    dib_statuses: BTreeMap<u16, String>,
    last_database_call: bool,
    refresh_used: bool,
    stat_cursor: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Reservation {
    owner: String,
    class: u8,
    current: bool,
    modified: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct SystemState {
    runtime: Option<ImsSystemRuntimeDefinition>,
    areas: BTreeMap<String, ImsPositionArea>,
    pools: BTreeMap<String, ImsBufferStatistics>,
    reservations: BTreeMap<String, Reservation>,
}

fn system_state(state: &mut State) -> &mut SystemState {
    Arc::make_mut(state.system.entry(ROW_KEY.into()).or_default())
}

#[cfg(test)]
pub(super) fn reservation_count(state: &State) -> usize {
    state
        .system
        .get(ROW_KEY)
        .map_or(0, |row| row.reservations.len())
}

fn runtime(state: &State) -> Result<&ImsSystemRuntimeDefinition, HostProblem> {
    state
        .system
        .get(ROW_KEY)
        .and_then(|system| system.runtime.as_ref())
        .ok_or(HostProblem::NotFound)
}

fn metadata_pcb<'a>(
    state: &'a State,
    psb: &str,
    pcb: u16,
) -> Result<(&'a str, &'a str, &'a str), HostProblem> {
    let index = usize::from(pcb)
        .checked_sub(1)
        .ok_or(HostProblem::Malformed)?;
    if let Some((metadata, psb)) = state.metadata.as_ref().and_then(|metadata| {
        metadata
            .psbs
            .iter()
            .find(|item| normalize(&item.name) == psb)
            .map(|item| (metadata, item))
    }) {
        let item = psb.pcbs.get(index).ok_or(HostProblem::NotFound)?;
        let mainframe_env_host_api::ImsPcbMetadata::Database(database_pcb) = item else {
            return Err(HostProblem::Unsupported);
        };
        let database = metadata
            .databases
            .iter()
            .find(|db| normalize(&db.name) == normalize(&database_pcb.database))
            .ok_or(HostProblem::InfrastructureFailure)?;
        // The organization spelling is supplied by the closed metadata enum.
        return Ok((
            &database.name,
            &database_pcb.processing_options,
            match database.organization {
                ImsDatabaseOrganization::Dedb => "DEDB",
                ImsDatabaseOrganization::Gsam => "GSAM",
                ImsDatabaseOrganization::Hdam => "HDAM",
                ImsDatabaseOrganization::Hidam => "HIDAM",
                ImsDatabaseOrganization::Hisam => "HISAM",
                ImsDatabaseOrganization::Hsam => "HSAM",
                ImsDatabaseOrganization::Index => "INDEX",
                ImsDatabaseOrganization::Msdb => "MSDB",
                ImsDatabaseOrganization::Phdam => "PHDAM",
                ImsDatabaseOrganization::Phidam => "PHIDAM",
                ImsDatabaseOrganization::Psindex => "PSINDEX",
                ImsDatabaseOrganization::Shisam => "SHISAM",
                ImsDatabaseOrganization::Shsam => "SHSAM",
            },
        ));
    }
    let psb = state
        .definitions
        .as_ref()
        .and_then(|definitions| {
            definitions
                .psbs
                .iter()
                .find(|item| normalize(&item.name) == psb)
        })
        .ok_or(HostProblem::NotFound)?;
    let item = psb.pcbs.get(index).ok_or(HostProblem::NotFound)?;
    let database = state
        .definitions
        .as_ref()
        .and_then(|definitions| {
            definitions
                .databases
                .iter()
                .find(|db| normalize(&db.name) == normalize(&item.database))
        })
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok((&database.name, &item.processing_options, &database.access))
}

fn processing_option(raw: &str) -> Result<ImsProcessingOptionClass, HostProblem> {
    if raw.contains('A') {
        Ok(ImsProcessingOptionClass::All)
    } else if raw.contains('O') {
        Ok(ImsProcessingOptionClass::ReadWithoutIntegrity)
    } else if raw.contains('G') {
        Ok(ImsProcessingOptionClass::Read)
    } else if raw.contains('R') {
        Ok(ImsProcessingOptionClass::Replace)
    } else if raw.contains('D') {
        Ok(ImsProcessingOptionClass::Delete)
    } else if raw.contains('I') {
        Ok(ImsProcessingOptionClass::Insert)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn psb_name(state: &State, run: &str, request: &ImsRequest) -> Result<String, HostProblem> {
    let scheduled = state.sessions.get(run).map(|session| session.psb.as_str());
    let supplied = request.psb.as_deref().map(normalize);
    if supplied
        .as_deref()
        .zip(scheduled)
        .is_some_and(|(left, right)| left != right)
    {
        return Err(HostProblem::Malformed);
    }
    supplied
        .or_else(|| scheduled.map(str::to_owned))
        .ok_or(HostProblem::NotFound)
}

fn pcb_availability(
    state: &State,
    psb: &str,
    pcb: u16,
    statuses: &BTreeMap<u16, String>,
) -> Result<ImsPcbAvailability, HostProblem> {
    let (_, _, organization) = metadata_pcb(state, psb, pcb)?;
    let status = statuses.get(&pcb).cloned().unwrap_or_else(|| "  ".into());
    resolve_ims_status(
        status.as_bytes(),
        ImsStatusContext::Database,
        ImsPcbKind::Database,
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(ImsPcbAvailability {
        pcb,
        status,
        organization: organization.into(),
    })
}

fn psb_pcb_count(state: &State, psb: &str) -> Result<usize, HostProblem> {
    if let Some(item) = state.metadata.as_ref().and_then(|metadata| {
        metadata
            .psbs
            .iter()
            .find(|item| normalize(&item.name) == psb)
    }) {
        return Ok(item.pcbs.len());
    }
    state
        .definitions
        .as_ref()
        .and_then(|definitions| {
            definitions
                .psbs
                .iter()
                .find(|item| normalize(&item.name) == psb)
        })
        .map(|item| item.pcbs.len())
        .ok_or(HostProblem::NotFound)
}

fn call_identity(call: &ImsSystemCall, syntax: ImsCallSyntax) -> (u8, &'static str) {
    match call {
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Initial,
            ..
        } => (1, "INIT"),
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Availability,
            ..
        } => (
            12,
            if syntax == ImsCallSyntax::Call {
                "INIT"
            } else {
                "ACCEPT"
            },
        ),
        ImsSystemCall::Query { .. } => (
            13,
            if syntax == ImsCallSyntax::Call {
                "INIT"
            } else {
                "QUERY"
            },
        ),
        ImsSystemCall::Refresh => (
            14,
            if syntax == ImsCallSyntax::Call {
                "INIT"
            } else {
                "REFRESH"
            },
        ),
        ImsSystemCall::Dequeue { .. } => (3, "DEQ"),
        ImsSystemCall::Gscd => (7, "GSCD"),
        ImsSystemCall::Position { .. } => (11, "POS"),
        ImsSystemCall::Statistics { .. } => (22, "STAT"),
    }
}

fn validate_call_site(
    state: &State,
    run: &str,
    request: &ImsRequest,
    system: &ImsSystemRequest,
) -> Result<(String, Option<String>), HostProblem> {
    let psb = psb_name(state, run, request)?;
    let (row, name) = call_identity(&system.call, system.syntax);
    let ssa_form = match &system.call {
        ImsSystemCall::Position { ssa: Some(ssa), .. } if ssa.field.is_some() => {
            ImsSsaForm::Qualified
        }
        ImsSystemCall::Position { ssa: Some(_), .. } => ImsSsaForm::Unqualified,
        _ => ImsSsaForm::Absent,
    };
    let database = if request.pcb == 0 {
        None
    } else {
        let (database_name, raw_option, organization) = metadata_pcb(state, &psb, request.pcb)?;
        let option = processing_option(raw_option)?;
        let site = ImsCallSite {
            official_row: format!("ibm-ims-15.6-dli-2026-08-31:dli-call-families:{row:04}"),
            name,
            syntax: system.syntax,
            context: system.context,
            pcb_kind: Some(ImsPcbKind::Database),
            organization: Some(organization),
            processing_option: Some(option),
            ssa_form,
        };
        validate_ims_call_site(&site).map_err(|_| HostProblem::Unsupported)?;
        Some(normalize(database_name))
    };
    if request.pcb == 0 {
        let site = ImsCallSite {
            official_row: format!("ibm-ims-15.6-dli-2026-08-31:dli-call-families:{row:04}"),
            name,
            syntax: system.syntax,
            context: system.context,
            pcb_kind: Some(ImsPcbKind::Io),
            organization: None,
            processing_option: None,
            ssa_form,
        };
        validate_ims_call_site(&site).map_err(|_| HostProblem::Unsupported)?;
    }
    if matches!(
        &system.call,
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Initial,
            ..
        }
    ) && system.syntax != ImsCallSyntax::Call
    {
        return Err(HostProblem::Unsupported);
    }
    if let Some(session) = state.sessions.get(run)
        && request.pcb != 0
        && request.pcb != session.pcb
    {
        return Err(HostProblem::Malformed);
    }
    Ok((psb, database))
}

fn validate_shape(request: &ImsRequest) -> Result<&ImsSystemRequest, HostProblem> {
    let system = request.system.as_ref().ok_or(HostProblem::Malformed)?;
    system.validate(mainframe_env_host_api::HostLimits::default())?;
    if request.operation != ImsOperation::System
        || request.q_class.is_some()
        || !request.data.is_empty()
        || !request.segments.is_empty()
        || !request.qualifiers.is_empty()
        || request.checkpoint_id.is_some()
    {
        return Err(HostProblem::Malformed);
    }
    if matches!(
        &system.call,
        ImsSystemCall::Accept { .. } | ImsSystemCall::Query { .. } | ImsSystemCall::Refresh
    ) && request.pcb != 0
    {
        return Err(HostProblem::Malformed);
    }
    if matches!(
        &system.call,
        ImsSystemCall::Position { .. } | ImsSystemCall::Statistics { .. }
    ) && request.pcb == 0
    {
        return Err(HostProblem::Malformed);
    }
    if matches!(&system.call, ImsSystemCall::Dequeue { class: None }) && request.pcb == 0
        || matches!(&system.call, ImsSystemCall::Dequeue { class: Some(_) }) && request.pcb != 0
    {
        return Err(HostProblem::Malformed);
    }
    Ok(system)
}

pub(super) fn resources(
    state: &State,
    invocation: &Invocation,
    request: &ImsRequest,
) -> Result<Vec<EnterpriseResource>, HostProblem> {
    let system = validate_shape(request)?;
    let batch_context = matches!(
        system.context,
        ImsExecutionContext::DbBatch | ImsExecutionContext::TmBatch
    );
    if (invocation.service_class == ServiceClass::Batch) != batch_context {
        return Err(HostProblem::Unsupported);
    }
    let run = invocation.run_unit_id.as_str();
    let (psb, database) = validate_call_site(state, run, request, system)?;
    let mut resources = vec![EnterpriseResource::new(
        EnterpriseResourceClass::ImsPsb,
        psb.clone(),
        AccessIntent::Execute,
    )?];
    let mut databases = BTreeSet::new();
    if let Some(database) = &database {
        databases.insert(database.clone());
    }
    if matches!(
        &system.call,
        ImsSystemCall::Accept { .. } | ImsSystemCall::Refresh
    ) {
        for pcb in 1..=psb_pcb_count(state, &psb)? {
            if let Ok((name, _, _)) = metadata_pcb(state, &psb, pcb as u16) {
                databases.insert(normalize(name));
            }
        }
    }
    if let ImsSystemCall::Query { target_pcb } = &system.call {
        databases.insert(normalize(metadata_pcb(state, &psb, *target_pcb)?.0));
    }
    if matches!(&system.call, ImsSystemCall::Dequeue { .. }) {
        if let Some(row) = state.system.get(ROW_KEY) {
            for (key, reservation) in &row.reservations {
                if reservation.owner == run
                    && database
                        .as_ref()
                        .is_none_or(|name| key.starts_with(&format!("{name}:")))
                {
                    if let Some(name) = key.split(':').next() {
                        databases.insert(name.into());
                    }
                }
            }
        }
    }
    let intent = if matches!(&system.call, ImsSystemCall::Dequeue { .. }) {
        AccessIntent::Update
    } else {
        AccessIntent::Read
    };
    resources.extend(
        databases
            .into_iter()
            .map(|name| EnterpriseResource::new(EnterpriseResourceClass::ImsDatabase, name, intent))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(resources)
}

fn output(
    status: &str,
    result: ImsSystemResult,
    pcb_kind: ImsPcbKind,
) -> Result<ImsResult, HostProblem> {
    let context = if pcb_kind == ImsPcbKind::Io {
        ImsStatusContext::SystemService
    } else {
        ImsStatusContext::Database
    };
    resolve_ims_status(status.as_bytes(), context, pcb_kind)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(ImsResult {
        status: status.into(),
        segments: Vec::new(),
        checkpoint_id: None,
        affected_segments: 0,
        system: Some(result),
    })
}

pub(super) fn apply_request(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let system = validate_shape(request)?;
    let (psb, database) = validate_call_site(state, run, request, system)?;
    let pcb_kind = if request.pcb == 0 {
        ImsPcbKind::Io
    } else {
        ImsPcbKind::Database
    };
    let result = match &system.call {
        ImsSystemCall::Accept { group, .. } => {
            let count = psb_pcb_count(state, &psb)?;
            if count > limits.max_pcbs {
                return Err(HostProblem::ResourceExhausted);
            }
            let session = state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?;
            let session = &mut Arc::make_mut(session).system;
            if session.accepted.is_some() {
                return Err(HostProblem::Malformed);
            }
            session.accepted = Some(*group);
            output("  ", ImsSystemResult::Accepted { group: *group }, pcb_kind)
        }
        ImsSystemCall::Query { target_pcb } => {
            let session = &state.sessions.get(run).ok_or(HostProblem::NotFound)?.system;
            let pcb = pcb_availability(state, &psb, *target_pcb, &session.dib_statuses)?;
            output("  ", ImsSystemResult::Query { pcb }, pcb_kind)
        }
        ImsSystemCall::Refresh => {
            let session = &state.sessions.get(run).ok_or(HostProblem::NotFound)?.system;
            if !session.last_database_call || session.refresh_used {
                return Err(HostProblem::Malformed);
            }
            let statuses = session.statuses.clone();
            let mut pcbs = Vec::new();
            for number in 1..=psb_pcb_count(state, &psb)? {
                if let Ok(pcb) = pcb_availability(state, &psb, number as u16, &statuses) {
                    pcbs.push(pcb);
                }
            }
            let session =
                &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?)
                    .system;
            session.dib_statuses = statuses;
            session.refresh_used = true;
            output("  ", ImsSystemResult::Refreshed { pcbs }, pcb_kind)
        }
        ImsSystemCall::Dequeue { class } => {
            let expected = class.map(ImsQClass::byte);
            let prefix = database.as_ref().map(|name| format!("{name}:"));
            let row = system_state(state);
            let previous = row.reservations.len();
            row.reservations.retain(|key, reservation| {
                reservation.owner != run
                    || prefix
                        .as_ref()
                        .is_some_and(|prefix| !key.starts_with(prefix))
                    || reservation.current
                    || reservation.modified
                    || expected.is_some_and(|class| class != reservation.class)
            });
            output(
                "  ",
                ImsSystemResult::Dequeued {
                    released: (previous - row.reservations.len()) as u32,
                },
                pcb_kind,
            )
        }
        ImsSystemCall::Gscd => {
            let directory = runtime(state)?
                .directory
                .as_ref()
                .ok_or(HostProblem::NotFound)?;
            let status = state
                .sessions
                .get(run)
                .and_then(|session| session.system.statuses.get(&request.pcb))
                .map(String::as_str)
                .unwrap_or("  ");
            output(
                status,
                ImsSystemResult::Gscd {
                    scd_address: directory.scd_address,
                    pst_address: directory.pst_address,
                },
                pcb_kind,
            )
        }
        ImsSystemCall::Position { ssa, keyword } => {
            let database = database.ok_or(HostProblem::Malformed)?;
            let metadata = state.metadata.as_ref().ok_or(HostProblem::Unsupported)?;
            let db = metadata
                .databases
                .iter()
                .find(|db| normalize(&db.name) == database)
                .ok_or(HostProblem::NotFound)?;
            if db.organization != ImsDatabaseOrganization::Dedb {
                return Err(HostProblem::Unsupported);
            }
            let mut qualified_missing = false;
            if let Some(ssa) = ssa {
                let segment = db
                    .segments
                    .iter()
                    .find(|item| normalize(&item.name) == ssa.segment)
                    .ok_or(HostProblem::Malformed)?;
                if let (Some(field), Some(value)) = (&ssa.field, &ssa.value) {
                    if !segment.fields.iter().any(|item| {
                        item.name
                            .as_deref()
                            .is_some_and(|name| normalize(name) == *field)
                            && item.length == value.len()
                    }) {
                        return Err(HostProblem::Malformed);
                    }
                    // A qualified POS requires a matching retained DEDB record.
                    qualified_missing = if let Some(image) = state.generic_databases.get(&database)
                    {
                        let engine = crate::database::DatabaseEngine::restore(
                            (**image).clone(),
                            generic::engine_limits(limits),
                        )
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                        !engine.ordered_records().iter().any(|record| {
                            record.segment == ssa.segment
                                && segment.fields.iter().any(|item| {
                                    item.name
                                        .as_deref()
                                        .is_some_and(|name| normalize(name) == *field)
                                        && record.data.get(item.offset..item.offset + item.length)
                                            == Some(value.as_slice())
                                })
                        })
                    } else {
                        true
                    };
                }
            }
            let areas = state
                .system
                .get(ROW_KEY)
                .and_then(|row| row.runtime.as_ref().map(|runtime| (row, runtime)))
                .map(|(row, runtime)| {
                    runtime
                        .dedb_areas
                        .iter()
                        .filter(|area| normalize(&area.database) == database)
                        .filter_map(|area| row.areas.get(&normalize(&area.name)).cloned())
                        .map(|mut area| {
                            if matches!(keyword, ImsPositionKeyword::V5SegmentRba) {
                                area.timestamp = None;
                                area.ims_id = None;
                            }
                            area
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            output(
                if qualified_missing {
                    "GE"
                } else if areas.is_empty() {
                    "FH"
                } else {
                    "  "
                },
                ImsSystemResult::Positioned {
                    areas: if qualified_missing { Vec::new() } else { areas },
                },
                pcb_kind,
            )
        }
        ImsSystemCall::Statistics { function } => {
            let database = database.ok_or(HostProblem::Malformed)?;
            let kind = match function.family {
                ImsStatisticsFamily::Dbas | ImsStatisticsFamily::Dbes => ImsBufferPoolKind::Osam,
                ImsStatisticsFamily::Vbas | ImsStatisticsFamily::Vbes => ImsBufferPoolKind::Vsam,
            };
            let candidates = state
                .system
                .get(ROW_KEY)
                .map(|row| {
                    row.pools
                        .values()
                        .filter(|pool| pool.kind == kind)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let session =
                &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?)
                    .system;
            let cursor_key = format!("{database}:{:?}", function.family);
            let cursor = session.stat_cursor.entry(cursor_key).or_default();
            let pool = candidates.get(*cursor).cloned();
            if pool.is_some() {
                *cursor += 1;
            }
            output(
                if pool.is_some() { "  " } else { "GE" },
                ImsSystemResult::Statistics { pool },
                pcb_kind,
            )
        }
    }?;
    if request.pcb != 0
        && !matches!(&system.call, ImsSystemCall::Gscd)
        && let Some(session) = state.sessions.get_mut(run)
    {
        Arc::make_mut(session)
            .system
            .statuses
            .insert(request.pcb, result.status.clone());
    }
    Ok(result)
}

pub(super) fn observe_database_call(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    result: &ImsResult,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if matches!(
        request.operation,
        ImsOperation::Commit | ImsOperation::Rollback | ImsOperation::Terminate
    ) {
        if let Some(row) = state.system.get_mut(ROW_KEY) {
            Arc::make_mut(row)
                .reservations
                .retain(|_, reservation| reservation.owner != run);
        }
        return Ok(());
    }
    if matches!(
        request.operation,
        ImsOperation::Schedule
            | ImsOperation::Checkpoint
            | ImsOperation::Load
            | ImsOperation::Unload
    ) {
        return Ok(());
    }
    let Some(session) = state.sessions.get(run).cloned() else {
        return Ok(());
    };
    let session_system =
        &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).system;
    session_system
        .statuses
        .insert(session.pcb, result.status.clone());
    session_system.last_database_call = true;
    let is_get = matches!(
        request.operation,
        ImsOperation::GetUnique
            | ImsOperation::GetNext
            | ImsOperation::GetNextParent
            | ImsOperation::GetHoldUnique
            | ImsOperation::GetHoldNext
            | ImsOperation::GetHoldNextParent
    );
    if request.q_class.is_some() && !is_get {
        return Err(HostProblem::Malformed);
    }
    let class = request.q_class;
    if class.is_some_and(|class| !class.is_valid()) {
        return Err(HostProblem::Malformed);
    }
    let (database, _, _) = metadata_pcb(state, &session.psb, session.pcb)?;
    let database = normalize(database);
    let location = if session.generic {
        state
            .sessions
            .get(run)
            .and_then(|session| session.position.current())
            .map(|id| serde_json::to_string(&id).map_err(|_| HostProblem::InfrastructureFailure))
            .transpose()?
    } else {
        state
            .sessions
            .get(run)
            .and_then(|session| session.last.as_ref())
            .map(|location| {
                serde_json::to_string(location).map_err(|_| HostProblem::InfrastructureFailure)
            })
            .transpose()?
    };
    if is_get && result.status == "  " {
        let row = system_state(state);
        for reservation in row
            .reservations
            .values_mut()
            .filter(|reservation| reservation.owner == run)
        {
            reservation.current = false;
        }
        if let (Some(class), Some(location)) = (class, location) {
            let key = format!("{database}:{location}");
            if row
                .reservations
                .get(&key)
                .is_some_and(|existing| existing.owner != run)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            if row.reservations.len() >= limits.max_roots && !row.reservations.contains_key(&key) {
                return Err(HostProblem::ResourceExhausted);
            }
            row.reservations.insert(
                key,
                Reservation {
                    owner: run.into(),
                    class: class.byte(),
                    current: true,
                    modified: false,
                },
            );
        }
    } else if matches!(
        request.operation,
        ImsOperation::Replace | ImsOperation::Delete
    ) && result.status == "  "
    {
        if let Some(row) = state.system.get_mut(ROW_KEY) {
            for reservation in Arc::make_mut(row)
                .reservations
                .values_mut()
                .filter(|reservation| reservation.owner == run && reservation.current)
            {
                reservation.modified = true;
            }
        }
    }
    Ok(())
}

pub(super) fn validate_state(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    if state.system.len() > 1 || state.system.keys().any(|key| key != ROW_KEY) {
        return Err(HostProblem::InfrastructureFailure);
    }
    let Some(row) = state.system.get(ROW_KEY) else {
        return Ok(());
    };
    if row.areas.len() > limits.max_databases.saturating_mul(limits.max_segments)
        || row.pools.len() > limits.max_segments
        || row.reservations.len() > limits.max_roots
        || row.reservations.values().any(|reservation| {
            !state.sessions.contains_key(&reservation.owner)
                || ImsQClass::new(reservation.class).is_none()
        })
    {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(runtime) = &row.runtime {
        validate_runtime(state, runtime, limits)?;
        if runtime.dedb_areas.len() != row.areas.len()
            || runtime.buffer_pools.len() != row.pools.len()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    } else if !row.areas.is_empty() || !row.pools.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    for session in state.sessions.values().chain(state.checkpoints.values()) {
        if session.system.statuses.len() > limits.max_pcbs
            || session.system.dib_statuses.len() > limits.max_pcbs
            || session.system.stat_cursor.len() > limits.max_segments
            || session
                .system
                .statuses
                .iter()
                .chain(session.system.dib_statuses.iter())
                .any(|(pcb, status)| {
                    *pcb == 0
                        || resolve_ims_status(
                            status.as_bytes(),
                            ImsStatusContext::Database,
                            ImsPcbKind::Database,
                        )
                        .is_err()
                })
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

fn validate_runtime(
    state: &State,
    runtime: &ImsSystemRuntimeDefinition,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if runtime.dedb_areas.len() > limits.max_databases.saturating_mul(limits.max_segments)
        || runtime.buffer_pools.len() > limits.max_segments
        || runtime
            .directory
            .as_ref()
            .is_some_and(|directory| directory.scd_address == 0 || directory.pst_address == 0)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut names = BTreeSet::new();
    for area in &runtime.dedb_areas {
        let database = normalize(&area.database);
        if area.name.is_empty()
            || area.name.len() > 8
            || !names.insert(normalize(&area.name))
            || area.sdep_capacity_cis == 0
            || area.iov_capacity_cis == 0
            || state.metadata.as_ref().is_none_or(|metadata| {
                !metadata.databases.iter().any(|db| {
                    normalize(&db.name) == database
                        && db.organization == ImsDatabaseOrganization::Dedb
                })
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    names.clear();
    for pool in &runtime.buffer_pools {
        if pool.name.is_empty()
            || pool.name.len() > 8
            || !names.insert(normalize(&pool.name))
            || pool.buffer_bytes == 0
            || pool.buffers == 0
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

impl ImsService {
    /// Install bounded system resources in the existing IMS provider store.
    pub fn install_system_runtime(
        &self,
        runtime: ImsSystemRuntimeDefinition,
    ) -> Result<(), HostProblem> {
        let mut durable = self.lock()?;
        validate_runtime(&durable.state, &runtime, self.limits)?;
        if let Some(current) = durable
            .state
            .system
            .get(ROW_KEY)
            .and_then(|row| row.runtime.as_ref())
        {
            return if current == &runtime {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let mut next = durable.state.scoped_snapshot();
        let row = system_state(&mut next);
        for area in &runtime.dedb_areas {
            row.areas.insert(
                normalize(&area.name),
                ImsPositionArea {
                    name: normalize(&area.name),
                    position: [0; 8],
                    unused_sdep_cis: area.sdep_capacity_cis,
                    unused_iov_cis: area.iov_capacity_cis,
                    timestamp: None,
                    ims_id: None,
                },
            );
        }
        for pool in &runtime.buffer_pools {
            row.pools.insert(
                normalize(&pool.name),
                ImsBufferStatistics {
                    pool: normalize(&pool.name),
                    kind: pool.kind,
                    buffer_bytes: pool.buffer_bytes,
                    buffers: pool.buffers,
                    reads: 0,
                    writes: 0,
                },
            );
        }
        row.runtime = Some(runtime);
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    /// Publish one DEDB area's position from the IMS database authority.
    pub fn publish_dedb_area_position(&self, area: ImsPositionArea) -> Result<(), HostProblem> {
        let mut durable = self.lock()?;
        let name = normalize(&area.name);
        let configured: &ImsDedbAreaDefinition = runtime(&durable.state)?
            .dedb_areas
            .iter()
            .find(|configured| normalize(&configured.name) == name)
            .ok_or(HostProblem::NotFound)?;
        if area.name != name
            || area.unused_sdep_cis > configured.sdep_capacity_cis
            || area.unused_iov_cis > configured.iov_capacity_cis
            || area.ims_id.as_ref().is_some_and(|id| id.len() > 8)
        {
            return Err(HostProblem::Malformed);
        }
        let mut next = durable.state.scoped_snapshot();
        system_state(&mut next).areas.insert(name, area);
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    /// Publish bounded buffer-pool counters from the IMS buffer authority.
    pub fn publish_buffer_statistics(
        &self,
        statistics: ImsBufferStatistics,
    ) -> Result<(), HostProblem> {
        let mut durable = self.lock()?;
        let name = normalize(&statistics.pool);
        let configured = runtime(&durable.state)?
            .buffer_pools
            .iter()
            .find(|configured| normalize(&configured.name) == name)
            .ok_or(HostProblem::NotFound)?;
        if statistics.pool != name
            || statistics.kind != configured.kind
            || statistics.buffer_bytes != configured.buffer_bytes
            || statistics.buffers != configured.buffers
        {
            return Err(HostProblem::Malformed);
        }
        let mut next = durable.state.scoped_snapshot();
        system_state(&mut next).pools.insert(name, statistics);
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }
}
