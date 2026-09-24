//! Bounded data flow within the allocated conversation's durable record.
//!
//! Peer arrivals and transmission acknowledgements are explicit inputs from
//! a transport adapter. A staged SEND never becomes a confirmed WAIT merely
//! because a local queue accepted its bytes.

use super::{
    ConversationContext, ConversationKind, ConversationOwner, ConversationProblem,
    ConversationRecord, ConversationState,
};
use serde::{Deserialize, Serialize};

const MAX_FRAMES: usize = 256;
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationDataFrame {
    pub bytes: Vec<u8>,
    pub end_of_chain: bool,
    pub fmh: bool,
    pub signal: bool,
    pub end_structured_field: bool,
}

impl ConversationDataFrame {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.bytes.len() > MAX_FRAME_BYTES || self.signal && !self.bytes.is_empty() {
            return Err(ConversationProblem::Length);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StagedSend {
    frame: ConversationDataFrame,
    next_state: ConversationState,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationDataState {
    inbound: Vec<ConversationDataFrame>,
    outbound: Vec<StagedSend>,
    signal_pending: bool,
    terminal_error: bool,
}

impl ConversationDataState {
    pub fn is_empty(&self) -> bool {
        self.inbound.is_empty()
            && self.outbound.is_empty()
            && !self.signal_pending
            && !self.terminal_error
    }

    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.inbound.len() + self.outbound.len() > MAX_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        let bytes = self
            .inbound
            .iter()
            .map(|frame| frame.bytes.len())
            .chain(self.outbound.iter().map(|send| send.frame.bytes.len()))
            .try_fold(0usize, |total, size| total.checked_add(size))
            .ok_or(ConversationProblem::Exhausted)?;
        if bytes > MAX_QUEUED_BYTES
            || self
                .inbound
                .iter()
                .any(|frame| frame.signal || frame.validate().is_err())
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

    pub fn pending_inbound(&self) -> usize {
        self.inbound.len()
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
    pub state: ConversationState,
}

impl ConversationRecord {
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
        frame: ConversationDataFrame,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        frame.validate()?;
        if self.data.terminal_error {
            return Err(ConversationProblem::WrongState);
        }
        let mut next = self.data.clone();
        if frame.signal {
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
    ) -> Result<(), ConversationProblem> {
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
        next.outbound.push(StagedSend { frame, next_state });
        next.validate()?;
        self.next_sequence()?;
        self.data = next;
        Ok(())
    }

    /// Peer-confirmed completion of the oldest staged SEND. The adapter must
    /// persist its external outcome before calling this transition.
    pub fn acknowledge_send(
        &mut self,
        owner: &ConversationOwner,
        context: ConversationContext,
    ) -> Result<(), ConversationProblem> {
        self.check_owner(owner, context)?;
        if self.data.outbound.is_empty() {
            return Err(ConversationProblem::WrongState);
        }
        self.next_sequence()?;
        let send = self.data.outbound.remove(0);
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
        if self.data.signal_pending && !basic {
            self.next_sequence()?;
            self.data.signal_pending = false;
            return Ok(Some(ConversationDataReply {
                bytes: Vec::new(),
                returned_length: 0,
                original_length: 0,
                condition: DataCondition::Signal,
                end_of_chain: false,
                inbound_fmh: false,
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
                    state: self.state,
                }));
            }
            let mut bytes = Vec::new();
            let mut consumed = 0usize;
            let mut partial_take = 0usize;
            let mut end_of_chain = false;
            let mut inbound_fmh = false;
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
                if llid || frame.end_of_chain || bytes.len() == max_length {
                    break;
                }
            }
            self.next_sequence()?;
            self.data.inbound.drain(..consumed);
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
            state: self.state,
        }))
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
        }
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
        record
            .acknowledge_send(&owner, ConversationContext::Local)
            .unwrap();
        assert_eq!(
            record.wait_transmitted(&owner, ConversationContext::Local, false),
            Ok(true)
        );
        assert_eq!(record.state, ConversationState::Receive);
        record
            .enqueue_peer_data(&owner, ConversationContext::Local, frame(b"ABCDEFGH", true))
            .unwrap();
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
            record.acknowledge_send(&stale, ConversationContext::Local),
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
            .enqueue_peer_data(&owner, ConversationContext::Local, frame(b"ABC", false))
            .unwrap();
        record
            .enqueue_peer_data(&owner, ConversationContext::Local, frame(b"DEFG", true))
            .unwrap();
        let first = record
            .receive_data(&owner, ConversationContext::Local, true, 10, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(first.bytes, b"ABC");
        assert_eq!(first.condition, DataCondition::Normal);
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
            .enqueue_peer_data(&owner, ConversationContext::Local, frame(b"ABCDEFG", true))
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
