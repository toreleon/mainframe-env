//! Bounded dialect-owned verification for typed executable plans.

use crate::cobol_layout::{
    CobolLayoutAbi, alternate_primary_name, is_numeric_layout, validate_address_width,
    validate_definition, validate_typed_numeric,
};
use crate::{
    Attribute, CicsCondition, CicsEffectPlan, CicsOperandValue, CicsOperationContract,
    CicsOutputName, CicsPlanLimits, DecimalAssignmentPlan, DecimalConditionContract,
    DecimalExpression, DecimalOperationContract, DecimalPlanLimits, Effect, Module, Operation,
    OperationCatalog, OperationId, OperationIdentity, OperationSemanticContract, StorageId,
    cics_executable_descriptor, decimal_assignment_plan_wire_version, decode_cics_effect_plan,
    decode_decimal_assignment_plan,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod layout_relations;
mod layout_storage;

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
    abis: BTreeMap<OperationId, Result<CobolLayoutAbi<'a>, &'static str>>,
    storage: BTreeMap<String, Vec<&'a crate::StorageRegion>>,
    storage_by_id: &'a [crate::StorageRegion],
    address_mode: Result<Option<crate::CobolAddressMode>, &'static str>,
    odo_descendant_ancestors: BTreeSet<(OperationIdentity, String)>,
    alias_participants: BTreeSet<(OperationIdentity, String)>,
    occurs_descendants: BTreeMap<(OperationIdentity, String), Vec<(OperationId, String)>>,
    odo_by_root: BTreeMap<(OperationIdentity, String), Vec<OperationId>>,
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
            OperationSemanticContract::CobolLayoutDefinition => {
                validate_cobol_layout_site(operation, &layouts)
            }
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
                OperationSemanticContract::CobolLayoutDefinition => Some(identity.clone()),
                OperationSemanticContract::Structural => None,
            })
            .collect::<BTreeSet<_>>();
        let mut definitions =
            BTreeMap::<OperationIdentity, BTreeMap<String, Vec<&Operation>>>::new();
        let mut abis = BTreeMap::new();
        let mut storage = BTreeMap::<String, Vec<&crate::StorageRegion>>::new();
        let mut odo_descendant_ancestors = BTreeSet::new();
        let mut alias_participants = BTreeSet::new();
        let mut occurs_descendants = BTreeMap::new();
        let mut odo_by_root = BTreeMap::new();
        let address_mode = crate::cobol_runtime_config(module)
            .map(|config| config.and_then(|config| config.address_mode))
            .map_err(|_| "COBOL runtime configuration is malformed");
        for operation in module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| identities.contains(&operation.identity))
        {
            let abi = validate_definition(operation);
            if let Ok(layout) = abi
                && !layout.depending_on.is_empty()
            {
                let root = layout
                    .name
                    .split('.')
                    .next()
                    .unwrap_or(layout.name)
                    .to_ascii_uppercase();
                odo_by_root
                    .entry((operation.identity.clone(), root))
                    .or_insert_with(Vec::new)
                    .push(layout.id);
                let mut ancestor = layout.name;
                while let Some((parent, _)) = ancestor.rsplit_once('.') {
                    odo_descendant_ancestors
                        .insert((operation.identity.clone(), parent.to_ascii_uppercase()));
                    ancestor = parent;
                }
            }
            if let Ok(layout) = abi
                && layout.occurs_clause
            {
                let mut ancestor = layout.parent;
                while !ancestor.is_empty() {
                    occurs_descendants
                        .entry((operation.identity.clone(), ancestor.to_ascii_uppercase()))
                        .or_insert_with(Vec::new)
                        .push((layout.id, layout.name.to_ascii_uppercase()));
                    ancestor = ancestor.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
            }
            if let Ok(layout) = abi
                && !layout.alias_of.is_empty()
                && layout.category != "condition"
            {
                alias_participants
                    .insert((operation.identity.clone(), layout.name.to_ascii_uppercase()));
                alias_participants.insert((
                    operation.identity.clone(),
                    layout.alias_of.to_ascii_uppercase(),
                ));
            }
            abis.insert(operation.id, abi);
            if let Some(Attribute::Text(name)) = operation.attributes.get("name") {
                definitions
                    .entry(operation.identity.clone())
                    .or_default()
                    .entry(name.to_ascii_uppercase())
                    .or_default()
                    .push(operation);
            }
        }
        for region in module.storage() {
            storage
                .entry(region.name.to_ascii_uppercase())
                .or_default()
                .push(region);
        }
        Self {
            definitions,
            abis,
            storage,
            storage_by_id: module.storage(),
            address_mode,
            odo_descendant_ancestors,
            alias_participants,
            occurs_descendants,
            odo_by_root,
        }
    }

    fn get(&self, identity: &OperationIdentity, name: &str) -> Option<&[&Operation]> {
        self.definitions
            .get(identity)?
            .get(&name.to_ascii_uppercase())
            .map(Vec::as_slice)
    }

    fn abi(&self, definition: &Operation) -> Result<CobolLayoutAbi<'a>, &'static str> {
        self.abis
            .get(&definition.id)
            .copied()
            .ok_or("COBOL layout definition is missing from its ABI index")?
    }

    fn resolve(
        &self,
        identity: &OperationIdentity,
        owner: &CobolLayoutAbi<'_>,
        reference: &str,
    ) -> Result<CobolLayoutAbi<'a>, &'static str> {
        let Some(definitions) = self.definitions.get(identity) else {
            return Err("COBOL layout reference has no dialect index");
        };
        let owner_components = owner
            .name
            .split('.')
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for definition in definitions.values().flatten() {
            let candidate = self.abi(definition)?;
            if !crate::cobol_layout_reference_matches(
                candidate.name,
                candidate.simple_name,
                reference,
            ) {
                continue;
            }
            let proximity = owner_components
                .iter()
                .zip(candidate.name.split('.').map(str::to_ascii_uppercase))
                .take_while(|(left, right)| **left == *right)
                .count();
            candidates.push((proximity, candidate));
        }
        candidates.sort_by_key(|(proximity, _)| std::cmp::Reverse(*proximity));
        match candidates.as_slice() {
            [(_, candidate)] => Ok(*candidate),
            [(best, candidate), rest @ ..] if rest.first().is_some_and(|(next, _)| next < best) => {
                Ok(*candidate)
            }
            _ => Err("COBOL layout reference is missing or ambiguous"),
        }
    }

    fn resolve_key(
        &self,
        identity: &OperationIdentity,
        owner: &CobolLayoutAbi<'_>,
        reference: &str,
    ) -> Result<CobolLayoutAbi<'a>, &'static str> {
        if !reference.contains('.') && reference.split_whitespace().count() == 1 {
            let prefix = format!("{}.", owner.name.to_ascii_uppercase());
            let mut candidates = self
                .definitions
                .get(identity)
                .into_iter()
                .flat_map(|definitions| definitions.values())
                .flatten()
                .map(|definition| self.abi(definition))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|candidate| {
                    candidate.simple_name.eq_ignore_ascii_case(reference)
                        && candidate.name.to_ascii_uppercase().starts_with(&prefix)
                });
            if let Some(candidate) = candidates.next() {
                if candidates.next().is_none() {
                    return Ok(candidate);
                }
                return Err("COBOL table key is ambiguous within its subject");
            }
        }
        self.resolve(identity, owner, reference)
    }

    fn has_odo_descendant(&self, identity: &OperationIdentity, name: &str) -> bool {
        self.odo_descendant_ancestors
            .contains(&(identity.clone(), name.to_ascii_uppercase()))
    }

    fn participates_in_alias(&self, identity: &OperationIdentity, name: &str) -> bool {
        self.alias_participants
            .contains(&(identity.clone(), name.to_ascii_uppercase()))
    }

    fn key_follows_nested_table(
        &self,
        identity: &OperationIdentity,
        owner: &str,
        target: &CobolLayoutAbi<'_>,
    ) -> bool {
        let Some(entries) = self
            .occurs_descendants
            .get(&(identity.clone(), owner.to_ascii_uppercase()))
        else {
            return false;
        };
        let before = entries.partition_point(|(id, _)| *id < target.id);
        let target_name = target.name.to_ascii_uppercase();
        entries[..before]
            .iter()
            .rev()
            .any(|(_, table)| !target_name.starts_with(&format!("{table}.")))
    }

    fn follows_odo_in_record(
        &self,
        identity: &OperationIdentity,
        target: &CobolLayoutAbi<'_>,
    ) -> bool {
        let root = target
            .name
            .split('.')
            .next()
            .unwrap_or(target.name)
            .to_ascii_uppercase();
        self.odo_by_root
            .get(&(identity.clone(), root))
            .is_some_and(|ids| ids.partition_point(|id| *id < target.id) > 0)
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
                | OperationSemanticContract::CobolLayoutDefinition
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
            CicsOutputName::Commarea
            | CicsOutputName::Into
            | CicsOutputName::Ridfld
            | CicsOutputName::Mmddyy
            | CicsOutputName::Mmddyyyy
            | CicsOutputName::Time
            | CicsOutputName::Yyddd
            | CicsOutputName::Yymmdd
            | CicsOutputName::Yyyymmdd
            | CicsOutputName::Assign(_) => SlotUse::WRITE,
            CicsOutputName::Abstime
            | CicsOutputName::Milliseconds
            | CicsOutputName::Resp
            | CicsOutputName::Resp2 => SlotUse::NUMERIC_WRITE,
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
            validate_layout_definition(
                layouts.abi(definition)?,
                name.as_str(),
                storage.size,
                usage,
            )?;
        }
    }
    Ok(())
}

fn validate_layout_definition(
    layout: CobolLayoutAbi<'_>,
    expected_name: &str,
    storage_extent: u64,
    usage: SlotUse,
) -> Result<(), &'static str> {
    if layout.name != expected_name {
        return Err("plan layout definition name is not exact");
    }
    if usage.numeric && !is_numeric_layout(layout.category) {
        return Err("decimal or response-code slot is not numeric storage");
    }
    if usage.writable && matches!(layout.category, "condition" | "rename") {
        return Err("plan receiver is not writable storage");
    }
    let extent = if layout.dynamic {
        layout.dynamic_limit
    } else {
        layout.length
    };
    if extent == 0 || extent != storage_extent {
        return Err("plan layout extent does not match its storage view");
    }
    if usage.numeric {
        validate_typed_numeric(&layout)?;
    }
    Ok(())
}

fn validate_cobol_layout_site(
    definition: &Operation,
    layouts: &LayoutIndex<'_>,
) -> Result<(), &'static str> {
    let layout = layouts.abi(definition)?;
    match layouts.get(&definition.identity, layout.name) {
        Some([indexed]) if std::ptr::eq(*indexed, definition) => {
            validate_address_width(&layout, layouts.address_mode?)?;
            layouts.validate_storage_topology(&definition.identity, &layout)?;
            if layout.dynamic && layouts.participates_in_alias(&definition.identity, layout.name) {
                return Err("dynamic COBOL layout cannot participate in an alias hierarchy");
            }
            let mut ancestor = layout.parent;
            while !ancestor.is_empty() {
                let Some([parent_operation]) = layouts.get(&definition.identity, ancestor) else {
                    return Err("COBOL layout parent has no unique definition");
                };
                if parent_operation.id >= definition.id {
                    return Err("COBOL layout parent is not defined before its child");
                }
                let parent = layouts.abi(parent_operation)?;
                if layout.dynamic
                    && (parent.occurs_clause
                        || layouts.participates_in_alias(&definition.identity, parent.name))
                {
                    return Err("dynamic COBOL layout cannot be subordinate to a table or alias");
                }
                ancestor = parent.parent;
            }
            if let Some(primary) = alternate_primary_name(&layout)
                && !matches!(
                    layouts.get(&definition.identity, &primary),
                    Some([target]) if target.id < definition.id
                )
            {
                return Err("alternate COBOL layout has no unique primary definition");
            }
            match layout.category {
                "condition" => match (layout.parent, layout.alias_of) {
                    ("", "") if !layout.condition_values.is_empty() => {
                        let values = layout.condition_values.split('\u{1f}').collect::<Vec<_>>();
                        crate::validate_cobol_level78_value(&values)?;
                    }
                    (parent, alias)
                        if !parent.is_empty()
                            && parent.eq_ignore_ascii_case(alias)
                            && !layout.condition_values.is_empty() =>
                    {
                        let Some([target]) = layouts.get(&definition.identity, alias) else {
                            return Err("COBOL condition has no unique conditional variable");
                        };
                        if target.id >= definition.id {
                            return Err("COBOL condition does not name a prior variable");
                        }
                        let target = layouts.abi(target)?;
                        if matches!(target.category, "condition" | "rename") {
                            return Err("COBOL condition variable category is ineligible");
                        }
                        let values = layout.condition_values.split('\u{1f}').collect::<Vec<_>>();
                        crate::validate_cobol_condition_values(
                            target.category,
                            target.digits,
                            target.scale,
                            target.signed,
                            target.element_length,
                            &values,
                        )?;
                    }
                    _ => return Err("COBOL condition association is malformed"),
                },
                "rename" => {
                    layout_relations::validate_rename(definition, &layout, layouts)?;
                }
                _ if !layout.alias_of.is_empty() => {
                    let Some([target]) = layouts.get(&definition.identity, layout.alias_of) else {
                        return Err("COBOL layout alias has no unique prior definition");
                    };
                    if target.id >= definition.id {
                        return Err("COBOL layout alias does not name a prior definition");
                    }
                    let target = layouts.abi(target)?;
                    if !target.parent.eq_ignore_ascii_case(layout.parent)
                        || target.occurs_clause
                        || layout.dynamic
                        || target.dynamic
                    {
                        return Err("COBOL layout alias target is ineligible or at another level");
                    }
                }
                _ => {}
            }
            if !layout.depending_on.is_empty()
                && !layout.depending_on.eq_ignore_ascii_case("EIBCALEN")
            {
                let target = layouts.resolve(&definition.identity, &layout, layout.depending_on)?;
                let mut target_ancestor = target.parent;
                while !target_ancestor.is_empty() {
                    let Some([parent]) = layouts.get(&definition.identity, target_ancestor) else {
                        return Err("COBOL OCCURS object has a broken parent hierarchy");
                    };
                    let parent = layouts.abi(parent)?;
                    if parent.occurs_clause {
                        return Err("COBOL OCCURS object is subordinate to a table");
                    }
                    target_ancestor = parent.parent;
                }
                if !matches!(
                    target.category,
                    "numeric_display" | "packed_decimal" | "binary"
                ) || target.simple_name.eq_ignore_ascii_case("FILLER")
                    || target.scale != 0
                    || target.dynamic
                    || target.occurs_clause
                    || layouts.runtime_storage(&target).is_err()
                    || layouts.follows_odo_in_record(&definition.identity, &target)
                    || target
                        .name
                        .to_ascii_uppercase()
                        .starts_with(&format!("{}.", layout.name.to_ascii_uppercase()))
                {
                    return Err("COBOL OCCURS DEPENDING ON binding is not an eligible integer");
                }
            }
            let mut key_extent = 0u64;
            let key_count = layout
                .keys
                .split('\u{1f}')
                .filter(|key| !key.is_empty())
                .count();
            for encoded in layout.keys.split('\u{1f}').filter(|key| !key.is_empty()) {
                let (_, reference) = encoded
                    .split_once(':')
                    .ok_or("COBOL layout key metadata is malformed")?;
                let target = layouts.resolve_key(&definition.identity, &layout, reference)?;
                let owner = layout.name.to_ascii_uppercase();
                let target_name = target.name.to_ascii_uppercase();
                if target.simple_name.eq_ignore_ascii_case("FILLER")
                    || !crate::cobol_table_key_category_is_eligible(target.category)
                    || layouts.has_odo_descendant(&definition.identity, target.name)
                {
                    return Err("COBOL table key category or subtree is ineligible");
                }
                if target_name == owner {
                    if key_count != 1 {
                        return Err("a COBOL table subject key must be the only key");
                    }
                } else {
                    if !target_name.starts_with(&format!("{owner}."))
                        || target.occurs_clause
                        || target.dynamic
                        || target.unbounded
                        || layouts.key_follows_nested_table(
                            &definition.identity,
                            layout.name,
                            &target,
                        )
                    {
                        return Err("COBOL table key is outside its eligible subject subtree");
                    }
                    let mut key_ancestor = target.parent;
                    while !key_ancestor.eq_ignore_ascii_case(layout.name) {
                        let Some([parent]) = layouts.get(&definition.identity, key_ancestor) else {
                            return Err("COBOL table key has a broken parent hierarchy");
                        };
                        let parent = layouts.abi(parent)?;
                        if parent.occurs_clause {
                            return Err("COBOL table key is nested below another table");
                        }
                        if parent.parent.is_empty() {
                            return Err("COBOL table key is outside its subject hierarchy");
                        }
                        key_ancestor = parent.parent;
                    }
                }
                key_extent = key_extent
                    .checked_add(target.element_length)
                    .filter(|extent| *extent <= crate::COBOL_MAX_TABLE_KEY_BYTES)
                    .ok_or("COBOL table key extent exceeds its static ABI limit")?;
            }
            Ok(())
        }
        Some(_) => Err("COBOL layout definition name is duplicated"),
        None => Err("COBOL layout definition is missing from its dialect index"),
    }
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

    fn layout_attributes(
        name: &str,
        category: &str,
        picture: &str,
        digits: i64,
        scale: i64,
        signed: bool,
        length: i64,
    ) -> BTreeMap<String, Attribute> {
        BTreeMap::from([
            ("name".into(), Attribute::Text(name.into())),
            ("simple_name".into(), Attribute::Text(name.into())),
            ("category".into(), Attribute::Text(category.into())),
            ("picture".into(), Attribute::Text(picture.into())),
            ("digits".into(), Attribute::Integer(digits)),
            ("scale".into(), Attribute::Integer(scale)),
            ("signed".into(), Attribute::Integer(i64::from(signed))),
            ("sign_separate".into(), Attribute::Integer(0)),
            ("section".into(), Attribute::Text("working".into())),
            ("offset".into(), Attribute::Integer(0)),
            ("length".into(), Attribute::Integer(length)),
            ("element_length".into(), Attribute::Integer(length)),
            ("occurs".into(), Attribute::Integer(1)),
            ("parent".into(), Attribute::Text(String::new())),
            ("condition_values".into(), Attribute::Text(String::new())),
        ])
    }

    fn verify_layout_definitions(
        definitions: Vec<BTreeMap<String, Attribute>>,
    ) -> Result<crate::LegalModule, crate::VerificationProblem> {
        verify_layout_definitions_with_storage_mode(definitions, true, true, None)
    }

    fn verify_layout_definitions_with_storage(
        definitions: Vec<BTreeMap<String, Attribute>>,
        include_runtime_storage: bool,
    ) -> Result<crate::LegalModule, crate::VerificationProblem> {
        verify_layout_definitions_with_storage_mode(
            definitions,
            include_runtime_storage,
            true,
            None,
        )
    }

    fn verify_layout_definitions_with_storage_mode(
        definitions: Vec<BTreeMap<String, Attribute>>,
        include_runtime_storage: bool,
        link_views: bool,
        address_mode: Option<&str>,
    ) -> Result<crate::LegalModule, crate::VerificationProblem> {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let mut declared_storage = BTreeMap::<String, (StorageId, u64, u64)>::new();
        for attributes in definitions.iter().filter(|_| include_runtime_storage) {
            let Some(Attribute::Text(name)) = attributes.get("name") else {
                continue;
            };
            let length = match attributes.get("length") {
                Some(Attribute::Integer(value)) if *value > 0 => *value as u64,
                _ if matches!(attributes.get("dynamic"), Some(Attribute::Integer(1))) => {
                    match attributes.get("dynamic_limit") {
                        Some(Attribute::Integer(value)) if *value > 0 => *value as u64,
                        _ => 0,
                    }
                }
                _ => 0,
            };
            if length > 0 {
                let offset = match attributes.get("offset") {
                    Some(Attribute::Integer(value)) if *value >= 0 => *value as u64,
                    _ => 0,
                };
                let dynamic = matches!(attributes.get("dynamic"), Some(Attribute::Integer(1)));
                let unbounded = matches!(attributes.get("unbounded"), Some(Attribute::Integer(1)));
                let parent = match attributes.get("parent") {
                    Some(Attribute::Text(value)) => value.as_str(),
                    _ => "",
                };
                let alias = match attributes.get("alias_of") {
                    Some(Attribute::Text(value)) => value.as_str(),
                    _ => "",
                };
                let owner = if !parent.is_empty() { parent } else { alias };
                let view = (link_views && !dynamic && !unbounded && !owner.is_empty())
                    .then(|| declared_storage.get(&owner.to_ascii_uppercase()))
                    .flatten()
                    .and_then(|(storage, owner_offset, owner_length)| {
                        let relative = offset.checked_sub(*owner_offset)?;
                        (relative.checked_add(length)? <= *owner_length).then_some(
                            StorageReference {
                                storage: *storage,
                                offset: relative,
                                length,
                            },
                        )
                    });
                if let Ok(storage) = builder.add_storage(name.to_ascii_lowercase(), length, view) {
                    declared_storage.insert(name.to_ascii_uppercase(), (storage, offset, length));
                }
            }
        }
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let define = crate::cobol_layout_definition_identity();
        let config = address_mode.map(|address_mode| {
            let identity = OperationIdentity::new("mainframe.core.cobol", "config", 1).unwrap();
            builder
                .add_operation(
                    block,
                    identity.clone(),
                    Vec::new(),
                    0,
                    BTreeMap::from([
                        ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                        ("display_sign".into(), Attribute::Text("compatible".into())),
                        ("address_mode".into(), Attribute::Text(address_mode.into())),
                    ]),
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
            identity
        });
        for attributes in definitions {
            builder
                .add_operation(
                    block,
                    define.clone(),
                    Vec::new(),
                    0,
                    attributes,
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        let halt = OperationIdentity::new("test", "halt", 1).unwrap();
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
        catalog
            .register(crate::cobol_layout_definition_schema())
            .unwrap();
        if let Some(config) = &config {
            catalog
                .register(OperationSchema::pure(config.clone(), 0, 0))
                .unwrap();
        }
        let mut halt_schema = OperationSchema::pure(halt.clone(), 0, 0);
        halt_schema.terminator = true;
        catalog.register(halt_schema).unwrap();
        let mut allowed_operations = BTreeSet::from([define, halt]);
        allowed_operations.extend(config);
        verify_legal(
            builder.finish().unwrap(),
            &catalog,
            &LegalityProfile {
                allowed_operations,
                allowed_runtime_imports: BTreeSet::new(),
            },
        )
    }

    #[test]
    fn cobol_layout_schema_accepts_supported_numeric_and_dynamic_abis() {
        let mut dynamic = layout_attributes("DYNAMIC", "alphanumeric", "X", 0, 0, false, 0);
        dynamic.insert("element_length".into(), Attribute::Integer(1));
        dynamic.insert("dynamic".into(), Attribute::Integer(1));
        dynamic.insert("dynamic_limit".into(), Attribute::Integer(100));
        let mut dynamic_utf8 = layout_attributes("DYNAMIC-U", "utf8", "U", 0, 0, false, 0);
        dynamic_utf8.insert("element_length".into(), Attribute::Integer(4));
        dynamic_utf8.insert("dynamic".into(), Attribute::Integer(1));
        dynamic_utf8.insert("dynamic_limit".into(), Attribute::Integer(100));
        let mut float_occurs = layout_attributes("FLOAT-TABLE", "float_short", "", 0, 0, false, 8);
        float_occurs.insert("element_length".into(), Attribute::Integer(4));
        float_occurs.insert("occurs".into(), Attribute::Integer(2));
        float_occurs.insert("occurs_min".into(), Attribute::Integer(2));
        float_occurs.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut national_blank =
            layout_attributes("NATIONAL-BLANK", "numeric_edited", "9(2)", 2, 0, false, 4);
        national_blank.insert("blank_when_zero".into(), Attribute::Integer(1));
        let mut indexed_once = layout_attributes("INDEXED-ONCE", "group", "", 0, 0, false, 1);
        indexed_once.insert("occurs_clause".into(), Attribute::Integer(1));
        indexed_once.insert("indexes".into(), Attribute::Text("IX_".into()));
        let definitions = vec![
            layout_attributes("DISPLAY", "numeric_display", "S9(3)V99", 5, 2, true, 5),
            layout_attributes("EDITED", "numeric_edited", "ZZ9.99-", 5, 2, true, 7),
            layout_attributes("PACKED", "packed_decimal", "S9(7)V99", 9, 2, true, 5),
            layout_attributes("BINARY", "binary", "S9(9)", 9, 0, true, 4),
            layout_attributes("FLOAT-SHORT", "float_short", "", 0, 0, false, 4),
            layout_attributes("FLOAT-LONG", "float_long", "", 0, 0, false, 8),
            layout_attributes("P-ONLY", "numeric_display", "P(2)", 2, 0, false, 1),
            layout_attributes("SIGN-ONLY", "numeric_edited", "+", 0, 0, true, 1),
            layout_attributes("NATIONAL-EDITED", "national_edited", "ZZ9", 3, 0, false, 6),
            layout_attributes(
                "PICTURE-BOUNDARY",
                "numeric_display",
                "9(1000000)",
                1_000_000,
                0,
                false,
                1_000_000,
            ),
            layout_attributes(
                "NATIONAL-NUMERIC",
                "numeric_display",
                "9(2)",
                2,
                0,
                false,
                4,
            ),
            national_blank,
            float_occurs,
            dynamic,
            dynamic_utf8,
            indexed_once,
        ];
        verify_layout_definitions(definitions)
            .expect("every supported layout representation should verify");
    }

    #[test]
    fn cobol_layout_schema_rejects_wrong_types_domains_shapes_and_duplicates() {
        let base = || layout_attributes("RESULT", "numeric_display", "9(3)", 3, 0, false, 3);
        let mut invalid = Vec::new();
        let mut missing = base();
        missing.remove("picture");
        invalid.push(missing);
        let mut wrong_type = base();
        wrong_type.insert("digits".into(), Attribute::Text("not-an-integer".into()));
        invalid.push(wrong_type);
        let mut negative_scale = base();
        negative_scale.insert("scale".into(), Attribute::Integer(-1));
        invalid.push(negative_scale);
        let mut noncanonical_boolean = base();
        noncanonical_boolean.insert("signed".into(), Attribute::Integer(2));
        invalid.push(noncanonical_boolean);
        let mut wrong_section = base();
        wrong_section.insert("section".into(), Attribute::Text("elsewhere".into()));
        invalid.push(wrong_section);
        let mut wrong_occurrence_extent = base();
        wrong_occurrence_extent.insert("occurs".into(), Attribute::Integer(2));
        invalid.push(wrong_occurrence_extent);
        let mut wrong_picture_shape = base();
        wrong_picture_shape.insert("picture".into(), Attribute::Text("9(2)".into()));
        invalid.push(wrong_picture_shape);
        let mut static_dynamic_limit = base();
        static_dynamic_limit.insert("dynamic_limit".into(), Attribute::Integer(3));
        invalid.push(static_dynamic_limit);
        let mut malformed_keys = base();
        malformed_keys.insert("keys".into(), Attribute::Text("BROKEN".into()));
        invalid.push(malformed_keys);
        let mut packed_too_wide =
            layout_attributes("PACKED", "packed_decimal", "9(32)", 32, 0, false, 17);
        packed_too_wide.insert("occurs_clause".into(), Attribute::Integer(0));
        invalid.push(packed_too_wide);
        let mut wrong_national_width =
            layout_attributes("NATIONAL-EDITED", "national_edited", "ZZ9", 3, 0, false, 3);
        wrong_national_width.insert("occurs_clause".into(), Attribute::Integer(0));
        invalid.push(wrong_national_width);
        let mut wrong_blank_category = base();
        wrong_blank_category.insert("blank_when_zero".into(), Attribute::Integer(1));
        invalid.push(wrong_blank_category);
        invalid.push(layout_attributes(
            "TEXT-AS-NUMERIC",
            "alphanumeric",
            "9",
            0,
            0,
            false,
            1,
        ));
        invalid.push(layout_attributes(
            "GROUP-WITH-PICTURE",
            "group",
            "X",
            0,
            0,
            false,
            1,
        ));
        invalid.push(layout_attributes(
            "SHORT-POINTER",
            "pointer",
            "",
            0,
            0,
            false,
            3,
        ));
        let mut utf8_mismatch = layout_attributes("UTF8", "utf8", "U", 0, 0, false, 4);
        utf8_mismatch.insert("byte_length".into(), Attribute::Integer(3));
        invalid.push(utf8_mismatch);
        let mut foreign_object_class =
            layout_attributes("TEXT", "alphanumeric", "X", 0, 0, false, 1);
        foreign_object_class.insert("object_class".into(), Attribute::Text("CUSTOMER".into()));
        invalid.push(foreign_object_class);
        let mut picture_over_limit = layout_attributes(
            "OVER-LIMIT",
            "numeric_display",
            "9(1000001)",
            1_000_001,
            0,
            false,
            1_000_001,
        );
        picture_over_limit.insert("occurs_clause".into(), Attribute::Integer(0));
        invalid.push(picture_over_limit);
        let mut aggregate_picture_over_limit = layout_attributes(
            "AGGREGATE-OVER-LIMIT",
            "numeric_display",
            "9(600000)9(600000)",
            1_200_000,
            0,
            false,
            1_200_000,
        );
        aggregate_picture_over_limit.insert("occurs_clause".into(), Attribute::Integer(0));
        invalid.push(aggregate_picture_over_limit);
        let mut huge_picture = base();
        huge_picture.insert(
            "picture".into(),
            Attribute::Text("9(18446744073709551615)".into()),
        );
        invalid.push(huge_picture);
        let mut wrong_simple_name = base();
        wrong_simple_name.insert("simple_name".into(), Attribute::Text("OTHER".into()));
        invalid.push(wrong_simple_name);
        let mut missing_parent = base();
        missing_parent.insert("name".into(), Attribute::Text("ROOT.RESULT".into()));
        missing_parent.insert("parent".into(), Attribute::Text("ROOT".into()));
        invalid.push(missing_parent);
        let mut hidden_occurs = base();
        hidden_occurs.insert("length".into(), Attribute::Integer(6));
        hidden_occurs.insert("occurs".into(), Attribute::Integer(2));
        hidden_occurs.insert("occurs_clause".into(), Attribute::Integer(0));
        invalid.push(hidden_occurs);
        let mut omitted_occurs_marker = base();
        omitted_occurs_marker.insert("length".into(), Attribute::Integer(6));
        omitted_occurs_marker.insert("occurs".into(), Attribute::Integer(2));
        omitted_occurs_marker.remove("occurs_clause");
        invalid.push(omitted_occurs_marker);
        let mut unbounded_without_dependency =
            layout_attributes("UNBOUNDED", "group", "", 0, 0, false, 16 * 1_024 * 1_024);
        unbounded_without_dependency.insert("element_length".into(), Attribute::Integer(4_096));
        unbounded_without_dependency.insert("occurs".into(), Attribute::Integer(4_096));
        unbounded_without_dependency.insert("occurs_clause".into(), Attribute::Integer(1));
        unbounded_without_dependency.insert("unbounded".into(), Attribute::Integer(1));
        invalid.push(unbounded_without_dependency);
        let mut unbounded_wrong_capacity =
            layout_attributes("UNBOUNDED", "group", "", 0, 0, false, 4_095 * 4_096);
        unbounded_wrong_capacity.insert("element_length".into(), Attribute::Integer(4_096));
        unbounded_wrong_capacity.insert("occurs".into(), Attribute::Integer(4_095));
        unbounded_wrong_capacity.insert("occurs_clause".into(), Attribute::Integer(1));
        unbounded_wrong_capacity.insert("unbounded".into(), Attribute::Integer(1));
        unbounded_wrong_capacity.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        invalid.push(unbounded_wrong_capacity);
        let mut dynamic_table_without_marker =
            layout_attributes("DYNAMIC-T", "alphanumeric", "X", 0, 0, false, 0);
        dynamic_table_without_marker.insert("element_length".into(), Attribute::Integer(1));
        dynamic_table_without_marker.insert("indexes".into(), Attribute::Text("IX".into()));
        dynamic_table_without_marker.insert("dynamic".into(), Attribute::Integer(1));
        dynamic_table_without_marker.insert("dynamic_limit".into(), Attribute::Integer(100));
        invalid.push(dynamic_table_without_marker);
        let mut dynamic_table = layout_attributes("DYNAMIC-T", "alphanumeric", "X", 0, 0, false, 0);
        dynamic_table.insert("element_length".into(), Attribute::Integer(1));
        dynamic_table.insert("occurs".into(), Attribute::Integer(2));
        dynamic_table.insert("occurs_min".into(), Attribute::Integer(2));
        dynamic_table.insert("occurs_clause".into(), Attribute::Integer(1));
        dynamic_table.insert("indexes".into(), Attribute::Text("DYNAMIC-IX".into()));
        dynamic_table.insert("dynamic".into(), Attribute::Integer(1));
        dynamic_table.insert("dynamic_limit".into(), Attribute::Integer(100));
        invalid.push(dynamic_table);
        let mut dynamic_over_limit =
            layout_attributes("DYNAMIC", "alphanumeric", "X", 0, 0, false, 0);
        dynamic_over_limit.insert("element_length".into(), Attribute::Integer(1));
        dynamic_over_limit.insert("dynamic".into(), Attribute::Integer(1));
        dynamic_over_limit.insert("dynamic_limit".into(), Attribute::Integer(1_000_000_000));
        invalid.push(dynamic_over_limit);
        let mut invalid_index_name = base();
        invalid_index_name.insert("occurs_clause".into(), Attribute::Integer(1));
        invalid_index_name.insert("indexes".into(), Attribute::Text("100".into()));
        invalid.push(invalid_index_name);
        let mut overlong_index_name = base();
        overlong_index_name.insert("occurs_clause".into(), Attribute::Integer(1));
        overlong_index_name.insert("indexes".into(), Attribute::Text("I".repeat(31)));
        invalid.push(overlong_index_name);
        let mut too_many_indexes = base();
        too_many_indexes.insert("occurs_clause".into(), Attribute::Integer(1));
        too_many_indexes.insert(
            "indexes".into(),
            Attribute::Text(
                (0..=crate::COBOL_MAX_INDEX_NAMES)
                    .map(|index| format!("IX-{index}"))
                    .collect::<Vec<_>>()
                    .join("\u{1f}"),
            ),
        );
        invalid.push(too_many_indexes);
        let mut too_many_keys = base();
        too_many_keys.insert("occurs_clause".into(), Attribute::Integer(1));
        too_many_keys.insert(
            "keys".into(),
            Attribute::Text(
                (0..=crate::COBOL_MAX_TABLE_KEYS)
                    .map(|index| format!("A:KEY-{index}"))
                    .collect::<Vec<_>>()
                    .join("\u{1f}"),
            ),
        );
        invalid.push(too_many_keys);
        let mut raw_filler = base();
        raw_filler.insert("name".into(), Attribute::Text("FILLER".into()));
        raw_filler.insert("simple_name".into(), Attribute::Text("FILLER".into()));
        invalid.push(raw_filler);
        let mut short_filler_suffix = base();
        short_filler_suffix.insert("name".into(), Attribute::Text("FILLER#1".into()));
        short_filler_suffix.insert("simple_name".into(), Attribute::Text("FILLER".into()));
        invalid.push(short_filler_suffix);
        let define = crate::cobol_layout_definition_identity();
        for attributes in invalid {
            assert!(matches!(
                verify_layout_definitions(vec![attributes]),
                Err(crate::VerificationProblem::SemanticMismatch(identity)) if identity == define
            ));
        }
        assert!(matches!(
            verify_layout_definitions(vec![base(), base()]),
            Err(crate::VerificationProblem::SemanticMismatch(identity)) if identity == define
        ));
        let mut alternate = base();
        alternate.insert(
            "name".into(),
            Attribute::Text("RESULT#ALTERNATE00002".into()),
        );
        assert!(matches!(
            verify_layout_definitions(vec![alternate]),
            Err(crate::VerificationProblem::SemanticMismatch(identity)) if identity == define
        ));
        let mut missing_alias = base();
        missing_alias.insert("alias_of".into(), Attribute::Text("MISSING".into()));
        assert!(matches!(
            verify_layout_definitions(vec![missing_alias]),
            Err(crate::VerificationProblem::SemanticMismatch(identity)) if identity == define
        ));

        let primary = base();
        let mut independent_alias = base();
        independent_alias.insert("name".into(), Attribute::Text("VIEW".into()));
        independent_alias.insert("simple_name".into(), Attribute::Text("VIEW".into()));
        independent_alias.insert("alias_of".into(), Attribute::Text("RESULT".into()));
        assert!(
            verify_layout_definitions_with_storage_mode(
                vec![primary, independent_alias],
                true,
                false,
                None,
            )
            .is_err()
        );
        let pointer64 = layout_attributes("PTR", "pointer", "", 0, 0, false, 8);
        assert!(
            verify_layout_definitions_with_storage_mode(vec![pointer64], true, true, Some("32"),)
                .is_err()
        );
    }

    #[test]
    fn cobol_layout_relationships_resolve_odo_keys_dynamic_and_aliases() {
        let count = layout_attributes("COUNT", "numeric_display", "9", 1, 0, false, 1);
        let mut table = layout_attributes("TABLE", "group", "", 0, 0, false, 2);
        table.insert("element_length".into(), Attribute::Integer(1));
        table.insert("occurs".into(), Attribute::Integer(2));
        table.insert("occurs_min".into(), Attribute::Integer(1));
        table.insert("occurs_clause".into(), Attribute::Integer(1));
        table.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        table.insert("keys".into(), Attribute::Text("A:KEY".into()));
        let mut key = layout_attributes("TABLE.KEY", "alphanumeric", "X", 0, 0, false, 1);
        key.insert("simple_name".into(), Attribute::Text("KEY".into()));
        key.insert("parent".into(), Attribute::Text("TABLE".into()));
        let filler = {
            let mut attributes =
                layout_attributes("FILLER#00001", "alphanumeric", "X", 0, 0, false, 1);
            attributes.insert("simple_name".into(), Attribute::Text("FILLER".into()));
            attributes
        };
        let primary = layout_attributes("RESULT", "numeric_display", "9", 1, 0, false, 1);
        let alternate = {
            let mut attributes = layout_attributes(
                "RESULT#ALTERNATE00005",
                "numeric_display",
                "9",
                1,
                0,
                false,
                1,
            );
            attributes.insert("simple_name".into(), Attribute::Text("RESULT".into()));
            attributes.insert("alias_of".into(), Attribute::Text("RESULT".into()));
            attributes
        };
        verify_layout_definitions(vec![count, table, key, filler, primary, alternate])
            .expect("canonical ODO, key, filler, and alternate relationships should verify");

        let count = layout_attributes("COUNT", "numeric_display", "9", 1, 0, false, 1);
        let mut table = layout_attributes("TABLE", "group", "", 0, 0, false, 2);
        table.insert("element_length".into(), Attribute::Integer(1));
        table.insert("occurs".into(), Attribute::Integer(2));
        table.insert("occurs_min".into(), Attribute::Integer(1));
        table.insert("occurs_clause".into(), Attribute::Integer(1));
        table.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        assert!(verify_layout_definitions_with_storage(vec![count, table], false).is_err());

        let flag = layout_attributes("FLAG", "alphanumeric", "X", 0, 0, false, 1);
        let mut condition = layout_attributes("FLAG.YES", "condition", "", 0, 0, false, 0);
        condition.insert("simple_name".into(), Attribute::Text("YES".into()));
        condition.insert("parent".into(), Attribute::Text("FLAG".into()));
        condition.insert("alias_of".into(), Attribute::Text("FLAG".into()));
        condition.insert("condition_values".into(), Attribute::Text("'Y'".into()));
        let mut dynamic = layout_attributes("DYNAMIC", "alphanumeric", "X", 0, 0, false, 0);
        dynamic.insert("element_length".into(), Attribute::Integer(1));
        dynamic.insert("dynamic".into(), Attribute::Integer(1));
        dynamic.insert("dynamic_limit".into(), Attribute::Integer(100));
        let mut dynamic_condition =
            layout_attributes("DYNAMIC.EMPTY", "condition", "", 0, 0, false, 0);
        dynamic_condition.insert("simple_name".into(), Attribute::Text("EMPTY".into()));
        dynamic_condition.insert("parent".into(), Attribute::Text("DYNAMIC".into()));
        dynamic_condition.insert("alias_of".into(), Attribute::Text("DYNAMIC".into()));
        dynamic_condition.insert("condition_values".into(), Attribute::Text("SPACE".into()));
        verify_layout_definitions(vec![flag, condition, dynamic, dynamic_condition])
            .expect("condition associations are not REDEFINES aliases");

        let number = layout_attributes("NUMBER", "numeric_display", "9", 1, 0, false, 1);
        let mut numeric_condition =
            layout_attributes("NUMBER.VALID", "condition", "", 0, 0, false, 0);
        numeric_condition.insert("simple_name".into(), Attribute::Text("VALID".into()));
        numeric_condition.insert("parent".into(), Attribute::Text("NUMBER".into()));
        numeric_condition.insert("alias_of".into(), Attribute::Text("NUMBER".into()));
        numeric_condition.insert("condition_values".into(), Attribute::Text("1".into()));
        let mut constant = layout_attributes("CONSTANT", "condition", "", 0, 0, false, 0);
        constant.insert("condition_values".into(), Attribute::Text("1".into()));
        verify_layout_definitions(vec![number, numeric_condition, constant])
            .expect("numeric conditions and level-78 constants have distinct valid shapes");

        let number = layout_attributes("NUMBER", "numeric_display", "9", 1, 0, false, 1);
        let mut wrong_class = layout_attributes("NUMBER.BAD", "condition", "", 0, 0, false, 0);
        wrong_class.insert("simple_name".into(), Attribute::Text("BAD".into()));
        wrong_class.insert("parent".into(), Attribute::Text("NUMBER".into()));
        wrong_class.insert("alias_of".into(), Attribute::Text("NUMBER".into()));
        wrong_class.insert("condition_values".into(), Attribute::Text("'ABC'".into()));
        assert!(verify_layout_definitions(vec![number, wrong_class]).is_err());

        let root = layout_attributes("ROOT", "group", "", 0, 0, false, 2);
        let mut group = layout_attributes("ROOT.G", "group", "", 0, 0, false, 2);
        group.insert("simple_name".into(), Attribute::Text("G".into()));
        group.insert("parent".into(), Attribute::Text("ROOT".into()));
        let mut first = layout_attributes("ROOT.G.A", "alphanumeric", "X", 0, 0, false, 1);
        first.insert("simple_name".into(), Attribute::Text("A".into()));
        first.insert("parent".into(), Attribute::Text("ROOT.G".into()));
        let mut second = layout_attributes("ROOT.G.B", "alphanumeric", "X", 0, 0, false, 1);
        second.insert("simple_name".into(), Attribute::Text("B".into()));
        second.insert("parent".into(), Attribute::Text("ROOT.G".into()));
        second.insert("offset".into(), Attribute::Integer(1));
        let mut rename = layout_attributes("ROOT.AB", "rename", "", 0, 0, false, 2);
        rename.insert("simple_name".into(), Attribute::Text("AB".into()));
        rename.insert("parent".into(), Attribute::Text("ROOT".into()));
        rename.insert("alias_of".into(), Attribute::Text("ROOT.G.A".into()));
        rename.insert("rename_through".into(), Attribute::Text("ROOT.G.B".into()));
        verify_layout_definitions(vec![root, group, first, second, rename])
            .expect("a nested RENAMES range start is not a sibling REDEFINES target");

        let root = layout_attributes("ROOT", "group", "", 0, 0, false, 1);
        let mut item = layout_attributes("ROOT.A", "alphanumeric", "X", 0, 0, false, 1);
        item.insert("simple_name".into(), Attribute::Text("A".into()));
        item.insert("parent".into(), Attribute::Text("ROOT".into()));
        let mut same_endpoint = layout_attributes("ROOT.R", "rename", "", 0, 0, false, 1);
        same_endpoint.insert("simple_name".into(), Attribute::Text("R".into()));
        same_endpoint.insert("parent".into(), Attribute::Text("ROOT".into()));
        same_endpoint.insert("alias_of".into(), Attribute::Text("ROOT.A".into()));
        same_endpoint.insert("rename_through".into(), Attribute::Text("ROOT.A".into()));
        assert!(verify_layout_definitions(vec![root, item, same_endpoint]).is_err());

        let count = layout_attributes("COUNT", "numeric_display", "9", 1, 0, false, 1);
        let root = layout_attributes("ROOT", "group", "", 0, 0, false, 4);
        let mut first = layout_attributes("ROOT.A", "alphanumeric", "X", 0, 0, false, 1);
        first.insert("simple_name".into(), Attribute::Text("A".into()));
        first.insert("parent".into(), Attribute::Text("ROOT".into()));
        let mut odo = layout_attributes("ROOT.T", "alphanumeric", "X", 0, 0, false, 2);
        odo.insert("simple_name".into(), Attribute::Text("T".into()));
        odo.insert("parent".into(), Attribute::Text("ROOT".into()));
        odo.insert("offset".into(), Attribute::Integer(1));
        odo.insert("element_length".into(), Attribute::Integer(1));
        odo.insert("occurs".into(), Attribute::Integer(2));
        odo.insert("occurs_min".into(), Attribute::Integer(1));
        odo.insert("occurs_clause".into(), Attribute::Integer(1));
        odo.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        let mut end = layout_attributes("ROOT.B", "alphanumeric", "X", 0, 0, false, 1);
        end.insert("simple_name".into(), Attribute::Text("B".into()));
        end.insert("parent".into(), Attribute::Text("ROOT".into()));
        end.insert("offset".into(), Attribute::Integer(3));
        let mut rename = layout_attributes("ROOT.R", "rename", "", 0, 0, false, 4);
        rename.insert("simple_name".into(), Attribute::Text("R".into()));
        rename.insert("parent".into(), Attribute::Text("ROOT".into()));
        rename.insert("alias_of".into(), Attribute::Text("ROOT.A".into()));
        rename.insert("rename_through".into(), Attribute::Text("ROOT.B".into()));
        assert!(verify_layout_definitions(vec![count, root, first, odo, end, rename]).is_err());

        let flag = layout_attributes("FLAG", "alphanumeric", "X", 0, 0, false, 1);
        let other = layout_attributes("OTHER", "alphanumeric", "X", 0, 0, false, 1);
        let mut malformed_condition = layout_attributes("FLAG.NO", "condition", "", 0, 0, false, 0);
        malformed_condition.insert("simple_name".into(), Attribute::Text("NO".into()));
        malformed_condition.insert("parent".into(), Attribute::Text("FLAG".into()));
        malformed_condition.insert("alias_of".into(), Attribute::Text("OTHER".into()));
        malformed_condition.insert("condition_values".into(), Attribute::Text("N".into()));
        assert!(verify_layout_definitions(vec![flag, other, malformed_condition]).is_err());

        let root = layout_attributes("ROOT", "group", "", 0, 0, false, 1);
        let other = layout_attributes("OTHER", "alphanumeric", "X", 0, 0, false, 1);
        let mut outside_rename = layout_attributes("ROOT.BAD", "rename", "", 0, 0, false, 1);
        outside_rename.insert("simple_name".into(), Attribute::Text("BAD".into()));
        outside_rename.insert("parent".into(), Attribute::Text("ROOT".into()));
        outside_rename.insert("alias_of".into(), Attribute::Text("OTHER".into()));
        assert!(verify_layout_definitions(vec![root, other, outside_rename]).is_err());

        let mut missing_dependency = layout_attributes("TABLE", "group", "", 0, 0, false, 2);
        missing_dependency.insert("element_length".into(), Attribute::Integer(1));
        missing_dependency.insert("occurs".into(), Attribute::Integer(2));
        missing_dependency.insert("occurs_min".into(), Attribute::Integer(1));
        missing_dependency.insert("occurs_clause".into(), Attribute::Integer(1));
        missing_dependency.insert("depending_on".into(), Attribute::Text("MISSING".into()));
        assert!(verify_layout_definitions(vec![missing_dependency]).is_err());

        let text = layout_attributes("TEXT", "alphanumeric", "X", 0, 0, false, 1);
        let mut ineligible_dependency = layout_attributes("TABLE", "group", "", 0, 0, false, 2);
        ineligible_dependency.insert("element_length".into(), Attribute::Integer(1));
        ineligible_dependency.insert("occurs".into(), Attribute::Integer(2));
        ineligible_dependency.insert("occurs_min".into(), Attribute::Integer(1));
        ineligible_dependency.insert("occurs_clause".into(), Attribute::Integer(1));
        ineligible_dependency.insert("depending_on".into(), Attribute::Text("TEXT".into()));
        assert!(verify_layout_definitions(vec![text, ineligible_dependency]).is_err());

        let mut missing_key = layout_attributes("TABLE", "group", "", 0, 0, false, 1);
        missing_key.insert("occurs_clause".into(), Attribute::Integer(1));
        missing_key.insert("keys".into(), Attribute::Text("A:MISSING".into()));
        assert!(verify_layout_definitions(vec![missing_key]).is_err());

        let mut repeated_subject_key = layout_attributes("TABLE", "group", "", 0, 0, false, 1);
        repeated_subject_key.insert("occurs_clause".into(), Attribute::Integer(1));
        repeated_subject_key.insert(
            "keys".into(),
            Attribute::Text("A:TABLE\u{1f}D:TABLE".into()),
        );
        assert!(verify_layout_definitions(vec![repeated_subject_key]).is_err());

        let mut dynamic = layout_attributes("DYNAMIC", "alphanumeric", "X", 0, 0, false, 0);
        dynamic.insert("element_length".into(), Attribute::Integer(1));
        dynamic.insert("dynamic".into(), Attribute::Integer(1));
        dynamic.insert("dynamic_limit".into(), Attribute::Integer(100));
        let mut alias = layout_attributes("VIEW", "alphanumeric", "X", 0, 0, false, 1);
        alias.insert("alias_of".into(), Attribute::Text("DYNAMIC".into()));
        assert!(verify_layout_definitions(vec![dynamic, alias]).is_err());

        let alias_group = |name: &str, alias_of: &str| {
            let mut attributes = layout_attributes(name, "group", "", 0, 0, false, 1);
            if !alias_of.is_empty() {
                attributes.insert("alias_of".into(), Attribute::Text(alias_of.into()));
            }
            attributes
        };
        let dynamic_child = |name: &str, parent: &str| {
            let mut attributes = layout_attributes(name, "alphanumeric", "X", 0, 0, false, 0);
            attributes.insert("simple_name".into(), Attribute::Text("DYN".into()));
            attributes.insert("parent".into(), Attribute::Text(parent.into()));
            attributes.insert("element_length".into(), Attribute::Integer(1));
            attributes.insert("dynamic".into(), Attribute::Integer(1));
            attributes.insert("dynamic_limit".into(), Attribute::Integer(100));
            attributes
        };
        assert!(
            verify_layout_definitions(vec![
                alias_group("BASE", ""),
                dynamic_child("BASE.DYN", "BASE"),
                alias_group("VIEW", "BASE"),
            ])
            .is_err()
        );
        assert!(
            verify_layout_definitions(vec![
                alias_group("BASE", ""),
                alias_group("VIEW", "BASE"),
                dynamic_child("VIEW.DYN", "VIEW"),
            ])
            .is_err()
        );

        let mut table_parent = layout_attributes("ROOT", "group", "", 0, 0, false, 2);
        table_parent.insert("element_length".into(), Attribute::Integer(1));
        table_parent.insert("occurs".into(), Attribute::Integer(2));
        table_parent.insert("occurs_min".into(), Attribute::Integer(2));
        table_parent.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut dynamic_child =
            layout_attributes("ROOT.DYNAMIC", "alphanumeric", "X", 0, 0, false, 0);
        dynamic_child.insert("simple_name".into(), Attribute::Text("DYNAMIC".into()));
        dynamic_child.insert("parent".into(), Attribute::Text("ROOT".into()));
        dynamic_child.insert("element_length".into(), Attribute::Integer(1));
        dynamic_child.insert("dynamic".into(), Attribute::Integer(1));
        dynamic_child.insert("dynamic_limit".into(), Attribute::Integer(100));
        assert!(verify_layout_definitions(vec![table_parent, dynamic_child]).is_err());

        let count = layout_attributes("COUNT", "numeric_display", "9", 1, 0, false, 1);
        let mut wide_table = layout_attributes("WIDE-TABLE", "group", "", 0, 0, false, 514);
        wide_table.insert("element_length".into(), Attribute::Integer(257));
        wide_table.insert("occurs".into(), Attribute::Integer(2));
        wide_table.insert("occurs_min".into(), Attribute::Integer(1));
        wide_table.insert("occurs_clause".into(), Attribute::Integer(1));
        wide_table.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        wide_table.insert("keys".into(), Attribute::Text("A:WIDE-KEY".into()));
        let mut wide_key =
            layout_attributes("WIDE-TABLE.WIDE-KEY", "alphanumeric", "X", 0, 0, false, 257);
        wide_key.insert("simple_name".into(), Attribute::Text("WIDE-KEY".into()));
        wide_key.insert("parent".into(), Attribute::Text("WIDE-TABLE".into()));
        assert!(verify_layout_definitions(vec![count, wide_table, wide_key]).is_err());

        let mut root = layout_attributes("ROOT", "group", "", 0, 0, false, 3);
        let mut group = layout_attributes("ROOT.GROUP", "group", "", 0, 0, false, 1);
        group.insert("simple_name".into(), Attribute::Text("GROUP".into()));
        group.insert("parent".into(), Attribute::Text("ROOT".into()));
        let mut qualified_count =
            layout_attributes("ROOT.GROUP.COUNT", "numeric_display", "9", 1, 0, false, 1);
        qualified_count.insert("simple_name".into(), Attribute::Text("COUNT".into()));
        qualified_count.insert("parent".into(), Attribute::Text("ROOT.GROUP".into()));
        let mut qualified_table = layout_attributes("ROOT.TABLE", "group", "", 0, 0, false, 2);
        qualified_table.insert("simple_name".into(), Attribute::Text("TABLE".into()));
        qualified_table.insert("parent".into(), Attribute::Text("ROOT".into()));
        qualified_table.insert("element_length".into(), Attribute::Integer(1));
        qualified_table.insert("occurs".into(), Attribute::Integer(2));
        qualified_table.insert("occurs_min".into(), Attribute::Integer(1));
        qualified_table.insert("occurs_clause".into(), Attribute::Integer(1));
        qualified_table.insert("offset".into(), Attribute::Integer(1));
        qualified_table.insert(
            "depending_on".into(),
            Attribute::Text("COUNT OF GROUP".into()),
        );
        qualified_table.insert("keys".into(), Attribute::Text("A:KEY OF KEY-G".into()));
        let mut key_group = layout_attributes("ROOT.TABLE.KEY-G", "group", "", 0, 0, false, 1);
        key_group.insert("simple_name".into(), Attribute::Text("KEY-G".into()));
        key_group.insert("parent".into(), Attribute::Text("ROOT.TABLE".into()));
        key_group.insert("offset".into(), Attribute::Integer(1));
        let mut qualified_key =
            layout_attributes("ROOT.TABLE.KEY-G.KEY", "alphanumeric", "X", 0, 0, false, 1);
        qualified_key.insert("simple_name".into(), Attribute::Text("KEY".into()));
        qualified_key.insert("parent".into(), Attribute::Text("ROOT.TABLE.KEY-G".into()));
        qualified_key.insert("offset".into(), Attribute::Integer(1));
        verify_layout_definitions(vec![
            root.clone(),
            group,
            qualified_count,
            qualified_table,
            key_group,
            qualified_key,
        ])
        .expect("partial qualifiers should resolve through the relative hierarchy");

        let mut pointer_table = layout_attributes("TABLE", "group", "", 0, 0, false, 1);
        pointer_table.insert("occurs_clause".into(), Attribute::Integer(1));
        pointer_table.insert("keys".into(), Attribute::Text("A:PTR".into()));
        let mut pointer_key = layout_attributes("TABLE.PTR", "pointer", "", 0, 0, false, 8);
        pointer_key.insert("simple_name".into(), Attribute::Text("PTR".into()));
        pointer_key.insert("parent".into(), Attribute::Text("TABLE".into()));
        assert!(verify_layout_definitions(vec![pointer_table, pointer_key]).is_err());

        let mut ordered = layout_attributes("ORDERED", "group", "", 0, 0, false, 2);
        ordered.insert("element_length".into(), Attribute::Integer(1));
        ordered.insert("occurs".into(), Attribute::Integer(2));
        ordered.insert("occurs_min".into(), Attribute::Integer(2));
        ordered.insert("occurs_clause".into(), Attribute::Integer(1));
        ordered.insert("keys".into(), Attribute::Text("A:KEY".into()));
        let mut nested = layout_attributes("ORDERED.NEST", "group", "", 0, 0, false, 2);
        nested.insert("simple_name".into(), Attribute::Text("NEST".into()));
        nested.insert("parent".into(), Attribute::Text("ORDERED".into()));
        nested.insert("element_length".into(), Attribute::Integer(1));
        nested.insert("occurs".into(), Attribute::Integer(2));
        nested.insert("occurs_min".into(), Attribute::Integer(2));
        nested.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut following_key =
            layout_attributes("ORDERED.KEY", "alphanumeric", "X", 0, 0, false, 1);
        following_key.insert("simple_name".into(), Attribute::Text("KEY".into()));
        following_key.insert("parent".into(), Attribute::Text("ORDERED".into()));
        verify_layout_definitions(vec![ordered.clone(), following_key.clone(), nested.clone()])
            .expect("a key before a nested fixed table remains eligible");
        assert!(verify_layout_definitions(vec![ordered, nested, following_key]).is_err());

        let count = layout_attributes("COUNT", "numeric_display", "9", 1, 0, false, 1);
        let mut subject_key = layout_attributes("TABLE", "group", "", 0, 0, false, 1);
        subject_key.insert("occurs_clause".into(), Attribute::Integer(1));
        subject_key.insert("keys".into(), Attribute::Text("A:TABLE".into()));
        let mut nested_odo = layout_attributes("TABLE.SUB", "alphanumeric", "X", 0, 0, false, 2);
        nested_odo.insert("simple_name".into(), Attribute::Text("SUB".into()));
        nested_odo.insert("parent".into(), Attribute::Text("TABLE".into()));
        nested_odo.insert("element_length".into(), Attribute::Integer(1));
        nested_odo.insert("occurs".into(), Attribute::Integer(2));
        nested_odo.insert("occurs_min".into(), Attribute::Integer(1));
        nested_odo.insert("occurs_clause".into(), Attribute::Integer(1));
        nested_odo.insert("depending_on".into(), Attribute::Text("COUNT".into()));
        assert!(verify_layout_definitions(vec![count, subject_key, nested_odo]).is_err());

        let mut ordered_table = layout_attributes("TABLE", "group", "", 0, 0, false, 1);
        ordered_table.insert("occurs_clause".into(), Attribute::Integer(1));
        ordered_table.insert("keys".into(), Attribute::Text("A:KEY".into()));
        let mut preceding_table =
            layout_attributes("TABLE.NEST", "alphanumeric", "X", 0, 0, false, 2);
        preceding_table.insert("simple_name".into(), Attribute::Text("NEST".into()));
        preceding_table.insert("parent".into(), Attribute::Text("TABLE".into()));
        preceding_table.insert("element_length".into(), Attribute::Integer(1));
        preceding_table.insert("occurs".into(), Attribute::Integer(2));
        preceding_table.insert("occurs_min".into(), Attribute::Integer(2));
        preceding_table.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut following_key = layout_attributes("TABLE.KEY", "alphanumeric", "X", 0, 0, false, 1);
        following_key.insert("simple_name".into(), Attribute::Text("KEY".into()));
        following_key.insert("parent".into(), Attribute::Text("TABLE".into()));
        assert!(
            verify_layout_definitions(vec![ordered_table, preceding_table, following_key]).is_err()
        );

        let mut occurs_target = layout_attributes("BASE", "alphanumeric", "X", 0, 0, false, 1);
        occurs_target.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut alias = layout_attributes("VIEW", "alphanumeric", "X", 0, 0, false, 1);
        alias.insert("alias_of".into(), Attribute::Text("BASE".into()));
        assert!(verify_layout_definitions(vec![occurs_target, alias]).is_err());

        root.insert("length".into(), Attribute::Integer(1));
        let mut base = layout_attributes("ROOT.BASE", "alphanumeric", "X", 0, 0, false, 1);
        base.insert("simple_name".into(), Attribute::Text("BASE".into()));
        base.insert("parent".into(), Attribute::Text("ROOT".into()));
        let mut wrong_level_alias = layout_attributes("VIEW", "alphanumeric", "X", 0, 0, false, 1);
        wrong_level_alias.insert("alias_of".into(), Attribute::Text("ROOT.BASE".into()));
        assert!(verify_layout_definitions(vec![root, base, wrong_level_alias]).is_err());

        let mut counts = layout_attributes("COUNTS", "group", "", 0, 0, false, 2);
        counts.insert("element_length".into(), Attribute::Integer(1));
        counts.insert("occurs".into(), Attribute::Integer(2));
        counts.insert("occurs_min".into(), Attribute::Integer(2));
        counts.insert("occurs_clause".into(), Attribute::Integer(1));
        let mut nested_count =
            layout_attributes("COUNTS.N", "numeric_display", "9", 1, 0, false, 1);
        nested_count.insert("simple_name".into(), Attribute::Text("N".into()));
        nested_count.insert("parent".into(), Attribute::Text("COUNTS".into()));
        let mut dependent = layout_attributes("TABLE", "group", "", 0, 0, false, 2);
        dependent.insert("element_length".into(), Attribute::Integer(1));
        dependent.insert("occurs".into(), Attribute::Integer(2));
        dependent.insert("occurs_min".into(), Attribute::Integer(1));
        dependent.insert("occurs_clause".into(), Attribute::Integer(1));
        dependent.insert("depending_on".into(), Attribute::Text("N".into()));
        assert!(verify_layout_definitions(vec![counts, nested_count, dependent]).is_err());

        let record = layout_attributes("RECORD", "group", "", 0, 0, false, 1);
        let mut first_count =
            layout_attributes("RECORD.FIRST", "numeric_display", "9", 1, 0, false, 1);
        first_count.insert("simple_name".into(), Attribute::Text("FIRST".into()));
        first_count.insert("parent".into(), Attribute::Text("RECORD".into()));
        let mut first_table =
            layout_attributes("RECORD.TABLE-A", "alphanumeric", "X", 0, 0, false, 2);
        first_table.insert("simple_name".into(), Attribute::Text("TABLE-A".into()));
        first_table.insert("parent".into(), Attribute::Text("RECORD".into()));
        first_table.insert("element_length".into(), Attribute::Integer(1));
        first_table.insert("occurs".into(), Attribute::Integer(2));
        first_table.insert("occurs_min".into(), Attribute::Integer(1));
        first_table.insert("occurs_clause".into(), Attribute::Integer(1));
        first_table.insert("depending_on".into(), Attribute::Text("FIRST".into()));
        let mut late_count =
            layout_attributes("RECORD.LATE", "numeric_display", "9", 1, 0, false, 1);
        late_count.insert("simple_name".into(), Attribute::Text("LATE".into()));
        late_count.insert("parent".into(), Attribute::Text("RECORD".into()));
        let mut second_table =
            layout_attributes("RECORD.TABLE-B", "alphanumeric", "X", 0, 0, false, 2);
        second_table.insert("simple_name".into(), Attribute::Text("TABLE-B".into()));
        second_table.insert("parent".into(), Attribute::Text("RECORD".into()));
        second_table.insert("element_length".into(), Attribute::Integer(1));
        second_table.insert("occurs".into(), Attribute::Integer(2));
        second_table.insert("occurs_min".into(), Attribute::Integer(1));
        second_table.insert("occurs_clause".into(), Attribute::Integer(1));
        second_table.insert("depending_on".into(), Attribute::Text("LATE".into()));
        assert!(
            verify_layout_definitions(vec![
                record,
                first_count,
                first_table,
                late_count,
                second_table,
            ])
            .is_err()
        );
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
