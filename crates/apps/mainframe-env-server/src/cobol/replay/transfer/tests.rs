//! Negative protocol tests; compiled backend route proofs live in product tests.
#[cfg(test)]
mod protocol {
    use super::super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, parent};
    use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};
    use mainframe_env_host_api::{CicsDisposition, CicsResponse};
    use mainframe_env_store::{MemoryStore, StoreLimits};
    use serde_json::{Value, json};

    struct Proof {
        _root: TestRoot,
        invocation: Invocation,
        machine: ReferenceMachine,
        row: ProviderStateRecord,
        execution: ExecutionRecord,
        checkpoint: CheckpointRecord,
        event: LifecycleEvent,
        observed: Transfer,
    }

    #[test]
    fn transfer_intent_secondary_cursor_failure_cannot_turn_pending_control_into_known_return() {
        let proof = Proof::new();
        let pending = [
            ExecutionOutcome::Transfer(proof.observed.clone()),
            ExecutionOutcome::Invoke(mainframe_env_execution_api::ChildInvocation {
                selector: proof.invocation.selector.clone(),
                artifact: proof.invocation.artifact.clone(),
                payload: proof.observed.payload.clone(),
            }),
            ExecutionOutcome::Suspended(mainframe_env_execution_api::Suspension {
                kind: "cics".into(),
                resume_token: "yield".into(),
                state_bytes: 0,
            }),
        ];
        for outcome in pending {
            for problem in [
                HostProblem::InfrastructureFailure,
                HostProblem::ResourceExhausted,
            ] {
                assert_eq!(
                    preserve_control_cursor_failure(&outcome, problem),
                    HostProblem::UnknownOutcome
                );
            }
        }
        assert_eq!(
            preserve_control_cursor_failure(
                &ExecutionOutcome::Cancelled,
                HostProblem::InfrastructureFailure
            ),
            HostProblem::InfrastructureFailure
        );
    }

    #[test]
    fn transfer_intent_metadata_has_independent_frozen_framed_vectors() {
        let mut receipt: Receipt = serde_json::from_slice(&Proof::new().row.payload).unwrap();
        receipt.schema_version = 3;
        receipt.transfer = Some(TransferIntent {
            selector: "EXIT".into(),
            schema: "mainframe-env.cics.payload@1".into(),
            bytes: b"ROOT".to_vec(),
            source_selector: "program:MID".into(),
            source_artifact: format!("sha256:{}", "1".repeat(64)),
            source_attempt: 1,
            source_version: 6,
            checkpoint_digest: "2".repeat(64),
            checkpoint_sequence: 3,
            checkpoint_machine_schema: 1,
            checkpoint_schema: "mainframe-env.reference-machine-checkpoint@12".into(),
        });
        assert_eq!(
            receipt.transfer.as_ref().unwrap().metadata_digest(),
            "51ca9c2093c9a9969f178ca9fe4db1a7cb287023852561dbdfa4eaa12be6830e"
        );
        assert_eq!(
            receipt_metadata_digest(&receipt),
            "aa80e2907e39995fe0dc6459a8299539c56220a7772d3e65824bd9c5a67af7cd"
        );
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
            fixture.install("MID", "IDENTIFICATION DIVISION. PROGRAM-ID. MID. PROCEDURE DIVISION. EXEC CICS XCTL PROGRAM('EXIT') END-EXEC. GOBACK.");
            let admitted = fixture
                .router
                .cobol
                .preflight_installed_program("MID", false)
                .unwrap();
            let caller = parent();
            let key = "a".repeat(64);
            let mut invocation = caller.clone();
            invocation.parent_execution_id = Some(caller.execution_id.clone());
            invocation.execution_id = ExecutionId::new(
                format!("online-call-execution-{key}"),
                InvocationLimits::default(),
            )
            .unwrap();
            invocation.selector =
                Selector::new("program:MID", InvocationLimits::default()).unwrap();
            invocation.artifact = admitted.artifact;
            let mut machine = ReferenceMachine::from_binary(
                admitted.executable.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            let quantum = Quantum::new(10_000, 16 * 1024 * 1024).unwrap();
            let MachineDrive::HostCall(effect) = machine.drive(MachineResume::Start, quantum)
            else {
                panic!("expected compiled XCTL");
            };
            let MachineDrive::Transfer(observed) = machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Cics(CicsResponse {
                        disposition: CicsDisposition::Transfer,
                        condition: "NORMAL".into(),
                        response: 0,
                        response2: 0,
                        applid: String::new(),
                        sysid: String::new(),
                        transaction: String::new(),
                        aid: 0,
                        target: Some("EXIT".into()),
                        next_transaction: None,
                        payload: BoundedPayload::new(
                            "mainframe-env.cics.payload@1",
                            Vec::new(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                        outputs: BTreeMap::new(),
                        unit_of_work: None,
                    })),
                }),
                quantum,
            ) else {
                panic!("expected observed Transfer");
            };
            let payload = machine.checkpoint().unwrap();
            let execution = ExecutionRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                selector: invocation.selector.clone(),
                artifact: invocation.artifact.clone(),
                principal: invocation.principal.id().clone(),
                state: ExecutionState::Suspended,
                attempt: invocation.attempt,
                version: 6,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: None,
            };
            let event = LifecycleEvent {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                sequence: 6,
                attempt: invocation.attempt,
                tick: 9,
                kind: LifecycleEventKind::Suspended,
            };
            let checkpoint = CheckpointRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                session_id: None,
                schema_version: 1,
                machine_schema_version: 1,
                artifact: invocation.artifact.clone(),
                provider_generation: mainframe_env_interpreter::INTERPRETER_GENERATION.into(),
                required_host_interfaces: BTreeMap::from([
                    ("mainframe-env.execution-api".into(), "1".into()),
                    ("mainframe-env.host-api".into(), "1".into()),
                ]),
                effect_sequence: machine.effect_sequence(),
                transaction: None,
                principal: invocation.principal.id().clone(),
                security_classification: "application-data".into(),
                encryption_key_reference: None,
                payload_size: payload.bytes().len() as u64,
                payload_digest: Sha256::digest(payload.bytes()).into(),
                payload: payload.bytes().to_vec(),
            };
            let mut receipt = Receipt {
                schema_version: 2,
                replay_key: key.clone(),
                fingerprint: "b".repeat(64),
                child_execution: invocation.execution_id.as_str().into(),
                owner_execution: caller.execution_id.as_str().into(),
                owner_run_unit: caller.run_unit_id.as_str().into(),
                owner_principal: caller.principal.id().as_str().into(),
                protocol_key: protocol_key(caller.run_unit_id.as_str()),
                run_state_key: run_state_key(
                    caller.run_unit_id.as_str(),
                    caller.principal.id().as_str(),
                ),
                metadata_digest: String::new(),
                completion_tick: None,
                reply: None,
                transfer: None,
                target: None,
            };
            receipt.metadata_digest = receipt_metadata_digest(&receipt);
            let row = ProviderStateRecord {
                namespace: CALL_REPLAY_NAMESPACE.into(),
                key,
                version: 1,
                payload: serde_json::to_vec(&receipt).unwrap(),
            };
            Self {
                _root: root,
                invocation,
                machine,
                row,
                execution,
                checkpoint,
                event,
                observed,
            }
        }

        fn prepare(&self) -> Result<ProviderStateRecord, HostProblem> {
            prepare_transfer_receipt(
                self.row.clone(),
                &self.invocation,
                &self.execution,
                &self.checkpoint,
                &self.event,
                &self.machine,
                &self.observed,
            )
        }
    }

    #[test]
    fn transfer_intent_revalidation_rejects_newer_source_with_matching_new_event() {
        let mut proof = Proof::new();
        let staged = proof.prepare().unwrap();
        let DecodedReceipt::Current(receipt) = decode_receipt(&staged).unwrap() else {
            panic!("current intent required");
        };
        let intent = receipt.transfer.unwrap();
        assert!(
            intent
                .attest_source(
                    &proof.invocation,
                    &proof.execution,
                    &proof.checkpoint,
                    &proof.event,
                    &proof.machine,
                    &proof.observed
                )
                .is_ok()
        );
        proof.execution.version += 1;
        proof.event.sequence = proof.execution.version;
        assert_eq!(
            intent.attest_source(
                &proof.invocation,
                &proof.execution,
                &proof.checkpoint,
                &proof.event,
                &proof.machine,
                &proof.observed
            ),
            Err(HostProblem::UnknownOutcome)
        );
        proof.execution.version -= 1;
        proof.event.sequence = proof.execution.version;
        proof.observed.replace_frame = false;
        assert_eq!(
            intent.attest_source(
                &proof.invocation,
                &proof.execution,
                &proof.checkpoint,
                &proof.event,
                &proof.machine,
                &proof.observed
            ),
            Err(HostProblem::UnknownOutcome)
        );
    }

    #[test]
    fn transfer_intent_reader_preserves_legacy_and_active_retention() {
        let proof = Proof::new();
        assert!(decode_receipt(&proof.row).is_ok());
        let staged = proof.prepare().unwrap();
        let descriptor = describe_call_replay_row(&staged).unwrap();
        assert_eq!(descriptor.state, CobolRetentionState::Active);
        assert!(descriptor.terminal_tick.is_none());
        assert!(
            descriptor
                .dependencies
                .contains(&CobolRetentionDependency::Execution(
                    proof.invocation.execution_id.as_str().into()
                ))
        );
        assert!(
            descriptor
                .dependencies
                .contains(&CobolRetentionDependency::Execution(
                    parent().execution_id.as_str().into()
                ))
        );
        assert_eq!(
            previous(staged, &"b".repeat(64), &parent()),
            Err(HostProblem::UnknownOutcome)
        );
        let legacy = ProviderStateRecord { payload: serde_json::to_vec(&json!({"schema_version":1,"fingerprint":"b".repeat(64),"child_execution":proof.invocation.execution_id.as_str(),"reply":null})).unwrap(), ..proof.row.clone() };
        assert!(decode_receipt(&legacy).is_ok());
        let mut completed: Receipt = serde_json::from_slice(&proof.row.payload).unwrap();
        completed.reply = Some(Reply {
            schema: "mainframe-env.cobol.call-result@1".into(),
            bytes: vec![0, 0, 0, 0],
        });
        completed.completion_tick = Some(7);
        completed.metadata_digest = receipt_metadata_digest(&completed);
        assert!(
            decode_receipt(&ProviderStateRecord {
                version: 2,
                payload: serde_json::to_vec(&completed).unwrap(),
                ..proof.row.clone()
            })
            .is_ok()
        );
    }

    #[test]
    fn transfer_intent_reader_rejects_relabels_tampering_bounds_and_terminal_claims() {
        let staged = Proof::new().prepare().unwrap();
        for (field, bad) in [
            ("selector", json!("OTHER")),
            ("schema", json!("wrong@1")),
            ("bytes", json!([1])),
            ("source_selector", json!("program:OTHER")),
            (
                "source_artifact",
                json!("sha256:".to_string() + &"c".repeat(64)),
            ),
            ("source_attempt", json!(2)),
            ("source_version", json!(7)),
            ("checkpoint_digest", json!("c".repeat(64))),
            ("checkpoint_sequence", json!(2)),
            ("checkpoint_machine_schema", json!(2)),
            ("checkpoint_schema", json!("wrong@1")),
        ] {
            let mut value: Value = serde_json::from_slice(&staged.payload).unwrap();
            value["transfer"][field] = bad;
            assert!(
                decode_receipt(&ProviderStateRecord {
                    payload: serde_json::to_vec(&value).unwrap(),
                    ..staged.clone()
                })
                .is_err(),
                "{field}"
            );
        }
        for case in 0..12 {
            let mut receipt: Receipt = serde_json::from_slice(&staged.payload).unwrap();
            let mut version = 2;
            match case {
                0 => receipt.schema_version = 2,
                1 => receipt.schema_version = 4,
                2 => receipt.transfer = None,
                3 => version = 1,
                4 => version = 3,
                5 => receipt.completion_tick = Some(9),
                6 => {
                    receipt.reply = Some(Reply {
                        schema: "reply@1".into(),
                        bytes: Vec::new(),
                    })
                }
                7 => receipt.transfer.as_mut().unwrap().bytes = vec![0; 32_764],
                8 => receipt.transfer.as_mut().unwrap().source_version = 0,
                9 => receipt.transfer.as_mut().unwrap().source_attempt = 0,
                10 => receipt.transfer.as_mut().unwrap().selector = String::new(),
                _ => receipt.transfer.as_mut().unwrap().checkpoint_digest = "bad".into(),
            }
            receipt.metadata_digest = receipt_metadata_digest(&receipt);
            assert!(
                decode_receipt(&ProviderStateRecord {
                    version,
                    payload: serde_json::to_vec(&receipt).unwrap(),
                    ..staged.clone()
                })
                .is_err(),
                "rehashed case {case}"
            );
        }
        for extra in [",\"unknown\":0", ",\"schema_version\":3"] {
            let mut row = staged.clone();
            row.payload.pop();
            row.payload.extend_from_slice(extra.as_bytes());
            row.payload.push(b'}');
            assert!(decode_receipt(&row).is_err());
        }
    }

    #[test]
    fn transfer_intent_preparation_rejects_foreign_or_inconsistent_source_proof() {
        let mut proof = Proof::new();
        assert!(proof.prepare().is_ok());
        let checkpoint = proof.checkpoint.clone();
        for case in 0..11 {
            proof.checkpoint = checkpoint.clone();
            match case {
                0 => proof.checkpoint.payload_digest[0] ^= 1,
                1 => proof.checkpoint.payload_size += 1,
                2 => proof.checkpoint.payload[0] ^= 1,
                3 => proof.checkpoint.effect_sequence += 1,
                4 => proof.checkpoint.machine_schema_version = 2,
                5 => proof.checkpoint.execution_id = parent().execution_id,
                6 => proof.checkpoint.schema_version = 2,
                7 => proof.checkpoint.artifact = parent().artifact,
                8 => proof.checkpoint.provider_generation = "wrong-generation".into(),
                9 => proof.checkpoint.required_host_interfaces.clear(),
                _ => {
                    proof.checkpoint.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
            }
            assert_eq!(
                proof.prepare(),
                Err(HostProblem::UnknownOutcome),
                "checkpoint {case}"
            );
        }
        proof.checkpoint = checkpoint;
        let execution = proof.execution.clone();
        for case in 0..9 {
            proof.execution = execution.clone();
            match case {
                0 => proof.execution.state = ExecutionState::Completed,
                1 => proof.execution.terminal_tick = Some(9),
                2 => proof.execution.attempt += 1,
                3 => proof.execution.version += 1,
                4 => proof.execution.execution_id = parent().execution_id,
                5 => proof.execution.artifact = parent().artifact,
                6 => proof.execution.selector = parent().selector,
                7 => {
                    proof.execution.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
                _ => {
                    proof.execution.principal = mainframe_env_execution_api::PrincipalId::new(
                        "FOREIGN",
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
            }
            assert_eq!(
                proof.prepare(),
                Err(HostProblem::UnknownOutcome),
                "execution {case}"
            );
        }
        proof.execution = execution;
        let event = proof.event.clone();
        for case in 0..5 {
            proof.event = event.clone();
            match case {
                0 => proof.event.kind = LifecycleEventKind::HandoffCompleted,
                1 => proof.event.tick = 0,
                2 => proof.event.sequence += 1,
                3 => proof.event.attempt += 1,
                _ => proof.event.execution_id = parent().execution_id,
            }
            assert_eq!(
                proof.prepare(),
                Err(HostProblem::UnknownOutcome),
                "event {case}"
            );
        }
        proof.event = event;
        let observed = proof.observed.clone();
        for case in 0..3 {
            proof.observed = observed.clone();
            match case {
                0 => proof.observed.replace_frame = false,
                1 => {
                    proof.observed.payload =
                        BoundedPayload::new("wrong@1", Vec::new(), InvocationLimits::default())
                            .unwrap()
                }
                _ => {
                    proof.observed.payload = BoundedPayload::new(
                        "mainframe-env.cics.payload@1",
                        vec![0; 32_764],
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
            }
            assert_eq!(
                proof.prepare(),
                Err(HostProblem::UnknownOutcome),
                "observed {case}"
            );
        }
        proof.observed = observed;
        proof.invocation.parent_execution_id = None;
        assert_eq!(proof.prepare(), Err(HostProblem::UnknownOutcome));
    }

    #[test]
    fn transfer_intent_publication_is_cas_fenced_and_capacity_failure_stays_unknown() {
        let proof = Proof::new();
        let staged = proof.prepare().unwrap();
        let store = MemoryStore::new(StoreLimits {
            max_blob_bytes: proof.row.payload.len(),
            ..Default::default()
        });
        store.put_provider_state(proof.row.clone(), None).unwrap();
        assert!(staged.payload.len() > proof.row.payload.len());
        assert_eq!(
            publish_transfer_intent(&store, staged.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            store
                .get_provider_state(CALL_REPLAY_NAMESPACE, &proof.row.key)
                .unwrap(),
            Some(proof.row.clone())
        );
        let store = MemoryStore::new(Default::default());
        store.put_provider_state(proof.row.clone(), None).unwrap();
        publish_transfer_intent(&store, staged.clone()).unwrap();
        assert_eq!(
            publish_transfer_intent(&store, staged.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            store
                .get_provider_state(CALL_REPLAY_NAMESPACE, &proof.row.key)
                .unwrap(),
            Some(staged)
        );
    }
}
