//! First direct child through the signed selected package and real coordinator.
use super::application_recovery::{recovery_db, run_recovery_machine};
use super::*;
use mainframe_env_host_api::{ImsExecutionContext, ImsNavigationRequest};
use mainframe_env_ims::{ImsGenericLoadImage, ImsGenericLoadRecord};

#[path = "ssa_first_tests/recovery_tests.rs"]
mod recovery_tests;

const RUN: &str = "signed-ssa-first";
const APP: &str = "SIGNED-IMS-APPLICATION";

fn open(store: Arc<dyn PlatformStore>, config: ServerConfig) -> Arc<ProductServer> {
    ProductServer::open_with_package_trust(
        config,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        Arc::new(test_package_trust()),
    )
    .unwrap()
}

fn call(seq: u64, pcb: u16, op: ImsOperation, ssas: &[&[u8]]) -> HostRequest {
    let mut req = recovery_db(op, seq, "signed-first");
    req.pcb = pcb;
    HostRequest::ImsNavigation(ImsNavigationRequest {
        request: req,
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    })
}

fn batch(server: &ProductServer, execution: &str) -> Invocation {
    let mut invocation = tm_invocation(RUN, execution);
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
        [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
            .into_iter()
            .collect(),
        InvocationLimits::default(),
    )
    .unwrap();
    invocation
}

fn install(server: &ProductServer) -> String {
    install_variant(server, None, false)
}

fn install_variant(server: &ProductServer, condition: Option<u8>, empty: bool) -> String {
    let trust = test_package_trust();
    let mut package = signed_ims_package(&trust, 1, 1);
    let catalog = package.sections.ims_metadata.as_mut().unwrap();
    for segment in &mut catalog.databases[0].segments {
        segment.min_length = 3;
        segment.max_length = 3;
        segment.fields[0].length = 2;
    }
    let ImsPcbMetadata::Database(first) = &mut catalog.psbs[0].pcbs[0] else {
        unreachable!()
    };
    first.sensitive_segments[1].processing_options = None;
    let mut other = first.clone();
    other.name = "OTHERPCB".into();
    catalog.psbs[0].pcbs.push(ImsPcbMetadata::Database(other));
    let ImsPcbMetadata::Database(other) = &mut catalog.psbs[0].pcbs[1] else {
        unreachable!()
    };
    match condition {
        Some(0) => {
            other.sensitive_segments.pop();
        }
        Some(1) => other.sensitive_segments[1].processing_options = Some("I".into()),
        _ => {}
    }
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
    let invocation = batch(server, "signed-first-load");
    let mut image = ImsGenericLoadImage {
        database: "AUTHDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A0X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C1Q".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C2R".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C3S".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2Y".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(5),
                data: b"D1T".to_vec(),
            },
        ],
    };
    if empty {
        image
            .records
            .retain(|r| r.data == b"A0X" || r.data == b"B2Y");
    }
    let mut load = recovery_db(ImsOperation::Load, 900, "signed-first-load");
    load.data = serde_json::to_vec(&image).unwrap();
    assert_eq!(
        server
            .ims_execute_selected(APP, &invocation, &load)
            .unwrap()
            .status,
        "  "
    );
    assert_eq!(
        server
            .ims_execute_selected(
                APP,
                &invocation,
                &recovery_db(ImsOperation::Schedule, 901, "signed-first-load")
            )
            .unwrap()
            .status,
        "  "
    );
    server
        .ims_execute_selected(
            APP,
            &invocation,
            &recovery_db(ImsOperation::Commit, 902, "signed-first-load"),
        )
        .unwrap();
    staged.identity
}

fn result(effect: &EffectResult) -> &mainframe_env_host_api::ImsResult {
    match &effect.outcome {
        Ok(HostResult::Ims(r)) => r,
        other => panic!("IMS result: {other:?}"),
    }
}

fn fail_first(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let server = open(store.clone(), config);
    install(&server);
    let invocation = batch(&server, "signed-first-first");
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &invocation,
        vec![
            call(3, 1, ImsOperation::GetUnique, &[b"ROOT    (ROOTKEY EQA1)"]),
            call(4, 1, ImsOperation::GetNextParent, &[b"CHILD    "]),
            call(5, 1, ImsOperation::GetNextParent, &[b"CHILD   *F "]),
        ],
    );
    assert_eq!(result(&results[0]).segments[0].data, b"A1X");
    assert_eq!(result(&results[1]).segments[0].data, b"C1Q");
    assert_eq!(
        results[2].outcome.as_ref().map(|r| match r {
            HostResult::Ims(r) => r.segments[0].data.as_slice(),
            _ => b"".as_slice(),
        }),
        Ok(b"C1Q".as_slice())
    );
    assert_eq!(&result(&results[2]).segments[0].data[..2], b"C1");
    assert_eq!(
        result(&results[2]).segments[0].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    let request = call(5, 1, ImsOperation::GetNextParent, &[b"CHILD   *F "]);
    let journal = store
        .effect(&request.mutation().unwrap().idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(journal.state, EffectState::Completed);
    assert_eq!(
        journal.request_digest,
        mainframe_env_host_api::canonical_request_digest(&request).unwrap()
    );
    let expected = mainframe_env_host_api::ImsResult {
        status: "  ".into(),
        segments: vec![mainframe_env_host_api::ImsSegment {
            name: "CHILD".into(),
            parent_key: Some(b"A1".to_vec()),
            data: b"C1Q".to_vec(),
        }],
        checkpoint_id: None,
        affected_segments: 0,
        system: None,
    };
    assert_eq!(result(&results[2]), &expected);
    assert_eq!(
        journal.result_digest,
        Some(
            mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Ims(expected)))
                .unwrap()
        )
    );
}

#[test]
fn ssa_first_direct_child_fail_first_signed_memory() {
    fail_first(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn ssa_first_direct_child_fail_first_signed_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "signed-ssa-first-fail-first-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url;
    fail_first(store, config);
    std::fs::remove_file(file).unwrap();
}

fn selected(
    server: &ProductServer,
    inv: &Invocation,
    request: HostRequest,
) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
    let HostRequest::ImsNavigation(req) = request else {
        panic!("navigation")
    };
    server.ims_navigation_selected(APP, inv, &req)
}

fn cursor(store: &dyn PlatformStore, pcb: u16) -> serde_json::Value {
    let row = store
        .get_provider_state("ims-v1-session-index", RUN)
        .unwrap()
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    if pcb == 1 {
        body["value"]["position"].clone()
    } else {
        body["value"]["pcb_positions"][pcb.to_string()].clone()
    }
}

fn rows(store: &dyn PlatformStore) -> Vec<mainframe_env_store_api::ProviderStateRecord> {
    [
        "ims-v1-generic-database",
        "ims-v1-session-index",
        "ims-v1-generic-unit-of-work",
        "ims-v1-replay",
        "ims-v1-system",
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}

fn backends(name: &str, mut test: impl FnMut(Arc<dyn PlatformStore>, ServerConfig, Option<&str>)) {
    eprintln!("scenario signed-{name}/memory");
    test(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        None,
    );
    let file =
        std::env::temp_dir().join(format!("signed-first-{name}-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Sqlite;
    cfg.sqlite_url = url.clone();
    eprintln!("scenario signed-{name}/sqlite");
    test(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        cfg,
        Some(&url),
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn ssa_first_direct_child_signed_forward_hold_ge_pcbs_parentage_replay_and_reopen() {
    backends("forward", |store, cfg, url| {
        let server = open(store.clone(), cfg.clone());
        install(&server);
        let inv = batch(&server, "signed-first-forward");
        selected(
            &server,
            &inv,
            call(
                3,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQB2)"],
            ),
        )
        .unwrap();
        let other = cursor(&*store, 2);
        for (i, op) in [ImsOperation::GetNextParent, ImsOperation::GetHoldNextParent]
            .into_iter()
            .enumerate()
        {
            for advance in 0..=3 {
                let seq = 10 + i as u64 * 100 + advance * 20;
                let mut calls = vec![call(
                    seq,
                    1,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA1)"],
                )];
                for offset in 0..advance {
                    calls.push(call(
                        seq + 1 + offset,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[b"CHILD    "],
                    ));
                }
                calls.push(call(seq + 5, 1, op, &[b"CHILD   *F "]));
                let coordinator_inv = batch(&server, &format!("signed-first-forward-{seq}"));
                let effects = run_recovery_machine(&server, store.clone(), &coordinator_inv, calls);
                let found = result(effects.last().unwrap());
                assert_eq!(found.status, "  ");
                assert_eq!(found.segments[0].data, b"C1Q");
                assert_eq!(
                    found.segments[0].parent_key.as_deref(),
                    Some(b"A1".as_slice())
                );
                assert_eq!(
                    cursor(&*store, 1),
                    serde_json::json!({"current":3,"parentage":2,
                    "held":if op==ImsOperation::GetHoldNextParent {serde_json::json!({"id":3,"version":1})} else {serde_json::Value::Null},"after_end":false})
                );
                for offset in [6, 7] {
                    let r = selected(
                        &server,
                        &inv,
                        call(seq + offset, 1, op, &[b"CHILD   *-F- "]),
                    )
                    .unwrap();
                    assert_eq!(r.status, "  ");
                    assert_eq!(r.segments[0].data, b"C1Q");
                    assert_eq!(
                        cursor(&*store, 1),
                        serde_json::json!({"current":3,"parentage":2,
                        "held":if op==ImsOperation::GetHoldNextParent {serde_json::json!({"id":3,"version":1})} else {serde_json::Value::Null},"after_end":false})
                    );
                }
                assert_eq!(
                    selected(
                        &server,
                        &inv,
                        call(seq + 8, 1, ImsOperation::GetNextParent, &[b"CHILD    "])
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C2R"
                );
                selected(
                    &server,
                    &inv,
                    call(
                        seq + 9,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[b"CHILD   *F "],
                    ),
                )
                .unwrap();
                assert_eq!(
                    selected(&server, &inv, call(seq + 10, 1, ImsOperation::GetNext, &[]))
                        .unwrap()
                        .segments[0]
                        .data,
                    b"C2R"
                );
                assert_eq!(cursor(&*store, 2), other);
            }
        }
        for (i, op) in [ImsOperation::GetNext, ImsOperation::GetHoldNext]
            .into_iter()
            .enumerate()
        {
            let seq = 300 + i as u64 * 10;
            selected(
                &server,
                &inv,
                call(
                    seq,
                    1,
                    ImsOperation::GetUnique,
                    &[b"ROOT    (ROOTKEY EQA0)"],
                ),
            )
            .unwrap();
            assert_eq!(
                selected(
                    &server,
                    &inv,
                    call(seq + 1, 1, op, &[b"ROOT    (ROOTKEY EQA1)"])
                )
                .unwrap()
                .segments[0]
                    .data,
                b"A1X"
            );
            assert_eq!(
                selected(
                    &server,
                    &inv,
                    call(
                        seq + 2,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[b"CHILD   *F "]
                    )
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C1Q"
            );
        }
        selected(
            &server,
            &inv,
            call(
                330,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    *P(ROOTKEY EQA1)", b"CHILD   (CHILDKEYEQC1)"],
            ),
        )
        .unwrap();
        let replay = call(331, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]);
        assert_eq!(
            selected(&server, &inv, replay.clone()).unwrap().segments[0].data,
            b"C1Q"
        );
        selected(&server, &inv, call(332, 1, ImsOperation::GetNext, &[])).unwrap();
        let before = rows(&*store);
        assert_eq!(
            selected(&server, &inv, replay.clone()).unwrap().segments[0].data,
            b"C1Q"
        );
        assert_eq!(rows(&*store), before);
        let HostRequest::ImsNavigation(mut changed) = replay.clone() else {
            unreachable!()
        };
        changed.ssas[0] = b"CHILD   *-F- ".to_vec();
        assert_eq!(
            server.ims_navigation_selected(APP, &inv, &changed),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        let invalid = call(
            333,
            1,
            ImsOperation::GetNextParent,
            &[b"ROOT    (ROOTKEY EQA1)", b"CHILD   *F "],
        );
        assert_eq!(
            selected(&server, &inv, invalid),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*store), before);
        let mut denied = inv.clone();
        denied.principal = Principal::new(
            PrincipalId::new("NOUSER", InvocationLimits::default()).unwrap(),
            [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
                .into_iter()
                .collect(),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            selected(&server, &denied, replay.clone()),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), before);
        drop(server);
        let reopened: Arc<dyn PlatformStore> = url.map_or_else(
            || store.clone(),
            |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
        );
        let server = open(reopened.clone(), cfg);
        let inv = batch(&server, "signed-first-forward");
        assert_eq!(
            selected(&server, &inv, replay).unwrap().segments[0].data,
            b"C1Q"
        );
        assert_eq!(rows(&*reopened), before);
        assert_eq!(
            cursor(&*reopened, 1),
            serde_json::json!({"current":4,"parentage":4,"held":null,"after_end":false})
        );
    });
}

#[test]
fn ssa_first_direct_child_signed_ac_am_and_empty_root_exhaustion() {
    for condition in [0, 1] {
        backends(&format!("condition-{condition}"), |store, cfg, _| {
            let server = open(store.clone(), cfg);
            install_variant(&server, Some(condition), false);
            let inv = batch(&server, "signed-first-condition");
            selected(
                &server,
                &inv,
                call(
                    3,
                    2,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA1)"],
                ),
            )
            .unwrap();
            let before = cursor(&*store, 2);
            let database = store
                .get_provider_state("ims-v1-generic-database", "AUTHDB")
                .unwrap()
                .unwrap()
                .payload;
            let request = call(4, 2, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]);
            let effects = run_recovery_machine(&server, store.clone(), &inv, vec![request.clone()]);
            assert_eq!(
                result(&effects[0]).status,
                if condition == 0 { "AC" } else { "AM" }
            );
            assert!(result(&effects[0]).segments.is_empty());
            assert_eq!(cursor(&*store, 2), before);
            assert_eq!(
                store
                    .get_provider_state("ims-v1-generic-database", "AUTHDB")
                    .unwrap()
                    .unwrap()
                    .payload,
                database
            );
            let journal = store
                .effect(&request.mutation().unwrap().idempotency_key)
                .unwrap()
                .unwrap();
            assert_eq!(journal.state, EffectState::Completed);
            let before = rows(&*store);
            assert_eq!(
                selected(&server, &inv, request).unwrap(),
                *result(&effects[0])
            );
            assert_eq!(rows(&*store), before);
        });
    }
    backends("empty-root", |store, cfg, _| {
        let server = open(store.clone(), cfg);
        install_variant(&server, None, true);
        for (i, op) in [ImsOperation::GetNextParent, ImsOperation::GetHoldNextParent]
            .into_iter()
            .enumerate()
        {
            let seq = 10 + i as u64 * 20;
            let inv = batch(&server, &format!("signed-first-empty-{seq}"));
            let effects = run_recovery_machine(
                &server,
                store.clone(),
                &inv,
                vec![
                    call(
                        seq,
                        1,
                        ImsOperation::GetHoldUnique,
                        &[b"ROOT    (ROOTKEY EQA0)"],
                    ),
                    call(seq + 1, 1, op, &[b"CHILD   *F "]),
                ],
            );
            assert_eq!(result(&effects[1]).status, "GE");
            assert!(result(&effects[1]).segments.is_empty());
            assert_eq!(
                cursor(&*store, 1),
                serde_json::json!({"current":1,"parentage":1,"held":null,"after_end":false})
            );
            for offset in [2, 3] {
                assert_eq!(
                    selected(&server, &inv, call(seq + offset, 1, op, &[b"CHILD   *F "]))
                        .unwrap()
                        .status,
                    "GE"
                );
            }
            assert_eq!(
                selected(
                    &server,
                    &inv,
                    call(seq + 4, 1, ImsOperation::GetNextParent, &[b"CHILD    "])
                )
                .unwrap()
                .status,
                "GE"
            );
            assert_eq!(
                selected(&server, &inv, call(seq + 5, 1, ImsOperation::GetNext, &[]))
                    .unwrap()
                    .segments[0]
                    .data,
                b"B2Y"
            );
        }
    });
}
