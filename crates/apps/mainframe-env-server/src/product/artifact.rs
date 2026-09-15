use super::continuation::{self, OnlineMachineContinuation};
use super::{OnlineExchangeState, ProductServer, normalize_online_name};
use crate::cobol::artifact::{admit_executable_artifact, executable_artifact_metadata};
use mainframe_env_compiler_api::{PublishedArtifact, VersionedArtifactManifest};
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use mainframe_env_host_api::SessionId;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, PlatformStore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineProgramDefinition {
    /// Installed program name.
    pub name: String,
    /// Immutable payload content identity.
    pub artifact: ArtifactRef,
    /// Canonical executable IR bytes.
    pub payload: Vec<u8>,
    /// Explicit current or supported historical manifest.
    pub manifest: VersionedArtifactManifest,
    /// Publisher semantic identity bound to the manifest and payload record.
    pub semantic_identity: String,
}

impl OnlineProgramDefinition {
    /// Build an online installation definition from a current publisher result.
    pub fn current(name: impl Into<String>, artifact: &PublishedArtifact) -> Self {
        Self {
            name: name.into(),
            artifact: published_reference(artifact),
            payload: artifact.payload().to_vec(),
            manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
            semantic_identity: artifact.semantic_id().to_reference(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchProgramDefinition {
    /// Installed program name.
    pub name: String,
    /// Immutable payload content identity.
    pub artifact: ArtifactRef,
    /// Canonical executable IR bytes.
    pub payload: Vec<u8>,
    /// Explicit current or supported historical manifest.
    pub manifest: VersionedArtifactManifest,
    /// Publisher semantic identity bound to the manifest and payload record.
    pub semantic_identity: String,
}

impl BatchProgramDefinition {
    /// Build a batch installation definition from a current publisher result.
    pub fn current(name: impl Into<String>, artifact: &PublishedArtifact) -> Self {
        Self {
            name: name.into(),
            artifact: published_reference(artifact),
            payload: artifact.payload().to_vec(),
            manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
            semantic_identity: artifact.semantic_id().to_reference(),
        }
    }
}

fn published_reference(artifact: &PublishedArtifact) -> ArtifactRef {
    ArtifactRef::new(
        artifact.content_id().to_reference(),
        InvocationLimits::default(),
    )
    .expect("published content identity is bounded")
}

pub(super) fn admitted_record(
    artifact: &ArtifactRef,
    payload: &[u8],
    manifest: &VersionedArtifactManifest,
    semantic_identity: &str,
) -> Result<ArtifactRecord, HostProblem> {
    let payload_digest: [u8; 32] = Sha256::digest(payload).into();
    if artifact.as_str() != format!("sha256:{}", super::hex_digest(&payload_digest)) {
        return Err(HostProblem::IdempotencyConflict);
    }
    let record = ArtifactRecord {
        artifact: artifact.clone(),
        media_type: "application/vnd.mainframe-env.core-mir".into(),
        payload_digest,
        payload: payload.to_vec(),
        executable: Some(executable_artifact_metadata(
            manifest,
            semantic_identity,
            &payload_digest,
        )),
    };
    admit_executable_artifact(&record)?;
    Ok(record)
}

pub(super) struct InstalledArtifactCatalog {
    pub(super) online_programs: BTreeMap<String, ArtifactRef>,
    pub(super) online_transactions: BTreeMap<String, String>,
}

pub(super) fn preflight_installed(
    store: &dyn PlatformStore,
    artifacts: &dyn ArtifactStore,
) -> Result<InstalledArtifactCatalog, HostProblem> {
    let mut online_programs = BTreeMap::new();
    for row in store
        .list_provider_state("online-program", 4096)
        .map_err(super::store_error)?
    {
        let artifact = ArtifactRef::new(
            String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = artifacts
            .get_artifact(&artifact)
            .map_err(super::store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        admit_executable_artifact(&record)?;
        if online_programs.insert(row.key, artifact).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let mut online_transactions = BTreeMap::new();
    for row in store
        .list_provider_state("online-transaction", 4096)
        .map_err(super::store_error)?
    {
        let program =
            String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        if !online_programs.contains_key(&program)
            || online_transactions.insert(row.key, program).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    for row in store
        .list_provider_state("batch-program", 4096)
        .map_err(super::store_error)?
    {
        let artifact = ArtifactRef::new(
            String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = artifacts
            .get_artifact(&artifact)
            .map_err(super::store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        admit_executable_artifact(&record)?;
    }
    Ok(InstalledArtifactCatalog {
        online_programs,
        online_transactions,
    })
}

pub(super) fn preflight_one(
    artifacts: &dyn ArtifactStore,
    artifact: &ArtifactRef,
) -> Result<(), HostProblem> {
    let record = artifacts
        .get_artifact(artifact)
        .map_err(super::store_error)?
        .ok_or(HostProblem::NotFound)?;
    admit_executable_artifact(&record).map(|_| ())
}

impl ProductServer {
    pub(super) fn preflight_online_program(
        &self,
        program: &str,
    ) -> Result<(String, ArtifactRef), HostProblem> {
        let program = normalize_online_name(program, 128)?;
        let artifact = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&program)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        preflight_one(self.artifacts.as_ref(), &artifact)?;
        Ok((program, artifact))
    }

    pub(super) fn preflight_online_continuation(
        &self,
        session: &SessionId,
        expected_program: Option<&str>,
    ) -> Result<Option<OnlineMachineContinuation>, HostProblem> {
        let continuation = self.online_machine_continuation(session)?;
        let Some(saved) = continuation.as_ref() else {
            return Ok(continuation);
        };
        if saved.transfer.is_none()
            && expected_program
                .map(|program| normalize_online_name(program, 128))
                .transpose()?
                .is_some_and(|program| program != saved.program)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let artifact = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&saved.program)
            .cloned()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if artifact != saved.artifact {
            return Err(HostProblem::InfrastructureFailure);
        }
        preflight_one(self.artifacts.as_ref(), &artifact)?;
        continuation::preflight_provider_generations(
            self.host.as_ref(),
            &saved.provider_generations,
        )?;
        Ok(continuation)
    }

    pub(super) fn preflight_online_exchange_state(
        &self,
        state: &OnlineExchangeState,
    ) -> Result<(), HostProblem> {
        let invocation = self.online_exchange_invocation(state)?;
        let artifact = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&state.program)
            .cloned()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if artifact != invocation.artifact {
            return Err(HostProblem::InfrastructureFailure);
        }
        preflight_one(self.artifacts.as_ref(), &artifact)?;
        continuation::preflight_provider_generations(
            self.host.as_ref(),
            &invocation.provider_generations,
        )
    }
}
