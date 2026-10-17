//! Source-owned REPROCT controls. These checks grant no journey, issue or IBM credit.
//! Every input profile here is explicit; no JCL native encoding default is inferred.
use mainframe_env_batch::{
    AmsStatement, DdPlan, JclBundle, JclConversionLimits, JclLimits, JclSourceOriginKind,
    ProgramInput, ProgramOutput, ProgramRouter, convert_jcl, decode_program_output,
    parse_idcams_control, parse_jcl, validate_idcams_control,
};
use mainframe_env_dataset::{DatasetLimits, DatasetService};
use mainframe_env_encoding::{CodePage, EncodingProblem};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult,
    EffectRequest, HostProblem, HostProvider, HostRequest, HostResult, MemberName, Mutation,
    ProgramName, ProgramRequest, RecordFormat,
};
use mainframe_env_store::MemoryStore;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CORPUS_ENV: &str = "CARDDEMO_CORPUS_DIR";

const FIXTURE: &str = include_str!("j10_reproct_fixture.json");
const CONTROL: &str = "app/ctl/REPROCT.ctl";
const CONTROL_SHA: &str = "5c6b8f26aa53c23131265e715b53130a1a7afd88b5bd3319aeca91f3d1a55bf5";
const ROWS_SHA: &str = "d4be5d9e718500a6c3865f3c1857887ab2e024133019e38385ecd5290a381098";
const DATASET: &str = "AWS.M2.CARDDEMO.CNTL";
const MEMBER: &str = "REPROCT";
const SOURCE_LIMIT: usize = 262144;
type Checked<T> = Result<T, &'static str>;

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
fn fixture() -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(value["schema"], "private.carddemo.j10-source-controls@1");
    assert_eq!(
        value["corpus_commit"],
        "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
    );
    assert_eq!(value["source_pins"].as_object().unwrap().len(), 5);
    assert_eq!(value["control_source_sha256"], CONTROL_SHA);
    assert_eq!(value["raw_f80_sha256"], ROWS_SHA);
    assert_eq!(value["rows"].as_array().unwrap().len(), 15);
    value
}
fn directory_id(path: &Path, private: bool) -> Checked<(u64, u64)> {
    let stat = fs::symlink_metadata(path).map_err(|_| "directory is unavailable")?;
    if !stat.is_dir() || (private && stat.mode() & 0o777 != 0o700) {
        return Err("directory identity or private mode differs");
    }
    Ok((stat.dev(), stat.ino()))
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSeal {
    identity: (u64, u64),
    bytes: usize,
    sha256: String,
}
fn read_file(path: &Path, private: bool) -> Checked<(Vec<u8>, FileSeal)> {
    let before = fs::symlink_metadata(path).map_err(|_| "source file is unavailable")?;
    if !before.is_file()
        || before.nlink() != 1
        || before.len() > SOURCE_LIMIT as u64
        || (private && before.mode() & 0o777 != 0o600)
    {
        return Err("source file type, links, mode or bound differs");
    }
    let mut file = fs::File::open(path).map_err(|_| "source open failed")?;
    let opened = file
        .metadata()
        .map_err(|_| "opened source identity unavailable")?;
    let identity = (before.dev(), before.ino());
    if !opened.is_file() || (opened.dev(), opened.ino()) != identity {
        return Err("opened source identity differs");
    }
    let mut raw = Vec::new();
    Read::take(&mut file, (SOURCE_LIMIT + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| "bounded source read failed")?;
    let held = file
        .metadata()
        .map_err(|_| "held source identity unavailable")?;
    let after = fs::symlink_metadata(path).map_err(|_| "final source identity unavailable")?;
    if raw.len() > SOURCE_LIMIT
        || before.len() != raw.len() as u64
        || held.len() != raw.len() as u64
        || after.len() != raw.len() as u64
        || (held.dev(), held.ino()) != identity
        || (after.dev(), after.ino()) != identity
        || !after.is_file()
        || after.nlink() != 1
        || (private && after.mode() & 0o777 != 0o600)
    {
        return Err("source changed during bounded read");
    }
    let seal = FileSeal {
        identity,
        bytes: raw.len(),
        sha256: digest(&raw),
    };
    Ok((raw, seal))
}
fn pinned(relative: &str) -> Vec<u8> {
    let value = fixture();
    let expected = value["source_pins"][relative]
        .as_str()
        .expect("declared source pin");
    let root = PathBuf::from(env::var_os(CORPUS_ENV).expect("pinned corpus input required"));
    let identity = directory_id(&root, false).unwrap();
    let path = root.join(relative);
    assert!(
        path.canonicalize()
            .unwrap()
            .starts_with(root.canonicalize().unwrap())
    );
    let (raw, seal) = read_file(&path, false).unwrap();
    assert_eq!(seal.sha256, expected, "selected source pin differs");
    assert_eq!(directory_id(&root, false).unwrap(), identity);
    raw
}
fn f80(lines: &[Vec<u8>]) -> Checked<Vec<Vec<u8>>> {
    if lines.is_empty() || lines.len() > 4096 {
        return Err("ASCII F80 source record bound differs");
    }
    lines
        .iter()
        .map(|line| {
            if !line.is_ascii() || line.len() > 80 || line.contains(&b'\n') {
                return Err("ASCII F80 source record cannot be represented exactly");
            }
            let mut row = line.clone();
            row.resize(80, b' ');
            Ok(row)
        })
        .collect()
}
fn control_rows() -> Vec<Vec<u8>> {
    let source = pinned(CONTROL);
    let lines = source.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    let lines = if source.ends_with(b"\n") {
        &lines[..lines.len() - 1]
    } else {
        &lines
    };
    let rows = f80(&lines.iter().map(|line| line.to_vec()).collect::<Vec<_>>()).unwrap();
    let value = fixture();
    assert_eq!(rows.len(), 15);
    for (ordinal, (row, expected)) in rows
        .iter()
        .zip(value["rows"].as_array().unwrap())
        .enumerate()
    {
        assert_eq!(expected["ordinal"].as_u64(), Some(ordinal as u64));
        assert_eq!(expected["bytes"].as_u64(), Some(row.len() as u64));
        assert_eq!(expected["sha256"].as_str(), Some(digest(row).as_str()));
    }
    assert_eq!(digest(&rows.concat()), ROWS_SHA);
    rows
}
fn text_records(rows: &[Vec<u8>]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| row.iter().copied().chain(std::iter::once(b'\n')))
        .collect()
}
fn attributes() -> DatasetAttributes {
    DatasetAttributes {
        organization: DatasetOrganization::Partitioned,
        record_format: RecordFormat::Fixed,
        logical_record_length: 80,
        key_offset: None,
        key_length: None,
        ccsid: Some(1208),
    }
}
fn mutation(sequence: u64) -> Mutation {
    Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("j10-source-{sequence}"),
            InvocationLimits::default(),
        )
        .unwrap(),
        transaction: None,
    }
}
fn provider() -> (Arc<MemoryStore>, Arc<DatasetService>) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    (store, service)
}
fn create_member(service: &DatasetService, rows: Vec<Vec<u8>>) -> u64 {
    let dataset = DatasetName::new(DATASET, 128).unwrap();
    let DatasetResult::Created { version } = service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(),
            mutation: mutation(1),
        })
        .unwrap()
    else {
        panic!("unexpected create result")
    };
    let DatasetResult::Mutated { version: written } = service
        .invoke(DatasetRequest::Write {
            dataset,
            member: Some(MemberName::new(MEMBER, 128).unwrap()),
            records: rows,
            expected_version: Some(version),
            mutation: mutation(2),
        })
        .unwrap()
    else {
        panic!("unexpected member write result")
    };
    written
}
#[derive(Clone)]
struct MemberCapture {
    dataset: String,
    member: String,
    attributes: DatasetAttributes,
    before_version: u64,
    after_attributes: DatasetAttributes,
    after_version: u64,
    names: Vec<String>,
    more: bool,
    rows: Vec<Vec<u8>>,
    identities: Vec<Vec<u8>>,
    record_version: u64,
}
fn capture(service: &DatasetService) -> MemberCapture {
    let dataset = DatasetName::new(DATASET, 128).unwrap();
    let DatasetResult::Attributes {
        attributes,
        version,
    } = service
        .invoke(DatasetRequest::Attributes {
            dataset: dataset.clone(),
        })
        .unwrap()
    else {
        panic!("unexpected attributes result")
    };
    let DatasetResult::Members { names, more } = service
        .invoke(DatasetRequest::ListMembers {
            dataset: dataset.clone(),
            start: None,
            max_items: 4096,
        })
        .unwrap()
    else {
        panic!("unexpected member list result")
    };
    let DatasetResult::Records {
        records,
        identities,
        version: record_version,
    } = service
        .invoke(DatasetRequest::Read {
            dataset: dataset.clone(),
            member: Some(MemberName::new(MEMBER, 128).unwrap()),
            key: None,
            max_records: 4096,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("unexpected member read result")
    };
    let DatasetResult::Attributes {
        attributes: after_attributes,
        version: after_version,
    } = service
        .invoke(DatasetRequest::Attributes {
            dataset: dataset.clone(),
        })
        .unwrap()
    else {
        panic!("unexpected final attributes result")
    };
    MemberCapture {
        dataset: dataset.as_str().into(),
        member: MEMBER.into(),
        attributes,
        before_version: version,
        after_attributes,
        after_version,
        names: names.iter().map(|name| name.as_str().to_string()).collect(),
        more,
        rows: records,
        identities,
        record_version,
    }
}
fn complete(observed: &MemberCapture, expected: &[Vec<u8>]) -> bool {
    observed.dataset == DATASET
        && observed.member == MEMBER
        && observed.attributes == attributes()
        && observed.after_attributes == attributes()
        && observed.before_version > 0
        && observed.before_version == observed.after_version
        && observed.record_version == observed.before_version
        && observed.names == [MEMBER.to_string()]
        && !observed.more
        && observed.rows == expected
        && observed.rows.len() == 15
        && observed.identities
            == (0..15u64)
                .map(|ordinal| ordinal.to_be_bytes().to_vec())
                .collect::<Vec<_>>()
        && digest(&observed.rows.concat()) == ROWS_SHA
}
fn close_provider(store: Arc<MemoryStore>, service: Arc<DatasetService>) {
    assert_eq!(
        Arc::strong_count(&service),
        1,
        "unknown provider lease remains"
    );
    drop(service);
    assert_eq!(Arc::strong_count(&store), 1, "unknown store lease remains");
    drop(store);
}

// Private, serial filesystem observations. These are not an atomic race/CAS claim.
// No process, daemon, ProductServer or artifact worker is created by this owner.
struct Owner {
    parent: PathBuf,
    parent_id: (u64, u64),
    evidence: PathBuf,
    evidence_id: (u64, u64),
    owned: PathBuf,
    owned_id: (u64, u64),
    files: BTreeMap<String, FileSeal>,
}
impl Owner {
    fn claim(label: &str) -> Self {
        let parent = PathBuf::from(
            env::var_os("CARDDEMO_J10_CONTROLS_EVIDENCE")
                .expect("fresh private control evidence parent required"),
        );
        let parent_id = directory_id(&parent, true).unwrap();
        let evidence = parent.join(label);
        fs::DirBuilder::new().mode(0o700).create(&evidence).unwrap();
        assert_eq!(directory_id(&parent, true).unwrap(), parent_id);
        let evidence_id = directory_id(&evidence, true).unwrap();
        let owned = evidence.join("owned-state");
        fs::DirBuilder::new().mode(0o700).create(&owned).unwrap();
        let owned_id = directory_id(&owned, true).unwrap();
        Self {
            parent,
            parent_id,
            evidence,
            evidence_id,
            owned,
            owned_id,
            files: BTreeMap::new(),
        }
    }
    fn verify_root(&self) -> Checked<()> {
        if directory_id(&self.parent, true)? != self.parent_id
            || directory_id(&self.evidence, true)? != self.evidence_id
        {
            return Err("owned evidence root identity differs");
        }
        Ok(())
    }
    fn verify(&self) -> Checked<()> {
        self.verify_root()?;
        if directory_id(&self.owned, true)? != self.owned_id {
            return Err("owned state identity differs");
        }
        let names = fs::read_dir(&self.owned)
            .map_err(|_| "owned listing failed")?
            .map(|entry| {
                entry
                    .map_err(|_| "owned entry unavailable")
                    .and_then(|entry| {
                        entry
                            .file_name()
                            .into_string()
                            .map_err(|_| "owned name invalid")
                    })
            })
            .collect::<Checked<BTreeSet<_>>>()?;
        if names != self.files.keys().cloned().collect() {
            return Err("owned namespace contains unknown entries");
        }
        for (name, expected) in &self.files {
            if read_file(&self.owned.join(name), true)?.1 != *expected {
                return Err("owned source file identity or contents differ");
            }
        }
        Ok(())
    }
    fn write_source(&mut self, name: &str, raw: &[u8]) -> Checked<()> {
        self.verify()?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
            || raw.len() > SOURCE_LIMIT
        {
            return Err("owned source name or bound differs");
        }
        let path = self.owned.join(name);
        let mut options = fs::OpenOptions::new();
        options.write(true).mode(0o600);
        if self.files.contains_key(name) {
            options.truncate(true);
        } else {
            options.create_new(true);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| "owned source open failed")?;
        let opened = file
            .metadata()
            .map_err(|_| "owned opened identity unavailable")?;
        if let Some(expected) = self.files.get(name)
            && (opened.dev(), opened.ino()) != expected.identity
        {
            return Err("owned write opened a different file");
        }
        file.write_all(raw)
            .map_err(|_| "owned source write failed")?;
        file.sync_all().map_err(|_| "owned source sync failed")?;
        self.verify_root()?;
        if directory_id(&self.owned, true)? != self.owned_id {
            return Err("owned state changed before write admission");
        }
        let (retained, seal) = read_file(&path, true)?;
        if seal.identity != (opened.dev(), opened.ino()) || retained != raw {
            return Err("owned written source differs");
        }
        self.files.insert(name.into(), seal);
        self.verify()
    }
    fn replace_source(&mut self, name: &str, raw: &[u8]) -> Checked<()> {
        self.verify()?;
        if !self.files.contains_key(name) {
            return Err("replacement does not name an owned source");
        }
        fs::remove_file(self.owned.join(name)).map_err(|_| "owned source remove failed")?;
        self.files.remove(name);
        self.write_source(name, raw)
    }
    fn retain(&self, label: &str, value: &serde_json::Value) -> Checked<()> {
        self.verify_root()?;
        let path = self.evidence.join(format!("{label}.json"));
        let raw =
            serde_json::to_vec_pretty(value).map_err(|_| "retained control evidence is invalid")?;
        if raw.len() > 64 * 1024 * 1024 {
            return Err("retained control evidence exceeds original output bound");
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| "unknown receipt destination or receipt create failed")?;
        let opened = file
            .metadata()
            .map_err(|_| "retained opened identity unavailable")?;
        file.write_all(&raw)
            .map_err(|_| "retained control write failed")?;
        file.sync_all()
            .map_err(|_| "retained control sync failed")?;
        self.verify_root()?;
        let (saved, seal) = read_file(&path, true)?;
        if seal.identity != (opened.dev(), opened.ino()) || saved != raw {
            return Err("retained control evidence custody differs");
        }
        self.verify_root()
    }
    fn finish(&self, primary: &str) -> Checked<()> {
        self.verify()?;
        let metadata = serde_json::json!({"primary":primary,"status":"failure-retained-before-cleanup","source_sha256":CONTROL_SHA,"issue_observations":[],"journey_observations":[],"application_credit":0,"vendor_credit":0,"owned_identity":[self.owned_id.0,self.owned_id.1]});
        self.retain("before-cleanup", &metadata)?;
        self.verify()?;
        fs::remove_dir_all(&self.owned).map_err(|_| "owned removal failed")?;
        match fs::symlink_metadata(&self.owned) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("owned removal absence is unverified"),
        }
        self.retain("after-cleanup", &serde_json::json!({"primary":primary,"status":"control-only-cleanup-verified","removed":true,"issue_observations":[],"journey_observations":[],"application_credit":0,"vendor_credit":0}))
    }
}
fn pin_then_publish(
    owner: &Owner,
    path: &Path,
    expected: &str,
    publish: impl FnOnce(),
) -> Checked<()> {
    owner.verify()?;
    let (raw, seal) = read_file(path, true)?;
    if seal.sha256 != expected || digest(&raw) != expected {
        return Err("point-of-use source pin differs");
    }
    owner.verify()?;
    publish();
    Ok(())
}
fn binding(job: &str, procedure: &str) -> bool {
    let bundle = JclBundle {
        primary: job.into(),
        cataloged_procedures: BTreeMap::from([("REPROC".into(), procedure.into())]),
        symbols: BTreeMap::from([("SYSUID".into(), "IBMUSER".into())]),
        ..JclBundle::default()
    };
    let Ok(conversion) = convert_jcl(&bundle, JclConversionLimits::default()) else {
        return false;
    };
    if !conversion.is_valid() {
        return false;
    }
    let Ok(plan) = parse_jcl(&bundle, JclLimits::default()) else {
        return false;
    };
    let Some(step) = plan.steps.iter().find(|step| step.name == "STEP05R.PRC001") else {
        return false;
    };
    let dd = |name: &str| step.dds.iter().find(|dd| dd.name == name);
    let Some(sysin) = dd("SYSIN") else {
        return false;
    };
    let Some(input) = dd("FILEIN") else {
        return false;
    };
    let Some(output) = dd("FILEOUT") else {
        return false;
    };
    if step.program != "IDCAMS"
        || sysin.dataset.as_deref() != Some(DATASET)
        || sysin.member.as_deref() != Some(MEMBER)
        || input.dataset.as_deref() != Some("AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS")
        || output.dataset.as_deref() != Some("AWS.M2.CARDDEMO.TRANSACT.BKUP")
        || output.generation != Some(1)
    {
        return false;
    }
    let Some(document) = conversion.plan() else {
        return false;
    };
    ["STEP05R.PRC001.FILEIN", "STEP05R.PRC001.FILEOUT"]
        .iter()
        .all(|name| {
            document.statements().iter().any(|statement| {
                statement.name() == Some(*name)
                    && statement
                        .source()
                        .origins()
                        .iter()
                        .any(|origin| origin.kind() == JclSourceOriginKind::OverrideUse)
            })
        })
}
fn dd(name: &str, raw: &[u8]) -> DdPlan {
    DdPlan {
        name: name.into(),
        dataset: None,
        member: None,
        generation: None,
        organization: None,
        record_format: None,
        logical_record_length: None,
        ccsid: None,
        temporary: false,
        sysout: None,
        disposition: Vec::new(),
        inline_data: raw.to_vec(),
        concatenation: false,
        source_line: 1,
        source_end_line: 1,
        parameters: Vec::new(),
    }
}
fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("request", limits).unwrap(),
        ExecutionId::new("execution", limits).unwrap(),
        RunUnitId::new("run", limits).unwrap(),
        None,
        Selector::new("program:test", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("USER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new("key", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn route(program: &str, input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
    let limits = InvocationLimits::default();
    let invocation = invocation();
    let router = ProgramRouter::with_builtins(limits);
    let result = router.invoke(
        &invocation,
        EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 17,
            deadline_tick: 100,
            idempotency_key: None,
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new(program, 128).unwrap(),
                payload: BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    serde_json::to_vec(input).unwrap(),
                    limits,
                )
                .unwrap(),
                service: None,
            }),
        },
    );
    assert_eq!(result.sequence, 17);
    let HostResult::Program(payload) = result.outcome? else {
        panic!("unexpected program result")
    };
    decode_program_output(&payload)
}
fn copy(input: &ProgramInput) -> ProgramOutput {
    route("IEBGENER", input).unwrap()
}

#[path = "j10_source_controls_tests.rs"]
mod tests;
