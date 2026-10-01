use super::*;
use crate::database::{
    DatabaseDefinition, DatabaseEngine, EngineLimits, EngineProblem, FieldDefinition,
    FieldPredicate, InsertRequest, ReadKind, ReadRequest, RecordView, Relation,
    SecondaryIndexDefinition, SegmentDefinition, SegmentSelector,
};
use crate::{ImsDatabaseMetadata, ImsDatabasePcbMetadata, ImsPcbMetadata};

mod logical;

/// Ordered bulk image; each parent refers to an earlier record by zero-based index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsGenericLoadRecord {
    pub segment: String,
    pub parent: Option<usize>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsGenericLoadImage {
    pub database: String,
    pub records: Vec<ImsGenericLoadRecord>,
}

fn engine_limits(limits: ImsLimits) -> EngineLimits {
    EngineLimits {
        max_segments: limits.max_segments,
        max_fields_per_segment: ImsMetadataLimits::default().max_fields_per_segment,
        max_secondary_indexes: ImsMetadataLimits::default().max_named_fields_and_indexes,
        max_records: limits.max_roots,
        max_children_per_parent: limits.max_children_per_root,
        max_segment_bytes: limits.max_segment_bytes,
        max_name_bytes: 64,
        max_predicates: 64,
    }
}

fn definition(database: &ImsDatabaseMetadata) -> Result<DatabaseDefinition, HostProblem> {
    let organization = database.organization;
    let mut remaining = database.segments.clone();
    let mut segments = Vec::new();
    while !remaining.is_empty() {
        let index = remaining
            .iter()
            .position(|segment| {
                segment.parent.as_ref().is_none_or(|parent| {
                    segments
                        .iter()
                        .any(|placed: &SegmentDefinition| placed.name == normalize(parent))
                })
            })
            .ok_or(HostProblem::Malformed)?;
        let segment = remaining.remove(index);
        segments.push(SegmentDefinition {
            name: normalize(&segment.name),
            parent: segment.parent.as_deref().map(normalize),
            min_length: segment.min_length,
            max_length: segment.max_length,
            key_field: segment
                .fields
                .iter()
                .find(|field| field.sequence)
                .and_then(|field| field.name.as_deref())
                .map(normalize),
            fields: segment
                .fields
                .iter()
                .filter_map(|field| {
                    field.name.as_ref().map(|name| FieldDefinition {
                        name: normalize(name),
                        offset: field.offset,
                        length: field.length,
                    })
                })
                .collect(),
        });
    }
    let mut secondary_indexes = Vec::new();
    for index in &database.secondary_indexes {
        if index.source_fields.len() != 1
            || normalize(&index.source_segment) != normalize(&index.target_segment)
        {
            return Err(HostProblem::Unsupported);
        }
        secondary_indexes.push(SecondaryIndexDefinition {
            name: normalize(&index.name),
            source_segment: normalize(&index.source_segment),
            field: normalize(&index.source_fields[0]),
            unique: false,
        });
    }
    Ok(DatabaseDefinition {
        name: normalize(&database.name),
        organization,
        segments,
        secondary_indexes,
    })
}

pub(super) fn install_metadata(
    service: &ImsService,
    metadata: ImsMetadataCatalog,
) -> Result<String, HostProblem> {
    let identity = validate_ims_metadata(&metadata, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::Malformed)?
        .digest;
    let mut images = BTreeMap::new();
    for database in &metadata.databases {
        let engine = DatabaseEngine::new(definition(database)?, engine_limits(service.limits))
            .map_err(install_error)?;
        images.insert(normalize(&database.name), Arc::new(engine.image()));
    }
    let mut durable = service.lock()?;
    if let Some(current) = &durable.state.metadata {
        return if current == &metadata {
            Ok(identity)
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    if let Some(legacy) = &durable.state.definitions
        && (legacy
            .databases
            .iter()
            .any(|db| images.contains_key(&normalize(&db.name)))
            || legacy.psbs.iter().any(|psb| {
                metadata
                    .psbs
                    .iter()
                    .any(|candidate| normalize(&candidate.name) == normalize(&psb.name))
            }))
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    let mut next = durable.state.scoped_snapshot();
    next.metadata = Some(metadata);
    next.generic_databases = images;
    super::validate_state(&next, service.limits)?;
    service.persist(&mut durable, next)?;
    Ok(identity)
}

fn install_error(problem: EngineProblem) -> HostProblem {
    match problem {
        EngineProblem::LimitExceeded => HostProblem::ResourceExhausted,
        EngineProblem::Unsupported => HostProblem::Unsupported,
        _ => HostProblem::Malformed,
    }
}

pub(super) fn refresh_databases(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut versions = RowVersions::new();
    durable.state.generic_databases = load_row_map(
        store,
        GENERIC_DATABASE_NAMESPACE,
        limits.max_databases,
        limits,
        &mut versions,
    )?;
    durable
        .versions
        .retain(|(namespace, _), _| namespace != GENERIC_DATABASE_NAMESPACE);
    durable.versions.extend(versions);
    let mut versions = RowVersions::new();
    durable.state.generic_pending_undo = load_row_map(
        store,
        GENERIC_PENDING_NAMESPACE,
        limits.max_sessions,
        limits,
        &mut versions,
    )?;
    durable
        .versions
        .retain(|(namespace, _), _| namespace != GENERIC_PENDING_NAMESPACE);
    durable.versions.extend(versions);
    super::validate_state(&durable.state, limits)
}

fn scheduled_pcb<'a>(
    state: &'a State,
    psb_name: &str,
    pcb_number: u16,
) -> Result<(&'a str, &'a ImsDatabasePcbMetadata), HostProblem> {
    let psb = state
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .psbs
                .iter()
                .find(|psb| normalize(&psb.name) == normalize(psb_name))
        })
        .ok_or(HostProblem::NotFound)?;
    let index = usize::from(pcb_number)
        .checked_sub(1)
        .ok_or(HostProblem::Malformed)?;
    match psb.pcbs.get(index).ok_or(HostProblem::NotFound)? {
        ImsPcbMetadata::Database(pcb) => Ok((&psb.name, pcb)),
        _ => Err(HostProblem::Unsupported),
    }
}

fn session_pcb<'a>(state: &'a State, run: &str) -> Result<&'a ImsDatabasePcbMetadata, HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    if !session.generic {
        return Err(HostProblem::NotFound);
    }
    scheduled_pcb(state, &session.psb, session.pcb).map(|(_, pcb)| pcb)
}

fn session_database(state: &State, run: &str) -> Result<String, HostProblem> {
    Ok(normalize(&session_pcb(state, run)?.database))
}

pub(super) fn is_generic(state: &State, run: &str, request: &ImsRequest) -> bool {
    match request.operation {
        ImsOperation::Schedule => request.psb.as_deref().is_some_and(|name| {
            state.metadata.as_ref().is_some_and(|metadata| {
                metadata
                    .psbs
                    .iter()
                    .any(|psb| normalize(&psb.name) == normalize(name))
            })
        }),
        ImsOperation::Load => serde_json::from_slice::<ImsGenericLoadImage>(&request.data)
            .ok()
            .is_some_and(|image| {
                state
                    .generic_databases
                    .contains_key(&normalize(&image.database))
            }),
        ImsOperation::Unload => {
            request
                .psb
                .as_deref()
                .is_some_and(|name| state.generic_databases.contains_key(&normalize(name)))
                || state
                    .sessions
                    .get(run)
                    .is_some_and(|session| session.generic)
        }
        ImsOperation::Commit | ImsOperation::Rollback => {
            state.generic_pending_undo.contains_key(run)
                || state
                    .sessions
                    .get(run)
                    .is_some_and(|session| session.generic)
        }
        _ => state
            .sessions
            .get(run)
            .is_some_and(|session| session.generic),
    }
}

pub(super) fn resources(
    state: &State,
    invocation: &Invocation,
    request: &ImsRequest,
) -> Result<Vec<EnterpriseResource>, HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let mut resources = Vec::new();
    let psb = if request.operation == ImsOperation::Schedule {
        request.psb.as_deref().map(normalize)
    } else {
        state.sessions.get(run).map(|session| session.psb.clone())
    };
    if let Some(psb) = psb {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsPsb,
            psb,
            AccessIntent::Execute,
        )?);
    }
    let mut databases = BTreeSet::new();
    match request.operation {
        ImsOperation::Schedule => {
            let (_, pcb) = scheduled_pcb(
                state,
                request.psb.as_deref().ok_or(HostProblem::Malformed)?,
                request.pcb,
            )?;
            databases.insert(normalize(&pcb.database));
        }
        ImsOperation::Load => {
            let image: ImsGenericLoadImage =
                serde_json::from_slice(&request.data).map_err(|_| HostProblem::Malformed)?;
            databases.insert(normalize(&image.database));
        }
        ImsOperation::Unload => {
            databases.insert(
                request
                    .psb
                    .as_deref()
                    .map(normalize)
                    .or_else(|| session_database(state, run).ok())
                    .ok_or(HostProblem::NotFound)?,
            );
        }
        ImsOperation::Commit | ImsOperation::Rollback => {
            if let Some(pending) = state.generic_pending_undo.get(run) {
                databases.extend(pending.keys().cloned());
            }
            if let Some(pending) = state.pending_undo.get(run) {
                databases.extend(pending.keys().cloned());
            }
        }
        ImsOperation::Terminate | ImsOperation::Checkpoint => {
            if let Ok(name) = session_database(state, run) {
                databases.insert(name);
            }
        }
        _ => {
            databases.insert(session_database(state, run)?);
        }
    }
    if databases.is_empty()
        && matches!(
            request.operation,
            ImsOperation::Commit | ImsOperation::Rollback
        )
    {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsUnitOfWork,
            "CURRENT",
            AccessIntent::Update,
        )?);
    }
    let intent = if request.operation.is_mutating() {
        AccessIntent::Update
    } else {
        AccessIntent::Read
    };
    let connected = databases
        .iter()
        .map(|name| logical::related_databases(state, name))
        .collect::<Result<Vec<_>, _>>()?;
    for name in &databases {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsDatabase,
            name,
            intent,
        )?);
    }
    let related_intent = if request.operation == ImsOperation::Delete {
        AccessIntent::Update
    } else {
        AccessIntent::Read
    };
    let connected = connected.into_iter().flatten().collect::<BTreeSet<_>>();
    for name in connected.difference(&databases) {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsDatabase,
            name,
            related_intent,
        )?);
    }
    Ok(resources)
}

pub(super) fn apply_request(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    match request.operation {
        ImsOperation::Schedule => {
            if state.sessions.contains_key(run) {
                return Ok(status("TC"));
            }
            if state.sessions.len() >= limits.max_sessions {
                return Err(HostProblem::ResourceExhausted);
            }
            let (psb, _) = scheduled_pcb(
                state,
                request.psb.as_deref().ok_or(HostProblem::Malformed)?,
                request.pcb,
            )?;
            state.sessions.insert(
                run.into(),
                Arc::new(Session {
                    psb: normalize(psb),
                    pcb: request.pcb,
                    generic: true,
                    root_position: 0,
                    child_position: 0,
                    current_root: None,
                    last: None,
                    position: PcbPosition::default(),
                }),
            );
            Ok(status("  "))
        }
        ImsOperation::Terminate => {
            state.sessions.remove(run).ok_or(HostProblem::NotFound)?;
            Ok(status("  "))
        }
        ImsOperation::GetUnique
        | ImsOperation::GetNext
        | ImsOperation::GetNextParent
        | ImsOperation::GetHoldUnique
        | ImsOperation::GetHoldNext
        | ImsOperation::GetHoldNextParent => read(state, run, request, limits),
        ImsOperation::Insert | ImsOperation::Replace | ImsOperation::Delete => {
            mutate(state, run, request, limits)
        }
        ImsOperation::Checkpoint => checkpoint(state, run, request, limits),
        ImsOperation::Load => load(state, run, request, limits),
        ImsOperation::Unload => unload(state, run, request, limits),
        ImsOperation::Commit => {
            state.pending_undo.remove(run);
            state.generic_pending_undo.remove(run);
            Ok(status("  "))
        }
        ImsOperation::Rollback => {
            if let Some(pending) = state.pending_undo.remove(run) {
                for (name, image) in pending.iter() {
                    state.databases.insert(name.clone(), image.clone());
                }
            }
            if let Some(pending) = state.generic_pending_undo.remove(run) {
                for (name, image) in pending.iter() {
                    state.generic_databases.insert(name.clone(), image.clone());
                    reset_positions(state, name, None);
                }
            }
            if let Some(session) = state.sessions.get_mut(run) {
                Arc::make_mut(session).position = PcbPosition::default();
            }
            Ok(status("  "))
        }
    }
}

fn restored(state: &State, name: &str, limits: ImsLimits) -> Result<DatabaseEngine, HostProblem> {
    let image = state
        .generic_databases
        .get(name)
        .ok_or(HostProblem::NotFound)?;
    DatabaseEngine::restore((**image).clone(), engine_limits(limits))
        .map_err(|_| HostProblem::InfrastructureFailure)
}

fn read_request(request: &ImsRequest, engine: &DatabaseEngine) -> Result<ReadRequest, HostProblem> {
    let (kind, hold) = match request.operation {
        ImsOperation::GetUnique => (ReadKind::Unique, false),
        ImsOperation::GetNext => (ReadKind::Next, false),
        ImsOperation::GetNextParent => (ReadKind::NextInParent, false),
        ImsOperation::GetHoldUnique => (ReadKind::Unique, true),
        ImsOperation::GetHoldNext => (ReadKind::Next, true),
        ImsOperation::GetHoldNextParent => (ReadKind::NextInParent, true),
        _ => return Err(HostProblem::Malformed),
    };
    let target = request
        .segments
        .last()
        .map(|name| normalize(name))
        .or_else(|| {
            (kind == ReadKind::Unique).then(|| engine.definition().segments[0].name.clone())
        });
    let mut path = Vec::<SegmentSelector>::new();
    for qualifier in &request.qualifiers {
        let name = normalize(&qualifier.segment);
        let index = path
            .iter()
            .position(|selector| selector.segment == name)
            .unwrap_or_else(|| {
                path.push(SegmentSelector {
                    segment: name,
                    predicates: Vec::new(),
                });
                path.len() - 1
            });
        path[index].predicates.push(FieldPredicate {
            field: normalize(&qualifier.field),
            relation: Relation::Equal,
            value: qualifier.value.clone(),
        });
    }
    if !path.is_empty() && target.is_none() {
        return Err(HostProblem::Malformed);
    }
    Ok(ReadRequest {
        kind,
        target,
        path,
        hold,
    })
}

fn allowed(pcb: &ImsDatabasePcbMetadata, segment: &str, operation: ImsOperation) -> bool {
    let Some(sensitive) = pcb
        .sensitive_segments
        .iter()
        .find(|item| normalize(&item.name) == segment)
    else {
        return false;
    };
    let options = sensitive
        .processing_options
        .as_deref()
        .unwrap_or(&pcb.processing_options);
    let required = match operation {
        ImsOperation::Insert => 'I',
        ImsOperation::Replace => 'R',
        ImsOperation::Delete => 'D',
        _ => 'G',
    };
    options.contains('A')
        || options.contains(required)
        || (required == 'G' && (options.contains('R') || options.contains('D')))
}

fn segment_result(engine: &DatabaseEngine, view: RecordView) -> Result<ImsSegment, HostProblem> {
    let parent_key = if let Some(parent) = view.parent {
        let path = engine
            .path_to(parent)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let parent = path.last().ok_or(HostProblem::InfrastructureFailure)?;
        let definition = engine
            .definition()
            .segments
            .iter()
            .find(|segment| segment.name == parent.segment)
            .ok_or(HostProblem::InfrastructureFailure)?;
        definition
            .key_field
            .as_ref()
            .and_then(|key| definition.fields.iter().find(|field| &field.name == key))
            .and_then(|field| parent.data.get(field.offset..field.offset + field.length))
            .map(<[u8]>::to_vec)
    } else {
        None
    };
    Ok(ImsSegment {
        name: view.segment,
        parent_key,
        data: view.data,
    })
}

fn read(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let pcb = session_pcb(state, run)?.clone();
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    let read = read_request(request, &engine)?;
    if let Some(target) = &read.target
        && !allowed(&pcb, target, request.operation)
    {
        return Ok(status("AC"));
    }
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    match engine.read(&mut session.position, &read) {
        Ok(view) => Ok(ImsResult {
            status: "  ".into(),
            segments: vec![logical::segment_result(state, limits, &engine, view)?],
            checkpoint_id: None,
            affected_segments: 0,
        }),
        Err(problem) => Ok(status(engine_status(problem))),
    }
}

fn engine_status(problem: EngineProblem) -> &'static str {
    match problem {
        EngineProblem::NotFound => "GE",
        EngineProblem::EndOfDatabase => "GB",
        EngineProblem::Duplicate | EngineProblem::IndexConflict => "II",
        EngineProblem::KeyChange => "DA",
        EngineProblem::HoldRequired | EngineProblem::StaleHold => "DJ",
        EngineProblem::ParentageRequired | EngineProblem::PathMismatch => "GP",
        EngineProblem::InvalidRequest | EngineProblem::InvalidDefinition => "AK",
        EngineProblem::InvalidData => "AT",
        EngineProblem::Unsupported => "AC",
        EngineProblem::LimitExceeded => "FM",
    }
}

fn begin_unit(state: &mut State, run: &str, name: &str) -> Result<(), HostProblem> {
    let image = state
        .generic_databases
        .get(name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    Arc::make_mut(state.generic_pending_undo.entry(run.into()).or_default())
        .entry(name.into())
        .or_insert(image);
    Ok(())
}

fn reset_positions(state: &mut State, database: &str, except: Option<&str>) {
    let runs = state
        .sessions
        .keys()
        .filter(|run| except != Some(run.as_str()))
        .filter(|run| session_database(state, run).as_deref() == Ok(database))
        .cloned()
        .collect::<Vec<_>>();
    for run in runs {
        if let Some(session) = state.sessions.get_mut(&run) {
            Arc::make_mut(session).position = PcbPosition::default();
        }
    }
}

fn mutate(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let pcb = session_pcb(state, run)?.clone();
    let name = normalize(&pcb.database);
    let mut engine = restored(state, &name, limits)?;
    let mut position = state
        .sessions
        .get(run)
        .ok_or(HostProblem::NotFound)?
        .position
        .clone();
    let target = if request.operation == ImsOperation::Insert {
        request
            .segments
            .last()
            .map(|segment| normalize(segment))
            .ok_or(HostProblem::Malformed)?
    } else {
        let Some(current) = position.current() else {
            return Ok(status("DJ"));
        };
        engine
            .path_to(current)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .last()
            .ok_or(HostProblem::InfrastructureFailure)?
            .segment
            .clone()
    };
    if !allowed(&pcb, &target, request.operation) {
        return Ok(status("AC"));
    }
    if request.operation == ImsOperation::Delete {
        let (count, images) = match logical::delete_cascade(state, limits, &name, &mut position) {
            Ok(changed) => changed,
            Err(logical::LogicalMutationError::Status(code)) => return Ok(status(code)),
            Err(logical::LogicalMutationError::Host(problem)) => return Err(problem),
        };
        for (database, image) in images {
            begin_unit(state, run, &database)?;
            state
                .generic_databases
                .insert(database.clone(), Arc::new(image.image()));
            reset_positions(state, &database, Some(run));
        }
        Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).position =
            position;
        return Ok(affected(count as u64));
    }
    let changed = match request.operation {
        ImsOperation::Insert => {
            let segment = engine
                .definition()
                .segments
                .iter()
                .find(|segment| segment.name == target);
            let Some(segment) = segment else {
                return Ok(status("AK"));
            };
            let parent = match &segment.parent {
                None => None,
                Some(parent_name) if request.qualifiers.is_empty() => {
                    position.current().or(position.parentage()).and_then(|id| {
                        engine.path_to(id).ok().and_then(|path| {
                            path.into_iter()
                                .rev()
                                .find(|view| &view.segment == parent_name)
                                .map(|view| view.id)
                        })
                    })
                }
                Some(parent_name) => {
                    let mut ancestry = BTreeSet::new();
                    let mut cursor = Some(parent_name.as_str());
                    while let Some(name) = cursor {
                        ancestry.insert(name.to_string());
                        cursor = engine
                            .definition()
                            .segments
                            .iter()
                            .find(|candidate| candidate.name == name)
                            .and_then(|candidate| candidate.parent.as_deref());
                    }
                    let mut lookup = request.clone();
                    lookup.operation = ImsOperation::GetUnique;
                    lookup.segments = vec![parent_name.clone()];
                    lookup
                        .qualifiers
                        .retain(|qualifier| ancestry.contains(&normalize(&qualifier.segment)));
                    let query = read_request(&lookup, &engine)?;
                    match engine.read(&mut PcbPosition::default(), &query) {
                        Ok(view) => Some(view.id),
                        Err(problem) => return Ok(status(engine_status(problem))),
                    }
                }
            };
            if segment.parent.is_some() && parent.is_none() {
                return Ok(status("GP"));
            }
            let inserted = engine.insert(InsertRequest {
                segment: target,
                parent,
                data: request.data.clone(),
            });
            match inserted {
                Ok(view) => {
                    match logical::link_insert(state, limits, &name, &view, request, &mut engine) {
                        Ok(()) => {}
                        Err(logical::LogicalMutationError::Status(code)) => {
                            return Ok(status(code));
                        }
                        Err(logical::LogicalMutationError::Host(problem)) => return Err(problem),
                    }
                    position.set_current(view.id);
                    Ok(1usize)
                }
                Err(problem) => Err(problem),
            }
        }
        ImsOperation::Replace => engine.replace(&mut position, &request.data).map(|_| 1),
        _ => unreachable!(),
    };
    let count = match changed {
        Ok(count) => count,
        Err(problem) => return Ok(status(engine_status(problem))),
    };
    begin_unit(state, run, &name)?;
    state
        .generic_databases
        .insert(name, Arc::new(engine.image()));
    Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).position = position;
    Ok(affected(count as u64))
}

fn checkpoint(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let id = request
        .checkpoint_id
        .clone()
        .ok_or(HostProblem::Malformed)?;
    if state.checkpoints.len() >= limits.max_checkpoints && !state.checkpoints.contains_key(&id) {
        return Err(HostProblem::ResourceExhausted);
    }
    let session = state
        .sessions
        .get(run)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    state.checkpoints.insert(id.clone(), session);
    Ok(ImsResult {
        status: "  ".into(),
        segments: Vec::new(),
        checkpoint_id: Some(id),
        affected_segments: 0,
    })
}

fn load(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let image: ImsGenericLoadImage =
        serde_json::from_slice(&request.data).map_err(|_| HostProblem::Malformed)?;
    let name = normalize(&image.database);
    let prior = restored(state, &name, limits)?;
    let mut engine = DatabaseEngine::new(prior.definition().clone(), engine_limits(limits))
        .map_err(install_error)?;
    let mut ids = Vec::new();
    for record in &image.records {
        let parent = record
            .parent
            .map(|index| ids.get(index).copied().ok_or(HostProblem::Malformed))
            .transpose()?;
        let view = engine
            .insert_loaded(InsertRequest {
                segment: normalize(&record.segment),
                parent,
                data: record.data.clone(),
            })
            .map_err(install_error)?;
        ids.push(view.id);
    }
    begin_unit(state, run, &name)?;
    state
        .generic_databases
        .insert(name, Arc::new(engine.image()));
    let name = normalize(&image.database);
    reset_positions(state, &name, None);
    Ok(affected(ids.len() as u64))
}

fn unload(
    state: &State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let name = request
        .psb
        .as_deref()
        .map(normalize)
        .or_else(|| session_database(state, run).ok())
        .ok_or(HostProblem::NotFound)?;
    let engine = restored(state, &name, limits)?;
    let segments = engine
        .ordered_records()
        .into_iter()
        .take(request.max_segments as usize)
        .map(|view| segment_result(&engine, view))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImsResult {
        status: "  ".into(),
        segments,
        checkpoint_id: None,
        affected_segments: 0,
    })
}

pub(super) fn validate_state(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    if state.generic_databases.len() > limits.max_databases
        || state.generic_pending_undo.len() > limits.max_sessions
        || state.generic_pending_undo.keys().any(String::is_empty)
        || state
            .generic_pending_undo
            .values()
            .any(|pending| pending.len() > limits.max_databases)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let Some(metadata) = &state.metadata else {
        return if state.generic_databases.is_empty()
            && state.generic_pending_undo.is_empty()
            && state
                .sessions
                .values()
                .chain(state.checkpoints.values())
                .all(|session| !session.generic)
        {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        };
    };
    validate_ims_metadata(metadata, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let names = metadata
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if names != state.generic_databases.keys().cloned().collect() {
        return Err(HostProblem::InfrastructureFailure);
    }
    for (name, image) in &state.generic_databases {
        let engine = DatabaseEngine::restore((**image).clone(), engine_limits(limits))
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let source = metadata
            .databases
            .iter()
            .find(|database| normalize(&database.name) == *name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if engine.definition()
            != &definition(source).map_err(|_| HostProblem::InfrastructureFailure)?
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    logical::validate_links(state, limits)?;
    for pending in state.generic_pending_undo.values() {
        for (name, image) in pending.iter() {
            let engine = DatabaseEngine::restore((**image).clone(), engine_limits(limits))
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let source = metadata
                .databases
                .iter()
                .find(|database| normalize(&database.name) == *name)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if engine.definition()
                != &definition(source).map_err(|_| HostProblem::InfrastructureFailure)?
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
    }
    for session in state.sessions.values().filter(|session| session.generic) {
        let pcb = scheduled_pcb(state, &session.psb, session.pcb)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .1;
        let engine = restored(state, &normalize(&pcb.database), limits)?;
        engine
            .validate_position(&session.position)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    for session in state.checkpoints.values().filter(|session| session.generic) {
        scheduled_pcb(state, &session.psb, session.pcb)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IMS_METADATA_SCHEMA_V1, ImsDatabaseOrganization, ImsDbLevel, ImsFieldMetadata,
        ImsLogicalRelationshipMetadata, ImsPsbMetadata, ImsSecondaryIndexMetadata,
        ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
    };
    use mainframe_env_execution_api::{
        ArtifactRef, ExecutionId, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId,
        Selector, TraceId,
    };
    use mainframe_env_host_api::{ImsQualifier, Mutation};
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

    mod closure_tests;

    fn catalog() -> ImsMetadataCatalog {
        fn field(name: &str, offset: usize, sequence: bool) -> ImsFieldMetadata {
            ImsFieldMetadata {
                name: Some(name.into()),
                offset,
                length: if sequence { 2 } else { 1 },
                sequence,
                unique: sequence,
            }
        }
        fn segment(name: &str, parent: Option<&str>, key: &str) -> ImsSegmentMetadata {
            ImsSegmentMetadata {
                name: name.into(),
                parent: parent.map(str::to_owned),
                min_length: 3,
                max_length: 3,
                fields: vec![field(key, 0, true), field("KIND", 2, false)],
            }
        }
        ImsMetadataCatalog {
            schema_version: IMS_METADATA_SCHEMA_V1.into(),
            databases: vec![ImsDatabaseMetadata {
                name: "GENDB".into(),
                version: 1,
                organization: ImsDatabaseOrganization::Hidam,
                segments: vec![
                    segment("ROOT", None, "ROOTKEY"),
                    segment("CHILD", Some("ROOT"), "CHILDKEY"),
                ],
                secondary_indexes: vec![],
                logical_relationships: vec![],
            }],
            psbs: vec![ImsPsbMetadata {
                name: "GENPSB".into(),
                database_level: ImsDbLevel::Current,
                pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                    name: "GENPCB".into(),
                    database: "GENDB".into(),
                    database_version: Some(1),
                    processing_options: "AP".into(),
                    sensitive_segments: vec![
                        ImsSensitiveSegmentMetadata {
                            name: "ROOT".into(),
                            parent: None,
                            processing_options: None,
                        },
                        ImsSensitiveSegmentMetadata {
                            name: "CHILD".into(),
                            parent: Some("ROOT".into()),
                            processing_options: None,
                        },
                    ],
                })],
            }],
        }
    }

    fn invocation(run: &str) -> Invocation {
        let limits = InvocationLimits::default();
        let grants = ["host.ims.read", "host.ims.write"]
            .into_iter()
            .map(|name| CapabilityId::new(name, limits).unwrap())
            .collect();
        Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("ims:generic", limits).unwrap(),
            ArtifactRef::new("ims:generic", limits).unwrap(),
            Principal::new(PrincipalId::new("IBMUSER", limits).unwrap(), grants, limits).unwrap(),
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new(format!("trace-{run}"), limits).unwrap(),
            IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn request(
        run: &str,
        op: ImsOperation,
        sequence: u64,
        segments: &[&str],
        data: &[u8],
    ) -> ImsRequest {
        let limits = InvocationLimits::default();
        ImsRequest {
            operation: op,
            psb: (op == ImsOperation::Schedule).then(|| "GENPSB".into()),
            pcb: 1,
            segments: segments.iter().map(|s| (*s).into()).collect(),
            data: data.to_vec(),
            qualifiers: vec![],
            checkpoint_id: (op == ImsOperation::Checkpoint).then(|| format!("CHK-{run}")),
            max_segments: 64,
            mutation: op.is_mutating().then(|| Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(format!("{run}-{sequence}"), limits).unwrap(),
                transaction: Some("IMS-GENERIC".into()),
            }),
        }
    }

    fn execute(service: &ImsService, run: &str, req: &ImsRequest) -> ImsResult {
        service.execute(&invocation(run), req).unwrap()
    }

    fn qualifier(key: &[u8]) -> ImsQualifier {
        ImsQualifier {
            segment: "ROOT".into(),
            field: "ROOTKEY".into(),
            value: key.into(),
        }
    }

    fn exercise(store: Arc<dyn ProviderStateStore>) {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        service.install_metadata(catalog()).unwrap();
        assert_eq!(service.install_metadata(catalog()).unwrap().len(), 71);
        let run = "generic-run";
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b"")
            )
            .status,
            "  "
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X")
            )
            .affected_segments,
            1
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, 3, &["ROOT"], b"B2Y")
            )
            .affected_segments,
            1
        );
        let mut hold = request(run, ImsOperation::GetHoldUnique, 4, &["ROOT"], b"");
        hold.qualifiers.push(qualifier(b"A1"));
        assert_eq!(execute(&service, run, &hold).segments[0].data, b"A1X");
        let held = service.lock().unwrap().state.sessions[run].position.clone();
        assert!(held.is_held());
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 5, &[], b"A1Z")
            )
            .status,
            "  "
        );
        assert!(
            service.lock().unwrap().state.sessions[run]
                .position
                .is_held()
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 6, &[], b"A1Q")
            )
            .status,
            "  "
        );
        let mut failed = request(run, ImsOperation::GetUnique, 7, &["ROOT"], b"");
        failed.qualifiers.push(qualifier(b"ZZ"));
        assert_eq!(execute(&service, run, &failed).status, "GE");
        let failed_position = service.lock().unwrap().state.sessions[run].position.clone();
        assert_eq!(failed_position.current(), held.current());
        assert_eq!(failed_position.parentage(), None);
        assert!(!failed_position.is_held());
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 8, &[], b"A1R")
            )
            .status,
            "DJ"
        );
        assert_eq!(
            service.lock().unwrap().state.sessions[run].position,
            failed_position
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::GetNext, 9, &[], b"")
            )
            .segments[0]
                .data,
            b"B2Y"
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::GetNext, 10, &[], b"")
            )
            .status,
            "GB"
        );
        assert_eq!(
            service.lock().unwrap().state.sessions[run]
                .position
                .current(),
            None
        );
        let first_insert = request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X");
        assert_eq!(execute(&service, run, &first_insert).status, "  ");
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Rollback, 11, &[], b"")
            )
            .status,
            "  "
        );
        assert_eq!(
            service.lock().unwrap().state.generic_databases["GENDB"].clone(),
            Arc::new(
                DatabaseEngine::new(
                    definition(&catalog().databases[0]).unwrap(),
                    engine_limits(ImsLimits::default())
                )
                .unwrap()
                .image()
            )
        );
        drop(service);
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            execute(
                &reopened,
                run,
                &request(run, ImsOperation::GetNext, 12, &[], b"")
            )
            .status,
            "GB"
        );
        assert_eq!(execute(&reopened, run, &first_insert).status, "  ");
        assert_eq!(
            reopened.lock().unwrap().state.generic_databases["GENDB"].clone(),
            Arc::new(
                DatabaseEngine::new(
                    definition(&catalog().databases[0]).unwrap(),
                    engine_limits(ImsLimits::default())
                )
                .unwrap()
                .image()
            )
        );
    }

    #[test]
    fn public_generic_status_position_replay_rollback_and_memory_reopen() {
        exercise(Arc::new(MemoryStore::new(Default::default())));
    }

    #[test]
    fn public_generic_status_position_replay_rollback_and_sqlite_reopen() {
        let file = std::env::temp_dir().join(format!(
            "ims-generic-{}-{}.sqlite",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let url = format!("sqlite://{}?mode=rwc", file.display());
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        exercise(store);
        std::fs::remove_file(file).unwrap();
    }

    #[derive(Default)]
    struct Policy {
        deny_update: Mutex<bool>,
        seen: Mutex<Vec<EnterpriseResource>>,
    }

    impl EnterpriseAuthorizer for Policy {
        fn authorize(
            &self,
            _: &PrincipalId,
            resource: &EnterpriseResource,
        ) -> Result<(), HostProblem> {
            self.seen.lock().unwrap().push(resource.clone());
            if *self.deny_update.lock().unwrap()
                && resource.class == EnterpriseResourceClass::ImsDatabase
                && resource.intent == AccessIntent::Update
            {
                Err(HostProblem::Unauthorized)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn authorization_precedes_generic_mutation_and_replay() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
        service.install_metadata(catalog()).unwrap();
        let run = "policy-run";
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        *policy.deny_update.lock().unwrap() = true;
        let insert = request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X");
        assert_eq!(
            service.execute(&invocation(run), &insert),
            Err(HostProblem::Unauthorized)
        );
        let engine = restored(
            &service.lock().unwrap().state,
            "GENDB",
            ImsLimits::default(),
        )
        .unwrap();
        assert_eq!(engine.record_count(), 0);
        assert!(
            !service
                .lock()
                .unwrap()
                .state
                .replay
                .contains_key("policy-run-2")
        );
        assert!(
            policy
                .seen
                .lock()
                .unwrap()
                .iter()
                .any(|resource| resource.class == EnterpriseResourceClass::ImsDatabase)
        );
    }

    fn legacy_catalog() -> ImsApplicationDefinition {
        ImsApplicationDefinition {
            databases: vec![ImsDatabaseDefinition {
                name: "OLDDB".into(),
                access: "HIDAM".into(),
                secondary_index: None,
                segments: vec![ImsSegmentDefinition {
                    name: "ROOT".into(),
                    parent: None,
                    length: 3,
                    key_field: "ROOTKEY".into(),
                    key_offset: 0,
                    key_length: 2,
                }],
            }],
            psbs: vec![ImsPsbDefinition {
                name: "OLDPSB".into(),
                pcbs: vec![ImsPcbDefinition {
                    name: "OLDPCB".into(),
                    database: "OLDDB".into(),
                    processing_options: "AP".into(),
                    segments: vec!["ROOT".into()],
                }],
            }],
        }
    }

    fn corrupt_row(
        store: &dyn ProviderStateStore,
        namespace: &str,
        key: &str,
        field: &str,
        replacement: serde_json::Value,
    ) {
        let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        value["value"][field] = replacement;
        row.payload = serde_json::to_vec(&value).unwrap();
        let prior = row.version;
        row.version += 1;
        store.put_provider_state(row, Some(prior)).unwrap();
    }

    #[test]
    fn coexisting_catalogs_execute_and_legacy_corruption_is_checked() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        service.install(legacy_catalog()).unwrap();
        service.install_metadata(catalog()).unwrap();
        let run = "mixed-generic";
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let run = "mixed-legacy";
        let mut schedule = request(run, ImsOperation::Schedule, 1, &[], b"");
        schedule.psb = Some("OLDPSB".into());
        execute(&service, run, &schedule);
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"B2Y"),
        );
        assert_eq!(service.hierarchy("OLDDB").unwrap().len(), 1);
        assert_eq!(
            restored(
                &service.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .record_count(),
            1
        );
        drop(service);
        assert!(ImsService::open(store.clone(), ImsLimits::default()).is_ok());
        corrupt_row(
            &*store,
            DATABASE_NAMESPACE,
            "OLDDB",
            "secondary_index",
            serde_json::json!({"bad":"missing"}),
        );
        assert!(matches!(
            ImsService::open(store, ImsLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn corrupt_generic_image_is_rejected_on_reopen() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        ImsService::open(store.clone(), ImsLimits::default())
            .unwrap()
            .install_metadata(catalog())
            .unwrap();
        corrupt_row(
            &*store,
            GENERIC_DATABASE_NAMESPACE,
            "GENDB",
            "next_id",
            serde_json::json!(0),
        );
        assert!(matches!(
            ImsService::open(store, ImsLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn committed_generic_rows_survive_fresh_sqlite_connection() {
        let file = std::env::temp_dir().join(format!(
            "ims-generic-commit-{}-{}.sqlite",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let url = format!("sqlite://{}?mode=rwc", file.display());
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let service = ImsService::open(store, ImsLimits::default()).unwrap();
            service.install_metadata(catalog()).unwrap();
            let run = "commit-run";
            execute(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b""),
            );
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
            );
            execute(
                &service,
                run,
                &request(run, ImsOperation::Commit, 3, &[], b""),
            );
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let service = ImsService::open(store, ImsLimits::default()).unwrap();
            let mut get = request("commit-run", ImsOperation::GetUnique, 4, &["ROOT"], b"");
            get.qualifiers.push(qualifier(b"A1"));
            assert_eq!(
                execute(&service, "commit-run", &get).segments[0].data,
                b"A1X"
            );
            assert!(
                service
                    .lock()
                    .unwrap()
                    .state
                    .generic_pending_undo
                    .is_empty()
            );
        }
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn generic_child_hold_delete_index_and_bulk_image_use_typed_metadata() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        metadata.databases[0]
            .secondary_indexes
            .push(ImsSecondaryIndexMetadata {
                name: "ROOTKIND".into(),
                target_segment: "ROOT".into(),
                source_segment: "ROOT".into(),
                source_fields: vec!["KIND".into()],
            });
        service.install_metadata(metadata).unwrap();
        let run = "child-run";
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let mut root = request(run, ImsOperation::GetUnique, 3, &["ROOT"], b"");
        root.qualifiers.push(qualifier(b"A1"));
        execute(&service, run, &root);
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 4, &["CHILD"], b"C1Q"),
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::GetUnique, 5, &["ROOT"], b""),
        );
        let held = execute(
            &service,
            run,
            &request(run, ImsOperation::GetHoldNextParent, 6, &["CHILD"], b""),
        );
        assert_eq!(held.segments[0].data, b"C1Q");
        assert_eq!(held.segments[0].parent_key, Some(b"A1".to_vec()));
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 7, &[], b"C1Z")
            )
            .status,
            "  "
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Delete, 8, &[], b"")
            )
            .affected_segments,
            1
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::GetNextParent, 9, &["CHILD"], b"")
            )
            .status,
            "GE"
        );
        let engine = restored(
            &service.lock().unwrap().state,
            "GENDB",
            ImsLimits::default(),
        )
        .unwrap();
        assert_eq!(engine.lookup_index("ROOTKIND", b"X").unwrap().len(), 1);
        let image = ImsGenericLoadImage {
            database: "GENDB".into(),
            records: vec![
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"D1W".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"E1V".to_vec(),
                },
            ],
        };
        let load = request(
            run,
            ImsOperation::Load,
            10,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        );
        assert_eq!(execute(&service, run, &load).affected_segments, 2);
        let mut unload = request(run, ImsOperation::Unload, 11, &[], b"");
        unload.psb = Some("GENDB".into());
        assert_eq!(
            execute(&service, run, &unload)
                .segments
                .iter()
                .map(|segment| segment.data.clone())
                .collect::<Vec<_>>(),
            vec![b"D1W".to_vec(), b"E1V".to_vec()]
        );
    }
}
