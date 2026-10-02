//! Unchanged row-store migration and atomic publication helpers.
use super::*;

pub(super) fn load_or_migrate(
    store: &dyn ProviderStateStore,
    limits: MqLimits,
) -> Result<(State, RowVersions), HostProblem> {
    let Some(manifest_record) = store
        .get_provider_state(STATE_NAMESPACE, STATE_KEY)
        .map_err(store_error)?
    else {
        ensure_row_namespaces_empty(store)?;
        return Ok((
            State {
                next_handle: 1,
                ..State::default()
            },
            RowVersions::new(),
        ));
    };
    if manifest_record.namespace != STATE_NAMESPACE
        || manifest_record.key != STATE_KEY
        || manifest_record.version == 0
        || manifest_record.payload.len() > limits.max_state_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if let Ok(manifest) = serde_json::from_slice::<RowStoreManifest>(&manifest_record.payload) {
        if manifest.schema_version != ROW_STORE_SCHEMA || manifest.next_handle == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut versions = RowVersions::from([(
            (STATE_NAMESPACE.into(), STATE_KEY.into()),
            manifest_record.version,
        )]);
        let mut state = State {
            definitions: manifest.definitions,
            catalog: load_catalog_row(store, limits, &mut versions)?,
            queues: load_row_map(
                store,
                QUEUE_NAMESPACE,
                limits.max_queues,
                limits,
                &mut versions,
            )?,
            handles: load_row_map(
                store,
                HANDLE_NAMESPACE,
                limits.max_handles,
                limits,
                &mut versions,
            )?,
            pending: load_row_map(
                store,
                PENDING_NAMESPACE,
                limits.max_pending_units,
                limits,
                &mut versions,
            )?,
            replay: load_row_map(
                store,
                REPLAY_NAMESPACE,
                limits.max_replays,
                limits,
                &mut versions,
            )?,
            next_handle: manifest.next_handle,
        };
        let missing_catalog = state.catalog.is_none() && state.definitions.is_some();
        if missing_catalog {
            let catalog = legacy_catalog(state.definitions.as_ref().unwrap(), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            state.catalog = Some(Arc::new(catalog));
        }
        validate_state(&state, limits)?;
        let mut changes = Vec::new();
        if missing_catalog {
            let payload = encode_catalog_row(state.catalog.as_ref().unwrap())?;
            changes.push(put_row_change(
                CATALOG_NAMESPACE,
                CATALOG_KEY,
                payload,
                &versions,
                limits.max_state_bytes,
            )?);
        }
        if state.definitions.is_some() {
            let normalized = RowStoreManifest {
                schema_version: ROW_STORE_SCHEMA.into(),
                definitions: None,
                next_handle: state.next_handle,
            };
            changes.push(put_row_change(
                STATE_NAMESPACE,
                STATE_KEY,
                serde_json::to_vec(&normalized).map_err(|_| HostProblem::InfrastructureFailure)?,
                &versions,
                limits.max_state_bytes,
            )?);
        }
        commit_row_changes(store, changes, &mut versions)?;
        state.definitions = None;
        return Ok((state, versions));
    }

    let mut legacy: State = serde_json::from_slice(&manifest_record.payload)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if legacy.next_handle == 0 {
        legacy.next_handle = 1;
    }
    if let Some(definitions) = &legacy.definitions {
        legacy.catalog = Some(Arc::new(
            legacy_catalog(definitions, limits).map_err(|_| HostProblem::InfrastructureFailure)?,
        ));
    }
    validate_state(&legacy, limits)?;
    legacy.definitions = None;
    ensure_row_namespaces_empty(store)?;
    let mut versions = RowVersions::from([(
        (STATE_NAMESPACE.into(), STATE_KEY.into()),
        manifest_record.version,
    )]);
    let changes = row_changes(&State::default(), &legacy, &versions, limits, true)?;
    commit_row_changes(store, changes, &mut versions)?;
    Ok((legacy, versions))
}

pub(super) fn ensure_row_namespaces_empty(
    store: &dyn ProviderStateStore,
) -> Result<(), HostProblem> {
    for namespace in [
        QUEUE_NAMESPACE,
        CATALOG_NAMESPACE,
        HANDLE_NAMESPACE,
        PENDING_NAMESPACE,
        REPLAY_NAMESPACE,
    ] {
        if !store
            .list_provider_state(namespace, 1)
            .map_err(store_error)?
            .is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

pub(super) fn load_row_map<T: DeserializeOwned>(
    store: &dyn ProviderStateStore,
    namespace: &str,
    max: usize,
    limits: MqLimits,
    versions: &mut RowVersions,
) -> Result<BTreeMap<String, T>, HostProblem> {
    let fetch = max.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
    let records = store
        .list_provider_state(namespace, fetch)
        .map_err(store_error)?;
    if records.len() > max {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = BTreeMap::new();
    for record in records {
        if record.namespace != namespace
            || record.key.is_empty()
            || record.version == 0
            || record.payload.len() > limits.max_state_bytes
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let row: ObjectRow<T> = serde_json::from_slice(&record.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
            return Err(HostProblem::InfrastructureFailure);
        }
        versions.insert((namespace.into(), record.key.clone()), record.version);
        if values.insert(record.key, row.value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

pub(super) fn row_changes(
    current: &State,
    next: &State,
    versions: &RowVersions,
    limits: MqLimits,
    force_manifest_write: bool,
) -> Result<Vec<RowChange>, HostProblem> {
    let mut changes = Vec::new();
    let current_manifest = RowStoreManifest {
        schema_version: ROW_STORE_SCHEMA.into(),
        definitions: current.definitions.clone(),
        next_handle: current.next_handle,
    };
    let next_manifest = RowStoreManifest {
        schema_version: ROW_STORE_SCHEMA.into(),
        definitions: next.definitions.clone(),
        next_handle: next.next_handle,
    };
    if force_manifest_write
        || current_manifest != next_manifest
        || !versions.contains_key(&(STATE_NAMESPACE.into(), STATE_KEY.into()))
    {
        let payload =
            serde_json::to_vec(&next_manifest).map_err(|_| HostProblem::InfrastructureFailure)?;
        changes.push(put_row_change(
            STATE_NAMESPACE,
            STATE_KEY,
            payload,
            versions,
            limits.max_state_bytes,
        )?);
    }
    map_arc_row_changes(
        QUEUE_NAMESPACE,
        &current.queues,
        &next.queues,
        versions,
        limits,
        &mut changes,
    )?;
    if current.catalog != next.catalog {
        if let Some(catalog) = &next.catalog {
            changes.push(put_row_change(
                CATALOG_NAMESPACE,
                CATALOG_KEY,
                encode_catalog_row(catalog)?,
                versions,
                limits.max_state_bytes,
            )?);
        } else if let Some(version) = versions.get(&(CATALOG_NAMESPACE.into(), CATALOG_KEY.into()))
        {
            changes.push(RowChange {
                namespace: CATALOG_NAMESPACE.into(),
                key: CATALOG_KEY.into(),
                next_version: None,
                mutation: ProviderStateMutation::Delete {
                    namespace: CATALOG_NAMESPACE.into(),
                    key: CATALOG_KEY.into(),
                    expected_version: *version,
                },
            });
        }
    }
    map_arc_row_changes(
        HANDLE_NAMESPACE,
        &current.handles,
        &next.handles,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        PENDING_NAMESPACE,
        &current.pending,
        &next.pending,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        REPLAY_NAMESPACE,
        &current.replay,
        &next.replay,
        versions,
        limits,
        &mut changes,
    )?;
    Ok(changes)
}

pub(super) fn map_arc_row_changes<T: Serialize>(
    namespace: &str,
    current: &BTreeMap<String, Arc<T>>,
    next: &BTreeMap<String, Arc<T>>,
    versions: &RowVersions,
    limits: MqLimits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    for (key, value) in next {
        if !current
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, value))
        {
            changes.push(put_row_change(
                namespace,
                key,
                encode_object_row(key, value)?,
                versions,
                limits.max_state_bytes,
            )?);
        }
    }
    for key in current.keys().filter(|key| !next.contains_key(*key)) {
        let version = versions
            .get(&(namespace.into(), key.clone()))
            .copied()
            .ok_or(HostProblem::InfrastructureFailure)?;
        changes.push(RowChange {
            namespace: namespace.into(),
            key: key.clone(),
            next_version: None,
            mutation: ProviderStateMutation::Delete {
                namespace: namespace.into(),
                key: key.clone(),
                expected_version: version,
            },
        });
    }
    Ok(())
}

pub(super) fn encode_object_row<T: Serialize>(
    key: &str,
    value: &T,
) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(&ObjectRow {
        schema_version: OBJECT_ROW_SCHEMA.into(),
        object_key: key.into(),
        value,
    })
    .map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn put_row_change(
    namespace: &str,
    key: &str,
    payload: Vec<u8>,
    versions: &RowVersions,
    max_state_bytes: usize,
) -> Result<RowChange, HostProblem> {
    if payload.len() > max_state_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let current = versions.get(&(namespace.into(), key.into())).copied();
    let next = current
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(RowChange {
        namespace: namespace.into(),
        key: key.into(),
        next_version: Some(next),
        mutation: ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: next,
                payload,
            },
            expected_version: current,
        }),
    })
}

pub(super) fn commit_row_changes(
    store: &dyn ProviderStateStore,
    changes: Vec<RowChange>,
    versions: &mut RowVersions,
) -> Result<(), HostProblem> {
    if changes.is_empty() {
        return Ok(());
    }
    let applied = changes
        .iter()
        .map(|change| {
            (
                (change.namespace.clone(), change.key.clone()),
                change.next_version,
            )
        })
        .collect::<Vec<_>>();
    store
        .mutate_provider_states_atomic(changes.into_iter().map(|change| change.mutation).collect())
        .map_err(store_error)?;
    for (key, version) in applied {
        match version {
            Some(version) => {
                versions.insert(key, version);
            }
            None => {
                versions.remove(&key);
            }
        }
    }
    Ok(())
}
