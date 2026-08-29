use crate::model::validate_extent;
use crate::{IrProblem, LegalityProfile, Module, OperationCatalog, OperationIdentity};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    pub operation_count: usize,
    pub value_count: usize,
    pub storage_bytes: u64,
    pub operations: BTreeSet<OperationIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegalModule {
    module: Module,
    report: VerificationReport,
}

impl LegalModule {
    #[must_use]
    pub fn module(&self) -> &Module {
        &self.module
    }
    #[must_use]
    pub fn report(&self) -> &VerificationReport {
        &self.report
    }
    #[must_use]
    pub fn into_module(self) -> Module {
        self.module
    }
}

pub fn verify(
    module: &Module,
    catalog: &OperationCatalog,
) -> Result<VerificationReport, VerificationProblem> {
    if module.regions().is_empty() {
        return Err(VerificationProblem::EmptyModule);
    }
    let mut operation_count = 0usize;
    let mut values_seen = 0usize;
    let mut identities = BTreeSet::new();
    for region in module.regions() {
        if region.blocks.is_empty() {
            return Err(VerificationProblem::EmptyRegion);
        }
        for block in &region.blocks {
            if block.operations.is_empty() {
                return Err(VerificationProblem::EmptyBlock);
            }
            for (position, operation) in block.operations.iter().enumerate() {
                let schema = catalog.get(&operation.identity).ok_or_else(|| {
                    VerificationProblem::UnknownOperation(operation.identity.clone())
                })?;
                if !(schema.min_operands..=schema.max_operands).contains(&operation.operands.len())
                    || !(schema.min_results..=schema.max_results).contains(&operation.results.len())
                {
                    return Err(VerificationProblem::SchemaMismatch(
                        operation.identity.clone(),
                    ));
                }
                if operation
                    .operands
                    .iter()
                    .any(|operand| operand.get() as usize >= values_seen)
                {
                    return Err(VerificationProblem::UseBeforeDefinition);
                }
                for (expected, result) in (values_seen..).zip(&operation.results) {
                    if result.get() as usize != expected {
                        return Err(VerificationProblem::NonCanonicalValueId);
                    }
                }
                if !schema
                    .required_attributes
                    .iter()
                    .all(|name| operation.attributes.contains_key(name))
                {
                    return Err(VerificationProblem::SchemaMismatch(
                        operation.identity.clone(),
                    ));
                }
                if operation
                    .effects
                    .iter()
                    .any(|effect| !schema.allowed_effects.contains(effect))
                {
                    return Err(VerificationProblem::EffectMismatch(
                        operation.identity.clone(),
                    ));
                }
                for reference in &operation.storage {
                    let storage = module
                        .storage()
                        .get(reference.storage.get() as usize)
                        .ok_or(VerificationProblem::InvalidStorage)?;
                    validate_extent(reference.offset, reference.length, storage.size)
                        .map_err(VerificationProblem::Model)?;
                }
                if schema.terminator != (position + 1 == block.operations.len()) {
                    return Err(VerificationProblem::TerminatorMismatch(
                        operation.identity.clone(),
                    ));
                }
                values_seen += operation.results.len();
                operation_count += 1;
                identities.insert(operation.identity.clone());
            }
        }
    }
    if values_seen != module.value_count() as usize {
        return Err(VerificationProblem::NonCanonicalValueId);
    }
    for storage in module.storage() {
        if let Some(alias) = &storage.alias_of {
            let target = module
                .storage()
                .get(alias.storage.get() as usize)
                .ok_or(VerificationProblem::InvalidStorage)?;
            validate_extent(alias.offset, alias.length, target.size)
                .map_err(VerificationProblem::Model)?;
        }
    }
    let storage_bytes = module
        .storage()
        .iter()
        .try_fold(0u64, |sum, storage| sum.checked_add(storage.size))
        .ok_or(VerificationProblem::ResourceOverflow)?;
    Ok(VerificationReport {
        operation_count,
        value_count: values_seen,
        storage_bytes,
        operations: identities,
    })
}

pub fn verify_legal(
    module: Module,
    catalog: &OperationCatalog,
    profile: &LegalityProfile,
) -> Result<LegalModule, VerificationProblem> {
    let report = verify(&module, catalog)?;
    for identity in &report.operations {
        if !profile.allowed_operations.contains(identity) {
            return Err(VerificationProblem::IllegalOperation(identity.clone()));
        }
        let schema = catalog
            .get(identity)
            .ok_or_else(|| VerificationProblem::UnknownOperation(identity.clone()))?;
        if !schema.executable {
            return Err(VerificationProblem::AnalysisOnlyOperation(identity.clone()));
        }
        if let Some(runtime_import) = &schema.runtime_import
            && !profile.allowed_runtime_imports.contains(runtime_import)
        {
            return Err(VerificationProblem::MissingRuntimeImport(
                runtime_import.clone(),
            ));
        }
    }
    Ok(LegalModule { module, report })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationProblem {
    EmptyModule,
    EmptyRegion,
    EmptyBlock,
    UnknownOperation(OperationIdentity),
    SchemaMismatch(OperationIdentity),
    EffectMismatch(OperationIdentity),
    UseBeforeDefinition,
    NonCanonicalValueId,
    InvalidStorage,
    TerminatorMismatch(OperationIdentity),
    IllegalOperation(OperationIdentity),
    AnalysisOnlyOperation(OperationIdentity),
    MissingRuntimeImport(String),
    ResourceOverflow,
    Model(IrProblem),
}

impl fmt::Display for VerificationProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IR verification failed: {self:?}")
    }
}

impl std::error::Error for VerificationProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, IrLimits, ModuleBuilder, OperationSchema};
    use std::collections::BTreeMap;

    #[test]
    fn unknown_operation_fails_closed() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new("test", "return", 1).unwrap();
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let module = builder.finish().unwrap();
        assert_eq!(
            verify(&module, &OperationCatalog::default()),
            Err(VerificationProblem::UnknownOperation(identity))
        );
    }

    #[test]
    fn legal_module_requires_registered_terminator() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new("test", "return", 1).unwrap();
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let module = builder.finish().unwrap();
        let mut schema = OperationSchema::pure(identity.clone(), 0, 0);
        schema.terminator = true;
        let mut catalog = OperationCatalog::default();
        catalog.register(schema).unwrap();
        let profile = LegalityProfile {
            allowed_operations: BTreeSet::from([identity]),
            allowed_runtime_imports: BTreeSet::new(),
        };
        assert!(verify_legal(module, &catalog, &profile).is_ok());
    }

    #[test]
    fn analysis_only_operation_cannot_legalize() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new("test", "unknown", 1).unwrap();
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let module = builder.finish().unwrap();
        let mut schema = OperationSchema::pure(identity.clone(), 0, 0);
        schema.terminator = true;
        schema.executable = false;
        let mut catalog = OperationCatalog::default();
        catalog.register(schema).unwrap();
        let profile = LegalityProfile {
            allowed_operations: BTreeSet::from([identity.clone()]),
            allowed_runtime_imports: BTreeSet::new(),
        };
        assert_eq!(
            verify_legal(module, &catalog, &profile),
            Err(VerificationProblem::AnalysisOnlyOperation(identity))
        );
    }

    #[test]
    fn attribute_variant_is_owned() {
        assert_eq!(Attribute::Boolean(true), Attribute::Boolean(true));
    }
}
