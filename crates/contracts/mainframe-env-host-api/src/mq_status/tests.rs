use super::*;
use crate::mq_mqi::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn reviewed_completion_wire_constants_reject_unknown_and_wide_inputs() {
    for (completion, number, symbol) in [
        (MqCompletion::Ok, 0, "MQCC_OK"),
        (MqCompletion::Warning, 1, "MQCC_WARNING"),
        (MqCompletion::Failed, 2, "MQCC_FAILED"),
    ] {
        assert_eq!(completion.wire_number(), number);
        assert_eq!(completion.symbol(), symbol);
        assert_eq!(
            MqCompletion::from_wire_number(i64::from(number)),
            Ok(completion)
        );
    }
    for number in [-1, -2, i64::from(i32::MIN), 3, i64::from(i32::MAX)] {
        assert_eq!(
            MqCompletion::from_wire_number(number),
            Err(MqStatusProblem::UnknownCompletionNumber)
        );
    }
    for number in [
        i64::MIN,
        i64::from(i32::MIN) - 1,
        i64::from(i32::MAX) + 1,
        i64::MAX,
    ] {
        assert_eq!(
            MqCompletion::from_wire_number(number),
            Err(MqStatusProblem::NumericOutOfRange)
        );
    }
    assert_eq!(
        MQ_COMPLETION_WIRE_TOPIC_SHA256,
        "87fb6467cf1e1c1715146fea70957e82970fcf4019ce15d44a750eae443d33ca"
    );
}

#[test]
fn wire_status_admission_preserves_call_membership_pending_and_alias_rejection() {
    for (call, completion, reason, symbol) in [
        (MqMqiCall::Put, 0, 0, "MQRC_NONE"),
        (MqMqiCall::Get, 1, 2079, "MQRC_TRUNCATED_MSG_ACCEPTED"),
        (MqMqiCall::Get, 2, 2033, "MQRC_NO_MSG_AVAILABLE"),
        (MqMqiCall::Commit, 1, 2003, "MQRC_BACKED_OUT"),
    ] {
        let status = MqReviewedStatus::from_wire_pair(call, completion, reason).unwrap();
        assert_eq!(status.reason_symbol(), symbol);
        assert_eq!(status.wire_pair(), (completion as i32, reason as i32));
    }
    for (call, completion, reason, error) in [
        (
            MqMqiCall::CallbackFunction,
            0,
            0,
            MqStatusProblem::NoCallReturn,
        ),
        (
            MqMqiCall::CallbackFunction,
            -1,
            i64::MAX,
            MqStatusProblem::NoCallReturn,
        ),
        (
            MqMqiCall::Get,
            -1,
            2033,
            MqStatusProblem::UnknownCompletionNumber,
        ),
        (MqMqiCall::Get, 0, 2033, MqStatusProblem::UnknownPair),
        (MqMqiCall::Begin, 2, 2033, MqStatusProblem::UnknownPair),
        (MqMqiCall::Get, 2, -1, MqStatusProblem::UnknownPair),
        (
            MqMqiCall::Get,
            2,
            i64::MAX,
            MqStatusProblem::NumericOutOfRange,
        ),
        (
            MqMqiCall::Put,
            2,
            2192,
            MqStatusProblem::AmbiguousReasonIdentity,
        ),
        (
            MqMqiCall::CreateMessageHandle,
            2,
            2273,
            MqStatusProblem::PendingSource(MqStatusReview::PendingNumericConflict),
        ),
        (
            MqMqiCall::CreateMessageHandle,
            2,
            2009,
            MqStatusProblem::PendingSource(MqStatusReview::PendingNumericConflict),
        ),
    ] {
        assert_eq!(
            MqReviewedStatus::from_wire_pair(call, completion, reason),
            Err(error)
        );
    }
    for symbol in ["MQRC_PAGESET_FULL", "MQRC_STORAGE_MEDIUM_FULL"] {
        let status =
            MqReviewedStatus::from_identity(MqMqiCall::Put, "MQCC_FAILED", symbol, 2192, 0x890)
                .unwrap();
        assert_eq!(status.wire_pair(), (2, 2192));
    }
}

#[test]
fn every_call_membership_and_source_identity_admits_only_reviewed_pairs() {
    let expected = [
        17, 15, 16, 69, 0, 34, 20, 42, 61, 13, 67, 23, 12, 15, 96, 37, 28, 18, 76, 127, 137, 42,
        19, 14, 24, 8,
    ];
    let mut admitted = 0;
    let mut pending = 0;
    let mut positions = 0;
    let mut symbols = std::collections::BTreeMap::new();
    let mut encoded = BTreeSet::new();
    assert_eq!(mq_status_calls().len(), 26);
    for (descriptor, count) in mq_status_calls().iter().zip(expected) {
        assert_eq!(mq_status_call(descriptor.call), descriptor);
        assert_eq!(descriptor.pairs().count(), count);
        let source = descriptor.call.source();
        positions += source.source_positions.len();
        assert_eq!(source.topic_sha256.len(), 64);
        assert_eq!(descriptor.reviewed_projection_sha256.len(), 64);
        let mut pairs = BTreeSet::new();
        for pair in descriptor.pairs() {
            assert!(pairs.insert((pair.completion.symbol(), pair.reason_symbol)));
            let value = MqReviewedStatus::from_symbols(
                descriptor.call,
                pair.completion.symbol(),
                pair.reason_symbol,
            );
            if pair.review != MqStatusReview::Admitted {
                pending += 1;
                assert_eq!(value, Err(MqStatusProblem::PendingSource(pair.review)));
                continue;
            }
            admitted += 1;
            let value = value.unwrap();
            let number = pair.declared_decimal.unwrap();
            let hexa = u32::from_str_radix(pair.declared_hex.unwrap(), 16).unwrap();
            assert_eq!(number as u32, hexa);
            assert_eq!(symbols.entry(pair.reason_symbol).or_insert(number), &number);
            assert_eq!(value.call(), descriptor.call);
            assert_eq!(value.identity(), pair);
            assert_eq!(value.completion(), pair.completion);
            assert_eq!(value.reason_decimal(), number);
            assert_eq!(value.reason_hex(), pair.declared_hex.unwrap());
            assert_eq!(
                MqReviewedStatus::from_identity(
                    descriptor.call,
                    pair.completion.symbol(),
                    pair.reason_symbol,
                    number,
                    hexa
                ),
                Ok(value)
            );
            assert_eq!(
                MqReviewedStatus::from_identity(
                    descriptor.call,
                    pair.completion.symbol(),
                    pair.reason_symbol,
                    number + 1,
                    hexa
                ),
                Err(MqStatusProblem::NumericMismatch)
            );
            assert_eq!(
                MqReviewedStatus::from_identity(
                    descriptor.call,
                    pair.completion.symbol(),
                    pair.reason_symbol,
                    number,
                    hexa + 1
                ),
                Err(MqStatusProblem::NumericMismatch)
            );
            let result = MqMqiResult {
                call: descriptor.call,
                outcome: MqMqiOutcome::ReviewedStatus { status: value },
            };
            let bytes = mq_mqi_result_bytes(&result, MqMqiLimits::default()).unwrap();
            assert!(encoded.insert(bytes.clone()));
            assert_eq!(
                mq_mqi_result_size(&result, MqMqiLimits::default()).unwrap(),
                bytes.len()
            );
            assert_eq!(
                mq_mqi_result_digest(&result, MqMqiLimits::default())
                    .unwrap()
                    .as_slice(),
                Sha256::digest(&bytes).as_slice()
            );
        }
    }
    assert_eq!((admitted, pending, positions), (1020, 10, 27));
    assert_eq!(MQ_STATUS_PAIR_COUNT, admitted + pending);
    assert_eq!(MQ_STATUS_PENDING_COUNT, pending);
}

#[test]
fn callback_unknown_foreign_and_wrong_completion_identities_fail_closed() {
    assert!(!mq_status_call(MqMqiCall::CallbackFunction).has_call_return);
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::CallbackFunction, "MQCC_OK", "MQRC_NONE"),
        Err(MqStatusProblem::NoCallReturn)
    );
    assert_eq!(
        MqReviewedStatus::from_reason_number(MqMqiCall::CallbackFunction, MqCompletion::Ok, 0),
        Err(MqStatusProblem::NoCallReturn)
    );
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::Begin, "MQCC_UNKNOWN", "MQRC_NONE"),
        Err(MqStatusProblem::UnknownCompletionSymbol)
    );
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::Begin, "0", "MQRC_NONE"),
        Err(MqStatusProblem::UnknownCompletionSymbol)
    );
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::Begin, "MQCC_WARNING", "MQRC_NONE"),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::Begin, "MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE"),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_FAILED", "MQRC_INVENTED"),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_reason_number(MqMqiCall::Get, MqCompletion::Failed, -1),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_reason_number(MqMqiCall::Get, MqCompletion::Failed, i32::MAX),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_reason_number(MqMqiCall::Get, MqCompletion::Failed, 0),
        Err(MqStatusProblem::UnknownPair)
    );
    // These numbers are coherent aliases in this page, so number-only lookup
    // must not choose a spelling. Either explicit symbolic identity is admitted.
    assert_eq!(
        MqReviewedStatus::from_reason_number(MqMqiCall::Put, MqCompletion::Failed, 2192),
        Err(MqStatusProblem::AmbiguousReasonIdentity)
    );
    for reason in ["MQRC_PAGESET_FULL", "MQRC_STORAGE_MEDIUM_FULL"] {
        assert!(
            MqReviewedStatus::from_identity(MqMqiCall::Put, "MQCC_FAILED", reason, 2192, 0x890)
                .is_ok()
        );
    }
    assert_eq!(
        MqReviewedStatus::from_reason_number(
            MqMqiCall::CreateMessageHandle,
            MqCompletion::Failed,
            2273
        ),
        Err(MqStatusProblem::PendingSource(
            MqStatusReview::PendingNumericConflict
        ))
    );
    assert_eq!(
        MqReviewedStatus::from_reason_number(
            MqMqiCall::CreateMessageHandle,
            MqCompletion::Failed,
            2009
        ),
        Err(MqStatusProblem::PendingSource(
            MqStatusReview::PendingNumericConflict
        ))
    );
    // The option/usage text mentions a warning absent from MQCTL's return table.
    assert_eq!(
        MqReviewedStatus::from_symbols(
            MqMqiCall::Control,
            "MQCC_WARNING",
            "MQRC_CONNECTION_SUSPENDED"
        ),
        Err(MqStatusProblem::UnknownPair)
    );
}

#[test]
fn observed_status_canonical_vectors_and_limits_preserve_outcome_distinctions() {
    // Independently framed vectors from the normative catalog and the existing
    // canonical byte contract. The old variants below exclude the new catalog.
    let vectors = [
        (
            MqMqiCall::Get,
            "MQCC_FAILED",
            "MQRC_NO_MSG_AVAILABLE",
            1785,
            "210c8ca4a54a55e1fec10816daf3b96330190ac5d4e6dcee3764c63b59a2bbbe",
        ),
        (
            MqMqiCall::Get,
            "MQCC_WARNING",
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            1792,
            "124a93ee0bc88a93459306543ca09b28f0c842bbe2b068d5e925dfd5ab24840a",
        ),
        (
            MqMqiCall::Put,
            "MQCC_OK",
            "MQRC_NONE",
            1769,
            "768bc2a0480ceb2f98f6642328d63a7ff51a227edfa59f504cb1761cff9fa53a",
        ),
    ];
    for (call, completion, reason, size, hash) in vectors {
        let status = MqReviewedStatus::from_symbols(call, completion, reason).unwrap();
        let result = MqMqiResult {
            call,
            outcome: MqMqiOutcome::ReviewedStatus { status },
        };
        let limits = MqMqiLimits::default();
        let bytes = mq_mqi_result_bytes(&result, limits).unwrap();
        assert_eq!(bytes.len(), size);
        assert_eq!(hex(&Sha256::digest(&bytes)), hash);
        assert!(
            bytes
                .windows(MQ_STATUS_CATALOG_SHA256.len())
                .any(|part| part == MQ_STATUS_CATALOG_SHA256.as_bytes())
        );
        let too_small = MqMqiLimits {
            canonical_bytes: size - 1,
            ..limits
        };
        assert_eq!(
            mq_mqi_result_bytes(&result, too_small),
            Err(MqMqiProblem::CanonicalLimit)
        );
        let wrong = MqMqiResult {
            call: MqMqiCall::Back,
            ..result.clone()
        };
        assert_eq!(
            mq_mqi_result_bytes(&wrong, limits),
            Err(MqMqiProblem::StatusCallMismatch)
        );
        for outcome in [
            MqMqiOutcome::UnknownOutcome,
            MqMqiOutcome::DuplicatePossible,
            MqMqiOutcome::Pending(MqMqiPending::StatusMapping),
        ] {
            let other = MqMqiResult { call, outcome };
            assert_ne!(mq_mqi_result_bytes(&other, limits).unwrap(), bytes);
        }
    }
    for (call, status, size, hash) in [
        (
            MqMqiCall::Close,
            MqMqiStatus::OkNone,
            1263,
            "4ad15c7b0ff2ec922ef8700eb4a2257b67ce706a6dc3f3833ad821cb0f8feb59",
        ),
        (
            MqMqiCall::Back,
            MqMqiStatus::FailedEnvironment,
            1273,
            "cb92bbb20a77a610103f2f2d96944c23c9eefc50992cb190026a2df83755784d",
        ),
    ] {
        let result = MqMqiResult {
            call,
            outcome: MqMqiOutcome::Completed {
                status,
                output: MqMqiOutput::NoOutput,
            },
        };
        let bytes = mq_mqi_result_bytes(&result, MqMqiLimits::default()).unwrap();
        assert_eq!(bytes.len(), size);
        assert_eq!(hex(&Sha256::digest(&bytes)), hash);
    }
    assert_eq!(MQ_MQI_RESULT_DOMAIN, b"mainframe-env.mq-mqi-result@1\0");
    assert_eq!(MQ_MQI_REQUEST_DOMAIN, b"mainframe-env.mq-mqi-request@1\0");
}
