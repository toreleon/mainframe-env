use super::super::super::{CicsService, Run};
use super::super::super::{bounded, decimal_payload};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use std::net::{Ipv4Addr, Ipv6Addr};

const MAX_URL_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 4096;

struct ParsedUrl<'a> {
    scheme: &'static str,
    host: &'a str,
    host_type: i64,
    port: u16,
    path: &'a str,
    query: &'a str,
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let url = request.arguments["URL"].bytes();
    let length = decimal(request, "URLLENGTH")?;
    let length = usize::try_from(length).map_err(|_| invalid_url())?;
    if length == 0 || length > url.len() || length > MAX_URL_BYTES {
        return Err(invalid_url());
    }
    let url = std::str::from_utf8(&url[..length]).map_err(|_| invalid_url())?;
    let parsed = parse(url)?;
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
    for (name, value) in [
        ("SCHEMENAME", parsed.scheme.as_bytes()),
        ("HOST", parsed.host.as_bytes()),
        ("PATH", parsed.path.as_bytes()),
        ("QUERYSTRING", parsed.query.as_bytes()),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(value.to_vec())?);
        }
    }
    for (name, value) in [
        ("HOSTTYPE", parsed.host_type),
        ("PORTNUMBER", i64::from(parsed.port)),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), decimal_payload(value)?);
        }
    }
    for (name, length_name, value, response2) in [
        ("HOST", "HOSTLENGTH", parsed.host.as_bytes(), 29),
        ("PATH", "PATHLENGTH", parsed.path.as_bytes(), 30),
        ("QUERYSTRING", "QUERYSTRLEN", parsed.query.as_bytes(), 8),
    ] {
        if !request.arguments.contains_key(name) {
            continue;
        }
        let capacity = decimal(request, length_name)?;
        let capacity = usize::try_from(capacity).map_err(|_| HostProblem::Malformed)?;
        let declared_name = format!("{name}.MAXLENGTH");
        if request.arguments.contains_key(&declared_name) {
            let declared = usize::try_from(decimal(request, &declared_name)?)
                .map_err(|_| HostProblem::Malformed)?;
            if capacity > declared {
                return Err(HostProblem::Malformed);
            }
        }
        if capacity > MAX_COMPONENT_BYTES {
            return Err(HostProblem::ResourceExhausted);
        }
        response.outputs.insert(
            length_name.into(),
            decimal_payload(
                i64::try_from(value.len()).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
        if capacity < value.len() {
            response
                .outputs
                .insert(name.into(), bounded(value[..capacity].to_vec())?);
            if response.response == 0 {
                response.condition = "LENGERR".into();
                response.response = 22;
                response.response2 = response2;
            }
        }
    }
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "URL",
        "URLLENGTH",
        "SCHEMENAME",
        "HOST",
        "HOSTLENGTH",
        "HOSTTYPE",
        "PORTNUMBER",
        "PATH",
        "PATHLENGTH",
        "QUERYSTRING",
        "QUERYSTRLEN",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
        "HOST.MAXLENGTH",
        "PATH.MAXLENGTH",
        "QUERYSTRING.MAXLENGTH",
    ];
    if request.mutation.is_some()
        || request
            .arguments
            .keys()
            .any(|name| !allowed.contains(&name.as_str()))
        || !request.arguments.contains_key("URL")
        || !request.arguments.contains_key("URLLENGTH")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (area, length) in [
        ("HOST", "HOSTLENGTH"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
    ] {
        if request.arguments.contains_key(area) != request.arguments.contains_key(length) {
            return Err(HostProblem::Malformed);
        }
    }
    if ![
        "SCHEMENAME",
        "HOST",
        "HOSTTYPE",
        "PORTNUMBER",
        "PATH",
        "QUERYSTRING",
    ]
    .iter()
    .any(|name| request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}

fn parse(url: &str) -> Result<ParsedUrl<'_>, HostProblem> {
    let (scheme, remainder) = url.split_once("://").ok_or_else(invalid_url)?;
    let scheme = if scheme.eq_ignore_ascii_case("http") {
        "HTTP"
    } else if scheme.eq_ignore_ascii_case("https") {
        "HTTPS"
    } else {
        return Err(invalid_url());
    };
    if url.bytes().any(|byte| byte == b'%' || byte == b' ') {
        validate_escapes(url)?;
    }
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    let suffix = &remainder[authority_end..];
    if authority.is_empty() || authority.contains(['@', '\\', ' ']) || suffix.contains('#') {
        return Err(invalid_url());
    }
    let (host, port) = if let Some(after_open) = authority.strip_prefix('[') {
        let (host, after_close) = after_open.split_once(']').ok_or_else(invalid_url)?;
        if host.parse::<Ipv6Addr>().is_err() {
            return Err(invalid_url());
        }
        let port = after_close.strip_prefix(':').unwrap_or("");
        if !after_close.is_empty() && !after_close.starts_with(':') {
            return Err(invalid_url());
        }
        (host, port)
    } else {
        let (host, port) = authority.rsplit_once(':').unwrap_or((authority, ""));
        if host.is_empty() || host.contains(':') {
            return Err(invalid_url());
        }
        (host, port)
    };
    let host_type = if host.parse::<Ipv4Addr>().is_ok() {
        2
    } else if host.parse::<Ipv6Addr>().is_ok() {
        3
    } else if host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        1
    } else {
        return Err(invalid_url());
    };
    let port = if port.is_empty() {
        if scheme == "HTTPS" { 443 } else { 80 }
    } else {
        port.parse::<u16>()
            .ok()
            .filter(|value| *value != 0)
            .ok_or_else(invalid_url)?
    };
    let (path, query) = suffix.split_once('?').unwrap_or((suffix, ""));
    if !path.is_empty() && !path.starts_with('/') {
        return Err(invalid_url());
    }
    Ok(ParsedUrl {
        scheme,
        host,
        host_type,
        port,
        path,
        query,
    })
}

fn validate_escapes(url: &str) -> Result<(), HostProblem> {
    let bytes = url.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            if bytes
                .get(at + 1)
                .is_none_or(|byte| !byte.is_ascii_hexdigit())
                || bytes
                    .get(at + 2)
                    .is_none_or(|byte| !byte.is_ascii_hexdigit())
            {
                return Err(HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2: 65,
                });
            }
            at += 3;
        } else {
            if bytes[at] == b' ' {
                return Err(invalid_url());
            }
            at += 1;
        }
    }
    Ok(())
}

fn invalid_url() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 28,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_keeps_escaped_components_and_classifies_ipv6() {
        let parsed = parse("https://[::1]:8443/a%20b?x=%2F&y=1").unwrap();
        assert_eq!(
            (parsed.scheme, parsed.host, parsed.port),
            ("HTTPS", "::1", 8443)
        );
        assert_eq!(parsed.host_type, 3);
        assert_eq!((parsed.path, parsed.query), ("/a%20b", "x=%2F&y=1"));
        assert_eq!(parse("http://example.com/a").unwrap().port, 80);
        assert_eq!(parse("https://example.com/").unwrap().port, 443);
        assert_eq!(
            parse("http://example.com/a%GG").err(),
            Some(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 65,
            })
        );
        assert_eq!(
            parse("http://example.com:0/").err(),
            Some(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 28,
            })
        );
    }
}
