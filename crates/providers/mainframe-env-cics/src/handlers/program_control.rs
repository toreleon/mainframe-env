use super::super::{
    CicsService, Run, argument_bytes, argument_text, bounded, normalize_terminal_name,
};
use super::{field, store_error};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, HostResult, ProgramLinkSelection, ProgramName, ProgramRequest,
};
use mainframe_env_store_api::{
    ArtifactStore, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const PROGRAM_DEFINITION_NAMESPACE: &str = "cics-program-definition-v1";
const APPLICATION_ENTRY_NAMESPACE: &str = "cics-application-entry-v1";
const PROGRAM_MAGIC: &[u8; 7] = b"MECPGD1";
const APPLICATION_MAGIC: &[u8; 7] = b"MECAED1";

mod invoke_application;
mod load;
mod release;
mod transfer_selection;
pub(in crate::service) use load::{
    ProgramLoadState, load_program_loads, release_task_program_loads,
};
pub(in crate::service) use transfer_selection::freeze as freeze_program_transfer;
pub(in crate::service) use transfer_selection::validate_frozen_selection;
pub(crate) use transfer_selection::validate_response as validate_transfer_selection;

/// One immutable installed program generation available to CICS program control.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsProgramDefinition {
    pub name: String,
    pub generation: u64,
    pub artifact: ArtifactRef,
    pub semantic_identity: String,
    pub entry_offset: u32,
    pub enabled: bool,
    pub remote: bool,
    pub reload: bool,
    pub java_status: CicsJavaStatus,
}

/// Source-visible Java entry-point readiness for INVOKE APPLICATION.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsJavaStatus {
    NotJava,
    ClassUnavailable,
    ServerNotFound,
    ServerDisabled,
    Available,
}

/// One AVAILABLE application-operation binding to an immutable program generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsApplicationEntryDefinition {
    pub application: String,
    pub platform: String,
    pub major_version: u32,
    pub minor_version: u32,
    pub micro_version: u32,
    pub operation: String,
    pub program: String,
    pub program_generation: u64,
    pub program_artifact: ArtifactRef,
    pub application_identity: String,
    pub available: bool,
}

impl CicsService {
    pub fn register_programs(&self, programs: &BTreeSet<String>) -> Result<(), HostProblem> {
        let normalized = programs
            .iter()
            .map(|program| normalize_terminal_name(program, 128))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if normalized.len() != programs.len() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut state = self.lock()?;
        let additions = normalized
            .iter()
            .filter(|program| !state.programs.contains(*program))
            .count();
        if state
            .programs
            .len()
            .checked_add(additions)
            .is_none_or(|total| total > self.limits.max_programs)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let writes = normalized
            .iter()
            .filter(|program| !state.programs.contains(*program))
            .map(|program| ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-program".into(),
                    key: program.clone(),
                    version: 1,
                    payload: Vec::new(),
                },
                expected_version: None,
            })
            .collect::<Vec<_>>();
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        state.programs.extend(normalized);
        Ok(())
    }

    /// Bind the shared immutable artifact authority used by program LOAD and application invoke.
    pub fn bind_artifact_store(
        &self,
        artifacts: Arc<dyn ArtifactStore>,
    ) -> Result<(), HostProblem> {
        {
            let state = self.lock()?;
            for definition in state
                .program_definitions
                .values()
                .flat_map(BTreeMap::values)
            {
                validate_program_artifact(artifacts.as_ref(), definition)?;
            }
        }
        self.artifacts
            .set(artifacts)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }

    /// Register immutable program generations without replacing an existing identity.
    pub fn register_program_definitions(
        &self,
        definitions: &[CicsProgramDefinition],
    ) -> Result<(), HostProblem> {
        register_program_definitions(self, definitions)
    }

    /// Register AVAILABLE application operation bindings used by INVOKE APPLICATION.
    pub fn register_application_entries(
        &self,
        entries: &[CicsApplicationEntryDefinition],
    ) -> Result<(), HostProblem> {
        register_application_entries(self, entries)
    }
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::Inquire => inquire(service, run, request),
        CicsOperation::InvokeApplication => invoke_application::invoke(service, run, request),
        CicsOperation::Load => load::invoke(service, run, request),
        CicsOperation::Release => release::invoke(service, run, request),
        CicsOperation::Link | CicsOperation::Xctl => transfer(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(in crate::service) fn validate_program_artifact(
    artifacts: &dyn ArtifactStore,
    definition: &CicsProgramDefinition,
) -> Result<(), HostProblem> {
    let record = artifacts
        .get_artifact(&definition.artifact)
        .map_err(store_error)?
        .ok_or(HostProblem::NotFound)?;
    let payload_digest: [u8; 32] = Sha256::digest(&record.payload).into();
    if record.artifact != definition.artifact
        || record.payload_digest != payload_digest
        || record.artifact.as_str() != format!("sha256:{:x}", Sha256::digest(&record.payload))
        || usize::try_from(definition.entry_offset)
            .ok()
            .is_none_or(|offset| offset > record.payload.len())
        || record.executable.as_ref().is_none_or(|metadata| {
            metadata.semantic_identity != definition.semantic_identity
                || !metadata.validates_payload(&payload_digest)
        })
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

pub(in crate::service) fn load_program_definitions(
    store: &dyn ProviderStateStore,
    limits: super::super::CicsLimits,
) -> Result<BTreeMap<String, BTreeMap<u64, CicsProgramDefinition>>, HostProblem> {
    let mut definitions = BTreeMap::<String, BTreeMap<u64, CicsProgramDefinition>>::new();
    for row in store
        .list_provider_state(PROGRAM_DEFINITION_NAMESPACE, limits.max_programs)
        .map_err(store_error)?
    {
        let definition = decode_program_definition(&row.payload)?;
        if row.version != 1
            || row.key != program_definition_key(&definition.name, definition.generation)
            || definitions
                .entry(definition.name.clone())
                .or_default()
                .insert(definition.generation, definition)
                .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(definitions)
}

pub(in crate::service) fn load_application_entries(
    store: &dyn ProviderStateStore,
    limits: super::super::CicsLimits,
) -> Result<Vec<CicsApplicationEntryDefinition>, HostProblem> {
    let mut entries = Vec::new();
    let mut keys = BTreeSet::new();
    for row in store
        .list_provider_state(APPLICATION_ENTRY_NAMESPACE, limits.max_programs)
        .map_err(store_error)?
    {
        let entry = decode_application_entry(&row.payload)?;
        let key = application_entry_key(&entry);
        if row.version != 1 || row.key != key || !keys.insert(key) {
            return Err(HostProblem::InfrastructureFailure);
        }
        entries.push(entry);
    }
    Ok(entries)
}

pub(in crate::service) fn validate_application_catalog(
    programs: &BTreeMap<String, BTreeMap<u64, CicsProgramDefinition>>,
    entries: &[CicsApplicationEntryDefinition],
) -> Result<(), HostProblem> {
    for entry in entries {
        let Some(program) = programs
            .get(&entry.program)
            .and_then(|generations| generations.get(&entry.program_generation))
        else {
            return Err(HostProblem::InfrastructureFailure);
        };
        if program.artifact != entry.program_artifact {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

pub(in crate::service) fn register_program_definitions(
    service: &CicsService,
    definitions: &[CicsProgramDefinition],
) -> Result<(), HostProblem> {
    if definitions.is_empty() || definitions.len() > service.limits.max_programs {
        return Err(HostProblem::Malformed);
    }
    let artifacts = service
        .artifacts
        .get()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut normalized = Vec::with_capacity(definitions.len());
    let mut supplied = BTreeSet::new();
    for definition in definitions {
        let mut definition = definition.clone();
        definition.name = normalize_program_name(&definition.name)?;
        if definition.generation == 0
            || !valid_semantic_identity(&definition.semantic_identity)
            || !supplied.insert((definition.name.clone(), definition.generation))
        {
            return Err(HostProblem::Malformed);
        }
        validate_program_artifact(artifacts.as_ref(), &definition)?;
        normalized.push(definition);
    }
    let mut state = service.lock()?;
    let existing_count = state
        .program_definitions
        .values()
        .map(BTreeMap::len)
        .sum::<usize>();
    let additions = normalized
        .iter()
        .filter(|definition| {
            !state
                .program_definitions
                .get(&definition.name)
                .is_some_and(|generations| generations.contains_key(&definition.generation))
        })
        .count();
    if existing_count
        .checked_add(additions)
        .is_none_or(|count| count > service.limits.max_programs)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut writes = Vec::new();
    for definition in &normalized {
        if let Some(existing) = state
            .program_definitions
            .get(&definition.name)
            .and_then(|generations| generations.get(&definition.generation))
        {
            if existing != definition {
                return Err(HostProblem::IdempotencyConflict);
            }
            continue;
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: PROGRAM_DEFINITION_NAMESPACE.into(),
                key: program_definition_key(&definition.name, definition.generation),
                version: 1,
                payload: encode_program_definition(definition)?,
            },
            expected_version: None,
        });
        if !state.programs.contains(&definition.name)
            && !writes.iter().any(|write| {
                write.record.namespace == "cics-program" && write.record.key == definition.name
            })
        {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-program".into(),
                    key: definition.name.clone(),
                    version: 1,
                    payload: Vec::new(),
                },
                expected_version: None,
            });
        }
    }
    if !writes.is_empty() {
        service
            .store
            .put_provider_states_atomic(writes)
            .map_err(store_error)?;
    }
    for definition in normalized {
        state.programs.insert(definition.name.clone());
        state
            .program_definitions
            .entry(definition.name.clone())
            .or_default()
            .insert(definition.generation, definition);
    }
    Ok(())
}

pub(in crate::service) fn register_application_entries(
    service: &CicsService,
    entries: &[CicsApplicationEntryDefinition],
) -> Result<(), HostProblem> {
    if entries.is_empty() || entries.len() > service.limits.max_programs {
        return Err(HostProblem::Malformed);
    }
    let mut normalized = Vec::with_capacity(entries.len());
    let mut supplied = BTreeSet::new();
    for entry in entries {
        let mut entry = entry.clone();
        entry.application = normalize_application_name(&entry.application)?;
        entry.platform = normalize_application_name(&entry.platform)?;
        entry.operation = normalize_application_name(&entry.operation)?;
        entry.program = normalize_program_name(&entry.program)?;
        let key = application_entry_key(&entry);
        if entry.program_generation == 0
            || !valid_content_identity(&entry.application_identity)
            || !supplied.insert(key)
        {
            return Err(HostProblem::Malformed);
        }
        normalized.push(entry);
    }
    let mut state = service.lock()?;
    if state
        .application_entries
        .len()
        .checked_add(normalized.len())
        .is_none_or(|count| count > service.limits.max_programs)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut writes = Vec::new();
    for entry in &normalized {
        let program = state
            .program_definitions
            .get(&entry.program)
            .and_then(|generations| generations.get(&entry.program_generation))
            .ok_or(HostProblem::NotFound)?;
        if program.artifact != entry.program_artifact {
            return Err(HostProblem::IdempotencyConflict);
        }
        let key = application_entry_key(entry);
        if let Some(existing) = state
            .application_entries
            .iter()
            .find(|existing| application_entry_key(existing) == key)
        {
            if existing != entry {
                return Err(HostProblem::IdempotencyConflict);
            }
            continue;
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: APPLICATION_ENTRY_NAMESPACE.into(),
                key,
                version: 1,
                payload: encode_application_entry(entry)?,
            },
            expected_version: None,
        });
    }
    if !writes.is_empty() {
        service
            .store
            .put_provider_states_atomic(writes)
            .map_err(store_error)?;
    }
    let existing_keys = state
        .application_entries
        .iter()
        .map(application_entry_key)
        .collect::<BTreeSet<_>>();
    state.application_entries.extend(
        normalized
            .into_iter()
            .filter(|entry| !existing_keys.contains(&application_entry_key(entry))),
    );
    Ok(())
}

fn decimal_usize(value: &BoundedPayload) -> Option<usize> {
    (value.schema() == "mainframe-env.cics.decimal@1")
        .then(|| std::str::from_utf8(value.bytes()).ok()?.parse().ok())
        .flatten()
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn normalize_program_name(value: &str) -> Result<String, HostProblem> {
    let value = value.trim().to_ascii_uppercase();
    if matches!(value.len(), 1..=8) && value.bytes().all(valid_program_character) {
        Ok(value)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn normalize_application_name(value: &str) -> Result<String, HostProblem> {
    let value = value.trim().to_ascii_uppercase();
    if matches!(value.len(), 1..=64)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'#' | b'@' | b'-')
        })
    {
        Ok(value)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn valid_content_identity(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_semantic_identity(value: &str) -> bool {
    value
        .strip_prefix("semantic-sha256:")
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn program_definition_key(name: &str, generation: u64) -> String {
    format!("{name}:{generation:020}")
}

fn application_entry_key(entry: &CicsApplicationEntryDefinition) -> String {
    format!(
        "{}/{}/{:010}.{:010}.{:010}/{}",
        entry.platform,
        entry.application,
        entry.major_version,
        entry.minor_version,
        entry.micro_version,
        entry.operation
    )
}

fn encode_program_definition(definition: &CicsProgramDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = PROGRAM_MAGIC.to_vec();
    field(&mut out, definition.name.as_bytes())?;
    out.extend_from_slice(&definition.generation.to_be_bytes());
    field(&mut out, definition.artifact.as_str().as_bytes())?;
    field(&mut out, definition.semantic_identity.as_bytes())?;
    out.extend_from_slice(&definition.entry_offset.to_be_bytes());
    out.extend([
        u8::from(definition.enabled),
        u8::from(definition.remote),
        u8::from(definition.reload),
        java_status_code(definition.java_status),
    ]);
    Ok(out)
}

fn decode_program_definition(bytes: &[u8]) -> Result<CicsProgramDefinition, HostProblem> {
    let mut reader = ProgramReader::new(bytes, PROGRAM_MAGIC)?;
    let name = normalize_program_name(&reader.text(8)?)?;
    let generation = reader.u64()?;
    let artifact = ArtifactRef::new(reader.text(80)?, InvocationLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let semantic_identity = reader.text(80)?;
    let entry_offset = reader.u32()?;
    let flags = reader.take(4)?;
    let definition = CicsProgramDefinition {
        name,
        generation,
        artifact,
        semantic_identity,
        entry_offset,
        enabled: flag(flags[0])?,
        remote: flag(flags[1])?,
        reload: flag(flags[2])?,
        java_status: decode_java_status(flags[3])?,
    };
    if !reader.done() || generation == 0 || !valid_semantic_identity(&definition.semantic_identity)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(definition)
}

fn encode_application_entry(
    entry: &CicsApplicationEntryDefinition,
) -> Result<Vec<u8>, HostProblem> {
    let mut out = APPLICATION_MAGIC.to_vec();
    for value in [
        entry.application.as_bytes(),
        entry.platform.as_bytes(),
        entry.operation.as_bytes(),
        entry.program.as_bytes(),
    ] {
        field(&mut out, value)?;
    }
    out.extend_from_slice(&entry.major_version.to_be_bytes());
    out.extend_from_slice(&entry.minor_version.to_be_bytes());
    out.extend_from_slice(&entry.micro_version.to_be_bytes());
    out.extend_from_slice(&entry.program_generation.to_be_bytes());
    field(&mut out, entry.program_artifact.as_str().as_bytes())?;
    field(&mut out, entry.application_identity.as_bytes())?;
    out.push(u8::from(entry.available));
    Ok(out)
}

fn decode_application_entry(bytes: &[u8]) -> Result<CicsApplicationEntryDefinition, HostProblem> {
    let mut reader = ProgramReader::new(bytes, APPLICATION_MAGIC)?;
    let application = normalize_application_name(&reader.text(64)?)?;
    let platform = normalize_application_name(&reader.text(64)?)?;
    let operation = normalize_application_name(&reader.text(64)?)?;
    let program = normalize_program_name(&reader.text(8)?)?;
    let major_version = reader.u32()?;
    let minor_version = reader.u32()?;
    let micro_version = reader.u32()?;
    let program_generation = reader.u64()?;
    let program_artifact = ArtifactRef::new(reader.text(80)?, InvocationLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let application_identity = reader.text(80)?;
    let available = flag(reader.take(1)?[0])?;
    let entry = CicsApplicationEntryDefinition {
        application,
        platform,
        major_version,
        minor_version,
        micro_version,
        operation,
        program,
        program_generation,
        program_artifact,
        application_identity,
        available,
    };
    if !reader.done()
        || program_generation == 0
        || !valid_content_identity(&entry.application_identity)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(entry)
}

fn flag(value: u8) -> Result<bool, HostProblem> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

const fn java_status_code(status: CicsJavaStatus) -> u8 {
    match status {
        CicsJavaStatus::NotJava => 0,
        CicsJavaStatus::ClassUnavailable => 1,
        CicsJavaStatus::ServerNotFound => 2,
        CicsJavaStatus::ServerDisabled => 3,
        CicsJavaStatus::Available => 4,
    }
}

fn decode_java_status(value: u8) -> Result<CicsJavaStatus, HostProblem> {
    match value {
        0 => Ok(CicsJavaStatus::NotJava),
        1 => Ok(CicsJavaStatus::ClassUnavailable),
        2 => Ok(CicsJavaStatus::ServerNotFound),
        3 => Ok(CicsJavaStatus::ServerDisabled),
        4 => Ok(CicsJavaStatus::Available),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

struct ProgramReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> ProgramReader<'a> {
    fn new(bytes: &'a [u8], magic: &[u8]) -> Result<Self, HostProblem> {
        if !bytes.starts_with(magic) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Self {
            bytes,
            at: magic.len(),
        })
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn field(&mut self, maximum: usize) -> Result<&'a [u8], HostProblem> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
        if length > maximum {
            return Err(HostProblem::InfrastructureFailure);
        }
        self.take(length)
    }

    fn text(&mut self, maximum: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.field(maximum)?.to_vec())
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn u32(&mut self) -> Result<u32, HostProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, HostProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn inquire(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let target = argument_text(request, "PROGRAM")?
        .trim()
        .to_ascii_uppercase();
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.PROGRAM.{target}"),
        AccessIntent::Execute,
    )?;
    if service.lock()?.programs.contains(&target) {
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    let program = ProgramName::new(target, 128).map_err(|_| HostProblem::Malformed)?;
    match service.nested(
        run,
        HostRequest::Program(ProgramRequest::Inquire { program }),
    ) {
        Err(HostProblem::NotFound) => Err(HostProblem::Condition {
            name: "PGMIDERR".into(),
            response: 27,
            response2: 0,
        }),
        Err(problem) => Err(problem),
        Ok(HostResult::Program(_)) => service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        ),
        Ok(_) => Err(HostProblem::ProviderFailure),
    }
}

fn valid_program_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#')
}

fn transfer(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_transfer_request(request)?;
    let target = argument_text(request, "PROGRAM")?
        .trim()
        .to_ascii_uppercase();
    if !matches!(target.len(), 1..=8) || !target.bytes().all(valid_program_character) {
        return Err(HostProblem::Malformed);
    }
    service
        .authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{target}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized if request.operation == CicsOperation::Link => {
                condition("NOTAUTH", 70, 101)
            }
            problem => problem,
        })?;
    let program = ProgramName::new(target.clone(), 128).map_err(|_| HostProblem::Malformed)?;
    let mut payload = argument_bytes(request, "COMMAREA").unwrap_or_default();
    if let Some(length) = request.arguments.get("LENGTH") {
        let length = std::str::from_utf8(length.bytes())
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| {
                *value <= 32_763 && (request.operation != CicsOperation::Link || *value != 0)
            })
            .ok_or_else(invalid_commarea_length)?;
        if !request.arguments.contains_key("COMMAREA") && length != 0 {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 26,
            });
        }
        if length > payload.len() {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: if request.operation == CicsOperation::Xctl {
                    28
                } else {
                    0
                },
            });
        }
        payload.truncate(length);
    }
    let commarea_limit = (request.operation == CicsOperation::Link
        && request.arguments.contains_key("COMMAREA"))
    .then_some(payload.len());
    let payload = bounded(payload)?;
    if request.operation == CicsOperation::Xctl && service.lock()?.programs.contains(&target) {
        let mut response = service.response(
            run,
            CicsDisposition::Transfer,
            "NORMAL",
            0,
            0,
            Some(target),
            None,
            payload.bytes().to_vec(),
        )?;
        freeze_program_transfer(service, &mut response)?;
        return Ok(response);
    }
    let host_request = if request.operation == CicsOperation::Link {
        HostRequest::Program(ProgramRequest::Link {
            program,
            payload,
            selection: local_link_selection(service, &target)?,
        })
    } else {
        HostRequest::Program(ProgramRequest::Xctl { program, payload })
    };
    let result = service.nested(run, host_request);
    if let Some(response) = super::program_abend::unwind(service, run, &result)? {
        return Ok(response);
    }
    let payload = match result {
        Ok(HostResult::Program(payload)) => {
            if let Some(limit) = commarea_limit {
                validate_commarea_reply(&payload, limit)?;
            }
            payload.bytes().to_vec()
        }
        Err(HostProblem::NotFound) if request.operation == CicsOperation::Link => {
            return Err(condition("PGMIDERR", 27, 1));
        }
        Err(problem) => return Err(problem),
        Ok(_) => return Err(HostProblem::ProviderFailure),
    };
    let mut response = service.response(
        run,
        if request.operation == CicsOperation::Link {
            CicsDisposition::Complete
        } else {
            CicsDisposition::Transfer
        },
        "NORMAL",
        0,
        0,
        Some(target),
        None,
        payload.clone(),
    )?;
    if request.operation == CicsOperation::Link && request.arguments.contains_key("COMMAREA") {
        response
            .outputs
            .insert("COMMAREA".into(), bounded(payload)?);
    }
    Ok(response)
}

pub(in crate::service) fn validate_replay_response(
    request: &CicsRequest,
    response: CicsResponse,
) -> Result<CicsResponse, HostProblem> {
    transfer_selection::validate_response(&response)?;
    let maximum = match request.operation {
        CicsOperation::Link => 32_763,
        CicsOperation::InvokeApplication => 24_576,
        _ => return Ok(response),
    };
    if super::program_abend::validate_replay(&response)? {
        return Ok(response);
    }
    if let Some(area) = request.arguments.get("COMMAREA") {
        // Handled source conditions have no COMMAREA result to copy back.
        if response.condition != "NORMAL" || response.response != 0 || response.response2 != 0 {
            if response.condition == "NORMAL"
                || response.response == 0
                || response.outputs.contains_key("COMMAREA")
                || response.payload.schema() != "mainframe-env.cics.payload@1"
                || !response.payload.bytes().is_empty()
            {
                return Err(HostProblem::ProviderFailure);
            }
            return Ok(response);
        }
        let limit = match request.arguments.get("LENGTH") {
            Some(length) => decimal_usize(length)
                .filter(|length| (1..=maximum).contains(length))
                .ok_or(HostProblem::ProviderFailure)?,
            None => area.bytes().len(),
        };
        if limit > area.bytes().len() {
            return Err(HostProblem::ProviderFailure);
        }
        let output = response
            .outputs
            .get("COMMAREA")
            .ok_or(HostProblem::ProviderFailure)?;
        validate_commarea_reply(output, limit)?;
        validate_commarea_reply(&response.payload, limit)?;
        if response.payload.bytes() != output.bytes() {
            return Err(HostProblem::ProviderFailure);
        }
    }
    Ok(response)
}

fn validate_commarea_reply(reply: &BoundedPayload, limit: usize) -> Result<(), HostProblem> {
    if reply.schema() != "mainframe-env.cics.payload@1" || reply.bytes().len() > limit {
        Err(HostProblem::ProviderFailure)
    } else {
        Ok(())
    }
}

fn local_link_selection(
    service: &CicsService,
    target: &str,
) -> Result<Option<ProgramLinkSelection>, HostProblem> {
    let definition = service
        .lock()?
        .program_definitions
        .get(target)
        .and_then(|generations| generations.last_key_value())
        .map(|(_, definition)| definition.clone());
    let Some(definition) = definition else {
        // Name-only compatibility hosts retain their existing dispatch authority.
        return Ok(None);
    };
    if !definition.enabled {
        return Err(condition("PGMIDERR", 27, 2));
    }
    if definition.remote
        || definition.entry_offset != 0
        || definition.java_status != CicsJavaStatus::NotJava
    {
        return Err(HostProblem::Unsupported);
    }
    validate_program_artifact(
        service
            .artifacts
            .get()
            .ok_or_else(|| condition("PGMIDERR", 27, 3))?
            .as_ref(),
        &definition,
    )
    .map_err(|_| condition("PGMIDERR", 27, 3))?;
    let content_identity = format!(
        "sha256:{:x}",
        Sha256::digest(encode_program_definition(&definition)?)
    );
    Ok(Some(ProgramLinkSelection {
        artifact: definition.artifact,
        generation: definition.generation,
        content_identity,
    }))
}

fn validate_transfer_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "COMMAREA",
        "DATALENGTH",
        "LENGTH",
        "OPTION.NOHANDLE",
        "PROGRAM",
        "RESP",
        "RESP2",
    ];
    if !request.arguments.contains_key("PROGRAM")
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || name.starts_with("OPTION.") && !value.bytes().is_empty()
                || name == "PROGRAM"
                    && !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                            | "mainframe-env.cics.argument@1"
                    )
                || name == "COMMAREA"
                    && !matches!(
                        value.schema(),
                        "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.argument@1"
                    )
                || name == "LENGTH" && value.schema() != "mainframe-env.cics.decimal@1"
                || name == "DATALENGTH" && value.schema() != "mainframe-env.cics.decimal@1"
        })
        || request.arguments.contains_key("DATALENGTH")
            && (!request.arguments.contains_key("COMMAREA")
                || !request.arguments.contains_key("LENGTH"))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn invalid_commarea_length() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 11,
    }
}
