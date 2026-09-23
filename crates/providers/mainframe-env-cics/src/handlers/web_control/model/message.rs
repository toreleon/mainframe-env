use super::{CicsWebVersion, Reader, field, read_flag, read_text, read_u16};
use mainframe_env_host_api::HostProblem;

const CLIENT_MAGIC: &[u8; 8] = b"MECWCLI1";
const SERVER_MAGIC: &[u8; 8] = b"MECWSRV1";
const MAX_BODY: usize = 64 * 1024 * 1024;

/// One checked outbound HTTP request, independent of the selected transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebRequest {
    /// GET, HEAD, PATCH, POST, PUT, TRACE, OPTIONS, or DELETE.
    pub method: String,
    /// Escaped path beginning with a slash.
    pub path: String,
    /// Escaped query without the question mark.
    pub query: String,
    /// Ordered application headers, including repeated names.
    pub headers: Vec<(String, String)>,
    /// Exact request entity bytes.
    pub body: Vec<u8>,
    /// Whether the request asks to close the connection after its response.
    pub close: bool,
}

/// One complete bounded HTTP response returned by a client transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebResponse {
    /// Observed HTTP protocol version.
    pub version: CicsWebVersion,
    /// HTTP status code.
    pub status: u16,
    /// Status reason phrase.
    pub reason: String,
    /// Ordered response headers, including repeated names.
    pub headers: Vec<(String, String)>,
    /// Exact response entity bytes.
    pub body: Vec<u8>,
}

/// Bounded response selected by WEB SEND in a CICS HTTP server task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebServerResponse {
    /// HTTP status code.
    pub status: u16,
    /// Status reason phrase.
    pub reason: String,
    /// Ordered response headers.
    pub headers: Vec<(String, String)>,
    /// Exact response entity bytes.
    pub body: Vec<u8>,
    /// Whether the response is deferred until task completion.
    pub eventual: bool,
    /// Whether the response asks the client to close the connection.
    pub close: bool,
    /// Document token retained for WEB RETRIEVE, when applicable.
    pub document_token: Option<[u8; 16]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebClientResponseState {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub token: [u8; 8],
    pub response: CicsWebResponse,
    pub cursor: usize,
    pub received: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebServerReply {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub response: CicsWebServerResponse,
    pub version: u64,
}

pub(in crate::service) fn encode_client_response(
    state: &WebClientResponseState,
) -> Result<Vec<u8>, HostProblem> {
    let mut out = CLIENT_MAGIC.to_vec();
    for text in [
        state.owner_execution.as_str(),
        state.owner_run_unit.as_str(),
        state.transaction.as_str(),
    ] {
        field(&mut out, text.as_bytes())?;
    }
    out.extend_from_slice(&state.token);
    out.extend_from_slice(
        &u32::try_from(state.cursor)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.push(u8::from(state.received));
    encode_response(&mut out, &state.response)?;
    Ok(out)
}

pub(in crate::service) fn decode_client_response(
    bytes: &[u8],
    version: u64,
) -> Result<WebClientResponseState, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != CLIENT_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let token: [u8; 8] = reader
        .take(8)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let cursor = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let received = read_flag(&mut reader)?;
    let response = decode_response(&mut reader)?;
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
        || cursor > response.body.len()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebClientResponseState {
        owner_execution,
        owner_run_unit,
        transaction,
        token,
        response,
        cursor,
        received,
        version,
    })
}

pub(in crate::service) fn encode_server_reply(
    state: &WebServerReply,
) -> Result<Vec<u8>, HostProblem> {
    let mut out = SERVER_MAGIC.to_vec();
    for text in [
        state.owner_execution.as_str(),
        state.owner_run_unit.as_str(),
        state.transaction.as_str(),
    ] {
        field(&mut out, text.as_bytes())?;
    }
    out.extend_from_slice(&state.response.status.to_be_bytes());
    field(&mut out, state.response.reason.as_bytes())?;
    encode_headers(&mut out, &state.response.headers)?;
    field(&mut out, &state.response.body)?;
    out.push(u8::from(state.response.eventual));
    out.push(u8::from(state.response.close));
    out.extend_from_slice(&state.response.document_token.unwrap_or([0; 16]));
    Ok(out)
}

pub(in crate::service) fn decode_server_reply(
    bytes: &[u8],
    version: u64,
) -> Result<WebServerReply, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != SERVER_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let status = read_u16(&mut reader)?;
    let reason = read_text(&mut reader, 256)?;
    let headers = decode_headers(&mut reader)?;
    let body = reader.field(MAX_BODY)?;
    let eventual = read_flag(&mut reader)?;
    let close = read_flag(&mut reader)?;
    let token: [u8; 16] = reader
        .take(16)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
        || !(100..=599).contains(&status)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebServerReply {
        owner_execution,
        owner_run_unit,
        transaction,
        response: CicsWebServerResponse {
            status,
            reason,
            headers,
            body,
            eventual,
            close,
            document_token: (token != [0; 16]).then_some(token),
        },
        version,
    })
}

fn encode_response(out: &mut Vec<u8>, response: &CicsWebResponse) -> Result<(), HostProblem> {
    out.extend_from_slice(&response.version.major.to_be_bytes());
    out.extend_from_slice(&response.version.minor.to_be_bytes());
    out.extend_from_slice(&response.status.to_be_bytes());
    field(out, response.reason.as_bytes())?;
    encode_headers(out, &response.headers)?;
    field(out, &response.body)?;
    Ok(())
}

fn decode_response(reader: &mut Reader<'_>) -> Result<CicsWebResponse, HostProblem> {
    let major = read_u16(reader)?;
    let minor = read_u16(reader)?;
    let status = read_u16(reader)?;
    let reason = read_text(reader, 256)?;
    let headers = decode_headers(reader)?;
    let body = reader.field(MAX_BODY)?;
    if major == 0 || !(100..=599).contains(&status) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsWebResponse {
        version: CicsWebVersion { major, minor },
        status,
        reason,
        headers,
        body,
    })
}

fn encode_headers(out: &mut Vec<u8>, headers: &[(String, String)]) -> Result<(), HostProblem> {
    if headers.len() > 128 {
        return Err(HostProblem::ResourceExhausted);
    }
    out.extend_from_slice(
        &u16::try_from(headers.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in headers {
        if name.is_empty() || name.len() > 128 || value.len() > 32000 {
            return Err(HostProblem::ResourceExhausted);
        }
        field(out, name.as_bytes())?;
        field(out, value.as_bytes())?;
    }
    Ok(())
}

fn decode_headers(reader: &mut Reader<'_>) -> Result<Vec<(String, String)>, HostProblem> {
    let count = usize::from(read_u16(reader)?);
    if count > 128 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        headers.push((read_text(reader, 128)?, read_text(reader, 32000)?));
    }
    if headers.iter().any(|(name, _)| name.is_empty()) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(headers)
}
