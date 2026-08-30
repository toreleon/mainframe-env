use crate::{DefaultProgramRouter, ServerConfig, default_program_router};
use axum::http::StatusCode;
use base64::Engine;
use mainframe_env_application::ApplicationInstaller;
use mainframe_env_batch::{BatchService, JclBundle};
use mainframe_env_cics::{
    BmsMapDefinition, CicsService, CicsTerminalSnapshot, CicsTraceEntry, cics_provider,
};
use mainframe_env_dataset::{DatasetService, dataset_providers};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, ExecutionOutcome, IdempotencyKey,
    Invocation, InvocationLimits, Machine, Principal, PrincipalId, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsOperation, ClockRequest, DatasetAttributes,
    DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostProvider, HostRequest, HostResult, MemberName, Mutation,
    RecordFormat, RegistrySnapshot, ResourceName, ScopedHostService, SecretRef, SecurityDecision,
    SessionId,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_racf::{MemorySecretResolver, RacfService, racf_providers};
use mainframe_env_store::{LocalArtifactStore, MemoryStore};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, PlatformStore, ProviderStateRecord, ProviderStateStore,
    ProviderStateWrite, StoreError, WorkRecord, WorkState,
};
use mainframe_env_zosmf::{
    Authentication, GatewayProblem, GatewayRequest, GatewayResponse, ZosmfBackend, ZosmfLimits,
};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_rustls::TlsAcceptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductMetrics {
    pub requests: u64,
    pub failures: u64,
    pub active: usize,
    pub sessions: usize,
    pub console_messages: usize,
    pub outbox_pending: usize,
    pub outbox_delivered: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineProgramDefinition {
    pub name: String,
    pub artifact: ArtifactRef,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineApplicationDefinition {
    pub programs: Vec<OnlineProgramDefinition>,
    pub transactions: BTreeMap<String, String>,
    pub maps: Vec<BmsMapDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineInstallReceipt {
    pub programs: usize,
    pub transactions: usize,
    pub maps: usize,
    pub identity: String,
    pub replayed: bool,
}

struct OnlineMachineContinuation {
    program: String,
    checkpoint: BoundedPayload,
    version: u64,
}

struct AuthSession {
    user: String,
    version: u64,
}

struct ConsoleMessage {
    key: String,
    console: String,
    text: Vec<u8>,
}

pub struct ProductServer {
    config: ServerConfig,
    store: Arc<dyn PlatformStore>,
    secrets: Arc<MemorySecretResolver>,
    racf: Arc<RacfService>,
    cics: Arc<CicsService>,
    dataset: Arc<DatasetService>,
    batch: Arc<BatchService>,
    artifacts: LocalArtifactStore,
    host: Arc<ScopedHostService>,
    applications: ApplicationInstaller,
    online_programs: Mutex<BTreeMap<String, ArtifactRef>>,
    online_transactions: Mutex<BTreeMap<String, String>>,
    online_traces: Mutex<BTreeMap<String, Vec<CicsTraceEntry>>>,
    sessions: Mutex<BTreeMap<String, AuthSession>>,
    console: Mutex<Vec<ConsoleMessage>>,
    sequence: AtomicU64,
    accepting: AtomicBool,
    requests: AtomicU64,
    failures: AtomicU64,
    active: AtomicUsize,
    outbox_delivered: AtomicU64,
}

impl ProductServer {
    pub fn open(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
    ) -> Result<Arc<Self>, HostProblem> {
        config.validate()?;
        let artifacts = LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024)
            .map_err(store_error)?;
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let racf = RacfService::open(provider_store.clone(), secrets.clone(), Default::default())?;
        let dataset = DatasetService::open(provider_store.clone(), Default::default())?;
        let inner_program: Arc<dyn HostProvider> = program.clone();
        let inner = scoped_host(&racf, &dataset, inner_program, false, None)?;
        let cics = CicsService::open(inner, provider_store.clone(), Default::default())?;
        let program_provider: Arc<dyn HostProvider> = program.clone();
        let host = scoped_host(
            &racf,
            &dataset,
            program_provider,
            true,
            Some(cics_provider(cics.clone(), InvocationLimits::default())),
        )?;
        program.bind_runtime(host.clone(), store.clone(), &config.artifact_root)?;
        let batch = BatchService::open(
            host.clone(),
            provider_store,
            Default::default(),
            Default::default(),
        )?;
        let mut sessions = BTreeMap::new();
        for row in store
            .list_provider_state("auth-session", 65536)
            .map_err(store_error)?
        {
            let user =
                String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
            sessions.insert(
                row.key,
                AuthSession {
                    user,
                    version: row.version,
                },
            );
        }
        let mut console = Vec::new();
        for row in store
            .list_provider_state("console-log", 65536)
            .map_err(store_error)?
        {
            let separator = row
                .payload
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(HostProblem::InfrastructureFailure)?;
            let (name, tail) = row.payload.split_at(separator);
            let text = tail.get(1..).ok_or(HostProblem::InfrastructureFailure)?;
            console.push(ConsoleMessage {
                key: row.key,
                console: String::from_utf8(name.to_vec())
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                text: text.to_vec(),
            });
        }
        let mut online_programs = BTreeMap::new();
        for row in store
            .list_provider_state("online-program", 4096)
            .map_err(store_error)?
        {
            let artifact = ArtifactRef::new(
                String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?;
            if artifacts
                .get_artifact(&artifact)
                .map_err(store_error)?
                .is_none()
                || online_programs.insert(row.key, artifact).is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut online_transactions = BTreeMap::new();
        for row in store
            .list_provider_state("online-transaction", 4096)
            .map_err(store_error)?
        {
            let program =
                String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
            if !online_programs.contains_key(&program)
                || online_transactions.insert(row.key, program).is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        cics.register_programs(&online_programs.keys().cloned().collect())?;
        let product = Arc::new(Self {
            config,
            store,
            secrets,
            racf,
            cics,
            dataset,
            batch,
            artifacts,
            host,
            applications: ApplicationInstaller::new("0.1.1"),
            online_programs: Mutex::new(online_programs),
            online_transactions: Mutex::new(online_transactions),
            online_traces: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(sessions),
            console: Mutex::new(console),
            sequence: AtomicU64::new(1),
            accepting: AtomicBool::new(true),
            requests: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            active: AtomicUsize::new(0),
            outbox_delivered: AtomicU64::new(0),
        });
        product.recover_local_wakeups()?;
        Ok(product)
    }

    pub fn memory(config: ServerConfig) -> Result<Arc<Self>, HostProblem> {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let program = default_program_router();
        Self::open(config, store, secrets, program)
    }

    #[must_use]
    pub fn application_installer(&self) -> ApplicationInstaller {
        self.applications.clone()
    }

    #[must_use]
    pub fn cics_service(&self) -> Arc<CicsService> {
        self.cics.clone()
    }

    #[must_use]
    pub fn batch_service(&self) -> Arc<BatchService> {
        self.batch.clone()
    }

    #[must_use]
    pub fn dataset_service(&self) -> Arc<DatasetService> {
        self.dataset.clone()
    }

    #[must_use]
    pub fn racf_service(&self) -> Arc<RacfService> {
        self.racf.clone()
    }

    pub fn online_trace(&self, session: &str) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        Ok(self
            .online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(session)
            .cloned()
            .unwrap_or_default())
    }

    pub fn online_operation_count(&self, operation: CicsOperation) -> Result<usize, HostProblem> {
        Ok(self
            .online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .values()
            .flatten()
            .filter(|entry| entry.operation == operation)
            .count())
    }

    pub fn install_online_application(
        &self,
        definition: OnlineApplicationDefinition,
    ) -> Result<OnlineInstallReceipt, HostProblem> {
        if definition.programs.is_empty()
            || definition.transactions.is_empty()
            || definition.maps.is_empty()
            || definition.programs.len() > 4096
            || definition.transactions.len() > 4096
        {
            return Err(HostProblem::Malformed);
        }
        let mut programs = BTreeMap::new();
        let mut identity = Sha256::new();
        for definition in &definition.programs {
            let name = normalize_online_name(&definition.name, 128)?;
            let digest: [u8; 32] = Sha256::digest(&definition.payload).into();
            if definition.artifact.as_str() != format!("sha256:{}", hex_digest(&digest))
                || programs
                    .insert(name.clone(), definition.artifact.clone())
                    .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            self.artifacts
                .put_artifact(ArtifactRecord {
                    artifact: definition.artifact.clone(),
                    media_type: "application/vnd.mainframe-env.core-mir".into(),
                    payload_digest: digest,
                    payload: definition.payload.clone(),
                })
                .map_err(store_error)?;
            digest_online_field(&mut identity, name.as_bytes());
            digest_online_field(&mut identity, definition.artifact.as_str().as_bytes());
        }
        let mut transactions = BTreeMap::new();
        for (transaction, program) in &definition.transactions {
            let transaction = normalize_online_name(transaction, 16)?;
            let program = normalize_online_name(program, 128)?;
            if !programs.contains_key(&program)
                || transactions
                    .insert(transaction.clone(), program.clone())
                    .is_some()
            {
                return Err(HostProblem::NotFound);
            }
            digest_online_field(&mut identity, transaction.as_bytes());
            digest_online_field(&mut identity, program.as_bytes());
        }
        let program_names = programs.keys().cloned().collect::<BTreeSet<_>>();
        self.cics.register_programs(&program_names)?;
        for map in &definition.maps {
            self.cics.register_map(map.clone())?;
            digest_online_field(&mut identity, map.mapset.as_bytes());
            digest_online_field(&mut identity, map.map.as_bytes());
            digest_online_field(&mut identity, &(map.fields.len() as u64).to_be_bytes());
        }
        let mut current_programs = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut current_transactions = self
            .online_transactions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut writes = Vec::new();
        for (name, artifact) in &programs {
            if let Some(existing) = current_programs.get(name) {
                if existing != artifact {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                writes.push(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "online-program".into(),
                        key: name.clone(),
                        version: 1,
                        payload: artifact.as_str().as_bytes().to_vec(),
                    },
                    expected_version: None,
                });
            }
        }
        for (transaction, program) in &transactions {
            if let Some(existing) = current_transactions.get(transaction) {
                if existing != program {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                writes.push(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "online-transaction".into(),
                        key: transaction.clone(),
                        version: 1,
                        payload: program.as_bytes().to_vec(),
                    },
                    expected_version: None,
                });
            }
        }
        let replayed = writes.is_empty();
        if !replayed {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        current_programs.extend(programs);
        current_transactions.extend(transactions);
        Ok(OnlineInstallReceipt {
            programs: definition.programs.len(),
            transactions: definition.transactions.len(),
            maps: definition.maps.len(),
            identity: format!("sha256:{:x}", identity.finalize()),
            replayed,
        })
    }

    fn online_machine_continuation(
        &self,
        session: &SessionId,
    ) -> Result<Option<OnlineMachineContinuation>, HostProblem> {
        self.store
            .get_provider_state("online-machine-continuation", session.as_str())
            .map_err(store_error)?
            .map(|record| decode_online_machine_continuation(&record))
            .transpose()
    }

    fn persist_online_machine_continuation(
        &self,
        session: &SessionId,
        program: &str,
        checkpoint: &BoundedPayload,
        current_version: Option<u64>,
    ) -> Result<u64, HostProblem> {
        let version = current_version
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "online-machine-continuation".into(),
                    key: session.as_str().into(),
                    version,
                    payload: encode_online_machine_continuation(program, checkpoint)?,
                },
                current_version,
            )
            .map_err(store_error)?;
        Ok(version)
    }

    fn clear_online_machine_continuation(
        &self,
        session: &SessionId,
        version: Option<u64>,
    ) -> Result<(), HostProblem> {
        if let Some(version) = version {
            self.store
                .delete_provider_state("online-machine-continuation", session.as_str(), version)
                .map_err(store_error)?;
        }
        Ok(())
    }

    fn finish_online_machine_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let trace = self.cics.terminal_run_trace(session, principal, now_tick)?;
        self.online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .entry(session.as_str().into())
            .or_default()
            .extend(trace);
        self.cics
            .complete_terminal_run(session, principal, now_tick)
    }

    fn run_online_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let context = self.cics.terminal_execution(session, principal, now_tick)?;
        let mut invocation = context.invocation;
        invocation.bindings.insert(
            "cics.commarea".into(),
            BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                context.commarea,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        invocation.bindings.insert(
            "cics.aid".into(),
            BoundedPayload::new(
                "mainframe-env.cics.aid@1",
                vec![context.aid],
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        invocation.bindings.insert(
            "cics.transaction".into(),
            BoundedPayload::new(
                "mainframe-env.cics.transaction@1",
                context.transaction.into_bytes(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        let saved = self.online_machine_continuation(session)?;
        let mut saved_version = saved.as_ref().map(|saved| saved.version);
        let mut saved_checkpoint = saved.as_ref().map(|saved| saved.checkpoint.clone());
        let mut current = normalize_online_name(
            saved
                .as_ref()
                .map_or(program, |saved| saved.program.as_str()),
            128,
        )?;
        let root_idempotency = invocation.idempotency_key.as_str().to_string();
        for frame in 0..invocation.limits.max_frames {
            let artifact = self
                .online_programs
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .get(&current)
                .cloned()
                .ok_or(HostProblem::NotFound)?;
            let record = self
                .artifacts
                .get_artifact(&artifact)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            invocation.selector =
                Selector::new(format!("program:{current}"), InvocationLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            invocation.artifact = artifact;
            invocation.idempotency_key = IdempotencyKey::new(
                format!("{root_idempotency}:{frame}:{current}"),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            let mut machine = ReferenceMachine::from_binary(
                &record.payload,
                invocation.clone(),
                CodecLimits::default(),
            )
            .map_err(|_| HostProblem::ProviderFailure)?;
            if let Some(checkpoint) = saved_checkpoint.take() {
                machine
                    .restore_checkpoint(&checkpoint)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            }
            let coordinator = ExecutionCoordinator::with_host(
                self.host.clone(),
                CoordinatorLimits {
                    max_quanta: 100,
                    ..CoordinatorLimits::default()
                },
            );
            match coordinator.execute(&mut machine, &invocation, ExecutionControl::default()) {
                ExecutionOutcome::Completed(_) => {
                    self.clear_online_machine_continuation(session, saved_version)?;
                    self.finish_online_machine_run(session, principal, now_tick)?;
                    return Ok(());
                }
                ExecutionOutcome::Suspended(_) => {
                    let checkpoint = machine.checkpoint().ok_or(HostProblem::ProviderFailure)?;
                    let _ = self.persist_online_machine_continuation(
                        session,
                        &current,
                        &checkpoint,
                        saved_version,
                    )?;
                    self.finish_online_machine_run(session, principal, now_tick)?;
                    return Ok(());
                }
                ExecutionOutcome::Transfer(transfer) if transfer.replace_frame => {
                    self.clear_online_machine_continuation(session, saved_version)?;
                    saved_version = None;
                    self.online_traces
                        .lock()
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        .entry(session.as_str().into())
                        .or_default()
                        .push(CicsTraceEntry {
                            operation: mainframe_env_host_api::CicsOperation::Retrieve,
                            outcome: format!("TRANSFER {}", transfer.selector.as_str()),
                            response: 0,
                            response2: 0,
                            payload_bytes: 0,
                        });
                    current = normalize_online_name(transfer.selector.as_str(), 128)?;
                    invocation.bindings.insert(
                        "cics.commarea".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.commarea@1",
                            transfer.payload.bytes().to_vec(),
                            InvocationLimits::default(),
                        )
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                    );
                }
                ExecutionOutcome::Condition(condition) => {
                    return Err(HostProblem::Condition {
                        name: condition.name,
                        response: condition.response,
                        response2: condition.response2,
                    });
                }
                ExecutionOutcome::Abend(abend) => {
                    return Err(HostProblem::Condition {
                        name: abend.code,
                        response: -1,
                        response2: 0,
                    });
                }
                ExecutionOutcome::TimedOut => return Err(HostProblem::TimedOut),
                ExecutionOutcome::Cancelled => return Err(HostProblem::Cancelled),
                ExecutionOutcome::ResourceExhausted(problem) => {
                    return Err(HostProblem::Condition {
                        name: format!(
                            "{} at {}",
                            problem.public_message,
                            machine.position_summary()
                        ),
                        response: -3,
                        response2: 0,
                    });
                }
                ExecutionOutcome::ProviderFailure(problem) => {
                    return Err(HostProblem::Condition {
                        name: problem.public_message,
                        response: -4,
                        response2: 0,
                    });
                }
                ExecutionOutcome::InfrastructureFailure(_) => {
                    return Err(HostProblem::InfrastructureFailure);
                }
                ExecutionOutcome::Rejected(problem) => {
                    return Err(HostProblem::Condition {
                        name: format!(
                            "{} at {}",
                            problem.public_message,
                            machine.position_summary()
                        ),
                        response: -2,
                        response2: 0,
                    });
                }
                ExecutionOutcome::Invoke(_) | ExecutionOutcome::Transfer(_) => {
                    return Err(HostProblem::Unsupported);
                }
            }
        }
        Err(HostProblem::ResourceExhausted)
    }

    pub fn bootstrap_user(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        self.bootstrap_identity(user, secret)?;
        for (class, pattern, access) in [
            (
                "DATASET",
                format!("{}.**", user.to_ascii_uppercase()),
                AccessIntent::Alter,
            ),
            ("JESJOBS", "JOB.**".into(), AccessIntent::Alter),
            ("FACILITY", "CONSOLE.**".into(), AccessIntent::Alter),
            ("TCICSTRN", "CICS.**".into(), AccessIntent::Execute),
        ] {
            self.racf.define_profile(class, &pattern, user, None)?;
            self.racf.permit(class, &pattern, user, access)?;
        }
        Ok(())
    }

    pub fn bootstrap_identity(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        let reference = format!("bootstrap:{user}");
        self.secrets.insert(&reference, secret.to_vec());
        let result = self.racf.add_user(
            user,
            &SecretRef::new(reference.clone(), HostLimits::default())?,
        );
        self.secrets.remove(&reference);
        result
    }

    pub fn router(self: &Arc<Self>) -> axum::Router {
        mainframe_env_zosmf::router(
            self.clone(),
            ZosmfLimits {
                max_body_bytes: self.config.max_body_bytes,
                max_concurrency: self.config.max_concurrency,
                timeout: Duration::from_millis(self.config.timeout_millis),
                max_page_items: 1000,
            },
        )
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.accepting.load(Ordering::SeqCst)
            && self.store.get_provider_state("jes-meta", "next-id").is_ok()
            && self.host.capability_ready("host.cics.execute")
            && self.artifacts.is_ready()
    }

    #[must_use]
    pub fn metrics(&self) -> ProductMetrics {
        ProductMetrics {
            requests: self.requests.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            active: self.active.load(Ordering::Relaxed),
            sessions: self.sessions.lock().map_or(0, |sessions| sessions.len()),
            console_messages: self.console.lock().map_or(0, |messages| messages.len()),
            outbox_pending: self
                .store
                .pending_notifications(4096)
                .map_or(0, |rows| rows.len()),
            outbox_delivered: self.outbox_delivered.load(Ordering::Relaxed),
        }
    }

    pub async fn graceful_shutdown(&self) -> bool {
        self.accepting.store(false, Ordering::SeqCst);
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.config.shutdown_millis);
        while self.active.load(Ordering::SeqCst) != 0 {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        true
    }

    pub fn tls_acceptor(
        certificates: Vec<Vec<u8>>,
        private_key: Vec<u8>,
    ) -> Result<TlsAcceptor, HostProblem> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| HostProblem::Malformed)?
            .with_no_client_auth()
            .with_single_cert(
                certificates.into_iter().map(CertificateDer::from).collect(),
                PrivateKeyDer::try_from(private_key).map_err(|_| HostProblem::Malformed)?,
            )
            .map_err(|_| HostProblem::Malformed)?;
        Ok(TlsAcceptor::from(Arc::new(config)))
    }

    fn handle(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, GatewayProblem> {
        if !self.accepting.load(Ordering::SeqCst) {
            return Err(gateway_problem(HostProblem::ResourceExhausted));
        }
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _guard = ActiveGuard(&self.active);
        let result = self.handle_inner(authentication, request);
        if self.recover_local_wakeups().is_err() && result.is_ok() {
            return Err(gateway_problem(HostProblem::InfrastructureFailure));
        }
        if result.is_err() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    fn recover_local_wakeups(&self) -> Result<(), HostProblem> {
        for notification in self
            .store
            .pending_notifications(4096)
            .map_err(store_error)?
        {
            self.store
                .mark_notification_delivered(&notification.notification_id, notification.version)
                .map_err(store_error)?;
            self.outbox_delivered.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn handle_inner(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, GatewayProblem> {
        if matches!(request, GatewayRequest::Info) {
            return Ok(GatewayResponse::json(
                StatusCode::OK,
                json!({
                    "zos_version":"mainframe-env 0.1",
                    "zosmf_port":"10443",
                    "zosmf_version":"mainframe-env.zosmf@1",
                    "api_version":"1",
                    "product_version":env!("CARGO_PKG_VERSION"),
                    "ready":self.ready(),
                    "capabilities":["datasets","jobs","security","console"]
                }),
            ));
        }
        if matches!(request, GatewayRequest::Authenticate) {
            let Authentication::Basic {
                ref user,
                ref secret,
            } = authentication
            else {
                return Err(unauthenticated());
            };
            self.verify(user, secret).map_err(|_| unauthenticated())?;
            let token = self.create_session(user).map_err(gateway_problem)?;
            return Ok(GatewayResponse::json(
                StatusCode::OK,
                json!({"user":user.to_ascii_uppercase(),"token":token}),
            ));
        }
        if matches!(request, GatewayRequest::Logout) {
            let Authentication::Bearer(token) = &authentication else {
                return Err(unauthenticated());
            };
            self.logout_token(token).map_err(gateway_problem)?;
            return Ok(GatewayResponse::empty(StatusCode::NO_CONTENT));
        }
        let principal = self.principal(authentication).map_err(|problem| {
            if problem == HostProblem::Unauthorized {
                unauthenticated()
            } else {
                gateway_problem(problem)
            }
        })?;
        match request {
            GatewayRequest::Logout => unreachable!("logout handled before principal resolution"),
            GatewayRequest::DatasetList {
                pattern,
                start,
                attributes,
                max,
            } => {
                let result = self.dataset_call(
                    &principal,
                    DatasetRequest::List {
                        pattern,
                        start: start
                            .map(|value| {
                                DatasetName::new(value, 128).map_err(|_| HostProblem::Malformed)
                            })
                            .transpose()
                            .map_err(gateway_problem)?,
                        max_items: u32::try_from(max)
                            .map_err(|_| gateway_problem(HostProblem::ResourceExhausted))?,
                    },
                )?;
                let DatasetResult::Listed { names, more } = result else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                let mut items = Vec::with_capacity(names.len());
                for name in names {
                    let mut item = json!({"dsname":name.as_str()});
                    if attributes {
                        let result = self.dataset_call(
                            &principal,
                            DatasetRequest::Attributes {
                                dataset: name.clone(),
                            },
                        )?;
                        let DatasetResult::Attributes { attributes, .. } = result else {
                            return Err(gateway_problem(HostProblem::ProviderFailure));
                        };
                        item["dsorg"] = json!(match attributes.organization {
                            DatasetOrganization::Sequential => "PS",
                            DatasetOrganization::Partitioned => "PO",
                            DatasetOrganization::KeySequenced
                            | DatasetOrganization::EntrySequenced
                            | DatasetOrganization::Relative => "VS",
                        });
                        item["recfm"] = json!(match attributes.record_format {
                            RecordFormat::Fixed => "F",
                            RecordFormat::FixedBlocked => "FB",
                            RecordFormat::Variable => "V",
                            RecordFormat::VariableBlocked => "VB",
                            RecordFormat::Undefined => "U",
                            RecordFormat::Line => "LINE",
                        });
                        item["lrecl"] = json!(attributes.logical_record_length);
                    }
                    items.push(item);
                }
                let count = items.len();
                let mut response = GatewayResponse::json(
                    StatusCode::OK,
                    json!({
                        "items":items,
                        "returnedRows":count,
                        "moreRows":more
                    }),
                );
                response
                    .headers
                    .insert("X-IBM-Response-Rows".into(), count.to_string());
                Ok(response)
            }
            GatewayRequest::DatasetRead { dataset, member } => {
                let result = self.dataset_call(
                    &principal,
                    DatasetRequest::Read {
                        dataset: dataset_name(&dataset)?,
                        member: member_name(member)?,
                        key: None,
                        max_records: 4096,
                    },
                )?;
                let DatasetResult::Records { records, .. } = result else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                Ok(GatewayResponse::bytes(
                    StatusCode::OK,
                    join_records(records),
                ))
            }
            GatewayRequest::DatasetWrite {
                dataset,
                member,
                bytes,
            } => {
                let name = dataset_name(&dataset)?;
                let (attributes, version) = match self.dataset_call(
                    &principal,
                    DatasetRequest::Attributes {
                        dataset: name.clone(),
                    },
                )? {
                    DatasetResult::Attributes {
                        attributes,
                        version,
                    } => (attributes, version),
                    _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
                };
                let records = records_for_write(&bytes, &attributes).map_err(gateway_problem)?;
                let member = member_name(member)?;
                let created_member = if member.is_some()
                    && attributes.organization == DatasetOrganization::Partitioned
                {
                    match self.dataset_call(
                        &principal,
                        DatasetRequest::ListMembers {
                            dataset: name.clone(),
                            start: None,
                            max_items: 4096,
                        },
                    )? {
                        DatasetResult::Members { names, .. } => !names.iter().any(|value| {
                            Some(value.as_str()) == member.as_ref().map(MemberName::as_str)
                        }),
                        _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
                    }
                } else {
                    false
                };
                self.dataset_call(
                    &principal,
                    DatasetRequest::Write {
                        dataset: name,
                        member,
                        records,
                        expected_version: Some(version),
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::empty(if created_member {
                    StatusCode::CREATED
                } else {
                    StatusCode::NO_CONTENT
                }))
            }
            GatewayRequest::DatasetCreate {
                dataset,
                attributes,
            } => {
                self.dataset_call(
                    &principal,
                    DatasetRequest::Create {
                        dataset: dataset_name(&dataset)?,
                        attributes: dataset_attributes(&attributes).map_err(gateway_problem)?,
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::empty(StatusCode::CREATED))
            }
            GatewayRequest::DatasetDelete { dataset, member } => {
                self.dataset_call(
                    &principal,
                    DatasetRequest::Delete {
                        dataset: dataset_name(&dataset)?,
                        member: member_name(member)?,
                        expected_version: None,
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::empty(StatusCode::NO_CONTENT))
            }
            GatewayRequest::MemberList {
                dataset,
                start,
                max,
            } => {
                let name = dataset_name(&dataset)?;
                let start = member_name(start)?;
                let DatasetResult::Members { names: items, more } = self.dataset_call(
                    &principal,
                    DatasetRequest::ListMembers {
                        dataset: name,
                        start,
                        max_items: u32::try_from(max)
                            .map_err(|_| gateway_problem(HostProblem::ResourceExhausted))?,
                    },
                )?
                else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                let count = items.len();
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"items":items.into_iter().map(|member|json!({"member":member.as_str()})).collect::<Vec<_>>(),"returnedRows":count,"moreRows":more}),
                ))
            }
            GatewayRequest::DatasetSearch {
                dataset,
                search,
                max,
            } => {
                let result = self.dataset_call(
                    &principal,
                    DatasetRequest::Read {
                        dataset: dataset_name(&dataset)?,
                        member: None,
                        key: None,
                        max_records: u32::try_from(max).unwrap_or(u32::MAX),
                    },
                )?;
                let DatasetResult::Records { records, .. } = result else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                let matches = records
                    .into_iter()
                    .enumerate()
                    .filter(|(_, record)| String::from_utf8_lossy(record).contains(&search))
                    .map(|(line, record)| json!({"line":line + 1,"text":String::from_utf8_lossy(&record)}))
                    .collect::<Vec<_>>();
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"items":matches}),
                ))
            }
            GatewayRequest::Ams { control } => self.ams(&principal, &control),
            GatewayRequest::JobList {
                owner,
                prefix,
                jobid,
                max,
            } => {
                let requested_owner = owner
                    .as_deref()
                    .filter(|owner| *owner != "*")
                    .unwrap_or(&principal);
                if !requested_owner.eq_ignore_ascii_case(&principal) {
                    return Err(gateway_problem(HostProblem::Unauthorized));
                }
                self.authorize_resource(&principal, "JESJOBS", "JOB.**", AccessIntent::Read)?;
                let owner = Some(
                    PrincipalId::new(requested_owner, InvocationLimits::default())
                        .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                );
                let (jobs, _) = self
                    .batch
                    .list(owner.as_ref(), None, max)
                    .map_err(gateway_problem)?;
                let items = jobs
                    .into_iter()
                    .filter(|job| {
                        prefix
                            .as_ref()
                            .is_none_or(|prefix| wildcard(prefix, &job.name))
                            && jobid
                                .as_ref()
                                .is_none_or(|jobid| job.id.eq_ignore_ascii_case(jobid))
                    })
                    .map(job_json)
                    .collect::<Vec<_>>();
                Ok(GatewayResponse::json(StatusCode::OK, Value::Array(items)))
            }
            GatewayRequest::JobSubmit { jcl } => {
                let capabilities = job_capabilities(&jcl);
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:job-submit",
                        ServiceClass::Batch,
                        &capabilities,
                    )
                    .map_err(gateway_problem)?;
                let snapshot = self
                    .batch
                    .submit(
                        &invocation,
                        &JclBundle {
                            primary: String::from_utf8(jcl)
                                .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                            ..Default::default()
                        },
                        &self.idempotency("submit").map_err(gateway_problem)?,
                        false,
                    )
                    .map_err(gateway_problem)?;
                let work_id = format!("jes:{}", snapshot.id);
                self.store
                    .enqueue(WorkRecord {
                        work_id: work_id.clone(),
                        execution_id: invocation.execution_id.clone(),
                        required_selector: invocation.selector.clone(),
                        required_generation: "mainframe-env-batch@1".into(),
                        artifact: invocation.artifact.clone(),
                        state: WorkState::Queued,
                        attempt: 0,
                        max_attempts: 3,
                        available_tick: 1,
                        deadline_tick: invocation.deadline_tick,
                        cancellation_requested: false,
                        worker_id: None,
                        lease_id: None,
                        lease_expiry_tick: None,
                        heartbeat_tick: None,
                        checkpoint_id: None,
                        effect_sequence: 0,
                        payload: snapshot.id.as_bytes().to_vec(),
                    })
                    .map_err(store_error)
                    .map_err(gateway_problem)?;
                let claimed = self
                    .store
                    .claim("jes-worker-0", 1, 10)
                    .map_err(store_error)
                    .map_err(gateway_problem)?
                    .ok_or_else(|| gateway_problem(HostProblem::InfrastructureFailure))?;
                if claimed.work_id != work_id {
                    let lease = claimed
                        .lease_id
                        .as_deref()
                        .ok_or_else(|| gateway_problem(HostProblem::InfrastructureFailure))?;
                    let _ = self.store.release(&claimed.work_id, lease, 2);
                    return Err(gateway_problem(HostProblem::InfrastructureFailure));
                }
                let lease = claimed
                    .lease_id
                    .clone()
                    .ok_or_else(|| gateway_problem(HostProblem::InfrastructureFailure))?;
                self.store
                    .heartbeat(&work_id, &lease, 2, 10)
                    .map_err(store_error)
                    .map_err(gateway_problem)?;
                let completed = match self.batch.run_next(&invocation, false) {
                    Ok(result) => {
                        self.store
                            .complete(&work_id, &lease)
                            .map_err(store_error)
                            .map_err(gateway_problem)?;
                        result.unwrap_or(snapshot)
                    }
                    Err(problem) => {
                        let _ = self.store.dead_letter(&work_id, &lease);
                        return Err(gateway_problem(problem));
                    }
                };
                Ok(GatewayResponse::json(
                    StatusCode::CREATED,
                    job_json(completed),
                ))
            }
            GatewayRequest::JobStatus { jobname, jobid } => {
                let job = self.batch.get(&jobid).map_err(gateway_problem)?;
                verify_job(&principal, &jobname, &job)?;
                self.authorize_resource(
                    &principal,
                    "JESJOBS",
                    &format!("JOB.{}", job.name),
                    AccessIntent::Read,
                )?;
                Ok(GatewayResponse::json(StatusCode::OK, job_json(job)))
            }
            GatewayRequest::JobCancel { jobname, jobid } => {
                let job = self.batch.get(&jobid).map_err(gateway_problem)?;
                verify_job(&principal, &jobname, &job)?;
                self.authorize_resource(
                    &principal,
                    "JESJOBS",
                    &format!("JOB.{}", job.name),
                    AccessIntent::Alter,
                )?;
                match self.store.request_cancellation(&format!("jes:{jobid}")) {
                    Ok(_) | Err(StoreError::NotFound) => {}
                    Err(error) => return Err(gateway_problem(store_error(error))),
                }
                self.batch.cancel(&jobid).map_err(gateway_problem)?;
                Ok(GatewayResponse::empty(StatusCode::NO_CONTENT))
            }
            GatewayRequest::JobPurge { jobname, jobid } => {
                let job = self.batch.get(&jobid).map_err(gateway_problem)?;
                verify_job(&principal, &jobname, &job)?;
                self.authorize_resource(
                    &principal,
                    "JESJOBS",
                    &format!("JOB.{}", job.name),
                    AccessIntent::Alter,
                )?;
                self.batch.purge(&jobid).map_err(gateway_problem)?;
                Ok(GatewayResponse::empty(StatusCode::NO_CONTENT))
            }
            GatewayRequest::SpoolList { jobname, jobid } => {
                let job = self.batch.get(&jobid).map_err(gateway_problem)?;
                verify_job(&principal, &jobname, &job)?;
                self.authorize_resource(
                    &principal,
                    "JESJOBS",
                    &format!("JOB.{}", job.name),
                    AccessIntent::Read,
                )?;
                let files = self
                    .batch
                    .spool_files(&jobid)
                    .map_err(gateway_problem)?
                    .into_iter()
                    .map(|(id, ddname, records, bytes)| {
                        let stepname = if ddname.starts_with("JES") {
                            "JES2"
                        } else {
                            ""
                        };
                        json!({
                            "jobid":job.id,
                            "jobname":job.name,
                            "id":id,
                            "ddname":ddname,
                            "stepname":stepname,
                            "procstep":"",
                            "class":"A",
                            "byte-count":bytes,
                            "record-count":records
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(GatewayResponse::json(StatusCode::OK, Value::Array(files)))
            }
            GatewayRequest::SpoolRead {
                jobname,
                jobid,
                file,
                start,
                max,
            } => {
                let job = self.batch.get(&jobid).map_err(gateway_problem)?;
                verify_job(&principal, &jobname, &job)?;
                self.authorize_resource(
                    &principal,
                    "JESJOBS",
                    &format!("JOB.{}", job.name),
                    AccessIntent::Read,
                )?;
                let records = self
                    .batch
                    .spool_by_index(&jobid, file, start, max)
                    .map_err(gateway_problem)?
                    .0;
                Ok(GatewayResponse::bytes(
                    StatusCode::OK,
                    join_records(records),
                ))
            }
            GatewayRequest::ConsoleIssue { name, command } => {
                self.console_issue(&principal, &name, &command)
            }
            GatewayRequest::ConsoleSolicited { name, key }
            | GatewayRequest::ConsoleDetection { name, key } => {
                self.console_message(&principal, &name, &key)
            }
            GatewayRequest::ConsoleLogs | GatewayRequest::ConsoleLog => {
                self.console_logs(&principal)
            }
            GatewayRequest::CicsLaunch {
                transaction,
                rows,
                columns,
            } => {
                let session_token = self.secure_token("cics")?;
                let csrf_token = self.secure_token("csrf")?;
                let session = SessionId::new(
                    &session_token,
                    InvocationLimits::default().max_binding_bytes,
                )
                .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
                let online = self.online_transaction(&transaction)?;
                let invocation = self.cics_invocation(
                    &principal,
                    &transaction,
                    online.as_ref().map(|(_, artifact)| artifact.clone()),
                )?;
                self.cics
                    .launch_terminal(
                        invocation,
                        &session,
                        &transaction,
                        rows,
                        columns,
                        &csrf_token,
                        current_tick()?,
                        15 * 60 * 1_000,
                    )
                    .map_err(gateway_problem)?;
                if let Some((program, _)) = online {
                    self.run_online_exchange(
                        &session,
                        &terminal_principal(&principal)?,
                        &program,
                        current_tick()?,
                    )
                    .map_err(gateway_problem)?;
                }
                let snapshot = self
                    .cics
                    .terminal_snapshot(&session, &terminal_principal(&principal)?, current_tick()?)
                    .map_err(gateway_problem)?;
                Ok(GatewayResponse::json(
                    StatusCode::CREATED,
                    json!({"session":snapshot.session,"csrf_token":csrf_token,"terminal":terminal_json(&snapshot)}),
                ))
            }
            GatewayRequest::CicsScreen { session, tn3270 } => {
                let session = terminal_session_id(&session)?;
                let principal = terminal_principal(&principal)?;
                let tick = current_tick()?;
                if tn3270 {
                    let bytes = self
                        .cics
                        .tn3270_screen(&session, &principal, tick)
                        .map_err(gateway_problem)?;
                    let mut response = GatewayResponse::bytes(StatusCode::OK, bytes);
                    response
                        .headers
                        .insert("content-type".into(), "application/octet-stream".into());
                    Ok(response)
                } else {
                    let snapshot = self
                        .cics
                        .terminal_snapshot(&session, &principal, tick)
                        .map_err(gateway_problem)?;
                    Ok(GatewayResponse::json(
                        StatusCode::OK,
                        terminal_json(&snapshot),
                    ))
                }
            }
            GatewayRequest::CicsInput {
                session,
                csrf_token,
                aid,
                fields,
            } => {
                let snapshot = self
                    .cics
                    .submit_terminal_input(
                        &terminal_session_id(&session)?,
                        &terminal_principal(&principal)?,
                        &csrf_token,
                        aid,
                        &fields,
                        current_tick()?,
                    )
                    .map_err(gateway_problem)?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    terminal_json(&snapshot),
                ))
            }
            GatewayRequest::CicsTn3270Input {
                session,
                csrf_token,
                record,
            } => {
                let snapshot = self
                    .cics
                    .submit_tn3270(
                        &terminal_session_id(&session)?,
                        &terminal_principal(&principal)?,
                        &csrf_token,
                        &record,
                        current_tick()?,
                    )
                    .map_err(gateway_problem)?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    terminal_json(&snapshot),
                ))
            }
            GatewayRequest::CicsResume {
                session,
                csrf_token,
            } => {
                let session = terminal_session_id(&session)?;
                let principal_id = terminal_principal(&principal)?;
                let snapshot = self
                    .cics
                    .terminal_snapshot(&session, &principal_id, current_tick()?)
                    .map_err(gateway_problem)?;
                let invocation = self.cics_invocation(&principal, &snapshot.transaction, None)?;
                let resumed = self
                    .cics
                    .resume_terminal(invocation, &session, &csrf_token, current_tick()?)
                    .map_err(gateway_problem)?;
                let online = self.online_transaction(&resumed.transaction)?;
                if let Some((program, _)) = online {
                    self.run_online_exchange(&session, &principal_id, &program, current_tick()?)
                        .map_err(gateway_problem)?;
                }
                let snapshot = self
                    .cics
                    .terminal_snapshot(&session, &principal_id, current_tick()?)
                    .map_err(gateway_problem)?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    terminal_json(&snapshot),
                ))
            }
            GatewayRequest::CicsDisconnect {
                session,
                csrf_token,
            } => {
                self.cics
                    .disconnect_terminal(
                        &terminal_session_id(&session)?,
                        &terminal_principal(&principal)?,
                        &csrf_token,
                        current_tick()?,
                    )
                    .map_err(gateway_problem)?;
                Ok(GatewayResponse::empty(StatusCode::NO_CONTENT))
            }
            GatewayRequest::Info | GatewayRequest::Authenticate => {
                Err(gateway_problem(HostProblem::Malformed))
            }
        }
    }

    fn verify(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let reference = format!("request:{sequence}");
        self.secrets.insert(&reference, secret.to_vec());
        let result = self.racf.authenticate(
            &PrincipalId::new(user.to_ascii_uppercase(), InvocationLimits::default())
                .map_err(|_| HostProblem::Unauthorized)?,
            &SecretRef::new(reference.clone(), HostLimits::default())?,
        );
        self.secrets.remove(&reference);
        match result? {
            SecurityDecision::Allow => Ok(()),
            _ => Err(HostProblem::Unauthorized),
        }
    }

    fn principal(&self, authentication: Authentication) -> Result<String, HostProblem> {
        match &authentication {
            Authentication::Basic { user, secret } => {
                self.verify(user, secret)?;
                Ok(user.to_ascii_uppercase())
            }
            Authentication::Bearer(token) => self
                .sessions
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .get(token)
                .map(|session| session.user.clone())
                .ok_or(HostProblem::Unauthorized),
            Authentication::Anonymous => Err(HostProblem::Unauthorized),
        }
    }

    fn create_session(&self, user: &str) -> Result<String, HostProblem> {
        let token = secure_random_token("session")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if sessions.len() >= 65536 {
            return Err(HostProblem::ResourceExhausted);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "auth-session".into(),
                    key: token.clone(),
                    version: 1,
                    payload: user.to_ascii_uppercase().into_bytes(),
                },
                None,
            )
            .map_err(store_error)?;
        sessions.insert(
            token.clone(),
            AuthSession {
                user: user.to_ascii_uppercase(),
                version: 1,
            },
        );
        Ok(token)
    }

    fn logout_token(&self, token: &str) -> Result<(), HostProblem> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let version = sessions.get(token).ok_or(HostProblem::NotFound)?.version;
        self.store
            .delete_provider_state("auth-session", token, version)
            .map_err(store_error)?;
        sessions.remove(token);
        Ok(())
    }

    fn dataset_call(
        &self,
        principal: &str,
        request: DatasetRequest,
    ) -> Result<DatasetResult, GatewayProblem> {
        if let DatasetRequest::ReadConcatenation { datasets, .. } = &request {
            for dataset in datasets {
                self.authorize_resource(
                    principal,
                    "DATASET",
                    dataset.as_str(),
                    AccessIntent::Read,
                )?;
            }
        }
        let dataset = match &request {
            DatasetRequest::List { pattern, .. } => Some(pattern.as_str()),
            DatasetRequest::ReadConcatenation { .. } => None,
            DatasetRequest::Rename { from, .. } => Some(from.as_str()),
            DatasetRequest::Attributes { dataset }
            | DatasetRequest::ListMembers { dataset, .. }
            | DatasetRequest::Read { dataset, .. }
            | DatasetRequest::ReadRelative { dataset, .. }
            | DatasetRequest::Create { dataset, .. }
            | DatasetRequest::Write { dataset, .. }
            | DatasetRequest::RewriteRecord { dataset, .. }
            | DatasetRequest::DeleteRecord { dataset, .. }
            | DatasetRequest::WriteRelative { dataset, .. }
            | DatasetRequest::DeleteRelative { dataset, .. }
            | DatasetRequest::Delete { dataset, .. }
            | DatasetRequest::StartBrowse { dataset, .. }
            | DatasetRequest::ReadNext { dataset, .. }
            | DatasetRequest::EndBrowse { dataset, .. } => Some(dataset.as_str()),
            DatasetRequest::DefineAlternateIndex { base, .. }
            | DatasetRequest::DefineGenerationGroup { base, .. }
            | DatasetRequest::CreateGeneration { base, .. }
            | DatasetRequest::ResolveGeneration { base, .. } => Some(base.as_str()),
        };
        if let Some(dataset) = dataset {
            self.authorize_resource(
                principal,
                "DATASET",
                dataset,
                if matches!(
                    request,
                    DatasetRequest::Attributes { .. }
                        | DatasetRequest::ListMembers { .. }
                        | DatasetRequest::Read { .. }
                        | DatasetRequest::ReadRelative { .. }
                        | DatasetRequest::ResolveGeneration { .. }
                        | DatasetRequest::List { .. }
                ) {
                    AccessIntent::Read
                } else {
                    AccessIntent::Update
                },
            )?;
        }
        let capability = if dataset_mutation(&request).is_some() {
            "host.dataset.write"
        } else {
            "host.dataset.read"
        };
        let invocation = self
            .invocation(
                principal,
                "zosmf:dataset",
                ServiceClass::System,
                &[capability],
            )
            .map_err(gateway_problem)?;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let mutation = dataset_mutation(&request);
        let idempotency_key = mutation.map(|mutation| mutation.idempotency_key.clone());
        let result = self.host.invoke(
            &invocation,
            1,
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: mutation.map_or(sequence, |mutation| mutation.sequence),
                deadline_tick: invocation.deadline_tick,
                idempotency_key,
                request: HostRequest::Dataset(request),
            },
        );
        match result.effect.outcome.map_err(gateway_problem)? {
            HostResult::Dataset(result) => Ok(result),
            _ => Err(gateway_problem(HostProblem::ProviderFailure)),
        }
    }

    fn authorize_resource(
        &self,
        principal: &str,
        class: &str,
        resource: &str,
        intent: AccessIntent,
    ) -> Result<(), GatewayProblem> {
        let invocation = self
            .invocation(
                principal,
                "security:authorize",
                ServiceClass::System,
                &["host.security.authorize"],
            )
            .map_err(gateway_problem)?;
        let result = self.host.invoke(
            &invocation,
            1,
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Security(
                    mainframe_env_host_api::SecurityRequest::Authorize {
                        principal: invocation.principal.id().clone(),
                        class: class.into(),
                        resource: ResourceName::new(resource, 246)
                            .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                        intent,
                    },
                ),
            },
        );
        match result.effect.outcome.map_err(gateway_problem)? {
            HostResult::Security(SecurityDecision::Allow) => Ok(()),
            HostResult::Security(_) => Err(gateway_problem(HostProblem::Unauthorized)),
            _ => Err(gateway_problem(HostProblem::ProviderFailure)),
        }
    }

    fn invocation(
        &self,
        principal: &str,
        selector: &str,
        service_class: ServiceClass,
        required_capabilities: &[&str],
    ) -> Result<Invocation, HostProblem> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let limits = InvocationLimits::default();
        let grants = required_capabilities
            .iter()
            .copied()
            .map(|name| {
                CapabilityId::new(name, limits).map_err(|_| HostProblem::InfrastructureFailure)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let generations = grants
            .iter()
            .cloned()
            .map(|capability| (capability, "1".to_string()))
            .collect();
        Invocation::new(
            RequestId::new(format!("request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(format!("run-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            None,
            Selector::new(selector, limits).map_err(|_| HostProblem::Malformed)?,
            ArtifactRef::new("artifact:none", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            Principal::new(
                PrincipalId::new(principal, limits).map_err(|_| HostProblem::Unauthorized)?,
                grants,
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            service_class,
            0,
            u64::MAX,
            TraceId::new(format!("trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .and_then(|invocation| invocation.with_provider_generations(generations, limits))
        .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn mutation(&self) -> Mutation {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("mutation-{sequence}"),
                InvocationLimits::default(),
            )
            .expect("bounded generated key"),
            transaction: Some(format!("zosmf-{sequence}")),
        }
    }

    fn idempotency(&self, kind: &str) -> Result<IdempotencyKey, HostProblem> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        IdempotencyKey::new(
            format!("zosmf-{kind}-{sequence}"),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn secure_token(&self, kind: &str) -> Result<String, GatewayProblem> {
        secure_random_token(kind).map_err(gateway_problem)
    }

    fn cics_invocation(
        &self,
        principal: &str,
        transaction: &str,
        artifact: Option<ArtifactRef>,
    ) -> Result<Invocation, GatewayProblem> {
        let mut invocation = self
            .invocation(
                principal,
                &format!("cics:{}", transaction.to_ascii_uppercase()),
                ServiceClass::Interactive,
                &[
                    "host.security.authorize",
                    "host.cics.execute",
                    "host.dataset.read",
                    "host.dataset.write",
                    "host.program.invoke",
                    "host.clock",
                ],
            )
            .map_err(gateway_problem)?;
        if let Some(artifact) = artifact {
            invocation.artifact = artifact;
        }
        Ok(invocation)
    }

    fn online_transaction(
        &self,
        transaction: &str,
    ) -> Result<Option<(String, ArtifactRef)>, GatewayProblem> {
        let transaction = normalize_online_name(transaction, 16).map_err(gateway_problem)?;
        let Some(program) = self
            .online_transactions
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?
            .get(&transaction)
            .cloned()
        else {
            return Ok(None);
        };
        let artifact = self
            .online_programs
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?
            .get(&program)
            .cloned()
            .ok_or_else(|| gateway_problem(HostProblem::InfrastructureFailure))?;
        Ok(Some((program, artifact)))
    }

    fn ams(&self, principal: &str, control: &[u8]) -> Result<GatewayResponse, GatewayProblem> {
        let text = String::from_utf8(control.to_vec())
            .map_err(|_| gateway_problem(HostProblem::Malformed))?;
        let words = text.split_whitespace().collect::<Vec<_>>();
        let command = words
            .first()
            .map(|value| value.to_ascii_uppercase())
            .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
        match command.as_str() {
            "LISTCAT" => {
                let result = self.dataset_call(
                    principal,
                    DatasetRequest::List {
                        pattern: format!("{}.**", principal.to_ascii_uppercase()),
                        start: None,
                        max_items: 1000,
                    },
                )?;
                let DatasetResult::Listed { names, more } = result else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"entries":names.into_iter().map(|name|name.as_str().to_string()).collect::<Vec<_>>(),"more":more}),
                ))
            }
            "DEFINE" => {
                let name = control_name(&text, "NAME")
                    .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
                self.dataset_call(
                    principal,
                    DatasetRequest::Create {
                        dataset: dataset_name(&name)?,
                        attributes: DatasetAttributes {
                            organization: DatasetOrganization::KeySequenced,
                            record_format: RecordFormat::Variable,
                            logical_record_length: 32760,
                            key_offset: Some(0),
                            key_length: Some(8),
                            ccsid: Some(37),
                        },
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"command":"DEFINE","dataset":name}),
                ))
            }
            "DELETE" => {
                let name = words
                    .get(1)
                    .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
                self.dataset_call(
                    principal,
                    DatasetRequest::Delete {
                        dataset: dataset_name(name)?,
                        member: None,
                        expected_version: None,
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"command":"DELETE","dataset":name}),
                ))
            }
            "REPRO" => {
                let input = control_name(&text, "INDATASET")
                    .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
                let output = control_name(&text, "OUTDATASET")
                    .ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
                let DatasetResult::Records { records, .. } = self.dataset_call(
                    principal,
                    DatasetRequest::Read {
                        dataset: dataset_name(&input)?,
                        member: None,
                        key: None,
                        max_records: 4096,
                    },
                )?
                else {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                };
                let version = match self.dataset_call(
                    principal,
                    DatasetRequest::Attributes {
                        dataset: dataset_name(&output)?,
                    },
                )? {
                    DatasetResult::Attributes { version, .. } => version,
                    _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
                };
                self.dataset_call(
                    principal,
                    DatasetRequest::Write {
                        dataset: dataset_name(&output)?,
                        member: None,
                        records,
                        expected_version: Some(version),
                        mutation: self.mutation(),
                    },
                )?;
                Ok(GatewayResponse::json(
                    StatusCode::OK,
                    json!({"command":"REPRO","from":input,"to":output}),
                ))
            }
            _ => Err(gateway_problem(HostProblem::Unsupported)),
        }
    }

    fn console_issue(
        &self,
        principal: &str,
        name: &str,
        command: &[u8],
    ) -> Result<GatewayResponse, GatewayProblem> {
        self.authorize_resource(
            principal,
            "FACILITY",
            &format!("CONSOLE.{}", name.to_ascii_uppercase()),
            AccessIntent::Execute,
        )?;
        let command = String::from_utf8(command.to_vec())
            .map_err(|_| gateway_problem(HostProblem::Malformed))?;
        let upper = command.trim().to_ascii_uppercase();
        let text = match upper.as_str() {
            "D IPLINFO" => b"IEE254I IPLINFO MAINFRAME-ENV 0.1".to_vec(),
            "D A,L" => b"IEE114I ACTIVE JOBS MAINFRAME-ENV".to_vec(),
            "D U,ALL" => b"IEE457I UNIT STATUS AVAILABLE".to_vec(),
            _ => return Err(gateway_problem(HostProblem::Unsupported)),
        };
        let mut messages = self
            .console
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        if messages.len() >= 65536 {
            return Err(gateway_problem(HostProblem::ResourceExhausted));
        }
        let key = format!("{:016}", self.sequence.fetch_add(1, Ordering::Relaxed));
        let mut payload = name.to_ascii_uppercase().into_bytes();
        payload.push(0);
        payload.extend_from_slice(&text);
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "console-log".into(),
                    key: key.clone(),
                    version: 1,
                    payload,
                },
                None,
            )
            .map_err(store_error)
            .map_err(gateway_problem)?;
        messages.push(ConsoleMessage {
            key: key.clone(),
            console: name.to_ascii_uppercase(),
            text: text.clone(),
        });
        Ok(GatewayResponse::json(
            StatusCode::OK,
            json!({"cmd-response-key":key,"cmd-response":String::from_utf8_lossy(&text)}),
        ))
    }

    fn console_message(
        &self,
        principal: &str,
        name: &str,
        key: &str,
    ) -> Result<GatewayResponse, GatewayProblem> {
        self.authorize_resource(
            principal,
            "FACILITY",
            &format!("CONSOLE.{}", name),
            AccessIntent::Read,
        )?;
        let messages = self
            .console
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        let message = messages
            .iter()
            .find(|message| message.console.eq_ignore_ascii_case(name) && message.key == key)
            .ok_or_else(|| gateway_problem(HostProblem::NotFound))?;
        Ok(GatewayResponse::json(
            StatusCode::OK,
            json!({"cmd-response-key":key,"cmd-response":String::from_utf8_lossy(&message.text)}),
        ))
    }

    fn console_logs(&self, principal: &str) -> Result<GatewayResponse, GatewayProblem> {
        self.authorize_resource(principal, "FACILITY", "CONSOLE.LOG", AccessIntent::Read)?;
        let messages = self
            .console
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        Ok(GatewayResponse::json(
            StatusCode::OK,
            json!({"items":messages.iter().map(|message|json!({"key":message.key,"console":message.console,"text":String::from_utf8_lossy(&message.text)})).collect::<Vec<_>>() }),
        ))
    }
}

impl ZosmfBackend for ProductServer {
    fn call(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, GatewayProblem> {
        self.handle(authentication, request)
    }
}

struct ActiveGuard<'a>(&'a AtomicUsize);

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn scoped_host(
    racf: &Arc<RacfService>,
    dataset: &Arc<DatasetService>,
    program: Arc<dyn HostProvider>,
    include_cics: bool,
    cics: Option<Arc<dyn HostProvider>>,
) -> Result<Arc<ScopedHostService>, HostProblem> {
    let limits = InvocationLimits::default();
    let mut providers = dataset_providers(dataset.clone(), limits);
    providers.extend(racf_providers(racf.clone(), limits));
    providers.push(Arc::new(SystemClockProvider::new(limits)) as Arc<dyn HostProvider>);
    providers.push(program);
    if include_cics {
        providers.push(cics.ok_or(HostProblem::InfrastructureFailure)?);
    }
    Ok(Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, providers, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ),
        HostLimits::default(),
    )))
}

struct SystemClockProvider {
    descriptor: CapabilityDescriptor,
}

impl SystemClockProvider {
    fn new(limits: InvocationLimits) -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.clock", limits)
                    .expect("static clock capability"),
                provider_id: "mainframe-env-system-clock".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.clock.request@1".into(),
                result_schema: "mainframe-env.clock.response@1".into(),
                max_request_bytes: 64,
                max_result_bytes: 64,
                ready: true,
            },
        }
    }
}

impl HostProvider for SystemClockProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Clock(request) => system_clock_value(request).map(HostResult::Clock),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn system_clock_value(request: ClockRequest) -> Result<String, HostProblem> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let seconds = i64::try_from(duration.as_secs()).map_err(|_| HostProblem::ResourceExhausted)?;
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_unix_days(days);
    let hour = rest / 3_600;
    let minute = (rest / 60) % 60;
    let second = rest % 60;
    let milliseconds = duration.subsec_millis();
    Ok(match request {
        ClockRequest::UtcTimestamp => {
            format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}{milliseconds:03}")
        }
        ClockRequest::Date => format!("{year:04}{month:02}{day:02}"),
        ClockRequest::Time => format!("{hour:02}{minute:02}{second:02}{milliseconds:03}"),
    })
}

fn civil_from_unix_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn dataset_name(value: &str) -> Result<DatasetName, GatewayProblem> {
    DatasetName::new(value.to_ascii_uppercase(), 128)
        .map_err(|_| gateway_problem(HostProblem::Malformed))
}

fn terminal_session_id(value: &str) -> Result<SessionId, GatewayProblem> {
    SessionId::new(value, InvocationLimits::default().max_binding_bytes)
        .map_err(|_| gateway_problem(HostProblem::Malformed))
}

fn terminal_principal(value: &str) -> Result<PrincipalId, GatewayProblem> {
    PrincipalId::new(value, InvocationLimits::default())
        .map_err(|_| gateway_problem(HostProblem::Unauthorized))
}

fn current_tick() -> Result<u64, GatewayProblem> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?
            .as_millis(),
    )
    .map_err(|_| gateway_problem(HostProblem::ResourceExhausted))
}

fn terminal_json(snapshot: &CicsTerminalSnapshot) -> Value {
    json!({
        "schema_version":"mainframe-env.cics-terminal@1",
        "session":snapshot.session,
        "principal":snapshot.principal,
        "transaction":snapshot.transaction,
        "run_unit":snapshot.run_unit,
        "rows":snapshot.rows,
        "columns":snapshot.columns,
        "aid":snapshot.aid,
        "screen_base64":base64::engine::general_purpose::STANDARD.encode(&snapshot.screen),
        "mapset":snapshot.mapset,
        "map":snapshot.map,
        "suspended":snapshot.suspended,
        "connected":snapshot.connected,
        "expires_at_tick":snapshot.expires_at_tick,
        "version":snapshot.version
    })
}

fn secure_random_token(kind: &str) -> Result<String, HostProblem> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(format!(
        "{kind}-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

fn normalize_online_name(value: &str, max: usize) -> Result<String, HostProblem> {
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.is_empty()
        || normalized.len() > max
        || !normalized.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(normalized)
    }
}

fn encode_online_machine_continuation(
    program: &str,
    checkpoint: &BoundedPayload,
) -> Result<Vec<u8>, HostProblem> {
    let mut encoded = b"MEOM1".to_vec();
    for value in [
        program.as_bytes(),
        checkpoint.schema().as_bytes(),
        checkpoint.bytes(),
    ] {
        encoded.extend_from_slice(
            &u32::try_from(value.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(value);
    }
    Ok(encoded)
}

fn decode_online_machine_continuation(
    record: &ProviderStateRecord,
) -> Result<OnlineMachineContinuation, HostProblem> {
    if !record.payload.starts_with(b"MEOM1") {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 5usize;
    let mut next = || -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            record
                .payload
                .get(at..at + 4)
                .ok_or(HostProblem::InfrastructureFailure)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        at += 4;
        let end = at
            .checked_add(length)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = record
            .payload
            .get(at..end)
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_vec();
        at = end;
        Ok(value)
    };
    let program = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let schema = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let bytes = next()?;
    if at != record.payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(OnlineMachineContinuation {
        program: normalize_online_name(&program, 128)?,
        checkpoint: BoundedPayload::new(
            schema,
            bytes,
            InvocationLimits {
                max_payload_bytes: 64 * 1024 * 1024,
                ..InvocationLimits::default()
            },
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        version: record.version,
    })
}

fn digest_online_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn dataset_mutation(request: &DatasetRequest) -> Option<&Mutation> {
    match request {
        DatasetRequest::Create { mutation, .. }
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::Rename { mutation, .. }
        | DatasetRequest::Delete { mutation, .. } => Some(mutation),
        _ => None,
    }
}

fn member_name(value: Option<String>) -> Result<Option<MemberName>, GatewayProblem> {
    value
        .map(|value| {
            MemberName::new(value.to_ascii_uppercase(), 8)
                .map_err(|_| gateway_problem(HostProblem::Malformed))
        })
        .transpose()
}

fn dataset_attributes(value: &Value) -> Result<DatasetAttributes, HostProblem> {
    let organization = match value
        .get("dsorg")
        .and_then(Value::as_str)
        .unwrap_or("PS")
        .to_ascii_uppercase()
        .as_str()
    {
        "PS" => DatasetOrganization::Sequential,
        "PO" | "PO-E" => DatasetOrganization::Partitioned,
        "VS" | "KSDS" => DatasetOrganization::KeySequenced,
        _ => return Err(HostProblem::Unsupported),
    };
    let record_format = match value
        .get("recfm")
        .and_then(Value::as_str)
        .unwrap_or("FB")
        .to_ascii_uppercase()
        .as_str()
    {
        "F" => RecordFormat::Fixed,
        "FB" => RecordFormat::FixedBlocked,
        "V" => RecordFormat::Variable,
        "VB" => RecordFormat::VariableBlocked,
        "U" => RecordFormat::Undefined,
        "LINE" => RecordFormat::Line,
        _ => return Err(HostProblem::Unsupported),
    };
    let logical_record_length = value.get("lrecl").and_then(Value::as_u64).unwrap_or(80);
    let attributes = DatasetAttributes {
        organization,
        record_format,
        logical_record_length: u32::try_from(logical_record_length)
            .map_err(|_| HostProblem::ResourceExhausted)?,
        key_offset: value
            .get("key_offset")
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).map_err(|_| HostProblem::ResourceExhausted))
            .transpose()?,
        key_length: value
            .get("key_length")
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).map_err(|_| HostProblem::ResourceExhausted))
            .transpose()?,
        ccsid: Some(37),
    };
    attributes.validate(HostLimits::default())?;
    Ok(attributes)
}

fn records_for_write(
    bytes: &[u8],
    attributes: &DatasetAttributes,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let mut records = bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
        .map(|record| record.strip_suffix(b"\r").unwrap_or(record).to_vec())
        .collect::<Vec<_>>();
    if records.is_empty() {
        records.push(Vec::new());
    }
    if matches!(
        attributes.record_format,
        RecordFormat::Fixed | RecordFormat::FixedBlocked
    ) {
        for record in &mut records {
            let length = attributes.logical_record_length as usize;
            if record.len() > length {
                return Err(HostProblem::Condition {
                    name: "LENGERR".into(),
                    response: 22,
                    response2: 0,
                });
            }
            record.resize(length, b' ');
        }
    }
    Ok(records)
}

fn join_records(records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, mut record) in records.into_iter().enumerate() {
        while record.last() == Some(&b' ') {
            record.pop();
        }
        if index > 0 {
            output.push(b'\n');
        }
        output.extend_from_slice(&record);
    }
    output
}

fn wildcard(pattern: &str, value: &str) -> bool {
    pattern == "*"
        || pattern.eq_ignore_ascii_case(value)
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| value.starts_with(prefix))
}

fn job_capabilities(jcl: &[u8]) -> Vec<&'static str> {
    let source = String::from_utf8_lossy(jcl).to_ascii_uppercase();
    let mut capabilities = vec!["host.security.authorize", "host.program.invoke"];
    if source.contains("DSN=") || source.contains("DISP=") || source.contains("PGM=IDCAMS") {
        capabilities.extend(["host.dataset.read", "host.dataset.write"]);
    }
    if source.contains("EXEC CICS") {
        capabilities.push("host.cics.execute");
    }
    if source.contains("ASKTIME") || source.contains("FORMATTIME") {
        capabilities.push("host.clock");
    }
    if source.contains("ACCEPT ") {
        capabilities.push("host.terminal");
    }
    capabilities
}

fn control_name(control: &str, keyword: &str) -> Option<String> {
    let upper = control.to_ascii_uppercase();
    let start = upper.find(&format!("{keyword}("))? + keyword.len() + 1;
    let end = upper[start..].find(')')? + start;
    let value = upper[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn job_json(job: mainframe_env_batch::JobSnapshot) -> Value {
    json!({
        "jobid":job.id,
        "jobname":job.name,
        "owner":job.owner,
        "status":if matches!(job.state, mainframe_env_batch::JobState::Completed | mainframe_env_batch::JobState::Failed | mainframe_env_batch::JobState::Cancelled) {"OUTPUT"} else {"ACTIVE"},
        "type":"JOB",
        "class":job.class.to_string(),
        "retcode":job.return_code.map(|code|format!("CC {code:04}"))
    })
}

fn verify_job(
    principal: &str,
    jobname: &str,
    job: &mainframe_env_batch::JobSnapshot,
) -> Result<(), GatewayProblem> {
    if !job.name.eq_ignore_ascii_case(jobname) {
        Err(gateway_problem(HostProblem::NotFound))
    } else if job.owner != principal {
        Err(gateway_problem(HostProblem::Unauthorized))
    } else {
        Ok(())
    }
}

fn unauthenticated() -> GatewayProblem {
    GatewayProblem::new(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "valid authentication is required",
    )
}

fn gateway_problem(problem: HostProblem) -> GatewayProblem {
    let (status, code) = match problem {
        HostProblem::Malformed => (StatusCode::BAD_REQUEST, "malformed"),
        HostProblem::Unsupported => (StatusCode::NOT_FOUND, "unsupported"),
        HostProblem::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        HostProblem::Unauthorized => (StatusCode::FORBIDDEN, "not_authorized"),
        HostProblem::Cancelled => (StatusCode::CONFLICT, "cancelled"),
        HostProblem::TimedOut => (StatusCode::REQUEST_TIMEOUT, "timed_out"),
        HostProblem::ResourceExhausted => (StatusCode::TOO_MANY_REQUESTS, "resource_exhausted"),
        HostProblem::IdempotencyConflict | HostProblem::UnknownOutcome => {
            (StatusCode::CONFLICT, "conflict")
        }
        HostProblem::Condition { .. } => (StatusCode::CONFLICT, "condition"),
        HostProblem::ProviderFailure | HostProblem::InfrastructureFailure => {
            (StatusCode::SERVICE_UNAVAILABLE, "infrastructure_failure")
        }
        HostProblem::MissingIdempotency => (StatusCode::BAD_REQUEST, "missing_idempotency"),
    };
    GatewayProblem::new(status, code, &problem.to_string())
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use base64::Engine;
    use mainframe_env_compiler::CobolCompiler;
    use mainframe_env_compiler_api::{
        CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
        CompilerService,
    };
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use mainframe_env_store::SqliteStateStore;
    use tower::ServiceExt;

    #[test]
    fn system_clock_provider_emits_bounded_utc_shapes() {
        assert_eq!(civil_from_unix_days(0), (1970, 1, 1));
        assert_eq!(civil_from_unix_days(20_695), (2026, 8, 30));
        let timestamp = system_clock_value(ClockRequest::UtcTimestamp).unwrap();
        let date = system_clock_value(ClockRequest::Date).unwrap();
        let time = system_clock_value(ClockRequest::Time).unwrap();
        assert_eq!(timestamp.len(), 17);
        assert_eq!(date.len(), 8);
        assert_eq!(time.len(), 9);
        assert!(timestamp.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(date.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(time.bytes().all(|byte| byte.is_ascii_digit()));
    }

    fn config() -> ServerConfig {
        ServerConfig {
            store_profile: crate::StoreProfile::Memory,
            tls: crate::TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            artifact_root: std::env::temp_dir().join(format!(
                "mainframe-env-server-artifacts-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            )),
            ..ServerConfig::default()
        }
    }

    fn basic() -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
        )
    }

    async fn call(
        app: &axum::Router,
        method: Method,
        uri: &str,
        body: &'static str,
    ) -> axum::response::Response {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header("authorization", basic());
        if method != Method::GET {
            request = request.header("x-csrf-zosmf-header", "true");
        }
        app.clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn composed_dataset_routes_match_selected_zosmf_contract() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let app = server.router();
        assert_eq!(
            call(
                &app,
                Method::POST,
                "/zosmf/restfiles/ds/IBMUSER.TEST.SEQ",
                r#"{"dsorg":"PS","recfm":"FB","lrecl":80}"#,
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        assert_eq!(
            call(
                &app,
                Method::PUT,
                "/zosmf/restfiles/ds/IBMUSER.TEST.SEQ",
                "HELLO FROM ZOWE CLI\r\n",
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        let response = call(
            &app,
            Method::GET,
            "/zosmf/restfiles/ds/IBMUSER.TEST.SEQ",
            "",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            "HELLO FROM ZOWE CLI"
        );
        let response = call(
            &app,
            Method::GET,
            "/zosmf/restfiles/ds?dslevel=IBMUSER.*",
            "",
        )
        .await;
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(body["returnedRows"], 1);
        assert_eq!(body["items"][0]["dsname"], "IBMUSER.TEST.SEQ");
        assert_eq!(
            call(
                &app,
                Method::POST,
                "/zosmf/restfiles/ds/IBMUSER.TEST.PDS",
                r#"{"dsorg":"PO","recfm":"FB","lrecl":80}"#,
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        let attributes = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/zosmf/restfiles/ds?dslevel=IBMUSER.TEST.PDS&start=IBMUSER.TEST.PDS")
                    .header("authorization", basic())
                    .header("x-ibm-attributes", "base")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attributes.status(), StatusCode::OK);
        let attributes: Value =
            serde_json::from_slice(&to_bytes(attributes.into_body(), 65_536).await.unwrap())
                .unwrap();
        assert_eq!(attributes["returnedRows"], 1);
        assert_eq!(attributes["items"][0]["dsorg"], "PO");
        assert_eq!(attributes["items"][0]["recfm"], "FB");
        assert_eq!(attributes["items"][0]["lrecl"], 80);
        assert_eq!(
            call(
                &app,
                Method::PUT,
                "/zosmf/restfiles/ds/IBMUSER.TEST.PDS(MEMBER)",
                "//MEMBER JOB CLASS=A\r\n",
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        let members = call(
            &app,
            Method::GET,
            "/zosmf/restfiles/ds/IBMUSER.TEST.PDS/member",
            "",
        )
        .await;
        let members: Value =
            serde_json::from_slice(&to_bytes(members.into_body(), 65_536).await.unwrap()).unwrap();
        assert_eq!(members["items"][0]["member"], "MEMBER");
    }

    #[tokio::test]
    async fn composed_job_auth_console_and_shutdown_routes_pass() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let app = server.router();
        let response = call(
            &app,
            Method::PUT,
            "/zosmf/restjobs/jobs",
            "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let job: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(job["jobname"], "TESTJOB");
        assert_eq!(job["status"], "OUTPUT");
        let id = job["jobid"].as_str().unwrap();
        let files = call(
            &app,
            Method::GET,
            &format!("/zosmf/restjobs/jobs/TESTJOB/{id}/files"),
            "",
        )
        .await;
        assert_eq!(files.status(), StatusCode::OK);
        let files: Value =
            serde_json::from_slice(&to_bytes(files.into_body(), 65_536).await.unwrap()).unwrap();
        assert!(files.as_array().unwrap().iter().all(|file| {
            file["jobid"] == id
                && file["jobname"] == "TESTJOB"
                && file.get("stepname").is_some()
                && file.get("procstep").is_some()
        }));
        let by_id = call(
            &app,
            Method::GET,
            &format!("/zosmf/restjobs/jobs?owner=*&jobid={id}"),
            "",
        )
        .await;
        assert_eq!(by_id.status(), StatusCode::OK);
        let by_id: Value =
            serde_json::from_slice(&to_bytes(by_id.into_body(), 65_536).await.unwrap()).unwrap();
        assert_eq!(by_id.as_array().unwrap().len(), 1);
        assert_eq!(by_id[0]["jobid"], id);
        assert_eq!(by_id[0]["retcode"], "CC 0000");
        let console = call(
            &app,
            Method::PUT,
            "/zosmf/restconsoles/consoles/OPER",
            "D IPLINFO",
        )
        .await;
        assert_eq!(console.status(), StatusCode::OK);
        let authenticated = call(&app, Method::POST, "/zosmf/services/authenticate", "").await;
        let session: Value =
            serde_json::from_slice(&to_bytes(authenticated.into_body(), 65536).await.unwrap())
                .unwrap();
        assert!(session["token"].as_str().unwrap().starts_with("session-"));
        assert!(server.metrics().requests >= 4);
        assert!(server.ready());
        assert!(server.graceful_shutdown().await);
        assert!(!server.ready());
    }

    #[tokio::test]
    async fn cobol_cics_job_uses_scoped_principal_and_typed_host_route() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let app = server.router();
        let response = call(
            &app,
            Method::PUT,
            "/zosmf/restjobs/jobs",
            "//CICSJOB JOB CLASS=A\n//STEP1 EXEC PGM=COBOL\n//SYSIN DD *\nIDENTIFICATION DIVISION.\nPROGRAM-ID. CICSBATCH.\nPROCEDURE DIVISION.\nEXEC CICS ASSIGN END-EXEC.\nDISPLAY 'CICS OK'.\nSTOP RUN.\n/*\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let job: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(job["retcode"], "CC 0000");
        assert!(server.metrics().outbox_delivered >= 5);
    }

    #[tokio::test]
    async fn public_cics_cc00_session_requires_authentication_and_csrf() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let app = server.router();
        let unauthenticated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mainframe-env/cics/v1/sessions")
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::from(r#"{"transaction":"CC00"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let launched = call(
            &app,
            Method::POST,
            "/mainframe-env/cics/v1/sessions",
            r#"{"transaction":"CC00","rows":24,"columns":80}"#,
        )
        .await;
        assert_eq!(launched.status(), StatusCode::CREATED);
        let launched: Value =
            serde_json::from_slice(&to_bytes(launched.into_body(), 65536).await.unwrap()).unwrap();
        let session = launched["session"].as_str().unwrap();
        let csrf = launched["csrf_token"].as_str().unwrap();
        assert_eq!(launched["terminal"]["transaction"], "CC00");
        let screen = call(
            &app,
            Method::GET,
            &format!("/mainframe-env/cics/v1/sessions/{session}"),
            "",
        )
        .await;
        assert_eq!(screen.status(), StatusCode::OK);
        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/mainframe-env/cics/v1/sessions/{session}"))
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .header("x-csrf-token", "wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let disconnected = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/mainframe-env/cics/v1/sessions/{session}"))
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disconnected.status(), StatusCode::NO_CONTENT);
        assert_eq!(server.metrics().active, 0);
    }

    #[tokio::test]
    async fn installed_online_program_drives_public_terminal_screen() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ONLINE.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 EIBCALEN PIC 9(4) VALUE 0.\n01 EIBAID PIC X VALUE SPACE.\n01 EIBTRNID PIC X(4) VALUE 'CC00'.\n01 MSG PIC X(5) VALUE 'HELLO'.\n01 STATE-DATA PIC X(5) VALUE 'STATE'.\nLINKAGE SECTION.\n01 DFHCOMMAREA PIC X(5).\nPROCEDURE DIVISION.\nEXEC CICS SEND TEXT FROM(MSG) END-EXEC.\nEXEC CICS RETURN TRANSID('CC00') COMMAREA(STATE-DATA) END-EXEC.\n";
        let path = LogicalPath::new("ONLINE.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "ONLINE.cbl",
                    source.to_vec(),
                    SourceFormat::Free,
                    SourceEncoding::Utf8,
                    limits,
                )
                .unwrap(),
            ],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("online fixture did not publish");
        };
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "ONLINE".into(),
                    artifact: artifact_ref,
                    payload: artifact.payload().to_vec(),
                }],
                transactions: BTreeMap::from([("CC00".into(), "ONLINE".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "ONLINE".into(),
                    map: "ONLINE".into(),
                    rows: 24,
                    columns: 80,
                    fields: vec![mainframe_env_cics::BmsFieldDefinition {
                        name: "INPUT".into(),
                        row: 1,
                        column: 1,
                        length: 8,
                        initial: Vec::new(),
                        color: None,
                        highlight: None,
                        protected: false,
                        secret: false,
                        fset: false,
                        justify_right: false,
                        fill_zero: false,
                        output_offset: None,
                        attribute_offset: None,
                    }],
                }],
            })
            .unwrap();
        let response = call(
            &server.router(),
            Method::POST,
            "/mainframe-env/cics/v1/sessions",
            r#"{"transaction":"CC00"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(body["terminal"]["screen_base64"].as_str().unwrap())
                .unwrap(),
            b"HELLO"
        );
    }

    #[test]
    fn invocation_grants_are_selector_scoped_and_generation_pinned() {
        let server = ProductServer::memory(config()).unwrap();
        let invocation = server
            .invocation(
                "IBMUSER",
                "zosmf:dataset",
                ServiceClass::System,
                &["host.dataset.read"],
            )
            .unwrap();
        assert_eq!(invocation.principal.grants().len(), 1);
        let capability =
            CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap();
        assert_eq!(
            invocation.provider_generations.get(&capability),
            Some(&"1".to_string())
        );
        assert!(!invocation.principal.has_grant(
            &CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap()
        ));
    }

    #[test]
    fn sqlite_product_reopen_preserves_security_dataset_and_console_state() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-product-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("product.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let mut config = config();
        config.store_profile = crate::StoreProfile::Sqlite;
        config.sqlite_url = url.clone();
        {
            let store: Arc<dyn PlatformStore> =
                Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 262144).unwrap());
            let server = ProductServer::open(
                config.clone(),
                store,
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap();
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            server
                .handle(
                    Authentication::Basic {
                        user: "IBMUSER".into(),
                        secret: b"TESTPASS".to_vec(),
                    },
                    GatewayRequest::DatasetCreate {
                        dataset: "IBMUSER.RESTART".into(),
                        attributes: json!({"dsorg":"PS","recfm":"V","lrecl":80}),
                    },
                )
                .unwrap();
            server
                .handle(
                    Authentication::Basic {
                        user: "IBMUSER".into(),
                        secret: b"TESTPASS".to_vec(),
                    },
                    GatewayRequest::ConsoleIssue {
                        name: "OPER".into(),
                        command: b"D IPLINFO".to_vec(),
                    },
                )
                .unwrap();
        }
        {
            let store: Arc<dyn PlatformStore> =
                Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 262144).unwrap());
            let server = ProductServer::open(
                config,
                store,
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap();
            let response = server
                .handle(
                    Authentication::Basic {
                        user: "IBMUSER".into(),
                        secret: b"TESTPASS".to_vec(),
                    },
                    GatewayRequest::DatasetList {
                        pattern: "IBMUSER.**".into(),
                        start: None,
                        attributes: false,
                        max: 10,
                    },
                )
                .unwrap();
            assert_eq!(response.status, StatusCode::OK);
            assert_eq!(server.metrics().console_messages, 1);
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }
}
