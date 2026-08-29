//! Typed, bounded CICS runtime and protocol-neutral session authority.

#![forbid(unsafe_code)]

mod service;

pub use service::{
    BmsFieldDefinition, BmsMapDefinition, CicsContinuation, CicsLimits, CicsService,
    CicsTerminalSnapshot, cics_provider,
};
