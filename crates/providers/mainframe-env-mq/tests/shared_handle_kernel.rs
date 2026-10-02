use mainframe_env_host_api::{
    MqHandleOwner, MqHandleProblem, MqHandleSharing, MqHconn, MqHostEnvironment, MqMessageProperty,
    MqPropertyQuery, MqPropertyType,
};
use mainframe_env_mq::{
    MqCallbackControl, MqCallbackState, MqHandleKernelOption, MqHandleKernelProblem,
    MqLocalQueueUsage, MqObjectCatalog, MqObjectDefinition, MqObjectLimits, MqObjectName,
    MqPubsubAuthorization, MqPubsubError, MqPubsubKernel, MqPubsubLimits, MqQueueManagerDefinition,
    MqSubscriptionDestination, MqSubscriptionMode,
};

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}
fn owner(task: u64) -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 1,
        thread_id: task,
        task_id: task,
        syncpoint_epoch: 1,
    }
}
fn kernel(slots: usize) -> MqPubsubKernel {
    let catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("QM"),
            default_transmission_queue: None,
        },
        vec![
            MqObjectDefinition::Topic { name: name("T") },
            MqObjectDefinition::LocalQueue {
                name: name("Q"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            },
            MqObjectDefinition::Subscription {
                name: name("D"),
                topic: name("T"),
                destination: MqSubscriptionDestination::Managed,
                durable: true,
            },
            MqObjectDefinition::Subscription {
                name: name("N"),
                topic: name("T"),
                destination: MqSubscriptionDestination::Queue(name("Q")),
                durable: false,
            },
        ],
        MqObjectLimits::default(),
    )
    .unwrap();
    MqPubsubKernel::new(catalog, MqPubsubLimits::default(), 1, slots).unwrap()
}
fn connect(kernel: &mut MqPubsubKernel, task: u64) -> MqHconn {
    kernel
        .message_handles_mut()
        .connect(owner(task), MqHandleSharing::NonShared)
        .unwrap()
}
fn subscribe(
    kernel: &mut MqPubsubKernel,
    connection: MqHconn,
    sub: &str,
) -> mainframe_env_mq::MqSubscriptionHandles {
    kernel
        .subscribe(
            owner(1),
            connection,
            &name(sub),
            MqSubscriptionMode::Create {
                publications_on_request: false,
            },
            MqPubsubAuthorization::Permit,
        )
        .unwrap()
}
fn property() -> MqMessageProperty {
    MqMessageProperty {
        name: "p".into(),
        kind: MqPropertyType::ByteString,
        value: vec![7],
    }
}

fn pending_callback(kernel: &mut MqPubsubKernel, connection: MqHconn) {
    let subscription = subscribe(kernel, connection, "D");
    kernel
        .register_callback(
            owner(1),
            connection,
            subscription.hobj,
            9,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    kernel
        .control(owner(1), connection, MqCallbackControl::Start)
        .unwrap();
    kernel
        .publish(
            &name("T"),
            mainframe_env_host_api::MqMessage {
                descriptor: mainframe_env_host_api::MqMessageDescriptor {
                    identifiers: mainframe_env_host_api::MqMessageIdentifiers::default(),
                    format: None,
                    expiry: mainframe_env_host_api::MqExpiry::Unlimited,
                    persistence: mainframe_env_host_api::MqPersistence::Persistent,
                    priority: mainframe_env_host_api::MqPriority::QueueDefault,
                    ordering: mainframe_env_host_api::MqMessageOrdering::default(),
                },
                body: vec![1],
                properties: Vec::new(),
            },
            false,
            None,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
}

#[test]
fn both_access_guards_reconcile_direct_retirement_before_dispatch() {
    for through_registry in [false, true] {
        let mut kernel = kernel(8);
        let connection = connect(&mut kernel, 1);
        pending_callback(&mut kernel, connection);
        if through_registry {
            kernel
                .handles_mut()
                .disconnect(owner(1), connection)
                .unwrap();
        } else {
            kernel
                .message_handles_mut()
                .disconnect(owner(1), connection)
                .unwrap();
        }
        assert_eq!(kernel.next_event(), Ok(None));
        assert_eq!(kernel.callback_state(connection), MqCallbackState::Stopped);
        assert_eq!(kernel.handles_mut().active_handles(), 0);
        let replacement = connect(&mut kernel, 1);
        kernel
            .subscribe(
                owner(1),
                replacement,
                &name("D"),
                MqSubscriptionMode::Resume,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
    }
}

#[test]
fn forgotten_guard_cannot_dispatch_retired_callback() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    pending_callback(&mut kernel, connection);
    let mut access = kernel.handles_mut();
    access.disconnect(owner(1), connection).unwrap();
    std::mem::forget(access);
    assert_eq!(kernel.next_event(), Ok(None));
    assert_eq!(kernel.callback_state(connection), MqCallbackState::Stopped);
}

#[test]
fn raw_disconnect_preserves_unassociated_properties_and_failed_in_use_state() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let unassociated = kernel
        .message_handles_mut()
        .create(
            owner(1),
            MqHconn::Unassociated,
            MqHandleKernelOption::Default,
        )
        .unwrap();
    kernel
        .message_handles_mut()
        .set(
            owner(1),
            MqHconn::Unassociated,
            unassociated.into(),
            property(),
            MqHandleKernelOption::Default,
        )
        .unwrap();
    let associated = kernel
        .message_handles_mut()
        .create(owner(1), connection, MqHandleKernelOption::Default)
        .unwrap();
    kernel
        .message_handles_mut()
        .begin_io(owner(1), connection, associated)
        .unwrap();
    assert_eq!(
        kernel.handles_mut().disconnect(owner(1), connection),
        Err(MqHandleProblem::InUse)
    );
    kernel
        .message_handles_mut()
        .end_io(owner(1), connection, associated)
        .unwrap();
    kernel
        .handles_mut()
        .disconnect(owner(1), connection)
        .unwrap();
    connect(&mut kernel, 1);
    assert_eq!(
        kernel.message_handles_mut().inquire(
            owner(1),
            MqHconn::Unassociated,
            unassociated.into(),
            &MqPropertyQuery::Exact("p".into()),
            None,
            1,
            MqHandleKernelOption::Default
        ),
        Ok(property())
    );
    kernel.handles_mut().end_processing_unit(owner(1)).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 0);
}

#[test]
fn one_connection_supports_properties_subscription_and_callback() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let message = kernel
        .message_handles_mut()
        .create(owner(1), connection, MqHandleKernelOption::Default)
        .unwrap();
    kernel
        .message_handles_mut()
        .set(
            owner(1),
            connection,
            message.into(),
            property(),
            MqHandleKernelOption::Default,
        )
        .unwrap();
    let subscription = subscribe(&mut kernel, connection, "D");
    kernel
        .register_callback(
            owner(1),
            connection,
            subscription.hobj,
            9,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    assert_eq!(
        kernel.control(owner(1), connection, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
    assert_eq!(kernel.handles_mut().active_handles(), 4);
    assert_eq!(
        kernel.message_handles_mut().inquire(
            owner(1),
            connection,
            message.into(),
            &MqPropertyQuery::Exact("p".into()),
            None,
            1,
            MqHandleKernelOption::Default
        ),
        Ok(property())
    );
}

#[test]
fn shared_slot_exhaustion_cannot_publish_partial_subscription() {
    let mut kernel = kernel(3);
    let connection = connect(&mut kernel, 1);
    kernel
        .message_handles_mut()
        .create(owner(1), connection, MqHandleKernelOption::Default)
        .unwrap();
    let before = kernel.snapshot().unwrap();
    assert_eq!(
        kernel.subscribe(
            owner(1),
            connection,
            &name("D"),
            MqSubscriptionMode::Create {
                publications_on_request: false
            },
            MqPubsubAuthorization::Permit
        ),
        Err(MqPubsubError::Handle(MqHandleProblem::Capacity))
    );
    assert_eq!(kernel.handles_mut().active_handles(), 2);
    assert_eq!(kernel.snapshot().unwrap(), before);
}

#[test]
fn disconnect_retires_all_associated_families_and_allows_nondurable_recreate() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let message = kernel
        .message_handles_mut()
        .create(owner(1), connection, MqHandleKernelOption::Default)
        .unwrap();
    let subscription = subscribe(&mut kernel, connection, "N");
    kernel
        .register_callback(
            owner(1),
            connection,
            subscription.hobj,
            9,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    kernel
        .control(owner(1), connection, MqCallbackControl::Start)
        .unwrap();
    kernel.disconnect(owner(1), connection).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 0);
    assert_eq!(kernel.callback_state(connection), MqCallbackState::Stopped);
    assert_eq!(
        kernel.close_subscription(owner(1), connection, subscription.hsub),
        Err(MqPubsubError::Handle(MqHandleProblem::Stale))
    );
    assert_eq!(
        kernel.message_handles_mut().set(
            owner(1),
            connection,
            message.into(),
            property(),
            MqHandleKernelOption::Default
        ),
        Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
    );
    let replacement = connect(&mut kernel, 1);
    subscribe(&mut kernel, replacement, "N");
}

#[test]
fn unit_retirement_preserves_other_owner_and_durable_subscription() {
    let mut kernel = kernel(12);
    let first = connect(&mut kernel, 1);
    let second = connect(&mut kernel, 2);
    subscribe(&mut kernel, first, "D");
    let message = kernel
        .message_handles_mut()
        .create(owner(2), second, MqHandleKernelOption::Default)
        .unwrap();
    kernel.end_processing_unit(owner(1)).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 2);
    kernel
        .message_handles_mut()
        .set(
            owner(2),
            second,
            message.into(),
            property(),
            MqHandleKernelOption::Default,
        )
        .unwrap();
    let first = connect(&mut kernel, 1);
    kernel
        .subscribe(
            owner(1),
            first,
            &name("D"),
            MqSubscriptionMode::Resume,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
}

#[test]
fn epoch_retirement_is_atomic_and_preserves_durable_snapshot() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let message = kernel
        .message_handles_mut()
        .create(owner(1), connection, MqHandleKernelOption::Default)
        .unwrap();
    subscribe(&mut kernel, connection, "D");
    let snapshot = kernel.snapshot().unwrap();
    assert_eq!(
        kernel.advance_handle_epoch(1),
        Err(MqPubsubError::Handle(MqHandleProblem::EpochNotAdvanced))
    );
    assert_eq!(kernel.handles_mut().active_handles(), 4);
    kernel.advance_handle_epoch(2).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 0);
    assert_eq!(kernel.snapshot().unwrap(), snapshot);
    let current = connect(&mut kernel, 1);
    assert_eq!(
        kernel.message_handles_mut().set(
            owner(1),
            current,
            message.into(),
            property(),
            MqHandleKernelOption::Default
        ),
        Err(MqHandleKernelProblem::Handle(MqHandleProblem::Stale))
    );
}

#[test]
fn foreign_registry_or_owner_rejection_preserves_all_state() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let mut foreign = self::kernel(8);
    let foreign_connection = connect(&mut foreign, 1);
    let before = kernel.snapshot().unwrap();
    assert_eq!(
        kernel.subscribe(
            owner(1),
            foreign_connection,
            &name("D"),
            MqSubscriptionMode::Create {
                publications_on_request: false
            },
            MqPubsubAuthorization::Permit
        ),
        Err(MqPubsubError::Handle(MqHandleProblem::Stale))
    );
    assert_eq!(
        kernel.disconnect(owner(2), connection),
        Err(MqPubsubError::Handle(MqHandleProblem::CrossOwner))
    );
    assert_eq!(kernel.snapshot().unwrap(), before);
    assert_eq!(kernel.handles_mut().active_handles(), 1);
}

#[test]
fn unassociated_properties_survive_connection_disconnect_and_reconnect() {
    let mut kernel = kernel(8);
    let connection = connect(&mut kernel, 1);
    let message = kernel
        .message_handles_mut()
        .create(
            owner(1),
            MqHconn::Unassociated,
            MqHandleKernelOption::Default,
        )
        .unwrap();
    kernel
        .message_handles_mut()
        .set(
            owner(1),
            MqHconn::Unassociated,
            message.into(),
            property(),
            MqHandleKernelOption::Default,
        )
        .unwrap();
    kernel.disconnect(owner(1), connection).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 1);
    let replacement = connect(&mut kernel, 1);
    assert_eq!(
        kernel.message_handles_mut().inquire(
            owner(1),
            MqHconn::Unassociated,
            message.into(),
            &MqPropertyQuery::Exact("p".into()),
            None,
            1,
            MqHandleKernelOption::Default
        ),
        Ok(property())
    );
    kernel.disconnect(owner(1), replacement).unwrap();
    kernel.end_processing_unit(owner(1)).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 0);
}

#[test]
fn cics_default_disconnect_is_a_noop_across_both_families() {
    let mut kernel = kernel(8);
    let cics = MqHandleOwner {
        environment: MqHostEnvironment::ZosCics,
        ..owner(1)
    };
    let connection = kernel
        .message_handles_mut()
        .bind_cics_default(cics)
        .unwrap();
    let message = kernel
        .message_handles_mut()
        .create(cics, connection, MqHandleKernelOption::Default)
        .unwrap();
    let subscription = kernel
        .subscribe(
            cics,
            connection,
            &name("D"),
            MqSubscriptionMode::Create {
                publications_on_request: false,
            },
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    kernel
        .register_callback(
            cics,
            connection,
            subscription.hobj,
            9,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    kernel
        .control(cics, connection, MqCallbackControl::Start)
        .unwrap();
    kernel.disconnect(cics, connection).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 4);
    assert_eq!(kernel.callback_state(connection), MqCallbackState::Started);
    kernel
        .message_handles_mut()
        .set(
            cics,
            connection,
            message.into(),
            property(),
            MqHandleKernelOption::Default,
        )
        .unwrap();
    kernel.end_processing_unit(cics).unwrap();
    assert_eq!(kernel.handles_mut().active_handles(), 0);
    assert_eq!(kernel.callback_state(connection), MqCallbackState::Stopped);
}
