use super::*;

fn two_databases(attach: bool, unsupported: bool) -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let mut other = metadata.databases[0].clone();
    other.name = "OTHERDB".into();
    if unsupported {
        other.organization = ImsDatabaseOrganization::Dedb;
    }
    metadata.databases.push(other);
    if attach {
        let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
            panic!()
        };
        pcb.name = "DBPCB2".into();
        pcb.database = "OTHERDB".into();
        metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    }
    metadata
}

fn install(store: Arc<dyn ProviderStateStore>, catalog: ImsMetadataCatalog) -> Arc<ImsService> {
    let service =
        ImsService::open_authorized(store, ImsLimits::default(), Arc::new(Allow)).unwrap();
    service.install_metadata(catalog.clone()).unwrap();
    service
        .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog))
        .unwrap();
    service
}

#[test]
fn backout_sets_rejects_unsupported_pcb_and_setu_restores_only_supported_images() {
    backends("backout-setu", |store| {
        let service = install(store.clone(), two_databases(true, true));
        let invocation = invocation();
        seed(&service, &invocation);
        assert_eq!(
            invoke(&service, &store, &invocation, 1, point(*b"NOPE", &[])),
            ImsRecoveryResult::Savepoint {
                status: "SC".into()
            }
        );
        assert_eq!(
            invoke(&service, &store, &invocation, 2, rols(*b"NOPE", 0)),
            condition("RC")
        );
        assert_eq!(
            invoke(
                &service,
                &store,
                &invocation,
                3,
                ImsRecoveryCall::Setu {
                    token: Some(*b"PART"),
                    user_data: Some(b"partial".to_vec())
                }
            ),
            ImsRecoveryResult::Savepoint {
                status: "SC".into()
            }
        );
        insert(&service, &invocation, 103, b"02B");
        let mut other = database_request(ImsOperation::Insert, 104, b"11U");
        other.pcb = 2;
        assert_eq!(service.execute(&invocation, &other).unwrap().status, "  ");
        assert_eq!(
            invoke(&service, &store, &invocation, 4, rols(*b"PART", 7)),
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: b"partial".to_vec()
            }
        );
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        let mut read = database_request(ImsOperation::Unload, 999, &[]);
        read.psb = Some("OTHERDB".into());
        assert_eq!(
            service.execute(&invocation, &read).unwrap().segments[0].data,
            b"11U"
        );
        let request = call(5, ImsRecoveryCall::Rolb);
        intent(&*store, &invocation, &request);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &request),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn backout_and_selected_checkpoint_reject_unrelated_pending_database_work() {
    backends("backout-scope", |store| {
        let service = install(store.clone(), two_databases(false, false));
        let invocation = invocation();
        seed(&service, &invocation);
        let image = ImsGenericLoadImage {
            database: "OTHERDB".into(),
            records: vec![ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"11U".to_vec(),
            }],
        };
        let mut load = database_request(ImsOperation::Load, 103, &[]);
        load.data = serde_json::to_vec(&image).unwrap();
        assert_eq!(service.execute(&invocation, &load).unwrap().status, "  ");
        for (sequence, operation) in [
            (1, point(*b"SCOP", &[])),
            (2, ImsRecoveryCall::Rolb),
            (
                3,
                ImsRecoveryCall::BasicCheckpoint {
                    id: "NOCOMMIT".into(),
                },
            ),
        ] {
            let request = call(sequence, operation);
            intent(&*store, &invocation, &request);
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &request),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&*store), before);
        }
    });
}

#[test]
fn backout_all_real_pcb_positions_and_holds_reset_and_saf_covers_every_database() {
    struct DenyOther;
    impl EnterpriseAuthorizer for DenyOther {
        fn authorize(&self, _: &PrincipalId, r: &EnterpriseResource) -> Result<(), HostProblem> {
            if r.class == mainframe_env_host_api::EnterpriseResourceClass::ImsDatabase
                && r.name.as_str() == "OTHERDB"
            {
                Err(HostProblem::Unauthorized)
            } else {
                Ok(())
            }
        }
    }
    backends("backout-pcbs", |store| {
        let service = install(store.clone(), two_databases(true, false));
        let invocation = invocation();
        seed(&service, &invocation);
        let mut other = database_request(ImsOperation::Insert, 103, b"11X");
        other.pcb = 2;
        service.execute(&invocation, &other).unwrap();
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Commit, 104, &[]),
            )
            .unwrap();
        for (pcb, seq, key) in [(1, 105, b"01"), (2, 106, b"11")] {
            let mut hold = database_request(ImsOperation::GetHoldUnique, seq, &[]);
            hold.pcb = pcb;
            hold.segments = vec!["ROOT".into()];
            hold.qualifiers = vec![ImsQualifier {
                segment: "ROOT".into(),
                field: "KEY".into(),
                value: key.to_vec(),
            }];
            assert_eq!(service.execute(&invocation, &hold).unwrap().status, "  ");
        }
        let request = call(1, point(*b"PCBS", &[]));
        intent(&*store, &invocation, &request);
        let before = snapshot(&*store);
        let denied =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(DenyOther))
                .unwrap();
        assert_eq!(
            dispatch(denied, store.clone(), &invocation, &request),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&*store), before);
        dispatch(service.clone(), store.clone(), &invocation, &request).unwrap();
        for (pcb, seq, bytes) in [(1, 107, b"01Z"), (2, 108, b"11Z")] {
            let mut replace = database_request(ImsOperation::Replace, seq, bytes);
            replace.pcb = pcb;
            replace.segments = vec!["ROOT".into()];
            assert_eq!(service.execute(&invocation, &replace).unwrap().status, "  ");
        }
        invoke(&service, &store, &invocation, 2, rols(*b"PCBS", 0));
        for (pcb, seq, bytes) in [(1, 109, b"01Z"), (2, 110, b"11Z")] {
            let mut replace = database_request(ImsOperation::Replace, seq, bytes);
            replace.pcb = pcb;
            replace.segments = vec!["ROOT".into()];
            assert_eq!(service.execute(&invocation, &replace).unwrap().status, "DJ");
        }
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        let mut read = database_request(ImsOperation::Unload, 999, &[]);
        read.psb = Some("OTHERDB".into());
        assert_eq!(
            service.execute(&invocation, &read).unwrap().segments[0].data,
            b"11X"
        );
    });
}

#[test]
fn backout_db_only_guard_preserves_real_tm_buffers_express_outputs_and_work_lease() {
    use mainframe_env_store_api::WorkStore;
    let store = Arc::new(MemoryStore::new(Default::default()));
    let effects: Arc<dyn TestStore> = store.clone();
    let service = open(store.clone());
    let invocation = invocation();
    seed(&service, &invocation);
    insert(&service, &invocation, 103, b"02B");
    let tm = TmService::open(
        store.clone(),
        store.clone(),
        Arc::new(Allow),
        TmLimits::default(),
    )
    .unwrap();
    tm.install(TmDefinitionSet {
        transactions: vec![TmTransactionDefinition {
            code: "BACK".into(),
            psb: "LOGPSB".into(),
            program_selector: "ims:back".into(),
            artifact: "artifact:back".into(),
            required_generation: "backout-1".into(),
            context: TmExecutionContext::MessageProcessing,
            priority: 1,
            timeout_ticks: 100,
            conversational: false,
            spa_size: 0,
            alternate_pcbs: vec![TmAlternatePcbDefinition {
                name: "EXP".into(),
                destination: TmDestination::Fixed("TERM2".into()),
                express: true,
            }],
        }],
    })
    .unwrap();
    let with_key = |key: &str| {
        let mut i = invocation.clone();
        i.idempotency_key = IdempotencyKey::new(key, InvocationLimits::default()).unwrap();
        i
    };
    tm.enqueue(
        &with_key("backout-tm-enqueue"),
        TmInputMessage {
            message_id: "backout-tm-input".into(),
            transaction: "BACK".into(),
            source: "TERM1".into(),
            user_id: None,
            group_name: None,
            conversation_id: None,
            segments: vec![b"input".to_vec()],
        },
    )
    .unwrap();
    let work = tm.claim("BACK", "backout-worker", 2, 50).unwrap().unwrap();
    tm.start(&with_key("backout-tm-start"), &work).unwrap();
    tm.call(
        &with_key("backout-tm-output"),
        TmCall::Insert {
            pcb: TmPcb::Alternate("EXP".into()),
            segment: b"sent".to_vec(),
        },
    )
    .unwrap();
    tm.call(
        &with_key("backout-tm-purge"),
        TmCall::Purge {
            pcb: TmPcb::Alternate("EXP".into()),
        },
    )
    .unwrap();
    tm.call(
        &with_key("backout-tm-buffer"),
        TmCall::Insert {
            pcb: TmPcb::Io,
            segment: b"buffered".to_vec(),
        },
    )
    .unwrap();
    assert_eq!(tm.outbound("TERM2", 16).unwrap().len(), 1);
    let before = snapshot(&*store);
    let queues = [
        "ims-tm-v1-session",
        "ims-tm-v1-message",
        "ims-tm-v1-outbound",
    ]
    .map(|ns| store.list_provider_state(ns, 64).unwrap());
    let lease = store.get_work(&work.work_id).unwrap();
    let request = call(1, ImsRecoveryCall::Rolb);
    intent(&*store, &invocation, &request);
    assert_eq!(
        dispatch(service, effects, &invocation, &request),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&*store), before);
    assert_eq!(
        [
            "ims-tm-v1-session",
            "ims-tm-v1-message",
            "ims-tm-v1-outbound"
        ]
        .map(|ns| store.list_provider_state(ns, 64).unwrap()),
        queues
    );
    assert_eq!(store.get_work(&work.work_id).unwrap(), lease);
    assert_eq!(tm.outbound("TERM2", 16).unwrap().len(), 1);
}
