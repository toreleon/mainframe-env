//! Actual retained root/index/lease proof; no host permission is minted here.
use super::*;
fn step(store: &dyn PlatformStore) -> (RootProviderPublication, CheckedReplayRefusalStep) {
    let (owner, mut effect) = original(store);
    effect.state = EffectState::Completed;
    effect.result_digest = Some([7; 32]);
    effect.resolved_tick = Some(10);
    store.record_result(&effect.key, effect.clone()).unwrap();
    store
        .mutate_provider_states_atomic(vec![put("native-root-a", "receipt", 1, None)])
        .unwrap();
    let execution = owner.occurrence.execution.clone();
    let e = event(
        &execution,
        4,
        LifecycleEventKind::EffectResult {
            sequence: effect.sequence,
        },
    );
    let mut a = audit(&owner, &effect, vec![]).audit;
    a.decision = AuditDecision::Deny;
    a.observed_tick = e.tick;
    let r = CheckedReplayRefusalStep {
        effect,
        execution,
        dependencies: vec![TerminalRowDependency::Exact(
            store
                .get_provider_state("native-root-a", "receipt")
                .unwrap()
                .unwrap(),
        )],
        audit: a,
        notification: outbox(&e),
        event: e,
    };
    (owner, r)
}
fn run(store: &dyn PlatformStore) {
    let (owner, r) = step(store);
    let before = observed(store, &owner);
    for scope in ["unregistered", "exact-shared"] {
        let mut bad = r.clone();
        bad.dependencies.push(TerminalRowDependency::Absent {
            namespace: scope.into(),
            key: "foreign".into(),
        });
        assert!(store.commit_checked_replay_refusal(bad).is_err());
        assert_eq!(observed(store, &owner), before);
    }
    let mut expired = r.clone();
    expired.event.tick = 100;
    expired.audit.observed_tick = 100;
    assert!(store.commit_checked_replay_refusal(expired).is_err());
    assert_eq!(observed(store, &owner), before);
    store.commit_checked_replay_refusal(r.clone()).unwrap();
    let x = store
        .get_execution(&r.execution.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(x.version, 4);
    assert_eq!(store.effect(&r.effect.key).unwrap(), Some(r.effect.clone()));
    assert_eq!(
        store.audit_records(&x.execution_id, 1, 20).unwrap(),
        vec![r.audit.clone()]
    );
    let mut closed = r;
    closed.execution = x;
    closed.event = event(
        &closed.execution,
        5,
        LifecycleEventKind::EffectResult {
            sequence: closed.effect.sequence,
        },
    );
    closed.notification = outbox(&closed.event);
    closed.audit.observed_tick = closed.event.tick;
    store
        .close_root_driver(&owner.occurrence.claim, &closed.execution, 10)
        .unwrap();
    let before = observed(store, &owner);
    assert!(store.commit_checked_replay_refusal(closed).is_err());
    assert_eq!(observed(store, &owner), before);
}
#[test]
fn memory_checked_replay_refusal_current_root_scope_deadline_and_closing_fence() {
    run(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_checked_replay_refusal_current_root_scope_deadline_and_closing_fence() {
    let f = OwnedSqlite::new();
    run(f.store());
}
