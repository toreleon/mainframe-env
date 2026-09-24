//! Bounded data flow within the allocated conversation's durable record.
//!
//! Peer arrivals and transmission acknowledgements are explicit inputs from
//! a transport adapter. A staged SEND never becomes a confirmed WAIT merely
//! because a local queue accepted its bytes.

use super::{
    ConversationContext, ConversationKind, ConversationOwner, ConversationProblem,
    ConversationRecord, ConversationState, GdsIssueFlow,
};
use crate::service::{CicsService, mutation_problem, store_error};
use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const MAX_FRAMES: usize = 256;
const MAX_FRAME_BYTES: usize = 32_767;
const MAX_QUEUED_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataCondition {
    Normal,
    LengthError,
    EndOfChain,
    InboundFmh,
    Signal,
}

/// Remote process parameters retained with the initial APPC carrier
/// frame until a WAIT or later data request confirms transmission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationConnectFrame {
    pub process: Vec<u8>,
    pub pip: Vec<u8>,
    pub sync_level: u8,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationDataFrame {
    pub bytes: Vec<u8>,
    pub end_of_chain: bool,
    pub fmh: bool,
    pub signal: bool,
    pub end_structured_field: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invite: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub defresp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_header: Option<super::ConversationAttachHeader>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<ConversationConnectFrame>,
}

impl ConversationDataFrame {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.bytes.len() > MAX_FRAME_BYTES
            || self.signal && !self.bytes.is_empty()
            || self.error_code.is_some() && (self.signal || !self.bytes.is_empty())
            || self.invite && self.end_of_chain
            || self.confirm && self.defresp
            || (self.signal || self.error_code.is_some())
                && (self.invite || self.confirm || self.defresp || self.attach_id.is_some())
            || self.attach_id.as_ref().is_some_and(|name| {
                name.is_empty()
                    || name.len() > 8
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
                    })
            })
            || self.attach_header.as_ref().is_some_and(|header| {
                header.validate().is_err()
                    || self.attach_id.as_deref() != Some(header.name.as_str())
            })
            || self.connect.as_ref().is_some_and(|connect| {
                connect.process.is_empty()
                    || connect.process.len() > super::MAX_PROCESS_BYTES
                    || connect.pip.len() > super::MAX_PIP_BYTES
                    || connect.sync_level > 2
                    || !self.bytes.is_empty()
                    || self.signal
                    || self.error_code.is_some()
                    || self.invite
                    || self.confirm
                    || self.defresp
                    || self.attach_id.is_some()
                    || self.attach_header.is_some()
                    || self.fmh
                    || self.end_of_chain
                    || !self.end_structured_field
            })
        {
            return Err(ConversationProblem::Length);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StagedSend {
    id: u64,
    frame: ConversationDataFrame,
    next_state: ConversationState,
    #[serde(default, skip_serializing_if = "is_false")]
    dispatch_attempted: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationDataState {
    inbound: Vec<ConversationDataFrame>,
    #[serde(default, skip_serializing_if = "is_false")]
    wait_eoc_observed: bool,
    outbound: Vec<StagedSend>,
    signal_pending: bool,
    terminal_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    peer_error_code: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "is_zero")]
    last_peer_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_peer_digest: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "is_zero")]
    next_send_id: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    last_acked_send_id: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl ConversationDataState {
    pub fn is_empty(&self) -> bool {
        self.inbound.is_empty()
            && !self.wait_eoc_observed
            && self.outbound.is_empty()
            && !self.signal_pending
            && !self.terminal_error
            && self.peer_error_code.is_none()
            && self.last_peer_sequence == 0
            && self.next_send_id == 0
            && self.last_acked_send_id == 0
    }

    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.inbound.len() + self.outbound.len() > MAX_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        if self.wait_eoc_observed && self.inbound.is_empty() {
            return Err(ConversationProblem::Malformed);
        }
        let bytes = self
            .inbound
            .iter()
            .map(|frame| frame.bytes.len())
            .chain(self.outbound.iter().map(|send| send.frame.bytes.len()))
            .try_fold(0usize, |total, size| total.checked_add(size))
            .ok_or(ConversationProblem::Exhausted)?;
        if bytes > MAX_QUEUED_BYTES
            || (self.last_peer_sequence == 0) != self.last_peer_digest.is_none()
            || self.last_acked_send_id > self.next_send_id
            || self
                .outbound
                .first()
                .is_some_and(|send| send.id <= self.last_acked_send_id)
            || self
                .outbound
                .last()
                .is_some_and(|send| send.id > self.next_send_id)
            || self
                .outbound
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || self
                .inbound
                .iter()
                .any(|frame| frame.signal || frame.connect.is_some() || frame.validate().is_err())
            || self.outbound.iter().any(|send| {
                send.frame.signal
                    || !send.frame.end_structured_field
                    || !matches!(
                        send.next_state,
                        ConversationState::Send
                            | ConversationState::Receive
                            | ConversationState::PendFree
                    )
                    || send.frame.validate().is_err()
            })
            || self
                .outbound
                .iter()
                .take(self.outbound.len().saturating_sub(1))
                .any(|send| send.next_state != ConversationState::Send)
        {
            return Err(ConversationProblem::Length);
        }
        Ok(())
    }

    pub fn pending_outbound(&self) -> usize {
        self.outbound.len()
    }

    pub fn next_outbound(&self) -> Option<(u64, &ConversationDataFrame, bool)> {
        self.outbound
            .first()
            .map(|send| (send.id, &send.frame, send.dispatch_attempted))
    }

    pub fn pending_inbound(&self) -> usize {
        self.inbound.len()
    }

    pub fn terminal_error(&self) -> bool {
        self.terminal_error
    }
}

impl ConversationRecord {
    /// Apply a partner ABEND or PREPARE in this allocation's durable record.
    /// SIGNAL and ERROR already enter through the peer data frame path;
    /// CONFIRMATION resolves the sender's confirmed transport attempt.
    pub fn accept_peer_issue(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        flow: GdsIssueFlow,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if !matches!(
            self.kind,
            ConversationKind::AppcMapped | ConversationKind::AppcBasic
        ) || self.pending_issue.is_some()
            || self.data.terminal_error
            || self.state == ConversationState::Free
        {
            return Err(ConversationProblem::WrongState);
        }
        match flow {
            GdsIssueFlow::Abend => {
                self.next_sequence()?;
                self.state = ConversationState::Free;
                self.data.peer_error_code = Some([0x08, 0x64, 0, 0]);
                self.data.terminal_error = self.kind == ConversationKind::AppcMapped;
            }
            GdsIssueFlow::Prepare
                if self.sync_level == Some(2) && self.state == ConversationState::Receive =>
            {
                self.next_sequence()?;
                self.state = ConversationState::SyncReceive;
            }
            _ => return Err(ConversationProblem::WrongState),
        }
        Ok(())
    }

    pub fn record_basic_negative_response(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        code: [u8; 4],
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind != ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        self.next_sequence()?;
        self.data.peer_error_code = Some(code);
        Ok(())
    }

    /// WAIT TERMINAL observes peer control without consuming GDS data.
    pub fn observe_basic_wait_terminal(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<Option<DataCondition>, ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind != ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        let condition = if self.data.signal_pending {
            self.data.signal_pending = false;
            Some(DataCondition::Signal)
        } else if self
            .data
            .inbound
            .first()
            .is_some_and(|frame| frame.end_of_chain)
            && !self.data.wait_eoc_observed
        {
            self.data.wait_eoc_observed = true;
            Some(DataCondition::EndOfChain)
        } else {
            None
        };
        if condition.is_some() {
            self.next_sequence()?;
        }
        Ok(condition)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationDataReply {
    pub bytes: Vec<u8>,
    pub returned_length: usize,
    pub original_length: usize,
    pub condition: DataCondition,
    pub end_of_chain: bool,
    pub inbound_fmh: bool,
    /// CDBCOMPL source indicator for one LLID-delimited GDS field.
    pub gds_field_complete: bool,
    pub state: ConversationState,
}

impl ConversationRecord {
    /// Produce the 24-byte DFHCDBLK area from the retained APPC basic state.
    /// Reserved bytes remain zero and no mapped EXEC condition is implied.
    pub fn gds_convdata(&self, field_complete: bool) -> Result<[u8; 24], ConversationProblem> {
        if self.kind != ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        let mut block = [0; 24];
        block[0] = u8::from(field_complete) * 0xff; // CDBCOMPL
        block[1] = u8::from(matches!(
            self.state,
            ConversationState::SyncReceive
                | ConversationState::SyncSend
                | ConversationState::SyncFree
        )) * 0xff; // CDBSYNC
        block[2] = u8::from(matches!(
            self.state,
            ConversationState::Free | ConversationState::ConfFree | ConversationState::SyncFree
        )) * 0xff; // CDBFREE
        block[3] = u8::from(matches!(
            self.state,
            ConversationState::Receive
                | ConversationState::ConfReceive
                | ConversationState::SyncReceive
        )) * 0xff; // CDBRECV
        block[4] = u8::from(self.data.signal_pending) * 0xff; // CDBSIG
        block[5] = u8::from(matches!(
            self.state,
            ConversationState::ConfReceive
                | ConversationState::ConfSend
                | ConversationState::ConfFree
        )) * 0xff; // CDBCONF
        if let Some(code) = self.data.peer_error_code {
            block[6] = 0xff; // CDBERR
            block[7..11].copy_from_slice(&code); // CDBERRCD
        }
        block[11] = u8::from(self.state == ConversationState::Rollback) * 0xff; // CDBSYNRB
        Ok(block)
    }

    /// Preserve one consumed SIGNAL in the GDS RECEIVE result while clearing
    /// the pending bit for subsequent commands.
    pub fn gds_receive_convdata(
        &self,
        reply: &ConversationDataReply,
    ) -> Result<[u8; 24], ConversationProblem> {
        let mut block = self.gds_convdata(reply.gds_field_complete)?;
        if reply.condition == DataCondition::Signal {
            block[4] = 0xff;
        }
        Ok(block)
    }

    /// A partner change-direction indicator may arrive independently of local
    /// SEND INVITE. It is a verified transport event, not a delivery guess.
    pub fn peer_offered_data(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.state != ConversationState::Send || !self.data.outbound.is_empty() {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        self.state = ConversationState::Receive;
        Ok(())
    }

    /// Admit a peer frame after transport validation. Its bytes and indicators
    /// become part of the same durable conversation row as the protocol state.
    pub fn enqueue_peer_data(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        peer_sequence: u64,
        frame: ConversationDataFrame,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        frame.validate()?;
        if frame.connect.is_some()
            || frame.error_code.is_some() && self.kind != ConversationKind::AppcBasic
        {
            return Err(ConversationProblem::WrongKind);
        }
        let digest: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&frame).map_err(|_| ConversationProblem::Malformed)?)
                .into();
        if peer_sequence == self.data.last_peer_sequence
            && self.data.last_peer_digest == Some(digest)
        {
            return Ok(());
        }
        if peer_sequence != self.data.last_peer_sequence.saturating_add(1) {
            return Err(ConversationProblem::WrongState);
        }
        if self.data.terminal_error {
            return Err(ConversationProblem::WrongState);
        }
        let mut next = self.data.clone();
        next.last_peer_sequence = peer_sequence;
        next.last_peer_digest = Some(digest);
        if let Some(code) = frame.error_code {
            next.peer_error_code = Some(code);
        } else if frame.signal {
            next.signal_pending = true;
        } else {
            next.inbound.push(frame);
        }
        next.validate()?;
        self.next_sequence()?;
        self.data = next;
        Ok(())
    }

    /// A session failure is sticky until FREE, as required by TERMERR.
    pub fn mark_terminal_error(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        self.next_sequence()?;
        self.data.terminal_error = true;
        Ok(())
    }

    /// Stage one SEND. INVITE changes direction only after the carrier
    /// acknowledges the staged data; LAST likewise ends the bracket then.
    pub fn stage_send(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        bytes: Vec<u8>,
        invite: bool,
        last: bool,
        fmh: bool,
    ) -> Result<u64, ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind == ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        if self.data.terminal_error
            || self.state != ConversationState::Send
                && !(self.kind == ConversationKind::Mro
                    && self.state == ConversationState::Allocated)
        {
            return Err(ConversationProblem::WrongState);
        }
        if invite && last {
            return Err(ConversationProblem::WrongState);
        }
        if self
            .data
            .outbound
            .last()
            .is_some_and(|send| send.next_state != ConversationState::Send)
        {
            return Err(ConversationProblem::WrongState);
        }
        let frame = ConversationDataFrame {
            bytes,
            end_of_chain: last,
            fmh,
            signal: false,
            end_structured_field: true,
            error_code: None,
            invite,
            confirm: false,
            defresp: false,
            attach_id: None,
            attach_header: None,
            connect: None,
        };
        frame.validate()?;
        let next_state = if last {
            ConversationState::PendFree
        } else if invite {
            ConversationState::Receive
        } else {
            ConversationState::Send
        };
        let mut next = self.data.clone();
        let send_id = next
            .next_send_id
            .checked_add(1)
            .ok_or(ConversationProblem::Exhausted)?;
        next.next_send_id = send_id;
        next.outbound.push(StagedSend {
            id: send_id,
            frame,
            next_state,
            dispatch_attempted: false,
        });
        next.validate()?;
        self.next_sequence()?;
        self.data = next;
        Ok(send_id)
    }

    /// Stage the APPC basic process parameters for GDS WAIT to confirm.
    pub fn stage_basic_connect(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<u64, ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.kind != ConversationKind::AppcBasic {
            return Err(ConversationProblem::WrongKind);
        }
        if self.state != ConversationState::Send || !self.data.outbound.is_empty() {
            return Err(ConversationProblem::WrongState);
        }
        let frame = ConversationDataFrame {
            end_structured_field: true,
            connect: Some(ConversationConnectFrame {
                process: self.process.clone().ok_or(ConversationProblem::Malformed)?,
                pip: self.pip.clone(),
                sync_level: self.sync_level.ok_or(ConversationProblem::Malformed)?,
            }),
            ..Default::default()
        };
        frame.validate()?;
        let mut next = self.data.clone();
        let id = next
            .next_send_id
            .checked_add(1)
            .ok_or(ConversationProblem::Exhausted)?;
        next.next_send_id = id;
        next.outbound.push(StagedSend {
            id,
            frame,
            next_state: ConversationState::Send,
            dispatch_attempted: false,
        });
        next.validate()?;
        self.next_sequence()?;
        self.data = next;
        Ok(id)
    }

    /// Peer-confirmed completion of the oldest staged SEND. The adapter must
    /// persist its external outcome before calling this transition.
    pub fn mark_send_attempted(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        send_id: u64,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        let Some(send) = self.data.outbound.first() else {
            return Err(ConversationProblem::WrongState);
        };
        if send.id != send_id {
            return Err(ConversationProblem::WrongState);
        }
        if send.dispatch_attempted {
            return Ok(());
        }
        self.next_sequence()?;
        self.data.outbound[0].dispatch_attempted = true;
        Ok(())
    }

    /// Only a transport-confirmed or reconciled outcome may remove this
    /// pre-dispatch uncertainty marker. No caller retries the transmission.
    pub fn acknowledge_send(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        send_id: u64,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if send_id != 0 && send_id == self.data.last_acked_send_id {
            return Ok(());
        }
        if self.data.outbound.first().map(|send| send.id) != Some(send_id) {
            return Err(ConversationProblem::WrongState);
        }
        if !self.data.outbound[0].dispatch_attempted {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        let send = self.data.outbound.remove(0);
        self.data.last_acked_send_id = send_id;
        self.state = send.next_state;
        Ok(())
    }

    pub fn wait_transmitted(
        &self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
    ) -> Result<bool, ConversationProblem> {
        self.check_owner(owner, context)?;
        if basic != (self.kind == ConversationKind::AppcBasic) {
            return Err(ConversationProblem::WrongKind);
        }
        if self.data.terminal_error {
            return Err(ConversationProblem::WrongState);
        }
        Ok(self.data.outbound.is_empty())
    }

    pub fn receive_data(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
        basic: bool,
        max_length: usize,
        retain_remainder: bool,
        llid: bool,
    ) -> Result<Option<ConversationDataReply>, ConversationProblem> {
        self.check_owner(owner, context)?;
        if basic != (self.kind == ConversationKind::AppcBasic) {
            return Err(ConversationProblem::WrongKind);
        }
        if self.data.terminal_error || self.state != ConversationState::Receive {
            return Err(ConversationProblem::WrongState);
        }
        if max_length > MAX_FRAME_BYTES {
            return Err(ConversationProblem::Length);
        }
        if self.data.signal_pending {
            self.next_sequence()?;
            self.data.signal_pending = false;
            return Ok(Some(ConversationDataReply {
                bytes: Vec::new(),
                returned_length: 0,
                original_length: 0,
                condition: DataCondition::Signal,
                end_of_chain: false,
                inbound_fmh: false,
                gds_field_complete: false,
                state: self.state,
            }));
        }
        if basic {
            let Some(first) = self.data.inbound.first() else {
                return Ok(None);
            };
            if max_length == 0 {
                return Ok(Some(ConversationDataReply {
                    bytes: Vec::new(),
                    returned_length: 0,
                    original_length: first.bytes.len(),
                    condition: DataCondition::Normal,
                    end_of_chain: false,
                    inbound_fmh: false,
                    gds_field_complete: false,
                    state: self.state,
                }));
            }
            let mut bytes = Vec::new();
            let mut consumed = 0usize;
            let mut partial_take = 0usize;
            let mut end_of_chain = false;
            let mut inbound_fmh = false;
            let mut field_complete = false;
            for frame in &self.data.inbound {
                let room = max_length.saturating_sub(bytes.len());
                let take = room.min(frame.bytes.len());
                bytes.extend_from_slice(&frame.bytes[..take]);
                inbound_fmh |= frame.fmh;
                if take < frame.bytes.len() {
                    partial_take = take;
                    break;
                }
                consumed += 1;
                end_of_chain = frame.end_of_chain;
                field_complete = frame.end_structured_field;
                if llid || frame.end_of_chain || bytes.len() == max_length {
                    break;
                }
            }
            self.next_sequence()?;
            self.data.inbound.drain(..consumed);
            if consumed != 0 {
                self.data.wait_eoc_observed = false;
            }
            if partial_take != 0 {
                self.data.inbound[0].bytes.drain(..partial_take);
            }
            if end_of_chain {
                self.state = ConversationState::Send;
            }
            return Ok(Some(ConversationDataReply {
                returned_length: bytes.len(),
                original_length: bytes.len(),
                bytes,
                condition: DataCondition::Normal,
                end_of_chain,
                inbound_fmh,
                gds_field_complete: llid && field_complete,
                state: self.state,
            }));
        }
        let Some(frame) = self.data.inbound.first().cloned() else {
            return Ok(None);
        };
        let original_length = frame.bytes.len();
        let take = max_length.min(original_length);
        let bytes = frame.bytes[..take].to_vec();
        let truncated = take < original_length;
        self.next_sequence()?;
        if truncated && retain_remainder {
            self.data.inbound[0].bytes.drain(..take);
        } else {
            self.data.inbound.remove(0);
        }
        let condition = if self.kind == ConversationKind::Mro && frame.fmh {
            DataCondition::InboundFmh
        } else if frame.end_of_chain && !truncated {
            DataCondition::EndOfChain
        } else if truncated && !retain_remainder {
            DataCondition::LengthError
        } else {
            DataCondition::Normal
        };
        let consumed_end = frame.end_of_chain && (!truncated || !retain_remainder);
        if consumed_end {
            self.state = ConversationState::Send;
        }
        Ok(Some(ConversationDataReply {
            bytes,
            returned_length: take,
            original_length,
            condition,
            end_of_chain: consumed_end,
            inbound_fmh: frame.fmh,
            gds_field_complete: false,
            state: self.state,
        }))
    }
}

impl CicsService {
    /// Accept one authenticated peer event into the existing conversation
    /// ledger. The caller supplies its transport event sequence and the
    /// allocation owner/lease; a duplicate with different bytes is rejected.
    pub fn accept_conversation_peer_frame(
        &self,
        token: [u8; 4],
        owner: &ConversationOwner,
        context: ConversationContext,
        peer_sequence: u64,
        frame: ConversationDataFrame,
    ) -> Result<(), HostProblem> {
        frame.validate().map_err(|_| HostProblem::Malformed)?;
        for _ in 0..32 {
            let current =
                super::ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            let record = next.conversation_mut(token).ok_or(HostProblem::NotFound)?;
            if record.kind != ConversationKind::AppcBasic {
                return Err(HostProblem::Unsupported);
            }
            record
                .enqueue_peer_data(owner, context, peer_sequence, frame.clone())
                .map_err(|problem| match problem {
                    ConversationProblem::NotOwned
                    | ConversationProblem::StaleOwner
                    | ConversationProblem::DplPrincipal => HostProblem::Unauthorized,
                    ConversationProblem::Length | ConversationProblem::Malformed => {
                        HostProblem::Malformed
                    }
                    ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
                    ConversationProblem::WrongKind | ConversationProblem::WrongState => {
                        HostProblem::IdempotencyConflict
                    }
                })?;
            if next == current {
                return Ok(());
            }
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(|error| mutation_problem(store_error(error)))?
            {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> ConversationOwner {
        ConversationOwner {
            execution: "execution-1".into(),
            run_unit: "unit-1".into(),
            lease_epoch: 7,
        }
    }

    fn frame(bytes: &[u8], end_of_chain: bool) -> ConversationDataFrame {
        ConversationDataFrame {
            bytes: bytes.to_vec(),
            end_of_chain,
            fmh: false,
            signal: false,
            end_structured_field: true,
            error_code: None,
            ..Default::default()
        }
    }

    #[test]
    fn peer_issue_abend_and_prepare_use_shared_state_and_basic_indicators() {
        let owner = owner();
        let mut mapped = ConversationRecord::allocate(
            *b"M001",
            "SYS1",
            ConversationKind::AppcMapped,
            owner.clone(),
            false,
        )
        .unwrap();
        mapped
            .accept_peer_issue(&owner, ConversationContext::Local, GdsIssueFlow::Abend)
            .unwrap();
        assert_eq!(mapped.state, ConversationState::Free);
        assert!(mapped.data.terminal_error());
        assert_eq!(mapped.data.peer_error_code, Some([0x08, 0x64, 0, 0]));
        assert_eq!(
            mapped.accept_peer_issue(&owner, ConversationContext::Local, GdsIssueFlow::Abend),
            Err(ConversationProblem::WrongState)
        );
        mapped
            .release(&owner, ConversationContext::Local, false)
            .unwrap();

        let mut basic = ConversationRecord::allocate(
            *b"B001",
            "SYS1",
            ConversationKind::AppcBasic,
            owner.clone(),
            false,
        )
        .unwrap();
        basic
            .accept_peer_issue(&owner, ConversationContext::Local, GdsIssueFlow::Abend)
            .unwrap();
        let block = basic.gds_convdata(false).unwrap();
        assert_eq!((block[2], block[6]), (0xff, 0xff));
        assert_eq!(&block[7..11], &[0x08, 0x64, 0, 0]);
        basic
            .release(&owner, ConversationContext::Local, true)
            .unwrap();

        let mut prepare = ConversationRecord::allocate(
            *b"B002",
            "SYS1",
            ConversationKind::AppcBasic,
            owner.clone(),
            false,
        )
        .unwrap();
        prepare
            .connect(
                &owner,
                ConversationContext::Local,
                true,
                b"PROC".to_vec(),
                vec![],
                2,
            )
            .unwrap();
        prepare
            .peer_offered_data(&owner, ConversationContext::Local)
            .unwrap();
        prepare
            .accept_peer_issue(&owner, ConversationContext::Local, GdsIssueFlow::Prepare)
            .unwrap();
        assert_eq!(prepare.state, ConversationState::SyncReceive);
        assert_eq!(prepare.gds_convdata(false).unwrap()[1], 0xff);
        let before = prepare.clone();
        assert_eq!(
            prepare.accept_peer_issue(&owner, ConversationContext::Local, GdsIssueFlow::Prepare),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(prepare, before);
    }

    #[test]
    fn mapped_send_wait_and_partial_receive_preserve_ack_and_remainder() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0001",
            "SYS1",
            ConversationKind::AppcMapped,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner,
                ConversationContext::Local,
                false,
                b"PROC".to_vec(),
                vec![],
                0,
            )
            .unwrap();
        record
            .stage_send(
                &owner,
                ConversationContext::Local,
                b"OUT".to_vec(),
                true,
                false,
                false,
            )
            .unwrap();
        assert_eq!(
            record.stage_send(
                &owner,
                ConversationContext::Local,
                b"LATE".to_vec(),
                false,
                false,
                false,
            ),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(
            record.wait_transmitted(&owner, ConversationContext::Local, false),
            Ok(false)
        );
        assert_eq!(record.state, ConversationState::Send);
        let encoded = record.encode().unwrap();
        record = ConversationRecord::decode(&encoded).unwrap();
        assert_eq!(record.data.pending_outbound(), 1);
        assert_eq!(record.data.next_outbound().map(|send| send.2), Some(false));
        assert_eq!(
            record.acknowledge_send(&owner, ConversationContext::Local, 1),
            Err(ConversationProblem::WrongState)
        );
        record
            .mark_send_attempted(&owner, ConversationContext::Local, 1)
            .unwrap();
        let sequence = record.sequence;
        assert_eq!(
            record.mark_send_attempted(&owner, ConversationContext::Local, 1),
            Ok(())
        );
        assert_eq!(record.sequence, sequence);
        let encoded = record.encode().unwrap();
        record = ConversationRecord::decode(&encoded).unwrap();
        assert_eq!(record.data.next_outbound().map(|send| send.2), Some(true));
        record
            .acknowledge_send(&owner, ConversationContext::Local, 1)
            .unwrap();
        assert_eq!(
            record.acknowledge_send(&owner, ConversationContext::Local, 1),
            Ok(())
        );
        assert_eq!(
            record.wait_transmitted(&owner, ConversationContext::Local, false),
            Ok(true)
        );
        assert_eq!(record.state, ConversationState::Receive);
        record
            .enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                1,
                frame(b"ABCDEFGH", true),
            )
            .unwrap();
        assert_eq!(
            record.enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                1,
                frame(b"ABCDEFGH", true)
            ),
            Ok(())
        );
        assert_eq!(record.data.pending_inbound(), 1);
        assert_eq!(
            record.enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                1,
                frame(b"DIFFERENT", true),
            ),
            Err(ConversationProblem::WrongState)
        );
        assert_eq!(
            record.enqueue_peer_data(&owner, ConversationContext::Local, 3, frame(b"GAP", true)),
            Err(ConversationProblem::WrongState)
        );
        let first = record
            .receive_data(&owner, ConversationContext::Local, false, 3, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(first.bytes, b"ABC");
        assert_eq!(first.returned_length, 3);
        assert_eq!(first.original_length, 8);
        assert_eq!(first.condition, DataCondition::Normal);
        assert_eq!(record.data.pending_inbound(), 1);
        let last = record
            .receive_data(&owner, ConversationContext::Local, false, 8, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(last.bytes, b"DEFGH");
        assert_eq!(last.condition, DataCondition::EndOfChain);
        assert_eq!(record.state, ConversationState::Send);
        assert_eq!(record.data.pending_inbound(), 0);
    }

    #[test]
    fn foreign_or_stale_owner_cannot_ack_or_consume() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0002",
            "SYS1",
            ConversationKind::Mro,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .stage_send(
                &owner,
                ConversationContext::Local,
                b"A".to_vec(),
                false,
                false,
                false,
            )
            .unwrap();
        let mut stale = owner.clone();
        stale.lease_epoch = 8;
        assert_eq!(
            record.acknowledge_send(&stale, ConversationContext::Local, 1),
            Err(ConversationProblem::StaleOwner)
        );
        assert_eq!(record.data.pending_outbound(), 1);
        stale.run_unit = "foreign".into();
        assert_eq!(
            record.wait_transmitted(&stale, ConversationContext::Local, false),
            Err(ConversationProblem::NotOwned)
        );
    }

    #[test]
    fn basic_buffer_crosses_fields_but_llid_stops_at_first_field() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0003",
            "SYS1",
            ConversationKind::AppcBasic,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner,
                ConversationContext::Local,
                true,
                b"PROC".to_vec(),
                vec![],
                0,
            )
            .unwrap();
        record
            .peer_offered_data(&owner, ConversationContext::Local)
            .unwrap();
        record
            .enqueue_peer_data(&owner, ConversationContext::Local, 1, frame(b"ABC", false))
            .unwrap();
        record
            .enqueue_peer_data(&owner, ConversationContext::Local, 2, frame(b"DEFG", true))
            .unwrap();
        let first = record
            .receive_data(&owner, ConversationContext::Local, true, 10, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(first.bytes, b"ABC");
        assert_eq!(first.condition, DataCondition::Normal);
        assert!(first.gds_field_complete);
        assert_eq!(
            record.gds_convdata(first.gds_field_complete).unwrap()[0],
            0xff
        );
        assert_eq!(record.data.pending_inbound(), 1);
        let last = record
            .receive_data(&owner, ConversationContext::Local, true, 2, true, false)
            .unwrap()
            .unwrap();
        assert_eq!(last.bytes, b"DE");
        assert_eq!(last.condition, DataCondition::Normal);
        assert_eq!(record.data.pending_inbound(), 1);
        let final_part = record
            .receive_data(&owner, ConversationContext::Local, true, 10, true, false)
            .unwrap()
            .unwrap();
        assert_eq!(final_part.bytes, b"FG");
        assert!(final_part.end_of_chain);
        assert_eq!(record.state, ConversationState::Send);
    }

    #[test]
    fn basic_receive_consumes_signal_and_retains_reply_indicator() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0007",
            "SYS1",
            ConversationKind::AppcBasic,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner,
                ConversationContext::Local,
                true,
                b"PROC".to_vec(),
                vec![],
                0,
            )
            .unwrap();
        record
            .peer_offered_data(&owner, ConversationContext::Local)
            .unwrap();
        let signal = ConversationDataFrame {
            bytes: Vec::new(),
            end_of_chain: false,
            fmh: false,
            signal: true,
            end_structured_field: false,
            error_code: None,
            ..Default::default()
        };
        record
            .enqueue_peer_data(&owner, ConversationContext::Local, 1, signal)
            .unwrap();
        assert_eq!(record.gds_convdata(false).unwrap()[4], 0xff);
        let received = record
            .receive_data(&owner, ConversationContext::Local, true, 16, false, false)
            .unwrap()
            .unwrap();
        assert_eq!(received.condition, DataCondition::Signal);
        assert_eq!(received.returned_length, 0);
        assert_eq!(record.gds_receive_convdata(&received).unwrap()[4], 0xff);
        assert_eq!(record.gds_convdata(false).unwrap()[4], 0);
        assert_eq!(
            record.receive_data(&owner, ConversationContext::Local, true, 16, false, false),
            Ok(None)
        );
    }

    #[test]
    fn basic_convdata_has_pinned_indicators_and_zero_reserved_bytes() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0005",
            "SYS1",
            ConversationKind::AppcBasic,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner,
                ConversationContext::Local,
                true,
                b"PROC".to_vec(),
                vec![],
                0,
            )
            .unwrap();
        record
            .peer_offered_data(&owner, ConversationContext::Local)
            .unwrap();
        record
            .enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                1,
                ConversationDataFrame {
                    bytes: Vec::new(),
                    end_of_chain: false,
                    fmh: false,
                    signal: true,
                    end_structured_field: false,
                    error_code: None,
                    ..Default::default()
                },
            )
            .unwrap();
        record
            .enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                2,
                ConversationDataFrame {
                    bytes: Vec::new(),
                    end_of_chain: false,
                    fmh: false,
                    signal: false,
                    end_structured_field: false,
                    error_code: Some([0x08, 0x89, 0, 0]),
                    ..Default::default()
                },
            )
            .unwrap();
        let data = record.gds_convdata(false).unwrap();
        assert_eq!(data[3], 0xff); // CDBRECV
        assert_eq!(data[4], 0xff); // CDBSIG
        assert_eq!(data[6], 0xff); // CDBERR
        assert_eq!(&data[7..11], &[0x08, 0x89, 0, 0]);
        assert!(data[12..].iter().all(|byte| *byte == 0));
        record.state = ConversationState::Rollback;
        assert_eq!(record.gds_convdata(false).unwrap()[11], 0xff); // CDBSYNRB
    }

    #[test]
    fn mapped_truncation_discards_remainder_and_records_end_of_chain() {
        let owner = owner();
        let mut record = ConversationRecord::allocate(
            *b"0004",
            "SYS1",
            ConversationKind::AppcMapped,
            owner.clone(),
            false,
        )
        .unwrap();
        record
            .connect(
                &owner,
                ConversationContext::Local,
                false,
                b"PROC".to_vec(),
                vec![],
                0,
            )
            .unwrap();
        record
            .peer_offered_data(&owner, ConversationContext::Local)
            .unwrap();
        record
            .enqueue_peer_data(
                &owner,
                ConversationContext::Local,
                1,
                frame(b"ABCDEFG", true),
            )
            .unwrap();
        let reply = record
            .receive_data(&owner, ConversationContext::Local, false, 3, false, false)
            .unwrap()
            .unwrap();
        assert_eq!(reply.bytes, b"ABC");
        assert_eq!(reply.original_length, 7);
        assert_eq!(reply.condition, DataCondition::LengthError);
        assert!(reply.end_of_chain);
        assert_eq!(record.state, ConversationState::Send);
        assert_eq!(record.data.pending_inbound(), 0);
    }
}
