use crate::CompilerProblem;
use mainframe_env_diagnostics::{Completeness, Diagnostic, Severity};
use mainframe_env_ir::{
    LegalModule, LegalityProfile, Module, OperationCatalog, VerificationReport, verify,
    verify_legal,
};
use mainframe_env_source::SourceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedProgram {
    source: SourceId,
    diagnostics: Vec<Diagnostic>,
    completeness: Completeness,
}

impl ParsedProgram {
    pub fn validated(
        source: SourceId,
        diagnostics: Vec<Diagnostic>,
        completeness: Completeness,
        max_diagnostics: usize,
    ) -> Result<Self, CompilerProblem> {
        validate_diagnostics(&diagnostics, max_diagnostics)?;
        Ok(Self {
            source,
            diagnostics,
            completeness,
        })
    }

    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticProgram {
    parsed: ParsedProgram,
    semantic_identity: [u8; 32],
    diagnostics: Vec<Diagnostic>,
    completeness: Completeness,
}

impl SemanticProgram {
    pub fn validated(
        parsed: ParsedProgram,
        semantic_identity: [u8; 32],
        diagnostics: Vec<Diagnostic>,
        completeness: Completeness,
        max_diagnostics: usize,
    ) -> Result<Self, CompilerProblem> {
        validate_diagnostics(&diagnostics, max_diagnostics)?;
        if parsed.completeness != Completeness::Complete && completeness == Completeness::Complete {
            return Err(CompilerProblem::IncompleteStage);
        }
        Ok(Self {
            parsed,
            semantic_identity,
            diagnostics,
            completeness,
        })
    }

    #[must_use]
    pub fn parsed(&self) -> &ParsedProgram {
        &self.parsed
    }
    #[must_use]
    pub const fn semantic_identity(&self) -> &[u8; 32] {
        &self.semantic_identity
    }
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHir {
    source: SourceId,
    module: Module,
    report: VerificationReport,
}

impl VerifiedHir {
    pub fn verify(
        semantic: &SemanticProgram,
        module: Module,
        catalog: &OperationCatalog,
    ) -> Result<Self, CompilerProblem> {
        if semantic.completeness != Completeness::Complete
            || semantic.parsed.completeness != Completeness::Complete
            || semantic
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity() == Severity::Error)
            || semantic
                .parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity() == Severity::Error)
        {
            return Err(CompilerProblem::IncompleteStage);
        }
        let report = verify(&module, catalog)
            .map_err(|problem| CompilerProblem::Verification(problem.to_string()))?;
        Ok(Self {
            source: semantic.parsed.source,
            module,
            report,
        })
    }

    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    #[must_use]
    pub fn module(&self) -> &Module {
        &self.module
    }
    #[must_use]
    pub fn report(&self) -> &VerificationReport {
        &self.report
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegalizedMir {
    source: SourceId,
    legal: LegalModule,
}

impl LegalizedMir {
    pub fn legalize(
        source: SourceId,
        module: Module,
        catalog: &OperationCatalog,
        profile: &LegalityProfile,
    ) -> Result<Self, CompilerProblem> {
        let legal = verify_legal(module, catalog, profile)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        Ok(Self { source, legal })
    }

    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    #[must_use]
    pub fn legal(&self) -> &LegalModule {
        &self.legal
    }
}

fn validate_diagnostics(diagnostics: &[Diagnostic], max: usize) -> Result<(), CompilerProblem> {
    if diagnostics.len() > max {
        Err(CompilerProblem::TooManyDiagnostics)
    } else {
        Ok(())
    }
}
