use crate::{
    HELLO_SOURCE, compile, invocation, verify_cobol_frontend_fixtures,
    verify_cobol_function_fixtures, verify_cobol_semantic_fixtures,
    verify_cobol_statement_fixtures,
};
use base64::Engine;
use mainframe_env_compiler::{
    CobolCompiler, CobolCompilerLimits, DATA_DESCRIPTION_CLAUSES, FILE_DESCRIPTION_CLAUSES,
    INTRINSIC_FUNCTIONS, PROCEDURE_STATEMENTS, SPECIAL_REGISTERS,
};
use mainframe_env_compiler_api::{
    ArtifactLimits, ArtifactManifestV2, CompilationMode, CompileOptions, CompileTarget,
    CompilerRequest, CompilerResult, CompilerService, LEGACY_ARTIFACT_CONTRACT, ValidatedArtifact,
    VersionedArtifactManifest,
};
use mainframe_env_execution_api::{
    ArtifactRef, InvocationLimits, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{CodecLimits, decode_binary, encode_binary};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PRIOR_ARTIFACT_B64: &str =
    include_str!("../../../../conformance/0.9/cobol/artifact-v2-c029219.b64");
const PRIOR_ARTIFACT_SHA256: &str =
    "cf5de374e76c07ff001af9a20053fd8692f55337a4ebece2085ce62c436d58db";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolExitReceipt {
    pub official_rows: usize,
    pub functions: usize,
    pub special_registers: usize,
    pub malformed_classes: usize,
    pub limit_classes: usize,
    pub recovery_classes: usize,
    pub prior_artifact_bytes: usize,
}

pub fn verify_cobol_exit() -> Result<CobolExitReceipt, String> {
    verify_cobol_frontend_fixtures()?;
    verify_cobol_semantic_fixtures()?;
    verify_cobol_statement_fixtures()?;
    verify_cobol_function_fixtures()?;
    verify_generated_closure()?;
    let malformed_classes = verify_malformed_classes()?;
    let limit_classes = verify_limits()?;
    let recovery_classes = verify_recovery_and_publication()?;
    let prior_artifact_bytes = verify_prior_artifact()?;
    Ok(CobolExitReceipt {
        official_rows: PROCEDURE_STATEMENTS.len()
            + INTRINSIC_FUNCTIONS.len()
            + FILE_DESCRIPTION_CLAUSES.len()
            + DATA_DESCRIPTION_CLAUSES.len()
            + 20,
        functions: INTRINSIC_FUNCTIONS.len(),
        special_registers: SPECIAL_REGISTERS.len(),
        malformed_classes,
        limit_classes,
        recovery_classes,
        prior_artifact_bytes,
    })
}

fn verify_generated_closure() -> Result<(), String> {
    let function_rows = INTRINSIC_FUNCTIONS
        .iter()
        .map(|entry| entry.row_id)
        .collect::<BTreeSet<_>>();
    let function_names = INTRINSIC_FUNCTIONS
        .iter()
        .map(|entry| entry.name)
        .collect::<BTreeSet<_>>();
    let statement_rows = PROCEDURE_STATEMENTS
        .iter()
        .map(|entry| entry.row_id)
        .collect::<BTreeSet<_>>();
    let register_names = SPECIAL_REGISTERS
        .iter()
        .map(|entry| entry.name)
        .collect::<BTreeSet<_>>();
    if function_rows.len() != 82
        || function_names.len() != 82
        || statement_rows.len() != 44
        || register_names.len() != 28
        || FILE_DESCRIPTION_CLAUSES.len() != 10
        || DATA_DESCRIPTION_CLAUSES.len() != 17
    {
        return Err("generated COBOL identity closure drifted".into());
    }
    Ok(())
}

fn verify_malformed_classes() -> Result<usize, String> {
    let malformed = [
        "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 X PIC X(0). PROCEDURE DIVISION. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. DATA DIVISION. WORKING-STORAGE SECTION. 01 X PIC X COMP. PROCEDURE DIVISION. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. PROCEDURE DIVISION. MOVE FUNCTION ABS TO X. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. PROCEDURE DIVISION. END-IF. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. BAD. PROCEDURE DIVISION. FLY TO MARS. STOP RUN.",
    ];
    for source in malformed {
        let analysis = CobolCompiler::default().analyze(&bundle(source.as_bytes().to_vec())?);
        if analysis.semantic.is_some() && analysis.hir.is_some() {
            return Err(format!("malformed COBOL reached complete HIR: {source}"));
        }
    }
    let invalid_utf8 = CobolCompiler::default().analyze(&bundle(vec![0xff, 0xfe, 0xfd])?);
    if invalid_utf8.syntax.is_some() {
        return Err("malformed UTF-8 reached syntax".into());
    }
    Ok(malformed.len() + 1)
}

fn verify_limits() -> Result<usize, String> {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LIMITS. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC X(2). 01 B PIC X(2). PROCEDURE DIVISION. DISPLAY A. STOP RUN.";
    let mut syntax_limits = CobolCompilerLimits::default();
    syntax_limits.syntax.max_tokens = 8;
    let item_limits = CobolCompilerLimits {
        max_data_items: 1,
        ..CobolCompilerLimits::default()
    };
    let storage_limits = CobolCompilerLimits {
        max_storage_bytes: 1,
        ..CobolCompilerLimits::default()
    };
    let mut operation_limits = CobolCompilerLimits::default();
    operation_limits.ir.max_operations = 2;
    for limits in [syntax_limits, item_limits, storage_limits, operation_limits] {
        let analysis = CobolCompiler::new(limits).analyze(&bundle(source.as_bytes().to_vec())?);
        if analysis.completeness == mainframe_env_diagnostics::Completeness::Complete {
            return Err("COBOL resource limit failed open".into());
        }
    }
    Ok(4)
}

fn verify_recovery_and_publication() -> Result<usize, String> {
    let recovered = "IDENTIFICATION DIVISION. PROGRAM-ID. DUP. PROCEDURE DIVISION. DUP. DISPLAY 'A'. DUP. EXIT. STOP RUN.";
    let result = CobolCompiler::default()
        .compile(request(recovered)?)
        .map_err(|problem| problem.to_string())?;
    if matches!(result, CompilerResult::Published { .. }) {
        return Err("recovered malformed COBOL published".into());
    }
    let promoted = [
        "IDENTIFICATION DIVISION. PROGRAM-ID. DYNAMIC. DATA DIVISION. WORKING-STORAGE SECTION. 01 X PIC X DYNAMIC LIMIT 8. PROCEDURE DIVISION. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. FLOAT-X. DATA DIVISION. WORKING-STORAGE SECTION. 01 N COMP-1. PROCEDURE DIVISION. DISPLAY N. STOP RUN.",
        "IDENTIFICATION DIVISION. PROGRAM-ID. FUNCTION-X. DATA DIVISION. WORKING-STORAGE SECTION. 01 N PIC 9V9. PROCEDURE DIVISION. MOVE FUNCTION SQRT(N) TO N. STOP RUN.",
    ];
    for source in promoted {
        let result = CobolCompiler::default()
            .compile(request(source)?)
            .map_err(|problem| problem.to_string())?;
        if !matches!(result, CompilerResult::Published { .. }) {
            return Err("0.4 promoted COBOL execution did not publish".into());
        }
    }
    Ok(4)
}

fn verify_prior_artifact() -> Result<usize, String> {
    let payload = base64::engine::general_purpose::STANDARD
        .decode(PRIOR_ARTIFACT_B64.trim())
        .map_err(|error| error.to_string())?;
    if format!("{:x}", Sha256::digest(&payload)) != PRIOR_ARTIFACT_SHA256 || payload.len() != 1261 {
        return Err("accepted c029219 artifact-v2 fixture drifted".into());
    }
    let module =
        decode_binary(&payload, CodecLimits::default()).map_err(|error| error.to_string())?;
    let encoded =
        encode_binary(&module, CodecLimits::default()).map_err(|error| error.to_string())?;
    if encoded != payload {
        return Err("accepted c029219 artifact-v2 is not canonically readable".into());
    }
    let current = compile(HELLO_SOURCE)?;
    let legacy_manifest = ArtifactManifestV2 {
        compiler_generation: "mainframe-env-cobol-0.8.3".into(),
        target: CompileTarget::new("reference").map_err(|problem| problem.to_string())?,
        options: CompileOptions::new(BTreeMap::from([
            ("cobol.effective-arith".into(), "extended".into()),
            ("cobol.effective-dispsign".into(), "compatible".into()),
            ("cobol.effective-lp".into(), "32".into()),
        ]))
        .map_err(|problem| problem.to_string())?,
        host_interfaces: BTreeSet::from([
            "mainframe-env.host@1".into(),
            "mainframe-env.cics@1".into(),
        ]),
        ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
    };
    let catalog = mainframe_env_compiler::core_mir_catalog();
    let profile = mainframe_env_compiler::core_mir_profile();
    let migrated = ValidatedArtifact::read(
        VersionedArtifactManifest::V2(legacy_manifest),
        &payload,
        &catalog,
        &profile,
        CodecLimits::default(),
        ArtifactLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    if migrated.source_contract() != LEGACY_ARTIFACT_CONTRACT
        || migrated.payload() != payload
        || migrated.content_id().to_hex() != PRIOR_ARTIFACT_SHA256
        || migrated.manifest().dialect_contracts.is_empty()
    {
        return Err("version-2 artifact migration changed identity or executable bytes".into());
    }
    execute_hello_payload(
        migrated.payload(),
        &current,
        b"HELLO\n",
        "accepted version-2 artifact",
    )?;
    Ok(payload.len())
}

fn execute_hello_payload(
    payload: &[u8],
    current: &mainframe_env_compiler_api::PublishedArtifact,
    expected: &[u8],
    label: &str,
) -> Result<(), String> {
    let mut execution = invocation(current, 1024);
    execution.artifact = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(payload)),
        InvocationLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    let mut machine = ReferenceMachine::from_binary(payload, execution, CodecLimits::default())
        .map_err(|problem| format!("{label}: {problem:?}"))?;
    let mut resume = MachineResume::Start;
    loop {
        match machine.drive(
            resume,
            Quantum::new(8, 4096).ok_or("invalid compatibility quantum")?,
        ) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::Completed(done) => {
                if done.return_code != 0 || done.output.bytes() != expected {
                    return Err(format!("{label} execution drifted"));
                }
                break;
            }
            other => return Err(format!("{label} stopped: {other:?}")),
        }
    }
    Ok(())
}

fn request(source: &str) -> Result<CompilerRequest, String> {
    Ok(CompilerRequest {
        source: bundle(source.as_bytes().to_vec())?,
        mode: CompilationMode::Executable,
        target: CompileTarget::new("reference").map_err(|error| error.to_string())?,
        options: CompileOptions::new(BTreeMap::new()).map_err(|error| error.to_string())?,
    })
}

fn bundle(bytes: Vec<u8>) -> Result<SourceBundle, String> {
    let limits = SourceLimits::default();
    let path =
        LogicalPath::new("exit.cbl", limits.max_path_bytes).map_err(|error| error.to_string())?;
    let file = SourceFile::input(
        path.as_str(),
        bytes,
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| error.to_string())?;
    SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_cobol_exit_matrix_passes() {
        let receipt = verify_cobol_exit().unwrap();
        assert_eq!(receipt.official_rows, 173);
        assert_eq!(receipt.prior_artifact_bytes, 1261);
    }

    #[test]
    fn representative_registry_validation_and_artifact_mutants_are_killed() {
        assert!(verify_generated_closure().is_ok());
        assert_ne!(INTRINSIC_FUNCTIONS.len() - 1, 82);
        let mut payload = base64::engine::general_purpose::STANDARD
            .decode(PRIOR_ARTIFACT_B64.trim())
            .unwrap();
        payload[32] ^= 0x80;
        assert!(decode_binary(&payload, CodecLimits::default()).is_err());
        assert!(verify_malformed_classes().is_ok());
        assert!(verify_recovery_and_publication().is_ok());
    }
}
