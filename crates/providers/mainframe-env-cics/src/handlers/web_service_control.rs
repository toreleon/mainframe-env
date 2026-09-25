use super::super::{
    CicsEffectReplay, CicsLimits, CicsService, Run, bounded, cics_effect_replay_binding_digest,
    decimal_payload, encode_cics_effect_replay,
};
use super::{field, store_error};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::BoundedPayload;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use std::collections::BTreeMap;

mod invoke;
mod soap_fault;
mod state;
mod wsa;
pub use invoke::CicsWebServiceDefinition;
use state::WebState;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    match request.operation {
        CicsOperation::SoapFaultCreate
        | CicsOperation::SoapFaultAdd
        | CicsOperation::SoapFaultDelete => {
            soap_fault::invoke(service, run, request, retention_tick)
        }
        CicsOperation::WsaContextBuild
        | CicsOperation::WsaContextDelete
        | CicsOperation::WsaContextGet
        | CicsOperation::WsaEprCreate => wsa::invoke(service, run, request, retention_tick),
        CicsOperation::InvokeService => invoke::invoke(service, run, request, retention_tick),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(in crate::service) fn usage(service: &CicsService) -> Result<(usize, usize), HostProblem> {
    state::usage(service)
}

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    state::release_task(service, run)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed: &[&str] = match request.operation {
        CicsOperation::InvokeService => &[
            "SERVICE",
            "OPERATION",
            "CHANNEL",
            "URI",
            "URIMAP",
            "SCOPE",
            "SCOPELEN",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::SoapFaultCreate => &[
            "FAULTCODE",
            "FAULTCODESTR",
            "FAULTCODELEN",
            "FAULTSTRING",
            "FAULTSTRLEN",
            "NATLANG",
            "ROLE",
            "ROLELENGTH",
            "FAULTACTOR",
            "FAULTACTLEN",
            "DETAIL",
            "DETAILLENGTH",
            "FROMCCSID",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::SoapFaultAdd => &[
            "FAULTSTRING",
            "FAULTSTRLEN",
            "NATLANG",
            "SUBCODESTR",
            "SUBCODELEN",
            "FROMCCSID",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::SoapFaultDelete => &["RESP", "RESP2", "OPTION.NOHANDLE"],
        CicsOperation::WsaContextBuild => &[
            "CHANNEL",
            "ACTION",
            "MESSAGEID",
            "RELATESURI",
            "RELATESTYPE",
            "EPRTYPE",
            "EPRFIELD",
            "EPRFROM",
            "EPRLENGTH",
            "FROMCCSID",
            "FROMCODEPAGE",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::WsaContextDelete => &["CHANNEL", "RESP", "RESP2", "OPTION.NOHANDLE"],
        CicsOperation::WsaContextGet => &[
            "CHANNEL",
            "CONTEXTTYPE",
            "ACTION",
            "MESSAGEID",
            "RELATESURI",
            "RELATESTYPE",
            "RELATESINDEX",
            "EPRTYPE",
            "EPRFIELD",
            "EPRINTO",
            "EPRSET",
            "EPRLENGTH",
            "INTOCCSID",
            "INTOCODEPAGE",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
            "EPRINTO.MAXLENGTH",
            "EPRSET.MAXLENGTH",
        ],
        CicsOperation::WsaEprCreate => &[
            "ADDRESS",
            "REFPARMS",
            "REFPARMSLEN",
            "METADATA",
            "METADATALEN",
            "EPRINTO",
            "EPRSET",
            "EPRLENGTH",
            "FROMCCSID",
            "FROMCODEPAGE",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
            "EPRINTO.MAXLENGTH",
            "EPRSET.MAXLENGTH",
        ],
        _ => return Err(HostProblem::Malformed),
    };
    let has = |name: &str| request.arguments.contains_key(name);
    if has("RESP2") && !has("RESP")
        || request
            .arguments
            .keys()
            .any(|name| !allowed.contains(&name.as_str()))
        || request.arguments.iter().any(|(name, value)| {
            let output = matches!(name.as_str(), "RESP" | "RESP2" | "EPRINTO" | "EPRSET")
                || request.operation == CicsOperation::WsaContextGet
                    && matches!(
                        name.as_str(),
                        "ACTION" | "MESSAGEID" | "RELATESURI" | "RELATESTYPE"
                    );
            let decimal = matches!(
                name.as_str(),
                "EPRLENGTH"
                    | "FAULTCODELEN"
                    | "FAULTSTRLEN"
                    | "ROLELENGTH"
                    | "FAULTACTLEN"
                    | "DETAILLENGTH"
                    | "SUBCODELEN"
                    | "FROMCCSID"
                    | "INTOCCSID"
                    | "REFPARMSLEN"
                    | "METADATALEN"
                    | "RELATESINDEX"
                    | "SCOPELEN"
                    | "EPRINTO.MAXLENGTH"
                    | "EPRSET.MAXLENGTH"
            );
            if name.starts_with("OPTION.") {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else if output {
                value.schema() != "mainframe-env.cics.argument@1"
            } else if decimal {
                value.schema() != "mainframe-env.cics.decimal@1"
            } else if matches!(
                name.as_str(),
                "FAULTCODE" | "EPRTYPE" | "EPRFIELD" | "CONTEXTTYPE"
            ) {
                value.schema() != "mainframe-env.cics.literal@1"
            } else {
                !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                )
            }
        })
        || match request.operation {
            CicsOperation::InvokeService => !has("SERVICE") || has("URI") && has("URIMAP"),
            CicsOperation::SoapFaultCreate => {
                !has("FAULTSTRING")
                    || !has("FAULTSTRLEN")
                    || has("FAULTCODE") == has("FAULTCODESTR")
            }
            CicsOperation::SoapFaultAdd => !has("FAULTSTRING") && !has("SUBCODESTR"),
            CicsOperation::WsaContextBuild => {
                !["ACTION", "MESSAGEID", "RELATESURI", "EPRFROM"]
                    .iter()
                    .any(|name| has(name))
                    || has("RELATESTYPE") && !has("RELATESURI")
            }
            CicsOperation::WsaContextGet => ![
                "ACTION",
                "MESSAGEID",
                "RELATESURI",
                "RELATESTYPE",
                "EPRINTO",
                "EPRSET",
            ]
            .iter()
            .any(|name| has(name)),
            CicsOperation::WsaEprCreate => !has("ADDRESS") || has("EPRINTO") == has("EPRSET"),
            _ => false,
        }
        || [
            ("FAULTCODESTR", "FAULTCODELEN"),
            ("FAULTSTRING", "FAULTSTRLEN"),
            ("ROLE", "ROLELENGTH"),
            ("FAULTACTOR", "FAULTACTLEN"),
            ("DETAIL", "DETAILLENGTH"),
            ("SUBCODESTR", "SUBCODELEN"),
            ("REFPARMS", "REFPARMSLEN"),
            ("METADATA", "METADATALEN"),
        ]
        .iter()
        .any(|(data, length)| allowed.contains(data) && has(data) != has(length))
        || matches!(
            request.operation,
            CicsOperation::WsaContextGet | CicsOperation::WsaEprCreate
        ) && (has("EPRINTO") || has("EPRSET"))
            && !has("EPRLENGTH")
        || has("FROMCCSID") && has("FROMCODEPAGE")
        || has("INTOCCSID") && has("INTOCODEPAGE")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn normal(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn value<'a>(request: &'a CicsRequest, name: &str) -> Option<&'a [u8]> {
    request.arguments.get(name).map(BoundedPayload::bytes)
}

fn text(request: &CicsRequest, name: &str, max: usize) -> Result<Option<String>, HostProblem> {
    value(request, name)
        .map(|bytes| {
            if bytes.len() > max {
                return Err(HostProblem::Malformed);
            }
            String::from_utf8(bytes.to_vec()).map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn number(request: &CicsRequest, name: &str) -> Result<Option<i64>, HostProblem> {
    text(request, name, 20)?
        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
        .transpose()
}

fn channel(service: &CicsService, run: &Run, request: &CicsRequest) -> Result<String, HostProblem> {
    let current = run
        .invocation
        .bindings
        .get("cics.channel")
        .filter(|value| value.schema() == "mainframe-env.cics.channel@1")
        .and_then(|value| String::from_utf8(value.bytes().to_vec()).ok());
    let name = text(request, "CHANNEL", 16)?
        .or(current.clone())
        .ok_or_else(|| condition("INVREQ", 16, 4))?;
    let name = name.trim_end_matches(' ').to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'$' | b'@'
                        | b'#'
                        | b'/'
                        | b'%'
                        | b'&'
                        | b'?'
                        | b'!'
                        | b':'
                        | b'|'
                        | b'"'
                        | b'='
                        | b','
                        | b';'
                        | b'<'
                        | b'>'
                        | b'.'
                        | b'-'
                        | b'_'
                )
        })
    {
        return Err(condition("CHANNELERR", 122, 1));
    }
    if current
        .as_deref()
        .is_some_and(|value| value.trim_end_matches(' ').eq_ignore_ascii_case(&name))
        || name == "DFHTRANSACTION"
    {
        return Ok(name);
    }
    if service
        .lock()?
        .transform_containers
        .keys()
        .any(|(known, _)| known == &name)
    {
        Ok(name)
    } else {
        Err(condition("CHANNELERR", 122, 2))
    }
}

fn authorize_channel(
    service: &CicsService,
    run: &mut Run,
    channel: &str,
    intent: AccessIntent,
) -> Result<(), HostProblem> {
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.WEB.CHANNEL.{channel}"),
        intent,
    )
}

fn web_role(run: &Run) -> Result<&'static str, HostProblem> {
    match run.invocation.bindings.get("cics.web.role") {
        Some(value) if value.schema() == "mainframe-env.cics.web-role@1" => match value.bytes() {
            b"requester" => Ok("requester"),
            b"provider" => Ok("provider"),
            _ => Err(HostProblem::Unauthorized),
        },
        _ => Err(HostProblem::Unauthorized),
    }
}

fn utf8_data(
    request: &CicsRequest,
    name: &str,
    max: usize,
    length: Option<&str>,
    response2: i32,
) -> Result<Option<String>, HostProblem> {
    let Some(raw) = value(request, name) else {
        return Ok(None);
    };
    let n = match length {
        Some(length) => number(request, length)?.ok_or(HostProblem::Malformed)?,
        None => i64::try_from(raw.len()).map_err(|_| HostProblem::ResourceExhausted)?,
    };
    if n < 0
        || usize::try_from(n)
            .ok()
            .is_none_or(|n| n > raw.len() || n > max)
    {
        return Err(condition("LENGERR", 22, response2));
    }
    let bytes = &raw[..n as usize];
    let result = match source_encoding(request)? {
        Some(page) => page
            .decode(bytes, max.saturating_mul(2))
            .map_err(|_| condition("CCSIDERR", 123, 6))?,
        None => String::from_utf8(bytes.to_vec()).map_err(|_| condition("CCSIDERR", 123, 6))?,
    };
    Ok(Some(result))
}

fn source_encoding(request: &CicsRequest) -> Result<Option<CodePage>, HostProblem> {
    if value(request, "FROMCCSID").is_some() && value(request, "FROMCODEPAGE").is_some() {
        return Err(HostProblem::Malformed);
    }
    let code = if let Some(ccsid) = number(request, "FROMCCSID")? {
        ccsid
    } else if let Some(codepage) = text(request, "FROMCODEPAGE", 40)? {
        match codepage.trim().to_ascii_uppercase().as_str() {
            "UTF-8" | "1208" => 1208,
            "IBM-037" | "IBM037" | "CP037" | "37" => 37,
            _ => return Err(condition("CODEPAGEERR", 125, 1)),
        }
    } else {
        1208
    };
    let soap = matches!(
        request.operation,
        CicsOperation::SoapFaultCreate | CicsOperation::SoapFaultAdd
    );
    if code <= 0 || code > 65_535 {
        return Err(condition("CCSIDERR", 123, if soap { 13 } else { 1 }));
    }
    match code {
        1208 => Ok(None),
        37 => Ok(Some(CodePage::Cp037)),
        _ => Err(condition("CCSIDERR", 123, if soap { 14 } else { 2 })),
    }
}

fn from_ccsid(request: &CicsRequest) -> Result<(), HostProblem> {
    source_encoding(request).map(|_| ())
}
