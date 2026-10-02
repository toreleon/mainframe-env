//! Private-kernel isolation only; these tests do not admit CICS MQOP_START.

use mainframe_env_host_api::{
    MqExpiry, MqHandleOwner, MqHandleProblem, MqHandleSharing, MqHconn, MqHobj, MqHostEnvironment,
    MqMessage, MqMessageDescriptor, MqMessageIdentifiers, MqMessageOrdering, MqPersistence,
    MqPriority,
};
use mainframe_env_mq::{
    MqCallbackControl, MqCallbackState, MqObjectCatalog, MqObjectDefinition, MqObjectLimits,
    MqObjectName, MqPubsubAuthorization, MqPubsubError, MqPubsubEvent, MqPubsubKernel,
    MqPubsubLimits, MqQueueManagerDefinition, MqSubscriptionDestination, MqSubscriptionMode,
};

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}

fn owner(task: u64) -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosCics,
        host_id: 1,
        process_id: 1,
        thread_id: task,
        task_id: task,
        syncpoint_epoch: 1,
    }
}

fn kernel() -> MqPubsubKernel {
    kernel_with_limits(MqPubsubLimits::default())
}

fn kernel_with_limits(limits: MqPubsubLimits) -> MqPubsubKernel {
    let catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("QM"),
            default_transmission_queue: None,
        },
        ["A", "B"]
            .into_iter()
            .flat_map(|suffix| {
                [
                    MqObjectDefinition::Topic {
                        name: name(&format!("T.{suffix}")),
                    },
                    MqObjectDefinition::Subscription {
                        name: name(&format!("S.{suffix}")),
                        topic: name(&format!("T.{suffix}")),
                        destination: MqSubscriptionDestination::Managed,
                        durable: true,
                    },
                ]
            })
            .collect(),
        MqObjectLimits::default(),
    )
    .unwrap();
    MqPubsubKernel::new(catalog, limits, 1, 16).unwrap()
}

fn bind(kernel: &mut MqPubsubKernel, owner: MqHandleOwner) -> MqHconn {
    kernel
        .message_handles_mut()
        .bind_cics_default(owner)
        .unwrap()
}

fn callback(
    kernel: &mut MqPubsubKernel,
    owner: MqHandleOwner,
    hconn: MqHconn,
    suffix: &str,
) -> MqHobj {
    let handles = kernel
        .subscribe(
            owner,
            hconn,
            &name(&format!("S.{suffix}")),
            MqSubscriptionMode::Create {
                publications_on_request: false,
            },
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    kernel
        .register_callback(
            owner,
            hconn,
            handles.hobj,
            if suffix == "A" { 1 } else { 2 },
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
    handles.hobj
}

fn publish(kernel: &mut MqPubsubKernel, suffix: &str) {
    kernel
        .publish(
            &name(&format!("T.{suffix}")),
            MqMessage {
                descriptor: MqMessageDescriptor {
                    identifiers: MqMessageIdentifiers::default(),
                    format: None,
                    expiry: MqExpiry::Unlimited,
                    persistence: MqPersistence::Persistent,
                    priority: MqPriority::QueueDefault,
                    ordering: MqMessageOrdering::default(),
                },
                body: vec![7],
                properties: vec![],
            },
            false,
            None,
            MqPubsubAuthorization::Permit,
        )
        .unwrap();
}

#[test]
fn default_tasks_start_stop_and_suspend_independently() {
    let mut kernel = kernel();
    for (task, suffix) in [(1, "A"), (2, "B")] {
        let hconn = bind(&mut kernel, owner(task));
        callback(&mut kernel, owner(task), hconn, suffix);
    }
    assert_eq!(
        kernel.control(owner(1), MqHconn::Default, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
    assert_eq!(
        kernel.control(owner(2), MqHconn::Default, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
    kernel
        .control(owner(1), MqHconn::Default, MqCallbackControl::Suspend)
        .unwrap();
    assert_eq!(
        kernel.callback_state(owner(2), MqHconn::Default),
        Ok(MqCallbackState::Started)
    );
    // B is still Started, so it can suspend independently of A.
    assert_eq!(
        kernel.control(owner(2), MqHconn::Default, MqCallbackControl::Suspend),
        Ok(MqCallbackState::Suspended)
    );
    kernel
        .control(owner(1), MqHconn::Default, MqCallbackControl::Stop)
        .unwrap();
    assert_eq!(
        kernel.callback_state(owner(1), MqHconn::Default),
        Ok(MqCallbackState::Stopped)
    );
    assert_eq!(
        kernel.callback_state(owner(2), MqHconn::Default),
        Ok(MqCallbackState::Suspended)
    );
    assert_eq!(
        kernel.control(owner(2), MqHconn::Default, MqCallbackControl::Resume),
        Ok(MqCallbackState::Started)
    );
}

#[test]
fn default_task_without_callback_cannot_borrow_another_tasks_callback() {
    let mut kernel = kernel();
    let hconn = bind(&mut kernel, owner(1));
    bind(&mut kernel, owner(2));
    callback(&mut kernel, owner(1), hconn, "A");
    assert_eq!(
        kernel.control(owner(2), hconn, MqCallbackControl::Start),
        Err(MqPubsubError::NoCallbacksActive)
    );
    assert_eq!(
        kernel.callback_state(owner(1), hconn),
        Ok(MqCallbackState::Stopped)
    );
    assert_eq!(
        kernel.callback_state(owner(2), hconn),
        Ok(MqCallbackState::Stopped)
    );
    assert_eq!(
        kernel.control(owner(1), hconn, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
}

#[test]
fn starting_one_default_task_does_not_dispatch_another_tasks_publication() {
    let mut kernel = kernel();
    for (task, suffix) in [(1, "A"), (2, "B")] {
        let hconn = bind(&mut kernel, owner(task));
        callback(&mut kernel, owner(task), hconn, suffix);
    }
    kernel
        .control(owner(1), MqHconn::Default, MqCallbackControl::Start)
        .unwrap();
    publish(&mut kernel, "B");
    assert_eq!(kernel.next_event().unwrap(), None);
    publish(&mut kernel, "A");
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 1, .. })
    ));
    assert_eq!(kernel.next_event().unwrap(), None);
    kernel
        .control(owner(2), MqHconn::Default, MqCallbackControl::Start)
        .unwrap();
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 2, .. })
    ));
    // Suspending B leaves A ready; stopping A leaves B ready after resume.
    kernel
        .control(owner(2), MqHconn::Default, MqCallbackControl::Suspend)
        .unwrap();
    publish(&mut kernel, "B");
    publish(&mut kernel, "A");
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 1, .. })
    ));
    assert_eq!(kernel.next_event().unwrap(), None);
    kernel
        .control(owner(1), MqHconn::Default, MqCallbackControl::Stop)
        .unwrap();
    kernel
        .control(owner(2), MqHconn::Default, MqCallbackControl::Resume)
        .unwrap();
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 2, .. })
    ));
    assert_eq!(kernel.next_event().unwrap(), None);
}

#[test]
fn default_control_uses_registry_task_scope_instead_of_full_owner_equality() {
    let mut kernel = kernel();
    let first = owner(1);
    let same_task = MqHandleOwner {
        thread_id: 9,
        syncpoint_epoch: 2,
        ..first
    };
    let hconn = bind(&mut kernel, first);
    callback(&mut kernel, first, hconn, "A");
    kernel
        .control(first, hconn, MqCallbackControl::Start)
        .unwrap();
    assert_eq!(
        kernel.callback_state(same_task, hconn),
        Ok(MqCallbackState::Started)
    );
    assert_eq!(
        kernel.control(same_task, hconn, MqCallbackControl::Suspend),
        Ok(MqCallbackState::Suspended)
    );
    assert_eq!(
        kernel.callback_state(first, hconn),
        Ok(MqCallbackState::Suspended)
    );
    kernel
        .control(same_task, hconn, MqCallbackControl::Stop)
        .unwrap();
    // A registered Hobj owned by the same task remains eligible across threads.
    assert_eq!(
        kernel.control(same_task, hconn, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
}

#[test]
fn default_task_identity_includes_host_and_process() {
    for second in [
        MqHandleOwner {
            host_id: 2,
            ..owner(1)
        },
        MqHandleOwner {
            process_id: 2,
            ..owner(1)
        },
    ] {
        let mut kernel = kernel();
        let hconn = bind(&mut kernel, owner(1));
        bind(&mut kernel, second);
        callback(&mut kernel, owner(1), hconn, "A");
        kernel
            .control(owner(1), hconn, MqCallbackControl::Start)
            .unwrap();
        assert_eq!(
            kernel.callback_state(second, hconn),
            Ok(MqCallbackState::Stopped)
        );
        assert_eq!(
            kernel.control(second, hconn, MqCallbackControl::Start),
            Err(MqPubsubError::NoCallbacksActive)
        );
        assert_eq!(
            kernel.callback_state(owner(1), hconn),
            Ok(MqCallbackState::Started)
        );
    }
}

#[test]
fn issued_shared_connection_retains_one_control_across_permitted_threads() {
    let first = MqHandleOwner {
        environment: MqHostEnvironment::MqiClient,
        ..owner(1)
    };
    let second = MqHandleOwner {
        thread_id: 2,
        task_id: 2,
        ..first
    };
    for sharing in [MqHandleSharing::SharedBlock, MqHandleSharing::SharedNoBlock] {
        let mut kernel = kernel();
        let hconn = kernel
            .message_handles_mut()
            .connect(first, sharing)
            .unwrap();
        callback(&mut kernel, first, hconn, "A");
        // A's registered callback is valid for B on this issued shared connection.
        assert_eq!(
            kernel.control(second, hconn, MqCallbackControl::Start),
            Ok(MqCallbackState::Started)
        );
        assert_eq!(
            kernel.callback_state(first, hconn),
            Ok(MqCallbackState::Started)
        );
        assert_eq!(
            kernel.control(first, hconn, MqCallbackControl::Start),
            Err(MqPubsubError::InvalidState)
        );
        publish(&mut kernel, "A");
        assert!(matches!(
            kernel.next_event().unwrap(),
            Some(MqPubsubEvent::Publication { callback_id: 1, .. })
        ));
        kernel
            .control(first, hconn, MqCallbackControl::Suspend)
            .unwrap();
        assert_eq!(
            kernel.callback_state(second, hconn),
            Ok(MqCallbackState::Suspended)
        );
        kernel
            .control(second, hconn, MqCallbackControl::Resume)
            .unwrap();
        assert_eq!(
            kernel.callback_state(first, hconn),
            Ok(MqCallbackState::Started)
        );
        kernel
            .control(second, hconn, MqCallbackControl::Stop)
            .unwrap();
        assert_eq!(
            kernel.callback_state(first, hconn),
            Ok(MqCallbackState::Stopped)
        );
    }
}

#[test]
fn invalid_controls_and_observations_leave_valid_task_state_unchanged() {
    let mut kernel = kernel();
    let hconn = bind(&mut kernel, owner(1));
    bind(&mut kernel, owner(2));
    callback(&mut kernel, owner(1), hconn, "A");
    kernel
        .control(owner(1), hconn, MqCallbackControl::Start)
        .unwrap();
    publish(&mut kernel, "A");
    let before = kernel.snapshot().unwrap();
    for operation in [
        MqCallbackControl::Suspend,
        MqCallbackControl::Resume,
        MqCallbackControl::Quiesce,
    ] {
        assert_eq!(
            kernel.control(owner(2), hconn, operation),
            Err(MqPubsubError::InvalidState)
        );
    }
    assert_eq!(
        kernel.control(owner(3), hconn, MqCallbackControl::Stop),
        Err(MqPubsubError::Handle(MqHandleProblem::MissingConnection))
    );
    assert_eq!(
        kernel.callback_state(owner(3), hconn),
        Err(MqPubsubError::Handle(MqHandleProblem::MissingConnection))
    );
    let invalid_owner = MqHandleOwner {
        task_id: 0,
        ..owner(1)
    };
    assert_eq!(
        kernel.control(invalid_owner, hconn, MqCallbackControl::Stop),
        Err(MqPubsubError::Handle(MqHandleProblem::InvalidOwner))
    );
    assert_eq!(kernel.snapshot().unwrap(), before);
    assert_eq!(
        kernel.callback_state(owner(1), hconn),
        Ok(MqCallbackState::Started)
    );
    assert_eq!(
        kernel.callback_state(owner(2), hconn),
        Ok(MqCallbackState::Stopped)
    );
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 1, .. })
    ));
}

#[test]
fn foreign_owner_of_issued_nonshared_connection_cannot_change_or_observe_control() {
    let first = MqHandleOwner {
        environment: MqHostEnvironment::MqiClient,
        ..owner(1)
    };
    let second = MqHandleOwner {
        thread_id: 2,
        ..first
    };
    let mut kernel = kernel();
    let hconn = kernel
        .message_handles_mut()
        .connect(first, MqHandleSharing::NonShared)
        .unwrap();
    callback(&mut kernel, first, hconn, "A");
    kernel
        .control(first, hconn, MqCallbackControl::Start)
        .unwrap();
    assert_eq!(
        kernel.control(second, hconn, MqCallbackControl::Stop),
        Err(MqPubsubError::Handle(MqHandleProblem::CrossOwner))
    );
    assert_eq!(
        kernel.callback_state(second, hconn),
        Err(MqPubsubError::Handle(MqHandleProblem::CrossOwner))
    );
    assert_eq!(
        kernel.callback_state(first, hconn),
        Ok(MqCallbackState::Started)
    );
    kernel.disconnect(first, hconn).unwrap();
    assert_eq!(
        kernel.control(first, hconn, MqCallbackControl::Start),
        Err(MqPubsubError::Handle(MqHandleProblem::Stale))
    );
    assert_eq!(
        kernel.callback_state(first, hconn),
        Err(MqPubsubError::Handle(MqHandleProblem::Stale))
    );
}

#[test]
fn task_retirement_preserves_surviving_default_control_and_dispatch() {
    let mut kernel = kernel();
    for (task, suffix) in [(1, "A"), (2, "B")] {
        let hconn = bind(&mut kernel, owner(task));
        callback(&mut kernel, owner(task), hconn, suffix);
        kernel
            .control(owner(task), hconn, MqCallbackControl::Start)
            .unwrap();
    }
    kernel.end_processing_unit(owner(1)).unwrap();
    assert_eq!(
        kernel.callback_state(owner(1), MqHconn::Default),
        Err(MqPubsubError::Handle(MqHandleProblem::MissingConnection))
    );
    assert_eq!(
        kernel.callback_state(owner(2), MqHconn::Default),
        Ok(MqCallbackState::Started)
    );
    publish(&mut kernel, "A");
    publish(&mut kernel, "B");
    assert!(matches!(
        kernel.next_event().unwrap(),
        Some(MqPubsubEvent::Publication { callback_id: 2, .. })
    ));
    assert_eq!(kernel.next_event().unwrap(), None);
    // Rebinding A's task cannot revive its retired control/callback.
    let hconn = bind(&mut kernel, owner(1));
    assert_eq!(
        kernel.callback_state(owner(1), hconn),
        Ok(MqCallbackState::Stopped)
    );
    assert_eq!(
        kernel.control(owner(1), hconn, MqCallbackControl::Start),
        Err(MqPubsubError::NoCallbacksActive)
    );
}

#[test]
fn default_control_capacity_rejection_is_atomic_and_stop_releases_capacity() {
    let mut kernel = kernel_with_limits(MqPubsubLimits {
        max_callbacks: 1,
        ..MqPubsubLimits::default()
    });
    let hconn = bind(&mut kernel, owner(1));
    bind(&mut kernel, owner(2));
    let hobj = callback(&mut kernel, owner(1), hconn, "A");
    kernel
        .control(owner(1), hconn, MqCallbackControl::Start)
        .unwrap();
    kernel.deregister_callback(owner(1), hconn, hobj).unwrap();
    callback(&mut kernel, owner(2), hconn, "B");
    let before = kernel.snapshot().unwrap();
    assert_eq!(
        kernel.control(owner(2), hconn, MqCallbackControl::Start),
        Err(MqPubsubError::ResourceExhausted)
    );
    assert_eq!(kernel.snapshot().unwrap(), before);
    assert_eq!(
        kernel.callback_state(owner(1), hconn),
        Ok(MqCallbackState::Started)
    );
    assert_eq!(
        kernel.callback_state(owner(2), hconn),
        Ok(MqCallbackState::Stopped)
    );
    kernel
        .control(owner(1), hconn, MqCallbackControl::Stop)
        .unwrap();
    assert_eq!(
        kernel.control(owner(2), hconn, MqCallbackControl::Start),
        Ok(MqCallbackState::Started)
    );
}
