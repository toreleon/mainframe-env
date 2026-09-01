//! Bounded COBOL frontend, typed HIR, Core-MIR lowering, and compiler service.

#![forbid(unsafe_code)]

mod generated;
mod hir;
mod lower;
mod semantic;
mod service;
mod syntax;

pub use generated::cobol_language::{
    COMPILER_DIRECTING_STATEMENTS, COMPILER_DIRECTIVE_GROUPS, COMPILER_DIRECTIVES,
    CompilerDirectingDescriptor, CompilerDirectingKind, CompilerDirectiveDescriptor,
    CompilerDirectiveGroup, CompilerDirectiveGroupDescriptor, CompilerDirectiveKind,
    compiler_directing_descriptor, compiler_directive_descriptor,
};

pub use hir::{
    CobolHir, ControlEdge, ControlEdgeKind, ControlNode, ControlRole, ControlScope, HirStatement,
    StatementKind, cobol_hir_catalog,
};
pub use lower::{CORE_NAMESPACE, core_mir_catalog, core_mir_profile};
pub use semantic::{
    CobolFileBinding, CobolLayout, DataCategory, DataReference, ResolutionProblem, SemanticModel,
    StorageSection,
};
pub use service::{CobolAnalysis, CobolCompiler, CobolCompilerLimits};
pub use syntax::{
    CobolLanguage, CobolSyntaxKind, CompilerDirectingNode, CompilerDirectiveNode, CompilerOption,
    CompilerOptionSet, Expansion, LosslessSyntax, SourceOrigin, SourceSpan, SyntaxLimits,
    SyntaxToken, SyntaxTokenId,
};

pub const COBOL_HIR_DIALECT: &str = "cobol.hir@1";
pub const CORE_MIR_DIALECT: &str = "mainframe.core.cobol@1";
