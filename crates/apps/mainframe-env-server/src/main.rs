mod maintenance;

use clap::Parser;
use mainframe_env_racf::{MemorySecretResolver, ResolvedSecret, SecretResolver};
use mainframe_env_server::{
    ArtifactProfile, ConfigOverrides, EnvironmentSecretResolver, HmacSha256PackageTrust,
    ProductServer, RetentionMaintenance, ServerConfig, StoreProfile, default_program_router,
};
use mainframe_env_store::{
    MemoryStore, PostgresArtifactStore, PostgresStateStore, SqliteStateStore,
};
use mainframe_env_store_api::{ArtifactStore, PlatformStore};
#[cfg(test)]
use maintenance::RetentionAction;
use maintenance::{MaintenanceError, ServerCommand};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "mainframe-env-server",
    version,
    about = "mainframe-env z/OS-compatible service"
)]
struct Cli {
    #[arg(value_name = "CONFIG", default_value = "config/mainframe-env.toml")]
    config: PathBuf,
    #[arg(long)]
    listen: Option<String>,
    #[arg(long, value_parser = parse_store_profile)]
    store_profile: Option<StoreProfile>,
    #[arg(long)]
    sqlite_url: Option<String>,
    #[arg(long)]
    postgres_url_reference: Option<String>,
    #[arg(long, value_parser = parse_artifact_profile)]
    artifact_profile: Option<ArtifactProfile>,
    #[arg(long)]
    artifact_root: Option<PathBuf>,
    #[arg(long)]
    max_body_bytes: Option<usize>,
    #[arg(long)]
    max_concurrency: Option<usize>,
    #[arg(long)]
    timeout_millis: Option<u64>,
    #[arg(long)]
    shutdown_millis: Option<u64>,
    #[arg(long, action = clap::ArgAction::Set)]
    tls_enabled: Option<bool>,
    #[arg(long)]
    tls_certificate_path: Option<PathBuf>,
    #[arg(long)]
    tls_private_key_reference: Option<String>,
    #[arg(long)]
    bootstrap_administrator: Option<String>,
    #[arg(long)]
    bootstrap_secret_reference: Option<String>,
    #[command(subcommand)]
    command: Option<ServerCommand>,
}

impl Cli {
    fn overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            listen: self.listen.clone(),
            store_profile: self.store_profile,
            sqlite_url: self.sqlite_url.clone(),
            postgres_url_reference: self.postgres_url_reference.clone(),
            artifact_profile: self.artifact_profile,
            artifact_root: self.artifact_root.clone(),
            max_body_bytes: self.max_body_bytes,
            max_concurrency: self.max_concurrency,
            timeout_millis: self.timeout_millis,
            shutdown_millis: self.shutdown_millis,
            tls_enabled: self.tls_enabled,
            tls_certificate_path: self.tls_certificate_path.clone(),
            tls_private_key_reference: self.tls_private_key_reference.clone(),
            bootstrap_administrator: self.bootstrap_administrator.clone(),
            bootstrap_secret_reference: self.bootstrap_secret_reference.clone(),
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let machine_output = cli.command.is_some();
    if let Err(problem) = run(cli).await {
        if machine_output {
            eprintln!("{}", problem.machine_json());
        } else {
            eprintln!("mainframe-env-server: {}", problem.detail);
        }
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct RunFailure {
    code: &'static str,
    detail: Box<str>,
    target: Option<&'static str>,
    attempts: Option<usize>,
    phase: Option<&'static str>,
    partial: bool,
    progress: Option<Box<serde_json::Value>>,
    authorization_required: Option<Box<serde_json::Value>>,
}

impl RunFailure {
    fn new(code: &'static str, detail: impl Into<Box<str>>) -> Self {
        Self {
            code,
            detail: detail.into(),
            target: None,
            attempts: None,
            phase: None,
            partial: false,
            progress: None,
            authorization_required: None,
        }
    }

    fn machine_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "mainframe-env.retention-error@2",
            "status": if self.partial { "partial" } else { "error" },
            "code": self.code,
            "detail": self.detail,
            "target": self.target,
            "attempts": self.attempts,
            "phase": self.phase,
            "progress": self.progress,
            "authorization_required": self.authorization_required,
        })
    }
}

impl From<MaintenanceError> for RunFailure {
    fn from(problem: MaintenanceError) -> Self {
        Self {
            code: problem.code,
            detail: problem.detail,
            target: problem.target,
            attempts: problem.attempts,
            phase: problem.phase,
            partial: problem.partial,
            progress: problem.progress,
            authorization_required: problem.authorization_required,
        }
    }
}

fn load_config(
    config_path: &std::path::Path,
    environment: &BTreeMap<String, String>,
    command: &Option<ServerCommand>,
    overrides: ConfigOverrides,
) -> Result<ServerConfig, RunFailure> {
    let loaded = if matches!(command, Some(ServerCommand::Retention { .. })) {
        ServerConfig::from_sources_for_retention(Some(config_path), environment, overrides)
    } else {
        ServerConfig::from_sources(Some(config_path), environment, overrides)
    };
    loaded.map_err(|problem| RunFailure::new("configuration", problem.to_string()))
}

async fn run(cli: Cli) -> Result<(), RunFailure> {
    let environment = std::env::vars()
        .filter(|(name, _)| {
            name.starts_with("MAINFRAME_ENV_") && !name.starts_with("MAINFRAME_ENV_SECRET_")
        })
        .collect::<BTreeMap<_, _>>();
    run_with_sources(
        cli,
        environment,
        Arc::new(EnvironmentSecretResolver::process()),
    )
    .await
}

async fn run_with_sources(
    cli: Cli,
    environment: BTreeMap<String, String>,
    runtime_secrets: Arc<dyn SecretResolver>,
) -> Result<(), RunFailure> {
    let overrides = cli.overrides();
    let config_path = cli.config;
    let command = cli.command;
    let config = load_config(&config_path, &environment, &command, overrides)?;
    let postgres_secret = if config.store_profile == StoreProfile::Postgres {
        let reference = config.postgres_url_reference.as_deref().ok_or_else(|| {
            RunFailure::new(
                "postgres_secret_resolution",
                "postgres_url_reference is missing",
            )
        })?;
        Some(
            resolve_config_secret(runtime_secrets.as_ref(), reference).map_err(|problem| {
                RunFailure::new("postgres_secret_resolution", problem.to_string())
            })?,
        )
    } else {
        None
    };
    let postgres_url = postgres_secret
        .as_deref()
        .map(|secret| {
            std::str::from_utf8(secret).map_err(|_| {
                RunFailure::new(
                    "postgres_secret_resolution",
                    "postgres_url_reference did not resolve UTF-8",
                )
            })
        })
        .transpose()?;
    let store: Arc<dyn PlatformStore> = match config.store_profile {
        StoreProfile::Memory => Arc::new(MemoryStore::new(Default::default())),
        StoreProfile::Sqlite => Arc::new(
            SqliteStateStore::open(&config.sqlite_url, 64 * 1024 * 1024, 262_144)
                .map_err(|problem| RunFailure::new("state_store_open", problem.to_string()))?,
        ),
        StoreProfile::Postgres => Arc::new(
            PostgresStateStore::open(
                postgres_url.ok_or_else(|| {
                    RunFailure::new("postgres_secret_resolution", "postgres URL is missing")
                })?,
                64 * 1024 * 1024,
                262_144,
            )
            .map_err(|problem| RunFailure::new("state_store_open", problem.to_string()))?,
        ),
    };
    if let Some(ServerCommand::Retention { action }) = command {
        let policy = config
            .retention
            .policy()
            .map_err(|problem| RunFailure::new("configuration", problem.to_string()))?;
        let maintenance = RetentionMaintenance::open(store, policy)
            .map_err(|problem| RunFailure::new("maintenance_open", problem.to_string()))?;
        let output = maintenance::execute(
            &maintenance,
            config.store_profile,
            config.retention.max_batch,
            action,
        )?;
        println!("{output}");
        return Ok(());
    }

    let address: SocketAddr = config
        .listen
        .parse()
        .map_err(|_| RunFailure::new("listen", "listen is not a socket address"))?;
    let tls = if config.tls.enabled {
        let certificate_path = config
            .tls
            .certificate_path
            .as_ref()
            .ok_or_else(|| RunFailure::new("tls", "TLS certificate path is missing"))?;
        let certificate = std::fs::read(certificate_path)
            .map_err(|error| RunFailure::new("tls", error.to_string()))?;
        let reference = config
            .tls
            .private_key_reference
            .as_deref()
            .ok_or_else(|| RunFailure::new("tls", "TLS private_key_reference is missing"))?;
        let private_key = resolve_config_secret(runtime_secrets.as_ref(), reference)
            .map_err(|problem| RunFailure::new("tls", problem.to_string()))?;
        Some(
            axum_server::tls_rustls::RustlsConfig::from_pem(certificate, private_key.to_vec())
                .await
                .map_err(|error| RunFailure::new("tls", error.to_string()))?,
        )
    } else {
        None
    };

    let shared_artifacts: Option<Arc<dyn ArtifactStore>> = match postgres_url {
        Some(url) => Some(Arc::new(
            PostgresArtifactStore::open(url, 64 * 1024 * 1024, 262_144)
                .map_err(|problem| RunFailure::new("artifact_store_open", problem.to_string()))?,
        )),
        None => None,
    };
    let package_trust = Arc::new(
        HmacSha256PackageTrust::from_environment(&environment, runtime_secrets.clone())
            .map_err(|problem| RunFailure::new("package_trust", problem.to_string()))?,
    );
    let secrets = Arc::new(MemorySecretResolver::default());
    let program = default_program_router();
    let server = match shared_artifacts {
        Some(artifacts) => ProductServer::open_with_package_trust_and_artifact_store(
            config.clone(),
            store,
            secrets,
            program,
            package_trust,
            artifacts,
        ),
        None => ProductServer::open_with_package_trust(
            config.clone(),
            store,
            secrets,
            program,
            package_trust,
        ),
    }
    .map_err(|problem| RunFailure::new("product_open", problem.to_string()))?;
    if let (Some(administrator), Some(reference)) = (
        config.bootstrap.administrator.as_deref(),
        config.bootstrap.secret_reference.as_deref(),
    ) {
        let reference = EnvironmentSecretResolver::parse_reference(reference)
            .map_err(|problem| RunFailure::new("bootstrap", problem.to_string()))?;
        server
            .bootstrap_administrator_from_reference(
                administrator,
                &reference,
                runtime_secrets.as_ref(),
            )
            .map_err(|problem| RunFailure::new("bootstrap", problem.to_string()))?;
    }
    server
        .start_background_workers()
        .map_err(|problem| RunFailure::new("worker_start", problem.to_string()))?;
    let router = server.router();
    if let Some(tls) = tls {
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
            .map_err(|error| RunFailure::new("serve", error.to_string()))?;
    } else {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| RunFailure::new("listen", error.to_string()))?;
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                wait_for_shutdown().await;
                let _ = server.graceful_shutdown().await;
            })
            .await
            .map_err(|error| RunFailure::new("serve", error.to_string()))?;
    }
    Ok(())
}

fn parse_store_profile(value: &str) -> Result<StoreProfile, String> {
    match value.to_ascii_lowercase().as_str() {
        "memory" => Ok(StoreProfile::Memory),
        "sqlite" => Ok(StoreProfile::Sqlite),
        "postgres" => Ok(StoreProfile::Postgres),
        _ => Err("store profile must be memory, sqlite, or postgres".into()),
    }
}

fn parse_artifact_profile(value: &str) -> Result<ArtifactProfile, String> {
    match value.to_ascii_lowercase().as_str() {
        "local" => Ok(ArtifactProfile::Local),
        "shared" => Ok(ArtifactProfile::Shared),
        _ => Err("artifact profile must be local or shared".into()),
    }
}

fn resolve_config_secret(
    resolver: &dyn SecretResolver,
    reference: &str,
) -> Result<ResolvedSecret, mainframe_env_host_api::HostProblem> {
    let reference = EnvironmentSecretResolver::parse_reference(reference)?;
    resolver.resolve(&reference)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_accepts_the_legacy_positional_config_path() {
        let cli = Cli::try_parse_from(["mainframe-env-server", "custom.toml"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("custom.toml"));
        assert_eq!(cli.command, None);
    }

    #[test]
    fn command_line_has_a_bounded_default_config_path() {
        let cli = Cli::try_parse_from(["mainframe-env-server"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("config/mainframe-env.toml"));
        assert_eq!(cli.command, None);
    }

    #[test]
    fn command_line_accepts_forecast_with_the_default_config() {
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            "retention",
            "forecast",
            "--observed-growth-per-tick",
            "9",
            "--conflict-retries",
            "2",
        ])
        .unwrap();
        assert_eq!(cli.config, PathBuf::from("config/mainframe-env.toml"));
        assert_eq!(
            cli.command,
            Some(ServerCommand::Retention {
                action: RetentionAction::Forecast {
                    observed_growth_per_tick: 9,
                    conflict_retries: 2,
                },
            })
        );
    }

    #[test]
    fn command_line_accepts_one_bounded_pass_after_a_legacy_config() {
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            "custom.toml",
            "retention",
            "maintain",
            "--max-records",
            "17",
        ])
        .unwrap();
        assert_eq!(cli.config, PathBuf::from("custom.toml"));
        assert_eq!(
            cli.command,
            Some(ServerCommand::Retention {
                action: RetentionAction::Maintain {
                    max_records: Some(17),
                    authorize_oversized_archive: None,
                    conflict_retries: 3,
                },
            })
        );
    }

    #[test]
    fn command_line_accepts_exact_oversized_archive_authorization() {
        let archive_id = format!("sha256:{}", "a".repeat(64));
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            "retention",
            "maintain",
            "--max-records",
            "8",
            "--authorize-oversized-archive",
            &archive_id,
        ])
        .unwrap();
        assert_eq!(
            cli.command,
            Some(ServerCommand::Retention {
                action: RetentionAction::Maintain {
                    max_records: Some(8),
                    authorize_oversized_archive: Some(archive_id),
                    conflict_retries: 3,
                },
            })
        );
    }

    #[test]
    fn machine_error_distinguishes_partial_progress() {
        let failure = RunFailure::from(MaintenanceError {
            code: "infrastructure_failure",
            detail: "failed after a completed target".into(),
            target: Some("mq-replay"),
            attempts: Some(1),
            phase: Some("target"),
            partial: true,
            progress: Some(Box::new(serde_json::json!({
                "completed_targets": [{ "target": "db2-replay" }],
            }))),
            authorization_required: None,
        });
        let output = failure.machine_json();
        assert_eq!(output["schema"], "mainframe-env.retention-error@2");
        assert_eq!(output["status"], "partial");
        assert_eq!(output["phase"], "target");
        assert_eq!(
            output["progress"]["completed_targets"][0]["target"],
            "db2-replay"
        );
    }

    #[test]
    fn command_line_rejects_unbounded_maintenance_options() {
        assert!(
            Cli::try_parse_from([
                "mainframe-env-server",
                "retention",
                "maintain",
                "--max-records",
                "0",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "mainframe-env-server",
                "retention",
                "forecast",
                "--conflict-retries",
                "9",
            ])
            .is_err()
        );
    }

    #[test]
    fn retention_cli_rejects_in_memory_sqlite_while_serving_config_may_use_it() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-retention-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        let config = ServerConfig {
            sqlite_url: "sqlite::memory:".into(),
            tls: mainframe_env_server::TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        };
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        assert!(load_config(&path, &BTreeMap::new(), &None, ConfigOverrides::default()).is_ok());
        let command = Some(ServerCommand::Retention {
            action: RetentionAction::Legacy {
                max_records: Some(1),
                conflict_retries: 0,
            },
        });
        let error = load_config(
            &path,
            &BTreeMap::new(),
            &command,
            ConfigOverrides::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, "configuration");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn command_line_accepts_explicit_legacy_reconciliation() {
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            "retention",
            "reconcile",
            "--target",
            "db2-replay",
            "--namespace",
            "db2-v1-replay",
            "--key",
            "mutation-17",
            "--expected-version",
            "4",
            "--owner-execution",
            "execution-17",
        ])
        .unwrap();
        assert_eq!(
            cli.command,
            Some(ServerCommand::Retention {
                action: RetentionAction::Reconcile {
                    target: mainframe_env_store_api::RetentionTarget::Db2Replay,
                    namespace: "db2-v1-replay".into(),
                    key: "mutation-17".into(),
                    expected_version: 4,
                    owner_execution: Some("execution-17".into()),
                    conflict_retries: 3,
                },
            })
        );
    }

    #[test]
    fn command_line_maps_named_overrides_without_secret_values() {
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            "custom.toml",
            "--listen",
            "127.0.0.1:20443",
            "--store-profile",
            "memory",
            "--artifact-root",
            "artifacts",
            "--sqlite-url",
            "sqlite://cli.db?mode=rwc",
            "--postgres-url-reference",
            "env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL",
            "--artifact-profile",
            "local",
            "--max-body-bytes",
            "8192",
            "--max-concurrency",
            "16",
            "--timeout-millis",
            "4000",
            "--shutdown-millis",
            "5000",
            "--tls-enabled",
            "false",
            "--tls-certificate-path",
            "server.crt",
            "--tls-private-key-reference",
            "env-base64:MAINFRAME_ENV_SECRET_TLS_PRIVATE_KEY",
            "--bootstrap-administrator",
            "ADMIN",
            "--bootstrap-secret-reference",
            "env-base64:MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN",
        ])
        .unwrap();
        let overrides = cli.overrides();
        assert_eq!(overrides.listen.as_deref(), Some("127.0.0.1:20443"));
        assert_eq!(overrides.store_profile, Some(StoreProfile::Memory));
        assert_eq!(
            overrides.sqlite_url.as_deref(),
            Some("sqlite://cli.db?mode=rwc")
        );
        assert_eq!(
            overrides.postgres_url_reference.as_deref(),
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL")
        );
        assert_eq!(overrides.artifact_profile, Some(ArtifactProfile::Local));
        assert_eq!(overrides.artifact_root, Some(PathBuf::from("artifacts")));
        assert_eq!(overrides.max_body_bytes, Some(8192));
        assert_eq!(overrides.max_concurrency, Some(16));
        assert_eq!(overrides.timeout_millis, Some(4000));
        assert_eq!(overrides.shutdown_millis, Some(5000));
        assert_eq!(overrides.tls_enabled, Some(false));
        assert_eq!(
            overrides.tls_certificate_path,
            Some(PathBuf::from("server.crt"))
        );
        assert_eq!(
            overrides.tls_private_key_reference.as_deref(),
            Some("env-base64:MAINFRAME_ENV_SECRET_TLS_PRIVATE_KEY")
        );
        assert_eq!(overrides.bootstrap_administrator.as_deref(), Some("ADMIN"));
        assert_eq!(
            overrides.bootstrap_secret_reference.as_deref(),
            Some("env-base64:MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN")
        );
    }

    #[tokio::test]
    async fn retention_command_exits_before_serving_authorities_and_secret_resolution() {
        use mainframe_env_host_api::{HostProblem, SecretRef};
        use mainframe_env_store_api::ProviderStateStore;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct RejectingResolver(AtomicUsize);

        impl SecretResolver for RejectingResolver {
            fn resolve(&self, _: &SecretRef) -> Result<ResolvedSecret, HostProblem> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(HostProblem::NotFound)
            }
        }

        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-r09-headless-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("state.db");
        let sqlite_url = format!("sqlite://{}?mode=rwc", database.display());
        let config_path = directory.join("config.toml");
        let config = ServerConfig {
            store_profile: StoreProfile::Sqlite,
            sqlite_url: sqlite_url.clone(),
            artifact_profile: ArtifactProfile::Local,
            artifact_root: directory.join("must-not-open-artifacts"),
            tls: mainframe_env_server::TlsConfig {
                enabled: true,
                certificate_path: Some(directory.join("missing-server.crt")),
                private_key_reference: Some(
                    "env-base64:MAINFRAME_ENV_SECRET_MISSING_TLS_KEY".into(),
                ),
            },
            bootstrap: mainframe_env_server::BootstrapConfig {
                administrator: Some("ADMIN".into()),
                secret_reference: Some("env-base64:MAINFRAME_ENV_SECRET_MISSING_BOOTSTRAP".into()),
            },
            ..ServerConfig::default()
        };
        std::fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
        let cli = Cli::try_parse_from([
            "mainframe-env-server",
            config_path.to_str().unwrap(),
            "retention",
            "forecast",
        ])
        .unwrap();
        let resolver = Arc::new(RejectingResolver(AtomicUsize::new(0)));
        let environment = BTreeMap::from([
            (
                "MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS".into(),
                "not-json".into(),
            ),
            ("MAINFRAME_ENV_ARTIFACT_STORE".into(), "not-a-store".into()),
            ("MAINFRAME_ENV_TLS".into(), "not-a-boolean".into()),
        ]);

        run_with_sources(cli, environment, resolver.clone())
            .await
            .unwrap();
        assert_eq!(resolver.0.load(Ordering::SeqCst), 0);
        assert!(!directory.join("must-not-open-artifacts").exists());

        let reopened = SqliteStateStore::open(&sqlite_url, 64 * 1024 * 1024, 262_144).unwrap();
        assert!(
            reopened
                .list_provider_state("server-bootstrap", 8)
                .unwrap()
                .is_empty()
        );
        assert!(
            reopened
                .get_provider_state("jes-meta", "next-id")
                .unwrap()
                .is_none()
        );
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
