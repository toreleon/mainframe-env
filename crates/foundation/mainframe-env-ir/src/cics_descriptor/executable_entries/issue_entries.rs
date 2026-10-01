use super::*;

pub(super) const ISSUE_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 20] = [
    issue(CicsPlanOperation::IssueAbend, "abend"),
    issue(CicsPlanOperation::GdsIssueAbend, "gds-abend"),
    issue(CicsPlanOperation::IssueConfirmation, "confirmation"),
    issue(CicsPlanOperation::GdsIssueConfirmation, "gds-confirmation"),
    issue(CicsPlanOperation::IssueError, "error"),
    issue(CicsPlanOperation::GdsIssueError, "gds-error"),
    issue(CicsPlanOperation::IssuePrepare, "prepare"),
    issue(CicsPlanOperation::GdsIssuePrepare, "gds-prepare"),
    issue(CicsPlanOperation::GdsIssueSignal, "gds-signal"),
    issue(CicsPlanOperation::IssueSignal, "signal"),
    device(CicsPlanOperation::IssueCopy, "copy"),
    device(CicsPlanOperation::IssueDisconnect, "disconnect"),
    device(CicsPlanOperation::IssueEndfile, "endfile"),
    device(CicsPlanOperation::IssueEndoutput, "endoutput"),
    device(CicsPlanOperation::IssueEods, "eods"),
    device(CicsPlanOperation::IssueEraseAup, "eraseaup"),
    device(CicsPlanOperation::IssueLoad, "load"),
    device(CicsPlanOperation::IssuePass, "pass"),
    device(CicsPlanOperation::IssuePrint, "print"),
    device(CicsPlanOperation::IssueReset, "reset"),
];

const fn issue(operation: CicsPlanOperation, name: &'static str) -> CicsExecutableDescriptor {
    CicsExecutableDescriptor {
        operation,
        namespace: "cics.conversation.issue",
        name,
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    }
}

const fn device(operation: CicsPlanOperation, name: &'static str) -> CicsExecutableDescriptor {
    CicsExecutableDescriptor {
        operation,
        namespace: "cics.terminal.issue",
        name,
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    }
}
