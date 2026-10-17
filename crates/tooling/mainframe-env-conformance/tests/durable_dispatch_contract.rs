#[path = "protected_dispatch/support.rs"]
mod support;
use mainframe_env_execution_api::{
    AuditDecision, ExecutionOutcome, IdempotencyKey, InvocationLimits,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_interpreter::{
    DurableEffectDispatch, DurableEffectDispatchHook, ExecutionControl, ExecutionCoordinator,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{EffectState, PlatformStore};
use std::sync::Arc;
use support::{Calls, host, invocation, machine};

struct Wrap {
    mode: &'static str,
    visits: usize,
}
impl DurableEffectDispatchHook for Wrap {
    fn with_dispatch(
        &mut self,
        actual: &DurableEffectDispatch<'_>,
        dispatch: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        self.visits += 1;
        actual.require_current(actual.original()).unwrap();
        assert_eq!(actual.intent().state, EffectState::Intent);
        assert_eq!(actual.intent().sequence, actual.effect().sequence);
        assert_eq!(
            actual
                .store()
                .effect(&actual.intent().key)
                .unwrap()
                .as_ref(),
            Some(actual.intent())
        );
        let mut changed = actual.original().clone();
        changed.attempt += 1;
        assert_eq!(
            actual.require_current(&changed),
            Err(HostProblem::UnknownOutcome)
        );
        if self.mode == "omit" {
            return Ok(());
        }
        if self.mode == "before" {
            return Err(HostProblem::UnknownOutcome);
        }
        dispatch()?;
        if self.mode == "repeat" {
            assert_eq!(dispatch(), Err(HostProblem::UnknownOutcome));
        }
        if self.mode == "after" {
            return Err(HostProblem::UnknownOutcome);
        }
        if self.mode == "panic" {
            panic!("actual wrapping owner failure");
        }
        Ok(())
    }
}

fn durable() -> (ExecutionCoordinator, Arc<dyn PlatformStore>, Calls) {
    let (actual_host, calls) = host();
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
    (
        ExecutionCoordinator::durable(actual_host, store.clone(), Default::default()),
        store,
        calls,
    )
}

#[test]
fn durable_dispatch_wraps_real_intent_once_and_preserves_original_results_and_audits() {
    let actor = invocation();
    let (driver, store, calls) = durable();
    let mut hook = Wrap {
        mode: "once",
        visits: 0,
    };
    let mut tick = 0;
    let outcome = driver.execute_resumable_with_dispatch_control(
        &mut machine(&actor),
        &actor,
        || {
            tick += 1;
            Ok(ExecutionControl {
                now_tick: tick,
                cancellation_requested: false,
            })
        },
        &mut hook,
    );
    assert!(matches!(outcome, ExecutionOutcome::Completed(_)));
    assert_eq!(hook.visits, 2);
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for (index, (observed, request)) in calls.iter().enumerate() {
        assert_eq!(observed, &actor);
        assert_eq!(request.sequence, index as u64 + 1);
        let row = store
            .effect(request.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(row.state, EffectState::Completed);
        assert_eq!(row.execution_id, actor.execution_id);
        assert!(row.result_digest.is_some());
    }
    let audits = store.audit_records(&actor.execution_id, 0, 10).unwrap();
    assert_eq!(audits.len(), 2);
    assert!(
        audits
            .iter()
            .all(|a| a.decision == AuditDecision::Success
                && a.invocation_key == actor.idempotency_key)
    );
}

#[test]
fn durable_dispatch_owner_refusal_never_manufactures_or_repeats_host_effects() {
    for mode in ["omit", "before", "repeat", "after", "panic"] {
        let actor = invocation();
        let (driver, store, calls) = durable();
        let mut hook = Wrap { mode, visits: 0 };
        let outcome = driver.execute_resumable_with_dispatch_control(
            &mut machine(&actor),
            &actor,
            || {
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            },
            &mut hook,
        );
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome())
        );
        assert_eq!(hook.visits, 1);
        let dispatched = !matches!(mode, "omit" | "before");
        assert_eq!(calls.lock().unwrap().len(), usize::from(dispatched));
        let key = IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap();
        let row = store.effect(&key).unwrap().unwrap();
        assert_eq!(
            row.state,
            if dispatched {
                EffectState::UnknownOutcome
            } else {
                EffectState::Intent
            }
        );
        assert_eq!(row.resolved_tick, None);
        assert_eq!(
            store
                .audit_records(&actor.execution_id, 0, 10)
                .unwrap()
                .len(),
            usize::from(dispatched)
        );
    }
}

#[test]
fn durable_dispatch_retained_uncertainty_refuses_before_any_new_wrapping_or_host_call() {
    let actor = invocation();
    let (driver, store, calls) = durable();
    let mut first = Wrap {
        mode: "after",
        visits: 0,
    };
    let mut run = machine(&actor);
    let _ = driver.execute_resumable_with_dispatch_control(
        &mut run,
        &actor,
        || {
            Ok(ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            })
        },
        &mut first,
    );
    let captured = store
        .effect(&IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap())
        .unwrap();
    let mut retry = Wrap {
        mode: "once",
        visits: 0,
    };
    let outcome = driver.execute_resumable_with_dispatch_control(
        &mut machine(&actor),
        &actor,
        || {
            Ok(ExecutionControl {
                now_tick: 2,
                cancellation_requested: false,
            })
        },
        &mut retry,
    );
    assert!(!matches!(outcome, ExecutionOutcome::Completed(_)));
    assert_eq!(retry.visits, 0);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(
        store
            .effect(&IdempotencyKey::new("key:1", InvocationLimits::default()).unwrap())
            .unwrap(),
        captured
    );
}
