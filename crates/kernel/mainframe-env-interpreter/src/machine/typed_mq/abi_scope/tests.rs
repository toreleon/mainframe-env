//! Private ABI tests, not installed root/provider/SAF or licensed acceptance.
use super::super::tests::{context, issued};
use super::*;

pub(super) fn scope(capacity: usize) -> Arc<MqMqiAbiScope> {
    Arc::new(MqMqiAbiScope::new(context(), capacity).unwrap())
}
pub(super) fn adopt(scope: &Arc<MqMqiAbiScope>, token: MqHconn) -> i32 {
    let reservation = scope.reserve().unwrap();
    let plan = scope.plan(Some(&reservation), Some(token), None).unwrap();
    let alias = plan.wire().unwrap();
    let mut guard = plan.guard(scope).unwrap();
    plan.commit(&mut guard);
    drop(guard);
    alias
}
#[test]
fn reservation_never_exposes_a_token_and_abort_burns_alias_without_leaking_capacity() {
    let scope = scope(1);
    let reservation = scope.reserve().unwrap();
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
    assert!(matches!(
        scope.reserve(),
        Err(HostProblem::ResourceExhausted)
    ));
    drop(reservation);
    let token = issued();
    assert_eq!(adopt(&scope, token), 2);
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
    assert_eq!(scope.connection(2), Ok(token));
}
#[test]
fn authoritative_repeat_reuses_alias_and_retirement_never_rewinds_it() {
    let scope = scope(2);
    let token = issued();
    assert_eq!(adopt(&scope, token), 1);
    assert_eq!(adopt(&scope, token), 1);
    let plan = scope.plan(None, None, Some(1)).unwrap();
    let mut guard = plan.guard(&scope).unwrap();
    plan.commit(&mut guard);
    drop(guard);
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
    assert_eq!(adopt(&scope, issued()), 3);
}
#[test]
fn equal_context_is_not_equal_scope_or_reservation_authority() {
    let first = scope(2);
    let foreign = scope(2);
    assert_ne!(first, foreign);
    let reservation = first.reserve().unwrap();
    assert!(
        foreign
            .plan(Some(&reservation), Some(issued()), None)
            .is_err()
    );
    assert_eq!(foreign.connection(1), Err(HostProblem::Malformed));
    assert_eq!(first.clone(), first);
}
#[test]
fn same_root_one_winner_refuses_a_stale_adoption_before_any_commit() {
    let scope = scope(2);
    let token = issued();
    let first = scope.reserve().unwrap();
    let second = scope.reserve().unwrap();
    let first_plan = scope.plan(Some(&first), Some(token), None).unwrap();
    let stale_plan = scope.plan(Some(&second), Some(token), None).unwrap();
    let mut guard = first_plan.guard(&scope).unwrap();
    first_plan.commit(&mut guard);
    drop(guard);
    assert!(stale_plan.guard(&scope).is_err());
    assert_eq!(scope.connection(1), Ok(token));
    assert_eq!(scope.connection(2), Err(HostProblem::Malformed));
}
#[test]
fn historical_and_special_tokens_cannot_be_adopted() {
    use mainframe_env_host_api::MqHandleObservation;
    let scope = scope(2);
    let live = issued();
    let historical = MqHandleObservation::capture_connection(live)
        .unwrap()
        .historical_connection()
        .unwrap();
    for token in [historical, MqHconn::Default, MqHconn::Unassociated] {
        let reservation = scope.reserve().unwrap();
        assert!(scope.plan(Some(&reservation), Some(token), None).is_err());
    }
    assert_eq!(scope.connection(1), Err(HostProblem::Malformed));
}
#[test]
fn context_fence_capacity_pic_range_and_poison_fail_closed() {
    for capacity in [0, MQ_MAX_HANDLE_SLOTS + 1] {
        assert!(MqMqiAbiScope::new(context(), capacity).is_err());
    }
    let scope = scope(2);
    let mut other = context();
    other.owner.task_id += 1;
    assert_eq!(scope.require_context(other), Err(HostProblem::Unauthorized));
    scope.table.lock().unwrap().next = LAST_ALIAS;
    let last = scope.reserve().unwrap();
    assert_eq!(last.0.alias, LAST_ALIAS);
    drop(last);
    assert!(matches!(
        scope.reserve(),
        Err(HostProblem::ResourceExhausted)
    ));
    scope.fence();
    assert_eq!(
        scope.require_context(context()),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        scope.connection(LAST_ALIAS),
        Err(HostProblem::UnknownOutcome)
    );
    let poisoned = super::tests::scope(2);
    let cloned = poisoned.clone();
    let _ = std::thread::spawn(move || {
        let _guard = cloned.table.lock().unwrap();
        panic!("private poison fixture");
    })
    .join();
    assert!(matches!(
        poisoned.reserve(),
        Err(HostProblem::UnknownOutcome)
    ));
}

mod machines;
