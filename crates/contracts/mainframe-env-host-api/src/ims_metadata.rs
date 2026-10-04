use crate::ims_pcb::ImsPcbKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Exact accepted IMS metadata serialization schema and digest-domain revision.
pub const IMS_METADATA_SCHEMA_V1: &str = "mainframe-env.ims-metadata@1";
const DIGEST_DOMAIN: &[u8] = b"mainframe-env.ims-metadata@1\0";
const MAX_DATABASE_VERSION: u32 = i32::MAX as u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Finite catalog cardinality and byte bounds used before metadata identity is produced.
pub struct ImsMetadataLimits {
    /// Maximum database definitions; the catalog must also contain at least one.
    pub max_databases: usize,
    /// Maximum PSB definitions in the catalog.
    pub max_psbs: usize,
    /// Maximum segment definitions for ordinary organizations.
    pub max_segments_per_database: usize,
    /// Separate maximum DEDB segment count; GSAM/MSDB remain single-segment.
    pub max_dedb_segments: usize,
    /// Maximum ancestor depth including the segment itself.
    pub max_hierarchy_depth: usize,
    /// Maximum fields plus target secondary-index entries per segment.
    pub max_fields_per_segment: usize,
    /// Maximum aggregate fields plus secondary indexes per database.
    pub max_fields_per_database: usize,
    /// Maximum aggregate named fields plus indexes per database.
    pub max_named_fields_and_indexes: usize,
    /// Maximum secondary indexes targeting one segment.
    pub max_secondary_indexes_per_segment: usize,
    /// Maximum logical relationships declared by one database.
    pub max_relationships_per_database: usize,
    /// Maximum PCB declarations in one PSB.
    pub max_pcbs_per_psb: usize,
    /// Maximum aggregate sensitive-segment entries across one PSB's PCBs.
    pub max_sensitive_segments_per_psb: usize,
    /// Maximum segment byte length, checked before field-range validation.
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
/// Metadata organization identity used for hierarchy/index constraints; not an installed database engine claim.
pub enum ImsDatabaseOrganization {
    #[serde(rename = "DEDB")]
    /// DEDB organization with its separate segment bound and root key constraints.
    Dedb,
    #[serde(rename = "GSAM")]
    /// GSAM organization restricted to one segment by this validator.
    Gsam,
    #[serde(rename = "HDAM")]
    /// HDAM metadata organization.
    Hdam,
    #[serde(rename = "HIDAM")]
    /// HIDAM metadata requiring unique root sequence.
    Hidam,
    #[serde(rename = "HISAM")]
    /// HISAM metadata requiring unique root sequence.
    Hisam,
    #[serde(rename = "HSAM")]
    /// HSAM metadata organization.
    Hsam,
    #[serde(rename = "INDEX")]
    /// INDEX metadata requiring unique root sequence.
    Index,
    #[serde(rename = "MSDB")]
    /// MSDB metadata restricted to one segment and unique root sequence.
    Msdb,
    #[serde(rename = "PHDAM")]
    /// PHDAM metadata organization.
    Phdam,
    #[serde(rename = "PHIDAM")]
    /// PHIDAM metadata requiring unique root sequence.
    Phidam,
    #[serde(rename = "PSINDEX")]
    /// PSINDEX metadata organization.
    Psindex,
    #[serde(rename = "SHISAM")]
    /// SHISAM metadata requiring unique root sequence.
    Shisam,
    #[serde(rename = "SHSAM")]
    /// SHSAM metadata organization.
    Shsam,
}

impl ImsDatabaseOrganization {
    fn segment_limit(self, limits: ImsMetadataLimits) -> usize {
        match self {
            Self::Dedb => limits.max_dedb_segments,
            Self::Gsam | Self::Msdb | Self::Shsam | Self::Shisam => 1,
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
/// Segment byte slice and sequence-key metadata; checked arithmetic bounds each field by segment length.
pub struct ImsFieldMetadata {
    /// Optional 1-8 byte ASCII alphanumeric field name; sequence fields must be named.
    pub name: Option<String>,
    /// Zero-based byte offset into the segment, checked with length for overflow.
    pub offset: usize,
    /// Positive field byte count whose end must fit max_length.
    pub length: usize,
    /// Mark the sole named sequence field; its end must fit min_length.
    pub sequence: bool,
    /// Declare sequence uniqueness; organizations requiring a unique root key check this flag.
    pub unique: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Named segment hierarchy and byte lengths; one root and acyclic bounded ancestry are required.
pub struct ImsSegmentMetadata {
    /// Unique normalized database-local segment name.
    pub name: String,
    /// Optional database-local parent name; the hierarchy must contain exactly one root.
    pub parent: Option<String>,
    /// Positive minimum segment bytes, no greater than max_length.
    pub min_length: usize,
    /// Maximum segment bytes, bounded by max_segment_bytes.
    pub max_length: usize,
    /// Ordered declared field slices; field names must be unique within the segment.
    pub fields: Vec<ImsFieldMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Named target/source segment relationship and ordered source fields, validated against catalog references.
pub struct ImsSecondaryIndexMetadata {
    /// Unique normalized index name within the database.
    pub name: String,
    /// Referenced indexed segment in this database.
    pub target_segment: String,
    /// Referenced segment supplying the indexed fields.
    pub source_segment: String,
    /// Nonempty ordered unique named fields; aggregate byte length must not exceed 240.
    pub source_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Cross-database logical relationship whose referenced databases/segments must exist and support the relationship.
pub struct ImsLogicalRelationshipMetadata {
    /// Referenced logical parent database name.
    pub parent_database: String,
    /// Existing segment in parent_database.
    pub parent_segment: String,
    /// Referenced logical child database name.
    pub child_database: String,
    /// Existing segment in child_database.
    pub child_segment: String,
    /// Retained paired-relationship declaration; validation does not allocate relationship state.
    pub paired: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Versioned database definition consumed by packages and providers after shared validation.
pub struct ImsDatabaseMetadata {
    /// Explicit GSAM application format; absence retains historical fixed-only admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gsam_format: Option<crate::ImsGsamFormat>,
    /// Database resource identity unique after normalization.
    pub name: String,
    /// Definition version bounded by signed 32-bit range.
    pub version: u32,
    /// Declared organization used for validation and route applicability.
    pub organization: ImsDatabaseOrganization,
    /// Nonempty hierarchy with exactly one root.
    pub segments: Vec<ImsSegmentMetadata>,
    /// Declared indexes whose references and search lengths must validate.
    pub secondary_indexes: Vec<ImsSecondaryIndexMetadata>,
    /// Declared logical parent/child links to catalog resources.
    pub logical_relationships: Vec<ImsLogicalRelationshipMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// PCB-sensitive path entry; parents must match metadata and precede children in the sensitive sequence.
pub struct ImsSensitiveSegmentMetadata {
    /// Existing segment made sensitive through this PCB.
    pub name: String,
    /// Actual metadata parent, which must precede this entry in the sensitive path.
    pub parent: Option<String>,
    /// Optional already constrained PROCOPT override; key option is permitted here.
    pub processing_options: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Named database PCB binding with ordered sensitivity and optional version selection.
pub struct ImsDatabasePcbMetadata {
    /// PCB identity unique within the PSB.
    pub name: String,
    /// Referenced database resource identity.
    pub database: String,
    /// Optional exact version; present values must match the referenced definition.
    pub database_version: Option<u32>,
    /// XDFLD identity selecting the secondary processing sequence. Historical
    /// descriptors omit this field and retain the primary processing sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_index: Option<String>,
    /// Validated uppercase PCB PROCOPT text, not an unchecked permission flag.
    pub processing_options: String,
    /// Nonempty ordered sensitivity path; parents must precede children.
    pub sensitive_segments: Vec<ImsSensitiveSegmentMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Alternate terminal PCB routing metadata; a nonmodifiable destination must be present.
pub struct ImsTerminalPcbMetadata {
    /// Unique normalized alternate PCB name within the PSB.
    pub name: String,
    /// Optional bounded terminal destination; required unless modifiable.
    pub destination: Option<String>,
    /// Allow deferred destination selection when no fixed destination is supplied.
    pub modifiable: bool,
    /// Retained express-routing declaration, not a delivery receipt.
    pub express: bool,
    /// Retained same-terminal routing declaration.
    pub same_terminal: bool,
    /// Retained response-mode declaration, not proof of an open conversation.
    pub response_mode: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
/// Metadata-supported PCB forms; host-kind mapping does not allocate a live PCB.
pub enum ImsPcbMetadata {
    /// Database PCB with closed database/sensitive-segment references.
    Database(ImsDatabasePcbMetadata),
    /// Alternate terminal PCB with explicit routing options.
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
/// PSB database-level selection retained in metadata identity, not an implicit runtime version resolution.
pub enum ImsDbLevel {
    /// Retain CURRENT database-level selection.
    Current,
    /// Retain BASE database-level selection.
    Base,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Named PSB and ordered PCB declarations; duplicate names and inconsistent database versions are rejected.
pub struct ImsPsbMetadata {
    /// Unique normalized PSB name, disjoint from database names.
    pub name: String,
    /// Explicit retained database-level selection.
    pub database_level: ImsDbLevel,
    /// Ordered bounded PCB declarations with unique normalized names.
    pub pcbs: Vec<ImsPcbMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Schema-qualified owned database/PSB graph. Deserialization alone does not validate its closure.
pub struct ImsMetadataCatalog {
    /// Must exactly equal IMS_METADATA_SCHEMA_V1.
    pub schema_version: String,
    /// Nonempty bounded database graph validated before digest production.
    pub databases: Vec<ImsDatabaseMetadata>,
    /// Bounded PSB graph whose database/segment references must close.
    pub psbs: Vec<ImsPsbMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Validated catalog counts and domain-separated serialization digest; no installed-provider or execution claim.
pub struct ImsMetadataIdentity {
    /// Validated database count.
    pub databases: usize,
    /// Validated aggregate segment count.
    pub segments: usize,
    /// Validated aggregate declared field count.
    pub fields: usize,
    /// Validated aggregate secondary-index count.
    pub secondary_indexes: usize,
    /// Validated aggregate relationship count.
    pub logical_relationships: usize,
    /// Validated PSB count.
    pub psbs: usize,
    /// Validated aggregate PCB count.
    pub pcbs: usize,
    /// sha256: digest of domain-separated serde JSON; order and retained spelling remain identity-bearing.
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Metadata graph/shape failure before identity production; no catalog mutation is performed by validation.
pub enum ImsMetadataProblem {
    /// Schema identity is not the supported metadata revision.
    UnsupportedSchema,
    /// Cardinality, version, depth or byte bound was exceeded.
    LimitExceeded,
    /// Name is not 1-8 bytes of ASCII alphanumeric text.
    InvalidName,
    /// A normalized name/reference occurs more than once where uniqueness is required.
    DuplicateName,
    /// A required database, segment, field, parent or destination is absent.
    MissingReference,
    /// The segment ancestry revisits a segment.
    Cycle,
    /// PROCOPT has invalid casing, length, letters or combinations.
    InvalidOption,
    /// Field slice/sequence range is empty, overflowing or outside the admitted segment.
    InvalidOffset,
    /// Organization, version or path relationship is not admitted.
    IncompatibleReference,
    /// Serialization for metadata identity failed.
    Encoding,
}

/// Validate schema, finite counts, names, hierarchy, options and graph references before hashing the original serialization. No normalization/reordering or installation occurs.
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
    if let Some(format) = &database.gsam_format {
        let record = database
            .segments
            .first()
            .ok_or(ImsMetadataProblem::MissingReference)?;
        if database.organization != ImsDatabaseOrganization::Gsam
            || database.segments.len() != 1
            || !record.fields.is_empty()
            || format
                .validate(record.min_length, record.max_length)
                .is_err()
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
    }
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
        if matches!(
            database.organization,
            ImsDatabaseOrganization::Hsam
                | ImsDatabaseOrganization::Shsam
                | ImsDatabaseOrganization::Shisam
        ) && segment.min_length != segment.max_length
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
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
            || index.source_fields.len() > limits.max_fields_per_segment.min(5)
        {
            return Err(ImsMetadataProblem::LimitExceeded);
        }
        let source_fields = source
            .fields
            .iter()
            .filter_map(|field| field.name.as_ref().map(|name| (normalize(name), field)))
            .collect::<BTreeMap<_, _>>();
        let mut ancestor = Some(*source);
        while ancestor.is_some_and(|segment| normalize(&segment.name) != normalize(&target.name)) {
            ancestor = ancestor
                .and_then(|segment| segment.parent.as_deref())
                .and_then(|parent| segments.get(&normalize(parent)).copied());
        }
        if ancestor.is_none()
            || target.fields.iter().any(|field| {
                field
                    .name
                    .as_deref()
                    .is_some_and(|name| normalize(name) == normalize(&index.name))
            })
        {
            return Err(ImsMetadataProblem::IncompatibleReference);
        }
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
                if let Some(name) = &pcb.secondary_index {
                    validate_name(name)?;
                    let index = database
                        .secondary_indexes
                        .iter()
                        .find(|index| normalize(&index.name) == normalize(name))
                        .ok_or(ImsMetadataProblem::MissingReference)?;
                    if pcb.processing_options.contains('L')
                        || normalize(&pcb.sensitive_segments[0].name)
                            != normalize(&index.target_segment)
                    {
                        return Err(ImsMetadataProblem::IncompatibleReference);
                    }
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

    fn seq_layout_host_catalog(
        organization: ImsDatabaseOrganization,
        two: bool,
    ) -> ImsMetadataCatalog {
        let mut value = shisam_fixed_catalog(organization);
        if two {
            let mut child = value.databases[0].segments[0].clone();
            child.name = "SEQCHILD".into();
            child.parent = Some(value.databases[0].segments[0].name.clone());
            value.databases[0].segments.push(child);
            let ImsPcbMetadata::Database(pcb) = &mut value.psbs[0].pcbs[0] else {
                unreachable!()
            };
            pcb.sensitive_segments.push(ImsSensitiveSegmentMetadata {
                name: "SEQCHILD".into(),
                parent: Some(pcb.sensitive_segments[0].name.clone()),
                processing_options: None,
            });
        }
        value
    }

    fn seq_layout_host_observe(
        value: &ImsMetadataCatalog,
    ) -> Result<ImsMetadataIdentity, ImsMetadataProblem> {
        let actual = validate_ims_metadata(value, ImsMetadataLimits::default());
        eprintln!(
            "SEQ_LAYOUT_HOST_METADATA={}; ACTUAL={actual:?}",
            serde_json::to_string(value).unwrap()
        );
        actual
    }

    #[test]
    fn seq_layout_host_shisam_multiple_types() {
        assert_eq!(
            seq_layout_host_observe(&seq_layout_host_catalog(
                ImsDatabaseOrganization::Shisam,
                true
            )),
            Err(ImsMetadataProblem::LimitExceeded)
        );
    }
    #[test]
    fn seq_layout_host_shsam_multiple_types() {
        assert_eq!(
            seq_layout_host_observe(&seq_layout_host_catalog(
                ImsDatabaseOrganization::Shsam,
                true
            )),
            Err(ImsMetadataProblem::LimitExceeded)
        );
    }
    fn seq_layout_host_variable(organization: ImsDatabaseOrganization, index: usize) {
        let mut value =
            seq_layout_host_catalog(organization, organization == ImsDatabaseOrganization::Hsam);
        validate_ims_metadata(&value, ImsMetadataLimits::default()).unwrap();
        value.databases[0].segments[index].max_length = 4;
        assert_eq!(
            seq_layout_host_observe(&value),
            Err(ImsMetadataProblem::IncompatibleReference)
        );
    }
    #[test]
    fn seq_layout_host_hsam_variable_root() {
        seq_layout_host_variable(ImsDatabaseOrganization::Hsam, 0);
    }
    #[test]
    fn seq_layout_host_hsam_variable_dependent() {
        seq_layout_host_variable(ImsDatabaseOrganization::Hsam, 1);
    }
    #[test]
    fn seq_layout_host_shsam_variable() {
        seq_layout_host_variable(ImsDatabaseOrganization::Shsam, 0);
    }
    #[test]
    fn seq_layout_host_shisam_variable_control() {
        seq_layout_host_variable(ImsDatabaseOrganization::Shisam, 0);
    }
    #[test]
    fn seq_layout_host_fixed_and_ranged_controls() {
        for organization in [
            ImsDatabaseOrganization::Hsam,
            ImsDatabaseOrganization::Shsam,
            ImsDatabaseOrganization::Shisam,
            ImsDatabaseOrganization::Hisam,
            ImsDatabaseOrganization::Hidam,
        ] {
            let mut value = seq_layout_host_catalog(
                organization,
                organization == ImsDatabaseOrganization::Hsam,
            );
            if matches!(
                organization,
                ImsDatabaseOrganization::Hisam | ImsDatabaseOrganization::Hidam
            ) {
                value.databases[0].segments[0].max_length = 4;
            }
            assert!(seq_layout_host_observe(&value).is_ok());
        }
    }

    fn shisam_fixed_catalog(organization: ImsDatabaseOrganization) -> ImsMetadataCatalog {
        let mut value = catalog();
        let db = &mut value.databases[0];
        db.organization = organization;
        db.segments.truncate(1);
        db.segments[0].min_length = 3;
        db.segments[0].max_length = 3;
        db.segments[0].fields[0].length = 2;
        db.secondary_indexes.clear();
        db.logical_relationships.clear();
        let ImsPcbMetadata::Database(pcb) = &mut value.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.sensitive_segments.truncate(1);
        value
    }

    #[test]
    fn shisam_fixed_layout_host_rejects_variable_root() {
        let good = shisam_fixed_catalog(ImsDatabaseOrganization::Shisam);
        validate_ims_metadata(&good, ImsMetadataLimits::default()).unwrap();
        let mut invalid = good;
        invalid.databases[0].segments[0].max_length = 4;
        let actual = validate_ims_metadata(&invalid, ImsMetadataLimits::default());
        eprintln!("SHISAM_HOST_VARIABLE_ACTUAL={actual:?}");
        assert_eq!(actual, Err(ImsMetadataProblem::IncompatibleReference));
    }

    #[test]
    fn shisam_fixed_layout_host_fixed_and_other_organization_controls() {
        for organization in [
            ImsDatabaseOrganization::Shisam,
            ImsDatabaseOrganization::Hisam,
            ImsDatabaseOrganization::Shsam,
            ImsDatabaseOrganization::Hidam,
        ] {
            let good = shisam_fixed_catalog(organization);
            let bytes = serde_json::to_vec(&good).unwrap();
            let identity = validate_ims_metadata(&good, ImsMetadataLimits::default()).unwrap();
            let decoded = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
            assert_eq!(
                validate_ims_metadata(&decoded, ImsMetadataLimits::default()),
                Ok(identity)
            );
        }
        for organization in [
            ImsDatabaseOrganization::Hisam,
            ImsDatabaseOrganization::Hidam,
        ] {
            let mut ranged = shisam_fixed_catalog(organization);
            ranged.databases[0].segments[0].max_length = 4;
            assert!(validate_ims_metadata(&ranged, ImsMetadataLimits::default()).is_ok());
        }
    }

    #[test]
    fn gsam_absent_format_keeps_historical_metadata_bytes_and_explicit_format_binds_identity() {
        let literal = br#"{"name":"GENDB","version":1,"organization":"GSAM","segments":[{"name":"RECORD","parent":null,"min_length":16,"max_length":16,"fields":[]}],"secondary_indexes":[],"logical_relationships":[]}"#;
        let mut db: ImsDatabaseMetadata = serde_json::from_slice(literal).unwrap();
        assert_eq!(db.gsam_format, None);
        assert_eq!(serde_json::to_vec(&db).unwrap(), literal);
        let catalog = |db| ImsMetadataCatalog {
            schema_version: IMS_METADATA_SCHEMA_V1.into(),
            databases: vec![db],
            psbs: vec![],
        };
        let old =
            validate_ims_metadata(&catalog(db.clone()), ImsMetadataLimits::default()).unwrap();
        db.segments[0].min_length = 12;
        db.gsam_format = Some(crate::ImsGsamFormat {
            version: 1,
            record_format: crate::ImsGsamRecordFormat::U,
            access_method: crate::ImsGsamAccessMethod::Bsam,
            block_size: 16,
            control: crate::ImsGsamControl::None,
        });
        let identity =
            validate_ims_metadata(&catalog(db.clone()), ImsMetadataLimits::default()).unwrap();
        assert_ne!(old.digest, identity.digest);
        db.gsam_format.as_mut().unwrap().control = crate::ImsGsamControl::Asa;
        assert_ne!(
            identity.digest,
            validate_ims_metadata(&catalog(db.clone()), ImsMetadataLimits::default())
                .unwrap()
                .digest
        );
        db.organization = ImsDatabaseOrganization::Hsam;
        assert!(validate_ims_metadata(&catalog(db), ImsMetadataLimits::default()).is_err());
    }

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
                gsam_format: None,
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
                    secondary_index: None,
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
    fn secondary_selector_absence_preserves_historical_metadata_bytes_and_identity() {
        let catalog = catalog();
        let old_pcb = r#"{"kind":"database","name":"AUTHPCB","database":"AUTHDB","database_version":7,"processing_options":"AP","sensitive_segments":[{"name":"ROOT","parent":null,"processing_options":null},{"name":"CHILD","parent":"ROOT","processing_options":"G"}]}"#;
        assert_eq!(
            serde_json::to_string(&catalog.psbs[0].pcbs[0]).unwrap(),
            old_pcb
        );
        let encoded = serde_json::to_vec(&catalog).unwrap();
        let identity = validate_ims_metadata(&catalog, ImsMetadataLimits::default()).unwrap();
        let mut value = serde_json::to_value(&catalog).unwrap();
        value["psbs"][0]["pcbs"][0]["secondary_index"] = serde_json::Value::Null;
        let decoded = serde_json::from_value(value).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        assert_eq!(
            validate_ims_metadata(&decoded, ImsMetadataLimits::default()).unwrap(),
            identity
        );
    }

    #[test]
    fn secondary_selector_references_and_source_target_ancestry_are_validated() {
        let mut metadata = catalog();
        metadata.databases[0].secondary_indexes[0].target_segment = "ROOT".into();
        let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.secondary_index = Some("AUTHX".into());
        assert!(validate_ims_metadata(&metadata, ImsMetadataLimits::default()).is_ok());
        let mut invalid = metadata.clone();
        let ImsPcbMetadata::Database(pcb) = &mut invalid.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.secondary_index = Some("MISSING".into());
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::MissingReference)
        );
        let mut invalid = metadata;
        invalid.databases[0].secondary_indexes[0].source_segment = "ROOT".into();
        invalid.databases[0].secondary_indexes[0].target_segment = "CHILD".into();
        assert_eq!(
            validate_ims_metadata(&invalid, ImsMetadataLimits::default()),
            Err(ImsMetadataProblem::IncompatibleReference)
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
