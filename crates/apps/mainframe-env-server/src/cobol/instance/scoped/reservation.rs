//! Pure admission postimage preparation. Row integrity supplies no live authority.
//! The manager must validate current core intent/control, original CALL rows and
//! real LINK origin/selection, and couple these writes with the pending CALL and
//! source fence in one transaction before dispatch or live lease registration.
use super::*;

#[cfg(test)]
mod tests;

/// Absence is an explicit observation by the admission owner, never a decoder
/// fallback for malformed, legacy, missing or mismatched existing state.
pub(super) enum RootRow<'a> {
    Existing(&'a ProviderStateRecord),
    Absent,
}

/// Owned proposals only. Untouched members (including a managed source) retain
/// their exact bytes and index versions. A later source fence requires the
/// manager to update its index version and revalidate the combined postimage.
pub(super) struct Prepared {
    pub(super) root_write: ProviderStateWrite,
    pub(super) target_write: ProviderStateWrite,
    pub(super) state: Option<Vec<u8>>,
    pub(super) initial: bool,
}

fn bound(actor: &Invocation, entry: &Entry) -> Result<(), HostProblem> {
    entry.validate_for(actor)?;
    if Entry::read(actor)?.as_ref() != Some(entry) {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

fn successor(version: Option<u64>) -> Result<u64, HostProblem> {
    match version {
        None => Ok(1),
        Some(version) => version
            .checked_add(1)
            .filter(|next| *next <= i64::MAX as u64)
            .ok_or(HostProblem::ResourceExhausted),
    }
}

/// Prepare only a new original CALL admission using trusted actual invocations
/// and the complete current root/member rows. Constructors and checksums do not
/// prove a selected machine, current core operation, live source, or LINK origin.
#[allow(
    clippy::too_many_arguments,
    reason = "explicit trusted admission observations"
)]
pub(super) fn prepare(
    root_actor: &Invocation,
    source_actor: &Invocation,
    target_actor: &Invocation,
    source_entry: &Entry,
    target_entry: &Entry,
    observed_root: RootRow<'_>,
    member_rows: &[ProviderStateRecord],
    initial: bool,
) -> Result<Prepared, HostProblem> {
    let key = run_key(root_actor);
    let (mut root, root_version) = match observed_root {
        RootRow::Existing(row) => {
            if row.key != key {
                return Err(HostProblem::UnknownOutcome);
            }
            (ScopedRun::decode(row)?, Some(row.version))
        }
        RootRow::Absent => (ScopedRun::fresh(root_actor)?, None),
    };
    root.validate_live_limits(root_actor)?;
    // Bind the entire original root entry, not only its run/principal hash.
    if root.root != Entry::root(root_actor)?
        || Entry::read(root_actor)?.is_some_and(|entry| entry != root.root)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    root.validate_members(&key, member_rows)?;
    if root.ended_tick.is_some() {
        return Err(HostProblem::ResourceExhausted);
    }
    bound(source_actor, source_entry)?;
    bound(target_actor, target_entry)?;

    let source_scope = root
        .scopes
        .get(source_entry.scope_id())
        .filter(|scope| scope.same_scope(source_entry))
        .ok_or(HostProblem::UnknownOutcome)?;
    // The top machine is unmanaged. Every other source must match the exact
    // indexed busy owner. This is read integrity; the manager still needs a lease.
    if source_entry != &root.root {
        let source_key = source_entry.member_key(source_entry.actor_identity().1)?;
        let row = member_rows
            .iter()
            .find(|row| row.key == source_key)
            .ok_or(HostProblem::UnknownOutcome)?;
        let source = ScopedInstance::decode(row)?;
        if source.owner.as_ref() != Some(source_entry) {
            return Err(HostProblem::UnknownOutcome);
        }
    }

    let call = target_entry.call_key().ok_or(HostProblem::UnknownOutcome)?;
    // Reuse Entry's exact source/target child relation (attempt, actual parent,
    // execution derived from CALL key, root/run/principal), without granting
    // factory authority. A native target must equal the inherited entry.
    let native = source_entry.native_call(source_actor, target_actor, call)?;
    let target_scope = if target_entry.is_creator() {
        let (owner, run, principal, parent) = target_entry.creation_identity();
        let (root_owner, root_run, root_principal, _) = root.root.creation_identity();
        if parent != Some(source_entry.scope_id())
            || (owner, run, principal) != (root_owner, root_run, root_principal)
            || source_entry.logical_level().checked_add(1) != Some(target_entry.logical_level())
            || root.scopes.contains_key(target_entry.scope_id())
        {
            return Err(HostProblem::UnknownOutcome);
        }
        if root.scopes.len() >= root.max_scopes as usize {
            return Err(HostProblem::ResourceExhausted);
        }
        target_entry.clone()
    } else {
        if target_entry != &native {
            return Err(HostProblem::UnknownOutcome);
        }
        source_scope.clone()
    };
    if root.calls.contains(call) {
        return Err(HostProblem::IdempotencyConflict);
    }
    // One unmanaged root frame plus all busy installed members, including the
    // proposed target. Idle members consume storage, never active frame slots.
    if root
        .active
        .checked_add(2)
        .is_none_or(|frames| frames as u64 > u64::from(root_actor.limits.max_frames))
    {
        return Err(HostProblem::ResourceExhausted);
    }

    let (_, program, artifact, _) = target_entry.actor_identity();
    let target_key = target_entry.member_key(program)?;
    let existing = member_rows.iter().find(|row| row.key == target_key);
    let (mut target, target_version, old_charge) = match existing {
        Some(row) => {
            let value = ScopedInstance::decode(row)?;
            if value.owner.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            if target_entry.is_creator() || value.artifact != artifact || value.initial != initial {
                return Err(HostProblem::UnknownOutcome);
            }
            let charged = root
                .members
                .get(&target_key)
                .ok_or(HostProblem::UnknownOutcome)?
                .charged_bytes;
            (value, Some(row.version), charged)
        }
        None => {
            if root.members.len() >= MAX_INSTANCES {
                return Err(HostProblem::ResourceExhausted);
            }
            (
                ScopedInstance {
                    schema_version: 3,
                    run_key: key.clone(),
                    scope_entry: target_scope.clone(),
                    max_state_bytes: root.max_member_bytes,
                    program: program.into(),
                    artifact: artifact.into(),
                    owner: None,
                    initial,
                    state: None,
                    metadata_digest: String::new(),
                },
                None,
                0,
            )
        }
    };
    let next_root_version = successor(root_version)?;
    let next_target_version = successor(target_version)?;
    if initial {
        target.state = None;
    }
    target.owner = Some(target_entry.clone());
    target.metadata_digest = target.expected_digest(&target_key)?;
    let target_write = ProviderStateWrite {
        expected_version: target_version,
        record: ProviderStateRecord {
            namespace: namespace(&key),
            key: target_key.clone(),
            version: next_target_version,
            payload: canonical(&target)?,
        },
    };
    // reserve_call_charge validates its intermediate root: insert a LINK scope
    // only after reserving its immutable original creation CALL charge.
    root.reserve_call_charge(&key, call)?;
    if target_entry.is_creator() {
        root.scopes
            .insert(target_scope.scope_id().into(), target_scope);
    }
    root.members.insert(
        target_key.clone(),
        Member {
            scope: target_entry.scope_id().into(),
            program: program.into(),
            artifact: artifact.into(),
            row_version: next_target_version,
            payload_digest: payload_digest(&target_write.record.payload),
            charged_bytes: root.max_member_bytes,
            busy: true,
        },
    );
    root.active = root
        .active
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    root.charged_bytes = root
        .charged_bytes
        .checked_sub(old_charge)
        .and_then(|charge| charge.checked_add(root.max_member_bytes))
        .ok_or(HostProblem::ResourceExhausted)?;
    root.refresh(&key)?;
    let mut postimage = member_rows.to_vec();
    postimage.retain(|row| row.key != target_key);
    postimage.push(target_write.record.clone());
    root.validate_members(&key, &postimage)?;
    let root_write = ProviderStateWrite {
        expected_version: root_version,
        record: ProviderStateRecord {
            namespace: RUN_STATE_NAMESPACE.into(),
            key,
            version: next_root_version,
            payload: canonical(&root)?,
        },
    };
    // Validate the exact output records, including canonical bytes and signed
    // SQL-compatible versions, rather than just the in-memory counters.
    ScopedInstance::decode(&target_write.record)?;
    ScopedRun::decode(&root_write.record)?.validate_live_limits(root_actor)?;
    Ok(Prepared {
        root_write,
        target_write,
        state: target.state,
        initial: target.initial,
    })
}
