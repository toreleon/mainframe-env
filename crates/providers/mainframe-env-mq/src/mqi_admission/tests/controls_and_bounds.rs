use super::*;
use mainframe_env_host_api::{MqHandleRegistry, mq_object_route::MqRouteName};

#[test]
fn admitted_identity_is_borrowed_exact_and_deterministic_on_retry() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    assert_eq!(env.review(), Ok(MqMqiPending::PublicDispatch));
    for _ in 0..2 {
        let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1,
        )
        .unwrap() else {
            panic!("boundary")
        };
        assert!(std::ptr::eq(identity.envelope, &env));
        assert!(std::ptr::eq(identity.mutation, &m));
        assert!(std::ptr::eq(identity.invocation(), &inv));
        assert_eq!(identity.owner, owner());
        assert_eq!(identity.observed_tick, 1);
        assert_eq!(identity.request_bytes, mq_mqi_request_size(&env).unwrap());
        assert_eq!(
            identity.request_digest,
            mq_mqi_request_digest(&env).unwrap()
        );
        assert_eq!(identity.effect().run_unit(), &inv.run_unit_id);
        assert_eq!(identity.effect().deadline_tick(), e.deadline_tick);
        assert_eq!(identity.effect().sequence(), m.sequence);
        assert_eq!(
            identity.effect().idempotency_key(),
            Some(&m.idempotency_key)
        );
        assert_eq!(identity.origin, MqReplayOwnerKind::CoreEffect);
        assert_eq!(identity.outer_effect_key, None);
        assert_eq!(identity.capability.as_str(), "host.mq.write");
    }
}

#[test]
fn cancellation_after_admission_and_probe_replacement_cannot_freeze_controls() {
    let inv = invocation().with_cancellation_probe(CancellationProbe::new());
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(
        &scope,
        &inv,
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        1,
    )
    .unwrap() else {
        panic!("boundary")
    };
    assert_eq!(identity.recheck_controls(89), Ok(()));
    assert_eq!(identity.recheck_controls(0), Err(HostProblem::Malformed));
    assert_eq!(identity.recheck_controls(90), Err(HostProblem::TimedOut));
    for probe in [None, Some(CancellationProbe::new())] {
        let mut candidate = inv.clone();
        candidate.cancellation_probe = probe;
        assert_eq!(
            admit_mqi(
                &scope,
                &candidate,
                &env,
                &m,
                MqMqiEffectOccurrence::from_effect(&e),
                1
            )
            .unwrap_err(),
            HostProblem::IdempotencyConflict
        );
    }
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(identity.recheck_controls(2), Err(HostProblem::Cancelled));
    assert_eq!(
        admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            2
        )
        .unwrap_err(),
        HostProblem::Cancelled
    );
}

#[test]
fn invocation_identity_owner_and_self_consistent_forged_occurrences_are_rejected() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    let state_calls = Cell::new(0);
    let saf_calls = Cell::new(0);
    let provider_calls = Cell::new(0);
    for case in 0..14 {
        let mut candidate = inv.clone();
        let mut changed_env = env.clone();
        let mut changed_m = m.clone();
        let mut changed_e = e.clone();
        match case {
            0 => {
                candidate.principal = Principal::new(
                    PrincipalId::new("FOREIGN", InvocationLimits::default()).unwrap(),
                    inv.principal.grants().clone(),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
            1 => {
                candidate.idempotency_key =
                    IdempotencyKey::new("forged-invocation", InvocationLimits::default()).unwrap()
            }
            2 => candidate.attempt += 1,
            3 => candidate.limits.max_effects += 1,
            4 => {
                changed_m.sequence += 1;
                changed_e.sequence = changed_m.sequence;
            }
            5 => {
                changed_m.idempotency_key =
                    IdempotencyKey::new("forged-effect", InvocationLimits::default()).unwrap();
                changed_e.idempotency_key = Some(changed_m.idempotency_key.clone());
            }
            6 => changed_m.transaction = Some("forged-uow".into()),
            7 => changed_env.context.owner.host_id += 1,
            8 => changed_env.context.owner.process_id += 1,
            9 => changed_env.context.owner.thread_id += 1,
            10 => changed_env.context.owner.task_id += 1,
            11 => changed_env.context.owner.syncpoint_epoch += 1,
            12 => changed_env.context.owner.environment = MqHostEnvironment::ZosBatch,
            13 => {
                if let MqMqiRequest::Connect(connect) = &mut changed_env.request {
                    connect.manager = Some(MqRouteName::new("Changed.QM").unwrap());
                }
            }
            _ => unreachable!(),
        }
        let result = admit_mqi(
            &scope,
            &candidate,
            &changed_env,
            &changed_m,
            MqMqiEffectOccurrence::from_effect(&changed_e),
            1,
        );
        if matches!(result, Ok(MqMqiAdmission::ServiceValidation(_))) {
            // Spies for the consuming service's protected continuation.
            state_calls.set(state_calls.get() + 1);
            saf_calls.set(saf_calls.get() + 1);
            provider_calls.set(provider_calls.get() + 1);
        }
        assert!(result.is_err(), "case {case}");
    }
    assert_eq!(
        (state_calls.get(), saf_calls.get(), provider_calls.get()),
        (0, 0, 0)
    );
}

#[test]
fn assertions_on_the_original_envelope_do_not_mint_trusted_owner_or_coordinator() {
    let inv = invocation();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    for case in 0..7 {
        let mut env = envelope();
        match case {
            0 => env.context.owner.host_id += 1,
            1 => env.context.owner.process_id += 1,
            2 => env.context.owner.thread_id += 1,
            3 => env.context.owner.task_id += 1,
            4 => env.context.owner.syncpoint_epoch += 1,
            5 => env.context.owner.environment = MqHostEnvironment::ZosBatch,
            6 => env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator,
            _ => unreachable!(),
        }
        // The original request can contain a forged assertion. Host ownership
        // comes independently from the service's actual lifecycle mapping.
        let scope = MqMqiServiceScope::for_host_dispatch(
            &inv,
            owner(),
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            &p,
        );
        assert_eq!(
            admit_mqi(
                &scope,
                &inv,
                &env,
                &m,
                MqMqiEffectOccurrence::from_effect(&e),
                1
            )
            .unwrap_err(),
            HostProblem::Malformed
        );
    }
}

#[test]
fn rejected_grants_cannot_be_added_back_to_a_trusted_scope() {
    let granted = invocation();
    let mut revoked = granted.clone();
    revoked.principal = Principal::new(
        granted.principal.id().clone(),
        BTreeSet::new(),
        InvocationLimits::default(),
    )
    .unwrap();
    let env = envelope();
    let m = mutation();
    let e = effect(&revoked, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &revoked,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    assert_eq!(
        admit_mqi(
            &scope,
            &granted,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1
        )
        .unwrap_err(),
        HostProblem::Unauthorized
    );
}

#[test]
fn provider_generation_readiness_capability_and_exact_budget_fail_closed() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let size = mq_mqi_request_size(&env).unwrap();
    for case in 0..8 {
        let mut inv = inv.clone();
        let mut p = provider();
        match case {
            0 => p.ready = false,
            1 => p.provider_id = "foreign-provider".into(),
            2 => {
                p.capability =
                    CapabilityId::new("host.mq.read", InvocationLimits::default()).unwrap()
            }
            3 => p.generation.clear(),
            4 => {
                inv.provider_generations
                    .insert(p.capability.clone(), "other-generation".into());
            }
            5 => p.max_request_bytes = 0,
            6 => p.max_request_bytes = size - 1,
            7 => p.max_request_bytes = size,
            _ => unreachable!(),
        }
        let scope = MqMqiServiceScope::for_host_dispatch(
            &inv,
            owner(),
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            &p,
        );
        let result = admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1,
        );
        if case == 7 {
            assert!(matches!(
                result.unwrap(),
                MqMqiAdmission::ServiceValidation(_)
            ));
        } else {
            assert!(result.is_err(), "provider case {case}");
        }
    }
}

#[test]
fn malformed_bounds_deadlines_missing_context_and_occurrence_limits_are_atomic() {
    for case in 0..13 {
        let mut inv = invocation();
        let mut env = envelope();
        let mut m = mutation();
        let mut e = effect(&inv, &m);
        let p = provider();
        match case {
            0 => {
                inv.bindings.remove("mq.host-context");
            }
            1 => inv.deadline_tick = 0,
            2 => inv.deadline_tick = u64::MAX,
            3 => e.deadline_tick = 0,
            4 => e.deadline_tick = u64::MAX,
            5 => {
                m.sequence = 0;
                e.sequence = 0;
            }
            6 => {
                m.sequence = inv.limits.max_effects + 1;
                e.sequence = m.sequence;
            }
            7 => env.limits.canonical_bytes = mq_mqi_request_size(&env).unwrap() - 1,
            8 => env.limits.canonical_bytes = 0,
            9 => env.limits.selectors += MqMqiLimits::default().selectors,
            10 => env.context.owner.host_id = 0,
            11 => bind(&mut inv, "oversized", "test@1", &vec![0; 4097]),
            12 => m.transaction = Some(String::new()),
            _ => unreachable!(),
        }
        let before = (inv.clone(), env.clone(), m.clone(), e.clone());
        let scope = MqMqiServiceScope::for_host_dispatch(
            &inv,
            owner(),
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            &p,
        );
        assert!(
            admit_mqi(
                &scope,
                &inv,
                &env,
                &m,
                MqMqiEffectOccurrence::from_effect(&e),
                1
            )
            .is_err(),
            "bound case {case}"
        );
        assert_eq!((inv, env, m, e), before);
    }
}

#[test]
fn callback_notification_role_and_pending_forms_never_become_commands() {
    let inv = invocation();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let mut registry = MqHandleRegistry::new(4, 16).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let handle = registry.create_message(owner(), connection).unwrap();
    let requests = [
        (
            MqMqiRequest::ConnectExtended(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::PendingStructure {
                    requested_version: Some(999),
                },
            }),
            MqMqiPending::StructureAndWireMapping,
        ),
        (
            MqMqiRequest::Control {
                connection,
                operation: MqMqiControl::StartWaitPending,
                options: MqMqiOptions::ContractDefault,
            },
            MqMqiPending::CallbackContext,
        ),
        (
            MqMqiRequest::Callback {
                connection,
                object,
                operation: MqMqiCallbackOperation::EventHandlerPending,
                options: MqMqiOptions::ContractDefault,
            },
            MqMqiPending::CallbackContext,
        ),
        (
            MqMqiRequest::Stat {
                connection,
                kind: MqMqiStatType::AsyncError,
                options: MqMqiOptions::ContractDefault,
            },
            MqMqiPending::StatusMapping,
        ),
        (
            MqMqiRequest::Inquire(MqMqiInquiry {
                connection,
                object,
                selectors: vec![MqMqiSelector::PendingInteger(1)],
                integer_capacity: 1,
                character_capacity: 0,
            }),
            MqMqiPending::SelectorAndAttributeMapping,
        ),
        (
            MqMqiRequest::Set(MqMqiSet {
                connection,
                object,
                selectors: vec![MqMqiSelector::PendingCharacter(999)],
                integers: vec![],
                characters: vec![],
            }),
            MqMqiPending::SelectorAndAttributeMapping,
        ),
    ];
    let before = registry.active_handles();
    for (request, expected) in requests {
        let mut env = envelope();
        env.request = request;
        let scope = MqMqiServiceScope::for_host_dispatch(
            &inv,
            owner(),
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            &p,
        );
        assert!(matches!(admit_mqi(&scope, &inv, &env, &m,
            MqMqiEffectOccurrence::from_effect(&e), 1).unwrap(),
            MqMqiAdmission::Pending { identity, reason } if reason == expected
                && identity.owner == owner()));
        assert_eq!(registry.active_handles(), before);
    }
    let mut env = envelope();
    env.request = MqMqiRequest::CallbackFunction {
        connection,
        callback_id: 1,
        message: None,
        get: None,
        context: MqMqiOptions::ContractDefault,
    };
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    assert_eq!(
        admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1
        )
        .unwrap_err(),
        HostProblem::Unsupported
    );
    assert_eq!(registry.active_handles(), before);
    registry.validate_connection(owner(), connection).unwrap();
    registry
        .validate_message_property(
            owner(),
            connection,
            mainframe_env_host_api::MqHandle::Message(handle),
        )
        .unwrap();
}
