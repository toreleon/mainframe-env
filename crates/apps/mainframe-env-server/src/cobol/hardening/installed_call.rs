use super::*;

#[test]
fn installed_call_uses_procedure_formal_order_for_arguments_and_results() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install(
        "ORDER",
        "IDENTIFICATION DIVISION. PROGRAM-ID. ORDER. DATA DIVISION. LINKAGE SECTION. 01 UNUSED-AREA PIC X. 01 ARG-TWO PIC X. 01 ARG-ONE PIC X. PROCEDURE DIVISION USING ARG-ONE ARG-TWO. MOVE ARG-ONE TO ARG-TWO. MOVE 'A' TO ARG-ONE. GOBACK.",
    );
    assert_eq!(
        fixture
            .call(
                &parent(),
                "ORDER",
                1,
                call_payload(&[vec![b'1'], vec![b'2']])
            )
            .outcome,
        Ok(HostResult::Program(
            encode_cobol_call_result(&[vec![b'A'], vec![b'1']]).unwrap()
        ))
    );
}

#[test]
fn installed_batch_parm_uses_declared_procedure_formal() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. PARMINST. DATA DIVISION. WORKING-STORAGE SECTION. 01 DISPLAY-LENGTH PIC 9(4). LINKAGE SECTION. 01 UNUSED-AREA PIC X(12). 01 PARM-AREA. 05 PARM-LENGTH PIC S9(4) COMP. 05 PARM-DATE PIC X(10). PROCEDURE DIVISION USING PARM-AREA. MOVE PARM-LENGTH TO DISPLAY-LENGTH. DISPLAY DISPLAY-LENGTH. DISPLAY PARM-DATE. STOP RUN.";
    fixture.install("PARMINST", source);
    let mut batch_input = input("");
    batch_input.parameter = Some("2022071800".into());
    let result = fixture.call(
        &parent(),
        "PARMINST",
        1,
        BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(&batch_input).unwrap(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let HostResult::Program(payload) = result.outcome.unwrap() else {
        panic!("program output");
    };
    let output: ProgramOutput = serde_json::from_slice(payload.bytes()).unwrap();
    assert_eq!(
        output.records,
        vec![b"0010".to_vec(), b"2022071800".to_vec()]
    );
}

#[test]
fn hardening_47_initial_and_cancel_have_distinct_real_call_lifecycles() {
    for initial in [false, true] {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            HostProblem::NotFound,
            false,
        );
        fixture.install(
            "COUNTER",
            &if initial {
                INSTANCE_COUNTER.replace(
                    "PROGRAM-ID. COUNTER.",
                    "PROGRAM-ID. COUNTER IS INITIAL PROGRAM.",
                )
            } else {
                INSTANCE_COUNTER.into()
            },
        );
        let caller = "IDENTIFICATION DIVISION.\nPROGRAM-ID. CALLER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 R PIC 9.\nPROCEDURE DIVISION.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nCANCEL 'COUNTER'.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nSTOP RUN.\n";
        let result = fixture.batch("COBOL", caller).outcome.unwrap();
        let HostResult::Program(payload) = result else {
            panic!("program output");
        };
        let output: ProgramOutput = serde_json::from_slice(payload.bytes()).unwrap();
        assert_eq!(
            output.records,
            if initial {
                vec![b"1".to_vec(), b"1".to_vec(), b"1".to_vec()]
            } else {
                vec![b"1".to_vec(), b"2".to_vec(), b"1".to_vec()]
            }
        );
    }
}
