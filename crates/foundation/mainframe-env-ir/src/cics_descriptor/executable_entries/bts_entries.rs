//! Executable descriptors for BTS child and LINK controls.

use super::*;

pub(super) const BTS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 6] = [
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::FetchAny,
        namespace: "cics.bts",
        name: "fetch-any",
        major: 1,
        effects: BTS_FETCH_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::FetchChild,
        namespace: "cics.bts",
        name: "fetch-child",
        major: 1,
        effects: BTS_FETCH_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::FreeChild,
        namespace: "cics.bts",
        name: "free-child",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::LinkAcqActivity,
        namespace: "cics.bts",
        name: "link-acqactivity",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::LinkAcqProcess,
        namespace: "cics.bts",
        name: "link-acqprocess",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::LinkActivity,
        namespace: "cics.bts",
        name: "link-activity",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
];
