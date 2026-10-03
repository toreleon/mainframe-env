//! Observed installed Transfer intent in its original CALL reservation.
//! This is not target admission, handoff completion or permission to redispatch.
use super::*;
use mainframe_env_execution_api::{LifecycleEvent, LifecycleEventKind, Machine, Transfer};
use mainframe_env_store_api::{
    CheckpointRecord, ExecutionRecord, ExecutionState, ProviderStateStore,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TransferIntent {
    selector: String,
    schema: String,
    bytes: Vec<u8>,
    source_selector: String,
    source_artifact: String,
    source_attempt: u32,
    source_version: u64,
    checkpoint_digest: String,
    checkpoint_sequence: u64,
    checkpoint_machine_schema: u32,
    checkpoint_schema: String,
}

fn valid_program_selector(selector: &str) -> bool {
    selector
        .strip_prefix("program:")
        .is_some_and(|name| !name.is_empty() && name.len() <= 128 && valid_identity(name))
        && valid_identity(selector)
}

pub(in super::super) fn preserve_control_cursor_failure(
    outcome: &ExecutionOutcome,
    problem: HostProblem,
) -> HostProblem {
    if matches!(
        outcome,
        ExecutionOutcome::Transfer(_)
            | ExecutionOutcome::Invoke(_)
            | ExecutionOutcome::Suspended(_)
    ) {
        HostProblem::UnknownOutcome
    } else {
        problem
    }
}

impl TransferIntent {
    pub(in super::super) fn selector(&self) -> &str {
        &self.selector
    }
    pub(in super::super) fn source_version(&self) -> u64 {
        self.source_version
    }
    pub(super) fn checkpoint_sequence(&self) -> u64 {
        self.checkpoint_sequence
    }

    pub(in super::super) fn matches(
        &self,
        source: &Invocation,
        machine: &ReferenceMachine,
        observed: &Transfer,
    ) -> bool {
        self.selector == observed.selector.as_str()
            && self.schema == observed.payload.schema()
            && self.bytes == observed.payload.bytes()
            && self.source_selector == source.selector.as_str()
            && self.source_artifact == source.artifact.as_str()
            && self.source_attempt == source.attempt
            && self.checkpoint_sequence == machine.effect_sequence()
            && machine.checkpoint().is_some_and(|checkpoint| {
                self.checkpoint_digest == format!("{:x}", Sha256::digest(checkpoint.bytes()))
            })
    }
    fn valid(&self) -> bool {
        valid_identity(&self.selector)
            && !self.selector.contains(':')
            && valid_program_selector(&self.source_selector)
            && self.schema == "mainframe-env.cics.payload@1"
            && self.bytes.len() <= 32_763
            && self
                .source_artifact
                .strip_prefix("sha256:")
                .is_some_and(valid_digest)
            && self.source_attempt > 0
            && (1..=i64::MAX as u64).contains(&self.source_version)
            && valid_digest(&self.checkpoint_digest)
            && self.checkpoint_sequence > 0
            && self.checkpoint_machine_schema == 1
            && self.checkpoint_schema == "mainframe-env.reference-machine-checkpoint@12"
    }

    pub(super) fn metadata_digest(&self) -> String {
        digest(&[
            b"installed-transfer-intent@1",
            self.selector.as_bytes(),
            self.schema.as_bytes(),
            &self.bytes,
            self.source_selector.as_bytes(),
            self.source_artifact.as_bytes(),
            &self.source_attempt.to_be_bytes(),
            &self.source_version.to_be_bytes(),
            self.checkpoint_digest.as_bytes(),
            &self.checkpoint_sequence.to_be_bytes(),
            &self.checkpoint_machine_schema.to_be_bytes(),
            self.checkpoint_schema.as_bytes(),
        ])
    }
}

pub(super) fn valid_receipt_phase(record: &ProviderStateRecord, receipt: &Receipt) -> bool {
    if let Some(target) = &receipt.target {
        return receipt.schema_version == 4
            && record.version == 3
            && receipt.reply.is_none()
            && receipt.completion_tick.is_none()
            && receipt
                .transfer
                .as_ref()
                .is_some_and(|intent| intent.valid())
            && target.valid(receipt);
    }
    match (receipt.schema_version, &receipt.transfer) {
        (2, None) => matches!(
            (
                record.version,
                receipt.reply.is_some(),
                receipt.completion_tick
            ),
            (1, false, None) | (2, true, Some(1..))
        ),
        (3, Some(intent)) => {
            record.version == 2
                && receipt.reply.is_none()
                && receipt.completion_tick.is_none()
                && intent.valid()
                && receipt
                    .child_execution
                    .starts_with("online-call-execution-")
        }
        _ => false,
    }
}

/// Capture only an observed same-level transfer after the durable coordinator
/// has suspended this exact leased child. Failures remain unknown. The source
/// checkpoint/instance are deliberately untouched; later advancement must
/// revalidate retained core proof and acquire the CICS-owned target selection.
pub(in super::super) fn persist_transfer_intent(
    store: &dyn PlatformStore,
    invocation: &Invocation,
    identity: &str,
    machine: &ReferenceMachine,
    observed: &Transfer,
) -> Result<(), HostProblem> {
    if !observed.replace_frame {
        return Err(HostProblem::UnknownOutcome);
    }
    let row = store
        .get_provider_state(CALL_REPLAY_NAMESPACE, identity)
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    let execution = store
        .get_execution(&invocation.execution_id)
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    let checkpoint = store
        .get_checkpoint(&invocation.execution_id)
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    let events = store
        .events(&invocation.execution_id, execution.version, 1)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let event = events.first().ok_or(HostProblem::UnknownOutcome)?;
    let staged = prepare_transfer_receipt(
        row,
        invocation,
        &execution,
        &checkpoint,
        event,
        machine,
        observed,
    )?;
    publish_transfer_intent(store, staged)
}

fn publish_transfer_intent(
    store: &dyn ProviderStateStore,
    staged: ProviderStateRecord,
) -> Result<(), HostProblem> {
    store
        .put_provider_state(staged, Some(1))
        .map_err(|_| HostProblem::UnknownOutcome)
}

#[allow(clippy::too_many_arguments)]
fn prepare_transfer_receipt(
    row: ProviderStateRecord,
    invocation: &Invocation,
    execution: &ExecutionRecord,
    checkpoint: &CheckpointRecord,
    event: &LifecycleEvent,
    machine: &ReferenceMachine,
    observed: &Transfer,
) -> Result<ProviderStateRecord, HostProblem> {
    if !observed.replace_frame {
        return Err(HostProblem::UnknownOutcome);
    }
    let DecodedReceipt::Current(mut receipt) =
        decode_receipt(&row).map_err(|_| HostProblem::UnknownOutcome)?
    else {
        return Err(HostProblem::UnknownOutcome);
    };
    if receipt.schema_version != 2
        || row.version != 1
        || receipt.child_execution != invocation.execution_id.as_str()
        || Some(receipt.owner_execution.as_str())
            != invocation
                .parent_execution_id
                .as_ref()
                .map(ExecutionId::as_str)
        || receipt.owner_run_unit != invocation.run_unit_id.as_str()
        || receipt.owner_principal != invocation.principal.id().as_str()
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let current = machine.checkpoint().ok_or(HostProblem::UnknownOutcome)?;
    let intent = TransferIntent {
        selector: observed.selector.as_str().into(),
        schema: observed.payload.schema().into(),
        bytes: observed.payload.bytes().to_vec(),
        source_selector: invocation.selector.as_str().into(),
        source_artifact: invocation.artifact.as_str().into(),
        source_attempt: invocation.attempt,
        source_version: execution.version,
        checkpoint_digest: format!("{:x}", Sha256::digest(&checkpoint.payload)),
        checkpoint_sequence: checkpoint.effect_sequence,
        checkpoint_machine_schema: checkpoint.machine_schema_version,
        checkpoint_schema: current.schema().into(),
    };
    if !intent.valid()
        || execution.execution_id != invocation.execution_id
        || execution.state != ExecutionState::Suspended
        || execution.run_unit_id != invocation.run_unit_id
        || execution.principal != *invocation.principal.id()
        || execution.artifact != invocation.artifact
        || execution.selector != invocation.selector
        || execution.attempt != invocation.attempt
        || execution.terminal_tick.is_some()
        || event.kind != LifecycleEventKind::Suspended
        || event.execution_id != invocation.execution_id
        || event.run_unit_id != invocation.run_unit_id
        || event.sequence != execution.version
        || event.attempt != invocation.attempt
        || event.tick == 0
        || checkpoint.execution_id != invocation.execution_id
        || checkpoint.run_unit_id != invocation.run_unit_id
        || checkpoint.principal != *invocation.principal.id()
        || checkpoint.artifact != invocation.artifact
        || checkpoint.schema_version != 1
        || checkpoint.provider_generation != mainframe_env_interpreter::INTERPRETER_GENERATION
        || checkpoint.required_host_interfaces
            != BTreeMap::from([
                ("mainframe-env.execution-api".into(), "1".into()),
                ("mainframe-env.host-api".into(), "1".into()),
            ])
        || checkpoint.payload_size != checkpoint.payload.len() as u64
        || checkpoint.payload_digest != <[u8; 32]>::from(Sha256::digest(&checkpoint.payload))
        || checkpoint.payload != current.bytes()
        || checkpoint.effect_sequence != machine.effect_sequence()
        || checkpoint.effect_sequence > invocation.limits.max_effects
    {
        return Err(HostProblem::UnknownOutcome);
    }
    receipt.schema_version = 3;
    receipt.transfer = Some(intent);
    receipt.metadata_digest = receipt_metadata_digest(&receipt);
    let staged = ProviderStateRecord {
        namespace: row.namespace,
        key: row.key,
        version: 2,
        payload: serde_json::to_vec(&receipt).map_err(|_| HostProblem::UnknownOutcome)?,
    };
    // Exact CAS protects the CALL owner. This does not atomically advance core
    // state; a future consumer must independently recheck the source tuple.
    decode_receipt(&staged).map_err(|_| HostProblem::UnknownOutcome)?;
    Ok(staged)
}

#[cfg(test)]
mod tests;
