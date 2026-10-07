#[cfg(test)]
mod tests {
    //! Actual terminal-image publication and selected-store restart regressions.
    use mainframe_env_execution_api::*;
    use mainframe_env_interpreter::ReferenceMachine;
    use mainframe_env_ir::{Attribute, CodecLimits, IrLimits, ModuleBuilder, OperationIdentity};
    use std::collections::{BTreeMap, BTreeSet};

    fn invocation(scoped: bool) -> Invocation {
        let limits = InvocationLimits::default();
        let mut invocation = Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
            None,
            Selector::new("test", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("USER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new("trace", limits).unwrap(),
            IdempotencyKey::new("key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap();

        invocation.attempt = 3;
        if scoped {
            invocation.bindings.insert(
                "cobol.storage-entry".into(),
                BoundedPayload::new(
                    "mainframe-env.cobol.storage-entry@1",
                    b"opaque opt-in".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
        }
        invocation
    }

    fn machine(invocation: Invocation, terminal: &str, args: &[&str]) -> ReferenceMachine {
        machine_with_lifecycle(invocation, terminal, args, "retained@1")
    }

    fn machine_with_lifecycle(
        invocation: Invocation,
        terminal: &str,
        args: &[&str],
        lifecycle: &str,
    ) -> ReferenceMachine {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        for (name, attributes) in [
            (
                "config",
                BTreeMap::from([
                    ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                    (
                        "program_lifecycle".into(),
                        Attribute::Text(lifecycle.into()),
                    ),
                    ("entry_formals_v1".into(), Attribute::Text(String::new())),
                ]),
            ),
            ("continue", BTreeMap::new()),
            (
                terminal,
                args.iter()
                    .enumerate()
                    .map(|(i, arg)| (format!("arg_{i:03}"), Attribute::Text((*arg).into())))
                    .collect(),
            ),
            ("halt", BTreeMap::new()),
        ] {
            builder
                .add_operation(
                    block,
                    OperationIdentity::new("mainframe.core.cobol", name, 1).unwrap(),
                    Vec::new(),
                    0,
                    attributes,
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        ReferenceMachine::from_binary(&bytes, invocation, CodecLimits::default()).unwrap()
    }

    use mainframe_env_host_api::{
        EffectRequest, EffectResult, HostLimits, RegistrySnapshot, ScopedHostService,
    };
    use mainframe_env_interpreter::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
    use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
    use mainframe_env_store_api::*;
    use std::sync::Arc;

    fn coordinator(store: Arc<dyn PlatformStore>) -> ExecutionCoordinator {
        let host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, Vec::new(), InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ));
        ExecutionCoordinator::durable(host, store, CoordinatorLimits::default())
    }
    fn control() -> ExecutionControl {
        ExecutionControl {
            now_tick: 9,
            cancellation_requested: false,
        }
    }
    struct SqliteFixture {
        root: std::path::PathBuf,
        url: String,
    }
    impl std::ops::Deref for SqliteFixture {
        type Target = str;
        fn deref(&self) -> &str {
            &self.url
        }
    }
    impl Drop for SqliteFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn sqlite(label: &str, bytes: usize) -> (SqliteFixture, Arc<SqliteStateStore>) {
        let root = std::env::temp_dir().join(super::super::unique(label));
        std::fs::create_dir(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
        let store = Arc::new(SqliteStateStore::open(&url, bytes, 4096).unwrap());
        (SqliteFixture { root, url }, store)
    }
    fn assert_complete(store: &dyn PlatformStore, inv: &Invocation, expected: &[u8]) {
        let execution = store.get_execution(&inv.execution_id).unwrap().unwrap();
        assert_eq!(execution.state, ExecutionState::Completed);
        assert_eq!(execution.version, 5);
        assert_eq!(execution.attempt, 3);
        assert_eq!(execution.terminal_tick, Some(9));
        assert_eq!(execution.run_unit_id, inv.run_unit_id);
        assert_eq!(execution.artifact, inv.artifact);
        assert_eq!(execution.principal, *inv.principal.id());
        let events = store.events(&inv.execution_id, 0, 10).unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(store.pending_notifications(10).unwrap().len(), 5);
        assert_eq!(events[3].kind, LifecycleEventKind::Completing);
        assert_eq!(
            events[4].kind,
            LifecycleEventKind::Completed { return_code: 0 }
        );
        assert_eq!(events[4].sequence, 5);
        assert_eq!(events[4].attempt, 3);
        assert_eq!(events[4].tick, 9);
        assert_eq!(events[4].execution_id, inv.execution_id);
        assert_eq!(events[4].run_unit_id, inv.run_unit_id);
        let checkpoint = store.get_checkpoint(&inv.execution_id).unwrap().unwrap();
        assert_eq!(checkpoint.payload, expected);
        assert_eq!(checkpoint.execution_id, inv.execution_id);
        assert_eq!(checkpoint.run_unit_id, inv.run_unit_id);
        assert_eq!(checkpoint.artifact, inv.artifact);
        assert_eq!(checkpoint.principal, *inv.principal.id());
        assert_eq!(checkpoint.effect_sequence, 0);
        assert_eq!(checkpoint.provider_generation, "mainframe-env-reference@1");
        assert_eq!(checkpoint.schema_version, 1);
        assert_eq!(checkpoint.machine_schema_version, 1);
        assert_eq!(
            checkpoint.required_host_interfaces,
            BTreeMap::from([
                ("mainframe-env.execution-api".into(), "1".into()),
                ("mainframe-env.host-api".into(), "1".into())
            ])
        );
        assert_eq!(checkpoint.payload_size, expected.len() as u64);
        use sha2::Digest;
        assert_eq!(
            checkpoint.payload_digest,
            <[u8; 32]>::from(sha2::Sha256::digest(expected))
        );
        assert_eq!(&expected[..8], b"MECP0012");
        assert_eq!(u64::from_be_bytes(expected[12..20].try_into().unwrap()), 2);
        assert_eq!(u64::from_be_bytes(expected[20..28].try_into().unwrap()), 0);
        assert_eq!(u64::from_be_bytes(expected[28..36].try_into().unwrap()), 3);
    }
    fn run_complete(
        store: Arc<dyn PlatformStore>,
        terminal: &str,
        args: &[&str],
    ) -> (Invocation, Vec<u8>) {
        let inv = invocation(true);
        let mut m = machine(inv.clone(), terminal, args);
        assert!(matches!(
            coordinator(store.clone()).execute(&mut m, &inv, control()),
            ExecutionOutcome::Completed(_)
        ));
        let bytes = m.completion_checkpoint().unwrap().bytes().to_vec();
        assert_complete(store.as_ref(), &inv, &bytes);
        (inv, bytes)
    }
    #[test]
    fn memory_native_returns_atomically_publish_exact_checkpoint() {
        for (terminal, args) in [("go_back", &[][..]), ("exit", &["PROGRAM"][..])] {
            run_complete(
                Arc::new(MemoryStore::new(StoreLimits::default())),
                terminal,
                args,
            );
        }
    }
    #[test]
    fn sqlite_native_returns_survive_physical_reopen_and_cannot_redispatch() {
        for (label, terminal, args) in [
            ("sqlite-goback", "go_back", &[][..]),
            ("sqlite-exit-program", "exit", &["PROGRAM"][..]),
        ] {
            let (url, store) = sqlite(label, 1024 * 1024);
            let (inv, bytes) = run_complete(store.clone(), terminal, args);
            drop(store);
            let reopened = Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 4096).unwrap());
            assert_complete(reopened.as_ref(), &inv, &bytes);
            let image = BoundedPayload::new(
                "mainframe-env.reference-machine-checkpoint@12",
                bytes.clone(),
                InvocationLimits {
                    max_payload_bytes: 1024 * 1024,
                    ..InvocationLimits::default()
                },
            )
            .unwrap();
            let mut cold = machine(inv.clone(), terminal, args);
            cold.restore_checkpoint(&image).unwrap();
            assert!(cold.completion_checkpoint().is_none());
            let mut counted = Counted {
                inner: cold,
                drives: 0,
            };
            assert!(!matches!(
                coordinator(reopened.clone()).execute_resumable_with_control(
                    &mut counted,
                    &inv,
                    || Ok(control())
                ),
                ExecutionOutcome::Completed(_)
            ));
            assert_eq!(counted.drives, 0);
            assert_complete(reopened.as_ref(), &inv, &bytes);
        }
    }
    struct Counted {
        inner: ReferenceMachine,
        drives: usize,
    }
    impl Machine for Counted {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;
        fn drive(
            &mut self,
            r: MachineResume<EffectResult>,
            q: Quantum,
        ) -> MachineDrive<EffectRequest> {
            self.drives += 1;
            self.inner.drive(r, q)
        }
        fn completion_checkpoint(&self) -> Option<BoundedPayload> {
            self.inner.completion_checkpoint()
        }
        fn effect_sequence(&self) -> u64 {
            self.inner.effect_sequence()
        }
    }
    struct InjectedCapture {
        bytes: Option<Vec<u8>>,
        drives: usize,
    }
    impl Machine for InjectedCapture {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;
        fn drive(
            &mut self,
            _: MachineResume<EffectResult>,
            _: Quantum,
        ) -> MachineDrive<EffectRequest> {
            self.drives += 1;
            MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new("test@1", Vec::new(), InvocationLimits::default())
                    .unwrap(),
            })
        }
        fn completion_checkpoint(&self) -> Option<BoundedPayload> {
            self.bytes.as_ref().map(|b| {
                BoundedPayload::new(
                    "test@1",
                    b.clone(),
                    InvocationLimits {
                        max_payload_bytes: 64 * 1024,
                        ..InvocationLimits::default()
                    },
                )
                .unwrap()
            })
        }
    }
    fn assert_fenced_failure(store: Arc<dyn PlatformStore>, bytes: Vec<u8>) -> Invocation {
        let inv = invocation(true);
        let mut m = InjectedCapture {
            bytes: Some(bytes),
            drives: 0,
        };
        assert!(matches!(
            coordinator(store.clone()).execute(&mut m, &inv, control()),
            ExecutionOutcome::InfrastructureFailure(_)
        ));
        assert_eq!(m.drives, 1);
        let row = store.get_execution(&inv.execution_id).unwrap().unwrap();
        assert_eq!(row.state, ExecutionState::Completing);
        assert_eq!(row.version, 4);
        assert_eq!(row.terminal_tick, None);
        let events = store.events(&inv.execution_id, 0, 10).unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(store.pending_notifications(10).unwrap().len(), 4);
        assert_eq!(events.last().unwrap().kind, LifecycleEventKind::Completing);
        assert!(store.get_checkpoint(&inv.execution_id).unwrap().is_none());
        let mut retry = InjectedCapture {
            bytes: Some(b"valid".to_vec()),
            drives: 0,
        };
        assert!(!matches!(
            coordinator(store.clone())
                .execute_resumable_with_control(&mut retry, &inv, || Ok(control())),
            ExecutionOutcome::Completed(_)
        ));
        assert_eq!(retry.drives, 0);
        assert_eq!(
            store.get_execution(&inv.execution_id).unwrap().unwrap(),
            row
        );
        assert_eq!(store.events(&inv.execution_id, 0, 10).unwrap(), events);
        inv
    }
    #[test]
    fn memory_invalid_checkpoint_rolls_back_completed_event_and_row() {
        assert_fenced_failure(
            Arc::new(MemoryStore::new(StoreLimits::default())),
            Vec::new(),
        );
    }
    #[test]
    fn memory_checkpoint_capacity_rolls_back_completed_event_and_row() {
        assert_fenced_failure(
            Arc::new(MemoryStore::new(StoreLimits {
                max_checkpoints: 0,
                ..StoreLimits::default()
            })),
            b"valid".to_vec(),
        );
    }
    #[test]
    fn sqlite_invalid_checkpoint_rolls_back_and_reopens_completing() {
        let (url, store) = sqlite("sqlite-invalid", 1024 * 1024);
        let inv = assert_fenced_failure(store.clone(), Vec::new());
        drop(store);
        let reopened = SqliteStateStore::open(&url, 1024 * 1024, 4096).unwrap();
        assert_eq!(
            reopened
                .get_execution(&inv.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completing
        );
        assert_eq!(reopened.events(&inv.execution_id, 0, 10).unwrap().len(), 4);
        assert!(
            reopened
                .get_checkpoint(&inv.execution_id)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn sqlite_checkpoint_capacity_rolls_back_and_reopens_completing() {
        let (url, store) = sqlite("sqlite-capacity", 2048);
        let inv = assert_fenced_failure(store.clone(), vec![7; 8192]);
        drop(store);
        let reopened = SqliteStateStore::open(&url, 2048, 4096).unwrap();
        assert_eq!(
            reopened
                .get_execution(&inv.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completing
        );
        assert_eq!(reopened.events(&inv.execution_id, 0, 10).unwrap().len(), 4);
        assert!(
            reopened
                .get_checkpoint(&inv.execution_id)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn memory_absent_capture_keeps_legacy_completion_without_checkpoint() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let inv = invocation(false);
        let mut m = InjectedCapture {
            bytes: None,
            drives: 0,
        };
        assert!(matches!(
            coordinator(store.clone()).execute(&mut m, &inv, control()),
            ExecutionOutcome::Completed(_)
        ));
        assert_eq!(
            store
                .get_execution(&inv.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert!(store.get_checkpoint(&inv.execution_id).unwrap().is_none());
    }
    #[test]
    fn sqlite_unscoped_reference_and_abnormal_return_do_not_publish_checkpoint() {
        for (label, scoped, terminal, args) in [
            ("sqlite-unscoped", false, "go_back", &[][..]),
            ("sqlite-stop", true, "stop_run", &[][..]),
        ] {
            let (_fixture, store) = sqlite(label, 1024 * 1024);
            let inv = invocation(scoped);
            let mut m = machine(inv.clone(), terminal, args);
            assert!(matches!(
                coordinator(store.clone()).execute(&mut m, &inv, control()),
                ExecutionOutcome::Completed(_)
            ));
            assert!(store.get_checkpoint(&inv.execution_id).unwrap().is_none());
        }
    }
    #[test]
    fn memory_post_drive_cancellation_and_deadline_prevent_terminal_publication() {
        for cancelled in [true, false] {
            let store = Arc::new(MemoryStore::new(StoreLimits::default()));
            let inv = invocation(true);
            let mut m = machine(inv.clone(), "go_back", &[]);
            let mut observations = 0;
            let outcome = coordinator(store.clone()).execute_with_control(&mut m, &inv, || {
                observations += 1;
                Ok(if observations == 1 {
                    control()
                } else {
                    ExecutionControl {
                        now_tick: if cancelled { 9 } else { 100 },
                        cancellation_requested: cancelled,
                    }
                })
            });
            assert!(matches!(
                outcome,
                ExecutionOutcome::Cancelled | ExecutionOutcome::TimedOut
            ));
            assert!(store.get_checkpoint(&inv.execution_id).unwrap().is_none());
            assert!(
                !store
                    .events(&inv.execution_id, 0, 10)
                    .unwrap()
                    .iter()
                    .any(|e| matches!(
                        e.kind,
                        LifecycleEventKind::Completing | LifecycleEventKind::Completed { .. }
                    ))
            );
        }
    }
    fn postgres(bytes: usize) -> (String, Arc<PostgresStateStore>) {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("fresh isolated PostgreSQL database required for this selector");
        let store = Arc::new(PostgresStateStore::open(&url, bytes, 4096).unwrap());
        assert!(
            store
                .get_execution(&invocation(true).execution_id)
                .unwrap()
                .is_none()
        );
        assert!(store.pending_notifications(1).unwrap().is_empty());
        (url, store)
    }
    fn postgres_return(terminal: &str, args: &[&str]) {
        let (url, store) = postgres(1024 * 1024);
        let (inv, bytes) = run_complete(store.clone(), terminal, args);
        drop(store);
        let reopened = Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 4096).unwrap());
        assert_complete(reopened.as_ref(), &inv, &bytes);
        let mut cold = machine(inv.clone(), terminal, args);
        let image = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@12",
            bytes.clone(),
            InvocationLimits {
                max_payload_bytes: 1024 * 1024,
                ..InvocationLimits::default()
            },
        )
        .unwrap();
        cold.restore_checkpoint(&image).unwrap();
        assert!(cold.completion_checkpoint().is_none());
        let mut counted = Counted {
            inner: cold,
            drives: 0,
        };
        assert!(!matches!(
            coordinator(reopened.clone()).execute_resumable_with_control(
                &mut counted,
                &inv,
                || Ok(control())
            ),
            ExecutionOutcome::Completed(_)
        ));
        assert_eq!(counted.drives, 0);
        assert_complete(reopened.as_ref(), &inv, &bytes);
    }
    #[test]
    #[ignore = "requires a fresh isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
    fn postgres_goback_reopens_completed_without_redispatch() {
        postgres_return("go_back", &[]);
    }
    #[test]
    #[ignore = "requires a fresh isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
    fn postgres_exit_program_reopens_completed_without_redispatch() {
        postgres_return("exit", &["PROGRAM"]);
    }
    fn postgres_failure(bytes: usize, image: Vec<u8>) {
        let (url, store) = postgres(bytes);
        let inv = assert_fenced_failure(store.clone(), image);
        let expected = store.get_execution(&inv.execution_id).unwrap().unwrap();
        let events = store.events(&inv.execution_id, 0, 10).unwrap();
        drop(store);
        let reopened = PostgresStateStore::open(&url, bytes, 4096).unwrap();
        assert_eq!(
            reopened.get_execution(&inv.execution_id).unwrap().unwrap(),
            expected
        );
        assert_eq!(reopened.events(&inv.execution_id, 0, 10).unwrap(), events);
        assert_eq!(reopened.pending_notifications(10).unwrap().len(), 4);
        assert!(
            reopened
                .get_checkpoint(&inv.execution_id)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    #[ignore = "requires a fresh isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
    fn postgres_invalid_image_reopens_completing_without_partial_publication() {
        postgres_failure(1024 * 1024, Vec::new());
    }
    #[test]
    #[ignore = "requires a fresh isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
    fn postgres_payload_limit_reopens_completing_without_partial_publication() {
        postgres_failure(2048, vec![7; 8192]);
    }

    #[test]
    fn legacy_none_preserves_earlier_checkpoint_without_fabricating_terminal_capture() {
        struct LegacyMachine;
        impl Machine for LegacyMachine {
            type Effect = EffectRequest;
            type EffectResult = EffectResult;
            fn drive(
                &mut self,
                _: MachineResume<EffectResult>,
                _: Quantum,
            ) -> MachineDrive<EffectRequest> {
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new("test@1", Vec::new(), InvocationLimits::default())
                        .unwrap(),
                })
            }
            fn checkpoint(&self) -> Option<BoundedPayload> {
                panic!("ordinary checkpoint must not be used as a terminal fallback")
            }
        }
        let inv = invocation(false);
        let source = machine(inv.clone(), "go_back", &[]);
        let bytes = source.checkpoint().unwrap().bytes().to_vec();
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        use sha2::Digest;
        let old = CheckpointRecord {
            execution_id: inv.execution_id.clone(),
            run_unit_id: inv.run_unit_id.clone(),
            session_id: Some("earlier".into()),
            schema_version: 1,
            machine_schema_version: 1,
            artifact: inv.artifact.clone(),
            provider_generation: "mainframe-env-reference@1".into(),
            required_host_interfaces: BTreeMap::from([
                ("mainframe-env.execution-api".into(), "1".into()),
                ("mainframe-env.host-api".into(), "1".into()),
            ]),
            effect_sequence: 0,
            transaction: None,
            principal: inv.principal.id().clone(),
            security_classification: "application-data".into(),
            encryption_key_reference: None,
            payload_size: bytes.len() as u64,
            payload_digest: sha2::Sha256::digest(&bytes).into(),
            payload: bytes,
        };
        let mut observations = 0;
        let mut m = LegacyMachine;
        assert!(m.completion_checkpoint().is_none());
        let outcome = coordinator(store.clone()).execute_with_control(&mut m, &inv, || {
            observations += 1;
            if observations == 2 {
                store.put_checkpoint(old.clone()).unwrap();
            }
            Ok(control())
        });
        assert!(matches!(outcome, ExecutionOutcome::Completed(_)));
        assert_eq!(
            store
                .get_execution(&inv.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert_eq!(
            store.get_checkpoint(&inv.execution_id).unwrap().unwrap(),
            old
        );
        assert_eq!(source.snapshot().program_counter, 0);
        assert_eq!(source.snapshot().executed_steps, 0);
        assert!(source.completion_checkpoint().is_none());
    }
}
