use super::*;
use crate::canonical::{Canonical, encode};

fn canonical<T: Canonical>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    encode(value, b"", 4096, &mut |part| bytes.extend_from_slice(part)).unwrap();
    bytes
}

// Decode the four documented fields independently of the product encoder.
fn parts(bytes: &[u8], type_name: &str) -> (u64, u32, u64, u64) {
    let mut data = bytes;
    fn take<'a>(data: &mut &'a [u8], count: usize) -> &'a [u8] {
        let (head, tail) = data.split_at(count);
        *data = tail;
        head
    }
    fn text<'a>(data: &mut &'a [u8]) -> &'a str {
        assert_eq!(take(data, 1), [1]);
        let n = u64::from_le_bytes(take(data, 8).try_into().unwrap()) as usize;
        std::str::from_utf8(take(data, n)).unwrap()
    }
    fn u64_value(data: &mut &[u8]) -> u64 {
        assert_eq!(take(data, 1), [0x13]);
        u64::from_le_bytes(take(data, 8).try_into().unwrap())
    }
    if type_name == "MqConnectionId" {
        assert_eq!(take(&mut data, 1), [0x41]);
        assert_eq!(text(&mut data), "MqHconn");
        assert_eq!(text(&mut data), "Issued");
        assert_eq!(take(&mut data, 8), 1u64.to_le_bytes());
        assert_eq!(text(&mut data), "0");
    }
    assert_eq!(take(&mut data, 1), [0x40]);
    assert_eq!(text(&mut data), type_name);
    assert_eq!(take(&mut data, 8), 4u64.to_le_bytes());
    assert_eq!(text(&mut data), "epoch");
    let epoch = u64_value(&mut data);
    assert_eq!(text(&mut data), "generation");
    let generation = u64_value(&mut data);
    assert_eq!(text(&mut data), "registry");
    let registry = u64_value(&mut data);
    assert_eq!(text(&mut data), "slot");
    assert_eq!(take(&mut data, 1), [0x12]);
    let slot = u32::from_le_bytes(take(&mut data, 4).try_into().unwrap());
    assert!(data.is_empty());
    (registry, slot, generation, epoch)
}

#[test]
fn actual_issued_tokens_encode_every_component_with_distinct_roles() {
    let f = Fixture::new(7);
    let MqHconn::Issued(id) = f.connection else {
        panic!()
    };
    assert_eq!(
        parts(&canonical(&f.connection), "MqConnectionId"),
        id.canonical_parts()
    );
    assert_eq!(
        parts(&canonical(&f.object), "MqHobj"),
        f.object.canonical_parts()
    );
    assert_eq!(
        parts(&canonical(&f.subscription), "MqHsub"),
        f.subscription.canonical_parts()
    );
    assert_eq!(
        parts(&canonical(&f.handle), "MqHmsg"),
        f.handle.canonical_parts()
    );
    let all = [
        canonical(&f.connection),
        canonical(&f.object),
        canonical(&f.subscription),
        canonical(&f.handle),
        canonical(&MqHconn::Default),
        canonical(&MqHconn::Unassociated),
    ];
    assert_eq!(all.into_iter().collect::<BTreeSet<_>>().len(), 6);
    assert_eq!(f.registry.active_handles(), 4);
}

#[test]
fn stale_generation_and_epoch_do_not_collide_or_get_reconstructed() {
    let mut f = Fixture::new(7);
    let old = f.object;
    f.registry
        .release(owner(), f.connection, old.into(), MqHandleKind::Object)
        .unwrap();
    let next = f.registry.create_object(owner(), f.connection).unwrap();
    let a = old.canonical_parts();
    let b = next.canonical_parts();
    assert_eq!((a.0, a.1, a.3), (b.0, b.1, b.3));
    assert_eq!(b.2, a.2 + 1);
    assert_ne!(canonical(&old), canonical(&next));
    assert_eq!(
        f.registry
            .validate(owner(), f.connection, old.into(), MqHandleKind::Object),
        Err(MqHandleProblem::Stale)
    );
    // Even rejected stale identities remain digestable for intent/reconciliation.
    let request = |object| {
        envelope(MqMqiRequest::Put {
            connection: f.connection,
            object,
            put: put(),
        })
    };
    assert_ne!(
        mq_mqi_request_digest(&request(old)),
        mq_mqi_request_digest(&request(next))
    );
    macro_rules! reuse {
        ($field:ident, $create:ident, $kind:ident) => {{
            let old = f.$field;
            let before = canonical(&old);
            f.registry
                .release(owner(), f.connection, old.into(), MqHandleKind::$kind)
                .unwrap();
            let new = f.registry.$create(owner(), f.connection).unwrap();
            let a = old.canonical_parts();
            let b = new.canonical_parts();
            assert_eq!((a.0, a.1, a.3), (b.0, b.1, b.3));
            assert_eq!(b.2, a.2 + 1);
            assert_ne!(before, canonical(&new));
            assert_eq!(
                f.registry
                    .validate(owner(), f.connection, old.into(), MqHandleKind::$kind),
                Err(MqHandleProblem::Stale)
            );
            f.$field = new;
        }};
    }
    reuse!(handle, create_message, Message);
    reuse!(subscription, create_subscription, Subscription);
    let stale_message = canonical(&f.handle);
    let stale_subscription = canonical(&f.subscription);
    let stale_connection = canonical(&f.connection);
    f.registry.advance_epoch(8).unwrap();
    let c = f
        .registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let o = f.registry.create_object(owner(), c).unwrap();
    let s = f.registry.create_subscription(owner(), c).unwrap();
    let h = f.registry.create_message(owner(), c).unwrap();
    let MqHconn::Issued(c_id) = c else { panic!() };
    let MqHconn::Issued(old_id) = f.connection else {
        panic!()
    };
    assert_eq!(c_id.canonical_parts().0, old_id.canonical_parts().0);
    assert_eq!(c_id.canonical_parts().3, 8);
    assert_eq!(o.canonical_parts().3, 8);
    assert_eq!(s.canonical_parts().3, 8);
    assert_eq!(h.canonical_parts().3, 8);
    assert_ne!(stale_connection, canonical(&c));
    assert_ne!(canonical(&next), canonical(&o));
    assert_ne!(stale_message, canonical(&h));
    assert_ne!(stale_subscription, canonical(&s));
}

#[test]
fn foreign_registry_and_connection_slot_have_exact_distinct_identities() {
    let f = Fixture::new(7);
    let g = Fixture::new(7);
    let MqHconn::Issued(f_id) = f.connection else {
        panic!()
    };
    let MqHconn::Issued(g_id) = g.connection else {
        panic!()
    };
    for (a, b) in [
        (f.object.canonical_parts(), g.object.canonical_parts()),
        (
            f.subscription.canonical_parts(),
            g.subscription.canonical_parts(),
        ),
        (f.handle.canonical_parts(), g.handle.canonical_parts()),
        (f_id.canonical_parts(), g_id.canonical_parts()),
    ] {
        assert_ne!(a.0, b.0);
        assert_eq!((a.1, a.2, a.3), (b.1, b.2, b.3));
    }
    for (a, b) in f.requests().into_iter().zip(g.requests()) {
        if !matches!(
            a,
            MqMqiRequest::Connect(_) | MqMqiRequest::ConnectExtended(_)
        ) {
            assert_ne!(
                mq_mqi_request_digest(&envelope(a)),
                mq_mqi_request_digest(&envelope(b))
            );
        }
    }
    assert_eq!(
        f.registry
            .validate(owner(), f.connection, g.object.into(), MqHandleKind::Object),
        Err(MqHandleProblem::Stale)
    );
    let mut f = f;
    let next_object = f.registry.create_object(owner(), f.connection).unwrap();
    let a = f.object.canonical_parts();
    let b = next_object.canonical_parts();
    assert_eq!((a.0, a.2, a.3), (b.0, b.2, b.3));
    assert_ne!(a.1, b.1);
    assert_ne!(canonical(&f.object), canonical(&next_object));
}

#[test]
fn owner_fields_and_syncpoint_context_are_all_semantic_inputs() {
    let f = Fixture::new(7);
    let base = envelope(MqMqiRequest::Commit {
        connection: f.connection,
        unit: 1,
    });
    let hash = mq_mqi_request_digest(&base).unwrap();
    let mut variants = Vec::new();
    let mut v = base.clone();
    v.context.owner.host_id += 1;
    variants.push(v);
    let mut v = base.clone();
    v.context.owner.process_id += 1;
    variants.push(v);
    let mut v = base.clone();
    v.context.owner.thread_id += 1;
    variants.push(v);
    let mut v = base.clone();
    v.context.owner.task_id += 1;
    variants.push(v);
    let mut v = base.clone();
    v.context.owner.syncpoint_epoch += 1;
    variants.push(v);
    let mut v = base.clone();
    v.context.owner.environment = MqHostEnvironment::ZosCics;
    variants.push(v);
    let mut v = base.clone();
    v.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
    variants.push(v);
    let mut seen = BTreeSet::from([hash]);
    for v in variants {
        assert!(seen.insert(mq_mqi_request_digest(&v).unwrap()));
    }
    let mut wrong = owner();
    wrong.task_id += 1;
    assert_eq!(
        f.registry.validate_connection(wrong, f.connection),
        Err(MqHandleProblem::CrossOwner)
    );
    assert_eq!(base.review(), Ok(MqMqiPending::PublicDispatch));
}

#[test]
fn connection_slot_reuse_changes_generation_within_same_epoch() {
    let mut f = Fixture::new(7);
    let MqHconn::Issued(old) = f.connection else {
        panic!()
    };
    let old_bytes = canonical(&f.connection);
    f.registry.disconnect(owner(), f.connection).unwrap();
    let next = f
        .registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let MqHconn::Issued(id) = next else { panic!() };
    let a = old.canonical_parts();
    let b = id.canonical_parts();
    assert_eq!((a.0, a.1, a.3), (b.0, b.1, b.3));
    assert_eq!(b.2, a.2 + 1);
    assert_ne!(old_bytes, canonical(&next));
}
