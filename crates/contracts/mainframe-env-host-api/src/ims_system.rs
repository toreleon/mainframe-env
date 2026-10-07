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
/// Buffer-pool category in the local statistics and runtime-definition projection.
pub enum ImsBufferPoolKind {
    /// OSAM pool identity.
    Osam,
    /// VSAM pool identity.
    Vsam,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// STAT function-family identity; enhanced execution support is separately bounded.
pub enum ImsStatisticsFamily {
    /// Basic OSAM-family selector.
    Dbas,
    /// Enhanced OSAM-family selector; a typed identity does not admit enhanced observations.
    Dbes,
    /// Basic VSAM-family selector.
    Vbas,
    /// Enhanced VSAM-family selector; a typed identity does not admit enhanced observations.
    Vbes,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// STAT format selector; returned host observations are not raw print/binary layouts.
pub enum ImsStatisticsFormat {
    /// Full-format selector identity.
    Full,
    /// OSAM-specific format selector, rejected with VSAM families.
    Osam,
    /// Summary-format selector identity.
    Summary,
    /// Unformatted selector identity, without a raw-layout equivalence claim.
    Unformatted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// STAT family, format and extension flag validated before capacity or result checks.
pub struct ImsStatisticsFunction {
    /// Requested statistics family.
    pub family: ImsStatisticsFamily,
    /// Requested format selector.
    pub format: ImsStatisticsFormat,
    /// Whether the extended function is requested; capacity support may remain Unsupported.
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
    /// Bounded host projection, not IBM print records or binary fullword layout.
    StatisticsV2 {
        /// STAT selector; support and capacity are checked separately from its identity.
        function: ImsStatisticsFunction,
        /// Caller capacity in bytes, bounded by the conservative minimum and host ceiling.
        io_area_bytes: u32,
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

/// Only explicitly published read/write counters are projected. No other IBM
/// buffer-handler, error, hiperspace or coupling-facility statistic is implied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsStatisticsObservationV2 {
    /// One explicitly ordered VSAM subpool observation.
    Subpool {
        /// Published geometry and counters for the selected subpool.
        statistics: ImsBufferStatistics,
    },
    /// Aggregate basic buffer geometry and published read/write counters.
    Totals {
        /// Aggregate buffer count.
        buffers: u64,
        /// Aggregate modeled buffer storage in bytes.
        storage_bytes: u64,
        /// Aggregate published read count.
        reads: u64,
        /// Aggregate published write count.
        writes: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned system-call observation; validate bounds and unsupported statistics families before use.
pub enum ImsSystemResult {
    /// Acknowledged typed status-group selection.
    Accepted {
        /// Status group reported by the operation.
        group: ImsStatusGroup,
    },
    /// Availability observation for one PCB.
    Query {
        /// Numbered PCB status and organization observation.
        pcb: ImsPcbAvailability,
    },
    /// Bounded refreshed PCB availability list.
    Refreshed {
        /// PCB observations bounded by max_fields.
        pcbs: Vec<ImsPcbAvailability>,
    },
    /// Modeled Q reservation release count.
    Dequeued {
        /// Number of reservations released by this operation.
        released: u32,
    },
    /// Installed local directory values, without physical address equivalence.
    Gscd {
        /// Modeled SCD address value from installed runtime metadata.
        scd_address: u32,
        /// Modeled PST address value from installed runtime metadata.
        pst_address: u32,
    },
    /// Bounded modeled DEDB area observations.
    Positioned {
        /// Area observations bounded by max_records.
        areas: Vec<ImsPositionArea>,
    },
    /// Legacy optional pool-statistics observation.
    Statistics {
        /// Observed pool, or None when no pool observation is returned.
        pool: Option<ImsBufferStatistics>,
    },
    /// Basic typed observation with the original requested selector.
    StatisticsV2 {
        /// Selector associated with this observation; unsupported enhanced families reject.
        function: ImsStatisticsFunction,
        /// Published observation, or None when no observation is available.
        observation: Option<ImsStatisticsObservationV2>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
/// Subpool category used with explicit definition order.
pub enum ImsVsamSubpoolType {
    /// Data-buffer subpool identity.
    Data,
    /// Index-buffer subpool identity.
    Index,
}

/// Explicit LSR definition order; names are resource identities, never sort keys.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsVsamSubpoolMetadata {
    /// Subpool resource identity referencing a modeled VSAM buffer pool.
    pub subpool: String,
    /// LSR pool identifier used to group explicit definitions.
    pub lsr_pool: u16,
    /// Explicit order within the installed subpool definitions; names do not order selection.
    pub definition_order: u16,
    /// Data or index buffer category.
    pub subpool_type: ImsVsamSubpoolType,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Installed local directory projection; values do not assert process or IBM physical addresses.
pub struct ImsSystemDirectory {
    /// Modeled SCD address returned by the local directory route.
    pub scd_address: u32,
    /// Modeled PST address returned by the local directory route.
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Optional ordered VSAM subpool metadata; absent historical fields default to an empty list.
    pub vsam_subpools_v2: Vec<ImsVsamSubpoolMetadata>,
}

impl ImsStatisticsFunction {
    /// Reject invalid family/format/extension combinations without granting observation support.
    pub fn validate(self) -> Result<(), HostProblem> {
        if self.extended
            && !(self.family == ImsStatisticsFamily::Dbes
                && matches!(
                    self.format,
                    ImsStatisticsFormat::Full
                        | ImsStatisticsFormat::Osam
                        | ImsStatisticsFormat::Unformatted
                ))
            || self.format == ImsStatisticsFormat::Osam
                && matches!(
                    self.family,
                    ImsStatisticsFamily::Vbas | ImsStatisticsFamily::Vbes
                )
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }

    /// Conservative capacity from the specific format topics. E1 capacities
    /// and DBASO lack a consistent proven form in this bounded contract.
    pub fn minimum_io_area_bytes(self) -> Result<u32, HostProblem> {
        self.validate()?;
        if self.extended
            || self.family == ImsStatisticsFamily::Dbas && self.format == ImsStatisticsFormat::Osam
        {
            return Err(HostProblem::Unsupported);
        }
        Ok(match (self.family, self.format) {
            (ImsStatisticsFamily::Dbas | ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full) => {
                360
            }
            (ImsStatisticsFamily::Dbes | ImsStatisticsFamily::Vbes, ImsStatisticsFormat::Full) => {
                600
            }
            (
                ImsStatisticsFamily::Dbas | ImsStatisticsFamily::Vbas,
                ImsStatisticsFormat::Summary,
            ) => 180,
            (_, ImsStatisticsFormat::Summary | ImsStatisticsFormat::Osam) => 360,
            (ImsStatisticsFamily::Dbes, ImsStatisticsFormat::Unformatted) => 84,
            (ImsStatisticsFamily::Vbes, ImsStatisticsFormat::Unformatted) => 104,
            (_, ImsStatisticsFormat::Unformatted) => 72,
        })
    }
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
            ImsSystemCall::Statistics { function } => function.validate(),
            ImsSystemCall::StatisticsV2 {
                function,
                io_area_bytes,
            } => {
                let minimum = function.minimum_io_area_bytes()?;
                if *io_area_bytes < minimum || *io_area_bytes as usize > limits.max_record_bytes {
                    return Err(HostProblem::Malformed);
                }
                Ok(())
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
            Self::StatisticsV2 {
                function,
                observation,
            } => {
                function.minimum_io_area_bytes()?;
                if matches!(
                    function.family,
                    ImsStatisticsFamily::Dbes | ImsStatisticsFamily::Vbes
                ) {
                    return Err(HostProblem::Unsupported);
                }
                match observation {
                    Some(ImsStatisticsObservationV2::Subpool { statistics }) => {
                        if function.family != ImsStatisticsFamily::Vbas
                            || statistics.kind != ImsBufferPoolKind::Vsam
                        {
                            return Err(HostProblem::Malformed);
                        }
                        Self::Statistics {
                            pool: Some(statistics.clone()),
                        }
                        .validate(limits)
                    }
                    Some(ImsStatisticsObservationV2::Totals {
                        buffers,
                        storage_bytes,
                        ..
                    }) if *buffers == 0 || *storage_bytes == 0 => Err(HostProblem::Malformed),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}
