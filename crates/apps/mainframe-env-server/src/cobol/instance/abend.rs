//! Known-ABEND instance disposition, never a cached successful CALL reply.
use super::*;
use mainframe_env_execution_api::LifecycleEventKind;
use mainframe_env_store_api::ExecutionState;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Exact bounded durable terminal identity, not a reusable program image.
pub(super) struct AbendProof {
    /// Child execution whose terminal event must still prove Abend.
    pub(super) execution: String,
    /// Stable root owner admitted before the child borrowed its program frame.
    pub(super) owner_execution: String,
    version: u64,
    attempt: u32,
    metadata_digest: String,
}

fn metadata_digest(
    record: &ProviderStateRecord,
    instance: &Instance,
    proof: &AbendProof,
) -> String {
    super::super::replay::digest(&[
        b"known-abend-instance@2",
        record.namespace.as_bytes(),
        record.key.as_bytes(),
        instance.artifact.as_bytes(),
        proof.owner_execution.as_bytes(),
        proof.execution.as_bytes(),
        &proof.version.to_be_bytes(),
        &proof.attempt.to_be_bytes(),
        &[u8::from(instance.open_files)],
    ])
}

/// Validate the supported instance generation and its abandoned-frame binding.
pub(super) fn valid_instance(record: &ProviderStateRecord, instance: &Instance) -> bool {
    match (&instance.abend, instance.schema_version) {
        (None, 1) => true,
        (Some(proof), 2) => {
            !instance.busy
                && record.version > 1
                && instance.state.is_none()
                && !instance.artifact.is_empty()
                && valid_identity(&proof.execution)
                && valid_identity(&proof.owner_execution)
                && proof.version > 0
                && proof.version <= i64::MAX as u64
                && proof.attempt > 0
                && proof.metadata_digest == metadata_digest(record, instance, proof)
        }
        _ => false,
    }
}

fn verify_execution(
    store: &dyn PlatformStore,
    invocation: &Invocation,
    program: &str,
    artifact: &str,
    proof: &AbendProof,
) -> Result<(), HostProblem> {
    let id = ExecutionId::new(&proof.execution, InvocationLimits::default())
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let execution = store
        .get_execution(&id)
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    if execution.execution_id != id
        || execution.state != ExecutionState::Failed
        || execution.version != proof.version
        || execution.attempt != proof.attempt
        || execution.run_unit_id != invocation.run_unit_id
        || execution.principal != *invocation.principal.id()
        || execution.artifact.as_str() != artifact
        || execution.selector.as_str() != format!("program:{program}")
        || execution.terminal_tick.is_none_or(|tick| tick == 0)
        || proof.owner_execution
            != super::super::replay::protocol_owner_execution(invocation)
                .map_err(|_| HostProblem::UnknownOutcome)?
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let events = store
        .events(&id, proof.version, 1)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let event = events.first().ok_or(HostProblem::UnknownOutcome)?;
    if event.kind != LifecycleEventKind::Abend
        || event.execution_id != id
        || event.run_unit_id != invocation.run_unit_id
        || event.sequence != proof.version
        || event.attempt != proof.attempt
        || Some(event.tick) != execution.terminal_tick
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

/// Require retained core proof before task-end removal of an abandoned instance.
pub(super) fn verify_for_cleanup(
    store: &dyn PlatformStore,
    invocation: &Invocation,
    record: &ProviderStateRecord,
    instance: &Instance,
) -> Result<(), HostProblem> {
    if let Some(proof) = &instance.abend {
        verify_execution(store, invocation, &record.key, &instance.artifact, proof)?;
    }
    Ok(())
}

impl Lease {
    /// Publish only this lease's known-ABEND disposition under both CAS fences.
    pub(in super::super) fn abended(
        mut self,
        store: &dyn PlatformStore,
        invocation: &Invocation,
        machine: &ReferenceMachine,
    ) -> Result<(), HostProblem> {
        // Only called for ExecutionOutcome::Abend after the durable coordinator
        // commits Failed + Abend. A generic Failed row is insufficient proof.
        let execution = store
            .get_execution(&invocation.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let mut proof = AbendProof {
            execution: invocation.execution_id.as_str().into(),
            owner_execution: super::super::replay::protocol_owner_execution(invocation)
                .map_err(|_| HostProblem::UnknownOutcome)?,
            version: execution.version,
            attempt: invocation.attempt,
            metadata_digest: String::new(),
        };
        verify_execution(
            store,
            invocation,
            &self.name,
            &self.instance.artifact,
            &proof,
        )?;
        self.instance.schema_version = 2;
        self.instance.busy = false;
        self.instance.open_files = !machine.dataset_cursors().is_empty();
        // Neither pre-call nor normal-return state can resume an abandoned frame.
        self.instance.state = None;
        let mut instance_write = write(
            &self.namespace,
            &self.name,
            &self.instance,
            Some(self.version),
        )?;
        proof.metadata_digest = metadata_digest(&instance_write.record, &self.instance, &proof);
        self.instance.abend = Some(proof);
        instance_write.record.payload =
            serde_json::to_vec(&self.instance).map_err(|_| HostProblem::UnknownOutcome)?;
        let (mut state, expected) = load_run(store, &self.run)?;
        adopt_run_owner(&mut state, invocation)?;
        if state.ended || state.active == 0 || !state.programs.contains(&self.name) {
            return Err(HostProblem::UnknownOutcome);
        }
        state.active -= 1;
        refresh_run_metadata(&mut state, &self.run);
        // No reply write: pending installed reservations remain non-retryable.
        store
            .put_provider_states_atomic(vec![
                write(RUN_STATE_NAMESPACE, &self.run, &state, expected)?,
                instance_write,
            ])
            .map_err(|_| HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobol::hardening::{Fixture, TestRoot, parent};
    use mainframe_env_execution_api::{
        Abend, AbendDumpDisposition, Condition, Machine, MachineDrive, MachineResume, Quantum,
    };
    use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};

    struct TerminalMachine(bool);
    impl Machine for TerminalMachine {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;
        fn drive(
            &mut self,
            _: MachineResume<EffectResult>,
            _: Quantum,
        ) -> MachineDrive<EffectRequest> {
            if self.0 {
                MachineDrive::Abend(Abend {
                    code: "U789".into(),
                    reason: None,
                    dump: AbendDumpDisposition::Suppressed,
                })
            } else {
                MachineDrive::Condition(Condition {
                    name: "NOT-ABEND".into(),
                    response: 16,
                    response2: 0,
                    handled: false,
                })
            }
        }
    }

    fn fixture(sqlite: bool) -> (TestRoot, Fixture, Invocation, ReferenceMachine) {
        let root = TestRoot::new();
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rwc", root.0.join("state.db").display()),
                    16 * 1024 * 1024,
                    10_000,
                )
                .unwrap(),
            )
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        fixture_with_store(root, store)
    }

    fn fixture_with_store(
        root: TestRoot,
        store: Arc<dyn PlatformStore>,
    ) -> (TestRoot, Fixture, Invocation, ReferenceMachine) {
        let fixture = Fixture::new(&root, store, HostProblem::NotFound, false);
        fixture.install(
            "LEAF",
            "IDENTIFICATION DIVISION. PROGRAM-ID. LEAF. PROCEDURE DIVISION. GOBACK.",
        );
        let admitted = fixture
            .router
            .cobol
            .preflight_installed_program("LEAF", false)
            .unwrap();
        let mut invocation = parent();
        invocation.artifact = admitted.artifact;
        invocation.selector = Selector::new("program:LEAF", InvocationLimits::default()).unwrap();
        let machine = ReferenceMachine::from_binary(
            admitted.executable.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        fixture
            .router
            .cobol
            .ensure_call_protocol(&invocation)
            .unwrap();
        (root, fixture, invocation, machine)
    }

    fn terminal(fixture: &Fixture, invocation: &Invocation, abend: bool) {
        let coordinator = ExecutionCoordinator::durable(
            fixture.host.clone(),
            fixture.store.clone(),
            CoordinatorLimits::default(),
        );
        let outcome = coordinator.execute(
            &mut TerminalMachine(abend),
            invocation,
            ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            },
        );
        assert!(if abend {
            matches!(outcome, ExecutionOutcome::Abend(_))
        } else {
            matches!(outcome, ExecutionOutcome::Condition(_))
        });
    }

    fn rows(
        fixture: &Fixture,
        invocation: &Invocation,
    ) -> (ProviderStateRecord, ProviderStateRecord) {
        let key = run_key(invocation);
        (
            fixture
                .store
                .get_provider_state(RUN_STATE_NAMESPACE, &key)
                .unwrap()
                .unwrap(),
            fixture
                .store
                .get_provider_state(&namespace(&key), "LEAF")
                .unwrap()
                .unwrap(),
        )
    }

    #[test]
    fn known_abend_instance_is_inactive_but_fenced_until_owned_task_cleanup() {
        for sqlite in [false, true] {
            let (root, mut fixture, invocation, mut machine) = fixture(sqlite);
            let lease =
                Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
            let (_, old) = rows(&fixture, &invocation);
            assert_eq!(decode_instance(&old).unwrap().schema_version, 1);
            assert!(
                !serde_json::from_slice::<serde_json::Value>(&old.payload)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .contains_key("abend")
            );
            terminal(&fixture, &invocation, true);
            lease
                .abended(fixture.store.as_ref(), &invocation, &machine)
                .unwrap();
            let (run, instance) = rows(&fixture, &invocation);
            assert_eq!(decode_run_state(&run).unwrap().active, 0);
            let saved = decode_instance(&instance).unwrap();
            assert_eq!(saved.schema_version, 2);
            assert!(!saved.busy);
            assert!(saved.abend.is_some());
            let descriptor = describe_instance_row(&instance).unwrap();
            assert_eq!(descriptor.state, CobolRetentionState::Active);
            assert!(descriptor.dependencies.contains(
                &super::super::super::retention::CobolRetentionDependency::Execution(
                    invocation.execution_id.as_str().into()
                )
            ));
            assert!(matches!(
                Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine),
                Err(HostProblem::UnknownOutcome)
            ));
            assert_eq!(rows(&fixture, &invocation), (run, instance));
            if sqlite {
                drop(fixture);
                fixture = Fixture::new(
                    &root,
                    Arc::new(
                        SqliteStateStore::open(
                            &format!("sqlite://{}?mode=rwc", root.0.join("state.db").display()),
                            16 * 1024 * 1024,
                            10_000,
                        )
                        .unwrap(),
                    ),
                    HostProblem::NotFound,
                    false,
                );
            }
            fixture.router.finish_run_unit(&invocation).unwrap();
            assert!(
                fixture
                    .store
                    .list_provider_state(&namespace(&run_key(&invocation)), 8)
                    .unwrap()
                    .is_empty()
            );
            let (state, _) = load_run(fixture.store.as_ref(), &run_key(&invocation)).unwrap();
            assert!(state.ended);
            assert_eq!(state.active, 0);
            assert_eq!(state.instances, 0);
            assert!(state.programs.is_empty());
        }
    }

    #[test]
    fn missing_non_abend_and_foreign_terminal_proofs_cannot_retire_a_lease() {
        for sqlite in [false, true] {
            for case in 0..7 {
                let (_root, fixture, invocation, mut machine) = fixture(sqlite);
                let lease =
                    Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine)
                        .unwrap();
                let before = rows(&fixture, &invocation);
                if case != 0 {
                    let mut foreign = invocation.clone();
                    match case {
                        2 => {
                            foreign.run_unit_id =
                                RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                        }
                        3 => {
                            foreign.principal = Principal::new(
                                mainframe_env_execution_api::PrincipalId::new(
                                    "FOREIGN",
                                    InvocationLimits::default(),
                                )
                                .unwrap(),
                                foreign.principal.grants().clone(),
                                InvocationLimits::default(),
                            )
                            .unwrap()
                        }
                        4 => {
                            foreign.artifact =
                                ArtifactRef::new("foreign-artifact", InvocationLimits::default())
                                    .unwrap()
                        }
                        5 => {
                            foreign.selector =
                                Selector::new("program:FOREIGN", InvocationLimits::default())
                                    .unwrap()
                        }
                        6 => foreign.attempt += 1,
                        _ => {}
                    }
                    terminal(&fixture, &foreign, case != 1);
                }
                assert_eq!(
                    lease.abended(fixture.store.as_ref(), &invocation, &machine),
                    Err(HostProblem::UnknownOutcome)
                );
                assert_eq!(
                    rows(&fixture, &invocation),
                    before,
                    "backend sqlite={sqlite} case={case}"
                );
            }
        }
    }

    #[test]
    fn abend_instance_cas_conflict_cannot_partially_decrement_the_run() {
        for sqlite in [false, true] {
            let (_root, fixture, invocation, mut machine) = fixture(sqlite);
            let lease =
                Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
            terminal(&fixture, &invocation, true);
            let (_, mut raced) = rows(&fixture, &invocation);
            let expected = raced.version;
            raced.version += 1;
            fixture
                .store
                .put_provider_state(raced, Some(expected))
                .unwrap();
            let before = rows(&fixture, &invocation);
            assert_eq!(
                lease.abended(fixture.store.as_ref(), &invocation, &machine),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(rows(&fixture, &invocation), before);
            assert_eq!(decode_run_state(&before.0).unwrap().active, 1);
        }
    }

    #[test]
    fn known_abend_codec_rejects_relabelled_and_corrupt_proof_metadata() {
        let (_root, fixture, invocation, mut machine) = fixture(false);
        let lease =
            Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
        terminal(&fixture, &invocation, true);
        lease
            .abended(fixture.store.as_ref(), &invocation, &machine)
            .unwrap();
        let (_, row) = rows(&fixture, &invocation);
        for (field, value) in [
            ("schema_version", serde_json::json!(1)),
            ("schema_version", serde_json::json!(3)),
            ("busy", serde_json::json!(true)),
            ("open_files", serde_json::json!(true)),
            ("artifact", serde_json::json!("foreign")),
            ("state", serde_json::json!([1])),
            ("abend", serde_json::Value::Null),
        ] {
            let mut malformed = row.clone();
            let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            payload[field] = value;
            malformed.payload = serde_json::to_vec(&payload).unwrap();
            assert!(decode_instance(&malformed).is_err(), "{field}");
        }
        let mut foreign = row.clone();
        foreign.key = "OTHER".into();
        assert!(decode_instance(&foreign).is_err());
        foreign = row.clone();
        foreign.namespace = namespace(&"0".repeat(64));
        assert!(decode_instance(&foreign).is_err());
        foreign = row.clone();
        foreign.version = 1;
        assert!(decode_instance(&foreign).is_err());
        let mut saved = decode_instance(&row).unwrap();
        let proof = saved.abend.as_mut().unwrap();
        proof.execution = "missing-execution".into();
        let proof = saved.abend.as_ref().unwrap();
        let digest = metadata_digest(&row, &saved, proof);
        saved.abend.as_mut().unwrap().metadata_digest = digest;
        assert_eq!(
            verify_for_cleanup(fixture.store.as_ref(), &invocation, &row, &saved),
            Err(HostProblem::UnknownOutcome)
        );
        let mut foreign_owner = decode_instance(&row).unwrap();
        foreign_owner.abend.as_mut().unwrap().owner_execution = "foreign-owner".into();
        let digest = metadata_digest(&row, &foreign_owner, foreign_owner.abend.as_ref().unwrap());
        foreign_owner.abend.as_mut().unwrap().metadata_digest = digest;
        assert!(valid_instance(&row, &foreign_owner));
        assert_eq!(
            verify_for_cleanup(fixture.store.as_ref(), &invocation, &row, &foreign_owner),
            Err(HostProblem::UnknownOutcome)
        );
        for (version, attempt, execution) in [
            (0, 1, "child".to_string()),
            (u64::MAX, 1, "child".to_string()),
            (1, 0, "child".to_string()),
            (1, 1, String::new()),
            (1, 1, "bad execution".to_string()),
            (1, 1, "x".repeat(129)),
        ] {
            let mut instance = decode_instance(&row).unwrap();
            let proof = instance.abend.as_mut().unwrap();
            proof.version = version;
            proof.attempt = attempt;
            proof.execution = execution;
            let digest = metadata_digest(&row, &instance, instance.abend.as_ref().unwrap());
            instance.abend.as_mut().unwrap().metadata_digest = digest;
            let mut malformed = row.clone();
            malformed.payload = serde_json::to_vec(&instance).unwrap();
            assert!(decode_instance(&malformed).is_err());
        }
    }

    #[test]
    fn known_abend_metadata_has_a_frozen_length_framed_vector() {
        let row = ProviderStateRecord {
            namespace: namespace(&"0".repeat(64)),
            key: "LEAF".into(),
            version: 2,
            payload: Vec::new(),
        };
        let instance = Instance {
            schema_version: 2,
            artifact: format!("sha256:{}", "a".repeat(64)),
            busy: false,
            open_files: false,
            state: None,
            abend: None,
        };
        let proof = AbendProof {
            execution: "child-execution".into(),
            owner_execution: "root-execution".into(),
            version: 9,
            attempt: 1,
            metadata_digest: String::new(),
        };
        assert_eq!(
            metadata_digest(&row, &instance, &proof),
            "14a03ab133ac156ecff00c2d6efd3e594d0672f059ca439e27b240cd1b7c2a27"
        );
    }

    #[test]
    fn owned_abend_cleanup_never_retires_another_active_instance() {
        for sqlite in [false, true] {
            let (_root, fixture, invocation, mut machine) = fixture(sqlite);
            let lease =
                Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
            let _other =
                Lease::acquire(fixture.store.as_ref(), &invocation, "OTHER", &mut machine).unwrap();
            terminal(&fixture, &invocation, true);
            lease
                .abended(fixture.store.as_ref(), &invocation, &machine)
                .unwrap();
            let before = rows(&fixture, &invocation);
            assert_eq!(decode_run_state(&before.0).unwrap().active, 1);
            assert_eq!(
                fixture.router.finish_run_unit(&invocation),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(rows(&fixture, &invocation), before);
            let other = fixture
                .store
                .get_provider_state(&namespace(&run_key(&invocation)), "OTHER")
                .unwrap()
                .unwrap();
            assert!(decode_instance(&other).unwrap().busy);
        }
    }

    #[test]
    fn abandoned_frame_owner_open_files_and_cancel_keep_cleanup_fenced() {
        for sqlite in [false, true] {
            let (_root, fixture, invocation, mut machine) = fixture(sqlite);
            let lease =
                Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
            terminal(&fixture, &invocation, true);
            lease
                .abended(fixture.store.as_ref(), &invocation, &machine)
                .unwrap();
            let before = rows(&fixture, &invocation);
            let mut foreign = invocation.clone();
            foreign.execution_id =
                ExecutionId::new("foreign-owner", InvocationLimits::default()).unwrap();
            assert_eq!(
                fixture.router.finish_run_unit(&foreign),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(rows(&fixture, &invocation), before);
            let effect = EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(
                    IdempotencyKey::new("cancel-abandoned", InvocationLimits::default()).unwrap(),
                ),
                request: HostRequest::Program(ProgramRequest::Cancel {
                    programs: vec![mainframe_env_host_api::ProgramName::new("LEAF", 128).unwrap()],
                }),
            };
            assert_eq!(
                fixture.router.invoke(&invocation, effect).outcome,
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&fixture, &invocation), before);
            let mut row = before.1.clone();
            let mut instance = decode_instance(&row).unwrap();
            instance.open_files = true;
            let digest = metadata_digest(&row, &instance, instance.abend.as_ref().unwrap());
            instance.abend.as_mut().unwrap().metadata_digest = digest;
            row.payload = serde_json::to_vec(&instance).unwrap();
            let version = row.version;
            row.version += 1;
            fixture
                .store
                .put_provider_state(row, Some(version))
                .unwrap();
            let open = rows(&fixture, &invocation);
            assert_eq!(
                fixture.router.finish_run_unit(&invocation),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&fixture, &invocation), open);
        }
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_known_abend_marker_reopens_and_owned_cleanup_is_atomic() {
        let url =
            std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").expect("fresh PostgreSQL fixture URL");
        let (root, fixture, invocation, mut machine) = fixture_with_store(
            TestRoot::new(),
            Arc::new(PostgresStateStore::open(&url, 16 * 1024 * 1024, 10_000).unwrap()),
        );
        let lease =
            Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
        terminal(&fixture, &invocation, true);
        // A losing instance CAS cannot partly publish a known-ABEND disposition.
        let (_, mut raced) = rows(&fixture, &invocation);
        let version = raced.version;
        raced.version += 1;
        fixture
            .store
            .put_provider_state(raced, Some(version))
            .unwrap();
        let before = rows(&fixture, &invocation);
        assert_eq!(
            lease.abended(fixture.store.as_ref(), &invocation, &machine),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(rows(&fixture, &invocation), before);
        // Leave the losing run fenced. Exercise success on a separate fresh run,
        // never by retrying or reconstructing its abandoned pending invocation.
        let raced_invocation = invocation.clone();
        let mut invocation = invocation;
        invocation.execution_id =
            ExecutionId::new("known-abend-owner", InvocationLimits::default()).unwrap();
        invocation.run_unit_id =
            RunUnitId::new("known-abend-run", InvocationLimits::default()).unwrap();
        fixture
            .router
            .cobol
            .ensure_call_protocol(&invocation)
            .unwrap();
        let admitted = fixture
            .router
            .cobol
            .preflight_installed_program("LEAF", false)
            .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            admitted.executable.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        let lease =
            Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine).unwrap();
        terminal(&fixture, &invocation, true);
        lease
            .abended(fixture.store.as_ref(), &invocation, &machine)
            .unwrap();
        let saved = rows(&fixture, &invocation);
        drop(fixture);
        let fixture = Fixture::new(
            &root,
            Arc::new(PostgresStateStore::open(&url, 16 * 1024 * 1024, 10_000).unwrap()),
            HostProblem::NotFound,
            false,
        );
        assert_eq!(rows(&fixture, &invocation), saved);
        assert!(matches!(
            Lease::acquire(fixture.store.as_ref(), &invocation, "LEAF", &mut machine),
            Err(HostProblem::UnknownOutcome)
        ));
        fixture.router.finish_run_unit(&invocation).unwrap();
        assert!(
            fixture
                .store
                .list_provider_state(&namespace(&run_key(&invocation)), 8)
                .unwrap()
                .is_empty()
        );
        let (state, _) = load_run(fixture.store.as_ref(), &run_key(&invocation)).unwrap();
        assert!(state.ended);
        assert_eq!(state.active, 0);
        assert_eq!(state.instances, 0);
        assert_eq!(rows(&fixture, &raced_invocation), before);
    }
}
