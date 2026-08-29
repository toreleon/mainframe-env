use crate::{Effect, OperationIdentity};
use std::collections::{BTreeMap, BTreeSet};

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
