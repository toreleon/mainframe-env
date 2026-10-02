use super::*;
use mainframe_env_execution_api::{Completion, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{ImsExecutionContext, ImsPcbFeedbackRequestV1, ImsPcbKeyFeedbackV1};

struct FeedbackMachine {
    effect: Option<EffectRequest>,
    result: Option<EffectResult>,
}
impl Machine for FeedbackMachine {
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
            _ => panic!("unexpected feedback resume"),
        }
    }
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let run = "feedback-signed";
    let mut invocation = tm_invocation(run, "feedback-signed");
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
    let make = |op, seq, data: Vec<u8>| ImsPcbFeedbackRequestV1 {
        request: carddemo_request(op, seq, data),
        context: ImsExecutionContext::DbBatch,
        ssas: None,
        key_capacity: 16,
    };
    let mut data = vec![b'X'; 100];
    data[..6].copy_from_slice(b"123456");
    let insert = make(ImsOperation::Insert, 2, data.clone());
    assert_eq!(
        server.ims_pcb_feedback_selected_v1("CARDDEMO-IMS", &invocation, &insert),
        Err(HostProblem::NotFound)
    );
    let mut corrupt = signed_carddemo_package(&trust, 1);
    corrupt.sections.ims_metadata.as_mut().unwrap().databases[0].version = 9;
    assert!(server.install_application_package_v2(&corrupt).is_err());
    let installed = server
        .install_application_package_v2(&signed_carddemo_package(&trust, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &invocation,
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    let inserted = server
        .ims_pcb_feedback_selected_v1("CARDDEMO-IMS", &invocation, &insert)
        .unwrap();
    assert_eq!(
        inserted.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: "PAUTSUM0".into(),
            segment_level: 1,
            bytes: b"123456".to_vec(),
        }
    );
    assert_eq!(inserted.feedback.transferred_data_length, 0);

    let get = make(ImsOperation::GetUnique, 3, vec![]);
    let request = HostRequest::ImsPcbFeedbackV1(get.clone());
    let mutation = request.mutation().unwrap();
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: mutation.sequence,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        request,
    };
    let key = effect.idempotency_key.clone().unwrap();
    let digest = mainframe_env_host_api::canonical_request_digest(&effect.request).unwrap();
    let mut machine = FeedbackMachine {
        effect: Some(effect),
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
    let result = machine.result.unwrap().outcome.unwrap();
    let HostResult::ImsPcbFeedbackV1(got) = result.clone() else {
        panic!("wrong result")
    };
    assert_eq!(got.feedback.key, inserted.feedback.key);
    assert_eq!(got.feedback.transferred_data_length, 100);
    assert_eq!(got.result.segments[0].data, data);
    let journal = store.effect(&key).unwrap().unwrap();
    assert_eq!(journal.state, EffectState::Completed);
    assert_eq!(journal.request_digest, digest);
    assert_eq!(
        journal.result_digest,
        Some(mainframe_env_host_api::canonical_result_digest(&Ok(result)).unwrap())
    );
    assert_eq!(
        server
            .ims_pcb_feedback_selected_v1("CARDDEMO-IMS", &invocation, &get)
            .unwrap(),
        got
    );
    let mut later = data;
    later[..6].copy_from_slice(b"654321");
    server
        .ims_pcb_feedback_selected_v1(
            "CARDDEMO-IMS",
            &invocation,
            &make(ImsOperation::Insert, 4, later),
        )
        .unwrap();
    assert_eq!(
        server
            .ims_pcb_feedback_selected_v1("CARDDEMO-IMS", &invocation, &get)
            .unwrap(),
        got
    );
    assert_eq!(
        server
            .ims_pcb_feedback_selected_v1("CARDDEMO-IMS", &invocation, &insert)
            .unwrap(),
        inserted
    );
}

#[test]
fn signed_memory_pcb_feedback_runs_through_real_coordinator() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}
#[test]
fn signed_file_sqlite_pcb_feedback_runs_through_real_coordinator() {
    let file =
        std::env::temp_dir().join(format!("ims-signed-feedback-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let mut configuration = config();
    configuration.store_profile = crate::StoreProfile::Sqlite;
    configuration.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        configuration,
    );
    std::fs::remove_file(file).unwrap();
}
