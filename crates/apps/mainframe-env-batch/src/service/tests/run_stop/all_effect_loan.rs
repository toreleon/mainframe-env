//! Fixture transport through genuine Batch/scoped providers, not Core/JES/native proof.
use super::*;
use crate::BatchEffectOccurrence;
use mainframe_env_host_api::canonical_request_digest;

struct FixtureProgram {
    descriptor: CapabilityDescriptor,
    internal_reader: bool,
}
impl HostProvider for FixtureProgram {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        assert!(
            matches!(&request.request, HostRequest::Program(ProgramRequest::Call { program, .. })
            if program.as_str() == "APPMAIN")
        );
        if !self.internal_reader {
            return success(request.sequence);
        }
        EffectResult {
            sequence: request.sequence,
            outcome: Ok(HostResult::Program(
                BoundedPayload::new(
                    "mainframe-env.program.output@1",
                    b"{\"return_code\":0,\"records\":[],\"dd_outputs\":{\"OUT\":[[47,47,67,72,73,76,68,32,74,79,66,32,67,76,65,83,83,61,65],[47,47,83,32,69,88,69,67,32,80,71,77,61,65,80,80,77,65,73,78]]},\"termination\":null}".to_vec(),
                    InvocationLimits::default(),
                ).unwrap(),
            )),
        }
    }
}
fn configured(
    store: Arc<dyn ProviderStateStore>,
    checkpoints: Arc<dyn CheckpointStore>,
    internal_reader: bool,
) -> Fixture {
    fixture_with_program(
        store,
        checkpoints,
        Arc::new(FixtureProgram {
            descriptor: builtins().descriptor().clone(),
            internal_reader,
        }),
    )
}
fn memory_fixture(internal_reader: bool) -> Fixture {
    let store = Arc::new(MemoryStore::new(Default::default()));
    configured(store.clone(), store, internal_reader)
}
fn delegate(f: &Fixture, occurrence: &BatchEffectOccurrence<'_>) -> EffectResult {
    assert!(f.batch.state.try_lock().is_ok());
    assert!(f.batch.get("JOB00001").is_ok());
    let actual = occurrence
        .program_invocation()
        .unwrap_or(occurrence.original());
    ScopedHostService::invoke(
        &f.batch.host,
        actual,
        1,
        false,
        occurrence.request().clone(),
    )
    .persist_with(|audit| f.store.record_audit(audit).map_err(store_error))
}

fn normal(f: &Fixture) {
    seed_dd_catalog(f);
    let id = submit(
        f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//OUT DD DSN=&&TEMP,DISP=(NEW,PASS,DELETE),DCB=IBMUSER.BASE\n//STEP2 EXEC PGM=APPMAIN\n",
    );
    let before = physical(f, &id);
    let trace_start = f.trace.lock().unwrap().len();
    let original = invocation();
    let mut seen = Vec::new();
    let mut views = Vec::new();
    let exit = f
        .batch
        .run_claimed_with_all_effect_dispatch(
            &original,
            &id,
            "INIT0001",
            Some(&mut |o: &BatchEffectOccurrence<'_>| {
                assert_eq!(o.original(), &original);
                assert_eq!(o.request().run_unit, original.run_unit_id);
                assert_eq!(o.request().deadline_tick, 100);
                if let HostRequest::Program(_) = &o.request().request {
                    let a = o.running_step().expect("genuine Program admission");
                    a.check_live().unwrap();
                    let mut post = original.clone();
                    post.bindings.insert(
                        "jes.work-id".into(),
                        BoundedPayload::new(
                            "mainframe-env.jes-work@1",
                            b"jes:JOB00001".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    );
                    assert_eq!(o.program_invocation(), Some(&post));
                    views.push(a.retain_for_host());
                } else {
                    assert!(o.running_step().is_none());
                    assert!(o.program_invocation().is_none());
                }
                seen.push(o.request().clone());
                delegate(f, o)
            }),
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert!(exit.stop().is_none());
    assert_eq!(exit.snapshot().state, JobState::Completed);
    assert_eq!(views.len(), 2);
    assert!(
        views
            .iter()
            .all(|v| v.check_live() == Err(HostProblem::Unauthorized))
    );
    assert_eq!(&f.trace.lock().unwrap()[trace_start..], &seen);
    // Exactly one external scoped audit per occurrence; no second Batch audit.
    let after = physical(f, &id);
    assert_eq!(after.audits.len() - before.audits.len(), seen.len());
    assert!(
        seen.iter()
            .any(|r| matches!(r.request, HostRequest::Security(_)))
    );
    assert!(seen.iter().any(|r| matches!(
        r.request,
        HostRequest::Dataset(DatasetRequest::Attributes { .. })
    )));
    assert!(seen.iter().any(|r| matches!(
        r.request,
        HostRequest::Dataset(DatasetRequest::ListCatalog { .. })
    )));
    assert!(seen.iter().any(|r| matches!(
        r.request,
        HostRequest::Dataset(DatasetRequest::Define { .. })
    )));
    assert!(seen.iter().any(|r| matches!(
        r.request,
        HostRequest::Dataset(DatasetRequest::Delete { .. })
    )));
    assert!(
        seen.iter()
            .any(|r| matches!(r.request, HostRequest::Spool(_)))
    );
}
#[test]
fn memory_all_actual_dd_program_and_retirement_effects_loan_once() {
    normal(&memory_fixture(false));
}
#[test]
fn sqlite_all_actual_dd_program_and_retirement_effects_loan_once() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    normal(&configured(store.clone(), store, false));
}

#[test]
fn literal_original_program_request_pre_post_and_canonical_metadata_are_exact() {
    let f = memory_fixture(false);
    let id = submit(&f, TWO);
    let mut original = invocation();
    original.parent_execution_id =
        Some(ExecutionId::new("actual-parent", InvocationLimits::default()).unwrap());
    let mut calls = 0;
    let mut previous = None::<RunningStepView>;
    f.batch.run_claimed_with_all_effect_dispatch(&original, &id, "INIT0001",
        Some(&mut |o: &BatchEffectOccurrence<'_>| {
            assert_eq!(o.original(), &original);
            if let HostRequest::Program(_) = &o.request().request {
                if let Some(view) = &previous {
                    assert_eq!(view.check_live(), Err(HostProblem::Unauthorized));
                }
                calls += 1;
                let (sequence, key, payload) = if calls == 1 {
                    (1, "jes:JOB00001:STEP1:1", b"{\"parameter\":null,\"dds\":[],\"execution\":{\"job_name\":\"TESTJOB\",\"step_name\":\"STEP1\"}}".as_slice())
                } else {
                    (2, "jes:JOB00001:STEP2:2", b"{\"parameter\":null,\"dds\":[],\"execution\":{\"job_name\":\"TESTJOB\",\"step_name\":\"STEP2\"}}".as_slice())
                };
                let expected = EffectRequest {
                    run_unit: original.run_unit_id.clone(), sequence, deadline_tick: 100,
                    idempotency_key: Some(IdempotencyKey::new(key, InvocationLimits::default()).unwrap()),
                    request: HostRequest::Program(ProgramRequest::Call {
                        program: ProgramName::new("APPMAIN", 128).unwrap(),
                        payload: BoundedPayload::new("mainframe-env.program.input@1", payload.to_vec(), InvocationLimits::default()).unwrap(), service: None,
                    }),
                };
                assert_eq!(o.request(), &expected);
                assert_eq!(canonical_request_digest(&o.request().request).unwrap(), canonical_request_digest(&expected.request).unwrap());
                previous = Some(o.running_step().unwrap().retain_for_host());
            }
            delegate(&f, o)
        }), &mut control).unwrap().unwrap();
    assert_eq!(calls, 2);
    assert_eq!(
        previous.unwrap().check_live(),
        Err(HostProblem::Unauthorized)
    );
}

#[derive(Clone, Copy)]
enum Failure {
    Sequence,
    Malformed,
    Unknown,
    Panic,
    Cancel,
    Deadline,
    Drift,
    Control,
    Cas,
}
fn stopped(f: &Fixture, failure: Failure) {
    seed_dd_catalog(f);
    let id = submit(
        f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//STEP2 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//STEP3 EXEC PGM=APPMAIN\n",
    );
    let mut saved = None;
    let trigger = Cell::new(false);
    let start = f.trace.lock().unwrap().len();
    let mut count = 0;
    let mut view = None;
    let exit = f.batch.run_claimed_with_all_effect_dispatch(&invocation(), &id, "INIT0001",
        Some(&mut |o: &BatchEffectOccurrence<'_>| {
            count += 1;
            if let Some(a) = o.running_step() { view = Some(a.retain_for_host()); }
            let mut result = delegate(f, o);
            if matches!(&o.request().request, HostRequest::Dataset(DatasetRequest::ListCatalog { pattern, .. }) if pattern == "IBMUSER.BASE.PATH") {
                trigger.set(true);
                if matches!(failure, Failure::Cas) {
                    let mut row = f.store.get_provider_state("jes-job", &id).unwrap().unwrap();
                    let old = row.version;
                    row.version += 1;
                    f.store.put_provider_state(row, Some(old)).unwrap();
                }
                saved = Some(physical(f, &id));
                match failure {
                    Failure::Sequence => result.sequence += 1,
                    Failure::Malformed => result.outcome = Err(HostProblem::Malformed),
                    Failure::Unknown => result.outcome = Err(HostProblem::UnknownOutcome),
                    Failure::Panic => panic!("fixture callback panic after actual known audit"),
                    _ => {},
                }
            }
            result
        }), &mut || {
            if trigger.get() {
                match failure {
                    Failure::Cancel => return Ok(BatchRunControl { now_tick: 1, cancellation_requested: true }),
                    Failure::Deadline => return Ok(BatchRunControl { now_tick: 100, cancellation_requested: false }),
                    Failure::Drift => return Ok(BatchRunControl { now_tick: 0, cancellation_requested: false }),
                    Failure::Control => return Err(HostProblem::InfrastructureFailure),
                    _ => {},
                }
            }
            control()
        });
    let expected = match failure {
        Failure::Malformed => HostProblem::Malformed,
        Failure::Cancel => HostProblem::Cancelled,
        Failure::Deadline => HostProblem::TimedOut,
        Failure::Drift | Failure::Control => HostProblem::InfrastructureFailure,
        _ => HostProblem::UnknownOutcome,
    };
    if matches!(failure, Failure::Cas) {
        assert!(matches!(exit, Err(HostProblem::UnknownOutcome)));
    } else {
        let exit = exit.unwrap().unwrap();
        assert_eq!(exit.stop(), Some(&expected));
        assert_eq!(exit.snapshot().state, JobState::Running);
        assert_eq!(exit.snapshot().steps[0].state, StepState::Completed);
    }
    assert_eq!(
        physical(f, &id),
        saved.expect("known preceding effect/audit")
    );
    assert_eq!(f.trace.lock().unwrap().len() - start, count);
    assert_eq!(view.unwrap().check_live(), Err(HostProblem::Unauthorized));
}
#[test]
fn memory_nested_transport_faults_and_late_controls_fence_every_later_action() {
    for failure in [
        Failure::Sequence,
        Failure::Malformed,
        Failure::Unknown,
        Failure::Panic,
        Failure::Cancel,
        Failure::Deadline,
        Failure::Drift,
        Failure::Control,
        Failure::Cas,
    ] {
        stopped(&memory_fixture(false), failure);
    }
}
#[test]
fn sqlite_nested_transport_faults_and_late_controls_fence_every_later_action() {
    for failure in [
        Failure::Sequence,
        Failure::Malformed,
        Failure::Unknown,
        Failure::Panic,
        Failure::Cancel,
        Failure::Deadline,
        Failure::Drift,
        Failure::Control,
        Failure::Cas,
    ] {
        let directory = OwnedDirectory::create();
        let store = directory.store("rwc");
        stopped(&configured(store.clone(), store, false), failure);
    }
}

#[test]
fn missing_external_owner_refuses_without_control_callback_or_run_write() {
    let f = memory_fixture(false);
    let id = submit(&f, TWO);
    let before = physical(&f, &id);
    let before_trace = f.trace.lock().unwrap().clone();
    let missing: Option<&mut fn(&BatchEffectOccurrence<'_>) -> EffectResult> = None;
    assert!(matches!(
        f.batch.run_claimed_with_all_effect_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            missing,
            &mut || panic!("must not observe missing owner")
        ),
        Err(HostProblem::Unsupported)
    ));
    assert_eq!(physical(&f, &id), before);
    assert_eq!(*f.trace.lock().unwrap(), before_trace);
}

fn internal_reader(f: &Fixture) {
    let id = submit(
        f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD SYSOUT=(A,INTRDR)\n",
    );
    let mut seen = Vec::new();
    f.batch
        .run_claimed_with_all_effect_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            Some(&mut |o: &BatchEffectOccurrence<'_>| {
                seen.push(o.request().clone());
                delegate(f, o)
            }),
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(f.batch.get("JOB00002").unwrap().state, JobState::Queued);
    assert!(seen.iter().any(|r| matches!(&r.request, HostRequest::Security(SecurityRequest::Authorize { resource, .. }) if resource.as_str() == "JOB.JOB00001.INTRDR")));
    assert!(seen.iter().any(|r| matches!(&r.request, HostRequest::Spool(SpoolRequest::Append { job, .. }) if job.as_str() == "JOB00002")));
}
#[test]
fn memory_internal_reader_host_effects_use_external_owner_without_batch_lock() {
    internal_reader(&memory_fixture(true));
}
#[test]
fn sqlite_internal_reader_host_effects_use_external_owner_without_batch_lock() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    internal_reader(&configured(store.clone(), store, true));
}

fn program_fault(f: &Fixture, panic: bool) {
    let id = submit(f, DD);
    let mut saved = None;
    let mut retained = None;
    let mut count = 0;
    let exit = f
        .batch
        .run_claimed_with_all_effect_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            Some(&mut |o: &BatchEffectOccurrence<'_>| {
                count += 1;
                if let Some(a) = o.running_step() {
                    a.check_live().unwrap();
                    retained = Some(a.retain_for_host());
                    saved = Some((physical(f, &id), f.trace.lock().unwrap().clone(), count));
                    if panic {
                        panic!("fixture Program callback panic");
                    }
                    return EffectResult {
                        sequence: o.request().sequence + 1,
                        outcome: Err(HostProblem::UnknownOutcome),
                    };
                }
                delegate(f, o)
            }),
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exit.stop(), Some(&HostProblem::UnknownOutcome));
    let (rows, trace, calls) = saved.unwrap();
    assert_eq!(physical(f, &id), rows);
    assert_eq!(*f.trace.lock().unwrap(), trace);
    assert_eq!(count, calls);
    assert_eq!(
        retained.unwrap().check_live(),
        Err(HostProblem::Unauthorized)
    );
}
#[test]
fn memory_program_panic_unknown_and_raw_sequence_fault_revoke_before_cleanup() {
    for panic in [false, true] {
        program_fault(&memory_fixture(false), panic);
    }
}
#[test]
fn sqlite_program_panic_unknown_and_raw_sequence_fault_revoke_before_cleanup() {
    for panic in [false, true] {
        let directory = OwnedDirectory::create();
        let store = directory.store("rwc");
        program_fault(&configured(store.clone(), store, false), panic);
    }
}

fn normalized_shared_reply(f: &Fixture) {
    seed_dd_catalog(f);
    let id = submit(
        f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//STEP2 EXEC PGM=APPMAIN\n",
    );
    *f.fault.lock().unwrap() = Some(malformed_reply::normalized_catalog_fault(f));
    let mut stop = None;
    let result = f.batch.run_claimed_with_all_effect_dispatch(
        &invocation(),
        &id,
        "INIT0001",
        Some(&mut |o: &BatchEffectOccurrence<'_>| {
            let result = delegate(f, o);
            if result.outcome == Err(HostProblem::Malformed) {
                // Actual scoped normalization and audit, not a fabricated reply.
                assert_eq!(result.sequence, o.request().sequence);
                let captured = physical(f, &id);
                assert!(captured.audits.iter().any(|a| {
                    a.decision == mainframe_env_execution_api::AuditDecision::Rejected
                        && a.effect_sequence == o.request().sequence
                        && a.resource
                            == mainframe_env_host_api::canonical_audit_resource_digest(
                                &o.request().request,
                            )
                }));
                stop = Some((captured, f.trace.lock().unwrap().clone()));
            }
            result
        }),
        &mut control,
    );
    assert!(matches!(result, Err(HostProblem::Malformed)));
    let (captured, trace) = stop.expect("real scoped normalized malformed audit");
    assert_eq!(physical(f, &id), captured);
    assert_eq!(*f.trace.lock().unwrap(), trace);
}
#[test]
fn memory_actual_shared_normalized_malformed_reply_fences_all_effect_loan() {
    normalized_shared_reply(&memory_fixture(false));
}
#[test]
fn sqlite_actual_shared_normalized_malformed_reply_fences_all_effect_loan() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    normalized_shared_reply(&configured(store.clone(), store, false));
}
