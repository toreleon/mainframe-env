use crate::CompilerDirectingKind;
use crate::hir::{CobolHir, HirProblem};
use crate::lower::{LowerProblem, core_mir_catalog, core_mir_profile, lower_to_core};
use crate::semantic::{SemanticModel, SemanticProblem};
use crate::syntax::{LosslessSyntax, SyntaxLimits, SyntaxProblem, decode_and_lex};
use mainframe_env_compiler_api::{
    ArtifactLimits, ArtifactManifest, CompilationMode, CompileOptions, CompilerProblem,
    CompilerRequest, CompilerResult, CompilerService, LegalizedMir, PublishedArtifact, VerifiedHir,
};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity,
};
use mainframe_env_ir::{
    COBOL_EFFECTIVE_ARITH_OPTION, COBOL_EFFECTIVE_DISPSIGN_OPTION, COBOL_EFFECTIVE_LP_OPTION,
    CodecLimits, IrLimits, to_text,
};
use mainframe_env_source::SourceBundle;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CobolCompilerLimits {
    pub syntax: SyntaxLimits,
    pub ir: IrLimits,
    pub codec: CodecLimits,
    pub artifact: ArtifactLimits,
    pub max_storage_bytes: usize,
    pub max_data_items: usize,
    pub max_diagnostics: usize,
}
impl Default for CobolCompilerLimits {
    fn default() -> Self {
        Self {
            syntax: SyntaxLimits::default(),
            ir: IrLimits::default(),
            codec: CodecLimits::default(),
            artifact: ArtifactLimits::default(),
            max_storage_bytes: 64 * 1024 * 1024,
            max_data_items: 65_536,
            max_diagnostics: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CobolAnalysis {
    pub syntax: Option<LosslessSyntax>,
    pub semantic: Option<SemanticModel>,
    pub hir: Option<CobolHir>,
    pub hir_text: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub completeness: Completeness,
}

/// An immutable COBOL analysis paired with verification of its exact HIR module.
///
/// Only `verify_hir` constructs this token. Lowering cannot accept raw metadata
/// or combine a verification result with a different mutable analysis.
pub(crate) struct VerifiedCobolHir<'a> {
    hir: &'a CobolHir,
    stage: VerifiedHir,
}

impl VerifiedCobolHir<'_> {
    pub(crate) fn hir(&self) -> &CobolHir {
        self.hir
    }
}

pub struct CobolCompiler {
    limits: CobolCompilerLimits,
}
impl CobolCompiler {
    #[must_use]
    pub fn new(limits: CobolCompilerLimits) -> Self {
        Self { limits }
    }
    #[must_use]
    pub const fn limits(&self) -> CobolCompilerLimits {
        self.limits
    }

    pub fn analyze(&self, source: &SourceBundle) -> CobolAnalysis {
        let mut diagnostics = Vec::new();
        let syntax = match decode_and_lex(source, self.limits.syntax) {
            Ok(syntax) => syntax,
            Err(problem) => return failed_analysis(diagnostic_for_syntax(problem)),
        };
        let semantic = match SemanticModel::analyze_with_origins(
            syntax.semantic_text(),
            syntax.semantic_origins(),
            syntax.effective_compiler_options().pointer_bytes(),
            self.limits.max_storage_bytes,
            self.limits.max_data_items,
        ) {
            Ok(semantic) => semantic,
            Err(problem) => {
                return CobolAnalysis {
                    syntax: Some(syntax),
                    semantic: None,
                    hir: None,
                    hir_text: None,
                    diagnostics: vec![diagnostic_for_semantic(problem)],
                    completeness: Completeness::Incomplete,
                };
            }
        };
        let hir = match CobolHir::build(source, &syntax, &semantic, self.limits.ir) {
            Ok(hir) => hir,
            Err(problem) => {
                return CobolAnalysis {
                    syntax: Some(syntax),
                    semantic: Some(semantic),
                    hir: None,
                    hir_text: None,
                    diagnostics: vec![diagnostic_for_hir(problem)],
                    completeness: Completeness::Incomplete,
                };
            }
        };
        let unsupported_statements = hir.unsupported();
        let incomplete_layouts = semantic.execution_incomplete_layouts();
        let incomplete_intrinsics = semantic.execution_incomplete_intrinsics();
        let incomplete_registers = semantic.execution_incomplete_special_registers();
        let execution_incomplete = !unsupported_statements.is_empty()
            || !incomplete_layouts.is_empty()
            || !incomplete_intrinsics.is_empty()
            || !incomplete_registers.is_empty();
        for kind in unsupported_statements {
            if diagnostics.len() >= self.limits.max_diagnostics {
                break;
            }
            diagnostics.push(diagnostic(
                "MECOB0200",
                Phase::Lower,
                FailureCategory::Unsupported,
                format!(
                    "{} is explicitly unsupported by the frozen 0.1 executable profile",
                    kind.slug()
                ),
            ));
        }
        for layout in incomplete_layouts {
            if diagnostics.len() >= self.limits.max_diagnostics {
                break;
            }
            diagnostics.push(diagnostic(
                "MECOB0201",
                Phase::Lower,
                FailureCategory::Unsupported,
                format!(
                    "{layout} has structural layout metadata but requires 0.4 execution support"
                ),
            ));
        }
        for intrinsic in incomplete_intrinsics {
            if diagnostics.len() >= self.limits.max_diagnostics {
                break;
            }
            diagnostics.push(diagnostic(
                "MECOB0202",
                Phase::Lower,
                FailureCategory::Unsupported,
                format!(
                    "FUNCTION {} is recognized and typed but has no 0.3 execution route",
                    crate::intrinsic_function_descriptor(intrinsic).name
                ),
            ));
        }
        for register in incomplete_registers {
            if diagnostics.len() >= self.limits.max_diagnostics {
                break;
            }
            diagnostics.push(diagnostic(
                "MECOB0203",
                Phase::Lower,
                FailureCategory::Unsupported,
                format!(
                    "{} is recognized and typed but has no 0.3 execution route",
                    crate::special_register_descriptor(register).name
                ),
            ));
        }
        let hir_text = to_text(&hir.module, self.limits.codec).ok();
        let completeness = if execution_incomplete {
            Completeness::Unsupported
        } else {
            Completeness::Complete
        };
        CobolAnalysis {
            syntax: Some(syntax),
            semantic: Some(semantic),
            hir: Some(hir),
            hir_text,
            diagnostics,
            completeness,
        }
    }

    fn verify_hir<'a>(
        &self,
        source: &SourceBundle,
        analysis: &'a CobolAnalysis,
    ) -> Result<VerifiedCobolHir<'a>, CompilerProblem> {
        if analysis.completeness != Completeness::Complete {
            return Err(CompilerProblem::IncompleteStage);
        }
        let hir = analysis
            .hir
            .as_ref()
            .ok_or(CompilerProblem::IncompleteStage)?;
        let stage =
            VerifiedHir::verify(source.id(), hir.module.clone(), &crate::cobol_hir_catalog())?;
        Ok(VerifiedCobolHir { hir, stage })
    }
}

impl Default for CobolCompiler {
    fn default() -> Self {
        Self::new(CobolCompilerLimits::default())
    }
}

impl CompilerService for CobolCompiler {
    fn compile(&self, request: CompilerRequest) -> Result<CompilerResult, CompilerProblem> {
        let analysis = self.analyze(&request.source);
        self.compile_analyzed(request, analysis)
    }
}

impl CobolCompiler {
    fn compile_analyzed(
        &self,
        request: CompilerRequest,
        mut analysis: CobolAnalysis,
    ) -> Result<CompilerResult, CompilerProblem> {
        if request.mode == CompilationMode::Analyze {
            let hir = match self.verify_hir(&request.source, &analysis) {
                Ok(verified) => Some(verified.stage),
                Err(CompilerProblem::IncompleteStage)
                    if analysis.completeness != Completeness::Complete =>
                {
                    None
                }
                Err(problem) => {
                    analysis.diagnostics.push(diagnostic(
                        "MECOB0300",
                        Phase::Verify,
                        FailureCategory::MalformedInput,
                        problem.to_string(),
                    ));
                    analysis.completeness = Completeness::Failed;
                    None
                }
            };
            return Ok(CompilerResult::Analysis {
                hir,
                diagnostics: analysis.diagnostics,
                completeness: analysis.completeness,
            });
        }
        if !analysis.diagnostics.is_empty() || analysis.completeness != Completeness::Complete {
            return Ok(CompilerResult::Failed {
                diagnostics: analysis.diagnostics,
                completeness: analysis.completeness,
            });
        }
        // Verification failure is terminal: no lowering, legalization or
        // artifact publication may happen without the associated proof.
        let verified = self.verify_hir(&request.source, &analysis)?;
        let syntax = analysis
            .syntax
            .as_ref()
            .ok_or(CompilerProblem::IncompleteStage)?;
        let effective_options = syntax.effective_compiler_options();
        let declaratives = syntax
            .compiler_directing_statements()
            .iter()
            .filter(|statement| statement.kind == CompilerDirectingKind::Use && statement.active)
            .map(|statement| {
                Ok((
                    statement
                        .declarative_section
                        .clone()
                        .ok_or(CompilerProblem::IncompleteStage)?,
                    statement.operands.clone(),
                ))
            })
            .collect::<Result<Vec<_>, CompilerProblem>>()?;
        let mir = lower_to_core(
            &verified,
            effective_options.arithmetic_mode().as_str(),
            effective_options.display_sign().as_str(),
            effective_options.lp(),
            &declaratives,
            self.limits.ir,
        )
        .map_err(lower_problem)?;
        let lowered = verified.stage.lower(mir);
        let legalized = LegalizedMir::legalize(lowered, &core_mir_catalog(), &core_mir_profile())?;
        let dialect_contracts = legalized
            .legal()
            .module()
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .map(|operation| {
                format!(
                    "{}@{}",
                    operation.identity.namespace(),
                    operation.identity.major()
                )
            })
            .collect();
        let mut manifest_options = request.options.values().clone();
        manifest_options.insert(
            COBOL_EFFECTIVE_LP_OPTION.into(),
            effective_options.lp().to_string(),
        );
        manifest_options.insert(
            COBOL_EFFECTIVE_ARITH_OPTION.into(),
            effective_options.arithmetic_mode().as_str().into(),
        );
        manifest_options.insert(
            COBOL_EFFECTIVE_DISPSIGN_OPTION.into(),
            effective_options.display_sign().as_str().into(),
        );
        let manifest = ArtifactManifest {
            compiler_generation: format!("mainframe-env-cobol-{}", env!("CARGO_PKG_VERSION")),
            target: request.target,
            options: CompileOptions::new(manifest_options)?,
            host_interfaces: BTreeSet::from([
                "mainframe-env.host@1".to_string(),
                "mainframe-env.cics@1".to_string(),
            ]),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.to_string(),
            dialect_contracts,
        };
        let artifact = PublishedArtifact::publish(
            legalized,
            manifest,
            self.limits.codec,
            self.limits.artifact,
        )?;
        Ok(CompilerResult::Published {
            artifact,
            diagnostics: Vec::new(),
        })
    }
}

fn failed_analysis(diagnostic: Diagnostic) -> CobolAnalysis {
    CobolAnalysis {
        syntax: None,
        semantic: None,
        hir: None,
        hir_text: None,
        diagnostics: vec![diagnostic],
        completeness: Completeness::Incomplete,
    }
}
fn diagnostic_for_syntax(problem: SyntaxProblem) -> Diagnostic {
    diagnostic(
        "MECOB0100",
        Phase::Source,
        FailureCategory::MalformedInput,
        problem.to_string(),
    )
}
fn diagnostic_for_semantic(problem: SemanticProblem) -> Diagnostic {
    diagnostic(
        "MECOB0101",
        Phase::Semantic,
        FailureCategory::MalformedInput,
        format!("COBOL semantic analysis failed: {problem:?}"),
    )
}
fn diagnostic_for_hir(problem: HirProblem) -> Diagnostic {
    let category = if problem == HirProblem::UnsupportedForm {
        FailureCategory::Unsupported
    } else {
        FailureCategory::MalformedInput
    };
    let message = match &problem {
        HirProblem::InvalidStatement { kind, line, detail } => {
            let target = kind.official_kind().map_or(kind.slug(), |official| {
                crate::generated::cobol_language::procedure_statement_descriptor(official).id
            });
            format!("COBOL {target} statement validation failed at procedure line {line}: {detail}")
        }
        _ => format!("COBOL HIR construction failed: {problem:?}"),
    };
    diagnostic("MECOB0102", Phase::Parse, category, message)
}
fn lower_problem(problem: LowerProblem) -> CompilerProblem {
    match problem {
        LowerProblem::UnsupportedConstruct => CompilerProblem::IncompleteStage,
        other => CompilerProblem::Legality(format!("COBOL lowering failed: {other:?}")),
    }
}
fn diagnostic(code: &str, phase: Phase, category: FailureCategory, message: String) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).expect("static diagnostic code"),
        Severity::Error,
        phase,
        category,
        Completeness::Incomplete,
        message,
        None,
        Redaction::Public,
        DiagnosticLimits::default(),
    )
    .expect("bounded static diagnostic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_compiler_api::{CompileOptions, CompileTarget};
    use mainframe_env_ir::{
        Attribute, CicsCondition, CicsEffectPlan, CicsOperandName, CicsOperandValue,
        CicsOutputName, CicsPlanCodecProblem, CicsPlanLimits, CicsPlanOperation, CodecLimits,
        DecimalExecutionPolicy, DecimalPlanLimits, StorageId, cics_executable_descriptor,
        decode_binary, decode_cics_effect_plan, decode_decimal_assignment_plan, encode_binary,
        encode_cics_effect_plan,
    };
    use mainframe_env_source::{
        LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn bundle(source: &str) -> SourceBundle {
        bundle_in_format(source, SourceFormat::Free)
    }

    #[test]
    fn bts_child_link_six_rows_lower_with_reserved_tags_and_reject_conflicting_wait() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. BTSP. DATA DIVISION. WORKING-STORAGE SECTION. 01 CHILD-X PIC X(16). 01 ANY-X PIC X(16). 01 CHANNEL-X PIC X(16). 01 AB-X PIC X(4). 01 STATUS-X PIC S9(9) COMP. 01 TIMEOUT-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS FETCH ANY(ANY-X) COMPSTATUS(STATUS-X) CHANNEL(CHANNEL-X) ABCODE(AB-X) NOSUSPEND END-EXEC. EXEC CICS FETCH CHILD(CHILD-X) COMPSTATUS(STATUS-X) TIMEOUT(TIMEOUT-X) END-EXEC. EXEC CICS FREE CHILD(CHILD-X) END-EXEC. EXEC CICS LINK ACQACTIVITY END-EXEC. EXEC CICS LINK ACQPROCESS END-EXEC. EXEC CICS LINK ACTIVITY('SUBTASK') INPUTEVENT('GO') END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.unwrap();
        let plans = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            plans.iter().map(|plan| plan.operation).collect::<Vec<_>>(),
            vec![
                CicsPlanOperation::FetchAny,
                CicsPlanOperation::FetchChild,
                CicsPlanOperation::FreeChild,
                CicsPlanOperation::LinkAcqActivity,
                CicsPlanOperation::LinkAcqProcess,
                CicsPlanOperation::LinkActivity,
            ]
        );
        for (plan, tag) in plans.iter().zip(216..=221) {
            let bytes = encode_cics_effect_plan(plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), tag);
        }
        assert!(
            plans[0]
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::BtsNoSuspend)
        );
        assert!(
            plans[1]
                .operands
                .iter()
                .any(|operand| operand.name == CicsOperandName::BtsTimeout)
        );
        let invalid = source.replace(
            "COMPSTATUS(STATUS-X) TIMEOUT(TIMEOUT-X)",
            "COMPSTATUS(STATUS-X) TIMEOUT(TIMEOUT-X) NOSUSPEND",
        );
        let rejected = CobolCompiler::default().analyze(&bundle(&invalid));
        assert!(!rejected.diagnostics.is_empty());
    }

    fn bundle_in_format(source: &str, format: SourceFormat) -> SourceBundle {
        bundle_with_options(source, format, BTreeMap::new())
    }

    fn bundle_with_options(
        source: &str,
        format: SourceFormat,
        options: BTreeMap<String, String>,
    ) -> SourceBundle {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("HELLO.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "HELLO.cbl",
            source.as_bytes().to_vec(),
            format,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        SourceBundle::new(&path, vec![file], options, Vec::new(), limits).unwrap()
    }
    fn request(source: &str, mode: CompilationMode) -> CompilerRequest {
        CompilerRequest {
            source: bundle(source),
            mode,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        }
    }
    const HELLO: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MSG PIC X(5) VALUE 'HELLO'.\nPROCEDURE DIVISION.\nDISPLAY MSG.\nSTOP RUN.\n";

    fn analysis_with_unknown_hir_operation(
        compiler: &CobolCompiler,
        source: &SourceBundle,
    ) -> CobolAnalysis {
        use mainframe_env_ir::{IrLimits, ModuleBuilder, OperationIdentity};
        let mut analysis = compiler.analyze(source);
        assert_eq!(analysis.completeness, Completeness::Complete);
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("hardening.invalid", "unregistered", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        analysis.hir.as_mut().unwrap().module = builder.finish().unwrap();
        analysis
    }

    #[test]
    fn hardening_52_failed_hir_verification_cannot_publish_an_executable() {
        let compiler = CobolCompiler::default();
        let request = request(HELLO, CompilationMode::Executable);
        let analysis = analysis_with_unknown_hir_operation(&compiler, &request.source);
        assert!(matches!(
            compiler.compile_analyzed(request, analysis),
            Err(CompilerProblem::Verification(_))
        ));
    }

    #[test]
    fn hardening_52_analysis_reports_failed_verification_instead_of_complete_without_hir() {
        let compiler = CobolCompiler::default();
        let request = request(HELLO, CompilationMode::Analyze);
        let analysis = analysis_with_unknown_hir_operation(&compiler, &request.source);
        let CompilerResult::Analysis {
            hir,
            diagnostics,
            completeness,
        } = compiler.compile_analyzed(request, analysis).unwrap()
        else {
            panic!("analysis must not publish an executable");
        };
        assert!(hir.is_none());
        assert_eq!(completeness, Completeness::Failed);
        assert!(diagnostics.iter().any(|d| d.code().as_str() == "MECOB0300"));
    }

    #[test]
    fn hardening_52_valid_analysis_retains_its_verified_hir() {
        let CompilerResult::Analysis {
            hir, completeness, ..
        } = CobolCompiler::default()
            .compile(request(HELLO, CompilationMode::Analyze))
            .unwrap()
        else {
            panic!("expected analysis result");
        };
        assert!(hir.is_some());
        assert_eq!(completeness, Completeness::Complete);
    }
    #[test]
    fn hello_publishes_legal_mir() {
        let result = CobolCompiler::default()
            .compile(request(HELLO, CompilationMode::Executable))
            .unwrap();
        match result {
            CompilerResult::Published { artifact, .. } => assert!(!artifact.payload().is_empty()),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn arithmetic_mode_is_embedded_in_manifest_and_executable_payload() {
        let source = "PROCESS ARITH(COMPAT)\nIDENTIFICATION DIVISION. PROGRAM-ID. COMPAT. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 1. 01 B PIC 99 VALUE 2. PROCEDURE DIVISION. ADD A TO B. STOP RUN.";
        let result = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("ARITH(COMPAT) did not publish: {result:?}");
        };
        assert_eq!(
            artifact
                .manifest()
                .options
                .values()
                .get("cobol.effective-arith")
                .map(String::as_str),
            Some("compatible")
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let config = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find(|operation| operation.identity.name() == "config")
            .expect("runtime config operation");
        assert_eq!(
            config.attributes.get("arithmetic_mode"),
            Some(&Attribute::Text("compatible".into()))
        );
        let assignment = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find(|operation| operation.identity.namespace() == "mainframe.decimal")
            .expect("typed decimal operation");
        let Attribute::Bytes(bytes) = &assignment.attributes["assignment_plan"] else {
            panic!("typed decimal plan bytes")
        };
        let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default()).unwrap();
        assert_eq!(plan.policy, DecimalExecutionPolicy::decimal18_v1());
    }
    #[test]
    fn display_sign_is_embedded_in_manifest_and_executable_payload() {
        let source = format!("PROCESS DISPSIGN(SEP)\n{HELLO}");
        let result = CobolCompiler::default()
            .compile(request(&source, CompilationMode::Executable))
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("DISPSIGN(SEP) did not publish: {result:?}");
        };
        assert_eq!(
            artifact
                .manifest()
                .options
                .values()
                .get("cobol.effective-dispsign")
                .map(String::as_str),
            Some("separate")
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let config = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find(|operation| operation.identity.name() == "config")
            .expect("runtime config operation");
        assert_eq!(
            config.attributes.get("display_sign"),
            Some(&Attribute::Text("separate".into()))
        );
    }
    #[test]
    fn exec_sql_publishes_through_typed_host_lowering() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLTEST. PROCEDURE DIVISION. EXEC SQL SELECT 1 END-EXEC. STOP RUN.";
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn sql_include_precompile_resolves_data_and_procedure_members() {
        let limits = SourceLimits::default();
        let primary_path = LogicalPath::new("SQLMAIN.cbl", limits.max_path_bytes).unwrap();
        let dcl_path = LogicalPath::new("DCLROW.dcl", limits.max_path_bytes).unwrap();
        let procedure_path = LogicalPath::new("SQLPROC.cpy", limits.max_path_bytes).unwrap();
        let primary = SourceFile::input(
            primary_path.as_str(),
            b"IDENTIFICATION DIVISION. PROGRAM-ID. SQLMAIN. DATA DIVISION. WORKING-STORAGE SECTION. EXEC SQL INCLUDE DCLROW END-EXEC. 01 KEY-X PIC X(2) VALUE '01'. PROCEDURE DIVISION. EXEC SQL INCLUDE SQLPROC END-EXEC. PERFORM SQL-READ. STOP RUN."
                .to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let dcl = SourceFile::input(
            dcl_path.as_str(),
            b"01 DCL-ROW. 05 DCL-COL PIC X(2).".to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let procedure = SourceFile::input(
            procedure_path.as_str(),
            b"SQL-READ. EXEC SQL SELECT COL INTO :DCL-COL FROM CARDDEMO.T WHERE COL = :KEY-X END-EXEC. EXIT."
                .to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let library =
            SourceLibrary::new("sql-includes", vec![dcl_path, procedure_path], limits).unwrap();
        let source = SourceBundle::with_libraries(
            &primary_path,
            vec![primary, dcl, procedure],
            vec![library],
            BTreeMap::from([("cobol.sql-precompile".into(), "true".into())]),
            Vec::new(),
            limits,
        )
        .unwrap();
        let analysis = CobolCompiler::default().analyze(&source);
        assert!(analysis.semantic.as_ref().is_some_and(|semantic| {
            semantic
                .layouts
                .iter()
                .any(|layout| layout.name == "DCL-COL")
        }));
        assert!(analysis.hir.as_ref().is_some_and(|hir| {
            hir.statements.iter().all(|statement| {
                statement.kind != crate::StatementKind::ExecSql
                    || !statement.arguments.iter().any(|token| token == "INCLUDE")
            })
        }));
        assert!(matches!(
            CobolCompiler::default()
                .compile(CompilerRequest {
                    source,
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference").unwrap(),
                    options: CompileOptions::new(BTreeMap::new()).unwrap(),
                })
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }
    #[test]
    fn recovered_duplicate_paragraph_never_publishes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DUPTEST. PROCEDURE DIVISION. DUP. DISPLAY 'A'. DUP. EXIT. STOP RUN.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(analysis.hir.as_ref().is_some_and(|hir| {
            hir.statements
                .iter()
                .any(|statement| statement.kind == crate::StatementKind::DuplicateLabel)
        }));
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Failed {
                completeness: Completeness::Unsupported,
                ..
            }
        ));
    }
    #[test]
    fn multiline_structured_control_publishes_through_cfg_lowering() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. PROCEDURE DIVISION. IF A = 1\n DISPLAY 'YES'\nEND-IF. STOP RUN.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(analysis.hir.is_some());
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }
    #[test]
    fn analysis_retains_lossless_syntax() {
        let analysis = CobolCompiler::default().analyze(&bundle(HELLO));
        assert_eq!(analysis.syntax.as_ref().unwrap().text(), HELLO);
        let semantic = analysis.semantic.as_ref().unwrap();
        assert!(
            semantic
                .divisions
                .iter()
                .all(|node| !node.source.is_empty())
        );
        assert!(semantic.scopes.iter().all(|node| !node.source.is_empty()));
        assert!(
            semantic
                .data_descriptions
                .iter()
                .all(|node| !node.source.is_empty()
                    && node.clauses.iter().all(|clause| !clause.source.is_empty()))
        );
        assert!(
            semantic
                .layouts
                .iter()
                .all(|layout| !layout.source.is_empty())
        );
        assert_eq!(analysis.completeness, Completeness::Complete);
    }

    #[test]
    fn lp64_types_and_bounded_dynamic_layouts_publish() {
        let source = "PROCESS LP(64)\nIDENTIFICATION DIVISION. PROGRAM-ID. STORAGE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. 01 OBJ OBJECT REFERENCE. 01 DYNAMIC-ITEM PIC X DYNAMIC LENGTH LIMIT IS 64. PROCEDURE DIVISION. STOP RUN.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("semantic metadata");
        assert_eq!(semantic.layout("PTR").unwrap().length, 8);
        assert_eq!(semantic.layout("OBJ").unwrap().length, 8);
        assert_eq!(
            semantic.layout("DYNAMIC-ITEM").unwrap().dynamic_limit,
            Some(64)
        );
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn layout_abi_validation_preserves_national_dynamic_utf8_and_float_tables() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LAYOUTABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 NATIONAL-X PIC 9(3) USAGE NATIONAL. 01 DYNAMIC-U PIC U DYNAMIC LENGTH LIMIT IS 100 UTF-8. 01 FLOAT-TABLE. 05 FLOAT-X OCCURS 2 TIMES COMP-1. PROCEDURE DIVISION. STOP RUN.";
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn unsupported_dynamic_table_and_alias_forms_fail_before_publication() {
        for declarations in [
            "01 ROOT-X. 05 DYN-X PIC X OCCURS 2 TIMES INDEXED BY IX DYNAMIC LENGTH LIMIT IS 100.",
            "01 ROOT-X. 05 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 100 OCCURS 2 TIMES INDEXED BY IX.",
            "01 ROOT-X OCCURS 2 TIMES. 05 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 100.",
            "01 BASE-X PIC X DYNAMIC LENGTH LIMIT IS 100. 01 VIEW-X REDEFINES BASE-X PIC X.",
            "01 BASE-X PIC X. 01 DYN-X REDEFINES BASE-X PIC X DYNAMIC LENGTH LIMIT IS 100.",
            "01 ROOT-X. 05 BASE-X. 10 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 100. 05 VIEW-X REDEFINES BASE-X. 10 FIX-X PIC X.",
            "01 ROOT-X. 05 BASE-X PIC X. 05 VIEW-X REDEFINES BASE-X. 10 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 100.",
            "01 ROOT-X. 05 BASE-X. 10 DYN-A PIC X DYNAMIC LENGTH LIMIT IS 100. 05 VIEW-X REDEFINES BASE-X. 10 DYN-B PIC X DYNAMIC LENGTH LIMIT IS 100.",
            "01 ROOT-X. 05 DYN-X PIC X OCCURS 1 TIMES DYNAMIC LENGTH LIMIT IS 100.",
            "01 ROOT-X. 05 TABLE-X OCCURS 1 TIMES. 10 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 100.",
        ] {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. DYNSCOPE. DATA DIVISION. WORKING-STORAGE SECTION. {declarations} PROCEDURE DIVISION. STOP RUN."
            );
            let analysis = CobolCompiler::default().analyze(&bundle(&source));
            assert!(analysis.semantic.is_none());
            assert!(analysis.hir.is_none());
            assert!(matches!(
                CobolCompiler::default()
                    .compile(request(&source, CompilationMode::Executable))
                    .unwrap(),
                CompilerResult::Failed { .. }
            ));
        }
    }

    #[test]
    fn type_instances_retain_definition_and_instance_provenance() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. TYPES.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PART-T TYPEDEF.\n 05 CODE-X PIC X(2).\n 05 QTY-X PIC 9(3) COMP-3.\n01 PART TYPE PART-T.\nPROCEDURE DIVISION.\nSTOP RUN.\n";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let semantic = analysis.semantic.expect("semantic metadata");
        assert!(
            semantic
                .layouts
                .iter()
                .all(|layout| !layout.source.is_empty())
        );
        assert!(
            semantic
                .layout("PART.CODE-X")
                .is_some_and(|layout| layout.source.len() >= 2)
        );
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("the allocated TYPE instance must publish");
        };
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let definition_names = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| {
                operation.identity.namespace() == "mainframe.core.cobol"
                    && operation.identity.name() == "define"
            })
            .filter_map(|operation| match operation.attributes.get("name") {
                Some(Attribute::Text(name)) => Some(name.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert!(definition_names.contains("PART"));
        assert!(definition_names.contains("PART.CODE-X"));
        assert!(
            definition_names
                .iter()
                .all(|name| !name.starts_with("PART-T"))
        );
        assert!(
            module
                .storage()
                .iter()
                .all(|region| !region.name.to_ascii_uppercase().starts_with("PART-T"))
        );
    }

    #[test]
    fn intrinsic_and_special_register_nodes_are_typed_and_publishable() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCTIONS.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TEXT-X PIC X(8) VALUE 'ABC'.\n01 NUM-X PIC 9(3)V99 COMP-3 VALUE 1.5.\n01 INT-X PIC 9(4) BINARY.\nPROCEDURE DIVISION.\nMOVE FUNCTION LOWER-CASE(TEXT-X) TO TEXT-X.\nMOVE FUNCTION ABS(NUM-X) TO NUM-X.\nMOVE LENGTH OF TEXT-X TO INT-X.\nDISPLAY WHEN-COMPILED.\nSTOP RUN.\n";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("function semantic model");
        assert_eq!(semantic.intrinsic_calls.len(), 2);
        assert!(
            semantic
                .intrinsic_calls
                .iter()
                .all(|call| !call.source.is_empty())
        );
        assert_eq!(semantic.special_registers.len(), 2);
        assert!(
            semantic
                .special_registers
                .iter()
                .all(|register| !register.source.is_empty())
        );
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MECOB0202")
        );
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MECOB0203")
        );
        assert!(matches!(
            CobolCompiler::default()
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn statement_hir_retains_generated_identity_options_and_source() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. OPTIONS.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT INPUT-FILE ASSIGN TO INPUTDD.\nDATA DIVISION.\nFILE SECTION.\nFD INPUT-FILE.\n01 INPUT-RECORD PIC X(8).\nPROCEDURE DIVISION.\nREAD INPUT-FILE INTO INPUT-RECORD AT END CONTINUE END-READ.\nSTOP RUN.\n";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let hir = analysis.hir.expect("valid statement HIR");
        let statement = hir
            .statements
            .iter()
            .find(|statement| statement.kind == crate::StatementKind::Read)
            .expect("typed READ statement");
        assert_eq!(
            statement.official,
            Some(crate::ProcedureStatementKind::Read)
        );
        assert!(
            [
                crate::StatementOptionKind::AtEnd,
                crate::StatementOptionKind::Into,
                crate::StatementOptionKind::ExplicitTerminator,
            ]
            .into_iter()
            .all(|kind| statement.options.iter().any(|option| option.kind == kind))
        );
        assert_eq!(
            statement
                .options
                .iter()
                .find(|option| option.kind == crate::StatementOptionKind::Into)
                .map(|option| option.operands.as_slice()),
            Some(["INPUT-RECORD".to_string()].as_slice())
        );
        assert!(statement.source.iter().all(|span| {
            span.source.as_str() == "HELLO.cbl" && span.source_start < span.source_end
        }));
        assert!(!statement.source.is_empty());
    }

    #[test]
    fn delete_and_relational_start_publish_through_the_single_core_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FILEOPS. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS DYNAMIC RECORD KEY IS REC-KEY FILE STATUS IS FILE-STATUS. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-RECORD. 05 REC-KEY PIC X(2). 05 REC-VALUE PIC X(6). WORKING-STORAGE SECTION. 01 FILE-STATUS PIC XX. PROCEDURE DIVISION. START TEST-FILE KEY IS NOT LESS THAN REC-KEY INVALID KEY CONTINUE END-START. DELETE TEST-FILE RECORD INVALID KEY CONTINUE END-DELETE. STOP RUN.";
        let result = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap();
        assert!(
            matches!(result, CompilerResult::Published { .. }),
            "{result:?}"
        );
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let hir = analysis.hir.expect("typed HIR");
        assert!(
            [crate::StatementKind::Start, crate::StatementKind::Delete]
                .into_iter()
                .all(|kind| hir
                    .statements
                    .iter()
                    .any(|statement| statement.kind == kind))
        );
    }

    #[test]
    fn sort_merge_release_and_return_publish_with_sd_runtime_metadata() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SORTOPS. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT INPUT-FILE ASSIGN TO INPUTDD ORGANIZATION IS SEQUENTIAL. SELECT OUTPUT-FILE ASSIGN TO OUTPUTDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD PIC X(8). FD OUTPUT-FILE. 01 OUTPUT-RECORD PIC X(8). SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(6). PROCEDURE DIVISION. SORT SORT-FILE ON ASCENDING KEY SORT-KEY USING INPUT-FILE GIVING OUTPUT-FILE. MERGE SORT-FILE ON ASCENDING KEY SORT-KEY USING INPUT-FILE GIVING OUTPUT-FILE. RELEASE SORT-RECORD FROM INPUT-RECORD. RETURN SORT-FILE RECORD INTO OUTPUT-RECORD AT END CONTINUE END-RETURN. STOP RUN.";
        let result = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap();
        assert!(
            matches!(result, CompilerResult::Published { .. }),
            "{result:?}"
        );
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let semantic = analysis.semantic.expect("typed semantic model");
        assert!(semantic.files.iter().any(|file| {
            file.select_name == "SORT-FILE"
                && file.record_name.as_deref() == Some("SORT-RECORD")
                && file.sort_merge
        }));
    }

    #[test]
    fn same_line_and_inline_hir_has_exact_provenance_in_all_source_formats() {
        let lines = [
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. BOUNDARIES.",
            "DATA DIVISION.",
            "WORKING-STORAGE SECTION.",
            "01 A PIC 9 VALUE 1.",
            "01 B PIC 9 VALUE 2.",
            "PROCEDURE DIVISION.",
            "MOVE A TO B DISPLAY B.",
            "IF A = B DISPLAY 'YES' ELSE DISPLAY 'NO' END-IF.",
            "STOP RUN.",
        ];
        for format in [
            SourceFormat::Free,
            SourceFormat::Fixed,
            SourceFormat::Variable,
        ] {
            let source = if format == SourceFormat::Free {
                lines.join("\n")
            } else {
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| format!("{:06} {line}\n", index + 1))
                    .collect::<String>()
            };
            let analysis = CobolCompiler::default().analyze(&bundle_in_format(&source, format));
            let hir = analysis.hir.expect("valid bounded statement HIR");
            assert_eq!(
                hir.statements
                    .iter()
                    .map(|statement| statement.kind)
                    .collect::<Vec<_>>(),
                [
                    crate::StatementKind::Move,
                    crate::StatementKind::Display,
                    crate::StatementKind::If,
                    crate::StatementKind::Display,
                    crate::StatementKind::Display,
                    crate::StatementKind::StopRun,
                    crate::StatementKind::ProgramEnd,
                ],
                "{format:?}"
            );
            let move_statement = &hir.statements[0];
            let display_statement = &hir.statements[1];
            assert!(
                !move_statement
                    .arguments
                    .iter()
                    .any(|token| token == "DISPLAY")
            );
            assert!(
                move_statement.source.last().unwrap().source_end
                    <= display_statement.source.first().unwrap().source_start
            );
            assert!(
                hir.statements
                    .iter()
                    .filter(|statement| statement.official.is_some())
                    .all(|statement| !statement.source.is_empty())
            );
            assert!(hir.control_nodes.iter().all(|node| !node.source.is_empty()));
        }
    }

    #[test]
    fn invalid_statement_grammar_precedes_execution_profile_diagnostics() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. BADSTART. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED RECORD KEY IS A. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-RECORD. 05 A PIC 9. PROCEDURE DIVISION. START TEST-FILE BOGUS. STOP RUN.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MECOB0102"
                && diagnostic.phase() == Phase::Parse
                && diagnostic.category() == FailureCategory::MalformedInput
                && diagnostic.public_message().contains("start")
        }));
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MECOB0200")
        );
    }

    #[test]
    fn effective_lp_is_shared_by_directives_layouts_and_manifest_identity() {
        let body = "IDENTIFICATION DIVISION. PROGRAM-ID. LPTEST. DATA DIVISION. WORKING-STORAGE SECTION. 01 IDX INDEX. 01 PTR POINTER. 01 OBJ OBJECT REFERENCE. PROCEDURE DIVISION. STOP RUN.";
        let cases = [
            (
                body.to_string(),
                BTreeMap::from([("cobol.lp".into(), "64".into())]),
            ),
            (format!("PROCESS LP(64)\n{body}"), BTreeMap::new()),
            (
                format!("PROCESS LP(64)\n{body}"),
                BTreeMap::from([("cobol.lp".into(), "64".into())]),
            ),
        ];
        for (source, options) in cases {
            let bundle = bundle_with_options(&source, SourceFormat::Free, options);
            let analysis = CobolCompiler::default().analyze(&bundle);
            let syntax = analysis.syntax.as_ref().expect("LP syntax");
            let semantic = analysis.semantic.as_ref().expect("LP semantic model");
            assert_eq!(syntax.effective_compiler_options().lp(), 64);
            for name in ["IDX", "PTR", "OBJ"] {
                assert_eq!(semantic.layout(name).unwrap().length, 8, "{name}");
            }
        }

        let conditional = ">>IF IGY-LP = 64\nIDENTIFICATION DIVISION.\nPROGRAM-ID. LP64.\n>>ELSE\nIDENTIFICATION DIVISION.\nPROGRAM-ID. LP32.\n>>END-IF\nPROCEDURE DIVISION.\nSTOP RUN.\n";
        let bundle = bundle_with_options(
            conditional,
            SourceFormat::Free,
            BTreeMap::from([("cobol.lp".into(), "64".into())]),
        );
        let analysis = CobolCompiler::default().analyze(&bundle);
        assert_eq!(analysis.semantic.as_ref().unwrap().program_id, "LP64");

        let result = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("effective LP source did not publish: {result:?}");
        };
        assert_eq!(
            artifact
                .manifest()
                .options
                .values()
                .get("cobol.effective-lp")
                .map(String::as_str),
            Some("64")
        );

        let conflicting = bundle_with_options(
            &format!("PROCESS LP(32)\n{body}"),
            SourceFormat::Free,
            BTreeMap::from([("cobol.lp".into(), "64".into())]),
        );
        let analysis = CobolCompiler::default().analyze(&conflicting);
        assert!(analysis.hir.is_none());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MECOB0100"
                && diagnostic
                    .public_message()
                    .contains("ConflictingCompilerOption")
        }));
    }

    #[test]
    fn when_compiled_register_is_typed_and_publishes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REGISTER. PROCEDURE DIVISION. DISPLAY WHEN-COMPILED. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("register semantic model");
        assert!(semantic.special_registers.iter().any(|register| {
            register.kind == crate::SpecialRegisterKind::WhenCompiled && register.runtime_supported
        }));
        assert!(analysis.hir.is_some());
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(analysis.diagnostics.is_empty());
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn comp1_and_comp2_publish_through_the_owned_ieee_adapter() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FLOATS. DATA DIVISION. WORKING-STORAGE SECTION. 01 SHORT-X COMP-1. 01 LONG-X COMP-2. PROCEDURE DIVISION. DISPLAY SHORT-X LONG-X. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("float semantic model");
        assert_eq!(
            semantic.layout("SHORT-X").unwrap().category,
            crate::DataCategory::FloatShort
        );
        assert_eq!(
            semantic.layout("LONG-X").unwrap().category,
            crate::DataCategory::FloatLong
        );
        assert!(analysis.hir.is_some());
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn completed_state_survives_zero_and_exhausted_diagnostic_caps() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CAPS. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLOAT-X COMP-1. PROCEDURE DIVISION. DISPLAY WHEN-COMPILED FLOAT-X. STOP RUN.";
        for max_diagnostics in [0, 1] {
            let compiler = CobolCompiler::new(CobolCompilerLimits {
                max_diagnostics,
                ..CobolCompilerLimits::default()
            });
            let analysis = compiler.analyze(&bundle(source));
            assert!(analysis.diagnostics.is_empty());
            assert_eq!(analysis.completeness, Completeness::Complete);
            assert!(analysis.hir.is_some());
            assert!(matches!(
                compiler
                    .compile(request(source, CompilationMode::Analyze))
                    .unwrap(),
                CompilerResult::Analysis {
                    hir: Some(_),
                    completeness: Completeness::Complete,
                    ..
                }
            ));
            assert!(matches!(
                compiler
                    .compile(request(source, CompilationMode::Executable))
                    .unwrap(),
                CompilerResult::Published { .. }
            ));
        }
    }

    #[test]
    fn malformed_present_lp_values_fail_instead_of_defaulting_to_lp32() {
        let body = "IDENTIFICATION DIVISION. PROGRAM-ID. BADLP. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. PROCEDURE DIVISION. STOP RUN.";
        let cases = [
            bundle(&format!("PROCESS LP(BOGUS)\n{body}")),
            bundle_with_options(
                body,
                SourceFormat::Free,
                BTreeMap::from([("cobol.lp".into(), "BOGUS".into())]),
            ),
        ];
        for source in cases {
            let analysis = CobolCompiler::default().analyze(&source);
            assert!(analysis.syntax.is_none());
            assert!(analysis.semantic.is_none());
            assert!(analysis.hir.is_none());
            assert_eq!(analysis.completeness, Completeness::Incomplete);
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code().as_str() == "MECOB0100"
                    && diagnostic
                        .public_message()
                        .contains("InvalidCompilerOption")
            }));
        }
    }

    #[test]
    fn multiple_arithmetic_receivers_publish_through_the_owned_runtime_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MULTIRECV. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. 01 B PIC 9 VALUE 2. 01 C PIC 9 VALUE 3. PROCEDURE DIVISION. ADD A TO B C. SUBTRACT A FROM B C. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis.hir.as_ref().expect("typed multi-receiver HIR");
        for kind in [crate::StatementKind::Add, crate::StatementKind::Subtract] {
            assert!(hir.statements.iter().any(|statement| {
                statement.kind == kind && statement.official == kind.official_kind()
            }));
        }
        assert_eq!(analysis.completeness, Completeness::Complete);
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Published { .. }
        ));
    }

    #[test]
    fn typed_arithmetic_is_proof_bound_and_publishes_one_atomic_plan_per_statement() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. PLAN. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 1. 01 B PIC 99 VALUE 2. 01 C PIC 99 VALUE 3. 01 LEFT-G. 05 X PIC 99 VALUE 4. 05 Y PIC 99 VALUE 5. 01 RIGHT-G. 05 X PIC 99 VALUE 6. 05 Y PIC 99 VALUE 7. PROCEDURE DIVISION. ADD A TO B C ROUNDED. ADD CORRESPONDING LEFT-G TO RIGHT-G. COMPUTE C = ( A + B ) * 2. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis.hir.as_ref().expect("typed arithmetic HIR");
        let hir_operations = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .collect::<Vec<_>>();
        let hir_dialects = hir_operations
            .iter()
            .map(|operation| {
                format!(
                    "{}@{}",
                    operation.identity.namespace(),
                    operation.identity.major()
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            hir_dialects,
            BTreeSet::from(["cobol.hir@1".into(), "cobol.hir@2".into()])
        );
        let typed_hir = hir_operations
            .iter()
            .copied()
            .filter(|operation| operation.identity.major() == 2)
            .collect::<Vec<_>>();
        assert_eq!(typed_hir.len(), 3);
        for operation in typed_hir {
            assert_eq!(operation.identity.namespace(), "cobol.hir");
            assert!(matches!(operation.identity.name(), "add" | "compute"));
            assert!(!operation.attributes.contains_key("arguments"));
            assert!(operation.location.is_some());
            assert_eq!(
                operation
                    .storage
                    .iter()
                    .map(|reference| reference.storage)
                    .collect::<BTreeSet<_>>()
                    .len(),
                operation.storage.len()
            );
            assert!(
                operation
                    .storage
                    .iter()
                    .all(|reference| reference.offset == 0 && reference.length > 0)
            );
            let Attribute::Bytes(bytes) = &operation.attributes["assignment_plan"] else {
                panic!("typed HIR plan bytes")
            };
            let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default()).unwrap();
            assert_eq!(
                plan.semantic_origin,
                if operation.identity.name() == "compute" {
                    "cobol.compute@1"
                } else {
                    "cobol.add@1"
                }
            );
            assert_eq!(plan.policy, DecimalExecutionPolicy::decimal34_v1());
            assert_eq!(
                plan.assignments.len(),
                if operation.identity.name() == "compute" {
                    1
                } else {
                    2
                }
            );
            for forbidden in [b"TO".as_slice(), b"GIVING", b"ROUNDED"] {
                assert!(
                    !bytes
                        .windows(forbidden.len())
                        .any(|window| window == forbidden)
                );
            }
        }

        let CompilerResult::Published { artifact, .. } = compiler
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published typed arithmetic")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from([
                "mainframe.core.cobol@1".into(),
                "mainframe.decimal@2".into(),
            ])
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let assignments = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| {
                operation.identity.namespace() == "mainframe.decimal"
                    && operation.identity.name() == "assign"
                    && operation.identity.major() == 2
            })
            .collect::<Vec<_>>();
        assert_eq!(assignments.len(), 3);
        for operation in assignments {
            assert!(!operation.attributes.contains_key("arguments"));
            assert!(!operation.attributes.contains_key("control_text"));
            assert_eq!(
                operation.attributes.get("typed_condition_status"),
                Some(&Attribute::Text("cobol.arithmetic-size-error@1".into()))
            );
            assert_eq!(
                operation.attributes.get("typed_condition_branches"),
                Some(&Attribute::Integer(0))
            );
            assert!(operation.location.is_some());
            let Attribute::Bytes(bytes) = &operation.attributes["assignment_plan"] else {
                panic!("typed executable plan bytes")
            };
            let plan = decode_decimal_assignment_plan(bytes, DecimalPlanLimits::default()).unwrap();
            assert!(matches!(
                plan.semantic_origin.as_str(),
                "cobol.add@1" | "cobol.compute@1"
            ));
            assert_eq!(plan.policy, DecimalExecutionPolicy::decimal34_v1());
            assert_eq!(
                plan.assignments.len(),
                if plan.semantic_origin == "cobol.compute@1" {
                    1
                } else {
                    2
                }
            );
            for forbidden in [b"TO".as_slice(), b"GIVING", b"ROUNDED"] {
                assert!(
                    !bytes
                        .windows(forbidden.len())
                        .any(|window| window == forbidden)
                );
            }
        }
    }

    #[test]
    fn typed_size_error_edges_are_bound_without_branch_text() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SIZEEDGE. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. 01 B PIC 9 VALUE 0. PROCEDURE DIVISION. COMPUTE B = A / B ON SIZE ERROR CONTINUE NOT ON SIZE ERROR CONTINUE END-COMPUTE. STOP RUN.";
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published typed size-error control")
        };
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let operations = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .collect::<Vec<_>>();
        let assignment = operations
            .iter()
            .copied()
            .find(|operation| operation.identity.namespace() == "mainframe.decimal")
            .expect("typed decimal assignment");
        assert_eq!(
            assignment.attributes.get("typed_condition_status"),
            Some(&Attribute::Text("cobol.arithmetic-size-error@1".into()))
        );
        assert_eq!(
            assignment.attributes.get("typed_condition_branches"),
            Some(&Attribute::Integer(3))
        );
        assert!(!assignment.attributes.contains_key("control_text"));
        let owner = match assignment.attributes.get("control_node") {
            Some(Attribute::Integer(owner)) => *owner,
            _ => panic!("typed condition owner"),
        };
        let branches = operations
            .iter()
            .copied()
            .filter(|operation| {
                operation.attributes.get("control_parent") == Some(&Attribute::Integer(owner))
                    && operation.attributes.get("control_role")
                        == Some(&Attribute::Text("branch".into()))
            })
            .collect::<Vec<_>>();
        assert_eq!(branches.len(), 2);
        assert_eq!(
            branches
                .iter()
                .map(|branch| branch.attributes.get("typed_condition_polarity"))
                .collect::<Vec<_>>(),
            vec![
                Some(&Attribute::Boolean(true)),
                Some(&Attribute::Boolean(false)),
            ]
        );
        assert!(branches.iter().all(|branch| {
            branch.attributes.get("typed_condition_status")
                == Some(&Attribute::Text("cobol.arithmetic-size-error@1".into()))
                && branch.attributes.contains_key("edge_branch_false")
        }));
        let schema = crate::core_mir_catalog()
            .get(&assignment.identity)
            .cloned()
            .expect("typed decimal schema");
        assert!(
            schema
                .required_attributes
                .contains("typed_condition_status")
        );
        assert!(
            schema
                .required_attributes
                .contains("typed_condition_branches")
        );
    }

    #[test]
    fn route_compiles_full_bms_list_and_timing_into_v2_tag() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ROUTEP. DATA DIVISION. WORKING-STORAGE SECTION. 01 LIST-X PIC X(16). 01 TITLE-X PIC X(12). 01 RC PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ROUTE LIST(LIST-X) TITLE(TITLE-X) INTERVAL(0) REQID('**') RESP(RC) END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.unwrap();
        let plan = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(plan.operation, CicsPlanOperation::Route);
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(u16::from_be_bytes([encoded[6], encoded[7]]), 87);
        let delayed = source.replace("INTERVAL(0)", "AFTER SECONDS(5) NLEOM");
        let delayed = CobolCompiler::default().analyze(&bundle(&delayed));
        assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
        let bad = source.replace("INTERVAL(0)", "INTERVAL(0) TIME(120000)");
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(&bad))
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn issue_family_compiles_ten_typed_v2_plans_and_rejects_bad_selection() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. OUTBD. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(4). 01 RID-X PIC S9(9) COMP. 01 LEN-X PIC S9(4) COMP VALUE 4. 01 PTR-X POINTER-32. 01 RC PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ISSUE ADD DESTID('DISK1') FROM(DATA-X) LENGTH(4) RESP(RC) END-EXEC. EXEC CICS ISSUE QUERY DESTID('DISK1') END-EXEC. EXEC CICS ISSUE RECEIVE INTO(DATA-X) LENGTH(LEN-X) END-EXEC. EXEC CICS ISSUE NOTE DESTID('REL1') RIDFLD(RID-X) RRN END-EXEC. EXEC CICS ISSUE ERASE DESTID('REL1') RIDFLD(RID-X) RRN END-EXEC. EXEC CICS ISSUE REPLACE DESTID('REL1') FROM(DATA-X) LENGTH(4) RIDFLD(RID-X) RRN END-EXEC. EXEC CICS ISSUE SEND CONSOLE FROM(DATA-X) LENGTH(4) NOWAIT END-EXEC. EXEC CICS ISSUE WAIT CONSOLE END-EXEC. EXEC CICS ISSUE END DESTID('DISK1') END-EXEC. EXEC CICS ISSUE ABORT DESTID('REL1') END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.unwrap();
        let plans = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .filter(|plan| {
                matches!(
                    plan.operation,
                    CicsPlanOperation::IssueAbort
                        | CicsPlanOperation::IssueAdd
                        | CicsPlanOperation::IssueEnd
                        | CicsPlanOperation::IssueErase
                        | CicsPlanOperation::IssueNote
                        | CicsPlanOperation::IssueQuery
                        | CicsPlanOperation::IssueReceive
                        | CicsPlanOperation::IssueReplace
                        | CicsPlanOperation::IssueSend
                        | CicsPlanOperation::IssueWait
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(plans.len(), 10);
        for plan in plans {
            let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(&encoded[..6], b"MCEP\0\x02");
        }
        let bad = "IDENTIFICATION DIVISION. PROGRAM-ID. OUTBD. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(4). PROCEDURE DIVISION. EXEC CICS ISSUE SEND CONSOLE DESTID('DISK1') FROM(DATA-X) LENGTH(4) END-EXEC.";
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(bad))
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn send_partnset_compiles_named_and_base_forms_through_v2_plan() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. PARTNS. DATA DIVISION. WORKING-STORAGE SECTION. 01 PSET PIC X(5) VALUE 'PSET1'. 01 RC PIC S9(9) COMP. 01 RC2 PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS SEND PARTNSET(PSET) RESP(RC) RESP2(RC2) END-EXEC. EXEC CICS SEND PARTNSET END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.expect("typed SEND PARTNSET HIR");
        let plans = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .filter(|plan| plan.operation == CicsPlanOperation::SendPartnset)
            .collect::<Vec<_>>();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].operands[0].name, CicsOperandName::Partnset);
        assert!(plans[1].operands.is_empty());
        for plan in plans {
            let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(&encoded[..6], b"MCEP\0\x02");
            assert_eq!(
                decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
                plan
            );
        }
    }

    #[test]
    fn receive_partn_compiles_partition_length_and_asis_through_v2_plan() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RPARTN. DATA DIVISION. WORKING-STORAGE SECTION. 01 PARTN-X PIC X(2). 01 DATA-X PIC X(5). 01 LEN-X PIC S9(4) COMP VALUE 5. 01 RC PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS RECEIVE PARTN(PARTN-X) INTO(DATA-X) LENGTH(LEN-X) ASIS RESP(RC) END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.expect("typed RECEIVE PARTN HIR");
        let plan = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .expect("RECEIVE PARTN plan");
        assert_eq!(plan.operation, CicsPlanOperation::ReceivePartn);
        assert_eq!(plan.operands[0].name, CicsOperandName::Length);
        assert!(
            plan.options
                .contains(&mainframe_env_ir::CicsPlanOption::AsIs)
        );
        assert!(
            plan.outputs
                .iter()
                .any(|output| output.name == CicsOutputName::Partn)
        );
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(&encoded[..8], b"MCEP\0\x02\0V");
    }

    #[test]
    fn send_control_and_page_compile_bms_flags_and_values() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. BMSPAGE. DATA DIVISION. WORKING-STORAGE SECTION. 01 CUR-X PIC S9(4) COMP VALUE 7. 01 RC PIC S9(9) COMP. 01 PTR-X POINTER-32. 01 TRAILER-X PIC X(8). PROCEDURE DIVISION. EXEC CICS SEND CONTROL ERASE FREEKB CURSOR(CUR-X) RESP(RC) END-EXEC. EXEC CICS SEND CONTROL ACCUM PAGING REQID('**') ALARM END-EXEC. EXEC CICS SEND PAGE RETAIN NOAUTOPAGE RESP(RC) END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let hir = analysis.hir.expect("typed BMS control HIR");
        let plans = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .filter(|plan| {
                matches!(
                    plan.operation,
                    CicsPlanOperation::SendControl | CicsPlanOperation::SendPage
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].operation, CicsPlanOperation::SendControl);
        assert_eq!(plans[0].operands[0].name, CicsOperandName::ControlCursor);
        assert_eq!(plans[1].operation, CicsPlanOperation::SendControl);
        assert!(
            plans[1]
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::Accum)
        );
        assert_eq!(plans[2].operation, CicsPlanOperation::SendPage);
        assert!(
            plans[2]
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::RetainPage)
        );
        for (plan, tag) in plans.iter().zip([88, 88, 89]) {
            let encoded = encode_cics_effect_plan(plan, CicsPlanLimits::default()).unwrap();
            assert_eq!(u16::from_be_bytes([encoded[6], encoded[7]]), tag);
        }
    }

    #[test]
    fn typed_cics_is_proof_bound_in_hir_and_the_published_executable() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSP. DATA DIVISION. WORKING-STORAGE SECTION. 01 AB-CODE PIC X(4) VALUE 'B001'. 01 ABS-X PIC S9(15) COMP-3. 01 DATE-X PIC X(10). 01 TIME-X PIC X(8). 01 MS-X PIC S9(9) COMP. 01 RECORD-X PIC X(4). 01 KEY-X PIC X(3) VALUE '003'. 01 LOCK-X PIC X(4) VALUE 'LOCK'. 01 PTR-X POINTER-32. 01 CORR-X PIC X(80) VALUE ALL 'A'. 01 PRIORITY-X PIC S9(4) COMP VALUE 200. 01 CODE-A PIC S9(9) COMP. 01 CODE-B PIC S9(9) COMP. 01 EVENT-NAME PIC X(16). 01 SUBEVENT-NAME PIC X(16). 01 TOKEN-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ASKTIME END-EXEC. EXEC CICS ASKTIME ABSTIME(ABS-X) END-EXEC. EXEC CICS FORMATTIME ABSTIME(ABS-X) DATESEP('-') YYYYMMDD(DATE-X) TIMESEP(':') TIME(TIME-X) MILLISECONDS(MS-X) END-EXEC. EXEC CICS LINK PROGRAM('CHILD') COMMAREA(RECORD-X) END-EXEC. EXEC CICS XCTL PROGRAM('NEXT') COMMAREA(RECORD-X) END-EXEC. EXEC CICS STARTBR FILE('ACCTDAT') RIDFLD(KEY-X) END-EXEC. EXEC CICS RESETBR FILE('ACCTDAT') RIDFLD(KEY-X) GTEQ END-EXEC. EXEC CICS READNEXT FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS READPREV DATASET('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS ENDBR FILE('ACCTDAT') END-EXEC. EXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) END-EXEC. EXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) END-EXEC. EXEC CICS READ FILE('ACCTDAT') UPDATE TOKEN(TOKEN-X) INTO(RECORD-X) RIDFLD(KEY-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS REWRITE DATASET('ACCTDAT') FROM(RECORD-X) END-EXEC. EXEC CICS UNLOCK FILE('ACCTDAT') TOKEN(TOKEN-X) END-EXEC. EXEC CICS ENQ RESOURCE(LOCK-X) LENGTH(4) UOW NOSUSPEND END-EXEC. EXEC CICS DEQ RESOURCE(LOCK-X) LENGTH(4) UOW END-EXEC. EXEC CICS ADDRESS SET(PTR-X) USING(ADDRESS OF RECORD-X) END-EXEC. EXEC CICS CHANGE TASK PRIORITY(PRIORITY-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS HANDLE AID ANYKEY(AID-HANDLER) ENTER END-EXEC. EXEC CICS HANDLE ABEND PROGRAM('ABEXIT') END-EXEC. EXEC CICS HANDLE CONDITION ERROR(ERROR-HANDLER) LENGERR END-EXEC. EXEC CICS IGNORE CONDITION PGMIDERR END-EXEC. EXEC CICS PUSH HANDLE END-EXEC. EXEC CICS POP HANDLE RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS SET ASSOCIATION USERCORRDATA(CORR-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS SUSPEND END-EXEC. EXEC CICS SYNCPOINT ROLLBACK NOHANDLE END-EXEC. EXEC CICS ABEND ABCODE(AB-CODE) NODUMP END-EXEC. EXEC CICS RETURN TRANSID('NEXT') COMMAREA(RECORD-X) END-EXEC.";
        let source = format!(
            "{source} EXEC CICS ADDRESS COMMAREA(PTR-X) END-EXEC. EXEC CICS WAIT EVENT ECADDR(PTR-X) NAME('WAITONE') END-EXEC. EXEC CICS WAIT EXTERNAL ECBLIST(PTR-X) NUMEVENTS(CODE-A) PURGEABLE END-EXEC. EXEC CICS WRITEQ TD QUEUE('OUTQ') FROM(RECORD-X) LENGTH(4) END-EXEC. EXEC CICS READQ TD QUEUE('OUTQ') INTO(RECORD-X) LENGTH(PRIORITY-X) END-EXEC. EXEC CICS DELETEQ TD QUEUE('OUTQ') END-EXEC. EXEC CICS DELETEQ TS QUEUE('TEMPQ') END-EXEC. EXEC CICS READQ TS QUEUE('TEMPQ') INTO(RECORD-X) LENGTH(PRIORITY-X) END-EXEC. EXEC CICS WRITEQ TS QUEUE('TEMPQ') FROM(RECORD-X) LENGTH(4) END-EXEC. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(4) END-EXEC. EXEC CICS FREEMAIN DATAPOINTER(PTR-X) END-EXEC. EXEC CICS SEND MAP('MENU') MAPSET('MAIN') FROM(RECORD-X) END-EXEC. EXEC CICS RECEIVE MAP('MENU') MAPSET('MAIN') END-EXEC. EXEC CICS SEND TEXT FROM(RECORD-X) END-EXEC. EXEC CICS ASSIGN ABCODE(AB-CODE) END-EXEC. EXEC CICS PURGE MESSAGE END-EXEC. EXEC CICS START TRANSID('NEXT') REQID('REQ0001') FROM(RECORD-X) LENGTH(4) INTERVAL(0) END-EXEC. EXEC CICS RETRIEVE INTO(RECORD-X) LENGTH(PRIORITY-X) END-EXEC. EXEC CICS CANCEL REQID('REQ0001') TRANSID('NEXT') END-EXEC. EXEC CICS DELAY INTERVAL(0) END-EXEC. EXEC CICS TRANSFORM DATATOJSON CHANNEL('WORK') INCONTAINER('SOURCE') TRANSFORMER('CUSTOMER') END-EXEC. EXEC CICS TRANSFORM DATATOXML CHANNEL('WORK') DATCONTAINER('SOURCE') XMLCONTAINER('XML') XMLTRANSFORM('CUSTOMERXML') END-EXEC. EXEC CICS TRANSFORM JSONTODATA CHANNEL('WORK') INCONTAINER('JSON') TRANSFORMER('CUSTOMER') END-EXEC. EXEC CICS TRANSFORM XMLTODATA CHANNEL('WORK') XMLCONTAINER('XML') END-EXEC. EXEC CICS DEFINE INPUT EVENT('GO') RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS DEFINE COMPOSITE EVENT('GROUP') OR SUBEVENT1('GO') END-EXEC. EXEC CICS ADD SUBEVENT('GO') EVENT('GROUP') END-EXEC. EXEC CICS REMOVE SUBEVENT('GO') EVENT('GROUP') END-EXEC. EXEC CICS DELETE EVENT('GO') END-EXEC. EXEC CICS DEFINE TIMER('CLOCK') EVENT('BELL') AFTER SECONDS(5) END-EXEC. EXEC CICS CHECK TIMER('CLOCK') STATUS(CODE-A) END-EXEC. EXEC CICS FORCE TIMER('CLOCK') END-EXEC. EXEC CICS DELETE TIMER('CLOCK') END-EXEC. EXEC CICS RETRIEVE REATTACH EVENT(EVENT-NAME) EVENTTYPE(CODE-A) END-EXEC. EXEC CICS RETRIEVE SUBEVENT(SUBEVENT-NAME) EVENT('GROUP') EVENTTYPE(CODE-A) END-EXEC. EXEC CICS TEST EVENT('GO') FIRESTATUS(CODE-A) END-EXEC. EXEC CICS SIGNAL EVENT('ORDER:GO') FROM(RECORD-X) FROMLENGTH(4) END-EXEC."
        );
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(&source));
        let hir = analysis
            .hir
            .as_ref()
            .unwrap_or_else(|| panic!("typed CICS HIR: {:?}", analysis.diagnostics));
        let encoded_hir = encode_binary(&hir.module, CodecLimits::default()).unwrap();
        let hir_module = decode_binary(&encoded_hir, CodecLimits::default()).unwrap();
        let hir_operations = hir_module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .collect::<Vec<_>>();
        assert_eq!(
            hir_operations
                .iter()
                .map(|operation| format!(
                    "{}@{}",
                    operation.identity.namespace(),
                    operation.identity.major()
                ))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["cobol.hir@1".into(), "cobol.hir@2".into()])
        );
        let typed_hir = hir_operations
            .iter()
            .copied()
            .filter(|operation| {
                operation.identity.namespace() == "cobol.hir"
                    && operation.identity.name() == "exec_cics"
                    && operation.identity.major() == 2
            })
            .collect::<Vec<_>>();
        assert_eq!(typed_hir.len(), 67);
        let mut hir_plans = Vec::new();
        for operation in typed_hir {
            assert!(!operation.attributes.contains_key("arguments"));
            assert!(!operation.attributes.contains_key("control_text"));
            assert!(operation.location.is_some());
            assert_eq!(
                operation
                    .storage
                    .iter()
                    .map(|reference| reference.storage)
                    .collect::<BTreeSet<_>>()
                    .len(),
                operation.storage.len()
            );
            assert!(
                operation
                    .storage
                    .iter()
                    .all(|reference| reference.offset == 0 && reference.length > 0)
            );
            let Attribute::Bytes(bytes) = &operation.attributes["cics_plan"] else {
                panic!("typed HIR CICS plan bytes")
            };
            let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
            assert_eq!(
                operation
                    .storage
                    .iter()
                    .map(|reference| reference.storage)
                    .collect::<BTreeSet<_>>(),
                cics_plan_storage(&plan)
            );
            for reference in &operation.storage {
                assert_eq!(
                    hir_module
                        .storage()
                        .iter()
                        .find(|region| region.id == reference.storage)
                        .unwrap()
                        .size,
                    reference.length
                );
            }
            let source_operation = match plan.operation {
                CicsPlanOperation::BtsEndBrowseContainer => {
                    crate::HirCicsOperation::BtsEndBrowseContainer
                }
                CicsPlanOperation::BtsGetNextContainer => {
                    crate::HirCicsOperation::BtsGetNextContainer
                }
                CicsPlanOperation::BtsInquireContainer => {
                    crate::HirCicsOperation::BtsInquireContainer
                }
                CicsPlanOperation::BtsStartBrowseContainer => {
                    crate::HirCicsOperation::BtsStartBrowseContainer
                }
                CicsPlanOperation::BtsEndBrowseEvent => crate::HirCicsOperation::BtsEndBrowseEvent,
                CicsPlanOperation::BtsGetNextEvent => crate::HirCicsOperation::BtsGetNextEvent,
                CicsPlanOperation::BtsInquireEvent => crate::HirCicsOperation::BtsInquireEvent,
                CicsPlanOperation::BtsStartBrowseEvent => {
                    crate::HirCicsOperation::BtsStartBrowseEvent
                }
                CicsPlanOperation::BtsEndBrowseTimer => crate::HirCicsOperation::BtsEndBrowseTimer,
                CicsPlanOperation::BtsInquireTimer => crate::HirCicsOperation::BtsInquireTimer,
                CicsPlanOperation::BtsStartBrowseTimer => {
                    crate::HirCicsOperation::BtsStartBrowseTimer
                }
                CicsPlanOperation::BtsStartBrowseActivity => {
                    crate::HirCicsOperation::BtsStartBrowseActivity
                }
                CicsPlanOperation::BtsGetNextActivity => {
                    crate::HirCicsOperation::BtsGetNextActivity
                }
                CicsPlanOperation::BtsEndBrowseActivity => {
                    crate::HirCicsOperation::BtsEndBrowseActivity
                }
                CicsPlanOperation::BtsInquireActivity => {
                    crate::HirCicsOperation::BtsInquireActivity
                }
                CicsPlanOperation::BtsStartBrowseProcess => {
                    crate::HirCicsOperation::BtsStartBrowseProcess
                }
                CicsPlanOperation::BtsGetNextProcess => crate::HirCicsOperation::BtsGetNextProcess,
                CicsPlanOperation::BtsEndBrowseProcess => {
                    crate::HirCicsOperation::BtsEndBrowseProcess
                }
                CicsPlanOperation::BtsInquireProcess => crate::HirCicsOperation::BtsInquireProcess,
                CicsPlanOperation::AcquireActivityId => crate::HirCicsOperation::AcquireActivityId,
                CicsPlanOperation::AcquireProcess => crate::HirCicsOperation::AcquireProcess,
                CicsPlanOperation::CancelAcqActivity => crate::HirCicsOperation::CancelAcqActivity,
                CicsPlanOperation::CancelAcqProcess => crate::HirCicsOperation::CancelAcqProcess,
                CicsPlanOperation::CancelActivity => crate::HirCicsOperation::CancelActivity,
                CicsPlanOperation::CheckAcqActivity => crate::HirCicsOperation::CheckAcqActivity,
                CicsPlanOperation::CheckAcqProcess => crate::HirCicsOperation::CheckAcqProcess,
                CicsPlanOperation::CheckActivity => crate::HirCicsOperation::CheckActivity,
                CicsPlanOperation::DefineActivity => crate::HirCicsOperation::DefineActivity,
                CicsPlanOperation::DefineProcess => crate::HirCicsOperation::DefineProcess,
                CicsPlanOperation::DeleteActivity => crate::HirCicsOperation::DeleteActivity,
                CicsPlanOperation::ResetAcqProcess => crate::HirCicsOperation::ResetAcqProcess,
                CicsPlanOperation::ResetActivity => crate::HirCicsOperation::ResetActivity,
                CicsPlanOperation::ResumeAcqActivity => crate::HirCicsOperation::ResumeAcqActivity,
                CicsPlanOperation::ResumeAcqProcess => crate::HirCicsOperation::ResumeAcqProcess,
                CicsPlanOperation::ResumeActivity => crate::HirCicsOperation::ResumeActivity,
                CicsPlanOperation::RunAcqActivity => crate::HirCicsOperation::RunAcqActivity,
                CicsPlanOperation::RunAcqProcess => crate::HirCicsOperation::RunAcqProcess,
                CicsPlanOperation::RunActivity => crate::HirCicsOperation::RunActivity,
                CicsPlanOperation::RunTransId => crate::HirCicsOperation::RunTransId,
                CicsPlanOperation::DeleteChannel => crate::HirCicsOperation::DeleteChannel,
                CicsPlanOperation::DeleteContainer => crate::HirCicsOperation::DeleteContainer,
                CicsPlanOperation::GetContainer => crate::HirCicsOperation::GetContainer,
                CicsPlanOperation::MoveContainer => crate::HirCicsOperation::MoveContainer,
                CicsPlanOperation::PutContainer => crate::HirCicsOperation::PutContainer,
                CicsPlanOperation::QueryChannel => crate::HirCicsOperation::QueryChannel,
                CicsPlanOperation::SuspendAcqActivity => {
                    crate::HirCicsOperation::SuspendAcqActivity
                }
                CicsPlanOperation::SuspendAcqProcess => crate::HirCicsOperation::SuspendAcqProcess,
                CicsPlanOperation::SuspendActivity => crate::HirCicsOperation::SuspendActivity,
                CicsPlanOperation::FetchAny => crate::HirCicsOperation::FetchAny,
                CicsPlanOperation::FetchChild => crate::HirCicsOperation::FetchChild,
                CicsPlanOperation::FreeChild => crate::HirCicsOperation::FreeChild,
                CicsPlanOperation::LinkAcqActivity => crate::HirCicsOperation::LinkAcqActivity,
                CicsPlanOperation::LinkAcqProcess => crate::HirCicsOperation::LinkAcqProcess,
                CicsPlanOperation::LinkActivity => crate::HirCicsOperation::LinkActivity,
                CicsPlanOperation::AllocateConversation => {
                    crate::HirCicsOperation::AllocateConversation
                }
                CicsPlanOperation::GdsAllocateConversation => {
                    crate::HirCicsOperation::GdsAllocateConversation
                }
                CicsPlanOperation::GdsAssignConversation => {
                    crate::HirCicsOperation::GdsAssignConversation
                }
                CicsPlanOperation::BuildAttach => crate::HirCicsOperation::BuildAttach,
                CicsPlanOperation::ConnectProcess => crate::HirCicsOperation::ConnectProcess,
                CicsPlanOperation::GdsConnectProcess => crate::HirCicsOperation::GdsConnectProcess,
                CicsPlanOperation::Converse => crate::HirCicsOperation::Converse,
                CicsPlanOperation::FreeConversation => crate::HirCicsOperation::FreeConversation,
                CicsPlanOperation::GdsFreeConversation => {
                    crate::HirCicsOperation::GdsFreeConversation
                }
                CicsPlanOperation::ReceiveConversation => {
                    crate::HirCicsOperation::ReceiveConversation
                }
                CicsPlanOperation::GdsReceiveConversation => {
                    crate::HirCicsOperation::GdsReceiveConversation
                }
                CicsPlanOperation::SendConversation => crate::HirCicsOperation::SendConversation,
                CicsPlanOperation::GdsWaitConversation => {
                    crate::HirCicsOperation::GdsWaitConversation
                }
                CicsPlanOperation::WaitConvid => crate::HirCicsOperation::WaitConvid,
                CicsPlanOperation::WaitSignal => crate::HirCicsOperation::WaitSignal,
                CicsPlanOperation::WaitTerminal => crate::HirCicsOperation::WaitTerminal,
                CicsPlanOperation::Abend => crate::HirCicsOperation::Abend,
                CicsPlanOperation::QuerySecurity => crate::HirCicsOperation::QuerySecurity,
                CicsPlanOperation::VerifyPassword => crate::HirCicsOperation::VerifyPassword,
                CicsPlanOperation::ChangePassword => crate::HirCicsOperation::ChangePassword,
                CicsPlanOperation::ChangePhrase => crate::HirCicsOperation::ChangePhrase,
                CicsPlanOperation::RequestPassTicket => crate::HirCicsOperation::RequestPassTicket,
                CicsPlanOperation::RequestEncryptPassTicket => {
                    crate::HirCicsOperation::RequestEncryptPassTicket
                }
                CicsPlanOperation::Signon => crate::HirCicsOperation::Signon,
                CicsPlanOperation::Signoff => crate::HirCicsOperation::Signoff,
                CicsPlanOperation::VerifyPhrase => crate::HirCicsOperation::VerifyPhrase,
                CicsPlanOperation::VerifyToken => crate::HirCicsOperation::VerifyToken,
                CicsPlanOperation::Address => crate::HirCicsOperation::Address,
                CicsPlanOperation::AddressSet => crate::HirCicsOperation::AddressSet,
                CicsPlanOperation::Asktime => crate::HirCicsOperation::Asktime,
                CicsPlanOperation::AsktimeEib => crate::HirCicsOperation::AsktimeEib,
                CicsPlanOperation::FormatTime => crate::HirCicsOperation::FormatTime,
                CicsPlanOperation::ConvertTime => crate::HirCicsOperation::ConvertTime,
                CicsPlanOperation::BifDeedit => crate::HirCicsOperation::BifDeedit,
                CicsPlanOperation::BifDigest => crate::HirCicsOperation::BifDigest,
                CicsPlanOperation::Cancel => crate::HirCicsOperation::Cancel,
                CicsPlanOperation::Delay => crate::HirCicsOperation::Delay,
                CicsPlanOperation::DefineCounter => crate::HirCicsOperation::DefineCounter,
                CicsPlanOperation::DefineDCounter => crate::HirCicsOperation::DefineDCounter,
                CicsPlanOperation::DeleteCounter => crate::HirCicsOperation::DeleteCounter,
                CicsPlanOperation::DeleteDCounter => crate::HirCicsOperation::DeleteDCounter,
                CicsPlanOperation::GetCounter => crate::HirCicsOperation::GetCounter,
                CicsPlanOperation::GetDCounter => crate::HirCicsOperation::GetDCounter,
                CicsPlanOperation::QueryCounter => crate::HirCicsOperation::QueryCounter,
                CicsPlanOperation::QueryDCounter => crate::HirCicsOperation::QueryDCounter,
                CicsPlanOperation::RewindCounter => crate::HirCicsOperation::RewindCounter,
                CicsPlanOperation::RewindDCounter => crate::HirCicsOperation::RewindDCounter,
                CicsPlanOperation::UpdateCounter => crate::HirCicsOperation::UpdateCounter,
                CicsPlanOperation::UpdateDCounter => crate::HirCicsOperation::UpdateDCounter,
                CicsPlanOperation::Post => crate::HirCicsOperation::Post,
                CicsPlanOperation::WriteOperator => crate::HirCicsOperation::WriteOperator,
                CicsPlanOperation::ExtractCertificate => {
                    crate::HirCicsOperation::ExtractCertificate
                }
                CicsPlanOperation::ExtractTcpip => crate::HirCicsOperation::ExtractTcpip,
                CicsPlanOperation::ChangeTask => crate::HirCicsOperation::ChangeTask,
                CicsPlanOperation::Deq => crate::HirCicsOperation::Deq,
                CicsPlanOperation::Enq => crate::HirCicsOperation::Enq,
                CicsPlanOperation::HandleAid => crate::HirCicsOperation::HandleAid,
                CicsPlanOperation::HandleAbend => crate::HirCicsOperation::HandleAbend,
                CicsPlanOperation::HandleCondition => crate::HirCicsOperation::HandleCondition,
                CicsPlanOperation::IgnoreCondition => crate::HirCicsOperation::IgnoreCondition,
                CicsPlanOperation::InvokeApplication => crate::HirCicsOperation::InvokeApplication,
                CicsPlanOperation::IssueAbort => crate::HirCicsOperation::IssueAbort,
                CicsPlanOperation::IssueAdd => crate::HirCicsOperation::IssueAdd,
                CicsPlanOperation::IssueEnd => crate::HirCicsOperation::IssueEnd,
                CicsPlanOperation::IssueErase => crate::HirCicsOperation::IssueErase,
                CicsPlanOperation::IssueNote => crate::HirCicsOperation::IssueNote,
                CicsPlanOperation::IssueQuery => crate::HirCicsOperation::IssueQuery,
                CicsPlanOperation::IssueReceive => crate::HirCicsOperation::IssueReceive,
                CicsPlanOperation::IssueReplace => crate::HirCicsOperation::IssueReplace,
                CicsPlanOperation::IssueSend => crate::HirCicsOperation::IssueSend,
                CicsPlanOperation::Route => crate::HirCicsOperation::Route,
                CicsPlanOperation::IssueWait => crate::HirCicsOperation::IssueWait,
                CicsPlanOperation::IssueAbend => crate::HirCicsOperation::IssueAbend,
                CicsPlanOperation::IssueConfirmation => crate::HirCicsOperation::IssueConfirmation,
                CicsPlanOperation::IssueCopy => crate::HirCicsOperation::IssueCopy,
                CicsPlanOperation::IssueDisconnect => crate::HirCicsOperation::IssueDisconnect,
                CicsPlanOperation::IssueEndfile => crate::HirCicsOperation::IssueEndfile,
                CicsPlanOperation::IssueEndoutput => crate::HirCicsOperation::IssueEndoutput,
                CicsPlanOperation::IssueEods => crate::HirCicsOperation::IssueEods,
                CicsPlanOperation::IssueEraseAup => crate::HirCicsOperation::IssueEraseAup,
                CicsPlanOperation::IssueError => crate::HirCicsOperation::IssueError,
                CicsPlanOperation::IssueLoad => crate::HirCicsOperation::IssueLoad,
                CicsPlanOperation::IssuePass => crate::HirCicsOperation::IssuePass,
                CicsPlanOperation::IssuePrepare => crate::HirCicsOperation::IssuePrepare,
                CicsPlanOperation::IssuePrint => crate::HirCicsOperation::IssuePrint,
                CicsPlanOperation::IssueReset => crate::HirCicsOperation::IssueReset,
                CicsPlanOperation::IssueSignal => crate::HirCicsOperation::IssueSignal,
                CicsPlanOperation::GdsIssueAbend
                | CicsPlanOperation::GdsIssueConfirmation
                | CicsPlanOperation::GdsIssueError
                | CicsPlanOperation::GdsIssuePrepare
                | CicsPlanOperation::GdsIssueSignal => {
                    panic!("GDS ISSUE is not applicable to COBOL HIR")
                }
                CicsPlanOperation::Load => crate::HirCicsOperation::Load,
                CicsPlanOperation::Release => crate::HirCicsOperation::Release,
                CicsPlanOperation::Link => crate::HirCicsOperation::Link,
                CicsPlanOperation::Xctl => crate::HirCicsOperation::Xctl,
                CicsPlanOperation::Return => crate::HirCicsOperation::Return,
                CicsPlanOperation::StartBrowse => crate::HirCicsOperation::StartBrowse,
                CicsPlanOperation::ResetBrowse => crate::HirCicsOperation::ResetBrowse,
                CicsPlanOperation::Unlock => crate::HirCicsOperation::Unlock,
                CicsPlanOperation::ReadNext => crate::HirCicsOperation::ReadNext,
                CicsPlanOperation::ReadPrev => crate::HirCicsOperation::ReadPrev,
                CicsPlanOperation::ReadTransientData => crate::HirCicsOperation::ReadTransientData,
                CicsPlanOperation::EndBrowse => crate::HirCicsOperation::EndBrowse,
                CicsPlanOperation::Delete => crate::HirCicsOperation::Delete,
                CicsPlanOperation::Write => crate::HirCicsOperation::Write,
                CicsPlanOperation::WriteTransientData => {
                    crate::HirCicsOperation::WriteTransientData
                }
                CicsPlanOperation::DeleteTransientData => {
                    crate::HirCicsOperation::DeleteTransientData
                }
                CicsPlanOperation::DeleteTemporaryStorage => {
                    crate::HirCicsOperation::DeleteTemporaryStorage
                }
                CicsPlanOperation::ReadTemporaryStorage => {
                    crate::HirCicsOperation::ReadTemporaryStorage
                }
                CicsPlanOperation::WriteTemporaryStorage => {
                    crate::HirCicsOperation::WriteTemporaryStorage
                }
                CicsPlanOperation::Getmain => crate::HirCicsOperation::Getmain,
                CicsPlanOperation::Getmain64 => {
                    panic!("GETMAIN64 must not originate from COBOL source")
                }
                CicsPlanOperation::Freemain => crate::HirCicsOperation::Freemain,
                CicsPlanOperation::Freemain64 => {
                    panic!("FREEMAIN64 must not originate from COBOL source")
                }
                CicsPlanOperation::ReceiveMap => crate::HirCicsOperation::ReceiveMap,
                CicsPlanOperation::SendMap => crate::HirCicsOperation::SendMap,
                CicsPlanOperation::SendText => crate::HirCicsOperation::SendText,
                CicsPlanOperation::SendPartnset => crate::HirCicsOperation::SendPartnset,
                CicsPlanOperation::ReceivePartn => crate::HirCicsOperation::ReceivePartn,
                CicsPlanOperation::SendControl => crate::HirCicsOperation::SendControl,
                CicsPlanOperation::SendPage => crate::HirCicsOperation::SendPage,
                CicsPlanOperation::Assign => crate::HirCicsOperation::Assign,
                CicsPlanOperation::PurgeMessage => crate::HirCicsOperation::PurgeMessage,
                CicsPlanOperation::PopHandle => crate::HirCicsOperation::PopHandle,
                CicsPlanOperation::PushHandle => crate::HirCicsOperation::PushHandle,
                CicsPlanOperation::Read => crate::HirCicsOperation::Read,
                CicsPlanOperation::Rewrite => crate::HirCicsOperation::Rewrite,
                CicsPlanOperation::SetAssociationUserCorrData => {
                    crate::HirCicsOperation::SetAssociationUserCorrData
                }
                CicsPlanOperation::SpoolClose => crate::HirCicsOperation::SpoolClose,
                CicsPlanOperation::SpoolOpenInput => crate::HirCicsOperation::SpoolOpenInput,
                CicsPlanOperation::SpoolOpenOutput => crate::HirCicsOperation::SpoolOpenOutput,
                CicsPlanOperation::SpoolRead => crate::HirCicsOperation::SpoolRead,
                CicsPlanOperation::SpoolWrite => crate::HirCicsOperation::SpoolWrite,
                CicsPlanOperation::EnterTraceNum => crate::HirCicsOperation::EnterTraceNum,
                CicsPlanOperation::Monitor => crate::HirCicsOperation::Monitor,
                CicsPlanOperation::DumpTransaction => crate::HirCicsOperation::DumpTransaction,
                CicsPlanOperation::Dump => crate::HirCicsOperation::Dump,
                CicsPlanOperation::Trace => crate::HirCicsOperation::Trace,
                CicsPlanOperation::EnterTraceId => crate::HirCicsOperation::EnterTraceId,
                CicsPlanOperation::Syncpoint => crate::HirCicsOperation::Syncpoint,
                CicsPlanOperation::Suspend => crate::HirCicsOperation::Suspend,
                CicsPlanOperation::WaitEvent => crate::HirCicsOperation::WaitEvent,
                CicsPlanOperation::WaitExternal => crate::HirCicsOperation::WaitExternal,
                CicsPlanOperation::WaitCics => crate::HirCicsOperation::WaitCics,
                CicsPlanOperation::Start => crate::HirCicsOperation::Start,
                CicsPlanOperation::StartAttach => crate::HirCicsOperation::StartAttach,
                CicsPlanOperation::StartBrexit => crate::HirCicsOperation::StartBrexit,
                CicsPlanOperation::Retrieve => crate::HirCicsOperation::Retrieve,
                CicsPlanOperation::DocumentCreate => crate::HirCicsOperation::DocumentCreate,
                CicsPlanOperation::DefineInputEvent => crate::HirCicsOperation::DefineInputEvent,
                CicsPlanOperation::DefineCompositeEvent => {
                    crate::HirCicsOperation::DefineCompositeEvent
                }
                CicsPlanOperation::AddSubevent => crate::HirCicsOperation::AddSubevent,
                CicsPlanOperation::RemoveSubevent => crate::HirCicsOperation::RemoveSubevent,
                CicsPlanOperation::DeleteEvent => crate::HirCicsOperation::DeleteEvent,
                CicsPlanOperation::CheckTimer => crate::HirCicsOperation::CheckTimer,
                CicsPlanOperation::DefineTimer => crate::HirCicsOperation::DefineTimer,
                CicsPlanOperation::DeleteTimer => crate::HirCicsOperation::DeleteTimer,
                CicsPlanOperation::ForceTimer => crate::HirCicsOperation::ForceTimer,
                CicsPlanOperation::RetrieveReattachEvent => {
                    crate::HirCicsOperation::RetrieveReattachEvent
                }
                CicsPlanOperation::RetrieveSubevent => crate::HirCicsOperation::RetrieveSubevent,
                CicsPlanOperation::TestEvent => crate::HirCicsOperation::TestEvent,
                CicsPlanOperation::SignalEvent => crate::HirCicsOperation::SignalEvent,
                CicsPlanOperation::DocumentDelete => crate::HirCicsOperation::DocumentDelete,
                CicsPlanOperation::DocumentInsert => crate::HirCicsOperation::DocumentInsert,
                CicsPlanOperation::DocumentRetrieve => crate::HirCicsOperation::DocumentRetrieve,
                CicsPlanOperation::DocumentSet => crate::HirCicsOperation::DocumentSet,
                CicsPlanOperation::InvokeService => crate::HirCicsOperation::InvokeService,
                CicsPlanOperation::SoapFaultAdd => crate::HirCicsOperation::SoapFaultAdd,
                CicsPlanOperation::SoapFaultCreate => crate::HirCicsOperation::SoapFaultCreate,
                CicsPlanOperation::SoapFaultDelete => crate::HirCicsOperation::SoapFaultDelete,
                CicsPlanOperation::WsaContextBuild => crate::HirCicsOperation::WsaContextBuild,
                CicsPlanOperation::WsaContextDelete => crate::HirCicsOperation::WsaContextDelete,
                CicsPlanOperation::WsaContextGet => crate::HirCicsOperation::WsaContextGet,
                CicsPlanOperation::WsaEprCreate => crate::HirCicsOperation::WsaEprCreate,
                CicsPlanOperation::TransformDataToJson => {
                    crate::HirCicsOperation::TransformDataToJson
                }
                CicsPlanOperation::TransformDataToXml => {
                    crate::HirCicsOperation::TransformDataToXml
                }
                CicsPlanOperation::TransformJsonToData => {
                    crate::HirCicsOperation::TransformJsonToData
                }
                CicsPlanOperation::TransformXmlToData => {
                    crate::HirCicsOperation::TransformXmlToData
                }
                CicsPlanOperation::WebParseUrl => crate::HirCicsOperation::WebParseUrl,
                CicsPlanOperation::WebOpen => crate::HirCicsOperation::WebOpen,
                CicsPlanOperation::WebClose => crate::HirCicsOperation::WebClose,
                CicsPlanOperation::WebExtract => crate::HirCicsOperation::WebExtract,
                CicsPlanOperation::ExtractAttach => crate::HirCicsOperation::ExtractAttach,
                CicsPlanOperation::ExtractAttributes => crate::HirCicsOperation::ExtractAttributes,
                CicsPlanOperation::GdsExtractAttributes => {
                    crate::HirCicsOperation::GdsExtractAttributes
                }
                CicsPlanOperation::ExtractLogonMsg => crate::HirCicsOperation::ExtractLogonMsg,
                CicsPlanOperation::ExtractProcess => crate::HirCicsOperation::ExtractProcess,
                CicsPlanOperation::GdsExtractProcess => crate::HirCicsOperation::GdsExtractProcess,
                CicsPlanOperation::ExtractTct => crate::HirCicsOperation::ExtractTct,
                CicsPlanOperation::Point => crate::HirCicsOperation::Point,
                CicsPlanOperation::ExtractWeb => crate::HirCicsOperation::ExtractWeb,
                CicsPlanOperation::WebRead => crate::HirCicsOperation::WebRead,
                CicsPlanOperation::WebStartBrowse => crate::HirCicsOperation::WebStartBrowse,
                CicsPlanOperation::WebReadNext => crate::HirCicsOperation::WebReadNext,
                CicsPlanOperation::WebEndBrowse => crate::HirCicsOperation::WebEndBrowse,
                CicsPlanOperation::WebWrite => crate::HirCicsOperation::WebWrite,
                CicsPlanOperation::WebSend => crate::HirCicsOperation::WebSend,
                CicsPlanOperation::WebRetrieve => crate::HirCicsOperation::WebRetrieve,
                CicsPlanOperation::WebReceive => crate::HirCicsOperation::WebReceive,
                CicsPlanOperation::WebConverse => crate::HirCicsOperation::WebConverse,
                CicsPlanOperation::WaitJournalName => crate::HirCicsOperation::WaitJournalName,
                CicsPlanOperation::WaitJournalNum => crate::HirCicsOperation::WaitJournalNum,
                CicsPlanOperation::WriteJournalName => crate::HirCicsOperation::WriteJournalName,
                CicsPlanOperation::WriteJournalNum => crate::HirCicsOperation::WriteJournalNum,
            };
            assert_eq!(
                operation.effects,
                crate::hir::cics::operation_effects(source_operation)
            );
            for forbidden in cics_grammar_tokens() {
                assert!(
                    !bytes
                        .windows(forbidden.len())
                        .any(|window| window == forbidden)
                );
            }
            hir_plans.push(plan);
        }
        assert_eq!(
            hir_plans
                .iter()
                .map(|plan| plan.operation)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CicsPlanOperation::Abend,
                CicsPlanOperation::Address,
                CicsPlanOperation::AddressSet,
                CicsPlanOperation::Asktime,
                CicsPlanOperation::AsktimeEib,
                CicsPlanOperation::FormatTime,
                CicsPlanOperation::Cancel,
                CicsPlanOperation::Delay,
                CicsPlanOperation::ChangeTask,
                CicsPlanOperation::Deq,
                CicsPlanOperation::Enq,
                CicsPlanOperation::HandleAid,
                CicsPlanOperation::HandleAbend,
                CicsPlanOperation::HandleCondition,
                CicsPlanOperation::IgnoreCondition,
                CicsPlanOperation::Link,
                CicsPlanOperation::Xctl,
                CicsPlanOperation::Return,
                CicsPlanOperation::StartBrowse,
                CicsPlanOperation::ResetBrowse,
                CicsPlanOperation::Unlock,
                CicsPlanOperation::ReadNext,
                CicsPlanOperation::ReadPrev,
                CicsPlanOperation::ReadTransientData,
                CicsPlanOperation::EndBrowse,
                CicsPlanOperation::Delete,
                CicsPlanOperation::Write,
                CicsPlanOperation::WriteTransientData,
                CicsPlanOperation::DeleteTransientData,
                CicsPlanOperation::DeleteTemporaryStorage,
                CicsPlanOperation::ReadTemporaryStorage,
                CicsPlanOperation::WriteTemporaryStorage,
                CicsPlanOperation::Getmain,
                CicsPlanOperation::Freemain,
                CicsPlanOperation::ReceiveMap,
                CicsPlanOperation::SendMap,
                CicsPlanOperation::SendText,
                CicsPlanOperation::Assign,
                CicsPlanOperation::PurgeMessage,
                CicsPlanOperation::PopHandle,
                CicsPlanOperation::PushHandle,
                CicsPlanOperation::Read,
                CicsPlanOperation::Rewrite,
                CicsPlanOperation::SetAssociationUserCorrData,
                CicsPlanOperation::Syncpoint,
                CicsPlanOperation::Suspend,
                CicsPlanOperation::WaitEvent,
                CicsPlanOperation::WaitExternal,
                CicsPlanOperation::Start,
                CicsPlanOperation::Retrieve,
                CicsPlanOperation::TransformDataToJson,
                CicsPlanOperation::TransformDataToXml,
                CicsPlanOperation::TransformJsonToData,
                CicsPlanOperation::TransformXmlToData,
                CicsPlanOperation::DefineInputEvent,
                CicsPlanOperation::DefineCompositeEvent,
                CicsPlanOperation::AddSubevent,
                CicsPlanOperation::RemoveSubevent,
                CicsPlanOperation::DeleteEvent,
                CicsPlanOperation::DefineTimer,
                CicsPlanOperation::CheckTimer,
                CicsPlanOperation::ForceTimer,
                CicsPlanOperation::DeleteTimer,
                CicsPlanOperation::RetrieveReattachEvent,
                CicsPlanOperation::RetrieveSubevent,
                CicsPlanOperation::TestEvent,
                CicsPlanOperation::SignalEvent,
            ])
        );
        let read = hir_plans
            .iter()
            .find(|plan| plan.operation == CicsPlanOperation::Read)
            .unwrap();
        assert!(
            read.options
                .contains(&mainframe_env_ir::CicsPlanOption::Update)
        );
        assert!(matches!(
            read.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::File)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Literal(bytes)) if bytes == b"ACCTDAT"
        ));
        assert!(matches!(
            read.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Ridfld)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Storage(slot)) if slot.qualified_layout_name == "KEY-X"
        ));
        assert!(read.outputs.iter().any(|output| {
            output.name == CicsOutputName::Into && output.target.qualified_layout_name == "RECORD-X"
        }));
        assert!(matches!(
            &read.condition,
            CicsCondition::Respond {
                response2: Some(_),
                ..
            }
        ));
        let event = hir_plans
            .iter()
            .find(|plan| plan.operation == CicsPlanOperation::DefineInputEvent)
            .expect("selected DEFINE INPUT EVENT plan");
        assert!(matches!(
            event
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Event)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Literal(bytes)) if bytes == b"GO"
        ));
        assert!(matches!(
            event.condition,
            CicsCondition::Respond {
                response2: Some(_),
                ..
            }
        ));
        let composite = hir_plans
            .iter()
            .find(|plan| plan.operation == CicsPlanOperation::DefineCompositeEvent)
            .expect("selected DEFINE COMPOSITE EVENT plan");
        assert!(
            composite
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::EventOr)
        );
        assert!(matches!(
            composite
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::SubEvent1)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Literal(bytes)) if bytes == b"GO"
        ));
        let rewrite = hir_plans
            .iter()
            .find(|plan| plan.operation == CicsPlanOperation::Rewrite)
            .unwrap();
        assert!(matches!(
            rewrite
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Dataset)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Literal(bytes)) if bytes == b"ACCTDAT"
        ));
        assert!(matches!(
            rewrite
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::From)
                .map(|operand| &operand.value),
            Some(CicsOperandValue::Storage(slot)) if slot.qualified_layout_name == "RECORD-X"
        ));
        assert!(rewrite.outputs.is_empty());
        assert_eq!(rewrite.condition, CicsCondition::Default);
        let syncpoint = hir_plans
            .iter()
            .find(|plan| plan.operation == CicsPlanOperation::Syncpoint)
            .unwrap();
        assert!(
            syncpoint
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::Rollback)
        );
        assert!(
            syncpoint
                .options
                .contains(&mainframe_env_ir::CicsPlanOption::NoHandle)
        );
        assert_eq!(syncpoint.condition, CicsCondition::NoHandle);

        let CompilerResult::Published { artifact, .. } = compiler
            .compile(request(&source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published typed CICS")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from([
                "cics.file@1".into(),
                "cics.interval@1".into(),
                "cics.program@1".into(),
                "cics.queue@1".into(),
                "cics.recovery@1".into(),
                "cics.storage@1".into(),
                "cics.task@1".into(),
                "cics.terminal@1".into(),
                "cics.time@1".into(),
                "cics.transform@1".into(),
                "cics.event@1".into(),
                "mainframe.core.cobol@1".into(),
            ])
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let operations = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| {
                matches!(
                    operation.identity.namespace(),
                    "cics.file"
                        | "cics.interval"
                        | "cics.program"
                        | "cics.queue"
                        | "cics.recovery"
                        | "cics.storage"
                        | "cics.task"
                        | "cics.terminal"
                        | "cics.time"
                        | "cics.transform"
                        | "cics.event"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), hir_plans.len());
        assert_eq!(
            operations
                .iter()
                .map(|operation| format!(
                    "{}@{}.{}",
                    operation.identity.namespace(),
                    operation.identity.major(),
                    operation.identity.name()
                ))
                .collect::<BTreeSet<_>>(),
            hir_plans
                .iter()
                .map(|plan| cics_executable_descriptor(plan.operation)
                    .identity()
                    .to_string())
                .collect()
        );
        let catalog = crate::core_mir_catalog();
        for operation in operations {
            assert!(!operation.attributes.contains_key("arguments"));
            assert!(!operation.attributes.contains_key("control_text"));
            assert!(operation.location.is_some());
            assert_eq!(
                operation
                    .storage
                    .iter()
                    .map(|reference| reference.storage)
                    .collect::<BTreeSet<_>>()
                    .len(),
                operation.storage.len()
            );
            assert!(
                operation
                    .storage
                    .iter()
                    .all(|reference| reference.offset == 0 && reference.length > 0)
            );
            let Attribute::Bytes(bytes) = &operation.attributes["cics_plan"] else {
                panic!("typed executable CICS plan bytes")
            };
            let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
            assert_eq!(
                operation
                    .storage
                    .iter()
                    .map(|reference| reference.storage)
                    .collect::<BTreeSet<_>>(),
                cics_plan_storage(&plan)
            );
            for reference in &operation.storage {
                assert_eq!(
                    module
                        .storage()
                        .iter()
                        .find(|region| region.id == reference.storage)
                        .unwrap()
                        .size,
                    reference.length
                );
            }
            let descriptor = cics_executable_descriptor(plan.operation);
            assert_eq!(operation.identity, descriptor.identity());
            assert_eq!(operation.effects, descriptor.effects);
            let schema = catalog.get(&operation.identity).expect("typed CICS schema");
            assert_eq!(
                schema.runtime_import.as_deref(),
                Some(descriptor.runtime_import)
            );
            assert_eq!(
                schema.allowed_effects,
                operation.effects.iter().copied().collect::<BTreeSet<_>>()
            );
            for forbidden in cics_grammar_tokens() {
                assert!(
                    !bytes
                        .windows(forbidden.len())
                        .any(|window| window == forbidden)
                );
            }
        }
    }

    #[test]
    fn readq_td_compiler_plans_require_one_destination_and_matching_length_output() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. READTDQP. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(6). 01 PTR-X POINTER. 01 LENGTH-X PIC S9(4) COMP VALUE 6. PROCEDURE DIVISION. EXEC CICS READQ TD QUEUE('IN01') INTO(DATA-X) LENGTH(LENGTH-X) END-EXEC. EXEC CICS READQ TD QUEUE('IN01') SET(PTR-X) LENGTH(LENGTH-X) END-EXEC. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis
            .hir
            .as_ref()
            .unwrap_or_else(|| panic!("READQ TD plans: {:?}", analysis.diagnostics));
        let plans = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
                    (plan.operation == CicsPlanOperation::ReadTransientData).then_some(plan)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(plans.len(), 2);

        for (plan, destination) in [
            (&plans[0], CicsOutputName::Into),
            (&plans[1], CicsOutputName::SetPointer),
        ] {
            let length_slot = match plan
                .operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Length)
                .map(|operand| &operand.value)
            {
                Some(CicsOperandValue::Storage(slot)) => slot,
                other => panic!("READQ TD LENGTH storage: {other:?}"),
            };
            assert!(plan.outputs.iter().any(|output| output.name == destination));
            assert_eq!(
                &plan
                    .outputs
                    .iter()
                    .find(|output| output.name == CicsOutputName::Length)
                    .expect("READQ TD LENGTH output")
                    .target,
                length_slot
            );
            assert!(encode_cics_effect_plan(plan, CicsPlanLimits::default()).is_ok());
        }

        let malformed = |plan: &CicsEffectPlan| {
            assert_eq!(
                encode_cics_effect_plan(plan, CicsPlanLimits::default()),
                Err(CicsPlanCodecProblem::Malformed)
            );
        };

        let mut missing_destination = plans[0].clone();
        missing_destination
            .outputs
            .retain(|output| output.name != CicsOutputName::Into);
        malformed(&missing_destination);

        let set_pointer = plans[1]
            .outputs
            .iter()
            .find(|output| output.name == CicsOutputName::SetPointer)
            .expect("READQ TD SET output")
            .clone();
        let mut conflicting_destinations = plans[0].clone();
        conflicting_destinations.outputs.push(set_pointer.clone());
        malformed(&conflicting_destinations);

        let mut literal_length = plans[0].clone();
        literal_length
            .operands
            .iter_mut()
            .find(|operand| operand.name == CicsOperandName::Length)
            .expect("READQ TD LENGTH operand")
            .value = CicsOperandValue::Integer(6);
        malformed(&literal_length);

        let mut mismatched_length_output = plans[0].clone();
        mismatched_length_output
            .outputs
            .iter_mut()
            .find(|output| output.name == CicsOutputName::Length)
            .expect("READQ TD LENGTH output")
            .target = set_pointer.target;
        malformed(&mismatched_length_output);
    }

    fn cics_grammar_tokens() -> [&'static [u8]; 21] {
        [
            b"DEQ",
            b"ENQ",
            b"READ",
            b"REWRITE",
            b"SYNCPOINT",
            b"FILE",
            b"DATASET",
            b"RIDFLD",
            b"INTO",
            b"RESP",
            b"RESP2",
            b"UPDATE",
            b"ROLLBACK",
            b"NOHANDLE",
            b"RESOURCE",
            b"LENGTH",
            b"MAXLIFETIME",
            b"TASK",
            b"UOW",
            b"NOSUSPEND",
            b"END-EXEC",
        ]
    }

    fn cics_plan_storage(plan: &CicsEffectPlan) -> BTreeSet<StorageId> {
        let mut storage = plan
            .operands
            .iter()
            .filter_map(|operand| match &operand.value {
                CicsOperandValue::Literal(_) => None,
                CicsOperandValue::Storage(slot) | CicsOperandValue::LengthOf(slot) => {
                    Some(slot.storage)
                }
                CicsOperandValue::Integer(_) => None,
            })
            .chain(plan.outputs.iter().map(|output| output.target.storage))
            .collect::<BTreeSet<_>>();
        if let CicsCondition::Respond {
            response,
            response2,
        } = &plan.condition
        {
            storage.insert(response.storage);
            storage.extend(response2.iter().map(|response| response.storage));
        }
        storage
    }

    #[test]
    fn typed_retrieve_and_numeric_file_form_keep_distinct_routes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(4). 01 LENGTH-X PIC S9(4) COMP VALUE 4. PROCEDURE DIVISION. EXEC CICS RETRIEVE INTO(RECORD-X) LENGTH(LENGTH-X) END-EXEC. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(003) END-EXEC. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis
            .hir
            .as_ref()
            .expect("mixed typed and compatibility CICS HIR");
        let statements = hir
            .statements
            .iter()
            .filter(|statement| statement.kind == crate::StatementKind::ExecCics)
            .collect::<Vec<_>>();
        assert_eq!(statements.len(), 2);
        assert!(matches!(
            statements[0].resolved.as_ref(),
            Some(crate::HirResolvedStatement::Cics(crate::HirCicsStatement {
                operation: crate::HirCicsOperation::Retrieve,
                ..
            }))
        ));
        assert!(statements[1].resolved.is_none());
        assert!(
            statements[0]
                .arguments
                .iter()
                .any(|argument| argument == "RETRIEVE")
        );
        assert!(
            statements[1]
                .arguments
                .iter()
                .any(|argument| argument == "003")
        );
        let hir_operations = hir
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| matches!(operation.identity.name(), "exec_cics" | "retrieve"))
            .collect::<Vec<_>>();
        assert_eq!(hir_operations.len(), 2);
        assert!(hir_operations.iter().any(|operation| {
            operation.identity.namespace() == "cobol.hir"
                && operation.identity.major() == 2
                && operation.attributes.contains_key("cics_plan")
        }));
        assert!(hir_operations.iter().any(|operation| {
            operation.identity.namespace() == "cobol.hir"
                && operation.identity.major() == 1
                && operation.attributes.contains_key("arguments")
        }));

        let CompilerResult::Published { artifact, .. } = compiler
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published mixed CICS")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from(["cics.task@1".into(), "mainframe.core.cobol@1".into()])
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let operations = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| matches!(operation.identity.name(), "exec_cics" | "retrieve"))
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 2);
        assert!(operations.iter().any(|operation| {
            operation.identity.namespace() == "cics.task"
                && operation.identity.name() == "retrieve"
                && operation.attributes.contains_key("cics_plan")
        }));
        assert!(operations.iter().any(|operation| {
            operation.identity.namespace() == "mainframe.core.cobol"
                && operation.identity.major() == 1
                && operation.attributes.contains_key("arguments")
        }));
        let numeric = operations
            .iter()
            .find_map(|operation| match operation.attributes.get("arguments") {
                Some(Attribute::Bytes(bytes))
                    if bytes.windows(3).any(|window| window == b"003") =>
                {
                    Some(bytes)
                }
                _ => None,
            })
            .expect("numeric CICS lexeme remains exact");
        assert!(numeric.windows(3).any(|window| window == b"003"));
    }

    #[test]
    fn unresolved_function_arithmetic_keeps_the_version_one_executable_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LEGACY. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X VALUE '1'. PROCEDURE DIVISION. COMPUTE RETURN-CODE = FUNCTION NUMVAL(TEXT-X). STOP RUN.";
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published legacy arithmetic")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from(["mainframe.core.cobol@1".into()])
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let operation = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .find(|operation| operation.identity.name() == "compute")
            .expect("legacy compute operation");
        assert_eq!(operation.identity.namespace(), "mainframe.core.cobol");
        assert_eq!(operation.identity.major(), 1);
        assert!(operation.attributes.contains_key("arguments"));
        assert!(!operation.attributes.contains_key("assignment_plan"));
        assert!(operation.location.is_some());
    }

    #[test]
    fn conversation_extract_cobol_forms_lower_to_distinct_v2_plans() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CONVX. DATA DIVISION. WORKING-STORAGE SECTION. 01 CV PIC X(4). 01 NET-X PIC X(8). 01 TERM-X PIC X(4). 01 PROC-X PIC X(8). 01 PROC-LEN PIC S9(4) COMP. 01 PROC-MAX PIC S9(4) COMP VALUE 8. 01 SYNC-X PIC S9(4) COMP. 01 PIP-PTR POINTER-32. 01 PIP-LEN PIC S9(4) COMP. 01 LOGON-X PIC X(256). 01 LOGON-LEN PIC S9(4) COMP. 01 RC PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS EXTRACT PROCESS PROCNAME(PROC-X) PROCLENGTH(PROC-LEN) MAXPROCLEN(PROC-MAX) SYNCLEVEL(SYNC-X) PIPLIST(PIP-PTR) PIPLENGTH(PIP-LEN) RESP(RC) END-EXEC. EXEC CICS POINT CONVID(CV) RESP(RC) END-EXEC. EXEC CICS EXTRACT TCT NETNAME(NET-X) TERMID(TERM-X) RESP(RC) END-EXEC. EXEC CICS EXTRACT LOGONMSG INTO(LOGON-X) LENGTH(LOGON-LEN) RESP(RC) END-EXEC.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let plans = analysis
            .hir
            .unwrap()
            .module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    Some(decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            plans.iter().map(|plan| plan.operation).collect::<Vec<_>>(),
            [
                CicsPlanOperation::ExtractProcess,
                CicsPlanOperation::Point,
                CicsPlanOperation::ExtractTct,
                CicsPlanOperation::ExtractLogonMsg,
            ]
        );
        assert!(
            plans[0]
                .outputs
                .iter()
                .any(|output| output.name == CicsOutputName::PipList)
        );
        assert!(
            plans[3]
                .outputs
                .iter()
                .any(|output| output.name == CicsOutputName::LogonInto)
        );
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(&source.replace(
                    "PROCNAME(PROC-X) PROCLENGTH(PROC-LEN)",
                    "PROCNAME(PROC-X)"
                )))
                .diagnostics
                .is_empty()
        );
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(
                    &source.replace("POINT CONVID(CV)", "POINT CONVID(CV) SESSION(CV)")
                ))
                .diagnostics
                .is_empty()
        );
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(
                    &source.replace("EXTRACT PROCESS", "GDS EXTRACT PROCESS")
                ))
                .diagnostics
                .is_empty()
        );
        let gds_source = "IDENTIFICATION DIVISION. PROGRAM-ID. GDSX. DATA DIVISION. WORKING-STORAGE SECTION. 01 CV PIC X(4). 01 DATA-X PIC X(24). 01 RC-X PIC X(6). PROCEDURE DIVISION. EXEC CICS GDS EXTRACT ATTRIBUTES CONVID(CV) CONVDATA(DATA-X) RETCODE(RC-X) END-EXEC.";
        assert!(
            !CobolCompiler::default()
                .analyze(&bundle(gds_source))
                .diagnostics
                .is_empty()
        );
    }
}
