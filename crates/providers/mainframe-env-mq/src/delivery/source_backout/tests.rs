use super::*;
use crate::delivery::full_message::tests::full;
use crate::delivery::tests::{catalog, kernel, message, name};

fn request(mode: MqGetMode, capacity: usize, truncation: MqTruncation) -> MqGetContract {
    MqGetContract {
        selection: Default::default(),
        mode,
        wait: MqWait::NoWait,
        truncation,
        buffer_capacity: capacity,
    }
}
fn setup(
    v: i32,
    cp: bool,
    persistent: bool,
    count: i32,
) -> (
    MqObjectCatalog,
    MqDeliveryKernel,
    QueueProfile,
    mainframe_env_host_api::mq_mqi::MqFullMessage,
) {
    let c = catalog();
    let mut m = full(v, cp, persistent);
    *count_mut(&mut m) = count;
    let p = QueueProfile::Complete {
        version: v,
        characters: m.descriptor.characters(),
    };
    let mut k = kernel(&c)
        .upgrade_profiles(&BTreeMap::from([(name("A"), p)]))
        .unwrap();
    k.put_full(&c, &name("A"), m.clone(), None).unwrap();
    (c, k, p, m)
}
fn restore(k: &MqDeliveryKernel, c: &MqObjectCatalog) -> MqDeliveryKernel {
    MqDeliveryKernel::decode_stored(
        &k.encode_live_checkpoint().unwrap(),
        c,
        k.limits,
        k.message_limits,
        k.default_persistence,
        false,
    )
    .unwrap()
}

#[test]
fn increments_once_saturates_and_preserves_every_other_observation() {
    // Diagnostic surrounding fields prove exact preservation only, not native
    // MQMD legality. Independently reviewed counters exercise this ONE policy.
    for v in [1, 2] {
        for cp in [false, true] {
            for persistent in [false, true] {
                for (before, after) in [(0, 1), (1, 2), (254, 255), (255, 255)] {
                    let (c, mut k, p, m) = setup(v, cp, persistent, before);
                    let got = k
                        .get_full(
                            &c,
                            &name("A"),
                            p,
                            &request(MqGetMode::Remove, 99, MqTruncation::Reject),
                            Some(7),
                        )
                        .unwrap();
                    assert_eq!(got.1, Some(m.clone()), "GET reports prior count");
                    k = restore(&k, &c);
                    assert_eq!(k.backout_complete_zos(7), Ok(MqDeliveryOutcome::Rejected));
                    let bytes = k.encode_live_checkpoint().unwrap();
                    assert_eq!(k.backout_complete_zos(7), Ok(MqDeliveryOutcome::Rejected));
                    assert_eq!(k.encode_live_checkpoint().unwrap(), bytes);
                    k = restore(&k, &c);
                    let mut expected = m;
                    *count_mut(&mut expected) = after;
                    assert_eq!(
                        k.get_full(
                            &c,
                            &name("A"),
                            p,
                            &request(MqGetMode::Remove, 99, MqTruncation::Reject),
                            None
                        )
                        .unwrap()
                        .1,
                        Some(expected)
                    );
                }
            }
        }
    }
}

#[test]
fn invalid_signed_counter_refuses_entire_mixed_unit_without_partial_change() {
    for invalid in [-1, i32::MIN, 256, i32::MAX] {
        let (c, mut k, p, mut m) = setup(2, false, true, 3);
        *count_mut(&mut m) = invalid;
        k.put_full(&c, &name("A"), m, None).unwrap();
        for _ in 0..2 {
            k.get_full(
                &c,
                &name("A"),
                p,
                &request(MqGetMode::Remove, 99, MqTruncation::Reject),
                Some(7),
            )
            .unwrap();
        }
        k.put_one(&c, &name("B"), message(b"discard"), Some(7))
            .unwrap();
        let before = k.clone();
        let bytes = k.encode_live_checkpoint().unwrap();
        assert_eq!(k.backout_complete_zos(7), Err(MqDeliveryError::Unsupported));
        assert_eq!(k, before);
        assert_eq!(k.encode_live_checkpoint().unwrap(), bytes);
        // Unchanged storage policy remains capable of preserving diagnostics.
        assert_eq!(k.backout(7), Ok(MqDeliveryOutcome::Rejected));
    }
}

#[test]
fn only_removed_gets_change_not_browse_rejected_truncation_puts_or_other_units() {
    let (c, mut k, p, m) = setup(1, false, true, 5);
    let (_, got, _) = k
        .get_full(
            &c,
            &name("A"),
            p,
            &request(MqGetMode::BrowseFirst, 99, MqTruncation::Reject),
            Some(8),
        )
        .unwrap();
    assert_eq!(got, Some(m.clone()));
    k.backout_complete_zos(8).unwrap();
    assert_eq!(
        k.queues[&name("A")].entries[0].message,
        Payload::Complete(m.clone())
    );
    k.get_full(
        &c,
        &name("A"),
        p,
        &request(MqGetMode::Remove, 1, MqTruncation::Reject),
        Some(9),
    )
    .unwrap();
    k.backout_complete_zos(9).unwrap();
    assert_eq!(
        k.queues[&name("A")].entries[0].message,
        Payload::Complete(m.clone())
    );
    k.get_full(
        &c,
        &name("A"),
        p,
        &request(MqGetMode::Remove, 1, MqTruncation::Accept),
        Some(10),
    )
    .unwrap();
    k.put_full(&c, &name("A"), m.clone(), Some(10)).unwrap();
    k.put_full(&c, &name("A"), m.clone(), Some(11)).unwrap();
    k.backout_complete_zos(10).unwrap();
    let mut expected = m.clone();
    *count_mut(&mut expected) = 6;
    assert_eq!(k.depth(&name("A")), Some(1));
    assert_eq!(
        k.queues[&name("A")].entries[0].message,
        Payload::Complete(expected)
    );
    assert_eq!(k.unit_outcome(11), MqDeliveryOutcome::Pending);
    k.commit(11).unwrap();
    assert_eq!(
        k.queues[&name("A")].entries[1].message,
        Payload::Complete(m)
    );
}

#[test]
fn legacy_partial_bytes_and_storage_full_backout_remain_exact() {
    let c = catalog();
    let mut k = kernel(&c);
    k.put_one(&c, &name("A"), message(b"partial"), None)
        .unwrap();
    k.get(
        &c,
        &name("A"),
        &request(MqGetMode::Remove, 99, MqTruncation::Reject),
        Some(7),
    )
    .unwrap();
    let mut old = k.clone();
    old.backout(7).unwrap();
    k.backout_complete_zos(7).unwrap();
    assert_eq!(k, old);
    assert_eq!(k.encode().unwrap(), old.encode().unwrap());
    assert_eq!(
        k.encode_live_checkpoint().unwrap(),
        old.encode_live_checkpoint().unwrap()
    );
    let (c, mut k, p, m) = setup(2, true, true, -31);
    k.get_full(
        &c,
        &name("A"),
        p,
        &request(MqGetMode::Remove, 99, MqTruncation::Reject),
        Some(7),
    )
    .unwrap();
    k.backout(7).unwrap();
    assert_eq!(
        k.get_full(
            &c,
            &name("A"),
            p,
            &request(MqGetMode::Remove, 99, MqTruncation::Reject),
            None
        )
        .unwrap()
        .1,
        Some(m)
    );
}

#[test]
fn prospective_finalization_quota_rolls_back_counter_and_queue_together() {
    let (c, mut k, p, _) = setup(2, false, true, 9);
    k.limits.finalized_units = 1;
    k.finalized.insert(90, true);
    k.get_full(
        &c,
        &name("A"),
        p,
        &request(MqGetMode::Remove, 99, MqTruncation::Reject),
        Some(7),
    )
    .unwrap();
    let before = k.clone();
    assert_eq!(
        k.backout_complete_zos(7),
        Err(MqDeliveryError::ResourceExhausted)
    );
    assert_eq!(k, before);
    assert_eq!(k.backout_complete_zos(0), Err(MqDeliveryError::InvalidUnit));
    assert_eq!(k, before);
    assert_eq!(
        k.backout_complete_zos(8),
        Ok(MqDeliveryOutcome::UnknownOutcome)
    );
    assert_eq!(k, before);
}
