use super::*;
use crate::retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
};

fn host(environment: MqHostEnvironment, coordinator: MqSyncpointOwner) -> Invocation {
    let mut value = invocation();
    let bytes: &[u8] = match (environment, coordinator) {
        (MqHostEnvironment::ZosBatch, MqSyncpointOwner::QueueManager) => b"zos-batch|queue-manager",
        (MqHostEnvironment::ZosBatch, _) => b"zos-batch|host-coordinator",
        (MqHostEnvironment::ZosImsBatchDli, MqSyncpointOwner::QueueManager) => {
            b"zos-ims-batch-dli|queue-manager"
        }
        (MqHostEnvironment::ZosImsBatchDli, _) => b"zos-ims-batch-dli|host-coordinator",
        (MqHostEnvironment::ZosCics, _) => b"zos-cics|host-coordinator",
        (MqHostEnvironment::ZosIms, _) => b"zos-ims|host-coordinator",
        (MqHostEnvironment::MqiClient, MqSyncpointOwner::QueueManager) => {
            b"mqi-client|queue-manager"
        }
        (MqHostEnvironment::MqiClient, _) => b"mqi-client|host-coordinator",
        (_, MqSyncpointOwner::QueueManager) => b"other-bindings|queue-manager",
        (_, _) => b"other-bindings|host-coordinator",
    };
    bind(
        &mut value,
        "mq.host-context",
        "mainframe-env.mq.host-context@1",
        bytes,
    );
    value
}

fn nested(value: &mut Invocation, m: &mut Mutation, outer: &[u8]) {
    m.idempotency_key = IdempotencyKey::new(
        format!("cics:{}:{}", value.run_unit_id, m.sequence),
        InvocationLimits::default(),
    )
    .unwrap();
    bind(
        value,
        CICS_NESTED_EFFECT_ORIGIN_BINDING,
        CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
        m.idempotency_key.as_str().as_bytes(),
    );
    bind(
        value,
        CICS_OUTER_EFFECT_ORIGIN_BINDING,
        CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
        outer,
    );
}

fn cics() -> Invocation {
    let mut value = invocation();
    value.bindings.remove("mq.host-context");
    bind(
        &mut value,
        "cics.execution-context",
        "mainframe-env.cics.execution-context@1",
        b"local",
    );
    value
}

#[test]
fn source_exact_syncpoint_matrix_does_not_select_a_coordinator() {
    let environments = [
        MqHostEnvironment::ZosBatch,
        MqHostEnvironment::ZosImsBatchDli,
        MqHostEnvironment::ZosCics,
        MqHostEnvironment::ZosIms,
        MqHostEnvironment::MqiClient,
        MqHostEnvironment::OtherBindings,
    ];
    let protected = Cell::new(0);
    for environment in environments {
        for coordinator in [
            MqSyncpointOwner::QueueManager,
            MqSyncpointOwner::HostCoordinator,
        ] {
            if matches!(
                environment,
                MqHostEnvironment::ZosCics | MqHostEnvironment::ZosIms
            ) && coordinator == MqSyncpointOwner::QueueManager
            {
                continue;
            }
            for call in [MqMqiCall::Back, MqMqiCall::Begin, MqMqiCall::Commit] {
                let inv = host(environment, coordinator);
                let trusted_owner = MqHandleOwner {
                    environment,
                    ..owner()
                };
                let mut env = envelope();
                env.context.owner = trusted_owner;
                env.context.syncpoint_owner = coordinator;
                // A real token is unnecessary for a pre-state forbidden
                // disposition; an allowed intent still needs live registry checks.
                let connection = if environment == MqHostEnvironment::ZosCics {
                    MqHconn::Default
                } else {
                    let mut registry =
                        mainframe_env_host_api::MqHandleRegistry::new(44, 16).unwrap();
                    registry
                        .connect(trusted_owner, MqHandleSharing::NonShared)
                        .unwrap()
                };
                env.request = match call {
                    MqMqiCall::Back => MqMqiRequest::Back {
                        connection,
                        unit: 1,
                    },
                    MqMqiCall::Begin => MqMqiRequest::Begin {
                        connection,
                        unit: 1,
                        options: MqMqiOptions::ContractDefault,
                    },
                    _ => MqMqiRequest::Commit {
                        connection,
                        unit: 1,
                    },
                };
                let m = mutation();
                let e = effect(&inv, &m, &env);
                let p = provider();
                let scope = scope(&inv, trusted_owner, &e, &p).unwrap();
                let result = admit_mqi(&scope, &inv, 1).unwrap();
                let before = protected.get();
                if matches!(&result, MqMqiAdmission::ServiceValidation(_)) {
                    protected.set(before + 1);
                }
                // Independent explicit expectation, not the product matrix.
                let forbidden = coordinator == MqSyncpointOwner::HostCoordinator
                    || matches!(
                        environment,
                        MqHostEnvironment::ZosCics | MqHostEnvironment::ZosIms
                    )
                    || (call == MqMqiCall::Begin && environment == MqHostEnvironment::MqiClient);
                if forbidden {
                    assert!(
                        matches!(result, MqMqiAdmission::ForbiddenContext(MqMqiResult {
                        call: c, outcome: MqMqiOutcome::Completed {
                            status: MqMqiStatus::FailedEnvironment, output: MqMqiOutput::NoOutput,
                        },
                    }) if c == call)
                    );
                    assert_eq!(mainframe_env_host_api::MQCC_FAILED, 2);
                    assert_eq!(mainframe_env_host_api::MQRC_ENVIRONMENT_ERROR, 2012);
                } else if call == MqMqiCall::Begin {
                    assert!(matches!(
                        result,
                        MqMqiAdmission::Pending {
                            reason: MqMqiPending::ExternalUnitOfWork,
                            ..
                        }
                    ));
                } else {
                    assert!(matches!(result, MqMqiAdmission::ServiceValidation(_)));
                }
                // Spy for entering the consuming service's next protected
                // validation phase. Forbidden/pending results cannot enter it.
                assert_eq!(
                    protected.get() - before,
                    usize::from(!forbidden && call != MqMqiCall::Begin)
                );
            }
        }
    }
}

#[test]
fn cics_binding_is_sufficient_without_invented_application_fields() {
    for context in [
        b"local".as_slice(),
        b"dpl-synconreturn",
        b"dpl-without-synconreturn",
        b"dpl-executionset-subset",
    ] {
        let mut inv = cics();
        bind(
            &mut inv,
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            context,
        );
        let trusted_owner = MqHandleOwner {
            environment: MqHostEnvironment::ZosCics,
            ..owner()
        };
        let mut env = envelope();
        env.context.owner = trusted_owner;
        env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
        let mut m = mutation();
        nested(&mut inv, &mut m, b"outer-cics-effect");
        let e = effect(&inv, &m, &env);
        let p = provider();
        let scope = scope(&inv, trusted_owner, &e, &p).unwrap();
        let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap()
        else {
            panic!("valid nested intent")
        };
        assert_eq!(identity.origin, MqReplayOwnerKind::CicsNested);
        assert_eq!(
            identity.outer_effect_key.as_deref(),
            Some("outer-cics-effect")
        );
        assert_eq!(identity.owner, trusted_owner);
        assert_eq!(identity.principal(), inv.principal.id());
        assert_eq!(identity.effect().sequence, 7);
        assert_eq!(
            identity.effect().idempotency_key.as_ref(),
            Some(&m.idempotency_key)
        );
    }
}

#[test]
fn exact_nested_origin_never_authorizes_direct_application_syncpoint() {
    for request in [
        MqMqiRequest::Back {
            connection: MqHconn::Default,
            unit: 1,
        },
        MqMqiRequest::Commit {
            connection: MqHconn::Default,
            unit: 1,
        },
        MqMqiRequest::Begin {
            connection: MqHconn::Default,
            unit: 1,
            options: MqMqiOptions::ContractDefault,
        },
    ] {
        let mut inv = cics();
        let mut m = mutation();
        nested(&mut inv, &mut m, b"outer");
        let trusted_owner = MqHandleOwner {
            environment: MqHostEnvironment::ZosCics,
            ..owner()
        };
        let mut env = envelope();
        env.context.owner = trusted_owner;
        env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
        env.request = request;
        let e = effect(&inv, &m, &env);
        let p = provider();
        let scope = scope(&inv, trusted_owner, &e, &p).unwrap();
        assert!(matches!(
            admit_mqi(&scope, &inv, 1).unwrap(),
            MqMqiAdmission::ForbiddenContext(_)
        ));
    }
}

#[test]
fn malformed_or_forged_bindings_fail_even_in_a_service_scope() {
    for case in 0..16 {
        let mut inv = cics();
        let mut m = mutation();
        nested(&mut inv, &mut m, b"outer");
        match case {
            0 => {
                inv.bindings.remove(CICS_NESTED_EFFECT_ORIGIN_BINDING);
            }
            1 => {
                inv.bindings.remove(CICS_OUTER_EFFECT_ORIGIN_BINDING);
            }
            2 => bind(
                &mut inv,
                CICS_NESTED_EFFECT_ORIGIN_BINDING,
                "wrong@1",
                m.idempotency_key.as_str().as_bytes(),
            ),
            3 => bind(
                &mut inv,
                CICS_NESTED_EFFECT_ORIGIN_BINDING,
                CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                b"cics:run:8",
            ),
            4 => {
                m.idempotency_key =
                    IdempotencyKey::new("cics:foreign:7", InvocationLimits::default()).unwrap();
                bind(
                    &mut inv,
                    CICS_NESTED_EFFECT_ORIGIN_BINDING,
                    CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                    m.idempotency_key.as_str().as_bytes(),
                );
            }
            5 => {
                m.idempotency_key =
                    IdempotencyKey::new("cics:run:07", InvocationLimits::default()).unwrap();
                bind(
                    &mut inv,
                    CICS_NESTED_EFFECT_ORIGIN_BINDING,
                    CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                    m.idempotency_key.as_str().as_bytes(),
                );
            }
            6 => bind(
                &mut inv,
                CICS_OUTER_EFFECT_ORIGIN_BINDING,
                "wrong@1",
                b"outer",
            ),
            7 => bind(
                &mut inv,
                CICS_OUTER_EFFECT_ORIGIN_BINDING,
                CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                &[255],
            ),
            8 => bind(
                &mut inv,
                CICS_OUTER_EFFECT_ORIGIN_BINDING,
                CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                b"",
            ),
            9 => bind(&mut inv, "cics.execution-context", "wrong@1", b"local"),
            10 => bind(
                &mut inv,
                "cics.execution-context",
                "mainframe-env.cics.execution-context@1",
                b"invalid",
            ),
            11 => bind(
                &mut inv,
                "mq.host-context",
                "mainframe-env.mq.host-context@1",
                b"zos-batch|queue-manager",
            ),
            12 => bind(
                &mut inv,
                "mq.host-context",
                "mainframe-env.mq.host-context@1",
                b"zos-cics|queue-manager",
            ),
            13 => bind(
                &mut inv,
                "mq.host-context",
                "mainframe-env.mq.host-context@1",
                b"zos-cics|unknown",
            ),
            14 => bind(
                &mut inv,
                "mq.host-context",
                "wrong@1",
                b"zos-cics|host-coordinator",
            ),
            15 => {
                inv.bindings.remove("cics.execution-context");
                bind(
                    &mut inv,
                    "mq.host-context",
                    "mainframe-env.mq.host-context@1",
                    b"zos-cics|host-coordinator",
                );
            }
            _ => unreachable!(),
        }
        let trusted_owner = MqHandleOwner {
            environment: MqHostEnvironment::ZosCics,
            ..owner()
        };
        let mut env = envelope();
        env.context.owner = trusted_owner;
        env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
        let e = effect(&inv, &m, &env);
        let p = provider();
        let scope = scope(&inv, trusted_owner, &e, &p).unwrap();
        assert!(admit_mqi(&scope, &inv, 1).is_err(), "binding case {case}");
    }
}

#[test]
fn host_owned_uow_and_source_callback_restrictions_are_specific_pending_forms() {
    use mainframe_env_host_api::{MqGetContract, MqGetMode, MqTruncation, MqWait};
    for environment in [
        MqHostEnvironment::ZosCics,
        MqHostEnvironment::ZosIms,
        MqHostEnvironment::ZosImsBatchDli,
        MqHostEnvironment::OtherBindings,
    ] {
        let inv = host(environment, MqSyncpointOwner::HostCoordinator);
        let trusted_owner = MqHandleOwner {
            environment,
            ..owner()
        };
        let mut registry = mainframe_env_host_api::MqHandleRegistry::new(42, 16).unwrap();
        let connection = if environment == MqHostEnvironment::ZosCics {
            registry.bind_cics_default(trusted_owner).unwrap()
        } else {
            registry
                .connect(trusted_owner, MqHandleSharing::NonShared)
                .unwrap()
        };
        let object = registry.create_object(trusted_owner, connection).unwrap();
        let get = MqGetContract {
            selection: Default::default(),
            mode: MqGetMode::Remove,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: 64,
        };
        let requests = [
            (
                MqMqiRequest::Get(MqMqiGet {
                    connection,
                    object,
                    get,
                    message_handle: None,
                    options: MqMqiOptions::ContractDefault,
                    unit: MqMqiUnitOfWork::Local { unit: 1 },
                }),
                true,
            ),
            (
                MqMqiRequest::Control {
                    connection,
                    operation: MqMqiControl::Start,
                    options: MqMqiOptions::ContractDefault,
                },
                environment != MqHostEnvironment::OtherBindings,
            ),
            (
                MqMqiRequest::Callback {
                    connection,
                    object,
                    operation: MqMqiCallbackOperation::Suspend,
                    options: MqMqiOptions::ContractDefault,
                },
                matches!(
                    environment,
                    MqHostEnvironment::ZosIms | MqHostEnvironment::ZosImsBatchDli
                ),
            ),
        ];
        for (request, pending) in requests {
            let mut env = envelope();
            env.context.owner = trusted_owner;
            env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
            env.request = request;
            let m = mutation();
            let e = effect(&inv, &m, &env);
            let p = provider();
            let scope = scope(&inv, trusted_owner, &e, &p).unwrap();
            let result = admit_mqi(&scope, &inv, 1).unwrap();
            if pending {
                assert!(matches!(result, MqMqiAdmission::Pending { .. }));
            } else {
                assert!(matches!(result, MqMqiAdmission::ServiceValidation(_)));
            }
        }
        assert_eq!(registry.active_handles(), 2);
    }
}
