use super::*;
use mainframe_env_host_api::mq_object_route::MqRouteName;
use mainframe_env_host_api::{MqHandleRegistry, canonical_request_digest, canonical_request_size};

#[test]
fn admitted_identity_is_borrowed_exact_and_deterministic_on_retry() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m, &env);
    let p = provider();
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let HostRequest::MqMqi(original) = &e.request else {
        unreachable!()
    };
    assert_eq!(original.envelope.review(), Ok(MqMqiPending::PublicDispatch));
    for _ in 0..2 {
        let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap()
        else {
            panic!("boundary")
        };
        assert!(std::ptr::eq(identity.envelope, &original.envelope));
        assert!(std::ptr::eq(identity.mutation, &original.mutation));
        assert!(std::ptr::eq(identity.effect(), &e));
        assert!(std::ptr::eq(identity.invocation(), &inv));
        assert_eq!(identity.owner, owner());
        assert_eq!(identity.observed_tick, 1);
        assert_eq!(
            identity.host_request_bytes,
            canonical_request_size(&e.request, p.max_request_bytes).unwrap()
        );
        assert_eq!(
            identity.host_request_digest,
            canonical_request_digest(&e.request).unwrap()
        );
        assert_eq!(identity.effect().run_unit, inv.run_unit_id);
        assert_eq!(identity.effect().deadline_tick, e.deadline_tick);
        assert_eq!(identity.effect().sequence, m.sequence);
        assert_eq!(
            identity.effect().idempotency_key.as_ref(),
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
    let e = effect(&inv, &mutation(), &envelope());
    let p = provider();
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!("boundary")
    };
    assert_eq!(identity.recheck_controls(89), Ok(()));
    assert_eq!(identity.recheck_controls(0), Err(HostProblem::Malformed));
    assert_eq!(identity.recheck_controls(90), Err(HostProblem::TimedOut));
    for probe in [None, Some(CancellationProbe::new())] {
        let mut candidate = inv.clone();
        candidate.cancellation_probe = probe;
        assert_eq!(
            admit_mqi(&scope, &candidate, 1).unwrap_err(),
            HostProblem::IdempotencyConflict
        );
    }
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(identity.recheck_controls(2), Err(HostProblem::Cancelled));
    assert_eq!(
        admit_mqi(&scope, &inv, 2).unwrap_err(),
        HostProblem::Cancelled
    );
}

#[test]
fn every_trusted_invocation_field_is_preserved_before_protected_continuation() {
    let inv = invocation();
    let e = effect(&inv, &mutation(), &envelope());
    let p = provider();
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let calls = Cell::new(0);
    for case in 0..17 {
        let mut candidate = inv.clone();
        let l = InvocationLimits::default();
        match case {
            0 => {
                candidate.principal = Principal::new(
                    PrincipalId::new("FOREIGN", l).unwrap(),
                    inv.principal.grants().clone(),
                    l,
                )
                .unwrap()
            }
            1 => candidate.idempotency_key = IdempotencyKey::new("forged", l).unwrap(),
            2 => candidate.attempt += 1,
            3 => candidate.limits.max_effects += 1,
            4 => candidate.execution_id = ExecutionId::new("other", l).unwrap(),
            5 => candidate.run_unit_id = RunUnitId::new("other", l).unwrap(),
            6 => candidate.request_id = RequestId::new("other", l).unwrap(),
            7 => candidate.parent_execution_id = Some(ExecutionId::new("parent", l).unwrap()),
            8 => candidate.selector = Selector::new("other", l).unwrap(),
            9 => candidate.artifact = ArtifactRef::new("other", l).unwrap(),
            10 => candidate.service_class = ServiceClass::Batch,
            11 => candidate.priority += 1,
            12 => candidate.trace_id = TraceId::new("other", l).unwrap(),
            13 => candidate.audit_correlation = "other".into(),
            14 => candidate.deadline_tick -= 1,
            15 => {
                candidate
                    .provider_generations
                    .insert(p.capability.clone(), "other".into());
            }
            16 => {
                candidate.principal = Principal::new(
                    inv.principal.id().clone(),
                    BTreeSet::from([p.capability.clone()]),
                    l,
                )
                .unwrap()
            }
            _ => unreachable!(),
        }
        let result = admit_mqi(&scope, &candidate, 1);
        if result.is_ok() {
            calls.set(calls.get() + 1);
        }
        assert!(result.is_err(), "invocation field {case}");
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn assertions_on_the_original_envelope_do_not_mint_trusted_owner_or_coordinator() {
    let inv = invocation();
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
        let original = effect(&inv, &mutation(), &env);
        assert_eq!(
            attempt(&inv, &inv, owner(), &original, &p, 1),
            Err(HostProblem::Malformed)
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
    let e = effect(&revoked, &mutation(), &envelope());
    let p = provider();
    assert_eq!(
        attempt(&revoked, &granted, owner(), &e, &p, 1),
        Err(HostProblem::Unauthorized)
    );
}

#[test]
fn provider_generation_readiness_capability_and_full_host_budget_fail_closed() {
    let inv = invocation();
    let e = effect(&inv, &mutation(), &envelope());
    let size = canonical_request_size(&e.request, usize::MAX).unwrap();
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
        let result = attempt(&inv, &inv, owner(), &e, &p, 1);
        if case == 7 {
            assert_eq!(result, Ok(()));
        } else {
            assert!(result.is_err(), "provider case {case}");
        }
    }
}

#[test]
fn malformed_bounds_deadlines_missing_context_and_occurrence_limits_are_atomic() {
    for case in 0..14 {
        let mut inv = invocation();
        let mut env = envelope();
        let mut m = mutation();
        match case {
            0 => {
                inv.bindings.remove("mq.host-context");
            }
            1 => inv.deadline_tick = 0,
            2 => inv.deadline_tick = u64::MAX,
            5 => m.sequence = 0,
            6 => m.sequence = inv.limits.max_effects + 1,
            7 => env.limits.canonical_bytes = 1,
            8 => env.limits.canonical_bytes = 0,
            9 => env.limits.selectors += MqMqiLimits::default().selectors,
            10 => env.context.owner.host_id = 0,
            11 => bind(&mut inv, "oversized", "test@1", &vec![0; 4097]),
            12 => m.transaction = Some(String::new()),
            _ => {}
        }
        let mut e = effect(&inv, &m, &env);
        if case == 3 {
            e.deadline_tick = 0;
        }
        if case == 4 {
            e.deadline_tick = u64::MAX;
        }
        if case == 13 {
            e.request = HostRequest::Clock(mainframe_env_host_api::ClockRequest::Date);
        }
        let before = (inv.clone(), e.clone());
        let p = provider();
        assert!(
            attempt(&inv, &inv, owner(), &e, &p, 1).is_err(),
            "bound case {case}"
        );
        assert_eq!((inv, e), before);
    }
}

#[test]
fn callback_notification_role_and_pending_forms_never_become_commands() {
    let inv = invocation();
    let p = provider();
    let mut registry = MqHandleRegistry::new(4, 16).unwrap();
    let connection = registry
        .connect(owner(), mainframe_env_host_api::MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let handle = registry.create_message(owner(), connection).unwrap();
    let requests = [
        (
            MqMqiRequest::ConnectExtended(MqMqiConnect {
                manager: None,
                sharing: mainframe_env_host_api::MqHandleSharing::NonShared,
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
        let e = effect(&inv, &mutation(), &env);
        let scope = scope(&inv, owner(), &e, &p).unwrap();
        assert!(matches!(admit_mqi(&scope, &inv, 1).unwrap(),
            MqMqiAdmission::Pending { _identity: identity, _reason: reason } if reason == expected && identity.owner == owner()));
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
    let e = effect(&inv, &mutation(), &env);
    assert_eq!(
        attempt(&inv, &inv, owner(), &e, &p, 1),
        Err(HostProblem::Malformed)
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

#[test]
fn trusted_host_limits_are_rechecked_on_the_same_original_occurrence() {
    let inv = invocation();
    let p = provider();
    let mut env = envelope();
    if let MqMqiRequest::Connect(value) = &mut env.request {
        value.manager = Some(MqRouteName::new("QMGR").unwrap());
    }
    let e = effect(&inv, &mutation(), &env);
    let occurrence = e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap();
    let limits = HostLimits {
        max_name_bytes: 3,
        ..HostLimits::default()
    };
    let scope = MqMqiServiceScope::for_host_dispatch(&inv, owner(), occurrence, &p, limits);
    assert_eq!(
        admit_mqi(&scope, &inv, 1).unwrap_err(),
        HostProblem::ResourceExhausted
    );
}
