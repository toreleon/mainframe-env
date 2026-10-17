//! Actual installed account plain READ consumption across two exited processes.
//! This Running pre-checkpoint cut is not the separate Suspended CD.J09 predicate.
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_store_api::*;
use std::collections::BTreeSet;
use std::sync::Mutex;
#[path = "terminal_account_read_store.rs"]
mod observed;
#[path = "terminal_account_read_protocol.rs"]
mod protocol;
use observed::ObservedStore;
#[path = "terminal_account_read_http.rs"]
mod http_transport;
use http_transport::Http;
#[path = "terminal_account_read_deadline_tests.rs"]
mod deadline;

fn marker(root: &Path) -> Result<Marker, CorpusProblem> {
    let bytes = protocol::read_private(&root.join("owner.json"), MAX_SECRET).map_err(io_problem)?;
    let value: Marker = decode(&bytes)?;
    let executable = env::current_exe().map_err(io_problem)?;
    protocol::require_root_identity(root, (value.root_device, value.root_inode))
        .map_err(io_problem)?;
    require(
        fs::canonicalize(root).map_err(io_problem)? == root
            && root.to_str() == Some(value.root.as_str())
            && value.nonce.len() == 64
            && value.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            && value.parent_pid > 0
            && value.parent_pid != std::process::id()
            && executable.to_str() == Some(value.executable.as_str())
            && protocol::executable_digest(&executable).map_err(io_problem)?
                == value.executable_digest
            && value.corpus_commit == CORPUS_COMMIT
            && value.corpus_content == CORPUS_CONTENT
            && env::var(protocol::NONCE_ENV).ok().as_deref() == Some(value.nonce.as_str()),
    )?;
    Ok(value)
}

fn inventory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json")
}

fn verify_corpus(corpus: &Path) -> Result<(), CorpusProblem> {
    let receipt = verify_carddemo_corpus(corpus, &inventory())?;
    require(receipt.commit == CORPUS_COMMIT && receipt.content.sha256 == CORPUS_CONTENT)?;
    source_artifact_binding::verify_pinned_sources(corpus)?;
    oracles::check_inputs(corpus)
}

fn route(handoff: &Handoff) -> Result<CardDemoOnlineSession, CorpusProblem> {
    require(
        handoff.session.len() <= 64
            && SessionId::new(&handoff.session, 64).is_ok()
            && handoff.bearer.starts_with("Bearer ")
            && handoff.bearer.len() <= 1024
            && !handoff.csrf.is_empty()
            && handoff.csrf.len() <= 256,
    )?;
    Ok(CardDemoOnlineSession {
        session: handoff.session.clone(),
        menu_response: serde_json::Value::Null, // Retained auth helper; never menu/source authority.
        headers: BTreeMap::from([
            ("authorization".into(), handoff.bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
            ("x-csrf-token".into(), handoff.csrf.clone()),
        ]),
    })
}

async fn denial_controls(
    http: &mut Http,
    route: &CardDemoOnlineSession,
) -> Result<(), CorpusProblem> {
    let endpoint = format!("/mainframe-env/cics/v1/sessions/{}", route.session);
    let (status, _) = http
        .call(Method::GET, &endpoint, BTreeMap::new(), Vec::new())
        .await?;
    require(status == StatusCode::UNAUTHORIZED)?;
    let mut wrong = route.headers.clone();
    wrong.insert("x-csrf-token".into(), "wrong-source-session-token".into());
    let (status, _) = http
        .call(
            Method::PUT,
            &format!("{endpoint}/input"),
            wrong,
            br#"{"aid":248,"fields":{}}"#.to_vec(),
        )
        .await?;
    require(status == StatusCode::FORBIDDEN)?;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(b"WEBADM:admin-transport-password")
    );
    let (status, _) = http
        .call(
            Method::GET,
            &endpoint,
            BTreeMap::from([("authorization".into(), basic)]),
            Vec::new(),
        )
        .await?;
    require(status == StatusCode::FORBIDDEN)
}

fn cleanup(
    root: &Path,
    expected_marker: &[u8],
    owned_root: (u64, u64),
) -> Result<(), CorpusProblem> {
    protocol::require_root_identity(root, owned_root).map_err(io_problem)?;
    require(
        fs::canonicalize(root).map_err(io_problem)? == root
            && protocol::read_private(&root.join("owner.json"), MAX_SECRET).map_err(io_problem)?
                == expected_marker,
    )?;
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(io_problem)? {
            let entry = entry.map_err(io_problem)?;
            count += 1;
            require(count <= 16384)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(io_problem)?;
            require(
                !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()),
            )?;
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    // Recheck the same exclusive physical root after the bounded walk.
    protocol::require_root_identity(root, owned_root).map_err(io_problem)?;
    fs::remove_dir_all(root).map_err(io_problem)?;
    require(protocol::is_absent(root).map_err(io_problem)?)
}

const ORACLE_ENV: &str = "CARDDEMO_ACCOUNT_READ_ORACLE";
const ORACLE_SHA: &str = "3d908b507caa2126ba147a074d14ec7f5c137a8de0cde9e29c44bea63d82279a";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handoff {
    nonce: String,
    session: String,
    bearer: String,
    csrf: String,
    public_digest: String,
    artifact: String,
    dataset_digest: String,
    artifact_digest: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(value: &str) -> Result<Vec<u8>, CorpusProblem> {
    require(value.len() <= MAX_RECEIPT * 2 && value.len().is_multiple_of(2))?;
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 16).ok())
                .ok_or_else(problem)
        })
        .collect()
}
fn oracle() -> Result<serde_json::Value, CorpusProblem> {
    let path = PathBuf::from(env::var_os(ORACLE_ENV).ok_or_else(problem)?);
    let bytes = protocol::read_private(&path, MAX_RECEIPT).map_err(io_problem)?;
    require(evidence::hash(&bytes) == ORACLE_SHA)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| problem())?;
    require(value["corpus_commit"] == CORPUS_COMMIT && value["account_input"] == "00000000050")?;
    Ok(value)
}

struct ReplyReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> ReplyReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CorpusProblem> {
        let end = self.at.checked_add(n).ok_or_else(problem)?;
        let value = self.bytes.get(self.at..end).ok_or_else(problem)?;
        self.at = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, CorpusProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| problem())?,
        ))
    }
    fn u64(&mut self) -> Result<u64, CorpusProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| problem())?,
        ))
    }
    fn field(&mut self, max: usize) -> Result<&'a [u8], CorpusProblem> {
        let n = usize::try_from(self.u32()?).map_err(|_| problem())?;
        require(n <= max)?;
        self.take(n)
    }
}

// Independent bounded envelope reader; provider ownership/digests are also checked
// through its public descriptor. No private codec export or product-derived expected data.
fn reply_record(row: &ProviderStateRecord) -> Result<(String, Vec<u8>), CorpusProblem> {
    let mut r = ReplyReader {
        bytes: &row.payload,
        at: 0,
    };
    require(
        row.namespace == "cics-effect-replay-v1" && row.version == 2 && r.take(8)? == b"MECER004",
    )?;
    require(r.field(128)? == row.key.as_bytes())?;
    r.field(128)?;
    r.field(128)?;
    require(r.u64()? > 0 && r.u64()? > 0 && r.u64()? > 0)?;
    r.take(96)?;
    require(r.take(1)? == [1] && r.field(128)? == b"NORMAL" && r.take(8)? == [0; 8])?;
    r.field(16)?;
    r.field(16)?;
    r.field(16)?;
    r.take(1)?;
    require(r.take(1)? == [0] && r.take(1)? == [0])?;
    r.field(128)?;
    let record = r.field(65536)?.to_vec();
    let outputs = r.u32()?;
    require(outputs <= 256)?;
    let mut names = BTreeSet::new();
    for _ in 0..outputs {
        require(names.insert(r.field(128)?.to_vec()))?;
        r.field(128)?;
        r.field(65536)?;
    }
    require(r.take(1)? == [0])?;
    let file = String::from_utf8(r.field(16)?.to_vec()).map_err(|_| problem())?;
    require(r.u64()? > 0)?;
    r.field(512)?;
    require(r.at == row.payload.len())?;
    Ok((file, record))
}

fn rows_observation(
    store: &dyn PlatformStore,
    execution: &ExecutionRecord,
    effects: &[EffectRecord],
) -> Result<serde_json::Value, CorpusProblem> {
    let expected = oracle()?;
    let records = expected["records"].as_array().ok_or_else(problem)?;
    require(
        effects.len() == 3 && records.len() == 3 && execution.state == ExecutionState::Running,
    )?;
    let mut observed = Vec::new();
    for (effect, record) in effects.iter().zip(records) {
        require(
            effect.execution_id == execution.execution_id
                && effect.run_unit_id == execution.run_unit_id
                && effect.state == EffectState::Completed
                && effect.intent.recovery_lease.is_none()
                && store.effect(&effect.key).map_err(|_| problem())?.as_ref() == Some(effect),
        )?;
        let row = store
            .get_provider_state("cics-effect-replay-v1", effect.key.as_str())
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let desc = mainframe_env_cics::describe_cics_replay_row(
            &row,
            Some(effect),
            mainframe_env_cics::CicsLimits::default(),
        )
        .map_err(|_| problem())?;
        require(
            desc.retention == mainframe_env_cics::CicsReplayRetentionState::Terminal
                && desc.codec == mainframe_env_cics::CicsReplayCodecVersion::PlainReadV4
                && desc.owner_execution.as_deref() == Some(execution.execution_id.as_str())
                && desc.owner_run_unit.as_deref() == Some(execution.run_unit_id.as_str()),
        )?;
        let (file, payload) = reply_record(&row)?;
        let expected_bytes = unhex(record["record_ascii_hex"].as_str().ok_or_else(problem)?)?;
        require(
            record["file"] == file
                && payload == expected_bytes
                && record["width"].as_u64() == Some(payload.len() as u64)
                && record["record_ascii_sha256"] == evidence::hash(&payload),
        )?;
        observed.push(
            serde_json::json!({"file": file, "effect": effect.key.as_str(),
            "sequence": effect.sequence, "request_sha256": hex(&effect.request_digest),
            "result_sha256": effect.result_digest.map(|value| hex(&value)),
            "core_resolved_tick": effect.resolved_tick, "row_version": row.version,
            "row_sha256": evidence::hash(&row.payload), "record_sha256": evidence::hash(&payload),
            "record_length": payload.len(), "payload_hex": hex(&row.payload)}),
        );
    }
    Ok(
        serde_json::json!({"execution":execution.execution_id.as_str(),
        "run":execution.run_unit_id.as_str(), "artifact":execution.artifact.as_str(),
        "principal":execution.principal.as_str(), "version":execution.version,
        "attempt":execution.attempt, "state":"Running", "records":observed}),
    )
}

fn public_handoff_at_cut(
    cics: &mainframe_env_cics::CicsService,
    handoff: &Handoff,
    execution: &ExecutionRecord,
    effect: &EffectRecord,
) -> Result<Handoff, CorpusProblem> {
    let tick = effect
        .resolved_tick
        .filter(|tick| *tick != 0)
        .ok_or_else(problem)?;
    require(
        execution.state == ExecutionState::Running
            && effect.state == EffectState::Completed
            && effect.execution_id == execution.execution_id
            && effect.run_unit_id == execution.run_unit_id,
    )?;
    let session = SessionId::new(&handoff.session, 64).map_err(|_| problem())?;
    // The provider's command lease has finished before this successful core commit.
    // Keep the normal principal, availability, connected and expiry fences.
    let snapshot = cics
        .terminal_snapshot(&session, &execution.principal, tick)
        .map_err(|_| problem())?;
    require(
        snapshot.session == handoff.session
            && snapshot.principal == execution.principal.as_str()
            && snapshot.run_unit == execution.run_unit_id.as_str(),
    )?;
    // Exact fifteen-field projection owned by ProductServer::terminal_json.
    let public = serde_json::json!({
        "schema_version":"mainframe-env.cics-terminal@1", "session":snapshot.session,
        "principal":snapshot.principal, "transaction":snapshot.transaction,
        "run_unit":snapshot.run_unit, "rows":snapshot.rows, "columns":snapshot.columns,
        "aid":snapshot.aid,
        "screen_base64":base64::engine::general_purpose::STANDARD.encode(&snapshot.screen),
        "mapset":snapshot.mapset, "map":snapshot.map, "suspended":snapshot.suspended,
        "connected":snapshot.connected, "expires_at_tick":snapshot.expires_at_tick,
        "version":snapshot.version
    });
    let mut handoff = handoff.clone();
    handoff.public_digest = evidence::hash(&encode(&public)?);
    Ok(handoff)
}

fn save_cut(
    root: &Path,
    store: &dyn PlatformStore,
    execution: &ExecutionRecord,
    effects: &[EffectRecord],
    fetches: BTreeMap<String, u64>,
    dataset_decisions: u64,
    armed: &observed::ArmedCut,
) -> Result<(), CorpusProblem> {
    let marker = marker(root)?;
    let observed = rows_observation(store, execution, effects)?;
    let handoff = armed.handoff_at_cut(execution, effects.last().ok_or_else(problem)?)?;
    require(handoff.nonce == marker.nonce && handoff.artifact == execution.artifact.as_str())?;
    protocol::write_owned(
        root,
        &root.join("handoff.json"),
        &encode(&handoff)?,
        MAX_SECRET,
        (marker.root_device, marker.root_inode),
    )
    .map_err(io_problem)?;
    let handoff: Handoff = decode(
        &protocol::read_private(&root.join("handoff.json"), MAX_SECRET).map_err(io_problem)?,
    )?;
    require(
        handoff.nonce == marker.nonce
            && handoff.artifact == execution.artifact.as_str()
            && dataset_decisions >= 3,
    )?;
    let value = serde_json::json!({"schema":"carddemo.account-completed-read-cut@1",
        "nonce":marker.nonce, "pid":std::process::id(), "parent_pid":marker.parent_pid,
        "executable_digest":marker.executable_digest, "owner":observed,
        "backing_provider_fetches":fetches, "dataset_host_decisions":dataset_decisions,
        "controlled_exit":77, "source_binding_verified":true,
        "checkpoint_after_results":false, "suspended_observation":false});
    protocol::write_owned(
        root,
        &root.join("prepare.json"),
        &encode(&value)?,
        MAX_RECEIPT,
        (marker.root_device, marker.root_inode),
    )
    .map_err(io_problem)
}

type ObservedServer = (
    Arc<ProductServer>,
    Arc<ObservedStore>,
    Arc<MemorySecretResolver>,
);

fn open_observed(
    root: &Path,
    prepare: bool,
    owned: (u64, u64),
) -> Result<ObservedServer, CorpusProblem> {
    protocol::require_root_identity(root, owned).map_err(io_problem)?;
    let path = root.join("state.db");
    require(if prepare {
        !path.exists()
    } else {
        path.is_file()
    })?;
    let url = format!(
        "sqlite://{}?mode={}",
        path.display(),
        if prepare { "rwc" } else { "rw" }
    );
    let inner: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(|_| problem())?);
    let store = Arc::new(ObservedStore::new(inner, root.to_path_buf()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: url,
        artifact_profile: ArtifactProfile::Local,
        artifact_root: root.join("artifacts"),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    require(
        config.timeout_millis == 30000
            && config.shutdown_millis == 30000
            && config.cobol_current_date.is_none(),
    )?;
    let server = ProductServer::open(
        config,
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(|_| problem())?;
    Ok((server, store, secrets))
}

async fn first_child(root: &Path, corpus: &Path, marker: &Marker) -> Result<(), CorpusProblem> {
    let package = verify_carddemo_application_package_from_env(&inventory())?;
    let (definition, source) =
        online_definition::carddemo_base_online_definition_observed(corpus, package)?;
    let selected: Vec<_> = definition
        .programs
        .iter()
        .filter(|value| value.name == "COACTVWC")
        .collect();
    require(selected.len() == 1)?;
    let artifact = selected[0].artifact.clone();
    let (server, store, _secrets) =
        open_observed(root, true, (marker.root_device, marker.root_inode))?;
    install_base_online_authorities(&server, corpus, &definition)?;
    let installed = server
        .install_online_application(definition)
        .map_err(|_| problem())?;
    require(
        !installed.replayed
            && installed.programs == 18
            && installed.transactions == 17
            && installed.maps == 17,
    )?;
    source.observe_initial(corpus, store.as_ref(), &root.join("artifacts"))?;
    let mut http = Http {
        app: server.router(),
        calls: 0,
    };
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(b"WEBUSER:transport-password")
    );
    let (status, body) = http
        .call(
            Method::POST,
            "/zosmf/services/authenticate",
            BTreeMap::from([("authorization".into(), basic)]),
            Vec::new(),
        )
        .await?;
    require(status == StatusCode::OK)?;
    let auth: serde_json::Value = serde_json::from_slice(&body).map_err(|_| problem())?;
    let bearer = format!("Bearer {}", auth["token"].as_str().ok_or_else(problem)?);
    let (status, body) = http
        .call(
            Method::POST,
            "/mainframe-env/cics/v1/sessions",
            BTreeMap::from([
                ("authorization".into(), bearer.clone()),
                ("x-csrf-zosmf-header".into(), "true".into()),
            ]),
            br#"{"transaction":"CC00","rows":24,"columns":80}"#.to_vec(),
        )
        .await?;
    require(status == StatusCode::CREATED)?;
    let launch: serde_json::Value = serde_json::from_slice(&body).map_err(|_| problem())?;
    require(launch["terminal"]["mapset"] == "COSGN00")?;
    let mut handoff = Handoff {
        nonce: marker.nonce.clone(),
        session: launch["session"].as_str().ok_or_else(problem)?.into(),
        bearer,
        csrf: launch["csrf_token"].as_str().ok_or_else(problem)?.into(),
        public_digest: String::new(),
        artifact: artifact.as_str().into(),
        dataset_digest: String::new(),
        artifact_digest: String::new(),
    };
    let route = route(&handoff)?;
    let menu = http
        .exchange(
            &route,
            0x7d,
            BTreeMap::from([
                ("USERID".into(), "USER0001".into()),
                ("PASSWD".into(), "PASSWORD".into()),
            ]),
        )
        .await?;
    require(
        menu["mapset"] == "COMEN01" && menu["connected"] == true && menu["suspended"] == false,
    )?;
    let prompt = http
        .exchange(
            &route,
            0x7d,
            BTreeMap::from([("OPTION".into(), "1".into())]),
        )
        .await?;
    require(
        prompt["mapset"] == "COACTVW" && prompt["map"] == "CACTVWA" && prompt["connected"] == true,
    )?;
    let public = http.read(&route).await?;
    require(public == prompt)?;
    handoff.dataset_digest = evidence::datasets(&server)?;
    handoff.artifact_digest = evidence::artifacts(store.as_ref(), &root.join("artifacts"))?.0;
    // PUT and fresh resume legitimately publish new session metadata before the cut.
    // Publish the handoff once, with its full public digest captured at that cut.
    store.arm(artifact, handoff, Arc::downgrade(&server.cics_service()))?;
    // PUT+resume runs the unchanged installed COACTVWC. The successful third core
    // completion exits this process from the store decorator before delivery/checkpoint.
    let _unexpected = http
        .exchange(
            &route,
            0x7d,
            BTreeMap::from([("ACCTSID".into(), "00000000050".into())]),
        )
        .await?;
    Err(problem())
}

async fn second_child(
    root: &Path,
    corpus: &Path,
    marker: &Marker,
) -> Result<serde_json::Value, CorpusProblem> {
    verify_corpus(corpus).map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.corpus",
            "pinned corpus observation refused",
        )
    })?;
    let handoff: Handoff = decode(
        &protocol::read_private(&root.join("handoff.json"), MAX_SECRET).map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.handoff-read",
                "private handoff read refused",
            )
        })?,
    )?;
    let cut: serde_json::Value = decode(
        &protocol::read_private(&root.join("prepare.json"), MAX_RECEIPT).map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.cut-read",
                "private cut receipt read refused",
            )
        })?,
    )?;
    require(
        handoff.nonce == marker.nonce
            && cut["nonce"] == marker.nonce
            && cut["controlled_exit"] == 77,
    )?;
    let route = route(&handoff).map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.route",
            "retained authenticated route refused",
        )
    })?;
    let (server, store, secrets) =
        open_observed(root, false, (marker.root_device, marker.root_inode)).map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.open",
                "reopened observed server refused",
            )
        })?;
    require(
        evidence::datasets(&server).map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.dataset-observation",
                "reopened dataset observation refused",
            )
        })? == handoff.dataset_digest
            && evidence::artifacts(store.as_ref(), &root.join("artifacts"))
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.account.resume.artifact-observation",
                        "reopened installed artifact observation refused",
                    )
                })?
                .0
                == handoff.artifact_digest,
    )
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.sources",
            "pre-resume source digests differ",
        )
    })?;
    let id = ExecutionId::new(
        cut["owner"]["execution"].as_str().ok_or_else(problem)?,
        InvocationLimits::default(),
    )
    .map_err(|_| problem())?;
    let execution = store
        .get_execution(&id)
        .map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.execution-read",
                "retained execution lookup refused",
            )
        })?
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.account.resume.execution-missing",
                "retained execution absent",
            )
        })?;
    let mut effects = Vec::new();
    for record in cut["owner"]["records"].as_array().ok_or_else(problem)? {
        let key = IdempotencyKey::new(
            record["effect"].as_str().ok_or_else(problem)?,
            InvocationLimits::default(),
        )
        .map_err(|_| problem())?;
        effects.push(
            store
                .effect(&key)
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.account.resume.effect-read",
                        "retained completed effect lookup refused",
                    )
                })?
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.account.resume.effect-missing",
                        "retained completed effect absent",
                    )
                })?,
        );
    }
    require(
        rows_observation(store.as_ref(), &execution, &effects).map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.core-observation",
                "reopened completed core observation refused",
            )
        })? == cut["owner"],
    )
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.core",
            "pre-resume completed core records differ",
        )
    })?;
    let mut http = Http {
        app: server.router(),
        calls: 0,
    };
    let public = http.read(&route).await.map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.public-read",
            "reopened authenticated public read refused",
        )
    })?;
    require(evidence::hash(&encode(&public)?) == handoff.public_digest).map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.public",
            "full cut-bound public snapshot differs after reopen",
        )
    })?;
    denial_controls(&mut http, &route).await.map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.denials",
            "original authentication denial controls refused",
        )
    })?;
    require(
        http.read(&route).await.map_err(|_| {
            CorpusProblem::new(
                "carddemo.account.resume.post-denial-read",
                "authenticated public read after denials refused",
            )
        })? == public,
    )?;
    let before_fetch = store.snapshot()?;
    let before_decisions = store.dataset_decisions()?;
    let reply = http.resume(&route).await.map_err(|_| {
        CorpusProblem::new(
            "carddemo.account.resume.dispatch",
            "original resume HTTP call refused",
        )
    })?;
    let after_fetch = store.snapshot()?;
    let after_decisions = store.dataset_decisions()?;
    // Host invocation and SAF are allowed. Any Dataset host decisions
    // cannot increase while these already completed READ results are consumed.
    require(after_decisions == before_decisions)?;
    for (namespace, value) in &after_fetch {
        if namespace.starts_with("dataset") {
            require(before_fetch.get(namespace).copied().unwrap_or(0) == *value)?;
        }
    }
    require(
        reply["session"] == handoff.session
            && reply["principal"] == "WEBUSER"
            && reply["mapset"] == "COACTVW"
            && reply["map"] == "CACTVWA"
            && reply["connected"] == true
            && reply["suspended"] == false
            && reply["version"]
                .as_u64()
                .zip(public["version"].as_u64())
                .is_some_and(|(new, old)| new > old),
    )?;
    let expected = oracle()?;
    let fields = online_screen_fields(&reply)?;
    for (name, expected) in expected["public_fields_ascii_hex"]
        .as_object()
        .ok_or_else(problem)?
    {
        let expected = unhex(expected.as_str().ok_or_else(problem)?)?;
        require(
            fields
                .get(name)
                .is_some_and(|value| value.as_slice() == expected),
        )?;
    }
    for effect in &effects {
        require(store.effect(&effect.key).map_err(|_| problem())?.as_ref() == Some(effect))?;
        let row = store
            .get_provider_state("cics-effect-replay-v1", effect.key.as_str())
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let old = cut["owner"]["records"]
            .as_array()
            .ok_or_else(problem)?
            .iter()
            .find(|value| value["effect"] == effect.key.as_str())
            .ok_or_else(problem)?;
        require(
            old["row_sha256"] == evidence::hash(&row.payload) && old["row_version"] == row.version,
        )?;
    }
    require(
        evidence::datasets(&server)? == handoff.dataset_digest
            && evidence::artifacts(store.as_ref(), &root.join("artifacts"))?.0
                == handoff.artifact_digest,
    )?;
    let (status, _) = http
        .call(
            Method::DELETE,
            &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
            route.headers.clone(),
            Vec::new(),
        )
        .await?;
    require(status == StatusCode::NO_CONTENT)?;
    let (status, _) = http
        .call(
            Method::GET,
            &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
            route.headers.clone(),
            Vec::new(),
        )
        .await?;
    require(status == StatusCode::NOT_FOUND)?;
    drop(http);
    require(
        server.graceful_shutdown().await
            && server.cics_service().active_worker_count() == 0
            && secrets.entry_count() == 0,
    )?;
    drop(server);
    drop(store);
    drop(secrets);
    Ok(
        serde_json::json!({"schema":"carddemo.account-completed-read-consumption@1",
        "nonce":marker.nonce,"pid":std::process::id(),"parent_pid":marker.parent_pid,
        "executable_digest":marker.executable_digest,"session":handoff.session,"artifact":handoff.artifact,
        "read_records":cut["owner"]["records"],"before_provider_fetches":before_fetch,"after_provider_fetches":after_fetch,
        "dataset_host_decisions_during_resume":after_decisions-before_decisions,
        "account_screen":reply,"disconnected":true,"drained":true,"source_binding_verified":true,
        "suspended_observation":false,"global_acceptance_credit":0}),
    )
}

#[test]
#[ignore = "exact pinned corpus/package; two actual checked child processes and compiled source"]
fn corpus_sqlite_account_completed_read_recovers_after_process_exit() {
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("exact corpus input required"));
    verify_corpus(&corpus).unwrap();
    oracle().unwrap();
    let executable = env::current_exe().unwrap();
    let executable_digest = protocol::executable_digest(&executable).unwrap();
    let nonce = evidence::hash(
        format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes(),
    );
    let parent = fs::canonicalize(env::temp_dir()).unwrap();
    let root = parent.join(format!(
        "carddemo-account-read-process-{}-{nonce}",
        std::process::id()
    ));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root).unwrap();
    let owned = protocol::root_identity(&root).unwrap();
    let marker = Marker {
        root: root.to_str().unwrap().into(),
        root_device: owned.0,
        root_inode: owned.1,
        nonce: nonce.clone(),
        parent_pid: std::process::id(),
        executable: executable.to_str().unwrap().into(),
        executable_digest,
        corpus_commit: CORPUS_COMMIT.into(),
        corpus_content: CORPUS_CONTENT.into(),
    };
    let marker_bytes = encode(&marker).unwrap();
    protocol::write_owned(
        &root,
        &root.join("owner.json"),
        &marker_bytes,
        MAX_SECRET,
        owned,
    )
    .unwrap();
    let mut receipts = Vec::new();
    for phase in [protocol::Phase::Prepare, protocol::Phase::Resume] {
        let name = if phase == protocol::Phase::Prepare {
            "prepare"
        } else {
            "resume"
        };
        let exited =
            protocol::run_phase(&executable, &root, &corpus, &nonce, phase, owned).unwrap();
        protocol::write_owned(
            &root,
            &root.join(format!("{name}.stdout")),
            &exited.stdout,
            128 * 1024,
            owned,
        )
        .unwrap();
        protocol::write_owned(
            &root,
            &root.join(format!("{name}.stderr")),
            &exited.stderr,
            128 * 1024,
            owned,
        )
        .unwrap();
        assert_eq!(
            exited.status.code(),
            Some(if name == "prepare" { 77 } else { 0 }),
            "checked child failed; private logs at {}",
            root.display()
        );
        let receipt: serde_json::Value = decode(
            &protocol::read_private(&root.join(format!("{name}.json")), MAX_RECEIPT).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["pid"].as_u64(), Some(u64::from(exited.pid)));
        assert_eq!(receipt["nonce"], nonce);
        assert_eq!(
            receipt["parent_pid"].as_u64(),
            Some(u64::from(marker.parent_pid))
        );
        assert_eq!(receipt["executable_digest"], marker.executable_digest);
        assert_eq!(receipt["source_binding_verified"], true);
        assert_eq!(receipt["suspended_observation"], false);
        receipts.push(receipt);
    }
    assert_ne!(receipts[0]["pid"], receipts[1]["pid"]);
    assert_eq!(receipts[0]["owner"]["records"], receipts[1]["read_records"]);
    assert_eq!(receipts[1]["dataset_host_decisions_during_resume"], 0);
    assert_eq!(receipts[1]["disconnected"], true);
    assert_eq!(receipts[1]["drained"], true);
    assert_eq!(
        protocol::executable_digest(&executable).unwrap(),
        marker.executable_digest
    );
    cleanup(&root, &marker_bytes, owned).unwrap();
    println!(
        "actual installed account source consumed three completed READ results across two checked exited processes; no repeated Dataset provider dispatch; separate Suspended J09/global/vendor/backend acceptance remains unchanged"
    );
}

#[test]
#[ignore = "private child entry; only exact bounded owning parent may invoke"]
fn corpus_sqlite_account_read_process_child() {
    let root =
        PathBuf::from(env::var_os(protocol::ROOT_ENV).expect("owned child fixture required"));
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("exact corpus required"));
    verify_corpus(&corpus).unwrap();
    let marker = marker(&root).unwrap();
    let phase = env::var(protocol::PHASE_ENV).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let receipt = runtime
        .block_on(async {
            match phase.as_str() {
                "prepare" => {
                    first_child(&root, &corpus, &marker).await?;
                    Err(problem())
                }
                "resume" => second_child(&root, &corpus, &marker).await,
                _ => Err(problem()),
            }
        })
        .unwrap();
    protocol::write_owned(
        &root,
        &root.join(format!("{phase}.json")),
        &encode(&receipt).unwrap(),
        MAX_RECEIPT,
        (marker.root_device, marker.root_inode),
    )
    .unwrap();
}

#[test]
fn plain_read_current_generation_classifier_preserves_exact_legacy_and_new_request_fences() {
    super::super::read_request_observations::current_plain_read_generation_controls();
}

#[test]
fn plain_read_dispatch_hook_preserves_unkeyed_assign_and_guards_keyed_calls() {
    use mainframe_env_host_api::{
        CapabilityDescriptor, CicsDisposition, CicsResponse, EffectResult, HostLimits,
        HostProvider, HostRequest, HostResult, ProgramRequest,
    };
    use mainframe_env_interpreter::{
        CoordinatorLimits, DurableEffectDispatch, DurableEffectDispatchHook, ExecutionControl,
        ExecutionCoordinator, ReferenceMachine,
    };

    struct Port {
        descriptor: CapabilityDescriptor,
        store: Arc<MemoryStore>,
        calls: Arc<Mutex<Vec<EffectRequest>>>,
    }
    impl HostProvider for Port {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
            assert_eq!(effect.run_unit, actor.run_unit_id);
            let outcome = match &effect.request {
                HostRequest::Cics(request) => {
                    assert_eq!(request.operation, CicsOperation::Assign);
                    assert_eq!(effect.sequence, 1);
                    assert_eq!(effect.idempotency_key, None);
                    assert_eq!(request.mutation, None);
                    assert!(!effect.request.requires_dispatch_record());
                    Ok(HostResult::Cics(CicsResponse {
                        disposition: CicsDisposition::Complete,
                        condition: "NORMAL".into(),
                        response: 0,
                        response2: 0,
                        applid: "TESTAPPL".into(),
                        sysid: "TEST".into(),
                        transaction: "TEST".into(),
                        aid: 0,
                        target: None,
                        next_transaction: None,
                        payload: BoundedPayload::new(
                            "mainframe-env.cics.payload@1",
                            Vec::new(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                        outputs: BTreeMap::from([(
                            "APPLID".into(),
                            BoundedPayload::new(
                                "mainframe-env.cics.text@1",
                                b"TESTAPPL".to_vec(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                        )]),
                        unit_of_work: None,
                    }))
                }
                HostRequest::Program(ProgramRequest::Call { program, .. }) => {
                    assert_eq!(program.as_str(), "KEYED");
                    assert_eq!(effect.sequence, 2);
                    let intent = self
                        .store
                        .effect(effect.idempotency_key.as_ref().unwrap())
                        .unwrap()
                        .unwrap();
                    assert_eq!(intent.state, EffectState::Intent);
                    assert_eq!(intent.execution_id, actor.execution_id);
                    assert_eq!(intent.sequence, 2);
                    Ok(HostResult::Program(
                        BoundedPayload::new(
                            "mainframe-env.cobol.call-result@1",
                            vec![0, 0, 0, 0],
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ))
                }
                _ => panic!("only the actual compiled ASSIGN and CALL are selected"),
            };
            let sequence = effect.sequence;
            self.calls.lock().unwrap().push(effect);
            EffectResult { sequence, outcome }
        }
    }
    struct Hook {
        mode: &'static str,
        calls: usize,
    }
    impl DurableEffectDispatchHook for Hook {
        fn with_dispatch(
            &mut self,
            actual: &DurableEffectDispatch<'_>,
            dispatch: &mut dyn FnMut() -> Result<(), HostProblem>,
        ) -> Result<(), HostProblem> {
            self.calls += 1;
            actual.require_current(actual.original())?;
            assert_eq!(actual.effect().sequence, 2);
            assert_eq!(actual.intent().state, EffectState::Intent);
            if self.mode == "refuse" {
                return Err(HostProblem::UnknownOutcome);
            }
            dispatch()?;
            if self.mode == "twice" {
                dispatch()?;
            }
            Ok(())
        }
    }

    let artifact = crate::compile(
        "IDENTIFICATION DIVISION. PROGRAM-ID. HOOKROUT. \
         DATA DIVISION. WORKING-STORAGE SECTION. 01 APPL PIC X(8). \
         PROCEDURE DIVISION. EXEC CICS ASSIGN APPLID(APPL) END-EXEC. \
         DISPLAY APPL. CALL 'KEYED'. STOP RUN.",
    )
    .unwrap();
    for mode in ["once", "refuse", "twice"] {
        let mut actor = crate::invocation(&artifact, 4096);
        actor.principal = Principal::new(
            PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
            BTreeSet::from([
                CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap(),
                CapabilityId::new("host.program.invoke", InvocationLimits::default()).unwrap(),
            ]),
            InvocationLimits::default(),
        )
        .unwrap();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let calls: Arc<Mutex<Vec<EffectRequest>>> = Arc::default();
        let providers: Vec<Arc<dyn HostProvider>> = ["host.cics.execute", "host.program.invoke"]
            .into_iter()
            .map(|capability| {
                Arc::new(Port {
                    descriptor: CapabilityDescriptor {
                        capability: CapabilityId::new(capability, InvocationLimits::default())
                            .unwrap(),
                        provider_id: capability.into(),
                        generation: "1".into(),
                        request_schema: "dispatch-routing-request@1".into(),
                        result_schema: "dispatch-routing-result@1".into(),
                        max_request_bytes: 4096,
                        max_result_bytes: 4096,
                        ready: true,
                    },
                    store: store.clone(),
                    calls: calls.clone(),
                }) as Arc<dyn HostProvider>
            })
            .collect();
        let host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ));
        let coordinator =
            ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            actor.clone(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .unwrap();
        let mut hook = Hook { mode, calls: 0 };
        let outcome = coordinator.execute_resumable_with_dispatch_control(
            &mut machine,
            &actor,
            || {
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            },
            &mut hook,
        );
        assert_eq!(hook.calls, 1, "{mode}");
        assert_eq!(machine.snapshot().output, b"TESTAPPL\n");
        let unkeyed = IdempotencyKey::new(
            format!("{}:1", actor.idempotency_key),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(store.effect(&unkeyed).unwrap(), None);
        let keyed = IdempotencyKey::new(
            format!("{}:2", actor.idempotency_key),
            InvocationLimits::default(),
        )
        .unwrap();
        let retained = store.effect(&keyed).unwrap().unwrap();
        let expected_calls = if mode == "refuse" { 1 } else { 2 };
        assert_eq!(calls.lock().unwrap().len(), expected_calls);
        let audits = store.audit_records(&actor.execution_id, 0, 8).unwrap();
        assert_eq!(audits.len(), expected_calls);
        assert_eq!(audits[0].effect_sequence, 1);
        assert_eq!(audits[0].decision, AuditDecision::Success);
        let events = store.events(&actor.execution_id, 0, 32).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    matches!(event.kind, LifecycleEventKind::EffectResult { sequence: 1 })
                })
                .count(),
            1
        );
        if mode == "once" {
            assert!(matches!(outcome, ExecutionOutcome::Completed(_)));
            assert_eq!(retained.state, EffectState::Completed);
            assert_eq!(audits[1].decision, AuditDecision::Success);
        } else {
            assert!(matches!(outcome,
                ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome()));
            assert_eq!(
                retained.state,
                if mode == "refuse" {
                    EffectState::Intent
                } else {
                    EffectState::UnknownOutcome
                }
            );
        }
    }
}
