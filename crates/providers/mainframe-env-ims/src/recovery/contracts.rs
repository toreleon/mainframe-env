//! Bounded, versioned IMS recovery inputs and images. These are provider-side
//! projections; the coordinator owns canonical effects and the shared store
//! owns durable CAS and UOW publication.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const RECOVERY_IMAGE_SCHEMA: &str = "mainframe-env.ims-recovery-image@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryLimits {
    pub max_checkpoints: usize,
    pub max_checkpoint_id_bytes: usize,
    pub max_user_areas: usize,
    pub max_user_area_bytes: usize,
    pub max_positions: usize,
    pub max_position_key_bytes: usize,
    pub max_log_data_bytes: usize,
    pub max_log_records: usize,
    pub max_backout_points: usize,
    pub max_utility_records: usize,
    pub max_utility_bytes: usize,
    pub max_state_bytes: usize,
    pub max_database_name_bytes: usize,
}

impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            max_checkpoints: 1_024,
            max_checkpoint_id_bytes: 8,
            max_user_areas: 7,
            max_user_area_bytes: 32 * 1_024,
            max_positions: 64,
            max_position_key_bytes: 256,
            max_log_data_bytes: 32 * 1_024,
            max_log_records: 65_536,
            max_backout_points: 9,
            max_utility_records: 65_536,
            max_utility_bytes: 64 * 1_024 * 1_024,
            max_state_bytes: 64 * 1_024 * 1_024,
            max_database_name_bytes: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryProblem {
    InvalidRequest,
    Unauthorized,
    LimitExceeded,
    Unsupported,
    NotFound,
    Conflict,
    CorruptImage,
    InfrastructureFailure,
    UnknownOutcome,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryContext {
    Batch,
    MessageDrivenBatch,
    MessageProcessing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointKind {
    Basic,
    Symbolic,
}

/// A PCB's retained hierarchy key or discriminated GSAM logical position.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPcbPosition {
    /// Exact owned format/bounds identity. Absent preserves old checkpoint bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gsam_format: Option<[u8; 32]>,
    pub pcb: String,
    pub database: String,
    pub segment_key: Vec<u8>,
    /// GSAM has no hierarchy key. Absent preserves historical digest bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gsam: Option<SavedGsamPosition>,
}

/// Provider-derived logical positions, never physical IBM RSA layouts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum SavedGsamPosition {
    Beginning,
    Eof,
    Record(mainframe_env_host_api::ImsGsamAddress),
    /// Output resumes appending after an integrity-checked live prefix.
    Output {
        records: usize,
        prefix_digest: [u8; 32],
    },
}

impl SavedPcbPosition {
    fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if !valid_name(&self.pcb, limits.max_database_name_bytes)
            || !valid_name(&self.database, limits.max_database_name_bytes)
            || (self.segment_key.is_empty() != self.gsam.is_some())
            || self.gsam_format.is_some() && self.gsam.is_none()
            || self.gsam_format == Some([0; 32])
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.segment_key.len() > limits.max_position_key_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        if let Some(position) = &self.gsam {
            match position {
                SavedGsamPosition::Record(address)
                    if address.database != self.database || address.token == [0; 32] =>
                {
                    return Err(RecoveryProblem::InvalidRequest);
                }
                SavedGsamPosition::Output { records, .. }
                    if *records > limits.max_utility_records =>
                {
                    return Err(RecoveryProblem::LimitExceeded);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Call-level CHKP contract. Runtime commits through its existing UOW owner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRequest {
    pub id: String,
    pub kind: CheckpointKind,
    pub context: RecoveryContext,
    pub prior_xrst: bool,
    pub user_areas: Vec<Vec<u8>>,
    pub positions: Vec<SavedPcbPosition>,
}

impl CheckpointRequest {
    pub fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if !valid_checkpoint_id(&self.id, limits.max_checkpoint_id_bytes) {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.kind == CheckpointKind::Symbolic {
            if !self.prior_xrst {
                return Err(RecoveryProblem::InvalidRequest);
            }
            if self.context == RecoveryContext::MessageProcessing {
                return Err(RecoveryProblem::Unsupported);
            }
        } else if !self.user_areas.is_empty() {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.user_areas.len() > limits.max_user_areas
            || self.positions.len() > limits.max_positions
            || self
                .user_areas
                .iter()
                .any(|area| area.len() > limits.max_user_area_bytes)
        {
            return Err(RecoveryProblem::LimitExceeded);
        }
        let mut names = BTreeSet::new();
        for position in &self.positions {
            position.validate(limits)?;
            if self.kind == CheckpointKind::Basic && position.gsam.is_some() {
                return Err(RecoveryProblem::Unsupported);
            }
            if !names.insert(position.pcb.as_str()) {
                return Err(RecoveryProblem::InvalidRequest);
            }
        }
        Ok(())
    }
}

/// Integrity-checked checkpoint payload. The digest covers the full image
/// except itself, including the committed database image identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointImage {
    pub schema_version: String,
    pub sequence: u64,
    pub request: CheckpointRequest,
    pub committed_database_digest: [u8; 32],
    pub image_digest: [u8; 32],
}

impl CheckpointImage {
    pub fn seal(
        sequence: u64,
        request: CheckpointRequest,
        committed_database_digest: [u8; 32],
        limits: RecoveryLimits,
    ) -> Result<Self, RecoveryProblem> {
        request.validate(limits)?;
        if sequence == 0 {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let mut image = Self {
            schema_version: RECOVERY_IMAGE_SCHEMA.into(),
            sequence,
            request,
            committed_database_digest,
            image_digest: [0; 32],
        };
        image.image_digest = image.digest();
        Ok(image)
    }

    pub fn verify(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.schema_version != RECOVERY_IMAGE_SCHEMA
            || self.sequence == 0
            || self.request.validate(limits).is_err()
            || self.image_digest != self.digest()
        {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(())
    }

    fn digest(&self) -> [u8; 32] {
        let bytes = serde_json::to_vec(&(
            RECOVERY_IMAGE_SCHEMA,
            &self.schema_version,
            self.sequence,
            &self.request,
            self.committed_database_digest,
        ))
        .expect("bounded checkpoint image is JSON serializable");
        Sha256::digest(bytes).into()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartSelection {
    Normal,
    Last,
    Id(String),
    Timestamp(String),
}

impl RestartSelection {
    pub fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if let Self::Id(id) = self
            && !valid_checkpoint_id(id, limits.max_checkpoint_id_bytes)
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if let Self::Timestamp(timestamp) = self
            && (timestamp.len() != 14
                || !timestamp.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepositionStatus {
    Reestablished,
    NotFound,
    NotAttempted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartPcbStatus {
    pub pcb: String,
    pub status: RepositionStatus,
}

/// Payload of a LOG call, excluding LL/ZZ/C framing. LL is checked by the
/// caller's chosen interface; the generic bound also fits the two-byte form.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogRequest {
    pub code: u8,
    pub data: Vec<u8>,
}

impl LogRequest {
    pub fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if self.code < 0xa0 {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.data.len() > limits.max_log_data_bytes || self.data.len() > u16::MAX as usize - 5 {
            return Err(RecoveryProblem::LimitExceeded);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackoutPointKind {
    Sets,
    Setu,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UtilityKind {
    InitialLoad,
    Extract,
    Reorganize,
    DatabaseRecovery,
    LogRecovery,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UtilityPlan {
    pub kind: UtilityKind,
    pub database: String,
    pub expected_input_digest: [u8; 32],
    pub expected_records: usize,
}

impl UtilityPlan {
    pub fn validate(&self, limits: RecoveryLimits) -> Result<(), RecoveryProblem> {
        if !valid_name(&self.database, limits.max_database_name_bytes) {
            return Err(RecoveryProblem::InvalidRequest);
        }
        if self.expected_records > limits.max_utility_records {
            return Err(RecoveryProblem::LimitExceeded);
        }
        Ok(())
    }
}

fn valid_checkpoint_id(id: &str, bound: usize) -> bool {
    !id.is_empty()
        && id.len() <= bound.min(8)
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_name(name: &str, bound: usize) -> bool {
    !name.is_empty()
        && name.len() <= bound
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
