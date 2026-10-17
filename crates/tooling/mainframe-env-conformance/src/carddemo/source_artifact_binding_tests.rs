//! Literal synthetic refusal controls; no corpus, compiler, install, or acceptance credit.
use super::*;
use mainframe_env_compiler_api::{ArtifactManifest, CompileOptions, CompileTarget};
use mainframe_env_store_api::{ProviderStateStore, ProviderStateWrite};

fn definition() -> OnlineProgramDefinition {
    let payload = b"independent selected publisher fixture".to_vec();
    OnlineProgramDefinition {
        name: "COSGN00C".into(),
        artifact: ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(&payload)),
            InvocationLimits::default(),
        )
        .unwrap(),
        payload,
        manifest: VersionedArtifactManifest::V3(ArtifactManifest {
            compiler_generation: "synthetic-compiler".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
            host_interfaces: BTreeSet::from(["mainframe-env.host@1".into()]),
            ir_contract: "mainframe-env.core-mir@1".into(),
            dialect_contracts: BTreeSet::from(["mainframe-env.cobol@1".into()]),
        }),
        semantic_identity: format!("semantic-sha256:{}", "a".repeat(64)),
    }
}
fn literal_snapshot() -> InstalledSnapshot {
    let definition = definition();
    let program = ProviderStateRecord {
        namespace: "online-program".into(),
        key: "COSGN00C".into(),
        version: 1,
        payload: definition.artifact.as_str().as_bytes().to_vec(),
    };
    let transaction = ProviderStateRecord {
        namespace: "online-transaction".into(),
        key: "CC00".into(),
        version: 1,
        payload: b"COSGN00C".to_vec(),
    };
    InstalledSnapshot {
        programs: BTreeMap::from([("COSGN00C".into(), program)]),
        transactions: BTreeMap::from([("CC00".into(), transaction)]),
        artifacts: BTreeMap::from([("COSGN00C".into(), publisher_record(&definition).unwrap())]),
        envelope_identity: String::new(),
    }
}
fn no_binding_from_mismatch(expected: &InstalledSnapshot, actual: &InstalledSnapshot) {
    let mut observations = RouteObservations::default();
    let result = (|| {
        compare_snapshot(expected, actual)?;
        InstallationProof {
            snapshot: actual.clone(),
        }
        .bind(&mut observations)
    })();
    assert!(result.is_err());
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
}

#[test]
fn missing_extra_swapped_reference_and_transaction_mappings_refuse() {
    let expected = literal_snapshot();
    assert!(compare_snapshot(&expected, &expected).is_ok());
    let mut cases = Vec::new();
    let mut actual = expected.clone();
    actual.programs.clear();
    cases.push(actual);
    let mut actual = expected.clone();
    actual
        .programs
        .insert("EXTRA".into(), expected.programs["COSGN00C"].clone());
    cases.push(actual);
    let mut actual = expected.clone();
    actual.programs.get_mut("COSGN00C").unwrap().payload =
        format!("sha256:{}", "b".repeat(64)).into_bytes();
    cases.push(actual);
    let mut actual = expected.clone();
    let row = actual.programs.remove("COSGN00C").unwrap();
    actual.programs.insert("OTHER".into(), row);
    cases.push(actual);
    let mut actual = expected.clone();
    actual.transactions.clear();
    cases.push(actual);
    let mut actual = expected.clone();
    actual
        .transactions
        .insert("XX00".into(), expected.transactions["CC00"].clone());
    cases.push(actual);
    let mut actual = expected.clone();
    actual.transactions.get_mut("CC00").unwrap().payload = b"OTHER".to_vec();
    cases.push(actual);
    for actual in cases {
        no_binding_from_mismatch(&expected, &actual);
    }
}

#[test]
fn state_namespace_key_and_version_drift_refuse_for_each_catalog() {
    let expected = literal_snapshot();
    for namespace in ["program", "transaction"] {
        for which in 0..3 {
            let mut actual = expected.clone();
            let row = if namespace == "program" {
                actual.programs.get_mut("COSGN00C").unwrap()
            } else {
                actual.transactions.get_mut("CC00").unwrap()
            };
            match which {
                0 => row.namespace.push('X'),
                1 => row.key.push('X'),
                _ => row.version += 1,
            }
            no_binding_from_mismatch(&expected, &actual);
        }
    }
}

#[test]
fn complete_payload_reference_media_digest_and_metadata_drift_refuse() {
    let expected = literal_snapshot();
    for which in 0..17 {
        let mut actual = expected.clone();
        let artifact = actual.artifacts.get_mut("COSGN00C").unwrap();
        match which {
            0 => artifact.payload[0] ^= 1,
            1 => artifact.payload_digest[0] ^= 1,
            2 => {
                artifact.artifact = ArtifactRef::new(
                    format!("sha256:{}", "b".repeat(64)),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
            3 => artifact.media_type.push('X'),
            4 => artifact.executable = None,
            _ => {
                let metadata = artifact.executable.as_mut().unwrap();
                match which {
                    5 => metadata.artifact_contract.push('X'),
                    6 => metadata.compatibility_profile.push('X'),
                    7 => metadata.compiler_generation.push('X'),
                    8 => metadata.target.push('X'),
                    9 => {
                        metadata.options.insert("changed".into(), "true".into());
                    }
                    10 => {
                        metadata.host_interfaces.insert("other@1".into());
                    }
                    11 => metadata.ir_contract.push('X'),
                    12 => metadata.dialect_contracts = None,
                    13 => {
                        metadata
                            .dialect_contracts
                            .as_mut()
                            .unwrap()
                            .insert("other@1".into());
                    }
                    14 => {
                        metadata.semantic_identity = format!("semantic-sha256:{}", "b".repeat(64))
                    }
                    15 => metadata.manifest_payload_digest[0] ^= 1,
                    _ => {
                        // Even a self-consistent payload/metadata rebind is wrong
                        // when it differs from the source-selected publisher.
                        artifact.payload[0] ^= 1;
                        artifact.payload_digest = Sha256::digest(&artifact.payload).into();
                        artifact.artifact = ArtifactRef::new(
                            format!("sha256:{:x}", Sha256::digest(&artifact.payload)),
                            InvocationLimits::default(),
                        )
                        .unwrap();
                        *metadata = metadata.clone().bind_to_payload(&artifact.payload_digest);
                    }
                }
            }
        }
        no_binding_from_mismatch(&expected, &actual);
    }
    let mut actual = expected.clone();
    actual.artifacts.clear();
    no_binding_from_mismatch(&expected, &actual);
    let mut actual = expected.clone();
    actual
        .artifacts
        .insert("EXTRA".into(), expected.artifacts["COSGN00C"].clone());
    no_binding_from_mismatch(&expected, &actual);
}

#[test]
fn frozen_source_same_count_bytes_missing_extra_format_encoding_and_library_order_refuse() {
    let digest = format!("{:x}", Sha256::digest(b"independent frozen source"));
    assert!(require_pinned_bytes("source", b"independent frozen source", &digest).is_ok());
    assert!(require_pinned_bytes("source", b"independent frozen sourcf", &digest).is_err());
    let expected = BTreeMap::from([
        (
            "primary.cbl".into(),
            (digest.clone(), SourceFormat::Fixed, SourceEncoding::Utf8),
        ),
        (
            "copy.cpy".into(),
            (digest, SourceFormat::Fixed, SourceEncoding::Utf8),
        ),
    ]);
    assert!(compare_source_files(&expected, &expected).is_ok());
    for which in 0..5 {
        let mut actual = expected.clone();
        match which {
            0 => {
                actual.remove("copy.cpy");
            }
            1 => {
                actual.insert("extra.cpy".into(), expected["copy.cpy"].clone());
            }
            2 => actual.get_mut("copy.cpy").unwrap().0.push('X'),
            3 => actual.get_mut("copy.cpy").unwrap().1 = SourceFormat::Free,
            _ => actual.get_mut("copy.cpy").unwrap().2 = SourceEncoding::Ebcdic(37),
        }
        assert!(compare_source_files(&expected, &actual).is_err());
    }
    let limits = SourceLimits::default();
    let a = SourceLibrary::new(
        "application-00",
        vec![LogicalPath::new("a.cpy", 512).unwrap()],
        limits,
    )
    .unwrap();
    let b = SourceLibrary::new(
        "application-01",
        vec![LogicalPath::new("b.cpy", 512).unwrap()],
        limits,
    )
    .unwrap();
    assert!(compare_source_libraries(&[a.clone(), b.clone()], &[a.clone(), b.clone()]).is_ok());
    assert!(compare_source_libraries(&[a.clone(), b.clone()], &[b.clone(), a.clone()]).is_err());
    assert!(compare_source_libraries(&[a.clone(), b], &[a]).is_err());
}

fn package_receipt() -> CardDemoPackageReceipt {
    CardDemoPackageReceipt {
        schema_version: "mainframe-env.carddemo-package-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: pins::CORPUS_COMMIT.into(),
        package_name: "AWS-CARDDEMO".into(),
        package_version: "0.1.1".into(),
        package_identity: pins::COHORT_IDENTITY.into(),
        entries: pins::COHORT_ENTRIES
            .iter()
            .map(|(path, digest)| ((*path).into(), (*digest).into()))
            .collect(),
        staged_not_ready: true,
        committed_ready: true,
        idempotent_reinstall: true,
        negative_controls: 5,
    }
}
#[test]
fn stale_cohort_source_identity_entries_and_readiness_refuse() {
    let expected = package_receipt();
    assert!(validate_package(&expected).is_ok());
    for which in 0..11 {
        let mut actual = expected.clone();
        match which {
            0 => actual.schema_version.push('X'),
            1 => actual.status = "failed".into(),
            2 => actual.corpus_commit = "0".repeat(40),
            3 => actual.package_name.push('X'),
            4 => actual.package_version.push('X'),
            5 => actual.package_identity = format!("sha256:{}", "b".repeat(64)),
            6 => {
                actual.entries.remove("cohorts/source");
            }
            7 => {
                actual.entries.insert(
                    "cohorts/source".into(),
                    format!("sha256:{}", "b".repeat(64)),
                );
            }
            8 => actual.staged_not_ready = false,
            9 => actual.committed_ready = false,
            _ => actual.negative_controls = 4,
        }
        assert!(validate_package(&actual).is_err());
    }
    let mut actual = expected;
    actual.idempotent_reinstall = false;
    assert!(validate_package(&actual).is_err());
}

#[test]
fn bounded_durable_scan_retains_exact_rows_and_detects_overflow_or_wrong_version() {
    let store = MemoryStore::new(Default::default());
    let rows = literal_snapshot().programs;
    store
        .put_provider_states_atomic(
            rows.values()
                .cloned()
                .map(|record| ProviderStateWrite {
                    record,
                    expected_version: None,
                })
                .collect(),
        )
        .unwrap();
    assert_eq!(read_states(&store, "online-program", 2).unwrap(), rows);
    let mut extra = rows["COSGN00C"].clone();
    extra.key = "EXTRA".into();
    store
        .put_provider_states_atomic(vec![ProviderStateWrite {
            record: extra,
            expected_version: None,
        }])
        .unwrap();
    assert_ne!(read_states(&store, "online-program", 2).unwrap(), rows);
    let mut changed = rows["COSGN00C"].clone();
    changed.version = 2;
    store
        .put_provider_states_atomic(vec![ProviderStateWrite {
            record: changed,
            expected_version: Some(1),
        }])
        .unwrap();
    assert!(read_states(&store, "online-program", 3).is_err());
}

#[test]
fn incomplete_selected_install_proof_cannot_emit_installed_binding() {
    let mut observations = RouteObservations::default();
    assert!(
        InstallationProof {
            snapshot: literal_snapshot()
        }
        .bind(&mut observations)
        .is_err()
    );
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
}

#[test]
fn reopen_requires_same_source_envelope_state_and_full_artifacts() {
    let mut initial = literal_snapshot();
    initial.envelope_identity = format!("sha256:{}", "a".repeat(64));
    assert!(compare_reopened(&initial, &initial).is_ok());
    for which in 0..4 {
        let mut reopened = initial.clone();
        match which {
            0 => reopened.envelope_identity = format!("sha256:{}", "b".repeat(64)),
            1 => reopened.programs.get_mut("COSGN00C").unwrap().version += 1,
            2 => reopened.transactions.get_mut("CC00").unwrap().payload = b"OTHER".to_vec(),
            _ => reopened.artifacts.get_mut("COSGN00C").unwrap().payload[0] ^= 1,
        }
        let mut observations = RouteObservations::default();
        let result = (|| {
            compare_reopened(&initial, &reopened)?;
            InstallationProof { snapshot: reopened }.bind(&mut observations)
        })();
        assert!(result.is_err());
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }
}

#[test]
fn primary_source_publisher_name_or_content_reference_mismatch_refuses() {
    let limits = SourceLimits::default();
    let path = LogicalPath::new("app/cbl/COSGN00C.cbl", limits.max_path_bytes).unwrap();
    let bundle = SourceBundle::new(
        &path,
        vec![
            SourceFile::input(
                path.as_str(),
                b"synthetic source".to_vec(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .unwrap();
    assert!(source_binding("OTHER", path.as_str(), &bundle).is_err());
    assert!(source_binding("COSGN00C", "app/cbl/OTHER.cbl", &bundle).is_err());
    // A source-selected name does not excuse missing/wrong frozen copybook bytes.
    assert!(source_binding("COSGN00C", path.as_str(), &bundle).is_err());
    let mut expected = SourceInstallExpectation {
        package: package_receipt(),
        programs: BTreeMap::new(),
        transactions: BTreeMap::new(),
    };
    let mut program = definition();
    program.name = "OTHER".into();
    assert!(
        expected
            .add_program("COSGN00C", path.as_str(), &bundle, &program)
            .is_err()
    );
    assert!(expected.programs.is_empty());
    let mut program = definition();
    program.payload[0] ^= 1;
    assert!(publisher_record(&program).is_err());
    let mut program = definition();
    if let VersionedArtifactManifest::V3(manifest) = &mut program.manifest {
        manifest.target = CompileTarget::new("other").unwrap();
    }
    assert!(publisher_record(&program).is_err());
}

#[test]
fn catalog_combination_refuses_unbound_or_incomplete_actual_program_mappings() {
    let snapshot = literal_snapshot();
    assert!(
        InstallationProof {
            snapshot: snapshot.clone()
        }
        .require_resource_mappings()
        .is_err()
    );
    let mut full = InstalledSnapshot {
        programs: BTreeMap::new(),
        transactions: BTreeMap::new(),
        artifacts: BTreeMap::new(),
        envelope_identity: format!("sha256:{}", "a".repeat(64)),
    };
    for name in pinned_programs() {
        let record = snapshot.artifacts["COSGN00C"].clone();
        full.programs.insert(
            name.clone(),
            ProviderStateRecord {
                namespace: "online-program".into(),
                key: name.clone(),
                version: 1,
                payload: record.artifact.as_str().as_bytes().to_vec(),
            },
        );
        full.artifacts.insert(name, record);
    }
    for (transaction, program) in pinned_transactions() {
        full.transactions.insert(
            transaction.clone(),
            ProviderStateRecord {
                namespace: "online-transaction".into(),
                key: transaction,
                version: 1,
                payload: program.into_bytes(),
            },
        );
    }
    // Shape completeness is necessary but every actual mapping/content binding
    // must still hold. These synthetic controls do not call the token binder.
    for which in 0..4 {
        let mut actual = full.clone();
        match which {
            0 => actual.transactions.get_mut("CC00").unwrap().payload = b"OTHER".to_vec(),
            1 => actual.programs.get_mut("COSGN00C").unwrap().payload = b"wrong-ref".to_vec(),
            2 => actual.artifacts.get_mut("COSGN00C").unwrap().payload[0] ^= 1,
            _ => {
                actual
                    .artifacts
                    .get_mut("COSGN00C")
                    .unwrap()
                    .executable
                    .as_mut()
                    .unwrap()
                    .manifest_payload_digest[0] ^= 1
            }
        }
        assert!(
            InstallationProof { snapshot: actual }
                .require_resource_mappings()
                .is_err()
        );
    }
}

#[test]
fn offered_duplicate_program_or_transaction_swap_cannot_finish_source_binding() {
    let mut expected = SourceInstallExpectation {
        package: package_receipt(),
        programs: BTreeMap::new(),
        transactions: BTreeMap::new(),
    };
    let mut programs = Vec::new();
    for name in pinned_programs() {
        let mut program = definition();
        program.name = name.clone();
        expected.programs.insert(
            name,
            ExpectedProgram {
                source: serde_json::json!({"authority":"synthetic zero-credit refusal control"}),
                record: publisher_record(&program).unwrap(),
            },
        );
        programs.push(program);
    }
    let mut offered = OnlineApplicationDefinition {
        programs,
        transactions: pinned_transactions(),
        maps: Vec::new(),
    };
    let saved = offered.programs[0].clone();
    offered.programs[0] = offered.programs[1].clone();
    assert!(expected.finish(&offered).is_err());
    assert!(expected.transactions.is_empty());
    offered.programs[0] = saved;
    offered
        .transactions
        .insert("CC00".into(), "COADM01C".into());
    assert!(expected.finish(&offered).is_err());
    assert!(expected.transactions.is_empty());
}

#[test]
fn retained_versioned_manifest_preserves_variants_and_every_offered_field() {
    let legacy = mainframe_env_compiler_api::ArtifactManifestV2 {
        compiler_generation: "synthetic-compiler@7".into(),
        target: CompileTarget::new("synthetic-target.v2").unwrap(),
        options: CompileOptions::new(BTreeMap::from([
            ("alpha".into(), "first\tvalue".into()),
            ("omega".into(), "last\nvalue".into()),
        ]))
        .unwrap(),
        host_interfaces: BTreeSet::from(["synthetic.host-z@2".into(), "synthetic.host-a@1".into()]),
        ir_contract: "synthetic.ir@3".into(),
    };
    let current = ArtifactManifest {
        compiler_generation: legacy.compiler_generation.clone(),
        target: legacy.target.clone(),
        options: legacy.options.clone(),
        host_interfaces: legacy.host_interfaces.clone(),
        ir_contract: legacy.ir_contract.clone(),
        dialect_contracts: BTreeSet::from([
            "synthetic.dialect-z@2".into(),
            "synthetic.dialect-a@1".into(),
        ]),
    };
    let expected_v2 = serde_json::json!({"V2": {
        "compiler_generation": "synthetic-compiler@7",
        "target": "synthetic-target.v2",
        "options": {"alpha": "first\tvalue", "omega": "last\nvalue"},
        "host_interfaces": ["synthetic.host-a@1", "synthetic.host-z@2"],
        "ir_contract": "synthetic.ir@3",
    }});
    let expected_v3 = serde_json::json!({"V3": {
        "compiler_generation": "synthetic-compiler@7",
        "target": "synthetic-target.v2",
        "options": {"alpha": "first\tvalue", "omega": "last\nvalue"},
        "host_interfaces": ["synthetic.host-a@1", "synthetic.host-z@2"],
        "ir_contract": "synthetic.ir@3",
        "dialect_contracts": ["synthetic.dialect-a@1", "synthetic.dialect-z@2"],
    }});
    assert_ne!(expected_v2, expected_v3);
    for (offered, expected, fields) in [
        (
            VersionedArtifactManifest::V2(legacy),
            expected_v2.clone(),
            5,
        ),
        (
            VersionedArtifactManifest::V3(current.clone()),
            expected_v3,
            6,
        ),
    ] {
        assert_eq!(retain_manifest(&offered), expected);
        for field in 0..fields {
            let mut changed = offered.clone();
            let (generation, target, options, hosts, ir, dialects) = match &mut changed {
                VersionedArtifactManifest::V2(value) => (
                    &mut value.compiler_generation,
                    &mut value.target,
                    &mut value.options,
                    &mut value.host_interfaces,
                    &mut value.ir_contract,
                    None,
                ),
                VersionedArtifactManifest::V3(value) => (
                    &mut value.compiler_generation,
                    &mut value.target,
                    &mut value.options,
                    &mut value.host_interfaces,
                    &mut value.ir_contract,
                    Some(&mut value.dialect_contracts),
                ),
            };
            match field {
                0 => generation.push('X'),
                1 => *target = CompileTarget::new("changed-target.v3").unwrap(),
                2 => {
                    let mut values = options.values().clone();
                    values.get_mut("omega").unwrap().push('X');
                    *options = CompileOptions::new(values).unwrap();
                }
                3 => {
                    hosts.remove("synthetic.host-z@2");
                }
                4 => ir.push('X'),
                _ => {
                    dialects.unwrap().remove("synthetic.dialect-z@2");
                }
            }
            assert_ne!(retain_manifest(&changed), expected);
        }
    }
    let mut empty_current = current;
    empty_current.dialect_contracts.clear();
    let retained = retain_manifest(&VersionedArtifactManifest::V3(empty_current));
    assert_eq!(retained["V3"]["dialect_contracts"], serde_json::json!([]));
    assert_ne!(retained, expected_v2);
}
