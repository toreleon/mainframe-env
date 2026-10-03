//! Bounded IMS transaction-management contracts and runtime.

mod codec;
mod contracts;
mod model;
mod package;
mod service;
mod support;

pub use contracts::{
    TmAlternatePcbDefinition, TmCall, TmConversationAction, TmDefinitionSet, TmDestination,
    TmExecutionContext, TmInputMessage, TmLimits, TmPcb, TmPcbStatus, TmTransactionDefinition,
};
pub use model::{
    TmCallResult, TmCancelReceipt, TmConversationView, TmEnqueueReceipt, TmInstallReceipt,
    TmMessageState, TmOutboundMessage, TmPcbView, TmScheduleReceipt,
};
pub use package::TmPackageBinding;
pub use service::TmService;

/// Recovery must not settle a TM-backed run through the DB-only adapter.
pub(crate) fn has_session(
    store: &dyn mainframe_env_store_api::ProviderStateStore,
    run: &str,
    max: usize,
) -> Result<bool, mainframe_env_host_api::HostProblem> {
    codec::read::<model::SessionRow>(store, codec::SESSION_NAMESPACE, run, max)
        .map(|row| row.is_some())
}
