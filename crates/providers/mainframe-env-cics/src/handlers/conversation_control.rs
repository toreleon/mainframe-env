//! Transport-neutral conversation lifecycle shared by CIC-905 command families.
//!
//! A transport adapter may carry frames, but only this contract changes CICS
//! ownership and protocol state. The caller persists each validated transition
//! with the enclosing provider effect and audit record.

use crate::conversation_protocol::ConversationIndicators;
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};
use serde::{Deserialize, Serialize};

mod allocate;
mod build_attach;
mod connect_process;
mod converse;
mod data;
mod definitions;
mod exchange;
mod free;
mod gds;
mod gds_allocate;
mod gds_assign;
mod gds_connect_process;
mod gds_free;
mod gds_issue;
mod gds_receive;
mod gds_wait;
mod issue_staging;
mod issue_transition;
mod ledger;
mod peer;
mod receive;
mod replay;
mod send;
mod state_cvda;
mod transport;
mod wait_convid;
mod wait_signal;
mod wait_terminal;
pub use data::{
    ConversationConnectFrame, ConversationDataFrame, ConversationDataReply, ConversationDataState,
    DataCondition,
};
pub use definitions::{
    ConversationPartnerDefinition, ConversationPartnerProcessDefinition,
    ConversationProfileDefinition,
};
pub use exchange::{
    ConversationExchangeState, ConversationOutboundFrame, ConversationPeerFrame,
    ConversationPendingAttempt, ConversationPendingConverse, MAX_EXCHANGE_FRAME_BYTES,
    MAX_PENDING_PEER_FRAMES, MAX_RECORDED_OUTBOUND_FRAMES,
};
pub use gds::{
    GdsAllocateFailure, GdsAssignFailure, GdsConnectFailure, GdsFreeFailure, GdsReceiveFailure,
    GdsReturnCode, GdsWaitFailure,
};
pub use gds_issue::{GdsIssueFailure, GdsIssueFlow};
pub(in crate::service) use issue_staging::{
    confirm as confirm_issue_control, mark_attempted as mark_issue_control_attempted,
};
pub use issue_transition::{IssuePendingControl, IssueRequestIdentity, IssueValidationProblem};
pub use ledger::{
    CONVERSATION_STATE_NAMESPACE, ConversationAttachHeader, ConversationLedger,
    ConversationSystemDefinition, SignalFacilityRecord, SignalLuType,
};
pub use replay::{
    CONVERSATION_REPLAY_NAMESPACE, ConversationReplay, ConversationReply, load_conversation_replay,
    prune_conversation_replays,
};
pub use transport::{CicsConversationTransport, ConversationTransmitOutcome};

impl CicsService {
    pub(in crate::service) fn conversation_transport(
        &self,
    ) -> Result<Option<std::sync::Arc<dyn CicsConversationTransport>>, HostProblem> {
        Ok(self.lock()?.conversation_transport.clone())
    }

    pub fn install_conversation_transport(
        &self,
        transport: std::sync::Arc<dyn CicsConversationTransport>,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        if let Some(existing) = state.conversation_transport.as_ref() {
            return if std::sync::Arc::ptr_eq(existing, &transport) {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        state.conversation_transport = Some(transport);
        Ok(())
    }
}

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    for _ in 0..32 {
        let current = ConversationLedger::load(service.store.as_ref())
            .map_err(crate::service::store_error)?;
        let mut next = current.clone();
        next.release_task(&owner)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if next == current {
            return Ok(());
        }
        if current
            .persist(&mut next, service.store.as_ref())
            .map_err(|error| crate::service::mutation_problem(crate::service::store_error(error)))?
        {
            return Ok(());
        }
    }
    Err(HostProblem::UnknownOutcome)
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::AllocateConversation => {
            allocate::invoke(service, run, request, retention_tick)
        }
        CicsOperation::GdsAllocateConversation => {
            gds_allocate::invoke(service, run, request, retention_tick)
        }
        CicsOperation::GdsAssignConversation => gds_assign::invoke(service, run, request),
        CicsOperation::WaitSignal => wait_signal::invoke(service, run, request, retention_tick),
        CicsOperation::BuildAttach => build_attach::invoke(service, run, request, retention_tick),
        CicsOperation::ConnectProcess => {
            connect_process::invoke(service, run, request, retention_tick)
        }
        CicsOperation::GdsConnectProcess => {
            gds_connect_process::invoke(service, run, request, retention_tick)
        }
        CicsOperation::FreeConversation => free::invoke(service, run, request, retention_tick),
        CicsOperation::GdsFreeConversation => {
            gds_free::invoke(service, run, request, retention_tick)
        }
        CicsOperation::ReceiveConversation => {
            receive::invoke(service, run, request, retention_tick)
        }
        CicsOperation::GdsReceiveConversation => {
            gds_receive::invoke(service, run, request, retention_tick)
        }
        CicsOperation::SendConversation => send::invoke(service, run, request, retention_tick),
        CicsOperation::GdsWaitConversation => {
            gds_wait::invoke(service, run, request, retention_tick)
        }
        CicsOperation::WaitConvid => wait_convid::invoke(service, run, request, retention_tick),
        CicsOperation::WaitTerminal => wait_terminal::invoke(service, run, request, retention_tick),
        CicsOperation::Converse => converse::invoke(service, run, request, retention_tick),
        CicsOperation::IssueAbend
        | CicsOperation::IssueConfirmation
        | CicsOperation::IssueError
        | CicsOperation::IssuePrepare
        | CicsOperation::IssueSignal => {
            issue_staging::invoke(service, run, request, retention_tick)
        }
        _ => Err(HostProblem::Unsupported),
    }
}

pub(in crate::service) fn context(run: &Run) -> Result<ConversationContext, HostProblem> {
    let Some(value) = run.invocation.bindings.get("cics.execution-context") else {
        return Ok(ConversationContext::Local);
    };
    if value.schema() != "mainframe-env.cics.execution-context@1" {
        return Err(HostProblem::Malformed);
    }
    match value.bytes() {
        b"local" => Ok(ConversationContext::Local),
        b"dpl-synconreturn" | b"dpl-without-synconreturn" | b"dpl-executionset-subset" => {
            Ok(ConversationContext::DplServer)
        }
        _ => Err(HostProblem::Malformed),
    }
}

pub(in crate::service) fn deadline(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if let Some(clock) = &service.replay_clock
        && clock.now_tick()? >= run.invocation.deadline_tick
    {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

/// Durable encoding version for one allocated conversation.
pub const CONVERSATION_RECORD_VERSION: u16 = 2;
/// Maximum length of a partner process name defined by APPC.
pub const MAX_PROCESS_BYTES: usize = 64;
/// Mapped APPC PIP limit; basic GDS has its own 763-byte limit.
pub const MAX_PIP_BYTES: usize = 32_763;
/// APPC basic PIP limit from GDS CONNECT PROCESS.
pub const MAX_BASIC_PIP_BYTES: usize = 763;
const MAX_CONVERSATION_RECORD_BYTES: usize = 384 * 1024;

/// The session protocol selected at allocation, independent of its carrier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationKind {
    AppcMapped,
    AppcBasic,
    Mro,
    /// LU 6.1 facilities remain available to the integrated EXTRACT routes.
    LuType61,
}

/// Source-visible conversation state. Future sibling commands can add guarded
/// transitions without redefining the durable identity or owner.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationState {
    Allocated,
    Send,
    Receive,
    Free,
    PendFree,
    PendReceive,
    ConfFree,
    ConfReceive,
    ConfSend,
    SyncFree,
    SyncReceive,
    SyncSend,
    Rollback,
}

impl ConversationState {
    /// Fullword conversation STATE CVDA from the pinned dfha80c table.
    pub const fn cvda(self) -> i32 {
        match self {
            Self::Allocated => 82,
            Self::ConfFree => 83,
            Self::ConfReceive => 84,
            Self::ConfSend => 85,
            Self::Free => 86,
            Self::PendFree => 87,
            Self::PendReceive => 88,
            Self::Receive => 89,
            Self::Rollback => 90,
            Self::Send => 91,
            Self::SyncFree => 92,
            Self::SyncReceive => 93,
            Self::SyncSend => 94,
        }
    }
}

/// The invocation that owns a conversation. Its lease epoch fences resumed
/// workers and prevents a stale task from reusing a released session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationOwner {
    pub execution: String,
    pub run_unit: String,
    pub lease_epoch: u64,
}

impl ConversationOwner {
    fn valid(&self) -> bool {
        !self.execution.is_empty()
            && self.execution.len() <= 128
            && !self.execution.contains('\0')
            && !self.run_unit.is_empty()
            && self.run_unit.len() <= 128
            && !self.run_unit.contains('\0')
            && self.lease_epoch != 0
    }
}

/// No transport or transaction/program name appears in this authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRecord {
    pub version: u16,
    pub token: [u8; 4],
    pub system: String,
    pub kind: ConversationKind,
    pub owner: ConversationOwner,
    pub principal_facility: bool,
    /// A FREE command has returned this allocation to the session pool.
    pub released: bool,
    pub state: ConversationState,
    pub sync_level: Option<u8>,
    pub process: Option<Vec<u8>>,
    pub pip: Vec<u8>,
    #[serde(default, skip_serializing_if = "ConversationIndicators::is_empty")]
    pub indicators: ConversationIndicators,
    pub sequence: u64,
    /// PROFILE for mapped/MRO or MODENAME for basic APPC. Missing only in
    /// canonical v1 records, whose default is recovered by the v2 reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing_profile: Option<String>,
    /// A control flow staged in this same durable record before peer dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_issue: Option<IssuePendingControl>,
    /// Trusted TCTTE selector for an MRO alternate facility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mro_session_name: Option<String>,
    /// Peer and local data share this allocation's durable owner and CAS lifecycle.
    #[serde(default, skip_serializing_if = "ConversationDataState::is_empty")]
    pub data: ConversationDataState,
}

/// Local/DPL invocation context supplied by the trusted host boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationContext {
    Local,
    DplServer,
}

/// Fail-closed protocol errors; command handlers map these to the source's
/// mapped conditions or GDS six-byte return codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationProblem {
    Malformed,
    NotOwned,
    StaleOwner,
    DplPrincipal,
    WrongKind,
    WrongState,
    Length,
    Exhausted,
}

impl ConversationRecord {
    pub fn allocate(
        token: [u8; 4],
        system: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
        principal_facility: bool,
    ) -> Result<Self, ConversationProblem> {
        let default_profile = if kind == ConversationKind::AppcBasic {
            "DEFAULT"
        } else {
            "DFHCICSA"
        };
        Self::allocate_with_profile(
            token,
            system,
            kind,
            owner,
            principal_facility,
            default_profile,
        )
    }

    pub fn allocate_with_profile(
        token: [u8; 4],
        system: &str,
        kind: ConversationKind,
        owner: ConversationOwner,
        principal_facility: bool,
        processing_profile: &str,
    ) -> Result<Self, ConversationProblem> {
        let record = Self {
            version: CONVERSATION_RECORD_VERSION,
            token,
            system: system.into(),
            kind,
            owner,
            principal_facility,
            released: false,
            state: ConversationState::Allocated,
            sync_level: None,
            process: None,
            pip: Vec::new(),
            indicators: ConversationIndicators::default(),
            sequence: 0,
            processing_profile: Some(processing_profile.into()),
            pending_issue: None,
            mro_session_name: None,
            data: ConversationDataState::default(),
        };
        record.validate()?;
        Ok(record)
    }

    /// Decode an existing provider row. Unknown schema versions, malformed
    /// lengths and impossible state combinations cannot become live sessions.
    pub fn decode(bytes: &[u8]) -> Result<Self, ConversationProblem> {
        if bytes.len() > MAX_CONVERSATION_RECORD_BYTES {
            return Err(ConversationProblem::Length);
        }
        let record: Self =
            serde_json::from_slice(bytes).map_err(|_| ConversationProblem::Malformed)?;
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ConversationProblem> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ConversationProblem::Malformed)?;
        if encoded.len() > MAX_CONVERSATION_RECORD_BYTES {
            return Err(ConversationProblem::Length);
        }
        Ok(encoded)
    }

    pub fn effective_processing_profile(&self) -> &str {
        self.processing_profile
            .as_deref()
            .unwrap_or(if self.kind == ConversationKind::AppcBasic {
                "DEFAULT"
            } else {
                "DFHCICSA"
            })
    }

    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if !matches!(self.version, 1 | CONVERSATION_RECORD_VERSION)
            || self.version == 1 && self.processing_profile.is_some()
            || self.version == 1 && (self.mro_session_name.is_some() || !self.data.is_empty())
            || self.version == CONVERSATION_RECORD_VERSION && self.processing_profile.is_none()
            || self.processing_profile.as_ref().is_some_and(|profile| {
                profile.is_empty()
                    || profile.len() > 8
                    || !profile.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
                    })
            })
            || self.validate_pending_issue().is_err()
            || self.mro_session_name.as_ref().is_some_and(|name| {
                self.kind != ConversationKind::Mro
                    || name.is_empty()
                    || name.len() > 4
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
                    })
            })
            || self.token == [0; 4]
            || self.system.is_empty()
            || self.system.len() > 4
            || !self.system.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
            })
            || !self.owner.valid()
            || self
                .process
                .as_ref()
                .is_some_and(|name| name.is_empty() || name.len() > MAX_PROCESS_BYTES)
            || self.pip.len() > pip_limit(self.kind)
            || self.sync_level.is_some_and(|level| level > 2)
            || (self.released && self.state != ConversationState::Free)
            || (self.released && !self.data.is_empty())
            || (self.process.is_none() && self.sync_level.is_some())
            || (!matches!(
                self.kind,
                ConversationKind::Mro | ConversationKind::LuType61
            ) && self.state != ConversationState::Allocated
                && self.process.is_none()
                && self.state != ConversationState::Free)
            || (self.kind == ConversationKind::Mro
                && matches!(
                    self.state,
                    ConversationState::ConfFree
                        | ConversationState::ConfReceive
                        | ConversationState::ConfSend
                        | ConversationState::PendReceive
                ))
        {
            return Err(ConversationProblem::Malformed);
        }
        validate_pip(&self.pip, self.kind)?;
        self.data.validate()?;
        Ok(())
    }

    /// Check this task's authority before any operation or state mutation.
    /// The DPL server's function-shipping principal facility is restricted
    /// even though other task-owned alternate facilities may be usable.
    pub fn check_owner(
        &self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        if self.owner.execution != owner.execution || self.owner.run_unit != owner.run_unit {
            return Err(ConversationProblem::NotOwned);
        }
        if self.owner.lease_epoch != owner.lease_epoch {
            return Err(ConversationProblem::StaleOwner);
        }
        if self.principal_facility && context == ConversationContext::DplServer {
            return Err(ConversationProblem::DplPrincipal);
        }
        if self.released {
            return Err(ConversationProblem::WrongState);
        }
        Ok(())
    }

    /// CONNECT PROCESS / GDS CONNECT PROCESS move only an allocated APPC
    /// conversation into SEND. The complete PIP record boundaries are checked
    /// before any durable write or transport dispatch.
    pub fn connect(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        process: Vec<u8>,
        pip: Vec<u8>,
        sync_level: u8,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.pending_issue.is_some() {
            return Err(ConversationProblem::WrongState);
        }
        let expected = if basic {
            ConversationKind::AppcBasic
        } else {
            ConversationKind::AppcMapped
        };
        if self.kind != expected {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Allocated {
            return Err(ConversationProblem::WrongState);
        }
        if process.is_empty() || process.len() > MAX_PROCESS_BYTES || sync_level > 2 {
            return Err(ConversationProblem::Length);
        }
        validate_pip(&pip, self.kind)?;
        self.next_sequence()?;
        self.process = Some(process);
        self.pip = pip;
        self.sync_level = Some(sync_level);
        self.state = ConversationState::Send;
        Ok(())
    }

    /// Record the result of an actual mapped APPC or MRO exchange. A transport
    /// result is an input, never inferred from a successful broker delivery.
    pub fn complete_converse(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        next: ConversationState,
    ) -> Result<(), ConversationProblem> {
        self.check_converse_start(owner, context)?;
        if !matches!(
            next,
            ConversationState::Send
                | ConversationState::Receive
                | ConversationState::Free
                | ConversationState::PendFree
                | ConversationState::PendReceive
                | ConversationState::ConfReceive
                | ConversationState::SyncReceive
        ) || self.kind == ConversationKind::Mro
            && matches!(
                next,
                ConversationState::PendReceive | ConversationState::ConfReceive
            )
        {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = next;
        Ok(())
    }

    pub fn check_converse_start(
        &self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.pending_issue.is_some() {
            return Err(ConversationProblem::WrongState);
        }
        if matches!(
            self.kind,
            ConversationKind::AppcBasic | ConversationKind::LuType61
        ) {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Send
            && !(self.kind == ConversationKind::Mro && self.state == ConversationState::Allocated)
        {
            return Err(ConversationProblem::WrongState);
        }
        Ok(())
    }

    /// Apply a peer protocol completion, independently of message delivery.
    /// The basic conversation's GDS FREE becomes legal only after this event.
    pub fn peer_finished(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.pending_issue.is_some() {
            return Err(ConversationProblem::WrongState);
        }
        if self.kind != ConversationKind::AppcBasic || self.state != ConversationState::Send {
            return Err(ConversationProblem::WrongState);
        }
        if let Some((send_id, frame, _)) = self.data.next_outbound() {
            if frame.connect.is_none() || self.data.pending_outbound() != 1 {
                return Err(ConversationProblem::WrongState);
            }
            self.mark_send_attempted(owner, context, send_id)?;
            self.acknowledge_send(owner, context, send_id)?;
        }
        self.next_sequence()?;
        self.state = ConversationState::Free;
        self.indicators.free_required = true;
        Ok(())
    }

    /// FREE and GDS FREE release this task's allocation. APPC basic requires
    /// the source-defined FREE state; releasing it earlier is a state check.
    pub fn release(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.pending_issue.is_some() {
            return Err(ConversationProblem::WrongState);
        }
        if basic != (self.kind == ConversationKind::AppcBasic) {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Free
            && (basic
                || matches!(
                    self.state,
                    ConversationState::PendFree | ConversationState::Rollback
                ))
        {
            return Err(ConversationProblem::WrongState);
        }
        if self.data.pending_outbound() != 0 {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = ConversationState::Free;
        self.released = true;
        self.data = ConversationDataState::default();
        Ok(())
    }

    fn next_sequence(&mut self) -> Result<(), ConversationProblem> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ConversationProblem::Exhausted)?;
        Ok(())
    }
}

fn pip_limit(kind: ConversationKind) -> usize {
    if kind == ConversationKind::AppcBasic {
        MAX_BASIC_PIP_BYTES
    } else {
        MAX_PIP_BYTES
    }
}

fn validate_pip(pip: &[u8], kind: ConversationKind) -> Result<(), ConversationProblem> {
    if pip.is_empty() {
        return Ok(());
    }
    if !(4..=pip_limit(kind)).contains(&pip.len()) {
        return Err(ConversationProblem::Length);
    }
    let mut cursor = 0usize;
    while cursor < pip.len() {
        if pip.len() - cursor < 4 {
            return Err(ConversationProblem::Length);
        }
        let length = usize::from(u16::from_be_bytes([pip[cursor], pip[cursor + 1]]));
        if length < 4 || length > pip.len() - cursor || pip[cursor + 2..cursor + 4] != [0, 0] {
            return Err(ConversationProblem::Length);
        }
        cursor += length;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(epoch: u64) -> ConversationOwner {
        ConversationOwner {
            execution: "execution".into(),
            run_unit: "run".into(),
            lease_epoch: epoch,
        }
    }

    #[test]
    fn mapped_lifecycle_and_restart_preserve_owner_and_state() {
        let mut record = ConversationRecord::allocate(
            *b"C001",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner(3),
                ConversationContext::Local,
                false,
                b"TRAN".to_vec(),
                vec![0, 4, 0, 0],
                2,
            )
            .unwrap();
        let bytes = record.encode().unwrap();
        let mut reopened = ConversationRecord::decode(&bytes).unwrap();
        assert_eq!(reopened.sequence, 1);
        assert_eq!(
            reopened.check_owner(&owner(2), ConversationContext::Local),
            Err(ConversationProblem::StaleOwner)
        );
        assert_eq!(
            reopened.complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Allocated
            ),
            Err(ConversationProblem::WrongState)
        );
        reopened
            .complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Receive,
            )
            .unwrap();
        reopened
            .release(&owner(3), ConversationContext::Local, false)
            .unwrap();
        assert_eq!(reopened.state, ConversationState::Free);
        assert!(reopened.released);
        assert_eq!(reopened.sequence, 3);
        assert_eq!(
            reopened.release(&owner(3), ConversationContext::Local, false),
            Err(ConversationProblem::WrongState)
        );
    }

    #[test]
    fn dpl_principal_and_wrong_owner_cannot_mutate() {
        let mut record = ConversationRecord::allocate(
            *b"C002",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            true,
        )
        .unwrap();
        let before = record.clone();
        assert_eq!(
            record.connect(
                &owner(3),
                ConversationContext::DplServer,
                false,
                b"TRAN".to_vec(),
                vec![],
                0
            ),
            Err(ConversationProblem::DplPrincipal)
        );
        let alien = ConversationOwner {
            execution: "other".into(),
            ..owner(3)
        };
        assert_eq!(
            record.release(&alien, ConversationContext::Local, false),
            Err(ConversationProblem::NotOwned)
        );
        assert_eq!(record, before);
    }

    #[test]
    fn basic_release_requires_free_and_connect_validates_pip() {
        let mut record = ConversationRecord::allocate(
            *b"C003",
            "SYS1",
            ConversationKind::AppcBasic,
            owner(3),
            false,
        )
        .unwrap();
        assert_eq!(
            record.release(&owner(3), ConversationContext::Local, true),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(
            record.connect(
                &owner(3),
                ConversationContext::Local,
                true,
                b"TRAN".to_vec(),
                vec![0, 3, 0, 0],
                0
            ),
            Err(ConversationProblem::Length)
        );
        assert_eq!(record.state, ConversationState::Allocated);
        record
            .connect(
                &owner(3),
                ConversationContext::Local,
                true,
                b"TRAN".to_vec(),
                vec![0, 4, 0, 0],
                0,
            )
            .unwrap();
        assert_eq!(
            record.complete_converse(
                &owner(3),
                ConversationContext::Local,
                ConversationState::Free
            ),
            Err(ConversationProblem::WrongKind)
        );
        record
            .peer_finished(&owner(3), ConversationContext::Local)
            .unwrap();
        record
            .release(&owner(3), ConversationContext::Local, true)
            .unwrap();
    }

    #[test]
    fn mapped_and_basic_pip_limits_follow_distinct_source_bounds() {
        let mut pip = vec![0; 764];
        pip[..2].copy_from_slice(&764u16.to_be_bytes());
        let mut mapped = ConversationRecord::allocate(
            *b"C007",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            false,
        )
        .unwrap();
        mapped
            .connect(
                &owner(3),
                ConversationContext::Local,
                false,
                b"TRAN".to_vec(),
                pip.clone(),
                0,
            )
            .unwrap();
        assert!(mapped.validate().is_ok());
        let mut basic = ConversationRecord::allocate(
            *b"C008",
            "SYS1",
            ConversationKind::AppcBasic,
            owner(3),
            false,
        )
        .unwrap();
        assert_eq!(
            basic.connect(
                &owner(3),
                ConversationContext::Local,
                true,
                b"TRAN".to_vec(),
                pip,
                0,
            ),
            Err(ConversationProblem::Length)
        );
        assert_eq!(basic.state, ConversationState::Allocated);
    }

    #[test]
    fn corrupt_rows_and_invalid_owners_fail_closed() {
        let record =
            ConversationRecord::allocate(*b"C004", "SYS1", ConversationKind::Mro, owner(3), false)
                .unwrap();
        let mut encoded = record.encode().unwrap();
        encoded.extend_from_slice(b"{}{}");
        assert_eq!(
            ConversationRecord::decode(&encoded),
            Err(ConversationProblem::Malformed)
        );
        let mut corrupt = record;
        corrupt.version = 3;
        assert_eq!(corrupt.encode(), Err(ConversationProblem::Malformed));
        let mut unknown: serde_json::Value = serde_json::from_slice(
            &ConversationRecord::allocate(*b"C006", "SYS1", ConversationKind::Mro, owner(3), false)
                .unwrap()
                .encode()
                .unwrap(),
        )
        .unwrap();
        unknown["unreviewed_field"] = serde_json::Value::Bool(true);
        assert_eq!(
            ConversationRecord::decode(&serde_json::to_vec(&unknown).unwrap()),
            Err(ConversationProblem::Malformed)
        );
        assert_eq!(
            ConversationRecord::allocate(*b"C005", "SYS1", ConversationKind::Mro, owner(0), false),
            Err(ConversationProblem::Malformed)
        );
    }

    #[test]
    fn v1_record_reads_without_profile_and_v2_records_preserve_selected_profile() {
        let selected = ConversationRecord::allocate_with_profile(
            *b"C007",
            "SYS1",
            ConversationKind::AppcMapped,
            owner(3),
            false,
            "FAST",
        )
        .unwrap();
        assert_eq!(selected.version, 2);
        assert_eq!(selected.processing_profile.as_deref(), Some("FAST"));
        assert_eq!(
            ConversationRecord::decode(&selected.encode().unwrap()),
            Ok(selected.clone())
        );

        let mut legacy = selected;
        legacy.version = 1;
        legacy.processing_profile = None;
        let bytes = legacy.encode().unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("processing_profile"));
        assert_eq!(legacy.effective_processing_profile(), "DFHCICSA");
        assert_eq!(ConversationRecord::decode(&bytes), Ok(legacy));
        let mut impossible =
            ConversationRecord::allocate(*b"C008", "SYS1", ConversationKind::Mro, owner(3), false)
                .unwrap();
        impossible.version = 1;
        impossible.processing_profile = None;
        impossible.mro_session_name = Some("S001".into());
        assert_eq!(impossible.validate(), Err(ConversationProblem::Malformed));
    }
}
