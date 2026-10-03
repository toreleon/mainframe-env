use mainframe_env_execution_api::InvocationLimits;

/// Stable generated semantic-identity descriptor contract, without execution credit.
pub const GENERATED_IDENTITY_CONTRACT: &str = "mainframe-env.generated-semantic-identity@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Generated source identity locator and display metadata; descriptor presence earns no execution credit.
pub struct SemanticIdentityDescriptor {
    /// Exact generated official operation identity.
    pub id: &'static str,
    /// Pinned publication baseline identity.
    pub baseline: &'static str,
    /// Owning subsystem label used by handler validation.
    pub subsystem: &'static str,
    /// Catalog source unit identity.
    pub unit: &'static str,
    /// Human-readable catalog label, not a dispatch alias.
    pub label: &'static str,
}

include!("generated/official_semantic_identities.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Separation between exact generated IBM identities and explicitly prefixed custom operations.
pub enum SemanticNamespace {
    /// Exact generated ibm- identity vocabulary.
    Official,
    /// Explicit custom: namespace without official-source credit.
    Custom,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Bounded control-free semantic identity; official names require generated membership, custom names require a nonempty custom suffix.
pub struct SemanticOperationId(String);

impl SemanticOperationId {
    /// Reject empty/over-limit/control-containing values, unknown ibm- identities and missing custom: suffixes; retain exact spelling without normalization.
    pub fn new(
        value: impl Into<String>,
        limits: InvocationLimits,
    ) -> Result<Self, SemanticIdentityProblem> {
        let value = value.into();
        if value.is_empty()
            || value.len() > limits.max_identity_bytes
            || value.chars().any(char::is_control)
        {
            return Err(SemanticIdentityProblem::InvalidIdentity);
        }
        if value.starts_with("ibm-") {
            if official_semantic_identity(&value).is_none() {
                return Err(SemanticIdentityProblem::UnknownOfficialIdentity);
            }
        } else if !value.starts_with("custom:") || value.len() == "custom:".len() {
            return Err(SemanticIdentityProblem::InvalidNamespace);
        }
        Ok(Self(value))
    }

    #[must_use]
    /// Borrow exact admitted identity spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    /// Return the official or custom namespace established during construction.
    pub fn namespace(&self) -> SemanticNamespace {
        if self.0.starts_with("ibm-") {
            SemanticNamespace::Official
        } else {
            SemanticNamespace::Custom
        }
    }

    #[must_use]
    /// Return the exact generated descriptor for an official identity; custom identities have none.
    pub fn official_descriptor(&self) -> Option<&'static SemanticIdentityDescriptor> {
        official_semantic_identity(&self.0)
    }
}

#[must_use]
/// Borrow the complete sorted generated identity registry; it does not enumerate installed handlers.
pub fn official_semantic_identities() -> &'static [SemanticIdentityDescriptor] {
    OFFICIAL_SEMANTIC_IDENTITIES
}

#[must_use]
/// Look up an exact case-sensitive generated identity; unknown spellings return None.
pub fn official_semantic_identity(id: &str) -> Option<&'static SemanticIdentityDescriptor> {
    OFFICIAL_SEMANTIC_IDENTITIES
        .binary_search_by_key(&id, |descriptor| descriptor.id)
        .ok()
        .map(|index| &OFFICIAL_SEMANTIC_IDENTITIES[index])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Semantic identity admission failure, preserving malformed, namespace and unknown official cases.
pub enum SemanticIdentityProblem {
    /// Identity is empty, over limit or contains a control character.
    InvalidIdentity,
    /// Identity lacks an admitted official/custom namespace or custom suffix.
    InvalidNamespace,
    /// An ibm- spelling is absent from the generated registry.
    UnknownOfficialIdentity,
}

impl std::fmt::Display for SemanticIdentityProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "semantic identity is invalid: {self:?}")
    }
}

impl std::error::Error for SemanticIdentityProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn generated_official_identities_are_sorted_unique_and_exhaustive() {
        let identities = official_semantic_identities();
        assert_eq!(identities.len(), 1_506);
        assert!(identities.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert_eq!(
            identities
                .iter()
                .map(|descriptor| descriptor.baseline)
                .collect::<BTreeSet<_>>()
                .len(),
            9
        );
        for descriptor in identities {
            assert_eq!(official_semantic_identity(descriptor.id), Some(descriptor));
        }
    }

    #[test]
    fn official_and_custom_namespaces_do_not_overlap() {
        let limits = InvocationLimits::default();
        let official =
            SemanticOperationId::new(OFFICIAL_SEMANTIC_IDENTITIES[0].id, limits).unwrap();
        assert_eq!(official.namespace(), SemanticNamespace::Official);
        let custom = SemanticOperationId::new("custom:vendor.operation@1", limits).unwrap();
        assert_eq!(custom.namespace(), SemanticNamespace::Custom);
        assert_eq!(
            SemanticOperationId::new("ibm-unknown:unit:0001", limits),
            Err(SemanticIdentityProblem::UnknownOfficialIdentity)
        );
        assert_eq!(
            SemanticOperationId::new("vendor.operation", limits),
            Err(SemanticIdentityProblem::InvalidNamespace)
        );
    }
}
