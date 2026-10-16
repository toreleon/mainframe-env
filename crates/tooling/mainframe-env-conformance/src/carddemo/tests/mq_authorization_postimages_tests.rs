//! Synthetic precondition/comparison controls; no provider or application credit.
use super::*;
use sha2::{Digest, Sha256};

const ORIGINAL: &str = include_str!("mq_firstparty_scenarios/fixture.json");
const CONDITIONAL: &str = include_str!("mq_authorization_postimages/conditional-fixture.json");
fn conditional() -> serde_json::Value {
    assert!(CONDITIONAL.len() <= 65_536);
    assert_eq!(
        format!("{:x}", Sha256::digest(CONDITIONAL)),
        "074319f937ff7a52f577da79d92114236c691eff7e12bcf6712af5303b50a60d"
    );
    serde_json::from_str(CONDITIONAL).unwrap()
}
fn decode(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn reference(case: Case) -> (RequestOffer, Vec<u8>) {
    assert_eq!(
        format!("{:x}", Sha256::digest(ORIGINAL)),
        "c47edb82bdfe95caaceabbec4a02195016e09015cb9a551f2d0fb7c3643ef3d3"
    );
    let root: serde_json::Value = serde_json::from_str(ORIGINAL).unwrap();
    let id = match case {
        Case::Approved => "j16-approved",
        Case::InsufficientCredit => "j16-insufficient",
        Case::MissingCrossReference => "j16-missing-xref",
        _ => panic!("J16 case required"),
    };
    let row = root["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    let text = |key: &str| row[key].as_str().unwrap();
    let seeds = row["seed_images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| SeedImage {
            dataset: r["dataset"].as_str().unwrap().into(),
            key: decode(r["key_hex"].as_str().unwrap()),
            record: decode(r["record_hex"].as_str().unwrap()),
        })
        .collect();
    (
        RequestOffer {
            program: text("program").into(),
            transaction: text("transaction").into(),
            input_queue: text("input_queue").into(),
            reply_queue: text("reply_queue").into(),
            body: decode(text("request_hex")),
            message_id: decode(text("message_id_hex")).try_into().unwrap(),
            correlation_id: decode(text("correlation_id_hex")).try_into().unwrap(),
            required_clock: root["clock"].as_str().unwrap().into(),
            seeds,
        },
        decode(text("expected_reply_hex")),
    )
}
fn initial() -> Before {
    let fixture = conditional();
    let context = &fixture["required_context"];
    // Source-authored current-compiler initialization expectation. Its actual
    // compiled receipt remains NULL; this constructor proves no guest behavior.
    Before {
        summary_work: decode(context["compiled_initial_summary_hex"].as_str().unwrap()),
        detail_work: decode(context["compiled_initial_detail_hex"].as_str().unwrap()),
        hierarchy: Vec::new(),
        previous_summary_found: false,
        yyddd: 26_242,
        hhmmss: 120_000,
        milliseconds: 0,
    }
}
fn approved() -> (RequestOffer, Projection) {
    let offer = reference(Case::Approved).0;
    let result = derive(Case::Approved, &offer, &initial()).unwrap();
    (offer, result)
}
fn synthetic_writes(projection: &Projection) -> Vec<ImsRequest> {
    projection
        .conditional_writes
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let names = image
                .segments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let qualifiers = image
                .root_key
                .as_ref()
                .map(|key| {
                    vec![ImsQualifier {
                        segment: "PAUTSUM0".into(),
                        field: "ACCNTID".into(),
                        value: key.clone(),
                    }]
                })
                .unwrap_or_default();
            let mut request = crate::carddemo::ims_request(
                image.operation,
                index as u64 + 1,
                None,
                &names,
                image.data.clone(),
                qualifiers,
                None,
            )
            .unwrap();
            request.max_segments = 1;
            request
        })
        .collect()
}

#[test]
fn fresh_approved_complete_images_preserve_vsam_and_source_amount_distinction() {
    let (offer, result) = approved();
    let fixture = conditional();
    let expected = &fixture["approved_fresh"];
    assert_eq!(
        result.summary_work,
        decode(expected["summary_hex"].as_str().unwrap())
    );
    assert_eq!(
        result.detail_work,
        decode(expected["detail_hex"].as_str().unwrap())
    );
    assert_eq!(result.unchanged_vsam, offer.seeds);
    assert_eq!(result.summary_work.len(), 100);
    assert_eq!(result.detail_work.len(), 200);
    assert_eq!(result.summary_work[..6], [0, 0, 0, 0, 0, 0x1c]);
    assert_eq!(result.summary_work[6..15], *b"000000001");
    assert_eq!(result.summary_work[26..32], [0, 0, 1, 0, 0, 0x0c]); // 1000.00 limit
    assert_eq!(result.summary_work[38..44], [0, 0, 0, 0x10, 0, 0x0c]); // ONLY new100.00
    assert_eq!(result.summary_work[50..54], [0, 1, 0, 0]);
    assert_eq!(result.summary_work[66..], [b' '; 34]);
    assert_eq!(
        result.detail_work[..8],
        [0x73, 0x75, 0x7c, 0x87, 0x99, 0x99, 0x99, 0x9c]
    );
    assert_eq!(result.detail_work[173..183], *b"P         ");
    assert_eq!(result.detail_work[183..], [b' '; 17]);
    assert_eq!(result.conditional_writes[0].operation, ImsOperation::Insert);
    assert_eq!(
        result.conditional_writes[1].segments,
        ["PAUTSUM0", "PAUTDTL1"]
    );
    assert_eq!(
        result.conditional_writes[1].root_key.as_deref(),
        Some(&[0, 0, 0, 0, 0, 0x1c][..])
    );
    let rows = result.conditional_success_hierarchy.as_ref().unwrap();
    result
        .compare_complete_postimages(&offer.seeds, rows)
        .unwrap();
    let writes = synthetic_writes(&result);
    result
        .compare_write_offers(&writes.iter().collect::<Vec<_>>())
        .unwrap();
    let mut swapped = writes.clone();
    swapped.reverse();
    assert_eq!(
        result.compare_write_offers(&swapped.iter().collect::<Vec<_>>()),
        Err(Refusal::Postimage)
    );
    for at in 0..writes.len() {
        let mut changed = writes.clone();
        changed[at].pcb = 2;
        assert_eq!(
            result.compare_write_offers(&changed.iter().collect::<Vec<_>>()),
            Err(Refusal::Postimage)
        );
        let mut changed = writes.clone();
        changed[at].mutation = None;
        assert_eq!(
            result.compare_write_offers(&changed.iter().collect::<Vec<_>>()),
            Err(Refusal::Postimage)
        );
        for offset in 0..writes[at].data.len() {
            let mut changed = writes.clone();
            changed[at].data[offset] ^= 1;
            assert_eq!(
                result.compare_write_offers(&changed.iter().collect::<Vec<_>>()),
                Err(Refusal::Postimage)
            );
        }
    }
    // Full retained bytes, including filler, are compared across every image.
    for index in 0..offer.seeds.len() {
        for offset in 0..offer.seeds[index].record.len() {
            let mut wrong = offer.seeds.clone();
            wrong[index].record[offset] ^= 1;
            assert_eq!(
                result.compare_complete_postimages(&wrong, rows),
                Err(Refusal::Postimage)
            );
        }
    }
    for offset in 0..100 {
        let mut wrong = rows.clone();
        wrong[0].data[offset] ^= 1;
        assert_eq!(
            result.compare_complete_postimages(&offer.seeds, &wrong),
            Err(Refusal::Postimage)
        );
    }
    for offset in 0..200 {
        let mut wrong = rows.clone();
        wrong[0].children[0][offset] ^= 1;
        assert_eq!(
            result.compare_complete_postimages(&offer.seeds, &wrong),
            Err(Refusal::Postimage)
        );
    }
}

#[test]
fn decline_uses_explicit_prior_detail_amount_not_current_request_and_preserves_neighbors() {
    let (_, first) = approved();
    let offer = reference(Case::InsufficientCredit).0;
    let mut before = initial();
    before.summary_work = first.summary_work.clone();
    before.detail_work = first.detail_work.clone();
    before.hierarchy = first.conditional_success_hierarchy.clone().unwrap();
    before.previous_summary_found = true;
    before.milliseconds = 1;
    let mut neighbor = before.hierarchy[0].clone();
    neighbor.data[5] = 0x2c;
    neighbor.data[66..].fill(b'N');
    before.hierarchy.push(neighbor.clone());
    let second = derive(Case::InsufficientCredit, &offer, &before).unwrap();
    let fixture = conditional();
    let expected = &fixture["insufficient_after_actual_prior_approved"];
    assert_eq!(
        second.summary_work,
        decode(expected["summary_hex"].as_str().unwrap())
    );
    assert_eq!(
        second.detail_work,
        decode(expected["detail_hex"].as_str().unwrap())
    );
    assert_eq!(
        second.conditional_writes[0].operation,
        ImsOperation::Replace
    );
    assert_eq!(second.summary_work[50..54], [0, 1, 0, 1]);
    assert_eq!(packed(&second.summary_work[60..66]).unwrap(), 10_000); // prior100.00, not new1000.00
    assert_eq!(packed(&second.summary_work[38..44]).unwrap(), 10_000);
    assert_eq!(packed(&second.detail_work[74..81]).unwrap(), 100_000);
    assert_eq!(packed(&second.detail_work[81..88]).unwrap(), 0);
    assert_eq!(second.detail_work[62..68], *b"054100");
    let rows = second.conditional_success_hierarchy.as_ref().unwrap();
    assert_eq!(rows[1], neighbor);
    assert_eq!(rows[0].children.len(), 2);
    assert_eq!(rows[0].children[0], second.detail_work); // reverse-time key ordering
    second
        .compare_complete_postimages(&offer.seeds, rows)
        .unwrap();
    assert_eq!(
        second.unproved_observations(),
        ["duplicate/idempotent request", "cross-resource commit"]
    );
}

#[test]
fn fresh_decline_cannot_invent_valid_zero_for_unassigned_packed_detail() {
    let offer = reference(Case::InsufficientCredit).0;
    assert!(matches!(
        derive(Case::InsufficientCredit, &offer, &initial()),
        Err(Refusal::Decimal)
    ));
    let mut explicit = initial();
    put_packed(&mut explicit.detail_work[74..81], 0, 12).unwrap();
    // Explicit valid input is a different precondition, never inferred for the
    // unmodified first instance. This result still has no execution authority.
    let conditional = derive(Case::InsufficientCredit, &offer, &explicit).unwrap();
    assert_eq!(packed(&conditional.summary_work[60..66]).unwrap(), 0);
}

#[test]
fn missing_xref_writes_nothing_but_stale_summary_branch_is_not_assumed_reset() {
    let offer = reference(Case::MissingCrossReference).0;
    let (_, first) = approved();
    let mut before = initial();
    before.hierarchy = first.conditional_success_hierarchy.unwrap();
    let missing = derive(Case::MissingCrossReference, &offer, &before).unwrap();
    assert!(missing.conditional_writes.is_empty());
    missing
        .compare_complete_postimages(&offer.seeds, &before.hierarchy)
        .unwrap();
    before.previous_summary_found = true;
    assert!(matches!(
        derive(Case::MissingCrossReference, &offer, &before),
        Err(Refusal::StaleSummaryBranch)
    ));
}

#[test]
fn duplicate_clock_key_never_mints_idempotence_or_unknown_post_error_state() {
    let (offer, first) = approved();
    let mut before = initial();
    before.hierarchy = first.conditional_success_hierarchy.clone().unwrap();
    before.summary_work = first.summary_work;
    before.detail_work = first.detail_work;
    before.previous_summary_found = true;
    let collision = derive(Case::Approved, &offer, &before).unwrap();
    assert_eq!(packed(&collision.summary_work[38..44]).unwrap(), 20_000);
    assert!(collision.conditional_success_hierarchy.is_none());
    assert_eq!(
        collision.compare_complete_postimages(&offer.seeds, &before.hierarchy),
        Err(Refusal::DuplicateNeedsDisposition)
    );
    before.milliseconds = 1;
    let later = derive(Case::Approved, &offer, &before).unwrap();
    assert_eq!(
        later.conditional_success_hierarchy.as_ref().unwrap()[0]
            .children
            .len(),
        2
    );
    assert_eq!(packed(&later.summary_work[38..44]).unwrap(), 20_000);
    assert_eq!(
        later.unproved_observations(),
        ["duplicate/idempotent request", "cross-resource commit"]
    );
}

#[test]
fn reply_pointer_and_buffer_carry_without_reset_and_overflow_is_not_predicted() {
    let first = reference(Case::Approved).1;
    let second = reference(Case::InsufficientCredit).1;
    let (buffer, pointer, message) = append_reply(&[b' '; 200], 1, &first).unwrap();
    assert_eq!(message, first);
    assert_eq!(pointer, first.len());
    let (_, pointer2, message2) = append_reply(&buffer, pointer, &second).unwrap();
    let mut expected = first[..first.len() - 1].to_vec();
    expected.extend_from_slice(&second);
    assert_eq!(message2, expected);
    assert_eq!(pointer2, first.len() + second.len() - 1);
    let fixture = conditional();
    let second_reference = &fixture["insufficient_after_actual_prior_approved"];
    assert_eq!(
        message2,
        decode(second_reference["reply_hex"].as_str().unwrap())
    );
    assert_eq!(
        pointer2,
        second_reference["reply_pointer"].as_u64().unwrap() as usize
    );
    assert_ne!(message2, second);
    assert_eq!(append_reply(&buffer, 200, &second), Err(Refusal::Bounds));
    assert_eq!(append_reply(&buffer, 0, &second), Err(Refusal::Fixture));
}
