use super::*;

#[test]
fn exact_canonical_budget_succeeds_and_one_byte_short_fails_all_helpers() {
    let f = Fixture::new(7);
    let mut value = envelope(MqMqiRequest::Put {
        connection: f.connection,
        object: f.object,
        put: put(),
    });
    let size = mq_mqi_request_size(&value).unwrap();
    value.limits.canonical_bytes = size;
    assert_eq!(mq_mqi_request_bytes(&value).unwrap().len(), size);
    assert!(mq_mqi_request_digest(&value).is_ok());
    value.limits.canonical_bytes = size - 1;
    assert_eq!(
        mq_mqi_request_size(&value),
        Err(MqMqiProblem::CanonicalLimit)
    );
    assert_eq!(
        mq_mqi_request_bytes(&value),
        Err(MqMqiProblem::CanonicalLimit)
    );
    assert_eq!(
        mq_mqi_request_digest(&value),
        Err(MqMqiProblem::CanonicalLimit)
    );
    let result = MqMqiResult {
        call: MqMqiCall::Put,
        outcome: MqMqiOutcome::UnknownOutcome,
    };
    let mut limits = MqMqiLimits::default();
    let size = mq_mqi_result_size(&result, limits).unwrap();
    limits.canonical_bytes = size;
    assert_eq!(mq_mqi_result_bytes(&result, limits).unwrap().len(), size);
    assert!(mq_mqi_result_digest(&result, limits).is_ok());
    limits.canonical_bytes = size - 1;
    assert_eq!(
        mq_mqi_result_size(&result, limits),
        Err(MqMqiProblem::CanonicalLimit)
    );
    assert_eq!(
        mq_mqi_result_bytes(&result, limits),
        Err(MqMqiProblem::CanonicalLimit)
    );
    assert_eq!(
        mq_mqi_result_digest(&result, limits),
        Err(MqMqiProblem::CanonicalLimit)
    );
}

#[test]
fn budgets_cannot_be_widened_and_all_smaller_limits_change_identity() {
    let base = envelope(MqMqiRequest::Connect(MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    }));
    let hash = mq_mqi_request_digest(&base).unwrap();
    let mut seen = BTreeSet::from([hash]);
    macro_rules! limit {
        ($($field:ident).+) => {{
            let mut v=base.clone(); v.limits.$($field).+ -=1;
            assert!(seen.insert(mq_mqi_request_digest(&v).unwrap()));
            let mut v=base.clone(); v.limits.$($field).+ +=1;
            assert!(mq_mqi_request_digest(&v).is_err());
            let mut v=base.clone(); v.limits.$($field).+ = 0;
            assert!(mq_mqi_request_digest(&v).is_err());
        }};
    }
    limit!(canonical_bytes);
    limit!(selectors);
    limit!(attribute_bytes);
    limit!(buffer_bytes);
    limit!(message.body_bytes);
    limit!(message.identifier_bytes);
    limit!(message.format_bytes);
    limit!(message.properties);
    limit!(message.property_name_bytes);
    limit!(message.property_value_bytes);
    limit!(message.property_total_bytes);
    limit!(message.distribution_items);
    limit!(message.destination_bytes);
    limit!(message.wait_ticks);
    assert_eq!(seen.len(), 15);
}

#[test]
fn message_property_and_buffer_bounds_reject_without_changing_registry_state() {
    let f = Fixture::new(7);
    let mut requests = Vec::new();
    let mut v = put();
    v.message.body = vec![0; MqMessageLimits::default().body_bytes + 1];
    requests.push(MqMqiRequest::Put {
        connection: f.connection,
        object: f.object,
        put: v,
    });
    let mut v = property();
    v.value = vec![0; MqMessageLimits::default().property_value_bytes + 1];
    requests.push(MqMqiRequest::SetProperty {
        connection: f.connection,
        handle: f.handle,
        property: v,
        options: MqMqiOptions::ContractDefault,
    });
    let mut v = property();
    v.kind = MqPropertyType::Int64;
    requests.push(MqMqiRequest::SetProperty {
        connection: f.connection,
        handle: f.handle,
        property: v,
        options: MqMqiOptions::ContractDefault,
    });
    let mut v = f.buffer();
    v.buffer = vec![0; 129];
    v.capacity = 128;
    requests.push(MqMqiRequest::BufferToHandle(v));
    let mut v = f.buffer();
    v.capacity = MqMqiLimits::default().buffer_bytes + 1;
    requests.push(MqMqiRequest::HandleToBuffer(v));
    let mut v = get();
    v.buffer_capacity = MqMessageLimits::default().body_bytes + 1;
    requests.push(MqMqiRequest::Get(MqMqiGet {
        connection: f.connection,
        object: f.object,
        get: v,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }));
    for request in requests {
        let v = envelope(request);
        let before = v.clone();
        assert!(mq_mqi_request_bytes(&v).is_err());
        assert!(mq_mqi_request_digest(&v).is_err());
        assert_eq!(v, before);
        assert_eq!(f.registry.active_handles(), 4);
        f.registry
            .validate_message_property(owner(), f.connection, f.handle.into())
            .unwrap();
    }
}

#[test]
fn selector_order_duplicates_and_array_capacities_remain_meaningful() {
    let f = Fixture::new(7);
    let base = MqMqiInquiry {
        connection: f.connection,
        object: f.object,
        selectors: vec![
            MqMqiSelector::PendingInteger(1),
            MqMqiSelector::PendingCharacter(2),
        ],
        integer_capacity: 1,
        character_capacity: 48,
    };
    let hash = mq_mqi_request_digest(&envelope(MqMqiRequest::Inquire(base.clone()))).unwrap();
    let mut seen = BTreeSet::from([hash]);
    let mut v = base.clone();
    v.selectors.reverse();
    assert!(seen.insert(mq_mqi_request_digest(&envelope(MqMqiRequest::Inquire(v))).unwrap()));
    let mut v = base.clone();
    v.selectors.push(v.selectors[0]);
    assert!(seen.insert(mq_mqi_request_digest(&envelope(MqMqiRequest::Inquire(v))).unwrap()));
    let mut v = base.clone();
    v.integer_capacity = 0;
    assert!(seen.insert(mq_mqi_request_digest(&envelope(MqMqiRequest::Inquire(v))).unwrap()));
    let mut v = base.clone();
    v.character_capacity = 0;
    assert!(seen.insert(mq_mqi_request_digest(&envelope(MqMqiRequest::Inquire(v))).unwrap()));
    let mut v = base.clone();
    v.selectors = vec![MqMqiSelector::PendingInteger(1); 256];
    assert!(mq_mqi_request_bytes(&envelope(MqMqiRequest::Inquire(v.clone()))).is_ok());
    v.selectors.push(MqMqiSelector::PendingInteger(1));
    assert_eq!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::Inquire(v))),
        Err(MqMqiProblem::SelectorCount)
    );
    let valid = MqMqiSet {
        connection: f.connection,
        object: f.object,
        selectors: vec![MqMqiSelector::PendingInteger(1); 2],
        integers: vec![7, 8],
        characters: vec![],
    };
    assert!(mq_mqi_request_bytes(&envelope(MqMqiRequest::Set(valid.clone()))).is_ok());
    let mut v = valid;
    v.integers.pop();
    assert_eq!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::Set(v))),
        Err(MqMqiProblem::AttributeCount)
    );
}

#[test]
fn special_connection_context_callback_wait_and_unit_forms_fail_closed() {
    let options = MqMqiOptions::ContractDefault;
    assert!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::CreateMessageHandle {
            connection: MqHconn::Unassociated,
            options
        }))
        .is_ok()
    );
    assert_eq!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::Disconnect {
            connection: MqHconn::Unassociated
        })),
        Err(MqMqiProblem::Connection)
    );
    let mut v = envelope(MqMqiRequest::Commit {
        connection: MqHconn::Default,
        unit: 1,
    });
    assert_eq!(mq_mqi_request_bytes(&v), Err(MqMqiProblem::Connection));
    v.context.owner.environment = MqHostEnvironment::ZosCics;
    assert!(mq_mqi_request_bytes(&v).is_ok()); // shape is not dispatch authority
    assert_eq!(v.review(), Ok(MqMqiPending::PublicDispatch));
    v.context.owner.task_id = 0;
    assert_eq!(mq_mqi_request_bytes(&v), Err(MqMqiProblem::Context));
    let f = Fixture::new(7);
    assert_eq!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::Commit {
            connection: f.connection,
            unit: 0
        })),
        Err(MqMqiProblem::Unit)
    );
    assert_eq!(
        mq_mqi_request_bytes(&envelope(MqMqiRequest::CallbackFunction {
            connection: f.connection,
            callback_id: 0,
            message: None,
            get: None,
            context: options
        })),
        Err(MqMqiProblem::Callback)
    );
    let mut contract = get();
    contract.wait = MqWait::BoundedHostTicks(0);
    let value = envelope(MqMqiRequest::Get(MqMqiGet {
        connection: f.connection,
        object: f.object,
        get: contract,
        message_handle: None,
        options,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }));
    assert_eq!(
        mq_mqi_request_bytes(&value),
        Err(MqMqiProblem::Message(MqMessageProblem::Wait))
    );
    let q = MqMqiRequest::DeleteProperty {
        connection: f.connection,
        handle: f.handle,
        query: MqPropertyQuery::Prefix(String::new()),
        options,
    };
    assert!(mq_mqi_request_bytes(&envelope(q)).is_err());
}
