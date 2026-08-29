//! Stable, bounded diagnostics and execution problem categories.

#![forbid(unsafe_code)]

mod code;
mod problem;

pub use code::{Completeness, DiagnosticCode, FailureCategory, Phase, Redaction, Severity};
pub use problem::{
    Diagnostic, DiagnosticLimits, DiagnosticProblem, ExecutionProblem, RelatedSpan, SourceSpan,
};

/// Stable diagnostic schema identity.
pub const DIAGNOSTIC_CONTRACT: &str = "mainframe-env.diagnostic@1";
