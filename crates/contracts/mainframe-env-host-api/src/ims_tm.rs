//! Signed IMS TM definition contracts shared by packages and the runtime.

use crate::HostProblem;
use mainframe_env_execution_api::{ArtifactRef, InvocationLimits, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Bounds shared by the IMS TM contract validator and durable runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TmLimits {
    /// Maximum definitions installed in one transaction set; default 256.
    pub max_transactions: usize,
    /// Maximum alternate PCBs per transaction; default 64.
    pub max_alternate_pcbs: usize,
    /// Maximum retained input-message count; default 65,536.
    pub max_queued_messages: usize,
    /// Maximum retained outbound-message count; default 65,536.
    pub max_outbound_messages: usize,
    /// Maximum segments per message; default 256.
    pub max_segments_per_message: usize,
    /// Maximum message-segment length in bytes; default 32 KiB.
    pub max_segment_bytes: usize,
    /// Maximum conversation scratchpad length in bytes; default 32 KiB.
    pub max_spa_bytes: usize,
    /// Maximum retained replay-receipt count; default 65,536.
    pub max_replays: usize,
    /// Maximum serialized runtime-state length in bytes; default 64 MiB.
    pub max_state_bytes: usize,
    /// Maximum positive timeout in host logical ticks; default 86,400,000.
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
    /// Message-processing context identity.
    MessageProcessing,
    /// Message-driven batch context identity.
    MessageDrivenBatch,
    /// CPI communications context identity, subject to explicit call restrictions.
    CpiCommunications,
    /// Fast Path context identity, subject to explicit call restrictions.
    FastPath,
}

/// Initial routing behavior of an alternate PCB.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TmDestination {
    /// Initial fixed destination name validated by the transaction contract.
    Fixed(String),
    /// Destination is supplied or changed through an admitted runtime call.
    Modifiable,
}

/// One named alternate PCB made available to every instance of a transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmAlternatePcbDefinition {
    /// PCB identity unique within the transaction definition.
    pub name: String,
    /// Fixed or modifiable initial destination rule.
    pub destination: TmDestination,
    /// Declared express-message flag consumed by the TM runtime.
    pub express: bool,
}

/// Generic transaction-to-program scheduling metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmTransactionDefinition {
    /// Validated uppercase transaction resource identity, at most eight bytes.
    pub code: String,
    /// Validated PSB resource identity selected by this transaction.
    pub psb: String,
    /// Bounded execution selector for the scheduled program.
    pub program_selector: String,
    /// Bounded artifact reference consumed by package/runtime selection.
    pub artifact: String,
    /// Nonempty bounded generation identity fencing the selected package.
    pub required_generation: String,
    /// Declared TM context whose calls require separate applicability checks.
    pub context: TmExecutionContext,
    /// Scheduling priority value retained by the shared runtime.
    pub priority: u8,
    /// Positive relative timeout in host logical ticks, bounded by max_timeout_ticks.
    pub timeout_ticks: u64,
    /// Whether a nonzero scratchpad is required by this definition.
    pub conversational: bool,
    /// Scratchpad size in bytes; zero exactly for nonconversational definitions.
    pub spa_size: usize,
    /// Bounded alternate PCB definitions with distinct names.
    pub alternate_pcbs: Vec<TmAlternatePcbDefinition>,
}

impl TmTransactionDefinition {
    /// Validate names, execution identities, timeout, scratchpad and alternate PCB bounds.
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

    /// Look up an exact alternate PCB name; return None when absent.
    pub fn alternate(&self, name: &str) -> Option<&TmAlternatePcbDefinition> {
        self.alternate_pcbs.iter().find(|pcb| pcb.name == name)
    }
}

/// Complete bounded transaction definition set installed as one generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TmDefinitionSet {
    /// Nonempty bounded transaction definitions with unique transaction codes.
    pub transactions: Vec<TmTransactionDefinition>,
}

impl TmDefinitionSet {
    /// Validate every definition and reject empty, oversized or duplicate-code sets.
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
