use crate::{CicsPlanOperation, DecimalPlanWireVersion, Effect, OperationIdentity};
use std::collections::{BTreeMap, BTreeSet};

/// Static semantic contract attached to a typed operation schema.
///
/// The contract selects a bounded validator implemented by the IR crate. It is
/// deliberately a semantic-dialect contract rather than a language enum: a
/// frontend chooses an operation contract, while the verifier remains unaware
/// of frontend AST or HIR types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationSemanticContract {
    /// Generic structural verification is sufficient for this operation.
    Structural,
    /// Decimal assignment plan encoded in one bytes attribute.
    DecimalAssignment(DecimalOperationContract),
    /// Typed CICS effect plan encoded in one bytes attribute.
    CicsEffect(CicsOperationContract),
}

/// Static contract for one decimal assignment operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalOperationContract {
    /// Attribute containing the canonical decimal plan.
    pub plan_attribute: String,
    /// Wire version required by this operation semantic major.
    pub expected_plan_version: DecimalPlanWireVersion,
    /// Permitted producer identities for a compatibility or owning-HIR route.
    /// An empty set accepts any canonical provenance because current
    /// executable behavior is selected by the plan policy, not by origin.
    pub allowed_semantic_origins: BTreeSet<String>,
    /// Executable layout-definition operation used to prove the plan's
    /// qualified names, storage views, extents, and access categories.
    ///
    /// `None` is reserved for a HIR whose storage arena is authoritative and
    /// which does not yet encode executable layout metadata.
    pub layout_definition_operation: Option<OperationIdentity>,
    /// Optional typed condition topology required by an executable operation.
    pub condition: Option<DecimalConditionContract>,
}

/// Attribute vocabulary and status identity for typed decimal condition edges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalConditionContract {
    pub status: String,
    pub status_attribute: String,
    pub branch_mask_attribute: String,
    pub branch_polarity_attribute: String,
}

/// Static contract for one typed CICS effect operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsOperationContract {
    /// Attribute containing the canonical CICS effect plan.
    pub plan_attribute: String,
    /// Expected executable operation. HIR uses `None` because its one command
    /// identity can carry any operation represented by the validated plan.
    pub expected_operation: Option<CicsPlanOperation>,
    /// Executable layout-definition operation used to prove the plan's
    /// qualified names, storage views, extents, and access categories.
    /// `None` selects the documented storage-only HIR binding.
    pub layout_definition_operation: Option<OperationIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationSchema {
    pub identity: OperationIdentity,
    pub min_operands: usize,
    pub max_operands: usize,
    pub min_results: usize,
    pub max_results: usize,
    pub required_attributes: BTreeSet<String>,
    pub allowed_effects: BTreeSet<Effect>,
    pub terminator: bool,
    pub executable: bool,
    pub runtime_import: Option<String>,
    /// Optional dialect-owned validation beyond the generic outer IR shape.
    pub semantic_contract: OperationSemanticContract,
}

impl OperationSchema {
    #[must_use]
    pub fn pure(identity: OperationIdentity, operands: usize, results: usize) -> Self {
        Self {
            identity,
            min_operands: operands,
            max_operands: operands,
            min_results: results,
            max_results: results,
            required_attributes: BTreeSet::new(),
            allowed_effects: BTreeSet::new(),
            terminator: false,
            executable: true,
            runtime_import: None,
            semantic_contract: OperationSemanticContract::Structural,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationCatalog {
    entries: BTreeMap<OperationIdentity, OperationSchema>,
}

impl OperationCatalog {
    pub fn register(&mut self, schema: OperationSchema) -> Result<(), CatalogProblem> {
        if self.entries.contains_key(&schema.identity) {
            Err(CatalogProblem::DuplicateOperation)
        } else {
            self.entries.insert(schema.identity.clone(), schema);
            Ok(())
        }
    }

    #[must_use]
    pub fn get(&self, identity: &OperationIdentity) -> Option<&OperationSchema> {
        self.entries.get(identity)
    }

    #[must_use]
    pub fn identities(&self) -> impl ExactSizeIterator<Item = &OperationIdentity> {
        self.entries.keys()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogProblem {
    DuplicateOperation,
}

impl std::fmt::Display for CatalogProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "operation catalog contains a duplicate identity")
    }
}

impl std::error::Error for CatalogProblem {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegalityProfile {
    pub allowed_operations: BTreeSet<OperationIdentity>,
    pub allowed_runtime_imports: BTreeSet<String>,
}
