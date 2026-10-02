use mainframe_env_execution_api::InvocationLimits;

/// Version identifier for the generated semantic-identity descriptor format.
pub const GENERATED_IDENTITY_CONTRACT: &str = "mainframe-env.generated-semantic-identity@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Static catalog identity metadata.
/// A registered descriptor establishes a catalog identity, not executable support or conformance
/// credit.
pub struct SemanticIdentityDescriptor {
    /// Full stable identity string used for exact catalog lookup.
    pub id: &'static str,
    /// Pinned baseline identity from which this catalog descriptor was generated.
    pub baseline: &'static str,
    /// Subsystem owning the catalog row.
    pub subsystem: &'static str,
    /// Catalog unit containing this row.
    pub unit: &'static str,
    /// Human-readable catalog row label.
    pub label: &'static str,
}

include!("generated/official_semantic_identities.rs");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Authority namespace of a validated semantic operation identity.
pub enum SemanticNamespace {
    /// Identity registered in the generated official catalog.
    Official,
    /// Caller-defined identity under the `custom:` prefix.
    Custom,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Owned, bounded official catalog identity or nonempty `custom:` identity.
/// Official IDs must be present in the generated identity table at construction time.
pub struct SemanticOperationId(String);

impl SemanticOperationId {
    /// Own and validate an identity against the byte limit and namespace rules.
    /// Reject unknown `ibm-` identities, empty custom suffixes and control characters.
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
    /// Borrow the exact validated identity without allocating.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    /// Return the official or custom namespace established at construction.
    pub fn namespace(&self) -> SemanticNamespace {
        if self.0.starts_with("ibm-") {
            SemanticNamespace::Official
        } else {
            SemanticNamespace::Custom
        }
    }

    #[must_use]
    /// Borrow official catalog metadata, or return `None` for a custom identity.
    pub fn official_descriptor(&self) -> Option<&'static SemanticIdentityDescriptor> {
        official_semantic_identity(&self.0)
    }
}

#[must_use]
/// Borrow the generated official identity table in sorted identity order.
pub fn official_semantic_identities() -> &'static [SemanticIdentityDescriptor] {
    OFFICIAL_SEMANTIC_IDENTITIES
}

#[must_use]
/// Look up an exact official identity; unknown IDs return `None`.
pub fn official_semantic_identity(id: &str) -> Option<&'static SemanticIdentityDescriptor> {
    OFFICIAL_SEMANTIC_IDENTITIES
        .binary_search_by_key(&id, |descriptor| descriptor.id)
        .ok()
        .map(|index| &OFFICIAL_SEMANTIC_IDENTITIES[index])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Reason a semantic operation identity failed construction.
pub enum SemanticIdentityProblem {
    /// The identity is empty, oversized or contains control characters.
    InvalidIdentity,
    /// The identity is neither registered official syntax nor a nonempty custom identity.
    InvalidNamespace,
    /// An `ibm-` identity is absent from the generated official catalog.
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
