//! Prior ordinary connection selection, never allocation or an owner constructor.
use super::*;
use mainframe_env_host_api::MqHandleProblem;

pub(super) fn validate_profile(
    state: &rich_state::RichStoredState,
    connect: &MqMqiConnect,
) -> Result<(), HostProblem> {
    if connect.options != MqMqiOptions::ContractDefault
        || connect.sharing != MqHandleSharing::NonShared
    {
        return Err(HostProblem::Unsupported);
    }
    if connect
        .manager
        .as_ref()
        .is_some_and(|name| name.as_str() != state.catalog.queue_manager().name.as_str())
    {
        return Err(HostProblem::NotFound);
    }
    Ok(())
}

pub(super) fn prior(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
) -> Result<Option<ConnectionBinding>, HostProblem> {
    if owner.environment != MqHostEnvironment::ZosBatch || logical.owner() != owner {
        return Err(HostProblem::Unauthorized);
    }
    // This selected subset issues only indexed connections and objects. Missing
    // or duplicate indexes cannot silently become a new first connection.
    let indexed = runtime
        .connections
        .len()
        .checked_add(runtime.objects.len())
        .ok_or(HostProblem::ResourceExhausted)?;
    if runtime.handles.handles_mut().active_handles() != indexed {
        return Err(HostProblem::Malformed);
    }
    let mut selected = None;
    for (index, binding) in runtime.connections.iter().enumerate() {
        if runtime.connections[..index].iter().any(|old| {
            old.connection == binding.connection
                || old.key == binding.key
                || old.unit == binding.unit
        }) {
            return Err(HostProblem::Malformed);
        }
        match runtime
            .handles
            .handles_mut()
            .validate_connection(owner, binding.connection)
        {
            Ok(()) => {
                if selected.is_some() || state.ownership.control.as_ref() != Some(&runtime.control)
                {
                    return Err(HostProblem::Malformed);
                }
                state
                    .ownership
                    .units
                    .get(&binding.unit)
                    .ok_or(HostProblem::Malformed)?
                    .require_owner(logical, &binding.key, &runtime.control, binding.unit)?;
                selected = Some(binding.clone());
            }
            // A live connection owned by another admitted task is not ours.
            // All other lifetime/kind/foreign-registry failures are corruption.
            Err(MqHandleProblem::CrossOwner) => {}
            Err(_) => return Err(HostProblem::Malformed),
        }
    }
    Ok(selected)
}
