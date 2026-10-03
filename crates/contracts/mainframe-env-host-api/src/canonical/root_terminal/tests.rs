use super::*;
use mainframe_env_execution_api::InvocationLimits;

#[test]
fn terminal_resource_is_bounded_and_distinguishes_original_identities_and_disposition() {
    let limits = InvocationLimits::default();
    let execution = ExecutionId::new("root", limits).unwrap();
    let foreign = ExecutionId::new("foreign-root", limits).unwrap();
    let run = RunUnitId::new("run", limits).unwrap();
    let principal = PrincipalId::new("principal", limits).unwrap();
    let key = IdempotencyKey::new("original-root-key", limits).unwrap();
    let rows = [RootTerminalResourceRow::Exact {
        namespace: "mq-uow",
        key: "unit",
        version: 4,
        payload: b"pending",
    }];
    let config = [3; 32];
    let completion = Completion {
        return_code: 0,
        output: BoundedPayload::new("test-output@1", vec![7], limits).unwrap(),
    };
    let other_completion = Completion {
        return_code: 1,
        output: completion.output.clone(),
    };
    let abend = Abend {
        code: "U0999".into(),
        reason: None,
        dump: AbendDumpDisposition::Unspecified,
    };
    let mut value = RootTerminalResource {
        execution: &execution,
        run: &run,
        principal: &principal,
        invocation_key: &key,
        attempt: 1,
        lifecycle_sequence: 5,
        observed_tick: 10,
        configuration_digest: &config,
        provider_epoch: 9,
        closing_version: 2,
        closing_payload: b"exact-closing",
        disposition: RootTerminalDisposition::Normal { return_code: 0 },
        machine: RootTerminalMachineObservation::Completed(&completion),
        rows: &rows,
    };
    let size = canonical_root_terminal_resource_size(&value, 4096).unwrap();
    let digest = canonical_root_terminal_resource_digest(&value, size).unwrap();
    assert_eq!(
        canonical_root_terminal_resource_digest(&value, size - 1),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        canonical_root_terminal_resource_size(&value, size - 1),
        Err(HostProblem::ResourceExhausted)
    );
    value.execution = &foreign;
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.execution = &execution;
    value.attempt += 1;
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.attempt -= 1;
    value.provider_epoch += 1;
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.provider_epoch -= 1;
    value.closing_version += 1;
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.closing_version -= 1;
    value.observed_tick += 1;
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.observed_tick -= 1;
    value.disposition = RootTerminalDisposition::KnownAbnormal;
    assert_eq!(
        canonical_root_terminal_resource_digest(&value, 4096),
        Err(HostProblem::Malformed)
    );
    value.machine = RootTerminalMachineObservation::Abended(&abend);
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
    value.disposition = RootTerminalDisposition::Normal { return_code: 1 };
    value.machine = RootTerminalMachineObservation::Completed(&other_completion);
    assert_ne!(
        canonical_root_terminal_resource_digest(&value, 4096).unwrap(),
        digest
    );
}

#[test]
fn terminal_row_semantics_have_distinct_canonical_bytes() {
    fn bytes(row: RootTerminalResourceRow<'_>) -> Vec<u8> {
        let mut result = Vec::new();
        encode(&row, DOMAIN, 4096, &mut |b| result.extend_from_slice(b)).unwrap();
        result
    }
    let exact = bytes(RootTerminalResourceRow::Exact {
        namespace: "mq",
        key: "q",
        version: 1,
        payload: b"x",
    });
    for row in [
        RootTerminalResourceRow::Exact {
            namespace: "mq",
            key: "q",
            version: 2,
            payload: b"x",
        },
        RootTerminalResourceRow::Exact {
            namespace: "mq",
            key: "q",
            version: 1,
            payload: b"y",
        },
        RootTerminalResourceRow::Absent {
            namespace: "mq",
            key: "q",
        },
        RootTerminalResourceRow::Put {
            namespace: "mq",
            key: "q",
            version: 1,
            expected: None,
            payload: b"x",
        },
        RootTerminalResourceRow::Delete {
            namespace: "mq",
            key: "q",
            expected: 1,
        },
        RootTerminalResourceRow::Move {
            namespace: "mq",
            old_key: "old",
            key: "q",
            version: 1,
            expected: 1,
            payload: b"x",
        },
    ] {
        assert_ne!(bytes(row), exact);
    }
    assert_ne!(
        bytes(RootTerminalResourceRow::Put {
            namespace: "mq",
            key: "q",
            version: 2,
            expected: Some(1),
            payload: b"x"
        }),
        bytes(RootTerminalResourceRow::Put {
            namespace: "mq",
            key: "q",
            version: 2,
            expected: None,
            payload: b"x"
        })
    );
    assert_ne!(
        bytes(RootTerminalResourceRow::Move {
            namespace: "mq",
            old_key: "a",
            key: "q",
            version: 2,
            expected: 1,
            payload: b"x"
        }),
        bytes(RootTerminalResourceRow::Move {
            namespace: "mq",
            old_key: "b",
            key: "q",
            version: 2,
            expected: 1,
            payload: b"x"
        })
    );
}
