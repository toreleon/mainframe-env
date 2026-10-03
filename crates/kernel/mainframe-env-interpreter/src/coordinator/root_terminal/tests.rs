//! Structural pre-dispatch filtering only; not a host admission or ABEND oracle.
use super::*;
use mainframe_env_execution_api::{BoundedPayload, IdempotencyKey, RunUnitId};
use mainframe_env_host_api::{
    HostRequest, ProgramName, ProgramRequest, RuntimeServiceKind, RuntimeServiceName,
    RuntimeServiceSelector,
};

fn occurrence(
    program: &str,
    schema: &str,
    service: Option<RuntimeServiceSelector>,
) -> EffectRequest {
    let limits = InvocationLimits::default();
    EffectRequest {
        run_unit: RunUnitId::new("original-root-run", limits).unwrap(),
        sequence: 1,
        deadline_tick: 100,
        idempotency_key: Some(IdempotencyKey::new("original-effect-key", limits).unwrap()),
        request: HostRequest::Program(ProgramRequest::Call {
            program: ProgramName::new(program, 128).unwrap(),
            service,
            payload: BoundedPayload::new(schema, Vec::new(), limits).unwrap(),
        }),
    }
}

#[test]
fn native_program_scope_requires_actual_le_abend_selector_not_a_program_label() {
    let selector = RuntimeServiceSelector {
        kind: RuntimeServiceKind::LanguageEnvironment,
        name: RuntimeServiceName::new("CEE3ABD", 128).unwrap(),
        abi_version: 1,
    };
    assert!(local_effect(&occurrence(
        "CEE3ABD",
        "mainframe-env.cobol.call@1",
        Some(selector.clone())
    )));
    assert!(local_effect(&occurrence(
        "COMPILED-CHILD",
        "mainframe-env.cobol.call@1",
        None
    )));
    for mutant in 0..5 {
        let mut service = selector.clone();
        let mut program = "CEE3ABD";
        let mut schema = "mainframe-env.cobol.call@1";
        match mutant {
            0 => service.kind = RuntimeServiceKind::HostExtension,
            1 => service.abi_version = 2,
            2 => service.name = RuntimeServiceName::new("MVSWAIT", 128).unwrap(),
            3 => schema = "mainframe-env.program.input@1",
            4 => program = "OTHER-COMPILED-PROGRAM",
            _ => unreachable!(),
        }
        assert!(
            !local_effect(&occurrence(program, schema, Some(service))),
            "input {mutant}"
        );
    }
    assert!(!local_effect(&occurrence(
        "CEE3ABD",
        "mainframe-env.program.input@1",
        None
    )));
}
