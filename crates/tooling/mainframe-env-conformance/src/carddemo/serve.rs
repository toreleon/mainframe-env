//! Local interactive application composition; certification remains a separate command.

use super::*;
use axum::response::Html;
use axum::routing::get;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::net::SocketAddr;

/// Launch the pinned upstream online application with persistent state and a browser terminal.
/// The embedded transport identities are demonstration accounts, for local development only.
pub fn serve_carddemo_from_env(
    inventory_path: &Path,
    state_dir: &Path,
    listen: SocketAddr,
) -> Result<(), CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    serve_carddemo_application(inventory_path, &corpus_dir, &corpus_dir, state_dir, listen)
}

/// Compose an editable application separately from its verified seed/reference corpus.
/// A changed application requires a fresh state directory; existing generations are immutable.
pub fn serve_carddemo_application(
    inventory_path: &Path,
    corpus_dir: &Path,
    source_dir: &Path,
    state_dir: &Path,
    listen: SocketAddr,
) -> Result<(), CorpusProblem> {
    verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    fs::create_dir_all(state_dir).map_err(serve_problem)?;
    let state_dir = fs::canonicalize(state_dir).map_err(serve_problem)?;
    let database = state_dir.join("state.db");
    let database = database.to_str().ok_or_else(|| {
        CorpusProblem::new("carddemo.serve.path", "state directory must be UTF-8")
    })?;
    let sqlite_url = format!("sqlite://{database}?mode=rwc");
    let config = ServerConfig {
        listen: listen.to_string(),
        store_profile: StoreProfile::Sqlite,
        sqlite_url: sqlite_url.clone(),
        artifact_profile: ArtifactProfile::Local,
        artifact_root: state_dir.join("artifacts"),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    eprintln!("Compiling CardDemo online programs from the pinned upstream checkout...");
    let definition = carddemo_base_online_definition(source_dir)?;
    let layouts = browser_layouts(source_dir)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(serve_problem)?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(listen).await.map_err(serve_problem)?;
        let store = Arc::new(
            SqliteStateStore::open(&sqlite_url, 64 * 1024 * 1024, 262_144)
                .map_err(serve_problem)?,
        );
        let server = ProductServer::open(
            config,
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .map_err(terminal_problem)?;
        server.bootstrap_administrator("IBMUSER", b"TESTPASS").map_err(terminal_problem)?;
        // Publish a launcher-owned completion marker only after all authorities and
        // programs are installed. Reopen retains edited datasets and transport identities.
        let marker = store.get_provider_state("carddemo-live-install", "base-online")
            .map_err(serve_problem)?;
        let installed = if let Some(marker) = marker {
            if marker.version != 1 {
                return Err(CorpusProblem::new("carddemo.serve.install", "unsupported installation marker"));
            }
            let installed = server.install_online_application(definition).map_err(terminal_problem)?;
            if marker.payload != installed.identity.as_bytes() {
                return Err(CorpusProblem::new("carddemo.serve.install", "installed application identity differs"));
            }
            installed
        } else {
            install_base_online_authorities(&server, &corpus_dir, &definition)?;
            let installed = server.install_online_application(definition).map_err(terminal_problem)?;
            store.put_provider_state(ProviderStateRecord {
                namespace: "carddemo-live-install".into(),
                key: "base-online".into(),
                version: 1,
                payload: installed.identity.as_bytes().to_vec(),
            }, None).map_err(serve_problem)?;
            installed
        };
        server.start_background_workers().map_err(terminal_problem)?;
        let readiness_server = server.clone();
        let app = server.router()
            .route("/readyz", get(move || {
                let ready = readiness_server.readiness().ready();
                async move {
                    (if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
                     axum::Json(serde_json::json!({"ready":ready,"profile":"carddemo-online"})))
                }
            }))
            .route("/", get(|| async {
                (
                    [("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"), ("cache-control", "no-store")],
                    Html(include_str!("web/index.html")),
                )
            }))
            .route("/favicon.svg", get(|| async {
                ([("content-type", "image/svg+xml")], include_str!("web/favicon.svg"))
            }))
            .route("/terminal.js", get(|| async {
                ([("content-type", "text/javascript; charset=utf-8")], include_str!("web/terminal.js"))
            }))
            .route("/terminal.css", get(|| async {
                ([("content-type", "text/css; charset=utf-8")], include_str!("web/terminal.css"))
            }))
            .route("/carddemo/layouts", get(move || {
                let layouts = layouts.clone();
                async move { axum::Json(layouts) }
            }));
        eprintln!("CardDemo running at http://{listen} ({}/{}/{} programs/transactions/maps)", installed.programs, installed.transactions, installed.maps);
        eprintln!("Application sign-on: USER0001 / PASSWORD; sample account: 00000000050");
        eprintln!("Persistent application state: {}", state_dir.display());
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await;
        let drained = server.graceful_shutdown().await;
        result.map_err(serve_problem)?;
        if !drained {
            return Err(CorpusProblem::new("carddemo.serve.shutdown", "background workers did not drain"));
        }
        Ok(())
    })
}

fn serve_problem(error: impl fmt::Display) -> CorpusProblem {
    CorpusProblem::new("carddemo.serve.infrastructure", error.to_string())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn browser_layouts(corpus_dir: &Path) -> Result<serde_json::Value, CorpusProblem> {
    let mut layouts = serde_json::Map::new();
    for relative in collect_paths(corpus_dir, &["app/bms"], "bms")? {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(serve_problem)?;
        let parsed = parse_bms(&source).map_err(package_problem)?;
        let mapset = Path::new(&relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| CorpusProblem::new("carddemo.serve.layout", "mapset name is missing"))?;
        let fields = parsed.fields.iter().map(|field| {
            let attributes = field.attributes.iter().map(|attr| attr.to_ascii_uppercase()).collect::<BTreeSet<_>>();
            serde_json::json!({
                "name":field.name, "position":field.position, "length":field.length,
                "initial":field.initial, "protected":attributes.contains("PROT") || attributes.contains("ASKIP"),
                "secret":attributes.contains("DRK")
            })
        }).collect::<Vec<_>>();
        layouts.insert(
            mapset.to_string(),
            serde_json::json!({"map":parsed.name, "fields":fields}),
        );
    }
    Ok(serde_json::Value::Object(layouts))
}
