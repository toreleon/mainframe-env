//! Shared bounded custody-index validation and storage-marker encoding.
use super::*;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, RootCustodyCutoff};

pub(crate) fn custody_remaining(
    snapshot: &RootClosureSnapshot,
) -> Result<(usize, usize), StoreError> {
    let (operations, bytes) = snapshot.captured_usage()?;
    // Reserve the complete next ownership row plus a conservative bounded
    // marker/identity allowance before cloning any extra retained dependency.
    let reserve = snapshot
        .closing
        .payload
        .len()
        .checked_add(2048)
        .ok_or(StoreError::CapacityExceeded)?;
    Ok((
        MAX_ROOT_OPERATIONS
            .checked_sub(operations + 2)
            .ok_or(StoreError::CapacityExceeded)?,
        MAX_ROOT_PAYLOAD_BYTES
            .checked_sub(bytes)
            .and_then(|n| n.checked_sub(reserve))
            .ok_or(StoreError::CapacityExceeded)?,
    ))
}
pub(crate) fn custody_require_row(
    snapshot: &RootClosureSnapshot,
    row: &ProviderStateRecord,
) -> Result<(), StoreError> {
    let (operations, bytes) = custody_remaining(snapshot)?;
    if operations == 0 || row.namespace.len() + row.key.len() + row.payload.len() > bytes {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(())
}

pub(crate) fn custody_index_identities(doc: &Document) -> Vec<(String, String)> {
    let mut result = vec![(RUN_NAMESPACE.into(), doc.run.clone())];
    result.extend(
        doc.actors
            .iter()
            .map(|actor| (ACTOR_NAMESPACE.into(), actor.execution.clone())),
    );
    result.extend(
        doc.provider_namespaces
            .iter()
            .map(|scope| (SCOPE_NAMESPACE.into(), scope.clone())),
    );
    result.extend(
        doc.provider_rows
            .iter()
            .map(|(namespace, key)| (ROW_SCOPE_NAMESPACE.into(), row_scope_key(namespace, key))),
    );
    result
}
pub(crate) fn custody_marker(
    capture: &RootCustodyCutoff,
) -> Result<ProviderStateRecord, StoreError> {
    let payload = serde_json::to_vec(&json!({
        "schema": "mainframe-env.root-custody-cutoff@1",
        "root": capture.preserved.claim.admission().execution.execution_id.as_str(),
        "run": capture.preserved.claim.admission().execution.run_unit_id.as_str(),
        "fenced_version": capture.fenced.version,
        "captured_digest": capture.captured_storage_digest()?.as_slice(),
        "observed_tick": capture.preserved.observed_tick,
    }))
    .map_err(|_| StoreError::IncompatibleVersion)?;
    Ok(ProviderStateRecord {
        namespace: ROOT_CUSTODY_NAMESPACE.into(),
        key: capture.fenced.key.clone(),
        version: 1,
        payload,
    })
}
