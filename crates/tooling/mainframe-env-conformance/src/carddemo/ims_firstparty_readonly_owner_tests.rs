//! Actual installed COPAUS0C readonly guest, independent complete COPY seed and map oracle.
//! This local owner does not certify J15–18, hold/update paths, raw CALL ABI or IBM parity.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsUowDependencyState,
    CicsUowState, describe_cics_replay_row, describe_cics_uow_row,
};
use mainframe_env_execution_api::{AuditDecision, AuditResourceDigestFormat, LifecycleEventKind};
use mainframe_env_host_api::HostRequest;
use mainframe_env_ims::{ImsReplayDependency, ImsReplayRetentionState, describe_ims_replay_row};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, EffectState, ExecutionRecord, ExecutionState, PlatformStore,
    ProviderStateRecord,
};

#[path = "ims_firstparty_readonly_oracle.rs"]
mod oracle;
#[path = "ims_firstparty_readonly_pins.rs"]
mod pins;
use oracle::{CallImage, Oracle, SegmentImage};

const PROGRAM: &str = "COPAUS0C";
const PRIMARY: &str = "app/app-authorization-ims-db2-mq/cbl/COPAUS0C.cbl";
const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.ims.installed_firstparty_readonly_owner",
        "actual source, installed guest, independent bytes or owned teardown differs",
    )
}
fn need(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(refused()) }
}
fn sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, expected) in pins::SOURCE_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        need(format!("{:x}", Sha256::digest(bytes)) == *expected)?;
    }
    Ok(())
}
fn definition(
    corpus: &Path,
) -> Result<(OnlineApplicationDefinition, ArtifactRecord, String), CorpusProblem> {
    sources(corpus)?;
    let mut bundles = explicit_carddemo_bundles(corpus)?
        .into_iter()
        .filter(|(name, _)| name == PRIMARY)
        .collect::<Vec<_>>();
    need(bundles.len() == 1)?;
    let (_, bundle) = bundles.pop().ok_or_else(refused)?;
    need(
        bundle
            .file(bundle.primary())
            .is_some_and(|file| file.path().as_str() == PRIMARY)
            && bundle.options().is_empty()
            && bundle.provenance().is_empty(),
    )?;
    let abi = subsystem_abi_libraries(SourceLimits::default())?;
    let mut expected = pins::SOURCE_PINS
        .iter()
        .filter(|(path, _)| {
            *path == PRIMARY
                || pins::COPY_ROOTS
                    .iter()
                    .any(|root| path.starts_with(&format!("{root}/")))
        })
        .map(|(path, sha)| {
            (
                (*path).to_string(),
                (
                    (*sha).to_string(),
                    SourceFormat::Fixed,
                    SourceEncoding::Utf8,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for file in &abi.files {
        need(
            expected
                .insert(
                    file.path().as_str().into(),
                    (
                        format!("{:x}", Sha256::digest(file.bytes())),
                        file.format(),
                        file.encoding(),
                    ),
                )
                .is_none(),
        )?;
    }
    let mut actual = BTreeMap::new();
    for file in bundle.files() {
        need(
            actual
                .insert(
                    file.path().as_str().to_string(),
                    (
                        format!("{:x}", Sha256::digest(file.bytes())),
                        file.format(),
                        file.encoding(),
                    ),
                )
                .is_none(),
        )?;
    }
    need(actual == expected)?;
    let mut libraries = Vec::new();
    for (index, root) in pins::COPY_ROOTS.iter().enumerate() {
        let members = pins::SOURCE_PINS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{root}/")))
            .map(|(path, _)| LogicalPath::new(*path, SourceLimits::default().max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        libraries.push(
            SourceLibrary::new(
                format!("application-{index:02}"),
                members,
                SourceLimits::default(),
            )
            .map_err(|_| refused())?,
        );
    }
    libraries.extend(abi.libraries);
    need(bundle.libraries() == libraries)?;
    let compiler = CobolCompiler::default();
    let semantic = compiler.analyze(&bundle).semantic.ok_or_else(refused)?;
    let result = compiler
        .compile(CompilerRequest {
            source: bundle.clone(),
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(|_| refused())?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|_| refused())?,
        })
        .map_err(|_| refused())?;
    let CompilerResult::Published { artifact, .. } = result else {
        return Err(refused());
    };
    let program = OnlineProgramDefinition::current(PROGRAM.to_string(), &artifact);
    let record = source_artifact_binding::publisher_record(&program)?;
    let csd = String::from_utf8(read_corpus_file(
        corpus,
        &corpus.join("app/app-authorization-ims-db2-mq/csd/CRDDEMO2.csd"),
    )?)
    .map_err(|_| refused())?;
    let routes = parse_csd(&csd)
        .map_err(package_problem)?
        .into_iter()
        .filter(|row| row.kind == "TRANSACTION" && row.name == "CPVS")
        .collect::<Vec<_>>();
    need(
        matches!(routes.as_slice(),[row] if row.properties.get("PROGRAM").map(String::as_str)==Some(PROGRAM)),
    )?;
    let map = bms::carddemo_map(
        corpus,
        "app/app-authorization-ims-db2-mq/bms/COPAU00.bms",
        &[semantic],
    )?;
    need(map.mapset == "COPAU00" && map.map == "COPAU0A" && map.fields.len() == 62)?;
    Ok((
        OnlineApplicationDefinition {
            programs: vec![program],
            transactions: BTreeMap::from([("CPVS".into(), PROGRAM.into())]),
            maps: vec![map],
        },
        record,
        bundle.id().to_hex(),
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
        .get_provider_state("online-transaction", "CPVS")
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    need(
        program.namespace == "online-program"
            && program.key == PROGRAM
            && program.version == 1
            && program.payload == record.artifact.as_str().as_bytes()
            && transaction.namespace == "online-transaction"
            && transaction.key == "CPVS"
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
    let found = store
        .list_provider_state_bounded(namespace, MAX_ROWS + 1, MAX_BYTES)
        .map_err(|_| refused())?;
    need(found.len() <= MAX_ROWS)?;
    let mut rows = BTreeMap::new();
    for row in found {
        need(row.namespace == namespace && rows.insert(row.key.clone(), row).is_none())?;
    }
    Ok(rows)
}
fn hierarchy(service: &ImsService, oracle: &Oracle) -> Result<(), CorpusProblem> {
    let control = ims_invocation("firstparty-readonly-observe", true, None)?;
    let result = service
        .execute(
            &control,
            &ims_request(
                ImsOperation::Unload,
                1,
                Some("DBPAUTP0"),
                &[],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(|_| refused())?;
    need(result.status == "  " && result.affected_segments == 0)?;
    let actual = result
        .segments
        .into_iter()
        .map(|segment| (segment.name, segment.parent_key, segment.data))
        .collect::<Vec<_>>();
    oracle.compare_segments(&actual)
}
fn dataset_images(server: &ProductServer, corpus: &Path) -> Result<(), CorpusProblem> {
    for (source, dataset, width) in [
        ("ACCTDATA", "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS", 300),
        ("CARDXREF", "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS", 50),
        ("CUSTDATA", "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS", 500),
    ] {
        let expected = read_corpus_file(
            corpus,
            &corpus.join(format!("app/data/EBCDIC/AWS.M2.CARDDEMO.{source}.PS")),
        )?;
        need(expected.len() % width == 0 && expected.len() / width <= 4096)?;
        let attributes = server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new(dataset, 128).map_err(|_| refused())?,
            })
            .map_err(|_| refused())?;
        let expected_attributes = DatasetAttributes {
            organization: DatasetOrganization::KeySequenced,
            record_format: RecordFormat::Fixed,
            logical_record_length: u32::try_from(width).map_err(|_| refused())?,
            key_offset: Some(0),
            key_length: Some(if source == "ACCTDATA" {
                11
            } else if source == "CARDXREF" {
                16
            } else {
                9
            }),
            ccsid: Some(37),
        };
        need(
            matches!(attributes,DatasetResult::Attributes {attributes,..} if attributes==expected_attributes),
        )?;
        let actual = utility_records(server, dataset, None)?;
        need(
            actual.len() == expected.len() / width
                && actual
                    .iter()
                    .map(Vec::as_slice)
                    .eq(expected.chunks_exact(width)),
        )?;
    }
    Ok(())
}
fn check_actor(
    store: &dyn PlatformStore,
    effect: &mainframe_env_store_api::EffectRecord,
    record: &ArtifactRecord,
    run: &RunUnitId,
    capability: &str,
) -> Result<ExecutionRecord, CorpusProblem> {
    let owner = store
        .get_execution(&effect.execution_id)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    need(
        owner.selector.as_str() == "program:COPAUS0C"
            && owner.artifact == record.artifact
            && owner.run_unit_id == *run
            && owner.principal.as_str() == "WEBADM"
            && owner.attempt > 0
            && owner.state == ExecutionState::Completed
            && owner.terminal_tick.is_some_and(|tick| tick > 0)
            && owner.owner_lease.is_none()
            && owner.lease_expiry_tick.is_none()
            && effect.execution_id == owner.execution_id
            && effect.run_unit_id == *run
            && effect.intent.owner == owner.execution_id
            && effect.intent.attempt == owner.attempt
            && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some(capability)
            && effect.state == EffectState::Completed
            && effect.result_digest.is_some(),
    )?;
    let events = store
        .events(&owner.execution_id, owner.version, 2)
        .map_err(|_| refused())?;
    need(matches!(events.as_slice(),[event] if event.validate()
        && event.execution_id==owner.execution_id && event.run_unit_id==*run
        && event.sequence==owner.version && event.attempt==owner.attempt
        && Some(event.tick)==owner.terminal_tick && event.tick>0
        && matches!(event.kind,LifecycleEventKind::Completed {return_code:0})))?;
    let resource = effect.intent.audit_resource.as_ref().ok_or_else(refused)?;
    let invocation = effect
        .intent
        .audit_invocation_key
        .as_ref()
        .ok_or_else(refused)?;
    need(resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1)?;
    let audits = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| refused())?;
    need(audits.len() <= MAX_ROWS)?;
    let matches = audits
        .iter()
        .filter(|audit| {
            audit.effect_sequence == effect.sequence && audit.capability.as_str() == capability
        })
        .collect::<Vec<_>>();
    need(
        matches!(matches.as_slice(),[audit] if audit.execution_id==owner.execution_id
        && audit.run_unit_id==*run && audit.principal==owner.principal
        && audit.attempt==owner.attempt && audit.invocation_key==*invocation
        && audit.resource==*resource && audit.decision==AuditDecision::Success),
    )?;
    Ok(owner)
}
fn observe_guest(
    store: &dyn PlatformStore,
    record: &ArtifactRecord,
    terminal: &serde_json::Value,
    session: &str,
    before_cics: &BTreeMap<String, ProviderStateRecord>,
    before_ims: &BTreeMap<String, ProviderStateRecord>,
    oracle: &Oracle,
) -> Result<String, CorpusProblem> {
    need(
        terminal["schema_version"] == "mainframe-env.cics-terminal@1"
            && terminal["session"] == session
            && terminal["principal"] == "WEBADM"
            && terminal["connected"] == true
            && terminal["suspended"] == false
            && terminal["mapset"] == "COPAU00"
            && terminal["map"] == "COPAU0A",
    )?;
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(refused)?,
        InvocationLimits::default(),
    )
    .map_err(|_| refused())?;
    oracle.compare_fields(&online_screen_fields(terminal)?)?;
    let after_cics = rows(store, "cics-effect-replay-v1")?;
    let after_ims = rows(store, "ims-v1-replay")?;
    need(
        before_cics
            .iter()
            .all(|(key, row)| after_cics.get(key) == Some(row))
            && before_ims
                .iter()
                .all(|(key, row)| after_ims.get(key) == Some(row)),
    )?;
    let mut owner = None;
    let mut cics_count = 0;
    let mut cics_effects = BTreeMap::new();
    for (key, row) in after_cics
        .iter()
        .filter(|(key, _)| !before_cics.contains_key(*key))
    {
        let effect = store
            .effect(&IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refused())?;
        need(
            descriptor.codec == CicsReplayCodecVersion::RetentionV3
                && descriptor.retention == CicsReplayRetentionState::Terminal
                && descriptor.owner_execution.as_deref() == Some(effect.execution_id.as_str())
                && descriptor.owner_run_unit.as_deref() == Some(run.as_str()),
        )?;
        let found = check_actor(store, &effect, record, &run, "host.cics.execute")?;
        need(owner.as_ref().is_none_or(|old| old == &found))?;
        owner = Some(found);
        cics_count += 1;
        need(cics_effects.insert(key.clone(), effect).is_none())?;
    }
    need(cics_count > 0)?;
    let owner = owner.ok_or_else(refused)?;
    let mut results = BTreeMap::<u64, CallImage>::new();
    let mut nested_commits = 0;
    for (key, row) in after_ims
        .iter()
        .filter(|(key, _)| !before_ims.contains_key(*key))
    {
        let effect = store
            .effect(&IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?)
            .map_err(|_| refused())?;
        let descriptor = describe_ims_replay_row(row, effect.as_ref(), ImsLimits::default())
            .map_err(|_| refused())?;
        need(
            descriptor.retention == ImsReplayRetentionState::Terminal
                && descriptor.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                && descriptor.owner_run_unit.as_deref() == Some(run.as_str()),
        )?;
        // Decode only after the provider's complete strict result/retention codec validation.
        let payload: serde_json::Value =
            serde_json::from_slice(&row.payload).map_err(|_| refused())?;
        let value = &payload["value"];
        need(
            value["affected_segments"] == 0
                && value["checkpoint_id"].is_null()
                && value["system"].is_null()
                && value["gsam"].is_null()
                && value["pcb_feedback_v1"].is_null(),
        )?;
        let status = value["status"].as_str().ok_or_else(refused)?.to_string();
        let segments: Vec<SegmentImage> =
            serde_json::from_value(value["segments"].clone()).map_err(|_| refused())?;
        match descriptor.dependency {
            Some(ImsReplayDependency::CoreEffect) => {
                let effect = effect.ok_or_else(refused)?;
                need(
                    descriptor.sequence == Some(effect.sequence)
                        && check_actor(store, &effect, record, &run, "host.ims.write")? == owner,
                )?;
                need(
                    results
                        .insert(effect.sequence, (status, segments))
                        .is_none(),
                )?;
            }
            Some(ImsReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key,
            }) => {
                // The source's SYNCPOINT has a nested IMS commit, not an invented same-key core.
                need(
                    effect.is_none()
                        && run_unit == run.as_str()
                        && descriptor.sequence == Some(sequence)
                        && descriptor.outer_effect_key.as_deref()
                            == Some(outer_effect_key.as_str())
                        && status == "  "
                        && segments.is_empty()
                        && value["affected_segments"] == 0,
                )?;
                let outer = cics_effects.get(&outer_effect_key).ok_or_else(refused)?;
                need(check_actor(store, outer, record, &run, "host.cics.execute")? == owner)?;
                let commit = HostRequest::Ims(ImsRequest {
                    operation: ImsOperation::Commit,
                    psb: None,
                    pcb: 1,
                    segments: Vec::new(),
                    data: Vec::new(),
                    qualifiers: Vec::new(),
                    checkpoint_id: None,
                    max_segments: 1,
                    mutation: Some(Mutation {
                        sequence,
                        idempotency_key: IdempotencyKey::new(key, InvocationLimits::default())
                            .map_err(|_| refused())?,
                        transaction: Some("CPVS".into()),
                    }),
                    system: None,
                    q_class: None,
                });
                need(
                    mainframe_env_host_api::canonical_request_digest(&commit)
                        .map_err(|_| refused())?
                        == descriptor.request_digest,
                )?;
                let resource = mainframe_env_host_api::canonical_audit_resource_digest(&commit);
                let invocation = outer
                    .intent
                    .audit_invocation_key
                    .as_ref()
                    .ok_or_else(refused)?;
                let audits = store
                    .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
                    .map_err(|_| refused())?;
                need(audits.len() <= MAX_ROWS)?;
                let audits = audits
                    .iter()
                    .filter(|audit| {
                        audit.effect_sequence == sequence
                            && audit.capability.as_str() == "host.ims.write"
                    })
                    .collect::<Vec<_>>();
                need(
                    matches!(audits.as_slice(),[audit] if audit.execution_id==owner.execution_id
                    && audit.run_unit_id==run && audit.principal==owner.principal && audit.attempt==owner.attempt
                    && audit.invocation_key==*invocation && audit.resource==resource
                    && audit.decision==AuditDecision::Success),
                )?;
                let uow = store
                    .get_provider_state("cics-uow", &outer_effect_key)
                    .map_err(|_| refused())?
                    .ok_or_else(refused)?;
                let undo = store
                    .get_provider_state("cics-uow-undo", run.as_str())
                    .map_err(|_| refused())?;
                let uow = describe_cics_uow_row(&uow, undo.as_ref()).map_err(|_| refused())?;
                need(
                    uow.state == CicsUowState::Committed
                        && uow.dependency == CicsUowDependencyState::Clear
                        && uow.transaction == "CPVS"
                        && uow.effect_key.as_deref() == Some(outer_effect_key.as_str())
                        && uow.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                        && uow.owner_run_unit.as_deref() == Some(run.as_str()),
                )?;
                nested_commits += 1;
            }
            None => return Err(refused()),
        }
    }
    need(nested_commits == 1)?;
    oracle.compare_results(&results.into_values().collect::<Vec<_>>())?;
    Ok(format!(
        "{:x}",
        Sha256::digest(owner.execution_id.as_str().as_bytes())
    ))
}
fn field_metadata(terminal: &serde_json::Value) -> serde_json::Value {
    match online_screen_fields(terminal) {
        Ok(fields) => serde_json::json!({"mapset_matches":terminal["mapset"]=="COPAU00",
            "map_matches":terminal["map"]=="COPAU0A", "field_count":fields.len(),
            "fields":fields.into_iter().filter(|(name,_)| oracle::named_field(name)).map(|(name,bytes)| serde_json::json!({"name":name,
                "bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})).collect::<Vec<_>>() }),
        Err(problem) => serde_json::json!({"screen_decode_problem_code":problem.code}),
    }
}
fn host_problem_classification(message: &str) -> Option<&'static str> {
    let problem = message.strip_prefix("host service failed: ")?;
    match problem {
        "Malformed" => Some("Malformed"),
        "Unsupported" => Some("Unsupported"),
        "NotFound" => Some("NotFound"),
        "Unauthorized" => Some("Unauthorized"),
        "Cancelled" => Some("Cancelled"),
        "TimedOut" => Some("TimedOut"),
        "ResourceExhausted" => Some("ResourceExhausted"),
        "ProviderFailure" => Some("ProviderFailure"),
        "InfrastructureFailure" => Some("InfrastructureFailure"),
        "MissingIdempotency" => Some("MissingIdempotency"),
        "IdempotencyConflict" => Some("IdempotencyConflict"),
        "UnknownOutcome" => Some("UnknownOutcome"),
        value
            if value.starts_with("UnsupportedCapability { capability: ")
                && value.ends_with(" }") =>
        {
            Some("UnsupportedCapability")
        }
        value if value.starts_with("Condition { name: ") && value.ends_with(" }") => {
            Some("Condition")
        }
        _ => None,
    }
}
fn public_unsupported_capability(message: &str) -> Option<&str> {
    let value =
        message.strip_prefix("host service failed: UnsupportedCapability { capability: \"")?;
    let (capability, detail) = value.split_once("\", detail: ")?;
    if capability.is_empty()
        || capability.len() > 128
        || !capability.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':' | b'@')
        })
        || !detail.starts_with('"')
        || !detail.ends_with("\" }")
    {
        return None;
    }
    Some(capability)
}
fn debug_optional_text(value: &str) -> Option<Option<String>> {
    if value == "None" {
        return Some(None);
    }
    let encoded = value.strip_prefix("Some(")?.strip_suffix(')')?;
    let decoded = serde_json::from_str::<String>(encoded).ok()?;
    (format!("{decoded:?}") == encoded).then_some(Some(decoded))
}
fn public_machine_position(position: &str) -> Option<(u64, String)> {
    let (counter, rest) = position.split_once(' ')?;
    let pc = counter.parse::<u64>().ok()?;
    if pc.to_string() != counter {
        return None;
    }
    let (opcode, arguments) = rest.split_once(' ')?;
    if !mainframe_env_compiler::core_mir_catalog()
        .identities()
        .any(|operation| operation.name() == opcode)
    {
        return None;
    }
    let mut decoder = serde_json::Deserializer::from_str(arguments).into_iter::<Vec<String>>();
    let decoded = decoder.next()?.ok()?;
    let offset = decoder.byte_offset();
    if format!("{decoded:?}") != arguments[..offset] {
        return None;
    }
    let remaining = arguments[offset..].strip_prefix(" role=")?;
    let (role, remaining) = remaining.split_once(" scope=")?;
    let (scope, text) = remaining.split_once(" text=")?;
    let role = debug_optional_text(role)?;
    let scope = debug_optional_text(scope)?;
    debug_optional_text(text)?;
    if !matches!(
        role.as_deref(),
        None | Some(
            "statement"
                | "block_start"
                | "branch"
                | "block_end"
                | "label"
                | "external_target"
                | "recovered"
                | "transfer"
                | "terminator"
        )
    ) || !matches!(
        scope.as_deref(),
        None | Some("" | "if" | "evaluate" | "search" | "perform")
    ) {
        return None;
    }
    Some((pc, opcode.to_string()))
}
fn condition_problem_metadata(message: &str) -> Option<serde_json::Value> {
    let value = message
        .strip_prefix("host service failed: Condition { name: ")?
        .strip_suffix(" }")?;
    let (value, response2) = value.rsplit_once(", response2: ")?;
    let (encoded_name, response) = value.rsplit_once(", response: ")?;
    let primary = response.parse::<i32>().ok()?;
    let secondary = response2.parse::<i32>().ok()?;
    if primary.to_string() != response || secondary.to_string() != response2 {
        return None;
    }
    let name = serde_json::from_str::<String>(encoded_name).ok()?;
    if format!("{name:?}") != encoded_name {
        return None;
    }
    let mut positions = name
        .match_indices(" at pc=")
        .filter_map(|(offset, marker)| {
            public_machine_position(&name[offset + marker.len()..])
                .map(|(pc, opcode)| (offset, pc, opcode))
        })
        .collect::<Vec<_>>();
    let position = if positions.len() == 1 {
        positions.pop()
    } else {
        None
    };
    let public_message = position
        .as_ref()
        .map_or(name.as_str(), |(offset, _, _)| &name[..*offset]);
    let classification = match public_message {
        "invalid executable artifact or data" => Some("invalid_executable_artifact_or_data"),
        "unsupported COBOL form" => Some("unsupported_cobol_form"),
        "machine resource exhausted" => Some("machine_resource_exhausted"),
        "host authorization denied" => Some("host_authorization_denied"),
        "host cancelled" => Some("host_cancelled"),
        "host timed out" => Some("host_timed_out"),
        "host outcome unknown" => Some("host_outcome_unknown"),
        _ => None,
    };
    Some(serde_json::json!({
        "response": primary, "response2": secondary,
        "name_bytes": name.len(), "name_sha256": format!("{:x}", Sha256::digest(name.as_bytes())),
        "machine_position": position.map(|(_, pc, opcode)| serde_json::json!({"pc": pc, "opcode": opcode})),
        "public_execution_problem": classification
    }))
}
fn terminal_http_problem_metadata(problem: &CorpusProblem) -> Option<serde_json::Value> {
    if problem.code != "carddemo.online.exchange_failed" {
        return None;
    }
    let (phase, detail) =
        if let Some(detail) = problem.detail.strip_prefix("terminal input returned ") {
            ("input", detail)
        } else {
            (
                "resume",
                problem.detail.strip_prefix("terminal exchange returned ")?,
            )
        };
    let (status_text, body) = detail.split_once(": ")?;
    let status = status_text.split_once(' ')?.0.parse::<u16>().ok()?;
    if StatusCode::from_u16(status).ok()?.to_string() != status_text {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let object = value.as_object()?;
    if object.len() != 3 || value["status"].as_u64()? != u64::from(status) {
        return None;
    }
    let category = value["category"].as_str()?;
    let message = value["message"].as_str()?;
    if category.is_empty()
        || category.len() > 64
        || !category
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    Some(serde_json::json!({
        "phase": phase, "status": status, "category": category,
        "message_bytes": message.len(),
        "message_sha256": format!("{:x}", Sha256::digest(message.as_bytes())),
        "host_problem_classification": host_problem_classification(message),
        "unsupported_capability": public_unsupported_capability(message),
        "condition": condition_problem_metadata(message)
    }))
}
fn problem_metadata(problem: &CorpusProblem) -> serde_json::Value {
    let mut metadata = serde_json::json!({"code": problem.code,
        "detail_bytes": problem.detail.len(),
        "detail_sha256": format!("{:x}", Sha256::digest(problem.detail.as_bytes()))});
    if let Some(transport) = terminal_http_problem_metadata(problem) {
        metadata["terminal_http"] = transport;
    }
    metadata
}
fn launch_metadata(status: StatusCode, body: &[u8]) -> serde_json::Value {
    let category = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["category"].as_str().map(str::to_string));
    serde_json::json!({"status": status.as_u16(), "body_bytes": body.len(),
        "body_sha256": format!("{:x}", Sha256::digest(body)), "category": category})
}
async fn exercise(
    corpus: &Path,
    oracle: &Oracle,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<(), CorpusProblem> {
    owned.check()?;
    sources(corpus)?;
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: owned.artifacts.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    need(config.cobol_current_date.is_none())?;
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|_| refused())?;
    let app = server.router();
    let mut retained = None;
    let probe = async {
        owned.check()?;
        let (online, record, source_identity) = definition(corpus)?;
        receipt["stage"] = serde_json::json!("source-publisher-install");
        owned.save(receipt)?;
        install_base_online_authorities(&server, corpus, &online)?;
        server
            .bootstrap_user("IBMUSER", b"TESTPASS")
            .map_err(|_| refused())?;
        let racf = server.racf_service();
        for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
            racf.define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
                .map_err(|_| refused())?;
            for principal in ["IBMUSER", "WEBADM"] {
                racf.permit(class, name, principal, AccessIntent::Control)
                    .map_err(|_| refused())?;
            }
        }
        // The readonly guest SYNCPOINT still authorizes the generic IMS UOW.
        // Preserve the seed actor when this exact profile shadows its wildcard.
        racf.define_profile("IMSUOW", "CURRENT", "IBMUSER", None)
            .map_err(|_| refused())?;
        for principal in ["IBMUSER", "WEBADM"] {
            racf.permit("IMSUOW", "CURRENT", principal, AccessIntent::Update)
                .map_err(|_| refused())?;
        }
        for (principal, resource, intent, expected) in [
            (
                "IBMUSER",
                "CURRENT",
                AccessIntent::Update,
                SecurityDecision::Allow,
            ),
            (
                "WEBADM",
                "CURRENT",
                AccessIntent::Update,
                SecurityDecision::Allow,
            ),
            (
                "WEBUSER",
                "CURRENT",
                AccessIntent::Update,
                SecurityDecision::Deny,
            ),
            (
                "WEBADM",
                "OTHER",
                AccessIntent::Update,
                SecurityDecision::Deny,
            ),
            (
                "WEBADM",
                "CURRENT",
                AccessIntent::Control,
                SecurityDecision::Deny,
            ),
        ] {
            let principal =
                PrincipalId::new(principal, InvocationLimits::default()).map_err(|_| refused())?;
            let resource = ResourceName::new(
                resource,
                mainframe_env_host_api::HostLimits::default().max_name_bytes,
            )
            .map_err(|_| refused())?;
            need(
                racf.authorize(&principal, "IMSUOW", &resource, intent)
                    .map_err(|_| refused())?
                    == expected,
            )?;
        }
        owned.check()?;
        server
            .install_online_application(online)
            .map_err(|_| refused())?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        let ims = server.ims_service();
        ims.install_metadata(ims_packages::metadata(corpus)?)
            .map_err(|_| refused())?;
        let seed = ims_invocation("firstparty-readonly-seed", true, None)?;
        let load = ims_request(
            ImsOperation::Load,
            90_001,
            None,
            &[],
            serde_json::to_vec(&oracle.image()).map_err(|_| refused())?,
            Vec::new(),
            None,
        )?;
        let result = ims.execute(&seed, &load).map_err(|_| refused())?;
        need(result.status == "  " && result.affected_segments == 6)?;
        let result = ims
            .execute(
                &seed,
                &ims_request(
                    ImsOperation::Commit,
                    90_002,
                    None,
                    &[],
                    Vec::new(),
                    Vec::new(),
                    None,
                )?,
            )
            .map_err(|_| refused())?;
        need(result.status == "  ")?;
        hierarchy(&ims, oracle)?;
        dataset_images(&server, corpus)?;
        receipt["source_identity"] = serde_json::json!(source_identity);
        receipt["artifact"] = serde_json::json!(record.artifact.as_str());
        receipt["artifact_payload_sha256"] =
            serde_json::json!(format!("{:x}", Sha256::digest(&record.payload)));
        receipt["stage"] = serde_json::json!("installed-cpvs-before-enter");
        owned.save(receipt)?;
        receipt["probe_step"] = serde_json::json!("launch-http");
        let mut headers = BTreeMap::from([
            (
                "authorization".into(),
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD
                        .encode("WEBADM:admin-transport-password")
                ),
            ),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]);
        let launch = terminal_http(
            &app,
            Method::POST,
            "/mainframe-env/cics/v1/sessions",
            headers.clone(),
            br#"{"transaction":"CPVS"}"#.to_vec(),
        )
        .await
        .map_err(|problem| {
            receipt["probe_transport_problem"] = problem_metadata(&problem);
            refused()
        })?;
        receipt["launch_response_metadata"] = launch_metadata(launch.0, &launch.1);
        receipt["probe_step"] = serde_json::json!("launch-http-status");
        need(launch.0 == StatusCode::CREATED)?;
        receipt["probe_step"] = serde_json::json!("launch-response-json");
        let initial: serde_json::Value =
            serde_json::from_slice(&launch.1).map_err(|_| refused())?;
        receipt["probe_step"] = serde_json::json!("launch-session-id");
        let session = initial["session"].as_str().ok_or_else(refused)?.to_string();
        let initial_terminal = &initial["terminal"];
        receipt["initial_terminal_metadata"] = serde_json::json!({
            "schema_matches": initial_terminal["schema_version"] == "mainframe-env.cics-terminal@1",
            "session_matches": initial_terminal["session"] == session,
            "principal_matches": initial_terminal["principal"] == "WEBADM",
            "transaction_matches": initial_terminal["transaction"] == "CPVS",
            "connected": initial_terminal["connected"].as_bool(),
            "suspended": initial_terminal["suspended"].as_bool(),
            "mapset_matches": initial_terminal["mapset"] == "COPAU00",
            "map_matches": initial_terminal["map"] == "COPAU0A"
        });
        receipt["probe_step"] = serde_json::json!("initial-terminal-contract");
        need(
            initial_terminal["schema_version"] == "mainframe-env.cics-terminal@1"
                && initial_terminal["session"] == session
                && initial_terminal["principal"] == "WEBADM"
                && initial_terminal["transaction"] == "CPVS"
                && initial_terminal["connected"] == true
                && initial_terminal["suspended"] == false
                && initial_terminal["mapset"] == "COPAU00"
                && initial_terminal["map"] == "COPAU0A",
        )?;
        receipt["probe_step"] = serde_json::json!("launch-csrf-token");
        headers.insert(
            "x-csrf-token".into(),
            initial["csrf_token"]
                .as_str()
                .ok_or_else(refused)?
                .to_string(),
        );
        receipt["probe_step"] = serde_json::json!("before-enter-replay-rows");
        let before_cics = rows(store.as_ref(), "cics-effect-replay-v1")?;
        let before_ims = rows(store.as_ref(), "ims-v1-replay")?;
        receipt["probe_step"] = serde_json::json!("before-enter-trace");
        let trace_before = server.online_trace(&session).map_err(|_| refused())?;
        need(trace_before.len() <= MAX_ROWS)?;
        receipt["probe_step"] = serde_json::json!("terminal-exchange");
        let terminal = carddemo_terminal_exchange(
            &app,
            &session,
            &headers,
            0x7d,
            BTreeMap::from([("ACCTID".into(), oracle.account.clone())]),
        )
        .await
        .map_err(|problem| {
            receipt["probe_transport_problem"] = problem_metadata(&problem);
            refused()
        })?;
        receipt["probe_step"] = serde_json::json!("guest-observation");
        receipt["terminal_failure_metadata"] = field_metadata(&terminal);
        owned.save(receipt)?;
        let owner = observe_guest(
            store.as_ref(),
            &record,
            &terminal,
            &session,
            &before_cics,
            &before_ims,
            oracle,
        )?;
        let trace = server.online_trace(&session).map_err(|_| refused())?;
        need(trace.len() <= MAX_ROWS && trace.starts_with(&trace_before))?;
        let reads = trace[trace_before.len()..]
            .iter()
            .filter(|entry| entry.operation == CicsOperation::Read)
            .collect::<Vec<_>>();
        need(
            reads.len() == 3
                && reads.iter().all(|entry| {
                    entry.outcome == "NORMAL" && entry.response == 0 && entry.response2 == 0
                }),
        )?;
        hierarchy(&ims, oracle)?;
        dataset_images(&server, corpus)?;
        sources(corpus)?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        receipt["actual_execution_sha256"] = serde_json::json!(owner);
        receipt["actual_ims_calls"] = serde_json::json!(8);
        receipt["actual_nested_syncpoint_commits"] = serde_json::json!(1);
        receipt["fixed_named_fields_compared"] = serde_json::json!(62);
        receipt["whole_seed_hierarchy_compared"] = serde_json::json!(true);
        receipt["actual_installed_guest_compared"] = serde_json::json!(true);
        // Technical failure-only hashes are unnecessary after the exact owner comparison succeeds.
        receipt["terminal_failure_metadata"] = serde_json::Value::Null;
        let uri = format!("/mainframe-env/cics/v1/sessions/{session}");
        need(
            terminal_http(&app, Method::DELETE, &uri, headers.clone(), Vec::new())
                .await
                .map_err(|_| refused())?
                .0
                == StatusCode::NO_CONTENT,
        )?;
        need(
            terminal_http(&app, Method::GET, &uri, headers, Vec::new())
                .await
                .map_err(|_| refused())?
                .0
                == StatusCode::NOT_FOUND,
        )?;
        receipt["session_disconnect_checked"] = serde_json::json!(true);
        retained = Some(record);
        receipt["stage"] = serde_json::json!("before-first-drain");
        Ok::<_, CorpusProblem>(())
    }
    .await;
    if let Err(problem) = &probe {
        receipt["probe_failure_step"] = receipt["probe_step"].clone();
        receipt["probe_problem_metadata"] = problem_metadata(problem);
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let before_save = owned.save(receipt);
    drop(app);
    let drained = server.graceful_shutdown().await;
    receipt["first_drain"] = serde_json::json!(drained);
    drop(server);
    let after_save = owned.save(receipt);
    before_save?;
    after_save?;
    need(drained)?;
    owned.check()?;
    receipt["stage"] = serde_json::json!("same-memory-store-reopen");
    owned.save(receipt)?;
    let reopened = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        secrets,
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|_| refused())?;
    let reopening = (|| {
        owned.check()?;
        sources(corpus)?;
        installed(
            store.as_ref(),
            &owned.artifacts,
            retained.as_ref().ok_or_else(refused)?,
        )?;
        hierarchy(&reopened.ims_service(), oracle)?;
        dataset_images(&reopened, corpus)?;
        receipt["reopen_compared"] = serde_json::json!(true);
        Ok::<_, CorpusProblem>(())
    })();
    if let Err(problem) = &reopening {
        receipt["reopening_problem_metadata"] = problem_metadata(problem);
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let before_save = owned.save(receipt);
    let drained = reopened.graceful_shutdown().await;
    receipt["second_drain"] = serde_json::json!(drained);
    drop(reopened);
    let after_save = owned.save(receipt);
    probe?;
    reopening?;
    before_save?;
    after_save?;
    need(drained)?;
    owned.check()?;
    receipt["stage"] = serde_json::json!("two-checked-drains-before-disposal");
    owned.save(receipt)?;
    owned.check()?;
    fs::remove_dir_all(&owned.artifacts).map_err(|_| refused())?;
    match fs::symlink_metadata(&owned.artifacts) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(refused()),
    }
    owned.artifacts_removed = true;
    owned.check()?;
    receipt["cleanup_verified"] = serde_json::json!(true);
    receipt["stage"] = serde_json::json!("local-installed-firstparty-readonly-owner-pass");
    owned.save(receipt)
}

#[test]
#[ignore = "real installed COPAUS0C readonly owner; exact pinned corpus/private oracle and fresh external evidence; no J15–18/vendor/hold/rawCALL credit"]
fn pinned_installed_copaus0_readonly_guest_compares_complete_seed_and_named_map() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let corpus = verify_carddemo_corpus_from_env(
        &repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    sources(&corpus_dir).unwrap();
    let offered_oracle = PathBuf::from(
        env::var_os("CARDDEMO_IMS_READONLY_ORACLE")
            .expect("exact external private oracle required"),
    );
    let oracle =
        Oracle::read(&offered_oracle).expect("independent private oracle admission refused");
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_IMS_READONLY_EVIDENCE").expect("fresh external evidence required"),
    );
    let parent = offered.parent().unwrap().canonicalize().unwrap();
    assert!(
        !parent.starts_with(repository.canonicalize().unwrap()),
        "evidence stays outside Git"
    );
    let evidence = parent.join(offered.file_name().unwrap());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .expect("exclusive evidence required");
    let metadata = fs::symlink_metadata(&evidence).unwrap();
    let identity = (metadata.dev(), metadata.ino());
    let artifacts = evidence.join("owned-artifacts");
    require_owned_directory(&evidence, identity).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .unwrap();
    let metadata = fs::symlink_metadata(&artifacts).unwrap();
    let artifact_identity = (metadata.dev(), metadata.ino());
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, artifact_identity).unwrap();
    let mut receipt = serde_json::json!({"schema":"external.carddemo-installed-copaus0-readonly-owner@1",
        "corpus_commit":corpus.commit,"backend":"memory","physical_store_restart":false,
        "source_program":"COPAUS0C","transaction":"CPVS","stage":"before-open",
        "first_drain":false,"second_drain":false,"reopen_compared":false,"cleanup_verified":false,
        "session_disconnect_checked":false,"actual_installed_guest_compared":false,"actual_ims_calls":0,
        "whole_seed_hierarchy_compared":false,"fixed_named_fields_compared":0,
        "root_seed_bytes":100,"child_seed_bytes":200,"child_seed_count":5,
        "whole_private_guest_storage_postimage_claimed":false,"raw_storage_retained":false,
        "original_j15_j16_j17_j18_criteria_unchanged":true,"application_credit":0,"vendor_credit":0,
        "acceptance_credit":0,"artifact_root":artifacts,"artifact_identity":artifact_identity,
        "controls":"default config/limits/router/date; original public HTTP and process bounds"});
    owned.save(&receipt).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(exercise(&corpus_dir, &oracle, &mut owned, &mut receipt));
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    owned.save(&receipt).unwrap();
    assert!(
        result.is_ok(),
        "real readonly owner refused; metadata retained at {}",
        evidence.display()
    );
    assert_eq!(receipt["cleanup_verified"], true);
}
