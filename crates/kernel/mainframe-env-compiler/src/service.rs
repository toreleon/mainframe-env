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
        Attribute, CICS_EXECUTABLE_DESCRIPTORS, CicsCondition, CicsEffectPlan, CicsOperandName,
        CicsOperandValue, CicsOutputName, CicsPlanLimits, CicsPlanOperation, CodecLimits,
        DecimalExecutionPolicy, DecimalPlanLimits, StorageId, cics_executable_descriptor,
        decode_binary, decode_cics_effect_plan, decode_decimal_assignment_plan, encode_binary,
    };
    use mainframe_env_source::{
        LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn bundle(source: &str) -> SourceBundle {
        bundle_in_format(source, SourceFormat::Free)
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
    fn typed_cics_is_proof_bound_in_hir_and_the_published_executable() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSP. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(4). 01 KEY-X PIC X(3) VALUE '003'. 01 LOCK-X PIC X(4) VALUE 'LOCK'. 01 PTR-X POINTER. 01 CORR-X PIC X(80) VALUE ALL 'A'. 01 PRIORITY-X PIC S9(4) COMP VALUE 200. 01 CODE-A PIC S9(9) COMP. 01 CODE-B PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ASKTIME END-EXEC. EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(RECORD-X) RIDFLD(KEY-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS REWRITE DATASET('ACCTDAT') FROM(RECORD-X) END-EXEC. EXEC CICS ENQ RESOURCE(LOCK-X) LENGTH(4) UOW NOSUSPEND END-EXEC. EXEC CICS DEQ RESOURCE(LOCK-X) LENGTH(4) UOW END-EXEC. EXEC CICS ADDRESS SET(PTR-X) USING(ADDRESS OF RECORD-X) END-EXEC. EXEC CICS CHANGE TASK PRIORITY(PRIORITY-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS HANDLE AID ANYKEY(AID-HANDLER) ENTER END-EXEC. EXEC CICS HANDLE CONDITION ERROR(ERROR-HANDLER) LENGERR END-EXEC. EXEC CICS IGNORE CONDITION PGMIDERR END-EXEC. EXEC CICS PUSH HANDLE END-EXEC. EXEC CICS POP HANDLE RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS SET ASSOCIATION USERCORRDATA(CORR-X) RESP(CODE-A) RESP2(CODE-B) END-EXEC. EXEC CICS SUSPEND END-EXEC. EXEC CICS SYNCPOINT ROLLBACK NOHANDLE END-EXEC. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis.hir.as_ref().expect("typed CICS HIR");
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
        assert_eq!(typed_hir.len(), 15);
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
                CicsPlanOperation::AddressSet => crate::HirCicsOperation::AddressSet,
                CicsPlanOperation::AsktimeEib => crate::HirCicsOperation::AsktimeEib,
                CicsPlanOperation::ChangeTask => crate::HirCicsOperation::ChangeTask,
                CicsPlanOperation::Deq => crate::HirCicsOperation::Deq,
                CicsPlanOperation::Enq => crate::HirCicsOperation::Enq,
                CicsPlanOperation::HandleAid => crate::HirCicsOperation::HandleAid,
                CicsPlanOperation::HandleCondition => crate::HirCicsOperation::HandleCondition,
                CicsPlanOperation::IgnoreCondition => crate::HirCicsOperation::IgnoreCondition,
                CicsPlanOperation::PopHandle => crate::HirCicsOperation::PopHandle,
                CicsPlanOperation::PushHandle => crate::HirCicsOperation::PushHandle,
                CicsPlanOperation::Read => crate::HirCicsOperation::Read,
                CicsPlanOperation::Rewrite => crate::HirCicsOperation::Rewrite,
                CicsPlanOperation::SetAssociationUserCorrData => {
                    crate::HirCicsOperation::SetAssociationUserCorrData
                }
                CicsPlanOperation::Syncpoint => crate::HirCicsOperation::Syncpoint,
                CicsPlanOperation::Suspend => crate::HirCicsOperation::Suspend,
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
                CicsPlanOperation::AddressSet,
                CicsPlanOperation::AsktimeEib,
                CicsPlanOperation::ChangeTask,
                CicsPlanOperation::Deq,
                CicsPlanOperation::Enq,
                CicsPlanOperation::HandleAid,
                CicsPlanOperation::HandleCondition,
                CicsPlanOperation::IgnoreCondition,
                CicsPlanOperation::PopHandle,
                CicsPlanOperation::PushHandle,
                CicsPlanOperation::Read,
                CicsPlanOperation::Rewrite,
                CicsPlanOperation::SetAssociationUserCorrData,
                CicsPlanOperation::Syncpoint,
                CicsPlanOperation::Suspend,
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
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published typed CICS")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from([
                "cics.file@1".into(),
                "cics.recovery@1".into(),
                "cics.task@1".into(),
                "cics.time@1".into(),
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
                    "cics.file" | "cics.recovery" | "cics.task" | "cics.time"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 15);
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
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.identity().to_string())
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
                CicsOperandValue::Storage(slot) => Some(slot.storage),
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
    fn unsupported_and_numeric_cics_forms_keep_the_version_one_route() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(4). PROCEDURE DIVISION. EXEC CICS WRITEQ TD QUEUE('OUTQ') FROM(RECORD-X) END-EXEC. EXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(003) END-EXEC. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis.hir.as_ref().expect("legacy-compatible CICS HIR");
        let statements = hir
            .statements
            .iter()
            .filter(|statement| statement.kind == crate::StatementKind::ExecCics)
            .collect::<Vec<_>>();
        assert_eq!(statements.len(), 2);
        assert!(
            statements
                .iter()
                .all(|statement| statement.resolved.is_none())
        );
        assert!(
            statements[0]
                .arguments
                .iter()
                .any(|argument| argument == "WRITEQ")
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
            .filter(|operation| operation.identity.name() == "exec_cics")
            .collect::<Vec<_>>();
        assert_eq!(hir_operations.len(), 2);
        assert!(hir_operations.iter().all(|operation| {
            operation.identity.namespace() == "cobol.hir"
                && operation.identity.major() == 1
                && operation.attributes.contains_key("arguments")
                && !operation.attributes.contains_key("cics_plan")
        }));

        let CompilerResult::Published { artifact, .. } = compiler
            .compile(request(source, CompilationMode::Executable))
            .unwrap()
        else {
            panic!("published legacy CICS")
        };
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from(["mainframe.core.cobol@1".into()])
        );
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let operations = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| operation.identity.name() == "exec_cics")
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 2);
        assert!(operations.iter().all(|operation| {
            operation.identity.namespace() == "mainframe.core.cobol"
                && operation.identity.major() == 1
                && operation.attributes.contains_key("arguments")
                && !operation.attributes.contains_key("cics_plan")
        }));
        let numeric = operations
            .iter()
            .find_map(|operation| match &operation.attributes["arguments"] {
                Attribute::Bytes(bytes) if bytes.windows(3).any(|window| window == b"003") => {
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
}
