//! Existing IMS definition and retained-state validation.

use super::*;

pub(super) fn validate_definition(
    definition: &ImsApplicationDefinition,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if definition.databases.is_empty()
        || definition.databases.len() > limits.max_databases
        || definition.psbs.is_empty()
        || definition.psbs.len() > limits.max_psbs
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let databases = definition
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if databases.len() != definition.databases.len() {
        return Err(HostProblem::Malformed);
    }
    for database in &definition.databases {
        if !matches!(normalize(&database.access).as_str(), "HIDAM" | "INDEX")
            || database.segments.is_empty()
            || database.segments.len() > limits.max_segments
        {
            return Err(HostProblem::Unsupported);
        }
        let names = database
            .segments
            .iter()
            .map(|segment| normalize(&segment.name))
            .collect::<BTreeSet<_>>();
        if names.len() != database.segments.len()
            || database.segments.iter().any(|segment| {
                segment.name.is_empty()
                    || segment.length == 0
                    || segment.length > limits.max_segment_bytes
                    || segment.key_length == 0
                    || segment
                        .key_offset
                        .checked_add(segment.key_length)
                        .is_none_or(|end| end > segment.length)
                    || segment
                        .parent
                        .as_ref()
                        .is_some_and(|parent| !names.contains(&normalize(parent)))
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    let mut psb_names = BTreeSet::new();
    for psb in &definition.psbs {
        if !psb_names.insert(normalize(&psb.name))
            || psb.pcbs.is_empty()
            || psb.pcbs.len() > limits.max_pcbs
            || psb.pcbs.iter().any(|pcb| {
                !databases.contains(&normalize(&pcb.database))
                    || pcb.segments.is_empty()
                    || pcb.processing_options.is_empty()
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

pub(super) fn validate_state(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    application_recovery::validate_sessions(state)?;
    if state.sessions.len() > limits.max_sessions
        || state.checkpoints.len() > limits.max_checkpoints
        || state.replay.len() > limits.max_replays
        || state.pending_undo.len() > limits.max_sessions
        || state.databases.len() > limits.max_databases
        || state.sessions.keys().any(String::is_empty)
        || state.checkpoints.keys().any(String::is_empty)
        || state.pending_undo.keys().any(String::is_empty)
        || state
            .replay
            .iter()
            .any(|(key, recorded)| validate_ims_recorded_result(key, recorded, limits).is_err())
        || state.databases.values().any(|database| {
            database.roots.len() > limits.max_roots
                || database.roots.values().any(|root| {
                    root.data.len() > limits.max_segment_bytes
                        || root.children.len() > limits.max_children_per_root
                        || root
                            .children
                            .values()
                            .any(|child| child.len() > limits.max_segment_bytes)
                })
        })
    {
        return Err(HostProblem::ResourceExhausted);
    }
    generic::validate_state(state, limits)?;
    system::validate_state(state, limits)?;
    if let (Some(legacy), Some(metadata)) = (&state.definitions, &state.metadata)
        && (legacy.databases.iter().any(|database| {
            metadata
                .databases
                .iter()
                .any(|candidate| normalize(&candidate.name) == normalize(&database.name))
        }) || legacy.psbs.iter().any(|psb| {
            metadata
                .psbs
                .iter()
                .any(|candidate| normalize(&candidate.name) == normalize(&psb.name))
        }))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let Some(definitions) = &state.definitions else {
        return if state.databases.is_empty()
            && state.sessions.values().all(|session| session.generic)
            && state.checkpoints.values().all(|session| session.generic)
            && state.pending_undo.is_empty()
        {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        };
    };
    validate_definition(definitions, limits)?;
    let defined = definitions
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if defined != state.databases.keys().cloned().collect()
        || state
            .databases
            .iter()
            .any(|(name, database)| !valid_database_state(definitions, name, database, limits))
        || state
            .sessions
            .values()
            .chain(state.checkpoints.values())
            .filter(|session| !session.generic)
            .any(|session| !valid_session(definitions, session))
        || state.pending_undo.values().any(|databases| {
            databases.iter().any(|(name, database)| {
                !defined.contains(name)
                    || !valid_database_state(definitions, name, database, limits)
            })
        })
        || state.replay.keys().any(String::is_empty)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

pub(super) fn valid_database_state(
    definitions: &ImsApplicationDefinition,
    name: &str,
    database: &DatabaseState,
    limits: ImsLimits,
) -> bool {
    let Some(definition) = definitions
        .databases
        .iter()
        .find(|definition| normalize(&definition.name) == name)
    else {
        return false;
    };
    let Some(root_definition) = definition
        .segments
        .iter()
        .find(|segment| segment.parent.is_none())
    else {
        return false;
    };
    let child_definition = definition.segments.iter().find(|segment| {
        segment
            .parent
            .as_ref()
            .is_some_and(|parent| normalize(parent) == normalize(&root_definition.name))
    });
    database.secondary_index.len() == database.roots.len()
        && database
            .secondary_index
            .iter()
            .all(|(key, root)| key == root && database.roots.contains_key(root))
        && database.roots.iter().all(|(key, root)| {
            validate_segment_data(&root.data, root_definition, limits).is_ok()
                && data_key(&root.data, root_definition).as_deref() == Ok(key.as_str())
                && root.children.iter().all(|(key, child)| {
                    child_definition.is_some_and(|definition| {
                        validate_segment_data(child, definition, limits).is_ok()
                            && data_key(child, definition).as_deref() == Ok(key.as_str())
                    })
                })
        })
}

pub(super) fn valid_session(definitions: &ImsApplicationDefinition, session: &Session) -> bool {
    definitions
        .psbs
        .iter()
        .find(|psb| normalize(&psb.name) == session.psb)
        .is_some_and(|psb| session.pcb > 0 && usize::from(session.pcb) <= psb.pcbs.len())
}
