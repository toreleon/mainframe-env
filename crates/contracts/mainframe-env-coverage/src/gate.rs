//! Unchanged shared gate identities, extracted to keep the legacy facade shrinking.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One independently evidenced capability gate.
pub enum CoverageGate {
    /// The frontend recognizes the capability syntax or protocol identity.
    Recognized,
    /// Invalid inputs are rejected and valid inputs reach typed validation.
    Validated,
    /// A valid request reaches its owned semantic implementation.
    Executed,
    /// Success and failure outcomes are observably distinct and exact.
    Conditioned,
    /// State and identity survive the required restart boundary.
    Recovered,
    /// An approved independent oracle agrees with the candidate behavior.
    Differential,
}

impl CoverageGate {
    /// Every gate in canonical evaluation order.
    pub const ALL: [Self; 6] = [
        Self::Recognized,
        Self::Validated,
        Self::Executed,
        Self::Conditioned,
        Self::Recovered,
        Self::Differential,
    ];

    #[must_use]
    /// Stable lowercase identifier used in manifests and receipts.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Recognized => "recognized",
            Self::Validated => "validated",
            Self::Executed => "executed",
            Self::Conditioned => "conditioned",
            Self::Recovered => "recovered",
            Self::Differential => "differential",
        }
    }
}
