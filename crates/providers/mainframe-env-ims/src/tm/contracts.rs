use mainframe_env_execution_api::{ArtifactRef, IdempotencyKey, InvocationLimits, Selector};
use mainframe_env_host_api::{HostProblem, ImsPcbKind, ImsStatusContext, resolve_ims_status};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Bounds shared by the IMS TM contract validator and durable runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TmLimits {
    pub max_transactions: usize,
    pub max_alternate_pcbs: usize,
    pub max_queued_messages: usize,
    pub max_segments_per_message: usize,
    pub max_segment_bytes: usize,
    pub max_spa_bytes: usize,
    pub max_replays: usize,
    pub max_state_bytes: usize,
    pub max_timeout_ticks: u64,
}

impl Default for TmLimits {
    fn default() -> Self {
        Self {
            max_transactions: 256,
            max_alternate_pcbs: 64,
            max_queued_messages: 65_536,
            max_segments_per_message: 256,
            max_segment_bytes: 32 * 1024,
            max_spa_bytes: 32 * 1024,
            max_replays: 65_536,
            max_state_bytes: 64 * 1024 * 1024,
            max_timeout_ticks: 86_400_000,
        }
    }
}

/// Execution contexts whose call applicability is explicit in the TM contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmExecutionContext {
    MessageProcessing,
    MessageDrivenBatch,
    CpiCommunications,
    FastPath,
}

/// Initial routing behavior of an alternate PCB.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmDestination {
    Fixed(String),
    Modifiable,
}

/// One named alternate PCB made available to every instance of a transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmAlternatePcbDefinition {
    pub name: String,
    pub destination: TmDestination,
    pub express: bool,
}

/// Generic transaction-to-program scheduling metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmTransactionDefinition {
    pub code: String,
    pub psb: String,
    pub program_selector: String,
    pub artifact: String,
    pub required_generation: String,
    pub context: TmExecutionContext,
    pub priority: u8,
    pub timeout_ticks: u64,
    pub conversational: bool,
    pub spa_size: usize,
    pub alternate_pcbs: Vec<TmAlternatePcbDefinition>,
}

impl TmTransactionDefinition {
    pub fn validate(&self, limits: TmLimits) -> Result<(), HostProblem> {
        if !valid_tm_name(&self.code) || !valid_tm_name(&self.psb) {
            return Err(HostProblem::Malformed);
        }
        let identity_limits = InvocationLimits::default();
        Selector::new(self.program_selector.clone(), identity_limits)
            .map_err(|_| HostProblem::Malformed)?;
        ArtifactRef::new(self.artifact.clone(), identity_limits)
            .map_err(|_| HostProblem::Malformed)?;
        if self.required_generation.is_empty()
            || self.required_generation.len() > identity_limits.max_identity_bytes
            || self.required_generation.chars().any(char::is_control)
        {
            return Err(HostProblem::Malformed);
        }
        if self.timeout_ticks == 0 {
            return Err(HostProblem::Malformed);
        }
        if self.timeout_ticks > limits.max_timeout_ticks
            || self.alternate_pcbs.len() > limits.max_alternate_pcbs
            || self.spa_size > limits.max_spa_bytes
        {
            return Err(HostProblem::ResourceExhausted);
        }
        if self.conversational != (self.spa_size != 0) {
            return Err(HostProblem::Malformed);
        }
        let mut names = BTreeSet::new();
        for pcb in &self.alternate_pcbs {
            if !valid_tm_name(&pcb.name) || !names.insert(pcb.name.as_str()) {
                return Err(HostProblem::Malformed);
            }
            if let TmDestination::Fixed(destination) = &pcb.destination
                && !valid_tm_name(destination)
            {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }

    pub(crate) fn alternate(&self, name: &str) -> Option<&TmAlternatePcbDefinition> {
        self.alternate_pcbs.iter().find(|pcb| pcb.name == name)
    }
}

/// Complete bounded transaction definition set installed as one generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmDefinitionSet {
    pub transactions: Vec<TmTransactionDefinition>,
}

impl TmDefinitionSet {
    pub fn validate(&self, limits: TmLimits) -> Result<(), HostProblem> {
        if self.transactions.is_empty() {
            return Err(HostProblem::Malformed);
        }
        if self.transactions.len() > limits.max_transactions {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut codes = BTreeSet::new();
        for transaction in &self.transactions {
            transaction.validate(limits)?;
            if !codes.insert(transaction.code.as_str()) {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }
}

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
            [b'A', b'D'] => Ok(Self::INVALID_CALL),
            [b'Q', b'F'] => Ok(Self::INVALID_SEGMENT_LENGTH),
            _ => Err(serde::de::Error::custom("unsupported IMS TM PCB status")),
        }
    }
}

impl TmPcbStatus {
    pub const SUCCESS: Self = Self(*b"  ");
    pub const NO_MORE_MESSAGES: Self = Self(*b"QC");
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
            [b'A', b'D'] => "AD",
            [b'Q', b'F'] => "QF",
            _ => panic!("unsupported IMS TM PCB status"),
        }
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
