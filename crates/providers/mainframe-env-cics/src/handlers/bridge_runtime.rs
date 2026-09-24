//! Immutable bridge callback state retained across target task recovery.

use crate::service::{CicsLimits, CicsService, store_error};
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits, PrincipalId};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use serde::{Deserialize, Serialize};

const NAMESPACE: &str = "cics-bridge-runtime-v1";
const SCHEMA: &str = "mainframe-env.cics.bridge-runtime@1";
const MAX_FRAME_BYTES: usize = 32_767;

/// Selected BRXA state shared by the target task and its bridge exit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsBridgeRuntime {
    pub schema: String,
    pub request_id: String,
    pub run_unit: String,
    pub principal: String,
    pub exit: String,
    pub artifact: String,
    pub abi_version: u32,
    pub bind_code: [u8; 2],
    pub commarea_address: u32,
    pub commarea_capacity: usize,
    pub bound_frame: Vec<u8>,
    pub start_code: [u8; 2],
}

impl CicsBridgeRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: String,
        run_unit: String,
        principal: String,
        exit: String,
        artifact: String,
        abi_version: u32,
        bind_code: [u8; 2],
        commarea_address: u32,
        commarea_capacity: usize,
        bound_frame: Vec<u8>,
        start_code: [u8; 2],
    ) -> Result<Self, HostProblem> {
        let record = Self {
            schema: SCHEMA.into(),
            request_id,
            run_unit,
            principal,
            exit,
            artifact,
            abi_version,
            bind_code,
            commarea_address,
            commarea_capacity,
            bound_frame,
            start_code,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema != SCHEMA
            || self.request_id.len() != 24
            || !self
                .request_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.run_unit != format!("run-cics-bridge-{}", self.request_id)
            || PrincipalId::new(&self.principal, InvocationLimits::default()).is_err()
            || self.exit.is_empty()
            || self.exit.len() > 8
            || !self
                .exit
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            || self.artifact.len() != 71
            || !self.artifact.starts_with("sha256:")
            || !self.artifact[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || ArtifactRef::new(&self.artifact, InvocationLimits::default()).is_err()
            || self.abi_version == 0
            || self.bind_code.contains(&0)
            || self.commarea_address == 0
            || self.commarea_capacity < 56 + 180 + 108 + 1
            || self.commarea_capacity > MAX_FRAME_BYTES
            || self.bound_frame.len() > self.commarea_capacity
            || self.bound_frame.len() < 56 + 180 + 48
            || self.bound_frame.len() > MAX_FRAME_BYTES
            || &self.bound_frame[..8] != b">BRAREA "
            || &self.bound_frame[56..64] != b">BRTRANA"
            || &self.bound_frame[56 + 180..56 + 180 + 8] != b">BRCOMMA"
            || self.bound_frame[0x0c..0x10] != self.abi_version.to_be_bytes()
            || !matches!(&self.start_code, b"S " | b"SD" | b"TD")
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

impl CicsService {
    /// Persist the exact post-Bind image before the target task can execute.
    pub fn register_bridge_runtime(&self, runtime: CicsBridgeRuntime) -> Result<(), HostProblem> {
        runtime.validate()?;
        let key = runtime.request_id.clone();
        let payload =
            serde_json::to_vec(&runtime).map_err(|_| HostProblem::InfrastructureFailure)?;
        if let Some(current) = self
            .store
            .get_provider_state(NAMESPACE, &key)
            .map_err(store_error)?
        {
            return if decode(&current)? == runtime {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key,
                    version: 1,
                    payload,
                },
                None,
            )
            .map_err(store_error)
    }

    /// Load one validated bridge frame for a target invocation.
    pub fn bridge_runtime(&self, request_id: &str) -> Result<CicsBridgeRuntime, HostProblem> {
        let row = self
            .store
            .get_provider_state(NAMESPACE, request_id)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        decode(&row)
    }

    /// Delete bridge state after the target task and its continuation settle.
    pub fn release_bridge_runtime(&self, request_id: &str) -> Result<(), HostProblem> {
        if let Some(row) = self
            .store
            .get_provider_state(NAMESPACE, request_id)
            .map_err(store_error)?
        {
            decode(&row)?;
            self.store
                .delete_provider_state(NAMESPACE, request_id, row.version)
                .map_err(store_error)?;
        }
        Ok(())
    }
}

pub(in crate::service) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_runs.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_runs {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in rows {
        decode(&row)?;
    }
    Ok(())
}

fn decode(row: &ProviderStateRecord) -> Result<CicsBridgeRuntime, HostProblem> {
    if row.namespace != NAMESPACE
        || row.version != 1
        || row.payload.len() > MAX_FRAME_BYTES * 5 + 2048
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let runtime: CicsBridgeRuntime =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if row.key != runtime.request_id
        || runtime.validate().is_err()
        || serde_json::to_vec(&runtime).map_err(|_| HostProblem::InfrastructureFailure)?
            != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(runtime)
}
