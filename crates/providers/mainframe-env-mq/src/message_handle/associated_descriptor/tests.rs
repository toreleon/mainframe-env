use super::*;
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
#[test]
fn reviewed_entries_and_legacy_kernel_frames_cannot_silently_convert_each_other() {
    let mut kernel = MqHandleKernel::new(1, 8, MqMessageLimits::default()).unwrap();
    let who = owner();
    let c = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
    let request = MqPropertyRequest::Create {
        connection: c,
        options: MqPropertyOptions::checked(MqMqiCall::CreateMessageHandle, 1, 0).unwrap(),
    };
    let stage = kernel
        .stage_property(who, &request, MqMqiLimits::default())
        .unwrap();
    let provisional = match &stage.output {
        MqMqiOutput::MessageHandle(v) => *v,
        _ => panic!(),
    };
    let h = stage.adopt().unwrap();
    assert!(provisional.is_historical());
    assert!(!h.is_historical());
    assert_eq!(
        mainframe_env_host_api::MqHandleObservation::from(MqHandle::Message(provisional)),
        mainframe_env_host_api::MqHandleObservation::from(MqHandle::Message(h))
    );
    let before = format!("{kernel:?}");
    for buffer in [vec![], b"MHK1\0\0\0\0\0\0".to_vec()] {
        assert_eq!(
            kernel.from_buffer(who, c, h.into(), &buffer, true, MqBufferCodec::KernelV1),
            Err(MqHandleKernelProblem::UnsupportedWire)
        );
    }
    assert_eq!(
        kernel.to_buffer(
            who,
            c,
            h.into(),
            &MqPropertyQuery::Prefix("".into()),
            128,
            MqBufferCodec::KernelV1
        ),
        Err(MqHandleKernelProblem::UnsupportedWire)
    );
    assert_eq!(format!("{kernel:?}"), before);
    let legacy = kernel
        .create(who, c, MqHandleKernelOption::Default)
        .unwrap();
    let inquire = MqPropertyRequest::Inquire {
        connection: c,
        handle: legacy,
        options: MqPropertyOptions::checked(MqMqiCall::InquireProperty, 1, 0).unwrap(),
        name: MqPropertyName::checked("a".into(), MqMessageLimits::default()).unwrap(),
        requested_type: 0,
        name_capacity: 32,
        value_capacity: 32,
    };
    assert!(matches!(
        kernel.stage_property(who, &inquire, MqMqiLimits::default()),
        Err(MqHandleKernelProblem::UnsupportedWire)
    ));
}
#[test]
fn associated_creation_abort_preserves_slots_generation_counter_and_every_existing_entry() {
    let mut kernel = MqHandleKernel::new(7, 8, MqMessageLimits::default()).unwrap();
    let who = owner();
    let c = kernel.connect(who, MqHandleSharing::NonShared).unwrap();
    let before = format!("{kernel:?}");
    let request = MqPropertyRequest::Create {
        connection: c,
        options: MqPropertyOptions::checked(MqMqiCall::CreateMessageHandle, 1, 0).unwrap(),
    };
    let stage = kernel
        .stage_property(who, &request, MqMqiLimits::default())
        .unwrap();
    let MqMqiOutput::MessageHandle(abandoned) = &stage.output else {
        panic!()
    };
    let abandoned = *abandoned;
    drop(stage);
    assert_eq!(format!("{kernel:?}"), before);
    let h = kernel
        .stage_property(who, &request, MqMqiLimits::default())
        .unwrap()
        .adopt()
        .unwrap();
    assert_eq!(
        mainframe_env_host_api::MqHandleObservation::from(MqHandle::Message(h)),
        mainframe_env_host_api::MqHandleObservation::from(MqHandle::Message(abandoned))
    );
    assert_eq!(
        kernel
            .registry
            .validate_message_property(who, c, abandoned.into()),
        Err(MqHandleProblem::Historical)
    );
}
