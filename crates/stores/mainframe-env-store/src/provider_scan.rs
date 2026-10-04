//! Structural namespace-page limits; no publication or runtime authority.

use mainframe_env_store_api::{MAX_PROVIDER_NAMESPACE_BYTES, MAX_PROVIDER_STATE_SCAN, StoreError};

pub(super) const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn limits(namespace: &str, max: usize, max_bytes: usize) -> Result<(), StoreError> {
    if namespace.is_empty() || namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES {
        return Err(StoreError::IncompatibleVersion);
    }
    if max == 0 || max > MAX_PROVIDER_STATE_SCAN || max_bytes == 0 || max_bytes > MAX_PAGE_BYTES {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(())
}

pub(super) fn charge(
    total: &mut usize,
    namespace: usize,
    key: usize,
    payload: usize,
    max_bytes: usize,
) -> Result<(), StoreError> {
    *total = total
        .checked_add(namespace)
        .and_then(|value| value.checked_add(key))
        .and_then(|value| value.checked_add(payload))
        .ok_or(StoreError::CapacityExceeded)?;
    if *total > max_bytes {
        return Err(StoreError::CapacityExceeded);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
