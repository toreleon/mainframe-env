//! Trusted peer-completion hook for APPC basic conversations.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationPeerFrame,
    ConversationProblem, ConversationReplay, ConversationReply, ConversationState,
    load_conversation_replay,
};
use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{AccessIntent, HostProblem};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const EVENT_PREFIX: &str = "peer-free:";
const FRAME_PREFIX: &str = "peer-frame:";

impl CicsService {
    /// Queue one explicit APPC/MRO peer frame for a later CONVERSE. The
    /// frame and its receipt are committed together; no default peer exists.
    pub fn offer_conversation_peer_frame(
        &self,
        run_unit: &RunUnitId,
        token: [u8; 4],
        frame: ConversationPeerFrame,
        event_id: &str,
    ) -> Result<(), HostProblem> {
        if !valid_event_id(event_id) || frame.validate().is_err() {
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
        let key = format!("{FRAME_PREFIX}{event_id}");
        let digest = frame_digest(&owner, token, &frame, event_id)?;
        let initial = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let record = initial.conversation(token).ok_or(HostProblem::NotFound)?;
        record
            .check_owner(&owner, context)
            .map_err(|_| HostProblem::Unsupported)?;
        if matches!(
            record.kind,
            ConversationKind::AppcBasic | ConversationKind::LuType61
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
            let record = next.conversation(token).ok_or(HostProblem::NotFound)?;
            record
                .check_owner(&owner, context)
                .map_err(|_| HostProblem::Unsupported)?;
            if record.system != system
                || matches!(
                    record.kind,
                    ConversationKind::AppcBasic | ConversationKind::LuType61
                )
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            next.offer_peer_frame(token, frame.clone())
                .map_err(|problem| match problem {
                    ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
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
                    state: None,
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

fn valid_event_id(event_id: &str) -> bool {
    !event_id.is_empty()
        && event_id.len() <= 128
        && event_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn frame_digest(
    owner: &ConversationOwner,
    token: [u8; 4],
    frame: &ConversationPeerFrame,
    event_id: &str,
) -> Result<[u8; 32], HostProblem> {
    let body = serde_json::to_vec(frame).map_err(|_| HostProblem::Malformed)?;
    let mut hash = Sha256::new();
    for part in [
        owner.execution.as_bytes(),
        owner.run_unit.as_bytes(),
        token.as_slice(),
        event_id.as_bytes(),
        body.as_slice(),
    ] {
        hash.update((part.len() as u32).to_be_bytes());
        hash.update(part);
    }
    Ok(hash.finalize().into())
}
