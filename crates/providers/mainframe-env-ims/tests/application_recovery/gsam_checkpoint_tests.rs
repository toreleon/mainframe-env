//! Independent logical file expectations from IMS 15.6 GSAM/CHKP/XRST pins.
use super::*;
use checkpoint_tests::{call, invoke_call, next_execution, snapshot};
use mainframe_env_host_api::{
    ImsGsamAddress, ImsGsamRequest, ImsGsamResult, ImsGsamSearchArgument, ImsOperation,
    ImsRestartSelection,
};

fn metadata() -> ImsMetadataCatalog {
    let mut c = catalog();
    c.databases[0].organization = ImsDatabaseOrganization::Gsam;
    c.databases[0].segments[0].fields.clear();
    let ImsPcbMetadata::Database(input) = &mut c.psbs[0].pcbs[0] else {
        unreachable!()
    };
    input.processing_options = "G".into();
    let mut output = input.clone();
    output.name = "OUTPUT".into();
    output.processing_options = "L".into();
    c.psbs[0].pcbs.push(ImsPcbMetadata::Database(output));
    c
}

fn installed(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service =
        ImsService::open_authorized(store, ImsLimits::default(), Arc::new(Allow)).unwrap();
    service.install_metadata(metadata()).unwrap();
    service
        .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&metadata()))
        .unwrap();
    service
}

fn db(sequence: u64, operation: ImsOperation, pcb: u16, data: &[u8]) -> ImsGsamRequest {
    let mut request = database_request(operation, sequence, data);
    request.segments.clear();
    request.pcb = pcb;
    ImsGsamRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        save_address: operation != ImsOperation::GetUnique,
        search: None,
    }
}

fn gsam(
    service: &Arc<ImsService>,
    invocation: &Invocation,
    request: ImsGsamRequest,
) -> Result<ImsGsamResult, HostProblem> {
    // Both saved GN and ISRT are canonical mutating host operations.
    let host = HostRequest::ImsGsam(request.clone());
    let result = ims_providers(service.clone(), InvocationLimits::default())
        .remove(1)
        .invoke(
            invocation,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: request.request.mutation.as_ref().unwrap().sequence,
                idempotency_key: request
                    .request
                    .mutation
                    .as_ref()
                    .map(|m| m.idempotency_key.clone()),
                deadline_tick: invocation.deadline_tick,
                request: host,
            },
        )
        .outcome?;
    let HostResult::ImsGsam(result) = result else {
        panic!("GSAM result family")
    };
    Ok(result)
}

fn schedule(service: &ImsService, invocation: &Invocation) {
    assert_eq!(
        service
            .execute(
                invocation,
                &database_request(ImsOperation::Schedule, 900, b"")
            )
            .unwrap()
            .status,
        "  "
    );
}

fn normal(service: &Arc<ImsService>, store: &Arc<dyn TestStore>, invocation: &Invocation) {
    invoke_call(
        service,
        store,
        invocation,
        1,
        ImsRecoveryCall::Restart {
            selection: ImsRestartSelection::Normal,
            area_lengths: vec![],
        },
    );
}

fn symbolic(id: &str) -> ImsRecoveryCall {
    ImsRecoveryCall::SymbolicCheckpoint {
        id: id.into(),
        user_areas: vec![b"SAVE".to_vec()],
    }
}

fn restart(id: &str) -> ImsRecoveryCall {
    ImsRecoveryCall::Restart {
        selection: ImsRestartSelection::Checkpoint(id.into()),
        area_lengths: vec![4],
    }
}

fn gu(
    service: &Arc<ImsService>,
    invocation: &Invocation,
    sequence: u64,
    address: ImsGsamAddress,
) -> ImsGsamResult {
    let mut request = db(sequence, ImsOperation::GetUnique, 1, b"");
    request.search = Some(ImsGsamSearchArgument::Record(address));
    gsam(service, invocation, request).unwrap()
}

#[test]
fn gsam_checkpoint_actual_addresses_commit_and_restart_file_position() {
    backends("gsam-roundtrip", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        let a = gsam(&service, &first, db(2, ImsOperation::Insert, 2, b"01A"))
            .unwrap()
            .address
            .unwrap();
        let b = gsam(&service, &first, db(3, ImsOperation::Insert, 2, b"02B"))
            .unwrap()
            .address
            .unwrap();
        let read = gsam(&service, &first, db(4, ImsOperation::GetNext, 1, b"")).unwrap();
        assert_eq!(read.address, Some(a.clone()));
        assert_eq!(read.result.segments[0].data, b"01A");
        invoke_call(&service, &store, &first, 5, symbolic("GSAM1"));
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        let later = gsam(&service, &first, db(6, ImsOperation::Insert, 2, b"03C"))
            .unwrap()
            .address
            .unwrap();
        assert!(
            !store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        drop(service);
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let next = next_execution(&first, "gsam-restarted");
        let result = invoke_call(&service, &store, &next, 7, restart("GSAM1"));
        assert!(
            matches!(result, ImsRecoveryResult::Restarted { user_areas, pcb_statuses, .. } if user_areas == vec![b"SAVE".to_vec()] && pcb_statuses == vec![(1, "  ".into()), (2, "  ".into())])
        );
        let next_record = gsam(&service, &next, db(8, ImsOperation::GetNext, 1, b"")).unwrap();
        assert_eq!(next_record.address, Some(b));
        assert_eq!(next_record.result.segments[0].data, b"02B");
        assert_eq!(gu(&service, &next, 9, later).result.status, "AJ");
        assert_eq!(gu(&service, &next, 10, a).result.segments[0].data, b"01A");
    });
}

#[test]
fn gsam_checkpoint_start_empty_eof_and_no_save_positions() {
    for (label, records, reads, expected) in [
        ("empty", 0, 0, None),
        ("start", 2, 0, Some(&b"01A"[..])),
        ("eof", 2, 3, Some(&b"01A"[..])),
        ("no-save", 2, 1, Some(&b"02B"[..])),
    ] {
        backends(&format!("gsam-{label}"), |store| {
            let service = installed(store.clone());
            let first = invocation();
            schedule(&service, &first);
            normal(&service, &store, &first);
            for i in 0..records {
                gsam(
                    &service,
                    &first,
                    db(
                        10 + i,
                        ImsOperation::Insert,
                        2,
                        if i == 0 { b"01A" } else { b"02B" },
                    ),
                )
                .unwrap();
            }
            for i in 0..reads {
                let mut read = db(20 + i, ImsOperation::GetNext, 1, b"");
                read.save_address = false;
                let result = gsam(&service, &first, read).unwrap();
                assert_eq!(result.result.status, if i == records { "GB" } else { "  " });
                assert!(result.address.is_none());
            }
            invoke_call(&service, &store, &first, 30, symbolic("POSITION"));
            let row = recovery_rows(&*store).remove(0);
            let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            let positions = &json["checkpoints"]["POSITION"]["request"]["positions"];
            assert!(positions.as_array().unwrap().iter().all(|p| {
                p["segment_key"].as_array().unwrap().is_empty() && p.get("gsam").is_some()
            }));
            let next = next_execution(&first, &format!("gsam-{label}-restart"));
            invoke_call(&service, &store, &next, 31, restart("POSITION"));
            let result = gsam(&service, &next, db(32, ImsOperation::GetNext, 1, b"")).unwrap();
            if let Some(data) = expected {
                assert_eq!(result.result.segments[0].data, data);
            } else {
                assert_eq!(result.result.status, "GB");
            }
        });
    }
}

#[test]
fn gsam_checkpoint_and_xrst_replay_after_later_work_never_apply_again() {
    backends("gsam-replay", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        gsam(&service, &first, db(2, ImsOperation::Insert, 2, b"01A")).unwrap();
        let checkpoint = call(3, symbolic("REPLAY"));
        intent(&*store, &first, &checkpoint);
        let saved = dispatch(service.clone(), store.clone(), &first, &checkpoint).unwrap();
        let later_request = db(4, ImsOperation::Insert, 2, b"02B");
        let later = gsam(&service, &first, later_request.clone()).unwrap();
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &checkpoint),
            Ok(saved)
        );
        assert_eq!(snapshot(&*store), before);
        let next = next_execution(&first, "gsam-replay-restarted");
        let r = call(5, restart("REPLAY"));
        intent(&*store, &next, &r);
        let restored = dispatch(service.clone(), store.clone(), &next, &r).unwrap();
        let added = gsam(&service, &next, db(6, ImsOperation::Insert, 2, b"02B")).unwrap();
        assert_ne!(added.address, later.address);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Ok(restored)
        );
        assert_eq!(gsam(&service, &first, later_request), Ok(later.clone()));
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            gu(&service, &next, 7, later.address.unwrap()).result.status,
            "AJ"
        );
        assert_eq!(
            gu(&service, &next, 8, added.address.unwrap())
                .result
                .segments[0]
                .data,
            b"02B"
        );
    });
}

#[test]
fn gsam_checkpoint_basic_order_selectors_limits_and_authorization_no_mutation() {
    backends("gsam-negative", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        for (sequence, operation, expected) in [
            (
                1,
                ImsRecoveryCall::BasicCheckpoint { id: "BASIC".into() },
                HostProblem::Unsupported,
            ),
            (2, symbolic("EARLY"), HostProblem::Malformed),
            (3, restart("UNKNOWN"), HostProblem::NotFound),
            (
                4,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Timestamp("REGN1231200000".into()),
                    area_lengths: vec![],
                },
                HostProblem::Unsupported,
            ),
            (
                5,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Last,
                    area_lengths: vec![],
                },
                HostProblem::Unsupported,
            ),
        ] {
            let r = call(sequence, operation);
            intent(&*store, &first, &r);
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &first, &r),
                Err(expected)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let mut basic = database_request(ImsOperation::Checkpoint, 6, b"");
        basic.checkpoint_id = Some("BASIC".into());
        let before = snapshot(&*store);
        assert_eq!(
            service.execute(&first, &basic),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
        let mut malformed = call(7, symbolic("BAD-ID"));
        malformed.call = ImsRecoveryCall::SymbolicCheckpoint {
            id: "BAD ID".into(),
            user_areas: vec![],
        };
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &malformed),
            Err(HostProblem::Malformed)
        );
        invoke_call(
            &service,
            &store,
            &next_execution(&first, "gsam-neg-normal"),
            10,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        struct Deny;
        impl EnterpriseAuthorizer for Deny {
            fn authorize(
                &self,
                _: &PrincipalId,
                resource: &EnterpriseResource,
            ) -> Result<(), HostProblem> {
                if resource.name.as_str() == "LOGDB" {
                    Err(HostProblem::Unauthorized)
                } else {
                    Ok(())
                }
            }
        }
        let denied =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Deny))
                .unwrap();
        let r = call(8, symbolic("DENIED"));
        intent(&*store, &first, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(denied, store.clone(), &first, &r),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn gsam_checkpoint_capacity_rejects_without_committing_output() {
    backends("gsam-capacity", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        invoke_call(&service, &store, &first, 2, symbolic("ONE"));
        gsam(&service, &first, db(3, ImsOperation::Insert, 2, b"01A")).unwrap();
        let limited = ImsService::open_authorized(
            store.clone(),
            ImsLimits {
                max_checkpoints: 1,
                ..ImsLimits::default()
            },
            Arc::new(Allow),
        )
        .unwrap();
        let r = call(4, symbolic("TWO"));
        intent(&*store, &first, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(limited, store.clone(), &first, &r),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn gsam_checkpoint_atomic_cas_faults_and_lost_acknowledgements() {
    backends("gsam-fault", |backend| {
        let faults = Arc::new(FaultRows {
            inner: backend.clone(),
            mode: AtomicU8::new(0),
        });
        let store: Arc<dyn TestStore> = backend.clone();
        let service = installed(faults.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        gsam(&service, &first, db(2, ImsOperation::Insert, 2, b"01A")).unwrap();
        let r = call(3, symbolic("FAULT"));
        intent(&*store, &first, &r);
        let before = snapshot(&*store);
        faults.mode.store(1, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &r),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&*store), before);
        faults.mode.store(4, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &r),
            Err(HostProblem::IdempotencyConflict)
        );
        let after = snapshot(&*store);
        assert_eq!(after[1..], before[1..]);
        assert_eq!(after[0][0].payload, before[0][0].payload);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &first, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(
            matches!(service.observe_application_recovery(&first, &r).unwrap(), Some(ImsRecoveryResult::Checkpointed { id, .. }) if id == "FAULT")
        );
        gsam(&service, &first, db(4, ImsOperation::Insert, 2, b"02B")).unwrap();
        let next = next_execution(&first, "gsam-fault-restart");
        let restart = call(5, restart("FAULT"));
        intent(&*store, &next, &restart);
        let before_restart = snapshot(&*store);
        faults.mode.store(4, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &restart),
            Err(HostProblem::IdempotencyConflict)
        );
        let after_restart_race = snapshot(&*store);
        assert_eq!(after_restart_race[1..], before_restart[1..]);
        assert_eq!(
            after_restart_race[0][0].payload,
            before_restart[0][0].payload
        );
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &restart),
            Err(HostProblem::UnknownOutcome)
        );
        let observed = service
            .observe_application_recovery(&next, &restart)
            .unwrap()
            .unwrap();
        let reopened =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(reopened.clone(), store.clone(), &next, &restart),
            Ok(observed)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            gsam(&reopened, &next, db(6, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .segments[0]
                .data,
            b"01A"
        );
        let absent = call(7, symbolic("ABSENT"));
        assert_eq!(
            reopened
                .observe_application_recovery(&next, &absent)
                .unwrap(),
            None
        );
    });
}

#[test]
fn gsam_checkpoint_sqlite_child_process_exit_and_replay() {
    let path = std::env::temp_dir().join(format!(
        "ims-gsam-checkpoint-child-{}.sqlite",
        std::process::id()
    ));
    for phase in ["checkpoint", "restart", "resume"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "gsam_checkpoint_tests::gsam_checkpoint_process_worker",
                "--nocapture",
            ])
            .env("IMS_GSAM_CHECKPOINT_PATH", &path)
            .env("IMS_GSAM_CHECKPOINT_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn gsam_checkpoint_process_worker() {
    let Ok(path) = std::env::var("IMS_GSAM_CHECKPOINT_PATH") else {
        return;
    };
    let store: Arc<dyn TestStore> = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let first = invocation();
    let phase = std::env::var("IMS_GSAM_CHECKPOINT_PHASE").unwrap();
    if phase == "checkpoint" {
        let service = installed(store.clone());
        schedule(&service, &first);
        normal(&service, &store, &first);
        for (seq, data) in [(2, b"01A"), (3, b"02B")] {
            gsam(&service, &first, db(seq, ImsOperation::Insert, 2, data)).unwrap();
        }
        gsam(&service, &first, db(4, ImsOperation::GetNext, 1, b"")).unwrap();
        invoke_call(&service, &store, &first, 5, symbolic("CHILD"));
        gsam(&service, &first, db(6, ImsOperation::Insert, 2, b"03C")).unwrap();
    } else {
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let next = next_execution(&first, "gsam-process-restart");
        let r = call(7, restart("CHILD"));
        if phase == "restart" {
            intent(&*store, &next, &r);
        }
        let expected = dispatch(service.clone(), store.clone(), &next, &r).unwrap();
        assert!(
            matches!(expected, ImsRecoveryResult::Restarted { user_areas, .. } if user_areas == vec![b"SAVE".to_vec()])
        );
        let read = db(8, ImsOperation::GetNext, 1, b"");
        let found = gsam(&service, &next, read).unwrap();
        assert_eq!(found.result.segments[0].data, b"02B");
        assert_eq!(
            gsam(&service, &next, db(9, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .status,
            "GB"
        );
    }
}

#[test]
fn gsam_checkpoint_multiple_databases_and_independent_pcbs_restore_atomically() {
    backends("gsam-multiple", |store| {
        let mut metadata = metadata();
        let mut second = metadata.databases[0].clone();
        second.name = "SECOND".into();
        metadata.databases.push(second);
        let ImsPcbMetadata::Database(reader) = &metadata.psbs[0].pcbs[0] else {
            unreachable!()
        };
        let mut other_reader = reader.clone();
        other_reader.name = "OTHREAD".into();
        other_reader.database = "SECOND".into();
        let mut independent = reader.clone();
        independent.name = "INDEPEND".into();
        let mut other_writer = other_reader.clone();
        other_writer.name = "OTHWRITE".into();
        other_writer.processing_options = "L".into();
        metadata.psbs[0].pcbs.extend([
            ImsPcbMetadata::Database(independent),
            ImsPcbMetadata::Database(other_reader),
            ImsPcbMetadata::Database(other_writer),
        ]);
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        service.install_metadata(metadata.clone()).unwrap();
        service
            .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&metadata))
            .unwrap();
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        for (seq, pcb, data) in [
            (2, 2, b"01A"),
            (3, 2, b"02B"),
            (4, 5, b"11X"),
            (5, 5, b"12Y"),
        ] {
            gsam(&service, &first, db(seq, ImsOperation::Insert, pcb, data)).unwrap();
        }
        for (seq, pcb) in [(6, 1), (7, 3), (8, 3), (9, 4)] {
            gsam(&service, &first, db(seq, ImsOperation::GetNext, pcb, b"")).unwrap();
        }
        invoke_call(&service, &store, &first, 10, symbolic("MULTI"));
        for (seq, pcb, data) in [(11, 2, b"03C"), (12, 5, b"13Z")] {
            gsam(&service, &first, db(seq, ImsOperation::Insert, pcb, data)).unwrap();
        }
        let next = next_execution(&first, "gsam-multi-restart");
        let result = invoke_call(&service, &store, &next, 13, restart("MULTI"));
        assert!(
            matches!(result, ImsRecoveryResult::Restarted { pcb_statuses, .. } if pcb_statuses == (1..=5).map(|pcb| (pcb, "  ".into())).collect::<Vec<_>>())
        );
        for (seq, pcb, data) in [(14, 1, b"02B"), (15, 4, b"12Y")] {
            assert_eq!(
                gsam(&service, &next, db(seq, ImsOperation::GetNext, pcb, b""))
                    .unwrap()
                    .result
                    .segments[0]
                    .data,
                data
            );
        }
        assert_eq!(
            gsam(&service, &next, db(16, ImsOperation::GetNext, 3, b""))
                .unwrap()
                .result
                .status,
            "GB"
        );
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn gsam_checkpoint_empty_output_and_stale_replaced_file_never_restore_old_addresses() {
    backends("gsam-empty-output", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        invoke_call(&service, &store, &first, 2, symbolic("EMPTY"));
        let removed = gsam(&service, &first, db(3, ImsOperation::Insert, 2, b"01A"))
            .unwrap()
            .address
            .unwrap();
        let next = next_execution(&first, "gsam-empty-restart");
        invoke_call(&service, &store, &next, 4, restart("EMPTY"));
        assert_eq!(
            gsam(&service, &next, db(5, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .status,
            "GB"
        );
        assert_eq!(gu(&service, &next, 6, removed).result.status, "AJ");
        let inserted = gsam(&service, &next, db(7, ImsOperation::Insert, 2, b"01A"))
            .unwrap()
            .address
            .unwrap();
        gu(&service, &next, 8, inserted.clone());
        invoke_call(&service, &store, &next, 9, symbolic("LIVE"));
        let mut load = database_request(ImsOperation::Load, 10, b"");
        load.pcb = 2;
        load.data = serde_json::to_vec(&ImsGenericLoadImage {
            database: "LOGDB".into(),
            records: vec![],
        })
        .unwrap();
        service.execute(&next, &load).unwrap();
        let reopened =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let later = next_execution(&next, "gsam-stale-restart");
        let r = call(11, restart("LIVE"));
        intent(&*store, &later, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(reopened.clone(), store.clone(), &later, &r),
            Err(HostProblem::ProviderFailure)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(gu(&reopened, &later, 12, inserted).result.status, "AJ");
    });
}

#[test]
fn gsam_checkpoint_input_only_address_materialization_can_restart_after_settlement() {
    backends("gsam-input-only", |store| {
        let mut input_only = metadata();
        input_only.psbs[0].pcbs.truncate(1);
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        service.install_metadata(input_only.clone()).unwrap();
        service
            .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&input_only))
            .unwrap();
        let first = invocation();
        schedule(&service, &first);
        let image = ImsGenericLoadImage {
            database: "LOGDB".into(),
            records: [b"01A", b"02B"]
                .iter()
                .map(|data| ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: data.to_vec(),
                })
                .collect(),
        };
        let mut load = database_request(ImsOperation::Load, 10, b"");
        load.pcb = 1;
        load.data = serde_json::to_vec(&image).unwrap();
        service.execute(&first, &load).unwrap();
        service
            .execute(&first, &database_request(ImsOperation::Commit, 11, b""))
            .unwrap();
        normal(&service, &store, &first);
        let mut read = db(2, ImsOperation::GetNext, 1, b"");
        read.save_address = false;
        gsam(&service, &first, read).unwrap();
        invoke_call(&service, &store, &first, 3, symbolic("INPUT"));
        gsam(&service, &first, db(4, ImsOperation::GetNext, 1, b"")).unwrap();
        let next = next_execution(&first, "gsam-input-restart");
        invoke_call(&service, &store, &next, 5, restart("INPUT"));
        assert_eq!(
            gsam(&service, &next, db(6, ImsOperation::GetNext, 1, b""))
                .unwrap()
                .result
                .segments[0]
                .data,
            b"02B"
        );
    });
}

#[test]
fn gsam_checkpoint_committed_later_suffix_requires_reconciliation_without_erasing_it() {
    backends("gsam-settled-suffix", |store| {
        let service = installed(store.clone());
        let first = invocation();
        schedule(&service, &first);
        normal(&service, &store, &first);
        gsam(&service, &first, db(2, ImsOperation::Insert, 2, b"01A")).unwrap();
        invoke_call(&service, &store, &first, 3, symbolic("SETTLED"));
        let later_address = gsam(&service, &first, db(4, ImsOperation::Insert, 2, b"02B"))
            .unwrap()
            .address
            .unwrap();
        service
            .execute(&first, &database_request(ImsOperation::Commit, 5, b""))
            .unwrap();
        let next = next_execution(&first, "gsam-settled-restart");
        let r = call(6, restart("SETTLED"));
        intent(&*store, &next, &r);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &next, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            gu(&service, &next, 7, later_address).result.segments[0].data,
            b"02B"
        );
    });
}
