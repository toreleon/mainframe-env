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
    ClauseDescriptor, CompilerDirectingDescriptor, CompilerDirectingKind,
    CompilerDirectiveDescriptor, CompilerDirectiveGroup, CompilerDirectiveGroupDescriptor,
    CompilerDirectiveKind, DATA_DESCRIPTION_CLAUSES, DataDescriptionClauseKind,
    FILE_DESCRIPTION_CLAUSES, FileDescriptionClauseKind, INTRINSIC_FUNCTIONS,
    IntrinsicArgumentClass, IntrinsicFunctionDescriptor, IntrinsicFunctionKind,
    IntrinsicResultRule, IntrinsicSignature, PROCEDURE_STATEMENTS, ProcedureStatementDescriptor,
    ProcedureStatementKind, SPECIAL_REGISTERS, SpecialRegisterDescriptor, SpecialRegisterKind,
    SpecialRegisterLengthKind, SpecialRegisterOperand, SpecialRegisterUsage,
    SpecialRegisterValueType, compiler_directing_descriptor, compiler_directive_descriptor,
    data_description_clause_descriptor, file_description_clause_descriptor,
    intrinsic_function_descriptor, intrinsic_function_named, procedure_statement_descriptor,
    special_register_descriptor, special_register_named,
};

pub use hir::{
    CobolHir, ControlEdge, ControlEdgeKind, ControlNode, ControlRole, ControlScope, HirAddMode,
    HirAddStatement, HirArithmeticReceiver, HirBinaryOperator, HirCicsConditionPolicy,
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsStatement, HirCicsValue, HirComputeStatement, HirCorrespondingPair,
    HirDataReference, HirNumericExpression, HirNumericLiteral, HirResolvedStatement, HirRounding,
    HirSizeErrorPolicy, HirStatement, HirUnaryOperator, StatementKind, StatementOption,
    StatementOptionKind, cobol_hir_catalog,
};
pub use lower::{
    CORE_NAMESPACE, PUBLISHABLE_LAYOUT_CATEGORIES, core_mir_catalog, core_mir_profile,
};
pub use semantic::{
    CobolClauseKind, CobolClauseNode, CobolDataDescription, CobolDivisionKind, CobolDivisionNode,
    CobolFileBinding, CobolFileDescription, CobolIntrinsicArgument, CobolIntrinsicCall,
    CobolLayout, CobolScope, CobolScopeId, CobolScopeKind, CobolSectionKind, CobolSectionNode,
    CobolSpecialRegisterReference, CobolTableKey, CobolUsage, DataCategory, DataReference,
    IntrinsicValueType, ResolutionProblem, SemanticModel, StorageSection,
};
pub use service::{CobolAnalysis, CobolCompiler, CobolCompilerLimits};
pub use syntax::{
    CobolLanguage, CobolSyntaxKind, CompilerDirectingNode, CompilerDirectiveNode, CompilerOption,
    CompilerOptionSet, EffectiveArithmeticMode, EffectiveCompilerOptions, EffectiveDisplaySign,
    Expansion, LosslessSyntax, SourceOrigin, SourceSpan, SyntaxLimits, SyntaxToken, SyntaxTokenId,
};

/// Historical token-oriented COBOL HIR dialect retained for compatibility.
pub const COBOL_HIR_DIALECT: &str = "cobol.hir@1";
/// Typed COBOL HIR dialect used by resolved statement families.
pub const COBOL_TYPED_HIR_DIALECT: &str = "cobol.hir@2";
/// Every COBOL HIR dialect version the compiler may currently emit.
pub const COBOL_HIR_DIALECTS: &[&str] = &[COBOL_HIR_DIALECT, COBOL_TYPED_HIR_DIALECT];
pub const CORE_MIR_DIALECT: &str = "mainframe.core.cobol@1";
