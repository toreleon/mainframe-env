use super::*;

mod parser;
use parser::{XmlReadProblem, data_from_xml, parse_xml};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let channel = request_name(request, "CHANNEL", 16, "CHANNELERR", 122, 1)?;
    let xml_name = request_name(request, "XMLCONTAINER", 16, "CONTAINERERR", 110, 1)?;
    let namespace_name = request
        .arguments
        .contains_key("NSCONTAINER")
        .then(|| request_name(request, "NSCONTAINER", 16, "CONTAINERERR", 110, 2))
        .transpose()?;
    let transformer = request
        .arguments
        .contains_key("XMLTRANSFORM")
        .then(|| request_name(request, "XMLTRANSFORM", 32, "INVREQ", 16, 4))
        .transpose()?;
    let output = request
        .arguments
        .contains_key("DATCONTAINER")
        .then(|| request_name(request, "DATCONTAINER", 16, "CONTAINERERR", 110, 3))
        .transpose()?;
    if transformer.is_some() && output.is_none() {
        return condition("INVREQ", 16, 16);
    }
    if let Some(transformer) = transformer.as_deref() {
        match service.authorize(
            run,
            "TRANSFORM",
            &format!("CICS.XML.{transformer}"),
            AccessIntent::Update,
        ) {
            Ok(()) => {}
            Err(HostProblem::Unauthorized) => return condition("INVREQ", 16, 101),
            Err(problem) => return Err(problem),
        }
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

    let (xml, namespace_bytes, definition) = {
        let state = service.lock()?;
        if !state
            .transform_containers
            .keys()
            .any(|(candidate, _)| candidate == &channel)
        {
            return condition("CHANNELERR", 122, 2);
        }
        let xml = match state
            .transform_containers
            .get(&(channel.clone(), xml_name.clone()))
        {
            Some(container) => container.clone(),
            None => return condition("CONTAINERERR", 110, 1),
        };
        if xml.bytes.is_empty() {
            return condition("INVREQ", 16, 2);
        }
        if xml.mode == CicsTransformContainerMode::Bit && std::str::from_utf8(&xml.bytes).is_err() {
            return condition("INVREQ", 16, 7);
        }
        let namespace_bytes = match namespace_name.as_deref() {
            Some(name) => {
                let namespace = match state
                    .transform_containers
                    .get(&(channel.clone(), name.to_string()))
                {
                    Some(container) => container,
                    None => return condition("CONTAINERERR", 110, 2),
                };
                if namespace.mode != CicsTransformContainerMode::Char {
                    return condition("INVREQ", 16, 7);
                }
                Some(namespace.bytes.clone())
            }
            None => None,
        };
        let definition = match transformer.as_deref() {
            Some(name) => {
                let definition = match state
                    .transform_resources
                    .get(&(CicsTransformFormat::Xml, name.to_string()))
                {
                    Some(definition) => definition.clone(),
                    None => return condition("NOTFND", 13, 1),
                };
                if !definition.enabled {
                    return condition("INVREQ", 16, 1);
                }
                Some(definition)
            }
            None => None,
        };
        (xml, namespace_bytes, definition)
    };
    let source = std::str::from_utf8(&xml.bytes).map_err(|_| HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 3,
    })?;
    let namespace_source = namespace_bytes
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 3,
        })?;
    let parsed = match parse_xml(source, namespace_source, service.limits) {
        Ok(parsed) => parsed,
        Err(XmlReadProblem::Syntax) => return condition("INVREQ", 16, 3),
        Err(XmlReadProblem::Conversion) => return condition("INVREQ", 16, 4),
        Err(XmlReadProblem::ResourceExhausted) => return Err(HostProblem::ResourceExhausted),
    };
    let outputs = metadata_outputs(request, &parsed.metadata, false)?;
    let effect = TransformEffect {
        operation: request.operation,
        request_digest,
        outputs: outputs.clone(),
    };
    if let Some(definition) = definition {
        let bound = definition
            .xml
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if parsed.metadata.element_name != bound.element_name
            || parsed.metadata.element_namespace != bound.element_namespace
        {
            return condition("INVREQ", 16, 9);
        }
        let (type_name, type_namespace) = type_selection(request, &parsed.metadata)?;
        if type_name.as_deref() != bound.type_name.as_deref()
            || type_namespace.as_deref() != bound.type_namespace.as_deref()
        {
            return condition("INVREQ", 16, 10);
        }
        let data = match data_from_xml(&definition, &parsed, service.limits) {
            Ok(data) => data,
            Err(XmlReadProblem::ResourceExhausted) => return Err(HostProblem::ResourceExhausted),
            Err(_) => return condition("INVREQ", 16, 4),
        };
        persist_effect_and_output(
            service,
            &channel,
            output
                .as_deref()
                .ok_or(HostProblem::InfrastructureFailure)?,
            effect_key,
            effect,
            CicsTransformContainerMode::Bit,
            data,
        )?;
    } else {
        persist_query_effect(service, effect_key, effect)?;
    }
    normal_response(service, run, outputs)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "CHANNEL",
        "DATCONTAINER",
        "ELEMNAME",
        "ELEMNAMELEN",
        "ELEMNS",
        "ELEMNSLEN",
        "NSCONTAINER",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
        "TYPENAME",
        "TYPENAMELEN",
        "TYPENS",
        "TYPENSLEN",
        "XMLCONTAINER",
        "XMLTRANSFORM",
    ];
    if request.operation != CicsOperation::TransformXmlToData
        || request.mutation.is_none()
        || !request.arguments.contains_key("CHANNEL")
        || !request.arguments.contains_key("XMLCONTAINER")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || [
            ("ELEMNAME", "ELEMNAMELEN"),
            ("ELEMNS", "ELEMNSLEN"),
            ("TYPENAME", "TYPENAMELEN"),
            ("TYPENS", "TYPENSLEN"),
        ]
        .iter()
        .any(|(text, length)| {
            request.arguments.contains_key(*text) != request.arguments.contains_key(*length)
        })
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "CHANNEL" | "DATCONTAINER" | "NSCONTAINER" | "XMLCONTAINER"
                    | "XMLTRANSFORM" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    "ELEMNAME" | "ELEMNS" | "TYPENAME" | "TYPENS" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.argument@1"
                            | "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                    ),
                    "ELEMNAMELEN" | "ELEMNSLEN" | "TYPENAMELEN" | "TYPENSLEN" => {
                        value.schema() != "mainframe-env.cics.decimal@1"
                    }
                    "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    _ => true,
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn type_selection(
    request: &CicsRequest,
    xml: &CicsXmlTransformMetadata,
) -> Result<(Option<String>, Option<String>), HostProblem> {
    let override_name = request
        .arguments
        .get("TYPENAME")
        .map(|_| argument_text(request, "TYPENAME"))
        .transpose()?
        .map(|name| name.trim_end().to_string())
        .filter(|name| !name.is_empty());
    if let Some(name) = override_name {
        let namespace = request
            .arguments
            .get("TYPENS")
            .map(|_| argument_text(request, "TYPENS"))
            .transpose()?
            .map(|namespace| namespace.trim_end().to_string())
            .unwrap_or_default();
        Ok((Some(name), Some(namespace)))
    } else {
        Ok((xml.type_name.clone(), xml.type_namespace.clone()))
    }
}

fn persist_query_effect(
    service: &CicsService,
    effect_key: &str,
    effect: TransformEffect,
) -> Result<(), HostProblem> {
    let _state = service.lock()?;
    if let Some(existing) = service
        .store
        .get_provider_state(EFFECT_NAMESPACE, effect_key)
        .map_err(store_error)?
    {
        return if decode_effect(&existing.payload, service.limits)? == effect {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: EFFECT_NAMESPACE.into(),
                key: effect_key.into(),
                version: 1,
                payload: encode_effect(&effect)?,
            },
            None,
        )
        .map_err(store_error)
        .map_err(mutation_problem)
}
