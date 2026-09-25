//! Source-reviewed UNLOCK over the task-owned file update authority.

use super::super::{
    CicsFileStatus, CicsService, DurableFileStatus, Run, argument_optional, argument_text,
    encode_file_status, store_error,
};
use super::file_tokens;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, DatasetName, HostProblem,
};
use mainframe_env_store_api::ProviderStateRecord;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let logical_name = argument_text(request, "DATASET")
        .or_else(|_| argument_text(request, "FILE"))?
        .trim()
        .to_ascii_uppercase();
    let definition = service.lock()?.file_aliases.get(&logical_name).cloned();
    let name = definition
        .as_ref()
        .map(|definition| definition.dataset.as_str().to_string())
        .unwrap_or_else(|| logical_name.clone());
    let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
    service
        .authorize(run, "DATASET", dataset.as_str(), AccessIntent::Update)
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 101,
            },
            other => other,
        })?;
    if argument_optional(request, "SYSID")
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case(&run.sysid))
    {
        return Err(HostProblem::Condition {
            name: "SYSIDERR".into(),
            response: 53,
            response2: 130,
        });
    }
    let token = file_tokens::argument(request)?;
    let held = match token {
        Some(token) => {
            file_tokens::held(run, token, dataset.as_str())?;
            true
        }
        None => run.current_records.contains_key(dataset.as_str()),
    };
    if !held {
        if definition.is_none() {
            return Err(HostProblem::Condition {
                name: "FILENOTFOUND".into(),
                response: 12,
                response2: 1,
            });
        }
        let mut state = service.lock()?;
        match state.file_statuses.get(&logical_name).copied() {
            Some(DurableFileStatus {
                status: CicsFileStatus::Closed,
                ..
            }) => {
                return Err(HostProblem::Condition {
                    name: "NOTOPEN".into(),
                    response: 19,
                    response2: 60,
                });
            }
            Some(DurableFileStatus {
                status: CicsFileStatus::Disabled,
                ..
            }) => {
                return Err(HostProblem::Condition {
                    name: "DISABLED".into(),
                    response: 84,
                    response2: 50,
                });
            }
            Some(
                current @ DurableFileStatus {
                    status: CicsFileStatus::ClosedEnabled,
                    ..
                },
            ) => {
                let version = current
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                service
                    .store
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: "cics-file-status".into(),
                            key: logical_name.clone(),
                            version,
                            payload: encode_file_status(CicsFileStatus::Open),
                        },
                        Some(current.version),
                    )
                    .map_err(store_error)?;
                state.file_statuses.insert(
                    logical_name.clone(),
                    DurableFileStatus {
                        status: CicsFileStatus::Open,
                        version,
                    },
                );
            }
            Some(DurableFileStatus {
                status: CicsFileStatus::Open,
                ..
            })
            | None => {}
        }
    }
    match token {
        Some(token) => file_tokens::consume(run, token, dataset.as_str())?,
        None => {
            run.current_records.remove(dataset.as_str());
            run.file_updates
                .current_record_values
                .remove(dataset.as_str());
        }
    }
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}
