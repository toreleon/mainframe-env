use super::*;
use crate::{
    MqAliasTarget, MqDynamicQueueKind, MqDynamicQueuePattern, MqLifecycleOwner, MqLocalQueueUsage,
    MqObjectDefinition, MqObjectLimits, MqQueueManagerDefinition, MqSubscriptionDestination,
};
use mainframe_env_host_api::mq_object_route::MQ_OBJECT_OPEN_SOURCE;
use mainframe_env_host_api::{MqHandleSharing, MqHostEnvironment, mq_mqi_contract_by_label};

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}

fn catalog() -> MqObjectCatalog {
    MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("Manager.Mixed"),
            default_transmission_queue: Some(name("Transmit")),
        },
        vec![
            MqObjectDefinition::LocalQueue {
                name: name("Local.Mixed"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: Some(name("Process.Mixed")),
            },
            MqObjectDefinition::LocalQueue {
                name: name("Transmit"),
                usage: MqLocalQueueUsage::Transmission,
                trigger_process: None,
            },
            MqObjectDefinition::AliasQueue {
                name: name("Alias"),
                target: MqAliasTarget::Queue(name("Local.Mixed")),
            },
            MqObjectDefinition::AliasQueue {
                name: name("TopicAlias"),
                target: MqAliasTarget::Topic(name("Topic")),
            },
            MqObjectDefinition::RemoteQueue {
                name: name("Remote"),
                remote_queue: Some(name("Destination")),
                remote_queue_manager: name("Elsewhere"),
                transmission_queue: Some(name("Transmit")),
            },
            MqObjectDefinition::ModelQueue {
                name: name("Model"),
                definition_type: MqDynamicQueueKind::Temporary,
                trigger_process: None,
            },
            MqObjectDefinition::Process {
                name: name("Process.Mixed"),
            },
            // Same spelling in two namespaces must not confuse the inquiry.
            MqObjectDefinition::Process {
                name: name("Local.Mixed"),
            },
            MqObjectDefinition::Topic {
                name: name("Topic"),
            },
            MqObjectDefinition::Subscription {
                name: name("Subscription"),
                topic: name("Topic"),
                destination: MqSubscriptionDestination::Managed,
                durable: true,
            },
        ],
        MqObjectLimits::default(),
    )
    .unwrap()
}

fn owner(environment: MqHostEnvironment) -> MqHandleOwner {
    MqHandleOwner {
        environment,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}

struct Fixture {
    catalog: MqObjectCatalog,
    handles: MqHandleRegistry,
    owner: MqHandleOwner,
    connection: MqHconn,
    handle: MqHandle,
}

impl Fixture {
    fn new(environment: MqHostEnvironment) -> Self {
        let mut handles = MqHandleRegistry::new(1, 16).unwrap();
        let owner = owner(environment);
        let connection = if environment == MqHostEnvironment::ZosCics {
            handles.bind_cics_default(owner).unwrap()
        } else {
            handles.connect(owner, MqHandleSharing::NonShared).unwrap()
        };
        let handle = handles.create_object(owner, connection).unwrap().into();
        Self {
            catalog: catalog(),
            handles,
            owner,
            connection,
            handle,
        }
    }

    fn binding<'a>(&self, lookup: &'a MqObjectLookup) -> MqInquiryBinding<'a> {
        MqInquiryBinding {
            owner: self.owner,
            connection: self.connection,
            handle: self.handle,
            lookup,
            open_access: &[MqRouteOpenAccess::Inquire],
        }
    }

    fn inquire(
        &self,
        lookup: &MqObjectLookup,
        selectors: &[MqInquirySelector],
        capacity: MqInquiryCapacity,
    ) -> Result<MqInquiryResult, MqInquiryError> {
        let before = self.catalog.encode().unwrap();
        let active = self.handles.active_handles();
        let request = MqInquiryRequest::new(selectors, capacity)?;
        let result = inquire_object(&self.catalog, &self.handles, self.binding(lookup), &request);
        assert_eq!(self.catalog.encode().unwrap(), before);
        assert_eq!(self.handles.active_handles(), active);
        result
    }
}

fn selector(attribute: MqInquiryAttribute) -> MqInquirySelector {
    MqInquirySelector::Named(attribute)
}

fn capacity() -> MqInquiryCapacity {
    MqInquiryCapacity {
        integer_attributes: MQ_INQUIRY_MAX_SELECTORS,
        character_attributes: MQ_INQUIRY_MAX_SELECTORS,
        character_bytes: MQ_INQUIRY_MAX_CHARACTER_BYTES,
    }
}

#[test]
fn reviewed_sources_and_named_fields_bind_to_existing_contract() {
    let inq = mq_mqi_contract_by_label("MQINQ").unwrap();
    assert_eq!(inq.official_row, MQ_INQUIRY_SOURCE.row);
    assert_eq!(inq.topic_path, MQ_INQUIRY_SOURCE.topic);
    assert_eq!(inq.topic_sha256, MQ_INQUIRY_SOURCE.sha256);
    assert_eq!(
        MQ_OBJECT_OPEN_SOURCE.row,
        mq_mqi_contract_by_label("MQOPEN").unwrap().official_row
    );
    for attribute in [
        MqInquiryAttribute::QueueName,
        MqInquiryAttribute::QueueType,
        MqInquiryAttribute::ProcessName,
        MqInquiryAttribute::QueueManagerName,
        MqInquiryAttribute::DefaultTransmissionQueueName,
        MqInquiryAttribute::BaseQueueNamePending,
        MqInquiryAttribute::QueueUsagePending,
        MqInquiryAttribute::CurrentQueueDepthPending,
    ] {
        assert_eq!(
            MqInquiryAttribute::from_symbol(attribute.symbol()),
            Ok(attribute)
        );
    }
    for symbol in ["", "MQCA_q_NAME", " MQCA_Q_NAME", "MQIA_MADE_UP", "20"] {
        assert_eq!(
            MqInquiryAttribute::from_symbol(symbol),
            Err(MqInquiryError::UnknownSelector)
        );
    }
}

#[test]
fn mixed_selectors_preserve_type_order_and_every_duplicate() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    let lookup = MqObjectLookup::Queue(name("Alias"));
    let selectors = [
        selector(MqInquiryAttribute::QueueName),
        selector(MqInquiryAttribute::QueueType),
        selector(MqInquiryAttribute::QueueName),
        selector(MqInquiryAttribute::QueueType),
    ];
    let result = f.inquire(&lookup, &selectors, capacity()).unwrap();
    assert_eq!(
        result.integers,
        vec![MqInquiryInteger::QueueType(MqObjectKind::AliasQueue); 2]
    );
    assert_eq!(
        result.characters,
        vec![
            MqInquiryCharacter {
                attribute: MqInquiryAttribute::QueueName,
                name: Some(name("Alias")),
            };
            2
        ]
    );
    assert_eq!(
        result.used,
        MqInquiryCapacity {
            integer_attributes: 2,
            character_attributes: 2,
            character_bytes: 10,
        }
    );
    assert_eq!(
        f.inquire(&lookup, &selectors, result.used),
        Ok(result.clone())
    );
    // Retries are pure reads and do not add replay rows or consume handles.
    assert_eq!(f.inquire(&lookup, &selectors, capacity()), Ok(result));
}

#[test]
fn manager_character_order_and_unset_optional_field_are_preserved() {
    let mut f = Fixture::new(MqHostEnvironment::ZosBatch);
    let selectors = [
        selector(MqInquiryAttribute::DefaultTransmissionQueueName),
        selector(MqInquiryAttribute::QueueManagerName),
        selector(MqInquiryAttribute::DefaultTransmissionQueueName),
    ];
    let result = f
        .inquire(&MqObjectLookup::QueueManager, &selectors, capacity())
        .unwrap();
    assert_eq!(
        result
            .characters
            .iter()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(name("Transmit")),
            Some(name("Manager.Mixed")),
            Some(name("Transmit"))
        ]
    );
    assert_eq!(result.used.character_bytes, 29);
    f.catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("Manager.Mixed"),
            default_transmission_queue: None,
        },
        vec![],
        MqObjectLimits::default(),
    )
    .unwrap();
    let result = f
        .inquire(
            &MqObjectLookup::QueueManager,
            &selectors[..1],
            MqInquiryCapacity {
                character_attributes: 1,
                ..MqInquiryCapacity::default()
            },
        )
        .unwrap();
    assert_eq!(result.characters[0].name, None);
    assert_eq!(result.used.character_bytes, 0);
}

#[test]
fn namespace_and_local_remote_identity_come_from_one_catalog() {
    let f = Fixture::new(MqHostEnvironment::OtherBindings);
    for (queue, kind) in [
        ("Local.Mixed", MqObjectKind::LocalQueue),
        ("Remote", MqObjectKind::RemoteQueue),
    ] {
        let result = f
            .inquire(
                &MqObjectLookup::Queue(name(queue)),
                &[
                    selector(MqInquiryAttribute::QueueType),
                    selector(MqInquiryAttribute::QueueName),
                ],
                capacity(),
            )
            .unwrap();
        assert_eq!(result.integers, vec![MqInquiryInteger::QueueType(kind)]);
        assert_eq!(result.characters[0].name, Some(name(queue)));
    }
    let result = f
        .inquire(
            &MqObjectLookup::Process(name("Local.Mixed")),
            &[selector(MqInquiryAttribute::ProcessName)],
            capacity(),
        )
        .unwrap();
    assert_eq!(result.characters[0].name, Some(name("Local.Mixed")));
}

#[test]
fn dynamic_local_inquiry_survives_catalog_restart_without_opening_model() {
    let mut f = Fixture::new(MqHostEnvironment::ZosBatch);
    assert_eq!(
        f.inquire(&MqObjectLookup::Queue(name("Model")), &[], capacity()),
        Err(MqInquiryError::Pending(
            MqInquiryPending::ModelRequiresDynamicQueue
        ))
    );
    let instance = f
        .catalog
        .create_model_instance(
            &name("Model"),
            &MqDynamicQueuePattern::new("Dynamic.*").unwrap(),
            MqLifecycleOwner::new("test-owner").unwrap(),
        )
        .unwrap();
    let lookup = MqObjectLookup::Queue(instance.name.clone());
    let selectors = [
        selector(MqInquiryAttribute::QueueName),
        selector(MqInquiryAttribute::QueueType),
    ];
    let expected = f.inquire(&lookup, &selectors, capacity()).unwrap();
    assert_eq!(expected.characters[0].name, Some(instance.name));
    assert_eq!(
        expected.integers,
        vec![MqInquiryInteger::QueueType(MqObjectKind::LocalQueue)]
    );
    let bytes = f.catalog.encode().unwrap();
    f.catalog = MqObjectCatalog::decode(&bytes, MqObjectLimits::default()).unwrap();
    assert_eq!(f.inquire(&lookup, &selectors, capacity()), Ok(expected));
    assert_eq!(f.catalog.encode().unwrap(), bytes);
}

#[test]
fn zero_and_maximum_selector_and_output_boundaries() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    let lookup = MqObjectLookup::Queue(name("Local.Mixed"));
    assert_eq!(
        f.inquire(&lookup, &[], MqInquiryCapacity::default()),
        Ok(MqInquiryResult::default())
    );
    let selectors = vec![selector(MqInquiryAttribute::QueueType); MQ_INQUIRY_MAX_SELECTORS];
    assert_eq!(
        f.inquire(&lookup, &selectors, capacity())
            .unwrap()
            .integers
            .len(),
        256
    );
    let too_many = vec![selector(MqInquiryAttribute::QueueType); MQ_INQUIRY_MAX_SELECTORS + 1];
    assert_eq!(
        MqInquiryRequest::new(&too_many, capacity()),
        Err(MqInquiryError::SelectorLimit)
    );
    let max_name = "a".repeat(crate::MQ_OBJECT_NAME_BYTES);
    let mut f = f;
    f.catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("Manager.Mixed"),
            default_transmission_queue: None,
        },
        vec![MqObjectDefinition::LocalQueue {
            name: name(&max_name),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: None,
        }],
        MqObjectLimits::default(),
    )
    .unwrap();
    let result = f
        .inquire(
            &MqObjectLookup::Queue(name(&max_name)),
            &vec![selector(MqInquiryAttribute::QueueName); MQ_INQUIRY_MAX_SELECTORS],
            capacity(),
        )
        .unwrap();
    assert_eq!(result.used.character_bytes, MQ_INQUIRY_MAX_CHARACTER_BYTES);
    assert_eq!(result.characters.len(), MQ_INQUIRY_MAX_SELECTORS);
    for invalid in [
        MqInquiryCapacity {
            integer_attributes: usize::MAX,
            ..capacity()
        },
        MqInquiryCapacity {
            character_attributes: MQ_INQUIRY_MAX_SELECTORS + 1,
            ..capacity()
        },
        MqInquiryCapacity {
            character_bytes: MQ_INQUIRY_MAX_CHARACTER_BYTES + 1,
            ..capacity()
        },
    ] {
        assert_eq!(
            MqInquiryRequest::new(&[], invalid),
            Err(MqInquiryError::CapacityLimit)
        );
    }
}

#[test]
fn each_short_output_budget_fails_without_partial_output_or_catalog_mutation() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    let lookup = MqObjectLookup::Queue(name("Local.Mixed"));
    let selectors = [
        selector(MqInquiryAttribute::QueueName),
        selector(MqInquiryAttribute::QueueType),
    ];
    let required = MqInquiryCapacity {
        integer_attributes: 1,
        character_attributes: 1,
        character_bytes: 11,
    };
    for short in [
        MqInquiryCapacity::default(),
        MqInquiryCapacity {
            integer_attributes: 0,
            ..required
        },
        MqInquiryCapacity {
            character_attributes: 0,
            ..required
        },
        MqInquiryCapacity {
            character_bytes: 10,
            ..required
        },
    ] {
        assert_eq!(
            f.inquire(&lookup, &selectors, short),
            Err(MqInquiryError::OutputTooSmall { required })
        );
    }
    assert_eq!(
        f.inquire(&lookup, &selectors, required).unwrap().used,
        required
    );
}

#[test]
fn invalid_applicability_and_pending_selectors_fail_closed_after_a_valid_prefix() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    for (lookup, valid, invalid) in [
        (
            MqObjectLookup::Queue(name("Local.Mixed")),
            MqInquiryAttribute::QueueName,
            MqInquiryAttribute::QueueManagerName,
        ),
        (
            MqObjectLookup::QueueManager,
            MqInquiryAttribute::QueueManagerName,
            MqInquiryAttribute::QueueType,
        ),
        (
            MqObjectLookup::Process(name("Process.Mixed")),
            MqInquiryAttribute::ProcessName,
            MqInquiryAttribute::QueueName,
        ),
        (
            MqObjectLookup::Process(name("Process.Mixed")),
            MqInquiryAttribute::ProcessName,
            MqInquiryAttribute::DefaultTransmissionQueueName,
        ),
    ] {
        // Selector validation precedes private capacity errors, even after a valid field.
        assert_eq!(
            f.inquire(
                &lookup,
                &[selector(valid), selector(invalid)],
                MqInquiryCapacity::default()
            ),
            Err(MqInquiryError::InvalidSelectorForObject(invalid))
        );
    }
    let lookup = MqObjectLookup::Queue(name("Local.Mixed"));
    for (attribute, pending) in [
        (
            MqInquiryAttribute::ProcessName,
            MqInquiryPending::AttributeApplicability(MqInquiryAttribute::ProcessName),
        ),
        (
            MqInquiryAttribute::BaseQueueNamePending,
            MqInquiryPending::AttributeApplicability(MqInquiryAttribute::BaseQueueNamePending),
        ),
        (
            MqInquiryAttribute::QueueUsagePending,
            MqInquiryPending::AttributeValue(MqInquiryAttribute::QueueUsagePending),
        ),
        (
            MqInquiryAttribute::CurrentQueueDepthPending,
            MqInquiryPending::AttributeValue(MqInquiryAttribute::CurrentQueueDepthPending),
        ),
    ] {
        assert_eq!(
            f.inquire(
                &lookup,
                &[selector(MqInquiryAttribute::QueueName), selector(attribute)],
                capacity()
            ),
            Err(MqInquiryError::Pending(pending))
        );
    }
    for numeric in [i32::MIN, -1, 0, 20, i32::MAX] {
        assert_eq!(
            f.inquire(
                &lookup,
                &[
                    selector(MqInquiryAttribute::QueueName),
                    MqInquirySelector::NumericPending(numeric)
                ],
                capacity()
            ),
            Err(MqInquiryError::Pending(MqInquiryPending::NumericSelector))
        );
    }
    assert_eq!(
        pending_character_layout(),
        MqInquiryError::Pending(MqInquiryPending::CharacterLayout)
    );
    assert_eq!(
        pending_mqset(),
        MqInquiryError::Pending(MqInquiryPending::MqSet)
    );
}

#[test]
fn unsupported_objects_and_missing_names_never_report_success() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    for (lookup, kind) in [
        (MqObjectLookup::Topic(name("Topic")), MqObjectKind::Topic),
        (
            MqObjectLookup::Subscription(name("Subscription")),
            MqObjectKind::Subscription,
        ),
    ] {
        assert_eq!(
            f.inquire(&lookup, &[], capacity()),
            Err(MqInquiryError::Pending(MqInquiryPending::ObjectKind(kind)))
        );
    }
    // The existing catalog does not grant inquiry on topic-target aliases.
    assert_eq!(
        f.inquire(
            &MqObjectLookup::Queue(name("TopicAlias")),
            &[selector(MqInquiryAttribute::QueueName)],
            capacity()
        ),
        Err(MqInquiryError::Object(MqObjectError::UnsupportedCapability))
    );
    for lookup in [
        MqObjectLookup::Queue(name("Missing")),
        MqObjectLookup::Process(name("Missing")),
    ] {
        assert_eq!(
            f.inquire(&lookup, &[], capacity()),
            Err(MqInquiryError::Object(MqObjectError::UnknownObject))
        );
    }
}

#[test]
fn inquire_access_is_required_even_for_zero_selectors() {
    let f = Fixture::new(MqHostEnvironment::MqiClient);
    let lookup = MqObjectLookup::Queue(name("Local.Mixed"));
    let request = MqInquiryRequest::new(&[], capacity()).unwrap();
    for access in [
        &[][..],
        &[MqRouteOpenAccess::Output][..],
        &[MqRouteOpenAccess::Set][..],
    ] {
        let binding = MqInquiryBinding {
            open_access: access,
            ..f.binding(&lookup)
        };
        assert_eq!(
            inquire_object(&f.catalog, &f.handles, binding, &request),
            Err(MqInquiryError::NotOpenForInquire)
        );
    }
    let access = [MqRouteOpenAccess::Output, MqRouteOpenAccess::Inquire];
    assert_eq!(
        inquire_object(
            &f.catalog,
            &f.handles,
            MqInquiryBinding {
                open_access: &access,
                ..f.binding(&lookup)
            },
            &request
        ),
        Ok(MqInquiryResult::default())
    );
}

#[test]
fn handle_role_owner_connection_generation_and_registry_are_rechecked() {
    let mut f = Fixture::new(MqHostEnvironment::MqiClient);
    let lookup = MqObjectLookup::Queue(name("Local.Mixed"));
    let request = MqInquiryRequest::new(&[], capacity()).unwrap();
    let before = f.catalog.encode().unwrap();
    let other = f
        .handles
        .connect(f.owner, MqHandleSharing::NonShared)
        .unwrap();
    let message = f.handles.create_message(f.owner, f.connection).unwrap();
    let sub = f
        .handles
        .create_subscription(f.owner, f.connection)
        .unwrap();
    let mut cross_owner = f.owner;
    cross_owner.process_id += 1;
    for (binding, error) in [
        (
            MqInquiryBinding {
                handle: message.into(),
                ..f.binding(&lookup)
            },
            MqHandleProblem::WrongKind,
        ),
        (
            MqInquiryBinding {
                handle: sub.into(),
                ..f.binding(&lookup)
            },
            MqHandleProblem::WrongKind,
        ),
        (
            MqInquiryBinding {
                connection: other,
                ..f.binding(&lookup)
            },
            MqHandleProblem::CrossConnection,
        ),
        (
            MqInquiryBinding {
                owner: cross_owner,
                ..f.binding(&lookup)
            },
            MqHandleProblem::CrossOwner,
        ),
        (
            MqInquiryBinding {
                connection: MqHconn::Unassociated,
                ..f.binding(&lookup)
            },
            MqHandleProblem::SpecialConnection,
        ),
    ] {
        let active = f.handles.active_handles();
        assert_eq!(
            inquire_object(&f.catalog, &f.handles, binding, &request),
            Err(MqInquiryError::Handle(error))
        );
        assert_eq!(f.handles.active_handles(), active);
    }
    let other_registry = MqHandleRegistry::new(1, 16).unwrap();
    assert_eq!(
        inquire_object(&f.catalog, &other_registry, f.binding(&lookup), &request),
        Err(MqInquiryError::Handle(MqHandleProblem::Stale))
    );
    let old = f.handle;
    f.handles
        .release(f.owner, f.connection, old, MqHandleKind::Object)
        .unwrap();
    f.handle = f
        .handles
        .create_object(f.owner, f.connection)
        .unwrap()
        .into();
    assert_eq!(
        inquire_object(
            &f.catalog,
            &f.handles,
            MqInquiryBinding {
                handle: old,
                ..f.binding(&lookup)
            },
            &request
        ),
        Err(MqInquiryError::Handle(MqHandleProblem::Stale))
    );
    assert_eq!(
        f.inquire(&lookup, &[], capacity()),
        Ok(MqInquiryResult::default())
    );
    f.handles.advance_epoch(2).unwrap();
    assert_eq!(
        f.inquire(&lookup, &[], capacity()),
        Err(MqInquiryError::Handle(MqHandleProblem::Stale))
    );
    assert_eq!(f.catalog.encode().unwrap(), before);
}

#[test]
fn existing_host_owner_contract_admits_inquiry_and_cics_default_expires() {
    for environment in [
        MqHostEnvironment::ZosBatch,
        MqHostEnvironment::ZosImsBatchDli,
        MqHostEnvironment::ZosCics,
        MqHostEnvironment::ZosIms,
        MqHostEnvironment::MqiClient,
        MqHostEnvironment::OtherBindings,
    ] {
        let f = Fixture::new(environment);
        let result = f
            .inquire(
                &MqObjectLookup::QueueManager,
                &[selector(MqInquiryAttribute::QueueManagerName)],
                capacity(),
            )
            .unwrap();
        assert_eq!(result.characters[0].name, Some(name("Manager.Mixed")));
    }
    let mut f = Fixture::new(MqHostEnvironment::ZosCics);
    f.handles.end_processing_unit(f.owner).unwrap();
    assert_eq!(
        f.inquire(&MqObjectLookup::QueueManager, &[], capacity()),
        Err(MqInquiryError::Handle(MqHandleProblem::MissingConnection))
    );
}
