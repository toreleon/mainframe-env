use mainframe_env_racf::MemorySecretResolver;
use mainframe_env_server::{
    ConfigOverrides, HmacSha256PackageTrust, ProductServer, ServerConfig, StoreProfile,
    default_program_router,
};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};
use mainframe_env_store_api::PlatformStore;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    if let Err(problem) = run().await {
        eprintln!("mainframe-env-server: {problem}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/mainframe-env.toml"));
    let environment = std::env::vars()
        .filter(|(name, _)| name.starts_with("MAINFRAME_ENV_"))
        .collect::<BTreeMap<_, _>>();
    let config =
        ServerConfig::from_sources(Some(&config_path), &environment, ConfigOverrides::default())
            .map_err(|problem| problem.to_string())?;
    let store: Arc<dyn PlatformStore> = match config.store_profile {
        StoreProfile::Memory => Arc::new(MemoryStore::new(Default::default())),
        StoreProfile::Sqlite => Arc::new(
            SqliteStateStore::open(&config.sqlite_url, 64 * 1024 * 1024, 262_144)
                .map_err(|problem| problem.to_string())?,
        ),
        StoreProfile::Postgres => {
            let url = environment
                .get("MAINFRAME_ENV_POSTGRES_URL")
                .ok_or("MAINFRAME_ENV_POSTGRES_URL did not resolve postgres_url_reference")?;
            Arc::new(
                PostgresStateStore::open(url, 64 * 1024 * 1024, 262_144)
                    .map_err(|problem| problem.to_string())?,
            )
        }
    };
    let package_trust = Arc::new(
        HmacSha256PackageTrust::from_environment(&environment)
            .map_err(|problem| problem.to_string())?,
    );
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        package_trust,
    )
    .map_err(|problem| problem.to_string())?;
    let address: SocketAddr = config
        .listen
        .parse()
        .map_err(|_| "listen is not a socket address")?;
    let router = server.router();
    if config.tls.enabled {
        let certificate = config
            .tls
            .certificate_path
            .as_ref()
            .ok_or("TLS certificate path is missing")?;
        let key_path = environment
            .get("MAINFRAME_ENV_TLS_KEY_PATH")
            .ok_or("MAINFRAME_ENV_TLS_KEY_PATH did not resolve private_key_reference")?;
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(certificate, key_path)
            .await
            .map_err(|error| error.to_string())?;
        let handle = axum_server::Handle::new();
        let shutdown = handle.clone();
        let product = server.clone();
        let grace = Duration::from_millis(config.shutdown_millis);
        tokio::spawn(async move {
            wait_for_shutdown().await;
            let _ = product.graceful_shutdown().await;
            shutdown.graceful_shutdown(Some(grace));
        });
        axum_server::bind_rustls(address, tls)
            .handle(handle)
            .serve(router.into_make_service())
            .await
            .map_err(|error| error.to_string())?;
    } else {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| error.to_string())?;
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                wait_for_shutdown().await;
                let _ = server.graceful_shutdown().await;
            })
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
