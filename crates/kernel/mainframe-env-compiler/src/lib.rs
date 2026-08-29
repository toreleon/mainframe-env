//! Bounded COBOL frontend, typed HIR, Core-MIR lowering, and compiler service.

#![forbid(unsafe_code)]

mod hir;
mod lower;
mod semantic;
mod service;
mod syntax;

pub use hir::{CobolHir, HirStatement, StatementKind, cobol_hir_catalog};
pub use lower::{CORE_NAMESPACE, core_mir_catalog, core_mir_profile};
pub use semantic::{CobolLayout, DataCategory, SemanticModel};
pub use service::{CobolAnalysis, CobolCompiler, CobolCompilerLimits};
pub use syntax::{CobolLanguage, CobolSyntaxKind, Expansion, LosslessSyntax, SyntaxLimits};

pub const COBOL_HIR_DIALECT: &str = "cobol.hir@1";
pub const CORE_MIR_DIALECT: &str = "mainframe.core.cobol@1";
