use super::*;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request, CicsOperation::TransformJsonToData)?;
    let channel = request_name(request, "CHANNEL", 16, "CHANNELERR", 122, 1)?;
    let input = request_name(request, "INCONTAINER", 16, "CONTAINERERR", 110, 1)?;
    let output = match request.arguments.get("OUTCONTAINER") {
        Some(_) => request_name(request, "OUTCONTAINER", 16, "CONTAINERERR", 110, 0)?,
        None => DEFAULT_JSON_DATA_OUTPUT.into(),
    };
    let transformer = request_name(request, "TRANSFORMER", 16, "INVREQ", 16, 4)?;
    match service.authorize(
        run,
        "TRANSFORM",
        &format!("CICS.JSON.{transformer}"),
        AccessIntent::Update,
    ) {
        Ok(()) => {}
        Err(HostProblem::Unauthorized) => return condition("INVREQ", 16, 101),
        Err(problem) => return Err(problem),
    }

    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    if let Some(effect) = replay_effect(service, effect_key, request_digest, request.operation)? {
        return normal_response(service, run, effect.outputs);
    }

    let (definition, source) = {
        let state = service.lock()?;
        if !state
            .transform_containers
            .keys()
            .any(|(candidate, _)| candidate == &channel)
        {
            return condition("CHANNELERR", 122, 2);
        }
        let source = match state
            .transform_containers
            .get(&(channel.clone(), input.clone()))
        {
            Some(container) => container.clone(),
            None => return condition("CONTAINERERR", 110, 1),
        };
        if source.mode == CicsTransformContainerMode::Bit
            && std::str::from_utf8(&source.bytes).is_err()
        {
            return condition("INVREQ", 16, 7);
        }
        let definition = match state
            .transform_resources
            .get(&(CicsTransformFormat::Json, transformer.clone()))
        {
            Some(definition) => definition.clone(),
            None => return condition("NOTFND", 13, 1),
        };
        if !definition.enabled {
            return condition("INVREQ", 16, 1);
        }
        (definition, source)
    };
    let transformed = match data_from_json(&definition, &source.bytes, service.limits) {
        Ok(bytes) => bytes,
        Err(HostProblem::ResourceExhausted) => return Err(HostProblem::ResourceExhausted),
        Err(_) => return condition("INVREQ", 16, 4),
    };
    persist_effect_and_output(
        service,
        &channel,
        &output,
        effect_key,
        TransformEffect {
            operation: request.operation,
            request_digest,
            outputs: BTreeMap::new(),
        },
        CicsTransformContainerMode::Bit,
        transformed,
    )?;
    normal_response(service, run, BTreeMap::new())
}

fn data_from_json(
    definition: &CicsTransformDefinition,
    json: &[u8],
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    let value: Value = serde_json::from_slice(json).map_err(|_| HostProblem::Malformed)?;
    let object = value.as_object().ok_or(HostProblem::Malformed)?;
    if object.len() != definition.fields.len() {
        return Err(HostProblem::Malformed);
    }
    let length = definition
        .fields
        .iter()
        .map(|field| field.offset.checked_add(field.length))
        .collect::<Option<Vec<_>>>()
        .ok_or(HostProblem::ResourceExhausted)?
        .into_iter()
        .max()
        .ok_or(HostProblem::Malformed)?;
    if length > limits.max_transform_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut data = vec![b' '; length];
    for field in &definition.fields {
        let value = object.get(&field.name).ok_or(HostProblem::Malformed)?;
        let encoded = match field.kind {
            CicsTransformFieldKind::Text => value
                .as_str()
                .ok_or(HostProblem::Malformed)?
                .as_bytes()
                .to_vec(),
            CicsTransformFieldKind::SignedInteger => {
                let number = value.as_i64().ok_or(HostProblem::Malformed)?;
                format!("{number:0width$}", width = field.length).into_bytes()
            }
        };
        if encoded.len() > field.length {
            return Err(HostProblem::Malformed);
        }
        let end = field.offset + field.length;
        let target = &mut data[field.offset..end];
        target[..encoded.len()].copy_from_slice(&encoded);
    }
    Ok(data)
}
