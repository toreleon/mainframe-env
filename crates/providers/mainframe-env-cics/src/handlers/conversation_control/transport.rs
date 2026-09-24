//! Confirmed transport boundary for staged APPC/MRO data.
//!
//! A carrier returns a confirmed transmission only after its own durable
//! outcome is known. A caller must persist `mark_send_attempted` before
//! `transmit`; after any uncertain return it uses `reconcile` by send ID and
//! never repeats `transmit` for that ID.

use super::ConversationDataFrame;
use super::data::MAX_FRAMES;
use super::{ConversationLedger, ConversationOwner, ConversationProblem};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::HostProblem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationTransmitOutcome {
    Confirmed,
    Pending,
}

pub trait CicsConversationTransport: Send + Sync {
    fn transmit(
        &self,
        system: &str,
        token: [u8; 4],
        send_id: u64,
        frame: &ConversationDataFrame,
        invocation: &Invocation,
    ) -> Result<ConversationTransmitOutcome, HostProblem>;

    /// This must be a read-only carrier outcome query. The default preserves
    /// uncertainty when no reconciliation endpoint is available.
    fn reconcile(
        &self,
        _system: &str,
        _token: [u8; 4],
        _send_id: u64,
        _invocation: &Invocation,
    ) -> Result<ConversationTransmitOutcome, HostProblem> {
        Err(HostProblem::UnknownOutcome)
    }
}

impl CicsService {
    /// Advance one staged frame through a durable pre-dispatch marker and
    /// carrier-confirmed acknowledgement. An attempted frame is reconciled
    /// by ID on every subsequent call; it is never transmitted a second time.
    pub(in crate::service) fn flush_conversation_send(
        &self,
        run: &Run,
        token: [u8; 4],
    ) -> Result<ConversationTransmitOutcome, HostProblem> {
        let owner = ConversationOwner {
            execution: run.invocation.execution_id.as_str().into(),
            run_unit: run.invocation.run_unit_id.as_str().into(),
            lease_epoch: u64::from(run.invocation.attempt),
        };
        let context = super::context(run)?;
        for _ in 0..=MAX_FRAMES {
            super::deadline(self, run)?;
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let record = current
                .conversation(token)
                .ok_or_else(|| problem(ConversationProblem::NotOwned))?;
            record.check_owner(&owner, context).map_err(problem)?;
            let Some((send_id, frame, attempted)) = record.data.next_outbound() else {
                return Ok(ConversationTransmitOutcome::Confirmed);
            };
            let Some(transport) = self.conversation_transport()? else {
                return Ok(ConversationTransmitOutcome::Pending);
            };
            let frame = frame.clone();
            let system = record.system.clone();
            if !attempted {
                let mut next = current.clone();
                next.conversation_mut(token)
                    .ok_or(HostProblem::InfrastructureFailure)?
                    .mark_send_attempted(&owner, context, send_id)
                    .map_err(problem)?;
                if !current
                    .persist(&mut next, self.store.as_ref())
                    .map_err(|error| mutation_problem(store_error(error)))?
                {
                    continue;
                }
            }
            if run.invocation.cancellation_requested() || super::deadline(self, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            let outcome = if attempted {
                transport.reconcile(&system, token, send_id, &run.invocation)
            } else {
                transport.transmit(&system, token, send_id, &frame, &run.invocation)
            }
            .map_err(|_| HostProblem::UnknownOutcome)?;
            if outcome == ConversationTransmitOutcome::Pending {
                return Ok(outcome);
            }
            let mut acknowledged = false;
            for _ in 0..32 {
                let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
                let mut next = current.clone();
                let record = next
                    .conversation_mut(token)
                    .ok_or(HostProblem::UnknownOutcome)?;
                record
                    .acknowledge_send(&owner, context, send_id)
                    .map_err(problem)?;
                if current
                    .persist(&mut next, self.store.as_ref())
                    .map_err(|error| mutation_problem(store_error(error)))?
                {
                    if run.invocation.cancellation_requested()
                        || super::deadline(self, run).is_err()
                    {
                        return Err(HostProblem::UnknownOutcome);
                    }
                    acknowledged = true;
                    break;
                }
            }
            if !acknowledged {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

fn problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned => HostProblem::Condition {
            name: "NOTALLOC".into(),
            response: 61,
            response2: 0,
        },
        ConversationProblem::DplPrincipal => HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 200,
        },
        ConversationProblem::WrongKind | ConversationProblem::WrongState => {
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 0,
            }
        }
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        ConversationProblem::Length => HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        },
        ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
        ConversationProblem::Malformed => HostProblem::InfrastructureFailure,
    }
}
