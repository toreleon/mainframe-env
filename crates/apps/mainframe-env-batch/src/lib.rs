//! Bounded JCL planning, durable JES lifecycle, spool, and program routing.

#![forbid(unsafe_code)]

mod controller;
mod jcl;
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
pub use program::{
    Program, ProgramInput, ProgramOutput, ProgramRouter, SystemServiceProgram, UtilityDisposition,
    common_program_catalog_sha256, decode_program_output, system_service_program,
    utility_disposition,
};
pub use service::{BatchLimits, BatchService, JobSnapshot, JobState, validate_idcams_control};
