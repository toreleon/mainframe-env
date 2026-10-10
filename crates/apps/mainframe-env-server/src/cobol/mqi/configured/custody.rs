//! Closed custody derived from the real configured driver and retained root.
//! No copied claim, filename, matching-ID or application-grant constructor.
use super::*;
use mainframe_env_store_api::{ExecutionState, RootCustodyCutoff};
use std::sync::atomic::Ordering;

/// Deliberately installed privileged Rust manager for this exact physical host.
/// It is not Clone/Serde and cannot be installed after native drive has begun.
pub struct NativeMqRootManager {
    host: Arc<ConfiguredInstalledMqHost>,
}

/// Actual same-host root custody after irreversible volatile and durable cutoff.
/// Drop preserves unknown work; it never settles, restores or admits a successor.
pub struct NativeMqRootCustody {
    host: Arc<ConfiguredInstalledMqHost>,
    original: Invocation,
    root: Arc<MqTrustedBatchRoot>,
    capture: RootCustodyCutoff,
}
impl NativeMqRootCustody {
    /// Read-only complete storage observation; this cannot mint another custody.
    pub fn captured(&self) -> &RootCustodyCutoff {
        &self.capture
    }
    /// Frozen genuine compiled invocation, including grants and original deadline.
    pub fn original(&self) -> &Invocation {
        &self.original
    }
    /// Observe whether the exact originating embedding remains retained.
    /// This grants no authority from a substituted host or root claim.
    pub fn same_host(&self, host: &Arc<ConfiguredInstalledMqHost>) -> bool {
        Arc::ptr_eq(&self.host, host) && self.root.native_authority_revoked()
    }
}
impl ConfiguredInstalledMqHost {
    /// Privileged Rust setup only. The real admitted native root must later
    /// populate this same host's closed topology through NativeRootHooks.
    pub fn install_native_root_manager(
        self: &Arc<Self>,
    ) -> Result<NativeMqRootManager, HostProblem> {
        if !self.native_points || !self.store.root_custody_supported() {
            return Err(HostProblem::Unsupported);
        }
        let topology = self
            .topology
            .try_lock()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if !topology.roots.is_empty() || !topology.frames.is_empty() {
            return Err(HostProblem::Unauthorized);
        }
        self.custody_installed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| HostProblem::Unauthorized)?;
        Ok(NativeMqRootManager { host: self.clone() })
    }
}
impl NativeMqRootManager {
    /// Cut off one actual live synchronous compiled root. This is an explicit
    /// owning infrastructure decision, never an inferred MQ task-end or backout.
    /// All callbacks precede topology/backend locks. Revocation uses only atomics
    /// and never waits for the old frame or selected-service mutex.
    pub fn cutoff_live_root(
        &self,
        original: &Invocation,
    ) -> Result<NativeMqRootCustody, HostProblem> {
        if original.parent_execution_id.is_some() || original.cancellation_requested() {
            return Err(HostProblem::Unauthorized);
        }
        let tick = self.host.clock.custody_tick()?;
        if tick == 0 || tick >= original.deadline_tick || original.deadline_tick > i64::MAX as u64 {
            return Err(HostProblem::UnknownOutcome);
        }
        let (root, claim, frames) = {
            let topology = self
                .host
                .topology
                .try_lock()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            // First profile owns exactly one selected MQ graph/authority.
            // Preparing or foreign topology cannot be silently omitted.
            if topology.roots.len() != 1
                || topology.frames.len() > mainframe_env_store_api::MAX_ROOT_ACTORS - 1
            {
                return Err(HostProblem::UnknownOutcome);
            }
            let Some(RootEntry::Retained {
                root,
                frame,
                native: Some(claim),
            }) = topology.roots.get(&original.execution_id)
            else {
                return Err(HostProblem::Unauthorized);
            };
            if frame.original() != original
                || !frame.same_control(&self.host.control)
                || frame.compiled().is_none()
                || claim.admission().execution.execution_id != original.execution_id
                || claim.admission().execution.run_unit_id != original.run_unit_id
                || claim.admission().execution.principal != *original.principal.id()
                || claim.admission().execution.attempt != original.attempt
                || claim.admission().execution.artifact != original.artifact
                || claim.admission().execution.selector != original.selector
                || claim.admission().invocation_key != original.idempotency_key
                || claim.admission().deadline_tick != original.deadline_tick
            {
                return Err(HostProblem::Unauthorized);
            }
            frame.check_original(original)?;
            let mut frames = vec![frame.clone()];
            for entry in topology.frames.values() {
                let FrameEntry::Retained(frame) = entry else {
                    return Err(HostProblem::UnknownOutcome);
                };
                if frame.native_root_execution() != original.execution_id
                    || !frame.same_control(&self.host.control)
                    || frame.compiled().is_none()
                {
                    return Err(HostProblem::Unauthorized);
                }
                frames.push(frame.clone());
            }
            (root.clone(), claim.as_ref().clone(), frames)
        };
        let execution = self
            .host
            .store
            .get_execution(&original.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if execution.state != ExecutionState::Running
            || execution.run_unit_id != original.run_unit_id
            || execution.principal != *original.principal.id()
            || execution.attempt != original.attempt
            || execution.artifact != original.artifact
            || execution.selector != original.selector
        {
            return Err(HostProblem::UnknownOutcome);
        }
        root.revoke_native_authority()?;
        for frame in frames {
            frame.revoke_for_custody();
        }
        // Backend rechecks the whole original graph and every scope/actor index
        // under its physical lock/TX. Failure keeps transport fenced and yields
        // no custody object/new authority; there is no automatic second attempt.
        let capture = self
            .host
            .store
            .capture_root_custody_cutoff(&claim, &execution, tick)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        Ok(NativeMqRootCustody {
            host: self.host.clone(),
            original: original.clone(),
            root,
            capture,
        })
    }
}

/// Non-reentrant view of the exact retained configured clock. No selected tick,
/// global override or callback recursion can be supplied by the application.
struct CustodyClock(Arc<dyn MqReplayClock>);
impl MqReplayClock for CustodyClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.custody_tick()
    }
    fn custody_tick(&self) -> Result<u64, HostProblem> {
        self.0.custody_tick()
    }
}
impl NativeMqRootManager {
    /// A genuinely cold selected service on the SAME physically retained store.
    /// A fresh SQLite open by filename does not satisfy this custody contract.
    /// Storage/unknown rows remain unchanged until the actual compiled successor
    /// coordinator makes its one versioned admission transaction.
    pub fn prepare_successor_host(
        &self,
        custody: NativeMqRootCustody,
    ) -> Result<Arc<ConfiguredInstalledMqHost>, HostProblem> {
        if !custody.same_host(&self.host) {
            return Err(HostProblem::Unauthorized);
        }
        self.prepare_at_store(custody, self.host.store.clone())
    }
    /// Close/reopen the held SQLite file only through the actual retained
    /// manager/store. A second adapter with identical rows or a matching path
    /// cannot stand in for this exact originating SQLx owner.
    pub fn prepare_owned_sqlite_successor(
        &self,
        custody: NativeMqRootCustody,
        source: Arc<mainframe_env_store::SqliteStateStore>,
    ) -> Result<Arc<ConfiguredInstalledMqHost>, HostProblem> {
        if !custody.same_host(&self.host) {
            return Err(HostProblem::Unauthorized);
        }
        let actual: Arc<dyn PlatformStore> = source.clone();
        if !Arc::ptr_eq(&actual, &self.host.store) {
            return Err(HostProblem::Unauthorized);
        }
        let reopened = source
            .reopen_owned_root(&custody.capture)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        self.prepare_at_store(custody, reopened)
    }
    fn prepare_at_store(
        &self,
        custody: NativeMqRootCustody,
        store: Arc<dyn PlatformStore>,
    ) -> Result<Arc<ConfiguredInstalledMqHost>, HostProblem> {
        let clock: Arc<dyn MqReplayClock> = Arc::new(CustodyClock(self.host.clock.clone()));
        let host = ConfiguredInstalledMqHost::open_configured(
            store,
            self.host.authorizer.clone(),
            clock,
            self.host.descriptor.clone(),
            self.host.mq_limits,
            self.host.host_limits,
            self.host.mqi_limits,
            self.host.generation,
            self.host.fence,
            self.host.bounds,
            true,
        )?;
        *host
            .successor
            .lock()
            .map_err(|_| HostProblem::UnknownOutcome)? = Some(custody.capture);
        Ok(host)
    }
}
