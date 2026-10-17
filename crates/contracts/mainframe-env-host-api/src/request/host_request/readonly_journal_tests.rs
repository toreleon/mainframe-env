//! Typed dispatch ownership; no mutation or provider authority is synthesized.
use super::*;

fn file_request(operation: CicsOperation) -> HostRequest {
    HostRequest::Cics(CicsRequest {
        operation,
        arguments: Default::default(),
        condition_policy: CicsConditionPolicy::NoHandle,
        mutation: None,
    })
}

#[test]
fn readonly_occurrences_keep_capabilities_and_none_mutation_metadata() {
    let read = HostRequest::Dataset(DatasetRequest::Read {
        dataset: crate::DatasetName::new("TEST.INPUT", 128).unwrap(),
        member: None,
        key: None,
        control: Default::default(),
        max_records: 1,
    });
    assert!(read.requires_dispatch_record());
    assert!(!read.is_mutating());
    assert_eq!(read.mutation(), None);
    assert_eq!(
        read.required_capability(InvocationLimits::default())
            .as_str(),
        "host.dataset.read"
    );
    for operation in [
        CicsOperation::Read,
        CicsOperation::ReadNext,
        CicsOperation::ReadPrev,
        CicsOperation::StartBrowse,
        CicsOperation::EndBrowse,
    ] {
        let request = file_request(operation);
        assert!(request.requires_dispatch_record());
        assert!(!request.is_mutating());
        assert_eq!(request.mutation(), None);
        let effect = EffectRequest {
            run_unit: RunUnitId::new("run", InvocationLimits::default()).unwrap(),
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: Some(
                IdempotencyKey::new("original:1", InvocationLimits::default()).unwrap(),
            ),
            request,
        };
        assert_eq!(effect.validate(HostLimits::default()), Ok(()));
        assert_eq!(
            effect
                .request
                .required_capability(InvocationLimits::default())
                .as_str(),
            "host.cics.execute"
        );
    }
}

#[test]
fn unselected_observations_and_existing_token_binding_remain_distinct() {
    for request in [
        HostRequest::State(StateRequest::Get {
            key: "value".into(),
        }),
        HostRequest::Dataset(DatasetRequest::Attributes {
            dataset: crate::DatasetName::new("TEST.INPUT", 128).unwrap(),
        }),
        file_request(CicsOperation::Assign),
    ] {
        assert!(!request.requires_dispatch_record());
        assert!(!request.is_mutating());
    }
    let limits = InvocationLimits::default();
    let HostRequest::Cics(mut token) = file_request(CicsOperation::Read) else {
        unreachable!()
    };
    token.arguments.insert(
        "TOKEN".into(),
        BoundedPayload::new("mainframe-env.cics.literal@1", b"T".to_vec(), limits).unwrap(),
    );
    assert!(token.is_mutating());
    let key = IdempotencyKey::new("original:1", limits).unwrap();
    let mut effect = EffectRequest {
        run_unit: RunUnitId::new("run", limits).unwrap(),
        sequence: 1,
        deadline_tick: 100,
        idempotency_key: Some(key.clone()),
        request: HostRequest::Cics(token.clone()),
    };
    assert_eq!(
        effect.validate(HostLimits::default()),
        Err(HostProblem::MissingIdempotency)
    );
    token.mutation = Some(Mutation {
        sequence: 1,
        idempotency_key: key,
        transaction: None,
    });
    effect.request = HostRequest::Cics(token);
    assert_eq!(effect.validate(HostLimits::default()), Ok(()));
    effect.sequence = 2;
    assert_eq!(
        effect.validate(HostLimits::default()),
        Err(HostProblem::IdempotencyConflict)
    );
}
