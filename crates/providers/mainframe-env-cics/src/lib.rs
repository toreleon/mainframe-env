//! Typed, bounded CICS runtime and protocol-neutral session authority.

#![forbid(unsafe_code)]

mod abi;
mod service;

pub use abi::cics_abi_library;

pub use service::{
    BmsFieldDefinition, BmsMapDefinition, CicsContinuation, CicsFileDefinition, CicsFileStatus,
    CicsLimits, CicsService, CicsTerminalExecution, CicsTerminalSnapshot, CicsTraceEntry,
    cics_provider,
};

#[cfg(feature = "fault-injection")]
pub use service::CicsFileFaultPoint;
