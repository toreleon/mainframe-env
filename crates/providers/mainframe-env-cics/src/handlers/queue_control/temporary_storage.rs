use super::*;
use mainframe_env_store_api::ProviderStateRecord;

pub(super) fn delete_temporary(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "QNAME" | "QUEUE" | "SYSID" => false,
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    let queue = temporary_queue_name(request)?;
    validate_temporary_system(run, request, 0)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TS.{queue}"),
        AccessIntent::Update,
    )?;
    let current = service
        .store
        .get_provider_state("cics-tsq", &queue)
        .map_err(store_error)?
        .ok_or_else(|| HostProblem::Condition {
            name: "QIDERR".into(),
            response: 44,
            response2: 0,
        })?;
    decode_transient(&current.payload, current.version, service.limits)?;
    service
        .store
        .delete_provider_state("cics-tsq", &queue, current.version)
        .map_err(store_error)
        .map_err(mutation_problem)?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemporaryPlacement {
    Auxiliary,
    Main,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemporaryLock {
    None,
    Recovery,
    Indoubt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TemporaryQueue {
    items: Vec<(String, Vec<u8>)>,
    next_item: usize,
    placement: TemporaryPlacement,
    lock: TemporaryLock,
    version: u64,
}

fn encode_temporary(queue: &TemporaryQueue) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"METS1".to_vec();
    out.push(match queue.placement {
        TemporaryPlacement::Auxiliary => 0,
        TemporaryPlacement::Main => 1,
    });
    out.push(match queue.lock {
        TemporaryLock::None => 0,
        TemporaryLock::Recovery => 1,
        TemporaryLock::Indoubt => 2,
    });
    out.extend_from_slice(
        &u32::try_from(queue.next_item)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(queue.items.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (effect_key, value) in &queue.items {
        super::super::field(&mut out, effect_key.as_bytes())?;
        super::super::field(&mut out, value)?;
    }
    Ok(out)
}

fn decode_temporary(
    bytes: &[u8],
    version: u64,
    limits: super::super::super::CicsLimits,
) -> Result<TemporaryQueue, HostProblem> {
    if bytes.starts_with(b"MECT2") {
        let legacy = decode_transient(bytes, version, limits)?;
        return Ok(TemporaryQueue {
            items: legacy.records,
            next_item: 0,
            placement: TemporaryPlacement::Auxiliary,
            lock: TemporaryLock::None,
            version,
        });
    }
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(5)? != b"METS1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let placement = match reader.take(1)?[0] {
        0 => TemporaryPlacement::Auxiliary,
        1 => TemporaryPlacement::Main,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let lock = match reader.take(1)?[0] {
        0 => TemporaryLock::None,
        1 => TemporaryLock::Recovery,
        2 => TemporaryLock::Indoubt,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let next_item = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if count == 0 || count > limits.max_queue_records || next_item > count || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut items = Vec::with_capacity(count);
    let mut total = 0usize;
    for _ in 0..count {
        let effect_key = String::from_utf8(reader.field(256)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = reader.field(limits.max_queue_bytes)?;
        total = total
            .checked_add(value.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if effect_key.is_empty() || value.is_empty() || total > limits.max_queue_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
        items.push((effect_key, value));
    }
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(TemporaryQueue {
        items,
        next_item,
        placement,
        lock,
        version,
    })
}

fn decimal_argument(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn invalid_padded_storage_name(request: &CicsRequest) -> bool {
    [("QUEUE", 8), ("QNAME", 16)].iter().any(|(name, length)| {
        request.arguments.get(*name).is_some_and(|value| {
            value.schema() == "mainframe-env.cics.storage-value@1" && value.bytes().len() != *length
        })
    })
}

fn validate_read_temporary_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("QUEUE") == request.arguments.contains_key("QNAME")
        || request.arguments.contains_key("INTO") == request.arguments.contains_key("SET")
        || !request.arguments.contains_key("LENGTH")
        || request.arguments.contains_key("ITEM") && request.arguments.contains_key("OPTION.NEXT")
        || invalid_padded_storage_name(request)
        || request.arguments.contains_key("SET.MAXLENGTH") != request.arguments.contains_key("SET")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "QNAME" | "QUEUE" | "SYSID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "ITEM" | "LENGTH" | "SET.MAXLENGTH" => {
                    value.schema() != "mainframe-env.cics.decimal@1"
                }
                "INTO" | "SET" | "NUMITEMS" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "OPTION.NEXT" | "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(super) fn read_temporary(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_read_temporary_request(request)?;
    let queue_name = temporary_queue_name(request)?;
    validate_temporary_system(run, request, 4)?;
    service
        .authorize(
            run,
            "QUEUE",
            &format!("CICS.TS.{queue_name}"),
            AccessIntent::Read,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 101,
            },
            other => other,
        })?;
    let _guard = service.lock()?;
    let current = service
        .store
        .get_provider_state("cics-tsq", &queue_name)
        .map_err(store_error)?
        .ok_or_else(|| HostProblem::Condition {
            name: "QIDERR".into(),
            response: 44,
            response2: 0,
        })?;
    let mut queue = decode_temporary(&current.payload, current.version, service.limits)?;
    let index = if request.arguments.contains_key("ITEM") {
        let item = decimal_argument(request, "ITEM")?;
        usize::try_from(item)
            .ok()
            .and_then(|item| item.checked_sub(1))
            .filter(|index| *index < queue.items.len())
            .ok_or_else(|| HostProblem::Condition {
                name: "ITEMERR".into(),
                response: 26,
                response2: 0,
            })?
    } else {
        if queue.next_item >= queue.items.len() {
            return Err(HostProblem::Condition {
                name: "ITEMERR".into(),
                response: 26,
                response2: 0,
            });
        }
        queue.next_item
    };
    let mut value = queue.items[index].1.clone();
    let actual = value.len();
    if request.arguments.contains_key("SET") {
        let capacity = usize::try_from(decimal_argument(request, "SET.MAXLENGTH")?)
            .map_err(|_| HostProblem::Malformed)?;
        if actual > capacity {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    queue.next_item = index + 1;
    queue.version = queue
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-tsq".into(),
                key: queue_name,
                version: queue.version,
                payload: encode_temporary(&queue)?,
            },
            Some(current.version),
        )
        .map_err(store_error)
        .map_err(mutation_problem)?;

    let mut condition = ("NORMAL", 0, 0);
    if request.arguments.contains_key("INTO") {
        let maximum = usize::try_from(decimal_argument(request, "LENGTH")?.max(0))
            .map_err(|_| HostProblem::Malformed)?;
        if value.len() > maximum {
            value.truncate(maximum);
            condition = ("LENGERR", 22, 0);
        }
    }
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        condition.0,
        condition.1,
        condition.2,
        None,
        None,
        value.clone(),
    )?;
    response.outputs.insert(
        "LENGTH".into(),
        decimal_payload(i64::try_from(actual).map_err(|_| HostProblem::ResourceExhausted)?)?,
    );
    if request.arguments.contains_key("SET") {
        response.outputs.insert("SET".into(), bounded(value)?);
    }
    if condition.1 == 0 && request.arguments.contains_key("NUMITEMS") {
        response.outputs.insert(
            "NUMITEMS".into(),
            decimal_payload(
                i64::try_from(queue.items.len()).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    Ok(response)
}

fn validate_write_temporary_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let rewrite = request.arguments.contains_key("OPTION.REWRITE");
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("QUEUE") == request.arguments.contains_key("QNAME")
        || !request.arguments.contains_key("FROM")
        || !request.arguments.contains_key("LENGTH")
        || rewrite && !request.arguments.contains_key("ITEM")
        || rewrite && request.arguments.contains_key("NUMITEMS")
        || request.arguments.contains_key("OPTION.AUXILIARY")
            && request.arguments.contains_key("OPTION.MAIN")
        || invalid_padded_storage_name(request)
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "QNAME" | "QUEUE" | "SYSID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "FROM" => value.schema() != "mainframe-env.cics.storage-value@1",
                "ITEM" | "LENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "NUMITEMS" | "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.AUXILIARY" | "OPTION.MAIN" | "OPTION.NOHANDLE" | "OPTION.NOSUSPEND"
                | "OPTION.REWRITE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn temporary_lock_problem(lock: TemporaryLock) -> Option<HostProblem> {
    match lock {
        TemporaryLock::None => None,
        TemporaryLock::Recovery => Some(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        }),
        TemporaryLock::Indoubt => Some(HostProblem::Condition {
            name: "LOCKED".into(),
            response: 100,
            response2: 0,
        }),
    }
}

fn temporary_usage(service: &CicsService) -> Result<(usize, usize), HostProblem> {
    let maximum = service
        .limits
        .max_queue_records
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = service
        .store
        .list_provider_state("cics-tsq", maximum)
        .map_err(store_error)?;
    if rows.len() > service.limits.max_queue_records {
        return Ok((
            service.limits.max_queue_records,
            service.limits.max_queue_bytes,
        ));
    }
    rows.into_iter().try_fold((0usize, 0usize), |usage, row| {
        let queue = decode_temporary(&row.payload, row.version, service.limits)?;
        let records = usage
            .0
            .checked_add(queue.items.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        let bytes = queue
            .items
            .into_iter()
            .try_fold(usage.1, |total, (_, value)| {
                total
                    .checked_add(value.len())
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
        Ok((records, bytes))
    })
}

fn no_temporary_space(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let active_handle = matches!(request.condition_policy, CicsConditionPolicy::Default)
        && run.handlers.contains_key("NOSPACE");
    let no_suspend = request.arguments.contains_key("OPTION.NOSUSPEND");
    if no_suspend
        && matches!(request.condition_policy, CicsConditionPolicy::Default)
        && !active_handle
    {
        return service.response(
            run,
            CicsDisposition::Ignored,
            "NOSPACE",
            18,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    if no_suspend || active_handle {
        return super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "NOSPACE".into(),
                response: 18,
                response2: 0,
            },
        );
    }
    service.response(
        run,
        CicsDisposition::Suspended,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn write_temporary_response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    item_number: usize,
    item_count: usize,
) -> Result<CicsResponse, HostProblem> {
    let rewrite = request.arguments.contains_key("OPTION.REWRITE");
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    if request.arguments.contains_key("NUMITEMS") {
        response.outputs.insert(
            "NUMITEMS".into(),
            decimal_payload(
                i64::try_from(item_count).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    if !rewrite && request.arguments.contains_key("ITEM") {
        response.outputs.insert(
            "ITEM".into(),
            decimal_payload(
                i64::try_from(item_number).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    Ok(response)
}

pub(super) fn write_temporary(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_write_temporary_request(request)?;
    let queue_name = temporary_queue_name(request)?;
    validate_temporary_system(run, request, 4)?;
    service
        .authorize(
            run,
            "QUEUE",
            &format!("CICS.TS.{queue_name}"),
            AccessIntent::Update,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 101,
            },
            other => other,
        })?;
    let requested_length = decimal_argument(request, "LENGTH")?;
    if !(1..=32_763).contains(&requested_length) {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    let mut value = argument_bytes(request, "FROM").ok_or(HostProblem::Malformed)?;
    let requested_length = usize::try_from(requested_length).map_err(|_| HostProblem::Malformed)?;
    if requested_length > value.len() {
        return Err(HostProblem::Malformed);
    }
    value.truncate(requested_length);
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let rewrite = request.arguments.contains_key("OPTION.REWRITE");
    let _guard = service.lock()?;
    let current = service
        .store
        .get_provider_state("cics-tsq", &queue_name)
        .map_err(store_error)?;
    if rewrite && current.is_none() {
        return Err(HostProblem::Condition {
            name: "QIDERR".into(),
            response: 44,
            response2: 0,
        });
    }
    let (mut queue, expected) = match current {
        Some(record) => {
            let queue = decode_temporary(&record.payload, record.version, service.limits)?;
            if let Some(problem) = temporary_lock_problem(queue.lock) {
                return Err(problem);
            }
            (queue, Some(record.version))
        }
        None => (
            TemporaryQueue {
                items: Vec::new(),
                next_item: 0,
                placement: if request.arguments.contains_key("OPTION.MAIN") {
                    TemporaryPlacement::Main
                } else {
                    TemporaryPlacement::Auxiliary
                },
                lock: TemporaryLock::None,
                version: 0,
            },
            None,
        ),
    };
    let rewrite_item = if rewrite {
        Some(
            usize::try_from(decimal_argument(request, "ITEM")?)
                .ok()
                .and_then(|item| item.checked_sub(1))
                .filter(|index| *index < queue.items.len())
                .ok_or_else(|| HostProblem::Condition {
                    name: "ITEMERR".into(),
                    response: 26,
                    response2: 0,
                })?,
        )
    } else {
        None
    };
    if let Some((index, (_, prior))) = queue
        .items
        .iter()
        .enumerate()
        .find(|(_, (key, _))| key == effect_key)
    {
        if prior != &value || rewrite_item.is_some_and(|item| item != index) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let item_number = index + 1;
        let item_count = if rewrite {
            queue.items.len()
        } else {
            item_number
        };
        return write_temporary_response(service, run, request, item_number, item_count);
    }
    let (records, bytes) = temporary_usage(service)?;
    let item_number = if let Some(item) = rewrite_item {
        let previous = queue.items[item].1.len();
        let next_bytes = bytes
            .checked_sub(previous)
            .and_then(|total| total.checked_add(value.len()))
            .ok_or(HostProblem::ResourceExhausted)?;
        if next_bytes > service.limits.max_queue_bytes {
            return no_temporary_space(service, run, request);
        }
        queue.items[item] = (effect_key.into(), value);
        item + 1
    } else {
        let item = queue
            .items
            .len()
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let next_records = records
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let next_bytes = bytes
            .checked_add(value.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if item > 32_767
            || next_records > service.limits.max_queue_records
            || next_bytes > service.limits.max_queue_bytes
        {
            if item > 32_767 {
                return Err(HostProblem::Condition {
                    name: "ITEMERR".into(),
                    response: 26,
                    response2: 0,
                });
            }
            return no_temporary_space(service, run, request);
        }
        queue.items.push((effect_key.into(), value));
        item
    };
    queue.version = queue
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-tsq".into(),
                key: queue_name,
                version: queue.version,
                payload: encode_temporary(&queue)?,
            },
            expected,
        )
        .map_err(store_error)
        .map_err(mutation_problem)?;
    write_temporary_response(service, run, request, item_number, queue.items.len())
}
