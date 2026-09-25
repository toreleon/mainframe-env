use super::{Reader, field, read_text};
use mainframe_env_host_api::HostProblem;

const MAGIC: &[u8; 8] = b"MECWBOD1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebServerBodyCursor {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub cursor: usize,
    pub version: u64,
}

pub(in crate::service) fn encode_body_cursor(
    state: &WebServerBodyCursor,
) -> Result<Vec<u8>, HostProblem> {
    let mut out = MAGIC.to_vec();
    for text in [
        state.owner_execution.as_str(),
        state.owner_run_unit.as_str(),
        state.transaction.as_str(),
    ] {
        field(&mut out, text.as_bytes())?;
    }
    out.extend_from_slice(
        &u32::try_from(state.cursor)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    Ok(out)
}

pub(in crate::service) fn decode_body_cursor(
    bytes: &[u8],
    version: u64,
) -> Result<WebServerBodyCursor, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if version == 0 || reader.take(8)? != MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let cursor = u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ) as usize;
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebServerBodyCursor {
        owner_execution,
        owner_run_unit,
        transaction,
        cursor,
        version,
    })
}
