//! Read-only native observations of the SAME selected authority, never permits.
use super::*;
use crate::delivery::full_message::QueueProfile;
use crate::trusted_batch_embedding::MqTrustedBatchPointTarget;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_store_api::ExecutionState;

struct ClockObservation<'a>(&'a std::sync::atomic::AtomicBool);
impl Drop for ClockObservation<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
fn observed_clock(
    service: &MqService,
    clock: &dyn crate::MqReplayClock,
) -> Result<u64, HostProblem> {
    if service
        .producer_sampling
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(HostProblem::Unsupported);
    }
    let _observation = ClockObservation(&service.producer_sampling);
    let tick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clock.now_tick()))
        .map_err(|_| HostProblem::ProviderFailure)??;
    if tick == 0 {
        return Err(HostProblem::Malformed);
    }
    Ok(tick)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StructureFacts {
    pub(crate) catalog: Arc<MqObjectCatalog>,
    pub(crate) generation: u64,
    pub(crate) fence: u64,
    pub(crate) incarnation: u64,
    pub(crate) owner: MqHandleOwner,
    pub(crate) unit: u64,
    pub(crate) characters: MqMdCharacterEncoding,
    pub(crate) ccsid: i32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PointFacts {
    pub(crate) structure: StructureFacts,
    pub(crate) queue: crate::MqObjectName,
    pub(crate) version: i32,
    pub(crate) max_message_bytes: usize,
}

fn running(
    store: &dyn mainframe_env_store_api::PlatformStore,
    inv: &Invocation,
    now: u64,
) -> Result<(), HostProblem> {
    let execution = store
        .get_execution(&inv.execution_id)
        .map_err(store_error)?
        .ok_or(HostProblem::Unauthorized)?;
    if execution.execution_id != inv.execution_id
        || execution.run_unit_id != inv.run_unit_id
        || execution.principal != *inv.principal.id()
        || execution.attempt != inv.attempt
        || execution.selector != inv.selector
        || execution.artifact != inv.artifact
        || execution.version == 0
        || execution.state != ExecutionState::Running
        || execution.terminal_tick.is_some()
        || execution.lease_expiry_tick.is_some_and(|t| t <= now)
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(())
}
fn physical(service: &MqService, state: &rich_state::RichStoredState) -> Result<(), HostProblem> {
    let store = service
        .selected_store
        .as_ref()
        .ok_or(HostProblem::Unsupported)?;
    let (generation, fence) = state.marker.identity.generation_and_fence();
    // One bounded physical snapshot, decoded by the sole stored-state authority.
    // Never merge catalog/meta/unit observations from different physical scans.
    let rich_state::StoredAuthority::Rich(fresh) = rich_state::read(
        &**store,
        generation,
        fence,
        rich_state::ReaderLimits {
            legacy: service.limits,
            ..Default::default()
        },
    )
    .map_err(selection::selection_error)?
    else {
        return Err(HostProblem::Unsupported);
    };
    if fresh.marker != state.marker
        || fresh.versions != state.versions
        || fresh.catalog != state.catalog
        || fresh.ownership.control != state.ownership.control
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

impl MqService {
    #[cfg(test)]
    pub(crate) fn native_test_profile(
        &self,
        queue: crate::MqObjectName,
        version: i32,
        characters: MqMdCharacterEncoding,
    ) {
        let state = self.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(state) = &*state else {
            panic!()
        };
        let plan = state
            .plan_profile_upgrade(
                &BTreeMap::from([(
                    queue,
                    QueueProfile::Complete {
                        version,
                        characters,
                    },
                )]),
                Default::default(),
            )
            .unwrap();
        self.selected_store
            .as_ref()
            .unwrap()
            .mutate_provider_states_atomic(plan.into_parts().0)
            .unwrap();
    }

    pub(crate) fn native_structure(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        call: MqMqiCall,
        connection: MqHconn,
    ) -> Result<StructureFacts, HostProblem> {
        self.native_observe(frame, invocation, call, connection, None, None)
            .map(|r| r.0)
    }
    pub(crate) fn native_point(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        call: MqMqiCall,
        connection: MqHconn,
        target: &MqTrustedBatchPointTarget,
        closed: Option<&PointFacts>,
    ) -> Result<PointFacts, HostProblem> {
        self.native_observe(frame, invocation, call, connection, Some(target), closed)?
            .1
            .ok_or(HostProblem::Unsupported)
    }
    fn native_observe(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        call: MqMqiCall,
        connection: MqHconn,
        target: Option<&MqTrustedBatchPointTarget>,
        closed: Option<&PointFacts>,
    ) -> Result<(StructureFacts, Option<PointFacts>), HostProblem> {
        if !matches!(
            call,
            MqMqiCall::Open | MqMqiCall::Close | MqMqiCall::Put | MqMqiCall::PutOne
        ) || !matches!(connection, MqHconn::Issued(_))
        {
            return Err(HostProblem::Unsupported);
        }
        let guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &*guard else {
            return Err(HostProblem::Unsupported);
        };
        let runtime = state.runtime.as_ref().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let clock = self.replay_clock.as_ref().ok_or(HostProblem::Unsupported)?;
        let now = observed_clock(self, &**clock)?;
        if now < state.delivery.tick() {
            return Err(HostProblem::Malformed);
        }
        let owner = runtime.directory.owner_for(frame, invocation, now)?;
        let logical = runtime
            .directory
            .logical_batch_owner(frame, invocation, now)?;
        if owner.environment != MqHostEnvironment::ZosBatch
            || runtime
                .directory
                .context_for(frame, invocation, now)?
                .require(invocation, owner)?
                .owner
                != MqSyncpointOwner::QueueManager
            || !invocation.principal.has_grant(
                &CapabilityId::new("host.mq.write", Default::default())
                    .map_err(|_| HostProblem::Malformed)?,
            )
        {
            return Err(HostProblem::Unauthorized);
        }
        // This selected profile issues ONLY ordinary nonshared connections (the
        // connection-warning validator). Validation observes the same registry,
        // without accessor reclamation, allocation, token reconstruction or SAF.
        let registry = runtime.handles.registry();
        registry
            .validate_connection(owner, connection)
            .map_err(handle_error)?;
        let binding = runtime
            .connections
            .iter()
            .find(|b| b.connection == connection)
            .ok_or(HostProblem::Malformed)?;
        if state.ownership.control.as_ref() != Some(&runtime.control) {
            return Err(HostProblem::UnknownOutcome);
        }
        state
            .ownership
            .units
            .get(&binding.unit)
            .ok_or(HostProblem::Malformed)?
            .require_owner(&logical, &binding.key, &runtime.control, binding.unit)?;
        let attrs = state
            .catalog
            .native_attributes()
            .ok_or(HostProblem::Unsupported)?;
        let structure = StructureFacts {
            catalog: state.catalog.clone(),
            generation: runtime.control.generation,
            fence: runtime.control.fence,
            incarnation: runtime.control.registry_epoch,
            owner,
            unit: binding.unit,
            characters: attrs.characters.md(),
            ccsid: attrs.coded_char_set_id,
        };
        let point = if let Some(target) = target {
            let queue = match (call, target) {
                (MqMqiCall::Open, MqTrustedBatchPointTarget::Open { lookup, access })
                    if matches!(
                        access,
                        MqRouteOpenAccess::Output | MqRouteOpenAccess::InputShared
                    ) =>
                {
                    local_lookup(lookup)?
                }
                (MqMqiCall::PutOne, MqTrustedBatchPointTarget::PutOne { lookup }) => {
                    local_lookup(lookup)?
                }
                (MqMqiCall::Put | MqMqiCall::Close, MqTrustedBatchPointTarget::Object(object)) => {
                    if let Some(old) = closed {
                        if call != MqMqiCall::Close {
                            return Err(HostProblem::Unsupported);
                        }
                        // Encoding after known CLOSE does not revive/revalidate a
                        // retired HOBJ. The opaque captured queue is observation,
                        // never another request target or live registry authority.
                        old.queue.clone()
                    } else {
                        registry
                            .validate(owner, connection, (*object).into(), MqHandleKind::Object)
                            .map_err(handle_error)?;
                        let object = runtime
                            .objects
                            .iter()
                            .find(|b| b.connection == connection && b.object == *object)
                            .ok_or(HostProblem::Unauthorized)?;
                        if object.path != [object.queue.as_str()]
                            || (call == MqMqiCall::Put
                                && !object.access.contains(&MqRouteOpenAccess::Output))
                        {
                            return Err(HostProblem::Unsupported);
                        }
                        object.queue.clone()
                    }
                }
                _ => return Err(HostProblem::Unsupported),
            };
            if !state.catalog.definitions().any(|d| {
                matches!(d, crate::MqObjectDefinition::LocalQueue {
                name, usage: crate::MqLocalQueueUsage::Normal, .. } if *name == queue)
            }) {
                return Err(HostProblem::Unsupported);
            }
            let q = attrs
                .queues
                .iter()
                .find(|a| a.name == queue)
                .ok_or(HostProblem::Unsupported)?;
            if q.delivery_sequence != crate::MqNativeDeliverySequence::Fifo {
                return Err(HostProblem::Unsupported);
            }
            let QueueProfile::Complete {
                version,
                characters,
            } = state
                .delivery
                .full_queue_profile(&queue)
                .map_err(delivery_error)?
            else {
                return Err(HostProblem::Unsupported);
            };
            if characters != structure.characters {
                return Err(HostProblem::Unsupported);
            }
            Some(PointFacts {
                structure: structure.clone(),
                queue,
                version,
                max_message_bytes: attrs.max_msg_length.min(q.max_msg_length) as usize,
            })
        } else {
            None
        };
        let store = self
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        running(&**store, invocation, now)?;
        // No context/GMT/encoder sampling; no mutable registry borrow survives.
        super::super::producer::check_live(self, invocation)?;
        let after = observed_clock(self, &**clock)?;
        if after < now {
            return Err(HostProblem::Malformed);
        }
        runtime.directory.owner_for(frame, invocation, after)?;
        running(&**store, invocation, after)?;
        // Every host callback has finished before the final physical snapshot.
        // A clock/source callback cannot advance catalog/control dependencies
        // after the only comparison and still return stale usable facts.
        physical(self, state)?;
        Ok((structure, point))
    }
}
fn local_lookup(lookup: &MqRouteLookup) -> Result<crate::MqObjectName, HostProblem> {
    match lookup {
        MqRouteLookup::Queue {
            name,
            manager: None,
            dynamic_pattern: None,
        } => crate::MqObjectName::new(name.as_str()).map_err(|_| HostProblem::Malformed),
        _ => Err(HostProblem::Unsupported),
    }
}
