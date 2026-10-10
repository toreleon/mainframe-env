//! First SQLite initialization diagnostic; no repeated Memory/full-batch owner.
//! Only bounded source/record/key hashes and versions are retained, never raw data.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const INPUTS: &str = include_str!("financial_initial_expected.json");
const CLOCK: &str = "2022070600000000+0000";
const JOBS: &[&str] = &[
    "app/jcl/ACCTFILE.jcl",
    "app/jcl/CARDFILE.jcl",
    "app/jcl/CUSTFILE.jcl",
    "app/jcl/XREFFILE.jcl",
    "app/jcl/TCATBALF.jcl",
    "app/jcl/TRANCATG.jcl",
    "app/jcl/TRANTYPE.jcl",
    "app/jcl/DISCGRP.jcl",
    "app/jcl/DUSRSECJ.jcl",
    "app/jcl/TRANFILE.jcl",
    "app/jcl/DEFGDGB.jcl",
    "app/jcl/DALYREJS.jcl",
];
fn offered(corpus: &Path) -> Result<(), CorpusProblem> {
    let frozen: serde_json::Value = serde_json::from_str(INPUTS).map_err(package_problem)?;
    refuse(
        frozen["schema"] != "private.carddemo.financial-initial-inputs@1"
            || frozen["corpus_commit"] != "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            || format!("{:x}", Sha256::digest(INPUTS.as_bytes()))
                != "ba9f9770e65b5e22a8e509858b017857d4cae6f4e9e7c98ec00aaa6c1a21b348",
        "financial initial frozen input envelope differs",
    )?;
    let pins = frozen["source_pins"]
        .as_object()
        .ok_or_else(|| drift("financial initial source pins missing"))?;
    refuse(
        pins.len() != 204,
        "complete financial initialization source pin set differs",
    )?;
    for (path, pin) in pins {
        let raw = read_corpus_file(corpus, &corpus.join(path))?;
        refuse(
            Some(raw.len() as u64) != pin["bytes"].as_u64()
                || format!("{:x}", Sha256::digest(raw)) != pin["sha256"],
            "financial initialization exact source input differs",
        )?;
    }
    source_artifact_binding::verify_pinned_sources(corpus)
}
fn physical(owner: &Owner, path: &Path, identity: (u64, u64)) -> Result<(), CorpusProblem> {
    owner.verify()?;
    refuse(
        directory_id(path)? != identity,
        "financial initialization physical namespace changed",
    )
}
fn dataset_state(store: &dyn ProviderStateStore) -> Result<serde_json::Value, CorpusProblem> {
    let rows = store
        .list_provider_state("dataset", 4097)
        .map_err(package_problem)?;
    refuse(
        rows.len() > 4096
            || rows.iter().any(|row| {
                row.namespace != "dataset"
                    || row.version == 0
                    || row.payload.len() > 64 * 1024 * 1024
            })
            || rows
                .iter()
                .map(|row| &row.key)
                .collect::<BTreeSet<_>>()
                .len()
                != rows.len(),
        "actual financial initialization dataset state bounds/identity differ",
    )?;
    Ok(serde_json::json!(rows.iter().map(|row|serde_json::json!({"dataset":row.key,"version":row.version,"bytes":row.payload.len(),"sha256":format!("{:x}",Sha256::digest(&row.payload))})).collect::<Vec<_>>()))
}
fn complete_datasets(rows: &serde_json::Value) -> bool {
    let Ok(frozen) = serde_json::from_str::<serde_json::Value>(INPUTS) else {
        return false;
    };
    let Some(expected) = frozen["initial_expected"].as_array() else {
        return false;
    };
    let Some(rows) = rows.as_array() else {
        return false;
    };
    rows.len() == 9
        && rows.len() == expected.len()
        && rows.iter().zip(expected).all(|(row, pin)| {
            row["dataset"] == pin["dataset"]
                && row["attributes"] == pin["attributes"]
                && row["records"] == pin["records"]
                && row["framed_sha256"] == pin["framed_sha256"]
                && row["version"].as_u64().is_some_and(|v| v > 0)
        })
}
fn complete_pages(report: &serde_json::Value) -> bool {
    let Ok(frozen) = serde_json::from_str::<serde_json::Value>(INPUTS) else {
        return false;
    };
    let Some(expected) = frozen["initial_expected"].as_array() else {
        return false;
    };
    let Some(rows) = report["pages"].as_array() else {
        return false;
    };
    if rows.len() != 72 {
        return false;
    }
    for (index, row) in rows.iter().enumerate() {
        let phase = index / 36;
        let dataset = (index % 36) / 4;
        let operation = index % 4;
        let pin = &expected[dataset];
        if row["owner_phase"] != if phase == 0 { "initial" } else { "reopen" }
            || row["phase"] != "initial"
            || row["dataset"] != pin["dataset"]
            || row["expected_records"] != pin["records"]
            || row["expected_width"] != pin["attributes"]["logical_record_length"]
            || row["expected_framed_sha256"] != pin["framed_sha256"]
        {
            return false;
        }
        let actual = &row["actual"];
        let version = &report["phases"][phase]["datasets"][dataset]["version"];
        let valid = match operation {
            0 | 3 => {
                actual["operation"] == "attributes"
                    && actual["attributes"] == pin["attributes"]
                    && &actual["version"] == version
            }
            1 => {
                actual["operation"] == "page"
                    && actual["start"] == 0
                    && actual["max_records"] == 4096
                    && actual["reverse"] == false
                    && actual["actual_records"] == pin["records"]
                    && actual["actual_identities"] == pin["records"]
                    && &actual["version"] == version
                    && actual["rows"] == pin["record_metadata"]
            }
            _ => {
                actual["operation"] == "condition"
                    && actual["condition"] == "NOTFND"
                    && actual["response"] == 13
                    && actual["response2"] == 0
            }
        };
        if !valid {
            return false;
        }
    }
    true
}
fn finished(report: &serde_json::Value) -> bool {
    let Some(jobs) = report["initialization_jobs"].as_array() else {
        return false;
    };
    if jobs.len() != 12
        || jobs.iter().zip(JOBS).any(|(row, path)| {
            row["source"] != *path || row["state"] != "Completed" || row["return_code"] != 0
        })
    {
        return false;
    }
    let Some(phases) = report["phases"].as_array() else {
        return false;
    };
    if phases.len() != 2
        || phases[0]["phase"] != "initial"
        || phases[1]["phase"] != "reopen"
        || phases[0]["datasets"] != phases[1]["datasets"]
        || phases[0]["dataset_provider_state"] != phases[1]["dataset_provider_state"]
        || !complete_datasets(&phases[0]["datasets"])
        || !complete_datasets(&phases[1]["datasets"])
    {
        return false;
    }
    report["shutdowns"] == serde_json::json!([true, true])
        && complete_pages(report)
        && report["installed_artifact_source_binding"] == true
        && report["reopened_artifact_source_binding"] == true
        && report["issue_observations"] == serde_json::json!([])
}
fn capture(
    owner: &Owner,
    path: &Path,
    identity: (u64, u64),
    report: &mut serde_json::Value,
    server: &ProductServer,
    stage: &str,
) -> Result<serde_json::Value, CorpusProblem> {
    super::super::super::financial_observations::capture_initial_diagnostic(
        |request| {
            physical(owner, path, identity).map_err(|_| HostProblem::InfrastructureFailure)?;
            server.dataset_service().invoke(request)
        },
        |mut row| {
            owner.verify()?;
            row["owner_phase"] = serde_json::json!(stage);
            let pages = report["pages"]
                .as_array_mut()
                .ok_or_else(|| drift("financial initialization page ledger invalid"))?;
            refuse(
                pages.len() >= 72,
                "financial initialization exact page/attribute/EOF ledger bound exceeded",
            )?;
            pages.push(row);
            owner.save(report)
        },
    )
}
async fn drain(
    owner: &Owner,
    report: &mut serde_json::Value,
    server: Arc<ProductServer>,
) -> Result<bool, CorpusProblem> {
    let stopped = server.graceful_shutdown().await;
    drop(server);
    report["shutdowns"]
        .as_array_mut()
        .ok_or_else(|| drift("financial initialization shutdown ledger invalid"))?
        .push(serde_json::json!(stopped));
    owner.save(report)?;
    Ok(stopped)
}
async fn run(corpus: &Path, inventory: &Path, evidence: PathBuf) -> Result<(), CorpusProblem> {
    verify_carddemo_corpus(corpus, inventory)?;
    offered(corpus)?;
    let env_corpus = PathBuf::from(
        env::var_os(CORPUS_ENV)
            .ok_or_else(|| drift("financial initialization exact corpus environment required"))?,
    );
    refuse(
        env_corpus != corpus,
        "financial initialization direct/environment source offer differs",
    )?;
    let package = verify_carddemo_application_package_from_env(inventory)?;
    let (online, binding) =
        online_definition::carddemo_base_online_definition_observed(corpus, package)?;
    let owner = Owner::claim(evidence)?;
    owner.verify()?;
    let path = owner.owned.join("sqlite-initial");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(package_problem)?;
    let identity = directory_id(&path)?;
    let url = format!("sqlite:{}?mode=rwc", path.join("state.sqlite").display());
    let config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: url.clone(),
        artifact_root: path.join("artifacts"),
        cobol_current_date: Some(CLOCK.into()),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let mut report = serde_json::json!({"schema":"private.carddemo.financial-sqlite-initial-owner@1","status":"running","cleanup":"pending","store_profile":"sqlite","oracle_sha256":format!("{:x}",Sha256::digest(INPUTS.as_bytes())),"initialization_jobs":[],"phases":[],"pages":[],"shutdowns":[],"issue_observations":[],"qualification":"First distinct fresh SQLite initialization-only owner; original twelve utility jobs and default controls/limits; nine full source oracle comparisons, no operational financial job or whole application credit","publisher":online.programs.iter().map(|program|serde_json::json!({"program":program.name,"artifact":program.artifact.as_str(),"payload_bytes":program.payload.len(),"payload_sha256":format!("{:x}",Sha256::digest(&program.payload)),"semantic_identity":program.semantic_identity,"manifest":source_artifact_binding::retain_manifest(&program.manifest)})).collect::<Vec<_>>()});
    owner.save(&report)?;
    physical(&owner, &path, identity)?;
    let store =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?);
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let initial = (|| {
        physical(&owner, &path, identity)?;
        server
            .bootstrap_user("IBMUSER", b"TESTPASS")
            .map_err(terminal_problem)?;
        install_base_online_authorities(&server, corpus, &online)?;
        physical(&owner, &path, identity)?;
        server
            .install_online_application(online)
            .map_err(terminal_problem)?;
        let proof = binding.observe_initial(corpus, store.as_ref(), &config.artifact_root)?;
        report["installed_artifact_source_binding"] = serde_json::json!(true);
        owner.save(&report)?;
        let racf = server.racf_service();
        for object in carddemo_base_seed_objects(corpus)? {
            physical(&owner, &path, identity)?;
            racf.permit(
                "DATASET",
                object.dataset.as_str(),
                "IBMUSER",
                AccessIntent::Alter,
            )
            .map_err(terminal_problem)?;
        }
        let csd = String::from_utf8(read_corpus_file(
            corpus,
            &corpus.join("app/csd/CARDDEMO.CSD"),
        )?)
        .map_err(package_problem)?;
        for dataset in parse_csd(&csd)
            .map_err(package_problem)?
            .into_iter()
            .filter(|row| row.kind == "FILE")
            .filter_map(|row| row.properties.get("DSNAME").cloned())
        {
            physical(&owner, &path, identity)?;
            racf.permit("DATASET", &dataset, "IBMUSER", AccessIntent::Alter)
                .map_err(terminal_problem)?;
        }
        for pattern in ["AWS.M2.CARDDEMO.**", "AWS.M2.CARDEMO.**"] {
            physical(&owner, &path, identity)?;
            racf.define_profile("DATASET", pattern, "IBMUSER", None)
                .map_err(terminal_problem)?;
            racf.permit("DATASET", pattern, "IBMUSER", AccessIntent::Alter)
                .map_err(terminal_problem)?;
        }
        physical(&owner, &path, identity)?;
        install_base_batch_source_datasets(&server, corpus)?;
        Ok::<_, CorpusProblem>(proof)
    })();
    let initial = match initial {
        Err(error) => Err(error),
        Ok(proof) => {
            async {
                let app=server.router();let mut ids=BTreeSet::new();
                for relative in JOBS {
                    physical(&owner,&path,identity)?;let jcl=String::from_utf8(read_corpus_file(corpus,&corpus.join(relative))?).map_err(package_problem)?;
                    // Every offered initialization program is a native utility;
                    // no absent compiled batch program is silently substituted.
                    let plan=parse_jcl(&JclBundle {primary:jcl.clone(),symbols:BTreeMap::from([("SYSUID".into(),"IBMUSER".into())]),..JclBundle::default()},JclLimits::default()).map_err(package_problem)?;
                    refuse(plan.steps.iter().any(|step|!matches!(step.program.as_str(),"IDCAMS"|"IEBGENER"|"SDSF")),"financial initialization gained a compiled program dependency")?;
                    let id=submit_job_with_retcode(&server,&app,&jcl,"CC 0000").await?;
                    refuse(!ids.insert(id.clone()),"financial initialization real job ID repeated")?;physical(&owner,&path,identity)?;
                    let job=server.batch_service().get(&id).map_err(terminal_problem)?;let label=plan.name;
                    refuse(job.id!=id || job.name!=label || job.owner!="IBMUSER" || job.state!=JobState::Completed || job.return_code!=Some(0) || job.abend_code.is_some() || job.active_step.is_some(),"financial initialization exact native job did not complete")?;
                    report["initialization_jobs"].as_array_mut().ok_or_else(||drift("financial initialization job ledger invalid"))?.push(serde_json::json!({"source":relative,"job_id":id,"job_name":job.name,"owner":job.owner,"state":"Completed","return_code":job.return_code,"steps":job.steps.iter().map(|step|serde_json::json!({"name":step.name,"state":step.state,"termination":step.termination})).collect::<Vec<_>>() }));owner.save(&report)?;
                }
                let datasets=capture(&owner,&path,identity,&mut report,&server,"initial")?;let provider=dataset_state(store.as_ref())?;
                report["phases"].as_array_mut().ok_or_else(||drift("financial initialization phase ledger invalid"))?.push(serde_json::json!({"phase":"initial","datasets":datasets,"dataset_provider_state":provider}));owner.save(&report)?;
                Ok::<_,CorpusProblem>(proof)
            }.await
        }
    };
    let stopped = drain(&owner, &mut report, server).await?;
    drop(store);
    let result = match initial {
        Err(error) => Err(error),
        Ok(proof) if stopped => {
            physical(&owner, &path, identity)?;
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
                let _unbound =
                    proof.observe_reopened(corpus, store.as_ref(), &path.join("artifacts"))?;
                report["reopened_artifact_source_binding"] = serde_json::json!(true);
                owner.save(&report)?;
                let datasets = capture(&owner, &path, identity, &mut report, &server, "reopen")?;
                let provider = dataset_state(store.as_ref())?;
                refuse(
                    report["phases"][0]["datasets"] != datasets
                        || report["phases"][0]["dataset_provider_state"] != provider,
                    "financial initialization full selected dataset/hash/version state changed on fresh SQLite reopen",
                )?;
                report["phases"].as_array_mut().ok_or_else(||drift("financial initialization phase ledger invalid"))?.push(serde_json::json!({"phase":"reopen","datasets":datasets,"dataset_provider_state":provider}));
                owner.save(&report)
            })();
            let stopped = drain(&owner, &mut report, server).await?;
            drop(store);
            reopened.and_then(|_| {
                refuse(
                    !stopped,
                    "financial initialization reopened drain uncertain",
                )
            })
        }
        Ok(_) => Err(drift("financial initialization initial drain uncertain")),
    };
    if let Err(error) = result {
        report["status"] = serde_json::json!("fail-owned-state-retained");
        report["failure_code"] = serde_json::json!(&error.code);
        owner.save(&report)?;
        return Err(error);
    }
    offered(corpus)?;
    physical(&owner, &path, identity)?;
    refuse(
        !finished(&report),
        "financial initialization comparisons or closure incomplete",
    )?;
    report["cleanup"] = serde_json::json!("verified-owned-removal-pending");
    owner.save(&report)?;
    physical(&owner, &path, identity)?;
    fs::remove_dir_all(&owner.owned).map_err(package_problem)?;
    absence(fs::symlink_metadata(&owner.owned))?;
    report["cleanup"] = serde_json::json!("removed-and-absence-verified");
    report["status"] = serde_json::json!("pass-qualified-initial-only");
    owner.save(&report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> serde_json::Value {
        // Source-shaped synthetic cleanup context, never a producer or issue witness.
        let frozen: serde_json::Value = serde_json::from_str(INPUTS).unwrap();
        let expected = frozen["initial_expected"].as_array().unwrap();
        let datasets=expected.iter().map(|pin|serde_json::json!({"dataset":pin["dataset"],"attributes":pin["attributes"],"records":pin["records"],"framed_sha256":pin["framed_sha256"],"version":1})).collect::<Vec<_>>();
        let mut pages = Vec::new();
        for stage in ["initial", "reopen"] {
            for pin in expected {
                for operation in 0..4 {
                    let actual = match operation {
                        0 | 3 => {
                            serde_json::json!({"operation":"attributes","attributes":pin["attributes"],"version":1})
                        }
                        1 => {
                            serde_json::json!({"operation":"page","start":0,"max_records":4096,"reverse":false,"actual_records":pin["records"],"actual_identities":pin["records"],"version":1,"rows":pin["record_metadata"]})
                        }
                        _ => {
                            serde_json::json!({"operation":"condition","condition":"NOTFND","response":13,"response2":0})
                        }
                    };
                    pages.push(serde_json::json!({"owner_phase":stage,"phase":"initial","dataset":pin["dataset"],"expected_records":pin["records"],"expected_width":pin["attributes"]["logical_record_length"],"expected_framed_sha256":pin["framed_sha256"],"actual":actual}));
                }
            }
        }
        serde_json::json!({"initialization_jobs":JOBS.iter().map(|source|serde_json::json!({"source":source,"state":"Completed","return_code":0})).collect::<Vec<_>>(),"phases":[{"phase":"initial","datasets":datasets,"dataset_provider_state":[]},{"phase":"reopen","datasets":datasets,"dataset_provider_state":[]}],"pages":pages,"shutdowns":[true,true],"installed_artifact_source_binding":true,"reopened_artifact_source_binding":true,"issue_observations":[]})
    }
    #[test]
    fn omitted_reordered_jobs_and_failed_drains_refuse_initial_cleanup() {
        assert!(finished(&context()));
        for variant in 0..6 {
            let mut value = context();
            match variant {
                0 => {
                    value["initialization_jobs"].as_array_mut().unwrap().pop();
                }
                1 => {
                    value["initialization_jobs"]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 1);
                }
                2 => value["initialization_jobs"][0]["return_code"] = serde_json::json!(4),
                3 => value["shutdowns"][1] = serde_json::json!(false),
                4 => value["installed_artifact_source_binding"] = serde_json::json!(false),
                _ => value["reopened_artifact_source_binding"] = serde_json::json!(false),
            };
            assert!(!finished(&value));
        }
    }
    #[test]
    fn incomplete_changed_pages_or_reopen_versions_refuse_initial_cleanup() {
        for variant in 0..5 {
            let mut value = context();
            match variant {
                0 => {
                    value["pages"].as_array_mut().unwrap().pop();
                }
                1 => value["phases"][1]["datasets"][0]["version"] = serde_json::json!(2),
                2 => {
                    value["phases"][1]["dataset_provider_state"] =
                        serde_json::json!([{"version":2}])
                }
                3 => {
                    value["phases"][1]["datasets"].as_array_mut().unwrap().pop();
                }
                _ => value["issue_observations"] = serde_json::json!(["unearned"]),
            };
            assert!(!finished(&value));
        }
    }
    #[test]
    fn same_count_row_key_ordinal_layout_digest_and_eof_metadata_changes_refuse() {
        assert!(finished(&context()));
        for variant in 0..9 {
            let mut value = context();
            match variant {
                0 => {
                    value["pages"][1]["actual"]["rows"][0]["row_sha256"] =
                        serde_json::json!("wrong")
                }
                1 => {
                    value["pages"][1]["actual"]["rows"][0]["key_sha256"] =
                        serde_json::json!("wrong")
                }
                2 => value["pages"][1]["actual"]["rows"][0]["ordinal"] = serde_json::json!(1),
                3 => value["pages"][1]["actual"]["rows"][0]["row_bytes"] = serde_json::json!(299),
                4 => value["pages"][2]["actual"]["response2"] = serde_json::json!(80),
                5 => {
                    value["phases"][0]["datasets"][0]["framed_sha256"] = serde_json::json!("wrong")
                }
                6 => value["pages"][1]["actual"]["max_records"] = serde_json::json!(4095),
                7 => value["pages"][1]["actual"]["start"] = serde_json::json!(1),
                _ => value["pages"][3]["actual"]["attributes"]["ccsid"] = serde_json::json!(1208),
            };
            assert!(!finished(&value));
        }
    }
    #[test]
    fn replaced_initial_sqlite_namespace_refuses_before_removal() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = env::temp_dir().join(format!(
            "financial-initial-control-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&parent).unwrap();
        let owner = Owner::claim(parent.join("evidence")).unwrap();
        let path = owner.owned.join("sqlite-initial");
        fs::create_dir(&path).unwrap();
        let identity = directory_id(&path).unwrap();
        fs::rename(&path, owner.owned.join("retained-original")).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("sentinel"), b"unknown namespace").unwrap();
        assert!(physical(&owner, &path, identity).is_err());
        assert_eq!(
            fs::read(path.join("sentinel")).unwrap(),
            b"unknown namespace"
        );
        // The test owns every claimed/replaced directory and disposes only here.
        fs::remove_dir_all(parent).unwrap();
    }
    #[tokio::test]
    #[ignore = "first actual fresh SQLite initialization-only owner; exact corpus/fresh CARDDEMO_FINANCIAL_INITIAL_EVIDENCE; original supervised serial1800s/64MiB"]
    async fn pinned_sqlite_initial12_jobs_and_full9_datasets_are_owned_metadata_only() {
        let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("exact pinned corpus required"));
        let evidence = PathBuf::from(
            env::var_os("CARDDEMO_FINANCIAL_INITIAL_EVIDENCE")
                .expect("fresh absent diagnostic evidence root required"),
        );
        let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json");
        run(&corpus, &inventory, evidence.clone()).await.unwrap();
        let result: serde_json::Value =
            serde_json::from_slice(&fs::read(evidence.join("seed-lifecycle.json")).unwrap())
                .unwrap();
        assert!(finished(&result));
        assert_eq!(result["cleanup"], "removed-and-absence-verified");
        assert_eq!(result["issue_observations"], serde_json::json!([]));
        println!(
            "Actual distinct SQLite initialization-only source/hash/version/paging/reopen/cleanup observations; zero financial/full application/issue credit"
        );
    }
}
