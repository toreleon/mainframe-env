//! Durable operator-message authority for WRITE OPERATOR.

mod active;
mod authority;
mod write;

pub(super) use active::validate_store as validate_active_operator_commands;
pub(super) use authority::list as load_operator_messages;
pub use write::CICS_OPERATOR_WORK_GENERATION;
pub(in crate::service) use write::invoke;

use crate::service::CicsService;
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::{
    AccessIntent, EffectRequest, HostProblem, HostRequest, HostResult, ResourceName,
    SecurityDecision, SecurityRequest,
};

/// Console-visible identity and content of one durable CICS operator message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsOperatorMessageView {
    /// Deterministic reply identity issued with the message.
    pub key: String,
    /// Exact message bytes supplied by the CICS task.
    pub text: Vec<u8>,
    /// Specifically selected console, if routing codes were not used.
    pub console: Option<String>,
    /// One-byte MVS routing codes, with the default code 2 represented explicitly.
    pub routes: Vec<u8>,
    /// Retained-action descriptor code 2, 3, or 11, if any.
    pub action: Option<u8>,
    /// Whether the issuing task is awaiting an operator reply.
    pub reply_pending: bool,
}

fn view(record: authority::OperatorMessage) -> CicsOperatorMessageView {
    CicsOperatorMessageView {
        key: record.key,
        text: record.text,
        console: record.console,
        routes: record.routes,
        action: record.action,
        reply_pending: record.state == authority::MessageState::Waiting,
    }
}

impl CicsService {
    /// Read all bounded, validated CICS operator messages for a trusted console gateway.
    pub fn operator_messages(&self) -> Result<Vec<CicsOperatorMessageView>, HostProblem> {
        authority::list(self.store.as_ref(), self.limits)
            .map(|records| records.into_iter().map(view).collect())
    }

    /// Read one validated CICS operator message before deciding console access.
    pub fn operator_message(
        &self,
        key: &str,
    ) -> Result<Option<CicsOperatorMessageView>, HostProblem> {
        authority::read(self.store.as_ref(), key).map(|record| record.map(view))
    }

    /// Authorize a console operator and durably post a reply before waking its CICS task.
    ///
    /// The caller must resume the returned run unit after this method succeeds.
    pub fn submit_operator_reply(
        &self,
        invocation: &Invocation,
        console: &str,
        key: &str,
        reply: &[u8],
        now_tick: u64,
    ) -> Result<String, HostProblem> {
        if !(2..=8).contains(&console.len())
            || !console.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'@' | b'#' | b'$')
            })
        {
            return Err(HostProblem::Malformed);
        }
        let record = authority::read(self.store.as_ref(), key)?.ok_or(HostProblem::NotFound)?;
        if record
            .console
            .as_ref()
            .is_some_and(|selected| selected != console)
        {
            return Err(HostProblem::NotFound);
        }
        let resource = format!("CONSOLE.{console}");
        let resource = ResourceName::new(&resource, 246).map_err(|_| HostProblem::Malformed)?;
        let authorization = self.invoke_host(
            invocation,
            now_tick,
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Security(SecurityRequest::Authorize {
                    principal: invocation.principal.id().clone(),
                    class: "FACILITY".into(),
                    resource,
                    intent: AccessIntent::Execute,
                }),
            },
        );
        match authorization.outcome? {
            HostResult::Security(SecurityDecision::Allow) => {}
            HostResult::Security(_) => return Err(HostProblem::Unauthorized),
            _ => return Err(HostProblem::ProviderFailure),
        }
        let answered = authority::answer(self.store.as_ref(), key, reply, now_tick)?;
        Ok(answered.run_unit)
    }
}
