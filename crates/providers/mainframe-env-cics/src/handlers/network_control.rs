//! Source-bounded EXTRACT commands over trusted task TCP/IP context.

use super::network_context::{
    CicsCertificateName, CicsClientCertificate, CicsTcpipAuthenticate, CicsTcpipPrivacy,
    CicsTcpipSslType,
};
use crate::service::{CicsService, Run, bounded, decimal_payload};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use std::net::IpAddr;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::ExtractCertificate => extract_certificate(service, run, request),
        CicsOperation::ExtractTcpip => extract_tcpip(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn extract_tcpip(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_tcpip_request(request)?;
    let context = service
        .current_tcpip_context(run)?
        .ok_or_else(|| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 5,
        })?;
    let client_address = context
        .client_address
        .map_or_else(|| "0.0.0.0".into(), |ip| ip.to_string());
    let server_address = context
        .server_address
        .map_or_else(|| "0.0.0.0".into(), |ip| ip.to_string());
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    let mut length_error = None;
    for (buffer, length, text, response2) in [
        ("CLIENTADDR", "CADDRLENGTH", client_address.as_str(), 3),
        ("SERVERADDR", "SADDRLENGTH", server_address.as_str(), 4),
        (
            "CLIENTNAME",
            "CNAMELENGTH",
            context.client_name.as_deref().unwrap_or(""),
            6,
        ),
        (
            "SERVERNAME",
            "SNAMELENGTH",
            context.server_name.as_deref().unwrap_or(""),
            7,
        ),
    ] {
        if !request.arguments.contains_key(buffer) {
            continue;
        }
        let capacity = tcpip_capacity(request, length, buffer)?;
        if capacity == 0 {
            length_error.get_or_insert(1);
            result.outputs.insert(length.into(), decimal_payload(0)?);
            continue;
        }
        let bytes = text.as_bytes();
        let copied = capacity.min(bytes.len());
        result
            .outputs
            .insert(buffer.into(), bounded(bytes[..copied].to_vec())?);
        result
            .outputs
            .insert(length.into(), decimal_payload(copied as i64)?);
        let ipv6_needs_39 = match buffer {
            "CLIENTADDR" => matches!(context.client_address, Some(IpAddr::V6(_))) && capacity < 39,
            "SERVERADDR" => matches!(context.server_address, Some(IpAddr::V6(_))) && capacity < 39,
            _ => false,
        };
        if copied < bytes.len() || ipv6_needs_39 {
            length_error.get_or_insert(response2);
        }
    }
    for name in request.arguments.keys() {
        let bytes = match name.as_str() {
            "CLIENTADDRNU" => Some(ipv4_bytes(context.client_address).to_vec()),
            "SERVERADDRNU" => Some(ipv4_bytes(context.server_address).to_vec()),
            "CLNTADDR6NU" => Some(ipv6_bytes(context.client_address).to_vec()),
            "SRVRADDR6NU" => Some(ipv6_bytes(context.server_address).to_vec()),
            "TCPIPSERVICE" => {
                let mut bytes = context.tcpip_service.as_bytes().to_vec();
                bytes.resize(8, b' ');
                Some(bytes)
            }
            "PORTNUMBER" => Some(format!("{:05}", context.port).into_bytes()),
            "PORTNUMNU" => {
                result
                    .outputs
                    .insert(name.clone(), decimal_payload(i64::from(context.port))?);
                None
            }
            "MAXDATALEN" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(i64::from(context.max_data_length))?,
                );
                None
            }
            "AUTHENTICATE" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(authenticate_cvda(context.authenticate))?,
                );
                None
            }
            "CLNTIPFAMILY" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(ip_family_cvda(context.client_address))?,
                );
                None
            }
            "SRVRIPFAMILY" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(ip_family_cvda(context.server_address))?,
                );
                None
            }
            "SSLTYPE" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(ssl_type_cvda(context.ssl_type))?,
                );
                None
            }
            "PRIVACY" => {
                result.outputs.insert(
                    name.clone(),
                    decimal_payload(privacy_cvda(context.privacy))?,
                );
                None
            }
            _ => None,
        };
        if let Some(bytes) = bytes {
            result.outputs.insert(name.clone(), bounded(bytes)?);
        }
    }
    if let Some(response2) = length_error {
        let mut condition = super::condition(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2,
            },
        )?;
        condition.outputs = result.outputs;
        Ok(condition)
    } else {
        Ok(result)
    }
}

// Exact CICS TS 6.x numeric CVDAs from pinned dfha80c.html.
fn authenticate_cvda(value: CicsTcpipAuthenticate) -> i64 {
    match value {
        CicsTcpipAuthenticate::Asserted => 1104,
        CicsTcpipAuthenticate::Autoauth => 1095,
        CicsTcpipAuthenticate::Autoregister => 1094,
        CicsTcpipAuthenticate::Basicauth => 1092,
        CicsTcpipAuthenticate::Certificauth => 1093,
        CicsTcpipAuthenticate::Noauthentic => 1091,
    }
}

fn ip_family_cvda(address: Option<IpAddr>) -> i64 {
    match address {
        Some(IpAddr::V4(_)) => 300,
        Some(IpAddr::V6(_)) => 301,
        None => 1,
    }
}

fn ssl_type_cvda(value: CicsTcpipSslType) -> i64 {
    match value {
        CicsTcpipSslType::Ssl => 1030,
        CicsTcpipSslType::Nossl => 1031,
        CicsTcpipSslType::Clientauth => 1032,
        CicsTcpipSslType::Attlsaware => 1205,
    }
}

fn privacy_cvda(value: CicsTcpipPrivacy) -> i64 {
    match value {
        CicsTcpipPrivacy::Notsupported => 15,
        CicsTcpipPrivacy::Required => 666,
        CicsTcpipPrivacy::Supported => 1106,
    }
}

fn tcpip_capacity(request: &CicsRequest, length: &str, buffer: &str) -> Result<usize, HostProblem> {
    let declared = request
        .arguments
        .get(length)
        .ok_or(HostProblem::Malformed)?;
    let declared = std::str::from_utf8(declared.bytes()).map_err(|_| HostProblem::Malformed)?;
    let declared = declared
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    let maximum = request
        .arguments
        .get(&format!("{buffer}.MAXLENGTH"))
        .ok_or(HostProblem::Malformed)?;
    let maximum = std::str::from_utf8(maximum.bytes()).map_err(|_| HostProblem::Malformed)?;
    let maximum = maximum
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if declared <= 0 {
        return Ok(0);
    }
    Ok(usize::try_from(declared)
        .map_err(|_| HostProblem::Malformed)?
        .min(maximum))
}

fn ipv4_bytes(ip: Option<IpAddr>) -> [u8; 4] {
    match ip {
        Some(IpAddr::V4(address)) => address.octets(),
        _ => [0; 4],
    }
}

fn ipv6_bytes(ip: Option<IpAddr>) -> [u8; 16] {
    match ip {
        Some(IpAddr::V6(address)) => address.octets(),
        _ => [0; 16],
    }
}

fn validate_tcpip_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const OUTPUTS: &[&str] = &[
        "CLIENTNAME",
        "CNAMELENGTH",
        "SERVERNAME",
        "SNAMELENGTH",
        "CLIENTADDR",
        "CADDRLENGTH",
        "CLIENTADDRNU",
        "CLNTADDR6NU",
        "SERVERADDR",
        "SADDRLENGTH",
        "SERVERADDRNU",
        "SRVRADDR6NU",
        "TCPIPSERVICE",
        "PORTNUMBER",
        "PORTNUMNU",
        "MAXDATALEN",
        "AUTHENTICATE",
        "CLNTIPFAMILY",
        "SRVRIPFAMILY",
        "SSLTYPE",
        "PRIVACY",
    ];
    const BUFFERS: &[&str] = &["CLIENTNAME", "SERVERNAME", "CLIENTADDR", "SERVERADDR"];
    let pairs = [
        ("CLIENTNAME", "CNAMELENGTH"),
        ("SERVERNAME", "SNAMELENGTH"),
        ("CLIENTADDR", "CADDRLENGTH"),
        ("SERVERADDR", "SADDRLENGTH"),
    ];
    let present = request
        .arguments
        .keys()
        .any(|name| OUTPUTS.contains(&name.as_str()));
    if request.operation != CicsOperation::ExtractTcpip
        || !present
        || pairs.iter().any(|(buffer, length)| {
            let selected = request.arguments.contains_key(*buffer);
            selected != request.arguments.contains_key(*length)
                || selected
                    != request
                        .arguments
                        .contains_key(&format!("{buffer}.MAXLENGTH"))
        })
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            if OUTPUTS.contains(&name.as_str()) {
                value.schema()
                    != if matches!(
                        name.as_str(),
                        "CNAMELENGTH" | "SNAMELENGTH" | "CADDRLENGTH" | "SADDRLENGTH"
                    ) {
                        "mainframe-env.cics.decimal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    }
            } else if name.ends_with(".MAXLENGTH")
                && BUFFERS
                    .iter()
                    .any(|buffer| name == &format!("{buffer}.MAXLENGTH"))
            {
                value.schema() != "mainframe-env.cics.decimal@1"
            } else if matches!(name.as_str(), "RESP" | "RESP2") {
                value.schema() != "mainframe-env.cics.argument@1"
            } else if name == "OPTION.NOHANDLE" {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else {
                true
            }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn extract_certificate(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_certificate_request(request)?;
    let context = service
        .current_tcpip_context(run)?
        .ok_or_else(|| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 5,
        })?;
    let certificate = context.certificate.as_ref();
    let selected = certificate.map(|certificate| {
        if request.arguments.contains_key("OPTION.ISSUER") {
            &certificate.issuer
        } else {
            &certificate.owner
        }
    });
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for name in request.arguments.keys() {
        let value = match name.as_str() {
            "CERTIFICATE" | "SERIALNUM" | "COMMONNAME" | "COUNTRY" | "STATE" | "LOCALITY"
            | "ORGANIZATION" | "ORGUNIT" => pointer_payload(name, certificate, selected)?,
            "LENGTH" | "SERIALNUMLEN" | "COMMONNAMLEN" | "COUNTRYLEN" | "STATELEN"
            | "LOCALITYLEN" | "ORGANIZATLEN" | "ORGUNITLEN" => {
                let bytes = match name.as_str() {
                    "LENGTH" => certificate.map(|value| value.der.as_slice()),
                    "SERIALNUMLEN" => certificate.map(|value| value.serial_number.as_slice()),
                    "COMMONNAMLEN" => selected.map(|value| value.common_name.as_slice()),
                    "COUNTRYLEN" => selected.map(|value| value.country.as_slice()),
                    "STATELEN" => selected.map(|value| value.state.as_slice()),
                    "LOCALITYLEN" => selected.map(|value| value.locality.as_slice()),
                    "ORGANIZATLEN" => selected.map(|value| value.organization.as_slice()),
                    "ORGUNITLEN" => selected.map(|value| value.organization_unit.as_slice()),
                    _ => unreachable!(),
                };
                decimal_payload(
                    i64::try_from(bytes.map_or(0, <[u8]>::len))
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                )?
            }
            "USERID" => {
                let mut user = certificate
                    .and_then(|value| value.user_id.as_deref())
                    .unwrap_or("")
                    .as_bytes()
                    .to_vec();
                user.resize(8, b' ');
                bounded(user)?
            }
            _ => continue,
        };
        response.outputs.insert(name.clone(), value);
    }
    Ok(response)
}

fn pointer_payload(
    name: &str,
    certificate: Option<&CicsClientCertificate>,
    selected: Option<&CicsCertificateName>,
) -> Result<BoundedPayload, HostProblem> {
    let bytes = match name {
        "CERTIFICATE" => certificate.map(|value| value.der.as_slice()),
        "SERIALNUM" => certificate.map(|value| value.serial_number.as_slice()),
        "COMMONNAME" => selected.map(|value| value.common_name.as_slice()),
        "COUNTRY" => selected.map(|value| value.country.as_slice()),
        "STATE" => selected.map(|value| value.state.as_slice()),
        "LOCALITY" => selected.map(|value| value.locality.as_slice()),
        "ORGANIZATION" => selected.map(|value| value.organization.as_slice()),
        "ORGUNIT" => selected.map(|value| value.organization_unit.as_slice()),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    match bytes.filter(|bytes| !bytes.is_empty()) {
        Some(bytes) => bounded(bytes.to_vec()),
        None => BoundedPayload::new(
            "mainframe-env.cics.pointer-null@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted),
    }
}

fn validate_certificate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const RESULTS: &[&str] = &[
        "CERTIFICATE",
        "LENGTH",
        "SERIALNUM",
        "SERIALNUMLEN",
        "USERID",
        "COMMONNAME",
        "COMMONNAMLEN",
        "COUNTRY",
        "COUNTRYLEN",
        "STATE",
        "STATELEN",
        "LOCALITY",
        "LOCALITYLEN",
        "ORGANIZATION",
        "ORGANIZATLEN",
        "ORGUNIT",
        "ORGUNITLEN",
    ];
    const FLAGS: &[&str] = &["OPTION.OWNER", "OPTION.ISSUER", "OPTION.NOHANDLE"];
    if request.operation != CicsOperation::ExtractCertificate
        || !request.arguments.contains_key("CERTIFICATE")
        || request.arguments.contains_key("OPTION.OWNER")
            && request.arguments.contains_key("OPTION.ISSUER")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            if RESULTS.contains(&name.as_str()) || matches!(name.as_str(), "RESP" | "RESP2") {
                value.schema() != "mainframe-env.cics.argument@1"
            } else if FLAGS.contains(&name.as_str()) {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else {
                true
            }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_numeric_cvda_values_cover_every_tcpip_context_variant() {
        // CICS TS 6.x dfha80c.html, pinned as cics-misc-tail-cvda.
        assert_eq!(
            [
                CicsTcpipAuthenticate::Asserted,
                CicsTcpipAuthenticate::Autoauth,
                CicsTcpipAuthenticate::Autoregister,
                CicsTcpipAuthenticate::Basicauth,
                CicsTcpipAuthenticate::Certificauth,
                CicsTcpipAuthenticate::Noauthentic,
            ]
            .map(authenticate_cvda),
            [1104, 1095, 1094, 1092, 1093, 1091]
        );
        assert_eq!(
            [
                CicsTcpipPrivacy::Notsupported,
                CicsTcpipPrivacy::Required,
                CicsTcpipPrivacy::Supported
            ]
            .map(privacy_cvda),
            [15, 666, 1106]
        );
        assert_eq!(
            [
                CicsTcpipSslType::Ssl,
                CicsTcpipSslType::Nossl,
                CicsTcpipSslType::Clientauth,
                CicsTcpipSslType::Attlsaware
            ]
            .map(ssl_type_cvda),
            [1030, 1031, 1032, 1205]
        );
        assert_eq!(ip_family_cvda(None), 1);
        assert_eq!(ip_family_cvda(Some("192.0.2.10".parse().unwrap())), 300);
        assert_eq!(ip_family_cvda(Some("2001:db8::1".parse().unwrap())), 301);
    }
}
