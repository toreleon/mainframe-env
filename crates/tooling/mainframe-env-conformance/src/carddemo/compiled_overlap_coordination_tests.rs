//! Controlled scheduling regressions only; no compiled corpus or application credit.
use super::*;
use std::time::Duration;

#[derive(Clone)]
struct Row {
    execution: ExecutionRecord,
    event: LifecycleEvent,
    effect: Option<EffectRecord>,
}
fn fixture(batch: bool, ordinal: usize, effect_sequence: u64) -> Row {
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
    Row {
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

fn coordination() -> Arc<Coordination> {
    let limits = InvocationLimits::default();
    Coordination::new(
        ArtifactRef::new("artifact:compiled-batch", limits).unwrap(),
        ArtifactRef::new("artifact:compiled-online", limits).unwrap(),
    )
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

struct Parked {
    coordination: Arc<Coordination>,
    row: Row,
    handle: Option<std::thread::JoinHandle<Result<ExecutionRecord, StoreError>>>,
}
impl Parked {
    fn join(&mut self) -> Result<ExecutionRecord, StoreError> {
        self.handle.take().unwrap().join().unwrap()
    }
}
impl Drop for Parked {
    fn drop(&mut self) {
        self.coordination.abort();
        if let Some(handle) = self.handle.take() {
            // Tests also release before join on assertion/unwind paths.
            let _ = handle.join();
        }
    }
}

fn park() -> Parked {
    let coordination = coordination();
    let deadline = Instant::now() + Duration::from_secs(120);
    coordination.arm(deadline).unwrap();
    let row = fixture(true, 1, 1);
    let offered = row.clone();
    let owner = coordination.clone();
    let handle = std::thread::spawn(move || {
        let result = Ok(offered.execution);
        owner.after_commit(&offered.event, offered.effect.as_ref(), &result);
        result
    });
    let parked = Parked {
        coordination,
        row,
        handle: Some(handle),
    };
    runtime()
        .block_on(parked.coordination.wait_for_boundary(deadline.into()))
        .unwrap();
    parked
}

fn offer(coordination: &Coordination, row: &Row) {
    coordination.after_commit(&row.event, row.effect.as_ref(), &Ok(row.execution.clone()));
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

fn seed(inner: &MemoryStore, batch: bool) -> Row {
    let mut row = fixture(batch, 1, 1);
    let mut execution = row.execution.clone();
    execution.state = ExecutionState::Admitted;
    execution.version = 1;
    let mut event = LifecycleEvent {
        sequence: 1,
        tick: 1,
        kind: LifecycleEventKind::Admitted,
        ..row.event.clone()
    };
    inner
        .admit_execution(execution.clone(), event.clone(), notification(&event))
        .unwrap();
    for (sequence, kind, state) in [
        (2, LifecycleEventKind::Queued, ExecutionState::Queued),
        (3, LifecycleEventKind::Started, ExecutionState::Running),
    ] {
        event.sequence = sequence;
        event.tick = sequence;
        event.kind = kind;
        execution = inner
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                notification(&event),
            )
            .unwrap();
    }
    let effect = row.effect.as_mut().unwrap();
    effect.intent.epoch = 4;
    effect.intent.created_tick = 4;
    effect.intent.recovery_after_tick = 4;
    effect.resolved_tick = Some(5);
    let mut intent = effect.clone();
    intent.state = EffectState::Intent;
    intent.resolved_tick = None;
    intent.result_digest = None;
    event.sequence = 4;
    event.tick = 4;
    event.kind = LifecycleEventKind::EffectIntent { sequence: 1 };
    inner
        .commit_execution_step(
            &execution.execution_id,
            3,
            None,
            event.clone(),
            Some(intent),
            None,
            None,
            notification(&event),
        )
        .unwrap();
    row.event.sequence = 5;
    row.event.tick = 5;
    row.execution.version = 5;
    row
}

fn commit(store: &CoordinatedStore, row: &Row) -> Result<ExecutionRecord, StoreError> {
    let effect = row.effect.as_ref().unwrap();
    let audit = AuditRecord {
        execution_id: row.execution.execution_id.clone(),
        run_unit_id: row.execution.run_unit_id.clone(),
        attempt: 1,
        effect_sequence: 1,
        observed_tick: 5,
        principal: row.execution.principal.clone(),
        invocation_key: effect.key.clone(),
        capability: effect.intent.capability.clone().unwrap(),
        resource: mainframe_env_execution_api::AuditResourceDigest {
            format: mainframe_env_execution_api::AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [9; 32],
        },
        decision: mainframe_env_execution_api::AuditDecision::Success,
    };
    store.commit_execution_step(
        &row.execution.execution_id,
        4,
        None,
        row.event.clone(),
        row.effect.clone(),
        Some(audit),
        None,
        notification(&row.event),
    )
}

#[test]
fn forwarded_real_commit_is_visible_before_park_and_exact_return_is_preserved() {
    let inner = Arc::new(MemoryStore::new(Default::default()));
    let batch = seed(inner.as_ref(), true);
    let online = seed(inner.as_ref(), false);
    let observed = Arc::new(ObservedStore::new(inner.clone()));
    observed.arm().unwrap();
    let coordination = coordination();
    let deadline = Instant::now() + Duration::from_secs(120);
    coordination.arm(deadline).unwrap();
    let store = Arc::new(CoordinatedStore::new(
        observed.clone(),
        coordination.clone(),
    ));
    let route = store.clone();
    let offered = batch.clone();
    let handle = std::thread::spawn(move || commit(route.as_ref(), &offered));
    let mut parked = Parked {
        coordination: coordination.clone(),
        row: batch,
        handle: Some(handle),
    };
    runtime()
        .block_on(coordination.wait_for_boundary(deadline.into()))
        .unwrap();
    // Both real inner mutexes are available while this actual caller waits.
    let actual = inner
        .get_execution(&parked.row.execution.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(actual, parked.row.execution);
    assert_eq!(
        observed.export().unwrap()["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(commit(store.as_ref(), &online).unwrap(), online.execution);
    assert_eq!(parked.join().unwrap(), actual);
    coordination.finish().unwrap();
    // A release and two real commits cannot replace the missing after-side.
    assert!(
        observed
            .prove_overlap(
                &coordination.batch_artifact,
                &coordination.online_artifact,
                &online.execution.run_unit_id,
            )
            .is_err()
    );
}

#[test]
fn wrong_same_actor_or_failed_markers_never_release_the_selected_batch() {
    let mut parked = park();
    for case in 0..9 {
        let mut online = fixture(false, 2, 1);
        match case {
            0 => online.execution.artifact = parked.row.execution.artifact.clone(),
            1 => online.execution.principal = parked.row.execution.principal.clone(),
            2 => online.effect.as_mut().unwrap().intent.capability = None,
            3 => online.effect.as_mut().unwrap().state = EffectState::Failed,
            4 => online.effect.as_mut().unwrap().resolved_tick = None,
            5 => online.effect.as_mut().unwrap().digest_format = EffectDigestFormat::LegacyDebug,
            6 => {
                online.effect.as_mut().unwrap().intent.owner =
                    parked.row.execution.execution_id.clone()
            }
            7 => {
                online.execution.run_unit_id = parked.row.execution.run_unit_id.clone();
                online.event.run_unit_id = online.execution.run_unit_id.clone();
                online.effect.as_mut().unwrap().run_unit_id = online.execution.run_unit_id.clone();
            }
            _ => {
                online.execution.execution_id = parked.row.execution.execution_id.clone();
                online.execution.run_unit_id = parked.row.execution.run_unit_id.clone();
                online.event.execution_id = online.execution.execution_id.clone();
                online.event.run_unit_id = online.execution.run_unit_id.clone();
                online.effect.as_mut().unwrap().execution_id =
                    online.execution.execution_id.clone();
                online.effect.as_mut().unwrap().run_unit_id = online.execution.run_unit_id.clone();
                online.effect.as_mut().unwrap().intent.owner =
                    online.execution.execution_id.clone();
            }
        }
        offer(&parked.coordination, &online);
        assert_eq!(parked.coordination.export()["released"], false);
        assert!(parked.coordination.finish().is_err());
    }
    offer(&parked.coordination, &fixture(false, 2, 1));
    assert_eq!(parked.join().unwrap(), parked.row.execution);
    parked.coordination.finish().unwrap();
}

#[test]
fn absent_or_unsolicited_online_effect_cannot_become_a_historical_live_claim() {
    let coordination = coordination();
    coordination
        .arm(Instant::now() + Duration::from_secs(120))
        .unwrap();
    offer(&coordination, &fixture(false, 2, 1));
    assert_eq!(coordination.export()["released"], false);
    assert!(coordination.finish().is_err());
    let mut parked = park();
    assert!(parked.coordination.finish().is_err());
    parked.coordination.abort();
    assert_eq!(parked.join().unwrap(), parked.row.execution);
    assert!(parked.coordination.finish().is_err());
}

#[test]
fn terminal_or_changed_selected_batch_releases_only_as_refusal() {
    for case in 0..3 {
        let mut parked = park();
        let mut terminal = parked.row.clone();
        match case {
            0 => {
                terminal.execution.state = ExecutionState::Completed;
                terminal.event.kind = LifecycleEventKind::Completed { return_code: 0 };
                terminal.effect = None;
            }
            1 => {
                terminal.execution.run_unit_id =
                    RunUnitId::new("changed-selected-run", InvocationLimits::default()).unwrap()
            }
            _ => terminal.execution.attempt += 1,
        }
        offer(&parked.coordination, &terminal);
        assert_eq!(parked.join().unwrap(), parked.row.execution);
        assert!(parked.coordination.finish().is_err());
        assert_eq!(
            parked.coordination.export()["refusal"],
            "selected-batch-retired-or-changed"
        );
    }
}

#[test]
fn expired_original_deadline_and_commit_error_forward_without_false_completion() {
    let coordination = coordination();
    coordination.arm(Instant::now()).unwrap();
    let batch = fixture(true, 1, 1);
    let original = Err(StoreError::Conflict);
    coordination.after_commit(&batch.event, batch.effect.as_ref(), &original);
    assert_eq!(original, Err(StoreError::Conflict));
    assert!(coordination.export()["batch"].is_null());
    offer(&coordination, &batch);
    assert_eq!(
        coordination.export()["refusal"],
        "original-completion-deadline"
    );
    assert!(coordination.finish().is_err());
}

#[test]
fn error_panic_drop_release_before_original_eighteen_future_join_and_cleanup() {
    // Construction, Pending, and successful completion keep the guard live
    // without aborting. Field-only capture would abort during construction.
    let successful = coordination();
    let mut polls = 0;
    let future = successful.clone().guard_result(std::future::poll_fn(|_| {
        polls += 1;
        if polls == 1 {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok::<_, CorpusProblem>(17))
        }
    }));
    assert_eq!(successful.export()["released"], false);
    assert!(successful.export()["refusal"].is_null());
    let mut future = Box::pin(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(successful.export()["released"], false);
    assert!(successful.export()["refusal"].is_null());
    assert!(matches!(
        future.as_mut().poll(&mut context),
        std::task::Poll::Ready(Ok(17))
    ));
    assert_eq!(successful.export()["released"], false);
    assert!(successful.export()["refusal"].is_null());
    drop(future);
    assert_eq!(successful.export()["released"], false);
    assert!(successful.export()["refusal"].is_null());
    // Original join_campaign is exercised, never replaced by a narrowed helper.
    let mut parked = park();
    let owner = parked.coordination.clone();
    let runtime = runtime();
    let (submissions, exchanges) = runtime.block_on(async {
        let mut submissions = Vec::new();
        for index in 0..16 {
            let owner = owner.clone();
            submissions.push(tokio::spawn(owner.guard_result(async move {
                if index == 0 {
                    panic!("fixed coordination submission panic");
                }
                if index == 1 {
                    return Err(refused());
                }
                Ok((StatusCode::CREATED, Vec::new()))
            })));
        }
        let exchanges = async {
            let _release_before_joins = owner.release_on_drop();
            let (account, cards) = tokio::join!(
                owner
                    .clone()
                    .guard_result(async { Err::<serde_json::Value, _>(refused()) }),
                owner
                    .clone()
                    .guard_result(async { Ok::<_, CorpusProblem>(serde_json::json!({})) }),
            );
            Ok((account?, cards?))
        };
        campaign::join_campaign(submissions, exchanges).await
    });
    assert_eq!(submissions.len(), 16);
    assert!(submissions[0].as_ref().unwrap_err().is_panic());
    assert!(submissions[1].as_ref().unwrap().is_err());
    assert!(
        submissions[2..]
            .iter()
            .all(|value| value.as_ref().unwrap().is_ok())
    );
    assert!(exchanges.is_err());
    assert_eq!(parked.join().unwrap(), parked.row.execution);
    assert!(parked.coordination.finish().is_err());
    // An offered future owns its guard even when it is never first-polled.
    let mut unpolled = park();
    let future = unpolled
        .coordination
        .clone()
        .guard_result(std::future::pending::<Result<(), CorpusProblem>>());
    drop(future);
    assert_eq!(unpolled.join().unwrap(), unpolled.row.execution);
    assert!(unpolled.coordination.finish().is_err());
    // A Pending request's explicit drop uses the same guard, with no extra poll.
    let mut dropped = park();
    let mut future = Box::pin(
        dropped
            .coordination
            .clone()
            .guard_result(std::future::pending::<Result<(), CorpusProblem>>()),
    );
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    drop(future);
    assert_eq!(dropped.join().unwrap(), dropped.row.execution);
    assert!(dropped.coordination.finish().is_err());
}
