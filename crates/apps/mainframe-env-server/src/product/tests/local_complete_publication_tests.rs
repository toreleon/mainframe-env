//! Real owned Complete/Applied publication and ordinary compiled controller/JES completion.
use super::*;
use mainframe_env_application::{
    APPLICATION_PUBLICATION_NAMESPACE, ApplicationPublicationState, PublicationSectionState,
};
use mainframe_env_execution_api::AuditDecision;

fn complete_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = definitions(server, trust, generation);
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
    let controllers = package
        .sections
        .batch_controllers
        .iter()
        .map(|item| {
            super::super::super::batch_controller::decode_application_batch_controller(
                &package, item,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    for (path, bytes) in [
        (
            "resource/batch-controllers",
            serde_json::to_vec(&controllers).unwrap(),
        ),
        (
            "resource/local-complete-publication",
            b"mainframe-env.application-local-complete-initial@1".to_vec(),
        ),
    ] {
        let sha = format!("sha256:{:x}", Sha256::digest(&bytes));
        package.base.manifest.entries.push(PackageEntry {
            path: path.into(),
            kind: EntryKind::Resource,
            sha256: sha.clone(),
            bytes: bytes.len(),
            depends_on: vec!["program/LOCALPGM".into()],
        });
        package.base.blobs.insert(sha, bytes);
    }
    resign_package(&mut package, trust);
    (package, definition)
}

fn publication(store: &dyn ProviderStateStore) -> ProviderStateRecord {
    store
        .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
        .unwrap()
        .unwrap()
}

fn all_selected_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut result = selected_rows(store);
    result.extend(
        store
            .list_provider_state(APPLICATION_PUBLICATION_NAMESPACE, 1024)
            .unwrap(),
    );
    result.extend(
        store
            .list_provider_state("batch-controller-state", 2)
            .unwrap(),
    );
    result.extend(store.list_provider_state("batch-program", 4096).unwrap());
    result.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    result
}

fn exercise_complete<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = complete_definitions(&server, &trust, 1);
    let staged = server.install_application_package_v2(&package).unwrap();
    let retained = {
        let owner = server.applications_v2.lock().unwrap();
        owner
            .installer
            .generation(&staged.package, 1, &staged.identity)
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        server.require_complete_application_publication(&retained),
        Err(HostProblem::NotFound)
    );
    let installed = server
        .publish_complete_local_application(&staged, definition)
        .unwrap();
    let record = publication(store.as_ref());
    let state = ApplicationPublicationState::from_payload(&record.payload).unwrap();
    assert!(state.matches_complete(
        &installed.generation.package,
        1,
        &installed.generation.identity
    ));
    assert_eq!(state.controllers, PublicationSectionState::Applied);
    assert_eq!(state.db2, PublicationSectionState::NotApplicable);
    assert_eq!(state.ims, PublicationSectionState::NotApplicable);
    assert_eq!(record.version, 1);
    assert_eq!(
        server.require_complete_application_publication(&retained),
        Ok(())
    );
    assert_eq!(
        server
            .selected_complete_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation.clone())
    );
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
    server.bootstrap_user("IBMUSER", b"LOCALPASS").unwrap();
    let job = super::super::submit_jcl_direct(&server, "IBMUSER", b"LOCALPASS", "//LOCAL75 JOB CLASS=A\n//STEP1 EXEC PGM=IKJEFT01\n//SYSTSIN DD *\n RUN PROGRAM(LOCALPGM)\n/*\n".into());
    let id = job["jobid"].as_str().unwrap();
    assert_eq!(
        server.run_jes_worker_once("runtime75-owner").unwrap(),
        Some(format!("jes:{id}"))
    );
    let actual = server.batch.get(id).unwrap();
    assert_eq!(actual.state, mainframe_env_batch::JobState::Completed);
    assert_eq!(actual.return_code, Some(0));
    assert_eq!(actual.steps.len(), 1);
    assert_eq!(
        actual.steps[0].state,
        mainframe_env_batch::StepState::Completed
    );
    let checkpoint = server.batch.checkpoint(id).unwrap().unwrap();
    assert_eq!(checkpoint.job_id, id);
    assert!(checkpoint.job_version > 0);
    assert_eq!(checkpoint.committed_steps, vec!["STEP1".to_string()]);
    let work = store.get_work(&format!("jes:{id}")).unwrap().unwrap();
    assert_eq!(work.state, WorkState::Completed);
    assert!(
        store
            .audit_records(&work.execution_id, 1, 128)
            .unwrap()
            .iter()
            .any(|audit| audit.capability.as_str() == "host.program.invoke"
                && audit.decision == AuditDecision::Success)
    );
    // Ordinary outer Batch dispatch persists a Success audit, not a fabricated core EffectRecord.
    assert_eq!(publication(store.as_ref()), record);
    let (_, retry) = complete_definitions(&server, &trust, 1);
    let before = all_selected_rows(store.as_ref());
    assert!(
        server
            .publish_complete_local_application(&installed.generation, retry)
            .unwrap()
            .replayed
    );
    assert_eq!(all_selected_rows(store.as_ref()), before);
    assert_eq!(publication(store.as_ref()), record);
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(publication(store.as_ref()), record);
    assert_eq!(
        reopened
            .selected_complete_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
    assert_eq!(reopened.batch.get(id).unwrap(), actual);
    assert_eq!(
        store.get_work(&format!("jes:{id}")).unwrap().unwrap().state,
        WorkState::Completed
    );
}

#[test]
fn complete_owned_publication_and_real_compiled_jes_controller_finish_on_memory_and_sqlite() {
    exercise_complete(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "local-complete-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    exercise_complete(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn complete_mode_requires_signed_marker_and_preserves_original_storage_port_refusal() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = definitions(&server, &trust, 1);
    let staged = server.install_application_package_v2(&package).unwrap();
    let before = all_selected_rows(store.as_ref());
    assert_eq!(
        server.publish_complete_local_application(&staged, definition),
        Err(HostProblem::NotFound)
    );
    assert_eq!(all_selected_rows(store.as_ref()), before);
    let (_, definition) = definitions(&server, &trust, 1);
    let installed = server
        .publish_local_application(&staged, definition)
        .unwrap();
    assert_eq!(
        server.selected_complete_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::NotFound)
    );
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
    assert_eq!(
        store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
            .unwrap(),
        None
    );
}

#[test]
fn complete_initial_port_refuses_later_generation_and_foreign_publication_without_adoption() {
    for generation in [1, 2] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = complete_definitions(&server, &trust, generation);
        let staged = server.install_application_package_v2(&package).unwrap();
        if generation == 1 {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
                        key: "LOCAL-COMPOSE".into(),
                        version: 1,
                        payload: b"foreign".to_vec(),
                    },
                    None,
                )
                .unwrap();
        }
        let before = all_selected_rows(store.as_ref());
        assert_eq!(
            server.publish_complete_local_application(&staged, definition),
            Err(if generation == 1 {
                HostProblem::IdempotencyConflict
            } else {
                HostProblem::Unsupported
            })
        );
        assert_eq!(all_selected_rows(store.as_ref()), before);
    }
}

#[test]
fn complete_commit_unknown_fences_actual_selection_and_reopens_only_genuine_full_cas() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = complete_definitions(&server, &trust, 1);
        let staged = server.install_application_package_v2(&package).unwrap();
        store.arm(outcome);
        assert_eq!(
            server.publish_complete_local_application(&staged, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.selected_complete_local_application("LOCAL-COMPOSE"),
            Err(HostProblem::UnknownOutcome)
        );
        let landed = store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
            .unwrap();
        assert_eq!(landed.is_some(), matches!(outcome, Outcome::After));
        drop(server);
        let reopened = fixture(store.clone(), trust);
        assert_eq!(
            reopened
                .selected_complete_local_application("LOCAL-COMPOSE")
                .unwrap()
                .is_some(),
            landed.is_some()
        );
    }
}

#[test]
fn complete_publication_version_drift_refuses_exact_selected_readback() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = complete_definitions(&server, &trust, 1);
    let staged = server.install_application_package_v2(&package).unwrap();
    server
        .publish_complete_local_application(&staged, definition)
        .unwrap();
    let mut row = publication(store.as_ref());
    row.version += 1;
    store.put_provider_state(row, Some(1)).unwrap();
    assert_eq!(
        server.selected_complete_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
}
