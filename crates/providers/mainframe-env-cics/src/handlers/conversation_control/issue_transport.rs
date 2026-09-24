//! Dispatch staged ISSUE controls through the confirmed conversation carrier.
//!
//! The dispatch marker is durable before I/O. A resumed attempt only asks the
//! carrier for its prior result by control ID; it never sends the control twice.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationTransmitOutcome,
};
use crate::service::{CicsService, Run, store_error};
use mainframe_env_host_api::HostProblem;

impl CicsService {
    pub(in crate::service) fn flush_issue_control(
        &self,
        run: &mut Run,
        token: [u8; 4],
        effect_key: &str,
        control_id: u64,
    ) -> Result<ConversationTransmitOutcome, HostProblem> {
        super::deadline(self, run)?;
        let owner = ConversationOwner {
            execution: run.invocation.execution_id.as_str().into(),
            run_unit: run.invocation.run_unit_id.as_str().into(),
            lease_epoch: u64::from(run.invocation.attempt),
        };
        let context = super::context(run)?;
        let ledger = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let record = ledger.conversation(token).ok_or(HostProblem::NotFound)?;
        record.check_owner(&owner, context).map_err(map_problem)?;
        let pending = record
            .pending_issue
            .as_ref()
            .ok_or(HostProblem::IdempotencyConflict)?;
        if pending.id != control_id || pending.effect_key != effect_key {
            return Err(HostProblem::IdempotencyConflict);
        }
        let Some(transport) = self.conversation_transport()? else {
            return Ok(ConversationTransmitOutcome::Pending);
        };
        let attempted = pending.attempted;
        let system = record.system.clone();
        let flow = pending.flow;
        if !attempted {
            super::issue_staging::mark_attempted(self, run, token, effect_key, control_id)?;
        }
        if run.invocation.cancellation_requested() || super::deadline(self, run).is_err() {
            return Err(HostProblem::UnknownOutcome);
        }
        let outcome = if attempted {
            transport.reconcile_issue(&system, token, control_id, &run.invocation)
        } else {
            transport.transmit_issue(&system, token, control_id, flow, &run.invocation)
        }
        .map_err(|_| HostProblem::UnknownOutcome)?;
        match outcome {
            ConversationTransmitOutcome::Confirmed => {
                super::issue_staging::confirm(self, run, token, effect_key, control_id)?;
                Ok(ConversationTransmitOutcome::Confirmed)
            }
            ConversationTransmitOutcome::Pending => Ok(ConversationTransmitOutcome::Pending),
            // A negative carrier result needs a source-specific ISSUE failure
            // reply. Preserve the attempted intent until that path is bound.
            ConversationTransmitOutcome::Rejected(_) => Err(HostProblem::UnknownOutcome),
        }
    }
}

fn map_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned
        | ConversationProblem::StaleOwner
        | ConversationProblem::DplPrincipal => HostProblem::Unauthorized,
        ConversationProblem::WrongKind | ConversationProblem::WrongState => {
            HostProblem::IdempotencyConflict
        }
        ConversationProblem::Malformed | ConversationProblem::Length => HostProblem::Malformed,
        ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
    }
}
