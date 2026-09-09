//! Bounded non-COBOL adapter proof for the shared decimal executable contract.

use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Machine,
    MachineDrive, MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{
    Attribute, CodecLimits, DecimalAssignment, DecimalAssignmentPlan, DecimalConditionContract,
    DecimalExecutionPolicy, DecimalExpression, DecimalOperationContract, DecimalPlanLimits,
    DecimalPlanWireVersion, DecimalReceiver, DecimalRoundingPolicy, DecimalStorageSlot, Effect,
    IrLimits, LegalityProfile, Module, ModuleBuilder, OperationCatalog, OperationIdentity,
    OperationSchema, OperationSemanticContract, StorageId, StorageReference, encode_binary,
    encode_decimal_assignment_plan, verify_legal,
};
use std::collections::{BTreeMap, BTreeSet};

/// Independent source identity used by the bounded second-frontend proof.
pub const LEDGER_FORMULA_CONTRACT: &str = "ledger.formula@1";

const CORE_NAMESPACE: &str = "mainframe.core.cobol";
const DECIMAL_NAMESPACE: &str = "mainframe.decimal";
const PLAN_ATTRIBUTE: &str = "assignment_plan";
const CONDITION_STATUS_ATTRIBUTE: &str = "typed_condition_status";
const CONDITION_BRANCHES_ATTRIBUTE: &str = "typed_condition_branches";
const CONDITION_POLARITY_ATTRIBUTE: &str = "typed_condition_polarity";
const SIZE_ERROR_STATUS: &str = "cobol.arithmetic-size-error@1";
const MAX_SOURCE_BYTES: usize = 1_024;
const MAX_DIGITS: usize = 34;

/// Evidence returned after parsing, verifying, and executing one ledger formula.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalAdapterReceipt {
    /// Source contract parsed by the independent adapter.
    pub input_contract: String,
    /// Executable operation selected by the adapter.
    pub executable_contract: String,
    /// Canonical plan contract carried by that operation.
    pub plan_contract: String,
    /// Qualified receiver whose exact bytes were observed.
    pub result_name: String,
    /// Exact receiver bytes after normal reference-machine execution.
    pub result_bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LedgerContext {
    Decimal18V1,
    Decimal34V1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LedgerField {
    name: String,
    value: u128,
    digits: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LedgerFormula {
    context: LedgerContext,
    left: LedgerField,
    right: LedgerField,
    receiver_name: String,
    receiver_digits: usize,
}

/// Parse and execute one bounded `ledger.formula@1` input through normal IR
/// verification and the production reference machine.
pub fn verify_decimal_adapter(source: &str) -> Result<DecimalAdapterReceipt, String> {
    let formula = LedgerFormula::parse(source)?;
    let module = lower_formula(&formula, decimal_identity(2), |bytes| bytes)?;
    let catalog = adapter_catalog()?;
    let profile = adapter_profile(&catalog);
    let legal = verify_legal(module, &catalog, &profile).map_err(|problem| problem.to_string())?;
    let binary = encode_binary(legal.module(), CodecLimits::default())
        .map_err(|problem| problem.to_string())?;
    let mut machine = ReferenceMachine::from_binary(&binary, invocation()?, CodecLimits::default())
        .map_err(|problem| format!("{problem:?}"))?;
    match machine.drive(MachineResume::Start, Quantum::new(100, 4_096).unwrap()) {
        MachineDrive::Completed(_) => {}
        other => return Err(format!("ledger formula did not complete: {other:?}")),
    }
    let result_bytes = machine
        .variable(&formula.receiver_name)
        .ok_or_else(|| "ledger result storage is unavailable".to_string())?
        .bytes()
        .to_vec();
    Ok(DecimalAdapterReceipt {
        input_contract: LEDGER_FORMULA_CONTRACT.into(),
        executable_contract: decimal_identity(2).to_string(),
        plan_contract: mainframe_env_ir::DECIMAL_ASSIGNMENT_PLAN_CONTRACT.into(),
        result_name: formula.receiver_name,
        result_bytes,
    })
}

impl LedgerFormula {
    fn parse(source: &str) -> Result<Self, String> {
        if source.len() > MAX_SOURCE_BYTES || !source.is_ascii() {
            return Err("ledger formula exceeds its source contract".into());
        }
        let lines = source.lines().map(str::trim).collect::<Vec<_>>();
        if lines.len() != 6 || lines[0] != LEDGER_FORMULA_CONTRACT {
            return Err("ledger formula identity or shape is invalid".into());
        }
        let context = match words(lines[1]).as_slice() {
            ["context", "decimal18-v1"] => LedgerContext::Decimal18V1,
            ["context", "decimal34-v1"] => LedgerContext::Decimal34V1,
            _ => return Err("ledger arithmetic context is unsupported".into()),
        };
        let left = parse_input(lines[2])?;
        let right = parse_input(lines[3])?;
        if left.name == right.name {
            return Err("ledger inputs must have distinct names".into());
        }
        let receiver = words(lines[4]);
        let ["receiver", receiver_name, "digits", receiver_digits] = receiver.as_slice() else {
            return Err("ledger receiver declaration is invalid".into());
        };
        validate_name(receiver_name)?;
        let receiver_digits = parse_digits(receiver_digits)?;
        if *receiver_name == left.name || *receiver_name == right.name {
            return Err("ledger receiver must have distinct storage".into());
        }
        let assignment = words(lines[5]);
        if assignment.as_slice()
            != [
                "assign",
                *receiver_name,
                "=",
                left.name.as_str(),
                "+",
                right.name.as_str(),
            ]
        {
            return Err("ledger assignment does not match its declarations".into());
        }
        Ok(Self {
            context,
            left,
            right,
            receiver_name: (*receiver_name).into(),
            receiver_digits,
        })
    }
}

fn words(line: &str) -> Vec<&str> {
    line.split_ascii_whitespace().collect()
}

fn parse_input(line: &str) -> Result<LedgerField, String> {
    let fields = words(line);
    let ["input", name, value, "digits", digits] = fields.as_slice() else {
        return Err("ledger input declaration is invalid".into());
    };
    validate_name(name)?;
    let digits = parse_digits(digits)?;
    let value = value
        .parse::<u128>()
        .map_err(|_| "ledger input value is invalid")?;
    if value.to_string().len() > digits {
        return Err("ledger input does not fit its declared digits".into());
    }
    Ok(LedgerField {
        name: (*name).into(),
        value,
        digits,
    })
}

fn parse_digits(value: &str) -> Result<usize, String> {
    let digits = value
        .parse::<usize>()
        .map_err(|_| "ledger digit count is invalid")?;
    if (1..=MAX_DIGITS).contains(&digits) {
        Ok(digits)
    } else {
        Err("ledger digit count is unsupported".into())
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.bytes().any(|byte| byte.is_ascii_lowercase())
        || name.split('.').any(|part| part.is_empty())
        || !name.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        Err("ledger storage name is noncanonical".into())
    } else {
        Ok(())
    }
}

fn lower_formula(
    formula: &LedgerFormula,
    decimal_operation: OperationIdentity,
    transform_plan: impl FnOnce(Vec<u8>) -> Vec<u8>,
) -> Result<Module, String> {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let left = builder
        .add_storage(
            formula.left.name.to_ascii_lowercase(),
            formula.left.digits as u64,
            None,
        )
        .map_err(|problem| problem.to_string())?;
    let right = builder
        .add_storage(
            formula.right.name.to_ascii_lowercase(),
            formula.right.digits as u64,
            None,
        )
        .map_err(|problem| problem.to_string())?;
    let receiver = builder
        .add_storage(
            formula.receiver_name.to_ascii_lowercase(),
            formula.receiver_digits as u64,
            None,
        )
        .map_err(|problem| problem.to_string())?;
    let region = builder
        .add_region()
        .map_err(|problem| problem.to_string())?;
    let block = builder
        .add_block(region)
        .map_err(|problem| problem.to_string())?;
    for (field, storage) in [(&formula.left, left), (&formula.right, right)] {
        add_layout(&mut builder, block, &field.name, field.digits)?;
        add_initial(&mut builder, block, storage, field.digits, field.value)?;
    }
    add_layout(
        &mut builder,
        block,
        &formula.receiver_name,
        formula.receiver_digits,
    )?;
    add_initial(&mut builder, block, receiver, formula.receiver_digits, 0)?;

    let slot = |storage, name: &str| DecimalStorageSlot {
        storage,
        qualified_layout_name: name.into(),
    };
    let policy = match formula.context {
        LedgerContext::Decimal18V1 => DecimalExecutionPolicy::decimal18_v1(),
        LedgerContext::Decimal34V1 => DecimalExecutionPolicy::decimal34_v1(),
    };
    let plan = DecimalAssignmentPlan {
        semantic_origin: LEDGER_FORMULA_CONTRACT.into(),
        policy,
        assignments: vec![DecimalAssignment {
            expression: DecimalExpression::Add {
                left: Box::new(DecimalExpression::Storage(slot(left, &formula.left.name))),
                right: Box::new(DecimalExpression::Storage(slot(right, &formula.right.name))),
            },
            receiver: DecimalReceiver {
                target: slot(receiver, &formula.receiver_name),
                rounding: DecimalRoundingPolicy::Truncation,
            },
        }],
    };
    let plan = transform_plan(
        encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default())
            .map_err(|problem| problem.to_string())?,
    );
    builder
        .add_operation(
            block,
            decimal_operation,
            Vec::new(),
            0,
            BTreeMap::from([
                (PLAN_ATTRIBUTE.into(), Attribute::Bytes(plan)),
                (
                    CONDITION_STATUS_ATTRIBUTE.into(),
                    Attribute::Text(SIZE_ERROR_STATUS.into()),
                ),
                (CONDITION_BRANCHES_ATTRIBUTE.into(), Attribute::Integer(0)),
            ]),
            vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition],
            [
                (left, formula.left.digits),
                (right, formula.right.digits),
                (receiver, formula.receiver_digits),
            ]
            .into_iter()
            .map(|(storage, length)| StorageReference {
                storage,
                offset: 0,
                length: length as u64,
            })
            .collect(),
            None,
        )
        .map_err(|problem| problem.to_string())?;
    builder
        .add_operation(
            block,
            core_identity("halt"),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .map_err(|problem| problem.to_string())?;
    builder.finish().map_err(|problem| problem.to_string())
}

fn add_layout(
    builder: &mut ModuleBuilder,
    block: mainframe_env_ir::BlockId,
    name: &str,
    digits: usize,
) -> Result<(), String> {
    builder
        .add_operation(
            block,
            core_identity("define"),
            Vec::new(),
            0,
            BTreeMap::from([
                ("name".into(), Attribute::Text(name.into())),
                (
                    "simple_name".into(),
                    Attribute::Text(name.rsplit('.').next().unwrap_or(name).into()),
                ),
                ("category".into(), Attribute::Text("numeric_display".into())),
                ("picture".into(), Attribute::Text(String::new())),
                ("digits".into(), Attribute::Integer(digits as i64)),
                ("scale".into(), Attribute::Integer(0)),
                ("signed".into(), Attribute::Integer(0)),
                ("sign_separate".into(), Attribute::Integer(0)),
                ("section".into(), Attribute::Text("working".into())),
                ("offset".into(), Attribute::Integer(0)),
                ("length".into(), Attribute::Integer(digits as i64)),
                ("element_length".into(), Attribute::Integer(digits as i64)),
                ("occurs".into(), Attribute::Integer(1)),
                ("parent".into(), Attribute::Text(String::new())),
                ("condition_values".into(), Attribute::Text(String::new())),
            ]),
            Vec::new(),
            Vec::new(),
            None,
        )
        .map(|_| ())
        .map_err(|problem| problem.to_string())
}

fn add_initial(
    builder: &mut ModuleBuilder,
    block: mainframe_env_ir::BlockId,
    storage: StorageId,
    digits: usize,
    value: u128,
) -> Result<(), String> {
    let bytes = format!("{value:0digits$}").into_bytes();
    builder
        .add_operation(
            block,
            core_identity("init"),
            Vec::new(),
            0,
            BTreeMap::from([("initial".into(), Attribute::Bytes(bytes))]),
            vec![Effect::MemoryWrite],
            vec![StorageReference {
                storage,
                offset: 0,
                length: digits as u64,
            }],
            None,
        )
        .map(|_| ())
        .map_err(|problem| problem.to_string())
}

fn adapter_catalog() -> Result<OperationCatalog, String> {
    let mut catalog = OperationCatalog::default();
    catalog
        .register(OperationSchema::pure(core_identity("define"), 0, 0))
        .map_err(|problem| problem.to_string())?;
    let mut init = OperationSchema::pure(core_identity("init"), 0, 0);
    init.allowed_effects.insert(Effect::MemoryWrite);
    catalog
        .register(init)
        .map_err(|problem| problem.to_string())?;
    let mut halt = OperationSchema::pure(core_identity("halt"), 0, 0);
    halt.terminator = true;
    catalog
        .register(halt)
        .map_err(|problem| problem.to_string())?;
    let mut decimal = OperationSchema::pure(decimal_identity(2), 0, 0);
    decimal.required_attributes = [
        PLAN_ATTRIBUTE.into(),
        CONDITION_STATUS_ATTRIBUTE.into(),
        CONDITION_BRANCHES_ATTRIBUTE.into(),
    ]
    .into_iter()
    .collect();
    decimal.allowed_effects = [Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]
        .into_iter()
        .collect();
    decimal.semantic_contract =
        OperationSemanticContract::DecimalAssignment(DecimalOperationContract {
            plan_attribute: PLAN_ATTRIBUTE.into(),
            expected_plan_version: DecimalPlanWireVersion::PolicyV2,
            allowed_semantic_origins: BTreeSet::new(),
            layout_definition_operation: Some(core_identity("define")),
            condition: Some(DecimalConditionContract {
                status: SIZE_ERROR_STATUS.into(),
                status_attribute: CONDITION_STATUS_ATTRIBUTE.into(),
                branch_mask_attribute: CONDITION_BRANCHES_ATTRIBUTE.into(),
                branch_polarity_attribute: CONDITION_POLARITY_ATTRIBUTE.into(),
            }),
        });
    catalog
        .register(decimal)
        .map_err(|problem| problem.to_string())?;
    Ok(catalog)
}

fn adapter_profile(catalog: &OperationCatalog) -> LegalityProfile {
    LegalityProfile {
        allowed_operations: catalog.identities().cloned().collect(),
        allowed_runtime_imports: BTreeSet::new(),
    }
}

fn core_identity(name: &str) -> OperationIdentity {
    OperationIdentity::new(CORE_NAMESPACE, name, 1).expect("static adapter operation identity")
}

fn decimal_identity(major: u16) -> OperationIdentity {
    OperationIdentity::new(DECIMAL_NAMESPACE, "assign", major)
        .expect("static decimal operation identity")
}

fn invocation() -> Result<Invocation, String> {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("ledger-decimal-request", limits).map_err(|problem| problem.to_string())?,
        ExecutionId::new("ledger-decimal-execution", limits)
            .map_err(|problem| problem.to_string())?,
        RunUnitId::new("ledger-decimal-run", limits).map_err(|problem| problem.to_string())?,
        None,
        Selector::new("adapter:ledger.formula@1", limits).map_err(|problem| problem.to_string())?,
        ArtifactRef::new("ledger-decimal-artifact", limits)
            .map_err(|problem| problem.to_string())?,
        Principal::new(
            PrincipalId::new("LEDGER", limits).map_err(|problem| problem.to_string())?,
            BTreeSet::<CapabilityId>::new(),
            limits,
        )
        .map_err(|problem| problem.to_string())?,
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("ledger-decimal-trace", limits).map_err(|problem| problem.to_string())?,
        IdempotencyKey::new("ledger-decimal-idempotency", limits)
            .map_err(|problem| problem.to_string())?,
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|problem| problem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECIMAL18: &str = "ledger.formula@1\n\
context decimal18-v1\n\
input LEDGER.LEFT 999999999999999999 digits 18\n\
input LEDGER.RIGHT 2 digits 18\n\
receiver LEDGER.TOTAL digits 19\n\
assign LEDGER.TOTAL = LEDGER.LEFT + LEDGER.RIGHT";

    const DECIMAL34: &str = "ledger.formula@1\n\
context decimal34-v1\n\
input LEDGER.LEFT 999999999999999999 digits 18\n\
input LEDGER.RIGHT 2 digits 18\n\
receiver LEDGER.TOTAL digits 19\n\
assign LEDGER.TOTAL = LEDGER.LEFT + LEDGER.RIGHT";

    #[test]
    fn independent_frontend_verifies_and_executes_exact_policy_bytes() {
        let decimal18 = verify_decimal_adapter(DECIMAL18).unwrap();
        let decimal34 = verify_decimal_adapter(DECIMAL34).unwrap();
        assert_eq!(decimal18.input_contract, LEDGER_FORMULA_CONTRACT);
        assert_eq!(decimal18.executable_contract, "mainframe.decimal@2.assign");
        assert_eq!(
            decimal18.plan_contract,
            "mainframe-env.decimal-assignment-plan@2"
        );
        assert_eq!(decimal18.result_bytes, b"1000000000000000000");
        assert_eq!(decimal34.result_bytes, b"1000000000000000001");
    }

    #[test]
    fn unsupported_or_mismatched_policy_contracts_receive_no_success() {
        let unsupported_source = DECIMAL34.replace("decimal34-v1", "decimal35-v1");
        assert!(verify_decimal_adapter(&unsupported_source).is_err());

        let formula = LedgerFormula::parse(DECIMAL34).unwrap();
        let unsupported_policy = lower_formula(&formula, decimal_identity(2), |mut bytes| {
            bytes[7] = u8::MAX;
            bytes
        })
        .unwrap();
        assert!(
            verify_legal(
                unsupported_policy,
                &adapter_catalog().unwrap(),
                &adapter_profile(&adapter_catalog().unwrap())
            )
            .is_err()
        );

        let mismatched_version =
            lower_formula(&formula, decimal_identity(1), |bytes| bytes).unwrap();
        let catalog = adapter_catalog().unwrap();
        assert!(verify_legal(mismatched_version, &catalog, &adapter_profile(&catalog)).is_err());
    }
}
