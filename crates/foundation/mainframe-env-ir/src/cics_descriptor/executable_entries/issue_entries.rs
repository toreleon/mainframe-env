use super::*;

pub(super) const ISSUE_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 10] = [
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
