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
