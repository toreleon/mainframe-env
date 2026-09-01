use mainframe_env_compiler::{
    CobolCompiler, INTRINSIC_FUNCTIONS, IntrinsicValueType, SPECIAL_REGISTERS,
};
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.3/cobol/function-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.function.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<FunctionFixture>,
    special_registers: Vec<RegisterFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionFixture {
    id: String,
    row_id: String,
    name: String,
    valid_expression: String,
    invalid_expression: String,
    result_type: String,
    fixed_length: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterFixture {
    id: String,
    name: String,
    value_type: String,
    length_kind: String,
    fixed_length: Option<usize>,
    writable: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct FunctionOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    type_hits: usize,
    provenance_hits: usize,
    details: Vec<String>,
}

struct FunctionDriver;
struct FixtureAvailable;
struct AcceptedObservation;
struct RejectedObservation;

static FUNCTION_DRIVER: FunctionDriver = FunctionDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static ACCEPTED_OBSERVATION: AcceptedObservation = AcceptedObservation;
static REJECTED_OBSERVATION: RejectedObservation = RejectedObservation;

pub fn verify_cobol_function_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-function-fixtures@1"
        || catalog.target_version != "0.3.0"
        || catalog.fixtures.len() != 82
        || catalog.special_registers.len() != 28
    {
        return Err("COBOL function fixture identity or denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || fixture.valid_expression.len() > 512
            || fixture.invalid_expression.len() > 512
        {
            return Err(format!("invalid COBOL function fixture {}", fixture.id));
        }
        let descriptor = INTRINSIC_FUNCTIONS
            .iter()
            .find(|descriptor| descriptor.id == fixture.id)
            .ok_or_else(|| format!("function fixture {} has no descriptor", fixture.id))?;
        if descriptor.row_id != fixture.row_id
            || descriptor.name != fixture.name
            || descriptor.fixed_length != fixture.fixed_length
        {
            return Err(format!("function fixture {} drifted", fixture.id));
        }
    }
    let actual_registers = SPECIAL_REGISTERS
        .iter()
        .map(|register| (register.id, register))
        .collect::<BTreeMap<_, _>>();
    let mut register_ids = BTreeSet::new();
    for fixture in &catalog.special_registers {
        let register = actual_registers
            .get(fixture.id.as_str())
            .ok_or_else(|| format!("special-register fixture {} is unknown", fixture.id))?;
        if !register_ids.insert(fixture.id.as_str())
            || register.name != fixture.name
            || register_value_type_slug(register.value_type) != fixture.value_type
            || length_kind_slug(register.length_kind) != fixture.length_kind
            || register.fixed_length != fixture.fixed_length
            || register.writable != fixture.writable
        {
            return Err(format!("special-register fixture {} drifted", fixture.id));
        }
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.function.driver", limits).map_err(|error| error.to_string())?,
        &FUNCTION_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.function.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![
        (
            ObservationRef::new("cobol.function.accepted", limits)
                .map_err(|error| error.to_string())?,
            &ACCEPTED_OBSERVATION,
        ),
        (
            ObservationRef::new("cobol.function.rejected", limits)
                .map_err(|error| error.to_string())?,
            &REJECTED_OBSERVATION,
        ),
    ])
}

impl ConformancePredicate for FixtureAvailable {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let (id, _) = parse_fixture_ref(fixture)?;
        Ok(fixture_catalog()?
            .fixtures
            .iter()
            .any(|fixture| fixture.id == id))
    }
}

impl ConformanceDriver for FunctionDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let (id, valid) = parse_fixture_ref(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL function fixture {id}"))?;
        let expression = if valid {
            &fixture.valid_expression
        } else {
            &fixture.invalid_expression
        };
        let analysis = CobolCompiler::default().analyze(&bundle(expression)?);
        let mut output = FunctionOutput {
            total: 1,
            accepted: 0,
            target_hits: 0,
            type_hits: 0,
            provenance_hits: 0,
            details: Vec::new(),
        };
        let Some(semantic) = analysis.semantic else {
            output.details.push(
                analysis
                    .diagnostics
                    .first()
                    .map_or_else(|| "rejected".into(), |diagnostic| format!("{diagnostic:?}")),
            );
            return output_bytes(output);
        };
        output.accepted = 1;
        let expected_kind = INTRINSIC_FUNCTIONS
            .iter()
            .find(|descriptor| descriptor.id == fixture.id)
            .map(|descriptor| descriptor.kind)
            .ok_or_else(|| format!("missing function descriptor {}", fixture.id))?;
        let matching = semantic
            .intrinsic_calls
            .iter()
            .find(|call| call.kind == expected_kind);
        output.target_hits = usize::from(matching.is_some());
        output.type_hits = usize::from(matching.is_some_and(|call| {
            value_type_slug(call.result_type) == fixture.result_type
                && call.fixed_length == fixture.fixed_length
        }));
        output.provenance_hits = usize::from(matching.is_some_and(|call| !call.source.is_empty()));
        output.details.push(format!(
            "accepted=true;target={};type={};provenance={}",
            output.target_hits, output.type_hits, output.provenance_hits
        ));
        output_bytes(output)
    }
}

impl ConformanceObservation for AcceptedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        ObservationCheck::new(
            output.accepted == 1
                && output.target_hits == 1
                && output.type_hits == 1
                && output.provenance_hits == 1,
            "valid intrinsic accepted with generated identity, exact result type, and provenance",
            format!(
                "accepted={};target_hits={};type_hits={};provenance_hits={};details={:?}",
                output.accepted,
                output.target_hits,
                output.type_hits,
                output.provenance_hits,
                output.details
            ),
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for RejectedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        ObservationCheck::new(
            output.accepted == 0,
            "invalid intrinsic argument count or class rejected",
            format!("accepted={};details={:?}", output.accepted, output.details),
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL function fixture catalog: {error}"))
}

fn parse_fixture_ref(fixture: &FixtureRef) -> Result<(&str, bool), String> {
    let value = fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign function fixture {fixture}"))?;
    if let Some(id) = value.strip_suffix(".valid") {
        Ok((id, true))
    } else if let Some(id) = value.strip_suffix(".invalid") {
        Ok((id, false))
    } else {
        Err(format!("invalid function fixture reference {fixture}"))
    }
}

fn bundle(expression: &str) -> Result<SourceBundle, String> {
    let source = format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. FUNCTIONS. DATA DIVISION. WORKING-STORAGE SECTION. 01 ALPHA PIC A(8) VALUE 'ABCDEFGH'. 01 TEXT PIC X(32) VALUE '123.45'. 01 TEXT2 PIC X(8) VALUE '$'. 01 DBCS-ITEM PIC G(2) DISPLAY-1. 01 INT PIC S9(9) BINARY VALUE 2. 01 INT2 PIC S9(9) BINARY VALUE 3. 01 NUM PIC S9(9)V99 COMP-3 VALUE 1.5. 01 NUM2 PIC S9(9)V99 COMP-3 VALUE 2.5. 01 NAT PIC N(16) NATIONAL. 01 NAT2 PIC N(16) NATIONAL. 01 UTF PIC U(16) BYTE-LENGTH 64 UTF-8. 01 PTR POINTER. 01 RESULT PIC X(128). PROCEDURE DIVISION. MOVE {expression} TO RESULT. STOP RUN."
    );
    let limits = SourceLimits::default();
    let path = LogicalPath::new("function.cbl", limits.max_path_bytes)
        .map_err(|error| error.to_string())?;
    let file = SourceFile::input(
        path.as_str(),
        source.into_bytes(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| error.to_string())?;
    SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
        .map_err(|error| error.to_string())
}

fn output_bytes(output: FunctionOutput) -> Result<DriverOutput, String> {
    DriverOutput::new(
        serde_json::to_vec(&output).map_err(|error| error.to_string())?,
        ConformanceLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn parse_output(output: &DriverOutput) -> Result<FunctionOutput, String> {
    serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())
}

const fn value_type_slug(value: IntrinsicValueType) -> &'static str {
    match value {
        IntrinsicValueType::Alphabetic => "alphabetic",
        IntrinsicValueType::Alphanumeric => "alphanumeric",
        IntrinsicValueType::Dbcs => "dbcs",
        IntrinsicValueType::Integer => "integer",
        IntrinsicValueType::Numeric => "numeric",
        IntrinsicValueType::National => "national",
        IntrinsicValueType::Utf8 => "utf8",
        IntrinsicValueType::Other => "other",
        IntrinsicValueType::Keyword => "keyword",
    }
}

const fn register_value_type_slug(
    value: mainframe_env_compiler::SpecialRegisterValueType,
) -> &'static str {
    match value {
        mainframe_env_compiler::SpecialRegisterValueType::Alphanumeric => "alphanumeric",
        mainframe_env_compiler::SpecialRegisterValueType::Integer => "integer",
        mainframe_env_compiler::SpecialRegisterValueType::National => "national",
        mainframe_env_compiler::SpecialRegisterValueType::Other => "other",
        mainframe_env_compiler::SpecialRegisterValueType::Group => "group",
    }
}

const fn length_kind_slug(
    value: mainframe_env_compiler::SpecialRegisterLengthKind,
) -> &'static str {
    match value {
        mainframe_env_compiler::SpecialRegisterLengthKind::Fixed => "fixed",
        mainframe_env_compiler::SpecialRegisterLengthKind::Lp => "lp",
        mainframe_env_compiler::SpecialRegisterLengthKind::Dynamic => "dynamic",
        mainframe_env_compiler::SpecialRegisterLengthKind::Dependent => "dependent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_82_functions_accept_valid_and_reject_invalid_signatures() {
        verify_cobol_function_fixtures().unwrap();
        let limits = ConformanceLimits::default();
        for fixture in fixture_catalog().unwrap().fixtures {
            let valid =
                FixtureRef::new(format!("cobol.function.{}.valid", fixture.id), limits).unwrap();
            let valid_output = FUNCTION_DRIVER.execute(&valid).unwrap();
            let valid_check = ACCEPTED_OBSERVATION.evaluate(&valid_output).unwrap();
            assert!(
                valid_check.matched,
                "{}: {}",
                fixture.id, valid_check.actual
            );
            let invalid =
                FixtureRef::new(format!("cobol.function.{}.invalid", fixture.id), limits).unwrap();
            let invalid_output = FUNCTION_DRIVER.execute(&invalid).unwrap();
            let invalid_check = REJECTED_OBSERVATION.evaluate(&invalid_output).unwrap();
            assert!(
                invalid_check.matched,
                "{}: {}",
                fixture.id, invalid_check.actual
            );
        }
    }

    #[test]
    fn special_register_catalog_and_negative_contexts_are_closed() {
        verify_cobol_function_fixtures().unwrap();
        let valid = CobolCompiler::default().analyze(&bundle("FUNCTION LENGTH(TEXT)").unwrap());
        assert!(valid.semantic.is_some());
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REG. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT PIC X. PROCEDURE DIVISION. MOVE 'X' TO WHEN-COMPILED. STOP RUN.";
        let limits = SourceLimits::default();
        let path = LogicalPath::new("register.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            path.as_str(),
            source.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let bundle =
            SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
        assert!(CobolCompiler::default().analyze(&bundle).semantic.is_none());
    }
}
