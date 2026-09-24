//! Explicit APPC/MRO peer frames and outbound exchange records.

use super::{ConversationProblem, ConversationState};
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
    /// The remainder retained by NOTRUNCATE for a later RECEIVE sibling.
    pub retained: Vec<u8>,
}

impl ConversationExchangeState {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.inbound.len() > MAX_PENDING_PEER_FRAMES
            || self.outbound.len() > MAX_RECORDED_OUTBOUND_FRAMES
            || self.retained.len() > MAX_EXCHANGE_FRAME_BYTES
        {
            return Err(ConversationProblem::Malformed);
        }
        for frame in &self.inbound {
            frame.validate()?;
        }
        for frame in &self.outbound {
            frame.validate()?;
        }
        Ok(())
    }

    pub fn offer(&mut self, frame: ConversationPeerFrame) -> Result<(), ConversationProblem> {
        frame.validate()?;
        if self.inbound.len() >= MAX_PENDING_PEER_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        self.inbound.push(frame);
        Ok(())
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
