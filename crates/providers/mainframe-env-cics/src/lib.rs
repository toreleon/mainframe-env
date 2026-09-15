//! Typed, bounded CICS runtime and protocol-neutral session authority.

#![forbid(unsafe_code)]

mod abi;
mod generated;
mod retention;
mod service;

pub use abi::cics_abi_library;

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, CICS_RETENTION_NAMESPACES,
    CicsReplayCodecVersion, CicsReplayRetentionState, CicsReplayRowDescriptor,
    CicsReplayValidationError, CicsUndoRowDescriptor, CicsUowCodecVersion, CicsUowDependencyState,
    CicsUowRowDescriptor, CicsUowState, CicsUowValidationError, describe_cics_replay_row,
    describe_cics_undo_row, describe_cics_uow_row,
};
pub use service::{
    BmsFieldDefinition, BmsMapDefinition, CicsContinuation, CicsEnqueueModelDefinition,
    CicsFileDefinition, CicsFileStatus, CicsLimits, CicsReplayClock, CicsService,
    CicsTerminalExecution, CicsTerminalSnapshot, CicsTraceEntry, cics_provider,
};

#[cfg(feature = "fault-injection")]
pub use service::CicsFileFaultPoint;
