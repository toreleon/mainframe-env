//! Versioned at-most-once dispatch for installed COBOL calls. Pending != retryable.
use super::*;
use mainframe_env_store_api::{ProviderStateMutation, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod target;
mod transfer;
pub(super) use transfer::persist_transfer_intent;
pub(super) use transfer::preserve_control_cursor_failure;

use super::retention::{
    CALL_PROTOCOL_NAMESPACE, CALL_REPLAY_NAMESPACE, CobolRetentionDependency,
    CobolRetentionRowDescriptor, CobolRetentionRowKind, CobolRetentionState,
    CobolRetentionValidationError, owner_dependencies, protocol_key, provider_dependency,
    run_state_key, valid_digest, valid_identity, validate_row_identity,
};

const RUN_OWNER_BINDING: &str = "cobol.run-owner-execution";
const RUN_OWNER_BINDING_SCHEMA: &str = "mainframe-env.cobol.run-owner@1";

/// Constructed only in the successful original CALL reservation branch below.
pub(super) struct WinningInstalledCall<'a> {
    parent: &'a Invocation,
    effect: &'a EffectRequest,
    store: &'a Arc<dyn PlatformStore>,
    reservation: &'a ProviderStateRecord,
    child_execution: &'a str,
}
impl WinningInstalledCall<'_> {
    pub(super) fn parent(&self) -> &Invocation {
        self.parent
    }
    pub(super) fn effect(&self) -> &EffectRequest {
        self.effect
    }
    pub(super) fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }
    pub(super) fn reservation(&self) -> &ProviderStateRecord {
        self.reservation
    }
    pub(super) fn child_execution(&self) -> &str {
        self.child_execution
    }
    pub(super) fn recheck(&self) -> Result<(), HostProblem> {
        if self
            .store
            .get_provider_state(&self.reservation.namespace, &self.reservation.key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .as_ref()
            != Some(self.reservation)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reply {
    schema: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReceipt {
    schema_version: u32,
    fingerprint: String,
    child_execution: String,
    reply: Option<Reply>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    replay_key: String,
    fingerprint: String,
    child_execution: String,
    owner_execution: String,
    owner_run_unit: String,
    owner_principal: String,
    protocol_key: String,
    run_state_key: String,
    metadata_digest: String,
    completion_tick: Option<u64>,
    reply: Option<Reply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transfer: Option<transfer::TransferIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<target::TargetStage>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CallProtocol {
    schema_version: u32,
    owner_execution: String,
    owner_run_unit: String,
    owner_principal: String,
    run_state_key: String,
    metadata_digest: String,
    ended_tick: Option<u64>,
}

enum DecodedReceipt {
    Legacy(LegacyReceipt),
    Current(Receipt),
}

enum DecodedProtocol {
    Legacy,
    Current(CallProtocol),
}

fn receipt_metadata_digest(receipt: &Receipt) -> String {
    let mut hash = Sha256::new();
    hash.update(if receipt.schema_version == 4 {
        b"mainframe-env.cobol-call-receipt-metadata@4\0"
    } else if receipt.schema_version == 3 {
        b"mainframe-env.cobol-call-receipt-metadata@3\0"
    } else {
        b"mainframe-env.cobol-call-receipt-metadata@2\0"
    });
    for field in [
        receipt.replay_key.as_bytes(),
        receipt.fingerprint.as_bytes(),
        receipt.child_execution.as_bytes(),
        receipt.owner_execution.as_bytes(),
        receipt.owner_run_unit.as_bytes(),
        receipt.owner_principal.as_bytes(),
        receipt.protocol_key.as_bytes(),
        receipt.run_state_key.as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update(receipt.completion_tick.unwrap_or(0).to_be_bytes());
    match &receipt.reply {
        Some(reply) => {
            hash.update([1]);
            hash.update((reply.schema.len() as u64).to_be_bytes());
            hash.update(reply.schema.as_bytes());
            hash.update((reply.bytes.len() as u64).to_be_bytes());
            hash.update(&reply.bytes);
        }
        None => hash.update([0]),
    }
    if let Some(transfer) = &receipt.transfer {
        hash.update(transfer.metadata_digest().as_bytes());
    }
    if let Some(target) = &receipt.target {
        hash.update(target.metadata_digest().as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn protocol_metadata_digest(protocol: &CallProtocol) -> String {
    let ended = protocol.ended_tick.unwrap_or(0).to_be_bytes();
    digest(&[
        if protocol.schema_version == 3 {
            b"protocol-metadata@3"
        } else {
            b"protocol-metadata"
        },
        protocol.owner_execution.as_bytes(),
        protocol.owner_run_unit.as_bytes(),
        protocol.owner_principal.as_bytes(),
        protocol.run_state_key.as_bytes(),
        &ended,
    ])
}

fn new_call_protocol(
    parent: &Invocation,
    outer_identity: bool,
) -> Result<CallProtocol, HostProblem> {
    let mut protocol = CallProtocol {
        schema_version: if outer_identity { 3 } else { 2 },
        owner_execution: protocol_owner_execution(parent)?,
        owner_run_unit: parent.run_unit_id.as_str().into(),
        owner_principal: parent.principal.id().as_str().into(),
        run_state_key: run_state_key(parent.run_unit_id.as_str(), parent.principal.id().as_str()),
        metadata_digest: String::new(),
        ended_tick: None,
    };
    protocol.metadata_digest = protocol_metadata_digest(&protocol);
    Ok(protocol)
}

pub(super) fn digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.installed-call@1\0");
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn protocol_owner_execution(invocation: &Invocation) -> Result<String, HostProblem> {
    let Some(binding) = invocation.bindings.get(RUN_OWNER_BINDING) else {
        return Ok(invocation.execution_id.as_str().into());
    };
    if binding.schema() != RUN_OWNER_BINDING_SCHEMA {
        return Err(HostProblem::Malformed);
    }
    let owner = std::str::from_utf8(binding.bytes()).map_err(|_| HostProblem::Malformed)?;
    if !valid_identity(owner) {
        return Err(HostProblem::Malformed);
    }
    Ok(owner.into())
}

pub(super) fn bind_protocol_owner(
    parent: &Invocation,
    bindings: &mut BTreeMap<String, BoundedPayload>,
) -> Result<(), HostProblem> {
    let owner = protocol_owner_execution(parent)?;
    bind_run_owner(&owner, bindings)
}

/// Restore an explicit owner without replacing a conflicting existing binding.
pub(super) fn bind_run_owner(
    owner: &str,
    bindings: &mut BTreeMap<String, BoundedPayload>,
) -> Result<(), HostProblem> {
    if !valid_identity(owner) {
        return Err(HostProblem::Malformed);
    }
    if let Some(binding) = bindings.get(RUN_OWNER_BINDING) {
        return if binding.schema() == RUN_OWNER_BINDING_SCHEMA
            && binding.bytes() == owner.as_bytes()
        {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    if bindings.len() >= InvocationLimits::default().max_bindings {
        return Err(HostProblem::ResourceExhausted);
    }
    bindings.insert(
        RUN_OWNER_BINDING.into(),
        BoundedPayload::new(
            RUN_OWNER_BINDING_SCHEMA,
            owner.as_bytes().to_vec(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(())
}

fn identity(parent: &Invocation, effect: &EffectRequest) -> Result<String, HostProblem> {
    let key = effect
        .idempotency_key
        .as_ref()
        .ok_or(HostProblem::Malformed)?;
    if effect.run_unit != parent.run_unit_id {
        return Err(HostProblem::Malformed);
    }
    // A retry/replay is the SAME logical call; a new occurrence needs a new key.
    Ok(digest(&[
        b"identity",
        parent.run_unit_id.as_str().as_bytes(),
        parent.execution_id.as_str().as_bytes(),
        key.as_str().as_bytes(),
    ]))
}

fn outer_program_identity(effect: &EffectRequest) -> Result<bool, HostProblem> {
    let Some(key) = effect.idempotency_key.as_ref() else {
        return Ok(false);
    };
    if !key.as_str().starts_with("cics-program-") {
        return Ok(false);
    }
    let Some(digest) = key.as_str().strip_prefix("cics-program-v2:") else {
        return Err(HostProblem::Unsupported);
    };
    if !valid_digest(digest)
        || effect.sequence == 0
        || !matches!(
            &effect.request,
            HostRequest::Program(ProgramRequest::Link { .. })
        )
    {
        return Err(HostProblem::Malformed);
    }
    Ok(true)
}

fn fingerprint(
    parent: &Invocation,
    effect: &EffectRequest,
    program: &str,
    payload: &BoundedPayload,
) -> Result<String, HostProblem> {
    let grants: Vec<_> = parent
        .principal
        .grants()
        .iter()
        .map(|v| v.as_str())
        .collect();
    let generations: Vec<_> = parent
        .provider_generations
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let bindings: Vec<_> = parent
        .bindings
        .iter()
        .map(|(k, v)| (k.as_str(), v.schema(), v.bytes()))
        .collect();
    let bytes = serde_json::to_vec(&(
        1_u32,
        parent.principal.id().as_str(),
        grants,
        parent.artifact.as_str(),
        bindings,
        generations,
        effect.sequence,
        program.to_ascii_uppercase(),
        payload.schema(),
        payload.bytes(),
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let base = digest(&[b"fingerprint", &bytes]);
    if let HostRequest::Program(ProgramRequest::Link {
        selection: Some(selection),
        ..
    }) = &effect.request
    {
        Ok(digest(&[
            b"selected-link-fingerprint",
            base.as_bytes(),
            selection.artifact.as_str().as_bytes(),
            &selection.generation.to_be_bytes(),
            selection.content_identity.as_bytes(),
        ]))
    } else {
        Ok(base)
    }
}

fn decode_receipt(
    record: &ProviderStateRecord,
) -> Result<DecodedReceipt, CobolRetentionValidationError> {
    validate_row_identity(record, CALL_REPLAY_NAMESPACE)?;
    if record.payload.len() > 64 * 1024 * 1024 {
        return Err(CobolRetentionValidationError::CorruptPayload);
    }
    if let Ok(receipt) = serde_json::from_slice::<Receipt>(&record.payload)
        && matches!(receipt.schema_version, 2 | 3 | 4)
    {
        if !valid_digest(&receipt.fingerprint)
            || receipt.replay_key != record.key
            || !valid_identity(&receipt.child_execution)
            || !valid_identity(&receipt.owner_execution)
            || !valid_identity(&receipt.owner_run_unit)
            || !valid_identity(&receipt.owner_principal)
            || receipt.protocol_key != protocol_key(&receipt.owner_run_unit)
            || receipt.run_state_key
                != run_state_key(&receipt.owner_run_unit, &receipt.owner_principal)
            || receipt.metadata_digest != receipt_metadata_digest(&receipt)
            || !matches!(
                receipt.child_execution.strip_suffix(&record.key),
                Some("online-call-execution-") | Some("batch-installed-execution-")
            )
            || receipt.reply.as_ref().is_some_and(|reply| {
                reply.schema.is_empty()
                    || reply.schema.len() > 128
                    || reply.bytes.len() > InvocationLimits::default().max_payload_bytes
            })
            || !transfer::valid_receipt_phase(record, &receipt)
        {
            return Err(CobolRetentionValidationError::InconsistentState);
        }
        return Ok(DecodedReceipt::Current(receipt));
    }
    let receipt: LegacyReceipt = serde_json::from_slice(&record.payload)
        .map_err(|_| CobolRetentionValidationError::CorruptPayload)?;
    if receipt.schema_version != 1
        || !valid_digest(&receipt.fingerprint)
        || !valid_identity(&receipt.child_execution)
        || !matches!(
            receipt.child_execution.strip_suffix(&record.key),
            Some("online-call-execution-") | Some("batch-installed-execution-")
        )
        || receipt.reply.as_ref().is_some_and(|reply| {
            reply.schema.is_empty()
                || reply.schema.len() > 128
                || reply.bytes.len() > InvocationLimits::default().max_payload_bytes
        })
        || !matches!(
            (record.version, receipt.reply.is_some()),
            (1, false) | (2, true)
        )
    {
        return Err(CobolRetentionValidationError::InconsistentState);
    }
    Ok(DecodedReceipt::Legacy(receipt))
}

#[allow(dead_code, reason = "R-11 product integration seam")]
pub(super) fn describe_call_replay_row(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    match decode_receipt(record)? {
        DecodedReceipt::Legacy(receipt) => Ok(CobolRetentionRowDescriptor {
            namespace: record.namespace.clone(),
            key: record.key.clone(),
            row_version: record.version,
            kind: CobolRetentionRowKind::CallReplay,
            state: if receipt.reply.is_some() {
                CobolRetentionState::LegacyProtected
            } else {
                CobolRetentionState::Active
            },
            owner_execution: None,
            owner_run_unit: None,
            terminal_tick: None,
            dependencies: vec![CobolRetentionDependency::Execution(receipt.child_execution)],
        }),
        DecodedReceipt::Current(receipt) => {
            let state = if receipt.reply.is_some() {
                CobolRetentionState::Terminal
            } else {
                CobolRetentionState::Active
            };
            let mut dependencies =
                owner_dependencies(&receipt.owner_execution, &receipt.owner_run_unit);
            if let Some(target) = &receipt.target {
                dependencies.extend(target.dependencies(&receipt));
            }
            dependencies.push(CobolRetentionDependency::Execution(receipt.child_execution));
            dependencies.push(provider_dependency(
                CALL_PROTOCOL_NAMESPACE,
                receipt.protocol_key,
            ));
            dependencies.push(provider_dependency(
                super::retention::RUN_STATE_NAMESPACE,
                receipt.run_state_key,
            ));
            Ok(CobolRetentionRowDescriptor {
                namespace: record.namespace.clone(),
                key: record.key.clone(),
                row_version: record.version,
                kind: CobolRetentionRowKind::CallReplay,
                state,
                owner_execution: Some(receipt.owner_execution),
                owner_run_unit: Some(receipt.owner_run_unit),
                terminal_tick: receipt.completion_tick,
                dependencies,
            })
        }
    }
}

fn previous(
    record: ProviderStateRecord,
    expected: &str,
    parent: &Invocation,
) -> Result<BoundedPayload, HostProblem> {
    let reply = match decode_receipt(&record).map_err(|_| HostProblem::UnknownOutcome)? {
        DecodedReceipt::Legacy(receipt) => {
            if receipt.fingerprint != expected {
                return Err(HostProblem::IdempotencyConflict);
            }
            receipt.reply
        }
        DecodedReceipt::Current(receipt) => {
            if receipt.fingerprint != expected
                || receipt.owner_execution != parent.execution_id.as_str()
                || receipt.owner_run_unit != parent.run_unit_id.as_str()
                || receipt.owner_principal != parent.principal.id().as_str()
                || receipt.run_state_key
                    != run_state_key(parent.run_unit_id.as_str(), parent.principal.id().as_str())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            receipt.reply
        }
    };
    let reply = reply.ok_or(HostProblem::UnknownOutcome)?;
    BoundedPayload::new(reply.schema, reply.bytes, InvocationLimits::default())
        .map_err(|_| HostProblem::UnknownOutcome)
}

fn decode_protocol(
    record: &ProviderStateRecord,
) -> Result<DecodedProtocol, CobolRetentionValidationError> {
    validate_row_identity(record, CALL_PROTOCOL_NAMESPACE)?;
    if !valid_digest(&record.key) {
        return Err(CobolRetentionValidationError::InvalidIdentity);
    }
    if record.version == 1 && record.payload == b"installed-call@2" {
        return Ok(DecodedProtocol::Legacy);
    }
    let protocol: CallProtocol = serde_json::from_slice(&record.payload)
        .map_err(|_| CobolRetentionValidationError::CorruptPayload)?;
    if !matches!(protocol.schema_version, 2 | 3)
        || !valid_identity(&protocol.owner_execution)
        || !valid_identity(&protocol.owner_run_unit)
        || !valid_identity(&protocol.owner_principal)
        || protocol.run_state_key
            != run_state_key(&protocol.owner_run_unit, &protocol.owner_principal)
        || protocol.metadata_digest != protocol_metadata_digest(&protocol)
        || record.key != protocol_key(&protocol.owner_run_unit)
        || !matches!(
            (record.version, protocol.ended_tick),
            (1, None) | (2, Some(1..))
        )
    {
        return Err(CobolRetentionValidationError::InconsistentState);
    }
    Ok(DecodedProtocol::Current(protocol))
}

#[allow(dead_code, reason = "R-11 product integration seam")]
pub(super) fn describe_call_protocol_row(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    match decode_protocol(record)? {
        DecodedProtocol::Legacy => Ok(CobolRetentionRowDescriptor {
            namespace: record.namespace.clone(),
            key: record.key.clone(),
            row_version: record.version,
            kind: CobolRetentionRowKind::CallProtocol,
            // The legacy marker has no durable end-state or owner evidence. It cannot be
            // aged safely even after an operator attests an unrelated execution.
            state: CobolRetentionState::Active,
            owner_execution: None,
            owner_run_unit: None,
            terminal_tick: None,
            dependencies: Vec::new(),
        }),
        DecodedProtocol::Current(protocol) => {
            let mut dependencies =
                owner_dependencies(&protocol.owner_execution, &protocol.owner_run_unit);
            dependencies.push(provider_dependency(
                super::retention::RUN_STATE_NAMESPACE,
                protocol.run_state_key,
            ));
            Ok(CobolRetentionRowDescriptor {
                namespace: record.namespace.clone(),
                key: record.key.clone(),
                row_version: record.version,
                kind: CobolRetentionRowKind::CallProtocol,
                state: if protocol.ended_tick.is_some() {
                    CobolRetentionState::Terminal
                } else {
                    CobolRetentionState::Active
                },
                owner_execution: Some(protocol.owner_execution),
                owner_run_unit: Some(protocol.owner_run_unit),
                terminal_tick: protocol.ended_tick,
                dependencies,
            })
        }
    }
}

pub(super) fn protocol_terminal_mutation(
    store: &dyn PlatformStore,
    invocation: &Invocation,
    ended_tick: u64,
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    if ended_tick == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let key = protocol_key(invocation.run_unit_id.as_str());
    let Some(record) = store
        .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
        .map_err(|_| HostProblem::InfrastructureFailure)?
    else {
        return Ok(None);
    };
    let protocol = match decode_protocol(&record).map_err(|_| HostProblem::UnknownOutcome)? {
        DecodedProtocol::Legacy => CallProtocol {
            schema_version: 2,
            owner_execution: protocol_owner_execution(invocation)?,
            owner_run_unit: invocation.run_unit_id.as_str().into(),
            owner_principal: invocation.principal.id().as_str().into(),
            run_state_key: run_state_key(
                invocation.run_unit_id.as_str(),
                invocation.principal.id().as_str(),
            ),
            metadata_digest: String::new(),
            ended_tick: Some(ended_tick),
        },
        DecodedProtocol::Current(mut protocol) => {
            if protocol.owner_execution != protocol_owner_execution(invocation)?
                || protocol.owner_run_unit != invocation.run_unit_id.as_str()
                || protocol.owner_principal != invocation.principal.id().as_str()
                || protocol.run_state_key
                    != run_state_key(
                        invocation.run_unit_id.as_str(),
                        invocation.principal.id().as_str(),
                    )
            {
                return Err(HostProblem::UnknownOutcome);
            }
            if let Some(existing) = protocol.ended_tick {
                return if existing == ended_tick {
                    Ok(None)
                } else {
                    Err(HostProblem::UnknownOutcome)
                };
            }
            protocol.ended_tick = Some(ended_tick);
            protocol
        }
    };
    let mut protocol = protocol;
    protocol.metadata_digest = protocol_metadata_digest(&protocol);
    let version = record
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(Some(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: CALL_PROTOCOL_NAMESPACE.into(),
            key,
            version,
            payload: serde_json::to_vec(&protocol)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: Some(record.version),
    })))
}

pub(super) fn retention_observation_tick(
    program: &CobolProgram,
    invocation: &Invocation,
) -> Result<u64, HostProblem> {
    let tick = program
        .observe_execution_control(invocation)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .now_tick;
    if tick == 0 {
        Err(HostProblem::InfrastructureFailure)
    } else {
        Ok(tick)
    }
}

impl CobolProgram {
    pub(super) fn ensure_call_protocol(&self, parent: &Invocation) -> Result<(), HostProblem> {
        self.ensure_call_protocol_identity(parent, false, true)
    }

    fn ensure_call_protocol_identity(
        &self,
        parent: &Invocation,
        outer_identity: bool,
        allow_create: bool,
    ) -> Result<(), HostProblem> {
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let key = protocol_key(parent.run_unit_id.as_str());
        let expected_owner = protocol_owner_execution(parent)?;
        match store
            .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            Some(record) => match decode_protocol(&record) {
                Ok(DecodedProtocol::Legacy) if !outer_identity => Ok(()),
                Ok(DecodedProtocol::Current(protocol))
                    if (!outer_identity || protocol.schema_version == 3)
                        && protocol.owner_execution == expected_owner
                        && protocol.owner_run_unit == parent.run_unit_id.as_str()
                        && protocol.owner_principal == parent.principal.id().as_str()
                        && protocol.run_state_key
                            == run_state_key(
                                parent.run_unit_id.as_str(),
                                parent.principal.id().as_str(),
                            ) =>
                {
                    if protocol.ended_tick.is_some() {
                        Err(run_ended_problem())
                    } else {
                        Ok(())
                    }
                }
                _ => Err(HostProblem::UnknownOutcome),
            },
            None if !allow_create || parent.attempt != 1 => Err(HostProblem::UnknownOutcome),
            None => {
                // Version-1 calls never persisted ordinary program state. Their
                // active run units cannot be continued by pretending this is the
                // first invocation under the new last-used-state protocol.
                if store
                    .get_provider_state("cobol-call-protocol@1", &key)
                    .map_err(|_| HostProblem::InfrastructureFailure)?
                    .is_some()
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                let record = ProviderStateRecord {
                    namespace: CALL_PROTOCOL_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    // Every freshly admitted run uses V3, including ordinary
                    // COBOL calls preceding a CICS LINK. Existing V2 rows stay
                    // readable but cannot authorize a new identity domain.
                    payload: serde_json::to_vec(&new_call_protocol(parent, true)?)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                };
                match store.put_provider_state(record, None) {
                    Ok(()) => Ok(()),
                    Err(StoreError::Conflict | StoreError::AlreadyExists) => {
                        match store.get_provider_state(CALL_PROTOCOL_NAMESPACE, &key) {
                            Ok(Some(record)) => match decode_protocol(&record) {
                                Ok(DecodedProtocol::Legacy) if !outer_identity => Ok(()),
                                Ok(DecodedProtocol::Current(protocol))
                                    if (!outer_identity || protocol.schema_version == 3)
                                        && protocol.owner_execution == expected_owner
                                        && protocol.owner_run_unit
                                            == parent.run_unit_id.as_str()
                                        && protocol.owner_principal
                                            == parent.principal.id().as_str()
                                        && protocol.run_state_key
                                            == run_state_key(
                                                parent.run_unit_id.as_str(),
                                                parent.principal.id().as_str(),
                                            ) =>
                                {
                                    if protocol.ended_tick.is_some() {
                                        Err(run_ended_problem())
                                    } else {
                                        Ok(())
                                    }
                                }
                                _ => Err(HostProblem::UnknownOutcome),
                            },
                            _ => Err(HostProblem::UnknownOutcome),
                        }
                    }
                    Err(_) => Err(HostProblem::InfrastructureFailure),
                }
            }
        }
    }

    pub(super) fn execute_installed_effect(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
        program: &str,
        payload: &BoundedPayload,
        selection: Option<&mainframe_env_host_api::ProgramLinkSelection>,
    ) -> Result<BoundedPayload, HostProblem> {
        if !matches!(
            payload.schema(),
            "mainframe-env.cobol.call@1" | "mainframe-env.program.input@1"
        ) {
            return Err(HostProblem::Malformed);
        }
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let key = identity(parent, effect)?;
        let fingerprint = fingerprint(parent, effect, program, payload)?;
        let outer_identity = outer_program_identity(effect)?;
        if outer_identity && effect.sequence > u64::from(parent.limits.max_effects) {
            return Err(HostProblem::ResourceExhausted);
        }
        let preflight = || match selection {
            Some(selection) => self.preflight_selected_program(program, selection),
            None => self.preflight_installed_program(
                program,
                payload.schema() == "mainframe-env.program.input@1",
            ),
        };
        if let Some(record) = store
            .get_provider_state(CALL_REPLAY_NAMESPACE, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            if outer_identity {
                self.ensure_call_protocol_identity(parent, true, false)?;
            }
            let result = previous(record, &fingerprint, parent)?;
            preflight()?;
            if payload.schema() == "mainframe-env.program.input@1" {
                self.finish_run_unit(parent)?;
            }
            return Ok(result);
        }
        let admitted = preflight()?;
        if payload.schema() == "mainframe-env.program.input@1" {
            self.validate_typed_parent(parent, effect)?;
        }
        self.ensure_call_protocol_identity(parent, outer_identity, true)?;
        let prefix = if payload.schema() == "mainframe-env.cobol.call@1" {
            "online-call-execution"
        } else {
            "batch-installed-execution"
        };
        let mut receipt = Receipt {
            schema_version: 2,
            replay_key: key.clone(),
            fingerprint,
            child_execution: format!("{prefix}-{key}"),
            owner_execution: parent.execution_id.as_str().into(),
            owner_run_unit: parent.run_unit_id.as_str().into(),
            owner_principal: parent.principal.id().as_str().into(),
            protocol_key: protocol_key(parent.run_unit_id.as_str()),
            run_state_key: run_state_key(
                parent.run_unit_id.as_str(),
                parent.principal.id().as_str(),
            ),
            metadata_digest: String::new(),
            completion_tick: None,
            reply: None,
            transfer: None,
            target: None,
        };
        receipt.metadata_digest = receipt_metadata_digest(&receipt);
        let pending = ProviderStateRecord {
            namespace: CALL_REPLAY_NAMESPACE.into(),
            key: key.clone(),
            version: 1,
            payload: serde_json::to_vec(&receipt)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        };
        if let Err(problem) = store.put_provider_state(pending.clone(), None) {
            if matches!(problem, StoreError::Conflict | StoreError::AlreadyExists) {
                return match store.get_provider_state(CALL_REPLAY_NAMESPACE, &key) {
                    Ok(Some(record)) => {
                        let result = previous(record, &receipt.fingerprint, parent)?;
                        if payload.schema() == "mainframe-env.program.input@1" {
                            self.finish_run_unit(parent)?;
                        }
                        Ok(result)
                    }
                    _ => Err(HostProblem::UnknownOutcome),
                };
            }
            return Err(HostProblem::InfrastructureFailure);
        }
        // No call can dispatch without winning the durable reservation.
        let original_call = WinningInstalledCall {
            parent,
            effect,
            store,
            reservation: &pending,
            child_execution: &receipt.child_execution,
        };
        let mut writes = Vec::new();
        let result = if payload.schema() == "mainframe-env.cobol.call@1" {
            self.execute_admitted(
                parent,
                program,
                admitted,
                payload,
                &key,
                &mut writes,
                Some(&original_call),
            )
        } else {
            self.execute_installed_batch_from_call(
                parent,
                program,
                admitted,
                payload,
                &key,
                Some(&original_call),
            )
            .and_then(|output| {
                serde_json::to_vec(&output).map_err(|_| HostProblem::ProviderFailure)
            })
            .and_then(|bytes| {
                BoundedPayload::new(
                    "mainframe-env.program.output@1",
                    bytes,
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)
            })
        }?;
        receipt.reply = Some(Reply {
            schema: result.schema().into(),
            bytes: result.bytes().to_vec(),
        });
        receipt.completion_tick = Some(retention_observation_tick(self, parent)?);
        receipt.metadata_digest = receipt_metadata_digest(&receipt);
        let payload = serde_json::to_vec(&receipt).map_err(|_| HostProblem::UnknownOutcome)?;
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: CALL_REPLAY_NAMESPACE.into(),
                key,
                version: 2,
                payload,
            },
            expected_version: Some(1),
        });
        store
            .put_provider_states_atomic(writes)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if result.schema() == "mainframe-env.program.output@1" {
            self.finish_run_unit(parent)?;
        }
        Ok(result)
    }
}

fn run_ended_problem() -> HostProblem {
    HostProblem::Condition {
        name: "COBOL-RUN-ENDED".into(),
        response: -9,
        response2: 0,
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, call_payload, parent};
    use mainframe_env_host_api::ProgramName;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::ProviderStateStore;

    fn effect(parent: &Invocation, key: &str) -> EffectRequest {
        EffectRequest {
            run_unit: parent.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: parent.deadline_tick,
            idempotency_key: Some(IdempotencyKey::new(key, InvocationLimits::default()).unwrap()),
            request: HostRequest::Program(ProgramRequest::Link {
                program: ProgramName::new("LEAF", 128).unwrap(),
                payload: call_payload(&[]),
                selection: None,
            }),
        }
    }

    #[test]
    fn outer_program_identity_legacy_protocols_fence_new_keys_and_keep_old_replies() {
        for sqlite in [false, true] {
            for generation in 0..3 {
                for completed in [false, true] {
                    let root = TestRoot::new();
                    let url = format!("sqlite://{}?mode=rwc", root.0.join("state.db").display());
                    let store: Arc<dyn PlatformStore> = if sqlite {
                        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
                    } else {
                        Arc::new(MemoryStore::new(Default::default()))
                    };
                    let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
                    fixture.install(
                        "LEAF",
                        "IDENTIFICATION DIVISION. PROGRAM-ID. LEAF. PROCEDURE DIVISION. GOBACK.",
                    );
                    let actor = parent();
                    let old = effect(&actor, "cics:parent-run:17");
                    let payload = call_payload(&[]);
                    let reply = encode_cobol_call_result(&[]).unwrap();
                    let record = ProviderStateRecord {
                        namespace: CALL_REPLAY_NAMESPACE.into(), key: identity(&actor, &old).unwrap(), version: if completed { 2 } else { 1 },
                        payload: serde_json::to_vec(&serde_json::json!({"schema_version":1, "fingerprint":fingerprint(&actor, &old, "LEAF", &payload).unwrap(), "child_execution":format!("online-call-execution-{}", identity(&actor, &old).unwrap()), "reply": if completed { Some(Reply { schema: reply.schema().into(), bytes: reply.bytes().to_vec() }) } else { None }})).unwrap(),
                    };
                    let mut initial = record.clone();
                    initial.version = 1;
                    store.put_provider_state(initial, None).unwrap();
                    if completed {
                        store.put_provider_state(record.clone(), Some(1)).unwrap();
                    }
                    let protocol = ProviderStateRecord {
                        namespace: if generation == 0 {
                            "cobol-call-protocol@1".into()
                        } else {
                            CALL_PROTOCOL_NAMESPACE.into()
                        },
                        key: protocol_key(actor.run_unit_id.as_str()),
                        version: 1,
                        payload: match generation {
                            0 => b"installed-call@1".to_vec(),
                            1 => b"installed-call@2".to_vec(),
                            _ => serde_json::to_vec(&new_call_protocol(&actor, false).unwrap())
                                .unwrap(),
                        },
                    };
                    store.put_provider_state(protocol.clone(), None).unwrap();
                    drop(fixture);
                    drop(store);
                    let store: Arc<dyn PlatformStore> = if sqlite {
                        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
                    } else {
                        // Memory has no reopen credit; use a fresh deterministic copy.
                        let copy = Arc::new(MemoryStore::new(Default::default()));
                        let mut initial = record.clone();
                        initial.version = 1;
                        copy.put_provider_state(initial, None).unwrap();
                        if completed {
                            copy.put_provider_state(record.clone(), Some(1)).unwrap();
                        }
                        copy.put_provider_state(protocol.clone(), None).unwrap();
                        copy
                    };
                    let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
                    if !sqlite {
                        fixture.install("LEAF", "IDENTIFICATION DIVISION. PROGRAM-ID. LEAF. PROCEDURE DIVISION. GOBACK.");
                    }
                    let new = effect(&actor, &format!("cics-program-v2:{}", "a".repeat(64)));
                    assert_eq!(
                        fixture
                            .router
                            .cobol
                            .execute_installed_effect(&actor, &new, "LEAF", &payload, None),
                        Err(HostProblem::UnknownOutcome)
                    );
                    assert_eq!(
                        store.list_provider_state(CALL_REPLAY_NAMESPACE, 8).unwrap(),
                        vec![record]
                    );
                    assert_eq!(
                        store
                            .get_provider_state(&protocol.namespace, &protocol.key)
                            .unwrap(),
                        Some(protocol)
                    );
                    assert_eq!(
                        fixture
                            .router
                            .cobol
                            .execute_installed_effect(&actor, &old, "LEAF", &payload, None),
                        if completed {
                            Ok(reply)
                        } else {
                            Err(HostProblem::UnknownOutcome)
                        },
                        "sqlite={sqlite} generation={generation} completed={completed}"
                    );
                }
            }
        }
    }

    #[test]
    fn outer_program_identity_completed_replay_conflict_and_missing_protocol_after_sqlite_reopen() {
        let root = TestRoot::new();
        let url = format!(
            "sqlite://{}?mode=rwc",
            root.0.join("completed.db").display()
        );
        let actor = parent();
        let request = effect(&actor, &format!("cics-program-v2:{}", "b".repeat(64)));
        let payload = call_payload(&[]);
        let expected;
        let calls;
        {
            let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
            fixture.install(
                "LEAF",
                "IDENTIFICATION DIVISION. PROGRAM-ID. LEAF. PROCEDURE DIVISION. GOBACK.",
            );
            // Ordinary admission before LINK must not create a counter-era run.
            fixture.router.cobol.ensure_call_protocol(&actor).unwrap();
            expected = fixture
                .router
                .cobol
                .execute_installed_effect(&actor, &request, "LEAF", &payload, None)
                .unwrap();
            assert_eq!(
                fixture
                    .router
                    .cobol
                    .execute_installed_effect(&actor, &request, "LEAF", &payload, None),
                Ok(expected.clone())
            );
            calls = store.list_provider_state(CALL_REPLAY_NAMESPACE, 8).unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].version, 2);
        }
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
        assert_eq!(
            fixture
                .router
                .cobol
                .execute_installed_effect(&actor, &request, "LEAF", &payload, None),
            Ok(expected)
        );
        let changed = call_payload(&[b"changed".to_vec()]);
        let mut conflict = request.clone();
        let HostRequest::Program(ProgramRequest::Link { payload, .. }) = &mut conflict.request
        else {
            unreachable!()
        };
        *payload = changed.clone();
        assert_eq!(
            fixture
                .router
                .cobol
                .execute_installed_effect(&actor, &conflict, "LEAF", &changed, None),
            Err(HostProblem::IdempotencyConflict)
        );
        let key = protocol_key(actor.run_unit_id.as_str());
        let protocol = store
            .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(CALL_PROTOCOL_NAMESPACE, &key, protocol.version)
            .unwrap();
        assert_eq!(
            fixture.router.cobol.execute_installed_effect(
                &actor,
                &request,
                "LEAF",
                &call_payload(&[]),
                None
            ),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(
            store
                .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.list_provider_state(CALL_REPLAY_NAMESPACE, 8).unwrap(),
            calls
        );
    }

    #[test]
    fn outer_program_identity_v3_digest_and_terminal_state_are_strict() {
        let root = TestRoot::new();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
        let actor = parent();
        assert_eq!(
            fixture
                .router
                .cobol
                .ensure_call_protocol_identity(&actor, true, false),
            Err(HostProblem::UnknownOutcome)
        );
        fixture
            .router
            .cobol
            .ensure_call_protocol_identity(&actor, true, true)
            .unwrap();
        let key = protocol_key(actor.run_unit_id.as_str());
        let row = store
            .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let protocol: CallProtocol = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(protocol.schema_version, 3);
        assert!(decode_protocol(&row).is_ok());
        let mut old = protocol.clone();
        old.schema_version = 2;
        assert_ne!(protocol_metadata_digest(&old), protocol.metadata_digest);
        for case in 0..5 {
            let mut bad = protocol.clone();
            match case {
                0 => bad.schema_version = 2,
                1 => bad.schema_version = 4,
                2 => bad.owner_execution = "foreign".into(),
                3 => bad.ended_tick = Some(0),
                _ => bad.ended_tick = Some(100),
            }
            let mut record = row.clone();
            record.payload = serde_json::to_vec(&bad).unwrap();
            assert!(decode_protocol(&record).is_err(), "case {case}");
        }
        let mutation = protocol_terminal_mutation(store.as_ref(), &actor, 100)
            .unwrap()
            .unwrap();
        let ProviderStateMutation::Put(write) = mutation else {
            panic!("expected protocol put")
        };
        store
            .put_provider_state(write.record, write.expected_version)
            .unwrap();
        let terminal = store
            .get_provider_state(CALL_PROTOCOL_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let descriptor = describe_call_protocol_row(&terminal).unwrap();
        assert_eq!(descriptor.state, CobolRetentionState::Terminal);
        assert_eq!(descriptor.terminal_tick, Some(100));
        assert_eq!(
            fixture
                .router
                .cobol
                .ensure_call_protocol_identity(&actor, true, false),
            Err(run_ended_problem())
        );
    }

    #[test]
    fn outer_program_identity_rejects_malformed_or_wrong_request_domains() {
        let actor = parent();
        let valid = effect(&actor, &format!("cics-program-v2:{}", "a".repeat(64)));
        assert_eq!(outer_program_identity(&valid), Ok(true));
        assert_eq!(
            outer_program_identity(&effect(&actor, "cics:parent-run:1")),
            Ok(false)
        );
        assert_eq!(
            outer_program_identity(&effect(
                &actor,
                &format!("cics-program-v3:{}", "a".repeat(64))
            )),
            Err(HostProblem::Unsupported)
        );
        for case in 0..4 {
            let mut bad = valid.clone();
            match case {
                0 => bad.sequence = 0,
                1 => {
                    bad.idempotency_key = Some(
                        IdempotencyKey::new("cics-program-v2:short", InvocationLimits::default())
                            .unwrap(),
                    )
                }
                2 => {
                    bad.idempotency_key = Some(
                        IdempotencyKey::new(
                            format!("cics-program-v2:{}", "A".repeat(64)),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    )
                }
                _ => {
                    bad.request = HostRequest::Program(ProgramRequest::Cancel {
                        programs: vec![ProgramName::new("LEAF", 128).unwrap()],
                    })
                }
            }
            assert_eq!(
                outer_program_identity(&bad),
                Err(HostProblem::Malformed),
                "case {case}"
            );
        }
        let root = TestRoot::new();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
        let mut oversized = valid;
        oversized.sequence = u64::from(actor.limits.max_effects) + 1;
        assert_eq!(
            fixture.router.cobol.execute_installed_effect(
                &actor,
                &oversized,
                "LEAF",
                &call_payload(&[]),
                None
            ),
            Err(HostProblem::ResourceExhausted)
        );
        assert!(
            store
                .list_provider_state(CALL_REPLAY_NAMESPACE, 8)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .list_provider_state(CALL_PROTOCOL_NAMESPACE, 8)
                .unwrap()
                .is_empty()
        );
    }
}
