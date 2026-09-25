use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let updates = if request.arguments.contains_key("SYMBOLLIST") {
        symbol_definitions(service, request, "LENGTH")?
    } else {
        single_symbol(service, request)?
    };
    let mut state = service.lock()?;
    let original = document_for_token(
        &state.documents,
        run,
        request.arguments["DOCTOKEN"].bytes(),
        1,
    )?
    .clone();
    let mut document = original.clone();
    document.symbols.extend(updates);
    if document.symbols.len() > service.limits.max_document_symbols {
        return Err(HostProblem::ResourceExhausted);
    }
    document.version = original
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let usage = document.usage_bytes();
    let remaining = state
        .document_bytes
        .checked_sub(original.usage_bytes())
        .ok_or(HostProblem::InfrastructureFailure)?;
    let total = remaining
        .checked_add(usage)
        .ok_or(HostProblem::ResourceExhausted)?;
    if usage > service.limits.max_screen_bytes || total > service.limits.max_document_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    let key = token_key(&document.token);
    service
        .store
        .mutate_provider_states_atomic(vec![
            ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DOCUMENT_NAMESPACE.into(),
                    key: key.clone(),
                    version: document.version,
                    payload: encode_document(&document)?,
                },
                expected_version: Some(original.version),
            }),
            ProviderStateMutation::Put(replay_write(run, request, retention_tick, &response)?),
        ])
        .map_err(store_error)?;
    state.documents.insert(key, document);
    state.document_bytes = total;
    Ok(response)
}

fn single_symbol(
    service: &CicsService,
    request: &CicsRequest,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    let value = request
        .arguments
        .get("VALUE")
        .ok_or(HostProblem::Malformed)?;
    let length = unsigned_length(request, "LENGTH", 10, false)?;
    if length > value.bytes().len() || length > service.limits.max_screen_bytes {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 10,
        });
    }
    let name = normalize_symbol(request.arguments["SYMBOL"].bytes()).map_err(|_| {
        HostProblem::Condition {
            name: "SYMBOLERR".into(),
            response: 116,
            response2: 0,
        }
    })?;
    let bytes = if request.arguments.contains_key("OPTION.UNESCAPED") {
        value.bytes()[..length].to_vec()
    } else {
        unescape_symbol(&value.bytes()[..length])
    };
    Ok(BTreeMap::from([(name, bytes)]))
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let single =
        request.arguments.contains_key("SYMBOL") && request.arguments.contains_key("VALUE");
    let list = request.arguments.contains_key("SYMBOLLIST");
    if !request.arguments.contains_key("DOCTOKEN")
        || !request.arguments.contains_key("LENGTH")
        || single == list
        || request.arguments.contains_key("SYMBOL") != request.arguments.contains_key("VALUE")
        || request.arguments.contains_key("DELIMITER") && !list
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "DOCTOKEN" | "VALUE" | "SYMBOLLIST" => {
                value.schema() == "mainframe-env.cics.storage-value@1"
            }
            "SYMBOL" | "DELIMITER" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "LENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" | "OPTION.UNESCAPED" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
