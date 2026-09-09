use mainframe_env_compiler::{core_mir_catalog, core_mir_profile};
use mainframe_env_compiler_api::{
    ARTIFACT_CONTRACT, ArtifactLimits, ArtifactManifest, ArtifactManifestV2, CompileOptions,
    CompileTarget, LEGACY_ARTIFACT_CONTRACT, PublishedArtifact, ValidatedArtifact,
    VersionedArtifactManifest,
};
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use mainframe_env_ir::{
    COBOL_EFFECTIVE_ARITH_OPTION, COBOL_EFFECTIVE_DISPSIGN_OPTION, COBOL_EFFECTIVE_LP_OPTION,
    CodecLimits,
};
use mainframe_env_store_api::{ArtifactRecord, ExecutableArtifactMetadata};
use std::collections::{BTreeMap, BTreeSet};

use super::CobolProgram;

pub(crate) const COBOL_REFERENCE_COMPATIBILITY_PROFILE: &str = "mainframe-env.cobol.reference@1";
const COBOL_COMPILER_GENERATION: &str = concat!("mainframe-env-cobol-", env!("CARGO_PKG_VERSION"));

fn supported_host_interfaces() -> BTreeSet<String> {
    ["mainframe-env.host@1", "mainframe-env.cics@1"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn supported_options(options: &BTreeMap<String, String>) -> bool {
    options.len() == 3
        && matches!(
            options
                .get(COBOL_EFFECTIVE_ARITH_OPTION)
                .map(String::as_str),
            Some("compatible" | "extended")
        )
        && matches!(
            options
                .get(COBOL_EFFECTIVE_DISPSIGN_OPTION)
                .map(String::as_str),
            Some("compatible" | "separate")
        )
        && matches!(
            options.get(COBOL_EFFECTIVE_LP_OPTION).map(String::as_str),
            Some("32" | "64")
        )
}

pub(crate) fn executable_artifact_metadata(
    manifest: &VersionedArtifactManifest,
    semantic_identity: &str,
    payload_digest: &[u8; 32],
) -> ExecutableArtifactMetadata {
    let metadata = match manifest {
        VersionedArtifactManifest::V2(manifest) => ExecutableArtifactMetadata {
            artifact_contract: LEGACY_ARTIFACT_CONTRACT.into(),
            compatibility_profile: COBOL_REFERENCE_COMPATIBILITY_PROFILE.into(),
            compiler_generation: manifest.compiler_generation.clone(),
            target: manifest.target.as_str().into(),
            options: manifest.options.values().clone(),
            host_interfaces: manifest.host_interfaces.clone(),
            ir_contract: manifest.ir_contract.clone(),
            dialect_contracts: None,
            semantic_identity: semantic_identity.into(),
            manifest_payload_digest: [0; 32],
        },
        VersionedArtifactManifest::V3(manifest) => ExecutableArtifactMetadata {
            artifact_contract: ARTIFACT_CONTRACT.into(),
            compatibility_profile: COBOL_REFERENCE_COMPATIBILITY_PROFILE.into(),
            compiler_generation: manifest.compiler_generation.clone(),
            target: manifest.target.as_str().into(),
            options: manifest.options.values().clone(),
            host_interfaces: manifest.host_interfaces.clone(),
            ir_contract: manifest.ir_contract.clone(),
            dialect_contracts: Some(manifest.dialect_contracts.clone()),
            semantic_identity: semantic_identity.into(),
            manifest_payload_digest: [0; 32],
        },
    };
    metadata.bind_to_payload(payload_digest)
}

fn versioned_manifest(
    metadata: &ExecutableArtifactMetadata,
) -> Result<VersionedArtifactManifest, HostProblem> {
    if metadata.compatibility_profile != COBOL_REFERENCE_COMPATIBILITY_PROFILE
        || metadata.compiler_generation != COBOL_COMPILER_GENERATION
        || metadata.target != "reference"
        || !supported_options(&metadata.options)
        || metadata.host_interfaces != supported_host_interfaces()
    {
        return Err(HostProblem::ProviderFailure);
    }
    let target =
        CompileTarget::new(metadata.target.clone()).map_err(|_| HostProblem::ProviderFailure)?;
    let options =
        CompileOptions::new(metadata.options.clone()).map_err(|_| HostProblem::ProviderFailure)?;
    match metadata.artifact_contract.as_str() {
        LEGACY_ARTIFACT_CONTRACT if metadata.dialect_contracts.is_none() => {
            Ok(VersionedArtifactManifest::V2(ArtifactManifestV2 {
                compiler_generation: metadata.compiler_generation.clone(),
                target,
                options,
                host_interfaces: metadata.host_interfaces.clone(),
                ir_contract: metadata.ir_contract.clone(),
            }))
        }
        ARTIFACT_CONTRACT => Ok(VersionedArtifactManifest::V3(ArtifactManifest {
            compiler_generation: metadata.compiler_generation.clone(),
            target,
            options,
            host_interfaces: metadata.host_interfaces.clone(),
            ir_contract: metadata.ir_contract.clone(),
            dialect_contracts: metadata
                .dialect_contracts
                .clone()
                .ok_or(HostProblem::ProviderFailure)?,
        })),
        _ => Err(HostProblem::ProviderFailure),
    }
}

pub(crate) fn admit_executable_artifact(
    record: &ArtifactRecord,
) -> Result<ValidatedArtifact, HostProblem> {
    if record.media_type != "application/vnd.mainframe-env.core-mir" {
        return Err(HostProblem::ProviderFailure);
    }
    if !record
        .executable
        .as_ref()
        .is_some_and(|metadata| metadata.validates_payload(&record.payload_digest))
    {
        return Err(HostProblem::ProviderFailure);
    }
    let manifest = versioned_manifest(
        record
            .executable
            .as_ref()
            .ok_or(HostProblem::ProviderFailure)?,
    )?;
    let artifact = ValidatedArtifact::read(
        manifest,
        &record.payload,
        &core_mir_catalog(),
        &core_mir_profile(),
        CodecLimits::default(),
        ArtifactLimits::default(),
    )
    .map_err(|_| HostProblem::ProviderFailure)?;
    if record.payload_digest != *artifact.content_id().as_bytes()
        || record.artifact.as_str() != artifact.content_id().to_reference()
    {
        return Err(HostProblem::ProviderFailure);
    }
    Ok(artifact)
}

pub(crate) fn published_artifact_record(
    artifact: &PublishedArtifact,
) -> Result<ArtifactRecord, HostProblem> {
    let manifest = VersionedArtifactManifest::V3(artifact.manifest().clone());
    Ok(ArtifactRecord {
        artifact: ArtifactRef::new(
            artifact.content_id().to_reference(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?,
        media_type: "application/vnd.mainframe-env.core-mir".into(),
        payload_digest: *artifact.content_id().as_bytes(),
        payload: artifact.payload().to_vec(),
        executable: Some(executable_artifact_metadata(
            &manifest,
            &artifact.semantic_id().to_reference(),
            artifact.content_id().as_bytes(),
        )),
    })
}

pub(super) struct AdmittedProgram {
    pub(super) artifact: ArtifactRef,
    pub(super) executable: ValidatedArtifact,
    pub(super) name: String,
}

impl CobolProgram {
    pub(super) fn preflight_installed_program(
        &self,
        program: &str,
        batch_only: bool,
    ) -> Result<AdmittedProgram, HostProblem> {
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let name = program.to_ascii_uppercase();
        let catalog = match store
            .get_provider_state("batch-program", &name)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            Some(catalog) => catalog,
            None if batch_only => return Err(HostProblem::NotFound),
            None => store
                .get_provider_state("online-program", &name)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .ok_or_else(|| HostProblem::Condition {
                    name: format!("PROGRAM-NOTFOUND:{name}"),
                    response: -7,
                    response2: 0,
                })?,
        };
        let artifact = ArtifactRef::new(
            String::from_utf8(catalog.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = artifacts
            .get_artifact(&artifact)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or_else(|| {
                if batch_only {
                    HostProblem::NotFound
                } else {
                    HostProblem::Condition {
                        name: format!("ARTIFACT-NOTFOUND:{name}"),
                        response: -8,
                        response2: 0,
                    }
                }
            })?;
        let executable = admit_executable_artifact(&record)?;
        Ok(AdmittedProgram {
            artifact,
            executable,
            name,
        })
    }
}

pub(super) fn admit_published_artifact(
    artifact: &PublishedArtifact,
) -> Result<ValidatedArtifact, HostProblem> {
    admit_executable_artifact(&published_artifact_record(artifact)?)
}
