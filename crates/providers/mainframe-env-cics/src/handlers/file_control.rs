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
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, DatasetName, DatasetRequest,
    DatasetResult, HostProblem, HostRequest, HostResult, MemberName, RecordFormat,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite};
use std::collections::BTreeMap;

mod first_reverse;
mod task_retirement;
pub(super) use task_retirement::release_task;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> first_reverse::Outcome {
    let mut completion = first_reverse::Completion::Keep;
    let result = match request.operation {
        CicsOperation::SetFileStatus => set_file_statuses(service, run, request),
        CicsOperation::Read
        | CicsOperation::Write
        | CicsOperation::Rewrite
        | CicsOperation::Delete
        | CicsOperation::StartBrowse
        | CicsOperation::ReadNext
        | CicsOperation::ReadPrev
        | CicsOperation::ResetBrowse
        | CicsOperation::EndBrowse => first_reverse::file(service, run, request, &mut completion),
        CicsOperation::Unlock => super::file_unlock::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    };
    first_reverse::Outcome::new(result, completion)
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
