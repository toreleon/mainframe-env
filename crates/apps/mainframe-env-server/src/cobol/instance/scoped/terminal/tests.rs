#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::cobol::hardening::parent;
    use mainframe_env_execution_api::{
        Completion, ExecutionOutcome, MachineDrive, MachineResume, Quantum,
    };
    use mainframe_env_host_api::{
        EffectRequest, EffectResult, HostLimits, RegistrySnapshot, ScopedHostService,
    };
    use mainframe_env_interpreter::{CoordinatorLimits, ExecutionCoordinator};
    use mainframe_env_ir::{Attribute, CodecLimits, IrLimits, ModuleBuilder, OperationIdentity};
    use mainframe_env_store::{MemoryStore, StoreLimits};
    use mainframe_env_store_api::{CheckpointStore, ExecutionStore, ProviderStateStore};
    use std::sync::Arc;

    fn context() -> (Arc<MemoryStore>, Invocation, LiveLease) {
        let mut root_actor = parent();
        root_actor.selector = Selector::new("program:MAIN", InvocationLimits::default()).unwrap();
        root_actor.artifact = ArtifactRef::new(
            format!("sha256:{}", "1".repeat(64)),
            InvocationLimits::default(),
        )
        .unwrap();
        root_actor.deadline_tick = 100;
        let mut root = ScopedRun::fresh(&root_actor).unwrap();
        let key = run_key(&root_actor);
        let call = "a".repeat(64);
        root.reserve_call_charge(&key, &call).unwrap();
        let mut actor = root_actor.clone();
        crate::cobol::replay::bind_protocol_owner(&root_actor, &mut actor.bindings).unwrap();
        actor.execution_id = ExecutionId::new(
            format!("online-call-execution-{call}"),
            InvocationLimits::default(),
        )
        .unwrap();
        actor.parent_execution_id = Some(root_actor.execution_id.clone());
        actor.selector = Selector::new("program:COUNT", InvocationLimits::default()).unwrap();
        actor.artifact = ArtifactRef::new(
            format!("sha256:{}", "2".repeat(64)),
            InvocationLimits::default(),
        )
        .unwrap();
        let entry = root.root.native_call(&root_actor, &actor, &call).unwrap();
        entry.bind(&mut actor).unwrap();
        let name = entry.member_key("COUNT").unwrap();
        let mut value = ScopedInstance {
            schema_version: 3,
            run_key: key.clone(),
            scope_entry: root.root.clone(),
            max_state_bytes: root.max_member_bytes,
            program: "COUNT".into(),
            artifact: actor.artifact.as_str().into(),
            owner: Some(entry.clone()),
            initial: false,
            state: None,
            metadata_digest: String::new(),
        };
        value.metadata_digest = value.expected_digest(&name).unwrap();
        let row = ProviderStateRecord {
            namespace: namespace(&key),
            key: name.clone(),
            version: 1,
            payload: canonical(&value).unwrap(),
        };
        root.members.insert(
            name,
            Member {
                scope: entry.scope_id().into(),
                program: "COUNT".into(),
                artifact: actor.artifact.as_str().into(),
                row_version: 1,
                payload_digest: payload_digest(&row.payload),
                charged_bytes: root.max_member_bytes,
                busy: true,
            },
        );
        root.active = 1;
        root.charged_bytes += root.max_member_bytes;
        root.refresh(&key).unwrap();
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        store
            .put_provider_states_atomic(vec![
                write(RUN_STATE_NAMESPACE, &key, &root, None).unwrap(),
                ProviderStateWrite {
                    record: row.clone(),
                    expected_version: None,
                },
            ])
            .unwrap();
        let lease = LiveLease::register_known_reservation(7, actor.clone(), entry, row).unwrap();
        (store, actor, lease)
    }
    fn machine(actor: Invocation, exit: bool) -> ReferenceMachine {
        let mut b = ModuleBuilder::new(IrLimits::default());
        let r = b.add_region().unwrap();
        let block = b.add_block(r).unwrap();
        for (name, attrs) in [
            (
                "config",
                BTreeMap::from([
                    ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                    (
                        "program_lifecycle".into(),
                        Attribute::Text("retained@1".into()),
                    ),
                    ("entry_formals_v1".into(), Attribute::Text(String::new())),
                ]),
            ),
            ("continue", BTreeMap::new()),
            (
                if exit { "exit" } else { "go_back" },
                if exit {
                    BTreeMap::from([("arg_000".into(), Attribute::Text("PROGRAM".into()))])
                } else {
                    BTreeMap::new()
                },
            ),
            ("halt", BTreeMap::new()),
        ] {
            b.add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", name, 1).unwrap(),
                Vec::new(),
                0,
                attrs,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        }
        let bytes =
            mainframe_env_ir::encode_binary(&b.finish().unwrap(), CodecLimits::default()).unwrap();
        ReferenceMachine::from_binary(&bytes, actor, CodecLimits::default()).unwrap()
    }
    fn coordinator(store: Arc<MemoryStore>) -> ExecutionCoordinator {
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
    fn complete(store: Arc<MemoryStore>, actor: &Invocation, exit: bool) -> ReferenceMachine {
        let mut m = machine(actor.clone(), exit);
        assert!(matches!(
            coordinator(store).execute(&mut m, actor, control()),
            ExecutionOutcome::Completed(_)
        ));
        m
    }
    #[test]
    fn actual_native_returns_require_exact_live_core_event_and_image_without_mutation() {
        for exit in [false, true] {
            let (store, actor, lease) = context();
            let m = complete(store.clone(), &actor, exit);
            let before = lease.record().unwrap();
            let obs = TerminalObservation::capture(&actor, &m, &lease, store.as_ref(), control())
                .unwrap();
            assert_eq!(obs.core().state, ExecutionState::Completed);
            assert_eq!(obs.core().version, 5);
            assert_eq!(obs.event().sequence, 5);
            assert_eq!(obs.witness().return_code(), 0);
            assert_eq!(obs.witness().program_counter(), 2);
            assert_eq!(obs.witness().executed_steps(), 3);
            assert_eq!(
                obs.witness().kind(),
                if exit {
                    mainframe_env_interpreter::InstalledProgramReturnKind::ExitProgram
                } else {
                    mainframe_env_interpreter::InstalledProgramReturnKind::Goback
                }
            );
            assert_eq!(obs.member(), &before);
            assert_eq!(
                obs.checkpoint().payload,
                m.completion_checkpoint().unwrap().bytes()
            );
            assert_eq!(lease.record().unwrap(), before);
            assert_eq!(
                store.get_execution(&actor.execution_id).unwrap().unwrap(),
                *obs.core()
            );
            assert_eq!(
                store.get_checkpoint(&actor.execution_id).unwrap().unwrap(),
                *obs.checkpoint()
            );
        }
    }
    #[test]
    fn cold_restored_image_and_return_marker_without_publication_are_insufficient() {
        let (store, actor, lease) = context();
        let mut m = machine(actor.clone(), false);
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Completed(_)
        ));
        assert!(
            TerminalObservation::capture(&actor, &m, &lease, store.as_ref(), control()).is_err()
        );
        let m = complete(store.clone(), &actor, false);
        let mut cold = machine(actor.clone(), false);
        cold.restore_checkpoint(&m.completion_checkpoint().unwrap())
            .unwrap();
        assert!(
            TerminalObservation::capture(&actor, &cold, &lease, store.as_ref(), control()).is_err()
        );
    }
    #[test]
    fn changed_current_checkpoint_metadata_or_rehashed_payload_never_matches_live_capture() {
        let (store, actor, lease) = context();
        let m = complete(store.clone(), &actor, false);
        let original = store.get_checkpoint(&actor.execution_id).unwrap().unwrap();
        for mode in [
            "payload",
            "principal",
            "artifact",
            "session",
            "generation",
            "interfaces",
            "effect",
            "security",
            "encryption",
            "transaction",
        ] {
            let mut c = original.clone();
            match mode {
                "payload" => {
                    c.payload.push(0);
                    c.payload_size = c.payload.len() as u64;
                    c.payload_digest = sha2::Sha256::digest(&c.payload).into();
                }
                "principal" => {
                    c.principal = mainframe_env_execution_api::PrincipalId::new(
                        "OTHER",
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
                "artifact" => {
                    c.artifact = ArtifactRef::new(
                        format!("sha256:{}", "3".repeat(64)),
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
                "session" => c.session_id = Some("foreign".into()),
                "generation" => c.provider_generation = "foreign@1".into(),
                "interfaces" => {
                    c.required_host_interfaces
                        .insert("foreign".into(), "1".into());
                }
                "effect" => c.effect_sequence += 1,
                "security" => c.security_classification = "foreign".into(),
                "encryption" => c.encryption_key_reference = Some("key".into()),
                "transaction" => c.transaction = Some("foreign".into()),
                _ => unreachable!(),
            }
            store.put_checkpoint(c).unwrap();
            assert!(
                TerminalObservation::capture(&actor, &m, &lease, store.as_ref(), control())
                    .is_err(),
                "{mode}"
            );
        }
        store.put_checkpoint(original).unwrap();
        assert!(
            TerminalObservation::capture(&actor, &m, &lease, store.as_ref(), control()).is_ok()
        );
    }
    #[test]
    fn exact_invocation_current_member_and_control_are_required() {
        let (store, actor, lease) = context();
        let m = complete(store.clone(), &actor, false);
        let mut changed = actor.clone();
        changed.priority += 1;
        assert!(
            TerminalObservation::capture(&changed, &m, &lease, store.as_ref(), control()).is_err()
        );
        assert!(matches!(
            TerminalObservation::capture(
                &actor,
                &m,
                &lease,
                store.as_ref(),
                ExecutionControl {
                    cancellation_requested: true,
                    ..control()
                }
            ),
            Err(HostProblem::Cancelled)
        ));
        assert!(matches!(
            TerminalObservation::capture(
                &actor,
                &m,
                &lease,
                store.as_ref(),
                ExecutionControl {
                    now_tick: 100,
                    ..control()
                }
            ),
            Err(HostProblem::TimedOut)
        ));
        assert!(
            TerminalObservation::capture(
                &actor,
                &m,
                &lease,
                store.as_ref(),
                ExecutionControl {
                    now_tick: 8,
                    ..control()
                }
            )
            .is_err()
        );
        let row = lease.record().unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: 2,
                    ..row.clone()
                },
                Some(1),
            )
            .unwrap();
        assert!(
            TerminalObservation::capture(&actor, &m, &lease, store.as_ref(), control()).is_err()
        );
        assert_eq!(lease.record().unwrap(), row);
    }
    #[test]
    fn completed_return_code_from_another_publisher_cannot_attest_this_machine() {
        let (store, actor, lease) = context();
        let mut actual = machine(actor.clone(), false);
        assert!(matches!(
            actual.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Completed(_)
        ));
        struct OtherPublisher {
            image: BoundedPayload,
        }
        impl Machine for OtherPublisher {
            type Effect = EffectRequest;
            type EffectResult = EffectResult;
            fn drive(
                &mut self,
                _: MachineResume<EffectResult>,
                _: Quantum,
            ) -> MachineDrive<EffectRequest> {
                MachineDrive::Completed(Completion {
                    return_code: 37,
                    output: BoundedPayload::new("test@1", Vec::new(), InvocationLimits::default())
                        .unwrap(),
                })
            }
            fn completion_checkpoint(&self) -> Option<BoundedPayload> {
                Some(self.image.clone())
            }
        }
        let mut other = OtherPublisher {
            image: actual.completion_checkpoint().unwrap(),
        };
        assert!(matches!(
            coordinator(store.clone()).execute(&mut other, &actor, control()),
            ExecutionOutcome::Completed(_)
        ));
        assert!(
            TerminalObservation::capture(&actor, &actual, &lease, store.as_ref(), control())
                .is_err()
        );
        assert_eq!(
            actual
                .attest_installed_program_return()
                .unwrap()
                .return_code(),
            0
        );
    }
}
