//! Corpus-owned IMS metadata and signed controller package fixture.
use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsPcbMetadata, ImsPsbMetadata,
    ImsSecondaryIndexMetadata, ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
};
use ring::hmac;

pub(super) fn sign_carddemo_package_identity(
    identity: &str,
) -> Result<PackageSignature, CorpusProblem> {
    mainframe_env_application::encode_package_authentication(
        "carddemo-conformance-key",
        identity,
        |data| {
            hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA256, b"carddemo-conformance-hmac-key-0001"),
                data,
            )
            .as_ref()
            .try_into()
            .expect("HMAC-SHA256 tag has 32 bytes")
        },
    )
    .map_err(package_problem)
}

pub(super) const APPLICATION: &str = "CARDDEMO-IMS-CORPUS";
const PREFIX: &str = "app/app-authorization-ims-db2-mq";
const DEFINITIONS: [&str; 8] = [
    "DBPAUTP0.dbd",
    "DBPAUTX0.dbd",
    "DLIGSAMP.PSB",
    "PADFLDBD.DBD",
    "PASFLDBD.DBD",
    "PAUTBUNL.PSB",
    "PSBPAUTB.psb",
    "PSBPAUTL.psb",
];

fn problem(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.ims.package", detail)
}

// This bounded tooling projection handles only the macros reached by this pinned
// profile. It is not a production DBDGEN/PSBGEN parser or an IBM compiler.
fn statements(source: &[u8]) -> Result<Vec<(String, String, String)>, CorpusProblem> {
    let source = std::str::from_utf8(source).map_err(|_| problem("definition is not UTF-8"))?;
    let macros = ["DBD", "SEGM", "FIELD", "LCHILD", "PCB", "SENSEG", "PSBGEN"];
    let mut result: Vec<(String, String, String)> = Vec::new();
    for line in source.lines() {
        if line.starts_with('*') || line.trim().is_empty() {
            continue;
        }
        let line = line
            .get(..line.len().min(71))
            .ok_or_else(|| problem("non-ASCII macro"))?
            .trim()
            .to_ascii_uppercase();
        let words = line.split_whitespace().collect::<Vec<_>>();
        let found = words.iter().take(2).position(|word| macros.contains(word));
        if let Some(index) = found {
            let label = if index == 1 { words[0] } else { "" };
            let operands = words[index + 1..].join("");
            result.push((words[index].into(), label.into(), operands));
        } else if line.contains('=')
            && let Some((_, _, operands)) = result.last_mut()
        {
            operands.push_str(&words.join(""));
        }
    }
    Ok(result)
}

fn operand<'a>(text: &'a str, name: &str) -> Result<&'a str, CorpusProblem> {
    let key = format!("{name}=");
    let start = text
        .find(&key)
        .ok_or_else(|| problem(format!("missing {name}")))?
        + key.len();
    let rest = &text[start..];
    let mut depth = 0usize;
    for (offset, byte) in rest.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| problem("unbalanced macro"))?
            }
            b',' if depth == 0 => return Ok(&rest[..offset]),
            _ => {}
        }
    }
    if depth != 0 {
        return Err(problem("unbalanced macro"));
    }
    Ok(rest)
}

fn number(text: &str, name: &str) -> Result<usize, CorpusProblem> {
    operand(text, name)?
        .parse()
        .map_err(|_| problem(format!("invalid {name}")))
}

pub(super) fn metadata(corpus: &Path) -> Result<ImsMetadataCatalog, CorpusProblem> {
    let read = |file: &str| read_corpus_file(corpus, &corpus.join(format!("{PREFIX}/ims/{file}")));
    let mut databases = Vec::new();
    for file in ["DBPAUTP0.dbd", "DBPAUTX0.dbd"] {
        let macros = statements(&read(file)?)?;
        let dbd = macros
            .iter()
            .find(|(kind, _, _)| kind == "DBD")
            .ok_or_else(|| problem("DBD missing"))?;
        let organization = match operand(&dbd.2, "ACCESS")? {
            "(HIDAM,VSAM)" => ImsDatabaseOrganization::Hidam,
            "(INDEX,VSAM,PROT)" => ImsDatabaseOrganization::Index,
            _ => return Err(problem("unsupported pinned DBD organization")),
        };
        let mut segments: Vec<ImsSegmentMetadata> = Vec::new();
        for (kind, _, text) in &macros {
            match kind.as_str() {
                "SEGM" => {
                    let parent = operand(text, "PARENT")?.trim_matches(['(', ')', ',']);
                    let length = number(text, "BYTES")?;
                    segments.push(ImsSegmentMetadata {
                        name: operand(text, "NAME")?.into(),
                        parent: (parent != "0").then(|| parent.into()),
                        min_length: length,
                        max_length: length,
                        fields: Vec::new(),
                    });
                }
                "FIELD" => {
                    let names = operand(text, "NAME")?
                        .trim_matches(['(', ')'])
                        .split(',')
                        .collect::<Vec<_>>();
                    let segment = segments
                        .last_mut()
                        .ok_or_else(|| problem("FIELD before SEGM"))?;
                    segment.fields.push(ImsFieldMetadata {
                        name: Some(names[0].into()),
                        offset: number(text, "START")?
                            .checked_sub(1)
                            .ok_or_else(|| problem("START is one-based"))?,
                        length: number(text, "BYTES")?,
                        sequence: names.contains(&"SEQ"),
                        unique: names.contains(&"U"),
                    });
                }
                _ => {}
            }
        }
        databases.push(ImsDatabaseMetadata {
            gsam_format: None,
            name: operand(&dbd.2, "NAME")?.into(),
            version: 1,
            organization,
            segments,
            secondary_indexes: Vec::new(),
            logical_relationships: Vec::new(),
        });
    }
    // The profile's maintained index projection comes from both reciprocal
    // LCHILD declarations, not a fabricated application database definition.
    let primary = statements(&read("DBPAUTP0.dbd")?)?;
    let index = statements(&read("DBPAUTX0.dbd")?)?;
    let link = primary
        .iter()
        .find(|(kind, _, _)| kind == "LCHILD")
        .ok_or_else(|| problem("primary index link missing"))?;
    let reverse = index
        .iter()
        .find(|(kind, _, _)| kind == "LCHILD")
        .ok_or_else(|| problem("index target missing"))?;
    if operand(&link.2, "NAME")? != "(PAUTINDX,DBPAUTX0)"
        || operand(&reverse.2, "NAME")? != "(PAUTSUM0,DBPAUTP0)"
    {
        return Err(problem("index link differs from the pinned profile"));
    }
    let primary_index = ImsSecondaryIndexMetadata {
        name: databases[1].name.clone(),
        target_segment: databases[0].segments[0].name.clone(),
        source_segment: databases[0].segments[0].name.clone(),
        source_fields: vec![operand(&reverse.2, "INDEX")?.into()],
    };
    databases[0].secondary_indexes.push(primary_index);
    let mut psbs = Vec::new();
    for file in ["PSBPAUTB.psb", "PSBPAUTL.psb", "PAUTBUNL.PSB"] {
        let macros = statements(&read(file)?)?;
        let psbgen = macros
            .iter()
            .find(|(kind, _, _)| kind == "PSBGEN")
            .ok_or_else(|| problem("PSBGEN missing"))?;
        let pcb = macros
            .iter()
            .find(|(kind, _, _)| kind == "PCB")
            .ok_or_else(|| problem("PCB missing"))?;
        let sensitive_segments = macros
            .iter()
            .filter(|(kind, _, _)| kind == "SENSEG")
            .map(|(_, _, text)| {
                let parent = operand(text, "PARENT")?;
                Ok(ImsSensitiveSegmentMetadata {
                    name: operand(text, "NAME")?.into(),
                    parent: (parent != "0").then(|| parent.into()),
                    processing_options: None,
                })
            })
            .collect::<Result<Vec<_>, CorpusProblem>>()?;
        psbs.push(ImsPsbMetadata {
            name: operand(&psbgen.2, "PSBNAME")?.into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: pcb.1.clone(),
                database: operand(&pcb.2, "DBDNAME")?.into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: operand(&pcb.2, "PROCOPT")?.into(),
                sensitive_segments,
            })],
        });
    }
    Ok(ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases,
        psbs,
    })
}

pub(super) fn package(
    corpus: &Path,
    generation: u64,
    programs: &[BatchProgramDefinition],
) -> Result<ApplicationPackageV2, CorpusProblem> {
    let catalog = metadata(corpus)?;
    let mut payloads = Vec::new();
    for file in [
        "CBPAUP0C.cbl",
        "COPAUA0C.cbl",
        "COPAUS0C.cbl",
        "COPAUS1C.cbl",
        "COPAUS2C.cbl",
        "DBUNLDGS.CBL",
        "PAUDBLOD.CBL",
        "PAUDBUNL.CBL",
    ] {
        payloads.push((
            EntryKind::Source,
            format!("source/cbl/{file}"),
            read_corpus_file(corpus, &corpus.join(format!("{PREFIX}/cbl/{file}")))?,
        ));
    }
    for program in programs {
        payloads.push((
            EntryKind::Program,
            format!("program/exec-dli/{}", program.name),
            program.payload.clone(),
        ));
    }
    for file in DEFINITIONS {
        payloads.push((
            EntryKind::Source,
            format!("source/ims/{file}"),
            read_corpus_file(corpus, &corpus.join(format!("{PREFIX}/ims/{file}")))?,
        ));
    }
    for file in ["LOADPADB.JCL", "UNLDPADB.JCL"] {
        payloads.push((
            EntryKind::Source,
            format!("source/jcl/{file}"),
            read_corpus_file(corpus, &corpus.join(format!("{PREFIX}/jcl/{file}")))?,
        ));
    }
    let controllers = carddemo_batch_controllers()
        .into_iter()
        .filter(|controller| {
            matches!(
                controller.name.as_str(),
                "AUTHORIZATION-IMS-LOAD" | "AUTHORIZATION-IMS-UNLOAD"
            )
        })
        .collect::<Vec<_>>();
    for controller in &controllers {
        let name = controller
            .properties
            .get("selector-program")
            .ok_or_else(|| problem("controller program missing"))?;
        let bytes = read_corpus_file(corpus, &corpus.join(format!("{PREFIX}/cbl/{name}.CBL")))?;
        payloads.push((EntryKind::Program, controller.program.clone(), bytes));
    }
    payloads.extend([
        (
            EntryKind::Resource,
            "resource/ims/catalog".into(),
            serde_json::to_vec(&catalog).map_err(package_problem)?,
        ),
        (
            EntryKind::Data,
            "data/ims/profile".into(),
            b"bounded-independent-record-fixture@1".to_vec(),
        ),
        (
            EntryKind::Profile,
            "profile/ims".into(),
            b"carddemo-ims".to_vec(),
        ),
        (
            EntryKind::Migration,
            "migration/ims".into(),
            b"application-package-v1-to-v2".to_vec(),
        ),
    ]);
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for (kind, path, bytes) in payloads {
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        entries.push(PackageEntry {
            path,
            kind,
            sha256: digest.clone(),
            bytes: bytes.len(),
            depends_on: (kind != EntryKind::Source)
                .then(|| "source/ims/DBPAUTP0.dbd".into())
                .into_iter()
                .collect(),
        });
        blobs.insert(digest, bytes);
    }
    let mut package = ApplicationPackageV2 {
        base: ApplicationPackage {
            manifest: ApplicationManifest {
                name: APPLICATION.into(),
                version: "0.2.0".into(),
                target_product: "0.2.0".into(),
                entries,
            },
            blobs,
        },
        generation,
        sections: ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V3_CONTRACT.into(),
            host_abi_libraries: Vec::new(),
            sql_tables: Vec::new(),
            sql_rows: Vec::new(),
            ims_definitions: Vec::new(),
            ims_rows: Vec::new(),
            ims_metadata: Some(catalog),
            ims_tm: None,
            mq_resources: Vec::new(),
            batch_controllers: controllers,
            security_resources: Vec::new(),
        },
        signature: PackageSignature {
            algorithm: mainframe_env_application::PACKAGE_AUTHENTICATION_ALGORITHM.into(),
            key_id: "carddemo-conformance-key".into(),
            value: "pending".into(),
        },
    };
    package.signature = sign_carddemo_package_identity(
        &package_generation_identity(&package).map_err(package_problem)?,
    )?;
    Ok(package)
}
