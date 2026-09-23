use mainframe_env_cics::{
    CICS_DELAY_WORK_GENERATION, CICS_POST_WORK_GENERATION, CICS_START_WORK_GENERATION, CicsService,
    CicsStartTask,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{PlatformStore, StoreError, WorkRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) const JES_WORK_GENERATION: &str = "mainframe-env-batch@1";
pub(crate) const JES_WORK_PAYLOAD_SCHEMA: &str = "mainframe-env.jes-work@1";
pub(crate) const JES_WORKER_COUNT: usize = 2;
pub(crate) const JES_LEASE_TICKS: u64 = 30_000;
pub(crate) const JES_HEARTBEAT_MILLIS: u64 = 5_000;
pub(crate) const JES_IDLE_MILLIS: u64 = 1_000;
pub(crate) const JES_WORKER_FRESHNESS_MILLIS: u64 = JES_HEARTBEAT_MILLIS * 3;
pub(crate) const JES_WORK_DEADLINE_TICKS: u64 = 24 * 60 * 60 * 1_000;
const MAX_WORK_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_CAPABILITIES: usize = 128;

pub(crate) fn claim_durable_work(
    store: &dyn PlatformStore,
    worker: &str,
    now_tick: u64,
) -> Result<Option<WorkRecord>, StoreError> {
    let jes = store.claim(worker, Some(JES_WORK_GENERATION), now_tick, JES_LEASE_TICKS)?;
    if jes.is_some() {
        return Ok(jes);
    }
    let start = store.claim(
        worker,
        Some(CICS_START_WORK_GENERATION),
        now_tick,
        JES_LEASE_TICKS,
    )?;
    if start.is_some() {
        return Ok(start);
    }
    let delay = store.claim(
        worker,
        Some(CICS_DELAY_WORK_GENERATION),
        now_tick,
        JES_LEASE_TICKS,
    )?;
    if delay.is_some() {
        return Ok(delay);
    }
    store.claim(
        worker,
        Some(CICS_POST_WORK_GENERATION),
        now_tick,
        JES_LEASE_TICKS,
    )
}

pub(crate) fn heartbeat_durable_work(
    store: &dyn PlatformStore,
    work: &WorkRecord,
    now_tick: u64,
) -> Result<(), StoreError> {
    store
        .heartbeat(
            &work.work_id,
            work.lease_id
                .as_deref()
                .ok_or(StoreError::InvalidTransition)?,
            work.lease_epoch,
            now_tick,
            JES_LEASE_TICKS,
        )
        .map(|_| ())
}

pub(crate) enum CicsWorkOutcome {
    Start(CicsStartTask),
    Delay,
    Post(bool),
}

pub(crate) fn process_cics_work(
    cics: &CicsService,
    work: &WorkRecord,
    now_tick: u64,
) -> Result<Option<CicsWorkOutcome>, HostProblem> {
    Ok(match work.required_generation.as_str() {
        CICS_START_WORK_GENERATION => Some(CicsWorkOutcome::Start(
            cics.promote_start_work(work, now_tick)?,
        )),
        CICS_DELAY_WORK_GENERATION => {
            cics.promote_delay_work(work, now_tick)?;
            Some(CicsWorkOutcome::Delay)
        }
        CICS_POST_WORK_GENERATION => Some(CicsWorkOutcome::Post(
            cics.promote_post_work(work, now_tick)?,
        )),
        _ => None,
    })
}

pub(crate) fn clear_worker_progress(progress: &Mutex<Vec<Option<Instant>>>, ordinal: usize) {
    if let Ok(mut progress) = progress.lock()
        && let Some(slot) = progress.get_mut(ordinal)
    {
        *slot = None;
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JesWorkPayload {
    pub(crate) schema_version: String,
    pub(crate) job_id: String,
    pub(crate) owner: String,
    pub(crate) capabilities: BTreeSet<String>,
}

impl JesWorkPayload {
    pub(crate) fn new(
        job_id: impl Into<String>,
        owner: impl Into<String>,
        capabilities: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, StoreError> {
        let payload = Self {
            schema_version: JES_WORK_PAYLOAD_SCHEMA.into(),
            job_id: job_id.into(),
            owner: owner.into(),
            capabilities: capabilities.into_iter().map(Into::into).collect(),
        };
        payload.validate()?;
        Ok(payload)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, StoreError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| StoreError::IncompatibleVersion)?;
        if bytes.len() > MAX_WORK_PAYLOAD_BYTES {
            return Err(StoreError::PayloadTooLarge);
        }
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.is_empty() || bytes.len() > MAX_WORK_PAYLOAD_BYTES {
            return Err(StoreError::PayloadTooLarge);
        }
        let payload: Self =
            serde_json::from_slice(bytes).map_err(|_| StoreError::IncompatibleVersion)?;
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.schema_version != JES_WORK_PAYLOAD_SCHEMA
            || self.job_id.is_empty()
            || self.job_id.len() > 11
            || !self.job_id.strip_prefix("JOB").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            })
            || self.owner.is_empty()
            || self.owner.len() > 8
            || !self.owner.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
            })
            || self.capabilities.is_empty()
            || self.capabilities.len() > MAX_CAPABILITIES
            || self.capabilities.iter().any(|capability| {
                capability.is_empty()
                    || capability.len() > 128
                    || capability.chars().any(char::is_control)
            })
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
    }
}

pub(crate) trait JesClock: Send + Sync {
    fn now_tick(&self) -> Result<u64, StoreError>;
}

pub(crate) struct DurableJesClock {
    store: Arc<dyn PlatformStore>,
    last: AtomicU64,
    anchor: u64,
    started: Instant,
}

impl DurableJesClock {
    pub(crate) fn new(store: Arc<dyn PlatformStore>) -> Result<Self, StoreError> {
        let wall = wall_tick()?;
        let persisted = store.advance_logical_clock(wall)?;
        Ok(Self {
            store,
            last: AtomicU64::new(persisted),
            anchor: persisted.max(wall),
            started: Instant::now(),
        })
    }

    fn tick_at(&self, wall_tick: u64) -> Result<u64, StoreError> {
        if wall_tick == 0 {
            return Err(StoreError::InvalidTransition);
        }
        let observed = wall_tick.max(self.last.load(Ordering::SeqCst));
        let next = self.store.advance_logical_clock(observed)?;
        self.last.fetch_max(next, Ordering::SeqCst);
        Ok(next)
    }
}

impl JesClock for DurableJesClock {
    fn now_tick(&self) -> Result<u64, StoreError> {
        let elapsed = u64::try_from(self.started.elapsed().as_millis())
            .map_err(|_| StoreError::CapacityExceeded)?;
        let monotonic = self
            .anchor
            .checked_add(elapsed)
            .ok_or(StoreError::CapacityExceeded)?;
        self.tick_at(wall_tick()?.max(monotonic))
    }
}

fn wall_tick() -> Result<u64, StoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::InvalidTransition)?
        .as_millis();
    u64::try_from(millis).map_err(|_| StoreError::CapacityExceeded)
}

#[cfg(test)]
pub(crate) struct ManualJesClock {
    tick: AtomicU64,
}

#[cfg(test)]
impl ManualJesClock {
    pub(crate) fn new(tick: u64) -> Self {
        Self {
            tick: AtomicU64::new(tick),
        }
    }

    pub(crate) fn advance(&self, ticks: u64) {
        self.tick.fetch_add(ticks, Ordering::SeqCst);
    }
}

#[cfg(test)]
impl JesClock for ManualJesClock {
    fn now_tick(&self) -> Result<u64, StoreError> {
        let tick = self.tick.load(Ordering::SeqCst);
        (tick != 0)
            .then_some(tick)
            .ok_or(StoreError::InvalidTransition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, StoreLimits};

    #[test]
    fn durable_clock_survives_wall_regression_and_process_reopen() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let first = DurableJesClock::new(store.clone()).unwrap();
        let baseline = first.tick_at(100).unwrap();
        assert_eq!(first.tick_at(50).unwrap(), baseline);
        let reopened = DurableJesClock::new(store).unwrap();
        assert!(reopened.tick_at(10).unwrap() >= baseline);
    }

    #[test]
    fn work_payload_is_bounded_typed_and_owner_scoped() {
        let payload = JesWorkPayload::new(
            "JOB00001",
            "ALICE",
            ["host.security.authorize", "host.spool.write"],
        )
        .unwrap();
        assert_eq!(
            JesWorkPayload::decode(&payload.encode().unwrap()).unwrap(),
            payload
        );
        let mut hostile = payload.encode().unwrap();
        hostile.extend(std::iter::repeat_n(b' ', MAX_WORK_PAYLOAD_BYTES));
        assert_eq!(
            JesWorkPayload::decode(&hostile),
            Err(StoreError::PayloadTooLarge)
        );
    }
}
