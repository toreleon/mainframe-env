use super::*;
use serde_json::{Value, json};

fn owner(environment: MqHostEnvironment) -> MqHandleOwner {
    MqHandleOwner {
        environment,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}
fn historical(handle: MqHandle) -> MqHandle {
    let captured = MqHandleObservation::from(handle);
    let bytes = serde_json::to_vec(&captured).unwrap();
    let observed: MqHandleObservation = serde_json::from_slice(&bytes).unwrap();
    match handle.kind() {
        MqHandleKind::Object => observed.historical_object().unwrap().into(),
        MqHandleKind::Subscription => observed.historical_subscription().unwrap().into(),
        MqHandleKind::Message => observed.historical_message().unwrap().into(),
    }
}

#[test]
fn all_coincident_historical_registry_accesses_fail_without_mutation() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let who = owner(MqHostEnvironment::ZosBatch);
    let conn = r.connect(who, MqHandleSharing::NonShared).unwrap();
    let hc = MqHandleObservation::capture_connection(conn)
        .unwrap()
        .historical_connection()
        .unwrap();
    let obj = r.create_object(who, conn).unwrap();
    let sub = r.create_subscription(who, conn).unwrap();
    let msg = r.create_message(who, conn).unwrap();
    let un = r.create_message(who, MqHconn::Unassociated).unwrap();
    assert_ne!(conn, hc);
    assert!(hc.is_historical());
    assert_eq!(
        r.validate_connection(who, hc),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(r.connection_id(who, hc), Err(MqHandleProblem::Historical));
    assert_eq!(r.disconnect(who, hc), Err(MqHandleProblem::Historical));
    assert_eq!(r.create_object(who, hc), Err(MqHandleProblem::Historical));
    assert_eq!(
        r.create_subscription(who, hc),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(r.create_message(who, hc), Err(MqHandleProblem::Historical));
    for live in [
        MqHandle::Object(obj),
        MqHandle::Subscription(sub),
        MqHandle::Message(msg),
    ] {
        let past = historical(live);
        assert_ne!(live, past);
        assert!(past.is_historical());
        assert_eq!(r.entry(past.id()).unwrap_err(), MqHandleProblem::Historical);
        assert_eq!(
            r.entry_mut(past.id()).unwrap_err(),
            MqHandleProblem::Historical
        );
        assert!(!r.is_live(past));
        assert!(r.is_live(live));
        assert_eq!(
            r.child_id(who, conn, past, live.kind()),
            Err(MqHandleProblem::Historical)
        );
        assert_eq!(
            r.validate(who, conn, past, live.kind()),
            Err(MqHandleProblem::Historical)
        );
        assert_eq!(
            r.validate(who, hc, live, live.kind()),
            Err(MqHandleProblem::Historical)
        );
        assert_eq!(
            r.release(who, conn, past, live.kind()),
            Err(MqHandleProblem::Historical)
        );
    }
    let hm = MqHandleObservation::from(MqHandle::Message(msg))
        .historical_message()
        .unwrap();
    let hu = MqHandleObservation::from(MqHandle::Message(un))
        .historical_message()
        .unwrap();
    assert_eq!(
        r.validate_message_property(who, conn, hm.into()),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        r.validate_message_property(who, MqHconn::Unassociated, hu.into()),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        r.release(who, MqHconn::Unassociated, hu.into(), MqHandleKind::Message),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        r.begin_message_io(who, conn, hm),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        r.begin_message_io(who, hc, msg),
        Err(MqHandleProblem::Historical)
    );
    r.begin_message_io(who, conn, msg).unwrap();
    assert_eq!(
        r.end_message_io(who, conn, hm),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        r.end_message_io(who, hc, msg),
        Err(MqHandleProblem::Historical)
    );
    assert!(r.entry(msg.0).unwrap().in_use);
    r.end_message_io(who, conn, msg).unwrap();
    assert_eq!(r.active_handles(), 5);
    for live in [
        MqHandle::Object(obj),
        MqHandle::Subscription(sub),
        MqHandle::Message(msg),
    ] {
        r.validate(who, conn, live, live.kind()).unwrap();
    }
    r.validate_message_property(who, MqHconn::Unassociated, un.into())
        .unwrap();
}

#[test]
fn exact_preexisting_resolution_is_readonly_and_stricter_than_shared_permission() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let who = owner(MqHostEnvironment::MqiClient);
    let conn = r.connect(who, MqHandleSharing::SharedBlock).unwrap();
    let co = MqHandleObservation::capture_connection(conn).unwrap();
    assert_eq!(r.resolve_observed_connection(who, co), Ok(conn));
    for live in [
        MqHandle::Object(r.create_object(who, conn).unwrap()),
        MqHandle::Subscription(r.create_subscription(who, conn).unwrap()),
        MqHandle::Message(r.create_message(who, conn).unwrap()),
    ] {
        let obs = MqHandleObservation::from(live);
        assert_eq!(
            r.resolve_observed_handle(who, conn, obs, live.kind()),
            Ok(live)
        );
        assert_eq!(
            r.resolve_observed_handle(who, co.historical_connection().unwrap(), obs, live.kind()),
            Err(MqHandleProblem::Historical)
        );
        let other = r.connect(who, MqHandleSharing::SharedBlock).unwrap();
        assert_eq!(
            r.resolve_observed_handle(who, other, obs, live.kind()),
            Err(MqHandleProblem::CrossConnection)
        );
        r.disconnect(who, other).unwrap();
        let mut changed = who;
        changed.thread_id += 1;
        r.validate(changed, conn, live, live.kind()).unwrap();
        assert_eq!(
            r.resolve_observed_handle(changed, conn, obs, live.kind()),
            Err(MqHandleProblem::CrossOwner)
        );
        for wrong in [
            MqHandleKind::Object,
            MqHandleKind::Subscription,
            MqHandleKind::Message,
        ] {
            if wrong != live.kind() {
                assert_eq!(
                    r.resolve_observed_handle(who, conn, obs, wrong),
                    Err(MqHandleProblem::WrongKind)
                );
            }
        }
    }
    for field in 0..6 {
        let mut changed = who;
        match field {
            0 => changed.environment = MqHostEnvironment::OtherBindings,
            1 => changed.host_id += 1,
            2 => changed.process_id += 1,
            3 => changed.thread_id += 1,
            4 => changed.task_id += 1,
            _ => changed.syncpoint_epoch += 1,
        }
        assert_eq!(
            r.resolve_observed_connection(changed, co),
            Err(MqHandleProblem::CrossOwner)
        );
    }
    let mut invalid = who;
    invalid.host_id = 0;
    assert_eq!(
        r.resolve_observed_connection(invalid, co),
        Err(MqHandleProblem::InvalidOwner)
    );
    assert_eq!(r.active_handles(), 4);
}

#[test]
fn no_retirement_reuse_foreign_or_restart_resurrection() {
    let mut r = MqHandleRegistry::new(7, 4).unwrap();
    let who = owner(MqHostEnvironment::ZosBatch);
    let conn = r.connect(who, MqHandleSharing::NonShared).unwrap();
    let co = MqHandleObservation::capture_connection(conn).unwrap();
    let obj = r.create_object(who, conn).unwrap();
    let obs = MqHandleObservation::from(MqHandle::Object(obj));
    r.release(who, conn, obj.into(), MqHandleKind::Object)
        .unwrap();
    let next = r.create_object(who, conn).unwrap();
    assert_ne!(obj, next);
    assert_eq!(
        r.resolve_observed_handle(who, conn, obs, MqHandleKind::Object),
        Err(MqHandleProblem::Stale)
    );
    let mut foreign = MqHandleRegistry::new(7, 4).unwrap();
    let foreign_conn = foreign.connect(who, MqHandleSharing::NonShared).unwrap();
    assert_eq!(
        foreign.resolve_observed_connection(who, co),
        Err(MqHandleProblem::Stale)
    );
    assert_eq!(
        foreign.resolve_observed_handle(who, foreign_conn, obs, MqHandleKind::Object),
        Err(MqHandleProblem::Stale)
    );
    r.advance_epoch(8).unwrap();
    assert_eq!(r.active_handles(), 0);
    assert_eq!(
        r.resolve_observed_connection(who, co),
        Err(MqHandleProblem::Stale)
    );
    let newer = r.connect(who, MqHandleSharing::NonShared).unwrap();
    assert_eq!(
        r.resolve_observed_connection(who, co),
        Err(MqHandleProblem::Stale)
    );
    assert_eq!(
        r.resolve_observed_handle(who, newer, obs, MqHandleKind::Object),
        Err(MqHandleProblem::Stale)
    );
    assert_eq!(r.active_handles(), 1);
    // Decode is still possible, but can never be used after restart.
    assert!(obs.historical_object().unwrap().is_historical());
}

#[test]
fn checked_default_and_unassociated_parent_roles_never_mint_a_historical_special() {
    for special in [MqHconn::Default, MqHconn::Unassociated] {
        assert_eq!(
            MqHandleObservation::capture_connection(special),
            Err(MqHandleProblem::SpecialConnection)
        );
    }
    let mut r = MqHandleRegistry::new(7, 8).unwrap();
    let who = owner(MqHostEnvironment::ZosCics);
    r.bind_cics_default(who).unwrap();
    let obj = r.create_object(who, MqHconn::Default).unwrap();
    let obs = MqHandleObservation::from(MqHandle::Object(obj));
    assert_eq!(
        r.resolve_observed_handle(who, MqHconn::Default, obs, MqHandleKind::Object),
        Ok(obj.into())
    );
    // Even an internally forged observation of the default slot cannot
    // relabel the CICS default entry as an issued connection.
    let fake = MqHandleObservation::capture(Role::Connection, r.defaults[0].1);
    assert_eq!(
        r.resolve_observed_connection(who, fake),
        Err(MqHandleProblem::SpecialConnection)
    );
    let un = r.create_message(who, MqHconn::Unassociated).unwrap();
    let unobs = MqHandleObservation::from(MqHandle::Message(un));
    assert_eq!(
        r.resolve_observed_handle(who, MqHconn::Unassociated, unobs, MqHandleKind::Message),
        Ok(un.into())
    );
    assert_eq!(
        r.resolve_observed_handle(who, MqHconn::Default, unobs, MqHandleKind::Message),
        Err(MqHandleProblem::CrossConnection)
    );
    r.begin_message_io(who, MqHconn::Default, un).unwrap();
    assert_eq!(
        r.resolve_observed_handle(who, MqHconn::Unassociated, unobs, MqHandleKind::Message),
        Err(MqHandleProblem::InUse)
    );
    r.end_message_io(who, MqHconn::Default, un).unwrap();
    let mut other = who;
    other.task_id += 1;
    r.bind_cics_default(other).unwrap();
    assert_eq!(
        r.resolve_observed_handle(other, MqHconn::Default, obs, MqHandleKind::Object),
        Err(MqHandleProblem::CrossOwner)
    );
    assert_eq!(r.active_handles(), 4);
}

#[test]
fn observation_role_and_fixed_identity_reader_are_strict() {
    let r = &mut MqHandleRegistry::new(7, 8).unwrap();
    let who = owner(MqHostEnvironment::ZosBatch);
    let conn = r.connect(who, MqHandleSharing::NonShared).unwrap();
    let obs = MqHandleObservation::capture_connection(conn).unwrap();
    assert_eq!(obs.historical_object(), Err(MqHandleProblem::WrongKind));
    assert_eq!(
        obs.historical_subscription(),
        Err(MqHandleProblem::WrongKind)
    );
    assert_eq!(obs.historical_message(), Err(MqHandleProblem::WrongKind));
    let v = serde_json::to_value(obs).unwrap();
    // A syntactically valid role change cannot reinterpret an existing slot's
    // actual role. No new entry or live permission is produced by resolution.
    let mut disguised = v.clone();
    disguised["role"] = json!("Object");
    let disguised: MqHandleObservation = serde_json::from_value(disguised).unwrap();
    assert_eq!(
        r.resolve_observed_handle(who, conn, disguised, MqHandleKind::Object),
        Err(MqHandleProblem::WrongKind)
    );
    let obj = r.create_object(who, conn).unwrap();
    let mut disguised =
        serde_json::to_value(MqHandleObservation::from(MqHandle::Object(obj))).unwrap();
    disguised["role"] = json!("Connection");
    let disguised: MqHandleObservation = serde_json::from_value(disguised).unwrap();
    assert_eq!(
        r.resolve_observed_connection(who, disguised),
        Err(MqHandleProblem::WrongKind)
    );
    for field in ["role", "registry", "slot", "generation", "epoch"] {
        let mut bad = v.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<MqHandleObservation>(bad).is_err());
    }
    for (field, value) in [
        ("registry", json!(0)),
        ("generation", json!(0)),
        ("epoch", json!(0)),
        ("slot", json!(MQ_MAX_HANDLE_SLOTS)),
        ("slot", json!(u64::MAX)),
        ("role", json!("Default")),
        ("role", json!({"Connection":null})),
        ("registry", json!(-1)),
        ("epoch", Value::Null),
        ("live", json!(true)),
    ] {
        let mut bad = v.clone();
        bad[field] = value;
        assert!(serde_json::from_value::<MqHandleObservation>(bad).is_err());
    }
    for field in ["role", "registry", "slot", "generation", "epoch"] {
        let mut raw = serde_json::to_string(&v).unwrap();
        raw.insert_str(1, &format!("\"{field}\":{},", v[field]));
        assert!(serde_json::from_str::<MqHandleObservation>(&raw).is_err());
    }
    let raw = serde_json::to_string(&v).unwrap() + "null";
    assert!(serde_json::from_str::<MqHandleObservation>(&raw).is_err());
    // These are exact canonical runtime identity fields, not SQL row versions;
    // lossless observation supports the registry's existing u64 epoch domain.
    let max: MqHandleObservation = serde_json::from_value(json!({
        "role":"Connection","registry":u64::MAX,"slot":65535,
        "generation":u64::MAX,"epoch":u64::MAX
    }))
    .unwrap();
    let historical = max.historical_connection().unwrap();
    assert!(historical.is_historical());
    assert_eq!(
        r.resolve_observed_connection(who, max),
        Err(MqHandleProblem::Stale)
    );
    assert_eq!(r.active_handles(), 2);
}
