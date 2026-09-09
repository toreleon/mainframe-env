use super::*;
use crate::runtime::{CobolArithmetic, CobolDecimal, CobolRounding, RuntimeContractProblem};
use mainframe_env_ir::{
    DecimalAssignmentPlan, DecimalExpression, DecimalPlanLimits, DecimalReceiver,
    DecimalRoundingPolicy, DecimalStorageSlot, Effect, decode_decimal_assignment_plan,
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
    OperationIdentity::new(NAMESPACE, NAME, 1).expect("static typed decimal operation")
}

pub(super) fn is_assign(operation: &Operation) -> bool {
    operation.identity.namespace() == NAMESPACE
        && operation.identity.name() == NAME
        && operation.identity.major() == 1
}

pub(super) fn validate_module_operations(operations: &[&Operation]) -> Result<(), MachineProblem> {
    for operation in operations
        .iter()
        .copied()
        .filter(|operation| is_assign(operation))
    {
        let plan = plan(operation)?;
        validate_declared_slots(operation, &plan)?;
    }
    Ok(())
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
        Ok(()) => machine.condition_status.arithmetic_size_error = false,
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

fn execute(machine: &mut ReferenceMachine, operation: &Operation) -> Result<(), MachineProblem> {
    let plan = plan(operation)?;
    validate_declared_slots(operation, &plan)?;
    validate_plan_slots(machine, operation, &plan)?;
    let mut staged = Vec::with_capacity(plan.assignments.len());
    for assignment in &plan.assignments {
        let value = evaluate(machine, operation, &assignment.expression)?;
        staged.push(stage_receiver(
            machine,
            operation,
            &assignment.receiver,
            value,
        )?);
    }
    for (view, bytes) in staged {
        machine.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
    }
    Ok(())
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
    let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default())
        .map_err(|problem| invalid_plan(&problem.to_string()))?;
    if !matches!(
        plan.semantic_origin.as_str(),
        "cobol.add@1" | "cobol.compute@1"
    ) {
        return Err(invalid_plan(
            "semantic origin is not an approved COBOL operation",
        ));
    }
    Ok(plan)
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
            let value = evaluate(machine, operation, value)?;
            decimal_checked(Decimal {
                coefficient: value
                    .coefficient
                    .checked_neg()
                    .ok_or(MachineProblem::SizeError)?,
                scale: value.scale,
            })
        }
        DecimalExpression::Add { left, right } => decimal_add(
            machine.arithmetic_mode,
            evaluate(machine, operation, left)?,
            evaluate(machine, operation, right)?,
        ),
        DecimalExpression::Subtract { left, right } => decimal_subtract(
            machine.arithmetic_mode,
            evaluate(machine, operation, left)?,
            evaluate(machine, operation, right)?,
        ),
        DecimalExpression::Multiply { left, right } => decimal_multiply(
            machine.arithmetic_mode,
            evaluate(machine, operation, left)?,
            evaluate(machine, operation, right)?,
        ),
        DecimalExpression::Divide { left, right } => {
            let left = evaluate(machine, operation, left)?;
            let right = evaluate(machine, operation, right)?;
            let scale = left
                .scale
                .max(right.scale)
                .checked_add(9)
                .ok_or(MachineProblem::SizeError)?;
            decimal_divide(machine.arithmetic_mode, left, right, scale)
        }
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
                        ("picture".into(), Attribute::Text(String::new())),
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
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
    }

    fn machine(
        make_plan: impl FnOnce(&BTreeMap<String, StorageId>) -> DecimalAssignmentPlan,
    ) -> ReferenceMachine {
        let bytes = binary(operation_identity(), make_plan, |plan| plan, false);
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
            assignments,
        }
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
    fn a_late_receiver_error_commits_none_of_the_staged_batch() {
        let mut machine = machine(|slots| {
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
        assert!(matches!(drive(&mut machine), MachineDrive::Condition(_)));
        assert_eq!(machine.variable("GOOD-X").unwrap().bytes(), b"1");
        assert_eq!(machine.variable("SMALL-X").unwrap().bytes(), b"0");
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
            OperationIdentity::new(NAMESPACE, NAME, 2).unwrap(),
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
                    ("picture".into(), Attribute::Text(String::new())),
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
    fn unapproved_semantic_origin_is_rejected_during_construction() {
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
                value.semantic_origin = "cobol.multiply@1".into();
                value
            },
            |bytes| bytes,
            false,
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&bytes, invocation(), CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }
}
