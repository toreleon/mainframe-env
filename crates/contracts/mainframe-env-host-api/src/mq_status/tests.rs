use super::*;
use crate::mq_mqi::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
            "58ae77258476b8e37191f99047b02a8f4a25682641ad2f4ef7f3ea3456be5084",
        ),
        (
            MqMqiCall::Get,
            "MQCC_WARNING",
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            1792,
            "bfd8bcd280d4a08fc13ae4b216eae5894b779437fb1075a096eeee7c5e2f5fec",
        ),
        (
            MqMqiCall::Put,
            "MQCC_OK",
            "MQRC_NONE",
            1769,
            "f909a123e8afed08eab39935358725198fe0030b44acfd1e9c96dddab9fe7a7f",
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
