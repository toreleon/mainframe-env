use super::*;

pub(in crate::machine) fn operation_identities() -> Vec<OperationIdentity> {
    CICS_EXECUTABLE_DESCRIPTORS
        .iter()
        .map(|descriptor| descriptor.identity())
        .collect()
}

pub(in crate::machine) fn validate_module_operations(
    module: &Module,
) -> Result<(), MachineProblem> {
    let mut catalog = OperationCatalog::default();
    for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
        catalog
            .register(operation_schema(descriptor))
            .expect("unique typed CICS identity");
    }
    verify_semantic_contracts(module, &catalog)
        .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))
}

pub(super) fn operation_schema(descriptor: CicsExecutableDescriptor) -> OperationSchema {
    let mut schema = OperationSchema::pure(descriptor.identity(), 0, 0);
    schema.required_attributes = [PLAN_ATTRIBUTE.into()].into_iter().collect();
    schema.allowed_effects = descriptor.effects.iter().copied().collect();
    schema.runtime_import = Some(descriptor.runtime_import.into());
    schema.semantic_contract = OperationSemanticContract::CicsEffect(CicsOperationContract {
        plan_attribute: PLAN_ATTRIBUTE.into(),
        expected_operation: Some(descriptor.operation),
        layout_definition_operation: Some(cobol_layout_definition_identity()),
    });
    schema
}

pub(super) fn expected_operation(identity: &OperationIdentity) -> Option<CicsPlanOperation> {
    cics_executable_descriptor_for_identity(identity).map(|descriptor| descriptor.operation)
}

pub(super) fn expected_effects(operation: CicsPlanOperation) -> &'static [Effect] {
    cics_executable_descriptor(operation).effects
}
