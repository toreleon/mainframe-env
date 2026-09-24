//! Extraction from the conversation-open protocol ledger plus task-local
//! presentation metadata. This module never owns protocol state transitions.

mod extract;
mod metadata;
mod process;

use super::super::{CicsService, Run, decimal_payload};
use super::store_error;
use crate::conversation_protocol::{
    ConversationContext, ConversationKind, ConversationLedger, ConversationOwner,
    ConversationProblem, ConversationRecord,
};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::ProviderStateRecord;
pub(in crate::service) use metadata::ExtractMetadata;
#[cfg(test)]
pub(in crate::service) use metadata::LuName;
use metadata::{MAX_ROW_BYTES, MAX_UNRESOLVED_REPLAYS, MutationReplay, NAMESPACE};
use std::collections::BTreeMap;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let ledger = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let key = run.invocation.run_unit_id.as_str();
    let row = service
        .store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?;
    let mut metadata = if let Some(row) = &row {
        decode(row, key)?
    } else {
        ExtractMetadata::for_run_unit(key.into())
    };
    let owner = owner(run);
    let mutation = mutation_identity(request)?;
    if let Some((effect_key, digest)) = &mutation
        && let Some(replay) = metadata
            .mutation_replays
            .iter()
            .chain(metadata.last_mutation.iter())
            .find(|replay| replay.key == *effect_key)
    {
        if replay.request_sha256 != *digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut response = normal(service, run)?;
        for (name, bytes) in &replay.outputs {
            response.outputs.insert(
                name.clone(),
                payload(&replay.output_schemas[name], bytes.clone())?,
            );
        }
        return Ok(response);
    }
    let response = match request.operation {
        CicsOperation::ExtractAttach => {
            extract::attach(service, run, request, &ledger, &metadata, &owner)
        }
        CicsOperation::ExtractAttributes | CicsOperation::GdsExtractAttributes => {
            extract::attributes(service, run, request, &ledger, &metadata, &owner)
        }
        CicsOperation::ExtractLogonMsg => {
            extract::logon(service, run, request, &ledger, &metadata, &owner)
        }
        CicsOperation::ExtractProcess | CicsOperation::GdsExtractProcess => {
            process::invoke(service, run, request, &ledger, &metadata, &owner)
        }
        CicsOperation::ExtractTct => {
            extract::tct(service, run, request, &ledger, &metadata, &owner)
        }
        CicsOperation::Point => extract::point(service, run, request, &ledger, &metadata, &owner),
        _ => Err(HostProblem::InfrastructureFailure),
    }?;
    if let Some((effect_key, digest)) = mutation {
        // Keep any reply whose outer replay insert has not yet succeeded.
        // Later task effects may execute before a caller retries an unknown
        // outcome, so replacing only the most recent reply is insufficient.
        let previous = metadata
            .mutation_replays
            .drain(..)
            .chain(metadata.last_mutation.take());
        let mut unresolved = Vec::new();
        for replay in previous {
            if service
                .store
                .get_provider_state("cics-effect-replay-v1", &replay.key)
                .map_err(store_error)?
                .is_none()
            {
                unresolved.push(replay);
            }
        }
        if unresolved.len() >= MAX_UNRESOLVED_REPLAYS {
            return Err(HostProblem::ResourceExhausted);
        }
        metadata.mutation_replays = unresolved;
        match request.operation {
            CicsOperation::ExtractLogonMsg => metadata.logon_consumed = true,
            CicsOperation::Point => {
                metadata.selected_token =
                    Some(select_facility(&ledger, &metadata, &owner, request)?.token);
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        let mut outputs = BTreeMap::new();
        let mut output_schemas = BTreeMap::new();
        for (name, value) in &response.outputs {
            outputs.insert(name.clone(), value.bytes().to_vec());
            output_schemas.insert(name.clone(), value.schema().to_string());
        }
        metadata.mutation_replays.push(MutationReplay {
            key: effect_key,
            request_sha256: digest,
            outputs,
            output_schemas,
        });
        write_metadata(service, &metadata, row.as_ref().map(|row| row.version))?;
    }
    Ok(response)
}

/// The conversation-open and terminal owners publish presentation facts only.
/// Protocol records and attach headers remain in ConversationLedger.
#[allow(dead_code)]
pub(in crate::service) fn publish_metadata(
    service: &CicsService,
    mut metadata: ExtractMetadata,
    expected_version: Option<u64>,
) -> Result<(), HostProblem> {
    metadata.validate()?;
    if !service
        .lock()?
        .runs
        .keys()
        .any(|id| id.as_str() == metadata.run_unit)
    {
        return Err(HostProblem::Unauthorized);
    }
    if let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, &metadata.run_unit)
        .map_err(store_error)?
    {
        let current = decode(&row, &metadata.run_unit)?;
        if current.logon_consumed && current.logon_message != metadata.logon_message {
            return Err(HostProblem::IdempotencyConflict);
        }
        metadata.selected_token = current.selected_token;
        metadata.logon_consumed = current.logon_consumed;
        metadata.mutation_replays = current.mutation_replays;
        metadata.last_mutation = current.last_mutation;
    } else if metadata.selected_token.is_some()
        || metadata.logon_consumed
        || !metadata.mutation_replays.is_empty()
        || metadata.last_mutation.is_some()
    {
        return Err(HostProblem::Malformed);
    }
    write_metadata(service, &metadata, expected_version)
}

fn decode(row: &ProviderStateRecord, key: &str) -> Result<ExtractMetadata, HostProblem> {
    if row.namespace != NAMESPACE
        || row.key != key
        || row.version == 0
        || row.payload.len() > MAX_ROW_BYTES
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let metadata: ExtractMetadata =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    metadata.validate()?;
    if metadata.run_unit != key {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(metadata)
}

fn write_metadata(
    service: &CicsService,
    metadata: &ExtractMetadata,
    expected: Option<u64>,
) -> Result<(), HostProblem> {
    metadata.validate()?;
    let payload = serde_json::to_vec(metadata).map_err(|_| HostProblem::InfrastructureFailure)?;
    if payload.len() > MAX_ROW_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: metadata.run_unit.clone(),
                version: expected.map_or(1, |version| version + 1),
                payload,
            },
            expected,
        )
        .map_err(store_error)
}

fn mutation_identity(request: &CicsRequest) -> Result<Option<(String, String)>, HostProblem> {
    if !request.operation.is_mutating() {
        return Ok(None);
    }
    let key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str()
        .to_string();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    Ok(Some((
        key,
        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    )))
}

pub(super) fn owner(run: &Run) -> ConversationOwner {
    ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    }
}

pub(super) fn context(run: &Run) -> Result<ConversationContext, HostProblem> {
    let Some(value) = run.invocation.bindings.get("cics.execution-context") else {
        return Ok(ConversationContext::Local);
    };
    if value.schema() != "mainframe-env.cics.execution-context@1" {
        return Err(HostProblem::Malformed);
    }
    match value.bytes() {
        b"local" => Ok(ConversationContext::Local),
        b"dpl-synconreturn" | b"dpl-without-synconreturn" | b"dpl-executionset-subset" => {
            Ok(ConversationContext::DplServer)
        }
        _ => Err(HostProblem::Malformed),
    }
}

pub(super) fn select_facility<'a>(
    ledger: &'a ConversationLedger,
    metadata: &ExtractMetadata,
    owner: &ConversationOwner,
    request: &CicsRequest,
) -> Result<&'a ConversationRecord, HostProblem> {
    let token = if let Some(value) = request.arguments.get("CONVID") {
        value
            .bytes()
            .try_into()
            .map_err(|_| HostProblem::Malformed)?
    } else if let Some(session) = text(request, "SESSION")? {
        *metadata
            .session_names
            .get(session.trim_end())
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?
    } else {
        ledger
            .conversations
            .values()
            .find(|record| record.principal_facility && &record.owner == owner && !record.released)
            .map(|record| record.token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?
    };
    let record = ledger
        .conversation(token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
    match record.check_owner(owner, ConversationContext::Local) {
        Ok(()) => Ok(record),
        Err(ConversationProblem::NotOwned | ConversationProblem::WrongState) => {
            Err(condition("NOTALLOC", 61, 0))
        }
        Err(ConversationProblem::StaleOwner) => Err(HostProblem::Unauthorized),
        Err(_) => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn normal(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
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

pub(super) fn gds_response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    code: [u8; 6],
) -> Result<CicsResponse, HostProblem> {
    let mut response = normal(service, run)?;
    if request.arguments.contains_key("RETCODE") {
        response
            .outputs
            .insert("RETCODE".into(), bytes(code.to_vec())?);
    }
    Ok(response)
}

pub(super) fn bytes(value: Vec<u8>) -> Result<BoundedPayload, HostProblem> {
    payload("mainframe-env.cics.payload@1", value)
}
pub(super) fn payload(schema: &str, value: Vec<u8>) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(schema, value, InvocationLimits::default())
        .map_err(|_| HostProblem::ResourceExhausted)
}
pub(super) fn number(value: i64) -> Result<BoundedPayload, HostProblem> {
    decimal_payload(value)
}
pub(super) fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
pub(super) fn text(request: &CicsRequest, name: &str) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed))
        .transpose()
}
pub(super) fn capacity(request: &CicsRequest, name: &str) -> Result<usize, HostProblem> {
    text(request, name)?
        .ok_or(HostProblem::Malformed)?
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed: &[&str] = match request.operation {
        CicsOperation::ExtractAttach => &[
            "ATTACHID",
            "CONVID",
            "SESSION",
            "PROCESS",
            "RESOURCE",
            "RPROCESS",
            "RRESOURCE",
            "QUEUE",
            "IUTYPE",
            "DATASTR",
            "RECFM",
            "PROCESS.MAXLENGTH",
            "RESOURCE.MAXLENGTH",
            "RPROCESS.MAXLENGTH",
            "RRESOURCE.MAXLENGTH",
            "QUEUE.MAXLENGTH",
        ],
        CicsOperation::ExtractAttributes => &["CONVID", "SESSION", "STATE"],
        CicsOperation::GdsExtractAttributes => &["CONVID", "STATE", "CONVDATA", "RETCODE"],
        CicsOperation::ExtractLogonMsg => {
            &["INTO", "SET", "LENGTH", "INTO.MAXLENGTH", "SET.MAXLENGTH"]
        }
        CicsOperation::ExtractProcess => &[
            "CONVID",
            "SESSION",
            "PROCNAME",
            "PROCLENGTH",
            "MAXPROCLEN",
            "SYNCLEVEL",
            "PIPLIST",
            "PIPLENGTH",
            "PROCNAME.MAXLENGTH",
            "PIPLIST.MAXLENGTH",
        ],
        CicsOperation::GdsExtractProcess => &[
            "CONVID",
            "PROCNAME",
            "PROCLENGTH",
            "MAXPROCLEN",
            "SYNCLEVEL",
            "PIPLIST",
            "PIPLENGTH",
            "RETCODE",
            "PROCNAME.MAXLENGTH",
            "PIPLIST.MAXLENGTH",
        ],
        CicsOperation::ExtractTct => &["NETNAME", "SYSID", "TERMID"],
        CicsOperation::Point => &["CONVID", "SESSION"],
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    for (name, value) in &request.arguments {
        let schema = value.schema();
        if name == "OPTION.NOHANDLE" {
            if schema != "mainframe-env.cics.option@1" || !value.bytes().is_empty() {
                return Err(HostProblem::Malformed);
            }
        } else if name == "RESP" || name == "RESP2" || allowed.contains(&name.as_str()) {
            let expected = if name.ends_with(".MAXLENGTH") || name == "MAXPROCLEN" {
                "mainframe-env.cics.decimal@1"
            } else if matches!(name.as_str(), "ATTACHID" | "CONVID" | "SESSION" | "NETNAME") {
                "mainframe-env.cics.storage-value@1"
            } else {
                "mainframe-env.cics.argument@1"
            };
            if schema != expected
                && !(matches!(name.as_str(), "ATTACHID" | "CONVID" | "SESSION" | "NETNAME")
                    && schema == "mainframe-env.cics.literal@1")
            {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    let has = |name| request.arguments.contains_key(name);
    if has("RESP2") && !has("RESP")
        || ["ATTACHID", "CONVID", "SESSION"]
            .iter()
            .filter(|name| has(name))
            .count()
            > 1
        || matches!(request.operation, CicsOperation::ExtractLogonMsg)
            && (!has("LENGTH") || has("INTO") == has("SET"))
        || matches!(request.operation, CicsOperation::ExtractAttributes) && !has("STATE")
        || matches!(
            request.operation,
            CicsOperation::GdsExtractAttributes | CicsOperation::GdsExtractProcess
        ) && (!has("CONVID") || !has("RETCODE"))
        || matches!(request.operation, CicsOperation::GdsExtractAttributes) && !has("CONVDATA")
        || matches!(request.operation, CicsOperation::ExtractTct)
            && (!has("NETNAME") || has("SYSID") == has("TERMID"))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
