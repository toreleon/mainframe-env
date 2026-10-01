use super::*;

impl Canonical for ImsExecutionContext {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let name = match self {
            Self::DbDc => "DbDc",
            Self::Dbctl => "Dbctl",
            Self::Dcctl => "Dcctl",
            Self::DbBatch => "DbBatch",
            Self::TmBatch => "TmBatch",
        };
        out.variant("ImsExecutionContext", name, 0)
    }
}

impl Canonical for ImsCallSyntax {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsCallSyntax",
            match self {
                Self::Call => "Call",
                Self::Command => "Command",
            },
            0,
        )
    }
}

impl Canonical for ImsStatusGroup {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsStatusGroup",
            match self {
                Self::A => "A",
                Self::B => "B",
            },
            0,
        )
    }
}

impl Canonical for ImsAcceptRow {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsAcceptRow",
            match self {
                Self::Initial => "Initial",
                Self::Availability => "Availability",
            },
            0,
        )
    }
}

impl Canonical for ImsQClass {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsQClass", 1)?;
        out.text("byte")?;
        self.byte().encode(out)
    }
}

impl Canonical for ImsPositionKeyword {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let name = match self {
            Self::Default => "Default",
            Self::V5SegmentRba => "V5SegmentRba",
            Self::PcSegmentRts => "PcSegmentRts",
            Self::PcSegmentHighWaterMark => "PcSegmentHighWaterMark",
            Self::PcHighestSegmentTs => "PcHighestSegmentTs",
            Self::PcLogicalBeginTs => "PcLogicalBeginTs",
            Self::PcSegmentTs => "PcSegmentTs",
        };
        out.variant("ImsPositionKeyword", name, 0)
    }
}

impl Canonical for ImsPositionSsa {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPositionSsa", 3)?;
        out.text("field")?;
        self.field.encode(out)?;
        out.text("segment")?;
        self.segment.encode(out)?;
        out.text("value")?;
        self.value.encode(out)
    }
}

impl Canonical for ImsStatisticsFamily {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsStatisticsFamily",
            match self {
                Self::Dbas => "Dbas",
                Self::Dbes => "Dbes",
                Self::Vbas => "Vbas",
                Self::Vbes => "Vbes",
            },
            0,
        )
    }
}

impl Canonical for ImsStatisticsFormat {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsStatisticsFormat",
            match self {
                Self::Full => "Full",
                Self::Osam => "Osam",
                Self::Summary => "Summary",
                Self::Unformatted => "Unformatted",
            },
            0,
        )
    }
}

impl Canonical for ImsStatisticsFunction {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsStatisticsFunction", 3)?;
        out.text("extended")?;
        self.extended.encode(out)?;
        out.text("family")?;
        self.family.encode(out)?;
        out.text("format")?;
        self.format.encode(out)
    }
}

impl Canonical for ImsSystemCall {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Accept { row, group } => {
                out.variant("ImsSystemCall", "Accept", 2)?;
                out.text("group")?;
                group.encode(out)?;
                out.text("row")?;
                row.encode(out)
            }
            Self::Query { target_pcb } => {
                out.variant("ImsSystemCall", "Query", 1)?;
                out.text("target_pcb")?;
                target_pcb.encode(out)
            }
            Self::Refresh => out.variant("ImsSystemCall", "Refresh", 0),
            Self::Dequeue { class } => {
                out.variant("ImsSystemCall", "Dequeue", 1)?;
                out.text("class")?;
                class.encode(out)
            }
            Self::Gscd => out.variant("ImsSystemCall", "Gscd", 0),
            Self::Position { ssa, keyword } => {
                out.variant("ImsSystemCall", "Position", 2)?;
                out.text("keyword")?;
                keyword.encode(out)?;
                out.text("ssa")?;
                ssa.encode(out)
            }
            Self::Statistics { function } => {
                out.variant("ImsSystemCall", "Statistics", 1)?;
                out.text("function")?;
                function.encode(out)
            }
        }
    }
}

impl Canonical for ImsSystemRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsSystemRequest", 3)?;
        out.text("call")?;
        self.call.encode(out)?;
        out.text("context")?;
        self.context.encode(out)?;
        out.text("syntax")?;
        self.syntax.encode(out)
    }
}

impl Canonical for ImsBufferPoolKind {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.variant(
            "ImsBufferPoolKind",
            match self {
                Self::Osam => "Osam",
                Self::Vsam => "Vsam",
            },
            0,
        )
    }
}

impl Canonical for ImsPcbAvailability {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPcbAvailability", 3)?;
        out.text("organization")?;
        self.organization.encode(out)?;
        out.text("pcb")?;
        self.pcb.encode(out)?;
        out.text("status")?;
        self.status.encode(out)
    }
}

impl Canonical for ImsPositionArea {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsPositionArea", 6)?;
        out.text("ims_id")?;
        self.ims_id.encode(out)?;
        out.text("name")?;
        self.name.encode(out)?;
        out.text("position")?;
        self.position.encode(out)?;
        out.text("timestamp")?;
        self.timestamp.encode(out)?;
        out.text("unused_iov_cis")?;
        self.unused_iov_cis.encode(out)?;
        out.text("unused_sdep_cis")?;
        self.unused_sdep_cis.encode(out)
    }
}

impl Canonical for ImsBufferStatistics {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsBufferStatistics", 6)?;
        out.text("buffer_bytes")?;
        self.buffer_bytes.encode(out)?;
        out.text("buffers")?;
        self.buffers.encode(out)?;
        out.text("kind")?;
        self.kind.encode(out)?;
        out.text("pool")?;
        self.pool.encode(out)?;
        out.text("reads")?;
        self.reads.encode(out)?;
        out.text("writes")?;
        self.writes.encode(out)
    }
}

impl Canonical for ImsSystemResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Accepted { group } => {
                out.variant("ImsSystemResult", "Accepted", 1)?;
                out.text("group")?;
                group.encode(out)
            }
            Self::Query { pcb } => {
                out.variant("ImsSystemResult", "Query", 1)?;
                out.text("pcb")?;
                pcb.encode(out)
            }
            Self::Refreshed { pcbs } => {
                out.variant("ImsSystemResult", "Refreshed", 1)?;
                out.text("pcbs")?;
                pcbs.encode(out)
            }
            Self::Dequeued { released } => {
                out.variant("ImsSystemResult", "Dequeued", 1)?;
                out.text("released")?;
                released.encode(out)
            }
            Self::Gscd {
                scd_address,
                pst_address,
            } => {
                out.variant("ImsSystemResult", "Gscd", 2)?;
                out.text("pst_address")?;
                pst_address.encode(out)?;
                out.text("scd_address")?;
                scd_address.encode(out)
            }
            Self::Positioned { areas } => {
                out.variant("ImsSystemResult", "Positioned", 1)?;
                out.text("areas")?;
                areas.encode(out)
            }
            Self::Statistics { pool } => {
                out.variant("ImsSystemResult", "Statistics", 1)?;
                out.text("pool")?;
                pool.encode(out)
            }
        }
    }
}
