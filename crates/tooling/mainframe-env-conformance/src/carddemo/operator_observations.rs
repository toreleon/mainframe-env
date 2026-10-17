//! Private witnesses for the existing in-process operator implementations.
//! Public status/count receipts cannot construct these witnesses.
use super::*;
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, PlatformStore, ProviderStateRecord};

fn problem() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.full.operator_observation_refused",
        "owned operator inputs, installation, compilation or workflow differs",
    )
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Inputs {
    inventory: PathBuf,
    inventory_sha256: String,
    corpus: PathBuf,
    commit: String,
    content_sha256: String,
}

impl Inputs {
    pub(super) fn verify(inventory: &Path) -> Result<Self, CorpusProblem> {
        let receipt = verify_carddemo_corpus_from_env(inventory)?;
        let corpus = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(problem)?);
        Ok(Self {
            inventory: fs::canonicalize(inventory).map_err(|_| problem())?,
            inventory_sha256: format!(
                "{:x}",
                Sha256::digest(fs::read(inventory).map_err(|_| problem())?)
            ),
            corpus: fs::canonicalize(corpus).map_err(|_| problem())?,
            commit: receipt.commit,
            content_sha256: receipt.content.sha256,
        })
    }

    fn current(&self) -> Result<(), CorpusProblem> {
        if *self != Self::verify(&self.inventory)? {
            return Err(problem());
        }
        Ok(())
    }
}

pub(super) struct OnlineProof {
    inputs: Inputs,
}

// The existing source/seed/catalog comparisons and durable installation witness
// are consumed here after the owner has checked both drains and artifact cleanup.
pub(super) fn finish_online(
    inputs: &Inputs,
    seed: seed_observations::SeedProof,
    catalog: catalog_observations::CatalogProof,
    installation: source_artifact_binding::InstallationProof,
    observations: &mut RouteObservations,
) -> Result<OnlineProof, CorpusProblem> {
    inputs.current()?;
    seed.observe(observations)?;
    catalog.bind_resource_catalog(&installation, observations)?;
    catalog.bind(observations)?;
    installation.bind(observations)?;
    Ok(OnlineProof {
        inputs: inputs.clone(),
    })
}

pub(super) struct BatchExpectation {
    inputs: Inputs,
    expected: BTreeMap<String, (ProviderStateRecord, ArtifactRecord)>,
    comparisons: usize,
    artifact_owner: Option<(PathBuf, u64, u64)>,
}

pub(super) struct BatchProof {
    inputs: Inputs,
}

fn expected_programs(
    definitions: &[BatchProgramDefinition],
) -> Result<BTreeMap<String, (ProviderStateRecord, ArtifactRecord)>, CorpusProblem> {
    let mut expected = BTreeMap::new();
    for definition in definitions {
        let online = OnlineProgramDefinition {
            name: definition.name.clone(),
            artifact: definition.artifact.clone(),
            payload: definition.payload.clone(),
            manifest: definition.manifest.clone(),
            semantic_identity: definition.semantic_identity.clone(),
        };
        let artifact = source_artifact_binding::publisher_record(&online)?;
        let row = ProviderStateRecord {
            namespace: "batch-program".into(),
            key: definition.name.clone(),
            version: 1,
            payload: definition.artifact.as_str().as_bytes().to_vec(),
        };
        if expected
            .insert(definition.name.clone(), (row, artifact))
            .is_some()
        {
            return Err(problem());
        }
    }
    // This is the existing eleven named programs plus CBSTM03B, not a new
    // acceptance floor. The caller's pinned JCL/call closure chooses them.
    if expected.len() != 12 {
        return Err(problem());
    }
    Ok(expected)
}

#[cfg(unix)]
fn artifact_owner(root: &Path) -> Result<(PathBuf, u64, u64), CorpusProblem> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(root).map_err(|_| problem())?;
    if !metadata.is_dir() {
        return Err(problem());
    }
    Ok((
        fs::canonicalize(root).map_err(|_| problem())?,
        metadata.dev(),
        metadata.ino(),
    ))
}

#[cfg(not(unix))]
fn artifact_owner(_: &Path) -> Result<(PathBuf, u64, u64), CorpusProblem> {
    Err(problem())
}

fn compare_programs(
    expected: &BTreeMap<String, (ProviderStateRecord, ArtifactRecord)>,
    store: &dyn PlatformStore,
    artifact_root: &Path,
) -> Result<(), CorpusProblem> {
    let artifacts =
        LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024).map_err(|_| problem())?;
    for (name, (expected_row, expected_artifact)) in expected {
        let row = store
            .get_provider_state("batch-program", name)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let artifact = artifacts
            .get_artifact(&expected_artifact.artifact)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        if row != *expected_row || artifact != *expected_artifact {
            return Err(problem());
        }
    }
    Ok(())
}

fn require_finished(
    comparisons: usize,
    owner: Option<&(PathBuf, u64, u64)>,
) -> Result<(), CorpusProblem> {
    if comparisons != 2 {
        return Err(problem());
    }
    let (root, _, _) = owner.ok_or_else(problem)?;
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(problem()),
    }
}

impl BatchExpectation {
    pub(super) fn new(
        inputs: &Inputs,
        definitions: &[BatchProgramDefinition],
    ) -> Result<Self, CorpusProblem> {
        inputs.current()?;
        Ok(Self {
            inputs: inputs.clone(),
            expected: expected_programs(definitions)?,
            comparisons: 0,
            artifact_owner: None,
        })
    }

    pub(super) fn compare_installation(
        &mut self,
        store: &dyn PlatformStore,
        artifact_root: &Path,
    ) -> Result<(), CorpusProblem> {
        let owner = artifact_owner(artifact_root)?;
        if self.comparisons >= 2
            || self
                .artifact_owner
                .as_ref()
                .is_some_and(|previous| previous != &owner)
        {
            return Err(problem());
        }
        compare_programs(&self.expected, store, artifact_root)?;
        self.inputs.current()?;
        self.artifact_owner = Some(owner);
        self.comparisons += 1;
        Ok(())
    }

    // Only after both actual installation comparisons, the selected public job
    // routes, checked restart shutdown and exact owned artifact cleanup succeed.
    pub(super) fn finish(self) -> Result<BatchProof, CorpusProblem> {
        require_finished(self.comparisons, self.artifact_owner.as_ref())?;
        self.inputs.current()?;
        Ok(BatchProof {
            inputs: self.inputs,
        })
    }
}

pub(super) fn observe_install_compile(
    inputs: &Inputs,
    online: OnlineProof,
    batch: BatchProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    // Run the same full44/62 source closure used by operator-compile. Batch work
    // is the already executed shared twelve-program owner, never replayed here.
    let closure = verify_carddemo_source_closures_from_env(&inputs.inventory)?;
    inputs.current()?;
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J19",
        "owned install/compile commands",
        online.inputs != *inputs
            || batch.inputs != *inputs
            || closure.corpus_commit != inputs.commit,
        || Ok(problem()),
    )
}

const WORKFLOW_SOURCES: [&str; 9] = [
    "scripts/local_compile.sh",
    "scripts/remote_compile.sh",
    "scripts/remote_refresh.sh",
    "scripts/remote_submit.sh",
    "scripts/run_full_batch.sh",
    "scripts/run_interest_calc.sh",
    "scripts/run_posting.sh",
    "scripts/upld_module.sh",
    "app/jcl/FTPJCL.JCL",
];
const MAPPINGS: [&[u8]; 2] = [
    b"FTP SITE FILETYPE=JES PUT -> PUT /zosmf/restjobs/jobs",
    b"FTP PUT AWS.M2.CARDEMO.FTP.TEST -> GET /zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST",
];

fn compare_download(
    method: &Method,
    path: &str,
    status: StatusCode,
    body: &[u8],
) -> Result<(), CorpusProblem> {
    if method != Method::GET
        || path != "/zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST"
        || status != StatusCode::OK
        || body != b"WELCOME-CD027"
    {
        return Err(problem());
    }
    Ok(())
}

pub(super) struct WorkflowMapping {
    inputs: Inputs,
    mapping_sha256: String,
}

pub(super) struct DownloadProof {
    mapping: WorkflowMapping,
}

impl WorkflowMapping {
    pub(super) fn verify(
        inputs: &Inputs,
        original_mapping_sha256: &str,
    ) -> Result<Self, CorpusProblem> {
        inputs.current()?;
        let mut digest = Sha256::new();
        for relative in WORKFLOW_SOURCES {
            let bytes = read_corpus_file(&inputs.corpus, &inputs.corpus.join(relative))?;
            digest_field(&mut digest, relative.as_bytes());
            digest_field(&mut digest, &bytes);
        }
        for mapping in MAPPINGS {
            digest_field(&mut digest, mapping);
        }
        let actual = format!("{:x}", digest.finalize());
        if actual != original_mapping_sha256 {
            return Err(problem());
        }
        Ok(Self {
            inputs: inputs.clone(),
            mapping_sha256: actual,
        })
    }

    // This consumes the source mapping only beside the actual authenticated GET
    // result. Preserve the existing selected route and exact download oracle.
    pub(super) fn compare_download(
        self,
        method: &Method,
        path: &str,
        status: StatusCode,
        body: &[u8],
    ) -> Result<DownloadProof, CorpusProblem> {
        compare_download(method, path, status, body)?;
        self.inputs.current()?;
        Ok(DownloadProof { mapping: self })
    }
}

#[cfg(test)]
#[path = "operator_observation_tests.rs"]
mod tests;

impl DownloadProof {
    // The caller owns the non-public JES principal/source/full-spool proof and
    // invokes this only after its exact positive comparison, memory drain and
    // checked artifact cleanup. No public receipt can manufacture this witness.
    pub(super) fn observe_owned_workflow(
        self,
        owned_jes: jes_security_observations::OperatorJesProof,
        observations: &mut RouteObservations,
    ) -> Result<(), CorpusProblem> {
        self.mapping.inputs.current()?;
        let refused = owned_jes.refused() || self.mapping.mapping_sha256.len() != 64;
        for requirement in [
            "z/OSMF submission",
            "selected FTP/JES flow",
            "repository workflow mapping",
        ] {
            observations.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J19",
                requirement,
                refused,
                || Ok(problem()),
            )?;
        }
        Ok(())
    }
}
