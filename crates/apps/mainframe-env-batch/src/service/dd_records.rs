//! Batch host invocation adapter for complete bounded dataset reads.

use super::*;

impl BatchService {
    pub(super) fn read_repro_dataset_records(
        &self,
        invocation: &(impl RunInput + ?Sized),
        dataset: &DatasetName,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<crate::dd_hydration::CompleteDatasetRead, HostProblem> {
        if operand(statement, &["COUNT"]).is_some() {
            // Explicit COUNT retains the original single-read selection. It
            // does not claim that this unlocked page is the complete dataset.
            let DatasetResult::Records { records, .. } = self.ams_dataset_read(
                invocation,
                effect_sequence,
                DatasetRequest::Read {
                    dataset: dataset.clone(),
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                },
            )?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            Ok(crate::dd_hydration::CompleteDatasetRead::Records(records))
        } else {
            self.read_dataset_records(invocation, None, dataset, None, effect_sequence)
        }
    }

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
