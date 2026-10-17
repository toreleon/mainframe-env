//! Failure-only physical observations. These never supply application credit.

use super::{CorpusProblem, Path, ProductServer};
use mainframe_env_execution_api::{ExecutionId, InvocationLimits};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{
    AuditSink, ExecutionRecord, ExecutionStore, IdempotencyStore, ProviderStateRecord,
    ProviderStateStore, WorkStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const MAX_CALL_ROWS: usize = 2_048;
const MAX_CALL_BYTES: usize = 8 * 1024 * 1024;
const MAX_UNRESOLVED: usize = 1_024;
const MAX_AUDITS: usize = 16_384;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MANIFEST_RESERVE: usize = 512 * 1024;
const CALL_NAMESPACE: &str = "cobol-call-replay@1";
type Observed<T> = Result<T, &'static str>;

pub(super) fn retain(
    problem: CorpusProblem,
    server: &ProductServer,
    store: &MemoryStore,
    artifact_root: &Path,
) -> CorpusProblem {
    preserve_failure(problem, |problem| {
        capture(problem, server, store, artifact_root)
    })
}

fn preserve_failure(
    problem: CorpusProblem,
    observe: impl FnOnce(&CorpusProblem) -> Observed<()>,
) -> CorpusProblem {
    // Only intentional observer output is restricted to fixed status/error
    // codes. Rust's inherited panic hook may run before catch_unwind; it is not
    // suppressed or replaced while application workers run. Abort/OOM is outside
    // this Result/unwind boundary. Native stderr remains an external receipt.
    let (status, code) =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observe(&problem))) {
            Ok(Ok(())) => ("OBSERVATIONS_RETAINED_ZERO_CREDIT", None),
            Ok(Err(code)) => ("INCOMPLETE", Some(code)),
            Err(_) => ("OBSERVER_PANICKED_ORIGINAL_FAILURE_RETAINED", None),
        };
    // A closed or failed stderr sink cannot replace the original application
    // error. Do not hold this process-wide stream lock during store reads.
    let _ = write_status(&mut std::io::stderr().lock(), status, code);
    problem
}

fn write_status(writer: &mut impl Write, status: &str, code: Option<&str>) -> std::io::Result<()> {
    match code {
        Some(code) => writeln!(writer, "CARDDEMO_CREASTMT_POSTMORTEM {status} code={code}"),
        None => writeln!(writer, "CARDDEMO_CREASTMT_POSTMORTEM {status}"),
    }
}

fn capture(
    problem: &CorpusProblem,
    server: &ProductServer,
    store: &MemoryStore,
    artifact_root: &Path,
) -> Observed<()> {
    let mut output = Output::new(artifact_root)?;
    output.write_json("manifest-start.json", &json!({
        "schema":"mainframe-env.carddemo-creastmt-failure-observation@1",
        "status":"incomplete", "original_failure_retained":true, "credit":0,
        "qualification":"Separate read-only observations; no atomic whole-store snapshot or machine PC."
    }))?;
    let mut observations = json!({"credit":0,
        "capture_started_unix_ns":observed_at()});
    let result = capture_selected(problem, server, store, &mut output, &mut observations);
    let manifest = json!({
        "schema":"mainframe-env.carddemo-creastmt-failure-observation@1",
        "status":if result.is_ok() {"retained"} else {"incomplete"},
        "error_code":result.as_ref().err(), "original_failure_retained":true,
        "observations":observations, "files":output.files, "credit":0,
        "qualification":"Separate physical reads may observe later progress after the original120s failure. No machine instruction, speedup, cleanup or acceptance claim."
    });
    output.write_manifest(&manifest)?;
    result
}

fn capture_selected(
    problem: &CorpusProblem,
    server: &ProductServer,
    store: &MemoryStore,
    output: &mut Output,
    observations: &mut Value,
) -> Observed<()> {
    // Parse the actual already-decided utility failure, then verify the exact
    // job through Batch. A similarly named newest job can never substitute.
    let failed = failed_job(problem)?;
    let job = server
        .batch_service()
        .get(&failed.id)
        .map_err(|_| "job-read")?;
    if job.id != failed.id || job.name != "CREASTMT" || job.owner != failed.owner {
        return Err("failed-job-binding");
    }
    let work_id = format!("jes:{}", job.id);
    let work = store
        .get_work(&work_id)
        .map_err(|_| "work-read")?
        .ok_or("work-missing")?;
    if work.payload.len() > 16 * 1024 {
        return Err("work-payload-bound");
    }
    let payload: Value = serde_json::from_slice(&work.payload).map_err(|_| "work-payload")?;
    if work.work_id != work_id
        || payload["job_id"] != job.id
        || payload["owner"] != job.owner
        || payload["schema_version"] != "mainframe-env.jes-work@1"
        || work.required_generation != "mainframe-env-batch@1"
        || work.lease_epoch == 0
    {
        return Err("work-job-binding");
    }
    let run = format!("jes-run-{}-{}", job.id, work.lease_epoch);
    observations["work"] = json!({
        "job_id":job.id, "job_name":job.name, "owner":job.owner,
        "job_state":format!("{:?}",job.state), "active_step":job.active_step,
        "job_version":job.version, "work_id":work.work_id,
        "execution":work.execution_id.as_str(), "lease_epoch":work.lease_epoch,
        "read_completed_unix_ns":observed_at(),
        "work_effect_sequence_is_not_machine_progress":work.effect_sequence
    });
    let rows = store
        .list_provider_state_bounded(CALL_NAMESPACE, MAX_CALL_ROWS + 1, MAX_CALL_BYTES)
        .map_err(|_| "call-page-read-or-byte-bound")?;
    if rows.len() > MAX_CALL_ROWS {
        return Err("call-page-truncated");
    }
    observations["call_page"] = json!({"rows":rows.len(),"read_completed_unix_ns":observed_at()});
    let (root_row, root) = select_call(
        &rows,
        work.execution_id.as_str(),
        &run,
        &job.owner,
        "program:CBSTM03A",
        store,
        false,
    )?
    .ok_or("root-call-missing")?;
    output.write_row("root-call", root_row)?;
    observations["root"] = execution_view(&root);
    let mut actors = vec![root];
    if let Some((child_row, child)) = select_call(
        &rows,
        actors[0].execution_id.as_str(),
        &run,
        &job.owner,
        "program:CBSTM03B",
        store,
        true,
    )? {
        output.write_row("active-child-call", child_row)?;
        observations["active_child"] = execution_view(&child);
        actors.push(child);
    } else {
        observations["active_child"] = json!({"status":"no_pending_call_in_observed_page"});
    }
    let unresolved = store
        .unresolved_effects(MAX_UNRESOLVED + 1)
        .map_err(|_| "unresolved-read")?;
    if unresolved.len() > MAX_UNRESOLVED {
        return Err("unresolved-truncated");
    }
    if unresolved
        .iter()
        .filter(|effect| {
            actors
                .iter()
                .any(|actor| actor.execution_id == effect.execution_id)
        })
        .any(|effect| {
            effect.run_unit_id.as_str() != run || effect.intent.owner != effect.execution_id
        })
    {
        return Err("unresolved-actor-binding");
    }
    observations["unresolved"] = Value::Array(
        unresolved
            .iter()
            .filter(|effect| {
                actors
                    .iter()
                    .any(|actor| actor.execution_id == effect.execution_id)
            })
            .map(|effect| {
                json!({
                    "execution":effect.execution_id.as_str(), "run":effect.run_unit_id.as_str(),
                    "sequence":effect.sequence,"key":effect.key.as_str(),
                    "state":format!("{:?}",effect.state),
                    "capability":effect.intent.capability.as_ref().map(|value|value.as_str()),
                    "request_digest":effect.request_digest
                })
            })
            .collect(),
    );
    let mut audits = Vec::new();
    for actor in &actors {
        let rows = store
            .audit_records(&actor.execution_id, 0, MAX_AUDITS + 1)
            .map_err(|_| "audit-read")?;
        if rows.len() > MAX_AUDITS {
            return Err("audit-page-truncated-not-last");
        }
        if rows
            .iter()
            .any(|row| row.execution_id != actor.execution_id || row.run_unit_id.as_str() != run)
        {
            return Err("audit-actor-binding");
        }
        if rows
            .windows(2)
            .any(|pair| pair[0].effect_sequence > pair[1].effect_sequence)
        {
            return Err("audit-order-not-last");
        }
        let last = rows.last();
        audits.push(json!({"execution":actor.execution_id.as_str(),"rows":rows.len(),
            "read_completed_unix_ns":observed_at(),
            "last":last.map(|row|json!({"sequence":row.effect_sequence,"capability":row.capability.as_str(),
                "decision":format!("{:?}",row.decision),"observed_tick":row.observed_tick}))}));
    }
    observations["audits"] = json!(audits);
    for (label, dataset) in [
        ("statement-ps", "AWS.M2.CARDDEMO.STATEMNT.PS"),
        ("statement-html", "AWS.M2.CARDDEMO.STATEMNT.HTML"),
    ] {
        match store
            .get_provider_state("dataset", dataset)
            .map_err(|_| "dataset-row-read")?
        {
            Some(row) if row.namespace == "dataset" && row.key == dataset => {
                output.write_row(label, &row)?
            }
            Some(_) => return Err("dataset-row-binding"),
            None => {
                observations[label] = json!({"status":"absent"});
            }
        }
    }
    Ok(())
}

struct FailedJob {
    id: String,
    owner: String,
}
fn failed_job(problem: &CorpusProblem) -> Observed<FailedJob> {
    if problem.code != "carddemo.utility.job_failed" || problem.detail.len() > 64 * 1024 {
        return Err("failed-job-error-shape");
    }
    let detail = problem
        .detail
        .strip_prefix("utility job did not complete: ")
        .ok_or("failed-job-error-prefix")?;
    let mut stream = serde_json::Deserializer::from_str(detail).into_iter::<Value>();
    let value = stream
        .next()
        .ok_or("failed-job-json")?
        .map_err(|_| "failed-job-json")?;
    if !detail[stream.byte_offset()..].starts_with("; execution=")
        || value["jobname"] != "CREASTMT"
        || !matches!(value["status"].as_str(), Some("INPUT" | "ACTIVE"))
    {
        return Err("failed-job-json-binding");
    }
    let id = value["jobid"].as_str().ok_or("failed-job-id")?;
    if id.len() > 32
        || !id.strip_prefix("JOB").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err("failed-job-id");
    }
    let owner = value["owner"].as_str().ok_or("failed-job-owner")?;
    if owner.is_empty()
        || owner.len() > 8
        || !owner.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
        })
    {
        return Err("failed-job-owner");
    }
    Ok(FailedJob {
        id: id.into(),
        owner: owner.into(),
    })
}

fn select_call<'a>(
    rows: &'a [ProviderStateRecord],
    owner: &str,
    run: &str,
    principal: &str,
    selector: &str,
    store: &MemoryStore,
    pending_only: bool,
) -> Observed<Option<(&'a ProviderStateRecord, ExecutionRecord)>> {
    let mut selected = None;
    for row in rows {
        if row.namespace != CALL_NAMESPACE {
            return Err("call-namespace");
        }
        let value: Value = serde_json::from_slice(&row.payload).map_err(|_| "call-json")?;
        if value["owner_execution"] != owner {
            continue;
        }
        if pending_only && !value["reply"].is_null() {
            continue;
        }
        if value["replay_key"] != row.key
            || value["owner_run_unit"] != run
            || value["owner_principal"] != principal
            || !matches!(row.version, 1 | 2)
            || value["schema_version"] != 2
        {
            return Err("call-owner-binding");
        }
        let id = value["child_execution"].as_str().ok_or("call-child-id")?;
        let prefix = if selector == "program:CBSTM03A" {
            "batch-installed-execution"
        } else {
            "online-call-execution"
        };
        if row.key.len() != 64
            || !row
                .key
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || id != format!("{prefix}-{}", row.key)
        {
            return Err("call-child-physical-key-binding");
        }
        let id = ExecutionId::new(id, InvocationLimits::default()).map_err(|_| "call-child-id")?;
        let execution = store
            .get_execution(&id)
            .map_err(|_| "child-read")?
            .ok_or("child-missing")?;
        if execution.execution_id != id
            || execution.run_unit_id.as_str() != run
            || execution.principal.as_str() != principal
            || execution.selector.as_str() != selector
        {
            return Err("child-binding");
        }
        if selected.replace((row, execution)).is_some() {
            return Err("call-ambiguous");
        }
    }
    Ok(selected)
}

fn execution_view(value: &ExecutionRecord) -> Value {
    json!({"execution":value.execution_id.as_str(),"run":value.run_unit_id.as_str(),
        "selector":value.selector.as_str(),"artifact":value.artifact.as_str(),
        "state":format!("{:?}",value.state),"version":value.version,"attempt":value.attempt,
        "read_completed_unix_ns":observed_at(),"binding_is_physical_observation_not_CALL_codec_or_dispatch_authority":true})
}

fn observed_at() -> Option<String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_nanos().to_string())
}

struct Output {
    directory: PathBuf,
    bytes: usize,
    files: Vec<Value>,
}
impl Output {
    fn new(root: &Path) -> Observed<Self> {
        if root.canonicalize().map_err(|_| "artifact-root")? != root
            || !fs::symlink_metadata(root)
                .map_err(|_| "artifact-root")?
                .is_dir()
        {
            return Err("artifact-root");
        }
        let directory = root.join("creastmt-failure-observation");
        create_private(&directory)?;
        Ok(Self {
            directory,
            bytes: 0,
            files: Vec::new(),
        })
    }
    fn charge(&mut self, bytes: usize) -> Observed<()> {
        let next = self.bytes.checked_add(bytes).ok_or("output-byte-bound")?;
        if bytes > MAX_FILE_BYTES || next > MAX_OUTPUT_BYTES - MANIFEST_RESERVE {
            return Err("output-byte-bound");
        }
        self.bytes = next;
        Ok(())
    }
    fn write(&mut self, name: &str, bytes: &[u8]) -> Observed<()> {
        if name.len() > 128
            || !matches!(
                Path::new(name).components().collect::<Vec<_>>().as_slice(),
                [std::path::Component::Normal(_)]
            )
        {
            return Err("observation-file-name");
        }
        self.charge(bytes.len())?;
        write_new(&self.directory.join(name), bytes)?;
        self.files.push(
            json!({"name":name,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))}),
        );
        Ok(())
    }
    fn write_json(&mut self, name: &str, value: &Value) -> Observed<()> {
        self.write(
            name,
            &serde_json::to_vec(value).map_err(|_| "observation-encode")?,
        )
    }
    fn write_manifest(&mut self, value: &Value) -> Observed<()> {
        let bytes = serde_json::to_vec(value).map_err(|_| "manifest-encode")?;
        if bytes.len() > MANIFEST_RESERVE {
            return Err("manifest-byte-bound");
        }
        write_new(&self.directory.join("manifest-final.json"), &bytes)
    }
    fn write_row(&mut self, label: &str, row: &ProviderStateRecord) -> Observed<()> {
        if row.payload.len() > MAX_FILE_BYTES {
            return Err("row-byte-bound");
        }
        self.write(&format!("{label}.payload.bin"), &row.payload)?;
        self.write_json(&format!("{label}.row.json"),&json!({
            "namespace":row.namespace,"key":row.key,"version":row.version,
            "read_copy_written_unix_ns":observed_at(),
            "payload_bytes":row.payload.len(),"payload_sha256":format!("{:x}",Sha256::digest(&row.payload))
        }))
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> Observed<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|_| "observation-directory-create")
}
#[cfg(not(unix))]
fn create_private(_: &Path) -> Observed<()> {
    Err("unsupported-private-permissions")
}
fn write_new(path: &Path, bytes: &[u8]) -> Observed<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| "observation-file-create")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "observation-file-write")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{ArtifactRef, PrincipalId, RunUnitId, Selector};
    use mainframe_env_store_api::ExecutionState;

    #[test]
    fn observer_error_and_panic_return_the_exact_original_failure() {
        for panic in [false, true] {
            let original = CorpusProblem::new("original-failure", "retained exact detail");
            let code = original.code.as_ptr();
            let detail = original.detail.as_ptr();
            let returned = preserve_failure(original, |_| {
                if panic {
                    panic!("observer fixture panic")
                } else {
                    Err("injected-observer-error")
                }
            });
            assert_eq!(
                returned,
                CorpusProblem::new("original-failure", "retained exact detail")
            );
            assert_eq!(returned.code.as_ptr(), code);
            assert_eq!(returned.detail.as_ptr(), detail);
        }
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(write_status(&mut Closed, "INCOMPLETE", Some("fixture-code")).is_err());
    }

    fn failed(name: &str, status: &str, id: &str) -> CorpusProblem {
        CorpusProblem::new(
            "carddemo.utility.job_failed",
            format!(
                "utility job did not complete: {}; execution=Some(Ok(...)); checkpoint=None; spool={{}}",
                json!({"jobname":name,"status":status,"jobid":id,"owner":"IBMUSER"})
            ),
        )
    }

    #[test]
    fn failed_job_selection_requires_the_actual_utility_failure_json() {
        let good = failed("CREASTMT", "ACTIVE", "JOB00021");
        let selected = failed_job(&good).unwrap();
        assert_eq!(selected.id, "JOB00021");
        assert_eq!(selected.owner, "IBMUSER");
        for wrong in [
            failed("OTHER", "ACTIVE", "JOB00021"),
            failed("CREASTMT", "OUTPUT", "JOB00021"),
            failed("CREASTMT", "ACTIVE", "JOB00021/other"),
            CorpusProblem::new("another-error", good.detail.clone()),
            CorpusProblem::new(
                &good.code,
                good.detail.replace("; execution=", "; different="),
            ),
        ] {
            assert!(failed_job(&wrong).is_err());
        }
    }

    #[test]
    fn observer_output_bounds_refuse_before_changing_the_charge() {
        let mut output = Output {
            directory: PathBuf::new(),
            bytes: 0,
            files: Vec::new(),
        };
        assert_eq!(output.charge(MAX_FILE_BYTES + 1), Err("output-byte-bound"));
        assert_eq!(output.bytes, 0);
        output.bytes = MAX_OUTPUT_BYTES - MANIFEST_RESERVE - 1;
        assert_eq!(output.charge(2), Err("output-byte-bound"));
        assert_eq!(output.bytes, MAX_OUTPUT_BYTES - MANIFEST_RESERVE - 1);
        assert_eq!(output.charge(1), Ok(()));
        assert_eq!(output.bytes, MAX_OUTPUT_BYTES - MANIFEST_RESERVE);
        assert_eq!(output.charge(1), Err("output-byte-bound"));
    }

    fn row_and_execution() -> (ProviderStateRecord, ExecutionRecord) {
        let limits = InvocationLimits::default();
        let key = "a".repeat(64);
        let child = format!("batch-installed-execution-{key}");
        (ProviderStateRecord{namespace:CALL_NAMESPACE.into(),key:key.clone(),version:1,
            payload:serde_json::to_vec(&json!({"schema_version":2,"owner_execution":"jes-owner","owner_run_unit":"jes-run-JOB00021-1","owner_principal":"IBMUSER","replay_key":key,"child_execution":child,"reply":null})).unwrap()},
         ExecutionRecord{execution_id:ExecutionId::new(child,limits).unwrap(),run_unit_id:RunUnitId::new("jes-run-JOB00021-1",limits).unwrap(),selector:Selector::new("program:CBSTM03A",limits).unwrap(),artifact:ArtifactRef::new("artifact",limits).unwrap(),principal:PrincipalId::new("IBMUSER",limits).unwrap(),state:ExecutionState::Admitted,attempt:1,version:1,owner_lease:None,lease_expiry_tick:None,terminal_tick:None})
    }

    #[test]
    fn physical_call_selection_rejects_wrong_key_run_principal_and_ambiguity() {
        let store = MemoryStore::new(Default::default());
        let (row, execution) = row_and_execution();
        store.create_execution(execution.clone()).unwrap();
        let rows = vec![row.clone()];
        let select = |rows: &[ProviderStateRecord], owner: &str, run: &str, principal: &str| {
            select_call(
                rows,
                owner,
                run,
                principal,
                "program:CBSTM03A",
                &store,
                false,
            )
            .map(|value| value.map(|(_, execution)| execution))
        };
        assert_eq!(
            select(&rows, "jes-owner", "jes-run-JOB00021-1", "IBMUSER"),
            Ok(Some(execution))
        );
        assert_eq!(
            select(&rows, "another-owner", "jes-run-JOB00021-1", "IBMUSER"),
            Ok(None)
        );
        assert!(select(&rows, "jes-owner", "another-run", "IBMUSER").is_err());
        assert!(select(&rows, "jes-owner", "jes-run-JOB00021-1", "OTHER").is_err());
        let mut wrong = row.clone();
        wrong.key = "b".repeat(64);
        assert!(select(&[wrong], "jes-owner", "jes-run-JOB00021-1", "IBMUSER").is_err());
        assert!(
            select(
                &[row.clone(), row],
                "jes-owner",
                "jes-run-JOB00021-1",
                "IBMUSER"
            )
            .is_err()
        );
    }

    #[test]
    fn physical_call_selection_requires_an_actual_matching_child_execution() {
        let store = MemoryStore::new(Default::default());
        let (row, mut execution) = row_and_execution();
        assert!(
            select_call(
                std::slice::from_ref(&row),
                "jes-owner",
                "jes-run-JOB00021-1",
                "IBMUSER",
                "program:CBSTM03A",
                &store,
                false
            )
            .is_err()
        );
        execution.selector = Selector::new("program:OTHER", InvocationLimits::default()).unwrap();
        store.create_execution(execution).unwrap();
        assert!(
            select_call(
                &[row],
                "jes-owner",
                "jes-run-JOB00021-1",
                "IBMUSER",
                "program:CBSTM03A",
                &store,
                false
            )
            .is_err()
        );
    }

    #[test]
    #[cfg(unix)]
    fn private_observation_files_never_overwrite_existing_paths() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-creastmt-observer-test-{}-{}",
            std::process::id(),
            observed_at().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let mut output = Output::new(&root).unwrap();
        assert_eq!(
            fs::metadata(&output.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        output.write("fixture.bin", b"retained").unwrap();
        assert!(output.write("fixture.bin", b"replacement").is_err());
        let file = output.directory.join("fixture.bin");
        assert_eq!(fs::read(&file).unwrap(), b"retained");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(file).unwrap();
        fs::remove_dir(output.directory).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
