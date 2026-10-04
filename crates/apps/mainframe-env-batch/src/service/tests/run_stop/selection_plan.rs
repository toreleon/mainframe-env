//! Actual submitted/registered Batch fixtures. No joined Core/JES/native admission.
use super::*;
use crate::service::run_stop::RunScope;

const ONE: &str = "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n";

fn prepare<'f>(
    f: &'f Fixture,
    id: &str,
) -> Result<crate::service::prepared_selection::PreparedSelection<'f>, HostProblem> {
    let original = invocation();
    let mut observer = control;
    let scope = RunScope::new(&original, &mut observer);
    f.batch.prepare_selection(&scope, id, "MEMBER1", "INIT0001")
}

fn structural(f: &Fixture) {
    let id = submit(f, ONE);
    let before = physical(f, &id);
    let trace = f.trace.lock().unwrap().len();
    let plan = prepare(f, &id).unwrap();
    assert_eq!(plan.configuration_rows(), [None, None]);
    assert_eq!(
        plan.current_row(),
        &f.store.get_provider_state("jes-job", &id).unwrap().unwrap()
    );
    assert_eq!(plan.dependencies(), &[plan.current_row().clone()]);
    let current: Job = serde_json::from_slice(&plan.current_row().payload).unwrap();
    assert_eq!(current.version, 1);
    assert_eq!(current.attempt, 0);
    assert_eq!(current.state, JobState::Queued);
    assert_eq!(
        current.events,
        ["submitted", "admitted", "origin:external", "queued"]
    );
    assert_eq!(
        current.route,
        JesJobRoute {
            origin_node: "LOCAL".into(),
            execution_node: "LOCAL".into(),
            output_node: "LOCAL".into(),
            owner_member: None
        }
    );
    assert_eq!(
        current.program_registrations["STEP1"],
        ProgramRegistration {
            schema_version: "mainframe-env.jes-utility-registry@1".into(),
            program: "APPMAIN".into(),
            disposition: None,
            handler: RegisteredProgramHandler::ProgramService,
        }
    );
    let writes = plan.expected_writes();
    // Independent literal transitions applied to the complete actual current input;
    // every unchanged field is compared, not a subset assembled from plan outputs.
    let mut expected_selected = current.clone();
    expected_selected.version = 2;
    expected_selected.state = JobState::Selected;
    expected_selected.initiator = Some("INIT0001".into());
    expected_selected.route.owner_member = Some("MEMBER1".into());
    expected_selected.events = vec![
        "submitted".into(),
        "admitted".into(),
        "origin:external".into(),
        "queued".into(),
        "selected:INIT0001".into(),
    ];
    let mut expected_running = expected_selected.clone();
    expected_running.version = 3;
    expected_running.attempt = 1;
    expected_running.state = JobState::Running;
    expected_running.events.push("running".into());
    assert_eq!(
        writes,
        [
            &job_record(&expected_selected).unwrap(),
            &job_record(&expected_running).unwrap()
        ]
    );
    plan.revalidate(&f.batch, &invocation()).unwrap();
    let after = physical(f, &id);
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.checkpoint, before.checkpoint);
    // The existing standalone audit advances the shared retention epoch once.
    // Planning itself does not write provider rows or a checkpoint.
    assert_eq!(after.epoch, before.epoch + 1);
    assert_eq!(after.audits.len(), before.audits.len() + 1); // existing standalone preflight
    let calls = f.trace.lock().unwrap();
    assert_eq!(calls.len(), trace + 1);
    assert_eq!(
        calls.last().unwrap(),
        &EffectRequest {
            run_unit: invocation().run_unit_id,
            sequence: 2,
            deadline_tick: 100,
            idempotency_key: None,
            request: HostRequest::Security(SecurityRequest::Authorize {
                principal: invocation().principal.id().clone(),
                class: "JESJOBS".into(),
                resource: ResourceName::new("JOB.TESTJOB", 246).unwrap(),
                intent: AccessIntent::Execute,
            }),
        }
    );
    drop(calls);
    let mut altered = invocation();
    altered.deadline_tick -= 1;
    assert_eq!(
        plan.revalidate(&f.batch, &altered),
        Err(HostProblem::Unauthorized)
    );
    let same_store_facade = BatchService::open_with_checkpoint_store(
        f.batch.host.clone(),
        f.store.clone(),
        f.checkpoints.clone(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        plan.revalidate(&same_store_facade, &invocation()),
        Err(HostProblem::Unauthorized)
    );
    // No plan can cross a physical adapter boundary even with exactly equal Job bytes.
    let foreign = memory();
    let foreign_id = submit(&foreign, ONE);
    assert_eq!(foreign_id, id);
    assert_eq!(
        foreign.store.get_provider_state("jes-job", &id).unwrap(),
        Some(plan.current_row().clone())
    );
    assert_eq!(
        plan.revalidate(&foreign.batch, &invocation()),
        Err(HostProblem::Unauthorized)
    );
    // A recovered facade on the SAME store is still only an observation. No drive API exists.
    let mut changed = f.store.get_provider_state("jes-job", &id).unwrap().unwrap();
    changed.version += 1;
    f.store.put_provider_state(changed, Some(1)).unwrap();
    assert_eq!(
        plan.revalidate(&f.batch, &invocation()),
        Err(HostProblem::IdempotencyConflict)
    );
}

#[test]
fn memory_exact_plan_and_preflight_are_observations_without_job_or_checkpoint_writes() {
    structural(&memory());
}
#[test]
fn sqlite_exact_plan_and_preflight_are_observations_without_job_or_checkpoint_writes() {
    let dir = OwnedDirectory::create();
    let store = dir.store("rwc");
    structural(&fixture(store.clone(), store));
}

#[derive(Clone, Copy)]
enum Change {
    Row,
    Phantom,
    Scheduler,
    Topology,
    SchedulerPhysical,
    TopologyPhysical,
    Deny,
    Cancel,
    Control,
}
fn changed_preflight(f: &Fixture, change: Change) {
    let id = submit(f, ONE);
    let before = physical(f, &id);
    let original = invocation();
    let stopped = Cell::new(false);
    let mut saved = None;
    let result = f.batch.run_claimed_with_all_effect_dispatch(
        &original,
        &id,
        "INIT0001",
        Some(&mut |o: &crate::BatchEffectOccurrence<'_>| {
            assert!(o.running_step().is_none());
            assert!(o.program_invocation().is_none());
            assert!(f.batch.state.try_lock().is_ok());
            assert!(f.batch.scheduler.try_lock().is_ok());
            assert!(f.batch.topology.try_lock().is_ok());
            let mut result = ScopedHostService::invoke(
                &f.batch.host,
                o.original(),
                1,
                false,
                o.request().clone(),
            )
            .persist_with(|audit| f.store.record_audit(audit).map_err(store_error));
            match change {
                Change::Row => {
                    let mut row = f.store.get_provider_state("jes-job", &id).unwrap().unwrap();
                    row.version += 1;
                    f.store.put_provider_state(row, Some(1)).unwrap();
                }
                Change::Phantom => {
                    let mut row = f.store.get_provider_state("jes-job", &id).unwrap().unwrap();
                    row.key = "JOB99999".into();
                    f.store.put_provider_state(row, None).unwrap();
                }
                Change::Scheduler => {
                    f.batch
                        .scheduler
                        .lock()
                        .unwrap()
                        .configuration
                        .initiators
                        .get_mut("INIT0001")
                        .unwrap()
                        .minimum_priority = 1;
                }
                Change::Topology => {
                    f.batch
                        .topology
                        .lock()
                        .unwrap()
                        .configuration
                        .members
                        .get_mut("MEMBER1")
                        .unwrap()
                        .max_active = 2;
                }
                Change::SchedulerPhysical => {
                    f.batch.stop_initiator(&invocation(), "INIT0001").unwrap();
                }
                Change::TopologyPhysical => {
                    let mut config = f.batch.topology().unwrap();
                    config.members.get_mut("MEMBER1").unwrap().max_active = 2;
                    f.batch.install_topology(&invocation(), config).unwrap();
                }
                Change::Deny => result.outcome = Ok(HostResult::Security(SecurityDecision::Deny)),
                Change::Cancel | Change::Control => stopped.set(true),
            }
            saved = Some(physical(f, &id));
            result
        }),
        &mut || {
            if stopped.get() {
                if matches!(change, Change::Control) {
                    return Err(HostProblem::InfrastructureFailure);
                }
                return Ok(BatchRunControl {
                    now_tick: 1,
                    cancellation_requested: true,
                });
            }
            control()
        },
    );
    let expected = match change {
        Change::Deny => HostProblem::Unauthorized,
        Change::Cancel => HostProblem::Cancelled,
        Change::Control => HostProblem::InfrastructureFailure,
        _ => HostProblem::IdempotencyConflict,
    };
    assert!(matches!(result, Err(ref p) if *p == expected));
    assert_eq!(physical(f, &id), saved.unwrap());
    assert_eq!(f.batch.get(&id).unwrap().state, JobState::Queued);
    assert_eq!(f.batch.get(&id).unwrap().attempt, 0);
    assert_eq!(physical(f, &id).checkpoint, before.checkpoint);
}
#[test]
fn memory_changed_preflight_physical_semantic_and_controls_cannot_publish_selection() {
    for change in [
        Change::Row,
        Change::Phantom,
        Change::Scheduler,
        Change::Topology,
        Change::SchedulerPhysical,
        Change::TopologyPhysical,
        Change::Deny,
        Change::Cancel,
        Change::Control,
    ] {
        changed_preflight(&memory(), change);
    }
}
#[test]
fn sqlite_changed_preflight_physical_semantic_and_controls_cannot_publish_selection() {
    for change in [
        Change::Row,
        Change::Phantom,
        Change::Scheduler,
        Change::Topology,
        Change::SchedulerPhysical,
        Change::TopologyPhysical,
        Change::Deny,
        Change::Cancel,
        Change::Control,
    ] {
        let dir = OwnedDirectory::create();
        let store = dir.store("rwc");
        changed_preflight(&fixture(store.clone(), store), change);
    }
}

fn prerequisites(f: &Fixture) {
    let id = submit(f, ONE);
    let before = physical(f, &id);
    let start = f.trace.lock().unwrap().len();
    assert!(matches!(
        f.batch
            .prepare_selection(&invocation(), &id, "MEMBER1", "INIT0001"),
        Err(HostProblem::Unsupported)
    ));
    let original = invocation();
    let mut observer = control;
    let scope = RunScope::new(&original, &mut observer);
    assert!(matches!(
        f.batch
            .prepare_selection(&scope, &id, "FOREIGN", "INIT0001"),
        Err(HostProblem::IdempotencyConflict)
    ));
    let mut foreign = original.clone();
    foreign.principal = Principal::new(
        PrincipalId::new("OTHER", InvocationLimits::default()).unwrap(),
        BTreeSet::new(),
        InvocationLimits::default(),
    )
    .unwrap();
    let mut observer = control;
    let foreign_scope = RunScope::new(&foreign, &mut observer);
    assert!(matches!(
        f.batch
            .prepare_selection(&foreign_scope, &id, "MEMBER1", "INIT0001"),
        Err(HostProblem::Unauthorized)
    ));
    // Corrupt only the fixture's current semantic registration; physical exactness must fail.
    f.batch
        .state
        .lock()
        .unwrap()
        .jobs
        .get_mut(&id)
        .unwrap()
        .program_registrations
        .clear();
    assert!(matches!(
        prepare(f, &id),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(physical(f, &id), before);
    assert_eq!(f.trace.lock().unwrap().len(), start);
}
#[test]
fn memory_fresh_owner_route_registration_checks_precede_authorization() {
    prerequisites(&memory());
}
#[test]
fn sqlite_fresh_owner_route_registration_checks_precede_authorization() {
    let dir = OwnedDirectory::create();
    let store = dir.store("rwc");
    prerequisites(&fixture(store.clone(), store));
}

fn physical_max_plus_one(f: &Fixture) {
    let id = submit(f, ONE);
    // Deliberately orphaned source rows are negative inputs, never admitted Jobs.
    // 4094 + 1 physical jobs exhaust the declared profile before any Job clone/decode.
    f.store
        .put_provider_states_atomic(
            (2..=4095)
                .map(|n| ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "jes-job".into(),
                        key: format!("JOB{n:05}"),
                        version: 1,
                        payload: vec![0],
                    },
                    expected_version: None,
                })
                .collect(),
        )
        .unwrap();
    let before = f.store.list_provider_state("jes-job", 4096).unwrap();
    let audits = f
        .store
        .audit_records(&invocation().execution_id, 0, 4096)
        .unwrap();
    let trace = f.trace.lock().unwrap().clone();
    assert!(matches!(
        prepare(f, &id),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(
        f.store.list_provider_state("jes-job", 4096).unwrap(),
        before
    );
    assert_eq!(
        f.store
            .audit_records(&invocation().execution_id, 0, 4096)
            .unwrap(),
        audits
    );
    assert_eq!(*f.trace.lock().unwrap(), trace);
    assert_eq!(f.batch.get(&id).unwrap().state, JobState::Queued);
}
#[test]
fn memory_physical_namespace_max_plus_one_fails_before_cached_quota_or_authorization() {
    physical_max_plus_one(&memory());
}
#[test]
fn sqlite_physical_namespace_max_plus_one_fails_before_cached_quota_or_authorization() {
    let dir = OwnedDirectory::create();
    let store = dir.store("rwc");
    physical_max_plus_one(&fixture(store.clone(), store));
}

fn events(f: &Fixture) {
    let id = submit(f, ONE);
    // Actual current Job with a valid but exhausted event budget; only the
    // independent fixture setup changes its row, never the plan under test.
    let mut state = f.batch.state.lock().unwrap();
    let job = state.jobs.get_mut(&id).unwrap();
    job.events
        .resize(f.batch.limits.max_events - 3, "fixture-event".into());
    job.version = 2;
    f.batch.persist_job(job, Some(1)).unwrap();
    drop(state);
    let before = physical(f, &id);
    assert!(matches!(
        prepare(f, &id),
        Err(HostProblem::ResourceExhausted)
    ));
    let after = physical(f, &id);
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.checkpoint, before.checkpoint);
    assert_eq!(after.audits.len(), before.audits.len() + 1); // historical preflight-before-capacity order
    assert_eq!(f.batch.get(&id).unwrap().state, JobState::Queued);
}
#[test]
fn memory_exhausted_event_plan_retains_existing_preflight_audit_order_without_selection() {
    events(&memory());
}
#[test]
fn sqlite_exhausted_event_plan_retains_existing_preflight_audit_order_without_selection() {
    let dir = OwnedDirectory::create();
    let store = dir.store("rwc");
    events(&fixture(store.clone(), store));
}

fn configured_dependencies(f: &Fixture) {
    let id = submit(f, ONE);
    f.batch.stop_initiator(&invocation(), "INIT0001").unwrap();
    f.batch.start_initiator(&invocation(), "INIT0001").unwrap();
    f.batch
        .install_topology(&invocation(), f.batch.topology().unwrap())
        .unwrap();
    let before = physical(f, &id);
    let plan = prepare(f, &id).unwrap();
    let scheduler = f
        .store
        .get_provider_state(SCHEDULER_STATE_NAMESPACE, CONFIGURATION_STATE_KEY)
        .unwrap()
        .unwrap();
    let topology = f
        .store
        .get_provider_state(TOPOLOGY_STATE_NAMESPACE, CONFIGURATION_STATE_KEY)
        .unwrap()
        .unwrap();
    assert_eq!(scheduler.version, 2);
    assert_eq!(topology.version, 1);
    assert_eq!(
        plan.configuration_rows(),
        [Some(&scheduler), Some(&topology)]
    );
    let after = physical(f, &id);
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.checkpoint, before.checkpoint);
    // Identical payload at a new physical metadata version is still a changed dependency.
    let mut changed = scheduler;
    changed.version = 3;
    f.store.put_provider_state(changed, Some(2)).unwrap();
    assert_eq!(
        plan.revalidate(&f.batch, &invocation()),
        Err(HostProblem::IdempotencyConflict)
    );
}
#[test]
fn memory_exact_registered_configuration_versions_are_dependencies_not_equal_bytes() {
    configured_dependencies(&memory());
}
#[test]
fn sqlite_exact_registered_configuration_versions_are_dependencies_not_equal_bytes() {
    let dir = OwnedDirectory::create();
    let store = dir.store("rwc");
    configured_dependencies(&fixture(store.clone(), store));
}
