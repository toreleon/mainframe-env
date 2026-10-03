use super::*;
use crate::mq_mqi::*;
use crate::mq_object_route::*;
use crate::*;
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits, RunUnitId};

mod canonical;

fn owner() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}

fn effect() -> EffectRequest {
    let limits = InvocationLimits::default();
    let key = IdempotencyKey::new("mqi-host-golden", limits).unwrap();
    EffectRequest {
        run_unit: RunUnitId::new("run", limits).unwrap(),
        sequence: 7,
        deadline_tick: 100,
        idempotency_key: Some(key.clone()),
        request: HostRequest::MqMqi(MqMqiHostRequest {
            envelope: MqMqiRequestEnvelope {
                context: MqMqiContext {
                    owner: owner(),
                    syncpoint_owner: MqSyncpointOwner::QueueManager,
                },
                limits: MqMqiLimits::default(),
                request: MqMqiRequest::Connect(MqMqiConnect {
                    manager: Some(MqRouteName::new("QMGR").unwrap()),
                    sharing: MqHandleSharing::NonShared,
                    options: MqMqiOptions::ContractDefault,
                }),
            },
            mutation: Mutation {
                sequence: 7,
                idempotency_key: key,
                transaction: None,
            },
        }),
    }
}

fn typed(effect: &mut EffectRequest) -> &mut MqMqiHostRequest {
    match &mut effect.request {
        HostRequest::MqMqi(value) => value,
        _ => unreachable!(),
    }
}

fn reply() -> MqMqiHostResult {
    MqMqiHostResult {
        result: MqMqiResult {
            call: MqMqiCall::Disconnect,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::NoOutput,
            },
        },
        limits: MqMqiLimits::default(),
    }
}

#[test]
fn occurrence_borrows_the_original_effect_envelope_and_exact_mutation() {
    let effect = effect();
    let occurrence = effect
        .mq_mqi_occurrence(HostLimits::default())
        .unwrap()
        .unwrap();
    let HostRequest::MqMqi(original) = &effect.request else {
        unreachable!()
    };
    assert!(std::ptr::eq(occurrence.effect(), &effect));
    assert!(std::ptr::eq(occurrence.request(), original));
    assert!(std::ptr::eq(occurrence.envelope(), &original.envelope));
    assert!(std::ptr::eq(occurrence.mutation(), &original.mutation));
    assert_eq!(occurrence.effect().sequence, occurrence.mutation().sequence);
    assert_eq!(
        occurrence.effect().idempotency_key.as_ref(),
        Some(&occurrence.mutation().idempotency_key)
    );
    assert_eq!(
        effect
            .request
            .required_capability(InvocationLimits::default())
            .as_str(),
        "host.mq.write"
    );
    assert!(effect.request.is_mutating());
    let mut other = effect.clone();
    other.request = HostRequest::Clock(crate::ClockRequest::UtcTimestamp);
    assert!(
        other
            .mq_mqi_occurrence(HostLimits::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn missing_and_substituted_occurrence_identity_is_rejected() {
    let limits = HostLimits::default();
    let mut candidate = effect();
    candidate.idempotency_key = None;
    assert_eq!(
        candidate.mq_mqi_occurrence(limits).err(),
        Some(HostProblem::MissingIdempotency)
    );
    for inner in [false, true] {
        let mut candidate = effect();
        if inner {
            typed(&mut candidate).mutation.sequence += 1;
        } else {
            candidate.sequence += 1;
        }
        assert_eq!(
            candidate.mq_mqi_occurrence(limits).err(),
            Some(HostProblem::IdempotencyConflict)
        );
        let mut candidate = effect();
        let replacement = IdempotencyKey::new("substituted", InvocationLimits::default()).unwrap();
        if inner {
            typed(&mut candidate).mutation.idempotency_key = replacement;
        } else {
            candidate.idempotency_key = Some(replacement);
        }
        assert_eq!(
            candidate.mq_mqi_occurrence(limits).err(),
            Some(HostProblem::IdempotencyConflict)
        );
    }
    for deadline in [false, true] {
        let mut candidate = effect();
        if deadline {
            candidate.deadline_tick = 0;
        } else {
            candidate.sequence = 0;
        }
        assert_eq!(
            candidate.mq_mqi_occurrence(limits).err(),
            Some(HostProblem::Malformed)
        );
    }
}

#[test]
fn observation_and_cursor_requests_also_require_exact_occurrence_identity() {
    let mut registry = MqHandleRegistry::new(9, 8).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let handle = registry.create_message(owner(), connection).unwrap();
    let requests = [
        MqMqiRequest::Inquire(MqMqiInquiry {
            connection,
            object,
            selectors: vec![],
            integer_capacity: 0,
            character_capacity: 0,
        }),
        MqMqiRequest::InquireProperty(MqMqiPropertyInquiry {
            connection,
            handle,
            query: MqPropertyQuery::Prefix(String::new()),
            after: Some("cursor".into()),
            requested_type: None,
            value_capacity: 8,
            name_capacity: 8,
            options: MqMqiOptions::ContractDefault,
        }),
        MqMqiRequest::Get(MqMqiGet {
            connection,
            object,
            message_handle: None,
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
            get: MqGetContract {
                selection: Default::default(),
                mode: MqGetMode::BrowseNext { cursor: 12 },
                wait: MqWait::NoWait,
                truncation: MqTruncation::Reject,
                buffer_capacity: 8,
            },
        }),
    ];
    for request in requests {
        let mut candidate = effect();
        typed(&mut candidate).envelope.request = request;
        assert_eq!(candidate.validate(HostLimits::default()), Ok(()));
        candidate.idempotency_key = None;
        assert_eq!(
            candidate.mq_mqi_occurrence(HostLimits::default()).err(),
            Some(HostProblem::MissingIdempotency)
        );
        candidate.idempotency_key = effect().idempotency_key;
        candidate.sequence += 1;
        assert_eq!(
            candidate.mq_mqi_occurrence(HostLimits::default()).err(),
            Some(HostProblem::IdempotencyConflict)
        );
    }
}

#[test]
fn callback_notification_cannot_be_an_application_command_or_reply() {
    let mut candidate = effect();
    typed(&mut candidate).envelope.request = MqMqiRequest::CallbackFunction {
        connection: MqHconn::Default,
        callback_id: 1,
        message: None,
        get: None,
        context: MqMqiOptions::ContractDefault,
    };
    assert_eq!(
        candidate.validate(HostLimits::default()),
        Err(HostProblem::Malformed)
    );
    for outcome in [
        MqMqiOutcome::Pending(MqMqiPending::CallbackContext),
        MqMqiOutcome::CallbackReturned {
            context: MqMqiOptions::ContractDefault,
        },
    ] {
        let mut value = reply();
        value.result.call = MqMqiCall::CallbackFunction;
        value.result.outcome = outcome;
        assert_eq!(
            HostResult::MqMqi(value).validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }
}

#[test]
fn malformed_context_mutation_and_source_status_are_not_admitted() {
    let mut candidate = effect();
    typed(&mut candidate).envelope.context.owner.host_id = 0;
    assert_eq!(
        candidate.validate(HostLimits::default()),
        Err(HostProblem::Malformed)
    );
    let mut candidate = effect();
    typed(&mut candidate).mutation.transaction = Some(String::new());
    assert!(candidate.validate(HostLimits::default()).is_err());
    let mut value = reply();
    value.result.outcome = MqMqiOutcome::Completed {
        status: MqMqiStatus::FailedEnvironment,
        output: MqMqiOutput::NoOutput,
    };
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::Malformed)
    );
    let mut value = reply();
    value.result.call = MqMqiCall::Connect;
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn reviewed_status_uses_the_original_call_validator_and_host_framing() {
    let status = crate::mq_status::MqReviewedStatus::from_symbols(
        MqMqiCall::Disconnect,
        "MQCC_OK",
        "MQRC_NONE",
    )
    .unwrap();
    let mut value = reply();
    value.result.outcome = MqMqiOutcome::ReviewedStatus { status };
    assert_eq!(value.validate(HostLimits::default()), Ok(()));
    assert_ne!(
        canonical_result_digest(&Ok(HostResult::MqMqi(value.clone()))).unwrap(),
        canonical_result_digest(&Ok(HostResult::MqMqi(reply()))).unwrap()
    );
    value.result.call = MqMqiCall::Connect;
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn pending_unknown_and_duplicate_outcomes_remain_distinct_typed_observations() {
    let mut hashes = std::collections::BTreeSet::new();
    for outcome in [
        MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
        MqMqiOutcome::UnknownOutcome,
        MqMqiOutcome::DuplicatePossible,
    ] {
        let mut value = reply();
        value.result.outcome = outcome.clone();
        assert_eq!(value.validate(HostLimits::default()), Ok(()));
        assert_eq!(value.result.outcome, outcome);
        hashes.insert(canonical_result_digest(&Ok(HostResult::MqMqi(value))).unwrap());
    }
    assert_eq!(hashes.len(), 3);
}

#[test]
fn host_field_limits_and_mqi_product_limits_both_apply() {
    let mut candidate = effect();
    let mut limits = HostLimits::default();
    limits.max_name_bytes = 3;
    assert_eq!(
        candidate.validate(limits),
        Err(HostProblem::ResourceExhausted)
    );
    typed(&mut candidate).envelope.limits.selectors = 0;
    assert_eq!(
        candidate.validate(HostLimits::default()),
        Err(HostProblem::ResourceExhausted)
    );
    let mut candidate = effect();
    typed(&mut candidate).envelope.limits.message.body_bytes += 1;
    assert_eq!(
        candidate.validate(HostLimits::default()),
        Err(HostProblem::ResourceExhausted)
    );
    let mut value = reply();
    value.limits.buffer_bytes = 0;
    assert_eq!(
        value.validate(HostLimits::default()),
        Err(HostProblem::ResourceExhausted)
    );
    let mut value = reply();
    value.result.call = MqMqiCall::Inquire;
    value.result.outcome = MqMqiOutcome::StatusPending {
        output: MqMqiOutput::Attributes {
            integers: vec![1, 2],
            characters: vec![0, 255],
        },
    };
    let mut limits = HostLimits::default();
    limits.max_fields = 1;
    assert_eq!(value.validate(limits), Err(HostProblem::ResourceExhausted));
    limits.max_fields = 2;
    limits.max_record_bytes = 1;
    assert_eq!(value.validate(limits), Err(HostProblem::ResourceExhausted));
    limits.max_record_bytes = 2;
    assert_eq!(value.validate(limits), Ok(()));
}

#[test]
fn payloads_and_output_capacities_are_preflighted_against_host_limits() {
    let mut registry = MqHandleRegistry::new(8, 8).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let handle = registry.create_message(owner(), connection).unwrap();
    let options = MqMqiOptions::ContractDefault;
    let descriptor = MqMessageDescriptor {
        identifiers: Default::default(),
        format: None,
        expiry: MqExpiry::Unlimited,
        persistence: MqPersistence::QueueDefault,
        priority: MqPriority::QueueDefault,
        ordering: Default::default(),
    };
    let requests = [
        MqMqiRequest::Inquire(MqMqiInquiry {
            connection,
            object,
            selectors: vec![],
            integer_capacity: usize::MAX,
            character_capacity: 0,
        }),
        MqMqiRequest::Set(MqMqiSet {
            connection,
            object,
            selectors: vec![],
            integers: vec![1, 2, 3],
            characters: vec![],
        }),
        MqMqiRequest::BufferToHandle(MqMqiBuffer {
            connection,
            handle,
            descriptor: descriptor.clone(),
            query: MqPropertyQuery::Prefix(String::new()),
            buffer: vec![],
            capacity: usize::MAX,
            strip_properties: false,
            format: MqMqiBufferFormat::KernelV1,
            options,
        }),
        MqMqiRequest::SetProperty {
            connection,
            handle,
            property: MqMessageProperty {
                name: "p".into(),
                kind: MqPropertyType::ByteString,
                value: vec![1, 2, 3],
            },
            options,
        },
        MqMqiRequest::PutOne {
            connection,
            lookup: MqRouteLookup::Queue {
                name: MqRouteName::new("Q").unwrap(),
                manager: None,
                dynamic_pattern: None,
            },
            alternate_user: None,
            put: MqMqiPut {
                message: MqMessage {
                    descriptor,
                    body: vec![1, 2, 3],
                    properties: vec![],
                },
                message_handle: None,
                context: MqMqiMessageContext::Default,
                options,
                unit: MqMqiUnitOfWork::NoSyncpoint,
            },
        },
    ];
    let mut limits = HostLimits::default();
    limits.max_record_bytes = 2;
    limits.max_fields = 2;
    for request in requests {
        let mut candidate = effect();
        typed(&mut candidate).envelope.request = request;
        assert_eq!(
            candidate.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
    }
    let mut value = reply();
    value.result.call = MqMqiCall::Get;
    value.result.outcome = MqMqiOutcome::StatusPending {
        output: MqMqiOutput::Got {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                required: 3,
                copied: 1,
            }),
            message: None,
            cursor: None,
        },
    };
    assert_eq!(value.validate(limits), Err(HostProblem::ResourceExhausted));
}
