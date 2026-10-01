//! Typed operands and results for the IMS system and GSAM-adjacent call families.

use crate::{HostLimits, HostProblem, ImsCallSyntax, ImsExecutionContext};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsStatusGroup {
    A,
    B,
}

/// The comparison catalog has two INIT/ACCEPT rows with the same call spelling.
/// This identity is used only for applicability validation, never for dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsAcceptRow {
    Initial,
    Availability,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ImsQClass(u8);

impl ImsQClass {
    #[must_use]
    pub fn new(value: u8) -> Option<Self> {
        (b'A'..=b'J').contains(&value).then_some(Self(value))
    }

    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        (b'A'..=b'J').contains(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsPositionKeyword {
    Default,
    V5SegmentRba,
    PcSegmentRts,
    PcSegmentHighWaterMark,
    PcHighestSegmentTs,
    PcLogicalBeginTs,
    PcSegmentTs,
}

/// Exactly one POS SSA; an absent predicate is an unqualified SSA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsPositionSsa {
    pub segment: String,
    pub field: Option<String>,
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsBufferPoolKind {
    Osam,
    Vsam,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsStatisticsFamily {
    Dbas,
    Dbes,
    Vbas,
    Vbes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsStatisticsFormat {
    Full,
    Osam,
    Summary,
    Unformatted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsStatisticsFunction {
    pub family: ImsStatisticsFamily,
    pub format: ImsStatisticsFormat,
    pub extended: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsSystemCall {
    Accept {
        row: ImsAcceptRow,
        group: ImsStatusGroup,
    },
    Query {
        target_pcb: u16,
    },
    Refresh,
    Dequeue {
        class: Option<ImsQClass>,
    },
    Gscd,
    Position {
        ssa: Option<ImsPositionSsa>,
        keyword: ImsPositionKeyword,
    },
    Statistics {
        function: ImsStatisticsFunction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSystemRequest {
    pub context: ImsExecutionContext,
    pub syntax: ImsCallSyntax,
    pub call: ImsSystemCall,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsPcbAvailability {
    pub pcb: u16,
    pub status: String,
    pub organization: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsPositionArea {
    pub name: String,
    /// Cycle count followed by relative byte address in the modeled DEDB area.
    pub position: [u8; 8],
    pub unused_sdep_cis: u32,
    pub unused_iov_cis: u32,
    pub timestamp: Option<u64>,
    pub ims_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsBufferStatistics {
    pub pool: String,
    pub kind: ImsBufferPoolKind,
    pub buffer_bytes: u32,
    pub buffers: u32,
    pub reads: u64,
    pub writes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsSystemResult {
    Accepted { group: ImsStatusGroup },
    Query { pcb: ImsPcbAvailability },
    Refreshed { pcbs: Vec<ImsPcbAvailability> },
    Dequeued { released: u32 },
    Gscd { scd_address: u32, pst_address: u32 },
    Positioned { areas: Vec<ImsPositionArea> },
    Statistics { pool: Option<ImsBufferStatistics> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSystemDirectory {
    pub scd_address: u32,
    pub pst_address: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsDedbAreaDefinition {
    pub database: String,
    pub name: String,
    pub sdep_capacity_cis: u32,
    pub iov_capacity_cis: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsBufferPoolDefinition {
    pub name: String,
    pub kind: ImsBufferPoolKind,
    pub buffer_bytes: u32,
    pub buffers: u32,
}

/// Runtime resources are installed into the existing IMS provider row store.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSystemRuntimeDefinition {
    pub directory: Option<ImsSystemDirectory>,
    pub dedb_areas: Vec<ImsDedbAreaDefinition>,
    pub buffer_pools: Vec<ImsBufferPoolDefinition>,
}

impl ImsSystemRequest {
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
                if let Some(ssa) = ssa {
                    if ssa.segment.is_empty()
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
                        })
                    {
                        return Err(HostProblem::Malformed);
                    }
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
