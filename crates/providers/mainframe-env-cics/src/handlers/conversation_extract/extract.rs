use super::{
    CicsService, ConversationKind, ConversationLedger, ConversationOwner, ExtractMetadata, Run,
    bytes, capacity, condition, context, gds_response, normal, number, payload, select_facility,
    text,
};
use crate::conversation_protocol::{
    ConversationContext, ConversationProblem, GdsExtractAttributesFailure as GdsFailure,
};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn attach(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let header_name = if let Some(id) = text(request, "ATTACHID")? {
        if id.is_empty() || id.len() > 8 {
            return Err(condition("CBIDERR", 62, 0));
        }
        id
    } else {
        let facility = select_facility(ledger, metadata, owner, request)?;
        if !matches!(
            facility.kind,
            ConversationKind::LuType61 | ConversationKind::Mro
        ) {
            return Err(condition("INVREQ", 16, 0));
        }
        metadata
            .received_attach
            .clone()
            .ok_or_else(|| condition("CBIDERR", 62, 0))?
    };
    let header = ledger
        .attach(owner, &header_name)
        .ok_or_else(|| condition("CBIDERR", 62, 0))?;
    let mut response = normal(service, run)?;
    for (name, value) in [
        ("PROCESS", &header.process),
        ("RESOURCE", &header.resource),
        ("RPROCESS", &header.return_process),
        ("RRESOURCE", &header.return_resource),
        ("QUEUE", &header.queue),
    ] {
        if request.arguments.contains_key(name) {
            if value.len() > capacity(request, &format!("{name}.MAXLENGTH"))? {
                return Err(condition("INVREQ", 16, 0));
            }
            response.outputs.insert(name.into(), bytes(value.clone())?);
        }
    }
    for (name, value) in [
        ("IUTYPE", header.iu_type),
        ("DATASTR", header.data_stream),
        ("RECFM", header.record_format),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), number(i64::from(value))?);
        }
    }
    Ok(response)
}

pub(super) fn attributes(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let gds = request.operation == CicsOperation::GdsExtractAttributes;
    let facility = match select_facility(ledger, metadata, owner, request) {
        Ok(facility) => facility,
        Err(HostProblem::Condition { name, .. }) if gds && name == "NOTALLOC" => {
            return gds_response(service, run, request, GdsFailure::NotOwned.retcode().0);
        }
        Err(problem) => return Err(problem),
    };
    if facility.principal_facility && context(run)? == ConversationContext::DplServer {
        return if gds {
            gds_response(service, run, request, GdsFailure::DplPrincipal.retcode().0)
        } else {
            Err(condition("INVREQ", 16, 200))
        };
    }
    if gds {
        let code = match facility.kind {
            ConversationKind::AppcBasic => [0; 6],
            ConversationKind::AppcMapped => GdsFailure::NotBasic.retcode().0,
            _ => GdsFailure::NotAppc.retcode().0,
        };
        if code != [0; 6] {
            return gds_response(service, run, request, code);
        }
        if request.arguments.contains_key("STATE") {
            return Err(HostProblem::Unsupported);
        }
        let mut response = gds_response(service, run, request, [0; 6])?;
        response.outputs.insert(
            "CONVDATA".into(),
            bytes(facility.indicators.convdata().to_vec())?,
        );
        return Ok(response);
    }
    if !matches!(
        facility.kind,
        ConversationKind::AppcMapped | ConversationKind::Mro
    ) {
        return Err(condition("INVREQ", 16, 0));
    }
    // The pinned CVDA explanation identifies the fullword shape but the
    // numeric value table dfha80c.html is not in the committed source corpus.
    Err(HostProblem::Unsupported)
}

pub(super) fn logon(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let _ = select_facility(ledger, metadata, owner, request)?;
    let data = if metadata.logon_consumed {
        &[][..]
    } else {
        metadata.logon_message.as_deref().unwrap_or_default()
    };
    let mut response = normal(service, run)?;
    response
        .outputs
        .insert("LENGTH".into(), number(data.len() as i64)?);
    if request.arguments.contains_key("INTO") {
        if data.len() > capacity(request, "INTO.MAXLENGTH")? {
            return Err(HostProblem::ResourceExhausted);
        }
        response
            .outputs
            .insert("INTO".into(), bytes(data.to_vec())?);
    } else if request.arguments.contains_key("SET") {
        if data.len() > capacity(request, "SET.MAXLENGTH")? {
            return Err(HostProblem::ResourceExhausted);
        }
        let value = if data.is_empty() {
            payload("mainframe-env.cics.pointer-null@1", Vec::new())?
        } else {
            bytes(data.to_vec())?
        };
        response.outputs.insert("SET".into(), value);
    }
    Ok(response)
}

pub(super) fn tct(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let netname = request
        .arguments
        .get("NETNAME")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    if netname.len() != 8 || !netname.is_ascii() {
        return Err(condition("INVREQ", 16, 0));
    }
    let netname = std::str::from_utf8(netname).map_err(|_| condition("INVREQ", 16, 0))?;
    let entry = metadata
        .netnames
        .get(netname)
        .ok_or_else(|| condition("INVREQ", 16, 0))?;
    let facility = ledger
        .conversation(entry.token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
    match facility.check_owner(owner, ConversationContext::Local) {
        Ok(()) if facility.kind == ConversationKind::LuType61 => {}
        Err(ConversationProblem::StaleOwner) => return Err(HostProblem::Unauthorized),
        _ => return Err(condition("NOTALLOC", 61, 0)),
    }
    let mut response = normal(service, run)?;
    if request.arguments.contains_key("SYSID") {
        response
            .outputs
            .insert("SYSID".into(), bytes(entry.sysid.as_bytes().to_vec())?);
    }
    if request.arguments.contains_key("TERMID") {
        response
            .outputs
            .insert("TERMID".into(), bytes(entry.termid.as_bytes().to_vec())?);
    }
    Ok(response)
}

pub(super) fn point(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let facility = select_facility(ledger, metadata, owner, request)?;
    if !matches!(
        facility.kind,
        ConversationKind::LuType61 | ConversationKind::Mro
    ) {
        return Err(condition("NOTALLOC", 61, 0));
    }
    normal(service, run)
}
