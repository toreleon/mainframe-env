//! Append-only read-completeness observations; complete values retain their v1 bytes.
use super::*;

pub(super) fn records(
    out: &mut Encoder<'_>,
    identities: &[Vec<u8>],
    records: &[Vec<u8>],
    version: u64,
    more: bool,
) -> Result<(), HostProblem> {
    out.variant(
        "DatasetResult",
        if more { "RecordsTruncated" } else { "Records" },
        if more { 4 } else { 3 },
    )?;
    out.text("identities")?;
    identities.encode(out)?;
    out.text("records")?;
    records.encode(out)?;
    out.text("version")?;
    version.encode(out)?;
    if more {
        out.text("more")?;
        more.encode(out)?;
    }
    Ok(())
}

pub(super) fn member_generation(
    out: &mut Encoder<'_>,
    generation: u64,
    identities: &[Vec<u8>],
    program_object: bool,
    records: &[Vec<u8>],
    version: u64,
    more: bool,
) -> Result<(), HostProblem> {
    out.variant(
        "DatasetResult",
        if more {
            "MemberGenerationTruncated"
        } else {
            "MemberGeneration"
        },
        if more { 6 } else { 5 },
    )?;
    out.text("generation")?;
    generation.encode(out)?;
    out.text("identities")?;
    identities.encode(out)?;
    out.text("program_object")?;
    program_object.encode(out)?;
    out.text("records")?;
    records.encode(out)?;
    out.text("version")?;
    version.encode(out)?;
    if more {
        out.text("more")?;
        more.encode(out)?;
    }
    Ok(())
}
