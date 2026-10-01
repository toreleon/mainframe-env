use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::{HostProblem, ImsPcbKind, ImsStatusContext, resolve_ims_status};
use serde::{Deserialize, Serialize};

pub use mainframe_env_host_api::{
    TmAlternatePcbDefinition, TmDefinitionSet, TmDestination, TmExecutionContext, TmLimits,
    TmTransactionDefinition,
};

/// One complete input message admitted to a transaction queue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmInputMessage {
    pub message_id: String,
    pub transaction: String,
    pub source: String,
    pub user_id: Option<String>,
    pub group_name: Option<String>,
    pub conversation_id: Option<String>,
    pub segments: Vec<Vec<u8>>,
}

impl TmInputMessage {
    pub fn validate(&self, limits: TmLimits) -> Result<(), HostProblem> {
        let identity_limits = InvocationLimits::default();
        IdempotencyKey::new(self.message_id.clone(), identity_limits)
            .map_err(|_| HostProblem::Malformed)?;
        if !valid_tm_name(&self.transaction)
            || !valid_tm_name(&self.source)
            || self
                .user_id
                .as_deref()
                .is_some_and(|value| !valid_tm_name(value))
            || self
                .group_name
                .as_deref()
                .is_some_and(|value| !valid_tm_name(value))
            || self
                .conversation_id
                .as_deref()
                .is_some_and(|value| IdempotencyKey::new(value, identity_limits).is_err())
        {
            return Err(HostProblem::Malformed);
        }
        if self.segments.is_empty() {
            return Err(HostProblem::Malformed);
        }
        if self.segments.len() > limits.max_segments_per_message
            || self
                .segments
                .iter()
                .any(|segment| segment.len() > limits.max_segment_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(())
    }
}

/// PCB selected for a message call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmPcb {
    Io,
    Alternate(String),
}

/// Conversation disposition applied at the current message commit point.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmConversationAction {
    Continue { spa: Vec<u8> },
    Switch { transaction: String, spa: Vec<u8> },
    End,
}

/// Typed IMS TM call made by one scheduled run unit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmCall {
    GetUnique,
    GetNext,
    Insert {
        pcb: TmPcb,
        segment: Vec<u8>,
    },
    Change {
        pcb: String,
        destination: String,
    },
    Purge {
        pcb: TmPcb,
    },
    Commit {
        conversation: Option<TmConversationAction>,
    },
    Rollback,
    Terminate,
}

impl TmCall {
    pub fn validate(
        &self,
        context: TmExecutionContext,
        transaction: &TmTransactionDefinition,
        limits: TmLimits,
    ) -> Result<(), HostProblem> {
        transaction.validate(limits)?;
        match self {
            Self::GetUnique | Self::GetNext => {
                supported_message_context(context)?;
            }
            Self::Insert { pcb, segment } => {
                validate_segment(segment, limits)?;
                validate_pcb(pcb, transaction)?;
                if context == TmExecutionContext::CpiCommunications && matches!(pcb, TmPcb::Io) {
                    return Err(HostProblem::Unsupported);
                }
            }
            Self::Change { pcb, destination } => {
                supported_message_context(context)?;
                if !valid_tm_name(destination)
                    || !matches!(
                        transaction.alternate(pcb),
                        Some(TmAlternatePcbDefinition {
                            destination: TmDestination::Modifiable,
                            ..
                        })
                    )
                {
                    return Err(HostProblem::Malformed);
                }
            }
            Self::Purge { pcb } => {
                if context == TmExecutionContext::FastPath {
                    return Err(HostProblem::Unsupported);
                }
                validate_pcb(pcb, transaction)?;
                if context == TmExecutionContext::CpiCommunications && matches!(pcb, TmPcb::Io) {
                    return Err(HostProblem::Unsupported);
                }
            }
            Self::Commit { conversation } => {
                if let Some(action) = conversation {
                    validate_conversation(action, transaction, limits)?;
                }
            }
            Self::Rollback | Self::Terminate => {}
        }
        Ok(())
    }
}

/// Exact two-byte core status values used by this bounded TM foundation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TmPcbStatus([u8; 2]);

impl<'de> Deserialize<'de> for TmPcbStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let code = <[u8; 2]>::deserialize(deserializer)?;
        resolve_ims_status(&code, ImsStatusContext::Message, ImsPcbKind::Io)
            .map_err(|_| serde::de::Error::custom("unsupported IMS TM PCB status"))?;
        match code {
            [b' ', b' '] => Ok(Self::SUCCESS),
            [b'Q', b'C'] => Ok(Self::NO_MORE_MESSAGES),
            [b'Q', b'D'] => Ok(Self::NO_MORE_SEGMENTS),
            [b'A', b'D'] => Ok(Self::INVALID_CALL),
            [b'Q', b'F'] => Ok(Self::INVALID_SEGMENT_LENGTH),
            _ => Err(serde::de::Error::custom("unsupported IMS TM PCB status")),
        }
    }
}

impl TmPcbStatus {
    pub const SUCCESS: Self = Self(*b"  ");
    pub const NO_MORE_MESSAGES: Self = Self(*b"QC");
    pub const NO_MORE_SEGMENTS: Self = Self(*b"QD");
    pub const INVALID_CALL: Self = Self(*b"AD");
    /// QF: the message segment is shorter than the minimum length (IMS 15.6
    /// message-call status table and the QF explanation topic), not a queue-full
    /// condition.
    pub const INVALID_SEGMENT_LENGTH: Self = Self(*b"QF");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self.0 {
            [b' ', b' '] => "  ",
            [b'Q', b'C'] => "QC",
            [b'Q', b'D'] => "QD",
            [b'A', b'D'] => "AD",
            [b'Q', b'F'] => "QF",
            _ => panic!("unsupported IMS TM PCB status"),
        }
    }

    pub(crate) const fn valid(self) -> bool {
        matches!(
            self.0,
            [b' ', b' '] | [b'Q', b'C'] | [b'Q', b'D'] | [b'A', b'D'] | [b'Q', b'F']
        )
    }
}

fn validate_segment(segment: &[u8], limits: TmLimits) -> Result<(), HostProblem> {
    if segment.len() > limits.max_segment_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn validate_pcb(pcb: &TmPcb, transaction: &TmTransactionDefinition) -> Result<(), HostProblem> {
    match pcb {
        TmPcb::Io => Ok(()),
        TmPcb::Alternate(name) if transaction.alternate(name).is_some() => Ok(()),
        TmPcb::Alternate(_) => Err(HostProblem::Malformed),
    }
}

fn validate_conversation(
    action: &TmConversationAction,
    transaction: &TmTransactionDefinition,
    limits: TmLimits,
) -> Result<(), HostProblem> {
    if !transaction.conversational {
        return Err(HostProblem::Unsupported);
    }
    match action {
        TmConversationAction::Continue { spa } => validate_spa(spa, transaction, limits),
        TmConversationAction::Switch {
            transaction: next,
            spa,
        } => {
            if !valid_tm_name(next) {
                return Err(HostProblem::Malformed);
            }
            validate_spa(spa, transaction, limits)
        }
        TmConversationAction::End => Ok(()),
    }
}

fn validate_spa(
    spa: &[u8],
    transaction: &TmTransactionDefinition,
    limits: TmLimits,
) -> Result<(), HostProblem> {
    if spa.len() > limits.max_spa_bytes || spa.len() > transaction.spa_size {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn supported_message_context(context: TmExecutionContext) -> Result<(), HostProblem> {
    match context {
        TmExecutionContext::MessageProcessing | TmExecutionContext::MessageDrivenBatch => Ok(()),
        TmExecutionContext::CpiCommunications | TmExecutionContext::FastPath => {
            Err(HostProblem::Unsupported)
        }
    }
}

fn valid_tm_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
        })
}
