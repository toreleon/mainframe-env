use super::*;

#[test]
fn prior_nonhandle_storage_and_host_canonical_golden_are_exact() {
    let value = result(MqMqiCall::Disconnect, MqMqiOutput::NoOutput, true);
    // Frozen independent host result golden, EFFECT-CANONICAL-V1.md. Private
    // storage field order/bytes are unchanged from sealed codec 0c6426e1.
    let hex = "feb3c22c6f1150b7a39bf17cee6612d0b722f524edc582e43b8a401b72826c27";
    let digest: Vec<u8> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(
        canonical_result_digest(&host(&value, MqMqiLimits::default()))
            .unwrap()
            .as_slice(),
        digest
    );
    let expected = format!(
        "{{\"schema_version\":\"mainframe-env.mq-mqi-result-storage@1\",\"call\":\"MQDISC\",\"outcome\":{{\"kind\":\"Completed\",\"status\":\"OkNone\",\"output\":{{\"kind\":\"NoOutput\"}}}},\"host_result_digest\":{}}}",
        serde_json::to_string(&digest).unwrap()
    );
    assert_eq!(stored(&value), expected.as_bytes());
    assert_eq!(restore(expected.as_bytes()).unwrap(), value);
}

fn who() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}
fn outputs(
    r: &mut MqHandleRegistry,
    owner: MqHandleOwner,
    conn: MqHconn,
) -> Vec<(MqMqiCall, MqMqiOutput)> {
    let obj = r.create_object(owner, conn).unwrap();
    let msg = r.create_message(owner, conn).unwrap();
    let sub = r.create_subscription(owner, conn).unwrap();
    let mut values = vec![
        (MqMqiCall::Connect, MqMqiOutput::Connected(conn)),
        (MqMqiCall::ConnectExtended, MqMqiOutput::Connected(conn)),
        (
            MqMqiCall::Open,
            MqMqiOutput::Opened {
                object: obj,
                dynamic: None,
            },
        ),
        (
            MqMqiCall::CreateMessageHandle,
            MqMqiOutput::MessageHandle(msg),
        ),
        (
            MqMqiCall::Subscribe,
            MqMqiOutput::Subscribed {
                object: obj,
                subscription: sub,
            },
        ),
    ];
    for kind in [MqRouteDynamicKind::Temporary, MqRouteDynamicKind::Permanent] {
        values.push((
            MqMqiCall::Open,
            MqMqiOutput::Opened {
                object: obj,
                dynamic: Some(MqDynamicQueueOpenResult {
                    handle: obj,
                    model: MqRouteName::new("MODEL.Queue").unwrap(),
                    name: MqRouteName::new("DYNAMIC.Queue_1").unwrap(),
                    kind,
                }),
            },
        ));
    }
    values
}
fn output(value: &MqMqiResult) -> &MqMqiOutput {
    match &value.outcome {
        MqMqiOutcome::Completed { output, .. } | MqMqiOutcome::StatusPending { output } => output,
        _ => panic!("original handle result must retain its output"),
    }
}
fn historical_roundtrip(value: &MqMqiResult) -> MqMqiResult {
    let limits = MqMqiLimits::default();
    let original = mq_mqi_result_bytes(value, limits).unwrap();
    let digest = canonical_result_digest(&host(value, limits)).unwrap();
    let size = canonical_result_size(&host(value, limits), limits.canonical_bytes).unwrap();
    let bytes = stored(value);
    let restored = restore(&bytes).unwrap();
    assert_eq!(mq_mqi_result_bytes(&restored, limits).unwrap(), original);
    assert_eq!(
        canonical_result_digest(&host(&restored, limits)).unwrap(),
        digest
    );
    assert_eq!(
        canonical_result_size(&host(&restored, limits), limits.canonical_bytes).unwrap(),
        size
    );
    assert_eq!(stored(&restored), bytes);
    assert_ne!(
        restored, *value,
        "authority equality must differ from canonical equality"
    );
    restored
}

#[test]
fn all_issued_outputs_keep_full_host_identity_metadata_and_no_live_authority() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let owner = who();
    let conn = r.connect(owner, MqHandleSharing::SharedBlock).unwrap();
    for (call, original) in outputs(&mut r, owner, conn) {
        for completed in [false, true] {
            let before = format!("{r:?}");
            let value = result(call, original.clone(), completed);
            let restored = historical_roundtrip(&value);
            // Structural host validation is unchanged and makes no permission claim.
            MqMqiHostResult {
                result: restored.clone(),
                limits: MqMqiLimits::default(),
            }
            .validate(HostLimits::default())
            .unwrap();
            match (output(&value), output(&restored)) {
                (MqMqiOutput::Connected(live), MqMqiOutput::Connected(past)) => {
                    assert!(past.is_historical());
                    assert_eq!(
                        r.validate_connection(owner, *past),
                        Err(MqHandleProblem::Historical)
                    );
                    let obs = MqHandleObservation::capture_connection(*past).unwrap();
                    assert_eq!(r.resolve_observed_connection(owner, obs), Ok(*live));
                }
                (
                    MqMqiOutput::Opened {
                        object: live,
                        dynamic: old,
                    },
                    MqMqiOutput::Opened {
                        object: past,
                        dynamic: new,
                    },
                ) => {
                    assert!(past.is_historical());
                    assert_eq!(
                        r.validate(owner, conn, (*past).into(), MqHandleKind::Object),
                        Err(MqHandleProblem::Historical)
                    );
                    assert_eq!(
                        r.resolve_observed_handle(
                            owner,
                            conn,
                            MqHandleObservation::from(MqHandle::Object(*past)),
                            MqHandleKind::Object
                        ),
                        Ok((*live).into())
                    );
                    match (old, new) {
                        (Some(a), Some(b)) => {
                            assert_eq!(a.model, b.model);
                            assert_eq!(a.name, b.name);
                            assert_eq!(a.kind, b.kind);
                            assert_eq!(b.handle, *past);
                            assert!(b.handle.is_historical());
                        }
                        (None, None) => {}
                        _ => panic!("dynamic metadata must be exact"),
                    }
                }
                (MqMqiOutput::MessageHandle(live), MqMqiOutput::MessageHandle(past)) => {
                    assert!(past.is_historical());
                    assert_eq!(
                        r.validate_message_property(owner, conn, (*past).into()),
                        Err(MqHandleProblem::Historical)
                    );
                    assert_eq!(
                        r.resolve_observed_handle(
                            owner,
                            conn,
                            MqHandleObservation::from(MqHandle::Message(*past)),
                            MqHandleKind::Message
                        ),
                        Ok((*live).into())
                    );
                }
                (
                    MqMqiOutput::Subscribed {
                        object: a,
                        subscription: b,
                    },
                    MqMqiOutput::Subscribed {
                        object: pa,
                        subscription: pb,
                    },
                ) => {
                    assert!(pa.is_historical());
                    assert!(pb.is_historical());
                    for (live, past, kind) in [
                        (
                            MqHandle::Object(*a),
                            MqHandle::Object(*pa),
                            MqHandleKind::Object,
                        ),
                        (
                            MqHandle::Subscription(*b),
                            MqHandle::Subscription(*pb),
                            MqHandleKind::Subscription,
                        ),
                    ] {
                        assert!(!r.is_live(past));
                        assert_eq!(
                            r.validate(owner, conn, past, kind),
                            Err(MqHandleProblem::Historical)
                        );
                        assert_eq!(
                            r.resolve_observed_handle(
                                owner,
                                conn,
                                MqHandleObservation::from(past),
                                kind
                            ),
                            Ok(live)
                        );
                    }
                }
                _ => panic!("call/output shape cannot change on successful decode"),
            }
            assert_eq!(
                format!("{r:?}"),
                before,
                "decode/capture/resolve cannot change registry"
            );
        }
    }
    assert_eq!(r.active_handles(), 4);
}

#[test]
fn decode_after_disconnect_epoch_and_foreign_reopen_never_recreates_authority() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let owner = who();
    let conn = r.connect(owner, MqHandleSharing::NonShared).unwrap();
    let fixtures = outputs(&mut r, owner, conn)
        .into_iter()
        .map(|(call, out)| stored(&result(call, out, true)))
        .collect::<Vec<_>>();
    r.disconnect(owner, conn).unwrap();
    r.advance_epoch(8).unwrap();
    let new = r.connect(owner, MqHandleSharing::NonShared).unwrap();
    let mut foreign = MqHandleRegistry::new(8, 16).unwrap();
    let fc = foreign.connect(owner, MqHandleSharing::NonShared).unwrap();
    for bytes in fixtures {
        let restored = restore(&bytes).unwrap();
        match output(&restored) {
            MqMqiOutput::Connected(c) => {
                assert!(c.is_historical());
                let obs = MqHandleObservation::capture_connection(*c).unwrap();
                assert_eq!(
                    r.resolve_observed_connection(owner, obs),
                    Err(MqHandleProblem::Stale)
                );
                assert_eq!(
                    foreign.resolve_observed_connection(owner, obs),
                    Err(MqHandleProblem::Stale)
                );
            }
            out => {
                let child = match out {
                    MqMqiOutput::Opened { object, .. } | MqMqiOutput::Subscribed { object, .. } => {
                        MqHandle::Object(*object)
                    }
                    MqMqiOutput::MessageHandle(m) => MqHandle::Message(*m),
                    _ => panic!("handle output expected"),
                };
                let kind = match child {
                    MqHandle::Object(_) => MqHandleKind::Object,
                    _ => MqHandleKind::Message,
                };
                let obs = MqHandleObservation::from(child);
                assert_eq!(
                    r.resolve_observed_handle(owner, new, obs, kind),
                    Err(MqHandleProblem::Stale)
                );
                assert_eq!(
                    foreign.resolve_observed_handle(owner, fc, obs, kind),
                    Err(MqHandleProblem::Stale)
                );
            }
        }
        assert_eq!(stored(&restored), bytes);
    }
    assert_eq!(r.active_handles(), 1);
    assert_eq!(foreign.active_handles(), 1);
}

#[test]
fn every_handle_field_is_required_strict_bounded_and_digest_bound() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let owner = who();
    let conn = r.connect(owner, MqHandleSharing::NonShared).unwrap();
    for (call, out) in outputs(&mut r, owner, conn) {
        let value = result(call, out, true);
        let v = as_value(&value);
        for bad in omissions(&v) {
            reject(&bad);
        }
        let output = v["outcome"]["output"].as_object().unwrap();
        for (field, payload) in output {
            if field == "kind" || payload.is_null() {
                continue;
            }
            let mut bad = v.clone();
            bad["outcome"]["output"][field]["live"] = json!(true);
            reject(&bad);
            if field == "dynamic" {
                for name in ["model_name", "queue_name"] {
                    let mut bad = v.clone();
                    bad["outcome"]["output"][field][name] =
                        json!("x".repeat(MQ_ROUTE_NAME_BYTES + 1));
                    reject(&bad);
                    assert!(
                        budget::preflight(
                            &serde_json::to_vec(&bad).unwrap(),
                            HostLimits::default(),
                            MqMqiLimits::default()
                        )
                        .is_err()
                    );
                }
                let mut bad = v.clone();
                bad["outcome"]["output"][field]["kind"] = json!({"Temporary":null});
                reject(&bad);
                continue;
            }
            for (part, new) in [
                ("registry", json!(0)),
                ("generation", json!(0)),
                ("epoch", json!(0)),
                ("slot", json!(MQ_MAX_HANDLE_SLOTS)),
                ("role", json!("Default")),
                ("role", json!({"Object":null})),
                ("registry", json!(-1)),
            ] {
                let mut bad = v.clone();
                bad["outcome"]["output"][field][part] = new;
                reject(&bad);
            }
            for part in ["registry", "slot", "generation", "epoch"] {
                let mut bad = v.clone();
                let current = bad["outcome"]["output"][field][part].as_u64().unwrap();
                bad["outcome"]["output"][field][part] = json!(current + 1);
                assert!(restore(&serde_json::to_vec(&bad).unwrap()).is_err());
            }
            let obj = serde_json::to_string(payload).unwrap();
            for part in ["role", "registry", "slot", "generation", "epoch"] {
                let mut duplicated = obj.clone();
                duplicated.insert_str(1, &format!("\"{part}\":{},", payload[part]));
                let raw = serde_json::to_string(&v)
                    .unwrap()
                    .replacen(&obj, &duplicated, 1);
                assert!(restore(raw.as_bytes()).is_err());
            }
        }
        let mut wrong = v.clone();
        wrong["call"] = json!("MQPUT");
        reject(&wrong);
        let bytes = stored(&value);
        assert_eq!(
            decode(
                &bytes,
                HostLimits::default(),
                MqMqiLimits::default(),
                bytes.len() - 1
            ),
            Err(ReplayError::Bounds)
        );
        assert_eq!(
            encode(
                &value,
                HostLimits::default(),
                MqMqiLimits::default(),
                bytes.len() - 1
            ),
            Err(ReplayError::Bounds)
        );
        assert!(restore(&[bytes.as_slice(), b" {}"].concat()).is_err());
    }
    assert_eq!(r.active_handles(), 4);
}

#[test]
fn coherent_wrong_roles_and_dynamic_identity_are_not_accepted_with_a_valid_digest() {
    let mut r = MqHandleRegistry::new(7, 16).unwrap();
    let owner = who();
    let conn = r.connect(owner, MqHandleSharing::NonShared).unwrap();
    let fixtures = outputs(&mut r, owner, conn);
    for (call, out) in &fixtures {
        let value = result(*call, out.clone(), true);
        let mut v = as_value(&value);
        let field = match out {
            MqMqiOutput::Connected(_) => "connection",
            MqMqiOutput::Opened { .. } | MqMqiOutput::Subscribed { .. } => "object",
            _ => "message",
        };
        v["outcome"]["output"][field]["role"] = json!("Subscription");
        assert_eq!(
            restore(&serde_json::to_vec(&v).unwrap()),
            Err(ReplayError::Handle(MqHandleProblem::WrongKind))
        );
    }
    let (call, mut out) = fixtures.last().unwrap().clone();
    let other = r.create_object(owner, conn).unwrap();
    if let MqMqiOutput::Opened {
        dynamic: Some(dynamic),
        ..
    } = &mut out
    {
        dynamic.handle = other;
    }
    let invalid = result(call, out, true);
    let mut v = as_value(&result(call, fixtures.last().unwrap().1.clone(), true));
    v["outcome"]["output"]["dynamic"]["handle"] =
        serde_json::to_value(MqHandleObservation::from(MqHandle::Object(other))).unwrap();
    coherent(&mut v, &invalid);
    assert!(matches!(
        restore(&serde_json::to_vec(&v).unwrap()),
        Err(ReplayError::Mqi(_))
    ));
    assert!(matches!(
        encode(
            &invalid,
            HostLimits::default(),
            MqMqiLimits::default(),
            BYTES
        ),
        Err(ReplayError::Mqi(_))
    ));
    for name in ["model_name", "queue_name"] {
        let mut bad = v.clone();
        bad["outcome"]["output"]["dynamic"][name] = json!("bad name");
        reject(&bad);
    }
}

#[test]
fn unassociated_and_default_bound_children_replay_only_as_historical_identities() {
    let mut r = MqHandleRegistry::new(7, 8).unwrap();
    let owner = MqHandleOwner {
        environment: MqHostEnvironment::ZosCics,
        ..who()
    };
    r.bind_cics_default(owner).unwrap();
    let obj = r.create_object(owner, MqHconn::Default).unwrap();
    let msg = r.create_message(owner, MqHconn::Unassociated).unwrap();
    for (call, out) in [
        (
            MqMqiCall::Open,
            MqMqiOutput::Opened {
                object: obj,
                dynamic: None,
            },
        ),
        (
            MqMqiCall::CreateMessageHandle,
            MqMqiOutput::MessageHandle(msg),
        ),
    ] {
        let restored = historical_roundtrip(&result(call, out, true));
        match output(&restored) {
            MqMqiOutput::Opened { object, .. } => {
                assert_eq!(
                    r.validate(
                        owner,
                        MqHconn::Default,
                        (*object).into(),
                        MqHandleKind::Object
                    ),
                    Err(MqHandleProblem::Historical)
                );
            }
            MqMqiOutput::MessageHandle(hmsg) => {
                assert_eq!(
                    r.validate_message_property(owner, MqHconn::Unassociated, (*hmsg).into()),
                    Err(MqHandleProblem::Historical)
                );
            }
            _ => panic!("exact shape expected"),
        }
    }
    assert_eq!(r.active_handles(), 3);
}
