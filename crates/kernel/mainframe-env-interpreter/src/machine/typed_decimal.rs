use super::*;
use crate::runtime::{CobolArithmetic, CobolDecimal, CobolRounding, RuntimeContractProblem};
use mainframe_env_ir::{
    DecimalArithmeticContext, DecimalAssignmentPlan, DecimalConditionContract,
    DecimalConditionPolicy, DecimalExpression, DecimalOperationContract, DecimalPlanLimits,
    DecimalPlanWireVersion, DecimalReceiver, DecimalReceiverUpdatePolicy, DecimalRoundingPolicy,
    DecimalStorageAbi, DecimalStorageSlot, Effect, Module, OperationCatalog, OperationSchema,
    OperationSemanticContract, cobol_layout_definition_identity,
    decimal_assignment_plan_wire_version, decode_decimal_assignment_plan,
    verify_semantic_contracts,
};

const NAMESPACE: &str = "mainframe.decimal";
const NAME: &str = "assign";
const PLAN_ATTRIBUTE: &str = "assignment_plan";
const CONDITION_STATUS_ATTRIBUTE: &str = "typed_condition_status";
const CONDITION_BRANCHES_ATTRIBUTE: &str = "typed_condition_branches";
const CONDITION_POLARITY_ATTRIBUTE: &str = "typed_condition_polarity";
const SIZE_ERROR_STATUS: &str = "cobol.arithmetic-size-error@1";
const SIZE_ERROR_BRANCH: u8 = 1;
const NOT_SIZE_ERROR_BRANCH: u8 = 2;

pub(super) fn operation_identity() -> OperationIdentity {
    OperationIdentity::new(NAMESPACE, NAME, 2).expect("static typed decimal operation")
}

fn legacy_operation_identity() -> OperationIdentity {
    OperationIdentity::new(NAMESPACE, NAME, 1).expect("static legacy decimal operation")
}

pub(super) fn operation_identities() -> [OperationIdentity; 2] {
    [legacy_operation_identity(), operation_identity()]
}

pub(super) fn is_assign(operation: &Operation) -> bool {
    operation.identity.namespace() == NAMESPACE
        && operation.identity.name() == NAME
        && matches!(operation.identity.major(), 1 | 2)
}

pub(super) fn validate_module_operations(module: &Module) -> Result<(), MachineProblem> {
    let mut catalog = OperationCatalog::default();
    for identity in operation_identities() {
        let (expected_plan_version, allowed_semantic_origins) = if identity.major() == 1 {
            (
                DecimalPlanWireVersion::LegacyV1,
                BTreeSet::from(["cobol.add@1".into(), "cobol.compute@1".into()]),
            )
        } else {
            (DecimalPlanWireVersion::PolicyV2, BTreeSet::new())
        };
        let mut schema = OperationSchema::pure(identity, 0, 0);
        schema.semantic_contract =
            OperationSemanticContract::DecimalAssignment(DecimalOperationContract {
                plan_attribute: PLAN_ATTRIBUTE.into(),
                expected_plan_version,
                allowed_semantic_origins,
                layout_definition_operation: Some(cobol_layout_definition_identity()),
                condition: Some(DecimalConditionContract {
                    status: SIZE_ERROR_STATUS.into(),
                    status_attribute: CONDITION_STATUS_ATTRIBUTE.into(),
                    branch_mask_attribute: CONDITION_BRANCHES_ATTRIBUTE.into(),
                    branch_polarity_attribute: CONDITION_POLARITY_ATTRIBUTE.into(),
                }),
            });
        catalog
            .register(schema)
            .expect("unique typed decimal identity");
    }
    verify_semantic_contracts(module, &catalog)
        .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))
}

pub(super) fn validate_machine(machine: &ReferenceMachine) -> Result<(), MachineProblem> {
    let branches_by_owner = machine
        .operations
        .iter()
        .filter(|operation| optional_text_attribute(operation, "control_role") == Some("branch"))
        .filter_map(|operation| {
            optional_integer_attribute(operation, "control_parent")
                .and_then(|owner| usize::try_from(owner).ok())
                .map(|owner| (owner, operation))
        })
        .fold(
            BTreeMap::<usize, Vec<&Operation>>::new(),
            |mut index, (owner, branch)| {
                index.entry(owner).or_default().push(branch);
                index
            },
        );
    for (pc, operation) in machine
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| is_assign(operation))
    {
        let plan = plan(operation)?;
        validate_declared_slots(operation, &plan)?;
        validate_plan_slots(machine, operation, &plan)?;
        validate_condition_edges(operation, &branches_by_owner)?;
        validate_control_node_identity(machine, operation, pc)?;
    }
    for (pc, branch) in machine
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| {
            optional_text_attribute(operation, "control_role") == Some("branch")
                && (operation
                    .attributes
                    .contains_key(CONDITION_STATUS_ATTRIBUTE)
                    || operation
                        .attributes
                        .contains_key(CONDITION_POLARITY_ATTRIBUTE))
        })
    {
        validate_marked_branch(machine, branch, pc)?;
    }
    Ok(())
}

pub(super) fn execute_with_condition(
    machine: &mut ReferenceMachine,
    operation: &Operation,
) -> Result<(), MachineProblem> {
    match execute(machine, operation) {
        Ok(DecimalExecutionStatus::Success) => {
            machine.condition_status.arithmetic_size_error = false
        }
        Ok(DecimalExecutionStatus::ReceiverSizeError) => {
            machine.condition_status.arithmetic_size_error = true
        }
        Err(MachineProblem::SizeError) => {
            machine.condition_status.arithmetic_size_error = true;
            if !has_size_error_handler(operation)? {
                return Err(MachineProblem::SizeError);
            }
        }
        Err(problem) => return Err(problem),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecimalExecutionStatus {
    Success,
    ReceiverSizeError,
}

fn execute(
    machine: &mut ReferenceMachine,
    operation: &Operation,
) -> Result<DecimalExecutionStatus, MachineProblem> {
    let plan = plan(operation)?;
    validate_declared_slots(operation, &plan)?;
    validate_plan_slots(machine, operation, &plan)?;
    let preserve_failed_receiver = has_size_error_handler(operation)?;

    // Both supported policies capture operands before any receiving-field
    // store. The policy then makes conversion failure either historical
    // whole-batch atomicity (@1) or receiver-local with successful stores (@2).
    let evaluated = plan
        .assignments
        .iter()
        .map(|assignment| evaluate(machine, operation, &plan, &assignment.expression))
        .collect::<Result<Vec<_>, _>>()?;
    let mut staged = Vec::with_capacity(plan.assignments.len());
    let mut receiver_size_error = false;
    for (assignment, value) in plan.assignments.iter().zip(evaluated) {
        match stage_receiver(
            machine,
            operation,
            &assignment.receiver,
            value,
            preserve_failed_receiver,
        ) {
            Ok(write) => staged.push(write),
            Err(MachineProblem::SizeError)
                if plan.policy.receiver_update
                    == DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1 =>
            {
                receiver_size_error = true;
                if !preserve_failed_receiver {
                    staged.push(stage_receiver_truncated(
                        machine,
                        operation,
                        &assignment.receiver,
                        value,
                    )?);
                }
            }
            Err(problem) => return Err(problem),
        }
    }
    for (view, bytes) in staged {
        machine.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
    }
    if receiver_size_error {
        Ok(DecimalExecutionStatus::ReceiverSizeError)
    } else {
        Ok(DecimalExecutionStatus::Success)
    }
}

fn plan(operation: &Operation) -> Result<DecimalAssignmentPlan, MachineProblem> {
    if !operation.operands.is_empty()
        || !operation.results.is_empty()
        || operation.effects != [Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]
    {
        return Err(invalid_plan("operation signature or effects are invalid"));
    }
    if operation.attributes.contains_key("arguments")
        || operation.attributes.contains_key("control_text")
        || operation
            .attributes
            .keys()
            .any(|name| name.starts_with("arg_"))
    {
        return Err(invalid_plan(
            "legacy arguments and control text attributes are forbidden",
        ));
    }
    condition_declaration(operation)?;
    let bytes = match operation.attributes.get(PLAN_ATTRIBUTE) {
        Some(Attribute::Bytes(bytes)) => bytes,
        _ => return Err(invalid_plan("missing bytes attribute assignment_plan")),
    };
    let expected_version = match operation.identity.major() {
        1 => DecimalPlanWireVersion::LegacyV1,
        2 => DecimalPlanWireVersion::PolicyV2,
        _ => return Err(invalid_plan("unsupported executable operation version")),
    };
    let actual_version = decimal_assignment_plan_wire_version(bytes)
        .map_err(|problem| invalid_plan(&problem.to_string()))?;
    if actual_version != expected_version {
        return Err(invalid_plan(
            "executable operation and decimal plan versions do not match",
        ));
    }
    let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default())
        .map_err(|problem| invalid_plan(&problem.to_string()))?;
    validate_execution_policy(&plan, expected_version)?;
    if expected_version == DecimalPlanWireVersion::LegacyV1
        && !matches!(
            plan.semantic_origin.as_str(),
            "cobol.add@1" | "cobol.compute@1"
        )
    {
        return Err(invalid_plan(
            "legacy semantic origin is not an approved COBOL operation",
        ));
    }
    Ok(plan)
}

fn validate_execution_policy(
    plan: &DecimalAssignmentPlan,
    version: DecimalPlanWireVersion,
) -> Result<(), MachineProblem> {
    let supported = match version {
        DecimalPlanWireVersion::LegacyV1 => {
            plan.policy.arithmetic_context == DecimalArithmeticContext::LegacyCobolModuleV1
                && plan.policy.storage_abi == DecimalStorageAbi::CobolNumericV1
                && plan.policy.receiver_update
                    == DecimalReceiverUpdatePolicy::CapturedOperandsAtomicV1
                && plan.policy.condition == DecimalConditionPolicy::CobolSizeErrorV1
        }
        DecimalPlanWireVersion::PolicyV2 => {
            matches!(
                plan.policy.arithmetic_context,
                DecimalArithmeticContext::Decimal18V1 | DecimalArithmeticContext::Decimal34V1
            ) && plan.policy.storage_abi == DecimalStorageAbi::CobolNumericV1
                && plan.policy.receiver_update
                    == DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1
                && plan.policy.condition == DecimalConditionPolicy::CobolSizeErrorV1
        }
    };
    if supported {
        Ok(())
    } else {
        Err(invalid_plan("unsupported decimal execution policy"))
    }
}

fn condition_declaration(operation: &Operation) -> Result<u8, MachineProblem> {
    match operation.attributes.get(CONDITION_STATUS_ATTRIBUTE) {
        Some(Attribute::Text(status)) if status == SIZE_ERROR_STATUS => {}
        _ => return Err(invalid_plan("typed condition status is missing or invalid")),
    }
    match operation.attributes.get(CONDITION_BRANCHES_ATTRIBUTE) {
        Some(Attribute::Integer(mask)) if (0..=3).contains(mask) => Ok(*mask as u8),
        _ => Err(invalid_plan(
            "typed condition branch declaration is missing or invalid",
        )),
    }
}

fn branch_polarity(operation: &Operation) -> Result<bool, MachineProblem> {
    if optional_text_attribute(operation, "control_role") != Some("branch")
        || optional_integer_attribute(operation, "control_node").is_none()
        || optional_integer_attribute(operation, "control_parent").is_none()
        || optional_integer_attribute(operation, "edge_branch_false").is_none()
    {
        return Err(invalid_plan(
            "typed condition branch edge metadata is invalid",
        ));
    }
    match (
        operation.attributes.get(CONDITION_STATUS_ATTRIBUTE),
        operation.attributes.get(CONDITION_POLARITY_ATTRIBUTE),
    ) {
        (Some(Attribute::Text(status)), Some(Attribute::Boolean(polarity)))
            if status == SIZE_ERROR_STATUS =>
        {
            Ok(*polarity)
        }
        _ => Err(invalid_plan(
            "typed condition branch status or polarity is missing or invalid",
        )),
    }
}

fn validate_condition_edges(
    operation: &Operation,
    branches_by_owner: &BTreeMap<usize, Vec<&Operation>>,
) -> Result<(), MachineProblem> {
    let mask = condition_declaration(operation)?;
    let owner = optional_integer_attribute(operation, "control_node")
        .and_then(|node| usize::try_from(node).ok());
    if mask != 0 && owner.is_none() {
        return Err(invalid_plan("typed condition owner is missing"));
    }
    let branches = owner
        .and_then(|owner| branches_by_owner.get(&owner))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut actual = 0u8;
    for branch in branches {
        actual |= if branch_polarity(branch)? {
            SIZE_ERROR_BRANCH
        } else {
            NOT_SIZE_ERROR_BRANCH
        };
    }
    if actual != mask || actual.count_ones() as usize != branches.len() {
        return Err(invalid_plan(
            "typed condition branch topology is inconsistent",
        ));
    }
    Ok(())
}

fn has_size_error_handler(operation: &Operation) -> Result<bool, MachineProblem> {
    Ok(condition_declaration(operation)? & SIZE_ERROR_BRANCH != 0)
}

fn validate_marked_branch(
    machine: &ReferenceMachine,
    operation: &Operation,
    pc: usize,
) -> Result<(), MachineProblem> {
    validate_control_node_identity(machine, operation, pc)?;
    let parent = optional_integer_attribute(operation, "control_parent")
        .and_then(|node| usize::try_from(node).ok())
        .and_then(|node| machine.control_nodes.get(&node))
        .and_then(|pc| machine.operations.get(*pc))
        .ok_or_else(|| invalid_plan("typed condition branch owner is missing"))?;
    if !is_assign(parent) {
        return Err(invalid_plan(
            "typed condition branch owner is not decimal assign",
        ));
    }
    let polarity = branch_polarity(operation)?;
    let required = if polarity {
        SIZE_ERROR_BRANCH
    } else {
        NOT_SIZE_ERROR_BRANCH
    };
    if condition_declaration(parent)? & required == 0 {
        return Err(invalid_plan(
            "typed condition branch is not declared by its owner",
        ));
    }
    Ok(())
}

fn validate_control_node_identity(
    machine: &ReferenceMachine,
    operation: &Operation,
    pc: usize,
) -> Result<(), MachineProblem> {
    let Some(node) = optional_integer_attribute(operation, "control_node") else {
        return Ok(());
    };
    if usize::try_from(node)
        .ok()
        .and_then(|node| machine.control_nodes.get(&node))
        == Some(&pc)
    {
        Ok(())
    } else {
        Err(invalid_plan(
            "typed condition control-node identity is invalid",
        ))
    }
}

pub(super) fn control_branch(
    machine: &ReferenceMachine,
    operation: &Operation,
) -> Result<Option<bool>, MachineProblem> {
    let marked = operation
        .attributes
        .contains_key(CONDITION_STATUS_ATTRIBUTE)
        || operation
            .attributes
            .contains_key(CONDITION_POLARITY_ATTRIBUTE);
    let parent = optional_integer_attribute(operation, "control_parent")
        .and_then(|node| usize::try_from(node).ok())
        .and_then(|node| machine.control_nodes.get(&node))
        .and_then(|pc| machine.operations.get(*pc));
    let Some(parent) = parent else {
        return if marked {
            Err(invalid_plan("typed condition branch owner is missing"))
        } else {
            Ok(None)
        };
    };
    if !is_assign(parent) {
        return if marked {
            Err(invalid_plan(
                "typed condition branch owner is not decimal assign",
            ))
        } else {
            Ok(None)
        };
    }
    let polarity = branch_polarity(operation)?;
    let required = if polarity {
        SIZE_ERROR_BRANCH
    } else {
        NOT_SIZE_ERROR_BRANCH
    };
    if condition_declaration(parent)? & required == 0 {
        return Err(invalid_plan(
            "typed condition branch is not declared by its owner",
        ));
    }
    Ok(Some(if polarity {
        machine.condition_status.arithmetic_size_error
    } else {
        !machine.condition_status.arithmetic_size_error
    }))
}

fn validate_declared_slots(
    operation: &Operation,
    plan: &DecimalAssignmentPlan,
) -> Result<(), MachineProblem> {
    let mut expected = BTreeMap::<StorageId, String>::new();
    for assignment in &plan.assignments {
        insert_plan_slot(&mut expected, &assignment.receiver.target)?;
        visit_expression_slots(&assignment.expression, &mut |slot| {
            insert_plan_slot(&mut expected, slot)
        })?;
    }
    let mut declared = BTreeSet::new();
    for reference in &operation.storage {
        if reference.offset != 0 || !declared.insert(reference.storage) {
            return Err(invalid_plan(
                "operation storage declarations must be unique offset-zero slots",
            ));
        }
    }
    if declared == expected.keys().copied().collect() {
        Ok(())
    } else {
        Err(invalid_plan(
            "operation storage declarations do not exactly match plan slots",
        ))
    }
}

fn insert_plan_slot(
    slots: &mut BTreeMap<StorageId, String>,
    slot: &DecimalStorageSlot,
) -> Result<(), MachineProblem> {
    if slots
        .insert(slot.storage, slot.qualified_layout_name.clone())
        .is_some_and(|name| name != slot.qualified_layout_name)
    {
        Err(invalid_plan(
            "one storage slot names more than one qualified layout",
        ))
    } else {
        Ok(())
    }
}

fn validate_plan_slots(
    machine: &ReferenceMachine,
    operation: &Operation,
    plan: &DecimalAssignmentPlan,
) -> Result<(), MachineProblem> {
    for assignment in &plan.assignments {
        validate_machine_slot(machine, operation, &assignment.receiver.target, true)?;
        validate_expression_slots(machine, operation, &assignment.expression)?;
    }
    Ok(())
}

fn validate_expression_slots(
    machine: &ReferenceMachine,
    operation: &Operation,
    expression: &DecimalExpression,
) -> Result<(), MachineProblem> {
    match expression {
        DecimalExpression::Literal { .. } => Ok(()),
        DecimalExpression::Storage(slot) => validate_machine_slot(machine, operation, slot, true),
        DecimalExpression::Length(slot) => validate_machine_slot(machine, operation, slot, false),
        DecimalExpression::Negate(value) => validate_expression_slots(machine, operation, value),
        DecimalExpression::Add { left, right }
        | DecimalExpression::Subtract { left, right }
        | DecimalExpression::Multiply { left, right }
        | DecimalExpression::Divide { left, right } => {
            validate_expression_slots(machine, operation, left)?;
            validate_expression_slots(machine, operation, right)
        }
    }
}

fn visit_expression_slots(
    expression: &DecimalExpression,
    visit: &mut impl FnMut(&DecimalStorageSlot) -> Result<(), MachineProblem>,
) -> Result<(), MachineProblem> {
    match expression {
        DecimalExpression::Literal { .. } => Ok(()),
        DecimalExpression::Storage(slot) | DecimalExpression::Length(slot) => visit(slot),
        DecimalExpression::Negate(value) => visit_expression_slots(value, visit),
        DecimalExpression::Add { left, right }
        | DecimalExpression::Subtract { left, right }
        | DecimalExpression::Multiply { left, right }
        | DecimalExpression::Divide { left, right } => {
            visit_expression_slots(left, visit)?;
            visit_expression_slots(right, visit)
        }
    }
}

fn validate_machine_slot(
    machine: &ReferenceMachine,
    operation: &Operation,
    slot: &DecimalStorageSlot,
    numeric: bool,
) -> Result<(), MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout name is unknown"))?;
    if layout.name != slot.qualified_layout_name || numeric && !is_numeric(layout.category) {
        return Err(invalid_plan("plan layout identity or category is invalid"));
    }
    if machine.storage_names_by_id.get(&slot.storage) != Some(&slot.qualified_layout_name) {
        return Err(invalid_plan("plan storage ID does not name its layout"));
    }
    let id_view = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or_else(|| invalid_plan("plan storage slot is unknown"))?;
    let name_view = machine
        .views
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout has no storage view"))?;
    if id_view != name_view {
        return Err(invalid_plan("plan storage slot does not match layout name"));
    }
    let exact_length = u64::try_from(id_view.length)
        .map_err(|_| invalid_plan("plan storage view length is invalid"))?;
    if !operation.storage.iter().any(|reference| {
        reference.storage == slot.storage
            && reference.offset == 0
            && reference.length == exact_length
    }) {
        return Err(invalid_plan(
            "operation does not declare the complete plan storage view",
        ));
    }
    Ok(())
}

fn evaluate(
    machine: &ReferenceMachine,
    operation: &Operation,
    plan: &DecimalAssignmentPlan,
    expression: &DecimalExpression,
) -> Result<Decimal, MachineProblem> {
    match expression {
        DecimalExpression::Literal { coefficient, scale } => decimal_checked(Decimal {
            coefficient: *coefficient,
            scale: *scale,
        }),
        DecimalExpression::Storage(slot) => {
            let layout = runtime_layout(machine, operation, slot, true)?;
            decode_decimal(&layout, &machine.read(&layout.name)?)
        }
        DecimalExpression::Length(slot) => {
            let layout = runtime_layout(machine, operation, slot, false)?;
            let length = if layout.dynamic {
                machine
                    .dynamic_lengths
                    .get(&layout.name)
                    .copied()
                    .unwrap_or(0)
            } else {
                machine.storage_view(&layout.name)?.length
            };
            Ok(Decimal {
                coefficient: i128::try_from(length)
                    .map_err(|_| MachineProblem::ResourceExhausted)?,
                scale: 0,
            })
        }
        DecimalExpression::Negate(value) => {
            let value = evaluate(machine, operation, plan, value)?;
            decimal_checked(Decimal {
                coefficient: value
                    .coefficient
                    .checked_neg()
                    .ok_or(MachineProblem::SizeError)?,
                scale: value.scale,
            })
        }
        DecimalExpression::Add { left, right } => decimal_add(
            arithmetic_mode(machine, plan)?,
            evaluate(machine, operation, plan, left)?,
            evaluate(machine, operation, plan, right)?,
        ),
        DecimalExpression::Subtract { left, right } => decimal_subtract(
            arithmetic_mode(machine, plan)?,
            evaluate(machine, operation, plan, left)?,
            evaluate(machine, operation, plan, right)?,
        ),
        DecimalExpression::Multiply { left, right } => decimal_multiply(
            arithmetic_mode(machine, plan)?,
            evaluate(machine, operation, plan, left)?,
            evaluate(machine, operation, plan, right)?,
        ),
        DecimalExpression::Divide { left, right } => {
            let left = evaluate(machine, operation, plan, left)?;
            let right = evaluate(machine, operation, plan, right)?;
            let scale = left
                .scale
                .max(right.scale)
                .checked_add(9)
                .ok_or(MachineProblem::SizeError)?;
            decimal_divide(arithmetic_mode(machine, plan)?, left, right, scale)
        }
    }
}

fn arithmetic_mode(
    machine: &ReferenceMachine,
    plan: &DecimalAssignmentPlan,
) -> Result<CobolArithmeticMode, MachineProblem> {
    match plan.policy.arithmetic_context {
        DecimalArithmeticContext::LegacyCobolModuleV1 => Ok(machine.arithmetic_mode),
        DecimalArithmeticContext::Decimal18V1 => Ok(CobolArithmeticMode::Compatible),
        DecimalArithmeticContext::Decimal34V1 => Ok(CobolArithmeticMode::Extended),
    }
}

fn runtime_layout(
    machine: &ReferenceMachine,
    operation: &Operation,
    slot: &DecimalStorageSlot,
    numeric: bool,
) -> Result<LayoutMetadata, MachineProblem> {
    validate_machine_slot(machine, operation, slot, numeric)?;
    machine
        .layouts
        .get(&slot.qualified_layout_name)
        .cloned()
        .ok_or_else(|| invalid_plan("validated plan layout disappeared"))
}

fn stage_receiver(
    machine: &ReferenceMachine,
    operation: &Operation,
    receiver: &DecimalReceiver,
    value: Decimal,
    check_edited_size: bool,
) -> Result<(StorageView, Vec<u8>), MachineProblem> {
    let layout = runtime_layout(machine, operation, &receiver.target, true)?;
    let view = machine.storage_view(&layout.name)?.clone();
    let value = if matches!(
        layout.category,
        LayoutCategory::FloatShort | LayoutCategory::FloatLong
    ) {
        value
    } else {
        round_to_scale(value, layout.scale, receiver.rounding)?
    };
    if check_edited_size
        && layout.category == LayoutCategory::NumericEdited
        && decimal_exceeds_picture(&layout, value)
    {
        return Err(MachineProblem::SizeError);
    }
    let bytes = encode_decimal(&layout, value)?;
    if bytes.len() != view.length
        || machine
            .bases
            .get(view.base)
            .and_then(|storage| storage.get(view.offset..view.offset.saturating_add(view.length)))
            .is_none()
    {
        return Err(MachineProblem::DataException);
    }
    if matches!(
        layout.category,
        LayoutCategory::FloatShort | LayoutCategory::FloatLong
    ) && receiver.rounding == DecimalRoundingPolicy::Prohibited
    {
        let stored = decode_decimal(&layout, &bytes)?;
        let (stored, original) = decimal_aligned(stored, value)?;
        if stored.coefficient != original.coefficient {
            return Err(MachineProblem::SizeError);
        }
    }
    Ok((view, bytes))
}

fn stage_receiver_truncated(
    machine: &ReferenceMachine,
    operation: &Operation,
    receiver: &DecimalReceiver,
    value: Decimal,
) -> Result<(StorageView, Vec<u8>), MachineProblem> {
    let layout = runtime_layout(machine, operation, &receiver.target, true)?;
    if matches!(
        layout.category,
        LayoutCategory::FloatShort | LayoutCategory::FloatLong
    ) {
        return Err(MachineProblem::SizeError);
    }
    let view = machine.storage_view(&layout.name)?.clone();
    let value = round_to_scale(value, layout.scale, receiver.rounding)?;
    let digits = u32::try_from(layout.digits).map_err(|_| MachineProblem::SizeError)?;
    let modulus = ten_power(digits)?;
    let truncated = Decimal {
        coefficient: value.coefficient % modulus,
        scale: value.scale,
    };
    let bytes = encode_decimal(&layout, truncated)?;
    if bytes.len() != view.length
        || machine
            .bases
            .get(view.base)
            .and_then(|storage| storage.get(view.offset..view.offset.saturating_add(view.length)))
            .is_none()
    {
        return Err(MachineProblem::DataException);
    }
    Ok((view, bytes))
}

fn round_to_scale(
    value: Decimal,
    scale: u32,
    policy: DecimalRoundingPolicy,
) -> Result<Decimal, MachineProblem> {
    if value.scale <= scale {
        return decimal_rescale(value, scale);
    }
    let factor = ten_power(value.scale - scale)?;
    let quotient = value.coefficient / factor;
    let remainder = value.coefficient % factor;
    let sign = value.coefficient.signum();
    let twice = remainder
        .unsigned_abs()
        .checked_mul(2)
        .ok_or(MachineProblem::SizeError)?;
    let increment = match policy {
        DecimalRoundingPolicy::Truncation => 0,
        DecimalRoundingPolicy::AwayFromZero if remainder != 0 => sign,
        DecimalRoundingPolicy::NearestAwayFromZero if twice >= factor as u128 => sign,
        DecimalRoundingPolicy::NearestEven
            if twice > factor as u128
                || twice == factor as u128 && quotient.unsigned_abs() % 2 == 1 =>
        {
            sign
        }
        DecimalRoundingPolicy::Prohibited if remainder != 0 => {
            return Err(MachineProblem::SizeError);
        }
        DecimalRoundingPolicy::TowardGreater if remainder > 0 => 1,
        DecimalRoundingPolicy::TowardLesser if remainder < 0 => -1,
        _ => 0,
    };
    decimal_checked(Decimal {
        coefficient: quotient
            .checked_add(increment)
            .ok_or(MachineProblem::SizeError)?,
        scale,
    })
}

pub(super) fn decimal_add(
    mode: CobolArithmeticMode,
    left: Decimal,
    right: Decimal,
) -> Result<Decimal, MachineProblem> {
    decimal_primitive_binary(mode, left, right, CobolArithmetic::add)
}

pub(super) fn decimal_subtract(
    mode: CobolArithmeticMode,
    left: Decimal,
    right: Decimal,
) -> Result<Decimal, MachineProblem> {
    decimal_primitive_binary(mode, left, right, CobolArithmetic::subtract)
}

pub(super) fn decimal_multiply(
    mode: CobolArithmeticMode,
    left: Decimal,
    right: Decimal,
) -> Result<Decimal, MachineProblem> {
    decimal_primitive_binary(mode, left, right, CobolArithmetic::multiply)
}

pub(super) fn decimal_divide(
    mode: CobolArithmeticMode,
    dividend: Decimal,
    divisor: Decimal,
    result_scale: u32,
) -> Result<Decimal, MachineProblem> {
    if divisor.coefficient == 0 {
        return Err(MachineProblem::SizeError);
    }
    decimal_rescale(
        decimal_primitive_binary(mode, dividend, divisor, CobolArithmetic::divide)?,
        result_scale,
    )
}

type DecimalBinaryOperation =
    fn(
        &mut CobolArithmetic,
        CobolDecimal,
        CobolDecimal,
    ) -> Result<(CobolDecimal, crate::runtime::CobolArithmeticFlags), RuntimeContractProblem>;

fn decimal_primitive_binary(
    mode: CobolArithmeticMode,
    left: Decimal,
    right: Decimal,
    operation: DecimalBinaryOperation,
) -> Result<Decimal, MachineProblem> {
    let mut arithmetic =
        CobolArithmetic::new(mode, CobolRounding::Truncation).map_err(runtime_decimal_problem)?;
    let (left, left_flags) = arithmetic
        .parse(&decimal_string(left))
        .map_err(runtime_decimal_problem)?;
    let (right, right_flags) = arithmetic
        .parse(&decimal_string(right))
        .map_err(runtime_decimal_problem)?;
    if left_flags.size_error() || right_flags.size_error() {
        return Err(MachineProblem::SizeError);
    }
    let (value, flags) =
        operation(&mut arithmetic, left, right).map_err(runtime_decimal_problem)?;
    if flags.size_error() {
        return Err(MachineProblem::SizeError);
    }
    decimal_text(&value.to_standard_string()).ok_or(MachineProblem::SizeError)
}

const fn runtime_decimal_problem(problem: RuntimeContractProblem) -> MachineProblem {
    match problem {
        RuntimeContractProblem::InvalidDecimal => MachineProblem::DataException,
        _ => MachineProblem::SizeError,
    }
}

fn invalid_plan(detail: &str) -> MachineProblem {
    MachineProblem::InvalidArtifact(format!("invalid typed decimal assignment plan: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_ir::{
        DecimalAssignment, Effect, IrLimits, ModuleBuilder, StorageReference,
        encode_decimal_assignment_plan,
    };

    #[derive(Clone, Copy)]
    struct TestLayout {
        name: &'static str,
        category: &'static str,
        digits: usize,
        scale: u32,
        length: usize,
        initial: &'static [u8],
    }

    const LAYOUTS: &[TestLayout] = &[
        TestLayout {
            name: "A",
            category: "numeric_display",
            digits: 3,
            scale: 0,
            length: 3,
            initial: b"002",
        },
        TestLayout {
            name: "B",
            category: "numeric_display",
            digits: 3,
            scale: 0,
            length: 3,
            initial: b"003",
        },
        TestLayout {
            name: "TEXT-X",
            category: "alphanumeric",
            digits: 0,
            scale: 0,
            length: 5,
            initial: b"HELLO",
        },
        TestLayout {
            name: "RESULT-X",
            category: "numeric_display",
            digits: 3,
            scale: 0,
            length: 3,
            initial: b"000",
        },
        TestLayout {
            name: "GOOD-X",
            category: "numeric_display",
            digits: 1,
            scale: 0,
            length: 1,
            initial: b"1",
        },
        TestLayout {
            name: "SMALL-X",
            category: "numeric_display",
            digits: 1,
            scale: 0,
            length: 1,
            initial: b"0",
        },
        TestLayout {
            name: "FIRST-X",
            category: "numeric_display",
            digits: 1,
            scale: 0,
            length: 1,
            initial: b"1",
        },
        TestLayout {
            name: "MIDDLE-X",
            category: "numeric_display",
            digits: 1,
            scale: 0,
            length: 1,
            initial: b"2",
        },
        TestLayout {
            name: "LAST-X",
            category: "numeric_display",
            digits: 1,
            scale: 0,
            length: 1,
            initial: b"3",
        },
        TestLayout {
            name: "WIDE-X",
            category: "numeric_display",
            digits: 19,
            scale: 0,
            length: 19,
            initial: b"0000000000000000000",
        },
    ];

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new("typed-decimal-request", limits).unwrap(),
            ExecutionId::new("typed-decimal-execution", limits).unwrap(),
            RunUnitId::new("typed-decimal-run", limits).unwrap(),
            None,
            Selector::new("program:TYPEDDEC", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::<CapabilityId>::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("typed-decimal-trace", limits).unwrap(),
            IdempotencyKey::new("typed-decimal-idempotency", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn slot(slots: &BTreeMap<String, StorageId>, name: &str) -> DecimalStorageSlot {
        DecimalStorageSlot {
            storage: slots[name],
            qualified_layout_name: name.into(),
        }
    }

    #[derive(Clone, Copy)]
    struct TestBranch<'a> {
        polarity: Option<bool>,
        status: Option<&'a str>,
        control_text: Option<&'a str>,
    }

    fn binary(
        identity: OperationIdentity,
        make_plan: impl FnOnce(&BTreeMap<String, StorageId>) -> DecimalAssignmentPlan,
        transform_plan: impl FnOnce(Vec<u8>) -> Vec<u8>,
        legacy_arguments: bool,
    ) -> Vec<u8> {
        binary_with_conditions(
            identity,
            make_plan,
            transform_plan,
            legacy_arguments,
            0,
            &[],
        )
    }

    fn binary_with_conditions(
        identity: OperationIdentity,
        make_plan: impl FnOnce(&BTreeMap<String, StorageId>) -> DecimalAssignmentPlan,
        transform_plan: impl FnOnce(Vec<u8>) -> Vec<u8>,
        legacy_arguments: bool,
        condition_mask: i64,
        branches: &[TestBranch<'_>],
    ) -> Vec<u8> {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let slots = LAYOUTS
            .iter()
            .map(|layout| {
                let storage = builder
                    .add_storage(layout.name.to_ascii_lowercase(), layout.length as u64, None)
                    .unwrap();
                (layout.name.to_string(), storage)
            })
            .collect::<BTreeMap<_, _>>();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        for layout in LAYOUTS {
            builder
                .add_operation(
                    block,
                    OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                    Vec::new(),
                    0,
                    BTreeMap::from([
                        ("name".into(), Attribute::Text(layout.name.into())),
                        ("simple_name".into(), Attribute::Text(layout.name.into())),
                        ("category".into(), Attribute::Text(layout.category.into())),
                        (
                            "picture".into(),
                            Attribute::Text(if layout.category == "numeric_display" {
                                format!("9({})", layout.digits)
                            } else {
                                format!("X({})", layout.length)
                            }),
                        ),
                        ("digits".into(), Attribute::Integer(layout.digits as i64)),
                        ("scale".into(), Attribute::Integer(i64::from(layout.scale))),
                        ("signed".into(), Attribute::Integer(0)),
                        ("sign_separate".into(), Attribute::Integer(0)),
                        ("section".into(), Attribute::Text("working".into())),
                        ("offset".into(), Attribute::Integer(0)),
                        ("length".into(), Attribute::Integer(layout.length as i64)),
                        (
                            "element_length".into(),
                            Attribute::Integer(layout.length as i64),
                        ),
                        ("occurs".into(), Attribute::Integer(1)),
                        ("parent".into(), Attribute::Text(String::new())),
                        ("condition_values".into(), Attribute::Text(String::new())),
                    ]),
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        for layout in LAYOUTS {
            builder
                .add_operation(
                    block,
                    OperationIdentity::new(super::super::NAMESPACE, "init", 1).unwrap(),
                    Vec::new(),
                    0,
                    BTreeMap::from([("initial".into(), Attribute::Bytes(layout.initial.to_vec()))]),
                    vec![Effect::MemoryWrite],
                    vec![StorageReference {
                        storage: slots[layout.name],
                        offset: 0,
                        length: layout.length as u64,
                    }],
                    None,
                )
                .unwrap();
        }
        let typed_plan = make_plan(&slots);
        let mut referenced = BTreeMap::new();
        for assignment in &typed_plan.assignments {
            insert_plan_slot(&mut referenced, &assignment.receiver.target).unwrap();
            visit_expression_slots(&assignment.expression, &mut |slot| {
                insert_plan_slot(&mut referenced, slot)
            })
            .unwrap();
        }
        let operation_storage = referenced
            .keys()
            .map(|storage| {
                let layout = LAYOUTS
                    .iter()
                    .find(|layout| slots[layout.name] == *storage)
                    .unwrap();
                StorageReference {
                    storage: *storage,
                    offset: 0,
                    length: layout.length as u64,
                }
            })
            .collect();
        let plan = transform_plan(
            encode_decimal_assignment_plan(&typed_plan, DecimalPlanLimits::default()).unwrap(),
        );
        let mut attributes = BTreeMap::from([
            (PLAN_ATTRIBUTE.into(), Attribute::Bytes(plan)),
            (
                CONDITION_STATUS_ATTRIBUTE.into(),
                Attribute::Text(SIZE_ERROR_STATUS.into()),
            ),
            (
                CONDITION_BRANCHES_ATTRIBUTE.into(),
                Attribute::Integer(condition_mask),
            ),
        ]);
        if condition_mask != 0 || !branches.is_empty() {
            attributes.insert("control_node".into(), Attribute::Integer(0));
            attributes.insert("control_role".into(), Attribute::Text("statement".into()));
        }
        if legacy_arguments {
            attributes.insert("arguments".into(), Attribute::Bytes(Vec::new()));
        }
        builder
            .add_operation(
                block,
                identity,
                Vec::new(),
                0,
                attributes,
                vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition],
                operation_storage,
                None,
            )
            .unwrap();
        for (index, branch) in branches.iter().enumerate() {
            let mut attributes = BTreeMap::from([
                (
                    "control_node".into(),
                    Attribute::Integer((index + 1) as i64),
                ),
                ("control_role".into(), Attribute::Text("branch".into())),
                ("control_parent".into(), Attribute::Integer(0)),
                (
                    "edge_branch_false".into(),
                    Attribute::Integer((index + 2) as i64),
                ),
            ]);
            if let Some(status) = branch.status {
                attributes.insert(
                    CONDITION_STATUS_ATTRIBUTE.into(),
                    Attribute::Text(status.into()),
                );
            }
            if let Some(polarity) = branch.polarity {
                attributes.insert(
                    CONDITION_POLARITY_ATTRIBUTE.into(),
                    Attribute::Boolean(polarity),
                );
            }
            if let Some(text) = branch.control_text {
                attributes.insert("control_text".into(), Attribute::Text(text.into()));
            }
            builder
                .add_operation(
                    block,
                    OperationIdentity::new(super::super::NAMESPACE, "control", 1).unwrap(),
                    Vec::new(),
                    0,
                    attributes,
                    vec![Effect::ProgramControl, Effect::Condition],
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        let halt_attributes = if branches.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([
                (
                    "control_node".into(),
                    Attribute::Integer((branches.len() + 1) as i64),
                ),
                ("control_role".into(), Attribute::Text("terminator".into())),
            ])
        };
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                halt_attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
    }

    fn machine(
        make_plan: impl FnOnce(&BTreeMap<String, StorageId>) -> DecimalAssignmentPlan,
    ) -> ReferenceMachine {
        let bytes = binary(operation_identity(), make_plan, |plan| plan, false);
        ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()).unwrap()
    }

    fn machine_with_size_error_handler(
        make_plan: impl FnOnce(&BTreeMap<String, StorageId>) -> DecimalAssignmentPlan,
    ) -> ReferenceMachine {
        let bytes = binary_with_conditions(
            operation_identity(),
            make_plan,
            |plan| plan,
            false,
            i64::from(SIZE_ERROR_BRANCH),
            &[TestBranch {
                polarity: Some(true),
                status: Some(SIZE_ERROR_STATUS),
                control_text: Some("ON SIZE ERROR"),
            }],
        );
        ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()).unwrap()
    }

    fn assignment(expression: DecimalExpression, target: DecimalStorageSlot) -> DecimalAssignment {
        DecimalAssignment {
            expression,
            receiver: DecimalReceiver {
                target,
                rounding: DecimalRoundingPolicy::Truncation,
            },
        }
    }

    fn plan(assignments: Vec<DecimalAssignment>) -> DecimalAssignmentPlan {
        DecimalAssignmentPlan {
            semantic_origin: "cobol.compute@1".into(),
            policy: mainframe_env_ir::DecimalExecutionPolicy::decimal34_v1(),
            assignments,
        }
    }

    fn as_legacy_plan_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
        bytes.drain(6..11);
        bytes
    }

    fn division_by_zero_plan(slots: &BTreeMap<String, StorageId>) -> DecimalAssignmentPlan {
        plan(vec![assignment(
            DecimalExpression::Divide {
                left: Box::new(DecimalExpression::Literal {
                    coefficient: 1,
                    scale: 0,
                }),
                right: Box::new(DecimalExpression::Literal {
                    coefficient: 0,
                    scale: 0,
                }),
            },
            slot(slots, "GOOD-X"),
        )])
    }

    fn drive(machine: &mut ReferenceMachine) -> MachineDrive<EffectRequest> {
        machine.drive(MachineResume::Start, Quantum::new(100, 4096).unwrap())
    }

    #[test]
    fn typed_expression_uses_every_primitive_and_length_then_writes_exact_bytes() {
        let mut machine = machine(|slots| {
            let left = DecimalExpression::Add {
                left: Box::new(DecimalExpression::Storage(slot(slots, "A"))),
                right: Box::new(DecimalExpression::Length(slot(slots, "TEXT-X"))),
            };
            let right = DecimalExpression::Subtract {
                left: Box::new(DecimalExpression::Storage(slot(slots, "B"))),
                right: Box::new(DecimalExpression::Negate(Box::new(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                ))),
            };
            plan(vec![assignment(
                DecimalExpression::Divide {
                    left: Box::new(DecimalExpression::Multiply {
                        left: Box::new(left),
                        right: Box::new(right),
                    }),
                    right: Box::new(DecimalExpression::Literal {
                        coefficient: 2,
                        scale: 0,
                    }),
                },
                slot(slots, "RESULT-X"),
            )])
        });
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert_eq!(machine.variable("RESULT-X").unwrap().bytes(), b"014");
    }

    #[test]
    fn current_empty_assignment_plan_is_an_explicit_noop() {
        let mut machine = machine(|_| plan(Vec::new()));
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert_eq!(machine.variable("RESULT-X").unwrap().bytes(), b"000");
    }

    #[test]
    fn all_rounding_policies_have_exact_directed_results() {
        let value = Decimal {
            coefficient: 121,
            scale: 1,
        };
        let negative = Decimal {
            coefficient: -121,
            scale: 1,
        };
        for (policy, expected) in [
            (DecimalRoundingPolicy::Truncation, 12),
            (DecimalRoundingPolicy::AwayFromZero, 13),
            (DecimalRoundingPolicy::NearestAwayFromZero, 12),
            (DecimalRoundingPolicy::NearestEven, 12),
            (DecimalRoundingPolicy::TowardGreater, 13),
            (DecimalRoundingPolicy::TowardLesser, 12),
        ] {
            assert_eq!(
                round_to_scale(value, 0, policy).unwrap().coefficient,
                expected
            );
        }
        assert_eq!(
            round_to_scale(negative, 0, DecimalRoundingPolicy::TowardGreater)
                .unwrap()
                .coefficient,
            -12
        );
        assert_eq!(
            round_to_scale(negative, 0, DecimalRoundingPolicy::TowardLesser)
                .unwrap()
                .coefficient,
            -13
        );
        assert_eq!(
            round_to_scale(value, 0, DecimalRoundingPolicy::Prohibited),
            Err(MachineProblem::SizeError)
        );
        assert_eq!(
            round_to_scale(
                Decimal {
                    coefficient: 120,
                    scale: 1,
                },
                0,
                DecimalRoundingPolicy::Prohibited,
            )
            .unwrap()
            .coefficient,
            12
        );
        assert_eq!(
            round_to_scale(
                Decimal {
                    coefficient: 25,
                    scale: 1,
                },
                0,
                DecimalRoundingPolicy::NearestEven,
            )
            .unwrap()
            .coefficient,
            2
        );
    }

    #[test]
    fn division_by_zero_is_a_size_error_and_does_not_mutate() {
        let mut machine = machine(division_by_zero_plan);
        assert!(matches!(drive(&mut machine), MachineDrive::Condition(_)));
        assert_eq!(machine.variable("GOOD-X").unwrap().bytes(), b"1");
    }

    #[test]
    fn typed_size_error_branching_ignores_changed_or_missing_control_text() {
        for control_text in [Some("NOT ON SIZE ERROR"), None] {
            let bytes = binary_with_conditions(
                operation_identity(),
                division_by_zero_plan,
                |plan| plan,
                false,
                i64::from(SIZE_ERROR_BRANCH),
                &[TestBranch {
                    polarity: Some(true),
                    status: Some(SIZE_ERROR_STATUS),
                    control_text,
                }],
            );
            let mut machine =
                ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default())
                    .unwrap();
            let assignment = machine
                .operations
                .iter()
                .find(|operation| is_assign(operation))
                .cloned()
                .unwrap();
            execute_with_condition(&mut machine, &assignment).unwrap();
            let branch = machine
                .operations
                .iter()
                .find(|operation| {
                    optional_text_attribute(operation, "control_role") == Some("branch")
                })
                .unwrap();
            assert_eq!(control_branch(&machine, branch).unwrap(), Some(true));
        }
    }

    #[test]
    fn missing_or_drifted_typed_size_error_metadata_is_rejected() {
        for (mask, branches) in [
            (
                i64::from(SIZE_ERROR_BRANCH),
                vec![TestBranch {
                    polarity: None,
                    status: Some(SIZE_ERROR_STATUS),
                    control_text: Some("ON SIZE ERROR"),
                }],
            ),
            (
                i64::from(SIZE_ERROR_BRANCH),
                vec![TestBranch {
                    polarity: Some(true),
                    status: Some("forged.status@1"),
                    control_text: Some("ON SIZE ERROR"),
                }],
            ),
            (i64::from(SIZE_ERROR_BRANCH), Vec::new()),
            (4, Vec::new()),
        ] {
            let bytes = binary_with_conditions(
                operation_identity(),
                division_by_zero_plan,
                |plan| plan,
                false,
                mask,
                &branches,
            );
            assert!(matches!(
                ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()),
                Err(MachineProblem::InvalidArtifact(_))
            ));
        }
    }

    #[test]
    fn receiver_local_size_error_preserves_only_the_failing_receiver() {
        let mut machine = machine_with_size_error_handler(|slots| {
            let first = assignment(
                DecimalExpression::Literal {
                    coefficient: 9,
                    scale: 0,
                },
                slot(slots, "GOOD-X"),
            );
            let mut second = assignment(
                DecimalExpression::Literal {
                    coefficient: 15,
                    scale: 1,
                },
                slot(slots, "SMALL-X"),
            );
            second.receiver.rounding = DecimalRoundingPolicy::Prohibited;
            plan(vec![first, second])
        });
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert!(machine.condition_status.arithmetic_size_error);
        assert_eq!(machine.variable("GOOD-X").unwrap().bytes(), b"9");
        assert_eq!(machine.variable("SMALL-X").unwrap().bytes(), b"0");
    }

    #[test]
    fn first_middle_and_last_receiver_failures_do_not_suppress_other_results() {
        for (failed, expected) in [(0, *b"156"), (1, *b"426"), (2, *b"453")] {
            let mut machine = machine_with_size_error_handler(|slots| {
                plan(
                    ["FIRST-X", "MIDDLE-X", "LAST-X"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, name)| {
                            assignment(
                                DecimalExpression::Literal {
                                    coefficient: if index == failed {
                                        10
                                    } else {
                                        i128::try_from(index).unwrap() + 4
                                    },
                                    scale: 0,
                                },
                                slot(slots, name),
                            )
                        })
                        .collect(),
                )
            });
            assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
            assert!(machine.condition_status.arithmetic_size_error);
            for (name, value) in ["FIRST-X", "MIDDLE-X", "LAST-X"].into_iter().zip(expected) {
                assert_eq!(machine.variable(name).unwrap().bytes(), &[value]);
            }
        }
    }

    #[test]
    fn all_success_and_all_failure_receiver_batches_have_exact_results() {
        let make_plan = |failed: bool| {
            move |slots: &BTreeMap<String, StorageId>| {
                plan(
                    ["FIRST-X", "MIDDLE-X", "LAST-X"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, name)| {
                            assignment(
                                DecimalExpression::Literal {
                                    coefficient: if failed {
                                        10
                                    } else {
                                        i128::try_from(index).unwrap() + 4
                                    },
                                    scale: 0,
                                },
                                slot(slots, name),
                            )
                        })
                        .collect(),
                )
            }
        };

        let mut success = machine(make_plan(false));
        assert!(matches!(drive(&mut success), MachineDrive::Completed(_)));
        assert_eq!(success.variable("FIRST-X").unwrap().bytes(), b"4");
        assert_eq!(success.variable("MIDDLE-X").unwrap().bytes(), b"5");
        assert_eq!(success.variable("LAST-X").unwrap().bytes(), b"6");

        let mut failure = machine_with_size_error_handler(make_plan(true));
        assert!(matches!(drive(&mut failure), MachineDrive::Completed(_)));
        assert!(failure.condition_status.arithmetic_size_error);
        assert_eq!(failure.variable("FIRST-X").unwrap().bytes(), b"1");
        assert_eq!(failure.variable("MIDDLE-X").unwrap().bytes(), b"2");
        assert_eq!(failure.variable("LAST-X").unwrap().bytes(), b"3");
    }

    #[test]
    fn receiver_overflow_without_size_error_handler_truncates_and_continues() {
        let mut machine = machine(|slots| {
            plan(vec![
                assignment(
                    DecimalExpression::Literal {
                        coefficient: 6,
                        scale: 0,
                    },
                    slot(slots, "GOOD-X"),
                ),
                assignment(
                    DecimalExpression::Literal {
                        coefficient: 42,
                        scale: 0,
                    },
                    slot(slots, "SMALL-X"),
                ),
            ])
        });
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert_eq!(machine.variable("GOOD-X").unwrap().bytes(), b"6");
        assert_eq!(machine.variable("SMALL-X").unwrap().bytes(), b"2");
    }

    #[test]
    fn operands_are_captured_before_any_receiver_is_written() {
        let mut machine = machine(|slots| {
            plan(vec![
                assignment(
                    DecimalExpression::Storage(slot(slots, "B")),
                    slot(slots, "A"),
                ),
                assignment(
                    DecimalExpression::Storage(slot(slots, "A")),
                    slot(slots, "B"),
                ),
            ])
        });
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert_eq!(machine.variable("A").unwrap().bytes(), b"003");
        assert_eq!(machine.variable("B").unwrap().bytes(), b"002");
    }

    #[test]
    fn shared_expression_failure_does_not_commit_any_receiver() {
        let mut machine = machine(|slots| {
            let failed_expression = || DecimalExpression::Divide {
                left: Box::new(DecimalExpression::Literal {
                    coefficient: 1,
                    scale: 0,
                }),
                right: Box::new(DecimalExpression::Literal {
                    coefficient: 0,
                    scale: 0,
                }),
            };
            plan(vec![
                assignment(
                    DecimalExpression::Literal {
                        coefficient: 4,
                        scale: 0,
                    },
                    slot(slots, "FIRST-X"),
                ),
                assignment(failed_expression(), slot(slots, "MIDDLE-X")),
                assignment(
                    DecimalExpression::Literal {
                        coefficient: 6,
                        scale: 0,
                    },
                    slot(slots, "LAST-X"),
                ),
            ])
        });
        assert!(matches!(drive(&mut machine), MachineDrive::Condition(_)));
        assert_eq!(machine.variable("FIRST-X").unwrap().bytes(), b"1");
        assert_eq!(machine.variable("MIDDLE-X").unwrap().bytes(), b"2");
        assert_eq!(machine.variable("LAST-X").unwrap().bytes(), b"3");
    }

    #[test]
    fn malformed_plan_and_wrong_dialect_identity_fail_during_construction() {
        let valid = |slots: &BTreeMap<String, StorageId>| {
            plan(vec![assignment(
                DecimalExpression::Literal {
                    coefficient: 1,
                    scale: 0,
                },
                slot(slots, "RESULT-X"),
            )])
        };
        let malformed = binary(
            operation_identity(),
            valid,
            |mut bytes| {
                bytes.push(0);
                bytes
            },
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&malformed, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));

        let wrong = binary(
            OperationIdentity::new(NAMESPACE, NAME, 3).unwrap(),
            valid,
            |bytes| bytes,
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&wrong, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn storage_slot_and_qualified_name_must_identify_the_same_declared_view() {
        let bytes = binary(
            operation_identity(),
            |slots| {
                plan(vec![assignment(
                    DecimalExpression::Storage(slot(slots, "A")),
                    DecimalStorageSlot {
                        storage: slots["RESULT-X"],
                        qualified_layout_name: "B".into(),
                    },
                )])
            },
            |bytes| bytes,
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn equal_alias_views_cannot_substitute_a_different_storage_identity() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let backing = builder.add_storage("backing", 3, None).unwrap();
        let alias = |storage| StorageReference {
            storage,
            offset: 0,
            length: 3,
        };
        let wrong = builder.add_storage("a", 3, Some(alias(backing))).unwrap();
        let named = builder.add_storage("b", 3, Some(alias(backing))).unwrap();
        assert_ne!(wrong, named);
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("B".into())),
                    ("simple_name".into(), Attribute::Text("B".into())),
                    ("category".into(), Attribute::Text("numeric_display".into())),
                    ("picture".into(), Attribute::Text("9(3)".into())),
                    ("digits".into(), Attribute::Integer(3)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(3)),
                    ("element_length".into(), Attribute::Integer(3)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let plan = plan(vec![assignment(
            DecimalExpression::Literal {
                coefficient: 1,
                scale: 0,
            },
            DecimalStorageSlot {
                storage: wrong,
                qualified_layout_name: "B".into(),
            },
        )]);
        builder
            .add_operation(
                block,
                operation_identity(),
                Vec::new(),
                0,
                BTreeMap::from([
                    (
                        PLAN_ATTRIBUTE.into(),
                        Attribute::Bytes(
                            encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default())
                                .unwrap(),
                        ),
                    ),
                    (
                        CONDITION_STATUS_ATTRIBUTE.into(),
                        Attribute::Text(SIZE_ERROR_STATUS.into()),
                    ),
                    (CONDITION_BRANCHES_ATTRIBUTE.into(), Attribute::Integer(0)),
                ]),
                vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition],
                vec![alias(wrong)],
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        assert!(matches!(
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn legacy_arguments_are_rejected_even_when_the_typed_plan_is_valid() {
        let bytes = binary(
            operation_identity(),
            |slots| {
                plan(vec![assignment(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                    slot(slots, "RESULT-X"),
                )])
            },
            |bytes| bytes,
            true,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn current_origin_is_provenance_and_does_not_select_execution() {
        let bytes = binary(
            operation_identity(),
            |slots| {
                let mut value = plan(vec![assignment(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                    slot(slots, "RESULT-X"),
                )]);
                value.semantic_origin = "ledger.formula@1".into();
                value
            },
            |bytes| bytes,
            false,
        );
        let mut machine =
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()).unwrap();
        assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
        assert_eq!(machine.variable("RESULT-X").unwrap().bytes(), b"001");
    }

    #[test]
    fn operation_plan_version_and_unknown_policy_fail_closed() {
        let current_plan_under_legacy_identity = binary(
            legacy_operation_identity(),
            |slots| {
                plan(vec![assignment(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                    slot(slots, "RESULT-X"),
                )])
            },
            |bytes| bytes,
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(
                &current_plan_under_legacy_identity,
                invocation(),
                CodecLimits::default()
            ),
            Err(MachineProblem::InvalidArtifact(_))
        ));

        let foreign_legacy_origin = binary(
            legacy_operation_identity(),
            |slots| {
                let mut value = plan(vec![assignment(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                    slot(slots, "RESULT-X"),
                )]);
                value.semantic_origin = "ledger.formula@1".into();
                value
            },
            as_legacy_plan_bytes,
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(
                &foreign_legacy_origin,
                invocation(),
                CodecLimits::default()
            ),
            Err(MachineProblem::InvalidArtifact(_))
        ));

        let unknown_policy = binary(
            operation_identity(),
            |slots| {
                plan(vec![assignment(
                    DecimalExpression::Literal {
                        coefficient: 1,
                        scale: 0,
                    },
                    slot(slots, "RESULT-X"),
                )])
            },
            |mut bytes| {
                bytes[7] = u8::MAX;
                bytes
            },
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&unknown_policy, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn explicit_arithmetic_context_changes_the_exact_result() {
        let run = |policy: mainframe_env_ir::DecimalExecutionPolicy| {
            let bytes = binary(
                operation_identity(),
                |slots| {
                    let mut value = plan(vec![assignment(
                        DecimalExpression::Add {
                            left: Box::new(DecimalExpression::Literal {
                                coefficient: 999_999_999_999_999_999,
                                scale: 0,
                            }),
                            right: Box::new(DecimalExpression::Literal {
                                coefficient: 2,
                                scale: 0,
                            }),
                        },
                        slot(slots, "WIDE-X"),
                    )]);
                    value.policy = policy;
                    value.semantic_origin = "ledger.formula@1".into();
                    value
                },
                |bytes| bytes,
                false,
            );
            let mut machine =
                ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default())
                    .unwrap();
            assert!(matches!(drive(&mut machine), MachineDrive::Completed(_)));
            machine.variable("WIDE-X").unwrap().bytes().to_vec()
        };
        assert_eq!(
            run(mainframe_env_ir::DecimalExecutionPolicy::decimal18_v1()),
            b"1000000000000000000"
        );
        assert_eq!(
            run(mainframe_env_ir::DecimalExecutionPolicy::decimal34_v1()),
            b"1000000000000000001"
        );
    }

    #[test]
    fn historical_assign_v1_preserves_whole_batch_receiver_atomicity() {
        let bytes = binary(
            legacy_operation_identity(),
            |slots| {
                plan(vec![
                    assignment(
                        DecimalExpression::Literal {
                            coefficient: 9,
                            scale: 0,
                        },
                        slot(slots, "GOOD-X"),
                    ),
                    assignment(
                        DecimalExpression::Literal {
                            coefficient: 10,
                            scale: 0,
                        },
                        slot(slots, "SMALL-X"),
                    ),
                ])
            },
            as_legacy_plan_bytes,
            false,
        );
        let mut machine =
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()).unwrap();
        assert!(matches!(drive(&mut machine), MachineDrive::Condition(_)));
        assert_eq!(machine.variable("GOOD-X").unwrap().bytes(), b"1");
        assert_eq!(machine.variable("SMALL-X").unwrap().bytes(), b"0");
    }
}
