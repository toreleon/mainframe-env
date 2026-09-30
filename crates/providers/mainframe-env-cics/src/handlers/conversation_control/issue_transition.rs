//! Guard ISSUE controls against the shared APPC allocation before dispatch.
//!
//! Staging and dispatch markers retain the current protocol state. Only a
//! confirmed partner result applies the source transition in the same ledger.

use super::{
    CONVERSATION_RECORD_VERSION, ConversationContext, ConversationKind, ConversationOwner,
    ConversationProblem, ConversationRecord, ConversationState, GdsIssueFlow,
};
use serde::{Deserialize, Serialize};

const MAX_ISSUE_EFFECT_KEY: usize = 256;

/// One control intent retained in the allocated conversation's ledger row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssuePendingControl {
    pub flow: GdsIssueFlow,
    pub effect_key: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<IssueRequestIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueRequestIdentity {
    pub principal: String,
    pub mutation_sequence: u64,
    pub digest: [u8; 32],
    pub deadline_tick: u64,
    pub retain_until_tick: u64,
    pub state_output: bool,
    pub convdata_output: bool,
    pub retcode_output: bool,
}

impl IssueRequestIdentity {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.principal.is_empty()
            || self.principal.len() > 128
            || self.principal.contains('\0')
            || self.mutation_sequence == 0
            || self.deadline_tick == 0
            || self.retain_until_tick < self.deadline_tick
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

impl IssuePendingControl {
    pub fn matches_request(
        &self,
        effect_key: &str,
        request: &IssueRequestIdentity,
    ) -> Result<(), ConversationProblem> {
        if self.effect_key != effect_key || self.request.as_ref() != Some(request) {
            return Err(ConversationProblem::StaleOwner);
        }
        Ok(())
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueValidationProblem {
    Protocol(ConversationProblem),
    WrongSyncLevel,
}

impl ConversationRecord {
    pub(super) fn validate_pending_issue(&self) -> Result<(), ConversationProblem> {
        let Some(pending) = &self.pending_issue else {
            return Ok(());
        };
        if self.version == 1
            || self.released
            || !matches!(
                self.kind,
                ConversationKind::AppcMapped | ConversationKind::AppcBasic
            )
            || pending.effect_key.is_empty()
            || pending.effect_key.len() > MAX_ISSUE_EFFECT_KEY
            || pending.effect_key.contains('\0')
            || pending.id == 0
            || pending.id > self.sequence
            || pending.request.as_ref().is_some_and(|request| {
                request.validate().is_err()
                    || request.convdata_output && self.kind != ConversationKind::AppcBasic
                    || request.retcode_output && self.kind != ConversationKind::AppcBasic
            })
            || !issue_state_valid(self, pending.flow)
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }

    pub fn check_issue(
        &self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        flow: GdsIssueFlow,
    ) -> Result<(), IssueValidationProblem> {
        self.check_owner(owner, context)
            .map_err(IssueValidationProblem::Protocol)?;
        let expected = if basic {
            ConversationKind::AppcBasic
        } else {
            ConversationKind::AppcMapped
        };
        if self.kind != expected {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongKind,
            ));
        }
        if self.pending_issue.is_some() {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState,
            ));
        }
        if flow == GdsIssueFlow::Prepare && self.sync_level != Some(2)
            || flow == GdsIssueFlow::Confirmation && self.sync_level == Some(0)
        {
            return Err(IssueValidationProblem::WrongSyncLevel);
        }
        if !issue_state_valid(self, flow) {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState,
            ));
        }
        Ok(())
    }

    /// Stage a control in the shared record without implying partner delivery.
    pub fn stage_issue(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        flow: GdsIssueFlow,
        effect_key: &str,
    ) -> Result<u64, IssueValidationProblem> {
        self.check_issue(owner, context, basic, flow)?;
        if effect_key.is_empty()
            || effect_key.len() > MAX_ISSUE_EFFECT_KEY
            || effect_key.contains('\0')
        {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::Malformed,
            ));
        }
        let migrated_profile =
            (self.version == 1).then(|| self.effective_processing_profile().to_owned());
        self.next_sequence()
            .map_err(IssueValidationProblem::Protocol)?;
        if let Some(profile) = migrated_profile {
            self.version = CONVERSATION_RECORD_VERSION;
            self.processing_profile = Some(profile);
        }
        let id = self.sequence;
        self.pending_issue = Some(IssuePendingControl {
            flow,
            effect_key: effect_key.into(),
            id,
            attempted: false,
            request: None,
        });
        Ok(id)
    }

    pub fn stage_issue_request(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        flow: GdsIssueFlow,
        effect_key: &str,
        request: IssueRequestIdentity,
    ) -> Result<u64, IssueValidationProblem> {
        request
            .validate()
            .map_err(IssueValidationProblem::Protocol)?;
        if (request.convdata_output || request.retcode_output) && !basic {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::Malformed,
            ));
        }
        let id = self.stage_issue(owner, context, basic, flow, effect_key)?;
        self.pending_issue
            .as_mut()
            .ok_or(IssueValidationProblem::Protocol(
                ConversationProblem::Malformed,
            ))?
            .request = Some(request);
        Ok(id)
    }

    /// Persist this marker before a carrier transmits the staged control.
    pub fn mark_issue_attempted(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        effect_key: &str,
        id: u64,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        let pending = self
            .pending_issue
            .as_ref()
            .ok_or(ConversationProblem::WrongState)?;
        if pending.effect_key != effect_key || pending.id != id {
            return Err(ConversationProblem::StaleOwner);
        }
        if !pending.attempted {
            self.next_sequence()?;
            self.pending_issue
                .as_mut()
                .ok_or(ConversationProblem::Malformed)?
                .attempted = true;
        }
        Ok(())
    }

    /// Apply a confirmed partner control; an attempted send is not an ack.
    pub fn confirm_issue(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        effect_key: &str,
        id: u64,
    ) -> Result<ConversationState, ConversationProblem> {
        self.check_owner(owner, context)?;
        let pending = self
            .pending_issue
            .as_ref()
            .ok_or(ConversationProblem::WrongState)?;
        if pending.effect_key != effect_key || pending.id != id {
            return Err(ConversationProblem::StaleOwner);
        }
        if !pending.attempted || !issue_state_valid(self, pending.flow) {
            return Err(ConversationProblem::WrongState);
        }
        let next_state = match pending.flow {
            GdsIssueFlow::Abend => ConversationState::Free,
            GdsIssueFlow::Confirmation => match self.state {
                ConversationState::ConfReceive => ConversationState::Receive,
                ConversationState::ConfSend => ConversationState::Send,
                ConversationState::ConfFree => ConversationState::Free,
                _ => return Err(ConversationProblem::WrongState),
            },
            // The peer-response contract has no EIBFREE/CDBFREE indicator. Only the
            // normal outcome is reachable; indicated deallocation needs its own input.
            GdsIssueFlow::Error => ConversationState::Send,
            GdsIssueFlow::Signal => self.state,
            GdsIssueFlow::Prepare => match self.state {
                ConversationState::Send => ConversationState::SyncSend,
                ConversationState::PendReceive => ConversationState::SyncReceive,
                ConversationState::PendFree => ConversationState::SyncFree,
                _ => return Err(ConversationProblem::WrongState),
            },
        };
        let release = pending.flow == GdsIssueFlow::Abend || next_state == ConversationState::Free;
        self.next_sequence()?;
        self.state = next_state;
        self.released = release;
        self.pending_issue = None;
        Ok(next_state)
    }
}

fn issue_state_valid(record: &ConversationRecord, flow: GdsIssueFlow) -> bool {
    match flow {
        GdsIssueFlow::Abend => matches!(
            (record.sync_level, record.state),
            (
                Some(0..=2),
                ConversationState::Send
                    | ConversationState::PendReceive
                    | ConversationState::PendFree
                    | ConversationState::Receive
            ) | (
                Some(1..=2),
                ConversationState::ConfReceive
                    | ConversationState::ConfSend
                    | ConversationState::ConfFree
            ) | (
                Some(2),
                ConversationState::SyncReceive
                    | ConversationState::SyncSend
                    | ConversationState::SyncFree
            )
        ),
        GdsIssueFlow::Confirmation => matches!(
            record.state,
            ConversationState::ConfReceive
                | ConversationState::ConfSend
                | ConversationState::ConfFree
        ),
        GdsIssueFlow::Error => matches!(
            (record.sync_level, record.state),
            (
                Some(0..=2),
                ConversationState::Send
                    | ConversationState::PendReceive
                    | ConversationState::Receive
            ) | (
                Some(1..=2),
                ConversationState::ConfReceive
                    | ConversationState::ConfSend
                    | ConversationState::ConfFree
            ) | (
                Some(2),
                ConversationState::SyncReceive
                    | ConversationState::SyncSend
                    | ConversationState::SyncFree
            )
        ),
        GdsIssueFlow::Prepare => {
            record.sync_level == Some(2)
                && matches!(
                    record.state,
                    ConversationState::Send
                        | ConversationState::PendReceive
                        | ConversationState::PendFree
                )
        }
        GdsIssueFlow::Signal => matches!(
            (record.sync_level, record.state),
            (
                Some(0..=2),
                ConversationState::Send
                    | ConversationState::PendReceive
                    | ConversationState::Receive
            ) | (
                Some(1..=2),
                ConversationState::ConfReceive
                    | ConversationState::ConfSend
                    | ConversationState::ConfFree
            ) | (
                Some(2),
                ConversationState::SyncReceive
                    | ConversationState::SyncSend
                    | ConversationState::SyncFree
            )
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ConversationLedger, ConversationSystemDefinition};
    use super::*;
    use mainframe_env_store::{PostgresStateStore, SqliteStateStore};
    use std::sync::{Arc, Barrier};

    fn owner() -> ConversationOwner {
        ConversationOwner {
            execution: "execution".into(),
            run_unit: "run".into(),
            lease_epoch: 1,
        }
    }

    fn connected(kind: ConversationKind, sync_level: u8) -> ConversationRecord {
        let mut record =
            ConversationRecord::allocate([0, 0, 0, 1], "S001", kind, owner(), false).unwrap();
        record
            .connect(
                &owner(),
                ConversationContext::Local,
                kind == ConversationKind::AppcBasic,
                b"PROGRAM".to_vec(),
                Vec::new(),
                sync_level,
            )
            .unwrap();
        record
    }

    #[test]
    fn issue_controls_follow_basic_and_mapped_state_tables() {
        use ConversationState::*;
        const STATES: [ConversationState; 13] = [
            Allocated,
            Send,
            PendReceive,
            PendFree,
            Receive,
            ConfReceive,
            ConfSend,
            ConfFree,
            SyncReceive,
            SyncSend,
            SyncFree,
            Free,
            Rollback,
        ];
        for kind in [ConversationKind::AppcBasic, ConversationKind::AppcMapped] {
            for level in 0..=2 {
                for (index, state) in STATES.into_iter().enumerate() {
                    let number = index + 1;
                    for flow in [
                        GdsIssueFlow::Abend,
                        GdsIssueFlow::Signal,
                        GdsIssueFlow::Prepare,
                        GdsIssueFlow::Confirmation,
                        GdsIssueFlow::Error,
                    ] {
                        let expected = match flow {
                            GdsIssueFlow::Abend
                                if (2..=5).contains(&number)
                                    || level >= 1 && (6..=8).contains(&number)
                                    || level == 2 && (9..=11).contains(&number) =>
                            {
                                Some(Free)
                            }
                            GdsIssueFlow::Signal
                                if [2, 3, 5].contains(&number)
                                    || level >= 1 && (6..=8).contains(&number)
                                    || level == 2 && (9..=11).contains(&number) =>
                            {
                                Some(state)
                            }
                            GdsIssueFlow::Prepare if level == 2 => match state {
                                Send => Some(SyncSend),
                                PendReceive => Some(SyncReceive),
                                PendFree => Some(SyncFree),
                                _ => None,
                            },
                            GdsIssueFlow::Confirmation if level >= 1 => match state {
                                ConfReceive => Some(Receive),
                                ConfSend => Some(Send),
                                ConfFree => Some(Free),
                                _ => None,
                            },
                            GdsIssueFlow::Error
                                if [Send, PendReceive, Receive].contains(&state)
                                    || level >= 1
                                        && [ConfReceive, ConfSend, ConfFree].contains(&state)
                                    || level == 2
                                        && [SyncReceive, SyncSend, SyncFree].contains(&state) =>
                            {
                                Some(Send)
                            }
                            _ => None,
                        };
                        let mut record = connected(kind, level);
                        record.state = state;
                        let before = record.clone();
                        let result = record.stage_issue(
                            &owner(),
                            ConversationContext::Local,
                            kind == ConversationKind::AppcBasic,
                            flow,
                            "table-cell",
                        );
                        if let Some(next) = expected {
                            let id = result.unwrap_or_else(|error| panic!(
                                "{kind:?} level={level} state={state:?} flow={flow:?}: {error:?}"
                            ));
                            assert_eq!(record.state, state);
                            record
                                .mark_issue_attempted(
                                    &owner(),
                                    ConversationContext::Local,
                                    "table-cell",
                                    id,
                                )
                                .unwrap();
                            assert_eq!(
                                record.confirm_issue(
                                    &owner(),
                                    ConversationContext::Local,
                                    "table-cell",
                                    id,
                                ),
                                Ok(next),
                                "{kind:?} level={level} state={state:?} flow={flow:?}"
                            );
                            assert_eq!(record.released, next == Free);
                        } else {
                            assert!(
                                result.is_err(),
                                "{kind:?} level={level} state={state:?} flow={flow:?}"
                            );
                            assert_eq!(record, before);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn basic_issue_confirmation_follows_sync_level_state_tables() {
        for sync_level in [1, 2] {
            for (start, end, released) in [
                (
                    ConversationState::ConfReceive,
                    ConversationState::Receive,
                    false,
                ),
                (ConversationState::ConfSend, ConversationState::Send, false),
                (ConversationState::ConfFree, ConversationState::Free, true),
            ] {
                let mut record = connected(ConversationKind::AppcBasic, sync_level);
                record.state = start;
                let id = record
                    .stage_issue(
                        &owner(),
                        ConversationContext::Local,
                        true,
                        GdsIssueFlow::Confirmation,
                        "confirm",
                    )
                    .unwrap();
                record
                    .mark_issue_attempted(&owner(), ConversationContext::Local, "confirm", id)
                    .unwrap();
                assert_eq!(
                    record.confirm_issue(&owner(), ConversationContext::Local, "confirm", id),
                    Ok(end)
                );
                assert_eq!(record.released, released);
                assert_eq!(
                    ConversationRecord::decode(&record.encode().unwrap()),
                    Ok(record)
                );
            }
        }
    }

    #[test]
    fn basic_issue_error_normal_outcome_is_send_for_every_valid_state() {
        for (sync_level, states) in [
            (
                0,
                &[
                    ConversationState::Send,
                    ConversationState::PendReceive,
                    ConversationState::Receive,
                ][..],
            ),
            (
                1,
                &[
                    ConversationState::Send,
                    ConversationState::PendReceive,
                    ConversationState::Receive,
                    ConversationState::ConfReceive,
                    ConversationState::ConfSend,
                    ConversationState::ConfFree,
                ][..],
            ),
            (
                2,
                &[
                    ConversationState::Send,
                    ConversationState::PendReceive,
                    ConversationState::Receive,
                    ConversationState::ConfReceive,
                    ConversationState::ConfSend,
                    ConversationState::ConfFree,
                    ConversationState::SyncReceive,
                    ConversationState::SyncSend,
                    ConversationState::SyncFree,
                ][..],
            ),
        ] {
            for &start in states {
                let mut record = connected(ConversationKind::AppcBasic, sync_level);
                record.state = start;
                let id = record
                    .stage_issue(
                        &owner(),
                        ConversationContext::Local,
                        true,
                        GdsIssueFlow::Error,
                        "error",
                    )
                    .unwrap();
                record
                    .mark_issue_attempted(&owner(), ConversationContext::Local, "error", id)
                    .unwrap();
                assert_eq!(
                    record.confirm_issue(&owner(), ConversationContext::Local, "error", id),
                    Ok(ConversationState::Send)
                );
                assert!(!record.released);
                assert_eq!(
                    ConversationRecord::decode(&record.encode().unwrap()),
                    Ok(record)
                );
            }
        }
    }

    #[test]
    fn basic_issue_response_rejects_states_outside_sync_level_tables() {
        for (sync_level, flow, states) in [
            (
                0,
                GdsIssueFlow::Confirmation,
                &[
                    ConversationState::ConfReceive,
                    ConversationState::ConfSend,
                    ConversationState::ConfFree,
                ][..],
            ),
            (
                1,
                GdsIssueFlow::Confirmation,
                &[
                    ConversationState::Send,
                    ConversationState::PendReceive,
                    ConversationState::Receive,
                ][..],
            ),
            (
                2,
                GdsIssueFlow::Confirmation,
                &[
                    ConversationState::Send,
                    ConversationState::SyncReceive,
                    ConversationState::SyncSend,
                ][..],
            ),
            (
                0,
                GdsIssueFlow::Error,
                &[
                    ConversationState::PendFree,
                    ConversationState::ConfReceive,
                    ConversationState::SyncReceive,
                ][..],
            ),
            (
                1,
                GdsIssueFlow::Error,
                &[
                    ConversationState::PendFree,
                    ConversationState::SyncReceive,
                    ConversationState::SyncSend,
                    ConversationState::SyncFree,
                ][..],
            ),
            (2, GdsIssueFlow::Error, &[ConversationState::PendFree][..]),
        ] {
            for &state in states {
                let mut record = connected(ConversationKind::AppcBasic, sync_level);
                record.state = state;
                let before = record.clone();
                assert!(
                    record
                        .check_issue(&owner(), ConversationContext::Local, true, flow)
                        .is_err(),
                    "sync_level={sync_level} state={state:?} flow={flow:?}"
                );
                assert_eq!(record, before);
            }
        }
    }

    #[test]
    fn issue_preflight_is_owner_and_sync_fenced_without_mutation() {
        let mut mapped = connected(ConversationKind::AppcMapped, 2);
        let original = mapped.clone();
        assert_eq!(
            mapped.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Prepare
            ),
            Ok(())
        );
        assert_eq!(mapped, original);
        mapped.state = ConversationState::Receive;
        assert_eq!(
            mapped.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Signal
            ),
            Ok(())
        );
        assert_eq!(
            mapped.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Prepare
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState
            ))
        );
        let mut stale = owner();
        stale.lease_epoch = 2;
        assert_eq!(
            mapped.check_issue(
                &stale,
                ConversationContext::Local,
                false,
                GdsIssueFlow::Signal
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::StaleOwner
            ))
        );
        assert_eq!(
            mapped.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Signal
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongKind
            ))
        );
        let basic = connected(ConversationKind::AppcBasic, 0);
        assert_eq!(
            basic.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Confirmation,
            ),
            Err(IssueValidationProblem::WrongSyncLevel)
        );
        assert_eq!(
            basic.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Prepare
            ),
            Err(IssueValidationProblem::WrongSyncLevel)
        );
        assert_eq!(
            basic.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Abend
            ),
            Ok(())
        );
        let mut confirmed = connected(ConversationKind::AppcBasic, 1);
        confirmed.state = ConversationState::ConfReceive;
        assert_eq!(
            confirmed.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Confirmation,
            ),
            Ok(())
        );
        assert_eq!(
            confirmed.check_issue(
                &owner(),
                ConversationContext::Local,
                true,
                GdsIssueFlow::Error
            ),
            Ok(())
        );
        let allocated = ConversationRecord::allocate(
            [0, 0, 0, 2],
            "S001",
            ConversationKind::AppcMapped,
            owner(),
            false,
        )
        .unwrap();
        assert_eq!(
            allocated.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Abend
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState
            ))
        );
        assert_eq!(
            allocated.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Signal
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState
            ))
        );
        assert_eq!(
            super::super::GdsIssueFailure::from_validation(
                IssueValidationProblem::WrongSyncLevel,
                ConversationKind::AppcBasic,
            ),
            Some(super::super::GdsIssueFailure::WrongSyncLevel)
        );
    }

    #[test]
    fn issue_control_requires_durable_attempt_before_confirmed_transition() {
        let mut record = connected(ConversationKind::AppcMapped, 2);
        record.state = ConversationState::Receive;
        let request = IssueRequestIdentity {
            principal: "IBMUSER".into(),
            mutation_sequence: 1,
            digest: [7; 32],
            deadline_tick: 100,
            retain_until_tick: 100,
            state_output: true,
            convdata_output: false,
            retcode_output: false,
        };
        let id = record
            .stage_issue_request(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Signal,
                "signal-1",
                request.clone(),
            )
            .unwrap();
        assert_eq!(record.state, ConversationState::Receive);
        assert!(!record.pending_issue.as_ref().unwrap().attempted);
        assert_eq!(
            record
                .pending_issue
                .as_ref()
                .unwrap()
                .matches_request("signal-1", &request),
            Ok(())
        );
        let mut changed = request.clone();
        changed.digest = [8; 32];
        assert_eq!(
            record
                .pending_issue
                .as_ref()
                .unwrap()
                .matches_request("signal-1", &changed),
            Err(ConversationProblem::StaleOwner)
        );
        let bytes = record.encode().unwrap();
        let mut reopened = ConversationRecord::decode(&bytes).unwrap();
        assert_eq!(reopened, record);
        assert_eq!(
            reopened.confirm_issue(&owner(), ConversationContext::Local, "signal-1", id),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(
            reopened.check_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Error
            ),
            Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState
            ))
        );
        reopened
            .mark_issue_attempted(&owner(), ConversationContext::Local, "signal-1", id)
            .unwrap();
        assert_eq!(
            reopened.mark_issue_attempted(&owner(), ConversationContext::Local, "other", id),
            Err(ConversationProblem::StaleOwner)
        );
        let attempted = ConversationRecord::decode(&reopened.encode().unwrap()).unwrap();
        assert!(attempted.pending_issue.as_ref().unwrap().attempted);
        assert_eq!(
            reopened.confirm_issue(&owner(), ConversationContext::Local, "signal-1", id),
            Ok(ConversationState::Receive)
        );
        assert!(reopened.pending_issue.is_none());
        assert_eq!(reopened.state, ConversationState::Receive);
    }

    #[test]
    fn confirmed_issue_controls_apply_only_their_source_state_change() {
        for (flow, start, end, sync_level) in [
            (
                GdsIssueFlow::Prepare,
                ConversationState::Send,
                ConversationState::SyncSend,
                2,
            ),
            (
                GdsIssueFlow::Confirmation,
                ConversationState::ConfReceive,
                ConversationState::Receive,
                1,
            ),
            (
                GdsIssueFlow::Error,
                ConversationState::ConfReceive,
                ConversationState::Send,
                1,
            ),
            (
                GdsIssueFlow::Error,
                ConversationState::Send,
                ConversationState::Send,
                1,
            ),
        ] {
            let mut record = connected(ConversationKind::AppcBasic, sync_level);
            record.state = start;
            let id = record
                .stage_issue(
                    &owner(),
                    ConversationContext::Local,
                    true,
                    flow,
                    "control-1",
                )
                .unwrap();
            assert_eq!(record.state, start);
            record
                .mark_issue_attempted(&owner(), ConversationContext::Local, "control-1", id)
                .unwrap();
            assert_eq!(
                record.confirm_issue(&owner(), ConversationContext::Local, "control-1", id),
                Ok(end)
            );
            assert_eq!(record.state, end);
            assert!(!record.released);
            assert!(record.pending_issue.is_none());
            assert_eq!(
                ConversationRecord::decode(&record.encode().unwrap()),
                Ok(record)
            );
        }
        let mut sending = connected(ConversationKind::AppcMapped, 0);
        let id = sending
            .stage_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Abend,
                "abend-1",
            )
            .unwrap();
        sending
            .mark_issue_attempted(&owner(), ConversationContext::Local, "abend-1", id)
            .unwrap();
        assert_eq!(
            sending.confirm_issue(&owner(), ConversationContext::Local, "abend-1", id),
            Ok(ConversationState::Free)
        );
        assert!(sending.released);
        assert_eq!(
            ConversationRecord::decode(&sending.encode().unwrap()),
            Ok(sending)
        );
    }

    #[test]
    fn pending_issue_survives_sqlite_reopen_until_confirmed() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-issue-control-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let token;
        let id;
        {
            let store = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            let current = ConversationLedger::load(&store).unwrap();
            let mut next = current.clone();
            next.register_system(ConversationSystemDefinition {
                sysid: "SYS1".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
            token = next
                .allocate("SYS1", ConversationKind::AppcMapped, owner())
                .unwrap()
                .token;
            let record = next.conversation_mut(token).unwrap();
            record
                .connect(
                    &owner(),
                    ConversationContext::Local,
                    false,
                    b"PROGRAM".to_vec(),
                    Vec::new(),
                    2,
                )
                .unwrap();
            id = record
                .stage_issue(
                    &owner(),
                    ConversationContext::Local,
                    false,
                    GdsIssueFlow::Prepare,
                    "prepare-1",
                )
                .unwrap();
            assert_eq!(
                record.release(&owner(), ConversationContext::Local, false),
                Err(ConversationProblem::WrongState)
            );
            let mut abandoned = next.clone();
            assert_eq!(abandoned.release_task(&owner()), Ok(1));
            assert!(
                abandoned
                    .conversation(token)
                    .unwrap()
                    .pending_issue
                    .is_none()
            );
            abandoned.validate().unwrap();
            assert!(current.persist(&mut next, &store).unwrap());
        }
        {
            let store = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            let current = ConversationLedger::load(&store).unwrap();
            let record = current.conversation(token).unwrap();
            assert_eq!(record.state, ConversationState::Send);
            assert!(!record.pending_issue.as_ref().unwrap().attempted);
            let mut next = current.clone();
            next.conversation_mut(token)
                .unwrap()
                .mark_issue_attempted(&owner(), ConversationContext::Local, "prepare-1", id)
                .unwrap();
            assert!(current.persist(&mut next, &store).unwrap());
        }
        {
            let store = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            let current = ConversationLedger::load(&store).unwrap();
            let mut unsafe_cleanup = current.clone();
            assert_eq!(
                unsafe_cleanup.release_task(&owner()),
                Err(ConversationProblem::WrongState)
            );
            let mut next = current.clone();
            assert_eq!(
                next.conversation_mut(token).unwrap().confirm_issue(
                    &owner(),
                    ConversationContext::Local,
                    "prepare-1",
                    id
                ),
                Ok(ConversationState::SyncSend)
            );
            assert!(current.persist(&mut next, &store).unwrap());
            let reopened = ConversationLedger::load(&store).unwrap();
            let record = reopened.conversation(token).unwrap();
            assert_eq!(record.state, ConversationState::SyncSend);
            assert!(record.pending_issue.is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staging_issue_upgrades_a_canonical_v1_conversation_record() {
        let mut legacy = connected(ConversationKind::AppcMapped, 2);
        legacy.version = 1;
        legacy.processing_profile = None;
        let old_bytes = legacy.encode().unwrap();
        assert!(!String::from_utf8_lossy(&old_bytes).contains("pending_issue"));
        let mut reopened = ConversationRecord::decode(&old_bytes).unwrap();
        reopened
            .stage_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Prepare,
                "prepare-legacy",
            )
            .unwrap();
        assert_eq!(reopened.version, CONVERSATION_RECORD_VERSION);
        assert_eq!(reopened.processing_profile.as_deref(), Some("DFHCICSA"));
        assert_eq!(
            ConversationRecord::decode(&reopened.encode().unwrap()),
            Ok(reopened)
        );
    }

    #[test]
    fn malformed_pending_issue_fails_durable_decode() {
        let mut record = connected(ConversationKind::AppcMapped, 2);
        record
            .stage_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Prepare,
                "prepare-1",
            )
            .unwrap();
        for corrupt in [
            {
                let mut value = record.clone();
                value.pending_issue.as_mut().unwrap().effect_key.clear();
                value
            },
            {
                let mut value = record.clone();
                value.pending_issue.as_mut().unwrap().effect_key =
                    "X".repeat(MAX_ISSUE_EFFECT_KEY + 1);
                value
            },
            {
                let mut value = record.clone();
                value.pending_issue.as_mut().unwrap().id = value.sequence + 1;
                value
            },
            {
                let mut value = record.clone();
                value.released = true;
                value.state = ConversationState::Free;
                value
            },
        ] {
            assert_eq!(corrupt.encode(), Err(ConversationProblem::Malformed));
            let bytes = serde_json::to_vec(&corrupt).unwrap();
            assert_eq!(
                ConversationRecord::decode(&bytes),
                Err(ConversationProblem::Malformed)
            );
        }
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn pending_issue_postgres_cas_race_and_restart_confirm_once() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit PostgreSQL test URL required");
        let first = PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let initial = ConversationLedger::load(&first).unwrap();
        let mut installed = initial.clone();
        installed
            .register_system(ConversationSystemDefinition {
                sysid: "I905".into(),
                kind: ConversationKind::AppcMapped,
                capacity: 1,
                enabled: true,
            })
            .unwrap();
        let token = installed
            .allocate("I905", ConversationKind::AppcMapped, owner())
            .unwrap()
            .token;
        installed
            .conversation_mut(token)
            .unwrap()
            .connect(
                &owner(),
                ConversationContext::Local,
                false,
                b"PROGRAM".to_vec(),
                Vec::new(),
                2,
            )
            .unwrap();
        assert!(initial.persist(&mut installed, &first).unwrap());
        drop(first);
        let gate = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            let gate = gate.clone();
            workers.push(std::thread::spawn(move || {
                let current = ConversationLedger::load(&store).unwrap();
                let mut next = current.clone();
                next.conversation_mut(token)
                    .unwrap()
                    .stage_issue(
                        &owner(),
                        ConversationContext::Local,
                        false,
                        GdsIssueFlow::Prepare,
                        "prepare-pg",
                    )
                    .unwrap();
                gate.wait();
                current.persist(&mut next, &store).unwrap()
            }));
        }
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|outcome| **outcome).count(), 1);
        let store = PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let current = ConversationLedger::load(&store).unwrap();
        let pending = current
            .conversation(token)
            .unwrap()
            .pending_issue
            .as_ref()
            .unwrap();
        assert_eq!(pending.effect_key, "prepare-pg");
        assert!(!pending.attempted);
        let id = pending.id;
        let mut attempted = current.clone();
        attempted
            .conversation_mut(token)
            .unwrap()
            .mark_issue_attempted(&owner(), ConversationContext::Local, "prepare-pg", id)
            .unwrap();
        assert!(current.persist(&mut attempted, &store).unwrap());
        drop(store);
        let reopened = PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let current = ConversationLedger::load(&reopened).unwrap();
        assert!(
            current
                .conversation(token)
                .unwrap()
                .pending_issue
                .as_ref()
                .unwrap()
                .attempted
        );
        let mut confirmed = current.clone();
        assert_eq!(
            confirmed.conversation_mut(token).unwrap().confirm_issue(
                &owner(),
                ConversationContext::Local,
                "prepare-pg",
                id
            ),
            Ok(ConversationState::SyncSend)
        );
        assert!(current.persist(&mut confirmed, &reopened).unwrap());
        let final_state = ConversationLedger::load(&reopened).unwrap();
        assert!(
            final_state
                .conversation(token)
                .unwrap()
                .pending_issue
                .is_none()
        );
        assert_eq!(
            final_state.conversation(token).unwrap().state,
            ConversationState::SyncSend
        );
    }
}
