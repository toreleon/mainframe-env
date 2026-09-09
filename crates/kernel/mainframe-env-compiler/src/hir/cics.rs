use super::{
    HirCicsConditionPolicy, HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation,
    HirCicsOption, HirCicsOutputBinding, HirCicsOutputName, HirCicsStatement, HirCicsValue,
    HirDataReference, HirProblem, HirResolvedStatement, HirStatement, StatementKind,
};
use mainframe_env_ir::{
    Attribute, BlockId, CicsCondition, CicsEffectPlan, CicsNamedOperand, CicsOperandName,
    CicsOperandValue, CicsOutputBinding, CicsOutputName, CicsPlanLimits, CicsPlanOperation,
    CicsPlanOption, CicsStorageSlot, Effect, ModuleBuilder, OperationCatalog, OperationIdentity,
    OperationSchema, StorageId, StorageReference, encode_cics_effect_plan,
};
use std::collections::BTreeMap;

pub(crate) const CICS_PLAN_ATTRIBUTE: &str = "cics_plan";

pub(crate) struct EncodedCicsEffect {
    pub bytes: Vec<u8>,
    pub storage: Vec<StorageReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CicsPlanProblem {
    MissingStorage(String),
    InvalidStorageExtent(String),
    InvalidPlan,
}

pub(crate) fn executable_identity(operation: HirCicsOperation) -> OperationIdentity {
    let (namespace, name) = match operation {
        HirCicsOperation::Read => ("cics.file", "read"),
        HirCicsOperation::Rewrite => ("cics.file", "rewrite"),
        HirCicsOperation::Syncpoint => ("cics.recovery", "syncpoint"),
    };
    OperationIdentity::new(namespace, name, 1).expect("static typed CICS identity")
}

pub(crate) fn operation_effects(operation: HirCicsOperation) -> Vec<Effect> {
    match operation {
        HirCicsOperation::Read => vec![
            Effect::DatasetRead,
            Effect::MemoryRead,
            Effect::MemoryWrite,
            Effect::Condition,
            Effect::Transaction,
        ],
        HirCicsOperation::Rewrite => vec![
            Effect::DatasetWrite,
            Effect::MemoryRead,
            Effect::MemoryWrite,
            Effect::Condition,
            Effect::Transaction,
        ],
        HirCicsOperation::Syncpoint => {
            vec![Effect::MemoryWrite, Effect::Condition, Effect::Transaction]
        }
    }
}

pub(crate) fn register_hir_operation(catalog: &mut OperationCatalog) {
    let identity = OperationIdentity::new("cobol.hir", StatementKind::ExecCics.slug(), 2)
        .expect("static typed CICS HIR identity");
    let mut schema = OperationSchema::pure(identity, 0, 0);
    schema.required_attributes = [CICS_PLAN_ATTRIBUTE.into()].into_iter().collect();
    schema.allowed_effects = [
        HirCicsOperation::Read,
        HirCicsOperation::Rewrite,
        HirCicsOperation::Syncpoint,
    ]
    .into_iter()
    .flat_map(operation_effects)
    .collect();
    catalog
        .register(schema)
        .expect("unique typed CICS HIR operation");
}

pub(crate) fn register_executable_operations(catalog: &mut OperationCatalog) {
    for operation in [
        HirCicsOperation::Read,
        HirCicsOperation::Rewrite,
        HirCicsOperation::Syncpoint,
    ] {
        let mut schema = OperationSchema::pure(executable_identity(operation), 0, 0);
        schema.required_attributes = [CICS_PLAN_ATTRIBUTE.into()].into_iter().collect();
        schema.allowed_effects = operation_effects(operation).into_iter().collect();
        schema.runtime_import = Some("host.cics".into());
        catalog
            .register(schema)
            .expect("unique typed CICS executable operation");
    }
}

pub(crate) fn emit_hir_operation(
    statement: &HirStatement,
    builder: &mut ModuleBuilder,
    block: BlockId,
    storage_ids: &BTreeMap<String, StorageId>,
) -> Result<bool, HirProblem> {
    let Some(HirResolvedStatement::Cics(command)) = statement.resolved.as_ref() else {
        return Ok(false);
    };
    let encoded = encode_statement(command, storage_ids).map_err(|problem| {
        HirProblem::InvalidResolvedStatement(
            statement.kind,
            statement.line,
            format!("CICS plan construction failed: {problem:?}"),
        )
    })?;
    builder
        .add_operation(
            block,
            OperationIdentity::new("cobol.hir", StatementKind::ExecCics.slug(), 2)
                .map_err(|_| HirProblem::UnknownStatement(statement.line))?,
            Vec::new(),
            0,
            BTreeMap::from([
                ("line".into(), Attribute::Integer(statement.line as i64)),
                (CICS_PLAN_ATTRIBUTE.into(), Attribute::Bytes(encoded.bytes)),
            ]),
            operation_effects(command.operation),
            encoded.storage,
            statement.location.clone(),
        )
        .map_err(|_| HirProblem::StatementLimitExceeded)?;
    Ok(true)
}

pub(crate) fn encode_statement(
    command: &HirCicsStatement,
    storage_ids: &BTreeMap<String, StorageId>,
) -> Result<EncodedCicsEffect, CicsPlanProblem> {
    let mut context = PlanContext {
        storage_ids,
        references: BTreeMap::new(),
    };
    let operands = command
        .operands
        .iter()
        .map(|operand| context.operand(operand))
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = command
        .outputs
        .iter()
        .map(|output| context.output(output))
        .collect::<Result<Vec<_>, _>>()?;
    let condition = match &command.condition_policy {
        HirCicsConditionPolicy::Default => CicsCondition::Default,
        HirCicsConditionPolicy::NoHandle => CicsCondition::NoHandle,
        HirCicsConditionPolicy::Respond {
            response,
            response2,
        } => CicsCondition::Respond {
            response: context.slot(response)?,
            response2: response2
                .as_ref()
                .map(|response| context.slot(response))
                .transpose()?,
        },
    };
    let plan = CicsEffectPlan {
        operation: plan_operation(command.operation),
        operands,
        options: command.options.iter().copied().map(plan_option).collect(),
        outputs,
        condition,
    };
    let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default())
        .map_err(|_| CicsPlanProblem::InvalidPlan)?;
    Ok(EncodedCicsEffect {
        bytes,
        storage: context.references.into_values().collect(),
    })
}

struct PlanContext<'a> {
    storage_ids: &'a BTreeMap<String, StorageId>,
    references: BTreeMap<StorageId, StorageReference>,
}

impl PlanContext<'_> {
    fn operand(
        &mut self,
        operand: &HirCicsNamedOperand,
    ) -> Result<CicsNamedOperand, CicsPlanProblem> {
        Ok(CicsNamedOperand {
            name: match operand.name {
                HirCicsOperandName::File => CicsOperandName::File,
                HirCicsOperandName::Dataset => CicsOperandName::Dataset,
                HirCicsOperandName::From => CicsOperandName::From,
                HirCicsOperandName::Ridfld => CicsOperandName::Ridfld,
            },
            value: match &operand.value {
                HirCicsValue::Literal(value) => {
                    CicsOperandValue::Literal(value.as_bytes().to_vec())
                }
                HirCicsValue::Data(reference) => CicsOperandValue::Storage(self.slot(reference)?),
            },
        })
    }

    fn output(
        &mut self,
        output: &HirCicsOutputBinding,
    ) -> Result<CicsOutputBinding, CicsPlanProblem> {
        Ok(CicsOutputBinding {
            name: match output.name {
                HirCicsOutputName::Into => CicsOutputName::Into,
                HirCicsOutputName::Resp => CicsOutputName::Resp,
                HirCicsOutputName::Resp2 => CicsOutputName::Resp2,
            },
            target: self.slot(&output.target)?,
        })
    }

    fn slot(&mut self, reference: &HirDataReference) -> Result<CicsStorageSlot, CicsPlanProblem> {
        let storage = *self
            .storage_ids
            .get(&reference.qualified_name)
            .ok_or_else(|| CicsPlanProblem::MissingStorage(reference.qualified_name.clone()))?;
        let length = if reference.dynamic {
            reference.dynamic_limit.unwrap_or(reference.length)
        } else {
            reference.length
        };
        if length == 0 {
            return Err(CicsPlanProblem::InvalidStorageExtent(
                reference.qualified_name.clone(),
            ));
        }
        let declared = StorageReference {
            storage,
            offset: 0,
            length: length as u64,
        };
        if self
            .references
            .insert(storage, declared.clone())
            .is_some_and(|existing| existing != declared)
        {
            return Err(CicsPlanProblem::InvalidStorageExtent(
                reference.qualified_name.clone(),
            ));
        }
        Ok(CicsStorageSlot {
            storage,
            qualified_layout_name: reference.qualified_name.clone(),
        })
    }
}

const fn plan_operation(operation: HirCicsOperation) -> CicsPlanOperation {
    match operation {
        HirCicsOperation::Read => CicsPlanOperation::Read,
        HirCicsOperation::Rewrite => CicsPlanOperation::Rewrite,
        HirCicsOperation::Syncpoint => CicsPlanOperation::Syncpoint,
    }
}

const fn plan_option(option: HirCicsOption) -> CicsPlanOption {
    match option {
        HirCicsOption::Update => CicsPlanOption::Update,
        HirCicsOption::Rollback => CicsPlanOption::Rollback,
        HirCicsOption::NoHandle => CicsPlanOption::NoHandle,
    }
}
