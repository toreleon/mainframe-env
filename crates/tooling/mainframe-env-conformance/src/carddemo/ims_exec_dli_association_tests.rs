//! Actual compiler/IR/machine offer controls; no application or IBM acceptance.
use super::*;
use mainframe_env_host_api::{HostLimits, HostRequest};
use mainframe_env_interpreter::ReferenceMachine;

fn compiled_drive(command: &str) -> MachineDrive<EffectRequest> {
    let source = format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. IMSORDER.\n\
         DATA DIVISION. WORKING-STORAGE SECTION.\n\
         01 PCB-N PIC S9(4) COMP VALUE 2.\n\
         01 PA-ACCT-ID PIC S9(11) COMP-3 VALUE 1.\n\
         01 PENDING-AUTH-DETAILS PIC X(200) VALUE ALL 'Z'.\n\
         01 ROOT-X PIC X(100).\n\
         01 ACCT-X PIC X(6) VALUE '000001'.\n\
         01 OTHER-X PIC X(3) VALUE 'QRS'.\n\
         01 DETAIL-X PIC X(8) VALUE 'C1234567'.\n\
         PROCEDURE DIVISION. {command} STOP RUN."
    );
    let definition = compile_free_batch_definition("IMSORDER", &source).unwrap();
    let mut invocation = ims_invocation("ims-source-order", true, None).unwrap();
    invocation.artifact = definition.artifact.clone();
    let mut machine = ReferenceMachine::from_binary(
        &definition.payload,
        invocation,
        mainframe_env_ir::CodecLimits::default(),
    )
    .unwrap();
    machine.drive(
        MachineResume::Start,
        Quantum {
            max_steps: 100,
            max_allocated_bytes: 1_048_576,
        },
    )
}

fn compiled_offer(command: &str) -> ImsRequest {
    let MachineDrive::HostCall(effect) = compiled_drive(command) else {
        panic!("actual compiled EXEC DLI host offer required")
    };
    effect.validate(HostLimits::default()).unwrap();
    assert_eq!(effect.sequence, 1);
    assert_eq!(effect.run_unit.as_str(), "ims-source-order");
    let HostRequest::Ims(request) = effect.request else {
        panic!("compiled EXEC DLI must preserve the existing typed IMS route")
    };
    assert_eq!(request.pcb, 2);
    assert_eq!(request.max_segments, 1);
    assert!(request.psb.is_none());
    assert!(request.checkpoint_id.is_none());
    assert!(request.system.is_none());
    assert!(request.q_class.is_none());
    assert!(request.mutation.is_some());
    request
}

#[test]
fn compiled_dli_parent_where_precedes_child_target() {
    // The pinned authorization source uses this ISRT operand order. This small
    // compiler fixture offers bytes only; it does not execute that application.
    let request = compiled_offer(
        "EXEC DLI ISRT USING PCB(PCB-N) SEGMENT(PAUTSUM0) \
         WHERE(ACCNTID = PA-ACCT-ID) SEGMENT(PAUTDTL1) \
         FROM(PENDING-AUTH-DETAILS) \
         SEGLENGTH(LENGTH OF PENDING-AUTH-DETAILS) END-EXEC.",
    );
    assert_eq!(request.operation, ImsOperation::Insert);
    assert_eq!(request.segments, ["PAUTSUM0", "PAUTDTL1"]);
    assert_eq!(request.data, vec![b'Z'; 200]);
    assert_eq!(
        request.qualifiers,
        vec![ImsQualifier {
            segment: "PAUTSUM0".into(),
            field: "ACCNTID".into(),
            value: vec![0, 0, 0, 0, 0, 0x1c],
        }]
    );
}

#[test]
fn compiled_dli_multiple_where_clauses_preserve_segment_and_operand_order() {
    let request = compiled_offer(
        "EXEC DLI GU USING PCB(PCB-N) SEGMENT(PAUTSUM0) INTO(ROOT-X) \
         WHERE(ACCNTID = ACCT-X) WHERE(OTHERKEY = OTHER-X) \
         SEGMENT(PAUTDTL1) WHERE(PAUT9CTS = DETAIL-X) END-EXEC.",
    );
    assert_eq!(request.operation, ImsOperation::GetUnique);
    assert_eq!(request.segments, ["PAUTSUM0", "PAUTDTL1"]);
    assert!(request.data.is_empty());
    assert_eq!(
        request.qualifiers,
        vec![
            ImsQualifier {
                segment: "PAUTSUM0".into(),
                field: "ACCNTID".into(),
                value: b"000001".to_vec(),
            },
            ImsQualifier {
                segment: "PAUTSUM0".into(),
                field: "OTHERKEY".into(),
                value: b"QRS".to_vec(),
            },
            ImsQualifier {
                segment: "PAUTDTL1".into(),
                field: "PAUT9CTS".into(),
                value: b"C1234567".to_vec(),
            },
        ]
    );
}

#[test]
fn compiled_dli_missing_empty_or_ambiguous_preceding_segment_refuses_offer() {
    for clauses in [
        "WHERE(ACCNTID = ACCT-X)",
        "WHERE(ACCNTID = ACCT-X) SEGMENT(PAUTSUM0)",
        "SEGMENT() WHERE(ACCNTID = ACCT-X)",
        "SEGMENT(PAUTSUM0 PAUTDTL1) WHERE(ACCNTID = ACCT-X)",
        "SEGMENT(PAUTSUM0) SEGMENT() WHERE(ACCNTID = ACCT-X)",
        "SEGMENT(PAUTSUM0) SEGMENT(PAUTDTL1 PAUTSUM0) WHERE(ACCNTID = ACCT-X)",
    ] {
        let command = format!("EXEC DLI GU USING PCB(PCB-N) {clauses} END-EXEC.");
        assert!(matches!(compiled_drive(&command), MachineDrive::Failed(_)));
    }
}

#[test]
fn compiled_dli_unknown_source_identity_is_not_reassigned_to_known_segment() {
    let request = compiled_offer(
        "EXEC DLI GU USING PCB(PCB-N) SEGMENT(UNKNOWN0) \
         WHERE(UNKNOWNF = ACCT-X) SEGMENT(PAUTDTL1) END-EXEC.",
    );
    assert_eq!(request.operation, ImsOperation::GetUnique);
    assert_eq!(request.segments, ["UNKNOWN0", "PAUTDTL1"]);
    assert_eq!(
        request.qualifiers,
        vec![ImsQualifier {
            segment: "UNKNOWN0".into(),
            field: "UNKNOWNF".into(),
            value: b"000001".to_vec(),
        }]
    );
    // The existing selected provider retains authority to refuse unknown metadata.
    // An emitted name is never an installed segment or field admission.
}
