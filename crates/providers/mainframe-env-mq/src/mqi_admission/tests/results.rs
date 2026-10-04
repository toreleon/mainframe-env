use super::*;
use mainframe_env_host_api::mq_status::MqReviewedStatus;
use mainframe_env_host_api::{
    EffectResult, HostResult, MqExpiry, MqGetContract, MqGetDisposition, MqGetMode,
    MqHandleRegistry, MqMessage, MqMessageDescriptor, MqMessageProperty, MqMqiHostResult,
    MqPersistence, MqPriority, MqPropertyQuery, MqPropertyType, MqTruncation,
    MqTruncationDisposition, MqWait, canonical_result_digest, canonical_result_size,
};

struct Fixture {
    registry: MqHandleRegistry,
    connection: MqHconn,
    object: mainframe_env_host_api::MqHobj,
    handle: mainframe_env_host_api::MqHmsg,
}
impl Fixture {
    fn new() -> Self {
        let mut registry = MqHandleRegistry::new(21, 16).unwrap();
        let connection = registry
            .connect(owner(), MqHandleSharing::NonShared)
            .unwrap();
        let object = registry.create_object(owner(), connection).unwrap();
        let handle = registry.create_message(owner(), connection).unwrap();
        Self {
            registry,
            connection,
            object,
            handle,
        }
    }
    fn get(&self, capacity: usize, mode: MqGetMode, truncation: MqTruncation) -> MqMqiRequest {
        MqMqiRequest::Get(MqMqiGet {
            connection: self.connection,
            object: self.object,
            message_handle: None,
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
            get: MqGetContract {
                selection: Default::default(),
                mode,
                wait: MqWait::NoWait,
                truncation,
                buffer_capacity: capacity,
            },
        })
    }
    fn inquire(&self, integers: usize, characters: usize) -> MqMqiRequest {
        MqMqiRequest::Inquire(MqMqiInquiry {
            connection: self.connection,
            object: self.object,
            selectors: vec![],
            integer_capacity: integers,
            character_capacity: characters,
        })
    }
    fn property(&self, name: usize, value: usize) -> MqMqiRequest {
        MqMqiRequest::InquireProperty(MqMqiPropertyInquiry {
            connection: self.connection,
            handle: self.handle,
            query: MqPropertyQuery::Prefix(String::new()),
            after: None,
            requested_type: None,
            name_capacity: name,
            value_capacity: value,
            options: MqMqiOptions::ContractDefault,
        })
    }
    fn conversion(&self, capacity: usize, to_handle: bool) -> MqMqiRequest {
        let buffer = MqMqiBuffer {
            connection: self.connection,
            handle: self.handle,
            descriptor: descriptor(),
            query: MqPropertyQuery::Prefix(String::new()),
            buffer: vec![],
            capacity,
            strip_properties: false,
            format: MqMqiBufferFormat::KernelV1,
            options: MqMqiOptions::ContractDefault,
        };
        if to_handle {
            MqMqiRequest::BufferToHandle(buffer)
        } else {
            MqMqiRequest::HandleToBuffer(buffer)
        }
    }
}

fn descriptor() -> MqMessageDescriptor {
    MqMessageDescriptor {
        identifiers: Default::default(),
        format: None,
        expiry: MqExpiry::Unlimited,
        persistence: MqPersistence::QueueDefault,
        priority: MqPriority::QueueDefault,
        ordering: Default::default(),
    }
}
fn message(copied: usize) -> MqMessage {
    MqMessage {
        descriptor: descriptor(),
        body: vec![0; copied],
        properties: vec![],
    }
}
fn typed(reply: &mut EffectResult) -> &mut MqMqiHostResult {
    match &mut reply.outcome {
        Ok(HostResult::MqMqi(value)) => value,
        _ => unreachable!(),
    }
}
fn with_limits(
    request: MqMqiRequest,
    limits: MqMqiLimits,
    change: impl FnOnce(&mut EffectResult),
) -> Result<(), HostProblem> {
    let inv = invocation();
    let p = provider();
    let mut env = envelope();
    env.limits = limits;
    env.request = request;
    let original = effect(&inv, &mutation(), &env);
    let scope = scope(&inv, owner(), &original, &p)?;
    let admission = admit_mqi(&scope, &inv, 1)?;
    let identity = match &admission {
        MqMqiAdmission::ServiceValidation(identity) | MqMqiAdmission::Pending { identity, .. } => {
            identity
        }
        MqMqiAdmission::ForbiddenContext(_) => panic!("test requires an admitted shape"),
    };
    let mut reply = EffectResult {
        sequence: original.sequence,
        outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
            result: MqMqiResult {
                call: env.request.call(),
                outcome: MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
            },
            limits: env.limits,
        })),
    };
    change(&mut reply);
    identity.preflight_result(&reply, 2).map(|_| ())
}
fn with_reply(
    request: MqMqiRequest,
    change: impl FnOnce(&mut EffectResult),
) -> Result<(), HostProblem> {
    with_limits(request, MqMqiLimits::default(), change)
}
fn check(request: MqMqiRequest, outcome: MqMqiOutcome) -> Result<(), HostProblem> {
    with_reply(request, |reply| typed(reply).result.outcome = outcome)
}
fn observation(output: MqMqiOutput) -> MqMqiOutcome {
    MqMqiOutcome::StatusPending { output }
}

#[test]
fn reviewed_get_warning_outputs_are_bound_to_actual_original_capacity_and_disposition() {
    let f = Fixture::new();
    let outcome = |reason, disposition, copied| MqMqiOutcome::ReviewedOutput {
        status: MqReviewedStatus::from_wire_pair(MqMqiCall::Get, 1, reason).unwrap(),
        output: MqMqiOutput::Got {
            disposition: MqGetDisposition::Message(disposition),
            message: Some(message(copied)),
            cursor: None,
        },
    };
    for (reason, mode, truncation, disposition) in [
        (
            2079,
            MqGetMode::Remove,
            MqTruncation::Accept,
            MqTruncationDisposition::AcceptedRemoved {
                required: 7,
                copied: 2,
            },
        ),
        (
            2079,
            MqGetMode::BrowseFirst,
            MqTruncation::Accept,
            MqTruncationDisposition::AcceptedBrowsed {
                required: 7,
                copied: 2,
            },
        ),
        (
            2080,
            MqGetMode::Remove,
            MqTruncation::Reject,
            MqTruncationDisposition::RejectedRetained {
                required: 7,
                copied: 2,
            },
        ),
    ] {
        assert_eq!(
            check(f.get(2, mode, truncation), outcome(reason, disposition, 2)),
            Ok(())
        );
        assert_eq!(
            check(f.get(1, mode, truncation), outcome(reason, disposition, 2)),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            check(f.get(3, mode, truncation), outcome(reason, disposition, 2)),
            Err(HostProblem::Malformed)
        );
    }
    assert_eq!(
        check(
            f.get(2, MqGetMode::Remove, MqTruncation::Reject),
            outcome(
                2079,
                MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 2
                },
                2
            )
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        check(
            f.get(2, MqGetMode::BrowseFirst, MqTruncation::Accept),
            outcome(
                2079,
                MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 2
                },
                2
            )
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        check(
            f.get(0, MqGetMode::Remove, MqTruncation::Accept),
            outcome(
                2079,
                MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 0
                },
                0
            )
        ),
        Ok(())
    );
    assert_eq!(f.registry.active_handles(), 3);
}

#[test]
fn reviewed_get_failed_empty_observation_does_not_become_unknown_or_success() {
    let f = Fixture::new();
    let status = MqReviewedStatus::from_wire_pair(MqMqiCall::Get, 2, 2033).unwrap();
    for disposition in [
        MqGetDisposition::NoMessage,
        MqGetDisposition::WaitExpired,
        MqGetDisposition::UnknownOutcome,
    ] {
        let outcome = MqMqiOutcome::ReviewedOutput {
            status,
            output: MqMqiOutput::Got {
                disposition,
                message: None,
                cursor: None,
            },
        };
        assert_eq!(
            check(f.get(0, MqGetMode::Remove, MqTruncation::Reject), outcome).is_ok(),
            disposition == MqGetDisposition::NoMessage
        );
    }
}

#[test]
fn result_call_limits_sequence_and_host_variant_are_bound_to_the_original() {
    let f = Fixture::new();
    let request = f.inquire(2, 2);
    assert_eq!(
        with_reply(request.clone(), |reply| typed(reply).result.call =
            MqMqiCall::Set),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        with_reply(request.clone(), |reply| reply.sequence += 1),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        with_reply(request.clone(), |reply| reply.outcome =
            Ok(HostResult::Clock("not MQI".into()))),
        Err(HostProblem::Malformed)
    );
    for case in 0..6 {
        let limits = MqMqiLimits {
            selectors: 8,
            attribute_bytes: 8,
            buffer_bytes: 8,
            canonical_bytes: 4096,
            ..MqMqiLimits::default()
        };
        assert_eq!(
            with_limits(request.clone(), limits, |reply| {
                let limits = &mut typed(reply).limits;
                match case {
                    0 => limits.selectors += 1,
                    1 => limits.attribute_bytes += 1,
                    2 => limits.buffer_bytes += 1,
                    3 => limits.canonical_bytes += 1,
                    4 => limits.message.body_bytes -= 1,
                    5 => limits.selectors -= 1,
                    _ => unreachable!(),
                }
            }),
            Err(HostProblem::Malformed)
        );
    }
    assert_eq!(f.registry.active_handles(), 3);
}

#[test]
fn get_copied_body_cannot_exceed_original_buffer_even_with_broad_host_limits() {
    let f = Fixture::new();
    let request = f.get(2, MqGetMode::Remove, MqTruncation::Reject);
    let outcome = |copied| {
        observation(MqMqiOutput::Got {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                length: copied,
            }),
            message: Some(message(copied)),
            cursor: None,
        })
    };
    assert_eq!(check(request.clone(), outcome(2)), Ok(()));
    assert_eq!(
        check(request, outcome(3)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(f.registry.active_handles(), 3);
}

#[test]
fn get_required_lengths_larger_than_capacity_and_zero_capacity_are_preserved() {
    let f = Fixture::new();
    for (mode, truncation, disposition) in [
        (
            MqGetMode::Remove,
            MqTruncation::Reject,
            MqTruncationDisposition::RejectedRetained {
                required: 7,
                copied: 2,
            },
        ),
        (
            MqGetMode::Remove,
            MqTruncation::Accept,
            MqTruncationDisposition::AcceptedRemoved {
                required: 7,
                copied: 2,
            },
        ),
        (
            MqGetMode::BrowseFirst,
            MqTruncation::Accept,
            MqTruncationDisposition::AcceptedBrowsed {
                required: 7,
                copied: 2,
            },
        ),
    ] {
        assert_eq!(
            check(
                f.get(2, mode, truncation),
                observation(MqMqiOutput::Got {
                    disposition: MqGetDisposition::Message(disposition),
                    message: Some(message(2)),
                    cursor: None,
                })
            ),
            Ok(())
        );
    }
    assert_eq!(
        check(
            f.get(0, MqGetMode::Remove, MqTruncation::Accept),
            observation(MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 0
                }),
                message: Some(message(0)),
                cursor: None,
            })
        ),
        Ok(())
    );
    // The shared disposition validator also enforces the original truncation intent.
    assert_eq!(
        check(
            f.get(2, MqGetMode::Remove, MqTruncation::Reject),
            observation(MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 2
                }),
                message: Some(message(2)),
                cursor: None,
            })
        ),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn inquiry_copied_integer_and_character_arrays_obey_original_capacities() {
    let f = Fixture::new();
    let outcome = |integers, characters| {
        observation(MqMqiOutput::Attributes {
            integers: vec![1; integers],
            characters: vec![0; characters],
        })
    };
    assert_eq!(check(f.inquire(2, 2), outcome(2, 2)), Ok(()));
    assert_eq!(
        check(f.inquire(2, 2), outcome(3, 2)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        check(f.inquire(2, 2), outcome(2, 3)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(check(f.inquire(0, 0), outcome(0, 0)), Ok(()));
    assert_eq!(
        check(f.inquire(0, 0), outcome(1, 0)),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn property_name_and_value_copied_bytes_obey_original_capacities() {
    let f = Fixture::new();
    let outcome = |name: &str, value| {
        observation(MqMqiOutput::Property(MqMessageProperty {
            name: name.into(),
            kind: MqPropertyType::ByteString,
            value: vec![0; value],
        }))
    };
    assert_eq!(check(f.property(3, 2), outcome("key", 2)), Ok(()));
    assert_eq!(
        check(f.property(2, 2), outcome("key", 2)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        check(f.property(3, 2), outcome("key", 3)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(check(f.property(3, 0), outcome("key", 0)), Ok(()));
    assert_eq!(
        check(f.property(3, 0), outcome("key", 1)),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        check(f.property(3, 2), outcome("", 0)),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn both_conversion_directions_bound_copied_buffer_without_forged_data_lengths() {
    let f = Fixture::new();
    for to_handle in [true, false] {
        let outcome = |bytes, data_length| {
            observation(MqMqiOutput::Buffer {
                descriptor: descriptor(),
                bytes: vec![0; bytes],
                data_length,
            })
        };
        assert_eq!(check(f.conversion(2, to_handle), outcome(2, 2)), Ok(()));
        assert_eq!(
            check(f.conversion(2, to_handle), outcome(3, 3)),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(check(f.conversion(0, to_handle), outcome(0, 0)), Ok(()));
        // The existing Buffer shape only represents copied bytes, not a fabricated required-length form.
        assert_eq!(
            check(f.conversion(2, to_handle), outcome(2, 7)),
            Err(HostProblem::Malformed)
        );
    }
}

#[test]
fn reviewed_size_reporting_statuses_are_call_specific_without_fake_payloads() {
    let f = Fixture::new();
    for request in [f.property(0, 0), f.conversion(0, false)] {
        let status = MqReviewedStatus::from_symbols(
            request.call(),
            "MQCC_FAILED",
            "MQRC_PROPERTY_VALUE_TOO_BIG",
        )
        .unwrap();
        assert_eq!(
            check(request, MqMqiOutcome::ReviewedStatus { status }),
            Ok(())
        );
    }
    let foreign_status =
        MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap();
    assert_eq!(
        check(
            f.inquire(2, 2),
            MqMqiOutcome::ReviewedStatus {
                status: foreign_status
            }
        ),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn payload_shape_cannot_bypass_shared_validation_by_using_pending_status() {
    let f = Fixture::new();
    assert_eq!(
        check(
            f.inquire(2, 2),
            observation(MqMqiOutput::Property(MqMessageProperty {
                name: "key".into(),
                kind: MqPropertyType::ByteString,
                value: vec![],
            }))
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        check(
            f.inquire(2, 2),
            MqMqiOutcome::CallbackReturned {
                context: MqMqiOptions::ContractDefault
            }
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        check(
            f.inquire(2, 2),
            MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            }
        ),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn result_preflight_borrows_actual_reply_and_preserves_uncertainty_identities() {
    let inv = invocation();
    let p = provider();
    let e = effect(&inv, &mutation(), &envelope());
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!()
    };
    let mut digests = BTreeSet::new();
    for outcome in [
        MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
        MqMqiOutcome::UnknownOutcome,
        MqMqiOutcome::DuplicatePossible,
    ] {
        let reply = EffectResult {
            sequence: e.sequence,
            outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                result: MqMqiResult {
                    call: identity.envelope.request.call(),
                    outcome: outcome.clone(),
                },
                limits: identity.envelope.limits,
            })),
        };
        let preflight = identity.preflight_result(&reply, 2).unwrap();
        assert!(std::ptr::eq(preflight.original, &e));
        assert!(std::ptr::eq(preflight.reply, &reply));
        assert_eq!(
            preflight.host_result_bytes,
            canonical_result_size(&reply.outcome, p.max_result_bytes).unwrap()
        );
        assert_eq!(
            preflight.host_result_digest,
            canonical_result_digest(&reply.outcome).unwrap()
        );
        let Ok(HostResult::MqMqi(original)) = &preflight.reply.outcome else {
            panic!()
        };
        assert_eq!(original.result.outcome, outcome);
        digests.insert(preflight.host_result_digest);
    }
    assert_eq!(digests.len(), 3);
}

#[test]
fn reviewed_output_preflight_retains_full_host_result_and_unknown_precedence() {
    let f = Fixture::new();
    let inv = invocation().with_cancellation_probe(CancellationProbe::new());
    let p = provider();
    let mut env = envelope();
    env.request = f.get(2, MqGetMode::Remove, MqTruncation::Reject);
    let e = effect(&inv, &mutation(), &env);
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!()
    };
    let mut reply = EffectResult {
        sequence: e.sequence,
        outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
            limits: env.limits,
            result: MqMqiResult {
                call: MqMqiCall::Get,
                outcome: MqMqiOutcome::ReviewedOutput {
                    status: MqReviewedStatus::from_wire_pair(MqMqiCall::Get, 1, 2080).unwrap(),
                    output: MqMqiOutput::Got {
                        disposition: MqGetDisposition::Message(
                            MqTruncationDisposition::RejectedRetained {
                                required: 7,
                                copied: 2,
                            },
                        ),
                        message: Some(message(2)),
                        cursor: None,
                    },
                },
            },
        })),
    };
    let before = reply.clone();
    let observed = identity.preflight_result(&reply, 2).unwrap();
    assert!(std::ptr::eq(observed.reply, &reply));
    assert_eq!(
        observed.host_result_digest,
        canonical_result_digest(&reply.outcome).unwrap()
    );
    assert_eq!(
        observed.host_result_bytes,
        canonical_result_size(&reply.outcome, p.max_result_bytes).unwrap()
    );
    assert_eq!(reply, before);
    let standalone =
        mainframe_env_host_api::mq_mqi::mq_mqi_result_digest(&typed(&mut reply).result, env.limits)
            .unwrap();
    assert_ne!(standalone, canonical_result_digest(&reply.outcome).unwrap());
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(
        identity.preflight_result(&reply, 2).unwrap_err(),
        HostProblem::Cancelled
    );
    reply.sequence += 1;
    reply.outcome = Err(HostProblem::UnknownOutcome);
    assert_eq!(
        identity.preflight_result(&reply, 2).unwrap_err(),
        HostProblem::UnknownOutcome
    );
    assert_eq!(f.registry.active_handles(), 3);
}

#[test]
fn complete_result_budget_includes_host_framing_and_live_controls_remain_live() {
    let inv = invocation().with_cancellation_probe(CancellationProbe::new());
    let e = effect(&inv, &mutation(), &envelope());
    let reply = EffectResult {
        sequence: e.sequence,
        outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
            result: MqMqiResult {
                call: MqMqiCall::Connect,
                outcome: MqMqiOutcome::UnknownOutcome,
            },
            limits: MqMqiLimits::default(),
        })),
    };
    let size = canonical_result_size(&reply.outcome, usize::MAX).unwrap();
    for budget in [size - 1, size] {
        let mut p = provider();
        p.max_result_bytes = budget;
        let scope = scope(&inv, owner(), &e, &p).unwrap();
        let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap()
        else {
            panic!()
        };
        let preflight = identity.preflight_result(&reply, 2);
        if budget < size {
            assert_eq!(preflight.unwrap_err(), HostProblem::ResourceExhausted);
        } else {
            assert_eq!(preflight.unwrap().host_result_bytes, size);
        }
    }
    let p = provider();
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!()
    };
    assert_eq!(
        identity.preflight_result(&reply, 0).unwrap_err(),
        HostProblem::Malformed
    );
    assert_eq!(
        identity.preflight_result(&reply, 90).unwrap_err(),
        HostProblem::TimedOut
    );
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(
        identity.preflight_result(&reply, 2).unwrap_err(),
        HostProblem::Cancelled
    );
}

#[test]
fn explicit_host_unknown_outcome_is_not_replaced_by_corrupt_metadata() {
    let inv = invocation();
    let e = effect(&inv, &mutation(), &envelope());
    let mut p = provider();
    p.max_result_bytes = 1;
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!()
    };
    let reply = EffectResult {
        sequence: 0,
        outcome: Err(HostProblem::UnknownOutcome),
    };
    assert_eq!(
        identity.preflight_result(&reply, 100).unwrap_err(),
        HostProblem::UnknownOutcome
    );
}
