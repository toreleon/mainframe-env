//! Private selected-route assurance over existing typed persisted receipts.
//! No host dispatch, public diagnostic schema, raw request receipt or clock credit.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsTraceEntry,
    describe_cics_replay_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, AuditResourceDigestFormat, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_store_api::{
    ExecutionRecord, ExecutionState, PlatformStore, ProviderStateRecord,
};

mod oracles;
#[cfg(test)]
mod tests;

const MAX_STAGE_ROWS: usize = 512;
const MAX_STAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ACTOR_AUDITS: usize = 512;
const MAX_STAGE_TRACE: usize = 512;
const REPLAY_NAMESPACE: &str = "cics-effect-replay-v1";

#[derive(Clone)]
pub(super) struct BeforeStage {
    rows: BTreeMap<String, ProviderStateRecord>,
    trace: Vec<CicsTraceEntry>,
}

#[derive(Clone, Copy)]
pub(super) enum Route {
    Signon,
    Account,
}

#[derive(Clone, Copy)]
pub(super) struct StageContext<'a> {
    store: &'a dyn PlatformStore,
    server: &'a ProductServer,
    corpus: &'a Path,
    session: &'a str,
    installed: &'a BTreeMap<String, ArtifactRef>,
}

impl<'a> StageContext<'a> {
    pub(super) fn new(
        store: &'a dyn PlatformStore,
        server: &'a ProductServer,
        corpus: &'a Path,
        session: &'a str,
        installed: &'a BTreeMap<String, ArtifactRef>,
    ) -> Self {
        Self {
            store,
            server,
            corpus,
            session,
            installed,
        }
    }
}

impl Route {
    fn programs(self) -> &'static [&'static str] {
        match self {
            Self::Signon => &["COSGN00C", "COMEN01C"],
            Self::Account => &["COACTVWC"],
        }
    }

    fn terminal_kind(self, program: &str) -> Option<LifecycleEventKind> {
        // Pinned regular sign-on replaces COSGN00C with COMEN01C through XCTL.
        // The target and the account-detail actor end through ordinary RETURN.
        match (self, program) {
            (Self::Signon, "COSGN00C") => Some(LifecycleEventKind::HandoffCompleted),
            (Self::Signon, "COMEN01C") | (Self::Account, "COACTVWC") => {
                Some(LifecycleEventKind::Completed { return_code: 0 })
            }
            _ => None,
        }
    }

    fn reading_program(self) -> &'static str {
        match self {
            Self::Signon => "COSGN00C",
            Self::Account => "COACTVWC",
        }
    }

    fn reads(self) -> &'static [oracles::ReadOracle] {
        match self {
            Self::Signon => &oracles::SIGNON_READ,
            Self::Account => &oracles::ACCOUNT_READS,
        }
    }
}

fn refuse() -> CorpusProblem {
    // Never serialize captured provider rows, customer data, audits or HTTP bodies.
    CorpusProblem::new(
        "carddemo.online.read_request_binding",
        "selected compiled READ request or its persisted owner evidence differs",
    )
}

pub(super) fn before_stage(
    store: &dyn PlatformStore,
    server: &ProductServer,
    session: &str,
) -> Result<BeforeStage, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(REPLAY_NAMESPACE, MAX_STAGE_ROWS + 1, MAX_STAGE_BYTES)
        .map_err(|_| refuse())?;
    let trace = server.online_trace(session).map_err(|_| refuse())?;
    if rows.len() > MAX_STAGE_ROWS || trace.len() > MAX_STAGE_TRACE {
        return Err(refuse());
    }
    let count = rows.len();
    let rows = rows
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    if rows.len() != count {
        return Err(refuse());
    }
    Ok(BeforeStage { rows, trace })
}

fn changed_rows<'a>(
    before: &BeforeStage,
    after: &'a BeforeStage,
) -> Result<Vec<&'a ProviderStateRecord>, CorpusProblem> {
    // This isolated existing fixture owns this stage. Historical mutation,
    // removal, a truncated page or a changed prefix grants no freshness proof.
    if before
        .rows
        .iter()
        .any(|(key, row)| after.rows.get(key) != Some(row))
        || !after.trace.starts_with(&before.trace)
    {
        return Err(refuse());
    }
    let changed = after
        .rows
        .iter()
        .filter(|(key, _)| !before.rows.contains_key(*key))
        .map(|(_, row)| row)
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return Err(refuse());
    }
    Ok(changed)
}

fn terminal_surface_matches(terminal: &serde_json::Value, session: &str, route: Route) -> bool {
    let (mapset, map) = match route {
        Route::Signon => ("COMEN01", "COMEN1A"),
        Route::Account => ("COACTVW", "CACTVWA"),
    };
    // Both selected stages consume supplied ENTER input, SEND, then RETURN.
    // Empty-input RECEIVE is the separate operation that sets suspended=true.
    terminal["session"] == session
        && terminal["principal"] == "WEBUSER"
        && terminal["mapset"] == mapset
        && terminal["map"] == map
        && terminal["connected"] == true
        && terminal["suspended"] == false
}

fn terminal_events_match(
    route: Route,
    program: &str,
    execution: &ExecutionRecord,
    run: &RunUnitId,
    events: &[LifecycleEvent],
) -> bool {
    let Some(expected_kind) = route.terminal_kind(program) else {
        return false;
    };
    matches!(events, [event] if event.validate()
        && event.execution_id == execution.execution_id && event.run_unit_id == *run
        && event.sequence == execution.version && event.attempt == execution.attempt
        && Some(event.tick) == execution.terminal_tick && event.tick != 0
        && event.kind == expected_kind)
}

fn check_execution(
    execution: &ExecutionRecord,
    program: &str,
    expected_artifact: &ArtifactRef,
    run: &RunUnitId,
) -> Result<(), CorpusProblem> {
    if execution.selector.as_str() != format!("program:{program}")
        || execution.artifact != *expected_artifact
        || execution.run_unit_id != *run
        || execution.principal.as_str() != "WEBUSER"
        || execution.attempt == 0
        || execution.state != ExecutionState::Completed
        || execution.terminal_tick.is_none_or(|tick| tick == 0)
    {
        return Err(refuse());
    }
    Ok(())
}

fn compare_audits(
    execution: &ExecutionRecord,
    invocation_key: &IdempotencyKey,
    terminal_sequence: u64,
    audits: &[AuditRecord],
    expected: &[oracles::ReadOracle],
) -> Result<(), CorpusProblem> {
    if audits.is_empty()
        || audits.len() > MAX_ACTOR_AUDITS
        || audits.iter().any(|audit| {
            audit.execution_id != execution.execution_id
                || audit.run_unit_id != execution.run_unit_id
                || audit.principal != execution.principal
                || audit.attempt != execution.attempt
                || audit.invocation_key != *invocation_key
                || audit.effect_sequence == 0
                || audit.decision != AuditDecision::Success
        })
    {
        return Err(refuse());
    }
    let full = AuditResourceDigestFormat::CanonicalHostResourceV1;
    let expected_outer = expected
        .iter()
        .map(oracles::ReadOracle::cics_digest)
        .collect::<Result<Vec<_>, _>>()?;
    let mut actual_outer = audits
        .iter()
        .filter(|audit| {
            audit.capability.as_str() == "host.cics.execute"
                && expected_outer
                    .iter()
                    .any(|digest| audit.resource.value == digest.value)
        })
        .collect::<Vec<_>>();
    actual_outer.sort_by_key(|audit| audit.effect_sequence);
    if actual_outer.len() != expected_outer.len()
        || actual_outer
            .iter()
            .zip(&expected_outer)
            .any(|(actual, expected)| {
                actual.resource.format != full
                    || actual.resource != *expected
                    || actual.effect_sequence >= terminal_sequence
            })
        || actual_outer
            .windows(2)
            .any(|pair| pair[0].effect_sequence >= pair[1].effect_sequence)
    {
        return Err(refuse());
    }
    // This program's source-owned dataset capability contains exactly an
    // Attributes preflight and the plain Read for each selected CICS READ.
    // Do not assert a total count across unrelated security/terminal/clock audits.
    let expected_inner = expected
        .iter()
        .flat_map(|read| [read.attributes_digest(), read.dataset_digest()])
        .collect::<Result<Vec<_>, _>>()?;
    let mut actual_inner = audits
        .iter()
        .filter(|audit| audit.capability.as_str() == "host.dataset.read")
        .collect::<Vec<_>>();
    actual_inner.sort_by_key(|audit| audit.effect_sequence);
    if actual_inner.len() != expected_inner.len()
        || actual_inner
            .iter()
            .zip(&expected_inner)
            .any(|(actual, expected)| {
                actual.resource.format != full || actual.resource != *expected
            })
        || actual_inner
            .windows(2)
            .any(|pair| pair[0].effect_sequence >= pair[1].effect_sequence)
    {
        return Err(refuse());
    }
    Ok(())
}

pub(super) fn compare_stage(
    context: &StageContext<'_>,
    before: &BeforeStage,
    terminal: &serde_json::Value,
    route: Route,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    let StageContext {
        store,
        server,
        corpus,
        session,
        installed,
    } = *context;
    oracles::check_inputs(corpus)?;
    if !terminal_surface_matches(terminal, session, route) {
        return Err(refuse());
    }
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(refuse)?,
        InvocationLimits::default(),
    )
    .map_err(|_| refuse())?;
    let after = before_stage(store, server, session)?;
    let changed = changed_rows(before, &after)?;
    let mut owners = BTreeMap::<String, ExecutionRecord>::new();
    let mut invocation_keys = BTreeMap::<String, IdempotencyKey>::new();
    let mut terminal_sequences = BTreeMap::<String, u64>::new();
    for row in changed {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| refuse())?;
        let effect = store
            .effect(&key)
            .map_err(|_| refuse())?
            .ok_or_else(refuse)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refuse())?;
        if descriptor.codec != CicsReplayCodecVersion::RetentionV3
            || descriptor.retention != CicsReplayRetentionState::Terminal
            || descriptor.owner_run_unit.as_deref() != Some(run.as_str())
        {
            return Err(refuse());
        }
        let id = ExecutionId::new(
            descriptor.owner_execution.as_deref().ok_or_else(refuse)?,
            InvocationLimits::default(),
        )
        .map_err(|_| refuse())?;
        let execution = store
            .get_execution(&id)
            .map_err(|_| refuse())?
            .ok_or_else(refuse)?;
        let program = execution
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or_else(refuse)?;
        if !route.programs().contains(&program) || execution.execution_id != id {
            return Err(refuse());
        }
        check_execution(
            &execution,
            program,
            installed.get(program).ok_or_else(refuse)?,
            &run,
        )?;
        let events = store
            .events(&id, execution.version, 2)
            .map_err(|_| refuse())?;
        if !terminal_events_match(route, program, &execution, &run, &events)
            || effect.intent.attempt != execution.attempt
            || effect.intent.capability.as_ref().map(CapabilityId::as_str)
                != Some("host.cics.execute")
            || effect.intent.audit_resource.is_none_or(|digest| {
                digest.format != AuditResourceDigestFormat::CanonicalHostResourceV1
            })
        {
            return Err(refuse());
        }
        let invocation_key = effect
            .intent
            .audit_invocation_key
            .clone()
            .ok_or_else(refuse)?;
        if owners
            .get(program)
            .is_some_and(|previous| previous != &execution)
            || invocation_keys
                .get(program)
                .is_some_and(|previous| previous != &invocation_key)
        {
            return Err(refuse());
        }
        let program = program.to_string();
        invocation_keys.insert(program.clone(), invocation_key);
        terminal_sequences
            .entry(program.clone())
            .and_modify(|sequence| *sequence = (*sequence).max(effect.sequence))
            .or_insert(effect.sequence);
        owners.insert(program, execution);
    }
    if owners.len() != route.programs().len()
        || route
            .programs()
            .iter()
            .any(|program| !owners.contains_key(*program))
    {
        return Err(refuse());
    }
    let actor = owners.get(route.reading_program()).ok_or_else(refuse)?;
    let audits = store
        .audit_records(&actor.execution_id, 0, MAX_ACTOR_AUDITS + 1)
        .map_err(|_| refuse())?;
    compare_audits(
        actor,
        &invocation_keys[route.reading_program()],
        terminal_sequences[route.reading_program()],
        &audits,
        route.reads(),
    )?;
    let trace = &after.trace[before.trace.len()..];
    let reads = trace
        .iter()
        .filter(|entry| entry.operation == CicsOperation::Read)
        .collect::<Vec<_>>();
    if reads.len() != route.reads().len()
        || reads.iter().zip(route.reads()).any(|(entry, expected)| {
            entry.outcome != "NORMAL"
                || entry.response != 0
                || entry.response2 != 0
                || entry.payload_bytes != expected.record_length
        })
        || trace
            .iter()
            .filter(|entry| {
                entry.operation == CicsOperation::Return
                    && entry.outcome == "NORMAL"
                    && entry.response == 0
                    && entry.response2 == 0
            })
            .count()
            != 1
        || (matches!(route, Route::Signon)
            && trace
                .iter()
                .filter(|entry| {
                    entry.operation == CicsOperation::Xctl
                        && entry.outcome == "NORMAL"
                        && entry.response == 0
                        && entry.response2 == 0
                })
                .count()
                != 1)
    {
        return Err(refuse());
    }
    let (id, requirement) = match route {
        Route::Signon => ("CD.J03", "USRSEC keyed read"),
        Route::Account => ("CD.J04", "ACCTDAT and cross-reference reads"),
    };
    // This remains transient until the original wrapper's both server drains,
    // owned artifact cleanup and all original predicates complete.
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        id,
        requirement,
        false,
        || Ok(refuse()),
    )
}
