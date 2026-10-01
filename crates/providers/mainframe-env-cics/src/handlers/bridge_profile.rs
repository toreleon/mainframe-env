//! Immutable deployment ABI values for installed START BREXIT exit programs.
//!
//! The pinned BRARC topic names, but does not number, the current version and
//! Bind command code. Deployments must bind those values explicitly to the
//! exact exit artifact; the runtime does not synthesize IBM constants.

use super::transaction_definition::installed_program_artifact;
use crate::service::{CicsLimits, CicsService, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const NAMESPACE: &str = "cics-bridge-abi-profile-v1";
const SCHEMA: &str = "mainframe-env.cics.bridge-abi-profile@1";

/// Deployment supplied ABI constants for one installed bridge exit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsBridgeAbiProfile {
    pub exit: String,
    pub version: u32,
    pub bind_code: [u8; 2],
}

/// Immutable ABI constants and artifact identity selected for one start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsBridgeAbiSelection {
    pub exit: String,
    pub artifact: String,
    pub version: u32,
    pub bind_code: [u8; 2],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileRecord {
    schema: String,
    exit: String,
    artifact: String,
    version: u32,
    bind_code: [u8; 2],
}

impl CicsService {
    /// Register one immutable ABI profile per installed exit program.
    pub fn register_bridge_abi_profiles(
        &self,
        profiles: &[CicsBridgeAbiProfile],
    ) -> Result<(), HostProblem> {
        if profiles.is_empty() || profiles.len() > self.limits.max_programs {
            return Err(HostProblem::Malformed);
        }
        let mut seen = BTreeSet::new();
        let mut writes = Vec::new();
        for profile in profiles {
            let exit = exact_exit(&profile.exit)?;
            if !seen.insert(exit.clone()) || !valid_constants(profile.version, profile.bind_code) {
                return Err(HostProblem::Malformed);
            }
            let artifact = installed_program_artifact(self, &exit)?
                .ok_or(HostProblem::NotFound)?
                .as_str()
                .to_string();
            let record = ProfileRecord {
                schema: SCHEMA.into(),
                exit: exit.clone(),
                artifact,
                version: profile.version,
                bind_code: profile.bind_code,
            };
            if let Some(current) = self
                .store
                .get_provider_state(NAMESPACE, &exit)
                .map_err(store_error)?
            {
                if decode(&current)? != record {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: exit,
                    version: 1,
                    payload: encode(&record)?,
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        Ok(())
    }

    /// Resolve the configured ABI values for the exact installed exit artifact.
    pub fn resolve_bridge_abi(
        &self,
        exit: &str,
        artifact: &str,
    ) -> Result<CicsBridgeAbiSelection, HostProblem> {
        let exit = exact_exit(exit)?;
        let row = self
            .store
            .get_provider_state(NAMESPACE, &exit)
            .map_err(store_error)?
            .ok_or_else(missing_profile)?;
        let record = decode(&row)?;
        if record.artifact != artifact
            || installed_program_artifact(self, &exit)?
                .is_none_or(|installed| installed.as_str() != artifact)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(CicsBridgeAbiSelection {
            exit,
            artifact: record.artifact,
            version: record.version,
            bind_code: record.bind_code,
        })
    }
}

pub(in crate::service) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_programs.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_programs {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in rows {
        decode(&row)?;
    }
    Ok(())
}

fn missing_profile() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 12,
    }
}

fn exact_exit(value: &str) -> Result<String, HostProblem> {
    if value.is_empty()
        || value.len() > 8
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value.into())
    }
}

fn valid_constants(version: u32, bind_code: [u8; 2]) -> bool {
    version != 0
        && bind_code.iter().all(u8::is_ascii_graphic)
        && !matches!(&bind_code, b"IN" | b"TM" | b"AB")
}

fn encode(record: &ProfileRecord) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<ProfileRecord, HostProblem> {
    if row.namespace != NAMESPACE || row.version != 1 || row.payload.len() > 256 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: ProfileRecord =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if record.schema != SCHEMA
        || exact_exit(&record.exit).is_err()
        || !valid_constants(record.version, record.bind_code)
        || !record.artifact.starts_with("sha256:")
        || record.artifact.len() != 71
        || !record.artifact[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || row.key != record.exit
        || encode(&record)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}
