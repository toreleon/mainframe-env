mod sections;
use sections::validate_sections;
mod preflight;
use preflight::*;

use super::{
    ApplicationPackage, EntryKind, InstallProblem, InstallState, digest_field, package_identity,
    validate_package, validate_sha256, validate_text,
};
use mainframe_env_host_api::{
    ImsMetadataCatalog, ImsMetadataLimits, ImsMetadataProblem, ImsPcbMetadata, TmDefinitionSet,
    TmDestination, TmLimits, validate_ims_metadata,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const APPLICATION_PACKAGE_V2_CONTRACT: &str = "mainframe-env.application-package@2";
pub const ABI_LIBRARY_SECTION_CONTRACT: &str = "mainframe-env.application.host-abi-libraries@1";
pub const SQL_SECTION_CONTRACT: &str = "mainframe-env.application.sql@1";
pub const SECURITY_RESOURCE_SECTION_CONTRACT: &str =
    "mainframe-env.application.security-resources@1";
pub const IMS_SECTION_CONTRACT: &str = "mainframe-env.application.ims@1";
pub const IMS_METADATA_SECTION_CONTRACT: &str = mainframe_env_host_api::IMS_METADATA_SCHEMA_V1;
pub const IMS_TM_SECTION_CONTRACT: &str = "mainframe-env.application.ims-tm@1";
pub const MQ_SECTION_CONTRACT: &str = "mainframe-env.application.mq@1";
pub const BATCH_CONTROLLER_SECTION_CONTRACT: &str = "mainframe-env.application.batch-controllers@1";
pub const APPLICATION_INSTALLER_STATE_CONTRACT: &str = "mainframe-env.application-installer@1";
const APPLICATION_SECTION_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum HostSubsystem {
    Cics,
    Db2,
    Ims,
    Mq,
}

impl HostSubsystem {
    const fn slug(self) -> &'static str {
        match self {
            Self::Cics => "cics",
            Self::Db2 => "db2",
            Self::Ims => "ims",
            Self::Mq => "mq",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AbiMember {
    pub name: String,
    pub blob_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AbiLibrary {
    pub id: String,
    pub subsystem: HostSubsystem,
    pub version: String,
    pub members: Vec<AbiMember>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SqlColumn {
    pub name: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SqlTable {
    pub name: String,
    pub columns: Vec<SqlColumn>,
    pub primary_key: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SqlSeedRow {
    pub table: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsDefinition {
    pub name: String,
    pub segments: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSeedRow {
    pub definition: String,
    pub segment: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum MqResourceKind {
    Queue,
    Process,
    Trigger,
}

impl MqResourceKind {
    const fn slug(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Process => "process",
            Self::Trigger => "trigger",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MqResource {
    pub name: String,
    pub kind: MqResourceKind,
    pub target: Option<String>,
    pub controller: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum BatchControllerKind {
    CobolProgram,
    ImsMessageProcessing,
    DeclarativeUtility,
}

impl BatchControllerKind {
    const fn slug(self) -> &'static str {
        match self {
            Self::CobolProgram => "cobol-program",
            Self::ImsMessageProcessing => "ims-message-processing",
            Self::DeclarativeUtility => "declarative-utility",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchController {
    pub name: String,
    pub program: String,
    pub kind: BatchControllerKind,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecurityResource {
    pub class: String,
    pub profile: String,
    pub owner: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationSections {
    pub schema_version: String,
    pub host_abi_libraries: Vec<AbiLibrary>,
    pub sql_tables: Vec<SqlTable>,
    pub sql_rows: Vec<SqlSeedRow>,
    pub ims_definitions: Vec<ImsDefinition>,
    pub ims_rows: Vec<ImsSeedRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ims_metadata: Option<ImsMetadataCatalog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ims_tm: Option<TmDefinitionSet>,
    pub mq_resources: Vec<MqResource>,
    pub batch_controllers: Vec<BatchController>,
    pub security_resources: Vec<SecurityResource>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PackageSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationPackageV2 {
    pub base: ApplicationPackage,
    pub generation: u64,
    pub sections: ApplicationSections,
    pub signature: PackageSignature,
}

pub trait PackageSignatureVerifier: Send + Sync {
    fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageLimits {
    pub max_sections: usize,
    pub max_items_per_section: usize,
    pub max_fields_per_record: usize,
    pub max_value_bytes: usize,
    pub max_manifest_entries: usize,
    pub max_dependencies_per_entry: usize,
    pub max_total_blob_bytes: usize,
    pub max_total_section_bytes: usize,
    pub max_total_nested_items: usize,
    pub max_members_per_abi_library: usize,
    pub max_columns_per_sql_table: usize,
    pub max_key_columns_per_sql_table: usize,
    pub max_rows_per_sql_table: usize,
    pub max_segments_per_ims_definition: usize,
    pub max_rows_per_ims_definition: usize,
    pub max_properties_per_controller: usize,
    pub max_applications: usize,
    pub max_retained_generations: usize,
    pub max_retained_package_bytes: usize,
    pub max_total_retained_package_bytes: usize,
    pub max_retained_nested_items: usize,
    pub max_total_retained_nested_items: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_sections: 10,
            max_items_per_section: 16_384,
            max_fields_per_record: 1_024,
            max_value_bytes: 1024 * 1024,
            max_manifest_entries: 65_536,
            max_dependencies_per_entry: 1_024,
            max_total_blob_bytes: 64 * 1024 * 1024,
            max_total_section_bytes: 64 * 1024 * 1024,
            max_total_nested_items: 262_144,
            max_members_per_abi_library: 4_096,
            max_columns_per_sql_table: 1_024,
            max_key_columns_per_sql_table: 64,
            max_rows_per_sql_table: 65_536,
            max_segments_per_ims_definition: 4_096,
            max_rows_per_ims_definition: 65_536,
            max_properties_per_controller: 1_024,
            max_applications: 1_024,
            max_retained_generations: 64,
            max_retained_package_bytes: 256 * 1024 * 1024,
            max_total_retained_package_bytes: 1024 * 1024 * 1024,
            max_retained_nested_items: 2_000_000,
            max_total_retained_nested_items: 8_000_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationGenerationRecord {
    pub package: String,
    pub version: String,
    pub generation: u64,
    pub identity: String,
    pub state: InstallState,
}

#[derive(Clone, Debug)]
pub struct SelectedApplicationGeneration {
    record: ApplicationGenerationRecord,
    package: Arc<ApplicationPackageV2>,
}

impl SelectedApplicationGeneration {
    #[must_use]
    pub fn record(&self) -> &ApplicationGenerationRecord {
        &self.record
    }

    #[must_use]
    pub fn package(&self) -> &ApplicationPackageV2 {
        self.package.as_ref()
    }
}

#[derive(Clone, Debug, Default)]
struct InstalledApplication {
    selected: Option<u64>,
    generations: BTreeMap<u64, ApplicationGenerationRecord>,
    packages: BTreeMap<u64, Arc<ApplicationPackageV2>>,
    retained_bytes: usize,
    retained_items: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationInstallerState {
    schema_version: String,
    applications: Vec<RetainedApplication>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetainedApplication {
    application: String,
    // Require an explicit selection or null; omission is a malformed retained state.
    #[serde(deserialize_with = "Option::deserialize")]
    selected: Option<u64>,
    generations: Vec<RetainedPackage>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetainedPackage {
    package: ApplicationPackageV2,
    state: InstallState,
}

fn validate_retained_state(
    state: &ApplicationInstallerState,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    if state.schema_version != APPLICATION_INSTALLER_STATE_CONTRACT {
        return Err(InstallProblem::InvalidIdentity);
    }
    if state.applications.len() > limits.max_applications {
        return Err(InstallProblem::LimitExceeded);
    }
    let mut names = BTreeSet::new();
    for application in &state.applications {
        validate_text(&application.application)?;
        if application.application.to_ascii_uppercase() != application.application {
            return Err(InstallProblem::InvalidIdentity);
        }
        if !names.insert(application.application.as_str()) {
            return Err(InstallProblem::DuplicateEntry);
        }
        if application.generations.len() > limits.max_retained_generations {
            return Err(InstallProblem::LimitExceeded);
        }
        let mut generations = BTreeMap::new();
        for retained in &application.generations {
            if retained.package.generation == 0
                || retained.package.base.manifest.name.to_ascii_uppercase()
                    != application.application
            {
                return Err(InstallProblem::InvalidIdentity);
            }
            if generations
                .insert(retained.package.generation, retained.state)
                .is_some()
            {
                return Err(InstallProblem::DuplicateEntry);
            }
        }
        // Null is a valid retained selection, including when Ready generations exist.
        if application
            .selected
            .is_some_and(|selected| generations.get(&selected) != Some(&InstallState::Ready))
        {
            return Err(InstallProblem::InvalidIdentity);
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct ApplicationInstallerV2 {
    product: String,
    limits: PackageLimits,
    verifier: Arc<dyn PackageSignatureVerifier>,
    applications: Arc<Mutex<BTreeMap<String, InstalledApplication>>>,
}

impl ApplicationInstallerV2 {
    #[must_use]
    pub fn new(
        product: impl Into<String>,
        limits: PackageLimits,
        verifier: Arc<dyn PackageSignatureVerifier>,
    ) -> Self {
        Self {
            product: product.into(),
            limits,
            verifier,
            applications: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn stage(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<ApplicationGenerationRecord, InstallProblem> {
        let ValidatedPackage {
            identity,
            footprint,
        } = validate_v2(package, &self.product, self.limits, self.verifier.as_ref())?;
        let key = package.base.manifest.name.to_ascii_uppercase();
        let mut applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        if !applications.contains_key(&key) && applications.len() >= self.limits.max_applications {
            return Err(InstallProblem::LimitExceeded);
        }
        if let Some(existing) = applications
            .get(&key)
            .and_then(|installed| installed.generations.get(&package.generation))
        {
            return if existing.identity == identity {
                Ok(existing.clone())
            } else {
                Err(InstallProblem::IdentityConflict)
            };
        }
        let _total_retained_bytes = applications
            .values()
            .try_fold(footprint.bytes, |total, installed| {
                total.checked_add(installed.retained_bytes)
            })
            .filter(|bytes| *bytes <= self.limits.max_total_retained_package_bytes)
            .ok_or(InstallProblem::LimitExceeded)?;
        let _total_retained_items = applications
            .values()
            .try_fold(footprint.items, |total, installed| {
                total.checked_add(installed.retained_items)
            })
            .filter(|items| *items <= self.limits.max_total_retained_nested_items)
            .ok_or(InstallProblem::LimitExceeded)?;
        let installed = applications.entry(key).or_default();
        if installed.generations.len() >= self.limits.max_retained_generations {
            return Err(InstallProblem::LimitExceeded);
        }
        let retained_bytes = installed
            .retained_bytes
            .checked_add(footprint.bytes)
            .filter(|bytes| *bytes <= self.limits.max_retained_package_bytes)
            .ok_or(InstallProblem::LimitExceeded)?;
        let retained_items = installed
            .retained_items
            .checked_add(footprint.items)
            .filter(|items| *items <= self.limits.max_retained_nested_items)
            .ok_or(InstallProblem::LimitExceeded)?;
        if installed
            .generations
            .last_key_value()
            .is_some_and(|(generation, _)| *generation >= package.generation)
        {
            return Err(InstallProblem::StaleGeneration);
        }
        let record = ApplicationGenerationRecord {
            package: package.base.manifest.name.clone(),
            version: package.base.manifest.version.clone(),
            generation: package.generation,
            identity,
            state: InstallState::Staged,
        };
        installed
            .generations
            .insert(package.generation, record.clone());
        installed
            .packages
            .insert(package.generation, Arc::new(package.clone()));
        installed.retained_bytes = retained_bytes;
        installed.retained_items = retained_items;
        Ok(record)
    }

    pub fn commit(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<ApplicationGenerationRecord, InstallProblem> {
        let identity =
            validate_v2(package, &self.product, self.limits, self.verifier.as_ref())?.identity;
        let key = package.base.manifest.name.to_ascii_uppercase();
        let mut applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let installed = applications
            .get_mut(&key)
            .ok_or(InstallProblem::UnknownStage)?;
        let record = installed
            .generations
            .get(&package.generation)
            .ok_or(InstallProblem::UnknownStage)?;
        if record.identity != identity {
            return Err(InstallProblem::IdentityConflict);
        }
        if record.state == InstallState::Ready {
            return Ok(record.clone());
        }
        // Explicit rollback does not make staged work older than a Ready generation current.
        if installed.generations.iter().any(|(generation, record)| {
            *generation > package.generation && record.state == InstallState::Ready
        }) {
            return Err(InstallProblem::StaleGeneration);
        }
        let record = installed
            .generations
            .get_mut(&package.generation)
            .ok_or(InstallProblem::UnknownStage)?;
        record.state = InstallState::Ready;
        installed.selected = Some(package.generation);
        Ok(record.clone())
    }

    pub fn install(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<ApplicationGenerationRecord, InstallProblem> {
        let staged = self.stage(package)?;
        if staged.state == InstallState::Ready {
            return Ok(staged);
        }
        self.commit(package)
    }

    pub fn rollback(
        &self,
        package: &str,
        generation: u64,
    ) -> Result<ApplicationGenerationRecord, InstallProblem> {
        let mut applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let installed = applications
            .get_mut(&package.to_ascii_uppercase())
            .ok_or(InstallProblem::UnknownStage)?;
        let record = installed
            .generations
            .get(&generation)
            .filter(|record| record.state == InstallState::Ready)
            .cloned()
            .ok_or(InstallProblem::UnknownStage)?;
        installed.selected = Some(generation);
        Ok(record)
    }

    pub fn selected(
        &self,
        package: &str,
    ) -> Result<Option<ApplicationGenerationRecord>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let Some(installed) = applications.get(&package.to_ascii_uppercase()) else {
            return Ok(None);
        };
        Ok(installed
            .selected
            .and_then(|generation| installed.generations.get(&generation))
            .filter(|record| record.state == InstallState::Ready)
            .cloned())
    }

    pub fn selected_package(
        &self,
        package: &str,
    ) -> Result<Option<Arc<ApplicationPackageV2>>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let Some(installed) = applications.get(&package.to_ascii_uppercase()) else {
            return Ok(None);
        };
        Ok(installed
            .selected
            .and_then(|generation| installed.packages.get(&generation))
            .cloned())
    }

    pub fn selected_generation(
        &self,
        package: &str,
    ) -> Result<Option<SelectedApplicationGeneration>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let Some(installed) = applications.get(&package.to_ascii_uppercase()) else {
            return Ok(None);
        };
        let Some(generation) = installed.selected else {
            return Ok(None);
        };
        let record = installed
            .generations
            .get(&generation)
            .filter(|record| record.state == InstallState::Ready)
            .cloned()
            .ok_or(InstallProblem::UnknownStage)?;
        let package = installed
            .packages
            .get(&generation)
            .cloned()
            .ok_or(InstallProblem::UnknownStage)?;
        Ok(Some(SelectedApplicationGeneration { record, package }))
    }

    pub fn retained_generation(
        &self,
        package: &str,
        generation: u64,
        identity: &str,
    ) -> Result<Option<SelectedApplicationGeneration>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let Some(installed) = applications.get(&package.to_ascii_uppercase()) else {
            return Ok(None);
        };
        let Some(record) = installed
            .generations
            .get(&generation)
            .filter(|record| record.state == InstallState::Ready && record.identity == identity)
            .cloned()
        else {
            return Ok(None);
        };
        let package = installed
            .packages
            .get(&generation)
            .cloned()
            .ok_or(InstallProblem::UnknownStage)?;
        Ok(Some(SelectedApplicationGeneration { record, package }))
    }

    pub fn generation(
        &self,
        package: &str,
        generation: u64,
        identity: &str,
    ) -> Result<Option<SelectedApplicationGeneration>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let Some(installed) = applications.get(&package.to_ascii_uppercase()) else {
            return Ok(None);
        };
        let Some(record) = installed
            .generations
            .get(&generation)
            .filter(|record| record.identity == identity)
            .cloned()
        else {
            return Ok(None);
        };
        let package = installed
            .packages
            .get(&generation)
            .cloned()
            .ok_or(InstallProblem::UnknownStage)?;
        Ok(Some(SelectedApplicationGeneration { record, package }))
    }

    pub fn state_payload(&self) -> Result<Vec<u8>, InstallProblem> {
        let applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let applications = applications
            .iter()
            .map(|(application, installed)| {
                let generations = installed
                    .packages
                    .iter()
                    .map(|(generation, package)| {
                        let state = installed
                            .generations
                            .get(generation)
                            .map(|record| record.state)
                            .ok_or(InstallProblem::UnknownStage)?;
                        Ok(RetainedPackage {
                            package: package.as_ref().clone(),
                            state,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(RetainedApplication {
                    application: application.clone(),
                    selected: installed.selected,
                    generations,
                })
            })
            .collect::<Result<Vec<_>, InstallProblem>>()?;
        let payload = serde_json::to_vec(&ApplicationInstallerState {
            schema_version: APPLICATION_INSTALLER_STATE_CONTRACT.into(),
            applications,
        })
        .map_err(|_| InstallProblem::InvalidIdentity)?;
        if payload.len() > self.limits.max_total_retained_package_bytes {
            Err(InstallProblem::LimitExceeded)
        } else {
            Ok(payload)
        }
    }

    pub fn from_state_payload(
        product: impl Into<String>,
        limits: PackageLimits,
        verifier: Arc<dyn PackageSignatureVerifier>,
        payload: &[u8],
    ) -> Result<Self, InstallProblem> {
        if payload.len() > limits.max_total_retained_package_bytes {
            return Err(InstallProblem::LimitExceeded);
        }
        let state: ApplicationInstallerState =
            serde_json::from_slice(payload).map_err(|_| InstallProblem::InvalidIdentity)?;
        validate_retained_state(&state, limits)?;
        let installer = Self::new(product, limits, verifier);
        for mut application in state.applications {
            application
                .generations
                .sort_by_key(|retained| retained.package.generation);
            for retained in &application.generations {
                installer.stage(&retained.package)?;
            }
            let mut applications = installer
                .applications
                .lock()
                .map_err(|_| InstallProblem::Poisoned)?;
            let installed = applications.entry(application.application).or_default();
            for retained in application.generations {
                installed
                    .generations
                    .get_mut(&retained.package.generation)
                    .ok_or(InstallProblem::UnknownStage)?
                    .state = retained.state;
            }
            installed.selected = application.selected;
        }
        Ok(installer)
    }
}

pub fn package_v2_identity(package: &ApplicationPackageV2) -> Result<String, InstallProblem> {
    let mut digest = Sha256::new();
    digest_field(&mut digest, APPLICATION_PACKAGE_V2_CONTRACT.as_bytes());
    digest_field(
        &mut digest,
        package_identity(&package.base.manifest)?.as_bytes(),
    );
    digest_field(&mut digest, &package.generation.to_be_bytes());
    digest_field(&mut digest, package.sections.schema_version.as_bytes());

    let mut abi = package
        .sections
        .host_abi_libraries
        .iter()
        .collect::<Vec<_>>();
    abi.sort_by_key(|library| &library.id);
    for library in abi {
        for field in [
            library.id.as_str(),
            library.subsystem.slug(),
            library.version.as_str(),
        ] {
            digest_field(&mut digest, field.as_bytes());
        }
        let mut members = library.members.iter().collect::<Vec<_>>();
        members.sort_by_key(|member| &member.name);
        for member in members {
            digest_field(&mut digest, member.name.as_bytes());
            digest_field(&mut digest, member.blob_sha256.as_bytes());
        }
    }
    let mut tables = package.sections.sql_tables.iter().collect::<Vec<_>>();
    tables.sort_by_key(|table| &table.name);
    for table in tables {
        digest_field(&mut digest, table.name.as_bytes());
        for column in &table.columns {
            digest_field(&mut digest, column.name.as_bytes());
            digest_field(&mut digest, &[u8::from(column.nullable)]);
        }
        for key in &table.primary_key {
            digest_field(&mut digest, key.as_bytes());
        }
    }
    let mut sql_rows = package.sections.sql_rows.iter().collect::<Vec<_>>();
    sql_rows.sort_by(|left, right| {
        left.table
            .cmp(&right.table)
            .then(left.values.cmp(&right.values))
    });
    for row in sql_rows {
        digest_field(&mut digest, row.table.as_bytes());
        digest_map(&mut digest, &row.values);
    }
    for definition in sorted_by(&package.sections.ims_definitions, |item| &item.name) {
        digest_field(&mut digest, definition.name.as_bytes());
        for segment in &definition.segments {
            digest_field(&mut digest, segment.as_bytes());
        }
    }
    let mut ims_rows = package.sections.ims_rows.iter().collect::<Vec<_>>();
    ims_rows.sort_by(|left, right| {
        left.definition
            .cmp(&right.definition)
            .then(left.segment.cmp(&right.segment))
            .then(left.values.cmp(&right.values))
    });
    for row in ims_rows {
        digest_field(&mut digest, row.definition.as_bytes());
        digest_field(&mut digest, row.segment.as_bytes());
        digest_map(&mut digest, &row.values);
    }
    if let Some(metadata) = &package.sections.ims_metadata {
        digest_field(&mut digest, IMS_METADATA_SECTION_CONTRACT.as_bytes());
        digest_field(
            &mut digest,
            &serde_json::to_vec(metadata).map_err(|_| InstallProblem::InvalidIdentity)?,
        );
    }
    if let Some(definitions) = &package.sections.ims_tm {
        digest_field(&mut digest, IMS_TM_SECTION_CONTRACT.as_bytes());
        digest_field(
            &mut digest,
            &serde_json::to_vec(definitions).map_err(|_| InstallProblem::InvalidIdentity)?,
        );
    }
    for resource in sorted_by(&package.sections.mq_resources, |item| &item.name) {
        digest_field(&mut digest, resource.name.as_bytes());
        digest_field(&mut digest, resource.kind.slug().as_bytes());
        digest_optional(&mut digest, resource.target.as_deref());
        digest_optional(&mut digest, resource.controller.as_deref());
    }
    for controller in sorted_by(&package.sections.batch_controllers, |item| &item.name) {
        digest_field(&mut digest, controller.name.as_bytes());
        digest_field(&mut digest, controller.program.as_bytes());
        digest_field(&mut digest, controller.kind.slug().as_bytes());
        digest_map(&mut digest, &controller.properties);
    }
    let mut security = package
        .sections
        .security_resources
        .iter()
        .collect::<Vec<_>>();
    security.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then(left.profile.cmp(&right.profile))
            .then(left.owner.cmp(&right.owner))
    });
    for resource in security {
        digest_field(&mut digest, resource.class.as_bytes());
        digest_field(&mut digest, resource.profile.as_bytes());
        digest_field(&mut digest, resource.owner.as_bytes());
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

struct ValidatedPackage {
    identity: String,
    footprint: PackageFootprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackageFootprint {
    bytes: usize,
    items: usize,
}

fn validate_v2(
    package: &ApplicationPackageV2,
    product: &str,
    limits: PackageLimits,
    verifier: &dyn PackageSignatureVerifier,
) -> Result<ValidatedPackage, InstallProblem> {
    let footprint = validate_aggregate_bounds(package, limits)?;
    validate_package(&package.base, product)?;
    if package.generation == 0
        || package.sections.schema_version != APPLICATION_PACKAGE_V2_CONTRACT
        || limits.max_sections < APPLICATION_SECTION_COUNT
    {
        return Err(InstallProblem::InvalidIdentity);
    }
    validate_text(&package.signature.algorithm)?;
    validate_text(&package.signature.key_id)?;
    validate_text(&package.signature.value)?;
    validate_sections(package, limits)?;
    let identity = package_v2_identity(package)?;
    if !verifier.verify(
        &package.signature.key_id,
        &package.signature.algorithm,
        &identity,
        &package.signature.value,
    ) {
        return Err(InstallProblem::InvalidSignature);
    }
    Ok(ValidatedPackage {
        identity,
        footprint,
    })
}

fn validate_values(
    values: &BTreeMap<String, String>,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    if values.is_empty() || values.len() > limits.max_fields_per_record {
        return Err(InstallProblem::LimitExceeded);
    }
    for (name, value) in values {
        validate_text(name)?;
        if value.len() > limits.max_value_bytes || value.chars().any(char::is_control) {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    Ok(())
}

fn sorted_by<'a, T, F>(values: &'a [T], key: F) -> Vec<&'a T>
where
    F: Fn(&'a T) -> &'a String,
{
    let mut values = values.iter().collect::<Vec<_>>();
    values.sort_by_key(|value| key(value));
    values
}

fn digest_map(digest: &mut Sha256, values: &BTreeMap<String, String>) {
    for (key, value) in values {
        digest_field(digest, key.as_bytes());
        digest_field(digest, value.as_bytes());
    }
}

fn digest_optional(digest: &mut Sha256, value: Option<&str>) {
    digest_field(digest, value.unwrap_or_default().as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_v1::sha256;
    use crate::{ApplicationManifest, EntryKind, PackageEntry};

    struct TestVerifier;

    impl PackageSignatureVerifier for TestVerifier {
        fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
            key_id == "test-key"
                && algorithm == "test-signature@1"
                && signature == format!("signed:{identity}")
        }
    }

    fn base_package() -> ApplicationPackage {
        let kinds = [
            EntryKind::Source,
            EntryKind::Resource,
            EntryKind::Program,
            EntryKind::Data,
            EntryKind::Profile,
            EntryKind::Migration,
        ];
        let mut entries = Vec::new();
        let mut blobs = BTreeMap::new();
        for (index, kind) in kinds.into_iter().enumerate() {
            let bytes = format!("payload-{index}").into_bytes();
            let digest = sha256(&bytes);
            blobs.insert(digest.clone(), bytes.clone());
            entries.push(PackageEntry {
                path: format!("{}/{index}", kind.slug()),
                kind,
                sha256: digest,
                bytes: bytes.len(),
                depends_on: (index > 0)
                    .then(|| "source/0".to_string())
                    .into_iter()
                    .collect(),
            });
        }
        ApplicationPackage {
            manifest: ApplicationManifest {
                name: "DEMO".into(),
                version: "2.0.0".into(),
                target_product: "0.2.0".into(),
                entries,
            },
            blobs,
        }
    }

    fn package(generation: u64) -> ApplicationPackageV2 {
        let base = base_package();
        let abi_blob = base.manifest.entries[0].sha256.clone();
        let sections = ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V2_CONTRACT.into(),
            host_abi_libraries: vec![AbiLibrary {
                id: "CICS-ABI".into(),
                subsystem: HostSubsystem::Cics,
                version: "1".into(),
                members: vec![AbiMember {
                    name: "DFHAID".into(),
                    blob_sha256: abi_blob,
                }],
            }],
            sql_tables: vec![SqlTable {
                name: "APP.TABLE".into(),
                columns: vec![
                    SqlColumn {
                        name: "ID".into(),
                        nullable: false,
                    },
                    SqlColumn {
                        name: "VALUE".into(),
                        nullable: true,
                    },
                ],
                primary_key: vec!["ID".into()],
            }],
            sql_rows: vec![SqlSeedRow {
                table: "APP.TABLE".into(),
                values: BTreeMap::from([("ID".into(), "1".into())]),
            }],
            ims_definitions: vec![ImsDefinition {
                name: "APPDB".into(),
                segments: BTreeSet::from(["ROOT".into()]),
            }],
            ims_rows: vec![ImsSeedRow {
                definition: "APPDB".into(),
                segment: "ROOT".into(),
                values: BTreeMap::from([("ID".into(), "1".into())]),
            }],
            ims_metadata: None,
            ims_tm: None,
            mq_resources: vec![MqResource {
                name: "APP.QUEUE".into(),
                kind: MqResourceKind::Queue,
                target: None,
                controller: Some("APP-CONTROLLER".into()),
            }],
            batch_controllers: vec![BatchController {
                name: "APP-CONTROLLER".into(),
                program: "program/2".into(),
                kind: BatchControllerKind::CobolProgram,
                properties: BTreeMap::from([("commit".into(), "step".into())]),
            }],
            security_resources: vec![SecurityResource {
                class: "FACILITY".into(),
                profile: "APP.EXECUTE".into(),
                owner: "APPADMIN".into(),
            }],
        };
        let mut package = ApplicationPackageV2 {
            base,
            generation,
            sections,
            signature: PackageSignature {
                algorithm: "test-signature@1".into(),
                key_id: "test-key".into(),
                value: "pending".into(),
            },
        };
        package.signature.value = format!("signed:{}", package_v2_identity(&package).unwrap());
        package
    }

    fn resign(package: &mut ApplicationPackageV2) {
        package.signature.value = format!("signed:{}", package_v2_identity(package).unwrap());
    }

    fn assert_limit(package: &ApplicationPackageV2, limits: PackageLimits) {
        let installer = ApplicationInstallerV2::new("0.2.0", limits, Arc::new(TestVerifier));
        assert_eq!(
            installer.install(package),
            Err(InstallProblem::LimitExceeded)
        );
        assert_eq!(installer.selected("DEMO").unwrap(), None);
    }

    #[test]
    fn all_subsystem_sections_are_reference_validated_before_staging() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        let mut invalid = package(1);
        invalid.sections.sql_rows[0].table = "MISSING".into();
        invalid.signature.value = format!("signed:{}", package_v2_identity(&invalid).unwrap());
        assert_eq!(
            installer.stage(&invalid),
            Err(InstallProblem::MissingReference)
        );
        assert_eq!(installer.selected("DEMO").unwrap(), None);
    }

    #[test]
    fn every_section_blob_reference_is_inside_the_verified_manifest_closure() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        let mut malicious = package(1);
        let original = b"unsigned-abi-original".to_vec();
        let digest = sha256(&original);
        malicious.base.blobs.insert(digest.clone(), original);
        malicious.sections.host_abi_libraries[0].members[0].blob_sha256 = digest.clone();
        resign(&mut malicious);
        malicious
            .base
            .blobs
            .insert(digest, b"changed-after-signing".to_vec());

        assert_eq!(
            installer.stage(&malicious),
            Err(InstallProblem::MissingReference)
        );
        assert_eq!(installer.selected("DEMO").unwrap(), None);

        for kind in [EntryKind::Program, EntryKind::Data] {
            let mut tampered = package(1);
            let entry = tampered
                .base
                .manifest
                .entries
                .iter()
                .find(|entry| entry.kind == kind)
                .unwrap();
            tampered
                .base
                .blobs
                .insert(entry.sha256.clone(), b"changed-under-signed-key".to_vec());
            assert_eq!(
                installer.stage(&tampered),
                Err(InstallProblem::ContentMismatch)
            );
        }
    }

    #[test]
    fn staged_or_corrupt_generations_are_never_selected_and_retry_is_atomic() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        let first = package(1);
        installer.install(&first).unwrap();
        let second = package(2);
        assert_eq!(
            installer.stage(&second).unwrap().state,
            InstallState::Staged
        );
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 1);
        assert_eq!(
            installer.install(&second).unwrap().state,
            InstallState::Ready
        );
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 2);
        assert_eq!(
            installer
                .selected_package("DEMO")
                .unwrap()
                .unwrap()
                .generation,
            2
        );
        let mut corrupt = package(3);
        corrupt.signature.value = "not-a-signature".into();
        assert_eq!(
            installer.install(&corrupt),
            Err(InstallProblem::InvalidSignature)
        );
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 2);
        assert_eq!(installer.rollback("DEMO", 1).unwrap().generation, 1);
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 1);
        assert_eq!(
            installer
                .selected_package("DEMO")
                .unwrap()
                .unwrap()
                .generation,
            1
        );
    }

    #[test]
    fn retained_ready_packages_revalidate_and_recover_from_durable_state() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        installer.install(&package(2)).unwrap();
        let payload = installer.state_payload().unwrap();

        let recovered = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &payload,
        )
        .unwrap();
        assert_eq!(recovered.selected("DEMO").unwrap().unwrap().generation, 2);
        recovered.rollback("DEMO", 1).unwrap();
        assert_eq!(recovered.selected("DEMO").unwrap().unwrap().generation, 1);

        let mut corrupt: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        corrupt["applications"][0]["generations"][0]["package"]["signature"]["value"] =
            serde_json::Value::String("forged".into());
        assert!(
            ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                Arc::new(TestVerifier),
                &serde_json::to_vec(&corrupt).unwrap(),
            )
            .is_err()
        );
    }

    #[test]
    fn generation_selection_ready_commit_retry_is_side_effect_free_after_rollback_and_reopen() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        let first = package(1);
        let second = package(2);
        installer.install(&first).unwrap();
        let before = installer.state_payload().unwrap();
        assert_eq!(installer.commit(&first).unwrap().state, InstallState::Ready);
        assert_eq!(installer.state_payload().unwrap(), before);

        installer.install(&second).unwrap();
        let before = installer.state_payload().unwrap();
        assert_eq!(installer.commit(&first).unwrap().state, InstallState::Ready);
        assert_eq!(installer.state_payload().unwrap(), before);
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 2);

        installer.rollback("DEMO", 1).unwrap();
        let before = installer.state_payload().unwrap();
        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &before,
        )
        .unwrap();
        assert_eq!(reopened.state_payload().unwrap(), before);
        assert_eq!(reopened.commit(&second).unwrap().state, InstallState::Ready);
        assert_eq!(
            reopened.install(&second).unwrap().state,
            InstallState::Ready
        );
        assert_eq!(reopened.state_payload().unwrap(), before);
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 1);
        reopened.rollback("DEMO", 2).unwrap();
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 2);
    }

    #[test]
    fn generation_selection_stale_staged_commit_refuses_without_mutation() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        let first = package(1);
        installer.stage(&first).unwrap();
        installer.install(&package(2)).unwrap();
        let before = installer.state_payload().unwrap();
        assert_eq!(
            installer.commit(&first),
            Err(InstallProblem::StaleGeneration)
        );
        assert_eq!(installer.state_payload().unwrap(), before);
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 2);

        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &before,
        )
        .unwrap();
        assert_eq!(
            reopened.commit(&first),
            Err(InstallProblem::StaleGeneration)
        );
        assert_eq!(reopened.state_payload().unwrap(), before);
    }

    #[test]
    fn generation_selection_stale_staged_commit_stays_stale_after_explicit_rollback() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        let second = package(2);
        installer.stage(&second).unwrap();
        installer.install(&package(3)).unwrap();
        installer.rollback("DEMO", 1).unwrap();
        let before = installer.state_payload().unwrap();
        assert_eq!(
            installer.commit(&second),
            Err(InstallProblem::StaleGeneration)
        );
        assert_eq!(installer.state_payload().unwrap(), before);
        installer.rollback("DEMO", 3).unwrap();
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 3);
    }

    #[test]
    fn generation_selection_conflicting_ready_commit_retry_refuses_without_mutation() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        installer.install(&package(2)).unwrap();
        let before = installer.state_payload().unwrap();
        let mut conflicting = package(1);
        conflicting.sections.sql_rows[0]
            .values
            .insert("VALUE".into(), "conflicting-generation".into());
        resign(&mut conflicting);
        assert_eq!(
            installer.commit(&conflicting),
            Err(InstallProblem::IdentityConflict)
        );
        assert_eq!(installer.state_payload().unwrap(), before);
        let mut corrupt = package(1);
        corrupt.signature.value = "invalid-signature".into();
        assert_eq!(
            installer.commit(&corrupt),
            Err(InstallProblem::InvalidSignature)
        );
        assert_eq!(installer.state_payload().unwrap(), before);
    }

    #[test]
    fn generation_selection_null_with_ready_generations_preserves_retained_rollback() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        installer.install(&package(2)).unwrap();
        let mut state: serde_json::Value =
            serde_json::from_slice(&installer.state_payload().unwrap()).unwrap();
        state["applications"][0]["selected"] = serde_json::Value::Null;
        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened.selected("DEMO").unwrap(), None);
        assert!(reopened.selected_generation("DEMO").unwrap().is_none());
        let before = reopened.state_payload().unwrap();
        assert_eq!(
            reopened.commit(&package(2)).unwrap().state,
            InstallState::Ready
        );
        assert_eq!(reopened.state_payload().unwrap(), before);
        reopened.rollback("DEMO", 1).unwrap();
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 1);
    }

    #[test]
    fn generation_selection_all_staged_null_round_trips_before_first_commit() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.stage(&package(1)).unwrap();
        let second = package(2);
        installer.stage(&second).unwrap();
        let before = installer.state_payload().unwrap();
        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &before,
        )
        .unwrap();
        assert_eq!(reopened.state_payload().unwrap(), before);
        assert_eq!(reopened.selected("DEMO").unwrap(), None);
        assert_eq!(
            reopened.rollback("DEMO", 1),
            Err(InstallProblem::UnknownStage)
        );
        assert_eq!(reopened.state_payload().unwrap(), before);
        assert_eq!(reopened.commit(&second).unwrap().state, InstallState::Ready);
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 2);
    }

    #[test]
    fn generation_selection_unordered_retained_generations_and_empty_application_reopen() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        installer.install(&package(2)).unwrap();
        installer.rollback("DEMO", 1).unwrap();
        let mut state: serde_json::Value =
            serde_json::from_slice(&installer.state_payload().unwrap()).unwrap();
        state["applications"][0]["generations"]
            .as_array_mut()
            .unwrap()
            .reverse();
        state["applications"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "application": "EMPTY",
                "selected": null,
                "generations": [],
            }));
        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 1);
        assert_eq!(reopened.selected("EMPTY").unwrap(), None);
        let recovered: serde_json::Value =
            serde_json::from_slice(&reopened.state_payload().unwrap()).unwrap();
        assert_eq!(recovered["applications"][1], state["applications"][1]);
        reopened.rollback("DEMO", 2).unwrap();
        assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 2);
    }

    struct RecoveryVerifier(std::sync::atomic::AtomicUsize);

    impl PackageSignatureVerifier for RecoveryVerifier {
        fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            TestVerifier.verify(key_id, algorithm, identity, signature)
        }
    }

    fn assert_invalid_retained_topology(payload: &[u8], expected: InstallProblem) {
        let verifier = Arc::new(RecoveryVerifier(std::sync::atomic::AtomicUsize::new(0)));
        let result = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            verifier.clone(),
            payload,
        );
        assert!(matches!(result, Err(problem) if problem == expected));
        assert_eq!(verifier.0.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn generation_selection_malformed_retained_topology_fails_before_verification() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        installer.stage(&package(2)).unwrap();
        let before = installer.state_payload().unwrap();
        let state: serde_json::Value = serde_json::from_slice(&before).unwrap();
        for selected in [0, 2, 99] {
            let mut invalid = state.clone();
            invalid["applications"][0]["selected"] = serde_json::json!(selected);
            assert_invalid_retained_topology(
                &serde_json::to_vec(&invalid).unwrap(),
                InstallProblem::InvalidIdentity,
            );
        }
        for duplicate_index in [0, 1] {
            let mut invalid = state.clone();
            let duplicate = invalid["applications"][0]["generations"][duplicate_index].clone();
            invalid["applications"][0]["generations"]
                .as_array_mut()
                .unwrap()
                .push(duplicate);
            assert_invalid_retained_topology(
                &serde_json::to_vec(&invalid).unwrap(),
                InstallProblem::DuplicateEntry,
            );
        }
        let mut later_invalid = state.clone();
        later_invalid["applications"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "application": "OTHER",
                "selected": 1,
                "generations": [],
            }));
        assert_invalid_retained_topology(
            &serde_json::to_vec(&later_invalid).unwrap(),
            InstallProblem::InvalidIdentity,
        );
        assert_eq!(installer.state_payload().unwrap(), before);
    }

    #[test]
    fn generation_selection_unknown_retained_fields_and_missing_selection_fail_closed() {
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package(1)).unwrap();
        let before = installer.state_payload().unwrap();
        let state: serde_json::Value = serde_json::from_slice(&before).unwrap();
        let mut root = state.clone();
        root["unknown"] = serde_json::json!(1);
        let mut application = state.clone();
        application["applications"][0]["unknown"] = serde_json::json!(1);
        let mut generation = state.clone();
        generation["applications"][0]["generations"][0]["unknown"] = serde_json::json!(1);
        let mut missing = state;
        missing["applications"][0]
            .as_object_mut()
            .unwrap()
            .remove("selected");
        for invalid in [root, application, generation, missing] {
            assert_invalid_retained_topology(
                &serde_json::to_vec(&invalid).unwrap(),
                InstallProblem::InvalidIdentity,
            );
        }
        assert_eq!(installer.state_payload().unwrap(), before);
    }

    #[test]
    fn package_identity_is_order_independent_but_section_content_sensitive() {
        let mut package = package(1);
        package.sections.sql_rows.push(SqlSeedRow {
            table: "APP.TABLE".into(),
            values: BTreeMap::from([("ID".into(), "2".into())]),
        });
        let expected = package_v2_identity(&package).unwrap();
        let mut reordered = package.clone();
        reordered.sections.sql_rows.reverse();
        assert_eq!(package_v2_identity(&reordered).unwrap(), expected);
        reordered.sections.sql_rows[0]
            .values
            .insert("VALUE".into(), "changed".into());
        assert_ne!(package_v2_identity(&reordered).unwrap(), expected);
    }

    #[test]
    fn optional_ims_metadata_preserves_legacy_package_wire_and_identity() {
        let package = package(1);
        let legacy_identity = package_v2_identity(&package).unwrap();
        let mut value = serde_json::to_value(&package).unwrap();
        assert!(value["sections"].get("ims_metadata").is_none());
        assert!(value["sections"].get("ims_tm").is_none());
        value["sections"]["ims_metadata"] = serde_json::Value::Null;
        value["sections"]["ims_tm"] = serde_json::Value::Null;
        let decoded: ApplicationPackageV2 = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.sections.ims_metadata, None);
        assert_eq!(decoded.sections.ims_tm, None);
        assert_eq!(package_v2_identity(&decoded).unwrap(), legacy_identity);

        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
        installer.install(&package).unwrap();
        installer.install(&self::package(2)).unwrap();
        installer.rollback("DEMO", 1).unwrap();
        let before = installer.state_payload().unwrap();
        for explicit_null in [false, true] {
            let mut state: serde_json::Value = serde_json::from_slice(&before).unwrap();
            for retained in state["applications"][0]["generations"]
                .as_array_mut()
                .unwrap()
            {
                let sections = &mut retained["package"]["sections"];
                for optional in ["ims_metadata", "ims_tm"] {
                    assert!(sections.get(optional).is_none());
                    if explicit_null {
                        sections[optional] = serde_json::Value::Null;
                    }
                }
            }
            let reopened = ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                Arc::new(TestVerifier),
                &serde_json::to_vec(&state).unwrap(),
            )
            .unwrap();
            let selected = reopened.selected("DEMO").unwrap().unwrap();
            assert_eq!(selected.generation, 1);
            assert_eq!(selected.identity, legacy_identity);
            assert_eq!(reopened.state_payload().unwrap(), before);
            reopened.rollback("DEMO", 2).unwrap();
            assert_eq!(reopened.selected("DEMO").unwrap().unwrap().generation, 2);
        }
    }

    #[test]
    fn hostile_nested_cardinalities_and_aggregate_bytes_fail_before_staging() {
        let baseline = package(1);
        macro_rules! limits {
            ($field:ident: $value:expr) => {
                PackageLimits {
                    $field: $value,
                    ..PackageLimits::default()
                }
            };
        }

        let limits = limits!(max_manifest_entries: baseline.base.manifest.entries.len() - 1);
        assert_limit(&baseline, limits);

        let limits = limits!(max_total_blob_bytes: baseline.base.blobs.values().map(Vec::len).sum::<usize>() - 1);
        assert_limit(&baseline, limits);

        let limits = limits!(max_members_per_abi_library: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_columns_per_sql_table: 1);
        assert_limit(&baseline, limits);

        let limits = limits!(max_key_columns_per_sql_table: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_rows_per_sql_table: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_segments_per_ims_definition: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_rows_per_ims_definition: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_properties_per_controller: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_applications: 0);
        assert_limit(&baseline, limits);

        let limits = limits!(max_total_nested_items: 1);
        assert_limit(&baseline, limits);

        let limits = limits!(max_total_section_bytes: 1);
        assert_limit(&baseline, limits);

        let mut oversized_name = package(1);
        oversized_name.sections.sql_rows[0].table = "X".repeat(1024 * 1024);
        assert_limit(&oversized_name, PackageLimits::default());
    }

    #[test]
    fn every_top_level_section_count_fails_in_allocation_free_preflight() {
        let baseline = package(1);
        let limits = PackageLimits {
            max_items_per_section: 1,
            ..PackageLimits::default()
        };
        let mut hostile = Vec::new();

        let mut candidate = baseline.clone();
        candidate
            .sections
            .host_abi_libraries
            .push(candidate.sections.host_abi_libraries[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .sql_tables
            .push(candidate.sections.sql_tables[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .sql_rows
            .push(candidate.sections.sql_rows[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .ims_definitions
            .push(candidate.sections.ims_definitions[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .ims_rows
            .push(candidate.sections.ims_rows[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .mq_resources
            .push(candidate.sections.mq_resources[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline.clone();
        candidate
            .sections
            .batch_controllers
            .push(candidate.sections.batch_controllers[0].clone());
        hostile.push(candidate);
        let mut candidate = baseline;
        candidate
            .sections
            .security_resources
            .push(candidate.sections.security_resources[0].clone());
        hostile.push(candidate);

        for candidate in hostile {
            assert_eq!(
                validate_aggregate_bounds(&candidate, limits),
                Err(InstallProblem::LimitExceeded)
            );
        }

        let high_cardinality_limits = PackageLimits {
            max_items_per_section: 1_024,
            ..PackageLimits::default()
        };
        let mut sql = package(1);
        sql.sections.sql_rows = (0..=1_024)
            .map(|index| SqlSeedRow {
                table: format!("APP.TABLE{index}"),
                values: BTreeMap::new(),
            })
            .collect();
        assert_eq!(
            validate_aggregate_bounds(&sql, high_cardinality_limits),
            Err(InstallProblem::LimitExceeded)
        );
        let mut ims = package(1);
        ims.sections.ims_rows = (0..=1_024)
            .map(|index| ImsSeedRow {
                definition: format!("APPDB{index}"),
                segment: "ROOT".into(),
                values: BTreeMap::new(),
            })
            .collect();
        assert_eq!(
            validate_aggregate_bounds(&ims, high_cardinality_limits),
            Err(InstallProblem::LimitExceeded)
        );
    }

    #[test]
    fn retained_generation_bytes_are_bounded_before_package_clone() {
        let first = package(1);
        let mut second = package(2);
        second.sections.sql_rows[0]
            .values
            .insert("VALUE".into(), "second-generation".into());
        resign(&mut second);

        let retained = validate_aggregate_bounds(&first, PackageLimits::default())
            .unwrap()
            .bytes;
        let limits = PackageLimits {
            max_retained_package_bytes: retained,
            ..PackageLimits::default()
        };
        let installer = ApplicationInstallerV2::new("0.2.0", limits, Arc::new(TestVerifier));
        installer.install(&first).unwrap();
        assert_eq!(
            installer.install(&second),
            Err(InstallProblem::LimitExceeded)
        );
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 1);

        let limits = PackageLimits {
            max_total_retained_package_bytes: retained,
            ..PackageLimits::default()
        };
        let installer = ApplicationInstallerV2::new("0.2.0", limits, Arc::new(TestVerifier));
        installer.install(&first).unwrap();
        let mut other = package(1);
        other.base.manifest.name = "OTHER".into();
        resign(&mut other);
        assert_eq!(
            installer.install(&other),
            Err(InstallProblem::LimitExceeded)
        );
    }

    #[test]
    fn sixty_four_small_generations_use_conservative_bytes_and_cardinality() {
        let first = package(1);
        let footprint = validate_aggregate_bounds(&first, PackageLimits::default()).unwrap();
        assert!(footprint.bytes >= footprint.items * 256);
        let limits = PackageLimits {
            max_retained_package_bytes: footprint.bytes * 64,
            max_total_retained_package_bytes: footprint.bytes * 64,
            max_retained_nested_items: footprint.items * 64,
            max_total_retained_nested_items: footprint.items * 64,
            ..PackageLimits::default()
        };
        let installer = ApplicationInstallerV2::new("0.2.0", limits, Arc::new(TestVerifier));
        for generation in 1..=64 {
            installer.install(&package(generation)).unwrap();
        }
        assert_eq!(installer.selected("DEMO").unwrap().unwrap().generation, 64);
        assert_eq!(
            installer.install(&package(65)),
            Err(InstallProblem::LimitExceeded)
        );

        let limits = PackageLimits {
            max_retained_nested_items: footprint.items * 2 - 1,
            ..PackageLimits::default()
        };
        let installer = ApplicationInstallerV2::new("0.2.0", limits, Arc::new(TestVerifier));
        installer.install(&first).unwrap();
        assert_eq!(
            installer.install(&package(2)),
            Err(InstallProblem::LimitExceeded)
        );
    }
}
