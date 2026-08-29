use crate::{DefaultProgramRouter, ServerConfig, default_program_router};
use axum::http::StatusCode;
use mainframe_env_application::ApplicationInstaller;
use mainframe_env_batch::{BatchService, JclBundle};
use mainframe_env_cics::{CicsService, cics_provider};
use mainframe_env_dataset::{DatasetService, dataset_providers};
use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    AccessIntent, DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest,
    DatasetResult, EffectRequest, HostLimits, HostProblem, HostProvider, HostRequest, HostResult,
    MemberName, Mutation, RecordFormat, RegistrySnapshot, ResourceName, ScopedHostService,
    SecretRef, SecurityDecision,
};
use mainframe_env_racf::{MemorySecretResolver, RacfService, racf_providers};
use mainframe_env_store::{LocalArtifactStore, MemoryStore};
use mainframe_env_store_api::{
    PlatformStore, ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState,
};
use mainframe_env_zosmf::{
    Authentication, GatewayProblem, GatewayRequest, GatewayResponse, ZosmfBackend, ZosmfLimits,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
    batch: Arc<BatchService>,
    artifacts: LocalArtifactStore,
    host: Arc<ScopedHostService>,
    applications: ApplicationInstaller,
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
        program.bind_runtime(host.clone(), store.clone())?;
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
        let product = Arc::new(Self {
            config,
            store,
            secrets,
            racf,
            batch,
            artifacts,
            host,
            applications: ApplicationInstaller::new("0.1.1"),
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

    pub fn bootstrap_user(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        let reference = format!("bootstrap:{user}");
        self.secrets.insert(&reference, secret.to_vec());
        let result = self.racf.add_user(
            user,
            &SecretRef::new(reference.clone(), HostLimits::default())?,
        );
        self.secrets.remove(&reference);
        result?;
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
                let count = names.len();
                let mut response = GatewayResponse::json(
                    StatusCode::OK,
                    json!({
                        "items":names.into_iter().map(|name| json!({"dsname":name.as_str()})).collect::<Vec<_>>(),
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
            GatewayRequest::JobList { owner, prefix, max } => {
                let requested_owner = owner.as_deref().unwrap_or(&principal);
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
                    .map(|(id, ddname, records, bytes)|json!({"id":id,"ddname":ddname,"class":"A","byte-count":bytes,"record-count":records}))
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
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let token = format!("session-{sequence:016x}");
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
        let dataset = match &request {
            DatasetRequest::List { pattern, .. } => Some(pattern.as_str()),
            DatasetRequest::Rename { from, .. } => Some(from.as_str()),
            DatasetRequest::Attributes { dataset }
            | DatasetRequest::ListMembers { dataset, .. }
            | DatasetRequest::Read { dataset, .. }
            | DatasetRequest::Create { dataset, .. }
            | DatasetRequest::Write { dataset, .. }
            | DatasetRequest::Delete { dataset, .. }
            | DatasetRequest::StartBrowse { dataset, .. }
            | DatasetRequest::ReadNext { dataset, .. }
            | DatasetRequest::EndBrowse { dataset, .. } => Some(dataset.as_str()),
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
            100,
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

fn dataset_name(value: &str) -> Result<DatasetName, GatewayProblem> {
    DatasetName::new(value.to_ascii_uppercase(), 128)
        .map_err(|_| gateway_problem(HostProblem::Malformed))
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
        .map(<[u8]>::to_vec)
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
    if source.contains("DSN=") || source.contains("DISP=") {
        capabilities.extend(["host.dataset.read", "host.dataset.write"]);
    }
    if source.contains("EXEC CICS") {
        capabilities.push("host.cics.execute");
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
    use mainframe_env_store::SqliteStateStore;
    use tower::ServiceExt;

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
                "HELLO FROM ZOWE CLI",
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
