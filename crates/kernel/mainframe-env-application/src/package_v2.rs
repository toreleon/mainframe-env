use super::{
    ApplicationPackage, EntryKind, InstallProblem, InstallState, digest_field, package_identity,
    validate_package, validate_sha256, validate_text,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const APPLICATION_PACKAGE_V2_CONTRACT: &str = "mainframe-env.application-package@2";
pub const ABI_LIBRARY_SECTION_CONTRACT: &str = "mainframe-env.application.host-abi-libraries@1";
pub const SQL_SECTION_CONTRACT: &str = "mainframe-env.application.sql@1";
pub const SECURITY_RESOURCE_SECTION_CONTRACT: &str =
    "mainframe-env.application.security-resources@1";
pub const IMS_SECTION_CONTRACT: &str = "mainframe-env.application.ims@1";
pub const MQ_SECTION_CONTRACT: &str = "mainframe-env.application.mq@1";
pub const BATCH_CONTROLLER_SECTION_CONTRACT: &str = "mainframe-env.application.batch-controllers@1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbiMember {
    pub name: String,
    pub blob_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbiLibrary {
    pub id: String,
    pub subsystem: HostSubsystem,
    pub version: String,
    pub members: Vec<AbiMember>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlColumn {
    pub name: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlTable {
    pub name: String,
    pub columns: Vec<SqlColumn>,
    pub primary_key: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlSeedRow {
    pub table: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsDefinition {
    pub name: String,
    pub segments: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSeedRow {
    pub definition: String,
    pub segment: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqResource {
    pub name: String,
    pub kind: MqResourceKind,
    pub target: Option<String>,
    pub controller: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchController {
    pub name: String,
    pub program: String,
    pub kind: BatchControllerKind,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityResource {
    pub class: String,
    pub profile: String,
    pub owner: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSections {
    pub schema_version: String,
    pub host_abi_libraries: Vec<AbiLibrary>,
    pub sql_tables: Vec<SqlTable>,
    pub sql_rows: Vec<SqlSeedRow>,
    pub ims_definitions: Vec<ImsDefinition>,
    pub ims_rows: Vec<ImsSeedRow>,
    pub mq_resources: Vec<MqResource>,
    pub batch_controllers: Vec<BatchController>,
    pub security_resources: Vec<SecurityResource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    pub max_retained_generations: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_sections: 8,
            max_items_per_section: 16_384,
            max_fields_per_record: 1_024,
            max_value_bytes: 1024 * 1024,
            max_retained_generations: 64,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationGenerationRecord {
    pub package: String,
    pub version: String,
    pub generation: u64,
    pub identity: String,
    pub state: InstallState,
}

#[derive(Clone, Debug, Default)]
struct InstalledApplication {
    selected: Option<u64>,
    generations: BTreeMap<u64, ApplicationGenerationRecord>,
    packages: BTreeMap<u64, Arc<ApplicationPackageV2>>,
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
        let identity = validate_v2(package, &self.product, self.limits, self.verifier.as_ref())?;
        let key = package.base.manifest.name.to_ascii_uppercase();
        let mut applications = self
            .applications
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?;
        let installed = applications.entry(key).or_default();
        if let Some(existing) = installed.generations.get(&package.generation) {
            return if existing.identity == identity {
                Ok(existing.clone())
            } else {
                Err(InstallProblem::IdentityConflict)
            };
        }
        if installed.generations.len() >= self.limits.max_retained_generations {
            return Err(InstallProblem::LimitExceeded);
        }
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
        Ok(record)
    }

    pub fn commit(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<ApplicationGenerationRecord, InstallProblem> {
        let identity = validate_v2(package, &self.product, self.limits, self.verifier.as_ref())?;
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

fn validate_v2(
    package: &ApplicationPackageV2,
    product: &str,
    limits: PackageLimits,
    verifier: &dyn PackageSignatureVerifier,
) -> Result<String, InstallProblem> {
    validate_package(&package.base, product)?;
    if package.generation == 0
        || package.sections.schema_version != APPLICATION_PACKAGE_V2_CONTRACT
        || limits.max_sections < 6
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
    Ok(identity)
}

fn validate_sections(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    let sections = &package.sections;
    for count in [
        sections.host_abi_libraries.len(),
        sections.sql_tables.len(),
        sections.sql_rows.len(),
        sections.ims_definitions.len(),
        sections.ims_rows.len(),
        sections.mq_resources.len(),
        sections.batch_controllers.len(),
        sections.security_resources.len(),
    ] {
        if count > limits.max_items_per_section {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    let program_paths = package
        .base
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Program)
        .map(|entry| entry.path.as_str())
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
                || !package.base.blobs.contains_key(&member.blob_sha256)
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
    use crate::sha256;
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
}
