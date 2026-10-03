//! Typed operands and results for the IMS system and GSAM-adjacent call families.

use crate::{HostLimits, HostProblem, ImsCallSyntax, ImsExecutionContext};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Typed INIT/ACCEPT group identity consumed by the existing system-call contract.
pub enum ImsStatusGroup {
    /// Group A selector identity.
    A,
    /// Group B selector identity.
    B,
}

/// The comparison catalog has two INIT/ACCEPT rows with the same call spelling.
/// This identity is used only for applicability validation, never for dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsAcceptRow {
    /// Initial INIT/ACCEPT catalog row identity.
    Initial,
    /// Availability INIT/ACCEPT catalog row identity with the same call spelling.
    Availability,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
/// Local owned Q-class byte; serde consumers still validate the admitted A-through-J range.
pub struct ImsQClass(u8);

impl ImsQClass {
    #[must_use]
    /// Construct only uppercase ASCII classes A through J; otherwise return None.
    pub fn new(value: u8) -> Option<Self> {
        (b'A'..=b'J').contains(&value).then_some(Self(value))
    }

    #[must_use]
    /// Return the stored class byte without changing its identity.
    pub const fn byte(self) -> u8 {
        self.0
    }

    #[must_use]
    /// Check the admitted class range, including values obtained through deserialization.
    pub fn is_valid(self) -> bool {
        (b'A'..=b'J').contains(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Typed POS keyword identity; validation separately constrains its SSA combination.
pub enum ImsPositionKeyword {
    /// Default POS projection selector.
    Default,
    /// V5 segment-relative-byte-address keyword identity.
    V5SegmentRba,
    /// PC segment RTS keyword identity.
    PcSegmentRts,
    /// PC segment high-water-mark keyword identity.
    PcSegmentHighWaterMark,
    /// PC highest-segment TS keyword identity.
    PcHighestSegmentTs,
    /// PC logical-begin TS keyword identity.
    PcLogicalBeginTs,
    /// PC segment TS keyword identity, requiring an SSA in this validator.
    PcSegmentTs,
}

/// Exactly one POS SSA; an absent predicate is an unqualified SSA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsPositionSsa {
    /// Uppercase segment name, at most eight bytes.
    pub segment: String,
    /// Optional uppercase predicate field name; presence must match value.
    pub field: Option<String>,
    /// Optional nonempty comparative bytes bounded by max_record_bytes.
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
/// Owned system-call operands; applicability, admission and runtime observations remain separate checks.
pub enum ImsSystemCall {
    /// Request a row-qualified INIT/ACCEPT group.
    Accept {
        /// Catalog row disambiguating identical INIT/ACCEPT spellings.
        row: ImsAcceptRow,
        /// Requested typed status group.
        group: ImsStatusGroup,
    },
    /// Request availability for one numbered PCB.
    Query {
        /// Positive PCB number in the selected PSB.
        target_pcb: u16,
    },
    /// Request refreshed PCB availability through the admitted system route.
    Refresh,
    /// Request release of modeled Q reservations.
    Dequeue {
        /// Optional Q class filter; None requests the route's unfiltered release.
        class: Option<ImsQClass>,
    },
    /// Request the installed local directory projection.
    Gscd,
    /// Request the modeled DEDB area-position projection.
    Position {
        /// Optional single POS SSA constrained by the selected keyword.
        ssa: Option<ImsPositionSsa>,
        /// Typed POS selector whose combination is validated before execution.
        keyword: ImsPositionKeyword,
    },
    /// Request the legacy bounded pool-statistics projection.
    Statistics {
        /// STAT selector; support and capacity are checked separately from its identity.
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
/// Typed system-call context and syntax attached to the existing IMS request route.
pub struct ImsSystemRequest {
    /// Execution-context identity checked by the applicability owner.
    pub context: ImsExecutionContext,
    /// CALL or command form used to validate the row spelling.
    pub syntax: ImsCallSyntax,
    /// Owned operands for one system family.
    pub call: ImsSystemCall,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Numbered PCB availability observation from the local installed metadata projection.
pub struct ImsPcbAvailability {
    /// Positive PCB number in the selected PSB.
    pub pcb: u16,
    /// Exact two-byte status text.
    pub status: String,
    /// Nonempty organization label bounded by host name limits.
    pub organization: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Local DEDB area observation; opaque position bytes are not a general physical-address authority.
pub struct ImsPositionArea {
    /// Area resource identity bounded by host name limits.
    pub name: String,
    /// Cycle count followed by relative byte address in the modeled DEDB area.
    pub position: [u8; 8],
    /// Count of modeled unused SDEP control intervals.
    pub unused_sdep_cis: u32,
    /// Count of modeled unused IOV control intervals.
    pub unused_iov_cis: u32,
    /// Optional timestamp projection; absence asserts no timestamp value or clock conversion.
    pub timestamp: Option<u64>,
    /// Optional subsystem identity; absence does not synthesize an identity.
    pub ims_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Explicit local buffer geometry and published counters, without physical buffer-handler parity.
pub struct ImsBufferStatistics {
    /// Pool resource identity bounded by host name limits.
    pub pool: String,
    /// OSAM or VSAM category of this pool.
    pub kind: ImsBufferPoolKind,
    /// Positive byte capacity per modeled buffer.
    pub buffer_bytes: u32,
    /// Positive modeled buffer count.
    pub buffers: u32,
    /// Published read counter in the existing local observation contract.
    pub reads: u64,
    /// Published write counter in the existing local observation contract.
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
/// Installed modeled DEDB area geometry for bounded system observations.
pub struct ImsDedbAreaDefinition {
    /// Database resource owning the area definition.
    pub database: String,
    /// Area resource identity.
    pub name: String,
    /// Modeled SDEP capacity in control intervals.
    pub sdep_capacity_cis: u32,
    /// Modeled IOV capacity in control intervals.
    pub iov_capacity_cis: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Installed modeled buffer-pool geometry; published counters are maintained separately.
pub struct ImsBufferPoolDefinition {
    /// Pool resource identity.
    pub name: String,
    /// OSAM or VSAM pool category.
    pub kind: ImsBufferPoolKind,
    /// Modeled capacity in bytes per buffer.
    pub buffer_bytes: u32,
    /// Modeled number of buffers.
    pub buffers: u32,
}

/// Runtime resources are installed into the existing IMS provider row store.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSystemRuntimeDefinition {
    /// Optional installed local directory; absence supplies no directory values.
    pub directory: Option<ImsSystemDirectory>,
    /// Installed modeled DEDB areas, bounded by the consuming runtime.
    pub dedb_areas: Vec<ImsDedbAreaDefinition>,
    /// Installed modeled buffer-pool definitions.
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
    /// Validate operand combinations and local bounds; context admission is checked separately.
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
    /// Check result bounds and selector/observation consistency, rejecting unsupported enhanced results.
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
