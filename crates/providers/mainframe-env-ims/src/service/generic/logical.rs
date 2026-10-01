//! Logical child occurrence links live in the child database image. The
//! service publishes all affected images in one ProviderStateStore CAS batch.

use super::*;
use crate::ImsLogicalRelationshipMetadata;
use crate::database::{LogicalLink, PcbPosition, RecordId};

pub(super) enum LogicalMutationError {
    Status(&'static str),
    Host(HostProblem),
}

impl From<HostProblem> for LogicalMutationError {
    fn from(value: HostProblem) -> Self {
        Self::Host(value)
    }
}

fn relationships(state: &State) -> Result<Vec<ImsLogicalRelationshipMetadata>, HostProblem> {
    let metadata = state
        .metadata
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut found = BTreeMap::new();
    for database in &metadata.databases {
        for relationship in &database.logical_relationships {
            let key = (
                normalize(&relationship.child_database),
                normalize(&relationship.child_segment),
                normalize(&relationship.parent_database),
                normalize(&relationship.parent_segment),
            );
            if let Some(prior) = found.insert(key, relationship.clone())
                && prior.paired != relationship.paired
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
    }
    Ok(found.into_values().collect())
}

pub(super) fn related_databases(
    state: &State,
    database: &str,
) -> Result<BTreeSet<String>, HostProblem> {
    let mut names = BTreeSet::new();
    for relationship in relationships(state)? {
        let parent = normalize(&relationship.parent_database);
        let child = normalize(&relationship.child_database);
        if child == database {
            names.insert(parent.clone());
        }
        if parent == database {
            names.insert(child);
        }
    }
    Ok(names)
}

pub(super) fn link_insert(
    state: &State,
    limits: ImsLimits,
    database: &str,
    view: &RecordView,
    request: &ImsRequest,
    engine: &mut DatabaseEngine,
) -> Result<(), LogicalMutationError> {
    for relationship in relationships(state)? {
        if normalize(&relationship.child_database) != database
            || normalize(&relationship.child_segment) != view.segment
        {
            continue;
        }
        let parent_database = normalize(&relationship.parent_database);
        let parent_segment = normalize(&relationship.parent_segment);
        let parent_engine = if parent_database == database {
            engine.clone()
        } else {
            restored(state, &parent_database, limits)?
        };
        let fields = parent_engine
            .definition()
            .segments
            .iter()
            .find(|segment| segment.name == parent_segment)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let predicates = request
            .qualifiers
            .iter()
            .filter(|qualifier| normalize(&qualifier.segment) == parent_segment)
            .collect::<Vec<_>>();
        if predicates.is_empty() {
            return Err(LogicalMutationError::Status("GP"));
        }
        let mut matching = parent_engine
            .ordered_records()
            .into_iter()
            .filter(|candidate| candidate.segment == parent_segment)
            .filter(|candidate| {
                predicates.iter().all(|qualifier| {
                    fields
                        .fields
                        .iter()
                        .find(|field| field.name == normalize(&qualifier.field))
                        .and_then(|field| {
                            candidate
                                .data
                                .get(field.offset..field.offset + field.length)
                        })
                        == Some(qualifier.value.as_slice())
                })
            });
        let Some(parent) = matching.next() else {
            return Err(LogicalMutationError::Status("GP"));
        };
        if matching.next().is_some() {
            return Err(LogicalMutationError::Status("II"));
        }
        if parent_database == database && parent.id == view.id {
            return Err(LogicalMutationError::Status("GP"));
        }
        engine
            .add_logical_link(LogicalLink {
                child: view.id,
                parent_database,
                parent_segment,
                parent: parent.id,
                paired: relationship.paired,
            })
            .map_err(|problem| LogicalMutationError::Status(engine_status(problem)))?;
    }
    Ok(())
}

pub(super) fn segment_result(
    state: &State,
    limits: ImsLimits,
    engine: &DatabaseEngine,
    view: RecordView,
) -> Result<ImsSegment, HostProblem> {
    let mut segment = super::segment_result(engine, view.clone())?;
    for link in engine
        .logical_links()
        .iter()
        .filter(|link| link.child == view.id)
    {
        let parent = restored(state, &link.parent_database, limits)?
            .path_to(link.parent)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .pop()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if parent.segment != link.parent_segment {
            return Err(HostProblem::InfrastructureFailure);
        }
        segment.data.extend_from_slice(&parent.data);
        if segment.data.len() > limits.max_segment_bytes.saturating_mul(2) {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(segment)
}

pub(super) fn validate_links(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    let declared = relationships(state)?;
    for (name, image) in &state.generic_databases {
        let engine = DatabaseEngine::restore((**image).clone(), engine_limits(limits))
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        for relationship in declared
            .iter()
            .filter(|relationship| normalize(&relationship.child_database) == *name)
        {
            let child_segment = normalize(&relationship.child_segment);
            let parent_database = normalize(&relationship.parent_database);
            let parent_segment = normalize(&relationship.parent_segment);
            for child in engine
                .export_records()
                .into_iter()
                .filter(|record| record.segment == child_segment)
            {
                if !engine.logical_links().iter().any(|link| {
                    link.child == child.id
                        && link.parent_database == parent_database
                        && link.parent_segment == parent_segment
                }) {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
        }
        for link in engine.logical_links() {
            let child = engine
                .path_to(link.child)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .pop()
                .ok_or(HostProblem::InfrastructureFailure)?;
            if !declared.iter().any(|relationship| {
                normalize(&relationship.child_database) == *name
                    && normalize(&relationship.child_segment) == child.segment
                    && normalize(&relationship.parent_database) == link.parent_database
                    && normalize(&relationship.parent_segment) == link.parent_segment
                    && relationship.paired == link.paired
            }) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let parent = restored(state, &link.parent_database, limits)?
                .path_to(link.parent)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .pop()
                .ok_or(HostProblem::InfrastructureFailure)?;
            if parent.segment != link.parent_segment {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
    }
    Ok(())
}

pub(super) fn delete_cascade(
    state: &State,
    limits: ImsLimits,
    database: &str,
    position: &mut PcbPosition,
) -> Result<(usize, BTreeMap<String, DatabaseEngine>), LogicalMutationError> {
    let target = position
        .current()
        .ok_or(LogicalMutationError::Status("DJ"))?;
    let mut engines = state
        .generic_databases
        .keys()
        .map(|name| Ok((name.clone(), restored(state, name, limits)?)))
        .collect::<Result<BTreeMap<_, _>, HostProblem>>()?;
    let mut removal = BTreeMap::<String, BTreeSet<RecordId>>::new();
    let mut pending = vec![(database.to_string(), target)];
    let mut visited = BTreeSet::new();
    while let Some((name, id)) = pending.pop() {
        if !visited.insert((name.clone(), id)) {
            continue;
        }
        let subtree = engines[&name]
            .subtree_ids(id)
            .map_err(|problem| LogicalMutationError::Status(engine_status(problem)))?;
        removal.entry(name.clone()).or_default().extend(&subtree);
        for (child_database, engine) in &engines {
            for link in engine.logical_links() {
                if link.parent_database == name && subtree.contains(&link.parent) {
                    if !link.paired {
                        return Err(LogicalMutationError::Status("GP"));
                    }
                    pending.push((child_database.clone(), link.child));
                }
            }
        }
        if visited.len() > limits.max_databases.saturating_mul(limits.max_roots) {
            return Err(LogicalMutationError::Host(HostProblem::ResourceExhausted));
        }
    }
    let mut count = 0;
    count += engines
        .get_mut(database)
        .ok_or(HostProblem::InfrastructureFailure)?
        .delete(position)
        .map_err(|problem| LogicalMutationError::Status(engine_status(problem)))?;
    for (name, ids) in removal {
        let engine = engines
            .get_mut(&name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        for id in ids {
            if engine.path_to(id).is_err() {
                continue;
            }
            count += engine
                .delete_by_id(id)
                .map_err(|problem| LogicalMutationError::Status(engine_status(problem)))?;
        }
    }
    engines.retain(|name, engine| {
        state
            .generic_databases
            .get(name)
            .is_some_and(|prior| engine.image() != **prior)
    });
    Ok((count, engines))
}
