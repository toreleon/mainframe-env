//! Signed IMS TM definition contracts shared by packages and the runtime.

use crate::HostProblem;
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Bounds shared by the IMS TM contract validator and durable runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TmLimits {
    /// Maximum transaction definitions in one installed set.
    pub max_transactions: usize,
    /// Maximum alternate PCBs per transaction definition.
    pub max_alternate_pcbs: usize,
    /// Maximum retained inbound messages for the runtime.
    pub max_queued_messages: usize,
    /// Maximum retained outbound messages for the runtime.
    pub max_outbound_messages: usize,
    /// Maximum segments in one runtime message.
    pub max_segments_per_message: usize,
    /// Maximum bytes per runtime message segment.
    pub max_segment_bytes: usize,
    /// Maximum conversational scratchpad bytes.
    pub max_spa_bytes: usize,
    /// Maximum retained replay entries for the runtime.
    pub max_replays: usize,
    /// Maximum serialized runtime state bytes.
    pub max_state_bytes: usize,
    /// Maximum positive transaction timeout in the runtime tick domain.
    pub max_timeout_ticks: u64,
}

impl Default for TmLimits {
    fn default() -> Self {
        Self {
            max_transactions: 256,
            max_alternate_pcbs: 64,
            max_queued_messages: 65_536,
            max_outbound_messages: 65_536,
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
    /// Message-processing transaction environment.
    MessageProcessing,
    /// Message-driven batch environment.
    MessageDrivenBatch,
    /// CPI communications environment.
    CpiCommunications,
    /// Fast Path environment.
    FastPath,
}

/// Initial routing behavior of an alternate PCB.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmDestination {
    /// Retain a validated fixed terminal destination.
    Fixed(String),
    /// Destination is selected later through the modeled runtime.
    Modifiable,
}

/// One named alternate PCB made available to every instance of a transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmAlternatePcbDefinition {
    /// Unique valid 1-8 byte uppercase/digit/@#$ PCB identity within the transaction.
    pub name: String,
    /// Fixed bounded destination or explicit modifiable routing.
    pub destination: TmDestination,
    /// Retained express-output selection; it is not a delivered-message receipt.
    pub express: bool,
}

/// Generic transaction-to-program scheduling metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmTransactionDefinition {
    /// Valid 1-8 byte uppercase/digit/@#$ transaction identity.
    pub code: String,
    /// Valid 1-8 byte uppercase/digit/@#$ PSB selection.
    pub psb: String,
    /// Bounded execution Selector identity for program routing.
    pub program_selector: String,
    /// Bounded immutable ArtifactRef spelling for program selection.
    pub artifact: String,
    /// Nonempty bounded control-free generation identity; validation does not install it.
    pub required_generation: String,
    /// Explicit message-processing environment for applicability checks.
    pub context: TmExecutionContext,
    /// Retained scheduling priority; this validator imposes no additional numeric range.
    pub priority: u8,
    /// Positive timeout duration bounded by max_timeout_ticks.
    pub timeout_ticks: u64,
    /// True exactly when spa_size is nonzero.
    pub conversational: bool,
    /// Conversational scratchpad byte count bounded by max_spa_bytes.
    pub spa_size: usize,
    /// Bounded alternate definitions with unique names and valid fixed destinations.
    pub alternate_pcbs: Vec<TmAlternatePcbDefinition>,
}

impl TmTransactionDefinition {
    /// Check names, execution identities, positive bounded timeout, conversational scratchpad agreement and alternate uniqueness; no program is installed or dispatched.
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

    /// Borrow the alternate definition by exact case-sensitive name; unknown names return None.
    pub fn alternate(&self, name: &str) -> Option<&TmAlternatePcbDefinition> {
        self.alternate_pcbs.iter().find(|pcb| pcb.name == name)
    }
}

/// Complete bounded transaction definition set installed as one generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmDefinitionSet {
    /// Nonempty bounded transaction set with unique codes, validated before generation installation.
    pub transactions: Vec<TmTransactionDefinition>,
}

impl TmDefinitionSet {
    /// Require a nonempty bounded set of individually valid definitions with unique transaction codes.
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

fn valid_tm_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
        })
}
