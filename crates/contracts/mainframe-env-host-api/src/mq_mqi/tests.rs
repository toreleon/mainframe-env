use super::*;
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

mod bounds;
mod identities;
mod mutations;
mod results;
mod reviewed_output;

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
fn envelope(request: MqMqiRequest) -> MqMqiRequestEnvelope {
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: owner(),
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: MqMqiLimits::default(),
        request,
    }
}
fn name(text: &str) -> MqRouteName {
    MqRouteName::new(text).unwrap()
}
fn lookup() -> MqRouteLookup {
    MqRouteLookup::Queue {
        name: name("QUEUE"),
        manager: None,
        dynamic_pattern: None,
    }
}
fn message() -> MqMessage {
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: Default::default(),
            format: Some("BYTES".into()),
            expiry: MqExpiry::Unlimited,
            persistence: MqPersistence::Persistent,
            priority: MqPriority::QueueDefault,
            ordering: Default::default(),
        },
        body: vec![0, 255, 1],
        properties: vec![],
    }
}
fn get() -> MqGetContract {
    MqGetContract {
        selection: Default::default(),
        mode: MqGetMode::Remove,
        wait: MqWait::NoWait,
        truncation: MqTruncation::Reject,
        buffer_capacity: 128,
    }
}
fn put() -> MqMqiPut {
    MqMqiPut {
        message: message(),
        message_handle: None,
        context: MqMqiMessageContext::Default,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }
}
fn property() -> MqMessageProperty {
    MqMessageProperty {
        name: "key".into(),
        kind: MqPropertyType::ByteString,
        value: vec![0, 255],
    }
}

struct Fixture {
    registry: MqHandleRegistry,
    connection: MqHconn,
    object: MqHobj,
    subscription: MqHsub,
    handle: MqHmsg,
}
impl Fixture {
    fn new(epoch: u64) -> Self {
        let mut registry = MqHandleRegistry::new(epoch, 16).unwrap();
        let connection = registry
            .connect(owner(), MqHandleSharing::NonShared)
            .unwrap();
        let object = registry.create_object(owner(), connection).unwrap();
        let subscription = registry.create_subscription(owner(), connection).unwrap();
        let handle = registry.create_message(owner(), connection).unwrap();
        Self {
            registry,
            connection,
            object,
            subscription,
            handle,
        }
    }
    fn buffer(&self) -> MqMqiBuffer {
        MqMqiBuffer {
            connection: self.connection,
            handle: self.handle,
            descriptor: message().descriptor,
            query: MqPropertyQuery::Prefix(String::new()),
            buffer: vec![1, 2],
            capacity: 128,
            strip_properties: true,
            format: MqMqiBufferFormat::KernelV1,
            options: MqMqiOptions::ContractDefault,
        }
    }
    fn requests(&self) -> Vec<MqMqiRequest> {
        let c = self.connection;
        let options = MqMqiOptions::ContractDefault;
        let connect = MqMqiConnect {
            manager: Some(name("QMGR")),
            sharing: MqHandleSharing::NonShared,
            options,
        };
        vec![
            MqMqiRequest::Back {
                connection: c,
                unit: 1,
            },
            MqMqiRequest::Begin {
                connection: c,
                unit: 1,
                options,
            },
            MqMqiRequest::BufferToHandle(self.buffer()),
            MqMqiRequest::Callback {
                connection: c,
                object: self.object,
                operation: MqMqiCallbackOperation::Register {
                    callback_id: 3,
                    get: get(),
                    suspended: false,
                },
                options,
            },
            MqMqiRequest::CallbackFunction {
                connection: c,
                callback_id: 3,
                message: Some(message()),
                get: Some(get()),
                context: MqMqiOptions::PendingStructure {
                    requested_version: None,
                },
            },
            MqMqiRequest::Close(
                MqObjectCloseRequest::new(
                    c,
                    MqRouteCloseTarget::Object {
                        handle: self.object,
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    MqRouteCloseMode::None,
                )
                .unwrap(),
            ),
            MqMqiRequest::Commit {
                connection: c,
                unit: 1,
            },
            MqMqiRequest::Connect(connect.clone()),
            MqMqiRequest::ConnectExtended(connect),
            MqMqiRequest::CreateMessageHandle {
                connection: c,
                options,
            },
            MqMqiRequest::Control {
                connection: c,
                operation: MqMqiControl::Start,
                options,
            },
            MqMqiRequest::Disconnect { connection: c },
            MqMqiRequest::DeleteMessageHandle {
                connection: c,
                handle: self.handle,
                options,
            },
            MqMqiRequest::DeleteProperty {
                connection: c,
                handle: self.handle,
                query: MqPropertyQuery::Exact("key".into()),
                options,
            },
            MqMqiRequest::Get(MqMqiGet {
                connection: c,
                object: self.object,
                get: get(),
                message_handle: Some(self.handle),
                options,
                unit: MqMqiUnitOfWork::NoSyncpoint,
            }),
            MqMqiRequest::Inquire(MqMqiInquiry {
                connection: c,
                object: self.object,
                selectors: vec![
                    MqMqiSelector::PendingInteger(1),
                    MqMqiSelector::PendingCharacter(2),
                ],
                integer_capacity: 1,
                character_capacity: 48,
            }),
            MqMqiRequest::InquireProperty(MqMqiPropertyInquiry {
                connection: c,
                handle: self.handle,
                query: MqPropertyQuery::Prefix(String::new()),
                after: None,
                requested_type: None,
                value_capacity: 128,
                name_capacity: 128,
                options,
            }),
            MqMqiRequest::HandleToBuffer(self.buffer()),
            MqMqiRequest::Open(
                MqObjectOpenRequest::new(
                    c,
                    lookup(),
                    &[MqRouteOpenAccess::Output],
                    MqRouteOpenModifiers::default(),
                )
                .unwrap(),
            ),
            MqMqiRequest::Put {
                connection: c,
                object: self.object,
                put: put(),
            },
            MqMqiRequest::PutOne {
                connection: c,
                lookup: lookup(),
                alternate_user: None,
                put: put(),
            },
            MqMqiRequest::Set(MqMqiSet {
                connection: c,
                object: self.object,
                selectors: vec![MqMqiSelector::PendingInteger(1)],
                integers: vec![7],
                characters: vec![],
            }),
            MqMqiRequest::SetProperty {
                connection: c,
                handle: self.handle,
                property: property(),
                options,
            },
            MqMqiRequest::Stat {
                connection: c,
                kind: MqMqiStatType::AsyncError,
                options,
            },
            MqMqiRequest::Subscribe(MqMqiSubscribe {
                connection: c,
                name: name("SUB"),
                mode: MqMqiSubscriptionMode::Create {
                    publications_on_request: true,
                },
                destination: MqMqiSubscriptionDestination::Catalog,
                options,
            }),
            MqMqiRequest::SubscriptionRequest {
                connection: c,
                subscription: self.subscription,
                options,
                unit: MqMqiUnitOfWork::Local { unit: 1 },
            },
        ]
    }
}

#[test]
fn all_26_payloads_preserve_27_source_positions_and_use_streaming_identity() {
    let f = Fixture::new(7);
    let requests = f.requests();
    assert_eq!(requests.len(), 26);
    let mut digests = BTreeSet::new();
    for (request, call) in requests.into_iter().zip(MqMqiCall::ALL) {
        assert_eq!(request.call(), call);
        let value = envelope(request);
        assert_eq!(value.review(), Ok(MqMqiPending::PublicDispatch));
        let bytes = mq_mqi_request_bytes(&value).unwrap();
        assert_eq!(mq_mqi_request_size(&value), Ok(bytes.len()));
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        assert_eq!(mq_mqi_request_digest(&value), Ok(hash));
        assert!(digests.insert(hash));
        assert_eq!(bytes, mq_mqi_request_bytes(&value).unwrap());
        check_canonical(&bytes, MQ_MQI_REQUEST_DOMAIN);
    }
    assert_eq!(
        MqMqiCall::ALL
            .iter()
            .map(|c| c.source().source_positions.len())
            .sum::<usize>(),
        27
    );
    assert_eq!(
        MqMqiCall::HandleToBuffer.source().source_positions,
        [18, 25]
    );
    for call in MqMqiCall::ALL {
        let contract = mq_mqi_contract_by_label(call.label()).unwrap();
        assert_eq!(contract.official_row, call.source().official_row);
        assert_eq!(contract.topic_sha256, call.source().topic_sha256);
    }
}

#[test]
fn all_result_identities_and_uncertainty_are_distinct_and_replay_stably() {
    let limits = MqMqiLimits::default();
    let mut seen = BTreeSet::new();
    for call in MqMqiCall::ALL {
        for outcome in [
            MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
            MqMqiOutcome::Pending(MqMqiPending::StatusMapping),
            MqMqiOutcome::UnknownOutcome,
            MqMqiOutcome::DuplicatePossible,
        ] {
            let value = MqMqiResult { call, outcome };
            let bytes = mq_mqi_result_bytes(&value, limits).unwrap();
            assert_eq!(mq_mqi_result_size(&value, limits), Ok(bytes.len()));
            let hash: [u8; 32] = Sha256::digest(&bytes).into();
            assert_eq!(mq_mqi_result_digest(&value, limits), Ok(hash));
            assert!(seen.insert(hash));
            check_canonical(&bytes, MQ_MQI_RESULT_DOMAIN);
        }
    }
    assert_eq!(seen.len(), 104);
}

// Independent schema reader checks type tags, complete framing and sorted field
// names. It uses the published EFFECT-CANONICAL-V1 specification, not an encoder.
fn check_canonical(bytes: &[u8], domain: &[u8]) {
    fn count(data: &mut &[u8]) -> usize {
        let (value, tail) = data.split_at(8);
        *data = tail;
        usize::try_from(u64::from_le_bytes(value.try_into().unwrap())).unwrap()
    }
    fn text<'a>(data: &mut &'a [u8]) -> &'a str {
        assert_eq!(data[0], 1);
        *data = &data[1..];
        let n = count(data);
        let (value, tail) = data.split_at(n);
        *data = tail;
        std::str::from_utf8(value).unwrap()
    }
    fn value(data: &mut &[u8]) {
        let tag = data[0];
        *data = &data[1..];
        match tag {
            1 | 2 => {
                let n = count(data);
                *data = &data[n..];
            }
            3 | 4 | 0x20 => {}
            0x10 | 0x18 => *data = &data[1..],
            0x11 | 0x19 => *data = &data[2..],
            0x12 | 0x1a => *data = &data[4..],
            0x13 | 0x15 | 0x1b => *data = &data[8..],
            0x21 => value(data),
            0x30 | 0x32 => {
                for _ in 0..count(data) {
                    value(data);
                }
            }
            0x40 | 0x41 => {
                text(data);
                if tag == 0x41 {
                    text(data);
                }
                let n = count(data);
                let mut previous = "";
                for _ in 0..n {
                    let field = text(data);
                    assert!(previous < field, "unsorted: {previous} / {field}");
                    previous = field;
                    value(data);
                }
            }
            0x42 => {
                text(data);
                text(data);
            }
            _ => panic!("unexpected tag {tag:x}"),
        }
    }
    let mut data = bytes.strip_prefix(domain).unwrap();
    assert_eq!(text(&mut data), EFFECT_CANONICAL_SCHEMA);
    assert_eq!(text(&mut data), MQ_MQI_BOUNDARY_SCHEMA);
    value(&mut data);
    assert!(data.is_empty());
}
