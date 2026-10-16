//! Real source-bound COPAUS0C navigation and paging; two individual J17 predicates.
use crate::carddemo::ims_firstparty_readonly::{
    dataset_images, definition, installed, rows, sources,
};
use crate::carddemo::owned_source_receipt::{ReceiptOwner, require_owned_directory};
use crate::carddemo::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsUowDependencyState,
    CicsUowState, describe_cics_replay_row, describe_cics_uow_row,
};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::HostRequest;
use mainframe_env_ims::{ImsReplayDependency, ImsReplayRetentionState, describe_ims_replay_row};
use mainframe_env_store_api::{ArtifactRecord, PlatformStore, ProviderStateRecord};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
#[path = "ims_firstparty_paging_observer.rs"]
mod observer;
#[path = "ims_firstparty_paging_oracle.rs"]
mod oracle;
use oracle::{PagingOracle, SegmentImage};
const MAX_ROWS: usize = 512;

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.ims.firstparty_paging_drift",
        "actual source, AID/field/request/actor comparisons or owned teardown differs",
    )
}
fn need(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(refused()) }
}
struct PagingCompleted {
    _private: (),
}

fn hierarchy(service: &ImsService, oracle: &PagingOracle) -> Result<(), CorpusProblem> {
    let invocation = ims_invocation("firstparty-paging-observe", true, None)?;
    let request = ims_request(
        ImsOperation::Unload,
        90_003,
        Some("DBPAUTP0"),
        &[],
        vec![],
        vec![],
        None,
    )?;
    let result = service
        .execute(&invocation, &request)
        .map_err(|_| refused())?;
    need(result.status == "  ")?;
    oracle.compare_hierarchy(
        &result
            .segments
            .into_iter()
            .map(|segment| (segment.name, segment.parent_key, segment.data))
            .collect::<Vec<_>>(),
    )
}

struct PagingRun<'a> {
    server: &'a ProductServer,
    app: &'a axum::Router,
    store: &'a dyn PlatformStore,
    record: &'a ArtifactRecord,
    session: &'a str,
    headers: &'a BTreeMap<String, String>,
    corpus: &'a Path,
    oracle: &'a PagingOracle,
}
async fn run_frames(
    run: PagingRun<'_>,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<(), CorpusProblem> {
    let PagingRun {
        server,
        app,
        store,
        record,
        session,
        headers,
        corpus,
        oracle,
    } = run;
    let mut owners = Vec::new();
    for (index, frame) in oracle.frames.iter().enumerate() {
        owned.check()?;
        sources(corpus)?;
        installed(store, &owned.artifacts, record)?;
        let before_cics = rows(store, "cics-effect-replay-v1")?;
        let before_ims = rows(store, "ims-v1-replay")?;
        let trace_before = server.online_trace(session).map_err(|_| refused())?;
        need(trace_before.len() <= MAX_ROWS)?;
        receipt["probe_step"] = serde_json::json!(frame.name);
        owned.save(receipt)?;
        let terminal =
            carddemo_terminal_exchange(app, session, headers, frame.aid, frame.inputs.clone())
                .await?;
        receipt["terminal_failure_metadata"] = field_metadata(&terminal);
        owned.save(receipt)?;
        let owner = observer::observe_page(
            observer::PageContext {
                store,
                record,
                terminal: &terminal,
                session,
                before_cics: &before_cics,
                before_ims: &before_ims,
                receipt,
            },
            oracle,
            index,
        )?;
        receipt["terminal_failure_metadata"] = serde_json::Value::Null;
        let trace = server.online_trace(session).map_err(|_| refused())?;
        need(trace.len() <= MAX_ROWS && trace.starts_with(&trace_before))?;
        let reads = trace[trace_before.len()..]
            .iter()
            .filter(|entry| entry.operation == CicsOperation::Read)
            .collect::<Vec<_>>();
        need(
            reads.len() == frame.dataset_reads
                && reads.iter().all(|entry| {
                    entry.outcome == "NORMAL" && entry.response == 0 && entry.response2 == 0
                }),
        )?;
        hierarchy(&server.ims_service(), oracle)?;
        dataset_images(server, corpus)?;
        owners.push(owner);
        receipt["completed_frames"] = serde_json::json!(index + 1);
        receipt["actual_execution_sha256s"] = serde_json::json!(owners);
        owned.save(receipt)?;
    }
    need(owners.len() == 5)
}

fn field_metadata(terminal: &serde_json::Value) -> serde_json::Value {
    match online_screen_fields(terminal) {
        Ok(fields) => serde_json::json!({"mapset_matches":terminal["mapset"]=="COPAU00",
            "map_matches":terminal["map"]=="COPAU0A", "field_count":fields.len(),
            "fields":fields.into_iter().map(|(name,bytes)| serde_json::json!({"name":name,
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
    oracle: &PagingOracle,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<PagingCompleted, CorpusProblem> {
    owned.check()?;
    sources(corpus)?;
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: owned.artifacts.clone(),
        cobol_current_date: Some(oracle.clock.clone()),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    need(config.cobol_current_date.as_deref() == Some("2022070600000000+0000"))?;
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
        let seed = ims_invocation("firstparty-paging-seed", true, None)?;
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
        need(result.status == "  " && result.affected_segments == 8)?;
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
        run_frames(
            PagingRun {
                server: &server,
                app: &app,
                store: store.as_ref(),
                record: &record,
                session: &session,
                headers: &headers,
                corpus,
                oracle,
            },
            owned,
            receipt,
        )
        .await?;
        hierarchy(&ims, oracle)?;
        dataset_images(&server, corpus)?;
        sources(corpus)?;
        installed(store.as_ref(), &owned.artifacts, &record)?;
        receipt["actual_ims_calls"] = serde_json::json!(26);
        receipt["actual_nested_syncpoint_commits"] = serde_json::json!(4);
        receipt["fixed_named_fields_compared_per_frame"] = serde_json::json!(62);
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
    receipt["stage"] = serde_json::json!("local-installed-firstparty-paging-owner-pass");
    owned.save(receipt)?;
    Ok(PagingCompleted { _private: () })
}

pub(super) fn verify_from_env(inventory_path: &Path) -> Result<RouteObservations, CorpusProblem> {
    let corpus = verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(refused)?);
    sources(&corpus_dir)?;
    let oracle_path = PathBuf::from(env::var_os("CARDDEMO_IMS_PAGING_ORACLE").ok_or_else(refused)?);
    let oracle = PagingOracle::read(&oracle_path)?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|_| refused())?;
    let parent = env::temp_dir().canonicalize().map_err(|_| refused())?;
    need(!parent.starts_with(&repository))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_nanos();
    let evidence = parent.join(format!(
        "mainframe-env-carddemo-ims-paging-{}-{nonce}",
        std::process::id()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .map_err(|_| refused())?;
    let m = fs::symlink_metadata(&evidence).map_err(|_| refused())?;
    let identity = (m.dev(), m.ino());
    require_owned_directory(&evidence, identity)?;
    let artifacts = evidence.join("owned-artifacts");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .map_err(|_| refused())?;
    let m = fs::symlink_metadata(&artifacts).map_err(|_| refused())?;
    let artifact_identity = (m.dev(), m.ino());
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, artifact_identity)?;
    let mut receipt = serde_json::json!({"schema":"mainframe-env.carddemo-ims-paging-observation@1",
        "corpus_commit":corpus.commit,"backend":"memory","physical_store_restart":false,
        "source_program":"COPAUS0C","transaction":"CPVS","stage":"before-open",
        "first_drain":false,"second_drain":false,"reopen_compared":false,"cleanup_verified":false,
        "session_disconnect_checked":false,"completed_frames":0,"actual_ims_calls":0,
        "whole_seed_hierarchy_compared":false,"root_seed_bytes":100,"child_seed_bytes":200,
        "child_seed_count":7,"whole_private_guest_storage_postimage_claimed":false,
        "bms_attribute_or_mdt_claimed":false,"application_credit":0,"vendor_credit":0,
        "acceptance_credit":0,"artifact_root":artifacts,"artifact_identity":artifact_identity});
    owned.save(&receipt)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| refused())?;
    let result = runtime.block_on(exercise(&corpus_dir, &oracle, &mut owned, &mut receipt));
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    let saved = owned.save(&receipt);
    let _completed = result?;
    saved?;
    let mut observed = RouteObservations::default();
    for requirement in ["IMS GU/GNP navigation", "BMS paging"] {
        observed.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J17",
            requirement,
            false,
            || Ok(refused()),
        )?;
    }
    Ok(observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "real installed COPAUS0C seven-child paging; exact new independent source-authored oracle; no complete journey/issue/vendor credit"]
    fn pinned_installed_paging_compares_enter_top_forward_bottom_and_back_before_observation() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let observed = verify_from_env(
            &repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
        )
        .expect("source, independent images, actual AIDs/requests and owned teardown required");
        assert_eq!(
            observed.journeys(),
            &[(
                "CD.J17".into(),
                vec!["IMS GU/GNP navigation".into(), "BMS paging".into()]
            )]
        );
        assert!(observed.issues().is_empty());
    }
}
