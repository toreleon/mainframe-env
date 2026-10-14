//! Compiled constructor expectations are independent of retained digest claims.
#[cfg(test)]
mod cases {
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
            Self::with_floats(false)
        }

        fn with_floats(floating: bool) -> Self {
            let root = TestRoot::new();
            let fixture = Fixture::new(
                &root,
                Arc::new(MemoryStore::new(Default::default())),
                HostProblem::NotFound,
                false,
            );
            let data = if floating {
                "WORKING-STORAGE SECTION. 01 F COMP-1. 01 D COMP-2."
            } else {
                ""
            };
            fixture.install("EXIT", &format!("IDENTIFICATION DIVISION. PROGRAM-ID. EXIT. DATA DIVISION. {data} LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION USING DFHCOMMAREA. MOVE 'GOOD' TO DFHCOMMAREA. GOBACK."));
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
    fn float_checkpoint_target_constructor_accepts_schema13_and_legacy12() {
        for floating in [false, true] {
            let proof = Proof::with_floats(floating);
            let schema = if floating {
                "mainframe-env.reference-machine-checkpoint@13"
            } else {
                "mainframe-env.reference-machine-checkpoint@12"
            };
            assert_eq!(
                proof.receipt.target.as_ref().unwrap().checkpoint_schema,
                schema
            );
            assert!(decode_receipt(&proof.row()).is_ok());
            assert_eq!(
                proof.attest().unwrap().1.checkpoint().unwrap().schema(),
                schema
            );
        }
    }

    #[test]
    fn float_checkpoint_target_rejects_rehashed_schema_header_mismatch() {
        for floating in [false, true] {
            let mut proof = Proof::with_floats(floating);
            proof.receipt.target.as_mut().unwrap().checkpoint_schema = if floating {
                "mainframe-env.reference-machine-checkpoint@12"
            } else {
                "mainframe-env.reference-machine-checkpoint@13"
            }
            .into();
            proof.receipt.metadata_digest = receipt_metadata_digest(&proof.receipt);
            assert!(decode_receipt(&proof.row()).is_err());
            assert!(matches!(proof.attest(), Err(HostProblem::UnknownOutcome)));
        }
    }

    fn legacy_ieee_executable(
        executable: &mainframe_env_compiler_api::ValidatedArtifact,
    ) -> mainframe_env_compiler_api::ValidatedArtifact {
        use mainframe_env_ir::{IrLimits, ModuleBuilder, decode_binary, encode_binary};
        let module = decode_binary(executable.payload(), CodecLimits::default()).unwrap();
        let mut builder = ModuleBuilder::new(IrLimits::default());
        for storage in module.storage() {
            builder
                .add_storage(&storage.name, storage.size, storage.alias_of.clone())
                .unwrap();
        }
        for region in module.regions() {
            let region_id = builder.add_region().unwrap();
            for block in &region.blocks {
                let block_id = builder.add_block(region_id).unwrap();
                for operation in &block.operations {
                    let mut attributes = operation.attributes.clone();
                    // Historical artifacts have no profile attribute, hence IEEE storage.
                    attributes.remove("float_profile");
                    builder
                        .add_operation(
                            block_id,
                            operation.identity.clone(),
                            operation.operands.clone(),
                            operation.results.len(),
                            attributes,
                            operation.effects.clone(),
                            operation.storage.clone(),
                            operation.location.clone(),
                        )
                        .unwrap();
                }
            }
        }
        let binary = encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap();
        mainframe_env_compiler_api::ValidatedArtifact::read(
            mainframe_env_compiler_api::VersionedArtifactManifest::V3(
                executable.manifest().clone(),
            ),
            &binary,
            &mainframe_env_compiler::core_mir_catalog(),
            &mainframe_env_compiler::core_mir_profile(),
            CodecLimits::default(),
            mainframe_env_compiler_api::ArtifactLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn float_checkpoint_legacy12_floating_target_attests_constructor() {
        let mut proof = Proof::with_floats(true);
        proof.executable = legacy_ieee_executable(&proof.executable);
        proof.selection.artifact = ArtifactRef::new(
            proof.executable.content_id().to_reference(),
            InvocationLimits::default(),
        )
        .unwrap();
        let next = target_invocation(
            &proof.source,
            &proof.receipt,
            &proof.selection,
            &proof.observed,
        )
        .unwrap();
        let saved = StagedInvocation::capture(&next).unwrap();
        let machine =
            ReferenceMachine::from_binary(proof.executable.payload(), next, CodecLimits::default())
                .unwrap();
        let checkpoint = machine.checkpoint().unwrap();
        let profiles = machine.snapshot().float_profiles;
        assert!(
            profiles
                .values()
                .all(|profile| profile == mainframe_env_ir::COBOL_FLOAT_IEEE)
        );
        let trailer = 4 + profiles
            .keys()
            .map(|name| 8 + name.len() + 1)
            .sum::<usize>();
        let mut bytes = checkpoint.bytes().to_vec();
        bytes.truncate(bytes.len() - trailer);
        bytes[..8].copy_from_slice(b"MECP0012");
        bytes[8..12].copy_from_slice(&12u32.to_be_bytes());
        let target = proof.receipt.target.as_mut().unwrap();
        target.artifact = proof.selection.artifact.as_str().into();
        target.context_digest = saved.digest().unwrap();
        target.invocation = saved;
        target.checkpoint_schema = "mainframe-env.reference-machine-checkpoint@12".into();
        target.checkpoint = STANDARD.encode(&bytes);
        target.checkpoint_digest = format!("{:x}", Sha256::digest(&bytes));
        proof.receipt.metadata_digest = receipt_metadata_digest(&proof.receipt);
        assert!(decode_receipt(&proof.row()).is_ok());
        let (_, constructor) = proof.attest().unwrap();
        assert_eq!(constructor.checkpoint().unwrap(), checkpoint);
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
