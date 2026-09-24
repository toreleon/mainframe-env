//! Trusted peer-completion hook for APPC basic conversations.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationReplay,
    ConversationReply, ConversationState, load_conversation_replay,
};
use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{AccessIntent, HostProblem};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const EVENT_PREFIX: &str = "peer-free:";

impl CicsService {
    /// Trusted APPC transport reports peer completion. This is not a CICS
    /// command or a delivery acknowledgement; it is a protocol-state event.
    pub fn record_conversation_peer_finished(
        &self,
        run_unit: &RunUnitId,
        token: [u8; 4],
        event_id: &str,
    ) -> Result<(), HostProblem> {
        if event_id.is_empty()
            || event_id.len() > 128
            || !event_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(HostProblem::Malformed);
        }
        let mut state = self.lock()?;
        let run = state.runs.get_mut(run_unit).ok_or(HostProblem::NotFound)?;
        super::deadline(self, run)?;
        let owner = ConversationOwner {
            execution: run.invocation.execution_id.as_str().into(),
            run_unit: run.invocation.run_unit_id.as_str().into(),
            lease_epoch: u64::from(run.invocation.attempt),
        };
        let principal = run.invocation.principal.id().as_str().to_owned();
        let context = super::context(run)?;
        let key = format!("{EVENT_PREFIX}{event_id}");
        let digest = event_digest(&owner, token, event_id);
        let initial = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let system = initial
            .conversation(token)
            .ok_or(HostProblem::NotFound)?
            .system
            .clone();
        self.authorize(
            run,
            "CONNECTION",
            &format!("CICS.CONNECTION.{system}"),
            AccessIntent::Execute,
        )?;
        for _ in 0..MAX_CAS_RETRIES {
            super::deadline(self, run)?;
            if let Some(saved) =
                load_conversation_replay(self.store.as_ref(), &key).map_err(store_error)?
            {
                saved
                    .matches_request(
                        &owner.execution,
                        &owner.run_unit,
                        &principal,
                        owner.lease_epoch,
                        1,
                        digest,
                    )
                    .map_err(store_error)?;
                return Ok(());
            }
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            let record = next.conversation_mut(token).ok_or(HostProblem::NotFound)?;
            if record.system != system {
                return Err(HostProblem::IdempotencyConflict);
            }
            record
                .peer_finished(&owner, context)
                .map_err(|problem| match problem {
                    ConversationProblem::StaleOwner | ConversationProblem::NotOwned => {
                        HostProblem::IdempotencyConflict
                    }
                    ConversationProblem::DplPrincipal | ConversationProblem::WrongState => {
                        HostProblem::Unsupported
                    }
                    _ => HostProblem::Malformed,
                })?;
            let replay = ConversationReplay {
                schema_version: 1,
                effect_key: key.clone(),
                owner_execution: owner.execution.clone(),
                owner_run_unit: owner.run_unit.clone(),
                owner_principal: principal.clone(),
                owner_epoch: owner.lease_epoch,
                mutation_sequence: 1,
                request_digest: digest,
                deadline_tick: run.invocation.deadline_tick,
                retain_until_tick: run.invocation.deadline_tick,
                reply: ConversationReply {
                    condition: "NORMAL".into(),
                    response: 0,
                    response2: 0,
                    state: Some(ConversationState::Free),
                    token: Some(token),
                    outputs: BTreeMap::new(),
                },
            };
            if current
                .persist_with_replay(&mut next, &replay, self.store.as_ref())
                .map_err(store_error)?
            {
                if super::deadline(self, run).is_err() {
                    return Err(HostProblem::UnknownOutcome);
                }
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

fn event_digest(owner: &ConversationOwner, token: [u8; 4], event_id: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in [
        owner.execution.as_bytes(),
        owner.run_unit.as_bytes(),
        token.as_slice(),
        event_id.as_bytes(),
    ] {
        hash.update((part.len() as u32).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}
