use super::*;
use crate::{RunningStepAdmission, RunningStepView};
use mainframe_env_store_api::AuditSink;

fn callback_result(sequence: u64) -> EffectResult {
    EffectResult {
        sequence,
        outcome: Ok(HostResult::Program(
            BoundedPayload::new(
                "mainframe-env.program.output@1",
                serde_json::to_vec(&ProgramOutput {
                    return_code: 0,
                    records: vec![],
                    dd_outputs: BTreeMap::new(),
                    termination: None,
                })
                .unwrap(),
                InvocationLimits::default(),
            )
            .unwrap(),
        )),
    }
}

fn submitted(batch: &BatchService) -> String {
    batch
        .submit(
            &invocation(),
            &bundle("APPMAIN"),
            &IdempotencyKey::new("running-step-test", InvocationLimits::default()).unwrap(),
            false,
        )
        .unwrap()
        .id
}

#[test]
fn genuine_running_callback_observes_exact_cas_registration_and_original() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let batch = service(store.clone(), builtins());
    let id = submitted(&batch);
    let mut original = invocation();
    original.parent_execution_id =
        Some(ExecutionId::new("real-parent", InvocationLimits::default()).unwrap());
    let mut retained = None;
    let mut emitted = None;
    let job = batch
        .run_claimed_with_program_dispatch(
            &original,
            &id,
            "INIT0001",
            false,
            &mut |admission: &RunningStepAdmission<'_>,
                  actual: &Invocation,
                  request: EffectRequest| {
                admission.check_live().unwrap();
                let view = admission.retain_for_host();
                view.check_live().unwrap();
                assert_eq!(view.original(), &original);
                assert_eq!(view.job_name(), "TESTJOB");
                assert_eq!(view.step_name(), "STEP1");
                assert_eq!(view.program(), "APPMAIN");
                assert_eq!((view.job_attempt(), view.step_attempt()), (1, 1));
                assert!(view.uses_store(&batch.store));
                let foreign: Arc<dyn ProviderStateStore> =
                    Arc::new(MemoryStore::new(Default::default()));
                assert!(!view.uses_store(&foreign));
                assert_eq!(
                    store.get_provider_state("jes-job", &id).unwrap().as_ref(),
                    Some(view.job_row())
                );
                let stored: Job = serde_json::from_slice(&view.job_row().payload).unwrap();
                assert_eq!(stored.state, JobState::Running);
                assert_eq!(stored.steps[0].state, StepState::Running);
                // A lock held across external dispatch would fail this synchronous check.
                assert!(batch.state.try_lock().is_ok());
                let mut expected = original.clone();
                expected.bindings.insert(
                    "jes.work-id".into(),
                    BoundedPayload::new(
                        "mainframe-env.jes-work@1",
                        format!("jes:{id}").into_bytes(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
                assert_eq!(actual, &expected);
                assert_eq!(actual.parent_execution_id, original.parent_execution_id);
                assert_eq!(request.run_unit, original.run_unit_id);
                assert_eq!(request.deadline_tick, original.deadline_tick);
                assert_eq!(request.sequence, stored.effect_sequence + 1);
                assert_eq!(
                    request.idempotency_key,
                    Some(effect_key(&stored, &stored.plan.steps[0], request.sequence).unwrap())
                );
                let HostRequest::Program(ProgramRequest::Call {
                    program,
                    payload,
                    service: None,
                }) = &request.request
                else {
                    panic!("original Program Call")
                };
                assert_eq!(program.as_str(), "APPMAIN");
                let input: ProgramInput = serde_json::from_slice(payload.bytes()).unwrap();
                assert_eq!(input.execution.unwrap().job_name, "TESTJOB");
                emitted = Some(request.clone());
                retained = Some(view);
                callback_result(request.sequence)
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(job.state, JobState::Completed);
    assert!(emitted.is_some());
    assert_eq!(
        retained.unwrap().check_live(),
        Err(HostProblem::Unauthorized)
    );
}

#[test]
fn old_run_claimed_request_is_identical_and_does_not_use_new_host_port() {
    struct Capture {
        descriptor: CapabilityDescriptor,
        request: Mutex<Option<(Invocation, EffectRequest)>>,
    }
    impl HostProvider for Capture {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, i: &Invocation, r: EffectRequest) -> EffectResult {
            *self.request.lock().unwrap() = Some((i.clone(), r.clone()));
            callback_result(r.sequence)
        }
    }
    let provider = Arc::new(Capture {
        descriptor: builtins().descriptor().clone(),
        request: Mutex::new(None),
    });
    let a = service(
        Arc::new(MemoryStore::new(Default::default())),
        provider.clone(),
    );
    let b = service(Arc::new(MemoryStore::new(Default::default())), builtins());
    let aid = submitted(&a);
    let bid = submitted(&b);
    assert_eq!(aid, bid);
    a.run_claimed(&invocation(), &aid, "INIT0001", false)
        .unwrap();
    let mut new = None;
    b.run_claimed_with_program_dispatch(
        &invocation(),
        &bid,
        "INIT0001",
        false,
        &mut |_: &RunningStepAdmission<'_>, i: &Invocation, r: EffectRequest| {
            new = Some((i.clone(), r.clone()));
            callback_result(r.sequence)
        },
    )
    .unwrap();
    assert_eq!(*provider.request.lock().unwrap(), new);
}

#[test]
fn error_panic_wrong_sequence_and_bad_reply_revoke_before_return() {
    for kind in 0..4 {
        let batch = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let id = submitted(&batch);
        let mut view = None;
        let result = batch.run_claimed_with_program_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            false,
            &mut |a: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                view = Some(a.retain_for_host());
                match kind {
                    0 => EffectResult {
                        sequence: r.sequence,
                        outcome: Err(HostProblem::ProviderFailure),
                    },
                    1 => panic!("bounded callback panic"),
                    2 => callback_result(r.sequence + 1),
                    _ => EffectResult {
                        sequence: r.sequence,
                        outcome: Ok(HostResult::Program(
                            BoundedPayload::new("wrong", vec![], InvocationLimits::default())
                                .unwrap(),
                        )),
                    },
                }
            },
        );
        if kind == 1 || kind == 2 {
            assert!(matches!(result, Err(HostProblem::UnknownOutcome)));
        } else {
            assert_eq!(result.unwrap().unwrap().state, JobState::Failed);
        }
        assert_eq!(view.unwrap().check_live(), Err(HostProblem::Unauthorized));
    }
}

#[test]
fn failed_running_checkpoint_never_mints_or_dispatches() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let checkpoints = Arc::new(FailNthCheckpointStore {
        inner: store.clone(),
        writes: AtomicUsize::new(0),
        fail_at: 2,
    });
    let batch = service_with_checkpoint_store(store, checkpoints, builtins()).unwrap();
    let id = submitted(&batch);
    let mut calls = 0;
    let result = batch.run_claimed_with_program_dispatch(
        &invocation(),
        &id,
        "INIT0001",
        false,
        &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
            calls += 1;
            callback_result(r.sequence)
        },
    );
    assert!(matches!(result, Err(HostProblem::UnknownOutcome)));
    assert_eq!(calls, 0);
}

#[test]
fn missing_or_conflicting_registration_never_mints() {
    for missing in [false, true] {
        let batch = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let id = submitted(&batch);
        {
            let mut state = batch.lock().unwrap();
            let job = state.jobs.get_mut(&id).unwrap();
            if missing {
                job.program_registrations.clear();
            } else {
                job.program_registrations.get_mut("STEP1").unwrap().program = "FOREIGN".into();
            }
        }
        let mut calls = 0;
        let result = batch.run_claimed_with_program_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            false,
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                calls += 1;
                callback_result(r.sequence)
            },
        );
        assert!(
            result.is_err()
                || result
                    .unwrap()
                    .is_some_and(|job| job.state == JobState::Failed)
        );
        assert_eq!(calls, 0);
    }
}

#[test]
fn stale_job_cas_and_cancelled_transition_never_mint() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let batch = service(store.clone(), builtins());
    let id = submitted(&batch);
    let mut row = store.get_provider_state("jes-job", &id).unwrap().unwrap();
    let prior = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(prior)).unwrap();
    let mut calls = 0;
    assert!(
        batch
            .run_claimed_with_program_dispatch(
                &invocation(),
                &id,
                "INIT0001",
                false,
                &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                    calls += 1;
                    callback_result(r.sequence)
                }
            )
            .is_err()
    );
    assert_eq!(calls, 0);
    let other = service(Arc::new(MemoryStore::new(Default::default())), builtins());
    let id = submitted(&other);
    other
        .run_claimed_with_program_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            true,
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                calls += 1;
                callback_result(r.sequence)
            },
        )
        .unwrap();
    assert_eq!(calls, 0);
}

#[test]
fn changed_job_observation_refuses_during_live_callback_without_dispatching() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let batch = service(store.clone(), builtins());
    let id = submitted(&batch);
    let mut retained = None;
    let result = batch.run_claimed_with_program_dispatch(
        &invocation(),
        &id,
        "INIT0001",
        false,
        &mut |a: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
            let view = a.retain_for_host();
            let mut row = view.job_row().clone();
            let prior = row.version;
            row.version += 1;
            store.put_provider_state(row, Some(prior)).unwrap();
            assert_eq!(view.check_live(), Err(HostProblem::Unauthorized));
            retained = Some(view);
            EffectResult {
                sequence: r.sequence,
                outcome: Err(HostProblem::ProviderFailure),
            }
        },
    );
    assert!(result.is_err());
    assert_eq!(
        retained.unwrap().check_live(),
        Err(HostProblem::Unauthorized)
    );
}

#[test]
fn retained_view_drop_performs_no_host_action_or_store_write() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let batch = service(store.clone(), builtins());
    let id = submitted(&batch);
    let mut retained: Option<RunningStepView> = None;
    batch
        .run_claimed_with_program_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            false,
            &mut |a: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                retained = Some(a.retain_for_host());
                callback_result(r.sequence)
            },
        )
        .unwrap();
    let before = store.list_provider_state_prefix("jes-", 4096).unwrap();
    let audits = store
        .audit_records(&invocation().execution_id, 0, 4096)
        .unwrap();
    drop(retained);
    assert_eq!(
        store.list_provider_state_prefix("jes-", 4096).unwrap(),
        before
    );
    assert_eq!(
        store
            .audit_records(&invocation().execution_id, 0, 4096)
            .unwrap(),
        audits
    );
}

#[test]
fn utilities_do_not_gain_running_program_admission() {
    let batch = service(Arc::new(MemoryStore::new(Default::default())), builtins());
    let id = batch
        .submit(
            &invocation(),
            &bundle("IEFBR14"),
            &IdempotencyKey::new("utility", InvocationLimits::default()).unwrap(),
            false,
        )
        .unwrap()
        .id;
    let mut calls = 0;
    assert_eq!(
        batch
            .run_claimed_with_program_dispatch(
                &invocation(),
                &id,
                "INIT0001",
                false,
                &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                    calls += 1;
                    callback_result(r.sequence)
                }
            )
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    assert_eq!(calls, 0);
}

#[test]
fn sqlite_genuine_running_view_is_revoked_and_not_restored_by_reopen() {
    let directory = std::env::temp_dir().join(format!(
        "mq-running-step-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("owned.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let store = Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65_536).unwrap());
    let batch = sqlite_service_with_checkpoint_store(store.clone()).unwrap();
    let id = submitted(&batch);
    let mut retained = None;
    assert_eq!(
        batch
            .run_claimed_with_program_dispatch(
                &invocation(),
                &id,
                "INIT0001",
                false,
                &mut |a: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                    a.check_live().unwrap();
                    retained = Some(a.retain_for_host());
                    callback_result(r.sequence)
                }
            )
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    let view = retained.unwrap();
    assert_eq!(view.check_live(), Err(HostProblem::Unauthorized));
    drop(view);
    drop(batch);
    drop(store);
    let reopened = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rw", path.display()),
            8 * 1024 * 1024,
            65_536,
        )
        .unwrap(),
    );
    let batch = sqlite_service_with_checkpoint_store(reopened.clone()).unwrap();
    assert_eq!(batch.get(&id).unwrap().state, JobState::Completed);
    let mut calls = 0;
    assert!(
        batch
            .run_claimed_with_program_dispatch(
                &invocation(),
                &id,
                "INIT0001",
                false,
                &mut |_: &RunningStepAdmission<'_>, _: &Invocation, r: EffectRequest| {
                    calls += 1;
                    callback_result(r.sequence)
                }
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(calls, 0);
    drop(batch);
    drop(reopened);
    std::fs::remove_dir_all(&directory).unwrap();
}
