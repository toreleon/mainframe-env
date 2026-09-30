use mainframe_env_conformance::{compile, execute};
use mainframe_env_execution_api::MachineDrive;

#[test]
fn issue_230_floating_currency_compiles_and_displays() {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. HELLO. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC S9(7)V99 VALUE 95.85. 01 R PIC $$$,$$9.99. PROCEDURE DIVISION. MOVE A TO R. DISPLAY R. STOP RUN.";
    let artifact = compile(source).unwrap();
    match execute(&artifact, 1024) {
        MachineDrive::Completed(done) => {
            assert_eq!(done.return_code, 0);
            // Expected bytes from GnuCOBOL 3.2.0 with cobc -x -std=ibm.
            assert_eq!(done.output.bytes(), b"    $95.85\n");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn floating_insertion_variants_pass_ir_verification() {
    for picture in ["$$$.99", "$$V99", "$$$$", "+++,++9.99", "----9"] {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. HELLO. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC S9(7)V99 VALUE 95.85. 01 R PIC {picture}. PROCEDURE DIVISION. MOVE A TO R. DISPLAY R. STOP RUN."
        );
        compile(&source).unwrap_or_else(|error| panic!("{picture}: {error}"));
    }
}

#[test]
fn embedded_simple_insertions_route_through_compiler_and_interpreter() {
    // Expected bytes from GnuCOBOL 3.2.0 with -std=ibm, an unlicensed reference.
    for (picture, value, expected) in [
        ("$$B$$9.9", "99999999999999999", b"$9 999.0\n".as_slice()),
        ("$$0$$9", "12345", b"$20345\n".as_slice()),
        ("++B++9", "12345", b"+2 345\n".as_slice()),
    ] {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. HELLO. DATA DIVISION. WORKING-STORAGE SECTION. 01 N0 PIC 9(17) USAGE DISPLAY VALUE {value}. 01 N5 PIC {picture}. PROCEDURE DIVISION. MOVE N0 TO N5. DISPLAY N5. STOP RUN."
        );
        let artifact = compile(&source).unwrap_or_else(|error| panic!("{picture}: {error}"));
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.return_code, 0, "{picture}");
                assert_eq!(done.output.bytes(), expected, "{picture}");
            }
            other => panic!("{picture}: {other:?}"),
        }
    }
}
