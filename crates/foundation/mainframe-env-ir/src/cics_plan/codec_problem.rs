use std::fmt;

/// Failure returned by the CICS effect-plan codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsPlanCodecProblem {
    /// The plan magic is absent.
    BadMagic,
    /// The encoded plan uses an unsupported version.
    UnsupportedVersion,
    /// The input ends before a declared field is complete.
    Truncated,
    /// Bytes remain after the complete plan.
    TrailingData,
    /// A tag, duplicate, or operation shape is invalid.
    Malformed,
    /// Text or field ordering is not canonical.
    NonCanonical,
    /// A configured resource limit was exceeded.
    LimitExceeded,
}

impl fmt::Display for CicsPlanCodecProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CICS effect plan codec failed: {self:?}")
    }
}

impl std::error::Error for CicsPlanCodecProblem {}
