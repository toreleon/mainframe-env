//! One streaming authority for terminal resources; old effect framing is unchanged.
use super::*;
use mainframe_env_execution_api::{
    Abend, AbendDumpDisposition, Completion, ExecutionId, RootTerminalDisposition,
    RootTerminalResourceDigest,
};

const DOMAIN: &[u8] = b"mainframe-env.root-terminal-resource@1\0";
const MAX_ROWS: usize = 4096;

mod setup;
#[cfg(test)]
mod tests;
pub use setup::{RootTerminalSetup, canonical_root_terminal_setup_digest};

/// One exact read/write identity in a terminal resource preimage.
/// It is an observation, not a row permit or an alternate persistence codec.
#[derive(Clone, Copy, Debug)]
pub enum RootTerminalResourceRow<'a> {
    /// Exact retained bytes and version must survive the physical recheck.
    Exact {
        /// Owning namespace.
        namespace: &'a str,
        /// Exact object key.
        key: &'a str,
        /// Original positive version.
        version: u64,
        /// Exact retained bytes, borrowed rather than deeply allocated.
        payload: &'a [u8],
    },
    /// The original observation requires no row at this identity.
    Absent {
        /// Owning namespace.
        namespace: &'a str,
        /// Exact object key.
        key: &'a str,
    },
    /// Exact proposed replacement or insert, with its original CAS expectation.
    Put {
        /// Owning namespace.
        namespace: &'a str,
        /// Exact object key.
        key: &'a str,
        /// Proposed version.
        version: u64,
        /// None means insert-only; Some is the exact prior version.
        expected: Option<u64>,
        /// Exact proposed bytes.
        payload: &'a [u8],
    },
    /// Exact deletion dependency and operation.
    Delete {
        /// Owning namespace.
        namespace: &'a str,
        /// Exact object key.
        key: &'a str,
        /// Positive exact prior version.
        expected: u64,
    },
    /// Exact existing move without collapsing old and new identities.
    Move {
        /// Owning namespace.
        namespace: &'a str,
        /// Exact prior object key.
        old_key: &'a str,
        /// Exact new object key.
        key: &'a str,
        /// Proposed version.
        version: u64,
        /// Positive exact prior version.
        expected: u64,
        /// Exact proposed bytes.
        payload: &'a [u8],
    },
}

/// Borrowed exact native-root resource identity for terminal SAF/audit composition.
/// Construction is not host attestation, a finality proof or authorization.
#[derive(Clone, Copy, Debug)]
pub enum RootTerminalMachineObservation<'a> {
    /// Exact normal machine output, including its schema and original bytes.
    Completed(&'a Completion),
    /// Actual modeled ABEND code/reason/dump disposition, not a generic error.
    Abended(&'a Abend),
}

/// Complete borrowed known terminal resource; structural data is not authority.
pub struct RootTerminalResource<'a> {
    /// Exact original root execution.
    pub execution: &'a ExecutionId,
    /// Exact original run.
    pub run: &'a RunUnitId,
    /// Exact original authenticated principal.
    pub principal: &'a PrincipalId,
    /// Original invocation key, distinct from effect keys.
    pub invocation_key: &'a IdempotencyKey,
    /// Positive original attempt.
    pub attempt: u32,
    /// Actual first terminal lifecycle sequence.
    pub lifecycle_sequence: u64,
    /// Actual final publication tick, separately bound from capture/creation.
    pub observed_tick: u64,
    /// Immutable setup/admitted artifact identity.
    pub configuration_digest: &'a [u8; 32],
    /// Exact captured physical provider mutation epoch.
    pub provider_epoch: u64,
    /// Exact Closing ownership row version.
    pub closing_version: u64,
    /// Exact complete Closing ownership bytes.
    pub closing_payload: &'a [u8],
    /// Genuine source-backed known disposition supplied by the exclusive driver.
    pub disposition: RootTerminalDisposition,
    /// Unmodified machine observation retained by the genuine winning driver.
    pub machine: RootTerminalMachineObservation<'a>,
    /// Complete ordered dependency and mutation list, not standalone MQI digests.
    pub rows: &'a [RootTerminalResourceRow<'a>],
}

impl Canonical for RootTerminalResourceRow<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Exact {
                namespace,
                key,
                version,
                payload,
            } => {
                out.variant("RootTerminalRow", "Exact", 4)?;
                namespace.encode(out)?;
                key.encode(out)?;
                version.encode(out)?;
                payload.encode(out)
            }
            Self::Absent { namespace, key } => {
                out.variant("RootTerminalRow", "Absent", 2)?;
                namespace.encode(out)?;
                key.encode(out)
            }
            Self::Put {
                namespace,
                key,
                version,
                expected,
                payload,
            } => {
                out.variant("RootTerminalRow", "Put", 5)?;
                namespace.encode(out)?;
                key.encode(out)?;
                version.encode(out)?;
                expected.encode(out)?;
                payload.encode(out)
            }
            Self::Delete {
                namespace,
                key,
                expected,
            } => {
                out.variant("RootTerminalRow", "Delete", 3)?;
                namespace.encode(out)?;
                key.encode(out)?;
                expected.encode(out)
            }
            Self::Move {
                namespace,
                old_key,
                key,
                version,
                expected,
                payload,
            } => {
                out.variant("RootTerminalRow", "Move", 6)?;
                namespace.encode(out)?;
                old_key.encode(out)?;
                key.encode(out)?;
                version.encode(out)?;
                expected.encode(out)?;
                payload.encode(out)
            }
        }
    }
}
impl Canonical for RootTerminalResource<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        if self.rows.len() > MAX_ROWS
            || self.attempt == 0
            || self.lifecycle_sequence == 0
            || self.closing_version == 0
            || self.observed_tick == 0
            || self.observed_tick > i64::MAX as u64
        {
            return Err(HostProblem::ResourceExhausted);
        }
        match (self.disposition, self.machine) {
            (
                RootTerminalDisposition::Normal { return_code },
                RootTerminalMachineObservation::Completed(value),
            ) if return_code == value.return_code => {}
            (
                RootTerminalDisposition::KnownAbnormal,
                RootTerminalMachineObservation::Abended(value),
            ) if !value.code.is_empty()
                && value.code.len() <= 128
                && value.reason.as_ref().is_none_or(|v| v.len() <= 16384) => {}
            _ => return Err(HostProblem::Malformed),
        }
        out.object("RootTerminalResource", 14)?;
        self.execution.as_str().encode(out)?;
        self.run.encode(out)?;
        self.principal.encode(out)?;
        self.invocation_key.encode(out)?;
        self.attempt.encode(out)?;
        self.lifecycle_sequence.encode(out)?;
        self.observed_tick.encode(out)?;
        self.configuration_digest.as_slice().encode(out)?;
        self.provider_epoch.encode(out)?;
        self.closing_version.encode(out)?;
        self.closing_payload.encode(out)?;
        match self.disposition {
            RootTerminalDisposition::Normal { return_code } => {
                out.variant("RootTerminalDisposition", "Normal", 1)?;
                return_code.encode(out)?;
            }
            RootTerminalDisposition::KnownAbnormal => {
                out.variant("RootTerminalDisposition", "KnownAbnormal", 0)?;
            }
        }
        match self.machine {
            RootTerminalMachineObservation::Completed(value) => {
                out.variant("RootTerminalMachine", "Completed", 2)?;
                value.return_code.encode(out)?;
                value.output.encode(out)?;
            }
            RootTerminalMachineObservation::Abended(value) => {
                out.variant("RootTerminalMachine", "Abended", 3)?;
                value.code.encode(out)?;
                value.reason.encode(out)?;
                out.variant(
                    "AbendDumpDisposition",
                    match value.dump {
                        AbendDumpDisposition::Unspecified => "Unspecified",
                        AbendDumpDisposition::Requested => "Requested",
                        AbendDumpDisposition::Suppressed => "Suppressed",
                    },
                    0,
                )?;
            }
        }
        self.rows.encode(out)
    }
}

/// Shared SHA-256 terminal resource identity from one bounded streaming codec.
/// The supplied budget is exact, capped by the unchanged canonical hard ceiling.
pub fn canonical_root_terminal_resource_digest(
    resource: &RootTerminalResource<'_>,
    byte_limit: usize,
) -> Result<RootTerminalResourceDigest, HostProblem> {
    let mut hash = Sha256::new();
    encode(
        resource,
        DOMAIN,
        byte_limit.min(MAX_CANONICAL_EFFECT_BYTES),
        &mut |b| hash.update(b),
    )?;
    Ok(RootTerminalResourceDigest {
        value: hash.finalize().into(),
    })
}

/// Count exact terminal resource bytes without allocation or a second codec.
pub fn canonical_root_terminal_resource_size(
    resource: &RootTerminalResource<'_>,
    byte_limit: usize,
) -> Result<usize, HostProblem> {
    encode(
        resource,
        DOMAIN,
        byte_limit.min(MAX_CANONICAL_EFFECT_BYTES),
        &mut |_| {},
    )
}
