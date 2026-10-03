use super::*;
use mainframe_env_host_api::{canonical_request_digest, canonical_request_size};

#[test]
fn retained_admission_identity_is_the_actual_original_host_request() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m, &env);
    let p = provider();
    let scope = scope(&inv, owner(), &e, &p).unwrap();
    let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap() else {
        panic!("typed intent")
    };
    assert_eq!(
        identity.host_request_digest,
        canonical_request_digest(&e.request).unwrap()
    );
    assert_eq!(
        identity.host_request_bytes,
        canonical_request_size(&e.request, p.max_request_bytes).unwrap()
    );
    assert_ne!(
        identity.host_request_digest,
        mq_mqi_request_digest(identity.envelope).unwrap()
    );
}

#[test]
fn original_mutation_is_part_of_host_identity_even_when_envelope_is_unchanged() {
    let inv = invocation();
    let p = provider();
    let env = envelope();
    let mut hashes = BTreeSet::new();
    let mut shape_hashes = BTreeSet::new();
    for case in 0..4 {
        let mut m = mutation();
        match case {
            1 => m.sequence += 1,
            2 => {
                m.idempotency_key =
                    IdempotencyKey::new("other", InvocationLimits::default()).unwrap()
            }
            3 => m.transaction = Some("UOW".into()),
            _ => {}
        }
        let e = effect(&inv, &m, &env);
        let scope = scope(&inv, owner(), &e, &p).unwrap();
        let MqMqiAdmission::ServiceValidation(identity) = admit_mqi(&scope, &inv, 1).unwrap()
        else {
            panic!()
        };
        hashes.insert(identity.host_request_digest);
        shape_hashes.insert(mq_mqi_request_digest(identity.envelope).unwrap());
    }
    assert_eq!(hashes.len(), 4);
    assert_eq!(shape_hashes.len(), 1);
}

#[test]
fn observations_and_cursor_calls_use_the_same_original_sequence_key_authority() {
    use mainframe_env_host_api::{
        MqGetContract, MqGetMode, MqHandleRegistry, MqPropertyQuery, MqTruncation, MqWait,
    };
    let inv = invocation();
    let p = provider();
    let mut registry = MqHandleRegistry::new(32, 8).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let handle = registry.create_message(owner(), connection).unwrap();
    let requests = [
        MqMqiRequest::Inquire(MqMqiInquiry {
            connection,
            object,
            selectors: vec![],
            integer_capacity: 1,
            character_capacity: 1,
        }),
        MqMqiRequest::InquireProperty(MqMqiPropertyInquiry {
            connection,
            handle,
            query: MqPropertyQuery::Prefix(String::new()),
            after: Some("cursor".into()),
            requested_type: None,
            value_capacity: 2,
            name_capacity: 8,
            options: MqMqiOptions::ContractDefault,
        }),
        MqMqiRequest::Get(MqMqiGet {
            connection,
            object,
            message_handle: None,
            get: MqGetContract {
                selection: Default::default(),
                mode: MqGetMode::BrowseNext { cursor: 9 },
                wait: MqWait::NoWait,
                truncation: MqTruncation::Reject,
                buffer_capacity: 2,
            },
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
        }),
    ];
    for request in requests {
        let mut env = envelope();
        env.request = request;
        let original = effect(&inv, &mutation(), &env);
        assert_eq!(attempt(&inv, &inv, owner(), &original, &p, 1), Ok(()));
        for case in 0..3 {
            let mut changed = original.clone();
            match case {
                0 => changed.idempotency_key = None,
                1 => changed.sequence += 1,
                2 => {
                    changed.idempotency_key =
                        Some(IdempotencyKey::new("other", InvocationLimits::default()).unwrap())
                }
                _ => unreachable!(),
            }
            assert!(attempt(&inv, &inv, owner(), &changed, &p, 1).is_err());
        }
    }
    assert_eq!(registry.active_handles(), 3);
}
