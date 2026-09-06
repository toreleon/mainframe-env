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
