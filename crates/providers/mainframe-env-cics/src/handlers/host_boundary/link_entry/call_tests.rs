//! Exact actual selected LINK call provenance; bounded local credit only.
// Source pins: LINK row0138 d807cf19; flow 19c888aa; rules ff633058.
use super::*;
use crate::service::tests::{invocation, register_load_program, service};
use mainframe_env_execution_api::{ArtifactRef, CapabilityId, Principal, PrincipalId, Selector};
use mainframe_env_host_api::{
    CapabilityDescriptor, HostLimits, HostProvider, RegistrySnapshot, ScopedHostService,
    SecurityDecision,
};
use mainframe_env_store::{MemoryStore, StoreLimits};
use std::sync::{Arc, Mutex, Weak};

type Inspect = dyn Fn(&CicsService, &Invocation, &EffectRequest) -> Result<BoundedPayload, HostProblem>
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
            HostRequest::Program(ProgramRequest::Link { .. }) => {
                let cics = self.cics.lock().unwrap().upgrade().unwrap();
                (self.inspect)(&cics, source, &effect).map(HostResult::Program)
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
    inspect: impl Fn(&CicsService, &Invocation, &EffectRequest) -> Result<BoundedPayload, HostProblem>
    + Send
    + Sync
    + 'static,
) -> (Arc<CicsService>, Arc<MemoryStore>, Invocation) {
    route_config(inspect, true, None)
}
fn route_config(
    inspect: impl Fn(&CicsService, &Invocation, &EffectRequest) -> Result<BoundedPayload, HostProblem>
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
                    provider_id: format!("call-proof-{name}"),
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
            b"independent-call-executable",
            0,
        );
    }
    let mut root =
        invocation().with_cancellation_probe(mainframe_env_execution_api::CancellationProbe::new());
    root.bindings.insert(
        "cics.commarea".into(),
        value("mainframe-env.cics.commarea@1", b"ROOT"),
    );
    let session = SessionId::new("CALL-SESSION", 64).unwrap();
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
fn tuple(effect: &EffectRequest) -> (&ProgramLinkSelection, &BoundedPayload) {
    let HostRequest::Program(ProgramRequest::Link {
        selection: Some(selection),
        payload,
        ..
    }) = &effect.request
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
        format!("original-cics-call-{sequence}"),
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

fn reject(cics: &CicsService, source: &Invocation, target: &Invocation, effect: &EffectRequest) {
    let before = image(cics);
    assert!(cics.attest_local_link_call(source, target, effect).is_err());
    assert_eq!(image(cics), before);
}

#[test]
fn actual_selected_call_retains_full_effect_original_outer_key_and_source_without_writes() {
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    let (cics, store, root) = route(move |cics, source, effect| {
        let (selection, payload) = tuple(effect);
        let child = target(source, selection, 1);
        let before = image(cics);
        let proof = cics.attest_local_link_call(source, &child, effect).unwrap();
        assert_eq!(proof.entry().source_invocation(), source);
        assert_eq!(proof.entry().target_invocation(), &child);
        assert_eq!(proof.entry().selection(), selection);
        assert_eq!(proof.entry().logical_level(), 2);
        assert_eq!(proof.entry().root_invocation(), source);
        assert_eq!(source.bindings["cics.commarea"].bytes(), b"ROOT");
        assert_eq!(child.bindings["cics.commarea"].bytes(), b"LINK");
        assert_eq!(proof.effect(), effect);
        assert_eq!(proof.effect().sequence, 1);
        assert_eq!(proof.outer_effect_key().as_str(), "original-cics-call-41");
        assert_ne!(
            Some(proof.outer_effect_key()),
            effect.idempotency_key.as_ref()
        );
        assert_eq!(image(cics), before);
        cics.ensure_run(&child).unwrap();
        let before = image(cics);
        cics.attest_local_link_call(source, &child, effect).unwrap();
        assert_eq!(image(cics), before);
        *capture.lock().unwrap() = Some(proof);
        Ok(payload.clone())
    });
    let rows = store.list_provider_state("cics-session", 8).unwrap();
    let response = link(&cics, &root, 41).unwrap();
    assert_eq!(response.outputs["COMMAREA"].bytes(), b"LINK");
    let proof = saved.lock().unwrap().take().unwrap();
    assert!(
        cics.attest_local_link_call(
            proof.entry().source_invocation(),
            proof.entry().target_invocation(),
            proof.effect()
        )
        .is_err()
    );
    // Known replay observes durable response only, with no callback or new loan.
    assert_eq!(link(&cics, &root, 41).unwrap(), response);
    assert!(saved.lock().unwrap().is_none());
    assert_eq!(store.list_provider_state("cics-session", 8).unwrap(), rows);
    assert!(cics.lock().unwrap().task_dispatch.claims.is_empty());
}

#[test]
fn actual_call_rejects_every_changed_effect_field_and_selected_tuple() {
    let (cics, _, root) = route(|cics, source, effect| {
        let (selection, payload) = tuple(effect);
        let child = target(source, selection, 1);
        for case in 0..15 {
            let mut changed = effect.clone();
            match case {
                0 => {
                    changed.run_unit =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
                1 => changed.sequence += 1,
                2 => changed.sequence = 0,
                3 => changed.deadline_tick -= 1,
                4 => changed.deadline_tick += 1,
                5 => changed.deadline_tick = 0,
                6 => {
                    changed.idempotency_key = Some(
                        IdempotencyKey::new("foreign-key", InvocationLimits::default()).unwrap(),
                    )
                }
                7 => changed.idempotency_key = None,
                8 => {
                    changed.request = HostRequest::Program(ProgramRequest::Link {
                        program: match &effect.request {
                            HostRequest::Program(ProgramRequest::Link { program, .. }) => {
                                program.clone()
                            }
                            _ => unreachable!(),
                        },
                        selection: Some(selection.clone()),
                        payload: value("mainframe-env.cics.commarea@1", b"CHANGED"),
                    })
                }
                _ => {
                    let HostRequest::Program(ProgramRequest::Link {
                        program,
                        selection,
                        payload,
                    }) = &mut changed.request
                    else {
                        unreachable!()
                    };
                    match case {
                        9 => *payload = value("foreign-schema@1", payload.bytes()),
                        10 => {
                            *program =
                                mainframe_env_host_api::ProgramName::new("OTHER", 64).unwrap()
                        }
                        11 => selection.as_mut().unwrap().generation += 1,
                        12 => {
                            selection.as_mut().unwrap().content_identity =
                                format!("sha256:{}", "b".repeat(64))
                        }
                        13 => {
                            selection.as_mut().unwrap().artifact =
                                ArtifactRef::new("foreign-artifact", InvocationLimits::default())
                                    .unwrap()
                        }
                        _ => *selection = None,
                    }
                }
            }
            reject(cics, source, &child, &changed);
        }
        cics.attest_local_link_call(source, &child, effect).unwrap();
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}

#[test]
fn actual_call_rejects_changed_original_source_even_with_matching_candidate_controls() {
    let (cics, _, root) = route(|cics, source, effect| {
        let (selection, payload) = tuple(effect);
        for case in 0..7 {
            let mut changed = source.clone();
            match case {
                0 => {
                    changed.bindings.insert(
                        "cics.commarea".into(),
                        value("mainframe-env.cics.commarea@1", b"LINK"),
                    );
                }
                1 => {
                    changed
                        .bindings
                        .insert("other.binding".into(), value("foreign@1", b"foreign"));
                }
                2 => {
                    changed.execution_id =
                        ExecutionId::new("foreign-source", InvocationLimits::default()).unwrap()
                }
                3 => changed.attempt += 1,
                4 => changed.deadline_tick -= 1,
                5 => changed.audit_correlation = "foreign-audit".into(),
                _ => {
                    changed.principal = Principal::new(
                        PrincipalId::new("FOREIGN", InvocationLimits::default()).unwrap(),
                        source.principal.grants().clone(),
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
            }
            let child = target(&changed, selection, 1);
            reject(cics, &changed, &child, effect);
        }
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}

#[test]
fn manually_selected_entry_keeps_old_proof_but_cannot_mint_call_provenance() {
    let cics = service(Arc::new(MemoryStore::new(Default::default())));
    let root = invocation();
    let session = SessionId::new("MANUAL-CALL", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(root.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let selection = ProgramLinkSelection {
        artifact: ArtifactRef::new("manual-artifact", InvocationLimits::default()).unwrap(),
        generation: 7,
        content_identity: format!("sha256:{}", "a".repeat(64)),
    };
    let child = target(&root, &selection, 1);
    let mut command =
        CommandLease::acquire_with_origin(&cics, &root.run_unit_id, Some(CicsOperation::Link))
            .unwrap();
    command.outer_effect_key = Some("manual-outer".into());
    command.current_program.program_occurrence = 1;
    let loan = ProgramLease::acquire_selected(
        &cics,
        &mut command,
        "CHILD",
        Some(selection.artifact.clone()),
        Some(selection.clone()),
    )
    .unwrap();
    let effect = EffectRequest {
        run_unit: root.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: root.deadline_tick,
        idempotency_key: Some(
            IdempotencyKey::new("manual-effect", InvocationLimits::default()).unwrap(),
        ),
        request: HostRequest::Program(ProgramRequest::Link {
            program: mainframe_env_host_api::ProgramName::new("CHILD", 64).unwrap(),
            selection: Some(selection.clone()),
            payload: value("mainframe-env.cics.commarea@1", b"LINK"),
        }),
    };
    cics.attest_local_link_entry(&root, &child, &selection)
        .unwrap();
    reject(&cics, &root, &child, &effect);
    drop(loan);
    command.finish().unwrap();
    assert!(cics.attest_local_link_call(&root, &child, &effect).is_err());
}

#[test]
fn nested_actual_calls_keep_each_original_key_and_only_current_thread_top_loan() {
    let (cics, _, root) = route(|cics, source, effect| {
        let (selection, payload) = tuple(effect);
        let level = cics.lock().unwrap().runs[&source.run_unit_id]
            .current_program
            .logical_level;
        let child = target(source, selection, level);
        let proof = cics.attest_local_link_call(source, &child, effect).unwrap();
        let expected = if level == 2 {
            "original-cics-call-1"
        } else {
            "original-cics-call-2"
        };
        assert_eq!(proof.outer_effect_key().as_str(), expected);
        assert_eq!(proof.effect().sequence, 1);
        std::thread::scope(|scope| {
            assert!(
                scope
                    .spawn(|| cics.attest_local_link_call(source, &child, effect).is_err())
                    .join()
                    .unwrap()
            );
        });
        if level == 2 {
            cics.ensure_run(&child).unwrap();
            assert_eq!(link(cics, &child, 2).unwrap().response, 0);
            let after = cics.attest_local_link_call(source, &child, effect).unwrap();
            assert_eq!(after.outer_effect_key(), proof.outer_effect_key());
            assert_eq!(after.effect(), proof.effect());
        } else {
            assert_eq!(level, 3);
            let state = cics.lock().unwrap();
            let ancestor = &state.task_dispatch.claims[&source.run_unit_id].loans[0];
            let ancestor_source = ancestor.parent.clone();
            let ancestor_target = ancestor.actor.clone().unwrap();
            let ancestor_effect = ancestor
                .entry
                .as_ref()
                .unwrap()
                .call
                .as_ref()
                .unwrap()
                .effect
                .clone();
            drop(state);
            reject(cics, &ancestor_source, &ancestor_target, &ancestor_effect);
        }
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
    assert!(cics.lock().unwrap().task_dispatch.claims.is_empty());
}

struct CallClock(std::sync::atomic::AtomicU64);
impl CicsReplayClock for CallClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}
#[test]
fn actual_call_expiry_and_cancellation_reject_without_mutation() {
    let clock = Arc::new(CallClock(std::sync::atomic::AtomicU64::new(10)));
    let control = clock.clone();
    let (cics, _, root) = route_config(
        move |cics, source, effect| {
            let (selection, payload) = tuple(effect);
            let child = target(source, selection, 1);
            cics.attest_local_link_call(source, &child, effect).unwrap();
            control
                .0
                .store(child.deadline_tick, std::sync::atomic::Ordering::SeqCst);
            let before = image(cics);
            assert!(matches!(
                cics.attest_local_link_call(source, &child, effect),
                Err(HostProblem::TimedOut)
            ));
            assert_eq!(image(cics), before);
            control.0.store(10, std::sync::atomic::Ordering::SeqCst);
            source.cancellation_probe.as_ref().unwrap().request();
            let before = image(cics);
            assert!(matches!(
                cics.attest_local_link_call(source, &child, effect),
                Err(HostProblem::Cancelled)
            ));
            assert_eq!(image(cics), before);
            Ok(payload.clone())
        },
        true,
        Some(clock),
    );
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}

#[test]
fn actual_call_unknown_exit_and_cold_provider_cannot_reconstruct_provenance() {
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    let (cics, store, root) = route(move |cics, source, effect| {
        let (selection, _) = tuple(effect);
        let child = target(source, selection, 1);
        let proof = cics.attest_local_link_call(source, &child, effect).unwrap();
        *capture.lock().unwrap() = Some(proof);
        Err(HostProblem::UnknownOutcome)
    });
    assert_eq!(link(&cics, &root, 1), Err(HostProblem::UnknownOutcome));
    let proof = saved.lock().unwrap().take().unwrap();
    assert!(matches!(
        cics.attest_local_link_call(
            proof.entry().source_invocation(),
            proof.entry().target_invocation(),
            proof.effect()
        ),
        Err(HostProblem::UnknownOutcome)
    ));
    let rows = store.list_provider_state("cics-session", 8).unwrap();
    let cold = CicsService::open(cics.host.clone(), store.clone(), CicsLimits::default()).unwrap();
    assert!(
        cold.attest_local_link_call(
            proof.entry().source_invocation(),
            proof.entry().target_invocation(),
            proof.effect()
        )
        .is_err()
    );
    assert_eq!(store.list_provider_state("cics-session", 8).unwrap(), rows);
    assert!(cold.lock().unwrap().task_dispatch.claims.is_empty());
}

#[test]
fn recorded_call_requires_positive_bound_occurrence() {
    let (cics, _, root) = route(|cics, source, effect| {
        let (selection, payload) = tuple(effect);
        let child = target(source, selection, 1);
        for occurrence in [0, 2, source.limits.max_effects + 1] {
            {
                let mut state = cics.lock().unwrap();
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
                    .call
                    .as_mut()
                    .unwrap()
                    .occurrence = occurrence;
            }
            reject(cics, source, &child, effect);
        }
        {
            let mut state = cics.lock().unwrap();
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
                .call
                .as_mut()
                .unwrap()
                .occurrence = effect.sequence;
        }
        cics.attest_local_link_call(source, &child, effect).unwrap();
        Ok(payload.clone())
    });
    assert_eq!(link(&cics, &root, 1).unwrap().response, 0);
}
