use super::*;
use crate::canonical::encode;
use crate::mq_status::{MqCompletion, MqReviewedStatus};

#[test]
fn reviewed_draft_schema_closes_the_new_projection_without_relabelling_old_catalogs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let read = |path| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap()
    };
    let schema = read("conformance/subsystems/mq/schemas/mq-structure-status-catalog.schema.json");
    let catalog = read("conformance/subsystems/mq/mq/structure-status-catalog.json");
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .unwrap();
    assert!(validator.is_valid(&catalog));
    let mut historical = catalog.clone();
    historical
        .as_object_mut()
        .unwrap()
        .remove("inquiry_local_type");
    // Existing @2 instances remain admitted as historical shapes. They cannot
    // generate the new contract: the generator separately requires its section.
    assert!(validator.is_valid(&historical));
    for mutation in 0..8 {
        let mut value = catalog.clone();
        match mutation {
            0 => value["inquiry_local_type"]["facts"][0]["decimal"] = 21.into(),
            1 => value["inquiry_local_type"]["facts"][1]["decimal"] = 2.into(),
            2 => value["inquiry_local_type"]["unknown"] = true.into(),
            3 => {
                value["inquiry_local_type"]["schema_version"] =
                    "mainframe-env.mq-inquiry-local-type-projection@2".into()
            }
            4 => value["schema_version"] = "mainframe-env.mq-structure-status-catalog@3".into(),
            5 => {
                value["inquiry_local_type"]
                    .as_object_mut()
                    .unwrap()
                    .remove("facts");
            }
            6 => value["inquiry_local_type"]["owned_limits"]["integer_slots"] = 257.into(),
            7 => value["inquiry_local_type"]["semantic_execution_credit"] = 1.into(),
            _ => unreachable!(),
        }
        assert!(!validator.is_valid(&value), "schema mutation {mutation}");
    }
}

fn checked(count: usize, capacity: i64) -> MqMqiLocalTypeInquiry {
    let f = Fixture::new(7);
    MqMqiLocalTypeInquiry::new(
        f.connection,
        f.object,
        &vec![20; count],
        capacity,
        0,
        MqMqiLimits::default(),
    )
    .unwrap()
}
fn result(integers: Vec<i32>, characters: Vec<u8>) -> MqMqiResult {
    MqMqiResult {
        call: MqMqiCall::Inquire,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Inquire, 0, 0).unwrap(),
            output: MqMqiOutput::Attributes {
                integers,
                characters,
            },
        },
    }
}
fn bytes(selector: MqMqiSelector) -> Vec<u8> {
    let mut bytes = Vec::new();
    encode(&selector, b"", 4096, &mut |part| {
        bytes.extend_from_slice(part)
    })
    .unwrap();
    bytes
}

#[test]
fn reviewed_number_does_not_promote_pending_and_unknown_signed_values_refuse() {
    assert_eq!(
        MqMqiSelector::reviewed_local_type(20)
            .unwrap()
            .reviewed_number(),
        Some(20)
    );
    assert_eq!(MqMqiSelector::PendingInteger(20).reviewed_number(), None);
    assert_eq!(MqMqiSelector::PendingCharacter(20).reviewed_number(), None);
    for value in [i64::MIN, -1, 0, 19, 21, 2016, i32::MAX as i64 + 1, i64::MAX] {
        assert_eq!(
            MqMqiSelector::reviewed_local_type(value),
            Err(MqMqiProblem::InquiryProfile)
        );
    }
}

#[test]
fn duplicate_occurrences_and_capacity_keep_distinct_original_request_identity() {
    let f = Fixture::new(7);
    let build = |numbers: &[i64], slots| {
        envelope(MqMqiRequest::Inquire(
            MqMqiLocalTypeInquiry::new(
                f.connection,
                f.object,
                numbers,
                slots,
                0,
                MqMqiLimits::default(),
            )
            .unwrap()
            .into_inquiry(),
        ))
    };
    let one = build(&[20], 1);
    let duplicated = build(&[20, 20], 2);
    let spare = build(&[20, 20], 3);
    let hashes = [one, duplicated, spare].map(|r| mq_mqi_request_digest(&r).unwrap());
    assert_eq!(hashes.into_iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(
        MqMqiLocalTypeInquiry::projection_sha256(),
        "0788fb791ee6a781efec7cb24a36362ab0aa86a65305c676d1c48ecaebe6311e"
    );
}

#[test]
fn independent_canonical_old_pending_literals_and_additive_reviewed_tag() {
    // Explicit published tags/lengths, independent of encoder-produced expectation.
    let pending_integer = b"\x41\x01\x0d\x00\x00\x00\x00\x00\x00\x00MqMqiSelector\x01\x0e\x00\x00\x00\x00\x00\x00\x00PendingInteger\x01\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\x00\x00\x00\x00\x00\x000\x1a\x14\x00\x00\x00";
    let pending_character = b"\x41\x01\x0d\x00\x00\x00\x00\x00\x00\x00MqMqiSelector\x01\x10\x00\x00\x00\x00\x00\x00\x00PendingCharacter\x01\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\x00\x00\x00\x00\x00\x000\x1a\x14\x00\x00\x00";
    let reviewed = b"\x41\x01\x0d\x00\x00\x00\x00\x00\x00\x00MqMqiSelector\x01\x11\x00\x00\x00\x00\x00\x00\x00ReviewedQueueType\x00\x00\x00\x00\x00\x00\x00\x00";
    assert_eq!(bytes(MqMqiSelector::PendingInteger(20)), pending_integer);
    assert_eq!(
        bytes(MqMqiSelector::PendingCharacter(20)),
        pending_character
    );
    assert_eq!(bytes(MqMqiSelector::ReviewedQueueType), reviewed);
}

#[test]
fn zero_duplicates_and_maximum_have_exact_complete_prefix_not_capacity_suffix() {
    for (count, capacity) in [(0, 0), (0, 256), (1, 3), (3, 3), (256, 256)] {
        let c = checked(count, capacity);
        assert_eq!(c.inquiry().selectors.len(), count);
        assert!(
            c.inquiry()
                .selectors
                .iter()
                .all(|s| *s == MqMqiSelector::ReviewedQueueType)
        );
        let r = result(vec![1; count], vec![]);
        assert_eq!(
            c.validate_result(&r, MqMqiLimits::default()).unwrap(),
            vec![1; count]
        );
        // A future writer copies only this returned prefix; no engine/storage is claimed.
        let mut caller = vec![87; capacity as usize];
        caller[..count].copy_from_slice(c.validate_result(&r, MqMqiLimits::default()).unwrap());
        assert!(caller[count..].iter().all(|v| *v == 87));
        if capacity as usize > count {
            assert!(
                c.validate_result(
                    &result(vec![1; capacity as usize], vec![]),
                    MqMqiLimits::default()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn checked_counts_limits_and_direct_assembly_refuse_short_mixed_or_other_profiles() {
    let f = Fixture::new(7);
    for (selectors, ints, chars) in [
        (vec![20; 257], 257, 0),
        (vec![20], 0, 0),
        (vec![], -1, 0),
        (vec![], 257, 0),
        (vec![20], i64::MAX, 0),
        (vec![20], 1, -1),
        (vec![20], 1, 1),
        (vec![21], 1, 0),
    ] {
        assert!(
            MqMqiLocalTypeInquiry::new(
                f.connection,
                f.object,
                &selectors,
                ints,
                chars,
                MqMqiLimits::default()
            )
            .is_err()
        );
    }
    for conn in [MqHconn::Default, MqHconn::Unassociated] {
        assert!(
            MqMqiLocalTypeInquiry::new(conn, f.object, &[], 0, 0, MqMqiLimits::default()).is_err()
        );
    }
    let base = checked(2, 3).into_inquiry();
    for selectors in [
        vec![MqMqiSelector::PendingInteger(20)],
        vec![
            MqMqiSelector::ReviewedQueueType,
            MqMqiSelector::PendingInteger(20),
        ],
        vec![MqMqiSelector::PendingCharacter(20)],
    ] {
        let mut v = base.clone();
        v.selectors = selectors;
        assert!(MqMqiLocalTypeInquiry::from_inquiry(v, MqMqiLimits::default()).is_err());
    }
    let limits = MqMqiLimits {
        selectors: 2,
        ..MqMqiLimits::default()
    };
    assert!(MqMqiLocalTypeInquiry::from_inquiry(base, limits).is_err());
}

#[test]
fn exact_status_class_scalar_cardinality_and_no_characters_are_mandatory() {
    let c = checked(2, 3);
    let limits = MqMqiLimits::default();
    for r in [
        result(vec![1], vec![]),
        result(vec![1, 1, 1], vec![]),
        result(vec![1, 2], vec![]),
        result(vec![1, -1], vec![]),
        result(vec![1, 1], vec![0]),
    ] {
        assert!(c.validate_result(&r, limits).is_err());
    }
    for outcome in [
        MqMqiOutcome::UnknownOutcome,
        MqMqiOutcome::StatusPending {
            output: MqMqiOutput::Attributes {
                integers: vec![1, 1],
                characters: vec![],
            },
        },
        MqMqiOutcome::ReviewedStatus {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Inquire, 0, 0).unwrap(),
        },
    ] {
        assert!(
            c.validate_result(
                &MqMqiResult {
                    call: MqMqiCall::Inquire,
                    outcome
                },
                limits
            )
            .is_err()
        );
    }
    for (completion, reason) in [(1, 2008), (1, 2022), (1, 2068), (2, 2065)] {
        let status =
            MqReviewedStatus::from_wire_pair(MqMqiCall::Inquire, completion, reason).unwrap();
        assert_ne!(status.completion(), MqCompletion::Ok);
        let r = MqMqiResult {
            call: MqMqiCall::Inquire,
            outcome: MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Attributes {
                    integers: vec![1, 1],
                    characters: vec![],
                },
            },
        };
        assert!(c.validate_result(&r, limits).is_err());
    }
    let mut r = result(vec![1, 1], vec![]);
    r.call = MqMqiCall::Set;
    assert!(c.validate_result(&r, limits).is_err());
}

#[test]
fn request_bound_checks_cover_direct_enum_assembly_and_pending_behavior_stays_old() {
    let inquiry = checked(2, 3).into_inquiry();
    let request = envelope(MqMqiRequest::Inquire(inquiry.clone()));
    assert!(request.validate().is_ok());
    assert_eq!(request.review().unwrap(), MqMqiPending::PublicDispatch);
    assert!(
        MqMqiResult::reviewed_output(
            MqReviewedStatus::from_wire_pair(MqMqiCall::Inquire, 0, 0).unwrap(),
            MqMqiOutput::Attributes {
                integers: vec![1, 1],
                characters: vec![]
            },
            &request
        )
        .is_ok()
    );
    assert!(
        result(vec![1, 1, 1], vec![])
            .validate_reviewed_output_for(&request.request)
            .is_err()
    );
    let mut mixed = inquiry.clone();
    mixed.selectors[1] = MqMqiSelector::PendingInteger(20);
    assert!(envelope(MqMqiRequest::Inquire(mixed)).validate().is_err());
    let mut pending = inquiry.clone();
    pending.selectors = vec![MqMqiSelector::PendingInteger(20); 2];
    pending.integer_capacity = 0;
    pending.character_capacity = 48;
    // Historical pending/short shape is not silently admitted or narrowed.
    assert!(envelope(MqMqiRequest::Inquire(pending)).validate().is_ok());
    let set = MqMqiSet {
        connection: inquiry.connection,
        object: inquiry.object,
        selectors: inquiry.selectors,
        integers: vec![1, 1],
        characters: vec![],
    };
    assert_eq!(
        envelope(MqMqiRequest::Set(set)).validate(),
        Err(MqMqiProblem::InquiryProfile)
    );
}
