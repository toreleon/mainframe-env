//! The real signed-package composition and canonical execution coordinator.
use super::*;
use mainframe_env_execution_api::{Completion, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{
    ImsCallSyntax, ImsExecutionContext, ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult,
};
use mainframe_env_store_api::AuditSink;
use std::path::PathBuf;

#[path = "ims_application_backout_tests.rs"]
mod application_backout;
#[path = "ims_tm_backout_gap_tests.rs"]
mod tm_backout_gap;

struct LogMachine {
    effect: Option<EffectRequest>,
    result: Option<EffectResult>,
}
impl Machine for LogMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.effect.take().unwrap()),
            MachineResume::HostResult(result) => {
                self.result = Some(result);
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new("test@1", vec![], InvocationLimits::default())
                        .unwrap(),
                })
            }
            _ => panic!("unexpected LOG machine resume"),
        }
    }
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let installed = server
        .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let ids = InvocationLimits::default();
    let mut invocation = tm_invocation("coordinator-log-run", "coordinator-log");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = 100;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    // The ordinary selected API installs the signed DBD/PSB before dispatch.
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &invocation,
            &ImsRequest {
                operation: ImsOperation::Schedule,
                psb: Some("AUTHPSB".into()),
                pcb: 1,
                segments: vec![],
                data: vec![],
                qualifiers: vec![],
                checkpoint_id: None,
                max_segments: 16,
                system: None,
                q_class: None,
                mutation: Some(Mutation {
                    sequence: 10,
                    idempotency_key: IdempotencyKey::new("log-schedule", ids).unwrap(),
                    transaction: None,
                }),
            },
        )
        .unwrap();
    let key = IdempotencyKey::new("coordinator-log-effect", ids).unwrap();
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        idempotency_key: Some(key.clone()),
        deadline_tick: invocation.deadline_tick,
        request: HostRequest::ImsRecovery(ImsRecoveryRequest {
            application: "SIGNED-IMS-APPLICATION".into(),
            package_identity: installed.identity,
            psb: "AUTHPSB".into(),
            database: "AUTHDB".into(),
            context: ImsExecutionContext::DbBatch,
            syntax: ImsCallSyntax::Call,
            call: ImsRecoveryCall::Log {
                code: 0xff,
                data: vec![],
            },
            mutation: Mutation {
                sequence: 1,
                idempotency_key: key.clone(),
                transaction: None,
            },
        }),
    };
    let request_digest = mainframe_env_host_api::canonical_request_digest(&effect.request).unwrap();
    let mut machine = LogMachine {
        effect: Some(effect),
        result: None,
    };
    let coordinator = ExecutionCoordinator::durable(
        server.host.clone(),
        store.clone(),
        CoordinatorLimits::default(),
    );
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    let expected = Ok(HostResult::ImsRecovery(ImsRecoveryResult::Logged {
        status: "  ".into(),
        sequence: 1,
    }));
    assert_eq!(machine.result.unwrap().outcome, expected);
    let effect = store.effect(&key).unwrap().unwrap();
    assert_eq!(effect.state, EffectState::Completed);
    assert_eq!(effect.digest_format, EffectDigestFormat::CanonicalHostV1);
    assert_eq!(effect.request_digest, request_digest);
    assert_eq!(
        effect.result_digest,
        Some(mainframe_env_host_api::canonical_result_digest(&expected).unwrap())
    );
    let audits = store
        .audit_records(&invocation.execution_id, 0, 16)
        .unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].decision,
        mainframe_env_execution_api::AuditDecision::Success
    );
    assert_eq!(audits[0].principal, *invocation.principal.id());
    assert_eq!(
        store
            .list_provider_state("ims-recovery-v1-session", 16)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn signed_memory_log_uses_canonical_coordinator_and_durable_audit() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_log_uses_canonical_coordinator_and_durable_audit() {
    let path = std::env::temp_dir().join(format!("ims-server-log-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    std::fs::remove_file(path).unwrap();
}

fn private_retention_policy() -> mainframe_env_store_api::RetentionPolicy {
    mainframe_env_store_api::RetentionPolicy {
        lifecycle_ticks: 1,
        idempotency_ticks: 1,
        audit_ticks: 1,
        archive_ticks: 1,
        low_watermark_percent: 70,
        high_watermark_percent: 85,
        max_batch: 64,
    }
}

fn exercise_private_retention_log(
    store: Arc<dyn PlatformStore>,
    config: ServerConfig,
    prune_ordinary: bool,
) -> (Arc<ProductServer>, Invocation, ImsRecoveryRequest) {
    use crate::retention_maintenance::provider::RetentionPlanner;
    use mainframe_env_store_api::RetentionTarget;
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let installed = server
        .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    let bootstrapped = server.bootstrap_record(BOOTSTRAP_KEY).unwrap().is_some();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    if !bootstrapped {
        for (class, profile) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
            server
                .racf
                .define_profile(class, profile, "IBMUSER", Some(AccessIntent::Control))
                .unwrap();
        }
    }
    let mut invocation = tm_invocation("private-retention-log-run", "private-retention-log");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap().saturating_add(60_000);
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
        BTreeSet::from([CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]),
        InvocationLimits::default(),
    )
    .unwrap();
    let schedule = recovery_db(ImsOperation::Schedule, 1, "private-retention-schedule");
    server
        .ims_execute_selected("SIGNED-IMS-APPLICATION", &invocation, &schedule)
        .unwrap();
    let request = recovery_request(
        &installed.identity,
        2,
        "private-retention-log",
        ImsRecoveryCall::Log {
            code: 0xa0,
            data: b"retained log".to_vec(),
        },
    );
    let host_request = HostRequest::ImsRecovery(request.clone());
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &invocation,
        vec![HostRequest::Ims(schedule), host_request.clone()],
    );
    assert_eq!(
        results[1].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Logged {
            status: "  ".into(),
            sequence: 1,
        }))
    );
    let effect = store
        .effect(&request.mutation.idempotency_key)
        .unwrap()
        .unwrap();
    let rows = store
        .list_provider_state_prefix("ims-recovery-v1-", 16)
        .unwrap();
    assert!(!rows.is_empty());
    let now = invocation.deadline_tick.saturating_add(100);
    let planner =
        RetentionPlanner::from_existing(store.clone(), private_retention_policy(), None).unwrap();
    if prune_ordinary {
        let ordinary = planner
            .archive_and_prune(RetentionTarget::ImsReplay, now, 64)
            .unwrap();
        assert_eq!(ordinary.pruned, 1);
        assert!(
            store
                .list_provider_state("ims-v1-replay", 16)
                .unwrap()
                .is_empty()
        );
    }
    let forecast = planner
        .forecast(RetentionTarget::ResolvedEffects, now, 0)
        .unwrap();
    assert_eq!(
        forecast.eligible_records, 0,
        "private LOG lost its canonical dependency"
    );
    let receipt = planner
        .archive_and_prune(RetentionTarget::ResolvedEffects, now, 64)
        .unwrap();
    assert_eq!(receipt.pruned, 0);
    assert_eq!(store.effect(&effect.key).unwrap(), Some(effect));
    let replay = server.host.invoke(
        &invocation,
        now.saturating_sub(200),
        false,
        EffectRequest {
            sequence: 2,
            run_unit: invocation.run_unit_id.clone(),
            request: host_request,
            idempotency_key: Some(request.mutation.idempotency_key.clone()),
            deadline_tick: invocation.deadline_tick,
        },
    );
    assert_eq!(
        replay.persist_with(|audit| store.record_audit(audit).map_err(store_error)),
        results[1]
    );
    assert_eq!(
        store
            .list_provider_state_prefix("ims-recovery-v1-", 16)
            .unwrap(),
        rows
    );
    (server, invocation, request)
}

#[test]
fn ims_private_recovery_retention_selected_log_memory() {
    exercise_private_retention_log(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        true,
    );
}

#[test]
fn ims_private_recovery_retention_selected_log_sqlite() {
    let path = std::env::temp_dir().join(format!(
        "ims-private-retention-log-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise_private_retention_log(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        config,
        true,
    );
    std::fs::remove_file(path).unwrap();
}

// This snapshot compares existing authoritative APIs, including full IMS payloads and
// canonical journals. It is an external test receipt, not a portable recovery-graph schema.
fn private_graph_snapshot(store: &dyn PlatformStore) -> String {
    use mainframe_env_store_api::RetentionTarget;
    let ids = InvocationLimits::default();
    let mut parts = vec![
        format!(
            "ims={:?}",
            store.list_provider_state_prefix("ims-", 4096).unwrap()
        ),
        format!(
            "ordinary_archives={:?}",
            store
                .retention_archives(RetentionTarget::ImsReplay, 64)
                .unwrap()
        ),
        format!("epoch={}", store.provider_state_retention_epoch().unwrap()),
        format!("clock={}", store.advance_logical_clock(1).unwrap()),
    ];
    for label in [
        "private-retention-log",
        "selected-checkpoint",
        "selected-restart-execution",
        "selected-basic-execution",
    ] {
        let execution = if label.starts_with("selected-") && label.ends_with("execution") {
            ExecutionId::new(label, ids).unwrap()
        } else {
            ExecutionId::new(format!("execution-{label}"), ids).unwrap()
        };
        parts.push(format!(
            "execution={:?};events={:?};audits={:?};checkpoint={:?}",
            store.get_execution(&execution).unwrap(),
            store.events(&execution, 0, 128).unwrap(),
            store.audit_records(&execution, 0, 128).unwrap(),
            store.get_checkpoint(&execution).unwrap()
        ));
    }
    for key in [
        "private-retention-schedule-1",
        "private-retention-log-2",
        "selected-first-1",
        "selected-first-4",
        "selected-restart-1",
        "selected-basic-20",
    ] {
        parts.push(format!(
            "effect={:?}",
            store
                .effect(&IdempotencyKey::new(key, ids).unwrap())
                .unwrap()
        ));
    }
    parts.join("\n")
}

fn copy_private_artifacts(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        assert!(!entry.file_type().unwrap().is_symlink());
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_private_artifacts(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
            let metadata = entry.metadata().unwrap();
            std::fs::set_permissions(&target, metadata.permissions()).unwrap();
            std::fs::File::open(&target)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_accessed(metadata.accessed().unwrap())
                        .set_modified(metadata.modified().unwrap()),
                )
                .unwrap();
        }
    }
}

#[test]
#[ignore = "substantive child phases run only from the bounded parent"]
fn ims_private_recovery_retention_process_worker() {
    use crate::retention_maintenance::provider::RetentionPlanner;
    use mainframe_env_store_api::{ArtifactStore, RetentionTarget};
    let root = PathBuf::from(
        std::env::var("IMS_PRIVATE_RETENTION_PROCESS_ROOT").expect("parent root required"),
    );
    let phase =
        std::env::var("IMS_PRIVATE_RETENTION_PROCESS_PHASE").expect("parent phase required");
    let restored = phase == "restore";
    let db = root.join(if restored {
        "restored.sqlite"
    } else {
        "source.sqlite"
    });
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.artifact_root = root.join(if restored {
        "restored-artifacts"
    } else {
        "source-artifacts"
    });
    config.sqlite_url = format!("sqlite:{}?mode=rwc", db.display());
    let store =
        Arc::new(SqliteStateStore::open(&config.sqlite_url, 64 * 1024 * 1024, 262_144).unwrap());
    store.integrity_check().unwrap();
    if phase == "seed" {
        // Materialize the six immutable blobs referenced by this signed package's
        // manifest through the existing local artifact authority.
        let trust = test_package_trust();
        let package = signed_ims_package(&trust, 1, 1);
        let artifacts = LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024).unwrap();
        for (id, payload) in &package.base.blobs {
            artifacts
                .put_artifact(mainframe_env_store_api::ArtifactRecord {
                    artifact: ArtifactRef::new(id, InvocationLimits::default()).unwrap(),
                    media_type: "application/octet-stream".into(),
                    payload_digest: Sha256::digest(payload).into(),
                    payload: payload.clone(),
                    executable: None,
                })
                .unwrap();
        }
        // Signed selection, real database/undo/checkpoints, named XRST and later
        // writes use the already accepted test composition; no arbitrary row backup.
        exercise_selected_checkpoint_restart(store.clone(), config.clone());
        // The inherited checkpoint fixture retains legacy direct-call receipts which
        // cannot certify ordinary pruning. That pruning is proved independently by
        // the two signed LOG tests; this graph keeps those receipts intact.
        let (_, invocation, _) = exercise_private_retention_log(store.clone(), config, false);
        std::fs::write(
            root.join("deadline.txt"),
            invocation.deadline_tick.to_string(),
        )
        .unwrap();
        std::fs::write(
            root.join("snapshot.txt"),
            private_graph_snapshot(store.as_ref()),
        )
        .unwrap();
    } else {
        assert_eq!(
            private_graph_snapshot(store.as_ref()),
            std::fs::read_to_string(root.join("snapshot.txt")).unwrap()
        );
        if phase == "backup" {
            // Seed process has exited: no active admission or serving authority.
            store.backup_to(&root.join("restored.sqlite")).unwrap();
            copy_private_artifacts(
                &root.join("source-artifacts"),
                &root.join("restored-artifacts"),
            );
            let bytes = std::fs::read(root.join("restored.sqlite")).unwrap();
            std::fs::write(
                root.join("backup.sha256"),
                format!("{:x}", Sha256::digest(bytes)),
            )
            .unwrap();
        } else {
            assert_eq!(phase, "restore");
            let before = store.list_provider_state_prefix("ims-", 4096).unwrap();
            let trust = Arc::new(test_package_trust());
            let server = ProductServer::open_with_package_trust(
                config.clone(),
                store.clone(),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
                trust,
            )
            .unwrap();
            assert!(server.readiness().artifact_store);
            // Verify every copied immutable object through the existing digest/envelope reader.
            let artifacts =
                LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024).unwrap();
            let mut verified = 0;
            for prefix in std::fs::read_dir(config.artifact_root.join("objects")).unwrap() {
                for object in std::fs::read_dir(prefix.unwrap().path()).unwrap() {
                    let id = ArtifactRef::new(
                        format!("sha256:{}", object.unwrap().file_name().to_str().unwrap()),
                        InvocationLimits::default(),
                    )
                    .unwrap();
                    assert!(artifacts.get_artifact(&id).unwrap().is_some());
                    verified += 1;
                }
            }
            assert_eq!(
                verified,
                signed_ims_package(&test_package_trust(), 1, 1)
                    .base
                    .blobs
                    .len()
            );
            let selected = server
                .ims
                .selected_metadata_generation("SIGNED-IMS-APPLICATION")
                .unwrap()
                .unwrap();
            let mut invocation =
                tm_invocation("private-retention-log-run", "private-retention-log");
            invocation.service_class = ServiceClass::Batch;
            invocation.deadline_tick = std::fs::read_to_string(root.join("deadline.txt"))
                .unwrap()
                .parse()
                .unwrap();
            invocation.principal = Principal::new(
                PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                BTreeSet::from([
                    CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap(),
                ]),
                InvocationLimits::default(),
            )
            .unwrap();
            let request = recovery_request(
                &selected.package_identity,
                2,
                "private-retention-log",
                ImsRecoveryCall::Log {
                    code: 0xa0,
                    data: b"retained log".to_vec(),
                },
            );
            let result = server
                .host
                .invoke(
                    &invocation,
                    server.jes_clock.now_tick().unwrap(),
                    false,
                    EffectRequest {
                        sequence: 2,
                        run_unit: invocation.run_unit_id.clone(),
                        idempotency_key: Some(request.mutation.idempotency_key.clone()),
                        deadline_tick: invocation.deadline_tick,
                        request: HostRequest::ImsRecovery(request),
                    },
                )
                .persist_with(|audit| store.record_audit(audit).map_err(store_error));
            assert_eq!(
                result.outcome,
                Ok(HostResult::ImsRecovery(ImsRecoveryResult::Logged {
                    status: "  ".into(),
                    sequence: 1
                }))
            );
            let planner =
                RetentionPlanner::from_existing(store.clone(), private_retention_policy(), None)
                    .unwrap();
            assert_eq!(
                planner
                    .forecast(
                        RetentionTarget::ResolvedEffects,
                        invocation.deadline_tick + 100,
                        0
                    )
                    .unwrap()
                    .eligible_records,
                0
            );
            assert_eq!(
                planner
                    .archive_and_prune(
                        RetentionTarget::ResolvedEffects,
                        invocation.deadline_tick + 100,
                        64
                    )
                    .unwrap()
                    .pruned,
                0
            );
            assert_eq!(
                store.list_provider_state_prefix("ims-", 4096).unwrap(),
                before
            );
            // Read-only authoritative checkpoint observation survives the same backup.
            let mut checkpoint_invocation =
                tm_invocation("selected-checkpoint-run", "selected-checkpoint");
            checkpoint_invocation.service_class = ServiceClass::Batch;
            checkpoint_invocation.principal = invocation.principal;
            let checkpoint = recovery_request(
                &selected.package_identity,
                4,
                "selected-first",
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "CHILD01".into(),
                    user_areas: vec![b"saved".to_vec()],
                },
            );
            assert_eq!(
                server
                    .ims
                    .observe_application_recovery(&checkpoint_invocation, &checkpoint)
                    .unwrap(),
                Some(ImsRecoveryResult::Checkpointed {
                    status: "  ".into(),
                    id: "CHILD01".into(),
                    sequence: 2
                })
            );
        }
    }
    println!(
        "private-retention substantive phase={phase} pid={}",
        std::process::id()
    );
}

#[test]
fn ims_private_recovery_retention_sqlite_separate_process_backup_restore() {
    let root = std::env::temp_dir().join(format!(
        "ims-private-retention-process-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let name = "product::tests::ims_package_tests::application_recovery::ims_private_recovery_retention_process_worker";
    for phase in ["seed", "backup", "restore"] {
        if phase == "restore" {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("restored.sqlite")).unwrap())
                ),
                std::fs::read_to_string(root.join("backup.sha256")).unwrap()
            );
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--ignored", "--exact", "--nocapture"])
            .env("IMS_PRIVATE_RETENTION_PROCESS_ROOT", &root)
            .env("IMS_PRIVATE_RETENTION_PROCESS_PHASE", phase)
            .output()
            .unwrap();
        println!(
            "phase={phase}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "phase={phase}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("test result: ok. 1 passed; 0 failed"),
            "phase={phase}: exact child must execute one substantive test"
        );
        assert!(
            stdout.contains(&format!("private-retention substantive phase={phase} pid=")),
            "phase={phase}: child must reach its phase assertions"
        );
    }
    assert!(
        !std::fs::read_to_string(root.join("backup.sha256"))
            .unwrap()
            .is_empty()
    );
    std::fs::remove_dir_all(root).unwrap();
}

struct RecoveryMachine {
    effects: std::collections::VecDeque<EffectRequest>,
    results: Vec<EffectResult>,
}

impl Machine for RecoveryMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        if let MachineResume::HostResult(result) = resume {
            self.results.push(result);
        }
        if let Some(effect) = self.effects.pop_front() {
            MachineDrive::HostCall(effect)
        } else {
            MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new("test@1", vec![], InvocationLimits::default()).unwrap(),
            })
        }
    }
}

pub(super) fn recovery_db(operation: ImsOperation, sequence: u64, key_prefix: &str) -> ImsRequest {
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "AUTHPSB".into()),
        pcb: 1,
        segments: vec![],
        data: vec![],
        qualifiers: vec![],
        checkpoint_id: None,
        max_segments: 16,
        system: None,
        q_class: None,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("{key_prefix}-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
    }
}

pub(super) fn recovery_request(
    identity: &str,
    sequence: u64,
    key_prefix: &str,
    call: ImsRecoveryCall,
) -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        application: "SIGNED-IMS-APPLICATION".into(),
        package_identity: identity.into(),
        psb: "AUTHPSB".into(),
        database: "AUTHDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call,
        mutation: Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("{key_prefix}-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    }
}

pub(super) fn run_recovery_machine(
    server: &ProductServer,
    store: Arc<dyn PlatformStore>,
    invocation: &Invocation,
    requests: Vec<HostRequest>,
) -> Vec<EffectResult> {
    let effects = requests
        .into_iter()
        .map(|request| {
            let mutation = request.mutation().unwrap();
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: mutation.sequence,
                idempotency_key: Some(mutation.idempotency_key.clone()),
                deadline_tick: invocation.deadline_tick,
                request,
            }
        })
        .collect();
    let mut machine = RecoveryMachine {
        effects,
        results: vec![],
    };
    let now_tick = server.jes_clock.now_tick().unwrap();
    let coordinator =
        ExecutionCoordinator::durable(server.host.clone(), store, CoordinatorLimits::default());
    let outcome = coordinator.execute(
        &mut machine,
        invocation,
        ExecutionControl {
            now_tick,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    machine.results
}

fn exercise_selected_checkpoint_restart(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    use mainframe_env_host_api::{ImsQualifier, ImsRestartSelection};
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let installed = server
        .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let ids = InvocationLimits::default();
    let mut invocation = tm_invocation("selected-checkpoint-run", "selected-checkpoint");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &invocation,
            &recovery_db(ImsOperation::Schedule, 10, "selected-schedule"),
        )
        .unwrap();
    let image = mainframe_env_ims::ImsGenericLoadImage {
        database: "AUTHDB".into(),
        records: [
            ("ROOT", None, &b"00000001ROOTDATA"[..]),
            ("CHILD", Some(0), &b"CHILD001CHILDONE"[..]),
            ("CHILD", Some(0), &b"CHILD002CHILDTWO"[..]),
            ("ROOT", None, &b"00000002ROOTDATA"[..]),
        ]
        .into_iter()
        .map(
            |(segment, parent, data)| mainframe_env_ims::ImsGenericLoadRecord {
                segment: segment.into(),
                parent,
                data: data.to_vec(),
            },
        )
        .collect(),
    };
    let mut load = recovery_db(ImsOperation::Load, 2, "selected-first");
    load.data = serde_json::to_vec(&image).unwrap();
    let mut gu = recovery_db(ImsOperation::GetHoldUnique, 3, "selected-first");
    gu.segments = vec!["ROOT".into(), "CHILD".into()];
    gu.qualifiers = vec![
        ImsQualifier {
            segment: "ROOT".into(),
            field: "ROOTKEY".into(),
            value: b"00000001".to_vec(),
        },
        ImsQualifier {
            segment: "CHILD".into(),
            field: "CHILDKEY".into(),
            value: b"CHILD001".to_vec(),
        },
    ];
    let checkpoint = recovery_request(
        &installed.identity,
        4,
        "selected-first",
        ImsRecoveryCall::SymbolicCheckpoint {
            id: "CHILD01".into(),
            user_areas: vec![b"saved".to_vec()],
        },
    );
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &invocation,
        vec![
            HostRequest::ImsRecovery(recovery_request(
                &installed.identity,
                1,
                "selected-first",
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Normal,
                    area_lengths: vec![5],
                },
            )),
            HostRequest::Ims(load),
            HostRequest::Ims(gu.clone()),
            HostRequest::ImsRecovery(checkpoint.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 5, "selected-first")),
        ],
    );
    assert_eq!(results.len(), 5);
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: None,
            user_areas: vec![],
            pcb_statuses: vec![]
        }))
    );
    assert_eq!(
        results[3].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "CHILD01".into(),
            sequence: 2
        }))
    );
    let Ok(HostResult::Ims(ref reset)) = results[4].outcome else {
        panic!("{:?}", results[4]);
    };
    assert_eq!(reset.segments[0].data, b"00000001ROOTDATA");
    let mut restarted = invocation.clone();
    restarted.execution_id = ExecutionId::new("selected-restart-execution", ids).unwrap();
    let restart = recovery_request(
        &installed.identity,
        1,
        "selected-restart",
        ImsRecoveryCall::Restart {
            selection: ImsRestartSelection::Checkpoint("CHILD01".into()),
            area_lengths: vec![5],
        },
    );
    gu.mutation = recovery_db(ImsOperation::GetUnique, 3, "selected-restart").mutation;
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &restarted,
        vec![
            HostRequest::ImsRecovery(restart.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 2, "selected-restart")),
            HostRequest::Ims(gu),
        ],
    );
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("CHILD01".into()),
            user_areas: vec![b"saved".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        }))
    );
    let Ok(HostResult::Ims(ref next)) = results[1].outcome else {
        panic!()
    };
    assert_eq!(next.segments[0].data, b"CHILD002CHILDTWO");
    let Ok(HostResult::Ims(ref unique)) = results[2].outcome else {
        panic!()
    };
    assert_eq!(unique.segments[0].data, b"CHILD001CHILDONE");
    let before = store
        .list_provider_state("ims-v1-session-index", 64)
        .unwrap();
    assert_eq!(
        server
            .ims
            .observe_application_recovery(&restarted, &restart)
            .unwrap(),
        Some(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("CHILD01".into()),
            user_areas: vec![b"saved".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        })
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap(),
        before
    );
    for request in [checkpoint, restart] {
        let effect = store
            .effect(&request.mutation.idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        assert_eq!(
            effect.request_digest,
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsRecovery(request))
                .unwrap()
        );
    }
    assert_eq!(
        store
            .audit_records(&restarted.execution_id, 0, 16)
            .unwrap()
            .len(),
        3
    );

    // Exercise basic CHKP through the same signed, canonical public route.
    // Interactive database work retains real undo until the batch CHKP commits it.
    let mut basic = invocation.clone();
    basic.execution_id = ExecutionId::new("selected-basic-execution", ids).unwrap();
    let mut writer = basic.clone();
    writer.service_class = ServiceClass::Interactive;
    let mut insert = recovery_db(ImsOperation::Insert, 10, "selected-basic-write");
    insert.segments = vec!["ROOT".into()];
    insert.data = b"00000003ROOTDATA".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &writer, &insert)
            .unwrap()
            .status,
        "  "
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap()
            .len(),
        1
    );
    let basic_request = recovery_request(
        &installed.identity,
        20,
        "selected-basic",
        ImsRecoveryCall::BasicCheckpoint {
            id: "BASIC01".into(),
        },
    );
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &basic,
        vec![
            HostRequest::ImsRecovery(basic_request.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 21, "selected-basic")),
        ],
    );
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "BASIC01".into(),
            sequence: 4,
        }))
    );
    assert!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap()
            .is_empty()
    );
    assert!(
        matches!(results[1].outcome, Ok(HostResult::Ims(ref r)) if r.segments[0].data == b"00000001ROOTDATA")
    );
    let mut later = recovery_db(ImsOperation::Insert, 30, "selected-basic-write");
    later.segments = vec!["ROOT".into()];
    later.data = b"00000004ROOTDATA".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &writer, &later)
            .unwrap()
            .status,
        "  "
    );
    let pending = store
        .list_provider_state("ims-v1-generic-unit-of-work", 64)
        .unwrap();
    assert_eq!(
        server
            .ims
            .observe_application_recovery(&basic, &basic_request)
            .unwrap(),
        Some(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "BASIC01".into(),
            sequence: 4,
        })
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap(),
        pending
    );
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &writer,
            &recovery_db(ImsOperation::Rollback, 31, "selected-basic-write"),
        )
        .unwrap();
    let mut gu = recovery_db(ImsOperation::GetUnique, 32, "selected-basic");
    gu.segments = vec!["ROOT".into()];
    gu.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "ROOTKEY".into(),
        value: b"00000003".to_vec(),
    }];
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &basic, &gu)
            .unwrap()
            .segments[0]
            .data,
        b"00000003ROOTDATA"
    );
}

#[test]
fn signed_memory_checkpoint_restart_runs_real_hierarchical_gu_gn_through_coordinator() {
    exercise_selected_checkpoint_restart(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_checkpoint_restart_runs_real_hierarchical_gu_gn_through_coordinator() {
    let path = std::env::temp_dir().join(format!(
        "ims-selected-checkpoint-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise_selected_checkpoint_restart(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    std::fs::remove_file(path).unwrap();
}
