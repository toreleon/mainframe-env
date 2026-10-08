//! Additive browse encoding; existing browse field order and bytes are retained.

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
        DatasetRequest::StartBrowse {
            dataset,
            key,
            relation,
        } => {
            out.variant("DatasetRequest", "StartBrowse", 3)?;
            out.text("dataset")?;
            dataset.encode(out)?;
            out.text("key")?;
            key.encode(out)?;
            out.text("relation")?;
            relation.encode(out)
        }
        DatasetRequest::ReadNext {
            control,
            cursor,
            dataset,
            reverse,
        } => {
            out.variant("DatasetRequest", "ReadNext", 4)?;
            out.text("control")?;
            control.encode(out)?;
            out.text("cursor")?;
            cursor.encode(out)?;
            out.text("dataset")?;
            dataset.encode(out)?;
            out.text("reverse")?;
            reverse.encode(out)?;
            Ok(())
        }
        DatasetRequest::EndBrowse { cursor, dataset } => {
            out.variant("DatasetRequest", "EndBrowse", 2)?;
            out.text("cursor")?;
            cursor.encode(out)?;
            out.text("dataset")?;
            dataset.encode(out)?;
            Ok(())
        }
        DatasetRequest::ReadBrowsePosition {
            cursor,
            dataset,
            expected_key,
        } => {
            out.variant("DatasetRequest", "ReadBrowsePosition", 3)?;
            out.text("cursor")?;
            cursor.encode(out)?;
            out.text("dataset")?;
            dataset.encode(out)?;
            out.text("expected_key")?;
            expected_key.encode(out)
        }
        _ => unreachable!("browse canonical helper receives only browse requests"),
    }
}
