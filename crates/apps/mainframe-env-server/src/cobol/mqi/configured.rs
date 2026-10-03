//! Deliberate ordinary installed-batch setup; never ProductServer fallback.
use super::{InstalledBatchAdmission, InstalledMqFrameSession, ProgramMqHostAdmission};
use crate::cobol::ProgramExecutionControl;
use mainframe_env_execution_api::{ExecutionId, Invocation};
use mainframe_env_host_api::mq_mqi::MqMqiLimits;
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, EnterpriseAuthorizer, HostLimits,
    HostProblem, HostProvider,
};
use mainframe_env_interpreter::{ExecutionControl, ExecutionControlError};
use mainframe_env_mq::{
    MqLimits, MqReplayClock, MqTrustedBatchRelationship, MqTrustedBatchRoot, MqTrustedBatchRuntime,
};
use mainframe_env_store_api::PlatformStore;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};
mod budget;
mod frame;
pub(in crate::cobol) mod native_call;
mod native_point;
mod native_root;
use frame::{ClosedFrame, Session};

/// Finite retained topology bounds. Revoked entries consume slots until a
/// separate source-bound task-end/recovery authority disposes of them.
#[derive(Clone, Copy, Debug)]
pub struct InstalledMqHostBounds {
    pub max_roots: usize,
    pub max_frames: usize,
}
enum RootEntry {
    Preparing,
    Retained {
        root: Arc<MqTrustedBatchRoot>,
        frame: Arc<ClosedFrame>,
        native: Option<mainframe_env_store_api::RootDriverClaim>,
    },
}
enum FrameEntry {
    Preparing,
    Retained(Arc<ClosedFrame>),
}
#[derive(Default)]
struct Topology {
    bytes: usize,
    roots: BTreeMap<ExecutionId, RootEntry>,
    frames: BTreeMap<ExecutionId, FrameEntry>,
}
struct ClockControl(Arc<dyn MqReplayClock>);
impl ProgramExecutionControl for ClockControl {
    fn observe(&self, original: &Invocation) -> Result<ExecutionControl, ExecutionControlError> {
        let now_tick = self
            .0
            .now_tick()
            .map_err(|_| ExecutionControlError::Unavailable)?;
        if now_tick == 0 || now_tick > i64::MAX as u64 {
            return Err(ExecutionControlError::Unavailable);
        }
        Ok(ExecutionControl {
            now_tick,
            cancellation_requested: original.cancellation_requested(),
        })
    }
}

/// PRIVILEGED RUST EMBEDDING: explicitly configured ordinary ZosBatch SAME TASK
/// installed CALL producer and its physical MQ provider. Application JSON,
/// matching identifiers and binding bytes cannot construct an admission proof.
/// Register this SAME Arc as HostProvider, bind its exact execution_control Arc
/// and itself as ProgramMqHostAdmission before router runtime publication.
/// Opens only existing rich state; does not switch ProductServer defaults,
/// normalize deployment state, infer task-end or grant shared-participant/all26
/// acceptance. The genuine server proof carries original compiled/core/CALL
/// authority; the configuration independently chooses ordinary SAME TASK.
pub struct ConfiguredInstalledMqHost {
    runtime: MqTrustedBatchRuntime,
    store: Arc<dyn PlatformStore>,
    control: Arc<dyn ProgramExecutionControl>,
    descriptor: CapabilityDescriptor,
    host_limits: HostLimits,
    bounds: InstalledMqHostBounds,
    mq_limits: MqLimits,
    mqi_limits: MqMqiLimits,
    generation: u64,
    fence: u64,
    topology: Mutex<Topology>,
    this: Weak<Self>,
    native_points: bool,
    // Read-only test observation of the real route, never a replacement provider.
    #[cfg(test)]
    originals: Mutex<Vec<(Invocation, EffectRequest, EffectResult)>>,
}
impl ConfiguredInstalledMqHost {
    /// Strict selected open with one store, mandatory SAF and one replay/control
    /// clock. Deployment/recovery supplies the trusted generation and fence.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        store: Arc<dyn PlatformStore>,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        clock: Arc<dyn MqReplayClock>,
        descriptor: CapabilityDescriptor,
        mq_limits: MqLimits,
        host_limits: HostLimits,
        mqi_limits: MqMqiLimits,
        generation: u64,
        fence: u64,
        bounds: InstalledMqHostBounds,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_configured(
            store,
            authorizer,
            clock,
            descriptor,
            mq_limits,
            host_limits,
            mqi_limits,
            generation,
            fence,
            bounds,
            false,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn open_configured(
        store: Arc<dyn PlatformStore>,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        clock: Arc<dyn MqReplayClock>,
        descriptor: CapabilityDescriptor,
        mq_limits: MqLimits,
        host_limits: HostLimits,
        mqi_limits: MqMqiLimits,
        generation: u64,
        fence: u64,
        bounds: InstalledMqHostBounds,
        native_points: bool,
    ) -> Result<Arc<Self>, HostProblem> {
        if bounds.max_roots == 0
            || bounds.max_roots > 4096
            || bounds.max_frames == 0
            || bounds.max_frames > 4096
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let control: Arc<dyn ProgramExecutionControl> = Arc::new(ClockControl(clock.clone()));
        let mut runtime = MqTrustedBatchRuntime::open(
            store.clone(),
            mq_limits,
            generation,
            fence,
            authorizer,
            clock,
            descriptor.clone(),
            host_limits,
            mqi_limits,
        )?;
        let source = native_points.then(|| {
            Arc::new(native_point::Source::new(
                store.clone(),
                host_limits.max_name_bytes,
            ))
        });
        if let Some(source) = &source {
            runtime.configure_producer_source(&store, source.clone())?;
        }
        let host = Arc::new_cyclic(|this| Self {
            runtime,
            store,
            control,
            descriptor,
            host_limits,
            bounds,
            mq_limits,
            mqi_limits,
            generation,
            fence,
            topology: Mutex::new(Topology::default()),
            this: this.clone(),
            native_points,
            #[cfg(test)]
            originals: Mutex::new(Vec::new()),
        });
        if let Some(source) = source {
            source.bind(&host)?;
        }
        Ok(host)
    }
    /// Exact physical control for frozen router binding, not a clock adapter
    /// evaluated with a fabricated Invocation.
    pub fn execution_control(&self) -> Arc<dyn ProgramExecutionControl> {
        self.control.clone()
    }
    fn parent_frame(&self, original: &Invocation) -> Result<Arc<ClosedFrame>, HostProblem> {
        if original.parent_execution_id.is_some() {
            let frame = {
                let map = self
                    .topology
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                match map.frames.get(&original.execution_id) {
                    Some(FrameEntry::Retained(frame)) => frame.clone(),
                    _ => return Err(HostProblem::Unauthorized),
                }
            };
            frame.check_original(original)?;
            return Ok(frame);
        }
        let charge = budget::charge(original, self.host_limits.max_state_bytes)?;
        {
            let mut map = self
                .topology
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            match map.roots.get(&original.execution_id) {
                Some(RootEntry::Retained { root, frame, .. }) => {
                    if root.original() != original {
                        return Err(HostProblem::Unauthorized);
                    }
                    return Ok(frame.clone());
                }
                Some(RootEntry::Preparing) => return Err(HostProblem::IdempotencyConflict),
                None => {}
            }
            if self.native_points {
                // Native points require eager genuine compiled root association;
                // a lazy fixture/local parent is not its root producer.
                return Err(HostProblem::Unsupported);
            }
            if map.roots.len() >= self.bounds.max_roots {
                return Err(HostProblem::ResourceExhausted);
            }
            map.reserve_bytes(charge, self.host_limits.max_state_bytes)?;
            map.roots
                .insert(original.execution_id.clone(), RootEntry::Preparing);
        }
        // No topology lock spans directory/service callbacks.
        let prepared = self.runtime.admit_root(original.clone()).map(|root| {
            let root = Arc::new(root);
            let frame = Arc::new(ClosedFrame::new(root.frame(), self.control.clone(), 0));
            (root, frame)
        });
        let mut map = self
            .topology
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        match prepared {
            Ok((root, frame)) => {
                map.roots.insert(
                    original.execution_id.clone(),
                    RootEntry::Retained {
                        root,
                        frame: frame.clone(),
                        native: None,
                    },
                );
                Ok(frame)
            }
            Err(problem) => {
                map.roots.remove(&original.execution_id);
                map.release_bytes(charge)?;
                Err(problem)
            }
        }
    }
    fn prepare(
        &self,
        proof: &InstalledBatchAdmission<'_>,
    ) -> Result<Arc<ClosedFrame>, HostProblem> {
        let child = proof.child();
        let mut charge = budget::charge(child, self.host_limits.max_state_bytes)?;
        if proof.native_root_owned() {
            charge = charge
                .checked_add(frame::CompiledFrame::charge(
                    proof,
                    self.host_limits.max_state_bytes,
                )?)
                .ok_or(HostProblem::ResourceExhausted)?;
            if charge > self.host_limits.max_state_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
        }
        {
            let mut map = self
                .topology
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if map.frames.contains_key(&child.execution_id) {
                return Err(HostProblem::IdempotencyConflict);
            }
            if map.frames.len() >= self.bounds.max_frames {
                return Err(HostProblem::ResourceExhausted);
            }
            map.reserve_bytes(charge, self.host_limits.max_state_bytes)?;
            map.frames
                .insert(child.execution_id.clone(), FrameEntry::Preparing);
        }
        let mut created = None;
        let prepared = (|| {
            let parent = self.parent_frame(proof.parent())?;
            parent.with_active(proof.parent(), |state| {
                let facet = self.runtime.prepare_same_task_child(
                    &state.facet,
                    child.clone(),
                    MqTrustedBatchRelationship::SameTaskCall,
                )?;
                let compiled = if proof.native_root_owned() {
                    frame::CompiledFrame::from_checked(proof)
                } else {
                    None
                };
                let frame = Arc::new(
                    ClosedFrame::new_compiled(
                        facet,
                        self.control.clone(),
                        proof.observed_control().now_tick,
                        compiled,
                    )
                    .with_abi(parent.abi.clone())?,
                );
                created = Some(frame.clone());
                Ok(frame)
            })
        })();
        let mut map = self
            .topology
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        match prepared {
            Ok(frame) => {
                map.frames.insert(
                    child.execution_id.clone(),
                    FrameEntry::Retained(frame.clone()),
                );
                Ok(frame)
            }
            Err(problem) => {
                if let Some(frame) = &created {
                    map.frames.insert(
                        child.execution_id.clone(),
                        FrameEntry::Retained(frame.clone()),
                    );
                } else {
                    map.frames.remove(&child.execution_id);
                    map.release_bytes(charge)?;
                }
                drop(map);
                // A post-preparation parent control/revocation failure owns
                // only this new child. Never retire the surviving parent/root.
                if let Some(frame) = created {
                    frame.abort()?;
                }
                Err(problem)
            }
        }
    }
}
impl ProgramMqHostAdmission for ConfiguredInstalledMqHost {
    fn admit_installed_batch(
        &self,
        proof: &InstalledBatchAdmission<'_>,
    ) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem> {
        if !Arc::ptr_eq(&self.store, proof.store())
            || !Arc::ptr_eq(&self.control, proof.execution_control())
        {
            return Err(HostProblem::Unauthorized);
        }
        let expected: Arc<dyn HostProvider> =
            self.this.upgrade().ok_or(HostProblem::Unauthorized)?;
        if !proof
            .host_runtime()
            .selects_same_provider(&self.descriptor.capability, &expected)?
        {
            return Err(HostProblem::Unauthorized);
        }
        let frame = self.prepare(proof)?;
        Ok(Box::new(Session::new(
            frame,
            self.store.clone(),
            self.control.clone(),
        )))
    }
}
impl HostProvider for ConfiguredInstalledMqHost {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, original: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = (|| {
            let occurrence = effect
                .mq_mqi_occurrence(self.host_limits)?
                .ok_or(HostProblem::Unsupported)?;
            let frame = {
                let map = self
                    .topology
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                match map.frames.get(&original.execution_id) {
                    Some(FrameEntry::Retained(frame)) => frame.clone(),
                    _ => match map.roots.get(&original.execution_id) {
                        Some(RootEntry::Retained {
                            frame,
                            native: Some(_),
                            ..
                        }) if original.parent_execution_id.is_none() => frame.clone(),
                        _ => return Err(HostProblem::Unauthorized),
                    },
                }
            };
            frame.dispatch(original, occurrence)
        })();
        let reply = match outcome {
            Ok(reply) => reply,
            Err(problem) => EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem),
            },
        };
        #[cfg(test)]
        {
            let mut originals = self.originals.lock().unwrap();
            assert!(originals.len() < 4096, "bounded test observation");
            originals.push((original.clone(), effect.clone(), reply.clone()));
        }
        reply
    }
}
#[cfg(test)]
mod tests;
