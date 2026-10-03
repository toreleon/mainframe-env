//! Invalid transport fixtures through the real shared boundary, not native/JES evidence.
use super::*;
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::canonical_audit_resource_digest;

#[derive(Clone, Copy)]
enum Kind {
    Catalog,
    LateCatalog,
    Security,
    KnownMalformed,
}

pub(super) struct Fault {
    kind: Kind,
    store: Arc<dyn ProviderStateStore>,
    checkpoints: Arc<dyn CheckpointStore>,
    captured: Option<(Physical, Vec<EffectRequest>, EffectResult, EffectResult)>,
}
pub(super) fn normalized_catalog_fault(f: &Fixture) -> Fault {
    Fault {
        kind: Kind::Catalog,
        store: f.store.clone(),
        checkpoints: f.checkpoints.clone(),
        captured: None,
    }
}
impl Fault {
    pub(super) fn corrupt(
        &mut self,
        request: &EffectRequest,
        result: &mut EffectResult,
        trace: &Mutex<Vec<EffectRequest>>,
    ) {
        let matches = match self.kind {
            Kind::Catalog | Kind::LateCatalog | Kind::KnownMalformed => matches!(&request.request,
                HostRequest::Dataset(DatasetRequest::ListCatalog { pattern, .. })
                    if pattern == "IBMUSER.BASE.PATH"),
            Kind::Security => matches!(&request.request,
                HostRequest::Security(SecurityRequest::Authorize { resource, .. })
                    if resource.as_str() == "IBMUSER.BASE"),
        };
        if !matches || self.captured.is_some() {
            return;
        }
        // The real read-only delegate completed. No fake result or audit replaces it.
        assert_eq!(result.sequence, request.sequence);
        assert!(result.outcome.is_ok());
        assert!(!request.request.is_mutating());
        let rows = ["jes", "spool", "artifact", "dataset"]
            .into_iter()
            .flat_map(|prefix| self.store.list_provider_state_prefix(prefix, 4096).unwrap())
            .collect();
        let before_audit = Physical {
            rows,
            audits: self
                .store
                .audit_records(&invocation().execution_id, 0, 4096)
                .unwrap(),
            checkpoint: self
                .checkpoints
                .get_checkpoint(&checkpoint_execution_id("JOB00001").unwrap())
                .unwrap(),
            epoch: self.store.provider_state_retention_epoch().unwrap(),
        };
        let delegated = result.clone();
        match self.kind {
            Kind::KnownMalformed => result.outcome = Err(HostProblem::Malformed),
            _ => result.sequence += 1,
        }
        self.captured = Some((
            before_audit,
            trace.lock().unwrap().clone(),
            delegated,
            result.clone(),
        ));
    }
}

fn case(f: &Fixture, kind: Kind, contained: bool) {
    seed_dd_catalog(f);
    let source = match kind {
        Kind::LateCatalog => {
            "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//STEP2 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//STEP3 EXEC PGM=APPMAIN\n"
        }
        Kind::Security => {
            "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//OUT DD DSN=&&TEMP,DISP=(NEW,PASS,DELETE),DCB=IBMUSER.BASE\n//STEP2 EXEC PGM=APPMAIN\n"
        }
        _ => {
            "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n//IN DD DSN=IBMUSER.BASE.PATH,DISP=SHR\n//STEP2 EXEC PGM=APPMAIN\n"
        }
    };
    let id = submit(f, source);
    if !contained {
        // Old authorization intentionally retains its historical lock behavior.
        *f.reentry.lock().unwrap() = None;
    }
    *f.fault.lock().unwrap() = Some(Fault {
        kind,
        store: f.store.clone(),
        checkpoints: f.checkpoints.clone(),
        captured: None,
    });
    let calls = Cell::new(0);
    let mut dispatch = |_: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
        calls.set(calls.get() + 1);
        success(request.sequence)
    };
    let result = if contained {
        f.batch
            .run_claimed_with_run_observer(
                &invocation(),
                &id,
                "INIT0001",
                &mut dispatch,
                &mut control,
            )
            .and_then(|exit| {
                if matches!(kind, Kind::LateCatalog) {
                    let exit = exit.expect("genuine first Running Program minted run owner");
                    assert_eq!(exit.stop(), Some(&HostProblem::Malformed));
                    assert_eq!(exit.snapshot().steps[0].state, StepState::Completed);
                    Err(HostProblem::Malformed)
                } else {
                    Ok(exit.map(|exit| exit.snapshot().clone()))
                }
            })
    } else {
        f.batch.run_claimed_with_program_dispatch(
            &invocation(),
            &id,
            "INIT0001",
            false,
            &mut dispatch,
        )
    };
    let (before, requests, raw, returned) = f
        .fault
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .captured
        .take()
        .expect("real nested delegate executed");
    let request = requests.last().unwrap();
    assert_eq!(raw.sequence, request.sequence);
    assert!(raw.outcome.is_ok());
    match kind {
        Kind::KnownMalformed => assert_eq!(returned.outcome, Err(HostProblem::Malformed)),
        _ => {
            assert_eq!(returned.sequence, request.sequence + 1);
            assert_eq!(returned.outcome, raw.outcome);
        }
    }
    assert_eq!(calls.get(), usize::from(matches!(kind, Kind::LateCatalog)));
    let after = physical(f, &id);
    if contained {
        assert!(matches!(result, Err(HostProblem::Malformed)));
        // This is the actual shared failure-audit stop boundary. Nothing else ran.
        assert_eq!(*f.trace.lock().unwrap(), requests);
        assert_eq!(after.rows, before.rows);
        assert_eq!(after.checkpoint, before.checkpoint);
        // The existing real audit publication advances retention exactly once.
        assert_eq!(after.epoch, before.epoch.checked_add(1).unwrap());
        assert_eq!(after.audits.len(), before.audits.len() + 1);
        // Audit enumeration is sequence-sorted, not append order. Submission
        // and run histories can interleave; preserve each old record exactly.
        let mut retained = after.audits.clone();
        let index = retained
            .iter()
            .position(|audit| {
                audit.decision == AuditDecision::Rejected
                    && audit.effect_sequence == request.sequence
                    && audit.resource == canonical_audit_resource_digest(&request.request)
            })
            .expect("actual shared failure audit");
        let audit = retained.remove(index);
        assert_eq!(retained, before.audits);
        assert_eq!(audit.decision, AuditDecision::Rejected);
        assert_eq!(audit.effect_sequence, request.sequence);
        assert_eq!(audit.execution_id, invocation().execution_id);
        assert_eq!(audit.run_unit_id, request.run_unit);
        assert_eq!(audit.principal, invocation().principal.id().clone());
        assert_eq!(audit.invocation_key, invocation().idempotency_key);
        assert_eq!(
            audit.resource,
            canonical_audit_resource_digest(&request.request)
        );
        assert_eq!(f.batch.get(&id).unwrap().state, JobState::Running);
        // Capture the complete audited stop boundary, including that epoch.
        assert_eq!(physical(f, &id), after);
    } else {
        // Older Program-only route keeps its existing known-error retirement.
        assert_eq!(result.unwrap().unwrap().state, JobState::Failed);
        assert_ne!(after.rows, before.rows);
        assert!(f.trace.lock().unwrap().len() > requests.len());
    }
}

#[test]
fn memory_nested_catalog_wrong_sequence_fences_shared_normalized_reply() {
    case(&memory(), Kind::Catalog, true);
}
#[test]
fn sqlite_nested_catalog_wrong_sequence_fences_shared_normalized_reply() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    case(&fixture(store.clone(), store), Kind::Catalog, true);
}
#[test]
fn memory_completed_first_step_then_malformed_nested_reply_keeps_checkpoint() {
    case(&memory(), Kind::LateCatalog, true);
}
#[test]
fn sqlite_completed_first_step_then_malformed_nested_reply_keeps_checkpoint() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    case(&fixture(store.clone(), store), Kind::LateCatalog, true);
}
#[test]
fn memory_nested_security_wrong_sequence_fences_dd_disposition() {
    case(&memory(), Kind::Security, true);
}
#[test]
fn sqlite_nested_security_wrong_sequence_fences_dd_disposition() {
    let directory = OwnedDirectory::create();
    let store = directory.store("rwc");
    case(&fixture(store.clone(), store), Kind::Security, true);
}
#[test]
fn known_malformed_read_only_reply_also_fences_contained_path() {
    case(&memory(), Kind::KnownMalformed, true);
}
#[test]
fn older_program_only_route_keeps_known_error_disposition() {
    case(&memory(), Kind::Catalog, false);
}

#[test]
fn shared_read_only_boundary_normalizes_sequence_but_retains_malformed() {
    let f = memory();
    seed_dd_catalog(&f);
    *f.fault.lock().unwrap() = Some(Fault {
        kind: Kind::Catalog,
        store: f.store.clone(),
        checkpoints: f.checkpoints.clone(),
        captured: None,
    });
    let original = invocation();
    let request = EffectRequest {
        run_unit: original.run_unit_id.clone(),
        sequence: 41,
        deadline_tick: original.deadline_tick,
        idempotency_key: None,
        request: HostRequest::Dataset(DatasetRequest::ListCatalog {
            pattern: "IBMUSER.BASE.PATH".into(),
            start: None,
            max_items: 2,
        }),
    };
    let result = f.batch.invoke_host(&original, 1, false, request);
    let (_, _, delegated, returned) = f
        .fault
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .captured
        .take()
        .unwrap();
    assert!(delegated.outcome.is_ok());
    assert_eq!(returned.sequence, 42);
    assert_eq!(result.sequence, 41);
    assert_eq!(result.outcome, Err(HostProblem::Malformed));
    let audits = f
        .store
        .audit_records(&original.execution_id, 0, 4096)
        .unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].decision, AuditDecision::Rejected);
    assert_eq!(audits[0].effect_sequence, 41);
}

#[test]
fn malformed_program_step_error_revokes_before_disposition() {
    let f = memory();
    let id = submit(&f, DD);
    let mut before = None;
    let mut trace = None;
    let mut view = None;
    let mut calls = 0;
    let exit = f
        .batch
        .run_claimed_with_run_observer(
            &invocation(),
            &id,
            "INIT0001",
            &mut |admission: &RunningStepAdmission<'_>, _: &Invocation, request: EffectRequest| {
                calls += 1;
                view = Some(admission.retain_for_host());
                before = Some(physical(&f, &id));
                trace = Some(f.trace.lock().unwrap().clone());
                EffectResult {
                    sequence: request.sequence,
                    outcome: Err(HostProblem::Malformed),
                }
            },
            &mut control,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exit.stop(), Some(&HostProblem::Malformed));
    assert_eq!(exit.snapshot().state, JobState::Running);
    assert_eq!(calls, 1);
    assert_eq!(physical(&f, &id), before.unwrap());
    assert_eq!(*f.trace.lock().unwrap(), trace.unwrap());
    assert_eq!(view.unwrap().check_live(), Err(HostProblem::Unauthorized));
}
