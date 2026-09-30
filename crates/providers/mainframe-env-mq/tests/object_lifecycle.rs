use mainframe_env_mq::{
    MqAliasTarget, MqChannelRoute, MqCloseMode, MqCloseOutcome, MqDynamicQueueKind,
    MqDynamicQueuePattern, MqDynamicQueueState, MqLifecycleOwner, MqLimits, MqLocalQueueUsage,
    MqObjectCapability, MqObjectCatalog, MqObjectDefinition, MqObjectError, MqObjectKind,
    MqObjectLimits, MqObjectLookup, MqObjectName, MqQueueDefinition, MqQueueManagerDefinition,
    MqResolvedTarget, MqSubscriptionDestination,
};
use std::collections::BTreeSet;

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}

fn owner(value: &str) -> MqLifecycleOwner {
    MqLifecycleOwner::new(value).unwrap()
}

fn capabilities(values: &[MqObjectCapability]) -> BTreeSet<MqObjectCapability> {
    values.iter().copied().collect()
}

fn manager() -> MqQueueManagerDefinition {
    MqQueueManagerDefinition {
        name: name("LOCAL.QM"),
        default_transmission_queue: Some(name("SYSTEM.XMITQ")),
    }
}

fn complete_definitions() -> Vec<MqObjectDefinition> {
    vec![
        MqObjectDefinition::LocalQueue {
            name: name("LOCAL.Q"),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: Some(name("PROCESS.A")),
        },
        MqObjectDefinition::LocalQueue {
            name: name("SYSTEM.XMITQ"),
            usage: MqLocalQueueUsage::Transmission,
            trigger_process: None,
        },
        MqObjectDefinition::AliasQueue {
            name: name("ALIAS.Q"),
            target: MqAliasTarget::Queue(name("LOCAL.Q")),
        },
        MqObjectDefinition::RemoteQueue {
            name: name("REMOTE.Q"),
            remote_queue: Some(name("TARGET.Q")),
            remote_queue_manager: name("QM.ALIAS"),
            transmission_queue: None,
        },
        MqObjectDefinition::RemoteQueue {
            name: name("QM.ALIAS"),
            remote_queue: None,
            remote_queue_manager: name("REMOTE.QM"),
            transmission_queue: None,
        },
        MqObjectDefinition::ModelQueue {
            name: name("MODEL.Q"),
            definition_type: MqDynamicQueueKind::Permanent,
            trigger_process: None,
        },
        MqObjectDefinition::Topic {
            name: name("TOPIC.A"),
        },
        MqObjectDefinition::AliasQueue {
            name: name("TOPIC.ALIAS"),
            target: MqAliasTarget::Topic(name("TOPIC.A")),
        },
        MqObjectDefinition::Subscription {
            name: name("SUB.A"),
            topic: name("TOPIC.A"),
            destination: MqSubscriptionDestination::Managed,
            durable: true,
        },
        MqObjectDefinition::Process {
            name: name("PROCESS.A"),
        },
    ]
}

#[test]
fn names_kinds_and_capabilities_are_bounded_and_typed() {
    assert_eq!(name("request.q   ").as_str(), "request.q");
    assert_eq!(name("request.q\0").as_str(), "request.q");
    assert_eq!(name("request.q\0ignored").as_str(), "request.q");
    assert_ne!(name("request.q"), name("REQUEST.Q"));
    assert_eq!(
        MqObjectName::new(" REQUEST.Q"),
        Err(MqObjectError::InvalidName)
    );
    assert_eq!(
        MqObjectName::new("REQUEST Q"),
        Err(MqObjectError::InvalidName)
    );
    assert_eq!(
        MqObjectName::new("REQUEST*Q"),
        Err(MqObjectError::InvalidName)
    );
    assert_eq!(MqObjectName::new("é"), Err(MqObjectError::InvalidName));
    assert_eq!(
        MqObjectName::new("A".repeat(49)),
        Err(MqObjectError::InvalidName)
    );

    let catalog =
        MqObjectCatalog::new(manager(), complete_definitions(), MqObjectLimits::default()).unwrap();
    let matrix = [
        (
            MqObjectLookup::QueueManager,
            MqObjectKind::QueueManager,
            capabilities(&[MqObjectCapability::Inquire]),
        ),
        (
            MqObjectLookup::Queue(name("LOCAL.Q")),
            MqObjectKind::LocalQueue,
            capabilities(&[
                MqObjectCapability::Input,
                MqObjectCapability::Browse,
                MqObjectCapability::Output,
                MqObjectCapability::Inquire,
                MqObjectCapability::Set,
            ]),
        ),
        (
            MqObjectLookup::Queue(name("ALIAS.Q")),
            MqObjectKind::AliasQueue,
            capabilities(&[
                MqObjectCapability::Input,
                MqObjectCapability::Browse,
                MqObjectCapability::Output,
                MqObjectCapability::Inquire,
                MqObjectCapability::Set,
            ]),
        ),
        (
            MqObjectLookup::Queue(name("REMOTE.Q")),
            MqObjectKind::RemoteQueue,
            capabilities(&[
                MqObjectCapability::Output,
                MqObjectCapability::Inquire,
                MqObjectCapability::Set,
            ]),
        ),
        (
            MqObjectLookup::Queue(name("MODEL.Q")),
            MqObjectKind::ModelQueue,
            capabilities(&[
                MqObjectCapability::Input,
                MqObjectCapability::Browse,
                MqObjectCapability::Output,
                MqObjectCapability::Inquire,
                MqObjectCapability::Set,
            ]),
        ),
        (
            MqObjectLookup::Topic(name("TOPIC.A")),
            MqObjectKind::Topic,
            capabilities(&[MqObjectCapability::Publish, MqObjectCapability::Subscribe]),
        ),
        (
            MqObjectLookup::Subscription(name("SUB.A")),
            MqObjectKind::Subscription,
            capabilities(&[MqObjectCapability::RequestPublications]),
        ),
        (
            MqObjectLookup::Process(name("PROCESS.A")),
            MqObjectKind::Process,
            capabilities(&[MqObjectCapability::Inquire]),
        ),
    ];
    for (lookup, kind, expected) in matrix {
        assert_eq!(catalog.kind(&lookup), Ok(kind));
        assert_eq!(catalog.capabilities(&lookup), Ok(expected));
    }

    let case_distinct = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("Case.QM"),
            default_transmission_queue: None,
        },
        vec![
            MqObjectDefinition::LocalQueue {
                name: name("request.q"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            },
            MqObjectDefinition::LocalQueue {
                name: name("REQUEST.Q"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            },
        ],
        MqObjectLimits::default(),
    )
    .unwrap();
    let case_distinct =
        MqObjectCatalog::decode(&case_distinct.encode().unwrap(), MqObjectLimits::default())
            .unwrap();
    for expected in ["request.q", "REQUEST.Q"] {
        assert_eq!(
            case_distinct
                .resolve(
                    &MqObjectLookup::Queue(name(expected)),
                    MqObjectCapability::Output,
                )
                .unwrap()
                .target,
            MqResolvedTarget::Queue {
                name: name(expected),
                dynamic: false,
                model: None,
            }
        );
    }
}

#[test]
fn alias_and_remote_resolution_is_deterministic_and_fail_closed() {
    let catalog =
        MqObjectCatalog::new(manager(), complete_definitions(), MqObjectLimits::default()).unwrap();

    let local = catalog
        .resolve(
            &MqObjectLookup::Queue(name("ALIAS.Q")),
            MqObjectCapability::Input,
        )
        .unwrap();
    assert_eq!(
        local.target,
        MqResolvedTarget::Queue {
            name: name("LOCAL.Q"),
            dynamic: false,
            model: None,
        }
    );
    assert_eq!(
        local
            .path
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>(),
        [MqObjectKind::AliasQueue, MqObjectKind::LocalQueue]
    );

    let remote = catalog
        .resolve(
            &MqObjectLookup::Queue(name("REMOTE.Q")),
            MqObjectCapability::Output,
        )
        .unwrap();
    assert_eq!(
        remote.target,
        MqResolvedTarget::Remote(MqChannelRoute {
            local_definition: name("REMOTE.Q"),
            remote_queue: name("TARGET.Q"),
            remote_queue_manager: name("REMOTE.QM"),
            transmission_queue: name("SYSTEM.XMITQ"),
        })
    );
    assert_eq!(
        catalog.resolve(
            &MqObjectLookup::Queue(name("REMOTE.Q")),
            MqObjectCapability::Input,
        ),
        Err(MqObjectError::UnsupportedCapability)
    );

    let topic = catalog
        .resolve(
            &MqObjectLookup::Queue(name("TOPIC.ALIAS")),
            MqObjectCapability::Publish,
        )
        .unwrap();
    assert_eq!(
        topic.target,
        MqResolvedTarget::Topic {
            name: name("TOPIC.A")
        }
    );

    let alias_cycle = vec![
        MqObjectDefinition::AliasQueue {
            name: name("A"),
            target: MqAliasTarget::Queue(name("B")),
        },
        MqObjectDefinition::AliasQueue {
            name: name("B"),
            target: MqAliasTarget::Queue(name("A")),
        },
    ];
    assert_eq!(
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            alias_cycle,
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::ResolutionCycle)
    );

    let shallow = MqObjectLimits {
        max_resolution_depth: 2,
        ..MqObjectLimits::default()
    };
    let too_deep = vec![
        MqObjectDefinition::LocalQueue {
            name: name("BASE"),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: None,
        },
        MqObjectDefinition::AliasQueue {
            name: name("C"),
            target: MqAliasTarget::Queue(name("BASE")),
        },
        MqObjectDefinition::AliasQueue {
            name: name("B"),
            target: MqAliasTarget::Queue(name("C")),
        },
        MqObjectDefinition::AliasQueue {
            name: name("A"),
            target: MqAliasTarget::Queue(name("B")),
        },
    ];
    assert_eq!(
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            too_deep,
            shallow,
        ),
        Err(MqObjectError::ResolutionDepthExceeded)
    );

    let invalid_xmit = vec![
        MqObjectDefinition::LocalQueue {
            name: name("NOT.XMIT"),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: None,
        },
        MqObjectDefinition::RemoteQueue {
            name: name("REMOTE"),
            remote_queue: Some(name("TARGET")),
            remote_queue_manager: name("REMOTE.QM"),
            transmission_queue: Some(name("NOT.XMIT")),
        },
    ];
    assert_eq!(
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            invalid_xmit,
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::InvalidReferenceKind)
    );

    let remote_cycle = vec![
        MqObjectDefinition::LocalQueue {
            name: name("XMIT"),
            usage: MqLocalQueueUsage::Transmission,
            trigger_process: None,
        },
        MqObjectDefinition::RemoteQueue {
            name: name("REMOTE"),
            remote_queue: Some(name("TARGET")),
            remote_queue_manager: name("QM.A"),
            transmission_queue: Some(name("XMIT")),
        },
        MqObjectDefinition::RemoteQueue {
            name: name("QM.A"),
            remote_queue: None,
            remote_queue_manager: name("QM.B"),
            transmission_queue: Some(name("XMIT")),
        },
        MqObjectDefinition::RemoteQueue {
            name: name("QM.B"),
            remote_queue: None,
            remote_queue_manager: name("QM.A"),
            transmission_queue: Some(name("XMIT")),
        },
    ];
    assert_eq!(
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            remote_cycle,
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::ResolutionCycle)
    );
}

#[test]
fn model_instances_survive_restart_and_obey_close_lifecycle() {
    let definitions = vec![
        MqObjectDefinition::ModelQueue {
            name: name("TEMP.MODEL"),
            definition_type: MqDynamicQueueKind::Temporary,
            trigger_process: Some(name("PROCESS.A")),
        },
        MqObjectDefinition::ModelQueue {
            name: name("PERM.MODEL"),
            definition_type: MqDynamicQueueKind::Permanent,
            trigger_process: None,
        },
        MqObjectDefinition::Process {
            name: name("PROCESS.A"),
        },
    ];
    let mut catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("QM"),
            default_transmission_queue: None,
        },
        definitions,
        MqObjectLimits::default(),
    )
    .unwrap();
    let creator = owner("execution-a");
    let other = owner("execution-b");
    let temporary = catalog
        .create_model_instance(
            &name("TEMP.MODEL"),
            &MqDynamicQueuePattern::new("app.temp.*").unwrap(),
            creator.clone(),
        )
        .unwrap();
    assert_eq!(temporary.name.as_str(), "app.temp.0000000000000001");

    let bytes = catalog.encode().unwrap();
    let mut restarted = MqObjectCatalog::decode(&bytes, MqObjectLimits::default()).unwrap();
    let resolved = restarted
        .resolve(
            &MqObjectLookup::Queue(temporary.name.clone()),
            MqObjectCapability::Input,
        )
        .unwrap();
    assert_eq!(
        resolved.target,
        MqResolvedTarget::Queue {
            name: temporary.name.clone(),
            dynamic: true,
            model: Some(name("TEMP.MODEL")),
        }
    );
    let permanent = restarted
        .create_model_instance(
            &name("PERM.MODEL"),
            &MqDynamicQueuePattern::new("APP.PERM.Q").unwrap(),
            creator.clone(),
        )
        .unwrap();
    assert_eq!(permanent.instance_id, 2);

    assert_eq!(
        restarted.close_model_instance(
            &temporary.name,
            &other,
            MqCloseMode::Retain,
            MqDynamicQueueState {
                messages: 3,
                pending_updates: 1,
            },
            false,
        ),
        Ok(MqCloseOutcome::Retained)
    );
    assert_eq!(
        restarted.close_model_instance(
            &temporary.name,
            &other,
            MqCloseMode::DeletePurge,
            MqDynamicQueueState::default(),
            true,
        ),
        Err(MqObjectError::InvalidCloseMode)
    );
    assert_eq!(
        restarted.close_model_instance(
            &temporary.name,
            &creator,
            MqCloseMode::Retain,
            MqDynamicQueueState {
                messages: 3,
                pending_updates: 1,
            },
            false,
        ),
        Ok(MqCloseOutcome::Deleted { purged_messages: 3 })
    );

    assert_eq!(
        restarted.close_model_instance(
            &permanent.name,
            &other,
            MqCloseMode::Delete,
            MqDynamicQueueState::default(),
            false,
        ),
        Err(MqObjectError::NotAuthorized)
    );
    assert_eq!(
        restarted.close_model_instance(
            &permanent.name,
            &creator,
            MqCloseMode::Delete,
            MqDynamicQueueState {
                messages: 1,
                pending_updates: 0,
            },
            false,
        ),
        Err(MqObjectError::ObjectInUse)
    );
    assert_eq!(
        restarted.close_model_instance(
            &permanent.name,
            &creator,
            MqCloseMode::DeletePurge,
            MqDynamicQueueState {
                messages: 2,
                pending_updates: 1,
            },
            false,
        ),
        Err(MqObjectError::ObjectInUse)
    );
    assert_eq!(
        restarted.close_model_instance(
            &permanent.name,
            &creator,
            MqCloseMode::DeletePurge,
            MqDynamicQueueState {
                messages: 2,
                pending_updates: 0,
            },
            false,
        ),
        Ok(MqCloseOutcome::Deleted { purged_messages: 2 })
    );
}

#[test]
fn strict_snapshot_and_queue_only_migration_reject_corruption() {
    let migrated = MqObjectCatalog::from_queue_definitions(
        MqQueueManagerDefinition {
            name: name("QM"),
            default_transmission_queue: None,
        },
        &[
            MqQueueDefinition {
                name: "REQUEST.Q".into(),
                trigger_program: Some("PROCESS.A".into()),
            },
            MqQueueDefinition {
                name: "REPLY.Q".into(),
                trigger_program: None,
            },
        ],
        MqObjectLimits::default(),
    )
    .unwrap();
    assert_eq!(
        migrated.kind(&MqObjectLookup::Queue(name("REQUEST.Q"))),
        Ok(MqObjectKind::LocalQueue)
    );
    assert_eq!(
        migrated.kind(&MqObjectLookup::Process(name("PROCESS.A"))),
        Ok(MqObjectKind::Process)
    );

    let bytes = migrated.encode().unwrap();
    let decoded = MqObjectCatalog::decode(&bytes, MqObjectLimits::default()).unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);

    let mut unsupported: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unsupported["schema_version"] = serde_json::json!("mainframe-env.mq-object-catalog@999");
    assert_eq!(
        MqObjectCatalog::decode(
            &serde_json::to_vec(&unsupported).unwrap(),
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::UnsupportedSchema)
    );

    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unknown["forged"] = serde_json::json!(true);
    assert_eq!(
        MqObjectCatalog::decode(
            &serde_json::to_vec(&unknown).unwrap(),
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::CorruptSnapshot)
    );

    let mut reordered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    reordered["objects"].as_array_mut().unwrap().reverse();
    assert_eq!(
        MqObjectCatalog::decode(
            &serde_json::to_vec(&reordered).unwrap(),
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::CorruptSnapshot)
    );

    let mut broken_reference: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let objects = broken_reference["objects"].as_array_mut().unwrap();
    let queue = objects
        .iter_mut()
        .find(|object| object["kind"] == "local-queue" && object["name"] == "REQUEST.Q")
        .unwrap();
    queue["trigger_process"] = serde_json::json!("MISSING.PROCESS");
    assert_eq!(
        MqObjectCatalog::decode(
            &serde_json::to_vec(&broken_reference).unwrap(),
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::MissingReference)
    );

    let mut noncanonical: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    noncanonical["queue_manager"]["name"] = serde_json::json!("QM ");
    assert_eq!(
        MqObjectCatalog::decode(
            &serde_json::to_vec(&noncanonical).unwrap(),
            MqObjectLimits::default(),
        ),
        Err(MqObjectError::CorruptSnapshot)
    );

    let duplicate = [
        MqQueueDefinition {
            name: "DUP.Q".into(),
            trigger_program: None,
        },
        MqQueueDefinition {
            name: "DUP.Q".into(),
            trigger_program: None,
        },
    ];
    assert_eq!(
        MqObjectCatalog::from_queue_definitions(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            &duplicate,
            MqObjectLimits {
                max_objects: MqLimits::default().max_queues,
                ..MqObjectLimits::default()
            },
        ),
        Err(MqObjectError::DuplicateObject)
    );
}
