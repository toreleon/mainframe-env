//! Selected first-party source -> publisher -> actual installed artifact binding.
//! This private envelope complements the original v1 cohort receipt. It is not
//! generic package publication, IBM compiler parity, or full CD-009/CD-016 closure.
use super::*;
use mainframe_env_compiler_api::{ARTIFACT_CONTRACT, VersionedArtifactManifest};
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ExecutableArtifactMetadata, PlatformStore, ProviderStateRecord,
};

#[path = "source_artifact_pins.rs"]
mod pins;

const ENVELOPE: &str = "mainframe-env.carddemo-selected-source-artifact-binding@1";
const COMPATIBILITY: &str = "mainframe-env.cobol.reference@1";

fn problem(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.source_artifact_binding", detail)
}
fn refuse(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value { Err(problem(detail)) } else { Ok(()) }
}

#[derive(Clone)]
struct ExpectedProgram {
    source: serde_json::Value,
    record: ArtifactRecord,
}

pub(super) struct SourceInstallExpectation {
    package: CardDemoPackageReceipt,
    programs: BTreeMap<String, ExpectedProgram>,
    transactions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstalledSnapshot {
    programs: BTreeMap<String, ProviderStateRecord>,
    transactions: BTreeMap<String, ProviderStateRecord>,
    artifacts: BTreeMap<String, ArtifactRecord>,
    envelope_identity: String,
}

pub(super) struct InitialInstallProof {
    expected: SourceInstallExpectation,
    snapshot: InstalledSnapshot,
}
pub(super) struct InstallationProof {
    snapshot: InstalledSnapshot,
}

pub(super) fn verify_pinned_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, expected) in pins::SOURCE_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        require_pinned_bytes(path, &bytes, expected)?;
    }
    Ok(())
}
fn require_pinned_bytes(path: &str, bytes: &[u8], expected: &str) -> Result<(), CorpusProblem> {
    refuse(format!("{:x}", Sha256::digest(bytes)) != expected, path)
}

fn validate_package(package: &CardDemoPackageReceipt) -> Result<(), CorpusProblem> {
    let entries = pins::COHORT_ENTRIES
        .iter()
        .map(|(path, digest)| ((*path).to_string(), (*digest).to_string()))
        .collect::<BTreeMap<_, _>>();
    refuse(
        package.schema_version != "mainframe-env.carddemo-package-receipt@1"
            || package.status != "pass"
            || package.corpus_commit != pins::CORPUS_COMMIT
            || package.package_name != "AWS-CARDDEMO"
            || package.package_version != "0.1.1"
            || package.package_identity != pins::COHORT_IDENTITY
            || package.entries != entries
            || !package.staged_not_ready
            || !package.committed_ready
            || !package.idempotent_reinstall
            || package.negative_controls != 5,
        "original cohort receipt differs",
    )
}

fn pinned_transactions() -> BTreeMap<String, String> {
    pins::TRANSACTIONS
        .iter()
        .map(|(transaction, program)| ((*transaction).into(), (*program).into()))
        .collect()
}
fn pinned_programs() -> BTreeSet<String> {
    let mut names = pinned_transactions().into_values().collect::<BTreeSet<_>>();
    names.insert("CSUTLDTC".into());
    names
}

fn source_binding(
    name: &str,
    primary: &str,
    bundle: &SourceBundle,
) -> Result<serde_json::Value, CorpusProblem> {
    refuse(
        !pinned_programs().contains(name)
            || primary != format!("app/cbl/{name}.cbl")
            || bundle
                .file(bundle.primary())
                .map(|file| file.path().as_str())
                != Some(primary)
            || !bundle.options().is_empty()
            || !bundle.provenance().is_empty(),
        "primary source or compiler-source options/provenance differ",
    )?;
    let limits = SourceLimits::default();
    // Repository-authored ABI sources have separate, explicit local authority.
    // They are bound exactly but are not first-party or IBM publication text.
    let abi = subsystem_abi_libraries(limits)?;
    let mut expected_files = BTreeMap::new();
    for (path, digest) in pins::SOURCE_PINS {
        if *path == primary
            || pins::COPY_ROOTS
                .iter()
                .any(|root| path.starts_with(&format!("{root}/")))
        {
            expected_files.insert(
                (*path).to_string(),
                (
                    (*digest).to_string(),
                    SourceFormat::Fixed,
                    SourceEncoding::Utf8,
                ),
            );
        }
    }
    refuse(
        expected_files.len() != 63,
        "primary plus 62 copybooks differs",
    )?;
    for file in &abi.files {
        refuse(
            expected_files
                .insert(
                    file.path().as_str().into(),
                    (
                        format!("{:x}", Sha256::digest(file.bytes())),
                        file.format(),
                        file.encoding(),
                    ),
                )
                .is_some(),
            "ABI path conflicts",
        )?;
    }
    let mut actual_files = BTreeMap::new();
    let mut files = Vec::new();
    for file in bundle.files() {
        let path = file.path().as_str();
        let digest = format!("{:x}", Sha256::digest(file.bytes()));
        refuse(
            actual_files
                .insert(
                    path.to_string(),
                    (digest.clone(), file.format(), file.encoding()),
                )
                .is_some(),
            "duplicate source",
        )?;
        let format = match file.format() {
            SourceFormat::Fixed => "fixed",
            SourceFormat::Free => "free",
            SourceFormat::Variable => "variable",
        };
        files.push(serde_json::json!({"path":path, "bytes":file.bytes().len(),
            "sha256":digest, "format":format, "encoding":"utf8"}));
    }
    compare_source_files(&expected_files, &actual_files)?;
    let mut expected_libraries = Vec::new();
    for (index, root) in pins::COPY_ROOTS.iter().enumerate() {
        let members = pins::SOURCE_PINS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{root}/")))
            .map(|(path, _)| LogicalPath::new(*path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| problem(error.to_string()))?;
        expected_libraries.push(
            SourceLibrary::new(format!("application-{index:02}"), members, limits)
                .map_err(|error| problem(error.to_string()))?,
        );
    }
    expected_libraries.extend(abi.libraries);
    compare_source_libraries(&expected_libraries, bundle.libraries())?;
    let libraries = bundle
        .libraries()
        .iter()
        .map(|library| {
            serde_json::json!({
                "name":library.name(), "members":library.members().iter().map(LogicalPath::as_str)
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    Ok(
        serde_json::json!({"primary":primary, "source_identity":bundle.id().to_hex(),
        "files":files, "libraries":libraries, "options":{}, "provenance":[],
        "compile_mode":"executable", "compile_target":"reference", "compile_options":{},
        "authority":"pinned first-party sources plus repository-authored host ABI"}),
    )
}

type SourceFiles = BTreeMap<String, (String, SourceFormat, SourceEncoding)>;
fn compare_source_files(expected: &SourceFiles, actual: &SourceFiles) -> Result<(), CorpusProblem> {
    refuse(
        expected != actual,
        "source closure bytes/path/format/encoding differs",
    )
}
fn compare_source_libraries(
    expected: &[SourceLibrary],
    actual: &[SourceLibrary],
) -> Result<(), CorpusProblem> {
    refuse(expected != actual, "ordered source-library closure differs")
}

/// Retain the complete offered manifest without reinterpreting its version.
/// Exhaustive patterns keep new variants and fields as capture obligations.
#[cfg(test)]
pub(super) fn retain_manifest(manifest: &VersionedArtifactManifest) -> serde_json::Value {
    match manifest {
        VersionedArtifactManifest::V2(mainframe_env_compiler_api::ArtifactManifestV2 {
            compiler_generation,
            target,
            options,
            host_interfaces,
            ir_contract,
        }) => serde_json::json!({"V2": {
            "compiler_generation": compiler_generation,
            "target": target.as_str(),
            "options": options.values(),
            "host_interfaces": host_interfaces,
            "ir_contract": ir_contract,
        }}),
        VersionedArtifactManifest::V3(mainframe_env_compiler_api::ArtifactManifest {
            compiler_generation,
            target,
            options,
            host_interfaces,
            ir_contract,
            dialect_contracts,
        }) => serde_json::json!({"V3": {
            "compiler_generation": compiler_generation,
            "target": target.as_str(),
            "options": options.values(),
            "host_interfaces": host_interfaces,
            "ir_contract": ir_contract,
            "dialect_contracts": dialect_contracts,
        }}),
    }
}

pub(super) fn publisher_record(
    definition: &OnlineProgramDefinition,
) -> Result<ArtifactRecord, CorpusProblem> {
    let VersionedArtifactManifest::V3(manifest) = &definition.manifest else {
        return Err(problem("selected publisher manifest is not current v3"));
    };
    let payload_digest: [u8; 32] = Sha256::digest(&definition.payload).into();
    refuse(
        definition.artifact.as_str() != format!("sha256:{:x}", Sha256::digest(&definition.payload))
            || definition.payload.is_empty()
            || definition.payload.len() > 64 * 1024 * 1024
            || manifest.target.as_str() != "reference",
        "publisher content reference differs",
    )?;
    // These fields are the public stored executable-metadata contract. Compare
    // complete metadata, including its independent payload-binding digest.
    let metadata = ExecutableArtifactMetadata {
        artifact_contract: ARTIFACT_CONTRACT.into(),
        compatibility_profile: COMPATIBILITY.into(),
        compiler_generation: manifest.compiler_generation.clone(),
        target: manifest.target.as_str().into(),
        options: manifest.options.values().clone(),
        host_interfaces: manifest.host_interfaces.clone(),
        ir_contract: manifest.ir_contract.clone(),
        dialect_contracts: Some(manifest.dialect_contracts.clone()),
        semantic_identity: definition.semantic_identity.clone(),
        manifest_payload_digest: [0; 32],
    }
    .bind_to_payload(&payload_digest);
    refuse(
        !metadata.validate() || !metadata.validates_payload(&payload_digest),
        "publisher metadata binding differs",
    )?;
    Ok(ArtifactRecord {
        artifact: definition.artifact.clone(),
        media_type: "application/vnd.mainframe-env.core-mir".into(),
        payload_digest,
        payload: definition.payload.clone(),
        executable: Some(metadata),
    })
}

impl SourceInstallExpectation {
    pub(super) fn new(
        corpus: &Path,
        package: CardDemoPackageReceipt,
    ) -> Result<Self, CorpusProblem> {
        validate_package(&package)?;
        verify_pinned_sources(corpus)?;
        Ok(Self {
            package,
            programs: BTreeMap::new(),
            transactions: BTreeMap::new(),
        })
    }
    pub(super) fn add_program(
        &mut self,
        name: &str,
        primary: &str,
        bundle: &SourceBundle,
        definition: &OnlineProgramDefinition,
    ) -> Result<(), CorpusProblem> {
        refuse(
            definition.name != name,
            "published program name differs from selected source",
        )?;
        let source = source_binding(name, primary, bundle)?;
        let record = publisher_record(definition)?;
        refuse(
            self.programs
                .insert(name.into(), ExpectedProgram { source, record })
                .is_some(),
            "duplicate selected publisher",
        )
    }
    pub(super) fn finish(
        &mut self,
        definition: &OnlineApplicationDefinition,
    ) -> Result<(), CorpusProblem> {
        refuse(
            self.programs.keys().cloned().collect::<BTreeSet<_>>() != pinned_programs()
                || definition.transactions != pinned_transactions()
                || definition.programs.len() != 18
                || definition
                    .programs
                    .iter()
                    .map(|program| program.name.clone())
                    .collect::<BTreeSet<_>>()
                    != pinned_programs(),
            "selected program/transaction closure differs",
        )?;
        for definition in &definition.programs {
            refuse(
                self.programs
                    .get(&definition.name)
                    .map(|expected| &expected.record)
                    != Some(&publisher_record(definition)?),
                "offered publisher mapping differs",
            )?;
        }
        self.transactions = definition.transactions.clone();
        Ok(())
    }
    pub(super) fn observe_initial(
        self,
        corpus: &Path,
        store: &dyn PlatformStore,
        artifact_root: &Path,
    ) -> Result<InitialInstallProof, CorpusProblem> {
        let snapshot = self.read_snapshot(corpus, store, artifact_root)?;
        Ok(InitialInstallProof {
            expected: self,
            snapshot,
        })
    }
    fn expected_snapshot(&self) -> InstalledSnapshot {
        let state = |namespace: &str, key: &str, payload: Vec<u8>| ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: 1,
            payload,
        };
        InstalledSnapshot {
            programs: self
                .programs
                .iter()
                .map(|(name, program)| {
                    (
                        name.clone(),
                        state(
                            "online-program",
                            name,
                            program.record.artifact.as_str().as_bytes().to_vec(),
                        ),
                    )
                })
                .collect(),
            transactions: self
                .transactions
                .iter()
                .map(|(transaction, program)| {
                    (
                        transaction.clone(),
                        state(
                            "online-transaction",
                            transaction,
                            program.as_bytes().to_vec(),
                        ),
                    )
                })
                .collect(),
            artifacts: self
                .programs
                .iter()
                .map(|(name, program)| (name.clone(), program.record.clone()))
                .collect(),
            envelope_identity: String::new(),
        }
    }
    fn envelope_identity(&self, snapshot: &InstalledSnapshot) -> Result<String, CorpusProblem> {
        let programs = self.programs.iter().map(|(name, expected)| {
            let actual = snapshot.artifacts.get(name).ok_or_else(|| problem("missing artifact"))?;
            let metadata = actual.executable.as_ref().ok_or_else(|| problem("missing metadata"))?;
            Ok((name.clone(), serde_json::json!({"source":expected.source,
                "installed_reference":actual.artifact.as_str(), "payload_bytes":actual.payload.len(),
                "payload_sha256":format!("{:x}", Sha256::digest(&actual.payload)),
                "media_type":actual.media_type, "artifact_contract":metadata.artifact_contract,
                "compatibility_profile":metadata.compatibility_profile,
                "compiler_generation":metadata.compiler_generation, "target":metadata.target,
                "options":metadata.options, "host_interfaces":metadata.host_interfaces,
                "ir_contract":metadata.ir_contract, "dialect_contracts":metadata.dialect_contracts,
                "semantic_identity":metadata.semantic_identity,
                "manifest_payload_digest":metadata.manifest_payload_digest})))
        }).collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
        let envelope = serde_json::json!({"schema_version":ENVELOPE,
            "corpus_commit":pins::CORPUS_COMMIT, "cohort_package_identity":self.package.package_identity,
            "cohort_entries":self.package.entries, "programs":programs,
            "transactions":self.transactions,
            "scope":"selected installed fixture; no generic atomic package publication or IBM parity"});
        let bytes = serde_json::to_vec(&envelope).map_err(|error| problem(error.to_string()))?;
        refuse(
            bytes.len() > 1024 * 1024,
            "selected binding envelope exceeds 1 MiB",
        )?;
        Ok(format!("sha256:{:x}", Sha256::digest(&bytes)))
    }
    fn read_snapshot(
        &self,
        corpus: &Path,
        store: &dyn PlatformStore,
        artifact_root: &Path,
    ) -> Result<InstalledSnapshot, CorpusProblem> {
        verify_pinned_sources(corpus)?;
        validate_package(&self.package)?;
        refuse(
            self.programs.len() != 18 || self.transactions != pinned_transactions(),
            "incomplete selected source/artifact expectation",
        )?;
        let mut snapshot = InstalledSnapshot {
            programs: read_states(store, "online-program", 19)?,
            transactions: read_states(store, "online-transaction", 18)?,
            artifacts: BTreeMap::new(),
            envelope_identity: String::new(),
        };
        let expected = self.expected_snapshot();
        refuse(
            snapshot.programs != expected.programs
                || snapshot.transactions != expected.transactions,
            "actual durable name/reference or transaction mapping differs",
        )?;
        // The namespace was exclusively claimed by the owning wrapper. ProductServer
        // uses this same LocalArtifactStore root and 64 MiB object limit. This open
        // creates/syncs objects and is therefore not a general read-only API.
        let artifacts = LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024)
            .map_err(|error| problem(error.to_string()))?;
        for (name, row) in &snapshot.programs {
            let reference = ArtifactRef::new(
                String::from_utf8(row.payload.clone())
                    .map_err(|error| problem(error.to_string()))?,
                InvocationLimits::default(),
            )
            .map_err(|error| problem(error.to_string()))?;
            let record = artifacts
                .get_artifact(&reference)
                .map_err(|error| problem(error.to_string()))?
                .ok_or_else(|| problem("actual installed artifact is missing"))?;
            refuse(
                snapshot.artifacts.insert(name.clone(), record).is_some(),
                "duplicate artifact name",
            )?;
        }
        compare_snapshot(&expected, &snapshot)?;
        snapshot.envelope_identity = self.envelope_identity(&snapshot)?;
        Ok(snapshot)
    }
}
fn read_states(
    store: &dyn PlatformStore,
    namespace: &str,
    max: usize,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(namespace, max, 32 * 1024)
        .map_err(|error| problem(error.to_string()))?;
    let mut states = BTreeMap::new();
    for row in rows {
        refuse(
            row.namespace != namespace
                || row.version != 1
                || states.insert(row.key.clone(), row).is_some(),
            "invalid/duplicate selected state",
        )?;
    }
    Ok(states)
}
fn compare_snapshot(
    expected: &InstalledSnapshot,
    actual: &InstalledSnapshot,
) -> Result<(), CorpusProblem> {
    refuse(
        expected.programs != actual.programs
            || expected.transactions != actual.transactions
            || expected.artifacts != actual.artifacts,
        "complete actual installed state/artifact bytes or executable metadata differs",
    )
}
fn compare_reopened(
    initial: &InstalledSnapshot,
    reopened: &InstalledSnapshot,
) -> Result<(), CorpusProblem> {
    refuse(
        initial != reopened,
        "selected installed source/artifact binding changed on reopen",
    )
}

impl InitialInstallProof {
    pub(super) fn observe_reopened(
        self,
        corpus: &Path,
        store: &dyn PlatformStore,
        artifact_root: &Path,
    ) -> Result<InstallationProof, CorpusProblem> {
        let reopened = self.expected.read_snapshot(corpus, store, artifact_root)?;
        compare_reopened(&self.snapshot, &reopened)?;
        Ok(InstallationProof { snapshot: reopened })
    }
}
impl InstallationProof {
    /// Combine the independently captured FILE/map catalog with the exact actual
    /// durable program/transaction mappings already verified against their source
    /// publishers and repeated after reopen. This is a private fixture witness.
    pub(super) fn require_resource_mappings(&self) -> Result<(), CorpusProblem> {
        refuse(
            self.snapshot
                .programs
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != pinned_programs()
                || self
                    .snapshot
                    .artifacts
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    != pinned_programs()
                || self.snapshot.transactions.len() != 17
                || !self.snapshot.envelope_identity.starts_with("sha256:")
                || self.snapshot.envelope_identity.len() != 71,
            "incomplete resource installation proof",
        )?;
        for (transaction, program) in pinned_transactions() {
            let expected = ProviderStateRecord {
                namespace: "online-transaction".into(),
                key: transaction.clone(),
                version: 1,
                payload: program.into_bytes(),
            };
            refuse(
                self.snapshot.transactions.get(&transaction) != Some(&expected),
                "source-derived transaction mapping differs",
            )?;
        }
        for (name, record) in &self.snapshot.artifacts {
            let expected = ProviderStateRecord {
                namespace: "online-program".into(),
                key: name.clone(),
                version: 1,
                payload: record.artifact.as_str().as_bytes().to_vec(),
            };
            let payload_digest: [u8; 32] = Sha256::digest(&record.payload).into();
            refuse(
                self.snapshot.programs.get(name) != Some(&expected)
                    || record.payload_digest != payload_digest
                    || record.artifact.as_str()
                        != format!("sha256:{:x}", Sha256::digest(&record.payload))
                    || record.executable.as_ref().is_none_or(|metadata| {
                        !metadata.validate() || !metadata.validates_payload(&payload_digest)
                    }),
                "immutable actual installed program mapping differs",
            )?;
        }
        Ok(())
    }
    // Only called after the existing route/coverage guards, both checked server
    // shutdowns, and checked removal of the exclusively owned artifact namespace.
    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        self.require_resource_mappings()?;
        for requirement in ["package identity", "program artifacts"] {
            observations.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J01",
                requirement,
                false,
                || Ok(problem("installed binding refused")),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "source_artifact_binding_tests.rs"]
mod tests;
