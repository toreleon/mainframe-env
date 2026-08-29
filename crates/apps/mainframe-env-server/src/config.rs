use mainframe_env_host_api::HostProblem;
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub enabled: bool,
    pub certificate_path: Option<PathBuf>,
    pub private_key_reference: Option<String>,
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
    pub artifact_root: PathBuf,
    pub max_body_bytes: usize,
    pub max_concurrency: usize,
    pub timeout_millis: u64,
    pub shutdown_millis: u64,
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
            artifact_root: PathBuf::from("mainframe-env-artifacts"),
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrency: 256,
            timeout_millis: 30_000,
            shutdown_millis: 30_000,
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
    pub artifact_root: Option<PathBuf>,
    pub tls_enabled: Option<bool>,
}

impl ServerConfig {
    pub fn from_sources(
        file: Option<&Path>,
        environment: &BTreeMap<String, String>,
        cli: ConfigOverrides,
    ) -> Result<Self, HostProblem> {
        let mut config = if let Some(path) = file {
            let bytes = std::fs::read(path).map_err(|_| HostProblem::InfrastructureFailure)?;
            toml::from_str(std::str::from_utf8(&bytes).map_err(|_| HostProblem::Malformed)?)
                .map_err(|_| HostProblem::Malformed)?
        } else {
            Self::default()
        };
        if let Some(value) = environment.get("MAINFRAME_ENV_LISTEN") {
            config.listen.clone_from(value);
        }
        if let Some(value) = environment.get("MAINFRAME_ENV_STORE") {
            config.store_profile = parse_store(value)?;
        }
        if let Some(value) = environment.get("MAINFRAME_ENV_SQLITE_URL") {
            config.sqlite_url.clone_from(value);
        }
        if let Some(value) = environment.get("MAINFRAME_ENV_POSTGRES_URL_REF") {
            config.postgres_url_reference = Some(value.clone());
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
        if let Some(value) = cli.store_profile {
            config.store_profile = value;
        }
        if let Some(value) = cli.sqlite_url {
            config.sqlite_url = value;
        }
        if let Some(value) = cli.postgres_url_reference {
            config.postgres_url_reference = Some(value);
        }
        if let Some(value) = cli.artifact_root {
            config.artifact_root = value;
        }
        if let Some(value) = cli.tls_enabled {
            config.tls.enabled = value;
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.profile != "core-server"
            || self.listen.is_empty()
            || self.max_body_bytes == 0
            || self.max_concurrency == 0
            || self.timeout_millis == 0
            || self.shutdown_millis == 0
            || self.artifact_root.as_os_str().is_empty()
            || (self.store_profile == StoreProfile::Sqlite && self.sqlite_url.is_empty())
            || (self.store_profile == StoreProfile::Postgres
                && self
                    .postgres_url_reference
                    .as_deref()
                    .is_none_or(str::is_empty))
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

fn parse_store(value: &str) -> Result<StoreProfile, HostProblem> {
    match value.to_ascii_lowercase().as_str() {
        "memory" => Ok(StoreProfile::Memory),
        "sqlite" => Ok(StoreProfile::Sqlite),
        "postgres" => Ok(StoreProfile::Postgres),
        _ => Err(HostProblem::Malformed),
    }
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
}
