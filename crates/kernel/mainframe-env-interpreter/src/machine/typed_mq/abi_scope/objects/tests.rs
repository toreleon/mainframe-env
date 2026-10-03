//! Private root-table laws; no provider/public execution credit.
use super::super::super::tests::context;
use super::super::tests::{adopt, scope};
use super::*;
use mainframe_env_host_api::MqHandleRegistry;

fn tokens() -> (MqHconn, MqHconn, MqHobj) {
    let mut registry = MqHandleRegistry::new(1, 8).unwrap();
    let c = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let other = registry
        .connect(context().owner, MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(context().owner, c).unwrap();
    (c, other, object)
}
fn object(scope: &Arc<MqMqiAbiScope>, c_alias: i32, c: MqHconn, token: MqHobj) -> i32 {
    let r = scope.reserve().unwrap();
    let p = scope
        .object_plan(c_alias, c, Some(&r), Some(token), None)
        .unwrap();
    let alias = p.wire().unwrap();
    let mut guard = p.guard(scope).unwrap();
    p.commit(&mut guard);
    drop(guard);
    alias
}
#[test]
fn families_parents_cold_and_historical_are_not_interchangeable() {
    let shared = scope(5);
    let (c, other, token) = tokens();
    assert_eq!(adopt(&shared, c), 1);
    assert_eq!(adopt(&shared, other), 2);
    assert_eq!(object(&shared, 1, c, token), 3);
    assert_eq!(shared.object(1, c), Err(HostProblem::Malformed));
    assert_eq!(shared.connection(3), Err(HostProblem::Malformed));
    assert_eq!(shared.object(3, other), Err(HostProblem::Malformed));
    assert_eq!(scope(5).object(3, c), Err(HostProblem::Malformed));
    let historical = mainframe_env_host_api::MqHandleObservation::from(
        mainframe_env_host_api::MqHandle::Object(token),
    )
    .historical_object()
    .unwrap();
    let r = shared.reserve().unwrap();
    assert!(
        shared
            .object_plan(1, c, Some(&r), Some(historical), None)
            .is_err()
    );
    assert!(
        shared
            .object_plan(2, c, Some(&r), Some(token), None)
            .is_err()
    );
}
#[test]
fn close_only_one_object_disc_all_parent_objects_and_never_aba() {
    let shared = scope(6);
    let (c, other, token) = tokens();
    let ca = adopt(&shared, c);
    let oa = adopt(&shared, other);
    let first = object(&shared, ca, c, token);
    let (_, _, next) = tokens();
    let second = object(&shared, ca, c, next);
    let (_, _, foreign) = tokens();
    let third = object(&shared, oa, other, foreign);
    let close = shared.object_plan(ca, c, None, None, Some(first)).unwrap();
    let mut guard = close.guard(&shared).unwrap();
    close.commit(&mut guard);
    drop(guard);
    assert_eq!(shared.object(first, c), Err(HostProblem::Malformed));
    assert_eq!(shared.object(second, c), Ok(next));
    assert!(close.guard(&shared).is_err());
    let disc = shared.plan(None, None, Some(ca)).unwrap();
    let mut guard = disc.guard(&shared).unwrap();
    disc.commit(&mut guard);
    drop(guard);
    assert_eq!(shared.object(second, c), Err(HostProblem::Malformed));
    assert_eq!(shared.object(third, other), Ok(foreign));
    let (_, _, fresh) = tokens();
    assert!(object(&shared, oa, other, fresh) > third);
}
#[test]
fn duplicate_adoption_one_winner_and_parent_retirement_refuse_stale_plan() {
    let shared = scope(4);
    let (c, _, token) = tokens();
    let ca = adopt(&shared, c);
    let r1 = shared.reserve().unwrap();
    let r2 = shared.reserve().unwrap();
    let p1 = shared
        .object_plan(ca, c, Some(&r1), Some(token), None)
        .unwrap();
    let p2 = shared
        .object_plan(ca, c, Some(&r2), Some(token), None)
        .unwrap();
    let mut g = p1.guard(&shared).unwrap();
    p1.commit(&mut g);
    drop(g);
    assert!(p2.guard(&shared).is_err());
    let (_, _, fresh) = tokens();
    let p3 = shared
        .object_plan(ca, c, Some(&r2), Some(fresh), None)
        .unwrap();
    let d = shared.plan(None, None, Some(ca)).unwrap();
    let mut g = d.guard(&shared).unwrap();
    d.commit(&mut g);
    drop(g);
    assert!(p3.guard(&shared).is_err());
    assert_eq!(shared.object(2, c), Err(HostProblem::Malformed));
}

#[test]
fn concurrent_same_token_has_one_root_table_winner() {
    let shared = scope(3);
    let (c, _, token) = tokens();
    let ca = adopt(&shared, c);
    let r1 = shared.reserve().unwrap();
    let r2 = shared.reserve().unwrap();
    let plans = [
        shared
            .object_plan(ca, c, Some(&r1), Some(token), None)
            .unwrap(),
        shared
            .object_plan(ca, c, Some(&r2), Some(token), None)
            .unwrap(),
    ];
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let joins = plans
        .into_iter()
        .map(|plan| {
            let shared = shared.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                if let Ok(mut guard) = plan.guard(&shared) {
                    plan.commit(&mut guard);
                    true
                } else {
                    false
                }
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        joins
            .into_iter()
            .filter(|j| j.thread().id() != std::thread::current().id())
            .map(|j| usize::from(j.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(
        [2, 3]
            .into_iter()
            .filter(|a| shared.object(*a, c) == Ok(token))
            .count(),
        1
    );
}
