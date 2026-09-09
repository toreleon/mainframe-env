//! Shared static-dialect admission before runtime metadata construction.

use super::{MachineProblem, typed_cics, typed_decimal};
use mainframe_env_ir::{
    Module, OperationCatalog, cobol_layout_definition_schema, verify_semantic_contracts,
};

pub(super) fn validate(module: &Module) -> Result<(), MachineProblem> {
    let mut catalog = OperationCatalog::default();
    catalog
        .register(cobol_layout_definition_schema())
        .expect("unique COBOL layout-definition schema");
    verify_semantic_contracts(module, &catalog)
        .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))?;
    typed_cics::validate_module_operations(module)?;
    typed_decimal::validate_module_operations(module)
}
