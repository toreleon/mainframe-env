use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;

#[test]
fn actual_original_invocation_byte_budget_fails_before_frame_allocation_and_saf() {
    for sqlite in [false, true] {
        let mut f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        f.parent.bindings.insert(
            "bounded-host-test".into(),
            BoundedPayload::new("test@1", vec![0; 512 * 1024], Default::default()).unwrap(),
        );
        let before = f.rows();
        let (_, reply) = f.run(f.effect(1, "MQFLOW"));
        assert_eq!(reply.outcome, Err(HostProblem::ResourceExhausted));
        assert_eq!(f.rows(), before);
        let topology = f.mq.topology.lock().unwrap();
        assert!(topology.roots.is_empty() && topology.frames.is_empty());
        assert_eq!(topology.bytes, 0);
        assert!(f.saf.resources.lock().unwrap().is_empty());
    }
}

#[test]
fn exhausted_actual_retained_frame_capacity_never_makes_a_finished_frame_fresh() {
    for sqlite in [false, true] {
        let mut f = Fixture::bounded(sqlite, 1);
        f.install("MQFLOW", SOURCE);
        assert!(f.run(f.effect(1, "MQFLOW")).1.outcome.is_ok());
        let before = f.rows();
        let saf_calls = f.saf.resources.lock().unwrap().len();
        // Another genuinely coordinated root and original CALL, not a forged
        // child/root conversion. Finished frame slots deliberately stay retained.
        f.parent.execution_id = ExecutionId::new("another-root", Default::default()).unwrap();
        f.parent.run_unit_id = RunUnitId::new("another-run", Default::default()).unwrap();
        f.parent.idempotency_key =
            IdempotencyKey::new("another-root-key", Default::default()).unwrap();
        let mut effect = f.effect(1, "MQFLOW");
        effect.idempotency_key =
            Some(IdempotencyKey::new("another-original-call", Default::default()).unwrap());
        let (_, reply) = f.run(effect);
        assert_eq!(reply.outcome, Err(HostProblem::ResourceExhausted));
        assert_eq!(f.rows(), before);
        assert_eq!(f.saf.resources.lock().unwrap().len(), saf_calls);
        assert_eq!(f.factory.children.lock().unwrap().len(), 1);
    }
}

#[test]
fn physical_audit_saturation_rolls_back_entire_selected_batch_on_both_backends() {
    for sqlite in [false, true] {
        let f = Fixture::quota(sqlite, 16, false, false, 128);
        f.install("MQFLOW", SOURCE);
        let store = f.store.clone();
        let count = Arc::new(Mutex::new(0));
        let captured = count.clone();
        let prototype = Arc::new(Mutex::new(None));
        let prepared = prototype.clone();
        *f.factory.hook.lock().unwrap() = Some(Box::new(move |proof| {
            let inv = proof.parent();
            let audit = AuditRecord {
                execution_id: inv.execution_id.clone(),
                run_unit_id: inv.run_unit_id.clone(),
                attempt: inv.attempt,
                effect_sequence: proof.original_call().sequence,
                observed_tick: 20,
                principal: inv.principal.id().clone(),
                invocation_key: inv.idempotency_key.clone(),
                capability: proof
                    .original_call()
                    .request
                    .required_capability(Default::default()),
                resource: canonical_audit_resource_digest(&proof.original_call().request),
                decision: AuditDecision::Success,
            };
            *prepared.lock().unwrap() = Some(audit);
        }));
        *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
            let audit = prototype.lock().unwrap().clone().unwrap();
            // Fill only AFTER the actual selected service has admitted the
            // original child core intent and reached mandatory SAF. This is
            // publication rollback, not startup/execution-admission saturation.
            // Real bounded backend quota, with independently stored ordinals.
            // Setup filling is not a provider permission or execution claim.
            for _ in 0..=128 {
                if store.record_audit(audit.clone()).is_err() {
                    break;
                }
                *captured.lock().unwrap() += 1;
            }
            assert!(*captured.lock().unwrap() > 0);
        }));
        let before = f.rows();
        let (_, reply) = f.run_raw(f.effect(1, "MQFLOW"));
        assert!(reply.is_none_or(|r| r.outcome.is_err()));
        assert_eq!(f.rows(), before);
        assert!(!f.saf.resources.lock().unwrap().is_empty());
        assert!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            f.store
                .audit_records(&f.parent.execution_id, 0, 128)
                .unwrap()
                .len(),
            *count.lock().unwrap()
        );
    }
}
