use super::*;
use mainframe_env_execution_api::BoundedPayload;
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsResponse, EffectResult, HostRequest, HostResult,
};
use mainframe_env_ir::{
    Attribute, CicsPlanLimits, IrLimits, ModuleBuilder, decode_binary, decode_cics_effect_plan,
    encode_binary, encode_cics_effect_plan,
};

type FormatCase = (
    &'static str,
    usize,
    &'static [u8],
    &'static [u8],
    &'static [u8],
);

const FORMATS: [FormatCase; 6] = [
    ("MMDDYY", 8, b"083026", b"08/30/26", b"08-30-26"),
    ("YYMMDD", 8, b"260830", b"26/08/30", b"26-08-30"),
    ("TIME", 8, b"123456", b"12:34:56", b"12.34.56"),
    ("MMDDYYYY", 10, b"08302026", b"08/30/2026", b"08-30-2026"),
    ("YYYYMMDD", 10, b"20260830", b"2026/08/30", b"2026-08-30"),
    ("YYDDD", 6, b"26242", b"26/242", b"26-242"),
];

fn source(output: &str, width: usize, separators: &str) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. FMT224. DATA DIVISION. \
         WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3 VALUE 3997082096789. \
         01 OUT-X PIC X({width}) VALUE ALL 'X'. PROCEDURE DIVISION. \
         EXEC CICS FORMATTIME ABSTIME(ABS-X) {output}(OUT-X) {separators} END-EXEC. \
         STOP RUN."
    )
}

fn execute_output(output: &str, width: usize, separators: &str, formatted: &[u8]) {
    let artifact = compile(&source(output, width, separators)).unwrap();
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation(&artifact, 1024),
        CodecLimits::default(),
    )
    .unwrap_or_else(|problem| panic!("{output} width {width} {separators}: {problem:?}"));
    let drive = machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap());
    let MachineDrive::HostCall(effect) = drive else {
        panic!("FORMATTIME must pass runtime validation: {drive:?}");
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("FORMATTIME must call the CICS host");
    };
    assert_eq!(request.operation, CicsOperation::FormatTime);
    assert_eq!(request.arguments["ABSTIME"].bytes(), b"3997082096789");
    for name in ["DATESEP", "TIMESEP"] {
        let option = format!("OPTION.{name}");
        if separators.contains(&format!("{name}(")) {
            assert!(request.arguments.contains_key(name));
        } else if separators.contains(name) {
            assert!(request.arguments[&option].bytes().is_empty());
        } else {
            assert!(!request.arguments.contains_key(name));
            assert!(!request.arguments.contains_key(&option));
        }
    }
    let payload = |bytes: &[u8]| {
        BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            bytes.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    };
    let response = CicsResponse {
        disposition: CicsDisposition::Complete,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "APP".into(),
        sysid: "SYS".into(),
        transaction: "T001".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: payload(b""),
        outputs: BTreeMap::from([(output.into(), payload(formatted))]),
        unit_of_work: None,
    };
    let result = machine.drive(
        MachineResume::HostResult(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Cics(response)),
        }),
        Quantum::new(64, 1024).unwrap(),
    );
    assert!(matches!(result, MachineDrive::Completed(_)), "{result:?}");
    let mut expected = formatted.to_vec();
    expected.resize(width, b'X');
    assert_eq!(machine.variable("OUT-X").unwrap().bytes(), expected);
}

#[test]
fn compact_formattime_outputs_pass_compiled_artifact_admission() {
    for (output, _, formatted, _, _) in FORMATS {
        execute_output(output, formatted.len(), "", formatted);
    }
}

#[test]
fn separated_formattime_outputs_admit_bare_and_explicit_separators() {
    for (output, width, _, bare, explicit) in FORMATS {
        execute_output(output, width, "DATESEP TIMESEP", bare);
        execute_output(output, width, "DATESEP('-') TIMESEP('.')", explicit);
    }
}

#[test]
fn compact_formattime_preserves_documented_field_suffixes() {
    for (output, width, formatted, _, _) in FORMATS {
        execute_output(output, width, "", formatted);
    }
}

#[test]
fn formattime_compact_width_depends_on_the_corresponding_separator() {
    execute_output("TIME", 6, "DATESEP", b"123456");
    execute_output("TIME", 6, "DATESEP('-')", b"123456");
    execute_output("MMDDYY", 6, "TIMESEP", b"083026");
    execute_output("YYDDD", 5, "TIMESEP(':')", b"26242");
}

// Redirect a normally compiled FORMATTIME plan to a separately declared bad-width
// field, retaining coherent slot IDs and declarations to reach layout admission.
fn admit_redirected_output(output: &str, field: usize, width: usize, separators: &str) {
    let source = source(output, field, separators).replace(
        "PROCEDURE DIVISION.",
        &format!("01 BAD-X PIC X({width}). PROCEDURE DIVISION."),
    );
    let artifact = compile(&source).unwrap();
    let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let bad = module
        .storage()
        .iter()
        .find(|slot| slot.name == "bad-x")
        .unwrap();
    let mut builder = ModuleBuilder::new(IrLimits::default());
    for slot in module.storage() {
        assert_eq!(
            builder
                .add_storage(&slot.name, slot.size, slot.alias_of.clone())
                .unwrap(),
            slot.id
        );
    }
    for region in module.regions() {
        let region_id = builder.add_region().unwrap();
        for block in &region.blocks {
            let block_id = builder.add_block(region_id).unwrap();
            for operation in &block.operations {
                let mut attributes = operation.attributes.clone();
                let mut storage = operation.storage.clone();
                if let Some(Attribute::Bytes(bytes)) = attributes.get_mut("cics_plan") {
                    let mut plan =
                        decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
                    let target = &mut plan.outputs[0].target;
                    let reference = storage
                        .iter_mut()
                        .find(|slot| slot.storage == target.storage)
                        .unwrap();
                    target.storage = bad.id;
                    target.qualified_layout_name = "BAD-X".into();
                    reference.storage = bad.id;
                    reference.length = bad.size;
                    *bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
                }
                builder
                    .add_operation(
                        block_id,
                        operation.identity.clone(),
                        operation.operands.clone(),
                        operation.results.len(),
                        attributes,
                        operation.effects.clone(),
                        storage,
                        operation.location.clone(),
                    )
                    .unwrap();
            }
        }
    }
    let bytes = encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap();
    let Err(problem) =
        ReferenceMachine::from_binary(&bytes, invocation(&artifact, 1024), CodecLimits::default())
    else {
        panic!("invalid output width must fail admission");
    };
    assert!(
        format!("{problem:?}").contains("FORMATTIME character output has the wrong layout"),
        "{output} width {width} {separators}: {problem:?}"
    );
}

#[test]
fn formattime_invalid_widths_are_rejected_at_compile_and_artifact_admission() {
    for (output, field, compact, _, _) in FORMATS {
        for separators in ["", "DATESEP TIMESEP", "DATESEP('-') TIMESEP('.')"] {
            for width in [
                compact.len() - 1,
                compact.len(),
                compact.len() + 1,
                field + 1,
            ] {
                if width == field || (separators.is_empty() && width == compact.len()) {
                    continue;
                }
                let problem = compile(&source(output, width, separators)).unwrap_err();
                assert!(
                    problem.contains("FORMATTIME output has an invalid field length"),
                    "{problem}"
                );
                admit_redirected_output(output, field, width, separators);
            }
        }
    }
}
