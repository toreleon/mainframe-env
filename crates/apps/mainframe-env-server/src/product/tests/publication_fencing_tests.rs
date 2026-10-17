//! Draft behavioral expectations for CV-204.publication-fencing.
//! Exercises authenticated JES admission against signed retained application generations.
use super::*;
use mainframe_env_application::{SqlColumn, SqlTable};
use mainframe_env_db2::{Db2ColumnDefinition, Db2ResultEncoding};
use mainframe_env_store_api::{ArtifactStore, ProviderStateStore};
use std::sync::atomic::{AtomicU64, Ordering};
#[path = "publication_fault_store.rs"]
mod fault;
use fault::{FaultStore, Outcome, Point};

static CASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const APP: &str = "PUBLICATION-FENCE";
const TABLE: &str = "FENCE.T";
fn table_name(generation: u64) -> &'static str {
    if generation == 1 { TABLE } else { "FENCE.NEXT" }
}
const ONE: &str = "FENCEONE";
const TWO: &str = "FENCETWO";
const MARKER_ONE: &[u8] = b"PUBLICATION G1";
const MARKER_TWO: &[u8] = b"PUBLICATION G2";

struct TestDirectory(std::path::PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "failed to remove test directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn test_config() -> (ServerConfig, TestDirectory) {
    let mut result = config();
    let directory = TestDirectory(std::env::temp_dir().join(format!(
        "publication-fencing-{}-{}",
        std::process::id(),
        CASE_SEQUENCE.fetch_add(1, Ordering::SeqCst)
    )));
    std::fs::create_dir_all(&directory.0).unwrap();
    result.artifact_root = directory.0.clone();
    (result, directory)
}

fn replace_blob(package: &mut ApplicationPackageV2, path: &str, bytes: Vec<u8>) {
    let entry = package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|entry| entry.path == path)
        .unwrap();
    package.base.blobs.remove(&entry.sha256);
    entry.sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    entry.bytes = bytes.len();
    package.base.blobs.insert(entry.sha256.clone(), bytes);
}

// Six required manifest kinds, one controller, one SQL table, no optional IMS/TM sections.
// Compiled payload bytes come from the existing publisher; display expectations do not.
fn tiny_package(
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, BatchProgramDefinition) {
    assert!(matches!(generation, 1 | 2));
    let (name, marker) = if generation == 1 {
        (ONE, "PUBLICATION G1")
    } else {
        (TWO, "PUBLICATION G2")
    };
    let source = format!(
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. {name}.\nPROCEDURE DIVISION.\nDISPLAY '{marker}'.\nSTOP RUN.\n"
    );
    let published = published_source_fixture(name, &source);
    let program = BatchProgramDefinition::current(name, &published);
    let mut package = signed_controller_package(trust);
    package.base.manifest.name = APP.into();
    package.generation = generation;
    replace_blob(&mut package, "source/manifest", source.into_bytes());
    replace_blob(&mut package, "program/REALPGM", program.payload.clone());
    package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|entry| entry.kind == EntryKind::Program)
        .unwrap()
        .path = format!("program/{name}");
    package.sections.batch_controllers[0].program = format!("program/{name}");
    package.sections.batch_controllers[0]
        .properties
        .insert("selector-program".into(), name.into());
    package.sections.sql_tables = vec![SqlTable {
        name: table_name(generation).into(),
        columns: vec![SqlColumn {
            name: "ID".into(),
            nullable: false,
        }],
        primary_key: vec!["ID".into()],
    }];
    let definitions = vec![Db2TableDefinition {
        name: table_name(generation).into(),
        columns: vec![Db2ColumnDefinition {
            name: "ID".into(),
            nullable: false,
            max_bytes: usize::try_from(generation).unwrap(),
            result_encoding: Db2ResultEncoding::Raw,
            default_value: None,
        }],
        primary_key: vec!["ID".into()],
        foreign_keys: vec![],
        extract: None,
    }];
    replace_blob(
        &mut package,
        "data/manifest",
        serde_json::to_vec(&definitions).unwrap(),
    );
    package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|entry| entry.kind == EntryKind::Data)
        .unwrap()
        .path = "data/db2/catalog".into();
    resign_package(&mut package, trust);
    (package, program)
}

struct Harness {
    server: Arc<ProductServer>,
    store: Arc<FaultStore<MemoryStore>>,
    trust: Arc<HmacSha256PackageTrust>,
    config: ServerConfig,
    first: ApplicationGenerationRecord,
    _directory: TestDirectory,
}

impl Harness {
    fn first() -> Self {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let (config, directory) = test_config();
        let server = ProductServer::open_with_package_trust(
            config.clone(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let (package, program) = tiny_package(&trust, 1);
        server.install_batch_programs(vec![program]).unwrap();
        let first = server.install_application_package_v2(&package).unwrap();
        server.publish_application_generation(&first).unwrap();
        Self {
            server,
            store,
            trust,
            config,
            first,
            _directory: directory,
        }
    }
    fn stage_second(&self) -> ApplicationGenerationRecord {
        let (package, program) = tiny_package(&self.trust, 2);
        self.server.install_batch_programs(vec![program]).unwrap();
        self.server
            .install_application_package_v2(&package)
            .unwrap()
    }
    async fn reopen(self) -> Self {
        let Self {
            server,
            store,
            trust,
            config,
            first,
            _directory: directory,
        } = self;
        assert!(server.graceful_shutdown().await);
        drop(server);
        let server = ProductServer::open_with_package_trust(
            config.clone(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        Self {
            server,
            store,
            trust,
            config,
            first,
            _directory: directory,
        }
    }
}

// Take only the application/provider families concerned by this slice, after staging.
fn publication_rows(store: &impl ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = Vec::new();
    for prefix in [
        "application-publication-v2",
        "application-package-v2",
        "batch-controller-state",
        "db2-v1-",
        "db2-state",
        "ims-v1-metadata",
        "ims-tm-",
    ] {
        rows.extend(store.list_provider_state_prefix(prefix, 256).unwrap());
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}

// Public JobSubmit, JES worker, controller dispatch and public spool retrieval.
// No assertion calls resolve_controller or verify_controller_program directly.
async fn route(
    server: &Arc<ProductServer>,
    program: &str,
) -> (mainframe_env_batch::JobSnapshot, Vec<Vec<u8>>) {
    let response = call(&server.router(), Method::PUT, "/zosmf/restjobs/jobs", format!("//FENCEJOB JOB CLASS=A\n//STEP1 EXEC PGM=IKJEFT01\n//SYSTSIN DD *\nRUN PROGRAM({program})\n/*\n//SYSPRINT DD SYSOUT=*\n")).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let job: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let id = job["jobid"].as_str().unwrap();
    let completed = wait_for_terminal_job(server, id).await;
    let response = call(
        &server.router(),
        Method::GET,
        &format!("/zosmf/restjobs/jobs/FENCEJOB/{id}/files"),
        "",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let files: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let mut outputs = Vec::new();
    for file in files.as_array().unwrap() {
        let response = call(
            &server.router(),
            Method::GET,
            &format!(
                "/zosmf/restjobs/jobs/FENCEJOB/{id}/files/{}/records",
                file["id"].as_u64().unwrap()
            ),
            "",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        outputs.push(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        );
    }
    let receipt = server
        .config
        .artifact_root
        .join(format!("jes-{id}-{program}.json"));
    let value = serde_json::json!({
        "program": program, "job": format!("{completed:?}"),
        "return_code": completed.return_code, "outputs": &outputs,
    });
    std::fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    eprintln!("JES receipt: {}", receipt.display());
    (completed, outputs)
}

async fn runs(server: &Arc<ProductServer>, program: &str, expected: &[u8]) {
    let (job, outputs) = route(server, program).await;
    assert_eq!(job.return_code, Some(0), "{job:?}");
    assert!(
        outputs.iter().any(|output| output == expected),
        "{outputs:?}"
    );
    let other = if expected == MARKER_ONE {
        MARKER_TWO
    } else {
        MARKER_ONE
    };
    assert!(
        outputs
            .iter()
            .all(|output| !output.windows(other.len()).any(|bytes| bytes == other))
    );
}
async fn refuses(server: &Arc<ProductServer>, program: &str) {
    let (job, outputs) = route(server, program).await;
    assert_refused(&job, &outputs);
}
fn assert_refused(job: &mainframe_env_batch::JobSnapshot, outputs: &[Vec<u8>]) {
    assert_ne!(
        job.return_code,
        Some(0),
        "incomplete or unbound controller executed: {job:?}"
    );
    for marker in [MARKER_ONE, MARKER_TWO] {
        assert!(
            outputs
                .iter()
                .all(|output| !output.windows(marker.len()).any(|bytes| bytes == marker)),
            "{outputs:?}"
        );
    }
}

// Collect only product refusal assertions so every independent window executes.
// Setup, authentication, hook-hit and reopen controls remain immediate assertions.
async fn observe_refusal(
    server: &Arc<ProductServer>,
    program: &str,
    case: &str,
    failures: &mut Vec<String>,
) {
    let (job, outputs) = route(server, program).await;
    if std::panic::catch_unwind(|| assert_refused(&job, &outputs)).is_err() {
        failures.push(format!(
            "{case}: {program} executed or exposed a forbidden marker"
        ));
    }
}
fn save_rows(server: &ProductServer, label: &str, rows: &[ProviderStateRecord]) {
    std::fs::write(
        server
            .config
            .artifact_root
            .join(format!("state-{label}.txt")),
        format!("{rows:#?}"),
    )
    .unwrap();
}

#[tokio::test]
async fn publication_fencing_complete_install_rollback_retry_reopen_control() {
    let h = Harness::first();
    runs(&h.server, ONE, MARKER_ONE).await;
    refuses(&h.server, TWO).await;
    let second = h.stage_second();
    h.server.publish_application_generation(&second).unwrap();
    assert!(
        h.server
            .publish_application_generation(&second)
            .unwrap()
            .replayed
    );
    runs(&h.server, TWO, MARKER_TWO).await;
    refuses(&h.server, ONE).await;
    h.server.rollback_application_generation(&h.first).unwrap();
    assert!(
        h.server
            .rollback_application_generation(&h.first)
            .unwrap()
            .replayed
    );
    let h = h.reopen().await;
    runs(&h.server, ONE, MARKER_ONE).await;
    refuses(&h.server, TWO).await;
    assert_eq!(
        h.server
            .db2_service()
            .table_definition(TABLE)
            .unwrap()
            .columns[0]
            .max_bytes,
        1
    );
}

#[tokio::test]
async fn publication_fencing_missing_sql_catalog_refuses_before_mutation() {
    prevalidation_case("missing").await;
}
#[tokio::test]
async fn publication_fencing_invalid_sql_catalog_refuses_before_mutation() {
    prevalidation_case("invalid").await;
}
#[tokio::test]
async fn publication_fencing_wrong_sql_catalog_refuses_before_mutation() {
    prevalidation_case("wrong").await;
}
async fn prevalidation_case(case: &str) {
    let h = Harness::first();
    let (mut package, program) = tiny_package(&h.trust, 2);
    match case {
        "missing" => {
            package
                .base
                .manifest
                .entries
                .iter_mut()
                .find(|entry| entry.kind == EntryKind::Data)
                .unwrap()
                .path = "data/manifest".into()
        }
        "invalid" => replace_blob(&mut package, "data/db2/catalog", b"{".to_vec()),
        "wrong" => replace_blob(&mut package, "data/db2/catalog", b"[]".to_vec()),
        _ => unreachable!(),
    }
    resign_package(&mut package, &h.trust);
    h.server.install_batch_programs(vec![program]).unwrap();
    let second = h.server.install_application_package_v2(&package).unwrap();
    let before = publication_rows(h.store.as_ref());
    let result = h.server.publish_application_generation(&second);
    let after = publication_rows(h.store.as_ref());
    save_rows(&h.server, "before", &before);
    save_rows(&h.server, "after", &after);
    eprintln!(
        "prevalidation {case}: result={result:?}, state_unchanged={}",
        before == after
    );
    // Execute the proposed newly selected route before checking structural snapshots.
    let (job, outputs) = route(&h.server, TWO).await;
    assert_refused(&job, &outputs);
    assert_eq!(result, Err(HostProblem::Malformed));
    assert_eq!(
        after, before,
        "prevalidation must precede prepared record and every provider write"
    );
    runs(&h.server, ONE, MARKER_ONE).await;
    assert_eq!(
        h.server
            .db2_service()
            .table_definition(TABLE)
            .unwrap()
            .columns[0]
            .max_bytes,
        1
    );
    let h = h.reopen().await;
    runs(&h.server, ONE, MARKER_ONE).await;
}

#[tokio::test]
async fn publication_fencing_missing_executable_refuses_actual_jes_route() {
    let h = Harness::first();
    let (_, program) = tiny_package(&h.trust, 1);
    h.server
        .artifacts
        .delete_artifact(&program.artifact)
        .unwrap();
    refuses(&h.server, ONE).await;
}
#[tokio::test]
async fn publication_fencing_substituted_mapping_refuses_actual_jes_route() {
    let h = Harness::first();
    let (_, second) = tiny_package(&h.trust, 2);
    h.server
        .install_batch_programs(vec![second.clone()])
        .unwrap();
    let current = h
        .store
        .get_provider_state("batch-program", ONE)
        .unwrap()
        .unwrap();
    h.store
        .put_provider_state(
            ProviderStateRecord {
                version: current.version + 1,
                payload: second.artifact.as_str().as_bytes().to_vec(),
                ..current.clone()
            },
            Some(current.version),
        )
        .unwrap();
    refuses(&h.server, ONE).await;
}
#[tokio::test]
async fn publication_fencing_substituted_object_refuses_actual_jes_route() {
    let h = Harness::first();
    let (_, first) = tiny_package(&h.trust, 1);
    let (_, second) = tiny_package(&h.trust, 2);
    h.server
        .install_batch_programs(vec![second.clone()])
        .unwrap();
    let path = |artifact: &ArtifactRef| {
        let digest = artifact.as_str().strip_prefix("sha256:").unwrap();
        h.config
            .artifact_root
            .join("objects")
            .join(&digest[..2])
            .join(digest)
    };
    // Out-of-band corruption of the external immutable authority; no product writer accepts it.
    std::fs::copy(path(&second.artifact), path(&first.artifact)).unwrap();
    refuses(&h.server, ONE).await;
}

fn windows(outcome: Outcome) -> Vec<Point> {
    let section = |field, state: &str| Point {
        namespace: APPLICATION_PUBLICATION_NAMESPACE,
        field: Some((field, Value::String(state.into()))),
        outcome,
    };
    vec![
        Point {
            namespace: "batch-controller-state",
            field: None,
            outcome,
        },
        section("controllers", "applying"),
        section("controllers", "applied"),
        section("db2", "applying"),
        Point {
            namespace: "db2-v1-installation",
            field: None,
            outcome,
        },
        section("db2", "applied"),
        section("ims", "applying"),
        Point {
            namespace: "ims-v1-metadata-selection",
            field: None,
            outcome,
        },
        section("ims", "applied"),
        Point {
            namespace: APPLICATION_V2_STATE_NAMESPACE,
            field: None,
            outcome,
        },
        Point {
            namespace: APPLICATION_PUBLICATION_NAMESPACE,
            field: Some(("complete", Value::Bool(true))),
            outcome,
        },
    ]
}
#[tokio::test]
async fn publication_fencing_failed_install_windows_block_jes_then_retry_reopen() {
    interrupted(false).await;
}
#[tokio::test]
async fn publication_fencing_failed_rollback_windows_block_jes_then_retry_reopen() {
    interrupted(true).await;
}
async fn interrupted(rollback: bool) {
    let mut failures = Vec::new();
    for outcome in [Outcome::Before, Outcome::After] {
        for point in windows(outcome) {
            let h = Harness::first();
            let second = h.stage_second();
            if rollback {
                h.server.publish_application_generation(&second).unwrap();
            }
            h.store.arm(point.clone());
            let result = if rollback {
                h.server.rollback_application_generation(&h.first)
            } else {
                h.server.publish_application_generation(&second)
            };
            assert!(result.is_err(), "fault must return an error: {point:?}");
            assert!(
                !h.store.armed(),
                "not a behavioral red if boundary was not reached: {point:?}"
            );
            assert_eq!(*h.store.hits.lock().unwrap(), 1);
            let case = format!("rollback={rollback} point={point:?}");
            eprintln!("HOOK FIRED: {case}, result={result:?}, hits=1");
            save_rows(
                &h.server,
                "interrupted",
                &publication_rows(h.store.as_ref()),
            );
            let complete_durable = point.namespace == APPLICATION_PUBLICATION_NAMESPACE
                && point.field.as_ref().is_some_and(|(field, value)| {
                    *field == "complete" && *value == Value::Bool(true)
                })
                && matches!(outcome, Outcome::After);
            if complete_durable {
                // Lost return after the final complete write is already safe to admit.
                runs(
                    &h.server,
                    if rollback { ONE } else { TWO },
                    if rollback { MARKER_ONE } else { MARKER_TWO },
                )
                .await;
            } else {
                // During durable partial publication even a still-retained old controller is fenced.
                observe_refusal(&h.server, ONE, &case, &mut failures).await;
                observe_refusal(&h.server, TWO, &case, &mut failures).await;
            }
            // Reopen reconciles unknown writes using the existing durable record and CAS versions.
            let h = h.reopen().await;
            let expected = if rollback { &h.first } else { &second };
            assert!(
                if rollback {
                    h.server.rollback_application_generation(expected).unwrap()
                } else {
                    h.server.publish_application_generation(expected).unwrap()
                }
                .replayed
            );
            runs(
                &h.server,
                if rollback { ONE } else { TWO },
                if rollback { MARKER_ONE } else { MARKER_TWO },
            )
            .await;
            refuses(&h.server, if rollback { TWO } else { ONE }).await;
            assert_eq!(
                h.server
                    .db2_service()
                    .table_definition(table_name(expected.generation))
                    .unwrap()
                    .columns[0]
                    .max_bytes,
                if rollback { 1 } else { 2 }
            );
            assert!(h.server.graceful_shutdown().await);
        }
    }
    assert!(
        failures.is_empty(),
        "publication refusal violations: {failures:#?}"
    );
}

#[tokio::test]
async fn publication_fencing_prepared_write_before_and_after_have_distinct_admission() {
    let mut failures = Vec::new();
    for rollback in [false, true] {
        for outcome in [Outcome::Before, Outcome::After] {
            let h = Harness::first();
            let second = h.stage_second();
            if rollback {
                h.server.publish_application_generation(&second).unwrap();
            }
            let before = publication_rows(h.store.as_ref());
            h.store.arm(Point {
                namespace: APPLICATION_PUBLICATION_NAMESPACE,
                field: None,
                outcome,
            });
            assert!(
                if rollback {
                    h.server.rollback_application_generation(&h.first)
                } else {
                    h.server.publish_application_generation(&second)
                }
                .is_err()
            );
            assert!(!h.store.armed());
            assert_eq!(*h.store.hits.lock().unwrap(), 1);
            let case = format!("prepared rollback={rollback} outcome={outcome:?}");
            eprintln!("HOOK FIRED: {case}, hits=1");
            save_rows(&h.server, "prepared-before", &before);
            save_rows(
                &h.server,
                "prepared-after",
                &publication_rows(h.store.as_ref()),
            );
            let (old, old_marker, new, new_marker) = if rollback {
                (TWO, MARKER_TWO, ONE, MARKER_ONE)
            } else {
                (ONE, MARKER_ONE, TWO, MARKER_TWO)
            };
            if matches!(outcome, Outcome::Before) {
                assert_eq!(publication_rows(h.store.as_ref()), before);
                runs(&h.server, old, old_marker).await;
            } else {
                observe_refusal(&h.server, old, &case, &mut failures).await;
            }
            observe_refusal(&h.server, new, &case, &mut failures).await;
            let h = h.reopen().await;
            if matches!(outcome, Outcome::Before) {
                runs(&h.server, old, old_marker).await;
                if rollback {
                    h.server.rollback_application_generation(&h.first).unwrap();
                } else {
                    h.server.publish_application_generation(&second).unwrap();
                }
            }
            runs(&h.server, new, new_marker).await;
            assert!(h.server.graceful_shutdown().await);
        }
    }
    assert!(
        failures.is_empty(),
        "prepared refusal violations: {failures:#?}"
    );
}

#[tokio::test]
async fn publication_fencing_historical_absent_optional_fields_and_null_sections_control() {
    let h = Harness::first();
    let (package, _) = tiny_package(&h.trust, 1);
    let mut wire = serde_json::to_value(&package).unwrap();
    assert_eq!(wire["sections"]["ims_metadata"], Value::Null);
    assert_eq!(wire["sections"]["ims_tm"], Value::Null);
    wire["sections"]
        .as_object_mut()
        .unwrap()
        .remove("ims_metadata");
    wire["sections"].as_object_mut().unwrap().remove("ims_tm");
    let historical: ApplicationPackageV2 = serde_json::from_value(wire).unwrap();
    assert_eq!(
        h.server
            .install_application_package_v2(&historical)
            .unwrap()
            .identity,
        h.first.identity
    );
    let mut record = h
        .store
        .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP)
        .unwrap()
        .unwrap();
    let old_version = record.version;
    let mut value: Value = serde_json::from_slice(&record.payload).unwrap();
    value.as_object_mut().unwrap().remove("ims");
    record.payload = serde_json::to_vec(&value).unwrap();
    record.version += 1;
    h.store
        .put_provider_state(record, Some(old_version))
        .unwrap();
    let h = h.reopen().await;
    runs(&h.server, ONE, MARKER_ONE).await;
    assert!(
        h.server
            .publish_application_generation(&h.first)
            .unwrap()
            .replayed
    );
    assert!(h.server.graceful_shutdown().await);
}

#[path = "publication_sqlite_process_tests.rs"]
mod sqlite_process;

#[path = "publication_interleaving_tests.rs"]
mod interleaving;
