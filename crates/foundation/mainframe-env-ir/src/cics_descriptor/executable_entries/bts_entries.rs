use super::*;

const fn descriptor(
    operation: CicsPlanOperation,
    name: &'static str,
    effects: &'static [Effect],
) -> CicsExecutableDescriptor {
    CicsExecutableDescriptor {
        operation,
        namespace: "cics.bts",
        name,
        major: 1,
        effects,
        runtime_import: CICS_RUNTIME_IMPORT,
    }
}

pub(super) const BTS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 44] = [
    descriptor(
        CicsPlanOperation::AcquireActivityId,
        "acquire-activityid",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::AcquireProcess,
        "acquire-process",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CancelAcqActivity,
        "cancel-acqactivity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CancelAcqProcess,
        "cancel-acqprocess",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CancelActivity,
        "cancel-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CheckAcqActivity,
        "check-acqactivity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CheckAcqProcess,
        "check-acqprocess",
        BTS_READ_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::CheckActivity,
        "check-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::DefineActivity,
        "define-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::DefineProcess,
        "define-process",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::DeleteActivity,
        "delete-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::ResetAcqProcess,
        "reset-acqprocess",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::ResetActivity,
        "reset-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::ResumeAcqActivity,
        "resume-acqactivity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::ResumeAcqProcess,
        "resume-acqprocess",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::ResumeActivity,
        "resume-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::RunAcqActivity,
        "run-acqactivity",
        BTS_RUN_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::RunAcqProcess,
        "run-acqprocess",
        BTS_RUN_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::RunActivity,
        "run-activity",
        BTS_RUN_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::RunTransId,
        "run-transid",
        BTS_RUN_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::SuspendAcqActivity,
        "suspend-acqactivity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::SuspendAcqProcess,
        "suspend-acqprocess",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::SuspendActivity,
        "suspend-activity",
        BTS_MUTATE_EFFECTS,
    ),
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
    descriptor(
        CicsPlanOperation::BtsEndBrowseActivity,
        "endbrowse-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsGetNextActivity,
        "getnext-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsInquireActivity,
        "inquire-activityid",
        BTS_READ_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsStartBrowseActivity,
        "startbrowse-activity",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsEndBrowseProcess,
        "endbrowse-process",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsGetNextProcess,
        "getnext-process",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsInquireProcess,
        "inquire-process",
        BTS_READ_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsStartBrowseProcess,
        "startbrowse-process",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsEndBrowseEvent,
        "endbrowse-event",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsGetNextEvent,
        "getnext-event",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsInquireEvent,
        "inquire-event",
        BTS_READ_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsStartBrowseEvent,
        "startbrowse-event",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsEndBrowseTimer,
        "endbrowse-timer",
        BTS_MUTATE_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsInquireTimer,
        "inquire-timer",
        BTS_READ_EFFECTS,
    ),
    descriptor(
        CicsPlanOperation::BtsStartBrowseTimer,
        "startbrowse-timer",
        BTS_MUTATE_EFFECTS,
    ),
];
