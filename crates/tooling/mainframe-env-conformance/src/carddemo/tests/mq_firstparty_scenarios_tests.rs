use super::*;

// These controls use supplied synthetic projections, not native guest receipts.
// They intentionally cannot supply the original complete application proof.
fn fixtures() -> (Fixtures, SourceProjection) {
    (
        Fixtures::load().unwrap(),
        SourceProjection {
            fixture_sha256: FIXTURE_SHA.into(),
        },
    )
}
fn projection(f: &Fixtures, source: &SourceProjection, case: Case) -> MessageProjection {
    let spec = f.case(case, source).unwrap();
    let offer = f.offer(case, source).unwrap();
    let suffix = format!("/{}.cbl", offer.program);
    let pin = f
        .source_pins
        .iter()
        .find(|pin| pin.path.ends_with(&suffix))
        .unwrap();
    MessageProjection {
        program: offer.program,
        transaction: offer.transaction,
        source_sha256: pin.sha256.clone(),
        clock: offer.required_clock,
        input_queue: offer.input_queue,
        reply_queue: offer.reply_queue,
        request: offer.body,
        request_message_id: offer.message_id,
        request_correlation_id: offer.correlation_id,
        reply: decode_hex(&spec.expected_reply_hex, MAX_MESSAGE_BYTES).unwrap(),
        reply_message_id: offer.message_id,
        reply_correlation_id: offer.correlation_id,
    }
}

#[test]
fn deterministic_offers_preserve_firstparty_wire_shapes_and_source_literals() {
    let (f, s) = fixtures();
    for case in [
        Case::Date,
        Case::Account,
        Case::Approved,
        Case::InsufficientCredit,
        Case::MissingCrossReference,
    ] {
        assert_eq!(f.offer(case, &s).unwrap(), f.offer(case, &s).unwrap());
        let p = projection(&f, &s, case);
        let result = f.compare_message_projection(case, &s, &p).unwrap();
        assert_eq!(
            result.compared_message_bytes,
            p.request.len() + p.reply.len()
        );
    }
    let account = f.offer(Case::Account, &s).unwrap();
    assert_eq!(&account.body[..15], b"INQA00000000001");
    assert!(account.body[15..].iter().all(|byte| *byte == b' '));
    let date = projection(&f, &s, Case::Date);
    let prefix = b"SYSTEM DATE : 08-30-2026SYSTEM TIME : 12:00:00";
    assert_eq!(&date.reply[..prefix.len()], prefix);
    assert!(date.reply[prefix.len()..].iter().all(|byte| *byte == b' '));
    let auth = projection(&f, &s, Case::Approved);
    assert_eq!(
        auth.request.iter().filter(|byte| **byte == b',').count(),
        17
    );
    assert_eq!(&auth.reply[auth.reply.len() - 2..], b", ");
}

#[test]
fn supplied_source_set_refuses_missing_unknown_duplicate_and_changed_bytes() {
    let (mut f, _) = fixtures();
    assert!(matches!(f.compare_sources(&[]), Err(Refusal::Source)));
    // Explicit synthetic pin receiver exercises the same comparator, including
    // the later duplicate/unknown branches. It is not a corpus authentication.
    for (index, pin) in f.source_pins.iter_mut().enumerate() {
        pin.path = format!("synthetic-{index}");
        pin.bytes = 6;
        pin.sha256 = hex(b"source");
    }
    let names = f
        .source_pins
        .iter()
        .map(|pin| pin.path.clone())
        .collect::<Vec<_>>();
    let original = names
        .iter()
        .map(|name| (name.as_str(), b"source".as_slice()))
        .collect::<Vec<_>>();
    assert!(f.compare_sources(&original).is_ok());
    let mut duplicate = original.clone();
    duplicate[11] = duplicate[0];
    assert!(matches!(
        f.compare_sources(&duplicate),
        Err(Refusal::Source)
    ));
    let mut unknown = original.clone();
    unknown[11].0 = "unknown";
    assert!(matches!(f.compare_sources(&unknown), Err(Refusal::Source)));
    let mut changed = original;
    changed[11].1 = b"sourcf";
    assert!(matches!(f.compare_sources(&changed), Err(Refusal::Source)));
    let oversized = vec![0; MAX_SOURCE_BYTES + 1];
    let mut inputs = vec![("unknown", b"".as_slice()); 12];
    inputs[0] = (f.source_pins[0].path.as_str(), &oversized);
    assert!(matches!(f.compare_sources(&inputs), Err(Refusal::Bounds)));
}

#[test]
fn complete_request_and_reply_comparisons_refuse_every_tail_or_length_change() {
    let (f, s) = fixtures();
    for case in [Case::Date, Case::Account, Case::Approved] {
        let original = projection(&f, &s, case);
        let mut p = original.clone();
        let last = p.request.len() - 1;
        p.request[last] ^= 1;
        assert_eq!(
            f.compare_message_projection(case, &s, &p),
            Err(Refusal::Request)
        );
        let mut p = original.clone();
        let last = p.reply.len() - 1;
        p.reply[last] ^= 1;
        assert_eq!(
            f.compare_message_projection(case, &s, &p),
            Err(Refusal::Reply)
        );
        let mut p = original.clone();
        p.reply.pop();
        assert_eq!(
            f.compare_message_projection(case, &s, &p),
            Err(Refusal::Reply)
        );
        let mut p = original;
        p.request.push(0);
        assert!(f.compare_message_projection(case, &s, &p).is_err());
    }
}

#[test]
fn queues_clock_correlation_and_original_base_message_id_cannot_be_rebound() {
    let (f, s) = fixtures();
    let original = projection(&f, &s, Case::Date);
    for index in 0..8 {
        let mut p = original.clone();
        match index {
            0 => p.clock = "later".into(),
            1 => p.input_queue.push('X'),
            2 => p.reply_queue.push('X'),
            3 => p.request_message_id[0] ^= 1,
            4 => p.request_correlation_id[0] ^= 1,
            5 => p.reply_message_id[0] ^= 1,
            6 => p.reply_correlation_id[0] ^= 1,
            _ => p.transaction = "WRONG".into(),
        }
        assert_eq!(
            f.compare_message_projection(Case::Date, &s, &p),
            Err(Refusal::Identity)
        );
    }
}

#[test]
fn approved_and_declined_replies_cannot_substitute_for_each_other_or_a_new_owner() {
    let (f, s) = fixtures();
    let approved = projection(&f, &s, Case::Approved);
    let mut declined = projection(&f, &s, Case::InsufficientCredit);
    declined.reply = approved.reply;
    assert_eq!(
        f.compare_message_projection(Case::InsufficientCredit, &s, &declined),
        Err(Refusal::Reply)
    );
    let mut p = projection(&f, &s, Case::Approved);
    p.source_sha256 = "0".repeat(64);
    assert_eq!(
        f.compare_message_projection(Case::Approved, &s, &p),
        Err(Refusal::Source)
    );
    let mut p = projection(&f, &s, Case::Approved);
    p.program = "PROVIDER-PROBE".into();
    assert_eq!(
        f.compare_message_projection(Case::Approved, &s, &p),
        Err(Refusal::Source)
    );
}

#[test]
fn complete_seed_images_refuse_filler_key_missing_extra_and_neighbor_drift() {
    let (f, s) = fixtures();
    let original = f.offer(Case::Approved, &s).unwrap().seeds;
    f.compare_seed_images(Case::Approved, &s, &original)
        .unwrap();
    for index in 0..original.len() {
        let mut changed = original.clone();
        let last = changed[index].record.len() - 1;
        changed[index].record[last] ^= 1;
        assert_eq!(
            f.compare_seed_images(Case::Approved, &s, &changed),
            Err(Refusal::Seed)
        );
        let mut changed = original.clone();
        changed[index].key[0] ^= 1;
        assert_eq!(
            f.compare_seed_images(Case::Approved, &s, &changed),
            Err(Refusal::Seed)
        );
    }
    let mut missing = original.clone();
    missing.pop();
    assert_eq!(
        f.compare_seed_images(Case::Approved, &s, &missing),
        Err(Refusal::Seed)
    );
    let mut extra = original.clone();
    extra.push(original[0].clone());
    assert_eq!(
        f.compare_seed_images(Case::Approved, &s, &extra),
        Err(Refusal::Seed)
    );
    let mut reordered = original;
    reordered.swap(0, 1);
    assert_eq!(
        f.compare_seed_images(Case::Approved, &s, &reordered),
        Err(Refusal::Seed)
    );
}

#[test]
fn hex_and_message_bounds_refuse_before_unbounded_allocation_or_projection_success() {
    assert_eq!(decode_hex("0", 1), Err(Refusal::Bounds));
    assert_eq!(decode_hex("FF", 1), Err(Refusal::Fixture));
    assert_eq!(decode_hex(&"00".repeat(501), 500), Err(Refusal::Bounds));
    let (f, s) = fixtures();
    let mut p = projection(&f, &s, Case::Approved);
    p.reply.resize(MAX_MESSAGE_BYTES + 1, 0);
    assert_eq!(
        f.compare_message_projection(Case::Approved, &s, &p),
        Err(Refusal::Bounds)
    );
    let invalid = SourceProjection {
        fixture_sha256: "0".repeat(64),
    };
    assert_eq!(f.offer(Case::Date, &invalid), Err(Refusal::Source));
}

#[test]
fn authorization_projection_does_not_claim_authentication_of_issued_reply_message_id() {
    let (f, s) = fixtures();
    let mut p = projection(&f, &s, Case::Approved);
    p.reply_message_id = [0x5a; 24];
    assert!(
        !f.compare_message_projection(Case::Approved, &s, &p)
            .unwrap()
            .reply_message_id_checked
    );
    // This explicitly unverified field must later be joined to the real MQI
    // result; neither a projection nor a provider counter yields any token.
}
