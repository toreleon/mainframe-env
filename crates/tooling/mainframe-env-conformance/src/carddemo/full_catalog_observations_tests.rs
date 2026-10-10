//! Synthetic complete-state/ownership controls; no application or issue credit.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Temporary(PathBuf);
impl Temporary {
    fn create() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "full-catalog-control-{}-{}-{}",
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
fn fixture() -> CatalogSnapshot {
    let states = CATALOGS
        .iter()
        .map(|(namespace, _)| {
            (
                (*namespace).into(),
                BTreeMap::from([
                    (
                        "A".into(),
                        record(namespace, "A", b"complete first offered binding  ".to_vec()),
                    ),
                    (
                        "B".into(),
                        record(namespace, "B", b"complete second offered binding ".to_vec()),
                    ),
                ]),
            )
        })
        .collect();
    let payload = b"synthetic full artifact bytes; no executable claim".to_vec();
    let artifact = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(&payload)),
        InvocationLimits::default(),
    )
    .unwrap();
    CatalogSnapshot {
        states,
        artifacts: BTreeMap::from([(
            "PROGRAM".into(),
            ArtifactRecord {
                artifact,
                media_type: "application/test".into(),
                payload_digest: Sha256::digest(&payload).into(),
                payload,
                executable: None,
            },
        )]),
    }
}
fn ready() -> serde_json::Value {
    serde_json::json!({"snapshots":["synthetic only","synthetic only","synthetic only"],"shutdowns":[true,true],"status":"pending","cleanup":"pending","issue_observations":[],"journey_observations":[]})
}

#[test]
fn every_same_count_payload_reference_namespace_key_and_version_drift_refuses() {
    let expected = fixture();
    assert!(compare(&expected, &expected.clone()).is_ok());
    for namespace in expected.states.keys() {
        for variant in 0..5 {
            let mut actual = expected.clone();
            let row = actual
                .states
                .get_mut(namespace)
                .unwrap()
                .get_mut("B")
                .unwrap();
            match variant {
                0 => row.payload[1] ^= 1,
                1 => row.payload.pop().map(|_| ()).unwrap(),
                2 => row.namespace = "other".into(),
                3 => row.key = "wrong".into(),
                _ => row.version += 1,
            };
            assert!(compare(&expected, &actual).is_err());
        }
    }
}
#[test]
fn every_missing_extra_or_renamed_catalog_or_artifact_refuses() {
    let expected = fixture();
    for namespace in expected.states.keys() {
        let mut actual = expected.clone();
        actual.states.remove(namespace);
        assert!(compare(&expected, &actual).is_err());
        let mut actual = expected.clone();
        actual.states.get_mut(namespace).unwrap().remove("B");
        assert!(compare(&expected, &actual).is_err());
        let mut actual = expected.clone();
        let row = record(namespace, "C", b"extra".to_vec());
        actual
            .states
            .get_mut(namespace)
            .unwrap()
            .insert("C".into(), row);
        assert!(compare(&expected, &actual).is_err());
    }
    let mut actual = expected.clone();
    let row = actual.artifacts.remove("PROGRAM").unwrap();
    actual.artifacts.insert("OTHER".into(), row);
    assert!(compare(&expected, &actual).is_err());
}
#[test]
fn complete_artifact_bytes_digest_reference_and_metadata_are_required() {
    let expected = fixture();
    for variant in 0..5 {
        let mut actual = expected.clone();
        let row = actual.artifacts.get_mut("PROGRAM").unwrap();
        match variant {
            0 => row.payload[1] ^= 1,
            1 => row.payload.pop().map(|_| ()).unwrap(),
            2 => row.payload_digest[0] ^= 1,
            3 => row.media_type = "different".into(),
            _ => {
                row.artifact =
                    ArtifactRef::new(format!("sha256:{:064x}", 0), InvocationLimits::default())
                        .unwrap()
            }
        };
        assert!(compare(&expected, &actual).is_err());
    }
}
#[test]
fn retained_executable_metadata_preserves_every_field_and_optional_dialect_state() {
    let mut expected = fixture();
    assert_eq!(
        retain_snapshot(&expected)["artifacts"][0]["executable_metadata"],
        serde_json::Value::Null
    );
    let metadata = ExecutableArtifactMetadata {
        artifact_contract: "synthetic.artifact@3".into(),
        compatibility_profile: "synthetic.profile@1".into(),
        compiler_generation: "synthetic.compiler@7".into(),
        target: "synthetic-target".into(),
        options: BTreeMap::from([
            ("alpha".into(), "first value".into()),
            ("omega".into(), "second value".into()),
        ]),
        host_interfaces: std::collections::BTreeSet::from([
            "synthetic.host-a@1".into(),
            "synthetic.host-z@2".into(),
        ]),
        ir_contract: "synthetic.ir@3".into(),
        dialect_contracts: Some(std::collections::BTreeSet::from([
            "synthetic.dialect-a@1".into(),
            "synthetic.dialect-z@2".into(),
        ])),
        semantic_identity: "synthetic offered semantic identity".into(),
        manifest_payload_digest: [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31,
        ],
    };
    let offered = serde_json::json!({
        "artifact_contract": "synthetic.artifact@3",
        "compatibility_profile": "synthetic.profile@1",
        "compiler_generation": "synthetic.compiler@7",
        "target": "synthetic-target",
        "options": {"alpha": "first value", "omega": "second value"},
        "host_interfaces": ["synthetic.host-a@1", "synthetic.host-z@2"],
        "ir_contract": "synthetic.ir@3",
        "dialect_contracts": ["synthetic.dialect-a@1", "synthetic.dialect-z@2"],
        "semantic_identity": "synthetic offered semantic identity",
        "manifest_payload_digest": [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            23, 24, 25, 26, 27, 28, 29, 30, 31,
        ],
    });
    expected.artifacts.get_mut("PROGRAM").unwrap().executable = Some(metadata);
    assert_eq!(
        retain_snapshot(&expected)["artifacts"][0]["executable_metadata"],
        offered
    );
    for field in 0..10 {
        let mut drift = expected.clone();
        let actual = drift
            .artifacts
            .get_mut("PROGRAM")
            .unwrap()
            .executable
            .as_mut()
            .unwrap();
        match field {
            0 => actual.artifact_contract.push('X'),
            1 => actual.compatibility_profile.push('X'),
            2 => actual.compiler_generation.push('X'),
            3 => actual.target.push('X'),
            4 => actual.options.get_mut("omega").unwrap().push('X'),
            5 => {
                actual.host_interfaces.remove("synthetic.host-z@2");
            }
            6 => actual.ir_contract.push('X'),
            7 => {
                actual
                    .dialect_contracts
                    .as_mut()
                    .unwrap()
                    .remove("synthetic.dialect-z@2");
            }
            8 => actual.semantic_identity.push('X'),
            _ => actual.manifest_payload_digest[31] ^= 1,
        }
        assert!(compare(&expected, &drift).is_err());
        assert_ne!(
            retain_snapshot(&drift)["artifacts"][0]["executable_metadata"],
            offered
        );
    }
    let mut missing = expected.clone();
    missing
        .artifacts
        .get_mut("PROGRAM")
        .unwrap()
        .executable
        .as_mut()
        .unwrap()
        .dialect_contracts = None;
    let mut empty = missing.clone();
    empty
        .artifacts
        .get_mut("PROGRAM")
        .unwrap()
        .executable
        .as_mut()
        .unwrap()
        .dialect_contracts = Some(std::collections::BTreeSet::new());
    assert_eq!(
        retain_snapshot(&missing)["artifacts"][0]["executable_metadata"]["dialect_contracts"],
        serde_json::Value::Null
    );
    assert_eq!(
        retain_snapshot(&empty)["artifacts"][0]["executable_metadata"]["dialect_contracts"],
        serde_json::json!([])
    );
    assert!(compare(&expected, &missing).is_err());
    assert!(compare(&expected, &empty).is_err());
    assert!(compare(&missing, &empty).is_err());
}
#[test]
fn incomplete_extra_and_nonversion1_actual_store_inventory_refuses() {
    let store = MemoryStore::new(Default::default());
    assert!(read_rows(&store, "fixture", 2).is_err());
    store
        .put_provider_state(record("fixture", "A", b"first".to_vec()), None)
        .unwrap();
    store
        .put_provider_state(record("fixture", "B", b"second".to_vec()), None)
        .unwrap();
    assert_eq!(read_rows(&store, "fixture", 2).unwrap().len(), 2);
    store
        .put_provider_state(record("fixture", "C", b"extra".to_vec()), None)
        .unwrap();
    assert!(read_rows(&store, "fixture", 2).is_err());
    let store = MemoryStore::new(Default::default());
    let mut bad = record("fixture", "A", b"revision".to_vec());
    store.put_provider_state(bad.clone(), None).unwrap();
    bad.version = 2;
    store.put_provider_state(bad, Some(1)).unwrap();
    assert!(read_rows(&store, "fixture", 1).is_err());
}
#[test]
fn uncertain_drain_missing_snapshot_and_unknown_destination_preserve_owned_state() {
    let temp = Temporary::create();
    for shutdowns in [
        serde_json::json!([true]),
        serde_json::json!([false, true]),
        serde_json::json!([true, false]),
        serde_json::json!([true, null]),
        serde_json::json!([true, true, true]),
    ] {
        let owner = Owner::claim(
            temp.0
                .join(format!("case{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))),
        )
        .unwrap();
        let mut report = ready();
        report["shutdowns"] = shutdowns;
        assert!(owner.cleanup(&mut report).is_err());
        assert!(owner.owned.is_dir());
    }
    let owner = Owner::claim(temp.0.join("snapshots")).unwrap();
    let mut report = ready();
    report["snapshots"].as_array_mut().unwrap().pop();
    assert!(owner.cleanup(&mut report).is_err());
    assert!(owner.owned.is_dir());
    let owner = Owner::claim(temp.0.join("unknown")).unwrap();
    fs::write(owner.evidence.join("full-catalog.json"), b"unowned receipt").unwrap();
    assert!(owner.cleanup(&mut ready()).is_err());
    assert!(owner.owned.is_dir());
    assert_eq!(
        fs::read(owner.evidence.join("full-catalog.json")).unwrap(),
        b"unowned receipt"
    );
}
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
#[test]
fn replaced_owned_root_preserves_replacement_and_refuses_cleanup() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    let original = owner.evidence.join("retained-original");
    fs::rename(&owner.owned, &original).unwrap();
    fs::create_dir(&owner.owned).unwrap();
    fs::write(owner.owned.join("sentinel"), b"unknown bytes").unwrap();
    assert!(owner.cleanup(&mut ready()).is_err());
    assert_eq!(
        fs::read(owner.owned.join("sentinel")).unwrap(),
        b"unknown bytes"
    );
    assert!(original.is_dir());
}
#[test]
fn replaced_root_staging_or_final_receipt_refuses_successful_publication() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("root")).unwrap();
    let original = temp.0.join("original-root");
    assert!(
        owner
            .save_observed(
                &ready(),
                || {
                    fs::rename(&owner.evidence, &original).unwrap();
                    fs::create_dir(&owner.evidence).unwrap();
                },
                || {}
            )
            .is_err()
    );
    assert!(original.join("full-catalog.json.next").is_file());
    assert!(!owner.evidence.join("full-catalog.json").exists());
    let owner = Owner::claim(temp.0.join("staging")).unwrap();
    let staging = owner.evidence.join("full-catalog.json.next");
    assert!(
        owner
            .save_observed(
                &ready(),
                || {
                    let size = fs::metadata(&staging).unwrap().len() as usize;
                    fs::write(&staging, vec![b'X'; size]).unwrap();
                },
                || {}
            )
            .is_err()
    );
    assert!(!owner.evidence.join("full-catalog.json").exists());
    let owner = Owner::claim(temp.0.join("final")).unwrap();
    owner.save(&ready()).unwrap();
    let final_path = owner.evidence.join("full-catalog.json");
    fs::rename(&final_path, owner.evidence.join("retained-prior")).unwrap();
    fs::write(&final_path, b"unowned final bytes").unwrap();
    assert!(owner.save(&ready()).is_err());
    assert_eq!(fs::read(&final_path).unwrap(), b"unowned final bytes");
}
#[test]
fn actual_exact_cleanup_retains_only_diagnostic_status_and_no_observation_tokens() {
    let temp = Temporary::create();
    let owner = Owner::claim(temp.0.join("evidence")).unwrap();
    owner.cleanup(&mut ready()).unwrap();
    assert!(absence(fs::symlink_metadata(&owner.owned)).is_ok());
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(owner.evidence.join("full-catalog.json")).unwrap())
            .unwrap();
    assert_eq!(report["issue_observations"], serde_json::json!([]));
    assert_eq!(report["journey_observations"], serde_json::json!([]));
    assert_eq!(
        fs::metadata(owner.evidence.join("full-catalog.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}
#[test]
fn dangling_symlink_and_uncertain_absence_are_not_missing_owned_state() {
    let temp = Temporary::create();
    let link = temp.0.join("dangling");
    std::os::unix::fs::symlink(temp.0.join("absent"), &link).unwrap();
    assert!(absence(fs::symlink_metadata(&link)).is_err());
    assert!(
        absence(Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )))
        .is_err()
    );
    assert!(absence(Err(std::io::Error::from(std::io::ErrorKind::NotFound))).is_ok());
}

#[tokio::test]
#[ignore = "exact pinned corpus and fresh external FULL_CATALOG_EVIDENCE; serial original1800s/64MiB supervision"]
async fn corpus_full_installed_catalog_projection_reopens_without_issue_credit() {
    let corpus =
        PathBuf::from(env::var_os(CORPUS_ENV).expect("pinned first-party corpus required"));
    let evidence = PathBuf::from(
        env::var_os("CARDDEMO_FULL_CATALOG_EVIDENCE")
            .expect("fresh external owned evidence required"),
    );
    let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let result = run(&corpus, &inventory, evidence.clone()).await.unwrap();
    assert_eq!(result["snapshots"].as_array().unwrap().len(), 3);
    assert_eq!(result["shutdowns"], serde_json::json!([true, true]));
    assert_eq!(result["issue_observations"], serde_json::json!([]));
    assert_eq!(result["journey_observations"], serde_json::json!([]));
    assert_eq!(result["application_credit"], 0);
    assert!(absence(fs::symlink_metadata(evidence.join("owned-state"))).is_ok());
}
