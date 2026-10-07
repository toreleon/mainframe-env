use super::*;
use mainframe_env_host_api::ImsResult;

struct Participant {
    descriptor: CapabilityDescriptor,
    calls: Mutex<Vec<(String, ImsRequest)>>,
    load_status: &'static str,
    commit_fails: bool,
}

impl HostProvider for Participant {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Ims(request) => {
                let operation = request.operation;
                self.calls
                    .lock()
                    .unwrap()
                    .push((invocation.run_unit_id.to_string(), request));
                if operation == ImsOperation::Commit && self.commit_fails {
                    Err(HostProblem::InfrastructureFailure)
                } else {
                    Ok(HostResult::Ims(ImsResult {
                        status: if operation == ImsOperation::Load {
                            self.load_status
                        } else {
                            "  "
                        }
                        .into(),
                        segments: Vec::new(),
                        checkpoint_id: None,
                        affected_segments: u64::from(operation == ImsOperation::Load),
                        system: None,
                    }))
                }
            }
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn fixture(
    load_status: &'static str,
    commit_fails: bool,
) -> (
    Arc<BatchService>,
    Arc<Participant>,
    Invocation,
    Job,
    ProgramInput,
) {
    let limits = InvocationLimits::default();
    let participant = Arc::new(Participant {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.ims.write", limits).unwrap(),
            provider_id: "ims-participant".into(),
            generation: "1".into(),
            request_schema: "ims@1".into(),
            result_schema: "ims@1".into(),
            max_request_bytes: 65536,
            max_result_bytes: 65536,
            ready: true,
        },
        calls: Mutex::new(Vec::new()),
        load_status,
        commit_fails,
    });
    let service = service(
        Arc::new(MemoryStore::new(Default::default())),
        participant.clone(),
    );
    let mut invocation = invocation();
    let mut grants = invocation.principal.grants().clone();
    grants.insert(CapabilityId::new("host.ims.write", limits).unwrap());
    invocation.principal =
        Principal::new(invocation.principal.id().clone(), grants, limits).unwrap();
    let submitted = service
        .submit(
            &invocation,
            &bundle("DFSRRC00"),
            &IdempotencyKey::new("ims-loader", limits).unwrap(),
            false,
        )
        .unwrap();
    let job = service.state.lock().unwrap().jobs[&submitted.id].clone();
    let mut generation = controller_generation(1, "LOADER");
    generation.controllers[0].selector =
        BatchControllerSelector::ims("BMP", "LOADER", Some("PSB")).unwrap();
    generation.controllers[0].plan = BatchControllerPlan::ImsLoad {
        database: "DATABASE".into(),
        root_dd: "ROOTS".into(),
        child_dd: "CHILDREN".into(),
        root_record_bytes: 4,
        child_record_bytes: 4,
        parent_key_bytes: 2,
    };
    service.install_controllers(generation).unwrap();
    let input = ProgramInput {
        parameter: Some("BMP,LOADER,PSB".into()),
        dds: Vec::new(),
        dd_records: BTreeMap::from([
            ("ROOTS".into(), vec![b"01AB".to_vec()]),
            ("CHILDREN".into(), vec![b"01CD".to_vec()]),
        ]),
        execution: None,
    };
    (service, participant, invocation, job, input)
}

#[test]
fn successful_loader_commits_same_owner_with_distinct_effect_before_success() {
    let (service, participant, invocation, job, input) = fixture("  ", false);
    let output = service
        .execute_ims_controller(&invocation, &job, &job.plan.steps[0], &input, &mut 0)
        .unwrap();
    assert_eq!(output.return_code, 0);
    let calls = participant.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1.operation, ImsOperation::Load);
    assert_eq!(calls[1].1.operation, ImsOperation::Commit);
    assert_eq!(calls[0].0, calls[1].0);
    assert_ne!(
        calls[0].1.mutation.as_ref().unwrap().idempotency_key,
        calls[1].1.mutation.as_ref().unwrap().idempotency_key
    );
}

#[test]
fn loader_failure_never_returns_success_or_commits_rejected_load() {
    let (service, participant, invocation, job, input) = fixture("GE", false);
    let output = service
        .execute_ims_controller(&invocation, &job, &job.plan.steps[0], &input, &mut 0)
        .unwrap();
    assert_eq!(output.return_code, 8);
    assert_eq!(participant.calls.lock().unwrap().len(), 1);
    let (service, participant, invocation, job, input) = fixture("  ", true);
    assert_eq!(
        service.execute_ims_controller(&invocation, &job, &job.plan.steps[0], &input, &mut 0),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(participant.calls.lock().unwrap().len(), 2);
}

#[test]
fn loader_rejects_unowned_sysin_before_loading() {
    let (service, participant, invocation, job, mut input) = fixture("  ", false);
    input
        .dd_records
        .insert("SYSIN".into(), vec![b"UNKNOWN CONTROL".to_vec()]);
    assert_eq!(
        service.execute_ims_controller(&invocation, &job, &job.plan.steps[0], &input, &mut 0),
        Err(HostProblem::Unsupported)
    );
    assert!(participant.calls.lock().unwrap().is_empty());
}

#[test]
fn purge_rejects_extra_or_unimplemented_controls_before_scheduling() {
    let (service, participant, invocation, job, mut input) = fixture("  ", false);
    let mut generation = controller_generation(2, "PURGE");
    generation.controllers[0].selector =
        BatchControllerSelector::ims("BMP", "PURGE", Some("PSB")).unwrap();
    generation.controllers[0].plan = BatchControllerPlan::ImsPurge {
        psb: "PSB".into(),
        root_segment: "ROOT".into(),
        child_segment: "CHILD".into(),
        control_dd: "SYSIN".into(),
        required_expiry_days: "00".into(),
        checkpoint_prefix: "DEMO".into(),
        summary_field: "SUMMARY".into(),
    };
    service.install_controllers(generation).unwrap();
    input.parameter = Some("BMP,PURGE,PSB".into());
    for (records, expected) in [
        (
            vec![b"00,00001,00001,Y".to_vec(), b"EXTRA".to_vec()],
            HostProblem::Malformed,
        ),
        (vec![b"00,00002,00001,Y".to_vec()], HostProblem::Unsupported),
        (vec![b"00,00001,00001,N".to_vec()], HostProblem::Unsupported),
        (vec![b"05,00001,00001,Y".to_vec()], HostProblem::Unsupported),
    ] {
        input.dd_records.insert("SYSIN".into(), records);
        assert_eq!(
            service.execute_ims_controller(&invocation, &job, &job.plan.steps[0], &input, &mut 0),
            Err(expected)
        );
    }
    assert!(participant.calls.lock().unwrap().is_empty());
}
