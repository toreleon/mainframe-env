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

    // Controlled first-party replies exercise response ownership, not a provider or IBM oracle.
    fn check_cics_bms_reply(
        source: &str,
        operation: mainframe_env_host_api::CicsOperation,
        frame: &[u8],
        named: &[(&str, &str, &[u8])],
        fields: &[(&str, &[u8], &[u8])],
        refused: bool,
    ) {
        use mainframe_env_host_api::{CicsDisposition, CicsResponse};

        retain_execution_bytes(source, "cbl", source.as_bytes()).unwrap();
        retain_execution_bytes(source, "reply-payload", frame).unwrap();
        let reply = serde_json::json!({
            "operation": format!("{operation:?}"),
            "disposition": "Complete", "condition": "NORMAL", "response": 0, "response2": 0,
            "applid": "", "sysid": "", "transaction": "", "aid": 0,
            "target": null, "next_transaction": null, "unit_of_work": null,
            "payload_schema": "mainframe-env.cics.payload@1", "payload_bytes": frame,
            "outputs": named.iter().map(|(name, schema, bytes)| {
                serde_json::json!({"name": name, "schema": schema, "bytes": bytes})
            }).collect::<Vec<_>>(),
        });
        retain_execution_bytes(
            source,
            "reply.json",
            &serde_json::to_vec_pretty(&reply).unwrap(),
        )
        .unwrap();
        for (name, before, after) in fields {
            retain_execution_bytes(source, &format!("{name}.before-expected"), before).unwrap();
            retain_execution_bytes(source, &format!("{name}.expected"), after).unwrap();
        }
        retain_execution_bytes(
            source,
            "expected-terminal",
            if refused {
                b"Failed"
            } else {
                b"Completed:0:DONE\n"
            },
        )
        .unwrap();
        let artifact = crate::compile(source).expect("controlled CICS source compiles");
        retain_execution_bytes(source, "bin", artifact.payload()).unwrap();
        let invocation = crate::invocation(&artifact, 4096);
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .expect("controlled CICS machine");
        let quantum = Quantum::new(512, 64 * 1024).unwrap();
        let mut effect = None;
        for _ in 0..32 {
            match machine.drive(MachineResume::Start, quantum) {
                MachineDrive::Continue => {}
                MachineDrive::HostCall(request) => {
                    effect = Some(request);
                    break;
                }
                other => panic!("expected one typed CICS request, got {other:?}"),
            }
        }
        let effect = effect.expect("bounded source reaches a host request");
        retain_execution_bytes(source, "request", format!("{effect:?}").as_bytes()).unwrap();
        let HostRequest::Cics(request) = &effect.request else {
            panic!("expected typed CICS host request");
        };
        assert_eq!(request.operation, operation);
        let initial = fields
            .iter()
            .map(|(name, before, _)| {
                let actual = machine
                    .variable(name)
                    .expect("initial storage")
                    .bytes()
                    .to_vec();
                retain_execution_bytes(source, &format!("{name}.before-actual"), &actual).unwrap();
                (name, before, actual)
            })
            .collect::<Vec<_>>();
        for (name, expected, actual) in initial {
            assert_eq!(actual, *expected, "independent initial storage {name}");
        }
        let payload = |schema: &str, bytes: &[u8]| {
            BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
        };
        let response = CicsResponse {
            disposition: CicsDisposition::Complete,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            applid: String::new(),
            sysid: String::new(),
            transaction: String::new(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: payload("mainframe-env.cics.payload@1", frame),
            outputs: named
                .iter()
                .map(|(name, schema, bytes)| ((*name).into(), payload(schema, bytes)))
                .collect(),
            unit_of_work: None,
        };
        let mut terminal = machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            }),
            quantum,
        );
        for _ in 0..32 {
            if !matches!(terminal, MachineDrive::Continue) {
                break;
            }
            terminal = machine.drive(MachineResume::Start, quantum);
        }
        retain_execution_bytes(source, "terminal", format!("{terminal:?}").as_bytes()).unwrap();
        let observations = fields
            .iter()
            .map(|(name, _, expected)| {
                let actual = machine
                    .variable(name)
                    .expect("observed storage")
                    .bytes()
                    .to_vec();
                retain_execution_bytes(source, &format!("{name}.actual"), &actual).unwrap();
                eprintln!("field={name}; expected={expected:?}; actual={actual:?}");
                (name, expected, actual)
            })
            .collect::<Vec<_>>();
        eprintln!("refused={refused}; terminal={terminal:?}");
        for (name, expected, actual) in observations {
            assert_eq!(actual, *expected, "literal response-owned storage {name}");
        }
        if refused {
            assert!(matches!(terminal, MachineDrive::Failed(_)), "{terminal:?}");
        } else {
            let MachineDrive::Completed(completion) = terminal else {
                panic!("expected completion, got {terminal:?}");
            };
            assert_eq!(completion.return_code, 0);
            assert_eq!(completion.output.bytes(), b"DONE\n");
        }
    }

    const BMS_INPUT_STORAGE_DATA: &str = r#"
DATA DIVISION. WORKING-STORAGE SECTION.
01 LEFT-EDGE PIC X(4) VALUE 'LEFT'.
01 INPUT-MAP.
   05 GUARD-L PIC X(2) VALUE '<<'.
   05 ALPHAL PIC S9(4) COMP VALUE 258.
   05 ALPHAF PIC X VALUE 'a'.
   05 ALPHAI PIC X(4) VALUE 'old!'.
   05 GUARD-M PIC X(2) VALUE '||'.
   05 BETAL PIC S9(4) COMP VALUE 772.
   05 BETAF PIC X VALUE 'b'.
   05 BETAI PIC X(5) VALUE 'stay?'.
   05 GUARD-R PIC X(2) VALUE '>>'.
01 RIGHT-EDGE PIC X(4) VALUE 'RITE'.
"#;

    #[test]
    fn cics_bms_input_storage_present_fields_exclude_transport_frame() {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-PRESENT. {BMS_INPUT_STORAGE_DATA}\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        check_cics_bms_reply(
            &source,
            mainframe_env_host_api::CicsOperation::ReceiveMap,
            b"FRAME!TITLE-NAME!\x00\x00\x00\x09TRANSPORT",
            &[
                ("BMS.ALPHA", "mainframe-env.cics.payload@1", b"HI"),
                ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"2"),
                ("BMS.BETA", "mainframe-env.cics.payload@1", b"XYZ"),
                ("BMS.BETA.LENGTH", "mainframe-env.cics.decimal@1", b"3"),
            ],
            &[
                ("LEFT-EDGE", b"LEFT", b"LEFT"),
                (
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"<<\x00\x02aHI  ||\x00\x03bXYZ  >>",
                ),
                ("RIGHT-EDGE", b"RITE", b"RITE"),
            ],
            false,
        );
    }

    #[test]
    fn cics_bms_input_storage_present_empty_differs_from_omitted() {
        let empty = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-EMPTY. {BMS_INPUT_STORAGE_DATA}\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        let empty_result = std::panic::catch_unwind(|| {
            check_cics_bms_reply(
                &empty,
                mainframe_env_host_api::CicsOperation::ReceiveMap,
                b"EMPTY-FRAME!",
                &[
                    ("BMS.ALPHA", "mainframe-env.cics.payload@1", b""),
                    ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"0"),
                ],
                &[(
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"<<\x00\x00a    ||\x03\x04bstay?>>",
                )],
                false,
            );
        });
        let omitted = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-OMITTED. {BMS_INPUT_STORAGE_DATA}\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        let omitted_result = std::panic::catch_unwind(|| {
            check_cics_bms_reply(
                &omitted,
                mainframe_env_host_api::CicsOperation::ReceiveMap,
                b"OMITTED-FRAME!",
                &[],
                &[(
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                )],
                false,
            );
        });
        assert!(
            empty_result.is_ok() && omitted_result.is_ok(),
            "empty/omitted controls"
        );
    }

    #[test]
    fn cics_bms_input_storage_absent_zero_and_sentinel_bits_survive() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. BMS-ABSENT.
DATA DIVISION. WORKING-STORAGE SECTION.
01 INPUT-MAP.
   05 ALPHAL PIC S9(4) COMP VALUE 258.
   05 ALPHAF PIC X VALUE 'a'.
   05 ALPHAI PIC X(4) VALUE 'old!'.
   05 ZEROL PIC S9(4) COMP.
   05 ZEROF PIC X.
   05 ZEROI PIC X(4).
   05 SENTL PIC S9(4) COMP VALUE 1286.
   05 SENTF PIC X VALUE '!'.
   05 SENTI PIC X(4) VALUE 'KEEP'.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO ZEROL ZEROF ZEROI.
    EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.
    DISPLAY 'DONE'. STOP RUN.
"#;
        check_cics_bms_reply(
            source,
            mainframe_env_host_api::CicsOperation::ReceiveMap,
            b"ABSENT!DESCRIPTOR!\x00\x00\x00\x06TITLE!",
            &[
                ("BMS.ALPHA", "mainframe-env.cics.payload@1", b"NEW!"),
                ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"4"),
            ],
            &[(
                "INPUT-MAP",
                b"\x01\x02aold!\x00\x00\x00\x00\x00\x00\x00\x05\x06!KEEP",
                b"\x00\x04aNEW!\x00\x00\x00\x00\x00\x00\x00\x05\x06!KEEP",
            )],
            false,
        );
    }

    #[test]
    fn cics_bms_input_storage_duplicate_names_stay_in_selected_into_group() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. BMS-QUALIFIED.
DATA DIVISION. WORKING-STORAGE SECTION.
01 OTHER-MAP.
   05 ALPHAL PIC S9(4) COMP VALUE 258.
   05 ALPHAF PIC X VALUE 'x'.
   05 ALPHAI PIC X(4) VALUE 'KEEP'.
01 SELECTED-MAP.
   05 ALPHAL PIC S9(4) COMP VALUE 772.
   05 ALPHAF PIC X VALUE 'y'.
   05 ALPHAI PIC X(4) VALUE 'old!'.
01 ADJACENT-X PIC X(4) VALUE 'EDGE'.
PROCEDURE DIVISION.
    EXEC CICS RECEIVE MAP('SYNMAP') INTO(SELECTED-MAP) END-EXEC.
    DISPLAY 'DONE'. STOP RUN.
"#;
        check_cics_bms_reply(
            source,
            mainframe_env_host_api::CicsOperation::ReceiveMap,
            b"QUALIFIED-FRAME!",
            &[
                ("BMS.ALPHA", "mainframe-env.cics.payload@1", b"OK"),
                ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"2"),
            ],
            &[
                ("OTHER-MAP", b"\x01\x02xKEEP", b"\x01\x02xKEEP"),
                ("SELECTED-MAP", b"\x03\x04yold!", b"\x00\x02yOK  "),
                ("ADJACENT-X", b"EDGE", b"EDGE"),
            ],
            false,
        );
    }

    #[test]
    fn cics_bms_input_storage_non_symbolic_receive_keeps_raw_payload() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. BMS-RAW.
DATA DIVISION. WORKING-STORAGE SECTION.
01 RAW-BUFFER PIC X(8) VALUE 'oldbytes'.
01 ADJACENT-X PIC X(4) VALUE 'EDGE'.
PROCEDURE DIVISION.
    EXEC CICS RECEIVE MAP('SYNMAP') INTO(RAW-BUFFER) END-EXEC.
    DISPLAY 'DONE'. STOP RUN.
"#;
        check_cics_bms_reply(
            source,
            mainframe_env_host_api::CicsOperation::ReceiveMap,
            b"RAW12345EXTRA",
            &[],
            &[
                ("RAW-BUFFER", b"oldbytes", b"RAW12345"),
                ("ADJACENT-X", b"EDGE", b"EDGE"),
            ],
            false,
        );
    }

    #[test]
    fn cics_bms_input_storage_non_bms_into_keeps_raw_payload() {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-NONMAP. {BMS_INPUT_STORAGE_DATA}\n\
             01 WS-LENGTH PIC S9(4) COMP VALUE 21.\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS READQ TD QUEUE('SYNQ') INTO(INPUT-MAP) LENGTH(WS-LENGTH) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        check_cics_bms_reply(
            &source,
            mainframe_env_host_api::CicsOperation::ReadTransientData,
            b"NONMAP-RAW-IMAGE-1234",
            &[],
            &[
                ("LEFT-EDGE", b"LEFT", b"LEFT"),
                (
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"NONMAP-RAW-IMAGE-1234",
                ),
                ("WS-LENGTH", b"\x00\x15", b"\x00\x15"),
                ("RIGHT-EDGE", b"RITE", b"RITE"),
            ],
            false,
        );
    }

    #[test]
    fn cics_bms_input_storage_thirteen_qualified_move_spaces_baseline() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. BMS-MOVE-BASELINE.
DATA DIVISION. WORKING-STORAGE SECTION.
01 RESULT-MAP.
   05 FIELD-01 PIC X(16) VALUE 'AAAAAAAAAAAAAAAA'.
   05 FIELD-02 PIC X(16) VALUE 'BBBBBBBBBBBBBBBB'.
   05 FIELD-03 PIC X(2) VALUE 'CC'.
   05 FIELD-04 PIC X(4) VALUE 'DDDD'.
   05 FIELD-05 PIC X(10) VALUE 'EEEEEEEEEE'.
   05 FIELD-06 PIC X(60) VALUE
       'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF'.
   05 FIELD-07 PIC X(12) VALUE 'GGGGGGGGGGGG'.
   05 FIELD-08 PIC X(10) VALUE 'HHHHHHHHHH'.
   05 FIELD-09 PIC X(10) VALUE 'IIIIIIIIII'.
   05 FIELD-10 PIC X(9) VALUE 'JJJJJJJJJ'.
   05 FIELD-11 PIC X(30) VALUE 'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKK'.
   05 FIELD-12 PIC X(25) VALUE 'LLLLLLLLLLLLLLLLLLLLLLLLL'.
   05 FIELD-13 PIC X(10) VALUE 'MMMMMMMMMM'.
01 SHADOW-MAP.
   05 FIELD-01 PIC X VALUE 'a'.
   05 FIELD-02 PIC X VALUE 'b'.
   05 FIELD-03 PIC X VALUE 'c'.
   05 FIELD-04 PIC X VALUE 'd'.
   05 FIELD-05 PIC X VALUE 'e'.
   05 FIELD-06 PIC X VALUE 'f'.
   05 FIELD-07 PIC X VALUE 'g'.
   05 FIELD-08 PIC X VALUE 'h'.
   05 FIELD-09 PIC X VALUE 'i'.
   05 FIELD-10 PIC X VALUE 'j'.
   05 FIELD-11 PIC X VALUE 'k'.
   05 FIELD-12 PIC X VALUE 'l'.
   05 FIELD-13 PIC X VALUE 'm'.
PROCEDURE DIVISION.
    MOVE SPACES TO FIELD-01 OF RESULT-MAP FIELD-02 OF RESULT-MAP
        FIELD-03 OF RESULT-MAP FIELD-04 OF RESULT-MAP FIELD-05 OF RESULT-MAP
        FIELD-06 OF RESULT-MAP FIELD-07 OF RESULT-MAP FIELD-08 OF RESULT-MAP
        FIELD-09 OF RESULT-MAP FIELD-10 OF RESULT-MAP FIELD-11 OF RESULT-MAP
        FIELD-12 OF RESULT-MAP FIELD-13 OF RESULT-MAP.
    DISPLAY 'DONE'. STOP RUN.
"#;
        let expected = &[0x20; 214];
        retain_execution_bytes(source, "cbl", source.as_bytes()).unwrap();
        retain_execution_bytes(source, "expected-output", b"DONE\n").unwrap();
        retain_execution_bytes(source, "expected-terminal", b"Completed:0:DONE\n").unwrap();
        retain_execution_bytes(source, "RESULT-MAP.expected", expected).unwrap();
        retain_execution_bytes(source, "SHADOW-MAP.expected", b"abcdefghijklm").unwrap();
        let (output, machine) = execute_without_effects(source).expect("qualified MOVE baseline");
        let result = machine.variable("RESULT-MAP").unwrap();
        let shadow = machine.variable("SHADOW-MAP").unwrap();
        retain_execution_bytes(source, "RESULT-MAP.actual", result.bytes()).unwrap();
        retain_execution_bytes(source, "SHADOW-MAP.actual", shadow.bytes()).unwrap();
        assert_eq!(result.bytes(), expected);
        assert_eq!(shadow.bytes(), b"abcdefghijklm");
        assert_eq!(output, b"DONE\n");
    }

    #[test]
    fn cics_bms_input_storage_malformed_projection_refuses_before_mapped_writes() {
        let malformed = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-BAD-LENGTH. {BMS_INPUT_STORAGE_DATA}\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        let malformed_result = std::panic::catch_unwind(|| {
            check_cics_bms_reply(
                &malformed,
                mainframe_env_host_api::CicsOperation::ReceiveMap,
                b"BAD-LENGTH-FRAME!",
                &[
                    ("BMS.ALPHA", "mainframe-env.cics.payload@1", b"OK"),
                    ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"2"),
                    ("BMS.BETA", "mainframe-env.cics.payload@1", b"XYZ"),
                    ("BMS.BETA.LENGTH", "mainframe-env.cics.decimal@1", b"bad"),
                ],
                &[(
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                )],
                true,
            );
        });
        let oversized = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BMS-OVERSIZED. {BMS_INPUT_STORAGE_DATA}\n\
             PROCEDURE DIVISION.\n\
             EXEC CICS RECEIVE MAP('SYNMAP') INTO(INPUT-MAP) END-EXEC.\n\
             DISPLAY 'DONE'. STOP RUN."
        );
        let oversized_result = std::panic::catch_unwind(|| {
            check_cics_bms_reply(
                &oversized,
                mainframe_env_host_api::CicsOperation::ReceiveMap,
                b"OVERSIZED-FRAME!",
                &[
                    ("BMS.ALPHA", "mainframe-env.cics.payload@1", b"OK"),
                    ("BMS.ALPHA.LENGTH", "mainframe-env.cics.decimal@1", b"2"),
                    ("BMS.BETA", "mainframe-env.cics.payload@1", b"TOOLONG"),
                    ("BMS.BETA.LENGTH", "mainframe-env.cics.decimal@1", b"7"),
                ],
                &[(
                    "INPUT-MAP",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                    b"<<\x01\x02aold!||\x03\x04bstay?>>",
                )],
                true,
            );
        });
        assert!(
            malformed_result.is_ok() && oversized_result.is_ok(),
            "malformed/oversized controls"
        );
    }

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

    fn check_figurative_relation(source: &str, expected_output: &[u8], fields: &[(&str, &[u8])]) {
        retain_execution_bytes(source, "cbl", source.as_bytes()).unwrap();
        retain_execution_bytes(source, "expected-output", expected_output).unwrap();
        let (output, machine) =
            execute_without_effects(source).expect("figurative relation runtime");
        let mut observations = Vec::new();
        for (name, expected) in fields {
            let actual = machine
                .variable(name)
                .expect("observed storage")
                .bytes()
                .to_vec();
            retain_execution_bytes(source, &format!("{name}.actual"), &actual).unwrap();
            retain_execution_bytes(source, &format!("{name}.expected"), expected).unwrap();
            eprintln!("field={name}; expected={expected:?}; actual={actual:?}");
            observations.push((name, expected, actual));
        }
        eprintln!("expected-output={expected_output:?}; actual-output={output:?}");
        for (name, expected, actual) in observations {
            assert_eq!(actual, *expected, "unchanged storage {name}");
        }
        assert_eq!(output, expected_output, "literal branch outcomes");
    }

    #[test]
    fn figurative_relation_low_both_positions() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-LOW.
DATA DIVISION. WORKING-STORAGE SECTION.
01 WIDE-X PIC X(16).
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO WIDE-X.
    IF WIDE-X = LOW-VALUES DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF LOW-VALUES = WIDE-X DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF WIDE-X = LOW-VALUE DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF LOW-VALUE = WIDE-X DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(source, b"T1\nT2\nT3\nT4\n", &[("WIDE-X", &[0; 16])]);
    }

    #[test]
    fn figurative_relation_high_null_aliases() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-HIGH-NULL.
DATA DIVISION. WORKING-STORAGE SECTION.
01 HIGH-X PIC X(5). 01 NULL-X PIC X(7).
PROCEDURE DIVISION.
    MOVE HIGH-VALUES TO HIGH-X. MOVE LOW-VALUES TO NULL-X.
    IF HIGH-X = HIGH-VALUE DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF HIGH-VALUE = HIGH-X DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF HIGH-X = HIGH-VALUES DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF HIGH-VALUES = HIGH-X DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF NULL-X = NULL DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF NULL = NULL-X DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF NULL-X = NULLS DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    IF NULLS = NULL-X DISPLAY 'T8' ELSE DISPLAY 'F8' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nT2\nT3\nT4\nT5\nT6\nT7\nT8\n",
            &[("HIGH-X", &[255; 5]), ("NULL-X", &[0; 7])],
        );
    }

    #[test]
    fn figurative_relation_abbreviated_branches() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-ABBREVIATED.
DATA DIVISION. WORKING-STORAGE SECTION.
01 LOW-X PIC X(4). 01 SPACE-X PIC X(4) VALUE SPACES.
01 OTHER-X PIC X(4) VALUE 'ABCD'.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO LOW-X.
    IF LOW-X = SPACES OR LOW-VALUES DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF SPACE-X = SPACES OR LOW-VALUES DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF OTHER-X = SPACES OR LOW-VALUES DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF LOW-X NOT = SPACES AND LOW-VALUES DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF SPACE-X NOT = SPACES AND LOW-VALUES DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF OTHER-X NOT = SPACES AND LOW-VALUES DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF NOT (LOW-X = SPACES OR LOW-VALUES) DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    IF NOT (SPACE-X = SPACES OR LOW-VALUES) DISPLAY 'T8' ELSE DISPLAY 'F8' END-IF.
    IF NOT (OTHER-X = SPACES OR LOW-VALUES) DISPLAY 'T9' ELSE DISPLAY 'F9' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nT2\nF3\nF4\nF5\nT6\nF7\nF8\nT9\n",
            &[
                ("LOW-X", &[0; 4]),
                ("SPACE-X", b"    "),
                ("OTHER-X", b"ABCD"),
            ],
        );
    }

    #[test]
    fn figurative_relation_alphanumeric_zero_space_aliases() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-ZERO-SPACE.
DATA DIVISION. WORKING-STORAGE SECTION.
01 ZERO-X PIC X(6) VALUE '000000'. 01 SPACE-X PIC X(6) VALUE SPACES.
PROCEDURE DIVISION.
    IF ZERO-X = ZERO DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF ZERO = ZERO-X DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF ZERO-X = ZEROS DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF ZEROS = ZERO-X DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF ZERO-X = ZEROES DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF ZEROES = ZERO-X DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF SPACE-X = SPACE DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    IF SPACE = SPACE-X DISPLAY 'T8' ELSE DISPLAY 'F8' END-IF.
    IF SPACE-X = SPACES DISPLAY 'T9' ELSE DISPLAY 'F9' END-IF.
    IF SPACES = SPACE-X DISPLAY 'TA' ELSE DISPLAY 'FA' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nT2\nT3\nT4\nT5\nT6\nT7\nT8\nT9\nTA\n",
            &[("ZERO-X", b"000000"), ("SPACE-X", b"      ")],
        );
    }

    #[test]
    fn figurative_relation_ordinary_padding_quoted_literals() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-ORDINARY.
DATA DIVISION. WORKING-STORAGE SECTION.
01 SHORT-X PIC X. 01 WIDE-X PIC X(4).
01 PAD-X PIC X(4). 01 TEXT-X PIC X(10) VALUE 'LOW-VALUES'.
01 ZERO-TEXT PIC X(6) VALUE 'ZERO'. 01 ASCII-X PIC X(6) VALUE '000000'.
01 LETTER-X PIC X(4) VALUE 'A'.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO SHORT-X WIDE-X.
    MOVE SPACES TO PAD-X. MOVE LOW-VALUES TO PAD-X(1:1).
    IF SHORT-X = WIDE-X DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF WIDE-X = SHORT-X DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF SHORT-X = PAD-X DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF TEXT-X = 'LOW-VALUES' DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF 'LOW-VALUES' = TEXT-X DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF ZERO-TEXT = 'ZERO' DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF 'ZERO' = ZERO-TEXT DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    IF WIDE-X = 'LOW-VALUES' DISPLAY 'T8' ELSE DISPLAY 'F8' END-IF.
    IF ASCII-X = 'ZERO' DISPLAY 'T9' ELSE DISPLAY 'F9' END-IF.
    IF LETTER-X = 'A' DISPLAY 'TA' ELSE DISPLAY 'FA' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"F1\nF2\nT3\nT4\nT5\nT6\nT7\nF8\nF9\nTA\n",
            &[
                ("SHORT-X", &[0]),
                ("WIDE-X", &[0; 4]),
                ("PAD-X", b"\0   "),
                ("TEXT-X", b"LOW-VALUES"),
                ("ZERO-TEXT", b"ZERO  "),
                ("ASCII-X", b"000000"),
                ("LETTER-X", b"A   "),
            ],
        );
    }

    #[test]
    fn figurative_relation_selected_reference_width() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-REFERENCES.
DATA DIVISION. WORKING-STORAGE SECTION.
01 FIRST-GROUP. 05 LEAF-X PIC X(6) VALUE 'ABCDEF'.
01 SECOND-GROUP. 05 LEAF-X PIC X(6) VALUE 'L000R!'.
01 TABLE-ROOT. 05 ITEM-X PIC X(3) OCCURS 2 TIMES.
01 WS-IDX PIC 9 VALUE 2.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO LEAF-X OF SECOND-GROUP(2:3).
    MOVE 'ONE' TO ITEM-X(1). MOVE HIGH-VALUES TO ITEM-X(2).
    IF LEAF-X OF SECOND-GROUP(2:3) = LOW-VALUES DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF LOW-VALUES = LEAF-X OF SECOND-GROUP(2:3) DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF LEAF-X OF SECOND-GROUP = LOW-VALUES DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF ITEM-X(WS-IDX) = HIGH-VALUES DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF HIGH-VALUES = ITEM-X(WS-IDX) DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nT2\nF3\nT4\nT5\n",
            &[
                ("FIRST-GROUP", b"ABCDEF"),
                ("SECOND-GROUP", b"L\0\0\0R!"),
                ("TABLE-ROOT", b"ONE\xff\xff\xff"),
                ("WS-IDX", b"2"),
            ],
        );
    }

    #[test]
    fn figurative_relation_numeric_level88_baseline() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-BASELINE.
DATA DIVISION. WORKING-STORAGE SECTION.
01 NUM PIC 9(3) VALUE 0.
01 LOW-X PIC X(4). 88 IS-LOW VALUE LOW-VALUES.
01 HIGH-X PIC X(4). 88 IS-HIGH VALUE HIGH-VALUES.
01 SPACE-X PIC X(4) VALUE SPACES. 88 IS-SPACE VALUE SPACES.
01 ZERO-X PIC X(4) VALUE '0000'. 88 IS-ZERO VALUE ZEROS.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO LOW-X. MOVE HIGH-VALUES TO HIGH-X.
    IF NUM = ZERO DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF NUM = ZEROS DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF NUM = ZEROES DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF IS-LOW DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF IS-HIGH DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF IS-SPACE DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF IS-ZERO DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nT2\nT3\nT4\nT5\nT6\nT7\n",
            &[
                ("NUM", b"000"),
                ("LOW-X", &[0; 4]),
                ("HIGH-X", &[255; 4]),
                ("SPACE-X", b"    "),
                ("ZERO-X", b"0000"),
            ],
        );
    }

    #[test]
    fn figurative_relation_operators_collation() {
        let source = r#"
IDENTIFICATION DIVISION. PROGRAM-ID. FIG-OPERATORS.
DATA DIVISION. WORKING-STORAGE SECTION.
01 LOW-X PIC X(4). 01 HIGH-X PIC X(4).
01 LETTER-X PIC X(4) VALUE 'AAAA'. 01 DIGIT-X PIC X(4) VALUE '0000'.
PROCEDURE DIVISION.
    MOVE LOW-VALUES TO LOW-X. MOVE HIGH-VALUES TO HIGH-X.
    IF LOW-X = LOW-VALUES DISPLAY 'T1' ELSE DISPLAY 'F1' END-IF.
    IF LOW-X <> LOW-VALUES DISPLAY 'T2' ELSE DISPLAY 'F2' END-IF.
    IF LOW-X >= LOW-VALUES DISPLAY 'T3' ELSE DISPLAY 'F3' END-IF.
    IF LOW-X <= LOW-VALUES DISPLAY 'T4' ELSE DISPLAY 'F4' END-IF.
    IF LOW-X < LOW-VALUES DISPLAY 'T5' ELSE DISPLAY 'F5' END-IF.
    IF LOW-X > LOW-VALUES DISPLAY 'T6' ELSE DISPLAY 'F6' END-IF.
    IF LOW-X NOT = LOW-VALUES DISPLAY 'T7' ELSE DISPLAY 'F7' END-IF.
    IF HIGH-X = HIGH-VALUES DISPLAY 'T8' ELSE DISPLAY 'F8' END-IF.
    IF HIGH-X <> HIGH-VALUES DISPLAY 'T9' ELSE DISPLAY 'F9' END-IF.
    IF HIGH-X >= HIGH-VALUES DISPLAY 'TA' ELSE DISPLAY 'FA' END-IF.
    IF HIGH-X <= HIGH-VALUES DISPLAY 'TB' ELSE DISPLAY 'FB' END-IF.
    IF HIGH-X < HIGH-VALUES DISPLAY 'TC' ELSE DISPLAY 'FC' END-IF.
    IF HIGH-X > HIGH-VALUES DISPLAY 'TD' ELSE DISPLAY 'FD' END-IF.
    IF HIGH-X NOT = HIGH-VALUES DISPLAY 'TE' ELSE DISPLAY 'FE' END-IF.
    IF LOW-VALUES < LETTER-X DISPLAY 'TF' ELSE DISPLAY 'FF' END-IF.
    IF HIGH-VALUES > LETTER-X DISPLAY 'TG' ELSE DISPLAY 'FG' END-IF.
    IF LETTER-X < ZEROES DISPLAY 'TH' ELSE DISPLAY 'FH' END-IF.
    IF DIGIT-X > LETTER-X DISPLAY 'TI' ELSE DISPLAY 'FI' END-IF.
    STOP RUN.
"#;
        check_figurative_relation(
            source,
            b"T1\nF2\nT3\nT4\nF5\nF6\nF7\nT8\nF9\nTA\nTB\nFC\nFD\nFE\nTF\nTG\nTH\nTI\n",
            &[
                ("LOW-X", &[0; 4]),
                ("HIGH-X", &[255; 4]),
                ("LETTER-X", b"AAAA"),
                ("DIGIT-X", b"0000"),
            ],
        );
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
