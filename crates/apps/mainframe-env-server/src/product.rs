use crate::cobol::artifact::admit_executable_artifact;
use crate::cobol::bind_compatible_runtime_services;
use crate::console_retention::{decode_console_log_rows, encode_console_log};
use crate::jes_admission::ChildAdmissionResult;
use crate::jes_worker::{
    DurableJesClock, JES_HEARTBEAT_MILLIS, JES_IDLE_MILLIS, JES_WORK_GENERATION, JES_WORKER_COUNT,
    JES_WORKER_FRESHNESS_MILLIS, JesClock, JesWorkPayload, claim_durable_work,
    clear_worker_progress, heartbeat_durable_work,
};
use crate::retention_maintenance::provider::RetentionPlanner;
use crate::{
    ArtifactProfile, DefaultProgramRouter, EnvironmentSecretResolver, ServerConfig,
    default_program_router,
};
use axum::http::StatusCode;
use base64::Engine;
use mainframe_env_application::{
    ApplicationGenerationRecord, ApplicationInstaller, ApplicationInstallerV2,
    ApplicationPackageV2, BatchController as ApplicationBatchController, BatchControllerKind,
    EntryKind, InstallProblem, InstallState, PackageLimits, PackageSignatureVerifier,
    SelectedApplicationGeneration,
};
use mainframe_env_batch::{
    BATCH_CONTROLLER_REGISTRY_CONTRACT, BatchControllerDefinition, BatchControllerGeneration,
    BatchControllerInstallReceipt, BatchControllerPlan, BatchControllerProgram,
    BatchControllerSelector, BatchLimits, BatchService, JclBundle,
};
use mainframe_env_cics::{
    BmsMapDefinition, CicsReplayClock, CicsService, CicsTerminalExecution, CicsTerminalSnapshot,
    CicsTraceEntry, cics_provider,
};
use mainframe_env_dataset::{DatasetReplayClock, DatasetService, dataset_providers};
use mainframe_env_db2::{
    Db2CatalogGeneration, Db2Limits, Db2ReplayClock, Db2SeedRow, Db2Service, db2_providers,
    decode_table_definitions_bounded,
};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, Cancellation, CancellationId, CapabilityId, ExecutionId,
    ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits, LifecycleEventKind, Machine,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsOperation, ClockRequest, DatasetAttributes,
    DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    EnterpriseAuthorizer, HostLimits, HostProblem, HostProvider, HostRequest, HostResult,
    MemberName, Mutation, RecordFormat, RegistrySnapshot, ResourceName, ScopedHostService,
    SecretRef, SecurityDecision, SessionId, TerminalRequest,
};
use mainframe_env_ims::{ImsReplayClock, ImsService, ims_providers};
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionCoordinator, ReferenceMachine};
use mainframe_env_ir::CodecLimits;
use mainframe_env_mq::{MqReplayClock, MqService, mq_providers};
use mainframe_env_racf::{
    MemorySecretResolver, PrincipalAuthenticationEpoch, RacfService, ResolvedSecret,
    SecretResolver, racf_providers,
};
use mainframe_env_spool::{SpoolRetentionClock, SpoolService, spool_providers};
use mainframe_env_store::{LocalArtifactStore, MemoryStore};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ArtifactStoreHealth, CheckpointStore, EffectDigestFormat,
    EffectState, ExecutionState, PlatformStore, ProviderStateMutation, ProviderStateRecord,
    ProviderStateStore, ProviderStateWrite, RetentionAgeReconciliation, RetentionArchive,
    RetentionArchivePruneOutcome, RetentionArchivePruneRequest, RetentionForecast,
    RetentionLegacyRow, RetentionReceipt, RetentionReconciliationReceipt, RetentionTarget,
    SaturationLevel, StoreError, WorkRecord, WorkState, WorkStore,
};
use mainframe_env_zosmf::{
    Authentication, GatewayCallContext, GatewayProblem, GatewayRequest, GatewayResponse,
    ZosmfBackend, ZosmfLimits,
};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_rustls::TlsAcceptor;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductMetrics {
    pub requests: u64,
    pub failures: u64,
    pub active: usize,
    pub sessions: usize,
    pub console_messages: usize,
    pub jes_workers: usize,
    pub jes_worker_healthy: usize,
    /// Successful JES queue polls, lease heartbeats, and terminal writes.
    pub jes_worker_progress: u64,
    /// Failed JES queue polls, lease heartbeats, joins, and terminal writes.
    pub jes_worker_failures: u64,
    pub jes_active: usize,
    pub outbox_pending: usize,
    pub outbox_delivered: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProductReadiness {
    pub accepting: bool,
    pub writable_store: bool,
    pub retention_capacity: ProductCapacityStatus,
    pub retention_warning: bool,
    pub bootstrap_identity: bool,
    pub host_capabilities: bool,
    pub artifact_store: bool,
    pub jes_workers: bool,
}

impl ProductReadiness {
    #[must_use]
    pub const fn ready(self) -> bool {
        self.accepting
            && self.writable_store
            && self.retention_capacity.accepts_traffic()
            && self.bootstrap_identity
            && self.host_capabilities
            && self.artifact_store
            && self.jes_workers
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProductCapacityStatus {
    Healthy,
    LowWatermark,
    HighWatermark,
    Full,
    Unavailable,
}

impl ProductCapacityStatus {
    const fn accepts_traffic(self) -> bool {
        matches!(self, Self::Healthy | Self::LowWatermark)
    }

    const fn warning(self) -> bool {
        matches!(self, Self::LowWatermark | Self::HighWatermark | Self::Full)
    }
}

impl From<SaturationLevel> for ProductCapacityStatus {
    fn from(value: SaturationLevel) -> Self {
        match value {
            SaturationLevel::Healthy => Self::Healthy,
            SaturationLevel::LowWatermark => Self::LowWatermark,
            SaturationLevel::HighWatermark => Self::HighWatermark,
            SaturationLevel::Full => Self::Full,
        }
    }
}

mod artifact;
pub use artifact::{BatchProgramDefinition, OnlineProgramDefinition};
mod bootstrap;
mod continuation;
mod interval_wakeup;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchInstallReceipt {
    pub programs: usize,
    pub identity: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPublicationReceipt {
    pub package: String,
    pub generation: u64,
    pub identity: String,
    pub controllers: usize,
    pub db2_catalog: bool,
    pub replayed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PublicationSectionState {
    NotApplicable,
    Pending,
    Applying,
    Applied,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PublicationAction {
    Install,
    Rollback,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ApplicationPublicationState {
    schema_version: String,
    package: String,
    generation: u64,
    identity: String,
    action: PublicationAction,
    controllers: PublicationSectionState,
    db2: PublicationSectionState,
    complete: bool,
}

struct DurableApplicationPublication {
    store_version: u64,
    state: ApplicationPublicationState,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalExchangeRecovery {
    Completed,
    HandoffCompleted,
    Cancelled,
    TimedOut,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OnlineExchangeState {
    schema_version: String,
    program: String,
    request_id: String,
    execution_id: String,
    run_unit_id: String,
    selector: String,
    artifact: String,
    principal: String,
    grants: BTreeSet<String>,
    provider_generations: BTreeMap<String, String>,
    priority: u8,
    deadline_tick: u64,
    trace_id: String,
    idempotency_key: String,
    attempt: u32,
    audit_correlation: String,
    transaction: String,
    commarea: Vec<u8>,
    aid: u8,
    blocking_effect: Option<String>,
    #[serde(skip)]
    version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthSession {
    schema_version: String,
    user: String,
    issued_tick: u64,
    last_used_tick: u64,
    absolute_expires_tick: u64,
    idle_expires_tick: u64,
    principal_epoch: String,
    version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthSessionIndex {
    schema_version: String,
    sessions: BTreeMap<String, String>,
    version: u64,
}

#[derive(Clone)]
struct VerifiedAuthentication {
    user: String,
    principal_epoch: PrincipalAuthenticationEpoch,
}

impl AuthSession {
    fn expired(&self, now_tick: u64) -> bool {
        now_tick >= self.absolute_expires_tick || now_tick >= self.idle_expires_tick
    }

    fn clock_regressed(&self, now_tick: u64) -> bool {
        now_tick < self.issued_tick || now_tick < self.last_used_tick
    }
}

struct ConsoleMessage {
    key: String,
    console: String,
    text: Vec<u8>,
}

struct SequenceState {
    next: u64,
    version: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JesWorkOutcome {
    Completed,
    Cancelled,
    Deferred,
}

struct EnterpriseReplayClock(Arc<dyn JesClock>);

impl Db2ReplayClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

impl ImsReplayClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

impl MqReplayClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

impl DatasetReplayClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

impl CicsReplayClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

impl SpoolRetentionClock for EnterpriseReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.now_tick().map_err(store_error)
    }
}

pub struct ProductServer {
    config: ServerConfig,
    pub(crate) store: Arc<dyn PlatformStore>,
    secrets: Arc<MemorySecretResolver>,
    racf: Arc<RacfService>,
    cics: Arc<CicsService>,
    dataset: Arc<DatasetService>,
    db2: Arc<Db2Service>,
    ims: Arc<ImsService>,
    mq: Arc<MqService>,
    spool: Arc<SpoolService>,
    pub(crate) batch: Arc<BatchService>,
    artifacts: Arc<ProductArtifactStore>,
    host: Arc<ScopedHostService>,
    program: Arc<DefaultProgramRouter>,
    applications: ApplicationInstaller,
    applications_v2: Mutex<DurableApplicationsV2>,
    application_publication: Mutex<()>,
    job_submission: Mutex<()>,
    online_programs: Mutex<BTreeMap<String, ArtifactRef>>,
    online_transactions: Mutex<BTreeMap<String, String>>,
    online_traces: Mutex<BTreeMap<String, Vec<CicsTraceEntry>>>,
    sessions: Mutex<BTreeMap<String, AuthSession>>,
    console: Mutex<Vec<ConsoleMessage>>,
    sequence: Mutex<SequenceState>,
    jes_clock: Arc<dyn JesClock>,
    jes_workers_started: AtomicBool,
    jes_workers_stopping: AtomicBool,
    pub(crate) jes_worker_notify: tokio::sync::Notify,
    jes_worker_handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    jes_worker_active: AtomicUsize,
    jes_worker_last_progress: Mutex<Vec<Option<Instant>>>,
    jes_worker_progress: AtomicU64,
    jes_worker_failures: AtomicU64,
    outbox_delivery: Mutex<()>,
    accepting: AtomicBool,
    requests: AtomicU64,
    failures: AtomicU64,
    active: AtomicUsize,
    outbox_delivered: AtomicU64,
}

enum ProductArtifactStore {
    Local(LocalArtifactStore),
    Shared(Arc<dyn ArtifactStore>),
}

impl ProductArtifactStore {
    fn is_ready(&self) -> bool {
        self.health().is_ok_and(ArtifactStoreHealth::ready)
    }
}

impl ArtifactStore for ProductArtifactStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        match self {
            Self::Local(store) => store.health(),
            Self::Shared(store) => store.health(),
        }
    }

    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        match self {
            Self::Local(store) => store.put_artifact(record),
            Self::Shared(store) => store.put_artifact(record),
        }
    }

    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        match self {
            Self::Local(store) => store.get_artifact(id),
            Self::Shared(store) => store.get_artifact(id),
        }
    }

    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        match self {
            Self::Local(store) => store.delete_artifact(id),
            Self::Shared(store) => store.delete_artifact(id),
        }
    }
}

const APPLICATION_V2_STATE_NAMESPACE: &str = "application-package-v2";
const APPLICATION_V2_STATE_KEY: &str = "registry";
const APPLICATION_PUBLICATION_NAMESPACE: &str = "application-publication-v2";
const APPLICATION_PUBLICATION_CONTRACT: &str = "mainframe-env.application-publication@1";
const AUTH_SESSION_NAMESPACE: &str = "auth-session-v2";
const AUTH_SESSION_INDEX_NAMESPACE: &str = "auth-session-index-v2";
const AUTH_SESSION_INDEX_KEY: &str = "global";
const LEGACY_AUTH_SESSION_NAMESPACE: &str = "auth-session";
const AUTH_SESSION_CONTRACT: &str = "mainframe-env.auth-session@3";
const AUTH_SESSION_INDEX_CONTRACT: &str = "mainframe-env.auth-session-index@2";
const ONLINE_EXCHANGE_NAMESPACE: &str = "online-exchange-v1";
const ONLINE_EXCHANGE_CONTRACT: &str = "mainframe-env.online-exchange@1";
const MAX_AUTH_SESSIONS: usize = 65_536;
const MAX_AUTH_SESSIONS_PER_USER: usize = 8;
const AUTH_SESSION_ABSOLUTE_TTL_MILLIS: u64 = 8 * 60 * 60 * 1000;
const AUTH_SESSION_IDLE_TTL_MILLIS: u64 = 30 * 60 * 1000;
static NEXT_JES_WORKER_POOL: AtomicU64 = AtomicU64::new(1);
pub(crate) const JES_ALLOWED_WORK_CAPABILITIES: [&str; 15] = [
    "host.cics.execute",
    "host.clock",
    "host.dataset.read",
    "host.dataset.write",
    "host.db2.read",
    "host.db2.write",
    "host.ims.read",
    "host.ims.write",
    "host.mq.read",
    "host.mq.write",
    "host.program.invoke",
    "host.security.authorize",
    "host.spool.read",
    "host.spool.write",
    "host.terminal",
];
const BOOTSTRAP_NAMESPACE: &str = "server-bootstrap";
const BOOTSTRAP_CLAIM_KEY: &str = "first-administrator-claim";
const BOOTSTRAP_KEY: &str = "first-administrator";
const BOOTSTRAP_CAS_ATTEMPTS: usize = 16;

thread_local! {
    static GATEWAY_CALL_CONTEXT: RefCell<Option<GatewayCallContext>> = const { RefCell::new(None) };
}

struct GatewayCallContextScope(Option<GatewayCallContext>);

impl GatewayCallContextScope {
    fn enter(context: GatewayCallContext) -> Self {
        Self(GATEWAY_CALL_CONTEXT.with(|current| current.replace(Some(context))))
    }
}

impl Drop for GatewayCallContextScope {
    fn drop(&mut self) {
        GATEWAY_CALL_CONTEXT.with(|current| {
            current.replace(self.0.take());
        });
    }
}

struct DurableApplicationsV2 {
    installer: ApplicationInstallerV2,
    store_version: u64,
    verifier: Arc<dyn PackageSignatureVerifier>,
}

pub struct HmacSha256PackageTrust {
    references: BTreeMap<String, SecretRef>,
    secrets: Arc<dyn SecretResolver>,
}

impl HmacSha256PackageTrust {
    pub fn new(
        references: BTreeMap<String, SecretRef>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, HostProblem> {
        if references.len() > 1_024
            || references.iter().any(|(key_id, _)| {
                key_id.is_empty() || key_id.len() > 128 || key_id.chars().any(char::is_control)
            })
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            references,
            secrets,
        })
    }

    pub fn from_environment(
        environment: &BTreeMap<String, String>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, HostProblem> {
        let Some(encoded) = environment.get("MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS") else {
            return Self::new(BTreeMap::new(), secrets);
        };
        let encoded: BTreeMap<String, String> =
            serde_json::from_str(encoded).map_err(|_| HostProblem::Malformed)?;
        let references = encoded
            .into_iter()
            .map(|(key_id, reference)| {
                EnvironmentSecretResolver::parse_reference(&reference)
                    .map(|reference| (key_id, reference))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Self::new(references, secrets)
    }
}

impl PackageSignatureVerifier for HmacSha256PackageTrust {
    fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
        if algorithm != "hmac-sha256@1" {
            return false;
        }
        let Some(reference) = self.references.get(key_id) else {
            return false;
        };
        let Ok(key) = self.secrets.resolve(reference) else {
            return false;
        };
        if key.len() < 32 || key.len() > 4_096 {
            return false;
        }
        let Ok(signature) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(signature)
        else {
            return false;
        };
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, &key),
            identity.as_bytes(),
            &signature,
        )
        .is_ok()
    }
}

struct RejectPackageTrust;

impl PackageSignatureVerifier for RejectPackageTrust {
    fn verify(&self, _: &str, _: &str, _: &str, _: &str) -> bool {
        false
    }
}

impl ProductServer {
    pub fn open(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_with_package_trust(
            config,
            store,
            secrets,
            program,
            Arc::new(RejectPackageTrust),
        )
    }

    pub fn open_with_package_trust(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
        package_trust: Arc<dyn PackageSignatureVerifier>,
    ) -> Result<Arc<Self>, HostProblem> {
        if config.artifact_profile != ArtifactProfile::Local {
            return Err(HostProblem::Malformed);
        }
        let artifacts = Arc::new(ProductArtifactStore::Local(
            LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024)
                .map_err(store_error)?,
        ));
        Self::open_configured(
            config,
            store,
            secrets,
            program,
            package_trust,
            artifacts,
            None,
        )
    }

    pub fn open_with_package_trust_and_artifact_store(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
        package_trust: Arc<dyn PackageSignatureVerifier>,
        artifacts: Arc<dyn ArtifactStore>,
    ) -> Result<Arc<Self>, HostProblem> {
        if config.artifact_profile != ArtifactProfile::Shared {
            return Err(HostProblem::Malformed);
        }
        Self::open_configured(
            config,
            store,
            secrets,
            program,
            package_trust,
            Arc::new(ProductArtifactStore::Shared(artifacts)),
            None,
        )
    }

    pub fn open_with_artifact_store(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
        artifacts: Arc<dyn ArtifactStore>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_with_package_trust_and_artifact_store(
            config,
            store,
            secrets,
            program,
            Arc::new(RejectPackageTrust),
            artifacts,
        )
    }

    fn open_configured(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
        package_trust: Arc<dyn PackageSignatureVerifier>,
        artifacts: Arc<ProductArtifactStore>,
        jes_clock: Option<Arc<dyn JesClock>>,
    ) -> Result<Arc<Self>, HostProblem> {
        config.validate()?;
        let installed = artifact::preflight_installed(store.as_ref(), artifacts.as_ref())?;
        let jes_clock: Arc<dyn JesClock> = match jes_clock {
            Some(clock) => clock,
            None => Arc::new(DurableJesClock::new(store.clone()).map_err(store_error)?),
        };
        let enterprise_replay_clock = Arc::new(EnterpriseReplayClock(jes_clock.clone()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let racf = RacfService::open(provider_store.clone(), secrets.clone(), Default::default())?;
        let dataset = DatasetService::open_with_replay_clock(
            provider_store.clone(),
            Default::default(),
            enterprise_replay_clock.clone(),
        )?;
        let enterprise_authorizer: Arc<dyn EnterpriseAuthorizer> = racf.clone();
        let db2 = Db2Service::open_authorized_with_replay_clock(
            provider_store.clone(),
            Default::default(),
            enterprise_authorizer.clone(),
            enterprise_replay_clock.clone(),
        )?;
        let ims = ImsService::open_authorized_with_replay_clock(
            provider_store.clone(),
            Default::default(),
            enterprise_authorizer.clone(),
            enterprise_replay_clock.clone(),
        )?;
        let mq = MqService::open_authorized_with_replay_clock(
            provider_store.clone(),
            Default::default(),
            enterprise_authorizer,
            enterprise_replay_clock.clone(),
        )?;
        let spool_artifacts: Arc<dyn ArtifactStore> = artifacts.clone();
        let spool = SpoolService::open_with_retention_clock(
            provider_store.clone(),
            spool_artifacts,
            Default::default(),
            enterprise_replay_clock.clone(),
        )?;
        let mut enterprise_providers = db2_providers(db2.clone(), InvocationLimits::default());
        enterprise_providers.extend(ims_providers(ims.clone(), InvocationLimits::default()));
        enterprise_providers.extend(mq_providers(mq.clone(), InvocationLimits::default()));
        let inner_program: Arc<dyn HostProvider> = program.clone();
        let inner = scoped_host(
            &racf,
            &dataset,
            inner_program,
            enterprise_providers,
            false,
            None,
        )?;
        let cics_work_store: Arc<dyn WorkStore> = store.clone();
        let cics = CicsService::open_with_runtime(
            inner,
            provider_store.clone(),
            cics_work_store,
            Default::default(),
            enterprise_replay_clock,
        )?;
        cics.bind_artifact_store(artifacts.clone())?;
        let mut enterprise_providers = db2_providers(db2.clone(), InvocationLimits::default());
        enterprise_providers.extend(ims_providers(ims.clone(), InvocationLimits::default()));
        enterprise_providers.extend(mq_providers(mq.clone(), InvocationLimits::default()));
        enterprise_providers.extend(spool_providers(spool.clone(), InvocationLimits::default()));
        let host = scoped_host(
            &racf,
            &dataset,
            program.clone(),
            enterprise_providers,
            true,
            Some(cics_provider(cics.clone(), InvocationLimits::default())),
        )?;
        let program_artifacts: Arc<dyn ArtifactStore> = artifacts.clone();
        program.bind_runtime(host.clone(), store.clone(), program_artifacts)?;
        let checkpoint_store: Arc<dyn CheckpointStore> = store.clone();
        let batch = BatchService::open_with_checkpoint_store(
            host.clone(),
            provider_store,
            checkpoint_store,
            Default::default(),
            BatchLimits {
                max_active: JES_WORKER_COUNT,
                ..BatchLimits::default()
            },
        )?;
        let (application_store_version, applications_v2) = match store
            .get_provider_state(APPLICATION_V2_STATE_NAMESPACE, APPLICATION_V2_STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => (
                record.version,
                ApplicationInstallerV2::from_state_payload(
                    "0.2.0",
                    PackageLimits::default(),
                    package_trust.clone(),
                    &record.payload,
                )
                .map_err(application_install_problem)?,
            ),
            None => (
                0,
                ApplicationInstallerV2::new(
                    "0.2.0",
                    PackageLimits::default(),
                    package_trust.clone(),
                ),
            ),
        };
        for row in store
            .list_provider_state(LEGACY_AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
            .map_err(store_error)?
        {
            store
                .delete_provider_state(LEGACY_AUTH_SESSION_NAMESPACE, &row.key, row.version)
                .map_err(store_error)?;
        }
        let mut sessions = BTreeMap::new();
        let now_tick = jes_clock.now_tick().map_err(store_error)?;
        let active_principals = racf.active_principal_epochs()?;
        let stored_sessions = store
            .list_provider_state(AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
            .map_err(store_error)?;
        for row in stored_sessions {
            let session = decode_auth_session(&row)?;
            let valid_principal = active_principals
                .get(&session.user)
                .is_some_and(|epoch| epoch.as_str() == session.principal_epoch.as_str());
            if session.expired(now_tick) || session.clock_regressed(now_tick) || !valid_principal {
                store
                    .delete_provider_state(AUTH_SESSION_NAMESPACE, &row.key, row.version)
                    .map_err(store_error)?;
            } else if sessions.insert(row.key, session).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        reconcile_auth_session_index(&*store)?;
        let console_rows = store
            .list_provider_state("console-log", 65_537)
            .map_err(store_error)?;
        if console_rows.len() > 65_536 {
            return Err(HostProblem::ResourceExhausted);
        }
        let console = decode_console_log_rows(&console_rows)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .into_iter()
            .map(|entry| ConsoleMessage {
                key: entry.key,
                console: entry.console,
                text: entry.text,
            })
            .collect();
        let online_programs = installed.online_programs;
        let online_transactions = installed.online_transactions;
        cics.register_programs(&online_programs.keys().cloned().collect())?;
        let sequence = match store
            .get_provider_state("server-meta", "next-sequence")
            .map_err(store_error)?
        {
            Some(row) => {
                let next = u64::from_be_bytes(
                    row.payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                );
                if next == 0 {
                    return Err(HostProblem::InfrastructureFailure);
                }
                SequenceState {
                    next,
                    version: Some(row.version),
                }
            }
            None => SequenceState {
                next: 1,
                version: None,
            },
        };
        let product = Arc::new(Self {
            config,
            store,
            secrets,
            racf,
            cics,
            dataset,
            db2,
            ims,
            mq,
            spool,
            batch,
            artifacts,
            host,
            program,
            applications: ApplicationInstaller::new("0.1.1"),
            applications_v2: Mutex::new(DurableApplicationsV2 {
                installer: applications_v2,
                store_version: application_store_version,
                verifier: package_trust,
            }),
            application_publication: Mutex::new(()),
            job_submission: Mutex::new(()),
            online_programs: Mutex::new(online_programs),
            online_transactions: Mutex::new(online_transactions),
            online_traces: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(sessions),
            console: Mutex::new(console),
            sequence: Mutex::new(sequence),
            jes_clock,
            jes_workers_started: AtomicBool::new(false),
            jes_workers_stopping: AtomicBool::new(false),
            jes_worker_notify: tokio::sync::Notify::new(),
            jes_worker_handles: Mutex::new(Vec::new()),
            jes_worker_active: AtomicUsize::new(0),
            jes_worker_last_progress: Mutex::new(vec![None; JES_WORKER_COUNT]),
            jes_worker_progress: AtomicU64::new(0),
            jes_worker_failures: AtomicU64::new(0),
            outbox_delivery: Mutex::new(()),
            accepting: AtomicBool::new(true),
            requests: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            active: AtomicUsize::new(0),
            outbox_delivered: AtomicU64::new(0),
        });
        product.recover_application_publications()?;
        product.recover_local_wakeups()?;
        Ok(product)
    }

    pub fn memory(config: ServerConfig) -> Result<Arc<Self>, HostProblem> {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let program = default_program_router();
        Self::open(config, store, secrets, program)
    }

    pub fn memory_with_package_trust(
        config: ServerConfig,
        package_trust: Arc<dyn PackageSignatureVerifier>,
    ) -> Result<Arc<Self>, HostProblem> {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let program = default_program_router();
        Self::open_with_package_trust(config, store, secrets, program, package_trust)
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
    pub fn db2_service(&self) -> Arc<Db2Service> {
        self.db2.clone()
    }

    #[must_use]
    pub fn ims_service(&self) -> Arc<ImsService> {
        self.ims.clone()
    }

    #[must_use]
    pub fn mq_service(&self) -> Arc<MqService> {
        self.mq.clone()
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
        let mut artifact_records = Vec::with_capacity(definition.programs.len());
        let mut identity = Sha256::new();
        for definition in &definition.programs {
            let name = normalize_online_name(&definition.name, 128)?;
            if programs
                .insert(name.clone(), definition.artifact.clone())
                .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let record = artifact::admitted_record(
                &definition.artifact,
                &definition.payload,
                &definition.manifest,
                &definition.semantic_identity,
            )?;
            if let Some(existing) = artifact_records
                .iter()
                .find(|existing: &&ArtifactRecord| existing.artifact == record.artifact)
            {
                if existing != &record {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                artifact_records.push(record);
            }
            digest_online_field(&mut identity, name.as_bytes());
            digest_online_field(&mut identity, definition.artifact.as_str().as_bytes());
        }
        for record in artifact_records {
            self.artifacts.put_artifact(record).map_err(store_error)?;
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

    pub fn install_batch_programs(
        &self,
        definitions: Vec<BatchProgramDefinition>,
    ) -> Result<BatchInstallReceipt, HostProblem> {
        if definitions.is_empty() || definitions.len() > 4096 {
            return Err(HostProblem::Malformed);
        }
        let mut programs = BTreeMap::new();
        let mut artifact_records = Vec::with_capacity(definitions.len());
        let mut identity = Sha256::new();
        for definition in &definitions {
            let name = normalize_online_name(&definition.name, 128)?;
            if programs
                .insert(name.clone(), definition.artifact.clone())
                .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let record = artifact::admitted_record(
                &definition.artifact,
                &definition.payload,
                &definition.manifest,
                &definition.semantic_identity,
            )?;
            if let Some(existing) = artifact_records
                .iter()
                .find(|existing: &&ArtifactRecord| existing.artifact == record.artifact)
            {
                if existing != &record {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                artifact_records.push(record);
            }
            digest_online_field(&mut identity, name.as_bytes());
            digest_online_field(&mut identity, definition.artifact.as_str().as_bytes());
        }
        for record in artifact_records {
            self.artifacts.put_artifact(record).map_err(store_error)?;
        }
        let mut writes = Vec::new();
        for (name, artifact) in &programs {
            match self
                .store
                .get_provider_state("batch-program", name)
                .map_err(store_error)?
            {
                Some(current) if current.payload != artifact.as_str().as_bytes() => {
                    return Err(HostProblem::IdempotencyConflict);
                }
                Some(_) => {}
                None => writes.push(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "batch-program".into(),
                        key: name.clone(),
                        version: 1,
                        payload: artifact.as_str().as_bytes().to_vec(),
                    },
                    expected_version: None,
                }),
            }
        }
        let replayed = writes.is_empty();
        if !replayed {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        Ok(BatchInstallReceipt {
            programs: definitions.len(),
            identity: format!("sha256:{:x}", identity.finalize()),
            replayed,
        })
    }

    pub fn install_application_package_v2(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<ApplicationGenerationRecord, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let before = durable
            .installer
            .state_payload()
            .map_err(application_install_problem)?;
        let record = durable
            .installer
            .stage(package)
            .map_err(application_install_problem)?;
        if let Err(problem) = self.persist_application_installer(&mut durable) {
            let verifier = durable.verifier.clone();
            durable.installer = ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                verifier,
                &before,
            )
            .map_err(application_install_problem)?;
            return Err(problem);
        }
        Ok(record)
    }

    pub fn publish_application_generation(
        &self,
        expected: &ApplicationGenerationRecord,
    ) -> Result<ApplicationPublicationReceipt, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let selected = self.application_generation_v2(expected)?;
        let package = selected.package().clone();
        let key = package.base.manifest.name.to_ascii_uppercase();
        let db2_applicable = package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog");
        let existing = self
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, &key)
            .map_err(store_error)?;
        let mut durable = match existing {
            Some(record) => {
                let state: ApplicationPublicationState = serde_json::from_slice(&record.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if state.schema_version != APPLICATION_PUBLICATION_CONTRACT
                    || state.package.to_ascii_uppercase() != key
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                if state.action == PublicationAction::Install
                    && state.generation == expected.generation
                    && state.identity == expected.identity
                {
                    DurableApplicationPublication {
                        store_version: record.version,
                        state,
                    }
                } else if state.complete && state.generation < expected.generation {
                    let mut durable = DurableApplicationPublication {
                        store_version: record.version,
                        state: install_publication_state(&package, expected, db2_applicable),
                    };
                    self.persist_application_publication(&mut durable)?;
                    durable
                } else {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
            None => {
                let mut durable = DurableApplicationPublication {
                    store_version: 0,
                    state: install_publication_state(&package, expected, db2_applicable),
                };
                self.persist_application_publication(&mut durable)?;
                durable
            }
        };
        if durable.state.complete {
            self.selected_application_v2(expected)?;
            return Ok(ApplicationPublicationReceipt {
                package: package.base.manifest.name,
                generation: package.generation,
                identity: expected.identity.clone(),
                controllers: package.sections.batch_controllers.len(),
                db2_catalog: db2_applicable,
                replayed: true,
            });
        }

        let controllers = if durable.state.controllers == PublicationSectionState::Applied {
            package.sections.batch_controllers.len()
        } else {
            durable.state.controllers = PublicationSectionState::Applying;
            self.persist_application_publication(&mut durable)?;
            match self.apply_application_batch_controllers(&selected) {
                Ok(receipt) => {
                    durable.state.controllers = PublicationSectionState::Applied;
                    self.persist_application_publication(&mut durable)?;
                    receipt.controllers
                }
                Err(problem) => {
                    durable.state.controllers = PublicationSectionState::Failed;
                    self.persist_application_publication(&mut durable)?;
                    return Err(problem);
                }
            }
        };

        if db2_applicable && durable.state.db2 != PublicationSectionState::Applied {
            durable.state.db2 = PublicationSectionState::Applying;
            self.persist_application_publication(&mut durable)?;
            if let Err(problem) = self.apply_application_db2_catalog(&selected) {
                durable.state.db2 = PublicationSectionState::Failed;
                self.persist_application_publication(&mut durable)?;
                return Err(problem);
            }
            durable.state.db2 = PublicationSectionState::Applied;
            self.persist_application_publication(&mut durable)?;
        }

        self.commit_application_generation(&package)?;
        durable.state.complete = true;
        self.persist_application_publication(&mut durable)?;
        Ok(ApplicationPublicationReceipt {
            package: package.base.manifest.name,
            generation: package.generation,
            identity: expected.identity.clone(),
            controllers,
            db2_catalog: db2_applicable,
            replayed: false,
        })
    }

    pub fn rollback_application_generation(
        &self,
        expected: &ApplicationGenerationRecord,
    ) -> Result<ApplicationPublicationReceipt, HostProblem> {
        let _publication = self
            .application_publication
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let selected = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .retained_generation(&expected.package, expected.generation, &expected.identity)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if selected.record().version != expected.version {
            return Err(HostProblem::IdempotencyConflict);
        }
        let package = selected.package().clone();
        let key = package.base.manifest.name.to_ascii_uppercase();
        let db2_applicable = package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog");
        let existing = self
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, &key)
            .map_err(store_error)?;
        let mut durable = if let Some(record) = existing {
            let state: ApplicationPublicationState = serde_json::from_slice(&record.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if state.action == PublicationAction::Rollback
                && state.generation == expected.generation
                && state.identity == expected.identity
            {
                DurableApplicationPublication {
                    store_version: record.version,
                    state,
                }
            } else {
                DurableApplicationPublication {
                    store_version: record.version,
                    state: rollback_publication_state(&package, expected, db2_applicable),
                }
            }
        } else {
            DurableApplicationPublication {
                store_version: 0,
                state: rollback_publication_state(&package, expected, db2_applicable),
            }
        };
        if durable.state.complete {
            self.selected_application_v2(expected)?;
            return Ok(ApplicationPublicationReceipt {
                package: package.base.manifest.name,
                generation: package.generation,
                identity: expected.identity.clone(),
                controllers: package.sections.batch_controllers.len(),
                db2_catalog: db2_applicable,
                replayed: true,
            });
        }
        self.persist_application_publication(&mut durable)?;

        if durable.state.controllers != PublicationSectionState::Applied {
            durable.state.controllers = PublicationSectionState::Applying;
            self.persist_application_publication(&mut durable)?;
            if let Err(problem) = self
                .batch
                .rollback_controllers(&package.base.manifest.name, package.generation)
            {
                durable.state.controllers = PublicationSectionState::Failed;
                self.persist_application_publication(&mut durable)?;
                return Err(problem);
            }
            durable.state.controllers = PublicationSectionState::Applied;
            self.persist_application_publication(&mut durable)?;
        }
        if db2_applicable && durable.state.db2 != PublicationSectionState::Applied {
            durable.state.db2 = PublicationSectionState::Applying;
            self.persist_application_publication(&mut durable)?;
            if let Err(problem) = self
                .db2
                .rollback_catalog(&package.base.manifest.name, package.generation)
            {
                durable.state.db2 = PublicationSectionState::Failed;
                self.persist_application_publication(&mut durable)?;
                return Err(problem);
            }
            durable.state.db2 = PublicationSectionState::Applied;
            self.persist_application_publication(&mut durable)?;
        }
        self.select_application_generation(&package.base.manifest.name, package.generation)?;
        durable.state.complete = true;
        self.persist_application_publication(&mut durable)?;
        Ok(ApplicationPublicationReceipt {
            package: package.base.manifest.name,
            generation: package.generation,
            identity: expected.identity.clone(),
            controllers: package.sections.batch_controllers.len(),
            db2_catalog: db2_applicable,
            replayed: false,
        })
    }

    fn apply_application_batch_controllers(
        &self,
        selected: &SelectedApplicationGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let package = selected.package();
        let controllers = package
            .sections
            .batch_controllers
            .iter()
            .map(|controller| decode_application_batch_controller(package, controller))
            .collect::<Result<Vec<_>, _>>()?;
        self.batch.install_controllers(BatchControllerGeneration {
            schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: package.base.manifest.name.clone(),
            generation: package.generation,
            identity: selected.record().identity.clone(),
            controllers,
        })
    }

    fn apply_application_db2_catalog(
        &self,
        selected: &SelectedApplicationGeneration,
    ) -> Result<(), HostProblem> {
        let package = selected.package();
        let catalog_entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog")
            .ok_or(HostProblem::Malformed)?;
        let catalog_blob = package
            .base
            .blobs
            .get(&catalog_entry.sha256)
            .ok_or(HostProblem::Malformed)?;
        let tables = decode_table_definitions_bounded(catalog_blob, Db2Limits::default())?;
        let declared = package
            .sections
            .sql_tables
            .iter()
            .map(|table| {
                (
                    table.name.to_ascii_uppercase(),
                    table
                        .columns
                        .iter()
                        .map(|column| (column.name.to_ascii_uppercase(), column.nullable))
                        .collect::<Vec<_>>(),
                    table
                        .primary_key
                        .iter()
                        .map(|column| column.to_ascii_uppercase())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeSet<_>>();
        let signed = tables
            .iter()
            .map(|table| {
                (
                    table.name.to_ascii_uppercase(),
                    table
                        .columns
                        .iter()
                        .map(|column| (column.name.to_ascii_uppercase(), column.nullable))
                        .collect::<Vec<_>>(),
                    table
                        .primary_key
                        .iter()
                        .map(|column| column.to_ascii_uppercase())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeSet<_>>();
        if declared != signed {
            return Err(HostProblem::Malformed);
        }
        let rows = package
            .sections
            .sql_rows
            .iter()
            .map(|row| Db2SeedRow {
                table: row.table.clone(),
                values: row
                    .values
                    .iter()
                    .map(|(name, value)| (name.clone(), value.as_bytes().to_vec()))
                    .collect(),
            })
            .collect();
        self.db2.install_catalog(Db2CatalogGeneration {
            application: package.base.manifest.name.clone(),
            generation: package.generation,
            identity: selected.record().identity.clone(),
            tables,
            rows,
        })
    }

    fn application_generation_v2(
        &self,
        expected: &ApplicationGenerationRecord,
    ) -> Result<SelectedApplicationGeneration, HostProblem> {
        let generation = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .generation(&expected.package, expected.generation, &expected.identity)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if generation.record().package != expected.package
            || generation.record().version != expected.version
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(generation)
    }

    fn commit_application_generation(
        &self,
        package: &ApplicationPackageV2,
    ) -> Result<(), HostProblem> {
        let mut durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let before = durable
            .installer
            .state_payload()
            .map_err(application_install_problem)?;
        durable
            .installer
            .commit(package)
            .map_err(application_install_problem)?;
        if let Err(problem) = self.persist_application_installer(&mut durable) {
            let verifier = durable.verifier.clone();
            durable.installer = ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                verifier,
                &before,
            )
            .map_err(application_install_problem)?;
            return Err(problem);
        }
        Ok(())
    }

    fn select_application_generation(
        &self,
        application: &str,
        generation: u64,
    ) -> Result<(), HostProblem> {
        let mut durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let before = durable
            .installer
            .state_payload()
            .map_err(application_install_problem)?;
        durable
            .installer
            .rollback(application, generation)
            .map_err(application_install_problem)?;
        if let Err(problem) = self.persist_application_installer(&mut durable) {
            let verifier = durable.verifier.clone();
            durable.installer = ApplicationInstallerV2::from_state_payload(
                "0.2.0",
                PackageLimits::default(),
                verifier,
                &before,
            )
            .map_err(application_install_problem)?;
            return Err(problem);
        }
        Ok(())
    }

    fn persist_application_installer(
        &self,
        durable: &mut DurableApplicationsV2,
    ) -> Result<(), HostProblem> {
        let payload = durable
            .installer
            .state_payload()
            .map_err(application_install_problem)?;
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: APPLICATION_V2_STATE_NAMESPACE.into(),
                    key: APPLICATION_V2_STATE_KEY.into(),
                    version,
                    payload,
                },
                (durable.store_version != 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        Ok(())
    }

    fn persist_application_publication(
        &self,
        durable: &mut DurableApplicationPublication,
    ) -> Result<(), HostProblem> {
        let payload =
            serde_json::to_vec(&durable.state).map_err(|_| HostProblem::InfrastructureFailure)?;
        if payload.len() > 64 * 1024 {
            return Err(HostProblem::ResourceExhausted);
        }
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
                    key: durable.state.package.to_ascii_uppercase(),
                    version,
                    payload,
                },
                (durable.store_version != 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        Ok(())
    }

    fn recover_application_publications(&self) -> Result<(), HostProblem> {
        for record in self
            .store
            .list_provider_state(APPLICATION_PUBLICATION_NAMESPACE, 1_024)
            .map_err(store_error)?
        {
            if record.payload.len() > 64 * 1024 {
                return Err(HostProblem::ResourceExhausted);
            }
            let state: ApplicationPublicationState = serde_json::from_slice(&record.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if state.schema_version != APPLICATION_PUBLICATION_CONTRACT
                || state.package.to_ascii_uppercase() != record.key
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let retained = self
                .applications_v2
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .installer
                .generation(&state.package, state.generation, &state.identity)
                .map_err(application_install_problem)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let expected = retained.record().clone();
            drop(retained);
            if !state.complete {
                match state.action {
                    PublicationAction::Install => {
                        self.publish_application_generation(&expected)?;
                    }
                    PublicationAction::Rollback => {
                        self.rollback_application_generation(&expected)?;
                    }
                }
            } else {
                self.selected_application_v2(&expected)?;
            }
        }
        Ok(())
    }

    fn selected_application_v2(
        &self,
        expected: &ApplicationGenerationRecord,
    ) -> Result<SelectedApplicationGeneration, HostProblem> {
        let selected = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .installer
            .selected_generation(&expected.package)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if selected.record().generation != expected.generation
            || selected.record().identity != expected.identity
            || selected.record().state != InstallState::Ready
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(selected)
    }

    fn begin_online_exchange(
        &self,
        session: &SessionId,
        program: &str,
        context: &CicsTerminalExecution,
    ) -> Result<OnlineExchangeState, HostProblem> {
        let state = OnlineExchangeState {
            schema_version: ONLINE_EXCHANGE_CONTRACT.into(),
            program: normalize_online_name(program, 128)?,
            request_id: context.invocation.request_id.as_str().into(),
            execution_id: context.invocation.execution_id.as_str().into(),
            run_unit_id: context.invocation.run_unit_id.as_str().into(),
            selector: context.invocation.selector.as_str().into(),
            artifact: context.invocation.artifact.as_str().into(),
            principal: context.invocation.principal.id().as_str().into(),
            grants: context
                .invocation
                .principal
                .grants()
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect(),
            provider_generations: context
                .invocation
                .provider_generations
                .iter()
                .map(|(capability, generation)| {
                    (capability.as_str().to_string(), generation.clone())
                })
                .collect(),
            priority: context.invocation.priority,
            deadline_tick: context.invocation.deadline_tick,
            trace_id: context.invocation.trace_id.as_str().into(),
            idempotency_key: context.invocation.idempotency_key.as_str().into(),
            attempt: context.invocation.attempt,
            audit_correlation: context.invocation.audit_correlation.clone(),
            transaction: normalize_online_name(&context.transaction, 16)?,
            commarea: context.commarea.clone(),
            aid: context.aid,
            blocking_effect: None,
            version: 1,
        };
        validate_online_exchange(&state)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: session.as_str().into(),
                    version: state.version,
                    payload: encode_online_exchange(&state)?,
                },
                None,
            )
            .map_err(store_error)?;
        Ok(state)
    }

    fn clear_online_exchange(
        &self,
        session: &SessionId,
        state: &OnlineExchangeState,
    ) -> Result<(), HostProblem> {
        self.store
            .delete_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str(), state.version)
            .map_err(store_error)
    }

    fn online_exchange_invocation(
        &self,
        state: &OnlineExchangeState,
    ) -> Result<Invocation, HostProblem> {
        validate_online_exchange(state)?;
        let limits = InvocationLimits::default();
        let grants = state
            .grants
            .iter()
            .map(|capability| {
                CapabilityId::new(capability, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let generations = state
            .provider_generations
            .iter()
            .map(|(capability, generation)| {
                Ok((
                    CapabilityId::new(capability, limits)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                    generation.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, HostProblem>>()?;
        let deadline_tick = current_gateway_call_context()
            .map_or(state.deadline_tick, |context| context.deadline_tick());
        let mut invocation = Invocation::new(
            RequestId::new(&state.request_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(&state.execution_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(&state.run_unit_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            None,
            Selector::new(&state.selector, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ArtifactRef::new(&state.artifact, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            Principal::new(
                PrincipalId::new(&state.principal, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                grants,
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ServiceClass::Interactive,
            state.priority,
            deadline_tick,
            TraceId::new(&state.trace_id, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(&state.idempotency_key, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state.attempt,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .and_then(|invocation| invocation.with_provider_generations(generations, limits))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        invocation
            .audit_correlation
            .clone_from(&state.audit_correlation);
        if let Some(context) = current_gateway_call_context() {
            invocation = invocation.with_cancellation_probe(context.cancellation_probe());
        }
        Ok(invocation)
    }

    fn online_exchange_blocked(&self, state: &OnlineExchangeState) -> Result<bool, HostProblem> {
        let Some(key) = state.blocking_effect.as_deref() else {
            return Ok(false);
        };
        let key = IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let effect = self
            .store
            .effect(&key)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        Ok(matches!(
            effect.state,
            EffectState::Intent | EffectState::UnknownOutcome
        ))
    }

    fn reconcile_online_exchange(
        &self,
        session: &SessionId,
        state: &mut OnlineExchangeState,
    ) -> Result<bool, HostProblem> {
        let Some(key) = state.blocking_effect.as_deref() else {
            return Ok(true);
        };
        let key = IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let effect = self
            .store
            .effect(&key)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if effect.execution_id.as_str() != state.execution_id
            || effect.run_unit_id.as_str() != state.run_unit_id
            || effect.digest_format != EffectDigestFormat::CanonicalHostV1
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        match effect.state {
            EffectState::Completed => {}
            EffectState::UnknownOutcome => {
                let result_digest = self
                    .cics
                    .reconciled_effect_result_digest(&key, effect.request_digest)?;
                self.store
                    .reconcile_unknown_versioned(
                        &key,
                        EffectState::Completed,
                        EffectDigestFormat::CanonicalHostV1,
                        result_digest,
                    )
                    .map_err(store_error)?;
            }
            EffectState::Intent | EffectState::Failed => return Ok(false),
        }
        state.blocking_effect = None;
        self.persist_online_exchange(session, state)?;
        Ok(true)
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

    fn clear_execution_checkpoint(&self, execution_id: &ExecutionId) -> Result<(), HostProblem> {
        match self.store.delete_checkpoint(execution_id) {
            Ok(()) | Err(StoreError::NotFound) => Ok(()),
            Err(problem) => Err(store_error(problem)),
        }
    }

    /// Recover the gap between execution terminalization and product/CICS
    /// cleanup. A handoff completion retains the product-owned continuation;
    /// every other terminal outcome discards it before a fresh task may start.
    fn recover_terminal_online_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        exchange: &OnlineExchangeState,
        now_tick: u64,
    ) -> Result<Option<TerminalExchangeRecovery>, HostProblem> {
        let invocation = self.online_exchange_invocation(exchange)?;
        let Some(execution) = self
            .store
            .get_execution(&invocation.execution_id)
            .map_err(store_error)?
        else {
            // The exchange is persisted before coordinator admission. A crash
            // in that narrow gap is safe to resume under the same identity.
            return Ok(None);
        };
        if !execution.state.terminal() {
            return Ok(None);
        }
        let last = self
            .store
            .events(&invocation.execution_id, execution.version, 1)
            .map_err(store_error)?
            .into_iter()
            .next()
            .filter(|event| event.sequence == execution.version)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let preserve_handoff = matches!(last.kind, LifecycleEventKind::HandoffCompleted);
        let recovered = match execution.state {
            ExecutionState::Completed if preserve_handoff => {
                TerminalExchangeRecovery::HandoffCompleted
            }
            ExecutionState::Completed => TerminalExchangeRecovery::Completed,
            ExecutionState::Cancelled => TerminalExchangeRecovery::Cancelled,
            ExecutionState::TimedOut => TerminalExchangeRecovery::TimedOut,
            ExecutionState::Failed | ExecutionState::DeadLetter => TerminalExchangeRecovery::Failed,
            ExecutionState::Admitted
            | ExecutionState::Queued
            | ExecutionState::Running
            | ExecutionState::Suspended
            | ExecutionState::Completing => return Err(HostProblem::InfrastructureFailure),
        };
        let saved = self.online_machine_continuation(session)?;
        let missing_handoff = preserve_handoff && saved.is_none();

        // Evaluate every cleanup before propagating the first failure. This
        // prevents a recoverable stale row from repeatedly blocking a session.
        let program_result = self.program.finish_run_unit(&invocation);
        let cics_result = self.discard_online_machine_run_if_present(
            session,
            principal,
            now_tick,
            preserve_handoff,
        );
        let continuation_result = if preserve_handoff {
            Ok(())
        } else {
            self.clear_online_machine_continuation(
                session,
                saved.as_ref().map(|continuation| continuation.version),
            )
        };
        let checkpoint_result = self.clear_execution_checkpoint(&invocation.execution_id);
        let exchange_result = self.clear_online_exchange(session, exchange);
        for result in [
            program_result,
            continuation_result,
            checkpoint_result,
            cics_result,
            exchange_result,
        ] {
            result?;
        }
        if missing_handoff {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(recovered))
    }

    fn finish_known_online_failure(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        invocation: &Invocation,
        now_tick: u64,
        saved_version: Option<u64>,
        exchange: &OnlineExchangeState,
    ) -> Result<(), HostProblem> {
        let results = [
            self.program.finish_run_unit(invocation),
            self.clear_online_machine_continuation(session, saved_version),
            self.clear_execution_checkpoint(&invocation.execution_id),
            self.abort_online_machine_run(session, principal, now_tick),
            self.clear_online_exchange(session, exchange),
        ];
        for result in results {
            result?;
        }
        Ok(())
    }

    fn run_online_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let mut saved = self.preflight_online_continuation(session, None)?;
        if saved.is_none() {
            self.preflight_online_program(program)?;
        }
        let mut exchange = self.online_exchange(session)?;
        self.recover_pending_online_transfer_if_present(
            session,
            &mut saved,
            &mut exchange,
            now_tick,
        )?;
        if let Some(state) = exchange.as_ref() {
            self.preflight_online_exchange_state(state)?;
            let expected_program = saved
                .as_ref()
                .map_or(program, |continuation| continuation.program.as_str());
            if state.principal != principal.as_str()
                || state.program != normalize_online_name(expected_program, 128)?
            {
                return Err(HostProblem::Unauthorized);
            }
            if let Some(recovered) =
                self.recover_terminal_online_exchange(session, principal, state, now_tick)?
            {
                return match recovered {
                    TerminalExchangeRecovery::Completed
                    | TerminalExchangeRecovery::HandoffCompleted => Ok(()),
                    TerminalExchangeRecovery::Cancelled => Err(HostProblem::Cancelled),
                    TerminalExchangeRecovery::TimedOut => Err(HostProblem::TimedOut),
                    TerminalExchangeRecovery::Failed => Err(HostProblem::ProviderFailure),
                };
            }
        }
        let blocked = exchange
            .as_ref()
            .map(|state| self.online_exchange_blocked(state))
            .transpose()?
            .unwrap_or(false);
        if blocked
            && !self.reconcile_online_exchange(
                session,
                exchange
                    .as_mut()
                    .ok_or(HostProblem::InfrastructureFailure)?,
            )?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let context = match exchange.as_ref() {
            Some(state) => {
                let mut invocation = self.online_exchange_invocation(state)?;
                continuation::restore_online_machine_priority(&mut invocation, saved.as_ref());
                self.cics.restore_terminal_run(
                    invocation.clone(),
                    session,
                    &state.transaction,
                    state.commarea.clone(),
                    now_tick,
                )?;
                CicsTerminalExecution {
                    invocation,
                    transaction: state.transaction.clone(),
                    commarea: state.commarea.clone(),
                    aid: state.aid,
                }
            }
            None => {
                let context = self.cics.terminal_execution(session, principal, now_tick)?;
                let exchange_program = saved
                    .as_ref()
                    .map_or(program, |continuation| continuation.program.as_str());
                exchange = Some(self.begin_online_exchange(session, exchange_program, &context)?);
                context
            }
        };
        let mut invocation = context.invocation;
        if let Some(continuation) = saved.as_ref() {
            invocation
                .provider_generations
                .clone_from(&continuation.provider_generations);
        }
        continuation::restore_online_machine_context(&mut invocation, saved.as_ref())?;
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
        bind_compatible_runtime_services(&mut invocation)?;
        let mut saved_version = saved.as_ref().map(|saved| saved.version);
        let mut saved_checkpoint = saved.as_ref().map(|saved| saved.checkpoint.clone());
        let mut current = normalize_online_name(
            saved
                .as_ref()
                .map_or(program, |saved| saved.program.as_str()),
            128,
        )?;
        let mut root_idempotency = invocation.idempotency_key.as_str().to_string();
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
            if frame == 0
                && saved
                    .as_ref()
                    .is_some_and(|continuation| continuation.artifact != artifact)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let executable = admit_executable_artifact(&record)?;
            invocation.selector =
                Selector::new(format!("program:{current}"), InvocationLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            invocation.artifact = artifact.clone();
            invocation.idempotency_key = IdempotencyKey::new(
                format!("{root_idempotency}:{current}"),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            let mut machine = ReferenceMachine::from_binary(
                executable.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .map_err(|_| HostProblem::ProviderFailure)?;
            if let Some(checkpoint) = saved_checkpoint.take() {
                machine
                    .restore_checkpoint(&checkpoint)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            }
            let coordinator = ExecutionCoordinator::durable(
                self.host.clone(),
                self.store.clone(),
                CoordinatorLimits {
                    max_quanta: 100,
                    ..CoordinatorLimits::default()
                },
            );
            match coordinator.execute_resumable_with_control(&mut machine, &invocation, || {
                self.program.observe_execution_control(&invocation)
            }) {
                ExecutionOutcome::Completed(_) => {
                    self.program.finish_run_unit(&invocation)?;
                    self.clear_online_machine_continuation(session, saved_version)?;
                    self.finish_online_machine_run(session, principal, now_tick)?;
                    self.clear_online_exchange(
                        session,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Ok(());
                }
                ExecutionOutcome::Suspended(suspension) => {
                    return self.finish_online_suspension(
                        session,
                        principal,
                        &current,
                        &artifact,
                        &invocation,
                        &machine,
                        saved_version,
                        &suspension,
                        &coordinator,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        now_tick,
                    );
                }
                ExecutionOutcome::Transfer(transfer) if transfer.replace_frame => {
                    let (next_program, next_artifact) =
                        self.preflight_online_program(transfer.selector.as_str())?;
                    let (next, checkpoint, next_version) = self.stage_online_program_transfer(
                        session,
                        &next_program,
                        &next_artifact,
                        &invocation,
                        &transfer.payload,
                        saved_version,
                        exchange
                            .as_mut()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        &coordinator,
                        now_tick,
                    )?;
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
                    current = next_program;
                    invocation = next;
                    root_idempotency = invocation.idempotency_key.as_str().into();
                    saved_checkpoint = Some(checkpoint);
                    saved_version = Some(next_version);
                }
                ExecutionOutcome::Condition(condition) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: condition.name,
                        response: condition.response,
                        response2: condition.response2,
                    });
                }
                ExecutionOutcome::Abend(abend) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: abend.code,
                        response: -1,
                        response2: 0,
                    });
                }
                ExecutionOutcome::TimedOut => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::TimedOut);
                }
                ExecutionOutcome::Cancelled => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Cancelled);
                }
                ExecutionOutcome::ResourceExhausted(problem) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
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
                    if problem.has_unknown_outcome() {
                        let key = IdempotencyKey::new(
                            format!(
                                "{}:{}",
                                invocation.idempotency_key,
                                machine.effect_sequence()
                            ),
                            InvocationLimits::default(),
                        )
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                        if self
                            .store
                            .effect(&key)
                            .map_err(store_error)?
                            .is_some_and(|effect| {
                                matches!(
                                    effect.state,
                                    EffectState::Intent | EffectState::UnknownOutcome
                                )
                            })
                            && let Some(state) = exchange.as_mut()
                        {
                            state.blocking_effect = Some(key.as_str().into());
                            let _ = self.persist_online_exchange(session, state);
                        }
                        return Err(HostProblem::UnknownOutcome);
                    }
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
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
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
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
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Unsupported);
                }
            }
        }
        Err(HostProblem::ResourceExhausted)
    }

    pub fn bootstrap_user(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        self.bootstrap_identity(user, secret)?;
        self.grant_bootstrap_profiles(user)
    }

    /// Create the first administrator from caller-owned secret bytes.
    ///
    /// Committed and recoverable partial bootstraps do not read these bytes.
    /// A durable claim prevents concurrent starters from selecting different
    /// first administrators.
    pub fn bootstrap_administrator(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        self.bootstrap_administrator_with(user, || ResolvedSecret::new(secret.to_vec()))
    }

    /// Create the first administrator through a lazily resolved secret reference.
    ///
    /// The resolver is called only when no committed or recoverable partial
    /// bootstrap exists. This permits operators to remove one-time bootstrap
    /// material after the durable receipt has committed.
    pub fn bootstrap_administrator_from_reference(
        &self,
        user: &str,
        reference: &SecretRef,
        resolver: &dyn SecretResolver,
    ) -> Result<(), HostProblem> {
        self.bootstrap_administrator_with(user, || resolver.resolve(reference))
    }

    fn bootstrap_administrator_with(
        &self,
        user: &str,
        resolve: impl FnOnce() -> Result<ResolvedSecret, HostProblem>,
    ) -> Result<(), HostProblem> {
        let principal = PrincipalId::new(user.to_ascii_uppercase(), InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        if let Some(marker) = self.bootstrap_record(BOOTSTRAP_KEY)? {
            self.validate_bootstrap_record(&marker, BOOTSTRAP_KEY, &principal)?;
            if let Some(claim) = self.bootstrap_record(BOOTSTRAP_CLAIM_KEY)? {
                self.validate_bootstrap_record(&claim, BOOTSTRAP_CLAIM_KEY, &principal)?;
            }
            return self.require_bootstrap_administrator(&principal);
        }
        self.acquire_bootstrap_claim(&principal)?;
        if !self.racf.bootstrap_administrator_ready(&principal)? {
            let secret = resolve()?;
            let reference = SecretRef::new(
                format!(
                    "bootstrap-administrator:{}:{}",
                    principal.as_str(),
                    self.next_sequence()?
                ),
                HostLimits::default(),
            )?;
            let _scope = self.secrets.scoped(&reference, secret.to_vec())?;
            if let Err(problem) = self
                .racf
                .bootstrap_administrator(principal.as_str(), &reference)
                && (!matches!(
                    problem,
                    HostProblem::Unauthorized | HostProblem::IdempotencyConflict
                ) || !self.racf.bootstrap_administrator_ready(&principal)?)
            {
                return Err(problem);
            }
        }
        self.grant_bootstrap_profiles(principal.as_str())?;
        self.commit_bootstrap_marker(&principal)
    }

    fn bootstrap_record(&self, key: &str) -> Result<Option<ProviderStateRecord>, HostProblem> {
        self.store
            .get_provider_state(BOOTSTRAP_NAMESPACE, key)
            .map_err(store_error)
    }

    fn validate_bootstrap_record(
        &self,
        record: &ProviderStateRecord,
        key: &str,
        principal: &PrincipalId,
    ) -> Result<(), HostProblem> {
        if record.namespace != BOOTSTRAP_NAMESPACE || record.key != key || record.version != 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let recorded =
            std::str::from_utf8(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        if recorded != principal.as_str() {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }

    fn acquire_bootstrap_claim(&self, principal: &PrincipalId) -> Result<(), HostProblem> {
        for _ in 0..BOOTSTRAP_CAS_ATTEMPTS {
            if let Some(claim) = self.bootstrap_record(BOOTSTRAP_CLAIM_KEY)? {
                return self.validate_bootstrap_record(&claim, BOOTSTRAP_CLAIM_KEY, principal);
            }
            let active = self.racf.active_principal_epochs()?;
            let recoverable_partial = active.len() == 1
                && active.contains_key(principal.as_str())
                && self.racf.bootstrap_administrator_ready(principal)?;
            if !active.is_empty() && !recoverable_partial {
                return Err(HostProblem::Unauthorized);
            }
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: BOOTSTRAP_NAMESPACE.into(),
                    key: BOOTSTRAP_CLAIM_KEY.into(),
                    version: 1,
                    payload: principal.as_str().as_bytes().to_vec(),
                },
                None,
            ) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    fn commit_bootstrap_marker(&self, principal: &PrincipalId) -> Result<(), HostProblem> {
        for _ in 0..BOOTSTRAP_CAS_ATTEMPTS {
            if let Some(marker) = self.bootstrap_record(BOOTSTRAP_KEY)? {
                return self.validate_bootstrap_record(&marker, BOOTSTRAP_KEY, principal);
            }
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: BOOTSTRAP_NAMESPACE.into(),
                    key: BOOTSTRAP_KEY.into(),
                    version: 1,
                    payload: principal.as_str().as_bytes().to_vec(),
                },
                None,
            ) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    fn require_bootstrap_administrator(&self, principal: &PrincipalId) -> Result<(), HostProblem> {
        if self.racf.bootstrap_administrator_ready(principal)? {
            Ok(())
        } else {
            Err(HostProblem::Unauthorized)
        }
    }

    pub fn bootstrap_identity(&self, user: &str, secret: &[u8]) -> Result<(), HostProblem> {
        let principal = PrincipalId::new(user.to_ascii_uppercase(), InvocationLimits::default())
            .map_err(|_| HostProblem::Malformed)?;
        let reference = SecretRef::new(
            format!("bootstrap:{}:{}", principal.as_str(), self.next_sequence()?),
            HostLimits::default(),
        )?;
        let _scope = self.secrets.scoped(&reference, secret.to_vec())?;
        self.racf.add_user(principal.as_str(), &reference)
    }

    pub fn start_background_workers(self: &Arc<Self>) -> Result<(), HostProblem> {
        if self.jes_workers_stopping.load(Ordering::SeqCst) {
            return Err(HostProblem::InfrastructureFailure);
        }
        if self
            .jes_workers_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            self.jes_workers_started.store(false, Ordering::SeqCst);
            HostProblem::InfrastructureFailure
        })?;
        let pool = NEXT_JES_WORKER_POOL.fetch_add(1, Ordering::Relaxed);
        let mut handles = self
            .jes_worker_handles
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        handles.reserve(JES_WORKER_COUNT);
        for ordinal in 0..JES_WORKER_COUNT {
            let server = Arc::downgrade(self);
            let worker = format!("jes-worker-{pool}-{ordinal}");
            handles.push(runtime.spawn(Self::jes_worker_loop(server, worker, ordinal)));
        }
        Ok(())
    }

    async fn jes_worker_loop(server: Weak<Self>, worker: String, ordinal: usize) {
        loop {
            let Some(product) = server.upgrade() else {
                return;
            };
            if product.jes_workers_stopping.load(Ordering::SeqCst) {
                clear_worker_progress(&product.jes_worker_last_progress, ordinal);
                return;
            }
            let claim_product = product.clone();
            let claim_worker = worker.clone();
            let claimed =
                tokio::task::spawn_blocking(move || claim_product.claim_jes_work(&claim_worker))
                    .await;
            let claimed = match claimed {
                Ok(Ok(value)) => {
                    product.record_jes_worker_progress(ordinal);
                    value
                }
                Ok(Err(_)) | Err(_) => {
                    product.record_jes_worker_failure(ordinal);
                    tokio::time::sleep(Duration::from_millis(JES_IDLE_MILLIS)).await;
                    continue;
                }
            };
            let Some(work) = claimed else {
                tokio::select! {
                    () = product.jes_worker_notify.notified() => {},
                    () = tokio::time::sleep(Duration::from_millis(JES_IDLE_MILLIS)) => {},
                }
                continue;
            };

            product.jes_worker_active.fetch_add(1, Ordering::SeqCst);
            let execute_product = product.clone();
            let execute_work = work.clone();
            let mut execution = tokio::task::spawn_blocking(move || {
                execute_product.process_claimed_jes_work(&execute_work)
            });
            let mut lease_current = true;
            let joined = loop {
                tokio::select! {
                    result = &mut execution => break result,
                    () = tokio::time::sleep(Duration::from_millis(JES_HEARTBEAT_MILLIS)) => {
                        let heartbeat_product = product.clone();
                        let heartbeat_work = work.clone();
                        match tokio::task::spawn_blocking(move || {
                            heartbeat_product.heartbeat_jes_work(&heartbeat_work)
                        }).await {
                            Ok(Ok(())) => product.record_jes_worker_progress(ordinal),
                            Ok(Err(_)) | Err(_) => {
                                product.record_jes_worker_failure(ordinal);
                                lease_current = false;
                                break execution.await;
                            }
                        }
                    }
                }
            };
            product.jes_worker_active.fetch_sub(1, Ordering::SeqCst);
            if lease_current {
                if let Ok(outcome) = joined {
                    let finish_product = product.clone();
                    let finish_work = work.clone();
                    match tokio::task::spawn_blocking(move || {
                        finish_product.finish_claimed_jes_work(&finish_work, outcome)
                    })
                    .await
                    {
                        Ok(Ok(())) => product.record_jes_worker_progress(ordinal),
                        Ok(Err(_)) | Err(_) => product.record_jes_worker_failure(ordinal),
                    }
                } else {
                    product.record_jes_worker_failure(ordinal);
                }
            }
        }
    }

    fn record_jes_worker_progress(&self, ordinal: usize) {
        if let Ok(mut progress) = self.jes_worker_last_progress.lock()
            && let Some(slot) = progress.get_mut(ordinal)
        {
            *slot = Some(Instant::now());
        }
        self.jes_worker_progress.fetch_add(1, Ordering::Relaxed);
    }

    fn record_jes_worker_failure(&self, ordinal: usize) {
        clear_worker_progress(&self.jes_worker_last_progress, ordinal);
        self.jes_worker_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn fresh_jes_workers_at(&self, now: Instant) -> usize {
        self.jes_worker_last_progress.lock().map_or(0, |progress| {
            progress
                .iter()
                .filter(|last| {
                    last.is_some_and(|last| {
                        now.saturating_duration_since(last)
                            <= Duration::from_millis(JES_WORKER_FRESHNESS_MILLIS)
                    })
                })
                .count()
        })
    }

    fn claim_jes_work(&self, worker: &str) -> Result<Option<WorkRecord>, HostProblem> {
        claim_durable_work(self.store.as_ref(), worker, self.jes_tick()?).map_err(store_error)
    }

    fn heartbeat_jes_work(&self, work: &WorkRecord) -> Result<(), HostProblem> {
        heartbeat_durable_work(self.store.as_ref(), work, self.jes_tick()?).map_err(store_error)
    }

    fn process_claimed_jes_work(&self, work: &WorkRecord) -> Result<JesWorkOutcome, HostProblem> {
        if let Some(outcome) = self.process_interval_work(work, self.jes_tick()?)? {
            return Ok(outcome);
        }
        if work.state != WorkState::Claimed
            || work.required_generation != JES_WORK_GENERATION
            || work.required_selector.as_str() != "zosmf:job-submit"
            || work.artifact.as_str() != "artifact:none"
            || work.max_attempts != 3
            || work.lease_epoch == 0
            || work.lease_id.is_none()
            || work.work_id.strip_prefix("jes:").is_none()
        {
            return Err(HostProblem::Malformed);
        }
        let payload = JesWorkPayload::decode(&work.payload).map_err(store_error)?;
        if work.work_id != format!("jes:{}", payload.job_id)
            || payload
                .capabilities
                .iter()
                .any(|capability| !JES_ALLOWED_WORK_CAPABILITIES.contains(&capability.as_str()))
        {
            return Err(HostProblem::Malformed);
        }
        let child_admission_retry_allowed = work.attempt < work.max_attempts;
        loop {
            let job = self.batch.get(&payload.job_id)?;
            if job.owner != payload.owner {
                return Err(HostProblem::Unauthorized);
            }
            if job.priority != work.priority {
                return Err(HostProblem::Malformed);
            }
            match job.state {
                mainframe_env_batch::JobState::Completed
                | mainframe_env_batch::JobState::Failed => {
                    return match self.admit_internal_reader_children(
                        &payload.job_id,
                        child_admission_retry_allowed,
                    )? {
                        ChildAdmissionResult::Ok => Ok(JesWorkOutcome::Completed),
                        ChildAdmissionResult::Retry => Ok(JesWorkOutcome::Deferred),
                    };
                }
                mainframe_env_batch::JobState::Cancelled => {
                    return match self.admit_internal_reader_children(
                        &payload.job_id,
                        child_admission_retry_allowed,
                    )? {
                        ChildAdmissionResult::Ok => Ok(JesWorkOutcome::Cancelled),
                        ChildAdmissionResult::Retry => Ok(JesWorkOutcome::Deferred),
                    };
                }
                mainframe_env_batch::JobState::Held => return Ok(JesWorkOutcome::Deferred),
                mainframe_env_batch::JobState::Submitted
                | mainframe_env_batch::JobState::Output => {
                    return Err(HostProblem::InfrastructureFailure);
                }
                mainframe_env_batch::JobState::Queued
                | mainframe_env_batch::JobState::Selected
                | mainframe_env_batch::JobState::Running => {}
            }
            let current = self
                .store
                .get_work(&work.work_id)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            if current.lease_id != work.lease_id || current.lease_epoch != work.lease_epoch {
                return Err(HostProblem::InfrastructureFailure);
            }
            let invocation =
                self.jes_work_invocation(work, &payload, current.cancellation_requested)?;
            if current.cancellation_requested {
                match self.batch.cancel(&invocation, &payload.job_id) {
                    Ok(_) => return Ok(JesWorkOutcome::Cancelled),
                    Err(HostProblem::Condition { .. })
                        if self.batch.get(&payload.job_id)?.state
                            == mainframe_env_batch::JobState::Cancelled =>
                    {
                        return Ok(JesWorkOutcome::Cancelled);
                    }
                    Err(problem) => return Err(problem),
                }
            }
            match self
                .batch
                .run_claimed(&invocation, &payload.job_id, "INIT0001", false)?
            {
                Some(job) if job.state == mainframe_env_batch::JobState::Cancelled => {
                    return match self.admit_internal_reader_children(
                        &payload.job_id,
                        child_admission_retry_allowed,
                    )? {
                        ChildAdmissionResult::Ok => Ok(JesWorkOutcome::Cancelled),
                        ChildAdmissionResult::Retry => Ok(JesWorkOutcome::Deferred),
                    };
                }
                Some(_) => {
                    return match self.admit_internal_reader_children(
                        &payload.job_id,
                        child_admission_retry_allowed,
                    )? {
                        ChildAdmissionResult::Ok => Ok(JesWorkOutcome::Completed),
                        ChildAdmissionResult::Retry => Ok(JesWorkOutcome::Deferred),
                    };
                }
                None if self.jes_workers_stopping.load(Ordering::SeqCst) => {
                    return Ok(JesWorkOutcome::Deferred);
                }
                None => std::thread::sleep(Duration::from_millis(JES_IDLE_MILLIS)),
            }
        }
    }

    fn finish_claimed_jes_work(
        &self,
        work: &WorkRecord,
        outcome: Result<JesWorkOutcome, HostProblem>,
    ) -> Result<(), HostProblem> {
        let lease = work
            .lease_id
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let now_tick = self.jes_tick()?;
        self.recover_local_wakeups()?;
        match outcome {
            Ok(JesWorkOutcome::Completed) => self
                .store
                .complete(&work.work_id, lease, work.lease_epoch, now_tick)
                .map_err(store_error),
            Ok(JesWorkOutcome::Cancelled) => {
                self.store
                    .request_cancellation(&work.work_id)
                    .map_err(store_error)?;
                self.store
                    .release(&work.work_id, lease, work.lease_epoch, now_tick, now_tick)
                    .map(|_| ())
                    .map_err(store_error)
            }
            Ok(JesWorkOutcome::Deferred) => self
                .store
                .release(
                    &work.work_id,
                    lease,
                    work.lease_epoch,
                    now_tick,
                    now_tick
                        .checked_add(JES_IDLE_MILLIS)
                        .ok_or(HostProblem::ResourceExhausted)?,
                )
                .map(|_| ())
                .map_err(store_error),
            Err(_) => self
                .store
                .dead_letter(&work.work_id, lease, work.lease_epoch, now_tick)
                .map(|_| ())
                .map_err(store_error),
        }
    }

    fn jes_work_invocation(
        &self,
        work: &WorkRecord,
        payload: &JesWorkPayload,
        cancelled: bool,
    ) -> Result<Invocation, HostProblem> {
        let limits = InvocationLimits::default();
        let grants = payload
            .capabilities
            .iter()
            .map(|capability| {
                CapabilityId::new(capability, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let generations = grants
            .iter()
            .cloned()
            .map(|capability| (capability, "1".to_string()))
            .collect();
        let bindings = BTreeMap::from([(
            "jes.work-id".into(),
            BoundedPayload::new(
                "mainframe-env.jes-work@1",
                work.work_id.as_bytes().to_vec(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        )]);
        let identity = format!("{}-{}", payload.job_id, work.lease_epoch);
        let mut invocation = Invocation::new(
            RequestId::new(format!("jes-request-{identity}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            work.execution_id.clone(),
            RunUnitId::new(format!("jes-run-{identity}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            None,
            work.required_selector.clone(),
            work.artifact.clone(),
            Principal::new(
                PrincipalId::new(&payload.owner, limits).map_err(|_| HostProblem::Unauthorized)?,
                grants,
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ServiceClass::Batch,
            0,
            work.deadline_tick,
            TraceId::new(format!("jes-trace-{identity}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("jes-work-{}", payload.job_id), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            work.attempt,
            ResourceLimits::default(),
            bindings,
            limits,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .with_provider_generations(generations, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if cancelled {
            invocation = invocation.with_cancellation(
                Cancellation::new(
                    CancellationId::new(format!("jes-cancel-{identity}"), limits)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                    "durable JES work cancellation",
                    self.jes_tick()?,
                    limits,
                )
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
        }
        Ok(invocation)
    }

    pub(crate) fn jes_tick(&self) -> Result<u64, HostProblem> {
        self.jes_clock.now_tick().map_err(store_error)
    }

    #[cfg(test)]
    fn open_with_clock(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        clock: Arc<dyn JesClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        if config.artifact_profile != ArtifactProfile::Local {
            return Err(HostProblem::Malformed);
        }
        let artifacts = Arc::new(ProductArtifactStore::Local(
            LocalArtifactStore::open(&config.artifact_root, 64 * 1024 * 1024)
                .map_err(store_error)?,
        ));
        Self::open_configured(
            config,
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            Arc::new(RejectPackageTrust),
            artifacts,
            Some(clock),
        )
    }

    #[cfg(test)]
    fn run_jes_worker_once(&self, worker: &str) -> Result<Option<String>, HostProblem> {
        let Some(work) = self.claim_jes_work(worker)? else {
            return Ok(None);
        };
        let work_id = work.work_id.clone();
        let outcome = self.process_claimed_jes_work(&work);
        self.finish_claimed_jes_work(&work, outcome)?;
        Ok(Some(work_id))
    }

    pub fn router(self: &Arc<Self>) -> axum::Router {
        let _ = self.start_background_workers();
        mainframe_env_zosmf::router(
            self.clone(),
            ZosmfLimits {
                max_body_bytes: self.config.max_body_bytes,
                max_concurrency: self.config.max_concurrency,
                max_blocking: self.config.max_concurrency.min(4),
                timeout: Duration::from_millis(self.config.timeout_millis),
                max_page_items: 1000,
            },
        )
    }

    #[must_use]
    pub const fn live(&self) -> bool {
        true
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.readiness().ready()
    }

    #[must_use]
    pub fn readiness(&self) -> ProductReadiness {
        let capacity = self
            .config
            .retention
            .policy()
            .ok()
            .and_then(|policy| self.store.retention_capacity_health(policy).ok());
        let retention_capacity = capacity
            .as_ref()
            .map_or(ProductCapacityStatus::Unavailable, |health| {
                health.saturation.into()
            });
        ProductReadiness {
            accepting: self.accepting.load(Ordering::SeqCst),
            writable_store: capacity.is_some(),
            retention_capacity,
            retention_warning: retention_capacity.warning(),
            bootstrap_identity: self.bootstrap_principal_ready(),
            host_capabilities: [
                "host.cics.execute",
                "host.db2.read",
                "host.db2.write",
                "host.ims.read",
                "host.ims.write",
                "host.mq.read",
                "host.mq.write",
            ]
            .into_iter()
            .all(|capability| self.host.capability_ready(capability)),
            artifact_store: self.artifacts.is_ready(),
            jes_workers: self.jes_workers_ready(),
        }
    }

    fn bootstrap_principal_ready(&self) -> bool {
        let marker = match self
            .store
            .get_provider_state(BOOTSTRAP_NAMESPACE, BOOTSTRAP_KEY)
        {
            Ok(Some(marker)) if marker.version == 1 => marker,
            _ => return false,
        };
        let administrator = std::str::from_utf8(&marker.payload)
            .ok()
            .and_then(|administrator| {
                PrincipalId::new(administrator, InvocationLimits::default()).ok()
            });
        let Some(administrator) = administrator else {
            return false;
        };
        match self.bootstrap_record(BOOTSTRAP_CLAIM_KEY) {
            Ok(Some(claim))
                if self
                    .validate_bootstrap_record(&claim, BOOTSTRAP_CLAIM_KEY, &administrator)
                    .is_err() =>
            {
                return false;
            }
            Ok(_) => {}
            Err(_) => return false,
        }
        self.racf
            .bootstrap_administrator_ready(&administrator)
            .unwrap_or(false)
    }

    fn jes_workers_ready(&self) -> bool {
        self.jes_workers_ready_at(Instant::now())
    }

    fn jes_workers_ready_at(&self, now: Instant) -> bool {
        self.jes_workers_started.load(Ordering::SeqCst)
            && !self.jes_workers_stopping.load(Ordering::SeqCst)
            && self.fresh_jes_workers_at(now) == JES_WORKER_COUNT
            && self.jes_worker_handles.lock().is_ok_and(|handles| {
                handles.len() == JES_WORKER_COUNT
                    && handles.iter().all(|handle| !handle.is_finished())
            })
    }

    #[must_use]
    pub fn metrics(&self) -> ProductMetrics {
        ProductMetrics {
            requests: self.requests.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            active: self.active.load(Ordering::Relaxed),
            sessions: self.sessions.lock().map_or(0, |sessions| sessions.len()),
            console_messages: self.console.lock().map_or(0, |messages| messages.len()),
            jes_workers: usize::from(
                self.jes_workers_started.load(Ordering::Relaxed)
                    && !self.jes_workers_stopping.load(Ordering::Relaxed),
            ) * JES_WORKER_COUNT,
            jes_worker_healthy: self.fresh_jes_workers_at(Instant::now()),
            jes_worker_progress: self.jes_worker_progress.load(Ordering::Relaxed),
            jes_worker_failures: self.jes_worker_failures.load(Ordering::Relaxed),
            jes_active: self.jes_worker_active.load(Ordering::Relaxed),
            outbox_pending: self
                .store
                .pending_notifications(4096)
                .map_or(0, |rows| rows.len()),
            outbox_delivered: self.outbox_delivered.load(Ordering::Relaxed),
        }
    }

    fn retention_planner(&self) -> Result<RetentionPlanner, HostProblem> {
        RetentionPlanner::from_existing(
            self.store.clone(),
            self.config.retention.policy()?,
            Some(self.racf.database().clone()),
        )
    }

    /// Forecast eligibility and capacity for one retained record family.
    pub fn operator_retention_forecast(
        &self,
        target: RetentionTarget,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, HostProblem> {
        self.retention_planner()?
            .forecast(target, self.jes_tick()?, observed_growth_per_tick)
    }
    /// Atomically archive and prune at most `max_records` eligible source rows.
    pub fn operator_archive_and_prune(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem> {
        let receipt =
            self.retention_planner()?
                .archive_and_prune(target, self.jes_tick()?, max_records)?;
        if receipt.pruned != 0 {
            match target {
                RetentionTarget::DatasetReplay => {
                    self.dataset.refresh_replay_index()?;
                }
                RetentionTarget::SpoolJobs => self.spool.refresh_after_external_retention()?,
                RetentionTarget::ConsoleLog => self.refresh_console_cache()?,
                _ => {}
            }
        }
        Ok(receipt)
    }

    /// Read whole verified archives containing at most `max` source rows in total.
    pub fn operator_retention_archives(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionArchive>, HostProblem> {
        let policy = self.config.retention.policy()?;
        if max == 0 || max > policy.max_batch {
            return Err(HostProblem::Malformed);
        }
        self.store
            .retention_archives(target, max)
            .map_err(store_error)
    }

    /// Permanently delete whole archive batches containing at most `max` source rows.
    pub fn operator_prune_retention_archives(&self, max: usize) -> Result<usize, HostProblem> {
        self.store
            .prune_retention_archives(self.config.retention.policy()?, self.jes_tick()?, max)
            .map_err(store_error)
    }

    /// Permanently delete expired archives, requiring an exact identity for an oversized batch.
    pub fn operator_prune_retention_archives_authorized(
        &self,
        mut request: RetentionArchivePruneRequest,
    ) -> Result<RetentionArchivePruneOutcome, HostProblem> {
        request.now_tick = self.jes_tick()?;
        self.store
            .prune_retention_archives_authorized(self.config.retention.policy()?, request)
            .map_err(store_error)
    }

    /// Add conservative owner/age metadata to one operator-inspected legacy row.
    pub fn operator_reconcile_retention_age(
        &self,
        request: RetentionAgeReconciliation,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        self.retention_planner()?
            .reconcile(request, self.jes_tick()?)
    }

    /// List protected legacy rows and their exact reconciliation CAS tokens.
    pub fn operator_retention_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        self.retention_planner()?.legacy_rows(target, max)
    }

    pub async fn graceful_shutdown(&self) -> bool {
        self.accepting.store(false, Ordering::SeqCst);
        self.jes_workers_stopping.store(true, Ordering::SeqCst);
        self.jes_worker_notify.notify_waiters();
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.config.shutdown_millis);
        while self.active.load(Ordering::SeqCst) != 0 {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let handles = match self.jes_worker_handles.lock() {
            Ok(mut handles) => std::mem::take(&mut *handles),
            Err(_) => return false,
        };
        for mut handle in handles {
            let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now())
            else {
                handle.abort();
                return false;
            };
            if tokio::time::timeout(remaining, &mut handle).await.is_err() {
                handle.abort();
                return false;
            }
        }
        if self.jes_worker_active.load(Ordering::SeqCst) != 0 {
            return false;
        }
        if let Ok(mut progress) = self.jes_worker_last_progress.lock() {
            progress.fill(None);
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

    fn handle_with_context(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
        context: GatewayCallContext,
    ) -> Result<GatewayResponse, GatewayProblem> {
        let _scope = GatewayCallContextScope::enter(context);
        self.handle(authentication, request)
    }

    fn recover_local_wakeups(&self) -> Result<(), HostProblem> {
        let _delivery = self
            .outbox_delivery
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        for notification in self
            .store
            .pending_notifications(4096)
            .map_err(store_error)?
        {
            let delivered_tick = self.jes_tick()?;
            self.store
                .mark_notification_delivered(
                    &notification.notification_id,
                    notification.version,
                    delivered_tick,
                )
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
        if let Some(context) = current_gateway_call_context() {
            if context.cancellation_requested() {
                return Err(gateway_problem(HostProblem::Cancelled));
            }
            if context.deadline_elapsed() {
                return Err(gateway_problem(HostProblem::TimedOut));
            }
        }
        if matches!(request, GatewayRequest::Info) {
            let readiness = self.readiness();
            let listen = self
                .config
                .listen
                .parse::<SocketAddr>()
                .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
            return Ok(GatewayResponse::json(
                StatusCode::OK,
                json!({
                    "zos_version":"mainframe-env 0.1",
                    "zosmf_port":listen.port().to_string(),
                    "listen":self.config.listen.as_str(),
                    "zosmf_version":"mainframe-env.zosmf@1",
                    "api_version":"1",
                    "product_version":env!("CARGO_PKG_VERSION"),
                    "live":self.live(),
                    "ready":readiness.ready(),
                    "readiness":readiness,
                    "capabilities":["datasets","jobs","security","console"]
                }),
            ));
        }
        if matches!(request, GatewayRequest::Authenticate) {
            let (user, token) = match &authentication {
                Authentication::Basic { user, secret } => {
                    let verified = self.verify(user, secret).map_err(|_| unauthenticated())?;
                    let token = self.create_session(&verified).map_err(gateway_problem)?;
                    (verified.user, token)
                }
                Authentication::Bearer(token) => {
                    self.rotate_session(token).map_err(gateway_problem)?
                }
                Authentication::Anonymous => return Err(unauthenticated()),
            };
            return Ok(GatewayResponse::json(
                StatusCode::OK,
                json!({"user":user,"token":token}),
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
                let start = start
                    .map(|value| DatasetName::new(value, 128).map_err(|_| HostProblem::Malformed))
                    .transpose()
                    .map_err(gateway_problem)?;
                let (names, more) = self.visible_dataset_names(&principal, pattern, start, max)?;
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
                            DatasetOrganization::PartitionedExtended => "PO-E",
                            DatasetOrganization::KeySequenced
                            | DatasetOrganization::EntrySequenced
                            | DatasetOrganization::Relative
                            | DatasetOrganization::VariableRelative => "VS",
                            DatasetOrganization::Linear => "LDS",
                        });
                        item["recfm"] = json!(match attributes.record_format {
                            RecordFormat::Fixed => "F",
                            RecordFormat::FixedBlocked => "FB",
                            RecordFormat::FixedBlockedStandard => "FBS",
                            RecordFormat::Variable => "V",
                            RecordFormat::VariableBlocked => "VB",
                            RecordFormat::VariableSpanned => "VS",
                            RecordFormat::VariableBlockedSpanned => "VBS",
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
                        control: Default::default(),
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
                        mutation: self.mutation().map_err(gateway_problem)?,
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
                        mutation: self.mutation().map_err(gateway_problem)?,
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
                        purge: false,
                        current_date: None,
                        mutation: self.mutation().map_err(gateway_problem)?,
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
                        control: Default::default(),
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
                // This single-node composition owns one JES worker identity and the store exposes
                // FIFO rather than claim-by-id. Keep submit -> claim -> completion together so a
                // slower mandatory audit cannot let one request steal another request's work.
                let _submission = self
                    .job_submission
                    .lock()
                    .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
                let bundle = self.jcl_bundle(&principal, jcl)?;
                let plan = self.batch.plan(&bundle).map_err(gateway_problem)?;
                let capabilities =
                    job_capabilities(self.store.as_ref(), &plan).map_err(gateway_problem)?;
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:job-submit",
                        ServiceClass::Batch,
                        &capabilities,
                    )
                    .map_err(gateway_problem)?;
                let now_tick = self.jes_tick().map_err(gateway_problem)?;
                let snapshot = self
                    .batch
                    .submit(
                        &invocation,
                        &bundle,
                        &self.idempotency("submit").map_err(gateway_problem)?,
                        false,
                    )
                    .map_err(gateway_problem)?;
                if let Err(error) =
                    self.enqueue_jes_work(&invocation, &snapshot, &capabilities, now_tick)
                {
                    let _ = self.batch.cancel(&invocation, &snapshot.id);
                    return Err(gateway_problem(error));
                }
                Ok(GatewayResponse::json(
                    StatusCode::CREATED,
                    job_json(snapshot),
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
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:job-cancel",
                        ServiceClass::Batch,
                        &["host.security.authorize", "host.spool.write"],
                    )
                    .map_err(gateway_problem)?;
                self.batch
                    .cancel(&invocation, &jobid)
                    .map_err(gateway_problem)?;
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
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:job-purge",
                        ServiceClass::Batch,
                        &["host.security.authorize", "host.spool.write"],
                    )
                    .map_err(gateway_problem)?;
                self.batch
                    .purge(&invocation, &jobid)
                    .map_err(gateway_problem)?;
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
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:spool-list",
                        ServiceClass::Batch,
                        &["host.security.authorize", "host.spool.read"],
                    )
                    .map_err(gateway_problem)?;
                let files = self
                    .batch
                    .spool_files(&invocation, &jobid)
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
                let invocation = self
                    .invocation(
                        &principal,
                        "zosmf:spool-read",
                        ServiceClass::Batch,
                        &["host.security.authorize", "host.spool.read"],
                    )
                    .map_err(gateway_problem)?;
                let records = self
                    .batch
                    .spool_by_index(&invocation, &jobid, file, start, max)
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
                if let Some((_, artifact)) = online.as_ref() {
                    artifact::preflight_one(self.artifacts.as_ref(), artifact)
                        .map_err(gateway_problem)?;
                }
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
                self.preflight_online_continuation(&session, None)
                    .map_err(gateway_problem)?;
                let snapshot = self.online_exchange(&session).map_err(gateway_problem)?;
                let mut start_fresh_task = snapshot.is_none();
                if let Some(exchange) = snapshot {
                    self.preflight_online_exchange_state(&exchange)
                        .map_err(gateway_problem)?;
                    let tick = current_tick()?;
                    self.cics
                        .validate_terminal_resume(&session, &principal_id, &csrf_token, tick)
                        .map_err(gateway_problem)?;
                    match self
                        .recover_terminal_online_exchange(&session, &principal_id, &exchange, tick)
                        .map_err(gateway_problem)?
                    {
                        Some(TerminalExchangeRecovery::HandoffCompleted) => {
                            start_fresh_task = true;
                        }
                        Some(TerminalExchangeRecovery::Completed) => {}
                        Some(TerminalExchangeRecovery::Cancelled) => {
                            return Err(gateway_problem(HostProblem::Cancelled));
                        }
                        Some(TerminalExchangeRecovery::TimedOut) => {
                            return Err(gateway_problem(HostProblem::TimedOut));
                        }
                        Some(TerminalExchangeRecovery::Failed) => {
                            return Err(gateway_problem(HostProblem::ProviderFailure));
                        }
                        None => {
                            self.run_online_exchange(
                                &session,
                                &principal_id,
                                &exchange.program,
                                tick,
                            )
                            .map_err(gateway_problem)?;
                        }
                    }
                }
                if start_fresh_task {
                    self.resume_fresh_online_task(
                        &session,
                        &principal,
                        &principal_id,
                        &csrf_token,
                    )?;
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

    fn verify(&self, user: &str, secret: &[u8]) -> Result<VerifiedAuthentication, HostProblem> {
        let principal = PrincipalId::new(user.to_ascii_uppercase(), InvocationLimits::default())
            .map_err(|_| HostProblem::Unauthorized)?;
        let sequence = self.next_sequence()?;
        let reference = format!("request:{sequence}");
        let reference = SecretRef::new(reference, HostLimits::default())?;
        let _scope = self.secrets.scoped(&reference, secret.to_vec())?;
        let (decision, principal_epoch) =
            self.racf.authenticate_with_epoch(&principal, &reference)?;
        match (decision, principal_epoch) {
            (SecurityDecision::Allow, Some(principal_epoch)) => Ok(VerifiedAuthentication {
                user: principal.as_str().into(),
                principal_epoch,
            }),
            _ => Err(HostProblem::Unauthorized),
        }
    }

    fn principal(&self, authentication: Authentication) -> Result<String, HostProblem> {
        match &authentication {
            Authentication::Basic { user, secret } => {
                self.verify(user, secret).map(|verified| verified.user)
            }
            Authentication::Bearer(token) => self.use_session(token),
            Authentication::Anonymous => Err(HostProblem::Unauthorized),
        }
    }

    fn cleanup_auth_sessions(&self, now_tick: u64) -> Result<(), HostProblem> {
        let active_principals = self.racf.active_principal_epochs()?;
        let rows = self
            .store
            .list_provider_state(AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
            .map_err(store_error)?;
        let mut retained = BTreeMap::new();
        let mut stale = Vec::new();
        for row in rows {
            let session = decode_auth_session(&row)?;
            if session.expired(now_tick)
                || session.clock_regressed(now_tick)
                || active_principals
                    .get(&session.user)
                    .is_none_or(|epoch| epoch.as_str() != session.principal_epoch.as_str())
            {
                stale.push(row.key);
            } else {
                retained.insert(row.key, session);
            }
        }
        for key in stale {
            self.revoke_session_key(&key)?;
        }
        *self
            .sessions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)? = retained;
        Ok(())
    }

    fn revoke_session_key(&self, key: &str) -> Result<bool, HostProblem> {
        const MAX_ATTEMPTS: usize = 4;
        for _ in 0..MAX_ATTEMPTS {
            let Some(record) = self
                .store
                .get_provider_state(AUTH_SESSION_NAMESPACE, key)
                .map_err(store_error)?
            else {
                if let Ok(mut sessions) = self.sessions.lock() {
                    sessions.remove(key);
                }
                return Ok(false);
            };
            let mut index = load_auth_session_index(&*self.store)?;
            if index.sessions.remove(key).is_none() {
                reconcile_auth_session_index(&*self.store)?;
                continue;
            }
            let previous_index_version = index.version;
            index.version = index
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutations = vec![
                ProviderStateMutation::Delete {
                    namespace: AUTH_SESSION_NAMESPACE.into(),
                    key: key.into(),
                    expected_version: record.version,
                },
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: AUTH_SESSION_INDEX_NAMESPACE.into(),
                        key: AUTH_SESSION_INDEX_KEY.into(),
                        version: index.version,
                        payload: encode_auth_session_index(&index)?,
                    },
                    expected_version: Some(previous_index_version),
                }),
            ];
            match self.store.mutate_provider_states_atomic(mutations) {
                Ok(()) => {
                    if let Ok(mut sessions) = self.sessions.lock() {
                        sessions.remove(key);
                    }
                    return Ok(true);
                }
                Err(StoreError::Conflict | StoreError::NotFound) => continue,
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::InfrastructureFailure)
    }

    fn use_session(&self, token: &str) -> Result<String, HostProblem> {
        let key = auth_session_key(token);
        const MAX_ATTEMPTS: usize = 4;
        for _ in 0..MAX_ATTEMPTS {
            let Some(record) = self
                .store
                .get_provider_state(AUTH_SESSION_NAMESPACE, &key)
                .map_err(store_error)?
            else {
                return Err(HostProblem::Unauthorized);
            };
            let mut session = decode_auth_session(&record)?;
            let index = load_auth_session_index(&*self.store)?;
            if index.sessions.get(&key) != Some(&session.user) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let now_tick = self.jes_tick()?;
            let principal = PrincipalId::new(&session.user, InvocationLimits::default())
                .map_err(|_| HostProblem::Unauthorized)?;
            let valid_principal = self
                .racf
                .active_principal_epoch(&principal)?
                .is_some_and(|epoch| epoch.as_str() == session.principal_epoch.as_str());
            if session.expired(now_tick) || !valid_principal {
                self.revoke_session_key(&key)?;
                return Err(HostProblem::Unauthorized);
            }
            if session.clock_regressed(now_tick) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let previous_version = session.version;
            session.last_used_tick = now_tick;
            session.idle_expires_tick = now_tick
                .checked_add(AUTH_SESSION_IDLE_TTL_MILLIS)
                .ok_or(HostProblem::ResourceExhausted)?
                .min(session.absolute_expires_tick);
            session.version = session
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: AUTH_SESSION_NAMESPACE.into(),
                    key: key.clone(),
                    version: session.version,
                    payload: encode_auth_session(&session)?,
                },
                Some(previous_version),
            ) {
                Ok(()) => {
                    let user = session.user.clone();
                    self.sessions
                        .lock()
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        .insert(key, session);
                    return Ok(user);
                }
                Err(StoreError::Conflict) => continue,
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::InfrastructureFailure)
    }

    fn create_session(&self, verified: &VerifiedAuthentication) -> Result<String, HostProblem> {
        let now_tick = self.jes_tick()?;
        self.cleanup_auth_sessions(now_tick)?;
        const MAX_ATTEMPTS: usize = 4;
        for _ in 0..MAX_ATTEMPTS {
            let principal = PrincipalId::new(&verified.user, InvocationLimits::default())
                .map_err(|_| HostProblem::Unauthorized)?;
            if self.racf.active_principal_epoch(&principal)?.as_ref()
                != Some(&verified.principal_epoch)
            {
                return Err(HostProblem::Unauthorized);
            }
            let mut index = load_auth_session_index(&*self.store)?;
            if index.sessions.len() >= MAX_AUTH_SESSIONS
                || index
                    .sessions
                    .values()
                    .filter(|user| *user == &verified.user)
                    .count()
                    >= MAX_AUTH_SESSIONS_PER_USER
            {
                return Err(HostProblem::ResourceExhausted);
            }
            let token = Zeroizing::new(secure_random_token("session")?);
            let key = auth_session_key(&token);
            if index.sessions.contains_key(&key) {
                continue;
            }
            let session = AuthSession {
                schema_version: AUTH_SESSION_CONTRACT.into(),
                user: verified.user.clone(),
                issued_tick: now_tick,
                last_used_tick: now_tick,
                absolute_expires_tick: now_tick
                    .checked_add(AUTH_SESSION_ABSOLUTE_TTL_MILLIS)
                    .ok_or(HostProblem::ResourceExhausted)?,
                idle_expires_tick: now_tick
                    .checked_add(AUTH_SESSION_IDLE_TTL_MILLIS)
                    .ok_or(HostProblem::ResourceExhausted)?,
                principal_epoch: verified.principal_epoch.as_str().into(),
                version: 1,
            };
            let previous_index_version = index.version;
            index.sessions.insert(key.clone(), verified.user.clone());
            index.version = index
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutations = vec![
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: AUTH_SESSION_NAMESPACE.into(),
                        key: key.clone(),
                        version: 1,
                        payload: encode_auth_session(&session)?,
                    },
                    expected_version: None,
                }),
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: AUTH_SESSION_INDEX_NAMESPACE.into(),
                        key: AUTH_SESSION_INDEX_KEY.into(),
                        version: index.version,
                        payload: encode_auth_session_index(&index)?,
                    },
                    expected_version: Some(previous_index_version),
                }),
            ];
            match self.store.mutate_provider_states_atomic(mutations) {
                Ok(()) => {
                    self.sessions
                        .lock()
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        .insert(key, session);
                    return Ok(token.to_string());
                }
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::InfrastructureFailure)
    }

    fn rotate_session(&self, token: &str) -> Result<(String, String), HostProblem> {
        let old_key = auth_session_key(token);
        const MAX_ATTEMPTS: usize = 4;
        for _ in 0..MAX_ATTEMPTS {
            let old_record = self
                .store
                .get_provider_state(AUTH_SESSION_NAMESPACE, &old_key)
                .map_err(store_error)?
                .ok_or(HostProblem::Unauthorized)?;
            let old_session = decode_auth_session(&old_record)?;
            let mut index = load_auth_session_index(&*self.store)?;
            if index.sessions.get(&old_key) != Some(&old_session.user) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let now_tick = self.jes_tick()?;
            let principal = PrincipalId::new(&old_session.user, InvocationLimits::default())
                .map_err(|_| HostProblem::Unauthorized)?;
            if old_session.expired(now_tick)
                || self
                    .racf
                    .active_principal_epoch(&principal)?
                    .is_none_or(|epoch| epoch.as_str() != old_session.principal_epoch.as_str())
            {
                self.revoke_session_key(&old_key)?;
                return Err(HostProblem::Unauthorized);
            }
            if old_session.clock_regressed(now_tick) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let new_token = Zeroizing::new(secure_random_token("session")?);
            let new_key = auth_session_key(&new_token);
            if index.sessions.contains_key(&new_key) {
                continue;
            }
            let new_session = AuthSession {
                schema_version: AUTH_SESSION_CONTRACT.into(),
                user: old_session.user.clone(),
                issued_tick: old_session.issued_tick,
                last_used_tick: now_tick,
                absolute_expires_tick: old_session.absolute_expires_tick,
                idle_expires_tick: now_tick
                    .checked_add(AUTH_SESSION_IDLE_TTL_MILLIS)
                    .ok_or(HostProblem::ResourceExhausted)?
                    .min(old_session.absolute_expires_tick),
                principal_epoch: old_session.principal_epoch,
                version: 1,
            };
            index.sessions.remove(&old_key);
            index
                .sessions
                .insert(new_key.clone(), new_session.user.clone());
            let previous_index_version = index.version;
            index.version = index
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutations = vec![
                ProviderStateMutation::Delete {
                    namespace: AUTH_SESSION_NAMESPACE.into(),
                    key: old_key.clone(),
                    expected_version: old_record.version,
                },
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: AUTH_SESSION_NAMESPACE.into(),
                        key: new_key.clone(),
                        version: new_session.version,
                        payload: encode_auth_session(&new_session)?,
                    },
                    expected_version: None,
                }),
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: AUTH_SESSION_INDEX_NAMESPACE.into(),
                        key: AUTH_SESSION_INDEX_KEY.into(),
                        version: index.version,
                        payload: encode_auth_session_index(&index)?,
                    },
                    expected_version: Some(previous_index_version),
                }),
            ];
            match self.store.mutate_provider_states_atomic(mutations) {
                Ok(()) => {
                    let mut sessions = self
                        .sessions
                        .lock()
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    sessions.remove(&old_key);
                    sessions.insert(new_key, new_session.clone());
                    return Ok((new_session.user, new_token.to_string()));
                }
                Err(StoreError::Conflict | StoreError::NotFound | StoreError::AlreadyExists) => {
                    continue;
                }
                Err(problem) => return Err(store_error(problem)),
            }
        }
        Err(HostProblem::InfrastructureFailure)
    }

    fn logout_token(&self, token: &str) -> Result<(), HostProblem> {
        let key = auth_session_key(token);
        self.revoke_session_key(&key)?
            .then_some(())
            .ok_or(HostProblem::NotFound)
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
        if let DatasetRequest::DefineAlias { target, .. } = &request {
            self.authorize_resource(principal, "DATASET", target.as_str(), AccessIntent::Read)?;
        }
        if let DatasetRequest::BuildAlternateIndex { base, .. } = &request {
            self.authorize_resource(principal, "DATASET", base.as_str(), AccessIntent::Read)?;
        }
        if let DatasetRequest::TvsStatus { owner, .. }
        | DatasetRequest::AcquireLock { owner, .. }
        | DatasetRequest::ReleaseLock { owner, .. }
        | DatasetRequest::BeginTvs { owner, .. }
        | DatasetRequest::StageTvs { owner, .. }
        | DatasetRequest::CompleteTvs { owner, .. }
        | DatasetRequest::ReconcileTvs { owner, .. } = &request
            && owner.as_str() != principal
        {
            return Err(gateway_problem(HostProblem::Unauthorized));
        }
        let dataset = match &request {
            DatasetRequest::Capabilities
            | DatasetRequest::TvsStatus { .. }
            | DatasetRequest::BeginTvs { .. }
            | DatasetRequest::CompleteTvs { .. }
            | DatasetRequest::ReconcileTvs { .. } => None,
            DatasetRequest::List { .. } => None,
            DatasetRequest::ListCatalog { pattern, .. } => Some(pattern.as_str()),
            DatasetRequest::ListVolumes { .. } => Some("VOLUME.**"),
            DatasetRequest::ReadConcatenation { .. } => None,
            DatasetRequest::Rename { from, .. } => Some(from.as_str()),
            DatasetRequest::ResolveCatalog { name } => Some(name.as_str()),
            DatasetRequest::DefineCatalog { catalog, .. }
            | DatasetRequest::SetCatalogConnection { catalog, .. } => Some(catalog.as_str()),
            DatasetRequest::DefineAlias { alias, .. } => Some(alias.as_str()),
            DatasetRequest::Attributes { dataset }
            | DatasetRequest::Describe { dataset }
            | DatasetRequest::Diagnose { dataset }
            | DatasetRequest::ListLocks { dataset, .. }
            | DatasetRequest::ListMembers { dataset, .. }
            | DatasetRequest::Read { dataset, .. }
            | DatasetRequest::ReadGeneric { dataset, .. }
            | DatasetRequest::ReadRelative { dataset, .. }
            | DatasetRequest::ReadRba { dataset, .. }
            | DatasetRequest::ReadSequential { dataset, .. }
            | DatasetRequest::Snapshot { dataset, .. }
            | DatasetRequest::ReadMemberGeneration { dataset, .. }
            | DatasetRequest::Create { dataset, .. }
            | DatasetRequest::Define { dataset, .. }
            | DatasetRequest::Alter { dataset, .. }
            | DatasetRequest::SetLifecycle { dataset, .. }
            | DatasetRequest::RecordBackup { dataset, .. }
            | DatasetRequest::Restore { dataset, .. }
            | DatasetRequest::DefineMemberAlias { dataset, .. }
            | DatasetRequest::WriteMemberGeneration { dataset, .. }
            | DatasetRequest::DeleteMemberGeneration { dataset, .. }
            | DatasetRequest::AcquireLock { dataset, .. }
            | DatasetRequest::ReleaseLock { dataset, .. }
            | DatasetRequest::Write { dataset, .. }
            | DatasetRequest::Append { dataset, .. }
            | DatasetRequest::Truncate { dataset, .. }
            | DatasetRequest::RewriteRecord { dataset, .. }
            | DatasetRequest::DeleteRecord { dataset, .. }
            | DatasetRequest::WriteRelative { dataset, .. }
            | DatasetRequest::DeleteRelative { dataset, .. }
            | DatasetRequest::WriteRba { dataset, .. }
            | DatasetRequest::Delete { dataset, .. }
            | DatasetRequest::StartBrowse { dataset, .. }
            | DatasetRequest::ReadNext { dataset, .. }
            | DatasetRequest::EndBrowse { dataset, .. }
            | DatasetRequest::Close { dataset, .. } => Some(dataset.as_str()),
            DatasetRequest::DefinePath { path, .. } => Some(path.as_str()),
            DatasetRequest::BuildAlternateIndex { index, .. } => Some(index.as_str()),
            DatasetRequest::StageTvs { operation, .. } => Some(match operation {
                mainframe_env_host_api::TvsRecordOperation::Insert { dataset, .. }
                | mainframe_env_host_api::TvsRecordOperation::Rewrite { dataset, .. }
                | mainframe_env_host_api::TvsRecordOperation::Delete { dataset, .. } => {
                    dataset.as_str()
                }
            }),
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
                        | DatasetRequest::Describe { .. }
                        | DatasetRequest::Diagnose { .. }
                        | DatasetRequest::ListLocks { .. }
                        | DatasetRequest::TvsStatus { .. }
                        | DatasetRequest::ListMembers { .. }
                        | DatasetRequest::Read { .. }
                        | DatasetRequest::ReadGeneric { .. }
                        | DatasetRequest::ReadRelative { .. }
                        | DatasetRequest::ReadRba { .. }
                        | DatasetRequest::ReadSequential { .. }
                        | DatasetRequest::Snapshot { .. }
                        | DatasetRequest::ReadMemberGeneration { .. }
                        | DatasetRequest::ResolveCatalog { .. }
                        | DatasetRequest::ResolveGeneration { .. }
                        | DatasetRequest::List { .. }
                        | DatasetRequest::ListCatalog { .. }
                        | DatasetRequest::ListVolumes { .. }
                        | DatasetRequest::StartBrowse { .. }
                        | DatasetRequest::ReadNext { .. }
                        | DatasetRequest::EndBrowse { .. }
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
        let sequence = self.next_sequence().map_err(gateway_problem)?;
        let mutation = dataset_mutation(&request);
        let idempotency_key = mutation.map(|mutation| mutation.idempotency_key.clone());
        let result = self
            .host
            .invoke(
                &invocation,
                self.jes_tick().map_err(gateway_problem)?,
                invocation.cancellation_requested(),
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: mutation.map_or(sequence, |mutation| mutation.sequence),
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key,
                    request: HostRequest::Dataset(request),
                },
            )
            .persist_with(|audit| self.store.record_audit(audit).map_err(store_error));
        match result.outcome.map_err(gateway_problem)? {
            HostResult::Dataset(result) => Ok(result),
            _ => Err(gateway_problem(HostProblem::ProviderFailure)),
        }
    }

    fn jcl_bundle(&self, principal: &str, jcl: Vec<u8>) -> Result<JclBundle, GatewayProblem> {
        let primary =
            String::from_utf8(jcl).map_err(|_| gateway_problem(HostProblem::Malformed))?;
        let mut cataloged_procedures = BTreeMap::new();
        for library in jcl_library_names(&primary).map_err(gateway_problem)? {
            self.authorize_resource(principal, "DATASET", &library, AccessIntent::Read)?;
            let dataset = DatasetName::new(library, 128)
                .map_err(|_| gateway_problem(HostProblem::Malformed))?;
            let (ccsid, members) = match (
                self.dataset
                    .invoke(DatasetRequest::Attributes {
                        dataset: dataset.clone(),
                    })
                    .map_err(gateway_problem)?,
                self.dataset
                    .invoke(DatasetRequest::ListMembers {
                        dataset: dataset.clone(),
                        start: None,
                        max_items: 4_096,
                    })
                    .map_err(gateway_problem)?,
            ) {
                (
                    DatasetResult::Attributes { attributes, .. },
                    DatasetResult::Members { names, more: false },
                ) => (attributes.ccsid, names),
                _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
            };
            for member in members {
                let records = match self
                    .dataset
                    .invoke(DatasetRequest::Read {
                        dataset: dataset.clone(),
                        member: Some(member.clone()),
                        key: None,
                        max_records: 4_096,
                        control: Default::default(),
                    })
                    .map_err(gateway_problem)?
                {
                    DatasetResult::Records { records, .. } => records,
                    _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
                };
                let mut source = String::new();
                for record in records {
                    let mut line = match ccsid {
                        None | Some(1208) => String::from_utf8(record)
                            .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                        Some(37) => CodePage::Cp037
                            .decode(&record, record.len().saturating_mul(4).max(1))
                            .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                        Some(_) => return Err(gateway_problem(HostProblem::Unsupported)),
                    };
                    while line.ends_with(' ') {
                        line.pop();
                    }
                    source.push_str(&line);
                    source.push('\n');
                }
                if cataloged_procedures
                    .insert(member.as_str().to_ascii_uppercase(), source)
                    .is_some()
                {
                    return Err(gateway_problem(HostProblem::IdempotencyConflict));
                }
            }
        }
        Ok(JclBundle {
            primary,
            cataloged_procedures,
            ..Default::default()
        })
    }

    fn authorize_resource(
        &self,
        principal: &str,
        class: &str,
        resource: &str,
        intent: AccessIntent,
    ) -> Result<(), GatewayProblem> {
        match self.resource_decision(principal, class, resource, intent)? {
            SecurityDecision::Allow => Ok(()),
            _ => Err(gateway_problem(HostProblem::Unauthorized)),
        }
    }

    fn resource_decision(
        &self,
        principal: &str,
        class: &str,
        resource: &str,
        intent: AccessIntent,
    ) -> Result<SecurityDecision, GatewayProblem> {
        let invocation = self
            .invocation(
                principal,
                "security:authorize",
                ServiceClass::System,
                &["host.security.authorize"],
            )
            .map_err(gateway_problem)?;
        let result = self
            .host
            .invoke(
                &invocation,
                self.jes_tick().map_err(gateway_problem)?,
                invocation.cancellation_requested(),
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
            )
            .persist_with(|audit| self.store.record_audit(audit).map_err(store_error));
        match result.outcome.map_err(gateway_problem)? {
            HostResult::Security(decision) => Ok(decision),
            _ => Err(gateway_problem(HostProblem::ProviderFailure)),
        }
    }

    fn visible_dataset_names(
        &self,
        principal: &str,
        pattern: String,
        start: Option<DatasetName>,
        max: usize,
    ) -> Result<(Vec<DatasetName>, bool), GatewayProblem> {
        const SCAN_PAGE_ITEMS: u32 = 256;
        const MAX_SCANNED_NAMES: usize = 262_144;
        if max == 0 {
            return Err(gateway_problem(HostProblem::Malformed));
        }
        let mut cursor = start;
        let mut continuation = false;
        let mut scanned = 0usize;
        let mut visible = Vec::with_capacity(max.saturating_add(1));
        loop {
            let DatasetResult::Listed { names, more } = self.dataset_call(
                principal,
                DatasetRequest::List {
                    pattern: pattern.clone(),
                    start: cursor.clone(),
                    max_items: SCAN_PAGE_ITEMS,
                },
            )?
            else {
                return Err(gateway_problem(HostProblem::ProviderFailure));
            };
            let previous = cursor.as_ref().map(|name| name.as_str().to_owned());
            let mut progressed = false;
            for name in names {
                if continuation && previous.as_deref() == Some(name.as_str()) {
                    continue;
                }
                progressed = true;
                scanned = scanned
                    .checked_add(1)
                    .ok_or_else(|| gateway_problem(HostProblem::ResourceExhausted))?;
                if scanned > MAX_SCANNED_NAMES {
                    return Err(gateway_problem(HostProblem::ResourceExhausted));
                }
                cursor = Some(name.clone());
                if self.resource_decision(
                    principal,
                    "DATASET",
                    name.as_str(),
                    AccessIntent::Read,
                )? == SecurityDecision::Allow
                {
                    visible.push(name);
                    if visible.len() > max {
                        visible.pop();
                        return Ok((visible, true));
                    }
                }
            }
            if !more {
                return Ok((visible, false));
            }
            if !progressed {
                return Err(gateway_problem(HostProblem::InfrastructureFailure));
            }
            continuation = true;
        }
    }

    fn next_sequence(&self) -> Result<u64, HostProblem> {
        let mut state = self
            .sequence
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let current = state.next;
        let successor = current
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let version = state
            .version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "server-meta".into(),
                    key: "next-sequence".into(),
                    version,
                    payload: successor.to_be_bytes().to_vec(),
                },
                state.version,
            )
            .map_err(store_error)?;
        state.next = successor;
        state.version = Some(version);
        Ok(current)
    }

    pub(crate) fn invocation(
        &self,
        principal: &str,
        selector: &str,
        service_class: ServiceClass,
        required_capabilities: &[&str],
    ) -> Result<Invocation, HostProblem> {
        let sequence = self.next_sequence()?;
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
        let context = current_gateway_call_context();
        let deadline_tick = self
            .jes_tick()?
            .checked_add(self.config.timeout_millis)
            .ok_or(HostProblem::ResourceExhausted)?;
        let invocation = Invocation::new(
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
            deadline_tick,
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
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        Ok(if let Some(context) = context {
            invocation.with_cancellation_probe(context.cancellation_probe())
        } else {
            invocation
        })
    }

    fn mutation(&self) -> Result<Mutation, HostProblem> {
        let sequence = self.next_sequence()?;
        Ok(Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("mutation-{sequence}"),
                InvocationLimits::default(),
            )
            .expect("bounded generated key"),
            transaction: Some(format!("zosmf-{sequence}")),
        })
    }

    fn idempotency(&self, kind: &str) -> Result<IdempotencyKey, HostProblem> {
        let sequence = self.next_sequence()?;
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
                &continuation::ONLINE_PROVIDER_CAPABILITIES,
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
                let (names, more) = self.visible_dataset_names(
                    principal,
                    format!("{}.**", principal.to_ascii_uppercase()),
                    None,
                    1000,
                )?;
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
                        mutation: self.mutation().map_err(gateway_problem)?,
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
                        purge: false,
                        current_date: None,
                        mutation: self.mutation().map_err(gateway_problem)?,
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
                        control: Default::default(),
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
                        mutation: self.mutation().map_err(gateway_problem)?,
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

    fn refresh_console_cache(&self) -> Result<(), HostProblem> {
        let rows = self
            .store
            .list_provider_state("console-log", 65_537)
            .map_err(store_error)?;
        if rows.len() > 65_536 {
            return Err(HostProblem::ResourceExhausted);
        }
        let decoded = decode_console_log_rows(&rows)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .into_iter()
            .map(|entry| ConsoleMessage {
                key: entry.key,
                console: entry.console,
                text: entry.text,
            })
            .collect();
        *self
            .console
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)? = decoded;
        Ok(())
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
        self.refresh_console_cache().map_err(gateway_problem)?;
        let mut messages = self
            .console
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        if messages.len() >= 65536 {
            return Err(gateway_problem(HostProblem::ResourceExhausted));
        }
        let key = format!("{:016}", self.next_sequence().map_err(gateway_problem)?);
        let observed_tick = self.jes_tick().map_err(gateway_problem)?;
        let payload = encode_console_log(
            &key,
            &name.to_ascii_uppercase(),
            &text,
            principal,
            "direct-console",
            observed_tick,
        )
        .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
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
        self.refresh_console_cache().map_err(gateway_problem)?;
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
        self.refresh_console_cache().map_err(gateway_problem)?;
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
        context: GatewayCallContext,
    ) -> Result<GatewayResponse, GatewayProblem> {
        self.handle_with_context(authentication, request, context)
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
    additional: Vec<Arc<dyn HostProvider>>,
    include_cics: bool,
    cics: Option<Arc<dyn HostProvider>>,
) -> Result<Arc<ScopedHostService>, HostProblem> {
    let limits = InvocationLimits::default();
    let mut providers = dataset_providers(dataset.clone(), limits);
    providers.extend(racf_providers(racf.clone(), limits));
    providers.push(Arc::new(SystemClockProvider::new(limits)) as Arc<dyn HostProvider>);
    providers.push(Arc::new(BatchTerminalProvider::new(limits)) as Arc<dyn HostProvider>);
    providers.push(program);
    providers.extend(additional);
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

struct BatchTerminalProvider {
    descriptor: CapabilityDescriptor,
}

impl BatchTerminalProvider {
    fn new(limits: InvocationLimits) -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.terminal", limits)
                    .expect("static terminal capability"),
                provider_id: "mainframe-env-batch-terminal".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.terminal-request@1".into(),
                result_schema: "mainframe-env.terminal-result@1".into(),
                max_request_bytes: 64 * 1024,
                max_result_bytes: 4 * 1024 * 1024,
                ready: true,
            },
        }
    }
}

impl HostProvider for BatchTerminalProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Terminal(TerminalRequest::Read { .. }) => invocation
                .bindings
                .get("cobol.terminal.input")
                .cloned()
                .map(HostResult::Terminal)
                .ok_or(HostProblem::NotFound),
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
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
                max_request_bytes: 256,
                max_result_bytes: 256,
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

fn current_gateway_call_context() -> Option<GatewayCallContext> {
    GATEWAY_CALL_CONTEXT.with(|current| current.borrow().clone())
}

fn auth_session_key(token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.auth-session-token@2\0");
    digest.update((token.len() as u64).to_be_bytes());
    digest.update(token.as_bytes());
    hex_digest(&digest.finalize())
}

fn encode_auth_session(session: &AuthSession) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(session).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_auth_session(record: &ProviderStateRecord) -> Result<AuthSession, HostProblem> {
    if record.namespace != AUTH_SESSION_NAMESPACE
        || record.key.len() != 64
        || !record
            .key
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let session: AuthSession =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    PrincipalId::new(&session.user, InvocationLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if session.schema_version != AUTH_SESSION_CONTRACT
        || session.version != record.version
        || session.principal_epoch.len() != 71
        || !session.principal_epoch.starts_with("sha256:")
        || !session.principal_epoch[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || session.issued_tick > session.last_used_tick
        || session.last_used_tick >= session.idle_expires_tick
        || session.idle_expires_tick > session.absolute_expires_tick
        || session
            .absolute_expires_tick
            .checked_sub(session.issued_tick)
            .is_none_or(|lifetime| lifetime == 0 || lifetime > AUTH_SESSION_ABSOLUTE_TTL_MILLIS)
        || session
            .idle_expires_tick
            .checked_sub(session.last_used_tick)
            .is_none_or(|lifetime| lifetime == 0 || lifetime > AUTH_SESSION_IDLE_TTL_MILLIS)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(session)
}

fn encode_auth_session_index(index: &AuthSessionIndex) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(index).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_auth_session_index(
    record: &ProviderStateRecord,
) -> Result<AuthSessionIndex, HostProblem> {
    if record.namespace != AUTH_SESSION_INDEX_NAMESPACE || record.key != AUTH_SESSION_INDEX_KEY {
        return Err(HostProblem::InfrastructureFailure);
    }
    let index: AuthSessionIndex =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if index.schema_version != AUTH_SESSION_INDEX_CONTRACT
        || index.version != record.version
        || index.version == 0
        || index.sessions.len() > MAX_AUTH_SESSIONS
        || index.sessions.iter().any(|(key, user)| {
            key.len() != 64
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                || PrincipalId::new(user, InvocationLimits::default()).is_err()
        })
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(index)
}

fn stored_auth_session_entries(
    store: &dyn PlatformStore,
) -> Result<BTreeMap<String, String>, HostProblem> {
    let rows = store
        .list_provider_state(AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
        .map_err(store_error)?;
    rows.into_iter()
        .map(|row| {
            let session = decode_auth_session(&row)?;
            Ok((row.key, session.user))
        })
        .collect()
}

fn reconcile_auth_session_index(store: &dyn PlatformStore) -> Result<(), HostProblem> {
    reconcile_auth_session_index_with_scan_hook(store, || {})
}

fn reconcile_auth_session_index_with_scan_hook(
    store: &dyn PlatformStore,
    mut after_scan: impl FnMut(),
) -> Result<(), HostProblem> {
    const MAX_ATTEMPTS: usize = 4;
    for _ in 0..MAX_ATTEMPTS {
        let before = store
            .get_provider_state(AUTH_SESSION_INDEX_NAMESPACE, AUTH_SESSION_INDEX_KEY)
            .map_err(store_error)?;
        let mut sessions = stored_auth_session_entries(store)?;
        after_scan();
        let current = store
            .get_provider_state(AUTH_SESSION_INDEX_NAMESPACE, AUTH_SESSION_INDEX_KEY)
            .map_err(store_error)?;
        if before != current {
            continue;
        }
        let (version, expected_version) = match &current {
            Some(record) => {
                let index = decode_auth_session_index(record)?;
                for (key, indexed_user) in &index.sessions {
                    if sessions.contains_key(key) {
                        continue;
                    }
                    if let Some(session_record) = store
                        .get_provider_state(AUTH_SESSION_NAMESPACE, key)
                        .map_err(store_error)?
                    {
                        let session = decode_auth_session(&session_record)?;
                        if &session.user != indexed_user {
                            return Err(HostProblem::InfrastructureFailure);
                        }
                        sessions.insert(key.clone(), session.user);
                    }
                }
                if index.sessions == sessions {
                    return Ok(());
                }
                (
                    index
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?,
                    Some(index.version),
                )
            }
            None => (1, None),
        };
        let index = AuthSessionIndex {
            schema_version: AUTH_SESSION_INDEX_CONTRACT.into(),
            sessions,
            version,
        };
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: AUTH_SESSION_INDEX_NAMESPACE.into(),
                key: AUTH_SESSION_INDEX_KEY.into(),
                version,
                payload: encode_auth_session_index(&index)?,
            },
            expected_version,
        ) {
            Ok(()) => return Ok(()),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
            Err(problem) => return Err(store_error(problem)),
        }
    }
    Err(HostProblem::InfrastructureFailure)
}

fn load_auth_session_index(store: &dyn PlatformStore) -> Result<AuthSessionIndex, HostProblem> {
    store
        .get_provider_state(AUTH_SESSION_INDEX_NAMESPACE, AUTH_SESSION_INDEX_KEY)
        .map_err(store_error)?
        .as_ref()
        .map(decode_auth_session_index)
        .transpose()?
        .ok_or(HostProblem::InfrastructureFailure)
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

fn decode_application_batch_controller(
    package: &ApplicationPackageV2,
    controller: &ApplicationBatchController,
) -> Result<BatchControllerDefinition, HostProblem> {
    let launcher = controller_property(controller, "launcher")?;
    let program = controller_property(controller, "selector-program")?.to_string();
    let selector = match launcher {
        "tso-run" => BatchControllerSelector::TsoRun { program },
        "ims-controller" => BatchControllerSelector::ImsController {
            mode: controller_property(controller, "selector-mode")?.to_string(),
            program,
            qualifier: controller.properties.get("selector-qualifier").cloned(),
        },
        _ => return Err(HostProblem::Malformed),
    };
    let behavior = controller_property(controller, "behavior")?;
    let artifact = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Program && entry.path == controller.program)
        .ok_or(HostProblem::Malformed)?;
    let artifact_program = artifact
        .path
        .rsplit('/')
        .next()
        .filter(|name| name.eq_ignore_ascii_case(selector.program()))
        .ok_or(HostProblem::Malformed)?;
    if artifact_program.is_empty() {
        return Err(HostProblem::Malformed);
    }
    let plan = match behavior {
        "program-call" if controller.kind == BatchControllerKind::CobolProgram => {
            validate_controller_properties(
                controller,
                &["behavior", "launcher", "selector-program"],
                &[],
            )?;
            BatchControllerPlan::ProgramCall
        }
        "ims-load" if controller.kind == BatchControllerKind::ImsMessageProcessing => {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "database",
                    "root-dd",
                    "child-dd",
                    "root-record-bytes",
                    "child-record-bytes",
                    "parent-key-bytes",
                ],
                &[],
            )?;
            BatchControllerPlan::ImsLoad {
                database: controller_property(controller, "database")?.into(),
                root_dd: controller_property(controller, "root-dd")?.into(),
                child_dd: controller_property(controller, "child-dd")?.into(),
                root_record_bytes: controller_usize(controller, "root-record-bytes")?,
                child_record_bytes: controller_usize(controller, "child-record-bytes")?,
                parent_key_bytes: controller_usize(controller, "parent-key-bytes")?,
            }
        }
        "ims-unload"
            if matches!(
                controller.kind,
                BatchControllerKind::ImsMessageProcessing | BatchControllerKind::DeclarativeUtility
            ) =>
        {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "database",
                    "root-segment",
                    "child-segment",
                ],
                &["root-output-dd", "child-output-dd", "combined-output-dd"],
            )?;
            BatchControllerPlan::ImsUnload {
                database: controller_property(controller, "database")?.into(),
                root_segment: controller_property(controller, "root-segment")?.into(),
                child_segment: controller_property(controller, "child-segment")?.into(),
                root_output_dd: controller.properties.get("root-output-dd").cloned(),
                child_output_dd: controller.properties.get("child-output-dd").cloned(),
                combined_output_dd: controller.properties.get("combined-output-dd").cloned(),
            }
        }
        "ims-purge" if controller.kind == BatchControllerKind::ImsMessageProcessing => {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "psb",
                    "root-segment",
                    "child-segment",
                    "control-dd",
                    "required-expiry-days",
                    "checkpoint-prefix",
                    "summary-field",
                ],
                &[],
            )?;
            BatchControllerPlan::ImsPurge {
                psb: controller_property(controller, "psb")?.into(),
                root_segment: controller_property(controller, "root-segment")?.into(),
                child_segment: controller_property(controller, "child-segment")?.into(),
                control_dd: controller_property(controller, "control-dd")?.into(),
                required_expiry_days: controller_property(controller, "required-expiry-days")?
                    .into(),
                checkpoint_prefix: controller_property(controller, "checkpoint-prefix")?.into(),
                summary_field: controller_property(controller, "summary-field")?.into(),
            }
        }
        _ => return Err(HostProblem::Malformed),
    };
    Ok(BatchControllerDefinition {
        name: controller.name.clone(),
        selector,
        program: BatchControllerProgram {
            path: artifact.path.clone(),
            identity: artifact.sha256.clone(),
        },
        plan,
    })
}

fn controller_property<'a>(
    controller: &'a ApplicationBatchController,
    name: &str,
) -> Result<&'a str, HostProblem> {
    controller
        .properties
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(HostProblem::Malformed)
}

fn controller_usize(
    controller: &ApplicationBatchController,
    name: &str,
) -> Result<usize, HostProblem> {
    controller_property(controller, name)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn validate_controller_properties(
    controller: &ApplicationBatchController,
    required: &[&str],
    optional: &[&str],
) -> Result<(), HostProblem> {
    let accepted = required
        .iter()
        .chain(optional)
        .copied()
        .collect::<BTreeSet<_>>();
    if required
        .iter()
        .any(|name| !controller.properties.contains_key(*name))
        || controller
            .properties
            .keys()
            .any(|name| !accepted.contains(name.as_str()))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
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

fn encode_online_exchange(state: &OnlineExchangeState) -> Result<Vec<u8>, HostProblem> {
    validate_online_exchange(state)?;
    serde_json::to_vec(state).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_online_exchange(
    record: &ProviderStateRecord,
) -> Result<OnlineExchangeState, HostProblem> {
    let mut state: OnlineExchangeState =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    state.version = record.version;
    validate_online_exchange(&state)?;
    Ok(state)
}

fn validate_online_exchange(state: &OnlineExchangeState) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    if state.schema_version != ONLINE_EXCHANGE_CONTRACT
        || state.version == 0
        || normalize_online_name(&state.program, 128)? != state.program
        || normalize_online_name(&state.transaction, 16)? != state.transaction
        || state.deadline_tick == 0
        || state.attempt == 0
        || state.grants.is_empty()
        || state.grants.len() > limits.max_capabilities
        || state.provider_generations.len() > limits.max_capabilities
        || state.commarea.len() > limits.max_payload_bytes
        || state.audit_correlation.is_empty()
        || state.audit_correlation.len() > limits.max_identity_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    RequestId::new(&state.request_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    ExecutionId::new(&state.execution_id, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    RunUnitId::new(&state.run_unit_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Selector::new(&state.selector, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    ArtifactRef::new(&state.artifact, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    PrincipalId::new(&state.principal, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    TraceId::new(&state.trace_id, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    IdempotencyKey::new(&state.idempotency_key, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    for grant in &state.grants {
        CapabilityId::new(grant, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    for (capability, generation) in &state.provider_generations {
        if !state.grants.contains(capability)
            || generation.is_empty()
            || generation.len() > limits.max_identity_bytes
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        CapabilityId::new(capability, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    if let Some(key) = &state.blocking_effect {
        IdempotencyKey::new(key, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    Ok(())
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
        | DatasetRequest::Define { mutation, .. }
        | DatasetRequest::Alter { mutation, .. }
        | DatasetRequest::SetLifecycle { mutation, .. }
        | DatasetRequest::RecordBackup { mutation, .. }
        | DatasetRequest::Restore { mutation, .. }
        | DatasetRequest::DefineCatalog { mutation, .. }
        | DatasetRequest::SetCatalogConnection { mutation, .. }
        | DatasetRequest::DefineAlias { mutation, .. }
        | DatasetRequest::DefineMemberAlias { mutation, .. }
        | DatasetRequest::WriteMemberGeneration { mutation, .. }
        | DatasetRequest::DeleteMemberGeneration { mutation, .. }
        | DatasetRequest::AcquireLock { mutation, .. }
        | DatasetRequest::ReleaseLock { mutation, .. }
        | DatasetRequest::BeginTvs { mutation, .. }
        | DatasetRequest::StageTvs { mutation, .. }
        | DatasetRequest::CompleteTvs { mutation, .. }
        | DatasetRequest::ReconcileTvs { mutation, .. }
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::Append { mutation, .. }
        | DatasetRequest::Truncate { mutation, .. }
        | DatasetRequest::RewriteRecord { mutation, .. }
        | DatasetRequest::DeleteRecord { mutation, .. }
        | DatasetRequest::WriteRelative { mutation, .. }
        | DatasetRequest::DeleteRelative { mutation, .. }
        | DatasetRequest::WriteRba { mutation, .. }
        | DatasetRequest::DefineAlternateIndex { mutation, .. }
        | DatasetRequest::BuildAlternateIndex { mutation, .. }
        | DatasetRequest::DefinePath { mutation, .. }
        | DatasetRequest::DefineGenerationGroup { mutation, .. }
        | DatasetRequest::CreateGeneration { mutation, .. }
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
        "PO" => DatasetOrganization::Partitioned,
        "PO-E" => DatasetOrganization::PartitionedExtended,
        "VS" | "KSDS" => DatasetOrganization::KeySequenced,
        "ESDS" => DatasetOrganization::EntrySequenced,
        "RRDS" => DatasetOrganization::Relative,
        "VRRDS" => DatasetOrganization::VariableRelative,
        "LDS" => DatasetOrganization::Linear,
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
        "FBS" => RecordFormat::FixedBlockedStandard,
        "V" => RecordFormat::Variable,
        "VB" => RecordFormat::VariableBlocked,
        "VS" => RecordFormat::VariableSpanned,
        "VBS" => RecordFormat::VariableBlockedSpanned,
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

pub(crate) fn job_capabilities(
    store: &dyn ProviderStateStore,
    plan: &mainframe_env_batch::JobPlan,
) -> Result<Vec<&'static str>, HostProblem> {
    let mut capabilities = BTreeSet::from([
        "host.security.authorize",
        "host.program.invoke",
        "host.spool.read",
        "host.spool.write",
    ]);
    if plan
        .steps
        .iter()
        .flat_map(|step| &step.dds)
        .any(|dd| dd.dataset.is_some())
    {
        capabilities.extend(["host.dataset.read", "host.dataset.write"]);
    }
    for step in &plan.steps {
        match step.program.as_str() {
            "IDCAMS" => capabilities.extend(["host.dataset.read", "host.dataset.write"]),
            "SDSF" => {
                capabilities.extend([
                    "host.cics.execute",
                    "host.dataset.read",
                    "host.dataset.write",
                ]);
            }
            "IKJEFT01" => capabilities.extend(["host.db2.read", "host.db2.write"]),
            "DFSRRC00" => capabilities.extend(["host.ims.read", "host.ims.write"]),
            "COBOL" => extend_cobol_capabilities(&mut capabilities),
            program => {
                if store
                    .get_provider_state("batch-program", program)
                    .map_err(store_error)?
                    .is_some()
                {
                    extend_cobol_capabilities(&mut capabilities);
                }
            }
        }
    }
    Ok(capabilities.into_iter().collect())
}

fn extend_cobol_capabilities(capabilities: &mut BTreeSet<&'static str>) {
    capabilities.extend([
        "host.cics.execute",
        "host.clock",
        "host.dataset.read",
        "host.dataset.write",
        "host.db2.read",
        "host.db2.write",
        "host.ims.read",
        "host.ims.write",
        "host.mq.read",
        "host.mq.write",
        "host.terminal",
    ]);
}

fn jcl_library_names(source: &str) -> Result<Vec<String>, HostProblem> {
    let control = source
        .lines()
        .filter(|line| line.starts_with("//") && !line.starts_with("//*"))
        .map(|line| line.get(..line.len().min(72)).unwrap_or(line))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let mut libraries = Vec::new();
    let mut rest = control.as_str();
    while let Some(start) = rest.find("JCLLIB ORDER=(") {
        rest = &rest[start + "JCLLIB ORDER=(".len()..];
        let end = rest.find(')').ok_or(HostProblem::Malformed)?;
        for name in rest[..end].split(',') {
            let name = name.trim().trim_matches(['\'', '"']);
            DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
            if libraries.iter().any(|existing| existing == name) {
                return Err(HostProblem::Malformed);
            }
            libraries.push(name.to_string());
        }
        rest = &rest[end + 1..];
    }
    Ok(libraries)
}

fn control_name(control: &str, keyword: &str) -> Option<String> {
    let upper = control.to_ascii_uppercase();
    let start = upper.find(&format!("{keyword}("))? + keyword.len() + 1;
    let end = upper[start..].find(')')? + start;
    let value = upper[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn job_json(job: mainframe_env_batch::JobSnapshot) -> Value {
    let job_type = if job.kind == mainframe_env_batch::JesJobKind::StartedTask {
        "STC"
    } else {
        "JOB"
    };
    let retcode = job
        .abend_code
        .map(|code| format!("ABEND {code}"))
        .or_else(|| job.return_code.map(|code| format!("CC {code:04}")));
    json!({
        "jobid":job.id,
        "jobname":job.name,
        "owner":job.owner,
        "status":if matches!(job.state, mainframe_env_batch::JobState::Completed | mainframe_env_batch::JobState::Failed | mainframe_env_batch::JobState::Cancelled) {"OUTPUT"} else {"ACTIVE"},
        "type":job_type,
        "class":job.class.to_string(),
        "retcode":retcode,
        "origin-node":job.route.origin_node,
        "execution-node":job.route.execution_node,
        "output-node":job.route.output_node,
        "mas-member":job.route.owner_member
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
        HostProblem::UnsupportedCapability { .. } => {
            (StatusCode::NOT_IMPLEMENTED, "unsupported_capability")
        }
        HostProblem::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        HostProblem::Unauthorized => (StatusCode::FORBIDDEN, "not_authorized"),
        HostProblem::Cancelled => (StatusCode::CONFLICT, "cancelled"),
        HostProblem::TimedOut => (StatusCode::REQUEST_TIMEOUT, "timed_out"),
        HostProblem::ResourceExhausted => (StatusCode::TOO_MANY_REQUESTS, "resource_exhausted"),
        HostProblem::IdempotencyConflict => (StatusCode::CONFLICT, "conflict"),
        HostProblem::UnknownOutcome => (StatusCode::CONFLICT, "unknown_outcome"),
        HostProblem::Condition { .. } => (StatusCode::CONFLICT, "condition"),
        HostProblem::ProviderFailure | HostProblem::InfrastructureFailure => {
            (StatusCode::SERVICE_UNAVAILABLE, "infrastructure_failure")
        }
        HostProblem::MissingIdempotency => (StatusCode::BAD_REQUEST, "missing_idempotency"),
    };
    GatewayProblem::new(status, code, &problem.to_string())
}

pub(crate) fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

fn application_install_problem(problem: InstallProblem) -> HostProblem {
    match problem {
        InstallProblem::IdentityConflict | InstallProblem::StaleGeneration => {
            HostProblem::IdempotencyConflict
        }
        InstallProblem::UnknownStage => HostProblem::NotFound,
        InstallProblem::Poisoned => HostProblem::InfrastructureFailure,
        InstallProblem::LimitExceeded => HostProblem::ResourceExhausted,
        InstallProblem::InvalidIdentity
        | InstallProblem::InvalidPath
        | InstallProblem::DuplicateEntry
        | InstallProblem::MissingKind
        | InstallProblem::MissingBlob
        | InstallProblem::ContentMismatch
        | InstallProblem::OrphanDependency
        | InstallProblem::IncompatibleProduct
        | InstallProblem::InvalidSignature
        | InstallProblem::MissingReference => HostProblem::Malformed,
    }
}

fn rollback_publication_state(
    package: &ApplicationPackageV2,
    expected: &ApplicationGenerationRecord,
    db2_applicable: bool,
) -> ApplicationPublicationState {
    ApplicationPublicationState {
        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
        package: package.base.manifest.name.clone(),
        generation: package.generation,
        identity: expected.identity.clone(),
        action: PublicationAction::Rollback,
        controllers: PublicationSectionState::Pending,
        db2: if db2_applicable {
            PublicationSectionState::Pending
        } else {
            PublicationSectionState::NotApplicable
        },
        complete: false,
    }
}

fn install_publication_state(
    package: &ApplicationPackageV2,
    expected: &ApplicationGenerationRecord,
    db2_applicable: bool,
) -> ApplicationPublicationState {
    ApplicationPublicationState {
        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
        package: package.base.manifest.name.clone(),
        generation: package.generation,
        identity: expected.identity.clone(),
        action: PublicationAction::Install,
        controllers: PublicationSectionState::Pending,
        db2: if db2_applicable {
            PublicationSectionState::Pending
        } else {
            PublicationSectionState::NotApplicable
        },
        complete: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jes_worker::ManualJesClock;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use base64::Engine;
    use continuation::{
        PendingOnlineTransfer, decode_online_machine_continuation,
        encode_online_machine_continuation, encode_online_machine_continuation_with_transfer,
    };
    use mainframe_env_cics::{
        CICS_DELAY_WORK_GENERATION, CICS_START_WORK_GENERATION, CicsApplicationEntryDefinition,
        CicsEventPostMode, CicsJavaStatus, CicsProgramDefinition,
    };
    use mainframe_env_compiler::CobolCompiler;
    use mainframe_env_compiler_api::{
        ARTIFACT_CONTRACT, ArtifactManifestV2, CompilationMode, CompileOptions, CompileTarget,
        CompilerRequest, CompilerResult, CompilerService, LEGACY_ARTIFACT_CONTRACT,
        PublishedArtifact, VersionedArtifactManifest,
    };
    use mainframe_env_db2::Db2TableDefinition;
    use mainframe_env_host_api::{CicsConditionPolicy, CicsRequest};
    use mainframe_env_interpreter::{ExecutionControl, ExecutionControlError};
    use mainframe_env_ir::{
        Attribute, CicsPlanOperation, IrLimits, ModuleBuilder, OperationIdentity,
        cics_executable_descriptor,
    };
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use mainframe_env_store::SqliteStateStore;
    use mainframe_env_store_api::{RetentionRequest, RetentionStore, WorkStore};
    use std::sync::Barrier;
    use tower::ServiceExt;

    fn session_tick() -> Result<u64, HostProblem> {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_millis(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)
    }

    fn stage_protected_start(
        server: &ProductServer,
        invocation: &Invocation,
        current_transaction: &str,
        target_transaction: &str,
        request_id: &str,
        sequence: u64,
    ) {
        let argument = |schema: &str, bytes: Vec<u8>| {
            BoundedPayload::new(schema, bytes, InvocationLimits::default()).unwrap()
        };
        let request = CicsRequest {
            operation: CicsOperation::Start,
            arguments: BTreeMap::from([
                (
                    "TRANSID".into(),
                    argument(
                        "mainframe-env.cics.literal@1",
                        target_transaction.as_bytes().to_vec(),
                    ),
                ),
                (
                    "REQID".into(),
                    argument(
                        "mainframe-env.cics.literal@1",
                        request_id.as_bytes().to_vec(),
                    ),
                ),
                (
                    "FROM".into(),
                    argument("mainframe-env.cics.storage-value@1", b"RECOVER".to_vec()),
                ),
                (
                    "INTERVAL".into(),
                    argument("mainframe-env.cics.decimal@1", b"0".to_vec()),
                ),
                (
                    "OPTION.PROTECT".into(),
                    argument("mainframe-env.cics.option@1", Vec::new()),
                ),
            ]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("recover-protected-start-{request_id}"),
                    InvocationLimits::default(),
                )
                .unwrap(),
                transaction: Some(current_transaction.into()),
            }),
        };
        server
            .cics
            .invoke(
                &EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: request
                        .mutation
                        .as_ref()
                        .map(|mutation| mutation.idempotency_key.clone()),
                    request: HostRequest::Cics(request.clone()),
                },
                request,
            )
            .unwrap();
    }

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

        let server = ProductServer::memory(config()).unwrap();
        let invocation = server
            .invocation(
                "IBMUSER",
                "clock:test",
                ServiceClass::Interactive,
                &["host.clock"],
            )
            .unwrap();
        let selected = server
            .host
            .invoke(
                &invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: 1,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: None,
                    request: HostRequest::Clock(ClockRequest::UtcTimestamp),
                },
            )
            .into_transaction_parts()
            .0;
        assert!(matches!(
            selected.outcome,
            Ok(HostResult::Clock(value)) if value.len() == 17
        ));
    }

    /// Regression tests for #195. Measured canonical sizes (bf749b2's
    /// encoding): requests are 135/127/127 bytes and results are 109/100/101
    /// bytes for UtcTimestamp/Date/Time, all over the old 64-byte budget.
    #[test]
    fn system_clock_budgets_fit_every_canonical_clock_request_and_result() {
        let limits = InvocationLimits::default();
        let descriptor = &SystemClockProvider::new(limits).descriptor;
        for (request, result) in [
            (ClockRequest::UtcTimestamp, "0".repeat(17)),
            (ClockRequest::Date, "0".repeat(8)),
            (ClockRequest::Time, "0".repeat(9)),
        ] {
            let host_request = HostRequest::Clock(request);
            let request_size = mainframe_env_host_api::canonical_request_size(
                &host_request,
                descriptor
                    .max_request_bytes
                    .min(mainframe_env_host_api::MAX_CANONICAL_EFFECT_BYTES),
            );
            assert!(
                request_size.is_ok(),
                "clock request {request:?} canonical size exceeds max_request_bytes={}",
                descriptor.max_request_bytes
            );
            let host_result: Result<HostResult, HostProblem> = Ok(HostResult::Clock(result));
            let result_size = mainframe_env_host_api::canonical_result_size(
                &host_result,
                descriptor
                    .max_result_bytes
                    .min(mainframe_env_host_api::MAX_CANONICAL_EFFECT_BYTES),
            );
            assert!(
                result_size.is_ok(),
                "clock result for {request:?} canonical size exceeds max_result_bytes={}",
                descriptor.max_result_bytes
            );
        }
    }

    /// Regression test for #195. Every `ClockRequest` variant must round-trip
    /// through `ScopedHostService::invoke` with the real `SystemClockProvider`.
    #[test]
    fn system_clock_request_round_trips_through_the_scoped_host_service() {
        let l = InvocationLimits::default();
        let capability = CapabilityId::new("host.clock", l).unwrap();
        let host = ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![Arc::new(SystemClockProvider::new(l)) as Arc<dyn HostProvider>],
                    l,
                )
                .unwrap(),
            ),
            HostLimits::default(),
        );
        let invocation = Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("test", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", l).unwrap(),
                std::collections::BTreeSet::from([capability]),
                l,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("idem", l).unwrap(),
            1,
            ResourceLimits::default(),
            std::collections::BTreeMap::new(),
            l,
        )
        .unwrap();
        for (sequence, request, expected_width) in [
            (1u64, ClockRequest::UtcTimestamp, 17usize),
            (2, ClockRequest::Date, 8),
            (3, ClockRequest::Time, 9),
        ] {
            let effect = EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: 100,
                idempotency_key: None,
                request: HostRequest::Clock(request),
            };
            let (result, _audit) = host
                .invoke(&invocation, 0, false, effect)
                .into_transaction_parts();
            match result.outcome {
                Ok(HostResult::Clock(value)) => assert_eq!(
                    value.len(),
                    expected_width,
                    "clock request {request:?} returned an unexpected width"
                ),
                other => panic!("clock request {request:?} must succeed, got {other:?}"),
            }
        }
    }

    #[test]
    fn authentication_scopes_do_not_leak_invalid_request_secrets() {
        let server = ProductServer::memory(config()).unwrap();
        let invalid = "X".repeat(InvocationLimits::default().max_binding_bytes + 1);
        assert!(matches!(
            server.verify(&invalid, b"secret-that-must-not-remain"),
            Err(HostProblem::Unauthorized)
        ));
        assert_eq!(server.secrets.entry_count(), 0);
    }

    #[test]
    fn restored_session_ttls_are_bounded_and_clock_rollback_fails_closed() {
        let mut session = AuthSession {
            schema_version: AUTH_SESSION_CONTRACT.into(),
            user: "IBMUSER".into(),
            issued_tick: 10,
            last_used_tick: 10,
            absolute_expires_tick: 10 + AUTH_SESSION_ABSOLUTE_TTL_MILLIS,
            idle_expires_tick: 10 + AUTH_SESSION_IDLE_TTL_MILLIS,
            principal_epoch: format!("sha256:{:064x}", 1),
            version: 1,
        };
        let record = |session: &AuthSession| ProviderStateRecord {
            namespace: AUTH_SESSION_NAMESPACE.into(),
            key: "a".repeat(64),
            version: session.version,
            payload: encode_auth_session(session).unwrap(),
        };
        assert_eq!(decode_auth_session(&record(&session)).unwrap(), session);
        assert!(session.clock_regressed(9));
        session.absolute_expires_tick += 1;
        assert_eq!(
            decode_auth_session(&record(&session)),
            Err(HostProblem::InfrastructureFailure)
        );
    }

    #[test]
    fn auth_sessions_use_the_durable_clock_across_wall_clock_regression() {
        let durable_tick = session_tick().unwrap().saturating_add(1_000_000);
        let store = Arc::new(MemoryStore::new(Default::default()));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let first = ProductServer::open_with_clock(
            config(),
            platform,
            Arc::new(ManualJesClock::new(durable_tick)),
        )
        .unwrap();
        first.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        let verified = first.verify("IBMUSER", b"TESTPASS").unwrap();
        let token = first.create_session(&verified).unwrap();
        let key = auth_session_key(&token);
        let stored = store
            .get_provider_state(AUTH_SESSION_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_auth_session(&stored).unwrap().issued_tick,
            durable_tick
        );
        drop(first);

        let platform: Arc<dyn PlatformStore> = store;
        let reopened = ProductServer::open_with_clock(
            config(),
            platform,
            Arc::new(ManualJesClock::new(durable_tick)),
        )
        .unwrap();
        assert_eq!(
            reopened
                .principal(Authentication::Bearer(token.clone()))
                .unwrap(),
            "IBMUSER"
        );
        let (user, rotated) = reopened.rotate_session(&token).unwrap();
        assert_eq!(user, "IBMUSER");
        assert_eq!(
            reopened.principal(Authentication::Bearer(rotated)).unwrap(),
            "IBMUSER"
        );
    }

    #[test]
    fn bearer_sessions_store_only_hashes_expire_and_follow_principal_epoch() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        let verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
        let token = server.create_session(&verified).unwrap();
        let key = auth_session_key(&token);
        assert_ne!(key, token);
        let rows = server
            .store
            .list_provider_state(AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, key);
        assert!(!String::from_utf8_lossy(&rows[0].payload).contains(&token));
        assert_eq!(
            server
                .principal(Authentication::Bearer(token.clone()))
                .unwrap(),
            "IBMUSER"
        );
        let (rotated_user, rotated) = server.rotate_session(&token).unwrap();
        assert_eq!(rotated_user, "IBMUSER");
        assert_ne!(rotated, token);
        assert_eq!(
            server.principal(Authentication::Bearer(token)),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            server
                .principal(Authentication::Bearer(rotated.clone()))
                .unwrap(),
            "IBMUSER"
        );

        server
            .racf
            .set_user_state("IBMUSER", false, true, false)
            .unwrap();
        assert_eq!(
            server.principal(Authentication::Bearer(rotated)),
            Err(HostProblem::Unauthorized)
        );
        assert!(
            server
                .store
                .list_provider_state(AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
                .unwrap()
                .is_empty()
        );

        server
            .racf
            .set_user_state("IBMUSER", false, false, false)
            .unwrap();
        let stale_verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .set_user_state("IBMUSER", false, false, true)
            .unwrap();
        server
            .racf
            .set_user_state("IBMUSER", false, false, false)
            .unwrap();
        assert_eq!(
            server.create_session(&stale_verified),
            Err(HostProblem::Unauthorized)
        );
        let verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
        let expired = server.create_session(&verified).unwrap();
        let expired_key = auth_session_key(&expired);
        let mut expired_session = server.sessions.lock().unwrap()[&expired_key].clone();
        let previous_version = expired_session.version;
        expired_session.issued_tick = 0;
        expired_session.last_used_tick = 0;
        expired_session.idle_expires_tick = 1;
        expired_session.absolute_expires_tick = 2;
        expired_session.version += 1;
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: AUTH_SESSION_NAMESPACE.into(),
                    key: expired_key,
                    version: expired_session.version,
                    payload: encode_auth_session(&expired_session).unwrap(),
                },
                Some(previous_version),
            )
            .unwrap();
        assert_eq!(
            server.principal(Authentication::Bearer(expired)),
            Err(HostProblem::Unauthorized)
        );
    }

    #[test]
    fn deleted_and_recreated_principal_cannot_reuse_an_authentication_epoch() {
        use mainframe_env_racf::CommandContext;

        let server = ProductServer::memory(config()).unwrap();
        let admin_reference = SecretRef::new("test:epoch-admin", HostLimits::default()).unwrap();
        let _admin_secret = server
            .secrets
            .scoped(&admin_reference, b"ADMIN-PASS1".to_vec())
            .unwrap();
        server
            .racf
            .bootstrap_administrator("RACFADM", &admin_reference)
            .unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let command = |id, tick| CommandContext::new(admin.clone(), id, "EPOCH-ABA", tick).unwrap();
        server
            .racf
            .execute_command(
                &command("EPOCH-ADD-1", 2),
                "ADDUSER USER1 PASSWORD('VALID-PASS1')",
            )
            .unwrap();

        let stale_verified = server.verify("USER1", b"VALID-PASS1").unwrap();
        let stale_token = server.create_session(&stale_verified).unwrap();
        server
            .racf
            .execute_command(&command("EPOCH-DELETE", 3), "DELUSER USER1")
            .unwrap();
        server
            .racf
            .execute_command(
                &command("EPOCH-ADD-2", 4),
                "ADDUSER USER1 PASSWORD('VALID-PASS1')",
            )
            .unwrap();

        assert_eq!(
            server.principal(Authentication::Bearer(stale_token)),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            server.create_session(&stale_verified),
            Err(HostProblem::Unauthorized)
        );
        let fresh_verified = server.verify("USER1", b"VALID-PASS1").unwrap();
        assert_ne!(
            stale_verified.principal_epoch,
            fresh_verified.principal_epoch
        );
        assert!(server.create_session(&fresh_verified).is_ok());
    }

    #[test]
    fn sessions_are_per_user_bounded_and_legacy_raw_tokens_are_revoked_on_open() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: LEGACY_AUTH_SESSION_NAMESPACE.into(),
                    key: "raw-legacy-bearer".into(),
                    version: 1,
                    payload: b"IBMUSER".to_vec(),
                },
                None,
            )
            .unwrap();
        let server = ProductServer::open(
            config(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert!(
            store
                .list_provider_state(LEGACY_AUTH_SESSION_NAMESPACE, MAX_AUTH_SESSIONS)
                .unwrap()
                .is_empty()
        );
        server.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        let verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
        let tokens = (0..MAX_AUTH_SESSIONS_PER_USER)
            .map(|_| server.create_session(&verified).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            server.create_session(&verified),
            Err(HostProblem::ResourceExhausted)
        );
        let expired_key = auth_session_key(tokens.last().unwrap());
        let mut expired = server.sessions.lock().unwrap()[&expired_key].clone();
        let previous_version = expired.version;
        expired.issued_tick = 0;
        expired.last_used_tick = 0;
        expired.idle_expires_tick = 1;
        expired.absolute_expires_tick = 2;
        expired.version += 1;
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: AUTH_SESSION_NAMESPACE.into(),
                    key: expired_key,
                    version: expired.version,
                    payload: encode_auth_session(&expired).unwrap(),
                },
                Some(previous_version),
            )
            .unwrap();
        drop(server);
        let reopened = ProductServer::open(
            config(),
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert_eq!(
            reopened
                .principal(Authentication::Bearer(tokens[0].clone()))
                .unwrap(),
            "IBMUSER"
        );
        assert_eq!(
            reopened.principal(Authentication::Bearer(tokens.last().unwrap().clone())),
            Err(HostProblem::Unauthorized)
        );
    }

    #[test]
    fn shared_store_session_quota_is_atomically_fenced_across_servers() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(ManualJesClock::new(100));
        let first = ProductServer::open_with_clock(config(), store.clone(), clock.clone()).unwrap();
        first.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        let second = ProductServer::open_with_clock(config(), store, clock).unwrap();
        let first_verified = first.verify("IBMUSER", b"TESTPASS").unwrap();
        let second_verified = first_verified.clone();
        for _ in 0..(MAX_AUTH_SESSIONS_PER_USER - 1) {
            first.create_session(&first_verified).unwrap();
        }
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let first_worker = {
            let server = first.clone();
            let barrier = barrier.clone();
            let verified = first_verified.clone();
            std::thread::spawn(move || {
                barrier.wait();
                server.create_session(&verified)
            })
        };
        let second_worker = {
            let server = second.clone();
            let barrier = barrier.clone();
            let verified = second_verified;
            std::thread::spawn(move || {
                barrier.wait();
                server.create_session(&verified)
            })
        };
        barrier.wait();
        let results = [first_worker.join().unwrap(), second_worker.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| { result.as_ref().err() == Some(&HostProblem::ResourceExhausted) })
                .count(),
            1
        );
        let index = load_auth_session_index(&*first.store).unwrap();
        assert_eq!(index.sessions.len(), MAX_AUTH_SESSIONS_PER_USER);
    }

    #[test]
    fn session_index_reconciliation_retries_a_concurrent_create_snapshot() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        let verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
        let scanned = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        let worker = {
            let store = server.store.clone();
            let scanned = scanned.clone();
            let resume = resume.clone();
            std::thread::spawn(move || {
                let mut pause = true;
                reconcile_auth_session_index_with_scan_hook(&*store, || {
                    if pause {
                        pause = false;
                        scanned.wait();
                        resume.wait();
                    }
                })
            })
        };

        scanned.wait();
        let token = server.create_session(&verified).unwrap();
        let key = auth_session_key(&token);
        resume.wait();
        worker.join().unwrap().unwrap();

        let index = load_auth_session_index(&*server.store).unwrap();
        assert_eq!(
            index.sessions.get(&key).map(String::as_str),
            Some("IBMUSER")
        );
        assert_eq!(
            server.principal(Authentication::Bearer(token)).unwrap(),
            "IBMUSER"
        );
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

    fn published_fixture(name: &str, body: &str) -> PublishedArtifact {
        let source = format!(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. {name}.\nPROCEDURE DIVISION.\n{body}\nSTOP RUN.\n"
        );
        published_source_fixture(name, &source)
    }

    fn published_source_fixture(name: &str, source: &str) -> PublishedArtifact {
        let limits = SourceLimits::default();
        let path = LogicalPath::new(format!("{name}.cbl"), limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    path.as_str(),
                    source.as_bytes().to_vec(),
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
            panic!("fixture did not publish");
        };
        artifact
    }

    fn malformed_layout_payload() -> Vec<u8> {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        builder.add_storage("result", 3, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "config", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                    ("display_sign".into(), Attribute::Text("compatible".into())),
                    ("address_mode".into(), Attribute::Text("32".into())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                mainframe_env_ir::cobol_layout_definition_identity(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("RESULT".into())),
                    ("simple_name".into(), Attribute::Text("RESULT".into())),
                    ("category".into(), Attribute::Text("numeric_display".into())),
                    ("picture".into(), Attribute::Text("9(3)".into())),
                    ("digits".into(), Attribute::Text("not-an-integer".into())),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(3)),
                    ("element_length".into(), Attribute::Integer(3)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
    }

    fn malformed_cics_plan_payload() -> Vec<u8> {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "config", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                    ("display_sign".into(), Attribute::Text("compatible".into())),
                    ("address_mode".into(), Attribute::Text("32".into())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let descriptor = cics_executable_descriptor(CicsPlanOperation::Syncpoint);
        builder
            .add_operation(
                block,
                descriptor.identity(),
                Vec::new(),
                0,
                BTreeMap::from([("cics_plan".into(), Attribute::Bytes(Vec::new()))]),
                descriptor.effects.to_vec(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
    }

    #[test]
    fn product_install_rejects_manifest_and_payload_mismatches_before_catalog_mutation() {
        let server = ProductServer::memory(config()).unwrap();
        let artifact = published_fixture("ADMIT", "DISPLAY 'ADMITTED'.");

        let assert_rejected =
            |name: &str, manifest: VersionedArtifactManifest, payload: Vec<u8>| {
                let digest: [u8; 32] = Sha256::digest(&payload).into();
                let reference = ArtifactRef::new(
                    format!("sha256:{}", hex_digest(&digest)),
                    InvocationLimits::default(),
                )
                .unwrap();
                let result = server.install_batch_programs(vec![BatchProgramDefinition {
                    name: name.into(),
                    artifact: reference.clone(),
                    payload,
                    manifest,
                    semantic_identity: artifact.semantic_id().to_reference(),
                }]);
                assert_eq!(result, Err(HostProblem::ProviderFailure));
                assert!(server.artifacts.get_artifact(&reference).unwrap().is_none());
                assert!(
                    server
                        .store
                        .get_provider_state("batch-program", name)
                        .unwrap()
                        .is_none()
                );
            };

        let mut missing = artifact.manifest().clone();
        missing.dialect_contracts.clear();
        assert_rejected(
            "MISSING",
            VersionedArtifactManifest::V3(missing),
            artifact.payload().to_vec(),
        );
        let mut extra = artifact.manifest().clone();
        extra.dialect_contracts.insert("invented.dialect@1".into());
        assert_rejected(
            "EXTRA",
            VersionedArtifactManifest::V3(extra),
            artifact.payload().to_vec(),
        );
        let mut wrong_major = artifact.manifest().clone();
        let first = wrong_major.dialect_contracts.pop_first().unwrap();
        let namespace = first.rsplit_once('@').unwrap().0;
        wrong_major
            .dialect_contracts
            .insert(format!("{namespace}@99"));
        assert_rejected(
            "WRONGMAJOR",
            VersionedArtifactManifest::V3(wrong_major),
            artifact.payload().to_vec(),
        );
        let other = published_fixture("OTHER", "EXEC CICS SYNCPOINT END-EXEC.");
        assert_rejected(
            "WRONGMANIFEST",
            VersionedArtifactManifest::V3(other.manifest().clone()),
            artifact.payload().to_vec(),
        );
        let mut missing_host = artifact.manifest().clone();
        missing_host.host_interfaces.remove("mainframe-env.cics@1");
        assert_rejected(
            "MISSINGHOST",
            VersionedArtifactManifest::V3(missing_host),
            artifact.payload().to_vec(),
        );
        let mut wrong_generation = artifact.manifest().clone();
        wrong_generation.compiler_generation = "mainframe-env-cobol-9.9.9".into();
        assert_rejected(
            "WRONGGEN",
            VersionedArtifactManifest::V3(wrong_generation),
            artifact.payload().to_vec(),
        );
        let mut unknown_option = artifact.manifest().clone();
        let mut options = unknown_option.options.values().clone();
        options.insert("unreviewed-runtime-switch".into(), "enabled".into());
        unknown_option.options = CompileOptions::new(options).unwrap();
        assert_rejected(
            "BADOPTION",
            VersionedArtifactManifest::V3(unknown_option),
            artifact.payload().to_vec(),
        );
        for (name, key, first, second) in [
            (
                "FLIPPEDARITH",
                "cobol.effective-arith",
                "extended",
                "compatible",
            ),
            (
                "FLIPPEDSIGN",
                "cobol.effective-dispsign",
                "compatible",
                "separate",
            ),
            ("FLIPPEDLP", "cobol.effective-lp", "32", "64"),
        ] {
            let mut mismatch = artifact.manifest().clone();
            let mut options = mismatch.options.values().clone();
            let flipped = if options.get(key).is_some_and(|value| value == first) {
                second
            } else {
                first
            };
            options.insert(key.into(), flipped.into());
            mismatch.options = CompileOptions::new(options).unwrap();
            assert_rejected(
                name,
                VersionedArtifactManifest::V3(mismatch),
                artifact.payload().to_vec(),
            );
        }
        assert_rejected(
            "MALFORMED",
            VersionedArtifactManifest::V3(artifact.manifest().clone()),
            b"not canonical IR".to_vec(),
        );
        assert_eq!(
            artifact.manifest().dialect_contracts,
            BTreeSet::from(["mainframe.core.cobol@1".into()])
        );
        assert_rejected(
            "BADLAYOUT",
            VersionedArtifactManifest::V3(artifact.manifest().clone()),
            malformed_layout_payload(),
        );

        let typed = published_fixture("TYPEDPLAN", "EXEC CICS SYNCPOINT END-EXEC.");
        let payload = malformed_cics_plan_payload();
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        let reference = ArtifactRef::new(
            format!("sha256:{}", hex_digest(&digest)),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            server.install_batch_programs(vec![BatchProgramDefinition {
                name: "BADTYPEDPLAN".into(),
                artifact: reference.clone(),
                payload,
                manifest: VersionedArtifactManifest::V3(typed.manifest().clone()),
                semantic_identity: typed.semantic_id().to_reference(),
            }]),
            Err(HostProblem::ProviderFailure)
        );
        assert!(server.artifacts.get_artifact(&reference).unwrap().is_none());
        assert!(
            server
                .store
                .get_provider_state("batch-program", "BADTYPEDPLAN")
                .unwrap()
                .is_none()
        );

        let mut wrong_identity = BatchProgramDefinition::current("WRONGIDENTITY", &artifact);
        wrong_identity.artifact =
            ArtifactRef::new(format!("sha256:{:064x}", 0), InvocationLimits::default()).unwrap();
        assert_eq!(
            server.install_batch_programs(vec![wrong_identity]),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            server
                .store
                .get_provider_state("batch-program", "WRONGIDENTITY")
                .unwrap()
                .is_none()
        );

        let good = BatchProgramDefinition::current("GOODFIRST", &artifact);
        let good_id = good.artifact.clone();
        let mut bad = BatchProgramDefinition::current("BADSECOND", &artifact);
        let VersionedArtifactManifest::V3(bad_manifest) = &mut bad.manifest else {
            unreachable!()
        };
        bad_manifest.dialect_contracts.insert("unexpected@7".into());
        assert_eq!(
            server.install_batch_programs(vec![good, bad]),
            Err(HostProblem::ProviderFailure)
        );
        assert!(server.artifacts.get_artifact(&good_id).unwrap().is_none());
    }

    struct StaticArtifactStore {
        record: Mutex<ArtifactRecord>,
    }

    impl ArtifactStore for StaticArtifactStore {
        fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
            Ok(ArtifactStoreHealth {
                readable: true,
                writable: true,
                used_objects: Some(1),
                max_objects: Some(2),
                used_bytes: None,
                max_bytes: None,
            })
        }

        fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
            let current = self
                .record
                .lock()
                .map_err(|_| StoreError::Infrastructure("poisoned artifact fixture".into()))?;
            if *current == record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            }
        }

        fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
            let record = self
                .record
                .lock()
                .map_err(|_| StoreError::Infrastructure("poisoned artifact fixture".into()))?;
            Ok((record.artifact == *id).then(|| record.clone()))
        }

        fn delete_artifact(&self, _: &ArtifactRef) -> Result<(), StoreError> {
            Err(StoreError::NotFound)
        }
    }

    #[test]
    fn product_reload_rejects_missing_unsupported_or_incompatible_executable_metadata() {
        let artifact = published_fixture("RELOAD", "DISPLAY 'RELOAD'.");
        let reject = |mutate: fn(&mut ArtifactRecord), rebind: bool| {
            let store = Arc::new(MemoryStore::new(Default::default()));
            let reference = ArtifactRef::new(
                artifact.content_id().to_reference(),
                InvocationLimits::default(),
            )
            .unwrap();
            let mut record = crate::cobol::artifact::published_artifact_record(&artifact).unwrap();
            mutate(&mut record);
            if rebind {
                let payload_digest = record.payload_digest;
                let metadata = record.executable.as_mut().unwrap();
                metadata.manifest_payload_digest =
                    metadata.expected_manifest_payload_digest(&payload_digest);
            }
            let artifacts = Arc::new(StaticArtifactStore {
                record: Mutex::new(record),
            });
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "batch-program".into(),
                        key: "RELOAD".into(),
                        version: 1,
                        payload: reference.as_str().as_bytes().to_vec(),
                    },
                    None,
                )
                .unwrap();
            let mut server_config = config();
            server_config.store_profile = crate::StoreProfile::Postgres;
            server_config.artifact_profile = ArtifactProfile::Shared;
            server_config.postgres_url_reference =
                Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
            let platform: Arc<dyn PlatformStore> = store.clone();
            let artifacts: Arc<dyn ArtifactStore> = artifacts;
            assert!(matches!(
                ProductServer::open_with_artifact_store(
                    server_config,
                    platform,
                    Arc::new(MemorySecretResolver::default()),
                    default_program_router(),
                    artifacts,
                ),
                Err(HostProblem::ProviderFailure)
            ));
        };
        reject(|record| record.executable = None, false);
        reject(
            |record| {
                record.executable.as_mut().unwrap().artifact_contract =
                    "mainframe-env.artifact@1".into();
            },
            true,
        );
        reject(
            |record| {
                record.executable.as_mut().unwrap().compatibility_profile =
                    "mainframe-env.cobol.other-runtime@1".into();
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .host_interfaces
                    .insert("mainframe-env.future-host@9".into());
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .host_interfaces
                    .remove("mainframe-env.cics@1");
            },
            true,
        );
        reject(
            |record| {
                record.executable.as_mut().unwrap().compiler_generation =
                    "mainframe-env-cobol-9.9.9".into();
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .options
                    .insert("unreviewed-runtime-switch".into(), "enabled".into());
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .options
                    .insert("cobol.effective-arith".into(), "compatible".into());
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .options
                    .insert("cobol.effective-dispsign".into(), "separate".into());
            },
            true,
        );
        reject(
            |record| {
                record
                    .executable
                    .as_mut()
                    .unwrap()
                    .options
                    .insert("cobol.effective-lp".into(), "64".into());
            },
            true,
        );
        reject(
            |record| {
                record.executable.as_mut().unwrap().semantic_identity =
                    format!("semantic-sha256:{:064x}", 9);
            },
            false,
        );
    }

    #[test]
    fn invalid_startup_artifact_preflight_leaves_store_and_router_unmodified() {
        let published = published_fixture("STARTUP", "DISPLAY 'STARTUP'.");
        let valid = crate::cobol::artifact::published_artifact_record(&published).unwrap();
        let reference = valid.artifact.clone();
        let mut invalid = valid.clone();
        invalid.executable.as_mut().unwrap().compiler_generation =
            "mainframe-env-cobol-9.9.9".into();
        let payload_digest = invalid.payload_digest;
        let metadata = invalid.executable.as_mut().unwrap();
        metadata.manifest_payload_digest =
            metadata.expected_manifest_payload_digest(&payload_digest);
        let artifacts = Arc::new(StaticArtifactStore {
            record: Mutex::new(invalid),
        });
        let store = Arc::new(MemoryStore::new(Default::default()));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "STARTUP".into(),
                    version: 1,
                    payload: reference.as_str().as_bytes().to_vec(),
                },
                None,
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: LEGACY_AUTH_SESSION_NAMESPACE.into(),
                    key: "retained-before-preflight".into(),
                    version: 1,
                    payload: b"must remain".to_vec(),
                },
                None,
            )
            .unwrap();
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Postgres;
        server_config.artifact_profile = ArtifactProfile::Shared;
        server_config.postgres_url_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
        let router = default_program_router();
        let platform: Arc<dyn PlatformStore> = store.clone();
        let artifact_store: Arc<dyn ArtifactStore> = artifacts.clone();
        assert!(matches!(
            ProductServer::open_with_artifact_store(
                server_config.clone(),
                platform,
                Arc::new(MemorySecretResolver::default()),
                router.clone(),
                artifact_store,
            ),
            Err(HostProblem::ProviderFailure)
        ));
        assert!(
            store
                .get_provider_state(LEGACY_AUTH_SESSION_NAMESPACE, "retained-before-preflight")
                .unwrap()
                .is_some()
        );

        *artifacts.record.lock().unwrap() = valid;
        let platform: Arc<dyn PlatformStore> = store;
        let artifact_store: Arc<dyn ArtifactStore> = artifacts;
        ProductServer::open_with_artifact_store(
            server_config,
            platform,
            Arc::new(MemorySecretResolver::default()),
            router,
            artifact_store,
        )
        .expect("failed preflight must not bind the shared router");
    }

    #[test]
    fn installed_call_preflight_rejects_before_protocol_or_replay_reservation() {
        use mainframe_env_host_api::{ProgramName, ProgramRequest};

        let published = published_fixture("PREFLIGHT", "DISPLAY 'PREFLIGHT'.");
        let record = crate::cobol::artifact::published_artifact_record(&published).unwrap();
        let artifacts = Arc::new(StaticArtifactStore {
            record: Mutex::new(record),
        });
        let store = Arc::new(MemoryStore::new(Default::default()));
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Postgres;
        server_config.artifact_profile = ArtifactProfile::Shared;
        server_config.postgres_url_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
        let platform: Arc<dyn PlatformStore> = store.clone();
        let artifact_store: Arc<dyn ArtifactStore> = artifacts.clone();
        let server = ProductServer::open_with_artifact_store(
            server_config,
            platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            artifact_store,
        )
        .unwrap();
        server
            .install_batch_programs(vec![BatchProgramDefinition::current(
                "PREFLIGHT",
                &published,
            )])
            .unwrap();
        let mut record = artifacts.record.lock().unwrap();
        record
            .executable
            .as_mut()
            .unwrap()
            .options
            .insert("cobol.effective-arith".into(), "compatible".into());
        let payload_digest = record.payload_digest;
        let metadata = record.executable.as_mut().unwrap();
        metadata.manifest_payload_digest =
            metadata.expected_manifest_payload_digest(&payload_digest);
        drop(record);

        let limits = InvocationLimits::default();
        let capability = CapabilityId::new("host.program.invoke", limits).unwrap();
        let parent = Invocation::new(
            RequestId::new("preflight-parent-request", limits).unwrap(),
            ExecutionId::new("preflight-parent-execution", limits).unwrap(),
            RunUnitId::new("preflight-parent-run", limits).unwrap(),
            None,
            Selector::new("program:PARENT", limits).unwrap(),
            ArtifactRef::new("artifact:parent", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::from([capability.clone()]),
                limits,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            u64::MAX,
            TraceId::new("preflight-parent-trace", limits).unwrap(),
            IdempotencyKey::new("preflight-parent-key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
        .with_provider_generations(BTreeMap::from([(capability, "1".into())]), limits)
        .unwrap();
        let effect = EffectRequest {
            run_unit: parent.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: parent.deadline_tick,
            idempotency_key: Some(IdempotencyKey::new("preflight-call", limits).unwrap()),
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new("PREFLIGHT", 128).unwrap(),
                payload: BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    b"{}".to_vec(),
                    limits,
                )
                .unwrap(),
                service: None,
            }),
        };
        assert_eq!(
            server.program.invoke(&parent, effect).outcome,
            Err(HostProblem::ProviderFailure)
        );
        for namespace in [
            crate::cobol::retention::CALL_PROTOCOL_NAMESPACE,
            crate::cobol::retention::CALL_REPLAY_NAMESPACE,
        ] {
            assert!(store.list_provider_state(namespace, 8).unwrap().is_empty());
        }
    }

    async fn execute_installed_batch_job(
        server: &Arc<ProductServer>,
        job_name: &str,
        program: &str,
    ) -> Vec<u8> {
        let app = server.router();
        let response = call(
            &app,
            Method::PUT,
            "/zosmf/restjobs/jobs",
            format!("//{job_name} JOB CLASS=A\n//STEP1 EXEC PGM={program}\n"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let job: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65_536).await.unwrap()).unwrap();
        let id = job["jobid"].as_str().unwrap();
        let completed = wait_for_terminal_job(server, id).await;
        assert_eq!(completed.return_code, Some(0));
        let files = call(
            &app,
            Method::GET,
            &format!("/zosmf/restjobs/jobs/{job_name}/{id}/files"),
            "",
        )
        .await;
        assert_eq!(files.status(), StatusCode::OK);
        let files: Value =
            serde_json::from_slice(&to_bytes(files.into_body(), 65_536).await.unwrap()).unwrap();
        let mut output = Vec::new();
        for file in files.as_array().unwrap() {
            let file_id = file["id"].as_u64().unwrap();
            let records = call(
                &app,
                Method::GET,
                &format!("/zosmf/restjobs/jobs/{job_name}/{id}/files/{file_id}/records"),
                "",
            )
            .await;
            if records.status() == StatusCode::OK {
                output.extend_from_slice(&to_bytes(records.into_body(), 65_536).await.unwrap());
            }
        }
        output
    }

    #[tokio::test]
    async fn current_v3_artifact_installs_reloads_and_executes_with_exact_manifest() {
        let published = published_fixture("CURRV3", "DISPLAY 'CURRENT-V3'.");
        let definition = BatchProgramDefinition::current("CURRV3", &published);
        let artifact = definition.artifact.clone();
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-current-artifact-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            session_tick().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = root.join("artifacts");
        let secrets = Arc::new(MemorySecretResolver::default());
        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let first = ProductServer::open(
            server_config.clone(),
            first_platform,
            secrets.clone(),
            default_program_router(),
        )
        .unwrap();
        first.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        first.install_batch_programs(vec![definition]).unwrap();
        let stored = first.artifacts.get_artifact(&artifact).unwrap().unwrap();
        let metadata = stored.executable.as_ref().unwrap();
        assert_eq!(metadata.artifact_contract, ARTIFACT_CONTRACT);
        assert_eq!(
            metadata.compatibility_profile,
            crate::cobol::artifact::COBOL_REFERENCE_COMPATIBILITY_PROFILE
        );
        assert_eq!(
            metadata.compiler_generation,
            published.manifest().compiler_generation
        );
        assert_eq!(metadata.target, published.manifest().target.as_str());
        assert_eq!(&metadata.options, published.manifest().options.values());
        assert_eq!(
            &metadata.host_interfaces,
            &published.manifest().host_interfaces
        );
        assert_eq!(metadata.ir_contract, published.manifest().ir_contract);
        assert_eq!(
            metadata.dialect_contracts.as_ref(),
            Some(&published.manifest().dialect_contracts)
        );
        assert_eq!(
            metadata.semantic_identity,
            published.semantic_id().to_reference()
        );
        assert!(metadata.validates_payload(&stored.payload_digest));
        drop((first, first_store));

        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let second = ProductServer::open(
            server_config,
            second_platform,
            secrets,
            default_program_router(),
        )
        .unwrap();
        let reloaded = second.artifacts.get_artifact(&artifact).unwrap().unwrap();
        assert_eq!(reloaded, stored);
        let accepted = admit_executable_artifact(&reloaded).unwrap();
        assert_eq!(accepted.source_contract(), ARTIFACT_CONTRACT);
        assert_eq!(accepted.content_id().to_reference(), artifact.as_str());
        assert_eq!(accepted.manifest(), published.manifest());
        let output = execute_installed_batch_job(&second, "CURRJOB", "CURRV3").await;
        assert!(
            output
                .windows(b"CURRENT-V3".len())
                .any(|part| part == b"CURRENT-V3")
        );
        drop((second, second_store));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn retained_historical_v2_artifact_installs_reloads_and_executes_unchanged() {
        const HISTORICAL_SOURCE_COMMIT: &str = "c029219f0dd647d9255b4334d93167f5524d062d";
        const HISTORICAL_SHA256: &str =
            "cf5de374e76c07ff001af9a20053fd8692f55337a4ebece2085ce62c436d58db";
        const HISTORICAL_SEMANTIC_ID: &str =
            "semantic-sha256:f27f98bc6fa22cc145c9df52483b26346b2e1a9aac3272df49fa14f731ec45c9";
        const HISTORICAL_B64: &str =
            include_str!("../../../../conformance/0.9/cobol/artifact-v2-c029219.b64");
        let provenance: Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.9/cobol/artifact-v2-c029219.json"
        ))
        .unwrap();
        assert_eq!(provenance["source_commit"], HISTORICAL_SOURCE_COMMIT);
        assert_eq!(provenance["artifact_contract"], LEGACY_ARTIFACT_CONTRACT);
        assert_eq!(
            provenance["content_id"],
            format!("sha256:{HISTORICAL_SHA256}")
        );
        assert_eq!(provenance["semantic_id"], HISTORICAL_SEMANTIC_ID);
        let payload = base64::engine::general_purpose::STANDARD
            .decode(HISTORICAL_B64.trim())
            .unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&payload)), HISTORICAL_SHA256);
        assert_eq!(
            HISTORICAL_SOURCE_COMMIT,
            "c029219f0dd647d9255b4334d93167f5524d062d"
        );
        assert_eq!(payload.len(), 1261);
        let manifest = ArtifactManifestV2 {
            compiler_generation: "mainframe-env-cobol-0.8.3".into(),
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::from([
                ("cobol.effective-arith".into(), "extended".into()),
                ("cobol.effective-dispsign".into(), "compatible".into()),
                ("cobol.effective-lp".into(), "32".into()),
            ]))
            .unwrap(),
            host_interfaces: BTreeSet::from([
                "mainframe-env.host@1".into(),
                "mainframe-env.cics@1".into(),
            ]),
            ir_contract: mainframe_env_ir::IR_ENVELOPE_CONTRACT.into(),
        };
        let artifact = ArtifactRef::new(
            format!("sha256:{HISTORICAL_SHA256}"),
            InvocationLimits::default(),
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-historical-artifact-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            session_tick().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = root.join("artifacts");
        let secrets = Arc::new(MemorySecretResolver::default());
        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let first = ProductServer::open(
            server_config.clone(),
            first_platform,
            secrets.clone(),
            default_program_router(),
        )
        .unwrap();
        first.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        first
            .install_batch_programs(vec![BatchProgramDefinition {
                name: "HISTV2".into(),
                artifact: artifact.clone(),
                payload: payload.clone(),
                manifest: VersionedArtifactManifest::V2(manifest.clone()),
                semantic_identity: HISTORICAL_SEMANTIC_ID.into(),
            }])
            .unwrap();
        let stored = first.artifacts.get_artifact(&artifact).unwrap().unwrap();
        let metadata = stored.executable.as_ref().unwrap();
        assert_eq!(metadata.artifact_contract, LEGACY_ARTIFACT_CONTRACT);
        assert_eq!(
            metadata.compatibility_profile,
            crate::cobol::artifact::COBOL_REFERENCE_COMPATIBILITY_PROFILE
        );
        assert_eq!(
            metadata.compiler_generation,
            manifest.compiler_generation.as_str()
        );
        assert_eq!(metadata.target, manifest.target.as_str());
        assert_eq!(&metadata.options, manifest.options.values());
        assert_eq!(&metadata.host_interfaces, &manifest.host_interfaces);
        assert_eq!(metadata.ir_contract, manifest.ir_contract.as_str());
        assert_eq!(metadata.dialect_contracts, None);
        assert_eq!(metadata.semantic_identity, HISTORICAL_SEMANTIC_ID);
        assert!(metadata.validates_payload(&stored.payload_digest));
        assert_eq!(stored.payload, payload);
        let stored_digest: [u8; 32] = Sha256::digest(&stored.payload).into();
        assert_eq!(stored.payload_digest, stored_digest);
        drop((first, first_store));

        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let second = ProductServer::open(
            server_config,
            second_platform,
            secrets,
            default_program_router(),
        )
        .unwrap();
        let reloaded = second.artifacts.get_artifact(&artifact).unwrap().unwrap();
        assert_eq!(reloaded, stored);
        let accepted = admit_executable_artifact(&reloaded).unwrap();
        assert_eq!(accepted.source_contract(), LEGACY_ARTIFACT_CONTRACT);
        assert_eq!(accepted.content_id().to_reference(), artifact.as_str());
        assert_eq!(
            accepted.manifest().dialect_contracts,
            BTreeSet::from(["mainframe.core.cobol@1".into()])
        );
        assert_eq!(
            provenance["derived_dialect_contracts"],
            serde_json::json!(["mainframe.core.cobol@1"])
        );
        let output = execute_installed_batch_job(&second, "HISTJOB", "HISTV2").await;
        assert!(output.windows(b"HELLO".len()).any(|part| part == b"HELLO"));
        drop((second, second_store));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn job_grants_come_from_the_verified_plan_not_comments_or_inline_data() {
        let server = ProductServer::memory(config()).unwrap();
        let inert = server
            .batch
            .plan(&JclBundle {
                primary: "//SAFEJOB JOB CLASS=A\n//* PGM=DFSRRC00 EXEC SQL MQPUT\n//STEP1 EXEC PGM=IEFBR14\n//SYSIN DD *\nEXEC SQL DELETE FROM SECRET.TABLE; MQPUT\n/*\n".into(),
                ..Default::default()
            })
            .unwrap();
        let grants = job_capabilities(server.store.as_ref(), &inert).unwrap();
        for forbidden in [
            "host.db2.read",
            "host.db2.write",
            "host.ims.read",
            "host.ims.write",
            "host.mq.read",
            "host.mq.write",
            "host.cics.execute",
        ] {
            assert!(!grants.contains(&forbidden), "unexpected grant {forbidden}");
        }

        let db2 = server
            .batch
            .plan(&JclBundle {
                primary: "//DB2JOB JOB CLASS=A\n//STEP1 EXEC PGM=IKJEFT01\n".into(),
                ..Default::default()
            })
            .unwrap();
        let grants = job_capabilities(server.store.as_ref(), &db2).unwrap();
        assert!(grants.contains(&"host.db2.read"));
        assert!(grants.contains(&"host.db2.write"));
    }

    fn worker_test_server(
        tick: u64,
    ) -> (Arc<ProductServer>, Arc<MemoryStore>, Arc<ManualJesClock>) {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(ManualJesClock::new(tick));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let server = ProductServer::open_with_clock(config(), platform, clock.clone()).unwrap();
        (server, store, clock)
    }

    async fn wait_for_worker_health(server: &ProductServer, expected: usize) {
        for _ in 0..4_000 {
            if server.metrics().jes_worker_healthy == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!(
            "JES worker health did not reach {expected}: {:?}",
            server.metrics()
        );
    }

    struct ToggleJesClock {
        tick: AtomicU64,
        failing: AtomicBool,
    }

    impl ToggleJesClock {
        fn new(tick: u64) -> Self {
            Self {
                tick: AtomicU64::new(tick),
                failing: AtomicBool::new(false),
            }
        }
    }

    impl JesClock for ToggleJesClock {
        fn now_tick(&self) -> Result<u64, StoreError> {
            if self.failing.load(Ordering::SeqCst) {
                Err(StoreError::Infrastructure(
                    "injected JES clock failure".into(),
                ))
            } else {
                Ok(self.tick.fetch_add(1, Ordering::SeqCst))
            }
        }
    }

    struct HealthOnlyArtifactStore {
        health: Result<ArtifactStoreHealth, StoreError>,
    }

    impl ArtifactStore for HealthOnlyArtifactStore {
        fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
            self.health.clone()
        }

        fn put_artifact(&self, _: ArtifactRecord) -> Result<(), StoreError> {
            Err(StoreError::Infrastructure(
                "health-only artifact store".into(),
            ))
        }

        fn get_artifact(&self, _: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
            Err(StoreError::Infrastructure(
                "health-only artifact store".into(),
            ))
        }

        fn delete_artifact(&self, _: &ArtifactRef) -> Result<(), StoreError> {
            Err(StoreError::Infrastructure(
                "health-only artifact store".into(),
            ))
        }
    }

    fn server_with_artifact_health(
        health: Result<ArtifactStoreHealth, StoreError>,
    ) -> Arc<ProductServer> {
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Postgres;
        server_config.artifact_profile = ArtifactProfile::Shared;
        server_config.postgres_url_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        ProductServer::open_with_artifact_store(
            server_config,
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            Arc::new(HealthOnlyArtifactStore { health }),
        )
        .unwrap()
    }

    fn submit_direct(server: &ProductServer, user: &str, secret: &[u8], name: &str) -> Value {
        submit_jcl_direct(
            server,
            user,
            secret,
            format!("//{name} JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n"),
        )
    }

    fn submit_jcl_direct(server: &ProductServer, user: &str, secret: &[u8], jcl: String) -> Value {
        let response = server
            .handle(
                Authentication::Basic {
                    user: user.into(),
                    secret: secret.to_vec(),
                },
                GatewayRequest::JobSubmit {
                    jcl: jcl.into_bytes(),
                },
            )
            .unwrap();
        assert_eq!(response.status, StatusCode::CREATED);
        let mainframe_env_zosmf::GatewayBody::Json(job) = response.body else {
            panic!("job submission did not return JSON")
        };
        assert_eq!(job["status"], "ACTIVE");
        job
    }

    fn only_internal_reader_child(
        server: &ProductServer,
        parent_job_id: &str,
    ) -> (
        mainframe_env_batch::JobSnapshot,
        mainframe_env_batch::JobPlan,
    ) {
        let children = server
            .batch
            .internal_reader_children(parent_job_id)
            .unwrap();
        assert_eq!(children.len(), 1);
        children.into_iter().next().unwrap()
    }

    async fn wait_for_terminal_job(
        server: &ProductServer,
        id: &str,
    ) -> mainframe_env_batch::JobSnapshot {
        for _ in 0..2_000 {
            let job = server.batch.get(id).unwrap();
            let work_terminal = server
                .store
                .get_work(&format!("jes:{id}"))
                .unwrap()
                .is_some_and(|work| {
                    matches!(
                        work.state,
                        WorkState::Completed | WorkState::Cancelled | WorkState::DeadLetter
                    )
                });
            if job.state.terminal() && work_terminal {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("job {id} did not reach a terminal state")
    }

    #[test]
    fn jes_worker_is_fifo_fair_and_preserves_multi_user_identity() {
        let (server, store, _) = worker_test_server(100);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        server.bootstrap_user("BOB", b"BOBPASSWORD").unwrap();
        let first = submit_direct(&server, "ALICE", b"ALICEPASS", "ALICEA");
        let second = submit_direct(&server, "BOB", b"BOBPASSWORD", "BOBJOB");
        let third = submit_direct(&server, "ALICE", b"ALICEPASS", "ALICEB");
        let ids = [&first, &second, &third].map(|job| job["jobid"].as_str().unwrap().to_string());

        for (ordinal, expected) in ids.iter().enumerate() {
            assert_eq!(
                server
                    .run_jes_worker_once(&format!("fair-worker-{ordinal}"))
                    .unwrap(),
                Some(format!("jes:{expected}"))
            );
        }
        assert!(
            server
                .run_jes_worker_once("fair-worker-done")
                .unwrap()
                .is_none()
        );
        for (id, owner) in ids.iter().zip(["ALICE", "BOB", "ALICE"]) {
            assert_eq!(server.batch.get(id).unwrap().owner, owner);
            assert_eq!(
                store.get_work(&format!("jes:{id}")).unwrap().unwrap().state,
                WorkState::Completed
            );
        }
    }

    #[test]
    fn expired_crashed_jes_lease_is_reclaimed_and_stale_epoch_is_fenced() {
        let (server, store, clock) = worker_test_server(200);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let job = submit_direct(&server, "ALICE", b"ALICEPASS", "RECOVER");
        let id = job["jobid"].as_str().unwrap();
        let work_id = format!("jes:{id}");
        let crashed = store
            .claim("crashed-worker", Some(JES_WORK_GENERATION), 200, 10)
            .unwrap()
            .unwrap();
        assert_eq!(crashed.lease_epoch, 1);
        clock.advance(10);

        assert_eq!(
            server.run_jes_worker_once("recovery-worker").unwrap(),
            Some(work_id.clone())
        );
        let recovered = store.get_work(&work_id).unwrap().unwrap();
        assert_eq!(
            (recovered.state, recovered.attempt, recovered.lease_epoch),
            (WorkState::Completed, 2, 2)
        );
        assert_eq!(
            server.batch.get(id).unwrap().state,
            mainframe_env_batch::JobState::Completed
        );
        assert_eq!(
            store.complete(
                &work_id,
                crashed.lease_id.as_deref().unwrap(),
                crashed.lease_epoch,
                210,
            ),
            Err(StoreError::LeaseConflict)
        );
    }

    #[test]
    fn reclaimed_terminal_parent_admits_internal_reader_child_once() {
        let (server, store, clock) = worker_test_server(250);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap();
        let parent_work = store
            .claim("crashed-worker", Some(JES_WORK_GENERATION), 250, 10)
            .unwrap()
            .unwrap();
        assert_eq!(
            server.process_claimed_jes_work(&parent_work),
            Ok(JesWorkOutcome::Completed)
        );
        let (child, _) = only_internal_reader_child(&server, parent_id);
        let child_work_id = format!("jes:{}", child.id);
        let admitted = store.get_work(&child_work_id).unwrap().unwrap();
        assert_eq!((admitted.state, admitted.attempt), (WorkState::Queued, 0));

        clock.advance(10);
        assert_eq!(
            server.run_jes_worker_once("recovery-worker").unwrap(),
            Some(format!("jes:{parent_id}"))
        );
        let after_reclaim = store.get_work(&child_work_id).unwrap().unwrap();
        assert_eq!(
            (after_reclaim.state, after_reclaim.attempt),
            (WorkState::Queued, 0)
        );
        assert_eq!(
            server.run_jes_worker_once("child-worker").unwrap(),
            Some(child_work_id.clone())
        );
        assert_eq!(store.get_work(&child_work_id).unwrap().unwrap().attempt, 1);
        assert_eq!(
            server.batch.get(&child.id).unwrap().state,
            mainframe_env_batch::JobState::Completed
        );
    }

    #[test]
    fn internal_reader_child_work_uses_the_child_plan_capabilities() {
        let (server, store, _) = worker_test_server(275);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IKJEFT01\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n//WORK DD DSN=&&WORK,DISP=(NEW,DELETE,DELETE)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap();
        assert_eq!(
            server.run_jes_worker_once("parent-worker").unwrap(),
            Some(format!("jes:{parent_id}"))
        );
        let (child, child_plan) = only_internal_reader_child(&server, parent_id);
        let capabilities = job_capabilities(server.store.as_ref(), &child_plan).unwrap();
        let payload = JesWorkPayload::decode(
            &store
                .get_work(&format!("jes:{}", child.id))
                .unwrap()
                .unwrap()
                .payload,
        )
        .unwrap();
        assert_eq!(
            payload.capabilities,
            capabilities
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        );
        assert!(payload.capabilities.contains("host.db2.read"));
        assert!(payload.capabilities.contains("host.db2.write"));
        assert!(!payload.capabilities.contains("host.dataset.read"));
        assert!(!payload.capabilities.contains("host.dataset.write"));
    }

    #[test]
    fn denied_internal_reader_creates_neither_child_nor_work() {
        let (server, store, _) = worker_test_server(290);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        server.bootstrap_identity("OTHER", b"OTHERPASS1").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap();
        server
            .racf
            .define_profile("JESJOBS", &format!("JOB.{parent_id}.INTRDR"), "OTHER", None)
            .unwrap();
        assert_eq!(
            server.run_jes_worker_once("denied-worker").unwrap(),
            Some(format!("jes:{parent_id}"))
        );
        assert_eq!(
            server.batch.get(parent_id).unwrap().state,
            mainframe_env_batch::JobState::Failed
        );
        assert!(
            server
                .batch
                .internal_reader_children(parent_id)
                .unwrap()
                .is_empty()
        );
        assert!(store.get_work("jes:JOB00002").unwrap().is_none());
    }

    #[test]
    fn admit_internal_reader_children_retries_every_child_after_one_is_blocked() {
        // A worker-run parent writes two children to INTRDR from two separate
        // steps (idempotency is keyed by job+step, so one step can only ever
        // admit one child). "JOB00002" is the first child's predictable ID in
        // a fresh test server (see `denied_internal_reader_creates_neither_child_nor_work`
        // above). Pre-seed a work record at that exact work ID with a
        // mismatched `required_generation`, so the first child's own
        // admission collides on `AlreadyExists` against a record that does
        // not match its frozen identity: an infrastructure-classified,
        // retryable failure that must not stop the second child from being
        // attempted and admitted.
        let (server, store, _) = worker_test_server(600);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        store
            .enqueue(WorkRecord {
                work_id: "jes:JOB00002".into(),
                execution_id: ExecutionId::new("blocker-execution", InvocationLimits::default())
                    .unwrap(),
                required_selector: Selector::new("zosmf:job-submit", InvocationLimits::default())
                    .unwrap(),
                required_generation: "blocked-generation".into(),
                artifact: ArtifactRef::new("artifact:none", InvocationLimits::default()).unwrap(),
                state: WorkState::Queued,
                priority: 5,
                attempt: 0,
                max_attempts: 3,
                available_tick: 0,
                deadline_tick: 1_000_000,
                cancellation_requested: false,
                worker_id: None,
                lease_id: None,
                lease_epoch: 0,
                lease_expiry_tick: None,
                heartbeat_tick: None,
                terminal_tick: None,
                checkpoint_id: None,
                effect_sequence: 0,
                payload: JesWorkPayload::new("JOB00002", "ALICE", ["host.security.authorize"])
                    .unwrap()
                    .encode()
                    .unwrap(),
            })
            .unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//STEPA EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILDA JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n//STEPB EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILDB JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap().to_string();
        let parent_work = store
            .claim("worker", Some(JES_WORK_GENERATION), 600, 10)
            .unwrap()
            .unwrap();
        assert_eq!(
            server.process_claimed_jes_work(&parent_work),
            Ok(JesWorkOutcome::Deferred)
        );
        let children = server.batch.internal_reader_children(&parent_id).unwrap();
        assert_eq!(children.len(), 2);
        let blocked = children
            .iter()
            .find(|(job, _)| job.id == "JOB00002")
            .expect("the first internal-reader child in a fresh test server is JOB00002");
        let free = children
            .iter()
            .find(|(job, _)| job.id != "JOB00002")
            .unwrap();

        // The blocked child's own admission failed on a mismatched
        // `AlreadyExists`: the placeholder record is untouched, and the
        // child job itself was neither cancelled nor given a real work
        // record.
        let still_blocked = store.get_work("jes:JOB00002").unwrap().unwrap();
        assert_eq!(still_blocked.required_generation, "blocked-generation");
        assert_eq!(
            server.batch.get(&blocked.0.id).unwrap().state,
            mainframe_env_batch::JobState::Queued
        );

        // The second child still gets its own work record even though the
        // first child's admission failed first.
        let free_work = store
            .get_work(&format!("jes:{}", free.0.id))
            .unwrap()
            .expect("the second child must not be starved by the first child's failure");
        assert_eq!(free_work.state, WorkState::Queued);
        assert_eq!(free_work.attempt, 0);
        assert_eq!(free_work.required_generation, JES_WORK_GENERATION);

        // Calling admission again directly is idempotent: the blocked child
        // is retried (still fails against the placeholder) and the already
        // admitted sibling is a no-op.
        assert!(matches!(
            server.admit_internal_reader_children(&parent_id, true),
            Ok(ChildAdmissionResult::Retry)
        ));
    }

    #[test]
    fn exhausted_internal_reader_child_admission_cancels_the_unadmitted_child() {
        // A child whose admission fails for a persistently transient reason
        // (here, the durable store is already at capacity) must not dead-
        // letter the parent's own work on the first attempt: the parent's
        // work is released for a bounded retry instead. On the parent's last
        // attempt, the still-unadmitted child is cancelled and verified so
        // neither lifecycle is stranded without recoverable work.
        let limits = mainframe_env_store::StoreLimits {
            max_work_items: 1,
            ..Default::default()
        };
        let store = Arc::new(MemoryStore::new(limits));
        let clock = Arc::new(ManualJesClock::new(700));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let server = ProductServer::open_with_clock(config(), platform, clock.clone()).unwrap();
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap().to_string();
        let parent_work_id = format!("jes:{parent_id}");

        for attempt in 1..=3u32 {
            clock.advance(JES_IDLE_MILLIS + 10);
            assert_eq!(
                server.run_jes_worker_once("retry-worker").unwrap(),
                Some(parent_work_id.clone()),
                "attempt {attempt} should still claim the parent's own work"
            );
            let work = store.get_work(&parent_work_id).unwrap().unwrap();
            assert_eq!(work.attempt, attempt);
            if attempt < 3 {
                assert_eq!(work.state, WorkState::Queued);
            } else {
                assert_eq!(work.state, WorkState::Completed);
            }
        }
        // No further claim is possible: the parent's work is terminal.
        assert_eq!(
            store
                .claim("retry-worker", Some(JES_WORK_GENERATION), 10_000, 10)
                .unwrap(),
            None
        );
        let (child, _) = only_internal_reader_child(&server, &parent_id);
        assert_eq!(child.state, mainframe_env_batch::JobState::Cancelled);
        assert!(
            store
                .get_work(&format!("jes:{}", child.id))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reclaimed_parent_reuses_frozen_child_capabilities_despite_registry_change() {
        let (server, store, clock) = worker_test_server(400);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=MYPROG\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap().to_string();
        let parent_work = store
            .claim("crashed-worker", Some(JES_WORK_GENERATION), 400, 10)
            .unwrap()
            .unwrap();
        assert_eq!(
            server.process_claimed_jes_work(&parent_work),
            Ok(JesWorkOutcome::Completed)
        );
        let (child, _) = only_internal_reader_child(&server, &parent_id);
        let child_work_id = format!("jes:{}", child.id);
        let admitted = store.get_work(&child_work_id).unwrap().unwrap();
        let admitted_payload = JesWorkPayload::decode(&admitted.payload).unwrap();
        assert!(!admitted_payload.capabilities.contains("host.db2.read"));

        // The mutable `batch-program` registry gains a binding for "MYPROG"
        // only after the child was already admitted with the narrower,
        // pre-registration capability set.
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "MYPROG".into(),
                    version: 1,
                    payload: b"1".to_vec(),
                },
                None,
            )
            .unwrap();

        clock.advance(10);
        assert_eq!(
            server.run_jes_worker_once("recovery-worker").unwrap(),
            Some(format!("jes:{parent_id}"))
        );

        let after_reclaim = store.get_work(&child_work_id).unwrap().unwrap();
        assert_eq!(
            (after_reclaim.state, after_reclaim.attempt),
            (WorkState::Queued, 0)
        );
        let after_payload = JesWorkPayload::decode(&after_reclaim.payload).unwrap();
        assert_eq!(
            after_payload.capabilities, admitted_payload.capabilities,
            "reclaim must validate the existing record, not overwrite it with a fresh recomputation"
        );
        assert_eq!(
            server.batch.get(&child.id).unwrap().state,
            mainframe_env_batch::JobState::Queued
        );
    }

    #[test]
    fn cancel_internal_reader_child_verifies_the_job_reaches_cancelled() {
        let (server, _store, _clock) = worker_test_server(50);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap();
        assert_eq!(
            server.run_jes_worker_once("worker").unwrap(),
            Some(format!("jes:{parent_id}"))
        );
        let (child, _) = only_internal_reader_child(&server, parent_id);
        assert!(server.cancel_internal_reader_child(&child));
        assert_eq!(
            server.batch.get(&child.id).unwrap().state,
            mainframe_env_batch::JobState::Cancelled
        );
    }

    #[test]
    fn sqlite_restart_reclaims_crashed_jes_work_with_a_new_epoch() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-jes-worker-restart-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = directory.join("artifacts");

        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let first = ProductServer::open_with_clock(
            server_config.clone(),
            first_platform,
            Arc::new(ManualJesClock::new(500)),
        )
        .unwrap();
        first.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let job = submit_jcl_direct(
            &first,
            "ALICE",
            b"ALICEPASS",
            "//RESTART JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                .into(),
        );
        let id = job["jobid"].as_str().unwrap().to_string();
        let work_id = format!("jes:{id}");
        let stale = first_store
            .claim("crashed-process", Some(JES_WORK_GENERATION), 500, 10)
            .unwrap()
            .unwrap();
        assert_eq!(
            first.process_claimed_jes_work(&stale),
            Ok(JesWorkOutcome::Completed)
        );
        let (child, _) = only_internal_reader_child(&first, &id);
        let child_work_id = format!("jes:{}", child.id);
        assert_eq!(
            first_store.get_work(&child_work_id).unwrap().unwrap().state,
            WorkState::Queued
        );
        drop((first, first_store));

        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let second = ProductServer::open_with_clock(
            server_config,
            second_platform,
            Arc::new(ManualJesClock::new(510)),
        )
        .unwrap();
        assert_eq!(
            second.run_jes_worker_once("restarted-process").unwrap(),
            Some(work_id.clone())
        );
        let work = second_store.get_work(&work_id).unwrap().unwrap();
        assert_eq!((work.state, work.lease_epoch), (WorkState::Completed, 2));
        assert_eq!(
            second.batch.get(&id).unwrap().state,
            mainframe_env_batch::JobState::Completed
        );
        assert_eq!(
            second_store
                .get_work(&child_work_id)
                .unwrap()
                .unwrap()
                .attempt,
            0
        );
        assert_eq!(
            second.run_jes_worker_once("restarted-child").unwrap(),
            Some(child_work_id.clone())
        );
        assert_eq!(
            second.batch.get(&child.id).unwrap().state,
            mainframe_env_batch::JobState::Completed
        );
        assert_eq!(
            second_store.complete(
                &work_id,
                stale.lease_id.as_deref().unwrap(),
                stale.lease_epoch,
                510,
            ),
            Err(StoreError::LeaseConflict)
        );
        drop((second, second_store));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancellation_of_claimed_jes_work_wins_before_worker_completion() {
        let (server, store, _) = worker_test_server(300);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let job = submit_direct(&server, "ALICE", b"ALICEPASS", "CANCEL");
        let id = job["jobid"].as_str().unwrap();
        let work_id = format!("jes:{id}");
        let claimed = store
            .claim("cancel-worker", Some(JES_WORK_GENERATION), 300, 20)
            .unwrap()
            .unwrap();
        let payload = JesWorkPayload::decode(&claimed.payload).unwrap();
        let invocation = server
            .jes_work_invocation(&claimed, &payload, false)
            .unwrap();
        assert_eq!(
            invocation.bindings["jes.work-id"].bytes(),
            work_id.as_bytes()
        );
        let response = server
            .handle(
                Authentication::Basic {
                    user: "ALICE".into(),
                    secret: b"ALICEPASS".to_vec(),
                },
                GatewayRequest::JobCancel {
                    jobname: "CANCEL".into(),
                    jobid: id.into(),
                },
            )
            .unwrap();
        assert_eq!(response.status, StatusCode::NO_CONTENT);
        let outcome = server.process_claimed_jes_work(&claimed);
        assert_eq!(outcome, Ok(JesWorkOutcome::Cancelled));
        server.finish_claimed_jes_work(&claimed, outcome).unwrap();
        assert_eq!(
            store.get_work(&work_id).unwrap().unwrap().state,
            WorkState::Cancelled
        );
        assert_eq!(
            server.batch.get(id).unwrap().state,
            mainframe_env_batch::JobState::Cancelled
        );
    }

    #[test]
    fn jes_worker_rejects_cross_user_payload_substitution() {
        let (server, store, _) = worker_test_server(350);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        server.bootstrap_user("BOB", b"BOBPASSWORD").unwrap();
        let job = submit_direct(&server, "ALICE", b"ALICEPASS", "ISOLATE");
        let id = job["jobid"].as_str().unwrap();
        let claimed = store
            .claim("isolation-worker", Some(JES_WORK_GENERATION), 350, 20)
            .unwrap()
            .unwrap();
        let mut forged = claimed.clone();
        let mut payload = JesWorkPayload::decode(&forged.payload).unwrap();
        payload.owner = "BOB".into();
        forged.payload = payload.encode().unwrap();
        assert_eq!(
            server.process_claimed_jes_work(&forged),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            server.batch.get(id).unwrap().state,
            mainframe_env_batch::JobState::Queued
        );
    }

    #[tokio::test]
    async fn bounded_worker_pool_starts_once_and_joins_on_shutdown() {
        let (server, _, _) = worker_test_server(400);
        server
            .bootstrap_administrator("ADMIN", b"TESTPASS")
            .unwrap();
        server.start_background_workers().unwrap();
        server.start_background_workers().unwrap();
        assert_eq!(
            server.jes_worker_handles.lock().unwrap().len(),
            JES_WORKER_COUNT
        );
        assert_eq!(server.metrics().jes_workers, JES_WORKER_COUNT);
        wait_for_worker_health(&server, JES_WORKER_COUNT).await;
        assert!(server.ready());
        assert!(server.graceful_shutdown().await);
        assert!(server.jes_worker_handles.lock().unwrap().is_empty());
        assert_eq!(server.jes_worker_active.load(Ordering::SeqCst), 0);
        assert_eq!(server.metrics().jes_workers, 0);
        assert!(!server.ready());
    }

    #[tokio::test]
    async fn background_workers_process_multiple_users_without_request_coupling() {
        let (server, store, _) = worker_test_server(700);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        server.bootstrap_user("BOB", b"BOBPASSWORD").unwrap();
        server.start_background_workers().unwrap();
        let alice = submit_direct(&server, "ALICE", b"ALICEPASS", "ALICEBG");
        let bob = submit_direct(&server, "BOB", b"BOBPASSWORD", "BOBBG");
        let alice_id = alice["jobid"].as_str().unwrap();
        let bob_id = bob["jobid"].as_str().unwrap();
        let (alice_job, bob_job) = tokio::join!(
            wait_for_terminal_job(&server, alice_id),
            wait_for_terminal_job(&server, bob_id)
        );
        assert_eq!(
            (alice_job.owner.as_str(), bob_job.owner.as_str()),
            ("ALICE", "BOB")
        );
        for id in [alice_id, bob_id] {
            assert_eq!(
                store.get_work(&format!("jes:{id}")).unwrap().unwrap().state,
                WorkState::Completed
            );
        }
        assert!(server.graceful_shutdown().await);
    }

    #[tokio::test]
    async fn background_workers_complete_internal_reader_children() {
        let (server, store, _) = worker_test_server(750);
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        server.start_background_workers().unwrap();
        let parent = submit_jcl_direct(
            &server,
            "ALICE",
            b"ALICEPASS",
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n//WORK DD DSN=&&WORK,DISP=(NEW,DELETE,DELETE)\n"
                .into(),
        );
        let parent_id = parent["jobid"].as_str().unwrap();
        let mut child = None;
        for _ in 0..2_000 {
            let children = server.batch.internal_reader_children(parent_id).unwrap();
            if let Some(found) = children.into_iter().next() {
                child = Some(found);
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let (child, child_plan) = child.expect("internal-reader child did not appear");
        let completed = wait_for_terminal_job(&server, &child.id).await;
        assert_eq!(completed.state, mainframe_env_batch::JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
        let payload = JesWorkPayload::decode(
            &store
                .get_work(&format!("jes:{}", child.id))
                .unwrap()
                .unwrap()
                .payload,
        )
        .unwrap();
        assert_eq!(
            payload.capabilities,
            job_capabilities(server.store.as_ref(), &child_plan)
                .unwrap()
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        );
        assert!(!payload.capabilities.contains("host.dataset.read"));
        assert!(!payload.capabilities.contains("host.dataset.write"));
        assert!(server.graceful_shutdown().await);
    }

    #[tokio::test]
    async fn worker_readiness_tracks_queue_progress_and_persistent_failures() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(ToggleJesClock::new(900));
        let platform: Arc<dyn PlatformStore> = store;
        let server = ProductServer::open_with_clock(config(), platform, clock.clone()).unwrap();
        server
            .bootstrap_administrator("ADMIN", b"TESTPASS")
            .unwrap();
        server.start_background_workers().unwrap();
        wait_for_worker_health(&server, JES_WORKER_COUNT).await;
        let before = server.metrics();
        assert!(before.jes_worker_progress >= JES_WORKER_COUNT as u64);
        assert!(server.readiness().jes_workers);

        clock.failing.store(true, Ordering::SeqCst);
        server.jes_worker_notify.notify_waiters();
        for _ in 0..4_000 {
            let metrics = server.metrics();
            if metrics.jes_worker_failures >= JES_WORKER_COUNT as u64
                && metrics.jes_worker_healthy == 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let failed = server.metrics();
        assert!(failed.jes_worker_failures >= JES_WORKER_COUNT as u64);
        assert_eq!(failed.jes_worker_healthy, 0);
        assert!(
            server
                .jes_worker_handles
                .lock()
                .unwrap()
                .iter()
                .all(|handle| !handle.is_finished())
        );
        assert!(!server.readiness().jes_workers);
        assert!(server.graceful_shutdown().await);
    }

    #[tokio::test]
    async fn worker_readiness_expires_when_progress_stalls_without_a_reported_failure() {
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("ADMIN", b"TESTPASS")
            .unwrap();
        server.start_background_workers().unwrap();
        wait_for_worker_health(&server, JES_WORKER_COUNT).await;
        assert!(server.jes_workers_ready_at(Instant::now()));
        let failures = server.metrics().jes_worker_failures;

        let stalled_observation =
            Instant::now() + Duration::from_millis(JES_WORKER_FRESHNESS_MILLIS.saturating_mul(2));
        assert!(!server.jes_workers_ready_at(stalled_observation));
        assert_eq!(server.metrics().jes_worker_failures, failures);
        assert!(
            server
                .jes_worker_handles
                .lock()
                .unwrap()
                .iter()
                .all(|handle| !handle.is_finished())
        );
        assert!(server.graceful_shutdown().await);
    }

    #[test]
    fn artifact_readiness_rejects_write_denial_and_reported_saturation() {
        let denied = server_with_artifact_health(Err(StoreError::Infrastructure(
            "injected write denial".into(),
        )));
        assert!(!denied.readiness().artifact_store);

        for health in [
            ArtifactStoreHealth {
                readable: true,
                writable: true,
                used_objects: Some(8),
                max_objects: Some(8),
                used_bytes: None,
                max_bytes: None,
            },
            ArtifactStoreHealth {
                readable: true,
                writable: true,
                used_objects: None,
                max_objects: None,
                used_bytes: Some(4096),
                max_bytes: Some(4096),
            },
        ] {
            assert!(
                !server_with_artifact_health(Ok(health))
                    .readiness()
                    .artifact_store
            );
        }
    }

    #[test]
    fn committed_bootstrap_restart_does_not_resolve_removed_secret() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let reference = SecretRef::new("test:one-time-admin", HostLimits::default()).unwrap();
        let external = Arc::new(MemorySecretResolver::default());
        external.insert(reference.as_str(), b"TESTPASS".to_vec());
        let first_platform: Arc<dyn PlatformStore> = store.clone();
        let first = ProductServer::open(
            config(),
            first_platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        first
            .bootstrap_administrator_from_reference("ADMIN", &reference, external.as_ref())
            .unwrap();
        drop(first);
        external.remove(reference.as_str());

        let second_platform: Arc<dyn PlatformStore> = store;
        let second = ProductServer::open(
            config(),
            second_platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        second
            .bootstrap_administrator_from_reference("ADMIN", &reference, external.as_ref())
            .unwrap();
        assert!(second.verify("ADMIN", b"TESTPASS").is_ok());
    }

    #[test]
    fn pending_bootstrap_claim_survives_missing_secret_and_rejects_replacement() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let reference = SecretRef::new("test:pending-admin", HostLimits::default()).unwrap();
        let external = Arc::new(MemorySecretResolver::default());
        let first_platform: Arc<dyn PlatformStore> = store.clone();
        let first = ProductServer::open(
            config(),
            first_platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert_eq!(
            first.bootstrap_administrator_from_reference("ADMIN", &reference, external.as_ref()),
            Err(HostProblem::NotFound)
        );
        assert_eq!(
            first
                .bootstrap_record(BOOTSTRAP_CLAIM_KEY)
                .unwrap()
                .unwrap()
                .payload,
            b"ADMIN"
        );
        drop(first);

        external.insert(reference.as_str(), b"TESTPASS".to_vec());
        let second_platform: Arc<dyn PlatformStore> = store;
        let second = ProductServer::open(
            config(),
            second_platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert_eq!(
            second.bootstrap_administrator_from_reference("OTHER", &reference, external.as_ref()),
            Err(HostProblem::Unauthorized)
        );
        second
            .bootstrap_administrator_from_reference("ADMIN", &reference, external.as_ref())
            .unwrap();
        assert!(second.verify("ADMIN", b"TESTPASS").is_ok());
        assert_eq!(second.racf.active_principal_epochs().unwrap().len(), 1);
    }

    #[test]
    fn bootstrap_rejects_ordinary_existing_user_and_mismatched_marker_before_mutation() {
        let ordinary = ProductServer::memory(config()).unwrap();
        ordinary.bootstrap_identity("ADMIN", b"TESTPASS").unwrap();
        assert_eq!(
            ordinary.bootstrap_administrator("ADMIN", b"TESTPASS"),
            Err(HostProblem::Unauthorized)
        );
        assert!(
            ordinary
                .bootstrap_record(BOOTSTRAP_CLAIM_KEY)
                .unwrap()
                .is_none()
        );
        let principal = PrincipalId::new("ADMIN", InvocationLimits::default()).unwrap();
        assert!(
            !ordinary
                .racf
                .bootstrap_administrator_ready(&principal)
                .unwrap()
        );

        let mismatch = ProductServer::memory(config()).unwrap();
        mismatch
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: BOOTSTRAP_NAMESPACE.into(),
                    key: BOOTSTRAP_KEY.into(),
                    version: 1,
                    payload: b"OTHER".to_vec(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            mismatch.bootstrap_administrator("ADMIN", b"TESTPASS"),
            Err(HostProblem::Unauthorized)
        );
        assert!(mismatch.racf.active_principal_epochs().unwrap().is_empty());
        assert!(
            mismatch
                .bootstrap_record(BOOTSTRAP_CLAIM_KEY)
                .unwrap()
                .is_none()
        );

        let claim_mismatch = ProductServer::memory(config()).unwrap();
        claim_mismatch
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: BOOTSTRAP_NAMESPACE.into(),
                    key: BOOTSTRAP_CLAIM_KEY.into(),
                    version: 1,
                    payload: b"OTHER".to_vec(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            claim_mismatch.bootstrap_administrator("ADMIN", b"TESTPASS"),
            Err(HostProblem::Unauthorized)
        );
        assert!(
            claim_mismatch
                .racf
                .active_principal_epochs()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn bootstrap_recovers_proven_partial_administrator_without_reading_secret() {
        let server = ProductServer::memory(config()).unwrap();
        let reference = SecretRef::new("test:partial-admin", HostLimits::default()).unwrap();
        let scope = server
            .secrets
            .scoped(&reference, b"TESTPASS".to_vec())
            .unwrap();
        server
            .racf
            .bootstrap_administrator("ADMIN", &reference)
            .unwrap();
        drop(scope);
        assert!(server.bootstrap_record(BOOTSTRAP_KEY).unwrap().is_none());
        server.bootstrap_administrator("ADMIN", b"").unwrap();
        assert!(server.bootstrap_principal_ready());
        assert!(server.verify("ADMIN", b"TESTPASS").is_ok());
    }

    #[test]
    fn concurrent_bootstrap_claim_allows_exactly_one_principal() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let open = |store: Arc<MemoryStore>| {
            let platform: Arc<dyn PlatformStore> = store;
            ProductServer::open(
                config(),
                platform,
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap()
        };
        let left = open(store.clone());
        let right = open(store);
        let barrier = Arc::new(Barrier::new(3));
        let spawn = |server: Arc<ProductServer>, user: &'static str, barrier: Arc<Barrier>| {
            std::thread::spawn(move || {
                barrier.wait();
                (user, server.bootstrap_administrator(user, b"TESTPASS"))
            })
        };
        let left_worker = spawn(left.clone(), "ADMINA", barrier.clone());
        let right_worker = spawn(right.clone(), "ADMINB", barrier.clone());
        barrier.wait();
        let outcomes = [left_worker.join().unwrap(), right_worker.join().unwrap()];
        assert_eq!(
            outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(_, result)| *result == Err(HostProblem::Unauthorized))
                .count(),
            1
        );
        let winner = outcomes
            .iter()
            .find_map(|(user, result)| result.is_ok().then_some(*user))
            .unwrap();
        assert_eq!(
            left.bootstrap_record(BOOTSTRAP_KEY)
                .unwrap()
                .unwrap()
                .payload,
            winner.as_bytes()
        );
        assert_eq!(left.racf.active_principal_epochs().unwrap().len(), 1);
    }

    #[test]
    fn info_reports_the_validated_configured_listener() {
        let mut server_config = config();
        server_config.listen = "127.0.0.1:20443".into();
        let server = ProductServer::memory(server_config).unwrap();
        let before_epoch = server.store.provider_state_retention_epoch().unwrap();
        let before_capacity = server
            .store
            .retention_capacity_health(server.config.retention.policy().unwrap())
            .unwrap();
        let response = server
            .handle(Authentication::Anonymous, GatewayRequest::Info)
            .unwrap();
        let mainframe_env_zosmf::GatewayBody::Json(info) = response.body else {
            panic!("information response was not JSON")
        };
        assert_eq!(info["listen"], "127.0.0.1:20443");
        assert_eq!(info["zosmf_port"], "20443");
        assert_eq!(info["readiness"]["retention_capacity"], "healthy");
        assert_eq!(info["readiness"]["retention_warning"], false);
        let second = server
            .handle(Authentication::Anonymous, GatewayRequest::Info)
            .unwrap();
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(
            server.store.provider_state_retention_epoch().unwrap(),
            before_epoch
        );
        assert_eq!(
            server
                .store
                .retention_capacity_health(server.config.retention.policy().unwrap())
                .unwrap(),
            before_capacity
        );
    }

    #[test]
    fn readiness_warns_at_low_capacity_and_rejects_high_and_full_capacity() {
        let store = Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
            max_provider_state: 100,
            ..Default::default()
        }));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let server = ProductServer::open(
            config(),
            platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        let policy = server.config.retention.policy().unwrap();
        let provider_usage = |store: &MemoryStore| {
            store
                .retention_capacity_health(policy)
                .unwrap()
                .targets
                .into_iter()
                .find(|entry| entry.target == RetentionTarget::Db2Replay)
                .unwrap()
                .used
        };
        let fill_to = |target: usize| {
            let used = provider_usage(&store);
            assert!(used <= target);
            for ordinal in used..target {
                store
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: "readiness-capacity-test".into(),
                            key: format!("row-{ordinal:03}"),
                            version: 1,
                            payload: Vec::new(),
                        },
                        None,
                    )
                    .unwrap();
            }
        };

        assert_eq!(
            server.readiness().retention_capacity,
            ProductCapacityStatus::Healthy
        );
        fill_to(70);
        let low = server.readiness();
        assert!(low.writable_store);
        assert_eq!(low.retention_capacity, ProductCapacityStatus::LowWatermark);
        assert!(low.retention_warning);
        assert!(low.retention_capacity.accepts_traffic());
        assert!(!low.ready());

        fill_to(85);
        let high = server.readiness();
        assert!(high.writable_store);
        assert_eq!(
            high.retention_capacity,
            ProductCapacityStatus::HighWatermark
        );
        assert!(high.retention_warning);
        assert!(!high.retention_capacity.accepts_traffic());
        assert!(!high.ready());

        fill_to(100);
        let full = server.readiness();
        assert!(full.writable_store);
        assert_eq!(full.retention_capacity, ProductCapacityStatus::Full);
        assert!(full.retention_warning);
        assert!(!full.retention_capacity.accepts_traffic());
        assert!(!full.ready());
    }

    #[tokio::test]
    async fn first_administrator_gates_readiness_and_bootstrap_is_fail_closed() {
        let server = ProductServer::memory(config()).unwrap();
        assert!(server.live());
        assert!(!server.ready());
        let initial = server.readiness();
        assert!(initial.accepting && initial.writable_store && initial.artifact_store);
        assert_eq!(initial.retention_capacity, ProductCapacityStatus::Healthy);
        assert!(!initial.retention_warning);
        assert!(!initial.bootstrap_identity);
        assert!(!initial.jes_workers);

        server
            .bootstrap_administrator("ADMIN", b"TESTPASS")
            .unwrap();
        assert!(!server.ready());
        server.start_background_workers().unwrap();
        wait_for_worker_health(&server, JES_WORKER_COUNT).await;
        let ready = server.readiness();
        assert!(ready.ready() && ready.host_capabilities && ready.jes_workers);
        assert!(server.verify("ADMIN", b"TESTPASS").is_ok());

        // Declarative replay never replaces the established credential.
        server
            .bootstrap_administrator("ADMIN", b"DIFFERENT1")
            .unwrap();
        assert!(server.verify("ADMIN", b"TESTPASS").is_ok());
        assert_eq!(
            server.bootstrap_administrator("OTHER", b"OTHERPASS1"),
            Err(HostProblem::Unauthorized)
        );
        assert!(server.graceful_shutdown().await);
    }

    const TEST_PACKAGE_KEY: &[u8] = b"test-production-package-trust-key";

    fn test_package_trust() -> HmacSha256PackageTrust {
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:package-test-key", TEST_PACKAGE_KEY.to_vec());
        HmacSha256PackageTrust::new(
            BTreeMap::from([(
                "test-production-key".into(),
                SecretRef::new("secret:package-test-key", HostLimits::default()).unwrap(),
            )]),
            resolver,
        )
        .unwrap()
    }

    fn sign_test_package_identity(identity: &str) -> String {
        sign_package_identity_with_key(TEST_PACKAGE_KEY, identity)
    }

    fn sign_package_identity_with_key(key: &[u8], identity: &str) -> String {
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, key),
            identity.as_bytes(),
        ))
    }

    #[test]
    fn package_trust_resolves_verification_only_secrets_with_rotation_and_revocation() {
        let resolver = Arc::new(MemorySecretResolver::default());
        let reference =
            SecretRef::new("secret:rotating-package-key", HostLimits::default()).unwrap();
        resolver.insert(reference.as_str(), TEST_PACKAGE_KEY.to_vec());
        let trust = HmacSha256PackageTrust::new(
            BTreeMap::from([("key-1".into(), reference.clone())]),
            resolver.clone(),
        )
        .unwrap();
        let identity = format!("sha256:{:064x}", 71);
        let first = sign_package_identity_with_key(TEST_PACKAGE_KEY, &identity);
        assert!(trust.verify("key-1", "hmac-sha256@1", &identity, &first));
        assert!(!trust.verify("missing", "hmac-sha256@1", &identity, &first));

        resolver.remove(reference.as_str());
        assert!(!trust.verify("key-1", "hmac-sha256@1", &identity, &first));
        let rotated = b"rotated-production-package-key-0002";
        resolver.insert(reference.as_str(), rotated.to_vec());
        assert!(!trust.verify("key-1", "hmac-sha256@1", &identity, &first));
        let second = sign_package_identity_with_key(rotated, &identity);
        assert!(trust.verify("key-1", "hmac-sha256@1", &identity, &second));

        let environment = BTreeMap::from([(
            "MAINFRAME_ENV_PACKAGE_HMAC_KEYS".into(),
            "plaintext-must-not-be-a-production-input".into(),
        )]);
        let empty = HmacSha256PackageTrust::from_environment(&environment, resolver).unwrap();
        assert!(!empty.verify("key-1", "hmac-sha256@1", &identity, &second));
        assert!(!format!("{reference:?}").contains("rotated-production-package-key"));

        let unsupported = BTreeMap::from([(
            "MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS".into(),
            r#"{"key-1":"secret://unsupported/package-key"}"#.into(),
        )]);
        assert!(matches!(
            HmacSha256PackageTrust::from_environment(
                &unsupported,
                Arc::new(MemorySecretResolver::default())
            ),
            Err(HostProblem::Malformed)
        ));
    }

    fn signed_controller_package(
        _trust: &HmacSha256PackageTrust,
    ) -> mainframe_env_application::ApplicationPackageV2 {
        use mainframe_env_application::{
            APPLICATION_PACKAGE_V2_CONTRACT, ApplicationManifest, ApplicationPackage,
            ApplicationSections, BatchController, EntryKind, PackageEntry, PackageSignature,
        };
        let definitions = [
            (EntryKind::Source, "source/manifest"),
            (EntryKind::Resource, "resource/manifest"),
            (EntryKind::Program, "program/REALPGM"),
            (EntryKind::Data, "data/manifest"),
            (EntryKind::Profile, "profile/manifest"),
            (EntryKind::Migration, "migration/manifest"),
        ];
        let mut entries = Vec::new();
        let mut blobs = BTreeMap::new();
        for (kind, path) in definitions {
            let bytes = path.as_bytes().to_vec();
            let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
            blobs.insert(sha256.clone(), bytes.clone());
            entries.push(PackageEntry {
                path: path.into(),
                kind,
                sha256,
                bytes: bytes.len(),
                depends_on: (kind != EntryKind::Source)
                    .then(|| "source/manifest".into())
                    .into_iter()
                    .collect(),
            });
        }
        let mut package = ApplicationPackageV2 {
            base: ApplicationPackage {
                manifest: ApplicationManifest {
                    name: "TRUSTED-APPLICATION".into(),
                    version: "0.2.0".into(),
                    target_product: "0.2.0".into(),
                    entries,
                },
                blobs,
            },
            generation: 1,
            sections: ApplicationSections {
                schema_version: APPLICATION_PACKAGE_V2_CONTRACT.into(),
                host_abi_libraries: Vec::new(),
                sql_tables: Vec::new(),
                sql_rows: Vec::new(),
                ims_definitions: Vec::new(),
                ims_rows: Vec::new(),
                mq_resources: Vec::new(),
                batch_controllers: vec![BatchController {
                    name: "TRUSTED-CONTROLLER".into(),
                    program: "program/REALPGM".into(),
                    kind: BatchControllerKind::CobolProgram,
                    properties: BTreeMap::from([
                        ("launcher".into(), "tso-run".into()),
                        ("selector-program".into(), "REALPGM".into()),
                        ("behavior".into(), "program-call".into()),
                    ]),
                }],
                security_resources: Vec::new(),
            },
            signature: PackageSignature {
                algorithm: "hmac-sha256@1".into(),
                key_id: "test-production-key".into(),
                value: "invalid".into(),
            },
        };
        let identity = mainframe_env_application::package_v2_identity(&package).unwrap();
        package.signature.value = sign_test_package_identity(&identity);
        package
    }

    fn resign_package(
        package: &mut mainframe_env_application::ApplicationPackageV2,
        _trust: &HmacSha256PackageTrust,
    ) {
        let identity = mainframe_env_application::package_v2_identity(package).unwrap();
        package.signature.value = sign_test_package_identity(&identity);
    }

    fn signed_db2_package(
        trust: &HmacSha256PackageTrust,
    ) -> (
        mainframe_env_application::ApplicationPackageV2,
        Vec<Db2TableDefinition>,
    ) {
        use mainframe_env_application::{SqlColumn, SqlTable};
        use mainframe_env_db2::{
            Db2ColumnDefinition, Db2ExtractField, Db2ExtractLayout, Db2ForeignKeyDefinition,
            Db2ResultEncoding,
        };
        let definitions = vec![
            Db2TableDefinition {
                name: "SIGNED.PARENT".into(),
                columns: vec![
                    Db2ColumnDefinition {
                        name: "ID".into(),
                        nullable: false,
                        max_bytes: 4,
                        result_encoding: Db2ResultEncoding::Raw,
                        default_value: None,
                    },
                    Db2ColumnDefinition {
                        name: "VALUE".into(),
                        nullable: false,
                        max_bytes: 37,
                        result_encoding: Db2ResultEncoding::Varchar,
                        default_value: Some(b"SIGNED-DEFAULT".to_vec()),
                    },
                ],
                primary_key: vec!["ID".into()],
                foreign_keys: Vec::new(),
                extract: Some(Db2ExtractLayout {
                    fields: vec![Db2ExtractField {
                        column: "VALUE".into(),
                        width: 41,
                    }],
                    trailer: b"SIGNED".to_vec(),
                }),
            },
            Db2TableDefinition {
                name: "SIGNED.CHILD".into(),
                columns: vec![
                    Db2ColumnDefinition {
                        name: "PARENT_ID".into(),
                        nullable: false,
                        max_bytes: 4,
                        result_encoding: Db2ResultEncoding::Raw,
                        default_value: None,
                    },
                    Db2ColumnDefinition {
                        name: "DETAIL".into(),
                        nullable: false,
                        max_bytes: 16,
                        result_encoding: Db2ResultEncoding::Raw,
                        default_value: Some(b"DETAIL".to_vec()),
                    },
                ],
                primary_key: vec!["PARENT_ID".into(), "DETAIL".into()],
                foreign_keys: vec![Db2ForeignKeyDefinition {
                    columns: vec!["PARENT_ID".into()],
                    referenced_table: "SIGNED.PARENT".into(),
                    referenced_columns: vec!["ID".into()],
                    delete_restrict: true,
                }],
                extract: None,
            },
        ];
        let mut package = signed_controller_package(trust);
        package.base.manifest.name = "SIGNED-DB2-APPLICATION".into();
        package.sections.sql_tables = definitions
            .iter()
            .map(|table| SqlTable {
                name: table.name.clone(),
                columns: table
                    .columns
                    .iter()
                    .map(|column| SqlColumn {
                        name: column.name.clone(),
                        nullable: column.nullable,
                    })
                    .collect(),
                primary_key: table.primary_key.clone(),
            })
            .collect();
        let bytes = serde_json::to_vec(&definitions).unwrap();
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        let entry = package
            .base
            .manifest
            .entries
            .iter_mut()
            .find(|entry| entry.kind == EntryKind::Data)
            .unwrap();
        package.base.blobs.remove(&entry.sha256);
        entry.path = "data/db2/catalog".into();
        entry.sha256 = sha256.clone();
        entry.bytes = bytes.len();
        package.base.blobs.insert(sha256, bytes);
        let identity = mainframe_env_application::package_v2_identity(&package).unwrap();
        package.signature.value = sign_test_package_identity(&identity);
        (package, definitions)
    }

    #[test]
    fn subsystem_publication_requires_server_verified_selected_package_handle() {
        let trust = Arc::new(test_package_trust());
        let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
        let missing = ApplicationGenerationRecord {
            package: "TRUSTED-APPLICATION".into(),
            version: "0.2.0".into(),
            generation: 1,
            identity: format!("sha256:{:064x}", 1),
            state: InstallState::Ready,
        };
        assert_eq!(
            server.publish_application_generation(&missing),
            Err(HostProblem::NotFound)
        );

        let mut package = signed_controller_package(&trust);
        package.signature.value = "caller-supplied-digest-is-not-trust".into();
        assert_eq!(
            server.install_application_package_v2(&package),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            server.publish_application_generation(&missing),
            Err(HostProblem::NotFound)
        );

        let package = signed_controller_package(&trust);
        let staged = server.install_application_package_v2(&package).unwrap();
        assert_eq!(staged.state, InstallState::Staged);
        let published = server.publish_application_generation(&staged).unwrap();
        assert_eq!(published.identity, staged.identity);
        assert_eq!(published.controllers, 1);
    }

    #[test]
    fn verified_package_selection_survives_restart_before_publication_retry() {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = ProductServer::open_with_package_trust(
            config(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        let package = signed_controller_package(&trust);
        let installed = server.install_application_package_v2(&package).unwrap();
        drop(server);

        let restarted = ProductServer::open_with_package_trust(
            config(),
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust,
        )
        .unwrap();
        let replayed = restarted
            .publish_application_generation(&installed)
            .unwrap();
        assert_eq!(replayed.identity, installed.identity);
        assert!(!replayed.replayed);
        assert!(
            restarted
                .publish_application_generation(&installed)
                .unwrap()
                .replayed
        );
    }

    #[test]
    fn partial_publication_recovers_without_mixed_generation_and_rollback_is_durable() {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = ProductServer::open_with_package_trust(
            config(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        let (first_package, _) = signed_db2_package(&trust);
        let first = server
            .install_application_package_v2(&first_package)
            .unwrap();
        let retained = server.application_generation_v2(&first).unwrap();
        server
            .apply_application_batch_controllers(&retained)
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
                    key: first.package.to_ascii_uppercase(),
                    version: 1,
                    payload: serde_json::to_vec(&ApplicationPublicationState {
                        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
                        package: first.package.clone(),
                        generation: first.generation,
                        identity: first.identity.clone(),
                        action: PublicationAction::Install,
                        controllers: PublicationSectionState::Applied,
                        db2: PublicationSectionState::Applying,
                        complete: false,
                    })
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        drop(server);

        let restarted = ProductServer::open_with_package_trust(
            config(),
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        assert!(
            restarted
                .publish_application_generation(&first)
                .unwrap()
                .replayed
        );
        assert!(
            restarted
                .db2_service()
                .table_definition("SIGNED.PARENT")
                .is_ok()
        );

        let mut second_package = first_package.clone();
        second_package.generation = 2;
        second_package.sections.batch_controllers[0].name = "SECOND-CONTROLLER".into();
        resign_package(&mut second_package, &trust);
        let second = restarted
            .install_application_package_v2(&second_package)
            .unwrap();
        restarted.publish_application_generation(&second).unwrap();
        assert_eq!(
            restarted.publish_application_generation(&first),
            Err(HostProblem::IdempotencyConflict)
        );
        restarted.rollback_application_generation(&first).unwrap();
        drop(restarted);

        let rolled_back = ProductServer::open_with_package_trust(
            config(),
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust,
        )
        .unwrap();
        assert_eq!(
            rolled_back
                .selected_application_v2(&first)
                .unwrap()
                .record()
                .generation,
            1
        );
    }

    #[test]
    fn signed_hostile_db2_catalog_is_bounded_before_provider_mutation() {
        let trust = Arc::new(test_package_trust());
        let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
        let (mut package, definitions) = signed_db2_package(&trust);
        let hostile = (0..=Db2Limits::default().max_tables)
            .map(|index| {
                let mut table = definitions[0].clone();
                table.name = format!("SIGNED.HOSTILE{index}");
                table
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&hostile).unwrap();
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        let entry = package
            .base
            .manifest
            .entries
            .iter_mut()
            .find(|entry| entry.path == "data/db2/catalog")
            .unwrap();
        package.base.blobs.remove(&entry.sha256);
        entry.sha256 = digest.clone();
        entry.bytes = bytes.len();
        package.base.blobs.insert(digest, bytes);
        resign_package(&mut package, &trust);

        let staged = server.install_application_package_v2(&package).unwrap();
        assert_eq!(
            server.publish_application_generation(&staged),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            server.db2_service().table_definition("SIGNED.HOSTILE0"),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn db2_publication_derives_every_definition_field_from_the_signed_blob() {
        let trust = Arc::new(test_package_trust());
        let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
        let (package, signed) = signed_db2_package(&trust);
        let installed = server.install_application_package_v2(&package).unwrap();

        let mut untrusted_caller_copy = signed.clone();
        untrusted_caller_copy[0].columns[1].max_bytes = 1;
        untrusted_caller_copy[0].columns[1].result_encoding =
            mainframe_env_db2::Db2ResultEncoding::Raw;
        untrusted_caller_copy[0].columns[1].default_value = Some(b"TAMPERED".to_vec());
        untrusted_caller_copy[1].foreign_keys[0].referenced_columns = vec!["VALUE".into()];
        untrusted_caller_copy[1].foreign_keys[0].delete_restrict = false;
        untrusted_caller_copy[0].extract = None;

        server.publish_application_generation(&installed).unwrap();
        let installed_parent = server
            .db2_service()
            .table_definition("SIGNED.PARENT")
            .unwrap();
        let installed_child = server
            .db2_service()
            .table_definition("SIGNED.CHILD")
            .unwrap();
        assert_eq!(installed_parent, signed[0]);
        assert_eq!(installed_child, signed[1]);
        assert_ne!(installed_parent, untrusted_caller_copy[0]);
        assert_ne!(installed_child, untrusted_caller_copy[1]);
        assert_eq!(installed_parent.columns[1].max_bytes, 37);
        assert_eq!(
            installed_parent.columns[1].result_encoding,
            mainframe_env_db2::Db2ResultEncoding::Varchar
        );
        assert_eq!(
            installed_parent.columns[1].default_value.as_deref(),
            Some(b"SIGNED-DEFAULT".as_slice())
        );
        assert!(installed_child.foreign_keys[0].delete_restrict);
        assert_eq!(installed_child.foreign_keys[0].referenced_columns, ["ID"]);
        assert_eq!(installed_parent.extract, signed[0].extract);
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
        body: impl Into<Body>,
    ) -> axum::response::Response {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header("authorization", basic());
        if method != Method::GET {
            request = request.header("x-csrf-zosmf-header", "true");
        }
        app.clone()
            .oneshot(request.body(body.into()).unwrap())
            .await
            .unwrap()
    }

    #[test]
    fn dataset_catalog_listing_filters_each_name_without_hidden_pagination_hints() {
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
        server.bootstrap_identity("OTHER", b"OTHERPASS1").unwrap();
        server
            .racf
            .define_profile("DATASET", "CATALOG.**", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("DATASET", "CATALOG.**", "IBMUSER", AccessIntent::Alter)
            .unwrap();
        server
            .racf
            .define_profile("DATASET", "CATALOG.HIDDEN", "OTHER", None)
            .unwrap();
        server
            .racf
            .permit("DATASET", "CATALOG.HIDDEN", "OTHER", AccessIntent::Alter)
            .unwrap();
        let attributes = json!({"dsorg":"PS","recfm":"V","lrecl":80});
        server
            .handle(
                Authentication::Basic {
                    user: "OTHER".into(),
                    secret: b"OTHERPASS1".to_vec(),
                },
                GatewayRequest::DatasetCreate {
                    dataset: "CATALOG.HIDDEN".into(),
                    attributes: attributes.clone(),
                },
            )
            .unwrap();
        server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::DatasetCreate {
                    dataset: "CATALOG.PUBLIC".into(),
                    attributes,
                },
            )
            .unwrap();

        let response = server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::DatasetList {
                    pattern: "CATALOG.**".into(),
                    start: None,
                    attributes: false,
                    max: 1,
                },
            )
            .unwrap();
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.headers["X-IBM-Response-Rows"], "1");
        let mainframe_env_zosmf::GatewayBody::Json(body) = response.body else {
            panic!("dataset list did not return JSON")
        };
        assert_eq!(body["returnedRows"], 1);
        assert_eq!(body["moreRows"], false);
        assert_eq!(body["items"][0]["dsname"], "CATALOG.PUBLIC");
        assert!(!body.to_string().contains("CATALOG.HIDDEN"));
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
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
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
        assert_eq!(job["status"], "ACTIVE");
        assert_eq!(job["retcode"], Value::Null);
        assert_eq!(server.metrics().active, 0);
        let id = job["jobid"].as_str().unwrap().to_string();
        let completed = wait_for_terminal_job(&server, &id).await;
        assert_eq!(completed.state, mainframe_env_batch::JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
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
        let original_token = session["token"].as_str().unwrap();
        assert!(original_token.starts_with("session-"));
        let rotated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/zosmf/services/authenticate")
                    .header("authorization", format!("Bearer {original_token}"))
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rotated.status(), StatusCode::OK);
        let rotated: Value =
            serde_json::from_slice(&to_bytes(rotated.into_body(), 65536).await.unwrap()).unwrap();
        let rotated_token = rotated["token"].as_str().unwrap();
        assert_ne!(rotated_token, original_token);
        let old = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/zosmf/restjobs/jobs?owner=*")
                    .header("authorization", format!("Bearer {original_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
        let renewed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/zosmf/restjobs/jobs?owner=*")
                    .header("authorization", format!("Bearer {rotated_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(renewed.status(), StatusCode::OK);
        assert!(server.metrics().requests >= 4);
        wait_for_worker_health(&server, JES_WORKER_COUNT).await;
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
        assert_eq!(job["status"], "ACTIVE");
        let completed = wait_for_terminal_job(&server, job["jobid"].as_str().unwrap()).await;
        assert_eq!(completed.return_code, Some(0));
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
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("CC00".into(), "ONLINE".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "ONLINE".into(),
                    map: "ONLINE".into(),
                    line: 1,
                    column: 1,
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
        let stored = server
            .artifacts
            .get_artifact(&artifact_ref)
            .unwrap()
            .unwrap();
        let metadata = stored.executable.as_ref().unwrap();
        assert_eq!(metadata.artifact_contract, ARTIFACT_CONTRACT);
        assert_eq!(
            metadata.compatibility_profile,
            crate::cobol::artifact::COBOL_REFERENCE_COMPATIBILITY_PROFILE
        );
        assert_eq!(
            metadata.dialect_contracts.as_ref(),
            Some(&artifact.manifest().dialect_contracts)
        );
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
        let run_unit = body["terminal"]["run_unit"].as_str().unwrap();
        let execution_id = ExecutionId::new(
            format!("execution-{}", run_unit.strip_prefix("run-").unwrap()),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed,
            "online execution did not reach a durable terminal state"
        );
        assert!(
            server
                .store
                .events(&execution_id, 1, 32)
                .unwrap()
                .iter()
                .any(|event| matches!(
                    event.kind,
                    mainframe_env_execution_api::LifecycleEventKind::EffectIntent { .. }
                )),
            "online host effects bypassed the durable journal"
        );
    }

    #[test]
    fn compiled_start_and_retrieve_cross_shared_worker_and_durable_coordinator() {
        use mainframe_env_racf::CommandContext;

        let starter = published_source_fixture(
            "STARTER",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. STARTER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'PAYLOAD'.\n01 DATA-Y PIC X(8) VALUE 'SETDATA'.\n01 LENGTH-X PIC S9(4) COMP VALUE 7.\n01 TIME-X PIC S9(9) COMP VALUE 0.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') REQID('00000000') FROM(DATA-X) LENGTH(LENGTH-X) AFTER SECONDS(TIME-X) RTRANSID('BACK') RTERMID('T001') QUEUE('WORKQ') USERID('TARGET') FMH RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS START TRANSID('NX00') FROM(DATA-Y) LENGTH(LENGTH-X) INTERVAL(0) FMH PROTECT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let receiver = published_source_fixture(
            "RECEIVER",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. RECEIVER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE ALL 'Z'.\n01 SET-DATA-X PIC X(8) VALUE ALL 'Q'.\n01 LENGTH-X PIC S9(4) COMP VALUE 8.\n01 SET-LENGTH-X PIC S9(4) COMP VALUE 0.\n01 PTR-X POINTER.\n01 RTRANS-X PIC X(4) VALUE SPACES.\n01 RTERM-X PIC X(4) VALUE SPACES.\n01 QUEUE-X PIC X(8) VALUE SPACES.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nLINKAGE SECTION.\n01 LINK-X PIC X(7).\nPROCEDURE DIVISION.\nEXEC CICS RETRIEVE INTO(DATA-X) LENGTH(LENGTH-X) RTRANSID(RTRANS-X) RTERMID(RTERM-X) QUEUE(QUEUE-X) WAIT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS RETRIEVE SET(PTR-X) LENGTH(SET-LENGTH-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nSET ADDRESS OF LINK-X TO PTR-X.\nMOVE LINK-X TO SET-DATA-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = |artifact: &PublishedArtifact| {
            ArtifactRef::new(
                format!("sha256:{:x}", Sha256::digest(artifact.payload())),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let starter_ref = artifact_ref(&starter);
        let receiver_ref = artifact_ref(&receiver);
        let program = |name: &str, artifact: &PublishedArtifact, reference: ArtifactRef| {
            OnlineProgramDefinition {
                name: name.into(),
                artifact: reference,
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }
        };
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server.bootstrap_identity("TARGET", b"TARGETPASS").unwrap();
        server
            .racf
            .execute_command(
                &CommandContext::new(
                    PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    "interval-surrogate-class",
                    "interval-surrogate-class",
                    1,
                )
                .unwrap(),
                "SETROPTS CLASSACT(SURROGAT)",
            )
            .unwrap();
        server
            .racf
            .define_profile("SURROGAT", "TARGET.DFHSTART", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("SURROGAT", "TARGET.DFHSTART", "IBMUSER", AccessIntent::Read)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    program("STARTER", &starter, starter_ref.clone()),
                    program("RECEIVER", &receiver, receiver_ref.clone()),
                ],
                transactions: BTreeMap::from([
                    ("ST00".into(), "STARTER".into()),
                    ("NX00".into(), "RECEIVER".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "INTERVL".into(),
                    map: "INTERVL".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();

        let receiver_session = SessionId::new("interval-receiver", 64).unwrap();
        let receiver_invocation = server
            .cics_invocation("IBMUSER", "NX00", Some(receiver_ref))
            .unwrap();
        server
            .cics
            .launch_background_task(receiver_invocation.clone(), &receiver_session, "NX00")
            .unwrap();
        let receiver_context = server
            .cics
            .terminal_execution(&receiver_session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&receiver_session, "RECEIVER", &receiver_context)
            .unwrap();
        server
            .run_online_exchange(&receiver_session, &principal, "RECEIVER", 2)
            .unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&receiver_invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );

        let starter_session = SessionId::new("interval-starter", 64).unwrap();
        let starter_invocation = server
            .cics_invocation("IBMUSER", "ST00", Some(starter_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                starter_invocation.clone(),
                &starter_session,
                "ST00",
                24,
                80,
                "interval-starter-csrf",
                3,
                10_000,
            )
            .unwrap();
        let starter_context = server
            .cics
            .terminal_execution(&starter_session, &principal, 4)
            .unwrap();
        server
            .begin_online_exchange(&starter_session, "STARTER", &starter_context)
            .unwrap();
        server
            .run_online_exchange(&starter_session, &principal, "STARTER", 4)
            .unwrap();
        assert!(
            server
                .cics
                .terminal_run_trace(&starter_session, &principal, 4)
                .unwrap()
                .iter()
                .any(|entry| entry.operation == CicsOperation::Start)
        );
        let starter_continuation = server
            .online_machine_continuation(&starter_session)
            .unwrap()
            .unwrap();
        let mut restored_starter = ReferenceMachine::from_binary(
            starter.payload(),
            starter_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored_starter
            .restore_checkpoint(&starter_continuation.checkpoint)
            .unwrap();
        let generated_request_id = restored_starter
            .variable("EIBREQID")
            .unwrap()
            .bytes()
            .to_vec();
        assert_eq!(generated_request_id.len(), 8);
        assert!(generated_request_id.iter().all(u8::is_ascii_hexdigit));

        for request_id in [b"00000000".as_slice(), generated_request_id.as_slice()] {
            let work = server
                .claim_jes_work("interval-worker")
                .unwrap()
                .expect("due START work");
            assert_eq!(work.required_generation, CICS_START_WORK_GENERATION);
            assert_eq!(work.payload, request_id);
            server
                .cics
                .promote_start_work(&work, server.jes_tick().unwrap())
                .unwrap();
            server
                .store
                .complete(
                    &work.work_id,
                    work.lease_id.as_deref().unwrap(),
                    work.lease_epoch,
                    server.jes_tick().unwrap(),
                )
                .unwrap();
        }

        server
            .run_online_exchange(&receiver_session, &principal, "RECEIVER", 5)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&receiver_session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            receiver.payload(),
            receiver_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("DATA-X").unwrap().bytes(), b"PAYLOAD ");
        assert_eq!(
            restored.variable("SET-DATA-X").unwrap().bytes(),
            b"SETDATA "
        );
        assert_eq!(restored.variable("LINK-X").unwrap().bytes(), b"SETDATA");
        assert_eq!(restored.variable("LENGTH-X").unwrap().bytes(), &[0, 7]);
        assert_eq!(restored.variable("SET-LENGTH-X").unwrap().bytes(), &[0, 7]);
        assert!(
            restored
                .variable("PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert_eq!(restored.variable("RTRANS-X").unwrap().bytes(), b"BACK");
        assert_eq!(restored.variable("RTERM-X").unwrap().bytes(), b"T001");
        assert_eq!(restored.variable("QUEUE-X").unwrap().bytes(), b"WORKQ   ");
        assert_eq!(restored.variable("EIBFMH").unwrap().bytes(), &[0xff]);
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 0]);
        assert!(
            server
                .cics
                .terminal_run_trace(&receiver_session, &principal, 5)
                .unwrap()
                .iter()
                .any(|entry| entry.operation == CicsOperation::Retrieve)
        );
    }

    #[test]
    fn due_local_start_launches_one_facilityless_target_across_worker_retry() {
        let starter = published_source_fixture(
            "AUTOSTRT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. AUTOSTRT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'PAYLOAD'.\n01 LENGTH-X PIC S9(4) COMP VALUE 7.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('ATGT') REQID('AUTO0001') FROM(DATA-X) LENGTH(LENGTH-X) INTERVAL(0) END-EXEC.\nSTOP RUN.\n",
        );
        let target = published_source_fixture(
            "AUTOTRGT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. AUTOTRGT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE ALL 'Z'.\n01 LENGTH-X PIC S9(4) COMP VALUE 8.\nPROCEDURE DIVISION.\nEXEC CICS RETRIEVE INTO(DATA-X) LENGTH(LENGTH-X) END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = |artifact: &PublishedArtifact| {
            ArtifactRef::new(
                format!("sha256:{:x}", Sha256::digest(artifact.payload())),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let starter_ref = artifact_ref(&starter);
        let target_ref = artifact_ref(&target);
        let program = |name: &str, artifact: &PublishedArtifact, reference: ArtifactRef| {
            OnlineProgramDefinition {
                name: name.into(),
                artifact: reference,
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }
        };
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    program("AUTOSTRT", &starter, starter_ref.clone()),
                    program("AUTOTRGT", &target, target_ref),
                ],
                transactions: BTreeMap::from([
                    ("ASTR".into(), "AUTOSTRT".into()),
                    ("ATGT".into(), "AUTOTRGT".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "AUTOSTRT".into(),
                    map: "AUTOSTRT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("automatic-start-issuer", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "ASTR", Some(starter_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "ASTR",
                24,
                80,
                "automatic-start-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "AUTOSTRT", 2)
            .unwrap();

        let first = server
            .claim_jes_work("automatic-start-worker-1")
            .unwrap()
            .expect("due START work");
        assert_eq!(first.work_id, "cics-start:AUTO0001");
        assert!(matches!(
            server.process_claimed_jes_work(&first).unwrap(),
            JesWorkOutcome::Completed
        ));
        let first_execution = server
            .store
            .get_execution(&first.execution_id)
            .unwrap()
            .expect("started target execution");
        assert_eq!(first_execution.state, ExecutionState::Completed);
        let first_version = first_execution.version;

        let retry_tick = server.jes_tick().unwrap();
        server
            .store
            .release(
                &first.work_id,
                first.lease_id.as_deref().unwrap(),
                first.lease_epoch,
                retry_tick,
                retry_tick,
            )
            .unwrap();
        let retry = server
            .claim_jes_work("automatic-start-worker-2")
            .unwrap()
            .expect("reclaimed START work");
        assert!(retry.lease_epoch > first.lease_epoch);
        let outcome = server.process_claimed_jes_work(&retry).unwrap();
        server.finish_claimed_jes_work(&retry, Ok(outcome)).unwrap();
        let retried_execution = server
            .store
            .get_execution(&retry.execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(retried_execution.state, ExecutionState::Completed);
        assert_eq!(retried_execution.version, first_version);
        assert_eq!(
            server
                .store
                .get_work(&retry.work_id)
                .unwrap()
                .unwrap()
                .state,
            WorkState::Completed
        );
        assert!(
            server
                .store
                .get_provider_state(
                    "cics-session",
                    &format!("cics-start-task-{}", retry.execution_id),
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sqlite_restart_launches_ready_start_once_and_recovers_post_execution_gap() {
        let starter = published_source_fixture(
            "RSTSTRT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. RSTSTRT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'RESTART'.\n01 LENGTH-X PIC S9(4) COMP VALUE 7.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('RTGT') REQID('RST00001') FROM(DATA-X) LENGTH(LENGTH-X) INTERVAL(0) END-EXEC.\nSTOP RUN.\n",
        );
        let target = published_source_fixture(
            "RSTTRGT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. RSTTRGT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE ALL 'Z'.\n01 LENGTH-X PIC S9(4) COMP VALUE 8.\nPROCEDURE DIVISION.\nEXEC CICS RETRIEVE INTO(DATA-X) LENGTH(LENGTH-X) END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = |artifact: &PublishedArtifact| {
            ArtifactRef::new(
                format!("sha256:{:x}", Sha256::digest(artifact.payload())),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let starter_ref = artifact_ref(&starter);
        let target_ref = artifact_ref(&target);
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-start-launch-restart-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = directory.join("artifacts");
        server_config.timeout_millis = 10_000;

        let first_clock = Arc::new(ManualJesClock::new(100));
        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let first = ProductServer::open_with_clock(
            server_config.clone(),
            first_platform,
            first_clock.clone(),
        )
        .unwrap();
        let execution_clock = first_clock;
        first
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        first.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let program = |name: &str, artifact: &PublishedArtifact, reference: ArtifactRef| {
            OnlineProgramDefinition {
                name: name.into(),
                artifact: reference,
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }
        };
        first
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    program("RSTSTRT", &starter, starter_ref.clone()),
                    program("RSTTRGT", &target, target_ref),
                ],
                transactions: BTreeMap::from([
                    ("RSTR".into(), "RSTSTRT".into()),
                    ("RTGT".into(), "RSTTRGT".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "RSTSTRT".into(),
                    map: "RSTSTRT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("restart-start-issuer", 64).unwrap();
        let invocation = first
            .cics_invocation("IBMUSER", "RSTR", Some(starter_ref))
            .unwrap();
        first
            .cics
            .launch_terminal(
                invocation,
                &session,
                "RSTR",
                24,
                80,
                "restart-start-csrf",
                100,
                10_000,
            )
            .unwrap();
        first
            .run_online_exchange(&session, &principal, "RSTSTRT", 100)
            .unwrap();
        let first_work = first
            .claim_jes_work("start-before-launch-crash")
            .unwrap()
            .unwrap();
        first
            .cics
            .promote_start_work(&first_work, first.jes_tick().unwrap())
            .unwrap();
        let execution_id = first_work.execution_id.clone();
        let first_epoch = first_work.lease_epoch;
        assert!(first.store.get_execution(&execution_id).unwrap().is_none());
        drop((first, first_store));

        let second_tick = 100 + crate::jes_worker::JES_LEASE_TICKS + 1;
        let second_clock = Arc::new(ManualJesClock::new(second_tick));
        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let second = ProductServer::open_with_clock(
            server_config.clone(),
            second_platform,
            second_clock.clone(),
        )
        .unwrap();
        let execution_clock = second_clock;
        second
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let second_work = second
            .claim_jes_work("start-after-launch-crash")
            .unwrap()
            .unwrap();
        assert!(second_work.lease_epoch > first_epoch);
        assert!(matches!(
            second.process_claimed_jes_work(&second_work).unwrap(),
            JesWorkOutcome::Completed
        ));
        let completed = second.store.get_execution(&execution_id).unwrap().unwrap();
        assert_eq!(completed.state, ExecutionState::Completed);
        let completed_version = completed.version;
        let second_epoch = second_work.lease_epoch;
        drop((second, second_store));

        let third_tick = second_tick + crate::jes_worker::JES_LEASE_TICKS + 1;
        let third_clock = Arc::new(ManualJesClock::new(third_tick));
        let third_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let third_platform: Arc<dyn PlatformStore> = third_store.clone();
        let third =
            ProductServer::open_with_clock(server_config, third_platform, third_clock.clone())
                .unwrap();
        let execution_clock = third_clock;
        third
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let third_work = third
            .claim_jes_work("start-after-execution-crash")
            .unwrap()
            .unwrap();
        assert!(third_work.lease_epoch > second_epoch);
        let outcome = third.process_claimed_jes_work(&third_work).unwrap();
        third
            .finish_claimed_jes_work(&third_work, Ok(outcome))
            .unwrap();
        let recovered = third.store.get_execution(&execution_id).unwrap().unwrap();
        assert_eq!(recovered.state, ExecutionState::Completed);
        assert_eq!(recovered.version, completed_version);
        assert_eq!(
            third
                .store
                .get_work(&third_work.work_id)
                .unwrap()
                .unwrap()
                .state,
            WorkState::Completed
        );
        drop((third, third_store));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compiled_start_termid_binds_the_named_virtual_terminal() {
        let issuer = published_source_fixture(
            "STARTTRM",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. STARTTRM.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'PAYLOAD'.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NEXT') TERMID('T000') REQID('TRMID001') FROM(DATA-X) INTERVAL(0) END-EXEC.\nSTOP RUN.\n",
        );
        let target = published_source_fixture(
            "TRMTRGT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. TRMTRGT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE ALL 'Z'.\n01 LENGTH-X PIC S9(4) COMP VALUE 8.\nPROCEDURE DIVISION.\nEXEC CICS RETRIEVE INTO(DATA-X) LENGTH(LENGTH-X) END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = |artifact: &PublishedArtifact| {
            ArtifactRef::new(
                format!("sha256:{:x}", Sha256::digest(artifact.payload())),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let issuer_ref = artifact_ref(&issuer);
        let target_ref = artifact_ref(&target);
        let program = |name: &str, artifact: &PublishedArtifact, reference: ArtifactRef| {
            OnlineProgramDefinition {
                name: name.into(),
                artifact: reference,
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }
        };
        let clock = Arc::new(ManualJesClock::new(100));
        let platform: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let server = ProductServer::open_with_clock(config(), platform, clock.clone()).unwrap();
        let execution_clock = clock.clone();
        server
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    program("STARTTRM", &issuer, issuer_ref.clone()),
                    program("TRMTRGT", &target, target_ref),
                ],
                transactions: BTreeMap::from([
                    ("STRM".into(), "STARTTRM".into()),
                    ("NEXT".into(), "TRMTRGT".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "STARTTRM".into(),
                    map: "STARTTRM".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("start-termid-issuer", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "STRM", Some(issuer_ref.clone()))
            .unwrap();
        let launch_tick = server.jes_tick().unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "STRM",
                24,
                80,
                "start-termid-csrf",
                launch_tick,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "STARTTRM", launch_tick)
            .unwrap();
        let busy = server
            .cics_invocation("IBMUSER", "STRM", Some(issuer_ref))
            .unwrap();
        server
            .cics
            .resume_terminal(busy, &session, "start-termid-csrf", launch_tick)
            .unwrap();
        let deferred = server
            .claim_jes_work("start-termid-worker")
            .unwrap()
            .unwrap();
        let outcome = server.process_claimed_jes_work(&deferred).unwrap();
        assert_eq!(outcome, JesWorkOutcome::Deferred);
        server
            .finish_claimed_jes_work(&deferred, Ok(outcome))
            .unwrap();
        assert_eq!(
            server
                .store
                .get_work(&deferred.work_id)
                .unwrap()
                .unwrap()
                .state,
            WorkState::Queued
        );
        server
            .cics
            .complete_terminal_run(&session, &principal, launch_tick)
            .unwrap();
        clock.advance(JES_IDLE_MILLIS);
        let work = server
            .claim_jes_work("start-termid-worker-retry")
            .unwrap()
            .unwrap();
        let outcome = server.process_claimed_jes_work(&work).unwrap();
        server.finish_claimed_jes_work(&work, Ok(outcome)).unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&work.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let terminal = server
            .cics
            .terminal_snapshot(&session, &principal, server.jes_tick().unwrap())
            .unwrap();
        assert_eq!(terminal.transaction, "NEXT");
        assert_eq!(
            server.store.get_work(&work.work_id).unwrap().unwrap().state,
            WorkState::Completed
        );
    }

    #[test]
    fn compiled_deleteq_td_deallocates_the_queue_and_reports_missing_redelete() {
        let artifact = published_source_fixture(
            "DELETETD",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELETETD.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(6) VALUE 'ABCDEF'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS WRITEQ TD QUEUE('OUTQ') FROM(DATA-X) LENGTH(3) END-EXEC.\nEXEC CICS DELETEQ TD QUEUE('OUTQ') END-EXEC.\nEXEC CICS DELETEQ TD QUEUE('OUTQ') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .racf
            .define_profile("QUEUE", "CICS.TD.OUTQ", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("QUEUE", "CICS.TD.OUTQ", "IBMUSER", AccessIntent::Update)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELETETD".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DQTD".into(), "DELETETD".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELETETD".into(),
                    map: "DELETETD".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("deleteq-td-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DQTD", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DQTD",
                24,
                80,
                "deleteq-td-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DELETETD", 2)
            .unwrap();
        assert!(server.cics.transient_records("OUTQ").unwrap().is_empty());
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 44]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0, 0, 0, 0]);
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::DeleteTransientData)
                .count(),
            2
        );
    }

    #[test]
    fn compiled_document_create_selects_the_typed_durable_route() {
        let artifact = published_source_fixture(
            "DOCCREAT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DOCCREAT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TOKEN-X PIC X(16).\n01 TEXT-X PIC X(8) VALUE 'DOCUMENT'.\n01 LENGTH-X PIC S9(9) COMP VALUE 4.\n01 SIZE-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DOCUMENT CREATE DOCTOKEN(TOKEN-X) TEXT(TEXT-X) LENGTH(LENGTH-X) DOCSIZE(SIZE-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DOCCREAT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DOCC".into(), "DOCCREAT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DOCCREAT".into(),
                    map: "DOCCREAT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("document-create-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DOCC", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DOCC",
                24,
                80,
                "document-create-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DOCCREAT", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_ne!(restored.variable("TOKEN-X").unwrap().bytes(), &[0; 16]);
        assert_eq!(restored.variable("SIZE-X").unwrap().bytes(), &[0, 0, 0, 4]);
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::DocumentCreate)
                .count(),
            1
        );
        assert_eq!(
            server
                .store
                .list_provider_state("cics-document-v1", 8)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn compiled_document_delete_releases_created_document() {
        let artifact = published_source_fixture(
            "DOCDELET",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DOCDELET.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TOKEN-X PIC X(16).\n01 TEXT-X PIC X(8) VALUE 'DOCUMENT'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\n01 DELETE-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS DOCUMENT CREATE DOCTOKEN(TOKEN-X) TEXT(TEXT-X) LENGTH(8) END-EXEC.\nEXEC CICS DOCUMENT DELETE DOCTOKEN(TOKEN-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE EIBFN TO DELETE-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DOCDELET".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DOCD".into(), "DOCDELET".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DOCDELET".into(),
                    map: "DOCDELET".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("document-delete-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DOCD", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DOCD",
                24,
                80,
                "document-delete-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DOCDELET", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 0]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0, 0, 0, 0]);
        assert_eq!(
            restored.variable("DELETE-FN").unwrap().bytes(),
            &[0x3C, 0x10]
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::DocumentDelete)
                .count(),
            1
        );
        assert!(
            server
                .store
                .list_provider_state("cics-document-v1", 8)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn compiled_document_insert_updates_size_and_eibfn() {
        let artifact = published_source_fixture(
            "DOCINSRT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DOCINSRT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TOKEN-X PIC X(16).\n01 TEXT-X PIC X(4) VALUE 'DATA'.\n01 SIZE-X PIC S9(9) COMP.\n01 INSERT-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS DOCUMENT CREATE DOCTOKEN(TOKEN-X) END-EXEC.\nEXEC CICS DOCUMENT INSERT DOCTOKEN(TOKEN-X) TEXT(TEXT-X) LENGTH(4) DOCSIZE(SIZE-X) END-EXEC.\nMOVE EIBFN TO INSERT-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DOCINSRT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DOCI".into(), "DOCINSRT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DOCINSRT".into(),
                    map: "DOCINSRT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("document-insert-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DOCI", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DOCI",
                24,
                80,
                "document-insert-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DOCINSRT", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("SIZE-X").unwrap().bytes(), &[0, 0, 0, 4]);
        assert_eq!(
            restored.variable("INSERT-FN").unwrap().bytes(),
            &[0x3C, 0x04]
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::DocumentInsert)
                .count(),
            1
        );
        assert_eq!(
            server
                .store
                .list_provider_state("cics-document-v1", 8)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn compiled_readq_td_consumes_into_and_returns_original_length() {
        let artifact = published_source_fixture(
            "READQTD",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READQTD.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(6) VALUE 'ABCDEF'.\n01 INTO-X PIC X(4) VALUE SPACES.\n01 LENGTH-X PIC S9(4) COMP VALUE 3.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\n01 EMPTY-RESP-X PIC S9(9) COMP.\n01 EMPTY-RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS WRITEQ TD QUEUE('IN01') FROM(DATA-X) END-EXEC.\nEXEC CICS READQ TD QUEUE('IN01') INTO(INTO-X) LENGTH(LENGTH-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS READQ TD QUEUE('IN01') INTO(INTO-X) RESP(EMPTY-RESP-X) RESP2(EMPTY-RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .racf
            .define_profile("QUEUE", "CICS.TD.IN01", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("QUEUE", "CICS.TD.IN01", "IBMUSER", AccessIntent::Update)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READQTD".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RQTD".into(), "READQTD".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READQTD".into(),
                    map: "READQTD".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("readq-td-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RQTD", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RQTD",
                24,
                80,
                "readq-td-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READQTD", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("INTO-X").unwrap().bytes(), b"ABC ");
        assert_eq!(restored.variable("LENGTH-X").unwrap().bytes(), &[0, 6]);
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 22]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0, 0, 0, 0]);
        assert_eq!(
            restored.variable("EMPTY-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 23]
        );
        assert_eq!(
            restored.variable("EMPTY-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 0]
        );
        assert!(server.cics.transient_records("IN01").unwrap().is_empty());
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::ReadTransientData)
                .count(),
            2
        );
    }

    #[test]
    fn compiled_readq_td_set_allocates_checkpointed_record_storage() {
        let artifact = published_source_fixture(
            "READQSET",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READQSET.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(7) VALUE 'POINTER'.\n01 PTR-X POINTER.\n01 LENGTH-X PIC S9(4) COMP VALUE 7.\n01 OBSERVED-X PIC X(7) VALUE SPACES.\nLINKAGE SECTION.\n01 LINK-X PIC X(7).\nPROCEDURE DIVISION.\nEXEC CICS WRITEQ TD QUEUE('SETQ') FROM(DATA-X) END-EXEC.\nEXEC CICS READQ TD QUEUE('SETQ') SET(PTR-X) LENGTH(LENGTH-X) END-EXEC.\nSET ADDRESS OF LINK-X TO PTR-X.\nMOVE LINK-X TO OBSERVED-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .racf
            .define_profile("QUEUE", "CICS.TD.SETQ", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("QUEUE", "CICS.TD.SETQ", "IBMUSER", AccessIntent::Update)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READQSET".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RQST".into(), "READQSET".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READQSET".into(),
                    map: "READQSET".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("readq-td-set-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RQST", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RQST",
                24,
                80,
                "readq-td-set-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READQSET", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("OBSERVED-X").unwrap().bytes(), b"POINTER");
        assert_eq!(restored.variable("LENGTH-X").unwrap().bytes(), &[0, 7]);
        assert!(
            restored
                .variable("PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert!(server.cics.transient_records("SETQ").unwrap().is_empty());
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::ReadTransientData)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_tdq_sysid_selects_only_the_local_system() {
        let artifact = published_source_fixture(
            "TDQSYSID",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. TDQSYSID.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(5) VALUE 'LOCAL'.\n01 INTO-X PIC X(5) VALUE SPACES.\n01 SYSID-X PIC X(4) VALUE 'S001'.\n01 REMOTE-X PIC X(4) VALUE 'R001'.\n01 READ-RESP-X PIC S9(9) COMP.\n01 READ-RESP2-X PIC S9(9) COMP.\n01 DELETE-RESP-X PIC S9(9) COMP.\n01 DELETE-RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS WRITEQ TD QUEUE('SYSQ') FROM(DATA-X) SYSID(SYSID-X) END-EXEC.\nEXEC CICS READQ TD QUEUE('SYSQ') INTO(INTO-X) SYSID(REMOTE-X) RESP(READ-RESP-X) RESP2(READ-RESP2-X) END-EXEC.\nEXEC CICS READQ TD QUEUE('SYSQ') INTO(INTO-X) SYSID(SYSID-X) END-EXEC.\nEXEC CICS WRITEQ TD QUEUE('SYSQ') FROM(DATA-X) SYSID(SYSID-X) END-EXEC.\nEXEC CICS DELETEQ TD QUEUE('SYSQ') SYSID(REMOTE-X) RESP(DELETE-RESP-X) RESP2(DELETE-RESP2-X) END-EXEC.\nEXEC CICS DELETEQ TD QUEUE('SYSQ') SYSID(SYSID-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .racf
            .define_profile("QUEUE", "CICS.TD.SYSQ", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit("QUEUE", "CICS.TD.SYSQ", "IBMUSER", AccessIntent::Update)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "TDQSYSID".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("TDSY".into(), "TDQSYSID".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "TDQSYSID".into(),
                    map: "TDQSYSID".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("tdq-sysid-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "TDSY", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "TDSY",
                24,
                80,
                "tdq-sysid-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "TDQSYSID", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("INTO-X").unwrap().bytes(), b"LOCAL");
        assert_eq!(
            restored.variable("READ-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 53]
        );
        assert_eq!(
            restored.variable("READ-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 0]
        );
        assert_eq!(
            restored.variable("DELETE-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 53]
        );
        assert_eq!(
            restored.variable("DELETE-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 0]
        );
        assert!(server.cics.transient_records("SYSQ").unwrap().is_empty());
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 2)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.outcome == "SYSIDERR" && entry.response == 53)
                .count(),
            2
        );
    }

    #[test]
    fn compiled_getmain_and_freemain_preserve_checkpointed_virtual_storage_rules() {
        let artifact = published_source_fixture(
            "GETMAINA",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. GETMAINA.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PTR-X POINTER.\n01 DATA-PTR-X POINTER.\n01 ZERO-PTR-X POINTER.\n01 LENGTH-X PIC S9(9) COMP VALUE 4.\n01 ZERO-X PIC S9(9) COMP VALUE 0.\n01 INIT-X PIC X VALUE 'Z'.\n01 OBSERVED-X PIC X(4) VALUE SPACES.\n01 DATA-OBSERVED-X PIC X(4) VALUE SPACES.\n01 ZERO-RESP-X PIC S9(9) COMP.\n01 ZERO-RESP2-X PIC S9(9) COMP.\n01 FREE-RESP-X PIC S9(9) COMP.\n01 FREE-RESP2-X PIC S9(9) COMP.\n01 STATIC-FREE-RESP-X PIC S9(9) COMP.\n01 STATIC-FREE-RESP2-X PIC S9(9) COMP.\n01 UNASSIGNED-RESP-X PIC S9(9) COMP.\n01 UNASSIGNED-RESP2-X PIC S9(9) COMP.\n01 DATA-FREE-RESP-X PIC S9(9) COMP.\n01 DATA-FREE-RESP2-X PIC S9(9) COMP.\nLINKAGE SECTION.\n01 LINK-X PIC X(4).\n01 LINK-Y PIC X(4).\nPROCEDURE DIVISION.\nEXEC CICS GETMAIN SET(PTR-X) FLENGTH(LENGTH-X) INITIMG(INIT-X) NOSUSPEND END-EXEC.\nSET ADDRESS OF LINK-X TO PTR-X.\nMOVE LINK-X TO OBSERVED-X.\nEXEC CICS GETMAIN SET(ZERO-PTR-X) FLENGTH(ZERO-X) RESP(ZERO-RESP-X) RESP2(ZERO-RESP2-X) END-EXEC.\nEXEC CICS FREEMAIN DATAPOINTER(PTR-X) END-EXEC.\nEXEC CICS FREEMAIN DATAPOINTER(PTR-X) RESP(FREE-RESP-X) RESP2(FREE-RESP2-X) END-EXEC.\nEXEC CICS FREEMAIN DATA(OBSERVED-X) RESP(STATIC-FREE-RESP-X) RESP2(STATIC-FREE-RESP2-X) END-EXEC.\nEXEC CICS FREEMAIN DATA(LINK-Y) RESP(UNASSIGNED-RESP-X) RESP2(UNASSIGNED-RESP2-X) END-EXEC.\nEXEC CICS GETMAIN SET(DATA-PTR-X) FLENGTH(LENGTH-X) INITIMG(INIT-X) END-EXEC.\nSET ADDRESS OF LINK-Y TO DATA-PTR-X.\nMOVE LINK-Y TO DATA-OBSERVED-X.\nEXEC CICS FREEMAIN DATA(LINK-Y) END-EXEC.\nEXEC CICS FREEMAIN DATA(LINK-Y) RESP(DATA-FREE-RESP-X) RESP2(DATA-FREE-RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "GETMAINA".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("GM00".into(), "GETMAINA".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "GETMAINA".into(),
                    map: "GETMAINA".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("getmain-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "GM00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "GM00",
                24,
                80,
                "getmain-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "GETMAINA", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert!(restored.variable("LINK-X").is_none());
        assert!(restored.variable("LINK-Y").is_none());
        assert_eq!(restored.variable("OBSERVED-X").unwrap().bytes(), b"ZZZZ");
        assert_eq!(
            restored.variable("DATA-OBSERVED-X").unwrap().bytes(),
            b"ZZZZ"
        );
        assert!(
            restored
                .variable("PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert!(
            restored
                .variable("ZERO-PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(
            restored
                .variable("DATA-PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert_eq!(
            restored.variable("ZERO-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 22]
        );
        assert_eq!(
            restored.variable("ZERO-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 1]
        );
        assert_eq!(
            restored.variable("FREE-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("FREE-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 1]
        );
        assert_eq!(
            restored.variable("STATIC-FREE-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("STATIC-FREE-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 1]
        );
        assert_eq!(
            restored.variable("UNASSIGNED-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("UNASSIGNED-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 1]
        );
        assert_eq!(
            restored.variable("DATA-FREE-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("DATA-FREE-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 1]
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Getmain)
                .count(),
            3
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Freemain)
                .count(),
            6
        );
    }

    #[test]
    fn compiled_getmain_length_uses_halfword_compatibility_storage() {
        let artifact = published_source_fixture(
            "GETMAINL",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. GETMAINL.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PTR-X POINTER.\n01 ZERO-PTR-X POINTER.\n01 LENGTH-X PIC 9(4) COMP VALUE 4.\n01 ZERO-X PIC 9(4) COMP VALUE 0.\n01 INIT-X PIC X VALUE 'Q'.\n01 OBSERVED-X PIC X(4) VALUE SPACES.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nLINKAGE SECTION.\n01 LINK-X PIC X(4).\nPROCEDURE DIVISION.\nEXEC CICS GETMAIN SET(PTR-X) LENGTH(LENGTH-X) INITIMG(INIT-X) END-EXEC.\nSET ADDRESS OF LINK-X TO PTR-X.\nMOVE LINK-X TO OBSERVED-X.\nEXEC CICS GETMAIN SET(ZERO-PTR-X) LENGTH(ZERO-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS FREEMAIN DATA(LINK-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "GETMAINL".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("GL00".into(), "GETMAINL".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "GETMAINL".into(),
                    map: "GETMAINL".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("getmain-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "GL00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "GL00",
                24,
                80,
                "getmain-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "GETMAINL", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert!(restored.variable("LINK-X").is_none());
        assert_eq!(restored.variable("OBSERVED-X").unwrap().bytes(), b"QQQQ");
        assert!(
            restored
                .variable("PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert!(
            restored
                .variable("ZERO-PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 22]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0, 0, 0, 1]);
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 2)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Getmain)
                .count(),
            2
        );
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Freemain)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_delete_without_ridfld_consumes_read_update_hold() {
        let artifact = published_source_fixture(
            "CURDEL",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. CURDEL.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 RECORD-X PIC X(7).\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) UPDATE END-EXEC.\nEXEC CICS DELETE FILE('ACCTDAT') END-EXEC.\nEXEC CICS SYNCPOINT END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"V",
                        "lrecl":16,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    member: None,
                    bytes: b"003DATA".to_vec(),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "CURDEL".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("CD00".into(), "CURDEL".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "CURDEL".into(),
                    map: "CURDEL".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("current-record-delete-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "CD00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "CD00",
                24,
                80,
                "current-record-delete-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "CURDEL", 2)
            .unwrap();
        assert!(matches!(
            server.dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
                member: None,
                key: Some(b"003".to_vec()),
                max_records: 1,
                control: Default::default(),
            }),
            Err(HostProblem::Condition {
                ref name,
                response: 13,
                ..
            }) if name == "NOTFND"
        ));
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 3)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Delete)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_delete_file_keylength_removes_selected_key() {
        let artifact = published_source_fixture(
            "DELKEY",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELKEY.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3.\nPROCEDURE DIVISION.\nEXEC CICS DELETE FILE('ACCTDAT') RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC.\nEXEC CICS SYNCPOINT END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"V",
                        "lrecl":16,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    member: None,
                    bytes: b"003DATA".to_vec(),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELKEY".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DK00".into(), "DELKEY".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELKEY".into(),
                    map: "DELKEY".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("delete-file-keylength-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DK00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "DK00",
                24,
                80,
                "delete-file-keylength-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DELKEY", 2)
            .unwrap();
        assert!(matches!(
            server.dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
                member: None,
                key: Some(b"003".to_vec()),
                max_records: 1,
                control: Default::default(),
            }),
            Err(HostProblem::Condition {
                ref name,
                response: 13,
                ..
            }) if name == "NOTFND"
        ));
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Delete)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_read_file_gteq_returns_next_keyed_record() {
        let artifact = published_source_fixture(
            "READGTEQ",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READGTEQ.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '004'.\n01 ZERO-X PIC S9(4) COMP VALUE 0.\n01 RECORD-X PIC X(7).\n01 FIRST-X PIC X(7).\n01 GENERIC-FIRST-X PIC X(7).\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) GTEQ END-EXEC.\nEXEC CICS READ FILE('ACCTDAT') INTO(FIRST-X) RIDFLD(KEY-X) KEYLENGTH(ZERO-X) GTEQ END-EXEC.\nEXEC CICS READ FILE('ACCTDAT') INTO(GENERIC-FIRST-X) RIDFLD(KEY-X) KEYLENGTH(ZERO-X) GENERIC GTEQ END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"F",
                        "lrecl":7,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        for bytes in [b"003DATA".to_vec(), b"005NEXT".to_vec()] {
            server
                .handle(
                    authentication(),
                    GatewayRequest::DatasetWrite {
                        dataset: "IBMUSER.ACCTDAT".into(),
                        member: None,
                        bytes,
                    },
                )
                .unwrap();
        }
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READGTEQ".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RG00".into(), "READGTEQ".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READGTEQ".into(),
                    map: "READGTEQ".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("read-file-gteq-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RG00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RG00",
                24,
                80,
                "read-file-gteq-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READGTEQ", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("RECORD-X").unwrap().bytes(), b"005NEXT");
        assert_eq!(restored.variable("FIRST-X").unwrap().bytes(), b"003DATA");
        assert_eq!(
            restored.variable("GENERIC-FIRST-X").unwrap().bytes(),
            b"003DATA"
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 4)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            3
        );
    }

    #[test]
    fn compiled_read_file_generic_matches_prefix_and_reports_notfnd() {
        let artifact = published_source_fixture(
            "READGEN",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READGEN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '00Z'.\n01 KEY-LENGTH-X PIC S9(4) COMP VALUE 2.\n01 RECORD-X PIC X(7).\n01 MISS-X PIC X(7).\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) GENERIC EQUAL END-EXEC.\nMOVE '01Z' TO KEY-X.\nEXEC CICS READ FILE('ACCTDAT') INTO(MISS-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) GENERIC RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"F",
                        "lrecl":7,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        for bytes in [b"003DATA".to_vec(), b"005NEXT".to_vec()] {
            server
                .handle(
                    authentication(),
                    GatewayRequest::DatasetWrite {
                        dataset: "IBMUSER.ACCTDAT".into(),
                        member: None,
                        bytes,
                    },
                )
                .unwrap();
        }
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READGEN".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RE00".into(), "READGEN".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READGEN".into(),
                    map: "READGEN".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("read-file-generic-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RE00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RE00",
                24,
                80,
                "read-file-generic-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READGEN", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("RECORD-X").unwrap().bytes(), b"003DATA");
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0, 0, 0, 13]);
        assert_eq!(
            restored.variable("RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 80]
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            2
        );
    }

    #[test]
    fn compiled_read_file_length_writes_actual_record_length() {
        let artifact = published_source_fixture(
            "READLEN",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READLEN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 LENGTH-X PIC S9(4) COMP VALUE 8.\n01 RECORD-X PIC X(8) VALUE SPACES.\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"V",
                        "lrecl":16,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    member: None,
                    bytes: b"003DATA".to_vec(),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READLEN".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RN00".into(), "READLEN".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READLEN".into(),
                    map: "READLEN".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("read-file-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RN00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RN00",
                24,
                80,
                "read-file-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READLEN", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            &restored.variable("RECORD-X").unwrap().bytes()[..7],
            b"003DATA"
        );
        assert_eq!(restored.variable("LENGTH-X").unwrap().bytes(), &[0, 7]);
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_read_file_keylength_returns_selected_record() {
        let artifact = published_source_fixture(
            "READKEYL",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. READKEYL.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3.\n01 RECORD-X PIC X(7).\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(RECORD-X) RIDFLD(KEY-X) KEYLENGTH(KEY-LENGTH-X) EQUAL END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"F",
                        "lrecl":7,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    member: None,
                    bytes: b"003DATA".to_vec(),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "READKEYL".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RK00".into(), "READKEYL".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "READKEYL".into(),
                    map: "READKEYL".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("read-file-keylength-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RK00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "RK00",
                24,
                80,
                "read-file-keylength-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "READKEYL", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("RECORD-X").unwrap().bytes(), b"003DATA");
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_rewrite_file_length_persists_selected_prefix() {
        let artifact = published_source_fixture(
            "REWRITEL",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. REWRITEL.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 READ-X PIC X(8).\n01 UPDATE-X PIC X(8) VALUE '003ABCDE'.\n01 LENGTH-X PIC S9(4) COMP VALUE 5.\nPROCEDURE DIVISION.\nEXEC CICS READ FILE('ACCTDAT') INTO(READ-X) RIDFLD(KEY-X) UPDATE END-EXEC.\nEXEC CICS REWRITE FILE('ACCTDAT') FROM(UPDATE-X) LENGTH(LENGTH-X) END-EXEC.\nEXEC CICS SYNCPOINT END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let authentication = || Authentication::Basic {
            user: "IBMUSER".into(),
            secret: b"TESTPASS".to_vec(),
        };
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"V",
                        "lrecl":16,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    member: None,
                    bytes: b"003OLD".to_vec(),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "REWRITEL".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("RL00".into(), "REWRITEL".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "REWRITEL".into(),
                    map: "REWRITEL".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("rewrite-file-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RL00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "RL00",
                24,
                80,
                "rewrite-file-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "REWRITEL", 2)
            .unwrap();
        let result = server
            .dataset
            .invoke(DatasetRequest::Read {
                dataset: DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
                member: None,
                key: Some(b"003".to_vec()),
                max_records: 1,
                control: Default::default(),
            })
            .unwrap();
        let DatasetResult::Records { records, .. } = result else {
            panic!("expected rewritten keyed record");
        };
        assert_eq!(records, vec![b"003AB".to_vec()]);
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 3)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Read)
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Rewrite)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_write_file_length_persists_only_selected_prefix() {
        let artifact = published_source_fixture(
            "WRITELEN",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. WRITELEN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 KEY-X PIC X(3) VALUE '003'.\n01 RECORD-X PIC X(8) VALUE '003ABCDE'.\n01 LENGTH-X PIC S9(4) COMP VALUE 5.\n01 KEY-LENGTH-X PIC S9(4) COMP VALUE 3.\nPROCEDURE DIVISION.\nEXEC CICS WRITE FILE('ACCTDAT') FROM(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) KEYLENGTH(KEY-LENGTH-X) END-EXEC.\nEXEC CICS SYNCPOINT END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::DatasetCreate {
                    dataset: "IBMUSER.ACCTDAT".into(),
                    attributes: json!({
                        "dsorg":"KSDS",
                        "recfm":"V",
                        "lrecl":16,
                        "key_offset":0,
                        "key_length":3
                    }),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "WRITELEN".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("WL00".into(), "WRITELEN".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "WRITELN".into(),
                    map: "WRITELN".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("write-file-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "WL00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "WL00",
                24,
                80,
                "write-file-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "WRITELEN", 2)
            .unwrap();
        assert!(matches!(
            server.dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
                member: None,
                key: Some(b"003".to_vec()),
                max_records: 1,
                control: Default::default(),
            }),
            Ok(DatasetResult::Records { ref records, .. }) if records == &[b"003AB".to_vec()]
        ));
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Write)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_send_map_length_selects_the_bounded_from_prefix() {
        let artifact = published_source_fixture(
            "SENDLEN",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. SENDLEN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 OUTPUT-X PIC X(8) VALUE 'ABCDEFGH'.\n01 LENGTH-X PIC S9(4) COMP VALUE 4.\nPROCEDURE DIVISION.\nEXEC CICS SEND MAP('SHORT') MAPSET('LENGTHS') FROM(OUTPUT-X) LENGTH(LENGTH-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "SENDLEN".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("SL00".into(), "SENDLEN".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "LENGTHS".into(),
                    map: "SHORT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: vec![mainframe_env_cics::BmsFieldDefinition {
                        name: "VALUE".into(),
                        row: 1,
                        column: 1,
                        length: 4,
                        initial: Vec::new(),
                        color: None,
                        highlight: None,
                        protected: false,
                        secret: false,
                        fset: false,
                        justify_right: false,
                        fill_zero: false,
                        output_offset: Some(0),
                        attribute_offset: None,
                    }],
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("send-map-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "SL00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "SL00",
                24,
                80,
                "send-map-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "SENDLEN", 2)
            .unwrap();
        let wire = server.cics.tn3270_screen(&session, &principal, 3).unwrap();
        assert!(wire.windows(4).any(|bytes| bytes == b"ABCD"));
        assert!(!wire.windows(4).any(|bytes| bytes == b"EFGH"));
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::SendMap)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_send_map_maponly_selects_map_defaults() {
        let artifact = published_source_fixture(
            "SENDDEF",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. SENDDEF.\nPROCEDURE DIVISION.\nEXEC CICS SEND MAP('WELCOME') MAPSET('DEFMAPS') MAPONLY END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "SENDDEF".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("SD00".into(), "SENDDEF".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DEFMAPS".into(),
                    map: "WELCOME".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: vec![mainframe_env_cics::BmsFieldDefinition {
                        name: "TITLE".into(),
                        row: 1,
                        column: 1,
                        length: 7,
                        initial: b"WELCOME".to_vec(),
                        color: None,
                        highlight: None,
                        protected: true,
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
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("send-map-maponly-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "SD00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "SD00",
                24,
                80,
                "send-map-maponly-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "SENDDEF", 2)
            .unwrap();
        let wire = server.cics.tn3270_screen(&session, &principal, 3).unwrap();
        assert!(wire.windows(7).any(|bytes| bytes == b"WELCOME"));
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 3)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.operation == CicsOperation::SendMap)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_send_map_dataonly_uses_symbolic_data_and_attributes() {
        let artifact = published_source_fixture(
            "SENDDAT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. SENDDAT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 OUTPUT-X.\n  05 VALUE-X PIC X VALUE 'A'.\n  05 ATTR-X PIC X VALUE X'C1'.\nPROCEDURE DIVISION.\nEXEC CICS SEND MAP('UPDATE') MAPSET('DATAMAP') FROM(OUTPUT-X) DATAONLY END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "SENDDAT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("SU00".into(), "SENDDAT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DATAMAP".into(),
                    map: "UPDATE".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: vec![mainframe_env_cics::BmsFieldDefinition {
                        name: "VALUE".into(),
                        row: 1,
                        column: 1,
                        length: 1,
                        initial: b"Z".to_vec(),
                        color: None,
                        highlight: None,
                        protected: true,
                        secret: false,
                        fset: false,
                        justify_right: false,
                        fill_zero: false,
                        output_offset: Some(0),
                        attribute_offset: Some(1),
                    }],
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("send-map-dataonly-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "SU00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "SU00",
                24,
                80,
                "send-map-dataonly-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "SENDDAT", 2)
            .unwrap();
        let wire = server.cics.tn3270_screen(&session, &principal, 3).unwrap();
        assert!(wire.windows(3).any(|bytes| bytes == [0x1d, 0x00, b'A']));
        assert!(!wire.contains(&b'Z'));
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::SendMap)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_send_text_length_selects_the_bounded_from_prefix() {
        let artifact = published_source_fixture(
            "SENDTXT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. SENDTXT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 OUTPUT-X PIC X(10) VALUE 'HELLOWORLD'.\n01 LENGTH-X PIC S9(4) COMP VALUE 5.\nPROCEDURE DIVISION.\nEXEC CICS SEND TEXT FROM(OUTPUT-X) LENGTH(LENGTH-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "SENDTXT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("ST00".into(), "SENDTXT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "SENDTXT".into(),
                    map: "SENDTXT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("send-text-length-selected", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "ST00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "ST00",
                24,
                80,
                "send-text-length-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "SENDTXT", 2)
            .unwrap();
        let screen = server
            .cics
            .terminal_snapshot(&session, &principal, 3)
            .unwrap()
            .screen;
        assert_eq!(screen, b"HELLO");
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 3)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::SendText)
                .count(),
            1
        );
    }

    #[test]
    fn compiled_start_without_data_queues_the_target_without_a_false_payload() {
        let artifact = published_source_fixture(
            "NODATA",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. NODATA.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') REQID('NODATA01') AFTER SECONDS(0) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "NODATA".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([
                    ("ND00".into(), "NODATA".into()),
                    ("NX00".into(), "NODATA".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "NODATA".into(),
                    map: "NODATA".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("start-no-data", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "ND00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "ND00",
                24,
                80,
                "start-no-data-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "NODATA", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "NODATA", 2)
            .unwrap();
        assert_eq!(
            server
                .store
                .get_work("cics-start:NODATA01")
                .unwrap()
                .unwrap()
                .state,
            WorkState::Queued
        );
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap()
                .into_iter()
                .find(|entry| entry.operation == CicsOperation::Start)
                .map(|entry| (entry.outcome, entry.response, entry.response2)),
            Some(("NORMAL".into(), 0, 0))
        );
    }

    #[test]
    fn compiled_start_nocheck_queues_generated_identity_without_setting_eibreqid() {
        let artifact = published_source_fixture(
            "STARTNC",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. STARTNC.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'PAYLOAD'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') FROM(DATA-X) INTERVAL(0) NOCHECK RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "STARTNC".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([
                    ("SN00".into(), "STARTNC".into()),
                    ("NX00".into(), "STARTNC".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "STARTNC".into(),
                    map: "STARTNC".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("start-nocheck", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "SN00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "SN00",
                24,
                80,
                "start-nocheck-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "STARTNC", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "STARTNC", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("EIBREQID").unwrap().bytes(), &[0; 8]);
        let work = server
            .claim_jes_work("start-nocheck-worker")
            .unwrap()
            .unwrap();
        assert_eq!(work.required_generation, CICS_START_WORK_GENERATION);
        assert_eq!(work.payload.len(), 8);
        assert!(work.payload.iter().all(u8::is_ascii_hexdigit));
        assert_eq!(
            work.work_id,
            format!("cics-start:{}", String::from_utf8_lossy(&work.payload))
        );
    }

    #[test]
    fn compiled_start_userid_rejects_unknown_and_revoked_principals_before_surrogate() {
        let artifact = published_source_fixture(
            "USERSTAT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. USERSTAT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'AS-USER'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') REQID('MISSUSR1') FROM(DATA-X) INTERVAL(0) USERID('MISSING') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS START TRANSID('NX00') REQID('REVKUSR1') FROM(DATA-X) INTERVAL(0) USERID('REVOKED') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .bootstrap_identity("REVOKED", b"REVOKEDPASS")
            .unwrap();
        server
            .racf
            .set_user_state("REVOKED", false, true, false)
            .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "USERSTAT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([
                    ("UV00".into(), "USERSTAT".into()),
                    ("NX00".into(), "USERSTAT".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "USERSTAT".into(),
                    map: "USERSTAT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("start-user-validation", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "UV00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "UV00",
                24,
                80,
                "start-user-validation-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "USERSTAT", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "USERSTAT", 2)
            .unwrap();

        let start_outcomes = server
            .cics
            .terminal_run_trace(&session, &principal, 2)
            .unwrap()
            .into_iter()
            .filter(|entry| entry.operation == CicsOperation::Start)
            .map(|entry| (entry.outcome, entry.response, entry.response2))
            .collect::<Vec<_>>();
        assert_eq!(
            start_outcomes,
            vec![("USERIDERR".into(), 69, 8), ("USERIDERR".into(), 69, 19)]
        );
        for request_id in ["MISSUSR1", "REVKUSR1"] {
            assert!(
                server
                    .store
                    .get_provider_state("cics-interval-start-v1", request_id)
                    .unwrap()
                    .is_none()
            );
            assert!(
                server
                    .store
                    .get_work(&format!("cics-start:{request_id}"))
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
    }

    #[test]
    fn compiled_task_end_commits_protected_start_without_explicit_syncpoint() {
        let artifact = published_source_fixture(
            "ENDCOMMIT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. ENDCOMMIT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'TASK-END'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') REQID('ENDCMT01') FROM(DATA-X) INTERVAL(0) PROTECT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "ENDCOMMIT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([
                    ("TE00".into(), "ENDCOMMIT".into()),
                    ("NX00".into(), "ENDCOMMIT".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "ENDCMT".into(),
                    map: "ENDCMT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("implicit-task-end", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "TE00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "TE00",
                24,
                80,
                "implicit-task-end-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        let exchange = server
            .begin_online_exchange(&session, "ENDCOMMIT", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "ENDCOMMIT", 2)
            .unwrap();

        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert_eq!(
            server
                .store
                .get_work("cics-start:ENDCMT01")
                .unwrap()
                .unwrap()
                .state,
            WorkState::Queued
        );
        assert_eq!(
            server.cics.terminal_run_trace(&session, &principal, 2),
            Err(HostProblem::NotFound),
            "normal completion removed the volatile run after committing protected work"
        );

        // Recreate the durable-terminal crash gap: the coordinator has
        // committed Completed, while the exchange and protected START undo
        // rows still await product/CICS cleanup.
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: session.as_str().into(),
                    version: exchange.version,
                    payload: encode_online_exchange(&exchange).unwrap(),
                },
                None,
            )
            .unwrap();
        server
            .cics
            .restore_terminal_run(
                context.invocation.clone(),
                &session,
                &context.transaction,
                context.commarea.clone(),
                3,
            )
            .unwrap();
        stage_protected_start(&server, &context.invocation, "TE00", "NX00", "RECVEND1", 99);
        assert!(
            server
                .store
                .get_work("cics-start:RECVEND1")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            server
                .recover_terminal_online_exchange(&session, &principal, &exchange, 3)
                .unwrap(),
            Some(TerminalExchangeRecovery::Completed)
        );
        assert_eq!(
            server
                .store
                .get_work("cics-start:RECVEND1")
                .unwrap()
                .unwrap()
                .state,
            WorkState::Queued,
            "Completed recovery did not commit the protected START"
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn compiled_cancel_removes_unhonored_start_from_shared_worker_lane() {
        let artifact = published_source_fixture(
            "CANCELR",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. CANCELR.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(8) VALUE 'CANCELME'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('NX00') REQID('CAN0002') FROM(DATA-X) INTERVAL(100) PROTECT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS CANCEL REQID('CAN0002') TRANSID('NX00') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "CANCELR".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("CN00".into(), "CANCELR".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "CANCELR".into(),
                    map: "CANCELR".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("interval-cancel", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "CN00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "CN00",
                24,
                80,
                "interval-cancel-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "CANCELR", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "CANCELR", 2)
            .unwrap();
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 2)
            .unwrap();
        assert!(
            trace
                .iter()
                .any(|entry| entry.operation == CicsOperation::Start)
        );
        assert!(trace.iter().any(|entry| {
            entry.operation == CicsOperation::Cancel
                && entry.outcome == "NORMAL"
                && entry.response == 0
        }));
        assert!(trace.iter().any(|entry| {
            entry.operation == CicsOperation::Syncpoint
                && entry.outcome == "NORMAL"
                && entry.response == 0
        }));
        let work = server
            .store
            .get_work("cics-start:CAN0002")
            .unwrap()
            .unwrap();
        assert_eq!(work.state, WorkState::Cancelled);
        assert!(work.cancellation_requested);
        assert!(
            server
                .claim_jes_work("cancel-selected-worker")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn compiled_zero_delay_crosses_durable_coordinator_without_suspending() {
        let artifact = published_source_fixture(
            "DELAY0",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELAY0.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DELAY RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS DELAY INTERVAL(0) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELAY0".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DL00".into(), "DELAY0".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELAY0".into(),
                    map: "DELAY0".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("interval-delay-zero", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DL00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation,
                &session,
                "DL00",
                24,
                80,
                "interval-delay-zero-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "DELAY0", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DELAY0", 2)
            .unwrap();
        let trace = server
            .cics
            .terminal_run_trace(&session, &principal, 2)
            .unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|entry| {
                    entry.operation == CicsOperation::Delay
                        && entry.outcome == "NORMAL"
                        && entry.response == 0
                })
                .count(),
            2
        );
    }

    #[test]
    fn compiled_dynamic_for_delay_suspends_promotes_and_resumes() {
        let artifact = published_source_fixture(
            "DELAY1",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELAY1.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DONE-X PIC X VALUE '0'.\n01 TIME-X PIC S9(9) COMP VALUE 1.\n01 MS-X PIC S9(9) COMP VALUE 250.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DELAY FOR SECONDS(TIME-X) MILLISECS(MS-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE '1' TO DONE-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let clock = Arc::new(ManualJesClock::new(100));
        let platform_store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let server =
            ProductServer::open_with_clock(config(), platform_store, clock.clone()).unwrap();
        let execution_clock = clock.clone();
        server
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELAY1".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DL01".into(), "DELAY1".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELAY1".into(),
                    map: "DELAY1".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("interval-delay-positive", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DL01", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DL01",
                24,
                80,
                "interval-delay-positive-csrf",
                100,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 100)
            .unwrap();
        server
            .begin_online_exchange(&session, "DELAY1", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DELAY1", 100)
            .unwrap();
        assert!(
            server
                .claim_jes_work("delay-product-worker")
                .unwrap()
                .is_none()
        );
        clock.advance(1_250);
        let work = server
            .claim_jes_work("delay-product-worker")
            .unwrap()
            .unwrap();
        assert_eq!(work.required_generation, CICS_DELAY_WORK_GENERATION);
        let outcome = server.process_claimed_jes_work(&work).unwrap();
        server.finish_claimed_jes_work(&work, Ok(outcome)).unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("DONE-X").unwrap().bytes(), b"1");
        // Restoring the terminal run starts a fresh in-memory trace segment; the
        // durable provider tests cover the preceding suspended invocation.
        assert_eq!(
            server
                .cics
                .terminal_run_trace(&session, &principal, 1_350)
                .unwrap()
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Delay)
                .count(),
            1
        );
    }

    #[test]
    fn sqlite_restart_promotes_delay_and_resumes_the_durable_exchange() {
        let artifact = published_source_fixture(
            "DELAYR",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELAYR.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DONE-X PIC X VALUE '0'.\n01 TIME-X PIC S9(9) COMP VALUE 1.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DELAY FOR SECONDS(TIME-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE '1' TO DONE-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-delay-restart-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = directory.join("artifacts");
        server_config.timeout_millis = 10_000;

        let first_clock = Arc::new(ManualJesClock::new(100));
        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let first = ProductServer::open_with_clock(
            server_config.clone(),
            first_platform,
            first_clock.clone(),
        )
        .unwrap();
        let execution_clock = first_clock.clone();
        first
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        first.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        first
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELAYR".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DL0R".into(), "DELAYR".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELAYR".into(),
                    map: "DELAYR".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("interval-delay-restart", 64).unwrap();
        let invocation = first
            .cics_invocation("IBMUSER", "DL0R", Some(artifact_ref))
            .unwrap();
        first
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DL0R",
                24,
                80,
                "interval-delay-restart-csrf",
                100,
                10_000,
            )
            .unwrap();
        let context = first
            .cics
            .terminal_execution(&session, &principal, 100)
            .unwrap();
        first
            .begin_online_exchange(&session, "DELAYR", &context)
            .unwrap();
        first
            .run_online_exchange(&session, &principal, "DELAYR", 100)
            .unwrap();
        assert!(first.claim_jes_work("before-restart").unwrap().is_none());
        drop((first, first_store));

        let second_clock = Arc::new(ManualJesClock::new(1_100));
        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let second =
            ProductServer::open_with_clock(server_config, second_platform, second_clock.clone())
                .unwrap();
        let execution_clock = second_clock;
        second
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let work_id = second
            .run_jes_worker_once("after-restart")
            .unwrap()
            .expect("the restarted worker must claim the due DELAY");
        assert!(work_id.starts_with("cics-delay:"));
        assert_eq!(
            second_store.get_work(&work_id).unwrap().unwrap().state,
            WorkState::Completed
        );
        let continuation = second
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("DONE-X").unwrap().bytes(), b"1");

        drop((second, second_store));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compiled_dynamic_packed_time_suspends_promotes_and_resumes() {
        let artifact = published_source_fixture(
            "DELAYT",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELAYT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DONE-X PIC X VALUE '0'.\n01 TIME-X PIC S9(6) COMP-3 VALUE 995959.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DELAY TIME(TIME-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE '1' TO DONE-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let clock = Arc::new(ManualJesClock::new(100));
        let platform_store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let mut settings = config();
        settings.timeout_millis = 500_000_000;
        let server =
            ProductServer::open_with_clock(settings, platform_store, clock.clone()).unwrap();
        let execution_clock = clock.clone();
        server
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "DELAYT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("DL0T".into(), "DELAYT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "DELAYT".into(),
                    map: "DELAYT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let session = SessionId::new("interval-delay-packed-time", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "DL0T", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "DL0T",
                24,
                80,
                "interval-delay-packed-time-csrf",
                100,
                500_000_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 100)
            .unwrap();
        server
            .begin_online_exchange(&session, "DELAYT", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "DELAYT", 100)
            .unwrap();

        let mut claimed = None;
        for _ in 0..=100 {
            clock.advance(3_600_000);
            claimed = server.claim_jes_work("packed-time-product-worker").unwrap();
            if claimed.is_some() {
                break;
            }
        }
        let work = claimed.expect("packed TIME work must become due within 100 hours");
        assert_eq!(work.required_generation, CICS_DELAY_WORK_GENERATION);
        let outcome = server.process_claimed_jes_work(&work).unwrap();
        server.finish_claimed_jes_work(&work, Ok(outcome)).unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("DONE-X").unwrap().bytes(), b"1");
    }

    #[test]
    fn compiled_named_delay_is_cancelled_by_another_task_and_resumes() {
        let delay_artifact = published_source_fixture(
            "DELAY2",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. DELAY2.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DONE-X PIC X VALUE '0'.\n01 TIME-X PIC S9(6) COMP-3 VALUE 1.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS DELAY INTERVAL(TIME-X) REQID('WAIT0001') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE '1' TO DONE-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let cancel_artifact = published_source_fixture(
            "CANCEL2",
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. CANCEL2.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS CANCEL REQID('WAIT0001') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nSTOP RUN.\n",
        );
        let delay_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(delay_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let cancel_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(cancel_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let clock = Arc::new(ManualJesClock::new(100));
        let platform_store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let server =
            ProductServer::open_with_clock(config(), platform_store, clock.clone()).unwrap();
        let execution_clock = clock.clone();
        server
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "DELAY2".into(),
                        artifact: delay_ref.clone(),
                        payload: delay_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(delay_artifact.manifest().clone()),
                        semantic_identity: delay_artifact.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "CANCEL2".into(),
                        artifact: cancel_ref.clone(),
                        payload: cancel_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(cancel_artifact.manifest().clone()),
                        semantic_identity: cancel_artifact.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([
                    ("DL02".into(), "DELAY2".into()),
                    ("CN02".into(), "CANCEL2".into()),
                ]),
                maps: vec![
                    BmsMapDefinition {
                        mapset: "DELAY2".into(),
                        map: "DELAY2".into(),
                        line: 1,
                        column: 1,
                        rows: 24,
                        columns: 80,
                        fields: Vec::new(),
                    },
                    BmsMapDefinition {
                        mapset: "CANCEL2".into(),
                        map: "CANCEL2".into(),
                        line: 1,
                        column: 1,
                        rows: 24,
                        columns: 80,
                        fields: Vec::new(),
                    },
                ],
            })
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();

        let delay_session = SessionId::new("interval-delay-named", 64).unwrap();
        let delay_invocation = server
            .cics_invocation("IBMUSER", "DL02", Some(delay_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                delay_invocation.clone(),
                &delay_session,
                "DL02",
                24,
                80,
                "interval-delay-named-csrf",
                100,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&delay_session, &principal, 100)
            .unwrap();
        server
            .begin_online_exchange(&delay_session, "DELAY2", &context)
            .unwrap();
        server
            .run_online_exchange(&delay_session, &principal, "DELAY2", 100)
            .unwrap();

        let cancel_session = SessionId::new("interval-delay-canceller", 64).unwrap();
        let cancel_invocation = server
            .cics_invocation("IBMUSER", "CN02", Some(cancel_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                cancel_invocation,
                &cancel_session,
                "CN02",
                24,
                80,
                "interval-delay-canceller-csrf",
                100,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&cancel_session, &principal, 100)
            .unwrap();
        server
            .begin_online_exchange(&cancel_session, "CANCEL2", &context)
            .unwrap();
        server
            .run_online_exchange(&cancel_session, &principal, "CANCEL2", 100)
            .unwrap();
        assert!(
            server
                .claim_jes_work("cancelled-delay-worker")
                .unwrap()
                .is_none()
        );

        server
            .run_online_exchange(&delay_session, &principal, "DELAY2", 100)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&delay_session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            delay_artifact.payload(),
            delay_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("DONE-X").unwrap().bytes(), b"1");
    }

    #[test]
    fn online_wait_event_posts_and_resumes_the_compiled_selected_route() {
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. WAITEVT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 ECB-X PIC S9(9) COMP VALUE 0.\n01 ECB-PTR POINTER-32.\n01 WAIT-FN PIC X(2).\n01 DONE-X PIC X VALUE '0'.\nPROCEDURE DIVISION.\nSET ECB-PTR TO ADDRESS OF ECB-X.\nEXEC CICS WAIT EVENT ECADDR(ECB-PTR) NAME('EVENT001') END-EXEC.\nMOVE EIBFN TO WAIT-FN.\nMOVE '1' TO DONE-X.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let artifact = published_source_fixture("WAITEVT", std::str::from_utf8(source).unwrap());
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
                    name: "WAITEVT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("WE00".into(), "WAITEVT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "WAITEVT".into(),
                    map: "WAITEVT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("typed-wait-event", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "WE00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "WE00",
                24,
                80,
                "typed-wait-event-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "WAITEVT", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "WAITEVT", 2)
            .unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
        server
            .cics
            .post_task_event(&session, &principal, 3, 0, CicsEventPostMode::Standard)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "WAITEVT", 3)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("WAIT-FN").unwrap().bytes(), &[0x12, 0x02]);
        assert_eq!(restored.variable("DONE-X").unwrap().bytes(), b"1");
        assert_eq!(
            restored.variable("ECB-X").unwrap().bytes(),
            &[0x40, 0, 0, 0]
        );
        assert_eq!(restored.variable("EIBFN").unwrap().bytes(), &[0x12, 0x08]);
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 8)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .count(),
            3
        );
        server
            .run_online_exchange(&session, &principal, "WAITEVT", 4)
            .unwrap();
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_none()
        );
        assert!(
            server
                .store
                .list_provider_state("cics-task-wait-v1", 2)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn online_task_scheduling_yields_once_and_retains_changed_priority() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. SCHEDULE.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PRIORITY-X PIC S9(4) COMP VALUE 200.\n01 OBSERVED-PRIORITY PIC S9(4) COMP.\n01 ABCODE-X PIC X(4) VALUE 'ZZZZ'.\n01 ABDUMP-X PIC X VALUE 'Z'.\n01 ABOFFSET-X PIC S9(9) COMP VALUE 1.\n01 ABPROGRAM-X PIC X(8) VALUE ALL 'Z'.\n01 ALTERNATE-HEIGHT-X PIC S9(4) COMP VALUE 1.\n01 ALTERNATE-WIDTH-X PIC S9(4) COMP VALUE 1.\n01 APPLICATION-X PIC X(64).\n01 APPL-X PIC X(8).\n01 ASRA-PSW-X PIC X(8) VALUE ALL 'Z'.\n01 ASRA-PSW16-X PIC X(16) VALUE ALL 'Z'.\n01 ASRA-REGS-X PIC X(64) VALUE ALL 'Z'.\n01 ASRA-REGS64-X PIC X(128) VALUE ALL 'Z'.\n01 BRIDGE-X PIC X(4) VALUE 'ZZZZ'.\n01 CAPABILITY-X PIC X VALUE 'Z'.\n01 CHANNEL-X PIC X(16).\n01 CMDSEC-X PIC X.\n01 CWA-LENGTH-X PIC S9(4) COMP.\n01 DEFAULT-HEIGHT-X PIC S9(4) COMP VALUE 1.\n01 DEFAULT-WIDTH-X PIC S9(4) COMP VALUE 1.\n01 DS3270-X PIC X VALUE 'Z'.\n01 DSSCS-X PIC X VALUE 'Z'.\n01 FCI-X PIC X VALUE 'Z'.\n01 INITPARM-X PIC X(60) VALUE ALL 'Z'.\n01 INITPARM-LENGTH-X PIC S9(4) COMP.\n01 LINK-LEVEL-X PIC S9(4) COMP.\n01 MAJOR-X PIC S9(9) COMP.\n01 MICRO-X PIC S9(9) COMP.\n01 MINOR-X PIC S9(9) COMP.\n01 NEXT-TRANS-X PIC X(4) VALUE 'ZZZZ'.\n01 OPERATION-X PIC X(64).\n01 OPERKEYS-X PIC X(8).\n01 OPSECURITY-X PIC X(3).\n01 PARTITION-SET-X PIC X(6) VALUE 'ZZZZZZ'.\n01 PLATFORM-X PIC X(64).\n01 RESTART-X PIC X.\n01 RESSEC-X PIC X.\n01 SCREEN-HEIGHT-X PIC S9(4) COMP VALUE 1.\n01 SCREEN-WIDTH-X PIC S9(4) COMP VALUE 1.\n01 SYS-X PIC X(4).\n01 TCTUA-LENGTH-X PIC S9(4) COMP.\n01 TWA-LENGTH-X PIC S9(4) COMP.\n01 USER-X PIC X(8).\n01 ASSIGN-FN PIC X(2).\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS CHANGE TASK PRIORITY(PRIORITY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN APPLICATION(APPLICATION-X) APPLID(APPL-X) BRIDGE(BRIDGE-X) CHANNEL(CHANNEL-X) MAJORVERSION(MAJOR-X) MICROVERSION(MICRO-X) MINORVERSION(MINOR-X) OPERATION(OPERATION-X) PLATFORM(PLATFORM-X) SCRNHT(SCREEN-HEIGHT-X) SCRNWD(SCREEN-WIDTH-X) SYSID(SYS-X) TASKPRIORITY(OBSERVED-PRIORITY) USERID(USER-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN ALTSCRNHT(ALTERNATE-HEIGHT-X) ALTSCRNWD(ALTERNATE-WIDTH-X) CWALENG(CWA-LENGTH-X) DEFSCRNHT(DEFAULT-HEIGHT-X) DEFSCRNWD(DEFAULT-WIDTH-X) DS3270(DS3270-X) DSSCS(DSSCS-X) FCI(FCI-X) LINKLEVEL(LINK-LEVEL-X) OPERKEYS(OPERKEYS-X) PARTNSET(PARTITION-SET-X) RESTART(RESTART-X) TWALENG(TWA-LENGTH-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN APLKYBD(CAPABILITY-X) APLTEXT(CAPABILITY-X) BTRANS(CAPABILITY-X) COLOR(CAPABILITY-X) EWASUPP(CAPABILITY-X) EXTDS(CAPABILITY-X) GMMI(CAPABILITY-X) HILIGHT(CAPABILITY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN KATAKANA(CAPABILITY-X) MSRCONTROL(CAPABILITY-X) OUTLINE(CAPABILITY-X) PARTNS(CAPABILITY-X) PS(CAPABILITY-X) SOSI(CAPABILITY-X) TEXTKYBD(CAPABILITY-X) TEXTPRINT(CAPABILITY-X) VALIDATION(CAPABILITY-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN CMDSEC(CMDSEC-X) OPSECURITY(OPSECURITY-X) RESSEC(RESSEC-X) TCTUALENG(TCTUA-LENGTH-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN INITPARM(INITPARM-X) INITPARMLEN(INITPARM-LENGTH-X) NEXTTRANSID(NEXT-TRANS-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ASSIGN ABCODE(ABCODE-X) ABDUMP(ABDUMP-X) ABOFFSET(ABOFFSET-X) ABPROGRAM(ABPROGRAM-X) ASRAPSW(ASRA-PSW-X) ASRAPSW16(ASRA-PSW16-X) ASRAREGS(ASRA-REGS-X) ASRAREGS64(ASRA-REGS64-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE EIBFN TO ASSIGN-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let source = std::str::from_utf8(source)
            .unwrap()
            .replace(
                "TEXTPRINT(CAPABILITY-X) VALIDATION(CAPABILITY-X)",
                "TEXTPRINT(CAPABILITY-X) UNATTEND(CAPABILITY-X) VALIDATION(CAPABILITY-X)",
            )
            .replace(
                "ABPROGRAM(ABPROGRAM-X) ASRAPSW(ASRA-PSW-X)",
                "ABPROGRAM(ABPROGRAM-X) ASRAINTRPT(ASRA-PSW-X) ASRAPSW(ASRA-PSW-X) ERRORMSG(ERROR-MSG-X) ERRORMSGLEN(ERROR-MSG-LENGTH-X) ORGABCODE(ABCODE-X)",
            )
            .replace(
                "01 FCI-X PIC X VALUE 'Z'.",
                "01 ERROR-MSG-X PIC X(500) VALUE ALL 'Z'.\n01 ERROR-MSG-LENGTH-X PIC S9(4) COMP VALUE 1.\n01 FCI-X PIC X VALUE 'Z'.",
            )
            .replace(
                "01 LINK-LEVEL-X PIC S9(4) COMP.",
                "01 LINK-LEVEL-X PIC S9(4) COMP.\n01 LOCAL-CCSID-X PIC S9(9) COMP.\n01 MAP-COLUMN-X PIC S9(4) COMP VALUE 1.\n01 MAP-HEIGHT-X PIC S9(4) COMP VALUE 1.\n01 MAP-LINE-X PIC S9(4) COMP VALUE 1.\n01 MAP-WIDTH-X PIC S9(4) COMP VALUE 1.",
            )
            .replace(
                "FCI(FCI-X) LINKLEVEL(LINK-LEVEL-X)",
                "FCI(FCI-X) LINKLEVEL(LINK-LEVEL-X) LOCALCCSID(LOCAL-CCSID-X)",
            )
            .replace(
                "01 ASSIGN-FN PIC X(2).",
                "01 ASSIGN-FN PIC X(2).\n01 PURGE-FN PIC X(2).",
            )
            .replace(
                "PROCEDURE DIVISION.\nEXEC CICS CHANGE TASK",
                "PROCEDURE DIVISION.\nEXEC CICS ASSIGN INPARTN(INPUT-PARTITION-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE RESP-X TO INPUT-PARTITION-RESP-X.\nMOVE RESP2-X TO INPUT-PARTITION-RESP2-X.\nEXEC CICS SEND MAP('SCHEDUL') MAPSET('SCHEDUL') END-EXEC.\nEXEC CICS PURGE MESSAGE END-EXEC.\nMOVE EIBFN TO PURGE-FN.\nEXEC CICS CHANGE TASK",
            )
            .replace(
                "HILIGHT(CAPABILITY-X) RESP(RESP-X)",
                "HILIGHT(CAPABILITY-X) MAPCOLUMN(MAP-COLUMN-X) MAPHEIGHT(MAP-HEIGHT-X) MAPLINE(MAP-LINE-X) MAPWIDTH(MAP-WIDTH-X) RESP(RESP-X)",
            )
            .replace(
                "01 CWA-LENGTH-X PIC S9(4) COMP.",
                "01 BMS-DESTCOUNT-X PIC S9(4) COMP VALUE 7.\n01 BMS-LDCMNEM-X PIC X(2) VALUE 'ZZ'.\n01 BMS-LDCNUM-X PIC X VALUE 'Z'.\n01 BMS-PAGENUM-X PIC S9(4) COMP VALUE 7.\n01 BMS-PARTNPAGE-X PIC X(2) VALUE 'ZZ'.\n01 BMS-RESP-X PIC S9(9) COMP.\n01 BMS-RESP2-X PIC S9(9) COMP.\n01 CWA-LENGTH-X PIC S9(4) COMP.",
            )
            .replace(
                "EXEC CICS ASSIGN ABCODE(ABCODE-X)",
                "EXEC CICS ASSIGN DESTCOUNT(BMS-DESTCOUNT-X) LDCMNEM(BMS-LDCMNEM-X) LDCNUM(BMS-LDCNUM-X) PAGENUM(BMS-PAGENUM-X) PARTNPAGE(BMS-PARTNPAGE-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE RESP-X TO BMS-RESP-X.\nMOVE RESP2-X TO BMS-RESP2-X.\nEXEC CICS ASSIGN ABCODE(ABCODE-X)",
            )
            .replace(
                "01 INITPARM-X PIC X(60) VALUE ALL 'Z'.",
                "01 FACILITY-X PIC X(4) VALUE 'ZZZZ'.\n01 NETWORK-NAME-X PIC X(8) VALUE ALL 'Z'.\n01 TN-ADDRESS-X PIC X(39) VALUE ALL 'Z'.\n01 INVOKING-PROGRAM-X PIC X(8) VALUE ALL 'Z'.\n01 RETURN-PROGRAM-X PIC X(8) VALUE ALL 'Z'.\n01 INPUT-PARTITION-X PIC X(2) VALUE 'ZZ'.\n01 INPUT-PARTITION-RESP-X PIC S9(9) COMP.\n01 INPUT-PARTITION-RESP2-X PIC S9(9) COMP.\n01 INITPARM-X PIC X(60) VALUE ALL 'Z'.",
            )
            .replace(
                "EXEC CICS ASSIGN INITPARM(INITPARM-X)",
                "EXEC CICS ASSIGN FACILITY(FACILITY-X) NETNAME(NETWORK-NAME-X) TNADDR(TN-ADDRESS-X) INVOKINGPROG(INVOKING-PROGRAM-X) RETURNPROG(RETURN-PROGRAM-X) INITPARM(INITPARM-X)",
            )
            .replace(
                "01 PRIORITY-X PIC S9(4) COMP VALUE 200.",
                "01 PRIORITY-X PIC S9(4) COMP VALUE 200.\n01 TERMINAL-PRIORITY-X PIC S9(4) COMP VALUE 7.",
            )
            .replace(
                "EXEC CICS ASSIGN CMDSEC(CMDSEC-X)",
                "EXEC CICS ASSIGN TERMPRIORITY(TERMINAL-PRIORITY-X) CMDSEC(CMDSEC-X)",
            )
            .replace(
                "01 CMDSEC-X PIC X.",
                "01 CMDSEC-X PIC X.\n01 LANGUAGE-X PIC X(3) VALUE 'ZZZ'.",
            )
            .replace(
                "TERMPRIORITY(TERMINAL-PRIORITY-X) CMDSEC(CMDSEC-X)",
                "LANGINUSE(LANGUAGE-X) TERMPRIORITY(TERMINAL-PRIORITY-X) CMDSEC(CMDSEC-X)",
            )
            .replace(
                "01 INITPARM-LENGTH-X PIC S9(4) COMP.",
                "01 INITPARM-LENGTH-X PIC S9(4) COMP.\n01 INPUT-LENGTH-X PIC S9(4) COMP VALUE 7.",
            )
            .replace(
                "LANGINUSE(LANGUAGE-X) TERMPRIORITY",
                "INPUTMSGLEN(INPUT-LENGTH-X) LANGINUSE(LANGUAGE-X) TERMPRIORITY",
            );
        let path = LogicalPath::new("SCHEDULE.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "SCHEDULE.cbl",
                    source.as_bytes().to_vec(),
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
            panic!("task scheduling fixture did not publish");
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
                    name: "SCHEDULE".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("SC00".into(), "SCHEDULE".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "SCHEDUL".into(),
                    map: "SCHEDUL".into(),
                    line: 3,
                    column: 4,
                    rows: 10,
                    columns: 20,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("task-scheduling", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "SC00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "SC00",
                24,
                80,
                "task-scheduling-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "SCHEDULE", &context)
            .unwrap();

        server
            .run_online_exchange(&session, &principal, "SCHEDULE", 2)
            .unwrap();
        let first = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        assert_eq!(first.priority, Some(200));
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&first.checkpoint).unwrap();
        assert_eq!(restored.variable("EIBFN").unwrap().bytes(), &[0x5e, 0x06]);
        let current_record = server
            .store
            .get_provider_state("online-machine-continuation", session.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(&current_record.payload[..5], b"MEOM4");
        let mut legacy3 = current_record.clone();
        legacy3.payload.truncate(legacy3.payload.len() - 4);
        legacy3.payload[..5].copy_from_slice(b"MEOM3");
        let decoded_legacy3 = decode_online_machine_continuation(&legacy3).unwrap();
        assert_eq!(decoded_legacy3.priority, Some(200));
        assert!(decoded_legacy3.transfer.is_none());
        let mut legacy = legacy3;
        let mut at = 5usize;
        for _ in 0..3 {
            let length = usize::try_from(u32::from_be_bytes(
                legacy.payload[at..at + 4].try_into().unwrap(),
            ))
            .unwrap();
            at += 4 + length;
        }
        assert_eq!(&legacy.payload[at..at + 5], &[0, 0, 0, 1, 200]);
        legacy.payload.drain(at..at + 5);
        legacy.payload[..5].copy_from_slice(b"MEOM2");
        let decoded_legacy = decode_online_machine_continuation(&legacy).unwrap();
        assert_eq!(decoded_legacy.priority, None);
        assert_eq!(decoded_legacy.checkpoint, first.checkpoint);
        let platform_store = server.store.clone();
        drop(server);
        let server = ProductServer::open(
            config(),
            platform_store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );

        server
            .run_online_exchange(&session, &principal, "SCHEDULE", 3)
            .unwrap();
        let second = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        assert_eq!(second.priority, Some(200));
        assert!(second.version > first.version);
        restored.restore_checkpoint(&second.checkpoint).unwrap();
        assert_eq!(restored.variable("EIBFN").unwrap().bytes(), &[0x12, 0x08]);
        assert_eq!(
            restored.variable("PURGE-FN").unwrap().bytes(),
            &[0x18, 0x0a]
        );
        assert_eq!(
            restored.variable("ASSIGN-FN").unwrap().bytes(),
            &[0x02, 0x08]
        );
        assert_eq!(
            restored.variable("OBSERVED-PRIORITY").unwrap().bytes(),
            &[0, 200]
        );
        assert_eq!(
            restored.variable("TERMINAL-PRIORITY-X").unwrap().bytes(),
            &[0, 0]
        );
        assert_eq!(restored.variable("ABCODE-X").unwrap().bytes(), b"    ");
        assert_eq!(restored.variable("ABDUMP-X").unwrap().bytes(), &[0]);
        assert_eq!(restored.variable("ABOFFSET-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("ABPROGRAM-X").unwrap().bytes(), &[0; 8]);
        assert_eq!(
            restored.variable("ALTERNATE-HEIGHT-X").unwrap().bytes(),
            &[0, 24]
        );
        assert_eq!(
            restored.variable("ALTERNATE-WIDTH-X").unwrap().bytes(),
            &[0, 80]
        );
        assert_eq!(
            restored.variable("APPLICATION-X").unwrap().bytes(),
            &[b' '; 64]
        );
        assert_eq!(restored.variable("APPL-X").unwrap().bytes(), b"ME01    ");
        assert_eq!(restored.variable("ASRA-PSW-X").unwrap().bytes(), &[0; 8]);
        assert_eq!(restored.variable("ASRA-PSW16-X").unwrap().bytes(), &[0; 16]);
        assert_eq!(restored.variable("ASRA-REGS-X").unwrap().bytes(), &[0; 64]);
        assert_eq!(
            restored.variable("ASRA-REGS64-X").unwrap().bytes(),
            &[0; 128]
        );
        assert_eq!(restored.variable("BRIDGE-X").unwrap().bytes(), b"    ");
        assert_eq!(
            restored.variable("BMS-DESTCOUNT-X").unwrap().bytes(),
            &[0, 7]
        );
        assert_eq!(restored.variable("BMS-LDCMNEM-X").unwrap().bytes(), b"ZZ");
        assert_eq!(restored.variable("BMS-LDCNUM-X").unwrap().bytes(), b"Z");
        assert_eq!(restored.variable("BMS-PAGENUM-X").unwrap().bytes(), &[0, 7]);
        assert_eq!(restored.variable("BMS-PARTNPAGE-X").unwrap().bytes(), b"ZZ");
        assert_eq!(
            restored.variable("BMS-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("BMS-RESP2-X").unwrap().bytes(),
            &[0, 0, 0, 2]
        );
        assert_eq!(restored.variable("CAPABILITY-X").unwrap().bytes(), &[0]);
        assert_eq!(restored.variable("CHANNEL-X").unwrap().bytes(), &[b' '; 16]);
        assert_eq!(restored.variable("CMDSEC-X").unwrap().bytes(), b"X");
        assert_eq!(restored.variable("CWA-LENGTH-X").unwrap().bytes(), &[0; 2]);
        assert_eq!(
            restored.variable("DEFAULT-HEIGHT-X").unwrap().bytes(),
            &[0, 24]
        );
        assert_eq!(
            restored.variable("DEFAULT-WIDTH-X").unwrap().bytes(),
            &[0, 80]
        );
        assert_eq!(restored.variable("DS3270-X").unwrap().bytes(), &[0xff]);
        assert_eq!(restored.variable("DSSCS-X").unwrap().bytes(), &[0]);
        assert_eq!(restored.variable("ERROR-MSG-X").unwrap().bytes(), &[0; 500]);
        assert_eq!(
            restored.variable("ERROR-MSG-LENGTH-X").unwrap().bytes(),
            &[0; 2]
        );
        assert_eq!(restored.variable("FCI-X").unwrap().bytes(), &[1]);
        assert_eq!(restored.variable("FACILITY-X").unwrap().bytes(), b"T000");
        assert_eq!(
            restored.variable("INITPARM-X").unwrap().bytes(),
            &[b'Z'; 60]
        );
        assert_eq!(
            restored.variable("INITPARM-LENGTH-X").unwrap().bytes(),
            &[0; 2]
        );
        assert_eq!(
            restored.variable("INPUT-LENGTH-X").unwrap().bytes(),
            &[0; 2]
        );
        assert_eq!(
            restored.variable("INPUT-PARTITION-X").unwrap().bytes(),
            b"ZZ"
        );
        assert_eq!(
            restored.variable("INPUT-PARTITION-RESP-X").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored
                .variable("INPUT-PARTITION-RESP2-X")
                .unwrap()
                .bytes(),
            &[0, 0, 0, 2]
        );
        assert_eq!(restored.variable("LINK-LEVEL-X").unwrap().bytes(), &[0, 1]);
        assert_eq!(restored.variable("LANGUAGE-X").unwrap().bytes(), b"ENU");
        assert_eq!(
            restored.variable("LOCAL-CCSID-X").unwrap().bytes(),
            &[0, 0, 0, 37]
        );
        assert_eq!(restored.variable("MAP-COLUMN-X").unwrap().bytes(), &[0, 4]);
        assert_eq!(restored.variable("MAP-HEIGHT-X").unwrap().bytes(), &[0, 10]);
        assert_eq!(restored.variable("MAP-LINE-X").unwrap().bytes(), &[0, 3]);
        assert_eq!(restored.variable("MAP-WIDTH-X").unwrap().bytes(), &[0, 20]);
        assert_eq!(restored.variable("MAJOR-X").unwrap().bytes(), &[0xff; 4]);
        assert_eq!(restored.variable("MICRO-X").unwrap().bytes(), &[0xff; 4]);
        assert_eq!(restored.variable("MINOR-X").unwrap().bytes(), &[0xff; 4]);
        assert_eq!(restored.variable("NEXT-TRANS-X").unwrap().bytes(), b"    ");
        assert_eq!(
            restored.variable("NETWORK-NAME-X").unwrap().bytes(),
            b"T000    "
        );
        assert_eq!(
            restored.variable("OPERATION-X").unwrap().bytes(),
            &[b' '; 64]
        );
        assert_eq!(restored.variable("OPERKEYS-X").unwrap().bytes(), &[0; 8]);
        assert_eq!(restored.variable("OPSECURITY-X").unwrap().bytes(), &[0; 3]);
        assert_eq!(
            restored.variable("PARTITION-SET-X").unwrap().bytes(),
            b"      "
        );
        assert_eq!(
            restored.variable("PLATFORM-X").unwrap().bytes(),
            &[b' '; 64]
        );
        assert_eq!(restored.variable("RESTART-X").unwrap().bytes(), &[0]);
        assert_eq!(restored.variable("RESSEC-X").unwrap().bytes(), b"X");
        assert_eq!(
            restored.variable("INVOKING-PROGRAM-X").unwrap().bytes(),
            &[b' '; 8]
        );
        assert_eq!(
            restored.variable("RETURN-PROGRAM-X").unwrap().bytes(),
            &[b' '; 8]
        );
        assert_eq!(
            restored.variable("SCREEN-HEIGHT-X").unwrap().bytes(),
            &[0, 24]
        );
        assert_eq!(
            restored.variable("SCREEN-WIDTH-X").unwrap().bytes(),
            &[0, 80]
        );
        assert_eq!(restored.variable("SYS-X").unwrap().bytes(), b"S001");
        assert_eq!(
            restored.variable("TCTUA-LENGTH-X").unwrap().bytes(),
            &[0; 2]
        );
        assert_eq!(restored.variable("TWA-LENGTH-X").unwrap().bytes(), &[0; 2]);
        assert_eq!(
            restored.variable("TN-ADDRESS-X").unwrap().bytes(),
            &[b' '; 39]
        );
        assert_eq!(restored.variable("USER-X").unwrap().bytes(), b"IBMUSER ");
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0; 4]);
        server
            .run_online_exchange(&session, &principal, "SCHEDULE", 4)
            .unwrap();
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_none()
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let cics_audits = server
            .store
            .audit_records(&invocation.execution_id, 1, 32)
            .unwrap()
            .into_iter()
            .filter(|record| record.capability.as_str() == "host.cics.execute")
            .map(|record| (record.effect_sequence, record.decision))
            .collect::<Vec<_>>();
        assert_eq!(
            cics_audits,
            vec![
                (1, mainframe_env_execution_api::AuditDecision::Success),
                (2, mainframe_env_execution_api::AuditDecision::Success),
                (3, mainframe_env_execution_api::AuditDecision::Success),
                (4, mainframe_env_execution_api::AuditDecision::Success),
                (5, mainframe_env_execution_api::AuditDecision::Success),
                (6, mainframe_env_execution_api::AuditDecision::Success),
                (7, mainframe_env_execution_api::AuditDecision::Success),
                (8, mainframe_env_execution_api::AuditDecision::Success),
                (9, mainframe_env_execution_api::AuditDecision::Success),
                (10, mainframe_env_execution_api::AuditDecision::Success),
                (11, mainframe_env_execution_api::AuditDecision::Success),
                (12, mainframe_env_execution_api::AuditDecision::Success),
                (13, mainframe_env_execution_api::AuditDecision::Success),
            ]
        );
    }

    #[test]
    fn online_time_commands_update_packed_and_formatted_destinations() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ASKTIME.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 EIBDATE PIC S9(7) COMP-3.\n01 EIBTIME PIC S9(7) COMP-3.\n01 ABS-TIME PIC S9(15) COMP-3.\n01 DATE-OUT PIC X(10).\n01 TIME-OUT PIC X(8).\n01 MS-OUT PIC S9(9) COMP.\n01 ASKTIME-FN PIC X(2).\n01 ABSTIME-FN PIC X(2).\n01 FORMAT-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS ASKTIME END-EXEC.\nMOVE EIBFN TO ASKTIME-FN.\nEXEC CICS ASKTIME ABSTIME(ABS-TIME) END-EXEC.\nMOVE EIBFN TO ABSTIME-FN.\nEXEC CICS FORMATTIME ABSTIME(ABS-TIME) DATESEP('-') YYYYMMDD(DATE-OUT) TIMESEP(':') TIME(TIME-OUT) MILLISECONDS(MS-OUT) END-EXEC.\nMOVE EIBFN TO FORMAT-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("ASKTIME.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "ASKTIME.cbl",
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
            panic!("bare ASKTIME fixture did not publish");
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
                    name: "ASKTIME".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("AT00".into(), "ASKTIME".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "ASKTIME".into(),
                    map: "ASKTIME".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("bare-asktime", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "AT00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "AT00",
                24,
                80,
                "bare-asktime-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "ASKTIME", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "ASKTIME", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("ASKTIME-FN").unwrap().bytes(),
            &[0x10, 0x02]
        );
        assert_eq!(
            restored.variable("ABSTIME-FN").unwrap().bytes(),
            &[0x4a, 0x02]
        );
        assert_eq!(
            restored.variable("FORMAT-FN").unwrap().bytes(),
            &[0x4a, 0x04]
        );
        let absolute = restored.variable("ABS-TIME").unwrap().bytes().to_vec();
        assert_eq!(absolute.len(), 8);
        assert_eq!(absolute[7] & 0x0f, 0x0c);
        assert_ne!(absolute, &[0; 8]);
        let date = restored.variable("DATE-OUT").unwrap().bytes().to_vec();
        let time = restored.variable("TIME-OUT").unwrap().bytes().to_vec();
        let milliseconds = i32::from_be_bytes(
            restored
                .variable("MS-OUT")
                .unwrap()
                .bytes()
                .try_into()
                .unwrap(),
        );
        assert_eq!(date.len(), 10);
        assert_eq!((date[4], date[7]), (b'-', b'-'));
        assert!(
            date.iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        );
        assert_eq!(time.len(), 8);
        assert_eq!((time[2], time[5]), (b':', b':'));
        assert!(
            time.iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 2 | 5) || byte.is_ascii_digit())
        );
        assert!((0..=999).contains(&milliseconds));
        let eib_date = restored.variable("EIBDATE").unwrap().bytes().to_vec();
        let eib_time = restored.variable("EIBTIME").unwrap().bytes().to_vec();
        assert_eq!(eib_date.len(), 4);
        assert_eq!(eib_time.len(), 4);
        assert_eq!(eib_date[3] & 0x0f, 0x0c);
        assert_eq!(eib_time[3] & 0x0f, 0x0c);
        assert_ne!(eib_date, &[0; 4]);
        assert_ne!(eib_time, &[0; 4]);
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
    }

    #[test]
    fn online_invoke_application_crosses_compiled_selected_provider_route() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. APPINVOK.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 INVOKE-AREA PIC X(160) VALUE X'7B22706172616D65746572223A6E756C6C2C22646473223A5B5D7D'.\n01 INVOKE-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS INVOKE APPLICATION('PAYMENTS') OPERATION('RUN') PLATFORM('BANKING') COMMAREA(INVOKE-AREA) LENGTH(LENGTH OF INVOKE-AREA) END-EXEC.\nMOVE EIBFN TO INVOKE-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("APPINVOK.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "APPINVOK.cbl",
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
            panic!("INVOKE APPLICATION fixture did not publish");
        };
        let child_source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. APPCHLD.\nDATA DIVISION.\nLINKAGE SECTION.\n01 CHILD-AREA PIC X(160).\nPROCEDURE DIVISION USING CHILD-AREA.\nMOVE 'CHILD' TO CHILD-AREA.\nGOBACK.\n";
        let child_path = LogicalPath::new("APPCHLD.cbl", limits.max_path_bytes).unwrap();
        let child_bundle = SourceBundle::new(
            &child_path,
            vec![
                SourceFile::input(
                    "APPCHLD.cbl",
                    child_source.to_vec(),
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
        let CompilerResult::Published {
            artifact: child_artifact,
            ..
        } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: child_bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("INVOKE APPLICATION child fixture did not publish");
        };
        let latest_source = std::str::from_utf8(child_source)
            .unwrap()
            .replace("MOVE 'CHILD'", "MOVE 'LATEST'");
        let latest_bundle = SourceBundle::new(
            &child_path,
            vec![
                SourceFile::input(
                    "APPCHLD.cbl",
                    latest_source.into_bytes(),
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
        let CompilerResult::Published {
            artifact: latest_artifact,
            ..
        } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: latest_bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("INVOKE APPLICATION latest fixture did not publish");
        };
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .define_profile("FACILITY", "CICS.PROGRAM.APPCHLD", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit(
                "FACILITY",
                "CICS.PROGRAM.APPCHLD",
                "IBMUSER",
                AccessIntent::Execute,
            )
            .unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let child_artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(child_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let latest_artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(latest_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "APPINVOK".into(),
                        artifact: artifact_ref.clone(),
                        payload: artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                        semantic_identity: artifact.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "APPCHLD".into(),
                        artifact: child_artifact_ref.clone(),
                        payload: child_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(child_artifact.manifest().clone()),
                        semantic_identity: child_artifact.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([("IV00".into(), "APPINVOK".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "APPINVK".into(),
                    map: "APPINVK".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        server
            .artifacts
            .put_artifact(
                crate::cobol::artifact::published_artifact_record(&latest_artifact).unwrap(),
            )
            .unwrap();
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "online-program".into(),
                    key: "APPCHLD".into(),
                    version: 2,
                    payload: latest_artifact_ref.as_str().as_bytes().to_vec(),
                },
                Some(1),
            )
            .unwrap();
        server
            .online_programs
            .lock()
            .unwrap()
            .insert("APPCHLD".into(), latest_artifact_ref.clone());
        server
            .cics
            .register_program_definitions(&[
                CicsProgramDefinition {
                    name: "APPCHLD".into(),
                    generation: 1,
                    artifact: child_artifact_ref.clone(),
                    semantic_identity: child_artifact.semantic_id().to_reference(),
                    entry_offset: 0,
                    enabled: true,
                    remote: false,
                    reload: false,
                    java_status: CicsJavaStatus::NotJava,
                },
                CicsProgramDefinition {
                    name: "APPCHLD".into(),
                    generation: 2,
                    artifact: latest_artifact_ref,
                    semantic_identity: latest_artifact.semantic_id().to_reference(),
                    entry_offset: 0,
                    enabled: true,
                    remote: false,
                    reload: false,
                    java_status: CicsJavaStatus::NotJava,
                },
            ])
            .unwrap();
        server
            .cics
            .register_application_entries(&[CicsApplicationEntryDefinition {
                application: "PAYMENTS".into(),
                platform: "BANKING".into(),
                major_version: 1,
                minor_version: 0,
                micro_version: 0,
                operation: "RUN".into(),
                program: "APPCHLD".into(),
                program_generation: 1,
                program_artifact: child_artifact_ref.clone(),
                application_identity: format!(
                    "sha256:{:x}",
                    Sha256::digest(b"PAYMENTS-BANKING-1.0.0")
                ),
                available: true,
            }])
            .unwrap();
        let session = SessionId::new("typed-invoke-application", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "IV00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "IV00",
                24,
                80,
                "typed-invoke-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "APPINVOK", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "APPINVOK", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("INVOKE-FN").unwrap().bytes(),
            &[0x0e, 0x10]
        );
        assert!(
            restored
                .variable("INVOKE-AREA")
                .unwrap()
                .bytes()
                .starts_with(b"CHILD")
        );
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
    }

    #[test]
    fn online_load_and_release_cross_compiled_selected_provider_route() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LOADROUT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 SET-PTR-X POINTER.\n01 ENTRY-PTR-X POINTER.\n01 LENGTH-X PIC S9(4) COMP.\n01 SET-BYTES-X PIC X(4).\n01 ENTRY-BYTES-X PIC X(4).\n01 LOAD-FN PIC X(2).\n01 RELEASE-FN PIC X(2).\nLINKAGE SECTION.\n01 SET-LINK-X PIC X(4).\n01 ENTRY-LINK-X PIC X(4).\nPROCEDURE DIVISION.\nEXEC CICS LOAD PROGRAM('LOADPGM') SET(SET-PTR-X) ENTRY(ENTRY-PTR-X) LENGTH(LENGTH-X) HOLD END-EXEC.\nSET ADDRESS OF SET-LINK-X TO SET-PTR-X.\nSET ADDRESS OF ENTRY-LINK-X TO ENTRY-PTR-X.\nMOVE SET-LINK-X TO SET-BYTES-X.\nMOVE ENTRY-LINK-X TO ENTRY-BYTES-X.\nMOVE EIBFN TO LOAD-FN.\nEXEC CICS RELEASE PROGRAM('LOADPGM') END-EXEC.\nMOVE EIBFN TO RELEASE-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("LOADROUT.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "LOADROUT.cbl",
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
            panic!("LOAD fixture did not publish");
        };
        assert!(artifact.payload().len() <= i16::MAX as usize);
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .define_profile("FACILITY", "CICS.PROGRAM.LOADPGM", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit(
                "FACILITY",
                "CICS.PROGRAM.LOADPGM",
                "IBMUSER",
                AccessIntent::Execute,
            )
            .unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "LOADROUT".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("LD00".into(), "LOADROUT".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "LOADRT".into(),
                    map: "LOADRT".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        server
            .cics
            .register_program_definitions(&[CicsProgramDefinition {
                name: "LOADPGM".into(),
                generation: 1,
                artifact: artifact_ref.clone(),
                semantic_identity: artifact.semantic_id().to_reference(),
                entry_offset: 1,
                enabled: true,
                remote: false,
                reload: false,
                java_status: CicsJavaStatus::NotJava,
            }])
            .unwrap();
        let session = SessionId::new("typed-load-route", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "LD00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "LD00",
                24,
                80,
                "typed-load-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "LOADROUT", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "LOADROUT", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("LOAD-FN").unwrap().bytes(), &[0x0e, 0x06]);
        assert_eq!(
            restored.variable("RELEASE-FN").unwrap().bytes(),
            &[0x0e, 0x0a]
        );
        assert_eq!(
            restored.variable("LENGTH-X").unwrap().bytes(),
            &(artifact.payload().len() as i16).to_be_bytes()
        );
        assert_eq!(
            restored.variable("SET-BYTES-X").unwrap().bytes(),
            &artifact.payload()[..4]
        );
        assert_eq!(
            restored.variable("ENTRY-BYTES-X").unwrap().bytes(),
            &artifact.payload()[1..5]
        );
        assert!(
            restored
                .variable("SET-PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert!(
            restored
                .variable("ENTRY-PTR-X")
                .unwrap()
                .bytes()
                .iter()
                .any(|byte| *byte != 0)
        );
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
        assert!(
            server
                .store
                .get_provider_state("cics-program-load-v1", "LOADPGM")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn online_link_updates_typed_commarea_through_selected_program_route() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LINKER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 LINK-AREA PIC X(160) VALUE X'7B22706172616D65746572223A6E756C6C2C22646473223A5B5D7D'.\n01 LINK-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS LINK PROGRAM('IEFBR14') COMMAREA(LINK-AREA) LENGTH(LENGTH OF LINK-AREA) DATALENGTH(1) END-EXEC.\nMOVE EIBFN TO LINK-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("LINKER.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "LINKER.cbl",
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
        let compiled = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let artifact = match compiled {
            CompilerResult::Published { artifact, .. } => artifact,
            other => panic!("LINK fixture did not publish: {other:?}"),
        };
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .define_profile("FACILITY", "CICS.PROGRAM.IEFBR14", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit(
                "FACILITY",
                "CICS.PROGRAM.IEFBR14",
                "IBMUSER",
                AccessIntent::Execute,
            )
            .unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "LINKER".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("LK00".into(), "LINKER".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "LINKER".into(),
                    map: "LINKER".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("typed-link", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "LK00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "LK00",
                24,
                80,
                "typed-link-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "LINKER", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "LINKER", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("LINK-FN").unwrap().bytes(), &[0x0e, 0x02]);
        assert!(
            restored
                .variable("LINK-AREA")
                .unwrap()
                .bytes()
                .starts_with(b"{\"return_code\":0")
        );
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
    }

    #[test]
    fn online_xctl_replaces_the_frame_and_passes_typed_commarea() {
        run_online_xctl_fixture();
    }

    /// Issue #213: a program transfer keeps earlier task effects in trace order.
    #[test]
    fn online_xctl_preserves_prior_cics_trace_exactly_once() {
        let trace = run_online_xctl_fixture();
        assert_eq!(
            trace
                .iter()
                .map(|entry| entry.operation)
                .collect::<Vec<_>>(),
            vec![
                mainframe_env_host_api::CicsOperation::Xctl,
                mainframe_env_host_api::CicsOperation::Retrieve,
                mainframe_env_host_api::CicsOperation::Suspend,
            ],
        );
    }

    fn run_online_xctl_fixture() -> Vec<CicsTraceEntry> {
        let limits = SourceLimits::default();
        let compile = |name: &str, source: &[u8]| {
            let filename = format!("{name}.cbl");
            let path = LogicalPath::new(&filename, limits.max_path_bytes).unwrap();
            let bundle = SourceBundle::new(
                &path,
                vec![
                    SourceFile::input(
                        &filename,
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
                panic!("{name} XCTL fixture did not publish");
            };
            artifact
        };
        let main = compile(
            "XCTLMAIN",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. XCTLMAIN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 XCTL-AREA PIC X(8) VALUE 'REQUEST'.\n01 UNEXPECTED-HIT PIC X VALUE '0'.\nPROCEDURE DIVISION.\nEXEC CICS XCTL PROGRAM('NEXT') COMMAREA(XCTL-AREA) LENGTH(4) END-EXEC.\nMOVE '1' TO UNEXPECTED-HIT.\nSTOP RUN.\n",
        );
        let next = compile(
            "NEXT",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. NEXT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TARGET-HIT PIC X VALUE '0'.\n01 CALEN-X PIC S9(4) COMP VALUE 0.\nLINKAGE SECTION.\n01 DFHCOMMAREA PIC X(8).\nPROCEDURE DIVISION.\nMOVE EIBCALEN TO CALEN-X.\nMOVE '1' TO TARGET-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .define_profile("FACILITY", "CICS.PROGRAM.NEXT", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit(
                "FACILITY",
                "CICS.PROGRAM.NEXT",
                "IBMUSER",
                AccessIntent::Execute,
            )
            .unwrap();
        let main_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(main.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let next_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(next.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "XCTLMAIN".into(),
                        artifact: main_ref.clone(),
                        payload: main.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(main.manifest().clone()),
                        semantic_identity: main.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "NEXT".into(),
                        artifact: next_ref.clone(),
                        payload: next.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(next.manifest().clone()),
                        semantic_identity: next.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([("XC00".into(), "XCTLMAIN".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "XCTLMAIN".into(),
                    map: "XCTLMAIN".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("typed-xctl", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "XC00", Some(main_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "XC00",
                24,
                80,
                "typed-xctl-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "XCTLMAIN", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "XCTLMAIN", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        assert_eq!(continuation.program, "NEXT");
        assert_eq!(continuation.artifact, next_ref);
        let mut restored = ReferenceMachine::from_binary(
            next.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("DFHCOMMAREA").unwrap().bytes(),
            b"REQU    "
        );
        assert_eq!(restored.variable("CALEN-X").unwrap().bytes(), &[0, 4]);
        assert_eq!(restored.variable("TARGET-HIT").unwrap().bytes(), b"1");
        assert_eq!(
            server
                .store
                .get_execution(&invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let mut trace = server.online_trace(session.as_str()).unwrap();
        trace.extend(
            server
                .cics
                .terminal_run_trace(&session, &principal, 2)
                .unwrap(),
        );
        trace
    }

    #[test]
    fn online_task_association_uses_selected_security_and_durable_session_route() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ASSOC.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 CORR-X PIC X(80) VALUE ALL 'A'.\n01 SET-FN PIC X(2).\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS SET ASSOCIATION USERCORRDATA(CORR-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE EIBFN TO SET-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("ASSOC.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "ASSOC.cbl",
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
            panic!("task association fixture did not publish");
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
                    name: "ASSOC".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("AS00".into(), "ASSOC".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "ASSOC".into(),
                    map: "ASSOC".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("task-association", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "AS00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "AS00",
                24,
                80,
                "task-association-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "ASSOC", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "ASSOC", 2)
            .unwrap();

        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("SET-FN").unwrap().bytes(), &[0xc4, 0x04]);
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0; 4]);
        let session_row = server
            .store
            .get_provider_state("cics-session", session.as_str())
            .unwrap()
            .unwrap();
        assert!(
            session_row
                .payload
                .windows(64)
                .any(|window| window == [b'A'; 64])
        );
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            vec![
                (1, mainframe_env_execution_api::AuditDecision::Success),
                (2, mainframe_env_execution_api::AuditDecision::Success),
            ]
        );
        server
            .run_online_exchange(&session, &principal, "ASSOC", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_address_set_uses_checked_virtual_pointers_on_selected_route() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ADDRSET.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 SOURCE-X PIC X(4) VALUE 'ABCD'.\n01 OBSERVED-X PIC X(4).\n01 PTR-X POINTER.\n01 SET-FN PIC X(2).\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nLINKAGE SECTION.\n01 LINK-X PIC X(4).\nPROCEDURE DIVISION.\nEXEC CICS ADDRESS SET(PTR-X) USING(ADDRESS OF SOURCE-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ADDRESS SET(ADDRESS OF LINK-X) USING(PTR-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE 'WXYZ' TO LINK-X.\nMOVE SOURCE-X TO OBSERVED-X.\nMOVE EIBFN TO SET-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("ADDRSET.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "ADDRSET.cbl",
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
            panic!("ADDRESS SET fixture did not publish");
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
                    name: "ADDRSET".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("AD00".into(), "ADDRSET".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "ADDRSET".into(),
                    map: "ADDRSET".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("address-set", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "AD00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "AD00",
                24,
                80,
                "address-set-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "ADDRSET", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "ADDRSET", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("SOURCE-X").unwrap().bytes(), b"WXYZ");
        assert_eq!(restored.variable("OBSERVED-X").unwrap().bytes(), b"WXYZ");
        assert_eq!(restored.variable("SET-FN").unwrap().bytes(), &[0x02, 0x10]);
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            vec![
                (1, mainframe_env_execution_api::AuditDecision::Success),
                (2, mainframe_env_execution_api::AuditDecision::Success),
                (3, mainframe_env_execution_api::AuditDecision::Success),
            ]
        );
        server
            .run_online_exchange(&session, &principal, "ADDRSET", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_handle_abend_reset_reactivates_the_selected_exit_once() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HABRESET.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 COUNT-X PIC 9 VALUE 0.\n01 FIRST-FN PIC X(2).\n01 SECOND-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS START TRANSID('HR00') REQID('PRAB0001') FROM(START-DATA) INTERVAL(0) PROTECT END-EXEC.\nEXEC CICS HANDLE ABEND LABEL(ABEND-HANDLER) END-EXEC.\nEXEC CICS ABEND ABCODE('B001') END-EXEC.\nSTOP RUN.\nABEND-HANDLER.\nADD 1 TO COUNT-X.\nMOVE EIBFN TO FIRST-FN.\nIF COUNT-X = 1\n  EXEC CICS HANDLE ABEND RESET END-EXEC\n  EXEC CICS ABEND ABCODE('B002') END-EXEC\nEND-IF.\nMOVE EIBFN TO SECOND-FN.\nEXEC CICS HANDLE ABEND END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let source = std::str::from_utf8(source)
            .unwrap()
            .replace(
                "01 COUNT-X PIC 9 VALUE 0.\n",
                "01 COUNT-X PIC 9 VALUE 0.\n01 START-DATA PIC X(8) VALUE 'PROTECT'.\n01 CURRENT-ABCODE PIC X(4).\n01 ORIGINAL-ABCODE PIC X(4).\n",
            )
            .replace(
                "END-IF.\nMOVE EIBFN TO SECOND-FN.",
                "END-IF.\nMOVE EIBFN TO SECOND-FN.\nEXEC CICS ASSIGN ABCODE(CURRENT-ABCODE) ORGABCODE(ORIGINAL-ABCODE) END-EXEC.",
            );
        let path = LogicalPath::new("HABRESET.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "HABRESET.cbl",
                    source.as_bytes().to_vec(),
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
            panic!("HANDLE ABEND RESET fixture did not publish");
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
                    name: "HABRESET".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("HR00".into(), "HABRESET".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HABRESET".into(),
                    map: "HABRESET".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("handle-abend-reset", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "HR00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "HR00",
                24,
                80,
                "handle-abend-reset-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "HABRESET", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HABRESET", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("COUNT-X").unwrap().bytes(), b"2");
        assert_eq!(
            restored.variable("CURRENT-ABCODE").unwrap().bytes(),
            b"B002"
        );
        assert_eq!(
            restored.variable("ORIGINAL-ABCODE").unwrap().bytes(),
            b"B001"
        );
        assert_eq!(
            restored.variable("FIRST-FN").unwrap().bytes(),
            &[0x0e, 0x0c]
        );
        assert_eq!(
            restored.variable("SECOND-FN").unwrap().bytes(),
            &[0x0e, 0x0c]
        );
        assert!(
            server
                .store
                .get_provider_state("cics-interval-start-v1", "PRAB0001")
                .unwrap()
                .is_none()
        );
        assert!(
            server
                .store
                .get_work("cics-start:PRAB0001")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=8)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        server
            .run_online_exchange(&session, &principal, "HABRESET", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_handle_abend_program_transfers_to_the_selected_exit() {
        let limits = SourceLimits::default();
        let compile = |name: &str, source: &[u8]| {
            let filename = format!("{name}.cbl");
            let path = LogicalPath::new(&filename, limits.max_path_bytes).unwrap();
            let bundle = SourceBundle::new(
                &path,
                vec![
                    SourceFile::input(
                        &filename,
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
                panic!("{name} HANDLE ABEND PROGRAM fixture did not publish");
            };
            artifact
        };
        let main = compile(
            "HABPROG",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HABPROG.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 UNEXPECTED-HIT PIC X VALUE '0'.\nPROCEDURE DIVISION.\nEXEC CICS HANDLE ABEND PROGRAM('ABEXIT') END-EXEC.\nEXEC CICS ABEND ABCODE('B777') END-EXEC.\nMOVE '1' TO UNEXPECTED-HIT.\nSTOP RUN.\n",
        );
        let exit = compile(
            "ABEXIT",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ABEXIT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 CURRENT-ABCODE PIC X(4).\n01 CURRENT-ABDUMP PIC X.\n01 CURRENT-ABPROGRAM PIC X(8).\n01 CURRENT-PROGRAM PIC X(8).\n01 EXIT-HIT PIC X VALUE '0'.\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS ASSIGN ABCODE(CURRENT-ABCODE) ABDUMP(CURRENT-ABDUMP) ABPROGRAM(CURRENT-ABPROGRAM) PROGRAM(CURRENT-PROGRAM) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nMOVE '1' TO EXIT-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n",
        );
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .racf
            .define_profile("FACILITY", "CICS.PROGRAM.ABEXIT", "IBMUSER", None)
            .unwrap();
        server
            .racf
            .permit(
                "FACILITY",
                "CICS.PROGRAM.ABEXIT",
                "IBMUSER",
                AccessIntent::Execute,
            )
            .unwrap();
        let main_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(main.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let exit_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(exit.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "HABPROG".into(),
                        artifact: main_ref.clone(),
                        payload: main.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(main.manifest().clone()),
                        semantic_identity: main.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "ABEXIT".into(),
                        artifact: exit_ref.clone(),
                        payload: exit.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(exit.manifest().clone()),
                        semantic_identity: exit.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([("HP00".into(), "HABPROG".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HABPROG".into(),
                    map: "HABPROG".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("handle-abend-program", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "HP00", Some(main_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "HP00",
                24,
                80,
                "handle-abend-program-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        let original_exchange = server
            .begin_online_exchange(&session, "HABPROG", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HABPROG", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        assert_eq!(continuation.program, "ABEXIT");
        assert_eq!(continuation.artifact, exit_ref);
        let mut restored = ReferenceMachine::from_binary(
            exit.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("CURRENT-ABCODE").unwrap().bytes(),
            b"B777"
        );
        assert_eq!(
            restored.variable("CURRENT-ABDUMP").unwrap().bytes(),
            &[0xff]
        );
        assert_eq!(
            restored.variable("CURRENT-ABPROGRAM").unwrap().bytes(),
            b"HABPROG "
        );
        assert_eq!(
            restored.variable("CURRENT-PROGRAM").unwrap().bytes(),
            b"ABEXIT  "
        );
        assert_eq!(restored.variable("EXIT-HIT").unwrap().bytes(), b"1");
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0; 4]);
        let transferred_exchange = server.online_exchange(&session).unwrap().unwrap();
        assert_eq!(transferred_exchange.program, "ABEXIT");
        let transferred_execution = ExecutionId::new(
            &transferred_exchange.execution_id,
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=2)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            server
                .store
                .audit_records(&transferred_execution, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            vec![
                (1, mainframe_env_execution_api::AuditDecision::Success),
                (2, mainframe_env_execution_api::AuditDecision::Success),
            ]
        );

        let continuation_record = server
            .store
            .get_provider_state("online-machine-continuation", session.as_str())
            .unwrap()
            .unwrap();
        let pending = PendingOnlineTransfer {
            prior_execution_id: invocation.execution_id.as_str().into(),
            expected_exchange_version: original_exchange.version,
            next_exchange: transferred_exchange.clone(),
        };
        let staged = ProviderStateRecord {
            namespace: "online-machine-continuation".into(),
            key: session.as_str().into(),
            version: continuation_record.version + 1,
            payload: encode_online_machine_continuation_with_transfer(
                &continuation.program,
                &continuation.artifact,
                &continuation.provider_generations,
                continuation.priority.unwrap(),
                &continuation.checkpoint,
                Some(&pending),
            )
            .unwrap(),
        };
        server
            .store
            .put_provider_state(staged, Some(continuation_record.version))
            .unwrap();
        server
            .store
            .delete_provider_state(
                ONLINE_EXCHANGE_NAMESPACE,
                session.as_str(),
                transferred_exchange.version,
            )
            .unwrap();
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: session.as_str().into(),
                    version: original_exchange.version,
                    payload: encode_online_exchange(&original_exchange).unwrap(),
                },
                None,
            )
            .unwrap();
        let mut saved = server.online_machine_continuation(&session).unwrap();
        let mut exchange = server.online_exchange(&session).unwrap();
        server
            .recover_pending_online_transfer_if_present(&session, &mut saved, &mut exchange, 3)
            .unwrap();
        assert_eq!(exchange.unwrap(), transferred_exchange);
        assert!(saved.unwrap().transfer.is_none());
        server
            .run_online_exchange(&session, &principal, "HABPROG", 4)
            .unwrap();
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_none()
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_push_pop_handle_restores_the_outer_exit_snapshot() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HSTACK.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 INNER-FN PIC X(2).\n01 OUTER-FN PIC X(2).\n01 RESP-X PIC S9(9) COMP.\n01 RESP2-X PIC S9(9) COMP.\nPROCEDURE DIVISION.\nEXEC CICS HANDLE ABEND LABEL(OUTER-HANDLER) END-EXEC.\nEXEC CICS PUSH HANDLE END-EXEC.\nEXEC CICS HANDLE ABEND LABEL(INNER-HANDLER) END-EXEC.\nEXEC CICS ABEND ABCODE('B001') END-EXEC.\nSTOP RUN.\nINNER-HANDLER.\nMOVE EIBFN TO INNER-FN.\nEXEC CICS POP HANDLE RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nEXEC CICS ABEND ABCODE('B002') END-EXEC.\nSTOP RUN.\nOUTER-HANDLER.\nMOVE EIBFN TO OUTER-FN.\nEXEC CICS HANDLE ABEND CANCEL END-EXEC.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("HSTACK.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "HSTACK.cbl",
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
            panic!("PUSH/POP HANDLE fixture did not publish");
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
                    name: "HSTACK".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("HS00".into(), "HSTACK".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HSTACK".into(),
                    map: "HSTACK".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("push-pop-handle", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "HS00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "HS00",
                24,
                80,
                "push-pop-handle-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "HSTACK", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HSTACK", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("INNER-FN").unwrap().bytes(),
            &[0x0e, 0x0c]
        );
        assert_eq!(
            restored.variable("OUTER-FN").unwrap().bytes(),
            &[0x0e, 0x0c]
        );
        assert_eq!(restored.variable("RESP-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("RESP2-X").unwrap().bytes(), &[0; 4]);
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=8)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        server
            .run_online_exchange(&session, &principal, "HSTACK", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_ignore_condition_continues_and_survives_push_pop() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. IGNCOND.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 IGNORE-FN PIC X(2).\n01 POP-FN PIC X(2).\n01 FIRST-RESP PIC S9(9) COMP.\n01 SECOND-RESP PIC S9(9) COMP.\n01 ERROR-HIT PIC 9 VALUE 0.\nPROCEDURE DIVISION.\nEXEC CICS IGNORE CONDITION INVREQ END-EXEC.\nMOVE EIBFN TO IGNORE-FN.\nEXEC CICS POP HANDLE END-EXEC.\nMOVE EIBRESP TO FIRST-RESP.\nEXEC CICS PUSH HANDLE END-EXEC.\nEXEC CICS HANDLE CONDITION INVREQ(ERROR-HANDLER) END-EXEC.\nEXEC CICS POP HANDLE END-EXEC.\nEXEC CICS POP HANDLE END-EXEC.\nMOVE EIBFN TO POP-FN.\nMOVE EIBRESP TO SECOND-RESP.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\nERROR-HANDLER.\nMOVE 1 TO ERROR-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("IGNCOND.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "IGNCOND.cbl",
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
            panic!("IGNORE CONDITION fixture did not publish");
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
                    name: "IGNCOND".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("IC00".into(), "IGNCOND".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "IGNCOND".into(),
                    map: "IGNCOND".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("ignore-condition", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "IC00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "IC00",
                24,
                80,
                "ignore-condition-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "IGNCOND", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "IGNCOND", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("IGNORE-FN").unwrap().bytes(),
            &[0x02, 0x0a]
        );
        assert_eq!(restored.variable("POP-FN").unwrap().bytes(), &[0x02, 0x0e]);
        assert_eq!(
            restored.variable("FIRST-RESP").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("SECOND-RESP").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(restored.variable("ERROR-HIT").unwrap().bytes(), b"0");
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=7)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        server
            .run_online_exchange(&session, &principal, "IGNCOND", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_handle_condition_routes_multiple_and_deactivates_specific_exit() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HCOND.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 FIRST-FN PIC X(2).\n01 SECOND-FN PIC X(2).\n01 FIRST-RESP PIC S9(9) COMP.\n01 SECOND-RESP PIC S9(9) COMP.\n01 SPECIFIC-HIT PIC 9 VALUE 0.\n01 GENERAL-HIT PIC 9 VALUE 0.\n01 UNEXPECTED-HIT PIC 9 VALUE 0.\nPROCEDURE DIVISION.\nEXEC CICS HANDLE CONDITION ERROR(GENERAL-HANDLER) INVREQ(SPECIFIC-HANDLER) LENGERR END-EXEC.\nMOVE EIBFN TO FIRST-FN.\nEXEC CICS POP HANDLE END-EXEC.\nMOVE 9 TO UNEXPECTED-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\nSPECIFIC-HANDLER.\nMOVE 1 TO SPECIFIC-HIT.\nMOVE EIBRESP TO FIRST-RESP.\nEXEC CICS HANDLE CONDITION INVREQ END-EXEC.\nMOVE EIBFN TO SECOND-FN.\nEXEC CICS POP HANDLE END-EXEC.\nMOVE 9 TO UNEXPECTED-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\nGENERAL-HANDLER.\nMOVE 1 TO GENERAL-HIT.\nMOVE EIBRESP TO SECOND-RESP.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("HCOND.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "HCOND.cbl",
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
            panic!("HANDLE CONDITION fixture did not publish");
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
                    name: "HCOND".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("HC00".into(), "HCOND".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HCOND".into(),
                    map: "HCOND".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("handle-condition", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "HC00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "HC00",
                24,
                80,
                "handle-condition-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "HCOND", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HCOND", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("FIRST-FN").unwrap().bytes(),
            &[0x02, 0x04]
        );
        assert_eq!(
            restored.variable("SECOND-FN").unwrap().bytes(),
            &[0x02, 0x04]
        );
        assert_eq!(
            restored.variable("FIRST-RESP").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(
            restored.variable("SECOND-RESP").unwrap().bytes(),
            &[0, 0, 0, 16]
        );
        assert_eq!(restored.variable("SPECIFIC-HIT").unwrap().bytes(), b"1");
        assert_eq!(restored.variable("GENERAL-HIT").unwrap().bytes(), b"1");
        assert_eq!(restored.variable("UNEXPECTED-HIT").unwrap().bytes(), b"0");
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=5)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        server
            .run_online_exchange(&session, &principal, "HCOND", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_handle_aid_uses_the_typed_selected_route_and_handle_stack() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HAID.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 FIRST-FN PIC X(2).\n01 SECOND-FN PIC X(2).\nPROCEDURE DIVISION.\nEXEC CICS HANDLE AID ANYKEY(OUTER-AID) ENTER PF10 END-EXEC.\nMOVE EIBFN TO FIRST-FN.\nEXEC CICS PUSH HANDLE END-EXEC.\nEXEC CICS HANDLE AID PF1(INNER-AID) END-EXEC.\nEXEC CICS POP HANDLE END-EXEC.\nMOVE EIBFN TO SECOND-FN.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\nOUTER-AID.\nSTOP RUN.\nINNER-AID.\nSTOP RUN.\n";
        let path = LogicalPath::new("HAID.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "HAID.cbl",
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
            panic!("HANDLE AID fixture did not publish");
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
                    name: "HAID".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("HA00".into(), "HAID".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HAID".into(),
                    map: "HAID".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("handle-aid", 64).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "HA00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "HA00",
                24,
                80,
                "handle-aid-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "HAID", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HAID", 2)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(
            restored.variable("FIRST-FN").unwrap().bytes(),
            &[0x02, 0x06]
        );
        assert_eq!(
            restored.variable("SECOND-FN").unwrap().bytes(),
            &[0x02, 0x0e]
        );
        assert_eq!(
            server
                .store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap()
                .into_iter()
                .filter(|record| record.capability.as_str() == "host.cics.execute")
                .map(|record| (record.effect_sequence, record.decision))
                .collect::<Vec<_>>(),
            (1..=5)
                .map(|sequence| {
                    (
                        sequence,
                        mainframe_env_execution_api::AuditDecision::Success,
                    )
                })
                .collect::<Vec<_>>()
        );
        server
            .run_online_exchange(&session, &principal, "HAID", 3)
            .unwrap();
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[test]
    fn online_handle_aid_survives_a_durable_terminal_handoff() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. HARES.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 AID-HIT PIC X VALUE '0'.\n01 UNEXPECTED-HIT PIC X VALUE '0'.\nPROCEDURE DIVISION.\nEXEC CICS HANDLE AID ANYKEY(AID-HANDLER) END-EXEC.\nEXEC CICS SEND MAP('HARES') MAPSET('HARES') END-EXEC.\nEXEC CICS RECEIVE MAP('HARES') MAPSET('HARES') END-EXEC.\nMOVE '1' TO UNEXPECTED-HIT.\nSTOP RUN.\nAID-HANDLER.\nMOVE '1' TO AID-HIT.\nEXEC CICS SUSPEND END-EXEC.\nSTOP RUN.\n";
        let source = std::str::from_utf8(source)
            .unwrap()
            .replace(
                "01 UNEXPECTED-HIT PIC X VALUE '0'.",
                "01 UNEXPECTED-HIT PIC X VALUE '0'.\n01 INPUT-LENGTH-X PIC S9(4) COMP VALUE 0.",
            )
            .replace(
                "AID-HANDLER.\nMOVE '1' TO AID-HIT.",
                "AID-HANDLER.\nEXEC CICS ASSIGN INPUTMSGLEN(INPUT-LENGTH-X) END-EXEC.\nMOVE '1' TO AID-HIT.",
            );
        let path = LogicalPath::new("HARES.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "HARES.cbl",
                    source.as_bytes().to_vec(),
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
            panic!("durable HANDLE AID fixture did not publish");
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
                    name: "HARES".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("HR00".into(), "HARES".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "HARES".into(),
                    map: "HARES".into(),
                    line: 1,
                    column: 1,
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
        let session = SessionId::new("handle-aid-handoff", 64).unwrap();
        let first = server
            .cics_invocation("IBMUSER", "HR00", Some(artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .launch_terminal(
                first.clone(),
                &session,
                "HR00",
                24,
                80,
                "handle-aid-handoff-csrf",
                1,
                10_000,
            )
            .unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&session, "HARES", &context)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HARES", 2)
            .unwrap();
        assert!(matches!(
            server.cics.terminal_execution(&session, &principal, 2),
            Err(HostProblem::NotFound)
        ));
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_some()
        );

        server
            .cics
            .submit_terminal_input(
                &session,
                &principal,
                "handle-aid-handoff-csrf",
                0xf1,
                &BTreeMap::from([("INPUT".into(), b"AB".to_vec())]),
                3,
            )
            .unwrap();
        let resumed = server
            .cics_invocation("IBMUSER", "HR00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .resume_terminal(resumed.clone(), &session, "handle-aid-handoff-csrf", 4)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "HARES", 4)
            .unwrap();
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), resumed, CodecLimits::default())
                .unwrap();
        restored
            .restore_checkpoint(&continuation.checkpoint)
            .unwrap();
        assert_eq!(restored.variable("AID-HIT").unwrap().bytes(), b"1");
        assert_eq!(
            restored.variable("INPUT-LENGTH-X").unwrap().bytes(),
            &[0, 15]
        );
        assert_eq!(restored.variable("UNEXPECTED-HIT").unwrap().bytes(), b"0");
    }

    #[test]
    fn online_enqueue_wait_remains_durably_resumable_until_dequeue() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. WAITENQ.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 LOCK-NAME PIC X(4) VALUE 'LOCK'.\nPROCEDURE DIVISION.\nEXEC CICS ENQ RESOURCE(LOCK-NAME) LENGTH(4) UOW END-EXEC.\nDISPLAY 'ACQUIRED'.\nSTOP RUN.\n";
        let path = LogicalPath::new("WAITENQ.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "WAITENQ.cbl",
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
            panic!("enqueue wait fixture did not publish");
        };
        let clock = Arc::new(ManualJesClock::new(100));
        let platform_store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let server =
            ProductServer::open_with_clock(config(), platform_store, clock.clone()).unwrap();
        let execution_clock = clock.clone();
        server
            .program
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: execution_clock
                        .now_tick()
                        .map_err(|_| ExecutionControlError::Unavailable)?,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition {
                    name: "WAITENQ".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("EQ00".into(), "WAITENQ".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "WAITENQ".into(),
                    map: "WAITENQ".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: vec![mainframe_env_cics::BmsFieldDefinition {
                        name: "STATUS".into(),
                        row: 1,
                        column: 1,
                        length: 8,
                        initial: Vec::new(),
                        color: None,
                        highlight: None,
                        protected: true,
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
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();

        let owner_session = SessionId::new("enqueue-owner", 64).unwrap();
        let owner = server
            .cics_invocation("IBMUSER", "EQ00", Some(artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .launch_terminal(
                owner.clone(),
                &owner_session,
                "EQ00",
                24,
                80,
                "enqueue-owner-csrf",
                1,
                10_000,
            )
            .unwrap();
        let argument = |schema: &str, bytes: Vec<u8>| {
            BoundedPayload::new(schema, bytes, InvocationLimits::default()).unwrap()
        };
        let cics_request = |operation, sequence, key: &str| CicsRequest {
            operation,
            arguments: BTreeMap::from([
                (
                    "RESOURCE".into(),
                    argument("mainframe-env.cics.storage-value@1", b"LOCK".to_vec()),
                ),
                (
                    "LENGTH".into(),
                    argument("mainframe-env.cics.decimal@1", b"4".to_vec()),
                ),
            ]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
                transaction: Some("EQ00".into()),
            }),
        };
        let acquire = cics_request(CicsOperation::Enq, 1, "enqueue-owner-acquire");
        server
            .cics
            .invoke(
                &EffectRequest {
                    run_unit: owner.run_unit_id.clone(),
                    sequence: 1,
                    deadline_tick: owner.deadline_tick,
                    idempotency_key: acquire
                        .mutation
                        .as_ref()
                        .map(|mutation| mutation.idempotency_key.clone()),
                    request: HostRequest::Cics(acquire.clone()),
                },
                acquire,
            )
            .unwrap();

        let waiter_session = SessionId::new("enqueue-waiter", 64).unwrap();
        let waiter = server
            .cics_invocation("IBMUSER", "EQ00", Some(artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .launch_terminal(
                waiter.clone(),
                &waiter_session,
                "EQ00",
                24,
                80,
                "enqueue-waiter-csrf",
                1,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&waiter_session, &principal, 2)
            .unwrap();
        server
            .begin_online_exchange(&waiter_session, "WAITENQ", &context)
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&waiter_session, &principal, "WAITENQ", 2),
            Ok(())
        );
        assert_eq!(
            server
                .store
                .get_execution(&waiter.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Suspended
        );
        assert!(server.online_exchange(&waiter_session).unwrap().is_some());
        assert!(
            server
                .online_machine_continuation(&waiter_session)
                .unwrap()
                .is_some()
        );
        assert!(
            server
                .store
                .get_checkpoint(&waiter.execution_id)
                .unwrap()
                .is_some()
        );

        let release = cics_request(CicsOperation::Deq, 2, "enqueue-owner-release");
        server
            .cics
            .invoke(
                &EffectRequest {
                    run_unit: owner.run_unit_id.clone(),
                    sequence: 2,
                    deadline_tick: owner.deadline_tick,
                    idempotency_key: release
                        .mutation
                        .as_ref()
                        .map(|mutation| mutation.idempotency_key.clone()),
                    request: HostRequest::Cics(release.clone()),
                },
                release,
            )
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&waiter_session, &principal, "WAITENQ", 3),
            Ok(())
        );
        assert_eq!(
            server
                .store
                .get_execution(&waiter.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert!(server.online_exchange(&waiter_session).unwrap().is_none());
        assert!(
            server
                .online_machine_continuation(&waiter_session)
                .unwrap()
                .is_none()
        );
        assert!(
            server
                .store
                .list_provider_state("cics-enqueue-v1", 2)
                .unwrap()
                .is_empty()
        );

        let reacquire = cics_request(CicsOperation::Enq, 3, "enqueue-owner-reacquire");
        server
            .cics
            .invoke(
                &EffectRequest {
                    run_unit: owner.run_unit_id.clone(),
                    sequence: 3,
                    deadline_tick: owner.deadline_tick,
                    idempotency_key: reacquire
                        .mutation
                        .as_ref()
                        .map(|mutation| mutation.idempotency_key.clone()),
                    request: HostRequest::Cics(reacquire.clone()),
                },
                reacquire,
            )
            .unwrap();
        let timed_session = SessionId::new("enqueue-timed-waiter", 64).unwrap();
        let timed = server
            .cics_invocation("IBMUSER", "EQ00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(
                timed.clone(),
                &timed_session,
                "EQ00",
                24,
                80,
                "enqueue-timed-csrf",
                4,
                10_000,
            )
            .unwrap();
        let context = server
            .cics
            .terminal_execution(&timed_session, &principal, 4)
            .unwrap();
        server
            .begin_online_exchange(&timed_session, "WAITENQ", &context)
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&timed_session, &principal, "WAITENQ", 4),
            Ok(())
        );
        clock.advance(server.config.timeout_millis + 1);
        assert_eq!(
            server.run_online_exchange(&timed_session, &principal, "WAITENQ", 5),
            Err(HostProblem::TimedOut)
        );
        assert_eq!(
            server
                .store
                .get_execution(&timed.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::TimedOut
        );
        assert!(server.online_exchange(&timed_session).unwrap().is_none());

        let release = cics_request(CicsOperation::Deq, 4, "enqueue-owner-final-release");
        server
            .cics
            .invoke(
                &EffectRequest {
                    run_unit: owner.run_unit_id.clone(),
                    sequence: 4,
                    deadline_tick: owner.deadline_tick,
                    idempotency_key: release
                        .mutation
                        .as_ref()
                        .map(|mutation| mutation.idempotency_key.clone()),
                    request: HostRequest::Cics(release.clone()),
                },
                release,
            )
            .unwrap();
        assert!(
            server
                .store
                .list_provider_state("cics-enqueue-v1", 2)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn online_pseudo_conversations_handoff_without_leaking_suspended_executions() {
        fn assert_handoff(server: &ProductServer, execution_id: &ExecutionId) {
            let execution = server.store.get_execution(execution_id).unwrap().unwrap();
            assert_eq!(execution.state, ExecutionState::Completed);
            assert!(server.store.get_checkpoint(execution_id).unwrap().is_none());
            assert!(matches!(
                server
                    .store
                    .events(execution_id, execution.version, 1)
                    .unwrap()
                    .as_slice(),
                [event] if matches!(event.kind, LifecycleEventKind::HandoffCompleted)
            ));
        }

        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. PSEUDO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MSG PIC X(5) VALUE 'HELLO'.\nPROCEDURE DIVISION.\nEXEC CICS SEND MAP('PSEUDO') MAPSET('PSEUDO') END-EXEC.\nEXEC CICS RECEIVE MAP('PSEUDO') MAPSET('PSEUDO') END-EXEC.\nEXEC CICS SEND TEXT FROM(MSG) END-EXEC.\nEXEC CICS RETURN TRANSID('PS00') END-EXEC.\n";
        let path = LogicalPath::new("PSEUDO.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "PSEUDO.cbl",
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
            panic!("pseudo-conversation fixture did not publish");
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
                    name: "PSEUDO".into(),
                    artifact: artifact_ref.clone(),
                    payload: artifact.payload().to_vec(),
                    manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                    semantic_identity: artifact.semantic_id().to_reference(),
                }],
                transactions: BTreeMap::from([("PS00".into(), "PSEUDO".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "PSEUDO".into(),
                    map: "PSEUDO".into(),
                    line: 1,
                    column: 1,
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
        let session = SessionId::new("r03-pseudo-session", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let launch = server
            .cics_invocation("IBMUSER", "PS00", Some(artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .launch_terminal(
                launch,
                &session,
                "PS00",
                24,
                80,
                "r03-pseudo-csrf",
                1,
                10_000,
            )
            .unwrap();
        let first_context = server
            .cics
            .terminal_execution(&session, &principal, 2)
            .unwrap();
        let first_exchange = server
            .begin_online_exchange(&session, "PSEUDO", &first_context)
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&session, &principal, "PSEUDO", 2),
            Ok(())
        );
        assert_handoff(&server, &first_context.invocation.execution_id);
        let continuation = server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap();
        assert_eq!(continuation.artifact, artifact_ref);
        assert!(server.online_exchange(&session).unwrap().is_none());

        // Recreate the exact crash gap after the handoff event but before CICS
        // and exchange cleanup. Recovery must retain the product checkpoint.
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: session.as_str().into(),
                    version: first_exchange.version,
                    payload: encode_online_exchange(&first_exchange).unwrap(),
                },
                None,
            )
            .unwrap();
        server
            .cics
            .restore_terminal_run(
                first_context.invocation.clone(),
                &session,
                &first_context.transaction,
                first_context.commarea.clone(),
                3,
            )
            .unwrap();
        assert_eq!(
            server
                .recover_terminal_online_exchange(&session, &principal, &first_exchange, 3)
                .unwrap(),
            Some(TerminalExchangeRecovery::HandoffCompleted)
        );
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_some()
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
        assert!(matches!(
            server.cics.terminal_execution(&session, &principal, 3),
            Err(HostProblem::NotFound)
        ));

        let original_continuation = server
            .store
            .get_provider_state("online-machine-continuation", session.as_str())
            .unwrap()
            .unwrap();
        let decoded = decode_online_machine_continuation(&original_continuation).unwrap();
        let mut incompatible_continuation = original_continuation.clone();
        incompatible_continuation.version += 1;
        incompatible_continuation.payload = encode_online_machine_continuation(
            &decoded.program,
            &ArtifactRef::new(format!("sha256:{:064x}", 0), InvocationLimits::default()).unwrap(),
            &decoded.provider_generations,
            decoded.priority.unwrap_or(0),
            &decoded.checkpoint,
        )
        .unwrap();
        server
            .store
            .put_provider_state(
                incompatible_continuation.clone(),
                Some(original_continuation.version),
            )
            .unwrap();

        server
            .cics
            .submit_terminal_input(
                &session,
                &principal,
                "r03-pseudo-csrf",
                0x7d,
                &BTreeMap::from([("INPUT".into(), b"ONE".to_vec())]),
                4,
            )
            .unwrap();
        let resumed = server
            .cics_invocation("IBMUSER", "PS00", Some(artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .resume_terminal(resumed, &session, "r03-pseudo-csrf", 5)
            .unwrap();
        let completed_context = server
            .cics
            .terminal_execution(&session, &principal, 5)
            .unwrap();
        let sends_before = server
            .online_operation_count(CicsOperation::SendText)
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&session, &principal, "PSEUDO", 5),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            server
                .online_operation_count(CicsOperation::SendText)
                .unwrap(),
            sends_before
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
        assert!(
            server
                .store
                .get_execution(&completed_context.invocation.execution_id)
                .unwrap()
                .is_none()
        );
        let mut wrong_generations = decoded.provider_generations.clone();
        *wrong_generations.values_mut().next().unwrap() = "incompatible-generation".into();
        let mut generation_incompatible = original_continuation.clone();
        generation_incompatible.version = incompatible_continuation.version + 1;
        generation_incompatible.payload = encode_online_machine_continuation(
            &decoded.program,
            &artifact_ref,
            &wrong_generations,
            decoded.priority.unwrap_or(0),
            &decoded.checkpoint,
        )
        .unwrap();
        server
            .store
            .put_provider_state(
                generation_incompatible.clone(),
                Some(incompatible_continuation.version),
            )
            .unwrap();
        assert_eq!(
            server.run_online_exchange(&session, &principal, "PSEUDO", 5),
            Err(HostProblem::ProviderFailure)
        );
        assert_eq!(
            server
                .online_operation_count(CicsOperation::SendText)
                .unwrap(),
            sends_before
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
        assert!(
            server
                .store
                .get_execution(&completed_context.invocation.execution_id)
                .unwrap()
                .is_none()
        );
        let mut restored_continuation = original_continuation;
        restored_continuation.version = generation_incompatible.version + 1;
        server
            .store
            .put_provider_state(restored_continuation, Some(generation_incompatible.version))
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "PSEUDO", 5)
            .unwrap();
        assert_eq!(
            server
                .store
                .get_execution(&completed_context.invocation.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_none()
        );

        // The RETURN TRANSID starts another task. With no input it suspends
        // again, but the old execution is terminalized through a second handoff.
        let next = server
            .cics_invocation("IBMUSER", "PS00", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .resume_terminal(next, &session, "r03-pseudo-csrf", 6)
            .unwrap();
        let second_context = server
            .cics
            .terminal_execution(&session, &principal, 6)
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "PSEUDO", 6)
            .unwrap();
        assert_handoff(&server, &second_context.invocation.execution_id);
        assert!(
            server
                .online_machine_continuation(&session)
                .unwrap()
                .is_some()
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn online_unknown_reconciles_and_known_failure_does_not_strand_session() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. RECOVER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 DATA-X PIC X(4) VALUE 'AA11'.\n01 KEY-X PIC X(4) VALUE '0001'.\nPROCEDURE DIVISION.\nEXEC CICS WRITE FILE('RECFILE') FROM(DATA-X) RIDFLD(KEY-X) END-EXEC.\nSTOP RUN.\n";
        let path = LogicalPath::new("RECOVER.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "RECOVER.cbl",
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
        let recovery_compilation = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = recovery_compilation else {
            panic!("recovery fixture did not publish: {recovery_compilation:?}");
        };
        let known_source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. KNOWNFAIL.\nPROCEDURE DIVISION.\nCALL 'MISSING-PROGRAM'.\nSTOP RUN.\n";
        let known_path = LogicalPath::new("KNOWNFAIL.cbl", limits.max_path_bytes).unwrap();
        let known_bundle = SourceBundle::new(
            &known_path,
            vec![
                SourceFile::input(
                    "KNOWNFAIL.cbl",
                    known_source.to_vec(),
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
        let CompilerResult::Published {
            artifact: known_artifact,
            ..
        } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: known_bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("known-failure fixture did not publish");
        };
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-online-restart-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            session_tick().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let mut server_config = config();
        server_config.store_profile = crate::StoreProfile::Sqlite;
        server_config.sqlite_url = url.clone();
        server_config.artifact_root = root.join("artifacts");
        let first_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first_platform: Arc<dyn PlatformStore> = first_store.clone();
        let server = ProductServer::open(
            server_config.clone(),
            first_platform,
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
                    dataset: "IBMUSER.RECOVERY".into(),
                    attributes: json!({"dsorg":"PS","recfm":"V","lrecl":80}),
                },
            )
            .unwrap();
        server
            .cics
            .register_file_aliases(&BTreeMap::from([(
                "RECFILE".into(),
                DatasetName::new("IBMUSER.RECOVERY", 128).unwrap(),
            )]))
            .unwrap();
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let known_artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(known_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "RECOVER".into(),
                        artifact: artifact_ref.clone(),
                        payload: artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                        semantic_identity: artifact.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "KNOWNFAIL".into(),
                        artifact: known_artifact_ref.clone(),
                        payload: known_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(known_artifact.manifest().clone()),
                        semantic_identity: known_artifact.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([
                    ("RCVY".into(), "RECOVER".into()),
                    ("KFLR".into(), "KNOWNFAIL".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "RECOVER".into(),
                    map: "RECOVER".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        let session = SessionId::new("r03-unknown-session", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let invocation = server
            .cics_invocation("IBMUSER", "RCVY", Some(artifact_ref))
            .unwrap();
        server
            .cics
            .launch_terminal(invocation, &session, "RCVY", 24, 80, "r03-csrf", 1, 10_000)
            .unwrap();
        server
            .cics
            .inject_file_fault_once(
                CicsOperation::Write,
                "RECFILE",
                mainframe_env_cics::CicsFileFaultPoint::AfterMutation,
            )
            .unwrap();
        let problem = server
            .run_online_exchange(&session, &principal, "RECOVER", 2)
            .unwrap_err();
        assert_eq!(problem, HostProblem::UnknownOutcome);
        assert_eq!(gateway_problem(problem).code, "unknown_outcome");
        let exchange = server.online_exchange(&session).unwrap().unwrap();
        assert!(server.online_exchange_blocked(&exchange).unwrap());
        let key = IdempotencyKey::new(
            exchange.blocking_effect.as_deref().unwrap(),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            server.store.effect(&key).unwrap().unwrap().state,
            EffectState::UnknownOutcome
        );
        assert_eq!(
            server
                .store
                .get_execution(
                    &ExecutionId::new(&exchange.execution_id, InvocationLimits::default()).unwrap(),
                )
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Running
        );
        drop(server);
        drop(first_store);

        let second_store =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second_platform: Arc<dyn PlatformStore> = second_store.clone();
        let server = ProductServer::open(
            server_config,
            second_platform,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        assert_eq!(
            server.run_online_exchange(&session, &principal, "RECOVER", 3),
            Ok(()),
            "authoritatively reconciled session did not resume"
        );
        assert!(server.online_exchange(&session).unwrap().is_none());
        assert_eq!(
            server.store.effect(&key).unwrap().unwrap().state,
            EffectState::Completed
        );
        assert_eq!(
            server
                .store
                .get_execution(
                    &ExecutionId::new(&exchange.execution_id, InvocationLimits::default()).unwrap(),
                )
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let outer_audits = server
            .store
            .audit_records(
                &ExecutionId::new(&exchange.execution_id, InvocationLimits::default()).unwrap(),
                1,
                32,
            )
            .unwrap()
            .into_iter()
            .filter(|record| record.capability.as_str() == "host.cics.execute")
            .map(|record| record.decision)
            .collect::<Vec<_>>();
        assert!(outer_audits.contains(&mainframe_env_execution_api::AuditDecision::UnknownOutcome));
        assert!(outer_audits.contains(&mainframe_env_execution_api::AuditDecision::Success));
        let records = server
            .dataset
            .invoke(DatasetRequest::Read {
                dataset: DatasetName::new("IBMUSER.RECOVERY", 128).unwrap(),
                member: None,
                key: None,
                max_records: 8,
                control: Default::default(),
            })
            .unwrap();
        let DatasetResult::Records { records, .. } = records else {
            panic!("recovery dataset did not return records")
        };
        assert_eq!(
            records.len(),
            1,
            "reconciliation redispatched the committed write"
        );

        let known_session = SessionId::new("r03-known-session", 64).unwrap();
        let known_invocation = server
            .cics_invocation("IBMUSER", "KFLR", Some(known_artifact_ref.clone()))
            .unwrap();
        server
            .cics
            .launch_terminal(
                known_invocation,
                &known_session,
                "KFLR",
                24,
                80,
                "r03-known-csrf",
                4,
                10_000,
            )
            .unwrap();
        let known_context = server
            .cics
            .terminal_execution(&known_session, &principal, 5)
            .unwrap();
        let known_exchange = server
            .begin_online_exchange(&known_session, "KNOWNFAIL", &known_context)
            .unwrap();
        let known_result = server.run_online_exchange(&known_session, &principal, "KNOWNFAIL", 5);
        assert!(
            matches!(
                known_result,
                Err(HostProblem::Condition { response: -4, .. })
            ),
            "known provider failure lost its CICS condition projection: {known_result:?}"
        );
        assert!(server.online_exchange(&known_session).unwrap().is_none());

        // A crash after the Failed journal transition but before cleanup must
        // not convert the known failure into success or feed a terminal row
        // back through resumable execution.
        server
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: ONLINE_EXCHANGE_NAMESPACE.into(),
                    key: known_session.as_str().into(),
                    version: known_exchange.version,
                    payload: encode_online_exchange(&known_exchange).unwrap(),
                },
                None,
            )
            .unwrap();
        server
            .cics
            .restore_terminal_run(
                known_context.invocation.clone(),
                &known_session,
                &known_context.transaction,
                known_context.commarea.clone(),
                6,
            )
            .unwrap();
        stage_protected_start(
            &server,
            &known_context.invocation,
            "KFLR",
            "RCVY",
            "RECVFAIL",
            99,
        );
        assert!(
            server
                .store
                .get_work("cics-start:RECVFAIL")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            server
                .recover_terminal_online_exchange(&known_session, &principal, &known_exchange, 6,)
                .unwrap(),
            Some(TerminalExchangeRecovery::Failed)
        );
        assert!(
            server
                .store
                .get_provider_state("cics-interval-start-v1", "RECVFAIL")
                .unwrap()
                .is_none(),
            "Failed recovery retained the protected START"
        );
        assert!(
            server
                .store
                .get_work("cics-start:RECVFAIL")
                .unwrap()
                .is_none()
        );
        assert!(server.online_exchange(&known_session).unwrap().is_none());
        let retry_invocation = server
            .cics_invocation("IBMUSER", "KFLR", Some(known_artifact_ref))
            .unwrap();
        server
            .cics
            .resume_terminal(retry_invocation, &known_session, "r03-known-csrf", 7)
            .unwrap();
        assert!(
            matches!(
                server.run_online_exchange(&known_session, &principal, "KNOWNFAIL", 8),
                Err(HostProblem::Condition { response: -4, .. })
            ),
            "known terminal failure left a stale online exchange"
        );
        assert!(server.online_exchange(&known_session).unwrap().is_none());
        drop((server, second_store));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn installed_cobswait_runs_by_named_jes_program_and_calls_mvswait() {
        let limits = SourceLimits::default();
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. COBSWAIT.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MVSWAIT-TIME PIC 9(8) COMP.\n01 PARM-VALUE PIC X(8).\nPROCEDURE DIVISION.\nACCEPT PARM-VALUE FROM SYSIN.\nMOVE PARM-VALUE TO MVSWAIT-TIME.\nCALL 'MVSWAIT' USING MVSWAIT-TIME.\nDISPLAY 'WAIT COMPLETE'.\nSTOP RUN.\n";
        let path = LogicalPath::new("COBSWAIT.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "COBSWAIT.cbl",
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
        let compile_result = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = compile_result else {
            panic!("COBSWAIT fixture did not publish: {compile_result:?}");
        };
        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_batch_programs(vec![BatchProgramDefinition {
                name: "COBSWAIT".into(),
                artifact: ArtifactRef::new(
                    format!("sha256:{:x}", Sha256::digest(artifact.payload())),
                    InvocationLimits::default(),
                )
                .unwrap(),
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }])
            .unwrap();
        let response = call(
            &server.router(),
            Method::PUT,
            "/zosmf/restjobs/jobs",
            "//WAITJOB JOB CLASS=A\n//WAIT EXEC PGM=COBSWAIT\n//STEPLIB DD DSN=IBMUSER.LOADLIB,DISP=SHR\n//SYSIN DD *\n00000000\n/*\n",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let job: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65_536).await.unwrap()).unwrap();
        assert_eq!(job["status"], "ACTIVE");
        let completed = wait_for_terminal_job(&server, job["jobid"].as_str().unwrap()).await;
        assert_eq!(completed.return_code, Some(0));

        let abend_source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. ABENDER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 ABCODE PIC S9(9) COMP VALUE 999.\n01 TIMING PIC S9(9) COMP VALUE 0.\nPROCEDURE DIVISION.\nCALL 'CEE3ABD' USING ABCODE TIMING.\nSTOP RUN.\n";
        let path = LogicalPath::new("ABENDER.cbl", limits.max_path_bytes).unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![
                SourceFile::input(
                    "ABENDER.cbl",
                    abend_source.to_vec(),
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
        let CompilerResult::Published {
            artifact: abender, ..
        } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("ABENDER fixture did not publish");
        };
        server
            .install_batch_programs(vec![BatchProgramDefinition {
                name: "ABENDER".into(),
                artifact: ArtifactRef::new(
                    format!("sha256:{:x}", Sha256::digest(abender.payload())),
                    InvocationLimits::default(),
                )
                .unwrap(),
                payload: abender.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(abender.manifest().clone()),
                semantic_identity: abender.semantic_id().to_reference(),
            }])
            .unwrap();
        let response = call(
            &server.router(),
            Method::PUT,
            "/zosmf/restjobs/jobs",
            "//ABENDJOB JOB CLASS=A\n//FAIL EXEC PGM=ABENDER\n",
        )
        .await;
        let job: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65_536).await.unwrap()).unwrap();
        assert_eq!(job["status"], "ACTIVE");
        let completed = wait_for_terminal_job(&server, job["jobid"].as_str().unwrap()).await;
        assert_eq!(completed.abend_code.as_deref(), Some("U0999"));
    }

    #[test]
    fn invocation_grants_are_selector_scoped_and_generation_pinned() {
        let (server, _, _) = worker_test_server(500);
        let invocation = server
            .invocation(
                "IBMUSER",
                "zosmf:dataset",
                ServiceClass::System,
                &["host.dataset.read"],
            )
            .unwrap();
        assert_eq!(invocation.deadline_tick, 500 + server.config.timeout_millis);
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

        let deadline = 1;
        let context = GatewayCallContext::new(deadline).unwrap();
        let cancellation = context.cancellation_probe();
        let _scope = GatewayCallContextScope::enter(context);
        let controlled = server
            .invocation(
                "IBMUSER",
                "zosmf:dataset",
                ServiceClass::System,
                &["host.dataset.read"],
            )
            .unwrap();
        assert_eq!(controlled.deadline_tick, 500 + server.config.timeout_millis);
        assert!(!controlled.cancellation_requested());
        cancellation.request();
        assert!(controlled.cancellation_requested());
    }

    #[test]
    fn direct_audit_age_uses_durable_clock_after_wall_clock_regression() {
        let durable_tick = session_tick().unwrap().saturating_add(1_000_000);
        let mut server_config = config();
        server_config.retention.audit_ticks = 10;
        let store = Arc::new(MemoryStore::new(Default::default()));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let server = ProductServer::open_with_clock(
            server_config,
            platform,
            Arc::new(ManualJesClock::new(durable_tick)),
        )
        .unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let _ = server.resource_decision(
            "IBMUSER",
            "FACILITY",
            "RETENTION.CLOCK.TEST",
            AccessIntent::Read,
        );
        let forecast = server
            .operator_retention_forecast(RetentionTarget::Audit, 0)
            .unwrap();
        assert_eq!(forecast.active_records, 1);
        assert_eq!(forecast.eligible_records, 0);
    }

    #[test]
    fn legacy_console_sidecar_reconciles_and_reclaims_capacity() {
        let mut server_config = config();
        server_config.retention.lifecycle_ticks = 10;
        server_config.retention.max_batch = 8;
        let store = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(ManualJesClock::new(100));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let server =
            ProductServer::open_with_clock(server_config, platform, clock.clone()).unwrap();
        let row = ProviderStateRecord {
            namespace: "console-log".into(),
            key: "0000000000000001".into(),
            version: 1,
            payload: b"OPER\0legacy console".to_vec(),
        };
        store.put_provider_state(row, None).unwrap();
        assert!(
            server
                .retention_planner()
                .unwrap()
                .core_dependencies()
                .unwrap()
                .unowned
        );

        let legacy = server
            .operator_retention_legacy_rows(RetentionTarget::ConsoleLog, 8)
            .unwrap();
        assert_eq!(legacy.len(), 1);
        let receipt = server
            .operator_reconcile_retention_age(RetentionAgeReconciliation {
                target: RetentionTarget::ConsoleLog,
                namespace: legacy[0].namespace.clone(),
                key: legacy[0].key.clone(),
                expected_version: legacy[0].source_version,
                owner_execution: None,
            })
            .unwrap();
        assert_eq!(receipt.reconciled_tick, 100);
        assert!(
            !server
                .retention_planner()
                .unwrap()
                .core_dependencies()
                .unwrap()
                .unowned
        );
        clock.advance(9);
        assert_eq!(
            server
                .operator_archive_and_prune(RetentionTarget::ConsoleLog, 8)
                .unwrap()
                .pruned,
            0
        );
        clock.advance(1);
        assert_eq!(
            server
                .operator_archive_and_prune(RetentionTarget::ConsoleLog, 8)
                .unwrap()
                .pruned,
            1
        );
        assert!(
            store
                .get_provider_state("console-log", "0000000000000001")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn core_retention_requires_a_current_provider_dependency_snapshot() {
        let (server, store, _) = worker_test_server(100);
        let request = RetentionRequest {
            target: RetentionTarget::TerminalExecutions,
            now_tick: 100,
            max_records: 1,
        };
        assert_eq!(
            store.archive_and_prune(server.config.retention.policy().unwrap(), request),
            Err(StoreError::InvalidTransition)
        );
        let snapshot = server
            .retention_planner()
            .unwrap()
            .core_dependencies()
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "snapshot-race".into(),
                    key: "late-writer".into(),
                    version: 1,
                    payload: b"late".to_vec(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store.archive_and_prune_with_dependencies(
                server.config.retention.policy().unwrap(),
                request,
                &snapshot,
            ),
            Err(StoreError::Conflict)
        );
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
        let token;
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
            let verified = server.verify("IBMUSER", b"TESTPASS").unwrap();
            let original = server.create_session(&verified).unwrap();
            token = server.rotate_session(&original).unwrap().1;
            assert_eq!(
                server.principal(Authentication::Bearer(original)),
                Err(HostProblem::Unauthorized)
            );
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
                    Authentication::Bearer(token),
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

    // Regression for #181: CicsResume rejected the ordinary pseudo-conversational
    // hand-off between two different online transactions (EXEC CICS RETURN
    // TRANSID(x) followed by the terminal resuming into transaction x) as a 503
    // infrastructure_failure. The handler compared resume_terminal's admitted
    // transaction against the terminal's stale pre-resume snapshot instead of
    // re-resolving the online program for the transaction actually resumed.
    #[test]
    fn cics_resume_follows_return_transid_to_a_different_transaction() {
        fn compile(name: &str, source: &[u8]) -> PublishedArtifact {
            let limits = SourceLimits::default();
            let file_name = format!("{name}.cbl");
            let path = LogicalPath::new(file_name.clone(), limits.max_path_bytes).unwrap();
            let bundle = SourceBundle::new(
                &path,
                vec![
                    SourceFile::input(
                        file_name,
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
                panic!("{name} fixture did not publish");
            };
            artifact
        }

        let from_artifact = compile(
            "XFERFROM",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. XFERFROM.\nPROCEDURE DIVISION.\nEXEC CICS RETURN TRANSID('XFTO') END-EXEC.\n",
        );
        let to_artifact = compile(
            "XFERTO",
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. XFERTO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MSG PIC X(5) VALUE 'HELLO'.\nPROCEDURE DIVISION.\nEXEC CICS SEND TEXT FROM(MSG) END-EXEC.\nEXEC CICS RETURN END-EXEC.\n",
        );

        let server = ProductServer::memory(config()).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        let from_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(from_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let to_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(to_artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![
                    OnlineProgramDefinition {
                        name: "XFERFROM".into(),
                        artifact: from_ref.clone(),
                        payload: from_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(from_artifact.manifest().clone()),
                        semantic_identity: from_artifact.semantic_id().to_reference(),
                    },
                    OnlineProgramDefinition {
                        name: "XFERTO".into(),
                        artifact: to_ref.clone(),
                        payload: to_artifact.payload().to_vec(),
                        manifest: VersionedArtifactManifest::V3(to_artifact.manifest().clone()),
                        semantic_identity: to_artifact.semantic_id().to_reference(),
                    },
                ],
                transactions: BTreeMap::from([
                    ("XFFR".into(), "XFERFROM".into()),
                    ("XFTO".into(), "XFERTO".into()),
                ]),
                maps: vec![BmsMapDefinition {
                    mapset: "XFERTO".into(),
                    map: "XFERTO".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();

        let launch = server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::CicsLaunch {
                    transaction: "XFFR".into(),
                    rows: 24,
                    columns: 80,
                },
            )
            .unwrap();
        let mainframe_env_zosmf::GatewayBody::Json(launched) = launch.body else {
            panic!("launch response was not JSON")
        };
        let session = launched["session"].as_str().unwrap().to_string();
        let csrf_token = launched["csrf_token"].as_str().unwrap().to_string();

        // XFERFROM's own RETURN TRANSID('XFTO') already ran during launch, so
        // the pending continuation now targets transaction XFTO while the
        // terminal's own snapshot still reports the launch transaction XFFR.
        let sends_before = server
            .online_operation_count(CicsOperation::SendText)
            .unwrap();
        let response = server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::CicsResume { session, csrf_token },
            )
            .expect(
                "resume must follow resume_terminal's admitted transaction (XFTO) instead of \
                 rejecting the terminal's stale pre-resume snapshot (XFFR) as an infrastructure failure",
            );
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            server
                .online_operation_count(CicsOperation::SendText)
                .unwrap(),
            sends_before + 1,
            "XFERTO did not run after the XFFR -> XFTO transaction hand-off"
        );
    }
}
