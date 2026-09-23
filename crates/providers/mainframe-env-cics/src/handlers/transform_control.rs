use super::super::{CicsLimits, CicsService, Run, argument_text, bounded, mutation_problem};
use super::{field, store_error};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};

const RESOURCE_NAMESPACE: &str = "cics-transform-resource-v1";
const CONTAINER_NAMESPACE: &str = "cics-transform-container-v1";
const EFFECT_NAMESPACE: &str = "cics-transform-effect-v1";
const DEFAULT_JSON_OUTPUT: &str = "DFHJSON-JSON";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CicsTransformFormat {
    Json,
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

fn validate_request(request: &CicsRequest, operation: CicsOperation) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "CHANNEL",
        "INCONTAINER",
        "OPTION.NOHANDLE",
        "OUTCONTAINER",
        "RESP",
        "RESP2",
        "TRANSFORMER",
    ];
    if request.operation != operation
        || request.mutation.is_none()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || !request.arguments.contains_key("CHANNEL")
        || !request.arguments.contains_key("INCONTAINER")
        || !request.arguments.contains_key("TRANSFORMER")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "CHANNEL" | "INCONTAINER" | "OUTCONTAINER" | "TRANSFORMER" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
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
        response.outputs.insert(name, bounded(bytes)?);
    }
    Ok(response)
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
    definition.name = normalize_name(&definition.name, 16)?;
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
    Ok(out)
}

fn decode_definition(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsTransformDefinition, HostProblem> {
    let mut reader = Reader::new(bytes, b"METR1")?;
    let format = match reader.byte()? {
        1 => CicsTransformFormat::Json,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let enabled = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let name = reader.text(16)?;
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
    reader.finish()?;
    normalize_definition(
        CicsTransformDefinition {
            name,
            enabled,
            format,
            fields,
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

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], magic: &[u8]) -> Result<Self, HostProblem> {
        if !bytes.starts_with(magic) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Self {
            bytes,
            at: magic.len(),
        })
    }

    fn take(&mut self, amount: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    fn bytes(&mut self, maximum: usize) -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if length > maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(length)?.to_vec())
    }

    fn text(&mut self, maximum: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.bytes(maximum)?).map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn count(&mut self, maximum: usize) -> Result<usize, HostProblem> {
        let count = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if count > maximum {
            Err(HostProblem::ResourceExhausted)
        } else {
            Ok(count)
        }
    }

    fn usize(&mut self) -> Result<usize, HostProblem> {
        usize::try_from(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)
    }

    fn finish(self) -> Result<(), HostProblem> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        }
    }
}
