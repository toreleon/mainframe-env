//! Structural successor validation; custody is supplied by the closed host,
//! never inferred from these clonable storage observations.
use super::*;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, RootCustodySuccessor};

pub(crate) struct SuccessorPlan {
    pub(crate) document: Document,
    pub(crate) root: ProviderStateRecord,
    pub(crate) claim: RootDriverClaim,
    pub(crate) scopes: Vec<ProviderStateWrite>,
    pub(crate) frozen: Vec<ProviderStateWrite>,
    pub(crate) marker: ProviderStateWrite,
}

pub(crate) fn plan(
    request: &RootCustodySuccessor,
    current_scopes: &[ProviderStateRecord],
    max_row_bytes: usize,
) -> Result<SuccessorPlan, StoreError> {
    request.validate_bounds()?;
    let old = Document::read(&request.cutoff.preserved.closing)?;
    old.require_claim(&request.cutoff.preserved.claim)?;
    if old.phase != Phase::Open
        || request.cutoff.marker != super::custody::custody_marker(&request.cutoff)?
    {
        return Err(StoreError::Conflict);
    }
    let fenced = Document::read(&request.cutoff.fenced)?;
    let mut expected = old.clone();
    expected.phase = Phase::Uncertain;
    if fenced != expected
        || request.cutoff.fenced.version
            != request
                .cutoff
                .preserved
                .closing
                .version
                .checked_add(1)
                .ok_or(StoreError::Conflict)?
    {
        return Err(StoreError::Conflict);
    }
    let mut document = Document::new(&request.admission)?;
    if document.root == old.root || document.run == old.run || document.principal != old.principal {
        return Err(StoreError::Conflict);
    }
    let mut versions = Vec::with_capacity(document.provider_namespaces.len());
    let mut scopes = Vec::with_capacity(document.provider_namespaces.len());
    let mut transferred = false;
    for namespace in &document.provider_namespaces {
        let current = current_scopes
            .iter()
            .find(|row| row.namespace == SCOPE_NAMESPACE && row.key == *namespace);
        let (version, expected_version) = match current {
            Some(row) => {
                old.require_scope_index(row, namespace, "")?;
                if !request.cutoff.preserved.provider_dependencies.iter().any(|dependency| matches!(dependency, TerminalRowDependency::Exact(retained) if retained == row)) {
                    return Err(StoreError::Conflict);
                }
                transferred = true;
                (
                    row.version
                        .checked_add(1)
                        .filter(|n| *n <= i64::MAX as u64)
                        .ok_or(StoreError::Conflict)?,
                    Some(row.version),
                )
            }
            None => (1, None),
        };
        versions.push((namespace.clone(), version));
        scopes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: SCOPE_NAMESPACE.into(),
                key: namespace.clone(),
                version,
                payload: document.root.as_bytes().to_vec(),
            },
            expected_version,
        });
    }
    if !transferred {
        return Err(StoreError::Conflict);
    }
    document.set_successor_scopes(versions)?;
    let root = document.row(1, max_row_bytes)?;
    let claim = request.admission.clone().observe_inserted(&root)?;
    // Every old represented provider identity receives an irreversible physical
    // mutation fence. The real MQ control remains with the new namespace owner;
    // the three service infrastructure pins allow only identical payload CAS.
    // No actor/core row, old CALL catalog or old UOW is reassigned or decided.
    let mut frozen = Vec::new();
    let (mut fence_operations, mut fence_bytes) = request.cutoff.captured_usage()?;
    for dependency in &request.cutoff.preserved.provider_dependencies {
        let (namespace, identity_key, retained) = match dependency {
            TerminalRowDependency::Exact(row) => {
                (row.namespace.as_str(), row.key.as_str(), Some(row))
            }
            TerminalRowDependency::Absent { namespace, key } => {
                (namespace.as_str(), key.as_str(), None)
            }
        };
        if namespace.starts_with("durable-") {
            continue;
        }
        let handoff = request
            .handoff_rows
            .iter()
            .position(|identity| identity.namespace == namespace && identity.key == identity_key);
        // The source-derived handoff identities require exact existing rows.
        if retained.is_none() && handoff.is_some() {
            return Err(StoreError::Conflict);
        }
        if handoff == Some(0) {
            continue;
        }
        let key = row_scope_key(namespace, identity_key);
        if frozen
            .iter()
            .any(|write: &ProviderStateWrite| write.record.key == key)
        {
            continue;
        }
        // Legal identity bytes may contain JSON controls. Establish the complete
        // escaped payload bound before json! clones strings or encodes anything.
        pin_payload_bound(&old.root, namespace, identity_key)?;
        // Refuse before the bounded codec allocates its owned strings/bytes.
        fence_operations = fence_operations
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        fence_bytes = fence_bytes
            .checked_add(4096 + PIN_NAMESPACE.len() + namespace.len() + key.len())
            .ok_or(StoreError::CapacityExceeded)?;
        if fence_operations > MAX_ROOT_OPERATIONS || fence_bytes > MAX_ROOT_PAYLOAD_BYTES {
            return Err(StoreError::CapacityExceeded);
        }
        let payload = serde_json::to_vec(&json!({ "schema": PIN_SCHEMA, "owner": old.root,
            "namespace": namespace, "key": identity_key,
            "same_payload": handoff.is_some(), "absent": retained.is_none(),
            "payload_sha256": retained.map(|row| Sha256::digest(&row.payload).to_vec()) }))
        .map_err(|_| StoreError::IncompatibleVersion)?;
        if payload.len() > 4096 {
            return Err(StoreError::CapacityExceeded);
        }
        frozen.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: PIN_NAMESPACE.into(),
                key,
                version: 1,
                payload,
            },
            expected_version: None,
        });
    }
    let marker = ProviderStateRecord {
        namespace: ROOT_CUSTODY_NAMESPACE.into(),
        key: old.root,
        version: request
            .cutoff
            .marker
            .version
            .checked_add(1)
            .ok_or(StoreError::Conflict)?,
        payload: serde_json::to_vec(&json!({ "schema": "mainframe-env.root-custody-successor@1",
            "predecessor_marker_sha256": Sha256::digest(&request.cutoff.marker.payload).as_slice(),
            "successor": document.root, "run": document.run,
            "admitted_row_sha256": Sha256::digest(&root.payload).as_slice(),
            "observed_tick": request.admission.event.tick }))
        .map_err(|_| StoreError::IncompatibleVersion)?,
    };
    let (mut operations, mut bytes) = request.cutoff.captured_usage()?;
    for row in std::iter::once(&root)
        .chain(scopes.iter().map(|w| &w.record))
        .chain(frozen.iter().map(|w| &w.record))
        .chain(std::iter::once(&marker))
    {
        operations = operations
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        bytes = bytes
            .checked_add(row.namespace.len() + row.key.len() + row.payload.len())
            .ok_or(StoreError::CapacityExceeded)?;
    }
    // Actual admission execution/event/outbox plus actor/run/exact-row indexes.
    operations = operations
        .checked_add(5 + document.provider_rows.len())
        .ok_or(StoreError::CapacityExceeded)?;
    for payload in [
        durable::encode_execution(&request.admission.execution)?,
        durable::encode_event(&request.admission.event)?,
        durable::encode_outbox(&request.admission.notification)?,
    ] {
        bytes = bytes
            .checked_add(payload.len())
            .ok_or(StoreError::CapacityExceeded)?;
    }
    bytes = bytes
        .checked_add(
            (2 + document.provider_rows.len())
                .checked_mul(1024)
                .ok_or(StoreError::CapacityExceeded)?,
        )
        .ok_or(StoreError::CapacityExceeded)?;
    if operations > MAX_ROOT_OPERATIONS || bytes > MAX_ROOT_PAYLOAD_BYTES {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(SuccessorPlan {
        document,
        root,
        claim,
        scopes,
        frozen,
        marker: ProviderStateWrite {
            record: marker,
            expected_version: Some(request.cutoff.marker.version),
        },
    })
}

pub(crate) const PIN_NAMESPACE: &str = "durable-root-custody-pin-v1";
const PIN_SCHEMA: &str = "mainframe-env.root-custody-pin@1";

/// Bound the existing exact JSON codec without constructing Value/owned strings.
/// Fixed syntax94 + a worst32-byte decimal digest array129, plus quoted fields.
/// ASCII controls/quotes/slashes follow JSON escapes; non-ASCII UTF-8 bytes use
/// the conservative six-byte bound. Short source-owned ASCII codec is unchanged.
fn pin_payload_bound(owner: &str, namespace: &str, key: &str) -> Result<usize, StoreError> {
    let mut bound = 94_usize + 129;
    for text in [PIN_SCHEMA, owner, namespace, key] {
        bound = bound.checked_add(2).ok_or(StoreError::CapacityExceeded)?;
        for byte in text.bytes() {
            let encoded = match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                0..=31 | 128..=255 => 6,
                _ => 1,
            };
            bound = bound
                .checked_add(encoded)
                .ok_or(StoreError::CapacityExceeded)?;
            if bound > 4096 {
                return Err(StoreError::CapacityExceeded);
            }
        }
    }
    if bound > 4096 {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(bound)
}

/// A durable pin is a structural cutoff record, never actor or writer permission.
pub(crate) fn require_pin(
    pin: &ProviderStateRecord,
    namespace: &str,
    key: &str,
    proposed: Option<&[u8]>,
) -> Result<(), StoreError> {
    if pin.namespace != PIN_NAMESPACE
        || pin.version != 1
        || pin.key != row_scope_key(namespace, key)
        || pin.payload.len() > 4096
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let value: Value =
        serde_json::from_slice(&pin.payload).map_err(|_| StoreError::IncompatibleVersion)?;
    if value.get("schema").and_then(Value::as_str) != Some(PIN_SCHEMA)
        || value.get("namespace").and_then(Value::as_str) != Some(namespace)
        || value.get("key").and_then(Value::as_str) != Some(key)
        || value
            .get("owner")
            .and_then(Value::as_str)
            .is_none_or(|v| v.is_empty() || v.len() > 128)
    {
        return Err(StoreError::IncompatibleVersion);
    }
    if value.get("same_payload").and_then(Value::as_bool) != Some(true) {
        return Err(StoreError::InvalidTransition);
    }
    let proposed = proposed.ok_or(StoreError::InvalidTransition)?;
    let fields = value
        .get("payload_sha256")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 32)
        .ok_or(StoreError::IncompatibleVersion)?;
    let mut expected = [0; 32];
    for (byte, field) in expected.iter_mut().zip(fields) {
        *byte = u8::try_from(field.as_u64().ok_or(StoreError::IncompatibleVersion)?)
            .map_err(|_| StoreError::IncompatibleVersion)?;
    }
    if Sha256::digest(proposed).as_slice() != expected {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
