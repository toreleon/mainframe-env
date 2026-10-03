//! Bounded image visibility using existing undo and database-row CAS authority.
//! This is not an IBM record lock scheduler or a second committed image store.

use super::*;

#[derive(Default)]
pub(in crate::service) struct ReadFence {
    generic: BTreeSet<String>,
    legacy: BTreeSet<String>,
}

pub(in crate::service) fn is_read(operation: ImsOperation) -> bool {
    matches!(
        operation,
        ImsOperation::GetUnique
            | ImsOperation::GetNext
            | ImsOperation::GetNextParent
            | ImsOperation::GetHoldUnique
            | ImsOperation::GetHoldNext
            | ImsOperation::GetHoldNextParent
    )
}

pub(in crate::service) fn refresh_sessions(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    isolation::refresh_sessions(store, limits, durable)
}

// The legacy option string has no validated O authority. Refresh image and undo before fresh
// reads as well; exact replay never consults a later UOW's visibility decision.
pub(in crate::service) fn refresh_legacy(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut versions = RowVersions::new();
    durable.state.databases = load_row_map(
        store,
        DATABASE_NAMESPACE,
        limits.max_databases,
        limits,
        &mut versions,
    )?;
    durable.state.pending_undo = load_row_map(
        store,
        PENDING_NAMESPACE,
        limits.max_sessions,
        limits,
        &mut versions,
    )?;
    durable.versions.retain(|(namespace, _), _| {
        namespace != DATABASE_NAMESPACE && namespace != PENDING_NAMESPACE
    });
    durable.versions.extend(versions);
    for name in durable.state.databases.keys() {
        if store
            .get_provider_state(DATABASE_NAMESPACE, name)
            .map_err(store_error)?
            .map(|row| row.version)
            != durable
                .versions
                .get(&(DATABASE_NAMESPACE.into(), name.clone()))
                .copied()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok(())
}

fn without_integrity(
    pcb: &ImsDatabasePcbMetadata,
    organization: crate::ImsDatabaseOrganization,
) -> Result<bool, HostProblem> {
    if !pcb.processing_options.contains('O') {
        return Ok(false);
    }
    if !matches!(
        pcb.processing_options.as_str(),
        "GO" | "GOP" | "GON" | "GONP" | "GOT" | "GOTP"
    ) || matches!(
        organization,
        crate::ImsDatabaseOrganization::Msdb | crate::ImsDatabaseOrganization::Gsam
    ) || pcb.sensitive_segments.iter().any(|segment| {
        segment
            .processing_options
            .as_ref()
            .is_some_and(|options| options.bytes().any(|option| b"IRDA".contains(&option)))
    }) {
        // O is PCB authority and cannot coexist with update SENSEG authority.
        return Err(HostProblem::Unsupported);
    }
    Ok(true)
}

pub(in crate::service) fn prepare(
    state: &State,
    run: &str,
    request: &ImsRequest,
) -> Result<ReadFence, HostProblem> {
    if !is_read(request.operation) {
        return Ok(ReadFence::default());
    }
    if is_generic(state, run, request) {
        let pcb = session_pcb(state, run, request.pcb)?;
        let name = normalize(&pcb.database);
        let names = isolation::dependencies(state, &name)?;
        let organization = state
            .metadata
            .as_ref()
            .and_then(|metadata| {
                metadata
                    .databases
                    .iter()
                    .find(|database| normalize(&database.name) == name)
            })
            .ok_or(HostProblem::InfrastructureFailure)?
            .organization;
        if without_integrity(pcb, organization)? {
            // Concatenated logical-parent access has separate source RULES
            // authority, absent from this bounded PCB contract. O on the child
            // cannot grant an exemption on a different related database.
            if state.generic_pending_undo.iter().any(|(owner, undo)| {
                owner != run
                    && names
                        .iter()
                        .any(|related| related != &name && undo.contains_key(related))
            }) {
                return Err(HostProblem::IdempotencyConflict);
            }
            // O permits foreign pending data, never malformed/unproven undo.
            for (owner, undo) in &state.generic_pending_undo {
                if names.iter().any(|name| undo.contains_key(name)) {
                    isolation::ensure_backout(state, owner)?;
                }
            }
        } else {
            isolation::ensure_writer(state, run, &name)?;
        }
        Ok(ReadFence {
            generic: names,
            legacy: BTreeSet::new(),
        })
    } else {
        let (name, _, _) = context(state, run)?;
        if state
            .pending_undo
            .iter()
            .any(|(owner, undo)| owner != run && undo.contains_key(&name))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(ReadFence {
            generic: BTreeSet::new(),
            legacy: BTreeSet::from([name]),
        })
    }
}

pub(in crate::service) fn persist(
    service: &ImsService,
    durable: &mut DurableState,
    state: State,
    fence: &ReadFence,
) -> Result<(), HostProblem> {
    let mut changes = row_changes(
        &durable.state,
        &state,
        &durable.versions,
        service.limits,
        false,
    )?;
    for (namespace, names) in [
        (GENERIC_DATABASE_NAMESPACE, &fence.generic),
        (DATABASE_NAMESPACE, &fence.legacy),
    ] {
        for name in names {
            if changes
                .iter()
                .any(|change| change.namespace == namespace && change.key == *name)
            {
                continue;
            }
            let payload = if namespace == GENERIC_DATABASE_NAMESPACE {
                encode_object_row(
                    name,
                    state
                        .generic_databases
                        .get(name)
                        .ok_or(HostProblem::InfrastructureFailure)?,
                )?
            } else {
                encode_object_row(
                    name,
                    state
                        .databases
                        .get(name)
                        .ok_or(HostProblem::InfrastructureFailure)?,
                )?
            };
            changes.push(put_row_change(
                namespace,
                name,
                payload,
                &durable.versions,
                service.limits.max_state_bytes,
            )?);
        }
    }
    // Visibility, navigation/status/Q and original replay result publish in
    // one CAS. A new writer or no-op ownership transition invalidates the read.
    commit_row_changes(&*service.store, changes, &mut durable.versions)?;
    durable.state = state;
    Ok(())
}
