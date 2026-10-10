//! Actual first-party terminal RECEIVE suspension across two exited processes.
//! Test-only; default product clocks, deadlines, grants and lifecycle are preserved.
use super::*;
use mainframe_env_store_api::PlatformStore;
use std::io;

#[path = "terminal_process_negative_tests.rs"]
mod controls;
#[path = "terminal_process_evidence.rs"]
mod evidence;
#[path = "terminal_process_oracles.rs"]
mod oracles;
#[path = "terminal_process_passive_storage.rs"]
mod passive_storage;
#[path = "terminal_process_protocol.rs"]
mod protocol;

const MAX_SECRET: usize = 65536;
const MAX_RECEIPT: usize = 1024 * 1024;
const CORPUS_COMMIT: &str = "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e";
const CORPUS_CONTENT: &str = "df62d3b541c8b296c03432ebc93cf2f007d3792e2cffff8501a8f964e9afa260";

fn problem() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.terminal.process_restart",
        "owned actual source/process/session evidence refused",
    )
}
fn require(condition: bool) -> Result<(), CorpusProblem> {
    if condition { Ok(()) } else { Err(problem()) }
}
fn io_problem(_: io::Error) -> CorpusProblem {
    problem()
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CorpusProblem> {
    serde_json::to_vec(value).map_err(|_| problem())
}
fn decode<T: for<'a> Deserialize<'a> + Serialize>(bytes: &[u8]) -> Result<T, CorpusProblem> {
    let value: T = serde_json::from_slice(bytes).map_err(|_| problem())?;
    // Exact typed canonical roundtrip rejects duplicate map keys, unknown fields,
    // trailing input and alternative framing before any offered identity is used.
    require(encode(&value)? == bytes)?;
    Ok(value)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    root: String,
    root_device: u64,
    root_inode: u64,
    nonce: String,
    parent_pid: u32,
    executable: String,
    executable_digest: String,
    corpus_commit: String,
    corpus_content: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handoff {
    nonce: String,
    session: String,
    bearer: String,
    csrf: String,
    public_digest: String,
    durable_digest: String,
    artifact_digest: String,
    artifact: String,
    dataset_digest: String,
    security_digest: String,
    owner: evidence::Owner,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    phase: String,
    nonce: String,
    pid: u32,
    parent_pid: u32,
    executable_digest: String,
    corpus_commit: String,
    corpus_content: String,
    session: String,
    artifact: String,
    owner: evidence::Owner,
    source_binding_verified: bool,
    actual_suspended: bool,
    page: usize,
    worker_count: usize,
    drained: bool,
    disconnected: bool,
}

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

struct Http {
    app: axum::Router,
    calls: usize,
}
impl Http {
    async fn call(
        &mut self,
        method: Method,
        route: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<(StatusCode, Vec<u8>), CorpusProblem> {
        self.calls += 1;
        require(self.calls <= 16 && body.len() <= 65536 && route.len() <= 512)?;
        let mut builder = Request::builder().method(method).uri(route);
        for (key, value) in headers {
            builder = builder.header(key, value);
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::from(body)).map_err(|_| problem())?)
            .await
            .map_err(|_| problem())?;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 65536)
            .await
            .map_err(|_| problem())?;
        Ok((status, bytes.to_vec()))
    }
    async fn read(
        &mut self,
        route: &CardDemoOnlineSession,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, body) = self
            .call(
                Method::GET,
                &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
                route.headers.clone(),
                Vec::new(),
            )
            .await?;
        require(status == StatusCode::OK)?;
        serde_json::from_slice(&body).map_err(|_| problem())
    }
    async fn resume(
        &mut self,
        route: &CardDemoOnlineSession,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, body) = self
            .call(
                Method::POST,
                &format!("/mainframe-env/cics/v1/sessions/{}/resume", route.session),
                route.headers.clone(),
                Vec::new(),
            )
            .await?;
        require(status == StatusCode::OK)?;
        serde_json::from_slice(&body).map_err(|_| problem())
    }
    async fn exchange(
        &mut self,
        route: &CardDemoOnlineSession,
        aid: u8,
        fields: BTreeMap<String, String>,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, _) = self
            .call(
                Method::PUT,
                &format!("/mainframe-env/cics/v1/sessions/{}/input", route.session),
                route.headers.clone(),
                encode(&serde_json::json!({"aid":aid,"fields":fields}))?,
            )
            .await?;
        require(status == StatusCode::OK)?;
        self.resume(route).await
    }
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

async fn prepare(root: &Path, corpus: &Path, marker: &Marker) -> Result<Receipt, CorpusProblem> {
    // Independently source-bound compilation occurs once, before actual installed
    // records are read. The second process never invokes this builder or installer.
    let package = verify_carddemo_application_package_from_env(&inventory())?;
    let (definition, expected_source) =
        online_definition::carddemo_base_online_definition_observed(corpus, package)?;
    let (server, store, secrets) =
        open_server(root, true, (marker.root_device, marker.root_inode))?;
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
    let _source_proof =
        expected_source.observe_initial(corpus, store.as_ref(), &root.join("artifacts"))?;
    let (artifact_digest, artifact) = evidence::artifacts(store.as_ref(), &root.join("artifacts"))?;
    let dataset_digest = evidence::datasets(&server)?;
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
    require(
        launch["terminal"]["principal"] == "WEBUSER" && launch["terminal"]["mapset"] == "COSGN00",
    )?;
    let mut handoff = Handoff {
        nonce: marker.nonce.clone(),
        session: launch["session"].as_str().ok_or_else(problem)?.into(),
        bearer,
        csrf: launch["csrf_token"].as_str().ok_or_else(problem)?.into(),
        public_digest: String::new(),
        durable_digest: String::new(),
        artifact_digest,
        artifact: artifact.as_str().into(),
        dataset_digest,
        security_digest: String::new(),
        owner: evidence::Owner {
            execution: String::new(),
            run: String::new(),
            artifact: String::new(),
            version: 0,
            attempt: 0,
            tick: 0,
            retained_digest: String::new(),
        },
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
        menu["session"] == route.session
            && menu["principal"] == "WEBUSER"
            && menu["mapset"] == "COMEN01"
            && menu["map"] == "COMEN1A"
            && menu["connected"] == true
            && menu["suspended"] == false,
    )?;
    let first = http
        .exchange(
            &route,
            0x7d,
            BTreeMap::from([("OPTION".into(), "3".into())]),
        )
        .await?;
    oracles::page(&first, &route.session, 1, false)?;
    evidence::continuation(store.as_ref(), &route.session, 1)?;
    let before_pf8 = evidence::before(store.as_ref(), &server, &route.session)?;
    let second = http.exchange(&route, 0xf8, BTreeMap::new()).await?;
    oracles::page(&second, &route.session, 2, false)?;
    evidence::continuation(store.as_ref(), &route.session, 2)?;
    let _completed_owner = evidence::stage(
        store.as_ref(),
        &server,
        &route.session,
        &artifact,
        &before_pf8,
        &second,
        false,
    )?;
    let before_empty = evidence::before(store.as_ref(), &server, &route.session)?;
    // Genuine absent input: deliberately no PUT precedes this public resume.
    let suspended = http.resume(&route).await?;
    let persisted = http.read(&route).await?;
    require(
        suspended == persisted
            && persisted["version"]
                .as_u64()
                .zip(second["version"].as_u64())
                .is_some_and(|(next, prior)| next > prior),
    )?;
    oracles::page(&persisted, &route.session, 2, true)?;
    handoff.owner = evidence::stage(
        store.as_ref(),
        &server,
        &route.session,
        &artifact,
        &before_empty,
        &persisted,
        true,
    )?;
    evidence::continuation(store.as_ref(), &route.session, 2)?;
    evidence::machine(store.as_ref(), &route.session, &artifact)?;
    passive_storage::observe(
        store.as_ref(),
        &root.join("artifacts"),
        &route.session,
        &handoff.owner,
    )?;
    require(
        evidence::datasets(&server)? == handoff.dataset_digest
            && evidence::artifacts(store.as_ref(), &root.join("artifacts"))?.0
                == handoff.artifact_digest,
    )?;
    handoff.public_digest = evidence::hash(&encode(&persisted)?);
    handoff.durable_digest = evidence::durable_digest(store.as_ref(), &route.session)?;
    handoff.security_digest = evidence::security_retention(store.as_ref())?;
    protocol::write_owned(
        root,
        &root.join("handoff.json"),
        &encode(&handoff)?,
        MAX_SECRET,
        (marker.root_device, marker.root_inode),
    )
    .map_err(io_problem)?;
    drop(http);
    require(
        server.graceful_shutdown().await
            && server.cics_service().active_worker_count() == 0
            && secrets.entry_count() == 0,
    )?;
    let receipt = receipt(marker, &handoff, "prepare", 2, true, false);
    drop(server);
    drop(store);
    drop(secrets);
    Ok(receipt)
}

async fn resume(root: &Path, corpus: &Path, marker: &Marker) -> Result<Receipt, CorpusProblem> {
    oracles::check_inputs(corpus)?;
    let handoff: Handoff = decode(
        &protocol::read_private(&root.join("handoff.json"), MAX_SECRET).map_err(io_problem)?,
    )?;
    require(handoff.nonce == marker.nonce)?;
    let route = route(&handoff)?;
    let (server, store, secrets) =
        open_server(root, false, (marker.root_device, marker.root_inode))?;
    let (artifact_digest, artifact) = evidence::artifacts(store.as_ref(), &root.join("artifacts"))?;
    require(
        artifact_digest == handoff.artifact_digest
            && artifact.as_str() == handoff.artifact
            && evidence::datasets(&server)? == handoff.dataset_digest
            && evidence::durable_digest(store.as_ref(), &route.session)? == handoff.durable_digest
            && evidence::security_retention(store.as_ref())? == handoff.security_digest
            && evidence::owner_digest(store.as_ref(), &handoff.owner)?
                == handoff.owner.retained_digest,
    )?;
    evidence::continuation(store.as_ref(), &route.session, 2)?;
    evidence::machine(store.as_ref(), &route.session, &artifact)?;
    let mut http = Http {
        app: server.router(),
        calls: 0,
    };
    let retained = http.read(&route).await?;
    oracles::page(&retained, &route.session, 2, true)?;
    require(evidence::hash(&encode(&retained)?) == handoff.public_digest)?;
    passive_storage::observe(
        store.as_ref(),
        &root.join("artifacts"),
        &route.session,
        &handoff.owner,
    )?;
    denial_controls(&mut http, &route).await?;
    let unchanged = http.read(&route).await?;
    require(
        unchanged == retained
            && evidence::durable_digest(store.as_ref(), &route.session)? == handoff.durable_digest
            && evidence::owner_digest(store.as_ref(), &handoff.owner)?
                == handoff.owner.retained_digest
            && server
                .online_trace(&route.session)
                .map_err(|_| problem())?
                .is_empty(),
    )?;
    let before = evidence::before(store.as_ref(), &server, &route.session)?;
    let third = http.exchange(&route, 0xf8, BTreeMap::new()).await?;
    oracles::page(&third, &route.session, 3, false)?;
    evidence::continuation(store.as_ref(), &route.session, 3)?;
    let current_owner = evidence::stage(
        store.as_ref(),
        &server,
        &route.session,
        &artifact,
        &before,
        &third,
        false,
    )?;
    require(
        current_owner.execution != handoff.owner.execution
            && current_owner.run != handoff.owner.run
            && store
                .get_provider_state("online-machine-continuation", &route.session)
                .map_err(|_| problem())?
                .is_none()
            && store
                .get_provider_state("online-exchange-v1", &route.session)
                .map_err(|_| problem())?
                .is_none()
            && evidence::owner_digest(store.as_ref(), &handoff.owner)?
                == handoff.owner.retained_digest
            && evidence::datasets(&server)? == handoff.dataset_digest
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
    let mut receipt = receipt(marker, &handoff, "resume", 3, false, true);
    receipt.owner = current_owner;
    drop(server);
    drop(store);
    drop(secrets);
    Ok(receipt)
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

type OpenedServer = (
    Arc<ProductServer>,
    Arc<dyn PlatformStore>,
    Arc<MemorySecretResolver>,
);

fn open_server(
    root: &Path,
    prepare: bool,
    owned_root: (u64, u64),
) -> Result<OpenedServer, CorpusProblem> {
    // Refuse root drift before any SQLite/artifact/server authority opens.
    // The existing serial owned fixture does not claim an atomic filesystem fence.
    protocol::require_root_identity(root, owned_root).map_err(io_problem)?;
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
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).map_err(|_| problem())?);
    let secrets = Arc::new(MemorySecretResolver::default());
    require(secrets.entry_count() == 0)?;
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

fn receipt(
    marker: &Marker,
    handoff: &Handoff,
    phase: &str,
    page: usize,
    suspended: bool,
    disconnected: bool,
) -> Receipt {
    Receipt {
        phase: phase.into(),
        nonce: marker.nonce.clone(),
        pid: std::process::id(),
        parent_pid: marker.parent_pid,
        executable_digest: marker.executable_digest.clone(),
        corpus_commit: CORPUS_COMMIT.into(),
        corpus_content: CORPUS_CONTENT.into(),
        session: handoff.session.clone(),
        artifact: handoff.artifact.clone(),
        owner: handoff.owner.clone(),
        source_binding_verified: true,
        actual_suspended: suspended,
        page,
        worker_count: 0,
        drained: true,
        disconnected,
    }
}

fn receipt_matches(receipt: &Receipt, marker: &Marker, phase: &str, pid: u32) -> bool {
    receipt.phase == phase
        && receipt.nonce == marker.nonce
        && receipt.pid == pid
        && receipt.pid != marker.parent_pid
        && receipt.parent_pid == marker.parent_pid
        && receipt.executable_digest == marker.executable_digest
        && receipt.corpus_commit == CORPUS_COMMIT
        && receipt.corpus_content == CORPUS_CONTENT
        && SessionId::new(&receipt.session, 64).is_ok()
        && receipt.pid > 0
        && ExecutionId::new(&receipt.owner.execution, InvocationLimits::default()).is_ok()
        && RunUnitId::new(&receipt.owner.run, InvocationLimits::default()).is_ok()
        && ArtifactRef::new(&receipt.artifact, InvocationLimits::default()).is_ok()
        && receipt.owner.artifact == receipt.artifact
        && receipt.owner.retained_digest.len() == 64
        && receipt
            .owner
            .retained_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && receipt.owner.attempt > 0
        && receipt.owner.version > 0
        && receipt.owner.tick > 0
        && receipt.source_binding_verified
        && receipt.worker_count == 0
        && receipt.drained
        && if phase == "prepare" {
            receipt.page == 2 && receipt.actual_suspended && !receipt.disconnected
        } else {
            phase == "resume"
                && receipt.page == 3
                && !receipt.actual_suspended
                && receipt.disconnected
        }
}

fn receipt_pair_matches(first: &Receipt, second: &Receipt) -> bool {
    first.phase == "prepare"
        && second.phase == "resume"
        && first.pid != second.pid
        && first.session == second.session
        && first.artifact == second.artifact
        && first.owner.execution != second.owner.execution
        && first.owner.run != second.owner.run
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

#[test]
#[ignore = "requires exact pinned CARDDEMO_CORPUS_DIR; actual two-process source route"]
fn corpus_sqlite_terminal_recovers_after_process_exit() {
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("exact corpus input required"));
    verify_corpus(&corpus).unwrap();
    let executable = env::current_exe().unwrap();
    let executable_digest = protocol::executable_digest(&executable).unwrap();
    let nonce = evidence::hash(
        format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes(),
    );
    let parent = fs::canonicalize(env::temp_dir()).unwrap();
    let root = parent.join(format!(
        "carddemo-terminal-process-{}-{nonce}",
        std::process::id()
    ));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root).unwrap();
    let owned_root = protocol::root_identity(&root).unwrap();
    let marker = Marker {
        root: root.to_str().unwrap().into(),
        root_device: owned_root.0,
        root_inode: owned_root.1,
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
        owned_root,
    )
    .unwrap();
    let mut accepted = Vec::new();
    for phase in [protocol::Phase::Prepare, protocol::Phase::Resume] {
        let phase_name = if phase == protocol::Phase::Prepare {
            "prepare"
        } else {
            "resume"
        };
        let exited =
            protocol::run_phase(&executable, &root, &corpus, &nonce, phase, owned_root).unwrap();
        protocol::write_owned(
            &root,
            &root.join(format!("{phase_name}.stdout")),
            &exited.stdout,
            128 * 1024,
            owned_root,
        )
        .unwrap();
        protocol::write_owned(
            &root,
            &root.join(format!("{phase_name}.stderr")),
            &exited.stderr,
            128 * 1024,
            owned_root,
        )
        .unwrap();
        assert!(
            exited.status.success(),
            "actual {phase_name} child failed; bounded private logs retained at {}",
            root.display()
        );
        let receipt: Receipt = decode(
            &protocol::read_private(&root.join(format!("{phase_name}.json")), MAX_RECEIPT).unwrap(),
        )
        .unwrap();
        assert!(
            receipt_matches(&receipt, &marker, phase_name, exited.pid),
            "actual {phase_name} child receipt refused"
        );
        accepted.push(receipt);
    }
    assert!(
        receipt_pair_matches(&accepted[0], &accepted[1]),
        "two exited child processes did not preserve one selected source/session/artifact"
    );
    assert_eq!(
        protocol::executable_digest(&executable).unwrap(),
        marker.executable_digest
    );
    cleanup(&root, &marker_bytes, owned_root).unwrap();
    let mut observations = RouteObservations::default();
    observations
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J09",
            "process restart while suspended",
            false,
            || Ok(problem()),
        )
        .unwrap();
    assert!(observations.journeys().iter().any(|(id, values)| {
        id == "CD.J09"
            && values
                .iter()
                .any(|value| value == "process restart while suspended")
    }));
    println!(
        "actual selected CD.J09/process restart while suspended: two checked exited child processes; genuine inputless RECEIVE suspension; independently frozen page3 after actual PF8; exact owned fixture removal; zero global/license/orphan credit"
    );
}

#[test]
#[ignore = "private child entry; only exact bounded owning parent may invoke"]
fn corpus_sqlite_terminal_process_child() {
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
                "prepare" => prepare(&root, &corpus, &marker).await,
                "resume" => resume(&root, &corpus, &marker).await,
                _ => Err(problem()),
            }
        })
        .unwrap();
    // Published only after actual checked drain and all server/store handles drop.
    protocol::write_owned(
        &root,
        &root.join(format!("{phase}.json")),
        &encode(&receipt).unwrap(),
        MAX_RECEIPT,
        (marker.root_device, marker.root_inode),
    )
    .unwrap();
}
