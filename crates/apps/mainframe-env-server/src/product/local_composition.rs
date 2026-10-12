//! Finite local composition of genuine seed/CICS owners and selected online catalog rows.
//! Other subsystem families require their own held preparation before joining this profile.
use super::*;
#[path = "local_composition/source.rs"]
mod source;
pub use source::LocalSourceCompilation;
#[path = "local_composition/db2.rs"]
mod db2;
#[path = "local_composition/security.rs"]
mod security;
use mainframe_env_db2::Db2CatalogProjection;
#[path = "local_composition/ims.rs"]
mod ims;
use mainframe_env_ims::ImsCatalogProjection;
#[path = "local_composition/mq.rs"]
mod mq;
use mainframe_env_mq::MqCatalogProjection;
#[path = "local_composition/batch.rs"]
mod batch;
use mainframe_env_batch::BatchControllerProjection;
use mainframe_env_cics::CicsFileDefinition;
use mainframe_env_dataset::{DatasetSeedObject, SeedInstallReceipt};
use mainframe_env_racf::{RacfProfileComposition, RacfProfileProjection};
use mainframe_env_store_api::{MAX_PROVIDER_COMPOSITION_BYTES, ProviderCompositionCommit};

const LOCAL_GENERATION: &str = "application-local-generation";
const LOCAL_SELECTION: &str = "application-local-selection";
const LOCAL_CONTRACT: &str = "mainframe-env.application-local-composition@1";

/// Exact definitions and source-owned seed inputs offered to the finite local publisher.
/// Signed local Source commitments authenticate typed compiler inputs; no vendor execution is inferred.
pub struct LocalApplicationDefinition {
    /// Admitted immutable programs, complete transaction routes and full BMS maps.
    pub online: OnlineApplicationDefinition,
    /// Exact declared source requests whose raw files and full publisher result are verified.
    /// Each request must match its exact signed first-party Resource commitment.
    pub compilations: Vec<LocalSourceCompilation>,
    /// Explicit signed first-party Profile declarations; unrelated authority runtime state is retained.
    pub security_profiles: Vec<RacfProfileComposition>,
    /// Full raw seed objects bound to their actual Data manifest entry paths.
    pub seeds: Vec<DatasetSeedObject>,
    /// Actual CICS FILE definitions targeting this package's selected seed datasets.
    pub files: BTreeMap<String, CicsFileDefinition>,
    /// Resource manifest entry path for each normalized mapset/map pair.
    pub map_entries: BTreeMap<(String, String), String>,
    /// Resource manifest entry path for each normalized FILE name.
    pub file_entries: BTreeMap<String, String>,
}

/// Actual single-store selection result, not an IssueObservation or global complete flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalApplicationReceipt {
    /// Package generation selected by the actual installer.
    pub generation: ApplicationGenerationRecord,
    /// Full seed identity and original raw object/count receipt.
    pub seed: SeedInstallReceipt,
    /// Content binding of all participating static rows and seed identity.
    pub binding: String,
    /// Actual full SourceId of each genuine compilation authenticated by the signed commitment.
    /// Identity admission does not serialize reconstruction material or imply vendor equivalence.
    pub compiled_sources: BTreeMap<String, String>,
    /// Exact successful retry leaves runtime-edited seed data untouched.
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalSelection {
    schema_version: String,
    package: String,
    generation: u64,
    identity: String,
    seed_identity: String,
    seed_snapshot: String,
    compiled_sources: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    racf: Vec<RacfProfileProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    db2: Option<Db2CatalogProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ims: Option<ImsCatalogProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mq: Option<MqCatalogProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    batch: Option<BatchControllerProjection>,
    binding: String,
    #[serde(
        serialize_with = "source::serialize_provider_rows",
        deserialize_with = "source::deserialize_provider_rows"
    )]
    rows: Vec<ProviderStateRecord>,
}

impl ProductServer {
    /// Select a staged finite local package using one genuine atomic provider-state commit.
    /// All preparation owners stay held through its acknowledgement and cache publication.
    pub fn publish_local_application(
        &self,
        expected: &ApplicationGenerationRecord,
        definition: LocalApplicationDefinition,
    ) -> Result<LocalApplicationReceipt, HostProblem> {
        self.compose_local_application(expected, definition, false)
    }

    /// Select a retained Ready local generation through the same owned atomic composition.
    /// The caller supplies the original signed definitions and full seed inputs for comparison.
    pub fn rollback_local_application(
        &self,
        expected: &ApplicationGenerationRecord,
        definition: LocalApplicationDefinition,
    ) -> Result<LocalApplicationReceipt, HostProblem> {
        self.compose_local_application(expected, definition, true)
    }

    fn compose_local_application(
        &self,
        expected: &ApplicationGenerationRecord,
        definition: LocalApplicationDefinition,
        rollback: bool,
    ) -> Result<LocalApplicationReceipt, HostProblem> {
        let _publication = self.publication_write()?;
        self.require_local_owner_known()?;
        let mut durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let retained = durable
            .installer
            .generation(&expected.package, expected.generation, &expected.identity)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if retained.record().package != expected.package
            || retained.record().version != expected.version
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let before = durable
            .installer
            .state_payload()
            .map_err(application_install_problem)?;
        // This finite profile bounds every installer copy before provider preparation.
        // It does not increase the unchanged legacy installer or provider budgets.
        if before.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        let package = retained.package();
        reject_unprepared_sections(package)?;
        let program_definitions = &definition.online.programs;
        if program_definitions.is_empty()
            || program_definitions.len() > 4096
            || definition.compilations.len() != program_definitions.len()
            || definition.online.transactions.is_empty()
            || definition.online.transactions.len() > 4096
            || definition.online.maps.is_empty()
            || definition.seeds.is_empty()
        {
            return Err(HostProblem::Malformed);
        }
        let source_bytes = source::source_input_bytes(&definition.compilations)?;
        let mut used = BTreeSet::new();
        let profile_bytes =
            security::bind_profiles(package, &definition.security_profiles, &mut used)?;
        let (catalog, catalog_bytes) = db2::bind_catalog(self, &retained, &mut used)?;
        let (ims_catalog, ims_bytes) = ims::bind_catalog(package, &mut used)?;
        let (mq_catalog, mq_catalog_bytes) = mq::bind_catalog(package, &mut used)?;
        let (batch_generation, batch_bytes) = batch::bind_controllers(self, &retained, &mut used)?;
        let offered_bytes = definition
            .seeds
            .iter()
            .try_fold(0usize, |total, item| total.checked_add(item.bytes.len()))
            .and_then(|total| {
                program_definitions
                    .iter()
                    .try_fold(total, |total, item| total.checked_add(item.payload.len()))
            })
            .and_then(|total| total.checked_add(source_bytes))
            .and_then(|total| total.checked_add(profile_bytes))
            .and_then(|total| total.checked_add(catalog_bytes))
            .and_then(|total| total.checked_add(ims_bytes))
            .and_then(|total| total.checked_add(mq_catalog_bytes))
            .and_then(|total| total.checked_add(batch_bytes))
            .ok_or(HostProblem::ResourceExhausted)?;
        if offered_bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut programs = BTreeMap::new();
        let mut artifacts = Vec::new();
        let compiled_sources = source::compile_sources(
            package,
            program_definitions,
            definition.compilations,
            &mut used,
        )?;
        for item in program_definitions {
            let name = normalize_online_name(&item.name, 128)?;
            bind_entry(
                package,
                EntryKind::Program,
                &format!("program/{name}"),
                &item.payload,
                &mut used,
            )?;
            let record = artifact::admitted_record(
                &item.artifact,
                &item.payload,
                &item.manifest,
                &item.semantic_identity,
            )?;
            if programs.insert(name, item.artifact.clone()).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            artifacts.push(record);
        }
        if let Some(generation) = &batch_generation
            && generation.controllers.iter().any(|item| {
                programs
                    .get(item.selector.program())
                    .map(|artifact| artifact.as_str())
                    != Some(item.program.identity.as_str())
            })
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut transactions = BTreeMap::new();
        for (transaction, program) in &definition.online.transactions {
            let transaction = normalize_online_name(transaction, 16)?;
            let program = normalize_online_name(program, 128)?;
            bind_entry(
                package,
                EntryKind::Resource,
                &format!("transaction/{transaction}"),
                program.as_bytes(),
                &mut used,
            )?;
            if !programs.contains_key(&program)
                || transactions.insert(transaction, program).is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let mut datasets = BTreeSet::new();
        for seed in &definition.seeds {
            bind_entry(
                package,
                EntryKind::Data,
                &seed.source_id,
                &seed.bytes,
                &mut used,
            )?;
            if !datasets.insert(seed.dataset.as_str().to_string()) {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        if definition.map_entries.len() != definition.online.maps.len()
            || definition.file_entries.len() != definition.files.len()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        for map in &definition.online.maps {
            let key = (
                map.mapset.to_ascii_uppercase(),
                map.map.to_ascii_uppercase(),
            );
            let path = definition
                .map_entries
                .get(&key)
                .ok_or(HostProblem::NotFound)?;
            bind_entry(
                package,
                EntryKind::Resource,
                path,
                &self.cics.composition_map_payload(map)?,
                &mut used,
            )?;
        }
        for (name, file) in &definition.files {
            let normalized = normalize_online_name(name, 16)?;
            if !datasets.contains(file.dataset.as_str()) {
                return Err(HostProblem::NotFound);
            }
            let path = definition
                .file_entries
                .get(&normalized)
                .ok_or(HostProblem::NotFound)?;
            bind_entry(
                package,
                EntryKind::Resource,
                path,
                &self.cics.composition_file_payload(file)?,
                &mut used,
            )?;
        }
        let name = expected.package.to_ascii_uppercase();
        let previous = self.local_selection(&name)?;
        if self
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, &name)
            .map_err(store_error)?
            .is_some()
            || (previous.is_none() && retained.record().state != InstallState::Staged)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut current_programs = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut current_transactions = self
            .online_transactions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        // Match existing runtime lock direction: CICS handlers can call the dataset provider.
        let cics = self.cics.prepare_registration(
            &programs.keys().cloned().collect(),
            &definition.online.maps,
            &definition.files,
            previous
                .as_ref()
                .map_or(&[], |(_, selected)| selected.rows.as_slice()),
        )?;
        let seed = self.dataset.prepare_seed_selection(
            &name,
            &expected.generation.to_string(),
            definition.seeds,
            true,
        )?;
        let seed_receipt = seed.receipt();
        let seed_snapshot = seed.snapshot_digest()?;
        // Match the real Db2 execute owner: its durable lock precedes RACF authorization.
        let previous_catalog = previous
            .as_ref()
            .and_then(|(_, selected)| selected.db2.as_ref());
        let db2 = if let Some(catalog) = catalog {
            Some(
                self.db2
                    .prepare_catalog_composition(catalog, previous_catalog, rollback)?,
            )
        } else {
            if previous_catalog.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        };
        let catalog_projection = db2.as_ref().map(|prepared| prepared.projection().clone());
        // The real IMS runtime holds its provider owner before calling RACF.
        let previous_ims = previous
            .as_ref()
            .and_then(|(_, selected)| selected.ims.as_ref());
        let ims = if let Some(catalog) = ims_catalog {
            Some(self.ims.prepare_catalog_composition(
                &name,
                expected.generation,
                &expected.identity,
                catalog,
                previous_ims,
                rollback,
            )?)
        } else {
            if previous_ims.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        };
        let ims_projection = ims.as_ref().map(|prepared| prepared.projection().clone());
        // Actual legacy MQ execute takes this durable owner before RACF authorization.
        let previous_mq = previous
            .as_ref()
            .and_then(|(_, selected)| selected.mq.as_ref());
        let mq = if let Some(catalog) = mq_catalog {
            Some(self.mq.prepare_catalog_composition(
                &name,
                expected.generation,
                &expected.identity,
                catalog,
                previous_mq,
                rollback,
            )?)
        } else {
            if previous_mq.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        };
        let mq_projection = mq.as_ref().map(|prepared| prepared.projection().clone());
        // Every provider owner precedes RACF; batch follows MQ.
        let previous_batch = previous
            .as_ref()
            .and_then(|(_, selected)| selected.batch.as_ref());
        let batch = if let Some(generation) = batch_generation {
            Some(
                self.batch
                    .prepare_controller_composition(generation, previous_batch, rollback)?,
            )
        } else {
            if previous_batch.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        };
        let batch_projection = batch.as_ref().map(|prepared| prepared.projection().clone());
        // The new writer follows the existing CICS -> dataset lock direction.
        // No authorize/mutate call is made while this owner is held.
        let previous_profiles = previous
            .as_ref()
            .map_or(&[][..], |(_, selected)| selected.racf.as_slice());
        let racf = if definition.security_profiles.is_empty() {
            if !previous_profiles.is_empty() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        } else {
            let foreign = security::foreign_profile_owners(self, &name, &durable.installer)?;
            Some(self.racf.database().prepare_profile_composition(
                &definition.security_profiles,
                previous_profiles,
                &foreign,
            )?)
        };
        let projections = racf
            .as_ref()
            .map_or_else(Vec::new, |prepared| prepared.projections().to_vec());
        // Signed resource bytes also authenticate every original seed attribute and geometry.
        bind_entry(
            package,
            EntryKind::Resource,
            "seed/snapshot",
            seed_snapshot.as_bytes(),
            &mut used,
        )?;
        // Every executable/data/resource entry must have an actual participating owner.
        if package.base.manifest.entries.iter().any(|entry| {
            matches!(
                entry.kind,
                EntryKind::Source | EntryKind::Program | EntryKind::Data | EntryKind::Resource
            ) && !used.contains(&entry.path)
        }) {
            return Err(HostProblem::Unsupported);
        }
        let mut rows = cics
            .writes()
            .iter()
            .map(|write| write.record.clone())
            .collect::<Vec<_>>();
        let mut route_writes = Vec::new();
        for (key, artifact) in &programs {
            route_writes.push(
                self.prepare_local_route(
                    "online-program",
                    key,
                    artifact.as_str().as_bytes(),
                    current_programs
                        .get(key)
                        .map(|value| value.as_str().as_bytes()),
                    previous.as_ref(),
                )?,
            );
        }
        for (key, program) in &transactions {
            route_writes.push(self.prepare_local_route(
                "online-transaction",
                key,
                program.as_bytes(),
                current_transactions.get(key).map(|value| value.as_bytes()),
                previous.as_ref(),
            )?);
        }
        if let Some(prepared) = &batch {
            for controller in &prepared.projection().generation.controllers {
                let key = controller.selector.program();
                let cached = self
                    .store
                    .get_provider_state("batch-program", key)
                    .map_err(store_error)?;
                // Only the actual prior local owner can replace an existing compiled mapping.
                route_writes.push(self.prepare_local_route(
                    "batch-program",
                    key,
                    controller.program.identity.as_bytes(),
                    cached.as_ref().map(|row| row.payload.as_slice()),
                    previous.as_ref(),
                )?);
            }
        }
        rows.extend(route_writes.iter().map(|write| write.record.clone()));
        rows.sort_by(|left, right| {
            (&left.namespace, &left.key).cmp(&(&right.namespace, &right.key))
        });
        let selected = LocalSelection {
            schema_version: LOCAL_CONTRACT.into(),
            package: name.clone(),
            generation: expected.generation,
            identity: expected.identity.clone(),
            seed_identity: seed_receipt.identity.clone(),
            binding: local_binding(
                &expected.identity,
                &seed_receipt.identity,
                &seed_snapshot,
                &compiled_sources,
                &projections,
                (
                    catalog_projection.as_ref(),
                    ims_projection.as_ref(),
                    mq_projection.as_ref(),
                    batch_projection.as_ref(),
                ),
                &rows,
            )?,
            seed_snapshot,
            compiled_sources,
            racf: projections,
            db2: catalog_projection,
            ims: ims_projection,
            mq: mq_projection,
            batch: batch_projection,
            rows,
        };
        if let Some((_, prior)) = &previous {
            let old_keys = prior
                .rows
                .iter()
                .map(|row| (&row.namespace, &row.key))
                .collect::<Vec<_>>();
            let next_keys = selected
                .rows
                .iter()
                .map(|row| (&row.namespace, &row.key))
                .collect::<Vec<_>>();
            if old_keys != next_keys
                || security::projection_keys(&prior.racf)?
                    != security::projection_keys(&selected.racf)?
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            if prior.generation == expected.generation {
                if prior.identity != selected.identity
                    || prior.binding != selected.binding
                    || !seed_receipt.replayed
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                self.verify_static_local_rows(prior)?;
                let actual = durable
                    .installer
                    .selected(&name)
                    .map_err(application_install_problem)?
                    .ok_or(HostProblem::NotFound)?;
                if actual != *retained.record() || actual.state != InstallState::Ready {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(LocalApplicationReceipt {
                    generation: actual,
                    seed: seed_receipt,
                    binding: prior.binding.clone(),
                    compiled_sources: prior.compiled_sources.clone(),
                    replayed: true,
                });
            }
        }
        let generation_key = format!("{name}:{}", expected.generation);
        let saved_generation = self
            .store
            .get_provider_state(LOCAL_GENERATION, &generation_key)
            .map_err(store_error)?;
        if let Some(row) = &saved_generation {
            let original = decode_local_selection(row)?;
            if original.binding != selected.binding || original.identity != selected.identity {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else if rollback {
            return Err(HostProblem::NotFound);
        }
        // Build selection in an isolated actual installer copy; live Ready is not published early.
        let next = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            durable.verifier.clone(),
            &before,
        )
        .map_err(application_install_problem)?;
        let actual = if rollback {
            next.rollback(&name, expected.generation)
        } else {
            next.commit(package)
        }
        .map_err(application_install_problem)?;
        let installer_payload = next.state_payload().map_err(application_install_problem)?;
        let installer_version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let selection_payload = encode_local_selection(&selected)?;
        let mut writes = cics.writes().to_vec();
        writes.extend_from_slice(seed.writes());
        if let Some(prepared) = &db2 {
            writes.extend_from_slice(prepared.writes());
        }
        if let Some(prepared) = &ims {
            writes.extend_from_slice(prepared.writes());
        }
        if let Some(prepared) = &mq {
            writes.extend_from_slice(prepared.writes());
        }
        if let Some(prepared) = &batch {
            writes.extend_from_slice(prepared.writes());
        }
        if let Some(prepared) = &racf {
            writes.extend_from_slice(prepared.writes());
        }
        writes.extend(route_writes);
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: APPLICATION_V2_STATE_NAMESPACE.into(),
                key: APPLICATION_V2_STATE_KEY.into(),
                version: installer_version,
                payload: installer_payload,
            },
            expected_version: (durable.store_version != 0).then_some(durable.store_version),
        });
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: LOCAL_SELECTION.into(),
                key: name,
                version: previous
                    .as_ref()
                    .map_or(Some(1), |(row, _)| row.version.checked_add(1))
                    .ok_or(HostProblem::ResourceExhausted)?,
                payload: selection_payload.clone(),
            },
            expected_version: previous.as_ref().map(|(row, _)| row.version),
        });
        if saved_generation.is_none() {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: LOCAL_GENERATION.into(),
                    key: generation_key,
                    version: 1,
                    payload: selection_payload,
                },
                expected_version: None,
            });
        }
        // Immutable artifacts may be retained before the CAS; they cannot make a route Ready.
        for record in artifacts {
            self.artifacts.put_artifact(record).map_err(store_error)?;
        }
        let provider_store: Arc<dyn ProviderStateStore> = self.store.clone();
        let commit = match ProviderCompositionCommit::publish(provider_store, writes) {
            Ok(commit) => commit,
            Err(problem) => {
                if matches!(
                    problem,
                    StoreError::Infrastructure(_) | StoreError::Poisoned
                ) {
                    cics.fence_unknown();
                    seed.fence_unknown();
                    if let Some(prepared) = &db2 {
                        prepared.fence_unknown();
                    }
                    if let Some(prepared) = &ims {
                        prepared.fence_unknown();
                    }
                    if let Some(prepared) = &mq {
                        prepared.fence_unknown();
                    }
                    if let Some(prepared) = &batch {
                        prepared.fence_unknown();
                    }
                    if let Some(prepared) = &racf {
                        prepared.fence_unknown();
                    }
                    self.local_composition_uncertain
                        .store(true, Ordering::SeqCst);
                    return Err(HostProblem::UnknownOutcome);
                }
                return Err(store_error(problem));
            }
        };
        if !cics.accepts(&commit)
            || !seed.accepts(&commit)
            || db2
                .as_ref()
                .is_some_and(|prepared| !prepared.accepts(&commit))
            || ims
                .as_ref()
                .is_some_and(|prepared| !prepared.accepts(&commit))
            || mq
                .as_ref()
                .is_some_and(|prepared| !prepared.accepts(&commit))
            || batch
                .as_ref()
                .is_some_and(|prepared| !prepared.accepts(&commit))
            || racf
                .as_ref()
                .is_some_and(|prepared| !prepared.accepts(&commit))
        {
            cics.fence_unknown();
            seed.fence_unknown();
            if let Some(prepared) = &db2 {
                prepared.fence_unknown();
            }
            if let Some(prepared) = &ims {
                prepared.fence_unknown();
            }
            if let Some(prepared) = &mq {
                prepared.fence_unknown();
            }
            if let Some(prepared) = &batch {
                prepared.fence_unknown();
            }
            if let Some(prepared) = &racf {
                prepared.fence_unknown();
            }
            self.local_composition_uncertain
                .store(true, Ordering::SeqCst);
            return Err(HostProblem::UnknownOutcome);
        }
        let seed_receipt = seed.finish(&commit)?;
        cics.finish(&commit)?;
        if let Some(prepared) = db2 {
            prepared.finish(&commit)?;
        }
        if let Some(prepared) = ims {
            prepared.finish(&commit)?;
        }
        if let Some(prepared) = mq {
            prepared.finish(&commit)?;
        }
        if let Some(prepared) = batch {
            prepared.finish(&commit)?;
        }
        if let Some(prepared) = racf {
            prepared.finish(&commit)?;
        }
        current_programs.extend(programs);
        current_transactions.extend(transactions);
        durable.installer = next;
        durable.store_version = installer_version;
        Ok(LocalApplicationReceipt {
            generation: actual,
            seed: seed_receipt,
            binding: selected.binding,
            compiled_sources: selected.compiled_sources,
            replayed: false,
        })
    }

    fn prepare_local_route(
        &self,
        namespace: &str,
        key: &str,
        payload: &[u8],
        cached: Option<&[u8]>,
        previous: Option<&(ProviderStateRecord, LocalSelection)>,
    ) -> Result<ProviderStateWrite, HostProblem> {
        let current = self
            .store
            .get_provider_state(namespace, key)
            .map_err(store_error)?;
        if cached != current.as_ref().map(|row| row.payload.as_slice()) {
            return Err(HostProblem::IdempotencyConflict);
        }
        if let Some(row) = &current
            && !previous.is_some_and(|(_, selected)| selected.rows.contains(row))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: current
                    .as_ref()
                    .map_or(Some(1), |row| row.version.checked_add(1))
                    .ok_or(HostProblem::ResourceExhausted)?,
                payload: payload.to_vec(),
            },
            expected_version: current.map(|row| row.version),
        })
    }

    fn local_selection(
        &self,
        package: &str,
    ) -> Result<Option<(ProviderStateRecord, LocalSelection)>, HostProblem> {
        self.store
            .get_provider_state(LOCAL_SELECTION, package)
            .map_err(store_error)?
            .map(|row| decode_local_selection(&row).map(|selection| (row, selection)))
            .transpose()
    }

    fn verify_static_local_rows(&self, selected: &LocalSelection) -> Result<(), HostProblem> {
        for row in &selected.rows {
            if self
                .store
                .get_provider_state(&row.namespace, &row.key)
                .map_err(store_error)?
                .as_ref()
                != Some(row)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        Ok(())
    }

    pub(super) fn require_local_owner_known(&self) -> Result<(), HostProblem> {
        if self.local_composition_uncertain.load(Ordering::SeqCst) {
            Err(HostProblem::UnknownOutcome)
        } else {
            Ok(())
        }
    }

    /// Validate selected composition against actual retained package, seed and static rows.
    /// Missing/partial/foreign old saga rows cannot supply this profile's readiness authority.
    pub fn selected_local_application(
        &self,
        package: &str,
    ) -> Result<Option<ApplicationGenerationRecord>, HostProblem> {
        let _publication = self
            .application_publication
            .read()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_local_owner_known()?;
        self.selected_local_application_locked(package)
    }

    fn selected_local_application_locked(
        &self,
        package: &str,
    ) -> Result<Option<ApplicationGenerationRecord>, HostProblem> {
        let Some((_, selected)) = self.local_selection(&package.to_ascii_uppercase())? else {
            return Ok(None);
        };
        if self
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, &selected.package)
            .map_err(store_error)?
            .is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.verify_static_local_rows(&selected)?;
        if let Some(catalog) = &selected.db2 {
            self.db2.selected_catalog_composition_matches(catalog)?;
        }
        if let Some(catalog) = &selected.ims {
            self.ims.selected_catalog_composition_matches(catalog)?;
        }
        if let Some(catalog) = &selected.mq {
            self.mq.selected_catalog_composition_matches(catalog)?;
        }
        if let Some(controllers) = &selected.batch {
            self.batch
                .selected_controller_composition_matches(controllers)?;
        }
        if !selected.racf.is_empty() {
            self.racf
                .database()
                .selected_profile_composition_matches(&selected.racf)?;
        }
        let durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let actual = durable
            .installer
            .selected(&selected.package)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if actual.identity != selected.identity
            || actual.generation != selected.generation
            || actual.state != InstallState::Ready
            || !self.dataset.selected_seed_matches(
                &selected.package,
                &selected.generation.to_string(),
                &selected.seed_identity,
                &selected.seed_snapshot,
            )?
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let row = self
            .store
            .get_provider_state(
                LOCAL_GENERATION,
                &format!("{}:{}", selected.package, selected.generation),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let generation = decode_local_selection(&row)?;
        if generation.binding != selected.binding || generation.identity != selected.identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(Some(actual))
    }

    pub(super) fn validate_local_compositions(&self) -> Result<(), HostProblem> {
        let limits = PackageLimits::default();
        let maximum = limits
            .max_applications
            .checked_mul(limits.max_retained_generations)
            .ok_or(HostProblem::ResourceExhausted)?;
        let generations = self
            .store
            .list_provider_state(LOCAL_GENERATION, maximum + 1)
            .map_err(store_error)?;
        let selections = self
            .store
            .list_provider_state(LOCAL_SELECTION, limits.max_applications + 1)
            .map_err(store_error)?;
        if generations.len() > maximum || selections.len() > limits.max_applications {
            return Err(HostProblem::ResourceExhausted);
        }
        for row in generations {
            let retained = decode_local_selection(&row)?;
            if self.local_selection(&retained.package)?.is_none() {
                return Err(HostProblem::InfrastructureFailure);
            }
            let durable = self
                .applications_v2
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let actual = durable
                .installer
                .generation(&retained.package, retained.generation, &retained.identity)
                .map_err(application_install_problem)?
                .ok_or(HostProblem::NotFound)?;
            if actual.record().state != InstallState::Ready {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        for row in selections {
            let selected = decode_local_selection(&row)?;
            if row.key != selected.package
                || self
                    .selected_local_application_locked(&selected.package)?
                    .is_none()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(())
    }

    pub(super) fn refuse_local_online_definition(
        &self,
        definition: &OnlineApplicationDefinition,
    ) -> Result<(), HostProblem> {
        self.require_local_owner_known()?;
        let programs = definition
            .programs
            .iter()
            .map(|item| normalize_online_name(&item.name, 128))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let transactions = definition
            .transactions
            .keys()
            .map(|name| normalize_online_name(name, 16))
            .collect::<Result<BTreeSet<_>, _>>()?;
        for row in self
            .store
            .list_provider_state(
                LOCAL_SELECTION,
                PackageLimits::default().max_applications + 1,
            )
            .map_err(store_error)?
        {
            let selected = decode_local_selection(&row)?;
            if selected.rows.iter().any(|row| {
                (row.namespace == "online-program" && programs.contains(&row.key))
                    || (row.namespace == "online-transaction" && transactions.contains(&row.key))
            }) {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        Ok(())
    }

    pub(super) fn refuse_local_legacy_selection(&self, package: &str) -> Result<(), HostProblem> {
        self.require_local_owner_known()?;
        if self
            .local_selection(&package.to_ascii_uppercase())?
            .is_some()
        {
            Err(HostProblem::Unsupported)
        } else {
            Ok(())
        }
    }
}

fn reject_unprepared_sections(package: &ApplicationPackageV2) -> Result<(), HostProblem> {
    let sections = &package.sections;
    if !sections.host_abi_libraries.is_empty()
        || !sections.ims_definitions.is_empty()
        || !sections.ims_rows.is_empty()
        || sections.ims_tm.is_some()
    {
        Err(HostProblem::Unsupported)
    } else {
        Ok(())
    }
}

fn bind_entry(
    package: &ApplicationPackageV2,
    kind: EntryKind,
    path: &str,
    bytes: &[u8],
    used: &mut BTreeSet<String>,
) -> Result<(), HostProblem> {
    let entry = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == path && entry.kind == kind)
        .ok_or(HostProblem::NotFound)?;
    if !used.insert(path.into())
        || entry.bytes != bytes.len()
        || entry.sha256 != format!("sha256:{:x}", Sha256::digest(bytes))
        || package.base.blobs.get(&entry.sha256).map(Vec::as_slice) != Some(bytes)
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

fn local_binding(
    identity: &str,
    seed: &str,
    snapshot: &str,
    sources: &BTreeMap<String, String>,
    profiles: &[RacfProfileProjection],
    participants: (
        Option<&Db2CatalogProjection>,
        Option<&ImsCatalogProjection>,
        Option<&MqCatalogProjection>,
        Option<&BatchControllerProjection>,
    ),
    rows: &[ProviderStateRecord],
) -> Result<String, HostProblem> {
    let (catalog, ims_catalog, mq_catalog, controllers) = participants;
    let mut digest = Sha256::new();
    for field in [
        LOCAL_CONTRACT.as_bytes(),
        identity.as_bytes(),
        seed.as_bytes(),
        snapshot.as_bytes(),
    ] {
        digest_online_field(&mut digest, field);
    }
    for (program, source) in sources {
        digest_online_field(&mut digest, program.as_bytes());
        digest_online_field(&mut digest, source.as_bytes());
    }
    for projection in profiles {
        digest_online_field(&mut digest, &projection.content_payload()?);
    }
    if let Some(catalog) = catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(catalog) = ims_catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(catalog) = mq_catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(controllers) = controllers {
        digest_online_field(&mut digest, &controllers.content_payload()?);
    }
    for row in rows {
        for field in [
            row.namespace.as_bytes(),
            row.key.as_bytes(),
            row.payload.as_slice(),
        ] {
            digest_online_field(&mut digest, field);
        }
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn encode_local_selection(selection: &LocalSelection) -> Result<Vec<u8>, HostProblem> {
    let bytes = serde_json::to_vec(selection).map_err(|_| HostProblem::InfrastructureFailure)?;
    if bytes.len() > MAX_PROVIDER_COMPOSITION_BYTES / 2 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}

fn decode_local_selection(row: &ProviderStateRecord) -> Result<LocalSelection, HostProblem> {
    if row.version == 0 || row.payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 2 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let value: LocalSelection =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if !matches!(row.namespace.as_str(), LOCAL_SELECTION | LOCAL_GENERATION)
        || value.schema_version != LOCAL_CONTRACT
        || value.package.is_empty()
        || value.generation == 0
        || value.rows.is_empty()
        || value.rows.len() > 16384
        || value.compiled_sources.is_empty()
        || value.compiled_sources.len() > 4096
        || value.compiled_sources.iter().any(|(name, identity)| {
            normalize_online_name(name, 128).as_deref() != Ok(name.as_str())
                || identity.len() != 64
                || !identity
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        || value
            .compiled_sources
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != value
                .rows
                .iter()
                .filter(|row| row.namespace == "online-program")
                .map(|row| row.key.as_str())
                .collect::<BTreeSet<_>>()
        || value
            .db2
            .as_ref()
            .is_some_and(|catalog| !db2::valid_projection(catalog, &value))
        || value
            .ims
            .as_ref()
            .is_some_and(|catalog| !ims::valid_projection(catalog, &value))
        || value
            .mq
            .as_ref()
            .is_some_and(|catalog| !mq::valid_projection(catalog, &value))
        || value
            .batch
            .as_ref()
            .is_some_and(|controllers| !batch::valid_projection(controllers, &value))
        || value.racf.len() > 4096
        || security::projection_keys(&value.racf).is_err()
        || value
            .racf
            .windows(2)
            .any(|pair| pair[0].key() >= pair[1].key())
        || value.binding
            != local_binding(
                &value.identity,
                &value.seed_identity,
                &value.seed_snapshot,
                &value.compiled_sources,
                &value.racf,
                (
                    value.db2.as_ref(),
                    value.ims.as_ref(),
                    value.mq.as_ref(),
                    value.batch.as_ref(),
                ),
                &value.rows,
            )?
        || value
            .rows
            .windows(2)
            .any(|pair| (&pair[0].namespace, &pair[0].key) >= (&pair[1].namespace, &pair[1].key))
        || (row.namespace == LOCAL_SELECTION && row.key != value.package)
        || (row.namespace == LOCAL_GENERATION
            && row.key != format!("{}:{}", value.package, value.generation))
        || value
            .rows
            .iter()
            .any(|row| row.validate_write(MAX_PROVIDER_COMPOSITION_BYTES).is_err())
        || value.rows.iter().any(|row| {
            !matches!(
                row.namespace.as_str(),
                "cics-program"
                    | "cics-map"
                    | "cics-file-alias"
                    | "online-program"
                    | "online-transaction"
                    | "batch-program"
            )
        })
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(value)
}
