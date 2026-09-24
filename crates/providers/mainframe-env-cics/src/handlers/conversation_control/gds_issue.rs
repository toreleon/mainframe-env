//! Source-specific GDS ISSUE return codes over the shared APPC basic ledger.

use super::GdsReturnCode;

/// The five APPC basic ISSUE controls share the six-byte GDS result area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GdsIssueFlow {
    Abend,
    Confirmation,
    Error,
    Prepare,
    Signal,
}

/// Protocol failures reported in RETCODE, never as EXEC CICS conditions.
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
    pub const fn retcode(self, flow: GdsIssueFlow) -> GdsReturnCode {
        GdsReturnCode(match self {
            Self::NotAppc => [0x03, 0, 0, 0, 0, 0],
            Self::NotBasic => [0x03, 0x04, 0, 0, 0, 0],
            Self::StateCheck if matches!(flow, GdsIssueFlow::Prepare) => [0x03, 0x24, 0, 0, 0, 0],
            Self::StateCheck => [0x03, 0x08, 0, 0, 0, 0],
            Self::WrongSyncLevel if matches!(flow, GdsIssueFlow::Prepare) => {
                [0x03, 0x0c, 0, 0, 0, 0]
            }
            Self::WrongSyncLevel if matches!(flow, GdsIssueFlow::Confirmation) => {
                [0x03, 0x14, 0, 0, 0, 0]
            }
            Self::WrongSyncLevel => [0x03, 0x08, 0, 0, 0, 0],
            Self::NotOwned => [0x04, 0, 0, 0, 0, 0],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gds_issue_codes_are_six_bytes_and_flow_specific() {
        for flow in [
            GdsIssueFlow::Abend,
            GdsIssueFlow::Confirmation,
            GdsIssueFlow::Error,
            GdsIssueFlow::Prepare,
            GdsIssueFlow::Signal,
        ] {
            assert_eq!(GdsIssueFailure::NotAppc.retcode(flow).0, [3, 0, 0, 0, 0, 0]);
            assert_eq!(
                GdsIssueFailure::NotBasic.retcode(flow).0,
                [3, 4, 0, 0, 0, 0]
            );
            assert_eq!(
                GdsIssueFailure::NotOwned.retcode(flow).0,
                [4, 0, 0, 0, 0, 0]
            );
            assert_eq!(GdsIssueFailure::StateCheck.retcode(flow).0[0], 3);
        }
        assert_eq!(
            GdsIssueFailure::StateCheck.retcode(GdsIssueFlow::Prepare).0,
            [3, 36, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsIssueFailure::WrongSyncLevel
                .retcode(GdsIssueFlow::Prepare)
                .0,
            [3, 12, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsIssueFailure::WrongSyncLevel
                .retcode(GdsIssueFlow::Confirmation)
                .0,
            [3, 20, 0, 0, 0, 0]
        );
        assert_eq!(
            GdsIssueFailure::StateCheck.retcode(GdsIssueFlow::Signal).0,
            [3, 8, 0, 0, 0, 0]
        );
    }
}
