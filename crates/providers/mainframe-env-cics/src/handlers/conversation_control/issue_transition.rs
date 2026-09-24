//! Guard ISSUE controls against the shared APPC allocation before dispatch.
//!
//! Staging and dispatch markers retain the current protocol state. Only a
//! confirmed partner result applies the source transition in the same ledger.

use super::{
    ConversationContext, ConversationKind, ConversationOwner, ConversationProblem,
    ConversationRecord, ConversationState, GdsIssueFlow,
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
        self.next_sequence()
            .map_err(IssueValidationProblem::Protocol)?;
        let id = self.sequence;
        self.pending_issue = Some(IssuePendingControl {
            flow,
            effect_key: effect_key.into(),
            id,
            attempted: false,
        });
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
            GdsIssueFlow::Confirmation => ConversationState::Receive,
            GdsIssueFlow::Error if self.state == ConversationState::ConfReceive => {
                ConversationState::Receive
            }
            GdsIssueFlow::Error | GdsIssueFlow::Signal => self.state,
            GdsIssueFlow::Prepare => ConversationState::SyncReceive,
        };
        let release = pending.flow == GdsIssueFlow::Abend;
        self.next_sequence()?;
        self.state = next_state;
        self.released = release;
        self.pending_issue = None;
        Ok(next_state)
    }
}

fn issue_state_valid(record: &ConversationRecord, flow: GdsIssueFlow) -> bool {
    match flow {
        GdsIssueFlow::Abend => true,
        GdsIssueFlow::Confirmation => record.state == ConversationState::ConfReceive,
        GdsIssueFlow::Error => matches!(
            record.state,
            ConversationState::ConfReceive | ConversationState::Send | ConversationState::Receive
        ),
        GdsIssueFlow::Prepare => record.state == ConversationState::Send,
        GdsIssueFlow::Signal => record.state == ConversationState::Receive,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ConversationLedger, ConversationSystemDefinition};
    use super::*;
    use mainframe_env_store::SqliteStateStore;

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
            Ok(())
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
        let id = record
            .stage_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Signal,
                "signal-1",
            )
            .unwrap();
        assert_eq!(record.state, ConversationState::Receive);
        assert!(!record.pending_issue.as_ref().unwrap().attempted);
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
                ConversationState::SyncReceive,
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
                ConversationState::Receive,
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
        let mut allocated = ConversationRecord::allocate(
            [0, 0, 0, 2],
            "S001",
            ConversationKind::AppcMapped,
            owner(),
            false,
        )
        .unwrap();
        let id = allocated
            .stage_issue(
                &owner(),
                ConversationContext::Local,
                false,
                GdsIssueFlow::Abend,
                "abend-1",
            )
            .unwrap();
        allocated
            .mark_issue_attempted(&owner(), ConversationContext::Local, "abend-1", id)
            .unwrap();
        assert_eq!(
            allocated.confirm_issue(&owner(), ConversationContext::Local, "abend-1", id),
            Ok(ConversationState::Free)
        );
        assert!(allocated.released);
        assert_eq!(
            ConversationRecord::decode(&allocated.encode().unwrap()),
            Ok(allocated)
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
                Ok(ConversationState::SyncReceive)
            );
            assert!(current.persist(&mut next, &store).unwrap());
            let reopened = ConversationLedger::load(&store).unwrap();
            let record = reopened.conversation(token).unwrap();
            assert_eq!(record.state, ConversationState::SyncReceive);
            assert!(record.pending_issue.is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
