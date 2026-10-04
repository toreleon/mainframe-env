use super::*;

fn owner() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}
fn new_registry(max: usize) -> (MqHandleRegistry, MqHconn) {
    let mut registry = MqHandleRegistry::new(7, max).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    (registry, connection)
}
fn snapshot(registry: &MqHandleRegistry) -> String {
    format!("{registry:?}")
}
#[test]
fn abandoned_observation_never_revives_and_exact_generation_is_preserved() {
    let (mut registry, connection) = new_registry(4);
    let before = snapshot(&registry);
    let stage = registry.stage_message_create(owner(), connection).unwrap();
    let abandoned = stage.provisional().unwrap();
    assert!(abandoned.is_historical());
    stage.abort();
    assert_eq!(snapshot(&registry), before);
    let stage = registry.stage_message_create(owner(), connection).unwrap();
    assert_eq!(stage.provisional(), Some(abandoned));
    let MqMessageAdoption::Created(live) = stage.adopt() else {
        panic!("creation")
    };
    assert!(!live.is_historical());
    assert_eq!(live.canonical_parts(), abandoned.canonical_parts());
    assert_eq!(
        registry.validate_message_property(owner(), connection, abandoned.into()),
        Err(MqHandleProblem::Historical)
    );
    registry
        .validate_message_property(owner(), connection, live.into())
        .unwrap();
    assert_eq!(registry.active_handles(), 2);
}
#[test]
fn delete_and_use_abort_leave_every_slot_and_counter_untouched() {
    let (mut registry, connection) = new_registry(5);
    let object = registry.create_object(owner(), connection).unwrap();
    let message = registry.create_message(owner(), connection).unwrap();
    let before = snapshot(&registry);
    drop(
        registry
            .stage_message_delete(owner(), connection, message)
            .unwrap(),
    );
    assert_eq!(snapshot(&registry), before);
    assert_eq!(
        registry
            .stage_message_use(owner(), connection, message)
            .unwrap()
            .adopt(),
        MqMessageAdoption::Unchanged
    );
    assert_eq!(snapshot(&registry), before);
    assert_eq!(
        registry
            .stage_message_delete(owner(), connection, message)
            .unwrap()
            .adopt(),
        MqMessageAdoption::Deleted
    );
    assert_eq!(registry.active_handles(), 2);
    assert_eq!(
        registry.validate_message_property(owner(), connection, message.into()),
        Err(MqHandleProblem::Stale)
    );
    registry
        .validate(owner(), connection, object.into(), MqHandleKind::Object)
        .unwrap();
    let MqMessageAdoption::Created(next) = registry
        .stage_message_create(owner(), connection)
        .unwrap()
        .adopt()
    else {
        panic!("creation")
    };
    assert_eq!(next.0.slot, message.0.slot);
    assert_eq!(next.0.generation, message.0.generation + 1);
}
#[test]
fn foreign_historical_changed_epoch_in_use_and_closed_parent_refuse() {
    let (mut registry, connection) = new_registry(5);
    let (mut foreign, foreign_connection) = new_registry(5);
    let message = registry.create_message(owner(), connection).unwrap();
    let before = snapshot(&registry);
    assert!(matches!(
        foreign.stage_message_use(owner(), foreign_connection, message),
        Err(MqHandleProblem::Stale)
    ));
    let historical = MqHmsg(HandleId {
        historical: true,
        ..message.0
    });
    assert!(matches!(
        registry.stage_message_delete(owner(), connection, historical),
        Err(MqHandleProblem::Historical)
    ));
    assert_eq!(snapshot(&registry), before);
    registry
        .begin_message_io(owner(), connection, message)
        .unwrap();
    assert!(matches!(
        registry.stage_message_use(owner(), connection, message),
        Err(MqHandleProblem::InUse)
    ));
    registry
        .end_message_io(owner(), connection, message)
        .unwrap();
    let mut other = owner();
    other.process_id += 1;
    assert!(
        registry
            .stage_message_use(other, connection, message)
            .is_err()
    );
    registry.disconnect(owner(), connection).unwrap();
    assert!(registry.stage_message_create(owner(), connection).is_err());
    registry.advance_epoch(8).unwrap();
    assert!(
        registry
            .stage_message_use(owner(), connection, message)
            .is_err()
    );
}
#[test]
fn finite_capacity_special_and_shared_profile_refuse_without_mutation() {
    let (mut registry, connection) = new_registry(1);
    let before = snapshot(&registry);
    assert!(matches!(
        registry.stage_message_create(owner(), connection),
        Err(MqHandleProblem::Capacity)
    ));
    assert_eq!(snapshot(&registry), before);
    assert!(matches!(
        registry.stage_message_create(owner(), MqHconn::Unassociated),
        Err(MqHandleProblem::SpecialConnection)
    ));
    assert!(matches!(
        registry.stage_message_create(owner(), MqHconn::Default),
        Err(MqHandleProblem::SpecialConnection)
    ));
    let mut shared = MqHandleRegistry::new(7, 3).unwrap();
    let conn = shared
        .connect(owner(), MqHandleSharing::SharedBlock)
        .unwrap();
    assert!(matches!(
        shared.stage_message_create(owner(), conn),
        Err(MqHandleProblem::SpecialConnection)
    ));
}
#[test]
fn exhausted_reusable_generation_is_not_issued_and_nonmessage_state_is_exact() {
    let (mut registry, connection) = new_registry(3);
    let message = registry.create_message(owner(), connection).unwrap();
    registry.slots[message.0.slot as usize].generation = u64::MAX;
    let at_max = MqHmsg(HandleId {
        generation: u64::MAX,
        ..message.0
    });
    registry
        .stage_message_delete(owner(), connection, at_max)
        .unwrap()
        .adopt();
    assert_eq!(registry.slots[1].generation, 0);
    let parent = registry.slots[0];
    let MqMessageAdoption::Created(next) = registry
        .stage_message_create(owner(), connection)
        .unwrap()
        .adopt()
    else {
        panic!("creation")
    };
    assert_eq!(next.0.slot, 2);
    assert_eq!(format!("{:?}", registry.slots[0]), format!("{parent:?}"));
    assert_eq!(registry.slots[1].generation, 0);
}
