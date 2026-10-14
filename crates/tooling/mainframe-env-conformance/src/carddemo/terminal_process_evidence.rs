//! Private current-row/actor/checkpoint comparisons; no dispatch or public schema.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsTraceEntry,
    describe_cics_replay_row,
};
use mainframe_env_execution_api::{AuditDecision, AuditResourceDigestFormat, LifecycleEventKind};
use mainframe_env_host_api::{
    HostResult, canonical_audit_resource_digest, canonical_result_digest,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ArtifactStore, ExecutionState, PlatformStore, ProviderStateRecord};

const REPLAY: &str = "cics-effect-replay-v1";
const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct Before {
    rows: BTreeMap<String, ProviderStateRecord>,
    trace: Vec<CicsTraceEntry>,
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Owner {
    pub(super) execution: String,
    pub(super) run: String,
    pub(super) artifact: String,
    pub(super) version: u64,
    pub(super) attempt: u32,
    pub(super) tick: u64,
    pub(super) retained_digest: String,
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn debug_hash(value: &impl std::fmt::Debug) -> Result<String, CorpusProblem> {
    // Retention only, under the same qualified test executable in both children.
    // This private complete typed value is never printed or written as raw data.
    let value = format!("{value:?}");
    require(value.len() <= MAX_BYTES)?;
    Ok(hash(value.as_bytes()))
}

fn row_map(
    store: &dyn PlatformStore,
    namespace: &str,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(namespace, MAX_ROWS + 1, MAX_BYTES)
        .map_err(|_| problem())?;
    require(rows.len() <= MAX_ROWS)?;
    let mut result = BTreeMap::new();
    for row in rows {
        require(row.namespace == namespace && row.version > 0)?;
        require(result.insert(row.key.clone(), row).is_none())?;
    }
    Ok(result)
}

pub(super) fn before(
    store: &dyn PlatformStore,
    server: &ProductServer,
    session: &str,
) -> Result<Before, CorpusProblem> {
    let trace = server.online_trace(session).map_err(|_| problem())?;
    require(trace.len() < 4096)?;
    Ok(Before {
        rows: row_map(store, REPLAY)?,
        trace,
    })
}

pub(super) fn owner_digest(
    store: &dyn PlatformStore,
    owner: &Owner,
) -> Result<String, CorpusProblem> {
    let id =
        ExecutionId::new(&owner.execution, InvocationLimits::default()).map_err(|_| problem())?;
    let execution = store
        .get_execution(&id)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    require(
        execution.execution_id == id
            && execution.selector.as_str() == "program:COCRDLIC"
            && execution.principal.as_str() == "WEBUSER"
            && execution.run_unit_id.as_str() == owner.run
            && execution.artifact.as_str() == owner.artifact
            && execution.state == ExecutionState::Completed
            && execution.attempt == owner.attempt
            && execution.version == owner.version
            && execution.terminal_tick == Some(owner.tick)
            && owner.tick > 0
            && owner.version > 0
            && owner.attempt > 0,
    )?;
    let events = store.events(&id, 0, MAX_ROWS + 1).map_err(|_| problem())?;
    let audits = store
        .audit_records(&id, 0, MAX_ROWS + 1)
        .map_err(|_| problem())?;
    require(
        !events.is_empty()
            && events.len() <= MAX_ROWS
            && !audits.is_empty()
            && audits.len() <= MAX_ROWS,
    )?;
    debug_hash(&(execution, events, audits))
}

pub(super) fn stage(
    store: &dyn PlatformStore,
    server: &ProductServer,
    session: &str,
    artifact: &ArtifactRef,
    before: &Before,
    terminal: &serde_json::Value,
    suspension: bool,
) -> Result<Owner, CorpusProblem> {
    let after = self::before(store, server, session)?;
    require(
        before
            .rows
            .iter()
            .all(|(key, row)| after.rows.get(key) == Some(row))
            && after.trace.starts_with(&before.trace),
    )?;
    let trace = &after.trace[before.trace.len()..];
    let expected = expected_operations(suspension);
    require(trace_matches(trace, suspension))?;
    let fresh = after
        .rows
        .iter()
        .filter(|(key, _)| !before.rows.contains_key(*key))
        .map(|(_, row)| row)
        .collect::<Vec<_>>();
    require(fresh.len() == expected.len())?;
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(problem)?,
        InvocationLimits::default(),
    )
    .map_err(|_| problem())?;
    let mut actor = None;
    let mut invocation_key = None;
    let mut receive = 0;
    for row in fresh {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| problem())?;
        let effect = store
            .effect(&key)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let described = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| problem())?;
        require(
            described.codec == CicsReplayCodecVersion::RetentionV3
                && described.retention == CicsReplayRetentionState::Terminal
                && described.owner_run_unit.as_deref() == Some(run.as_str())
                && effect.intent.attempt > 0
                && effect.intent.capability.as_ref().map(CapabilityId::as_str)
                    == Some("host.cics.execute"),
        )?;
        let id = ExecutionId::new(
            described.owner_execution.as_deref().ok_or_else(problem)?,
            InvocationLimits::default(),
        )
        .map_err(|_| problem())?;
        let execution = store
            .get_execution(&id)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        require(
            execution.execution_id == id
                && execution.run_unit_id == run
                && execution.principal.as_str() == "WEBUSER"
                && execution.selector.as_str() == "program:COCRDLIC"
                && execution.artifact == *artifact
                && execution.state == ExecutionState::Completed
                && execution.attempt == effect.intent.attempt
                && execution.version > 1
                && execution.terminal_tick.is_some_and(|tick| tick > 0),
        )?;
        if let Some(previous) = &actor {
            require(previous == &execution)?;
        }
        actor = Some(execution);
        let audit_key = effect
            .intent
            .audit_invocation_key
            .clone()
            .ok_or_else(problem)?;
        if let Some(previous) = &invocation_key {
            require(previous == &audit_key)?;
        }
        invocation_key = Some(audit_key);
        if described.request_digest == oracles::receive_digest(effect.sequence, key)? {
            receive += 1;
            let expected = canonical_audit_resource_digest(&oracles::receive(
                effect.sequence,
                effect.key.clone(),
            )?);
            require(effect.intent.audit_resource == Some(expected))?;
        }
    }
    require(receive == 1)?;
    let actor = actor.ok_or_else(problem)?;
    let id = &actor.execution_id;
    let first = if suspension {
        actor.version - 1
    } else {
        actor.version
    };
    let events = store.events(id, first, 3).map_err(|_| problem())?;
    require(events.iter().all(|event| {
        event.validate()
            && event.execution_id == *id
            && event.run_unit_id == run
            && event.attempt == actor.attempt
            && event.tick > 0
    }))?;
    if suspension {
        require(matches!(events.as_slice(), [prior, final_event]
            if prior.sequence == actor.version - 1 && final_event.sequence == actor.version
            && matches!(prior.kind, LifecycleEventKind::Suspended)
            && matches!(final_event.kind, LifecycleEventKind::HandoffCompleted)
            && prior.tick <= final_event.tick && Some(final_event.tick) == actor.terminal_tick))?;
    } else {
        require(
            matches!(events.as_slice(), [final_event] if final_event.sequence == actor.version
            && matches!(final_event.kind, LifecycleEventKind::Completed { return_code: 0 })
            && Some(final_event.tick) == actor.terminal_tick),
        )?;
    }
    let audits = store
        .audit_records(id, 0, MAX_ROWS + 1)
        .map_err(|_| problem())?;
    require(
        !audits.is_empty()
            && audits.len() <= MAX_ROWS
            && audits.iter().all(|audit| {
                audit.execution_id == *id
                    && audit.run_unit_id == run
                    && audit.principal == actor.principal
                    && audit.attempt == actor.attempt
                    && Some(&audit.invocation_key) == invocation_key.as_ref()
                    && audit.effect_sequence > 0
                    && audit.decision == AuditDecision::Success
            }),
    )?;
    let outer = audits
        .iter()
        .filter(|audit| audit.capability.as_str() == "host.cics.execute")
        .collect::<Vec<_>>();
    require(outer.len() == expected.len())?;
    for audit in outer {
        let key = IdempotencyKey::new(
            format!(
                "{}:{}",
                audit.invocation_key.as_str(),
                audit.effect_sequence
            ),
            InvocationLimits::default(),
        )
        .map_err(|_| problem())?;
        let effect = store
            .effect(&key)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        require(
            effect.intent.audit_resource == Some(audit.resource)
                && audit.resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1,
        )?;
    }
    require(store.get_checkpoint(id).map_err(|_| problem())?.is_none())?;
    let mut owner = Owner {
        execution: id.as_str().into(),
        run: run.as_str().into(),
        artifact: artifact.as_str().into(),
        version: actor.version,
        attempt: actor.attempt,
        tick: actor.terminal_tick.ok_or_else(problem)?,
        retained_digest: String::new(),
    };
    owner.retained_digest = owner_digest(store, &owner)?;
    Ok(owner)
}

pub(super) fn expected_operations(suspension: bool) -> Vec<CicsOperation> {
    if suspension {
        vec![CicsOperation::ReceiveMap]
    } else {
        let mut expected = vec![CicsOperation::ReceiveMap, CicsOperation::StartBrowse];
        expected.extend([CicsOperation::ReadNext; 8]);
        expected.extend([
            CicsOperation::EndBrowse,
            CicsOperation::SendMap,
            CicsOperation::Return,
        ]);
        expected
    }
}

pub(super) fn trace_matches(trace: &[CicsTraceEntry], suspension: bool) -> bool {
    let expected = expected_operations(suspension);
    trace.len() == expected.len()
        && trace.iter().zip(&expected).all(|(entry, operation)| {
            entry.operation == *operation
                && entry.outcome == "NORMAL"
                && entry.response == 0
                && entry.response2 == 0
                && match operation {
                    // Seventeen pinned FSET slots: 122 values + 118 names + 136 framing.
                    // Explicit empty-fields PF8 is supplied input; absent-input suspension is zero.
                    CicsOperation::ReceiveMap => {
                        entry.payload_bytes == if suspension { 0 } else { 376 }
                    }
                    CicsOperation::ReadNext => entry.payload_bytes == 150,
                    CicsOperation::Return => entry.payload_bytes == 2000,
                    CicsOperation::SendMap => {
                        entry.payload_bytes > 0 && entry.payload_bytes <= 65536
                    }
                    _ => entry.payload_bytes == 0,
                }
        })
}

pub(super) fn envelope<'a>(
    bytes: &'a [u8],
    magic: &[u8],
    limits: &[usize],
) -> Result<Vec<&'a [u8]>, CorpusProblem> {
    require(bytes.starts_with(magic))?;
    let mut at = magic.len();
    let mut fields = Vec::new();
    for maximum in limits {
        let length = bytes
            .get(at..at + 4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .ok_or_else(problem)?;
        at += 4;
        let length = usize::try_from(u32::from_be_bytes(length)).map_err(|_| problem())?;
        require(length <= *maximum)?;
        let end = at.checked_add(length).ok_or_else(problem)?;
        fields.push(bytes.get(at..end).ok_or_else(problem)?);
        at = end;
    }
    require(at == bytes.len())?;
    Ok(fields)
}

pub(super) fn continuation(
    store: &dyn PlatformStore,
    session: &str,
    page: usize,
) -> Result<(), CorpusProblem> {
    let row = store
        .get_provider_state("cics-continuation", session)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    require(row.namespace == "cics-continuation" && row.key == session && row.version > 0)?;
    continuation_payload(&row.payload, page)
}

pub(super) fn continuation_payload(bytes: &[u8], page: usize) -> Result<(), CorpusProblem> {
    let fields = envelope(bytes, b"MECC1", &[16, 2000, 128, 256])?;
    require(
        fields[0] == b"CCLI"
            && fields[2].is_empty()
            && IdempotencyKey::new(
                std::str::from_utf8(fields[3]).map_err(|_| problem())?,
                InvocationLimits::default(),
            )
            .is_ok(),
    )?;
    oracles::commarea(fields[1], page)
}

pub(super) fn machine(
    store: &dyn PlatformStore,
    session: &str,
    artifact: &ArtifactRef,
) -> Result<(), CorpusProblem> {
    let row = store
        .get_provider_state("online-machine-continuation", session)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    require(
        row.namespace == "online-machine-continuation" && row.key == session && row.version > 0,
    )?;
    machine_payload(&row.payload, artifact)?;
    require(
        store
            .get_provider_state("online-exchange-v1", session)
            .map_err(|_| problem())?
            .is_none(),
    )
}

pub(super) fn machine_payload(bytes: &[u8], artifact: &ArtifactRef) -> Result<(), CorpusProblem> {
    let fields = envelope(bytes, b"MEOM4", &[128, 256, 4096, 1, 128, MAX_BYTES, 0])?;
    require(
        fields[0] == b"COCRDLIC"
            && fields[1] == artifact.as_str().as_bytes()
            && fields[3].len() == 1
            && fields[4] == b"mainframe-env.reference-machine-checkpoint@12"
            && !fields[5].is_empty()
            && fields[6].is_empty(),
    )?;
    let generations: BTreeMap<String, String> =
        serde_json::from_slice(fields[2]).map_err(|_| problem())?;
    require(serde_json::to_vec(&generations).map_err(|_| problem())? == fields[2])?;
    let names = [
        "host.security.authorize",
        "host.cics.execute",
        "host.dataset.read",
        "host.dataset.write",
        "host.db2.read",
        "host.db2.write",
        "host.ims.read",
        "host.ims.write",
        "host.mq.read",
        "host.mq.write",
        "host.program.invoke",
        "host.clock",
    ];
    require(
        generations.len() == names.len()
            && names
                .iter()
                .all(|name| generations.get(*name).is_some_and(|value| value == "1")),
    )?;
    Ok(())
}

pub(super) fn durable_digest(
    store: &dyn PlatformStore,
    session: &str,
) -> Result<String, CorpusProblem> {
    let mut components = Vec::new();
    for namespace in [
        "cics-session",
        "cics-continuation",
        "online-machine-continuation",
    ] {
        let row = store
            .get_provider_state(namespace, session)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        require(
            row.key == session
                && row.namespace == namespace
                && row.version > 0
                && row.payload.len() <= MAX_BYTES,
        )?;
        components.push(debug_hash(&row)?);
    }
    for row in row_map(store, REPLAY)?.values() {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| problem())?;
        let effect = store
            .effect(&key)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| problem())?;
        require(
            descriptor.codec
                == super::read_request_observations::expected_current_plain_read_codec(&effect)?
                && descriptor.retention == CicsReplayRetentionState::Terminal,
        )?;
        components.push(debug_hash(&(row, effect))?);
    }
    debug_hash(&components)
}

pub(super) fn security_retention(store: &dyn PlatformStore) -> Result<String, CorpusProblem> {
    // Exact same persisted credentials, permits and returned bearer authority
    // before cold HTTP. Auth requests legitimately advance idle/audit state;
    // this is a cross-process retention comparison, not an immutability claim
    // about later admitted security operations. Resolver fixture is empty in
    // both processes; source bootstrap/authentication scoped secrets do not live
    // across the process boundary or require a second bootstrap.
    let database = row_map(store, "racf-database-v2")?;
    let sessions = row_map(store, "auth-session-v2")?;
    let index = row_map(store, "auth-session-index-v2")?;
    require(
        database.len() == 1
            && database.contains_key("authority")
            && sessions.len() == 1
            && index.len() == 1
            && index.contains_key("global"),
    )?;
    debug_hash(&(database, sessions, index))
}

pub(super) fn artifacts(
    store: &dyn PlatformStore,
    root: &Path,
) -> Result<(String, ArtifactRef), CorpusProblem> {
    let programs = row_map(store, "online-program")?;
    let transactions = row_map(store, "online-transaction")?;
    require(
        programs.len() == 18
            && transactions.len() == 17
            && transactions
                .get("CCLI")
                .is_some_and(|row| row.version == 1 && row.payload == b"COCRDLIC"),
    )?;
    let artifact_store = LocalArtifactStore::open(root, 64 * 1024 * 1024).map_err(|_| problem())?;
    let mut parts = vec![debug_hash(&programs)?, debug_hash(&transactions)?];
    let mut selected = None;
    for (name, row) in programs {
        require(row.version == 1)?;
        let reference = ArtifactRef::new(
            std::str::from_utf8(&row.payload).map_err(|_| problem())?,
            InvocationLimits::default(),
        )
        .map_err(|_| problem())?;
        let record = artifact_store
            .get_artifact(&reference)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let digest: [u8; 32] = Sha256::digest(&record.payload).into();
        require(
            record.artifact == reference
                && reference.as_str() == format!("sha256:{}", hash(&record.payload))
                && record.payload_digest == digest
                && record.payload.len() <= 64 * 1024 * 1024
                && record.executable.as_ref().is_some_and(|metadata| {
                    metadata.validate() && metadata.validates_payload(&digest)
                }),
        )?;
        // Complete content/metadata retention under the source-verified initial
        // publisher proof; no raw executable copied to the outward handoff.
        parts.push(debug_hash(&(
            name.clone(),
            record.artifact,
            record.media_type,
            record.payload_digest,
            record.executable,
        ))?);
        if name == "COCRDLIC" {
            selected = Some(reference);
        }
    }
    Ok((debug_hash(&parts)?, selected.ok_or_else(problem)?))
}

pub(super) fn datasets(server: &ProductServer) -> Result<String, CorpusProblem> {
    card_observations::require_seed(&card_observations::capture(server)?)?;
    let mut hashes = Vec::new();
    for name in [
        "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
        "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
    ] {
        let dataset = DatasetName::new(name, 128).map_err(|_| problem())?;
        let requests = [
            DatasetRequest::Attributes {
                dataset: dataset.clone(),
            },
            DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: 51,
                control: Default::default(),
            },
            DatasetRequest::ListCatalog {
                pattern: name.into(),
                start: None,
                max_items: 2,
            },
        ];
        for request in requests {
            let result = server
                .dataset_service()
                .invoke(request)
                .map_err(|_| problem())?;
            hashes.push(
                canonical_result_digest(&Ok(HostResult::Dataset(result))).map_err(|_| problem())?,
            );
        }
    }
    debug_hash(&hashes)
}
