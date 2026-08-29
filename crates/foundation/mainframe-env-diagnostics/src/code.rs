use crate::DiagnosticProblem;
use std::fmt;

/// Stable, validated diagnostic code.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(String);

impl DiagnosticCode {
    /// Accepts 3-64 ASCII upper-case letters, digits, dots, underscores, or hyphens.
    pub fn new(value: impl Into<String>) -> Result<Self, DiagnosticProblem> {
        let value = value.into();
        if !(3..=64).contains(&value.len())
            || !value.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
        {
            return Err(DiagnosticProblem::InvalidCode);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Information,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Admission,
    Source,
    Preprocess,
    Parse,
    Semantic,
    Verify,
    Lower,
    Execute,
    Host,
    Store,
    Protocol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureCategory {
    MalformedInput,
    Unsupported,
    Condition,
    Abend,
    Cancelled,
    TimedOut,
    Rejected,
    ResourceExhausted,
    Unauthorized,
    ProviderFailure,
    InfrastructureFailure,
    IncompatibleVersion,
    UnknownOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completeness {
    Complete,
    Incomplete,
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Redaction {
    Public,
    Confidential,
    Secret,
}
