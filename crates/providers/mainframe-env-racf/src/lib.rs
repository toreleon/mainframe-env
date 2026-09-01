//! Fail-closed RACF/SAF authority and host-provider adapters.

#![forbid(unsafe_code)]

mod authority;
mod database;
mod model;

pub use authority::{
    MemorySecretResolver, RacfInstallReceipt, RacfLimits, RacfManifest, RacfProfileDefinition,
    RacfService, RacfUserDefinition, ResolvedSecret, SecretResolver, racf_providers,
};
pub use database::{SecurityDatabase, SecurityDatabaseSummary};
pub use model::{
    AccessCondition, AccessControlEntry, AccessLevel, Acee, AceeState, AuditFieldValue,
    AuditPolicy, CertificateReference, ClassDescriptor, DecisionOutcome, DecisionReason,
    GroupAuthority, GroupConnection, GroupProfile, KeyReference, KeyRing, MigrationState,
    PrincipalKind, PrincipalProfile, PrincipalState, ProfileSegment, ProfileTemplate,
    RecoveryRecord, RecoveryState, ResourceProfile, SECURITY_DATABASE_SCHEMA,
    SECURITY_PROFILE_SCHEMA, SECURITY_TRANSACTION_SCHEMA, SafDecision, SafStatus,
    SecurityAuditRecord, SecurityDatabaseLimits, SecurityMigration, SecuritySchemaProblem,
    SecurityToken, SecurityTransaction, SegmentFieldKind, SegmentFieldSchema, SegmentTemplate,
    SegmentValue, TokenKind, TokenState, TransactionState,
};
