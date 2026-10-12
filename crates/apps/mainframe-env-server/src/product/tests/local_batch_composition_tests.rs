//! Genuine selected controller CAS, compiled program mapping and original execution-authority refusal.
use super::*;
use mainframe_env_batch::{
    BATCH_CONTROLLER_REGISTRY_CONTRACT, BatchControllerGeneration, BatchControllerProjection,
    BatchControllerSelector,
};

fn controller_definitions(
    package: &ApplicationPackageV2,
) -> Vec<mainframe_env_batch::BatchControllerDefinition> {
    let mut result = package
        .sections
        .batch_controllers
        .iter()
        .map(|item| {
            super::super::super::batch_controller::decode_application_batch_controller(
                package, item,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}
fn signed_batch(package: &mut ApplicationPackageV2, trust: &HmacSha256PackageTrust) {
    let mut controller = signed_controller_package(trust)
        .sections
        .batch_controllers
        .remove(0);
    controller.name = "LOCAL-CONTROLLER".into();
    controller.program = "program/LOCALPGM".into();
    controller
        .properties
        .insert("selector-program".into(), "LOCALPGM".into());
    package.sections.batch_controllers = vec![controller];
    let raw = serde_json::to_vec(&controller_definitions(package)).unwrap();
    let sha = format!("sha256:{:x}", Sha256::digest(&raw));
    package.base.blobs.insert(sha.clone(), raw.clone());
    package.base.manifest.entries.push(PackageEntry {
        path: "resource/batch-controllers".into(),
        kind: EntryKind::Resource,
        sha256: sha,
        bytes: raw.len(),
        depends_on: vec!["program/LOCALPGM".into()],
    });
    resign_package(package, trust);
}
fn batch_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = definitions(server, trust, generation);
    signed_batch(&mut package, trust);
    (package, definition)
}
fn install_batch(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = batch_definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut result = selected_rows(store);
    for namespace in [
        "batch-controller-state",
        "batch-program",
        "jes-job",
        "jes-meta",
        "jes-scheduler",
        "jes-topology",
    ] {
        result.extend(store.list_provider_state(namespace, 4096).unwrap());
    }
    result.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    result
}
fn projection(store: &dyn ProviderStateStore) -> BatchControllerProjection {
    let row = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    serde_json::from_value(value["batch"].clone()).unwrap()
}
fn exercise_ready<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let installed = install_batch(&server, &trust, 1);
    let actual = projection(store.as_ref());
    assert_eq!(actual.generation.controllers.len(), 1);
    assert_eq!(actual.generation.identity, installed.generation.identity);
    assert_eq!(
        server
            .batch
            .selected_controller_composition_matches(&actual),
        Ok(())
    );
    let program = store
        .get_provider_state("batch-program", "LOCALPGM")
        .unwrap()
        .unwrap();
    assert_eq!(
        program.payload,
        actual.generation.controllers[0].program.identity.as_bytes()
    );
    let record = store
        .get_provider_state("batch-controller-state", "registry")
        .unwrap()
        .unwrap();
    assert!(record.version > 0);
    let retained = {
        let owner = server.applications_v2.lock().unwrap();
        owner
            .installer
            .generation("LOCAL-COMPOSE", 1, &installed.generation.identity)
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        server.require_complete_application_publication(&retained),
        Err(HostProblem::NotFound)
    );
    let before = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
    assert_eq!(
        reopened
            .batch
            .selected_controller_composition_matches(&actual),
        Ok(())
    );
}
#[test]
fn batch_full_signed_controller_registry_and_compiled_mapping_reopen_on_both_stores() {
    exercise_ready(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "batch-ready-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    exercise_ready(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn batch_retry_upgrade_and_retained_rollback_preserve_full_other_owners_and_original_seed_selection()
 {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install_batch(&server, &trust, 1);
    let mut foreign = projection(store.as_ref()).generation;
    foreign.application = "UNRELATED".into();
    foreign.controllers[0].name = "OTHER-CONTROLLER".into();
    foreign.controllers[0].selector = BatchControllerSelector::TsoRun {
        program: "OTHERPGM".into(),
    };
    foreign.controllers[0].program.path = "program/OTHERPGM".into();
    server.batch.install_controllers(foreign.clone()).unwrap();
    server
        .dataset
        .invoke(DatasetRequest::Write {
            dataset: DatasetName::new("LOCAL.SEED", 128).unwrap(),
            member: None,
            records: vec![RAW_TWO[..4].to_vec()],
            expected_version: Some(1),
            mutation: Mutation {
                sequence: 1,
                transaction: None,
                idempotency_key: IdempotencyKey::new(
                    "batch-runtime-seed-edit",
                    InvocationLimits::default(),
                )
                .unwrap(),
            },
        })
        .unwrap();
    let (_, definition) = batch_definitions(&server, &trust, 1);
    let before = rows(store.as_ref());
    assert!(
        server
            .publish_local_application(&first.generation, definition)
            .unwrap()
            .replayed
    );
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(seed_records(&server), vec![RAW_TWO[..4].to_vec()]);
    install_batch(&server, &trust, 2);
    let (_, definition) = batch_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(projection(store.as_ref()).generation.generation, 1);
    assert_eq!(
        seed_records(&server),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        server
            .batch
            .selected_controller_composition_matches(&BatchControllerProjection {
                generation: foreign
            }),
        Ok(())
    );
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(first.generation)
    );
}
#[test]
fn batch_foreign_selected_controller_and_legacy_owner_refuse_without_adoption() {
    for owner in ["FOREIGN", "LOCAL-COMPOSE"] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = batch_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        server
            .batch
            .install_controllers(BatchControllerGeneration {
                schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
                application: owner.into(),
                generation: 1,
                identity: expected.identity.clone(),
                controllers: controller_definitions(&package),
            })
            .unwrap();
        let before = rows(store.as_ref());
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn batch_missing_signed_resource_changed_section_and_unsupported_plan_refuse_before_mutation() {
    for mode in 0..3 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = batch_definitions(&server, &trust, 1);
        match mode {
            0 => package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/batch-controllers"),
            1 => package.sections.batch_controllers[0].name = "CHANGED-NAME".into(),
            _ => package.sections.batch_controllers[0]
                .properties
                .insert("behavior".into(), "ims-load".into())
                .map(|_| ())
                .unwrap(),
        }
        let used = package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect::<BTreeSet<_>>();
        package.base.blobs.retain(|sha, _| used.contains(sha));
        resign_package(&mut package, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn batch_unknown_shared_commit_and_wrong_store_acknowledgement_fence_actual_controller_owner() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = batch_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let offered = BatchControllerGeneration {
            schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: expected.package.clone(),
            generation: 1,
            identity: expected.identity.clone(),
            controllers: controller_definitions(&package),
        };
        let before = rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.batch.validate_controller_install(&offered),
            Err(HostProblem::UnknownOutcome)
        );
        drop(server);
        let reopened = fixture(store.clone(), trust);
        if matches!(outcome, Outcome::Before) {
            assert_eq!(rows(store.as_ref()), before);
            assert_eq!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap(),
                None
            );
        } else {
            assert!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap()
                    .is_some()
            );
        }
    }
    let trust = Arc::new(test_package_trust());
    let left = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(left.clone(), trust.clone());
    let (package, _) = batch_definitions(&server, &trust, 1);
    let generation = BatchControllerGeneration {
        schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
        application: "LOCAL-COMPOSE".into(),
        generation: 1,
        identity: mainframe_env_application::package_generation_identity(&package).unwrap(),
        controllers: controller_definitions(&package),
    };
    let prepared = server
        .batch
        .prepare_controller_composition(generation.clone(), None, false)
        .unwrap();
    let before = rows(left.as_ref());
    let commit = ProviderCompositionCommit::publish(
        Arc::new(MemoryStore::new(Default::default())),
        prepared.writes().to_vec(),
    )
    .unwrap();
    assert_eq!(prepared.finish(&commit), Err(HostProblem::UnknownOutcome));
    assert_eq!(rows(left.as_ref()), before);
    assert_eq!(
        server.batch.validate_controller_install(&generation),
        Err(HostProblem::UnknownOutcome)
    );
}
#[test]
fn batch_sqlite_shared_companion_conflict_keeps_full_registry_mapping_and_selection() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "batch-cas-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), trust.clone());
    install_batch(&server, &trust, 1);
    let prior = projection(store.as_ref());
    let (package, definition) = batch_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    let generation = BatchControllerGeneration {
        schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
        application: expected.package.clone(),
        generation: 2,
        identity: expected.identity.clone(),
        controllers: controller_definitions(&package),
    };
    let companion = ProviderStateRecord {
        namespace: "batch-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"UNCHANGED".to_vec(),
    };
    store.put_provider_state(companion.clone(), None).unwrap();
    let before = rows(store.as_ref());
    let prepared = server
        .batch
        .prepare_controller_composition(generation, Some(&prior), false)
        .unwrap();
    let mut writes = prepared.writes().to_vec();
    writes.push(ProviderStateWrite {
        record: companion.clone(),
        expected_version: None,
    });
    assert!(ProviderCompositionCommit::publish(store.clone(), writes).is_err());
    drop(prepared);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state(&companion.namespace, &companion.key)
            .unwrap(),
        Some(companion)
    );
    server
        .publish_local_application(&expected, definition)
        .unwrap();
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        2
    );
    drop(reopened);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn batch_actual_capacity_stale_full_projection_registry_version_and_program_bytes_refuse() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    install_batch(&server, &trust, 1);
    let prior = projection(store.as_ref());
    let before = rows(store.as_ref());
    let mut too_many = prior.generation.clone();
    too_many.generation = 2;
    too_many.controllers = vec![too_many.controllers[0].clone(); 4097];
    assert!(matches!(
        server
            .batch
            .prepare_controller_composition(too_many, Some(&prior), false),
        Err(HostProblem::ResourceExhausted)
    ));
    let mut wrong = prior.clone();
    wrong.generation.controllers[0].name = "WRONG-NAME".into();
    assert_eq!(
        server.batch.selected_controller_composition_matches(&wrong),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    let raw = store
        .get_provider_state("batch-controller-state", "registry")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: raw.version + 1,
                ..raw.clone()
            },
            Some(raw.version),
        )
        .unwrap();
    let before = rows(store.as_ref());
    assert_eq!(
        server.batch.selected_controller_composition_matches(&prior),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    drop(server);
    let server = fixture(store.clone(), trust);
    let program = store
        .get_provider_state("batch-program", "LOCALPGM")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: program.version + 1,
                payload: b"CORRUPT".to_vec(),
                ..program.clone()
            },
            Some(program.version),
        )
        .unwrap();
    let before = rows(store.as_ref());
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    // A foreign key in the one-row controller namespace is refused by the new
    // finite participant without adopting, deleting or changing any provider row.
    let trust = Arc::new(test_package_trust());
    let owned = Arc::new(MemoryStore::new(Default::default()));
    let actual = fixture(owned.clone(), trust.clone());
    install_batch(&actual, &trust, 1);
    let selected = projection(owned.as_ref());
    let foreign = ProviderStateRecord {
        namespace: "batch-controller-state".into(),
        key: "unexpected-owner".into(),
        version: 1,
        payload: b"FOREIGN-UNCHANGED".to_vec(),
    };
    owned.put_provider_state(foreign.clone(), None).unwrap();
    let full = rows(owned.as_ref());
    assert_eq!(
        actual
            .batch
            .selected_controller_composition_matches(&selected),
        Err(HostProblem::IdempotencyConflict)
    );
    assert!(matches!(
        actual.batch.prepare_controller_composition(
            selected.generation.clone(),
            Some(&selected),
            false
        ),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(rows(owned.as_ref()), full);
    assert_eq!(
        owned
            .get_provider_state(&foreign.namespace, &foreign.key)
            .unwrap(),
        Some(foreign)
    );
}
