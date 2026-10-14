//! Whole-dataset DD input reads and their aggregate record budget.
use super::*;

pub(super) const MAX_DATASET_INPUT_RECORDS: u32 = 4_096;

pub(super) fn validate_record_count(count: usize) -> Result<(), HostProblem> {
    if count > MAX_DATASET_INPUT_RECORDS as usize {
        return Err(HostProblem::Condition {
            name: "DATASET_READ_TRUNCATED".into(),
            response: 16,
            response2: 0,
        });
    }
    Ok(())
}

impl BatchService {
    pub(super) fn read_dataset_records(
        &self,
        invocation: &(impl RunInput + ?Sized),
        allocation: &DdRuntimeAllocation,
        effect_sequence: &mut u64,
    ) -> Result<Vec<Vec<u8>>, HostProblem> {
        invocation.check()?;
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
                request: HostRequest::Dataset(DatasetRequest::Read {
                    dataset: allocation.dataset.clone(),
                    member: allocation.member.clone(),
                    key: None,
                    max_records: MAX_DATASET_INPUT_RECORDS,
                    control: Default::default(),
                }),
            },
        );
        let HostResult::Dataset(DatasetResult::Records { records, .. }) = result.outcome? else {
            return Err(HostProblem::ProviderFailure);
        };
        Ok(records)
    }
}
