use super::*;

pub(super) const TASK_QUEUE_DESCRIPTORS: [CicsExecutableDescriptor; 2] = [
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Deq,
        namespace: "cics.task",
        name: "deq",
        major: 1,
        effects: DEQ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Enq,
        namespace: "cics.task",
        name: "enq",
        major: 1,
        effects: ENQ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
];
