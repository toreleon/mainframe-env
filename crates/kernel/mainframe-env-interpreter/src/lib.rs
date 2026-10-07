//! Deterministic bounded Core-MIR reference machine.

#![forbid(unsafe_code)]

mod coordinator;
mod machine;
mod recovery;
mod runtime;
pub mod storage64;
mod value;

pub use coordinator::{
    CheckedReplayAuditCapture, CheckedReplayObservations, CoordinatorLimits, ExecutionControl,
    ExecutionControlError, ExecutionCoordinator, NativeChildEnrollment, NativeRootAdmission,
    NativeRootConfiguration, NativeRootHooks, NativeRootTermination, WinningRootTerminal,
};
pub use machine::typed_mq::{MqMqiNativePoint, MqMqiNativePointTarget, MqMqiNativeStructure};
pub use machine::{
    InstalledProgramReturn, InstalledProgramReturnKind, MachineProblem, MachineSnapshot,
    MqMqiAbiScope, MqMqiConnxProfile, MqMqiProgramFrame, MqMqiProgramProfile, ReferenceMachine,
    SUPPORTED_LAYOUT_CATEGORIES, encode_cobol_call_result, supported_operations,
};
pub use recovery::{
    EffectRecoveryLimits, EffectRecoveryReport, EffectRecoveryResolution, StaleEffectRecoveryWorker,
};
pub use runtime::{
    BinaryFloating32, BinaryFloating64, CobolArithmetic, CobolArithmeticFlags, CobolArithmeticMode,
    CobolDecimal, CobolRounding, CobolRuntimeEnvironment, CobolRuntimeLimits, DecimalFloating128,
    RuntimeContractProblem, TextEncoding,
};
pub use value::{FixedValue, ValueProblem};

pub const INTERPRETER_GENERATION: &str = "mainframe-env-reference@1";
