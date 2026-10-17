//! Explicit historical-domain and current-admission compatibility controls.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn identity_framing_legacy_ambiguity_remains_frozen_but_fresh_legacy_refuses() {
    let (mut first, mut second) = identity_framing_graph_pair(1);
    first.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    second.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    resign(&mut first);
    second.signature = first.signature.clone();
    validate_sections(&first, PackageLimits::default()).unwrap();
    validate_sections(&second, PackageLimits::default()).unwrap();
    let frozen = "sha256:b3c2763fbfb542196f71bb176766af97c9b1118b820626226df960a45167a0fb";
    assert_eq!(package_v2_identity(&first).unwrap(), frozen);
    assert_eq!(package_v2_identity(&second).unwrap(), frozen);
    assert_eq!(package_generation_identity(&first).unwrap(), frozen);
    assert_eq!(package_generation_identity(&second).unwrap(), frozen);
    assert!(TestVerifier.verify(
        &second.signature.key_id,
        &second.signature.algorithm,
        frozen,
        &second.signature.value,
    ));
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
    let before = installer.state_payload().unwrap();
    for candidate in [&first, &second] {
        assert_eq!(
            installer.stage(candidate),
            Err(InstallProblem::InvalidIdentity)
        );
        assert_eq!(installer.state_payload().unwrap(), before);
    }
}

#[test]
fn identity_framing_exact_legacy_retry_checks_full_owned_graph_before_acknowledging() {
    let (mut first, mut substitute) = identity_framing_graph_pair(1);
    first.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    substitute.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    resign(&mut first);
    substitute.signature = first.signature.clone();
    let payload = trusted_state_fixture(vec![(first.clone(), InstallState::Ready)], Some(1));
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(TestVerifier),
        &payload,
    )
    .unwrap();
    assert_eq!(installer.state_payload().unwrap(), payload);
    let selected = installer.selected("DEMO").unwrap().unwrap();
    assert_eq!(installer.stage(&first).unwrap(), selected);
    assert_eq!(installer.commit(&first).unwrap(), selected);
    assert_eq!(installer.install(&first).unwrap(), selected);
    assert_eq!(
        installer.stage(&substitute),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(
        installer.commit(&substitute),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(
        installer.install(&substitute),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(installer.state_payload().unwrap(), payload);
    assert_eq!(
        installer
            .selected_package("DEMO")
            .unwrap()
            .unwrap()
            .as_ref(),
        &first
    );
}

#[test]
fn identity_framing_legacy_retry_includes_original_signature_even_if_another_is_valid() {
    struct RotatedVerifier;
    impl PackageSignatureVerifier for RotatedVerifier {
        fn verify(&self, key: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
            TestVerifier.verify(key, algorithm, identity, signature)
                || (key == "rotated-key"
                    && algorithm == "test-signature@1"
                    && signature == format!("rotated:{identity}"))
        }
    }
    let first = legacy_package(1);
    let payload = trusted_state_fixture(vec![(first.clone(), InstallState::Ready)], Some(1));
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(RotatedVerifier),
        &payload,
    )
    .unwrap();
    let mut rotated = first.clone();
    rotated.signature.key_id = "rotated-key".into();
    rotated.signature.value = format!("rotated:{}", package_v2_identity(&first).unwrap());
    assert_eq!(
        package_v2_identity(&rotated).unwrap(),
        package_v2_identity(&first).unwrap()
    );
    assert_eq!(
        installer.stage(&rotated),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(
        installer.commit(&rotated),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(installer.state_payload().unwrap(), payload);
}

#[test]
fn identity_framing_mixed_trusted_recovery_preserves_domains_state_and_runtime_rollback() {
    let legacy = legacy_package(1);
    let current = package(2);
    let staged = package(3);
    let payload = trusted_state_fixture(
        vec![
            (legacy.clone(), InstallState::Ready),
            (current.clone(), InstallState::Ready),
            (staged.clone(), InstallState::Staged),
        ],
        Some(2),
    );
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(TestVerifier),
        &payload,
    )
    .unwrap();
    assert_eq!(installer.state_payload().unwrap(), payload);
    let state: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(
        state["schema_version"],
        APPLICATION_INSTALLER_STATE_CONTRACT
    );
    assert_eq!(state.as_object().unwrap().len(), 2);
    assert_eq!(
        installer
            .selected_package("DEMO")
            .unwrap()
            .unwrap()
            .as_ref(),
        &current
    );
    let identity = package_v2_identity(&legacy).unwrap();
    let old = installer
        .retained_generation("DEMO", 1, &identity)
        .unwrap()
        .unwrap();
    assert_eq!(old.package(), &legacy);
    assert_eq!(installer.rollback("DEMO", 1).unwrap().identity, identity);
    assert_eq!(
        installer
            .selected_package("DEMO")
            .unwrap()
            .unwrap()
            .as_ref(),
        &legacy
    );
    let rollback_payload = installer.state_payload().unwrap();
    let reopened = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(TestVerifier),
        &rollback_payload,
    )
    .unwrap();
    assert_eq!(reopened.state_payload().unwrap(), rollback_payload);
    assert_eq!(
        reopened.commit(&current).unwrap().state,
        InstallState::Ready
    );
    assert_eq!(reopened.state_payload().unwrap(), rollback_payload);
    assert_eq!(
        reopened.rollback("DEMO", 3),
        Err(InstallProblem::UnknownStage)
    );
    let ready = reopened.commit(&staged).unwrap();
    assert_eq!(ready.generation, 3);
    assert_eq!(ready.state, InstallState::Ready);
}

#[test]
fn identity_framing_trusted_legacy_and_current_ready_null_selection_is_preserved() {
    let payload = trusted_state_fixture(
        vec![
            (legacy_package(1), InstallState::Ready),
            (package(2), InstallState::Ready),
        ],
        None,
    );
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(TestVerifier),
        &payload,
    )
    .unwrap();
    assert_eq!(installer.state_payload().unwrap(), payload);
    assert_eq!(installer.selected("DEMO").unwrap(), None);
    assert_eq!(installer.rollback("DEMO", 1).unwrap().generation, 1);
}

struct RevocableVerifier(AtomicBool);
impl PackageSignatureVerifier for RevocableVerifier {
    fn verify(&self, key: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
        !self.0.load(Ordering::SeqCst) && TestVerifier.verify(key, algorithm, identity, signature)
    }
}

#[test]
fn identity_framing_current_trust_revocation_applies_to_ready_retries_and_recovery() {
    let verifier = Arc::new(RevocableVerifier(AtomicBool::new(false)));
    let legacy = legacy_package(1);
    let current = package(2);
    let payload = trusted_state_fixture(
        vec![
            (legacy.clone(), InstallState::Ready),
            (current.clone(), InstallState::Ready),
        ],
        Some(2),
    );
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        verifier.clone(),
        &payload,
    )
    .unwrap();
    verifier.0.store(true, Ordering::SeqCst);
    for candidate in [&legacy, &current] {
        assert_eq!(
            installer.stage(candidate),
            Err(InstallProblem::InvalidSignature)
        );
        assert_eq!(
            installer.commit(candidate),
            Err(InstallProblem::InvalidSignature)
        );
        assert_eq!(
            installer.install(candidate),
            Err(InstallProblem::InvalidSignature)
        );
        assert_eq!(installer.state_payload().unwrap(), payload);
    }
    assert!(matches!(
        ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            verifier.clone(),
            &payload,
        ),
        Err(InstallProblem::InvalidSignature)
    ));
    verifier.0.store(false, Ordering::SeqCst);
    assert_eq!(installer.stage(&legacy).unwrap().state, InstallState::Ready);
    assert_eq!(
        installer.stage(&current).unwrap().state,
        InstallState::Ready
    );
}

#[test]
fn identity_framing_unknown_and_downgraded_domains_never_fall_back() {
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
    let original = package(1);
    installer.install(&original).unwrap();
    let before = installer.state_payload().unwrap();
    assert_eq!(
        package_v2_identity(&original),
        Err(InstallProblem::InvalidIdentity)
    );
    for domain in [
        "mainframe-env.application-package@1",
        "mainframe-env.application-package@4",
        "",
    ] {
        let mut unknown = package(2);
        unknown.sections.schema_version = domain.into();
        assert_eq!(
            package_generation_identity(&unknown),
            Err(InstallProblem::InvalidIdentity)
        );
        assert_eq!(
            installer.stage(&unknown),
            Err(InstallProblem::InvalidIdentity)
        );
        let payload = trusted_state_fixture(vec![(unknown, InstallState::Ready)], Some(2));
        assert!(matches!(
            ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                Arc::new(TestVerifier),
                &payload,
            ),
            Err(InstallProblem::InvalidIdentity)
        ));
    }
    let mut downgraded = package(2);
    downgraded.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    assert_eq!(
        installer.stage(&downgraded),
        Err(InstallProblem::InvalidSignature)
    );
    resign(&mut downgraded);
    assert_eq!(
        installer.stage(&downgraded),
        Err(InstallProblem::InvalidIdentity)
    );
    let mut relabeled = legacy_package(1);
    relabeled.sections.schema_version = APPLICATION_PACKAGE_V3_CONTRACT.into();
    assert_eq!(
        installer.stage(&relabeled),
        Err(InstallProblem::InvalidSignature)
    );
    assert_eq!(installer.state_payload().unwrap(), before);
}

#[test]
fn identity_framing_explicit_new_generation_preserves_original_legacy_record() {
    let legacy = legacy_package(1);
    let payload = trusted_state_fixture(vec![(legacy.clone(), InstallState::Ready)], Some(1));
    let installer = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(TestVerifier),
        &payload,
    )
    .unwrap();
    assert_eq!(
        installer.stage(&package(1)),
        Err(InstallProblem::IdentityConflict)
    );
    assert_eq!(installer.state_payload().unwrap(), payload);
    installer.install(&package(2)).unwrap();
    let identity = package_v2_identity(&legacy).unwrap();
    assert_eq!(
        installer
            .retained_generation("DEMO", 1, &identity)
            .unwrap()
            .unwrap()
            .package(),
        &legacy
    );
    assert_eq!(installer.rollback("DEMO", 1).unwrap().identity, identity);
}

#[test]
fn identity_framing_changed_valid_base_dependencies_refuse_the_original_signature() {
    let mut original = package(1);
    original.base.manifest.entries[2]
        .depends_on
        .push("resource/1".into());
    resign(&mut original);
    let mut changed = original.clone();
    changed.base.manifest.entries[2].depends_on.clear();
    changed.base.manifest.entries[3]
        .depends_on
        .push("resource/1".into());
    validate_package(&changed.base, "0.2.0").unwrap();
    assert_ne!(
        package_generation_identity(&original).unwrap(),
        package_generation_identity(&changed).unwrap()
    );
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
    installer.install(&original).unwrap();
    let before = installer.state_payload().unwrap();
    assert_eq!(
        installer.stage(&changed),
        Err(InstallProblem::InvalidSignature)
    );
    assert_eq!(
        installer.commit(&changed),
        Err(InstallProblem::InvalidSignature)
    );
    assert_eq!(installer.state_payload().unwrap(), before);
}

#[test]
fn identity_framing_semantic_vector_order_and_sorted_row_order_keep_their_distinction() {
    let mut original = package(1);
    original.sections.sql_tables[0]
        .primary_key
        .push("VALUE".into());
    original.sections.sql_rows.push(SqlSeedRow {
        table: "APP.TABLE".into(),
        values: BTreeMap::from([("ID".into(), "2".into())]),
    });
    original.base.manifest.entries[2]
        .depends_on
        .push("resource/1".into());
    resign(&mut original);
    let identity = package_generation_identity(&original).unwrap();
    let mut reordered = original.clone();
    reordered.sections.sql_rows.reverse();
    assert_eq!(package_generation_identity(&reordered).unwrap(), identity);
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.sections.sql_tables[0].columns.reverse();
    changes.push(changed);
    let mut changed = original.clone();
    changed.sections.sql_tables[0].primary_key.reverse();
    changes.push(changed);
    let mut changed = original.clone();
    changed.base.manifest.entries.reverse();
    changes.push(changed);
    let mut changed = original.clone();
    changed.base.manifest.entries[2].depends_on.reverse();
    changes.push(changed);
    for changed in changes {
        validate_sections(&changed, PackageLimits::default()).unwrap();
        validate_package(&changed.base, "0.2.0").unwrap();
        assert_ne!(package_generation_identity(&changed).unwrap(), identity);
        assert!(matches!(
            validate_v2(&changed, "0.2.0", PackageLimits::default(), &TestVerifier),
            Err(InstallProblem::InvalidSignature)
        ));
    }
}

#[test]
fn identity_framing_optional_absent_null_and_present_forms_are_version_selected() {
    let mut legacy_optional = package_with_optional_sections();
    legacy_optional.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    resign(&mut legacy_optional);
    for (index, candidate) in [
        legacy_package(1),
        package(1),
        legacy_optional,
        package_with_optional_sections(),
    ]
    .into_iter()
    .enumerate()
    {
        let identity = package_generation_identity(&candidate).unwrap();
        retain_schema_examples(index, &candidate);
        let mut wire = serde_json::to_value(&candidate).unwrap();
        if candidate.sections.ims_metadata.is_none() {
            wire["sections"]["ims_metadata"] = serde_json::Value::Null;
        }
        if candidate.sections.ims_tm.is_none() {
            wire["sections"]["ims_tm"] = serde_json::Value::Null;
        }
        let decoded: ApplicationPackageV2 = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, candidate);
        assert_eq!(package_generation_identity(&decoded).unwrap(), identity);
        let payload =
            trusted_state_fixture(vec![(candidate.clone(), InstallState::Ready)], Some(1));
        let reopened = ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(TestVerifier),
            &payload,
        )
        .unwrap();
        assert_eq!(reopened.state_payload().unwrap(), payload);
        assert_eq!(
            reopened.selected_package("DEMO").unwrap().unwrap().as_ref(),
            &candidate
        );
    }
    let absent = package(1);
    let metadata_and_tm = package_with_optional_sections();
    let mut metadata_only = metadata_and_tm.clone();
    metadata_only.sections.ims_tm = None;
    assert_ne!(
        package_generation_identity(&absent).unwrap(),
        package_generation_identity(&metadata_only).unwrap()
    );
    assert_ne!(
        package_generation_identity(&metadata_only).unwrap(),
        package_generation_identity(&metadata_and_tm).unwrap()
    );
}

// The parent owns native schema compilation. When explicitly requested by the
// focused check script, retain actual serde DTOs for its offline validator.
fn retain_schema_examples(index: usize, candidate: &ApplicationPackageV2) {
    let Some(directory) = std::env::var_os("IDENTITY_FRAMING_SCHEMA_FIXTURES") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let original = serde_json::to_value(candidate).unwrap();
    let write = |name: &str, value: &serde_json::Value| {
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    };
    write(&format!("positive-{index}-serialized"), &original);
    if candidate.sections.ims_metadata.is_none() {
        let mut explicit_null = original.clone();
        explicit_null["sections"]["ims_metadata"] = serde_json::Value::Null;
        explicit_null["sections"]["ims_tm"] = serde_json::Value::Null;
        write(&format!("positive-{index}-explicit-null"), &explicit_null);
    }
    if index != 1 {
        return;
    }
    let mut negative = original.clone();
    negative["sections"]["schema_version"] = "mainframe-env.application-package@4".into();
    write("negative-unknown-domain", &negative);
    let mut negative = original.clone();
    negative["schema_version"] = "mainframe-env.application-package@3".into();
    write("negative-second-discriminator", &negative);
    let mut negative = original.clone();
    negative.as_object_mut().unwrap().remove("base");
    write("negative-missing-base", &negative);
    let mut negative = original.clone();
    negative["base"]["manifest"]["entries"][0]["kind"] = "source".into();
    write("negative-slug-instead-of-dto-enum", &negative);
    let mut negative = original.clone();
    negative["base"]["blobs"] = serde_json::json!({
        "sha256:0000000000000000000000000000000000000000000000000000000000000000": [256]
    });
    write("negative-out-of-range-blob-byte", &negative);
    let mut negative = original.clone();
    negative["sections"]["mq_resources"][0]["kind"] = "Alias".into();
    write("negative-unknown-mq-kind", &negative);
    let mut negative = original.clone();
    negative["sections"]["sql_rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("table");
    write("negative-missing-sql-table", &negative);
    let mut negative = original.clone();
    negative["sections"]["ims_rows"][0]["unexpected"] = true.into();
    write("negative-unknown-ims-row-field", &negative);
    let mut negative = original;
    negative["generation"] = 0.into();
    write("negative-zero-generation", &negative);
}

fn above_default_budget_package(domain: &str) -> ApplicationPackageV2 {
    ApplicationPackageV2 {
        base: base_package(),
        generation: 1,
        sections: ApplicationSections {
            schema_version: domain.into(),
            host_abi_libraries: Vec::new(),
            sql_tables: Vec::new(),
            sql_rows: Vec::new(),
            ims_definitions: Vec::new(),
            ims_rows: Vec::new(),
            ims_metadata: None,
            ims_tm: None,
            mq_resources: Vec::new(),
            batch_controllers: vec![BatchController {
                name: "APP-CONTROLLER".into(),
                program: "program/2".into(),
                kind: BatchControllerKind::CobolProgram,
                properties: BTreeMap::from([("budget".into(), "x".repeat(1_048_577))]),
            }],
            security_resources: Vec::new(),
        },
        signature: PackageSignature {
            algorithm: "test-signature@1".into(),
            key_id: "test-key".into(),
            value: "pending".into(),
        },
    }
}

#[test]
fn identity_framing_explicit_budget_preserves_frozen_and_current_large_value_vectors() {
    let limits = PackageLimits {
        max_value_bytes: 1_048_577,
        ..PackageLimits::default()
    };
    // Independent finite preimages, including the literal 1,048,577-byte value,
    // are retained externally. Expected values are never computed by product code.
    for (domain, expected) in [
        (
            APPLICATION_PACKAGE_V2_CONTRACT,
            "sha256:2dbede5fa4b09ce40eaea7cbe2213e6676fb7fb71fff2da9e2708f09eafa880d",
        ),
        (
            APPLICATION_PACKAGE_V3_CONTRACT,
            "sha256:2fee297a1fbb25159fbe63fdbbb5c640511acce952e6e77704177a1634305197",
        ),
    ] {
        let mut candidate = above_default_budget_package(domain);
        validate_package(&candidate.base, "0.2.0").unwrap();
        validate_sections(&candidate, limits).unwrap();
        assert_eq!(
            package_generation_identity(&candidate),
            Err(InstallProblem::LimitExceeded)
        );
        assert_eq!(
            package_generation_identity_with_limits(&candidate, limits).unwrap(),
            expected
        );
        assert_eq!(
            package_generation_identity_with_limits(&candidate, PackageLimits::default()),
            Err(InstallProblem::LimitExceeded)
        );
        if domain == APPLICATION_PACKAGE_V2_CONTRACT {
            assert_eq!(
                package_v2_identity(&candidate),
                Err(InstallProblem::LimitExceeded)
            );
        }
        candidate.signature.value = format!("signed:{expected}");
        assert_eq!(
            validate_v2(&candidate, "0.2.0", limits, &TestVerifier)
                .unwrap()
                .identity,
            expected
        );
        let verifier = Arc::new(RecoveryVerifier(std::sync::atomic::AtomicUsize::new(0)));
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
        let before = installer.state_payload().unwrap();
        assert_eq!(
            installer.stage(&candidate),
            Err(InstallProblem::LimitExceeded)
        );
        assert_eq!(verifier.0.load(Ordering::Relaxed), 0);
        assert_eq!(installer.state_payload().unwrap(), before);
    }
}

#[test]
fn identity_framing_trusted_recovery_uses_explicit_budget_in_both_domains() {
    let limits = PackageLimits {
        max_value_bytes: 1_048_577,
        ..PackageLimits::default()
    };
    for domain in [
        APPLICATION_PACKAGE_V2_CONTRACT,
        APPLICATION_PACKAGE_V3_CONTRACT,
    ] {
        let mut candidate = above_default_budget_package(domain);
        let identity = package_generation_identity_with_limits(&candidate, limits).unwrap();
        candidate.signature.value = format!("signed:{identity}");
        let payload =
            trusted_state_fixture(vec![(candidate.clone(), InstallState::Ready)], Some(1));
        let verifier = Arc::new(RecoveryVerifier(std::sync::atomic::AtomicUsize::new(0)));
        assert!(matches!(
            ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                verifier.clone(),
                &payload,
            ),
            Err(InstallProblem::LimitExceeded)
        ));
        assert_eq!(verifier.0.load(Ordering::Relaxed), 0);
        let installer =
            ApplicationInstallerV2::from_state_payload("0.2.0", limits, verifier.clone(), &payload)
                .unwrap();
        assert_eq!(verifier.0.load(Ordering::Relaxed), 1);
        assert_eq!(
            installer.selected("DEMO").unwrap().unwrap().identity,
            identity
        );
        assert_eq!(installer.state_payload().unwrap(), payload);
        assert_eq!(
            installer.install(&candidate).unwrap().state,
            InstallState::Ready
        );
        assert_eq!(installer.rollback("DEMO", 1).unwrap().identity, identity);
        assert_eq!(installer.state_payload().unwrap(), payload);
    }
}

// Hand-authored identity-only fixture covering all ordinary record boundaries.
// Its digest does not confer installability: blob/reference closure still requires
// the independent admission validators exercised by the valid graph controls.
fn golden_fixture() -> ApplicationPackageV2 {
    let sha = format!("sha256:{}", "0".repeat(64));
    ApplicationPackageV2 {
        base: ApplicationPackage {
            manifest: ApplicationManifest {
                name: "A".into(),
                version: "1".into(),
                target_product: "P".into(),
                entries: vec![PackageEntry {
                    path: "source".into(),
                    kind: EntryKind::Source,
                    sha256: sha.clone(),
                    bytes: 7,
                    depends_on: vec!["dep".into()],
                }],
            },
            blobs: BTreeMap::new(),
        },
        generation: 1,
        sections: ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V3_CONTRACT.into(),
            host_abi_libraries: vec![AbiLibrary {
                id: "ABI".into(),
                subsystem: HostSubsystem::Cics,
                version: "1".into(),
                members: vec![AbiMember {
                    name: "M".into(),
                    blob_sha256: sha,
                }],
            }],
            sql_tables: vec![SqlTable {
                name: "T".into(),
                columns: vec![SqlColumn {
                    name: "C".into(),
                    nullable: true,
                }],
                primary_key: vec!["C".into()],
            }],
            sql_rows: vec![SqlSeedRow {
                table: "T".into(),
                values: BTreeMap::from([("C".into(), "V".into())]),
            }],
            ims_definitions: vec![ImsDefinition {
                name: "A".into(),
                segments: BTreeSet::from(["B".into()]),
            }],
            ims_rows: vec![ImsSeedRow {
                definition: "A".into(),
                segment: "B".into(),
                values: BTreeMap::from([("K".into(), "V".into())]),
            }],
            ims_metadata: None,
            ims_tm: None,
            mq_resources: vec![MqResource {
                name: "Q".into(),
                kind: MqResourceKind::Queue,
                target: Some("Q".into()),
                controller: None,
            }],
            batch_controllers: vec![BatchController {
                name: "CTRL".into(),
                program: "source".into(),
                kind: BatchControllerKind::CobolProgram,
                properties: BTreeMap::from([("p".into(), "v".into())]),
            }],
            security_resources: vec![SecurityResource {
                class: "F".into(),
                profile: "R".into(),
                owner: "O".into(),
            }],
        },
        signature: PackageSignature {
            algorithm: "test-signature@1".into(),
            key_id: "test-key".into(),
            value: "unused".into(),
        },
    }
}

// Literal DTO JSON freezes declaration order, enum form, explicit nulls and
// omitted optional fields. Expected hashes use independently authored token
// vectors; no test recreates the production hashing algorithm at runtime.
const METADATA_JSON: &str = concat!(
    "{\"schema_version\":\"mainframe-env.ims-metadata@1\",\"databases\":[{\"name\":\"DB\",",
    "\"version\":1,\"organization\":\"HDAM\",\"segments\":[{\"name\":\"ROOT\",\"parent\":null,",
    "\"min_length\":1,\"max_length\":1,\"fields\":[{\"name\":\"KEY\",\"offset\":0,\"length\":1,",
    "\"sequence\":true,\"unique\":true}]}],\"secondary_indexes\":[],\"logical_relationships\":[]}],",
    "\"psbs\":[{\"name\":\"PSB\",\"database_level\":\"current\",\"pcbs\":[{\"kind\":\"database\",",
    "\"name\":\"PCB\",\"database\":\"DB\",\"database_version\":null,\"processing_options\":\"G\",",
    "\"sensitive_segments\":[{\"name\":\"ROOT\",\"parent\":null,\"processing_options\":null}]}]}]}"
);
const TM_JSON: &str = concat!(
    "{\"transactions\":[{\"code\":\"TX\",\"psb\":\"PSB\",\"program_selector\":\"source\",",
    "\"artifact\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\",",
    "\"required_generation\":\"g\",\"context\":\"message-processing\",\"priority\":0,",
    "\"timeout_ticks\":1,\"conversational\":false,\"spa_size\":0,\"alternate_pcbs\":[{",
    "\"name\":\"OUT\",\"destination\":{\"fixed\":\"TERM\"},\"express\":false}]}]}"
);

#[test]
fn identity_framing_independent_ordinary_and_optional_golden_vectors_are_frozen() {
    // The independent finite-vector source has SHA-256
    // a820e45448f57033e0a90d80e1aa963a0ae6e3dc362308678d636a737fcef135.
    // Its byte preimages and inputs are retained in the external task handoff.
    let mut candidate = golden_fixture();
    assert_eq!(
        package_generation_identity(&candidate).unwrap(),
        "sha256:fea3f601db9c3923a41d6d49f7d167c78065a33228df63d751896368a82317fd"
    );
    let metadata: ImsMetadataCatalog = serde_json::from_str(METADATA_JSON).unwrap();
    assert_eq!(serde_json::to_string(&metadata).unwrap(), METADATA_JSON);
    candidate.sections.ims_metadata = Some(metadata);
    assert_eq!(
        package_generation_identity(&candidate).unwrap(),
        "sha256:363ba7117117b9acd8bea412c8cbf5fa080c5577c2114e968698751e7f8884f5"
    );
    let definitions: TmDefinitionSet = serde_json::from_str(TM_JSON).unwrap();
    assert_eq!(serde_json::to_string(&definitions).unwrap(), TM_JSON);
    candidate.sections.ims_tm = Some(definitions);
    assert_eq!(
        package_generation_identity(&candidate).unwrap(),
        "sha256:4b4c78d97d2af909de2c8f05808460241b0cff83a40b5f955f131e373b4c5d22"
    );
}

#[test]
fn identity_framing_standalone_v1_manifest_and_reader_remain_unchanged() {
    let base = base_package();
    assert_eq!(
        package_identity(&base.manifest).unwrap(),
        "sha256:9deda47cc342c1ceac21f85f0e9a7723b901cb31e7fc3eb9096e3fd479bebb39"
    );
    let installer = crate::ApplicationInstaller::new("0.2.0");
    assert_eq!(installer.install(&base).unwrap().state, InstallState::Ready);
}

#[test]
fn identity_framing_optional_string_presence_is_distinct_without_admission_credit() {
    let mut candidate = package(1);
    candidate.sections.mq_resources[0].target = None;
    let absent = package_generation_identity(&candidate).unwrap();
    candidate.sections.mq_resources[0].target = Some(String::new());
    assert_ne!(package_generation_identity(&candidate).unwrap(), absent);
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), Arc::new(TestVerifier));
    let before = installer.state_payload().unwrap();
    assert_eq!(
        installer.stage(&candidate),
        Err(InstallProblem::MissingReference)
    );
    assert_eq!(installer.state_payload().unwrap(), before);
}

#[test]
fn identity_framing_bounded_hashing_does_not_replace_reference_admission() {
    for domain in [
        APPLICATION_PACKAGE_V2_CONTRACT,
        APPLICATION_PACKAGE_V3_CONTRACT,
    ] {
        let mut candidate = package(1);
        candidate.sections.schema_version = domain.into();
        let mut metadata: serde_json::Value = serde_json::from_str(METADATA_JSON).unwrap();
        metadata["psbs"][0]["pcbs"][0]["database"] = "ABSENT".into();
        candidate.sections.ims_metadata = Some(serde_json::from_value(metadata).unwrap());
        // A publisher can sign a bounded hostile graph, while reference checks
        // still refuse it before invoking the verifier or changing installer state.
        resign(&mut candidate);
        let verifier = Arc::new(RecoveryVerifier(std::sync::atomic::AtomicUsize::new(0)));
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
        let before = installer.state_payload().unwrap();
        assert_eq!(
            installer.stage(&candidate),
            Err(InstallProblem::MissingReference)
        );
        assert_eq!(verifier.0.load(Ordering::Relaxed), 0);
        assert_eq!(installer.state_payload().unwrap(), before);
    }
}
