use mainframe_env_conformance::{compile, execute};
use mainframe_env_execution_api::MachineDrive;

fn output(source: &str) -> Vec<u8> {
    let artifact = compile(source).unwrap();
    match execute(&artifact, 1024) {
        MachineDrive::Completed(done) => {
            assert_eq!(done.return_code, 0);
            done.output.bytes().to_vec()
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn edited_receivers_raise_size_error_and_keep_their_values() {
    // Expected SIZE ERROR bytes from GnuCOBOL 3.2.0, cobc -x -std=ibm;
    // this is an unlicensed reference.
    for (picture, value, expected) in [
        ("9(4)", "12345", b"SIZE ERROR\n[0012]\n".as_slice()),
        ("ZZZ9", "12345", b"SIZE ERROR\n[  12]\n".as_slice()),
        ("Z,ZZ9", "12345", b"SIZE ERROR\n[   12]\n".as_slice()),
        ("$$$9", "12345", b"SIZE ERROR\n[ $12]\n".as_slice()),
        ("$$$$9", "12345", b"SIZE ERROR\n[  $12]\n".as_slice()),
        ("++++9", "12345", b"SIZE ERROR\n[  +12]\n".as_slice()),
        ("----9", "-12345", b"SIZE ERROR\n[   12]\n".as_slice()),
        ("ZZ9.99", "1234.56", b"SIZE ERROR\n[ 12.00]\n".as_slice()),
        (
            "$$B$$9.9",
            "12345.6",
            b"SIZE ERROR\n[   $12.0]\n".as_slice(),
        ),
    ] {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 P5 PIC {picture}. PROCEDURE DIVISION. MOVE 12 TO P5. COMPUTE P5 = {value} ON SIZE ERROR DISPLAY 'SIZE ERROR' NOT ON SIZE ERROR DISPLAY '[' P5 ']' END-COMPUTE. DISPLAY '[' P5 ']'. STOP RUN."
        );
        let artifact = compile(&source).unwrap_or_else(|error| panic!("{picture}: {error}"));
        let result = match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => done.output.bytes().to_vec(),
            other => panic!("{picture}: {other:?}"),
        };
        assert_eq!(result, expected, "{picture}");
    }
}

#[test]
fn edited_overflow_keeps_other_receivers_for_every_arithmetic_statement() {
    // GnuCOBOL 3.2.0, cobc -x -std=ibm, unlicensed reference.
    for (statement, expected) in [
        (
            "COMPUTE E G = 12345",
            b"SIZE ERROR\n[  12][12345]\n".as_slice(),
        ),
        (
            "ADD 12345 TO 0 GIVING E G",
            b"SIZE ERROR\n[  12][12345]\n".as_slice(),
        ),
        (
            "SUBTRACT 12345 FROM 0 GIVING E G",
            b"SIZE ERROR\n[  12][12345]\n".as_slice(),
        ),
        (
            "MULTIPLY 12345 BY 1 GIVING E",
            b"SIZE ERROR\n[  12][00000]\n".as_slice(),
        ),
        (
            "DIVIDE 12345 BY 1 GIVING E",
            b"SIZE ERROR\n[  12][00000]\n".as_slice(),
        ),
    ] {
        let verb = statement.split_whitespace().next().unwrap();
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 E PIC ZZZ9. 01 G PIC 9(5). PROCEDURE DIVISION. MOVE 12 TO E. MOVE 0 TO G. {statement} ON SIZE ERROR DISPLAY 'SIZE ERROR' NOT ON SIZE ERROR DISPLAY 'OK' END-{verb}. DISPLAY '[' E '][' G ']'. STOP RUN."
        );
        assert_eq!(output(&source), expected, "{statement}");
    }
}

#[test]
fn edited_receiver_fit_and_unsigned_negative_use_absolute_capacity() {
    for value in ["1234", "-1234"] {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 P5 PIC ZZZ9. PROCEDURE DIVISION. COMPUTE P5 = {value} ON SIZE ERROR DISPLAY 'SIZE ERROR' NOT ON SIZE ERROR DISPLAY '[' P5 ']' END-COMPUTE. STOP RUN."
        );
        assert_eq!(output(&source), b"[1234]\n", "{value}");
    }
}

#[test]
fn edited_overflow_without_size_error_phrase_keeps_existing_truncation() {
    for statement in ["COMPUTE P5 = 12345", "MULTIPLY 12345 BY 1 GIVING P5"] {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 P5 PIC ZZZ9. PROCEDURE DIVISION. {statement}. DISPLAY '[' P5 ']'. STOP RUN."
        );
        assert_eq!(output(&source), b"[2345]\n", "{statement}");
    }
}

#[test]
fn edited_overflow_with_only_not_on_size_error_keeps_existing_branch() {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. T. DATA DIVISION. WORKING-STORAGE SECTION. 01 P5 PIC ZZZ9. PROCEDURE DIVISION. COMPUTE P5 = 12345 NOT ON SIZE ERROR DISPLAY 'OK' END-COMPUTE. DISPLAY '[' P5 ']'. STOP RUN.";
    assert_eq!(output(source), b"OK\n[2345]\n");
}
