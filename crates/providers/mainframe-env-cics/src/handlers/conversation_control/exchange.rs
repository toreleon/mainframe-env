//! Explicit APPC/MRO peer frames and outbound exchange records.

use super::{ConversationAttachHeader, ConversationProblem, ConversationState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_EXCHANGE_FRAME_BYTES: usize = 1_048_576;
pub const MAX_PENDING_PEER_FRAMES: usize = 32;
pub const MAX_RECORDED_OUTBOUND_FRAMES: usize = 32;
pub const MAX_CONVERSE_ATTEMPTS: usize = 32;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_header: Option<ConversationAttachHeader>,
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
            || self.attach_header.as_ref().is_some_and(|header| {
                header.validate().is_err()
                    || self.attach_id.as_deref() != Some(header.name.as_str())
            })
        {
            return Err(ConversationProblem::Malformed);
        }
        Ok(())
    }
}

/// One outbound CONVERSE frame durably staged before waiting for its peer.
/// Its request identity fences retries without treating local delivery as an
/// APPC/MRO acknowledgement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationPendingConverse {
    pub owner_principal: String,
    pub owner_epoch: u64,
    pub semantic_digest: [u8; 32],
    pub attempts: Vec<ConversationPendingAttempt>,
    pub outbound: ConversationOutboundFrame,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationPendingAttempt {
    pub effect_key: String,
    pub mutation_sequence: u64,
    pub request_digest: [u8; 32],
}

impl ConversationPendingConverse {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        let mut keys = BTreeSet::new();
        if self.owner_principal.is_empty()
            || self.owner_principal.len() > 128
            || self.owner_epoch == 0
            || self.attempts.is_empty()
            || self.attempts.len() > MAX_CONVERSE_ATTEMPTS
            || self.attempts.iter().any(|attempt| {
                attempt.effect_key.is_empty()
                    || attempt.effect_key.len() > 256
                    || attempt.mutation_sequence == 0
                    || !keys.insert(&attempt.effect_key)
            })
            || self
                .attempts
                .windows(2)
                .any(|pair| pair[0].mutation_sequence >= pair[1].mutation_sequence)
        {
            return Err(ConversationProblem::Malformed);
        }
        self.outbound.validate()
    }

    pub fn accept_attempt(
        &mut self,
        attempt: ConversationPendingAttempt,
        principal: &str,
        epoch: u64,
        semantic_digest: [u8; 32],
    ) -> Result<bool, ConversationProblem> {
        if self.owner_principal != principal
            || self.owner_epoch != epoch
            || self.semantic_digest != semantic_digest
        {
            return Err(ConversationProblem::StaleOwner);
        }
        if let Some(saved) = self
            .attempts
            .iter()
            .find(|saved| saved.effect_key == attempt.effect_key)
        {
            return if saved == &attempt {
                Ok(false)
            } else {
                Err(ConversationProblem::StaleOwner)
            };
        }
        let first = &self.attempts[0];
        let last = self.attempts.last().ok_or(ConversationProblem::Malformed)?;
        let same_stream = first
            .effect_key
            .rsplit_once(':')
            .zip(attempt.effect_key.rsplit_once(':'))
            .is_some_and(
                |((first_prefix, first_sequence), (next_prefix, next_sequence))| {
                    first_prefix == next_prefix
                        && first_sequence.parse::<u64>() == Ok(first.mutation_sequence)
                        && next_sequence.parse::<u64>() == Ok(attempt.mutation_sequence)
                },
            );
        if !same_stream || attempt.mutation_sequence <= last.mutation_sequence {
            return Err(ConversationProblem::StaleOwner);
        }
        if self.attempts.len() >= MAX_CONVERSE_ATTEMPTS {
            return Err(ConversationProblem::Exhausted);
        }
        self.attempts.push(attempt);
        Ok(true)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExchangeState {
    pub inbound: Vec<ConversationPeerFrame>,
    pub outbound: Vec<ConversationOutboundFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_converse: Option<ConversationPendingConverse>,
    /// The remainder retained by NOTRUNCATE for a later RECEIVE sibling.
    pub retained: Vec<u8>,
}

impl ConversationExchangeState {
    pub fn validate(&self) -> Result<(), ConversationProblem> {
        if self.inbound.len() > MAX_PENDING_PEER_FRAMES
            || self.outbound.len() + usize::from(self.pending_converse.is_some())
                > MAX_RECORDED_OUTBOUND_FRAMES
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
        if let Some(pending) = &self.pending_converse {
            pending.validate()?;
            if !self.retained.is_empty() {
                return Err(ConversationProblem::Malformed);
            }
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

    pub fn stage_converse(
        &mut self,
        pending: ConversationPendingConverse,
    ) -> Result<(), ConversationProblem> {
        pending.validate()?;
        if self.pending_converse.is_some() || !self.retained.is_empty() {
            return Err(ConversationProblem::WrongState);
        }
        if self.outbound.len() >= MAX_RECORDED_OUTBOUND_FRAMES {
            return Err(ConversationProblem::Exhausted);
        }
        self.pending_converse = Some(pending);
        Ok(())
    }

    pub fn complete_pending_converse(&mut self) -> Result<(), ConversationProblem> {
        let pending = self
            .pending_converse
            .take()
            .ok_or(ConversationProblem::WrongState)?;
        self.record_outbound(pending.outbound)
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
                attach_header: None,
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
