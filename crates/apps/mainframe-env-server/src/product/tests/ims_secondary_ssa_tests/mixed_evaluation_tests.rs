//! Signed package, selected public route, actual durable coordinator.
use super::*;
use mainframe_env_execution_api::{Completion, MachineDrive, MachineResume, Quantum};
use mainframe_env_store_api::IdempotencyStore;

fn mixed(server: &ProductServer) {
    let run = "signed-composite";
    let mut indexed = b"PAUTSUM0(BYVALUE EQ".to_vec();
    indexed.extend([255, 255]);
    indexed.extend(b"|BYVALUE EQ");
    indexed.extend([0, 255]);
    indexed.extend(b"&BYVALUE EQ??)");
    for (selected, seq, raw, key) in [
        (
            1,
            30,
            b"PAUTSUM0(ACCNTID EQ000001|ACCNTID EQ000002&ACCNTID EQ000003)".as_slice(),
            b"000001",
        ),
        (3, 31, indexed.as_slice(), b"000001"),
    ] {
        let mut request = carddemo_request(ImsOperation::GetHoldUnique, seq, vec![]);
        request.pcb = selected;
        request.segments.clear();
        let nav = ImsNavigationRequest {
            request,
            context: ImsExecutionContext::DbBatch,
            ssas: vec![raw.to_vec()],
        };
        let invocation = tm_invocation(run, &format!("mixed-valid-{selected}"));
        let result = server
            .ims_navigation_selected("CARDDEMO-IMS", &invocation, &nav)
            .unwrap();
        assert_eq!(result.status, "  ");
        assert_eq!(&result.segments[0].data[..6], key);
        assert_eq!(
            server
                .ims_navigation_selected("CARDDEMO-IMS", &invocation, &nav)
                .unwrap(),
            result
        );
        let mut later = nav.clone();
        later.request = carddemo_request(ImsOperation::GetUnique, seq + 10, vec![]);
        later.request.pcb = selected;
        later.request.segments.clear();
        later.ssas = vec![b"PAUTSUM0(ACCNTID EQ000002)".to_vec()];
        assert_eq!(
            &server
                .ims_navigation_selected(
                    "CARDDEMO-IMS",
                    &tm_invocation(run, &format!("mixed-later-{selected}")),
                    &later
                )
                .unwrap()
                .segments[0]
                .data[..6],
            b"000002"
        );
        let before = server
            .store
            .list_provider_state("ims-v1-session-index", 262_144)
            .unwrap();
        assert_eq!(
            server
                .ims_navigation_selected("CARDDEMO-IMS", &invocation, &nav)
                .unwrap(),
            result
        );
        assert_eq!(
            server
                .store
                .list_provider_state("ims-v1-session-index", 262_144)
                .unwrap(),
            before
        );
    }
}

#[test]
fn mixed_evaluation_signed_selected_memory() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    composite(&server, &trust);
    mixed(&server);
}

struct NavigationMachine {
    effect: Option<EffectRequest>,
    result: Option<EffectResult>,
}
impl Machine for NavigationMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.effect.take().unwrap()),
            MachineResume::HostResult(result) => {
                self.result = Some(result);
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new("test@1", vec![], InvocationLimits::default())
                        .unwrap(),
                })
            }
            _ => panic!("unexpected navigation resume"),
        }
    }
}

fn coordinator(server: &ProductServer, store: Arc<dyn PlatformStore>) {
    // A second child pointer gives target 000001 both ffff and 00ff keys.
    // Target 000002 has only 00ff and must not satisfy Independent AND.
    let mut data = vec![0; 200];
    data[..8].copy_from_slice(b"CHILD003");
    data[10] = 255;
    let mut insert = carddemo_request(ImsOperation::Insert, 50, data);
    insert.segments = vec!["PAUTDTL1".into()];
    insert.qualifiers = vec![ImsQualifier {
        segment: "PAUTSUM0".into(),
        field: "ACCNTID".into(),
        value: b"000001".to_vec(),
    }];
    assert_eq!(
        server
            .ims_execute_selected(
                "CARDDEMO-IMS",
                &tm_invocation("signed-composite", "correlated-child"),
                &insert
            )
            .unwrap()
            .status,
        "  "
    );
    let mut invocation = tm_invocation("signed-composite", "mixed-coordinator");
    invocation.service_class = ServiceClass::Batch;
    let limits = InvocationLimits::default();
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", limits).unwrap(),
        [CapabilityId::new("host.ims.write", limits).unwrap()]
            .into_iter()
            .collect(),
        limits,
    )
    .unwrap();
    let key = IdempotencyKey::new("mixed-coordinator-effect", limits).unwrap();
    let mut request = carddemo_request(ImsOperation::GetHoldUnique, 1, vec![]);
    request.pcb = 3;
    request.segments.clear();
    request.mutation = Some(Mutation {
        sequence: 1,
        idempotency_key: key.clone(),
        transaction: None,
    });
    let mut raw = b"PAUTSUM0(BYVALUE EQ".to_vec();
    raw.extend([255, 255]);
    raw.extend(b"#BYVALUE EQ");
    raw.extend([0, 255]);
    raw.push(b')');
    let request = HostRequest::ImsNavigation(ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: vec![raw],
    });
    let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
    let mut machine = NavigationMachine {
        effect: Some(EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 1,
            idempotency_key: Some(key.clone()),
            deadline_tick: invocation.deadline_tick,
            request,
        }),
        result: None,
    };
    let coordinator = ExecutionCoordinator::durable(
        server.host.clone(),
        store.clone(),
        CoordinatorLimits::default(),
    );
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        ExecutionControl {
            now_tick: server.jes_clock.now_tick().unwrap(),
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    let result = machine.result.unwrap().outcome;
    let Ok(HostResult::Ims(found)) = &result else {
        panic!("{result:?}")
    };
    assert_eq!(found.status, "  ");
    assert_eq!(&found.segments[0].data[..6], b"000001");
    let effect = store.effect(&key).unwrap().unwrap();
    assert_eq!(effect.state, EffectState::Completed);
    assert_eq!(effect.digest_format, EffectDigestFormat::CanonicalHostV1);
    assert_eq!(effect.request_digest, digest);
    assert_eq!(
        effect.result_digest,
        Some(mainframe_env_host_api::canonical_result_digest(&result).unwrap())
    );
}

#[test]
fn mixed_evaluation_signed_package_actual_coordinator_memory() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    composite(&server, &trust);
    coordinator(&server, store);
}

#[test]
fn mixed_evaluation_signed_selected_sqlite_reopen_and_coordinator() {
    let file = std::env::temp_dir().join(format!(
        "ims-signed-evaluation-{}-{}.sqlite",
        std::process::id(),
        NEXT_SECONDARY_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Sqlite;
    settings.sqlite_url = url.clone();
    let trust = Arc::new(test_package_trust());
    let open = || {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let server = ProductServer::open_with_package_trust(
            settings.clone(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        (server, store)
    };
    let (server, store) = open();
    composite(&server, &trust);
    mixed(&server);
    coordinator(&server, store);
    drop(server);
    let (server, store) = open();
    mixed(&server);
    assert_eq!(
        store
            .effect(
                &IdempotencyKey::new("mixed-coordinator-effect", InvocationLimits::default())
                    .unwrap()
            )
            .unwrap()
            .unwrap()
            .state,
        EffectState::Completed
    );
    drop(server);
    drop(store);
    std::fs::remove_file(file).unwrap();
}
