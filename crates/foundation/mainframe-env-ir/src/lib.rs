//! Bounded generic IR, schemas, verification, and owned codecs.

#![forbid(unsafe_code)]

mod catalog;
mod cics_administrative;
mod cics_descriptor;
mod cics_plan;
mod cobol_config;
mod cobol_layout;
mod cobol_reserved_words;
mod codec;
mod decimal_plan;
mod model;
mod semantic_verify;
mod verify;

pub use catalog::{
    COBOL_LAYOUT_DEFINITION_MAJOR, COBOL_LAYOUT_DEFINITION_NAME, COBOL_LAYOUT_DEFINITION_NAMESPACE,
    CatalogProblem, CicsOperationContract, DecimalConditionContract, DecimalOperationContract,
    LegalityProfile, OperationCatalog, OperationSchema, OperationSemanticContract,
    cobol_layout_definition_identity, cobol_layout_definition_schema,
};
pub use cics_administrative::{
    CICS_SPI_FEPI_AUTOMATIC_REGISTRATION, CICS_SPI_FEPI_COVERAGE_CREDIT,
    CICS_SPI_FEPI_IDENTITY_REGISTRY, CICS_SPI_FEPI_IDENTITY_REGISTRY_SHA256,
    CICS_SPI_FEPI_PUBLIC_ROUTES, CICS_SPI_FEPI_RUNTIME_HANDLERS, CICS_SPI_FEPI_SEMANTIC_AUTHORITY,
    CICS_SPI_FEPI_SOURCE_AUTHORITY_SHA256, CICS_SPI_FEPI_SOURCE_TOPIC,
    CICS_SPI_FEPI_SOURCE_TOPIC_SHA256, CicsAdministrativeCommandIdentity,
    CicsAdministrativeInterface, cics_administrative_identities_for_eibfn,
    cics_administrative_identity_for_official_row,
};
pub use cics_descriptor::{
    CICS_APPLICATION_AID_NAMES, CICS_APPLICATION_CONDITION_AUTHORITY_SHA256,
    CICS_APPLICATION_CONDITION_NAMES, CICS_APPLICATION_CONDITION_NAMES_SHA256,
    CICS_APPLICATION_REGISTRY, CICS_APPLICATION_REGISTRY_FROZEN, CICS_APPLICATION_REGISTRY_SHA256,
    CICS_EXECUTABLE_DESCRIPTORS, CICS_RUNTIME_IMPORT, CicsApplicationCobolApplicability,
    CicsApplicationConditionClauseDescriptor, CicsApplicationConditionLabelOperand,
    CicsApplicationConstraintStatus, CicsApplicationHandlerReadiness,
    CicsApplicationOptionAlternative, CicsApplicationOptionDependency,
    CicsApplicationOptionDescriptor, CicsApplicationOptionDirection,
    CicsApplicationOptionValueShape, CicsApplicationRegistryDescriptor,
    CicsApplicationRegistryMatch, CicsExecutableDescriptor,
    cics_application_registry_candidates_for_tokens,
    cics_application_registry_for_runtime_operation, cics_application_registry_for_tokens,
    cics_executable_descriptor, cics_executable_descriptor_for_identity,
};
pub use cics_plan::{
    BtsBrowseInput, BtsBrowseOutput, CICS_ASSIGN_OUTPUT_NAMES, CICS_CERTIFICATE_OUTPUT_NAMES,
    CICS_EFFECT_PLAN_CONTRACT, CICS_TCPIP_OUTPUT_NAMES, CicsAssignOutput, CicsCertificateOutput,
    CicsCondition, CicsEffectPlan, CicsNamedOperand, CicsOperandName, CicsOperandValue,
    CicsOutputBinding, CicsOutputName, CicsPlanCodecProblem, CicsPlanLimits, CicsPlanOperation,
    CicsPlanOption, CicsStorageSlot, CicsTcpipOutput, decode_cics_effect_plan,
    encode_cics_effect_plan,
};
pub use cobol_config::{
    COBOL_EFFECTIVE_ARITH_OPTION, COBOL_EFFECTIVE_DISPSIGN_OPTION, COBOL_EFFECTIVE_LP_OPTION,
    COBOL_RUNTIME_CONFIG_MAJOR, COBOL_RUNTIME_CONFIG_NAME, COBOL_RUNTIME_CONFIG_NAMESPACE,
    CobolAddressMode, CobolArithmeticMode, CobolDisplaySign, CobolRuntimeConfig,
    CobolRuntimeConfigProblem, cobol_runtime_config,
};
pub use cobol_layout::{
    COBOL_MAX_INDEX_NAMES, COBOL_MAX_TABLE_KEY_BYTES, COBOL_MAX_TABLE_KEYS,
    COBOL_MAX_UNBOUNDED_OCCURRENCES, COBOL_MAX_UNBOUNDED_STORAGE_BYTES, cobol_index_name_is_valid,
    cobol_layout_reference_matches, cobol_source_word_is_undefinable,
    cobol_table_key_category_is_eligible, validate_cobol_condition_values,
    validate_cobol_level78_value,
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
