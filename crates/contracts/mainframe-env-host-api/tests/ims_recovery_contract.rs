use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::*;

fn request() -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        application: "LOGAPP".into(),
        package_identity: format!("sha256:{}", "a".repeat(64)),
        psb: "LOGPSB".into(),
        database: "LOGDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call: ImsRecoveryCall::Log {
            code: 0xa0,
            data: vec![0, 0xff, b'\n'],
        },
        mutation: Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("log-1", InvocationLimits::default()).unwrap(),
            transaction: None,
        },
    }
}

#[test]
fn log_contract_rejects_invalid_and_unsupported_forms_without_dropping_operands() {
    let limits = HostLimits::default();
    assert_eq!(request().validate(limits), Ok(()));
    for code in [0xa0, 0xff] {
        let mut r = request();
        r.call = ImsRecoveryCall::Log { code, data: vec![] };
        assert_eq!(r.validate(limits), Ok(()));
    }
    let mut r = request();
    r.call = ImsRecoveryCall::Log {
        code: 0x9f,
        data: vec![],
    };
    assert_eq!(r.validate(limits), Err(HostProblem::Malformed));
    r.call = ImsRecoveryCall::Log {
        code: 0xa0,
        data: vec![0; 32 * 1024],
    };
    assert_eq!(r.validate(limits), Ok(()));
    r.call = ImsRecoveryCall::Log {
        code: 0xa0,
        data: vec![0; 32 * 1024 + 1],
    };
    assert_eq!(r.validate(limits), Err(HostProblem::ResourceExhausted));
    r = request();
    r.mutation.transaction = Some("DISTRIBUTED".into());
    assert_eq!(r.validate(limits), Err(HostProblem::Unsupported));
    r = request();
    r.syntax = ImsCallSyntax::Command;
    assert_eq!(r.validate(limits), Err(HostProblem::Unsupported));
    for context in [
        ImsExecutionContext::DbDc,
        ImsExecutionContext::Dbctl,
        ImsExecutionContext::Dcctl,
        ImsExecutionContext::TmBatch,
    ] {
        r = request();
        r.context = context;
        assert_eq!(r.validate(limits), Err(HostProblem::Unsupported));
    }
    for change in [0, 1, 2, 3] {
        r = request();
        match change {
            0 => r.package_identity = "sha256:abc".into(),
            1 => r.psb = "".into(),
            2 => r.database = "TOO-LONG-NAME".into(),
            _ => r.mutation.sequence = 0,
        }
        assert_eq!(r.validate(limits), Err(HostProblem::Malformed));
    }
    assert!(HostRequest::ImsRecovery(request()).is_mutating());
    assert_eq!(
        HostRequest::ImsRecovery(request())
            .required_capability(InvocationLimits::default())
            .as_str(),
        "host.ims.write"
    );
    assert_eq!(
        ImsRecoveryResult::Logged {
            status: "GE".into(),
            sequence: 1
        }
        .validate(),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        ImsRecoveryResult::Logged {
            status: "  ".into(),
            sequence: 0
        }
        .validate(),
        Err(HostProblem::Malformed)
    );
}

#[test]
fn additive_recovery_canonical_golden_is_independently_framed() {
    // Independently assembled from EFFECT-CANONICAL-V1 tags, ASCII field order,
    // u64 little-endian lengths and SHA-256; not captured from this encoder.
    let request = HostRequest::ImsRecovery(request());
    let result = Ok(HostResult::ImsRecovery(ImsRecoveryResult::Logged {
        status: "  ".into(),
        sequence: 1,
    }));
    let hex = |digest: [u8; 32]| {
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        canonical_request_size(&request, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
        702
    );
    assert_eq!(
        hex(canonical_request_digest(&request).unwrap()),
        "ef3108235fa884c10300732ac0a913d2eb6c3b62b64f1c01a9f80f33056bcd00"
    );
    assert_eq!(
        canonical_result_size(&result, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
        191
    );
    assert_eq!(
        hex(canonical_result_digest(&result).unwrap()),
        "8226957f98ecf1d1de7dc3dcdefced8bdc743ef1d85057f0aceae92380c37e50"
    );
    // All prior IMS/Db2/MQ canonical goldens remain in canonical/tests.rs.
    let legacy = Ok(HostResult::Ims(ImsResult {
        status: "  ".into(),
        segments: vec![],
        checkpoint_id: None,
        affected_segments: 0,
        system: None,
    }));
    assert_eq!(
        canonical_result_size(&legacy, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
        218
    );
    assert_eq!(
        hex(canonical_result_digest(&legacy).unwrap()),
        "faf1ab27e09ccdd795f735bcdbc98f1db83fd4bc42caab4907ba58abe79b0c49"
    );
}

#[test]
fn checkpoint_restart_canonical_goldens_preserve_exact_selector_and_area_identity() {
    // Independent Python struct framing from EFFECT-CANONICAL-V1, including
    // little-endian integer widths, ASCII field order and request/result domains.
    // The same recipe reproduces the pre-existing LOG golden above.
    let cases = [
        (
            ImsRecoveryCall::BasicCheckpoint {
                id: "CHK00001".into(),
            },
            702,
            "6f989e38ad73d8fb7fc74f6a97a72dd97357b833c009bb51c8b4d0d2765b8337",
        ),
        (
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "CHK00001".into(),
                user_areas: vec![vec![0, 255, 10], b"XY".to_vec()],
            },
            756,
            "addd1562a861cfc41b3c114b5e97ccf3d6205778c997a31d9614f876f43f568f",
        ),
        (
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3],
            },
            775,
            "b37adc8f38498168d446ee797dac79b0c4076a94ff8885e18118d798bbac16fe",
        ),
        (
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("CHK00001".into()),
                area_lengths: vec![3],
            },
            806,
            "bfaf1015aa6b3db25040c589825f20a40915d33681de2e98f2de53462b1984ae",
        ),
        (
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Timestamp("00012741234560".into()),
                area_lengths: vec![3],
            },
            811,
            "209088e84007703c73c6a5c62e315496572b9a545b0396322770f559ffb67562",
        ),
        (
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Last,
                area_lengths: vec![3],
            },
            773,
            "b6c90d1d9b8ece2c56724423e4509913e235071085d9eae195c541321e5ef625",
        ),
    ];
    let hex = |digest: [u8; 32]| {
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    for (call, size, expected) in cases {
        let request = HostRequest::ImsRecovery(ImsRecoveryRequest { call, ..request() });
        assert_eq!(
            canonical_request_size(&request, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            size
        );
        assert_eq!(hex(canonical_request_digest(&request).unwrap()), expected);
    }
    for (result, size, expected) in [
        (
            ImsRecoveryResult::Checkpointed {
                status: "  ".into(),
                id: "CHK00001".into(),
                sequence: 1,
            },
            225,
            "4a3bdaf6f77f5d08bbc300be10c4c1847f2b045e37c163f506b3386f8555174e",
        ),
        (
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("CHK00001".into()),
                user_areas: vec![vec![0, 255, 10]],
                pcb_statuses: vec![(1, "  ".into())],
            },
            301,
            "aa7e3aef9da1ecbbfb32eb8d9c7f86a9cacb7e67681fc32aa1abcc1098387e4d",
        ),
    ] {
        let result = Ok(HostResult::ImsRecovery(result));
        assert_eq!(
            canonical_result_size(&result, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            size
        );
        assert_eq!(hex(canonical_result_digest(&result).unwrap()), expected);
    }
}

#[test]
fn checkpoint_restart_contract_rejects_malformed_ids_areas_and_result_shapes() {
    for id in ["", "123456789", "bad id", "é"] {
        let r = ImsRecoveryRequest {
            call: ImsRecoveryCall::BasicCheckpoint { id: id.into() },
            ..request()
        };
        assert_eq!(
            r.validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }
    for timestamp in ["short", "abcdDDD1234560", "ééééééé"] {
        let r = ImsRecoveryRequest {
            call: ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Timestamp(timestamp.into()),
                area_lengths: vec![],
            },
            ..request()
        };
        assert_eq!(
            r.validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }
    for lengths in [vec![1; 8], vec![0], vec![32 * 1024 + 1]] {
        let r = ImsRecoveryRequest {
            call: ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: lengths,
            },
            ..request()
        };
        assert_eq!(
            r.validate(HostLimits::default()),
            Err(HostProblem::ResourceExhausted)
        );
    }
    for pcbs in [
        vec![(0, "  ".into())],
        vec![(1, "  ".into()), (1, "GE".into())],
        vec![(1, "FA".into())],
    ] {
        assert_eq!(
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("SAVE".into()),
                user_areas: vec![],
                pcb_statuses: pcbs
            }
            .validate(),
            Err(HostProblem::Malformed)
        );
    }
}

#[test]
fn backout_contract_preserves_exact_binary_tokens_optional_areas_and_status_shapes() {
    let limits = HostLimits::default();
    for call in [
        ImsRecoveryCall::Sets {
            token: Some([0, 255, 32, 10]),
            user_data: Some(vec![]),
        },
        ImsRecoveryCall::Setu {
            token: None,
            user_data: None,
        },
        ImsRecoveryCall::Rols {
            token: Some(*b"SAVE"),
            area_length: Some(0),
        },
        ImsRecoveryCall::Rols {
            token: None,
            area_length: None,
        },
        ImsRecoveryCall::Roll,
        ImsRecoveryCall::Rolb,
    ] {
        assert_eq!(
            ImsRecoveryRequest { call, ..request() }.validate(limits),
            Ok(())
        );
    }
    for call in [
        ImsRecoveryCall::Sets {
            token: Some(*b"SAVE"),
            user_data: None,
        },
        ImsRecoveryCall::Setu {
            token: None,
            user_data: Some(vec![]),
        },
        ImsRecoveryCall::Rols {
            token: None,
            area_length: Some(0),
        },
    ] {
        assert_eq!(
            ImsRecoveryRequest { call, ..request() }.validate(limits),
            Err(HostProblem::Malformed)
        );
    }
    for call in [
        ImsRecoveryCall::Sets {
            token: Some(*b"SAVE"),
            user_data: Some(vec![0; 32 * 1024 + 1]),
        },
        ImsRecoveryCall::Rols {
            token: Some(*b"SAVE"),
            area_length: Some(32 * 1024 + 1),
        },
    ] {
        assert_eq!(
            ImsRecoveryRequest { call, ..request() }.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
    }
    for status in ["  ", "SA", "SB", "SC"] {
        assert_eq!(
            ImsRecoveryResult::Savepoint {
                status: status.into()
            }
            .validate(),
            Ok(())
        );
    }
    for result in [
        ImsRecoveryResult::Savepoint {
            status: "RA".into(),
        },
        ImsRecoveryResult::BackedOut {
            status: "RA".into(),
            user_data: vec![1],
        },
        ImsRecoveryResult::Abended {
            code: "U0000".into(),
        },
    ] {
        assert_eq!(result.validate(), Err(HostProblem::Malformed));
    }
}

#[test]
fn backout_canonical_goldens_are_independently_framed_and_additive() {
    // Independently framed from EFFECT-CANONICAL-V1, with the same recipe
    // reproducing every old LOG/checkpoint/restart vector. Binary token arrays
    // use the byte-sequence encoding; present empty areas differ from absence.
    let token = Some([0, 255, 32, 10]);
    let hex = |digest: [u8; 32]| {
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    for (call, size, expected) in [
        (
            ImsRecoveryCall::Sets {
                token,
                user_data: Some(b"XY".to_vec()),
            },
            721,
            "79201b5dba11adfc1110a3236ed06f5ede6c337b1618ee225a66e2c4a985559e",
        ),
        (
            ImsRecoveryCall::Setu {
                token,
                user_data: Some(b"XY".to_vec()),
            },
            721,
            "46c1917821d8df852f6a29261385e99c2eb89a48d466d0ec9b9b01e2f61dbd66",
        ),
        (
            ImsRecoveryCall::Sets {
                token: None,
                user_data: None,
            },
            697,
            "50892afa446863b22c8e7a8bccc8242d8c1c256043b17268258e666471bffefe",
        ),
        (
            ImsRecoveryCall::Rols {
                token,
                area_length: Some(2),
            },
            721,
            "a35e8188971af8533a7baf2afeb0ad59b4e1baa47f037f27695d9104fdc69f61",
        ),
        (
            ImsRecoveryCall::Rols {
                token: None,
                area_length: None,
            },
            699,
            "7559ab8dd4dbd04a89900a0c57b2bec4e5e80576343a907a7ec47cb902a286b4",
        ),
        (
            ImsRecoveryCall::Roll,
            663,
            "f0cb80787594909ed4f9d456f824417cec89a0c23f5733584f4b2e06b2fbaeba",
        ),
        (
            ImsRecoveryCall::Rolb,
            663,
            "b8c2e47d991f64289e382cb4c1e3e7b9d0a2b88aa9ceec26e89f2e789d42d536",
        ),
    ] {
        let request = HostRequest::ImsRecovery(ImsRecoveryRequest { call, ..request() });
        assert_eq!(
            canonical_request_size(&request, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            size
        );
        assert_eq!(hex(canonical_request_digest(&request).unwrap()), expected);
    }
    for (result, size, expected) in [
        (
            ImsRecoveryResult::Savepoint {
                status: "SB".into(),
            },
            168,
            "49464da31de6996917a89b5899167b687386b27958255b3a8255f5fa63548fec",
        ),
        (
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: b"XY".to_vec(),
            },
            197,
            "4d6ba608a1b9941ac65b00b9326a5f28ef464e3deb925f8370412f9b43eeb109",
        ),
        (
            ImsRecoveryResult::Abended {
                code: "U0778".into(),
            },
            167,
            "6db213053a0e37e240cbe1945c5f0fd13f3bdbbf8f3ae65cb92737ccff01be8d",
        ),
    ] {
        let result = Ok(HostResult::ImsRecovery(result));
        assert_eq!(
            canonical_result_size(&result, MAX_CANONICAL_EFFECT_BYTES).unwrap(),
            size
        );
        assert_eq!(hex(canonical_result_digest(&result).unwrap()), expected);
    }
}
