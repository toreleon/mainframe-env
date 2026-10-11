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
use mainframe_env_host_api::{HostRequest, SecurityRequest, canonical_audit_resource_digest};
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

// Closed metadata only: no provider row, request, key, actor or HTTP value is formatted.
#[derive(Clone, Copy)]
enum RefusalStage {
    Snapshot,
    SnapshotBounds,
    SnapshotFreshness,
    FreshRows,
    Surface,
    RunIdentity,
    ReplayKey,
    CoreEffect,
    ReplayDescriptor,
    ReplayTerminal,
    OwnerIdentity,
    OwnerRecord,
    TerminalEvents,
    InvocationKey,
    OwnerAmbiguity,
    OwnerSet,
    AuditRead,
    AuditOwner,
    AuditOracle,
    OuterRead,
    InnerRead,
    Trace,
}

fn refuse_at(stage: RefusalStage, route: Option<Route>, row: Option<usize>) -> CorpusProblem {
    let stage = match stage {
        RefusalStage::Snapshot => "snapshot",
        RefusalStage::SnapshotBounds => "snapshot-bounds",
        RefusalStage::SnapshotFreshness => "snapshot-freshness",
        RefusalStage::FreshRows => "fresh-rows",
        RefusalStage::Surface => "surface",
        RefusalStage::RunIdentity => "run-identity",
        RefusalStage::ReplayKey => "replay-key",
        RefusalStage::CoreEffect => "core-effect",
        RefusalStage::ReplayDescriptor => "replay-descriptor",
        RefusalStage::ReplayTerminal => "replay-terminal",
        RefusalStage::OwnerIdentity => "owner-identity",
        RefusalStage::OwnerRecord => "owner-record",
        RefusalStage::TerminalEvents => "terminal-events",
        RefusalStage::InvocationKey => "invocation-key",
        RefusalStage::OwnerAmbiguity => "owner-ambiguity",
        RefusalStage::OwnerSet => "owner-set",
        RefusalStage::AuditRead => "audit-read",
        RefusalStage::AuditOwner => "audit-owner",
        RefusalStage::AuditOracle => "audit-oracle",
        RefusalStage::OuterRead => "outer-read",
        RefusalStage::InnerRead => "inner-read",
        RefusalStage::Trace => "trace",
    };
    let route = match route {
        Some(Route::Signon) => "signon",
        Some(Route::Account) => "account",
        None => "unspecified",
    };
    CorpusProblem::new(
        "carddemo.online.read_request_binding",
        format!("selected READ binding refused: route={route}; stage={stage}; row={row:?}"),
    )
}

fn audit_refusal_for_route(error: CorpusProblem, route: Route) -> CorpusProblem {
    // These are the only classified helper refusals. Any unclassified detail is
    // discarded, including an independent frozen request-construction refusal.
    let stage = [
        RefusalStage::AuditOwner,
        RefusalStage::OuterRead,
        RefusalStage::InnerRead,
    ]
    .into_iter()
    .find(|stage| refuse_at(*stage, None, None).detail == error.detail)
    .unwrap_or(RefusalStage::AuditOracle);
    refuse_at(stage, Some(route), None)
}

pub(super) fn before_stage(
    store: &dyn PlatformStore,
    server: &ProductServer,
    session: &str,
) -> Result<BeforeStage, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(REPLAY_NAMESPACE, MAX_STAGE_ROWS + 1, MAX_STAGE_BYTES)
        .map_err(|_| refuse_at(RefusalStage::Snapshot, None, None))?;
    let trace = server
        .online_trace(session)
        .map_err(|_| refuse_at(RefusalStage::Snapshot, None, None))?;
    if rows.len() > MAX_STAGE_ROWS || trace.len() > MAX_STAGE_TRACE {
        return Err(refuse_at(RefusalStage::SnapshotBounds, None, None));
    }
    let count = rows.len();
    let rows = rows
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    if rows.len() != count {
        return Err(refuse_at(RefusalStage::SnapshotBounds, None, None));
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
        return Err(refuse_at(RefusalStage::SnapshotFreshness, None, None));
    }
    let changed = after
        .rows
        .iter()
        .filter(|(key, _)| !before.rows.contains_key(*key))
        .map(|(_, row)| row)
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return Err(refuse_at(RefusalStage::FreshRows, None, None));
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
        return Err(refuse_at(RefusalStage::OwnerRecord, None, None));
    }
    Ok(())
}

// The existing CICS resume admits the fresh task before the server appends the
// selected program to its invocation key. This is a distinct exact admission
// record, not a program effect or a reason to discard unrelated audits.
fn terminal_admission(
    execution: &ExecutionRecord,
    invocation_key: &IdempotencyKey,
) -> Result<
    (
        IdempotencyKey,
        mainframe_env_execution_api::AuditResourceDigest,
    ),
    CorpusProblem,
> {
    let (program, resource) = match execution.selector.as_str() {
        "program:COSGN00C" => ("COSGN00C", "CICS.CC00"),
        // COMEN01C stages CM00, then its resumed task XCTLs COACTVWC. The
        // account's RETURN stages CAVW; the next resume authorizes the retained
        // current CM00 transaction before claiming that continuation.
        "program:COACTVWC" => ("COACTVWC", "CICS.CM00"),
        _ => return Err(refuse_at(RefusalStage::AuditOwner, None, None)),
    };
    let original = invocation_key
        .as_str()
        .strip_suffix(&format!(":{program}"))
        .filter(|key| !key.is_empty())
        .ok_or_else(|| refuse_at(RefusalStage::AuditOwner, None, None))?;
    let original = IdempotencyKey::new(original, InvocationLimits::default())
        .map_err(|_| refuse_at(RefusalStage::AuditOwner, None, None))?;
    let request = HostRequest::Security(SecurityRequest::Authorize {
        principal: execution.principal.clone(),
        class: "TCICSTRN".into(),
        resource: ResourceName::new(resource, 246)
            .map_err(|_| refuse_at(RefusalStage::AuditOwner, None, None))?,
        intent: AccessIntent::Execute,
    });
    Ok((original, canonical_audit_resource_digest(&request)))
}

fn compare_audits(
    execution: &ExecutionRecord,
    invocation_key: &IdempotencyKey,
    terminal_sequence: u64,
    audits: &[AuditRecord],
    expected: &[oracles::ReadOracle],
) -> Result<(), CorpusProblem> {
    let (admission_key, admission_digest) = terminal_admission(execution, invocation_key)?;
    let mut admissions = 0;
    if audits.is_empty()
        || audits.len() > MAX_ACTOR_AUDITS
        || audits.iter().any(|audit| {
            audit.execution_id != execution.execution_id
                || audit.run_unit_id != execution.run_unit_id
                || audit.principal != execution.principal
                || audit.attempt != execution.attempt
                || audit.effect_sequence == 0
                || audit.decision != AuditDecision::Success
                || if audit.invocation_key == *invocation_key {
                    false
                } else if audit.invocation_key == admission_key
                    && audit.capability.as_str() == "host.security.authorize"
                    && audit.resource == admission_digest
                    && audit.effect_sequence == 1
                    && audit.observed_tick == 0
                {
                    admissions += 1;
                    false
                } else {
                    true
                }
        })
        || admissions != 1
    {
        return Err(refuse_at(RefusalStage::AuditOwner, None, None));
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
        return Err(refuse_at(RefusalStage::OuterRead, None, None));
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
        return Err(refuse_at(RefusalStage::InnerRead, None, None));
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
    oracles::check_inputs(corpus).map_err(|error| {
        if error.code == "carddemo.online.read_request_binding" {
            refuse_at(RefusalStage::AuditOracle, Some(route), None)
        } else {
            error
        }
    })?;
    if !terminal_surface_matches(terminal, session, route) {
        return Err(refuse_at(RefusalStage::Surface, Some(route), None));
    }
    let run = RunUnitId::new(
        terminal["run_unit"]
            .as_str()
            .ok_or_else(|| refuse_at(RefusalStage::RunIdentity, Some(route), None))?,
        InvocationLimits::default(),
    )
    .map_err(|_| refuse_at(RefusalStage::RunIdentity, Some(route), None))?;
    let after = before_stage(store, server, session)
        .map_err(|_| refuse_at(RefusalStage::Snapshot, Some(route), None))?;
    let changed = changed_rows(before, &after)
        .map_err(|_| refuse_at(RefusalStage::SnapshotFreshness, Some(route), None))?;
    let mut owners = BTreeMap::<String, ExecutionRecord>::new();
    let mut invocation_keys = BTreeMap::<String, IdempotencyKey>::new();
    let mut terminal_sequences = BTreeMap::<String, u64>::new();
    for (ordinal, row) in changed.into_iter().enumerate() {
        let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
            .map_err(|_| refuse_at(RefusalStage::ReplayKey, Some(route), Some(ordinal)))?;
        let effect = store
            .effect(&key)
            .map_err(|_| refuse_at(RefusalStage::CoreEffect, Some(route), Some(ordinal)))?
            .ok_or_else(|| refuse_at(RefusalStage::CoreEffect, Some(route), Some(ordinal)))?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refuse_at(RefusalStage::ReplayDescriptor, Some(route), Some(ordinal)))?;
        if descriptor.codec != CicsReplayCodecVersion::RetentionV3
            || descriptor.retention != CicsReplayRetentionState::Terminal
            || descriptor.owner_run_unit.as_deref() != Some(run.as_str())
        {
            return Err(refuse_at(
                RefusalStage::ReplayTerminal,
                Some(route),
                Some(ordinal),
            ));
        }
        let id = ExecutionId::new(
            descriptor.owner_execution.as_deref().ok_or_else(|| {
                refuse_at(RefusalStage::OwnerIdentity, Some(route), Some(ordinal))
            })?,
            InvocationLimits::default(),
        )
        .map_err(|_| refuse_at(RefusalStage::OwnerIdentity, Some(route), Some(ordinal)))?;
        let execution = store
            .get_execution(&id)
            .map_err(|_| refuse_at(RefusalStage::OwnerRecord, Some(route), Some(ordinal)))?
            .ok_or_else(|| refuse_at(RefusalStage::OwnerRecord, Some(route), Some(ordinal)))?;
        let program = execution
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or_else(|| refuse_at(RefusalStage::OwnerIdentity, Some(route), Some(ordinal)))?;
        if !route.programs().contains(&program) || execution.execution_id != id {
            return Err(refuse_at(
                RefusalStage::OwnerIdentity,
                Some(route),
                Some(ordinal),
            ));
        }
        check_execution(
            &execution,
            program,
            installed
                .get(program)
                .ok_or_else(|| refuse_at(RefusalStage::OwnerRecord, Some(route), Some(ordinal)))?,
            &run,
        )
        .map_err(|_| refuse_at(RefusalStage::OwnerRecord, Some(route), Some(ordinal)))?;
        let events = store
            .events(&id, execution.version, 2)
            .map_err(|_| refuse_at(RefusalStage::TerminalEvents, Some(route), Some(ordinal)))?;
        if !terminal_events_match(route, program, &execution, &run, &events)
            || effect.intent.attempt != execution.attempt
            || effect.intent.capability.as_ref().map(CapabilityId::as_str)
                != Some("host.cics.execute")
            || effect.intent.audit_resource.is_none_or(|digest| {
                digest.format != AuditResourceDigestFormat::CanonicalHostResourceV1
            })
        {
            return Err(refuse_at(
                RefusalStage::TerminalEvents,
                Some(route),
                Some(ordinal),
            ));
        }
        let invocation_key =
            effect.intent.audit_invocation_key.clone().ok_or_else(|| {
                refuse_at(RefusalStage::InvocationKey, Some(route), Some(ordinal))
            })?;
        if owners
            .get(program)
            .is_some_and(|previous| previous != &execution)
            || invocation_keys
                .get(program)
                .is_some_and(|previous| previous != &invocation_key)
        {
            return Err(refuse_at(
                RefusalStage::OwnerAmbiguity,
                Some(route),
                Some(ordinal),
            ));
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
        return Err(refuse_at(RefusalStage::OwnerSet, Some(route), None));
    }
    let actor = owners
        .get(route.reading_program())
        .ok_or_else(|| refuse_at(RefusalStage::OwnerSet, Some(route), None))?;
    let audits = store
        .audit_records(&actor.execution_id, 0, MAX_ACTOR_AUDITS + 1)
        .map_err(|_| refuse_at(RefusalStage::AuditRead, Some(route), None))?;
    compare_audits(
        actor,
        &invocation_keys[route.reading_program()],
        terminal_sequences[route.reading_program()],
        &audits,
        route.reads(),
    )
    .map_err(|error| audit_refusal_for_route(error, route))?;
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
        return Err(refuse_at(RefusalStage::Trace, Some(route), None));
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
