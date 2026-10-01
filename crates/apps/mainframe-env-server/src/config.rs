use crate::EnvironmentSecretResolver;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::RetentionPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreProfile {
    Memory,
    Sqlite,
    Postgres,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactProfile {
    #[default]
    Local,
    Shared,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub enabled: bool,
    pub certificate_path: Option<PathBuf>,
    pub private_key_reference: Option<String>,
}

/// Operator-configured retention lifetimes, alert thresholds, and batch bound.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionConfig {
    /// Minimum logical ticks before terminal lifecycle state can be archived.
    pub lifecycle_ticks: u64,
    /// Minimum logical ticks preserving effect and provider replay idempotency.
    pub idempotency_ticks: u64,
    /// Minimum logical ticks preserving typed audit decisions before archival.
    pub audit_ticks: u64,
    /// Minimum logical ticks preserving archive batches before deletion.
    pub archive_ticks: u64,
    /// Used-capacity percentage that begins early warning.
    pub low_watermark_percent: u8,
    /// Used-capacity percentage that begins urgent warning.
    pub high_watermark_percent: u8,
    /// Maximum source records in one retention transaction.
    pub max_batch: usize,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            lifecycle_ticks: 7 * 24 * 60 * 60 * 1_000,
            idempotency_ticks: 24 * 60 * 60 * 1_000,
            audit_ticks: 90 * 24 * 60 * 60 * 1_000,
            archive_ticks: 365 * 24 * 60 * 60 * 1_000,
            low_watermark_percent: 70,
            high_watermark_percent: 85,
            max_batch: 1_024,
        }
    }
}

impl RetentionConfig {
    /// Convert configuration into the store contract after validating every bound.
    pub fn policy(self) -> Result<RetentionPolicy, HostProblem> {
        RetentionPolicy {
            lifecycle_ticks: self.lifecycle_ticks,
            idempotency_ticks: self.idempotency_ticks,
            audit_ticks: self.audit_ticks,
            archive_ticks: self.archive_ticks,
            low_watermark_percent: self.low_watermark_percent,
            high_watermark_percent: self.high_watermark_percent,
            max_batch: self.max_batch,
        }
        .validate()
        .map_err(|_| HostProblem::Malformed)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    pub administrator: Option<String>,
    pub secret_reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub schema_version: u32,
    pub profile: String,
    pub listen: String,
    pub store_profile: StoreProfile,
    pub sqlite_url: String,
    pub postgres_url_reference: Option<String>,
    #[serde(default)]
    pub artifact_profile: ArtifactProfile,
    pub artifact_root: PathBuf,
    pub max_body_bytes: usize,
    pub max_concurrency: usize,
    pub timeout_millis: u64,
    pub shutdown_millis: u64,
    #[serde(default)]
    pub retention: RetentionConfig,
    pub tls: TlsConfig,
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
    /// In-process COBOL business date; omitted from deployment configuration.
    #[serde(skip)]
    pub cobol_current_date: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: "core-server".into(),
            listen: "127.0.0.1:10443".into(),
            store_profile: StoreProfile::Sqlite,
            sqlite_url: "sqlite://mainframe-env.db?mode=rwc".into(),
            postgres_url_reference: None,
            artifact_profile: ArtifactProfile::Local,
            artifact_root: PathBuf::from("mainframe-env-artifacts"),
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrency: 256,
            timeout_millis: 30_000,
            shutdown_millis: 30_000,
            retention: RetentionConfig::default(),
            tls: TlsConfig {
                enabled: true,
                certificate_path: None,
                private_key_reference: None,
            },
            bootstrap: BootstrapConfig::default(),
            cobol_current_date: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigOverrides {
    pub listen: Option<String>,
    pub store_profile: Option<StoreProfile>,
    pub sqlite_url: Option<String>,
    pub postgres_url_reference: Option<String>,
    pub artifact_profile: Option<ArtifactProfile>,
    pub artifact_root: Option<PathBuf>,
    pub max_body_bytes: Option<usize>,
    pub max_concurrency: Option<usize>,
    pub timeout_millis: Option<u64>,
    pub shutdown_millis: Option<u64>,
    pub tls_enabled: Option<bool>,
    pub tls_certificate_path: Option<PathBuf>,
    pub tls_private_key_reference: Option<String>,
    pub bootstrap_administrator: Option<String>,
    pub bootstrap_secret_reference: Option<String>,
}

impl ServerConfig {
    pub fn from_sources(
        file: Option<&Path>,
        environment: &BTreeMap<String, String>,
        cli: ConfigOverrides,
    ) -> Result<Self, HostProblem> {
        Self::from_sources_for_purpose(file, environment, cli, false)
    }

    /// Load only the durable-store and retention configuration used by offline maintenance.
    ///
    /// Serving-only listener, TLS, artifact, and request-pool overrides are ignored so an invalid
    /// deployment-only environment cannot block inspection of an otherwise valid durable store.
    pub fn from_sources_for_retention(
        file: Option<&Path>,
        environment: &BTreeMap<String, String>,
        cli: ConfigOverrides,
    ) -> Result<Self, HostProblem> {
        Self::from_sources_for_purpose(file, environment, cli, true)
    }

    fn from_sources_for_purpose(
        file: Option<&Path>,
        environment: &BTreeMap<String, String>,
        cli: ConfigOverrides,
        retention_only: bool,
    ) -> Result<Self, HostProblem> {
        let mut config = if let Some(path) = file {
            let bytes = std::fs::read(path).map_err(|_| HostProblem::InfrastructureFailure)?;
            toml::from_str(std::str::from_utf8(&bytes).map_err(|_| HostProblem::Malformed)?)
                .map_err(|_| HostProblem::Malformed)?
        } else {
            Self::default()
        };
        if let Some(value) = environment.get("MAINFRAME_ENV_STORE") {
            config.store_profile = parse_store(value)?;
        }
        if let Some(value) = environment.get("MAINFRAME_ENV_SQLITE_URL") {
            config.sqlite_url.clone_from(value);
        }
        if let Some(value) = environment.get("MAINFRAME_ENV_POSTGRES_URL_REF") {
            config.postgres_url_reference = Some(value.clone());
        }
        if let Some(value) = cli.store_profile {
            config.store_profile = value;
        }
        if let Some(value) = cli.sqlite_url {
            config.sqlite_url = value;
        }
        if let Some(value) = cli.postgres_url_reference {
            config.postgres_url_reference = Some(value);
        }
        if !retention_only {
            if let Some(value) = environment.get("MAINFRAME_ENV_LISTEN") {
                config.listen.clone_from(value);
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_ARTIFACT_STORE") {
                config.artifact_profile = parse_artifact_store(value)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_ARTIFACT_ROOT") {
                config.artifact_root = value.into();
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_MAX_BODY_BYTES") {
                config.max_body_bytes = value.parse().map_err(|_| HostProblem::Malformed)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_MAX_CONCURRENCY") {
                config.max_concurrency = value.parse().map_err(|_| HostProblem::Malformed)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_TIMEOUT_MILLIS") {
                config.timeout_millis = value.parse().map_err(|_| HostProblem::Malformed)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_SHUTDOWN_MILLIS") {
                config.shutdown_millis = value.parse().map_err(|_| HostProblem::Malformed)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_TLS") {
                config.tls.enabled = value.parse().map_err(|_| HostProblem::Malformed)?;
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_TLS_CERTIFICATE_PATH") {
                config.tls.certificate_path = Some(value.into());
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_TLS_KEY_REF") {
                config.tls.private_key_reference = Some(value.clone());
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_BOOTSTRAP_ADMIN") {
                config.bootstrap.administrator = Some(value.clone());
            }
            if let Some(value) = environment.get("MAINFRAME_ENV_BOOTSTRAP_SECRET_REF") {
                config.bootstrap.secret_reference = Some(value.clone());
            }
            if let Some(value) = cli.listen {
                config.listen = value;
            }
            if let Some(value) = cli.artifact_profile {
                config.artifact_profile = value;
            }
            if let Some(value) = cli.artifact_root {
                config.artifact_root = value;
            }
            if let Some(value) = cli.max_body_bytes {
                config.max_body_bytes = value;
            }
            if let Some(value) = cli.max_concurrency {
                config.max_concurrency = value;
            }
            if let Some(value) = cli.timeout_millis {
                config.timeout_millis = value;
            }
            if let Some(value) = cli.shutdown_millis {
                config.shutdown_millis = value;
            }
            if let Some(value) = cli.tls_enabled {
                config.tls.enabled = value;
            }
            if let Some(value) = cli.tls_certificate_path {
                config.tls.certificate_path = Some(value);
            }
            if let Some(value) = cli.tls_private_key_reference {
                config.tls.private_key_reference = Some(value);
            }
            if let Some(value) = cli.bootstrap_administrator {
                config.bootstrap.administrator = Some(value);
            }
            if let Some(value) = cli.bootstrap_secret_reference {
                config.bootstrap.secret_reference = Some(value);
            }
        }
        if retention_only {
            config.validate_for_retention()?;
        } else {
            config.validate()?;
        }
        Ok(config)
    }

    /// Validate the subset required to open a durable store for offline retention.
    pub fn validate_for_retention(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.profile != "core-server"
            || self.retention.policy().is_err()
            || self.store_profile == StoreProfile::Memory
            || (self.store_profile == StoreProfile::Sqlite
                && (self.sqlite_url.is_empty() || sqlite_url_is_ephemeral(&self.sqlite_url)))
            || (self.store_profile == StoreProfile::Postgres
                && self
                    .postgres_url_reference
                    .as_deref()
                    .is_none_or(|reference| !valid_secret_reference(reference)))
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }

    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.profile != "core-server"
            || self
                .listen
                .parse::<SocketAddr>()
                .map_or(true, |address| address.port() == 0)
            || self.max_body_bytes == 0
            || self.max_concurrency == 0
            || self.timeout_millis == 0
            || self.shutdown_millis == 0
            || self.cobol_current_date.as_ref().is_some_and(|value| {
                let bytes = value.as_bytes();
                bytes.len() != 21
                    || !bytes[..16].iter().all(u8::is_ascii_digit)
                    || !matches!(bytes[16], b'+' | b'-')
                    || !bytes[17..].iter().all(u8::is_ascii_digit)
            })
            || self.retention.policy().is_err()
            || (self.artifact_profile == ArtifactProfile::Local
                && self.artifact_root.as_os_str().is_empty())
            || (self.store_profile == StoreProfile::Sqlite && self.sqlite_url.is_empty())
            || (self.store_profile == StoreProfile::Postgres
                && self
                    .postgres_url_reference
                    .as_deref()
                    .is_none_or(|reference| !valid_secret_reference(reference)))
            || (self.store_profile == StoreProfile::Postgres
                && self.artifact_profile != ArtifactProfile::Shared)
            || (self.store_profile != StoreProfile::Postgres
                && self.artifact_profile != ArtifactProfile::Local)
            || (self.tls.enabled
                && (self.tls.certificate_path.is_none()
                    || self
                        .tls
                        .private_key_reference
                        .as_deref()
                        .is_none_or(|reference| !valid_secret_reference(reference))))
            || (self.bootstrap.administrator.is_some() != self.bootstrap.secret_reference.is_some())
            || self
                .bootstrap
                .administrator
                .as_deref()
                .is_some_and(|administrator| !valid_bootstrap_administrator(administrator))
            || self
                .bootstrap
                .secret_reference
                .as_deref()
                .is_some_and(|reference| !valid_secret_reference(reference))
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

fn parse_artifact_store(value: &str) -> Result<ArtifactProfile, HostProblem> {
    match value.to_ascii_lowercase().as_str() {
        "local" => Ok(ArtifactProfile::Local),
        "shared" => Ok(ArtifactProfile::Shared),
        _ => Err(HostProblem::Malformed),
    }
}

fn valid_secret_reference(value: &str) -> bool {
    EnvironmentSecretResolver::parse_reference(value).is_ok()
}

fn valid_bootstrap_administrator(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn parse_store(value: &str) -> Result<StoreProfile, HostProblem> {
    match value.to_ascii_lowercase().as_str() {
        "memory" => Ok(StoreProfile::Memory),
        "sqlite" => Ok(StoreProfile::Sqlite),
        "postgres" => Ok(StoreProfile::Postgres),
        _ => Err(HostProblem::Malformed),
    }
}

fn sqlite_url_is_ephemeral(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains(":memory:")
        || normalized.contains("%3amemory%3a")
        || normalized
            .split_once('?')
            .is_some_and(|(_, query)| query.split('&').any(|field| field.trim() == "mode=memory"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_is_file_then_environment_then_cli() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        std::fs::write(
            &path,
            r#"schema_version=1
profile="core-server"
listen="127.0.0.1:10001"
store_profile="memory"
sqlite_url="sqlite://file"
artifact_root="artifacts"
max_body_bytes=1024
max_concurrency=2
timeout_millis=100
shutdown_millis=100
[tls]
enabled=false
"#,
        )
        .unwrap();
        let config = ServerConfig::from_sources(
            Some(&path),
            &BTreeMap::from([("MAINFRAME_ENV_LISTEN".into(), "127.0.0.1:10002".into())]),
            ConfigOverrides {
                listen: Some("127.0.0.1:10003".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(config.listen, "127.0.0.1:10003");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn incompatible_schema_and_incomplete_tls_fail() {
        let mut config = ServerConfig::default();
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.tls.enabled = false;
        assert!(config.validate().is_ok());
        config.schema_version = 2;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
    }

    #[test]
    fn postgres_requires_the_shared_artifact_profile() {
        let mut config = ServerConfig {
            store_profile: StoreProfile::Postgres,
            postgres_url_reference: Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into()),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        };
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.artifact_profile = ArtifactProfile::Shared;
        assert!(config.validate().is_ok());
        config.store_profile = StoreProfile::Memory;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
    }

    #[test]
    fn retention_watermarks_and_batches_are_bounded() {
        let mut config = ServerConfig {
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        };
        assert!(config.validate().is_ok());
        config.retention.max_batch = mainframe_env_store_api::MAX_RETENTION_BATCH + 1;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.retention.max_batch = 1;
        config.retention.low_watermark_percent = 85;
        config.retention.high_watermark_percent = 85;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.retention.low_watermark_percent = 70;
        config.retention.high_watermark_percent = 101;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.retention.high_watermark_percent = 85;
        config.retention.idempotency_ticks = 0;
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
    }

    #[test]
    fn retention_validation_ignores_serving_only_tls_listener_and_artifact_settings() {
        let mut config = ServerConfig::default();
        config.listen.clear();
        config.tls.enabled = true;
        config.tls.certificate_path = None;
        config.tls.private_key_reference = None;
        config.artifact_root = PathBuf::new();
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        assert!(config.validate_for_retention().is_ok());

        config.store_profile = StoreProfile::Memory;
        assert_eq!(config.validate_for_retention(), Err(HostProblem::Malformed));
    }

    #[test]
    fn retention_loading_ignores_invalid_serving_only_environment() {
        let environment = BTreeMap::from([
            ("MAINFRAME_ENV_STORE".into(), "sqlite".into()),
            (
                "MAINFRAME_ENV_SQLITE_URL".into(),
                "sqlite://retention-maintenance.db?mode=rwc".into(),
            ),
            ("MAINFRAME_ENV_ARTIFACT_STORE".into(), "not-a-store".into()),
            ("MAINFRAME_ENV_TLS".into(), "not-a-boolean".into()),
        ]);
        let config = ServerConfig::from_sources_for_retention(
            None,
            &environment,
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(config.store_profile, StoreProfile::Sqlite);
        assert_eq!(
            config.sqlite_url,
            "sqlite://retention-maintenance.db?mode=rwc"
        );
    }

    #[test]
    fn bootstrap_requires_an_explicit_principal_and_secret_reference_pair() {
        let mut config = ServerConfig::default();
        config.tls.enabled = false;
        config.bootstrap.administrator = Some("ADMIN".into());
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        config.bootstrap.secret_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN".into());
        assert!(config.validate().is_ok());

        let environment = BTreeMap::from([
            ("MAINFRAME_ENV_TLS".into(), "false".into()),
            ("MAINFRAME_ENV_BOOTSTRAP_ADMIN".into(), "ENVADMIN".into()),
            (
                "MAINFRAME_ENV_BOOTSTRAP_SECRET_REF".into(),
                "env-base64:MAINFRAME_ENV_SECRET_ENV_ADMIN".into(),
            ),
        ]);
        let loaded = ServerConfig::from_sources(
            None,
            &environment,
            ConfigOverrides {
                bootstrap_administrator: Some("CLIADMIN".into()),
                bootstrap_secret_reference: Some(
                    "env-base64:MAINFRAME_ENV_SECRET_CLI_ADMIN".into(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(loaded.bootstrap.administrator.as_deref(), Some("CLIADMIN"));
        assert_eq!(
            loaded.bootstrap.secret_reference.as_deref(),
            Some("env-base64:MAINFRAME_ENV_SECRET_CLI_ADMIN")
        );
    }

    #[test]
    fn retention_validation_rejects_every_ephemeral_sqlite_form() {
        for url in [
            ":memory:",
            "sqlite::memory:",
            "sqlite::memory:?cache=shared",
            "sqlite://:memory:",
            "file::memory:?cache=shared",
            "sqlite:file::memory:?cache=shared",
            "sqlite:///%3Amemory%3A",
            "sqlite://retention?mode=memory",
            "sqlite://retention?cache=shared&mode=memory",
            "SQLITE://RETENTION?MODE=MEMORY",
        ] {
            let config = ServerConfig {
                store_profile: StoreProfile::Sqlite,
                sqlite_url: url.into(),
                ..ServerConfig::default()
            };
            assert_eq!(
                config.validate_for_retention(),
                Err(HostProblem::Malformed),
                "ephemeral SQLite URL was accepted: {url}"
            );
        }
    }

    #[test]
    fn configured_listener_and_standalone_secret_provider_fail_closed() {
        let mut config = ServerConfig::default();
        config.tls.enabled = false;
        for listen in ["not-an-address", "127.0.0.1:0"] {
            config.listen = listen.into();
            assert_eq!(config.validate(), Err(HostProblem::Malformed));
        }
        config.listen = "127.0.0.1:10443".into();
        config.store_profile = StoreProfile::Postgres;
        config.artifact_profile = ArtifactProfile::Shared;
        config.postgres_url_reference = Some("secret://unsupported/postgres".into());
        assert_eq!(config.validate(), Err(HostProblem::Malformed));
        assert_eq!(config.validate_for_retention(), Err(HostProblem::Malformed));
    }

    #[test]
    fn retention_loading_applies_only_durable_store_cli_overrides() {
        let config = ServerConfig::from_sources_for_retention(
            None,
            &BTreeMap::new(),
            ConfigOverrides {
                store_profile: Some(StoreProfile::Sqlite),
                sqlite_url: Some("sqlite://retention-cli.db?mode=rwc".into()),
                listen: Some("not-a-listener".into()),
                artifact_profile: Some(ArtifactProfile::Shared),
                max_body_bytes: Some(0),
                tls_enabled: Some(true),
                tls_certificate_path: Some(PathBuf::new()),
                tls_private_key_reference: Some("not-a-reference".into()),
                bootstrap_administrator: Some("TOO-LONG-ADMIN".into()),
                bootstrap_secret_reference: Some("not-a-reference".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(config.store_profile, StoreProfile::Sqlite);
        assert_eq!(config.sqlite_url, "sqlite://retention-cli.db?mode=rwc");
        assert_eq!(config.listen, ServerConfig::default().listen);
        assert_eq!(config.artifact_profile, ArtifactProfile::Local);
        assert_eq!(
            config.max_body_bytes,
            ServerConfig::default().max_body_bytes
        );
        assert_eq!(config.tls, ServerConfig::default().tls);
        assert_eq!(config.bootstrap, BootstrapConfig::default());
    }
}
