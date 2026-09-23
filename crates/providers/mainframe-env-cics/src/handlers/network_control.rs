//! Source-bounded EXTRACT commands over trusted task TCP/IP context.

use super::network_context::{CicsCertificateName, CicsClientCertificate};
use crate::service::{CicsService, Run, bounded, decimal_payload};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::ExtractCertificate => extract_certificate(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
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
