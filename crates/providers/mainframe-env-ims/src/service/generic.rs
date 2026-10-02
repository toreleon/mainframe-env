use super::*;
use crate::database::{
    DatabaseDefinition, DatabaseEngine, EngineLimits, EngineProblem, FieldDefinition,
    FieldPredicate, InsertRequest, ReadKind, ReadRequest, RecordView, Relation,
    SecondaryIndexDefinition, SegmentDefinition, SegmentSelector,
};
use crate::{ImsDatabaseMetadata, ImsDatabasePcbMetadata, ImsPcbMetadata};

pub(super) mod gsam;
pub(super) mod integrity;
pub(super) mod isolation;
mod load_image;
mod logical;
pub(super) mod pcb;
mod secondary;
pub(super) mod ssa;

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

pub(super) fn engine_limits(limits: ImsLimits) -> EngineLimits {
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

pub(super) fn definition(
    database: &ImsDatabaseMetadata,
) -> Result<DatabaseDefinition, HostProblem> {
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
        secondary_indexes.push(SecondaryIndexDefinition {
            name: normalize(&index.name),
            source_segment: normalize(&index.source_segment),
            field: normalize(index.source_fields.first().ok_or(HostProblem::Malformed)?),
            additional_fields: index
                .source_fields
                .iter()
                .skip(1)
                .map(|field| normalize(field))
                .collect(),
            target_segment: (normalize(&index.target_segment) != normalize(&index.source_segment))
                .then(|| normalize(&index.target_segment)),
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
    service.validate_secondary_metadata(&metadata)?;
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
    isolation::refresh_sessions(store, limits, durable)?;
    system::reservations::refresh(store, limits, durable)?;
    isolation::refresh_undo(store, limits, durable)?;
    super::validate_state(&durable.state, limits)
}

pub(in crate::service) fn scheduled_pcb<'a>(
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

pub(in crate::service) fn session_pcb<'a>(
    state: &'a State,
    run: &str,
    number: u16,
) -> Result<&'a ImsDatabasePcbMetadata, HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    if !session.generic {
        return Err(HostProblem::NotFound);
    }
    scheduled_pcb(state, &session.psb, number).map(|(_, pcb)| pcb)
}

fn session_database(state: &State, run: &str, number: u16) -> Result<String, HostProblem> {
    Ok(normalize(&session_pcb(state, run, number)?.database))
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
        ImsOperation::Load => {
            request
                .psb
                .as_deref()
                .is_some_and(|name| state.generic_databases.contains_key(&normalize(name)))
                || load_image::decode(state, &request.data)
                    .ok()
                    .is_some_and(|image| {
                        state
                            .generic_databases
                            .contains_key(&normalize(&image.database))
                    })
        }
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
            let image = load_image::decode(state, &request.data)?;
            databases.insert(normalize(&image.database));
        }
        ImsOperation::Unload => {
            databases.insert(
                request
                    .psb
                    .as_deref()
                    .map(normalize)
                    .or_else(|| session_database(state, run, request.pcb).ok())
                    .ok_or(HostProblem::NotFound)?,
            );
        }
        ImsOperation::Commit | ImsOperation::Rollback => {
            databases.extend(system::reservations::owned_databases(state, run)?);
            if let Some(pending) = state.generic_pending_undo.get(run) {
                databases.extend(pending.keys().cloned());
            }
            if let Some(pending) = state.pending_undo.get(run) {
                databases.extend(pending.keys().cloned());
            }
        }
        ImsOperation::Terminate | ImsOperation::Checkpoint => {
            databases.extend(pcb::session_databases(state, run)?);
        }
        _ => {
            databases.insert(session_database(state, run, request.pcb)?);
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
                    pcb_positions: BTreeMap::new(),
                    system: system::SystemSession::default(),
                    recovery: application_recovery::ExecutionRecovery::default(),
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
            isolation::ensure_backout(state, run)?;
            state.pending_undo.remove(run);
            state.generic_pending_undo.remove(run);
            Ok(status("  "))
        }
        ImsOperation::Rollback => {
            isolation::ensure_backout(state, run)?;
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
                pcb::clear_positions(Arc::make_mut(session));
            }
            Ok(status("  "))
        }
        ImsOperation::System => Err(HostProblem::Malformed),
    }
}

pub(in crate::service) fn restored(
    state: &State,
    name: &str,
    limits: ImsLimits,
) -> Result<DatabaseEngine, HostProblem> {
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

pub(in crate::service) fn allowed(
    pcb: &ImsDatabasePcbMetadata,
    segment: &str,
    operation: ImsOperation,
) -> bool {
    if pcb.processing_options.contains('O')
        && matches!(
            operation,
            ImsOperation::Insert | ImsOperation::Replace | ImsOperation::Delete
        )
    {
        return false;
    }
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
    let pcb = session_pcb(state, run, request.pcb)?.clone();
    let engine = restored(state, &normalize(&pcb.database), limits)?;
    let read = read_request(request, &engine)?;
    if let Some(code) = pcb::read_status(&pcb, request, &read) {
        return Ok(status(code));
    }
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    let mut position = pcb::position(session, request.pcb);
    let parent_qualification = pcb::gnp_target_below_parent(&engine, &position, &read);
    let visible = |segment: &str| {
        read.target.is_some()
            || (allowed(&pcb, segment, request.operation) && !pcb::key_only(&pcb, segment))
    };
    let outcome = if let Some(index) = &pcb.secondary_index {
        engine.read_secondary_visible(&normalize(index), &mut position, &read, visible)
    } else {
        engine.read_visible(&mut position, &read, visible)
    };
    pcb::set_position(session, request.pcb, position);
    match outcome {
        Ok(_)
            if read
                .target
                .as_deref()
                .is_some_and(|target| pcb::key_only(&pcb, target)) =>
        {
            Ok(status("  "))
        }
        Ok(view) => Ok(ImsResult {
            status: "  ".into(),
            segments: vec![logical::segment_result(state, limits, &engine, view)?],
            checkpoint_id: None,
            affected_segments: 0,
            system: None,
        }),
        Err(EngineProblem::PathMismatch) if parent_qualification => Ok(status("GE")),
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

pub(super) fn reset_positions(state: &mut State, database: &str, except: Option<&str>) {
    pcb::reset_database_positions(state, database, except);
}

fn mutate(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let pcb = session_pcb(state, run, request.pcb)?.clone();
    let name = normalize(&pcb.database);
    let mut engine = restored(state, &name, limits)?;
    let mut position = pcb::position(
        state.sessions.get(run).ok_or(HostProblem::NotFound)?,
        request.pcb,
    );
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
        return Ok(status("AM"));
    }
    if secondary::restricted_mutation(&engine, &pcb, &target, request.operation) {
        return Ok(status("AM"));
    }
    isolation::ensure_writer(state, run, &name)?;
    if request.operation == ImsOperation::Delete {
        let (count, images) = match logical::delete_cascade(state, limits, &name, &mut position) {
            Ok(changed) => changed,
            Err(logical::LogicalMutationError::Status(code)) => return Ok(status(code)),
            Err(logical::LogicalMutationError::Host(problem)) => return Err(problem),
        };
        for (database, image) in images {
            isolation::publish_image(state, run, &database, image.image(), limits)?;
            reset_positions(state, &database, Some(run));
            pcb::reset_deleted_positions(state, run, request.pcb, &database, limits)?;
        }
        pcb::set_position(
            Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
            request.pcb,
            position,
        );
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
                    let found = if let Some(index) = &pcb.secondary_index {
                        engine.read_secondary_visible(
                            &normalize(index),
                            &mut PcbPosition::default(),
                            &query,
                            |_| true,
                        )
                    } else {
                        engine.read(&mut PcbPosition::default(), &query)
                    };
                    match found {
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
                    if let Some(index) = &pcb.secondary_index {
                        engine
                            .position_after_secondary_insert(
                                &normalize(index),
                                &mut position,
                                view.id,
                            )
                            .map(|()| 1usize)
                    } else {
                        position.set_current(view.id);
                        Ok(1usize)
                    }
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
    isolation::publish_image(state, run, &name, engine.image(), limits)?;
    pcb::set_position(
        Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?),
        request.pcb,
        position,
    );
    Ok(affected(count as u64))
}

fn checkpoint(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let psb = &state.sessions.get(run).ok_or(HostProblem::NotFound)?.psb;
    super::application_recovery::gsam_checkpoint::reject_basic(state, psb)?;
    let id = request
        .checkpoint_id
        .clone()
        .ok_or(HostProblem::Malformed)?;
    if state.checkpoints.len() >= limits.max_checkpoints && !state.checkpoints.contains_key(&id) {
        return Err(HostProblem::ResourceExhausted);
    }
    state.pending_undo.remove(run);
    state.generic_pending_undo.remove(run);
    // Preserve the basic checkpoint boundary across all selected DB PCBs.
    let session = Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?);
    pcb::clear_positions(session);
    state
        .checkpoints
        .insert(id.clone(), Arc::new(session.clone()));
    Ok(ImsResult {
        status: "  ".into(),
        segments: Vec::new(),
        checkpoint_id: Some(id),
        affected_segments: 0,
        system: None,
    })
}

fn load(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let image = load_image::decode(state, &request.data)?;
    let name = normalize(&image.database);
    let prior = restored(state, &name, limits)?;
    system::reservations::ensure_no_reservations(state, &name)?;
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
    isolation::publish_image(state, run, &name, engine.image(), limits)?;
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
        .or_else(|| session_database(state, run, request.pcb).ok())
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
        system: None,
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
                .all(|session| !session.generic && session.pcb_positions.is_empty())
        {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        };
    };
    validate_ims_metadata(metadata, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    secondary::validate_catalog(metadata).map_err(|_| HostProblem::InfrastructureFailure)?;
    let names = metadata
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    let image_names = metadata
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if !state
        .generic_databases
        .keys()
        .all(|name| names.contains(name))
        || !image_names.is_subset(&state.generic_databases.keys().cloned().collect())
    {
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
        pending.validate(limits)?;
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
    pcb::validate_sessions(state, limits)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
