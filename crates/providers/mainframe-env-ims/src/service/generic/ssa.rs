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
    primary: Option<crate::database::primary_position::PrimaryPlan>,
}

/// Selected catalog proof, separate from the live engine/occurrence proof.
/// None means an ordinary legacy producer; it cannot mint a certificate.
pub(in crate::service) fn primary_identity(
    state: &State,
    psb: &str,
    number: u16,
    pcb: &ImsDatabasePcbMetadata,
    engine: &DatabaseEngine,
    limits: ImsLimits,
) -> Result<Option<[u8; 32]>, HostProblem> {
    let catalog = state.metadata.as_ref().ok_or(HostProblem::Unsupported)?;
    let name = normalize(&pcb.database);
    let database = catalog
        .databases
        .iter()
        .find(|db| normalize(&db.name) == name)
        .ok_or(HostProblem::Unsupported)?;
    if pcb.secondary_index.is_some()
        || !engine.primary_shape()
        || pcb
            .processing_options
            .chars()
            .any(|c| matches!(c, 'O' | 'E' | 'Q' | 'K'))
        || pcb.sensitive_segments.iter().any(|s| {
            s.processing_options
                .as_ref()
                .is_some_and(|p| p.contains('K'))
        })
        || catalog
            .databases
            .iter()
            .flat_map(|db| &db.logical_relationships)
            .any(|r| normalize(&r.parent_database) == name || normalize(&r.child_database) == name)
        || definition(database)? != *engine.definition()
        || database.segments.iter().any(|s| {
            let mut fields = s.fields.iter().filter(|f| f.sequence);
            s.min_length != s.max_length
                || fields.next().is_none_or(|f| {
                    !f.unique
                        || f.name.is_none()
                        || f.length == 0
                        || f.offset
                            .checked_add(f.length)
                            .is_none_or(|end| end > s.min_length)
                })
                || fields.next().is_some()
        })
    {
        return Ok(None);
    }
    for other in state
        .generic_databases
        .keys()
        .filter(|other| other.as_str() != name)
    {
        if restored(state, other, limits)?
            .logical_links()
            .iter()
            .any(|link| normalize(&link.parent_database) == name)
        {
            return Ok(None);
        }
    }
    let material = serde_json::to_vec(&(psb, number, pcb, database))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut hasher = Sha256::new();
    hasher.update(b"mainframe-env.ims-primary-search@1\0");
    hasher.update(material);
    Ok(Some(hasher.finalize().into()))
}

/// Existing legacy request predicates converted only for the same SSA matcher
/// and traversal. Trusted Batch invocation is this route's context authority.
pub(super) fn ordinary_plan(
    state: &State,
    invocation: &Invocation,
    request: &ImsRequest,
    pcb: &ImsDatabasePcbMetadata,
    search: (&DatabaseEngine, &ReadRequest),
    position: &PcbPosition,
    limits: ImsLimits,
) -> Result<Option<crate::database::primary_position::PrimaryPlan>, HostProblem> {
    let (engine, read) = search;
    if invocation.service_class != ServiceClass::Batch {
        return Ok(None);
    }
    let session = state
        .sessions
        .get(invocation.run_unit_id.as_str())
        .ok_or(HostProblem::NotFound)?;
    let Some(digest) = primary_identity(state, &session.psb, request.pcb, pcb, engine, limits)?
    else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let mut target = read.target.as_deref();
    while let Some(name) = target {
        let segment = engine
            .definition()
            .segments
            .iter()
            .find(|s| s.name == name)
            .ok_or(HostProblem::Malformed)?;
        names.push(name.to_owned());
        target = segment.parent.as_deref();
    }
    names.reverse();
    let ssas = names
        .into_iter()
        .map(|name| {
            let predicates = read
                .path
                .iter()
                .filter(|s| s.segment == name)
                .flat_map(|s| &s.predicates)
                .map(|p| mainframe_env_host_api::ImsSsaPredicate {
                    field: mainframe_env_host_api::ImsSsaField::Named(p.field.clone()),
                    relation: mainframe_env_host_api::ImsSsaRelation::Equal,
                    value: p.value.clone(),
                })
                .collect::<Vec<_>>();
            ImsSsa {
                segment: name,
                command_format: false,
                commands: vec![],
                concatenated_key: None,
                connectors: vec![
                    mainframe_env_host_api::ImsSsaBoolean::DependentAnd;
                    predicates.len().saturating_sub(1)
                ],
                predicates,
            }
        })
        .collect();
    let plan = crate::database::primary_position::PrimaryPlan {
        digest,
        ssas,
        mask: [false; 2],
    };
    engine
        .validate_primary_plan(position, &plan)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(Some(plan))
}

pub(super) fn lookup_insert_parent(
    state: &State,
    invocation: &Invocation,
    request: &ImsRequest,
    pcb: &ImsDatabasePcbMetadata,
    search: (&DatabaseEngine, &str),
    position: &mut PcbPosition,
    limits: ImsLimits,
) -> Result<
    (
        Result<RecordView, EngineProblem>,
        Option<super::super::feedback::PrimaryCallWitness>,
    ),
    HostProblem,
> {
    let (engine, parent) = search;
    let mut ancestry = BTreeSet::new();
    let mut cursor = Some(parent);
    while let Some(name) = cursor {
        ancestry.insert(name.to_owned());
        cursor = engine
            .definition()
            .segments
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.parent.as_deref());
    }
    let mut lookup = request.clone();
    lookup.operation = ImsOperation::GetUnique;
    lookup.segments = vec![parent.to_owned()];
    lookup
        .qualifiers
        .retain(|q| ancestry.contains(&normalize(&q.segment)));
    let read = read_request(&lookup, engine)?;
    let plan = ordinary_plan(
        state,
        invocation,
        request,
        pcb,
        (engine, &read),
        position,
        limits,
    )?
    .ok_or(HostProblem::InfrastructureFailure)?;
    let found = engine.read_ssas_planned(position, &read, &plan.ssas, None, |_| true, Some(&plan));
    let witness = fresh_witness(position, true, &found);
    Ok((found, witness))
}

pub(super) fn fresh_witness(
    position: &PcbPosition,
    traced: bool,
    outcome: &Result<RecordView, EngineProblem>,
) -> Option<super::super::feedback::PrimaryCallWitness> {
    if traced && (outcome.is_ok() || matches!(outcome, Err(EngineProblem::NotFound))) {
        position
            .primary_feedback_path()
            .filter(|p| !p.is_empty())
            .map(|p| super::super::feedback::PrimaryCallWitness { path: p.to_vec() })
    } else {
        None
    }
}

fn primary_plan(
    state: &State,
    session: &Session,
    pcb: &ImsDatabasePcbMetadata,
    navigation: &ImsNavigationRequest,
    search: (&DatabaseEngine, &ReadRequest),
    ssas: &[ImsSsa],
    limits: ImsLimits,
) -> Result<Option<crate::database::primary_position::PrimaryPlan>, HostProblem> {
    let (engine, read) = search;
    let constrained = ssas
        .iter()
        .any(|s| s.commands.iter().any(|c| matches!(c.code, b'U' | b'V')));
    let candidate = (|| {
        if navigation.context != mainframe_env_host_api::ImsExecutionContext::DbBatch {
            return Ok(None);
        }
        let Some(digest) = primary_identity(
            state,
            &session.psb,
            navigation.request.pcb,
            pcb,
            engine,
            limits,
        )?
        else {
            return Ok(None);
        };
        if ssas.iter().any(|s| {
            s.concatenated_key.is_some()
                || s.connectors
                    .iter()
                    .any(|c| *c != mainframe_env_host_api::ImsSsaBoolean::DependentAnd)
                || s.commands
                    .iter()
                    .any(|c| !matches!(c.code, b'U' | b'V' | b'P'))
        }) {
            return Ok(None);
        }
        let position = pcb::position(session, navigation.request.pcb);
        if ssas.is_empty() {
            if constrained
                || position.primary_feedback_path().is_none()
                || !matches!(read.kind, ReadKind::Next | ReadKind::NextInParent)
            {
                return Ok(None);
            }
        } else {
            let expected = engine
                .definition()
                .segments
                .iter()
                .find(|s| Some(&s.name) == read.target.as_ref())
                .ok_or(HostProblem::Unsupported)?;
            let mut names = vec![expected.name.as_str()];
            let mut parent = expected.parent.as_deref();
            while let Some(name) = parent {
                names.push(name);
                parent = engine
                    .definition()
                    .segments
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or(HostProblem::Unsupported)?
                    .parent
                    .as_deref();
            }
            names.reverse();
            if names.len() != ssas.len()
                || names
                    .iter()
                    .zip(ssas)
                    .any(|(name, ssa)| *name != ssa.segment)
            {
                return Ok(None);
            }
        }
        let mut mask = [false; 2];
        if constrained {
            if read.kind != ReadKind::Next
                || ssas.len() != 3
                || ssas.iter().any(|s| {
                    !s.predicates.is_empty()
                        || s.commands.iter().any(|c| !matches!(c.code, b'U' | b'V'))
                })
                || !ssas[2].commands.is_empty()
                || ssas.iter().any(|s| s.commands.len() > 1)
            {
                return Ok(None);
            }
            let v_count = ssas
                .iter()
                .flat_map(|s| &s.commands)
                .filter(|c| c.code == b'V')
                .count();
            let u_count = ssas
                .iter()
                .flat_map(|s| &s.commands)
                .filter(|c| c.code == b'U')
                .count();
            if v_count > 1 || v_count > 0 && u_count > 0 {
                return Ok(None);
            }
            for (i, ssa) in ssas.iter().take(2).enumerate() {
                if let Some(c) = ssa.commands.first() {
                    mask[i] = true;
                    if c.code == b'V' {
                        mask[..=i].fill(true);
                    }
                }
            }
        }
        let plan = crate::database::primary_position::PrimaryPlan {
            digest,
            ssas: ssas.to_vec(),
            mask,
        };
        engine
            .validate_primary_plan(&position, &plan)
            .map_err(|_| HostProblem::Unsupported)?;
        Ok(Some(plan))
    })()?;
    if constrained && candidate.is_none() {
        return Err(HostProblem::Unsupported);
    }
    Ok(candidate)
}

/// The engine owns the live occurrence path; the scheduled catalog owns the
/// uniqueness and logical metadata omitted from its physical definition.
fn validate_direct_child_metadata(
    state: &State,
    pcb: &ImsDatabasePcbMetadata,
    navigation: &ImsNavigationRequest,
    engine: &DatabaseEngine,
    first: bool,
) -> Result<(), HostProblem> {
    if navigation.context != mainframe_env_host_api::ImsExecutionContext::DbBatch
        || !matches!(
            navigation.request.operation,
            ImsOperation::GetNextParent | ImsOperation::GetHoldNextParent
        )
        || pcb.secondary_index.is_some()
    {
        return Err(HostProblem::Unsupported);
    }
    let catalog = state.metadata.as_ref().ok_or(HostProblem::Unsupported)?;
    let name = normalize(&pcb.database);
    let database = catalog
        .databases
        .iter()
        .find(|db| normalize(&db.name) == name)
        .ok_or(HostProblem::Unsupported)?;
    if database.organization != crate::ImsDatabaseOrganization::Hidam
        || database.segments.len() != 2
        || first && !database.secondary_indexes.is_empty()
        || catalog
            .databases
            .iter()
            .flat_map(|db| &db.logical_relationships)
            .any(|relationship| {
                normalize(&relationship.parent_database) == name
                    || normalize(&relationship.child_database) == name
            })
        || definition(database)? != *engine.definition()
        || database.segments.iter().any(|segment| {
            let mut keys = segment.fields.iter().filter(|field| field.sequence);
            segment.min_length != segment.max_length
                || keys.next().is_none_or(|key| {
                    !key.unique
                        || key.name.is_none()
                        || key.length == 0
                        || key
                            .offset
                            .checked_add(key.length)
                            .is_none_or(|end| end > segment.min_length)
                })
                || keys.next().is_some()
        })
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
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
        if ssa.commands.iter().any(|c| {
            !matches!(
                c.code,
                b'C' | b'O' | b'D' | b'P' | b'F' | b'L' | b'U' | b'V'
            )
        }) || engine.definition().organization == crate::ImsDatabaseOrganization::Msdb
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
    if ssas
        .iter()
        .any(|ssa| ssa.commands.iter().any(|c| matches!(c.code, b'F' | b'L')))
    {
        let first = ssas
            .iter()
            .any(|ssa| ssa.commands.iter().any(|c| c.code == b'F'));
        validate_direct_child_metadata(state, pcb, navigation, &engine, first)?;
        engine
            .validate_last_direct_child(
                &pcb::position(session, navigation.request.pcb),
                &read,
                &ssas,
                secondary,
            )
            .map_err(|_| HostProblem::Unsupported)?;
    }
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
    let primary = primary_plan(
        state,
        session,
        pcb,
        navigation,
        (&engine, &read),
        &ssas,
        limits,
    )?;
    Ok(Prepared {
        ssas,
        read,
        sensitivity,
        primary,
    })
}

pub(in crate::service) fn read(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    prepared: &Prepared,
    limits: ImsLimits,
) -> Result<
    (
        ImsResult,
        Option<super::super::feedback::PrimaryCallWitness>,
    ),
    HostProblem,
> {
    let pcb = session_pcb(state, run, request.pcb)?.clone();
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    if let Some(code) = prepared.sensitivity {
        return Ok((status(code), None));
    }
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    let mut position = pcb::position(session, request.pcb);
    let parent_qualification = pcb::gnp_target_below_parent(&engine, &position, &prepared.read);
    let secondary = pcb.secondary_index.as_deref().map(normalize);
    let visible = |segment: &str| {
        prepared.read.target.is_some()
            || (allowed(&pcb, segment, request.operation) && !pcb::key_only(&pcb, segment))
    };
    let outcome = if let Some(plan) = &prepared.primary {
        engine.read_ssas_planned(
            &mut position,
            &prepared.read,
            &prepared.ssas,
            secondary.as_deref(),
            visible,
            Some(plan),
        )
    } else {
        engine.read_ssas(
            &mut position,
            &prepared.read,
            &prepared.ssas,
            secondary.as_deref(),
            visible,
        )
    };
    let witness = fresh_witness(&position, prepared.primary.is_some(), &outcome);
    pcb::set_position(session, request.pcb, position);
    let view = match outcome {
        Ok(view) => view,
        Err(EngineProblem::Unsupported) => return Err(HostProblem::Unsupported),
        Err(EngineProblem::PathMismatch) if parent_qualification => {
            return Ok((status("GE"), None));
        }
        Err(problem) => return Ok((status(engine_status(problem)), witness)),
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
    Ok((
        ImsResult {
            status: "  ".into(),
            segments,
            checkpoint_id: None,
            affected_segments: 0,
            system: None,
        },
        witness,
    ))
}
