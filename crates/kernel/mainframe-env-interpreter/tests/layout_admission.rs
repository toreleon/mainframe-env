use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Principal, PrincipalId,
    RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_interpreter::{MachineProblem, ReferenceMachine};
use mainframe_env_ir::{
    Attribute, CodecLimits, IrLimits, ModuleBuilder, OperationIdentity,
    cobol_layout_definition_identity, encode_binary,
};
use std::collections::{BTreeMap, BTreeSet};

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("layout-admission-request", limits).unwrap(),
        ExecutionId::new("layout-admission-execution", limits).unwrap(),
        RunUnitId::new("layout-admission-run", limits).unwrap(),
        None,
        Selector::new("program:LAYOUT", limits).unwrap(),
        ArtifactRef::new("artifact:layout", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("layout-admission-trace", limits).unwrap(),
        IdempotencyKey::new("layout-admission-idempotency", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn malformed_layout_binary() -> Vec<u8> {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            cobol_layout_definition_identity(),
            Vec::new(),
            0,
            BTreeMap::from([
                ("name".into(), Attribute::Text("RESULT".into())),
                ("simple_name".into(), Attribute::Text("RESULT".into())),
                ("category".into(), Attribute::Text("numeric_display".into())),
                ("picture".into(), Attribute::Text("9(3)".into())),
                ("digits".into(), Attribute::Text("not-an-integer".into())),
                ("scale".into(), Attribute::Integer(0)),
                ("signed".into(), Attribute::Integer(0)),
                ("sign_separate".into(), Attribute::Integer(0)),
                ("section".into(), Attribute::Text("working".into())),
                ("offset".into(), Attribute::Integer(0)),
                ("length".into(), Attribute::Integer(3)),
                ("element_length".into(), Attribute::Integer(3)),
                ("occurs".into(), Attribute::Integer(1)),
                ("parent".into(), Attribute::Text(String::new())),
                ("condition_values".into(), Attribute::Text(String::new())),
            ]),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new("mainframe.core.cobol", "halt", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
    encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
}

#[test]
fn malformed_layout_abi_fails_during_defensive_module_admission() {
    assert!(matches!(
        ReferenceMachine::from_binary(
            &malformed_layout_binary(),
            invocation(),
            CodecLimits::default(),
        ),
        Err(MachineProblem::InvalidArtifact(detail))
            if detail.contains("mainframe.core.cobol@1.define") && detail.contains("digits")
    ));
}
