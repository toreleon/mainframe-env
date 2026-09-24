//! Immutable TCP/IP ingress and client-certificate authority for a CICS task.

use crate::service::{CicsLimits, CicsService, Run, store_error};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

const NAMESPACE: &str = "cics-tcpip-context-v1";
const SCHEMA: &str = "mainframe-env.cics-tcpip-context@1";
const MAX_CERTIFICATE_BYTES: usize = 1024 * 1024;

/// Source-named client authentication selected by TCPIPSERVICE.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CicsTcpipAuthenticate {
    /// Identity asserted by a trusted intermediary.
    Asserted,
    /// CICS selects an authentication method.
    Autoauth,
    /// Authenticated users may be registered automatically.
    Autoregister,
    /// HTTP basic authentication.
    Basicauth,
    /// Client certificate authentication.
    Certificauth,
    /// No authentication requested.
    Noauthentic,
}

/// Source-named TLS privacy requirement of TCPIPSERVICE.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CicsTcpipPrivacy {
    /// Encryption is unavailable.
    Notsupported,
    /// Encryption is required.
    Required,
    /// Encryption is supported.
    Supported,
}

/// Source-named TLS state of the accepted connection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CicsTcpipSslType {
    /// SSL/TLS secures the connection.
    Ssl,
    /// No SSL/TLS security.
    Nossl,
    /// Client certificate authentication completed.
    Clientauth,
    /// The connection uses AT-TLS awareness.
    Attlsaware,
}

/// X.509 distinguished-name fields parsed by trusted TLS ingress.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsCertificateName {
    /// Common name.
    pub common_name: Vec<u8>,
    /// Country.
    pub country: Vec<u8>,
    /// State or province.
    pub state: Vec<u8>,
    /// Locality.
    pub locality: Vec<u8>,
    /// Organization.
    pub organization: Vec<u8>,
    /// Organization unit.
    pub organization_unit: Vec<u8>,
}

/// Accepted certificate and parsed fields supplied by trusted TLS ingress.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsClientCertificate {
    /// Exact DER certificate bytes.
    pub der: Vec<u8>,
    /// Serial number assigned by the issuer.
    pub serial_number: Vec<u8>,
    /// RACF user ID associated with this certificate, if known.
    pub user_id: Option<String>,
    /// Subject attributes.
    pub owner: CicsCertificateName,
    /// Issuer attributes.
    pub issuer: CicsCertificateName,
}

/// Trusted TCP/IP connection state bound immutably to one admitted CICS run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsTcpipContext {
    /// Peer address, or none when its source is undetermined.
    pub client_address: Option<IpAddr>,
    /// Local address, or none when unavailable.
    pub server_address: Option<IpAddr>,
    /// DNS-resolved peer name, if known.
    pub client_name: Option<String>,
    /// DNS-resolved server name, if known.
    pub server_name: Option<String>,
    /// TCPIPSERVICE resource name.
    pub tcpip_service: String,
    /// Local port that accepted the connection.
    pub port: u16,
    /// Requested client authentication method.
    pub authenticate: CicsTcpipAuthenticate,
    /// Required connection privacy.
    pub privacy: CicsTcpipPrivacy,
    /// Active TLS mode.
    pub ssl_type: CicsTcpipSslType,
    /// Configured maximum HTTP server input bytes.
    pub max_data_length: u32,
    /// Accepted client certificate, if one was supplied.
    pub certificate: Option<CicsClientCertificate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkRecord {
    schema: String,
    run_unit: String,
    execution: String,
    principal: String,
    context: CicsTcpipContext,
    version: u64,
}

impl CicsService {
    /// Bind one host-observed TCP/IP context after the CICS run is admitted.
    ///
    /// Rebinding a different value is rejected, including after SQLite reopen.
    pub fn bind_tcpip_context(
        &self,
        invocation: &Invocation,
        context: CicsTcpipContext,
    ) -> Result<(), HostProblem> {
        validate_context(&context)?;
        let state = self.lock()?;
        let run = state
            .runs
            .get(&invocation.run_unit_id)
            .ok_or(HostProblem::NotFound)?;
        if run.invocation.execution_id != invocation.execution_id
            || run.invocation.principal.id() != invocation.principal.id()
        {
            return Err(HostProblem::Unauthorized);
        }
        let owner = run.clone();
        drop(state);
        if let Some(existing) = self.current_tcpip_context(&owner)? {
            return if existing == context {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let record = NetworkRecord {
            schema: SCHEMA.into(),
            run_unit: invocation.run_unit_id.as_str().into(),
            execution: invocation.execution_id.as_str().into(),
            principal: invocation.principal.id().as_str().into(),
            context,
            version: 1,
        };
        let payload = encode(&record)?;
        match self.store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: record.run_unit.clone(),
                version: 1,
                payload,
            },
            None,
        ) {
            Ok(()) => Ok(()),
            Err(StoreError::AlreadyExists | StoreError::Conflict) => {
                let existing = read(self.store.as_ref(), &record.run_unit)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if existing == record {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                }
            }
            Err(error) => Err(store_error(error)),
        }
    }

    pub(in crate::service) fn current_tcpip_context(
        &self,
        run: &Run,
    ) -> Result<Option<CicsTcpipContext>, HostProblem> {
        let Some(record) = read(self.store.as_ref(), run.invocation.run_unit_id.as_str())? else {
            return Ok(None);
        };
        if record.execution != run.invocation.execution_id.as_str()
            || record.principal != run.invocation.principal.id().as_str()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(Some(record.context))
    }
}

pub(in crate::service::handlers) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_runs.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_runs {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total = 0usize;
    for row in rows {
        total = total
            .checked_add(row.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if total > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        decode(&row)?;
    }
    Ok(())
}

pub(in crate::service::handlers) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?
    else {
        return Ok(());
    };
    let record = decode(&row)?;
    if record.execution != run.invocation.execution_id.as_str() {
        return Err(HostProblem::IdempotencyConflict);
    }
    match service
        .store
        .delete_provider_state(NAMESPACE, key, row.version)
    {
        Ok(()) | Err(StoreError::NotFound) => Ok(()),
        Err(error) => Err(store_error(error)),
    }
}

fn read(store: &dyn ProviderStateStore, key: &str) -> Result<Option<NetworkRecord>, HostProblem> {
    store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?
        .map(|row| decode(&row))
        .transpose()
}

fn encode(record: &NetworkRecord) -> Result<Vec<u8>, HostProblem> {
    validate_record(record)?;
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<NetworkRecord, HostProblem> {
    if row.namespace != NAMESPACE || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: NetworkRecord =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate_record(&record)?;
    if row.key != record.run_unit
        || row.version != record.version
        || encode(&record)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}

fn validate_record(record: &NetworkRecord) -> Result<(), HostProblem> {
    if record.schema != SCHEMA
        || record.run_unit.is_empty()
        || record.execution.is_empty()
        || record.principal.is_empty()
        || record.version != 1
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate_context(&record.context).map_err(|_| HostProblem::InfrastructureFailure)
}

fn valid_der_envelope(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0x30 {
        return false;
    }
    let (header, length): (usize, usize) = match bytes[1] {
        0..=127 => (2, usize::from(bytes[1])),
        0x81 if bytes.len() >= 3 && bytes[2] >= 128 => (3, usize::from(bytes[2])),
        0x82 if bytes.len() >= 4 && bytes[2] != 0 => {
            (4, usize::from(u16::from_be_bytes([bytes[2], bytes[3]])))
        }
        0x83 if bytes.len() >= 5 && bytes[2] != 0 => (
            5,
            usize::try_from(u32::from_be_bytes([0, bytes[2], bytes[3], bytes[4]]))
                .unwrap_or(usize::MAX),
        ),
        _ => return false,
    };
    header.checked_add(length) == Some(bytes.len())
}

fn validate_context(context: &CicsTcpipContext) -> Result<(), HostProblem> {
    let dns = |name: &str| {
        !name.is_empty()
            && name.len() <= 255
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    };
    if context.tcpip_service.is_empty()
        || context.tcpip_service.len() > 8
        || !context.tcpip_service.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
        })
        || context.port == 0
        || context.max_data_length == 0
        || context
            .client_name
            .as_deref()
            .is_some_and(|name| !dns(name))
        || context
            .server_name
            .as_deref()
            .is_some_and(|name| !dns(name))
    {
        return Err(HostProblem::Malformed);
    }
    if let Some(certificate) = &context.certificate {
        if context.ssl_type != CicsTcpipSslType::Clientauth
            || context.authenticate != CicsTcpipAuthenticate::Certificauth
            || certificate.der.len() > MAX_CERTIFICATE_BYTES
            || !valid_der_envelope(&certificate.der)
            || certificate.serial_number.is_empty()
            || certificate.serial_number.len() > 64
            || certificate.user_id.as_deref().is_some_and(|user| {
                user.is_empty()
                    || user.len() > 8
                    || !user.bytes().all(|byte| {
                        byte.is_ascii_uppercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'@' | b'#' | b'$')
                    })
            })
            || !valid_certificate_name(&certificate.owner)
            || !valid_certificate_name(&certificate.issuer)
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn valid_certificate_name(name: &CicsCertificateName) -> bool {
    [
        &name.common_name,
        &name.country,
        &name.state,
        &name.locality,
        &name.organization,
        &name.organization_unit,
    ]
    .into_iter()
    .all(|field| field.len() <= 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> CicsTcpipContext {
        CicsTcpipContext {
            client_address: Some("192.0.2.10".parse().unwrap()),
            server_address: Some("2001:db8::1".parse().unwrap()),
            client_name: None,
            server_name: None,
            tcpip_service: "HTTP0001".into(),
            port: 443,
            authenticate: CicsTcpipAuthenticate::Certificauth,
            privacy: CicsTcpipPrivacy::Required,
            ssl_type: CicsTcpipSslType::Clientauth,
            max_data_length: 65_536,
            certificate: None,
        }
    }

    #[test]
    fn malformed_network_and_certificate_envelopes_fail_before_registration() {
        let mut invalid = context();
        invalid.tcpip_service = "lowercase".into();
        assert_eq!(validate_context(&invalid), Err(HostProblem::Malformed));
        invalid = context();
        invalid.certificate = Some(CicsClientCertificate {
            der: vec![0x30, 0x80, 0, 0],
            serial_number: vec![1],
            user_id: Some("IBMUSER".into()),
            owner: CicsCertificateName {
                common_name: vec![],
                country: vec![],
                state: vec![],
                locality: vec![],
                organization: vec![],
                organization_unit: vec![],
            },
            issuer: CicsCertificateName {
                common_name: vec![],
                country: vec![],
                state: vec![],
                locality: vec![],
                organization: vec![],
                organization_unit: vec![],
            },
        });
        assert_eq!(validate_context(&invalid), Err(HostProblem::Malformed));
        invalid.certificate.as_mut().unwrap().der = vec![0x30, 0x02, 0x05, 0x00];
        invalid.ssl_type = CicsTcpipSslType::Nossl;
        assert_eq!(validate_context(&invalid), Err(HostProblem::Malformed));
    }
}
