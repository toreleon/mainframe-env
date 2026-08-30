//! Bounded JCL planning, durable JES lifecycle, spool, and program routing.

#![forbid(unsafe_code)]

mod jcl;
mod program;
mod service;

pub use jcl::{
    DdPlan, Disposition, JclBundle, JclLimits, JobPlan, StepCondition, StepPlan, parse_jcl,
};
pub use program::{
    Program, ProgramInput, ProgramOutput, ProgramRouter, UtilityDisposition, decode_program_output,
    utility_disposition,
};
pub use service::{BatchLimits, BatchService, JobSnapshot, JobState, validate_idcams_control};
