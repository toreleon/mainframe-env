//! Public GSAM operand adapter; PCB, UOW, rows and recovery retain their owners.
use super::*;
use mainframe_env_host_api::{
    ImsCallSite, ImsCallSyntax, ImsExecutionContext, ImsGsamRequest, ImsGsamResult,
    ImsGsamSearchArgument, ImsPcbKind, ImsProcessingOptionClass, ImsSsaForm,
    validate_ims_call_site,
};

pub(in crate::service) fn prepare(
    state: &State,
    invocation: &Invocation,
    call: &ImsGsamRequest,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    if !session.generic || call.context != ImsExecutionContext::DbBatch {
        return Err(HostProblem::Unsupported);
    }
    let pcb = session_pcb(state, run, call.request.pcb)?;
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    if engine.definition().organization != crate::ImsDatabaseOrganization::Gsam {
        return Err(HostProblem::Unsupported);
    }
    let record = &engine.definition().segments[0];
    if record.min_length != record.max_length {
        // RECFM=V LL/RDW and RECFM=U PCB length need owned format metadata.
        return Err(HostProblem::Unsupported);
    }
    let option = match pcb.processing_options.as_str() {
        "G" | "GS" => ImsProcessingOptionClass::Read,
        "L" | "LS" => ImsProcessingOptionClass::Insert,
        _ => return Err(HostProblem::Unsupported),
    };
    let (row, name, form) = match call.request.operation {
        ImsOperation::GetUnique => (5, "GU", ImsSsaForm::RecordSearchArgument),
        ImsOperation::GetNext => (5, "GN", ImsSsaForm::Absent),
        ImsOperation::Insert => (8, "ISRT", ImsSsaForm::Absent),
        _ => return Err(HostProblem::Malformed),
    };
    validate_ims_call_site(&ImsCallSite {
        official_row: format!("ibm-ims-15.6-dli-2026-08-31:dli-call-families:{row:04}"),
        name,
        syntax: ImsCallSyntax::Call,
        context: call.context,
        pcb_kind: Some(ImsPcbKind::Gsam),
        organization: Some("GSAM"),
        processing_option: Some(option),
        ssa_form: form,
    })
    .map_err(|_| HostProblem::Unsupported)?;
    Ok(())
}

pub(in crate::service) fn apply(
    state: &mut State,
    versions: &RowVersions,
    run: &str,
    call: &ImsGsamRequest,
    request_digest: [u8; 32],
    limits: ImsLimits,
) -> Result<ImsGsamResult, HostProblem> {
    let request = &call.request;
    let pcb = session_pcb(state, run, request.pcb)?.clone();
    let name = normalize(&pcb.database);
    // The shared monotonic row CAS prevents identity revival after backout/load
    // even if a safely pruned idempotency key and identical record bytes recur.
    let version = versions
        .get(&(GENERIC_DATABASE_NAMESPACE.into(), name.clone()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.ims-gsam-address-issuance@1\0");
    hash.update(request_digest);
    hash.update(version.to_le_bytes());
    let seed = hash.finalize().into();
    let mut engine = restored(state, &name, limits)?;
    let mut position = pcb::position(
        state.sessions.get(run).ok_or(HostProblem::NotFound)?,
        request.pcb,
    );
    let wrap = |result| ImsGsamResult {
        result,
        address: None,
    };
    let mut changed = false;
    let view = match request.operation {
        ImsOperation::GetUnique => match &call.search {
            None => return Ok(wrap(status("AH"))),
            Some(ImsGsamSearchArgument::Beginning) => {
                pcb::set_position(
                    Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
                    request.pcb,
                    PcbPosition::default(),
                );
                return Ok(wrap(status("  ")));
            }
            Some(ImsGsamSearchArgument::Record(address)) => {
                match engine.read_gsam_address(&mut position, address) {
                    Ok(view) => view,
                    Err(EngineProblem::InvalidRequest) => return Ok(wrap(status("AJ"))),
                    Err(_) => return Err(HostProblem::InfrastructureFailure),
                }
            }
        },
        ImsOperation::GetNext => match engine.read_gsam_next(&mut position) {
            Ok(view) => view,
            Err(EngineProblem::EndOfDatabase) => {
                pcb::set_position(
                    Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
                    request.pcb,
                    position,
                );
                return Ok(wrap(status("GB")));
            }
            Err(_) => return Err(HostProblem::InfrastructureFailure),
        },
        ImsOperation::Insert => {
            isolation::ensure_writer(state, run, &name)?;
            let view = engine
                .insert(InsertRequest {
                    segment: engine.definition().segments[0].name.clone(),
                    parent: None,
                    data: request.data.clone(),
                })
                .map_err(install_error)?;
            // GSAM has no parentage or hold; input PCBs keep their positions.
            position = PcbPosition::default();
            let address = engine
                .issue_gsam_address(view.id, seed)
                .map_err(install_error)?
                .0;
            isolation::publish_image(state, run, &name, engine.image(), limits)?;
            pcb::set_position(
                Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
                request.pcb,
                position,
            );
            return Ok(ImsGsamResult {
                result: affected(1),
                address: call.save_address.then_some(address),
            });
        }
        _ => return Err(HostProblem::Malformed),
    };
    let address = if call.save_address {
        let (address, materialized) = engine
            .issue_gsam_address(view.id, seed)
            .map_err(install_error)?;
        changed |= materialized;
        Some(address)
    } else {
        None
    };
    if changed {
        isolation::publish_image(state, run, &name, engine.image(), limits)?;
    }
    pcb::set_position(
        Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
        request.pcb,
        position,
    );
    Ok(ImsGsamResult {
        result: ImsResult {
            status: "  ".into(),
            segments: vec![segment_result(&engine, view)?],
            checkpoint_id: None,
            affected_segments: 0,
            system: None,
        },
        address,
    })
}
