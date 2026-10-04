use super::*;
use mainframe_env_execution_api::*;
use std::collections::{BTreeMap, BTreeSet};

fn original() -> Invocation {
    let l = InvocationLimits::default();
    Invocation::new(
        RequestId::new("root-request", l).unwrap(),
        ExecutionId::new("root-execution", l).unwrap(),
        RunUnitId::new("root-run", l).unwrap(),
        None,
        Selector::new("program:ROOT", l).unwrap(),
        ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), l).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", l).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
            l,
        )
        .unwrap(),
        ServiceClass::Batch,
        4,
        100,
        TraceId::new("original-trace", l).unwrap(),
        IdempotencyKey::new("root-original-key", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap()
}
fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
        provider_id: "test-selected-mq".into(),
        generation: "test-generation".into(),
        request_schema: "mainframe-env.host-request@1".into(),
        result_schema: "mainframe-env.host-result@1".into(),
        max_request_bytes: 4096,
        max_result_bytes: 4096,
        ready: true,
    }
}
fn setup<'a>(v: &'a Invocation, d: &'a CapabilityDescriptor) -> RootTerminalSetup<'a> {
    RootTerminalSetup {
        original: v,
        provider: d,
        host_limits: HostLimits::default(),
        mqi_limits: MqMqiLimits::default(),
        generation: 3,
        fence: 7,
        mq_limits: &[8, 8, 4096, 32, 8, 32, 65536],
        content_digest: &[2; 32],
        manifest_payload_digest: &[4; 32],
        semantic_identity: "semantic-sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        catalog: RootTerminalResourceRow::Exact {
            namespace: "batch-program",
            key: "ROOT",
            version: 1,
            payload: b"original-artifact",
        },
    }
}

#[test]
fn frozen_root_setup_stream_is_exactly_bounded_and_input_sensitive() {
    let v = original();
    let d = descriptor();
    let value = setup(&v, &d);
    let count = encode(&value, SETUP_DOMAIN, 65536, &mut |_| {}).unwrap();
    let digest = canonical_root_terminal_setup_digest(&value, count).unwrap();
    assert_eq!(
        canonical_root_terminal_setup_digest(&value, count).unwrap(),
        digest
    );
    assert_eq!(
        canonical_root_terminal_setup_digest(&value, count - 1),
        Err(HostProblem::ResourceExhausted)
    );
    for mutant in 0..12 {
        let mut v = v.clone();
        let mut d = d.clone();
        match mutant {
            0 => {
                v.request_id = RequestId::new("other-request", InvocationLimits::default()).unwrap()
            }
            1 => {
                v.execution_id =
                    ExecutionId::new("other-execution", InvocationLimits::default()).unwrap()
            }
            2 => {
                v.idempotency_key =
                    IdempotencyKey::new("other-root-key", InvocationLimits::default()).unwrap()
            }
            3 => v.attempt += 1,
            4 => v.deadline_tick += 1,
            5 => v.audit_correlation = "other-audit".into(),
            6 => v
                .bindings
                .insert(
                    "host-config".into(),
                    BoundedPayload::new("config@1", vec![1], InvocationLimits::default()).unwrap(),
                )
                .map(|_| ())
                .unwrap_or(()),
            7 => {
                v.principal = Principal::new(
                    v.principal.id().clone(),
                    BTreeSet::new(),
                    InvocationLimits::default(),
                )
                .unwrap()
            }
            8 => v.limits.max_output_bytes += 1,
            9 => d.generation = "other-provider-generation".into(),
            10 => v.cancellation_probe = Some(CancellationProbe::new()),
            11 => v.trace_id = TraceId::new("other-trace", InvocationLimits::default()).unwrap(),
            _ => unreachable!(),
        }
        assert_ne!(
            canonical_root_terminal_setup_digest(&setup(&v, &d), 65536).unwrap(),
            digest,
            "input {mutant}"
        );
    }
    let mut value = value;
    value.manifest_payload_digest = &[5; 32];
    assert_ne!(
        canonical_root_terminal_setup_digest(&value, 65536).unwrap(),
        digest
    );
    value.manifest_payload_digest = &[4; 32];
    value.fence += 1;
    assert_ne!(
        canonical_root_terminal_setup_digest(&value, 65536).unwrap(),
        digest
    );
}

#[test]
fn frozen_root_setup_refuses_child_nonfinite_identity_and_oversized_profiles() {
    let d = descriptor();
    let mut v = original();
    v.parent_execution_id = Some(ExecutionId::new("parent", InvocationLimits::default()).unwrap());
    assert_eq!(
        canonical_root_terminal_setup_digest(&setup(&v, &d), 65536),
        Err(HostProblem::Malformed)
    );
    v.parent_execution_id = None;
    v.deadline_tick = u64::MAX;
    assert_eq!(
        canonical_root_terminal_setup_digest(&setup(&v, &d), 65536),
        Err(HostProblem::Malformed)
    );
    v.deadline_tick = 100;
    let mut value = setup(&v, &d);
    value.generation = i64::MAX as u64 + 1;
    assert_eq!(
        canonical_root_terminal_setup_digest(&value, 65536),
        Err(HostProblem::Malformed)
    );
    value.generation = 3;
    value.fence = 0;
    assert_eq!(
        canonical_root_terminal_setup_digest(&value, 65536),
        Err(HostProblem::Malformed)
    );
    value.fence = 7;
    value.mqi_limits.canonical_bytes = usize::MAX;
    assert_eq!(
        canonical_root_terminal_setup_digest(&value, 65536),
        Err(HostProblem::Malformed)
    );
}
