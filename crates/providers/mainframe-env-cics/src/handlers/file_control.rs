#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsFileDefinition {
    pub dataset: DatasetName,
    pub ccsid: Option<u16>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::service) struct DurableFileStatus {
    pub(in crate::service) status: CicsFileStatus,
    pub(in crate::service) version: u64,
}

#[cfg(feature = "fault-injection")]
use super::super::CicsFileFaultPoint;
use super::super::{
    CicsFileStatus, CicsService, DatasetUndo, Run, argument_bytes, argument_optional,
    argument_text, bounded, decode_dataset_bytes, encode_dataset_bytes, encode_file_status,
    nested_mutation, normalize_terminal_name, store_error,
};
use super::file_tokens::{self, FileUpdateToken};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, DatasetName,
    DatasetRequest, DatasetResult, HostProblem, HostRequest, HostResult, MemberName, RecordFormat,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite};
use std::collections::BTreeMap;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::SetFileStatus => set_file_statuses(service, run, request),
        CicsOperation::Read
        | CicsOperation::Write
        | CicsOperation::Rewrite
        | CicsOperation::Delete
        | CicsOperation::StartBrowse
        | CicsOperation::ReadNext
        | CicsOperation::ReadPrev
        | CicsOperation::ResetBrowse
        | CicsOperation::EndBrowse => file(service, run, request),
        CicsOperation::Unlock => super::file_unlock::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn access_for(operation: CicsOperation) -> AccessIntent {
    if operation == CicsOperation::ResetBrowse {
        AccessIntent::Read
    } else {
        super::super::access_for(operation)
    }
}

impl CicsService {
    pub fn file_status(&self, name: &str) -> Result<CicsFileStatus, HostProblem> {
        let name = normalize_terminal_name(name, 16)?;
        let state = self.lock()?;
        if !state.file_aliases.contains_key(&name) {
            return Err(HostProblem::NotFound);
        }
        Ok(state
            .file_statuses
            .get(&name)
            .map_or(CicsFileStatus::Open, |record| record.status))
    }
}

fn set_file_statuses(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.is_empty() {
        return Err(HostProblem::Malformed);
    }
    let mut requested = BTreeMap::new();
    for (name, value) in &request.arguments {
        let normalized = normalize_terminal_name(name, 16)?;
        if value.schema() != "mainframe-env.cics.file-status@1" {
            return Err(HostProblem::Malformed);
        }
        let status = match value.bytes() {
            b"OPEN" => CicsFileStatus::Open,
            b"CLOSED-ENABLED" => CicsFileStatus::ClosedEnabled,
            b"CLOSED" | b"CLOSED-UNENABLED" => CicsFileStatus::Closed,
            b"DISABLED" => CicsFileStatus::Disabled,
            _ => return Err(HostProblem::Malformed),
        };
        if requested.insert(normalized, status).is_some() {
            return Err(HostProblem::Malformed);
        }
    }
    let mut state = service.lock()?;
    if requested
        .keys()
        .any(|name| !state.file_aliases.contains_key(name))
    {
        return Err(HostProblem::NotFound);
    }
    let mut writes = Vec::new();
    let mut changes = BTreeMap::new();
    for (name, status) in &requested {
        let current = state.file_statuses.get(name).copied();
        if current.is_some_and(|current| current.status == *status)
            || current.is_none() && *status == CicsFileStatus::Open
        {
            continue;
        }
        let version = current.map_or(Ok(1), |current| {
            current
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "cics-file-status".into(),
                key: name.clone(),
                version,
                payload: encode_file_status(*status),
            },
            expected_version: current.map(|current| current.version),
        });
        changes.insert(
            name.clone(),
            DurableFileStatus {
                status: *status,
                version,
            },
        );
    }
    if !writes.is_empty() {
        service
            .store
            .put_provider_states_atomic(writes)
            .map_err(store_error)?;
    }
    for (name, status) in changes {
        state.file_statuses.insert(name, status);
    }
    drop(state);
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

fn file(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let logical_name = argument_text(request, "DATASET")
        .or_else(|_| argument_text(request, "FILE"))?
        .trim()
        .to_ascii_uppercase();
    #[cfg(feature = "fault-injection")]
    {
        let mut state = service.lock()?;
        if state
            .file_failure
            .as_ref()
            .is_some_and(|(operation, file)| {
                *operation == request.operation && file == &logical_name
            })
        {
            state.file_failure = None;
            return Err(HostProblem::Condition {
                name: "IOERR".into(),
                response: 17,
                response2: 1,
            });
        }
    }
    let definition = {
        let state = service.lock()?;
        state.file_aliases.get(&logical_name).cloned()
    };
    let ccsid = definition.as_ref().and_then(|definition| definition.ccsid);
    let name = definition
        .map(|definition| definition.dataset.as_str().to_string())
        .unwrap_or_else(|| logical_name.clone());
    let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
    let dataset_key = dataset.as_str().to_string();
    service
        .authorize(
            run,
            "DATASET",
            dataset.as_str(),
            access_for(request.operation),
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 101,
            },
            other => other,
        })?;
    {
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
                    response2: 0,
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
    let length = decimal_argument(request, "LENGTH")?;
    let key_length = signed_decimal_argument(request, "KEYLENGTH")?;
    let attributes = if (length.is_some()
        || key_length.is_some()
        || matches!(
            request.operation,
            CicsOperation::ReadNext | CicsOperation::ReadPrev
        ))
        && matches!(
            request.operation,
            CicsOperation::Read
                | CicsOperation::Write
                | CicsOperation::Rewrite
                | CicsOperation::Delete
                | CicsOperation::StartBrowse
                | CicsOperation::ResetBrowse
                | CicsOperation::ReadNext
                | CicsOperation::ReadPrev
        ) {
        match service.nested(
            run,
            HostRequest::Dataset(DatasetRequest::Attributes {
                dataset: dataset.clone(),
            }),
        )? {
            HostResult::Dataset(DatasetResult::Attributes { attributes, .. }) => Some(attributes),
            _ => return Err(HostProblem::ProviderFailure),
        }
    } else {
        None
    };
    let generic = request.arguments.contains_key("OPTION.GENERIC");
    let gteq = request.arguments.contains_key("OPTION.GTEQ");
    let equal = request.arguments.contains_key("OPTION.EQUAL");
    validate_search_relation(request.operation, equal, gteq)?;
    validate_key_length(
        request.operation,
        key_length,
        attributes.as_ref(),
        generic,
        gteq,
    )?;
    let mut length_condition = if matches!(
        request.operation,
        CicsOperation::Read
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::Write
            | CicsOperation::Rewrite
    ) {
        validate_record_length(request.operation, length, attributes.as_ref())?
    } else {
        None
    };
    let transfer_length = if matches!(
        request.operation,
        CicsOperation::Write | CicsOperation::Rewrite
    ) {
        length.map(|length| {
            attributes.as_ref().map_or(length, |attributes| {
                length.min(attributes.logical_record_length)
            })
        })
    } else {
        length
    };
    let member = argument_optional(request, "MEMBER")
        .map(|name| MemberName::new(name, 8).map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let token_id = if matches!(
        request.operation,
        CicsOperation::Rewrite | CicsOperation::Delete
    ) {
        file_tokens::argument(request)?
    } else {
        None
    };
    if token_id.is_some() && request.arguments.contains_key("RIDFLD") {
        return Err(HostProblem::Malformed);
    }
    let token_hold = token_id
        .map(|id| file_tokens::held(run, id, &dataset_key).cloned())
        .transpose()?;
    let token_output = request.arguments.contains_key("TOKEN")
        && matches!(
            request.operation,
            CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
        );
    let update_requested = token_output || request.arguments.contains_key("OPTION.UPDATE");
    if update_requested
        && !token_output
        && matches!(
            request.operation,
            CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
        )
        && run.current_records.contains_key(&dataset_key)
    {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 28,
        });
    }
    let pending_undo = match request.operation {
        CicsOperation::Write => argument_bytes(request, "RIDFLD")
            .map(|key| encode_dataset_bytes(ccsid, &key))
            .transpose()?
            .map(|key| DatasetUndo::Delete {
                dataset: dataset.clone(),
                key,
            }),
        CicsOperation::Rewrite | CicsOperation::Delete => {
            let key = token_hold
                .as_ref()
                .map(|hold| hold.identity.clone())
                .or(argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?
                    .or_else(|| run.current_records.get(&dataset_key).cloned()));
            let previous = token_hold
                .as_ref()
                .map(|hold| hold.record.clone())
                .or_else(|| {
                    run.file_updates
                        .current_record_values
                        .get(&dataset_key)
                        .cloned()
                });
            key.zip(previous).map(|(key, record)| DatasetUndo::Restore {
                dataset: dataset.clone(),
                key,
                record,
            })
        }
        _ => None,
    };
    let mutated_record = matches!(
        request.operation,
        CicsOperation::Write | CicsOperation::Rewrite
    )
    .then(|| {
        let mut record = argument_bytes(request, "FROM").unwrap_or_default();
        if let Some(length) = transfer_length {
            record.truncate(length as usize);
        }
        if request.operation == CicsOperation::Write
            && length.is_some()
            && let Some(attributes) = attributes.as_ref()
            && !variable_record_format(attributes.record_format)
        {
            record.resize(attributes.logical_record_length as usize, 0);
        }
        encode_dataset_bytes(ccsid, &record)
    })
    .transpose()?;
    let host_request = match request.operation {
        CicsOperation::Read => DatasetRequest::Read {
            dataset: dataset.clone(),
            member,
            key: argument_bytes(request, "RIDFLD")
                .map(|value| truncate_key(value, key_length))
                .transpose()?
                .map(|value| encode_dataset_bytes(ccsid, &value))
                .transpose()?,
            max_records: 1,
            control: Default::default(),
        },
        CicsOperation::Write => {
            let sequence = run
                .host_sequence
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutation = nested_mutation(run, sequence)?;
            DatasetRequest::Write {
                dataset: dataset.clone(),
                member,
                records: vec![mutated_record.clone().ok_or(HostProblem::ProviderFailure)?],
                expected_version: argument_optional(request, "VERSION")
                    .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                    .transpose()?,
                mutation,
            }
        }
        CicsOperation::Rewrite => {
            let sequence = run
                .host_sequence
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutation = nested_mutation(run, sequence)?;
            let key = token_hold
                .as_ref()
                .map(|hold| hold.identity.clone())
                .or_else(|| run.current_records.get(&dataset_key).cloned())
                .ok_or_else(|| HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2: 30,
                })?;
            DatasetRequest::RewriteRecord {
                dataset: dataset.clone(),
                key,
                record: mutated_record.clone().ok_or(HostProblem::ProviderFailure)?,
                expected_version: argument_optional(request, "VERSION")
                    .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                    .transpose()?,
                mutation,
            }
        }
        CicsOperation::Delete => {
            let sequence = run
                .host_sequence
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutation = nested_mutation(run, sequence)?;
            let key = token_hold
                .as_ref()
                .map(|hold| hold.identity.clone())
                .or(argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?)
                .or_else(|| run.current_records.get(&dataset_key).cloned())
                .ok_or_else(|| HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2: 31,
                })?;
            DatasetRequest::DeleteRecord {
                dataset: dataset.clone(),
                key,
                expected_version: argument_optional(request, "VERSION")
                    .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                    .transpose()?,
                mutation,
            }
        }
        CicsOperation::StartBrowse => DatasetRequest::StartBrowse {
            dataset: dataset.clone(),
            key: encode_dataset_bytes(
                ccsid,
                &truncate_key(
                    argument_bytes(request, "RIDFLD").unwrap_or_default(),
                    key_length,
                )?,
            )?,
            relation: if equal && !generic {
                mainframe_env_host_api::KeyRelation::Equal
            } else {
                mainframe_env_host_api::KeyRelation::GreaterOrEqual
            },
        },
        CicsOperation::ResetBrowse => {
            let cursor = owned_browse_cursor(run, request, &dataset_key, 36)?;
            DatasetRequest::ResetBrowse {
                dataset: dataset.clone(),
                cursor,
                key: encode_dataset_bytes(
                    ccsid,
                    &truncate_key(
                        argument_bytes(request, "RIDFLD").ok_or(HostProblem::Malformed)?,
                        key_length,
                    )?,
                )?,
                relation: if !gteq && !generic {
                    mainframe_env_host_api::KeyRelation::Equal
                } else {
                    mainframe_env_host_api::KeyRelation::GreaterOrEqual
                },
            }
        }
        CicsOperation::ReadNext | CicsOperation::ReadPrev => DatasetRequest::ReadNext {
            dataset: dataset.clone(),
            cursor: owned_browse_cursor(run, request, &dataset_key, 0)?,
            reverse: request.operation == CicsOperation::ReadPrev,
            control: Default::default(),
        },
        CicsOperation::EndBrowse => DatasetRequest::EndBrowse {
            dataset: dataset.clone(),
            cursor: owned_browse_cursor(run, request, &dataset_key, 0)?,
        },
        _ => return Err(HostProblem::Malformed),
    };
    let operation = request.operation;
    #[cfg(feature = "fault-injection")]
    if service.consume_file_fault(operation, &logical_name, CicsFileFaultPoint::BeforeIntent)? {
        return Err(HostProblem::InfrastructureFailure);
    }
    if let Some(undo) = pending_undo.clone() {
        service.append_undo(run, undo)?;
    }
    #[cfg(feature = "fault-injection")]
    if service.consume_file_fault(operation, &logical_name, CicsFileFaultPoint::AfterIntent)? {
        return Err(HostProblem::InfrastructureFailure);
    }
    if (operation == CicsOperation::StartBrowse && generic && equal)
        || (operation == CicsOperation::ResetBrowse && generic && !gteq)
    {
        let key = argument_bytes(request, "RIDFLD")
            .map(|value| truncate_key(value, key_length))
            .transpose()?
            .map(|value| encode_dataset_bytes(ccsid, &value))
            .transpose()?
            .ok_or(HostProblem::Malformed)?;
        read_relational(service, run, dataset.clone(), key, true)
            .map_err(|problem| normalize_file_not_found(operation, problem))?;
    }
    let result = if operation == CicsOperation::Read && (gteq || generic) {
        let key = argument_bytes(request, "RIDFLD")
            .map(|value| truncate_key(value, key_length))
            .transpose()?
            .map(|value| encode_dataset_bytes(ccsid, &value))
            .transpose()?
            .ok_or(HostProblem::Malformed)?;
        read_relational(service, run, dataset.clone(), key, generic && !gteq)
    } else {
        service.nested(run, HostRequest::Dataset(host_request))
    }
    .map_err(|problem| normalize_file_not_found(operation, problem))?;
    let mut browse_key = None;
    let mut issued_token = None;
    let mut payload = match result {
        HostResult::Dataset(DatasetResult::Records {
            records,
            identities,
            ..
        }) => {
            let record = records.into_iter().next().unwrap_or_default();
            if update_requested && let Some(identity) = identities.first() {
                if token_output {
                    issued_token = Some(file_tokens::issue(
                        service,
                        run,
                        FileUpdateToken {
                            dataset: dataset_key.clone(),
                            identity: identity.clone(),
                            record: record.clone(),
                            browse_cursor: None,
                        },
                    )?);
                } else {
                    run.current_records
                        .insert(dataset_key.clone(), identity.clone());
                    run.file_updates
                        .current_record_values
                        .insert(dataset_key.clone(), record.clone());
                }
            }
            decode_dataset_bytes(ccsid, &record)?
        }
        HostResult::Dataset(DatasetResult::Browse {
            cursor,
            record,
            identity,
            key,
        }) => {
            if operation == CicsOperation::StartBrowse {
                run.browses.insert(dataset_key.clone(), cursor.clone());
            } else if operation == CicsOperation::ResetBrowse {
                run.current_records.remove(&dataset_key);
                run.file_updates.current_record_values.remove(&dataset_key);
                file_tokens::invalidate_dataset(run, &dataset_key);
            } else if operation == CicsOperation::EndBrowse {
                run.browses.remove(&dataset_key);
                run.current_records.remove(&dataset_key);
                run.file_updates.current_record_values.remove(&dataset_key);
                file_tokens::invalidate_browse(run, &dataset_key, &cursor);
            } else if matches!(operation, CicsOperation::ReadNext | CicsOperation::ReadPrev) {
                file_tokens::invalidate_browse(run, &dataset_key, &cursor);
            }
            browse_key = key
                .map(|key| decode_dataset_bytes(ccsid, &key))
                .transpose()?;
            if matches!(operation, CicsOperation::ReadNext | CicsOperation::ReadPrev)
                && record.is_none()
            {
                return Err(HostProblem::Condition {
                    name: "ENDFILE".into(),
                    response: 20,
                    response2: 0,
                });
            }
            let record = record.unwrap_or_default();
            if !record.is_empty()
                && update_requested
                && let Some(identity) = identity
            {
                if token_output {
                    issued_token = Some(file_tokens::issue(
                        service,
                        run,
                        FileUpdateToken {
                            dataset: dataset_key.clone(),
                            identity,
                            record: record.clone(),
                            browse_cursor: Some(cursor.clone()),
                        },
                    )?);
                } else {
                    run.current_records.insert(dataset_key.clone(), identity);
                    run.file_updates
                        .current_record_values
                        .insert(dataset_key.clone(), record.clone());
                }
            }
            decode_dataset_bytes(ccsid, &record)?
        }
        HostResult::Dataset(_) => Vec::new(),
        _ => return Err(HostProblem::ProviderFailure),
    };
    let actual_length = apply_read_length(operation, length, &mut payload, &mut length_condition)?;
    if matches!(operation, CicsOperation::Delete | CicsOperation::Rewrite) {
        if let Some(token) = token_id {
            file_tokens::consume(run, token, &dataset_key)?;
        } else if operation == CicsOperation::Rewrite || !request.arguments.contains_key("RIDFLD") {
            run.current_records.remove(&dataset_key);
            run.file_updates.current_record_values.remove(&dataset_key);
        }
    }
    let (condition, response_code, response2) = length_condition.unwrap_or(("NORMAL", 0, 0));
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        condition,
        response_code,
        response2,
        None,
        None,
        payload,
    )?;
    if let Some(key) = browse_key {
        response.outputs.insert("RIDFLD".into(), bounded(key)?);
    }
    if let Some(token) = issued_token {
        response.outputs.insert(
            "TOKEN".into(),
            super::super::decimal_payload(i64::from(token))?,
        );
    }
    if matches!(
        operation,
        CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
    ) && length.is_some()
    {
        response.outputs.insert(
            "LENGTH".into(),
            super::super::decimal_payload(i64::from(actual_length))?,
        );
    }
    Ok(response)
}

fn read_relational(
    service: &CicsService,
    run: &mut Run,
    dataset: DatasetName,
    key: Vec<u8>,
    require_prefix: bool,
) -> Result<HostResult, HostProblem> {
    let start = service.nested(
        run,
        HostRequest::Dataset(DatasetRequest::StartBrowse {
            dataset: dataset.clone(),
            key: key.clone(),
            relation: mainframe_env_host_api::KeyRelation::GreaterOrEqual,
        }),
    )?;
    let HostResult::Dataset(DatasetResult::Browse { cursor, .. }) = start else {
        return Err(HostProblem::ProviderFailure);
    };
    let read = service.nested(
        run,
        HostRequest::Dataset(DatasetRequest::ReadNext {
            dataset: dataset.clone(),
            cursor: cursor.clone(),
            reverse: false,
            control: Default::default(),
        }),
    );
    let close = service.nested(
        run,
        HostRequest::Dataset(DatasetRequest::EndBrowse { dataset, cursor }),
    );
    close?;
    let result = read?;
    let matched = matches!(
        &result,
        HostResult::Dataset(DatasetResult::Browse {
            record: Some(_),
            key: Some(found),
            ..
        }) if !require_prefix || found.starts_with(&key)
    );
    if !matched {
        Err(HostProblem::NotFound)
    } else {
        Ok(result)
    }
}

fn decimal_argument(request: &CicsRequest, name: &str) -> Result<Option<u32>, HostProblem> {
    let Some(value) = request.arguments.get(name) else {
        return Ok(None);
    };
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    let value = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<u32>()
        .map_err(|_| HostProblem::Malformed)?;
    Ok(Some(value))
}

fn signed_decimal_argument(request: &CicsRequest, name: &str) -> Result<Option<i32>, HostProblem> {
    let Some(value) = request.arguments.get(name) else {
        return Ok(None);
    };
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    let value = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i32>()
        .map_err(|_| HostProblem::Malformed)?;
    Ok(Some(value))
}

fn truncate_key(mut value: Vec<u8>, key_length: Option<i32>) -> Result<Vec<u8>, HostProblem> {
    if let Some(key_length) = key_length {
        value.truncate(usize::try_from(key_length).map_err(|_| HostProblem::Malformed)?);
    }
    Ok(value)
}

fn owned_browse_cursor(
    run: &Run,
    request: &CicsRequest,
    dataset_key: &str,
    response2: i32,
) -> Result<String, HostProblem> {
    let cursor = run
        .browses
        .get(dataset_key)
        .ok_or_else(|| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2,
        })?;
    if argument_optional(request, "CURSOR").is_some_and(|requested| requested.as_str() != cursor) {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2,
        });
    }
    Ok(cursor.clone())
}

fn normalize_file_not_found(operation: CicsOperation, problem: HostProblem) -> HostProblem {
    match (operation, problem) {
        (
            CicsOperation::Read | CicsOperation::StartBrowse | CicsOperation::ResetBrowse,
            HostProblem::NotFound,
        ) => HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 80,
        },
        (
            CicsOperation::Read | CicsOperation::StartBrowse | CicsOperation::ResetBrowse,
            HostProblem::Condition {
                name, response: 13, ..
            },
        ) if name == "NOTFND" => HostProblem::Condition {
            name,
            response: 13,
            response2: 80,
        },
        (_, other) => other,
    }
}

fn validate_key_length(
    operation: CicsOperation,
    key_length: Option<i32>,
    attributes: Option<&mainframe_env_host_api::DatasetAttributes>,
    generic: bool,
    gteq: bool,
) -> Result<(), HostProblem> {
    if generic {
        if !matches!(
            operation,
            CicsOperation::Read | CicsOperation::StartBrowse | CicsOperation::ResetBrowse
        ) {
            return Err(HostProblem::Malformed);
        }
        let key_length = key_length.ok_or(HostProblem::Malformed)?;
        if key_length < 0 {
            return Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 42,
            });
        }
        if key_length == 0 {
            return if gteq {
                Ok(())
            } else {
                Err(HostProblem::Malformed)
            };
        }
        if attributes
            .and_then(|attributes| attributes.key_length)
            .is_none_or(|defined| i64::from(key_length) >= i64::from(defined))
        {
            return Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 25,
            });
        }
        return Ok(());
    }
    if matches!(
        operation,
        CicsOperation::Read | CicsOperation::StartBrowse | CicsOperation::ResetBrowse
    ) && gteq
        && key_length == Some(0)
    {
        return Ok(());
    }
    if matches!(
        operation,
        CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Delete
            | CicsOperation::StartBrowse
            | CicsOperation::ResetBrowse
    ) && let Some(key_length) = key_length
        && attributes
            .and_then(|attributes| attributes.key_length)
            .map(i64::from)
            != Some(i64::from(key_length))
    {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 26,
        });
    }
    Ok(())
}

fn validate_search_relation(
    operation: CicsOperation,
    equal: bool,
    gteq: bool,
) -> Result<(), HostProblem> {
    if equal
        && (!matches!(
            operation,
            CicsOperation::Read | CicsOperation::StartBrowse | CicsOperation::ResetBrowse
        ) || gteq)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_record_length(
    operation: CicsOperation,
    length: Option<u32>,
    attributes: Option<&mainframe_env_host_api::DatasetAttributes>,
) -> Result<Option<(&'static str, i32, i32)>, HostProblem> {
    let Some(attributes) = attributes else {
        return Ok(None);
    };
    let variable = variable_record_format(attributes.record_format);
    if variable && length.is_none() {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 10,
        });
    }
    let Some(length) = length else {
        return Ok(None);
    };
    if variable && length > attributes.logical_record_length {
        return if matches!(operation, CicsOperation::Write | CicsOperation::Rewrite) {
            Ok(Some(("LENGERR", 22, 12)))
        } else {
            Ok(None)
        };
    }
    if !variable && length != attributes.logical_record_length {
        let response2 = if matches!(
            operation,
            CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
        ) {
            13
        } else {
            14
        };
        if operation == CicsOperation::Rewrite {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2,
            });
        }
        return Ok(Some(("LENGERR", 22, response2)));
    }
    Ok(None)
}

fn variable_record_format(record_format: RecordFormat) -> bool {
    matches!(
        record_format,
        RecordFormat::Variable
            | RecordFormat::VariableBlocked
            | RecordFormat::VariableSpanned
            | RecordFormat::VariableBlockedSpanned
            | RecordFormat::Undefined
            | RecordFormat::Line
    )
}

fn apply_read_length(
    operation: CicsOperation,
    length: Option<u32>,
    payload: &mut Vec<u8>,
    condition: &mut Option<(&'static str, i32, i32)>,
) -> Result<u32, HostProblem> {
    let actual = u32::try_from(payload.len()).map_err(|_| HostProblem::ResourceExhausted)?;
    if matches!(
        operation,
        CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
    ) && let Some(maximum) = length
        && actual > maximum
    {
        payload.truncate(maximum as usize);
        *condition = Some(("LENGERR", 22, 11));
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::{DatasetAttributes, DatasetOrganization};

    fn attributes(record_format: RecordFormat, record_length: u32) -> DatasetAttributes {
        DatasetAttributes {
            organization: DatasetOrganization::KeySequenced,
            record_format,
            logical_record_length: record_length,
            key_offset: Some(0),
            key_length: Some(3),
            ccsid: None,
        }
    }

    #[test]
    fn read_length_reports_actual_before_truncation_and_sourced_conditions() {
        let variable = attributes(RecordFormat::Variable, 8);
        assert!(matches!(
            validate_record_length(CicsOperation::Read, None, Some(&variable)),
            Err(HostProblem::Condition {
                response: 22,
                response2: 10,
                ..
            })
        ));

        let mut payload = b"ABCDEFGH".to_vec();
        let mut condition =
            validate_record_length(CicsOperation::Read, Some(4), Some(&variable)).unwrap();
        assert_eq!(
            apply_read_length(CicsOperation::Read, Some(4), &mut payload, &mut condition,),
            Ok(8)
        );
        assert_eq!(payload, b"ABCD");
        assert_eq!(condition, Some(("LENGERR", 22, 11)));

        assert!(matches!(
            validate_record_length(CicsOperation::ReadNext, None, Some(&variable)),
            Err(HostProblem::Condition {
                response: 22,
                response2: 10,
                ..
            })
        ));
        let mut browse_payload = b"ABCDEFGH".to_vec();
        let mut browse_condition =
            validate_record_length(CicsOperation::ReadNext, Some(4), Some(&variable)).unwrap();
        assert_eq!(
            apply_read_length(
                CicsOperation::ReadNext,
                Some(4),
                &mut browse_payload,
                &mut browse_condition,
            ),
            Ok(8)
        );
        assert_eq!(browse_payload, b"ABCD");
        assert_eq!(browse_condition, Some(("LENGERR", 22, 11)));

        let fixed = attributes(RecordFormat::Fixed, 8);
        assert_eq!(
            validate_record_length(CicsOperation::Read, Some(10), Some(&fixed)),
            Ok(Some(("LENGERR", 22, 13)))
        );
        assert_eq!(
            validate_record_length(CicsOperation::ReadPrev, Some(10), Some(&fixed)),
            Ok(Some(("LENGERR", 22, 13)))
        );
    }

    #[test]
    fn write_rewrite_length_and_full_key_mismatch_use_ibm_resp2_values() {
        let variable = attributes(RecordFormat::Variable, 8);
        assert!(matches!(
            validate_record_length(CicsOperation::Rewrite, None, Some(&variable)),
            Err(HostProblem::Condition {
                response: 22,
                response2: 10,
                ..
            })
        ));
        assert_eq!(
            validate_record_length(CicsOperation::Rewrite, Some(9), Some(&variable)),
            Ok(Some(("LENGERR", 22, 12)))
        );
        assert_eq!(
            validate_record_length(CicsOperation::Write, Some(9), Some(&variable)),
            Ok(Some(("LENGERR", 22, 12)))
        );
        let fixed = attributes(RecordFormat::Fixed, 8);
        assert_eq!(
            validate_search_relation(CicsOperation::Read, true, false),
            Ok(())
        );
        assert_eq!(
            validate_search_relation(CicsOperation::Read, true, true),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            validate_search_relation(CicsOperation::Write, true, false),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            validate_search_relation(CicsOperation::StartBrowse, true, false),
            Ok(())
        );
        assert_eq!(
            validate_search_relation(CicsOperation::StartBrowse, true, true),
            Err(HostProblem::Malformed)
        );
        assert!(matches!(
            validate_record_length(CicsOperation::Rewrite, Some(7), Some(&fixed)),
            Err(HostProblem::Condition {
                response: 22,
                response2: 14,
                ..
            })
        ));
        assert!(matches!(
            validate_key_length(CicsOperation::Read, Some(2), Some(&fixed), false, false),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(3), Some(&fixed), false, false),
            Ok(())
        );
        assert!(matches!(
            validate_key_length(CicsOperation::Write, Some(2), Some(&fixed), false, false),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert!(matches!(
            validate_key_length(CicsOperation::Delete, Some(2), Some(&fixed), false, false),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(2), Some(&fixed), true, false),
            Ok(())
        );
        assert!(matches!(
            validate_key_length(CicsOperation::Read, Some(3), Some(&fixed), true, false),
            Err(HostProblem::Condition {
                response: 16,
                response2: 25,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(CicsOperation::Read, None, Some(&fixed), true, false),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(0), Some(&fixed), false, true),
            Ok(())
        );
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(0), Some(&fixed), true, true),
            Ok(())
        );
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(0), Some(&fixed), true, false),
            Err(HostProblem::Malformed)
        );
        assert!(matches!(
            validate_key_length(CicsOperation::Read, Some(0), Some(&fixed), false, false),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(
                CicsOperation::StartBrowse,
                Some(2),
                Some(&fixed),
                true,
                false,
            ),
            Ok(())
        );
        assert!(matches!(
            validate_key_length(
                CicsOperation::StartBrowse,
                Some(3),
                Some(&fixed),
                true,
                false,
            ),
            Err(HostProblem::Condition {
                response: 16,
                response2: 25,
                ..
            })
        ));
        assert!(matches!(
            validate_key_length(
                CicsOperation::StartBrowse,
                Some(-1),
                Some(&fixed),
                true,
                false,
            ),
            Err(HostProblem::Condition {
                response: 16,
                response2: 42,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(
                CicsOperation::StartBrowse,
                Some(0),
                Some(&fixed),
                true,
                true,
            ),
            Ok(())
        );
        assert_eq!(
            validate_record_length(CicsOperation::Write, Some(5), Some(&fixed)),
            Ok(Some(("LENGERR", 22, 14)))
        );
    }
}
