//! Batch host invocation adapter for complete bounded dataset reads.

use super::*;

impl BatchService {
    pub(super) fn read_dataset_records(
        &self,
        invocation: &(impl RunInput + ?Sized),
        allocation: Option<&DdRuntimeAllocation>,
        dataset: &DatasetName,
        member: Option<&MemberName>,
        effect_sequence: &mut u64,
    ) -> Result<crate::dd_hydration::CompleteDatasetRead, HostProblem> {
        crate::dd_hydration::complete_dataset_records(
            dataset,
            member,
            allocation.is_some_and(|allocation| allocation.lock_id.is_some()),
            invocation.original().limits.max_storage_bytes,
            || invocation.check().map(|_| ()),
            |request| {
                let sequence = next_effect_sequence(invocation, effect_sequence)?;
                let result = self.invoke_host(
                    invocation,
                    invocation.original().deadline_tick.saturating_sub(1),
                    false,
                    EffectRequest {
                        run_unit: invocation.original().run_unit_id.clone(),
                        sequence,
                        deadline_tick: invocation.original().deadline_tick,
                        idempotency_key: None,
                        request: HostRequest::Dataset(request),
                    },
                );
                let HostResult::Dataset(result) = result.outcome? else {
                    return Err(HostProblem::ProviderFailure);
                };
                Ok(result)
            },
        )
    }
}
