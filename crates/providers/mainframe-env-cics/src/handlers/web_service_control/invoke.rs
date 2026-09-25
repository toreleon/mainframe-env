use super::*;
use crate::service::handlers::transform_control::{self, TransformContainer};
use mainframe_env_host_api::{
    HostRequest, HostResult, ProgramLinkSelection, ProgramName, ProgramRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite};
use sha2::{Digest, Sha256};

const SERVICE_NAMESPACE: &str = "cics-web-service-v1";
const INTENT_NAMESPACE: &str = "cics-web-invoke-intent-v1";

/// Installed bounded local service transport selected by `INVOKE SERVICE`.
/// The named program generation is immutable and must already be registered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsWebServiceDefinition {
    /// CICS WEBSERVICE or channel service name, at most 32 characters.
    pub name: String,
    /// Installed local CICS program that receives the service body.
    pub program: String,
    /// Exact immutable installed program generation.
    pub generation: u64,
    /// Source WSDL operation admitted by this binding.
    pub operation: String,
    /// Optional SCA qualifying prefix for channel-based service lookup.
    pub scope: Option<String>,
    /// Optional default endpoint URI for XML services.
    pub uri: Option<String>,
    /// Optional named client URIMAP associated with the endpoint.
    pub urimap: Option<String>,
    /// Whether this binding uses channel service semantics without a URI.
    pub channel_based: bool,
    /// Whether the binding can currently be invoked.
    pub enabled: bool,
    /// Whether WS-Addressing context controls the endpoint.
    pub addressing: bool,
}

impl CicsService {
    /// Register an immutable local service binding for checked service invocation.
    pub fn register_web_service(
        &self,
        definition: CicsWebServiceDefinition,
    ) -> Result<(), HostProblem> {
        let normalized = normalize_definition(definition)?;
        let present = self
            .lock()?
            .program_definitions
            .get(&normalized.program)
            .and_then(|generations| generations.get(&normalized.generation))
            .is_some_and(|program| program.enabled && !program.remote);
        if !present {
            return Err(HostProblem::NotFound);
        }
        let resource_key = service_key(normalized.scope.as_deref(), &normalized.name);
        let row = self
            .store
            .get_provider_state(SERVICE_NAMESPACE, &resource_key)
            .map_err(store_error)?;
        if let Some(row) = row {
            return if decode_definition(&row.payload)? == normalized {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let rows = self
            .store
            .list_provider_state(SERVICE_NAMESPACE, self.limits.max_programs)
            .map_err(store_error)?;
        if rows.len() >= self.limits.max_programs {
            return Err(HostProblem::ResourceExhausted);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: SERVICE_NAMESPACE.into(),
                    key: resource_key,
                    version: 1,
                    payload: encode_definition(&normalized)?,
                },
                None,
            )
            .map_err(store_error)
    }
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let name = text(request, "SERVICE", 32)?.ok_or(HostProblem::Malformed)?;
    let name = name.trim_end_matches(' ').to_ascii_uppercase();
    if name.is_empty() {
        return Err(HostProblem::Malformed);
    }
    if value(request, "SCOPE").is_some() != value(request, "SCOPELEN").is_some() {
        return Err(condition("LENGERR", 22, 1));
    }
    let scope = if let Some(bytes) = value(request, "SCOPE") {
        let length = number(request, "SCOPELEN")?.ok_or(HostProblem::Malformed)?;
        if length <= 0
            || length > 160
            || usize::try_from(length).ok().is_none_or(|n| n > bytes.len())
        {
            return Err(condition("LENGERR", 22, 1));
        }
        Some(
            String::from_utf8(bytes[..length as usize].to_vec())
                .map_err(|_| condition("INVREQ", 16, 7))?,
        )
    } else {
        None
    };
    let resource_key = service_key(scope.as_deref(), &name);
    let definition = service
        .store
        .get_provider_state(SERVICE_NAMESPACE, &resource_key)
        .map_err(store_error)?
        .ok_or_else(|| condition("NOTFND", 13, 4))?;
    let definition = decode_definition(&definition.payload)?;
    if !definition.enabled {
        return Err(condition("INVREQ", 16, 8));
    }
    let operation =
        text(request, "OPERATION", 255)?.unwrap_or_else(|| definition.operation.clone());
    if operation.trim_end_matches(' ') != definition.operation {
        return Err(condition("NOTFND", 13, 3));
    }
    if value(request, "URI").is_some() && value(request, "URIMAP").is_some() {
        return Err(HostProblem::Malformed);
    }
    if scope.is_some() && !definition.channel_based {
        return Err(condition("INVREQ", 16, 7));
    }
    let channel = channel(service, run, request).map_err(|problem| match problem {
        HostProblem::Condition {
            name,
            response: 122,
            response2: 2,
        } if name == "CHANNELERR" => condition("NOTFND", 13, 2),
        problem => problem,
    })?;
    let mut channel_state = state::load(service, run, &channel)?;
    if definition.addressing
        && (value(request, "URI").is_some() || value(request, "URIMAP").is_some())
        || channel_state
            .fields
            .keys()
            .any(|key| key.starts_with("WSA.REQ."))
            && (value(request, "URI").is_some() || value(request, "URIMAP").is_some())
    {
        return Err(condition("INVREQ", 16, 19));
    }
    if !definition.channel_based {
        let uri = if let Some(uri) = text(request, "URI", 255)? {
            uri.trim_end_matches(' ').to_string()
        } else if let Some(map) = text(request, "URIMAP", 16)? {
            if definition.urimap.as_deref() != Some(map.trim_end_matches(' ')) {
                return Err(condition("NOTFND", 13, 6));
            }
            definition
                .uri
                .clone()
                .ok_or_else(|| condition("INVREQ", 16, 7))?
        } else {
            definition
                .uri
                .clone()
                .ok_or_else(|| condition("INVREQ", 16, 7))?
        };
        if !soap_fault::valid_uri(&uri) {
            return Err(condition("INVREQ", 16, 4));
        }
    }
    let (source_name, source) = {
        let state = service.lock()?;
        ["DFHWS-BODY", "DFHWS-DATA"]
            .into_iter()
            .find_map(|name| {
                state
                    .transform_containers
                    .get(&(channel.clone(), name.into()))
                    .cloned()
                    .map(|container| (name, container))
            })
            .ok_or_else(|| condition("INVREQ", 16, 12))?
    };
    if source.bytes.is_empty() {
        return Err(condition("INVREQ", 16, 103));
    }
    if source.bytes.len() > service.limits.max_transform_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let program = service
        .lock()?
        .program_definitions
        .get(&definition.program)
        .and_then(|generations| generations.get(&definition.generation))
        .cloned()
        .ok_or_else(|| condition("INVREQ", 16, 11))?;
    if !program.enabled || program.remote {
        return Err(condition("INVREQ", 16, 11));
    }
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.WEBSERVICE.{name}"),
        AccessIntent::Execute,
    )?;
    authorize_channel(service, run, &channel, AccessIntent::Update)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let intent_key = mutation.idempotency_key.as_str();
    if let Some(existing) = service
        .store
        .get_provider_state(INTENT_NAMESPACE, intent_key)
        .map_err(store_error)?
    {
        return if existing.payload == digest {
            Err(HostProblem::UnknownOutcome)
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: INTENT_NAMESPACE.into(),
                key: intent_key.into(),
                version: 1,
                payload: digest.to_vec(),
            },
            None,
        )
        .map_err(store_error)?;
    let result = service.nested(
        run,
        HostRequest::Program(ProgramRequest::Link {
            program: ProgramName::new(&definition.program, 128)
                .map_err(|_| HostProblem::Malformed)?,
            payload: bounded(source.bytes.clone())?,
            selection: Some(ProgramLinkSelection {
                artifact: program.artifact.clone(),
                generation: program.generation,
                content_identity: format!(
                    "sha256:{:x}",
                    Sha256::digest(encode_definition(&definition)?)
                ),
            }),
        }),
    );
    let bytes = match result {
        Ok(HostResult::Program(payload)) => payload.bytes().to_vec(),
        Err(HostProblem::NotFound) => return Err(condition("INVREQ", 16, 11)),
        Err(_) | Ok(_) => return Err(HostProblem::UnknownOutcome),
    };
    if bytes.len() > service.limits.max_transform_bytes {
        return Err(HostProblem::UnknownOutcome);
    }
    channel_state
        .fields
        .insert("SERVICE.RESPONSE".into(), bytes.clone());
    let mut response = normal(service, run).map_err(|_| HostProblem::UnknownOutcome)?;
    response.payload = bounded(bytes.clone()).map_err(|_| HostProblem::UnknownOutcome)?;
    if bytes.windows(6).any(|window| window == b"<Fault") {
        response.condition = "INVREQ".into();
        response.response = 16;
        response.response2 = 6;
    }
    let (write, updated, next_total) = {
        let state = service.lock()?;
        let key = (channel.clone(), source_name.into());
        let prior = state
            .transform_containers
            .get(&key)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let next_total = state
            .transform_bytes
            .checked_sub(prior.bytes.len())
            .and_then(|total| total.checked_add(bytes.len()))
            .ok_or(HostProblem::ResourceExhausted)?;
        if next_total > service.limits.max_transform_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let updated = TransformContainer {
            mode: prior.mode,
            bytes: bytes.clone(),
            version: prior
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
        };
        let write = ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: transform_control::CONTAINER_NAMESPACE.into(),
                key: transform_control::container_key(&channel, source_name),
                version: updated.version,
                payload: transform_control::encode_container(&updated)?,
            },
            expected_version: Some(prior.version),
        };
        (write, updated, next_total)
    };
    let persisted = state::persist_with(
        service,
        run,
        request,
        retention_tick,
        &channel,
        &channel_state,
        &response,
        vec![write],
        Some(next_total),
    );
    if persisted.is_ok() || persisted == Err(HostProblem::UnknownOutcome) {
        let mut state = service.lock()?;
        state.transform_bytes = next_total;
        state
            .transform_containers
            .insert((channel.clone(), source_name.into()), updated);
    }
    persisted.map_err(|_| HostProblem::UnknownOutcome)?;
    Ok(response)
}

fn service_key(scope: Option<&str>, name: &str) -> String {
    let scope = scope.unwrap_or_default();
    format!("{:03}:{scope}{name}", scope.len())
}

fn normalize_definition(
    mut definition: CicsWebServiceDefinition,
) -> Result<CicsWebServiceDefinition, HostProblem> {
    definition.name = definition.name.trim().to_ascii_uppercase();
    definition.program = definition.program.trim().to_ascii_uppercase();
    if definition.name.is_empty()
        || definition.name.len() > 32
        || definition.program.is_empty()
        || definition.program.len() > 128
        || definition.generation == 0
        || definition.operation.is_empty()
        || definition.operation.len() > 255
        || definition
            .uri
            .as_ref()
            .is_some_and(|uri| !soap_fault::valid_uri(uri))
        || definition
            .urimap
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > 16)
        || definition
            .scope
            .as_ref()
            .is_some_and(|scope| scope.is_empty() || scope.len() > 160)
        || definition.scope.is_some() && !definition.channel_based
        || !definition.channel_based && definition.uri.is_none()
    {
        return Err(HostProblem::Malformed);
    }
    Ok(definition)
}

fn encode_definition(definition: &CicsWebServiceDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = b"WSD1".to_vec();
    for value in [&definition.name, &definition.program, &definition.operation] {
        field(&mut bytes, value.as_bytes())?;
    }
    field(
        &mut bytes,
        definition.scope.as_deref().unwrap_or_default().as_bytes(),
    )?;
    bytes.extend_from_slice(&definition.generation.to_be_bytes());
    for value in [definition.uri.as_deref(), definition.urimap.as_deref()] {
        field(&mut bytes, value.unwrap_or_default().as_bytes())?;
    }
    bytes.extend_from_slice(&[
        u8::from(definition.channel_based),
        u8::from(definition.enabled),
        u8::from(definition.addressing),
    ]);
    Ok(bytes)
}

fn decode_definition(bytes: &[u8]) -> Result<CicsWebServiceDefinition, HostProblem> {
    let mut reader = DefinitionReader { bytes, at: 0 };
    if reader.take(4)? != b"WSD1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let name = reader.string(32)?;
    let program = reader.string(128)?;
    let operation = reader.string(255)?;
    let scope = reader.string(160)?;
    let generation = u64::from_be_bytes(
        reader
            .take(8)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let uri = reader.string(255)?;
    let urimap = reader.string(16)?;
    let flags = reader.take(3)?;
    if reader.at != bytes.len() || flags.iter().any(|flag| *flag > 1) {
        return Err(HostProblem::InfrastructureFailure);
    }
    normalize_definition(CicsWebServiceDefinition {
        name,
        program,
        generation,
        operation,
        scope: (!scope.is_empty()).then_some(scope),
        uri: (!uri.is_empty()).then_some(uri),
        urimap: (!urimap.is_empty()).then_some(urimap),
        channel_based: flags[0] == 1,
        enabled: flags[1] == 1,
        addressing: flags[2] == 1,
    })
    .map_err(|_| HostProblem::InfrastructureFailure)
}

struct DefinitionReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> DefinitionReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(result)
    }
    fn string(&mut self, max: usize) -> Result<String, HostProblem> {
        let n = u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ) as usize;
        if n > max {
            return Err(HostProblem::InfrastructureFailure);
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| HostProblem::InfrastructureFailure)
    }
}
