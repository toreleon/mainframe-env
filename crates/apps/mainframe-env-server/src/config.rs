use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::RetentionPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    pub tls_enabled: Option<bool>,
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
            if let Some(value) = environment.get("MAINFRAME_ENV_TLS") {
                config.tls.enabled = value.parse().map_err(|_| HostProblem::Malformed)?;
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
            if let Some(value) = cli.tls_enabled {
                config.tls.enabled = value;
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
                    .is_none_or(str::is_empty))
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }

    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.profile != "core-server"
            || self.listen.is_empty()
            || self.max_body_bytes == 0
            || self.max_concurrency == 0
            || self.timeout_millis == 0
            || self.shutdown_millis == 0
            || self.retention.policy().is_err()
            || (self.artifact_profile == ArtifactProfile::Local
                && self.artifact_root.as_os_str().is_empty())
            || (self.store_profile == StoreProfile::Sqlite && self.sqlite_url.is_empty())
            || (self.store_profile == StoreProfile::Postgres
                && self
                    .postgres_url_reference
                    .as_deref()
                    .is_none_or(str::is_empty))
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
                        .is_none_or(str::is_empty)))
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
listen="file:1"
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
            &BTreeMap::from([("MAINFRAME_ENV_LISTEN".into(), "env:2".into())]),
            ConfigOverrides {
                listen: Some("cli:3".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(config.listen, "cli:3");
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
            postgres_url_reference: Some("secret://postgres".into()),
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
}
