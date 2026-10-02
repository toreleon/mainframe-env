use super::super::tests::{catalog, kernel, message, name};
use super::*;
use mainframe_env_host_api::mq_md_value::{MqMdFields, MqMdV2Fields};
use serde_json::{Value, json};

pub(crate) fn full(version: i32, cp: bool, persistent: bool) -> MqFullMessage {
    // Independent typed fixture: every selected MQMD field is explicit. No
    // decoder, initial-value generator or policy producer supplies expectations.
    let fields = MqMdFields {
        struc_id: if cp {
            [0xd4, 0xc4, 0x40, 0x40]
        } else {
            *b"MD  "
        },
        report: i32::MIN,
        msg_type: -94,
        expiry: -1,
        feedback: i32::MAX,
        encoding: -273,
        coded_char_set_id: -37,
        format: [0, 32, 255, 7, 9, 8, 3, 0],
        priority: -44,
        persistence: i32::from(persistent),
        msg_id: [0xa3; 24],
        correl_id: [0x19; 24],
        backout_count: -31,
        reply_to_q: [0x40; 48],
        reply_to_q_mgr: [0; 48],
        user_identifier: [0xff; 12],
        accounting_token: [0xa7; 32],
        appl_identity_data: [0; 32],
        put_appl_type: -29,
        put_appl_name: [0x40; 28],
        put_date: [0; 8],
        put_time: [0xff; 8],
        appl_origin_data: [3, 0, 4, 255],
    };
    let characters = if cp {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    let descriptor = if version == 1 {
        MqMdValue::V1 { characters, fields }
    } else {
        MqMdValue::V2 {
            characters,
            fields,
            extension: MqMdV2Fields {
                group_id: [0x31; 24],
                msg_seq_number: -16,
                offset: i32::MIN,
                msg_flags: i32::MAX,
                original_length: -4,
            },
        }
    };
    MqFullMessage {
        descriptor,
        body: vec![0, 255, 37, 0, 64],
        properties: vec![MqMessageProperty {
            name: "usr.exact".into(),
            kind: MqPropertyType::ByteString,
            value: vec![0, 255, 0],
        }],
    }
}
fn upgraded(version: i32, cp: bool) -> (MqObjectCatalog, MqDeliveryKernel, QueueProfile) {
    let c = catalog();
    let k = kernel(&c);
    let m = full(version, cp, true);
    let profile = QueueProfile::Complete {
        version,
        characters: m.descriptor.characters(),
    };
    let k = k
        .upgrade_profiles(&BTreeMap::from([(name("A"), profile)]))
        .unwrap();
    (c, k, profile)
}
fn request(capacity: usize) -> MqGetContract {
    MqGetContract {
        selection: Default::default(),
        mode: MqGetMode::Remove,
        wait: MqWait::NoWait,
        truncation: MqTruncation::Reject,
        buffer_capacity: capacity,
    }
}
fn restore(k: &MqDeliveryKernel, c: &MqObjectCatalog, cold: bool) -> MqDeliveryKernel {
    MqDeliveryKernel::decode_stored(
        &if cold {
            k.encode().unwrap()
        } else {
            k.encode_live_checkpoint().unwrap()
        },
        c,
        k.limits,
        k.message_limits,
        k.default_persistence,
        cold,
    )
    .unwrap()
}
#[test]
fn full_v1_v2_character_profiles_live_cold_exact_pending_cursor_final() {
    for v in [1, 2] {
        for cp in [false, true] {
            let (c, mut k, p) = upgraded(v, cp);
            let m = full(v, cp, true);
            let non = full(v, cp, false);
            k.put_full(&c, &name("A"), m.clone(), None).unwrap();
            k.put_full(&c, &name("A"), non.clone(), None).unwrap();
            let mut browse = request(99);
            browse.mode = MqGetMode::BrowseFirst;
            let (_, got, cursor) = k.get_full(&c, &name("A"), p, &browse, None).unwrap();
            assert_eq!(got, Some(m.clone()));
            assert!(cursor.is_some());
            let (_, got, _) = k
                .get_full(&c, &name("A"), p, &request(99), Some(11))
                .unwrap();
            assert_eq!(got, Some(m.clone()));
            k.put_full(&c, &name("A"), m.clone(), Some(12)).unwrap();
            k.put_full(&c, &name("A"), m.clone(), Some(13)).unwrap();
            k.backout(13).unwrap();
            // Empty pending unit remains a live unit, not silently finalized.
            k.get_full(&c, &name("A"), p, &request(99), Some(12))
                .unwrap();
            k.put_full(&c, &name("A"), m.clone(), Some(15)).unwrap();
            k.get_full(&c, &name("A"), p, &request(99), Some(15))
                .unwrap();
            assert!(k.pending[&15].is_empty());
            let live = restore(&k, &c, false);
            assert_eq!(
                live.encode_live_checkpoint().unwrap(),
                k.encode_live_checkpoint().unwrap(),
                "established projection sorts pending entries by identity, not call order"
            );
            let mut cold = restore(&k, &c, true);
            assert!(cold.pending.is_empty());
            assert!(cold.cursors.is_empty());
            assert_eq!(cold.finalized, k.finalized);
            assert_eq!(
                cold.get_full(&c, &name("A"), p, &request(99), None)
                    .unwrap()
                    .1,
                Some(m)
            );
            assert_eq!(cold.depth(&name("A")), Some(0));
            let mut recovered = live;
            recovered.recover_backout().unwrap();
            assert_eq!(recovered.depth(&name("A")), Some(2));
        }
    }
}
#[test]
fn incompatible_profiles_unknown_policy_and_browse_do_not_mutate() {
    let (c, mut k, p) = upgraded(2, true);
    k.put_full(&c, &name("A"), full(2, true, true), None)
        .unwrap();
    let old = k.clone();
    assert_eq!(
        k.put_one(&c, &name("A"), message(b"partial"), None),
        Err(MqDeliveryError::Unsupported)
    );
    assert_eq!(
        k.get(&c, &name("A"), &request(10), None),
        Err(MqDeliveryError::Unsupported)
    );
    for m in [full(1, true, true), full(2, false, true)] {
        assert_eq!(
            k.put_full(&c, &name("A"), m, None),
            Err(MqDeliveryError::Unsupported)
        );
    }
    let wrong = QueueProfile::Complete {
        version: 1,
        characters: MqMdCharacterEncoding::OwnedCp037,
    };
    assert!(
        k.get_full(&c, &name("A"), wrong, &request(10), None)
            .is_err()
    );
    for persistence in [-1, 2, 94] {
        let mut m = full(2, true, true);
        if let MqMdValue::V2 { fields, .. } = &mut m.descriptor {
            fields.persistence = persistence;
        }
        assert_eq!(
            k.put_full(&c, &name("A"), m, None),
            Err(MqDeliveryError::Unsupported)
        );
    }
    let mut m = full(2, true, true);
    if let MqMdValue::V2 { fields, .. } = &mut m.descriptor {
        fields.expiry = 0;
    }
    assert_eq!(
        k.put_full(&c, &name("A"), m, None),
        Err(MqDeliveryError::Unsupported)
    );
    assert_eq!(k, old);
    let (_, got, _) = k.get_full(&c, &name("A"), p, &request(0), Some(9)).unwrap();
    assert_eq!(got.unwrap().body, Vec::<u8>::new());
    assert_eq!(k, old, "rejected truncation preserves entry/UOW/counter");
    let mut accepted = request(2);
    accepted.truncation = MqTruncation::Accept;
    let (_, got, _) = k.get_full(&c, &name("A"), p, &accepted, Some(9)).unwrap();
    assert_eq!(got.unwrap().body, vec![0, 255]);
    k.backout(9).unwrap();
    assert_eq!(k.queues[&name("A")].entries, old.queues[&name("A")].entries);
}
#[test]
fn strict_v2_required_fields_schema_sql_bounds_and_old_reader_refusal() {
    let (c, mut k, _) = upgraded(1, false);
    k.put_full(&c, &name("A"), full(1, false, true), None)
        .unwrap();
    let bytes = k.encode_live_checkpoint().unwrap();
    assert_eq!(
        MqDeliveryKernel::decode_live_checkpoint(
            &bytes,
            &c,
            k.limits,
            k.message_limits,
            k.default_persistence
        ),
        Err(MqDeliveryError::UnsupportedSchema)
    );
    assert_eq!(
        MqDeliveryKernel::decode(
            &k.encode().unwrap(),
            &c,
            k.limits,
            k.message_limits,
            k.default_persistence
        ),
        Err(MqDeliveryError::UnsupportedSchema)
    );
    let mut cases = Vec::new();
    let state: Value = serde_json::from_slice(&bytes).unwrap();
    for key in ["profile", "messages"] {
        let mut s = state.clone();
        s["queues"][0].as_object_mut().unwrap().remove(key);
        cases.push(s);
    }
    for key in ["expires_at", "persistent", "message"] {
        let mut s = state.clone();
        s["queues"][0]["messages"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        cases.push(s);
    }
    for field in ["schema_version", "next_id", "next_cursor", "tick"] {
        let mut s = state.clone();
        s[field] = if field == "schema_version" {
            json!("mainframe-env.mq-delivery-live@3")
        } else {
            json!(u64::MAX)
        };
        cases.push(s);
    }
    let mut s = state.clone();
    s["queues"][0]["profile"]["version"] = json!(2);
    cases.push(s);
    let mut s = state.clone();
    s["queues"][0]["messages"][0]["persistent"] = json!(false);
    cases.push(s);
    let mut s = state.clone();
    s["queues"][0]["messages"][0]["expires_at"] = json!(10);
    cases.push(s);
    let mut s = state.clone();
    s["queues"][0]["messages"][0]["message"]["value"]["extra"] = json!(1);
    cases.push(s);
    let mut s = state.clone();
    s["queues"][0]["messages"][0]["message"]["value"]["md"][0] = json!(255);
    cases.push(s);
    for s in cases {
        assert!(
            MqDeliveryKernel::decode_stored(
                &serde_json::to_vec(&s).unwrap(),
                &c,
                k.limits,
                k.message_limits,
                k.default_persistence,
                false
            )
            .is_err()
        );
    }
    for bad in [
        format!("{} 0", String::from_utf8(bytes.clone()).unwrap()),
        String::from_utf8(bytes.clone()).unwrap().replacen(
            "\"tick\":0",
            "\"tick\":0,\"tick\":0",
            1,
        ),
    ] {
        assert!(
            MqDeliveryKernel::decode_stored(
                bad.as_bytes(),
                &c,
                k.limits,
                k.message_limits,
                k.default_persistence,
                false
            )
            .is_err()
        );
    }
    let mut narrow = k.limits;
    narrow.snapshot_bytes = bytes.len() - 1;
    assert!(
        MqDeliveryKernel::decode_stored(
            &bytes,
            &c,
            narrow,
            k.message_limits,
            k.default_persistence,
            false
        )
        .is_err()
    );
    let mut narrow = k.message_limits;
    narrow.body_bytes = 4;
    assert!(
        MqDeliveryKernel::decode_stored(&bytes, &c, k.limits, narrow, k.default_persistence, false)
            .is_err()
    );
}
#[test]
fn upgrade_rejects_entries_empty_units_cursors_and_prospective_json_overflow() {
    let c = catalog();
    let mut k = kernel(&c);
    k.put_one(&c, &name("A"), message(b"old"), None).unwrap();
    let p = QueueProfile::Complete {
        version: 1,
        characters: MqMdCharacterEncoding::AsciiCompatible,
    };
    assert!(
        k.upgrade_profiles(&BTreeMap::from([(name("A"), p)]))
            .is_err()
    );
    let old = k.clone();
    k.pending.insert(9, Vec::new());
    assert!(k.upgrade_profiles(&BTreeMap::new()).is_err());
    assert_eq!(k.pending[&9], Vec::new());
    k = old;
    let mut browse = request(99);
    browse.mode = MqGetMode::BrowseFirst;
    k.get(&c, &name("A"), &browse, None).unwrap();
    assert!(k.upgrade_profiles(&BTreeMap::new()).is_err());
    let (c, mut k, _) = upgraded(2, false);
    let baseline = k.encode_live_checkpoint().unwrap().len();
    k.limits.snapshot_bytes = baseline + 10;
    let old = k.clone();
    assert!(
        k.put_full(&c, &name("A"), full(2, false, true), None)
            .is_err()
    );
    assert_eq!(k, old);
}

#[test]
fn schema_one_cannot_export_or_cold_discard_complete_profile_or_payload() {
    let (c, mut k, _) = upgraded(2, false);
    k.put_full(&c, &name("A"), full(2, false, false), None)
        .unwrap();
    k.schema_two = false;
    assert!(
        k.encode().is_err(),
        "nonpersistent complete payload cannot disappear into an accepted @1 cold export"
    );
    assert!(k.encode_live_checkpoint().is_err());
    k.queues.get_mut(&name("A")).unwrap().entries.clear();
    assert!(
        k.encode_live_checkpoint().is_err(),
        "even an empty complete profile cannot become a partial queue through @1"
    );
}

#[test]
fn v2_clock_and_empty_unit_recovery_check_prospective_bytes_before_adoption() {
    let (_, mut k, _) = upgraded(1, false);
    k.limits.snapshot_bytes = k.encode_live_checkpoint().unwrap().len();
    let old = k.clone();
    assert_eq!(k.advance_tick(10), Err(MqDeliveryError::ResourceExhausted));
    assert_eq!(k, old);
    k.limits.snapshot_bytes = MqDeliveryLimits::default().snapshot_bytes;
    k.pending.insert(1, Vec::new());
    k.limits.snapshot_bytes = k.encode_live_checkpoint().unwrap().len();
    let old = k.clone();
    assert_eq!(k.recover_backout(), Err(MqDeliveryError::ResourceExhausted));
    assert_eq!(
        k, old,
        "empty unit cannot become an over-budget final decision"
    );
}
