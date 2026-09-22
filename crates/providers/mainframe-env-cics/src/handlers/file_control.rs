#[cfg(feature = "fault-injection")]
use super::super::CicsFileFaultPoint;
use super::super::{
    CicsFileStatus, CicsService, DatasetUndo, DurableFileStatus, Run, access_for, argument_bytes,
    argument_optional, argument_text, bounded, decode_dataset_bytes, encode_dataset_bytes,
    encode_file_status, nested_mutation, normalize_terminal_name, store_error,
};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, DatasetName, DatasetRequest,
    DatasetResult, HostProblem, HostRequest, HostResult, MemberName, RecordFormat,
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
        | CicsOperation::EndBrowse => file(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
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
    let length = decimal_argument(request, "LENGTH")?;
    let key_length = decimal_argument(request, "KEYLENGTH")?;
    let attributes = if (length.is_some() || key_length.is_some())
        && matches!(
            request.operation,
            CicsOperation::Read
                | CicsOperation::Write
                | CicsOperation::Rewrite
                | CicsOperation::Delete
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
    validate_key_length(request.operation, key_length, attributes.as_ref())?;
    let mut length_condition = if matches!(
        request.operation,
        CicsOperation::Read | CicsOperation::Write | CicsOperation::Rewrite
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
    let pending_undo = match request.operation {
        CicsOperation::Write => argument_bytes(request, "RIDFLD")
            .map(|key| encode_dataset_bytes(ccsid, &key))
            .transpose()?
            .map(|key| DatasetUndo::Delete {
                dataset: dataset.clone(),
                key,
            }),
        CicsOperation::Rewrite | CicsOperation::Delete => {
            let key = argument_bytes(request, "RIDFLD")
                .map(|value| encode_dataset_bytes(ccsid, &value))
                .transpose()?
                .or_else(|| run.current_records.get(&dataset_key).cloned());
            key.zip(run.current_record_values.get(&dataset_key).cloned())
                .map(|(key, record)| DatasetUndo::Restore {
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
                .map(|mut value| {
                    if let Some(key_length) = key_length {
                        value.truncate(key_length as usize);
                    }
                    encode_dataset_bytes(ccsid, &value)
                })
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
            let key = argument_bytes(request, "RIDFLD")
                .map(|value| encode_dataset_bytes(ccsid, &value))
                .transpose()?
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
            let key = argument_bytes(request, "RIDFLD")
                .map(|value| encode_dataset_bytes(ccsid, &value))
                .transpose()?
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
                &argument_bytes(request, "RIDFLD").unwrap_or_default(),
            )?,
            relation: mainframe_env_host_api::KeyRelation::GreaterOrEqual,
        },
        CicsOperation::ReadNext | CicsOperation::ReadPrev => DatasetRequest::ReadNext {
            dataset: dataset.clone(),
            cursor: argument_optional(request, "CURSOR")
                .or_else(|| run.browses.get(&dataset_key).cloned())
                .ok_or_else(|| HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2: 0,
                })?,
            reverse: request.operation == CicsOperation::ReadPrev,
            control: Default::default(),
        },
        CicsOperation::EndBrowse => DatasetRequest::EndBrowse {
            dataset: dataset.clone(),
            cursor: argument_optional(request, "CURSOR")
                .or_else(|| run.browses.get(&dataset_key).cloned())
                .ok_or_else(|| HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2: 0,
                })?,
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
    let result = service
        .nested(run, HostRequest::Dataset(host_request))
        .map_err(|problem| match (operation, problem) {
            (CicsOperation::Read, HostProblem::NotFound) => HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 80,
            },
            (
                CicsOperation::Read,
                HostProblem::Condition {
                    name, response: 13, ..
                },
            ) if name == "NOTFND" => HostProblem::Condition {
                name,
                response: 13,
                response2: 80,
            },
            (_, other) => other,
        })?;
    let mut browse_key = None;
    let mut payload = match result {
        HostResult::Dataset(DatasetResult::Records {
            records,
            identities,
            ..
        }) => {
            if request.arguments.contains_key("OPTION.UPDATE")
                && let Some(identity) = identities.first()
            {
                run.current_records
                    .insert(dataset_key.clone(), identity.clone());
            }
            let record = records.into_iter().next().unwrap_or_default();
            if request.arguments.contains_key("OPTION.UPDATE") {
                run.current_record_values
                    .insert(dataset_key.clone(), record.clone());
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
                run.browses.insert(dataset_key.clone(), cursor);
            } else if operation == CicsOperation::EndBrowse {
                run.browses.remove(&dataset_key);
                run.current_records.remove(&dataset_key);
            }
            if request.arguments.contains_key("OPTION.UPDATE")
                && let Some(identity) = identity
            {
                run.current_records.insert(dataset_key.clone(), identity);
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
            if !record.is_empty() && request.arguments.contains_key("OPTION.UPDATE") {
                run.current_record_values
                    .insert(dataset_key.clone(), record.clone());
            }
            decode_dataset_bytes(ccsid, &record)?
        }
        HostResult::Dataset(_) => Vec::new(),
        _ => return Err(HostProblem::ProviderFailure),
    };
    let actual_length = apply_read_length(operation, length, &mut payload, &mut length_condition)?;
    if matches!(operation, CicsOperation::Delete | CicsOperation::Rewrite) {
        run.current_records.remove(&dataset_key);
        run.current_record_values.remove(&dataset_key);
    } else if operation == CicsOperation::Write
        && let Some(identity) = argument_bytes(request, "RIDFLD")
    {
        run.current_records
            .insert(dataset_key.clone(), encode_dataset_bytes(ccsid, &identity)?);
    }
    if let Some(record) = mutated_record
        && operation != CicsOperation::Rewrite
    {
        run.current_record_values.insert(dataset_key, record);
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
    if operation == CicsOperation::Read && length.is_some() {
        response.outputs.insert(
            "LENGTH".into(),
            super::super::decimal_payload(i64::from(actual_length))?,
        );
    }
    Ok(response)
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

fn validate_key_length(
    operation: CicsOperation,
    key_length: Option<u32>,
    attributes: Option<&mainframe_env_host_api::DatasetAttributes>,
) -> Result<(), HostProblem> {
    if matches!(
        operation,
        CicsOperation::Read | CicsOperation::Write | CicsOperation::Delete
    ) && let Some(key_length) = key_length
        && attributes.and_then(|attributes| attributes.key_length) != Some(key_length)
    {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 26,
        });
    }
    Ok(())
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
        let response2 = if operation == CicsOperation::Read {
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
    if operation == CicsOperation::Read
        && let Some(maximum) = length
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

        let fixed = attributes(RecordFormat::Fixed, 8);
        assert_eq!(
            validate_record_length(CicsOperation::Read, Some(10), Some(&fixed)),
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
        assert!(matches!(
            validate_record_length(CicsOperation::Rewrite, Some(7), Some(&fixed)),
            Err(HostProblem::Condition {
                response: 22,
                response2: 14,
                ..
            })
        ));
        assert!(matches!(
            validate_key_length(CicsOperation::Read, Some(2), Some(&fixed)),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert_eq!(
            validate_key_length(CicsOperation::Read, Some(3), Some(&fixed)),
            Ok(())
        );
        assert!(matches!(
            validate_key_length(CicsOperation::Write, Some(2), Some(&fixed)),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert!(matches!(
            validate_key_length(CicsOperation::Delete, Some(2), Some(&fixed)),
            Err(HostProblem::Condition {
                response: 16,
                response2: 26,
                ..
            })
        ));
        assert_eq!(
            validate_record_length(CicsOperation::Write, Some(5), Some(&fixed)),
            Ok(Some(("LENGERR", 22, 14)))
        );
    }
}
