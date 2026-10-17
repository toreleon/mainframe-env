//! Infrastructure fixtures do not attest compiled CardDemo or IBM MQ behavior.
#[path = "protected_dispatch/support.rs"]
mod support;
use mainframe_env_execution_api::{
    AuditDecision, ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits,
    LifecycleEventKind,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_interpreter::{
    DurableEffectDispatch, ExecutionControl, ExecutionCoordinator, ProtectedStorageAdmission,
    ProtectedStorageConfiguration, ProtectedStorageDispatchHooks,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{
    EffectState, ExecutionState, PlatformStore, ProviderStateRecord, ROOT_DRIVER_NAMESPACE,
};
use std::sync::Arc;
use support::{Calls, host, invocation, machine};

struct Hooks {
    mode: &'static str,
    admitted: usize,
    dispatches: usize,
    retained: usize,
    inserted: Option<ProviderStateRecord>,
}

impl Hooks {
    fn new(mode: &'static str) -> Self {
        Self {
            mode,
            admitted: 0,
            dispatches: 0,
            retained: 0,
            inserted: None,
        }
    }
}

impl ProtectedStorageDispatchHooks for Hooks {
    fn admitted(&mut self, owner: &ProtectedStorageAdmission<'_>) -> Result<(), HostProblem> {
        self.admitted += 1;
        owner.require_current(owner.original()).unwrap();
        let admitted = owner
            .store()
            .get_execution(&owner.original().execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(admitted.state, ExecutionState::Admitted);
        assert_eq!(admitted.version, 1);
        assert_eq!(owner.claim().admission().execution, admitted);
        assert_eq!(
            owner.claim().admission().invocation_key,
            owner.original().idempotency_key
        );
        self.inserted = Some(owner.claim().inserted_row().clone());
        let mut changed = owner.original().clone();
        changed.attempt += 1;
        assert_eq!(
            owner.require_current(&changed),
            Err(HostProblem::Unauthorized)
        );
        if self.mode == "refuse-admission" {
            return Err(HostProblem::Unsupported);
        }
        Ok(())
    }

    fn with_dispatch(
        &mut self,
        owner: &ProtectedStorageAdmission<'_>,
        actual: &DurableEffectDispatch<'_>,
        invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        self.dispatches += 1;
        owner.require_current(actual.original()).unwrap();
        actual.require_current(owner.original()).unwrap();
        assert!(Arc::ptr_eq(owner.store(), actual.store()));
        assert_eq!(actual.intent().state, EffectState::Intent);
        assert_eq!(actual.intent().execution_id, owner.original().execution_id);
        assert_eq!(
            actual
                .store()
                .effect(&actual.intent().key)
                .unwrap()
                .as_ref(),
            Some(actual.intent())
        );
        if self.mode == "omit" {
            return Ok(());
        }
        invoke()?;
        if self.mode == "repeat" {
            assert_eq!(invoke(), Err(HostProblem::UnknownOutcome));
        }
        if self.mode == "after" {
            return Err(HostProblem::UnknownOutcome);
        }
        if self.mode == "panic" {
            panic!("actual protected callback failed");
        }
        Ok(())
    }

    fn retain_uncertain(
        &mut self,
        actor: &Invocation,
        owner: Option<&ProtectedStorageAdmission<'_>>,
    ) -> Result<(), HostProblem> {
        self.retained += 1;
        if let Some(owner) = owner {
            assert_eq!(owner.original(), actor);
            assert_eq!(
                owner.require_current(actor),
                Err(HostProblem::UnknownOutcome)
            );
        }
        Ok(())
    }
}

struct Driver {
    coordinator: ExecutionCoordinator,
    store: Arc<dyn PlatformStore>,
    calls: Calls,
}

fn driver(store: Arc<dyn PlatformStore>) -> Driver {
    let (actual_host, calls) = host();
    Driver {
        coordinator: ExecutionCoordinator::durable(actual_host, store.clone(), Default::default()),
        store,
        calls,
    }
}

fn memory() -> Driver {
    driver(Arc::new(MemoryStore::new(Default::default())))
}

fn configuration() -> ProtectedStorageConfiguration {
    ProtectedStorageConfiguration {
        configuration_digest: [0x5a; 32],
        provider_namespaces: Vec::new(),
        provider_rows: Vec::new(),
    }
}

fn execute(d: &Driver, actor: &Invocation, hooks: &mut Hooks) -> ExecutionOutcome {
    let mut tick = 0;
    d.coordinator.execute_protected_storage_with_control(
        &mut machine(actor),
        actor,
        configuration(),
        hooks,
        || {
            tick += 1;
            Ok(ExecutionControl {
                now_tick: tick,
                cancellation_requested: false,
            })
        },
    )
}

fn positive(store: Arc<dyn PlatformStore>) {
    let d = driver(store);
    let actor = invocation();
    let mut hooks = Hooks::new("once");
    let outcome = execute(&d, &actor, &mut hooks);
    assert!(
        matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome())
    );
    assert_eq!(
        (hooks.admitted, hooks.dispatches, hooks.retained),
        (1, 2, 1)
    );
    assert_eq!(d.calls.lock().unwrap().len(), 2);
    let execution = d.store.get_execution(&actor.execution_id).unwrap().unwrap();
    assert_eq!(execution.state, ExecutionState::Running);
    assert_eq!(execution.terminal_tick, None);
    let before = hooks.inserted.as_ref().unwrap();
    let fenced = d
        .store
        .get_provider_state(&before.namespace, &before.key)
        .unwrap()
        .unwrap();
    assert_eq!(fenced.version, before.version + 1);
    assert_ne!(fenced, *before);
    for index in 1..=2 {
        let key = IdempotencyKey::new(format!("key:{index}"), InvocationLimits::default()).unwrap();
        let effect = d.store.effect(&key).unwrap().unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        assert_eq!(effect.execution_id, actor.execution_id);
        assert_eq!(effect.sequence, index);
        assert!(effect.result_digest.is_some());
    }
    let audits = d.store.audit_records(&actor.execution_id, 0, 10).unwrap();
    assert_eq!(audits.len(), 2);
    assert!(
        audits
            .iter()
            .all(|audit| audit.decision == AuditDecision::Success
                && audit.invocation_key == actor.idempotency_key)
    );
    let events = d.store.events(&actor.execution_id, 1, 65_536).unwrap();
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        LifecycleEventKind::Completing
            | LifecycleEventKind::Completed { .. }
            | LifecycleEventKind::Abend
            | LifecycleEventKind::HandoffCompleted
    )));
    let effects = [
        d.store
            .effect(&IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap())
            .unwrap(),
        d.store
            .effect(&IdempotencyKey::new("key:2", InvocationLimits::default()).unwrap())
            .unwrap(),
    ];
    let mut second = Hooks::new("once");
    assert!(
        matches!(execute(&d, &actor, &mut second), ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome())
    );
    assert_eq!((second.admitted, second.dispatches), (0, 0));
    assert_eq!(d.calls.lock().unwrap().len(), 2);
    assert_eq!(
        d.store.events(&actor.execution_id, 1, 65_536).unwrap(),
        events
    );
    assert_eq!(
        d.store
            .get_provider_state(&before.namespace, &before.key)
            .unwrap()
            .as_ref(),
        Some(&fenced)
    );
    for (index, expected) in effects.into_iter().enumerate() {
        assert_eq!(
            d.store
                .effect(
                    &IdempotencyKey::new(format!("key:{}", index + 1), InvocationLimits::default())
                        .unwrap()
                )
                .unwrap(),
            expected
        );
    }
}

#[test]
fn protected_storage_uses_actual_fresh_owner_and_intent_on_memory_and_sqlite_without_settlement() {
    positive(Arc::new(MemoryStore::new(Default::default())));
    let root = std::env::temp_dir().join(format!(
        "protected-storage-controls-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&root).unwrap();
    {
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        positive(Arc::new(
            SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap(),
        ));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_storage_refusals_keep_actual_owner_and_unknown_work_without_second_dispatch() {
    for mode in ["refuse-admission", "omit", "repeat", "after", "panic"] {
        let d = memory();
        let actor = invocation();
        let mut hooks = Hooks::new(mode);
        assert!(
            matches!(execute(&d, &actor, &mut hooks), ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome())
        );
        assert_eq!(hooks.admitted, 1);
        assert_eq!(hooks.retained, 1);
        let called = !matches!(mode, "refuse-admission" | "omit");
        assert_eq!(d.calls.lock().unwrap().len(), usize::from(called));
        let row = d
            .store
            .effect(&IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap())
            .unwrap();
        if mode == "refuse-admission" {
            assert!(row.is_none());
            assert_eq!(hooks.dispatches, 0);
        } else {
            let row = row.unwrap();
            assert_eq!(
                row.state,
                if called {
                    EffectState::UnknownOutcome
                } else {
                    EffectState::Intent
                }
            );
            assert_eq!(row.resolved_tick, None);
            assert_eq!(hooks.dispatches, 1);
        }
        assert_eq!(
            d.store
                .audit_records(&actor.execution_id, 0, 10)
                .unwrap()
                .len(),
            usize::from(called)
        );
        assert!(
            !d.store
                .get_execution(&actor.execution_id)
                .unwrap()
                .unwrap()
                .state
                .terminal()
        );
    }
}

#[test]
fn protected_storage_defaults_and_original_scope_bounds_refuse_without_fallback_or_host_calls() {
    struct Refuse;
    impl ProtectedStorageDispatchHooks for Refuse {}
    let d = memory();
    let actor = invocation();
    assert!(
        matches!(d.coordinator.execute_protected_storage_with_control(&mut machine(&actor), &actor,
        configuration(), &mut Refuse, || Ok(ExecutionControl { now_tick: 1, cancellation_requested: false })),
        ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome())
    );
    assert!(d.calls.lock().unwrap().is_empty());
    assert!(
        d.store
            .effect(&IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        d.store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, actor.execution_id.as_str())
            .unwrap()
            .is_some()
    );
    let d = memory();
    let mut config = configuration();
    config.provider_namespaces = (0..17).map(|index| format!("scope-{index}")).collect();
    let mut hooks = Hooks::new("once");
    assert!(
        matches!(d.coordinator.execute_protected_storage_with_control(&mut machine(&actor), &actor,
        config, &mut hooks, || Ok(ExecutionControl { now_tick: 1, cancellation_requested: false })),
        ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome())
    );
    assert_eq!((hooks.admitted, hooks.dispatches), (0, 0));
    assert!(d.calls.lock().unwrap().is_empty());
    assert!(
        d.store
            .get_execution(&actor.execution_id)
            .unwrap()
            .is_none()
    );
    assert!(
        d.store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, actor.execution_id.as_str())
            .unwrap()
            .is_none()
    );
    assert!(
        d.store
            .events(&actor.execution_id, 1, 65_536)
            .unwrap()
            .is_empty()
    );
}
