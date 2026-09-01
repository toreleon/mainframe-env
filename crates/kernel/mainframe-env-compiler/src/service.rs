use crate::hir::{CobolHir, HirProblem};
use crate::lower::{LowerProblem, core_mir_catalog, core_mir_profile, lower_to_core};
use crate::semantic::{SemanticModel, SemanticProblem};
use crate::syntax::{LosslessSyntax, SyntaxLimits, SyntaxProblem, decode_and_lex};
use mainframe_env_compiler_api::{
    ArtifactLimits, ArtifactManifest, CompilationMode, CompilerProblem, CompilerRequest,
    CompilerResult, CompilerService, LegalizedMir, ParsedProgram, PublishedArtifact,
    SemanticProgram, VerifiedHir,
};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity,
};
use mainframe_env_ir::{CodecLimits, IrLimits, to_text};
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
        let hir = match CobolHir::build(&syntax, &semantic, self.limits.ir) {
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
        for kind in hir.unsupported() {
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
        let hir_text = to_text(&hir.module, self.limits.codec).ok();
        let completeness = if diagnostics.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Unsupported
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

    fn verify_hir(
        &self,
        source: &SourceBundle,
        analysis: &CobolAnalysis,
    ) -> Result<VerifiedHir, CompilerProblem> {
        let parsed = ParsedProgram::validated(
            source.id(),
            Vec::new(),
            Completeness::Complete,
            self.limits.max_diagnostics,
        )?;
        let semantic = SemanticProgram::validated(
            parsed,
            *source.id().as_bytes(),
            Vec::new(),
            Completeness::Complete,
            self.limits.max_diagnostics,
        )?;
        let hir = analysis
            .hir
            .as_ref()
            .ok_or(CompilerProblem::IncompleteStage)?;
        VerifiedHir::verify(&semantic, hir.module.clone(), &crate::cobol_hir_catalog())
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
        let verified = self.verify_hir(&request.source, &analysis).ok();
        if request.mode == CompilationMode::Analyze {
            return Ok(CompilerResult::Analysis {
                hir: verified,
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
        let hir = analysis
            .hir
            .as_ref()
            .ok_or(CompilerProblem::IncompleteStage)?;
        let mir = lower_to_core(hir, self.limits.ir).map_err(lower_problem)?;
        let legalized = LegalizedMir::legalize(
            request.source.id(),
            mir,
            &core_mir_catalog(),
            &core_mir_profile(),
        )?;
        let payload =
            mainframe_env_ir::encode_binary(legalized.legal().module(), self.limits.codec)
                .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        let manifest = ArtifactManifest {
            compiler_generation: format!("mainframe-env-cobol-{}", env!("CARGO_PKG_VERSION")),
            target: request.target,
            options: request.options,
            host_interfaces: BTreeSet::from([
                "mainframe-env.host@1".to_string(),
                "mainframe-env.cics@1".to_string(),
            ]),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.to_string(),
        };
        let artifact =
            PublishedArtifact::publish(&legalized, manifest, payload, self.limits.artifact)?;
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
    diagnostic(
        "MECOB0102",
        Phase::Parse,
        category,
        format!("COBOL HIR construction failed: {problem:?}"),
    )
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
    use mainframe_env_source::{
        LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits,
    };
    use std::collections::BTreeMap;
    fn bundle(source: &str) -> SourceBundle {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("HELLO.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "HELLO.cbl",
            source.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap()
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
        assert_eq!(analysis.completeness, Completeness::Complete);
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
}
