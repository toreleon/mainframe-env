//! Guard ISSUE controls against the shared APPC allocation before dispatch.
//!
//! These checks never advance protocol state. A command handler must stage a
//! partner control and wait for its confirmed outcome in the same ledger.

use super::{
    ConversationContext, ConversationKind, ConversationOwner, ConversationProblem,
    ConversationRecord, ConversationState, GdsIssueFlow,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueValidationProblem {
    Protocol(ConversationProblem),
    WrongSyncLevel,
}

impl ConversationRecord {
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
        if flow == GdsIssueFlow::Prepare && self.sync_level != Some(2)
            || flow == GdsIssueFlow::Confirmation && self.sync_level == Some(0)
        {
            return Err(IssueValidationProblem::WrongSyncLevel);
        }
        if !match flow {
            GdsIssueFlow::Abend => true,
            GdsIssueFlow::Confirmation => self.state == ConversationState::ConfReceive,
            GdsIssueFlow::Error => matches!(
                self.state,
                ConversationState::ConfReceive
                    | ConversationState::Send
                    | ConversationState::Receive
            ),
            GdsIssueFlow::Prepare => self.state == ConversationState::Send,
            GdsIssueFlow::Signal => self.state == ConversationState::Receive,
        } {
            return Err(IssueValidationProblem::Protocol(
                ConversationProblem::WrongState,
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
