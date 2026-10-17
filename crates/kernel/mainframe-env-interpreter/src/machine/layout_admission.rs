//! Shared static-dialect admission before runtime metadata construction.

use super::{MachineProblem, typed_cics, typed_decimal};
use mainframe_env_ir::{
    Module, Operation, OperationCatalog, cobol_chunked_initialization_schema,
    cobol_initialization_bytes, cobol_layout_definition_schema, verify_semantic_contracts,
};

pub(super) fn validate(module: &Module) -> Result<(), MachineProblem> {
    let mut catalog = OperationCatalog::default();
    catalog
        .register(cobol_layout_definition_schema())
        .expect("unique COBOL layout-definition schema");
    catalog
        .register(cobol_chunked_initialization_schema())
        .expect("unique chunked COBOL initialization schema");
    verify_semantic_contracts(module, &catalog)
        .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))?;
    typed_cics::validate_module_operations(module)?;
    typed_decimal::validate_module_operations(module)
}

pub(super) fn initialization_bytes(operation: &Operation) -> Result<Vec<u8>, MachineProblem> {
    cobol_initialization_bytes(operation).map_err(|_| MachineProblem::InvalidOperation)
}
