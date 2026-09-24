use super::*;

/// Eight source-pinned extraction and positioning routes, in reserved tag order.
pub(super) const CONVERSATION_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 8] = [
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ExtractAttach,
        namespace: "cics.conversation",
        name: "extract-attach",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ExtractAttributes,
        namespace: "cics.conversation",
        name: "extract-attributes",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::GdsExtractAttributes,
        namespace: "cics.conversation",
        name: "gds-extract-attributes",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ExtractLogonMsg,
        namespace: "cics.conversation",
        name: "extract-logonmsg",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ExtractProcess,
        namespace: "cics.conversation",
        name: "extract-process",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::GdsExtractProcess,
        namespace: "cics.conversation",
        name: "gds-extract-process",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ExtractTct,
        namespace: "cics.conversation",
        name: "extract-tct",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Point,
        namespace: "cics.conversation",
        name: "point",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
];
