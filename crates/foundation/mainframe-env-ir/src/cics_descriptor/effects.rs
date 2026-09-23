use super::*;

pub(super) const DIAGNOSTIC_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
pub(super) const TRACE_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const MONITOR_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Clock,
    Effect::Condition,
    Effect::Transaction,
];

pub(super) const READ_EFFECTS: &[Effect] = &[
    Effect::DatasetRead,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const REWRITE_EFFECTS: &[Effect] = &[
    Effect::DatasetWrite,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const SYNCPOINT_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const DEQ_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const ENQ_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const CHANGE_TASK_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];
pub(super) const SUSPEND_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];
pub(super) const WAIT_EVENT_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const SET_ASSOCIATION_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const ADDRESS_SET_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const STORAGE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const ASKTIME_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Clock,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const FORMAT_TIME_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const ABEND_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::ProgramControl,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const CONTROL_TRANSFER_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::ProgramControl,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const HANDLE_STACK_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const IGNORE_CONDITION_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
pub(super) const QUEUE_WRITE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const JOURNAL_WAIT_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const TEMPORARY_QUEUE_WRITE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const TERMINAL_RECEIVE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalRead,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const TERMINAL_SEND_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const ASSIGN_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const PURGE_MESSAGE_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const START_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Clock,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const RETRIEVE_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const CANCEL_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const DELAY_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const DOCUMENT_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const EVENT_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
pub(super) const EVENT_TIMER_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Clock,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const TRANSFORM_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
pub(super) const WEB_SERVICE_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
pub(super) const WEB_PARSE_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
pub(super) const SPOOL_EFFECTS: &[Effect] = &[
    Effect::Spool,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const COUNTER_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const COUNTER_QUERY_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];

pub(super) const WEB_OPEN_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];

pub(super) const CHANGE_TASK_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];

pub(super) const WAIT_EVENT_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
