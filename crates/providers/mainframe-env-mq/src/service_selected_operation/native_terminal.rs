//! Prepare native MQ settlement under the existing sole selected authority.
//! Classification belongs to the genuine compiled pre-terminal winner. This
//! private layer receives its same-store closure, never an application MQDISC,
//! fabricated effect, coordinator permit from a row or transport classification.
use super::*;
use crate::mqi_lifecycle::TerminalRoot;
use mainframe_env_execution_api::RootTerminalDisposition;
use mainframe_env_host_api::MqDeliveryOutcome;
use mainframe_env_store_api::{
    RootClosureSnapshot, RootTerminalCommit, RootTerminalPublication, TerminalRowDependency,
};

/// Holds the same service/state/store until whole physical commit and adoption.
/// Dropping a prepared candidate only fences/restores volatile runtime ownership;
/// it never publishes, retires, backs out, commits or invokes external callbacks.
pub(crate) struct SelectedTerminalPreparation<'a> {
    service: &'a MqService,
    original: &'a Invocation,
    frame: FrameLease,
    guard: MutexGuard<'a, rich_state::StoredAuthority>,
    runtime: Option<SelectedRuntime>,
    retirement: Option<TerminalRoot>,
    connection: Option<mainframe_env_host_api::MqHconn>,
    next: Option<rich_state::RichStoredState>,
    mutations: Vec<ProviderStateMutation>,
    closure: RootClosureSnapshot,
    disposition: RootTerminalDisposition,
    last_tick: u64,
    committed: bool,
}

fn require_original(
    original: &Invocation,
    closure: &RootClosureSnapshot,
) -> Result<(), HostProblem> {
    closure.validate_bounds().map_err(store_error)?;
    let admitted = closure.claim.admission();
    if original.parent_execution_id.is_some()
        || original.execution_id != admitted.execution.execution_id
        || original.run_unit_id != admitted.execution.run_unit_id
        || original.artifact != admitted.execution.artifact
        || original.selector != admitted.execution.selector
        || *original.principal.id() != admitted.execution.principal
        || original.attempt != admitted.execution.attempt
        || original.idempotency_key != admitted.invocation_key
        || original.deadline_tick != admitted.deadline_tick
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(())
}

fn exact_record(closure: &RootClosureSnapshot, record: &ProviderStateRecord) -> bool {
    closure
        .provider_dependencies
        .iter()
        .any(|dependency| matches!(dependency, TerminalRowDependency::Exact(old) if old == record))
}

impl MqService {
    /// PRIVATE genuine native driver precondition: hold the live exclusive
    /// compiled winner and prove physical store/router/control/provider identity.
    /// The structural closure/disposition alone attests none of those facts.
    pub(crate) fn prepare_selected_native_terminal<'a>(
        &'a self,
        frame: FrameLease,
        original: &'a Invocation,
        closure: &RootClosureSnapshot,
        disposition: RootTerminalDisposition,
    ) -> Result<SelectedTerminalPreparation<'a>, HostProblem> {
        require_original(original, closure)?;
        let capability = CapabilityId::new("host.mq.write", InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        if !original.principal.has_grant(&capability) {
            return Err(HostProblem::Unauthorized);
        }
        let store = self
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        let clock = self.replay_clock.as_ref().ok_or(HostProblem::Unsupported)?;
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unsupported)?;
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let now = clock.now_tick()?;
        if now < closure.observed_tick
            || now >= original.deadline_tick
            || original.cancellation_requested()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        if store
            .get_provider_state(&closure.closing.namespace, &closure.closing.key)
            .map_err(store_error)?
            .as_ref()
            != Some(&closure.closing)
            || store
                .get_execution(&original.execution_id)
                .map_err(store_error)?
                .as_ref()
                != closure.actors.first().map(|actor| &actor.execution)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let records = state
            .rows
            .captured_records()
            .chain(state.retained_records.iter());
        for record in records {
            if !exact_record(closure, record) {
                return Err(HostProblem::UnknownOutcome);
            }
            // No represented task-end cursor owner/reclamation policy exists in
            // this base. Never delete a cursor by guessing its numeric owner.
            if record.namespace == "mq-delivery-live-v1-cursor" {
                return Err(HostProblem::Unsupported);
            }
        }
        let mut runtime = state.runtime.take().ok_or(HostProblem::Unauthorized)?;
        let prepared = (|| {
            if runtime.fenced {
                return Err(HostProblem::UnknownOutcome);
            }
            let retirement = runtime
                .directory
                .prepare_terminal_root(frame, original, now)?;
            let owner = runtime.directory.owner_for(frame, original, now)?;
            let logical = runtime
                .directory
                .logical_batch_owner(frame, original, now)?;
            let binding = if state.ownership.control.is_some() {
                transition::prior_terminal_connection(state, &mut runtime, &logical, owner)?
            } else {
                if !runtime.connections.is_empty()
                    || !runtime.objects.is_empty()
                    || !state.ownership.units.is_empty()
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                None
            };
            let mut resources = vec![EnterpriseResource::new(
                EnterpriseResourceClass::MqUnitOfWork,
                "CURRENT",
                AccessIntent::Update,
            )?];
            if let Some(binding) = &binding {
                let current = state
                    .ownership
                    .units
                    .get(&binding.unit)
                    .ok_or(HostProblem::UnknownOutcome)?;
                current.require_owner(&logical, &binding.key, &runtime.control, binding.unit)?;
                state
                    .receipts
                    .get(&binding.key)
                    .ok_or(HostProblem::UnknownOutcome)?
                    .require_terminal_connection(
                        &binding.key,
                        &logical,
                        &runtime.control,
                        closure,
                    )?;
                for queue in &current.queues {
                    resources.push(EnterpriseResource::new(
                        EnterpriseResourceClass::MqQueue,
                        queue,
                        AccessIntent::Update,
                    )?);
                }
            }
            for resource in &resources {
                authorizer.authorize(original.principal.id(), resource)?;
            }
            let mut delivery = state.delivery.clone();
            let mut units = state.ownership.units.clone();
            let commit = matches!(disposition, RootTerminalDisposition::Normal { .. });
            if let Some(binding) = &binding {
                let outcome = delivery.unit_outcome(binding.unit);
                match outcome {
                    MqDeliveryOutcome::Pending => {
                        let outcome = if commit {
                            delivery.commit(binding.unit)
                        } else {
                            delivery.backout_complete_zos(binding.unit)
                        }
                        .map_err(|_| HostProblem::UnknownOutcome)?;
                        if outcome
                            != if commit {
                                MqDeliveryOutcome::Accepted
                            } else {
                                MqDeliveryOutcome::Rejected
                            }
                        {
                            return Err(HostProblem::UnknownOutcome);
                        }
                    }
                    MqDeliveryOutcome::UnknownOutcome
                        if units
                            .get(&binding.unit)
                            .is_some_and(|unit| unit.queues.is_empty()) => {}
                    _ => return Err(HostProblem::UnknownOutcome),
                }
                units
                    .get_mut(&binding.unit)
                    .ok_or(HostProblem::UnknownOutcome)?
                    .finalize(commit)?;
            }
            // NO fresh unit/control allocation at native task termination.
            let additions = if state.ownership.control.is_some() {
                state
                    .ownership
                    .changes(&runtime.control, &units, self.limits)?
            } else {
                Vec::new()
            };
            let plan = state
                .plan_selected_delivery(
                    &delivery,
                    additions,
                    rich_state::publication::PublicationLimits::default(),
                )
                .map_err(|_| HostProblem::UnknownOutcome)?;
            let (mutations, next) = plan.into_parts();
            let tick = clock.now_tick()?;
            if tick < now || tick >= original.deadline_tick || original.cancellation_requested() {
                return Err(HostProblem::UnknownOutcome);
            }
            runtime.directory.owner_for(frame, original, tick)?;
            Ok((
                retirement,
                binding.map(|binding| binding.connection),
                mutations,
                next,
                tick,
            ))
        })();
        let (retirement, connection, mutations, next, tick) = match prepared {
            Ok(value) => value,
            Err(problem) => {
                runtime.fenced = true;
                state.runtime = Some(runtime);
                return Err(problem);
            }
        };
        Ok(SelectedTerminalPreparation {
            service: self,
            original,
            frame,
            guard,
            runtime: Some(runtime),
            retirement: Some(retirement),
            connection,
            next: Some(next),
            mutations,
            closure: closure.clone(),
            disposition,
            last_tick: tick,
            committed: false,
        })
    }
}

impl SelectedTerminalPreparation<'_> {
    pub(crate) fn mutations(&self) -> &[ProviderStateMutation] {
        &self.mutations
    }
    pub(crate) fn observed_tick(&self) -> u64 {
        self.last_tick
    }

    /// Caller composes the genuine core winner/COBOL closure and shared canonical
    /// typed audits. It cannot replace the staged MQ delta, original or store.
    pub(crate) fn publish(
        mut self,
        request: RootTerminalPublication,
    ) -> Result<RootTerminalCommit, HostProblem> {
        request.validate_bounds().map_err(store_error)?;
        if request.closure != self.closure
            || request.disposition != self.disposition
            || request.observed_tick < self.last_tick
            || !request.mutations.starts_with(&self.mutations)
            || request
                .audits
                .iter()
                .any(|audit| audit.capability.as_str() != "host.mq.write")
            || request.mutations[self.mutations.len()..]
                .iter()
                .any(|mutation| match mutation {
                    ProviderStateMutation::Put(write) => write.record.namespace.starts_with("mq-"),
                    ProviderStateMutation::Delete { namespace, .. } => namespace.starts_with("mq-"),
                    ProviderStateMutation::Move { record, .. } => {
                        record.namespace.starts_with("mq-")
                    }
                })
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let clock = self
            .service
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        let now = clock.now_tick()?;
        if now < request.observed_tick
            || now >= self.original.deadline_tick
            || self.original.cancellation_requested()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        // The final resource/audits/events are already frozen at the actual
        // requested observation. A changed clock requires a new original plan,
        // not silently rewritten bytes or an automatic retry.
        if now != request.observed_tick {
            return Err(HostProblem::UnknownOutcome);
        }
        self.runtime
            .as_ref()
            .ok_or(HostProblem::UnknownOutcome)?
            .directory
            .owner_for(self.frame, self.original, now)?;
        let store = self
            .service
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        let commit = store
            .commit_root_terminal_step(request)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let next = self.next.take().ok_or(HostProblem::UnknownOutcome)?;
        *self.guard = rich_state::StoredAuthority::Rich(next);
        if self
            .service
            .unknown_after_persist
            .swap(false, Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let tick = clock.now_tick()?;
        if tick < now
            || tick >= self.original.deadline_tick
            || self.original.cancellation_requested()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let runtime = self.runtime.as_mut().ok_or(HostProblem::UnknownOutcome)?;
        runtime
            .directory
            .owner_for(self.frame, self.original, tick)?;
        let retirement = self.retirement.take().ok_or(HostProblem::UnknownOutcome)?;
        runtime
            .directory
            .retire_terminal_root(retirement, &mut runtime.handles.handles_mut())?;
        if let Some(connection) = self.connection {
            transition::remove_retired_bindings(runtime, connection);
        }
        self.committed = true;
        Ok(commit)
    }
}
impl Drop for SelectedTerminalPreparation<'_> {
    fn drop(&mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            if !self.committed {
                runtime.fenced = true;
            }
            if let rich_state::StoredAuthority::Rich(state) = &mut *self.guard {
                state.runtime = Some(runtime);
            }
        }
    }
}
