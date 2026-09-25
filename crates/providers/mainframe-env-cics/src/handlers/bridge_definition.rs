//! Durable default bridge-exit selection for START BREXIT.

use super::transaction_definition::installed_program_artifact;
use crate::service::{CicsLimits, CicsService, store_error};
use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const NAMESPACE: &str = "cics-bridge-default-v1";
const SCHEMA: &str = "mainframe-env.cics.bridge-default@1";

/// A transaction resource's default user-written bridge exit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsBridgeExitDefault {
    /// Local target transaction identity (one to four characters).
    pub transaction: String,
    /// Installed exit program identity (one to eight characters).
    pub exit: String,
}

/// Resolved immutable program generation selected for a bridge exit call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsBridgeExitSelection {
    /// Local target transaction identity.
    pub transaction: String,
    /// Explicit or default bridge exit program.
    pub exit: String,
    /// Verified installed executable artifact.
    pub artifact: ArtifactRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BridgeDefaultRecord {
    schema: String,
    transaction: String,
    exit: String,
}

impl CicsService {
    /// Register immutable default BREXIT names on installed local transactions.
    ///
    /// An exact repeat is idempotent. A changed definition conflicts, including
    /// after SQLite reopen; a missing target or exit is never registered.
    pub fn register_bridge_exit_defaults(
        &self,
        definitions: &[CicsBridgeExitDefault],
    ) -> Result<(), HostProblem> {
        if definitions.is_empty() || definitions.len() > self.limits.max_programs {
            return Err(HostProblem::Malformed);
        }
        let mut seen = BTreeSet::new();
        let mut writes = Vec::new();
        for definition in definitions {
            let transaction = exact_name(&definition.transaction, 4)?;
            let exit = exact_name(&definition.exit, 8)?;
            if !seen.insert(transaction.clone()) {
                return Err(HostProblem::Malformed);
            }
            self.local_transaction_program(&transaction)?;
            if installed_program_artifact(self, &exit)?.is_none() {
                return Err(HostProblem::NotFound);
            }
            let record = BridgeDefaultRecord {
                schema: SCHEMA.into(),
                transaction: transaction.clone(),
                exit,
            };
            if let Some(current) = self
                .store
                .get_provider_state(NAMESPACE, &transaction)
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
                    key: transaction,
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

    /// Resolve the explicit exit or the transaction resource's default exit.
    ///
    /// Absent defaults return the source PGMIDERR 27/0 condition. The returned
    /// artifact is verified against the installed executable before dispatch.
    pub fn resolve_bridge_exit(
        &self,
        transaction: &str,
        explicit: Option<&str>,
    ) -> Result<CicsBridgeExitSelection, HostProblem> {
        let transaction = exact_name(transaction, 4)?;
        self.local_transaction_program(&transaction)?;
        let exit = match explicit {
            Some(name) => exact_name(name, 8)?,
            None => {
                let row = self
                    .store
                    .get_provider_state(NAMESPACE, &transaction)
                    .map_err(store_error)?
                    .ok_or_else(missing_exit)?;
                decode(&row)?.exit
            }
        };
        let artifact = installed_program_artifact(self, &exit)?.ok_or_else(missing_exit)?;
        Ok(CicsBridgeExitSelection {
            transaction,
            exit,
            artifact,
        })
    }
}

pub(in crate::service::handlers) fn validate_store(
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

fn missing_exit() -> HostProblem {
    HostProblem::Condition {
        name: "PGMIDERR".into(),
        response: 27,
        response2: 0,
    }
}

fn exact_name(value: &str, maximum: usize) -> Result<String, HostProblem> {
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value.into())
    }
}

fn encode(record: &BridgeDefaultRecord) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<BridgeDefaultRecord, HostProblem> {
    if row.namespace != NAMESPACE || row.version != 1 || row.payload.len() > 256 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: BridgeDefaultRecord =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if record.schema != SCHEMA
        || exact_name(&record.transaction, 4).is_err()
        || exact_name(&record.exit, 8).is_err()
        || row.key != record.transaction
        || encode(&record)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}
