use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation, DriverOutput,
    DriverRef, FixtureRef, ObservationCheck, ObservationRef, SpecProblem,
};
use mainframe_env_execution_api::MachineDrive;
use mainframe_env_ir::{
    Attribute, DecimalArithmeticContext, DecimalConditionPolicy, DecimalPlanLimits,
    DecimalReceiverUpdatePolicy, DecimalStorageAbi, decode_binary, decode_decimal_assignment_plan,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("../../../../conformance/0.9/cobol/arithmetic-fixtures.json");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CobolArithmeticCaseReport {
    pub case_id: String,
    pub output_hex: String,
    pub typed_decimal_operation_majors: Vec<u16>,
    pub decimal_plan_assignment_counts: Vec<u32>,
    pub assignment_targets: Vec<String>,
    pub semantic_origins: Vec<String>,
    pub execution_policies: Vec<String>,
    pub legacy_cobol_arithmetic_operations: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CobolArithmeticPilotReport {
    pub schema_version: String,
    pub fixture_digest: String,
    pub comparison_policy: String,
    pub results: Vec<CobolArithmeticCaseReport>,
    pub differential_credit: u8,
    pub licensed_credit: u8,
}

pub struct CobolArithmeticPilotRuntime {
    driver: ArithmeticDriver,
    observation: ArithmeticContractObservation,
}

#[must_use]
pub const fn cobol_arithmetic_pilot_runtime() -> CobolArithmeticPilotRuntime {
    CobolArithmeticPilotRuntime {
        driver: ArithmeticDriver,
        observation: ArithmeticContractObservation,
    }
}

impl CobolArithmeticPilotRuntime {
    pub fn bind<'a>(
        &'a self,
        spec: &CompiledSpec,
        drivers: &mut Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        observations: &mut Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        limits: ConformanceLimits,
    ) -> Result<(), SpecProblem> {
        let driver = DriverRef::new("cobol.typed-arithmetic.product-path", limits)?;
        if !spec.registries().drivers().contains(&driver) {
            return Ok(());
        }
        drivers.push((driver, &self.driver));
        observations.push((
            ObservationRef::new("cobol.typed-arithmetic.exact-contract", limits)?,
            &self.observation,
        ));
        Ok(())
    }
}

struct ArithmeticDriver;

impl ConformanceDriver for ArithmeticDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        if fixture.as_str() != "cobol.typed-arithmetic.issue-140-141-v1" {
            return Err(format!("unknown COBOL typed-arithmetic fixture {fixture}"));
        }
        let report = run_cobol_arithmetic_pilot()?;
        let bytes = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
        DriverOutput::new(bytes, ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

struct ArithmeticContractObservation;

impl ConformanceObservation for ArithmeticContractObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let report: CobolArithmeticPilotReport =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        let fixture: Value = serde_json::from_str(FIXTURE).map_err(|error| error.to_string())?;
        let expected_results = expected_results(&fixture)?;
        let expected_policy = fixture["comparison_policy"]["version"]
            .as_str()
            .ok_or_else(|| "COBOL arithmetic comparison policy is missing".to_string())?;
        let expected_identity = serde_json::json!({
            "schema_version": "mainframe-env.cobol-arithmetic-pilot-observation@1",
            "fixture_digest": digest(FIXTURE.as_bytes()),
            "comparison_policy": expected_policy,
            "differential_credit": 0,
            "licensed_credit": 0,
        });
        let actual_identity = serde_json::json!({
            "schema_version": report.schema_version,
            "fixture_digest": report.fixture_digest,
            "comparison_policy": report.comparison_policy,
            "differential_credit": report.differential_credit,
            "licensed_credit": report.licensed_credit,
        });
        let actual_results =
            serde_json::to_value(&report.results).map_err(|error| error.to_string())?;
        ObservationCheck::new(
            actual_identity == expected_identity && actual_results == expected_results,
            format!("identity={expected_identity}; results={expected_results}"),
            format!("identity={actual_identity}; results={actual_results}"),
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

pub fn run_cobol_arithmetic_pilot() -> Result<CobolArithmeticPilotReport, String> {
    let fixture: Value = serde_json::from_str(FIXTURE).map_err(|error| error.to_string())?;
    let cases = fixture["cases"]
        .as_array()
        .ok_or_else(|| "COBOL arithmetic cases are missing".to_string())?;
    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        let case_id = required_text(case, "case_id")?;
        let source = required_text(case, "program_source")?;
        let artifact = crate::compile(source)?;
        let output = match crate::execute(&artifact, 4096) {
            MachineDrive::Completed(done) => done.output.bytes().to_vec(),
            other => {
                return Err(format!(
                    "COBOL arithmetic case {case_id} did not complete: {other:?}"
                ));
            }
        };
        let module = decode_binary(artifact.payload(), mainframe_env_ir::CodecLimits::default())
            .map_err(|problem| format!("COBOL arithmetic case {case_id}: {problem:?}"))?;
        let mut typed_decimal_operation_majors = Vec::new();
        let mut decimal_plan_assignment_counts = Vec::new();
        let mut assignment_targets = Vec::new();
        let mut semantic_origins = Vec::new();
        let mut execution_policies = Vec::new();
        let mut legacy_cobol_arithmetic_operations = 0u32;
        for operation in module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
        {
            if operation.identity.namespace() == "mainframe.core.cobol"
                && matches!(operation.identity.name(), "add" | "compute")
            {
                legacy_cobol_arithmetic_operations = legacy_cobol_arithmetic_operations
                    .checked_add(1)
                    .ok_or_else(|| "COBOL arithmetic operation count overflow".to_string())?;
            }
            if operation.identity.namespace() != "mainframe.decimal"
                || operation.identity.name() != "assign"
            {
                continue;
            }
            typed_decimal_operation_majors.push(operation.identity.major());
            let Attribute::Bytes(bytes) = operation
                .attributes
                .get("assignment_plan")
                .ok_or_else(|| format!("COBOL arithmetic case {case_id} has no decimal plan"))?
            else {
                return Err(format!(
                    "COBOL arithmetic case {case_id} has a non-byte decimal plan"
                ));
            };
            let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default())
                .map_err(|problem| format!("COBOL arithmetic case {case_id}: {problem:?}"))?;
            decimal_plan_assignment_counts.push(
                u32::try_from(plan.assignments.len())
                    .map_err(|_| "COBOL arithmetic assignment count overflow".to_string())?,
            );
            assignment_targets.extend(
                plan.assignments
                    .iter()
                    .map(|assignment| assignment.receiver.target.qualified_layout_name.clone()),
            );
            semantic_origins.push(plan.semantic_origin);
            execution_policies.push(policy_identity(plan.policy)?);
        }
        results.push(CobolArithmeticCaseReport {
            case_id: case_id.into(),
            output_hex: hex(&output),
            typed_decimal_operation_majors,
            decimal_plan_assignment_counts,
            assignment_targets,
            semantic_origins,
            execution_policies,
            legacy_cobol_arithmetic_operations,
        });
    }
    Ok(CobolArithmeticPilotReport {
        schema_version: "mainframe-env.cobol-arithmetic-pilot-observation@1".into(),
        fixture_digest: digest(FIXTURE.as_bytes()),
        comparison_policy: fixture["comparison_policy"]["version"]
            .as_str()
            .ok_or_else(|| "COBOL arithmetic comparison policy is missing".to_string())?
            .into(),
        results,
        differential_credit: 0,
        licensed_credit: 0,
    })
}

fn expected_results(fixture: &Value) -> Result<Value, String> {
    let cases = fixture["cases"]
        .as_array()
        .ok_or_else(|| "COBOL arithmetic cases are missing".to_string())?;
    Ok(Value::Array(
        cases
            .iter()
            .map(|case| {
                Ok(serde_json::json!({
                    "case_id": required_text(case, "case_id")?,
                    "output_hex": required_text(case, "expected_output_hex")?,
                    "typed_decimal_operation_majors": case["expected_typed_plan"]["operation_majors"],
                    "decimal_plan_assignment_counts": case["expected_typed_plan"]["assignment_counts"],
                    "assignment_targets": case["expected_typed_plan"]["assignment_targets"],
                    "semantic_origins": case["expected_typed_plan"]["semantic_origins"],
                    "execution_policies": case["expected_typed_plan"]["execution_policies"],
                    "legacy_cobol_arithmetic_operations": 0,
                }))
            })
            .collect::<Result<Vec<_>, String>>()?,
    ))
}

fn required_text<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("COBOL arithmetic fixture field {name} is missing"))
}

fn policy_identity(policy: mainframe_env_ir::DecimalExecutionPolicy) -> Result<String, String> {
    let context = match policy.arithmetic_context {
        DecimalArithmeticContext::Decimal18V1 => "decimal18-v1",
        DecimalArithmeticContext::Decimal34V1 => "decimal34-v1",
        DecimalArithmeticContext::LegacyCobolModuleV1 => {
            return Err("implicit legacy arithmetic context reached the typed COBOL pilot".into());
        }
    };
    if policy.storage_abi != DecimalStorageAbi::CobolNumericV1
        || policy.receiver_update != DecimalReceiverUpdatePolicy::CapturedOperandsReceiverLocalV1
        || policy.condition != DecimalConditionPolicy::CobolSizeErrorV1
    {
        return Err("unexpected typed COBOL decimal execution policy".into());
    }
    Ok(format!(
        "{context}/cobol-numeric-v1/captured-operands-receiver-local-v1/cobol-size-error-v1"
    ))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)] as char,
                DIGITS[usize::from(byte & 15)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cobol_arithmetic_runs_source_to_exact_independent_contract() {
        let report = run_cobol_arithmetic_pilot().unwrap();
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(
            serde_json::to_value(report.results).unwrap(),
            expected_results(&fixture).unwrap()
        );
        assert_eq!(report.differential_credit, 0);
        assert_eq!(report.licensed_credit, 0);
    }

    #[test]
    fn cobol_arithmetic_comparator_detects_runtime_plan_and_identity_drift() {
        let report = run_cobol_arithmetic_pilot().unwrap();

        let mut wrong_output = report.clone();
        wrong_output.results[0].output_hex = "3439310a".into();
        let output = DriverOutput::new(
            serde_json::to_vec(&wrong_output).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(
            !ArithmeticContractObservation
                .evaluate(&output)
                .unwrap()
                .matched
        );

        let mut wrong_plan = report.clone();
        wrong_plan.results[1].decimal_plan_assignment_counts = vec![99];
        let output = DriverOutput::new(
            serde_json::to_vec(&wrong_plan).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(
            !ArithmeticContractObservation
                .evaluate(&output)
                .unwrap()
                .matched
        );

        let mut stale = report;
        stale.fixture_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        let output = DriverOutput::new(
            serde_json::to_vec(&stale).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(
            !ArithmeticContractObservation
                .evaluate(&output)
                .unwrap()
                .matched
        );
    }
}
