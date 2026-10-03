//! Genuine Batch/physical-store fixtures, not installed/JES/native/core evidence.
use super::*;
use crate::{BatchRunControl, RunningStepAdmission, RunningStepView};
use mainframe_env_execution_api::AuditRecord;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Weak;

mod all_effect_loan;
mod malformed_reply;
mod selection_plan;
mod selection_prefetch;

struct Spy {
    inner: Arc<dyn HostProvider>,
    trace: Arc<Mutex<Vec<EffectRequest>>>,
    batch: Arc<Mutex<Option<Weak<BatchService>>>>,
    fault: Arc<Mutex<Option<malformed_reply::Fault>>>,
}
impl HostProvider for Spy {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.inner.descriptor()
    }
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult {
        if let Some(batch) = self.batch.lock().unwrap().as_ref().and_then(Weak::upgrade) {
            assert!(
                batch.state.try_lock().is_ok(),
                "opt-in held Batch mutex across host callback"
            );
            // Actual synchronous reentry into a state-reading public method.
            assert!(batch.get("JOB00001").is_ok());
        }
        self.trace.lock().unwrap().push(request.clone());
        let mut result = self.inner.invoke(invocation, request.clone());
        if let Some(fault) = self.fault.lock().unwrap().as_mut() {
            fault.corrupt(&request, &mut result, &self.trace);
        }
        result
    }
}

struct Fixture {
    batch: Arc<BatchService>,
    store: Arc<dyn ProviderStateStore>,
    checkpoints: Arc<dyn CheckpointStore>,
    trace: Arc<Mutex<Vec<EffectRequest>>>,
    dataset: Arc<DatasetService>,
    reentry: Arc<Mutex<Option<Weak<BatchService>>>>,
    fault: Arc<Mutex<Option<malformed_reply::Fault>>>,
}
fn fixture(store: Arc<dyn ProviderStateStore>, checkpoints: Arc<dyn CheckpointStore>) -> Fixture {
    fixture_with_program(store, checkpoints, builtins())
}
fn fixture_with_program(
    store: Arc<dyn ProviderStateStore>,
    checkpoints: Arc<dyn CheckpointStore>,
    program: Arc<dyn HostProvider>,
) -> Fixture {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let batch_link = Arc::new(Mutex::new(None));
    let fault = Arc::new(Mutex::new(None));
    let limits = InvocationLimits::default();
    let security: Arc<dyn HostProvider> = Arc::new(SecurityProvider {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.security.authorize", limits).unwrap(),
            provider_id: "test-security".into(),
            generation: "1".into(),
            request_schema: "security@1".into(),
            result_schema: "decision@1".into(),
            max_request_bytes: 65536,
            max_result_bytes: 65536,
            ready: true,
        },
        deny_spool: false,
        deny_control: false,
        deny_internal_reader: false,
    });
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    let mut providers = vec![security, program];
    providers.extend(dataset_providers(dataset.clone(), limits));
    providers.extend(spool_test_providers(store.clone()));
    let providers = providers
        .into_iter()
        .map(|inner| {
            Arc::new(Spy {
                inner,
                trace: trace.clone(),
                batch: batch_link.clone(),
                fault: fault.clone(),
            }) as Arc<dyn HostProvider>
        })
        .collect();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, providers, limits).unwrap()),
        HostLimits::default(),
    ));
    let batch = BatchService::open_with_checkpoint_store(
        host,
        store.clone(),
        checkpoints.clone(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    Fixture {
        batch,
        store,
        checkpoints,
        trace,
        dataset,
        reentry: batch_link,
        fault,
    }
}
fn memory() -> Fixture {
    let store = Arc::new(MemoryStore::new(Default::default()));
    fixture(store.clone(), store)
}
fn submit(f: &Fixture, source: &str) -> String {
    let id = f
        .batch
        .submit(
            &invocation(),
            &JclBundle {
                primary: source.into(),
                ..Default::default()
            },
            &IdempotencyKey::new("run-stop-fixture", InvocationLimits::default()).unwrap(),
            false,
        )
        .unwrap()
        .id;
    *f.reentry.lock().unwrap() = Some(Arc::downgrade(&f.batch));
    id
}
const TWO: &str = "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//STEP2 EXEC PGM=APPMAIN\n";
const DD: &str = "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD DSN=&&TEMP,DISP=(NEW,PASS,DELETE),RECFM=VB,LRECL=256\n//STEP2 EXEC PGM=APPMAIN\n";
fn success(sequence: u64) -> EffectResult {
    EffectResult {
        sequence,
        outcome: Ok(HostResult::Program(
            BoundedPayload::new(
                "mainframe-env.program.output@1",
                b"{\"return_code\":0,\"records\":[],\"dd_outputs\":{},\"termination\":null}"
                    .to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        )),
    }
}
fn control() -> Result<BatchRunControl, HostProblem> {
    Ok(BatchRunControl {
        now_tick: 1,
        cancellation_requested: false,
    })
}
#[derive(Debug, PartialEq)]
struct Physical {
    rows: Vec<ProviderStateRecord>,
    audits: Vec<AuditRecord>,
    checkpoint: Option<CheckpointRecord>,
    epoch: u64,
}
fn physical(f: &Fixture, id: &str) -> Physical {
    let rows = ["jes", "spool", "artifact", "dataset"]
        .into_iter()
        .flat_map(|prefix| f.store.list_provider_state_prefix(prefix, 4096).unwrap())
        .collect();
    Physical {
        rows,
        audits: f
            .store
            .audit_records(&invocation().execution_id, 0, 4096)
            .unwrap(),
        checkpoint: f
            .checkpoints
            .get_checkpoint(&checkpoint_execution_id(id).unwrap())
            .unwrap(),
        epoch: f.store.provider_state_retention_epoch().unwrap(),
    }
}

#[test]
fn actual_normal_exit_binds_original_store_rows_and_revokes_views() {
    let f = memory();
    let id = submit(&f, TWO);
    assert_eq!(id, "JOB00001");
    let mut original = invocation();
    original.parent_execution_id =
        Some(ExecutionId::new("actual-parent", InvocationLimits::default()).unwrap());
    let mut retained = Vec::<RunningStepView>::new();
    let mut requests = Vec::new();
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &original,
            &id,
            "INIT0001",
            &mut |a: &RunningStepAdmission<'_>, actual: &Invocation, request: EffectRequest| {
                a.check_live().unwrap();
                assert!(f.batch.state.try_lock().is_ok());
                let view = a.retain_for_host();
                assert_eq!(view.original(), &original);
                let mut expected = original.clone();
                expected.bindings.insert(
                    "jes.work-id".into(),
                    BoundedPayload::new(
                        "mainframe-env.jes-work@1",
                        b"jes:JOB00001".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
                assert_eq!(actual, &expected);
                let HostRequest::Program(ProgramRequest::Call {
                    program,
                    payload,
                    service: None,
                }) = &request.request
                else {
                    panic!("Program")
                };
                assert_eq!(program.as_str(), "APPMAIN");
                assert_eq!(payload.schema(), "mainframe-env.program.input@1");
                requests.push(request.clone());
                retained.push(view);
                success(request.sequence)
            },
            &mut || {
                assert!(f.batch.state.try_lock().is_ok());
                f.batch.get(&id).unwrap();
                control()
            },
        )
        .unwrap()
        .unwrap();
    assert!(exit.stop().is_none());
    assert_eq!(exit.snapshot().state, JobState::Completed);
    assert!(exit.uses_store(&f.store));
    let other: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    assert!(!exit.uses_store(&other));
    assert_eq!(exit.original(), &original);
    assert_eq!(requests[0].sequence, 1);
    assert_eq!(requests[1].sequence, 2);
    assert_eq!(
        requests[0].idempotency_key.as_ref().unwrap().as_str(),
        "jes:JOB00001:STEP1:1"
    );
    assert_eq!(
        requests[1].idempotency_key.as_ref().unwrap().as_str(),
        "jes:JOB00001:STEP2:2"
    );
    assert!(exit.last_row().version > exit.admitted_row().version);
    for view in retained {
        assert_eq!(view.check_live(), Err(HostProblem::Unauthorized));
    }
}

fn stopped_case(kind: usize, with_dd: bool) {
    let f = memory();
    let id = submit(&f, if with_dd { DD } else { TWO });
    let stopped = Arc::new(AtomicBool::new(false));
    let original = invocation();
    let mut before = None;
    let mut trace_len = 0;
    let mut calls = 0;
    let mut view = None;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &original,
            &id,
            "INIT0001",
            &mut |a: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                calls += 1;
                a.check_live().unwrap();
                view = Some(a.retain_for_host());
                before = Some(physical(&f, &id));
                trace_len = f.trace.lock().unwrap().len();
                stopped.store(true, Ordering::SeqCst);
                match kind {
                    0 => EffectResult {
                        sequence: request.sequence,
                        outcome: Err(HostProblem::UnknownOutcome),
                    },
                    1 => panic!("contained application callback"),
                    2 => success(request.sequence + 1),
                    _ => success(request.sequence),
                }
            },
            &mut || {
                if stopped.load(Ordering::SeqCst) && kind >= 3 {
                    match kind {
                        3 => Err(HostProblem::InfrastructureFailure),
                        4 => Ok(BatchRunControl {
                            now_tick: original.deadline_tick,
                            cancellation_requested: false,
                        }),
                        5 => Ok(BatchRunControl {
                            now_tick: 1,
                            cancellation_requested: true,
                        }),
                        _ => panic!("contained observer panic"),
                    }
                } else {
                    control()
                }
            },
        )
        .unwrap()
        .unwrap();
    let expected = match kind {
        3 => HostProblem::InfrastructureFailure,
        4 => HostProblem::TimedOut,
        5 => HostProblem::Cancelled,
        _ => HostProblem::UnknownOutcome,
    };
    assert_eq!(exit.stop(), Some(&expected));
    assert_eq!(exit.snapshot().state, JobState::Running);
    assert_eq!(exit.snapshot().steps[0].state, StepState::Running);
    assert_eq!(calls, 1);
    assert_eq!(f.trace.lock().unwrap().len(), trace_len);
    assert_eq!(physical(&f, &id), before.unwrap());
    assert_eq!(view.unwrap().check_live(), Err(HostProblem::Unauthorized));
}
#[test]
fn unknown_stops_before_dd_disposition_and_all_later_host_actions() {
    stopped_case(0, true);
}
#[test]
fn callback_panic_stops_before_dd_disposition() {
    stopped_case(1, true);
}
#[test]
fn wrong_sequence_stops_before_dd_disposition() {
    stopped_case(2, true);
}
#[test]
fn known_result_then_control_failure_preserves_physical_rows() {
    stopped_case(3, true);
}
#[test]
fn known_result_then_deadline_preserves_physical_rows() {
    stopped_case(4, false);
}
#[test]
fn known_result_then_cancellation_preserves_physical_rows() {
    stopped_case(5, true);
}
#[test]
fn observer_panic_preserves_unknown_and_physical_rows() {
    stopped_case(6, false);
}

#[test]
fn original_live_probe_cancellation_stops_before_disposition() {
    let f = memory();
    let id = submit(&f, DD);
    let original =
        invocation().with_cancellation_probe(mainframe_env_execution_api::CancellationProbe::new());
    let mut captured = None;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &original,
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, actual: &Invocation, request: EffectRequest| {
                captured = Some(physical(&f, &id));
                actual.cancellation_probe.as_ref().unwrap().request();
                success(request.sequence)
            },
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exit.stop(), Some(&HostProblem::Cancelled));
    assert_eq!(physical(&f, &id), captured.unwrap());
}

#[test]
fn actual_known_abend_disposition_is_retired_without_known_success_claim() {
    let f = memory();
    let id = submit(&f, TWO);
    let mut calls = 0;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                calls += 1;
                EffectResult {
                    sequence: request.sequence,
                    outcome: Err(HostProblem::Condition {
                        name: "ABEND:S0C7".into(),
                        response: -1,
                        response2: 0,
                    }),
                }
            },
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(calls, 1);
    assert!(exit.stop().is_none());
    assert_eq!(exit.snapshot().state, JobState::Failed);
    assert_eq!(exit.snapshot().abend_code.as_deref(), Some("S0C7"));
}

#[test]
fn changed_physical_row_poison_refuses_exit_from_equal_ids() {
    let f = memory();
    let id = submit(&f, TWO);
    let mut calls = 0;
    let result = f.batch.run_claimed_with_run_observer(
        &invocation(),
        &id,
        "INIT0001",
        &mut |a: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
            calls += 1;
            let view = a.retain_for_host();
            let mut row = view.job_row().clone();
            let old = row.version;
            row.version += 1;
            f.store.put_provider_state(row, Some(old)).unwrap();
            assert_eq!(view.check_live(), Err(HostProblem::Unauthorized));
            success(request.sequence)
        },
        &mut control,
    );
    assert!(matches!(result, Err(HostProblem::UnknownOutcome)));
    assert_eq!(calls, 1);
    assert_eq!(f.batch.get(&id).unwrap().state, JobState::Running);
}

#[test]
fn foreign_checkpoint_adapter_refuses_before_any_runner_action() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let f = fixture(store, Arc::new(MemoryStore::new(Default::default())));
    let id = submit(&f, TWO);
    let before = physical(&f, &id);
    let calls = Cell::new(0);
    assert!(matches!(
        f.batch.run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                calls.set(calls.get() + 1);
                success(request.sequence)
            },
            &mut control
        ),
        Err(HostProblem::Unsupported)
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(physical(&f, &id), before);
}

fn seed_dd_catalog(f: &Fixture) {
    let base = DatasetName::new("IBMUSER.BASE", 128).unwrap();
    f.dataset
        .invoke(DatasetRequest::Create {
            dataset: base.clone(),
            attributes: DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 8,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: Some(37),
            },
            mutation: dataset_test_mutation(560),
        })
        .unwrap();
    let index = DatasetName::new("IBMUSER.BASE.AIX", 128).unwrap();
    f.dataset
        .invoke(DatasetRequest::DefineAlternateIndex {
            base,
            index: index.clone(),
            key_offset: 2,
            key_length: 2,
            allow_duplicates: true,
            upgrade: true,
            mutation: dataset_test_mutation(562),
        })
        .unwrap();
    f.dataset
        .invoke(DatasetRequest::DefinePath {
            path: DatasetName::new("IBMUSER.BASE.PATH", 128).unwrap(),
            index,
            mutation: dataset_test_mutation(563),
        })
        .unwrap();
}

// Observe an actual known nested effect after its existing audit publication.
// Failure there must prevent every subsequent effect, not just Program dispatch.
fn nested_dd_stop(f: &Fixture, path: bool, cancel: bool) {
    seed_dd_catalog(f);
    let source = if path {
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//STEP2 EXEC PGM=APPMAIN\n"
    } else {
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD DSN=&&TEMP,DISP=(NEW,PASS,DELETE),DCB=IBMUSER.BASE\n//STEP2 EXEC PGM=APPMAIN\n"
    };
    let id = submit(f, source);
    let mut stopped = None;
    let mut count = 0;
    let result = f.batch.run_claimed_with_run_observer(&invocation(), &id, "INIT0001",
        &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
            count += 1; success(request.sequence)
        }, &mut || {
            let trace = f.trace.lock().unwrap();
            let matching = trace.last().is_some_and(|request| if path {
                matches!(&request.request, HostRequest::Dataset(DatasetRequest::ListCatalog { pattern, .. })
                    if pattern == "IBMUSER.BASE.PATH")
            } else {
                matches!(&request.request, HostRequest::Security(SecurityRequest::Authorize { resource, .. })
                    if resource.as_str() == "IBMUSER.BASE")
            });
            if matching {
                stopped = Some((physical(f, &id), trace.len()));
                if cancel { Ok(BatchRunControl { now_tick: 1, cancellation_requested: true }) }
                else { Err(HostProblem::InfrastructureFailure) }
            } else { control() }
        });
    assert!(
        matches!(result, Err(problem) if problem == if cancel { HostProblem::Cancelled } else { HostProblem::InfrastructureFailure })
    );
    let (before, requests) = stopped.expect("actual nested known effect reached");
    assert_eq!(physical(f, &id), before);
    assert_eq!(f.trace.lock().unwrap().len(), requests);
    assert_eq!(count, 0);
    assert_eq!(f.batch.get(&id).unwrap().state, JobState::Running);
}

#[test]
fn nested_dcb_authorization_stop_prevents_attributes_and_cleanup() {
    nested_dd_stop(&memory(), false, false);
}

#[test]
fn nested_path_first_catalog_cancellation_prevents_second_iteration() {
    nested_dd_stop(&memory(), true, true);
}

#[test]
fn owned_sqlite_nested_path_control_failure_retains_first_catalog_history() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    nested_dd_stop(&fixture(store.clone(), store), true, false);
}

struct OwnedDirectory(PathBuf);
impl OwnedDirectory {
    fn create() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mq-run-stop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(std::fs::canonicalize(path).unwrap())
    }
    fn store(&self, mode: &str) -> Arc<SqliteStateStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode={mode}", self.0.join("owned.db").display()),
                8 * 1024 * 1024,
                65_536,
            )
            .unwrap(),
        )
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn owned_sqlite_stopped_run_reopens_without_mutation_or_owner_resurrection() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    let f = fixture(store.clone(), store.clone());
    let id = submit(&f, TWO);
    let mut saved = None;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                saved = Some(physical(&f, &id));
                EffectResult {
                    sequence: request.sequence,
                    outcome: Err(HostProblem::UnknownOutcome),
                }
            },
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exit.stop(), Some(&HostProblem::UnknownOutcome));
    assert_eq!(&physical(&f, &id), saved.as_ref().unwrap());
    let physical_row = exit.last_row().clone();
    drop(exit);
    drop(f);
    drop(store);
    let reopened = directory.store("rw");
    assert_eq!(
        reopened.get_provider_state("jes-job", &id).unwrap(),
        Some(physical_row)
    );
    let f = fixture(reopened.clone(), reopened);
    let count = Cell::new(0);
    assert!(matches!(
        f.batch.run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                count.set(count.get() + 1);
                success(request.sequence)
            },
            &mut control
        ),
        Err(HostProblem::Unsupported)
    ));
    assert_eq!(count.get(), 0);
}

#[test]
fn owned_sqlite_normal_run_retirement_and_checkpoint_are_exact_after_reopen() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    let f = fixture(store.clone(), store.clone());
    let id = submit(&f, TWO);
    let mut calls = 0;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                calls += 1;
                success(request.sequence)
            },
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(calls, 2);
    assert!(exit.stop().is_none());
    assert_eq!(exit.snapshot().state, JobState::Completed);
    let row = exit.last_row().clone();
    let checkpoint = f
        .checkpoints
        .get_checkpoint(&checkpoint_execution_id(&id).unwrap())
        .unwrap()
        .unwrap();
    let captured: JesCheckpoint = serde_json::from_slice(&checkpoint.payload).unwrap();
    assert_eq!(captured.job_version, row.version);
    assert_eq!(captured.committed_steps, vec!["STEP1", "STEP2"]);
    drop(exit);
    drop(f);
    drop(store);
    let reopened = directory.store("rw");
    assert_eq!(
        reopened.get_provider_state("jes-job", &id).unwrap(),
        Some(row)
    );
    assert_eq!(
        reopened
            .get_checkpoint(&checkpoint_execution_id(&id).unwrap())
            .unwrap(),
        Some(checkpoint)
    );
}

#[test]
fn modeled_return_condition_keeps_existing_step_skip_and_retirement() {
    let f = memory();
    let id = submit(
        &f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//STEP2 EXEC PGM=APPMAIN,COND=(4,EQ)\n",
    );
    let mut calls = 0;
    let exit = f.batch.run_claimed_with_run_observer(&invocation(), &id, "INIT0001",
        &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
            calls += 1; EffectResult { sequence: request.sequence, outcome: Ok(HostResult::Program(
                BoundedPayload::new("mainframe-env.program.output@1",
                    b"{\"return_code\":4,\"records\":[],\"dd_outputs\":{},\"termination\":null}".to_vec(),
                    InvocationLimits::default()).unwrap())) }
        }, &mut control).unwrap().unwrap();
    assert!(exit.stop().is_none());
    assert_eq!(calls, 1);
    assert_eq!(exit.snapshot().state, JobState::Completed);
    assert_eq!(exit.snapshot().return_code, Some(4));
    assert_eq!(exit.snapshot().steps[1].state, StepState::SkippedCondition);
}

#[test]
fn contained_internal_reader_output_never_holds_batch_lock_across_callbacks() {
    let f = memory();
    let id = submit(
        &f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD SYSOUT=(A,INTRDR)\n",
    );
    let result = f.batch.run_claimed_with_run_observer(
        &invocation(),
        &id,
        "INIT0001",
        &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
            // Literal input fixture uses the existing Program output vocabulary.
            let payload = serde_json::to_vec(&ProgramOutput {
                return_code: 0,
                records: vec![],
                dd_outputs: BTreeMap::from([(
                    "OUT".into(),
                    vec![
                        b"//CHILD JOB CLASS=A".to_vec(),
                        b"//S EXEC PGM=APPMAIN".to_vec(),
                    ],
                )]),
                termination: None,
            })
            .unwrap();
            EffectResult {
                sequence: request.sequence,
                outcome: Ok(HostResult::Program(
                    BoundedPayload::new(
                        "mainframe-env.program.output@1",
                        payload,
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                )),
            }
        },
        &mut control,
    );
    let exit = result.unwrap().unwrap();
    assert!(exit.stop().is_none());
    assert_eq!(exit.snapshot().state, JobState::Completed);
    let child = f.batch.get("JOB00002").unwrap();
    assert_eq!(child.state, JobState::Queued);
    assert_eq!(
        child.origin,
        JesSubmissionOrigin::InternalReader {
            parent_job_id: id,
            step_name: "STEP1".into()
        }
    );
}

#[test]
fn internal_reader_known_spool_then_stop_does_not_cleanup_or_publish_child() {
    let f = memory();
    let id = submit(
        &f,
        "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD SYSOUT=(A,INTRDR)\n",
    );
    let mut stopped = None;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                let payload = serde_json::to_vec(&ProgramOutput {
                    return_code: 0,
                    records: vec![],
                    dd_outputs: BTreeMap::from([(
                        "OUT".into(),
                        vec![
                            b"//CHILD JOB CLASS=A".to_vec(),
                            b"//S EXEC PGM=APPMAIN".to_vec(),
                        ],
                    )]),
                    termination: None,
                })
                .unwrap();
                EffectResult {
                    sequence: request.sequence,
                    outcome: Ok(HostResult::Program(
                        BoundedPayload::new(
                            "mainframe-env.program.output@1",
                            payload,
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    )),
                }
            },
            &mut || {
                let trace = f.trace.lock().unwrap();
                if trace.last().is_some_and(|request| {
                    matches!(&request.request,
                HostRequest::Spool(SpoolRequest::Append { job, .. }) if job.as_str() == "JOB00002")
                }) {
                    stopped = Some((physical(&f, &id), trace.len()));
                    Err(HostProblem::InfrastructureFailure)
                } else {
                    control()
                }
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(exit.stop(), Some(&HostProblem::InfrastructureFailure));
    assert_eq!(exit.snapshot().state, JobState::Running);
    let (rows, requests) = stopped.expect("known actual child spool/audit");
    assert_eq!(physical(&f, &id), rows);
    assert_eq!(f.trace.lock().unwrap().len(), requests);
    assert_eq!(
        f.store.get_provider_state("jes-job", "JOB00002").unwrap(),
        None
    );
}
