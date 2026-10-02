use super::*;
use crate::mq_mqi::*;
use crate::*;

struct Bindings {
    zos: bool,
    unit: Option<MqMqiUnitOfWork>,
    cursor: Option<u64>,
    tick: Option<u64>,
    policy: bool,
}
impl Default for Bindings {
    fn default() -> Self {
        Self {
            zos: false,
            unit: None,
            cursor: Some(17),
            tick: Some(30),
            policy: true,
        }
    }
}
impl MqWireBindings for Bindings {
    fn queue_defaults_are_represented(
        &self,
        _: MqHconn,
        _: Option<MqHobj>,
        _: Option<&MqRouteLookup>,
    ) -> bool {
        self.policy
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        if self.zos {
            MqWireQueueManagerPlatform::Zos
        } else {
            MqWireQueueManagerPlatform::Distributed
        }
    }
    fn admitted_unit(&self, _: MqHconn) -> Option<MqMqiUnitOfWork> {
        self.unit
    }
    fn existing_cursor(&self, _: MqHconn, _: MqHobj) -> Option<u64> {
        self.cursor
    }
    fn milliseconds_to_ticks(&self, ms: u32) -> Option<u64> {
        assert!(ms > 0);
        self.tick
    }
}

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
fn handles() -> (MqHconn, MqHobj) {
    let mut registry = MqHandleRegistry::new(1, 4).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    (
        connection,
        registry.create_object(owner(), connection).unwrap(),
    )
}
fn lookup() -> MqRouteLookup {
    MqRouteLookup::Queue {
        name: MqRouteName::new("QUEUE").unwrap(),
        manager: None,
        dynamic_pattern: None,
    }
}
fn get_input(options: i64) -> MqWireGet {
    let (connection, object) = handles();
    MqWireGet {
        connection,
        object,
        md_version: 2,
        gmo_version: 1,
        options,
        wait_milliseconds: 0,
        selection: Default::default(),
        buffer_capacity: 8,
    }
}
fn put_input(options: i64) -> MqWirePut {
    MqWirePut {
        md_version: 2,
        pmo_version: 1,
        options,
        message: MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers {
                    message_id: Some(vec![7; 24]),
                    correlation_id: Some(vec![9; 24]),
                    group_id: None,
                },
                format: Some("BYTES".into()),
                expiry: MqExpiry::RelativeHostTicks(90),
                persistence: MqPersistence::Persistent,
                priority: MqPriority::QueueDefault,
                ordering: Default::default(),
            },
            body: vec![0, 255, 1],
            properties: vec![],
        },
    }
}
fn decode(input: MqWireGet, bindings: &Bindings) -> Result<MqMqiGet, MqWireProblem> {
    get(input, bindings, MqMessageLimits::default())
}
fn decode_put(input: MqWirePut, bindings: &Bindings) -> Result<MqMqiPut, MqWireProblem> {
    let (c, o) = handles();
    put(c, o, input, bindings, MqMessageLimits::default())
}

#[test]
fn reviewed_named_constants_have_independent_numeric_expectations() {
    assert_eq!(
        (MQOO_INPUT_SHARED, MQOO_OUTPUT, MQCO_DELETE_PURGE),
        (2, 16, 2)
    );
    assert_eq!(
        (MQGMO_WAIT, MQGMO_BROWSE_FIRST, MQGMO_ACCEPT_TRUNCATED_MSG),
        (1, 16, 64)
    );
    assert_eq!(
        (MQPMO_SYNCPOINT, MQPMO_NO_SYNCPOINT, MQPMO_NEW_MSG_ID),
        (2, 4, 64)
    );
    assert_eq!(
        (
            MQOD_VERSION_1,
            MQGMO_VERSION_4,
            MQPMO_VERSION_3,
            MQMD_VERSION_2
        ),
        (1, 4, 3, 2)
    );
    assert_eq!(MQOO_RESOLVE_LOCAL_Q, MQOO_RESOLVE_LOCAL_TOPIC);
    assert_eq!((MQCO_NONE, MQCO_IMMEDIATE), (0, 0));
}

#[test]
fn open_produces_exact_existing_semantics() {
    let (c, _) = handles();
    let actual = open(c, lookup(), 1, 2 | 8 | 16, &Bindings::default()).unwrap();
    let expected = MqObjectOpenRequest::new(
        c,
        lookup(),
        &[
            MqRouteOpenAccess::InputShared,
            MqRouteOpenAccess::Browse,
            MqRouteOpenAccess::Output,
        ],
        Default::default(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn open_rejects_multiple_inputs_and_missing_access() {
    let (c, _) = handles();
    for flags in [3, 5, 6, 7] {
        assert_eq!(
            open(c, lookup(), 1, flags, &Bindings::default()),
            Err(MqWireProblem::IllegalCombination)
        );
    }
    assert_eq!(
        open(c, lookup(), 1, 0, &Bindings::default()),
        Err(MqWireProblem::Object(MqObjectRouteError::MissingAccess))
    );
}
#[test]
fn open_pending_modifiers_and_cpp_alias_do_not_become_defaults() {
    let (c, _) = handles();
    for bit in [
        128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288,
        1048576, 4194304,
    ] {
        assert!(matches!(
            open(c, lookup(), 1, 16 | bit, &Bindings::default()),
            Err(MqWireProblem::PendingOptions {
                family: MqWireFamily::Open,
                ..
            })
        ));
    }
}
#[test]
fn nonlocal_and_dynamic_forms_stay_pending() {
    let (c, _) = handles();
    for value in [
        MqRouteLookup::QueueManager,
        MqRouteLookup::Queue {
            name: MqRouteName::new("Q").unwrap(),
            manager: Some(MqRouteName::new("REMOTE").unwrap()),
            dynamic_pattern: None,
        },
    ] {
        assert_eq!(
            open(c, value, 1, 16, &Bindings::default()),
            Err(MqWireProblem::PendingFields)
        );
    }
}
#[test]
fn close_checks_single_mode_and_lifecycle() {
    let (c, o) = handles();
    let target = MqRouteCloseTarget::Object {
        handle: o,
        lifecycle: MqRouteCloseLifecycle::Predefined,
    };
    assert_eq!(
        close(c, target, 0, &Bindings::default()).unwrap().mode(),
        MqRouteCloseMode::None
    );
    assert_eq!(
        close(c, target, 1, &Bindings::default()),
        Err(MqWireProblem::Object(MqObjectRouteError::InvalidCloseMode))
    );
    assert_eq!(
        close(c, target, 3, &Bindings::default()),
        Err(MqWireProblem::IllegalCombination)
    );
    assert!(matches!(
        close(c, target, 32, &Bindings::default()),
        Err(MqWireProblem::PendingOptions { .. })
    ));
    let dynamic = MqRouteCloseTarget::Object {
        handle: o,
        lifecycle: MqRouteCloseLifecycle::PermanentDynamic,
    };
    assert_eq!(
        close(c, dynamic, 2, &Bindings::default()).unwrap().mode(),
        MqRouteCloseMode::DeletePurge
    );
}
#[test]
fn signed_mqlong_and_unknown_bits_fail_closed() {
    for value in [i64::MIN, i64::MAX, 2147483648] {
        assert_eq!(
            decode(get_input(value), &Bindings::default()),
            Err(MqWireProblem::MqlongRange)
        );
    }
    assert_eq!(
        decode(get_input(-1), &Bindings::default()),
        Err(MqWireProblem::NegativeOptions)
    );
    assert!(matches!(
        decode(get_input(1073741824), &Bindings::default()),
        Err(MqWireProblem::UnknownBits { .. })
    ));
}
#[test]
fn reviewed_and_unreviewed_versions_are_distinct() {
    for version in [2, 3, 4] {
        let mut i = get_input(4);
        i.gmo_version = version;
        assert_eq!(
            decode(i, &Bindings::default()),
            Err(MqWireProblem::PendingVersion {
                family: MqWireFamily::Get,
                version: version as i32
            })
        );
    }
    let mut i = get_input(4);
    i.gmo_version = 5;
    assert!(matches!(
        decode(i, &Bindings::default()),
        Err(MqWireProblem::UnreviewedVersion { .. })
    ));
    let (c, _) = handles();
    assert_eq!(
        open(c, lookup(), 2, 16, &Bindings::default()),
        Err(MqWireProblem::UnreviewedVersion {
            family: MqWireFamily::ObjectDescriptor,
            version: 2
        })
    );
    for version in [2, 3] {
        let mut i = put_input(4);
        i.pmo_version = version;
        assert!(matches!(
            decode_put(i, &Bindings::default()),
            Err(MqWireProblem::PendingVersion { .. })
        ));
    }
    for version in [0, -1, 3, 2147483648] {
        let mut i = get_input(4);
        i.md_version = version;
        assert!(decode(i, &Bindings::default()).is_err());
    }
}
#[test]
fn sync_defaults_use_queue_manager_platform_and_independent_unit() {
    let d = Bindings::default();
    assert_eq!(
        decode(get_input(0), &d).unwrap().unit,
        MqMqiUnitOfWork::NoSyncpoint
    );
    let mut z = Bindings {
        zos: true,
        ..Default::default()
    };
    assert_eq!(decode(get_input(0), &z), Err(MqWireProblem::MissingUnit));
    z.unit = Some(MqMqiUnitOfWork::Local { unit: 99 });
    assert_eq!(
        decode(get_input(0), &z).unwrap().unit,
        MqMqiUnitOfWork::Local { unit: 99 }
    );
    assert_eq!(
        decode(get_input(4), &z).unwrap().unit,
        MqMqiUnitOfWork::NoSyncpoint
    );
    assert_eq!(
        decode_put(put_input(0), &z).unwrap().unit,
        MqMqiUnitOfWork::Local { unit: 99 }
    );
    assert_eq!(decode(get_input(2), &d), Err(MqWireProblem::MissingUnit));
    z.unit = Some(MqMqiUnitOfWork::ExternalPending { unit: 12 });
    assert_eq!(
        decode(get_input(2), &z),
        Err(MqWireProblem::PendingExternalUnit)
    );
    z.unit = Some(MqMqiUnitOfWork::Local { unit: 0 });
    assert_eq!(decode(get_input(2), &z), Err(MqWireProblem::MissingUnit));
}
#[test]
fn browse_and_remove_under_cursor_are_distinct() {
    let b = Bindings {
        zos: true,
        ..Default::default()
    };
    let first = decode(get_input(16), &b).unwrap();
    assert_eq!(first.get.mode, MqGetMode::BrowseFirst);
    assert_eq!(first.unit, MqMqiUnitOfWork::NoSyncpoint);
    assert_eq!(
        decode(get_input(32), &b).unwrap().get.mode,
        MqGetMode::BrowseNext { cursor: 17 }
    );
    let mut b = b;
    b.unit = Some(MqMqiUnitOfWork::Local { unit: 44 });
    assert_eq!(
        decode(get_input(256 | 2), &b).unwrap().get.mode,
        MqGetMode::RemoveUnderCursor { cursor: 17 }
    );
    b.cursor = None;
    assert_eq!(decode(get_input(32), &b), Err(MqWireProblem::MissingCursor));
    for flags in [16 | 32, 16 | 256, 32 | 256, 16 | 2, 32 | 2, 2 | 4] {
        assert_eq!(
            decode(get_input(flags), &b),
            Err(MqWireProblem::IllegalCombination)
        );
    }
}
#[test]
fn selection_is_preserved_and_under_cursor_mismatch_stays_pending() {
    let mut i = get_input(4);
    i.selection.identifiers.correlation_id = Some(vec![9; 24]);
    let expected = i.selection.clone();
    assert_eq!(
        decode(i, &Bindings::default()).unwrap().get.selection,
        expected
    );
    let mut i = get_input(256 | 4);
    i.selection.identifiers.message_id = Some(vec![1; 24]);
    assert_eq!(
        decode(i, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
}
#[test]
fn bounded_wait_is_not_assumed_equal_to_host_ticks() {
    let mut i = get_input(1 | 4);
    i.wait_milliseconds = 7;
    assert_eq!(
        decode(i, &Bindings::default()).unwrap().get.wait,
        MqWait::BoundedHostTicks(30)
    );
    for ms in [-1, 2147483648] {
        let mut i = get_input(1);
        i.wait_milliseconds = ms;
        assert!(decode(i, &Bindings::default()).is_err());
    }
    for tick in [None, Some(0), Some(1000001)] {
        let mut i = get_input(1);
        i.wait_milliseconds = 1;
        assert_eq!(
            decode(
                i,
                &Bindings {
                    tick,
                    ..Default::default()
                }
            ),
            Err(MqWireProblem::PendingWait)
        );
    }
    let mut i = get_input(4);
    i.wait_milliseconds = 8;
    assert_eq!(
        decode(i, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
}
#[test]
fn truncation_and_buffer_capacity_are_checked() {
    assert_eq!(
        decode(get_input(4 | 64), &Bindings::default())
            .unwrap()
            .get
            .truncation,
        MqTruncation::Accept
    );
    assert_eq!(
        decode(get_input(4), &Bindings::default())
            .unwrap()
            .get
            .truncation,
        MqTruncation::Reject
    );
    for capacity in [-1, 1048577, 2147483648] {
        let mut i = get_input(4);
        i.buffer_capacity = capacity;
        assert!(decode(i, &Bindings::default()).is_err());
    }
}
#[test]
fn recognized_property_context_async_and_group_options_remain_pending() {
    for bit in [
        8, 128, 512, 1024, 2048, 8192, 16384, 32768, 33554432, 67108864, 134217728, 268435456,
    ] {
        assert!(matches!(
            decode(get_input(bit | 4), &Bindings::default()),
            Err(MqWireProblem::PendingOptions { .. })
        ));
    }
    for bit in [
        128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 2097152,
        8388608, 67108864, 134217728, 268435456,
    ] {
        assert!(matches!(
            decode_put(put_input(bit | 4), &Bindings::default()),
            Err(MqWireProblem::PendingOptions { .. })
        ));
    }
    assert_eq!(
        decode_put(put_input(2 | 4), &Bindings::default()),
        Err(MqWireProblem::IllegalCombination)
    );
}
#[test]
fn put_preserves_message_and_new_id_is_only_existing_generator_intent() {
    let input = put_input(4 | 32);
    let expected = input.message.clone();
    assert_eq!(
        decode_put(input, &Bindings::default()).unwrap().message,
        expected
    );
    let value = decode_put(put_input(4 | 64), &Bindings::default()).unwrap();
    assert_eq!(value.message.descriptor.identifiers.message_id, None);
    assert_eq!(
        value.message.descriptor.identifiers.correlation_id,
        Some(vec![9; 24])
    );
    assert_eq!(value.message.body, vec![0, 255, 1]);
    assert_eq!(
        value.message.descriptor.expiry,
        MqExpiry::RelativeHostTicks(90)
    );
}
#[test]
fn put_one_retains_hconn_and_rejects_unassociated() {
    let (c, _) = handles();
    let request = put_one(
        c,
        lookup(),
        1,
        put_input(4),
        &Bindings::default(),
        MqMessageLimits::default(),
    )
    .unwrap();
    assert!(matches!(request,MqMqiRequest::PutOne {connection,..} if connection==c));
    assert_eq!(
        put_one(
            MqHconn::Unassociated,
            lookup(),
            1,
            put_input(4),
            &Bindings::default(),
            MqMessageLimits::default()
        ),
        Err(MqWireProblem::InvalidConnection)
    );
}

#[test]
fn properties_grouping_and_unreviewed_descriptor_values_never_disappear() {
    let mut input = put_input(4);
    input.message.properties.push(MqMessageProperty {
        name: "P".into(),
        kind: MqPropertyType::ByteString,
        value: vec![1],
    });
    assert_eq!(
        decode_put(input, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = put_input(4);
    input.message.descriptor.ordering.segmentation_allowed = true;
    assert_eq!(
        decode_put(input, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = put_input(4);
    input.message.descriptor.expiry = MqExpiry::PendingSource;
    assert_eq!(
        decode_put(input, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = put_input(4);
    input.message.descriptor.persistence = MqPersistence::PendingSource;
    assert_eq!(
        decode_put(input, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = put_input(4);
    input.message.descriptor.priority = MqPriority::PendingNumeric(9);
    assert_eq!(
        decode_put(input, &Bindings::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = put_input(4);
    input.md_version = 1;
    assert!(decode_put(input, &Bindings::default()).is_ok());
}
#[test]
fn unknown_queue_defaults_never_turn_into_contract_default_success() {
    let b = Bindings {
        policy: false,
        ..Default::default()
    };
    let (c, object) = handles();
    assert_eq!(
        close(
            c,
            MqRouteCloseTarget::Object {
                handle: object,
                lifecycle: MqRouteCloseLifecycle::Predefined
            },
            0,
            &b
        ),
        Err(MqWireProblem::PendingFields)
    );
    assert_eq!(
        open(c, lookup(), 1, 16, &b),
        Err(MqWireProblem::PendingFields)
    );
    assert_eq!(decode(get_input(4), &b), Err(MqWireProblem::PendingFields));
    assert_eq!(
        decode_put(put_input(4), &b),
        Err(MqWireProblem::PendingFields)
    );
    assert_eq!(
        put_one(c, lookup(), 1, put_input(4), &b, MqMessageLimits::default()),
        Err(MqWireProblem::PendingFields)
    );
}
#[test]
fn converted_request_has_exact_existing_canonical_bytes() {
    let input = get_input(4 | 16 | 64);
    let connection = input.connection;
    let object = input.object;
    let actual = decode(input, &Bindings::default()).unwrap();
    let expected = MqMqiGet {
        connection,
        object,
        get: MqGetContract {
            selection: Default::default(),
            mode: MqGetMode::BrowseFirst,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Accept,
            buffer_capacity: 8,
        },
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    };
    let wrap = |value| MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: owner(),
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: MqMqiLimits::default(),
        request: MqMqiRequest::Get(value),
    };
    assert_eq!(
        mq_mqi_request_bytes(&wrap(actual)).unwrap(),
        mq_mqi_request_bytes(&wrap(expected)).unwrap()
    );
}

#[test]
fn mutually_exclusive_reviewed_context_response_and_conditional_sync_are_invalid() {
    for options in [4 | 32 | 16384, 4 | 256 | 512, 4 | 65536 | 131072] {
        assert_eq!(
            decode_put(put_input(options), &Bindings::default()),
            Err(MqWireProblem::IllegalCombination)
        );
    }
    for options in [2 | 4096, 4 | 4096, 16 | 4096, 32 | 4096] {
        assert_eq!(
            decode(get_input(options), &Bindings::default()),
            Err(MqWireProblem::IllegalCombination)
        );
    }
}
