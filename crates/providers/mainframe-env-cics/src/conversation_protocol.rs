//! Shared APPC/MRO protocol view used by EXTRACT and conversation-open.
//!
//! The conversation-control ledger is the sole durable authority. This module
//! retains the extraction-specific indicator and GDS return-code projections.

mod gds;
mod indicators;

pub use crate::service::{
    ConversationContext, ConversationKind, ConversationLedger, ConversationOwner,
    ConversationProblem, ConversationRecord, ConversationState,
};
pub use gds::{GdsExtractAttributesFailure, GdsExtractProcessFailure};
pub use indicators::ConversationIndicators;
