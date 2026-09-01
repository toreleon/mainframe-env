use crate::{CobolHir, ControlEdgeKind, ControlRole, ControlScope, DataCategory, StatementKind};
use mainframe_env_ir::{
    Attribute, Effect, IrLimits, LegalityProfile, Module, ModuleBuilder, OperationCatalog,
    OperationIdentity, OperationSchema, StorageReference,
};
use std::collections::{BTreeMap, BTreeSet};

pub const CORE_NAMESPACE: &str = "mainframe.core.cobol";

pub(crate) fn lower_to_core(hir: &CobolHir, limits: IrLimits) -> Result<Module, LowerProblem> {
    let mut unsupported = hir.unsupported();
    let structured = hir.control_nodes.iter().any(|start| {
        start.role == ControlRole::BlockStart
            && hir
                .control_nodes
                .iter()
                .any(|end| end.role == ControlRole::BlockEnd && end.parent == Some(start.id))
    });
    if structured {
        unsupported.remove(&StatementKind::NextSentence);
    }
    if !unsupported.is_empty() {
        return Err(LowerProblem::UnsupportedConstruct);
    }
    let mut builder = ModuleBuilder::new(limits);
    let mut storage = BTreeMap::new();
    let storage_bytes = hir
        .layouts
        .iter()
        .map(|layout| layout.offset.saturating_add(layout.length))
        .max()
        .unwrap_or(0);
    let program_storage = (storage_bytes > 0)
        .then(|| builder.add_storage("__program_storage", storage_bytes as u64, None))
        .transpose()
        .map_err(|_| LowerProblem::InvalidLayout)?;
    for layout in hir.layouts.iter().filter(|layout| layout.length > 0) {
        let alias = program_storage.map(|id| StorageReference {
            storage: id,
            offset: layout.offset as u64,
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
    for layout in &hir.layouts {
        let attributes = BTreeMap::from([
            (
                "name".into(),
                Attribute::Text(layout.qualified_name.clone()),
            ),
            ("simple_name".into(), Attribute::Text(layout.name.clone())),
            (
                "category".into(),
                Attribute::Text(category_slug(layout.category).into()),
            ),
            (
                "picture".into(),
                Attribute::Text(layout.picture.clone().unwrap_or_default()),
            ),
            ("digits".into(), Attribute::Integer(layout.digits as i64)),
            ("scale".into(), Attribute::Integer(layout.scale as i64)),
            (
                "signed".into(),
                Attribute::Integer(i64::from(layout.signed)),
            ),
            (
                "sign_separate".into(),
                Attribute::Integer(i64::from(layout.sign_separate)),
            ),
            (
                "justified_right".into(),
                Attribute::Integer(i64::from(layout.justified_right)),
            ),
            (
                "section".into(),
                Attribute::Text(
                    match layout.section {
                        crate::StorageSection::File => "file",
                        crate::StorageSection::Working => "working",
                        crate::StorageSection::Local => "local",
                        crate::StorageSection::Linkage => "linkage",
                    }
                    .into(),
                ),
            ),
            ("offset".into(), Attribute::Integer(layout.offset as i64)),
            ("length".into(), Attribute::Integer(layout.length as i64)),
            (
                "element_length".into(),
                Attribute::Integer(layout.element_length as i64),
            ),
            ("occurs".into(), Attribute::Integer(layout.occurs as i64)),
            (
                "parent".into(),
                Attribute::Text(layout.parent.clone().unwrap_or_default()),
            ),
            (
                "condition_values".into(),
                Attribute::Text(layout.condition_values.join("\u{1f}")),
            ),
        ]);
        builder
            .add_operation(
                block,
                core_identity("define")?,
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    for file in &hir.files {
        let attributes = BTreeMap::from([
            ("name".into(), Attribute::Text(file.select_name.clone())),
            (
                "assignment".into(),
                Attribute::Text(file.assignment.clone()),
            ),
            (
                "record_name".into(),
                Attribute::Text(file.record_name.clone().unwrap_or_default()),
            ),
            (
                "organization".into(),
                Attribute::Text(file.organization.clone()),
            ),
            (
                "access_mode".into(),
                Attribute::Text(file.access_mode.clone()),
            ),
            (
                "record_key".into(),
                Attribute::Text(file.record_key.clone().unwrap_or_default()),
            ),
            (
                "alternate_record_keys".into(),
                Attribute::Text(file.alternate_record_keys.join("\u{1f}")),
            ),
            (
                "relative_key".into(),
                Attribute::Text(file.relative_key.clone().unwrap_or_default()),
            ),
            (
                "file_status".into(),
                Attribute::Text(file.file_status.clone().unwrap_or_default()),
            ),
        ]);
        builder
            .add_operation(
                block,
                core_identity("file")?,
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    for layout in hir.layouts.iter().filter(|layout| {
        layout.allocated
            && layout.length > 0
            && layout.parent.is_none()
            && layout.alias_of.is_none()
    }) {
        let id = *storage
            .get(&layout.qualified_name)
            .ok_or(LowerProblem::InvalidLayout)?;
        let attributes = BTreeMap::from([
            (
                "name".into(),
                Attribute::Text(layout.qualified_name.clone()),
            ),
            (
                "initial".into(),
                Attribute::Bytes(runtime_root_initial(layout, &hir.layouts)),
            ),
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
    if structured {
        lower_structured(hir, &mut builder, block, &storage)?;
    } else {
        for statement in &hir.statements {
            lower_statement(
                statement,
                hir,
                &mut builder,
                block,
                &storage,
                BTreeMap::new(),
            )?;
        }
    }
    builder.finish().map_err(|_| LowerProblem::LimitExceeded)
}

fn lower_structured(
    hir: &CobolHir,
    builder: &mut ModuleBuilder,
    block: mainframe_env_ir::BlockId,
    storage: &BTreeMap<String, mainframe_env_ir::StorageId>,
) -> Result<(), LowerProblem> {
    for node in &hir.control_nodes {
        let mut control = BTreeMap::from([
            ("control_node".into(), Attribute::Integer(node.id as i64)),
            (
                "control_role".into(),
                Attribute::Text(control_role_slug(node.role).into()),
            ),
            (
                "control_scope".into(),
                Attribute::Text(node.scope.map(control_scope_slug).unwrap_or("").into()),
            ),
            (
                "control_parent".into(),
                Attribute::Integer(node.parent.map_or(-1, |parent| parent as i64)),
            ),
            ("control_text".into(), Attribute::Text(node.text.clone())),
        ]);
        for edge in hir.control_edges.iter().filter(|edge| edge.from == node.id) {
            control
                .entry(edge_attribute(edge.kind).into())
                .or_insert(Attribute::Integer(edge.to as i64));
        }
        if node.role == ControlRole::Branch {
            let false_target = hir
                .control_nodes
                .iter()
                .skip(node.id + 1)
                .find(|candidate| {
                    candidate.parent == node.parent
                        && matches!(
                            candidate.role,
                            ControlRole::Branch | ControlRole::BlockEnd | ControlRole::Terminator
                        )
                })
                .map(|candidate| candidate.id)
                .ok_or_else(|| {
                    LowerProblem::InvalidControl(format!(
                        "branch node {} parent {:?} has no following sibling: {}",
                        node.id, node.parent, node.text
                    ))
                })?;
            control.insert(
                "edge_branch_false".into(),
                Attribute::Integer(false_target as i64),
            );
            if node
                .text
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("WHEN ")
                && hir.control_nodes.get(node.id + 1).is_some_and(|candidate| {
                    candidate.parent == node.parent && candidate.role == ControlRole::Branch
                })
                && let Some(true_target) =
                    hir.control_nodes
                        .iter()
                        .skip(node.id + 1)
                        .find(|candidate| {
                            candidate.parent != node.parent || candidate.role != ControlRole::Branch
                        })
            {
                control.insert(
                    "edge_branch_true".into(),
                    Attribute::Integer(true_target.id as i64),
                );
            }
        }
        if let Some(statement) = node.statement.and_then(|index| hir.statements.get(index)) {
            lower_statement(statement, hir, builder, block, storage, control)?;
        } else {
            builder
                .add_operation(
                    block,
                    core_identity("control")?,
                    Vec::new(),
                    0,
                    control,
                    vec![Effect::ProgramControl, Effect::Condition],
                    Vec::new(),
                    None,
                )
                .map_err(|_| LowerProblem::LimitExceeded)?;
        }
    }
    builder
        .add_operation(
            block,
            core_identity("halt")?,
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .map_err(|_| LowerProblem::LimitExceeded)?;
    Ok(())
}

fn runtime_root_initial(root: &crate::CobolLayout, layouts: &[crate::CobolLayout]) -> Vec<u8> {
    if !matches!(
        root.category,
        DataCategory::Group | DataCategory::NationalGroup | DataCategory::Utf8Group
    ) {
        return root.initial.clone();
    }
    let mut element = vec![b' '; root.element_length];
    for child in layouts.iter().filter(|layout| {
        layout.parent.as_deref() == Some(root.qualified_name.as_str())
            && layout.allocated
            && layout.alias_of.is_none()
            && layout.length > 0
    }) {
        let Some(relative) = child.offset.checked_sub(root.offset) else {
            continue;
        };
        if relative >= element.len() {
            continue;
        }
        let copy = child.initial.len().min(element.len() - relative);
        element[relative..relative + copy].copy_from_slice(&child.initial[..copy]);
    }
    element.repeat(root.occurs)
}

fn lower_statement(
    statement: &crate::HirStatement,
    hir: &CobolHir,
    builder: &mut ModuleBuilder,
    block: mainframe_env_ir::BlockId,
    storage: &BTreeMap<String, mainframe_env_ir::StorageId>,
    mut attributes: BTreeMap<String, Attribute>,
) -> Result<(), LowerProblem> {
    let name = match statement.kind {
        StatementKind::ProgramEnd => "halt",
        other => other.slug(),
    };
    attributes.insert("line".into(), Attribute::Integer(statement.line as i64));
    attributes.insert(
        "arguments".into(),
        Attribute::Bytes(encode_arguments(&statement.arguments)),
    );
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
    Ok(())
}

fn encode_arguments(arguments: &[String]) -> Vec<u8> {
    let mut encoded = Vec::new();
    for argument in arguments {
        encoded.extend_from_slice(&(argument.len() as u64).to_be_bytes());
        encoded.extend_from_slice(argument.as_bytes());
    }
    encoded
}

pub fn core_mir_catalog() -> OperationCatalog {
    let mut catalog = OperationCatalog::default();
    catalog
        .register(OperationSchema::pure(
            core_identity("define").expect("static identity"),
            0,
            0,
        ))
        .expect("unique define");
    catalog
        .register(OperationSchema::pure(
            core_identity("file").expect("static identity"),
            0,
            0,
        ))
        .expect("unique file");
    let mut control =
        OperationSchema::pure(core_identity("control").expect("static identity"), 0, 0);
    control.allowed_effects = BTreeSet::from([Effect::ProgramControl, Effect::Condition]);
    catalog.register(control).expect("unique control");
    let mut init = OperationSchema::pure(core_identity("init").expect("static identity"), 0, 0);
    init.allowed_effects = BTreeSet::from([Effect::MemoryWrite]);
    catalog.register(init).expect("unique init");
    for kind in StatementKind::official()
        .into_iter()
        .filter(|kind| kind.supported())
        .chain([
            StatementKind::NextSentence,
            StatementKind::ExecCics,
            StatementKind::ExecDli,
            StatementKind::ExecSql,
            StatementKind::Label,
            StatementKind::ProgramEnd,
        ])
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

const fn control_role_slug(role: ControlRole) -> &'static str {
    match role {
        ControlRole::Statement => "statement",
        ControlRole::BlockStart => "block_start",
        ControlRole::Branch => "branch",
        ControlRole::BlockEnd => "block_end",
        ControlRole::Label => "label",
        ControlRole::ExternalTarget => "external_target",
        ControlRole::Recovered => "recovered",
        ControlRole::Transfer => "transfer",
        ControlRole::Terminator => "terminator",
    }
}

const fn control_scope_slug(scope: ControlScope) -> &'static str {
    match scope {
        ControlScope::If => "if",
        ControlScope::Evaluate => "evaluate",
        ControlScope::Search => "search",
        ControlScope::Perform => "perform",
    }
}

const fn edge_attribute(kind: ControlEdgeKind) -> &'static str {
    match kind {
        ControlEdgeKind::Fallthrough => "edge_fallthrough",
        ControlEdgeKind::Branch => "edge_branch",
        ControlEdgeKind::True => "edge_true",
        ControlEdgeKind::False => "edge_false",
        ControlEdgeKind::Loop => "edge_loop",
        ControlEdgeKind::Call => "edge_call",
        ControlEdgeKind::Transfer => "edge_transfer",
        ControlEdgeKind::Return => "edge_return",
    }
}

const fn category_slug(category: DataCategory) -> &'static str {
    match category {
        DataCategory::Alphabetic => "alphabetic",
        DataCategory::Alphanumeric => "alphanumeric",
        DataCategory::AlphanumericEdited => "alphanumeric_edited",
        DataCategory::Dbcs => "dbcs",
        DataCategory::National => "national",
        DataCategory::NationalEdited => "national_edited",
        DataCategory::Utf8 => "utf8",
        DataCategory::NumericDisplay => "numeric_display",
        DataCategory::NumericEdited => "numeric_edited",
        DataCategory::PackedDecimal => "packed_decimal",
        DataCategory::Binary => "binary",
        DataCategory::FloatShort => "float_short",
        DataCategory::FloatLong => "float_long",
        DataCategory::Index => "index",
        DataCategory::Pointer => "pointer",
        DataCategory::Pointer32 => "pointer_32",
        DataCategory::ProcedurePointer => "procedure_pointer",
        DataCategory::FunctionPointer => "function_pointer",
        DataCategory::ObjectReference => "object_reference",
        DataCategory::Group => "group",
        DataCategory::NationalGroup => "national_group",
        DataCategory::Utf8Group => "utf8_group",
        DataCategory::Condition => "condition",
        DataCategory::Rename => "rename",
    }
}

pub fn core_mir_profile() -> LegalityProfile {
    let catalog = core_mir_catalog();
    LegalityProfile {
        allowed_operations: catalog.identities().cloned().collect(),
        allowed_runtime_imports: [
            "host.terminal",
            "host.program",
            "host.dataset",
            "host.cics",
            "host.db2",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    }
}

fn runtime_import(kind: StatementKind) -> Option<&'static str> {
    use StatementKind as K;
    match kind {
        K::Accept | K::Display => Some("host.terminal"),
        K::Call | K::Cancel | K::ExecDli => Some("host.program"),
        K::ExecSql => Some("host.db2"),
        K::Open | K::Close | K::Read | K::Rewrite | K::Write => Some("host.dataset"),
        K::ExecCics => Some("host.cics"),
        _ => None,
    }
}

fn core_identity(name: &str) -> Result<OperationIdentity, LowerProblem> {
    OperationIdentity::new(CORE_NAMESPACE, name, 1).map_err(|_| LowerProblem::InvalidOperation)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LowerProblem {
    UnsupportedConstruct,
    InvalidLayout,
    InvalidOperation,
    InvalidControl(String),
    LimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_catalog_has_no_unsupported_frozen_operations() {
        let catalog = core_mir_catalog();
        assert!(catalog.get(&core_identity("exec_sql").unwrap()).is_some());
        assert!(catalog.get(&core_identity("display").unwrap()).is_some());
    }
}
