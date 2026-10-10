//! Complete initial raw seed comparison for one exact pinned CardDemo fixture.
//! Thirteen first-party EBCDIC source files include the byte-identical ACCDATA /
//! ACCTDATA alias, yielding twelve unique installed seeds. Expected records,
//! keys and digests come from exact source pins and declared first-party layouts;
//! no runtime encoder, observed row, or observed ordering supplies expectations.
//! Three derived .AIX.PATH views use the existing fixture's direct AlternateIndex
//! representation. This checks those installed views, with no separate IBM PATH
//! parity or logical CICS alias catalog credit. Only the named dataset-count /
//! digest observation is emitted, after the owning wrapper's checked cleanup.
use super::*;
#[cfg(all(test, target_os = "linux"))]
#[path = "seed_state_lifecycle.rs"]
mod lifecycle;
use mainframe_env_host_api::{CatalogEntryKind, CatalogListEntry};

struct SeedSpec {
    source: &'static str,
    target: &'static str,
    lrecl: u32,
    key: Option<(u32, u32)>,
    count: usize,
    sha256: &'static str,
}

const SEEDS: &[SeedSpec] = &[
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCDATA.PS",
        target: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        lrecl: 300,
        key: Some((0, 11)),
        count: 50,
        sha256: "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS",
        target: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        lrecl: 300,
        key: Some((0, 11)),
        count: 50,
        sha256: "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDDATA.PS",
        target: "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
        lrecl: 150,
        key: Some((0, 16)),
        count: 50,
        sha256: "b5d968b6865bd48fb45928ef6cacce9b6aacdaec7b51cfcc9819b9fafcaed3e5",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        target: "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
        lrecl: 50,
        key: Some((0, 16)),
        count: 50,
        sha256: "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS",
        target: "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
        lrecl: 500,
        key: Some((0, 9)),
        count: 50,
        sha256: "0435915cd35ea3077bad9d391fea1520bc7f30ec06d6e29fbe68290f3d6f27f3",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS",
        target: "AWS.M2.CARDDEMO.DALYTRAN.PS",
        lrecl: 350,
        key: None,
        count: 300,
        sha256: "479b1f99cb7adcd9b79e94708f04c8bde0a010ba87f2ed69ba8af1effe57d076",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
        target: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        lrecl: 350,
        key: Some((0, 16)),
        count: 1,
        sha256: "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.DISCGRP.PS",
        target: "AWS.M2.CARDDEMO.DISCGRP.VSAM.KSDS",
        lrecl: 50,
        key: Some((0, 16)),
        count: 51,
        sha256: "722df78926c1ac01ed8e1138d2bba72593ac21046c994685c64fc7c87a5851d3",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.EXPORT.DATA.PS",
        target: "AWS.M2.CARDDEMO.EXPORT.DATA",
        lrecl: 500,
        key: Some((28, 4)),
        count: 500,
        sha256: "e1d6cfbe62a77b5c7e3bd78d920813a76ca7bf18280a17f988221ddeda19b3ba",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.TCATBALF.PS",
        target: "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
        lrecl: 50,
        key: Some((0, 17)),
        count: 50,
        sha256: "725dbe47d34d67053090efde8819af686ce424ed9dc0c8fd3f33cd1bf786b68a",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.TRANCATG.PS",
        target: "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
        lrecl: 60,
        key: Some((0, 6)),
        count: 18,
        sha256: "3c171c53c0b90ec7b0c267305cf5ba051825dfff78021425c601141f2f3c99e2",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.TRANTYPE.PS",
        target: "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
        lrecl: 60,
        key: Some((0, 2)),
        count: 7,
        sha256: "97735eb6979387143a98947a0681aed745a63c6a1f7dfbea1667dd4a6d567cf2",
    },
    SeedSpec {
        source: "app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS",
        target: "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
        lrecl: 80,
        key: Some((0, 8)),
        count: 10,
        sha256: "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8",
    },
];

struct ViewSpec {
    target: &'static str,
    base: &'static str,
    key: (u32, u32),
    sha256: &'static str,
}

const VIEWS: &[ViewSpec] = &[
    ViewSpec {
        target: "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
        base: "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
        key: (16, 11),
        sha256: "04e8b91d6ca0ed704f30835bfff31fbe6f58343b5d6bc235724257044448844c",
    },
    ViewSpec {
        target: "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
        base: "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
        key: (25, 11),
        sha256: "9dfe25ea510f46d1018dd68ca40c33651b729eccc1e96eb19e1d95854023ade9",
    },
    ViewSpec {
        target: "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
        base: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        key: (304, 26),
        sha256: "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    },
];

// Layout/alias evidence is source-pinned too. Source docs define primary keys,
// fixed widths and index keys/relationships; CCSID37 and direct AIX registration
// are the existing admitted local fixture contract, not imported IBM semantics.
const LAYOUT_SOURCES: &[(&str, &str)] = &[
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/jcl/ACCTFILE.jcl",
        "febba716137a3f59302ef1136c60dccce4d07d32e1d118b15d48f99266dea9e7",
    ),
    (
        "app/jcl/CARDFILE.jcl",
        "0240faee5aec80d976ce0b15a85f101fa0f901b09b72d64545daa09858ddcc0c",
    ),
    (
        "app/jcl/XREFFILE.jcl",
        "ae0edd90056cccacd26412fad2cf3d95656c2a2810b66d8c316cbaac9e81248f",
    ),
    (
        "app/jcl/CUSTFILE.jcl",
        "3f222d940612015b0c3b532253e34e8e60f02a670dd780a10b9db1f90e8deea7",
    ),
    (
        "app/jcl/TRANFILE.jcl",
        "360080ae5bf5153c3ecbf82385b070a6c7a2cdbf18870685951bf9894516730a",
    ),
    (
        "app/jcl/DISCGRP.jcl",
        "ef70df333b0acbf77f977df607cbaa559d18d8d0213eac93e478f2f0d4b447bb",
    ),
    (
        "app/jcl/CBEXPORT.jcl",
        "2af5ab38310c86231f9374618b5ee488b494ff79d0158ab460659616002ca54f",
    ),
    (
        "app/jcl/TCATBALF.jcl",
        "fa89dbfa8c34cb43c20298f6851c3aa1635556d3469ba7eac5859abe2305556e",
    ),
    (
        "app/jcl/TRANCATG.jcl",
        "3030bce92bf7f086b826b39bd711f6a37eec730a5e8fb0c503da612f3b01cade",
    ),
    (
        "app/jcl/TRANTYPE.jcl",
        "7a10aa43bd6737007a19fbfa5592934ef553816ffeef8c208db34d4475518051",
    ),
    (
        "app/jcl/DUSRSECJ.jcl",
        "02b0b78847e722b133b23d8f6152471214ea65d4f4c08692a5f42425987ca3c4",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT02Y.cpy",
        "9f1c62ef31b9d541712f1ed28f79e011ea7338611f5e0f30f2e1cbf624181223",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CVCUS01Y.cpy",
        "944fd9a8eb10a683d0db97b9a87f2151982f649970287eb74358826056af25f8",
    ),
    (
        "app/cpy/CVTRA05Y.cpy",
        "d7bde0e78ff608497087b9909c889ed39e347269f964bda767b92b547fbb5fec",
    ),
    (
        "app/cpy/CVEXPORT.cpy",
        "49d07843c9bd6f1a8f8eeb65f1085dabbced582f825773a7772970d1146409f4",
    ),
    (
        "app/cpy/CSUSR01Y.cpy",
        "37c34094dc4124bc6255e0b06a0387442f4f0dd9d51dd6623b559d370aa0d2cb",
    ),
    (
        "samples/m2/mf/CardDemo_runtime.zip",
        "683ef5bb461553ae9cee17c8c115f63bcb241ddeb832a2f1c62e8f96e9dbe2e3",
    ),
];

#[derive(Clone, Eq, PartialEq)]
struct ExpectedRows {
    attributes: DatasetAttributes,
    records: Vec<Vec<u8>>,
    identities: Vec<Vec<u8>>,
    sha256: String,
    kind: CatalogEntryKind,
    related: Option<String>,
}

#[derive(Clone, Eq, PartialEq)]
struct ActualRows {
    attributes: DatasetAttributes,
    attributes_version: u64,
    records: Vec<Vec<u8>>,
    identities: Vec<Vec<u8>>,
    version: u64,
    catalog: CatalogListEntry,
}

// No external caller can construct or alter its complete evidence maps.
pub(super) struct SeedProof {
    expected: BTreeMap<String, ExpectedRows>,
    actual: BTreeMap<String, ActualRows>,
}

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.seed_records_drift", detail)
}

fn raw_digest(records: &[Vec<u8>]) -> String {
    let mut digest = Sha256::new();
    for record in records {
        digest.update(record);
    }
    format!("{:x}", digest.finalize())
}

fn source_rows(spec: &SeedSpec, bytes: Vec<u8>) -> Result<ExpectedRows, CorpusProblem> {
    if bytes.len() != spec.count * spec.lrecl as usize
        || format!("{:x}", Sha256::digest(&bytes)) != spec.sha256
    {
        return Err(drift(format!(
            "{} fixed source count/digest drift",
            spec.source
        )));
    }
    let records = bytes
        .chunks_exact(spec.lrecl as usize)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let identities = if let Some((offset, length)) = spec.key {
        let start = offset as usize;
        let end = start + length as usize;
        if length == 0 || end > spec.lrecl as usize {
            return Err(drift("declared source key exceeds its fixed row"));
        }
        let keys = records
            .iter()
            .map(|row| row[start..end].to_vec())
            .collect::<Vec<_>>();
        // Every pinned source is already canonical. Never sort actual rows or
        // accept duplicate keys; source-order drift refuses before application launch.
        if keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(drift(format!(
                "{} noncanonical or duplicate source keys",
                spec.source
            )));
        }
        keys
    } else {
        // Local public PS read identity contract: zero-based BE-u64 positions.
        // This identity is not an IBM sequential-file record address assertion.
        (0..records.len())
            .map(|position| (position as u64).to_be_bytes().to_vec())
            .collect()
    };
    Ok(ExpectedRows {
        attributes: DatasetAttributes {
            organization: if spec.key.is_some() {
                DatasetOrganization::KeySequenced
            } else {
                DatasetOrganization::Sequential
            },
            record_format: RecordFormat::Fixed,
            logical_record_length: spec.lrecl,
            key_offset: spec.key.map(|key| key.0),
            key_length: spec.key.map(|key| key.1),
            ccsid: Some(37),
        },
        records,
        identities,
        sha256: spec.sha256.into(),
        kind: CatalogEntryKind::Dataset,
        related: None,
    })
}

fn merge_source(
    expected: &mut BTreeMap<String, ExpectedRows>,
    target: &str,
    rows: ExpectedRows,
) -> Result<(), CorpusProblem> {
    if let Some(original) = expected.get(target) {
        if original != &rows {
            return Err(drift(format!(
                "{target} source alias bytes/layout/keys disagree"
            )));
        }
    } else {
        expected.insert(target.into(), rows);
    }
    Ok(())
}

fn expected_rows(corpus: &Path) -> Result<BTreeMap<String, ExpectedRows>, CorpusProblem> {
    for (source, expected) in LAYOUT_SOURCES {
        let bytes = read_corpus_file(corpus, &corpus.join(source))?;
        if format!("{:x}", Sha256::digest(bytes)) != *expected {
            return Err(drift(format!("{source} pinned layout/alias source drift")));
        }
    }
    let mut expected = BTreeMap::new();
    for spec in SEEDS {
        let bytes = read_corpus_file(corpus, &corpus.join(spec.source))?;
        merge_source(&mut expected, spec.target, source_rows(spec, bytes)?)?;
    }
    if SEEDS.len() != 13 || expected.len() != 12 {
        return Err(drift(
            "thirteen pinned inputs must produce twelve unique seed targets",
        ));
    }
    for spec in VIEWS {
        let base = expected
            .get(spec.base)
            .ok_or_else(|| drift("declared index base missing"))?;
        let start = spec.key.0 as usize;
        let end = start + spec.key.1 as usize;
        if end > base.attributes.logical_record_length as usize {
            return Err(drift("declared alternate key exceeds its pinned row"));
        }
        let primary_start =
            base.attributes
                .key_offset
                .ok_or_else(|| drift("index base key offset missing"))? as usize;
        let primary_end = primary_start
            + base
                .attributes
                .key_length
                .ok_or_else(|| drift("index base key length missing"))? as usize;
        let mut records = base.records.clone();
        // Sort only independently pinned expected records by declared alternate
        // key and stable primary identity; the frozen digest binds this order.
        records.sort_by(|left, right| {
            left[start..end].cmp(&right[start..end]).then_with(|| {
                left[primary_start..primary_end].cmp(&right[primary_start..primary_end])
            })
        });
        if raw_digest(&records) != spec.sha256 {
            return Err(drift("independent alternate-view order/digest drift"));
        }
        let identities = records
            .iter()
            .map(|row| row[primary_start..primary_end].to_vec())
            .collect();
        let mut attributes = base.attributes.clone();
        attributes.key_offset = Some(spec.key.0);
        attributes.key_length = Some(spec.key.1);
        if expected
            .insert(
                spec.target.into(),
                ExpectedRows {
                    attributes,
                    records,
                    identities,
                    sha256: spec.sha256.into(),
                    kind: CatalogEntryKind::AlternateIndex,
                    related: Some(spec.base.into()),
                },
            )
            .is_some()
        {
            return Err(drift("declared view aliases an unrelated installed target"));
        }
    }
    Ok(expected)
}

fn rows_match(name: &str, expected: &ExpectedRows, actual: &ActualRows) -> bool {
    actual.attributes == expected.attributes
        && actual.records == expected.records
        && actual.identities == expected.identities
        && raw_digest(&actual.records) == expected.sha256
        && actual.version > 0
        && actual.attributes_version == actual.version
        && actual.catalog.version == actual.version
        && actual.catalog.name.as_str() == name
        && actual.catalog.kind == expected.kind
        && actual.catalog.related.as_ref().map(DatasetName::as_str) == expected.related.as_deref()
}

fn complete_match(
    expected: &BTreeMap<String, ExpectedRows>,
    actual: &BTreeMap<String, ActualRows>,
) -> bool {
    expected.len() == 15
        && actual.keys().eq(expected.keys())
        && expected
            .iter()
            .all(|(name, rows)| rows_match(name, rows, &actual[name]))
}

pub(super) fn capture(server: &ProductServer, corpus: &Path) -> Result<SeedProof, CorpusProblem> {
    capture_expected(server, expected_rows(corpus)?)
}

fn capture_expected(
    server: &ProductServer,
    expected: BTreeMap<String, ExpectedRows>,
) -> Result<SeedProof, CorpusProblem> {
    let catalog = match server
        .dataset_service()
        .invoke(DatasetRequest::ListCatalog {
            pattern: "AWS.M2.CARDDEMO.*".into(),
            start: None,
            max_items: 16,
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::CatalogEntries {
            entries,
            more: false,
        } => entries,
        _ => {
            return Err(drift(
                "initial installed CardDemo dataset inventory is incomplete",
            ));
        }
    };
    let mut catalog_by_name = BTreeMap::new();
    for entry in catalog {
        if catalog_by_name
            .insert(entry.name.as_str().to_string(), entry)
            .is_some()
        {
            return Err(drift(
                "initial installed dataset inventory duplicates a name",
            ));
        }
    }
    if !catalog_by_name.keys().eq(expected.keys()) {
        return Err(drift(
            "initial installed seed/view target names missing or extra",
        ));
    }
    let mut actual = BTreeMap::new();
    for (name, rows) in &expected {
        let dataset =
            DatasetName::new(name, 128).map_err(|_| drift("frozen dataset name invalid"))?;
        let (attributes, attributes_version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: dataset.clone(),
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Attributes {
                attributes,
                version,
            } => (attributes, version),
            _ => {
                return Err(drift(
                    "initial installed seed attributes returned wrong result",
                ));
            }
        };
        // One beyond the independent exact count detects an extra row without
        // increasing any provider/global limit. Maximum selected request is501.
        let (records, identities, version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: (rows.records.len() + 1) as u32,
                control: Default::default(),
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Records {
                records,
                identities,
                version,
            } => (records, identities, version),
            _ => return Err(drift("initial installed seed read returned wrong result")),
        };
        actual.insert(
            name.clone(),
            ActualRows {
                attributes,
                attributes_version,
                records,
                identities,
                version,
                catalog: catalog_by_name
                    .remove(name)
                    .ok_or_else(|| drift("initial catalog identity missing"))?,
            },
        );
    }
    if !complete_match(&expected, &actual) {
        return Err(drift(
            "complete initial raw seed/view records, counts, digests, keys or layouts differ",
        ));
    }
    Ok(SeedProof { expected, actual })
}

impl SeedProof {
    // Consume only after actual base server drains and wrapper owned cleanup.
    pub(super) fn observe(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "dataset record counts and digests",
            !complete_match(&self.expected, &self.actual),
            || {
                Ok(drift(
                    "initial dataset evidence was incomplete or changed before publication",
                ))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (BTreeMap<String, ExpectedRows>, BTreeMap<String, ActualRows>) {
        // Synthetic comparison fixtures earn zero application acceptance.
        let mut expected = BTreeMap::new();
        let mut actual = BTreeMap::new();
        for index in 0..15 {
            let name = format!("FIXTUR{index:02}");
            let records = vec![b"A000".to_vec(), b"B000".to_vec()];
            let attributes = DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: Some(0),
                key_length: Some(1),
                ccsid: Some(37),
            };
            let identities = vec![b"A".to_vec(), b"B".to_vec()];
            expected.insert(
                name.clone(),
                ExpectedRows {
                    attributes: attributes.clone(),
                    sha256: raw_digest(&records),
                    records: records.clone(),
                    identities: identities.clone(),
                    kind: CatalogEntryKind::Dataset,
                    related: None,
                },
            );
            actual.insert(
                name.clone(),
                ActualRows {
                    attributes,
                    records,
                    identities,
                    attributes_version: 1,
                    version: 1,
                    catalog: CatalogListEntry {
                        name: DatasetName::new(name, 128).unwrap(),
                        kind: CatalogEntryKind::Dataset,
                        related: None,
                        version: 1,
                    },
                },
            );
        }
        (expected, actual)
    }

    #[test]
    fn every_same_count_raw_corruption_refuses_observation() {
        let (expected, original) = fixture();
        assert!(complete_match(&expected, &original));
        for name in original.keys() {
            let mut actual = original.clone();
            actual.get_mut(name).unwrap().records[1][3] ^= 1;
            let mut observed = RouteObservations::default();
            assert!(
                SeedProof {
                    expected: expected.clone(),
                    actual
                }
                .observe(&mut observed)
                .is_err()
            );
            assert!(observed.journeys().is_empty());
        }
    }

    #[test]
    fn every_missing_extra_or_reordered_record_refuses() {
        let (expected, original) = fixture();
        for name in original.keys() {
            for variant in 0..3 {
                let mut actual = original.clone();
                let rows = &mut actual.get_mut(name).unwrap().records;
                match variant {
                    0 => {
                        rows.pop();
                    }
                    1 => rows.push(b"C000".to_vec()),
                    _ => rows.swap(0, 1),
                }
                assert!(!complete_match(&expected, &actual));
            }
        }
    }

    #[test]
    fn every_missing_extra_or_alias_target_refuses() {
        let (expected, original) = fixture();
        for name in original.keys() {
            let mut actual = original.clone();
            let entry = actual.remove(name).unwrap();
            assert!(!complete_match(&expected, &actual));
            actual.insert("EXTRA".into(), entry);
            assert!(!complete_match(&expected, &actual));
        }
        let mut actual = original.clone();
        actual.insert("EXTRA".into(), original["FIXTUR00"].clone());
        assert!(!complete_match(&expected, &actual));
    }

    #[test]
    fn layout_identity_relationship_and_version_drift_refuse() {
        let (expected, original) = fixture();
        for variant in 0..9 {
            let mut actual = original.clone();
            let row = actual.get_mut("FIXTUR00").unwrap();
            match variant {
                0 => row.attributes.key_offset = Some(1),
                1 => row.attributes.key_length = Some(2),
                2 => row.attributes.logical_record_length = 5,
                3 => row.attributes.ccsid = Some(1047),
                4 => row.identities[0] = b"B".to_vec(),
                5 => row.catalog.related = Some(DatasetName::new("OTHER", 128).unwrap()),
                6 => row.catalog.kind = CatalogEntryKind::AlternateIndex,
                7 => row.catalog.name = DatasetName::new("OTHER", 128).unwrap(),
                _ => row.attributes_version += 1,
            }
            assert!(!complete_match(&expected, &actual));
        }
    }

    #[test]
    fn identical_source_alias_is_admitted_but_any_drift_refuses() {
        let (expected, _) = fixture();
        let rows = expected["FIXTUR00"].clone();
        let mut merged = BTreeMap::new();
        merge_source(&mut merged, "ALIAS", rows.clone()).unwrap();
        merge_source(&mut merged, "ALIAS", rows.clone()).unwrap();
        assert_eq!(merged.len(), 1);
        for variant in 0..3 {
            let mut wrong = rows.clone();
            match variant {
                0 => wrong.records[0][3] ^= 1,
                1 => wrong.attributes.key_offset = Some(1),
                _ => wrong.identities[0] = b"B".to_vec(),
            }
            assert!(merge_source(&mut merged, "ALIAS", wrong).is_err());
        }
    }

    #[test]
    fn source_digest_count_and_duplicate_key_drift_refuse() {
        let spec = SeedSpec {
            source: "SYNTHETIC",
            target: "SYNTHETIC",
            lrecl: 4,
            key: Some((0, 1)),
            count: 2,
            sha256: "8874f5240822052658cb2363befb188de6e880de8a2f4834f3d569f8386c3107",
        };
        assert!(source_rows(&spec, b"A000B000".to_vec()).is_ok());
        assert!(source_rows(&spec, b"A001B000".to_vec()).is_err());
        assert!(source_rows(&spec, b"A000".to_vec()).is_err());
        let spec = SeedSpec {
            source: "SYNTHETIC",
            target: "SYNTHETIC",
            lrecl: 4,
            key: Some((0, 1)),
            count: 2,
            sha256: "79b86c10ef1b27c1c6fa6d6649e0c20bf649c05bd69cb6a930750c8f6380c5fb",
        };
        assert!(source_rows(&spec, b"A000A000".to_vec()).is_err());
    }

    #[test]
    fn empty_or_incomplete_proof_cannot_create_observation() {
        let mut observed = RouteObservations::default();
        assert!(
            SeedProof {
                expected: BTreeMap::new(),
                actual: BTreeMap::new()
            }
            .observe(&mut observed)
            .is_err()
        );
        assert!(observed.journeys().is_empty());
    }
}
