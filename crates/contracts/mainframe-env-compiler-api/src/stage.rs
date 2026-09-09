use crate::CompilerProblem;
use mainframe_env_ir::{
    LegalModule, LegalityProfile, Module, OperationCatalog, VerificationReport, verify,
    verify_legal,
};
use mainframe_env_source::SourceId;

/// A HIR module whose structural and operation-catalog invariants were checked.
///
/// Its fields are private, and the only constructor runs the authoritative IR
/// verifier. Compiler frontends retain their parsed and semantic analysis as
/// private implementation state rather than accepting caller-authored claims.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHir {
    source: SourceId,
    module: Module,
    report: VerificationReport,
}

impl VerifiedHir {
    pub fn verify(
        source: SourceId,
        module: Module,
        catalog: &OperationCatalog,
    ) -> Result<Self, CompilerProblem> {
        let report = verify(&module, catalog)
            .map_err(|problem| CompilerProblem::Verification(problem.to_string()))?;
        Ok(Self {
            source,
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

    /// Consumes the verified HIR proof and binds a lowered module to its source.
    #[must_use]
    pub fn lower(self, module: Module) -> LoweredMir {
        LoweredMir {
            source: self.source,
            verified_hir_report: self.report,
            module,
        }
    }
}

/// A lowered MIR that is provenance-bound to a consumed verified HIR stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweredMir {
    source: SourceId,
    verified_hir_report: VerificationReport,
    module: Module,
}

impl LoweredMir {
    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }

    #[must_use]
    pub fn verified_hir_report(&self) -> &VerificationReport {
        &self.verified_hir_report
    }
}

/// Executable MIR whose complete operation set passed the legality profile.
///
/// Construction consumes [`LoweredMir`]; callers cannot substitute an
/// unrelated source identity at legalization time.
///
/// ```compile_fail
/// # use mainframe_env_compiler_api::LegalizedMir;
/// # use mainframe_env_ir::{LegalityProfile, Module, OperationCatalog};
/// # use mainframe_env_source::SourceId;
/// # fn fabricate(source: SourceId, module: Module, catalog: &OperationCatalog,
/// #              profile: &LegalityProfile) {
/// let _ = LegalizedMir::legalize(source, module, catalog, profile);
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegalizedMir {
    source: SourceId,
    legal: LegalModule,
}

impl LegalizedMir {
    pub fn legalize(
        lowered: LoweredMir,
        catalog: &OperationCatalog,
        profile: &LegalityProfile,
    ) -> Result<Self, CompilerProblem> {
        let legal = verify_legal(lowered.module, catalog, profile)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        Ok(Self {
            source: lowered.source,
            legal,
        })
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
