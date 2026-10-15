//! Exact existing COTRN02C -> CSUTLDTC -> CEEDAYS CALL evidence.
//! Private local compatibility proof; no IBM feedback or checkpoint credit.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, describe_cics_replay_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, AuditResourceDigestFormat, LifecycleEventKind,
};
use mainframe_env_host_api::{
    CicsConditionPolicy, CicsOperation, CicsRequest, HostRequest, HostResult, ProgramName,
    ProgramRequest, RuntimeServiceKind, RuntimeServiceName, RuntimeServiceSelector,
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactStore, EffectDigestFormat, EffectRecord, EffectState, ExecutionRecord, ExecutionState,
    PlatformStore, ProviderStateRecord,
};

#[cfg(test)]
#[path = "date_call_observations_tests.rs"]
mod tests;

const CALLS: &str = "cobol-call-replay@1";
const CICS: &str = "cics-effect-replay-v1";
const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_AUDITS: usize = 512;
const PROGRAMS: &[&str] = &["COTRN02C", "CSUTLDTC"];
const MASK: &[u8; 10] = b"YYYY-MM-DD";
const NAMES: &[&str] = &["CSUTLDTC-DATE", "CSUTLDTC-DATE-FORMAT", "CSUTLDTC-RESULT"];
const CEE_NAMES: &[&str] = &[
    "WS-DATE-TO-TEST",
    "WS-DATE-FORMAT",
    "OUTPUT-LILLIAN",
    "FEEDBACK-CODE",
];

#[track_caller]
fn problem() -> CorpusProblem {
    let location = std::panic::Location::caller();
    CorpusProblem::new(
        "carddemo.transaction.date_call_binding",
        format!(
            "selected date CALL inputs, linkage result or completed owner differ; observer={}:{}",
            location.file(),
            location.line(),
        ),
    )
}
#[track_caller]
fn refuse(value: bool) -> Result<(), CorpusProblem> {
    if value { Err(problem()) } else { Ok(()) }
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}
fn installed_digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.installed-call@1\0");
    for part in parts {
        field(&mut hash, part);
    }
    format!("{:x}", hash.finalize())
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: String,
    bytes: Vec<u8>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    replay_key: String,
    fingerprint: String,
    child_execution: String,
    owner_execution: String,
    owner_run_unit: String,
    owner_principal: String,
    protocol_key: String,
    run_state_key: String,
    metadata_digest: String,
    completion_tick: Option<u64>,
    reply: Option<Reply>,
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn receipt(row: &ProviderStateRecord) -> Result<Receipt, CorpusProblem> {
    refuse(row.namespace != CALLS || row.version != 2 || row.payload.len() > MAX_BYTES)?;
    let value: Receipt = serde_json::from_slice(&row.payload).map_err(|_| problem())?;
    refuse(
        value.schema_version != 2
            || value.replay_key != row.key
            || !hex(&row.key)
            || !hex(&value.fingerprint)
            || value.completion_tick.is_none_or(|tick| tick == 0)
            || value.child_execution != format!("online-call-execution-{}", row.key)
            || value.protocol_key
                != installed_digest(&[b"run-protocol", value.owner_run_unit.as_bytes()])
            || value.run_state_key
                != installed_digest(&[
                    b"instance-owner",
                    value.owner_run_unit.as_bytes(),
                    value.owner_principal.as_bytes(),
                ]),
    )?;
    let reply = value.reply.as_ref().ok_or_else(problem)?;
    refuse(reply.schema != "mainframe-env.cobol.call-result@1" || reply.bytes.len() > 4096)?;
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cobol-call-receipt-metadata@2\0");
    for bytes in [
        value.replay_key.as_bytes(),
        value.fingerprint.as_bytes(),
        value.child_execution.as_bytes(),
        value.owner_execution.as_bytes(),
        value.owner_run_unit.as_bytes(),
        value.owner_principal.as_bytes(),
        value.protocol_key.as_bytes(),
        value.run_state_key.as_bytes(),
    ] {
        field(&mut hash, bytes);
    }
    hash.update(value.completion_tick.unwrap_or(0).to_be_bytes());
    hash.update([1]);
    field(&mut hash, reply.schema.as_bytes());
    field(&mut hash, &reply.bytes);
    refuse(value.metadata_digest != format!("{:x}", hash.finalize()))?;
    Ok(value)
}
fn rows(
    store: &dyn PlatformStore,
    namespace: &str,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(namespace, MAX_ROWS + 1, MAX_BYTES)
        .map_err(|_| problem())?;
    refuse(rows.len() > MAX_ROWS)?;
    let count = rows.len();
    let result = rows
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    refuse(count != result.len())?;
    Ok(result)
}
fn fresh<'a>(
    before: &BTreeMap<String, ProviderStateRecord>,
    after: &'a BTreeMap<String, ProviderStateRecord>,
) -> Result<Vec<&'a ProviderStateRecord>, CorpusProblem> {
    refuse(before.iter().any(|(key, row)| after.get(key) != Some(row)))?;
    Ok(after
        .iter()
        .filter(|(key, _)| !before.contains_key(*key))
        .map(|(_, row)| row)
        .collect())
}

pub(super) fn check_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    source_artifact_binding::verify_pinned_sources(corpus)
}
pub(super) struct BeforeStage {
    calls: BTreeMap<String, ProviderStateRecord>,
    cics: BTreeMap<String, ProviderStateRecord>,
    installed: BTreeMap<String, ArtifactRef>,
}
impl BeforeStage {
    pub(super) fn capture(
        store: &dyn PlatformStore,
        artifact_root: &Path,
        definitions: &[OnlineProgramDefinition],
    ) -> Result<Self, CorpusProblem> {
        // This existing fixture owns the root exclusively. LocalArtifactStore::open
        // uses the same 64 MiB object limit and may sync its owned directory.
        let artifacts =
            LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024).map_err(|_| problem())?;
        let mut installed = BTreeMap::new();
        for name in PROGRAMS {
            let selected = definitions
                .iter()
                .filter(|program| program.name == *name)
                .collect::<Vec<_>>();
            refuse(selected.len() != 1)?;
            let definition = selected[0];
            let expected = source_artifact_binding::publisher_record(definition)?;
            let actual = artifacts
                .get_artifact(&definition.artifact)
                .map_err(|_| problem())?
                .ok_or_else(problem)?;
            let reference = store
                .get_provider_state("online-program", name)
                .map_err(|_| problem())?
                .ok_or_else(problem)?;
            refuse(
                actual != expected
                    || reference.namespace != "online-program"
                    || reference.key != *name
                    || reference.version != 1
                    || reference.payload != definition.artifact.as_str().as_bytes(),
            )?;
            installed.insert((*name).into(), definition.artifact.clone());
        }
        Ok(Self {
            calls: rows(store, CALLS)?,
            cics: rows(store, CICS)?,
            installed,
        })
    }
}

#[derive(Clone, Copy)]
struct Date {
    text: &'static [u8; 10],
    lilian: Option<i32>,
}
// Independent fixed civil-date distances from 1582-10-14. Invalid dates are
// explicit offered controls; no product parser/result is an expected-value source.
const FEB2026: Date = Date {
    text: b"2026-02-28",
    lilian: Some(161940),
};
const MAR2026: Date = Date {
    text: b"2026-03-01",
    lilian: Some(161941),
};
const FEB2024: Date = Date {
    text: b"2024-02-29",
    lilian: Some(161210),
};
const FEB2000: Date = Date {
    text: b"2000-02-29",
    lilian: Some(152444),
};
const MONTH13: Date = Date {
    text: b"2026-13-01",
    lilian: None,
};
const NONLEAP: Date = Date {
    text: b"2026-02-29",
    lilian: None,
};
const CENTURY: Date = Date {
    text: b"1900-02-29",
    lilian: None,
};
fn cases(origin: &str, processing: &str) -> Result<Vec<Date>, CorpusProblem> {
    Ok(match (origin, processing) {
        ("2026/02/28", "2026-02-28") | ("2026-02-28", "2026/02/28") => vec![],
        ("2026-13-01", "2026-02-28") => vec![MONTH13],
        ("2026-02-29", "2026-02-28") => vec![NONLEAP],
        ("1900-02-29", "2026-02-28") => vec![CENTURY],
        ("2026-02-28", "2026-13-01") => vec![FEB2026, MONTH13],
        ("2026-02-28", "2026-02-29") => vec![FEB2026, NONLEAP],
        ("2026-02-28", "1900-02-29") => vec![FEB2026, CENTURY],
        ("2026-02-28", "2026-03-01") => vec![FEB2026, MAR2026],
        ("2024-02-29", "2024-02-29") => vec![FEB2024, FEB2024],
        ("2000-02-29", "2000-02-29") => vec![FEB2000, FEB2000],
        _ => return Err(problem()),
    })
}
fn message(date: Date) -> Vec<u8> {
    let mut bytes = if date.lilian.is_some() {
        b"0000Mesg Code: 0000 Date is valid   TstDate: ".to_vec()
    } else {
        b"0003Mesg Code: 0001 Date is invalid TstDate: ".to_vec()
    };
    // CSUTLDTC moves the complete length-prefixed group to PIC X(10), not
    // VSTRING-TEXT. Preserve that exact source operation and its two prefix bytes.
    bytes.extend_from_slice(&[0, 10]);
    bytes.extend_from_slice(&date.text[..8]);
    bytes.extend_from_slice(b" Mask used:YYYY-MM-DD    ");
    bytes
}
fn argument_payload(names: &[&str], values: &[Vec<u8>]) -> Result<BoundedPayload, CorpusProblem> {
    refuse(names.len() != values.len())?;
    let mut bytes = (values.len() as u32).to_be_bytes().to_vec();
    for (name, value) in names.iter().zip(values) {
        bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| problem())
}
fn result_payload(values: &[Vec<u8>]) -> Result<BoundedPayload, CorpusProblem> {
    let mut bytes = (values.len() as u32).to_be_bytes().to_vec();
    for value in values {
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call-result@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| problem())
}
fn utility_request(date: Date) -> Result<HostRequest, CorpusProblem> {
    Ok(HostRequest::Program(ProgramRequest::Call {
        program: ProgramName::new("CSUTLDTC", 128).map_err(|_| problem())?,
        payload: argument_payload(NAMES, &[date.text.to_vec(), MASK.to_vec(), vec![b' '; 80]])?,
        service: None,
    }))
}
fn utility_result(date: Date) -> Result<BoundedPayload, CorpusProblem> {
    let message = message(date);
    refuse(message.len() != 80)?;
    result_payload(&[date.text.to_vec(), MASK.to_vec(), message])
}
fn vstring(text: &[u8; 10]) -> Vec<u8> {
    let mut value = vec![b' '; 258];
    value[..2].copy_from_slice(&10i16.to_be_bytes());
    value[2..12].copy_from_slice(text);
    value
}
fn cee_request(date: Date, occurrence: usize) -> Result<HostRequest, CorpusProblem> {
    let mut feedback = vec![0; 12];
    // First entry has binary numeric zeros and PIC X CASE-SEV-CTL/FACILITY-ID
    // spaces. Every second call follows a successful first CEEDAYS result.
    if occurrence == 0 {
        feedback[4..8].fill(b' ');
    }
    Ok(HostRequest::Program(ProgramRequest::Call {
        program: ProgramName::new("CEEDAYS", 128).map_err(|_| problem())?,
        payload: argument_payload(
            CEE_NAMES,
            &[vstring(date.text), vstring(MASK), vec![0; 4], feedback],
        )?,
        service: Some(RuntimeServiceSelector {
            kind: RuntimeServiceKind::LanguageEnvironment,
            name: RuntimeServiceName::new("CEEDAYS", 128).map_err(|_| problem())?,
            abi_version: 1,
        }),
    }))
}
fn cee_result(date: Date) -> Result<BoundedPayload, CorpusProblem> {
    let mut feedback = vec![0; 12];
    if date.lilian.is_none() {
        feedback[1] = 3;
        feedback[3] = 1;
    }
    result_payload(&[
        vstring(date.text),
        vstring(MASK),
        date.lilian.unwrap_or(0).to_be_bytes().to_vec(),
        feedback,
    ])
}
fn completed(
    store: &dyn PlatformStore,
    id: &str,
    program: &str,
    run: &RunUnitId,
    artifact: &ArtifactRef,
    return_code: i32,
) -> Result<ExecutionRecord, CorpusProblem> {
    let id = ExecutionId::new(id, InvocationLimits::default()).map_err(|_| problem())?;
    let actor = store
        .get_execution(&id)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    refuse(
        actor.execution_id != id
            || actor.run_unit_id != *run
            || actor.selector.as_str() != format!("program:{program}")
            || actor.artifact != *artifact
            || actor.principal.as_str() != "WEBUSER"
            || actor.attempt == 0
            || actor.state != ExecutionState::Completed
            || actor.owner_lease.is_some()
            || actor.lease_expiry_tick.is_some()
            || actor.version == 0
            || actor.terminal_tick.is_none_or(|tick| tick == 0),
    )?;
    let events = store.events(&id, actor.version, 2).map_err(|_| problem())?;
    refuse(
        events.len() != 1
            || events[0].execution_id != id
            || events[0].run_unit_id != *run
            || events[0].sequence != actor.version
            || events[0].attempt != actor.attempt
            || Some(events[0].tick) != actor.terminal_tick
            || events[0].kind != (LifecycleEventKind::Completed { return_code }),
    )?;
    Ok(actor)
}
fn program_audits(
    store: &dyn PlatformStore,
    actor: &ExecutionRecord,
) -> Result<Vec<AuditRecord>, CorpusProblem> {
    let audits = store
        .audit_records(&actor.execution_id, 0, MAX_AUDITS + 1)
        .map_err(|_| problem())?;
    refuse(
        audits.is_empty()
            || audits.len() > MAX_AUDITS
            || audits.iter().any(|audit| {
                audit.execution_id != actor.execution_id
                    || audit.run_unit_id != actor.run_unit_id
                    || audit.principal != actor.principal
                    || audit.attempt != actor.attempt
                    || audit.effect_sequence == 0
                    || audit.decision != AuditDecision::Success
            }),
    )?;
    let mut selected = audits
        .into_iter()
        .filter(|audit| audit.capability.as_str() == "host.program.invoke")
        .collect::<Vec<_>>();
    selected.sort_by_key(|audit| audit.effect_sequence);
    refuse(
        selected
            .windows(2)
            .any(|pair| pair[0].effect_sequence >= pair[1].effect_sequence),
    )?;
    Ok(selected)
}
fn effect_matches(
    effect: &EffectRecord,
    actor: &ExecutionRecord,
    audit: &AuditRecord,
    request: &HostRequest,
    result: &BoundedPayload,
    key: &IdempotencyKey,
) -> Result<(), CorpusProblem> {
    let expected_resource = canonical_audit_resource_digest(request);
    let expected_result =
        canonical_result_digest(&Ok(HostResult::Program(result.clone()))).map_err(|_| problem())?;
    refuse(
        effect.execution_id != actor.execution_id
            || effect.run_unit_id != actor.run_unit_id
            || effect.sequence != audit.effect_sequence
            || effect.key != *key
            || effect.digest_format != EffectDigestFormat::CanonicalHostV1
            || effect.request_digest != canonical_request_digest(request).map_err(|_| problem())?
            || effect.result_digest != Some(expected_result)
            || effect.state != EffectState::Completed
            || effect
                .resolved_tick
                .is_none_or(|tick| tick == 0 || Some(tick) > actor.terminal_tick)
            || effect.intent.owner != actor.execution_id
            || effect.intent.attempt != actor.attempt
            || effect.intent.capability.as_ref().map(CapabilityId::as_str)
                != Some("host.program.invoke")
            || effect.intent.audit_resource != Some(expected_resource)
            || audit.resource != expected_resource
            || effect.intent.audit_invocation_key.as_ref() != Some(&audit.invocation_key),
    )
}
fn effect(
    store: &dyn PlatformStore,
    actor: &ExecutionRecord,
    audit: &AuditRecord,
    request: &HostRequest,
    result: &BoundedPayload,
) -> Result<IdempotencyKey, CorpusProblem> {
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
    effect_matches(&effect, actor, audit, request, result, &key)?;
    Ok(key)
}

// ProductServer directly drives the installed terminal root. It does not
// invoke that root through the CALL router. Actual CICS replay/core ownership
// therefore discovers the caller, including stages that make zero date CALLs.
fn cics_effect_matches(effect: &EffectRecord, actor: &ExecutionRecord) -> bool {
    effect.execution_id == actor.execution_id
        && effect.run_unit_id == actor.run_unit_id
        && effect.sequence > 0
        && effect.state == EffectState::Completed
        && effect.digest_format == EffectDigestFormat::CanonicalHostV1
        && effect.result_digest.is_some()
        && effect
            .resolved_tick
            .is_some_and(|tick| tick > 0 && Some(tick) <= actor.terminal_tick)
        && effect.intent.owner == actor.execution_id
        && effect.intent.attempt == actor.attempt
        && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some("host.cics.execute")
        && effect.intent.audit_resource.is_some_and(|digest| {
            digest.format == AuditResourceDigestFormat::CanonicalHostResourceV1
        })
        && effect.intent.audit_invocation_key.is_some()
}

fn date_account_read_request() -> Result<HostRequest, CorpusProblem> {
    // Pinned COTRN02C READ-CXACAIX-FILE, after ACTIDIN 00000000050 in fields().
    // These operands precede date validation even when no date CALL is made.
    let operands: [(&str, &str, &[u8]); 7] = [
        ("DATASET", "mainframe-env.cics.storage-value@1", b"CXACAIX "),
        (
            "RIDFLD",
            "mainframe-env.cics.storage-value@1",
            b"00000000050",
        ),
        ("KEYLENGTH", "mainframe-env.cics.decimal@1", b"11"),
        ("LENGTH", "mainframe-env.cics.decimal@1", b"50"),
        ("INTO", "mainframe-env.cics.argument@1", b"CARD-XREF-RECORD"),
        (
            "RESP",
            "mainframe-env.cics.argument@1",
            b"WS-VARIABLES.WS-RESP-CD",
        ),
        (
            "RESP2",
            "mainframe-env.cics.argument@1",
            b"WS-VARIABLES.WS-REAS-CD",
        ),
    ];
    let mut arguments = BTreeMap::new();
    for (name, schema, bytes) in operands {
        arguments.insert(
            name.into(),
            BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default())
                .map_err(|_| problem())?,
        );
    }
    Ok(HostRequest::Cics(CicsRequest {
        operation: CicsOperation::Read,
        arguments,
        condition_policy: CicsConditionPolicy::Respond {
            response_field: "WS-VARIABLES.WS-RESP-CD".into(),
            response2_field: Some("WS-VARIABLES.WS-REAS-CD".into()),
        },
        mutation: None,
    }))
}

fn date_cics_codec_matches(
    effect: &EffectRecord,
    codec: CicsReplayCodecVersion,
) -> Result<(), CorpusProblem> {
    let request = date_account_read_request()?;
    let expected = if effect.request_digest
        == canonical_request_digest(&request).map_err(|_| problem())?
    {
        let HostRequest::Cics(read) = &request else {
            return Err(problem());
        };
        refuse(
            !read.is_completed_plain_read_replay()
                || effect.intent.audit_resource != Some(canonical_audit_resource_digest(&request)),
        )?;
        CicsReplayCodecVersion::PlainReadV4
    } else {
        CicsReplayCodecVersion::RetentionV3
    };
    refuse(codec != expected)
}

fn terminal_caller(
    store: &dyn PlatformStore,
    before: &BeforeStage,
    run: &RunUnitId,
) -> Result<(ExecutionRecord, IdempotencyKey, u64), CorpusProblem> {
    let after = rows(store, CICS)?;
    let changed = fresh(&before.cics, &after)?;
    refuse(changed.is_empty())?;
    let mut owner = None;
    let mut invocation = None;
    let mut terminal_sequence = 0;
    for row in changed {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| problem())?;
        let effect = store
            .effect(&key)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| problem())?;
        date_cics_codec_matches(&effect, descriptor.codec)?;
        refuse(
            descriptor.retention != CicsReplayRetentionState::Terminal
                || descriptor.owner_run_unit.as_deref() != Some(run.as_str()),
        )
        .map_err(|mut error| {
            error.detail.push_str(&format!(
                "; codec={:?}; retention={:?}; run_unit_matches={}",
                descriptor.codec,
                descriptor.retention,
                descriptor.owner_run_unit.as_deref() == Some(run.as_str()),
            ));
            error
        })?;
        let actor = completed(
            store,
            descriptor.owner_execution.as_deref().ok_or_else(problem)?,
            "COTRN02C",
            run,
            &before.installed["COTRN02C"],
            0,
        )?;
        refuse(!cics_effect_matches(&effect, &actor))?;
        let key = effect
            .intent
            .audit_invocation_key
            .clone()
            .ok_or_else(problem)?;
        refuse(
            owner.as_ref().is_some_and(|previous| previous != &actor)
                || invocation.as_ref().is_some_and(|previous| previous != &key),
        )?;
        terminal_sequence = terminal_sequence.max(effect.sequence);
        owner = Some(actor);
        invocation = Some(key);
    }
    Ok((
        owner.ok_or_else(problem)?,
        invocation.ok_or_else(problem)?,
        terminal_sequence,
    ))
}

pub(super) fn compare_stage(
    store: &dyn PlatformStore,
    before: &BeforeStage,
    terminal: &serde_json::Value,
    origin: &str,
    processing: &str,
) -> Result<(), CorpusProblem> {
    refuse(
        terminal["mapset"] != "COTRN02"
            || terminal["map"] != "COTRN2A"
            || terminal["suspended"] != false,
    )?;
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(problem)?,
        InvocationLimits::default(),
    )
    .map_err(|_| problem())?;
    let after = rows(store, CALLS)?;
    let expected = cases(origin, processing)?;
    let (caller, invocation, terminal_sequence) = terminal_caller(store, before, &run)?;
    let mut children = BTreeMap::new();
    for row in fresh(&before.calls, &after)? {
        let receipt = receipt(row)?;
        refuse(
            receipt.owner_execution != caller.execution_id.as_str()
                || receipt.owner_run_unit != run.as_str()
                || receipt.owner_principal != "WEBUSER",
        )?;
        // Only actual nested CSUTLDTC calls create these fresh installed-CALL
        // receipts. A fabricated outer caller receipt remains an extra row.
        refuse(children.insert(row.key.clone(), receipt).is_some())?;
    }
    let calls = program_audits(store, &caller)?;
    refuse(
        calls.len() != expected.len()
            || children.len() != expected.len()
            || calls.iter().any(|audit| {
                audit.invocation_key != invocation || audit.effect_sequence >= terminal_sequence
            }),
    )?;
    let mut child_ids = BTreeSet::new();
    for (occurrence, (audit, date)) in calls.iter().zip(expected).enumerate() {
        let result = utility_result(date)?;
        let key = effect(store, &caller, audit, &utility_request(date)?, &result)?;
        let replay_key = installed_digest(&[
            b"identity",
            run.as_str().as_bytes(),
            caller.execution_id.as_str().as_bytes(),
            key.as_str().as_bytes(),
        ]);
        let receipt = children.remove(&replay_key).ok_or_else(problem)?;
        let reply = receipt.reply.as_ref().ok_or_else(problem)?;
        refuse(
            receipt.owner_execution != caller.execution_id.as_str()
                || reply.schema != result.schema()
                || reply.bytes != result.bytes()
                || !child_ids.insert(receipt.child_execution.clone()),
        )?;
        let return_code = if date.lilian.is_some() { 0 } else { 3 };
        let child = completed(
            store,
            &receipt.child_execution,
            "CSUTLDTC",
            &run,
            &before.installed["CSUTLDTC"],
            return_code,
        )?;
        refuse(
            child.attempt != caller.attempt
                || receipt.completion_tick.is_none_or(|tick| {
                    Some(tick) < child.terminal_tick || Some(tick) > caller.terminal_tick
                }),
        )?;
        let nested = program_audits(store, &child)?;
        refuse(nested.len() != 1)?;
        effect(
            store,
            &child,
            &nested[0],
            &cee_request(date, occurrence)?,
            &cee_result(date)?,
        )?;
    }
    // No CD-007 closure token: file/status semantics and CBSTM03A -> CBSTM03B
    // still require their distinct actual caller and complete output proofs.
    refuse(!children.is_empty())
}
