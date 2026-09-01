//! Fail-closed RACF/SAF authority and host-provider adapters.

#![forbid(unsafe_code)]

mod authority;
mod command;
mod command_processor;
mod database;
mod model;

pub use authority::{
    MemorySecretResolver, RacfInstallReceipt, RacfLimits, RacfManifest, RacfProfileDefinition,
    RacfService, RacfUserDefinition, ResolvedSecret, SecretResolver, racf_providers,
};
pub use command::{
    CommandDescriptor, CommandDiagnostic, CommandDiagnosticCode, CommandDomain, CommandFamily,
    CommandLanguageLimits, SuppliedClassDescriptor, ValidatedCommand, command_descriptors,
    recognize_command, supplied_class_descriptors, validate_command,
};
pub use command_processor::{CommandContext, CommandObjectKind, CommandRecord, CommandResult};
pub use database::{SecurityDatabase, SecurityDatabaseSummary};
pub use model::{
    AccessCondition, AccessControlEntry, AccessLevel, Acee, AceeState, AuditFieldValue,
    AuditPolicy, CertificateReference, ClassDescriptor, DatabaseSharingMode, DecisionOutcome,
    DecisionReason, GroupAuthority, GroupConnection, GroupProfile, KeyReference, KeyRing,
    MigrationState, PrincipalKind, PrincipalProfile, PrincipalState, ProfileSegment,
    ProfileTemplate, RacfDatabaseStatus, RacfSubsystemState, RaclistCache, RecoveryRecord,
    RecoveryState, ResourceProfile, SECURITY_DATABASE_SCHEMA, SECURITY_PROFILE_SCHEMA,
    SECURITY_TRANSACTION_SCHEMA, SafDecision, SafStatus, SecurityAuditRecord,
    SecurityDatabaseLimits, SecurityMigration, SecurityPolicyOptions, SecuritySchemaProblem,
    SecurityToken, SecurityTransaction, SegmentFieldKind, SegmentFieldSchema, SegmentTemplate,
    SegmentValue, TokenKind, TokenState, TransactionState,
};
