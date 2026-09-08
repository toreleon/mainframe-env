//! Fail-closed RACF/SAF authority and host-provider adapters.

#![forbid(unsafe_code)]

mod audit;
mod authority;
mod command;
mod command_processor;
mod database;
mod model;
mod saf;

pub use audit::SmfType80Record;
pub use authority::{
    EphemeralSecretScope, MemorySecretResolver, MemorySecretResolverLimits,
    PrincipalAuthenticationEpoch, RacfInstallReceipt, RacfLimits, RacfManifest,
    RacfProfileDefinition, RacfService, RacfUserDefinition, ResolvedSecret, SecretResolver,
    racf_providers,
};
pub use command::{
    CommandDescriptor, CommandDiagnostic, CommandDiagnosticCode, CommandDomain, CommandFamily,
    CommandLanguageLimits, FlatValueRoleDescriptor, RacrouteDescriptor, RacrouteRequestType,
    SuppliedClassDescriptor, SyntaxTokenBehavior, SyntaxTokenDescriptor, ValidatedCommand,
    command_descriptors, racroute_descriptors, recognize_command, supplied_class_descriptors,
    validate_command,
};
pub use command_processor::{CommandContext, CommandObjectKind, CommandRecord, CommandResult};
pub use database::{SecurityDatabase, SecurityDatabaseSummary, SecuritySemanticProjection};
pub use model::{
    AccessCondition, AccessControlEntry, AccessLevel, Acee, AceeState, AssociationState,
    AuditFieldValue, AuditPolicy, CertificateReference, ClassDescriptor, DatabaseSharingMode,
    DecisionOutcome, DecisionReason, GroupAuthority, GroupConnection, GroupProfile,
    IdentityMapping, KeyReference, KeyRing, MfaFactor, MfaFactorKind, MigrationState,
    PrincipalKind, PrincipalProfile, PrincipalState, ProfileSegment, ProfileTemplate,
    RacfDatabaseStatus, RacfSubsystemState, RaclistCache, RecoveryRecord, RecoveryState,
    ResourceProfile, RrsfNode, RrsfNodeState, SECURITY_DATABASE_SCHEMA, SECURITY_PROFILE_SCHEMA,
    SECURITY_TRANSACTION_SCHEMA, SafDecision, SafStatus, SecurityAuditRecord,
    SecurityDatabaseLimits, SecurityMigration, SecurityPolicyOptions, SecurityRequestDigestFormat,
    SecuritySchemaProblem, SecurityToken, SecurityTransaction, SegmentFieldKind,
    SegmentFieldSchema, SegmentTemplate, SegmentValue, SignonSession, SignonSessionState,
    TokenKind, TokenState, TransactionState, UserAssociation,
};
pub use saf::{
    AccessEnvironment, AceeSummary, ExtractedSecurityRecord, RacrouteOutcome, RacrouteRequest,
    RacrouteResult, RacrouteState, SafDefineAction, SafExtractKind, SafRequestContext,
    SafVerifyAction, TokenMetadata,
};
