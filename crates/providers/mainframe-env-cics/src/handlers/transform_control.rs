use super::super::{
    CicsLimits, CicsService, Run, argument_text, bounded, decimal_payload, mutation_problem,
};
use super::{field, store_error};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};

mod json_to_data;
mod reader;
mod xml_to_data;
use reader::Reader;

const RESOURCE_NAMESPACE: &str = "cics-transform-resource-v1";
const CONTAINER_NAMESPACE: &str = "cics-transform-container-v1";
const EFFECT_NAMESPACE: &str = "cics-transform-effect-v1";
const DEFAULT_JSON_OUTPUT: &str = "DFHJSON-JSON";
const DEFAULT_JSON_DATA_OUTPUT: &str = "DFHJSON-DATA";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XmlTransformProblem {
    ShortInput,
    InvalidData,
    Conversion,
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CicsTransformFormat {
    Json,
    Xml,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsTransformFieldKind {
    Text,
    SignedInteger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTransformFieldDefinition {
    pub name: String,
    pub offset: usize,
    pub length: usize,
    pub kind: CicsTransformFieldKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTransformDefinition {
    pub name: String,
    pub enabled: bool,
    pub format: CicsTransformFormat,
    pub fields: Vec<CicsTransformFieldDefinition>,
    pub xml: Option<CicsXmlTransformMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsXmlTransformMetadata {
    pub element_name: String,
    pub element_namespace: String,
    pub type_name: Option<String>,
    pub type_namespace: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsTransformContainerMode {
    Bit,
    Char,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransformContainer {
    pub(super) mode: CicsTransformContainerMode,
    pub(super) bytes: Vec<u8>,
    pub(super) version: u64,
}

pub(crate) type TransformContainerMap = BTreeMap<(String, String), TransformContainer>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TransformEffect {
    operation: CicsOperation,
    request_digest: [u8; 32],
    outputs: BTreeMap<String, Vec<u8>>,
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::TransformDataToJson => data_to_json(service, run, request),
        CicsOperation::TransformDataToXml => data_to_xml(service, run, request),
        CicsOperation::TransformJsonToData => json_to_data::invoke(service, run, request),
        CicsOperation::TransformXmlToData => xml_to_data::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(crate) fn load_resources(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<BTreeMap<(CicsTransformFormat, String), CicsTransformDefinition>, HostProblem> {
    let mut resources = BTreeMap::new();
    for row in store
        .list_provider_state(RESOURCE_NAMESPACE, limits.max_transform_resources)
        .map_err(store_error)?
    {
        if row.version != 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let definition = decode_definition(&row.payload, limits)?;
        let key = (definition.format, definition.name.clone());
        if row.key != resource_key(definition.format, &definition.name)
            || resources.insert(key, definition).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(resources)
}

pub(crate) fn load_containers(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(TransformContainerMap, usize), HostProblem> {
    let mut containers = BTreeMap::new();
    let mut bytes = 0usize;
    for row in store
        .list_provider_state(CONTAINER_NAMESPACE, limits.max_transform_containers)
        .map_err(store_error)?
    {
        let (channel, name) = split_container_key(&row.key)?;
        let container = decode_container(&row.payload, row.version, limits)?;
        bytes = bytes
            .checked_add(container.bytes.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > limits.max_transform_bytes
            || containers.insert((channel, name), container).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok((containers, bytes))
}

pub(crate) fn register_definition(
    service: &CicsService,
    definition: CicsTransformDefinition,
) -> Result<(), HostProblem> {
    let definition = normalize_definition(definition, service.limits)?;
    let key = (definition.format, definition.name.clone());
    let mut state = service.lock()?;
    if let Some(existing) = state.transform_resources.get(&key) {
        return if existing == &definition {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    if state.transform_resources.len() >= service.limits.max_transform_resources {
        return Err(HostProblem::ResourceExhausted);
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: RESOURCE_NAMESPACE.into(),
                key: resource_key(definition.format, &definition.name),
                version: 1,
                payload: encode_definition(&definition)?,
            },
            None,
        )
        .map_err(store_error)?;
    state.transform_resources.insert(key, definition);
    Ok(())
}

pub(crate) fn put_container(
    service: &CicsService,
    channel: &str,
    name: &str,
    mode: CicsTransformContainerMode,
    bytes: Vec<u8>,
) -> Result<(), HostProblem> {
    let channel = normalize_name(channel, 16)?;
    let name = normalize_name(name, 16)?;
    if bytes.len() > service.limits.max_transform_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let key = (channel.clone(), name.clone());
    let mut state = service.lock()?;
    if let Some(existing) = state.transform_containers.get(&key) {
        return if existing.mode == mode && existing.bytes == bytes {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    if state.transform_containers.len() >= service.limits.max_transform_containers
        || state
            .transform_bytes
            .checked_add(bytes.len())
            .is_none_or(|total| total > service.limits.max_transform_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let container = TransformContainer {
        mode,
        bytes,
        version: 1,
    };
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: CONTAINER_NAMESPACE.into(),
                key: container_key(&channel, &name),
                version: 1,
                payload: encode_container(&container)?,
            },
            None,
        )
        .map_err(store_error)?;
    state.transform_bytes += container.bytes.len();
    state.transform_containers.insert(key, container);
    Ok(())
}

pub(crate) fn container(
    service: &CicsService,
    channel: &str,
    name: &str,
) -> Result<(CicsTransformContainerMode, Vec<u8>), HostProblem> {
    let channel = normalize_name(channel, 16)?;
    let name = normalize_name(name, 16)?;
    let state = service.lock()?;
    let container = state
        .transform_containers
        .get(&(channel, name))
        .ok_or(HostProblem::NotFound)?;
    Ok((container.mode, container.bytes.clone()))
}

fn data_to_json(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request, CicsOperation::TransformDataToJson)?;
    let channel = request_name(request, "CHANNEL", 16, "CHANNELERR", 122, 1)?;
    let input = request_name(request, "INCONTAINER", 16, "CONTAINERERR", 110, 3)?;
    let output = match request.arguments.get("OUTCONTAINER") {
        Some(_) => request_name(request, "OUTCONTAINER", 16, "CONTAINERERR", 110, 0)?,
        None => DEFAULT_JSON_OUTPUT.into(),
    };
    let transformer = request_name(request, "TRANSFORMER", 16, "INVREQ", 16, 6)?;
    match service.authorize(
        run,
        "TRANSFORM",
        &format!("CICS.JSON.{transformer}"),
        AccessIntent::Update,
    ) {
        Ok(()) => {}
        Err(HostProblem::Unauthorized) => {
            return condition("INVREQ", 16, 101);
        }
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
            None => return condition("CONTAINERERR", 110, 3),
        };
        if source.mode != CicsTransformContainerMode::Bit {
            return condition("INVREQ", 16, 8);
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
    let transformed = match json_from_data(&definition, &source.bytes, service.limits) {
        Ok(bytes) => bytes,
        Err(HostProblem::ResourceExhausted) => return Err(HostProblem::ResourceExhausted),
        Err(_) => return condition("INVREQ", 16, 6),
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
        CicsTransformContainerMode::Char,
        transformed,
    )?;
    normal_response(service, run, BTreeMap::new())
}

fn data_to_xml(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request, CicsOperation::TransformDataToXml)?;
    let channel = request_name(request, "CHANNEL", 16, "CHANNELERR", 122, 1)?;
    let input = request_name(request, "DATCONTAINER", 16, "CONTAINERERR", 110, 3)?;
    let output = request_name(request, "XMLCONTAINER", 16, "CONTAINERERR", 110, 1)?;
    let transformer = request_name(request, "XMLTRANSFORM", 32, "INVREQ", 16, 6)?;
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
            None => return condition("CONTAINERERR", 110, 3),
        };
        if source.mode != CicsTransformContainerMode::Bit {
            return condition("INVREQ", 16, 8);
        }
        let definition = match state
            .transform_resources
            .get(&(CicsTransformFormat::Xml, transformer.clone()))
        {
            Some(definition) => definition.clone(),
            None => return condition("NOTFND", 13, 1),
        };
        if !definition.enabled {
            return condition("INVREQ", 16, 1);
        }
        (definition, source)
    };
    let metadata = definition
        .xml
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let outputs = metadata_outputs(request, metadata, true)?;
    let transformed = match xml_from_data(&definition, metadata, &source.bytes, service.limits) {
        Ok(bytes) => bytes,
        Err(XmlTransformProblem::ShortInput) => return condition("LENGERR", 22, 1),
        Err(XmlTransformProblem::InvalidData) => return condition("INVREQ", 16, 5),
        Err(XmlTransformProblem::Conversion) => return condition("INVREQ", 16, 6),
        Err(XmlTransformProblem::ResourceExhausted) => return Err(HostProblem::ResourceExhausted),
    };
    persist_effect_and_output(
        service,
        &channel,
        &output,
        effect_key,
        TransformEffect {
            operation: request.operation,
            request_digest,
            outputs: outputs.clone(),
        },
        CicsTransformContainerMode::Char,
        transformed,
    )?;
    normal_response(service, run, outputs)
}

fn validate_request(request: &CicsRequest, operation: CicsOperation) -> Result<(), HostProblem> {
    const JSON_ALLOWED: &[&str] = &[
        "CHANNEL",
        "INCONTAINER",
        "OPTION.NOHANDLE",
        "OUTCONTAINER",
        "RESP",
        "RESP2",
        "TRANSFORMER",
    ];
    const XML_ALLOWED: &[&str] = &[
        "CHANNEL",
        "DATCONTAINER",
        "ELEMNAME",
        "ELEMNAMELEN",
        "ELEMNS",
        "ELEMNSLEN",
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
    let (allowed, required) = match operation {
        CicsOperation::TransformDataToJson => {
            (JSON_ALLOWED, &["CHANNEL", "INCONTAINER", "TRANSFORMER"][..])
        }
        CicsOperation::TransformJsonToData => {
            (JSON_ALLOWED, &["CHANNEL", "INCONTAINER", "TRANSFORMER"][..])
        }
        CicsOperation::TransformDataToXml => (
            XML_ALLOWED,
            &["CHANNEL", "DATCONTAINER", "XMLCONTAINER", "XMLTRANSFORM"][..],
        ),
        _ => return Err(HostProblem::Malformed),
    };
    if request.operation != operation
        || request.mutation.is_none()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || required
            .iter()
            .any(|name| !request.arguments.contains_key(*name))
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
            !allowed.contains(&name.as_str())
                || match name.as_str() {
                    "CHANNEL" | "DATCONTAINER" | "INCONTAINER" | "OUTCONTAINER" | "TRANSFORMER"
                    | "XMLCONTAINER" | "XMLTRANSFORM" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    "ELEMNAMELEN" | "ELEMNSLEN" | "TYPENAMELEN" | "TYPENSLEN" => {
                        value.schema() != "mainframe-env.cics.decimal@1"
                    }
                    "ELEMNAME" | "ELEMNS" | "TYPENAME" | "TYPENS" => {
                        value.schema() != "mainframe-env.cics.argument@1"
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

#[allow(clippy::too_many_arguments)]
fn request_name(
    request: &CicsRequest,
    name: &str,
    maximum: usize,
    condition_name: &str,
    response: i32,
    response2: i32,
) -> Result<String, HostProblem> {
    let value = argument_text(request, name)?;
    normalize_name(&value, maximum).map_err(|_| HostProblem::Condition {
        name: condition_name.into(),
        response,
        response2,
    })
}

fn condition<T>(name: &str, response: i32, response2: i32) -> Result<T, HostProblem> {
    Err(HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    })
}

fn normal_response(
    service: &CicsService,
    run: &Run,
    outputs: BTreeMap<String, Vec<u8>>,
) -> Result<CicsResponse, HostProblem> {
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
    for (name, bytes) in outputs {
        let payload = if matches!(
            name.as_str(),
            "ELEMNAMELEN" | "ELEMNSLEN" | "TYPENAMELEN" | "TYPENSLEN"
        ) {
            decimal_payload(
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|value| value.parse::<i64>().ok())
                    .ok_or(HostProblem::InfrastructureFailure)?,
            )?
        } else {
            bounded(bytes)?
        };
        response.outputs.insert(name, payload);
    }
    Ok(response)
}

fn metadata_outputs(
    request: &CicsRequest,
    metadata: &CicsXmlTransformMetadata,
    enforce_name_namespace_maximum: bool,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    let mut outputs = BTreeMap::new();
    for (name, length, value, small, too_large) in [
        (
            "ELEMNAME",
            "ELEMNAMELEN",
            metadata.element_name.as_str(),
            2,
            Some(6),
        ),
        (
            "ELEMNS",
            "ELEMNSLEN",
            metadata.element_namespace.as_str(),
            3,
            Some(7),
        ),
        (
            "TYPENAME",
            "TYPENAMELEN",
            metadata.type_name.as_deref().unwrap_or_default(),
            4,
            None,
        ),
        (
            "TYPENS",
            "TYPENSLEN",
            metadata.type_namespace.as_deref().unwrap_or_default(),
            5,
            None,
        ),
    ] {
        if !request.arguments.contains_key(name) {
            continue;
        }
        let maximum = decimal_argument(request, length)?;
        if enforce_name_namespace_maximum
            && let Some(response2) = too_large
            && maximum > 255
        {
            return condition("LENGERR", 22, response2);
        }
        let actual = i64::try_from(value.len()).map_err(|_| HostProblem::ResourceExhausted)?;
        if maximum < actual || maximum < 0 {
            return condition("LENGERR", 22, small);
        }
        outputs.insert(name.into(), value.as_bytes().to_vec());
        outputs.insert(length.into(), actual.to_string().into_bytes());
    }
    Ok(outputs)
}

fn decimal_argument(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}

fn json_from_data(
    definition: &CicsTransformDefinition,
    data: &[u8],
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    let mut object = Map::new();
    for field_definition in &definition.fields {
        let end = field_definition
            .offset
            .checked_add(field_definition.length)
            .ok_or(HostProblem::Malformed)?;
        let bytes = data
            .get(field_definition.offset..end)
            .ok_or(HostProblem::Malformed)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| HostProblem::Malformed)?
            .trim_end_matches(' ');
        let value = match field_definition.kind {
            CicsTransformFieldKind::Text => Value::String(text.into()),
            CicsTransformFieldKind::SignedInteger => Value::Number(Number::from(
                text.parse::<i64>().map_err(|_| HostProblem::Malformed)?,
            )),
        };
        object.insert(field_definition.name.clone(), value);
    }
    let bytes = serde_json::to_vec(&Value::Object(object)).map_err(|_| HostProblem::Malformed)?;
    if bytes.len() > limits.max_transform_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(bytes)
    }
}

fn xml_from_data(
    definition: &CicsTransformDefinition,
    metadata: &CicsXmlTransformMetadata,
    data: &[u8],
    limits: CicsLimits,
) -> Result<Vec<u8>, XmlTransformProblem> {
    let mut xml = String::new();
    xml.push('<');
    xml.push_str(&metadata.element_name);
    if !metadata.element_namespace.is_empty() {
        xml.push_str(" xmlns=\"");
        push_xml_escaped(&mut xml, &metadata.element_namespace, true);
        xml.push('"');
    }
    if let Some(type_name) = metadata.type_name.as_deref() {
        xml.push_str(" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"");
        xml.push_str(" xsi:type=\"");
        if let Some(namespace) = metadata.type_namespace.as_deref()
            && !namespace.is_empty()
        {
            xml.push_str("t:");
        }
        push_xml_escaped(&mut xml, type_name, true);
        xml.push('"');
        if let Some(namespace) = metadata.type_namespace.as_deref()
            && !namespace.is_empty()
        {
            xml.push_str(" xmlns:t=\"");
            push_xml_escaped(&mut xml, namespace, true);
            xml.push('"');
        }
    }
    xml.push('>');
    for field_definition in &definition.fields {
        let end = field_definition
            .offset
            .checked_add(field_definition.length)
            .ok_or(XmlTransformProblem::ResourceExhausted)?;
        let bytes = data
            .get(field_definition.offset..end)
            .ok_or(XmlTransformProblem::ShortInput)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| XmlTransformProblem::InvalidData)?
            .trim_end_matches(' ');
        if !text.chars().all(xml_character_allowed) {
            return Err(XmlTransformProblem::InvalidData);
        }
        let value = match field_definition.kind {
            CicsTransformFieldKind::Text => text.to_string(),
            CicsTransformFieldKind::SignedInteger => text
                .parse::<i64>()
                .map_err(|_| XmlTransformProblem::Conversion)?
                .to_string(),
        };
        xml.push('<');
        xml.push_str(&field_definition.name);
        xml.push('>');
        push_xml_escaped(&mut xml, &value, false);
        xml.push_str("</");
        xml.push_str(&field_definition.name);
        xml.push('>');
        if xml.len() > limits.max_transform_bytes {
            return Err(XmlTransformProblem::ResourceExhausted);
        }
    }
    xml.push_str("</");
    xml.push_str(&metadata.element_name);
    xml.push('>');
    if xml.len() > limits.max_transform_bytes {
        Err(XmlTransformProblem::ResourceExhausted)
    } else {
        Ok(xml.into_bytes())
    }
}

fn push_xml_escaped(output: &mut String, value: &str, attribute: bool) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            '\'' if attribute => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
}

fn xml_character_allowed(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

fn persist_effect_and_output(
    service: &CicsService,
    channel: &str,
    output: &str,
    effect_key: &str,
    effect: TransformEffect,
    mode: CicsTransformContainerMode,
    bytes: Vec<u8>,
) -> Result<(), HostProblem> {
    let mut state = service.lock()?;
    if let Some(existing) = service
        .store
        .get_provider_state(EFFECT_NAMESPACE, effect_key)
        .map_err(store_error)?
    {
        let existing = decode_effect(&existing.payload, service.limits)?;
        return if existing == effect {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    let target_key = (channel.to_string(), output.to_string());
    let prior = state.transform_containers.get(&target_key).cloned();
    let prior_bytes = prior.as_ref().map_or(0, |container| container.bytes.len());
    let next_total = state
        .transform_bytes
        .checked_sub(prior_bytes)
        .and_then(|total| total.checked_add(bytes.len()))
        .ok_or(HostProblem::ResourceExhausted)?;
    if next_total > service.limits.max_transform_bytes
        || prior.is_none()
            && state.transform_containers.len() >= service.limits.max_transform_containers
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let version = prior.as_ref().map_or(Ok(1), |container| {
        container
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)
    })?;
    let container = TransformContainer {
        mode,
        bytes,
        version,
    };
    service
        .store
        .put_provider_states_atomic(vec![
            ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CONTAINER_NAMESPACE.into(),
                    key: container_key(channel, output),
                    version,
                    payload: encode_container(&container)?,
                },
                expected_version: prior.as_ref().map(|container| container.version),
            },
            ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: EFFECT_NAMESPACE.into(),
                    key: effect_key.into(),
                    version: 1,
                    payload: encode_effect(&effect)?,
                },
                expected_version: None,
            },
        ])
        .map_err(store_error)
        .map_err(mutation_problem)?;
    state.transform_bytes = next_total;
    state.transform_containers.insert(target_key, container);
    Ok(())
}

fn replay_effect(
    service: &CicsService,
    effect_key: &str,
    request_digest: [u8; 32],
    operation: CicsOperation,
) -> Result<Option<TransformEffect>, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(EFFECT_NAMESPACE, effect_key)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let effect = decode_effect(&row.payload, service.limits)?;
    if effect.operation != operation || effect.request_digest != request_digest {
        Err(HostProblem::IdempotencyConflict)
    } else {
        Ok(Some(effect))
    }
}

fn normalize_definition(
    mut definition: CicsTransformDefinition,
    limits: CicsLimits,
) -> Result<CicsTransformDefinition, HostProblem> {
    definition.name = normalize_name(
        &definition.name,
        match definition.format {
            CicsTransformFormat::Json => 16,
            CicsTransformFormat::Xml => 32,
        },
    )?;
    if definition.fields.is_empty() || definition.fields.len() > limits.max_fields {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut names = BTreeSet::new();
    let mut extents = Vec::new();
    for field_definition in &definition.fields {
        if !valid_field_name(&field_definition.name)
            || !names.insert(field_definition.name.clone())
            || field_definition.length == 0
        {
            return Err(HostProblem::Malformed);
        }
        let end = field_definition
            .offset
            .checked_add(field_definition.length)
            .ok_or(HostProblem::ResourceExhausted)?;
        if end > limits.max_transform_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        extents.push((field_definition.offset, end));
    }
    extents.sort_unstable();
    if extents.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(HostProblem::Malformed);
    }
    match (definition.format, definition.xml.as_ref()) {
        (CicsTransformFormat::Json, None) => {}
        (CicsTransformFormat::Xml, Some(metadata))
            if valid_field_name(&metadata.element_name)
                && metadata.element_name.len() <= 255
                && metadata.element_namespace.len() <= 255
                && metadata
                    .element_namespace
                    .chars()
                    .all(xml_character_allowed)
                && metadata.type_name.as_deref().is_none_or(valid_field_name)
                && metadata
                    .type_name
                    .as_ref()
                    .is_none_or(|name| name.len() <= 255)
                && metadata.type_namespace.as_ref().is_none_or(|namespace| {
                    namespace.len() <= 255 && namespace.chars().all(xml_character_allowed)
                })
                && metadata.type_name.is_some() == metadata.type_namespace.is_some() => {}
        _ => return Err(HostProblem::Malformed),
    }
    Ok(definition)
}

fn valid_field_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn normalize_name(value: &str, maximum: usize) -> Result<String, HostProblem> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty()
        || value.len() > maximum
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@')
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value)
    }
}

fn resource_key(format: CicsTransformFormat, name: &str) -> String {
    match format {
        CicsTransformFormat::Json => format!("JSON/{name}"),
        CicsTransformFormat::Xml => format!("XML/{name}"),
    }
}

fn container_key(channel: &str, name: &str) -> String {
    format!("{channel}/{name}")
}

fn split_container_key(value: &str) -> Result<(String, String), HostProblem> {
    let (channel, name) = value
        .split_once('/')
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok((normalize_name(channel, 16)?, normalize_name(name, 16)?))
}

fn encode_definition(definition: &CicsTransformDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"METR1".to_vec();
    out.push(match definition.format {
        CicsTransformFormat::Json => 1,
        CicsTransformFormat::Xml => 2,
    });
    out.push(u8::from(definition.enabled));
    field(&mut out, definition.name.as_bytes())?;
    out.extend_from_slice(
        &u32::try_from(definition.fields.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for definition in &definition.fields {
        field(&mut out, definition.name.as_bytes())?;
        out.extend_from_slice(
            &u64::try_from(definition.offset)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        out.extend_from_slice(
            &u64::try_from(definition.length)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        out.push(match definition.kind {
            CicsTransformFieldKind::Text => 1,
            CicsTransformFieldKind::SignedInteger => 2,
        });
    }
    if let Some(metadata) = &definition.xml {
        field(&mut out, metadata.element_name.as_bytes())?;
        field(&mut out, metadata.element_namespace.as_bytes())?;
        match (&metadata.type_name, &metadata.type_namespace) {
            (Some(name), Some(namespace)) => {
                out.push(1);
                field(&mut out, name.as_bytes())?;
                field(&mut out, namespace.as_bytes())?;
            }
            (None, None) => out.push(0),
            _ => return Err(HostProblem::Malformed),
        }
    }
    Ok(out)
}

fn decode_definition(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsTransformDefinition, HostProblem> {
    let mut reader = Reader::new(bytes, b"METR1")?;
    let format = match reader.byte()? {
        1 => CicsTransformFormat::Json,
        2 => CicsTransformFormat::Xml,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let enabled = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let name = reader.text(match format {
        CicsTransformFormat::Json => 16,
        CicsTransformFormat::Xml => 32,
    })?;
    let count = reader.count(limits.max_fields)?;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        fields.push(CicsTransformFieldDefinition {
            name: reader.text(128)?,
            offset: reader.usize()?,
            length: reader.usize()?,
            kind: match reader.byte()? {
                1 => CicsTransformFieldKind::Text,
                2 => CicsTransformFieldKind::SignedInteger,
                _ => return Err(HostProblem::InfrastructureFailure),
            },
        });
    }
    let xml = if format == CicsTransformFormat::Xml {
        let element_name = reader.text(255)?;
        let element_namespace = reader.text(255)?;
        let (type_name, type_namespace) = match reader.byte()? {
            0 => (None, None),
            1 => (Some(reader.text(255)?), Some(reader.text(255)?)),
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        Some(CicsXmlTransformMetadata {
            element_name,
            element_namespace,
            type_name,
            type_namespace,
        })
    } else {
        None
    };
    reader.finish()?;
    normalize_definition(
        CicsTransformDefinition {
            name,
            enabled,
            format,
            fields,
            xml,
        },
        limits,
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn encode_container(container: &TransformContainer) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"METC1".to_vec();
    out.push(match container.mode {
        CicsTransformContainerMode::Bit => 1,
        CicsTransformContainerMode::Char => 2,
    });
    field(&mut out, &container.bytes)?;
    Ok(out)
}

fn decode_container(
    bytes: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<TransformContainer, HostProblem> {
    let mut reader = Reader::new(bytes, b"METC1")?;
    let mode = match reader.byte()? {
        1 => CicsTransformContainerMode::Bit,
        2 => CicsTransformContainerMode::Char,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let bytes = reader.bytes(limits.max_transform_bytes)?;
    reader.finish()?;
    if version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(TransformContainer {
        mode,
        bytes,
        version,
    })
}

fn encode_effect(effect: &TransformEffect) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"METE1".to_vec();
    out.push(match effect.operation {
        CicsOperation::TransformDataToJson => 1,
        CicsOperation::TransformDataToXml => 2,
        CicsOperation::TransformJsonToData => 3,
        CicsOperation::TransformXmlToData => 4,
        _ => return Err(HostProblem::InfrastructureFailure),
    });
    out.extend_from_slice(&effect.request_digest);
    out.extend_from_slice(
        &u32::try_from(effect.outputs.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &effect.outputs {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value)?;
    }
    Ok(out)
}

fn decode_effect(bytes: &[u8], limits: CicsLimits) -> Result<TransformEffect, HostProblem> {
    let mut reader = Reader::new(bytes, b"METE1")?;
    let operation = match reader.byte()? {
        1 => CicsOperation::TransformDataToJson,
        2 => CicsOperation::TransformDataToXml,
        3 => CicsOperation::TransformJsonToData,
        4 => CicsOperation::TransformXmlToData,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let request_digest = reader
        .take(32)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = reader.count(limits.max_fields)?;
    let mut outputs = BTreeMap::new();
    for _ in 0..count {
        if outputs
            .insert(reader.text(128)?, reader.bytes(limits.max_transform_bytes)?)
            .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    reader.finish()?;
    Ok(TransformEffect {
        operation,
        request_digest,
        outputs,
    })
}
