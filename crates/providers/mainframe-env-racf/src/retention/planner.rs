use crate::database::{
    DATABASE_KEY, DATABASE_NAMESPACE, SecurityDatabase, decode_snapshot, encode_snapshot,
    store_problem,
};
use crate::model::{
    RecoveryRecord, RecoveryState, SecurityAuditRecord, SecurityDatabaseLimits,
    SecurityDatabaseSnapshot, SecurityTransaction, TransactionState,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderRetentionDependency, ProviderRetentionRow, ProviderStateArchiveReplacement,
    ProviderStateRecord, ProviderStateWrite, RetentionObservation, RetentionObservationProof,
    RetentionTarget, StoreError,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const RACF_AUDIT_NAMESPACE: &str = "racf-audit";
pub(super) const RACF_TRANSACTION_NAMESPACE: &str = "racf-transaction";
pub(super) const RACF_RECOVERY_NAMESPACE: &str = "racf-recovery";
const LOGICAL_SOURCE_VERSION: u64 = 1;
const MAX_RACF_OBSERVATIONS: usize = 262_144;
const MAX_PLAN_ATTEMPTS: usize = 4;

/// Hard maximum number of RACF records moved by one retention transaction.
pub const MAX_RACF_RETENTION_BATCH: usize = 4_096;

/// Validated age, alert, and batch bounds for provider-local RACF retention.
///
/// Archive and observation capacities belong to the durable store and are reported through
/// [`RacfRetentionForecast`], avoiding a second provider-local limit that can diverge from the
/// transactional authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfRetentionPolicy {
    /// Minimum logical ticks retained after terminal observation.
    pub retain_ticks: u64,
    /// Usage percentage at which warning pressure begins.
    pub low_watermark_percent: u8,
    /// Usage percentage at which urgent pressure begins.
    pub high_watermark_percent: u8,
    /// Maximum logical evidence records moved by one transaction.
    pub max_batch_records: usize,
}

impl Default for RacfRetentionPolicy {
    fn default() -> Self {
        Self {
            retain_ticks: 86_400,
            low_watermark_percent: 70,
            high_watermark_percent: 90,
            max_batch_records: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RacfRetentionPressure {
    /// Every measured authority is below its low watermark.
    Healthy,
    /// At least one authority reached the low watermark.
    LowWatermark,
    /// At least one authority reached the high watermark.
    HighWatermark,
    /// At least one authority has no remaining capacity.
    Full,
}

/// Stable identity for one fully decoded RACF evidence subrecord.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfRetentionDescriptor {
    /// Synthetic archive namespace for the logical subrecord.
    pub namespace: String,
    /// Stable logical subrecord identity.
    pub key: String,
    /// Immutable logical version, independent of aggregate generations.
    pub source_version: u64,
    /// SHA-256 of the exact standalone bytes archived for this row.
    pub source_digest: [u8; 32],
    /// Intrinsic trusted tick, or `None` for protected legacy evidence.
    pub intrinsic_tick: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfRetentionForecast {
    /// Exact RACF aggregate generation inspected.
    pub generation: u64,
    /// Supplied nonzero logical observation tick.
    pub now_tick: u64,
    /// Inclusive age boundary derived from policy.
    pub watermark_tick: u64,
    /// Live RACF audit rows.
    pub audits: usize,
    /// Audit rows eligible for archive.
    pub eligible_audits: usize,
    /// Legacy audit rows awaiting an observation.
    pub unaged_audits: usize,
    /// Remaining provider-local audit slots.
    pub audit_headroom: usize,
    /// Live RACF transaction rows.
    pub transactions: usize,
    /// Transaction rows eligible for archive.
    pub eligible_transactions: usize,
    /// Remaining provider-local transaction slots.
    pub transaction_headroom: usize,
    /// Live RACF recovery rows.
    pub recovery_records: usize,
    /// Recovery rows eligible for archive.
    pub eligible_recovery_records: usize,
    /// Remaining provider-local recovery slots.
    pub recovery_headroom: usize,
    /// Legacy transaction rows awaiting an observation.
    pub unaged_transactions: usize,
    /// Legacy recovery rows awaiting an observation.
    pub unaged_recovery_records: usize,
    /// RACF rows already held by the shared archive authority.
    pub archive_records: usize,
    /// Shared archive row capacity.
    pub archive_capacity: usize,
    /// Shared archive row headroom.
    pub archive_record_headroom: usize,
    /// Accounted bytes used by shared archives.
    pub archive_bytes: u64,
    /// Shared archive byte capacity.
    pub archive_byte_capacity: u64,
    /// Shared archive byte headroom.
    pub archive_byte_headroom: u64,
    /// RACF observations held by the shared sidecar authority.
    pub observation_records: usize,
    /// Shared observation row capacity.
    pub observation_capacity: usize,
    /// Shared observation row headroom.
    pub observation_record_headroom: usize,
    /// Accounted bytes used by shared observations.
    pub observation_bytes: u64,
    /// Shared observation byte capacity.
    pub observation_byte_capacity: u64,
    /// Shared observation byte headroom.
    pub observation_byte_headroom: u64,
    /// Worst provider-local or shared-authority pressure.
    pub pressure: RacfRetentionPressure,
}

/// Result of one RACF archive-before-prune transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfRetentionReceipt {
    /// Aggregate generation committed by the replacement.
    pub generation: u64,
    /// Inclusive policy boundary used for selection.
    pub watermark_tick: u64,
    /// Archived RACF audit records.
    pub archived_audits: usize,
    /// Archived terminal transaction records.
    pub archived_transactions: usize,
    /// Archived terminal recovery records.
    pub archived_recovery_records: usize,
    /// Dedicated archive identity, or `None` for an empty pass.
    pub archive_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EvidenceKind {
    Recovery,
    Transaction,
    Audit,
}

impl EvidenceKind {
    const fn namespace(self) -> &'static str {
        match self {
            Self::Recovery => RACF_RECOVERY_NAMESPACE,
            Self::Transaction => RACF_TRANSACTION_NAMESPACE,
            Self::Audit => RACF_AUDIT_NAMESPACE,
        }
    }
}

#[derive(Clone)]
struct EncodedDescriptor {
    descriptor: RacfRetentionDescriptor,
    kind: EvidenceKind,
    payload: Vec<u8>,
}

#[derive(Clone)]
struct Candidate {
    encoded: EncodedDescriptor,
    retention_tick: u64,
    observation: Option<RetentionObservationProof>,
}

struct Eligibility {
    candidates: Vec<Candidate>,
    unaged_audits: usize,
    unaged_transactions: usize,
    unaged_recovery_records: usize,
}

struct ObservationIndex(BTreeMap<(String, String), (Option<u64>, RetentionObservation)>);

impl ObservationIndex {
    fn new(observations: &[RetentionObservation]) -> Result<Self, HostProblem> {
        if observations.len() > MAX_RACF_OBSERVATIONS {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut rows = BTreeMap::new();
        for observation in observations
            .iter()
            .filter(|row| row.target == RetentionTarget::RacfEvidence)
        {
            if !valid_evidence_namespace(&observation.namespace)
                || observation.key.is_empty()
                || observation.source_version != LOGICAL_SOURCE_VERSION
                || observation.observed_tick == 0
                || observation.owner_execution.is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let identity = (observation.namespace.clone(), observation.key.clone());
            if rows.insert(identity, (None, observation.clone())).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(Self(rows))
    }

    fn with_versions(observations: &[(u64, RetentionObservation)]) -> Result<Self, HostProblem> {
        if observations.len() > MAX_RACF_OBSERVATIONS {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut rows = BTreeMap::new();
        for (version, observation) in observations
            .iter()
            .filter(|(_, row)| row.target == RetentionTarget::RacfEvidence)
        {
            if *version == 0
                || !valid_evidence_namespace(&observation.namespace)
                || observation.key.is_empty()
                || observation.source_version != LOGICAL_SOURCE_VERSION
                || observation.observed_tick == 0
                || observation.owner_execution.is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let identity = (observation.namespace.clone(), observation.key.clone());
            if rows
                .insert(identity, (Some(*version), observation.clone()))
                .is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(Self(rows))
    }

    fn tick(&self, descriptor: &RacfRetentionDescriptor) -> Option<u64> {
        descriptor.intrinsic_tick.or_else(|| {
            self.0
                .get(&(descriptor.namespace.clone(), descriptor.key.clone()))
                .filter(|(_, row)| row.source_digest == descriptor.source_digest)
                .map(|(_, row)| row.observed_tick)
        })
    }

    fn proof(&self, descriptor: &RacfRetentionDescriptor) -> Option<RetentionObservationProof> {
        descriptor.intrinsic_tick.is_none().then(|| {
            self.0
                .get(&(descriptor.namespace.clone(), descriptor.key.clone()))
                .and_then(|(version, observation)| {
                    version.map(|version| RetentionObservationProof {
                        version,
                        observation: observation.clone(),
                    })
                })
        })?
    }
}

impl SecurityDatabase {
    pub fn retention_descriptors(&self) -> Result<Vec<RacfRetentionDescriptor>, HostProblem> {
        for _ in 0..MAX_PLAN_ATTEMPTS {
            let before = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            let source = self.source_record()?;
            let snapshot = decode_snapshot(&source, self.limits)?;
            let descriptors = encoded_descriptors(&snapshot, self.limits)?
                .into_iter()
                .map(|row| row.descriptor)
                .collect();
            let after = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            if before == after {
                return Ok(descriptors);
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    pub fn retention_forecast(
        &self,
        policy: RacfRetentionPolicy,
        supplied_tick: u64,
    ) -> Result<RacfRetentionForecast, HostProblem> {
        self.retention_forecast_with_observations(policy, supplied_tick, &[])
    }

    pub fn retention_forecast_with_observations(
        &self,
        policy: RacfRetentionPolicy,
        supplied_tick: u64,
        observations: &[RetentionObservation],
    ) -> Result<RacfRetentionForecast, HostProblem> {
        validate_retention_policy(policy)?;
        if supplied_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        let observations = ObservationIndex::new(observations)?;
        for _ in 0..MAX_PLAN_ATTEMPTS {
            let before = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            let source = self.source_record()?;
            let snapshot = decode_snapshot(&source, self.limits)?;
            let now_tick = supplied_tick.max(snapshot.retention_tick);
            let watermark_tick = now_tick.saturating_sub(policy.retain_ticks);
            let eligibility = eligibility(
                &snapshot,
                &observations,
                now_tick,
                watermark_tick,
                self.limits,
            )?;
            let authority = self
                .store
                .provider_retention_authority_usage(RetentionTarget::RacfEvidence)
                .map_err(store_problem)?;
            let after = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            if before != after {
                continue;
            }
            let pressure = [
                retention_pressure(snapshot.audits.len(), self.limits.max_audits, policy),
                retention_pressure(
                    snapshot.transactions.len(),
                    self.limits.max_transactions,
                    policy,
                ),
                retention_pressure(
                    snapshot.recovery.len(),
                    self.limits.max_recovery_records,
                    policy,
                ),
                retention_pressure(
                    authority.shared_archive_rows,
                    authority.archive_row_capacity,
                    policy,
                ),
                retention_pressure_u64(
                    authority.shared_archive_bytes,
                    authority.archive_byte_capacity,
                    policy,
                ),
                retention_pressure(
                    authority.shared_observation_rows,
                    authority.observation_row_capacity,
                    policy,
                ),
                retention_pressure_u64(
                    authority.shared_observation_bytes,
                    authority.observation_byte_capacity,
                    policy,
                ),
            ]
            .into_iter()
            .max()
            .unwrap_or(RacfRetentionPressure::Healthy);
            return Ok(RacfRetentionForecast {
                generation: snapshot.generation,
                now_tick,
                watermark_tick,
                audits: snapshot.audits.len(),
                eligible_audits: count_candidates(&eligibility, EvidenceKind::Audit),
                unaged_audits: eligibility.unaged_audits,
                audit_headroom: self.limits.max_audits.saturating_sub(snapshot.audits.len()),
                transactions: snapshot.transactions.len(),
                eligible_transactions: count_candidates(&eligibility, EvidenceKind::Transaction),
                transaction_headroom: self
                    .limits
                    .max_transactions
                    .saturating_sub(snapshot.transactions.len()),
                recovery_records: snapshot.recovery.len(),
                eligible_recovery_records: count_candidates(&eligibility, EvidenceKind::Recovery),
                recovery_headroom: self
                    .limits
                    .max_recovery_records
                    .saturating_sub(snapshot.recovery.len()),
                unaged_transactions: eligibility.unaged_transactions,
                unaged_recovery_records: eligibility.unaged_recovery_records,
                archive_records: authority.shared_archive_rows,
                archive_capacity: authority.archive_row_capacity,
                archive_record_headroom: authority
                    .archive_row_capacity
                    .saturating_sub(authority.shared_archive_rows),
                archive_bytes: authority.shared_archive_bytes,
                archive_byte_capacity: authority.archive_byte_capacity,
                archive_byte_headroom: authority
                    .archive_byte_capacity
                    .saturating_sub(authority.shared_archive_bytes),
                observation_records: authority.shared_observation_rows,
                observation_capacity: authority.observation_row_capacity,
                observation_record_headroom: authority
                    .observation_row_capacity
                    .saturating_sub(authority.shared_observation_rows),
                observation_bytes: authority.shared_observation_bytes,
                observation_byte_capacity: authority.observation_byte_capacity,
                observation_byte_headroom: authority
                    .observation_byte_capacity
                    .saturating_sub(authority.shared_observation_bytes),
                pressure,
            });
        }
        Err(HostProblem::IdempotencyConflict)
    }

    fn source_record(&self) -> Result<ProviderStateRecord, HostProblem> {
        self.store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)
    }
}

impl SecurityDatabase {
    pub fn archive_and_prune(
        &self,
        policy: RacfRetentionPolicy,
        supplied_tick: u64,
    ) -> Result<RacfRetentionReceipt, HostProblem> {
        self.archive_and_prune_indexed(policy, supplied_tick, ObservationIndex::new(&[])?, None)
    }

    /// Archive using exact versioned observation rows captured under `expected_epoch`.
    pub fn archive_and_prune_with_observations(
        &self,
        policy: RacfRetentionPolicy,
        supplied_tick: u64,
        observations: &[(u64, RetentionObservation)],
        expected_epoch: u64,
    ) -> Result<RacfRetentionReceipt, HostProblem> {
        self.archive_and_prune_indexed(
            policy,
            supplied_tick,
            ObservationIndex::with_versions(observations)?,
            Some(expected_epoch),
        )
    }

    fn archive_and_prune_indexed(
        &self,
        policy: RacfRetentionPolicy,
        supplied_tick: u64,
        observations: ObservationIndex,
        expected_epoch: Option<u64>,
    ) -> Result<RacfRetentionReceipt, HostProblem> {
        validate_retention_policy(policy)?;
        if supplied_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        let durable_tick = self
            .store
            .advance_logical_clock(supplied_tick)
            .map_err(store_problem)?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut batch_limit = policy.max_batch_records;
        'conflict: for attempt in 0..MAX_PLAN_ATTEMPTS {
            let current_epoch = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            let before = expected_epoch.unwrap_or(current_epoch);
            if current_epoch != before {
                return Err(HostProblem::IdempotencyConflict);
            }
            let source = self.source_record()?;
            let snapshot = decode_snapshot(&source, self.limits)?;
            let now_tick = self
                .store
                .advance_logical_clock(durable_tick.max(snapshot.retention_tick))
                .map_err(store_problem)?;
            let watermark_tick = now_tick.saturating_sub(policy.retain_ticks);
            loop {
                let eligibility = eligibility(
                    &snapshot,
                    &observations,
                    now_tick,
                    watermark_tick,
                    self.limits,
                )?;
                let selected = select_candidates(&eligibility, policy, batch_limit);
                if selected.is_empty() {
                    let after = self
                        .store
                        .provider_state_retention_epoch()
                        .map_err(store_problem)?;
                    if before != after {
                        continue 'conflict;
                    }
                    return Ok(empty_receipt(snapshot.generation, watermark_tick));
                }
                let request = build_replacement_request(
                    before,
                    source.clone(),
                    &snapshot,
                    &selected,
                    now_tick,
                    watermark_tick,
                    &observations,
                    self.limits,
                )?;
                let after = self
                    .store
                    .provider_state_retention_epoch()
                    .map_err(store_problem)?;
                if before != after {
                    if expected_epoch.is_some() {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    continue 'conflict;
                }
                match self
                    .store
                    .archive_provider_state_replacement(request.clone())
                {
                    Ok(archive) => {
                        return Ok(receipt(
                            request.replacement.record.version,
                            watermark_tick,
                            &request.rows,
                            Some(archive.archive_id),
                        ));
                    }
                    Err(StoreError::Conflict | StoreError::AlreadyExists)
                        if attempt + 1 < MAX_PLAN_ATTEMPTS =>
                    {
                        if expected_epoch.is_some() {
                            return Err(HostProblem::IdempotencyConflict);
                        }
                        continue 'conflict;
                    }
                    Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge)
                        if request.rows.len() > 1 =>
                    {
                        batch_limit = (request.rows.len() / 2).max(1);
                    }
                    Err(problem) => return Err(store_problem(problem)),
                }
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    pub(crate) fn transaction_for_replay(
        &self,
        snapshot: &SecurityDatabaseSnapshot,
        id: &str,
    ) -> Result<Option<SecurityTransaction>, HostProblem> {
        Ok(snapshot.transactions.get(id).cloned())
    }
}

fn validate_retention_policy(policy: RacfRetentionPolicy) -> Result<(), HostProblem> {
    if policy.retain_ticks == 0
        || policy.low_watermark_percent == 0
        || policy.low_watermark_percent >= policy.high_watermark_percent
        || policy.high_watermark_percent > 100
        || policy.max_batch_records == 0
        || policy.max_batch_records > MAX_RACF_RETENTION_BATCH
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn encoded_descriptors(
    snapshot: &SecurityDatabaseSnapshot,
    limits: SecurityDatabaseLimits,
) -> Result<Vec<EncodedDescriptor>, HostProblem> {
    let mut rows = Vec::with_capacity(
        snapshot
            .audits
            .len()
            .saturating_add(snapshot.transactions.len())
            .saturating_add(snapshot.recovery.len()),
    );
    for recovery in snapshot
        .recovery
        .values()
        .filter(|row| terminal_recovery(row))
    {
        rows.push(encoded_descriptor(
            EvidenceKind::Recovery,
            &recovery.id,
            recovery.terminal_tick,
            recovery,
            limits,
        )?);
    }
    for transaction in snapshot
        .transactions
        .values()
        .filter(|row| terminal_transaction(row))
    {
        rows.push(encoded_descriptor(
            EvidenceKind::Transaction,
            &transaction.id,
            transaction.terminal_tick,
            transaction,
            limits,
        )?);
    }
    for audit in &snapshot.audits {
        rows.push(encoded_descriptor(
            EvidenceKind::Audit,
            &audit.id,
            audit_retention_tick(audit),
            audit,
            limits,
        )?);
    }
    rows.sort_by(|left, right| descriptor_key(left).cmp(&descriptor_key(right)));
    Ok(rows)
}

fn encoded_descriptor<T: Serialize>(
    kind: EvidenceKind,
    key: &str,
    intrinsic_tick: Option<u64>,
    value: &T,
    limits: SecurityDatabaseLimits,
) -> Result<EncodedDescriptor, HostProblem> {
    let payload = serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)?;
    if key.is_empty() || payload.len() > limits.max_database_bytes || intrinsic_tick == Some(0) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(EncodedDescriptor {
        descriptor: RacfRetentionDescriptor {
            namespace: kind.namespace().into(),
            key: key.into(),
            source_version: LOGICAL_SOURCE_VERSION,
            source_digest: Sha256::digest(&payload).into(),
            intrinsic_tick,
        },
        kind,
        payload,
    })
}

fn descriptor_key(row: &EncodedDescriptor) -> (EvidenceKind, &str) {
    (row.kind, row.descriptor.key.as_str())
}

fn eligibility(
    snapshot: &SecurityDatabaseSnapshot,
    observations: &ObservationIndex,
    now_tick: u64,
    watermark_tick: u64,
    limits: SecurityDatabaseLimits,
) -> Result<Eligibility, HostProblem> {
    let descriptors = encoded_descriptors(snapshot, limits)?;
    let mut resolved = BTreeMap::new();
    let mut unaged_audits = 0;
    let mut unaged_transactions = 0;
    let mut unaged_recovery_records = 0;
    for row in &descriptors {
        let tick = observations.tick(&row.descriptor);
        if tick.is_some_and(|tick| tick == 0 || tick > now_tick) {
            return Err(HostProblem::InfrastructureFailure);
        }
        if tick.is_none() {
            match row.kind {
                EvidenceKind::Audit => unaged_audits += 1,
                EvidenceKind::Transaction => unaged_transactions += 1,
                EvidenceKind::Recovery => unaged_recovery_records += 1,
            }
        }
        resolved.insert(
            (row.descriptor.namespace.clone(), row.descriptor.key.clone()),
            tick,
        );
    }
    let mut candidates = Vec::new();
    for row in &descriptors {
        let tick = resolved
            .get(&(row.descriptor.namespace.clone(), row.descriptor.key.clone()))
            .copied()
            .flatten();
        let Some(tick) = tick.filter(|tick| old_retention_tick(*tick, now_tick, watermark_tick))
        else {
            continue;
        };
        let dependencies_terminal = match row.kind {
            EvidenceKind::Transaction => snapshot
                .recovery
                .values()
                .filter(|recovery| recovery.transaction_id == row.descriptor.key)
                .all(|recovery| {
                    terminal_recovery(recovery)
                        && resolved
                            .get(&(RACF_RECOVERY_NAMESPACE.into(), recovery.id.clone()))
                            .copied()
                            .flatten()
                            .is_some_and(|tick| old_retention_tick(tick, now_tick, watermark_tick))
                }),
            EvidenceKind::Recovery | EvidenceKind::Audit => true,
        };
        if dependencies_terminal {
            candidates.push(Candidate {
                encoded: row.clone(),
                retention_tick: tick,
                observation: observations.proof(&row.descriptor),
            });
        }
    }
    candidates.sort_by(|left, right| {
        (
            left.encoded.kind,
            left.retention_tick,
            left.encoded.descriptor.key.as_str(),
        )
            .cmp(&(
                right.encoded.kind,
                right.retention_tick,
                right.encoded.descriptor.key.as_str(),
            ))
    });
    Ok(Eligibility {
        candidates,
        unaged_audits,
        unaged_transactions,
        unaged_recovery_records,
    })
}

fn select_candidates(
    eligibility: &Eligibility,
    policy: RacfRetentionPolicy,
    batch_limit: usize,
) -> Vec<Candidate> {
    eligibility
        .candidates
        .iter()
        .take(batch_limit.min(policy.max_batch_records))
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_replacement_request(
    expected_epoch: u64,
    source: ProviderStateRecord,
    snapshot: &SecurityDatabaseSnapshot,
    selected: &[Candidate],
    archived_tick: u64,
    watermark_tick: u64,
    observations: &ObservationIndex,
    limits: SecurityDatabaseLimits,
) -> Result<ProviderStateArchiveReplacement, HostProblem> {
    let mut replacement = snapshot.clone();
    for candidate in selected {
        remove_evidence(&mut replacement, &candidate.encoded)?;
    }
    replacement.generation = replacement
        .generation
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let request = ProviderStateArchiveReplacement {
        expected_epoch,
        target: RetentionTarget::RacfEvidence,
        archived_tick,
        watermark_tick,
        replacement: ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: DATABASE_NAMESPACE.into(),
                key: DATABASE_KEY.into(),
                version: replacement.generation,
                payload: encode_snapshot(&replacement, limits)?,
            },
            expected_version: Some(source.version),
        },
        source,
        rows: selected
            .iter()
            .map(|candidate| ProviderRetentionRow {
                row: ProviderStateRecord {
                    namespace: candidate.encoded.descriptor.namespace.clone(),
                    key: candidate.encoded.descriptor.key.clone(),
                    version: LOGICAL_SOURCE_VERSION,
                    payload: candidate.encoded.payload.clone(),
                },
                owner_execution: None,
                owner_run_unit: None,
                retention_tick: candidate.retention_tick,
                observation: candidate.observation.clone(),
                dependency: ProviderRetentionDependency::None,
            })
            .collect(),
    };
    validate_replacement_request(&request, observations, limits)?;
    Ok(request)
}

fn validate_replacement_request(
    request: &ProviderStateArchiveReplacement,
    observations: &ObservationIndex,
    limits: SecurityDatabaseLimits,
) -> Result<(), HostProblem> {
    if request.target != RetentionTarget::RacfEvidence
        || request.archived_tick == 0
        || request.watermark_tick > request.archived_tick
        || request.rows.is_empty()
        || request.rows.len() > MAX_RACF_RETENTION_BATCH
        || request.source.namespace != DATABASE_NAMESPACE
        || request.source.key != DATABASE_KEY
        || request.replacement.record.namespace != DATABASE_NAMESPACE
        || request.replacement.record.key != DATABASE_KEY
        || request.replacement.expected_version != Some(request.source.version)
        || request.replacement.record.version != request.source.version.checked_add(1).unwrap_or(0)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let source = decode_snapshot(&request.source, limits)?;
    let replacement = decode_snapshot(&request.replacement.record, limits)?;
    let eligibility = eligibility(
        &source,
        observations,
        request.archived_tick,
        request.watermark_tick,
        limits,
    )?;
    if request.rows.len() > eligibility.candidates.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut expected = source.clone();
    let mut identities = BTreeSet::new();
    for (row, candidate) in request.rows.iter().zip(&eligibility.candidates) {
        if row.row.namespace != candidate.encoded.descriptor.namespace
            || row.row.key != candidate.encoded.descriptor.key
            || row.row.version != LOGICAL_SOURCE_VERSION
            || row.row.payload != candidate.encoded.payload
            || row.retention_tick != candidate.retention_tick
            || row.observation != candidate.observation
            || row.retention_tick == 0
            || row.retention_tick > request.watermark_tick
            || row.owner_execution.is_some()
            || row.owner_run_unit.is_some()
            || row.dependency != ProviderRetentionDependency::None
            || !identities.insert((row.row.namespace.as_str(), row.row.key.as_str()))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        validate_canonical_payload(&candidate.encoded, limits)?;
        remove_evidence(&mut expected, &candidate.encoded)?;
    }
    if request.rows.iter().any(|row| {
        row.row.namespace == RACF_TRANSACTION_NAMESPACE
            && expected
                .recovery
                .values()
                .any(|recovery| recovery.transaction_id == row.row.key)
    }) {
        return Err(HostProblem::InfrastructureFailure);
    }
    expected.generation = expected
        .generation
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    if replacement != expected
        || request.replacement.record.payload != encode_snapshot(&expected, limits)?
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn validate_canonical_payload(
    row: &EncodedDescriptor,
    limits: SecurityDatabaseLimits,
) -> Result<(), HostProblem> {
    match row.kind {
        EvidenceKind::Audit => decode_canonical::<SecurityAuditRecord>(&row.payload, limits),
        EvidenceKind::Transaction => decode_canonical::<SecurityTransaction>(&row.payload, limits),
        EvidenceKind::Recovery => decode_canonical::<RecoveryRecord>(&row.payload, limits),
    }
}

fn decode_canonical<T: DeserializeOwned + Serialize>(
    payload: &[u8],
    limits: SecurityDatabaseLimits,
) -> Result<(), HostProblem> {
    if payload.len() > limits.max_database_bytes {
        return Err(HostProblem::InfrastructureFailure);
    }
    let decoded: T =
        serde_json::from_slice(payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if serde_json::to_vec(&decoded).map_err(|_| HostProblem::InfrastructureFailure)? == payload {
        Ok(())
    } else {
        Err(HostProblem::InfrastructureFailure)
    }
}

fn remove_evidence(
    snapshot: &mut SecurityDatabaseSnapshot,
    row: &EncodedDescriptor,
) -> Result<(), HostProblem> {
    match row.kind {
        EvidenceKind::Audit => {
            let before = snapshot.audits.len();
            snapshot
                .audits
                .retain(|audit| audit.id != row.descriptor.key);
            if snapshot.audits.len().checked_add(1) != Some(before) {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        EvidenceKind::Transaction => {
            if snapshot.transactions.remove(&row.descriptor.key).is_none() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        EvidenceKind::Recovery => {
            if snapshot.recovery.remove(&row.descriptor.key).is_none() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
    }
    Ok(())
}

fn receipt(
    generation: u64,
    watermark_tick: u64,
    rows: &[ProviderRetentionRow],
    archive_id: Option<String>,
) -> RacfRetentionReceipt {
    RacfRetentionReceipt {
        generation,
        watermark_tick,
        archived_audits: rows
            .iter()
            .filter(|row| row.row.namespace == RACF_AUDIT_NAMESPACE)
            .count(),
        archived_transactions: rows
            .iter()
            .filter(|row| row.row.namespace == RACF_TRANSACTION_NAMESPACE)
            .count(),
        archived_recovery_records: rows
            .iter()
            .filter(|row| row.row.namespace == RACF_RECOVERY_NAMESPACE)
            .count(),
        archive_id,
    }
}

fn empty_receipt(generation: u64, watermark_tick: u64) -> RacfRetentionReceipt {
    RacfRetentionReceipt {
        generation,
        watermark_tick,
        archived_audits: 0,
        archived_transactions: 0,
        archived_recovery_records: 0,
        archive_id: None,
    }
}

fn count_candidates(eligibility: &Eligibility, kind: EvidenceKind) -> usize {
    eligibility
        .candidates
        .iter()
        .filter(|row| row.encoded.kind == kind)
        .count()
}

fn retention_pressure(
    used: usize,
    capacity: usize,
    policy: RacfRetentionPolicy,
) -> RacfRetentionPressure {
    if used >= capacity {
        return RacfRetentionPressure::Full;
    }
    let used_percent = used as u128 * 100;
    let capacity = capacity as u128;
    if used_percent >= capacity * u128::from(policy.high_watermark_percent) {
        RacfRetentionPressure::HighWatermark
    } else if used_percent >= capacity * u128::from(policy.low_watermark_percent) {
        RacfRetentionPressure::LowWatermark
    } else {
        RacfRetentionPressure::Healthy
    }
}

fn retention_pressure_u64(
    used: u64,
    capacity: u64,
    policy: RacfRetentionPolicy,
) -> RacfRetentionPressure {
    if used >= capacity {
        return RacfRetentionPressure::Full;
    }
    let used_percent = u128::from(used) * 100;
    let capacity = u128::from(capacity);
    if used_percent >= capacity * u128::from(policy.high_watermark_percent) {
        RacfRetentionPressure::HighWatermark
    } else if used_percent >= capacity * u128::from(policy.low_watermark_percent) {
        RacfRetentionPressure::LowWatermark
    } else {
        RacfRetentionPressure::Healthy
    }
}

fn terminal_transaction(transaction: &SecurityTransaction) -> bool {
    matches!(
        transaction.state,
        TransactionState::Committed | TransactionState::RolledBack
    )
}

fn terminal_recovery(recovery: &RecoveryRecord) -> bool {
    matches!(
        recovery.state,
        RecoveryState::Reconciled | RecoveryState::Failed
    )
}

fn audit_retention_tick(audit: &SecurityAuditRecord) -> Option<u64> {
    audit
        .retention_observed_tick
        .or_else(|| (audit.tick != 0).then_some(audit.tick))
}

fn old_retention_tick(tick: u64, now_tick: u64, watermark_tick: u64) -> bool {
    tick != 0 && tick < now_tick && tick <= watermark_tick
}

fn valid_evidence_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        RACF_AUDIT_NAMESPACE | RACF_TRANSACTION_NAMESPACE | RACF_RECOVERY_NAMESPACE
    )
}

#[cfg(test)]
pub(super) fn test_build_request(
    expected_epoch: u64,
    source: ProviderStateRecord,
    policy: RacfRetentionPolicy,
    now_tick: u64,
    observations: &[RetentionObservation],
    limits: SecurityDatabaseLimits,
) -> Result<ProviderStateArchiveReplacement, HostProblem> {
    let snapshot = decode_snapshot(&source, limits)?;
    let watermark = now_tick.saturating_sub(policy.retain_ticks);
    let observations = ObservationIndex::new(observations)?;
    let eligible = eligibility(&snapshot, &observations, now_tick, watermark, limits)?;
    let selected = select_candidates(&eligible, policy, policy.max_batch_records);
    build_replacement_request(
        expected_epoch,
        source,
        &snapshot,
        &selected,
        now_tick,
        watermark,
        &observations,
        limits,
    )
}

#[cfg(test)]
pub(super) fn test_validate_request(
    request: &ProviderStateArchiveReplacement,
    observations: &[RetentionObservation],
    limits: SecurityDatabaseLimits,
) -> Result<(), HostProblem> {
    validate_replacement_request(request, &ObservationIndex::new(observations)?, limits)
}
