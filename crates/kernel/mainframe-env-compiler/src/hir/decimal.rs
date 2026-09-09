use super::{
    HirAddMode, HirArithmeticReceiver, HirBinaryOperator, HirDataReference, HirNumericExpression,
    HirNumericLiteral, HirProblem, HirResolvedStatement, HirRounding, HirStatement,
    HirUnaryOperator, StatementKind, effects,
};
use mainframe_env_ir::{
    Attribute, BlockId, DecimalAssignment, DecimalAssignmentPlan, DecimalConditionContract,
    DecimalExecutionPolicy, DecimalExpression, DecimalOperationContract, DecimalPlanLimits,
    DecimalPlanWireVersion, DecimalReceiver, DecimalRoundingPolicy, DecimalStorageSlot, Effect,
    ModuleBuilder, OperationCatalog, OperationIdentity, OperationSchema, OperationSemanticContract,
    StorageId, StorageReference, cobol_layout_definition_identity, encode_decimal_assignment_plan,
};
use std::collections::BTreeMap;

pub(crate) const DECIMAL_NAMESPACE: &str = "mainframe.decimal";
pub(crate) const DECIMAL_ASSIGN: &str = "assign";
pub(crate) const ASSIGNMENT_PLAN_ATTRIBUTE: &str = "assignment_plan";
pub(crate) const CONDITION_STATUS_ATTRIBUTE: &str = "typed_condition_status";
pub(crate) const CONDITION_BRANCHES_ATTRIBUTE: &str = "typed_condition_branches";
pub(crate) const CONDITION_POLARITY_ATTRIBUTE: &str = "typed_condition_polarity";
pub(crate) const SIZE_ERROR_STATUS: &str = "cobol.arithmetic-size-error@1";

pub(crate) struct EncodedDecimalAssignment {
    pub bytes: Vec<u8>,
    pub storage: Vec<StorageReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DecimalPlanProblem {
    NotArithmetic,
    MissingStorage(String),
    InvalidStorageExtent(String),
    InvalidLiteral,
    InvalidPlan,
    InvalidArithmeticContext,
}

pub(crate) fn execution_policy(
    arithmetic_mode: &str,
) -> Result<DecimalExecutionPolicy, DecimalPlanProblem> {
    match arithmetic_mode {
        "compatible" => Ok(DecimalExecutionPolicy::decimal18_v1()),
        "extended" => Ok(DecimalExecutionPolicy::decimal34_v1()),
        _ => Err(DecimalPlanProblem::InvalidArithmeticContext),
    }
}

pub(crate) fn hir_execution_policy(
    arithmetic_mode: &str,
) -> Result<DecimalExecutionPolicy, HirProblem> {
    execution_policy(arithmetic_mode).map_err(|_| {
        HirProblem::InvalidResolvedStatement(
            StatementKind::Compute,
            0,
            "invalid effective arithmetic context".into(),
        )
    })
}

pub(crate) fn executable_identity() -> OperationIdentity {
    OperationIdentity::new(DECIMAL_NAMESPACE, DECIMAL_ASSIGN, 2)
        .expect("static decimal assignment identity")
}

fn legacy_executable_identity() -> OperationIdentity {
    OperationIdentity::new(DECIMAL_NAMESPACE, DECIMAL_ASSIGN, 1)
        .expect("static legacy decimal assignment identity")
}

pub(crate) fn register_hir_operations(catalog: &mut OperationCatalog) {
    for kind in [StatementKind::Add, StatementKind::Compute] {
        let identity =
            OperationIdentity::new("cobol.hir", kind.slug(), 2).expect("static typed HIR identity");
        let mut schema = OperationSchema::pure(identity, 0, 0);
        schema.required_attributes = [ASSIGNMENT_PLAN_ATTRIBUTE.into()].into_iter().collect();
        schema.allowed_effects = effects(kind).into_iter().collect();
        schema.semantic_contract =
            OperationSemanticContract::DecimalAssignment(DecimalOperationContract {
                plan_attribute: ASSIGNMENT_PLAN_ATTRIBUTE.into(),
                expected_plan_version: DecimalPlanWireVersion::PolicyV2,
                allowed_semantic_origins: [format!("cobol.{}@1", kind.slug())]
                    .into_iter()
                    .collect(),
                layout_definition_operation: None,
                condition: None,
            });
        catalog
            .register(schema)
            .expect("unique typed HIR operation");
    }
}

pub(crate) fn register_executable_operation(catalog: &mut OperationCatalog) {
    for (identity, version, origins) in [
        (
            legacy_executable_identity(),
            DecimalPlanWireVersion::LegacyV1,
            ["cobol.add@1".into(), "cobol.compute@1".into()]
                .into_iter()
                .collect(),
        ),
        (
            executable_identity(),
            DecimalPlanWireVersion::PolicyV2,
            Default::default(),
        ),
    ] {
        let mut schema = OperationSchema::pure(identity, 0, 0);
        schema.required_attributes = [
            ASSIGNMENT_PLAN_ATTRIBUTE.into(),
            CONDITION_STATUS_ATTRIBUTE.into(),
            CONDITION_BRANCHES_ATTRIBUTE.into(),
        ]
        .into_iter()
        .collect();
        schema.allowed_effects = [Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]
            .into_iter()
            .collect();
        schema.semantic_contract =
            OperationSemanticContract::DecimalAssignment(DecimalOperationContract {
                plan_attribute: ASSIGNMENT_PLAN_ATTRIBUTE.into(),
                expected_plan_version: version,
                allowed_semantic_origins: origins,
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
            .expect("unique decimal assignment operation");
    }
}

pub(crate) fn emit_hir_operation(
    statement: &HirStatement,
    builder: &mut ModuleBuilder,
    block: BlockId,
    storage_ids: &BTreeMap<String, StorageId>,
    policy: DecimalExecutionPolicy,
) -> Result<bool, HirProblem> {
    let Some(resolved @ (HirResolvedStatement::Add(_) | HirResolvedStatement::Compute(_))) =
        statement.resolved.as_ref()
    else {
        return Ok(false);
    };
    let encoded = encode_statement(resolved, storage_ids, policy).map_err(|problem| {
        HirProblem::InvalidResolvedStatement(
            statement.kind,
            statement.line,
            format!("decimal plan construction failed: {problem:?}"),
        )
    })?;
    builder
        .add_operation(
            block,
            OperationIdentity::new("cobol.hir", statement.kind.slug(), 2)
                .map_err(|_| HirProblem::UnknownStatement(statement.line))?,
            Vec::new(),
            0,
            BTreeMap::from([
                ("line".into(), Attribute::Integer(statement.line as i64)),
                (
                    ASSIGNMENT_PLAN_ATTRIBUTE.into(),
                    Attribute::Bytes(encoded.bytes),
                ),
            ]),
            effects(statement.kind),
            encoded.storage,
            statement.location.clone(),
        )
        .map_err(|_| HirProblem::StatementLimitExceeded)?;
    Ok(true)
}

pub(crate) fn encode_statement(
    resolved: &HirResolvedStatement,
    storage_ids: &BTreeMap<String, StorageId>,
    policy: DecimalExecutionPolicy,
) -> Result<EncodedDecimalAssignment, DecimalPlanProblem> {
    let mut context = PlanContext {
        storage_ids,
        references: BTreeMap::new(),
    };
    let plan = match resolved {
        HirResolvedStatement::Add(add) => DecimalAssignmentPlan {
            semantic_origin: "cobol.add@1".into(),
            policy,
            assignments: match &add.mode {
                HirAddMode::To { sources, receivers } => {
                    let sources = context.sum(sources)?;
                    context.assign_receivers(receivers, |receiver, context| {
                        Ok(DecimalExpression::Add {
                            left: Box::new(context.storage_expression(&receiver.target)?),
                            right: Box::new(sources.clone()),
                        })
                    })?
                }
                HirAddMode::Giving {
                    sources,
                    augend,
                    receivers,
                } => {
                    let mut value = context.sum(sources)?;
                    if let Some(augend) = augend {
                        value = DecimalExpression::Add {
                            left: Box::new(value),
                            right: Box::new(context.expression(augend)?),
                        };
                    }
                    context.assign_receivers(receivers, |_, _| Ok(value.clone()))?
                }
                HirAddMode::Corresponding { pairs, .. } => pairs
                    .iter()
                    .map(|pair| {
                        let expression = DecimalExpression::Add {
                            left: Box::new(context.storage_expression(&pair.receiver.target)?),
                            right: Box::new(context.storage_expression(&pair.source)?),
                        };
                        context.assignment(&pair.receiver, expression)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            },
        },
        HirResolvedStatement::Compute(compute) => {
            let expression = context.expression(&compute.expression)?;
            DecimalAssignmentPlan {
                semantic_origin: "cobol.compute@1".into(),
                policy,
                assignments: context
                    .assign_receivers(&compute.receivers, |_, _| Ok(expression.clone()))?,
            }
        }
        HirResolvedStatement::Cics(_) => return Err(DecimalPlanProblem::NotArithmetic),
    };
    let bytes = encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default())
        .map_err(|_| DecimalPlanProblem::InvalidPlan)?;
    Ok(EncodedDecimalAssignment {
        bytes,
        storage: context.references.into_values().collect(),
    })
}

struct PlanContext<'a> {
    storage_ids: &'a BTreeMap<String, StorageId>,
    references: BTreeMap<StorageId, StorageReference>,
}

impl PlanContext<'_> {
    fn assign_receivers(
        &mut self,
        receivers: &[HirArithmeticReceiver],
        mut expression: impl FnMut(
            &HirArithmeticReceiver,
            &mut Self,
        ) -> Result<DecimalExpression, DecimalPlanProblem>,
    ) -> Result<Vec<DecimalAssignment>, DecimalPlanProblem> {
        receivers
            .iter()
            .map(|receiver| {
                let expression = expression(receiver, self)?;
                self.assignment(receiver, expression)
            })
            .collect()
    }

    fn assignment(
        &mut self,
        receiver: &HirArithmeticReceiver,
        expression: DecimalExpression,
    ) -> Result<DecimalAssignment, DecimalPlanProblem> {
        Ok(DecimalAssignment {
            expression,
            receiver: DecimalReceiver {
                target: self.slot(&receiver.target)?,
                rounding: match receiver.rounding {
                    HirRounding::Truncate => DecimalRoundingPolicy::Truncation,
                    HirRounding::Rounded => DecimalRoundingPolicy::NearestAwayFromZero,
                },
            },
        })
    }

    fn sum(
        &mut self,
        expressions: &[HirNumericExpression],
    ) -> Result<DecimalExpression, DecimalPlanProblem> {
        let mut expressions = expressions.iter();
        let first = expressions.next().ok_or(DecimalPlanProblem::InvalidPlan)?;
        let mut result = DecimalExpression::Add {
            left: Box::new(DecimalExpression::Literal {
                coefficient: 0,
                scale: 0,
            }),
            right: Box::new(self.expression(first)?),
        };
        for expression in expressions {
            result = DecimalExpression::Add {
                left: Box::new(result),
                right: Box::new(self.expression(expression)?),
            };
        }
        Ok(result)
    }

    fn expression(
        &mut self,
        expression: &HirNumericExpression,
    ) -> Result<DecimalExpression, DecimalPlanProblem> {
        Ok(match expression {
            HirNumericExpression::Literal(literal) => self.literal(literal)?,
            HirNumericExpression::Data(reference) => self.storage_expression(reference)?,
            HirNumericExpression::LengthOf(reference) => {
                DecimalExpression::Length(self.slot(reference)?)
            }
            HirNumericExpression::Unary { operator, operand } => {
                let operand = self.expression(operand)?;
                match operator {
                    HirUnaryOperator::Plus => operand,
                    HirUnaryOperator::Negate => DecimalExpression::Negate(Box::new(operand)),
                }
            }
            HirNumericExpression::Binary {
                operator,
                left,
                right,
            } => {
                let left = Box::new(self.expression(left)?);
                let right = Box::new(self.expression(right)?);
                match operator {
                    HirBinaryOperator::Add => DecimalExpression::Add { left, right },
                    HirBinaryOperator::Subtract => DecimalExpression::Subtract { left, right },
                    HirBinaryOperator::Multiply => DecimalExpression::Multiply { left, right },
                    HirBinaryOperator::Divide => DecimalExpression::Divide { left, right },
                }
            }
        })
    }

    fn literal(
        &self,
        literal: &HirNumericLiteral,
    ) -> Result<DecimalExpression, DecimalPlanProblem> {
        let coefficient = literal
            .digits
            .parse::<i128>()
            .map_err(|_| DecimalPlanProblem::InvalidLiteral)?;
        let coefficient = if literal.negative {
            coefficient
                .checked_neg()
                .ok_or(DecimalPlanProblem::InvalidLiteral)?
        } else {
            coefficient
        };
        Ok(DecimalExpression::Literal {
            coefficient,
            scale: u32::try_from(literal.scale).map_err(|_| DecimalPlanProblem::InvalidLiteral)?,
        })
    }

    fn storage_expression(
        &mut self,
        reference: &HirDataReference,
    ) -> Result<DecimalExpression, DecimalPlanProblem> {
        self.slot(reference).map(DecimalExpression::Storage)
    }

    fn slot(
        &mut self,
        reference: &HirDataReference,
    ) -> Result<DecimalStorageSlot, DecimalPlanProblem> {
        let storage = *self
            .storage_ids
            .get(&reference.qualified_name)
            .ok_or_else(|| DecimalPlanProblem::MissingStorage(reference.qualified_name.clone()))?;
        let length = if reference.dynamic {
            reference.dynamic_limit.unwrap_or(reference.length)
        } else {
            reference.length
        };
        if length == 0 {
            return Err(DecimalPlanProblem::InvalidStorageExtent(
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
            return Err(DecimalPlanProblem::InvalidStorageExtent(
                reference.qualified_name.clone(),
            ));
        }
        Ok(DecimalStorageSlot {
            storage,
            qualified_layout_name: reference.qualified_name.clone(),
        })
    }
}
