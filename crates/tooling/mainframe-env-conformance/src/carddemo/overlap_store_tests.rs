//! Synthetic admission and genuine MemoryStore-forwarding controls; no corpus credit.
use super::*;

fn fixture(batch: bool, ordinal: usize, effect_sequence: u64) -> Committed {
    let limits = InvocationLimits::default();
    let execution_id = ExecutionId::new(
        if batch {
            "batch-execution"
        } else {
            "online-execution"
        },
        limits,
    )
    .unwrap();
    let run_unit_id =
        RunUnitId::new(if batch { "batch-run" } else { "online-run" }, limits).unwrap();
    let execution = ExecutionRecord {
        execution_id: execution_id.clone(),
        run_unit_id: run_unit_id.clone(),
        selector: Selector::new(
            if batch {
                "program:CBCUS01C"
            } else {
                "program:COACTVWC"
            },
            limits,
        )
        .unwrap(),
        artifact: ArtifactRef::new(
            if batch {
                "artifact:compiled-batch"
            } else {
                "artifact:compiled-online"
            },
            limits,
        )
        .unwrap(),
        principal: PrincipalId::new(if batch { "IBMUSER" } else { "WEBUSER" }, limits).unwrap(),
        state: ExecutionState::Running,
        attempt: 1,
        version: ordinal as u64 + 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    Committed {
        ordinal,
        event: LifecycleEvent {
            execution_id: execution_id.clone(),
            run_unit_id: run_unit_id.clone(),
            sequence: effect_sequence * 2 + 3,
            attempt: 1,
            tick: 10,
            kind: LifecycleEventKind::EffectResult {
                sequence: effect_sequence,
            },
        },
        execution,
        effect: Some(EffectRecord {
            execution_id: execution_id.clone(),
            run_unit_id,
            sequence: effect_sequence,
            key: IdempotencyKey::new(format!("{execution_id}-{effect_sequence}"), limits).unwrap(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: [1; 32],
            intent: EffectIntentMetadata {
                owner: execution_id,
                attempt: 1,
                capability: Some(
                    CapabilityId::new(
                        if batch {
                            "host.dataset.read"
                        } else {
                            "host.cics.execute"
                        },
                        limits,
                    )
                    .unwrap(),
                ),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 5,
                recovery_after_tick: 5,
                epoch: 1,
                recovery_lease: None,
            },
            state: EffectState::Completed,
            result_digest: Some([2; 32]),
            resolved_tick: Some(10),
        }),
    }
}

fn campaign() -> Capture {
    Capture {
        armed: true,
        incomplete: false,
        commits: vec![
            fixture(true, 1, 1),
            fixture(false, 2, 1),
            fixture(true, 3, 2),
        ],
    }
}

fn check(capture: &Capture) -> Result<OverlapWitness, CorpusProblem> {
    let limits = InvocationLimits::default();
    prove(
        capture,
        &ArtifactRef::new("artifact:compiled-batch", limits).unwrap(),
        &ArtifactRef::new("artifact:compiled-online", limits).unwrap(),
        &RunUnitId::new("online-run", limits).unwrap(),
    )
}

#[test]
fn ordered_completed_effects_produce_only_a_scoped_witness() {
    let witness = check(&campaign()).unwrap();
    assert_eq!(witness.ordinals, [1, 2, 3]);
    assert_eq!(witness.batch_effect_sequences, [1, 2]);
    assert_eq!(witness.online_effect_sequence, 1);
    assert_eq!(witness.batch_execution, "batch-execution");
    assert_eq!(witness.batch_run, "batch-run");
    assert_eq!(witness.online_execution, "online-execution");
    assert_eq!(witness.online_run, "online-run");
}

#[test]
fn missing_online_or_either_batch_side_refuses() {
    for omitted in 0..3 {
        let mut capture = campaign();
        capture.commits.remove(omitted);
        for (index, commit) in capture.commits.iter_mut().enumerate() {
            commit.ordinal = index + 1;
        }
        assert!(check(&capture).is_err());
    }
}

#[test]
fn sequential_or_reordered_records_refuse() {
    for order in [[0, 2, 1], [1, 0, 2]] {
        let original = campaign();
        let mut capture = campaign();
        capture.commits = order
            .into_iter()
            .enumerate()
            .map(|(index, selected)| {
                let mut commit = original.commits[selected].clone();
                commit.ordinal = index + 1;
                commit
            })
            .collect();
        assert!(check(&capture).is_err());
    }
    let mut capture = campaign();
    capture.commits.swap(0, 1);
    assert!(check(&capture).is_err());
}

#[test]
fn admission_and_active_job_placeholders_never_replace_effects() {
    for phase in [
        LifecycleEventKind::Admitted,
        LifecycleEventKind::Queued,
        LifecycleEventKind::Started,
    ] {
        let mut capture = campaign();
        capture.commits[0].event.kind = phase;
        assert!(check(&capture).is_err());
    }
}

#[test]
fn incomplete_unarmed_overflow_and_unknown_records_refuse() {
    let mut capture = campaign();
    capture.incomplete = true;
    assert!(check(&capture).is_err());
    let mut capture = campaign();
    capture.armed = false;
    assert!(check(&capture).is_err());
    let mut capture = campaign();
    capture.commits = (0..=MAX_COMMITS)
        .map(|index| fixture(true, index + 1, index as u64 + 1))
        .collect();
    assert!(check(&capture).is_err());
    for state in [
        EffectState::Intent,
        EffectState::UnknownOutcome,
        EffectState::Failed,
    ] {
        let mut capture = campaign();
        capture.commits[1].effect.as_mut().unwrap().state = state;
        assert!(check(&capture).is_err());
    }
}

#[test]
fn changed_run_attempt_artifact_principal_or_capability_refuses() {
    let limits = InvocationLimits::default();
    for changed in 0..6 {
        let mut capture = campaign();
        match changed {
            0 => {
                capture.commits[2].execution.run_unit_id =
                    RunUnitId::new("other-run", limits).unwrap()
            }
            1 => capture.commits[2].execution.attempt = 2,
            2 => {
                capture.commits[2].execution.artifact =
                    ArtifactRef::new("artifact:other", limits).unwrap()
            }
            3 => {
                capture.commits[1].execution.principal = PrincipalId::new("OTHER", limits).unwrap()
            }
            4 => {
                capture.commits[1]
                    .effect
                    .as_mut()
                    .unwrap()
                    .intent
                    .capability = Some(CapabilityId::new("host.dataset.read", limits).unwrap())
            }
            _ => {
                capture.commits[2].execution.selector =
                    Selector::new("program:OTHER", limits).unwrap()
            }
        }
        assert!(check(&capture).is_err(), "changed identity field {changed}");
    }
}

#[test]
fn duplicate_or_reversed_effect_and_event_sequences_refuse() {
    let mut capture = campaign();
    capture.commits[2].event.sequence = capture.commits[0].event.sequence;
    assert!(check(&capture).is_err());
    let mut capture = campaign();
    capture.commits[2].effect.as_mut().unwrap().sequence = 1;
    capture.commits[2].event.kind = LifecycleEventKind::EffectResult { sequence: 1 };
    assert!(check(&capture).is_err());
}

#[test]
fn intervening_suspend_resume_handoff_or_terminal_breaks_one_attempt() {
    for phase in [
        LifecycleEventKind::Suspended,
        LifecycleEventKind::Resumed,
        LifecycleEventKind::HandoffCompleted,
        LifecycleEventKind::Completed { return_code: 0 },
    ] {
        let mut capture = campaign();
        let mut boundary = fixture(true, 2, 1);
        boundary.effect = None;
        boundary.event.kind = phase.clone();
        if matches!(phase, LifecycleEventKind::Completed { .. }) {
            boundary.execution.state = ExecutionState::Completed;
        }
        capture.commits.insert(1, boundary);
        for (index, commit) in capture.commits.iter_mut().enumerate() {
            commit.ordinal = index + 1;
        }
        assert!(check(&capture).is_err());
    }
}

#[test]
fn legacy_missing_result_resolution_or_wrong_effect_owner_refuses() {
    for changed in 0..5 {
        let mut capture = campaign();
        let effect = capture.commits[1].effect.as_mut().unwrap();
        match changed {
            0 => effect.digest_format = EffectDigestFormat::LegacyDebug,
            1 => effect.result_digest = None,
            2 => effect.resolved_tick = None,
            3 => {
                effect.intent.owner =
                    ExecutionId::new("wrong-owner", InvocationLimits::default()).unwrap()
            }
            _ => effect.intent.attempt = 2,
        }
        assert!(check(&capture).is_err());
    }
}

fn notification(event: &LifecycleEvent) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}:{}", event.execution_id, event.sequence),
        execution_id: event.execution_id.clone(),
        sequence: event.sequence,
        topic: "execution.lifecycle.v1".into(),
        payload: mainframe_env_execution_api::lifecycle_notification_payload(&event.kind),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    }
}

fn queued(inner: &MemoryStore, identity: &str) -> (ExecutionRecord, LifecycleEvent) {
    let mut execution = fixture(true, 1, 1).execution;
    execution.execution_id = ExecutionId::new(identity, InvocationLimits::default()).unwrap();
    execution.state = ExecutionState::Admitted;
    execution.version = 1;
    let admitted = LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence: 1,
        attempt: 1,
        tick: 1,
        kind: LifecycleEventKind::Admitted,
    };
    inner
        .admit_execution(execution.clone(), admitted.clone(), notification(&admitted))
        .unwrap();
    let queued = LifecycleEvent {
        sequence: 2,
        kind: LifecycleEventKind::Queued,
        ..admitted
    };
    (execution, queued)
}

#[test]
fn real_forwarded_commit_and_return_are_equal_with_capture_incomplete() {
    let inner = Arc::new(MemoryStore::new(Default::default()));
    let (execution, event) = queued(&inner, "forwarded");
    let observed = ObservedStore::new(inner.clone());
    observed.arm().unwrap();
    observed.capture.lock().unwrap().incomplete = true;
    let returned = observed
        .commit_execution_step(
            &execution.execution_id,
            1,
            Some(ExecutionState::Queued),
            event.clone(),
            None,
            None,
            None,
            notification(&event),
        )
        .unwrap();
    assert_eq!(
        returned,
        inner
            .get_execution(&execution.execution_id)
            .unwrap()
            .unwrap()
    );
    assert_eq!(returned.state, ExecutionState::Queued);
    assert_eq!(returned.version, 2);
    assert_eq!(
        inner.events(&execution.execution_id, 1, 4).unwrap(),
        vec![
            LifecycleEvent {
                sequence: 1,
                kind: LifecycleEventKind::Admitted,
                ..event.clone()
            },
            event
        ]
    );
    assert!(
        observed
            .prove_overlap(
                &execution.artifact,
                &ArtifactRef::new("artifact:compiled-online", InvocationLimits::default()).unwrap(),
                &RunUnitId::new("online-run", InvocationLimits::default()).unwrap()
            )
            .is_err()
    );
}

#[test]
fn backend_conflict_is_unchanged_and_prevents_capture_proof() {
    let inner = Arc::new(MemoryStore::new(Default::default()));
    let (execution, event) = queued(&inner, "conflict");
    let observed = ObservedStore::new(inner.clone());
    observed.arm().unwrap();
    let expected = inner.commit_execution_step(
        &execution.execution_id,
        99,
        Some(ExecutionState::Queued),
        event.clone(),
        None,
        None,
        None,
        notification(&event),
    );
    let actual = observed.commit_execution_step(
        &execution.execution_id,
        99,
        Some(ExecutionState::Queued),
        event.clone(),
        None,
        None,
        None,
        notification(&event),
    );
    assert_eq!(actual, expected);
    assert!(observed.capture.lock().unwrap().incomplete);
    assert!(observed.capture.lock().unwrap().commits.is_empty());
    assert_eq!(
        inner
            .get_execution(&execution.execution_id)
            .unwrap()
            .unwrap(),
        execution
    );
}

#[test]
fn observer_lock_precedes_real_commit_so_postreturn_reordering_is_impossible() {
    let inner = Arc::new(MemoryStore::new(Default::default()));
    let (execution, event) = queued(&inner, "ordering");
    let observed = Arc::new(ObservedStore::new(inner.clone()));
    observed.arm().unwrap();
    let held = observed.capture.lock().unwrap();
    let (entered, entry) = std::sync::mpsc::channel();
    let (finished, finish) = std::sync::mpsc::channel();
    let store = observed.clone();
    let id = execution.execution_id.clone();
    let handle = std::thread::spawn(move || {
        entered.send(()).unwrap();
        let result = store.commit_execution_step(
            &id,
            1,
            Some(ExecutionState::Queued),
            event.clone(),
            None,
            None,
            None,
            notification(&event),
        );
        finished.send(result).unwrap();
    });
    entry
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(matches!(
        finish.recv_timeout(std::time::Duration::from_millis(20)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        inner
            .get_execution(&execution.execution_id)
            .unwrap()
            .unwrap(),
        execution
    );
    drop(held);
    let actual = finish
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    handle.join().unwrap();
    let capture = observed.capture.lock().unwrap();
    assert_eq!(capture.commits.len(), 1);
    assert_eq!(capture.commits[0].execution, actual);
    assert_eq!(capture.commits[0].ordinal, 1);
}

#[test]
fn actual_unobserved_suspend_and_resume_preserve_results_but_refuse_proof() {
    let inner = Arc::new(MemoryStore::new(Default::default()));
    let (execution, _) = queued(&inner, "batch-execution");
    let queued = inner
        .transition_execution(&execution.execution_id, 1, ExecutionState::Queued, 1)
        .unwrap();
    let running = inner
        .transition_execution(
            &execution.execution_id,
            queued.version,
            ExecutionState::Running,
            2,
        )
        .unwrap();
    let observed = ObservedStore::new(inner.clone());
    observed.arm().unwrap();
    observed.capture.lock().unwrap().commits = campaign().commits;
    assert!(check(&observed.capture.lock().unwrap()).is_ok());
    let suspended = observed
        .transition_execution(
            &execution.execution_id,
            running.version,
            ExecutionState::Suspended,
            3,
        )
        .unwrap();
    assert_eq!(
        suspended,
        inner
            .get_execution(&execution.execution_id)
            .unwrap()
            .unwrap()
    );
    let queued = observed
        .transition_execution(
            &execution.execution_id,
            suspended.version,
            ExecutionState::Queued,
            4,
        )
        .unwrap();
    let resumed = observed
        .transition_execution(
            &execution.execution_id,
            queued.version,
            ExecutionState::Running,
            5,
        )
        .unwrap();
    assert_eq!(
        resumed,
        inner
            .get_execution(&execution.execution_id)
            .unwrap()
            .unwrap()
    );
    assert!(observed.capture.lock().unwrap().incomplete);
    assert!(check(&observed.capture.lock().unwrap()).is_err());
}
