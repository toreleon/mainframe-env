//! Independent live-boundary proofs; no installed storage or recovery credit.
// LINK row0138 d807cf19..., calling flow 19c888aa... lines 10–20 and
// rules ff633058... lines 93–109 supply the level/entry distinction.
use super::*;
use crate::service::tests::{invocation, register_load_program, service};
use mainframe_env_execution_api::{ArtifactRef, CapabilityId, Principal, PrincipalId, Selector};
use mainframe_env_host_api::{
    CapabilityDescriptor, HostLimits, HostProvider, RegistrySnapshot, ScopedHostService,
    SecurityDecision,
};
use mainframe_env_store::{MemoryStore, StoreLimits};
use std::sync::{Arc, Mutex, Weak};

type Inspect = dyn Fn(&CicsService, &Invocation, &ProgramRequest) -> Result<BoundedPayload, HostProblem>
    + Send
    + Sync;
struct InspectProvider {
    descriptor: CapabilityDescriptor,
    cics: Arc<Mutex<Weak<CicsService>>>,
    inspect: Arc<Inspect>,
}
impl HostProvider for InspectProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, source: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match &effect.request {
            HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
            HostRequest::Program(request @ ProgramRequest::Link { .. }) => {
                let cics = self.cics.lock().unwrap().upgrade().unwrap();
                (self.inspect)(&cics, source, request).map(HostResult::Program)
            }
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}
fn route(
    inspect: impl Fn(&CicsService, &Invocation, &ProgramRequest) -> Result<BoundedPayload, HostProblem>
    + Send
    + Sync
    + 'static,
) -> (Arc<CicsService>, Arc<MemoryStore>, Invocation) {
    route_config(inspect, true, None)
}
fn route_config(
    inspect: impl Fn(&CicsService, &Invocation, &ProgramRequest) -> Result<BoundedPayload, HostProblem>
    + Send
    + Sync
    + 'static,
    selected: bool,
    clock: Option<Arc<dyn CicsReplayClock>>,
) -> (Arc<CicsService>, Arc<MemoryStore>, Invocation) {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let slot = Arc::new(Mutex::new(Weak::new()));
    let inspect: Arc<Inspect> = Arc::new(inspect);
    let providers = ["host.security.authorize", "host.program.invoke"]
        .into_iter()
        .map(|name| {
            Arc::new(InspectProvider {
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(name, InvocationLimits::default()).unwrap(),
                    provider_id: format!("entry-proof-{name}"),
                    generation: "1".into(),
                    request_schema: "request@1".into(),
                    result_schema: "result@1".into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 4 * 1024 * 1024,
                    ready: true,
                },
                cics: slot.clone(),
                inspect: inspect.clone(),
            }) as Arc<dyn HostProvider>
        })
        .collect();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    let cics = match clock {
        Some(clock) => {
            CicsService::open_with_replay_clock(host, store.clone(), CicsLimits::default(), clock)
        }
        None => CicsService::open(host, store.clone(), CicsLimits::default()),
    }
    .unwrap();
    *slot.lock().unwrap() = Arc::downgrade(&cics);
    cics.bind_artifact_store(store.clone()).unwrap();
    if selected {
        register_load_program(
            &cics,
            store.as_ref(),
            "CHILD",
            7,
            b"independent-entry-executable",
            0,
        );
    }
    let mut root =
        invocation().with_cancellation_probe(mainframe_env_execution_api::CancellationProbe::new());
    root.bindings.insert(
        "cics.commarea".into(),
        value("mainframe-env.cics.commarea@1", b"ROOT"),
    );
    let session = SessionId::new("ENTRY-SESSION", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(root.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    (cics, store, root)
}
fn value(schema: &str, bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
}
fn target(source: &Invocation, selection: &ProgramLinkSelection, ordinal: u32) -> Invocation {
    // A test candidate, not the manager's deterministic target constructor.
    let mut target = source.clone();
    target.parent_execution_id = Some(source.execution_id.clone());
    target.execution_id = ExecutionId::new(
        format!("entry-candidate-{ordinal}"),
        InvocationLimits::default(),
    )
    .unwrap();
    target.selector = Selector::new("program:CHILD", InvocationLimits::default()).unwrap();
    target.artifact = selection.artifact.clone();
    target.bindings.insert(
        "cics.commarea".into(),
        value("mainframe-env.cics.commarea@1", b"LINK"),
    );
    target
}
fn tuple(request: &ProgramRequest) -> (&ProgramLinkSelection, &BoundedPayload) {
    let ProgramRequest::Link {
        selection: Some(selection),
        payload,
        ..
    } = request
    else {
        panic!("expected actual selected LINK");
    };
    (selection, payload)
}
fn link(
    cics: &CicsService,
    actor: &Invocation,
    sequence: u64,
) -> Result<CicsResponse, HostProblem> {
    let key = IdempotencyKey::new(
        format!("independent-entry-{sequence}"),
        InvocationLimits::default(),
    )
    .unwrap();
    let request = CicsRequest {
        operation: CicsOperation::Link,
        arguments: BTreeMap::from([
            (
                "PROGRAM".into(),
                value("mainframe-env.cics.literal@1", b"child"),
            ),
            (
                "COMMAREA".into(),
                value("mainframe-env.cics.argument@1", b"LINK"),
            ),
            ("LENGTH".into(), value("mainframe-env.cics.decimal@1", b"4")),
        ]),
        condition_policy: mainframe_env_host_api::CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: None,
        },
        mutation: Some(Mutation {
            sequence,
            idempotency_key: key.clone(),
            transaction: Some("MENU".into()),
        }),
    };
    let effect = EffectRequest {
        sequence,
        run_unit: actor.run_unit_id.clone(),
        idempotency_key: Some(key),
        deadline_tick: actor.deadline_tick,
        request: HostRequest::Cics(request.clone()),
    };
    cics.invoke(&effect, request)
}
fn rejected(
    cics: &CicsService,
    source: &Invocation,
    target: &Invocation,
    selection: &ProgramLinkSelection,
) {
    assert!(
        cics.attest_local_link_entry(source, target, selection)
            .is_err()
    );
}
#[test]
fn absent_manual_and_unselected_loans_never_attest() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store);
    let root = invocation();
    let session = SessionId::new("MANUAL-ENTRY", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(root.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let selected = ProgramLinkSelection {
        artifact: ArtifactRef::new("manual-artifact", InvocationLimits::default()).unwrap(),
        generation: 7,
        content_identity: format!("sha256:{}", "a".repeat(64)),
    };
    let child = target(&root, &selected, 1);
    rejected(&cics, &root, &child, &selected);
    let mut command = CommandLease::acquire(&cics, &root.run_unit_id).unwrap();
    let loan = ProgramLease::acquire(
        &cics,
        &mut command,
        "CHILD",
        Some(selected.artifact.clone()),
    )
    .unwrap();
    rejected(&cics, &root, &child, &selected);
    loan.finish().unwrap();
    command.finish().unwrap();
    // Even a typed LINK command cannot elevate an unselected manual program loan.
    let mut command =
        CommandLease::acquire_with_origin(&cics, &root.run_unit_id, Some(CicsOperation::Link))
            .unwrap();
    let loan = ProgramLease::acquire(
        &cics,
        &mut command,
        "CHILD",
        Some(selected.artifact.clone()),
    )
    .unwrap();
    rejected(&cics, &root, &child, &selected);
    loan.finish().unwrap();
    command.finish().unwrap();
    rejected(&cics, &root, &child, &selected);
}
#[test]
fn production_entry_is_read_only_before_and_after_exact_actor_admission() {
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    let (cics, store, root) = route(move |cics, source, request| {
        let (selection, payload) = tuple(request);
        let child = target(source, selection, 1);
        let before = image(cics);
        let token = cics
            .attest_local_link_entry(source, &child, selection)
            .unwrap();
        assert_eq!(token.source_invocation(), source);
        assert_eq!(token.target_invocation(), &child);
        assert_eq!(token.selection(), selection);
        assert_eq!(token.logical_level(), 2);
        assert_eq!(token.root_invocation().execution_id, source.execution_id);
        assert_eq!(source.bindings["cics.commarea"].bytes(), b"ROOT");
        assert_eq!(image(cics), before);
        cics.ensure_run(&child).unwrap();
        let before = image(cics);
        cics.attest_local_link_entry(source, &child, selection)
            .unwrap();
        assert_eq!(image(cics), before);
        let mut foreign = child.clone();
        foreign.execution_id = ExecutionId::new("foreign", InvocationLimits::default()).unwrap();
        rejected(cics, source, &foreign, selection);
        *capture.lock().unwrap() = Some((source.clone(), child, selection.clone()));
        Ok(payload.clone())
    });
    let rows = store.list_provider_state("cics-session", 8).unwrap();
    assert_eq!(
        link(&cics, &root, 1).unwrap().outputs["COMMAREA"].bytes(),
        b"LINK"
    );
    let (source, child, selection) = saved.lock().unwrap().clone().unwrap();
    rejected(&cics, &source, &child, &selection);
    assert_eq!(store.list_provider_state("cics-session", 8).unwrap(), rows);
    assert!(cics.lock().unwrap().task_dispatch.claims.is_empty());
}
fn image(cics: &CicsService) -> String {
    let state = cics.lock().unwrap();
    let claim = &state.task_dispatch.claims.values().next().unwrap();
    let task = state.runs.values().next().unwrap();
    let rows = [
        "cics-session",
        "cics-program-definition-v1",
        "cics-effect-replay-v1",
    ]
    .map(|namespace| cics.store.list_provider_state(namespace, 32).unwrap());
    let epoch = cics.store.provider_state_retention_epoch().unwrap();
    let frame = &task.current_program;
    let frame_image = format!(
        "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}",
        frame.program_occurrence,
        frame.invoking_program,
        frame.return_program,
        frame.channel,
        frame.parent_execution_id,
        frame.initial_entry
    );
    format!(
        "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}",
        claim.thread,
        claim.session,
        claim.commands,
        claim.command_origin,
        claim
            .loans
            .iter()
            .map(|loan| (
                &loan.parent,
                &loan.program,
                &loan.artifact,
                &loan.actor,
                &loan.entry,
                &loan.handles
            ))
            .collect::<Vec<_>>(),
        task.invocation,
        task.current_program.effect_invocation,
        task.current_program.logical_level,
        task.current_records,
        task.handlers,
        state.task_dispatch.uncertain_sessions,
        frame_image,
        rows,
        epoch
    )
}
#[test]
fn production_entry_rejects_forged_source_selection_and_control_envelopes() {
    let (cics, _, root) = route(|cics, source, request| {
        let (selection, payload) = tuple(request);
        let child = target(source, selection, 1);
        let before = image(cics);
        let mut wrong_source = source.clone();
        wrong_source.bindings.remove("cics.commarea");
        rejected(cics, &wrong_source, &child, selection);
        for case in 0..4 {
            let mut wrong = selection.clone();
            match case {
                0 => wrong.generation += 1,
                1 => wrong.content_identity = format!("sha256:{}", "b".repeat(64)),
                2 => {
                    wrong.artifact =
                        ArtifactRef::new("wrong-artifact", InvocationLimits::default()).unwrap()
                }
                _ => wrong.generation = 0,
            }
            rejected(cics, source, &child, &wrong);
        }
        for case in 0..29 {
            let mut bad = child.clone();
            match case {
                0 => bad.parent_execution_id = None,
                1 => {
                    bad.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
                2 => {
                    bad.principal = Principal::new(
                        PrincipalId::new("FOREIGN", InvocationLimits::default()).unwrap(),
                        source.principal.grants().clone(),
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
                3 => {
                    bad.selector =
                        Selector::new("program:OTHER", InvocationLimits::default()).unwrap()
                }
                4 => bad.artifact = source.artifact.clone(),
                5 => bad.attempt += 1,
                6 => bad.attempt = 0,
                7 => bad.priority += 1,
                8 => bad.service_class = mainframe_env_execution_api::ServiceClass::Batch,
                9 => {
                    bad.provider_generations.insert(
                        CapabilityId::new("host.program.invoke", InvocationLimits::default())
                            .unwrap(),
                        "foreign".into(),
                    );
                }
                10 => bad.audit_correlation = "foreign".into(),
                11 => {
                    bad.cancellation_probe =
                        Some(mainframe_env_execution_api::CancellationProbe::new())
                }
                12 => bad.deadline_tick += 1,
                13 => bad.deadline_tick = 0,
                14 => bad.limits.max_frames += 1,
                15 => bad.limits.max_steps += 1,
                16 => bad.limits.max_storage_bytes += 1,
                17 => bad.limits.max_output_bytes += 1,
                18 => bad.limits.max_effects += 1,
                19 => bad.limits.max_events += 1,
                20 => bad.limits.max_steps = 0,
                21 => {
                    bad.bindings.insert(
                        "cics.execution-context".into(),
                        value(
                            "mainframe-env.cics.execution-context@1",
                            b"dpl-synconreturn",
                        ),
                    );
                }
                22 => {
                    bad.bindings.insert(
                        "cics.session".into(),
                        value("mainframe-env.cics.session@1", b"OTHER"),
                    );
                }
                23 => {
                    bad.bindings.insert(
                        "cics.program-entry".into(),
                        value("mainframe-env.cics.program-entry@1", b"initial"),
                    );
                }
                24 => bad.execution_id = source.execution_id.clone(),
                25 => {
                    bad.bindings.insert(
                        "cics.channel".into(),
                        value("mainframe-env.cics.channel@1", b"FORGED"),
                    );
                }
                26 => {
                    bad.bindings
                        .insert("cics.commarea".into(), value("wrong@1", b"LINK"));
                }
                27 => {
                    for index in 0..129 {
                        bad.bindings
                            .insert(format!("test-{index}"), bounded(Vec::new()).unwrap());
                    }
                }
                _ => {
                    bad.bindings
                        .insert("x".repeat(4097), bounded(Vec::new()).unwrap());
                }
            }
            assert!(
                cics.attest_local_link_entry(source, &bad, selection)
                    .is_err(),
                "case {case}"
            );
        }
        let mut bounded = child.clone();
        bounded.deadline_tick -= 1;
        bounded.limits.max_steps -= 1;
        cics.attest_local_link_entry(source, &bounded, selection)
            .unwrap();
        assert_eq!(image(cics), before);
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}
#[test]
fn production_entry_fences_thread_outstanding_command_depth_origin_and_uncertainty() {
    let (cics, _, root) = route(|cics, source, request| {
        let (selection, payload) = tuple(request);
        let child = target(source, selection, 1);
        let before = image(cics);
        std::thread::scope(|scope| {
            scope
                .spawn(|| rejected(cics, source, &child, selection))
                .join()
                .unwrap();
        });
        assert_eq!(image(cics), before);
        for case in 0..5 {
            {
                let mut state = cics.lock().unwrap();
                let task = state.runs.get_mut(&source.run_unit_id).unwrap();
                match case {
                    0 => task.current_program.logical_level += 1,
                    1 => {
                        task.program_abend =
                            Some(super::super::super::program_abend::PendingProgramAbend {
                                response: cics_response_stub(),
                                record: None,
                                cancel_exits: false,
                            })
                    }
                    2 => {
                        state
                            .task_dispatch
                            .uncertain_sessions
                            .insert("ENTRY-SESSION".into());
                    }
                    3 => {
                        state
                            .task_dispatch
                            .claims
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .command_origin = Some(CicsOperation::InvokeApplication)
                    }
                    _ => {
                        state
                            .task_dispatch
                            .claims
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .loans
                            .last_mut()
                            .unwrap()
                            .entry
                            .as_mut()
                            .unwrap()
                            .origin = None
                    }
                }
            }
            rejected(cics, source, &child, selection);
            {
                let mut state = cics.lock().unwrap();
                match case {
                    0 => {
                        state
                            .runs
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .current_program
                            .logical_level -= 1
                    }
                    1 => {
                        state
                            .runs
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .program_abend = None
                    }
                    2 => {
                        state.task_dispatch.uncertain_sessions.clear();
                    }
                    3 => {
                        state
                            .task_dispatch
                            .claims
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .command_origin = Some(CicsOperation::Link)
                    }
                    _ => {
                        state
                            .task_dispatch
                            .claims
                            .get_mut(&source.run_unit_id)
                            .unwrap()
                            .loans
                            .last_mut()
                            .unwrap()
                            .entry
                            .as_mut()
                            .unwrap()
                            .origin = Some(CicsOperation::Link)
                    }
                }
            }
        }
        cics.ensure_run(&child).unwrap();
        let command = CommandLease::acquire(cics, &source.run_unit_id).unwrap();
        rejected(cics, source, &child, selection);
        command.finish().unwrap();
        cics.attest_local_link_entry(source, &child, selection)
            .unwrap();
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}
fn cics_response_stub() -> CicsResponse {
    CicsResponse {
        condition: "ERROR".into(),
        response: 1,
        response2: 0,
        applid: "TEST".into(),
        sysid: "TEST".into(),
        transaction: "MENU".into(),
        aid: 0,
        disposition: CicsDisposition::Abended,
        target: None,
        next_transaction: None,
        payload: bounded(Vec::new()).unwrap(),
        outputs: BTreeMap::new(),
        unit_of_work: None,
    }
}
#[test]
fn production_nested_entries_preserve_original_source_root_and_restore_origin() {
    let (cics, _, root) = route(|cics, source, request| {
        let (selection, payload) = tuple(request);
        let level = cics.lock().unwrap().runs[&source.run_unit_id]
            .current_program
            .logical_level;
        let child = target(source, selection, level);
        let token = cics
            .attest_local_link_entry(source, &child, selection)
            .unwrap();
        assert_eq!(token.logical_level(), level);
        assert_eq!(token.root_invocation().parent_execution_id, None);
        assert_eq!(
            token.root_invocation().execution_id.as_str(),
            "execution-run"
        );
        if level == 2 {
            cics.ensure_run(&child).unwrap();
            assert_eq!(link(cics, &child, 2).unwrap().response, 0);
            cics.attest_local_link_entry(source, &child, selection)
                .unwrap();
            assert_eq!(
                cics.lock().unwrap().runs[&source.run_unit_id]
                    .current_program
                    .logical_level,
                2
            );
        } else {
            assert_eq!(level, 3);
            assert_eq!(
                source.parent_execution_id.as_ref().unwrap().as_str(),
                "execution-run"
            );
        }
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
    assert_eq!(
        cics.lock().unwrap().runs[&root.run_unit_id]
            .current_program
            .logical_level,
        1
    );
    assert!(cics.lock().unwrap().task_dispatch.claims.is_empty());
}

#[test]
fn production_unselected_link_and_selected_invoke_application_do_not_attest_link() {
    let (cics, _, root) = route_config(
        |cics, source, request| {
            let ProgramRequest::Link {
                selection: None,
                payload,
                ..
            } = request
            else {
                panic!("unselected LINK");
            };
            let selection = ProgramLinkSelection {
                artifact: ArtifactRef::new("fake", InvocationLimits::default()).unwrap(),
                generation: 7,
                content_identity: format!("sha256:{}", "a".repeat(64)),
            };
            rejected(cics, source, &target(source, &selection, 1), &selection);
            Ok(payload.clone())
        },
        false,
        None,
    );
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
    let (cics, _, root) = route(|cics, source, request| {
        let (selection, payload) = tuple(request);
        rejected(cics, source, &target(source, selection, 1), selection);
        Ok(payload.clone())
    });
    let artifact = cics.lock().unwrap().program_definitions["CHILD"][&7]
        .artifact
        .clone();
    cics.register_application_entries(&[CicsApplicationEntryDefinition {
        application: "ENTRYAPP".into(),
        platform: "LOCAL".into(),
        major_version: 1,
        minor_version: 0,
        micro_version: 0,
        operation: "RUN".into(),
        program: "CHILD".into(),
        program_generation: 7,
        program_artifact: artifact.clone(),
        application_identity: artifact.as_str().into(),
        available: true,
    }])
    .unwrap();
    let request = CicsRequest {
        operation: CicsOperation::InvokeApplication,
        arguments: BTreeMap::from([
            (
                "APPLICATION".into(),
                value("mainframe-env.cics.literal@1", b"ENTRYAPP"),
            ),
            (
                "PLATFORM".into(),
                value("mainframe-env.cics.literal@1", b"LOCAL"),
            ),
            (
                "OPERATION".into(),
                value("mainframe-env.cics.literal@1", b"RUN"),
            ),
        ]),
        condition_policy: mainframe_env_host_api::CicsConditionPolicy::NoHandle,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("entry-application", InvocationLimits::default())
                .unwrap(),
            transaction: Some("MENU".into()),
        }),
    };
    let effect = EffectRequest {
        run_unit: root.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: 100,
        idempotency_key: Some(request.mutation.as_ref().unwrap().idempotency_key.clone()),
        request: HostRequest::Cics(request.clone()),
    };
    assert_eq!(cics.invoke(&effect, request).unwrap().response, 0);
}
struct EntryClock;
impl CicsReplayClock for EntryClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(10)
    }
}
#[test]
fn production_entry_observes_deadline_and_live_cancellation_without_admission() {
    let (cics, _, root) = route_config(
        |cics, source, request| {
            let (selection, _) = tuple(request);
            let mut child = target(source, selection, 1);
            let before = image(cics);
            child.deadline_tick = 10;
            assert!(matches!(
                cics.attest_local_link_entry(source, &child, selection),
                Err(HostProblem::TimedOut)
            ));
            assert_eq!(image(cics), before);
            child.deadline_tick = 11;
            cics.attest_local_link_entry(source, &child, selection)
                .unwrap();
            source.cancellation_probe.as_ref().unwrap().request();
            let before = image(cics);
            assert!(matches!(
                cics.attest_local_link_entry(source, &child, selection),
                Err(HostProblem::Cancelled)
            ));
            assert_eq!(image(cics), before);
            Err(HostProblem::Cancelled)
        },
        true,
        Some(Arc::new(EntryClock)),
    );
    // The boundary probe returns its known cancellation result through the host.
    assert_eq!(link(&cics, &root, 1), Err(HostProblem::Cancelled));
    assert!(cics.lock().unwrap().task_dispatch.claims.is_empty());
}
#[test]
fn known_replay_never_recreates_entry_or_redispatches() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    let (cics, _, root) = route(move |cics, source, request| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (selection, payload) = tuple(request);
        let child = target(source, selection, 1);
        cics.attest_local_link_entry(source, &child, selection)
            .unwrap();
        *capture.lock().unwrap() = Some((source.clone(), child, selection.clone()));
        Ok(payload.clone())
    });
    let response = link(&cics, &root, 1).unwrap();
    assert_eq!(link(&cics, &root, 1).unwrap(), response);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (source, child, selection) = saved.lock().unwrap().clone().unwrap();
    rejected(&cics, &source, &child, &selection);
}
#[test]
fn unknown_result_fences_warm_entry_and_cold_provider_has_no_loan() {
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    let (cics, store, root) = route(move |cics, source, request| {
        let (selection, _) = tuple(request);
        let child = target(source, selection, 1);
        cics.attest_local_link_entry(source, &child, selection)
            .unwrap();
        *capture.lock().unwrap() = Some((source.clone(), child, selection.clone()));
        Err(HostProblem::UnknownOutcome)
    });
    assert_eq!(link(&cics, &root, 1), Err(HostProblem::UnknownOutcome));
    let (source, child, selection) = saved.lock().unwrap().clone().unwrap();
    assert!(matches!(
        cics.attest_local_link_entry(&source, &child, &selection),
        Err(HostProblem::UnknownOutcome)
    ));
    let rows = store.list_provider_state("cics-session", 8).unwrap();
    let cold = CicsService::open(cics.host.clone(), store.clone(), CicsLimits::default()).unwrap();
    rejected(&cold, &source, &child, &selection);
    assert_eq!(store.list_provider_state("cics-session", 8).unwrap(), rows);
    assert!(cold.lock().unwrap().task_dispatch.claims.is_empty());
}

#[test]
fn production_context_requires_unchanged_local_cics_envelope() {
    for (schema, bytes, allowed) in [
        (
            "mainframe-env.cics.execution-context@1",
            b"local".as_slice(),
            true,
        ),
        (
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".as_slice(),
            false,
        ),
        ("wrong@1", b"local".as_slice(), false),
    ] {
        let (cics, _, mut root) = route(move |cics, source, request| {
            let (selection, payload) = tuple(request);
            let before = image(cics);
            assert_eq!(
                cics.attest_local_link_entry(source, &target(source, selection, 1), selection)
                    .is_ok(),
                allowed
            );
            assert_eq!(image(cics), before);
            Ok(payload.clone())
        });
        root.bindings
            .insert("cics.execution-context".into(), value(schema, bytes));
        {
            let mut state = cics.lock().unwrap();
            let task = state.runs.get_mut(&root.run_unit_id).unwrap();
            task.invocation = root.clone();
            task.current_program.effect_invocation = root.clone();
        }
        assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
    }
}
