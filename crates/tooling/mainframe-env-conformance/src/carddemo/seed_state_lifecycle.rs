//! Actual isolated seed generations and CSD file routes; no application execution credit.
//! Existing local direct AIX views and Cp037 host transport remain qualified.
use super::*;
use mainframe_env_cics::CicsService;
use mainframe_env_dataset::SeedInstallReceipt;
use mainframe_env_host_api::{CicsDisposition, HostRequest};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::sync::Mutex;

#[path = "seed_alias_routes.rs"]
mod seed_alias_routes;
use seed_alias_routes::alias_routes;

const ORACLE: &str = include_str!("seed_lifecycle_expected.json");
const PHASES: &[&str] = &[
    "g1",
    "reinstall",
    "g2",
    "rollback",
    "corrupt-refused",
    "capacity-refused",
    "aliases",
    "reopen",
];
const DATA_NAMESPACES: &[(&str, usize)] = &[
    ("dataset", 12),
    ("dataset-aix", 3),
    ("dataset-seed-generation", 2),
    ("dataset-seed-selection", 1),
];

#[derive(Deserialize)]
struct KeyRange {
    offset: u32,
    length: u32,
    record_length: u32,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostRecord {
    ordinal: usize,
    row_bytes: usize,
    row_sha256: String,
    key_bytes: usize,
    key_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    name: String,
    dataset: String,
    aix: bool,
    records: Vec<HostRecord>,
    #[serde(skip)]
    host_rows: Vec<Vec<u8>>,
    #[serde(skip)]
    host_keys: Vec<Vec<u8>>,
}
#[derive(Deserialize)]
struct ExternalOracle {
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct FullAlias {
    name: String,
    dataset: String,
    aix: bool,
    key_host_base64: Vec<String>,
    rows_host_base64: Vec<String>,
}
#[derive(Deserialize)]
struct FullOracle {
    schema_version: String,
    corpus_commit: String,
    aliases: Vec<FullAlias>,
}
#[derive(Deserialize)]
struct Oracle {
    schema_version: String,
    corpus_commit: String,
    source_pins: BTreeMap<String, String>,
    runtime_key_ranges: BTreeMap<String, KeyRange>,
    external_oracle: ExternalOracle,
    import_member_sha256: String,
    aliases: Vec<Alias>,
    upgrade_dalytran_sha256: String,
    input_records: usize,
    input_bytes: usize,
    generation_1_identity: String,
    generation_2_identity: String,
}

fn refuse(value: bool, message: &str) -> Result<(), CorpusProblem> {
    if value { Err(drift(message)) } else { Ok(()) }
}
fn bytes(encoded: &str) -> Result<Vec<u8>, CorpusProblem> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(package_problem)
}
fn complete_host_expectations(alias: &Alias) -> bool {
    !alias.records.is_empty()
        && alias.records.len() <= 501
        && alias.records.len() == alias.host_rows.len()
        && alias.records.len() == alias.host_keys.len()
        && alias
            .records
            .iter()
            .zip(&alias.host_rows)
            .zip(&alias.host_keys)
            .enumerate()
            .all(|(ordinal, ((record, row), key))| {
                record.ordinal == ordinal
                    && record.row_bytes == row.len()
                    && record.key_bytes == key.len()
                    && record.row_sha256 == format!("{:x}", Sha256::digest(row))
                    && record.key_sha256 == format!("{:x}", Sha256::digest(key))
            })
}
fn external_expectations(path: &Path, seal: &ExternalOracle) -> Result<Vec<u8>, CorpusProblem> {
    refuse(
        seal.bytes != 103075
            || seal.sha256 != "83c98c8ca2247e3c456e35c2111508d147ad538f9b140e7c0e3dca1eedf3b930",
        "independent full expectation seal differs",
    )?;
    let before = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !before.is_file() || before.len() != seal.bytes,
        "independent full expectation is not the exact plain file",
    )?;
    let file = fs::File::open(path).map_err(package_problem)?;
    let opened = file.metadata().map_err(package_problem)?;
    refuse(
        !opened.is_file()
            || (before.dev(), before.ino(), before.len())
                != (opened.dev(), opened.ino(), opened.len()),
        "independent full expectation identity changed",
    )?;
    let mut raw = Vec::new();
    file.take(seal.bytes + 1)
        .read_to_end(&mut raw)
        .map_err(package_problem)?;
    let after = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !after.is_file()
            || (before.dev(), before.ino(), before.len())
                != (after.dev(), after.ino(), after.len())
            || raw.len() as u64 != seal.bytes
            || format!("{:x}", Sha256::digest(&raw)) != seal.sha256,
        "independent full expectation bytes or identity differ",
    )?;
    Ok(raw)
}
fn bind_host_expectations(oracle: &mut Oracle, raw: &[u8]) -> Result<(), CorpusProblem> {
    let full: FullOracle = serde_json::from_slice(raw).map_err(package_problem)?;
    refuse(
        full.schema_version != "private.carddemo.seed-lifecycle-oracle@1"
            || full.corpus_commit != oracle.corpus_commit
            || full.aliases.len() != oracle.aliases.len(),
        "independent full expectation shape differs",
    )?;
    for (alias, full) in oracle.aliases.iter_mut().zip(full.aliases) {
        refuse(
            alias.name != full.name || alias.dataset != full.dataset || alias.aix != full.aix,
            "independent full expectation alias/order differs",
        )?;
        alias.host_rows = full
            .rows_host_base64
            .iter()
            .map(|row| bytes(row))
            .collect::<Result<_, _>>()?;
        alias.host_keys = full
            .key_host_base64
            .iter()
            .map(|key| bytes(key))
            .collect::<Result<_, _>>()?;
        refuse(
            !complete_host_expectations(alias),
            "independent full expectation row/key/ordinal/length/hash differs",
        )?;
    }
    Ok(())
}
fn pinned(corpus: &Path) -> Result<Oracle, CorpusProblem> {
    let mut oracle: Oracle = serde_json::from_str(ORACLE).map_err(package_problem)?;
    refuse(
        oracle.schema_version != "private.carddemo.seed-lifecycle-metadata@1"
            || oracle.corpus_commit != "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            || oracle.source_pins.len() != 15
            || oracle.runtime_key_ranges.len() != 13
            || oracle.aliases.len() != 8
            || oracle.aliases.iter().filter(|row| row.aix).count() != 2
            || oracle.input_records != 1187
            || oracle.input_bytes != 427700,
        "frozen lifecycle oracle differs",
    )?;
    for (path, expected) in &oracle.source_pins {
        let raw = read_corpus_file(corpus, &corpus.join(path))?;
        refuse(
            format!("{:x}", Sha256::digest(&raw)) != *expected,
            "lifecycle offered source differs",
        )?;
    }
    let raw = read_runtime_oracle(
        corpus,
        "samples/m2/mf/CardDemo_runtime.zip!import_dataset.json",
    )?;
    refuse(
        format!("{:x}", Sha256::digest(&raw)) != oracle.import_member_sha256,
        "runtime catalog member differs",
    )?;
    let import: serde_json::Value = serde_json::from_slice(&raw).map_err(package_problem)?;
    let mut actual = BTreeMap::new();
    for wrapper in import["dataSets"]
        .as_array()
        .ok_or_else(|| drift("runtime datasets missing"))?
    {
        let data = &wrapper["dataSet"];
        let vsam = &data["datasetOrg"]["vsam"];
        if vsam["format"].as_str() != Some("KS") {
            continue;
        }
        let name = data["datasetName"]
            .as_str()
            .ok_or_else(|| drift("runtime key target missing"))?;
        let tuple = (
            vsam["primaryKey"]["offset"].as_u64(),
            vsam["primaryKey"]["length"].as_u64(),
            data["recordLength"]["max"].as_u64(),
        );
        refuse(
            actual.insert(name.to_string(), tuple).is_some(),
            "runtime key target duplicated",
        )?;
    }
    let expected = oracle
        .runtime_key_ranges
        .iter()
        .map(|(name, key)| {
            (
                name.clone(),
                (
                    Some(u64::from(key.offset)),
                    Some(u64::from(key.length)),
                    Some(u64::from(key.record_length)),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    refuse(
        actual != expected,
        "complete thirteen runtime key definitions differ",
    )?;
    // EXPORT's source-owned local (28,4) layout is not in this migrated catalog.
    refuse(
        actual.contains_key("AWS.M2.CARDDEMO.EXPORT.DATA"),
        "EXPORT local/catalog qualification changed",
    )?;
    let csd = String::from_utf8(read_corpus_file(
        corpus,
        &corpus.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(package_problem)?;
    let actual = parse_csd(&csd)
        .map_err(package_problem)?
        .into_iter()
        .filter(|row| row.kind == "FILE")
        .map(|row| {
            let target = row
                .properties
                .get("DSNAME")
                .cloned()
                .ok_or_else(|| drift("CSD alias target missing"))?;
            Ok((row.name, target))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let expected = oracle
        .aliases
        .iter()
        .map(|row| (row.name.clone(), row.dataset.clone()))
        .collect::<BTreeMap<_, _>>();
    refuse(
        actual != expected || expected.len() != 8,
        "complete source CSD FILE aliases differ",
    )?;
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_SEED_LIFECYCLE_EXPECTED")
            .ok_or_else(|| drift("independent external full expectation input required"))?,
    );
    let raw = external_expectations(&offered, &oracle.external_oracle)?;
    bind_host_expectations(&mut oracle, &raw)?;
    Ok(oracle)
}

fn upgraded(
    mut expected: BTreeMap<String, ExpectedRows>,
    oracle: &Oracle,
) -> Result<BTreeMap<String, ExpectedRows>, CorpusProblem> {
    let rows = expected
        .get_mut("AWS.M2.CARDDEMO.DALYTRAN.PS")
        .ok_or_else(|| drift("upgrade target missing"))?;
    let last = rows
        .records
        .last_mut()
        .and_then(|row| row.last_mut())
        .ok_or_else(|| drift("upgrade row missing"))?;
    *last ^= 1;
    rows.sha256 = raw_digest(&rows.records);
    refuse(
        rows.sha256 != oracle.upgrade_dalytran_sha256,
        "independent controlled upgrade bytes differ",
    )?;
    Ok(expected)
}
fn same(before: &SeedProof, after: &SeedProof) -> bool {
    before.expected == after.expected && before.actual == after.actual
}
fn advanced(before: &SeedProof, after: &SeedProof) -> bool {
    before.actual.keys().eq(after.actual.keys())
        && before
            .actual
            .iter()
            .all(|(name, row)| row.version.checked_add(1) == Some(after.actual[name].version))
}
fn state(
    store: &dyn ProviderStateStore,
) -> Result<BTreeMap<String, Vec<ProviderStateRecord>>, CorpusProblem> {
    DATA_NAMESPACES
        .iter()
        .map(|(namespace, bound)| {
            let rows = store
                .list_provider_state(namespace, bound + 1)
                .map_err(package_problem)?;
            refuse(
                rows.len() > *bound,
                "seed provider namespace has an extra row",
            )?;
            Ok(((*namespace).to_string(), rows))
        })
        .collect()
}
fn check_receipt(
    receipt: &SeedInstallReceipt,
    generation: &str,
    identity: &str,
    replayed: bool,
) -> Result<serde_json::Value, CorpusProblem> {
    refuse(
        receipt.package != "CARDDEMO"
            || receipt.generation != generation
            || receipt.identity != identity
            || receipt.replayed != replayed
            || receipt.seed_objects != 13
            || receipt.datasets != 12
            || receipt.records != 1137
            || receipt.bytes != 427700,
        "actual seed generation receipt differs from independent complete offered inputs",
    )?;
    Ok(
        serde_json::json!({"package":receipt.package,"generation":receipt.generation,"identity":receipt.identity,
        "replayed":receipt.replayed,"seed_objects":receipt.seed_objects,"datasets":receipt.datasets,"records":receipt.records,"bytes":receipt.bytes}),
    )
}
fn state_json(rows: &BTreeMap<String, Vec<ProviderStateRecord>>) -> serde_json::Value {
    serde_json::Value::Object(rows.iter().map(|(namespace,records)| (namespace.clone(),serde_json::Value::Array(records.iter().map(|row|serde_json::json!({"namespace":row.namespace,"key":row.key,"version":row.version,"payload_base64":base64::engine::general_purpose::STANDARD.encode(&row.payload)})).collect()))).collect())
}
fn runtime_keys(oracle: &Oracle, proof: &SeedProof) -> bool {
    oracle.runtime_key_ranges.iter().all(|(name, key)| {
        proof.actual.get(name).is_some_and(|row| {
            row.attributes.key_offset == Some(key.offset)
                && row.attributes.key_length == Some(key.length)
                && row.attributes.logical_record_length == key.record_length
        })
    })
}
fn require_generation(server: &ProductServer, generation: &str) -> Result<(), CorpusProblem> {
    refuse(
        server
            .dataset_service()
            .selected_seed_generation("CARDDEMO")
            .map_err(terminal_problem)?
            .as_deref()
            != Some(generation),
        "complete snapshot and selected seed generation differ",
    )
}
fn alias_records(oracle: &Oracle) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    oracle
        .aliases
        .iter()
        .map(|row| {
            let mut payload = b"MEFA1".to_vec();
            payload.extend_from_slice(&(row.dataset.len() as u32).to_be_bytes());
            payload.extend_from_slice(row.dataset.as_bytes());
            payload.extend_from_slice(&37u16.to_be_bytes());
            Ok((
                row.name.clone(),
                ProviderStateRecord {
                    namespace: "cics-file-alias".into(),
                    key: row.name.clone(),
                    version: 1,
                    payload,
                },
            ))
        })
        .collect()
}
fn check_alias_records(
    store: &dyn ProviderStateStore,
    oracle: &Oracle,
) -> Result<(), CorpusProblem> {
    let rows = store
        .list_provider_state("cics-file-alias", 9)
        .map_err(package_problem)?;
    let actual = rows
        .iter()
        .map(|row| (row.key.clone(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    refuse(
        rows.len() != 8 || actual.len() != 8 || actual != alias_records(oracle)?,
        "actual installed CICS aliases differ",
    )
}
fn operand(raw: Vec<u8>) -> Result<BoundedPayload, CorpusProblem> {
    BoundedPayload::new(
        "mainframe-env.cics.literal@1",
        raw,
        InvocationLimits::default(),
    )
    .map_err(package_problem)
}
fn command(
    owner: &Owner,
    service: &CicsService,
    invocation: &Invocation,
    sequence: &mut u64,
    operation: CicsOperation,
    arguments: BTreeMap<String, BoundedPayload>,
) -> Result<mainframe_env_host_api::CicsResponse, CorpusProblem> {
    owner.verify()?;
    *sequence += 1;
    let request = CicsRequest {
        operation,
        arguments,
        condition_policy: CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        },
        mutation: operation.is_mutating().then(|| Mutation {
            sequence: *sequence,
            idempotency_key: IdempotencyKey::new(
                format!("seed-route-{}-{sequence}", invocation.run_unit_id.as_str()),
                InvocationLimits::default(),
            )
            .expect("bounded route key"),
            transaction: Some("SEED".into()),
        }),
    };
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: *sequence,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: request
            .mutation
            .as_ref()
            .map(|row| row.idempotency_key.clone()),
        request: HostRequest::Cics(request.clone()),
    };
    service.invoke(&effect, request).map_err(terminal_problem)
}
fn normal(response: &mainframe_env_host_api::CicsResponse) -> bool {
    response.payload.schema() == "mainframe-env.cics.payload@1"
        && response.applid == "CARDDEMO"
        && response.sysid == "LOCAL"
        && response.transaction == "SEED"
        && response.condition == "NORMAL"
        && response.response == 0
        && response.response2 == 0
        && response.disposition == CicsDisposition::Complete
        && response.target.is_none()
        && response.next_transaction.is_none()
}
fn row_reply(
    response: &mainframe_env_host_api::CicsResponse,
    expected: &[u8],
    key: Option<&[u8]>,
) -> bool {
    normal(response)
        && response.payload.bytes() == expected
        && key.is_none_or(|key| {
            response.outputs.get("RIDFLD").is_some_and(|actual| {
                actual.schema() == "mainframe-env.cics.payload@1" && actual.bytes() == key
            })
        })
}
fn eof_reply(response: &mainframe_env_host_api::CicsResponse) -> bool {
    response.payload.schema() == "mainframe-env.cics.payload@1"
        && response.applid == "CARDDEMO"
        && response.sysid == "LOCAL"
        && response.transaction == "SEED"
        && response.condition == "ENDFILE"
        && response.response == 20
        && response.response2 == 0
        && response.disposition == CicsDisposition::Complete
        && response.payload.bytes().is_empty()
        && response.target.is_none()
        && response.next_transaction.is_none()
}

fn proof_json(proof: &SeedProof) -> serde_json::Value {
    serde_json::Value::Object(proof.actual.iter().map(|(name, row)| (name.clone(), serde_json::json!({"version":row.version,"attributes_version":row.attributes_version,"records_base64":row.records.iter().map(|raw| base64::engine::general_purpose::STANDARD.encode(raw)).collect::<Vec<_>>(),"identities_base64":row.identities.iter().map(|raw| base64::engine::general_purpose::STANDARD.encode(raw)).collect::<Vec<_>>(),"raw_sha256":raw_digest(&row.records),"key_offset":row.attributes.key_offset,"key_length":row.attributes.key_length,"lrecl":row.attributes.logical_record_length,"ccsid":row.attributes.ccsid,"catalog_version":row.catalog.version,"catalog_related":row.catalog.related.as_ref().map(DatasetName::as_str)}))).collect())
}
fn stage(
    server: &ProductServer,
    report: &mut serde_json::Value,
    name: &str,
    proof: &SeedProof,
) -> Result<(), CorpusProblem> {
    refuse(
        !complete_match(&proof.expected, &proof.actual),
        "lifecycle stage has incomplete raw proof",
    )?;
    let rows = report["phases"]
        .as_array_mut()
        .ok_or_else(|| drift("owned phase ledger invalid"))?;
    refuse(
        PHASES.get(rows.len()) != Some(&name),
        "actual lifecycle phases skipped or reordered",
    )?;
    let generation = server
        .dataset_service()
        .selected_seed_generation("CARDDEMO")
        .map_err(terminal_problem)?;
    refuse(
        generation.as_deref() != Some(if name == "g2" { "g2" } else { "g1" }),
        "phase selected generation differs",
    )?;
    rows.push(serde_json::json!({"phase":name,"selected_generation":generation,"complete_raw_snapshot":proof_json(proof)}));
    Ok(())
}

#[derive(Clone)]
struct RetainedReceipt {
    identity: (u64, u64),
    bytes: usize,
    sha256: String,
}
struct Owner {
    evidence: PathBuf,
    evidence_id: (u64, u64),
    owned: PathBuf,
    owned_id: (u64, u64),
    receipt: Mutex<Option<RetainedReceipt>>,
}
fn receipt_destination(
    path: &Path,
    expected: Option<&RetainedReceipt>,
) -> Result<(), CorpusProblem> {
    let before = fs::symlink_metadata(path);
    let Some(expected) = expected else {
        return match before {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(drift(
                "first retained seed receipt destination is unknown or unobservable",
            )),
        };
    };
    let before = before.map_err(package_problem)?;
    refuse(
        !before.is_file()
            || (before.dev(), before.ino()) != expected.identity
            || before.len() != expected.bytes as u64,
        "prior retained seed receipt identity differs",
    )?;
    let file = fs::File::open(path).map_err(package_problem)?;
    let opened = file.metadata().map_err(package_problem)?;
    refuse(
        !opened.is_file()
            || (opened.dev(), opened.ino()) != expected.identity
            || opened.len() != expected.bytes as u64,
        "opened prior seed receipt identity differs",
    )?;
    let mut raw = Vec::new();
    file.take(expected.bytes as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(package_problem)?;
    let after = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !after.is_file()
            || (after.dev(), after.ino()) != expected.identity
            || after.len() != expected.bytes as u64
            || raw.len() != expected.bytes
            || format!("{:x}", Sha256::digest(raw)) != expected.sha256,
        "prior retained seed receipt bytes or identity differ",
    )
}
fn directory_id(path: &Path) -> Result<(u64, u64), CorpusProblem> {
    let metadata = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !metadata.is_dir(),
        "owned seed namespace is not a plain directory",
    )?;
    Ok((metadata.dev(), metadata.ino()))
}
fn absence(observed: std::io::Result<fs::Metadata>) -> Result<(), CorpusProblem> {
    match observed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(drift("owned seed state absence observation failed")),
        Ok(_) => Err(drift("owned seed state remains after disposal")),
    }
}
fn target_counts() -> BTreeMap<String, usize> {
    let mut counts = SEEDS
        .iter()
        .map(|spec| (spec.target.to_string(), spec.count))
        .collect::<BTreeMap<_, _>>();
    for view in VIEWS {
        if let Some(count) = counts.get(view.base).copied() {
            counts.insert(view.target.into(), count);
        }
    }
    counts
}
fn snapshot_shape(value: &serde_json::Value) -> bool {
    let Some(actual) = value.as_object() else {
        return false;
    };
    let expected = target_counts();
    actual.keys().eq(expected.keys())
        && expected.iter().all(|(name, count)| {
            let row = &actual[name];
            let version = row["version"].as_u64();
            version.is_some_and(|value| value > 0)
                && row["attributes_version"].as_u64() == version
                && row["catalog_version"].as_u64() == version
                && row["records_base64"].as_array().map(Vec::len) == Some(*count)
                && row["identities_base64"].as_array().map(Vec::len) == Some(*count)
        })
}
fn finished(report: &serde_json::Value) -> bool {
    report["phases"].as_array().is_some_and(|rows| {
        rows.len() == PHASES.len()
            && rows.iter().zip(PHASES).all(|(row, phase)| {
                row["phase"].as_str() == Some(phase)
                    && row["selected_generation"].as_str()
                        == Some(if *phase == "g2" { "g2" } else { "g1" })
                    && snapshot_shape(&row["complete_raw_snapshot"])
            })
    }) && report["shutdowns"] == serde_json::json!([true, true])
}
impl Owner {
    fn claim(evidence: PathBuf) -> Result<Self, CorpusProblem> {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&evidence)
            .map_err(package_problem)?;
        let evidence_id = directory_id(&evidence)?;
        let owned = evidence.join("owned-state");
        refuse(
            directory_id(&evidence)? != evidence_id,
            "owned seed evidence identity changed",
        )?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&owned)
            .map_err(package_problem)?;
        let owned_id = directory_id(&owned)?;
        Ok(Self {
            evidence,
            evidence_id,
            owned,
            owned_id,
            receipt: Mutex::new(None),
        })
    }
    fn verify(&self) -> Result<(), CorpusProblem> {
        refuse(
            directory_id(&self.evidence)? != self.evidence_id
                || directory_id(&self.owned)? != self.owned_id,
            "owned seed namespace identity changed",
        )
    }
    fn save(&self, report: &serde_json::Value) -> Result<(), CorpusProblem> {
        self.save_observed(report, || {}, || {})
    }
    fn save_observed(
        &self,
        report: &serde_json::Value,
        before_publication: impl FnOnce(),
        after_publication: impl FnOnce(),
    ) -> Result<(), CorpusProblem> {
        let mut retained = self.receipt.lock().map_err(package_problem)?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed",
        )?;
        let destination = self.evidence.join("seed-lifecycle.json");
        receipt_destination(&destination, retained.as_ref())?;
        let raw = serde_json::to_vec_pretty(report).map_err(package_problem)?;
        refuse(
            raw.len() > 64 * 1024 * 1024,
            "retained seed evidence exceeds original output ceiling",
        )?;
        let temporary = self.evidence.join("seed-lifecycle.json.next");
        let mut stream = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(package_problem)?;
        let opened = stream.metadata().map_err(package_problem)?;
        refuse(
            !opened.is_file(),
            "opened retained seed staging file is not plain",
        )?;
        let identity = (opened.dev(), opened.ino());
        stream.write_all(&raw).map_err(package_problem)?;
        stream.sync_all().map_err(package_problem)?;
        before_publication();
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed before receipt publication",
        )?;
        let written = stream.metadata().map_err(package_problem)?;
        let staged = fs::symlink_metadata(&temporary).map_err(package_problem)?;
        refuse(
            !written.is_file()
                || !staged.is_file()
                || (written.dev(), written.ino()) != identity
                || (staged.dev(), staged.ino()) != identity
                || written.len() != raw.len() as u64
                || staged.len() != raw.len() as u64,
            "retained seed staging identity differs after sync",
        )?;
        let published = RetainedReceipt {
            identity,
            bytes: raw.len(),
            sha256: format!("{:x}", Sha256::digest(&raw)),
        };
        receipt_destination(&temporary, Some(&published))?;
        receipt_destination(&destination, retained.as_ref())?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed at receipt publication",
        )?;
        fs::rename(&temporary, &destination).map_err(package_problem)?;
        after_publication();
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed after receipt publication",
        )?;
        receipt_destination(&destination, Some(&published))?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed after final receipt observation",
        )?;
        *retained = Some(published);
        Ok(())
    }
    fn cleanup(&self, report: &mut serde_json::Value) -> Result<(), CorpusProblem> {
        self.verify()?;
        refuse(
            !finished(report),
            "actual seed comparisons or server closure incomplete",
        )?;
        report["cleanup"] = serde_json::json!("verified-owned-removal-pending");
        self.save(report)?;
        self.verify()?;
        fs::remove_dir_all(&self.owned).map_err(package_problem)?;
        absence(fs::symlink_metadata(&self.owned))?;
        report["cleanup"] = serde_json::json!("removed-and-absence-verified");
        report["status"] = serde_json::json!("pass");
        self.save(report)
    }
}

async fn run(
    corpus: &Path,
    inventory: &Path,
    evidence: PathBuf,
) -> Result<RouteObservations, CorpusProblem> {
    verify_carddemo_corpus(corpus, inventory)?;
    let oracle = pinned(corpus)?;
    let expected = expected_rows(corpus)?;
    let g2_expected = upgraded(expected.clone(), &oracle)?;
    let objects = carddemo_base_seed_objects(corpus)?;
    let owner = Owner::claim(evidence)?;
    let mut report = serde_json::json!({"schema":"private.carddemo.seed-lifecycle@1","status":"running","phases":[],"shutdowns":[],"cleanup":"pending","issue_observations":[],"application_credit":0,"oracle_sha256":format!("{:x}",Sha256::digest(ORACLE.as_bytes())),"external_oracle_sha256":oracle.external_oracle.sha256,"external_oracle_bytes":oracle.external_oracle.bytes,"corpus_commit":oracle.corpus_commit,"owned_identity":[owner.owned_id.0,owner.owned_id.1]});
    owner.save(&report)?;
    owner.verify()?;
    let url = format!(
        "sqlite:{}?mode=rwc",
        owner.owned.join("state.sqlite").display()
    );
    let config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: url.clone(),
        artifact_root: owner.owned.join("artifacts"),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?);
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let result = (|| {
        owner.verify()?;
        server
            .bootstrap_user("IBMUSER", b"SEEDPASS")
            .map_err(terminal_problem)?;
        let service = server.dataset_service();
        owner.verify()?;
        let receipt = service
            .install_seed_generation("CARDDEMO", "g1", objects.clone())
            .map_err(terminal_problem)?;
        report["initial_receipt"] =
            check_receipt(&receipt, "g1", &oracle.generation_1_identity, false)?;
        for (index, spec) in VIEWS.iter().enumerate() {
            owner.verify()?;
            service
                .invoke(DatasetRequest::DefineAlternateIndex {
                    base: DatasetName::new(spec.base, 128).map_err(package_problem)?,
                    index: DatasetName::new(spec.target, 128).map_err(package_problem)?,
                    key_offset: spec.key.0,
                    key_length: spec.key.1,
                    allow_duplicates: true,
                    upgrade: true,
                    mutation: Mutation {
                        sequence: index as u64 + 1,
                        idempotency_key: IdempotencyKey::new(
                            format!("seed-index-{index}"),
                            InvocationLimits::default(),
                        )
                        .map_err(package_problem)?,
                        transaction: Some("SEED-INSTALL".into()),
                    },
                })
                .map_err(terminal_problem)?;
        }
        let initial = capture_expected(&server, expected.clone())?;
        require_generation(&server, "g1")?;
        stage(&server, &mut report, "g1", &initial)?;
        owner.save(&report)?;
        refuse(
            !runtime_keys(&oracle, &initial),
            "actual installed layouts disagree with runtime key oracle",
        )?;
        let initial_state = state(store.as_ref())?;
        report["initial_provider_state"] = state_json(&initial_state);
        owner.save(&report)?;
        owner.verify()?;
        let replay = service
            .install_seed_generation("CARDDEMO", "g1", objects.clone())
            .map_err(terminal_problem)?;
        report["reinstall_receipt"] =
            check_receipt(&replay, "g1", &oracle.generation_1_identity, true)?;
        let reinstalled = capture_expected(&server, expected.clone())?;
        require_generation(&server, "g1")?;
        refuse(
            !replay.replayed
                || !same(&initial, &reinstalled)
                || initial_state != state(store.as_ref())?,
            "reinstall changed complete records/layout/identities/versions",
        )?;
        stage(&server, &mut report, "reinstall", &reinstalled)?;
        owner.save(&report)?;
        report["reinstall_provider_state"] = state_json(&state(store.as_ref())?);
        owner.save(&report)?;
        let mut upgrade_objects = objects.clone();
        let upgrade = upgrade_objects
            .iter_mut()
            .find(|row| row.dataset.as_str() == "AWS.M2.CARDDEMO.DALYTRAN.PS")
            .ok_or_else(|| drift("actual upgrade object missing"))?;
        *upgrade
            .bytes
            .last_mut()
            .ok_or_else(|| drift("actual upgrade bytes missing"))? ^= 1;
        upgrade.sha256 = format!("sha256:{:x}", Sha256::digest(&upgrade.bytes));
        owner.verify()?;
        let receipt = service
            .install_seed_generation("CARDDEMO", "g2", upgrade_objects)
            .map_err(terminal_problem)?;
        report["upgrade_receipt"] =
            check_receipt(&receipt, "g2", &oracle.generation_2_identity, false)?;
        let upgraded = capture_expected(&server, g2_expected.clone())?;
        require_generation(&server, "g2")?;
        refuse(
            !advanced(&reinstalled, &upgraded),
            "upgrade revisions did not advance for every seed/view",
        )?;
        stage(&server, &mut report, "g2", &upgraded)?;
        owner.save(&report)?;
        report["upgrade_provider_state"] = state_json(&state(store.as_ref())?);
        owner.save(&report)?;
        owner.verify()?;
        let receipt = service
            .rollback_seed_generation("CARDDEMO", "g1")
            .map_err(terminal_problem)?;
        report["rollback_receipt"] =
            check_receipt(&receipt, "g1", &oracle.generation_1_identity, false)?;
        let rollback = capture_expected(&server, expected.clone())?;
        require_generation(&server, "g1")?;
        refuse(
            !advanced(&upgraded, &rollback),
            "rollback revisions did not advance for every seed/view",
        )?;
        stage(&server, &mut report, "rollback", &rollback)?;
        owner.save(&report)?;
        let before = state(store.as_ref())?;
        report["rollback_provider_state"] = state_json(&before);
        owner.save(&report)?;
        let mut corrupt = objects.clone();
        corrupt[0].sha256 = format!("sha256:{:064x}", 0);
        owner.verify()?;
        let result = service.install_seed_generation("CORRUPT", "g1", corrupt);
        report["corrupt_result"] = serde_json::json!(format!("{result:?}"));
        refuse(
            result != Err(HostProblem::IdempotencyConflict),
            "corrupt full seed input refused with wrong result",
        )?;
        let after = capture_expected(&server, expected.clone())?;
        require_generation(&server, "g1")?;
        refuse(
            before != state(store.as_ref())? || !same(&rollback, &after),
            "corrupt seed refusal changed retained complete state",
        )?;
        stage(&server, &mut report, "corrupt-refused", &after)?;
        owner.save(&report)?;
        report["corrupt_refusal_provider_state"] = state_json(&state(store.as_ref())?);
        owner.save(&report)?;
        owner.verify()?;
        let bounded_store = Arc::new(MemoryStore::new(Default::default()));
        let bounded = DatasetService::open(
            bounded_store.clone(),
            DatasetLimits {
                max_total_bytes: 427699,
                ..DatasetLimits::default()
            },
        )
        .map_err(terminal_problem)?;
        let result = bounded.install_seed_generation("BOUNDED", "g1", objects.clone());
        report["capacity_result"] = serde_json::json!(format!("{result:?}"));
        refuse(
            result != Err(HostProblem::ResourceExhausted)
                || state(bounded_store.as_ref())?
                    .values()
                    .any(|rows| !rows.is_empty()),
            "capacity refusal changed fresh provider state or returned wrong result",
        )?;
        let after = capture_expected(&server, expected.clone())?;
        refuse(
            before != state(store.as_ref())? || !same(&rollback, &after),
            "isolated capacity control changed real selected state",
        )?;
        stage(&server, &mut report, "capacity-refused", &after)?;
        owner.save(&report)?;
        report["capacity_refusal_provider_state"] = state_json(&state(store.as_ref())?);
        report["bounded_capacity_provider_state"] = state_json(&state(bounded_store.as_ref())?);
        owner.save(&report)?;
        let aliases = oracle
            .aliases
            .iter()
            .map(|row| {
                Ok((
                    row.name.clone(),
                    CicsFileDefinition {
                        dataset: DatasetName::new(&row.dataset, 128).map_err(package_problem)?,
                        ccsid: Some(37),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
        owner.verify()?;
        server
            .cics_service()
            .register_file_definitions(&aliases)
            .map_err(terminal_problem)?;
        for alias in &oracle.aliases {
            owner.verify()?;
            server
                .racf_service()
                .define_profile("DATASET", &alias.dataset, "IBMUSER", None)
                .map_err(terminal_problem)?;
            owner.verify()?;
            server
                .racf_service()
                .permit("DATASET", &alias.dataset, "IBMUSER", AccessIntent::Read)
                .map_err(terminal_problem)?;
        }
        check_alias_records(store.as_ref(), &oracle)?;
        report["alias_routes"] = alias_routes(&owner, &server, &oracle, "initial")?;
        let after = capture_expected(&server, expected.clone())?;
        refuse(
            !same(&rollback, &after),
            "read-only CICS alias routes changed full seed state",
        )?;
        stage(&server, &mut report, "aliases", &after)?;
        owner.save(&report)?;
        Ok::<_, CorpusProblem>((after, state(store.as_ref())?))
    })();
    let stopped = server.graceful_shutdown().await;
    report["shutdowns"] = serde_json::json!([stopped]);
    owner.save(&report)?;
    drop(server);
    drop(store);
    let result = match result {
        Err(error) => Err(error),
        Ok((before, rows)) if stopped => {
            owner.verify()?;
            let store = Arc::new(
                SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?,
            );
            let server = ProductServer::open(
                config,
                store.clone(),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .map_err(terminal_problem)?;
            let reopened = (|| {
                let after = capture_expected(&server, expected)?;
                require_generation(&server, "g1")?;
                refuse(
                    !same(&before, &after) || rows != state(store.as_ref())?,
                    "fresh SQLite/provider reopen changed exact data or revisions",
                )?;
                report["reopened_provider_state"] = state_json(&state(store.as_ref())?);
                check_alias_records(store.as_ref(), &oracle)?;
                report["reopened_alias_routes"] =
                    alias_routes(&owner, &server, &oracle, "reopened")?;
                let again = capture_expected(&server, after.expected.clone())?;
                refuse(
                    !same(&after, &again),
                    "reopened CICS browse changed complete raw state",
                )?;
                stage(&server, &mut report, "reopen", &again)?;
                Ok::<_, CorpusProblem>(())
            })();
            let stopped = server.graceful_shutdown().await;
            report["shutdowns"]
                .as_array_mut()
                .ok_or_else(|| drift("owned shutdown ledger invalid"))?
                .push(serde_json::json!(stopped));
            owner.save(&report)?;
            drop(server);
            drop(store);
            reopened.and_then(|_| refuse(!stopped, "reopened server closure uncertain"))
        }
        Ok(_) => Err(drift("initial server closure uncertain")),
    };
    if let Err(error) = result {
        report["status"] = serde_json::json!("fail-owned-state-retained");
        report["failure"] = serde_json::json!(error.to_string());
        owner.save(&report)?;
        return Err(error);
    }
    pinned(corpus)?;
    expected_rows(corpus)?;
    owner.cleanup(&mut report)?;
    let mut observations = RouteObservations::default();
    // The four complete predicates come from this fresh owner; no sidecar input.
    for requirement in [
        "13 base EBCDIC seeds import with exact record boundaries and digests",
        "runtime-oracle key offsets and lengths are checked",
        "CICS FILE names resolve to datasets and AIX paths",
        "reinstall, upgrade, rollback, corrupt input, and capacity cases are covered",
    ] {
        observations.compare(
            journey_closure::AuthorityKind::Issue,
            "CD-016",
            requirement,
            !finished(&report)
                || report["cleanup"] != serde_json::json!("removed-and-absence-verified"),
            || Ok(drift("complete source seed owner did not finish")),
        )?;
    }
    report["issue_observations"] = serde_json::json!(observations.issues());
    owner.save(&report)?;
    Ok(observations)
}

#[cfg(test)]
#[path = "seed_state_lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "financial_initial_lifecycle.rs"]
mod financial_initial;

#[cfg(test)]
#[path = "v3_retained_lifecycle.rs"]
mod v3_retained;
