use super::*;

#[test]
fn rfh2_original_connx_source_is_once_replay_and_warning_reuse_do_not_resample() {
    for extended in [false, true] {
        let mut f = Fixture::new(false);
        let source = configure(&mut f);
        let connect = MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        };
        let e = f.effect(
            1,
            if extended {
                MqMqiRequest::ConnectExtended(connect)
            } else {
                MqMqiRequest::Connect(connect)
            },
        );
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        let MqMqiOutput::Connected(c) = output(reply.clone()) else {
            panic!()
        };
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.execute(&e).unwrap(), reply);
        let h = hmsg(f.call(2, create(c)));
        source.mode.store(1, Ordering::SeqCst);
        let reuse = f.call(
            3,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        assert_eq!(pair(&reuse), (MqCompletion::Warning, 2002));
        assert_eq!(output(reuse), MqMqiOutput::Connected(c));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        let r = f.call(4, import(c, h, md(), vec![]));
        assert_eq!(pair(&r), (MqCompletion::Ok, 0));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn rfh2_source_range_refusal_error_panic_and_callback_reentry_never_allocate_or_publish_partial_state()
 {
    for mode in 1..4 {
        let mut f = Fixture::new(false);
        let source = configure(&mut f);
        source.mode.store(mode, Ordering::SeqCst);
        let e = f.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        f.seed(&e);
        let rows = f.rows();
        let state = snapshot(&f);
        assert_eq!(
            f.execute(&e),
            Err(if mode == 1 {
                HostProblem::Unsupported
            } else {
                HostProblem::InfrastructureFailure
            })
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(snapshot(&f), state);
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert!(
            f.store
                .get_provider_state(receipt::NAMESPACE, "effect-1")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            f.store
                .audit_records(&f.inv.execution_id, 0, 128)
                .unwrap()
                .len(),
            if mode == 1 { 0 } else { 1 }
        );
    }
    let mut f = Fixture::new(false);
    let source = configure(&mut f);
    *source.reentry.lock().unwrap() = Some(Arc::downgrade(&f.service));
    let c = f.connect();
    assert!(matches!(c, MqHconn::Issued(_)));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn rfh2_privileged_frozen_runtime_port_accepts_actual_original_dispatch_not_binding_attestation() {
    let f = Fixture::new(false);
    let source = Arc::new(Codeset::default());
    let runtime = crate::MqTrustedBatchRuntime::open_with_rfh2_codeset_source(
        f.store.clone(),
        MqLimits::default(),
        3,
        5,
        f.saf.clone(),
        f.clock.clone(),
        f.provider.clone(),
        HostLimits::default(),
        MqMqiLimits::default(),
        source.clone(),
    )
    .unwrap();
    let mut inv = f.inv.clone();
    inv.bindings.clear();
    let root = runtime.admit_root(inv).unwrap();
    let mut frame = root.frame();
    let mut effect = f.effect(
        1,
        MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        }),
    );
    let HostRequest::MqMqi(v) = &mut effect.request else {
        panic!()
    };
    v.envelope.context = frame.context().unwrap();
    f.seed(&effect);
    let reply = frame
        .dispatch(
            effect
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        output(reply.clone()),
        MqMqiOutput::Connected(MqHconn::Issued(_))
    ));
    assert_eq!(
        frame
            .dispatch(
                effect
                    .mq_mqi_occurrence(HostLimits::default())
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
        reply
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}
