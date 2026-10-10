//! Synthetic refusal controls earn no application, corpus or issue credit.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Temporary(PathBuf);
impl Temporary {
    fn create() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "seed-lifecycle-control-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture() -> SeedProof {
    let mut expected = BTreeMap::new();
    let mut actual = BTreeMap::new();
    for i in 0..15 {
        let name = format!("FIXTUR{i:02}");
        let records = vec![b"A000".to_vec(), b"B000".to_vec()];
        let identities = vec![b"A".to_vec(), b"B".to_vec()];
        let attributes = DatasetAttributes {
            organization: DatasetOrganization::KeySequenced,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: Some(0),
            key_length: Some(1),
            ccsid: Some(37),
        };
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
                attributes_version: 1,
                records,
                identities,
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
    SeedProof { expected, actual }
}
fn copied(proof: &SeedProof) -> SeedProof {
    SeedProof {
        expected: proof.expected.clone(),
        actual: proof.actual.clone(),
    }
}
fn completed_context() -> serde_json::Value {
    // Ownership helper context only. These synthetic bytes never run/bind the producer.
    let snapshot = serde_json::Value::Object(target_counts().into_iter().map(|(name,count)| (name,serde_json::json!({"version":3,"attributes_version":3,"catalog_version":3,"records_base64":vec!["synthetic";count],"identities_base64":vec!["synthetic";count]}))).collect());
    serde_json::json!({"phases":PHASES.iter().map(|name|serde_json::json!({"phase":name,"selected_generation":if *name=="g2" {"g2"} else {"g1"},"complete_raw_snapshot":snapshot})).collect::<Vec<_>>(),"shutdowns":[true,true],"status":"comparisons-pending-cleanup","cleanup":"pending","issue_observations":[]})
}
fn reply(raw: &[u8]) -> mainframe_env_host_api::CicsResponse {
    mainframe_env_host_api::CicsResponse {
        disposition: CicsDisposition::Complete,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "CARDDEMO".into(),
        sysid: "LOCAL".into(),
        transaction: "SEED".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            raw.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        unit_of_work: None,
    }
}

#[test]
fn every_same_count_raw_key_layout_and_revision_change_breaks_unchanged_state() {
    let before = fixture();
    assert!(complete_match(&before.expected, &before.actual));
    assert!(same(&before, &copied(&before)));
    for name in before.actual.keys() {
        for variant in 0..6 {
            let mut after = copied(&before);
            let row = after.actual.get_mut(name).unwrap();
            match variant {
                0 => row.records[1][3] ^= 1,
                1 => row.identities[1][0] ^= 1,
                2 => row.attributes.ccsid = Some(1047),
                3 => row.version += 1,
                4 => row.catalog.version += 1,
                _ => row.catalog.related = Some(DatasetName::new("OTHER", 128).unwrap()),
            }
            assert!(!same(&before, &after));
        }
    }
}
#[test]
fn every_generation_member_must_advance_and_target_set_remain_exact() {
    let before = fixture();
    let mut after = copied(&before);
    for row in after.actual.values_mut() {
        row.version += 1;
        row.attributes_version += 1;
        row.catalog.version += 1;
    }
    assert!(advanced(&before, &after));
    for name in before.actual.keys() {
        let mut missing = copied(&after);
        missing.actual.remove(name);
        assert!(!advanced(&before, &missing));
        let mut reused = copied(&after);
        reused.actual.get_mut(name).unwrap().version = 1;
        assert!(!advanced(&before, &reused));
    }
}
#[test]
fn complete_phase_names_order_selection_and_raw_snapshot_are_required() {
    let complete = completed_context();
    assert!(finished(&complete));
    for i in 0..PHASES.len() {
        let mut missing = complete.clone();
        missing["phases"].as_array_mut().unwrap().remove(i);
        assert!(!finished(&missing));
        let mut wrong = complete.clone();
        wrong["phases"][i]["selected_generation"] = serde_json::json!("other");
        assert!(!finished(&wrong));
        let mut skipped = complete.clone();
        skipped["phases"][i]["complete_raw_snapshot"] = serde_json::json!({});
        assert!(!finished(&skipped));
    }
    let mut wrong = complete.clone();
    wrong["phases"].as_array_mut().unwrap().swap(0, 1);
    assert!(!finished(&wrong));
    let mut duplicate = complete.clone();
    let row = duplicate["phases"][0].clone();
    duplicate["phases"].as_array_mut().unwrap().push(row);
    assert!(!finished(&duplicate));
}
#[test]
fn every_failed_unknown_or_missing_server_stop_refuses_completion() {
    for stops in [
        serde_json::json!([]),
        serde_json::json!([true]),
        serde_json::json!([false, true]),
        serde_json::json!([true, false]),
        serde_json::json!([true, null]),
        serde_json::json!([true, true, true]),
    ] {
        let mut report = completed_context();
        report["shutdowns"] = stops;
        assert!(!finished(&report));
    }
}
#[test]
fn complete_cics_row_and_key_are_required_without_sorting_or_byte_normalization() {
    let mut response = reply(b"complete-source-row");
    response.outputs.insert(
        "RIDFLD".into(),
        BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            b"KEY".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    assert!(row_reply(&response, b"complete-source-row", Some(b"KEY")));
    assert!(!row_reply(&response, b"complete-source-raw", Some(b"KEY")));
    assert!(!row_reply(&response, b"complete-source-row", Some(b"KEZ")));
    response.outputs.clear();
    assert!(!row_reply(&response, b"complete-source-row", Some(b"KEY")));
}
#[test]
fn independent_metadata_keeps_full_padding_keys_ordinals_lengths_and_order() {
    let rows = vec![
        b"first complete padded row  ".to_vec(),
        b"second complete padded row ".to_vec(),
    ];
    let keys = vec![b"KEY1 ".to_vec(), b"KEY2 ".to_vec()];
    let records = rows
        .iter()
        .zip(&keys)
        .enumerate()
        .map(|(ordinal, (row, key))| HostRecord {
            ordinal,
            row_bytes: row.len(),
            row_sha256: format!("{:x}", Sha256::digest(row)),
            key_bytes: key.len(),
            key_sha256: format!("{:x}", Sha256::digest(key)),
        })
        .collect::<Vec<_>>();
    let make = || Alias {
        name: "SYNTHETIC".into(),
        dataset: "SYNTHETIC.DATA".into(),
        aix: false,
        records: records.clone(),
        host_rows: rows.clone(),
        host_keys: keys.clone(),
    };
    assert!(complete_host_expectations(&make()));
    for variant in 0..9 {
        let mut alias = make();
        match variant {
            0 => {
                alias.host_rows[0].pop();
            }
            1 => alias.host_rows[1][0] ^= 1,
            2 => alias.host_rows.swap(0, 1),
            3 => alias.host_keys[0][4] = b'X',
            4 => alias.host_keys.swap(0, 1),
            5 => alias.records[1].ordinal = 0,
            6 => alias.records[1].row_bytes -= 1,
            7 => {
                alias.host_rows.pop();
                alias.host_keys.pop();
            }
            _ => alias.records[1].key_sha256 = alias.records[0].key_sha256.clone(),
        };
        assert!(!complete_host_expectations(&alias));
    }
}
#[test]
fn external_full_expectation_refuses_wrong_length_same_size_bytes_and_links() {
    let temp = Temporary::create();
    let path = temp.0.join("offered.json");
    let seal = ExternalOracle {
        bytes: 103075,
        sha256: "83c98c8ca2247e3c456e35c2111508d147ad538f9b140e7c0e3dca1eedf3b930".into(),
    };
    fs::write(&path, b"short untrusted expectation").unwrap();
    assert!(external_expectations(&path, &seal).is_err());
    fs::write(&path, vec![b' '; 103075]).unwrap();
    assert!(external_expectations(&path, &seal).is_err());
    let link = temp.0.join("linked.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(external_expectations(&link, &seal).is_err());
    let wrong = ExternalOracle {
        bytes: 103075,
        sha256: format!("{:064x}", 0),
    };
    assert!(external_expectations(&path, &wrong).is_err());
    let metadata: serde_json::Value = serde_json::from_str(ORACLE).unwrap();
    for alias in metadata["aliases"].as_array().unwrap() {
        assert!(alias.get("rows_host_base64").is_none());
        assert!(alias.get("key_host_base64").is_none());
        assert!(alias["records"].as_array().is_some());
    }
}
#[test]
fn full_raw_evidence_claims_private_directories_and_retains_private_files() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    assert_eq!(fs::metadata(&owner.evidence).unwrap().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(&owner.owned).unwrap().mode() & 0o777, 0o700);
    owner
        .save(&serde_json::json!({"synthetic_retention":"control only; no observations"}))
        .unwrap();
    assert_eq!(
        fs::metadata(owner.evidence.join("seed-lifecycle.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}
#[test]
fn cics_reply_schema_actor_condition_and_transfer_drift_refuse() {
    let original = reply(b"row");
    assert!(row_reply(&original, b"row", None));
    for variant in 0..8 {
        let mut row = original.clone();
        match variant {
            0 => row.applid = "OTHER".into(),
            1 => row.sysid = "OTHER".into(),
            2 => row.transaction = "OTHER".into(),
            3 => row.response = 13,
            4 => row.target = Some("LABEL".into()),
            5 => row.next_transaction = Some("NEXT".into()),
            6 => row.disposition = CicsDisposition::Suspended,
            _ => {
                row.payload = BoundedPayload::new(
                    "wrong-schema",
                    b"row".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
        };
        assert!(!row_reply(&row, b"row", None));
    }
}
#[test]
fn eof_requires_exact_condition_codes_empty_payload_and_completed_actor() {
    let mut eof = reply(b"");
    eof.condition = "ENDFILE".into();
    eof.response = 20;
    assert!(eof_reply(&eof));
    for variant in 0..6 {
        let mut row = eof.clone();
        match variant {
            0 => row.condition = "NORMAL".into(),
            1 => row.response = 13,
            2 => row.response2 = 1,
            3 => {
                row.payload = BoundedPayload::new(
                    "mainframe-env.cics.payload@1",
                    b"extra".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
            4 => row.disposition = CicsDisposition::Abended,
            _ => row.transaction = "OTHER".into(),
        };
        assert!(!eof_reply(&row));
    }
}
#[test]
fn namespace_snapshot_refuses_extra_actual_provider_rows() {
    let store = MemoryStore::new(Default::default());
    assert!(state(&store).unwrap().values().all(Vec::is_empty));
    for i in 0..13 {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: format!("EXTRA{i}"),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    assert!(state(&store).is_err());
}
#[test]
fn replacing_owned_namespace_refuses_cleanup_and_preserves_replacement_bytes() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    let original = owner.evidence.join("retained-original");
    fs::rename(&owner.owned, &original).unwrap();
    fs::create_dir(&owner.owned).unwrap();
    fs::write(owner.owned.join("sentinel"), b"replacement bytes").unwrap();
    assert!(owner.verify().is_err());
    assert!(owner.cleanup(&mut completed_context()).is_err());
    assert_eq!(
        fs::read(owner.owned.join("sentinel")).unwrap(),
        b"replacement bytes"
    );
    assert!(original.is_dir());
}
#[test]
fn required_pre_removal_retention_failure_keeps_actual_owned_state() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    fs::write(owner.owned.join("artifact"), b"retained actual bytes").unwrap();
    fs::write(
        owner.evidence.join("seed-lifecycle.json.next"),
        b"force create_new refusal",
    )
    .unwrap();
    assert!(owner.cleanup(&mut completed_context()).is_err());
    assert_eq!(
        fs::read(owner.owned.join("artifact")).unwrap(),
        b"retained actual bytes"
    );
}
#[test]
fn unknown_and_replaced_receipt_destinations_are_never_overwritten() {
    let temp = Temporary::create();
    let report = serde_json::json!({"synthetic":"ownership only"});
    let owner = Owner::claim(temp.0.join("unknown")).unwrap();
    let destination = owner.evidence.join("seed-lifecycle.json");
    fs::write(&destination, b"unknown prior receipt").unwrap();
    assert!(owner.save(&report).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"unknown prior receipt");
    assert!(!owner.evidence.join("seed-lifecycle.json.next").exists());
    let owner = Owner::claim(temp.0.join("appeared")).unwrap();
    let destination = owner.evidence.join("seed-lifecycle.json");
    assert!(
        owner
            .save_observed(
                &report,
                || fs::write(&destination, b"appeared after initial admission").unwrap(),
                || {}
            )
            .is_err()
    );
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"appeared after initial admission"
    );
    assert!(owner.evidence.join("seed-lifecycle.json.next").is_file());
    let owner = Owner::claim(temp.0.join("replaced")).unwrap();
    owner.save(&report).unwrap();
    let destination = owner.evidence.join("seed-lifecycle.json");
    let original = owner.evidence.join("retained-original.json");
    fs::rename(&destination, &original).unwrap();
    fs::write(&destination, b"replacement receipt").unwrap();
    assert!(owner.save(&report).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"replacement receipt");
    assert!(original.is_file());
}
#[test]
fn evidence_root_replacement_after_sync_refuses_publication_and_retains_original() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    let original = temp.0.join("original");
    assert!(
        owner
            .save_observed(
                &serde_json::json!({"synthetic":"retained original staging"}),
                || {
                    fs::rename(&owner.evidence, &original).unwrap();
                    fs::create_dir(&owner.evidence).unwrap();
                    fs::write(owner.evidence.join("sentinel"), b"unknown root bytes").unwrap();
                },
                || {}
            )
            .is_err()
    );
    assert_eq!(
        fs::read(owner.evidence.join("sentinel")).unwrap(),
        b"unknown root bytes"
    );
    assert!(!owner.evidence.join("seed-lifecycle.json").exists());
    assert!(original.join("seed-lifecycle.json.next").is_file());
    assert!(original.join("owned-state").is_dir());
    assert!(owner.receipt.lock().unwrap().is_none());
}
#[test]
fn replaced_open_staging_file_cannot_be_published_as_retained_receipt() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    let staging = owner.evidence.join("seed-lifecycle.json.next");
    let original = owner.evidence.join("original-next");
    assert!(
        owner
            .save_observed(
                &serde_json::json!({"synthetic":"staging identity"}),
                || {
                    fs::rename(&staging, &original).unwrap();
                    fs::write(&staging, b"unknown staging bytes").unwrap();
                },
                || {}
            )
            .is_err()
    );
    assert_eq!(fs::read(&staging).unwrap(), b"unknown staging bytes");
    assert!(original.is_file());
    assert!(!owner.evidence.join("seed-lifecycle.json").exists());
    assert!(owner.receipt.lock().unwrap().is_none());
    let owner = Owner::claim(temp.0.join("inplace")).unwrap();
    let staging = owner.evidence.join("seed-lifecycle.json.next");
    assert!(
        owner
            .save_observed(
                &serde_json::json!({"synthetic":"same-size inplace staging"}),
                || {
                    let length = fs::metadata(&staging).unwrap().len() as usize;
                    fs::write(&staging, vec![b'X'; length]).unwrap();
                },
                || {}
            )
            .is_err()
    );
    assert!(!owner.evidence.join("seed-lifecycle.json").exists());
    assert!(staging.is_file());
    assert!(owner.receipt.lock().unwrap().is_none());
}
#[test]
fn post_publication_root_replacement_cannot_claim_successful_retention() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    let original = temp.0.join("original");
    assert!(
        owner
            .save_observed(
                &serde_json::json!({"synthetic":"post publication identity"}),
                || {},
                || {
                    fs::rename(&owner.evidence, &original).unwrap();
                    fs::create_dir(&owner.evidence).unwrap();
                    fs::write(
                        owner.evidence.join("seed-lifecycle.json"),
                        b"unknown replacement final",
                    )
                    .unwrap();
                }
            )
            .is_err()
    );
    assert_eq!(
        fs::read(owner.evidence.join("seed-lifecycle.json")).unwrap(),
        b"unknown replacement final"
    );
    assert!(original.join("seed-lifecycle.json").is_file());
    assert!(original.join("owned-state").is_dir());
    assert!(owner.receipt.lock().unwrap().is_none());
}
#[test]
fn final_absence_is_retained_only_after_actual_exact_owned_disposal() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    fs::write(owner.owned.join("artifact"), b"owned control bytes").unwrap();
    owner.cleanup(&mut completed_context()).unwrap();
    assert!(!owner.owned.exists());
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(owner.evidence.join("seed-lifecycle.json")).unwrap())
            .unwrap();
    assert_eq!(report["cleanup"], "removed-and-absence-verified");
    assert_eq!(report["issue_observations"], serde_json::json!([]));
}
#[test]
fn absence_uncertainty_is_not_relabelled_as_missing_state() {
    assert!(absence(Err(std::io::Error::from(std::io::ErrorKind::NotFound))).is_ok());
    assert!(
        absence(Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )))
        .is_err()
    );
    let temp = Temporary::create();
    assert!(absence(fs::symlink_metadata(&temp.0)).is_err());
}
#[test]
fn count_matching_generation_receipt_without_identity_or_replay_match_refuses() {
    let receipt = SeedInstallReceipt {
        package: "CARDDEMO".into(),
        generation: "g1".into(),
        identity: "independent-identity".into(),
        seed_objects: 13,
        datasets: 12,
        records: 1137,
        bytes: 427700,
        replayed: false,
    };
    assert!(check_receipt(&receipt, "g1", "independent-identity", false).is_ok());
    assert!(check_receipt(&receipt, "g1", "wrong-identity", false).is_err());
    assert!(check_receipt(&receipt, "g1", "independent-identity", true).is_err());
    assert!(check_receipt(&receipt, "g2", "independent-identity", false).is_err());
}

#[tokio::test]
#[ignore = "actual exact pinned corpus and fresh CARDDEMO_SEED_LIFECYCLE_EVIDENCE owner; run serial under original outer supervision"]
async fn pinned_seed_state_lifecycle_produces_only_complete_owned_issue_predicates() {
    let corpus =
        PathBuf::from(env::var_os(CORPUS_ENV).expect("exact pinned first-party corpus required"));
    let evidence = PathBuf::from(
        env::var_os("CARDDEMO_SEED_LIFECYCLE_EVIDENCE")
            .expect("fresh external evidence owner required"),
    );
    let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let observed = run(&corpus, &inventory, evidence.clone()).await.unwrap();
    assert!(observed.journeys().is_empty());
    assert_eq!(observed.issues().len(), 1);
    assert_eq!(observed.issues()[0].0, "CD-016");
    assert_eq!(observed.issues()[0].1.len(), 4);
    assert!(!evidence.join("owned-state").exists());
    println!(
        "Actual complete pinned source seed lifecycle: {:?}; raw records/keys/layouts/revisions, selected generations, eight FILE routes and fresh SQLite reopen; local direct AIX and Cp037 transport, zero compiled application execution credit",
        observed.issues()
    );
}
