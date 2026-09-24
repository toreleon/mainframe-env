//! Trusted partner ABEND/PREPARE ingress on the shared conversation ledger.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, GdsIssueFlow, load_conversation_replay,
};
use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{AccessIntent, HostProblem};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const EVENT_PREFIX: &str = "peer-issue:";

impl CicsService {
    /// Persist a partner control before a carrier confirms its source ISSUE.
    /// The target run's owner lease and CONNECTION SAF check fence this event;
    /// a repeated event ID with different content never changes the ledger.
    pub fn accept_conversation_peer_issue(
        &self,
        run_unit: &RunUnitId,
        token: [u8; 4],
        flow: GdsIssueFlow,
        event_id: &str,
    ) -> Result<(), HostProblem> {
        if !matches!(flow, GdsIssueFlow::Abend | GdsIssueFlow::Prepare)
            || event_id.is_empty()
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
        let effect_key = format!("{EVENT_PREFIX}{event_id}");
        let digest = request_digest(&owner, token, flow, event_id)?;
        let initial = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let record = initial.conversation(token).ok_or(HostProblem::NotFound)?;
        if record.owner != owner
            || record.principal_facility && context == super::ConversationContext::DplServer
        {
            return Err(HostProblem::Unauthorized);
        }
        if !matches!(
            record.kind,
            ConversationKind::AppcMapped | ConversationKind::AppcBasic
        ) {
            return Err(HostProblem::Unsupported);
        }
        let system = record.system.clone();
        self.authorize(
            run,
            "CONNECTION",
            &format!("CICS.CONNECTION.{system}"),
            AccessIntent::Execute,
        )?;
        for _ in 0..MAX_CAS_RETRIES {
            super::deadline(self, run)?;
            if let Some(saved) =
                load_conversation_replay(self.store.as_ref(), &effect_key).map_err(store_error)?
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
                .accept_peer_issue(&owner, context, flow)
                .map_err(map_problem)?;
            let replay = ConversationReplay {
                schema_version: 1,
                effect_key: effect_key.clone(),
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
                    state: Some(record.state),
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

fn request_digest(
    owner: &ConversationOwner,
    token: [u8; 4],
    flow: GdsIssueFlow,
    event_id: &str,
) -> Result<[u8; 32], HostProblem> {
    let body =
        serde_json::to_vec(&(owner, token, flow, event_id)).map_err(|_| HostProblem::Malformed)?;
    Ok(Sha256::digest(body).into())
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
