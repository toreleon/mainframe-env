use super::*;

#[test]
fn payload_mutations_for_every_call_change_the_digest() {
    let f = Fixture::new(7);
    for base in f.requests() {
        let mut altered = base.clone();
        match &mut altered {
            MqMqiRequest::Back { unit, .. }
            | MqMqiRequest::Begin { unit, .. }
            | MqMqiRequest::Commit { unit, .. } => *unit += 1,
            MqMqiRequest::BufferToHandle(v) => v.strip_properties = !v.strip_properties,
            MqMqiRequest::HandleToBuffer(v) => v.query = MqPropertyQuery::Exact("key".into()),
            MqMqiRequest::Callback { operation, .. } => {
                *operation = MqMqiCallbackOperation::Suspend
            }
            MqMqiRequest::CallbackFunction { callback_id, .. } => *callback_id += 1,
            MqMqiRequest::Close(v) => {
                *v = MqObjectCloseRequest::new(
                    f.connection,
                    MqRouteCloseTarget::Object {
                        handle: f.object,
                        lifecycle: MqRouteCloseLifecycle::Unknown,
                    },
                    MqRouteCloseMode::Delete,
                )
                .unwrap()
            }
            MqMqiRequest::Connect(v) | MqMqiRequest::ConnectExtended(v) => {
                v.manager = Some(name("QMGR2"))
            }
            MqMqiRequest::CreateMessageHandle { options, .. }
            | MqMqiRequest::DeleteMessageHandle { options, .. }
            | MqMqiRequest::DeleteProperty { options, .. }
            | MqMqiRequest::SetProperty { options, .. } => {
                *options = MqMqiOptions::PendingStructure {
                    requested_version: Some(4),
                }
            }
            MqMqiRequest::Control { operation, .. } => *operation = MqMqiControl::StartWaitPending,
            MqMqiRequest::Disconnect { connection } => {
                let g = Fixture::new(7);
                *connection = g.connection;
            }
            MqMqiRequest::Get(v) => v.get.buffer_capacity -= 1,
            MqMqiRequest::Inquire(v) => v.selectors[0] = MqMqiSelector::PendingInteger(2),
            MqMqiRequest::InquireProperty(v) => v.after = Some("key".into()),
            MqMqiRequest::Open(v) => {
                *v = MqObjectOpenRequest::new(
                    f.connection,
                    MqRouteLookup::Queue {
                        name: name("QUEUE2"),
                        manager: None,
                        dynamic_pattern: None,
                    },
                    &[MqRouteOpenAccess::Output],
                    MqRouteOpenModifiers::default(),
                )
                .unwrap()
            }
            MqMqiRequest::Put { put, .. } => put.message.body.push(1),
            MqMqiRequest::PutOne { alternate_user, .. } => {
                *alternate_user = Some(MqRouteAlternateUser::new("USER").unwrap())
            }
            MqMqiRequest::Set(v) => v.integers[0] += 1,
            MqMqiRequest::Stat { kind, .. } => *kind = MqMqiStatType::ReconnectionError,
            MqMqiRequest::Subscribe(v) => v.mode = MqMqiSubscriptionMode::Resume,
            MqMqiRequest::SubscriptionRequest { unit, .. } => {
                *unit = MqMqiUnitOfWork::ExternalPending { unit: 1 }
            }
        }
        assert_ne!(
            mq_mqi_request_digest(&envelope(base)).unwrap(),
            mq_mqi_request_digest(&envelope(altered)).unwrap()
        );
    }
}

#[test]
fn descriptor_property_get_context_and_handle_inputs_are_not_omitted() {
    let f = Fixture::new(7);
    let mut seen = BTreeSet::new();
    let mut variants = vec![put()];
    let mut v = put();
    v.message.descriptor.identifiers.message_id = Some(vec![0, 1]);
    variants.push(v);
    let mut v = put();
    v.message.descriptor.identifiers.correlation_id = Some(vec![0, 1]);
    variants.push(v);
    let mut v = put();
    v.message.descriptor.identifiers.group_id = Some(vec![0, 1]);
    v.message.descriptor.ordering.group_sequence = Some(1);
    variants.push(v.clone());
    v.message.descriptor.ordering.group_sequence = Some(2);
    variants.push(v.clone());
    v.message.descriptor.ordering.last_in_group = true;
    variants.push(v);
    let mut v = put();
    v.message.descriptor.ordering.segment_offset = Some(0);
    variants.push(v.clone());
    v.message.descriptor.ordering.segment_offset = Some(1);
    variants.push(v.clone());
    v.message.descriptor.ordering.last_segment = true;
    variants.push(v.clone());
    v.message.descriptor.ordering.segmentation_allowed = true;
    variants.push(v);
    let mut v = put();
    v.message.descriptor.format = Some("BYTES2".into());
    variants.push(v);
    let mut v = put();
    v.message.descriptor.format = None;
    variants.push(v);
    let mut v = put();
    v.message.descriptor.expiry = MqExpiry::RelativeHostTicks(1);
    variants.push(v);
    let mut v = put();
    v.message.descriptor.persistence = MqPersistence::NonPersistent;
    variants.push(v);
    let mut v = put();
    v.message.descriptor.priority = MqPriority::PendingNumeric(1);
    variants.push(v);
    let mut v = put();
    v.message.properties.push(property());
    variants.push(v.clone());
    v.message.properties[0].name = "key2".into();
    variants.push(v.clone());
    v.message.properties[0].value = vec![255, 0];
    variants.push(v.clone());
    v.message.properties[0].kind = MqPropertyType::String;
    variants.push(v);
    let mut v = put();
    v.message_handle = Some(f.handle);
    variants.push(v);
    let mut v = put();
    v.context = MqMqiMessageContext::PassIdentityPending { source: f.object };
    variants.push(v);
    let mut v = put();
    v.context = MqMqiMessageContext::PassAllPending { source: f.object };
    variants.push(v);
    let mut v = put();
    v.context = MqMqiMessageContext::SetIdentityPending {
        user: MqRouteAlternateUser::new("USER").unwrap(),
    };
    variants.push(v);
    let mut v = put();
    v.context = MqMqiMessageContext::SetAllPending {
        user: MqRouteAlternateUser::new("USER").unwrap(),
    };
    variants.push(v);
    let mut v = put();
    v.unit = MqMqiUnitOfWork::Local { unit: 1 };
    variants.push(v);
    let mut v = put();
    v.options = MqMqiOptions::PendingStructure {
        requested_version: None,
    };
    variants.push(v.clone());
    v.options = MqMqiOptions::PendingStructure {
        requested_version: Some(1),
    };
    variants.push(v.clone());
    v.options = MqMqiOptions::PendingStructure {
        requested_version: Some(2),
    };
    variants.push(v);
    for put in variants {
        let request = envelope(MqMqiRequest::Put {
            connection: f.connection,
            object: f.object,
            put,
        });
        assert!(seen.insert(mq_mqi_request_digest(&request).unwrap()));
        check_canonical(
            &mq_mqi_request_bytes(&request).unwrap(),
            MQ_MQI_REQUEST_DOMAIN,
        );
    }
    let get_value = |get| {
        envelope(MqMqiRequest::Get(MqMqiGet {
            connection: f.connection,
            object: f.object,
            get,
            message_handle: None,
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
        }))
    };
    let mut variants = vec![get()];
    let mut v = get();
    v.selection.identifiers.message_id = Some(vec![0, 255]);
    variants.push(v);
    let mut v = get();
    v.selection.identifiers.correlation_id = Some(vec![0, 255]);
    variants.push(v);
    let mut v = get();
    v.mode = MqGetMode::BrowseFirst;
    variants.push(v);
    let mut v = get();
    v.mode = MqGetMode::BrowseNext { cursor: 1 };
    variants.push(v.clone());
    v.mode = MqGetMode::BrowseNext { cursor: 2 };
    variants.push(v);
    let mut v = get();
    v.mode = MqGetMode::RemoveUnderCursor { cursor: 1 };
    variants.push(v);
    let mut v = get();
    v.wait = MqWait::BoundedHostTicks(1);
    variants.push(v.clone());
    v.wait = MqWait::BoundedHostTicks(2);
    variants.push(v);
    let mut v = get();
    v.truncation = MqTruncation::Accept;
    variants.push(v);
    for v in variants {
        assert!(seen.insert(mq_mqi_request_digest(&get_value(v)).unwrap()));
    }
}

#[test]
fn request_and_result_domains_and_golden_default_connection_are_frozen() {
    let request = envelope(MqMqiRequest::Connect(MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    }));
    let digest = mq_mqi_request_digest(&request).unwrap();
    // Filled from the reviewed explicit schema once, independent of runtime
    // handle IDs; no IBM execution expectation is derived from these bytes.
    assert_eq!(
        digest,
        [
            0x7a, 0x6b, 0x7d, 0x89, 0x27, 0x84, 0x2c, 0xf0, 0x7b, 0x17, 0x98, 0xb4, 0x6f, 0x86,
            0xe9, 0x37, 0x8a, 0x11, 0x1e, 0x55, 0x93, 0xef, 0x91, 0xdd, 0xdb, 0xd3, 0x74, 0x65,
            0xaf, 0x4f, 0x73, 0x4a
        ]
    );
    let result = MqMqiResult {
        call: MqMqiCall::Connect,
        outcome: MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
    };
    assert_ne!(
        digest,
        mq_mqi_result_digest(&result, MqMqiLimits::default()).unwrap()
    );
}
