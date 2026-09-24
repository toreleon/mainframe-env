use super::{
    CicsService, ConversationKind, ConversationLedger, ConversationOwner, ExtractMetadata, Run,
    bytes, capacity, condition, context, gds_response, normal, number, payload, select_facility,
    text,
};
use crate::conversation_protocol::{
    ConversationContext, ConversationProblem, ConversationState,
    GdsExtractAttributesFailure as GdsFailure,
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
        let mut response = gds_response(service, run, request, [0; 6])?;
        if request.arguments.contains_key("STATE") {
            response
                .outputs
                .insert("STATE".into(), number(state_cvda(facility.state))?);
        }
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
    if facility.kind == ConversationKind::Mro
        && !matches!(
            facility.state,
            ConversationState::Allocated
                | ConversationState::Free
                | ConversationState::PendFree
                | ConversationState::Receive
                | ConversationState::Rollback
                | ConversationState::Send
                | ConversationState::SyncFree
                | ConversationState::SyncReceive
                | ConversationState::SyncSend
        )
    {
        return Err(condition("INVREQ", 16, 0));
    }
    let mut response = normal(service, run)?;
    response
        .outputs
        .insert("STATE".into(), number(state_cvda(facility.state))?);
    Ok(response)
}

/// Pinned `dfha80c.html` CVDAs for EXTRACT ATTRIBUTES and GDS STATE.
fn state_cvda(state: ConversationState) -> i64 {
    match state {
        ConversationState::Allocated => 82,
        ConversationState::ConfFree => 83,
        ConversationState::ConfReceive => 84,
        ConversationState::ConfSend => 85,
        ConversationState::Free => 86,
        ConversationState::PendFree => 87,
        ConversationState::PendReceive => 88,
        ConversationState::Receive => 89,
        ConversationState::Rollback => 90,
        ConversationState::Send => 91,
        ConversationState::SyncFree => 92,
        ConversationState::SyncReceive => 93,
        ConversationState::SyncSend => 94,
    }
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

#[cfg(test)]
mod tests {
    use super::{ConversationState as S, state_cvda};

    #[test]
    fn attributes_state_values_match_pinned_cics_cvda_table() {
        for (state, expected) in [
            (S::Allocated, 82),
            (S::ConfFree, 83),
            (S::ConfReceive, 84),
            (S::ConfSend, 85),
            (S::Free, 86),
            (S::PendFree, 87),
            (S::PendReceive, 88),
            (S::Receive, 89),
            (S::Rollback, 90),
            (S::Send, 91),
            (S::SyncFree, 92),
            (S::SyncReceive, 93),
            (S::SyncSend, 94),
        ] {
            assert_eq!(state_cvda(state), expected);
        }
    }
}
