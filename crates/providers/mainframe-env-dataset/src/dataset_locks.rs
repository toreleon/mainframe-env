//! Dataset-name lock-conflict checks and DELETE lock retention.
//!
//! Split out of `service.rs` to keep it under its ADR-0010 module-review
//! budget (`conformance/subsystems/cics/application/inventory/module-budgets.json`). Covers the
//! CREATE/DEFINE dataset-name reservation conflict check, the DELETE
//! lock-ownership check, and which locks a DELETE removes versus keeps: the
//! deleting transaction's own `Dataset`-target locks survive the delete so
//! the same job's step-end `ReleaseLock` still finds them.

use crate::service::State;
use mainframe_env_host_api::{DatasetLockReceipt, DatasetLockTarget, DatasetName, Mutation};

/// Whether an active `Dataset`-target lock on `dataset`, held by a
/// transaction other than `mutation`'s own, blocks a CREATE or DEFINE.
pub(crate) fn dataset_name_lock_conflicts(
    state: &State,
    dataset: &DatasetName,
    mutation: &Mutation,
) -> bool {
    state.locks.values().any(|lock| {
        lock.dataset == *dataset
            && matches!(lock.target, DatasetLockTarget::Dataset)
            && lock.expires_at > mutation.sequence
            && lock.transaction.as_deref() != mutation.transaction.as_deref()
    })
}

/// Whether an existing transactional lock on `dataset`, owned by neither
/// `delete_mutation`'s transaction nor its `lock_id`, blocks a DELETE.
pub(crate) fn dataset_delete_lock_conflicts(
    state: &State,
    dataset: &DatasetName,
    delete_mutation: &Mutation,
) -> bool {
    state.locks.values().any(|lock| {
        lock.dataset == *dataset
            && lock.transaction.is_some()
            && delete_mutation.transaction.as_deref() != lock.transaction.as_deref()
            && delete_mutation.transaction.as_deref() != Some(lock.lock_id.as_str())
    })
}

/// The subset of `locks` a DELETE removes: everything except the deleting
/// transaction's own `Dataset`-target locks, which stay held until step end.
pub(crate) fn dataset_delete_lock_retention(
    locks: &[DatasetLockReceipt],
    delete_mutation: &Mutation,
) -> Vec<DatasetLockReceipt> {
    locks
        .iter()
        .filter(|lock| {
            !matches!(lock.target, DatasetLockTarget::Dataset)
                || delete_mutation.transaction.is_none()
                || delete_mutation.transaction.as_deref() != lock.transaction.as_deref()
        })
        .cloned()
        .collect::<Vec<_>>()
}
