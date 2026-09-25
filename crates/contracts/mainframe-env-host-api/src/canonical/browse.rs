//! Canonical RESETBR request encoding outside the frozen exhaustive schema.

use super::*;

pub(super) fn encode_browse_request(
    request: &DatasetRequest,
    out: &mut Encoder<'_>,
) -> Result<(), HostProblem> {
    match request {
        DatasetRequest::ResetBrowse {
            cursor,
            dataset,
            key,
            relation,
        } => {
            out.variant("DatasetRequest", "ResetBrowse", 4)?;
            out.text("cursor")?;
            cursor.encode(out)?;
            out.text("dataset")?;
            dataset.encode(out)?;
            out.text("key")?;
            key.encode(out)?;
            out.text("relation")?;
            relation.encode(out)
        }
        _ => unreachable!("browse canonical helper is called only for RESETBR"),
    }
}
