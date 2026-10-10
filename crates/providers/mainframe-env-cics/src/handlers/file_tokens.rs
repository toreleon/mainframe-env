//! Task-owned file update tokens with a durable, never-reused numeric allocator.

use super::super::{CicsService, Run, store_error};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::{CicsRequest, HostProblem};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-file-token-counter";
const KEY: &str = "global";

#[derive(Clone)]
pub(in crate::service) struct FileUpdateToken {
    pub dataset: String,
    pub identity: Vec<u8>,
    pub record: Vec<u8>,
    pub browse_cursor: Option<String>,
}

#[derive(Clone, Default)]
pub(in crate::service) struct FileUpdateState {
    pub(in crate::service) current_record_values: BTreeMap<String, Vec<u8>>,
    pub(in crate::service) file_tokens: BTreeMap<u32, FileUpdateToken>,
    /// Confirmed STARTBR owners, retained even when a later browse replaces the active cursor.
    pub(in crate::service) task_browses: BTreeMap<(String, String), FileBrowseOwner>,
}

#[derive(Clone)]
pub(in crate::service) struct FileBrowseOwner {
    pub(in crate::service) actor: Invocation,
    /// An unbound or uncertain retirement reply cannot authorize another END attempt.
    pub(in crate::service) retirement_unknown: bool,
}

fn invalid_token() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 47,
    }
}

pub(super) fn argument(request: &CicsRequest) -> Result<Option<u32>, HostProblem> {
    let Some(value) = request.arguments.get("TOKEN") else {
        return Ok(None);
    };
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    let numeric = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if !(1..=i64::from(i32::MAX)).contains(&numeric) {
        return Err(invalid_token());
    }
    Ok(Some(numeric as u32))
}

fn allocate(service: &CicsService) -> Result<u32, HostProblem> {
    for _ in 0..16 {
        let current = service
            .store
            .get_provider_state(NAMESPACE, KEY)
            .map_err(store_error)?;
        let previous = match &current {
            Some(row) => {
                if row.payload.len() != 4
                    || row.version
                        != u64::from(u32::from_be_bytes(
                            row.payload[..4]
                                .try_into()
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                        ))
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                row.version
            }
            None => 0,
        };
        let next = previous
            .checked_add(1)
            .filter(|value| *value <= i32::MAX as u64)
            .ok_or(HostProblem::ResourceExhausted)?;
        match service.store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: KEY.into(),
                version: next,
                payload: (next as u32).to_be_bytes().to_vec(),
            },
            current.map(|row| row.version),
        ) {
            Ok(()) => return Ok(next as u32),
            Err(StoreError::AlreadyExists | StoreError::Conflict) => continue,
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

pub(super) fn issue(
    service: &CicsService,
    run: &mut Run,
    token: FileUpdateToken,
) -> Result<u32, HostProblem> {
    let id = allocate(service)?;
    if run.file_updates.file_tokens.insert(id, token).is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(id)
}

pub(super) fn held<'a>(
    run: &'a Run,
    id: u32,
    dataset: &str,
) -> Result<&'a FileUpdateToken, HostProblem> {
    run.file_updates
        .file_tokens
        .get(&id)
        .filter(|token| token.dataset == dataset)
        .ok_or_else(invalid_token)
}

pub(super) fn consume(run: &mut Run, id: u32, dataset: &str) -> Result<(), HostProblem> {
    held(run, id, dataset)?;
    run.file_updates.file_tokens.remove(&id);
    Ok(())
}

pub(super) fn invalidate_dataset(run: &mut Run, dataset: &str) {
    run.file_updates
        .file_tokens
        .retain(|_, token| token.dataset != dataset);
}

pub(super) fn invalidate_browse(run: &mut Run, dataset: &str, cursor: &str) {
    run.file_updates.file_tokens.retain(|_, token| {
        token.dataset != dataset || token.browse_cursor.as_deref() != Some(cursor)
    });
}
