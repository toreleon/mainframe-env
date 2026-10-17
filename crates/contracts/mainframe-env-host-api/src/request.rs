use crate::clock::ClockRequest;
use crate::dataset::{
    DatasetLockTarget, DatasetProviderCapabilities, DatasetSnapshot, TvsRecordOperation,
};
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, InvocationLimits, RunUnitId,
};
use serde::{Deserialize, Serialize};
use std::fmt;
mod host_request;
mod host_result;
mod ims;
pub use host_result::HostResult;
pub use ims::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Resource ceilings for host request and result validation.
/// Byte limits count encoded bytes, not characters; providers may impose tighter limits.
pub struct HostLimits {
    /// Maximum encoded byte length for bounded names and identifiers.
    pub max_name_bytes: usize,
    /// Maximum bytes in one record, key, message or comparable record payload.
    pub max_record_bytes: usize,
    /// Maximum record or bounded listing count admitted by host validation.
    pub max_records: usize,
    /// Maximum count of fields, operands or qualifiers where the contract checks a field bound.
    pub max_fields: usize,
    /// Maximum number of supplemental audit key/value pairs.
    pub max_audit_fields: usize,
    /// Maximum bytes for state values and other payloads checked against the state byte ceiling.
    pub max_state_bytes: usize,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_name_bytes: 128,
            max_record_bytes: 1024 * 1024,
            max_records: 4096,
            max_fields: 512,
            max_audit_fields: 128,
            max_state_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Storage organization carried by the dataset contract.
/// A variant identifies the requested layout; provider capabilities determine availability.
pub enum DatasetOrganization {
    /// Sequential record dataset.
    Sequential,
    /// Directory of named members in a partitioned dataset.
    Partitioned,
    /// Extended partitioned dataset with member generations.
    PartitionedExtended,
    /// Records addressed by an embedded key.
    KeySequenced,
    /// Records addressed by entry position or relative byte address.
    EntrySequenced,
    /// Records addressed by relative record number.
    Relative,
    /// Relative records with variable-length payloads.
    VariableRelative,
    /// Byte-addressed linear dataset.
    Linear,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Logical record representation carried in dataset attributes.
/// These tags do not themselves implement physical blocking or spanning.
pub enum RecordFormat {
    /// Fixed-length logical records.
    Fixed,
    /// Fixed-length records grouped into blocks.
    FixedBlocked,
    /// Standard blocked fixed-length format tag.
    FixedBlockedStandard,
    /// Variable-length logical records.
    Variable,
    /// Variable-length records grouped into blocks.
    VariableBlocked,
    /// Variable-length records that may span blocks.
    VariableSpanned,
    /// Blocked variable-length format permitting spanning.
    VariableBlockedSpanned,
    /// Record structure supplied by the access method or caller.
    Undefined,
    /// Line-oriented record representation.
    Line,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Comparison used to position a keyed dataset browse relative to supplied key bytes.
pub enum KeyRelation {
    /// Select an exact key match.
    Equal,
    /// Select a key strictly greater than the supplied key.
    Greater,
    /// Select an equal key or the first greater key.
    GreaterOrEqual,
    /// Select a key strictly less than the supplied key.
    Less,
    /// Select an equal key or the first lesser key.
    LessOrEqual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Requested read-lock behavior, subject to the dataset provider and access mode.
pub enum DatasetReadLockMode {
    #[default]
    /// Use the provider-selected default lock behavior.
    Default,
    /// Request a read lock.
    Lock,
    /// Request a lock retained beyond the read.
    KeptLock,
    /// Request a read without acquiring a lock.
    NoLock,
    /// Request the provider-supported ignore-lock mode.
    IgnoreLock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Optional lock and wait controls accompanying a dataset read.
pub struct DatasetReadControl {
    /// Requested lock behavior for this read.
    pub lock: DatasetReadLockMode,
    /// Optional wait choice; `None` leaves selection to the provider.
    pub wait: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Selects the reel or unit form of a dataset close request.
pub enum DatasetReelUnit {
    /// Select reel-oriented close handling.
    Reel,
    /// Select unit-oriented close handling.
    Unit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Close options passed to the dataset provider for applicability checks.
pub struct DatasetCloseControl {
    /// Optional reel or unit close form.
    pub reel_or_unit: Option<DatasetReelUnit>,
    /// Request close without rewinding.
    pub no_rewind: bool,
    /// Request media removal handling.
    pub removal: bool,
    /// Request the provider's close-lock behavior.
    pub lock: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Requested access level for a resource authorization decision.
pub enum AccessIntent {
    /// Read resource contents.
    Read,
    /// Execute the named resource.
    Execute,
    /// Update existing resource contents.
    Update,
    /// Request control-level resource access.
    Control,
    /// Request alter-level resource access.
    Alter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned reference to a credential resolved by the security provider.
/// The string identifies a secret; callers should not place credential bytes in it.
pub struct SecretRef(String);

impl SecretRef {
    /// Own a nonempty credential reference within `max_name_bytes`.
    /// Returns `Malformed` for an empty, oversized or whitespace-containing value.
    pub fn new(value: impl Into<String>, limits: HostLimits) -> Result<Self, HostProblem> {
        let value = value.into();
        if value.is_empty()
            || value.len() > limits.max_name_bytes
            || value.contains(char::is_whitespace)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    /// Borrow the retained reference string without resolving the secret.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Logical dataset layout and optional character-encoding metadata.
/// Keyed layouts require a nonempty key wholly within the logical record.
pub struct DatasetAttributes {
    /// Requested record-addressing organization.
    pub organization: DatasetOrganization,
    /// Requested logical record representation.
    pub record_format: RecordFormat,
    /// Nonzero logical record length in bytes, bounded by `max_record_bytes`.
    pub logical_record_length: u32,
    /// Optional zero-based byte offset of the embedded key.
    pub key_offset: Option<u32>,
    /// Optional nonzero key length in bytes; supplied together with `key_offset`.
    pub key_length: Option<u32>,
    /// Optional nonzero coded character set identifier; absence carries no explicit encoding
    /// choice.
    pub ccsid: Option<u16>,
}

impl DatasetAttributes {
    /// Check record size, paired key bounds and nonzero optional CCSID.
    /// A key is required exactly for the key-sequenced organization; invalid layouts return
    /// `Malformed`.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.logical_record_length == 0
            || self.logical_record_length as usize > limits.max_record_bytes
            || self
                .key_offset
                .zip(self.key_length)
                .is_some_and(|(offset, length)| {
                    length == 0
                        || offset
                            .checked_add(length)
                            .is_none_or(|end| end > self.logical_record_length)
                })
            || self.key_offset.is_some() != self.key_length.is_some()
            || (self.organization == DatasetOrganization::KeySequenced) != self.key_offset.is_some()
            || self.ccsid == Some(0)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Replay identity for a host mutation, with an optional transaction association.
/// The outer effect must carry the same sequence and idempotency key.
pub struct Mutation {
    /// Nonzero effect sequence used to correlate and validate replay metadata.
    pub sequence: u64,
    /// Stable identity for replay protection of this mutation.
    pub idempotency_key: IdempotencyKey,
    /// Optional nonempty transaction identity bounded by `max_name_bytes`.
    pub transaction: Option<String>,
}

impl Mutation {
    /// Check the nonzero sequence and optional transaction-name bound.
    /// Outer key/sequence agreement is checked by `EffectRequest::validate`.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0
            || self
                .transaction
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > limits.max_name_bytes)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

mod dataset_types;
pub use dataset_types::*;

mod runtime_types;
pub use runtime_types::*;

mod database_types;
pub use database_types::*;

mod mq;
pub use mq::*;
mod mq_mqi;
pub use mq_mqi::*;
mod browse;
mod cics;
pub use cics::*;
mod program;
pub use program::*;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned request dispatched under a built-in host capability.
/// Local shape validation does not establish provider support or resource authorization.
pub enum HostRequest {
    /// Dataset access and catalog operation.
    Dataset(DatasetRequest),
    /// Program control operation.
    Program(ProgramRequest),
    /// Job-owned spool operation.
    Spool(SpoolRequest),
    /// Terminal session operation.
    Terminal(TerminalRequest),
    /// Security identity, access or audit operation.
    Security(SecurityRequest),
    /// Clock value selection.
    Clock(ClockRequest),
    /// Opaque host state operation.
    State(StateRequest),
    /// Typed CICS command with its command-owned validation and replay identity.
    /// Mutation identity is checked before provider dispatch.
    Cics(CicsRequest),
    /// Bounded SQL operation or completion data.
    Db2(Db2Request),
    /// Bounded IMS operation or status/data observation.
    Ims(ImsRequest),
    /// Additive owned application recovery call, separate from database operands.
    ImsRecovery(crate::ImsRecoveryRequest),
    /// Owned selected-PCB navigation occurrence.
    ImsNavigation(crate::ImsNavigationRequest),
    /// Standalone-batch GSAM record calls with owned logical addresses.
    ImsGsam(crate::ImsGsamRequest),
    /// Versioned owned selected-database-PCB feedback route.
    ImsPcbFeedbackV1(crate::ImsPcbFeedbackRequestV1),
    /// Legacy MQ request or observation; typed MQI uses its separate contract.
    Mq(MqRequest),
    /// Additive journal/replay boundary; public MQI dispatch remains pending.
    MqMqi(Box<MqMqiHostRequest>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Original host effect envelope. Mutations must bind the outer key and sequence to the nested mutation.
pub struct EffectRequest {
    /// Original run-unit identity retained across dispatch and replay.
    pub run_unit: RunUnitId,
    /// Positive original effect sequence used for request/reply and nested-mutation binding.
    pub sequence: u64,
    /// Positive absolute deadline in the execution clock domain; validation does not read or advance a clock.
    pub deadline_tick: u64,
    /// Original replay key; required for mutations and compared against the nested mutation key.
    pub idempotency_key: Option<IdempotencyKey>,
    /// Original typed host operands; nested mutation identity must match this envelope.
    pub request: HostRequest,
}

impl EffectRequest {
    /// Require a nonzero sequence/deadline and validate the typed payload.
    /// For mutations, require an outer key and reject disagreement with payload replay metadata.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0 || self.deadline_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        if self.request.is_mutating() {
            let key = self
                .idempotency_key
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?;
            if let Some(mutation) = self.request.mutation()
                && (&mutation.idempotency_key != key || mutation.sequence != self.sequence)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        self.request.validate(limits)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Sequence-bound reply that separates successful data from typed host failures.
pub struct EffectResult {
    /// Nonzero effect sequence used to correlate and validate replay metadata.
    pub sequence: u64,
    /// Successful typed reply or failure; uncertainty is preserved as `UnknownOutcome`.
    pub outcome: Result<HostResult, HostProblem>,
}

impl EffectResult {
    /// Require the expected nonzero sequence and validate either reply data or structured failure
    /// fields.
    pub fn validate(&self, expected_sequence: u64, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0 || self.sequence != expected_sequence {
            return Err(HostProblem::Malformed);
        }
        match &self.outcome {
            Ok(result) => result.validate(limits)?,
            Err(problem) => problem.validate(limits)?,
        }
        Ok(())
    }
}

fn validate_dataset(request: &DatasetRequest, limits: HostLimits) -> Result<(), HostProblem> {
    match request {
        DatasetRequest::Capabilities
        | DatasetRequest::Describe { .. }
        | DatasetRequest::Diagnose { .. }
        | DatasetRequest::ResolveCatalog { .. } => Ok(()),
        DatasetRequest::ListCatalog {
            pattern, max_items, ..
        } if pattern.len() > limits.max_name_bytes
            || *max_items == 0
            || *max_items as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListVolumes { start, max_items }
            if *max_items == 0
                || *max_items as usize > limits.max_records
                || start.as_ref().is_some_and(|start| {
                    start.is_empty() || start.len() > limits.max_name_bytes
                }) =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListLocks {
            now_tick,
            max_items,
            ..
        } => {
            if *now_tick == 0 {
                Err(HostProblem::Malformed)
            } else if *max_items == 0 || *max_items as usize > limits.max_records {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        DatasetRequest::TvsStatus { transaction, .. } => validate_transaction(transaction, limits),
        DatasetRequest::List {
            max_items, pattern, ..
        } if *max_items == 0
            || *max_items as usize > limits.max_records
            || pattern.len() > limits.max_name_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListMembers { max_items, .. }
            if *max_items == 0 || *max_items as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadMemberGeneration { relative, .. } if *relative > 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::ReadMemberGeneration { max_records, .. }
            if *max_records == 0 || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Read {
            key, max_records, ..
        } if *max_records == 0
            || *max_records as usize > limits.max_records
            || key
                .as_ref()
                .is_some_and(|value| value.len() > limits.max_record_bytes) =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadGeneric {
            key_prefix,
            max_records,
            ..
        } if key_prefix.is_empty()
            || key_prefix.len() > limits.max_record_bytes
            || *max_records == 0
            || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadConcatenation {
            datasets,
            max_records,
            ..
        } if datasets.is_empty()
            || datasets.len() > limits.max_records
            || *max_records == 0
            || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadRelative { record_number, .. } if *record_number == 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::ReadRba { max_bytes, .. }
            if *max_bytes == 0 || *max_bytes as usize > limits.max_record_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadSequential { max_records, .. }
            if *max_records == 0 || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Snapshot {
            max_records,
            max_members,
            ..
        } if *max_records == 0
            || *max_members == 0
            || *max_records as usize > limits.max_records
            || *max_members as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Create {
            attributes,
            mutation,
            ..
        } => {
            attributes.validate(limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Define {
            definition,
            mutation,
            ..
        }
        | DatasetRequest::Alter {
            definition,
            mutation,
            ..
        } => {
            definition.validate(
                limits,
                DatasetProviderCapabilities::all_contract_capabilities(),
            )?;
            mutation.validate(limits)
        }
        DatasetRequest::SetLifecycle { mutation, .. }
        | DatasetRequest::RecordBackup { mutation, .. } => mutation.validate(limits),
        DatasetRequest::Restore {
            snapshot, mutation, ..
        } => {
            validate_dataset_snapshot(snapshot, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::DefineCatalog { mutation, .. }
        | DatasetRequest::SetCatalogConnection { mutation, .. }
        | DatasetRequest::DefineAlias { mutation, .. }
        | DatasetRequest::DefineMemberAlias { mutation, .. } => mutation.validate(limits),
        DatasetRequest::WriteMemberGeneration {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::DeleteMemberGeneration {
            generation,
            mutation,
            ..
        } => {
            if *generation == 0 {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::AcquireLock {
            target,
            now_tick,
            lease_ticks,
            transaction,
            mutation,
            ..
        } => {
            if *now_tick == 0
                || *lease_ticks == 0
                || matches!(target, DatasetLockTarget::Record(identity) if identity.is_empty())
            {
                return Err(HostProblem::Malformed);
            }
            if matches!(target, DatasetLockTarget::Record(identity) if identity.len() > limits.max_record_bytes)
            {
                return Err(HostProblem::ResourceExhausted);
            }
            if let Some(transaction) = transaction {
                validate_transaction(transaction, limits)?;
            }
            mutation.validate(limits)
        }
        DatasetRequest::ReleaseLock {
            lock_id, mutation, ..
        } => {
            if lock_id.is_empty() || lock_id.len() > limits.max_name_bytes {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::BeginTvs {
            transaction,
            mutation,
            ..
        }
        | DatasetRequest::CompleteTvs {
            transaction,
            mutation,
            ..
        }
        | DatasetRequest::ReconcileTvs {
            transaction,
            mutation,
            ..
        } => {
            validate_transaction(transaction, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::StageTvs {
            transaction,
            operation,
            mutation,
            ..
        } => {
            validate_transaction(transaction, limits)?;
            validate_tvs_operation(operation, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Write {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Append {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Truncate { mutation, .. } => mutation.validate(limits),
        DatasetRequest::RewriteRecord {
            key,
            record,
            mutation,
            ..
        } if key.len() > limits.max_record_bytes || record.len() > limits.max_record_bytes => {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::RewriteRecord { mutation, .. } => mutation.validate(limits),
        DatasetRequest::DeleteRecord { key, mutation, .. } => {
            if key.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DefineAlternateIndex {
            key_offset,
            key_length,
            mutation,
            ..
        } => {
            if *key_length == 0
                || key_offset
                    .checked_add(*key_length)
                    .is_none_or(|end| end as usize > limits.max_record_bytes)
            {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::BuildAlternateIndex { mutation, .. } => mutation.validate(limits),
        DatasetRequest::DefinePath { mutation, .. } => mutation.validate(limits),
        DatasetRequest::WriteRelative {
            record_number,
            record,
            mutation,
            ..
        } => {
            if *record_number == 0 {
                Err(HostProblem::Malformed)
            } else if record.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DeleteRelative {
            record_number,
            mutation,
            ..
        } => {
            if *record_number == 0 {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::WriteRba { data, mutation, .. } => {
            if data.is_empty() {
                Err(HostProblem::Malformed)
            } else if data.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DefineGenerationGroup {
            limit, mutation, ..
        } => {
            if *limit == 0 || *limit as usize > limits.max_records {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::CreateGeneration {
            attributes,
            records,
            mutation,
            ..
        } => {
            attributes.validate(limits)?;
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::ResolveGeneration { relative, .. } if *relative > 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::Rename { mutation, .. } => mutation.validate(limits),
        DatasetRequest::Delete {
            current_date,
            mutation,
            ..
        } => {
            if current_date.is_some_and(|date| !valid_julian_date(date)) {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        // Browse key and cursor bounds share one validator.
        DatasetRequest::StartBrowse { .. }
        | DatasetRequest::ResetBrowse { .. }
        | DatasetRequest::ReadNext { .. }
        | DatasetRequest::ReadBrowsePosition { .. }
        | DatasetRequest::EndBrowse { .. } => browse::validate(request, limits),
        DatasetRequest::Close {
            cursor, control, ..
        } if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > limits.max_name_bytes)
            || (control.lock
                && (control.reel_or_unit.is_some() || control.no_rewind || control.removal))
            || (control.removal && control.reel_or_unit.is_none()) =>
        {
            Err(HostProblem::Malformed)
        }
        _ => Ok(()),
    }
}

fn validate_records(records: &[Vec<u8>], limits: HostLimits) -> Result<(), HostProblem> {
    if records.len() > limits.max_records
        || records
            .iter()
            .any(|record| record.len() > limits.max_record_bytes)
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn validate_spool_file(file: &str, limits: HostLimits) -> Result<(), HostProblem> {
    if file.is_empty()
        || file.len() > limits.max_name_bytes
        || file
            .chars()
            .any(|character| character.is_control() || !character.is_ascii())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_dataset_snapshot(
    snapshot: &DatasetSnapshot,
    limits: HostLimits,
) -> Result<(), HostProblem> {
    snapshot.definition.validate(
        limits,
        DatasetProviderCapabilities::all_contract_capabilities(),
    )?;
    validate_records(&snapshot.records, limits)?;
    if snapshot.relative_records.len() > limits.max_records
        || snapshot.members.len() > limits.max_records
        || snapshot.linear_data.len() > limits.max_state_bytes
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut previous_rrn = 0u64;
    for relative in &snapshot.relative_records {
        if relative.record_number == 0 || relative.record_number <= previous_rrn {
            return Err(HostProblem::Malformed);
        }
        previous_rrn = relative.record_number;
        validate_records(std::slice::from_ref(&relative.record), limits)?;
    }
    let mut total = snapshot
        .records
        .len()
        .checked_add(snapshot.relative_records.len())
        .ok_or(HostProblem::ResourceExhausted)?;
    let mut total_bytes = snapshot
        .records
        .iter()
        .chain(
            snapshot
                .relative_records
                .iter()
                .map(|relative| &relative.record),
        )
        .try_fold(snapshot.linear_data.len(), |total, record| {
            total
                .checked_add(record.len())
                .ok_or(HostProblem::ResourceExhausted)
        })?;
    let mut previous_member = None;
    for member in &snapshot.members {
        if previous_member.is_some_and(|previous: &str| previous >= member.name.as_str()) {
            return Err(HostProblem::Malformed);
        }
        previous_member = Some(member.name.as_str());
        validate_records(&member.records, limits)?;
        if member.generations.len() > limits.max_records
            || member.alias_of.is_some()
                && (!member.records.is_empty() || !member.generations.is_empty())
        {
            return Err(HostProblem::Malformed);
        }
        total = total
            .checked_add(member.records.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        total_bytes = member
            .records
            .iter()
            .try_fold(total_bytes, |total, record| {
                total
                    .checked_add(record.len())
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
        let mut previous = 0u64;
        for generation in &member.generations {
            if generation.generation == 0 || generation.generation <= previous {
                return Err(HostProblem::Malformed);
            }
            previous = generation.generation;
            validate_records(&generation.records, limits)?;
            total = total
                .checked_add(generation.records.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            total_bytes = generation
                .records
                .iter()
                .try_fold(total_bytes, |total, record| {
                    total
                        .checked_add(record.len())
                        .ok_or(HostProblem::ResourceExhausted)
                })?;
        }
    }
    let shape_valid = match snapshot.definition.attributes.organization {
        DatasetOrganization::Sequential
        | DatasetOrganization::KeySequenced
        | DatasetOrganization::EntrySequenced => {
            snapshot.relative_records.is_empty()
                && snapshot.members.is_empty()
                && snapshot.linear_data.is_empty()
        }
        DatasetOrganization::Relative | DatasetOrganization::VariableRelative => {
            snapshot.records.is_empty()
                && snapshot.members.is_empty()
                && snapshot.linear_data.is_empty()
        }
        DatasetOrganization::Partitioned => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.linear_data.is_empty()
                && snapshot
                    .members
                    .iter()
                    .all(|member| member.alias_of.is_none() && member.generations.is_empty())
        }
        DatasetOrganization::PartitionedExtended => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.linear_data.is_empty()
                && snapshot.members.iter().all(|member| {
                    member.records.is_empty()
                        && member.alias_of.as_ref().map_or(
                            !member.generations.is_empty(),
                            |target| {
                                target != &member.name
                                    && snapshot.members.iter().any(|candidate| {
                                        candidate.name == *target && candidate.alias_of.is_none()
                                    })
                            },
                        )
                })
        }
        DatasetOrganization::Linear => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.members.is_empty()
        }
    };
    if !shape_valid {
        Err(HostProblem::Malformed)
    } else if total > limits.max_records || total_bytes > limits.max_state_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}
fn validate_transaction(transaction: &str, limits: HostLimits) -> Result<(), HostProblem> {
    if transaction.is_empty()
        || transaction.len() > limits.max_name_bytes
        || transaction.chars().any(char::is_control)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}
fn valid_julian_date(date: u32) -> bool {
    let year = date / 1000;
    let day = date % 1000;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    (1900..=9999).contains(&year) && day != 0 && day <= if leap { 366 } else { 365 }
}
fn validate_tvs_operation(
    operation: &TvsRecordOperation,
    limits: HostLimits,
) -> Result<(), HostProblem> {
    match operation {
        TvsRecordOperation::Insert { record, .. } => {
            if record.is_empty() {
                Err(HostProblem::Malformed)
            } else if record.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        TvsRecordOperation::Rewrite { key, record, .. } => {
            if key.is_empty() || record.is_empty() {
                Err(HostProblem::Malformed)
            } else if key.len() > limits.max_record_bytes || record.len() > limits.max_record_bytes
            {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        TvsRecordOperation::Delete { key, .. } => {
            if key.is_empty() {
                Err(HostProblem::Malformed)
            } else if key.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
    }
}
fn validate_fields(fields: &[TerminalField], limits: HostLimits) -> Result<(), HostProblem> {
    if fields.len() > limits.max_fields
        || fields.iter().any(|field| {
            field.name.is_empty()
                || field.name.len() > limits.max_name_bytes
                || field.value.len() > limits.max_record_bytes
                || field.value.len() > usize::from(field.length)
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed rejection, failure or uncertainty at the host boundary.
/// `UnknownOutcome` requires reconciliation rather than assuming the effect did not occur.
pub enum HostProblem {
    /// The envelope or payload violates its structural contract.
    Malformed,
    /// No supported implementation is available for the request.
    Unsupported,
    /// A specific provider capability is unavailable, with explanatory detail.
    UnsupportedCapability {
        /// Bounded nonempty identity of the unsupported capability.
        capability: String,
        /// Nonempty explanatory text bounded by `max_state_bytes`.
        detail: String,
    },
    /// The requested resource was not found.
    NotFound,
    /// A named subsystem condition with primary and secondary response codes.
    Condition {
        /// Nonempty condition name bounded by `max_name_bytes`.
        name: String,
        /// Primary signed subsystem response code.
        response: i32,
        /// Secondary signed subsystem response code.
        response2: i32,
    },
    /// The invocation lacks the required authority.
    Unauthorized,
    /// Cancellation prevented the requested operation.
    Cancelled,
    /// The logical deadline was reached.
    TimedOut,
    /// A request, reply or provider resource exceeds an admitted bound.
    ResourceExhausted,
    /// The selected provider is unavailable or fails its contract.
    ProviderFailure,
    /// Host infrastructure failed while dispatching the effect.
    InfrastructureFailure,
    /// A replay-protected operation has no required idempotency identity.
    MissingIdempotency,
    /// Replay metadata disagrees with an existing identity or outer envelope.
    IdempotencyConflict,
    /// Whether the effect took place cannot be established.
    /// Reconcile durable state before deciding whether a retry is safe.
    UnknownOutcome,
}
impl fmt::Display for HostProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host service failed: {self:?}")
    }
}
impl std::error::Error for HostProblem {}

impl HostProblem {
    fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::UnsupportedCapability { capability, detail }
                if capability.is_empty()
                    || capability.len() > limits.max_name_bytes
                    || detail.is_empty()
                    || detail.len() > limits.max_state_bytes =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Condition { name, .. }
                if name.is_empty() || name.len() > limits.max_name_bytes =>
            {
                Err(HostProblem::Malformed)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DatasetName;
    use std::collections::BTreeMap;

    #[test]
    fn mutating_effect_requires_idempotency() {
        let invocation = InvocationLimits::default();
        let host = HostLimits::default();
        let request = EffectRequest {
            run_unit: RunUnitId::new("run-1", invocation).unwrap(),
            sequence: 1,
            deadline_tick: 1,
            idempotency_key: None,
            request: HostRequest::State(StateRequest::Delete {
                key: "x".into(),
                expected_version: None,
                mutation: Mutation {
                    sequence: 1,
                    idempotency_key: IdempotencyKey::new("idem-1", invocation).unwrap(),
                    transaction: None,
                },
            }),
        };
        assert_eq!(request.validate(host), Err(HostProblem::MissingIdempotency));
    }

    #[test]
    fn token_producing_read_requires_mutation_replay_identity() {
        let token_target = BoundedPayload::new(
            "mainframe-env.cics.argument@1",
            b"TOKEN-X".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let token_read = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments: BTreeMap::from([("TOKEN".into(), token_target)]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: None,
        });
        assert!(token_read.is_mutating());
        assert_eq!(
            token_read.validate(HostLimits::default()),
            Err(HostProblem::MissingIdempotency)
        );
        let plain_read = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments: BTreeMap::new(),
            condition_policy: CicsConditionPolicy::Default,
            mutation: None,
        });
        assert!(!plain_read.is_mutating());
    }

    #[test]
    fn records_are_bounded() {
        let invocation = InvocationLimits::default();
        let limits = HostLimits {
            max_record_bytes: 1,
            ..HostLimits::default()
        };
        let mutation = Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("i", invocation).unwrap(),
            transaction: None,
        };
        let request = HostRequest::Dataset(DatasetRequest::Write {
            dataset: DatasetName::new("USER.DATA", 44).unwrap(),
            member: None,
            records: vec![vec![1, 2]],
            expected_version: None,
            mutation,
        });
        assert_eq!(
            request.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
    }

    #[test]
    fn close_control_rejects_conflicting_lock_and_reel_dispositions() {
        let dataset = DatasetName::new("USER.DATA", 44).unwrap();
        let valid = HostRequest::Dataset(DatasetRequest::Close {
            dataset: dataset.clone(),
            cursor: None,
            control: DatasetCloseControl {
                reel_or_unit: Some(DatasetReelUnit::Reel),
                no_rewind: true,
                ..DatasetCloseControl::default()
            },
        });
        assert_eq!(valid.validate(HostLimits::default()), Ok(()));

        let invalid = HostRequest::Dataset(DatasetRequest::Close {
            dataset,
            cursor: None,
            control: DatasetCloseControl {
                reel_or_unit: Some(DatasetReelUnit::Unit),
                lock: true,
                ..DatasetCloseControl::default()
            },
        });
        assert_eq!(
            invalid.validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn all_cics_runtime_operation_names_are_unique() {
        let forms = [
            CicsOperation::Abend,
            CicsOperation::AddSubevent,
            CicsOperation::Address,
            CicsOperation::AddressSet,
            CicsOperation::Asktime,
            CicsOperation::BifDeedit,
            CicsOperation::BifDigest,
            CicsOperation::AsktimeEib,
            CicsOperation::Assign,
            CicsOperation::ChangeTask,
            CicsOperation::Post,
            CicsOperation::WriteOperator,
            CicsOperation::ExtractCertificate,
            CicsOperation::ExtractTcpip,
            CicsOperation::Deq,
            CicsOperation::Delete,
            CicsOperation::DefineInputEvent,
            CicsOperation::DefineCompositeEvent,
            CicsOperation::DocumentCreate,
            CicsOperation::DocumentDelete,
            CicsOperation::DocumentInsert,
            CicsOperation::DocumentRetrieve,
            CicsOperation::DocumentSet,
            CicsOperation::DeleteTransientData,
            CicsOperation::DeleteTemporaryStorage,
            CicsOperation::ReadTemporaryStorage,
            CicsOperation::WriteTemporaryStorage,
            CicsOperation::Enq,
            CicsOperation::EndBrowse,
            CicsOperation::FormatTime,
            CicsOperation::ConvertTime,
            CicsOperation::Freemain,
            CicsOperation::Freemain64,
            CicsOperation::Getmain,
            CicsOperation::Getmain64,
            CicsOperation::HandleAbend,
            CicsOperation::HandleAid,
            CicsOperation::HandleCondition,
            CicsOperation::IgnoreCondition,
            CicsOperation::Inquire,
            CicsOperation::InvokeApplication,
            CicsOperation::Load,
            CicsOperation::Release,
            CicsOperation::Link,
            CicsOperation::PopHandle,
            CicsOperation::PushHandle,
            CicsOperation::PurgeMessage,
            CicsOperation::Read,
            CicsOperation::ReadNext,
            CicsOperation::ReadPrev,
            CicsOperation::ResetBrowse,
            CicsOperation::ReadTransientData,
            CicsOperation::RemoveSubevent,
            CicsOperation::DeleteEvent,
            CicsOperation::CheckTimer,
            CicsOperation::DefineTimer,
            CicsOperation::DeleteTimer,
            CicsOperation::ForceTimer,
            CicsOperation::RetrieveReattachEvent,
            CicsOperation::RetrieveSubevent,
            CicsOperation::TestEvent,
            CicsOperation::SignalEvent,
            CicsOperation::DefineCounter,
            CicsOperation::DefineDCounter,
            CicsOperation::DeleteCounter,
            CicsOperation::DeleteDCounter,
            CicsOperation::GetCounter,
            CicsOperation::GetDCounter,
            CicsOperation::QueryCounter,
            CicsOperation::QueryDCounter,
            CicsOperation::RewindCounter,
            CicsOperation::RewindDCounter,
            CicsOperation::UpdateCounter,
            CicsOperation::UpdateDCounter,
            CicsOperation::ReceiveMap,
            CicsOperation::ReceivePartn,
            CicsOperation::Retrieve,
            CicsOperation::Return,
            CicsOperation::Rewrite,
            CicsOperation::SendText,
            CicsOperation::SendMap,
            CicsOperation::SendControl,
            CicsOperation::SendPage,
            CicsOperation::SendPartnset,
            CicsOperation::SetAssociationUserCorrData,
            CicsOperation::SetFileStatus,
            CicsOperation::SpoolClose,
            CicsOperation::SpoolOpenInput,
            CicsOperation::SpoolOpenOutput,
            CicsOperation::SpoolRead,
            CicsOperation::SpoolWrite,
            CicsOperation::Start,
            CicsOperation::StartBrowse,
            CicsOperation::StartAttach,
            CicsOperation::Suspend,
            CicsOperation::WaitEvent,
            CicsOperation::WaitExternal,
            CicsOperation::WaitCics,
            CicsOperation::Syncpoint,
            CicsOperation::InvokeService,
            CicsOperation::SoapFaultAdd,
            CicsOperation::SoapFaultCreate,
            CicsOperation::SoapFaultDelete,
            CicsOperation::WsaContextBuild,
            CicsOperation::WsaContextDelete,
            CicsOperation::WsaContextGet,
            CicsOperation::WsaEprCreate,
            CicsOperation::TransformDataToJson,
            CicsOperation::TransformDataToXml,
            CicsOperation::TransformJsonToData,
            CicsOperation::TransformXmlToData,
            CicsOperation::WebParseUrl,
            CicsOperation::WebOpen,
            CicsOperation::WebClose,
            CicsOperation::WebExtract,
            CicsOperation::ExtractWeb,
            CicsOperation::WebRead,
            CicsOperation::WebStartBrowse,
            CicsOperation::WebReadNext,
            CicsOperation::WebEndBrowse,
            CicsOperation::WebWrite,
            CicsOperation::WebSend,
            CicsOperation::WebRetrieve,
            CicsOperation::WebReceive,
            CicsOperation::WebConverse,
            CicsOperation::Unlock,
            CicsOperation::Write,
            CicsOperation::WriteTransientData,
            CicsOperation::Xctl,
        ];
        assert_eq!(forms.len(), 129);
        let names = forms
            .iter()
            .map(|operation| operation.runtime_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), forms.len());
    }
}
