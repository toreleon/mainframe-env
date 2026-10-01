use super::{
    ApplicationPackage, EntryKind, InstallProblem, InstallState, digest_field, package_identity,
    validate_package, validate_sha256, validate_text,
};
use mainframe_env_host_api::{
    ImsMetadataCatalog, ImsMetadataLimits, ImsMetadataProblem, validate_ims_metadata,
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
pub const MQ_SECTION_CONTRACT: &str = "mainframe-env.application.mq@1";
pub const BATCH_CONTROLLER_SECTION_CONTRACT: &str = "mainframe-env.application.batch-controllers@1";
pub const APPLICATION_INSTALLER_STATE_CONTRACT: &str = "mainframe-env.application-installer@1";
const APPLICATION_SECTION_COUNT: usize = 9;

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
            max_sections: 9,
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
struct ApplicationInstallerState {
    schema_version: String,
    applications: Vec<RetainedApplication>,
}

#[derive(Deserialize, Serialize)]
struct RetainedApplication {
    application: String,
    selected: Option<u64>,
    generations: Vec<RetainedPackage>,
}

#[derive(Deserialize, Serialize)]
struct RetainedPackage {
    package: ApplicationPackageV2,
    state: InstallState,
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
            .get_mut(&package.generation)
            .ok_or(InstallProblem::UnknownStage)?;
        if record.identity != identity {
            return Err(InstallProblem::IdentityConflict);
        }
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
        if state.schema_version != APPLICATION_INSTALLER_STATE_CONTRACT
            || state.applications.len() > limits.max_applications
        {
            return Err(InstallProblem::InvalidIdentity);
        }
        let installer = Self::new(product, limits, verifier);
        let mut names = BTreeSet::new();
        for application in state.applications {
            let normalized = application.application.to_ascii_uppercase();
            if application.generations.len() > limits.max_retained_generations
                || normalized != application.application
                || !names.insert(normalized.clone())
            {
                return Err(InstallProblem::LimitExceeded);
            }
            for retained in application.generations {
                if retained.package.base.manifest.name.to_ascii_uppercase() != normalized {
                    return Err(InstallProblem::InvalidIdentity);
                }
                installer.stage(&retained.package)?;
                if retained.state == InstallState::Ready {
                    installer.commit(&retained.package)?;
                }
            }
            if let Some(selected) = application.selected {
                installer.rollback(&normalized, selected)?;
            }
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

fn validate_aggregate_bounds(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<PackageFootprint, InstallProblem> {
    let sections = &package.sections;
    if let Some(metadata) = &sections.ims_metadata {
        validate_ims_metadata(metadata, ImsMetadataLimits::default()).map_err(|problem| {
            if problem == ImsMetadataProblem::LimitExceeded {
                InstallProblem::LimitExceeded
            } else {
                InstallProblem::MissingReference
            }
        })?;
    }
    let section_counts = [
        sections.host_abi_libraries.len(),
        sections.sql_tables.len(),
        sections.sql_rows.len(),
        sections.ims_definitions.len(),
        sections.ims_rows.len(),
        usize::from(sections.ims_metadata.is_some()),
        sections.mq_resources.len(),
        sections.batch_controllers.len(),
        sections.security_resources.len(),
    ];
    if section_counts
        .iter()
        .any(|count| *count > limits.max_items_per_section)
        || bounded_sum(section_counts, limits.max_total_nested_items).is_err()
    {
        return Err(InstallProblem::LimitExceeded);
    }
    if package.base.manifest.entries.len() > limits.max_manifest_entries
        || package.base.blobs.len() > limits.max_manifest_entries
        || package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.depends_on.len() > limits.max_dependencies_per_entry)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    validate_preflight_text(package, limits)?;
    let metadata_bytes = sections
        .ims_metadata
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| InstallProblem::InvalidIdentity)?
        .map_or(0, |bytes| bytes.len());
    let section_bytes = bounded_sum(
        section_text_lengths(package).chain(std::iter::once(metadata_bytes)),
        limits.max_total_section_bytes,
    )?;
    let blob_bytes = bounded_sum(
        package.base.blobs.values().map(Vec::len),
        limits.max_total_blob_bytes,
    )?;
    if sections
        .host_abi_libraries
        .iter()
        .any(|library| library.members.len() > limits.max_members_per_abi_library)
        || sections.sql_tables.iter().any(|table| {
            table.columns.len() > limits.max_columns_per_sql_table
                || table.primary_key.len() > limits.max_key_columns_per_sql_table
        })
        || sections
            .ims_definitions
            .iter()
            .any(|definition| definition.segments.len() > limits.max_segments_per_ims_definition)
        || sections
            .batch_controllers
            .iter()
            .any(|controller| controller.properties.len() > limits.max_properties_per_controller)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    let mut sql_rows = BTreeMap::<String, usize>::new();
    for row in &sections.sql_rows {
        let count = sql_rows.entry(row.table.to_ascii_uppercase()).or_default();
        *count = count.checked_add(1).ok_or(InstallProblem::LimitExceeded)?;
        if *count > limits.max_rows_per_sql_table {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    let mut ims_rows = BTreeMap::<String, usize>::new();
    for row in &sections.ims_rows {
        let count = ims_rows
            .entry(row.definition.to_ascii_uppercase())
            .or_default();
        *count = count.checked_add(1).ok_or(InstallProblem::LimitExceeded)?;
        if *count > limits.max_rows_per_ims_definition {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    let nested_items = bounded_sum(
        package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| 1usize.saturating_add(entry.depends_on.len()))
            .chain(
                sections
                    .host_abi_libraries
                    .iter()
                    .map(|item| 1 + item.members.len()),
            )
            .chain(
                sections
                    .sql_tables
                    .iter()
                    .map(|item| 1 + item.columns.len() + item.primary_key.len()),
            )
            .chain(sections.sql_rows.iter().map(|item| 1 + item.values.len()))
            .chain(
                sections
                    .ims_definitions
                    .iter()
                    .map(|item| 1 + item.segments.len()),
            )
            .chain(sections.ims_rows.iter().map(|item| 1 + item.values.len()))
            .chain(std::iter::once(metadata_nested_items(
                sections.ims_metadata.as_ref(),
            )?))
            .chain(sections.mq_resources.iter().map(|_| 3))
            .chain(
                sections
                    .batch_controllers
                    .iter()
                    .map(|item| 1 + item.properties.len()),
            )
            .chain(sections.security_resources.iter().map(|_| 3))
            .chain(std::iter::once(package.base.blobs.len()))
            .chain(std::iter::once(16)),
        limits.max_total_nested_items,
    )?;
    let structural_bytes = nested_items
        .checked_mul(256)
        .ok_or(InstallProblem::LimitExceeded)?;
    let bytes = blob_bytes
        .checked_add(section_bytes)
        .and_then(|bytes| bytes.checked_add(structural_bytes))
        .ok_or(InstallProblem::LimitExceeded)?;
    Ok(PackageFootprint {
        bytes,
        items: nested_items,
    })
}

fn validate_preflight_text(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    for value in [
        package.base.manifest.name.as_str(),
        package.base.manifest.version.as_str(),
        package.base.manifest.target_product.as_str(),
        package.sections.schema_version.as_str(),
        package.signature.algorithm.as_str(),
        package.signature.key_id.as_str(),
        package.signature.value.as_str(),
    ] {
        bounded_text(value, 256)?;
    }
    for digest in package.base.blobs.keys() {
        bounded_text(digest, 71)?;
    }
    for entry in &package.base.manifest.entries {
        bounded_text(&entry.path, 4_096)?;
        bounded_text(&entry.sha256, 71)?;
        for dependency in &entry.depends_on {
            bounded_text(dependency, 4_096)?;
        }
    }
    for library in &package.sections.host_abi_libraries {
        bounded_text(&library.id, 256)?;
        bounded_text(&library.version, 256)?;
        for member in &library.members {
            bounded_text(&member.name, 256)?;
            bounded_text(&member.blob_sha256, 71)?;
        }
    }
    for table in &package.sections.sql_tables {
        bounded_text(&table.name, 256)?;
        for column in &table.columns {
            bounded_text(&column.name, 256)?;
        }
        for key in &table.primary_key {
            bounded_text(key, 256)?;
        }
    }
    for row in &package.sections.sql_rows {
        bounded_text(&row.table, 256)?;
        bounded_values(&row.values, limits)?;
    }
    for definition in &package.sections.ims_definitions {
        bounded_text(&definition.name, 256)?;
        for segment in &definition.segments {
            bounded_text(segment, 256)?;
        }
    }
    for row in &package.sections.ims_rows {
        bounded_text(&row.definition, 256)?;
        bounded_text(&row.segment, 256)?;
        bounded_values(&row.values, limits)?;
    }
    for resource in &package.sections.mq_resources {
        bounded_text(&resource.name, 256)?;
        if let Some(target) = &resource.target {
            bounded_text(target, 256)?;
        }
        if let Some(controller) = &resource.controller {
            bounded_text(controller, 256)?;
        }
    }
    for controller in &package.sections.batch_controllers {
        bounded_text(&controller.name, 256)?;
        bounded_text(&controller.program, 4_096)?;
        bounded_values(&controller.properties, limits)?;
    }
    for resource in &package.sections.security_resources {
        bounded_text(&resource.class, 256)?;
        bounded_text(&resource.profile, 256)?;
        bounded_text(&resource.owner, 256)?;
    }
    Ok(())
}

fn metadata_nested_items(metadata: Option<&ImsMetadataCatalog>) -> Result<usize, InstallProblem> {
    let Some(metadata) = metadata else {
        return Ok(0);
    };
    let database_items = metadata
        .databases
        .iter()
        .try_fold(0usize, |total, database| {
            let segment_items = database
                .segments
                .iter()
                .try_fold(0usize, |total, segment| {
                    total
                        .checked_add(1 + segment.fields.len())
                        .ok_or(InstallProblem::LimitExceeded)
                })?;
            total
                .checked_add(1 + segment_items)
                .and_then(|total| total.checked_add(database.secondary_indexes.len()))
                .and_then(|total| total.checked_add(database.logical_relationships.len()))
                .ok_or(InstallProblem::LimitExceeded)
        })?;
    let psb_items = metadata.psbs.iter().try_fold(0usize, |total, psb| {
        let pcb_items = psb.pcbs.iter().try_fold(0usize, |total, pcb| {
            let sensitive = match pcb {
                mainframe_env_host_api::ImsPcbMetadata::Database(pcb) => {
                    pcb.sensitive_segments.len()
                }
                mainframe_env_host_api::ImsPcbMetadata::AlternateTerminal(_) => 0,
            };
            total
                .checked_add(1 + sensitive)
                .ok_or(InstallProblem::LimitExceeded)
        })?;
        total
            .checked_add(1 + pcb_items)
            .ok_or(InstallProblem::LimitExceeded)
    })?;
    database_items
        .checked_add(psb_items)
        .and_then(|total| total.checked_add(1))
        .ok_or(InstallProblem::LimitExceeded)
}

fn bounded_values(
    values: &BTreeMap<String, String>,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    if values.len() > limits.max_fields_per_record {
        return Err(InstallProblem::LimitExceeded);
    }
    for (name, value) in values {
        bounded_text(name, 256)?;
        bounded_text(value, limits.max_value_bytes)?;
    }
    Ok(())
}

fn bounded_text(value: &str, maximum: usize) -> Result<(), InstallProblem> {
    if value.len() > maximum {
        Err(InstallProblem::LimitExceeded)
    } else {
        Ok(())
    }
}

fn section_text_lengths(package: &ApplicationPackageV2) -> impl Iterator<Item = usize> + '_ {
    let sections = &package.sections;
    [
        package.base.manifest.name.len(),
        package.base.manifest.version.len(),
        package.base.manifest.target_product.len(),
        sections.schema_version.len(),
        package.signature.algorithm.len(),
        package.signature.key_id.len(),
        package.signature.value.len(),
    ]
    .into_iter()
    .chain(package.base.blobs.keys().map(String::len))
    .chain(package.base.manifest.entries.iter().flat_map(|entry| {
        std::iter::once(entry.path.len())
            .chain(std::iter::once(entry.sha256.len()))
            .chain(entry.depends_on.iter().map(String::len))
    }))
    .chain(sections.host_abi_libraries.iter().flat_map(|library| {
        [library.id.len(), library.version.len()].into_iter().chain(
            library
                .members
                .iter()
                .flat_map(|member| [member.name.len(), member.blob_sha256.len()]),
        )
    }))
    .chain(sections.sql_tables.iter().flat_map(|table| {
        std::iter::once(table.name.len())
            .chain(table.columns.iter().map(|column| column.name.len()))
            .chain(table.primary_key.iter().map(String::len))
    }))
    .chain(sections.sql_rows.iter().flat_map(|row| {
        std::iter::once(row.table.len()).chain(
            row.values
                .iter()
                .flat_map(|(name, value)| [name.len(), value.len()]),
        )
    }))
    .chain(sections.ims_definitions.iter().flat_map(|definition| {
        std::iter::once(definition.name.len()).chain(definition.segments.iter().map(String::len))
    }))
    .chain(sections.ims_rows.iter().flat_map(|row| {
        [row.definition.len(), row.segment.len()].into_iter().chain(
            row.values
                .iter()
                .flat_map(|(name, value)| [name.len(), value.len()]),
        )
    }))
    .chain(sections.mq_resources.iter().flat_map(|resource| {
        std::iter::once(resource.name.len())
            .chain(resource.target.iter().map(String::len))
            .chain(resource.controller.iter().map(String::len))
    }))
    .chain(sections.batch_controllers.iter().flat_map(|controller| {
        [controller.name.len(), controller.program.len()]
            .into_iter()
            .chain(
                controller
                    .properties
                    .iter()
                    .flat_map(|(name, value)| [name.len(), value.len()]),
            )
    }))
    .chain(sections.security_resources.iter().flat_map(|resource| {
        [
            resource.class.len(),
            resource.profile.len(),
            resource.owner.len(),
        ]
    }))
}

fn bounded_sum(
    values: impl IntoIterator<Item = usize>,
    maximum: usize,
) -> Result<usize, InstallProblem> {
    let mut total = 0usize;
    for value in values {
        total = total
            .checked_add(value)
            .filter(|total| *total <= maximum)
            .ok_or(InstallProblem::LimitExceeded)?;
    }
    Ok(total)
}

fn validate_sections(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    let sections = &package.sections;
    let program_paths = package
        .base
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Program)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let manifest_blobs = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.as_str())
        .collect::<BTreeSet<_>>();
    let mut abi_ids = BTreeSet::new();
    for library in &sections.host_abi_libraries {
        validate_text(&library.id)?;
        validate_text(&library.version)?;
        if !abi_ids.insert(library.id.to_ascii_uppercase()) || library.members.is_empty() {
            return Err(InstallProblem::DuplicateEntry);
        }
        let mut members = BTreeSet::new();
        for member in &library.members {
            validate_text(&member.name)?;
            validate_sha256(&member.blob_sha256)?;
            if !members.insert(member.name.to_ascii_uppercase())
                || !manifest_blobs.contains(member.blob_sha256.as_str())
            {
                return Err(InstallProblem::MissingReference);
            }
        }
    }
    let mut table_names = BTreeSet::new();
    let mut table_columns = BTreeMap::new();
    let mut required_columns = BTreeMap::new();
    for table in &sections.sql_tables {
        validate_text(&table.name)?;
        let normalized = table.name.to_ascii_uppercase();
        if !table_names.insert(normalized.clone()) || table.columns.is_empty() {
            return Err(InstallProblem::DuplicateEntry);
        }
        let mut columns = BTreeSet::new();
        let mut required = BTreeSet::new();
        for column in &table.columns {
            validate_text(&column.name)?;
            let column_name = column.name.to_ascii_uppercase();
            if !columns.insert(column_name.clone()) {
                return Err(InstallProblem::DuplicateEntry);
            }
            if !column.nullable {
                required.insert(column_name);
            }
        }
        if table.primary_key.is_empty()
            || table
                .primary_key
                .iter()
                .any(|key| !columns.contains(&key.to_ascii_uppercase()))
        {
            return Err(InstallProblem::MissingReference);
        }
        table_columns.insert(normalized.clone(), columns);
        required_columns.insert(normalized, required);
    }
    for row in &sections.sql_rows {
        let table = row.table.to_ascii_uppercase();
        validate_values(&row.values, limits)?;
        let columns = table_columns
            .get(&table)
            .ok_or(InstallProblem::MissingReference)?;
        let present = row
            .values
            .keys()
            .map(|column| column.to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
        if !present.is_subset(columns)
            || !required_columns
                .get(&table)
                .is_some_and(|required| required.is_subset(&present))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
    let mut ims = BTreeMap::new();
    for definition in &sections.ims_definitions {
        validate_text(&definition.name)?;
        if definition.segments.is_empty()
            || ims
                .insert(definition.name.to_ascii_uppercase(), &definition.segments)
                .is_some()
        {
            return Err(InstallProblem::DuplicateEntry);
        }
        for segment in &definition.segments {
            validate_text(segment)?;
        }
    }
    for row in &sections.ims_rows {
        validate_values(&row.values, limits)?;
        if !ims
            .get(&row.definition.to_ascii_uppercase())
            .is_some_and(|segments| segments.contains(&row.segment))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
    if let Some(metadata) = &sections.ims_metadata {
        validate_ims_metadata(metadata, ImsMetadataLimits::default()).map_err(|problem| {
            if problem == ImsMetadataProblem::LimitExceeded {
                InstallProblem::LimitExceeded
            } else {
                InstallProblem::MissingReference
            }
        })?;
    }
    let controllers = sections
        .batch_controllers
        .iter()
        .map(|controller| controller.name.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    if controllers.len() != sections.batch_controllers.len() {
        return Err(InstallProblem::DuplicateEntry);
    }
    for controller in &sections.batch_controllers {
        validate_text(&controller.name)?;
        validate_values(&controller.properties, limits)?;
        if !program_paths.contains(controller.program.as_str()) {
            return Err(InstallProblem::MissingReference);
        }
    }
    let resources = sections
        .mq_resources
        .iter()
        .map(|resource| resource.name.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    if resources.len() != sections.mq_resources.len() {
        return Err(InstallProblem::DuplicateEntry);
    }
    for resource in &sections.mq_resources {
        validate_text(&resource.name)?;
        if resource
            .target
            .as_ref()
            .is_some_and(|target| !resources.contains(&target.to_ascii_uppercase()))
            || resource
                .controller
                .as_ref()
                .is_some_and(|controller| !controllers.contains(&controller.to_ascii_uppercase()))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
    let mut security = BTreeSet::new();
    for resource in &sections.security_resources {
        validate_text(&resource.class)?;
        validate_text(&resource.profile)?;
        validate_text(&resource.owner)?;
        if !security.insert((
            resource.class.to_ascii_uppercase(),
            resource.profile.to_ascii_uppercase(),
        )) {
            return Err(InstallProblem::DuplicateEntry);
        }
    }
    Ok(())
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
        value["sections"]["ims_metadata"] = serde_json::Value::Null;
        let decoded: ApplicationPackageV2 = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.sections.ims_metadata, None);
        assert_eq!(package_v2_identity(&decoded).unwrap(), legacy_identity);
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
