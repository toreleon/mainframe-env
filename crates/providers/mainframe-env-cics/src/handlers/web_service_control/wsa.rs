use super::*;
use crate::service::handlers::transform_control;
use mainframe_env_encoding::CodePage;

const WSA_NS: &str = "http://www.w3.org/2005/08/addressing";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::WsaEprCreate => create_epr(service, run, request),
        CicsOperation::WsaContextBuild => build(service, run, request, retention_tick),
        CicsOperation::WsaContextGet => get(service, run, request),
        CicsOperation::WsaContextDelete => delete(service, run, request, retention_tick),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn build(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let role = web_role(run)?;
    let channel = channel(service, run, request)?;
    let mut state = state::load(service, run, &channel)?;
    let context = if role == "provider" { "RESP" } else { "REQ" };
    if role == "requester" && !request.arguments.contains_key("CHANNEL") {
        return Err(condition("INVREQ", 16, 4));
    }
    if value(request, "ACTION").is_none()
        && value(request, "MESSAGEID").is_none()
        && value(request, "RELATESURI").is_none()
        && value(request, "EPRFROM").is_none()
    {
        return Err(HostProblem::Malformed);
    }
    if value(request, "FROMCCSID").is_some() && value(request, "FROMCODEPAGE").is_some() {
        return Err(HostProblem::Malformed);
    }
    from_ccsid(request)?;
    for (option, error) in [
        ("ACTION", 6),
        ("MESSAGEID", 7),
        ("RELATESURI", 8),
        ("RELATESTYPE", 9),
    ] {
        if let Some(data) = utf8_data(request, option, 255, None, error)? {
            let uri = data.trim_end_matches(' ');
            if !soap_fault::valid_uri(uri) {
                return Err(condition("INVREQ", 16, error));
            }
            if option == "RELATESURI" {
                let next = (1..=32)
                    .find(|index| {
                        !state
                            .fields
                            .contains_key(&format!("WSA.{context}.RELATES.{index}.URI"))
                    })
                    .ok_or(HostProblem::ResourceExhausted)?;
                state.fields.insert(
                    format!("WSA.{context}.RELATES.{next}.URI"),
                    uri.as_bytes().to_vec(),
                );
                let kind = text(request, "RELATESTYPE", 255)?
                    .unwrap_or_else(|| "http://www.w3.org/2005/08/addressing/reply".into());
                if !soap_fault::valid_uri(kind.trim_end_matches(' ')) {
                    return Err(condition("INVREQ", 16, 9));
                }
                state.fields.insert(
                    format!("WSA.{context}.RELATES.{next}.TYPE"),
                    kind.trim_end_matches(' ').as_bytes().to_vec(),
                );
            } else if option != "RELATESTYPE" {
                state
                    .fields
                    .insert(format!("WSA.{context}.{option}"), uri.as_bytes().to_vec());
            }
        }
    }
    if value(request, "EPRFROM").is_some() {
        let kind = text(request, "EPRTYPE", 16)?.ok_or(HostProblem::Malformed)?;
        let field = text(request, "EPRFIELD", 16)?.ok_or(HostProblem::Malformed)?;
        if !matches!(
            kind.as_str(),
            "TOEPR" | "REPLYTOEPR" | "FAULTTOEPR" | "FROMEPR"
        ) || !matches!(field.as_str(), "ADDRESS" | "ALL" | "METADATA" | "REFPARMS")
        {
            return Err(HostProblem::Malformed);
        }
        let data = utf8_data(
            request,
            "EPRFROM",
            service.limits.max_screen_bytes,
            Some("EPRLENGTH"),
            20,
        )?
        .ok_or(HostProblem::Malformed)?;
        let normalized = if field == "ADDRESS" {
            let uri = data.trim_end_matches(' ');
            if !soap_fault::valid_uri(uri) {
                return Err(condition("INVREQ", 16, 15));
            }
            uri.to_string()
        } else {
            if !transform_control::valid_web_xml(&data, service.limits) {
                return Err(condition(
                    "INVREQ",
                    16,
                    match field.as_str() {
                        "METADATA" => 13,
                        "REFPARMS" => 14,
                        _ => 10,
                    },
                ));
            }
            data.into()
        };
        state.fields.insert(
            format!("WSA.{context}.EPR.{kind}.{field}"),
            normalized.into_bytes(),
        );
    } else if value(request, "EPRTYPE").is_some()
        || value(request, "EPRFIELD").is_some()
        || value(request, "EPRLENGTH").is_some()
    {
        return Err(HostProblem::Malformed);
    }
    authorize_channel(service, run, &channel, AccessIntent::Update)?;
    let response = normal(service, run)?;
    state::persist(
        service,
        run,
        request,
        retention_tick,
        &channel,
        &state,
        &response,
    )?;
    Ok(response)
}

fn delete(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if web_role(run)? != "requester" {
        return Err(condition("INVREQ", 16, 5));
    }
    let channel = channel(service, run, request)?;
    let mut state = state::load(service, run, &channel)?;
    if !state.fields.keys().any(|name| name.starts_with("WSA.")) {
        return Err(condition("NOTFND", 13, 3));
    }
    authorize_channel(service, run, &channel, AccessIntent::Update)?;
    state.fields.retain(|name, _| !name.starts_with("WSA."));
    let response = normal(service, run)?;
    state::persist(
        service,
        run,
        request,
        retention_tick,
        &channel,
        &state,
        &response,
    )?;
    Ok(response)
}

fn get(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let role = web_role(run)?;
    let channel = channel(service, run, request)?;
    if role == "requester" && !request.arguments.contains_key("CHANNEL") {
        return Err(condition("INVREQ", 16, 4));
    }
    let kind = text(request, "CONTEXTTYPE", 16)?.unwrap_or_else(|| "REQCONTEXT".into());
    let context = match kind.as_str() {
        "REQCONTEXT" => "REQ",
        "RESPCONTEXT" if role == "requester" => "RESP",
        _ => return Err(condition("INVREQ", 16, 4)),
    };
    let state = state::load(service, run, &channel)?;
    if !state
        .fields
        .keys()
        .any(|name| name.starts_with(&format!("WSA.{context}.")))
    {
        return Err(condition("NOTFND", 13, 3));
    }
    authorize_channel(service, run, &channel, AccessIntent::Read)?;
    let target_page = target_encoding(request)?;
    let index = number(request, "RELATESINDEX")?.unwrap_or(1);
    if index < 1 || index > 32 {
        return Err(condition("INVREQ", 16, 11));
    }
    let mut response = normal(service, run)?;
    for (name, key) in [
        ("ACTION", "ACTION"),
        ("MESSAGEID", "MESSAGEID"),
        ("RELATESURI", "RELATESURI"),
        ("RELATESTYPE", "RELATESTYPE"),
    ] {
        if !request.arguments.contains_key(name) {
            continue;
        }
        let key = if key.starts_with("RELATES") {
            format!(
                "WSA.{context}.RELATES.{index}.{}",
                if key == "RELATESURI" { "URI" } else { "TYPE" }
            )
        } else {
            format!("WSA.{context}.{key}")
        };
        let bytes = state
            .fields
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if key.contains("RELATES") && bytes.is_empty() && index > 1 {
            return Err(condition("INVREQ", 16, 12));
        }
        response
            .outputs
            .insert(name.into(), bounded(padded(bytes, 255, target_page)?)?);
    }
    if request.arguments.contains_key("EPRINTO") || request.arguments.contains_key("EPRSET") {
        let epr_type = text(request, "EPRTYPE", 16)?.ok_or(HostProblem::Malformed)?;
        let epr_field = text(request, "EPRFIELD", 16)?.ok_or(HostProblem::Malformed)?;
        if !matches!(
            epr_type.as_str(),
            "TOEPR" | "REPLYTOEPR" | "FAULTTOEPR" | "FROMEPR"
        ) || !matches!(
            epr_field.as_str(),
            "ADDRESS" | "ALL" | "METADATA" | "REFPARMS"
        ) {
            return Err(HostProblem::Malformed);
        }
        let key = format!("WSA.{context}.EPR.{epr_type}.{epr_field}");
        let bytes = state
            .fields
            .get(&key)
            .ok_or_else(|| condition("NOTFND", 13, 3))?;
        let encoded = encode_output(bytes, target_page)?;
        epr_output(request, &mut response, &encoded)?;
    }
    Ok(response)
}

fn create_epr(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if value(request, "FROMCCSID").is_some() && value(request, "FROMCODEPAGE").is_some() {
        return Err(HostProblem::Malformed);
    }
    from_ccsid(request)?;
    let address = text(request, "ADDRESS", 255)?.ok_or(HostProblem::Malformed)?;
    let address = address.trim_end_matches(' ');
    if !soap_fault::valid_uri(address) {
        return Err(condition("INVREQ", 16, 8));
    }
    let metadata = utf8_data(
        request,
        "METADATA",
        service.limits.max_screen_bytes,
        Some("METADATALEN"),
        20,
    )?;
    let refparms = utf8_data(
        request,
        "REFPARMS",
        service.limits.max_screen_bytes,
        Some("REFPARMSLEN"),
        20,
    )?;
    for (fragment, code) in [(metadata.as_deref(), 13), (refparms.as_deref(), 14)] {
        if let Some(fragment) = fragment {
            let wrapped = format!("<root>{fragment}</root>");
            if !transform_control::valid_web_xml(&wrapped, service.limits) {
                return Err(condition("INVREQ", 16, code));
            }
        }
    }
    let mut xml = format!(
        "<wsa:EndpointReference xmlns:wsa=\"{WSA_NS}\"><wsa:Address>{}</wsa:Address>",
        escape(address)
    );
    if let Some(refparms) = refparms {
        xml.push_str("<wsa:ReferenceParameters>");
        xml.push_str(&refparms);
        xml.push_str("</wsa:ReferenceParameters>");
    }
    if let Some(metadata) = metadata {
        xml.push_str("<wsa:Metadata>");
        xml.push_str(&metadata);
        xml.push_str("</wsa:Metadata>");
    }
    xml.push_str("</wsa:EndpointReference>");
    if xml.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut response = normal(service, run)?;
    epr_output(request, &mut response, xml.as_bytes())?;
    Ok(response)
}

fn epr_output(
    request: &CicsRequest,
    response: &mut CicsResponse,
    bytes: &[u8],
) -> Result<(), HostProblem> {
    let into = request.arguments.contains_key("EPRINTO");
    let set = request.arguments.contains_key("EPRSET");
    if into == set {
        return Err(HostProblem::Malformed);
    }
    if into {
        let max = number(request, "EPRLENGTH")?.ok_or(HostProblem::Malformed)?;
        let max = max.max(0) as usize;
        let max = number(request, "EPRINTO.MAXLENGTH")?
            .map(|n| n.max(0) as usize)
            .map_or(max, |capacity| capacity.min(max));
        let length = max.min(bytes.len());
        response
            .outputs
            .insert("EPRINTO".into(), bounded(bytes[..length].to_vec())?);
        if length < bytes.len() {
            response.condition = "LENGERR".into();
            response.response = 22;
            response.response2 = 20;
        }
    } else {
        response
            .outputs
            .insert("EPRSET".into(), bounded(bytes.to_vec())?);
    }
    response
        .outputs
        .insert("EPRLENGTH".into(), decimal_payload(bytes.len() as i64)?);
    Ok(())
}

fn target_encoding(request: &CicsRequest) -> Result<Option<CodePage>, HostProblem> {
    if value(request, "INTOCCSID").is_some() && value(request, "INTOCODEPAGE").is_some() {
        return Err(HostProblem::Malformed);
    }
    let code = if let Some(ccsid) = number(request, "INTOCCSID")? {
        ccsid
    } else if let Some(codepage) = text(request, "INTOCODEPAGE", 40)? {
        match codepage.trim().to_ascii_uppercase().as_str() {
            "UTF-8" | "1208" => 1208,
            "IBM-037" | "IBM037" | "CP037" | "37" => 37,
            _ => return Err(condition("CODEPAGEERR", 125, 1)),
        }
    } else {
        1208
    };
    if code <= 0 || code > 65_535 {
        return Err(condition("CCSIDERR", 123, 1));
    }
    match code {
        1208 => Ok(None),
        37 => Ok(Some(CodePage::Cp037)),
        _ => Err(condition("CCSIDERR", 123, 2)),
    }
}

fn encode_output(bytes: &[u8], page: Option<CodePage>) -> Result<Vec<u8>, HostProblem> {
    match page {
        None => Ok(bytes.to_vec()),
        Some(page) => page
            .encode(
                std::str::from_utf8(bytes).map_err(|_| condition("CCSIDERR", 123, 6))?,
                bytes.len().saturating_mul(2),
            )
            .map_err(|_| condition("CCSIDERR", 123, 6)),
    }
}

fn padded(bytes: &[u8], width: usize, page: Option<CodePage>) -> Result<Vec<u8>, HostProblem> {
    let mut result = encode_output(bytes, page)?;
    result.truncate(width);
    result.resize(width, if page.is_some() { 0x40 } else { b' ' });
    Ok(result)
}

fn escape(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(character),
        }
    }
    result
}
