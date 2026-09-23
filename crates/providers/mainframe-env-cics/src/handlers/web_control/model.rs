use super::super::super::{CicsLimits, Reader, field, store_error};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateStore;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) const SESSION_NAMESPACE: &str = "cics-web-session-v1";
pub(super) const URIMAP_NAMESPACE: &str = "cics-web-urimap-v1";
pub(super) const BROWSE_NAMESPACE: &str = "cics-web-browse-v1";
pub(super) const HEADER_NAMESPACE: &str = "cics-web-header-stage-v1";
pub(super) const CLIENT_RESPONSE_NAMESPACE: &str = "cics-web-client-response-v1";
pub(super) const SERVER_RESPONSE_NAMESPACE: &str = "cics-web-server-response-v1";
pub(super) const DISPATCH_NAMESPACE: &str = "cics-web-dispatch-v1";
pub(super) const BODY_CURSOR_NAMESPACE: &str = "cics-web-body-cursor-v1";
const SESSION_MAGIC: &[u8; 8] = b"MECWEB01";
const URIMAP_MAGIC: &[u8; 8] = b"MECWURI1";
const BROWSE_MAGIC: &[u8; 8] = b"MECWBR01";
const HEADER_MAGIC: &[u8; 8] = b"MECWHDR1";

mod body;
mod message;
pub(in crate::service) use body::WebServerBodyCursor;
pub(in crate::service) use body::{decode_body_cursor, encode_body_cursor};
pub use message::{CicsWebRequest, CicsWebResponse, CicsWebServerResponse};
pub(in crate::service) use message::{WebClientResponseState, WebServerReply};
pub(super) use message::{
    decode_client_response, decode_server_reply, encode_client_response, encode_server_reply,
};

/// A bounded HTTP endpoint selected by WEB OPEN, independent of the transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebEndpoint {
    /// HTTP or HTTPS, as selected by SCHEME or an installed URIMAP.
    pub scheme: String,
    /// DNS name or unbracketed IP address.
    pub host: String,
    /// Effective TCP port, including the scheme default.
    pub port: u16,
    /// Default path for subsequent client requests.
    pub default_path: String,
    /// Installed URIMAP name when the endpoint came from a resource definition.
    pub urimap: Option<String>,
    /// Selected host code page for converted response bodies.
    pub code_page: u16,
    /// TLS client certificate label, when selected.
    pub certificate: Option<String>,
}

/// HTTP protocol level observed by the selected transport during WEB OPEN.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsWebVersion {
    /// Major HTTP protocol number.
    pub major: u16,
    /// Minor HTTP protocol number.
    pub minor: u16,
}

/// One HTTP request assigned to a CICS Web-support task by the host adapter.
/// Values retain the wire spelling needed by WEB EXTRACT and header browsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebInboundRequest {
    /// Whether the listener classified this as an HTTP request.
    pub http: bool,
    /// HTTP or HTTPS listener scheme.
    pub scheme: String,
    /// Host from the absolute request URI or Host header, without a port.
    pub host: String,
    /// Effective listener port.
    pub port: u16,
    /// Original HTTP method token.
    pub method: String,
    /// Request protocol version.
    pub version: CicsWebVersion,
    /// Escaped request path, beginning with a slash.
    pub path: String,
    /// Escaped query bytes without the question mark.
    pub query: String,
    /// Matched inbound URIMAP, if any.
    pub urimap: Option<String>,
    /// Request entity bytes for WEB RECEIVE.
    pub body: Vec<u8>,
    /// Ordered HTTP headers, retaining repeated fields.
    pub headers: Vec<(String, String)>,
}

/// Transport operations needed by the CICS client session lifecycle.
/// Implementations receive the live invocation to inspect deadline and cancellation.
pub trait CicsWebTransport: Send + Sync {
    /// Establish an endpoint connection and return its observed HTTP level.
    fn open(
        &self,
        endpoint: &CicsWebEndpoint,
        invocation: &Invocation,
    ) -> Result<CicsWebVersion, HostProblem>;

    /// Release one task-owned connection, optionally returning it to a pool.
    fn release(
        &self,
        endpoint: &CicsWebEndpoint,
        token: [u8; 8],
        pooled: bool,
        invocation: &Invocation,
    ) -> Result<(), HostProblem>;

    /// Dispatch one checked client request and return the complete bounded response.
    /// A transport that has no exchange implementation fails closed.
    fn exchange(
        &self,
        _endpoint: &CicsWebEndpoint,
        _token: [u8; 8],
        _request: &CicsWebRequest,
        _invocation: &Invocation,
    ) -> Result<CicsWebResponse, HostProblem> {
        Err(HostProblem::ProviderFailure)
    }
}

/// Installed client URIMAP used by WEB OPEN.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebUriMapDefinition {
    /// Up-to-eight-character resource name.
    pub name: String,
    /// Whether USAGE(CLIENT) is enabled.
    pub enabled: bool,
    /// HTTP or HTTPS endpoint scheme.
    pub scheme: String,
    /// Host name or address without an embedded port.
    pub host: String,
    /// Explicit port, or zero for the scheme default.
    pub port: u16,
    /// Default request path.
    pub path: String,
    /// Whether WEB CLOSE may pool the connection.
    pub pooled: bool,
    /// Optional TLS client certificate label.
    pub certificate: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebClientSession {
    pub token: [u8; 8],
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub endpoint: CicsWebEndpoint,
    pub pooled: bool,
    pub http_version: CicsWebVersion,
    pub server_closed: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebBrowse {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub kind: String,
    pub client_token: Option<[u8; 8]>,
    pub entries: Vec<(Vec<u8>, Vec<u8>)>,
    pub cursor: usize,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct WebHeaderStage {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub transaction: String,
    pub client_token: Option<[u8; 8]>,
    pub headers: Vec<(String, String)>,
    pub version: u64,
}

pub(in crate::service) struct WebState {
    pub sessions: BTreeMap<String, WebClientSession>,
    pub browses: BTreeMap<String, WebBrowse>,
    pub pending_headers: BTreeMap<String, WebHeaderStage>,
    pub client_responses: BTreeMap<String, WebClientResponseState>,
    pub server_responses: BTreeMap<String, WebServerReply>,
    pub body_cursors: BTreeMap<String, WebServerBodyCursor>,
    pub inbound: BTreeMap<String, CicsWebInboundRequest>,
    pub urimaps: BTreeMap<String, CicsWebUriMapDefinition>,
    pub transport: Option<Arc<dyn CicsWebTransport>>,
    pub bytes: usize,
}

pub(in crate::service) fn load(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<WebState, HostProblem> {
    let mut state = WebState {
        sessions: BTreeMap::new(),
        browses: BTreeMap::new(),
        pending_headers: BTreeMap::new(),
        client_responses: BTreeMap::new(),
        server_responses: BTreeMap::new(),
        body_cursors: BTreeMap::new(),
        inbound: BTreeMap::new(),
        urimaps: BTreeMap::new(),
        transport: None,
        bytes: 0,
    };
    for row in store
        .list_provider_state(SESSION_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let session = decode_session(&row.payload, row.version)?;
        if row.key != token_key(session.token) || state.sessions.insert(row.key, session).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(URIMAP_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        if row.version != 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let definition = decode_urimap(&row.payload)?;
        if row.key != definition.name || state.urimaps.insert(row.key, definition).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(BROWSE_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let browse = decode_browse(&row.payload, row.version)?;
        if row.key != browse_key(&browse.owner_run_unit, &browse.kind)
            || state.browses.insert(row.key, browse).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(HEADER_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let headers = decode_header_stage(&row.payload, row.version)?;
        if row.key != header_stage_key(&headers.owner_run_unit, headers.client_token)
            || state.pending_headers.insert(row.key, headers).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(CLIENT_RESPONSE_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let response = decode_client_response(&row.payload, row.version)?;
        if row.key != token_key(response.token)
            || state.client_responses.insert(row.key, response).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(SERVER_RESPONSE_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let response = decode_server_reply(&row.payload, row.version)?;
        if row.key != response.owner_run_unit
            || state.server_responses.insert(row.key, response).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for row in store
        .list_provider_state(BODY_CURSOR_NAMESPACE, limits.max_web_sessions)
        .map_err(store_error)?
    {
        let cursor = decode_body_cursor(&row.payload, row.version)?;
        if row.key != cursor.owner_run_unit || state.body_cursors.insert(row.key, cursor).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.bytes = state
            .bytes
            .checked_add(row.payload.len())
            .filter(|total| *total <= limits.max_web_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    Ok(state)
}

pub(super) fn header_stage_key(run_unit: &str, token: Option<[u8; 8]>) -> String {
    format!(
        "{run_unit}:{}",
        token.map_or_else(|| "server".into(), token_key)
    )
}

pub(super) fn encode_header_stage(stage: &WebHeaderStage) -> Result<Vec<u8>, HostProblem> {
    let mut out = HEADER_MAGIC.to_vec();
    for text in [
        stage.owner_execution.as_str(),
        stage.owner_run_unit.as_str(),
        stage.transaction.as_str(),
    ] {
        field(&mut out, text.as_bytes())?;
    }
    out.extend_from_slice(&stage.client_token.unwrap_or([0; 8]));
    out.extend_from_slice(
        &u16::try_from(stage.headers.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &stage.headers {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value.as_bytes())?;
    }
    Ok(out)
}

fn decode_header_stage(bytes: &[u8], version: u64) -> Result<WebHeaderStage, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != HEADER_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let token: [u8; 8] = reader
        .take(8)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = usize::from(read_u16(&mut reader)?);
    if count > 128 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        headers.push((read_text(&mut reader, 128)?, read_text(&mut reader, 32000)?));
    }
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
        || headers
            .iter()
            .any(|(name, value)| name.is_empty() || value.is_empty())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebHeaderStage {
        owner_execution,
        owner_run_unit,
        transaction,
        client_token: (token != [0; 8]).then_some(token),
        headers,
        version,
    })
}

pub(super) fn browse_key(run_unit: &str, kind: &str) -> String {
    format!("{run_unit}:{kind}")
}

pub(super) fn encode_browse(browse: &WebBrowse) -> Result<Vec<u8>, HostProblem> {
    let mut out = BROWSE_MAGIC.to_vec();
    for text in [
        browse.owner_execution.as_str(),
        browse.owner_run_unit.as_str(),
        browse.transaction.as_str(),
        browse.kind.as_str(),
    ] {
        field(&mut out, text.as_bytes())?;
    }
    out.extend_from_slice(&browse.client_token.unwrap_or([0; 8]));
    out.extend_from_slice(
        &u16::try_from(browse.entries.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &browse.entries {
        field(&mut out, name)?;
        field(&mut out, value)?;
    }
    out.extend_from_slice(
        &u16::try_from(browse.cursor)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    Ok(out)
}

fn decode_browse(bytes: &[u8], version: u64) -> Result<WebBrowse, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != BROWSE_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let kind = read_text(&mut reader, 16)?;
    let token: [u8; 8] = reader
        .take(8)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = usize::from(read_u16(&mut reader)?);
    if count > 128 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push((reader.field(8192)?, reader.field(8192)?));
    }
    let cursor = usize::from(read_u16(&mut reader)?);
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
        || !matches!(kind.as_str(), "HTTPHEADER" | "FORMFIELD" | "QUERYPARM")
        || cursor > entries.len()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebBrowse {
        owner_execution,
        owner_run_unit,
        transaction,
        kind,
        client_token: (token != [0; 8]).then_some(token),
        entries,
        cursor,
        version,
    })
}

pub(super) fn token_key(token: [u8; 8]) -> String {
    let mut out = String::with_capacity(16);
    for byte in token {
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").expect("fixed hexadecimal formatting");
    }
    out
}

pub(super) fn encode_session(session: &WebClientSession) -> Result<Vec<u8>, HostProblem> {
    let mut out = SESSION_MAGIC.to_vec();
    out.extend_from_slice(&session.token);
    for value in [
        session.owner_execution.as_str(),
        session.owner_run_unit.as_str(),
        session.transaction.as_str(),
        session.endpoint.scheme.as_str(),
        session.endpoint.host.as_str(),
        session.endpoint.default_path.as_str(),
        session.endpoint.urimap.as_deref().unwrap_or(""),
        session.endpoint.certificate.as_deref().unwrap_or(""),
    ] {
        field(&mut out, value.as_bytes())?;
    }
    out.extend_from_slice(&session.endpoint.port.to_be_bytes());
    out.extend_from_slice(&session.endpoint.code_page.to_be_bytes());
    out.push(u8::from(session.pooled));
    out.extend_from_slice(&session.http_version.major.to_be_bytes());
    out.extend_from_slice(&session.http_version.minor.to_be_bytes());
    out.push(u8::from(session.server_closed));
    Ok(out)
}

pub(super) fn encode_urimap(definition: &CicsWebUriMapDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = URIMAP_MAGIC.to_vec();
    field(&mut out, definition.name.as_bytes())?;
    out.push(u8::from(definition.enabled));
    field(&mut out, definition.scheme.as_bytes())?;
    field(&mut out, definition.host.as_bytes())?;
    out.extend_from_slice(&definition.port.to_be_bytes());
    field(&mut out, definition.path.as_bytes())?;
    out.push(u8::from(definition.pooled));
    field(
        &mut out,
        definition.certificate.as_deref().unwrap_or("").as_bytes(),
    )?;
    Ok(out)
}

fn decode_urimap(bytes: &[u8]) -> Result<CicsWebUriMapDefinition, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != URIMAP_MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let name = read_text(&mut reader, 8)?;
    let enabled = read_flag(&mut reader)?;
    let scheme = read_text(&mut reader, 5)?;
    let host = read_text(&mut reader, 255)?;
    let port = read_u16(&mut reader)?;
    let path = read_text(&mut reader, 4096)?;
    let pooled = read_flag(&mut reader)?;
    let certificate = read_text(&mut reader, 32)?;
    if reader.at != bytes.len()
        || name.is_empty()
        || !matches!(scheme.as_str(), "HTTP" | "HTTPS")
        || host.is_empty()
        || !path.starts_with('/')
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsWebUriMapDefinition {
        name,
        enabled,
        scheme,
        host,
        port,
        path,
        pooled,
        certificate: (!certificate.is_empty()).then_some(certificate),
    })
}

fn decode_session(bytes: &[u8], version: u64) -> Result<WebClientSession, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(8)? != SESSION_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let token: [u8; 8] = reader
        .take(8)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 8)?;
    let scheme = read_text(&mut reader, 5)?;
    let host = read_text(&mut reader, 255)?;
    let default_path = read_text(&mut reader, 4096)?;
    let urimap = read_text(&mut reader, 8)?;
    let certificate = read_text(&mut reader, 32)?;
    let port = read_u16(&mut reader)?;
    let code_page = read_u16(&mut reader)?;
    let pooled = read_flag(&mut reader)?;
    let major = read_u16(&mut reader)?;
    let minor = read_u16(&mut reader)?;
    let server_closed = read_flag(&mut reader)?;
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
        || !matches!(scheme.as_str(), "HTTP" | "HTTPS")
        || host.is_empty()
        || port == 0
        || code_page == 0
        || !default_path.starts_with('/')
        || major == 0
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(WebClientSession {
        token,
        owner_execution,
        owner_run_unit,
        transaction,
        endpoint: CicsWebEndpoint {
            scheme,
            host,
            port,
            default_path,
            urimap: (!urimap.is_empty()).then_some(urimap),
            code_page,
            certificate: (!certificate.is_empty()).then_some(certificate),
        },
        pooled,
        http_version: CicsWebVersion { major, minor },
        server_closed,
        version,
    })
}

fn read_text(reader: &mut Reader<'_>, maximum: usize) -> Result<String, HostProblem> {
    String::from_utf8(reader.field(maximum)?).map_err(|_| HostProblem::InfrastructureFailure)
}

fn read_u16(reader: &mut Reader<'_>) -> Result<u16, HostProblem> {
    Ok(u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
}

fn read_flag(reader: &mut Reader<'_>) -> Result<bool, HostProblem> {
    match reader.take(1)?[0] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}
