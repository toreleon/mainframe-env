//! Bounded JCL planning, durable JES lifecycle, spool, and program routing.

#![forbid(unsafe_code)]

mod controller;
mod jcl;
mod jcl_schema;
mod jcl_syntax;
mod program;
mod service;

pub use controller::{
    BATCH_CONTROLLER_REGISTRY_CONTRACT, BatchControllerDefinition, BatchControllerGeneration,
    BatchControllerInstallReceipt, BatchControllerPlan, BatchControllerProgram,
    BatchControllerSelector,
};

pub use jcl::{
    DdPlan, Disposition, JclBundle, JclLimits, JobPlan, StepCondition, StepPlan, parse_jcl,
};
pub use jcl_schema::{
    JCL_PLAN_CONTRACT, JclCapabilityRequirement, JclCapabilityState, JclDiagnosticProjection,
    JclGeneratedIdentity, JclParameterNode, JclParameterOutcome, JclPlanDocument, JclPlanNode,
    JclProcedureDefinition, JclRelatedDiagnostic, JclSourceOrigin, JclSourceOriginKind,
    JclSourceSpan, JclStatementNode, JclSymbolDefinition,
};
pub use jcl_syntax::{
    JCL_SYNTAX_CONTRACT, JclLanguage, JclLosslessSyntax, JclRecord, JclRecordKind,
    JclSyntaxAnalysis, JclSyntaxKind, JclSyntaxLimits, JclSyntaxProblem, analyze_jcl_syntax,
};
pub use program::{
    Program, ProgramInput, ProgramOutput, ProgramRouter, SystemServiceProgram, UtilityDisposition,
    common_program_catalog_sha256, decode_program_output, system_service_program,
    utility_disposition,
};
pub use service::{BatchLimits, BatchService, JobSnapshot, JobState, validate_idcams_control};
