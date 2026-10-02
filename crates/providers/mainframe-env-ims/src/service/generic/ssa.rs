//! Adapter from bounded raw operands to metadata and engine-owned navigation.
use super::*;
use mainframe_env_host_api::{
    ImsCallSite, ImsCallSyntax, ImsNavigationRequest, ImsPcbKind, ImsSsa, ImsSsaForm, ImsSsaLimits,
    ImsSsaProblem, parse_ims_ssa, validate_ims_call_site,
};

pub(in crate::service) struct Prepared {
    ssas: Vec<ImsSsa>,
    read: ReadRequest,
    sensitivity: Option<&'static str>,
}

pub(in crate::service) fn prepare(
    state: &State,
    invocation: &Invocation,
    navigation: &ImsNavigationRequest,
    limits: ImsLimits,
) -> Result<Prepared, HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    if !session.generic {
        return Err(HostProblem::Unsupported);
    }
    let pcb = session_pcb(state, run, navigation.request.pcb)?;
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    let selected_index = pcb.secondary_index.as_deref().map(normalize);
    let secondary = selected_index.as_deref();
    let fields = engine.ssa_fields(secondary);
    let ssas = navigation
        .ssas
        .iter()
        .map(|raw| {
            parse_ims_ssa(raw, ImsSsaLimits::default(), &fields).map_err(|p| {
                if p == ImsSsaProblem::ResourceExhausted {
                    HostProblem::ResourceExhausted
                } else {
                    HostProblem::Malformed
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if engine.definition().organization == crate::ImsDatabaseOrganization::Gsam {
        return Err(HostProblem::Unsupported);
    }
    if ssas.iter().filter(|s| s.concatenated_key.is_some()).count() > 1 {
        return Err(HostProblem::Malformed);
    }
    for ssa in &ssas {
        if !engine
            .definition()
            .segments
            .iter()
            .any(|s| s.name == ssa.segment)
        {
            return Err(HostProblem::Malformed);
        }
        if ssa
            .commands
            .iter()
            .any(|c| !matches!(c.code, b'C' | b'O' | b'D' | b'P'))
            || engine.definition().organization == crate::ImsDatabaseOrganization::Msdb
                && !ssa.commands.is_empty()
            || engine.definition().organization == crate::ImsDatabaseOrganization::Dedb
                && ssa.commands.iter().any(|c| matches!(c.code, b'D' | b'P'))
            || !pcb.processing_options.contains('P') && ssa.commands.iter().any(|c| c.code == b'D')
        {
            return Err(HostProblem::Unsupported);
        }
    }
    if ssas
        .iter()
        .filter(|s| s.commands.iter().any(|c| c.code == b'P'))
        .count()
        > 1
    {
        return Err(HostProblem::Unsupported);
    }
    let mut read = read_request(&navigation.request, &engine)?;
    if let Some(last) = ssas.last() {
        read.target = Some(last.segment.clone());
    }
    read.path = ssas
        .iter()
        .map(|s| SegmentSelector {
            segment: s.segment.clone(),
            predicates: vec![],
        })
        .collect();
    engine
        .validate_ssas(&read, &ssas, secondary)
        .map_err(|p| match p {
            EngineProblem::Unsupported => HostProblem::Unsupported,
            EngineProblem::LimitExceeded => HostProblem::ResourceExhausted,
            _ => HostProblem::Malformed,
        })?;
    let form = if ssas.iter().any(|s| s.concatenated_key.is_some()) {
        ImsSsaForm::ConcatenatedKey
    } else if ssas
        .iter()
        .any(|s| s.commands.iter().any(|c| c.code == b'D'))
    {
        ImsSsaForm::Path
    } else if ssas.iter().any(|s| !s.predicates.is_empty()) {
        ImsSsaForm::Qualified
    } else if ssas.is_empty() {
        ImsSsaForm::Absent
    } else {
        ImsSsaForm::Unqualified
    };
    let (row, name) = match navigation.request.operation {
        ImsOperation::GetUnique => (5, "GU"),
        ImsOperation::GetNext => (5, "GN"),
        ImsOperation::GetNextParent => (5, "GNP"),
        ImsOperation::GetHoldUnique => (6, "GHU"),
        ImsOperation::GetHoldNext => (6, "GHN"),
        ImsOperation::GetHoldNextParent => (6, "GHNP"),
        _ => return Err(HostProblem::Malformed),
    };
    let (_, raw_option, organization) =
        system::metadata_pcb(state, &session.psb, navigation.request.pcb)?;
    let option = system::processing_option(raw_option)?;
    validate_ims_call_site(&ImsCallSite {
        official_row: format!("ibm-ims-15.6-dli-2026-08-31:dli-call-families:{row:04}"),
        name,
        syntax: ImsCallSyntax::Call,
        context: navigation.context,
        pcb_kind: Some(ImsPcbKind::Database),
        organization: Some(organization),
        processing_option: Some(option),
        ssa_form: form,
    })
    .map_err(|_| HostProblem::Unsupported)?;
    let mut sensitivity_request = navigation.request.clone();
    sensitivity_request.segments = ssas.iter().map(|ssa| ssa.segment.clone()).collect();
    let sensitivity = pcb::read_status(pcb, &sensitivity_request, &read);
    Ok(Prepared {
        ssas,
        read,
        sensitivity,
    })
}

pub(in crate::service) fn read(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    prepared: &Prepared,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let pcb = session_pcb(state, run, request.pcb)?.clone();
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    if let Some(code) = prepared.sensitivity {
        return Ok(status(code));
    }
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    let mut position = pcb::position(session, request.pcb);
    let parent_qualification = pcb::gnp_target_below_parent(&engine, &position, &prepared.read);
    let secondary = pcb.secondary_index.as_deref().map(normalize);
    let outcome = engine.read_ssas(
        &mut position,
        &prepared.read,
        &prepared.ssas,
        secondary.as_deref(),
        |segment| {
            prepared.read.target.is_some()
                || (allowed(&pcb, segment, request.operation) && !pcb::key_only(&pcb, segment))
        },
    );
    pcb::set_position(session, request.pcb, position);
    let view = match outcome {
        Ok(view) => view,
        Err(EngineProblem::Unsupported) => return Err(HostProblem::Unsupported),
        Err(EngineProblem::PathMismatch) if parent_qualification => return Ok(status("GE")),
        Err(problem) => return Ok(status(engine_status(problem))),
    };
    let path = engine
        .path_to(view.id)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let selected = path
        .into_iter()
        .filter(|v| {
            !pcb::key_only(&pcb, &v.segment)
                && (v.id == view.id
                    || prepared.ssas.iter().any(|s| {
                        s.segment == v.segment && s.commands.iter().any(|c| c.code == b'D')
                    }))
        })
        .collect::<Vec<_>>();
    if selected.len() > request.max_segments as usize {
        return Err(HostProblem::ResourceExhausted);
    }
    let segments = selected
        .into_iter()
        .map(|v| logical::segment_result(state, limits, &engine, v))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImsResult {
        status: "  ".into(),
        segments,
        checkpoint_id: None,
        affected_segments: 0,
        system: None,
    })
}
