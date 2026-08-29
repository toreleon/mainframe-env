//! Deterministic bounded Core-MIR reference machine.

#![forbid(unsafe_code)]

mod machine;
mod value;

pub use machine::{MachineProblem, MachineSnapshot, ReferenceMachine, supported_operations};
pub use value::{FixedValue, ValueProblem};

pub const INTERPRETER_GENERATION: &str = "mainframe-env-reference@1";
