use crate::{CobolHir, StatementKind};
use mainframe_env_ir::{
    Attribute, Effect, IrLimits, LegalityProfile, Module, ModuleBuilder, OperationCatalog,
    OperationIdentity, OperationSchema, StorageReference,
};
use std::collections::{BTreeMap, BTreeSet};

pub const CORE_NAMESPACE: &str = "mainframe.core.cobol";

pub(crate) fn lower_to_core(hir: &CobolHir, limits: IrLimits) -> Result<Module, LowerProblem> {
    if !hir.unsupported().is_empty() {
        return Err(LowerProblem::UnsupportedConstruct);
    }
    let mut builder = ModuleBuilder::new(limits);
    let mut storage = BTreeMap::new();
    for layout in hir.layouts.iter().filter(|layout| layout.length > 0) {
        let alias = layout
            .alias_of
            .as_ref()
            .and_then(|name| storage.get(name))
            .map(|id| StorageReference {
                storage: *id,
                offset: 0,
                length: layout.length as u64,
            });
        let id = builder
            .add_storage(
                layout.qualified_name.to_ascii_lowercase(),
                layout.length as u64,
                alias,
            )
            .map_err(|_| LowerProblem::InvalidLayout)?;
        storage.insert(layout.qualified_name.clone(), id);
    }
    let region = builder
        .add_region()
        .map_err(|_| LowerProblem::LimitExceeded)?;
    let block = builder
        .add_block(region)
        .map_err(|_| LowerProblem::LimitExceeded)?;
    for layout in hir
        .layouts
        .iter()
        .filter(|layout| layout.length > 0 && layout.alias_of.is_none())
    {
        let id = *storage
            .get(&layout.qualified_name)
            .ok_or(LowerProblem::InvalidLayout)?;
        let attributes = BTreeMap::from([
            (
                "name".into(),
                Attribute::Text(layout.qualified_name.clone()),
            ),
            ("initial".into(), Attribute::Bytes(layout.initial.clone())),
            ("offset".into(), Attribute::Integer(layout.offset as i64)),
            ("length".into(), Attribute::Integer(layout.length as i64)),
        ]);
        builder
            .add_operation(
                block,
                core_identity("init")?,
                Vec::new(),
                0,
                attributes,
                vec![Effect::MemoryWrite],
                vec![StorageReference {
                    storage: id,
                    offset: 0,
                    length: layout.length as u64,
                }],
                None,
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    for statement in &hir.statements {
        let name = match statement.kind {
            StatementKind::ProgramEnd => "halt",
            other => other.slug(),
        };
        let mut attributes =
            BTreeMap::from([("line".into(), Attribute::Integer(statement.line as i64))]);
        for (index, argument) in statement.arguments.iter().enumerate() {
            attributes.insert(format!("arg_{index:03}"), Attribute::Text(argument.clone()));
        }
        let mut references = Vec::new();
        let mut seen = BTreeSet::new();
        for argument in &statement.arguments {
            let normalized = argument.trim_matches(['\'', '"']).to_ascii_uppercase();
            if seen.insert(normalized.clone())
                && let Some(layout) = hir
                    .layouts
                    .iter()
                    .find(|layout| layout.name == normalized && layout.length > 0)
            {
                let id = *storage
                    .get(&layout.qualified_name)
                    .ok_or(LowerProblem::InvalidLayout)?;
                references.push(StorageReference {
                    storage: id,
                    offset: 0,
                    length: layout.length as u64,
                });
            }
        }
        builder
            .add_operation(
                block,
                core_identity(name)?,
                Vec::new(),
                0,
                attributes,
                crate::hir::effects(statement.kind),
                references,
                None,
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    builder.finish().map_err(|_| LowerProblem::LimitExceeded)
}

pub fn core_mir_catalog() -> OperationCatalog {
    let mut catalog = OperationCatalog::default();
    let mut init = OperationSchema::pure(core_identity("init").expect("static identity"), 0, 0);
    init.allowed_effects = BTreeSet::from([Effect::MemoryWrite]);
    catalog.register(init).expect("unique init");
    for kind in StatementKind::frozen()
        .into_iter()
        .filter(|kind| kind.supported())
        .chain([StatementKind::Label, StatementKind::ProgramEnd])
    {
        let name = if kind == StatementKind::ProgramEnd {
            "halt"
        } else {
            kind.slug()
        };
        let mut schema = OperationSchema::pure(core_identity(name).expect("static identity"), 0, 0);
        schema.allowed_effects = crate::hir::effects(kind).into_iter().collect();
        schema.terminator = kind == StatementKind::ProgramEnd;
        schema.runtime_import = runtime_import(kind).map(str::to_string);
        catalog.register(schema).expect("unique core operation");
    }
    catalog
}

pub fn core_mir_profile() -> LegalityProfile {
    let catalog = core_mir_catalog();
    LegalityProfile {
        allowed_operations: catalog.identities().cloned().collect(),
        allowed_runtime_imports: ["host.terminal", "host.program", "host.dataset", "host.cics"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn runtime_import(kind: StatementKind) -> Option<&'static str> {
    use StatementKind as K;
    match kind {
        K::Accept | K::Display => Some("host.terminal"),
        K::Call | K::Cancel => Some("host.program"),
        K::Open | K::Close | K::Read | K::Write => Some("host.dataset"),
        K::ExecCics => Some("host.cics"),
        _ => None,
    }
}

fn core_identity(name: &str) -> Result<OperationIdentity, LowerProblem> {
    OperationIdentity::new(CORE_NAMESPACE, name, 1).map_err(|_| LowerProblem::InvalidOperation)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LowerProblem {
    UnsupportedConstruct,
    InvalidLayout,
    InvalidOperation,
    LimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_catalog_has_no_unsupported_frozen_operations() {
        let catalog = core_mir_catalog();
        assert!(catalog.get(&core_identity("exec_sql").unwrap()).is_none());
        assert!(catalog.get(&core_identity("display").unwrap()).is_some());
    }
}
