//! A fresh executable process owns every SQLite restart phase.
use super::*;
use std::process::Command;
const CHILD: &str =
    "product::tests::publication_fencing_tests::sqlite_process::publication_fencing_sqlite_child";
const ROOT_ENV: &str = "CV204_PUBLICATION_TEST_ROOT";
const PHASE_ENV: &str = "CV204_PUBLICATION_TEST_PHASE";

#[derive(serde::Serialize, serde::Deserialize)]
struct Records {
    first: ApplicationGenerationRecord,
    second: ApplicationGenerationRecord,
}
fn child(
    root: &std::path::Path,
    phase: &str,
    rollback: bool,
    point: usize,
) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROOT_ENV, root)
        .env(PHASE_ENV, phase)
        .env(
            "CV204_PUBLICATION_TEST_ROLLBACK",
            if rollback { "1" } else { "0" },
        )
        .env("CV204_PUBLICATION_TEST_POINT", point.to_string())
        .output()
        .unwrap()
}
#[tokio::test]
#[ignore = "subprocess entrypoint; parent supplies isolated file-SQLite fixture"]
async fn publication_fencing_sqlite_child() {
    let root = std::env::var_os(ROOT_ENV).expect("subprocess parent must supply fixture root");
    let root = std::path::PathBuf::from(root);
    let phase = std::env::var(PHASE_ENV).unwrap();
    let rollback = std::env::var("CV204_PUBLICATION_TEST_ROLLBACK").unwrap() == "1";
    let point: usize = std::env::var("CV204_PUBLICATION_TEST_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Sqlite;
    cfg.sqlite_url = format!("sqlite:{}?mode=rwc", root.join("state.sqlite").display());
    cfg.artifact_root = root.join("artifacts");
    let store = Arc::new(FaultStore::new(Arc::new(
        SqliteStateStore::open(&cfg.sqlite_url, 64 * 1024 * 1024, 262144).unwrap(),
    )));
    let trust = Arc::new(test_package_trust());
    if phase == "recover-fail" {
        store.arm(Point {
            namespace: APPLICATION_PUBLICATION_NAMESPACE,
            field: Some(("complete", Value::Bool(true))),
            outcome: Outcome::Before,
        });
    }
    let opened = ProductServer::open_with_package_trust(
        cfg,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    );
    if phase == "recover-fail" {
        assert!(
            opened.is_err(),
            "a failed recovery must expose no ProductServer/router"
        );
        assert!(
            !store.armed(),
            "recovery must reach the injected final write"
        );
        return;
    }
    let server = opened.unwrap();
    if phase == "setup" {
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let (first_package, first_program) = tiny_package(&trust, 1);
        let (second_package, second_program) = tiny_package(&trust, 2);
        server
            .install_batch_programs(vec![first_program, second_program])
            .unwrap();
        let first = server
            .install_application_package_v2(&first_package)
            .unwrap();
        server.publish_application_generation(&first).unwrap();
        let second = server
            .install_application_package_v2(&second_package)
            .unwrap();
        if rollback {
            server.publish_application_generation(&second).unwrap();
        }
        std::fs::write(
            root.join("records.json"),
            serde_json::to_vec(&Records { first, second }).unwrap(),
        )
        .unwrap();
        runs(
            &server,
            if rollback { TWO } else { ONE },
            if rollback { MARKER_TWO } else { MARKER_ONE },
        )
        .await;
    } else {
        let records: Records =
            serde_json::from_slice(&std::fs::read(root.join("records.json")).unwrap()).unwrap();
        if phase == "crash" {
            store.arm(windows(Outcome::ExitAfter).remove(point));
            let result = if rollback {
                server.rollback_application_generation(&records.first)
            } else {
                server.publish_application_generation(&records.second)
            };
            panic!("crash boundary was not reached: {result:?}");
        }
        assert_eq!(phase, "recover");
        let expected = if rollback {
            &records.first
        } else {
            &records.second
        };
        assert!(
            if rollback {
                server.rollback_application_generation(expected).unwrap()
            } else {
                server.publish_application_generation(expected).unwrap()
            }
            .replayed
        );
        runs(
            &server,
            if rollback { ONE } else { TWO },
            if rollback { MARKER_ONE } else { MARKER_TWO },
        )
        .await;
        refuses(&server, if rollback { TWO } else { ONE }).await;
        assert_eq!(
            server
                .db2_service()
                .table_definition(table_name(expected.generation))
                .unwrap()
                .columns[0]
                .max_bytes,
            if rollback { 1 } else { 2 }
        );
        let row = store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP)
            .unwrap()
            .unwrap();
        let state: Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(state["complete"], Value::Bool(true));
        assert_eq!(state["identity"], Value::String(expected.identity.clone()));
    }
    assert!(server.graceful_shutdown().await);
}
#[test]
fn publication_fencing_sqlite_subprocess_install_and_rollback_restart() {
    for rollback in [false, true] {
        let points = windows(Outcome::ExitAfter);
        for (point, boundary) in points.iter().enumerate() {
            let (config, _directory) = test_config();
            let root = config.artifact_root;
            for (ordinal, (phase, expected_exit)) in [
                ("setup", 0),
                ("crash", 86),
                ("recover-fail", 0),
                ("recover", 0),
                ("recover", 0),
            ]
            .into_iter()
            .enumerate()
            {
                if phase == "recover-fail"
                    && boundary
                        .field
                        .as_ref()
                        .is_some_and(|(field, _)| *field == "complete")
                {
                    continue;
                }
                let output = child(&root, phase, rollback, point);
                // Preserve child evidence outside Git, never in the disposable Cargo target.
                std::fs::write(
                    root.join(format!("{ordinal}-{phase}.stdout")),
                    &output.stdout,
                )
                .unwrap();
                std::fs::write(
                    root.join(format!("{ordinal}-{phase}.stderr")),
                    &output.stderr,
                )
                .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(expected_exit),
                    "{rollback} {point} {phase}: {output:?}"
                );
            }
        }
    }
}
