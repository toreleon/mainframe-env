use mainframe_env_compiler::{CobolCompiler, PROCEDURE_STATEMENTS};
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{
    BoundedPayload, InvocationLimits, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_host_api::{
    DatasetAttributes, DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest,
    EffectResult, HostRequest, HostResult, ProgramRequest, RecordFormat,
};
use mainframe_env_interpreter::{ReferenceMachine, encode_cobol_call_result};
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] = include_bytes!(
    "../../../../conformance/subsystems/cobol/execution/cobol/statement-runtime-fixtures.json"
);
const FIXTURE_PREFIX: &str = "cobol.statement-runtime.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_subsystem: String,
    fixtures: Vec<RuntimeFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFixture {
    id: String,
    row_id: String,
    procedure: String,
    expected_output: String,
    expected_return_code: i32,
    expected_effects: Vec<String>,
    expected_variables: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RuntimeOutput {
    matched: bool,
    expected: String,
    actual: String,
}

struct RuntimeDriver;
struct FixtureAvailable;
struct ExecutedObservation;

static RUNTIME_DRIVER: RuntimeDriver = RuntimeDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static EXECUTED_OBSERVATION: ExecutedObservation = ExecutedObservation;

pub fn verify_cobol_statement_runtime_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-statement-runtime-fixtures@1"
        || catalog.target_subsystem != "cobol.execution"
        || catalog.fixtures.len() != 44
    {
        return Err("COBOL statement runtime fixture identity or denominator drifted".into());
    }
    let official = PROCEDURE_STATEMENTS
        .iter()
        .map(|descriptor| (descriptor.id, descriptor.row_id))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || official.get(fixture.id.as_str()).copied() != Some(fixture.row_id.as_str())
            || fixture.procedure.len() > 8192
            || fixture.expected_output.len() > 8192
            || fixture.expected_effects.len() > 64
            || fixture.expected_variables.len() > 64
        {
            return Err(format!(
                "invalid COBOL statement runtime fixture {}",
                fixture.id
            ));
        }
        crate::compile(&source(fixture)).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.statement-runtime.driver", limits)
            .map_err(|error| error.to_string())?,
        &RUNTIME_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.statement-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.statement-runtime.executed", limits)
            .map_err(|error| error.to_string())?,
        &EXECUTED_OBSERVATION,
    )])
}

impl ConformancePredicate for FixtureAvailable {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let id = fixture_id(fixture)?;
        Ok(fixture_catalog()?
            .fixtures
            .iter()
            .any(|fixture| fixture.id == id))
    }
}

impl ConformanceDriver for RuntimeDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture_id(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL statement runtime fixture {id}"))?;
        let output = execute_fixture(fixture).unwrap_or_else(|error| RuntimeOutput {
            matched: false,
            expected: expected_summary(fixture),
            actual: error,
        });
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for ExecutedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output: RuntimeOutput =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        ObservationCheck::new(
            output.matched,
            output.expected,
            output.actual,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

fn execute_fixture(fixture: &RuntimeFixture) -> Result<RuntimeOutput, String> {
    let source = source(fixture);
    let trace = CobolCompiler::default()
        .analyze(&crate::source_bundle(&source))
        .hir
        .map(|hir| {
            hir.statements
                .iter()
                .map(|statement| format!("{}:{:?}", statement.kind.slug(), statement.arguments))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let artifact = crate::compile(&source)?;
    let mut invocation = crate::invocation(&artifact, 8192);
    for (logical, dataset) in [("TEST-FILE", "USER.TEST"), ("OUT-FILE", "USER.OUTPUT")] {
        invocation.bindings.insert(
            format!("cobol.dd.{logical}"),
            BoundedPayload::new(
                "mainframe-env.dataset-name@1",
                dataset.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|error| format!("{error:?}"))?;
    let mut resume = MachineResume::Start;
    let mut effects = Vec::new();
    let completion = loop {
        match machine.drive(
            resume,
            Quantum::new(512, 64 * 1024).ok_or("invalid fixture quantum")?,
        ) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                effects.push(effect_name(&effect));
                resume = MachineResume::HostResult(effect_result(&effect)?);
            }
            MachineDrive::Completed(completion) => break completion,
            other => {
                return Err(format!(
                    "terminal={other:?};position={}",
                    machine.position_summary()
                ));
            }
        }
    };
    let variables = fixture
        .expected_variables
        .keys()
        .map(|name| {
            let value = machine
                .variable(name)
                .ok_or_else(|| format!("missing variable {name}"))?;
            let expected = &fixture.expected_variables[name];
            let actual = if expected == "nonzero" {
                if value.bytes().iter().any(|byte| *byte != 0) {
                    "nonzero".into()
                } else {
                    "zero".into()
                }
            } else if expected.starts_with("hex:") {
                format!("hex:{}", hex(value.bytes()))
            } else {
                value.text()
            };
            Ok((name.clone(), actual))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let expected = expected_summary(fixture);
    let actual = format!(
        "output={:?};return_code={};effects={effects:?};variables={variables:?};trace={trace:?}",
        String::from_utf8_lossy(completion.output.bytes()),
        completion.return_code,
    );
    Ok(RuntimeOutput {
        matched: completion.output.bytes() == fixture.expected_output.as_bytes()
            && completion.return_code == fixture.expected_return_code
            && effects == fixture.expected_effects
            && variables == fixture.expected_variables,
        expected,
        actual,
    })
}

fn effect_name(effect: &EffectRequest) -> String {
    match &effect.request {
        HostRequest::Terminal(_) => "terminal.read",
        HostRequest::Clock(_) => "clock.read",
        HostRequest::Program(ProgramRequest::Call { .. }) => "program.call",
        HostRequest::Program(ProgramRequest::Invoke { .. }) => "program.invoke",
        HostRequest::Program(ProgramRequest::Cancel { .. }) => "program.cancel",
        HostRequest::Program(_) => "program.other",
        HostRequest::Dataset(DatasetRequest::Attributes { .. }) => "dataset.attributes",
        HostRequest::Dataset(DatasetRequest::Close { .. }) => "dataset.close",
        HostRequest::Dataset(DatasetRequest::StartBrowse { .. }) => "dataset.start-browse",
        HostRequest::Dataset(DatasetRequest::Read { .. }) => "dataset.read",
        HostRequest::Dataset(DatasetRequest::ReadNext { .. }) => "dataset.read-next",
        HostRequest::Dataset(DatasetRequest::DeleteRecord { .. }) => "dataset.delete-record",
        HostRequest::Dataset(DatasetRequest::RewriteRecord { .. }) => "dataset.rewrite-record",
        HostRequest::Dataset(DatasetRequest::Write { .. }) => "dataset.write",
        HostRequest::Dataset(_) => "dataset.other",
        _ => "host.other",
    }
    .into()
}

pub(crate) fn effect_result(effect: &EffectRequest) -> Result<EffectResult, String> {
    let outcome = match &effect.request {
        HostRequest::Terminal(_) => HostResult::Terminal(
            BoundedPayload::new(
                "mainframe-env.terminal.input@1",
                b"INPUT".to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|error| error.to_string())?,
        ),
        HostRequest::Clock(_) => HostResult::Clock("20240229123456789".into()),
        HostRequest::Program(ProgramRequest::Call { .. } | ProgramRequest::Invoke { .. }) => {
            HostResult::Program(
                encode_cobol_call_result(&[]).map_err(|error| format!("{error:?}"))?,
            )
        }
        HostRequest::Program(ProgramRequest::Cancel { .. }) => HostResult::Program(
            BoundedPayload::new(
                "mainframe-env.program.cancel@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .map_err(|error| error.to_string())?,
        ),
        HostRequest::Dataset(DatasetRequest::StartBrowse { .. }) => {
            HostResult::Dataset(DatasetResult::Browse {
                cursor: "cursor-1".into(),
                record: None,
                identity: None,
                key: None,
            })
        }
        HostRequest::Dataset(DatasetRequest::EndBrowse { .. }) => {
            HostResult::Dataset(DatasetResult::Browse {
                cursor: "cursor-1".into(),
                record: None,
                identity: None,
                key: None,
            })
        }
        HostRequest::Dataset(DatasetRequest::Close { cursor, .. }) => {
            HostResult::Dataset(DatasetResult::Browse {
                cursor: cursor.clone().unwrap_or_else(|| "closed".into()),
                record: None,
                identity: None,
                key: None,
            })
        }
        HostRequest::Dataset(DatasetRequest::Read { .. }) => {
            HostResult::Dataset(DatasetResult::Records {
                records: vec![b"RECORD  ".to_vec(), b"AA01    ".to_vec()],
                identities: vec![b"RE".to_vec(), b"AA".to_vec()],
                version: 1,
            })
        }
        HostRequest::Dataset(DatasetRequest::ReadNext { .. }) => {
            HostResult::Dataset(DatasetResult::Browse {
                cursor: "cursor-1".into(),
                record: Some(b"RECORD  ".to_vec()),
                identity: Some(b"RE".to_vec()),
                key: Some(b"RE".to_vec()),
            })
        }
        HostRequest::Dataset(DatasetRequest::Attributes { .. }) => {
            HostResult::Dataset(DatasetResult::Attributes {
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::KeySequenced,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 8,
                    key_offset: Some(0),
                    key_length: Some(2),
                    ccsid: None,
                },
                version: 1,
            })
        }
        HostRequest::Dataset(_) => HostResult::Dataset(DatasetResult::Mutated { version: 2 }),
        other => return Err(format!("fixture has no host response for {other:?}")),
    };
    Ok(EffectResult {
        sequence: effect.sequence,
        outcome: Ok(outcome),
    })
}

fn source(fixture: &RuntimeFixture) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. RUNTIME. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS DYNAMIC RECORD KEY IS REC-KEY. SELECT OUT-FILE ASSIGN TO OUTDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-RECORD. 05 REC-KEY PIC X(2) VALUE 'AA'. 05 REC-DATA PIC X(6). FD OUT-FILE. 01 OUT-RECORD PIC X(8). SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(6). WORKING-STORAGE SECTION. 01 A PIC 9(3) VALUE 1. 01 B PIC 9(3) VALUE 2. 01 C PIC X(256). 01 TEXT-X PIC X(8) VALUE 'VALUE'. 01 TEXT-Y PIC X(64). 01 JSON-X PIC X(32) VALUE '{{\"TEXT-Y\":\"VALUE\"}}'. 01 XML-X PIC X(32) VALUE '<TEXT-X>VALUE</TEXT-X>'. 01 PTR POINTER. 01 RECEIVER OBJECT REFERENCE CUSTOMER. 01 TABLE-GROUP. 05 TABLE-ITEM PIC X OCCURS 2 TIMES VALUE 'A'. PROCEDURE DIVISION. {}",
        fixture.procedure
    )
}

pub(super) fn assurance_sources() -> Result<Vec<(String, String, String)>, String> {
    Ok(fixture_catalog()?
        .fixtures
        .iter()
        .map(|fixture| {
            (
                format!("cobol.statement-runtime.{}", fixture.id),
                fixture.row_id.clone(),
                source(fixture),
            )
        })
        .collect())
}

fn expected_summary(fixture: &RuntimeFixture) -> String {
    format!(
        "output={:?};return_code={};effects={:?};variables={:?}",
        fixture.expected_output,
        fixture.expected_return_code,
        fixture.expected_effects,
        fixture.expected_variables
    )
}

fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL statement runtime fixture catalog: {error}"))
}

fn fixture_id(fixture: &FixtureRef) -> Result<&str, String> {
    fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign COBOL statement runtime fixture {fixture}"))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)] as char,
                DIGITS[usize::from(byte & 0x0f)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute_without_effects(source: &str) -> Result<(Vec<u8>, ReferenceMachine), String> {
        let artifact = crate::compile(source)?;
        retain_execution_bytes(source, "cbl", source.as_bytes())?;
        retain_execution_bytes(source, "bin", artifact.payload())?;
        let invocation = crate::invocation(&artifact, 4096);
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .map_err(|error| format!("{error:?}"))?;
        let mut resume = MachineResume::Start;
        let output = loop {
            match machine.drive(
                resume,
                Quantum::new(512, 64 * 1024).ok_or("invalid test quantum")?,
            ) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::Completed(completion) => break completion.output.bytes().to_vec(),
                other => {
                    return Err(format!(
                        "terminal={other:?};position={}",
                        machine.position_summary()
                    ));
                }
            }
        };
        retain_execution_bytes(source, "output", &output)?;
        Ok((output, machine))
    }

    fn retain_execution_bytes(source: &str, extension: &str, bytes: &[u8]) -> Result<(), String> {
        let Some(directory) = std::env::var_os("MAINFRAME_COBOL_TEST_RECEIPTS") else {
            return Ok(());
        };
        let name = source
            .split_whitespace()
            .skip_while(|token| *token != "PROGRAM-ID.")
            .nth(1)
            .ok_or("test source has no program identity")?
            .trim_end_matches('.');
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{name}.{extension}")),
            bytes,
        )
        .map_err(|error| format!("retain test execution bytes: {error}"))
    }

    const STRING_REFERENCE_MENU_DATA: &str = r#"
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-IDX PIC S9(4) COMP VALUE 2.
01 WS-TEXT PIC X(40) VALUE SPACES.
01 MENU-ROOT.
   05 MENU-DATA.
      10 FILLER PIC 9(2) VALUE 1.
      10 FILLER PIC X(35) VALUE 'Account View'.
      10 FILLER PIC X(8) VALUE 'COACTVWC'.
      10 FILLER PIC X VALUE 'U'.
      10 FILLER PIC 9(2) VALUE 6.
      10 FILLER PIC X(35) VALUE 'Transaction List'.
      10 FILLER PIC X(8) VALUE 'COTRN00C'.
      10 FILLER PIC X VALUE 'U'.
   05 MENU-TABLE REDEFINES MENU-DATA.
      10 MENU-ENTRY OCCURS 2 TIMES.
         15 MENU-NUM PIC 9(2).
         15 MENU-NAME PIC X(35).
         15 MENU-PGM PIC X(8).
         15 MENU-USER PIC X.
"#;

    #[test]
    fn string_reference_occurs_sender() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRING-REFERENCE-OCCURS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-IDX PIC S9(4) COMP VALUE 2.
01 WS-TEXT PIC X(8) VALUE '--------'.
01 TABLE-ROOT.
   05 ITEM-X PIC X(3) OCCURS 2 TIMES.
PROCEDURE DIVISION.
    MOVE 'ONE' TO ITEM-X(1).
    MOVE 'TWO' TO ITEM-X(2).
    STRING ITEM-X(WS-IDX) DELIMITED BY SIZE
           '!' DELIMITED BY SIZE INTO WS-TEXT END-STRING.
    DISPLAY WS-TEXT.
    STOP RUN.
"#;
        let (output, machine) = execute_without_effects(source).expect("OCCURS sender runtime");
        assert_eq!(output, b"TWO!----\n");
        assert_eq!(machine.variable("WS-TEXT").unwrap().bytes(), b"TWO!----");
        assert_eq!(machine.variable("TABLE-ROOT").unwrap().bytes(), b"ONETWO");
    }

    #[test]
    fn string_reference_redefines_menu_sender() {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. STRING-REFERENCE-MENU. {STRING_REFERENCE_MENU_DATA}\n\
             PROCEDURE DIVISION.\n\
             STRING MENU-NUM(WS-IDX) DELIMITED BY SIZE\n\
                    '. ' DELIMITED BY SIZE\n\
                    MENU-NAME(WS-IDX) DELIMITED BY SIZE\n\
                 INTO WS-TEXT END-STRING.\n\
             DISPLAY WS-TEXT. STOP RUN."
        );
        let (output, machine) = execute_without_effects(&source).expect("menu sender runtime");
        assert_eq!(output, b"06. Transaction List                    \n");
        assert_eq!(
            machine.variable("WS-TEXT").unwrap().bytes(),
            b"06. Transaction List                    "
        );
    }

    #[test]
    fn string_reference_shared_redefines_baseline() {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. STRING-REFERENCE-BASELINE. {STRING_REFERENCE_MENU_DATA}\n\
             PROCEDURE DIVISION.\n\
             DISPLAY MENU-NUM(WS-IDX).\n\
             DISPLAY MENU-NAME(WS-IDX).\n\
             DISPLAY MENU-PGM(WS-IDX).\n\
             DISPLAY MENU-USER(WS-IDX). STOP RUN."
        );
        let (output, machine) =
            execute_without_effects(&source).expect("shared reference baseline");
        assert_eq!(
            output,
            b"06\nTransaction List                   \nCOTRN00C\nU\n"
        );
        assert_eq!(
            machine.variable("WS-TEXT").unwrap().bytes(),
            b"                                        "
        );
        assert_eq!(
            machine.variable("MENU-DATA").unwrap().bytes(),
            b"01Account View                       COACTVWCU06Transaction List                   COTRN00CU"
        );
    }

    #[test]
    fn string_reference_qualified_modified_sender() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRING-REFERENCE-QUALIFIED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 FIRST-GROUP.
   05 LEAF-X PIC X(6) VALUE 'ABCDEF'.
01 SECOND-GROUP.
   05 LEAF-X PIC X(6) VALUE 'uvwxyz'.
01 WS-TEXT PIC X(10) VALUE '----------'.
PROCEDURE DIVISION.
    STRING LEAF-X OF SECOND-GROUP(2:3) DELIMITED BY SIZE
           '(Q)' DELIMITED BY SIZE INTO WS-TEXT END-STRING.
    DISPLAY WS-TEXT.
    STOP RUN.
"#;
        let (output, machine) = execute_without_effects(source).expect("qualified modified sender");
        assert_eq!(output, b"vwx(Q)----\n");
        assert_eq!(machine.variable("WS-TEXT").unwrap().bytes(), b"vwx(Q)----");
        assert_eq!(machine.variable("FIRST-GROUP").unwrap().bytes(), b"ABCDEF");
        assert_eq!(machine.variable("SECOND-GROUP").unwrap().bytes(), b"uvwxyz");
    }

    #[test]
    fn string_reference_indexed_delimiter() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRING-REFERENCE-DELIMITER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SOURCE-X PIC X(5) VALUE 'AA#BB'.
01 WS-IDX PIC S9(4) COMP VALUE 2.
01 WS-TEXT PIC X(8) VALUE '--------'.
01 DELIMITER-ROOT.
   05 DELIMITER-X PIC X OCCURS 2 TIMES.
PROCEDURE DIVISION.
    MOVE '!' TO DELIMITER-X(1).
    MOVE '#' TO DELIMITER-X(2).
    STRING SOURCE-X DELIMITED BY DELIMITER-X(WS-IDX)
        INTO WS-TEXT END-STRING.
    DISPLAY WS-TEXT.
    STOP RUN.
"#;
        let (output, machine) = execute_without_effects(source).expect("indexed delimiter runtime");
        assert_eq!(output, b"AA------\n");
        assert_eq!(machine.variable("WS-TEXT").unwrap().bytes(), b"AA------");
        assert_eq!(machine.variable("SOURCE-X").unwrap().bytes(), b"AA#BB");
        assert_eq!(machine.variable("DELIMITER-ROOT").unwrap().bytes(), b"!#");
    }

    #[test]
    fn string_reference_invalid_subscript() {
        let mut observations = Vec::new();
        for index in [0, 3] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. STRING-REFERENCE-INVALID-{index}.\n\
                 DATA DIVISION. WORKING-STORAGE SECTION.\n\
                 01 WS-IDX PIC S9(4) COMP VALUE {index}.\n\
                 01 WS-TEXT PIC X(8) VALUE '--------'.\n\
                 01 TABLE-ROOT. 05 ITEM-X PIC X(3) OCCURS 2 TIMES.\n\
                 PROCEDURE DIVISION.\n\
                 MOVE 'ONE' TO ITEM-X(1). MOVE 'TWO' TO ITEM-X(2).\n\
                 STRING ITEM-X(WS-IDX) DELIMITED BY SIZE INTO WS-TEXT END-STRING.\n\
                 STOP RUN."
            );
            let artifact = crate::compile(&source).expect("invalid-subscript source compiles");
            retain_execution_bytes(&source, "cbl", source.as_bytes()).unwrap();
            retain_execution_bytes(&source, "bin", artifact.payload()).unwrap();
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                crate::invocation(&artifact, 4096),
                CodecLimits::default(),
            )
            .unwrap();
            let terminal = loop {
                match machine.drive(MachineResume::Start, Quantum::new(512, 64 * 1024).unwrap()) {
                    MachineDrive::Continue => {}
                    terminal => break terminal,
                }
            };
            let target = machine.variable("WS-TEXT").unwrap().bytes().to_vec();
            let table = machine.variable("TABLE-ROOT").unwrap().bytes().to_vec();
            retain_execution_bytes(&source, "target", &target).unwrap();
            eprintln!("index={index}; terminal={terminal:?}; target={target:?}; table={table:?}");
            observations.push((index, terminal, target, table));
        }
        for (index, terminal, target, table) in observations {
            assert_eq!(
                target, b"--------",
                "index {index} must not mutate the target"
            );
            assert_eq!(table, b"ONETWO");
            let MachineDrive::Condition(condition) = terminal else {
                panic!("index {index} expected checked SubscriptError, got {terminal:?}");
            };
            assert_eq!(condition.name, "SUBSCRIPT-ERROR");
            assert_eq!(condition.response, 3);
            assert!(!condition.handled);
        }
    }

    #[test]
    fn string_reference_literal_pointer_overflow() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRING-REFERENCE-OVERFLOW.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(5) VALUE '-----'.
01 PTR-X PIC 9 VALUE 3.
PROCEDURE DIVISION.
    STRING 'A(B)' DELIMITED BY SIZE INTO WS-TEXT WITH POINTER PTR-X
        ON OVERFLOW DISPLAY 'OVERFLOW' END-STRING.
    DISPLAY WS-TEXT.
    DISPLAY PTR-X.
    STOP RUN.
"#;
        let (output, machine) = execute_without_effects(source).expect("literal pointer overflow");
        assert_eq!(output, b"OVERFLOW\n--A(B\n6\n");
        assert_eq!(machine.variable("WS-TEXT").unwrap().bytes(), b"--A(B");
        assert_eq!(machine.variable("PTR-X").unwrap().bytes(), b"6");
    }

    #[test]
    fn all_44_statement_runtime_fixtures_execute_exactly() {
        verify_cobol_statement_runtime_fixtures().unwrap();
        for fixture in fixture_catalog().unwrap().fixtures {
            let output = execute_fixture(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }

    #[test]
    fn bounded_dynamic_length_grows_and_length_of_observes_the_live_extent() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DYNAMIC-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 8. 01 LEN-X PIC 9. PROCEDURE DIVISION. MOVE 'HELLO' TO DYN-X. COMPUTE LEN-X = LENGTH OF DYN-X. DISPLAY DYN-X. DISPLAY LEN-X. STOP RUN.";
        let (output, machine) = execute_without_effects(source).expect("dynamic runtime");
        assert_eq!(output, b"HELLO\n5\n");
        assert_eq!(machine.variable("DYN-X").unwrap().bytes(), b"HELLO");
    }

    #[test]
    fn comp1_arithmetic_round_trips_through_a_display_receiver() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FLOAT-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLOAT-X COMP-1. 01 OUT-X PIC 9V9. PROCEDURE DIVISION. MOVE 1.5 TO FLOAT-X. ADD 0.5 TO FLOAT-X. MOVE FLOAT-X TO OUT-X. DISPLAY OUT-X. STOP RUN.";
        let (output, _) = execute_without_effects(source).expect("float runtime");
        assert_eq!(output, b"20\n");
    }

    #[test]
    fn arith_compat_changes_intermediate_precision_through_the_artifact_contract() {
        let body = "IDENTIFICATION DIVISION. PROGRAM-ID. ARITH-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 A-X PIC 9(20) VALUE 99999999999999999999. 01 B-X PIC 9(20) VALUE 1. 01 RESULT-X PIC 9(21). PROCEDURE DIVISION. COMPUTE RESULT-X = A-X + B-X. DISPLAY RESULT-X. STOP RUN.";
        let (extended, _) = execute_without_effects(body).expect("ARITH(EXTEND) runtime");
        let (compatible, _) = execute_without_effects(&format!("PROCESS ARITH(COMPAT)\n{body}"))
            .expect("ARITH(COMPAT) runtime");
        assert_eq!(extended, b"100000000000000000000\n");
        assert_eq!(compatible, b"099999999999999999900\n");
    }

    #[test]
    fn search_advances_the_owned_index_until_a_when_condition_matches() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SEARCH-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT-X. 05 TABLE-X OCCURS 3 TIMES INDEXED BY IDX. 10 VALUE-X PIC X. PROCEDURE DIVISION. MOVE 'A' TO VALUE-X(1). MOVE 'B' TO VALUE-X(2). MOVE 'C' TO VALUE-X(3). SEARCH TABLE-X AT END DISPLAY 'MISS' WHEN VALUE-X(IDX) = 'B' DISPLAY IDX END-SEARCH. STOP RUN.";
        let (output, _) = execute_without_effects(source).expect("search runtime");
        assert_eq!(output, b"2\n");
    }

    #[test]
    fn string_and_unstring_update_pointer_and_tally_without_overwriting_prefixes() {
        let string_source = "IDENTIFICATION DIVISION. PROGRAM-ID. STRING-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 TARGET-X PIC X(8) VALUE '--------'. 01 PTR-X PIC 9 VALUE 3. PROCEDURE DIVISION. STRING 'AB' DELIMITED BY SIZE INTO TARGET-X WITH POINTER PTR-X END-STRING. DISPLAY TARGET-X. DISPLAY PTR-X. STOP RUN.";
        let (output, _) = execute_without_effects(string_source).expect("string pointer runtime");
        assert_eq!(output, b"--AB----\n5\n");

        let unstring_source = "IDENTIFICATION DIVISION. PROGRAM-ID. UNSTRING-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-X PIC X(5) VALUE 'A,B,C'. 01 A-X PIC X(4). 01 B-X PIC X(4). 01 C-X PIC X(4). 01 PTR-X PIC 9 VALUE 1. 01 TALLY-X PIC 9 VALUE 0. PROCEDURE DIVISION. UNSTRING SOURCE-X DELIMITED BY ',' INTO A-X B-X C-X WITH POINTER PTR-X TALLYING IN TALLY-X END-UNSTRING. DISPLAY A-X B-X C-X. DISPLAY TALLY-X. STOP RUN.";
        let (output, _) =
            execute_without_effects(unstring_source).expect("unstring pointer runtime");
        assert_eq!(output, b"A   B   C   \n3\n");
    }

    #[test]
    fn call_uses_an_explicit_versioned_le_selector_without_program_name_dispatch() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LE-RUNTIME. PROCEDURE DIVISION. CALL 'LEFUNC'. STOP RUN.";
        let artifact = crate::compile(source).expect("LE fixture compiles");
        let mut invocation = crate::invocation(&artifact, 4096);
        invocation.bindings.insert(
            "cobol.runtime-service.LEFUNC".into(),
            BoundedPayload::new(
                "mainframe-env.runtime-service-selector@1",
                b"le:CEE-DATE:1".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let effect = loop {
            match machine.drive(
                MachineResume::Start,
                Quantum::new(64, 4096).expect("quantum"),
            ) {
                MachineDrive::Continue => {}
                MachineDrive::HostCall(effect) => break effect,
                other => panic!("unexpected {other:?}"),
            }
        };
        let HostRequest::Program(ProgramRequest::Call {
            program, service, ..
        }) = effect.request
        else {
            panic!("typed program call expected");
        };
        let service = service.expect("typed LE selector");
        assert_eq!(program.as_str(), "LEFUNC");
        assert_eq!(
            service.kind,
            mainframe_env_host_api::RuntimeServiceKind::LanguageEnvironment
        );
        assert_eq!(service.name.as_str(), "CEE-DATE");
        assert_eq!(service.abi_version, 1);
    }

    #[test]
    fn unbounded_occurs_uses_a_bounded_backing_store_and_live_depending_count() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. UNBOUNDED-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 COUNT-X PIC 9 VALUE 3. 01 ROOT-X. 05 TABLE-X OCCURS 1 TO UNBOUNDED TIMES DEPENDING ON COUNT-X INDEXED BY IDX. 10 VALUE-X PIC X. PROCEDURE DIVISION. MOVE 'C' TO VALUE-X(3). DISPLAY VALUE-X(3). STOP RUN.";
        let (output, machine) = execute_without_effects(source).expect("unbounded table runtime");
        assert_eq!(output, b"C\n");
        assert_eq!(machine.variable("TABLE-X").unwrap().bytes()[2], b'C');

        let out_of_active_range = "IDENTIFICATION DIVISION. PROGRAM-ID. UNBOUNDED-BOUND. DATA DIVISION. WORKING-STORAGE SECTION. 01 COUNT-X PIC 9 VALUE 3. 01 ROOT-X. 05 TABLE-X OCCURS 1 TO UNBOUNDED TIMES DEPENDING ON COUNT-X. 10 VALUE-X PIC X. PROCEDURE DIVISION. MOVE 'D' TO VALUE-X(4). STOP RUN.";
        let artifact =
            crate::compile(out_of_active_range).expect("bounded unbounded table compiles");
        match crate::execute(&artifact, 1024) {
            MachineDrive::Condition(condition) => assert_eq!(condition.name, "SUBSCRIPT-ERROR"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn qualified_occurs_dependency_selects_the_resolved_owner_relative_integer() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. QUALIFIED-ODO. DATA DIVISION. WORKING-STORAGE SECTION. 01 GROUP-A. 05 N PIC 9 VALUE 1. 01 GROUP-B. 05 N PIC 9 VALUE 2. 05 TABLE-B OCCURS 1 TO 3 TIMES DEPENDING ON N OF GROUP-B. 10 VALUE-B PIC X. PROCEDURE DIVISION. MOVE 'Z' TO VALUE-B(2). DISPLAY VALUE-B(2). STOP RUN.";
        let (output, machine) = execute_without_effects(source).expect("qualified ODO runtime");
        assert_eq!(output, b"Z\n");
        assert_eq!(machine.variable("TABLE-B").unwrap().bytes()[1], b'Z');
    }

    #[test]
    fn reference_modification_and_ebcdic_collation_are_byte_exact() {
        let reference_source = "IDENTIFICATION DIVISION. PROGRAM-ID. REFMOD. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X(5) VALUE 'ABCDE'. 01 OUT-X PIC X(3). PROCEDURE DIVISION. MOVE TEXT-X(2:3) TO OUT-X. MOVE 'Z' TO TEXT-X(3:1). DISPLAY OUT-X TEXT-X. STOP RUN.";
        let (output, _) =
            execute_without_effects(reference_source).expect("reference modification");
        assert_eq!(output, b"BCDABZDE\n");

        let collation_source = "IDENTIFICATION DIVISION. PROGRAM-ID. COLLATE. PROCEDURE DIVISION. IF 'A' > 'a' DISPLAY 'EBCDIC' ELSE DISPLAY 'BAD' END-IF. STOP RUN.";
        let (output, _) = execute_without_effects(collation_source).expect("CP037 collation");
        assert_eq!(output, b"EBCDIC\n");
    }
}
