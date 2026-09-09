use mainframe_env_store_api::{
    EffectRecord, EffectState, MAX_EFFECT_RECOVERY_OWNER_BYTES, PlatformStore, StoreError,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectRecoveryLimits {
    pub minimum_age_ticks: u64,
    pub lease_ticks: u64,
    pub max_intents: usize,
}

impl Default for EffectRecoveryLimits {
    fn default() -> Self {
        Self {
            minimum_age_ticks: 60,
            lease_ticks: 30,
            max_intents: 128,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectRecoveryResolution {
    /// The provider's durable idempotency ledger proves the mutation committed.
    Completed([u8; 32]),
    /// The provider's durable idempotency ledger proves the mutation did not commit.
    Failed([u8; 32]),
    /// No safe determination is currently available. Never redispatch automatically.
    Pending,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EffectRecoveryReport {
    pub scanned: usize,
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending: usize,
    pub contended: usize,
}

/// Bounded recovery worker for intents abandoned after host dispatch.
///
/// The resolver must consult an external durable idempotency ledger or an
/// equivalent authoritative observation. The worker never invokes the original
/// effect and therefore cannot turn a stale intent into a duplicate mutation.
pub struct StaleEffectRecoveryWorker {
    store: Arc<dyn PlatformStore>,
    owner: String,
    limits: EffectRecoveryLimits,
}

impl StaleEffectRecoveryWorker {
    pub fn new(
        store: Arc<dyn PlatformStore>,
        owner: impl Into<String>,
        limits: EffectRecoveryLimits,
    ) -> Result<Self, StoreError> {
        let owner = owner.into();
        if owner.is_empty()
            || owner.len() > MAX_EFFECT_RECOVERY_OWNER_BYTES
            || limits.minimum_age_ticks == 0
            || limits.lease_ticks == 0
            || limits.max_intents == 0
            || limits.max_intents > 65_536
        {
            return Err(StoreError::InvalidTransition);
        }
        Ok(Self {
            store,
            owner,
            limits,
        })
    }

    pub fn run_once<F>(
        &self,
        now_tick: u64,
        mut resolve: F,
    ) -> Result<EffectRecoveryReport, StoreError>
    where
        F: FnMut(&EffectRecord) -> Result<EffectRecoveryResolution, StoreError>,
    {
        let intents = self.store.stale_intents(
            now_tick,
            self.limits.minimum_age_ticks,
            self.limits.max_intents,
        )?;
        let mut report = EffectRecoveryReport {
            scanned: intents.len(),
            ..EffectRecoveryReport::default()
        };
        for intent in intents {
            let claimed = match self.store.claim_stale_intent(
                &intent.key,
                intent.intent.epoch,
                &self.owner,
                now_tick,
                self.limits.minimum_age_ticks,
                self.limits.lease_ticks,
            ) {
                Ok(claimed) => claimed,
                Err(error) if contention(&error) => {
                    report.contended += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            report.claimed += 1;
            let lease = claimed
                .intent
                .recovery_lease
                .as_ref()
                .ok_or(StoreError::LeaseConflict)?;
            let resolution = resolve(&claimed)?;
            let (state, digest) = match resolution {
                EffectRecoveryResolution::Completed(digest) => (EffectState::Completed, digest),
                EffectRecoveryResolution::Failed(digest) => (EffectState::Failed, digest),
                EffectRecoveryResolution::Pending => {
                    report.pending += 1;
                    continue;
                }
            };
            match self.store.reconcile_stale_intent(
                &claimed.key,
                &self.owner,
                lease.epoch,
                now_tick,
                state,
                claimed.digest_format,
                digest,
            ) {
                Ok(_) if state == EffectState::Completed => report.completed += 1,
                Ok(_) => report.failed += 1,
                Err(error) if contention(&error) => report.contended += 1,
                Err(error) => return Err(error),
            }
        }
        Ok(report)
    }
}

fn contention(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::NotFound
            | StoreError::Conflict
            | StoreError::InvalidTransition
            | StoreError::LeaseConflict
    )
}
