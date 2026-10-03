//! Root-scoped guards for existing low-level Memory writers.
use super::*;

pub(in crate::memory) fn guard_effect(
    state: &State,
    effect: &EffectRecord,
) -> Result<(), StoreError> {
    if state
        .provider_state
        .contains_key(&(RUN_NAMESPACE.into(), effect.run_unit_id.as_str().into()))
        && !state
            .provider_state
            .contains_key(&(ACTOR_NAMESPACE.into(), effect.execution_id.as_str().into()))
    {
        return Err(StoreError::InvalidTransition);
    }
    if let Some(binding) = state
        .provider_state
        .get(&(ACTOR_NAMESPACE.into(), effect.execution_id.as_str().into()))
    {
        let root =
            std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if Document::read(row(state, ROOT_DRIVER_NAMESPACE, root)?)?.run
            != effect.run_unit_id.as_str()
        {
            return Err(StoreError::InvalidTransition);
        }
    }
    guard_actor(state, &effect.execution_id, None)
}

/// Exact indexed lookup; no global prefix can substitute for membership.
pub(in crate::memory) fn guard_actor(
    state: &State,
    execution: &ExecutionId,
    next: Option<ExecutionState>,
) -> Result<(), StoreError> {
    let Some(binding) = state
        .provider_state
        .get(&(ACTOR_NAMESPACE.into(), execution.as_str().into()))
    else {
        return Ok(());
    };
    let root =
        std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
    let document = Document::read(row(state, ROOT_DRIVER_NAMESPACE, root)?)?;
    if !document
        .actors
        .iter()
        .any(|a| a.execution == execution.as_str())
    {
        return Err(StoreError::IncompatibleVersion);
    }
    if document.phase != Phase::Open
        || (document.root == execution.as_str()
            && next.is_some_and(|s| s.terminal() || s == ExecutionState::Completing))
    {
        return Err(StoreError::InvalidTransition);
    }
    Ok(())
}

pub(in crate::memory) fn guard_unenrolled(
    state: &State,
    execution: &ExecutionRecord,
) -> Result<(), StoreError> {
    if state
        .provider_state
        .contains_key(&(RUN_NAMESPACE.into(), execution.run_unit_id.as_str().into()))
    {
        return Err(StoreError::InvalidTransition);
    }
    Ok(())
}

pub(in crate::memory) fn guard_work(
    state: &State,
    execution: &ExecutionId,
) -> Result<(), StoreError> {
    if state
        .provider_state
        .contains_key(&(ACTOR_NAMESPACE.into(), execution.as_str().into()))
    {
        // Scheduled roots/checkpoint transfer are an explicitly unsupported
        // profile in this synchronous compiled driver. Never silently enqueue.
        return Err(StoreError::InvalidTransition);
    }
    Ok(())
}

pub(in crate::memory) fn guard_outbox_delivery(
    state: &State,
    execution: &ExecutionId,
) -> Result<(), StoreError> {
    let Some(binding) = state
        .provider_state
        .get(&(ACTOR_NAMESPACE.into(), execution.as_str().into()))
    else {
        return Ok(());
    };
    let root =
        std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
    let document = Document::read(row(state, ROOT_DRIVER_NAMESPACE, root)?)?;
    if document.phase == Phase::Closing {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}

pub(in crate::memory) fn guard_provider(
    state: &State,
    namespace: &str,
    key: &str,
    proposed: Option<&[u8]>,
) -> Result<(), StoreError> {
    if namespace.starts_with("durable-root-") {
        return Err(StoreError::InvalidTransition);
    }
    for (index_namespace, index_key) in [
        (SCOPE_NAMESPACE, namespace.to_string()),
        (ROW_SCOPE_NAMESPACE, row_scope_key(namespace, key)),
    ] {
        if let Some(binding) = state
            .provider_state
            .get(&(index_namespace.into(), index_key))
        {
            let root = std::str::from_utf8(&binding.payload)
                .map_err(|_| StoreError::IncompatibleVersion)?;
            let doc = Document::read(row(state, ROOT_DRIVER_NAMESPACE, root)?)?;
            if doc.phase != Phase::Open {
                return Err(StoreError::InvalidTransition);
            }
        }
    }
    if state.provider_state.keys().any(|(n, _)| n == RUN_NAMESPACE)
        && matches!(
            namespace,
            "cobol-call-replay@1"
                | "cobol-call-protocol@2"
                | "cobol-run-state@1"
                | "cobol-cancel@1"
        )
    {
        // This is only an ownership header fence, not CALL/schema acceptance.
        // The server's full codec remains the authority for the row's meaning.
        let old = state.provider_state.get(&(namespace.into(), key.into()));
        for bytes in old
            .map(|r| r.payload.as_slice())
            .into_iter()
            .chain(proposed)
        {
            if bytes.len() > mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES {
                return Err(StoreError::CapacityExceeded);
            }
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                // Exact enrolled identities were checked above. Unowned legacy
                // bytes retain their original validation authority/behavior.
                continue;
            };
            if let Some(run) = value
                .get("owner_run_unit")
                .and_then(serde_json::Value::as_str)
            {
                if let Some(binding) = state
                    .provider_state
                    .get(&(RUN_NAMESPACE.into(), run.into()))
                {
                    let root = std::str::from_utf8(&binding.payload)
                        .map_err(|_| StoreError::IncompatibleVersion)?;
                    let doc = Document::read(row(state, ROOT_DRIVER_NAMESPACE, root)?)?;
                    if doc.phase != Phase::Open {
                        return Err(StoreError::InvalidTransition);
                    }
                }
            }
        }
    }
    Ok(())
}
