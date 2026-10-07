//! Pre-instance-admission target checkpoint in the original pending CALL.
use super::super::staged_invocation::StagedInvocation;
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use mainframe_env_cics::CicsService;
use mainframe_env_execution_api::{Machine, Transfer};

mod admission;

const MAX_STAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetStage {
    selector: String,
    artifact: String,
    generation: u64,
    content_identity: String,
    invocation: StagedInvocation,
    context_digest: String,
    checkpoint_schema: String,
    checkpoint: String,
    checkpoint_digest: String,
}

fn target_invocation(
    source: &Invocation,
    receipt: &Receipt,
    selection: &mainframe_env_host_api::ProgramLinkSelection,
    observed: &Transfer,
) -> Result<Invocation, HostProblem> {
    let name = observed.selector.as_str();
    let sequence = target_identity(receipt, selection.generation, &selection.content_identity);
    let limits = InvocationLimits::default();
    let mut next = source.clone();
    next.request_id = RequestId::new(format!("online-transfer-request-{sequence}"), limits)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    next.execution_id = ExecutionId::new(format!("online-transfer-execution-{sequence}"), limits)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    next.parent_execution_id = Some(source.execution_id.clone());
    next.selector = Selector::new(format!("program:{name}"), limits)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    next.artifact = selection.artifact.clone();
    next.trace_id = TraceId::new(format!("online-transfer-trace-{sequence}"), limits)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    next.idempotency_key =
        IdempotencyKey::new(format!("online-transfer-effect-{sequence}"), limits)
            .map_err(|_| HostProblem::UnknownOutcome)?;
    next.bindings.insert(
        "cics.commarea".into(),
        BoundedPayload::new(
            "mainframe-env.cics.commarea@1",
            observed.payload.bytes().to_vec(),
            limits,
        )
        .map_err(|_| HostProblem::UnknownOutcome)?,
    );
    next.bindings.insert(
        "cobol.call.arguments".into(),
        super::super::selected_link::encode_selected_call(&[observed.payload.bytes().to_vec()])
            .map_err(|_| HostProblem::UnknownOutcome)?,
    );
    Ok(next)
}

fn target_identity(receipt: &Receipt, generation: u64, content_identity: &str) -> String {
    digest(&[
        b"installed-transfer-target@1",
        receipt.replay_key.as_bytes(),
        receipt.child_execution.as_bytes(),
        &generation.to_be_bytes(),
        content_identity.as_bytes(),
    ])
}

impl TargetStage {
    pub(super) fn dependencies(&self, receipt: &Receipt) -> Vec<CobolRetentionDependency> {
        vec![
            provider_dependency(
                "cics-program-definition-v1",
                format!(
                    "{}:{:020}",
                    self.selector.trim_start_matches("program:"),
                    self.generation
                ),
            ),
            provider_dependency(
                "cics-effect-replay-v1",
                format!(
                    "online-call-effect-{}:{}",
                    receipt.replay_key,
                    receipt
                        .transfer
                        .as_ref()
                        .expect("validated target has intent")
                        .checkpoint_sequence()
                ),
            ),
        ]
    }
    pub(super) fn metadata_digest(&self) -> String {
        digest(&[
            b"installed-target-stage@1",
            self.selector.as_bytes(),
            self.artifact.as_bytes(),
            &self.generation.to_be_bytes(),
            self.content_identity.as_bytes(),
            self.context_digest.as_bytes(),
            self.checkpoint_schema.as_bytes(),
            self.checkpoint_digest.as_bytes(),
        ])
    }

    pub(super) fn valid(&self, receipt: &Receipt) -> bool {
        let (execution, parent, run, principal, selector, artifact) = self.invocation.identity();
        if self.generation == 0
            || !receipt
                .child_execution
                .starts_with("online-call-execution-")
            || !self
                .content_identity
                .strip_prefix("sha256:")
                .is_some_and(valid_digest)
            || !self
                .artifact
                .strip_prefix("sha256:")
                .is_some_and(valid_digest)
            || !valid_digest(&self.context_digest)
            || !valid_digest(&self.checkpoint_digest)
            || self.checkpoint_schema != "mainframe-env.reference-machine-checkpoint@12"
            || self.checkpoint.len() > MAX_STAGE_BYTES
            || self.invocation.validate_syntax().is_err()
            || self.invocation.digest().ok().as_deref() != Some(&self.context_digest)
            || execution
                != format!(
                    "online-transfer-execution-{}",
                    target_identity(receipt, self.generation, &self.content_identity)
                )
            || parent != receipt.child_execution
            || run != receipt.owner_run_unit
            || principal != receipt.owner_principal
            || selector != self.selector
            || artifact != self.artifact
            || receipt
                .transfer
                .as_ref()
                .is_none_or(|intent| self.selector != format!("program:{}", intent.selector()))
        {
            return false;
        }
        let Ok(checkpoint) = STANDARD.decode(&self.checkpoint) else {
            return false;
        };
        STANDARD.encode(&checkpoint) == self.checkpoint
            && checkpoint.starts_with(b"MECP0012")
            && self.checkpoint_digest == format!("{:x}", Sha256::digest(checkpoint))
    }
}

impl DefaultProgramRouter {
    pub(crate) fn bind_product_runtime(
        &self,
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
        artifacts: Arc<dyn ArtifactStore>,
        cics: &Arc<CicsService>,
    ) -> Result<(), HostProblem> {
        self.bind_runtime(host, store, artifacts)?;
        self.cobol
            .transfer_owner
            .set(Arc::downgrade(cics))
            .map_err(|_| HostProblem::IdempotencyConflict)
    }
}

impl CobolProgram {
    pub(in super::super) fn stage_transfer_target(
        &self,
        source: &Invocation,
        identity: &str,
        machine: &ReferenceMachine,
        observed: &Transfer,
    ) -> Result<(), HostProblem> {
        let store = self.store.get().ok_or(HostProblem::UnknownOutcome)?;
        let cics = self
            .transfer_owner
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or(HostProblem::UnknownOutcome)?;
        let row = store
            .get_provider_state(CALL_REPLAY_NAMESPACE, identity)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let DecodedReceipt::Current(mut receipt) =
            decode_receipt(&row).map_err(|_| HostProblem::UnknownOutcome)?
        else {
            return Err(HostProblem::UnknownOutcome);
        };
        if receipt.schema_version != 3
            || row.version != 2
            || receipt.child_execution != source.execution_id.as_str()
            || receipt.owner_run_unit != source.run_unit_id.as_str()
            || receipt.owner_principal != source.principal.id().as_str()
            || receipt
                .transfer
                .as_ref()
                .is_none_or(|intent| !intent.matches(source, machine, observed))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let execution = store
            .get_execution(&source.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if execution.state != mainframe_env_store_api::ExecutionState::Suspended
            || execution.version
                != receipt
                    .transfer
                    .as_ref()
                    .ok_or(HostProblem::UnknownOutcome)?
                    .source_version()
            || execution.artifact != source.artifact
            || execution.selector != source.selector
            || execution.run_unit_id != source.run_unit_id
            || execution.principal != *source.principal.id()
            || execution.attempt != source.attempt
            || execution.terminal_tick.is_some()
            || source.parent_execution_id.as_ref().map(ExecutionId::as_str)
                != Some(receipt.owner_execution.as_str())
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let key = IdempotencyKey::new(
            format!("{}:{}", source.idempotency_key, machine.effect_sequence()),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        let effect = store
            .effect(&key)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let selection = cics.attested_program_transfer_selection(source, &effect, observed)?;
        let name = observed.selector.as_str();
        let admitted = self
            .preflight_selected_program(name, &selection)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let next = target_invocation(source, &receipt, &selection, observed)?;
        let invocation = StagedInvocation::capture(&next)?;
        let target_machine = ReferenceMachine::from_binary(
            admitted.executable.payload(),
            next,
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        let checkpoint = target_machine
            .checkpoint()
            .ok_or(HostProblem::UnknownOutcome)?;
        let target = TargetStage {
            selector: format!("program:{name}"),
            artifact: selection.artifact.as_str().into(),
            generation: selection.generation,
            content_identity: selection.content_identity,
            context_digest: invocation.digest()?,
            invocation,
            checkpoint_schema: checkpoint.schema().into(),
            checkpoint: STANDARD.encode(checkpoint.bytes()),
            checkpoint_digest: format!("{:x}", Sha256::digest(checkpoint.bytes())),
        };
        receipt.schema_version = 4;
        receipt.target = Some(target);
        receipt.metadata_digest = receipt_metadata_digest(&receipt);
        let staged = ProviderStateRecord {
            namespace: row.namespace,
            key: row.key,
            version: 3,
            payload: serde_json::to_vec(&receipt).map_err(|_| HostProblem::UnknownOutcome)?,
        };
        if staged.payload.len() > MAX_STAGE_BYTES {
            return Err(HostProblem::UnknownOutcome);
        }
        decode_receipt(&staged).map_err(|_| HostProblem::UnknownOutcome)?;
        // Read-only revalidation precedes the CALL CAS. It grants no frame or
        // instance admission and cannot turn this pending row into a reply.
        self.attest_staged_transfer(&staged, source, machine, observed)?;
        store
            .put_provider_state(staged, Some(2))
            .map_err(|_| HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobol::hardening::parent;
    use serde_json::{Value, json};

    fn row() -> ProviderStateRecord {
        let caller = parent();
        let key = "a".repeat(64);
        let mut receipt = Receipt {
            schema_version: 4, replay_key: key.clone(), fingerprint: "b".repeat(64),
            child_execution: format!("online-call-execution-{key}"),
            owner_execution: caller.execution_id.as_str().into(),
            owner_run_unit: caller.run_unit_id.as_str().into(),
            owner_principal: caller.principal.id().as_str().into(),
            protocol_key: protocol_key(caller.run_unit_id.as_str()),
            run_state_key: run_state_key(caller.run_unit_id.as_str(), caller.principal.id().as_str()),
            metadata_digest: String::new(), completion_tick: None, reply: None,
            transfer: Some(serde_json::from_value(json!({
                "selector":"EXIT", "schema":"mainframe-env.cics.payload@1", "bytes":[],
                "source_selector":"program:MID", "source_artifact":format!("sha256:{}", "1".repeat(64)),
                "source_attempt":1, "source_version":6, "checkpoint_digest":"2".repeat(64),
                "checkpoint_sequence":3, "checkpoint_machine_schema":1,
                "checkpoint_schema":"mainframe-env.reference-machine-checkpoint@12"
            })).unwrap()), target: None,
        };
        let identity = format!("sha256:{}", "3".repeat(64));
        let mut invocation = caller;
        invocation.parent_execution_id =
            Some(ExecutionId::new(&receipt.child_execution, InvocationLimits::default()).unwrap());
        invocation.execution_id = ExecutionId::new(
            format!(
                "online-transfer-execution-{}",
                target_identity(&receipt, 1, &identity)
            ),
            InvocationLimits::default(),
        )
        .unwrap();
        invocation.selector = Selector::new("program:EXIT", InvocationLimits::default()).unwrap();
        invocation.artifact = ArtifactRef::new(
            format!("sha256:{}", "4".repeat(64)),
            InvocationLimits::default(),
        )
        .unwrap();
        let saved = StagedInvocation::capture(&invocation).unwrap();
        // A syntax-only retention fixture, deliberately not an executable image.
        // The product route independently checks a real constructor checkpoint.
        let checkpoint = b"MECP0012syntax-only";
        receipt.target = Some(TargetStage {
            selector: "program:EXIT".into(),
            artifact: invocation.artifact.as_str().into(),
            generation: 1,
            content_identity: identity,
            context_digest: saved.digest().unwrap(),
            invocation: saved,
            checkpoint_schema: "mainframe-env.reference-machine-checkpoint@12".into(),
            checkpoint: STANDARD.encode(checkpoint),
            checkpoint_digest: format!("{:x}", Sha256::digest(checkpoint)),
        });
        receipt.metadata_digest = receipt_metadata_digest(&receipt);
        ProviderStateRecord {
            namespace: CALL_REPLAY_NAMESPACE.into(),
            key,
            version: 3,
            payload: serde_json::to_vec(&receipt).unwrap(),
        }
    }

    #[test]
    fn transfer_target_identity_and_metadata_have_independent_frozen_vectors() {
        let mut receipt: Receipt = serde_json::from_slice(&row().payload).unwrap();
        assert_eq!(
            target_identity(&receipt, 1, &format!("sha256:{}", "3".repeat(64))),
            "bd640697bcb979888ef57f2f27c64e1759fba15c0a9f70e138ea82c2e786d209"
        );
        let target = receipt.target.as_mut().unwrap();
        target.context_digest = "5".repeat(64);
        target.checkpoint_digest = "6".repeat(64);
        assert_eq!(
            target.metadata_digest(),
            "062e4ba2e0d4fd60e79387080a4f031f3ec758e869c984ff61594a45b751e0c3"
        );
    }

    #[test]
    fn transfer_target_reader_preserves_active_retention_and_rejects_rehashed_invalid_phases() {
        let good = row();
        assert!(decode_receipt(&good).is_ok());
        assert_eq!(
            describe_call_replay_row(&good).unwrap().state,
            CobolRetentionState::Active
        );
        for case in 0..13 {
            let mut receipt: Receipt = serde_json::from_slice(&good.payload).unwrap();
            let mut version = 3;
            match case {
                0 => version = 2,
                1 => receipt.schema_version = 3,
                2 => receipt.target = None,
                3 => receipt.transfer = None,
                4 => receipt.completion_tick = Some(9),
                5 => {
                    receipt.reply = Some(Reply {
                        schema: "reply@1".into(),
                        bytes: Vec::new(),
                    })
                }
                6 => receipt.target.as_mut().unwrap().generation = 0,
                7 => receipt.target.as_mut().unwrap().selector = "program:OTHER".into(),
                8 => receipt.target.as_mut().unwrap().artifact = "artifact".into(),
                9 => receipt.target.as_mut().unwrap().checkpoint.push('\n'),
                10 => {
                    receipt.target.as_mut().unwrap().checkpoint_schema =
                        "mainframe-env.reference-machine-checkpoint@11".into()
                }
                11 => receipt.target.as_mut().unwrap().context_digest = "0".repeat(64),
                _ => receipt.target.as_mut().unwrap().checkpoint_digest = "0".repeat(64),
            }
            receipt.metadata_digest = receipt_metadata_digest(&receipt);
            assert!(
                decode_receipt(&ProviderStateRecord {
                    version,
                    payload: serde_json::to_vec(&receipt).unwrap(),
                    ..good.clone()
                })
                .is_err(),
                "case {case}"
            );
        }
        for path in ["target", "invocation"] {
            let mut value: Value = serde_json::from_slice(&good.payload).unwrap();
            if path == "target" {
                value["target"]["extra"] = json!(true);
            } else {
                value["target"]["invocation"]["extra"] = json!(true);
            }
            assert!(
                decode_receipt(&ProviderStateRecord {
                    payload: serde_json::to_vec(&value).unwrap(),
                    ..good.clone()
                })
                .is_err()
            );
        }
    }
}
