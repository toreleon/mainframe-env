//! Resolve a lane from an authenticated task and the held lifecycle epoch.

use super::state::{ContainerOwner, valid_name};
use crate::service::handlers::bts_lifecycle::BtsLifecycleStore;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateStore;

#[derive(Clone, Copy)]
pub(super) struct OwnerIdentity<'a> {
    pub run_unit: &'a str,
    pub execution: &'a str,
    pub principal: &'a str,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(in crate::service::handlers) enum ContainerSelector<'a> {
    Channel(&'a str),
    Process {
        process_type: &'a str,
        process_name: &'a str,
        epoch: u64,
    },
    Activity {
        process_type: &'a str,
        process_name: &'a str,
        activity_id: &'a str,
        epoch: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResolvedScope {
    pub owner: ContainerOwner,
    pub class: &'static str,
    pub resource: String,
    pub process_version: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ScopeError {
    Unauthorized,
    StaleEpoch,
    Bounds,
    Backend(HostProblem),
}

pub(super) fn resolve(
    store: &dyn ProviderStateStore,
    identity: OwnerIdentity<'_>,
    selector: ContainerSelector<'_>,
) -> Result<ResolvedScope, ScopeError> {
    if [identity.run_unit, identity.execution, identity.principal]
        .iter()
        .any(|part| part.is_empty() || part.len() > 256)
    {
        return Err(ScopeError::Unauthorized);
    }
    if let ContainerSelector::Channel(channel) = selector {
        if !valid_name(channel, 16) {
            return Err(ScopeError::Bounds);
        }
        return Ok(ResolvedScope {
            owner: ContainerOwner::Channel {
                execution: identity.execution.into(),
                principal: identity.principal.into(),
                run_unit: identity.run_unit.into(),
                channel: channel.into(),
            },
            class: "CICSCHAN",
            resource: format!("CICS.CHANNEL.{channel}"),
            process_version: None,
        });
    }
    let (process_type, process_name, epoch, activity_id) = match selector {
        ContainerSelector::Process {
            process_type,
            process_name,
            epoch,
        } => (process_type, process_name, epoch, None),
        ContainerSelector::Activity {
            process_type,
            process_name,
            activity_id,
            epoch,
        } => (process_type, process_name, epoch, Some(activity_id)),
        ContainerSelector::Channel(_) => unreachable!(),
    };
    if !valid_name(process_type, 8) || !valid_name(process_name, 36) || epoch == 0 {
        return Err(ScopeError::Bounds);
    }
    let lifecycle = BtsLifecycleStore::new(store);
    let held = lifecycle
        .acquired_process_container_scope(identity.run_unit, identity.execution, identity.principal)
        .map_err(|problem| match problem {
            HostProblem::IdempotencyConflict => ScopeError::Unauthorized,
            other => ScopeError::Backend(other),
        })?
        .ok_or(ScopeError::StaleEpoch)?;
    if held.acquisition_epoch != epoch {
        return Err(ScopeError::StaleEpoch);
    }
    if held.process_type != process_type || held.process_name != process_name {
        return Err(ScopeError::Unauthorized);
    }
    let process = lifecycle
        .load_process(process_type, process_name)
        .map_err(ScopeError::Backend)?
        .ok_or(ScopeError::StaleEpoch)?;
    if !process.visible_to(identity.run_unit) {
        return Err(ScopeError::Unauthorized);
    }
    let owner = if let Some(activity_id) = activity_id {
        if activity_id != held.acquired_activity_id {
            return Err(ScopeError::Unauthorized);
        }
        let activity = process
            .activities
            .get(activity_id)
            .ok_or(ScopeError::StaleEpoch)?;
        if activity
            .pending_uow
            .as_deref()
            .is_some_and(|uow| uow != identity.run_unit)
        {
            return Err(ScopeError::Unauthorized);
        }
        ContainerOwner::Activity {
            process_type: process_type.into(),
            process_name: process_name.into(),
            root_activity_id: process.root_id.clone(),
            activity_id: activity_id.into(),
        }
    } else {
        ContainerOwner::Process {
            process_type: process_type.into(),
            process_name: process_name.into(),
            root_activity_id: process.root_id.clone(),
        }
    };
    Ok(ResolvedScope {
        owner,
        class: "BTSLIFE",
        resource: BtsLifecycleStore::saf_resource(process_type, process_name)
            .map_err(ScopeError::Backend)?,
        process_version: Some(process.row_version),
    })
}
