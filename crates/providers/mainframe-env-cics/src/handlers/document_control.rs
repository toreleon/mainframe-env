use super::super::Reader;
use super::super::{
    CicsDocumentTemplateDefinition, CicsEffectReplay, CicsLimits, CicsService, Run,
    cics_effect_replay_binding_digest, decimal_payload, encode_cics_effect_replay, field,
    store_error,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, HostResult, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const DOCUMENT_NAMESPACE: &str = "cics-document-v1";
const TEMPLATE_NAMESPACE: &str = "cics-document-template-v1";
const DOCUMENT_MAGIC: &[u8; 8] = b"MECDOC01";
const TEMPLATE_MAGIC: &[u8; 8] = b"MECTPL01";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct DocumentRecord {
    token: [u8; 16],
    owner_execution: String,
    owner_run_unit: String,
    transaction: String,
    segments: Vec<DocumentSegment>,
    symbols: BTreeMap<String, Vec<u8>>,
    bookmarks: BTreeMap<String, usize>,
    version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DocumentSegment {
    bytes: Vec<u8>,
    binary: bool,
    host_code_page: u16,
}

pub(in crate::service) type DocumentAuthority = (
    BTreeMap<String, DocumentRecord>,
    BTreeMap<String, CicsDocumentTemplateDefinition>,
    usize,
);

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::DocumentCreate => create(service, run, request, retention_tick),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(in crate::service) fn load_authority(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<DocumentAuthority, HostProblem> {
    let maximum = limits
        .max_documents
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = store
        .list_provider_state(DOCUMENT_NAMESPACE, maximum)
        .map_err(store_error)?;
    if rows.len() > limits.max_documents {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut documents = BTreeMap::new();
    let mut bytes = 0usize;
    for row in rows {
        let document = decode_document(&row.payload, row.version, limits)?;
        let key = token_key(&document.token);
        bytes = bytes
            .checked_add(document.usage_bytes())
            .ok_or(HostProblem::ResourceExhausted)?;
        if row.key != key
            || bytes > limits.max_document_bytes
            || documents.insert(key, document).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let maximum = limits
        .max_document_templates
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = store
        .list_provider_state(TEMPLATE_NAMESPACE, maximum)
        .map_err(store_error)?;
    if rows.len() > limits.max_document_templates {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut templates = BTreeMap::new();
    for row in rows {
        let template = decode_template(&row.payload, limits)?;
        let key = normalize_template_name(&template.name)?;
        if row.version == 0 || row.key != key || templates.insert(key, template).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok((documents, templates, bytes))
}

impl CicsService {
    /// Registers bounded durable DOCTEMPLATE definitions used by document commands.
    pub fn register_document_templates(
        &self,
        definitions: &[CicsDocumentTemplateDefinition],
    ) -> Result<(), HostProblem> {
        let mut normalized = BTreeMap::new();
        for definition in definitions {
            let mut definition = definition.clone();
            definition.name = normalize_template_name(&definition.name)?;
            definition.resource = normalize_resource_name(&definition.resource)?;
            if definition.content.len() > self.limits.max_screen_bytes
                || definition.host_code_page == 0
                || normalized
                    .insert(definition.name.clone(), definition)
                    .is_some()
            {
                return Err(HostProblem::Malformed);
            }
        }
        let mut state = self.lock()?;
        let additions = normalized
            .keys()
            .filter(|name| !state.document_templates.contains_key(*name))
            .count();
        if state
            .document_templates
            .len()
            .checked_add(additions)
            .is_none_or(|count| count > self.limits.max_document_templates)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for (name, definition) in &normalized {
            if let Some(existing) = state.document_templates.get(name) {
                if existing != definition {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: TEMPLATE_NAMESPACE.into(),
                    key: name.clone(),
                    version: 1,
                    payload: encode_template(definition)?,
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        state.document_templates.extend(normalized);
        Ok(())
    }
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let mut state = service.lock()?;
    let owned = state
        .documents
        .iter()
        .filter(|(_, document)| {
            document.owner_execution == run.invocation.execution_id.as_str()
                && document.owner_run_unit == run.invocation.run_unit_id.as_str()
        })
        .map(|(key, document)| (key.clone(), document.clone()))
        .collect::<Vec<_>>();
    if owned.is_empty() {
        return Ok(());
    }
    service
        .store
        .mutate_provider_states_atomic(
            owned
                .iter()
                .map(|(key, document)| ProviderStateMutation::Delete {
                    namespace: DOCUMENT_NAMESPACE.into(),
                    key: key.clone(),
                    expected_version: document.version,
                })
                .collect(),
        )
        .map_err(store_error)?;
    for (key, document) in owned {
        state.documents.remove(&key);
        state.document_bytes = state
            .document_bytes
            .checked_sub(document.usage_bytes())
            .ok_or(HostProblem::InfrastructureFailure)?;
    }
    Ok(())
}

fn create(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_create_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let symbols = symbol_definitions(service, request, "LISTLENGTH")?;
    let host_code_page = host_code_page(request)?;
    let template = if let Some(value) = request.arguments.get("TEMPLATE") {
        let name = normalize_template_name(std::str::from_utf8(value.bytes()).map_err(|_| {
            HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 3,
            }
        })?)?;
        let template = service
            .lock()?
            .document_templates
            .get(&name)
            .cloned()
            .ok_or_else(|| HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 3,
            })?;
        service.authorize(
            run,
            "DOCTEMPLATE",
            &format!("CICS.DOCTEMPLATE.{}", template.resource),
            AccessIntent::Read,
        )?;
        Some(template)
    } else {
        None
    };
    let mut state = service.lock()?;
    if state.documents.len() >= service.limits.max_documents {
        return Err(HostProblem::ResourceExhausted);
    }
    let token = generated_token(run, mutation.idempotency_key.as_str());
    let key = token_key(&token);
    if state.documents.contains_key(&key) {
        return Err(HostProblem::IdempotencyConflict);
    }
    let segments = if let Some(value) = request.arguments.get("FROMDOC") {
        let source = document_for_token(&state.documents, run, value.bytes(), 2)?;
        source.segments.clone()
    } else if let Some(template) = template {
        vec![DocumentSegment {
            bytes: substitute_symbols(&template.content, &symbols),
            binary: false,
            host_code_page: if request.arguments.contains_key("HOSTCODEPAGE") {
                host_code_page
            } else {
                template.host_code_page
            },
        }]
    } else if let Some((name, binary)) = [("FROM", false), ("TEXT", false), ("BINARY", true)]
        .into_iter()
        .find(|(name, _)| request.arguments.contains_key(*name))
    {
        let mut bytes = request.arguments[name].bytes().to_vec();
        let length = unsigned_length(request, "LENGTH", 1, true)?;
        if length > bytes.len() {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 1,
            });
        }
        bytes.truncate(length);
        vec![DocumentSegment {
            bytes,
            binary,
            host_code_page,
        }]
    } else {
        Vec::new()
    };
    let document = DocumentRecord {
        token,
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        segments,
        symbols,
        bookmarks: BTreeMap::new(),
        version: 1,
    };
    let document_size = document.retrieval_size();
    let usage = document.usage_bytes();
    if usage > service.limits.max_screen_bytes
        || state
            .document_bytes
            .checked_add(usage)
            .is_none_or(|total| total > service.limits.max_document_bytes)
    {
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
    response.outputs.insert(
        "DOCTOKEN".into(),
        super::super::bounded(document.token.to_vec())?,
    );
    if request.arguments.contains_key("DOCSIZE") {
        response.outputs.insert(
            "DOCSIZE".into(),
            decimal_payload(
                i64::try_from(document_size).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    service
        .store
        .put_provider_states_atomic(vec![
            ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DOCUMENT_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    payload: encode_document(&document)?,
                },
                expected_version: None,
            },
            replay_write(run, request, retention_tick, &response)?,
        ])
        .map_err(store_error)?;
    state.document_bytes += usage;
    state.documents.insert(key, document);
    Ok(response)
}

fn validate_create_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let content_sources = ["FROM", "TEXT", "BINARY", "FROMDOC", "TEMPLATE"]
        .into_iter()
        .filter(|name| request.arguments.contains_key(*name))
        .count();
    let buffered = ["FROM", "TEXT", "BINARY"]
        .into_iter()
        .any(|name| request.arguments.contains_key(name));
    let symbols = request.arguments.contains_key("SYMBOLLIST");
    if content_sources > 1
        || request.arguments.contains_key("LENGTH") != buffered
        || request.arguments.contains_key("LISTLENGTH") != symbols
        || (request.arguments.contains_key("DELIMITER") && !symbols)
        || (request.arguments.contains_key("OPTION.UNESCAPED") && !symbols)
        || !request.arguments.contains_key("DOCTOKEN")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "FROM" | "TEXT" | "BINARY" | "FROMDOC" | "SYMBOLLIST" => {
                value.schema() == "mainframe-env.cics.storage-value@1"
            }
            "TEMPLATE" | "DELIMITER" | "HOSTCODEPAGE" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "LENGTH" | "LISTLENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "DOCTOKEN" | "DOCSIZE" | "RESP" | "RESP2" => {
                value.schema() == "mainframe-env.cics.argument@1"
            }
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

fn symbol_definitions(
    service: &CicsService,
    request: &CicsRequest,
    length_name: &str,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    let Some(value) = request.arguments.get("SYMBOLLIST") else {
        return Ok(BTreeMap::new());
    };
    let length = unsigned_length(request, length_name, 9, false)?;
    if length > value.bytes().len() || length > service.limits.max_screen_bytes {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 9,
        });
    }
    let delimiter = request
        .arguments
        .get("DELIMITER")
        .map(|value| value.bytes())
        .unwrap_or(b"&");
    if delimiter.len() != 1 || invalid_delimiter(delimiter[0]) {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 8,
        });
    }
    let mut symbols = BTreeMap::new();
    let bytes = &value.bytes()[..length];
    let mut offset = 0usize;
    for definition in bytes.split(|byte| *byte == delimiter[0]) {
        let Some(equal) = definition.iter().position(|byte| *byte == b'=') else {
            return symbol_error(offset);
        };
        let name = match normalize_symbol(&definition[..equal]) {
            Ok(name) => name,
            Err(_) => return symbol_error(offset),
        };
        let raw = &definition[equal + 1..];
        let decoded = if request.arguments.contains_key("OPTION.UNESCAPED") {
            raw.to_vec()
        } else {
            unescape_symbol(raw).ok_or_else(|| HostProblem::Condition {
                name: "SYMBOLERR".into(),
                response: 116,
                response2: i32::try_from(offset).unwrap_or(i32::MAX),
            })?
        };
        if symbols.insert(name, decoded).is_some()
            || symbols.len() > service.limits.max_document_symbols
        {
            return symbol_error(offset);
        }
        offset = offset.saturating_add(definition.len()).saturating_add(1);
    }
    Ok(symbols)
}

fn unsigned_length(
    request: &CicsRequest,
    name: &str,
    response2: i32,
    allow_zero: bool,
) -> Result<usize, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    let parsed = std::str::from_utf8(value.bytes())
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| allow_zero || *value != 0)
        .ok_or_else(|| HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2,
        })?;
    Ok(parsed)
}

fn host_code_page(request: &CicsRequest) -> Result<u16, HostProblem> {
    let Some(value) = request.arguments.get("HOSTCODEPAGE") else {
        return Ok(37);
    };
    std::str::from_utf8(value.bytes())
        .ok()
        .map(str::trim)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2: 7,
        })
}

fn document_for_token<'a>(
    documents: &'a BTreeMap<String, DocumentRecord>,
    run: &Run,
    token: &[u8],
    response2: i32,
) -> Result<&'a DocumentRecord, HostProblem> {
    if token.len() != 16 {
        return Err(HostProblem::Malformed);
    }
    documents
        .get(&token_key(token))
        .filter(|document| {
            document.owner_execution == run.invocation.execution_id.as_str()
                && document.owner_run_unit == run.invocation.run_unit_id.as_str()
                && document.transaction == run.transaction
        })
        .ok_or_else(|| HostProblem::Condition {
            name: "NOTFND".into(),
            response: 13,
            response2,
        })
}

fn replay_write(
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    response: &CicsResponse,
) -> Result<ProviderStateWrite, HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut replay = CicsEffectReplay {
        effect_key: Some(mutation.idempotency_key.as_str().into()),
        owner_execution: Some(run.invocation.execution_id.as_str().into()),
        owner_run_unit: Some(run.invocation.run_unit_id.as_str().into()),
        sequence: Some(mutation.sequence),
        deadline_tick: Some(retention_tick),
        resolution_tick: None,
        request_digest,
        result_digest: Some(result_digest),
        binding_digest: None,
        response: response.clone(),
    };
    replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "cics-effect-replay-v1".into(),
            key: mutation.idempotency_key.as_str().into(),
            version: 1,
            payload: encode_cics_effect_replay(&replay)?,
        },
        expected_version: None,
    })
}

impl DocumentRecord {
    fn usage_bytes(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.bytes.len())
            .chain(
                self.symbols
                    .iter()
                    .map(|(name, value)| name.len() + value.len()),
            )
            .sum()
    }

    fn retrieval_size(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.bytes.len())
            .sum()
    }
}

fn encode_document(document: &DocumentRecord) -> Result<Vec<u8>, HostProblem> {
    let mut out = DOCUMENT_MAGIC.to_vec();
    out.extend_from_slice(&document.token);
    field(&mut out, document.owner_execution.as_bytes())?;
    field(&mut out, document.owner_run_unit.as_bytes())?;
    field(&mut out, document.transaction.as_bytes())?;
    out.extend_from_slice(
        &u32::try_from(document.segments.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for segment in &document.segments {
        out.push(u8::from(segment.binary));
        out.extend_from_slice(&segment.host_code_page.to_be_bytes());
        field(&mut out, &segment.bytes)?;
    }
    out.extend_from_slice(
        &u32::try_from(document.symbols.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &document.symbols {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value)?;
    }
    out.extend_from_slice(
        &u32::try_from(document.bookmarks.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, position) in &document.bookmarks {
        field(&mut out, name.as_bytes())?;
        out.extend_from_slice(
            &u64::try_from(*position)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
    }
    Ok(out)
}

fn decode_document(
    bytes: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<DocumentRecord, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(DOCUMENT_MAGIC.len())? != DOCUMENT_MAGIC || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let token: [u8; 16] = reader
        .take(16)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let owner_execution = read_text(&mut reader, 128)?;
    let owner_run_unit = read_text(&mut reader, 128)?;
    let transaction = read_text(&mut reader, 16)?;
    let segment_count = reader_u32(&mut reader)? as usize;
    if segment_count > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut segments = Vec::with_capacity(segment_count);
    let mut content_bytes = 0usize;
    for _ in 0..segment_count {
        let binary = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let host_code_page = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let value = reader.field(limits.max_screen_bytes)?;
        content_bytes = content_bytes
            .checked_add(value.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if host_code_page == 0 || content_bytes > limits.max_screen_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
        segments.push(DocumentSegment {
            bytes: value,
            binary,
            host_code_page,
        });
    }
    let symbol_count = reader_u32(&mut reader)? as usize;
    if symbol_count > limits.max_document_symbols {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut symbols = BTreeMap::new();
    for _ in 0..symbol_count {
        let name = normalize_symbol(&reader.field(32)?)?;
        let value = reader.field(limits.max_screen_bytes)?;
        if symbols.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let bookmark_count = reader_u32(&mut reader)? as usize;
    if bookmark_count > limits.max_document_bookmarks {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bookmarks = BTreeMap::new();
    for _ in 0..bookmark_count {
        let name = read_text(&mut reader, 16)?;
        let position = usize::try_from(reader_u64(&mut reader)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if position > content_bytes || bookmarks.insert(name, position).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if reader.at != bytes.len()
        || owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || transaction.is_empty()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let document = DocumentRecord {
        token,
        owner_execution,
        owner_run_unit,
        transaction,
        segments,
        symbols,
        bookmarks,
        version,
    };
    if document.usage_bytes() > limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(document)
}

fn encode_template(definition: &CicsDocumentTemplateDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = TEMPLATE_MAGIC.to_vec();
    field(&mut out, definition.name.as_bytes())?;
    field(&mut out, definition.resource.as_bytes())?;
    field(&mut out, &definition.content)?;
    out.extend_from_slice(&definition.host_code_page.to_be_bytes());
    Ok(out)
}

fn decode_template(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsDocumentTemplateDefinition, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(TEMPLATE_MAGIC.len())? != TEMPLATE_MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let name = read_text(&mut reader, 48)?;
    let resource = read_text(&mut reader, 128)?;
    let content = reader.field(limits.max_screen_bytes)?;
    let host_code_page = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    if reader.at != bytes.len() || host_code_page == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsDocumentTemplateDefinition {
        name,
        resource,
        content,
        host_code_page,
    })
}

fn read_text(reader: &mut Reader<'_>, maximum: usize) -> Result<String, HostProblem> {
    String::from_utf8(reader.field(maximum)?).map_err(|_| HostProblem::InfrastructureFailure)
}

fn reader_u32(reader: &mut Reader<'_>) -> Result<u32, HostProblem> {
    Ok(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
}

fn reader_u64(reader: &mut Reader<'_>) -> Result<u64, HostProblem> {
    Ok(u64::from_be_bytes(
        reader
            .take(8)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
}

fn generated_token(run: &Run, effect_key: &str) -> [u8; 16] {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.cics.document-token@1\0");
    hash_field(&mut digest, run.invocation.execution_id.as_str().as_bytes());
    hash_field(&mut digest, run.invocation.run_unit_id.as_str().as_bytes());
    hash_field(&mut digest, effect_key.as_bytes());
    digest.finalize()[..16]
        .try_into()
        .expect("SHA-256 prefix has fixed length")
}

fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn token_key(token: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut key = String::with_capacity(token.len() * 2);
    for byte in token {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    key
}

fn normalize_template_name(name: &str) -> Result<String, HostProblem> {
    let name = name.trim().to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 48
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name)
}

fn normalize_resource_name(name: &str) -> Result<String, HostProblem> {
    let name = name.trim().to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name)
}

fn normalize_symbol(name: &[u8]) -> Result<String, HostProblem> {
    let name = std::str::from_utf8(name)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .trim()
        .to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(name)
}

fn invalid_delimiter(value: u8) -> bool {
    value.is_ascii_alphanumeric()
        || value.is_ascii_whitespace()
        || matches!(value, b'=' | b'%' | b'+')
}

fn symbol_error<T>(offset: usize) -> Result<T, HostProblem> {
    Err(HostProblem::Condition {
        name: "SYMBOLERR".into(),
        response: 116,
        response2: i32::try_from(offset).unwrap_or(i32::MAX),
    })
}

fn unescape_symbol(value: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(value.len());
    let mut at = 0usize;
    while at < value.len() {
        match value[at] {
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < value.len() => {
                out.push((hex_digit(value[at + 1])? << 4) | hex_digit(value[at + 2])?);
                at += 3;
            }
            b'%' => return None,
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    Some(out)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn substitute_symbols(template: &[u8], symbols: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(template.len());
    let mut at = 0usize;
    while at < template.len() {
        if template[at] == b'&'
            && let Some(end) = template[at + 1..].iter().position(|byte| *byte == b';')
        {
            let end = at + 1 + end;
            if let Ok(name) = normalize_symbol(&template[at + 1..end])
                && let Some(value) = symbols.get(&name)
            {
                out.extend_from_slice(value);
                at = end + 1;
                continue;
            }
        }
        out.push(template[at]);
        at += 1;
    }
    out
}
