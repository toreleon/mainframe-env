use super::trace_tests::{backends, cursor, position, rows};
use super::*;

#[test]
fn ssa_primary_producer_absent_strict_reader_and_live_historical_fences() {
    let old = br#"{"current":2,"parentage":2,"held":null,"after_end":false}"#;
    let legacy: PcbPosition = serde_json::from_slice(old).unwrap();
    assert_eq!(serde_json::to_vec(&legacy).unwrap(), old);
    backends("reader", |store, _| {
        let run = "primary-reader";
        let service = seed(store, run);
        nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS).unwrap();
        let valid = cursor(&service, run, 1);
        let locked = service.lock().unwrap();
        let engine = restored(&locked.state, "GENDB", Default::default()).unwrap();
        let selected = position_without_lock(&locked.state, run);
        engine.validate_position(&selected).unwrap();
        let wire = serde_json::to_vec(&selected).unwrap();
        assert_eq!(
            serde_json::to_vec(&serde_json::from_slice::<PcbPosition>(&wire).unwrap()).unwrap(),
            wire
        );
        for (path, value) in [
            ("/primary_search/version", serde_json::json!(2)),
            ("/primary_search/observed_next_id", serde_json::json!(2)),
            ("/primary_search/observed_revision", serde_json::json!(11)),
            ("/primary_search/levels/1/key", serde_json::json!([66, 49])),
            ("/primary_search/levels/0", serde_json::Value::Null),
            ("/primary_search/levels/1/parent", serde_json::json!(10)),
            ("/primary_search/boundary/anchor_id", serde_json::json!(7)),
            (
                "/primary_search/boundary/path/1/key",
                serde_json::json!([66, 49, 49]),
            ),
            ("/after_end", serde_json::json!(true)),
            ("/primary_search/levels/1/id", serde_json::json!(6)),
        ] {
            let mut corrupt = valid.clone();
            *corrupt.pointer_mut(path).unwrap() = value;
            let position: PcbPosition = serde_json::from_value(corrupt).unwrap();
            assert!(
                engine.validate_position(&position).is_err(),
                "corruption {path}"
            );
        }
        let mut unknown = valid.clone();
        unknown["primary_search"]["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<PcbPosition>(unknown).is_err());
        let json = String::from_utf8(wire).unwrap();
        let duplicate = json.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
        assert!(serde_json::from_str::<PcbPosition>(&duplicate).is_err());
        // Standalone strict-reader fixture only, never installed as cursor state.
        let mut historical = valid.clone();
        historical["primary_search"]["boundary"]["anchor_id"] = serde_json::json!(5);
        historical["primary_search"]["boundary"]["provenance"] = serde_json::json!("deleted");
        let historical: PcbPosition = serde_json::from_value(historical).unwrap();
        engine
            .validate_primary_position(&historical, false)
            .unwrap();
        assert!(engine.validate_primary_position(&historical, true).is_err());
        assert!(legacy.primary_feedback_path().is_none());
    });
}

#[test]
fn ssa_primary_producer_runtime_keys_above_recovery_encoded_key_cap() {
    backends("wide-key", |store, _| {
        let run = "primary-wide-key";
        let mut catalog = metadata();
        let root = &mut catalog.databases[0].segments[0];
        root.min_length = 300;
        root.max_length = 300;
        root.fields[0].length = 300;
        let service = ImsService::open(store, Default::default()).unwrap();
        service.install_metadata(catalog).unwrap();
        let mut image = image();
        image.records[0].data = vec![65; 300];
        image.records[9].data = vec![66; 300];
        let mut load = request(run, ImsOperation::Load, 2, &[], b"");
        load.data = serde_json::to_vec(&image).unwrap();
        for req in [
            request(run, ImsOperation::Schedule, 1, &[], b""),
            load,
            request(run, ImsOperation::Commit, 3, &[], b""),
        ] {
            let HostResult::Ims(result) = invoke(&service, run, HostRequest::Ims(req)).unwrap()
            else {
                panic!()
            };
            assert_eq!(result.status, "  ");
        }
        assert_eq!(
            nav(
                &service,
                run,
                4,
                ImsOperation::GetUnique,
                &[b"A        ", b"B       (BKEY    EQB11)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"B1114x"
        );
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetNext, UU)
                .unwrap()
                .segments[0]
                .data,
            b"C111a"
        );
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["levels"][0]["key"],
            serde_json::json!(vec![65; 300])
        );
    });
}

fn position_without_lock(state: &State, run: &str) -> PcbPosition {
    super::super::super::super::pcb::position(&state.sessions[run], 1)
}

trait RecoveryStore: ProviderStateStore + mainframe_env_store_api::IdempotencyStore {}
impl<T: ProviderStateStore + mainframe_env_store_api::IdempotencyStore> RecoveryStore for T {}

fn recover(
    service: &Arc<ImsService>,
    store: Arc<dyn RecoveryStore>,
    inv: &Invocation,
    seq: u64,
    call: mainframe_env_host_api::ImsRecoveryCall,
) -> mainframe_env_host_api::ImsRecoveryResult {
    use mainframe_env_store_api::{
        EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState,
    };
    let request = mainframe_env_host_api::ImsRecoveryRequest {
        application: "PRIMARYAPP".into(),
        package_identity: format!("sha256:{}", "a".repeat(64)),
        psb: "GENPSB".into(),
        database: "GENDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call,
        mutation: Mutation {
            sequence: seq,
            idempotency_key: IdempotencyKey::new(
                format!("{}-recovery-{seq}", inv.run_unit_id.as_str()),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    };
    let host = HostRequest::ImsRecovery(request.clone());
    store
        .record_intent(EffectRecord {
            execution_id: inv.execution_id.clone(),
            run_unit_id: inv.run_unit_id.clone(),
            sequence: seq,
            key: request.mutation.idempotency_key.clone(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: mainframe_env_host_api::canonical_request_digest(&host).unwrap(),
            intent: EffectIntentMetadata {
                owner: inv.execution_id.clone(),
                attempt: inv.attempt,
                capability: Some(
                    CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap(),
                ),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 100,
                epoch: seq,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
        })
        .unwrap();
    let result =
        crate::ims_providers_with_recovery(service.clone(), store, InvocationLimits::default())[1]
            .invoke(
                inv,
                EffectRequest {
                    run_unit: inv.run_unit_id.clone(),
                    sequence: seq,
                    idempotency_key: Some(request.mutation.idempotency_key),
                    deadline_tick: inv.deadline_tick,
                    request: host,
                },
            )
            .outcome
            .unwrap();
    let HostResult::ImsRecovery(result) = result else {
        panic!()
    };
    result
}

fn checkpoint(store: Arc<dyn RecoveryStore>, url: Option<&str>) {
    use mainframe_env_host_api::{ImsRecoveryCall, ImsRecoveryResult, ImsRestartSelection};
    let run = "primary-checkpoint";
    let service = seed(store.clone(), run);
    drop(service);
    let service = ImsService::open_authorized(
        store.clone(),
        Default::default(),
        Arc::new(Policy::default()),
    )
    .unwrap();
    service
        .publish_metadata_generation(
            "PRIMARYAPP",
            1,
            &format!("sha256:{}", "a".repeat(64)),
            Some(&metadata()),
        )
        .unwrap();
    let mut inv = invocation_class(run, ServiceClass::Batch);
    assert!(matches!(
        recover(
            &service,
            store.clone(),
            &inv,
            100,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3]
            }
        ),
        ImsRecoveryResult::Restarted { .. }
    ));
    assert_eq!(
        nav(&service, run, 5, ImsOperation::GetHoldUnique, PREFIX)
            .unwrap()
            .segments[0]
            .data,
        b"B1114x"
    );
    assert_eq!(
        nav(&service, run, 6, ImsOperation::GetHoldNext, DATA_MISS)
            .unwrap()
            .status,
        "GE"
    );
    assert!(!position(&service, run, 1).is_held());
    assert!(matches!(
        recover(
            &service,
            store.clone(),
            &inv,
            101,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "PRIMB11".into(),
                user_areas: vec![b"XYZ".to_vec()]
            }
        ),
        ImsRecoveryResult::Checkpointed { .. }
    ));
    assert_eq!(position(&service, run, 1), PcbPosition::default());
    drop(service);
    let reopened: Arc<dyn RecoveryStore> = if let Some(url) = url {
        Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap())
    } else {
        store
    };
    let service = ImsService::open_authorized(
        reopened.clone(),
        Default::default(),
        Arc::new(Policy::default()),
    )
    .unwrap();
    inv.execution_id = ExecutionId::new("primary-xrst-new", InvocationLimits::default()).unwrap();
    assert_eq!(
        recover(
            &service,
            reopened,
            &inv,
            102,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("PRIMB11".into()),
                area_lengths: vec![3]
            }
        ),
        ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("PRIMB11".into()),
            user_areas: vec![b"XYZ".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        }
    );
    assert_eq!(cursor(&service, run, 1)["current"], 2);
    assert_eq!(cursor(&service, run, 1)["parentage"], 2);
    assert!(!position(&service, run, 1).is_held());
    assert_eq!(
        cursor(&service, run, 1)["primary_search"]["boundary"]["anchor_id"],
        2
    );
    assert_eq!(
        nav(&service, run, 7, ImsOperation::GetNext, UU)
            .unwrap()
            .segments[0]
            .data,
        b"C111a"
    );
    eprintln!("PRIMARY-CHKP-XRST-PASS GE/B11-prefix, actual GU, no hold");
}

#[test]
fn ssa_primary_producer_actual_chkp_ge_prefix_xrst_gu_reopen() {
    checkpoint(Arc::new(MemoryStore::new(Default::default())), None);
    let path = std::env::temp_dir().join(format!(
        "ims-primary-checkpoint-{}.sqlite",
        std::process::id()
    ));
    let _cleanup = SqliteFile(Some(path.clone()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    checkpoint(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Some(&url),
    );
}

#[test]
#[ignore = "requires a task-specific disposable PostgreSQL 18 database"]
fn ssa_primary_consumer_public_postgres_configured_parity() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("disposable PostgreSQL URL required; no skip credit");
    let store = Arc::new(
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    );
    let run = "primary-postgres";
    let service = seed(store.clone(), run);
    let HostResult::ImsPcbFeedbackV1(result) = invoke(
        &service,
        run,
        HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
            request: request(run, ImsOperation::GetNext, 5, &[], b""),
            context: ImsExecutionContext::DbBatch,
            ssas: Some(DATA_MISS.iter().map(|s| s.to_vec()).collect()),
            key_capacity: 5,
        }),
    )
    .unwrap() else {
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
        cursor(&service, run, 1)["primary_search"]["boundary"]["anchor_id"],
        8
    );
    assert_eq!(
        nav(&service, run, 6, ImsOperation::GetNext, &[])
            .unwrap()
            .segments[0]
            .data,
        b"E11f"
    );
    let mut seq = 7;
    for pattern in [
        UU,
        V,
        &[b"A       *U ".as_slice(), b"B        ", b"C        "][..],
        &[b"A        ".as_slice(), b"B       *U ", b"C        "][..],
        &[b"A       *V ".as_slice(), b"B        ", b"C        "][..],
    ] {
        assert_eq!(
            nav(&service, run, seq, ImsOperation::GetUnique, PREFIX)
                .unwrap()
                .segments[0]
                .data,
            b"B1114x"
        );
        seq += 1;
        assert_eq!(
            nav(&service, run, seq, ImsOperation::GetHoldNext, pattern)
                .unwrap()
                .segments[0]
                .data,
            b"C111a"
        );
        seq += 1;
    }
    assert_eq!(
        super::fences::legacy(
            &service,
            run,
            request(run, ImsOperation::Replace, seq, &[], b"C111z")
        )
        .status,
        "  "
    );
    seq += 1;
    assert_eq!(
        cursor(&service, run, 1)["held"],
        serde_json::json!({"id":3,"version":2})
    );
    assert_eq!(
        super::fences::legacy(
            &service,
            run,
            request(run, ImsOperation::Delete, seq, &[], b"")
        )
        .affected_segments,
        1
    );
    seq += 1;
    assert_eq!(
        cursor(&service, run, 1)["primary_search"]["boundary"]["provenance"],
        "deleted"
    );
    assert_eq!(
        nav(&service, run, seq, ImsOperation::GetHoldNext, UU)
            .unwrap()
            .segments[0]
            .data,
        b"C112b"
    );
    seq += 1;
    assert_eq!(
        super::fences::legacy(
            &service,
            run,
            request(run, ImsOperation::Rollback, seq, &[], b"")
        )
        .status,
        "  "
    );
    assert!(cursor(&service, run, 1).get("primary_search").is_none());
    drop(service);
    let reopened = Arc::new(
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    );
    checkpoint(reopened, None);
    eprintln!("PUBLIC-PRIMARY-POSTGRES-PASS five-patterns/feedback/hold/DLET/backout/CHKP/XRST");
}

#[test]
fn ssa_primary_producer_sqlite_separate_process_trace_hold_and_canonical_replay() {
    const ENV: &str = "IMS_PRIMARY_PRODUCER_PROCESS_URL";
    let name = std::thread::current().name().unwrap().to_owned();
    if let Ok(url) = std::env::var(ENV) {
        let mode = std::env::var("IMS_PRIMARY_PRODUCER_PROCESS_MODE").unwrap();
        let phase = std::env::var("IMS_PRIMARY_PRODUCER_PROCESS_PHASE").unwrap();
        let run = "primary-cold";
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        if phase == "write" {
            let service = seed(store, run);
            if mode == "deleted" {
                assert_eq!(
                    nav(
                        &service,
                        run,
                        5,
                        ImsOperation::GetHoldUnique,
                        &[
                            b"A       (AKEY    EQA1)",
                            b"B       (BKEY    EQB11)",
                            b"C       (CKEY    EQC112)"
                        ]
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C112b"
                );
                assert_eq!(
                    super::fences::legacy(
                        &service,
                        run,
                        request(run, ImsOperation::Delete, 6, &[], b"")
                    )
                    .affected_segments,
                    1
                );
                assert_eq!(
                    cursor(&service, run, 1)["primary_search"]["boundary"]["provenance"],
                    "deleted"
                );
                assert_eq!(
                    cursor(&service, run, 1)["primary_search"]["boundary"]["anchor_id"],
                    4
                );
            } else if mode == "hold" {
                assert_eq!(
                    nav(&service, run, 5, ImsOperation::GetHoldNext, UU)
                        .unwrap()
                        .segments[0]
                        .data,
                    b"C111a"
                );
                assert_eq!(
                    cursor(&service, run, 1)["held"],
                    serde_json::json!({"id":3,"version":1})
                );
            } else {
                assert_eq!(
                    nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS)
                        .unwrap()
                        .status,
                    "GE"
                );
                assert_eq!(
                    cursor(&service, run, 1)["primary_search"]["boundary"]["anchor_id"],
                    8
                );
                assert_eq!(
                    cursor(&service, run, 1)["primary_search"]["feedback_path"],
                    serde_json::json!([{"segment":"A","key":[65,49]},{"segment":"B","key":[66,49,49]}])
                );
            }
        } else {
            assert_eq!(phase, "read");
            let service = ImsService::open(store.clone(), Default::default()).unwrap();
            if mode == "deleted" {
                assert_eq!(
                    cursor(&service, run, 1)["primary_search"]["boundary"]["provenance"],
                    "deleted"
                );
                assert_eq!(
                    nav(&service, run, 7, ImsOperation::GetNext, &[])
                        .unwrap()
                        .segments[0]
                        .data,
                    b"D111e"
                );
                let before = rows(&*store);
                assert_eq!(
                    super::fences::legacy(
                        &service,
                        run,
                        request(run, ImsOperation::Delete, 6, &[], b"")
                    )
                    .affected_segments,
                    1
                );
                assert_eq!(rows(&*store), before);
            } else if mode == "hold" {
                assert!(position(&service, run, 1).is_held());
                let request = request(run, ImsOperation::Replace, 6, &["C"], b"C111z");
                let HostResult::Ims(result) =
                    invoke(&service, run, HostRequest::Ims(request)).unwrap()
                else {
                    panic!()
                };
                assert_eq!(result.status, "  ");
                assert_eq!(
                    cursor(&service, run, 1)["held"],
                    serde_json::json!({"id":3,"version":2})
                );
                let before = rows(&*store);
                assert_eq!(
                    nav(&service, run, 5, ImsOperation::GetHoldNext, UU)
                        .unwrap()
                        .segments[0]
                        .data,
                    b"C111a"
                );
                assert_eq!(rows(&*store), before);
                assert_eq!(
                    cursor(&service, run, 1)["held"],
                    serde_json::json!({"id":3,"version":2})
                );
            } else {
                let result = nav(
                    &service,
                    run,
                    6,
                    ImsOperation::GetNext,
                    if mode == "ordinary" { &[] } else { UU },
                )
                .unwrap();
                assert_eq!(
                    result.segments[0].data,
                    if mode == "ordinary" {
                        b"E11f".as_slice()
                    } else {
                        b"C111a".as_slice()
                    }
                );
                let before = rows(&*store);
                let result = nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS).unwrap();
                assert_eq!(result.status, "GE");
                assert!(result.segments.is_empty());
                assert_eq!(rows(&*store), before);
            }
        }
        eprintln!("PRIMARY-COLD-PASS {mode}/{phase}");
        return;
    }
    for mode in ["ordinary", "constrained", "hold", "deleted"] {
        let path = std::env::temp_dir().join(format!(
            "ims-primary-cold-{}-{mode}.sqlite",
            std::process::id()
        ));
        let _cleanup = SqliteFile(Some(path.clone()));
        let url = format!("sqlite:{}?mode=rwc", path.display());
        for phase in ["write", "read"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(ENV, &url)
                .env("IMS_PRIMARY_PRODUCER_PROCESS_MODE", mode)
                .env("IMS_PRIMARY_PRODUCER_PROCESS_PHASE", phase)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("{stdout}{stderr}");
            assert!(output.status.success());
            assert!(stdout.contains("1 passed"));
            assert!(stderr.contains(&format!("PRIMARY-COLD-PASS {mode}/{phase}")));
        }
    }
}
