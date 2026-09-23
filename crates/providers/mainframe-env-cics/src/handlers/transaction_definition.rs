//! Durable local transaction identity for immediate START commands.

use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use sha2::{Digest, Sha256};

/// Resolve a local, installed transaction before admitting a START task.
///
/// The server owns these versioned installation rows. A missing transaction
/// produces the source TRANSIDERR condition; a damaged retained definition is
/// infrastructure failure and cannot be mistaken for an undefined resource.
impl CicsService {
    /// Resolve the installed local program for a START target transaction.
    ///
    /// Undefined names return source TRANSIDERR 28/0. Damaged durable rows
    /// fail as infrastructure errors so callers never schedule an unknown task.
    pub fn local_transaction_program(&self, transaction: &str) -> Result<String, HostProblem> {
        local_program(self, transaction)
    }
}

fn local_program(service: &CicsService, transaction: &str) -> Result<String, HostProblem> {
    let transaction = normalized(transaction, 4)?;
    let row = service
        .store
        .get_provider_state("online-transaction", &transaction)
        .map_err(store_error)?
        .ok_or_else(|| HostProblem::Condition {
            name: "TRANSIDERR".into(),
            response: 28,
            response2: 0,
        })?;
    if row.namespace != "online-transaction" || row.key != transaction || row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let program = normalized(
        std::str::from_utf8(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
        128,
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if program.as_bytes() != row.payload {
        return Err(HostProblem::InfrastructureFailure);
    }
    installed_program_artifact(service, &program)?.ok_or(HostProblem::InfrastructureFailure)?;
    Ok(program)
}

pub(super) fn installed_program_artifact(
    service: &CicsService,
    program: &str,
) -> Result<Option<ArtifactRef>, HostProblem> {
    let program = normalized(program, 128)?;
    let Some(installed) = service
        .store
        .get_provider_state("online-program", &program)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    if installed.namespace != "online-program" || installed.key != program || installed.version != 1
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let artifact =
        std::str::from_utf8(&installed.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    let artifact = ArtifactRef::new(artifact, InvocationLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if artifact.as_str().len() != 71
        || !artifact.as_str().starts_with("sha256:")
        || !artifact.as_str()[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let artifacts = service
        .artifacts
        .get()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let executable = artifacts
        .get_artifact(&artifact)
        .map_err(store_error)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    let digest: [u8; 32] = Sha256::digest(&executable.payload).into();
    if executable.artifact != artifact
        || executable.media_type != "application/vnd.mainframe-env.core-mir"
        || executable.payload_digest != digest
        || executable
            .executable
            .as_ref()
            .is_none_or(|metadata| !metadata.validates_payload(&digest))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Some(artifact))
}

fn normalized(value: &str, max: usize) -> Result<String, HostProblem> {
    let name = value.trim().to_ascii_uppercase();
    if name.is_empty() || name.len() > max || !name.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(name)
    }
}
