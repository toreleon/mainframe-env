//! Explicit live z/OS full-message backout policy, not cold recovery admission.
//!
//! ibm-mq-9.4-programming-supplements-2026-09-12 q097395_ lines1498–1508:
//! a removed syncpoint GET subsequently backed out increments BackoutCount;
//! browse is excluded and z/OS saturates at255. The unchanged storage-only
//! backout/cold projections do not silently acquire this policy. The owning
//! selected operation must independently admit its context, unit and decision.

use super::*;
use mainframe_env_host_api::mq_md_value::MqMdValue;

fn count_mut(message: &mut mainframe_env_host_api::mq_mqi::MqFullMessage) -> &mut i32 {
    match &mut message.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => &mut fields.backout_count,
    }
}

impl MqDeliveryKernel {
    /// Explicit reviewed live backout candidate. Only complete payloads actually
    /// removed into this unit change; partial payload policy/bytes stay exact.
    /// This is neither a final-task-end permit nor HardenGetBackout crash policy.
    /// One existing atomic candidate restores entries/discards puts/finalizes;
    /// failure leaves every field unchanged and repeated decisions never recount.
    pub(crate) fn backout_complete_zos(
        &mut self,
        unit: u64,
    ) -> Result<MqDeliveryOutcome, MqDeliveryError> {
        if unit == 0 {
            return Err(MqDeliveryError::InvalidUnit);
        }
        if self.finalized.contains_key(&unit) || !self.pending.contains_key(&unit) {
            return self.backout(unit);
        }
        // Check the entire unit before any candidate work. Signed diagnostic
        // observations remain valid storage values but not native z/OS counters.
        for operation in &self.pending[&unit] {
            if let Pending::Get {
                entry:
                    Entry {
                        message: Payload::Complete(message),
                        ..
                    },
                ..
            } = operation
                && !(0..=255).contains(&message.descriptor.fields().backout_count)
            {
                return Err(MqDeliveryError::Unsupported);
            }
        }
        let mut next = self.clone();
        for operation in next.pending.get_mut(&unit).expect("checked unit") {
            if let Pending::Get {
                entry:
                    Entry {
                        message: Payload::Complete(message),
                        ..
                    },
                ..
            } = operation
            {
                let count = count_mut(message);
                *count = (*count + 1).min(255);
            }
        }
        next.apply_backout(unit)?;
        next.check_bounds()?;
        if next.schema_two {
            next.encode_live_checkpoint()?;
        }
        *self = next;
        Ok(MqDeliveryOutcome::Rejected)
    }
}

#[cfg(test)]
mod tests;
