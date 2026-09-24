//! Explicit APPC/MRO peer frames and outbound exchange records.

use super::{
    ConversationDataFrame, ConversationDataReply, ConversationKind, ConversationProblem,
    ConversationState, DataCondition,
};
use serde::{Deserialize, Serialize};

pub const MAX_EXCHANGE_FRAME_BYTES: usize = 1_048_576;
pub const MAX_PENDING_PEER_FRAMES: usize = 32;
pub const MAX_RECORDED_OUTBOUND_FRAMES: usize = 32;

/// One source-visible peer result. It is supplied by a trusted conversation
/// adapter, never inferred from broker delivery or a successful local send.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationPeerFrame {
    pub data: Vec<u8>,
    pub next_state: ConversationState,
    pub end_of_chain: bool,
    pub inbound_fmh: bool,
    pub signal: bool,
}

impl ConversationPeerFrame {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.data.len() > MAX_EXCHANGE_FRAME_BYTES
            || self.signal && (!self.data.is_empty() || self.inbound_fmh || self.end_of_chain)
            || !matches!(
                self.next_state,
                ConversationState::Send
                    | ConversationState::Receive
                    | ConversationState::Free
                    | ConversationState::PendFree
                    | ConversationState::ConfReceive
                    | ConversationState::SyncReceive
            )
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedPeerMetadata {
    pub next_state: ConversationState,
    pub end_of_chain: bool,
    pub inbound_fmh: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StagedExchangeSend {
    pub id: u64,
    pub frame: ConversationDataFrame,
    pub next_state: ConversationState,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dispatch_attempted: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// The exact application data and optional structured attach header selected
/// by one CONVERSE. This is evidence of a protocol exchange, not an MQ ack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationOutboundFrame {
    pub data: Vec<u8>,
    pub attach_id: Option<String>,
    pub fmh: bool,
    pub definite_response: bool,
}

impl ConversationOutboundFrame {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.data.len() > MAX_EXCHANGE_FRAME_BYTES
            || self.attach_id.as_ref().is_some_and(|name| {
                name.is_empty()
                    || name.len() > 8
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
                    })
            })
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExchangeState {
    pub inbound: Vec<ConversationPeerFrame>,
    pub outbound: Vec<ConversationOutboundFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_sends: Vec<StagedExchangeSend>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_send_id: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub last_acked_send_id: u64,
    /// The remainder retained by NOTRUNCATE for a later RECEIVE sibling.
    pub retained: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_meta: Option<RetainedPeerMetadata>,
}

impl ConversationExchangeState {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.inbound.len() > MAX_PENDING_PEER_FRAMES
            || self.outbound.len() > MAX_RECORDED_OUTBOUND_FRAMES
            || self.pending_sends.len() > MAX_RECORDED_OUTBOUND_FRAMES
            || self.retained.len() > MAX_EXCHANGE_FRAME_BYTES
            || self.retained.is_empty() && self.retained_meta.is_some()
            || self.last_acked_send_id > self.next_send_id
            || self
                .pending_sends
                .first()
                .is_some_and(|send| send.id <= self.last_acked_send_id)
            || self
                .pending_sends
                .last()
                .is_some_and(|send| send.id > self.next_send_id)
            || self
                .pending_sends
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(ConversationProblem::Malformed);
        }
        for frame in &self.inbound {
            frame.validate()?;
        }
        for frame in &self.outbound {
            frame.validate()?;
        }
        for send in &self.pending_sends {
            send.frame.validate()?;
            if send.frame.signal
                || send.frame.error_code.is_some()
                || !send.frame.end_structured_field
                || !matches!(
                    send.next_state,
                    ConversationState::Send
                        | ConversationState::Receive
                        | ConversationState::PendFree
                )
            {
                return Err(ConversationProblem::Malformed);
            }
        }
        if self
            .pending_sends
            .iter()
            .take(self.pending_sends.len().saturating_sub(1))
            .any(|send| send.next_state != ConversationState::Send)
        {
            return Err(ConversationProblem::WrongState);
        }
        Ok(())
    }

    pub fn pending_outbound(&self) -> usize {
        self.pending_sends.len()
    }

    pub fn next_outbound(&self) -> Option<(u64, &ConversationDataFrame, bool)> {
        self.pending_sends
            .first()
            .map(|send| (send.id, &send.frame, send.dispatch_attempted))
    }

    pub fn stage_send(
        &mut self,
        frame: ConversationDataFrame,
        next_state: ConversationState,
    ) -> Result<u64, ConversationProblem> {
        frame.validate()?;
        if frame.signal
            || frame.error_code.is_some()
            || !frame.end_structured_field
            || self.pending_sends.len() >= MAX_RECORDED_OUTBOUND_FRAMES
            || self
                .pending_sends
                .last()
                .is_some_and(|send| send.next_state != ConversationState::Send)
        {
            return Err(ConversationProblem::WrongState);
        }
        let id = self
            .next_send_id
            .checked_add(1)
            .ok_or(ConversationProblem::Exhausted)?;
        self.next_send_id = id;
        self.pending_sends.push(StagedExchangeSend {
            id,
            frame,
            next_state,
            dispatch_attempted: false,
        });
        self.validate()?;
        Ok(id)
    }

    pub fn mark_send_attempted(&mut self, send_id: u64) -> Result<(), ConversationProblem> {
        let send = self
            .pending_sends
            .first_mut()
            .ok_or(ConversationProblem::WrongState)?;
        if send.id != send_id {
            return Err(ConversationProblem::WrongState);
        }
        send.dispatch_attempted = true;
        Ok(())
    }

    pub fn acknowledge_send(
        &mut self,
        send_id: u64,
    ) -> Result<ConversationState, ConversationProblem> {
        if self.pending_sends.first().map(|send| send.id) != Some(send_id)
            || !self.pending_sends[0].dispatch_attempted
        {
            return Err(ConversationProblem::WrongState);
        }
        let send = self.pending_sends.remove(0);
        self.last_acked_send_id = send_id;
        Ok(send.next_state)
    }

    pub fn offer(&mut self, frame: ConversationPeerFrame) -> Result<(), ConversationProblem> {
        frame.validate()?;
        if self.inbound.len() >= MAX_PENDING_PEER_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        self.inbound.push(frame);
        Ok(())
    }

    /// Consume a mapped APPC/MRO peer frame from this shared exchange ledger.
    /// A retained remainder keeps the peer's flags until its final chunk.
    pub fn receive_mapped(
        &mut self,
        kind: ConversationKind,
        max_length: usize,
        retain_remainder: bool,
    ) -> Result<Option<ConversationDataReply>, ConversationProblem> {
        if kind == ConversationKind::AppcBasic || max_length > 32_767 {
            return Err(ConversationProblem::Length);
        }
        let (frame, retained) = if !self.retained.is_empty() {
            let meta = self
                .retained_meta
                .as_ref()
                .ok_or(ConversationProblem::WrongState)?;
            (
                ConversationPeerFrame {
                    data: self.retained.clone(),
                    next_state: meta.next_state,
                    end_of_chain: meta.end_of_chain,
                    inbound_fmh: meta.inbound_fmh,
                    signal: false,
                },
                true,
            )
        } else if let Some(frame) = self.inbound.first() {
            (frame.clone(), false)
        } else {
            return Ok(None);
        };
        if frame.signal {
            self.inbound.remove(0);
            return Ok(Some(ConversationDataReply {
                bytes: Vec::new(),
                returned_length: 0,
                original_length: 0,
                condition: DataCondition::Signal,
                end_of_chain: false,
                inbound_fmh: false,
                gds_field_complete: false,
                state: frame.next_state,
            }));
        }
        let original_length = frame.data.len();
        let take = max_length.min(original_length);
        let bytes = frame.data[..take].to_vec();
        let truncated = take < original_length;
        if truncated && retain_remainder {
            if take == 0 {
                return Ok(Some(ConversationDataReply {
                    bytes,
                    returned_length: 0,
                    original_length,
                    condition: DataCondition::Normal,
                    end_of_chain: false,
                    inbound_fmh: false,
                    gds_field_complete: false,
                    state: ConversationState::Receive,
                }));
            }
            self.retained = frame.data[take..].to_vec();
            self.retained_meta = Some(RetainedPeerMetadata {
                next_state: frame.next_state,
                end_of_chain: frame.end_of_chain,
                inbound_fmh: frame.inbound_fmh,
            });
        } else {
            self.retained.clear();
            self.retained_meta = None;
        }
        if !retained {
            self.inbound.remove(0);
        }
        let condition = if kind == ConversationKind::Mro && frame.inbound_fmh {
            DataCondition::InboundFmh
        } else if frame.end_of_chain && !truncated {
            DataCondition::EndOfChain
        } else if truncated && !retain_remainder {
            DataCondition::LengthError
        } else {
            DataCondition::Normal
        };
        Ok(Some(ConversationDataReply {
            bytes,
            returned_length: take,
            original_length,
            condition,
            end_of_chain: frame.end_of_chain && (!truncated || !retain_remainder),
            inbound_fmh: frame.inbound_fmh,
            gds_field_complete: false,
            state: if truncated && retain_remainder {
                ConversationState::Receive
            } else {
                frame.next_state
            },
        }))
    }

    pub fn record_outbound(
        &mut self,
        frame: ConversationOutboundFrame,
    ) -> Result<(), ConversationProblem> {
        frame.validate()?;
        if self.outbound.len() >= MAX_RECORDED_OUTBOUND_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        self.outbound.push(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_and_outbound_frames_are_explicit_and_bounded() {
        let mut exchange = ConversationExchangeState::default();
        exchange
            .offer(ConversationPeerFrame {
                data: b"RESPONSE".to_vec(),
                next_state: ConversationState::Receive,
                end_of_chain: true,
                inbound_fmh: false,
                signal: false,
            })
            .unwrap();
        exchange
            .record_outbound(ConversationOutboundFrame {
                data: b"REQUEST".to_vec(),
                attach_id: Some("HEADER1".into()),
                fmh: false,
                definite_response: true,
            })
            .unwrap();
        assert!(exchange.validate().is_ok());
        assert_eq!(exchange.inbound[0].data, b"RESPONSE");
        assert_eq!(exchange.outbound[0].data, b"REQUEST");
        assert_eq!(
            exchange.offer(ConversationPeerFrame {
                data: vec![0; MAX_EXCHANGE_FRAME_BYTES + 1],
                next_state: ConversationState::Receive,
                end_of_chain: false,
                inbound_fmh: false,
                signal: false,
            }),
            Err(ConversationProblem::Malformed)
        );
    }
}
