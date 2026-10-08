//! Host result validation retains the existing wire-contract authority.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed host observations validated for size and shape; result/request binding belongs to the dispatch boundary.
pub enum HostResult {
    /// Dataset-owned operation or observation.
    Dataset(DatasetResult),
    /// Program-routing operation or owned output payload.
    Program(BoundedPayload),
    /// Job-scoped spool operation or observation.
    Spool(SpoolResult),
    /// Session-scoped terminal operation or owned output payload.
    Terminal(BoundedPayload),
    /// Installed security authority request or decision.
    Security(SecurityDecision),
    /// Explicit clock request or bounded textual observation.
    Clock(String),
    /// Versioned host state request or observation.
    State {
        /// Optional observed state bytes; None retains the absence observation.
        value: Option<Vec<u8>>,
        /// Provider-reported state revision; validation here does not establish request CAS equality.
        version: u64,
    },
    /// Typed CICS observation retaining application dispositions.
    Cics(CicsResponse),
    /// Bounded SQL operation or completion data.
    Db2(Db2Result),
    /// Bounded IMS operation or status/data observation.
    Ims(ImsResult),
    /// Separate I/O PCB recovery response; it does not update a database PCB.
    ImsRecovery(crate::ImsRecoveryResult),
    /// GSAM saved-address output alongside the unchanged IMS status/data result.
    ImsGsam(crate::ImsGsamResult),
    /// Versioned owned feedback from the selected database PCB proposal.
    ImsPcbFeedbackV1(crate::ImsPcbFeedbackResultV1),
    /// Legacy MQ request or observation; typed MQI uses its separate contract.
    Mq(MqResult),
    /// Source-bound result shape with explicit limits, not execution authority.
    MqMqi(Box<MqMqiHostResult>),
}

impl HostResult {
    /// Check represented result bounds and internal shape; this does not bind the result to a particular request or establish success.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::MqMqi(result) => result.validate(limits),
            Self::ImsRecovery(result) => result.validate(),
            Self::Dataset(DatasetResult::Description(description)) => {
                description.definition.validate(
                    limits,
                    DatasetProviderCapabilities::all_contract_capabilities(),
                )?;
                if description.extents.is_empty()
                    || description.extents.len() > limits.max_records
                    || description.buffer_bytes == 0
                    || description.abstract_placement.is_empty()
                    || description.abstract_placement.len() > limits.max_name_bytes
                {
                    return Err(HostProblem::Malformed);
                }
                let mut next_start = 0u64;
                for (position, extent) in description.extents.iter().enumerate() {
                    if extent.ordinal != u32::try_from(position).unwrap_or(u32::MAX)
                        || extent.start != next_start
                        || extent.length == 0
                        || extent.volume_id.is_empty()
                        || extent.volume_id.len() > limits.max_name_bytes
                    {
                        return Err(HostProblem::Malformed);
                    }
                    next_start = next_start
                        .checked_add(extent.length)
                        .ok_or(HostProblem::ResourceExhausted)?;
                }
                if next_start != description.allocated_bytes
                    || description.high_used_rba > description.max_rba
                {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                }
            }
            Self::Dataset(DatasetResult::Catalog(resolution))
                if resolution.alias_chain.len() > limits.max_records || resolution.version == 0 =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::CatalogEntries { entries, .. })
                if entries.len() > limits.max_records
                    || entries.iter().any(|entry| entry.version == 0) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Volumes { volumes, .. }) => {
                if volumes.len() > limits.max_records {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut previous_volume = None;
                for volume in volumes {
                    if volume.volume_id.is_empty()
                        || volume.volume_id.len() > limits.max_name_bytes
                        || previous_volume
                            .is_some_and(|previous: &str| previous >= volume.volume_id.as_str())
                        || volume.extents.is_empty()
                        || volume.extents.len() > limits.max_records
                        || volume.used_bytes > volume.allocated_bytes
                    {
                        return Err(HostProblem::Malformed);
                    }
                    previous_volume = Some(&volume.volume_id);
                    let mut next_start = 0u64;
                    for extent in &volume.extents {
                        if extent.volume_start != next_start || extent.length == 0 {
                            return Err(HostProblem::Malformed);
                        }
                        next_start = next_start
                            .checked_add(extent.length)
                            .ok_or(HostProblem::ResourceExhausted)?;
                    }
                    if next_start != volume.allocated_bytes {
                        return Err(HostProblem::Malformed);
                    }
                }
                Ok(())
            }
            Self::Dataset(DatasetResult::Locks { locks })
                if locks.len() > limits.max_records
                    || locks.iter().any(|lock| {
                        lock.lock_id.is_empty()
                            || lock.lock_id.len() > limits.max_name_bytes
                            || lock.expires_at == 0
                            || lock.version == 0
                            || matches!(
                                &lock.target,
                                DatasetLockTarget::Record(identity)
                                    if identity.is_empty()
                                        || identity.len() > limits.max_record_bytes
                            )
                            || lock.transaction.as_ref().is_some_and(|transaction| {
                                transaction.is_empty() || transaction.len() > limits.max_name_bytes
                            })
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Tvs(receipt))
                if receipt.transaction.is_empty()
                    || receipt.transaction.len() > limits.max_name_bytes
                    || receipt.version == 0 =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Snapshot { snapshot, version }) => {
                if *version == 0 {
                    Err(HostProblem::Malformed)
                } else {
                    validate_dataset_snapshot(snapshot, limits)
                }
            }
            Self::Dataset(DatasetResult::MemberGeneration { generation: 0, .. }) => {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Diagnostics { diagnostics })
                if diagnostics.len() > limits.max_records
                    || diagnostics.iter().any(|diagnostic| {
                        diagnostic.code.is_empty()
                            || diagnostic.code.len() > limits.max_name_bytes
                            || diagnostic
                                .field
                                .as_ref()
                                .is_some_and(|field| field.len() > limits.max_name_bytes)
                            || diagnostic.detail.len() > limits.max_state_bytes
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Listed { names, .. })
                if names.len() > limits.max_records =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Members { names, .. })
                if names.len() > limits.max_records =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Records {
                records,
                identities,
                ..
            })
            | Self::Dataset(DatasetResult::MemberGeneration {
                records,
                identities,
                ..
            }) => {
                validate_records(records, limits)?;
                if identities.len() != records.len()
                    || identities
                        .iter()
                        .any(|identity| identity.len() > limits.max_record_bytes)
                {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                }
            }
            Self::Dataset(DatasetResult::Rba {
                data,
                rba,
                next_rba,
                ..
            }) if data.len() > limits.max_record_bytes
                || *next_rba < *rba
                || next_rba.saturating_sub(*rba)
                    != u64::try_from(data.len()).unwrap_or(u64::MAX) =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Browse {
                record,
                identity,
                key,
                ..
            }) if record
                .as_ref()
                .is_some_and(|value| value.len() > limits.max_record_bytes)
                || identity
                    .as_ref()
                    .is_some_and(|value| value.len() > limits.max_record_bytes)
                || key
                    .as_ref()
                    .is_some_and(|value| value.len() > limits.max_record_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Browse {
                record,
                identity,
                key,
                ..
            }) if record.is_some() != identity.is_some() || record.is_some() != key.is_some() => {
                Err(HostProblem::Malformed)
            }
            Self::Spool(SpoolResult::Files { files })
                if files.len() > limits.max_records
                    || files.iter().any(|file| {
                        file.file.is_empty()
                            || file.file.len() > limits.max_name_bytes
                            || file.file.chars().any(char::is_control)
                            || usize::try_from(file.record_count)
                                .map_or(true, |count| count > limits.max_records)
                            || file.version == 0
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolResult::Records {
                records, version, ..
            }) if *version == 0
                || records.len() > limits.max_records
                || records
                    .iter()
                    .any(|record| record.len() > limits.max_record_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolResult::Mutated { version, .. }) if *version == 0 => {
                Err(HostProblem::Malformed)
            }
            Self::Spool(SpoolResult::PurgePending {
                remaining_artifacts,
            }) if *remaining_artifacts == 0 => Err(HostProblem::Malformed),
            Self::Program(payload) | Self::Terminal(payload)
                if payload.bytes().len() > limits.max_state_bytes =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Cics(response)
                if response.condition.len() > limits.max_name_bytes
                    || response.applid.len() > limits.max_name_bytes
                    || response.sysid.len() > limits.max_name_bytes
                    || response.transaction.len() > limits.max_name_bytes
                    || response
                        .target
                        .as_ref()
                        .is_some_and(|value| value.len() > limits.max_name_bytes)
                    || response
                        .next_transaction
                        .as_ref()
                        .is_some_and(|value| value.len() > limits.max_name_bytes)
                    || response.payload.bytes().len() > limits.max_state_bytes
                    || response.outputs.len() > limits.max_fields
                    || response
                        .outputs
                        .values()
                        .any(|value| value.bytes().len() > limits.max_state_bytes)
                    || response
                        .outputs
                        .values()
                        .try_fold(0usize, |total, value| {
                            total.checked_add(value.bytes().len())
                        })
                        .is_none_or(|total| total > limits.max_state_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Db2(result)
                if result.sqlstate.len() != 5
                    || result.message.len() > limits.max_state_bytes
                    || result.rows.len() > limits.max_records
                    || result.rows.iter().any(|row| {
                        row.columns.len() > limits.max_fields
                            || row
                                .columns
                                .iter()
                                .any(|column| column.len() > limits.max_record_bytes)
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Ims(result)
                if result.status.len() != 2
                    || result.segments.len() > limits.max_records
                    || result
                        .checkpoint_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > limits.max_name_bytes)
                    || result.segments.iter().any(|segment| {
                        segment.name.is_empty()
                            || segment.name.len() > limits.max_name_bytes
                            || segment.data.len() > limits.max_record_bytes
                            || segment
                                .parent_key
                                .as_ref()
                                .is_some_and(|key| key.len() > limits.max_record_bytes)
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::ImsGsam(result) => result.validate(limits),
            Self::ImsPcbFeedbackV1(result) => result.validate(limits),
            Self::Ims(result)
                if result
                    .system
                    .as_ref()
                    .is_some_and(|system| system.validate(limits).is_err()) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Mq(result) => result.validate(limits),
            Self::State {
                value: Some(value), ..
            } if value.len() > limits.max_state_bytes => Err(HostProblem::ResourceExhausted),
            Self::Clock(value) if value.len() > limits.max_name_bytes => {
                Err(HostProblem::ResourceExhausted)
            }
            _ => Ok(()),
        }
    }
}
