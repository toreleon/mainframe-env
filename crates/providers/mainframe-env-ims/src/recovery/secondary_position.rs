//! Additive retained identity for a selected full-function pointer occurrence.
use super::{RecoveryLimits, RecoveryProblem};
use crate::database::RecordId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A live engine occurrence witness, distinct from an IBM physical address.
pub struct SavedSecondaryOccurrence {
    /// Internal occurrence identity, insufficient without the retained witness.
    pub id: RecordId,
    /// Domain-separated witness persisted on the existing engine record.
    pub identity: [u8; 32],
    /// Observed data version; settlement alone does not invalidate a pointer.
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Selected root-target processing position used by the existing XRST resolver.
pub struct SavedSecondaryPosition {
    /// The selected metadata index name.
    pub index: String,
    /// PSB, selected PCB and database-definition binding.
    pub metadata_digest: [u8; 32],
    /// Binary/composite search value, never substituted for a hierarchy key.
    pub search_key: Vec<u8>,
    /// Source pointer occurrence, which can differ from its returned target.
    pub source: SavedSecondaryOccurrence,
    /// Physical root target of the selected source pointer.
    pub target: SavedSecondaryOccurrence,
    /// Last returned occurrence under that target.
    pub current: SavedSecondaryOccurrence,
    /// Uniquely keyed physical path for deterministic source-order boundaries.
    pub source_path: Vec<(String, Vec<u8>)>,
    /// Uniquely keyed physical path for GU and missing-occurrence continuation.
    pub current_path: Vec<(String, Vec<u8>)>,
}

impl SavedSecondaryPosition {
    pub(super) fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= limits.max_database_name_bytes
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        };
        if !valid_name(&self.index)
            || self.metadata_digest == [0; 32]
            || self.search_key.is_empty()
            || [&self.source, &self.target, &self.current]
                .iter()
                .any(|p| !p.id.is_nonzero() || p.identity == [0; 32] || p.version == 0)
            || (self.source.id == self.target.id && self.source != self.target)
            || (self.source.id == self.current.id && self.source != self.current)
            || (self.target.id == self.current.id && self.target != self.current)
            || [&self.source_path, &self.current_path].iter().any(|path| {
                path.is_empty()
                    || path.len() > limits.max_positions
                    || path
                        .iter()
                        .any(|(name, key)| !valid_name(name) || key.is_empty())
            })
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let bytes = self
            .source_path
            .iter()
            .chain(&self.current_path)
            .try_fold(self.search_key.len(), |n, (_, key)| {
                n.checked_add(key.len())
            })
            .ok_or(RecoveryProblem::LimitExceeded)?;
        if bytes > limits.max_position_key_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        Ok(())
    }
}
