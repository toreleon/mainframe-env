use super::*;
fn catalog() -> MqObjectCatalog {
    MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: MqObjectName::new("QM").unwrap(),
            default_transmission_queue: None,
        },
        vec![MqObjectDefinition::LocalQueue {
            name: MqObjectName::new("Q").unwrap(),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: None,
        }],
        Default::default(),
    )
    .unwrap()
}
fn attrs() -> MqNativeAttributes {
    MqNativeAttributes {
        coded_char_set_id: 819,
        characters: MqNativeCharacters::Ascii819,
        max_msg_length: 32768,
        max_priority: i32::MAX,
        queues: vec![MqNativeQueueAttributes {
            name: MqObjectName::new("Q").unwrap(),
            max_msg_length: 0,
            delivery_sequence: MqNativeDeliverySequence::Fifo,
        }],
    }
}
#[test]
fn catalog_native2_explicit_domains_exact_old1_and_roundtrip() {
    let old = catalog().encode().unwrap();
    assert_eq!(old,br#"{"schema_version":"mainframe-env.mq-object-catalog@1","queue_manager":{"name":"QM","default_transmission_queue":null},"objects":[{"kind":"local-queue","name":"Q","usage":"normal","trigger_process":null}],"model_instances":[],"next_dynamic_id":1}"#);
    for cp in [false, true] {
        for max in [32768, 104857600] {
            let mut a = attrs();
            a.max_msg_length = max;
            if cp {
                a.coded_char_set_id = 37;
                a.characters = MqNativeCharacters::OwnedCp037;
            }
            let c = catalog().with_native_attributes(a.clone()).unwrap();
            let bytes = c.encode().unwrap();
            let decoded = MqObjectCatalog::decode(&bytes, Default::default()).unwrap();
            assert_eq!(decoded.native_attributes(), Some(&a));
            assert_eq!(decoded.encode().unwrap(), bytes);
            assert_eq!(catalog().encode().unwrap(), old);
        }
    }
    for i in 0..9 {
        let mut a = attrs();
        match i {
            0 => a.coded_char_set_id = 500,
            1 => a.max_msg_length = 32767,
            2 => a.max_msg_length = 104857601,
            3 => a.max_priority = -1,
            4 => a.queues[0].max_msg_length = -1,
            5 => a.queues[0].max_msg_length = 104857601,
            6 => a.queues.clear(),
            7 => a.queues[0].name = MqObjectName::new("OTHER").unwrap(),
            _ => a.queues.push(a.queues[0].clone()),
        }
        assert!(catalog().with_native_attributes(a).is_err());
    }
}
#[test]
fn catalog_native2_strict_required_unknown_duplicate_schema_counts_and_bytes() {
    let bytes = catalog()
        .with_native_attributes(attrs())
        .unwrap()
        .encode()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for field in [
        "schema_version",
        "queue_manager",
        "objects",
        "model_instances",
        "next_dynamic_id",
        "native_attributes",
    ] {
        let mut wrong = v.clone();
        wrong.as_object_mut().unwrap().remove(field);
        assert!(
            MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), Default::default())
                .is_err()
        );
    }
    for field in [
        "coded_char_set_id",
        "characters",
        "max_msg_length",
        "max_priority",
        "queues",
    ] {
        let mut wrong = v.clone();
        wrong["native_attributes"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), Default::default())
                .is_err()
        );
    }
    for (section, field) in [
        ("queue_manager", "default_transmission_queue"),
        ("objects", "trigger_process"),
    ] {
        let mut wrong = v.clone();
        let target = if section == "objects" {
            &mut wrong[section][0]
        } else {
            &mut wrong[section]
        };
        target.as_object_mut().unwrap().remove(field);
        assert!(
            MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), Default::default())
                .is_err()
        );
    }
    for schema in [
        MQ_OBJECT_CATALOG_SCHEMA,
        "mainframe-env.mq-object-catalog@3",
    ] {
        let mut wrong = v.clone();
        wrong["schema_version"] = serde_json::json!(schema);
        assert!(
            MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), Default::default())
                .is_err()
        );
    }
    let mut wrong = v.clone();
    wrong["native_attributes"]["extra"] = serde_json::json!(0);
    assert!(
        MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), Default::default()).is_err()
    );
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        MqObjectCatalog::decode(
            text.replace("\"max_priority\":", "\"max_priority\":0,\"max_priority\":")
                .as_bytes(),
            Default::default()
        )
        .is_err()
    );
    assert!(
        MqObjectCatalog::decode(&[bytes.as_slice(), b" null"].concat(), Default::default())
            .is_err()
    );
    let mut limits = MqObjectLimits::default();
    limits.max_persisted_bytes = bytes.len() - 1;
    assert!(MqObjectCatalog::decode(&bytes, limits).is_err());
    let mut wrong = v;
    wrong["native_attributes"]["queues"] =
        serde_json::json!(vec![wrong["native_attributes"]["queues"][0].clone(); 2]);
    let mut limits = MqObjectLimits::default();
    limits.max_objects = 1;
    assert!(MqObjectCatalog::decode(&serde_json::to_vec(&wrong).unwrap(), limits).is_err());
}
