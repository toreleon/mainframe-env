//! Q reservations share SystemState and database-row CAS with ordinary writers.
//! This is a local segment/record write fence, not an integrity-read lock manager.

use super::*;
use crate::database::{DatabaseEngine, DatabaseEngineImage, RecordId};

pub(in crate::service) fn refresh(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut versions = RowVersions::new();
    durable.state.system = load_row_map(store, SYSTEM_NAMESPACE, 1, limits, &mut versions)?;
    durable
        .versions
        .retain(|(namespace, _), _| namespace != SYSTEM_NAMESPACE);
    durable.versions.extend(versions);
    Ok(())
}

fn location(key: &str) -> Result<(&str, &str), HostProblem> {
    key.split_once(':')
        .ok_or(HostProblem::InfrastructureFailure)
}

pub(super) fn same_legacy_record(left: &str, right: &str) -> Result<bool, HostProblem> {
    fn root_key(location: SegmentLocation) -> String {
        match location {
            SegmentLocation::Root { key } => key,
            SegmentLocation::Child { root_key, .. } => root_key,
        }
    }
    let decode = |text| serde_json::from_str(text).map_err(|_| HostProblem::InfrastructureFailure);
    Ok(root_key(decode(left)?) == root_key(decode(right)?))
}

fn root(engine: &DatabaseEngine, id: RecordId) -> Result<RecordId, HostProblem> {
    engine
        .path_to(id)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .first()
        .map(|view| view.id)
        .ok_or(HostProblem::InfrastructureFailure)
}

// Include version even for a REPL that supplies identical bytes. The projection
// reads the existing image codec; it neither writes a schema nor owns navigation.
#[derive(Deserialize, Eq, PartialEq)]
struct RecordWitness {
    id: RecordId,
    segment: String,
    parent: Option<RecordId>,
    data: Vec<u8>,
    version: u64,
}

#[derive(Deserialize)]
struct ImageWitness {
    records: Vec<RecordWitness>,
}

fn records(image: &DatabaseEngineImage) -> Result<BTreeMap<RecordId, RecordWitness>, HostProblem> {
    let bytes = serde_json::to_vec(image).map_err(|_| HostProblem::InfrastructureFailure)?;
    let witness: ImageWitness =
        serde_json::from_slice(&bytes).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(witness
        .records
        .into_iter()
        .map(|record| (record.id, record))
        .collect())
}

pub(in crate::service) fn ensure_image(
    state: &mut State,
    run: &str,
    name: &str,
    image: &DatabaseEngineImage,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    let Some(row) = state.system.get(ROW_KEY) else {
        return Ok(());
    };
    let reservations = row
        .reservations
        .iter()
        .filter(|(key, _)| key.starts_with(&format!("{name}:")))
        .map(|(key, reservation)| (key.clone(), reservation.owner.clone()))
        .collect::<Vec<_>>();
    if reservations.is_empty() {
        return Ok(());
    }
    let before = state
        .generic_databases
        .get(name)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let engine = DatabaseEngine::restore((**before).clone(), generic::engine_limits(limits))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let prior = records(before)?;
    let next = records(image)?;
    let changed = prior
        .keys()
        .chain(next.keys())
        .filter(|id| prior.get(id) != next.get(id))
        .copied()
        .collect::<BTreeSet<_>>();
    let mut modified = Vec::new();
    for (key, owner) in reservations {
        let (_, location) = location(&key)?;
        let id: RecordId =
            serde_json::from_str(location).map_err(|_| HostProblem::InfrastructureFailure)?;
        if owner == run && !prior.contains_key(&id) {
            continue;
        }
        let reserved_root = root(&engine, id)?;
        // Root Q protects the database record; dependent Q protects that segment.
        if changed.contains(&id)
            || (reserved_root == id
                && changed.iter().any(|changed_id| {
                    prior.contains_key(changed_id) && root(&engine, *changed_id).ok() == Some(id)
                        || next.get(changed_id).is_some_and(|record| {
                            let mut current = Some(record.id);
                            while let Some(at) = current {
                                if at == id {
                                    return true;
                                }
                                current = next.get(&at).and_then(|record| record.parent);
                            }
                            false
                        })
                }))
        {
            if owner != run {
                return Err(HostProblem::IdempotencyConflict);
            }
            modified.push(key);
        }
    }
    if !modified.is_empty() {
        let row = system_state(state);
        for key in modified {
            row.reservations
                .get_mut(&key)
                .ok_or(HostProblem::InfrastructureFailure)?
                .modified = true;
        }
    }
    Ok(())
}

pub(in crate::service) fn ensure_no_reservations(
    state: &State,
    name: &str,
) -> Result<(), HostProblem> {
    if state.system.get(ROW_KEY).is_some_and(|row| {
        row.reservations
            .keys()
            .any(|key| key.starts_with(&format!("{name}:")))
    }) {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

pub(in crate::service) fn changed_databases(
    current: &State,
    next: &State,
) -> Result<BTreeSet<String>, HostProblem> {
    let before = current.system.get(ROW_KEY);
    let after = next.system.get(ROW_KEY);
    let mut names = BTreeSet::new();
    for row in before.into_iter().chain(after) {
        for key in row.reservations.keys() {
            if before.and_then(|row| row.reservations.get(key))
                != after.and_then(|row| row.reservations.get(key))
            {
                let (name, _) = location(key)?;
                names.insert(name.to_owned());
            }
        }
    }
    Ok(names)
}

pub(super) fn same_record(
    state: &State,
    database: &str,
    location: &str,
    id: RecordId,
    limits: ImsLimits,
) -> Result<bool, HostProblem> {
    let reserved: RecordId =
        serde_json::from_str(location).map_err(|_| HostProblem::InfrastructureFailure)?;
    let engine = generic::restored(state, database, limits)?;
    // A deleted owner reservation remains modified until settlement, but no
    // longer supplies a live position on a subsequent successful Get.
    let Ok(reserved_root) = root(&engine, reserved) else {
        return Ok(false);
    };
    Ok(reserved_root == root(&engine, id)?)
}

pub(super) fn ensure_acquisition(
    state: &State,
    run: &str,
    database: &str,
    location_text: &str,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if state.generic_databases.contains_key(database) {
        if state.metadata.as_ref().is_some_and(|metadata| {
            metadata.databases.iter().any(|db| {
                normalize(&db.name) == database
                    && matches!(
                        db.organization,
                        ImsDatabaseOrganization::Msdb
                            | ImsDatabaseOrganization::Index
                            | ImsDatabaseOrganization::Psindex
                    )
            })
        }) {
            return Err(HostProblem::Unsupported);
        }
        generic::isolation::ensure_writer(state, run, database)?;
        let id: RecordId =
            serde_json::from_str(location_text).map_err(|_| HostProblem::InfrastructureFailure)?;
        let engine = generic::restored(state, database, limits)?;
        let acquired_root = root(&engine, id)?;
        if let Some(row) = state.system.get(ROW_KEY) {
            for (key, reservation) in &row.reservations {
                let (name, location) = location(key)?;
                if name != database || reservation.owner == run {
                    continue;
                }
                let reserved: RecordId = serde_json::from_str(location)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if reserved == id
                    || (root(&engine, reserved)? == acquired_root
                        && (reserved == acquired_root || id == acquired_root))
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
    } else {
        if state
            .pending_undo
            .iter()
            .any(|(owner, undo)| owner != run && undo.contains_key(database))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let acquired: SegmentLocation =
            serde_json::from_str(location_text).map_err(|_| HostProblem::InfrastructureFailure)?;
        if let Some(row) = state.system.get(ROW_KEY) {
            for (key, reservation) in &row.reservations {
                let (name, text) = location(key)?;
                if name != database || reservation.owner == run {
                    continue;
                }
                let reserved: SegmentLocation =
                    serde_json::from_str(text).map_err(|_| HostProblem::InfrastructureFailure)?;
                if reserved == acquired
                    || (same_legacy_record(text, location_text)?
                        && (matches!(reserved, SegmentLocation::Root { .. })
                            || matches!(acquired, SegmentLocation::Root { .. })))
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
    }
    Ok(())
}

pub(in crate::service) fn owned_databases(
    state: &State,
    run: &str,
) -> Result<BTreeSet<String>, HostProblem> {
    state
        .system
        .get(ROW_KEY)
        .into_iter()
        .flat_map(|row| &row.reservations)
        .filter(|(_, reservation)| reservation.owner == run)
        .map(|(key, _)| location(key).map(|(name, _)| name.to_owned()))
        .collect()
}

pub(in crate::service) fn ensure_legacy_changes(
    current: &State,
    next: &mut State,
    run: &str,
    request: &ImsRequest,
    result: &ImsResult,
) -> Result<(), HostProblem> {
    let Some(row) = current.system.get(ROW_KEY) else {
        return Ok(());
    };
    let mut modified = Vec::new();
    for (key, reservation) in &row.reservations {
        let (name, text) = location(key)?;
        let Some(before) = current.databases.get(name) else {
            continue;
        };
        let after = next
            .databases
            .get(name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if Arc::ptr_eq(before, after) {
            continue;
        }
        let location: SegmentLocation =
            serde_json::from_str(text).map_err(|_| HostProblem::InfrastructureFailure)?;
        let replaced = request.operation == ImsOperation::Replace
            && result.status == "  "
            && current.sessions.get(run).is_some_and(|session| {
                !session.generic
                    && metadata_pcb(current, &session.psb, session.pcb)
                        .is_ok_and(|(database, _, _)| normalize(database) == name)
                    && session.last.as_ref().is_some_and(|target| {
                        target == &location
                            || matches!(&location, SegmentLocation::Root { key }
                                if matches!(target, SegmentLocation::Child { root_key, .. }
                                    if key == root_key))
                    })
            });
        let changed = replaced
            || match location {
                SegmentLocation::Root { key } => before.roots.get(&key) != after.roots.get(&key),
                SegmentLocation::Child { root_key, key } => {
                    before
                        .roots
                        .get(&root_key)
                        .and_then(|root| root.children.get(&key))
                        != after
                            .roots
                            .get(&root_key)
                            .and_then(|root| root.children.get(&key))
                }
            };
        if changed {
            if reservation.owner != run {
                return Err(HostProblem::IdempotencyConflict);
            }
            modified.push(key.clone());
        }
    }
    for key in modified {
        system_state(next)
            .reservations
            .get_mut(&key)
            .ok_or(HostProblem::InfrastructureFailure)?
            .modified = true;
    }
    Ok(())
}

pub(in crate::service) fn fence_legacy_rows(
    current: &State,
    next: &State,
    versions: &RowVersions,
    limits: ImsLimits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    for name in changed_databases(current, next)? {
        if let Some(image) = next.databases.get(&name)
            && !changes
                .iter()
                .any(|change| change.namespace == DATABASE_NAMESPACE && change.key == name)
        {
            changes.push(put_row_change(
                DATABASE_NAMESPACE,
                &name,
                encode_object_row(&name, image)?,
                versions,
                limits.max_state_bytes,
            )?);
        }
    }
    Ok(())
}
