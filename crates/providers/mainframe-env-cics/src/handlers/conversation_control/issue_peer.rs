//! Trusted partner ISSUE ingress on the shared conversation ledger.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, GdsIssueFailure, GdsIssueFlow, IssueValidationProblem,
    load_conversation_replay,
};
use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{AccessIntent, HostProblem};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const EVENT_PREFIX: &str = "peer-issue:";
const BASIC_RESPONSE_PREFIX: &str = "peer-basic-response:";

impl CicsService {
    /// Consume one trusted response to a carrier-confirmed basic SEND CONFIRM.
    /// A separate send ID fences responses for an older send on the same CONVID.
    pub fn accept_conversation_peer_basic_response(
        &self,
        run_unit: &RunUnitId,
        token: [u8; 4],
        send_id: u64,
        flow: GdsIssueFlow,
        event_id: &str,
    ) -> Result<ConversationReply, HostProblem> {
        if !matches!(flow, GdsIssueFlow::Confirmation | GdsIssueFlow::Error)
            || send_id == 0
            || !valid_event_id(event_id)
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
        let effect_key = format!("{BASIC_RESPONSE_PREFIX}{event_id}");
        let digest = Sha256::digest(
            serde_json::to_vec(&(&owner, token, send_id, flow, event_id))
                .map_err(|_| HostProblem::Malformed)?,
        )
        .into();
        let initial = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let record = initial.conversation(token).ok_or(HostProblem::NotFound)?;
        record.check_owner(&owner, context).map_err(map_problem)?;
        if record.kind != ConversationKind::AppcBasic {
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
                return Ok(saved.reply);
            }
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            let record = next.conversation_mut(token).ok_or(HostProblem::NotFound)?;
            if record.system != system {
                return Err(HostProblem::IdempotencyConflict);
            }
            record.check_owner(&owner, context).map_err(map_problem)?;
            if record.kind != ConversationKind::AppcBasic {
                return Err(HostProblem::Unsupported);
            }
            let result = record.accept_basic_peer_response(&owner, context, send_id, flow);
            let failure = match result {
                Ok(()) => None,
                Err(IssueValidationProblem::WrongSyncLevel) => {
                    Some(GdsIssueFailure::WrongSyncLevel)
                }
                Err(IssueValidationProblem::Protocol(ConversationProblem::WrongState)) => {
                    Some(GdsIssueFailure::StateCheck)
                }
                Err(IssueValidationProblem::Protocol(problem)) => {
                    return Err(map_problem(problem));
                }
            };
            let mut convdata = record.gds_convdata(false).map_err(map_problem)?;
            if failure.is_none() {
                convdata[if flow == GdsIssueFlow::Confirmation {
                    5
                } else {
                    6
                }] = 0xff;
            }
            let reply = ConversationReply {
                condition: "NORMAL".into(),
                response: 0,
                response2: 0,
                state: Some(record.state),
                token: Some(token),
                outputs: BTreeMap::from([
                    (
                        "RETCODE".into(),
                        failure
                            .map_or([0; 6], |failure| failure.retcode(flow).0)
                            .to_vec(),
                    ),
                    ("CONVDATA".into(), convdata.to_vec()),
                    ("STATE".into(), super::state_cvda::bytes(record.state)),
                ]),
            };
            if failure.is_some() {
                return Ok(reply);
            }
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
                reply: reply.clone(),
            };
            if current
                .persist_with_replay(&mut next, &replay, self.store.as_ref())
                .map_err(store_error)?
            {
                if super::deadline(self, run).is_err() {
                    return Err(HostProblem::UnknownOutcome);
                }
                return Ok(reply);
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

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
        if !matches!(flow, GdsIssueFlow::Abend | GdsIssueFlow::Prepare) || !valid_event_id(event_id)
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

fn valid_event_id(event_id: &str) -> bool {
    !event_id.is_empty()
        && event_id.len() <= 128
        && event_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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
