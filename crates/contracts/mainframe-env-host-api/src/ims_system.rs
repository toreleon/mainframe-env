//! Typed operands and results for the IMS system and GSAM-adjacent call families.

use crate::{HostLimits, HostProblem, ImsCallSyntax, ImsExecutionContext};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Explicit ACCEPT status-group operand; selection alone is not a successful ACCEPT.
pub enum ImsStatusGroup {
    /// Status-group A operand.
    A,
    /// Status-group B operand.
    B,
}

/// The comparison catalog has two INIT/ACCEPT rows with the same call spelling.
/// This identity is used only for applicability validation, never for dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsAcceptRow {
    /// First catalog ACCEPT applicability row.
    Initial,
    /// Availability catalog ACCEPT applicability row.
    Availability,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
/// Validated ASCII Q/LOCKCLASS letter A through J; deserialized values still require validity checks.
pub struct ImsQClass(u8);

impl ImsQClass {
    #[must_use]
    /// Admit exactly ASCII A through J; all other bytes return None.
    pub fn new(value: u8) -> Option<Self> {
        (b'A'..=b'J').contains(&value).then_some(Self(value))
    }

    #[must_use]
    /// Return the retained ASCII class byte without numeric reinterpretation.
    pub const fn byte(self) -> u8 {
        self.0
    }

    #[must_use]
    /// Recheck the A-J domain, including values created by deserialization.
    pub fn is_valid(self) -> bool {
        (b'A'..=b'J').contains(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// POS observation selector; SSA presence is checked against permitted keyword forms.
pub enum ImsPositionKeyword {
    /// Default POS selection, with optional SSA.
    Default,
    /// Select the V5 segment-RBA observation without SSA.
    V5SegmentRba,
    /// Select PC segment RTS observation without SSA.
    PcSegmentRts,
    /// Select PC segment high-water observation without SSA.
    PcSegmentHighWaterMark,
    /// Select highest-segment timestamp observation without SSA.
    PcHighestSegmentTs,
    /// Select logical-begin timestamp observation without SSA.
    PcLogicalBeginTs,
    /// Select segment timestamp; an SSA is required.
    PcSegmentTs,
}

/// Exactly one POS SSA; an absent predicate is an unqualified SSA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsPositionSsa {
    /// Uppercase/digit segment identity, 1-8 bytes.
    pub segment: String,
    /// Optional uppercase/digit field name, present exactly with value.
    pub field: Option<String>,
    /// Optional nonempty exact comparison bytes bounded by max_record_bytes.
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// OSAM/VSAM modeled statistics pool identity.
pub enum ImsBufferPoolKind {
    /// OSAM modeled pool.
    Osam,
    /// VSAM modeled pool.
    Vsam,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// STAT function family retained separately from output format and extension selection.
pub enum ImsStatisticsFamily {
    /// DBAS function family.
    Dbas,
    /// DBES function family; the only family permitting extended selection.
    Dbes,
    /// VBAS function family.
    Vbas,
    /// VBES function family.
    Vbes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// STAT output format validated with family and extended-mode applicability.
pub enum ImsStatisticsFormat {
    /// Full statistics representation.
    Full,
    /// OSAM representation, restricted to DBAS/DBES.
    Osam,
    /// Summary representation, without extended selection.
    Summary,
    /// Unformatted statistics representation.
    Unformatted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// STAT family/format/extension selection; only represented validated combinations are admitted.
pub struct ImsStatisticsFunction {
    /// Exact STAT function family.
    pub family: ImsStatisticsFamily,
    /// Format checked against the selected family.
    pub format: ImsStatisticsFormat,
    /// Extended mode admitted only for DBES Full, Osam or Unformatted.
    pub extended: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed system-call operands; admission does not imply availability in every execution context.
pub enum ImsSystemCall {
    /// Select source-qualified ACCEPT status group.
    Accept {
        /// Distinguish the two source ACCEPT applicability rows without inventing dispatch differences.
        row: ImsAcceptRow,
        /// Explicit ACCEPT status-group selector.
        group: ImsStatusGroup,
    },
    /// Observe a positive target PCB ordinal.
    Query {
        /// Positive one-based PCB ordinal queried by the call.
        target_pcb: u16,
    },
    /// Request availability refresh of modeled PCBs.
    Refresh,
    /// Request release of modeled Q-class reservations.
    Dequeue {
        /// Optional validated A-J Q class whose reservations are to be released.
        class: Option<ImsQClass>,
    },
    /// Observe modeled SCD/PST addresses.
    Gscd,
    /// Observe modeled area positioning under checked keyword/SSA selection.
    Position {
        /// Optional checked POS selector; presence constrains the keyword.
        ssa: Option<ImsPositionSsa>,
        /// POS observation selection, validated with SSA presence.
        keyword: ImsPositionKeyword,
    },
    /// Observe modeled pool statistics under checked function selection.
    Statistics {
        /// Validated STAT family/format/extension selection.
        function: ImsStatisticsFunction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Explicit execution context, syntax and system operands; structural validation is separate from call-site applicability.
pub struct ImsSystemRequest {
    /// Explicit trusted execution context, not inferred from call spelling.
    pub context: ImsExecutionContext,
    /// Explicit CALL/command syntax retained for applicability.
    pub syntax: ImsCallSyntax,
    /// Typed operands; call-site applicability requires its separate validator.
    pub call: ImsSystemCall,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Positive PCB ordinal, two-byte status and organization observation, not a PCB ownership token.
pub struct ImsPcbAvailability {
    /// Positive one-based observed PCB ordinal.
    pub pcb: u16,
    /// Exactly two bytes of observed PCB status.
    pub status: String,
    /// Nonempty bounded observed organization label.
    pub organization: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Modeled area position and capacity observations; undefined observations are not synthesized by this DTO.
pub struct ImsPositionArea {
    /// Nonempty bounded modeled area name.
    pub name: String,
    /// Cycle count followed by relative byte address in the modeled DEDB area.
    pub position: [u8; 8],
    /// Observed unused sequential dependent control-interval count.
    pub unused_sdep_cis: u32,
    /// Observed unused independent overflow control-interval count.
    pub unused_iov_cis: u32,
    /// Optional modeled timestamp observation; no implicit wall-clock conversion occurs.
    pub timestamp: Option<u64>,
    /// Optional bounded IMS identity associated with the observation.
    pub ims_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Positive pool geometry and read/write counters observed from the modeled provider.
pub struct ImsBufferStatistics {
    /// Nonempty bounded modeled pool name.
    pub pool: String,
    /// OSAM/VSAM pool classification.
    pub kind: ImsBufferPoolKind,
    /// Positive bytes per buffer.
    pub buffer_bytes: u32,
    /// Positive buffer count.
    pub buffers: u32,
    /// Observed modeled read counter.
    pub reads: u64,
    /// Observed modeled write counter.
    pub writes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Typed system observations with bounded areas/PCB lists; call-specific interpretation remains with the caller.
pub enum ImsSystemResult {
    /// Accepted group observation.
    Accepted {
        /** Retained accepted group selector. */
        group: ImsStatusGroup,
    },
    /// Observed target PCB availability.
    Query {
        /** Observed queried PCB metadata. */
        pcb: ImsPcbAvailability,
    },
    /// Bounded refreshed PCB observations.
    Refreshed {
        /** Bounded refreshed PCB observations, not live ownership tokens. */
        pcbs: Vec<ImsPcbAvailability>,
    },
    /// Observed reservation release count.
    Dequeued {
        /** Number of released modeled reservations. */
        released: u32,
    },
    /// Modeled SCD/PST address observation.
    Gscd {
        /** Modeled 32-bit SCD address, not a native process pointer. */
        scd_address: u32,
        /** Modeled 32-bit PST address, not a native process pointer. */
        pst_address: u32,
    },
    /// Bounded modeled area observations.
    Positioned {
        /** Bounded modeled area position observations. */
        areas: Vec<ImsPositionArea>,
    },
    /// Optional modeled pool observation.
    Statistics {
        /** Optional modeled pool statistics; absence is preserved rather than synthesized. */
        pool: Option<ImsBufferStatistics>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Modeled 32-bit directory addresses, not process pointers or access authority.
pub struct ImsSystemDirectory {
    /// Modeled 32-bit SCD address, not a native process pointer.
    pub scd_address: u32,
    /// Modeled 32-bit PST address, not a native process pointer.
    pub pst_address: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Named modeled DEDB area capacities used by the existing provider state.
pub struct ImsDedbAreaDefinition {
    /// Database label to which the modeled area belongs.
    pub database: String,
    /// Modeled area identity.
    pub name: String,
    /// Sequential dependent control-interval capacity, in interval counts.
    pub sdep_capacity_cis: u32,
    /// Independent overflow control-interval capacity, in interval counts.
    pub iov_capacity_cis: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Named modeled pool with byte size and count; no native buffer allocation is performed by this DTO.
pub struct ImsBufferPoolDefinition {
    /// Modeled pool identity.
    pub name: String,
    /// OSAM/VSAM pool selection.
    pub kind: ImsBufferPoolKind,
    /// Declared bytes per modeled buffer.
    pub buffer_bytes: u32,
    /// Declared modeled buffer count.
    pub buffers: u32,
}

/// Runtime resources are installed into the existing IMS provider row store.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSystemRuntimeDefinition {
    /// Optional modeled directory addresses.
    pub directory: Option<ImsSystemDirectory>,
    /// Modeled area definitions installed through existing provider storage.
    pub dedb_areas: Vec<ImsDedbAreaDefinition>,
    /// Modeled pool definitions installed through existing provider storage.
    pub buffer_pools: Vec<ImsBufferPoolDefinition>,
}

impl ImsSystemRequest {
    /// Check positive query ordinal, POS SSA/keyword shape and admitted STAT combinations; no provider or context policy is executed.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match &self.call {
            ImsSystemCall::Query { target_pcb } if *target_pcb == 0 => Err(HostProblem::Malformed),
            ImsSystemCall::Position { ssa, keyword } => {
                if ssa.is_some()
                    && !matches!(
                        keyword,
                        ImsPositionKeyword::Default | ImsPositionKeyword::PcSegmentTs
                    )
                    || ssa.is_none() && *keyword == ImsPositionKeyword::PcSegmentTs
                {
                    return Err(HostProblem::Malformed);
                }
                if let Some(ssa) = ssa
                    && (ssa.segment.is_empty()
                        || ssa.segment.len() > 8
                        || !ssa
                            .segment
                            .bytes()
                            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                        || ssa.field.is_some() != ssa.value.is_some()
                        || ssa.field.as_ref().is_some_and(|field| {
                            field.is_empty()
                                || field.len() > 8
                                || !field
                                    .bytes()
                                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                        })
                        || ssa.value.as_ref().is_some_and(|value| {
                            value.is_empty() || value.len() > limits.max_record_bytes
                        }))
                {
                    return Err(HostProblem::Malformed);
                }
                Ok(())
            }
            ImsSystemCall::Statistics { function } => {
                if function.extended
                    && !(function.family == ImsStatisticsFamily::Dbes
                        && matches!(
                            function.format,
                            ImsStatisticsFormat::Full
                                | ImsStatisticsFormat::Osam
                                | ImsStatisticsFormat::Unformatted
                        ))
                    || function.format == ImsStatisticsFormat::Osam
                        && !matches!(
                            function.family,
                            ImsStatisticsFamily::Dbas | ImsStatisticsFamily::Dbes
                        )
                {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

impl ImsSystemResult {
    /// Check represented observation lengths/counts and positive pool geometry; do not fabricate missing observations.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        let valid_name = |name: &str| !name.is_empty() && name.len() <= limits.max_name_bytes;
        match self {
            Self::Query { pcb }
                if pcb.pcb == 0 || pcb.status.len() != 2 || !valid_name(&pcb.organization) =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Refreshed { pcbs }
                if pcbs.len() > limits.max_fields
                    || pcbs.iter().any(|pcb| {
                        pcb.pcb == 0 || pcb.status.len() != 2 || !valid_name(&pcb.organization)
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Positioned { areas }
                if areas.len() > limits.max_records
                    || areas.iter().any(|area| {
                        !valid_name(&area.name)
                            || area.ims_id.as_ref().is_some_and(|id| !valid_name(id))
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Statistics { pool: Some(pool) }
                if !valid_name(&pool.pool) || pool.buffer_bytes == 0 || pool.buffers == 0 =>
            {
                Err(HostProblem::Malformed)
            }
            _ => Ok(()),
        }
    }
}
