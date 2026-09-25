use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let code_page = host_code_page(request)?;
    let template = if let Some(value) = request.arguments.get("TEMPLATE") {
        let name =
            normalize_template_name(std::str::from_utf8(value.bytes()).map_err(|_| not_found(3))?)
                .map_err(|_| not_found(3))?;
        let definition = service
            .lock()?
            .document_templates
            .get(&name)
            .cloned()
            .ok_or_else(|| not_found(3))?;
        service.authorize(
            run,
            "DOCTEMPLATE",
            &format!("CICS.DOCTEMPLATE.{}", definition.resource),
            AccessIntent::Read,
        )?;
        Some(definition)
    } else {
        None
    };
    let mut state = service.lock()?;
    let original = document_for_token(
        &state.documents,
        run,
        request.arguments["DOCTOKEN"].bytes(),
        1,
    )?
    .clone();
    let source = if let Some(value) = request.arguments.get("FROMDOC") {
        Some(document_for_token(&state.documents, run, value.bytes(), 2)?.clone())
    } else {
        None
    };
    let (mut inserted, inserted_bookmarks) = content_segments(
        request,
        &original,
        source.as_ref(),
        template.as_ref(),
        code_page,
        service.limits,
    )?;
    let old_size = original.retrieval_size();
    let at = bookmark_position(&original, request, "AT", 5)?.unwrap_or(old_size);
    let to = bookmark_position(&original, request, "TO", 6)?.unwrap_or(at);
    if to < at {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        });
    }
    let bookmark = request
        .arguments
        .get("BOOKMARK")
        .map(|value| name(value.bytes(), 16))
        .transpose()
        .map_err(|_| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 2,
        })?;
    let retained_bookmark = |name: &str| {
        original
            .bookmarks
            .get(name)
            .is_some_and(|position| *position <= at || *position >= to)
    };
    if bookmark.as_deref() == Some("TOP")
        || bookmark
            .as_ref()
            .is_some_and(|name| retained_bookmark(name))
        || inserted_bookmarks
            .keys()
            .any(|name| retained_bookmark(name) || bookmark.as_ref() == Some(name))
    {
        return Err(HostProblem::Condition {
            name: "DUPREC".into(),
            response: 14,
            response2: 0,
        });
    }
    let inserted_size = inserted
        .iter()
        .map(|segment| segment.bytes.len())
        .sum::<usize>();
    let mut document = original.clone();
    document.segments = slice_segments(&original.segments, 0, at);
    document.segments.append(&mut inserted);
    document
        .segments
        .extend(slice_segments(&original.segments, to, old_size));
    document
        .bookmarks
        .retain(|_, position| *position <= at || *position >= to);
    for position in document.bookmarks.values_mut() {
        if *position > at {
            *position = position
                .checked_sub(to - at)
                .and_then(|value| value.checked_add(inserted_size))
                .ok_or(HostProblem::ResourceExhausted)?;
        }
    }
    for (name, position) in inserted_bookmarks {
        document.bookmarks.insert(
            name,
            at.checked_add(position)
                .ok_or(HostProblem::ResourceExhausted)?,
        );
    }
    if let Some(name) = bookmark {
        document.bookmarks.insert(name, at + inserted_size);
    }
    if document.bookmarks.len() > service.limits.max_document_bookmarks
        || document.segments.len() > service.limits.max_queue_records
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let new_size = transport::encode_retrieved(&document, service.limits)?.len();
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
    if request.arguments.contains_key("DOCSIZE") {
        response.outputs.insert(
            "DOCSIZE".into(),
            decimal_payload(i64::try_from(new_size).map_err(|_| HostProblem::ResourceExhausted)?)?,
        );
    }
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

fn content_segments(
    request: &CicsRequest,
    document: &DocumentRecord,
    source: Option<&DocumentRecord>,
    template: Option<&CicsDocumentTemplateDefinition>,
    code_page: u16,
    limits: CicsLimits,
) -> Result<(Vec<DocumentSegment>, BTreeMap<String, usize>), HostProblem> {
    if let Some(source) = source {
        return Ok((source.segments.clone(), source.bookmarks.clone()));
    }
    if let Some(template) = template {
        return Ok((
            vec![DocumentSegment {
                bytes: substitute_symbols(
                    &template.content,
                    &document.symbols,
                    limits.max_screen_bytes,
                )?,
                binary: false,
                host_code_page: if request.arguments.contains_key("HOSTCODEPAGE") {
                    code_page
                } else {
                    template.host_code_page
                },
            }],
            BTreeMap::new(),
        ));
    }
    if let Some(value) = request.arguments.get("SYMBOL") {
        let symbol = name(value.bytes(), 32).map_err(|_| not_found(4))?;
        let bytes = document
            .symbols
            .get(&symbol)
            .cloned()
            .ok_or_else(|| not_found(4))?;
        return Ok((
            vec![DocumentSegment {
                bytes,
                binary: false,
                host_code_page: code_page,
            }],
            BTreeMap::new(),
        ));
    }
    if let Some((name, binary)) = [("FROM", false), ("TEXT", false), ("BINARY", true)]
        .into_iter()
        .find(|(name, _)| request.arguments.contains_key(*name))
    {
        let bytes = request.arguments[name].bytes();
        let length = unsigned_length(request, "LENGTH", 1, true)?;
        if length > bytes.len() {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 1,
            });
        }
        if name == "FROM" {
            if let Some((segments, bookmarks)) =
                transport::decode_from_buffer(&bytes[..length], limits)?
            {
                return Ok((segments, bookmarks));
            }
            let bytes =
                substitute_symbols(&bytes[..length], &document.symbols, limits.max_screen_bytes)?;
            return Ok((
                vec![DocumentSegment {
                    bytes,
                    binary: false,
                    host_code_page: code_page,
                }],
                BTreeMap::new(),
            ));
        }
        return Ok((
            vec![DocumentSegment {
                bytes: bytes[..length].to_vec(),
                binary,
                host_code_page: code_page,
            }],
            BTreeMap::new(),
        ));
    }
    Ok((Vec::new(), BTreeMap::new()))
}

fn bookmark_position(
    document: &DocumentRecord,
    request: &CicsRequest,
    option: &str,
    response2: i32,
) -> Result<Option<usize>, HostProblem> {
    let Some(value) = request.arguments.get(option) else {
        return Ok(None);
    };
    let name = name(value.bytes(), 16).map_err(|_| not_found(response2))?;
    if name == "TOP" {
        return Ok(Some(0));
    }
    document
        .bookmarks
        .get(&name)
        .copied()
        .map(Some)
        .ok_or_else(|| not_found(response2))
}

fn name(bytes: &[u8], maximum: usize) -> Result<String, HostProblem> {
    let value = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
    let value = value.trim_end().to_string();
    if value.is_empty()
        || value.len() > maximum
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(HostProblem::Malformed);
    }
    Ok(value)
}

fn not_found(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "NOTFND".into(),
        response: 13,
        response2,
    }
}

fn slice_segments(segments: &[DocumentSegment], start: usize, end: usize) -> Vec<DocumentSegment> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    for segment in segments {
        let next = offset + segment.bytes.len();
        let lower = start.max(offset);
        let upper = end.min(next);
        if lower < upper {
            result.push(DocumentSegment {
                bytes: segment.bytes[lower - offset..upper - offset].to_vec(),
                binary: segment.binary,
                host_code_page: segment.host_code_page,
            });
        }
        offset = next;
    }
    result
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let sources = ["FROM", "TEXT", "BINARY", "FROMDOC", "TEMPLATE", "SYMBOL"]
        .into_iter()
        .filter(|name| request.arguments.contains_key(*name))
        .count();
    let buffered = ["FROM", "TEXT", "BINARY"]
        .into_iter()
        .any(|name| request.arguments.contains_key(name));
    if !request.arguments.contains_key("DOCTOKEN")
        || sources > 1
        || sources == 0 && !request.arguments.contains_key("BOOKMARK")
        || request.arguments.contains_key("LENGTH") != buffered
        || request.arguments.contains_key("HOSTCODEPAGE")
            && !["TEXT", "SYMBOL", "TEMPLATE"]
                .into_iter()
                .any(|name| request.arguments.contains_key(name))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "DOCTOKEN" | "FROMDOC" | "FROM" | "TEXT" | "BINARY" => {
                value.schema() == "mainframe-env.cics.storage-value@1"
            }
            "TEMPLATE" | "SYMBOL" | "BOOKMARK" | "AT" | "TO" | "HOSTCODEPAGE" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "LENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "DOCSIZE" | "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
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
