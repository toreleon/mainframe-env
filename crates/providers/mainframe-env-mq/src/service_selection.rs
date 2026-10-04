//! One selected service state, not a second queue service or a public dispatcher.
//!
//! The strict opener never initializes, imports, advances fences or backs out
//! work. Its mandatory platform/authorization/clock inputs are composition
//! prerequisites, not a host-attestation or participant-readiness claim.

use super::*;
use mainframe_env_store_api::PlatformStore;
use std::ops::{Deref, DerefMut};

#[path = "service_selected_operation.rs"]
pub(super) mod operations;

/// Checked access keeps all historical service paths on the legacy variant.
/// The guard holds the sole authority lock; the variant cannot change while
/// a borrowed legacy state is exposed. Selected instances never obtain it.
pub(super) struct LegacyAccess<'a>(MutexGuard<'a, rich_state::StoredAuthority>);

impl<'a> LegacyAccess<'a> {
    pub(super) fn new(
        guard: MutexGuard<'a, rich_state::StoredAuthority>,
    ) -> Result<Self, HostProblem> {
        match &*guard {
            rich_state::StoredAuthority::Legacy(_) => Ok(Self(guard)),
            rich_state::StoredAuthority::Rich(_) => Err(HostProblem::Unsupported),
        }
    }
}

impl Deref for LegacyAccess<'_> {
    type Target = DurableState;
    fn deref(&self) -> &Self::Target {
        match &*self.0 {
            rich_state::StoredAuthority::Legacy(state) => state,
            rich_state::StoredAuthority::Rich(_) => {
                unreachable!("checked legacy authority cannot change under its guard")
            }
        }
    }
}
impl DerefMut for LegacyAccess<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut *self.0 {
            rich_state::StoredAuthority::Legacy(state) => state,
            rich_state::StoredAuthority::Rich(_) => {
                unreachable!("checked legacy authority cannot change under its guard")
            }
        }
    }
}

impl MqService {
    /// Private strict selection for actual MQI composition. The caller's
    /// generation/fence must come from deployment/recovery authority, not an
    /// application envelope. There is deliberately no optional policy path.
    /// `limits` bounds retained legacy state/replay; rich delivery uses the
    /// strict reader's existing default delivery/message/row profile.
    pub(crate) fn open_selected_mqi(
        store: Arc<dyn PlatformStore>,
        limits: MqLimits,
        generation: u64,
        fence: u64,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        replay_clock: Arc<dyn MqReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        let authority = rich_state::read(
            &*store,
            generation,
            fence,
            rich_state::ReaderLimits {
                legacy: limits,
                ..Default::default()
            },
        )
        .map_err(selection_error)?;
        // Trait upcasting preserves the same allocation/backend authority.
        // No separately supplied provider store can be paired with this core.
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        Ok(Arc::new(Self {
            store: provider_store,
            limits,
            durable: Mutex::new(authority),
            selected_store: Some(store),
            rfh2_source: None,
            unknown_after_persist: AtomicBool::new(false),
            authorizer: Some(authorizer),
            replay_clock: Some(replay_clock),
            producer_sources: None,
            producer_sampling: AtomicBool::new(false),
        }))
    }

    /// Later selected operations use this same union/lock. Historical service
    /// entry points cannot acquire it, and no rich state is projected to v1.
    pub(super) fn lock_selected(
        &self,
    ) -> Result<MutexGuard<'_, rich_state::StoredAuthority>, HostProblem> {
        if crate::trusted_batch_embedding::rfh2_source_capturing() {
            return Err(HostProblem::Unsupported);
        }
        if self.selected_store.is_none() {
            return Err(HostProblem::Unsupported);
        }
        if self.producer_sampling.load(Ordering::SeqCst) {
            return Err(HostProblem::Unsupported);
        }
        self.durable
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}

pub(super) fn selection_error(error: rich_state::ReadError) -> HostProblem {
    match error {
        rich_state::ReadError::Bounds => HostProblem::ResourceExhausted,
        rich_state::ReadError::Identity => HostProblem::IdempotencyConflict,
        rich_state::ReadError::Missing => HostProblem::NotFound,
        rich_state::ReadError::Corrupt => HostProblem::Malformed,
        rich_state::ReadError::Legacy(error) => error,
        rich_state::ReadError::Rows(_) => HostProblem::Malformed,
        rich_state::ReadError::Store(error) => store_error(error),
    }
}

#[cfg(test)]
#[path = "service_selection/tests.rs"]
mod tests;
