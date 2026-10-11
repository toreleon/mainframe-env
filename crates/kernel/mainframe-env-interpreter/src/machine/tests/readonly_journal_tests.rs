//! Actual machine occurrence keys, independent of mutation metadata.
use super::*;
use mainframe_env_host_api::{CicsConditionPolicy, CicsRequest, StateRequest};

fn fresh() -> ReferenceMachine {
    ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap()
}

fn requests() -> Vec<HostRequest> {
    let mut requests = vec![HostRequest::Dataset(DatasetRequest::Read {
        dataset: DatasetName::new("TEST.INPUT", 128).unwrap(),
        member: None,
        key: Some(b"7".to_vec()),
        control: Default::default(),
        max_records: 1,
    })];
    requests.extend(
        [
            CicsOperation::Read,
            CicsOperation::StartBrowse,
            CicsOperation::ReadNext,
            CicsOperation::ReadPrev,
            CicsOperation::EndBrowse,
        ]
        .into_iter()
        .map(|operation| {
            HostRequest::Cics(CicsRequest {
                operation,
                arguments: BTreeMap::new(),
                condition_policy: CicsConditionPolicy::NoHandle,
                mutation: None,
            })
        }),
    );
    requests
}

#[test]
fn readonly_keys_bind_original_occurrence_without_reclassifying_mutation() {
    let mut first = fresh();
    let mut replay = fresh();
    for (index, request) in requests().into_iter().enumerate() {
        let Step::Effect(offered) = first.effect(request.clone(), PendingKind::Ignore).unwrap()
        else {
            panic!("actual effect required")
        };
        let Step::Effect(repeated) = replay.effect(request.clone(), PendingKind::Ignore).unwrap()
        else {
            panic!("actual effect required")
        };
        assert_eq!(offered, repeated);
        assert_eq!(offered.request, request);
        assert!(!offered.request.is_mutating());
        assert_eq!(offered.request.mutation(), None);
        assert_eq!(offered.sequence, u64::try_from(index).unwrap() + 1);
        assert_eq!(
            offered.idempotency_key.as_ref().unwrap().as_str(),
            format!("idem:{}", index + 1)
        );
        assert_eq!(offered.run_unit, first.invocation.run_unit_id);
        assert_eq!(offered.deadline_tick, first.invocation.deadline_tick);
        assert_eq!(offered.validate(HostLimits::default()), Ok(()));
        first.pending = None;
        replay.pending = None;
    }
    let Step::Effect(unselected) = first
        .effect(
            HostRequest::State(StateRequest::Get {
                key: "value".into(),
            }),
            PendingKind::Ignore,
        )
        .unwrap()
    else {
        panic!("actual effect required")
    };
    assert_eq!(unselected.idempotency_key, None);
    assert_eq!(unselected.sequence, 7);
}

#[test]
fn readonly_sequence_and_original_key_budget_refuse_before_effect_offer() {
    let mut machine = fresh();
    machine.effect_sequence = machine.invocation.limits.max_effects;
    assert!(matches!(
        machine.effect(requests().remove(0), PendingKind::Ignore),
        Err(MachineProblem::ResourceExhausted)
    ));
    assert!(machine.pending.is_none());
    let mut machine = fresh();
    machine.invocation.idempotency_key = IdempotencyKey::new(
        "K".repeat(InvocationLimits::default().max_identity_bytes),
        InvocationLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        machine.effect(requests().remove(0), PendingKind::Ignore),
        Err(MachineProblem::ResourceExhausted)
    ));
    assert!(machine.pending.is_none());
}
