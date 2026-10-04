//! Native CALL registration from the exact original retained root/frame authority.
use super::*;
use crate::cobol::CobolProgram;
use mainframe_env_execution_api::InvocationLimits;
use mainframe_env_host_api::canonical_request_digest;
use mainframe_env_store_api::{
    ProviderStateIdentity, ProviderStateRecord, RootProviderRowAdmission,
};

pub(in crate::cobol) struct NativeCallOwnership {
    occurrence: RootProviderRowAdmission,
    catalog: ProviderStateRecord,
}
impl NativeCallOwnership {
    pub(in crate::cobol) fn enrollment(
        &self,
        reservation: &ProviderStateRecord,
    ) -> mainframe_env_interpreter::NativeChildEnrollment {
        mainframe_env_interpreter::NativeChildEnrollment {
            claim: self.occurrence.claim.clone(),
            parent_occurrence: self.occurrence.clone(),
            call: reservation.clone(),
            catalog: self.catalog.clone(),
        }
    }
}
impl CobolProgram {
    pub(in crate::cobol) fn prepare_native_call(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
        key: &str,
        admitted: &crate::cobol::artifact::AdmittedProgram,
    ) -> Result<Option<NativeCallOwnership>, HostProblem> {
        let Some(configured) = self.native_mq_host.get().and_then(Weak::upgrade) else {
            return Ok(None);
        };
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let control = self
            .control
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let host = self.host.get().ok_or(HostProblem::InfrastructureFailure)?;
        if !Arc::ptr_eq(store, &configured.store) || !Arc::ptr_eq(control, &configured.control) {
            return Err(HostProblem::Unauthorized);
        }
        let expected: Arc<dyn HostProvider> = configured.clone();
        if !host.selects_same_provider(&configured.descriptor.capability, &expected)? {
            return Err(HostProblem::Unauthorized);
        }
        let (claim, frame) = {
            let map = configured
                .topology
                .lock()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            // Root lineage comes from retained opaque references, not binding IDs.
            let root_id = if parent.parent_execution_id.is_none() {
                parent.execution_id.clone()
            } else {
                match map.frames.get(&parent.execution_id) {
                    Some(FrameEntry::Retained(frame)) => frame.native_root_execution(),
                    _ => return Err(HostProblem::Unauthorized),
                }
            };
            let claim = match map.roots.get(&root_id) {
                Some(RootEntry::Retained {
                    native: Some(claim),
                    ..
                }) => claim.clone(),
                _ => return Ok(None), // exact legacy/private route remains unchanged
            };
            let frame = if parent.parent_execution_id.is_none() {
                match map.roots.get(&root_id) {
                    Some(RootEntry::Retained { frame, .. }) => frame.clone(),
                    _ => return Err(HostProblem::Unauthorized),
                }
            } else {
                match map.frames.get(&parent.execution_id) {
                    Some(FrameEntry::Retained(frame)) => frame.clone(),
                    _ => return Err(HostProblem::Unauthorized),
                }
            };
            (claim, frame)
        };
        frame.with_active(parent, |_| {
            let crate::cobol::artifact::AdmittedProgramProvenance::Catalog(catalog) =
                &admitted.provenance
            else {
                return Err(HostProblem::Unsupported);
            };
            if catalog.namespace != "batch-program"
                || catalog.payload.len() > InvocationLimits::default().max_identity_bytes
            {
                return Err(HostProblem::Unsupported);
            }
            let now = control
                .observe(parent)
                .map_err(|_| HostProblem::UnknownOutcome)?;
            super::super::check_controls(parent, now, 0)?;
            let (_, execution) =
                super::super::proof::observe_parent(self, parent, effect, now.now_tick)?;
            let occurrence = RootProviderRowAdmission {
                claim,
                execution,
                effect_key: effect
                    .idempotency_key
                    .clone()
                    .ok_or(HostProblem::Malformed)?,
                effect_sequence: effect.sequence,
                request_digest: canonical_request_digest(&effect.request)?,
                identity: ProviderStateIdentity {
                    namespace: crate::cobol::retention::CALL_REPLAY_NAMESPACE.into(),
                    key: key.into(),
                },
                observed_tick: now.now_tick,
            };
            store
                .register_root_provider_row(occurrence.clone())
                .map_err(|_| HostProblem::UnknownOutcome)?;
            for identity in [
                ProviderStateIdentity {
                    namespace: catalog.namespace.clone(),
                    key: catalog.key.clone(),
                },
                ProviderStateIdentity {
                    namespace: "batch-file-cursor".into(),
                    key: format!("{}:{}", parent.run_unit_id.as_str(), admitted.name),
                },
            ] {
                let mut observation = occurrence.clone();
                observation.identity = identity;
                store
                    .register_root_provider_row(observation)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            }
            Ok(Some(NativeCallOwnership {
                occurrence,
                catalog: catalog.clone(),
            }))
        })
    }
}
