//! Bounded generic IR, schemas, verification, and owned codecs.

#![forbid(unsafe_code)]

mod catalog;
mod cics_descriptor;
mod cics_plan;
mod codec;
mod decimal_plan;
mod model;
mod semantic_verify;
mod verify;

pub use catalog::{
    CatalogProblem, CicsOperationContract, DecimalConditionContract, DecimalOperationContract,
    LegalityProfile, OperationCatalog, OperationSchema, OperationSemanticContract,
};
pub use cics_descriptor::{
    CICS_EXECUTABLE_DESCRIPTORS, CICS_RUNTIME_IMPORT, CicsExecutableDescriptor,
    cics_executable_descriptor, cics_executable_descriptor_for_identity,
};
pub use cics_plan::{
    CICS_EFFECT_PLAN_CONTRACT, CicsCondition, CicsEffectPlan, CicsNamedOperand, CicsOperandName,
    CicsOperandValue, CicsOutputBinding, CicsOutputName, CicsPlanCodecProblem, CicsPlanLimits,
    CicsPlanOperation, CicsPlanOption, CicsStorageSlot, decode_cics_effect_plan,
    encode_cics_effect_plan,
};
pub use codec::{CodecLimits, IrCodecProblem, decode_binary, encode_binary, parse_text, to_text};
pub use decimal_plan::{
    DECIMAL_ASSIGNMENT_PLAN_CONTRACT, DecimalArithmeticContext, DecimalAssignment,
    DecimalAssignmentPlan, DecimalConditionPolicy, DecimalExecutionPolicy, DecimalExpression,
    DecimalPlanCodecProblem, DecimalPlanLimits, DecimalPlanWireVersion, DecimalReceiver,
    DecimalReceiverUpdatePolicy, DecimalRoundingPolicy, DecimalStorageAbi, DecimalStorageSlot,
    LEGACY_DECIMAL_ASSIGNMENT_PLAN_CONTRACT, decimal_assignment_plan_wire_version,
    decode_decimal_assignment_plan, encode_decimal_assignment_plan,
};
pub use model::{
    Attribute, Block, BlockId, Effect, IrLimits, IrProblem, Module, ModuleBuilder, Operation,
    OperationId, OperationIdentity, Region, RegionId, StorageId, StorageReference, StorageRegion,
    TypeIdentity, ValueId,
};
pub use semantic_verify::{SemanticVerificationProblem, verify_semantic_contracts};
pub use verify::{LegalModule, VerificationProblem, VerificationReport, verify, verify_legal};

pub const IR_OBJECT_CONTRACT: &str = "mainframe-env.ir@1";
pub const IR_TEXT_CONTRACT: &str = "mainframe-env.ir-text@1";
pub const IR_BINARY_CONTRACT: &str = "mainframe-env.ir-binary@1";
pub const IR_ENVELOPE_CONTRACT: &str = "mainframe-env.ir-envelope@1";
