//! APPC basic GDS six-byte return codes from pinned CICS TS 6.x topics.
//!
//! GDS commands report through RETCODE, without raising EXEC CICS conditions.

use super::ConversationIssue;

/// Full six-byte GDS RETCODE, including zero trailing bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GdsReturnCode(pub [u8; 6]);

impl GdsReturnCode {
    pub const NORMAL: Self = Self([0; 6]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsAllocateFailure {
    UnknownSystem,
    WrongConnectionKind,
    NoImmediateSession,
    UnknownMode,
    RestrictedMode,
    CancelledWhileQueued,
    ClosedMode,
    DrainingMode,
    UnusableConnection,
    UnknownPartnerNetworkName,
    UnknownPartner,
    UnknownPartnerProfile,
}

impl GdsAllocateFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::UnknownSystem => [0x01, 0x0c, 0, 0, 0, 0],
            Self::WrongConnectionKind => [0x01, 0x0c, 0x04, 0, 0, 0],
            Self::NoImmediateSession => [0x01, 0x04, 0x04, 0, 0, 0],
            Self::UnknownMode => [0x01, 0x04, 0x08, 0, 0, 0],
            Self::RestrictedMode => [0x01, 0x04, 0x0c, 0, 0, 0],
            Self::CancelledWhileQueued => [0x01, 0x04, 0x10, 0, 0, 0],
            Self::ClosedMode => [0x01, 0x04, 0x14, 0, 0, 0],
            Self::DrainingMode => [0x01, 0x04, 0x18, 0, 0, 0],
            Self::UnusableConnection => [0x01, 0x08, 0, 0, 0, 0],
            Self::UnknownPartnerNetworkName => [0x01, 0x0c, 0x14, 0, 0, 0],
            Self::UnknownPartner => [0x02, 0x0c, 0, 0, 0, 0],
            Self::UnknownPartnerProfile => [0x06, 0, 0, 0, 0, 0],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsAssignFailure {
    NotAppc,
    NotBasic,
    NoPrincipalFacility,
}

impl GdsAssignFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::NoPrincipalFacility => [0x04, 0, 0, 0, 0, 0],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsConnectFailure {
    UnknownPartner,
    NotAppc,
    NotBasic,
    InvalidSyncLevel,
    StateCheck,
    NotOwned,
    InvalidProcessLength,
    InvalidPipLength,
}

impl GdsConnectFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::UnknownPartner => [0x02, 0x0c, 0, 0, 0, 0],
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::InvalidSyncLevel => [0x03, 0x0c, 0, 0, 0, 0],
            Self::StateCheck => [0x03, 0x08, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
            Self::InvalidProcessLength => [0x05, 0, 0, 0, 0, 0x20],
            Self::InvalidPipLength => [0x05, 0, 0, 0, 0x7f, 0xff],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsFreeFailure {
    NotAppc,
    NotBasic,
    StateCheck,
    NotOwned,
}

/// Source-specific GDS ISSUE failures. Each GDS form writes the full RETCODE
/// and never raises an EXEC CICS condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsIssueFailure {
    NotAppc,
    NotBasic,
    StateCheck,
    WrongSyncLevel,
    NotOwned,
}

impl GdsIssueFailure {
    #[must_use]
    pub const fn retcode(self, flow: ConversationIssue) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::StateCheck if matches!(flow, ConversationIssue::Prepare) => {
                [0x03, 0x24, 0, 0, 0, 0]
            }
            Self::StateCheck => [0x03, 0x08, 0, 0, 0, 0],
            Self::WrongSyncLevel if matches!(flow, ConversationIssue::Prepare) => {
                [0x03, 0x0c, 0, 0, 0, 0]
            }
            Self::WrongSyncLevel if matches!(flow, ConversationIssue::Confirmation) => {
                [0x03, 0x14, 0, 0, 0, 0]
            }
            Self::WrongSyncLevel => [0x03, 0x08, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
        })
    }
}

impl GdsFreeFailure {
    #[must_use]
    pub const fn retcode(self) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::StateCheck => [0x03, 0x08, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_gds_command_preserves_its_source_return_codes() {
        assert_eq!(GdsReturnCode::NORMAL.0, [0; 6]);
        assert_eq!(
            GdsAllocateFailure::NoImmediateSession.retcode().0,
            [1, 4, 4, 0, 0, 0]
        );
        assert_eq!(
            GdsAllocateFailure::CancelledWhileQueued.retcode().0,
            [1, 4, 16, 0, 0, 0]
        );
        assert_eq!(
            GdsAssignFailure::NoPrincipalFacility.retcode().0,
            [4, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsConnectFailure::InvalidProcessLength.retcode().0,
            [5, 0, 0, 0, 0, 32]
        );
        assert_eq!(
            GdsConnectFailure::InvalidPipLength.retcode().0,
            [5, 0, 0, 0, 127, 255]
        );
        assert_eq!(GdsFreeFailure::StateCheck.retcode().0, [3, 8, 0, 0, 0, 0]);
        assert_eq!(
            GdsIssueFailure::StateCheck
                .retcode(ConversationIssue::Prepare)
                .0,
            [3, 36, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsIssueFailure::WrongSyncLevel
                .retcode(ConversationIssue::Prepare)
                .0,
            [3, 12, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsIssueFailure::WrongSyncLevel
                .retcode(ConversationIssue::Confirmation)
                .0,
            [3, 20, 0, 0, 0, 0]
        );
    }
}
