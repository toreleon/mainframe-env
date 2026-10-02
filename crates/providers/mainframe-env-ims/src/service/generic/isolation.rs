//! Local image undo is safe only under exclusive pending-UOW ownership and
//! the shared database-row CAS. This module owns no locks or coordinator.

use super::*;
use std::ops::Deref;

const UNDO_SCHEMA: &str = "mainframe-env.ims-local-undo@2";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::service) struct PendingUndo {
    schema_version: String,
    images: BTreeMap<String, Arc<DatabaseEngineImage>>,
    post_images: BTreeMap<String, [u8; 32]>,
    owned_images: BTreeMap<String, BTreeSet<[u8; 32]>>,
}

impl Serialize for PendingUndo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.schema_version.is_empty() && self.post_images.is_empty() {
            return self.images.serialize(serializer);
        }
        #[derive(Serialize)]
        struct Current<'a> {
            schema_version: &'a str,
            images: &'a BTreeMap<String, Arc<DatabaseEngineImage>>,
            post_images: &'a BTreeMap<String, [u8; 32]>,
            owned_images: &'a BTreeMap<String, BTreeSet<[u8; 32]>>,
        }
        Current {
            schema_version: &self.schema_version,
            images: &self.images,
            post_images: &self.post_images,
            owned_images: &self.owned_images,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PendingUndo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Current {
            schema_version: String,
            images: BTreeMap<String, Arc<DatabaseEngineImage>>,
            post_images: BTreeMap<String, [u8; 32]>,
            owned_images: BTreeMap<String, BTreeSet<[u8; 32]>>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Reader {
            Current(Current),
            Legacy(BTreeMap<String, Arc<DatabaseEngineImage>>),
        }
        match Reader::deserialize(deserializer)? {
            Reader::Current(value) => {
                if value.schema_version != UNDO_SCHEMA
                    || value.images.keys().ne(value.post_images.keys())
                    || value.images.keys().ne(value.owned_images.keys())
                    || value
                        .post_images
                        .iter()
                        .any(|(name, digest)| !value.owned_images[name].contains(digest))
                {
                    return Err(serde::de::Error::custom("invalid IMS local undo witness"));
                }
                Ok(Self {
                    schema_version: value.schema_version,
                    images: value.images,
                    post_images: value.post_images,
                    owned_images: value.owned_images,
                })
            }
            Reader::Legacy(images) => Ok(Self {
                images,
                ..Self::default()
            }),
        }
    }
}

impl Deref for PendingUndo {
    type Target = BTreeMap<String, Arc<DatabaseEngineImage>>;

    fn deref(&self) -> &Self::Target {
        &self.images
    }
}

impl PendingUndo {
    pub(super) fn validate(&self, limits: ImsLimits) -> Result<(), HostProblem> {
        let mut count = 0usize;
        for (name, hashes) in &self.owned_images {
            count = count
                .checked_add(hashes.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            if !hashes.contains(&image_digest(&self.images[name])?) {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        if count > limits.max_replays.saturating_add(limits.max_databases) {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(())
    }
}

fn image_digest(image: &DatabaseEngineImage) -> Result<[u8; 32], HostProblem> {
    let bytes = serde_json::to_vec(image).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(Sha256::digest(bytes).into())
}

pub(in crate::service) fn dependencies(
    state: &State,
    name: &str,
) -> Result<BTreeSet<String>, HostProblem> {
    let mut names = BTreeSet::from([name.to_string()]);
    let mut pending = vec![name.to_string()];
    while let Some(name) = pending.pop() {
        for related in logical::related_databases(state, &name)? {
            if names.insert(related.clone()) {
                pending.push(related);
            }
        }
    }
    Ok(names)
}

pub(in crate::service) fn ensure_writer(
    state: &State,
    run: &str,
    name: &str,
) -> Result<(), HostProblem> {
    let names = dependencies(state, name)?;
    if state
        .generic_pending_undo
        .iter()
        .any(|(owner, undo)| owner != run && names.iter().any(|name| undo.contains_key(name)))
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    ensure_backout(state, run)
}

pub(in crate::service) fn ensure_no_pending(state: &State, name: &str) -> Result<(), HostProblem> {
    let names = dependencies(state, name)?;
    if state
        .generic_pending_undo
        .values()
        .any(|undo| names.iter().any(|name| undo.contains_key(name)))
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    system::reservations::ensure_no_reservations(state, name)?;
    Ok(())
}

pub(in crate::service) fn refresh_undo(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
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
    for undo in durable.state.generic_pending_undo.values() {
        undo.validate(limits)?;
    }
    verify_refresh(store, durable)
}

pub(super) fn refresh_sessions(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut versions = RowVersions::new();
    durable.state.sessions = load_row_map(
        store,
        SESSION_NAMESPACE,
        limits.max_sessions,
        limits,
        &mut versions,
    )?;
    durable
        .versions
        .retain(|(namespace, _), _| namespace != SESSION_NAMESPACE);
    durable.versions.extend(versions);
    Ok(())
}

pub(in crate::service) fn ensure_backout(state: &State, run: &str) -> Result<(), HostProblem> {
    let Some(undo) = state.generic_pending_undo.get(run) else {
        return Ok(());
    };
    if undo.schema_version != UNDO_SCHEMA || undo.images.keys().ne(undo.post_images.keys()) {
        // Earlier images carry no evidence that another writer did not commit.
        return Err(HostProblem::UnknownOutcome);
    }
    for (name, expected) in &undo.post_images {
        if state
            .generic_pending_undo
            .iter()
            .any(|(owner, other)| owner != run && other.contains_key(name))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let image = state
            .generic_databases
            .get(name)
            .ok_or(HostProblem::UnknownOutcome)?;
        if image_digest(image)? != *expected {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    Ok(())
}

pub(super) fn publish_image(
    state: &mut State,
    run: &str,
    name: &str,
    image: DatabaseEngineImage,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    ensure_writer(state, run, name)?;
    system::reservations::ensure_image(state, run, name, &image, limits)?;
    let prior = state
        .generic_databases
        .get(name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let digest = image_digest(&image)?;
    let prior_digest = image_digest(&prior)?;
    let undo = Arc::make_mut(state.generic_pending_undo.entry(run.into()).or_default());
    undo.schema_version = UNDO_SCHEMA.into();
    undo.images.entry(name.into()).or_insert(prior);
    undo.post_images.insert(name.into(), digest);
    let owned = undo.owned_images.entry(name.into()).or_default();
    owned.insert(prior_digest);
    owned.insert(digest);
    state.generic_databases.insert(name.into(), Arc::new(image));
    Ok(())
}

pub(in crate::service) fn publish_backout_image(
    state: &mut State,
    run: &str,
    name: &str,
    image: DatabaseEngineImage,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    ensure_writer(state, run, name)?;
    system::reservations::ensure_image(state, run, name, &image, limits)?;
    let digest = image_digest(&image)?;
    let undo = state
        .generic_pending_undo
        .get_mut(run)
        .ok_or(HostProblem::UnknownOutcome)?;
    if !undo
        .owned_images
        .get(name)
        .is_some_and(|owned| owned.contains(&digest))
    {
        return Err(HostProblem::UnknownOutcome);
    }
    // A savepoint backout is not a commit. Keep the original local undo and
    // ownership until the existing common local settlement removes it.
    Arc::make_mut(undo).post_images.insert(name.into(), digest);
    state.generic_databases.insert(name.into(), Arc::new(image));
    Ok(())
}

pub(super) fn verify_refresh(
    store: &dyn ProviderStateStore,
    durable: &DurableState,
) -> Result<(), HostProblem> {
    // UOW acquire/release and dependency mutations advance database CAS even
    // when image bytes are unchanged. Recheck the first scan after reading undo.
    for name in durable.state.generic_databases.keys() {
        let row = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, name)
            .map_err(store_error)?;
        let version = row.map(|row| row.version);
        if version
            != durable
                .versions
                .get(&(GENERIC_DATABASE_NAMESPACE.into(), name.clone()))
                .copied()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok(())
}

pub(in crate::service) fn fence_row_changes(
    current: &State,
    next: &State,
    versions: &RowVersions,
    limits: ImsLimits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    map_arc_row_changes(
        GENERIC_DATABASE_NAMESPACE,
        &current.generic_databases,
        &next.generic_databases,
        versions,
        limits,
        changes,
    )?;
    let mut names = changes
        .iter()
        .filter(|change| change.namespace == GENERIC_DATABASE_NAMESPACE)
        .map(|change| change.key.clone())
        .collect::<BTreeSet<_>>();
    for run in current
        .generic_pending_undo
        .keys()
        .chain(next.generic_pending_undo.keys())
    {
        let before = current.generic_pending_undo.get(run);
        let after = next.generic_pending_undo.get(run);
        if before.zip(after).is_some_and(|(a, b)| Arc::ptr_eq(a, b)) {
            continue;
        }
        for undo in before.into_iter().chain(after) {
            names.extend(undo.keys().cloned());
        }
    }
    let mut fenced = names.clone();
    names.extend(
        system::reservations::changed_databases(current, next)?
            .into_iter()
            .filter(|name| next.generic_databases.contains_key(name)),
    );
    system::reservations::fence_legacy_rows(current, next, versions, limits, changes)?;
    for name in names {
        fenced.extend(dependencies(next, &name)?);
    }
    for name in fenced {
        if !changes
            .iter()
            .any(|change| change.namespace == GENERIC_DATABASE_NAMESPACE && change.key == name)
        {
            let image = next
                .generic_databases
                .get(&name)
                .ok_or(HostProblem::InfrastructureFailure)?;
            changes.push(put_row_change(
                GENERIC_DATABASE_NAMESPACE,
                &name,
                encode_object_row(&name, image)?,
                versions,
                limits.max_state_bytes,
            )?);
        }
    }
    Ok(())
}

pub(in crate::service) fn publication_error(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Infrastructure(_) | StoreError::Poisoned => HostProblem::UnknownOutcome,
        other => store_error(other),
    }
}
