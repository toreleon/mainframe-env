use super::*;

fn open(store: Arc<dyn PlatformStore>, url: Option<&str>) -> Arc<ProductServer> {
    let mut cfg = config();
    if let Some(url) = url {
        cfg.store_profile = crate::StoreProfile::Sqlite;
        cfg.sqlite_url = url.into();
    }
    ProductServer::open_with_package_trust(
        cfg,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        Arc::new(test_package_trust()),
    )
    .unwrap()
}

fn seed(server: &ProductServer, store: &Arc<dyn PlatformStore>, run: &str) -> String {
    let trust = test_package_trust();
    let mut package = signed_ims_package(&trust, 1, 1);
    package.sections.ims_metadata = Some(literal_catalog());
    resign_package(&mut package, &trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let inv = batch(server, run, "primary-producer-install");
    let mut load = recovery_db(ImsOperation::Load, 2, "primary-proof");
    load.data = serde_json::to_vec(&literal_image()).unwrap();
    for req in [
        recovery_db(ImsOperation::Schedule, 1, "primary-proof"),
        load,
        recovery_db(ImsOperation::Commit, 3, "primary-proof"),
    ] {
        assert_eq!(
            server.ims_execute_selected(APP, &inv, &req).unwrap().status,
            "  "
        );
    }
    let result = ims(drive(
        server,
        store,
        run,
        "primary-producer-prefix",
        call(4, ImsOperation::GetUnique, PREFIX),
    ))
    .unwrap();
    assert_eq!(result.segments[0].data, b"B1114x");
    eprintln!("SIGNED-PRODUCER-SETUP {}", staged.identity);
    staged.identity
}

fn mutation_call(
    seq: u64,
    operation: ImsOperation,
    segment: Option<&str>,
    data: &[u8],
) -> HostRequest {
    let mut req = recovery_db(operation, seq, "primary-proof");
    req.segments = segment.into_iter().map(str::to_owned).collect();
    req.data = data.to_vec();
    HostRequest::Ims(req)
}

fn consumers(server: &ProductServer, store: &Arc<dyn PlatformStore>, run: &str) {
    let send = |seq, req| {
        ims(drive(
            server,
            store,
            run,
            &format!("primary-consumer-{seq}"),
            req,
        ))
        .unwrap()
    };
    let request = HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
        request: recovery_db(ImsOperation::GetNext, 5, "primary-proof"),
        context: ImsExecutionContext::DbBatch,
        ssas: Some(DATA_MISS.iter().map(|s| s.to_vec()).collect()),
        key_capacity: 5,
    });
    let HostResult::ImsPcbFeedbackV1(result) =
        drive(server, store, run, "primary-consumer-feedback", request)
            .outcome
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.result.status, "GE");
    assert!(result.result.segments.is_empty());
    assert_eq!(
        result.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: "B".into(),
            segment_level: 2,
            bytes: b"A1B11".to_vec()
        }
    );
    assert_eq!(result.feedback.transferred_data_length, 0);
    assert_eq!(
        cursor(&**store, run)["primary_search"]["boundary"]["anchor_id"],
        8
    );
    assert_eq!(
        send(6, call(6, ImsOperation::GetNext, &[])).segments[0].data,
        b"E11f"
    );
    assert_eq!(
        send(7, call(7, ImsOperation::GetUnique, PREFIX)).segments[0].data,
        b"B1114x"
    );
    assert_eq!(
        send(8, call(8, ImsOperation::GetHoldNext, UU)).segments[0].data,
        b"C111a"
    );
    assert_eq!(
        send(9, mutation_call(9, ImsOperation::Replace, None, b"C111z")).status,
        "  "
    );
    assert_eq!(
        cursor(&**store, run)["held"],
        serde_json::json!({"id":3,"version":2})
    );
    assert_eq!(
        cursor(&**store, run)["primary_search"]["levels"][2]["observed_version"],
        2
    );
    assert_eq!(
        send(10, mutation_call(10, ImsOperation::Delete, None, b"")).affected_segments,
        1
    );
    assert_eq!(
        cursor(&**store, run)["primary_search"]["boundary"]["provenance"],
        "deleted"
    );
    assert_eq!(
        send(11, call(11, ImsOperation::GetHoldNext, UU)).segments[0].data,
        b"C112b"
    );
    for seq in [12, 13] {
        let r = send(seq, call(seq, ImsOperation::GetHoldNext, UU));
        assert_eq!(r.status, "GE");
        assert!(r.segments.is_empty());
        assert!(cursor(&**store, run)["held"].is_null());
    }
    assert_eq!(
        send(14, call(14, ImsOperation::GetNext, &[])).segments[0].data,
        b"D111e"
    );
    let HostRequest::Ims(mut insert) = mutation_call(15, ImsOperation::Insert, Some("C"), b"C113n")
    else {
        panic!()
    };
    insert.qualifiers = vec![
        ImsQualifier {
            segment: "A".into(),
            field: "AKEY".into(),
            value: b"A1".to_vec(),
        },
        ImsQualifier {
            segment: "B".into(),
            field: "BKEY".into(),
            value: b"B11".to_vec(),
        },
    ];
    assert_eq!(send(15, HostRequest::Ims(insert)).affected_segments, 1);
    assert_eq!(cursor(&**store, run)["current"], 13);
    assert_eq!(
        cursor(&**store, run)["primary_search"]["levels"][1]["key"],
        serde_json::json!([66, 49, 49])
    );
    assert!(cursor(&**store, run)["held"].is_null());
    assert_eq!(
        send(16, call(16, ImsOperation::GetNext, &[])).segments[0].data,
        b"D111e"
    );
    assert_eq!(
        send(17, mutation_call(17, ImsOperation::Rollback, None, b"")).status,
        "  "
    );
    assert!(cursor(&**store, run).get("primary_search").is_none());
    assert_eq!(
        send(18, call(18, ImsOperation::GetUnique, PREFIX)).segments[0].data,
        b"B1114x"
    );
    assert_eq!(
        send(19, call(19, ImsOperation::GetHoldNext, UU)).segments[0].data,
        b"C111a"
    );
    eprintln!("SIGNED-PRIMARY-CONSUMERS-PASS feedback/REPL/DLET/ISRT/backout");
}

#[test]
fn ssa_primary_consumer_signed_memory_sqlite_real_mutations_and_backout() {
    for sqlite in [false, true] {
        let file = sqlite.then(|| {
            std::env::temp_dir().join(format!(
                "signed-primary-consumer-{}.sqlite",
                std::process::id()
            ))
        });
        let _cleanup = SqliteFile(file.clone());
        let url = file
            .as_ref()
            .map(|p| format!("sqlite:{}?mode=rwc", p.display()));
        let store: Arc<dyn PlatformStore> = url.as_ref().map_or_else(
            || Arc::new(MemoryStore::new(Default::default())) as Arc<dyn PlatformStore>,
            |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
        );
        let server = open(store.clone(), url.as_deref());
        let run = "signed-primary-consumer";
        seed(&server, &store, run);
        consumers(&server, &store, run);
    }
}

#[test]
#[ignore = "requires a task-specific disposable PostgreSQL 18 database"]
fn ssa_primary_consumer_signed_postgres_configured_parity() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("disposable PostgreSQL URL required; no skip credit");
    let store: Arc<dyn PlatformStore> = Arc::new(
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    );
    // The injected platform store owns durability; configuration remains the
    // existing selected-package test profile, not a second backend authority.
    let server = open(store.clone(), None);
    let run = "signed-primary-postgres";
    seed(&server, &store, run);
    consumers(&server, &store, run);
    let mut seq = 20;
    for pattern in [
        UU,
        V,
        &[b"A       *U ".as_slice(), b"B        ", b"C        "][..],
        &[b"A        ".as_slice(), b"B       *U ", b"C        "][..],
        &[b"A       *V ".as_slice(), b"B        ", b"C        "][..],
    ] {
        let result = ims(drive(
            &server,
            &store,
            run,
            &format!("primary-pg-prefix-{seq}"),
            call(seq, ImsOperation::GetUnique, PREFIX),
        ))
        .unwrap();
        assert_eq!(result.segments[0].data, b"B1114x");
        seq += 1;
        let result = ims(drive(
            &server,
            &store,
            run,
            &format!("primary-pg-next-{seq}"),
            call(seq, ImsOperation::GetHoldNext, pattern),
        ))
        .unwrap();
        assert_eq!(result.segments[0].data, b"C111a");
        seq += 1;
    }
    eprintln!("SIGNED-PRIMARY-POSTGRES-PASS five-patterns/feedback/mutations");
}

fn recovery_call(
    identity: &str,
    seq: u64,
    call: mainframe_env_host_api::ImsRecoveryCall,
) -> HostRequest {
    HostRequest::ImsRecovery(super::super::application_recovery::recovery_request(
        identity,
        seq,
        "primary-coherent",
        call,
    ))
}

#[test]
fn ssa_primary_consumer_signed_sqlite_coherent_backup_separate_process_graph() {
    use mainframe_env_host_api::{ImsRecoveryCall, ImsRecoveryResult, ImsRestartSelection};
    const ENV: &str = "IMS_PRIMARY_COHERENT_SOURCE";
    let name = std::thread::current().name().unwrap().to_owned();
    if let Ok(source) = std::env::var(ENV) {
        let destination = std::env::var("IMS_PRIMARY_COHERENT_BACKUP").unwrap();
        let phase = std::env::var("IMS_PRIMARY_COHERENT_PHASE").unwrap();
        let run = "signed-primary-coherent";
        let url = format!(
            "sqlite:{}?mode=rwc",
            if phase == "write" {
                &source
            } else {
                &destination
            }
        );
        let sqlite = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        sqlite.integrity_check().unwrap();
        let store: Arc<dyn PlatformStore> = sqlite.clone();
        let server = open(store.clone(), Some(&url));
        if phase == "write" {
            let identity = seed(&server, &store, run);
            // XRST and symbolic CHKP belong to the same live execution. Submit
            // one actual machine with three effects, never re-admit a completed
            // execution to manufacture an XRST marker for a later checkpoint.
            let requests = vec![
                recovery_call(
                    &identity,
                    5,
                    ImsRecoveryCall::Restart {
                        selection: ImsRestartSelection::Normal,
                        area_lengths: vec![3],
                    },
                ),
                call(6, ImsOperation::GetNext, DATA_MISS),
                recovery_call(
                    &identity,
                    7,
                    ImsRecoveryCall::SymbolicCheckpoint {
                        id: "COHERENT".into(),
                        user_areas: vec![b"XYZ".to_vec()],
                    },
                ),
            ];
            let mut results = run_recovery_machine(
                &server,
                store.clone(),
                &batch(&server, run, "primary-coherent-checkpoint"),
                requests.clone(),
            );
            assert_eq!(results.len(), 3);
            for (request, result) in requests.iter().zip(&results) {
                let journal = store
                    .effect(&request.mutation().unwrap().idempotency_key)
                    .unwrap()
                    .unwrap();
                assert_eq!(journal.state, EffectState::Completed);
                assert_eq!(
                    journal.request_digest,
                    mainframe_env_host_api::canonical_request_digest(request).unwrap()
                );
                assert_eq!(
                    journal.result_digest,
                    Some(mainframe_env_host_api::canonical_result_digest(&result.outcome).unwrap())
                );
            }
            let normal = results.remove(0);
            assert!(matches!(
                normal.outcome,
                Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted { .. }))
            ));
            assert_eq!(ims(results.remove(0)).unwrap().status, "GE");
            let cp = results.remove(0);
            assert!(matches!(
                cp.outcome,
                Ok(HostResult::ImsRecovery(
                    ImsRecoveryResult::Checkpointed { .. }
                ))
            ));
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-coherent-prefix",
                    call(8, ImsOperation::GetUnique, PREFIX)
                ))
                .unwrap()
                .segments[0]
                    .data,
                b"B1114x"
            );
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-cold-search",
                    call(9, ImsOperation::GetHoldNext, UU)
                ))
                .unwrap()
                .segments[0]
                    .data,
                b"C111a"
            );
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-coherent-replace",
                    mutation_call(10, ImsOperation::Replace, None, b"C111z")
                ))
                .unwrap()
                .status,
                "  "
            );
            assert_eq!(
                cursor(&*store, run)["held"],
                serde_json::json!({"id":3,"version":2})
            );
            assert!(
                !store
                    .list_provider_state("ims-v1-generic-unit-of-work", 4096)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                !store
                    .list_provider_state("ims-recovery-v1-session", 4096)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                !store
                    .audit_records(
                        &batch(&server, run, "primary-cold-search").execution_id,
                        0,
                        64
                    )
                    .unwrap()
                    .is_empty()
            );
            sqlite
                .backup_to(std::path::Path::new(&destination))
                .unwrap();
        } else {
            assert_eq!(phase, "read");
            assert_eq!(
                cursor(&*store, run)["held"],
                serde_json::json!({"id":3,"version":2})
            );
            assert!(
                !store
                    .list_provider_state("ims-v1-generic-unit-of-work", 4096)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                !store
                    .list_provider_state("ims-recovery-v1-session", 4096)
                    .unwrap()
                    .is_empty()
            );
            let before = rows(&*store);
            assert_eq!(
                selected_replay(&server, &store, run, call(9, ImsOperation::GetHoldNext, UU))
                    .segments[0]
                    .data,
                b"C111a"
            );
            assert_eq!(rows(&*store), before);
            assert_eq!(
                cursor(&*store, run)["held"],
                serde_json::json!({"id":3,"version":2})
            );
            let effect = store
                .effect(
                    &call(9, ImsOperation::GetHoldNext, UU)
                        .mutation()
                        .unwrap()
                        .idempotency_key,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                effect.intent.owner.as_str(),
                "execution-primary-cold-search"
            );
            assert!(
                !store
                    .audit_records(&effect.execution_id, 0, 64)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-coherent-restored-repl",
                    mutation_call(11, ImsOperation::Replace, None, b"C111q")
                ))
                .unwrap()
                .status,
                "  "
            );
            assert_eq!(
                cursor(&*store, run)["held"],
                serde_json::json!({"id":3,"version":3})
            );
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-coherent-backout",
                    mutation_call(12, ImsOperation::Rollback, None, b"")
                ))
                .unwrap()
                .status,
                "  "
            );
            assert!(cursor(&*store, run).get("primary_search").is_none());
            let trust = test_package_trust();
            let mut package = signed_ims_package(&trust, 1, 1);
            package.sections.ims_metadata = Some(literal_catalog());
            resign_package(&mut package, &trust);
            let identity = server
                .install_application_package_v2(&package)
                .unwrap()
                .identity;
            let restored = drive(
                &server,
                &store,
                run,
                "primary-coherent-xrst",
                recovery_call(
                    &identity,
                    102,
                    ImsRecoveryCall::Restart {
                        selection: ImsRestartSelection::Checkpoint("COHERENT".into()),
                        area_lengths: vec![3],
                    },
                ),
            );
            assert_eq!(
                restored.outcome,
                Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted {
                    status: "  ".into(),
                    checkpoint_id: Some("COHERENT".into()),
                    user_areas: vec![b"XYZ".to_vec()],
                    pcb_statuses: vec![(1, "  ".into())]
                }))
            );
            let c = cursor(&*store, run);
            assert_eq!(c["current"], 2);
            assert_eq!(c["parentage"], 2);
            assert!(c["held"].is_null());
            assert_eq!(c["primary_search"]["boundary"]["anchor_id"], 2);
            assert_eq!(
                ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-coherent-after-xrst",
                    call(13, ImsOperation::GetNext, UU)
                ))
                .unwrap()
                .segments[0]
                    .data,
                b"C111a"
            );
            sqlite.integrity_check().unwrap();
        }
        eprintln!(
            "SIGNED-PRIMARY-COHERENT-PASS {phase} backup/session/database/undo/recovery/replay/journal/audit/package"
        );
        return;
    }
    let stem = std::env::temp_dir().join(format!("ims-primary-coherent-{}", std::process::id()));
    let source = stem.with_extension("source.sqlite");
    let destination = stem.with_extension("backup.sqlite");
    let _source_cleanup = SqliteFile(Some(source.clone()));
    let _backup_cleanup = SqliteFile(Some(destination.clone()));
    for phase in ["write", "read"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env(ENV, &source)
            .env("IMS_PRIMARY_COHERENT_BACKUP", &destination)
            .env("IMS_PRIMARY_COHERENT_PHASE", phase)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{stdout}{stderr}");
        assert!(output.status.success());
        assert!(stdout.contains("1 passed"));
        assert!(stderr.contains(&format!("SIGNED-PRIMARY-COHERENT-PASS {phase}")));
    }
}

fn cursor(store: &dyn PlatformStore, run: &str) -> serde_json::Value {
    let row = store
        .get_provider_state("ims-v1-session-index", run)
        .unwrap()
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    body["value"]["position"].clone()
}

fn rows(store: &dyn PlatformStore) -> Vec<mainframe_env_store_api::ProviderStateRecord> {
    [
        "ims-v1-session-index",
        "ims-v1-generic-database",
        "ims-v1-generic-unit-of-work",
        "ims-v1-replay",
        "ims-v1-system",
        "ims-v1-checkpoint",
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}

fn selected_replay(
    server: &ProductServer,
    store: &Arc<dyn PlatformStore>,
    run: &str,
    request: HostRequest,
) -> mainframe_env_host_api::ImsResult {
    let key = request.mutation().unwrap().idempotency_key.clone();
    let retained = store.effect(&key).unwrap().unwrap();
    assert_eq!(retained.state, EffectState::Completed);
    assert_eq!(
        retained.request_digest,
        mainframe_env_host_api::canonical_request_digest(&request).unwrap()
    );
    let HostRequest::ImsNavigation(request) = request else {
        panic!()
    };
    // Retain the original effect identity. A completed execution is not a new
    // coordinator admission; the signed selected provider resolves its receipt.
    let result = server
        .ims_navigation_selected(APP, &batch(server, run, "primary-cold-search"), &request)
        .unwrap();
    assert_eq!(
        retained.result_digest,
        Some(
            mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Ims(result.clone())))
                .unwrap()
        )
    );
    let after = store.effect(&key).unwrap().unwrap();
    assert_eq!(after.request_digest, retained.request_digest);
    assert_eq!(after.result_digest, retained.result_digest);
    assert_eq!(after.intent, retained.intent);
    assert_eq!(after.state, retained.state);
    result
}

#[test]
fn ssa_primary_producer_signed_five_patterns_ghn_ge_and_forward_release() {
    for sqlite in [false, true] {
        for (i, pattern) in [
            UU,
            V,
            &[b"A       *U ".as_slice(), b"B        ", b"C        "][..],
            &[b"A        ".as_slice(), b"B       *U ", b"C        "][..],
            &[b"A       *V ".as_slice(), b"B        ", b"C        "][..],
        ]
        .into_iter()
        .enumerate()
        {
            let run = format!("signed-primary-pattern-{i}");
            let file = sqlite.then(|| {
                std::env::temp_dir().join(format!(
                    "signed-primary-producer-{}-{i}.sqlite",
                    std::process::id()
                ))
            });
            let _cleanup = SqliteFile(file.clone());
            let url = file
                .as_ref()
                .map(|p| format!("sqlite:{}?mode=rwc", p.display()));
            let store: Arc<dyn PlatformStore> = url.as_ref().map_or_else(
                || Arc::new(MemoryStore::new(Default::default())) as Arc<dyn PlatformStore>,
                |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
            );
            let server = open(store.clone(), url.as_deref());
            seed(&server, &store, &run);
            for (seq, id, expected) in [(5, 3, b"C111a".as_slice()), (6, 4, b"C112b".as_slice())] {
                let result = ims(drive(
                    &server,
                    &store,
                    &run,
                    &format!("primary-producer-next-{seq}"),
                    call(seq, ImsOperation::GetHoldNext, pattern),
                ))
                .unwrap();
                assert_eq!(result.status, "  ");
                assert_eq!(result.segments[0].data, expected);
                assert_eq!(
                    cursor(&*store, &run)["held"],
                    serde_json::json!({"id":id,"version":1})
                );
            }
            if i <= 1 {
                for seq in [7, 8] {
                    let result = ims(drive(
                        &server,
                        &store,
                        &run,
                        &format!("primary-producer-ge-{seq}"),
                        call(seq, ImsOperation::GetHoldNext, pattern),
                    ))
                    .unwrap();
                    assert_eq!(result.status, "GE");
                    assert!(result.segments.is_empty());
                    assert!(cursor(&*store, &run)["held"].is_null());
                }
                let result = ims(drive(
                    &server,
                    &store,
                    &run,
                    "primary-producer-ordinary",
                    call(9, ImsOperation::GetNext, &[]),
                ))
                .unwrap();
                assert_eq!(result.segments[0].data, b"D111e");
            } else {
                let result = ims(drive(
                    &server,
                    &store,
                    &run,
                    "primary-producer-release",
                    call(7, ImsOperation::GetHoldNext, pattern),
                ))
                .unwrap();
                assert_eq!(
                    result.segments[0].data,
                    if i == 3 { b"C211d" } else { b"C121c" }
                );
            }
            eprintln!("SIGNED-PATTERN-PASS {i}/sqlite={sqlite}");
        }
    }
}

#[test]
fn ssa_primary_producer_signed_sqlite_separate_writer_reader_trace_hold_replay() {
    const ENV: &str = "IMS_PRIMARY_SIGNED_PROCESS_URL";
    let name = std::thread::current().name().unwrap().to_owned();
    if let Ok(url) = std::env::var(ENV) {
        let phase = std::env::var("IMS_PRIMARY_SIGNED_PROCESS_PHASE").unwrap();
        let mode = std::env::var("IMS_PRIMARY_SIGNED_PROCESS_MODE").unwrap();
        let run = "signed-primary-cold";
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let server = open(store.clone(), Some(&url));
        if phase == "write" {
            seed(&server, &store, run);
            let result = ims(drive(
                &server,
                &store,
                run,
                "primary-cold-search",
                if mode == "hold" {
                    call(5, ImsOperation::GetHoldNext, UU)
                } else {
                    call(5, ImsOperation::GetNext, DATA_MISS)
                },
            ))
            .unwrap();
            if mode == "hold" {
                assert_eq!(result.segments[0].data, b"C111a");
                assert_eq!(
                    cursor(&*store, run)["held"],
                    serde_json::json!({"id":3,"version":1})
                );
            } else {
                assert_eq!(result.status, "GE");
                assert!(result.segments.is_empty());
                let state = cursor(&*store, run);
                assert_eq!(state["primary_search"]["boundary"]["anchor_id"], 8);
                assert_eq!(
                    state["primary_search"]["feedback_path"],
                    serde_json::json!([{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,49]}])
                );
            }
        } else {
            assert_eq!(phase, "read");
            if mode == "hold" {
                assert_eq!(
                    cursor(&*store, run)["held"],
                    serde_json::json!({"id":3,"version":1})
                );
                let mut request = recovery_db(ImsOperation::Replace, 6, "primary-proof");
                request.segments = vec!["C".into()];
                request.data = b"C111z".to_vec();
                assert_eq!(
                    ims(drive(
                        &server,
                        &store,
                        run,
                        "primary-cold-replace",
                        HostRequest::Ims(request)
                    ))
                    .unwrap()
                    .status,
                    "  "
                );
                let stable = rows(&*store);
                let result =
                    selected_replay(&server, &store, run, call(5, ImsOperation::GetHoldNext, UU));
                assert_eq!(result.segments[0].data, b"C111a");
                assert_eq!(rows(&*store), stable);
                assert_eq!(
                    cursor(&*store, run)["held"],
                    serde_json::json!({"id":3,"version":2})
                );
            } else {
                let result = ims(drive(
                    &server,
                    &store,
                    run,
                    "primary-cold-continuation",
                    call(
                        6,
                        ImsOperation::GetNext,
                        if mode == "ordinary" { &[] } else { UU },
                    ),
                ))
                .unwrap();
                assert_eq!(
                    result.segments[0].data,
                    if mode == "ordinary" {
                        b"E11f".as_slice()
                    } else {
                        b"C111a".as_slice()
                    }
                );
                let stable = rows(&*store);
                let result = selected_replay(
                    &server,
                    &store,
                    run,
                    call(5, ImsOperation::GetNext, DATA_MISS),
                );
                assert_eq!(result.status, "GE");
                assert!(result.segments.is_empty());
                assert_eq!(rows(&*store), stable);
            }
        }
        eprintln!("SIGNED-PRIMARY-COLD-PASS {mode}/{phase}");
        return;
    }
    for mode in ["ordinary", "constrained", "hold"] {
        let path = std::env::temp_dir().join(format!(
            "signed-primary-cold-{}-{mode}.sqlite",
            std::process::id()
        ));
        let _cleanup = SqliteFile(Some(path.clone()));
        let url = format!("sqlite:{}?mode=rwc", path.display());
        for phase in ["write", "read"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(ENV, &url)
                .env("IMS_PRIMARY_SIGNED_PROCESS_MODE", mode)
                .env("IMS_PRIMARY_SIGNED_PROCESS_PHASE", phase)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("{stdout}{stderr}");
            assert!(output.status.success());
            assert!(stdout.contains("1 passed"));
            assert!(stderr.contains(&format!("SIGNED-PRIMARY-COLD-PASS {mode}/{phase}")));
        }
    }
}
