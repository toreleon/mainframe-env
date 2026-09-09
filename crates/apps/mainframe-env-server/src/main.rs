mod maintenance;

use clap::Parser;
use mainframe_env_racf::MemorySecretResolver;
use mainframe_env_server::{
    ConfigOverrides, EnvironmentSecretResolver, HmacSha256PackageTrust, ProductServer,
    RetentionMaintenance, ServerConfig, StoreProfile, default_program_router,
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
    #[command(subcommand)]
    command: Option<ServerCommand>,
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
) -> Result<ServerConfig, RunFailure> {
    let loaded = if matches!(command, Some(ServerCommand::Retention { .. })) {
        ServerConfig::from_sources_for_retention(
            Some(config_path),
            environment,
            ConfigOverrides::default(),
        )
    } else {
        ServerConfig::from_sources(Some(config_path), environment, ConfigOverrides::default())
    };
    loaded.map_err(|problem| RunFailure::new("configuration", problem.to_string()))
}

async fn run(cli: Cli) -> Result<(), RunFailure> {
    let config_path = cli.config;
    let command = cli.command;
    let environment = std::env::vars()
        .filter(|(name, _)| {
            name.starts_with("MAINFRAME_ENV_") && !name.starts_with("MAINFRAME_ENV_SECRET_")
        })
        .collect::<BTreeMap<_, _>>();
    let config = load_config(&config_path, &environment, &command)?;
    let postgres_url = if config.store_profile == StoreProfile::Postgres {
        Some(
            environment
                .get("MAINFRAME_ENV_POSTGRES_URL")
                .cloned()
                .ok_or_else(|| {
                    RunFailure::new(
                        "postgres_secret_resolution",
                        "MAINFRAME_ENV_POSTGRES_URL did not resolve postgres_url_reference",
                    )
                })?,
        )
    } else {
        None
    };
    let store: Arc<dyn PlatformStore> = match config.store_profile {
        StoreProfile::Memory => Arc::new(MemoryStore::new(Default::default())),
        StoreProfile::Sqlite => Arc::new(
            SqliteStateStore::open(&config.sqlite_url, 64 * 1024 * 1024, 262_144)
                .map_err(|problem| RunFailure::new("state_store_open", problem.to_string()))?,
        ),
        StoreProfile::Postgres => Arc::new(
            PostgresStateStore::open(
                postgres_url.as_deref().ok_or_else(|| {
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
    let shared_artifacts: Option<Arc<dyn ArtifactStore>> = match postgres_url.as_deref() {
        Some(url) => Some(Arc::new(
            PostgresArtifactStore::open(url, 64 * 1024 * 1024, 262_144)
                .map_err(|problem| RunFailure::new("artifact_store_open", problem.to_string()))?,
        )),
        None => None,
    };
    let package_trust = Arc::new(
        HmacSha256PackageTrust::from_environment(
            &environment,
            Arc::new(EnvironmentSecretResolver::process()),
        )
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
    let address: SocketAddr = config
        .listen
        .parse()
        .map_err(|_| RunFailure::new("listen", "listen is not a socket address"))?;
    server
        .start_background_workers()
        .map_err(|problem| RunFailure::new("worker_start", problem.to_string()))?;
    let router = server.router();
    if config.tls.enabled {
        let certificate = config
            .tls
            .certificate_path
            .as_ref()
            .ok_or_else(|| RunFailure::new("tls", "TLS certificate path is missing"))?;
        let key_path = environment
            .get("MAINFRAME_ENV_TLS_KEY_PATH")
            .ok_or_else(|| {
                RunFailure::new(
                    "tls",
                    "MAINFRAME_ENV_TLS_KEY_PATH did not resolve private_key_reference",
                )
            })?;
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(certificate, key_path)
            .await
            .map_err(|error| RunFailure::new("tls", error.to_string()))?;
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
        assert!(load_config(&path, &BTreeMap::new(), &None).is_ok());
        let command = Some(ServerCommand::Retention {
            action: RetentionAction::Legacy {
                max_records: Some(1),
                conflict_retries: 0,
            },
        });
        let error = load_config(&path, &BTreeMap::new(), &command).unwrap_err();
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
}
