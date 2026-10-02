use super::*;

fn system_request(
    run: &str,
    sequence: u64,
    pcb: u16,
    context: ImsExecutionContext,
    syntax: ImsCallSyntax,
    call: ImsSystemCall,
) -> ImsRequest {
    let mut request = request(run, ImsOperation::System, sequence, &[], b"");
    request.pcb = pcb;
    request.system = Some(ImsSystemRequest {
        context,
        syntax,
        call,
    });
    request
}

fn system_catalog() -> ImsMetadataCatalog {
    let mut result = catalog();
    let mut fast = result.databases[0].clone();
    fast.name = "FASTDB".into();
    fast.organization = ImsDatabaseOrganization::Dedb;
    fast.segments.truncate(1);
    result.databases.push(fast);
    let ImsPcbMetadata::Database(mut pcb) = result.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "FASTPCB".into();
    pcb.database = "FASTDB".into();
    pcb.sensitive_segments.truncate(1);
    result.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    result
}

fn exercise_system(store: Arc<dyn ProviderStateStore>) {
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(system_catalog()).unwrap();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            directory: Some(mainframe_env_host_api::ImsSystemDirectory {
                scd_address: 0x1000,
                pst_address: 0x2000,
            }),
            dedb_areas: vec![ImsDedbAreaDefinition {
                database: "FASTDB".into(),
                name: "FASTA".into(),
                sdep_capacity_cis: 20,
                iov_capacity_cis: 30,
            }],
            buffer_pools: vec![ImsBufferPoolDefinition {
                name: "OSAM1".into(),
                kind: ImsBufferPoolKind::Osam,
                buffer_bytes: 4096,
                buffers: 8,
            }],
        })
        .unwrap();
    let run = "system-run";
    let fast_run = "fast-run";
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    let mut schedule_fast = request(fast_run, ImsOperation::Schedule, 1, &[], b"");
    schedule_fast.pcb = 2;
    assert_eq!(execute(&service, fast_run, &schedule_fast).status, "  ");
    let query_run = "query-without-accept";
    assert_eq!(
        execute(
            &service,
            query_run,
            &request(query_run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    let direct_query = system_request(
        query_run,
        2,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, query_run, &direct_query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let accept = system_request(
        run,
        2,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Availability,
            group: ImsStatusGroup::A,
        },
    );
    assert_eq!(
        execute(&service, run, &accept).system,
        Some(ImsSystemResult::Accepted {
            group: ImsStatusGroup::A
        })
    );
    let query = system_request(
        run,
        3,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, run, &query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let provider_query = system_request(
        run,
        22,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 2 },
    );
    let provider_invocation = invocation(run);
    let provider = ims_providers(service.clone(), InvocationLimits::default()).remove(1);
    let routed = provider.invoke(
        &provider_invocation,
        EffectRequest {
            run_unit: provider_invocation.run_unit_id.clone(),
            sequence: 22,
            deadline_tick: provider_invocation.deadline_tick,
            idempotency_key: provider_query
                .mutation
                .as_ref()
                .map(|mutation| mutation.idempotency_key.clone()),
            request: HostRequest::Ims(provider_query),
        },
    );
    assert!(matches!(
        routed.outcome,
        Ok(HostResult::Ims(ImsResult {
            system: Some(ImsSystemResult::Query { .. }),
            ..
        }))
    ));
    let refresh = system_request(
        run,
        4,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Refresh,
    );
    assert_eq!(
        service.execute(&invocation(run), &refresh),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 5, &["ROOT"], b"A1X")
        )
        .status,
        "  "
    );
    assert!(matches!(execute(&service, run, &refresh).system,
        Some(ImsSystemResult::Refreshed { pcbs }) if pcbs.len() == 2));
    assert_eq!(
        service.execute(
            &invocation(run),
            &system_request(
                run,
                6,
                0,
                ImsExecutionContext::DbDc,
                ImsCallSyntax::Command,
                ImsSystemCall::Refresh
            )
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 7, &["ROOT"], b"B2Y")
        )
        .status,
        "  "
    );
    let mut first = request(run, ImsOperation::GetUnique, 8, &["ROOT"], b"");
    first.qualifiers = vec![qualifier(b"A1")];
    first.q_class = ImsQClass::new(b'A');
    assert_eq!(execute(&service, run, &first).status, "  ");
    let deq = system_request(
        run,
        9,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue {
            class: ImsQClass::new(b'A'),
        },
    );
    assert_eq!(
        execute(&service, run, &deq).system,
        Some(ImsSystemResult::Dequeued { released: 0 })
    );
    let mut second = request(run, ImsOperation::GetUnique, 10, &["ROOT"], b"");
    second.qualifiers = vec![qualifier(b"B2")];
    second.q_class = ImsQClass::new(b'A');
    assert_eq!(execute(&service, run, &second).status, "  ");
    let deq_release = system_request(
        run,
        11,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue {
            class: ImsQClass::new(b'A'),
        },
    );
    assert_eq!(
        execute(&service, run, &deq_release).system,
        Some(ImsSystemResult::Dequeued { released: 1 })
    );
    let mut missing = request(run, ImsOperation::GetUnique, 12, &["ROOT"], b"");
    missing.qualifiers = vec![qualifier(b"Z9")];
    assert_eq!(execute(&service, run, &missing).status, "GE");
    let stale_query = system_request(
        run,
        20,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, run, &stale_query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let late_refresh = system_request(
        run,
        21,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Refresh,
    );
    assert_eq!(
        service.execute(&invocation(run), &late_refresh),
        Err(HostProblem::Malformed)
    );
    let gscd = system_request(
        run,
        13,
        1,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Gscd,
    );
    let applicable = system::resources(
        &service.lock().unwrap().state,
        &invocation_class(run, ServiceClass::Batch),
        &gscd,
    );
    assert!(applicable.is_ok(), "{applicable:?}");
    let gscd_result = service
        .execute(&invocation_class(run, ServiceClass::Batch), &gscd)
        .unwrap();
    assert_eq!(gscd_result.status, "GE");
    assert_eq!(
        gscd_result.system,
        Some(ImsSystemResult::Gscd {
            scd_address: 0x1000,
            pst_address: 0x2000,
        })
    );
    let gscd_io = system_request(
        run,
        17,
        0,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Gscd,
    );
    assert_eq!(
        service
            .execute(&invocation_class(run, ServiceClass::Batch), &gscd_io)
            .unwrap()
            .status,
        "  "
    );
    service
        .publish_buffer_statistics(ImsBufferStatistics {
            pool: "OSAM1".into(),
            kind: ImsBufferPoolKind::Osam,
            buffer_bytes: 4096,
            buffers: 8,
            reads: 12,
            writes: 3,
        })
        .unwrap();
    let function = ImsStatisticsFunction {
        family: ImsStatisticsFamily::Dbas,
        format: ImsStatisticsFormat::Full,
        extended: false,
    };
    let stat = system_request(
        run,
        14,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics { function },
    );
    let observed = execute(&service, run, &stat);
    assert!(
        matches!(observed.system, Some(ImsSystemResult::Statistics { pool: Some(ref pool) })
        if pool.reads == 12 && pool.writes == 3)
    );
    assert_eq!(execute(&service, run, &stat), observed);
    let exhausted = system_request(
        run,
        15,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics { function },
    );
    assert_eq!(execute(&service, run, &exhausted).status, "GE");
    let no_vsam_pool = system_request(
        run,
        23,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Vbas,
                format: ImsStatisticsFormat::Full,
                extended: false,
            },
        },
    );
    assert_eq!(execute(&service, run, &no_vsam_pool).status, "GE");
    let position = system_request(
        fast_run,
        2,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert!(matches!(execute(&service, fast_run, &position).system,
        Some(ImsSystemResult::Positioned { areas }) if areas[0].unused_iov_cis == 30));
    let initial = system_request(
        fast_run,
        5,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Initial,
            group: ImsStatusGroup::B,
        },
    );
    assert_eq!(
        execute(&service, fast_run, &initial).system,
        Some(ImsSystemResult::Accepted {
            group: ImsStatusGroup::B
        })
    );
    let dedb_deq = system_request(
        fast_run,
        6,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue { class: None },
    );
    assert_eq!(
        execute(&service, fast_run, &dedb_deq).system,
        Some(ImsSystemResult::Dequeued { released: 0 })
    );
    let qualified = system_request(
        fast_run,
        7,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: Some(ImsPositionSsa {
                segment: "ROOT".into(),
                field: Some("ROOTKEY".into()),
                value: Some(b"Z9".to_vec()),
            }),
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert_eq!(execute(&service, fast_run, &qualified).status, "GE");
    service
        .publish_dedb_area_position(ImsPositionArea {
            name: "FASTA".into(),
            position: [0, 0, 0, 1, 0, 0, 0, 8],
            unused_sdep_cis: 19,
            unused_iov_cis: 29,
            timestamp: Some(123),
            ims_id: Some("IMSA".into()),
        })
        .unwrap();
    let before = service.lock().unwrap().state.clone();
    let forbidden = system_request(
        fast_run,
        3,
        2,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert_eq!(
        service.execute(&invocation_class(fast_run, ServiceClass::Batch), &forbidden),
        Err(HostProblem::Unsupported)
    );
    let malformed = system_request(
        run,
        16,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 0 },
    );
    assert_eq!(
        service.execute(&invocation(run), &malformed),
        Err(HostProblem::Malformed)
    );
    let malformed_stat = system_request(
        run,
        18,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Vbas,
                format: ImsStatisticsFormat::Full,
                extended: true,
            },
        },
    );
    assert_eq!(
        service.execute(&invocation(run), &malformed_stat),
        Err(HostProblem::Malformed)
    );
    assert_eq!(service.lock().unwrap().state, before);
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(reopened.execute(&invocation(run), &stat).unwrap(), observed);
    let resumed = system_request(
        fast_run,
        4,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert!(matches!(execute(&reopened, fast_run, &resumed).system,
        Some(ImsSystemResult::Positioned { areas })
            if areas[0].position == [0, 0, 0, 1, 0, 0, 0, 8]));
    assert_eq!(
        system::reservation_count(&reopened.lock().unwrap().state),
        1
    );
}

#[test]
fn system_families_replay_and_restart_memory() {
    exercise_system(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn system_families_replay_and_restart_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-system-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let store: Arc<dyn ProviderStateStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise_system(store);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn system_authorization_precedes_stat_observation_and_replay() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let policy = Arc::new(Policy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    service.install_metadata(system_catalog()).unwrap();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            directory: None,
            dedb_areas: Vec::new(),
            buffer_pools: vec![ImsBufferPoolDefinition {
                name: "OSAM1".into(),
                kind: ImsBufferPoolKind::Osam,
                buffer_bytes: 4096,
                buffers: 8,
            }],
        })
        .unwrap();
    let run = "system-auth";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let stat = system_request(
        run,
        2,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Dbas,
                format: ImsStatisticsFormat::Full,
                extended: false,
            },
        },
    );
    let before = service.lock().unwrap().state.clone();
    *policy.deny_read.lock().unwrap() = true;
    assert_eq!(
        service.execute(&invocation(run), &stat),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(service.lock().unwrap().state, before);
    *policy.deny_read.lock().unwrap() = false;
    assert!(matches!(
        execute(&service, run, &stat).system,
        Some(ImsSystemResult::Statistics { pool: Some(_) })
    ));
    *policy.deny_read.lock().unwrap() = true;
    assert_eq!(
        service.execute(&invocation(run), &stat),
        Err(HostProblem::Unauthorized)
    );
}
