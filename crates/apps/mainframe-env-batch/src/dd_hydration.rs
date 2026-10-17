use crate::DdPlan;

pub(crate) fn is_program_library_dd(dd: &DdPlan) -> bool {
    dd.name.eq_ignore_ascii_case("STEPLIB")
        || dd.name.eq_ignore_ascii_case("JOBLIB")
        || dd.name.eq_ignore_ascii_case("DBRMLIB")
        || dd.name.eq_ignore_ascii_case("DFSRESLB")
        || dd.name.eq_ignore_ascii_case("IMS")
        || dd.name.eq_ignore_ascii_case("DFSVSAMP")
        || dd.name.eq_ignore_ascii_case("PROCLIB")
        || dd.name.eq_ignore_ascii_case("DFSSEL")
        || dd.name.to_ascii_uppercase().starts_with("DDPAUT")
}

/// True for DD names whose flattened `DdPlan::inline_data` bytes are still
/// read after `hydrate_dds` runs, so the flattened copy must be kept
/// alongside `dd_records` instead of relying on `dd_records` alone. Every
/// other dataset-backed DD carries its hydrated records exactly once, in
/// `dd_records` (#182). Known post-hydration readers of `inline_data`:
/// - `mainframe-env-server/src/cobol.rs` (~501): `SYSIN` becomes COBOL
///   terminal input (`cobol.terminal.input`).
/// - `mainframe-env-server/src/cobol.rs` `source_bundle`: `SYSIN` is the
///   compile-on-run primary source, and every `SYSLIB*` DD's `inline_data`
///   is a copybook file.
/// - `mainframe-env-batch/src/program.rs` (~361-366): the IDCAMS builtin
///   reads control statements from `SYSIN`.
pub(crate) fn retains_flattened_dataset_bytes(name: &str) -> bool {
    name.eq_ignore_ascii_case("SYSIN") || name.to_ascii_uppercase().starts_with("SYSLIB")
}

// Per-effect host admission remains4096. The aggregate ceiling is the unchanged
// default dataset-provider record ceiling, additionally fenced by run storage.
const PAGE_RECORDS: u32 = 4_096;
const MAX_DATASET_RECORDS: usize = 65_536;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CompleteDatasetRead {
    Records(Vec<Vec<u8>>),
    MissingMember,
}

impl CompleteDatasetRead {
    pub(crate) fn into_records(self) -> Result<Vec<Vec<u8>>, mainframe_env_host_api::HostProblem> {
        match self {
            Self::Records(records) => Ok(records),
            Self::MissingMember => Err(mainframe_env_host_api::HostProblem::NotFound),
        }
    }
}

/// Own one complete ordinal read, never return a silently shortened prefix.
/// Continuation is allowed only while the caller owns the original DD lock.
/// ReadSequential allocates no provider browse cursor and provides no `more`.
pub(crate) fn complete_dataset_records(
    dataset: &mainframe_env_host_api::DatasetName,
    member: Option<&mainframe_env_host_api::MemberName>,
    owns_dd_lock: bool,
    storage_bytes: u64,
    mut check: impl FnMut() -> Result<(), mainframe_env_host_api::HostProblem>,
    mut invoke: impl FnMut(
        mainframe_env_host_api::DatasetRequest,
    ) -> Result<
        mainframe_env_host_api::DatasetResult,
        mainframe_env_host_api::HostProblem,
    >,
) -> Result<CompleteDatasetRead, mainframe_env_host_api::HostProblem> {
    use mainframe_env_host_api::{DatasetRequest, DatasetResult, HostProblem};
    check()?;
    let DatasetResult::Attributes {
        attributes,
        version,
    } = invoke(DatasetRequest::Attributes {
        dataset: dataset.clone(),
    })?
    else {
        return Err(HostProblem::ProviderFailure);
    };
    let mut records = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    let mut payload_bytes = 0u64;
    let mut record_bytes = 0u64;
    let mut previous_identity: Option<Vec<u8>> = None;
    loop {
        check()?;
        let start = u64::try_from(records.len()).map_err(|_| HostProblem::ResourceExhausted)?;
        let page = invoke(DatasetRequest::ReadSequential {
            dataset: dataset.clone(),
            member: member.cloned(),
            start: Some(start),
            reverse: false,
            max_records: PAGE_RECORDS,
        });
        let (page, page_ids, page_version) = match page {
            Ok(DatasetResult::Records {
                records,
                identities,
                version,
            }) => (records, identities, version),
            // A full previous page requires an explicit terminal probe. A missing
            // dataset, first-page NOTFND, or different condition is never EOF.
            Err(HostProblem::Condition {
                name,
                response: 13,
                response2: 0,
            }) if name == "NOTFND" && start > 0 => break,
            Err(HostProblem::NotFound)
                if start == 0
                    && member.is_some()
                    && matches!(
                        attributes.organization,
                        mainframe_env_host_api::DatasetOrganization::Partitioned
                            | mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                    ) =>
            {
                // The initial Attributes succeeded. Recheck the same dataset
                // before classifying a plain provider NotFound as member absence.
                // Dataset disappearance and every other error remain errors.
                check()?;
                let DatasetResult::Attributes {
                    attributes: current,
                    version: current_version,
                } = invoke(DatasetRequest::Attributes {
                    dataset: dataset.clone(),
                })?
                else {
                    return Err(HostProblem::ProviderFailure);
                };
                if current != attributes || current_version != version {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(CompleteDatasetRead::MissingMember);
            }
            Err(problem) => return Err(problem),
            _ => return Err(HostProblem::ProviderFailure),
        };
        if page_version != version {
            return Err(HostProblem::IdempotencyConflict);
        }
        if page.len() > PAGE_RECORDS as usize || page.len() != page_ids.len() {
            return Err(HostProblem::ProviderFailure);
        }
        if records
            .len()
            .checked_add(page.len())
            .is_none_or(|n| n > MAX_DATASET_RECORDS)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let count = page.len();
        for (position, (record, identity)) in page.into_iter().zip(page_ids).enumerate() {
            validate_page_identity(
                &attributes,
                &record,
                &identity,
                start + position as u64,
                record_bytes,
                previous_identity.as_deref(),
            )?;
            if !identities.insert(identity.clone()) {
                return Err(HostProblem::ProviderFailure);
            }
            let added = u64::try_from(record.len())
                .ok()
                .and_then(|n| n.checked_add(identity.len() as u64))
                .and_then(|n| n.checked_add(std::mem::size_of::<Vec<u8>>() as u64))
                .ok_or(HostProblem::ResourceExhausted)?;
            payload_bytes = payload_bytes
                .checked_add(added)
                .ok_or(HostProblem::ResourceExhausted)?;
            if payload_bytes > storage_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            record_bytes = record_bytes
                .checked_add(record.len() as u64)
                .ok_or(HostProblem::ResourceExhausted)?;
            previous_identity = Some(identity);
            records.push(record);
        }
        if count < PAGE_RECORDS as usize {
            break;
        }
        // Without the held DD lock, a same-version delete/recreate cannot be
        // ruled out across pages. Saturation therefore refuses, never guesses.
        if !owns_dd_lock {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    check()?;
    let DatasetResult::Attributes {
        attributes: current,
        version: current_version,
    } = invoke(DatasetRequest::Attributes {
        dataset: dataset.clone(),
    })?
    else {
        return Err(HostProblem::ProviderFailure);
    };
    if current != attributes || current_version != version {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(CompleteDatasetRead::Records(records))
}

fn validate_page_identity(
    attributes: &mainframe_env_host_api::DatasetAttributes,
    record: &[u8],
    identity: &[u8],
    ordinal: u64,
    byte_position: u64,
    previous: Option<&[u8]>,
) -> Result<(), mainframe_env_host_api::HostProblem> {
    use mainframe_env_host_api::{DatasetOrganization as O, HostProblem};
    let valid = match attributes.organization {
        O::Sequential | O::Partitioned | O::PartitionedExtended => {
            identity == ordinal.to_be_bytes()
        }
        O::EntrySequenced => identity == byte_position.to_be_bytes(),
        O::KeySequenced => {
            attributes
                .key_offset
                .zip(attributes.key_length)
                .and_then(|(offset, length)| {
                    let end = offset.checked_add(length)?;
                    record.get(offset as usize..end as usize)
                })
                .is_some_and(|key| key == identity)
                && previous.is_none_or(|previous| previous < identity)
        }
        O::Relative | O::VariableRelative => {
            identity.len() == 8
                && identity != 0u64.to_be_bytes()
                && previous.is_none_or(|previous| previous < identity)
        }
        O::Linear => false,
    };
    if valid {
        Ok(())
    } else {
        Err(HostProblem::ProviderFailure)
    }
}

/// Original single-output transport capacity is not permission to discard
/// data. Explicit COUNT retains its original bounded selection semantics.
pub(crate) fn refuse_implicit_repro_prefix(
    records: usize,
    skip: usize,
    count: usize,
    explicit_count: bool,
) -> Result<(), mainframe_env_host_api::HostProblem> {
    if !explicit_count && records.saturating_sub(skip) > count {
        Err(mainframe_env_host_api::HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

pub(crate) fn check_hydration_storage(
    dds: &[crate::DdPlan],
    records: &std::collections::BTreeMap<String, Vec<Vec<u8>>>,
    storage_bytes: u64,
) -> Result<(), mainframe_env_host_api::HostProblem> {
    use mainframe_env_host_api::HostProblem;
    let mut bytes = 0u64;
    for record in records.values().flatten() {
        bytes = bytes
            .checked_add(record.len() as u64)
            .and_then(|n| n.checked_add(std::mem::size_of::<Vec<u8>>() as u64))
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    for dd in dds {
        bytes = bytes
            .checked_add(dd.inline_data.len() as u64)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if bytes > storage_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}
