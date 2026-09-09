//! Bounded dialect-owned verification for typed executable plans.

use crate::{
    Attribute, CicsCondition, CicsEffectPlan, CicsOperandValue, CicsOperationContract,
    CicsOutputName, CicsPlanLimits, DecimalAssignmentPlan, DecimalConditionContract,
    DecimalExpression, DecimalOperationContract, DecimalPlanLimits, Effect, Module, Operation,
    OperationCatalog, OperationIdentity, OperationSemanticContract, StorageId,
    cics_executable_descriptor, decimal_assignment_plan_wire_version, decode_cics_effect_plan,
    decode_decimal_assignment_plan,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Failure of a dialect-owned static operation contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticVerificationProblem {
    pub identity: OperationIdentity,
    pub detail: &'static str,
}

impl fmt::Display for SemanticVerificationProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "typed semantic verification failed for {}: {}",
            self.identity, self.detail
        )
    }
}

impl std::error::Error for SemanticVerificationProblem {}

#[derive(Clone, Copy, Default)]
struct SlotUse {
    numeric: bool,
    writable: bool,
}

impl SlotUse {
    const READ: Self = Self {
        numeric: false,
        writable: false,
    };
    const WRITE: Self = Self {
        numeric: false,
        writable: true,
    };
    const NUMERIC_READ: Self = Self {
        numeric: true,
        writable: false,
    };
    const NUMERIC_WRITE: Self = Self {
        numeric: true,
        writable: true,
    };

    fn merge(&mut self, other: Self) {
        self.numeric |= other.numeric;
        self.writable |= other.writable;
    }
}

#[derive(Clone, Copy)]
struct PlanSlot<'a> {
    storage: StorageId,
    name: &'a str,
    usage: SlotUse,
}

#[derive(Clone, Copy)]
struct OperationSite<'a> {
    operation: &'a Operation,
    block: usize,
    ordinal: usize,
}

struct ConditionTopology<'a> {
    sites: Vec<OperationSite<'a>>,
    nodes: BTreeMap<i64, Vec<usize>>,
    branches_by_parent: BTreeMap<i64, Vec<usize>>,
    successor_by_branch: BTreeMap<usize, usize>,
    owners_by_node: BTreeMap<i64, Vec<usize>>,
    contracts: Vec<DecimalConditionContract>,
}

struct LayoutIndex<'a> {
    definitions: BTreeMap<OperationIdentity, BTreeMap<String, Vec<&'a Operation>>>,
}

/// Validate every dialect contract registered by `catalog` against `module`.
///
/// Catalog entries that are absent from the module are harmless. This lets a
/// defensive runtime loader reuse the same validators with a catalog that
/// contains only its typed dialects, while the main verifier separately
/// requires schemas for every operation.
pub fn verify_semantic_contracts(
    module: &Module,
    catalog: &OperationCatalog,
) -> Result<(), SemanticVerificationProblem> {
    let topology = ConditionTopology::new(module, catalog);
    let layouts = LayoutIndex::new(module, catalog);
    for (site_index, site) in topology.sites.iter().enumerate() {
        let operation = site.operation;
        let Some(schema) = catalog.get(&operation.identity) else {
            continue;
        };
        let result = match &schema.semantic_contract {
            OperationSemanticContract::Structural => Ok(()),
            OperationSemanticContract::DecimalAssignment(contract) => {
                validate_decimal(module, operation, site_index, contract, &topology, &layouts)
            }
            OperationSemanticContract::CicsEffect(contract) => {
                validate_cics(module, operation, contract, &layouts)
            }
        };
        result.map_err(|detail| SemanticVerificationProblem {
            identity: operation.identity.clone(),
            detail,
        })?;
    }
    topology.validate_marked_branches(catalog)
}

impl<'a> LayoutIndex<'a> {
    fn new(module: &'a Module, catalog: &OperationCatalog) -> Self {
        let identities = catalog
            .identities()
            .filter_map(|identity| match &catalog.get(identity)?.semantic_contract {
                OperationSemanticContract::DecimalAssignment(contract) => {
                    contract.layout_definition_operation.clone()
                }
                OperationSemanticContract::CicsEffect(contract) => {
                    contract.layout_definition_operation.clone()
                }
                OperationSemanticContract::Structural => None,
            })
            .collect::<BTreeSet<_>>();
        let mut definitions =
            BTreeMap::<OperationIdentity, BTreeMap<String, Vec<&Operation>>>::new();
        for operation in module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| identities.contains(&operation.identity))
        {
            if let Some(Attribute::Text(name)) = operation.attributes.get("name") {
                definitions
                    .entry(operation.identity.clone())
                    .or_default()
                    .entry(name.to_ascii_uppercase())
                    .or_default()
                    .push(operation);
            }
        }
        Self { definitions }
    }

    fn get(&self, identity: &OperationIdentity, name: &str) -> Option<&[&Operation]> {
        self.definitions.get(identity)?.get(name).map(Vec::as_slice)
    }
}

impl<'a> ConditionTopology<'a> {
    fn new(module: &'a Module, catalog: &OperationCatalog) -> Self {
        let mut sites = Vec::new();
        let mut block = 0usize;
        for region in module.regions() {
            for current in &region.blocks {
                for operation in &current.operations {
                    sites.push(OperationSite {
                        operation,
                        block,
                        ordinal: sites.len(),
                    });
                }
                block += 1;
            }
        }
        let mut nodes = BTreeMap::<i64, Vec<usize>>::new();
        let mut branches_by_parent = BTreeMap::<i64, Vec<usize>>::new();
        let mut owners_by_node = BTreeMap::<i64, Vec<usize>>::new();
        let mut contracts = catalog
            .identities()
            .filter_map(|identity| match &catalog.get(identity)?.semantic_contract {
                OperationSemanticContract::DecimalAssignment(contract) => {
                    contract.condition.clone()
                }
                OperationSemanticContract::Structural
                | OperationSemanticContract::CicsEffect(_) => None,
            })
            .collect::<Vec<_>>();
        contracts.sort_by(|left, right| {
            (&left.status_attribute, &left.branch_polarity_attribute)
                .cmp(&(&right.status_attribute, &right.branch_polarity_attribute))
        });
        contracts.dedup();
        for (index, site) in sites.iter().enumerate() {
            if let Some(Attribute::Integer(node)) = site.operation.attributes.get("control_node") {
                nodes.entry(*node).or_default().push(index);
            }
            if is_branch(site.operation)
                && let Some(Attribute::Integer(parent)) =
                    site.operation.attributes.get("control_parent")
            {
                branches_by_parent.entry(*parent).or_default().push(index);
            }
            if let Some(OperationSemanticContract::DecimalAssignment(contract)) = catalog
                .get(&site.operation.identity)
                .map(|schema| &schema.semantic_contract)
                && contract.condition.is_some()
                && let Some(Attribute::Integer(node)) =
                    site.operation.attributes.get("control_node")
            {
                owners_by_node.entry(*node).or_default().push(index);
            }
        }
        let mut successor_by_branch = BTreeMap::new();
        let mut next_sibling = BTreeMap::<(usize, i64), usize>::new();
        let mut next_terminator = BTreeMap::<usize, usize>::new();
        for (index, site) in sites.iter().enumerate().rev() {
            let role = text_attribute(site.operation, "control_role");
            let parent = integer_attribute(site.operation, "control_parent");
            if role == Some("terminator") && parent.is_none() {
                next_terminator.insert(site.block, index);
            }
            if matches!(role, Some("branch" | "block_end" | "terminator"))
                && let Some(parent) = parent
            {
                if role == Some("branch")
                    && let Some(successor) = next_sibling
                        .get(&(site.block, parent))
                        .or_else(|| next_terminator.get(&site.block))
                {
                    successor_by_branch.insert(index, *successor);
                }
                next_sibling.insert((site.block, parent), index);
            }
        }
        Self {
            sites,
            nodes,
            branches_by_parent,
            successor_by_branch,
            owners_by_node,
            contracts,
        }
    }

    fn validate_owner(
        &self,
        operation: &Operation,
        owner_index: usize,
        contract: &DecimalConditionContract,
    ) -> Result<(), &'static str> {
        if self
            .sites
            .get(owner_index)
            .is_none_or(|site| !std::ptr::eq(site.operation, operation))
        {
            return Err("typed condition owner is outside the module");
        }
        let mask = condition_mask(operation, contract)?;
        let owner = match operation.attributes.get("control_node") {
            Some(Attribute::Integer(node)) => {
                if text_attribute(operation, "control_role") != Some("statement") {
                    return Err("typed condition owner is not a statement control node");
                }
                self.require_unique_node(*node, owner_index, "typed condition owner is invalid")?;
                Some(*node)
            }
            Some(_) => return Err("typed condition owner has the wrong type"),
            None if mask == 0 => None,
            None => return Err("typed condition owner is missing"),
        };
        let branches = owner
            .and_then(|node| self.branches_by_parent.get(&node))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut actual = 0u8;
        for branch in branches {
            let bit = self.validate_branch(*branch, owner_index, contract)?;
            if actual & bit != 0 {
                return Err("typed condition branch polarity is duplicated");
            }
            actual |= bit;
        }
        if actual != mask {
            return Err("typed condition branch mask does not match control topology");
        }
        Ok(())
    }

    fn validate_marked_branches(
        &self,
        catalog: &OperationCatalog,
    ) -> Result<(), SemanticVerificationProblem> {
        for (branch_index, site) in self.sites.iter().enumerate() {
            if !is_branch(site.operation) || !self.is_condition_marked(site.operation) {
                continue;
            }
            let parent = match site.operation.attributes.get("control_parent") {
                Some(Attribute::Integer(parent)) => *parent,
                _ => {
                    return Err(semantic_problem(
                        site.operation,
                        "typed condition branch parent is missing or invalid",
                    ));
                }
            };
            let Some([owner_index]) = self.owners_by_node.get(&parent).map(Vec::as_slice) else {
                return Err(semantic_problem(
                    site.operation,
                    "typed condition branch has no unique decimal owner",
                ));
            };
            let owner = self.sites[*owner_index].operation;
            let Some(OperationSemanticContract::DecimalAssignment(decimal)) = catalog
                .get(&owner.identity)
                .map(|schema| &schema.semantic_contract)
            else {
                return Err(semantic_problem(
                    site.operation,
                    "typed condition branch owner is not decimal assignment",
                ));
            };
            let Some(contract) = &decimal.condition else {
                return Err(semantic_problem(
                    site.operation,
                    "typed condition branch owner has no condition contract",
                ));
            };
            self.validate_branch(branch_index, *owner_index, contract)
                .map_err(|detail| semantic_problem(site.operation, detail))?;
        }
        Ok(())
    }

    fn is_condition_marked(&self, operation: &Operation) -> bool {
        self.contracts.iter().any(|contract| {
            operation
                .attributes
                .contains_key(&contract.status_attribute)
                || operation
                    .attributes
                    .contains_key(&contract.branch_polarity_attribute)
        })
    }

    fn validate_branch(
        &self,
        branch_index: usize,
        owner_index: usize,
        contract: &DecimalConditionContract,
    ) -> Result<u8, &'static str> {
        let branch = self.sites[branch_index];
        let owner = self.sites[owner_index];
        if branch.block != owner.block || branch.ordinal <= owner.ordinal {
            return Err("typed condition branch is outside or precedes its owner block");
        }
        let owner_node = integer_attribute(owner.operation, "control_node")
            .ok_or("typed condition owner is missing")?;
        if integer_attribute(branch.operation, "control_parent") != Some(owner_node) {
            return Err("typed condition branch parent does not match its owner");
        }
        let branch_node = integer_attribute(branch.operation, "control_node")
            .ok_or("typed condition branch has an invalid control node")?;
        self.require_unique_node(
            branch_node,
            branch_index,
            "typed condition branch has an invalid control node",
        )?;
        if branch_node == owner_node {
            return Err("typed condition branch reuses its owner control node");
        }
        match branch.operation.attributes.get(&contract.status_attribute) {
            Some(Attribute::Text(status)) if status == &contract.status => {}
            _ => return Err("typed condition branch status is missing or invalid"),
        }
        let bit = match branch
            .operation
            .attributes
            .get(&contract.branch_polarity_attribute)
        {
            Some(Attribute::Boolean(true)) => 1,
            Some(Attribute::Boolean(false)) => 2,
            _ => return Err("typed condition branch polarity is missing or invalid"),
        };
        if branch.operation.attributes.contains_key("edge_branch_true") {
            return Err("typed condition branch has an unexpected true edge");
        }
        let target_node = integer_attribute(branch.operation, "edge_branch_false")
            .ok_or("typed condition branch has an invalid false target")?;
        let target_index = self.unique_node(target_node)?;
        let target = self.sites[target_index];
        let target_role = text_attribute(target.operation, "control_role");
        let target_parent_is_valid = match target.operation.attributes.get("control_parent") {
            Some(Attribute::Integer(parent)) => *parent == owner_node,
            None => target_role == Some("terminator"),
            Some(_) => false,
        };
        if target.block != owner.block
            || target.ordinal <= branch.ordinal
            || target_index == owner_index
            || target_index == branch_index
            || self.successor_by_branch.get(&branch_index) != Some(&target_index)
            || !target_parent_is_valid
            || !matches!(target_role, Some("branch" | "block_end" | "terminator"))
        {
            return Err("typed condition branch false target violates control topology");
        }
        Ok(bit)
    }

    fn require_unique_node(
        &self,
        node: i64,
        expected_index: usize,
        detail: &'static str,
    ) -> Result<(), &'static str> {
        if node < 0 || self.unique_node(node).ok() != Some(expected_index) {
            Err(detail)
        } else {
            Ok(())
        }
    }

    fn unique_node(&self, node: i64) -> Result<usize, &'static str> {
        if node < 0 {
            return Err("control node identity is negative");
        }
        match self.nodes.get(&node).map(Vec::as_slice) {
            Some([index]) => Ok(*index),
            Some(_) => Err("control node identity is duplicated"),
            None => Err("control edge target is missing"),
        }
    }
}

fn semantic_problem(operation: &Operation, detail: &'static str) -> SemanticVerificationProblem {
    SemanticVerificationProblem {
        identity: operation.identity.clone(),
        detail,
    }
}

fn is_branch(operation: &Operation) -> bool {
    text_attribute(operation, "control_role") == Some("branch")
}

fn integer_attribute(operation: &Operation, name: &str) -> Option<i64> {
    match operation.attributes.get(name) {
        Some(Attribute::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn text_attribute<'a>(operation: &'a Operation, name: &str) -> Option<&'a str> {
    match operation.attributes.get(name) {
        Some(Attribute::Text(value)) => Some(value),
        _ => None,
    }
}

fn condition_mask(
    operation: &Operation,
    contract: &DecimalConditionContract,
) -> Result<u8, &'static str> {
    match operation.attributes.get(&contract.status_attribute) {
        Some(Attribute::Text(status)) if status == &contract.status => {}
        Some(_) => return Err("typed condition status has the wrong type or identity"),
        None => return Err("typed condition status is missing"),
    }
    match operation.attributes.get(&contract.branch_mask_attribute) {
        Some(Attribute::Integer(mask)) if (0..=3).contains(mask) => Ok(*mask as u8),
        Some(_) => Err("typed condition branch mask is invalid"),
        None => Err("typed condition branch mask is missing"),
    }
}

fn validate_decimal(
    module: &Module,
    operation: &Operation,
    operation_index: usize,
    contract: &DecimalOperationContract,
    topology: &ConditionTopology<'_>,
    layouts: &LayoutIndex<'_>,
) -> Result<(), &'static str> {
    forbid_legacy_attributes(operation)?;
    if !operation.operands.is_empty() || !operation.results.is_empty() {
        return Err("decimal operation signature is not exact");
    }
    exact_effects(
        operation,
        &[Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition],
    )?;
    let bytes = match operation.attributes.get(&contract.plan_attribute) {
        Some(Attribute::Bytes(bytes)) => bytes,
        Some(_) => return Err("decimal plan attribute has the wrong type"),
        None => return Err("decimal plan attribute is missing"),
    };
    if decimal_assignment_plan_wire_version(bytes)
        .map_err(|_| "decimal plan has an invalid or unsupported wire header")?
        != contract.expected_plan_version
    {
        return Err("decimal plan wire version does not match operation major");
    }
    let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default())
        .map_err(|_| "decimal plan is malformed, noncanonical, oversized, or unsupported")?;
    if !contract.allowed_semantic_origins.is_empty()
        && !contract
            .allowed_semantic_origins
            .contains(&plan.semantic_origin)
    {
        return Err("decimal operation identity does not match plan origin");
    }
    validate_slots(
        module,
        operation,
        decimal_slots(&plan),
        contract.layout_definition_operation.as_ref(),
        layouts,
    )?;
    if let Some(condition) = &contract.condition {
        topology.validate_owner(operation, operation_index, condition)?;
    }
    Ok(())
}

fn validate_cics(
    module: &Module,
    operation: &Operation,
    contract: &CicsOperationContract,
    layouts: &LayoutIndex<'_>,
) -> Result<(), &'static str> {
    forbid_legacy_attributes(operation)?;
    if !operation.operands.is_empty() || !operation.results.is_empty() {
        return Err("CICS operation signature is not exact");
    }
    let bytes = match operation.attributes.get(&contract.plan_attribute) {
        Some(Attribute::Bytes(bytes)) => bytes,
        Some(_) => return Err("CICS plan attribute has the wrong type"),
        None => return Err("CICS plan attribute is missing"),
    };
    let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default())
        .map_err(|_| "CICS plan is malformed, noncanonical, oversized, or unsupported")?;
    if contract
        .expected_operation
        .is_some_and(|expected| expected != plan.operation)
    {
        return Err("CICS operation identity does not match its plan");
    }
    exact_effects(
        operation,
        cics_executable_descriptor(plan.operation).effects,
    )?;
    validate_slots(
        module,
        operation,
        cics_slots(&plan),
        contract.layout_definition_operation.as_ref(),
        layouts,
    )
}

fn forbid_legacy_attributes(operation: &Operation) -> Result<(), &'static str> {
    if operation.attributes.contains_key("arguments")
        || operation.attributes.contains_key("control_text")
        || operation
            .attributes
            .keys()
            .any(|name| name.starts_with("arg_"))
    {
        Err("typed operation contains a legacy source-shaped attribute")
    } else {
        Ok(())
    }
}

fn exact_effects(operation: &Operation, expected: &[Effect]) -> Result<(), &'static str> {
    if operation.effects == expected {
        Ok(())
    } else {
        Err("typed operation effects are not exact")
    }
}

fn decimal_slots(plan: &DecimalAssignmentPlan) -> Vec<PlanSlot<'_>> {
    let mut slots = Vec::new();
    for assignment in &plan.assignments {
        slots.push(PlanSlot {
            storage: assignment.receiver.target.storage,
            name: assignment.receiver.target.qualified_layout_name.as_str(),
            usage: SlotUse::NUMERIC_WRITE,
        });
        expression_slots(&assignment.expression, &mut slots);
    }
    slots
}

fn expression_slots<'a>(expression: &'a DecimalExpression, slots: &mut Vec<PlanSlot<'a>>) {
    match expression {
        DecimalExpression::Literal { .. } => {}
        DecimalExpression::Storage(slot) => slots.push(PlanSlot {
            storage: slot.storage,
            name: slot.qualified_layout_name.as_str(),
            usage: SlotUse::NUMERIC_READ,
        }),
        DecimalExpression::Length(slot) => {
            slots.push(PlanSlot {
                storage: slot.storage,
                name: slot.qualified_layout_name.as_str(),
                usage: SlotUse::READ,
            });
        }
        DecimalExpression::Negate(value) => expression_slots(value, slots),
        DecimalExpression::Add { left, right }
        | DecimalExpression::Subtract { left, right }
        | DecimalExpression::Multiply { left, right }
        | DecimalExpression::Divide { left, right } => {
            expression_slots(left, slots);
            expression_slots(right, slots);
        }
    }
}

fn cics_slots(plan: &CicsEffectPlan) -> Vec<PlanSlot<'_>> {
    let mut slots = Vec::new();
    for operand in &plan.operands {
        if let CicsOperandValue::Storage(slot) = &operand.value {
            slots.push(PlanSlot {
                storage: slot.storage,
                name: slot.qualified_layout_name.as_str(),
                usage: SlotUse::READ,
            });
        }
    }
    slots.extend(plan.outputs.iter().map(|output| PlanSlot {
        storage: output.target.storage,
        name: output.target.qualified_layout_name.as_str(),
        usage: match output.name {
            CicsOutputName::Into => SlotUse::WRITE,
            CicsOutputName::Resp | CicsOutputName::Resp2 => SlotUse::NUMERIC_WRITE,
        },
    }));
    if let CicsCondition::Respond {
        response,
        response2,
    } = &plan.condition
    {
        slots.push(PlanSlot {
            storage: response.storage,
            name: response.qualified_layout_name.as_str(),
            usage: SlotUse::NUMERIC_WRITE,
        });
        if let Some(response2) = response2 {
            slots.push(PlanSlot {
                storage: response2.storage,
                name: response2.qualified_layout_name.as_str(),
                usage: SlotUse::NUMERIC_WRITE,
            });
        }
    }
    slots
}

fn validate_slots(
    module: &Module,
    operation: &Operation,
    slots: Vec<PlanSlot<'_>>,
    layout_definition: Option<&OperationIdentity>,
    layouts: &LayoutIndex<'_>,
) -> Result<(), &'static str> {
    let mut expected = BTreeMap::<StorageId, (String, SlotUse)>::new();
    let mut plan_names = BTreeMap::<String, StorageId>::new();
    let mut storage_names = BTreeMap::<String, Vec<StorageId>>::new();
    for storage in module.storage() {
        storage_names
            .entry(storage.name.to_ascii_uppercase())
            .or_default()
            .push(storage.id);
    }
    for slot in slots {
        if let Some(prior) = expected.get_mut(&slot.storage) {
            if prior.0 != slot.name {
                return Err("one plan storage ID names multiple layouts");
            }
            prior.1.merge(slot.usage);
            continue;
        }
        if plan_names
            .insert(slot.name.to_string(), slot.storage)
            .is_some_and(|prior| prior != slot.storage)
        {
            return Err("one plan layout name identifies multiple storage IDs");
        }
        let id = slot.storage;
        let name = slot.name;
        let Some(storage) = module.storage().get(id.get() as usize) else {
            return Err("plan storage ID is outside the module");
        };
        if !storage.name.eq_ignore_ascii_case(name) {
            return Err("plan storage ID does not name its qualified layout");
        }
        if storage_names
            .get(&name.to_ascii_uppercase())
            .map(Vec::as_slice)
            != Some(&[id])
        {
            return Err("plan qualified layout name is not a unique storage binding");
        }
        expected.insert(id, (name.to_string(), slot.usage));
    }
    let mut declared = BTreeSet::new();
    for reference in &operation.storage {
        let storage = module
            .storage()
            .get(reference.storage.get() as usize)
            .ok_or("operation storage declaration is outside the module")?;
        if reference.offset != 0
            || reference.length != storage.size
            || !declared.insert(reference.storage)
        {
            return Err("operation storage declarations are not unique exact extents");
        }
    }
    if declared != expected.keys().copied().collect() {
        return Err("operation storage declarations differ from plan slots");
    }
    if let Some(definition_identity) = layout_definition {
        for (storage_id, (name, usage)) in expected {
            let Some([definition]) = layouts.get(definition_identity, &name) else {
                return Err("plan layout has no unique executable definition");
            };
            let storage = &module.storage()[storage_id.get() as usize];
            validate_layout_definition(definition, name.as_str(), storage.size, usage)?;
        }
    }
    Ok(())
}

fn validate_layout_definition(
    definition: &Operation,
    expected_name: &str,
    storage_extent: u64,
    usage: SlotUse,
) -> Result<(), &'static str> {
    if text_attribute(definition, "name") != Some(expected_name) {
        return Err("plan layout definition name is not exact");
    }
    let category = text_attribute(definition, "category")
        .ok_or("plan layout category is missing or has the wrong type")?;
    if !is_known_layout(category) {
        return Err("plan layout category is unsupported");
    }
    if usage.numeric && !is_numeric_layout(category) {
        return Err("decimal or response-code slot is not numeric storage");
    }
    if usage.writable && matches!(category, "condition" | "rename") {
        return Err("plan receiver is not writable storage");
    }
    let length = nonnegative_integer_attribute(definition, "length")
        .ok_or("plan layout length is missing or invalid")?;
    let dynamic = match definition.attributes.get("dynamic") {
        None => false,
        Some(Attribute::Integer(value)) => *value != 0,
        Some(_) => return Err("plan layout dynamic marker has the wrong type"),
    };
    let extent = if dynamic {
        nonnegative_integer_attribute(definition, "dynamic_limit")
            .ok_or("dynamic plan layout limit is missing or invalid")?
    } else {
        length
    };
    if extent == 0 || extent != storage_extent {
        return Err("plan layout extent does not match its storage view");
    }
    Ok(())
}

fn nonnegative_integer_attribute(operation: &Operation, name: &str) -> Option<u64> {
    integer_attribute(operation, name).and_then(|value| u64::try_from(value).ok())
}

fn is_numeric_layout(category: &str) -> bool {
    matches!(
        category,
        "numeric_display"
            | "numeric_edited"
            | "packed_decimal"
            | "binary"
            | "float_short"
            | "float_long"
    )
}

fn is_known_layout(category: &str) -> bool {
    matches!(
        category,
        "alphabetic"
            | "alphanumeric"
            | "alphanumeric_edited"
            | "binary"
            | "condition"
            | "dbcs"
            | "float_long"
            | "float_short"
            | "function_pointer"
            | "group"
            | "index"
            | "national"
            | "national_edited"
            | "national_group"
            | "numeric_display"
            | "numeric_edited"
            | "object_reference"
            | "packed_decimal"
            | "pointer"
            | "pointer_32"
            | "procedure_pointer"
            | "rename"
            | "utf8"
            | "utf8_group"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CicsNamedOperand, CicsOperandName, CicsOutputBinding, CicsOutputName, CicsPlanOperation,
        CicsStorageSlot, IrLimits, LegalityProfile, ModuleBuilder, OperationSchema,
        StorageReference, encode_cics_effect_plan, verify_legal,
    };

    fn read_plan(key: StorageId, record: StorageId) -> CicsEffectPlan {
        CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(CicsStorageSlot {
                        storage: key,
                        qualified_layout_name: "KEY".into(),
                    }),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: record,
                    qualified_layout_name: "RECORD".into(),
                },
            }],
            condition: CicsCondition::Default,
        }
    }

    fn module_and_catalog(
        attribute: Attribute,
        effects: Vec<Effect>,
        forge_slot_name: bool,
    ) -> (Module, OperationCatalog, LegalityProfile) {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let key = builder.add_storage("key", 2, None).unwrap();
        let record = builder.add_storage("record", 8, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new("cics.file", "read", 1).unwrap();
        let halt = OperationIdentity::new("test", "halt", 1).unwrap();
        let storage = if forge_slot_name {
            vec![StorageReference {
                storage: record,
                offset: 0,
                length: 8,
            }]
        } else {
            vec![
                StorageReference {
                    storage: key,
                    offset: 0,
                    length: 2,
                },
                StorageReference {
                    storage: record,
                    offset: 0,
                    length: 8,
                },
            ]
        };
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                BTreeMap::from([("cics_plan".into(), attribute)]),
                effects,
                storage,
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                halt.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let mut catalog = OperationCatalog::default();
        let mut cics = OperationSchema::pure(identity.clone(), 0, 0);
        cics.required_attributes = BTreeSet::from(["cics_plan".into()]);
        cics.allowed_effects = cics_executable_descriptor(CicsPlanOperation::Read)
            .effects
            .iter()
            .copied()
            .collect();
        cics.semantic_contract = OperationSemanticContract::CicsEffect(CicsOperationContract {
            plan_attribute: "cics_plan".into(),
            expected_operation: Some(CicsPlanOperation::Read),
            layout_definition_operation: None,
        });
        catalog.register(cics).unwrap();
        let mut terminator = OperationSchema::pure(halt.clone(), 0, 0);
        terminator.terminator = true;
        catalog.register(terminator).unwrap();
        let profile = LegalityProfile {
            allowed_operations: BTreeSet::from([identity, halt]),
            allowed_runtime_imports: BTreeSet::new(),
        };
        (builder.finish().unwrap(), catalog, profile)
    }

    #[test]
    fn cics_typed_semantics_are_proven_before_legalization() {
        let mut storage = ModuleBuilder::new(IrLimits::default());
        let key = storage.add_storage("key", 2, None).unwrap();
        let record = storage.add_storage("record", 8, None).unwrap();
        let plan =
            encode_cics_effect_plan(&read_plan(key, record), CicsPlanLimits::default()).unwrap();
        let (module, catalog, profile) = module_and_catalog(
            Attribute::Bytes(plan.clone()),
            cics_executable_descriptor(CicsPlanOperation::Read)
                .effects
                .to_vec(),
            false,
        );
        assert!(verify_legal(module, &catalog, &profile).is_ok());
        let (forged, catalog, profile) = module_and_catalog(
            Attribute::Bytes(plan),
            cics_executable_descriptor(CicsPlanOperation::Read)
                .effects
                .to_vec(),
            true,
        );
        assert!(matches!(
            verify_legal(forged, &catalog, &profile),
            Err(crate::VerificationProblem::SemanticMismatch(_))
        ));
        let (missing_effects, catalog, profile) = module_and_catalog(
            Attribute::Bytes(
                encode_cics_effect_plan(&read_plan(key, record), CicsPlanLimits::default())
                    .unwrap(),
            ),
            vec![Effect::DatasetRead],
            false,
        );
        assert!(matches!(
            verify_legal(missing_effects, &catalog, &profile),
            Err(crate::VerificationProblem::SemanticMismatch(_))
        ));
    }

    #[test]
    fn malformed_type_effect_and_slot_fail_before_legalization() {
        for (attribute, effects, forged_slot) in [
            (
                Attribute::Bytes(Vec::new()),
                cics_executable_descriptor(CicsPlanOperation::Read)
                    .effects
                    .to_vec(),
                false,
            ),
            (
                Attribute::Text("not-bytes".into()),
                cics_executable_descriptor(CicsPlanOperation::Read)
                    .effects
                    .to_vec(),
                false,
            ),
            (
                Attribute::Bytes(Vec::new()),
                vec![Effect::DatasetRead],
                false,
            ),
        ] {
            let (module, catalog, profile) = module_and_catalog(attribute, effects, forged_slot);
            assert!(matches!(
                verify_legal(module, &catalog, &profile),
                Err(crate::VerificationProblem::SemanticMismatch(_))
                    | Err(crate::VerificationProblem::EffectMismatch(_))
            ));
        }
    }

    #[test]
    fn cics_plan_operation_must_match_the_executable_identity() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        let (module, catalog, profile) = module_and_catalog(
            Attribute::Bytes(bytes),
            cics_executable_descriptor(CicsPlanOperation::Read)
                .effects
                .to_vec(),
            false,
        );
        assert!(matches!(
            verify_legal(module, &catalog, &profile),
            Err(crate::VerificationProblem::SemanticMismatch(_))
        ));
    }

    #[test]
    fn structural_dialects_do_not_inherit_cobol_control_attribute_rules() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let step = OperationIdentity::new("foreign", "step", 1).unwrap();
        let duplicate = OperationIdentity::new("foreign", "duplicate", 1).unwrap();
        let return_identity = OperationIdentity::new("foreign", "return", 1).unwrap();
        builder
            .add_operation(
                block,
                step.clone(),
                Vec::new(),
                0,
                BTreeMap::from([(
                    "control_node".into(),
                    Attribute::Text("foreign-vocabulary".into()),
                )]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                duplicate.clone(),
                Vec::new(),
                0,
                BTreeMap::from([("control_node".into(), Attribute::Integer(7))]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                return_identity.clone(),
                Vec::new(),
                0,
                BTreeMap::from([("control_node".into(), Attribute::Integer(7))]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let mut catalog = OperationCatalog::default();
        catalog
            .register(OperationSchema::pure(step.clone(), 0, 0))
            .unwrap();
        catalog
            .register(OperationSchema::pure(duplicate.clone(), 0, 0))
            .unwrap();
        let mut return_schema = OperationSchema::pure(return_identity.clone(), 0, 0);
        return_schema.terminator = true;
        catalog.register(return_schema).unwrap();
        let profile = LegalityProfile {
            allowed_operations: BTreeSet::from([step, duplicate, return_identity]),
            allowed_runtime_imports: BTreeSet::new(),
        };
        assert!(verify_legal(builder.finish().unwrap(), &catalog, &profile).is_ok());
    }
}
