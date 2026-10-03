//! The rich operand adapter uses the same integrity and replay authority.
use super::*;
use mainframe_env_host_api::ImsNavigationRequest;

fn rich_read(service: &Arc<ImsService>, sequence: u64, pcb: u16) -> Result<ImsResult, HostProblem> {
    let invocation = invocation("integrity-b");
    let mut request = read(
        "integrity-b",
        ImsOperation::GetUnique,
        sequence,
        pcb,
        "CHILD",
    );
    request.segments.clear();
    let mutation = request.mutation.as_ref().unwrap().clone();
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: mutation.sequence,
        idempotency_key: Some(mutation.idempotency_key),
        request: HostRequest::ImsNavigation(ImsNavigationRequest {
            request,
            context: ImsExecutionContext::DbBatch,
            ssas: vec![b"CHILD    ".to_vec()],
        }),
        deadline_tick: invocation.deadline_tick,
    };
    match ims_providers(service.clone(), InvocationLimits::default())[1]
        .invoke(&invocation, effect)
        .outcome?
    {
        HostResult::Ims(result) => Ok(result),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn rich_ssa_integrity_replay_and_trusted_go_use_the_common_atomic_pipeline() {
    backends(|store| {
        let owner = seeded(store.clone());
        assert_eq!(rich_read(&owner, 2, 2).unwrap().segments[0].data, b"C1Y");
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        pending(&owner);
        let before = rows(&*store);
        assert_eq!(rich_read(&stale, 2, 2).unwrap().segments[0].data, b"C1Y");
        assert_eq!(rows(&*store), before);
        assert_eq!(
            rich_read(&stale, 3, 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        assert_eq!(rich_read(&stale, 4, 3).unwrap().segments[0].data, b"C1Z");
    });
}
