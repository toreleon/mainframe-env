//! A secondary cursor must not become a primary-order checkpoint position.
use super::*;
use mainframe_env_host_api::{ImsNavigationRequest, ImsQualifier};

#[path = "secondary_composition_tests.rs"]
mod composition_tests;

fn indexed_catalog() -> ImsMetadataCatalog {
    let mut c = catalog();
    c.databases[0].segments.push(ImsSegmentMetadata {
        name: "CHILD".into(),
        parent: Some("ROOT".into()),
        min_length: 4,
        max_length: 4,
        fields: vec![
            ImsFieldMetadata {
                name: Some("CKEY".into()),
                offset: 0,
                length: 2,
                sequence: true,
                unique: true,
            },
            ImsFieldMetadata {
                name: Some("X".into()),
                offset: 2,
                length: 1,
                sequence: false,
                unique: false,
            },
            ImsFieldMetadata {
                name: Some("Y".into()),
                offset: 3,
                length: 1,
                sequence: false,
                unique: false,
            },
            ImsFieldMetadata {
                name: Some("BYCOMP".into()),
                offset: 2,
                length: 1,
                sequence: false,
                unique: false,
            },
        ],
    });
    c.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "BYCOMP".into(),
            source_segment: "CHILD".into(),
            target_segment: "ROOT".into(),
            source_fields: vec!["X".into(), "Y".into()],
        });
    let ImsPcbMetadata::Database(pcb) = &mut c.psbs[0].pcbs[0] else {
        unreachable!()
    };
    pcb.sensitive_segments.push(ImsSensitiveSegmentMetadata {
        name: "CHILD".into(),
        parent: Some("ROOT".into()),
        processing_options: None,
    });
    let mut indexed = pcb.clone();
    indexed.name = "INDEXA".into();
    indexed.secondary_index = Some("BYCOMP".into());
    let mut other = indexed.clone();
    other.name = "INDEXB".into();
    c.psbs[0].pcbs.extend([
        ImsPcbMetadata::Database(indexed),
        ImsPcbMetadata::Database(other),
    ]);
    c
}

fn child(service: &ImsService, inv: &Invocation, seq: u64, root: &[u8], data: &[u8]) {
    let mut r = database_request(ImsOperation::Insert, seq, data);
    r.segments = vec!["CHILD".into()];
    r.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "KEY".into(),
        value: root.into(),
    }];
    assert_eq!(service.execute(inv, &r).unwrap().status, "  ");
}

fn setup(service: &Arc<ImsService>, store: &Arc<dyn TestStore>, inv: &Invocation) {
    seed(service, inv);
    child(service, inv, 14, b"01", b"A1\xff\0");
    child(service, inv, 15, b"02", b"B1\0\xff");
    child(service, inv, 16, b"02", b"B2\0\xff");
    child(service, inv, 17, b"03", b"C1Z0");
    service
        .execute(inv, &database_request(ImsOperation::Commit, 18, b""))
        .unwrap();
    invoke_call(
        service,
        store,
        inv,
        1,
        ImsRecoveryCall::Restart {
            selection: ImsRestartSelection::Normal,
            area_lengths: vec![],
        },
    );
}

fn nav(
    service: &Arc<ImsService>,
    inv: &Invocation,
    pcb: u16,
    op: ImsOperation,
    ssas: &[&[u8]],
) -> mainframe_env_host_api::ImsResult {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
    let mut request = database_request(op, NEXT.fetch_add(1, Ordering::Relaxed), b"");
    if let Some(m) = &mut request.mutation {
        m.idempotency_key = IdempotencyKey::new(
            format!("secondary-nav-{}-{}", inv.execution_id.as_str(), m.sequence),
            InvocationLimits::default(),
        )
        .unwrap();
    }
    request.pcb = pcb;
    request.segments.clear();
    service
        .execute_navigation(
            inv,
            &ImsNavigationRequest {
                request,
                context: ImsExecutionContext::DbBatch,
                ssas: ssas.iter().map(|s| s.to_vec()).collect(),
            },
        )
        .unwrap()
}

fn symbolic() -> ImsRecoveryCall {
    ImsRecoveryCall::SymbolicCheckpoint {
        id: "SECPOS".into(),
        user_areas: vec![b"SAVE".to_vec()],
    }
}

fn restart() -> ImsRecoveryCall {
    ImsRecoveryCall::Restart {
        selection: ImsRestartSelection::Checkpoint("SECPOS".into()),
        area_lengths: vec![4],
    }
}

#[test]
fn selected_secondary_binary_composite_collision_independent_pointers_hold_parentage_reopen() {
    backends("secondary-composite", |store| {
        let service = open_catalog(store.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        assert_eq!(
            nav(
                &service,
                &first,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)"]
            )
            .segments[0]
                .data,
            b"02B"
        );
        assert_eq!(
            nav(
                &service,
                &first,
                3,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\xff\0)"]
            )
            .segments[0]
                .data,
            b"01A"
        );
        let checkpoint = invoke_call(&service, &store, &first, 2, symbolic());
        let rows = recovery_rows(&*store);
        let json: serde_json::Value = serde_json::from_slice(&rows[0].payload).unwrap();
        assert!(json.to_string().contains("secondary"));
        drop(service);
        let service =
            ImsService::open_authorized(store.clone(), Default::default(), Arc::new(Allow))
                .unwrap();
        let next = next_execution(&first, "composite-reopen");
        assert!(
            matches!(invoke_call(&service,&store,&next,3,restart()), ImsRecoveryResult::Restarted { user_areas, pcb_statuses,.. } if user_areas == vec![b"SAVE".to_vec()] && pcb_statuses == vec![(1,"  ".into()),(2,"  ".into()),(3,"  ".into())])
        );
        let mut replace = database_request(ImsOperation::Replace, 20, b"02B");
        replace.pcb = 2;
        assert_eq!(service.execute(&next, &replace).unwrap().status, "DJ");
        assert_eq!(
            nav(
                &service,
                &next,
                2,
                ImsOperation::GetNextParent,
                &[b"CHILD    "]
            )
            .segments[0]
                .data,
            b"B1\0\xff"
        );
        assert_eq!(
            nav(&service, &next, 3, ImsOperation::GetNext, &[b"CHILD    "]).segments[0].data,
            b"A1\xff\0"
        );
        assert_eq!(
            nav(
                &service,
                &next,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)", b"CHILD   (CKEY    EQB1)"]
            )
            .segments[0]
                .data,
            b"B1\0\xff"
        );
        replace = database_request(ImsOperation::Replace, 21, b"B1Z1");
        replace.pcb = 2;
        assert_eq!(service.execute(&next, &replace).unwrap().status, "  ");
        assert_eq!(
            nav(
                &service,
                &next,
                2,
                ImsOperation::GetNextParent,
                &[b"CHILD    "]
            )
            .status,
            "GP"
        );
        service
            .execute(&next, &database_request(ImsOperation::Commit, 22, b""))
            .unwrap();
        let r = call(2, symbolic());
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &first, &r).unwrap(),
            checkpoint
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn selected_secondary_deleted_moved_back_and_reinserted_sources_never_revive_checkpoint() {
    for disposition in ["deleted", "moved", "reinserted", "reloaded"] {
        backends(&format!("secondary-{disposition}"), |store| {
            let service = open_catalog(store.clone(), indexed_catalog());
            let first = invocation();
            setup(&service, &store, &first);
            nav(
                &service,
                &first,
                2,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)"],
            );
            invoke_call(&service, &store, &first, 2, symbolic());
            let mut writer = next_execution(&first, "secondary-edit");
            writer.service_class = ServiceClass::Interactive;
            nav(
                &service,
                &writer,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (KEY     EQ02)", b"CHILD   (CKEY    EQB1)"],
            );
            let mut update = database_request(
                if disposition == "moved" {
                    ImsOperation::Replace
                } else {
                    ImsOperation::Delete
                },
                20,
                if disposition == "moved" { b"B1Z1" } else { b"" },
            );
            update.segments.clear();
            assert_eq!(service.execute(&writer, &update).unwrap().status, "  ");
            if disposition == "moved" {
                update.data = b"B1\0\xff".to_vec();
                update.mutation = database_request(ImsOperation::Replace, 21, b"").mutation;
                service.execute(&writer, &update).unwrap();
            }
            if disposition == "reinserted" {
                child(&service, &writer, 21, b"02", b"B1\0\xff");
            }
            if disposition == "reloaded" {
                let image = ImsGenericLoadImage {
                    database: "LOGDB".into(),
                    records: vec![
                        ImsGenericLoadRecord {
                            segment: "ROOT".into(),
                            parent: None,
                            data: b"02B".to_vec(),
                        },
                        ImsGenericLoadRecord {
                            segment: "CHILD".into(),
                            parent: Some(0),
                            data: b"B1\0\xff".to_vec(),
                        },
                        ImsGenericLoadRecord {
                            segment: "CHILD".into(),
                            parent: Some(0),
                            data: b"B2\0\xff".to_vec(),
                        },
                    ],
                };
                let mut load =
                    database_request(ImsOperation::Load, 21, &serde_json::to_vec(&image).unwrap());
                load.segments.clear();
                service.execute(&writer, &load).unwrap();
            }
            service
                .execute(&writer, &database_request(ImsOperation::Commit, 22, b""))
                .unwrap();
            let next = next_execution(&first, "secondary-stale");
            assert!(
                matches!(invoke_call(&service,&store,&next,3,restart()), ImsRecoveryResult::Restarted { pcb_statuses,.. } if pcb_statuses.iter().any(|p| p == &(2,"GE".into())))
            );
            assert_eq!(
                nav(&service, &next, 2, ImsOperation::GetNext, &[b"ROOT     "]).segments[0].data,
                b"02B"
            );
        });
    }
}

#[test]
fn selected_secondary_unchanged_pointer_data_replace_preserves_restart_identity() {
    backends("secondary-data-update", |store| {
        let service = open_catalog(store.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        invoke_call(&service, &store, &first, 2, symbolic());
        nav(
            &service,
            &first,
            1,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (KEY     EQ02)"],
        );
        let update = database_request(ImsOperation::Replace, 30, b"02X");
        service.execute(&first, &update).unwrap();
        service
            .execute(&first, &database_request(ImsOperation::Commit, 31, b""))
            .unwrap();
        let next = next_execution(&first, "secondary-data-restart");
        assert!(
            matches!(invoke_call(&service,&store,&next,3,restart()),ImsRecoveryResult::Restarted { pcb_statuses,.. } if pcb_statuses.iter().any(|p|p==&(2,"  ".into())))
        );
        assert_eq!(
            nav(
                &service,
                &next,
                2,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)"]
            )
            .segments[0]
                .data,
            b"02X"
        );
    });
}

#[test]
fn selected_secondary_same_target_distinct_pointer_occurrences_resume_independently() {
    backends("secondary-source-tie", |store| {
        let service = open_catalog(store.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        assert_eq!(
            nav(
                &service,
                &first,
                2,
                ImsOperation::GetUnique,
                &[b"ROOT    (BYCOMP  EQ\0\xff)"]
            )
            .segments[0]
                .data,
            b"02B"
        );
        nav(
            &service,
            &first,
            3,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        assert_eq!(
            nav(&service, &first, 3, ImsOperation::GetNext, &[b"ROOT     "]).segments[0].data,
            b"02B"
        );
        invoke_call(&service, &store, &first, 2, symbolic());
        let rows = recovery_rows(&*store);
        let json: serde_json::Value = serde_json::from_slice(&rows[0].payload).unwrap();
        let positions = json["checkpoints"]["SECPOS"]["request"]["positions"]
            .as_array()
            .unwrap();
        let a = &positions.iter().find(|p| p["pcb"] == "2").unwrap()["secondary"];
        let b = &positions.iter().find(|p| p["pcb"] == "3").unwrap()["secondary"];
        assert_eq!(a["target"], b["target"]);
        assert_eq!(a["current"], b["current"]);
        assert_ne!(a["source"], b["source"]);
        let next = next_execution(&first, "secondary-tie-restart");
        invoke_call(&service, &store, &next, 3, restart());
        assert_eq!(
            nav(&service, &next, 2, ImsOperation::GetNext, &[b"ROOT     "]).segments[0].data,
            b"02B"
        );
        assert_eq!(
            nav(&service, &next, 3, ImsOperation::GetNext, &[b"ROOT     "]).segments[0].data,
            b"03C"
        );
    });
}

#[test]
fn selected_secondary_missing_database_image_rejects_xrst_without_publishing_position() {
    backends("secondary-missing-image", |store| {
        let service = open_catalog(store.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        invoke_call(&service, &store, &first, 2, symbolic());
        let row = store
            .get_provider_state("ims-v1-generic-database", "LOGDB")
            .unwrap()
            .unwrap();
        store
            .delete_provider_state("ims-v1-generic-database", "LOGDB", row.version)
            .unwrap();
        let next = next_execution(&first, "secondary-missing-image");
        let r = call(3, restart());
        intent(&*store, &next, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            service.observe_application_recovery(&next, &r).unwrap(),
            None
        );
    });
}

#[test]
fn selected_secondary_nonunique_physical_source_path_is_unsupported_without_mutation() {
    backends("secondary-nonunique-path", |store| {
        let mut c = indexed_catalog();
        c.databases[0].segments[1].fields[0].unique = false;
        let service = open_catalog(store.clone(), c);
        let first = invocation();
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        let r = call(2, symbolic());
        intent(&*store, &first, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &first, &r),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn selected_secondary_checkpoint_real_database_cas_capacity_lost_ack_and_readonly_resolution() {
    backends("secondary-atomic", |store| {
        let faults = Arc::new(FaultRows {
            inner: store.clone(),
            mode: AtomicU8::new(0),
        });
        let service = open_catalog(faults.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        for (mode, seq, expected) in [
            (1, 2, HostProblem::ResourceExhausted),
            (4, 3, HostProblem::IdempotencyConflict),
        ] {
            let r = call(seq, symbolic());
            intent(&*store, &first, &r);
            let before = snapshot(&*store);
            faults.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &first, &r),
                Err(expected)
            );
            let after = snapshot(&*store);
            assert_eq!(&before[1..], &after[1..]);
            assert_eq!(
                service.observe_application_recovery(&first, &r).unwrap(),
                None
            );
        }
        let r = call(4, symbolic());
        intent(&*store, &first, &r);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &r),
            Err(HostProblem::UnknownOutcome)
        );
        let before = snapshot(&*store);
        assert!(matches!(
            service.observe_application_recovery(&first, &r).unwrap(),
            Some(ImsRecoveryResult::Checkpointed { .. })
        ));
        assert_eq!(snapshot(&*store), before);
        let absent = call(
            5,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "MISSING".into(),
                user_areas: vec![],
            },
        );
        intent(&*store, &first, &absent);
        assert_eq!(
            service
                .observe_application_recovery(&first, &absent)
                .unwrap(),
            None
        );
        let next = next_execution(&first, "secondary-atomic-restart");
        let r = call(6, restart());
        intent(&*store, &next, &r);
        let before = snapshot(&*store);
        faults.mode.store(4, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(&snapshot(&*store)[1..], &before[1..]);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Err(HostProblem::UnknownOutcome)
        );
        let before = snapshot(&*store);
        assert!(
            matches!(service.observe_application_recovery(&next,&r).unwrap(),Some(ImsRecoveryResult::Restarted { pcb_statuses,.. }) if pcb_statuses.iter().any(|p|p==&(2,"  ".into())))
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn selected_secondary_mixed_primary_gsam_and_later_secondary_pcbs_atomic_restart() {
    backends("secondary-mixed", |store| {
        let mut c = indexed_catalog();
        let mut file = catalog().databases.remove(0);
        file.name = "FILEDB".into();
        file.organization = ImsDatabaseOrganization::Gsam;
        file.segments[0].fields.clear();
        c.databases.push(file);
        let ImsPcbMetadata::Database(mut output) = catalog().psbs.remove(0).pcbs.remove(0) else {
            unreachable!()
        };
        output.name = "OUTPUT".into();
        output.database = "FILEDB".into();
        output.processing_options = "L".into();
        let mut input = output.clone();
        input.name = "INPUT".into();
        input.processing_options = "G".into();
        let ImsPcbMetadata::Database(mut later) = c.psbs[0].pcbs[1].clone() else {
            unreachable!()
        };
        later.name = "LATER".into();
        c.psbs[0].pcbs.extend([
            ImsPcbMetadata::Database(output),
            ImsPcbMetadata::Database(input),
            ImsPcbMetadata::Database(later),
        ]);
        let service = open_catalog(store.clone(), c);
        let first = invocation();
        setup(&service, &store, &first);
        let gsam = |inv: &Invocation, seq, pcb, op, data: &[u8]| {
            let mut request = database_request(op, seq, data);
            request.segments.clear();
            request.pcb = pcb;
            service
                .execute_gsam(
                    inv,
                    &mainframe_env_host_api::ImsGsamRequest {
                        undefined_length: None,
                        request,
                        context: ImsExecutionContext::DbBatch,
                        save_address: true,
                        search: None,
                    },
                )
                .unwrap()
        };
        gsam(&first, 30, 4, ImsOperation::Insert, b"01A");
        gsam(&first, 31, 4, ImsOperation::Insert, b"02B");
        gsam(&first, 32, 5, ImsOperation::GetNext, b"");
        nav(
            &service,
            &first,
            6,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        invoke_call(&service, &store, &first, 2, symbolic());
        gsam(&first, 33, 4, ImsOperation::Insert, b"03C");
        let next = next_execution(&first, "mixed-secondary-restart");
        assert!(
            matches!(invoke_call(&service,&store,&next,3,restart()),ImsRecoveryResult::Restarted { pcb_statuses,.. } if pcb_statuses.iter().any(|s|s==&(6,"  ".into())))
        );
        assert_eq!(
            gsam(&next, 34, 5, ImsOperation::GetNext, b"")
                .result
                .segments[0]
                .data,
            b"02B"
        );
        assert_eq!(
            gsam(&next, 35, 5, ImsOperation::GetNext, b"").result.status,
            "GB"
        );
        assert_eq!(
            nav(&service, &next, 6, ImsOperation::GetNext, &[b"CHILD    "]).segments[0].data,
            b"B1\0\xff"
        );
    });
}

#[test]
fn selected_secondary_context_auth_capacity_and_corrupt_images_never_mutate() {
    backends("secondary-negative", |store| {
        let service = open_catalog(store.clone(), indexed_catalog());
        let first = invocation();
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        for (seq, context, call_value, expected) in [
            (
                2,
                ImsExecutionContext::DbBatch,
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "BAD ID".into(),
                    user_areas: vec![],
                },
                HostProblem::Malformed,
            ),
            (
                3,
                ImsExecutionContext::DbDc,
                symbolic(),
                HostProblem::Unsupported,
            ),
            (
                4,
                ImsExecutionContext::DbBatch,
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "TOOMANY".into(),
                    user_areas: vec![vec![1]; 8],
                },
                HostProblem::ResourceExhausted,
            ),
        ] {
            let mut r = call(seq, call_value);
            r.context = context;
            let before = snapshot(&*store);
            // Invalid host operands are rejected before canonical intent lookup.
            assert_eq!(
                dispatch(service.clone(), store.clone(), &first, &r),
                Err(expected)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let denied =
            ImsService::open_authorized(store.clone(), Default::default(), Arc::new(DenyDatabase))
                .unwrap();
        let r = call(5, symbolic());
        intent(&*store, &first, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(denied, store.clone(), &first, &r),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&*store), before);
        invoke_call(&service, &store, &first, 6, symbolic());
        let mut row = store
            .get_provider_state("ims-v1-generic-database", "LOGDB")
            .unwrap()
            .unwrap();
        let old = row.version;
        row.version += 1;
        row.payload = b"malformed".to_vec();
        store.put_provider_state(row, Some(old)).unwrap();
        let next = next_execution(&first, "secondary-corrupt");
        let r = call(7, restart());
        intent(&*store, &next, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(snapshot(&*store), before);
        assert!(
            service
                .observe_application_recovery(&next, &r)
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn selected_secondary_sqlite_three_process_checkpoint_restart_continuation() {
    let path = std::env::temp_dir().join(format!(
        "ims-secondary-restart-process-{}.sqlite",
        std::process::id()
    ));
    for phase in ["checkpoint", "restart", "continue"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "checkpoint_tests::secondary_tests::selected_secondary_process_worker",
                "--nocapture",
            ])
            .env("IMS_SECONDARY_RESTART_PATH", &path)
            .env("IMS_SECONDARY_RESTART_PHASE", phase)
            .output()
            .unwrap();
        println!(
            "independent SQLite phase {phase}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "phase {phase}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selected_secondary_process_worker() {
    let Ok(path) = std::env::var("IMS_SECONDARY_RESTART_PATH") else {
        return;
    };
    let phase = std::env::var("IMS_SECONDARY_RESTART_PHASE").unwrap();
    let store: Arc<dyn TestStore> = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let service = open_catalog(store.clone(), indexed_catalog());
    let first = invocation();
    if phase == "checkpoint" {
        setup(&service, &store, &first);
        nav(
            &service,
            &first,
            2,
            ImsOperation::GetUnique,
            &[b"ROOT    (BYCOMP  EQ\0\xff)"],
        );
        invoke_call(&service, &store, &first, 2, symbolic());
    } else {
        let next = next_execution(&first, "secondary-process-restart");
        let r = call(3, restart());
        if phase == "restart" {
            intent(&*store, &next, &r);
        }
        let before = snapshot(&*store);
        assert!(
            matches!(dispatch(service.clone(),store.clone(),&next,&r).unwrap(),ImsRecoveryResult::Restarted { pcb_statuses,.. } if pcb_statuses.iter().any(|p|p==&(2,"  ".into())))
        );
        if phase == "continue" {
            assert_eq!(snapshot(&*store), before);
            assert_eq!(
                nav(&service, &next, 2, ImsOperation::GetNext, &[b"CHILD    "]).segments[0].data,
                b"B1\0\xff"
            );
            assert_eq!(
                nav(&service, &next, 2, ImsOperation::GetNext, &[b"CHILD    "]).segments[0].data,
                b"B2\0\xff"
            );
        } else {
            assert_eq!(phase, "restart");
        }
    }
}

#[test]
fn selected_secondary_checkpoint_restart_reestablishes_actual_gn_position() {
    backends("checkpoint-secondary", |store| {
        let mut metadata = catalog();
        metadata.databases[0].secondary_indexes = vec![ImsSecondaryIndexMetadata {
            name: "BYKEY".into(),
            source_segment: "ROOT".into(),
            target_segment: "ROOT".into(),
            source_fields: vec!["KEY".into()],
        }];
        let ImsPcbMetadata::Database(mut indexed) = metadata.psbs[0].pcbs[0].clone() else {
            panic!()
        };
        indexed.name = "INDEXPCB".into();
        indexed.secondary_index = Some("BYKEY".into());
        metadata.psbs[0]
            .pcbs
            .push(ImsPcbMetadata::Database(indexed));
        let service = open_catalog(store.clone(), metadata);
        let invocation = invocation();
        seed(&service, &invocation);
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Commit, 20, &[]),
            )
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        let mut read = database_request(ImsOperation::GetNext, 21, &[]);
        read.pcb = 2;
        assert_eq!(
            service.execute(&invocation, &read).unwrap().segments[0].data,
            b"01A"
        );
        let request = call(
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "INDEXPOS".into(),
                user_areas: vec![],
            },
        );
        intent(&*store, &invocation, &request);
        assert!(matches!(
            dispatch(service.clone(), store.clone(), &invocation, &request).unwrap(),
            ImsRecoveryResult::Checkpointed { .. }
        ));
        let restarted = next_execution(&invocation, "secondary-restart");
        assert!(matches!(invoke_call(&service, &store, &restarted, 3,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("INDEXPOS".into()),
                area_lengths: vec![],
            }), ImsRecoveryResult::Restarted { pcb_statuses, .. } if pcb_statuses == vec![(1, "  ".into()), (2, "  ".into())]));
        read.mutation.as_mut().unwrap().sequence = 22;
        read.mutation.as_mut().unwrap().idempotency_key =
            IdempotencyKey::new("secondary-next", InvocationLimits::default()).unwrap();
        assert_eq!(
            service.execute(&restarted, &read).unwrap().segments[0].data,
            b"02B"
        );
    });
}
