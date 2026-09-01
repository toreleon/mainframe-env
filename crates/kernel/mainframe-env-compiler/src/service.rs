use crate::hir::{CobolHir, HirProblem};
use crate::lower::{LowerProblem, core_mir_catalog, core_mir_profile, lower_to_core};
use crate::semantic::{SemanticModel, SemanticProblem};
use crate::syntax::{LosslessSyntax, SyntaxLimits, SyntaxProblem, decode_and_lex};
use mainframe_env_compiler_api::{
    ArtifactLimits, ArtifactManifest, CompilationMode, CompileOptions, CompilerProblem,
    CompilerRequest, CompilerResult, CompilerService, LegalizedMir, ParsedProgram,
    PublishedArtifact, SemanticProgram, VerifiedHir,
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
        let unsupported_statements = hir.unsupported();
        let incomplete_layouts = semantic.execution_incomplete_layouts();
        let incomplete_intrinsics = semantic.execution_incomplete_intrinsics();
        let incomplete_registers = semantic.execution_incomplete_special_registers();
        let incomplete_arithmetic = hir.execution_incomplete_arithmetic_receivers();
        let execution_incomplete = !unsupported_statements.is_empty()
            || !incomplete_layouts.is_empty()
            || !incomplete_intrinsics.is_empty()
            || !incomplete_registers.is_empty()
            || !incomplete_arithmetic.is_empty();
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
        for (kind, line) in incomplete_arithmetic {
            if diagnostics.len() >= self.limits.max_diagnostics {
                break;
            }
            diagnostics.push(diagnostic(
                "MECOB0204",
                Phase::Lower,
                FailureCategory::Unsupported,
                format!(
                    "{} at procedure line {line} has multiple receivers whose execution is deferred to 0.4",
                    kind.slug()
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

    fn verify_hir(
        &self,
        source: &SourceBundle,
        analysis: &CobolAnalysis,
    ) -> Result<VerifiedHir, CompilerProblem> {
        if analysis.completeness != Completeness::Complete {
            return Err(CompilerProblem::IncompleteStage);
        }
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
        let mut manifest_options = request.options.values().clone();
        manifest_options.insert(
            "cobol.effective-lp".into(),
            analysis
                .syntax
                .as_ref()
                .ok_or(CompilerProblem::IncompleteStage)?
                .effective_compiler_options()
                .lp()
                .to_string(),
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
    use mainframe_env_source::{
        LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits,
    };
    use std::collections::BTreeMap;

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
        assert!(
            semantic
                .layouts
                .iter()
                .all(|layout| !layout.source.is_empty())
        );
        assert_eq!(analysis.completeness, Completeness::Complete);
    }

    #[test]
    fn lp64_types_and_dynamic_layouts_are_typed_but_dynamic_execution_is_blocked() {
        let source = "PROCESS LP(64)\nIDENTIFICATION DIVISION. PROGRAM-ID. STORAGE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. 01 OBJ OBJECT REFERENCE. 01 DYNAMIC-ITEM PIC X DYNAMIC LENGTH LIMIT IS 64. PROCEDURE DIVISION. STOP RUN.";
        let analysis = CobolCompiler::default().analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("semantic metadata");
        assert_eq!(semantic.layout("PTR").unwrap().length, 8);
        assert_eq!(semantic.layout("OBJ").unwrap().length, 8);
        assert_eq!(
            semantic.layout("DYNAMIC-ITEM").unwrap().dynamic_limit,
            Some(64)
        );
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MECOB0201"
                && diagnostic.public_message().contains("DYNAMIC-ITEM")
        }));
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
    }

    #[test]
    fn intrinsic_and_special_register_nodes_are_typed_and_unsupported_execution_is_blocked() {
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
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MECOB0202" && diagnostic.public_message().contains("ABS")
        }));
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
    fn unsupported_special_registers_are_typed_but_never_publish() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REGISTER. PROCEDURE DIVISION. DISPLAY WHEN-COMPILED. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let semantic = analysis.semantic.as_ref().expect("register semantic model");
        assert!(semantic.special_registers.iter().any(|register| {
            register.kind == crate::SpecialRegisterKind::WhenCompiled && !register.runtime_supported
        }));
        assert!(analysis.hir.is_some());
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MECOB0203"
                && diagnostic.public_message().contains("WHEN-COMPILED")
        }));
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Failed {
                completeness: Completeness::Unsupported,
                ..
            }
        ));
    }

    #[test]
    fn comp1_and_comp2_keep_layouts_but_never_publish() {
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
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        for name in ["SHORT-X", "LONG-X"] {
            assert!(analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code().as_str() == "MECOB0201"
                    && diagnostic.public_message().contains(name)
            }));
        }
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Failed {
                completeness: Completeness::Unsupported,
                ..
            }
        ));
    }

    #[test]
    fn unsupported_state_survives_zero_and_exhausted_diagnostic_caps() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CAPS. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLOAT-X COMP-1. PROCEDURE DIVISION. DISPLAY WHEN-COMPILED FLOAT-X. STOP RUN.";
        for max_diagnostics in [0, 1] {
            let compiler = CobolCompiler::new(CobolCompilerLimits {
                max_diagnostics,
                ..CobolCompilerLimits::default()
            });
            let analysis = compiler.analyze(&bundle(source));
            assert_eq!(analysis.diagnostics.len(), max_diagnostics);
            assert_eq!(analysis.completeness, Completeness::Unsupported);
            assert!(analysis.hir.is_some());
            assert!(matches!(
                compiler
                    .compile(request(source, CompilationMode::Analyze))
                    .unwrap(),
                CompilerResult::Analysis {
                    hir: None,
                    completeness: Completeness::Unsupported,
                    ..
                }
            ));
            assert!(matches!(
                compiler
                    .compile(request(source, CompilationMode::Executable))
                    .unwrap(),
                CompilerResult::Failed {
                    completeness: Completeness::Unsupported,
                    ..
                }
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
    fn multiple_arithmetic_receivers_build_typed_hir_but_defer_execution() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MULTIRECV. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. 01 B PIC 9 VALUE 2. 01 C PIC 9 VALUE 3. PROCEDURE DIVISION. ADD A TO B C. SUBTRACT A FROM B C. STOP RUN.";
        let compiler = CobolCompiler::default();
        let analysis = compiler.analyze(&bundle(source));
        let hir = analysis.hir.as_ref().expect("typed multi-receiver HIR");
        for kind in [crate::StatementKind::Add, crate::StatementKind::Subtract] {
            assert!(hir.statements.iter().any(|statement| {
                statement.kind == kind && statement.official == kind.official_kind()
            }));
        }
        assert_eq!(analysis.completeness, Completeness::Unsupported);
        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code().as_str() == "MECOB0204")
                .count(),
            2
        );
        assert!(matches!(
            compiler
                .compile(request(source, CompilationMode::Executable))
                .unwrap(),
            CompilerResult::Failed {
                completeness: Completeness::Unsupported,
                ..
            }
        ));
    }
}
