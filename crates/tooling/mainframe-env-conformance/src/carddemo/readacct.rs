//! Reproducer for CBACT01C's READACCT output through the public job route.

use super::*;
use mainframe_env_batch::{StepState, StepTermination};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const DATE: &str = "2022070600000000+0000";

const OUTPUTS: [(&str, u32, RecordFormat, usize); 3] = [
    (
        "AWS.M2.CARDDEMO.ACCTDATA.PSCOMP",
        107,
        RecordFormat::FixedBlocked,
        1,
    ),
    (
        "AWS.M2.CARDDEMO.ACCTDATA.ARRYPS",
        110,
        RecordFormat::FixedBlocked,
        1,
    ),
    (
        "AWS.M2.CARDDEMO.ACCTDATA.VBPS",
        84,
        RecordFormat::VariableBlocked,
        2,
    ),
];

/// Run the pinned READACCT job and assert its observations.
pub fn verify_carddemo_readacct_from_env(inventory_path: &Path) -> Result<(), CorpusProblem> {
    capture_carddemo_readacct_from_env(inventory_path).map(|_| ())
}

/// Capture the self-recorded logical bundle for the pinned job.
pub fn capture_carddemo_readacct_from_env(inventory_path: &Path) -> Result<Value, CorpusProblem> {
    run_readacct(inventory_path, DATE, 0)
}

fn run_readacct(
    inventory_path: &Path,
    cobol_date: &str,
    wall_offset_micros: u64,
) -> Result<Value, CorpusProblem> {
    verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let bundles = explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(path, _)| path == "app/cbl/CBACT01C.cbl")
        .map(|(path, bundle)| ("CBACT01C".to_string(), (path, bundle)))
        .collect::<BTreeMap<_, _>>();
    let source_digests = bundles["CBACT01C"]
        .1
        .files()
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_string(),
                sha256_prefixed(file.bytes()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let definitions =
        compile_carddemo_batch_definitions(&bundles, &BTreeSet::from(["CBACT01C".to_string()]))?;
    let installed_artifacts = definitions
        .iter()
        .map(|definition| {
            json!({
                "name": definition.name, "artifact": definition.artifact.as_str(),
                "semantic_identity": definition.semantic_identity,
            })
        })
        .collect::<Vec<_>>();
    let jcl_bytes = read_corpus_file(&corpus_dir, &corpus_dir.join("app/jcl/READACCT.jcl"))?;
    let jcl = String::from_utf8(jcl_bytes.clone())
        .map_err(|error| CorpusProblem::new("carddemo.readacct.jcl_invalid", error.to_string()))?;
    let account = carddemo_base_seed_objects(&corpus_dir)?
        .into_iter()
        .find(|object| object.source_id == "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCDATA.PS")
        .ok_or_else(|| {
            CorpusProblem::new("carddemo.readacct.seed_missing", "account seed missing")
        })?;
    let (chunks, remainder) = account.bytes.as_chunks::<300>();
    let input = chunks
        .iter()
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    if input.is_empty() || !remainder.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.readacct.seed_invalid",
            "account record framing",
        ));
    }
    let initial = json!({
        "catalog": [account.dataset.as_str()], "dataset": account.dataset.as_str(),
        "attributes": account.attributes, "source": account.source_id,
        "source_digest": account.sha256, "record_count": input.len(),
        "records_digest": records_digest(&input), "records_base64": encode_records(&input),
    });
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let inventory: Value =
        serde_json::from_slice(&fs::read(inventory_path).map_err(|error| {
            CorpusProblem::new("carddemo.readacct.inventory", error.to_string())
        })?)
        .map_err(|error| CorpusProblem::new("carddemo.readacct.inventory", error.to_string()))?;
    let helper_sources = [
        "crates/apps/mainframe-env-server/src/cobol.rs",
        "crates/apps/mainframe-env-server/src/cobol/runtime.rs",
    ]
    .into_iter()
    .map(|path| {
        fs::read(repo.join(path))
            .map(|bytes| (path.to_string(), sha256_prefixed(&bytes)))
            .map_err(|error| {
                CorpusProblem::new("carddemo.readacct.helper_source", error.to_string())
            })
    })
    .collect::<Result<BTreeMap<_, _>, _>>()?;
    let toolchain = fs::read(repo.join("rust-toolchain.toml"))
        .map_err(|error| CorpusProblem::new("carddemo.readacct.toolchain", error.to_string()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.readacct.runtime", error.to_string()))?;
    let started = wall_micros()?.saturating_add(wall_offset_micros);
    let (job, outputs, spool, final_catalog) = runtime.block_on(async move {
        let config = ServerConfig {
            store_profile: StoreProfile::Memory,
            cobol_current_date: Some(cobol_date.into()),
            artifact_root: env::temp_dir().join(format!("mainframe-env-readacct-{}", std::process::id())),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        };
        let server = ProductServer::open(
            config,
            Arc::new(MemoryStore::new(Default::default())),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .map_err(terminal_problem)?;
        server.bootstrap_user("IBMUSER", b"TESTPASS").map_err(terminal_problem)?;
        server.install_batch_programs(definitions).map_err(terminal_problem)?;
        server
            .dataset_service()
            .install_seed_generation("READACCT", "g1", vec![account])
            .map_err(terminal_problem)?;
        let catalog = |server: &ProductServer| -> Result<Vec<String>, CorpusProblem> {
            let DatasetResult::Listed { names, more } = server.dataset_service()
                .invoke(DatasetRequest::List {
                    pattern: "AWS.M2.CARDDEMO.ACCTDATA.*".into(),
                    start: None, max_items: 16,
                }).map_err(terminal_problem)? else {
                return Err(CorpusProblem::new("carddemo.readacct.catalog", "list result invalid"));
            };
            if more { return Err(CorpusProblem::new("carddemo.readacct.catalog", "list truncated")); }
            Ok(names.into_iter().map(|name| name.as_str().to_string()).collect())
        };
        if catalog(&server)? != vec!["AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS".to_string()] {
            return Err(CorpusProblem::new("carddemo.readacct.catalog", "initial catalog differs"));
        }
        let racf = server.racf_service();
        racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
            .map_err(terminal_problem)?;
        racf.permit("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", AccessIntent::Alter)
            .map_err(terminal_problem)?;
        let app = server.router();
        let job_id = submit_job_with_retcode(&server, &app, &jcl, "CC 0000").await?;
        let job = server.batch_service().get(&job_id).map_err(terminal_problem)?;
        if job.state != JobState::Completed || job.return_code != Some(0)
            || job.steps.len() != 2
            || job.steps.iter().any(|step| {
                step.state != StepState::Completed
                    || step.termination != Some(StepTermination::ReturnCode { code: 0 })
            })
        {
            return Err(CorpusProblem::new(
                "carddemo.readacct.step_drift",
                format!("state={:?} rc={:?} steps={:?}", job.state, job.return_code, job.steps),
            ));
        }
        let spool = base_batch_spool_text(&server, &job_id)?;
        if !spool.contains("START OF EXECUTION OF PROGRAM CBACT01C")
            || !spool.contains("END OF EXECUTION OF PROGRAM CBACT01C")
            || !spool.contains("ACCT-ID                 :00000000042")
            || spool.contains("ERROR READING ACCOUNT FILE")
        {
            return Err(CorpusProblem::new("carddemo.readacct.spool_drift", spool));
        }
        let mut outputs = Vec::new();
        for (name, length, format, multiplier) in OUTPUTS {
            let dataset = DatasetName::new(name, 128).map_err(|_| {
                CorpusProblem::new("carddemo.readacct.dataset_invalid", name)
            })?;
            let DatasetResult::Attributes { attributes, .. } = server
                .dataset_service()
                .invoke(DatasetRequest::Attributes { dataset })
                .map_err(terminal_problem)? else {
                    return Err(CorpusProblem::new("carddemo.readacct.dataset_invalid", name));
                };
            if attributes.organization != DatasetOrganization::Sequential
                || attributes.record_format != format
                || attributes.logical_record_length != length
            {
                return Err(CorpusProblem::new(
                    "carddemo.readacct.attributes_drift",
                    format!("{name}: {attributes:?}"),
                ));
            }
            let records = utility_records(&server, name, None)?;
            if records.len() != input.len() * multiplier
                || records.iter().any(|record| {
                    record.len() > length as usize
                        || (format == RecordFormat::FixedBlocked && record.len() != length as usize)
                })
            {
                return Err(CorpusProblem::new(
                    "carddemo.readacct.record_drift",
                    format!("{name}: {} records for {} inputs", records.len(), input.len()),
                ));
            }
            for (index, source) in input.iter().enumerate() {
                let record = &records[index * multiplier];
                if record.get(..11) != source.get(..11)
                    || (name.ends_with("PSCOMP") && record.get(11..24) != source.get(11..24))
                    || (name.ends_with("ARRYPS") && record.get(11..23) != source.get(12..24))
                {
                    return Err(CorpusProblem::new(
                        "carddemo.readacct.field_drift",
                        format!("{name} account {index} differs from CBACT01C's MOVEs"),
                    ));
                }
                if name.ends_with("PSCOMP") {
                    let formatted = [&source[68..72], &source[73..75], &source[76..78]].concat();
                    if record.get(68..76) != Some(formatted.as_slice()) {
                        return Err(CorpusProblem::new(
                            "carddemo.readacct.cobdatft_drift",
                            format!("account {index} formatted reissue date differs"),
                        ));
                    }
                }
                if multiplier == 2
                    && (record.len() != 12
                        || records[index * 2 + 1].len() != 39
                        || records[index * 2 + 1].get(..11) != source.get(..11)
                        || records[index * 2 + 1].get(35..39) != source.get(68..72)
                        || record.get(11..12) != source.get(11..12))
                {
                    return Err(CorpusProblem::new(
                        "carddemo.readacct.vb_length_drift",
                        format!("VBPS account {index}: expected 12/39 byte pair and source ID/status"),
                    ));
                }
            }
            outputs.push(json!({
                "name": name, "attributes": attributes, "record_count": records.len(),
                "records_digest": records_digest(&records),
                "records_base64": encode_records(&records),
            }));
        }
        let final_catalog = catalog(&server)?;
        let expected_catalog = ["AWS.M2.CARDDEMO.ACCTDATA.ARRYPS",
            "AWS.M2.CARDDEMO.ACCTDATA.PSCOMP", "AWS.M2.CARDDEMO.ACCTDATA.VBPS",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS"];
        if final_catalog.iter().map(String::as_str).collect::<Vec<_>>() != expected_catalog {
            return Err(CorpusProblem::new("carddemo.readacct.catalog", format!(
                "final catalog differs: {final_catalog:?}")));
        }
        let invocation = base_batch_control_invocation()?;
        let spool = server.batch_service().spool_files(&invocation, &job_id)
            .map_err(terminal_problem)?
            .into_iter()
            .map(|(_, name, count, _)| {
                let (records, more) = server.batch_service()
                    .spool(&invocation, &job_id, &name, 0, count.max(1))
                    .map_err(terminal_problem)?;
                if more || records.len() != count {
                    return Err(CorpusProblem::new("carddemo.readacct.spool_drift", name));
                }
                Ok(json!({"name": name, "record_count": records.len(),
                    "records_digest": records_digest(&records),
                    "records_base64": encode_records(&records),
                    "records": records.iter().map(|record| String::from_utf8_lossy(record).into_owned())
                        .collect::<Vec<_>>() }))
            })
            .collect::<Result<Vec<_>, CorpusProblem>>()?;
        Ok::<_, CorpusProblem>((json!({
            "name": job.name, "state": job.state, "return_code": job.return_code,
            "abend_code": job.abend_code,
            "steps": job.steps.iter().map(|step| json!({
                "name": step.name, "state": step.state, "termination": step.termination,
            })).collect::<Vec<_>>(),
        }), outputs, spool, final_catalog))
    })?;
    let finished = wall_micros()?.saturating_add(wall_offset_micros);
    let logical = json!({
        "corpus": {"commit": inventory["commit"], "tree": inventory["tree"]},
        "sources": {"jcl": {"path": "app/jcl/READACCT.jcl", "sha256": sha256_prefixed(&jcl_bytes)},
            "bundle_files": source_digests, "helper_sources": helper_sources},
        "compiler": {"target": "reference", "mode": "executable", "options": {},
            "toolchain": "1.98.0", "toolchain_file_digest": sha256_prefixed(&toolchain),
            "installed_artifacts": installed_artifacts},
        "deterministic_inputs": {"cobol_current_date": cobol_date, "display_ccsid": 37,
            "locale": "C", "timezone": "UTC", "random_seed": null,
            "limits": {"jcl_region": "8M", "dataset_read_max_records": 4096}},
        "initial_state": initial,
        "effects": {"capture_level": "job-step-spool-dataset-observations",
            "not_captured": ["dataset open/read/write/create/delete effect stream",
                "program call and COBDATFT effect stream", "spool write effect stream"],
            "observations": [
                {"kind": "seed-install", "dataset": "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS"},
                {"kind": "job-step", "step": "PREDEL", "program": "IEFBR14"},
                {"kind": "job-step", "step": "STEP05", "program": "CBACT01C"},
                {"kind": "outputs-observed"}]},
        "job": job, "spool": spool, "outputs": outputs, "final_catalog": final_catalog,
    });
    let replay_digest = sha256_prefixed(&serde_json::to_vec(&logical).map_err(|error| {
        CorpusProblem::new("carddemo.readacct.serialization", error.to_string())
    })?);
    Ok(
        json!({"schema_version": "mainframe-env.run-bundle@1", "provenance": "self-recorded",
        "authority": "development-only", "logical": logical, "replay_digest": replay_digest,
        "wall_clock": {"started_epoch_micros": started, "finished_epoch_micros": finished,
            "duration_micros": finished.saturating_sub(started)}}),
    )
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn records_digest(records: &[Vec<u8>]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        digest_field(&mut hasher, record);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn encode_records(records: &[Vec<u8>]) -> Vec<String> {
    records
        .iter()
        .map(|record| base64::engine::general_purpose::STANDARD.encode(record))
        .collect()
}

fn wall_micros() -> Result<u64, CorpusProblem> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros() as u64)
        .map_err(|error| CorpusProblem::new("carddemo.readacct.wall_clock", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/0.1.1/inventory/carddemo-corpus.json")
    }

    #[test]
    fn readacct_vb_lengths_follow_depending_on() {
        verify_carddemo_readacct_from_env(&inventory()).unwrap();
    }

    #[test]
    fn readacct_display_literal_is_written_to_spool() {
        let bundle = capture_carddemo_readacct_from_env(&inventory()).unwrap();
        assert!(
            bundle["logical"]["spool"]
                .to_string()
                .contains("ACCT-ID                 :00000000042")
        );
    }

    #[test]
    fn readacct_bundle_replay_digest_stable() {
        let first = run_readacct(&inventory(), DATE, 0).unwrap();
        let second = run_readacct(&inventory(), DATE, 1_000_000).unwrap();
        assert_eq!(first["replay_digest"], second["replay_digest"]);
        assert_ne!(first["wall_clock"], second["wall_clock"]);
    }

    #[test]
    fn readacct_bundle_logical_input_changes_digest() {
        let first = run_readacct(&inventory(), DATE, 0).unwrap();
        let changed = run_readacct(&inventory(), "2022070700000000+0000", 0).unwrap();
        assert_ne!(first["replay_digest"], changed["replay_digest"]);
    }
}
