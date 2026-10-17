//! Private cfg(test) installed CODATE01, with an explicit per-server host clock.
//! No production clock override, timed WAIT, automatic trigger or journey token.
use super::*;
use crate::{StoreProfile, TlsConfig};
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsUowDependencyState,
    CicsUowState, describe_cics_replay_row, describe_cics_uow_row,
};
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{AuditDecision, AuditResourceDigestFormat};
use mainframe_env_host_api::{CicsConditionPolicy, CicsRequest, MqOperation, MqRequest, MqResult};
use mainframe_env_mq::{
    MqLimits, MqQueueDefinition, MqReplayDependency, MqReplayRetentionState,
    describe_mq_replay_row, mq_abi_library,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    materialize_host_abi_libraries,
};
use mainframe_env_store::SqliteStateStore;
use mainframe_env_store_api::{EffectRecord, ExecutionRecord};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[path = "codate_clock.rs"]
mod clock;
#[path = "codate_oracle.rs"]
mod oracle;
#[path = "codate_source_pins.rs"]
mod pins;
#[path = "codate_receipt.rs"]
mod receipt;
use receipt::{ReceiptOwner, require_owned_directory};
#[path = "codate_authorization.rs"]
mod authorization;
use authorization::{authorization_invocation, mq_client_invocation};
#[path = "codate_tests.rs"]
mod tests;

#[derive(Debug)]
struct CorpusProblem {
    code: &'static str,
    source_file: &'static str,
    source_line: u32,
    source_column: u32,
}
impl CorpusProblem {
    #[track_caller]
    fn new(code: &'static str, _detail: impl AsRef<str>) -> Self {
        let origin = std::panic::Location::caller();
        Self {
            code,
            source_file: origin.file(),
            source_line: origin.line(),
            source_column: origin.column(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestOffer {
    program: String,
    transaction: String,
    input_queue: String,
    reply_queue: String,
    body: Vec<u8>,
    message_id: [u8; 24],
    correlation_id: [u8; 24],
    reply: Vec<u8>,
}

fn read_corpus_file(root: &Path, path: &Path) -> Result<Vec<u8>, CorpusProblem> {
    need(path.starts_with(root) && path.canonicalize().map_err(|_| refused())? == path)?;
    let before = fs::symlink_metadata(path).map_err(|_| refused())?;
    need(before.is_file() && !before.file_type().is_symlink() && before.len() <= 2 * 1024 * 1024)?;
    let mut file = fs::File::open(path).map_err(|_| refused())?;
    let opened = file.metadata().map_err(|_| refused())?;
    need((before.dev(), before.ino(), before.len()) == (opened.dev(), opened.ino(), opened.len()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused())?;
    let after = fs::symlink_metadata(path).map_err(|_| refused())?;
    need(
        bytes.len() as u64 == before.len()
            && bytes.len() <= 2 * 1024 * 1024
            && (after.dev(), after.ino(), after.len())
                == (before.dev(), before.ino(), before.len())
            && !after.file_type().is_symlink(),
    )?;
    Ok(bytes)
}
fn mq_queue(name: &str, trigger_program: Option<&str>) -> MqQueueDefinition {
    MqQueueDefinition {
        name: name.into(),
        trigger_program: trigger_program.map(str::to_string),
    }
}

const PROGRAM: &str = "CODATE01";
const TRANSACTION: &str = "CDRD";
const PRIMARY: &str = "app/app-vsam-mq/cbl/CODATE01.cbl";
const ERROR_QUEUE: &str = "CARD.DEMO.ERROR";
const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[track_caller]
fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.mq.installed_legacy_date",
        "installed source, START, complete wire/image, replay, owner or teardown differs",
    )
}
#[track_caller]
fn need(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(refused()) }
}
fn source_inputs(corpus: &Path) -> Result<Vec<(String, Vec<u8>)>, CorpusProblem> {
    pins::SOURCES
        .iter()
        .map(|(path, digest, length)| {
            let bytes = read_corpus_file(corpus, &corpus.join(path))?;
            need(bytes.len() == *length && format!("{:x}", Sha256::digest(&bytes)) == *digest)?;
            Ok(((*path).into(), bytes))
        })
        .collect()
}
fn scenario(corpus: &Path) -> Result<RequestOffer, CorpusProblem> {
    source_inputs(corpus)?;
    oracle::offer()
}

// This is the repository's pinned compatibility MQTM, not a native IBM layout.
// Every source member is authenticated before compilation. Numeric BINARY fields
// use the existing reference compiler's four-byte signed big-endian layout.
fn start_payload(queue: &str) -> Result<Vec<u8>, CorpusProblem> {
    need(queue == "CARD.DEMO.REQUEST.DATE")?;
    let library = mq_abi_library();
    let member = library
        .members
        .iter()
        .find(|m| m.name == "CMQTML")
        .ok_or_else(refused)?;
    need(format!("{:x}", Sha256::digest(member.source.as_bytes())) == pins::MQTM_SOURCE_SHA)?;
    let mut bytes = vec![b' '; 684];
    bytes[..4].copy_from_slice(b"TM  ");
    bytes[4..8].copy_from_slice(&1_i32.to_be_bytes());
    bytes[8..8 + queue.len()].copy_from_slice(queue.as_bytes());
    bytes[168..172].copy_from_slice(&0_i32.to_be_bytes());
    Ok(bytes)
}

fn definition(
    corpus: &Path,
) -> Result<(OnlineApplicationDefinition, ArtifactRecord), CorpusProblem> {
    source_inputs(corpus)?;
    let limits = SourceLimits::default();
    let abi = materialize_host_abi_libraries(
        &[
            mainframe_env_cics::cics_abi_library(),
            mainframe_env_db2::db2_abi_library(),
            mainframe_env_mq::mq_abi_library(),
        ],
        limits,
    )
    .map_err(|_| refused())?;
    let primary = LogicalPath::new(PRIMARY, limits.max_path_bytes).map_err(|_| refused())?;
    let bytes = read_corpus_file(corpus, &corpus.join(PRIMARY))?;
    need(format!("{:x}", Sha256::digest(&bytes)) == pins::PROGRAM_SHA)?;
    let mut files = vec![
        SourceFile::input(
            PRIMARY,
            bytes,
            SourceFormat::Fixed,
            SourceEncoding::Utf8,
            limits,
        )
        .map_err(|_| refused())?,
    ];
    files.extend(abi.files);
    let libraries = abi.libraries;
    let bundle = SourceBundle::with_libraries(
        &primary,
        files,
        libraries,
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .map_err(|_| refused())?;
    need(bundle.options().is_empty() && bundle.provenance().is_empty())?;
    let compiler = CobolCompiler::default();
    let result = compiler
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(|_| refused())?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|_| refused())?,
        })
        .map_err(|_| refused())?;
    let CompilerResult::Published { artifact, .. } = result else {
        return Err(refused());
    };
    let program = OnlineProgramDefinition::current(PROGRAM, &artifact);
    let record =
        crate::cobol::artifact::published_artifact_record(&artifact).map_err(|_| refused())?;
    // Exact CSD bytes are among the twelve pins. Its original CDRD->CODATE01
    // mapping is selected without rewriting any first-party source.
    Ok((
        OnlineApplicationDefinition {
            programs: vec![program],
            transactions: BTreeMap::from([(TRANSACTION.into(), PROGRAM.into())]),
            // Existing server-unit map-catalog admission, unused by screenless
            // CODATE01. This synthetic map is never a BMS observation.
            maps: vec![BmsMapDefinition {
                mapset: "CDRDTEST".into(),
                map: "CDRDTEST".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: Vec::new(),
            }],
        },
        record,
    ))
}

fn installed(
    store: &dyn PlatformStore,
    artifacts: &Path,
    record: &ArtifactRecord,
) -> Result<(), CorpusProblem> {
    let program = store
        .get_provider_state("online-program", PROGRAM)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    let transaction = store
        .get_provider_state("online-transaction", TRANSACTION)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    need(
        program.namespace == "online-program"
            && program.key == PROGRAM
            && program.version == 1
            && program.payload == record.artifact.as_str().as_bytes()
            && transaction.namespace == "online-transaction"
            && transaction.key == TRANSACTION
            && transaction.version == 1
            && transaction.payload == PROGRAM.as_bytes(),
    )?;
    let local = LocalArtifactStore::open(artifacts, 64 * 1024 * 1024).map_err(|_| refused())?;
    need(
        local
            .get_artifact(&record.artifact)
            .map_err(|_| refused())?
            .as_ref()
            == Some(record),
    )
}

fn rows(
    store: &dyn PlatformStore,
    namespace: &str,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let values = store
        .list_provider_state(namespace, MAX_ROWS + 1)
        .map_err(|_| refused())?;
    need(values.len() <= MAX_ROWS)?;
    let mut total = 0usize;
    let mut result = BTreeMap::new();
    for row in values {
        total = total.checked_add(row.payload.len()).ok_or_else(refused)?;
        need(
            row.namespace == namespace
                && total <= MAX_BYTES
                && result.insert(row.key.clone(), row).is_none(),
        )?;
    }
    Ok(result)
}
fn unchanged(
    before: &BTreeMap<String, ProviderStateRecord>,
    after: &BTreeMap<String, ProviderStateRecord>,
) -> Result<(), CorpusProblem> {
    need(before.iter().all(|(key, row)| after.get(key) == Some(row)))
}

fn request(operation: MqOperation, sequence: u64, key: IdempotencyKey) -> MqRequest {
    MqRequest {
        operation,
        queue: None,
        handle: None,
        options: 0,
        message: Vec::new(),
        message_id: None,
        correlation_id: None,
        wait_ticks: 0,
        max_message_bytes: 1,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: key,
            transaction: None,
        }),
    }
}
fn expected_mq(
    index: usize,
    sequence: u64,
    key: IdempotencyKey,
    handles: [u32; 3],
    offer: &RequestOffer,
    reply: &[u8],
) -> Result<MqRequest, CorpusProblem> {
    need(index < 9 && sequence > 0 && reply.len() == 1000)?;
    let mut value = request(
        match index {
            0..=2 => MqOperation::Open,
            3 | 5 => MqOperation::Get,
            4 => MqOperation::Put,
            _ => MqOperation::Close,
        },
        sequence,
        key,
    );
    match index {
        0..=2 => {
            value.queue = Some(
                match index {
                    0 => ERROR_QUEUE,
                    1 => &offer.input_queue,
                    _ => &offer.reply_queue,
                }
                .into(),
            );
            value.options = if index == 1 {
                2 + 128 + 8192
            } else {
                16 + 512 + 8192
            };
        }
        3 | 5 => {
            value.handle = Some(handles[1]);
            value.options = 2 + 8192 + 16384 + 1;
            value.wait_ticks = 5000;
            value.max_message_bytes = 1000;
        }
        4 => {
            value.handle = Some(handles[2]);
            value.options = 2 + 32 + 8192;
            value.message = reply.to_vec();
            value.message_id = Some(offer.message_id.to_vec());
            value.correlation_id = Some(offer.correlation_id.to_vec());
            value.max_message_bytes = 1000;
        }
        _ => {}
    }
    // Source closes INPUT, OUTPUT, ERROR; the handle order is ERROR, INPUT, OUTPUT.
    if index >= 6 {
        value.handle = Some(handles[[1, 2, 0][index - 6]]);
    }
    Ok(value)
}
fn bind_request(request: &MqRequest, effect: &EffectRecord) -> Result<(), CorpusProblem> {
    bind_request_digests(
        request,
        effect.request_digest,
        effect.intent.audit_resource.as_ref().ok_or_else(refused)?,
    )
}
fn bind_request_digests(
    request: &MqRequest,
    digest: [u8; 32],
    resource: &mainframe_env_execution_api::AuditResourceDigest,
) -> Result<(), CorpusProblem> {
    let request = HostRequest::Mq(request.clone());
    need(
        mainframe_env_host_api::canonical_request_digest(&request).map_err(|_| refused())?
            == digest
            && *resource == mainframe_env_host_api::canonical_audit_resource_digest(&request),
    )
}
fn expected_mq_result(
    index: usize,
    handles: [u32; 3],
    offer: &RequestOffer,
) -> Result<MqResult, CorpusProblem> {
    need(index < 9)?;
    let mut result = MqResult {
        completion_code: 0,
        reason_code: 0,
        handle: None,
        message: Vec::new(),
        message_id: None,
        correlation_id: None,
        trigger_program: None,
    };
    match index {
        0..=2 => result.handle = Some(handles[index]),
        3 => {
            result.message = offer.body.clone();
            result.message_id = Some(offer.message_id.to_vec());
            result.correlation_id = Some(offer.correlation_id.to_vec());
        }
        4 => {
            result.message_id = Some(offer.message_id.to_vec());
            result.correlation_id = Some(offer.correlation_id.to_vec());
        }
        5 => {
            result.completion_code = 2;
            result.reason_code = 2033;
        }
        _ => {}
    }
    Ok(result)
}

fn actor(
    store: &dyn PlatformStore,
    owner: &ExecutionRecord,
    effect: &EffectRecord,
    capability: &str,
) -> Result<(), CorpusProblem> {
    need(
        effect.execution_id == owner.execution_id
            && effect.run_unit_id == owner.run_unit_id
            && effect.intent.owner == owner.execution_id
            && effect.intent.attempt == owner.attempt
            && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some(capability)
            && effect.state == EffectState::Completed
            && effect.result_digest.is_some()
            && effect.digest_format == EffectDigestFormat::CanonicalHostV1,
    )?;
    audit(
        store,
        owner,
        effect.sequence,
        capability,
        effect
            .intent
            .audit_invocation_key
            .as_ref()
            .ok_or_else(refused)?,
        effect.intent.audit_resource.as_ref().ok_or_else(refused)?,
    )
}
fn audit(
    store: &dyn PlatformStore,
    owner: &ExecutionRecord,
    sequence: u64,
    capability: &str,
    invocation: &IdempotencyKey,
    resource: &mainframe_env_execution_api::AuditResourceDigest,
) -> Result<(), CorpusProblem> {
    need(resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1)?;
    let audits = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| refused())?;
    need(audits.len() <= MAX_ROWS)?;
    let selected = audits
        .iter()
        .filter(|a| {
            a.effect_sequence == sequence
                && a.capability.as_str() == capability
                && a.resource == *resource
        })
        .collect::<Vec<_>>();
    need(
        matches!(selected.as_slice(), [a] if a.execution_id == owner.execution_id
        && a.run_unit_id == owner.run_unit_id && a.principal == owner.principal
        && a.attempt == owner.attempt && a.invocation_key == *invocation
        && a.resource == *resource && a.decision == AuditDecision::Success),
    )
}
fn mq_result(row: &ProviderStateRecord) -> Result<MqResult, CorpusProblem> {
    // Only called AFTER the provider's strict full-codec retention validator.
    let payload: serde_json::Value = serde_json::from_slice(&row.payload).map_err(|_| refused())?;
    let value = &payload["value"];
    let result = MqResult {
        completion_code: serde_json::from_value(value["completion_code"].clone())
            .map_err(|_| refused())?,
        reason_code: serde_json::from_value(value["reason_code"].clone()).map_err(|_| refused())?,
        handle: serde_json::from_value(value["handle"].clone()).map_err(|_| refused())?,
        message: serde_json::from_value(value["message"].clone()).map_err(|_| refused())?,
        message_id: serde_json::from_value(value["message_id"].clone()).map_err(|_| refused())?,
        correlation_id: serde_json::from_value(value["correlation_id"].clone())
            .map_err(|_| refused())?,
        trigger_program: serde_json::from_value(value["trigger_program"].clone())
            .map_err(|_| refused())?,
    };
    need(result.message.len() <= 1000)?;
    Ok(result)
}

struct GuestEvidence<'a> {
    record: &'a ArtifactRecord,
    work_id: &'a str,
    before_cics: &'a BTreeMap<String, ProviderStateRecord>,
    before_mq: &'a BTreeMap<String, ProviderStateRecord>,
    clock: &'a clock::SourceClock,
    offer: &'a RequestOffer,
}
fn guest_proof(
    store: &dyn PlatformStore,
    evidence: &GuestEvidence<'_>,
) -> Result<(), CorpusProblem> {
    let record = evidence.record;
    let work_id = evidence.work_id;
    let before_cics = evidence.before_cics;
    let before_mq = evidence.before_mq;
    let offer = evidence.offer;
    let work = store
        .get_work(work_id)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    need(
        work.state == WorkState::Completed
            && work.required_selector.as_str() == "cics:start"
            && work.required_generation == mainframe_env_cics::CICS_START_WORK_GENERATION,
    )?;
    let owner = store
        .get_execution(&work.execution_id)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    need(
        owner.execution_id == work.execution_id
            && owner.run_unit_id.as_str() == format!("run-{}", work.execution_id)
            && owner.selector.as_str() == "program:CODATE01"
            && owner.artifact == record.artifact
            && owner.principal.as_str() == "IBMUSER"
            && owner.state == ExecutionState::Completed
            && owner.attempt > 0
            && owner.terminal_tick.is_some_and(|t| t > 0)
            && owner.owner_lease.is_none()
            && owner.lease_expiry_tick.is_none(),
    )?;
    clock_proof(store, &owner, evidence.clock)?;
    let terminal = store
        .events(&owner.execution_id, owner.version, 2)
        .map_err(|_| refused())?;
    need(
        matches!(terminal.as_slice(), [e] if e.validate() && e.execution_id == owner.execution_id
        && e.run_unit_id == owner.run_unit_id && e.sequence == owner.version
        && e.attempt == owner.attempt && Some(e.tick) == owner.terminal_tick
        && matches!(e.kind, LifecycleEventKind::Completed { return_code: 0 })),
    )?;
    let cics = rows(store, "cics-effect-replay-v1")?;
    let mq = rows(store, "mq-v1-replay")?;
    unchanged(before_cics, &cics)?;
    unchanged(before_mq, &mq)?;
    let mut outer = BTreeMap::new();
    for (key, row) in cics.iter().filter(|(k, _)| !before_cics.contains_key(*k)) {
        let effect = store
            .effect(&IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refused())?;
        need(
            matches!(
                descriptor.codec,
                CicsReplayCodecVersion::RetentionV3 | CicsReplayCodecVersion::PlainReadV4
            ) && descriptor.retention == CicsReplayRetentionState::Terminal
                && descriptor.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                && descriptor.owner_run_unit.as_deref() == Some(owner.run_unit_id.as_str()),
        )?;
        actor(store, &owner, &effect, "host.cics.execute")?;
        need(outer.insert(key.clone(), effect).is_none())?;
    }
    need(!outer.is_empty())?;
    let mut core = BTreeMap::new();
    let mut commits = 0usize;
    for (key, row) in mq.iter().filter(|(k, _)| !before_mq.contains_key(*k)) {
        let key = IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?;
        let effect = store.effect(&key).map_err(|_| refused())?;
        let descriptor = describe_mq_replay_row(row, effect.as_ref(), MqLimits::default())
            .map_err(|_| refused())?;
        need(
            descriptor.retention == MqReplayRetentionState::Terminal
                && descriptor.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                && descriptor.owner_run_unit.as_deref() == Some(owner.run_unit_id.as_str()),
        )?;
        let result = mq_result(row)?;
        need(
            mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Mq(result.clone())))
                .map_err(|_| refused())?
                == descriptor.result_digest,
        )?;
        match descriptor.dependency {
            Some(MqReplayDependency::CoreEffect) => {
                let effect = effect.ok_or_else(refused)?;
                need(core.insert(effect.sequence, (effect, result)).is_none())?;
            }
            Some(MqReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key,
            }) => {
                need(
                    effect.is_none()
                        && run_unit == owner.run_unit_id.as_str()
                        && descriptor.sequence == Some(sequence)
                        && result.completion_code == 0
                        && result.reason_code == 0
                        && result.message.is_empty()
                        && result.handle.is_none()
                        && result.message_id.is_none()
                        && result.correlation_id.is_none()
                        && result.trigger_program.is_none(),
                )?;
                let outer = outer.get(&outer_effect_key).ok_or_else(refused)?;
                let mut commit = request(MqOperation::Commit, sequence, key);
                commit.mutation.as_mut().ok_or_else(refused)?.transaction =
                    Some(TRANSACTION.into());
                let expected = HostRequest::Mq(commit);
                need(
                    mainframe_env_host_api::canonical_request_digest(&expected)
                        .map_err(|_| refused())?
                        == descriptor.request_digest,
                )?;
                audit(
                    store,
                    &owner,
                    sequence,
                    "host.mq.write",
                    outer
                        .intent
                        .audit_invocation_key
                        .as_ref()
                        .ok_or_else(refused)?,
                    &mainframe_env_host_api::canonical_audit_resource_digest(&expected),
                )?;
                let row = store
                    .get_provider_state("cics-uow", &outer_effect_key)
                    .map_err(|_| refused())?
                    .ok_or_else(refused)?;
                let undo = store
                    .get_provider_state("cics-uow-undo", owner.run_unit_id.as_str())
                    .map_err(|_| refused())?;
                let uow = describe_cics_uow_row(&row, undo.as_ref()).map_err(|_| refused())?;
                need(
                    uow.state == CicsUowState::Committed
                        && uow.dependency == CicsUowDependencyState::Clear
                        && uow.transaction == TRANSACTION
                        && uow.effect_key.as_deref() == Some(outer_effect_key.as_str())
                        && uow.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                        && uow.owner_run_unit.as_deref() == Some(owner.run_unit_id.as_str()),
                )?;
                commits += 1;
            }
            None => return Err(refused()),
        }
    }
    need(core.len() == 9 && commits > 0)?;
    let calls = core.into_values().collect::<Vec<_>>();
    let handles = [
        calls[0].1.handle.ok_or_else(refused)?,
        calls[1].1.handle.ok_or_else(refused)?,
        calls[2].1.handle.ok_or_else(refused)?,
    ];
    need(
        handles.iter().all(|h| *h > 0)
            && handles[0] != handles[1]
            && handles[0] != handles[2]
            && handles[1] != handles[2],
    )?;
    let reply = &calls[4].1;
    need(reply.completion_code == 0 && reply.reason_code == 0)?;
    // The output queue contains the actual guest-produced bytes. The PUT result
    // alone has an empty message. Its expected request is bound below to the
    // independent source-authored full reply, not a hash sampled from the guest.
    let bytes = expected_date_reply(offer)?;
    for (index, (effect, result)) in calls.iter().enumerate() {
        let expected = expected_mq(
            index,
            effect.sequence,
            effect.key.clone(),
            handles,
            offer,
            &bytes,
        )?;
        let capability =
            HostRequest::Mq(expected.clone()).required_capability(InvocationLimits::default());
        actor(store, &owner, effect, capability.as_str())?;
        bind_request(&expected, effect)?;
        need(*result == expected_mq_result(index, handles, offer)?)?;
    }
    need(
        calls[3].1.message == offer.body
            && calls[3].1.message_id.as_deref() == Some(&offer.message_id)
            && calls[3].1.correlation_id.as_deref() == Some(&offer.correlation_id)
            && reply.message_id.as_deref() == Some(&offer.message_id)
            && reply.correlation_id.as_deref() == Some(&offer.correlation_id),
    )?;
    Ok(())
}

fn expected_date_reply(offer: &RequestOffer) -> Result<Vec<u8>, CorpusProblem> {
    let independent = oracle::offer()?;
    need(*offer == independent)?;
    Ok(independent.reply)
}

fn clock_proof(
    store: &dyn PlatformStore,
    owner: &ExecutionRecord,
    clock: &clock::SourceClock,
) -> Result<(), CorpusProblem> {
    let observed = clock.observations()?;
    let selected = observed
        .iter()
        .filter(|o| o.execution_id == owner.execution_id)
        .collect::<Vec<_>>();
    let [actual] = selected.as_slice() else {
        return Err(refused());
    };
    need(
        actual.run_unit_id == owner.run_unit_id
            && actual.principal == owner.principal
            && actual.attempt == owner.attempt
            && actual.sequence > 0
            && actual.request == HostRequest::Clock(ClockRequest::UtcTimestamp)
            && actual.result == HostResult::Clock(oracle::UTC_INPUT.into()),
    )?;
    audit(
        store,
        owner,
        actual.sequence,
        "host.clock",
        &actual.audit_key,
        &mainframe_env_host_api::canonical_audit_resource_digest(&actual.request),
    )
}

fn compare_queues(
    input: &[Vec<u8>],
    output: &[Vec<u8>],
    errors: &[Vec<u8>],
    offer: &RequestOffer,
    reply: Option<&[u8]>,
) -> Result<(), CorpusProblem> {
    need(errors.is_empty())?;
    need(
        input
            == if reply.is_some() {
                Vec::<Vec<u8>>::new()
            } else {
                vec![offer.body.clone()]
            },
    )?;
    need(output == reply.map_or_else(Vec::new, |b| vec![b.to_vec()]))
}
fn queues(
    server: &ProductServer,
    offer: &RequestOffer,
    reply: Option<&[u8]>,
) -> Result<(), CorpusProblem> {
    let mq = server.mq_service();
    compare_queues(
        &mq.queue_messages(&offer.input_queue)
            .map_err(|_| refused())?,
        &mq.queue_messages(&offer.reply_queue)
            .map_err(|_| refused())?,
        &mq.queue_messages(ERROR_QUEUE).map_err(|_| refused())?,
        offer,
        reply,
    )
}
fn sqlite(url: &str) -> Result<Arc<dyn PlatformStore>, CorpusProblem> {
    SqliteStateStore::open(url, 64 * 1024 * 1024, 65_536)
        .map(|store| Arc::new(store) as Arc<dyn PlatformStore>)
        .map_err(|_| refused())
}
fn server(
    config: &ServerConfig,
    store: Arc<dyn PlatformStore>,
    clock: Arc<clock::SourceClock>,
) -> Result<Arc<ProductServer>, CorpusProblem> {
    let artifacts = Arc::new(ProductArtifactStore::Local(
        LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024).map_err(|_| refused())?,
    ));
    ProductServer::open_configured_with_clocks(
        config.clone(),
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        Arc::new(RejectPackageTrust),
        artifacts,
        RuntimeClocks {
            jes: None,
            host: Some(clock),
        },
    )
    .map_err(|_| refused())
}
fn ready_to_compare(state: WorkState, active: usize, jes_active: usize) -> bool {
    state == WorkState::Completed && active == 0 && jes_active == 0
}

fn seed(server: &ProductServer, offer: &RequestOffer) -> Result<String, CorpusProblem> {
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .map_err(|_| refused())?;
    let racf = server.racf_service();
    for (class, resource) in [
        ("TCICSTRN", "CICS.CDRD"),
        ("FACILITY", "CICS.PROGRAM.CODATE01"),
    ] {
        racf.define_profile(class, resource, "IBMUSER", None)
            .map_err(|_| refused())?;
        racf.permit(class, resource, "IBMUSER", AccessIntent::Execute)
            .map_err(|_| refused())?;
    }
    for queue in [&offer.input_queue, &offer.reply_queue, ERROR_QUEUE] {
        racf.define_profile("QUEUE", queue, "IBMUSER", None)
            .map_err(|_| refused())?;
        racf.permit("QUEUE", queue, "IBMUSER", AccessIntent::Update)
            .map_err(|_| refused())?;
    }
    let mq = server.mq_service();
    mq.install(vec![
        mq_queue(&offer.input_queue, None),
        mq_queue(&offer.reply_queue, None),
        mq_queue(ERROR_QUEUE, None),
    ])
    .map_err(|_| refused())?;
    let invocation = mq_client_invocation(
        "installed-date-request-client",
        true,
        ServiceClass::Interactive,
    )?;
    let mut put = request(
        MqOperation::PutOne,
        1,
        IdempotencyKey::new("installed-date-request01", InvocationLimits::default())
            .map_err(|_| refused())?,
    );
    put.queue = Some(offer.input_queue.clone());
    put.message = offer.body.clone();
    put.message_id = Some(offer.message_id.to_vec());
    put.correlation_id = Some(offer.correlation_id.to_vec());
    put.max_message_bytes = 1000;
    let result = mq.execute(&invocation, &put).map_err(|_| refused())?;
    need(
        result.completion_code == 0
            && result.reason_code == 0
            && result.message_id.as_deref() == Some(&offer.message_id)
            && result.correlation_id.as_deref() == Some(&offer.correlation_id),
    )?;
    let seed = authorization_invocation("date-start-producer", true, ServiceClass::Interactive)?;
    let session = SessionId::new("date-start-producer-session", 128).map_err(|_| refused())?;
    let cics = server.cics_service();
    cics.launch_background_task(seed.clone(), &session, TRANSACTION)
        .map_err(|_| refused())?;
    let argument = |schema, bytes| {
        BoundedPayload::new(schema, bytes, InvocationLimits::default()).map_err(|_| refused())
    };
    let key = IdempotencyKey::new("date-start-offer01", InvocationLimits::default())
        .map_err(|_| refused())?;
    let command = CicsRequest {
        operation: CicsOperation::Start,
        arguments: BTreeMap::from([
            (
                "TRANSID".into(),
                argument(
                    "mainframe-env.cics.literal@1",
                    TRANSACTION.as_bytes().to_vec(),
                )?,
            ),
            (
                "REQID".into(),
                argument("mainframe-env.cics.literal@1", b"J15DT001".to_vec())?,
            ),
            (
                "FROM".into(),
                argument(
                    "mainframe-env.cics.storage-value@1",
                    start_payload(&offer.input_queue)?,
                )?,
            ),
            (
                "INTERVAL".into(),
                argument("mainframe-env.cics.decimal@1", b"0".to_vec())?,
            ),
        ]),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: key.clone(),
            transaction: Some(TRANSACTION.into()),
        }),
    };
    let response = cics
        .invoke(
            &EffectRequest {
                run_unit: seed.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: seed.deadline_tick,
                idempotency_key: Some(key),
                request: HostRequest::Cics(command.clone()),
            },
            command,
        )
        .map_err(|_| refused())?;
    need(response.condition == "NORMAL")?;
    cics.discard_terminal_run_if_present(&session, seed.principal.id(), 1)
        .map_err(|_| refused())?;
    cics.finish_background_task(&session, seed.principal.id())
        .map_err(|_| refused())?;
    Ok("cics-start:J15DT001".into())
}

fn run(
    corpus: &Path,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<(), CorpusProblem> {
    let offer = scenario(corpus)?;
    let reply = expected_date_reply(&offer)?;
    let clock = Arc::new(clock::SourceClock::new(oracle::UTC_INPUT)?);
    let database = owned.evidence.join("state.sqlite3");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&database)
        .map_err(|_| refused())?;
    let url = format!(
        "sqlite://{}?mode=rw",
        database.to_str().ok_or_else(refused)?
    );
    let config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: url.clone(),
        artifact_root: owned.artifacts.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| refused())?;
    let store = sqlite(&url)?;
    let first = server(&config, store.clone(), clock.clone())?;
    let seeded = (|| {
        owned.check()?;
        let (app, record) = definition(corpus)?;
        first
            .install_online_application(app)
            .map_err(|_| refused())?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        let work_id = seed(&first, &offer)?;
        queues(&first, &offer, None)?;
        let work = store
            .get_work(&work_id)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        need(work.state == WorkState::Queued && work.payload == b"J15DT001")?;
        Ok((record, work_id))
    })();
    let drained = runtime.block_on(first.graceful_shutdown());
    receipt["seed_server_drain"] = serde_json::json!(drained);
    owned.save(receipt)?;
    drop(first);
    drop(store);
    let (record, work_id) = seeded?;
    need(drained)?;
    // A new SQLite connection and ProductServer, not another wrapper around an
    // existing in-memory store. The complete request remains queued across stop.
    let store = sqlite(&url)?;
    let current = server(&config, store.clone(), clock.clone())?;
    let executed = runtime.block_on(async {
        installed(store.as_ref(), &owned.artifacts, &record)?;
        scenario(corpus)?;
        queues(&current, &offer, None)?;
        receipt["pending_request_restart_compared"] = serde_json::json!(true);
        receipt["physical_sqlite_reopens"] = serde_json::json!(1);
        owned.save(receipt)?;
        let before_cics = rows(store.as_ref(), "cics-effect-replay-v1")?;
        let before_mq = rows(store.as_ref(), "mq-v1-replay")?;
        current.start_background_workers().map_err(|_| refused())?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let work = store
                .get_work(&work_id)
                .map_err(|_| refused())?
                .ok_or_else(refused)?;
            let metrics = current.metrics();
            if ready_to_compare(work.state, metrics.active, metrics.jes_active) {
                break;
            }
            need(
                Instant::now() < deadline
                    && matches!(
                        work.state,
                        WorkState::Queued | WorkState::Claimed | WorkState::Completed
                    ),
            )?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        need(Instant::now() < deadline)?;
        guest_proof(
            store.as_ref(),
            &GuestEvidence {
                record: &record,
                work_id: &work_id,
                before_cics: &before_cics,
                before_mq: &before_mq,
                clock: &clock,
                offer: &offer,
            },
        )?;
        queues(&current, &offer, Some(&reply))?;
        scenario(corpus)?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        receipt["actual_installed_guest_compared"] = serde_json::json!(true);
        receipt["host_clock_input_compared"] = serde_json::json!(true);
        receipt["encoded_wait_ticks"] = serde_json::json!(5000);
        receipt["actual_wait_duration_claimed"] = serde_json::json!(false);
        receipt["empty_get_condition_compared"] = serde_json::json!(2033);
        receipt["full_reply_compared_bytes"] = serde_json::json!(1000);
        Ok::<_, CorpusProblem>((
            rows(store.as_ref(), "mq-v1-replay")?,
            rows(store.as_ref(), "cics-effect-replay-v1")?,
            before_mq,
            before_cics,
        ))
    });
    let drained = runtime.block_on(current.graceful_shutdown());
    receipt["guest_server_drain"] = serde_json::json!(drained);
    let saved = owned.save(receipt);
    drop(current);
    drop(store);
    let (mq_rows, cics_rows, before_mq, before_cics) = executed?;
    saved?;
    need(drained)?;
    let store = sqlite(&url)?;
    let reopened = server(&config, store.clone(), clock.clone())?;
    let compared = (|| {
        owned.check()?;
        scenario(corpus)?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        queues(&reopened, &offer, Some(&reply))?;
        need(
            rows(store.as_ref(), "mq-v1-replay")? == mq_rows
                && rows(store.as_ref(), "cics-effect-replay-v1")? == cics_rows,
        )?;
        guest_proof(
            store.as_ref(),
            &GuestEvidence {
                record: &record,
                work_id: &work_id,
                before_cics: &before_cics,
                before_mq: &before_mq,
                clock: &clock,
                offer: &offer,
            },
        )?;
        receipt["guest_proof_rechecked_after_physical_reopen"] = serde_json::json!(true);
        receipt["clock_input_observations"] = serde_json::json!(clock.observations()?.len());
        // This explicit provider client runs only after the complete first-party
        // proof is rechecked. It independently reads persisted reply IDs as well
        // as payload; it is not another guest effect or evidence of a core actor.
        let invocation = mq_client_invocation(
            "date-reopened-reply-client",
            true,
            ServiceClass::Interactive,
        )?;
        let mut get = request(
            MqOperation::Get,
            1,
            IdempotencyKey::new("date-reopened-reply-read01", InvocationLimits::default())
                .map_err(|_| refused())?,
        );
        get.queue = Some(offer.reply_queue.clone());
        get.max_message_bytes = 1000;
        let mq = reopened.mq_service();
        need(
            mq.execute(&invocation, &get).map_err(|_| refused())?
                == MqResult {
                    completion_code: 0,
                    reason_code: 0,
                    handle: None,
                    message: reply.clone(),
                    message_id: Some(offer.message_id.to_vec()),
                    correlation_id: Some(offer.correlation_id.to_vec()),
                    trigger_program: None,
                },
        )?;
        for queue in [&offer.input_queue, &offer.reply_queue, ERROR_QUEUE] {
            need(mq.queue_messages(queue).map_err(|_| refused())?.is_empty())?;
        }
        unchanged(&mq_rows, &rows(store.as_ref(), "mq-v1-replay")?)?;
        need(rows(store.as_ref(), "cics-effect-replay-v1")? == cics_rows)?;
        receipt["reopened_reply_provider_client_read_compared"] = serde_json::json!(true);
        receipt["post_guest_restart_compared"] = serde_json::json!(true);
        receipt["physical_sqlite_reopens"] = serde_json::json!(2);
        Ok::<_, CorpusProblem>(())
    })();
    let drained = runtime.block_on(reopened.graceful_shutdown());
    receipt["reopened_server_drain"] = serde_json::json!(drained);
    let saved = owned.save(receipt);
    drop(reopened);
    drop(store);
    compared?;
    saved?;
    need(drained)?;
    owned.check()?;
    fs::remove_dir_all(&owned.artifacts).map_err(|_| refused())?;
    owned.artifacts_removed = true;
    owned.check()?;
    receipt["cleanup_verified"] = serde_json::json!(true);
    receipt["stage"] = serde_json::json!("completed-source-date-start-zero-credit");
    owned.save(receipt)
}

fn verify_from_env() -> Result<serde_json::Value, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os("CARDDEMO_CORPUS_DIR").ok_or_else(refused)?);
    need(corpus_dir.canonicalize().map_err(|_| refused())? == corpus_dir)?;
    scenario(&corpus_dir)?;
    let parent = env::temp_dir().canonicalize().map_err(|_| refused())?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|_| refused())?;
    need(!parent.starts_with(repository))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_nanos();
    let evidence = parent.join(format!(
        "mainframe-env-firstparty-cics-mq-date-{}-{nonce}",
        std::process::id()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .map_err(|_| refused())?;
    let meta = fs::symlink_metadata(&evidence).map_err(|_| refused())?;
    let identity = (meta.dev(), meta.ino());
    require_owned_directory(&evidence, identity)?;
    let artifacts = evidence.join("owned-artifacts");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .map_err(|_| refused())?;
    let meta = fs::symlink_metadata(&artifacts).map_err(|_| refused())?;
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, (meta.dev(), meta.ino()))?;
    let mut receipt = serde_json::json!({"schema":"mainframe-env.carddemo-installed-cics-legacy-date-private-unit@1",
        "declared_corpus_commit":"59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e",
        "selected_source_pins_compared":12,"whole_corpus_git_checked":false,"source_program":PROGRAM,"transaction":TRANSACTION,
        "backend":"sqlite","stage":"before-open","actual_installed_guest_compared":false,
        "pending_request_restart_compared":false,"post_guest_restart_compared":false,"physical_sqlite_reopens":0,
        "guest_proof_rechecked_after_physical_reopen":false,"reopened_reply_provider_client_read_compared":false,
        "seed_server_drain":false,"guest_server_drain":false,"reopened_server_drain":false,"cleanup_verified":false,
        "automatic_mq_trigger_listener_claimed":false,"typed_batch_mqi_frame_bound":false,
        "actual_wait_duration_claimed":false,"declared_host_clock_input":oracle::UTC_INPUT,"host_clock_input_compared":false,
        "application_credit":0,"vendor_credit":0,"acceptance_credit":0,
        "j15_j16_labels_changed":false,"original20_114_105_unchanged":true,"artifact_root":artifacts});
    owned.save(&receipt)?;
    let result = run(&corpus_dir, &mut owned, &mut receipt);
    if let Err(problem) = &result {
        receipt["problem_code"] = serde_json::json!(problem.code);
        receipt["failure_origin"] = serde_json::json!({
            "file": problem.source_file,
            "line": problem.source_line,
            "column": problem.source_column,
        });
    }
    let saved = owned.save(&receipt);
    result?;
    saved?;
    Ok(receipt)
}
