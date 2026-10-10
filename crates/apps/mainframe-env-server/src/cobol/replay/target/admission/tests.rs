//! Compiled constructor expectations are independent of retained digest claims.
#[cfg(test)]
mod controls {
    use super::super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, parent};
    use mainframe_env_execution_api::CancellationProbe;
    use mainframe_env_host_api::ProgramLinkSelection;
    use mainframe_env_store::MemoryStore;
    use serde_json::json;

    struct Proof {
        _root: TestRoot,
        fixture: Fixture,
        source: Invocation,
        receipt: Receipt,
        observed: Transfer,
        selection: ProgramLinkSelection,
        executable: mainframe_env_compiler_api::ValidatedArtifact,
    }

    impl Proof {
        fn new() -> Self {
            let root = TestRoot::new();
            let fixture = Fixture::new(
                &root,
                Arc::new(MemoryStore::new(Default::default())),
                HostProblem::NotFound,
                false,
            );
            fixture.install("EXIT", "IDENTIFICATION DIVISION. PROGRAM-ID. EXIT. DATA DIVISION. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION USING DFHCOMMAREA. MOVE 'GOOD' TO DFHCOMMAREA. GOBACK.");
            let admitted = fixture
                .router
                .cobol
                .preflight_installed_program("EXIT", false)
                .unwrap();
            let caller = parent();
            let key = "a".repeat(64);
            let mut source = caller.clone();
            source.execution_id = ExecutionId::new(
                format!("online-call-execution-{key}"),
                InvocationLimits::default(),
            )
            .unwrap();
            source.parent_execution_id = Some(caller.execution_id.clone());
            source.selector = Selector::new("program:MID", InvocationLimits::default()).unwrap();
            source.idempotency_key = IdempotencyKey::new(
                format!("online-call-effect-{key}"),
                InvocationLimits::default(),
            )
            .unwrap();
            source.deadline_tick = 100;
            source.cancellation_probe = Some(CancellationProbe::default());
            let observed = Transfer {
                selector: Selector::new("EXIT", InvocationLimits::default()).unwrap(),
                payload: BoundedPayload::new(
                    "mainframe-env.cics.payload@1",
                    b"ROOT".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
                replace_frame: true,
            };
            let selection = ProgramLinkSelection {
                generation: 1,
                artifact: admitted.artifact,
                content_identity: format!("sha256:{}", "3".repeat(64)),
            };
            let mut receipt = Receipt {
                schema_version: 4, replay_key: key.clone(), fingerprint: "b".repeat(64),
                child_execution: source.execution_id.as_str().into(),
                owner_execution: caller.execution_id.as_str().into(),
                owner_run_unit: caller.run_unit_id.as_str().into(),
                owner_principal: caller.principal.id().as_str().into(),
                protocol_key: protocol_key(caller.run_unit_id.as_str()),
                run_state_key: run_state_key(caller.run_unit_id.as_str(), caller.principal.id().as_str()),
                metadata_digest: String::new(), completion_tick: None, reply: None,
                transfer: Some(serde_json::from_value(json!({
                    "selector":"EXIT", "schema":"mainframe-env.cics.payload@1", "bytes":b"ROOT".to_vec(),
                    "source_selector":"program:MID", "source_artifact":format!("sha256:{}", "1".repeat(64)),
                    "source_attempt":1, "source_version":6, "checkpoint_digest":"2".repeat(64),
                    "checkpoint_sequence":3, "checkpoint_machine_schema":1,
                    "checkpoint_schema":"mainframe-env.reference-machine-checkpoint@12"
                })).unwrap()), target: None,
            };
            let next = target_invocation(&source, &receipt, &selection, &observed).unwrap();
            let saved = StagedInvocation::capture(&next).unwrap();
            let machine = ReferenceMachine::from_binary(
                admitted.executable.payload(),
                next,
                CodecLimits::default(),
            )
            .unwrap();
            let checkpoint = machine.checkpoint().unwrap();
            receipt.target = Some(TargetStage {
                selector: "program:EXIT".into(),
                artifact: selection.artifact.as_str().into(),
                generation: selection.generation,
                content_identity: selection.content_identity.clone(),
                context_digest: saved.digest().unwrap(),
                invocation: saved,
                checkpoint_schema: checkpoint.schema().into(),
                checkpoint: STANDARD.encode(checkpoint.bytes()),
                checkpoint_digest: format!("{:x}", Sha256::digest(checkpoint.bytes())),
            });
            receipt.metadata_digest = receipt_metadata_digest(&receipt);
            Self {
                _root: root,
                fixture,
                source,
                receipt,
                observed,
                selection,
                executable: admitted.executable,
            }
        }

        fn attest(&self) -> Result<(Invocation, ReferenceMachine), HostProblem> {
            self.receipt.target.as_ref().unwrap().attest_constructor(
                &self.receipt,
                &self.source,
                &self.observed,
                &self.selection,
                &self.executable,
            )
        }

        fn row(&self) -> ProviderStateRecord {
            ProviderStateRecord {
                namespace: CALL_REPLAY_NAMESPACE.into(),
                key: self.receipt.replay_key.clone(),
                version: 3,
                payload: serde_json::to_vec(&self.receipt).unwrap(),
            }
        }
    }

    #[test]
    fn admission_constructor_preserves_commarea_and_rejects_rehashed_executed_image() {
        let mut proof = Proof::new();
        let (invocation, machine) = proof.attest().unwrap();
        assert_eq!(invocation.bindings["cics.commarea"].bytes(), b"ROOT");
        assert_eq!(invocation.principal, proof.source.principal);
        assert_eq!(
            invocation.cancellation_probe,
            proof.source.cancellation_probe
        );
        assert_eq!(machine.effect_sequence(), 0);
        assert_eq!(machine.variable("DFHCOMMAREA").unwrap().bytes(), &[0; 4]);
        let mut executed = ReferenceMachine::from_binary(
            proof.executable.payload(),
            invocation,
            CodecLimits::default(),
        )
        .unwrap();
        let action = executed.drive(
            mainframe_env_execution_api::MachineResume::Start,
            mainframe_env_execution_api::Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
        );
        assert!(matches!(
            action,
            mainframe_env_execution_api::MachineDrive::Completed(_)
        ));
        let image = executed.checkpoint().unwrap();
        let target = proof.receipt.target.as_mut().unwrap();
        target.checkpoint = STANDARD.encode(image.bytes());
        target.checkpoint_digest = format!("{:x}", Sha256::digest(image.bytes()));
        proof.receipt.metadata_digest = receipt_metadata_digest(&proof.receipt);
        assert!(
            decode_receipt(&proof.row()).is_ok(),
            "syntax-only retention still protects this image"
        );
        assert!(matches!(proof.attest(), Err(HostProblem::UnknownOutcome)));
    }

    #[test]
    fn admission_constructor_rejects_rehashed_context_and_foreign_generation_without_mutation() {
        let mut proof = Proof::new();
        let original = proof.receipt.clone();
        for (field, value) in [
            ("deadline", json!(proof.source.deadline_tick + 1)),
            ("priority", json!(proof.source.priority + 1)),
            ("attempt", json!(2)),
            ("audit", json!("foreign-audit")),
            ("request", json!("foreign-request")),
            ("trace", json!("foreign-trace")),
            ("key", json!("foreign-key")),
            ("grants", json!([])),
            (
                "generations",
                json!({"host.program.invoke":"foreign-generation"}),
            ),
        ] {
            proof.receipt = original.clone();
            let target = proof.receipt.target.as_mut().unwrap();
            let mut saved = serde_json::to_value(&target.invocation).unwrap();
            saved[field] = value;
            target.invocation = serde_json::from_value(saved).unwrap();
            target.context_digest = target.invocation.digest().unwrap();
            proof.receipt.metadata_digest = receipt_metadata_digest(&proof.receipt);
            assert!(decode_receipt(&proof.row()).is_ok(), "syntax {field}");
            assert!(
                matches!(proof.attest(), Err(HostProblem::UnknownOutcome)),
                "{field}"
            );
        }
        proof.receipt = original;
        let row = proof.row();
        proof
            .fixture
            .store
            .put_provider_state(
                {
                    let mut pending = proof.receipt.clone();
                    pending.schema_version = 2;
                    pending.transfer = None;
                    pending.target = None;
                    pending.metadata_digest = receipt_metadata_digest(&pending);
                    ProviderStateRecord {
                        version: 1,
                        payload: serde_json::to_vec(&pending).unwrap(),
                        ..row.clone()
                    }
                },
                None,
            )
            .unwrap();
        proof
            .fixture
            .store
            .put_provider_state(
                {
                    let mut intent = proof.receipt.clone();
                    intent.schema_version = 3;
                    intent.target = None;
                    intent.metadata_digest = receipt_metadata_digest(&intent);
                    ProviderStateRecord {
                        version: 2,
                        payload: serde_json::to_vec(&intent).unwrap(),
                        ..row.clone()
                    }
                },
                Some(1),
            )
            .unwrap();
        proof
            .fixture
            .store
            .put_provider_state(row.clone(), Some(2))
            .unwrap();
        for case in 0..3 {
            let mut selection = proof.selection.clone();
            match case {
                0 => selection.generation += 1,
                1 => selection.content_identity = format!("sha256:{}", "4".repeat(64)),
                _ => selection.artifact = proof.source.artifact.clone(),
            }
            assert!(matches!(
                proof.receipt.target.as_ref().unwrap().attest_constructor(
                    &proof.receipt,
                    &proof.source,
                    &proof.observed,
                    &selection,
                    &proof.executable
                ),
                Err(HostProblem::UnknownOutcome)
            ));
            assert_eq!(
                proof
                    .fixture
                    .store
                    .get_provider_state(CALL_REPLAY_NAMESPACE, &row.key)
                    .unwrap(),
                Some(row.clone())
            );
        }
    }

    #[test]
    fn admission_live_controls_reject_deadline_and_cancellation_as_unknown() {
        let proof = Proof::new();
        let live = ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        };
        assert!(attest_controls(&proof.source, live).is_ok());
        for control in [
            ExecutionControl {
                now_tick: 0,
                ..live
            },
            ExecutionControl {
                now_tick: proof.source.deadline_tick,
                ..live
            },
            ExecutionControl {
                cancellation_requested: true,
                ..live
            },
        ] {
            assert_eq!(
                attest_controls(&proof.source, control),
                Err(HostProblem::UnknownOutcome)
            );
        }
        proof.source.cancellation_probe.as_ref().unwrap().request();
        assert_eq!(
            attest_controls(&proof.source, live),
            Err(HostProblem::UnknownOutcome)
        );
        // The integrity/context binding is unchanged by requesting the live probe.
        assert!(proof.attest().is_ok());
    }
}
