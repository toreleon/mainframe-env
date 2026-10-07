use super::*;
use crate::canonical::{Canonical, Encoder, REQUEST_DIGEST_DOMAIN, RESULT_DIGEST_DOMAIN, encode};

fn bytes<T: Canonical>(value: &T, domain: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    encode(value, domain, MAX_CANONICAL_EFFECT_BYTES, &mut |part| {
        bytes.extend_from_slice(part)
    })
    .unwrap();
    bytes
}
fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn actual_host_preimages_are_distinct_and_frozen() {
    let value = effect();
    let request_bytes = bytes(&value.request, REQUEST_DIGEST_DOMAIN);
    let outcome = Ok(HostResult::MqMqi(reply()));
    let result_bytes = bytes(&outcome, RESULT_DIGEST_DOMAIN);
    assert!(
        request_bytes
            .windows(b"MqMqiHostRequest".len())
            .any(|part| part == b"MqMqiHostRequest")
    );
    assert!(
        result_bytes
            .windows(b"MqMqiHostResult".len())
            .any(|part| part == b"MqMqiHostResult")
    );
    assert_ne!(
        canonical_request_digest(&value.request).unwrap(),
        mq_mqi_request_digest(&typed(&mut value.clone()).envelope).unwrap()
    );
    // These fixed vectors cover the actual host domain, variant framing, envelope and Mutation.
    assert_eq!(
        (
            hex(canonical_request_digest(&value.request).unwrap()),
            hex(canonical_result_digest(&outcome).unwrap())
        ),
        (
            "433a011034843ece80caaef730dd18d990c3fba577df9d1dd84663004cf68b8f".into(),
            "feb3c22c6f1150b7a39bf17cee6612d0b722f524edc582e43b8a401b72826c27".into()
        )
    );
}

#[test]
fn exact_streaming_budgets_include_host_framing_and_result_discriminant() {
    let mut value = effect();
    let size = canonical_request_size(&value.request, MAX_CANONICAL_EFFECT_BYTES).unwrap();
    assert_eq!(
        crate::canonical::mq_mqi::request_size(typed(&mut value), size),
        Ok(size)
    );
    assert_eq!(
        canonical_request_size(&value.request, size - 1),
        Err(HostProblem::ResourceExhausted)
    );
    typed(&mut value).envelope.limits.canonical_bytes = size;
    assert_eq!(value.validate(HostLimits::default()), Ok(()));
    typed(&mut value).envelope.limits.canonical_bytes -= 1;
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::ResourceExhausted)
    );
    let mut value = reply();
    let size = canonical_result_size(
        &Ok(HostResult::MqMqi(value.clone())),
        MAX_CANONICAL_EFFECT_BYTES,
    )
    .unwrap();
    assert_eq!(
        crate::canonical::mq_mqi::result_size(&value, size),
        Ok(size)
    );
    value.limits.canonical_bytes = size;
    assert_eq!(value.validate(HostLimits::default()), Ok(()));
    value.limits.canonical_bytes -= 1;
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        crate::canonical::mq_mqi::result_size(&value, usize::MAX),
        Ok(size)
    );
}

#[test]
fn all_host_request_semantics_including_mutation_and_context_change_identity() {
    let original = effect();
    let digest = canonical_request_digest(&original.request).unwrap();
    let mut variants = Vec::new();
    macro_rules! changed {
        ($body:expr) => {{
            let mut value = original.clone();
            $body(typed(&mut value));
            variants.push(value.request);
        }};
    }
    changed!(|value: &mut MqMqiHostRequest| value.mutation.sequence += 1);
    changed!(
        |value: &mut MqMqiHostRequest| value.mutation.idempotency_key =
            IdempotencyKey::new("other", InvocationLimits::default()).unwrap()
    );
    changed!(|value: &mut MqMqiHostRequest| value.mutation.transaction = Some("UOW".into()));
    changed!(|value: &mut MqMqiHostRequest| value.envelope.context.owner.task_id += 1);
    changed!(|value: &mut MqMqiHostRequest| value.envelope.context.owner.syncpoint_epoch += 1);
    changed!(
        |value: &mut MqMqiHostRequest| value.envelope.context.syncpoint_owner =
            MqSyncpointOwner::HostCoordinator
    );
    changed!(|value: &mut MqMqiHostRequest| value.envelope.limits.selectors -= 1);
    changed!(|value: &mut MqMqiHostRequest| value.envelope.request =
        MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault
        }));
    for value in variants {
        assert_ne!(canonical_request_digest(&value).unwrap(), digest);
    }
    let result = reply();
    let mut changed = result.clone();
    changed.limits.selectors -= 1;
    assert_ne!(
        canonical_result_digest(&Ok(HostResult::MqMqi(result))).unwrap(),
        canonical_result_digest(&Ok(HostResult::MqMqi(changed))).unwrap()
    );
}

#[test]
fn encoder_stops_before_oversized_sink_write_without_a_second_codec() {
    struct Original<'a>(&'a HostRequest);
    impl Canonical for Original<'_> {
        fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
            self.0.encode(out)
        }
    }
    let value = effect();
    let mut seen = 0usize;
    assert_eq!(
        encode(
            &Original(&value.request),
            REQUEST_DIGEST_DOMAIN,
            100,
            &mut |part| {
                seen += part.len();
                assert!(seen <= 100);
            }
        ),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn actual_issued_handles_keep_registry_generation_epoch_and_role_in_host_identity() {
    fn identities(registry: &mut MqHandleRegistry) -> Vec<HostRequest> {
        let connection = registry
            .connect(owner(), MqHandleSharing::NonShared)
            .unwrap();
        let object = registry.create_object(owner(), connection).unwrap();
        let subscription = registry.create_subscription(owner(), connection).unwrap();
        let handle = registry.create_message(owner(), connection).unwrap();
        let requests = [
            MqMqiRequest::Disconnect { connection },
            MqMqiRequest::Inquire(MqMqiInquiry {
                connection,
                object,
                selectors: vec![],
                integer_capacity: 0,
                character_capacity: 0,
            }),
            MqMqiRequest::SubscriptionRequest {
                connection,
                subscription,
                options: MqMqiOptions::ContractDefault,
                unit: MqMqiUnitOfWork::NoSyncpoint,
            },
            MqMqiRequest::DeleteMessageHandle {
                connection,
                handle,
                options: MqMqiOptions::ContractDefault,
            },
        ];
        requests
            .into_iter()
            .map(|request| {
                let mut value = effect();
                typed(&mut value).envelope.request = request;
                value.request
            })
            .collect()
    }
    let mut registry = MqHandleRegistry::new(9, 16).unwrap();
    let first = identities(&mut registry);
    let mut foreign = MqHandleRegistry::new(9, 16).unwrap();
    let second = identities(&mut foreign);
    registry.advance_epoch(10).unwrap();
    let next = identities(&mut registry);
    for ((old, foreign), next) in first.iter().zip(&second).zip(&next) {
        assert_ne!(
            canonical_request_digest(old),
            canonical_request_digest(foreign)
        );
        assert_ne!(
            canonical_request_digest(old),
            canonical_request_digest(next)
        );
    }
    let mut registry = MqHandleRegistry::new(11, 4).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let old = registry.create_object(owner(), connection).unwrap();
    registry
        .release(owner(), connection, old.into(), MqHandleKind::Object)
        .unwrap();
    let next = registry.create_object(owner(), connection).unwrap();
    assert_eq!(old.canonical_parts().1, next.canonical_parts().1);
    assert_eq!(old.canonical_parts().2 + 1, next.canonical_parts().2);
    let request = |object| {
        let mut value = effect();
        typed(&mut value).envelope.request = MqMqiRequest::Inquire(MqMqiInquiry {
            connection,
            object,
            selectors: vec![],
            integer_capacity: 0,
            character_capacity: 0,
        });
        value.request
    };
    assert_ne!(
        canonical_request_digest(&request(old)),
        canonical_request_digest(&request(next))
    );
}

#[test]
fn result_call_status_payload_and_limits_are_all_in_the_host_identity() {
    let original = reply();
    let digest = canonical_result_digest(&Ok(HostResult::MqMqi(original.clone()))).unwrap();
    let mut variants = Vec::new();
    let mut value = original.clone();
    value.result.call = MqMqiCall::Close;
    variants.push(value);
    let mut value = original.clone();
    value.result.outcome = MqMqiOutcome::Completed {
        status: MqMqiStatus::FailedEnvironment,
        output: MqMqiOutput::NoOutput,
    };
    variants.push(value);
    let mut value = original.clone();
    value.result.outcome = MqMqiOutcome::StatusPending {
        output: MqMqiOutput::Attributes {
            integers: vec![1],
            characters: vec![0, 255],
        },
    };
    variants.push(value);
    let mut value = original;
    value.limits.canonical_bytes -= 1;
    variants.push(value);
    for value in variants {
        assert_ne!(
            canonical_result_digest(&Ok(HostResult::MqMqi(value))).unwrap(),
            digest
        );
    }
}
