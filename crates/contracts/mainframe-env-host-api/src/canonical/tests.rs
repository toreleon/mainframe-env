use super::*;

fn bytes<T: Canonical + ?Sized>(value: &T, domain: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let count = encode(value, domain, MAX_CANONICAL_EFFECT_BYTES, &mut |part| {
        bytes.extend_from_slice(part)
    })
    .unwrap();
    assert_eq!(count, bytes.len());
    bytes
}
fn hex(value: &[u8]) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn golden_request_and_unknown_result_are_versioned_and_domain_separated() {
    let request = HostRequest::State(StateRequest::Get { key: "one".into() });
    let result = Err(HostProblem::UnknownOutcome);
    assert_eq!(bytes(&request, REQUEST_DIGEST_DOMAIN).len(), 150);
    assert_eq!(
        hex(&canonical_request_digest(&request).unwrap()),
        "ae9669241c55748ca32bfe633c73d5bc14eaf68550dfcad22353bd1d6329cf74"
    );
    assert_eq!(bytes(&result, RESULT_DIGEST_DOMAIN).len(), 83);
    assert_eq!(
        hex(&canonical_result_digest(&result).unwrap()),
        "4df2605185ce5b0070cdc2054dc65da70a5bb2bb3302cc10b150468a06f7d5b9"
    );
    assert_ne!(
        digest(&request, REQUEST_DIGEST_DOMAIN).unwrap(),
        digest(&request, RESULT_DIGEST_DOMAIN).unwrap()
    );
}

#[test]
fn browse_request_variants_keep_their_frozen_canonical_bytes() {
    let dataset = DatasetName::new("CARDDEMO.ACCTDAT", 128).unwrap();
    let cases = [
        (
            DatasetRequest::StartBrowse {
                dataset: dataset.clone(),
                key: b"AA".to_vec(),
                relation: KeyRelation::GreaterOrEqual,
            },
            206,
            "4b16e6c153ba020279a02234c1e62c08f5b5f6406665e9c1be4184d4962b9a9d",
        ),
        (
            DatasetRequest::ResetBrowse {
                dataset: dataset.clone(),
                cursor: "CURSOR-1".into(),
                key: b"BB".to_vec(),
                relation: KeyRelation::GreaterOrEqual,
            },
            238,
            "a0065f358f2631f2c4c15827a8ce47aa7f7e10db7156ed4b1d6d18dd65c9da48",
        ),
        (
            DatasetRequest::EndBrowse {
                dataset,
                cursor: "CURSOR-1".into(),
            },
            144,
            "e1ec2e136a887a753dda5193666dee679560def2b22f1add2ef20988839730cb",
        ),
    ];
    for (request, size, expected_digest) in cases {
        assert_eq!(bytes(&request, b"").len(), size);
        assert_eq!(hex(&digest(&request, b"").unwrap()), expected_digest);
    }
}

#[test]
fn principal_validation_is_a_frozen_named_security_variant() {
    let principal = PrincipalId::new(
        "TARGET",
        mainframe_env_execution_api::InvocationLimits::default(),
    )
    .unwrap();
    assert_eq!(
        hex(&bytes(
            &SecurityRequest::ValidatePrincipal { principal },
            b""
        )),
        "41010f0000000000000053656375726974795265717565737401110000000000000056616c69646174655072696e636970616c01000000000000000109000000000000007072696e636970616c42010b000000000000005072696e636970616c4964010600000000000000544152474554"
    );
}

#[test]
fn selected_program_link_is_additive_identity_bound_and_validated() {
    let limits = mainframe_env_execution_api::InvocationLimits::default();
    let program = ProgramName::new("APPMAIN", 128).unwrap();
    let payload = BoundedPayload::new("payload@1", b"DATA".to_vec(), limits).unwrap();
    let artifact = ArtifactRef::new(format!("sha256:{:064x}", 1), limits).unwrap();
    let plain = HostRequest::Program(ProgramRequest::Link {
        program: program.clone(),
        payload: payload.clone(),
        selection: None,
    });
    let selected = HostRequest::Program(ProgramRequest::Link {
        program: program.clone(),
        payload: payload.clone(),
        selection: Some(ProgramLinkSelection {
            artifact: artifact.clone(),
            generation: 7,
            content_identity: format!("sha256:{:064x}", 2),
        }),
    });
    assert_eq!(plain.validate(HostLimits::default()), Ok(()));
    assert_eq!(selected.validate(HostLimits::default()), Ok(()));
    assert_ne!(
        canonical_request_digest(&plain).unwrap(),
        canonical_request_digest(&selected).unwrap()
    );
    for selection in [
        ProgramLinkSelection {
            artifact: artifact.clone(),
            generation: 0,
            content_identity: format!("sha256:{:064x}", 2),
        },
        ProgramLinkSelection {
            artifact,
            generation: 7,
            content_identity: "sha256:not-a-content-identity".into(),
        },
    ] {
        assert_eq!(
            HostRequest::Program(ProgramRequest::Link {
                program: program.clone(),
                payload: payload.clone(),
                selection: Some(selection),
            })
            .validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }
}

#[test]
fn cics_additive_wire_identities_are_frozen_named_variants() {
    assert_eq!(
        hex(&bytes(&CicsOperation::AddressSet, b"")),
        "41010d00000000000000436963734f7065726174696f6e010a00000000000000416464726573735365740000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::ChangeTask, b"")),
        "41010d00000000000000436963734f7065726174696f6e010a000000000000004368616e67655461736b0000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::Deq, b"")),
        "41010d00000000000000436963734f7065726174696f6e0103000000000000004465710000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::Enq, b"")),
        "41010d00000000000000436963734f7065726174696f6e010300000000000000456e710000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::HandleAid, b"")),
        "41010d00000000000000436963734f7065726174696f6e01090000000000000048616e646c654169640000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::IgnoreCondition, b"")),
        "41010d00000000000000436963734f7065726174696f6e010f0000000000000049676e6f7265436f6e646974696f6e0000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::PopHandle, b"")),
        "41010d00000000000000436963734f7065726174696f6e010900000000000000506f7048616e646c650000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::PushHandle, b"")),
        "41010d00000000000000436963734f7065726174696f6e010a000000000000005075736848616e646c650000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsDisposition::Ignored, b"")),
        "41010f0000000000000043696373446973706f736974696f6e01070000000000000049676e6f7265640000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::SetAssociationUserCorrData, b"")),
        "41010d00000000000000436963734f7065726174696f6e011a000000000000005365744173736f63696174696f6e55736572436f7272446174610000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::Suspend, b"")),
        "41010d00000000000000436963734f7065726174696f6e01070000000000000053757370656e640000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::WaitEvent, b"")),
        "41010d00000000000000436963734f7065726174696f6e010900000000000000576169744576656e740000000000000000"
    );
    assert_eq!(
        hex(&bytes(&CicsOperation::WaitExternal, b"")),
        "41010d00000000000000436963734f7065726174696f6e010c000000000000005761697445787465726e616c0000000000000000"
    );
}

#[test]
fn cics_abend_dump_metadata_participates_in_the_result_digest() {
    let response = |dump: Option<&[u8]>| -> Result<HostResult, HostProblem> {
        let outputs = dump
            .map(|value| {
                BTreeMap::from([(
                    "ABEND.DUMP".into(),
                    mainframe_env_execution_api::BoundedPayload::new(
                        "mainframe-env.cics.abend-dump@1",
                        value.to_vec(),
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap(),
                )])
            })
            .unwrap_or_default();
        Ok(HostResult::Cics(CicsResponse {
            disposition: CicsDisposition::Abended,
            condition: "ERROR".into(),
            response: 27,
            response2: 0,
            applid: "MEAPPL".into(),
            sysid: "MESYS".into(),
            transaction: "MENU".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                b"B001".to_vec(),
                mainframe_env_execution_api::InvocationLimits::default(),
            )
            .unwrap(),
            outputs,
            unit_of_work: None,
        }))
    };
    let requested = canonical_result_digest(&response(Some(b"requested"))).unwrap();
    let suppressed = canonical_result_digest(&response(Some(b"suppressed"))).unwrap();
    let historical = canonical_result_digest(&response(None)).unwrap();
    assert_ne!(requested, suppressed);
    assert_ne!(requested, historical);
    assert_ne!(suppressed, historical);
}

#[test]
fn audit_resource_digest_is_versioned_deterministic_and_distinguishes_resources() {
    let one = HostRequest::State(StateRequest::Get { key: "one".into() });
    let two = HostRequest::State(StateRequest::Get { key: "two".into() });
    assert_eq!(
        canonical_audit_resource_digest(&one),
        canonical_audit_resource_digest(&one)
    );
    assert_ne!(
        canonical_audit_resource_digest(&one),
        canonical_audit_resource_digest(&two)
    );
    assert_ne!(
        canonical_audit_resource_digest(&one).value,
        canonical_request_digest(&one).unwrap(),
        "audit and replay identities use separate canonical domains"
    );
}

#[test]
fn provider_replay_digests_share_the_host_journal_encoding_and_have_golden_identities() {
    let limits = mainframe_env_execution_api::InvocationLimits::default();
    let mutation = |key: &str, sequence| Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(key, limits).unwrap(),
        transaction: Some("UNIT-OF-WORK".into()),
    };
    let db2 = Db2Request {
        operation: Db2Operation::Insert,
        statement: "INSERT INTO T VALUES (:ID)".into(),
        cursor: None,
        inputs: BTreeMap::from([(
            "ID".into(),
            Db2HostVariable {
                value: vec![0, 1, 255],
                indicator: Some(-1),
            },
        )]),
        outputs: vec!["ID".into()],
        max_rows: 17,
        mutation: Some(mutation("db2-golden", 3)),
    };
    let ims = ImsRequest {
        operation: ImsOperation::Replace,
        psb: Some("AUTHPSB".into()),
        pcb: 2,
        segments: vec!["CUSTOMER".into(), "ORDER".into()],
        data: vec![0, 10, 255],
        qualifiers: vec![ImsQualifier {
            segment: "CUSTOMER".into(),
            field: "ID".into(),
            value: b"00017".to_vec(),
        }],
        checkpoint_id: Some("CHK00017".into()),
        max_segments: 19,
        mutation: Some(mutation("ims-golden", 5)),
    };
    let mq = MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("APP.REQUEST".into()),
        handle: Some(23),
        options: -7,
        message: vec![0, 10, 255],
        message_id: Some(vec![1; 24]),
        correlation_id: Some(vec![2; 24]),
        wait_ticks: 29,
        max_message_bytes: 31,
        mutation: Some(mutation("mq-golden", 7)),
    };

    let db2_digest = canonical_db2_request_digest(&db2).unwrap();
    let ims_digest = canonical_ims_request_digest(&ims).unwrap();
    let mq_digest = canonical_mq_request_digest(&mq).unwrap();
    assert_eq!(
        db2_digest,
        canonical_request_digest(&HostRequest::Db2(db2)).unwrap()
    );
    assert_eq!(
        ims_digest,
        canonical_request_digest(&HostRequest::Ims(ims)).unwrap()
    );
    assert_eq!(
        mq_digest,
        canonical_request_digest(&HostRequest::Mq(mq)).unwrap()
    );
    assert_eq!(
        hex(&db2_digest),
        "73deaa15e0e23619ee059776d818b7aa0b39805f4dc350f46cb013d3242cb4ad"
    );
    assert_eq!(
        hex(&ims_digest),
        "0be6adcc52e9699a9c1ae6976b0eba69e3b53d1de56b296a6c8a4e7a8621d872"
    );
    assert_eq!(
        hex(&mq_digest),
        "15290c92f51c0823f3a65fbe5f4ff0a96efad4561394b0b8ba7f1225b85a313b"
    );
}
#[test]
fn ordered_maps_ignore_insertion_order_but_not_key_or_value() {
    let a = BTreeMap::from([
        ("b".to_string(), vec![0u8, 255]),
        ("a".to_string(), vec![1]),
    ]);
    let mut b: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    b.insert("a".to_string(), vec![1]);
    b.insert("b".to_string(), vec![0, 255]);
    assert_eq!(bytes(&a, b""), bytes(&b, b""));
    b.insert("b".to_string(), vec![255, 0]);
    assert_ne!(bytes(&a, b""), bytes(&b, b""));
}
#[test]
fn absent_empty_binary_and_text_have_distinct_unambiguous_encodings() {
    assert_eq!(bytes(&None::<Vec<u8>>, b""), [0x20]);
    assert_eq!(
        bytes(&Some(Vec::<u8>::new()), b""),
        [0x21, 2, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        bytes(&vec![0u8, 255, 10], b""),
        [2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 255, 10]
    );
    assert_ne!(bytes(&b"abc".to_vec(), b""), bytes("abc", b""));
    assert_ne!(
        bytes(&vec![b"ab".to_vec(), b"c".to_vec()], b""),
        bytes(&vec![b"a".to_vec(), b"bc".to_vec()], b"")
    );
    let none = Ok(HostResult::State {
        value: None,
        version: 1,
    });
    let empty = Ok(HostResult::State {
        value: Some(vec![]),
        version: 1,
    });
    assert_ne!(
        canonical_result_digest(&none).unwrap(),
        canonical_result_digest(&empty).unwrap()
    );
}
#[test]
fn typed_budgets_use_exact_encoded_size_without_expanded_debug_bytes() {
    let request = HostRequest::State(StateRequest::Put {
        key: "binary".into(),
        value: vec![255; 4096],
        expected_version: None,
        mutation: Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new(
                "binary",
                mainframe_env_execution_api::InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    });
    let n = canonical_request_size(&request, usize::MAX).unwrap();
    assert!(n < 4600);
    assert_eq!(canonical_request_size(&request, n), Ok(n));
    assert_eq!(
        canonical_request_size(&request, n - 1),
        Err(HostProblem::ResourceExhausted)
    );
    let result = Ok(HostResult::State {
        value: Some(vec![255; 4096]),
        version: 1,
    });
    let n = canonical_result_size(&result, usize::MAX).unwrap();
    assert_eq!(canonical_result_size(&result, n), Ok(n));
    assert_eq!(
        canonical_result_size(&result, n - 1),
        Err(HostProblem::ResourceExhausted)
    );
    let mut touched = 0;
    let mut sink = |_: &[u8]| {
        touched += 1;
    };
    let mut out = Encoder {
        sink: &mut sink,
        size: usize::MAX,
        limit: usize::MAX,
    };
    assert_eq!(out.put(&[0]), Err(HostProblem::ResourceExhausted));
    assert_eq!(touched, 0);
}
#[test]
fn canonical_encoding_never_invokes_debug() {
    struct FormattingBomb;
    impl std::fmt::Debug for FormattingBomb {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("not a wire representation")
        }
    }
    impl Canonical for FormattingBomb {
        fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
            out.text("stable")
        }
    }
    assert_eq!(bytes(&FormattingBomb, b""), bytes("stable", b""));
}
