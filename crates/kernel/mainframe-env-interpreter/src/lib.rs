//! Deterministic bounded Core-MIR reference machine.

#![forbid(unsafe_code)]

mod coordinator;
mod machine;
mod value;

pub use coordinator::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
pub use machine::{
    MachineProblem, MachineSnapshot, ReferenceMachine, encode_cobol_call_result,
    supported_operations,
};
pub use value::{FixedValue, ValueProblem};

pub const INTERPRETER_GENERATION: &str = "mainframe-env-reference@1";
