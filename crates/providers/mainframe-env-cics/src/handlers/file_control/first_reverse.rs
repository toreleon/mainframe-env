//! First-read positioning and local completion ownership for file commands.
//! Existing file execution is extracted here; no nested clock or task retirement claim.

use super::*;
use crate::service::{CicsCommandFamily, handlers, program, terminal};
use mainframe_env_host_api::{DatasetAttributes, DatasetOrganization};

pub(in crate::service) struct Outcome {
    result: Result<CicsResponse, HostProblem>,
    completion: Completion,
}

impl Outcome {
    pub(super) fn new(result: Result<CicsResponse, HostProblem>, completion: Completion) -> Self {
        Self { result, completion }
    }

    pub(in crate::service) fn into_parts(self) -> (Result<CicsResponse, HostProblem>, Completion) {
        (self.result, self.completion)
    }
}

/// Local provenance, offered only after complete output construction or bound ordinary EOF.
pub(in crate::service) enum Completion {
    Keep,
    Consume {
        dataset: String,
        cursor: String,
    },
    Seed {
        dataset: String,
        cursor: String,
        key: Option<Vec<u8>>,
    },
    End {
        dataset: String,
        cursor: String,
    },
}

impl Completion {
    pub(in crate::service) fn commit(
        self,
        run: &mut Run,
        result: &Result<CicsResponse, HostProblem>,
    ) {
        if result.is_err() {
            return;
        }
        match self {
            Self::Keep => {}
            Self::Consume { dataset, cursor } | Self::End { dataset, cursor } => {
                if run
                    .initial_browse_positions
                    .get(&dataset)
                    .is_some_and(|(owned, _)| owned == &cursor)
                {
                    run.initial_browse_positions.remove(&dataset);
                }
            }
            Self::Seed {
                dataset,
                cursor,
                key,
            } => {
                if run.browses.get(&dataset) == Some(&cursor) {
                    if let Some(key) = key {
                        run.initial_browse_positions.insert(dataset, (cursor, key));
                    } else {
                        run.initial_browse_positions.remove(&dataset);
                    }
                }
            }
        }
    }
}

// These record only validated known provider effects; neither claims response atomicity.
fn record_start_owner(run: &mut Run, dataset: &str, cursor: &str) {
    // A known cursor replacement invalidates the previous seed even if later outputs fail.
    run.initial_browse_positions.remove(dataset);
    run.browses.insert(dataset.into(), cursor.into());
    run.file_updates.task_browses.insert(
        (dataset.into(), cursor.into()),
        file_tokens::FileBrowseOwner {
            actor: run.current_program.effect_invocation.clone(),
            retirement_unknown: false,
        },
    );
}

fn record_end_owner(run: &mut Run, dataset: &str) {
    // A bound fully empty ENDBR reply proves known retirement independently of later outputs.
    run.initial_browse_positions.remove(dataset);
    if let Some(cursor) = run.browses.remove(dataset) {
        run.file_updates
            .task_browses
            .remove(&(dataset.into(), cursor));
    }
}

fn ordinary_mode(request: &CicsRequest) -> bool {
    ![
        "OPTION.GENERIC",
        "OPTION.RBA",
        "OPTION.RRN",
        "OPTION.XRBA",
        "SYSID",
        "OPTION.UPDATE",
        "TOKEN",
        "REQID",
        "CURSOR.REQID",
    ]
    .iter()
    .any(|name| request.arguments.contains_key(*name))
}

fn provisional_key(request: &CicsRequest, key: &[u8]) -> Option<Vec<u8>> {
    (ordinary_mode(request)
        && !key.is_empty()
        && key.len() <= mainframe_env_host_api::HostLimits::default().max_record_bytes
        && !key.iter().all(|byte| *byte == 255))
    .then(|| key.to_vec())
}

fn selected_request(
    run: &Run,
    request: &CicsRequest,
    dataset: &DatasetName,
    cursor: &str,
    attributes: Option<&DatasetAttributes>,
    ccsid: Option<u16>,
) -> Result<Option<DatasetRequest>, HostProblem> {
    if request.operation != CicsOperation::ReadPrev || !ordinary_mode(request) {
        return Ok(None);
    }
    let Some((owned, seed)) = run.initial_browse_positions.get(dataset.as_str()) else {
        return Ok(None);
    };
    let Some(attributes) = attributes else {
        return Ok(None);
    };
    if owned != cursor
        || attributes.organization != DatasetOrganization::KeySequenced
        || attributes
            .key_length
            .is_none_or(|width| width as usize != seed.len())
    {
        return Ok(None);
    }
    let Some(supplied) = argument_bytes(request, "RIDFLD") else {
        return Ok(None);
    };
    let supplied = encode_dataset_bytes(ccsid, &supplied)?;
    if supplied != *seed {
        return Ok(None);
    }
    Ok(Some(DatasetRequest::ReadBrowsePosition {
        dataset: dataset.clone(),
        cursor: cursor.into(),
        expected_key: seed.clone(),
    }))
}

struct BrowsePlan {
    dataset: String,
    cursor: Option<String>,
    selected_key: Option<Vec<u8>>,
    seed: Option<Vec<u8>>,
    operation: CicsOperation,
}

impl BrowsePlan {
    fn new(request: &CicsRequest, host: &DatasetRequest, dataset: &str) -> Self {
        let (cursor, selected_key, seed) = match host {
            DatasetRequest::StartBrowse { key, .. } => (None, None, provisional_key(request, key)),
            DatasetRequest::ResetBrowse { cursor, key, .. } => {
                (Some(cursor.clone()), None, provisional_key(request, key))
            }
            DatasetRequest::ReadBrowsePosition {
                cursor,
                expected_key,
                ..
            } => (Some(cursor.clone()), Some(expected_key.clone()), None),
            DatasetRequest::ReadNext { cursor, .. } | DatasetRequest::EndBrowse { cursor, .. } => {
                (Some(cursor.clone()), None, None)
            }
            _ => (None, None, None),
        };
        Self {
            dataset: dataset.into(),
            cursor,
            selected_key,
            seed,
            operation: request.operation,
        }
    }

    fn validate(
        &self,
        cursor: &str,
        record: &Option<Vec<u8>>,
        identity: &Option<Vec<u8>>,
        key: &Option<Vec<u8>>,
    ) -> Result<(), HostProblem> {
        if cursor.is_empty()
            || cursor.len() > mainframe_env_host_api::HostLimits::default().max_name_bytes
        {
            return Err(HostProblem::ProviderFailure);
        }
        if self
            .cursor
            .as_deref()
            .is_some_and(|expected| expected != cursor)
        {
            return Err(HostProblem::ProviderFailure);
        }
        let complete = record.is_some() && identity.is_some() && key.is_some();
        let empty = record.is_none() && identity.is_none() && key.is_none();
        if !complete && !empty {
            return Err(HostProblem::Malformed);
        }
        if matches!(
            self.operation,
            CicsOperation::StartBrowse | CicsOperation::ResetBrowse | CicsOperation::EndBrowse
        ) && !empty
        {
            return Err(HostProblem::ProviderFailure);
        }
        if let Some(expected) = &self.selected_key
            && (!complete || key.as_ref() != Some(expected))
        {
            return Err(HostProblem::ProviderFailure);
        }
        Ok(())
    }

    fn completion(&self, cursor: &str) -> Completion {
        match self.operation {
            CicsOperation::StartBrowse | CicsOperation::ResetBrowse => Completion::Seed {
                dataset: self.dataset.clone(),
                cursor: cursor.into(),
                key: self.seed.clone(),
            },
            CicsOperation::ReadNext | CicsOperation::ReadPrev => Completion::Consume {
                dataset: self.dataset.clone(),
                cursor: cursor.into(),
            },
            CicsOperation::EndBrowse => Completion::End {
                dataset: self.dataset.clone(),
                cursor: cursor.into(),
            },
            _ => Completion::Keep,
        }
    }
}

pub(super) fn file(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    completion: &mut Completion,
) -> Result<CicsResponse, HostProblem> {
    if request.operation == CicsOperation::StartBrowse
        && run.file_updates.task_browses.len() >= service.limits.max_file_aliases
    {
        return Err(HostProblem::ResourceExhausted);
    }
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
            crate::service::access_for(request.operation),
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
        CicsOperation::ReadNext | CicsOperation::ReadPrev => {
            let cursor = owned_browse_cursor(run, request, &dataset_key, 0)?;
            selected_request(run, request, &dataset, &cursor, attributes.as_ref(), ccsid)?
                .unwrap_or(DatasetRequest::ReadNext {
                    dataset: dataset.clone(),
                    cursor,
                    reverse: request.operation == CicsOperation::ReadPrev,
                    control: Default::default(),
                })
        }
        CicsOperation::EndBrowse => {
            let cursor = owned_browse_cursor(run, request, &dataset_key, 0)?;
            if run
                .file_updates
                .task_browses
                .get(&(dataset_key.clone(), cursor.clone()))
                .is_some_and(|owner| owner.retirement_unknown)
            {
                return Err(HostProblem::UnknownOutcome);
            }
            super::task_retirement::validate_generation(
                service,
                &run.current_program.effect_invocation,
            )?;
            DatasetRequest::EndBrowse {
                dataset: dataset.clone(),
                cursor,
            }
        }
        _ => return Err(HostProblem::Malformed),
    };
    let plan = BrowsePlan::new(request, &host_request, &dataset_key);
    let mut validated_cursor = None;
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
    .map_err(|problem| {
        if operation == CicsOperation::EndBrowse
            && super::task_retirement::ambiguous(&problem)
            && let Some(cursor) = &plan.cursor
        {
            super::task_retirement::mark_unknown(run, &dataset_key, cursor);
        }
        normalize_file_not_found(operation, problem)
    })?;
    if matches!(
        operation,
        CicsOperation::StartBrowse
            | CicsOperation::ResetBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse
    ) && !matches!(&result, HostResult::Dataset(DatasetResult::Browse { .. }))
    {
        if operation == CicsOperation::EndBrowse
            && let Some(cursor) = &plan.cursor
        {
            super::task_retirement::mark_unknown(run, &dataset_key, cursor);
        }
        return Err(HostProblem::ProviderFailure);
    }
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
            if let Err(problem) = plan.validate(&cursor, &record, &identity, &key) {
                if operation == CicsOperation::EndBrowse
                    && let Some(cursor) = &plan.cursor
                {
                    super::task_retirement::mark_unknown(run, &dataset_key, cursor);
                }
                return Err(problem);
            }
            validated_cursor = Some(cursor.clone());
            if operation == CicsOperation::StartBrowse {
                record_start_owner(run, &dataset_key, &cursor);
            } else if operation == CicsOperation::ResetBrowse {
                run.current_records.remove(&dataset_key);
                run.file_updates.current_record_values.remove(&dataset_key);
                file_tokens::invalidate_dataset(run, &dataset_key);
            } else if operation == CicsOperation::EndBrowse {
                record_end_owner(run, &dataset_key);
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
                // Fully empty ordinary EOF is deliberately routed through the unchanged policy.
                // Selected empty replies have already failed binding above.
                *completion = plan.completion(&cursor);
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
    let response = (|| {
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
                crate::service::decimal_payload(i64::from(token))?,
            );
        }
        if matches!(
            operation,
            CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
        ) && length.is_some()
        {
            response.outputs.insert(
                "LENGTH".into(),
                crate::service::decimal_payload(i64::from(actual_length))?,
            );
        }
        Ok(response)
    })();
    finish_response(response, &plan, validated_cursor.as_deref(), completion)
}

fn finish_response(
    result: Result<CicsResponse, HostProblem>,
    plan: &BrowsePlan,
    validated_cursor: Option<&str>,
    completion: &mut Completion,
) -> Result<CicsResponse, HostProblem> {
    if result.is_ok()
        && let Some(cursor) = validated_cursor
    {
        *completion = plan.completion(cursor);
    }
    result
}

impl CicsService {
    pub(in crate::service) fn invoke_run(
        &self,
        run: &mut Run,
        request: CicsRequest,
        retention_tick: u64,
    ) -> Result<CicsResponse, HostProblem> {
        if let Some(mutation) = &request.mutation
            && mutation.transaction.as_deref() != Some(run.transaction.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let descriptor = handlers::authorize_and_describe(self, run, &request)?;
        let family = descriptor.family;
        handlers::preflight_conversation(self, run, family)?;
        let mut file_completion = None;
        let result = match family {
            CicsCommandFamily::TaskControl | CicsCommandFamily::StorageControl => {
                handlers::invoke_task_control(self, run, &request, retention_tick)
            }
            CicsCommandFamily::Time => handlers::invoke_time(self, run, &request),
            CicsCommandFamily::OperatorControl => handlers::invoke_operator(self, run, &request),
            CicsCommandFamily::NetworkControl => handlers::invoke_network(self, run, &request),
            CicsCommandFamily::ProgramControl => program(self, run, &request),
            CicsCommandFamily::TerminalControl => terminal(self, run, &request),
            CicsCommandFamily::FileControl => {
                let (result, completion) =
                    handlers::invoke_file_control(self, run, &request).into_parts();
                file_completion = Some(completion);
                result
            }
            CicsCommandFamily::QueueControl => handlers::invoke_queue_control(self, run, &request),
            CicsCommandFamily::CounterControl => handlers::invoke_counter(self, run, &request),
            CicsCommandFamily::Recovery => {
                handlers::invoke_recovery(self, run, &request, retention_tick)
            }
            CicsCommandFamily::IntervalControl | CicsCommandFamily::SpoolControl => {
                handlers::invoke_interval_or_spool_control(self, run, &request, family)
            }
            CicsCommandFamily::DocumentControl => {
                handlers::invoke_document_control(self, run, &request, retention_tick)
            }
            CicsCommandFamily::TransformControl
            | CicsCommandFamily::JournalControl
            | CicsCommandFamily::WebServiceControl
            | CicsCommandFamily::WebControl
            | CicsCommandFamily::BtsControl
            | CicsCommandFamily::EventControl
            | CicsCommandFamily::Diagnostics
            | CicsCommandFamily::SecurityControl
            | CicsCommandFamily::BuiltinFunctionControl
            | CicsCommandFamily::ConversationControl => handlers::invoke_extended_control(
                self,
                run,
                &request,
                descriptor.family,
                retention_tick,
            ),
        }
        .or_else(|problem| handlers::condition_for_request(self, run, &request, problem));
        if let Some(completion) = file_completion {
            completion.commit(run, &result);
        }
        result
    }
}

#[cfg(test)]
mod tests;
