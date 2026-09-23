use super::super::super::{CicsLimits, Reader, field, store_error};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateStore;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) const SESSION_NAMESPACE: &str = "cics-web-session-v1";
pub(super) const URIMAP_NAMESPACE: &str = "cics-web-urimap-v1";
const SESSION_MAGIC: &[u8; 8] = b"MECWEB01";
const URIMAP_MAGIC: &[u8; 8] = b"MECWURI1";

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
    /// Request entity bytes for WEB RECEIVE and WEB RETRIEVE.
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

pub(in crate::service) struct WebState {
    pub sessions: BTreeMap<String, WebClientSession>,
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
    Ok(state)
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
