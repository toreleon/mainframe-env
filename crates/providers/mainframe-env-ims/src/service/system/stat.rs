//! Bounded typed STAT projection on the existing runtime, session and row authority.
use super::*;
use mainframe_env_host_api::{
    ImsBufferPoolKind, ImsStatisticsFormat, ImsStatisticsFunction, ImsStatisticsObservationV2,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CursorV2 {
    next: usize,
}

pub(super) fn is_stat(call: &ImsSystemCall) -> bool {
    matches!(
        call,
        ImsSystemCall::Statistics { .. } | ImsSystemCall::StatisticsV2 { .. }
    )
}

pub(super) fn reset(session: &mut SystemSession, pcb: u16) {
    session.stat_cursors_v2.remove(&pcb);
}

pub(super) fn observe_database_call(state: &mut State, run: &str, request: &ImsRequest) {
    if let Some(session) = state.sessions.get_mut(run) {
        let session = Arc::make_mut(session);
        if matches!(
            request.operation,
            ImsOperation::Commit
                | ImsOperation::Rollback
                | ImsOperation::Terminate
                | ImsOperation::Checkpoint
                | ImsOperation::Schedule
        ) {
            session.system.stat_cursors_v2.clear();
        } else if request.pcb != 0
            && matches!(
                request.operation,
                ImsOperation::GetUnique
                    | ImsOperation::GetNext
                    | ImsOperation::GetNextParent
                    | ImsOperation::GetHoldUnique
                    | ImsOperation::GetHoldNext
                    | ImsOperation::GetHoldNextParent
                    | ImsOperation::Insert
                    | ImsOperation::Replace
                    | ImsOperation::Delete
            )
        {
            reset(&mut session.system, request.pcb);
        }
    }
}

fn candidates(
    row: &SystemState,
    function: ImsStatisticsFunction,
) -> Result<Vec<&ImsBufferStatistics>, HostProblem> {
    let runtime = row.runtime.as_ref().ok_or(HostProblem::NotFound)?;
    let kind = match function.family {
        ImsStatisticsFamily::Dbas => ImsBufferPoolKind::Osam,
        ImsStatisticsFamily::Vbas => ImsBufferPoolKind::Vsam,
        _ => return Err(HostProblem::Unsupported),
    };
    let mut pools = row
        .pools
        .values()
        .filter(|pool| pool.kind == kind)
        .collect::<Vec<_>>();
    if kind == ImsBufferPoolKind::Vsam && !pools.is_empty() {
        if runtime.vsam_subpools_v2.len() != pools.len() {
            return Err(HostProblem::Unsupported);
        }
        let mut ordered = pools
            .into_iter()
            .map(|pool| {
                let metadata = runtime
                    .vsam_subpools_v2
                    .iter()
                    .find(|m| m.subpool == pool.pool)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                Ok((
                    (
                        metadata.definition_order,
                        metadata.subpool_type,
                        pool.buffer_bytes,
                    ),
                    pool,
                ))
            })
            .collect::<Result<Vec<_>, HostProblem>>()?;
        ordered.sort_by_key(|(key, _)| *key);
        pools = ordered.into_iter().map(|(_, pool)| pool).collect();
    }
    if pools.iter().any(|pool| {
        row.published_pools_v2
            .as_ref()
            .is_none_or(|set| !set.contains(&pool.pool))
    }) {
        return Err(HostProblem::NotFound);
    }
    Ok(pools)
}

fn totals(pools: &[&ImsBufferStatistics]) -> Result<ImsStatisticsObservationV2, HostProblem> {
    let mut buffers = 0_u64;
    let mut storage_bytes = 0_u64;
    let mut reads = 0_u64;
    let mut writes = 0_u64;
    for pool in pools {
        buffers = buffers
            .checked_add(u64::from(pool.buffers))
            .ok_or(HostProblem::ResourceExhausted)?;
        storage_bytes = storage_bytes
            .checked_add(u64::from(pool.buffers) * u64::from(pool.buffer_bytes))
            .ok_or(HostProblem::ResourceExhausted)?;
        reads = reads
            .checked_add(pool.reads)
            .ok_or(HostProblem::ResourceExhausted)?;
        writes = writes
            .checked_add(pool.writes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    Ok(ImsStatisticsObservationV2::Totals {
        buffers,
        storage_bytes,
        reads,
        writes,
    })
}

pub(super) fn apply(
    state: &mut State,
    run: &str,
    pcb: u16,
    function: ImsStatisticsFunction,
    v2: bool,
) -> Result<ImsResult, HostProblem> {
    function.minimum_io_area_bytes()?;
    if matches!(
        function.family,
        ImsStatisticsFamily::Dbes | ImsStatisticsFamily::Vbes
    ) {
        return Err(HostProblem::Unsupported);
    }
    let row = state.system.get(ROW_KEY).ok_or(HostProblem::NotFound)?;
    let pools = candidates(row, function)?;
    if !v2 {
        // The old result can represent only one basic OSAM pool. It cannot claim
        // enhanced or aggregate output, a selected format, or proven I/O capacity.
        if function.format != ImsStatisticsFormat::Full
            || !pools.is_empty()
                && (function.family != ImsStatisticsFamily::Dbas || pools.len() != 1)
        {
            return Err(HostProblem::Unsupported);
        }
        let pool = pools.first().map(|pool| (*pool).clone());
        let session =
            &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).system;
        reset(session, pcb);
        return output(
            if pool.is_some() { "  " } else { "GE" },
            ImsSystemResult::Statistics { pool },
            ImsPcbKind::Database,
        );
    }
    let (status, observation, cursor) = if pools.is_empty() {
        ("GE", None, None)
    } else if function.family == ImsStatisticsFamily::Dbas {
        ("  ", Some(totals(&pools)?), None)
    } else {
        let next = state
            .sessions
            .get(run)
            .ok_or(HostProblem::NotFound)?
            .system
            .stat_cursors_v2
            .get(&pcb)
            .map_or(0, |cursor| cursor.next);
        if next < pools.len() {
            (
                "  ",
                Some(ImsStatisticsObservationV2::Subpool {
                    statistics: pools[next].clone(),
                }),
                Some(next + 1),
            )
        } else {
            ("GA", Some(totals(&pools)?), Some(pools.len()))
        }
    };
    let session =
        &mut Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).system;
    if let Some(next) = cursor {
        session.stat_cursors_v2.insert(pcb, CursorV2 { next });
    } else {
        reset(session, pcb);
    }
    output(
        status,
        ImsSystemResult::StatisticsV2 {
            function,
            observation,
        },
        ImsPcbKind::Database,
    )
}

pub(super) fn validate_runtime(runtime: &ImsSystemRuntimeDefinition) -> Result<(), HostProblem> {
    let vsam = runtime
        .buffer_pools
        .iter()
        .filter(|p| p.kind == ImsBufferPoolKind::Vsam)
        .collect::<Vec<_>>();
    if runtime.vsam_subpools_v2.is_empty() {
        return Ok(()); // Historical metadata is readable; VSAM execution requires ordering proof.
    }
    if runtime.vsam_subpools_v2.len() != vsam.len() {
        return Err(HostProblem::Malformed);
    }
    let mut names = BTreeSet::new();
    let mut pool_orders = BTreeMap::new();
    let mut order_pools = BTreeMap::new();
    let mut tuples = BTreeSet::new();
    for m in &runtime.vsam_subpools_v2 {
        let pool = vsam
            .iter()
            .find(|p| normalize(&p.name) == m.subpool)
            .ok_or(HostProblem::Malformed)?;
        if m.subpool != normalize(&m.subpool)
            || !names.insert(&m.subpool)
            || !tuples.insert((m.lsr_pool, m.subpool_type, pool.buffer_bytes))
            || pool_orders
                .insert(m.lsr_pool, m.definition_order)
                .is_some_and(|old| old != m.definition_order)
            || order_pools
                .insert(m.definition_order, m.lsr_pool)
                .is_some_and(|old| old != m.lsr_pool)
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

pub(super) fn validate_state(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    if let Some(row) = state.system.get(ROW_KEY) {
        if row.published_pools_v2.is_some() && row.runtime.is_none() {
            return Err(HostProblem::InfrastructureFailure);
        }
        if row.published_pools_v2.as_ref().is_some_and(|published| {
            published.len() > limits.max_segments
                || published.iter().any(|name| !row.pools.contains_key(name))
        }) {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some(runtime) = &row.runtime {
            for definition in &runtime.buffer_pools {
                let pool = row
                    .pools
                    .get(&normalize(&definition.name))
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if pool.pool != normalize(&definition.name)
                    || pool.kind != definition.kind
                    || pool.buffer_bytes != definition.buffer_bytes
                    || pool.buffers != definition.buffers
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
        }
    }
    for session in state.sessions.values().chain(state.checkpoints.values()) {
        if session.system.stat_cursors_v2.len() > limits.max_pcbs {
            return Err(HostProblem::InfrastructureFailure);
        }
        for (&pcb, cursor) in &session.system.stat_cursors_v2 {
            let (_, _, organization) = metadata_pcb(state, &session.psb, pcb)?;
            if pcb == 0
                || matches!(organization, "DEDB" | "MSDB" | "GSAM")
                || cursor.next == 0
                || state
                    .system
                    .get(ROW_KEY)
                    .and_then(|r| r.runtime.as_ref())
                    .is_none_or(|r| {
                        cursor.next > r.vsam_subpools_v2.len() || r.vsam_subpools_v2.is_empty()
                    })
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
    }
    Ok(())
}
