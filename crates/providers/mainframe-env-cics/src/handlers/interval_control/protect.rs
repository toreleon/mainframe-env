use super::*;
use mainframe_env_host_api::CicsUnitOfWorkOutcome;
use std::collections::BTreeSet;

pub(in crate::service) fn finish_syncpoint(
    service: &CicsService,
    run: &Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<(), HostProblem> {
    match outcome {
        CicsUnitOfWorkOutcome::Committed => commit(service, run),
        CicsUnitOfWorkOutcome::RolledBack => discard(service, run),
    }
}

fn commit(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let run_unit = run.invocation.run_unit_id.as_str();
    let (released, pending) = {
        let mut state = service.lock()?;
        let released = release(
            service.store.as_ref(),
            &mut state.interval_records,
            run_unit,
            service.limits,
        )?;
        let pending = state
            .interval_records
            .values()
            .filter(|record| {
                record.originating_run_unit == run_unit
                    && record.state == IntervalStartState::Pending
            })
            .cloned()
            .collect::<Vec<_>>();
        (released, pending)
    };
    if pending.is_empty() {
        return Ok(());
    }
    let work_store = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    for record in pending {
        let work_id = format!("cics-start:{}", record.request_id);
        if released.contains(&record.request_id)
            || work_store
                .get_work(&work_id)
                .map_err(store_error)?
                .is_none()
        {
            service.enqueue_interval_work(&record, run.invocation.priority)?;
        }
    }
    Ok(())
}

pub(super) fn release(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    run_unit: &str,
    limits: CicsLimits,
) -> Result<BTreeSet<String>, HostProblem> {
    let selected = records
        .values()
        .filter(|record| {
            record.state == IntervalStartState::ProtectedPending
                && record.originating_run_unit == run_unit
        })
        .map(|record| record.request_id.clone())
        .collect::<BTreeSet<_>>();
    for request_id in &selected {
        replace_state(
            store,
            records,
            request_id,
            IntervalStartState::Pending,
            None,
            None,
            limits,
        )?;
    }
    Ok(selected)
}

fn discard(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let run_unit = run.invocation.run_unit_id.as_str();
    let mut state = service.lock()?;
    let selected = state
        .interval_records
        .values()
        .filter(|record| {
            record.state == IntervalStartState::ProtectedPending
                && record.originating_run_unit == run_unit
        })
        .map(|record| (record.request_id.clone(), record.version))
        .collect::<Vec<_>>();
    for (request_id, version) in selected {
        service
            .store
            .delete_provider_state(NAMESPACE, &request_id, version)
            .map_err(store_error)?;
        state.interval_records.remove(&request_id);
    }
    Ok(())
}
