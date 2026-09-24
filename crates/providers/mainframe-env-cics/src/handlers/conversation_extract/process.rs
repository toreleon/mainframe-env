use super::{
    CicsService, ConversationKind, ConversationLedger, ConversationOwner, ExtractMetadata, Run,
    bytes, capacity, condition, context, gds_response, normal, number, payload, select_facility,
    text,
};
use crate::conversation_protocol::{ConversationContext, GdsExtractProcessFailure as GdsFailure};
use mainframe_env_host_api::{CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    ledger: &ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
) -> Result<CicsResponse, HostProblem> {
    let gds = request.operation == CicsOperation::GdsExtractProcess;
    let facility = match select_facility(ledger, metadata, owner, request) {
        Ok(facility) => facility,
        Err(HostProblem::Condition { name, .. }) if gds && name == "NOTALLOC" => {
            return gds_response(service, run, request, GdsFailure::NotOwned.retcode().0);
        }
        Err(problem) => return Err(problem),
    };
    if !gds && facility.principal_facility && context(run)? == ConversationContext::DplServer {
        return Err(condition("INVREQ", 16, 200));
    }
    if !facility.principal_facility || !metadata.network_attached {
        return if gds {
            gds_response(
                service,
                run,
                request,
                GdsFailure::NotAppcOrPrincipal.retcode().0,
            )
        } else {
            Err(condition("INVREQ", 16, 0))
        };
    }
    let valid_kind = if gds {
        facility.kind == ConversationKind::AppcBasic
    } else {
        facility.kind == ConversationKind::AppcMapped
    };
    if !valid_kind {
        return if gds {
            gds_response(
                service,
                run,
                request,
                match facility.kind {
                    ConversationKind::AppcMapped => GdsFailure::NotBasic.retcode().0,
                    _ => GdsFailure::NotAppcOrPrincipal.retcode().0,
                },
            )
        } else {
            Err(condition("INVREQ", 16, 0))
        };
    }
    if request.arguments.contains_key("MAXPROCLEN") && !request.arguments.contains_key("PROCNAME")
        || request.arguments.contains_key("PROCNAME")
            && !request.arguments.contains_key("PROCLENGTH")
        || request.arguments.contains_key("PIPLIST") != request.arguments.contains_key("PIPLENGTH")
    {
        return Err(HostProblem::Malformed);
    }
    let maximum = text(request, "MAXPROCLEN")?
        .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
        .transpose()?
        .unwrap_or(32);
    if !(1..=64).contains(&maximum) {
        return Err(HostProblem::Malformed);
    }
    if request.arguments.contains_key("PROCNAME") {
        let area = capacity(request, "PROCNAME.MAXLENGTH")?;
        if area < maximum || area > 64 {
            return Err(HostProblem::Malformed);
        }
    }
    if request.arguments.contains_key("PIPLIST")
        && facility.pip.len() > capacity(request, "PIPLIST.MAXLENGTH")?
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let process = facility
        .process
        .as_ref()
        .ok_or_else(|| condition("INVREQ", 16, 0))?;
    if process.len() > maximum {
        return if gds {
            gds_response(
                service,
                run,
                request,
                GdsFailure::ProcessTooLong.retcode().0,
            )
        } else {
            Err(condition("LENGERR", 22, 0))
        };
    }
    let mut response = if gds {
        gds_response(service, run, request, [0; 6])?
    } else {
        normal(service, run)?
    };
    if request.arguments.contains_key("PROCNAME") {
        let mut name = process.clone();
        name.resize(maximum, b' ');
        response.outputs.insert("PROCNAME".into(), bytes(name)?);
    }
    if request.arguments.contains_key("PROCLENGTH") {
        response
            .outputs
            .insert("PROCLENGTH".into(), number(process.len() as i64)?);
    }
    if request.arguments.contains_key("SYNCLEVEL") {
        let level = facility
            .sync_level
            .ok_or_else(|| condition("INVREQ", 16, 0))?;
        response
            .outputs
            .insert("SYNCLEVEL".into(), number(i64::from(level))?);
    }
    if request.arguments.contains_key("PIPLENGTH") {
        response
            .outputs
            .insert("PIPLENGTH".into(), number(facility.pip.len() as i64)?);
    }
    if request.arguments.contains_key("PIPLIST") {
        let value = if facility.pip.is_empty() {
            payload("mainframe-env.cics.pointer-null@1", Vec::new())?
        } else {
            bytes(facility.pip.clone())?
        };
        response.outputs.insert("PIPLIST".into(), value);
    }
    Ok(response)
}
