//! Bounded COBOL frontend, typed HIR, Core-MIR lowering, and compiler service.

#![forbid(unsafe_code)]

mod compatibility;
mod hir;
mod lower;
mod semantic;
mod service;
mod syntax;

pub use hir::{CobolHir, HirStatement, StatementKind, cobol_hir_catalog};
pub use lower::{CORE_NAMESPACE, core_mir_catalog, core_mir_profile};
pub use semantic::{
    CobolLayout, DataCategory, DataReference, ResolutionProblem, SemanticModel, StorageSection,
};
pub use service::{CobolAnalysis, CobolCompiler, CobolCompilerLimits};
pub use syntax::{
    CobolLanguage, CobolSyntaxKind, Expansion, LosslessSyntax, SourceOrigin, SourceSpan,
    SyntaxLimits,
};

pub const COBOL_HIR_DIALECT: &str = "cobol.hir@1";
pub const CORE_MIR_DIALECT: &str = "mainframe.core.cobol@1";
pub use compatibility::{
    COMPATIBILITY_COPYBOOK_CONTRACT, CompatibilityCopybook, CompatibilityProblem,
    compatibility_copybooks, owned_compatibility_library,
};
