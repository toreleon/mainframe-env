//! Synthetic controls and an ignored actual producer; no CD009 issue credit.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn package() -> ApplicationPackageV2 {
    let expected = inputs().unwrap();
    let mut payloads = vec![
        (EntryKind::Source, "source/manifest", b"source".to_vec()),
        (
            EntryKind::Resource,
            "resource/manifest",
            b"resource".to_vec(),
        ),
        (
            EntryKind::Data,
            "data/db2/catalog",
            serde_json::to_vec(&expected.local_tables).unwrap(),
        ),
        (EntryKind::Profile, "profile/manifest", b"profile".to_vec()),
        (
            EntryKind::Migration,
            "migration/manifest",
            b"application-package-v1-to-v2".to_vec(),
        ),
    ];
    for program in ["COBTUPDT", "PAUDBLOD", "PAUDBUNL", "CBPAUP0C", "DFSURGU0"] {
        let path = expected
            .programs
            .iter()
            .find(|row| row.path.ends_with(program))
            .unwrap()
            .path
            .as_str();
        payloads.push((
            EntryKind::Program,
            path,
            format!("synthetic-controller:{program}").into_bytes(),
        ));
    }
    let mut blobs = BTreeMap::new();
    let entries = payloads
        .into_iter()
        .map(|(kind, path, raw)| {
            let digest = format!("sha256:{:x}", Sha256::digest(&raw));
            let bytes = raw.len();
            blobs.insert(digest.clone(), raw);
            PackageEntry {
                path: path.into(),
                kind,
                sha256: digest,
                bytes,
                depends_on: if kind == EntryKind::Source {
                    vec![]
                } else {
                    vec!["source/manifest".into()]
                },
            }
        })
        .collect();
    let mut package = ApplicationPackageV2 {
        base: ApplicationPackage {
            manifest: ApplicationManifest {
                name: APP.into(),
                version: "0.2.0".into(),
                target_product: "0.2.0".into(),
                entries,
            },
            blobs,
        },
        generation: 1,
        sections: ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V3_CONTRACT.into(),
            host_abi_libraries: vec![],
            sql_tables: expected.schema_tables,
            sql_rows: vec![],
            ims_definitions: vec![],
            ims_rows: vec![],
            ims_metadata: None,
            ims_tm: None,
            mq_resources: vec![],
            batch_controllers: expected.controllers,
            security_resources: vec![],
        },
        signature: PackageSignature {
            algorithm: mainframe_env_application::PACKAGE_AUTHENTICATION_ALGORITHM.into(),
            key_id: "carddemo-conformance-key".into(),
            value: "pending".into(),
        },
    };
    resign(&mut package).unwrap();
    package
}
fn offered() -> BTreeMap<u64, ApplicationPackageV2> {
    let base = package();
    BTreeMap::from([
        (1, base.clone()),
        (2, controlled(&base, &inputs().unwrap(), false).unwrap()),
    ])
}
fn rows(
    expected: &BTreeMap<String, Vec<(String, serde_json::Value)>>,
) -> BTreeMap<String, Vec<ProviderStateRecord>> {
    expected
        .iter()
        .map(|(namespace, pins)| {
            (
                namespace.clone(),
                pins.iter()
                    .map(|(key, value)| ProviderStateRecord {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        version: 1,
                        payload: serde_json::to_vec(value).unwrap(),
                    })
                    .collect(),
            )
        })
        .collect()
}
fn fixture() -> Snapshot {
    let packages = BTreeMap::from([(1, package())]);
    let package = &packages[&1];
    let identity = package_generation_identity(package).unwrap();
    let artifact = ArtifactRef::new("synthetic-artifact", InvocationLimits::default()).unwrap();
    let mut rows = rows(
        &db2_rows(
            &inputs().unwrap(),
            &packages,
            &BTreeSet::new(),
            1,
            PublicationAction::Install,
        )
        .unwrap(),
    );
    rows.insert(
        "application-package-v2".into(),
        vec![ProviderStateRecord {
            namespace: "application-package-v2".into(),
            key: "registry".into(),
            version: 3,
            payload: b"synthetic retained complete state".to_vec(),
        }],
    );
    Snapshot {
        rows,
        selected: ApplicationGenerationRecord {
            package: APP.into(),
            version: "0.2.0".into(),
            generation: 1,
            identity,
            state: InstallState::Ready,
        },
        packages,
        staged: BTreeSet::new(),
        tables: inputs()
            .unwrap()
            .local_tables
            .into_iter()
            .map(|table| (table, vec![]))
            .collect(),
        artifact: ArtifactRecord {
            artifact,
            media_type: "application/synthetic".into(),
            payload_digest: Sha256::digest(b"synthetic payload").into(),
            payload: b"synthetic payload".to_vec(),
            executable: None,
        },
    }
}
fn completed() -> serde_json::Value {
    let scopes=SCOPES.iter().map(|scope|{
        let phases=if scope.ends_with("-main"){MAIN_PHASES}else{FAILURE_PHASES};
        let rows=phases.iter().enumerate().map(|(index,name)|{
            let packages=if index<if scope.ends_with("-main"){2}else{1}{serde_json::json!({"1":{}})}else{serde_json::json!({"1":{},"2":{}})};
            serde_json::json!({"phase":name,"actual_snapshot":{"selected":{"state":"Ready","generation":if *name=="g2"{2}else{1}},"retained_packages":packages,"staged_generations":if *name=="staged-g2" || scope.ends_with("-malformed") && index>0{vec![2]}else{Vec::<u64>::new()}}})
        }).collect::<Vec<_>>();((*scope).into(),serde_json::json!({"phases":rows}))
    }).collect::<serde_json::Map<_,_>>();
    serde_json::json!({"scopes":scopes,"drains":SCOPES.iter().flat_map(|scope|["initial","reopened"].map(|stage|serde_json::json!({"scope":scope,"stage":stage,"stopped":true}))).collect::<Vec<_>>(),"issue_observations":[]})
}
struct Temporary {
    path: PathBuf,
    identity: (u64, u64),
}
impl Temporary {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "v3-retained-control-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let identity = directory_id(&path).unwrap();
        Self { path, identity }
    }
    fn owner(&self) -> Owner {
        Owner::claim(self.path.join("evidence")).unwrap()
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        assert_eq!(directory_id(&self.path).unwrap(), self.identity);
        fs::remove_dir_all(&self.path).unwrap();
        absence(fs::symlink_metadata(&self.path)).unwrap();
    }
}
fn claims(owner: &Owner) -> Vec<(PathBuf, (u64, u64))> {
    SCOPES
        .iter()
        .map(|name| {
            let path = owner.owned.join(name);
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            let identity = directory_id(&path).unwrap();
            (path, identity)
        })
        .collect()
}

#[test]
fn complete_db2_namespaces_refuse_omitted_extra_same_count_keys_and_unknown_fields() {
    let expected = db2_rows(
        &inputs().unwrap(),
        &offered(),
        &BTreeSet::new(),
        1,
        PublicationAction::Rollback,
    )
    .unwrap();
    let actual = rows(&expected);
    assert!(db2_matches(&actual, &expected));
    for variant in 0..5 {
        let mut changed = actual.clone();
        match variant {
            0 => {
                changed.remove("db2-v1-schema");
            }
            1 => {
                changed.get_mut("db2-v1-table").unwrap().pop();
            }
            2 => changed.get_mut("db2-v1-table").unwrap()[0].key = "SAME-COUNT-WRONG".into(),
            3 => changed.get_mut("db2-v1-table").unwrap()[0].version = 0,
            _ => {
                let row = &mut changed.get_mut("db2-v1-table").unwrap()[0];
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                value["unobserved-field"] = serde_json::json!(true);
                row.payload = serde_json::to_vec(&value).unwrap();
            }
        };
        assert!(!db2_matches(&changed, &expected));
    }
}
#[test]
fn full_db2_schema_column_type_order_key_default_and_padding_changes_refuse() {
    let expected = db2_rows(
        &inputs().unwrap(),
        &offered(),
        &BTreeSet::new(),
        2,
        PublicationAction::Install,
    )
    .unwrap();
    let actual = rows(&expected);
    for variant in 0..5 {
        let mut changed = actual.clone();
        let row = &mut changed.get_mut("db2-v1-schema").unwrap()[1];
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        match variant {
            0 => {
                value["value"]["columns"].as_array_mut().unwrap().swap(0, 1);
            }
            1 => value["value"]["columns"][0]["max_bytes"] = serde_json::json!(99),
            2 => value["value"]["columns"][0]["nullable"] = serde_json::json!(true),
            3 => value["value"]["primary_key"] = serde_json::json!(["WRONG"]),
            _ => value["value"]["columns"][0]["default_value"] = serde_json::json!([32]),
        };
        row.payload = serde_json::to_vec(&value).unwrap();
        assert!(!db2_matches(&changed, &expected));
    }
}
#[test]
fn exact_generation_snapshot_selected_installation_and_provenance_are_independent() {
    let expected = db2_rows(
        &inputs().unwrap(),
        &offered(),
        &BTreeSet::new(),
        1,
        PublicationAction::Rollback,
    )
    .unwrap();
    let actual = rows(&expected);
    for (namespace, path) in [
        (
            "db2-v1-catalog-generation",
            vec!["value", "snapshot", "identity"],
        ),
        ("db2-v1-installation", vec!["value", "identity"]),
        (
            "db2-v1-table-provenance",
            vec!["value", "Application", "owner"],
        ),
        ("db2-state", vec!["schema_version"]),
    ] {
        let mut changed = actual.clone();
        let row = &mut changed.get_mut(namespace).unwrap()[0];
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let mut field = &mut value;
        for key in path {
            field = &mut field[key];
        }
        *field = serde_json::json!("changed");
        row.payload = serde_json::to_vec(&value).unwrap();
        assert!(!db2_matches(&changed, &expected));
    }
}
#[test]
fn staged_generation_is_retained_without_published_db2_or_controller_credit() {
    let expected = inputs().unwrap();
    let packages = offered();
    let staged = BTreeSet::from([2]);
    let pins = db2_rows(&expected, &packages, &staged, 1, PublicationAction::Install).unwrap();
    assert_eq!(pins["db2-v1-catalog-generation"].len(), 1);
    assert_eq!(
        pins["db2-v1-catalog-generation"][0].0,
        format!("{APP}|{:020}", 1)
    );
    let premature = rows(
        &db2_rows(
            &expected,
            &packages,
            &BTreeSet::new(),
            1,
            PublicationAction::Install,
        )
        .unwrap(),
    );
    assert!(!db2_matches(&premature, &pins));
}
#[test]
fn controller_full_selector_plan_artifact_tuple_and_selection_changes_refuse() {
    let expected = inputs().unwrap();
    let published = offered()
        .values()
        .map(|package| controller_generation(package, &expected).unwrap())
        .collect::<Vec<_>>();
    let actual = ControllerState {
        schema_version: "mainframe-env.batch-controller-state@1".into(),
        applications: vec![ControllerApplication {
            application: APP.into(),
            selected: 1,
            generations: published.clone(),
        }],
    };
    assert!(controllers_match(&actual, &published, 1));
    for variant in 0..6 {
        let mut changed = actual.clone();
        match variant {
            0 => changed.applications[0].selected = 2,
            1 => changed.applications[0].generations[0].identity = "wrong".into(),
            2 => {
                changed.applications[0].generations[0].controllers[0]
                    .program
                    .identity = "wrong".into()
            }
            3 => {
                changed.applications[0].generations[0].controllers[0].selector =
                    BatchControllerSelector::TsoRun {
                        program: "OTHER".into(),
                    }
            }
            4 => changed.applications[0].generations[0]
                .controllers
                .swap(0, 1),
            _ => {
                changed.applications[0].generations[0].controllers[1].plan =
                    BatchControllerPlan::ProgramCall
            }
        };
        assert!(!controllers_match(&changed, &published, 1));
    }
}
#[test]
fn publication_requires_complete_matching_package_generation_identity_action_and_sections() {
    let selected = fixture().selected;
    let actual = ApplicationPublicationState {
        schema_version: mainframe_env_application::APPLICATION_PUBLICATION_CONTRACT.into(),
        package: APP.into(),
        generation: 1,
        identity: selected.identity.clone(),
        action: PublicationAction::Rollback,
        controllers: PublicationSectionState::Applied,
        db2: PublicationSectionState::Applied,
        ims: PublicationSectionState::Applied,
        complete: true,
    };
    assert!(publication_matches(
        &actual,
        &selected,
        PublicationAction::Rollback
    ));
    for variant in 0..7 {
        let mut changed = actual.clone();
        match variant {
            0 => changed.generation = 2,
            1 => changed.identity = "wrong".into(),
            2 => changed.action = PublicationAction::Install,
            3 => changed.controllers = PublicationSectionState::Failed,
            4 => changed.db2 = PublicationSectionState::Applying,
            5 => changed.ims = PublicationSectionState::NotApplicable,
            _ => changed.complete = false,
        };
        assert!(!publication_matches(
            &changed,
            &selected,
            PublicationAction::Rollback
        ));
    }
}
#[test]
fn every_source_selected_generic_admission_refusal_preserves_actual_installer_payload() {
    let base = package();
    let core = ApplicationInstallerV2::new(
        "0.2.0",
        PackageLimits::default(),
        carddemo_package_trust().unwrap(),
    );
    core.stage(&base).unwrap();
    core.commit(&base).unwrap();
    let before = core.state_payload().unwrap();
    let candidates = admission_candidates(&base).unwrap();
    assert_eq!(candidates.len(), 7);
    for (_, problem, candidate) in candidates {
        assert_eq!(core.stage(&candidate), Err(problem));
        assert_eq!(core.state_payload().unwrap(), before);
    }
}
#[test]
fn same_count_raw_payload_revision_selected_table_and_artifact_changes_break_state_equality() {
    let before = fixture();
    assert!(same_providers(&before, &before));
    for variant in 0..5 {
        let mut changed = before.clone();
        match variant {
            0 => changed.rows.get_mut("db2-v1-table").unwrap()[0].payload[0] ^= 1,
            1 => changed.rows.get_mut("db2-v1-table").unwrap()[0].version += 1,
            2 => changed.selected.generation = 2,
            3 => changed.tables[0].1.push(vec![b"unexpected row".to_vec()]),
            _ => changed.artifact.payload[0] ^= 1,
        };
        assert!(!same_providers(&before, &changed));
    }
}
#[test]
fn reinstall_advances_only_registry_cas_once_while_all_content_and_provider_state_remain_exact() {
    let before = fixture();
    let mut valid = before.clone();
    valid.rows.get_mut("application-package-v2").unwrap()[0].version += 1;
    assert!(reinstalled(&before, &valid));
    for variant in 0..5 {
        let mut changed = valid.clone();
        match variant {
            0 => changed.rows.get_mut("application-package-v2").unwrap()[0].version -= 1,
            1 => changed.rows.get_mut("application-package-v2").unwrap()[0].version += 1,
            2 => changed.rows.get_mut("application-package-v2").unwrap()[0].payload[0] ^= 1,
            3 => {
                changed
                    .packages
                    .get_mut(&1)
                    .unwrap()
                    .base
                    .blobs
                    .values_mut()
                    .next()
                    .unwrap()[0] ^= 1
            }
            _ => changed.rows.get_mut("db2-v1-table").unwrap()[0].version += 1,
        };
        assert!(!reinstalled(&before, &changed));
    }
}
#[test]
fn full_phase_order_selection_retention_and_staged_shape_gate_cleanup() {
    assert!(finished_v3(&completed()));
    for variant in 0..6 {
        let mut value = completed();
        match variant {
            0 => {
                value["scopes"]["memory-main"]["phases"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            1 => {
                value["scopes"]["memory-main"]["phases"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            }
            2 => {
                value["scopes"]["sqlite-main"]["phases"][3]["actual_snapshot"]["selected"]["generation"] =
                    serde_json::json!(1)
            }
            3 => {
                value["scopes"]["memory-main"]["phases"][3]["actual_snapshot"]["retained_packages"]
                    .as_object_mut()
                    .unwrap()
                    .remove("1");
            }
            4 => {
                value["scopes"]["sqlite-malformed"]["phases"][2]["actual_snapshot"]["staged_generations"] =
                    serde_json::json!([])
            }
            _ => {
                value["scopes"]["memory-main"]["phases"][2]["actual_snapshot"]["staged_generations"] =
                    serde_json::json!([])
            }
        };
        assert!(!finished_v3(&value));
    }
}
#[test]
fn all_eight_exact_drains_are_required_and_no_issue_observation_is_created() {
    for variant in 0..4 {
        let mut value = completed();
        match variant {
            0 => {
                value["drains"].as_array_mut().unwrap().pop();
            }
            1 => {
                value["drains"].as_array_mut().unwrap().swap(0, 1);
            }
            2 => value["drains"][7]["stopped"] = serde_json::json!(false),
            _ => value["issue_observations"] = serde_json::json!(["CD-009"]),
        };
        assert!(!finished_v3(&value));
    }
}
#[test]
fn replaced_backend_namespace_refuses_before_product_or_artifact_open() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let backend = Backend::create(&owner, "memory-main", false).unwrap();
    assert!(backend.verify(&owner).is_ok());
    fs::rename(&backend.path, owner.owned.join("retained-original")).unwrap();
    fs::create_dir(&backend.path).unwrap();
    fs::write(backend.path.join("sentinel"), b"unknown namespace").unwrap();
    assert!(backend.verify(&owner).is_err());
    assert!(backend.open(&owner).is_err());
    assert_eq!(
        fs::read(backend.path.join("sentinel")).unwrap(),
        b"unknown namespace"
    );
}
#[test]
fn failed_required_receipt_retention_preserves_real_v3_namespace() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let claims = claims(&owner);
    fs::write(claims[0].0.join("artifact"), b"owned data").unwrap();
    fs::write(
        owner.evidence.join("seed-lifecycle.json"),
        b"unknown receipt",
    )
    .unwrap();
    let mut report = completed();
    assert!(cleanup_v3(&owner, &mut report, &claims).is_err());
    assert_eq!(
        fs::read(claims[0].0.join("artifact")).unwrap(),
        b"owned data"
    );
    assert_eq!(
        fs::read(owner.evidence.join("seed-lifecycle.json")).unwrap(),
        b"unknown receipt"
    );
}
#[test]
fn missing_scopes_and_uncertain_closure_preserve_owned_v3_state() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let claims = claims(&owner);
    let mut report = completed();
    report["drains"][0]["stopped"] = serde_json::json!(false);
    assert!(cleanup_v3(&owner, &mut report, &claims).is_err());
    assert!(owner.owned.is_dir());
    let mut report = completed();
    assert!(cleanup_v3(&owner, &mut report, &claims[..3]).is_err());
    assert!(owner.owned.is_dir());
}
#[test]
fn root_replacement_refuses_v3_removal_preserving_unknown_sentinel() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let claims = claims(&owner);
    fs::rename(&owner.owned, owner.evidence.join("retained-original")).unwrap();
    fs::create_dir(&owner.owned).unwrap();
    fs::write(owner.owned.join("sentinel"), b"unknown root").unwrap();
    assert!(cleanup_v3(&owner, &mut completed(), &claims).is_err());
    assert_eq!(
        fs::read(owner.owned.join("sentinel")).unwrap(),
        b"unknown root"
    );
}
#[test]
fn unknown_physical_scope_member_refuses_removal_even_with_all_four_valid_claims() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let claims = claims(&owner);
    fs::write(owner.owned.join("unknown"), b"unclaimed sentinel").unwrap();
    assert!(cleanup_v3(&owner, &mut completed(), &claims).is_err());
    assert_eq!(
        fs::read(owner.owned.join("unknown")).unwrap(),
        b"unclaimed sentinel"
    );
}
#[test]
fn actual_owned_v3_cleanup_requires_saved_receipt_then_retains_verified_absence() {
    let temp = Temporary::new();
    let owner = temp.owner();
    let claims = claims(&owner);
    let mut report = completed();
    owner.save(&report).unwrap();
    cleanup_v3(&owner, &mut report, &claims).unwrap();
    absence(fs::symlink_metadata(&owner.owned)).unwrap();
    let retained: serde_json::Value =
        serde_json::from_slice(&fs::read(owner.evidence.join("seed-lifecycle.json")).unwrap())
            .unwrap();
    assert_eq!(retained["cleanup"], "removed-and-absence-verified");
    assert_eq!(retained["issue_observations"], serde_json::json!([]));
}
#[tokio::test]
#[ignore = "actual exact pinned source and fresh CARDDEMO_V3_RETAINED_EVIDENCE; serial under original1800s/64MiB bounds"]
async fn pinned_v3_retained_package_state_owner() {
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("exact pinned corpus required"));
    let evidence = PathBuf::from(
        env::var_os("CARDDEMO_V3_RETAINED_EVIDENCE").expect("fresh absent evidence root required"),
    );
    let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    run(&corpus, &inventory, evidence.clone()).await.unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence.join("seed-lifecycle.json")).unwrap()).unwrap();
    assert!(finished_v3(&report));
    assert_eq!(report["cleanup"], "removed-and-absence-verified");
    assert_eq!(report["issue_observations"], serde_json::json!([]));
    println!(
        "Actual retained V3 package/selected publication/controller/local Db2/empty IMS/artifact state on Memory and fresh SQLite; staged failure, rollback, refusals, reopen and eight drains; zero whole CD009/compiled application execution credit"
    );
}
