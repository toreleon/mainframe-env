use super::tests::load_fixture;
use super::*;
use crate::delivery::full_message::tests::full;
use crate::delivery::tests::{catalog, kernel, name};
use mainframe_env_store::MemoryStore;
use serde_json::{Value, json};

#[test]
fn full_rows_coherent_corruption_cannot_bypass_single_checkpoint_validator() {
    let catalog = catalog();
    let mut k = kernel(&catalog);
    let message = full(2, true, true);
    let profile = QueueProfile::Complete {
        version: 2,
        characters: message.descriptor.characters(),
    };
    k = k
        .upgrade_profiles(&BTreeMap::from([(name("A"), profile)]))
        .unwrap();
    k.put_full(&catalog, &name("A"), message, None).unwrap();
    let store = MemoryStore::new(Default::default());
    let identity = DeliveryRowIdentity::new(&catalog, 3, 5).unwrap();
    let (_, rows) =
        DeliveryRows::initialize(&store, &k, &catalog, identity.clone(), Default::default())
            .unwrap()
            .into_parts();
    for case in 0..10 {
        let mut map = rows.records.clone();
        let row = map.get_mut(&key(QUEUE, "A")).unwrap();
        let mut v: Value = serde_json::from_slice(&row.payload).unwrap();
        match case {
            0 => {
                v["value"].as_object_mut().unwrap().remove("profile");
            }
            1 => v["value"]["profile"]["version"] = json!(1),
            2 => v["value"]["messages"][0]["persistent"] = json!(false),
            3 => v["value"]["messages"][0]["expires_at"] = json!(6),
            4 => v["value"]["messages"][0]["message"]["kind"] = json!("future"),
            5 => v["value"]["messages"][0]["message"]["value"]["md"] = json!([0, 1]),
            6 => v["value"]["messages"][0]["message"]["value"]["properties"][0]["extra"] = json!(9),
            7 => v["value"]["messages"][0]["id"] = json!(u64::MAX),
            8 => {
                v["value"]["messages"][0]["message"]["value"]["body"] =
                    json!(vec![0; k.message_limits.body_bytes + 1])
            }
            9 => v["value"] = json!({"name":"A","messages":[]}),
            _ => unreachable!(),
        }
        row.payload = serde_json::to_vec(&v).unwrap();
        // Recompute the legitimate object digest: these cases must fail typed
        // validation, rather than only fail because a checksum was left stale.
        let hash = digest_rows(&map, true);
        let meta = map.get_mut(&key(META, META_KEY)).unwrap();
        let mut metadata: Metadata = decode(meta).unwrap();
        metadata.rows_sha256 = hash;
        meta.payload = encode_object_row(META_KEY, &metadata).unwrap();
        assert!(
            DeliveryRows::restore(
                map.into_values().collect(),
                &catalog,
                identity.clone(),
                Default::default(),
                k.limits,
                k.message_limits,
                k.default_persistence
            )
            .is_err(),
            "coherent corruption {case}"
        );
    }
    assert!(
        store
            .list_provider_state_prefix(PREFIX, 10)
            .unwrap()
            .is_empty(),
        "pure plans and rejected restore never wrote"
    );
}

#[test]
fn empty_full_profile_snapshot_has_explicit_independent_golden_and_fence_preserves_v2_rows() {
    let c = catalog();
    let k = kernel(&c)
        .upgrade_profiles(&BTreeMap::from([(
            name("A"),
            QueueProfile::Complete {
                version: 1,
                characters:
                    mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding::AsciiCompatible,
            },
        )]))
        .unwrap();
    assert_eq!(k.encode_live_checkpoint().unwrap(),br#"{"schema_version":"mainframe-env.mq-delivery-live@2","manager":"QM","default_persistent":true,"tick":0,"next_id":1,"next_cursor":1,"queues":[{"name":"A","profile":{"kind":"complete","version":1,"characters":"ascii-compatible"},"messages":[]},{"name":"B","profile":{"kind":"partial"},"messages":[]}],"pending":[],"finalized":[],"cursors":[]}"#);
    let store = MemoryStore::new(Default::default());
    let identity = DeliveryRowIdentity::new(&c, 3, 5).unwrap();
    let (batch, rows) =
        DeliveryRows::initialize(&store, &k, &c, identity.clone(), Default::default())
            .unwrap()
            .into_parts();
    store.mutate_provider_states_atomic(batch).unwrap();
    let (batch, next) = rows.next_fence_delta().unwrap().into_parts();
    assert_eq!(batch.len(), 1);
    for (key, record) in &rows.records {
        if key.0 != META {
            assert_eq!(next.records.get(key), Some(record));
        }
    }
    store.mutate_provider_states_atomic(batch).unwrap();
    let (_, restored) = load_fixture(
        &store,
        &c,
        identity.next_fence().unwrap(),
        Default::default(),
        k.limits,
        k.message_limits,
        k.default_persistence,
    )
    .unwrap();
    assert_eq!(restored, k);
    assert!(
        load_fixture(
            &store,
            &c,
            identity,
            Default::default(),
            k.limits,
            k.message_limits,
            k.default_persistence
        )
        .is_err()
    );
}
