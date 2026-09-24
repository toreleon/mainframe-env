//! Typed, bounded CICS runtime and protocol-neutral session authority.

#![forbid(unsafe_code)]

mod abi;
mod conversation_protocol;
mod event_wait;
mod generated;
mod retention;
mod service;

pub use abi::cics_abi_library;
pub use conversation_protocol::{
    CONVERSATION_RECORD_VERSION, CONVERSATION_STATE_NAMESPACE, ConversationAttachHeader,
    ConversationContext, ConversationIndicators, ConversationKind, ConversationLedger,
    ConversationOwner, ConversationProblem, ConversationRecord, ConversationState,
    ConversationSystemDefinition, GdsAllocateFailure, GdsAssignFailure, GdsConnectFailure,
    GdsExtractAttributesFailure, GdsExtractProcessFailure, GdsFreeFailure, GdsReturnCode,
    MAX_BASIC_PIP_BYTES, MAX_PIP_BYTES, MAX_PROCESS_BYTES,
};
pub use event_wait::{CicsEventPostMode, CicsEventPurgeMode};
pub use service::bts_lifecycle;

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, CICS_RETENTION_NAMESPACES,
    CicsReplayCodecVersion, CicsReplayRetentionState, CicsReplayRowDescriptor,
    CicsReplayValidationError, CicsUndoRowDescriptor, CicsUowCodecVersion, CicsUowDependencyState,
    CicsUowRowDescriptor, CicsUowState, CicsUowValidationError, describe_cics_replay_row,
    describe_cics_undo_row, describe_cics_uow_row,
};
pub use service::{
    BmsFieldDefinition, BmsMapDefinition, CICS_DELAY_WORK_GENERATION, CICS_START_WORK_GENERATION,
    CicsApplicationEntryDefinition, CicsBmsControlSnapshot, CicsBtsChildCompletion,
    CicsBtsLinkContext, CicsContinuation, CicsDiagnosticDumpRecord, CicsDiagnosticSnapshot,
    CicsDiagnosticTraceRecord, CicsDocumentTemplateDefinition, CicsDumpCodeDefinition,
    CicsEnqueueModelDefinition, CicsFileDefinition, CicsFileStatus, CicsIntervalError,
    CicsIntervalMode, CicsIntervalTime, CicsJavaStatus, CicsLimits, CicsMonitorAction,
    CicsMonitorPointDefinition, CicsOutboardDestinationDefinition, CicsOutboardKind,
    CicsOutboardRecord, CicsOutboardSnapshot, CicsPartitionDefinition, CicsPartitionSetDefinition,
    CicsProgramDefinition, CicsReplayClock, CicsService, CicsSignalCaptureSpec, CicsSignalEmission,
    CicsSpoolReportSnapshot, CicsStartTask, CicsStartTerminal, CicsTerminalExecution,
    CicsTerminalSnapshot, CicsTraceConfiguration, CicsTraceEntry, CicsTransformContainerMode,
    CicsTransformDefinition, CicsTransformFieldDefinition, CicsTransformFieldKind,
    CicsTransformFormat, CicsTransientDataQueueDefinition, CicsTransientDataQueueKind,
    CicsTransientDataQueueOpen, CicsWebEndpoint, CicsWebInboundRequest, CicsWebRequest,
    CicsWebResponse, CicsWebServerResponse, CicsWebServiceDefinition, CicsWebTransport,
    CicsWebUriMapDefinition, CicsWebVersion, CicsXmlTransformMetadata, cics_provider,
};
pub use service::{
    BrxaBindFrame, BrxaBindReply, BrxaEndFrame, BrxaInitFrame, BrxaInitReply,
    CICS_BRIDGE_START_WORK_GENERATION, CICS_OPERATOR_WORK_GENERATION, CICS_POST_WORK_GENERATION,
    CicsBridgeAbiProfile, CicsBridgeAbiSelection, CicsBridgeExitDefault, CicsBridgeExitSelection,
    CicsBridgeRuntime, CicsBridgeStartIntent, CicsCertificateName, CicsClientCertificate,
    CicsCredentialChangeRequest, CicsCredentialDetails, CicsCredentialFailure, CicsCredentialKind,
    CicsCredentialRequest, CicsCredentialVerification, CicsOperatorMessageView,
    CicsPassTicketFailure, CicsPassTicketOutcome, CicsPassTicketRequest, CicsSecurityAccess,
    CicsSecurityAccessReason, CicsSecurityAuthority, CicsSecurityTokenKind, CicsTcpipAuthenticate,
    CicsTcpipContext, CicsTcpipPrivacy, CicsTcpipSslType, CicsTokenFailure, CicsTokenVerification,
    CicsTokenVerificationRequest,
};

#[cfg(feature = "fault-injection")]
pub use service::CicsFileFaultPoint;
