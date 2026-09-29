//! Bounded JCL planning, durable JES lifecycle, spool, and program routing.

#![forbid(unsafe_code)]

mod ams;
mod controller;
mod dd;
mod dd_hydration;
mod jcl;
mod jcl_catalog;
mod jcl_expand;
mod jcl_graph;
mod jcl_jecl;
mod jcl_plan;
mod jcl_schema;
mod jcl_statement;
mod jcl_syntax;
mod jes;
mod jes_children;
mod program;
mod service;

pub use controller::{
    BATCH_CONTROLLER_REGISTRY_CONTRACT, BatchControllerDefinition, BatchControllerGeneration,
    BatchControllerInstallReceipt, BatchControllerPlan, BatchControllerProgram,
    BatchControllerSelector,
};

pub use dd::{
    DdAllocationPlan, DdDispositionPlan, DdSourceKind, DdStatusDisposition, DdTerminalDisposition,
    JES_DD_ALLOCATION_CONTRACT, plan_dd_allocations,
};

pub use ams::{
    AMS_GRAMMAR_CONTRACT, AMS_GRAMMAR_SHA256, AmsCommand, AmsComparison, AmsRegister, AmsStatement,
    parse_idcams_control, validate_idcams_control,
};

pub use jcl::{
    DdPlan, Disposition, JclBundle, JclLimits, JobPlan, OutputPlan, StepCondition, StepPlan,
    parse_jcl,
};
pub use jcl_catalog::{
    DD_PARAMETERS, DdParameterId, EXEC_PARAMETERS, ExecParameterId, JCL_GENERATED_CATALOG_SHA256,
    JCL_OFFICIAL_CATALOG_SHA256, JCL_PLAN_SCHEMA_SHA256, JCL_PLANNER_SEMANTICS_SHA256,
    JCL_STATEMENTS, JES2_STATEMENTS, JOB_PARAMETERS, JclCatalogEntry, JclCatalogSupport,
    JclStatementId, JclValueShape, Jes2StatementId, JobParameterId, OUTPUT_PARAMETERS,
    OutputParameterId,
};
pub use jcl_expand::{
    JclBackwardReference, JclExpandedProcedure, JclExpandedStatement, JclExpandedSymbol,
    JclExpansion, JclExpansionLimits, JclExpansionProblem, expand_jcl,
};
pub use jcl_jecl::{JclExpandedJecl, JclParsedJecl, Jes2StatementAnalysis, parse_jes2_statements};
pub use jcl_plan::{JclConversion, JclConversionLimits, JclConversionProblem, convert_jcl};
pub use jcl_schema::{
    JCL_PLAN_CONTRACT, JclCapabilityRequirement, JclCapabilityState, JclDiagnosticProjection,
    JclGeneratedIdentity, JclParameterNode, JclParameterOutcome, JclPlanDocument, JclPlanNode,
    JclProcedureDefinition, JclRelatedDiagnostic, JclSourceOrigin, JclSourceOriginKind,
    JclSourceSpan, JclStatementNode, JclSymbolDefinition,
};
pub use jcl_statement::{
    JclParameterIdentity, JclParsedParameter, JclParsedStatement, JclStatementAnalysis,
    parse_jcl_statements,
};
pub use jcl_syntax::{
    JCL_SYNTAX_CONTRACT, JclLanguage, JclLosslessSyntax, JclRecord, JclRecordFields, JclRecordKind,
    JclSyntaxAnalysis, JclSyntaxKind, JclSyntaxLimits, JclSyntaxProblem, analyze_jcl_syntax,
};
pub use jes::{
    CancellationState, InitiatorDefinition, JES_CHECKPOINT_CONTRACT, JES_DURABLE_JOB_CONTRACT,
    JES_OUTPUT_CONTRACT, JES_RUNTIME_CONTRACT, JES_SPOOL_CONTRACT, JES_TOPOLOGY_CONTRACT,
    JES_UTILITY_REGISTRY_CONTRACT, JesCancellation, JesCheckpoint, JesClassDefinition,
    JesControlOperation, JesJobKind, JesJobRoute, JesMasMemberDefinition, JesNodeDefinition,
    JesOutputGroup, JesOutputState, JesQueue, JesSchedulerConfiguration, JesSpoolDescriptor,
    JesSpoolState, JesSubmissionOrigin, JesTopology, JobSelectionCandidate, JobState,
    StepExecution, StepState, StepTermination, UtilityFamily, UtilityHandler, select_job,
};
pub use program::{
    Program, ProgramExecutionContext, ProgramInput, ProgramOutput, ProgramRegistration,
    ProgramRouter, ProgramTermination, RegisteredProgramHandler, SystemServiceProgram,
    UtilityDisposition, common_program_catalog_sha256, decode_program_output,
    resolve_program_registration, system_service_program, utility_disposition,
};
pub use service::{BatchLimits, BatchService, JobSnapshot};
