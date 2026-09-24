//! Shared durable BTS process and activity authority.
//!
//! A process is one CAS object containing its root and bounded descendant tree.
//! This keeps parent/child transitions, completion events, and replay records in
//! one atomic write. The UOW acquisition is a separate versioned row and is
//! changed atomically with process definition. Sibling BTS command families use
//! this authority rather than storing their own process or activity copies.

use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PROCESS_NAMESPACE: &str = "cics-bts-process-v1";
const ACQUISITION_NAMESPACE: &str = "cics-bts-acquisition-v1";
const ACTIVITY_INDEX_NAMESPACE: &str = "cics-bts-activity-index-v1";
const PROCESS_SCHEMA: &str = "mainframe-env.cics.bts-process@1";
const ACQUISITION_SCHEMA: &str = "mainframe-env.cics.bts-acquisition@1";
const ACTIVITY_INDEX_SCHEMA: &str = "mainframe-env.cics.bts-activity-index@1";
const MAX_ACTIVITIES: usize = 256;
const MAX_REPLAYS: usize = 512;
const MAX_ROW_BYTES: usize = 1_048_576;
const MAX_CAS_ATTEMPTS: usize = 32;

/// Processing mode recorded by the BTS authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BtsMode {
    Initial,
    Active,
    Dormant,
    Cancelling,
    Complete,
}

/// Completion state is independent of the processing mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BtsCompletion {
    Incomplete,
    Normal,
    Abend,
    Forced,
}

/// A reference to a coordinator-owned checkpoint, fenced by activation epoch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsCheckpoint {
    pub schema_version: u8,
    pub activation_epoch: u64,
    pub owner_lease_epoch: u64,
    pub reference: String,
}

/// One activity in a process's bounded durable tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsActivity {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub completion_event: Option<String>,
    pub program: String,
    pub transid: String,
    pub userid: String,
    pub mode: BtsMode,
    pub completion: BtsCompletion,
    pub suspended: bool,
    pub activation_epoch: u64,
    pub checkpoint: Option<BtsCheckpoint>,
    pub acquired_by: Option<String>,
    /// A newly defined child is visible only in this UOW until syncpoint.
    #[serde(default)]
    pub pending_uow: Option<String>,
    pub abcode: Option<String>,
    pub abprogram: Option<String>,
}

/// Exact owner and result of one mutating BTS effect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsReplay {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub owner_principal: String,
    pub request_digest: [u8; 32],
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub outputs: BTreeMap<String, Vec<u8>>,
}

/// Bounded result saved atomically with a process mutation for fenced replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtsReply {
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub outputs: BTreeMap<String, Vec<u8>>,
}

impl BtsReply {
    pub fn normal() -> Self {
        Self {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            outputs: BTreeMap::new(),
        }
    }
}

/// A versioned process row; its root activity is identified by `root_id`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsProcess {
    pub schema_version: String,
    pub process_type: String,
    pub name: String,
    pub root_id: String,
    pub next_child_sequence: u64,
    pub epoch: u64,
    /// A DEFINE is visible only to its creating UOW until successful syncpoint.
    pub pending_uow: Option<String>,
    pub activities: BTreeMap<String, BtsActivity>,
    pub replays: BTreeMap<String, BtsReplay>,
    #[serde(skip)]
    pub row_version: u64,
}

impl BtsProcess {
    /// Build a pending process with one root activity.
    pub fn new(
        process_type: &str,
        name: &str,
        root_id: &str,
        program: &str,
        transid: &str,
        userid: &str,
        defining_uow: &str,
    ) -> Result<Self, HostProblem> {
        validate_name(process_type, 8, true)?;
        validate_name(name, 36, true)?;
        validate_activity_id(root_id)?;
        validate_identifier(program, 8)?;
        validate_identifier(transid, 4)?;
        validate_identifier(userid, 8)?;
        validate_identifier(defining_uow, 256)?;
        let root = BtsActivity {
            id: root_id.into(),
            name: name.into(),
            parent_id: None,
            completion_event: None,
            program: program.into(),
            transid: transid.into(),
            userid: userid.into(),
            mode: BtsMode::Initial,
            completion: BtsCompletion::Incomplete,
            suspended: false,
            activation_epoch: 0,
            checkpoint: None,
            acquired_by: Some(defining_uow.into()),
            pending_uow: Some(defining_uow.into()),
            abcode: None,
            abprogram: None,
        };
        let process = Self {
            schema_version: PROCESS_SCHEMA.into(),
            process_type: process_type.into(),
            name: name.into(),
            root_id: root_id.into(),
            next_child_sequence: 1,
            epoch: 1,
            pending_uow: Some(defining_uow.into()),
            activities: BTreeMap::from([(root_id.into(), root)]),
            replays: BTreeMap::new(),
            row_version: 0,
        };
        process.validate()?;
        Ok(process)
    }

    /// A pending definition is accessible only to the defining unit of work.
    pub fn visible_to(&self, uow: &str) -> bool {
        self.pending_uow.as_deref().is_none_or(|owner| owner == uow)
    }

    /// Find exactly one named direct child of an activity.
    pub fn child(&self, parent_id: &str, name: &str) -> Option<&BtsActivity> {
        self.activities.values().find(|activity| {
            activity.parent_id.as_deref() == Some(parent_id) && activity.name == name
        })
    }

    /// Reject corrupt, unbounded, or internally inconsistent persisted state.
    pub fn validate(&self) -> Result<(), HostProblem> {
        let bad = || HostProblem::InfrastructureFailure;
        if self.schema_version != PROCESS_SCHEMA
            || validate_name(&self.process_type, 8, true).is_err()
            || validate_name(&self.name, 36, true).is_err()
            || validate_activity_id(&self.root_id).is_err()
            || self.next_child_sequence == 0
            || self.epoch == 0
            || self.activities.is_empty()
            || self.activities.len() > MAX_ACTIVITIES
            || self.replays.len() > MAX_REPLAYS
            || self
                .pending_uow
                .as_deref()
                .is_some_and(|uow| validate_identifier(uow, 256).is_err())
        {
            return Err(bad());
        }
        let mut names = BTreeSet::new();
        let mut completion_events = BTreeSet::new();
        for (id, activity) in &self.activities {
            if id != &activity.id
                || validate_activity_id(id).is_err()
                || validate_name(
                    &activity.name,
                    if id == &self.root_id { 36 } else { 16 },
                    id == &self.root_id,
                )
                .is_err()
                || validate_identifier(&activity.program, 8).is_err()
                || validate_identifier(&activity.transid, 4).is_err()
                || validate_identifier(&activity.userid, 8).is_err()
                || activity
                    .abcode
                    .as_deref()
                    .is_some_and(|code| code.len() != 4)
                || activity
                    .abprogram
                    .as_deref()
                    .is_some_and(|name| name.len() != 8)
                || activity
                    .acquired_by
                    .as_deref()
                    .is_some_and(|owner| validate_identifier(owner, 256).is_err())
                || activity
                    .pending_uow
                    .as_deref()
                    .is_some_and(|owner| validate_identifier(owner, 256).is_err())
                || activity
                    .completion_event
                    .as_deref()
                    .is_some_and(|name| validate_name(name, 16, false).is_err())
                || activity.mode == BtsMode::Complete
                    && activity.completion == BtsCompletion::Incomplete
                || activity.mode != BtsMode::Complete
                    && activity.completion != BtsCompletion::Incomplete
                || activity.checkpoint.as_ref().is_some_and(|checkpoint| {
                    checkpoint.schema_version != 1
                        || checkpoint.activation_epoch != activity.activation_epoch
                        || checkpoint.owner_lease_epoch == 0
                        || validate_identifier(&checkpoint.reference, 256).is_err()
                })
            {
                return Err(bad());
            }
            match &activity.parent_id {
                None if id == &self.root_id
                    && activity.name == self.name
                    && activity.pending_uow == self.pending_uow => {}
                Some(parent) if id != &self.root_id && self.activities.contains_key(parent) => {
                    if activity.completion_event.is_none() {
                        return Err(bad());
                    }
                    if !names.insert((parent.clone(), activity.name.clone())) {
                        return Err(bad());
                    }
                    if !completion_events
                        .insert((parent.clone(), activity.completion_event.clone()))
                    {
                        return Err(bad());
                    }
                }
                _ => return Err(bad()),
            }
            let mut visited = BTreeSet::new();
            let mut cursor = activity;
            while let Some(parent) = &cursor.parent_id {
                if !visited.insert(cursor.id.as_str()) || visited.len() > MAX_ACTIVITIES {
                    return Err(bad());
                }
                cursor = self.activities.get(parent).ok_or_else(bad)?;
            }
            if cursor.id != self.root_id {
                return Err(bad());
            }
        }
        for (key, replay) in &self.replays {
            if validate_identifier(key, 256).is_err()
                || validate_identifier(&replay.owner_execution, 256).is_err()
                || validate_identifier(&replay.owner_run_unit, 256).is_err()
                || validate_identifier(&replay.owner_principal, 256).is_err()
                || validate_identifier(&replay.condition, 32).is_err()
                || replay.outputs.len() > 16
                || replay.outputs.values().any(|bytes| bytes.len() > 1024)
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}

/// Scope held for one UOW; the epoch is retained across syncpoints to fence ABA.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsAcquisition {
    pub schema_version: String,
    pub epoch: u64,
    pub owner_execution: String,
    pub owner_principal: String,
    pub process_type: Option<String>,
    pub process_name: Option<String>,
    pub activity_id: Option<String>,
    #[serde(default)]
    pub effect: Option<BtsAcquisitionEffect>,
    #[serde(skip)]
    pub row_version: u64,
}

/// One exact ACQUIRE or DEFINE PROCESS effect retained across UOW release.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsAcquisitionEffect {
    pub operation: String,
    pub key: String,
    pub request_digest: [u8; 32],
    pub process_type: String,
    pub process_name: String,
    pub activity_id: String,
    /// Repository-name reservation, present for catalog-backed DEFINE.
    #[serde(default)]
    pub repository_resource: Option<String>,
}

impl BtsAcquisition {
    pub fn empty(owner_execution: &str, owner_principal: &str) -> Result<Self, HostProblem> {
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        Ok(Self {
            schema_version: ACQUISITION_SCHEMA.into(),
            epoch: 1,
            owner_execution: owner_execution.into(),
            owner_principal: owner_principal.into(),
            process_type: None,
            process_name: None,
            activity_id: None,
            effect: None,
            row_version: 0,
        })
    }

    pub fn is_held(&self) -> bool {
        self.activity_id.is_some()
    }

    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != ACQUISITION_SCHEMA
            || self.epoch == 0
            || validate_identifier(&self.owner_execution, 256).is_err()
            || validate_identifier(&self.owner_principal, 256).is_err()
            || self.process_type.is_some() != self.process_name.is_some()
            || self.process_name.is_some() != self.activity_id.is_some()
            || self
                .process_type
                .as_deref()
                .is_some_and(|name| validate_name(name, 8, true).is_err())
            || self
                .process_name
                .as_deref()
                .is_some_and(|name| validate_name(name, 36, true).is_err())
            || self
                .activity_id
                .as_deref()
                .is_some_and(|id| validate_activity_id(id).is_err())
            || self.effect.as_ref().is_some_and(|effect| {
                !matches!(
                    effect.operation.as_str(),
                    "ACQUIRE ACTIVITYID" | "ACQUIRE PROCESS" | "DEFINE PROCESS"
                ) || validate_identifier(&effect.key, 256).is_err()
                    || validate_name(&effect.process_type, 8, true).is_err()
                    || validate_name(&effect.process_name, 36, true).is_err()
                    || validate_activity_id(&effect.activity_id).is_err()
                    || effect
                        .repository_resource
                        .as_deref()
                        .is_some_and(|resource| {
                            effect.operation != "DEFINE PROCESS"
                                || validate_identifier(resource, 44).is_err()
                        })
            })
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }
}

/// Durable index for resolving an opaque 52-byte activity ID without a scan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsActivityIndex {
    pub schema_version: String,
    pub activity_id: String,
    pub process_type: String,
    pub process_name: String,
    pub parent_id: Option<String>,
    pub pending_uow: Option<String>,
    #[serde(skip)]
    pub row_version: u64,
}

impl BtsActivityIndex {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != ACTIVITY_INDEX_SCHEMA
            || validate_activity_id(&self.activity_id).is_err()
            || validate_name(&self.process_type, 8, true).is_err()
            || validate_name(&self.process_name, 36, true).is_err()
            || self
                .parent_id
                .as_deref()
                .is_some_and(|parent| validate_activity_id(parent).is_err())
            || self
                .pending_uow
                .as_deref()
                .is_some_and(|owner| validate_identifier(owner, 256).is_err())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }
}

/// The minimum participant boundary shared with later BTS command slices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BtsParticipantContract {
    pub syncpoint_owner: &'static str,
    pub prepare_supported: bool,
    pub automatic_compensation: bool,
    pub idempotency_scope: &'static str,
    pub unknown_outcome_reissue: bool,
    pub schema_version: u8,
}

pub const BTS_PARTICIPANT: BtsParticipantContract = BtsParticipantContract {
    syncpoint_owner: "cics-uow",
    prepare_supported: false,
    automatic_compensation: false,
    idempotency_scope: "execution-run-unit-effect-sequence",
    unknown_outcome_reissue: false,
    schema_version: 1,
};

mod cancel;
mod children;
mod context;
mod participant;
mod removal;
mod repository;
mod run;
mod store;
mod transitions;
pub use context::BtsActivityContext;
pub(in crate::service) use participant::settle_recorded_uow;
pub use run::{BTS_RUN_WORK_GENERATION, BtsRunRecord, BtsRunState};
pub use store::BtsLifecycleStore;

fn put_process(
    key: &str,
    process: &BtsProcess,
    expected_version: Option<u64>,
) -> Result<ProviderStateMutation, HostProblem> {
    process.validate()?;
    let payload = serde_json::to_vec(process).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_ROW_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    let version = expected_version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: PROCESS_NAMESPACE.into(),
            key: key.into(),
            version,
            payload,
        },
        expected_version,
    }))
}

fn put_acquisition(
    run_unit: &str,
    acquisition: &BtsAcquisition,
) -> Result<ProviderStateMutation, HostProblem> {
    acquisition.validate()?;
    let payload = serde_json::to_vec(acquisition).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > 2048 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: ACQUISITION_NAMESPACE.into(),
            key: run_unit.into(),
            version: acquisition.row_version + 1,
            payload,
        },
        expected_version: (acquisition.row_version != 0).then_some(acquisition.row_version),
    }))
}

fn put_activity_index(
    index: &BtsActivityIndex,
    expected_version: Option<u64>,
) -> Result<ProviderStateMutation, HostProblem> {
    index.validate()?;
    let payload = serde_json::to_vec(index).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > 1024 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: ACTIVITY_INDEX_NAMESPACE.into(),
            key: index.activity_id.clone(),
            version: expected_version
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload,
        },
        expected_version,
    }))
}

fn activity_id(process_key: &str, incarnation: &str, sequence: u64) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cics.bts-activity-id@1\0");
    hash.update(process_key.as_bytes());
    hash.update([0]);
    hash.update(incarnation.as_bytes());
    hash.update([0]);
    hash.update(sequence.to_be_bytes());
    let digest = hash.finalize();
    let mut result = String::with_capacity(52);
    for byte in digest.iter().take(26) {
        result.push_str(&format!("{byte:02X}"));
    }
    result
}

fn validate_activity_id(value: &str) -> Result<(), HostProblem> {
    if value.len() != 52 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn validate_identifier(value: &str, max: usize) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > max
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn validate_name(value: &str, max: usize, blanks: bool) -> Result<(), HostProblem> {
    validate_identifier(value, max)?;
    if value.trim_ascii().is_empty()
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || b"$@#/%&?!:|\"=,;<>.-_".contains(&byte)
                || blanks && byte == b' ')
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn acquisition_and_definition_replay_exact_effect_after_restart() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process_exact(process.clone(), "UOW1", "EXEC1", "USER", "define", [1; 32])
            .unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        reopened
            .define_process_exact(process.clone(), "UOW1", "EXEC1", "USER", "define", [1; 32])
            .unwrap();
        assert_eq!(
            reopened.define_process_exact(process, "UOW1", "EXEC1", "USER", "define", [2; 32]),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        reopened
            .acquire_exact(
                "UOW2",
                "EXEC2",
                "USER",
                "TYPE",
                "ORDER",
                &root,
                "ACQUIRE PROCESS",
                "acquire",
                [3; 32],
            )
            .unwrap();
        let restarted = BtsLifecycleStore::new(&memory);
        restarted
            .acquire_exact(
                "UOW2",
                "EXEC2",
                "USER",
                "TYPE",
                "ORDER",
                &root,
                "ACQUIRE PROCESS",
                "acquire",
                [3; 32],
            )
            .unwrap();
        assert_eq!(
            restarted.acquire_exact(
                "UOW2",
                "EXEC2",
                "USER",
                "TYPE",
                "ORDER",
                &root,
                "ACQUIRE PROCESS",
                "acquire",
                [4; 32],
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            restarted.acquire_exact(
                "UOW2",
                "EXEC2",
                "USER",
                "TYPE",
                "ORDER",
                &root,
                "ACQUIRE PROCESS",
                "different",
                [5; 32],
            ),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 22,
            })
        );
    }

    #[test]
    fn definition_and_acquisition_are_atomic_and_fenced_across_restart() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process(process, "UOW1", "EXEC1", "USER")
            .unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        let saved = reopened.load_process("TYPE", "ORDER").unwrap().unwrap();
        assert_eq!(saved.row_version, 1);
        assert_eq!(saved.activities.len(), 1);
        assert!(saved.visible_to("UOW1"));
        assert!(!saved.visible_to("UOW2"));
        let held = reopened.load_acquisition("UOW1").unwrap().unwrap();
        assert_eq!(held.activity_id.as_deref(), Some(root.as_str()));
        assert_eq!(held.epoch, 1);
        reopened.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        let released = reopened.load_acquisition("UOW1").unwrap().unwrap();
        assert!(!released.is_held());
        assert_eq!(released.epoch, 2);
        assert!(
            reopened
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .pending_uow
                .is_none()
        );
        assert!(
            reopened
                .load_activity_index(&root)
                .unwrap()
                .unwrap()
                .pending_uow
                .is_none()
        );
    }

    #[test]
    fn duplicate_definition_leaves_prior_process_and_acquisition_unchanged() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let first =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process(first, "UOW1", "EXEC1", "USER")
            .unwrap();
        let second_root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW2").unwrap();
        assert_ne!(root, second_root);
        let second = BtsProcess::new(
            "TYPE",
            "ORDER",
            &second_root,
            "MAIN",
            "BTS1",
            "USER",
            "UOW2",
        )
        .unwrap();
        assert!(
            authority
                .define_process(second, "UOW2", "EXEC2", "USER")
                .is_err()
        );
        assert!(authority.load_acquisition("UOW2").unwrap().is_none());
        assert_eq!(
            authority
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .root_id,
            root
        );
    }

    #[test]
    fn cycle_and_checkpoint_epoch_are_rejected() {
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let mut process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW").unwrap();
        process.activities.get_mut(&root).unwrap().checkpoint = Some(BtsCheckpoint {
            schema_version: 1,
            activation_epoch: 1,
            owner_lease_epoch: 1,
            reference: "cp-1".into(),
        });
        assert!(process.validate().is_err());
        process.activities.get_mut(&root).unwrap().checkpoint = None;
        process.activities.get_mut(&root).unwrap().parent_id = Some(root.clone());
        assert!(process.validate().is_err());
    }

    #[test]
    fn rollback_removes_pending_process_and_root_index_atomically() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process(process, "UOW1", "EXEC1", "USER")
            .unwrap();
        authority
            .finish_uow("UOW1", "EXEC1", "USER", false)
            .unwrap();
        assert!(authority.load_process("TYPE", "ORDER").unwrap().is_none());
        assert!(authority.load_activity_index(&root).unwrap().is_none());
        assert_eq!(
            authority.load_acquisition("UOW1").unwrap().unwrap().epoch,
            2
        );
    }

    #[test]
    fn acquisition_and_replay_survive_reopen_without_repeating_transition() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        authority
            .define_process(process, "UOW1", "EXEC1", "USER")
            .unwrap();
        authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        authority
            .acquire("UOW2", "EXEC2", "USER", "TYPE", "ORDER", &root)
            .unwrap();
        let first = authority
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "effect-1",
                [7; 32],
                |process| {
                    process.activities.get_mut(&root).unwrap().suspended = true;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        assert_eq!(first, BtsReply::normal());
        let reopened = BtsLifecycleStore::new(&memory);
        let replay = reopened
            .mutate_process(
                "TYPE",
                "ORDER",
                "UOW2",
                "EXEC2",
                "USER",
                "effect-1",
                [7; 32],
                |_| panic!("replay must not run transition"),
            )
            .unwrap();
        assert_eq!(replay, first);
        assert!(
            reopened
                .mutate_process(
                    "TYPE",
                    "ORDER",
                    "UOW2",
                    "EXEC2",
                    "USER",
                    "effect-1",
                    [8; 32],
                    |_| { Ok(BtsReply::normal()) }
                )
                .is_err()
        );
        assert!(
            reopened
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .suspended
        );
        reopened.finish_uow("UOW2", "EXEC2", "USER", true).unwrap();
        assert!(
            reopened
                .load_process("TYPE", "ORDER")
                .unwrap()
                .unwrap()
                .activities[&root]
                .acquired_by
                .is_none()
        );
    }

    #[test]
    fn sqlite_reopen_preserves_epoch_replay_and_pending_rollback() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-authority-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            let process =
                BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
            authority
                .define_process_exact(process, "UOW1", "EXEC1", "USER", "define", [9; 32])
                .unwrap();
            assert!(authority.load_process("TYPE", "ORDER").unwrap().is_some());
        }
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            assert!(
                !authority
                    .load_process("TYPE", "ORDER")
                    .unwrap()
                    .unwrap()
                    .visible_to("UOW2")
            );
            authority
                .define_process_exact(
                    BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1")
                        .unwrap(),
                    "UOW1",
                    "EXEC1",
                    "USER",
                    "define",
                    [9; 32],
                )
                .unwrap();
            authority
                .finish_uow("UOW1", "EXEC1", "USER", false)
                .unwrap();
            assert!(authority.load_process("TYPE", "ORDER").unwrap().is_none());
            assert!(authority.load_activity_index(&root).unwrap().is_none());
            let acquisition = authority.load_acquisition("UOW1").unwrap().unwrap();
            assert_eq!(acquisition.epoch, 2);
            assert_eq!(acquisition.effect.unwrap().request_digest, [9; 32]);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn first_v1_pending_root_remains_readable_after_child_uow_extension() {
        let memory = MemoryStore::new(Default::default());
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        let process =
            BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap();
        let mut old = serde_json::to_value(&process).unwrap();
        old["activities"][&root]
            .as_object_mut()
            .unwrap()
            .remove("pending_uow");
        memory
            .put_provider_state(
                ProviderStateRecord {
                    namespace: PROCESS_NAMESPACE.into(),
                    key: BtsLifecycleStore::process_key("TYPE", "ORDER").unwrap(),
                    version: 1,
                    payload: serde_json::to_vec(&old).unwrap(),
                },
                None,
            )
            .unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        let loaded = reopened.load_process("TYPE", "ORDER").unwrap().unwrap();
        assert_eq!(
            loaded.activities[&root].pending_uow.as_deref(),
            Some("UOW1")
        );
    }
}
