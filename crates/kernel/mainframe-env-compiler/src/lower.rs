use crate::service::VerifiedCobolHir;
use crate::{CobolHir, ControlEdgeKind, ControlRole, ControlScope, DataCategory, StatementKind};
use mainframe_env_ir::{
    Attribute, COBOL_ENTRY_FORMALS_V1, COBOL_MAX_UNBOUNDED_OCCURRENCES,
    COBOL_MAX_UNBOUNDED_STORAGE_BYTES, Effect, IrLimits, LegalityProfile, Module, ModuleBuilder,
    OperationCatalog, OperationIdentity, OperationSchema, StorageReference,
    cobol_layout_definition_schema,
};
use std::collections::{BTreeMap, BTreeSet};

pub const CORE_NAMESPACE: &str = "mainframe.core.cobol";
const SIZE_ERROR_BRANCH: i64 = 1;
const NOT_SIZE_ERROR_BRANCH: i64 = 2;
pub const PUBLISHABLE_LAYOUT_CATEGORIES: &[&str] = &[
    "alphabetic",
    "alphanumeric",
    "alphanumeric_edited",
    "binary",
    "condition",
    "dbcs",
    "float_long",
    "float_short",
    "function_pointer",
    "group",
    "index",
    "national",
    "national_edited",
    "national_group",
    "numeric_display",
    "numeric_edited",
    "object_reference",
    "packed_decimal",
    "pointer",
    "pointer_32",
    "procedure_pointer",
    "rename",
    "utf8",
    "utf8_group",
];

pub(crate) fn lower_to_core(
    verified: &VerifiedCobolHir<'_>,
    arithmetic_mode: &str,
    display_sign: &str,
    address_mode: u8,
    declaratives: &[(String, Vec<String>)],
    limits: IrLimits,
) -> Result<Module, LowerProblem> {
    let hir = verified.hir();
    let mut unsupported = hir.unsupported();
    let structured = hir.control_nodes.iter().any(|node| {
        matches!(
            node.role,
            ControlRole::BlockStart
                | ControlRole::BlockEnd
                | ControlRole::Branch
                | ControlRole::Terminator
        )
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
        .filter(|layout| {
            !layout.typedef && !layout.dynamic && unbounded_ancestor(layout, &hir.layouts).is_none()
        })
        .map(|layout| layout.offset.saturating_add(layout.length))
        .max()
        .unwrap_or(0);
    let program_storage = (storage_bytes > 0)
        .then(|| builder.add_storage("__program_storage", storage_bytes as u64, None))
        .transpose()
        .map_err(|_| LowerProblem::InvalidLayout)?;
    for layout in hir.layouts.iter().filter(|layout| {
        !layout.typedef && (layout.length > 0 || layout.dynamic || layout.unbounded)
    }) {
        let unbounded_parent = unbounded_ancestor(layout, &hir.layouts);
        let storage_length = if layout.dynamic {
            layout.dynamic_limit.ok_or(LowerProblem::InvalidLayout)?
        } else if layout.unbounded {
            unbounded_storage_length(layout)?
        } else {
            layout.length
        };
        let alias = if let Some(parent) = unbounded_parent {
            Some(StorageReference {
                storage: *storage
                    .get(&parent.qualified_name)
                    .ok_or(LowerProblem::InvalidLayout)?,
                offset: layout.offset.saturating_sub(parent.offset) as u64,
                length: layout.length as u64,
            })
        } else if !layout.dynamic && !layout.unbounded {
            program_storage.map(|id| StorageReference {
                storage: id,
                offset: layout.offset as u64,
                length: layout.length as u64,
            })
        } else {
            None
        };
        let id = builder
            .add_storage(
                layout.qualified_name.to_ascii_lowercase(),
                storage_length as u64,
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
    builder
        .add_operation(
            block,
            core_identity("config")?,
            Vec::new(),
            0,
            BTreeMap::from([
                (
                    "arithmetic_mode".into(),
                    Attribute::Text(arithmetic_mode.into()),
                ),
                ("display_sign".into(), Attribute::Text(display_sign.into())),
                (
                    COBOL_ENTRY_FORMALS_V1.into(),
                    Attribute::Text(hir.entry_formals.join("\u{1f}")),
                ),
                (
                    "address_mode".into(),
                    Attribute::Text(address_mode.to_string()),
                ),
                (
                    "program_lifecycle".into(),
                    Attribute::Text(hir.program_lifecycle.clone()),
                ),
                (
                    "declaratives".into(),
                    Attribute::Text(
                        declaratives
                            .iter()
                            .map(|(section, operands)| {
                                std::iter::once(section.as_str())
                                    .chain(operands.iter().map(String::as_str))
                                    .collect::<Vec<_>>()
                                    .join("\u{1f}")
                            })
                            .collect::<Vec<_>>()
                            .join("\u{1e}"),
                    ),
                ),
            ]),
            Vec::new(),
            Vec::new(),
            None,
        )
        .map_err(|_| LowerProblem::LimitExceeded)?;
    for layout in hir.layouts.iter().filter(|layout| !layout.typedef) {
        let attributes = BTreeMap::from([
            (
                "name".into(),
                Attribute::Text(layout.qualified_name.clone()),
            ),
            ("simple_name".into(), Attribute::Text(layout.name.clone())),
            (
                "category".into(),
                Attribute::Text(layout.category.executable_name().into()),
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
                "blank_when_zero".into(),
                Attribute::Integer(i64::from(layout.blank_when_zero)),
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
            (
                "length".into(),
                Attribute::Integer(if layout.unbounded {
                    unbounded_storage_length(layout)? as i64
                } else {
                    layout.length as i64
                }),
            ),
            (
                "element_length".into(),
                Attribute::Integer(layout.element_length as i64),
            ),
            (
                "byte_length".into(),
                Attribute::Integer(layout.byte_length.unwrap_or_default() as i64),
            ),
            (
                "occurs".into(),
                Attribute::Integer(if layout.unbounded {
                    unbounded_occurrences(layout)? as i64
                } else {
                    layout.occurs as i64
                }),
            ),
            (
                "occurs_clause".into(),
                Attribute::Integer(i64::from(layout.occurs_clause)),
            ),
            (
                "occurs_min".into(),
                Attribute::Integer(layout.occurs_min as i64),
            ),
            (
                "unbounded".into(),
                Attribute::Integer(i64::from(layout.unbounded)),
            ),
            (
                "depending_on".into(),
                Attribute::Text(layout.depending_on.clone().unwrap_or_default()),
            ),
            (
                "indexes".into(),
                Attribute::Text(layout.indexes.join("\u{1f}")),
            ),
            (
                "keys".into(),
                Attribute::Text(
                    layout
                        .keys
                        .iter()
                        .map(|key| {
                            format!("{}:{}", if key.descending { "D" } else { "A" }, key.name)
                        })
                        .collect::<Vec<_>>()
                        .join("\u{1f}"),
                ),
            ),
            (
                "dynamic".into(),
                Attribute::Integer(i64::from(layout.dynamic)),
            ),
            (
                "dynamic_limit".into(),
                Attribute::Integer(layout.dynamic_limit.unwrap_or_default() as i64),
            ),
            (
                "parent".into(),
                Attribute::Text(layout.parent.clone().unwrap_or_default()),
            ),
            (
                "alias_of".into(),
                Attribute::Text(layout.alias_of.clone().unwrap_or_default()),
            ),
            (
                "rename_through".into(),
                Attribute::Text(layout.rename_through.clone().unwrap_or_default()),
            ),
            (
                "condition_values".into(),
                Attribute::Text(layout.condition_values.join("\u{1f}")),
            ),
            (
                "object_class".into(),
                Attribute::Text(layout.object_class.clone().unwrap_or_default()),
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
            (
                "sort_merge".into(),
                Attribute::Integer(i64::from(file.sort_merge)),
            ),
            (
                "description".into(),
                Attribute::Text(file.description.clone()),
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
            && (layout.length > 0 || layout.dynamic || layout.unbounded)
            && (layout.parent.is_none() || layout.unbounded)
            && layout.alias_of.is_none()
    }) {
        let id = *storage
            .get(&layout.qualified_name)
            .ok_or(LowerProblem::InvalidLayout)?;
        let storage_length = if layout.dynamic {
            layout.dynamic_limit.ok_or(LowerProblem::InvalidLayout)?
        } else if layout.unbounded {
            unbounded_storage_length(layout)?
        } else {
            layout.length
        };
        let attributes = BTreeMap::from([
            (
                "name".into(),
                Attribute::Text(layout.qualified_name.clone()),
            ),
            (
                "initial".into(),
                Attribute::Bytes(runtime_root_initial(
                    layout,
                    &hir.layouts,
                    if layout.unbounded {
                        unbounded_occurrences(layout)?
                    } else {
                        layout.occurs
                    },
                )),
            ),
            ("offset".into(), Attribute::Integer(layout.offset as i64)),
            ("length".into(), Attribute::Integer(storage_length as i64)),
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
                    length: storage_length as u64,
                }],
                None,
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    if structured {
        lower_structured(hir, &mut builder, block, &storage, arithmetic_mode)?;
    } else {
        for statement in &hir.statements {
            lower_statement(
                statement,
                hir,
                &mut builder,
                block,
                &storage,
                BTreeMap::new(),
                arithmetic_mode,
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
    arithmetic_mode: &str,
) -> Result<(), LowerProblem> {
    let typed_size_error_branches = typed_size_error_branch_polarities(hir)?;
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
        if let Some(polarity) = typed_size_error_branches.get(&node.id) {
            control.insert(
                crate::hir::decimal::CONDITION_STATUS_ATTRIBUTE.into(),
                Attribute::Text(crate::hir::decimal::SIZE_ERROR_STATUS.into()),
            );
            control.insert(
                crate::hir::decimal::CONDITION_POLARITY_ATTRIBUTE.into(),
                Attribute::Boolean(*polarity),
            );
        }
        if let Some(statement) = node.statement.and_then(|index| hir.statements.get(index)) {
            lower_statement(
                statement,
                hir,
                builder,
                block,
                storage,
                control,
                arithmetic_mode,
            )?;
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

fn runtime_root_initial(
    root: &crate::CobolLayout,
    layouts: &[crate::CobolLayout],
    occurrences: usize,
) -> Vec<u8> {
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
    element.repeat(occurrences)
}

fn unbounded_occurrences(layout: &crate::CobolLayout) -> Result<usize, LowerProblem> {
    if !layout.unbounded || layout.element_length == 0 {
        return Err(LowerProblem::InvalidLayout);
    }
    COBOL_MAX_UNBOUNDED_OCCURRENCES
        .min(
            COBOL_MAX_UNBOUNDED_STORAGE_BYTES
                .checked_div(layout.element_length)
                .ok_or(LowerProblem::InvalidLayout)?,
        )
        .checked_sub(layout.occurs_min)
        .map(|remaining| remaining + layout.occurs_min)
        .ok_or(LowerProblem::InvalidLayout)
}

fn unbounded_storage_length(layout: &crate::CobolLayout) -> Result<usize, LowerProblem> {
    layout
        .element_length
        .checked_mul(unbounded_occurrences(layout)?)
        .ok_or(LowerProblem::InvalidLayout)
}

fn unbounded_ancestor<'a>(
    layout: &crate::CobolLayout,
    layouts: &'a [crate::CobolLayout],
) -> Option<&'a crate::CobolLayout> {
    let mut parent = layout.parent.as_deref();
    while let Some(name) = parent {
        let candidate = layouts.iter().find(|item| item.qualified_name == name)?;
        if candidate.unbounded {
            return Some(candidate);
        }
        parent = candidate.parent.as_deref();
    }
    None
}

fn lower_statement(
    statement: &crate::HirStatement,
    hir: &CobolHir,
    builder: &mut ModuleBuilder,
    block: mainframe_env_ir::BlockId,
    storage: &BTreeMap<String, mainframe_env_ir::StorageId>,
    attributes: BTreeMap<String, Attribute>,
    arithmetic_mode: &str,
) -> Result<(), LowerProblem> {
    if let Some(resolved) = statement.resolved.as_ref()
        && matches!(
            resolved,
            crate::HirResolvedStatement::Add(_) | crate::HirResolvedStatement::Compute(_)
        )
    {
        let policy = crate::hir::decimal::execution_policy(arithmetic_mode)
            .map_err(|_| LowerProblem::InvalidOperation)?;
        let encoded = crate::hir::decimal::encode_statement(resolved, storage, policy)
            .map_err(|_| LowerProblem::InvalidOperation)?;
        let mut operation_attributes = attributes;
        operation_attributes.remove("control_text");
        let branches = typed_size_error_branch_mask(resolved)?;
        operation_attributes.insert(
            crate::hir::decimal::CONDITION_STATUS_ATTRIBUTE.into(),
            Attribute::Text(crate::hir::decimal::SIZE_ERROR_STATUS.into()),
        );
        operation_attributes.insert(
            crate::hir::decimal::CONDITION_BRANCHES_ATTRIBUTE.into(),
            Attribute::Integer(branches),
        );
        operation_attributes.insert("line".into(), Attribute::Integer(statement.line as i64));
        operation_attributes.insert(
            crate::hir::decimal::ASSIGNMENT_PLAN_ATTRIBUTE.into(),
            Attribute::Bytes(encoded.bytes),
        );
        builder
            .add_operation(
                block,
                crate::hir::decimal::executable_identity(),
                Vec::new(),
                0,
                operation_attributes,
                crate::hir::effects(statement.kind),
                encoded.storage,
                statement.location.clone(),
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
        return Ok(());
    }
    if let Some(crate::HirResolvedStatement::Cics(command)) = statement.resolved.as_ref() {
        let encoded = crate::hir::cics::encode_statement(command, storage)
            .map_err(|_| LowerProblem::InvalidOperation)?;
        let mut operation_attributes = attributes;
        operation_attributes.remove("control_text");
        operation_attributes.insert("line".into(), Attribute::Integer(statement.line as i64));
        operation_attributes.insert(
            crate::hir::cics::CICS_PLAN_ATTRIBUTE.into(),
            Attribute::Bytes(encoded.bytes),
        );
        builder
            .add_operation(
                block,
                crate::hir::cics::executable_identity(command.operation),
                Vec::new(),
                0,
                operation_attributes,
                crate::hir::cics::operation_effects(command.operation),
                encoded.storage,
                statement.location.clone(),
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
        return Ok(());
    }
    let name = match statement.kind {
        StatementKind::ProgramEnd => "halt",
        other => other.slug(),
    };
    let groups = statement_argument_groups(statement)?;
    let last = groups.len().saturating_sub(1);
    for (index, arguments) in groups.into_iter().enumerate() {
        let mut operation_attributes = attributes.clone();
        operation_attributes.insert("line".into(), Attribute::Integer(statement.line as i64));
        operation_attributes.insert(
            "arguments".into(),
            Attribute::Bytes(encode_arguments(&arguments)),
        );
        if index > 0 {
            for key in [
                "control_node",
                "control_role",
                "control_scope",
                "control_parent",
            ] {
                operation_attributes.remove(key);
            }
        }
        if index < last {
            operation_attributes.retain(|key, _| !key.starts_with("edge_"));
        }
        let mut references = Vec::new();
        let mut seen = BTreeSet::new();
        for argument in &arguments {
            let normalized = argument.trim_matches(['\'', '"']).to_ascii_uppercase();
            if seen.insert(normalized.clone())
                && let Some(layout) = hir.layouts.iter().find(|layout| {
                    !layout.typedef && layout.name == normalized && layout.length > 0
                })
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
                operation_attributes,
                crate::hir::effects(statement.kind),
                references,
                statement.location.clone(),
            )
            .map_err(|_| LowerProblem::LimitExceeded)?;
    }
    Ok(())
}

fn typed_size_error_branch_mask(
    resolved: &crate::HirResolvedStatement,
) -> Result<i64, LowerProblem> {
    let policy = match resolved {
        crate::HirResolvedStatement::Add(add) => add.size_error,
        crate::HirResolvedStatement::Compute(compute) => compute.size_error,
        crate::HirResolvedStatement::Cics(_) => return Err(LowerProblem::InvalidOperation),
    };
    Ok((i64::from(policy.on_size_error) * SIZE_ERROR_BRANCH)
        | (i64::from(policy.not_on_size_error) * NOT_SIZE_ERROR_BRANCH))
}

fn typed_size_error_branch_polarities(
    hir: &CobolHir,
) -> Result<BTreeMap<usize, bool>, LowerProblem> {
    let mut children = BTreeMap::<usize, Vec<usize>>::new();
    for branch in hir
        .control_nodes
        .iter()
        .filter(|node| node.role == ControlRole::Branch)
    {
        if let Some(parent) = branch.parent {
            children.entry(parent).or_default().push(branch.id);
        }
    }
    let mut polarities = BTreeMap::new();
    for parent in &hir.control_nodes {
        let Some(statement) = parent.statement.and_then(|index| hir.statements.get(index)) else {
            continue;
        };
        let Some(resolved) = statement.resolved.as_ref().filter(|resolved| {
            matches!(
                resolved,
                crate::HirResolvedStatement::Add(_) | crate::HirResolvedStatement::Compute(_)
            )
        }) else {
            continue;
        };
        let mask = typed_size_error_branch_mask(resolved)?;
        let expected = [
            (mask & SIZE_ERROR_BRANCH != 0).then_some(true),
            (mask & NOT_SIZE_ERROR_BRANCH != 0).then_some(false),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let branches = children
            .get(&parent.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if branches.len() != expected.len() {
            return Err(LowerProblem::InvalidControl(format!(
                "typed arithmetic node {} has a drifted condition branch set",
                parent.id
            )));
        }
        polarities.extend(branches.iter().copied().zip(expected));
    }
    Ok(polarities)
}

fn statement_argument_groups(
    statement: &crate::HirStatement,
) -> Result<Vec<Vec<String>>, LowerProblem> {
    if statement.kind == StatementKind::Open {
        let mut groups = Vec::new();
        let mut at = 0usize;
        while at < statement.arguments.len() {
            let mode = statement.arguments[at].clone();
            if !matches!(mode.as_str(), "INPUT" | "OUTPUT" | "I-O" | "EXTEND") {
                return Err(LowerProblem::InvalidOperation);
            }
            at += 1;
            let start = at;
            while at < statement.arguments.len()
                && !matches!(
                    statement.arguments[at].as_str(),
                    "INPUT" | "OUTPUT" | "I-O" | "EXTEND"
                )
            {
                groups.push(vec![mode.clone(), statement.arguments[at].clone()]);
                at += 1;
            }
            if at == start {
                return Err(LowerProblem::InvalidOperation);
            }
        }
        return Ok(groups);
    }
    if statement.kind == StatementKind::Close {
        let mut groups = Vec::new();
        let mut at = 0usize;
        while at < statement.arguments.len() {
            let mut group = vec![statement.arguments[at].clone()];
            at += 1;
            if statement
                .arguments
                .get(at)
                .is_some_and(|token| matches!(token.as_str(), "REEL" | "UNIT"))
            {
                group.push(statement.arguments[at].clone());
                at += 1;
            }
            if statement
                .arguments
                .get(at)
                .is_some_and(|token| token == "WITH")
            {
                group.push(statement.arguments[at].clone());
                at += 1;
                if statement
                    .arguments
                    .get(at)
                    .is_some_and(|token| token == "NO")
                {
                    group.push(statement.arguments[at].clone());
                    at += 1;
                }
                group.push(
                    statement
                        .arguments
                        .get(at)
                        .cloned()
                        .ok_or(LowerProblem::InvalidOperation)?,
                );
                at += 1;
            } else if statement
                .arguments
                .get(at)
                .is_some_and(|token| token == "FOR")
            {
                group.extend_from_slice(
                    statement
                        .arguments
                        .get(at..at + 2)
                        .ok_or(LowerProblem::InvalidOperation)?,
                );
                at += 2;
            }
            groups.push(group);
        }
        return Ok(groups);
    }
    Ok(vec![statement.arguments.clone()])
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
            core_identity("config").expect("static identity"),
            0,
            0,
        ))
        .expect("unique config");
    catalog
        .register(cobol_layout_definition_schema())
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
    crate::hir::decimal::register_executable_operation(&mut catalog);
    crate::hir::cics::register_executable_operations(&mut catalog);
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
