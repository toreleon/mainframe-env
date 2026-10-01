use crate::ims_pcb::ImsPcbKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const IMS_METADATA_SCHEMA_V1: &str = "mainframe-env.ims-metadata@1";
const DIGEST_DOMAIN: &[u8] = b"mainframe-env.ims-metadata@1\0";
const MAX_DATABASE_VERSION: u32 = i32::MAX as u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsMetadataLimits {
    pub max_databases: usize,
    pub max_psbs: usize,
    pub max_segments_per_database: usize,
    pub max_dedb_segments: usize,
    pub max_hierarchy_depth: usize,
    pub max_fields_per_segment: usize,
    pub max_fields_per_database: usize,
    pub max_named_fields_and_indexes: usize,
    pub max_secondary_indexes_per_segment: usize,
    pub max_relationships_per_database: usize,
    pub max_pcbs_per_psb: usize,
    pub max_sensitive_segments_per_psb: usize,
    pub max_segment_bytes: usize,
}

impl Default for ImsMetadataLimits {
    fn default() -> Self {
        Self {
            max_databases: 64,
            max_psbs: 256,
            max_segments_per_database: 255,
            max_dedb_segments: 127,
            max_hierarchy_depth: 15,
            max_fields_per_segment: 255,
            max_fields_per_database: 20_000,
            max_named_fields_and_indexes: 1_000,
            max_secondary_indexes_per_segment: 32,
            max_relationships_per_database: 255,
            max_pcbs_per_psb: 2_500,
            max_sensitive_segments_per_psb: 30_000,
            max_segment_bytes: 32 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsDatabaseOrganization {
    #[serde(rename = "DEDB")]
    Dedb,
    #[serde(rename = "GSAM")]
    Gsam,
    #[serde(rename = "HDAM")]
    Hdam,
    #[serde(rename = "HIDAM")]
    Hidam,
    #[serde(rename = "HISAM")]
    Hisam,
    #[serde(rename = "HSAM")]
    Hsam,
    #[serde(rename = "INDEX")]
    Index,
    #[serde(rename = "MSDB")]
    Msdb,
    #[serde(rename = "PHDAM")]
    Phdam,
    #[serde(rename = "PHIDAM")]
    Phidam,
    #[serde(rename = "PSINDEX")]
    Psindex,
    #[serde(rename = "SHISAM")]
    Shisam,
    #[serde(rename = "SHSAM")]
    Shsam,
}

impl ImsDatabaseOrganization {
    fn segment_limit(self, limits: ImsMetadataLimits) -> usize {
        match self {
            Self::Dedb => limits.max_dedb_segments,
            Self::Gsam | Self::Msdb => 1,
            _ => limits.max_segments_per_database,
        }
    }

    fn requires_unique_root_sequence(self) -> bool {
        matches!(
            self,
            Self::Dedb
                | Self::Hidam
                | Self::Hisam
                | Self::Index
                | Self::Msdb
                | Self::Phidam
                | Self::Shisam
        )
    }

    fn supports_secondary_indexes(self) -> bool {
        matches!(
            self,
            Self::Dedb | Self::Hdam | Self::Hidam | Self::Hisam | Self::Phdam | Self::Phidam
        )
    }

    fn supports_logical_relationships(self) -> bool {
        matches!(
            self,
            Self::Dedb | Self::Hdam | Self::Hidam | Self::Hisam | Self::Phdam | Self::Phidam
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsFieldMetadata {
    pub name: Option<String>,
    pub offset: usize,
    pub length: usize,
    pub sequence: bool,
    pub unique: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsSegmentMetadata {
    pub name: String,
    pub parent: Option<String>,
    pub min_length: usize,
    pub max_length: usize,
    pub fields: Vec<ImsFieldMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsSecondaryIndexMetadata {
    pub name: String,
    pub target_segment: String,
    pub source_segment: String,
    pub source_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsLogicalRelationshipMetadata {
    pub parent_database: String,
    pub parent_segment: String,
    pub child_database: String,
    pub child_segment: String,
    pub paired: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsDatabaseMetadata {
    pub name: String,
    pub version: u32,
    pub organization: ImsDatabaseOrganization,
    pub segments: Vec<ImsSegmentMetadata>,
    pub secondary_indexes: Vec<ImsSecondaryIndexMetadata>,
    pub logical_relationships: Vec<ImsLogicalRelationshipMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsSensitiveSegmentMetadata {
    pub name: String,
    pub parent: Option<String>,
    pub processing_options: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsDatabasePcbMetadata {
    pub name: String,
    pub database: String,
    pub database_version: Option<u32>,
    pub processing_options: String,
    pub sensitive_segments: Vec<ImsSensitiveSegmentMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsTerminalPcbMetadata {
    pub name: String,
    pub destination: Option<String>,
    pub modifiable: bool,
    pub express: bool,
    pub same_terminal: bool,
    pub response_mode: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ImsPcbMetadata {
    Database(ImsDatabasePcbMetadata),
    AlternateTerminal(ImsTerminalPcbMetadata),
}

impl ImsPcbMetadata {
    /// Maps the metadata variant to the shared host API PCB vocabulary.
    pub fn host_kind(&self) -> ImsPcbKind {
        match self {
            Self::Database(_) => ImsPcbKind::Database,
            Self::AlternateTerminal(_) => ImsPcbKind::Alternate,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImsDbLevel {
    Current,
    Base,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsPsbMetadata {
    pub name: String,
    pub database_level: ImsDbLevel,
    pub pcbs: Vec<ImsPcbMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsMetadataCatalog {
    pub schema_version: String,
    pub databases: Vec<ImsDatabaseMetadata>,
    pub psbs: Vec<ImsPsbMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsMetadataIdentity {
    pub databases: usize,
    pub segments: usize,
    pub fields: usize,
    pub secondary_indexes: usize,
    pub logical_relationships: usize,
    pub psbs: usize,
    pub pcbs: usize,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsMetadataProblem {
    UnsupportedSchema,
    LimitExceeded,
    InvalidName,
    DuplicateName,
    MissingReference,
    Cycle,
    InvalidOption,
    InvalidOffset,
    IncompatibleReference,
    Encoding,
}

pub fn validate_ims_metadata(
    catalog: &ImsMetadataCatalog,
    limits: ImsMetadataLimits,
) -> Result<ImsMetadataIdentity, ImsMetadataProblem> {
    if catalog.schema_version != IMS_METADATA_SCHEMA_V1 {
        return Err(ImsMetadataProblem::UnsupportedSchema);
    }
    if catalog.databases.is_empty()
        || catalog.databases.len() > limits.max_databases
        || catalog.psbs.len() > limits.max_psbs
    {
        return Err(ImsMetadataProblem::LimitExceeded);
    }

    let mut databases = BTreeMap::new();
    for database in &catalog.databases {
        validate_name(&database.name)?;
        if database.version > MAX_DATABASE_VERSION
            || databases
                .insert(normalize(&database.name), database)
                .is_some()
        {
            return Err(if database.version > MAX_DATABASE_VERSION {
                ImsMetadataProblem::LimitExceeded
            } else {
                ImsMetadataProblem::DuplicateName
            });
        }
        validate_database(database, limits)?;
    }
    for database in &catalog.databases {
        validate_relationships(database, &databases, limits)?;
    }

    let mut psb_names = BTreeSet::new();
    for psb in &catalog.psbs {
        validate_name(&psb.name)?;
        let name = normalize(&psb.name);
        if databases.contains_key(&name) || !psb_names.insert(name) {
            return Err(ImsMetadataProblem::DuplicateName);
        }
        validate_psb(psb, &databases, limits)?;
    }

    let bytes = serde_json::to_vec(catalog).map_err(|_| ImsMetadataProblem::Encoding)?;
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    digest.update(bytes);
    Ok(ImsMetadataIdentity {
        databases: catalog.databases.len(),
        segments: catalog
            .databases
            .iter()
            .map(|database| database.segments.len())
            .sum(),
        fields: catalog
            .databases
            .iter()
            .flat_map(|database| &database.segments)
            .map(|segment| segment.fields.len())
            .sum(),
        secondary_indexes: catalog
            .databases
            .iter()
            .map(|database| database.secondary_indexes.len())
            .sum(),
        logical_relationships: catalog
            .databases
            .iter()
            .map(|database| database.logical_relationships.len())
            .sum(),
        psbs: catalog.psbs.len(),
        pcbs: catalog.psbs.iter().map(|psb| psb.pcbs.len()).sum(),
        digest: format!("sha256:{:x}", digest.finalize()),
    })
}

// Sources for DBDGEN/DBD/SEGM/FIELD/XDFLD bounds and index rules:
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_dbdgenstmt.htm sha256:ae672b28c57e88f04a51ca54c152196f5f86eba67af3f91e99277b9d3676d53a
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_dbdstmt.htm sha256:ce1b4b70aa0b5803d25e5d72a71004cffb071361fcbc93a658bd3ce4e17cf7cd
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_segmstmt.htm sha256:014bf18fd4c89941bd6bafe3dccd66e607daf0523e618648334ca251eb2824d9
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_fieldstmt.htm sha256:94455fbdc9d5f7404c89c0fd73a813fee237fb851fc0d2348a96ba1ce9d36fcb
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_xdfldstmt.htm sha256:1954204faddc7fc6f55e5942342c3edbd2114781c136e4c0276794dc1818fce8
fn validate_database(
    database: &ImsDatabaseMetadata,
    limits: ImsMetadataLimits,
) -> Result<(), ImsMetadataProblem> {
    if database.segments.is_empty()
        || database.segments.len() > database.organization.segment_limit(limits)
        || database.secondary_indexes.len() > limits.max_named_fields_and_indexes
        || database.logical_relationships.len() > limits.max_relationships_per_database
    {
        return Err(ImsMetadataProblem::LimitExceeded);
    }
    let mut segments = BTreeMap::new();
    for segment in &database.segments {
        validate_name(&segment.name)?;
        if segments.insert(normalize(&segment.name), segment).is_some() {
            return Err(ImsMetadataProblem::DuplicateName);
        }
    }
    let mut all_fields = 0usize;
    let mut named_fields = 0usize;
    for segment in &database.segments {
        if segment.min_length == 0
            || segment.min_length > segment.max_length
            || segment.max_length > limits.max_segment_bytes
            || segment.fields.len() > limits.max_fields_per_segment
        {
            return Err(ImsMetadataProblem::LimitExceeded);
        }
        if let Some(parent) = &segment.parent
            && (!segments.contains_key(&normalize(parent))
                || normalize(parent) == normalize(&segment.name))
        {
            return Err(ImsMetadataProblem::MissingReference);
        }
        hierarchy_depth(segment, &segments, limits.max_hierarchy_depth)?;

        let mut names = BTreeSet::new();
        let mut sequence = None;
        for field in &segment.fields {
            all_fields = all_fields
                .checked_add(1)
                .ok_or(ImsMetadataProblem::LimitExceeded)?;
            if field.length == 0
                || field
                    .offset
                    .checked_add(field.length)
                    .is_none_or(|end| end > segment.max_length)
            {
                return Err(ImsMetadataProblem::InvalidOffset);
            }
            if let Some(name) = &field.name {
                validate_name(name)?;
                named_fields = named_fields
                    .checked_add(1)
                    .ok_or(ImsMetadataProblem::LimitExceeded)?;
                if !names.insert(normalize(name)) {
                    return Err(ImsMetadataProblem::DuplicateName);
                }
            }
            if field.sequence
                && (field.name.is_none()
                    || field
                        .offset
                        .checked_add(field.length)
                        .is_none_or(|end| end > segment.min_length)
                    || sequence.replace(field).is_some())
            {
                return Err(ImsMetadataProblem::InvalidOffset);
            }
        }
        if segment.parent.is_none()
            && database.organization.requires_unique_root_sequence()
            && !sequence.is_some_and(|field| field.unique)
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
    }

    let mut index_names = BTreeSet::new();
    let mut indexes_by_target = BTreeMap::<String, usize>::new();
    for index in &database.secondary_indexes {
        if !database.organization.supports_secondary_indexes() {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
        validate_name(&index.name)?;
        if !index_names.insert(normalize(&index.name)) {
            return Err(ImsMetadataProblem::DuplicateName);
        }
        let target = segments
            .get(&normalize(&index.target_segment))
            .ok_or(ImsMetadataProblem::MissingReference)?;
        let source = segments
            .get(&normalize(&index.source_segment))
            .ok_or(ImsMetadataProblem::MissingReference)?;
        let count = indexes_by_target
            .entry(normalize(&target.name))
            .or_default();
        *count += 1;
        if *count > limits.max_secondary_indexes_per_segment
            || index.source_fields.is_empty()
            || index.source_fields.len() > limits.max_fields_per_segment
        {
            return Err(ImsMetadataProblem::LimitExceeded);
        }
        let source_fields = source
            .fields
            .iter()
            .filter_map(|field| field.name.as_ref().map(|name| (normalize(name), field)))
            .collect::<BTreeMap<_, _>>();
        let mut length = 0usize;
        let mut selected = BTreeSet::new();
        for field in &index.source_fields {
            validate_name(field)?;
            let field = source_fields
                .get(&normalize(field))
                .ok_or(ImsMetadataProblem::MissingReference)?;
            if !selected.insert(normalize(field.name.as_deref().unwrap_or_default())) {
                return Err(ImsMetadataProblem::DuplicateName);
            }
            length = length
                .checked_add(field.length)
                .ok_or(ImsMetadataProblem::LimitExceeded)?;
        }
        if length > 240 {
            return Err(ImsMetadataProblem::LimitExceeded);
        }
    }

    if segments
        .values()
        .filter(|segment| segment.parent.is_none())
        .count()
        != 1
    {
        return Err(ImsMetadataProblem::IncompatibleReference);
    }

    let indexed = database.secondary_indexes.len();
    if database.segments.iter().any(|segment| {
        segment.fields.len()
            + indexes_by_target
                .get(&normalize(&segment.name))
                .copied()
                .unwrap_or_default()
            > limits.max_fields_per_segment
    }) || all_fields
        .checked_add(indexed)
        .is_none_or(|count| count > limits.max_fields_per_database)
        || named_fields
            .checked_add(indexed)
            .is_none_or(|count| count > limits.max_named_fields_and_indexes)
    {
        return Err(ImsMetadataProblem::LimitExceeded);
    }
    Ok(())
}

fn hierarchy_depth(
    segment: &ImsSegmentMetadata,
    segments: &BTreeMap<String, &ImsSegmentMetadata>,
    maximum: usize,
) -> Result<usize, ImsMetadataProblem> {
    let mut depth = 1usize;
    let mut current = segment;
    let mut seen = BTreeSet::from([normalize(&segment.name)]);
    while let Some(parent) = &current.parent {
        let parent = normalize(parent);
        if !seen.insert(parent.clone()) {
            return Err(ImsMetadataProblem::Cycle);
        }
        current = segments
            .get(&parent)
            .copied()
            .ok_or(ImsMetadataProblem::MissingReference)?;
        depth += 1;
        if depth > maximum {
            return Err(ImsMetadataProblem::LimitExceeded);
        }
    }
    Ok(depth)
}

// Sources for LCHILD relationship closure:
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_lchildstmt.htm sha256:518642c5f4fdbddb5f8a9ebc35dc609e5aeffa16b4951e99c2e655e285fcb9f8
fn validate_relationships(
    database: &ImsDatabaseMetadata,
    databases: &BTreeMap<String, &ImsDatabaseMetadata>,
    limits: ImsMetadataLimits,
) -> Result<(), ImsMetadataProblem> {
    if database.logical_relationships.len() > limits.max_relationships_per_database
        || (!database.logical_relationships.is_empty()
            && !database.organization.supports_logical_relationships())
    {
        return Err(ImsMetadataProblem::IncompatibleReference);
    }
    let mut relationships = BTreeSet::new();
    for relationship in &database.logical_relationships {
        for name in [
            &relationship.parent_database,
            &relationship.parent_segment,
            &relationship.child_database,
            &relationship.child_segment,
        ] {
            validate_name(name)?;
        }
        let parent = databases
            .get(&normalize(&relationship.parent_database))
            .copied()
            .ok_or(ImsMetadataProblem::MissingReference)?;
        let child = databases
            .get(&normalize(&relationship.child_database))
            .copied()
            .ok_or(ImsMetadataProblem::MissingReference)?;
        if !parent.organization.supports_logical_relationships()
            || !child.organization.supports_logical_relationships()
            || !has_segment(parent, &relationship.parent_segment)
            || !has_segment(child, &relationship.child_segment)
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
        let key = (
            normalize(&relationship.parent_database),
            normalize(&relationship.parent_segment),
            normalize(&relationship.child_database),
            normalize(&relationship.child_segment),
        );
        if !relationships.insert(key) {
            return Err(ImsMetadataProblem::DuplicateName);
        }
    }
    Ok(())
}

// Sources for PSBGEN and database/alternate PCB options:
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_psbgenpsbgenstmt.htm sha256:b373ee39a66431d4217f96809f90f4e4b648813dd0e484fbe8c87eccb3895d0c
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_psbgendlipcbstmt.htm sha256:0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_psbgenaltpcbstmt.htm sha256:89b117adcbc8ee2c9c114ce98258df1068548b3b88e0fac1e19892ffa8aba2a1
fn validate_psb(
    psb: &ImsPsbMetadata,
    databases: &BTreeMap<String, &ImsDatabaseMetadata>,
    limits: ImsMetadataLimits,
) -> Result<(), ImsMetadataProblem> {
    if psb.pcbs.len() > limits.max_pcbs_per_psb {
        return Err(ImsMetadataProblem::LimitExceeded);
    }
    let mut names = BTreeSet::new();
    let mut versions = BTreeMap::<String, Option<u32>>::new();
    let mut sensitive_count = 0usize;
    for pcb in &psb.pcbs {
        let name = match pcb {
            ImsPcbMetadata::Database(pcb) => &pcb.name,
            ImsPcbMetadata::AlternateTerminal(pcb) => &pcb.name,
        };
        validate_name(name)?;
        if !names.insert(normalize(name)) {
            return Err(ImsMetadataProblem::DuplicateName);
        }
        match pcb {
            ImsPcbMetadata::AlternateTerminal(pcb) => {
                if let Some(destination) = &pcb.destination {
                    validate_name(destination)?;
                } else if !pcb.modifiable {
                    return Err(ImsMetadataProblem::MissingReference);
                }
            }
            ImsPcbMetadata::Database(pcb) => {
                validate_procopt(&pcb.processing_options, false)?;
                let key = normalize(&pcb.database);
                let database = databases
                    .get(&key)
                    .copied()
                    .ok_or(ImsMetadataProblem::MissingReference)?;
                if pcb.database_version.is_some_and(|version| {
                    version > MAX_DATABASE_VERSION || version != database.version
                }) {
                    return Err(ImsMetadataProblem::IncompatibleReference);
                }
                match versions.insert(key, pcb.database_version) {
                    Some(previous) if previous != pcb.database_version => {
                        return Err(ImsMetadataProblem::IncompatibleReference);
                    }
                    _ => {}
                }
                if pcb.sensitive_segments.is_empty() {
                    return Err(ImsMetadataProblem::MissingReference);
                }
                sensitive_count = sensitive_count
                    .checked_add(pcb.sensitive_segments.len())
                    .ok_or(ImsMetadataProblem::LimitExceeded)?;
                if sensitive_count > limits.max_sensitive_segments_per_psb {
                    return Err(ImsMetadataProblem::LimitExceeded);
                }
                validate_sensitive_segments(pcb, database)?;
            }
        }
    }
    Ok(())
}

// Sources for SENSEG paths:
// SSEPH2_15.6.0/com.ibm.ims156.doc.sur/ims_psbgensensegstmt.htm sha256:baf8ed5e7ad87faf1ba02801da3479385ba5b4b5fc74474ca051d32d3014ebf4
fn validate_sensitive_segments(
    pcb: &ImsDatabasePcbMetadata,
    database: &ImsDatabaseMetadata,
) -> Result<(), ImsMetadataProblem> {
    let segments = database
        .segments
        .iter()
        .map(|segment| (normalize(&segment.name), segment))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for sensitive in &pcb.sensitive_segments {
        validate_name(&sensitive.name)?;
        if let Some(options) = &sensitive.processing_options {
            validate_procopt(options, true)?;
        }
        let segment = segments
            .get(&normalize(&sensitive.name))
            .copied()
            .ok_or(ImsMetadataProblem::MissingReference)?;
        if !seen.insert(normalize(&segment.name)) {
            return Err(ImsMetadataProblem::DuplicateName);
        }
        let actual_parent = segment.parent.as_deref().map(normalize);
        if actual_parent != sensitive.parent.as_deref().map(normalize)
            || actual_parent.is_some_and(|parent| !seen.contains(&parent))
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
    }
    Ok(())
}

fn validate_procopt(value: &str, key_allowed: bool) -> Result<(), ImsMetadataProblem> {
    if value.is_empty()
        || value.len() > 4
        || value != value.to_ascii_uppercase()
        || value
            .bytes()
            .any(|byte| !b"AGIRDPEHNLSTOK".contains(&byte) || (!key_allowed && byte == b'K'))
    {
        return Err(ImsMetadataProblem::InvalidOption);
    }
    if value.contains('O') && !matches!(value, "GO" | "GON" | "GONP" | "GOP" | "GOT" | "GOTP") {
        return Err(ImsMetadataProblem::InvalidOption);
    }
    if value.contains('A') && !matches!(value, "A" | "AP") {
        return Err(ImsMetadataProblem::InvalidOption);
    }
    if value.contains('L') && !matches!(value, "L" | "LS") {
        return Err(ImsMetadataProblem::InvalidOption);
    }
    let letters = value.bytes().collect::<BTreeSet<_>>();
    if letters.len() != value.len() || letters == BTreeSet::from(*b"GIRD") {
        return Err(ImsMetadataProblem::InvalidOption);
    }
    Ok(())
}

fn has_segment(database: &ImsDatabaseMetadata, name: &str) -> bool {
    database
        .segments
        .iter()
        .any(|segment| normalize(&segment.name) == normalize(name))
}

fn validate_name(name: &str) -> Result<(), ImsMetadataProblem> {
    if name.is_empty() || name.len() > 8 || !name.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Err(ImsMetadataProblem::InvalidName)
    } else {
        Ok(())
    }
}

fn normalize(value: &str) -> String {
    value.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, offset: usize, length: usize, sequence: bool) -> ImsFieldMetadata {
        ImsFieldMetadata {
            name: Some(name.into()),
            offset,
            length,
            sequence,
            unique: sequence,
        }
    }

    fn segment(name: &str, parent: Option<&str>, key: &str) -> ImsSegmentMetadata {
        ImsSegmentMetadata {
            name: name.into(),
            parent: parent.map(str::to_string),
            min_length: 16,
            max_length: 16,
            fields: vec![field(key, 0, 8, true)],
        }
    }

    fn catalog() -> ImsMetadataCatalog {
        ImsMetadataCatalog {
            schema_version: IMS_METADATA_SCHEMA_V1.into(),
            databases: vec![ImsDatabaseMetadata {
                name: "AUTHDB".into(),
                version: 7,
                organization: ImsDatabaseOrganization::Hidam,
                segments: vec![
                    segment("ROOT", None, "ROOTKEY"),
                    segment("CHILD", Some("ROOT"), "CHILDKEY"),
                ],
                secondary_indexes: vec![ImsSecondaryIndexMetadata {
                    name: "AUTHX".into(),
                    target_segment: "CHILD".into(),
                    source_segment: "CHILD".into(),
                    source_fields: vec!["CHILDKEY".into()],
                }],
                logical_relationships: vec![ImsLogicalRelationshipMetadata {
                    parent_database: "AUTHDB".into(),
                    parent_segment: "ROOT".into(),
                    child_database: "AUTHDB".into(),
                    child_segment: "CHILD".into(),
                    paired: true,
                }],
            }],
            psbs: vec![ImsPsbMetadata {
                name: "AUTHPSB".into(),
                database_level: ImsDbLevel::Current,
                pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                    name: "AUTHPCB".into(),
                    database: "AUTHDB".into(),
                    database_version: Some(7),
                    processing_options: "AP".into(),
                    sensitive_segments: vec![
                        ImsSensitiveSegmentMetadata {
                            name: "ROOT".into(),
                            parent: None,
                            processing_options: None,
                        },
                        ImsSensitiveSegmentMetadata {
                            name: "CHILD".into(),
                            parent: Some("ROOT".into()),
                            processing_options: Some("G".into()),
                        },
                    ],
                })],
            }],
        }
    }

    #[test]
    fn validates_cross_references_and_produces_a_stable_digest() {
        let catalog = catalog();
        assert_eq!(catalog.psbs[0].pcbs[0].host_kind(), ImsPcbKind::Database);
        let identity = validate_ims_metadata(&catalog, ImsMetadataLimits::default()).unwrap();
        assert_eq!(identity.databases, 1);
        assert_eq!(identity.segments, 2);
        assert_eq!(identity.fields, 2);
        assert_eq!(identity.secondary_indexes, 1);
        assert_eq!(identity.logical_relationships, 1);
        assert_eq!(identity.psbs, 1);
        assert_eq!(identity.pcbs, 1);
        assert_eq!(identity.digest.len(), 71);

        let encoded = serde_json::to_vec(&catalog).unwrap();
        let decoded: ImsMetadataCatalog = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            validate_ims_metadata(&decoded, ImsMetadataLimits::default()).unwrap(),
            identity
        );
    }

    #[test]
    fn rejects_cycles_missing_paths_and_inconsistent_database_versions() {
        let mut invalid = catalog();
        invalid.databases[0].segments[0].parent = Some("CHILD".into());
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::Cycle)
        );

        let mut invalid = catalog();
        let ImsPcbMetadata::Database(pcb) = &mut invalid.psbs[0].pcbs[0] else {
            unreachable!();
        };
        pcb.sensitive_segments.remove(0);
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::IncompatibleReference)
        );

        let mut invalid = catalog();
        let ImsPcbMetadata::Database(pcb) = &mut invalid.psbs[0].pcbs[0] else {
            unreachable!();
        };
        pcb.database_version = Some(6);
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::IncompatibleReference)
        );
    }

    #[test]
    fn rejects_invalid_options_offsets_and_secondary_index_sources() {
        let mut invalid = catalog();
        invalid.databases[0].segments[0].fields[0].offset = 12;
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::InvalidOffset)
        );

        let mut invalid = catalog();
        let ImsPcbMetadata::Database(pcb) = &mut invalid.psbs[0].pcbs[0] else {
            unreachable!();
        };
        pcb.processing_options = "GOK".into();
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::InvalidOption)
        );

        let mut invalid = catalog();
        invalid.databases[0].secondary_indexes[0].source_fields = vec!["MISSING".into()];
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::MissingReference)
        );
    }

    #[test]
    fn enforces_the_reviewed_statement_bounds() {
        let defaults = ImsMetadataLimits::default();
        assert_eq!(defaults.max_segments_per_database, 255);
        assert_eq!(defaults.max_dedb_segments, 127);
        assert_eq!(defaults.max_hierarchy_depth, 15);
        assert_eq!(defaults.max_fields_per_segment, 255);
        assert_eq!(defaults.max_fields_per_database, 20_000);
        assert_eq!(defaults.max_named_fields_and_indexes, 1_000);
        assert_eq!(defaults.max_secondary_indexes_per_segment, 32);
        assert_eq!(defaults.max_relationships_per_database, 255);
        assert_eq!(defaults.max_pcbs_per_psb, 2_500);
        assert_eq!(defaults.max_sensitive_segments_per_psb, 30_000);

        let mut invalid = catalog();
        invalid.databases[0].segments[0]
            .fields
            .push(field("OTHER", 8, 8, false));
        assert_eq!(
            validate_ims_metadata(
                &invalid,
                ImsMetadataLimits {
                    max_fields_per_segment: 1,
                    ..defaults
                }
            ),
            Err(ImsMetadataProblem::LimitExceeded)
        );

        let mut invalid = catalog();
        let second_index = invalid.databases[0].secondary_indexes[0].clone();
        invalid.databases[0].secondary_indexes.push(second_index);
        invalid.databases[0].secondary_indexes[1].name = "AUTHY".into();
        assert_eq!(
            validate_ims_metadata(
                &invalid,
                ImsMetadataLimits {
                    max_secondary_indexes_per_segment: 1,
                    ..defaults
                }
            ),
            Err(ImsMetadataProblem::LimitExceeded)
        );
    }
}
