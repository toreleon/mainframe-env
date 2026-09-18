#[path = "handlers/mod.rs"]
mod handlers;

use crate::generated::{CICS_COMMAND_DESCRIPTORS, CicsCommandFamily, command_descriptor};
use crate::retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, DecodedUow,
    UowRetentionMetadata,
};
pub use handlers::*;
use handlers::{
    decode_terminal_address, encode_terminal_address, terminal_field_address, validate_map,
};
pub(super) use handlers::{field, store_error};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    PrincipalId, RunUnitId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsDisposition, CicsOperation, CicsRequest, CicsResponse,
    CicsUnitOfWorkOutcome, DatasetName, EffectRequest, EffectResult, HostProblem, HostProvider,
    HostRequest, HostResult, Mutation, ResourceName, ScopedHostService, SecurityDecision,
    SecurityRequest, SessionId, canonical_request_digest, canonical_result_digest,
};
#[cfg(test)]
use mainframe_env_host_api::{
    CicsConditionPolicy, ClockRequest, DatasetRequest, DatasetResult, ProgramRequest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError, WorkStore,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsLimits {
    pub max_sessions: usize,
    pub max_runs: usize,
    pub max_maps: usize,
    pub max_programs: usize,
    pub max_file_aliases: usize,
    pub max_enqueue_models: usize,
    pub max_fields: usize,
    pub max_screen_bytes: usize,
    pub max_queue_records: usize,
    pub max_queue_bytes: usize,
}

impl Default for CicsLimits {
    fn default() -> Self {
        Self {
            max_sessions: 4096,
            max_runs: 4096,
            max_maps: 1024,
            max_programs: 4096,
            max_file_aliases: 1024,
            max_enqueue_models: 1024,
            max_fields: 512,
            max_screen_bytes: 4 * 1024 * 1024,
            max_queue_records: 65536,
            max_queue_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsFieldDefinition {
    pub name: String,
    pub row: u16,
    pub column: u16,
    pub length: u16,
    pub initial: Vec<u8>,
    pub color: Option<String>,
    pub highlight: Option<String>,
    pub protected: bool,
    pub secret: bool,
    pub fset: bool,
    pub justify_right: bool,
    pub fill_zero: bool,
    pub output_offset: Option<u32>,
    pub attribute_offset: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsMapDefinition {
    pub mapset: String,
    pub map: String,
    /// One-based terminal line at which the map is positioned.
    pub line: u16,
    /// One-based terminal column at which the map is positioned.
    pub column: u16,
    pub rows: u16,
    pub columns: u16,
    pub fields: Vec<BmsFieldDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsFileDefinition {
    pub dataset: DatasetName,
    pub ccsid: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsFileStatus {
    Open,
    /// The file is closed and enabled, so the next online file request auto-opens it.
    ClosedEnabled,
    /// The file is closed and unavailable to implicit open; file requests receive NOTOPEN.
    Closed,
    /// The file is disabled; this is distinct from the CLOSED + UNENABLED state.
    Disabled,
}

#[derive(Clone, Copy, Debug)]
struct DurableFileStatus {
    status: CicsFileStatus,
    version: u64,
}

pub(crate) struct CicsEffectReplay {
    pub(crate) effect_key: Option<String>,
    pub(crate) owner_execution: Option<String>,
    pub(crate) owner_run_unit: Option<String>,
    pub(crate) sequence: Option<u64>,
    pub(crate) deadline_tick: Option<u64>,
    pub(crate) resolution_tick: Option<u64>,
    pub(crate) request_digest: [u8; 32],
    pub(crate) result_digest: Option<[u8; 32]>,
    pub(crate) binding_digest: Option<[u8; 32]>,
    pub(crate) response: CicsResponse,
}

#[derive(Clone, Debug)]
struct Session {
    rows: u16,
    columns: u16,
    principal: String,
    transaction: String,
    run_unit: String,
    user_corr_data: Vec<u8>,
    user_corr_effect_key: Option<String>,
    user_corr_request_digest: Option<[u8; 32]>,
    csrf_sha256: String,
    idle_timeout_ticks: u64,
    expires_at_tick: u64,
    connected: bool,
    aid: u8,
    screen: Vec<u8>,
    input: Option<Vec<u8>>,
    suspended: bool,
    mapset: Option<String>,
    map: Option<String>,
    field_protection: BTreeMap<String, bool>,
    field_modified: BTreeMap<String, bool>,
    field_values: BTreeMap<String, Vec<u8>>,
    handle_state: handlers::HandleState,
    version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTerminalSnapshot {
    pub session: String,
    pub principal: String,
    pub transaction: String,
    pub run_unit: String,
    pub rows: u16,
    pub columns: u16,
    pub aid: u8,
    pub screen: Vec<u8>,
    pub mapset: Option<String>,
    pub map: Option<String>,
    pub suspended: bool,
    pub connected: bool,
    pub expires_at_tick: u64,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTerminalExecution {
    pub invocation: Invocation,
    pub transaction: String,
    pub commarea: Vec<u8>,
    pub aid: u8,
}

#[derive(Clone)]
struct Run {
    invocation: Invocation,
    current_program: Option<String>,
    session: String,
    transaction: String,
    applid: String,
    sysid: String,
    originating_task: String,
    host_sequence: u64,
    outer_effect_key: Option<String>,
    handlers: BTreeMap<String, String>,
    aid_handlers: BTreeMap<String, String>,
    ignored_conditions: BTreeSet<String>,
    abend_handler: Option<handlers::AbendExit>,
    cancelled_abend_handler: Option<handlers::AbendExit>,
    handle_stack: Vec<handlers::HandleFrame>,
    /// Latest explicit application ABEND for source-faithful ASSIGN outputs.
    /// It is restored only from the versioned session handle-state authority;
    /// machine-check diagnostics remain a separate unsupported context.
    latest_abend: Option<handlers::AbendRecord>,
    retrieve: Vec<u8>,
    current_records: BTreeMap<String, Vec<u8>>,
    current_record_values: BTreeMap<String, Vec<u8>>,
    undo: Vec<DatasetUndo>,
    undo_version: Option<u64>,
    browses: BTreeMap<String, String>,
    trace: Vec<CicsTraceEntry>,
}

#[derive(Clone, Debug)]
enum DatasetUndo {
    Restore {
        dataset: DatasetName,
        key: Vec<u8>,
        record: Vec<u8>,
    },
    Delete {
        dataset: DatasetName,
        key: Vec<u8>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTraceEntry {
    pub operation: CicsOperation,
    pub outcome: String,
    pub response: i32,
    pub response2: i32,
    pub payload_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsContinuation {
    pub transaction: String,
    pub commarea: Vec<u8>,
}

#[derive(Clone, Debug)]
struct DurableContinuation {
    transaction: String,
    commarea: Vec<u8>,
    claimed_by: Option<String>,
    effect_key: String,
    version: u64,
}

#[derive(Clone, Debug)]
struct TransientQueue {
    records: Vec<(String, Vec<u8>)>,
    version: u64,
}

struct State {
    sessions: BTreeMap<String, Session>,
    runs: BTreeMap<RunUnitId, Run>,
    maps: BTreeMap<(String, String), BmsMapDefinition>,
    programs: BTreeSet<String>,
    file_aliases: BTreeMap<String, CicsFileDefinition>,
    file_statuses: BTreeMap<String, DurableFileStatus>,
    enqueue_models: BTreeMap<String, CicsEnqueueModelDefinition>,
    continuations: BTreeMap<String, DurableContinuation>,
    transient: BTreeMap<String, TransientQueue>,
    transient_bytes: usize,
    // Internal authority for the declared records-core slice. Command handlers
    // remain deliberately disconnected until the producer/consumer slices seal.
    #[allow(dead_code)]
    interval_records: BTreeMap<String, handlers::IntervalStartRecord>,
    #[cfg(feature = "fault-injection")]
    file_failure: Option<(CicsOperation, String)>,
    #[cfg(feature = "fault-injection")]
    file_fault: Option<(CicsOperation, String, CicsFileFaultPoint)>,
    #[cfg(feature = "fault-injection")]
    mutation_fault: Option<CicsOperation>,
}

#[cfg(feature = "fault-injection")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsFileFaultPoint {
    BeforeIntent,
    AfterIntent,
    AfterMutation,
}

pub struct CicsService {
    host: Arc<ScopedHostService>,
    store: Arc<dyn ProviderStateStore>,
    limits: CicsLimits,
    state: Mutex<State>,
    replay_clock: Option<Arc<dyn CicsReplayClock>>,
    work_store: Option<Arc<dyn WorkStore>>,
    replay_unknown_after_persist: AtomicBool,
}

/// Trusted durable logical-time source used after CICS work is durably resolved.
pub trait CicsReplayClock: Send + Sync {
    /// Observe the current nonzero durable logical tick.
    fn now_tick(&self) -> Result<u64, HostProblem>;
}

impl CicsService {
    fn invoke_host(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
    ) -> EffectResult {
        ScopedHostService::invoke(
            &self.host,
            invocation,
            now_tick,
            cancellation_requested,
            request,
        )
        .persist_with(|audit| self.store.record_audit(audit).map_err(store_error))
    }

    pub fn open(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        limits: CicsLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(host, store, limits, None, None)
    }

    /// Open with the durable clock used to resolve outer replay and UOW age.
    pub fn open_with_replay_clock(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        limits: CicsLimits,
        replay_clock: Arc<dyn CicsReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(host, store, limits, Some(replay_clock), None)
    }

    fn open_inner(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        limits: CicsLimits,
        replay_clock: Option<Arc<dyn CicsReplayClock>>,
        work_store: Option<Arc<dyn WorkStore>>,
    ) -> Result<Arc<Self>, HostProblem> {
        let mut sessions = BTreeMap::new();
        for row in store
            .list_provider_state("cics-session", limits.max_sessions)
            .map_err(store_error)?
        {
            sessions.insert(row.key, decode_session(&row.payload, row.version, limits)?);
        }
        let mut continuations = BTreeMap::new();
        for row in store
            .list_provider_state("cics-continuation", limits.max_sessions)
            .map_err(store_error)?
        {
            continuations.insert(
                row.key,
                decode_continuation(&row.payload, row.version, limits)?,
            );
        }
        let mut maps = BTreeMap::new();
        for row in store
            .list_provider_state("cics-map", limits.max_maps)
            .map_err(store_error)?
        {
            let map = decode_map(&row.payload, limits)?;
            let key = (map.mapset.clone(), map.map.clone());
            if row.key != map_key(&map.mapset, &map.map) || maps.insert(key, map).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut programs = BTreeSet::new();
        for row in store
            .list_provider_state("cics-program", limits.max_programs)
            .map_err(store_error)?
        {
            if !row.payload.is_empty()
                || normalize_terminal_name(&row.key, 128).is_err()
                || !programs.insert(row.key)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut file_aliases = BTreeMap::new();
        for row in store
            .list_provider_state("cics-file-alias", limits.max_file_aliases)
            .map_err(store_error)?
        {
            let definition = decode_file_definition(&row.payload)?;
            file_aliases.insert(row.key, definition);
        }
        let mut file_statuses = BTreeMap::new();
        for row in store
            .list_provider_state("cics-file-status", limits.max_file_aliases)
            .map_err(store_error)?
        {
            if !file_aliases.contains_key(&row.key) {
                return Err(HostProblem::InfrastructureFailure);
            }
            file_statuses.insert(
                row.key,
                DurableFileStatus {
                    status: decode_file_status(&row.payload)?,
                    version: row.version,
                },
            );
        }
        let enqueue_models = handlers::load_enqueue_models(store.as_ref(), limits)?;
        let mut transient = BTreeMap::new();
        let mut transient_bytes = 0usize;
        for row in store
            .list_provider_state("cics-tdq", limits.max_queue_records)
            .map_err(store_error)?
        {
            let queue = decode_transient(&row.payload, row.version, limits)?;
            transient_bytes = transient_bytes
                .checked_add(
                    queue
                        .records
                        .iter()
                        .map(|(_, value)| value.len())
                        .sum::<usize>(),
                )
                .ok_or(HostProblem::ResourceExhausted)?;
            if transient.insert(row.key, queue).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        handlers::validate_enqueue_store(store.as_ref(), limits)?;
        let interval_records = handlers::load_interval_records(store.as_ref(), limits)?;
        Ok(Arc::new(Self {
            host,
            store,
            limits,
            replay_clock,
            work_store,
            replay_unknown_after_persist: AtomicBool::new(false),
            state: Mutex::new(State {
                sessions,
                runs: BTreeMap::new(),
                maps,
                programs,
                file_aliases,
                file_statuses,
                enqueue_models,
                continuations,
                transient,
                transient_bytes,
                interval_records,
                #[cfg(feature = "fault-injection")]
                file_failure: None,
                #[cfg(feature = "fault-injection")]
                file_fault: None,
                #[cfg(feature = "fault-injection")]
                mutation_fault: None,
            }),
        }))
    }

    #[cfg(feature = "fault-injection")]
    pub fn inject_file_failure_once(
        &self,
        operation: CicsOperation,
        file: &str,
    ) -> Result<(), HostProblem> {
        if !matches!(
            operation,
            CicsOperation::Read
                | CicsOperation::Write
                | CicsOperation::Rewrite
                | CicsOperation::Delete
                | CicsOperation::StartBrowse
                | CicsOperation::ReadNext
                | CicsOperation::ReadPrev
                | CicsOperation::EndBrowse
        ) {
            return Err(HostProblem::Malformed);
        }
        let file = normalize_terminal_name(file, 16)?;
        let mut state = self.lock()?;
        if state.file_failure.replace((operation, file)).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    #[cfg(feature = "fault-injection")]
    pub fn inject_file_fault_once(
        &self,
        operation: CicsOperation,
        file: &str,
        point: CicsFileFaultPoint,
    ) -> Result<(), HostProblem> {
        if !matches!(
            operation,
            CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
        ) {
            return Err(HostProblem::Malformed);
        }
        let file = normalize_terminal_name(file, 16)?;
        let mut state = self.lock()?;
        if state.file_fault.replace((operation, file, point)).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    /// Stop after a mutating CICS result is durably replayable but before the
    /// caller observes it. This models the exact process-crash gap shared by
    /// file, queue, program, and unit-of-work operations.
    #[cfg(feature = "fault-injection")]
    pub fn inject_mutation_fault_once(&self, operation: CicsOperation) -> Result<(), HostProblem> {
        if !operation.is_mutating() {
            return Err(HostProblem::Malformed);
        }
        let mut state = self.lock()?;
        if state.mutation_fault.replace(operation).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    /// Inject the exact crash gap after a protected outer replay write.
    #[cfg(any(test, feature = "fault-injection"))]
    pub fn inject_replay_unknown_after_persist_once(&self) {
        self.replay_unknown_after_persist
            .store(true, Ordering::SeqCst);
    }

    pub fn create_session(
        &self,
        session: &SessionId,
        rows: u16,
        columns: u16,
    ) -> Result<(), HostProblem> {
        if rows == 0 || columns == 0 {
            return Err(HostProblem::Malformed);
        }
        let screen_bytes = usize::from(rows)
            .checked_mul(usize::from(columns))
            .ok_or(HostProblem::ResourceExhausted)?;
        if screen_bytes > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let created = Session {
            rows,
            columns,
            principal: String::new(),
            transaction: String::new(),
            run_unit: String::new(),
            user_corr_data: Vec::new(),
            user_corr_effect_key: None,
            user_corr_request_digest: None,
            csrf_sha256: String::new(),
            idle_timeout_ticks: u64::MAX,
            expires_at_tick: u64::MAX,
            connected: true,
            aid: 0,
            screen: Vec::new(),
            input: None,
            suspended: false,
            mapset: None,
            map: None,
            field_protection: BTreeMap::new(),
            field_modified: BTreeMap::new(),
            field_values: BTreeMap::new(),
            handle_state: handlers::HandleState::default(),
            version: 1,
        };
        let mut state = self.lock()?;
        if state.sessions.len() >= self.limits.max_sessions {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.sessions.contains_key(session.as_str()) {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_session(session.as_str(), &created, None)?;
        state.sessions.insert(session.as_str().into(), created);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn launch_terminal(
        &self,
        invocation: Invocation,
        session: &SessionId,
        transaction: &str,
        rows: u16,
        columns: u16,
        csrf_token: &str,
        now_tick: u64,
        idle_timeout_ticks: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        validate_terminal_identity(transaction, 16)?;
        validate_terminal_identity(csrf_token, 256)?;
        if rows == 0 || columns == 0 || idle_timeout_ticks == 0 {
            return Err(HostProblem::Malformed);
        }
        let screen_bytes = usize::from(rows)
            .checked_mul(usize::from(columns))
            .ok_or(HostProblem::ResourceExhausted)?;
        if screen_bytes > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        self.authorize_terminal(&invocation, transaction)?;
        let expires_at_tick = now_tick
            .checked_add(idle_timeout_ticks)
            .ok_or(HostProblem::ResourceExhausted)?;
        let created = Session {
            rows,
            columns,
            principal: invocation.principal.id().as_str().into(),
            transaction: transaction.to_ascii_uppercase(),
            run_unit: invocation.run_unit_id.as_str().into(),
            user_corr_data: Vec::new(),
            user_corr_effect_key: None,
            user_corr_request_digest: None,
            csrf_sha256: terminal_secret_digest(csrf_token),
            idle_timeout_ticks,
            expires_at_tick,
            connected: true,
            aid: 0,
            screen: Vec::new(),
            input: None,
            suspended: false,
            mapset: None,
            map: None,
            field_protection: BTreeMap::new(),
            field_modified: BTreeMap::new(),
            field_values: BTreeMap::new(),
            handle_state: handlers::HandleState::default(),
            version: 1,
        };
        let run = handlers::new_run(
            invocation.clone(),
            session.as_str(),
            transaction,
            "ME01",
            "S001",
        );
        let mut state = self.lock()?;
        if state.sessions.len() >= self.limits.max_sessions
            || state.runs.len() >= self.limits.max_runs
        {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.sessions.contains_key(session.as_str())
            || state.runs.contains_key(&invocation.run_unit_id)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_session(session.as_str(), &created, None)?;
        state
            .sessions
            .insert(session.as_str().into(), created.clone());
        state.runs.insert(invocation.run_unit_id.clone(), run);
        Ok(terminal_snapshot(session.as_str(), &created))
    }

    pub fn terminal_snapshot(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        self.public_session(session, principal, None, now_tick)
            .map(|value| terminal_snapshot(session.as_str(), &value))
    }

    /// Validate ownership and the terminal anti-forgery token without changing
    /// the conversation. Durable online recovery uses this before resuming an
    /// exchange that was already admitted before a process boundary.
    pub fn validate_terminal_resume(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        self.public_session(session, principal, Some(csrf_token), now_tick)
            .map(|value| terminal_snapshot(session.as_str(), &value))
    }

    pub fn terminal_field_protected(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        field: &str,
        now_tick: u64,
    ) -> Result<bool, HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let state = self.lock()?;
        let (mapset, map) = current
            .mapset
            .as_ref()
            .zip(current.map.as_ref())
            .ok_or(HostProblem::NotFound)?;
        let definition = state
            .maps
            .get(&(mapset.clone(), map.clone()))
            .and_then(|map| {
                map.fields
                    .iter()
                    .find(|definition| definition.name.eq_ignore_ascii_case(field))
            })
            .ok_or(HostProblem::NotFound)?;
        Ok(current
            .field_protection
            .get(&definition.name.to_ascii_uppercase())
            .copied()
            .unwrap_or(definition.protected))
    }

    pub fn terminal_execution(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<CicsTerminalExecution, HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let state = self.lock()?;
        let run = state
            .runs
            .get(&run_id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if run.session != session.as_str()
            || run.invocation.principal.id() != principal
            || run.transaction != current.transaction
        {
            return Err(HostProblem::Unauthorized);
        }
        Ok(CicsTerminalExecution {
            invocation: run.invocation.clone(),
            transaction: run.transaction.clone(),
            commarea: run.retrieve.clone(),
            aid: current.aid,
        })
    }

    pub fn submit_terminal_input(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        aid: u8,
        fields: &BTreeMap<String, Vec<u8>>,
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        if !handlers::valid_terminal_aid(aid) || fields.len() > self.limits.max_fields {
            return Err(HostProblem::Malformed);
        }
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        let mut encoded = Vec::new();
        let mut normalized = BTreeMap::new();
        {
            let state = self.lock()?;
            let (mapset, map) = current
                .mapset
                .as_ref()
                .zip(current.map.as_ref())
                .ok_or(HostProblem::NotFound)?;
            let definition = state
                .maps
                .get(&(mapset.clone(), map.clone()))
                .ok_or(HostProblem::InfrastructureFailure)?;
            let displayed = decode_map_payload(&current.screen, self.limits)?;
            for (name, value) in fields {
                let definition = definition
                    .fields
                    .iter()
                    .find(|field| field.name.eq_ignore_ascii_case(name))
                    .ok_or(HostProblem::Malformed)?;
                let protected = current
                    .field_protection
                    .get(&definition.name.to_ascii_uppercase())
                    .copied()
                    .unwrap_or(definition.protected);
                let unchanged = displayed
                    .get(&definition.name.to_ascii_uppercase())
                    .is_some_and(|displayed| terminal_values_equal(displayed, value));
                if (protected && (definition.secret || !unchanged))
                    || value.len() > usize::from(definition.length)
                {
                    return Err(HostProblem::Unauthorized);
                }
                if normalized
                    .insert(name.to_ascii_uppercase(), value.clone())
                    .is_some()
                {
                    return Err(HostProblem::Malformed);
                }
            }
            for field_definition in &definition.fields {
                let name = field_definition.name.to_ascii_uppercase();
                if current.field_modified.get(&name).copied().unwrap_or(false)
                    && !normalized.contains_key(&name)
                {
                    let value = current
                        .field_values
                        .get(&name)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    if value.len() > usize::from(field_definition.length) {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                    normalized.insert(name, value.clone());
                }
            }
            if normalized.len() > self.limits.max_fields {
                return Err(HostProblem::ResourceExhausted);
            }
        }
        for (name, value) in &normalized {
            validate_terminal_identity(name, 32)?;
            if value.len() > self.limits.max_screen_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            field(&mut encoded, name.as_bytes())?;
            field(&mut encoded, value)?;
        }
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.expires_at_tick = now_tick
            .checked_add(next.idle_timeout_ticks)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.aid = aid;
        next.input = Some(encoded);
        next.suspended = false;
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_session(session.as_str(), &next, Some(current.version))?;
        state.sessions.insert(session.as_str().into(), next.clone());
        Ok(terminal_snapshot(session.as_str(), &next))
    }

    pub fn resume_terminal(
        &self,
        invocation: Invocation,
        session: &SessionId,
        csrf_token: &str,
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        let principal = invocation.principal.id();
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        self.authorize_terminal(&invocation, &current.transaction)?;
        let run_unit = invocation.run_unit_id.as_str().to_string();
        let has_continuation = self.lock()?.continuations.contains_key(session.as_str());
        let resumed_transaction = if has_continuation {
            let continuation = self.claim_continuation(invocation, session, "ME01", "S001")?;
            let transaction = continuation.transaction;
            self.lock()?
                .runs
                .get_mut(
                    &RunUnitId::new(&run_unit, InvocationLimits::default())
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )
                .ok_or(HostProblem::InfrastructureFailure)?
                .retrieve = continuation.commarea;
            transaction
        } else {
            self.register_run(invocation, session, &current.transaction, "ME01", "S001")?;
            current.transaction.clone()
        };
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.expires_at_tick = now_tick
            .checked_add(next.idle_timeout_ticks)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.run_unit = run_unit;
        next.transaction = resumed_transaction;
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_session(session.as_str(), &next, Some(current.version))?;
        state.sessions.insert(session.as_str().into(), next.clone());
        Ok(terminal_snapshot(session.as_str(), &next))
    }

    pub fn disconnect_terminal(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        for run in state
            .runs
            .values()
            .filter(|run| run.session == session.as_str())
            .cloned()
            .collect::<Vec<_>>()
        {
            handlers::release_task_state(self, &run)?;
        }
        self.store
            .delete_provider_state("cics-session", session.as_str(), current.version)
            .map_err(store_error)?;
        state.sessions.remove(session.as_str());
        state.runs.retain(|_, run| run.session != session.as_str());
        Ok(())
    }

    pub fn tn3270_screen(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<Vec<u8>, HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let (mapset, map) = current
            .mapset
            .as_ref()
            .zip(current.map.as_ref())
            .ok_or(HostProblem::NotFound)?;
        let state = self.lock()?;
        let definition = state
            .maps
            .get(&(mapset.clone(), map.clone()))
            .ok_or(HostProblem::InfrastructureFailure)?;
        encode_tn3270_screen(&current, definition, self.limits)
    }

    pub fn submit_tn3270(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        record: &[u8],
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        if record.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        let (mapset, map) = current
            .mapset
            .as_ref()
            .zip(current.map.as_ref())
            .ok_or(HostProblem::NotFound)?;
        let fields = {
            let state = self.lock()?;
            let definition = state
                .maps
                .get(&(mapset.clone(), map.clone()))
                .ok_or(HostProblem::InfrastructureFailure)?;
            decode_tn3270_input(record, &current, definition, self.limits)?
        };
        self.submit_terminal_input(session, principal, csrf_token, record[0], &fields, now_tick)
    }

    #[must_use]
    pub const fn active_worker_count(&self) -> usize {
        0
    }

    pub fn register_run(
        &self,
        invocation: Invocation,
        session: &SessionId,
        transaction: &str,
        applid: &str,
        sysid: &str,
    ) -> Result<(), HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        let values = [transaction, applid, sysid];
        if values.iter().any(|value| {
            value.is_empty()
                || value.len() > 16
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }) {
            return Err(HostProblem::Malformed);
        }
        let (undo, undo_version) = self.load_undo(&invocation.run_unit_id)?;
        let mut state = self.lock()?;
        if !state.sessions.contains_key(session.as_str()) {
            return Err(HostProblem::NotFound);
        }
        if state.runs.len() >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.runs.contains_key(&invocation.run_unit_id) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let retrieve = invocation
            .bindings
            .get("cics.retrieve")
            .map(|value| value.bytes().to_vec())
            .unwrap_or_default();
        let originating_task = originating_task_for(
            state
                .sessions
                .get(session.as_str())
                .ok_or(HostProblem::NotFound)?,
            &invocation,
        );
        let handle_state = state
            .sessions
            .get(session.as_str())
            .ok_or(HostProblem::NotFound)?
            .handle_state
            .clone();
        state.runs.insert(
            invocation.run_unit_id.clone(),
            handlers::new_run_with_state(
                invocation,
                session.as_str(),
                transaction,
                applid,
                sysid,
                handlers::RunSeed {
                    originating_task,
                    retrieve,
                    undo,
                    undo_version,
                    handle_state,
                },
            ),
        );
        Ok(())
    }

    /// Rebuild volatile CICS run state for the exact durable terminal run.
    ///
    /// The machine is replayed through its durable coordinator after this
    /// call. Durable HANDLE state and undo data are reloaded, while cursors
    /// remain transient. Session, run, and principal identity are checked
    /// before replacing any abandoned in-memory copy.
    pub fn restore_terminal_run(
        &self,
        invocation: Invocation,
        session: &SessionId,
        transaction: &str,
        retrieve: Vec<u8>,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        validate_terminal_identity(transaction, 16)?;
        if retrieve.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = self.public_session(session, invocation.principal.id(), None, now_tick)?;
        if current.run_unit != invocation.run_unit_id.as_str()
            || current.transaction != transaction.to_ascii_uppercase()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let (undo, undo_version) = self.load_undo(&invocation.run_unit_id)?;
        let mut state = self.lock()?;
        state.runs.retain(|_, run| run.session != session.as_str());
        if state.runs.len() >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        state.runs.insert(
            invocation.run_unit_id.clone(),
            handlers::new_run_with_state(
                invocation,
                session.as_str(),
                transaction,
                "ME01",
                "S001",
                handlers::RunSeed {
                    originating_task: current.run_unit,
                    retrieve,
                    undo,
                    undo_version,
                    handle_state: current.handle_state,
                },
            ),
        );
        Ok(())
    }

    pub fn claim_continuation(
        &self,
        invocation: Invocation,
        session: &SessionId,
        applid: &str,
        sysid: &str,
    ) -> Result<CicsContinuation, HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        let values = [applid, sysid];
        if values.iter().any(|value| {
            value.is_empty()
                || value.len() > 16
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }) {
            return Err(HostProblem::Malformed);
        }
        let (undo, undo_version) = self.load_undo(&invocation.run_unit_id)?;
        let mut state = self.lock()?;
        if !state.sessions.contains_key(session.as_str()) {
            return Err(HostProblem::NotFound);
        }
        if state.runs.len() >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.runs.contains_key(&invocation.run_unit_id) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current = state
            .continuations
            .get(session.as_str())
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let run_id = invocation.run_unit_id.as_str();
        let next = match current.claimed_by.as_deref() {
            None => {
                let mut next = current.clone();
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                next.claimed_by = Some(run_id.into());
                self.persist_continuation(session.as_str(), &next, Some(current.version))?;
                state
                    .continuations
                    .insert(session.as_str().into(), next.clone());
                next
            }
            Some(claimed) if claimed == run_id => current,
            Some(_) => return Err(HostProblem::IdempotencyConflict),
        };
        let continuation = CicsContinuation {
            transaction: next.transaction.clone(),
            commarea: next.commarea.clone(),
        };
        let originating_task = originating_task_for(
            state
                .sessions
                .get(session.as_str())
                .ok_or(HostProblem::NotFound)?,
            &invocation,
        );
        state.runs.insert(
            invocation.run_unit_id.clone(),
            handlers::new_run_with_state(
                invocation,
                session.as_str(),
                &next.transaction,
                applid,
                sysid,
                handlers::RunSeed {
                    originating_task,
                    retrieve: Vec::new(),
                    undo,
                    undo_version,
                    handle_state: handlers::HandleState::default(),
                },
            ),
        );
        Ok(continuation)
    }

    fn ensure_run(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        reject_reserved_nested_origin(invocation)?;
        let mut state = self.lock()?;
        if handlers::synchronize_current_program(&mut state.runs, invocation)? {
            return Ok(());
        }
        drop(state);
        let session_name = invocation
            .bindings
            .get("cics.session")
            .map(|value| String::from_utf8_lossy(value.bytes()).into_owned())
            .unwrap_or_else(|| format!("session-{}", invocation.run_unit_id));
        let session = SessionId::new(session_name, InvocationLimits::default().max_binding_bytes)
            .map_err(|_| HostProblem::ResourceExhausted)?;
        match self.create_session(&session, 24, 80) {
            Ok(()) | Err(HostProblem::IdempotencyConflict) => {}
            Err(problem) => return Err(problem),
        }
        let transaction = invocation
            .bindings
            .get("cics.transaction")
            .and_then(|payload| std::str::from_utf8(payload.bytes()).ok())
            .unwrap_or("DEFAULT");
        match self.register_run(invocation.clone(), &session, transaction, "ME01", "S001") {
            Ok(()) | Err(HostProblem::IdempotencyConflict) => Ok(()),
            Err(problem) => Err(problem),
        }
    }

    pub fn register_map(&self, definition: BmsMapDefinition) -> Result<(), HostProblem> {
        validate_map(&definition, self.limits)?;
        let mut definition = definition;
        definition.mapset = definition.mapset.to_ascii_uppercase();
        definition.map = definition.map.to_ascii_uppercase();
        for field in &mut definition.fields {
            field.name = field.name.to_ascii_uppercase();
        }
        let mut state = self.lock()?;
        let key = (definition.mapset.clone(), definition.map.clone());
        if let Some(existing) = state.maps.get(&key) {
            return if existing == &definition {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if state.maps.len() >= self.limits.max_maps {
            return Err(HostProblem::ResourceExhausted);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-map".into(),
                    key: map_key(&definition.mapset, &definition.map),
                    version: 1,
                    payload: encode_map(&definition)?,
                },
                None,
            )
            .map_err(store_error)?;
        state.maps.insert(key, definition);
        Ok(())
    }

    pub fn register_programs(&self, programs: &BTreeSet<String>) -> Result<(), HostProblem> {
        let normalized = programs
            .iter()
            .map(|program| normalize_terminal_name(program, 128))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if normalized.len() != programs.len() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut state = self.lock()?;
        let additions = normalized
            .iter()
            .filter(|program| !state.programs.contains(*program))
            .count();
        if state
            .programs
            .len()
            .checked_add(additions)
            .is_none_or(|total| total > self.limits.max_programs)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let writes = normalized
            .iter()
            .filter(|program| !state.programs.contains(*program))
            .map(|program| ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-program".into(),
                    key: program.clone(),
                    version: 1,
                    payload: Vec::new(),
                },
                expected_version: None,
            })
            .collect::<Vec<_>>();
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        state.programs.extend(normalized);
        Ok(())
    }

    pub fn register_file_aliases(
        &self,
        aliases: &BTreeMap<String, DatasetName>,
    ) -> Result<(), HostProblem> {
        self.register_file_definitions(
            &aliases
                .iter()
                .map(|(name, dataset)| {
                    (
                        name.clone(),
                        CicsFileDefinition {
                            dataset: dataset.clone(),
                            ccsid: None,
                        },
                    )
                })
                .collect(),
        )
    }

    pub fn register_file_definitions(
        &self,
        aliases: &BTreeMap<String, CicsFileDefinition>,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let new_aliases = aliases
            .keys()
            .filter(|name| !state.file_aliases.contains_key(*name))
            .count();
        if state
            .file_aliases
            .len()
            .checked_add(new_aliases)
            .is_none_or(|total| total > self.limits.max_file_aliases)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for (name, definition) in aliases {
            let name = name.trim().to_ascii_uppercase();
            if name.is_empty()
                || name.len() > 16
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(HostProblem::Malformed);
            }
            if let Some(existing) = state.file_aliases.get(&name) {
                if existing != definition {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-file-alias".into(),
                    key: name,
                    version: 1,
                    payload: encode_file_definition(definition)?,
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for (name, definition) in aliases {
            state
                .file_aliases
                .insert(name.trim().to_ascii_uppercase(), definition.clone());
        }
        Ok(())
    }

    pub fn submit_input(
        &self,
        session: &SessionId,
        aid: u8,
        fields: &BTreeMap<String, Vec<u8>>,
    ) -> Result<(), HostProblem> {
        if fields.len() > self.limits.max_fields {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut encoded = Vec::new();
        for (name, value) in fields {
            if name.is_empty() || name.len() > 32 || value.len() > self.limits.max_screen_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            field(&mut encoded, name.as_bytes())?;
            field(&mut encoded, value)?;
        }
        let mut state = self.lock()?;
        let current = state
            .sessions
            .get(session.as_str())
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version += 1;
        next.aid = aid;
        next.input = Some(encoded);
        next.suspended = false;
        self.persist_session(session.as_str(), &next, Some(current.version))?;
        state.sessions.insert(session.as_str().into(), next);
        Ok(())
    }

    pub fn transient_records(&self, queue: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
        let state = self.lock()?;
        Ok(state
            .transient
            .get(&queue.to_ascii_uppercase())
            .map(|queue| {
                queue
                    .records
                    .iter()
                    .map(|(_, value)| value.clone())
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn invoke(
        &self,
        effect: &EffectRequest,
        request: CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        if !request.operation.supported() {
            return Err(HostProblem::Unsupported);
        }
        let replay_identity = if request.operation.is_mutating() {
            let mutation = request
                .mutation
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?;
            if effect.idempotency_key.as_ref() != Some(&mutation.idempotency_key)
                || mutation.sequence != effect.sequence
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            Some((
                mutation.idempotency_key.clone(),
                canonical_request_digest(&HostRequest::Cics(request.clone()))
                    .map_err(|_| HostProblem::ResourceExhausted)?,
            ))
        } else {
            None
        };
        let (replay_owner, replay_run_unit) = {
            let state = self.lock()?;
            let run = state
                .runs
                .get(&effect.run_unit)
                .ok_or(HostProblem::Unauthorized)?;
            (
                run.invocation.execution_id.as_str().to_string(),
                run.invocation.run_unit_id.as_str().to_string(),
            )
        };
        if let Some((key, digest)) = replay_identity.as_ref()
            && let Some(record) = self
                .store
                .get_provider_state("cics-effect-replay-v1", key.as_str())
                .map_err(store_error)?
        {
            let replay = decode_cics_effect_replay(&record.payload, self.limits)?;
            validate_cics_effect_replay_identity(
                &replay,
                key.as_str(),
                &replay_owner,
                &replay_run_unit,
                effect.sequence,
                *digest,
            )?;
            let replay = self
                .finalize_effect_replay(record, replay, effect.deadline_tick)
                .map_err(|_| HostProblem::UnknownOutcome)?;
            return Ok(replay.response);
        }
        let mut run = self
            .lock()?
            .runs
            .remove(&effect.run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        run.outer_effect_key = effect.idempotency_key.as_ref().map(ToString::to_string);
        let operation = request.operation;
        #[cfg(feature = "fault-injection")]
        let after_mutation_file = matches!(
            operation,
            CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
        )
        .then(|| argument_text(&request, "FILE").or_else(|_| argument_text(&request, "DATASET")))
        .transpose()?
        .map(|name| name.trim().to_ascii_uppercase());
        let retention_tick = effect.deadline_tick.max(run.invocation.deadline_tick);
        let result = self.invoke_run(&mut run, request, retention_tick);
        let result = match (&result, replay_identity.as_ref()) {
            (Ok(response), Some((key, digest))) => {
                let result_digest =
                    canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                let mut replay = CicsEffectReplay {
                    effect_key: Some(key.as_str().into()),
                    owner_execution: Some(replay_owner.clone()),
                    owner_run_unit: Some(replay_run_unit.clone()),
                    sequence: Some(effect.sequence),
                    deadline_tick: Some(retention_tick),
                    resolution_tick: None,
                    request_digest: *digest,
                    result_digest: Some(result_digest),
                    binding_digest: None,
                    response: response.clone(),
                };
                replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
                let payload = encode_cics_effect_replay(&replay)?;
                match self.store.put_provider_state(
                    ProviderStateRecord {
                        namespace: "cics-effect-replay-v1".into(),
                        key: key.as_str().into(),
                        version: 1,
                        payload,
                    },
                    None,
                ) {
                    Ok(()) => {
                        if self
                            .replay_unknown_after_persist
                            .swap(false, Ordering::SeqCst)
                        {
                            Err(HostProblem::UnknownOutcome)
                        } else {
                            self.finalize_effect_replay(
                                ProviderStateRecord {
                                    namespace: "cics-effect-replay-v1".into(),
                                    key: key.as_str().into(),
                                    version: 1,
                                    payload: encode_cics_effect_replay(&replay)?,
                                },
                                replay,
                                retention_tick,
                            )
                            .map(|_| result)
                            .unwrap_or(Err(HostProblem::UnknownOutcome))
                        }
                    }
                    Err(StoreError::AlreadyExists | StoreError::Conflict) => {
                        match self
                            .store
                            .get_provider_state("cics-effect-replay-v1", key.as_str())
                            .map_err(store_error)?
                        {
                            Some(record) => {
                                let replay =
                                    decode_cics_effect_replay(&record.payload, self.limits)?;
                                validate_cics_effect_replay_identity(
                                    &replay,
                                    key.as_str(),
                                    &replay_owner,
                                    &replay_run_unit,
                                    effect.sequence,
                                    *digest,
                                )?;
                                if replay.response != *response {
                                    Err(HostProblem::IdempotencyConflict)
                                } else {
                                    self.finalize_effect_replay(record, replay, retention_tick)
                                        .map(|_| result)
                                        .unwrap_or(Err(HostProblem::UnknownOutcome))
                                }
                            }
                            _ => Err(HostProblem::IdempotencyConflict),
                        }
                    }
                    Err(_) => Err(HostProblem::UnknownOutcome),
                }
            }
            _ => result,
        };
        #[cfg(feature = "fault-injection")]
        let mutation_fault = result.is_ok() && self.consume_mutation_fault(operation)?;
        #[cfg(feature = "fault-injection")]
        let file_fault = if result.is_ok() {
            match after_mutation_file {
                Some(file) => {
                    self.consume_file_fault(operation, &file, CicsFileFaultPoint::AfterMutation)?
                }
                None => false,
            }
        } else {
            false
        };
        #[cfg(feature = "fault-injection")]
        let result = if mutation_fault || file_fault {
            Err(HostProblem::UnknownOutcome)
        } else {
            result
        };
        if run.trace.len() < 4096 {
            run.trace.push(match &result {
                Ok(response) => CicsTraceEntry {
                    operation,
                    outcome: response.condition.clone(),
                    response: response.response,
                    response2: response.response2,
                    payload_bytes: response.payload.bytes().len(),
                },
                Err(problem) => CicsTraceEntry {
                    operation,
                    outcome: format!("{problem:?}"),
                    response: -1,
                    response2: 0,
                    payload_bytes: 0,
                },
            });
        }
        self.lock()?.runs.insert(effect.run_unit.clone(), run);
        result
    }

    fn finalize_effect_replay(
        &self,
        record: ProviderStateRecord,
        mut replay: CicsEffectReplay,
        resolution_lower_bound: u64,
    ) -> Result<CicsEffectReplay, HostProblem> {
        if replay.effect_key.is_none() {
            return Ok(replay);
        }
        match (record.version, replay.resolution_tick) {
            (1, None) => {}
            (2, Some(_)) => return Ok(replay),
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        let Some(clock) = &self.replay_clock else {
            return Ok(replay);
        };
        if record.version != 1 || resolution_lower_bound == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let observed_tick = clock.now_tick()?;
        if observed_tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        replay.resolution_tick = Some(
            observed_tick
                .max(resolution_lower_bound)
                .max(replay.deadline_tick.unwrap_or(0)),
        );
        replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-effect-replay-v1".into(),
                    key: record.key,
                    version: 2,
                    payload: encode_cics_effect_replay(&replay)?,
                },
                Some(1),
            )
            .map_err(store_error)?;
        Ok(replay)
    }

    pub fn terminal_run_trace(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let state = self.lock()?;
        let run = state.runs.get(&run_id).ok_or(HostProblem::NotFound)?;
        if run.session != session.as_str() || run.invocation.principal.id() != principal {
            return Err(HostProblem::Unauthorized);
        }
        Ok(run.trace.clone())
    }

    pub fn terminal_continuation_ready(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<bool, HostProblem> {
        self.public_session(session, principal, None, now_tick)?;
        Ok(self
            .lock()?
            .continuations
            .get(session.as_str())
            .is_some_and(|continuation| continuation.claimed_by.is_none()))
    }

    fn invoke_run(
        &self,
        run: &mut Run,
        request: CicsRequest,
        retention_tick: u64,
    ) -> Result<CicsResponse, HostProblem> {
        if let Some(mutation) = &request.mutation
            && mutation.transaction.as_deref() != Some(run.transaction.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{}", run.transaction),
            AccessIntent::Execute,
        )?;
        let descriptor = command_descriptor(request.operation);
        debug_assert_eq!(CICS_COMMAND_DESCRIPTORS.len(), 40);
        debug_assert_eq!(descriptor.operation, request.operation);
        debug_assert_eq!(descriptor.mutating, request.operation.is_mutating());
        debug_assert!(!descriptor.syntax.is_empty() && !descriptor.official_row.is_empty());
        match descriptor.family {
            CicsCommandFamily::TaskControl => {
                handlers::invoke_task_control(self, run, &request, retention_tick)
            }
            CicsCommandFamily::Time => handlers::invoke_time(self, run, &request),
            CicsCommandFamily::ProgramControl => {
                handlers::invoke_program_control(self, run, &request)
            }
            CicsCommandFamily::TerminalControl => {
                handlers::invoke_terminal_control(self, run, &request)
            }
            CicsCommandFamily::FileControl => handlers::invoke_file_control(self, run, &request),
            CicsCommandFamily::QueueControl => handlers::invoke_queue_control(self, run, &request),
            CicsCommandFamily::Recovery => {
                handlers::invoke_recovery(self, run, &request, retention_tick)
            }
            CicsCommandFamily::IntervalControl => {
                handlers::invoke_interval_control(self, run, &request)
            }
        }
        .or_else(|problem| handlers::condition(self, run, &request.condition_policy, problem))
    }

    pub fn reconcile_unit_of_work(
        &self,
        key: &IdempotencyKey,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<(), HostProblem> {
        self.reconcile_unit_of_work_inner(key, outcome)
    }

    fn reconcile_unit_of_work_inner(
        &self,
        key: &IdempotencyKey,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<(), HostProblem> {
        let record = self
            .store
            .get_provider_state("cics-uow", key.as_str())
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let existing = decode_uow(&record.payload)?;
        if existing.outcome != outcome
            || existing
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.effect_key != key.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if existing.finalized {
            return Ok(());
        }
        let observed_tick = match (&self.replay_clock, existing.metadata.as_ref()) {
            (Some(clock), Some(metadata)) => match clock.now_tick() {
                Ok(tick) if tick != 0 => Some(tick.max(metadata.deadline_tick)),
                _ => return Err(HostProblem::UnknownOutcome),
            },
            _ => None,
        };
        let metadata = existing.metadata.map(|mut metadata| {
            if let Some(observed_tick) = observed_tick {
                metadata.terminal_tick = Some(observed_tick);
            }
            metadata
        });
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: key.as_str().into(),
                    version: record
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?,
                    payload: encode_uow(&UowRecord {
                        finalized: true,
                        outcome,
                        transaction: existing.transaction,
                        metadata,
                    })?,
                },
                Some(record.version),
            )
            .map_err(store_error)
    }

    /// Resolve an outer CICS effect from the durable provider replay ledger
    /// without dispatching the mutation again.
    pub fn reconciled_effect_result_digest(
        &self,
        key: &IdempotencyKey,
        request_digest: [u8; 32],
    ) -> Result<[u8; 32], HostProblem> {
        let record = self
            .store
            .get_provider_state("cics-effect-replay-v1", key.as_str())
            .map_err(store_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let replay = decode_cics_effect_replay(&record.payload, self.limits)?;
        if replay.request_digest != request_digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        canonical_result_digest(&Ok(HostResult::Cics(replay.response)))
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn authorize(
        &self,
        run: &mut Run,
        class: &str,
        resource: &str,
        intent: AccessIntent,
    ) -> Result<(), HostProblem> {
        let principal = PrincipalId::new(
            run.invocation.principal.id().as_str(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let resource = ResourceName::new(resource, 246).map_err(|_| HostProblem::Malformed)?;
        match self.nested(
            run,
            HostRequest::Security(SecurityRequest::Authorize {
                principal,
                class: class.into(),
                resource,
                intent,
            }),
        )? {
            HostResult::Security(SecurityDecision::Allow) => Ok(()),
            HostResult::Security(_) => Err(HostProblem::Unauthorized),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn nested(&self, run: &mut Run, request: HostRequest) -> Result<HostResult, HostProblem> {
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = request
            .is_mutating()
            .then(|| nested_key(run, run.host_sequence))
            .transpose()?;
        let nested_invocation = key
            .as_ref()
            .filter(|_| {
                matches!(
                    &request,
                    HostRequest::Dataset(_)
                        | HostRequest::Db2(_)
                        | HostRequest::Ims(_)
                        | HostRequest::Mq(_)
                )
            })
            .map(|key| {
                invocation_with_nested_origin(
                    &run.invocation,
                    key,
                    run.outer_effect_key
                        .as_deref()
                        .ok_or(HostProblem::InfrastructureFailure)?,
                )
            })
            .transpose()?;
        let result = self.invoke_host(
            nested_invocation.as_ref().unwrap_or(&run.invocation),
            run.invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: run.invocation.run_unit_id.clone(),
                sequence: run.host_sequence,
                deadline_tick: run.invocation.deadline_tick,
                idempotency_key: key,
                request,
            },
        );
        result.outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn response(
        &self,
        run: &Run,
        disposition: CicsDisposition,
        condition: &str,
        response: i32,
        response2: i32,
        target: Option<String>,
        next_transaction: Option<String>,
        payload: Vec<u8>,
    ) -> Result<CicsResponse, HostProblem> {
        Ok(CicsResponse {
            disposition,
            condition: condition.into(),
            response,
            response2,
            applid: run.applid.clone(),
            sysid: run.sysid.clone(),
            transaction: run.transaction.clone(),
            aid: 0,
            target,
            next_transaction,
            payload: bounded(payload)?,
            outputs: BTreeMap::new(),
            unit_of_work: None,
        })
    }

    fn authorize_terminal(
        &self,
        invocation: &Invocation,
        transaction: &str,
    ) -> Result<(), HostProblem> {
        let result = self.invoke_host(
            invocation,
            0,
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Security(SecurityRequest::Authorize {
                    principal: invocation.principal.id().clone(),
                    class: "TCICSTRN".into(),
                    resource: ResourceName::new(
                        format!("CICS.{}", transaction.to_ascii_uppercase()),
                        246,
                    )
                    .map_err(|_| HostProblem::Malformed)?,
                    intent: AccessIntent::Execute,
                }),
            },
        );
        match result.outcome? {
            HostResult::Security(SecurityDecision::Allow) => Ok(()),
            HostResult::Security(_) => Err(HostProblem::Unauthorized),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn public_session(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: Option<&str>,
        now_tick: u64,
    ) -> Result<Session, HostProblem> {
        let mut state = self.lock()?;
        let current = state
            .sessions
            .get(session.as_str())
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if current.principal.is_empty() || current.principal != principal.as_str() {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(token) = csrf_token
            && (token.is_empty() || terminal_secret_digest(token) != current.csrf_sha256)
        {
            return Err(HostProblem::Unauthorized);
        }
        if !current.connected {
            return Err(HostProblem::NotFound);
        }
        if now_tick >= current.expires_at_tick {
            for run in state
                .runs
                .values()
                .filter(|run| run.session == session.as_str())
                .cloned()
                .collect::<Vec<_>>()
            {
                handlers::release_task_state(self, &run)?;
            }
            self.store
                .delete_provider_state("cics-session", session.as_str(), current.version)
                .map_err(store_error)?;
            state.sessions.remove(session.as_str());
            state.runs.retain(|_, run| run.session != session.as_str());
            return Err(HostProblem::TimedOut);
        }
        Ok(current)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, HostProblem> {
        self.state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn load_undo(
        &self,
        run_unit: &RunUnitId,
    ) -> Result<(Vec<DatasetUndo>, Option<u64>), HostProblem> {
        let Some(record) = self
            .store
            .get_provider_state("cics-uow-undo", run_unit.as_str())
            .map_err(store_error)?
        else {
            return Ok((Vec::new(), None));
        };
        Ok((
            decode_undo(&record.payload, self.limits)?,
            Some(record.version),
        ))
    }

    fn append_undo(&self, run: &mut Run, operation: DatasetUndo) -> Result<(), HostProblem> {
        let mut next = run.undo.clone();
        if next.len() >= self.limits.max_queue_records {
            return Err(HostProblem::ResourceExhausted);
        }
        next.push(operation);
        let version = run
            .undo_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow-undo".into(),
                    key: run.invocation.run_unit_id.as_str().into(),
                    version,
                    payload: encode_undo(&next)?,
                },
                run.undo_version,
            )
            .map_err(store_error)?;
        run.undo = next;
        run.undo_version = Some(version);
        Ok(())
    }

    fn clear_undo(&self, run: &mut Run) -> Result<(), HostProblem> {
        if let Some(version) = run.undo_version {
            self.store
                .delete_provider_state(
                    "cics-uow-undo",
                    run.invocation.run_unit_id.as_str(),
                    version,
                )
                .map_err(store_error)?;
        }
        run.undo.clear();
        run.undo_version = None;
        Ok(())
    }

    #[cfg(feature = "fault-injection")]
    fn consume_file_fault(
        &self,
        operation: CicsOperation,
        file: &str,
        point: CicsFileFaultPoint,
    ) -> Result<bool, HostProblem> {
        let mut state = self.lock()?;
        let hit = state
            .file_fault
            .as_ref()
            .is_some_and(|value| value == &(operation, file.to_string(), point));
        if hit {
            state.file_fault = None;
        }
        Ok(hit)
    }

    #[cfg(feature = "fault-injection")]
    fn consume_mutation_fault(&self, operation: CicsOperation) -> Result<bool, HostProblem> {
        let mut state = self.lock()?;
        let hit = state.mutation_fault == Some(operation);
        if hit {
            state.mutation_fault = None;
        }
        Ok(hit)
    }

    fn persist_session(
        &self,
        key: &str,
        session: &Session,
        expected: Option<u64>,
    ) -> Result<(), HostProblem> {
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-session".into(),
                    key: key.into(),
                    version: session.version,
                    payload: handlers::encode_session(session)?,
                },
                expected,
            )
            .map_err(store_error)
    }

    fn persist_continuation(
        &self,
        key: &str,
        continuation: &DurableContinuation,
        expected: Option<u64>,
    ) -> Result<(), HostProblem> {
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-continuation".into(),
                    key: key.into(),
                    version: continuation.version,
                    payload: encode_continuation(continuation)?,
                },
                expected,
            )
            .map_err(store_error)
    }
}

struct Provider {
    service: Arc<CicsService>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        if let Err(problem) = reject_reserved_nested_origin(invocation) {
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem),
            };
        }
        if let Err(problem) = self.service.ensure_run(invocation) {
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem),
            };
        }
        let sequence = effect.sequence;
        let request = match effect.request.clone() {
            HostRequest::Cics(request) => request,
            _ => {
                return EffectResult {
                    sequence,
                    outcome: Err(HostProblem::Malformed),
                };
            }
        };
        EffectResult {
            sequence,
            outcome: self.service.invoke(&effect, request).map(HostResult::Cics),
        }
    }
}

fn reject_reserved_nested_origin(invocation: &Invocation) -> Result<(), HostProblem> {
    if invocation
        .bindings
        .contains_key(CICS_NESTED_EFFECT_ORIGIN_BINDING)
        || invocation
            .bindings
            .contains_key(CICS_OUTER_EFFECT_ORIGIN_BINDING)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_cics_effect_replay_identity(
    replay: &CicsEffectReplay,
    key: &str,
    owner_execution: &str,
    owner_run_unit: &str,
    sequence: u64,
    request_digest: [u8; 32],
) -> Result<(), HostProblem> {
    if replay.request_digest != request_digest
        || replay
            .owner_execution
            .as_deref()
            .is_some_and(|owner| owner != owner_execution)
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    if replay.effect_key.is_none() {
        return Ok(());
    }
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(replay.response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if replay.effect_key.as_deref() != Some(key)
        || replay.owner_execution.as_deref() != Some(owner_execution)
        || replay.owner_run_unit.as_deref() != Some(owner_run_unit)
        || replay.sequence != Some(sequence)
        || replay.deadline_tick.is_none_or(|tick| tick == 0)
        || replay.result_digest != Some(result_digest)
        || replay.binding_digest != Some(cics_effect_replay_binding_digest(replay))
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

fn invocation_with_nested_origin(
    invocation: &Invocation,
    key: &IdempotencyKey,
    outer_effect_key: &str,
) -> Result<Invocation, HostProblem> {
    reject_reserved_nested_origin(invocation)?;
    let limits = InvocationLimits::default();
    if invocation.bindings.len().saturating_add(2) > limits.max_bindings {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut nested = invocation.clone();
    nested.bindings.insert(
        CICS_NESTED_EFFECT_ORIGIN_BINDING.into(),
        BoundedPayload::new(
            CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
            key.as_str().as_bytes().to_vec(),
            limits,
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    nested.bindings.insert(
        CICS_OUTER_EFFECT_ORIGIN_BINDING.into(),
        BoundedPayload::new(
            CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
            outer_effect_key.as_bytes().to_vec(),
            limits,
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(nested)
}

pub fn cics_provider(service: Arc<CicsService>, limits: InvocationLimits) -> Arc<dyn HostProvider> {
    Arc::new(Provider {
        service,
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.cics.execute", limits).expect("static capability"),
            provider_id: "mainframe-env-cics".into(),
            generation: "1".into(),
            request_schema: "mainframe-env.cics.request@1".into(),
            result_schema: "mainframe-env.cics.response@1".into(),
            max_request_bytes: 4 * 1024 * 1024,
            max_result_bytes: 4 * 1024 * 1024,
            ready: true,
        },
    })
}

fn originating_task_for(session: &Session, invocation: &Invocation) -> String {
    if session.run_unit.is_empty() {
        invocation.run_unit_id.as_str().to_string()
    } else {
        session.run_unit.clone()
    }
}

fn terminal_snapshot(session: &str, value: &Session) -> CicsTerminalSnapshot {
    CicsTerminalSnapshot {
        session: session.into(),
        principal: value.principal.clone(),
        transaction: value.transaction.clone(),
        run_unit: value.run_unit.clone(),
        rows: value.rows,
        columns: value.columns,
        aid: value.aid,
        screen: value.screen.clone(),
        mapset: value.mapset.clone(),
        map: value.map.clone(),
        suspended: value.suspended,
        connected: value.connected,
        expires_at_tick: value.expires_at_tick,
        version: value.version,
    }
}

fn terminal_secret_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn validate_terminal_identity(value: &str, max: usize) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'/' && byte != b'\\')
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn normalize_terminal_name(value: &str, max: usize) -> Result<String, HostProblem> {
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

fn normalize_bms_input(field: &BmsFieldDefinition, value: &[u8]) -> Vec<u8> {
    if !field.justify_right || value.len() >= usize::from(field.length) {
        return value.to_vec();
    }
    let mut normalized =
        vec![if field.fill_zero { b'0' } else { b' ' }; usize::from(field.length) - value.len()];
    normalized.extend_from_slice(value);
    normalized
}

fn terminal_values_equal(left: &[u8], right: &[u8]) -> bool {
    trim_terminal_value(left) == trim_terminal_value(right)
}

fn trim_terminal_value(mut value: &[u8]) -> &[u8] {
    while value.last().is_some_and(|byte| matches!(*byte, 0 | b' ')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn symbolic_map_protection(map: &BmsMapDefinition, symbolic: &[u8]) -> BTreeMap<String, bool> {
    map.fields
        .iter()
        .map(|definition| {
            let protected = definition
                .attribute_offset
                .and_then(|offset| usize::try_from(offset).ok())
                .and_then(|offset| symbolic.get(offset).copied())
                .map(|attribute| match attribute {
                    0xc0 | 0xc1 | 0xc8 | 0xcc => false,
                    0xf0 | 0xf1 | 0xf8 => true,
                    _ => definition.protected,
                })
                .unwrap_or(definition.protected);
            (definition.name.to_ascii_uppercase(), protected)
        })
        .collect()
}

fn symbolic_map_modified(map: &BmsMapDefinition, symbolic: &[u8]) -> BTreeMap<String, bool> {
    map.fields
        .iter()
        .map(|definition| {
            let modified = definition
                .attribute_offset
                .and_then(|offset| usize::try_from(offset).ok())
                .and_then(|offset| symbolic.get(offset).copied())
                .and_then(|attribute| {
                    matches!(attribute, 0xc0 | 0xc1 | 0xc8 | 0xcc | 0xf0 | 0xf1 | 0xf8)
                        .then_some(attribute & 0x01 != 0)
                })
                .unwrap_or(definition.fset);
            (definition.name.to_ascii_uppercase(), modified)
        })
        .collect()
}

fn symbolic_map_values(
    map: &BmsMapDefinition,
    symbolic: &[u8],
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    map.fields
        .iter()
        .map(|definition| {
            let start = usize::try_from(definition.output_offset.ok_or(HostProblem::Unsupported)?)
                .map_err(|_| HostProblem::ResourceExhausted)?;
            let end = start
                .checked_add(usize::from(definition.length))
                .ok_or(HostProblem::ResourceExhausted)?;
            let value = symbolic
                .get(start..end)
                .ok_or(HostProblem::Malformed)?
                .to_vec();
            Ok((definition.name.to_ascii_uppercase(), value))
        })
        .collect()
}

fn encode_symbolic_map_output(
    map: &BmsMapDefinition,
    symbolic: &[u8],
) -> Result<Vec<u8>, HostProblem> {
    let mut encoded = Vec::new();
    let values = symbolic_map_values(map, symbolic)?;
    for definition in &map.fields {
        let value = values
            .get(&definition.name.to_ascii_uppercase())
            .ok_or(HostProblem::Malformed)?;
        field(&mut encoded, definition.name.as_bytes())?;
        field(&mut encoded, if definition.secret { &[] } else { value })?;
    }
    Ok(encoded)
}

fn encode_tn3270_screen(
    session: &Session,
    map: &BmsMapDefinition,
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    let displayed = decode_map_payload(&session.screen, limits).unwrap_or_default();
    let mut out = vec![0xf5, 0xc3];
    let mut fields = map.fields.iter().collect::<Vec<_>>();
    fields.sort_by_key(|field| (field.row, field.column, field.name.as_str()));
    for definition in fields {
        let address = terminal_field_address(session, map, definition)?;
        out.push(0x11);
        out.extend_from_slice(&encode_terminal_address(address)?);
        out.push(0x1d);
        let protected = session
            .field_protection
            .get(&definition.name.to_ascii_uppercase())
            .copied()
            .unwrap_or(definition.protected);
        out.push(if protected { 0x20 } else { 0x00 });
        let value = if definition.secret {
            Vec::new()
        } else {
            displayed
                .get(&definition.name.to_ascii_uppercase())
                .cloned()
                .unwrap_or_else(|| definition.initial.clone())
        };
        out.extend(value.into_iter().take(usize::from(definition.length)));
        if out.len() > limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(out)
}

fn decode_tn3270_input(
    record: &[u8],
    session: &Session,
    map: &BmsMapDefinition,
    limits: CicsLimits,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    if record.len() < 3 || !handlers::valid_terminal_aid(record[0]) || record.contains(&0xff) {
        return Err(HostProblem::Malformed);
    }
    decode_terminal_address(record[1], record[2])?;
    let mut fields = BTreeMap::new();
    let mut at = 3usize;
    while at < record.len() {
        if record.get(at) != Some(&0x11) || at.checked_add(3).is_none_or(|end| end > record.len()) {
            return Err(HostProblem::Malformed);
        }
        let address = decode_terminal_address(record[at + 1], record[at + 2])?;
        at += 3;
        let end = record[at..]
            .iter()
            .position(|byte| *byte == 0x11)
            .map_or(record.len(), |offset| at + offset);
        let definition = map
            .fields
            .iter()
            .find(|field| terminal_field_address(session, map, field) == Ok(address))
            .ok_or(HostProblem::Malformed)?;
        let value = record[at..end].to_vec();
        if definition.protected
            || value.len() > usize::from(definition.length)
            || fields
                .insert(definition.name.to_ascii_uppercase(), value)
                .is_some()
            || fields.len() > limits.max_fields
        {
            return Err(HostProblem::Unauthorized);
        }
        at = end;
    }
    Ok(fields)
}

fn decode_map_payload(
    payload: &[u8],
    limits: CicsLimits,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    let mut reader = Reader {
        bytes: payload,
        at: 0,
    };
    let mut fields = BTreeMap::new();
    while reader.at < payload.len() {
        let name = String::from_utf8(reader.field(32)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .to_ascii_uppercase();
        let value = reader.field(limits.max_screen_bytes)?;
        if fields.insert(name, value).is_some() || fields.len() > limits.max_fields {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(fields)
}

fn argument_bytes(request: &CicsRequest, name: &str) -> Option<Vec<u8>> {
    request
        .arguments
        .get(name)
        .map(|value| value.bytes().to_vec())
}

fn argument_optional(request: &CicsRequest, name: &str) -> Option<String> {
    argument_bytes(request, name).map(|value| String::from_utf8_lossy(&value).into_owned())
}

fn argument_text(request: &CicsRequest, name: &str) -> Result<String, HostProblem> {
    let value = argument_bytes(request, name).ok_or(HostProblem::Malformed)?;
    String::from_utf8(value).map_err(|_| HostProblem::Malformed)
}

fn bounded(bytes: Vec<u8>) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(
        "mainframe-env.cics.payload@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn access_for(operation: CicsOperation) -> AccessIntent {
    match operation {
        CicsOperation::Read
        | CicsOperation::ReadNext
        | CicsOperation::ReadPrev
        | CicsOperation::StartBrowse
        | CicsOperation::EndBrowse => AccessIntent::Read,
        _ => AccessIntent::Update,
    }
}

fn nested_key(run: &Run, sequence: u64) -> Result<IdempotencyKey, HostProblem> {
    IdempotencyKey::new(
        format!("cics:{}:{sequence}", run.invocation.run_unit_id),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn nested_mutation(run: &Run, sequence: u64) -> Result<Mutation, HostProblem> {
    Ok(Mutation {
        sequence,
        idempotency_key: nested_key(run, sequence)?,
        transaction: Some(run.transaction.clone()),
    })
}

fn encode_undo(operations: &[DatasetUndo]) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MECUNDO1".to_vec();
    payload.extend_from_slice(
        &u32::try_from(operations.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for operation in operations {
        match operation {
            DatasetUndo::Restore {
                dataset,
                key,
                record,
            } => {
                payload.push(1);
                field(&mut payload, dataset.as_str().as_bytes())?;
                field(&mut payload, key)?;
                field(&mut payload, record)?;
            }
            DatasetUndo::Delete { dataset, key } => {
                payload.push(2);
                field(&mut payload, dataset.as_str().as_bytes())?;
                field(&mut payload, key)?;
            }
        }
    }
    Ok(payload)
}

fn decode_undo(payload: &[u8], limits: CicsLimits) -> Result<Vec<DatasetUndo>, HostProblem> {
    let mut reader = Reader {
        bytes: payload,
        at: 0,
    };
    if reader.take(8)? != b"MECUNDO1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut operations = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = reader.take(1)?[0];
        let dataset = DatasetName::new(
            String::from_utf8(reader.field(128)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            128,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let key = reader.field(limits.max_queue_bytes)?;
        let operation = match kind {
            1 => DatasetUndo::Restore {
                dataset,
                key,
                record: reader.field(limits.max_queue_bytes)?,
            },
            2 => DatasetUndo::Delete { dataset, key },
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        operations.push(operation);
    }
    if reader.at != payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(operations)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UowRecord {
    finalized: bool,
    outcome: CicsUnitOfWorkOutcome,
    transaction: String,
    metadata: Option<UowRetentionMetadata>,
}

fn encode_uow(record: &UowRecord) -> Result<Vec<u8>, HostProblem> {
    crate::retention::encode_uow(&DecodedUow {
        state: crate::retention::CicsUowState::from_parts(record.finalized, record.outcome),
        transaction: record.transaction.clone(),
        metadata: record.metadata.clone(),
    })
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_uow(payload: &[u8]) -> Result<UowRecord, HostProblem> {
    let decoded =
        crate::retention::decode_uow(payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    let (finalized, outcome) = decoded.state.parts();
    Ok(UowRecord {
        finalized,
        outcome,
        transaction: decoded.transaction,
        metadata: decoded.metadata,
    })
}

pub(in crate::service) fn decimal_payload(value: i64) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(
        "mainframe-env.cics.decimal@1",
        value.to_string().into_bytes(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn encode_continuation(continuation: &DurableContinuation) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MECC1".to_vec();
    field(&mut out, continuation.transaction.as_bytes())?;
    field(&mut out, &continuation.commarea)?;
    field(
        &mut out,
        continuation.claimed_by.as_deref().unwrap_or("").as_bytes(),
    )?;
    field(&mut out, continuation.effect_key.as_bytes())?;
    Ok(out)
}

fn decode_continuation(
    bytes: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<DurableContinuation, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(5)? != b"MECC1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let transaction =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let commarea = reader.field(limits.max_screen_bytes)?;
    let claimed =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let effect_key =
        String::from_utf8(reader.field(256)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    if reader.at != bytes.len() || transaction.is_empty() || effect_key.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(DurableContinuation {
        transaction,
        commarea,
        claimed_by: (!claimed.is_empty()).then_some(claimed),
        effect_key,
        version,
    })
}

fn encode_transient(queue: &TransientQueue) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MECT2".to_vec();
    out.extend_from_slice(
        &u32::try_from(queue.records.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (key, value) in &queue.records {
        field(&mut out, key.as_bytes())?;
        field(&mut out, value)?;
    }
    Ok(out)
}

fn decode_transient(
    bytes: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<TransientQueue, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(5)? != b"MECT2" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if count > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut records = Vec::with_capacity(count);
    let mut total = 0usize;
    for _ in 0..count {
        let key = String::from_utf8(reader.field(256)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = reader.field(limits.max_queue_bytes)?;
        total = total
            .checked_add(value.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if key.is_empty() || total > limits.max_queue_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
        records.push((key, value));
    }
    if reader.at != bytes.len() || records.is_empty() || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(TransientQueue { records, version })
}

fn mutation_problem(problem: HostProblem) -> HostProblem {
    match problem {
        HostProblem::InfrastructureFailure | HostProblem::ProviderFailure => {
            HostProblem::UnknownOutcome
        }
        other => other,
    }
}

fn map_key(mapset: &str, map: &str) -> String {
    format!("{mapset}/{map}")
}

fn encode_file_definition(definition: &CicsFileDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MEFA1".to_vec();
    field(&mut out, definition.dataset.as_str().as_bytes())?;
    out.extend_from_slice(&definition.ccsid.unwrap_or(0).to_be_bytes());
    Ok(out)
}

fn decode_file_definition(bytes: &[u8]) -> Result<CicsFileDefinition, HostProblem> {
    if !bytes.starts_with(b"MEFA1") {
        let target =
            String::from_utf8(bytes.to_vec()).map_err(|_| HostProblem::InfrastructureFailure)?;
        return Ok(CicsFileDefinition {
            dataset: DatasetName::new(target, 128)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ccsid: None,
        });
    }
    let mut reader = Reader { bytes, at: 5 };
    let target =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let ccsid = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsFileDefinition {
        dataset: DatasetName::new(target, 128).map_err(|_| HostProblem::InfrastructureFailure)?,
        ccsid: (ccsid != 0).then_some(ccsid),
    })
}

fn encode_file_status(status: CicsFileStatus) -> Vec<u8> {
    match status {
        CicsFileStatus::Open => b"OPEN".to_vec(),
        CicsFileStatus::ClosedEnabled => b"CLOSED-ENABLED".to_vec(),
        CicsFileStatus::Closed => b"CLOSED".to_vec(),
        CicsFileStatus::Disabled => b"DISABLED".to_vec(),
    }
}

fn decode_file_status(bytes: &[u8]) -> Result<CicsFileStatus, HostProblem> {
    match bytes {
        b"OPEN" => Ok(CicsFileStatus::Open),
        b"CLOSED-ENABLED" => Ok(CicsFileStatus::ClosedEnabled),
        b"CLOSED" | b"CLOSED-UNENABLED" => Ok(CicsFileStatus::Closed),
        b"DISABLED" => Ok(CicsFileStatus::Disabled),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn encode_dataset_bytes(ccsid: Option<u16>, bytes: &[u8]) -> Result<Vec<u8>, HostProblem> {
    match ccsid {
        None | Some(1208) => Ok(bytes.to_vec()),
        Some(37) => match std::str::from_utf8(bytes) {
            Ok(text) => CodePage::Cp037
                .encode(text, bytes.len().saturating_mul(4).max(1))
                .map_err(|_| HostProblem::Malformed),
            Err(_) => Ok(bytes.to_vec()),
        },
        Some(_) => Err(HostProblem::Unsupported),
    }
}

fn decode_dataset_bytes(ccsid: Option<u16>, bytes: &[u8]) -> Result<Vec<u8>, HostProblem> {
    match ccsid {
        None | Some(1208) => Ok(bytes.to_vec()),
        Some(37) => CodePage::Cp037
            .decode(bytes, bytes.len().saturating_mul(4).max(1))
            .map(String::into_bytes)
            .map_err(|_| HostProblem::Malformed),
        Some(_) => Err(HostProblem::Unsupported),
    }
}

fn encode_map(map: &BmsMapDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MECM6".to_vec();
    field(&mut out, map.mapset.as_bytes())?;
    field(&mut out, map.map.as_bytes())?;
    out.extend_from_slice(&map.line.to_be_bytes());
    out.extend_from_slice(&map.column.to_be_bytes());
    out.extend_from_slice(&map.rows.to_be_bytes());
    out.extend_from_slice(&map.columns.to_be_bytes());
    out.extend_from_slice(
        &u32::try_from(map.fields.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for definition in &map.fields {
        field(&mut out, definition.name.as_bytes())?;
        out.extend_from_slice(&definition.row.to_be_bytes());
        out.extend_from_slice(&definition.column.to_be_bytes());
        out.extend_from_slice(&definition.length.to_be_bytes());
        field(&mut out, &definition.initial)?;
        field(
            &mut out,
            definition.color.as_deref().unwrap_or("").as_bytes(),
        )?;
        field(
            &mut out,
            definition.highlight.as_deref().unwrap_or("").as_bytes(),
        )?;
        out.push(u8::from(definition.protected));
        out.push(u8::from(definition.secret));
        out.push(u8::from(definition.fset));
        out.push(u8::from(definition.justify_right));
        out.push(u8::from(definition.fill_zero));
        match definition.output_offset {
            Some(offset) => {
                out.push(1);
                out.extend_from_slice(&offset.to_be_bytes());
            }
            None => out.push(0),
        }
        match definition.attribute_offset {
            Some(offset) => {
                out.push(1);
                out.extend_from_slice(&offset.to_be_bytes());
            }
            None => out.push(0),
        }
    }
    Ok(out)
}

fn decode_map(bytes: &[u8], limits: CicsLimits) -> Result<BmsMapDefinition, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    let version = match reader.take(5)? {
        b"MECM1" => 1,
        b"MECM2" => 2,
        b"MECM3" => 3,
        b"MECM4" => 4,
        b"MECM5" => 5,
        b"MECM6" => 6,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let mapset =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let map =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let (line, column) = if version >= 6 {
        let line = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let column = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        (line, column)
    } else {
        (1, 1)
    };
    let rows = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let columns = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if count > limits.max_fields {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let name =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let row = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let column = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let length = u16::from_be_bytes(
            reader
                .take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let initial = reader.field(limits.max_screen_bytes)?;
        let color =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let highlight =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let protected = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let secret = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let fset = if version >= 5 {
            match reader.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        } else {
            false
        };
        let (justify_right, fill_zero) = if version >= 2 {
            let justify_right = match reader.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            let fill_zero = match reader.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            (justify_right, fill_zero)
        } else {
            (false, false)
        };
        let output_offset = if version >= 3 {
            match reader.take(1)?[0] {
                0 => None,
                1 => Some(u32::from_be_bytes(
                    reader
                        .take(4)?
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )),
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        } else {
            None
        };
        let attribute_offset = if version >= 4 {
            match reader.take(1)?[0] {
                0 => None,
                1 => Some(u32::from_be_bytes(
                    reader
                        .take(4)?
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )),
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        } else {
            None
        };
        fields.push(BmsFieldDefinition {
            name,
            row,
            column,
            length,
            initial,
            color: (!color.is_empty()).then_some(color),
            highlight: (!highlight.is_empty()).then_some(highlight),
            protected,
            secret,
            fset,
            justify_right,
            fill_zero,
            output_offset,
            attribute_offset,
        });
    }
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let definition = BmsMapDefinition {
        mapset,
        map,
        line,
        column,
        rows,
        columns,
        fields,
    };
    validate_map(&definition, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(definition)
}

fn decode_session(bytes: &[u8], version: u64, limits: CicsLimits) -> Result<Session, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    let schema = handlers::session_schema_version(reader.take(5)?)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let rows = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let columns = u16::from_be_bytes(
        reader
            .take(2)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let (
        principal,
        transaction,
        run_unit,
        csrf_sha256,
        idle_timeout_ticks,
        expires_at_tick,
        connected,
    ) = if schema >= 2 {
        let principal = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let transaction =
            String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let run_unit = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let csrf_sha256 =
            String::from_utf8(reader.field(64)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let idle_timeout_ticks = u64::from_be_bytes(
            reader
                .take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let expires_at_tick = u64::from_be_bytes(
            reader
                .take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let connected = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let public = !principal.is_empty()
            && !transaction.is_empty()
            && !run_unit.is_empty()
            && csrf_sha256.len() == 64
            && idle_timeout_ticks != 0;
        let internal = principal.is_empty()
            && transaction.is_empty()
            && run_unit.is_empty()
            && csrf_sha256.is_empty()
            && idle_timeout_ticks == u64::MAX
            && expires_at_tick == u64::MAX;
        if !public && !internal {
            return Err(HostProblem::InfrastructureFailure);
        }
        (
            principal,
            transaction,
            run_unit,
            csrf_sha256,
            idle_timeout_ticks,
            expires_at_tick,
            connected,
        )
    } else {
        (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            u64::MAX,
            u64::MAX,
            true,
        )
    };
    let aid = reader.take(1)?[0];
    let suspended = match reader.take(1)?[0] {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let (mapset, map) = if schema >= 2 {
        let mapset =
            String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let map =
            String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        (
            (!mapset.is_empty()).then_some(mapset),
            (!map.is_empty()).then_some(map),
        )
    } else {
        (None, None)
    };
    let field_protection = if schema >= 3 {
        let count = usize::try_from(u32::from_be_bytes(
            reader
                .take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if count > limits.max_fields {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let name = String::from_utf8(reader.field(32)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let protected = match reader.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            if values.insert(name, protected).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        values
    } else {
        BTreeMap::new()
    };
    let field_modified = if schema >= 4 {
        handlers::decode_session_flags(&mut reader, limits)?
    } else {
        BTreeMap::new()
    };
    let field_values = if schema >= 4 {
        let count = usize::try_from(u32::from_be_bytes(
            reader
                .take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if count > limits.max_fields {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let name = String::from_utf8(reader.field(32)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let value = reader.field(limits.max_screen_bytes)?;
            if values.insert(name, value).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        values
    } else {
        BTreeMap::new()
    };
    let screen = reader.field(limits.max_screen_bytes)?;
    let input = match reader.take(1)?[0] {
        0 => None,
        1 => Some(reader.field(limits.max_screen_bytes)?),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let (user_corr_data, user_corr_effect_key, user_corr_request_digest) = if schema >= 5 {
        let data = reader.field(64)?;
        let key = String::from_utf8(reader.field(256)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let digest = match reader.take(1)?[0] {
            0 => None,
            1 => Some(
                reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        if key.is_empty() && (digest.is_some() || !data.is_empty())
            || !key.is_empty() && digest.is_none()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let key = if key.is_empty() {
            None
        } else {
            IdempotencyKey::new(&key, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            Some(key)
        };
        (data, key, digest)
    } else {
        (Vec::new(), None, None)
    };
    let handle_state = handlers::decode_session_handle_state(&mut reader, schema)?;
    if reader.at != bytes.len() || rows == 0 || columns == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Session {
        rows,
        columns,
        principal,
        transaction,
        run_unit,
        user_corr_data,
        user_corr_effect_key,
        user_corr_request_digest,
        csrf_sha256,
        idle_timeout_ticks,
        expires_at_tick,
        connected,
        aid,
        screen,
        input,
        suspended,
        mapset,
        map,
        field_protection,
        field_modified,
        field_values,
        handle_state,
        version,
    })
}

fn encode_cics_effect_replay(replay: &CicsEffectReplay) -> Result<Vec<u8>, HostProblem> {
    let response = &replay.response;
    let owner = replay
        .owner_execution
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let deadline_tick = replay
        .deadline_tick
        .filter(|tick| *tick != 0)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut out = if let Some(effect_key) = replay.effect_key.as_deref() {
        let owner_run_unit = replay
            .owner_run_unit
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let sequence = replay
            .sequence
            .filter(|sequence| *sequence != 0)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let result_digest = replay
            .result_digest
            .ok_or(HostProblem::InfrastructureFailure)?;
        let binding_digest = replay
            .binding_digest
            .ok_or(HostProblem::InfrastructureFailure)?;
        if replay.resolution_tick == Some(0)
            || replay
                .resolution_tick
                .is_some_and(|tick| tick < deadline_tick)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut out = b"MECER003".to_vec();
        field(&mut out, effect_key.as_bytes())?;
        field(&mut out, owner.as_bytes())?;
        field(&mut out, owner_run_unit.as_bytes())?;
        out.extend_from_slice(&sequence.to_be_bytes());
        out.extend_from_slice(&deadline_tick.to_be_bytes());
        out.extend_from_slice(&replay.resolution_tick.unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&replay.request_digest);
        out.extend_from_slice(&result_digest);
        out.extend_from_slice(&binding_digest);
        out
    } else {
        let mut out = b"MECER002".to_vec();
        field(&mut out, owner.as_bytes())?;
        out.extend_from_slice(&deadline_tick.to_be_bytes());
        out.extend_from_slice(&replay.request_digest);
        out
    };
    out.push(match response.disposition {
        CicsDisposition::Complete => 1,
        CicsDisposition::Suspended => 2,
        CicsDisposition::Transfer => 3,
        CicsDisposition::Handler => 4,
        CicsDisposition::Returned => 5,
        CicsDisposition::Abended => 6,
        CicsDisposition::Ignored => 7,
    });
    field(&mut out, response.condition.as_bytes())?;
    out.extend_from_slice(&response.response.to_be_bytes());
    out.extend_from_slice(&response.response2.to_be_bytes());
    field(&mut out, response.applid.as_bytes())?;
    field(&mut out, response.sysid.as_bytes())?;
    field(&mut out, response.transaction.as_bytes())?;
    out.push(response.aid);
    for value in [&response.target, &response.next_transaction] {
        match value {
            Some(value) => {
                out.push(1);
                field(&mut out, value.as_bytes())?;
            }
            None => out.push(0),
        }
    }
    field(&mut out, response.payload.schema().as_bytes())?;
    field(&mut out, response.payload.bytes())?;
    out.extend_from_slice(
        &u32::try_from(response.outputs.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &response.outputs {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value.schema().as_bytes())?;
        field(&mut out, value.bytes())?;
    }
    out.push(match response.unit_of_work {
        None => 0,
        Some(CicsUnitOfWorkOutcome::Committed) => 1,
        Some(CicsUnitOfWorkOutcome::RolledBack) => 2,
    });
    Ok(out)
}

pub(crate) fn decode_cics_effect_replay(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsEffectReplay, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    let version = match reader.take(8)? {
        b"MECER001" => 1,
        b"MECER002" => 2,
        b"MECER003" => 3,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let (effect_key, owner_execution, owner_run_unit, sequence, deadline_tick, resolution_tick) =
        if version == 3 {
            let effect_key =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let owner_execution =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let owner_run_unit =
                String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            let sequence = u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
            let deadline_tick = u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
            let resolution_tick = match u64::from_be_bytes(
                reader
                    .take(8)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ) {
                0 => None,
                tick => Some(tick),
            };
            (
                Some(effect_key),
                Some(owner_execution),
                Some(owner_run_unit),
                Some(sequence),
                Some(deadline_tick),
                resolution_tick,
            )
        } else if version == 2 {
            (
                None,
                Some(
                    String::from_utf8(
                        reader.field(InvocationLimits::default().max_identity_bytes)?,
                    )
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                ),
                None,
                None,
                Some(u64::from_be_bytes(
                    reader
                        .take(8)?
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )),
                None,
            )
        } else {
            (None, None, None, None, None, None)
        };
    if owner_execution
        .as_ref()
        .is_some_and(|owner| ExecutionId::new(owner, InvocationLimits::default()).is_err())
        || owner_run_unit
            .as_ref()
            .is_some_and(|owner| RunUnitId::new(owner, InvocationLimits::default()).is_err())
        || effect_key
            .as_ref()
            .is_some_and(|key| IdempotencyKey::new(key, InvocationLimits::default()).is_err())
        || sequence == Some(0)
        || deadline_tick == Some(0)
        || resolution_tick == Some(0)
        || resolution_tick
            .zip(deadline_tick)
            .is_some_and(|(resolution, deadline)| resolution < deadline)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let request_digest = reader
        .take(32)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let (result_digest, binding_digest) = if version == 3 {
        (
            Some(
                reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            Some(
                reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
        )
    } else {
        (None, None)
    };
    let disposition = match reader.take(1)?[0] {
        1 => CicsDisposition::Complete,
        2 => CicsDisposition::Suspended,
        3 => CicsDisposition::Transfer,
        4 => CicsDisposition::Handler,
        5 => CicsDisposition::Returned,
        6 => CicsDisposition::Abended,
        7 => CicsDisposition::Ignored,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let condition =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let response = i32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let response2 = i32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    let applid =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let sysid =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let transaction =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let aid = reader.take(1)?[0];
    let mut optional = || -> Result<Option<String>, HostProblem> {
        match reader.take(1)?[0] {
            0 => Ok(None),
            1 => String::from_utf8(reader.field(128)?)
                .map(Some)
                .map_err(|_| HostProblem::InfrastructureFailure),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    };
    let target = optional()?;
    let next_transaction = optional()?;
    let payload_schema =
        String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let payload = BoundedPayload::new(
        payload_schema,
        reader.field(limits.max_screen_bytes)?,
        InvocationLimits {
            max_payload_bytes: limits.max_screen_bytes,
            ..InvocationLimits::default()
        },
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let output_count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if output_count > limits.max_fields {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut outputs = BTreeMap::new();
    for _ in 0..output_count {
        let name = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let schema = String::from_utf8(reader.field(128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = BoundedPayload::new(
            schema,
            reader.field(limits.max_screen_bytes)?,
            InvocationLimits {
                max_payload_bytes: limits.max_screen_bytes,
                ..InvocationLimits::default()
            },
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if outputs.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let unit_of_work = match reader.take(1)?[0] {
        0 => None,
        1 => Some(CicsUnitOfWorkOutcome::Committed),
        2 => Some(CicsUnitOfWorkOutcome::RolledBack),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CicsEffectReplay {
        effect_key,
        owner_execution,
        owner_run_unit,
        sequence,
        deadline_tick,
        resolution_tick,
        request_digest,
        result_digest,
        binding_digest,
        response: CicsResponse {
            disposition,
            condition,
            response,
            response2,
            applid,
            sysid,
            transaction,
            aid,
            target,
            next_transaction,
            payload,
            outputs,
            unit_of_work,
        },
    })
}

pub(crate) fn cics_effect_replay_binding_digest(replay: &CicsEffectReplay) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cics-effect-replay-binding@1\0");
    for field in [
        replay.effect_key.as_deref().unwrap_or_default().as_bytes(),
        replay
            .owner_execution
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        replay
            .owner_run_unit
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update(replay.sequence.unwrap_or(0).to_be_bytes());
    hash.update(replay.deadline_tick.unwrap_or(0).to_be_bytes());
    hash.update(replay.resolution_tick.unwrap_or(0).to_be_bytes());
    hash.update(replay.request_digest);
    hash.update(replay.result_digest.unwrap_or([0; 32]));
    hash.finalize().into()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, amount: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn field(&mut self, max: usize) -> Result<Vec<u8>, HostProblem> {
        let amount = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if amount > max {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(amount)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, ExecutionId, Principal, RequestId, ResourceLimits, Selector, ServiceClass,
        TraceId,
    };
    use mainframe_env_host_api::{
        DatasetAttributes, DatasetOrganization, HostLimits, RecordFormat, RegistrySnapshot,
        SecretRef,
    };
    use mainframe_env_racf::{MemorySecretResolver, RacfService, racf_providers};
    use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};
    use mainframe_env_store_api::{
        EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState, WorkState,
    };
    use mainframe_env_store_api::{ProviderStateMutation, ProviderStateWrite};
    use std::collections::BTreeSet;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    struct TestCicsClock {
        tick: AtomicU64,
        fail_next: AtomicBool,
    }

    struct FailCicsReplayCasStore {
        inner: MemoryStore,
        fail_insert: AtomicBool,
        fail_next: AtomicBool,
        fail_session: AtomicBool,
    }

    impl FailCicsReplayCasStore {
        fn new() -> Self {
            Self {
                inner: MemoryStore::new(Default::default()),
                fail_insert: AtomicBool::new(false),
                fail_next: AtomicBool::new(false),
                fail_session: AtomicBool::new(false),
            }
        }
    }

    impl mainframe_env_store_api::AuditSink for FailCicsReplayCasStore {
        fn record_audit(
            &self,
            record: mainframe_env_execution_api::AuditRecord,
        ) -> Result<(), StoreError> {
            self.inner.record_audit(record)
        }

        fn audit_records(
            &self,
            execution_id: &ExecutionId,
            start_effect_sequence: u64,
            max: usize,
        ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
            self.inner
                .audit_records(execution_id, start_effect_sequence, max)
        }
    }

    impl ProviderStateStore for FailCicsReplayCasStore {
        fn get_provider_state(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<ProviderStateRecord>, StoreError> {
            self.inner.get_provider_state(namespace, key)
        }

        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, StoreError> {
            self.inner.list_provider_state(namespace, max)
        }

        fn put_provider_state(
            &self,
            record: ProviderStateRecord,
            expected_version: Option<u64>,
        ) -> Result<(), StoreError> {
            if record.namespace == "cics-session"
                && expected_version.is_some()
                && self.fail_session.swap(false, Ordering::SeqCst)
            {
                Err(StoreError::Infrastructure(
                    "injected-cics-session-cas-failure".into(),
                ))
            } else if record.namespace == "cics-effect-replay-v1"
                && expected_version.is_none()
                && self.fail_insert.swap(false, Ordering::SeqCst)
            {
                Err(StoreError::Infrastructure(
                    "injected-cics-replay-insert-failure".into(),
                ))
            } else if record.namespace == "cics-effect-replay-v1"
                && expected_version == Some(1)
                && self.fail_next.swap(false, Ordering::SeqCst)
            {
                Err(StoreError::Infrastructure(
                    "injected-cics-replay-cas-failure".into(),
                ))
            } else {
                self.inner.put_provider_state(record, expected_version)
            }
        }

        fn delete_provider_state(
            &self,
            namespace: &str,
            key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .delete_provider_state(namespace, key, expected_version)
        }

        fn move_provider_state(
            &self,
            record: ProviderStateRecord,
            old_key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .move_provider_state(record, old_key, expected_version)
        }

        fn put_provider_states_atomic(
            &self,
            writes: Vec<ProviderStateWrite>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_states_atomic(writes)
        }

        fn mutate_provider_states_atomic(
            &self,
            mutations: Vec<ProviderStateMutation>,
        ) -> Result<(), StoreError> {
            self.inner.mutate_provider_states_atomic(mutations)
        }
    }

    impl TestCicsClock {
        fn fixed(tick: u64) -> Self {
            Self {
                tick: AtomicU64::new(tick),
                fail_next: AtomicBool::new(false),
            }
        }
    }

    impl CicsReplayClock for TestCicsClock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            if self.fail_next.swap(false, Ordering::SeqCst) {
                Err(HostProblem::InfrastructureFailure)
            } else {
                Ok(self.tick.load(Ordering::SeqCst))
            }
        }
    }

    struct Authority {
        descriptor: CapabilityDescriptor,
    }

    type CommandSecurityTrace = Arc<Mutex<Vec<(String, String, AccessIntent)>>>;

    struct CommandSecurityAuthority {
        descriptor: CapabilityDescriptor,
        deny_command: bool,
        seen: CommandSecurityTrace,
    }

    #[derive(Default)]
    struct DatasetTrace {
        requests: Mutex<Vec<DatasetRequest>>,
        origins: Mutex<Vec<(String, String)>>,
    }

    struct TracedDataset {
        descriptor: CapabilityDescriptor,
        trace: Arc<DatasetTrace>,
    }

    struct PersistedDataset {
        descriptor: CapabilityDescriptor,
        record: Arc<Mutex<Option<Vec<u8>>>>,
    }

    struct SyncpointOriginProvider {
        descriptor: CapabilityDescriptor,
        seen: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    #[allow(dead_code)]
    struct CountedMutationProvider {
        descriptor: CapabilityDescriptor,
        commits: Arc<AtomicUsize>,
    }

    impl HostProvider for Authority {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
                HostRequest::Dataset(DatasetRequest::Read { .. }) => {
                    Ok(HostResult::Dataset(DatasetResult::Records {
                        records: vec![b"CARD0001".to_vec()],
                        identities: vec![b"CARD0001".to_vec()],
                        version: 1,
                    }))
                }
                HostRequest::Dataset(DatasetRequest::Attributes { .. }) => {
                    Ok(HostResult::Dataset(DatasetResult::Attributes {
                        attributes: DatasetAttributes {
                            organization: DatasetOrganization::Sequential,
                            record_format: RecordFormat::Variable,
                            logical_record_length: 80,
                            key_offset: None,
                            key_length: None,
                            ccsid: Some(37),
                        },
                        version: 1,
                    }))
                }
                HostRequest::Dataset(_) => {
                    Ok(HostResult::Dataset(DatasetResult::Mutated { version: 2 }))
                }
                HostRequest::Program(ProgramRequest::Inquire { program })
                    if program.as_str() == "MISSING" =>
                {
                    Err(HostProblem::NotFound)
                }
                HostRequest::Program(ProgramRequest::Inquire { .. }) => {
                    Ok(HostResult::Program(bounded(Vec::new()).unwrap()))
                }
                HostRequest::Program(_) => {
                    Ok(HostResult::Program(bounded(b"CHILD".to_vec()).unwrap()))
                }
                HostRequest::Clock(ClockRequest::UtcTimestamp) => {
                    Ok(HostResult::Clock("20260830123456789".into()))
                }
                _ => Err(HostProblem::Unsupported),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for CommandSecurityAuthority {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Security(SecurityRequest::Authorize {
                    class,
                    resource,
                    intent,
                    ..
                }) => {
                    self.seen.lock().unwrap().push((
                        class.clone(),
                        resource.as_str().to_string(),
                        intent,
                    ));
                    Ok(HostResult::Security(
                        if self.deny_command && class == "FACILITY" {
                            SecurityDecision::Deny
                        } else {
                            SecurityDecision::Allow
                        },
                    ))
                }
                _ => Err(HostProblem::Unsupported),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for TracedDataset {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
            if let Some(key) = &effect.idempotency_key {
                let nested = invocation
                    .bindings
                    .get(CICS_NESTED_EFFECT_ORIGIN_BINDING)
                    .map(|value| String::from_utf8(value.bytes().to_vec()).unwrap());
                let outer = invocation
                    .bindings
                    .get(CICS_OUTER_EFFECT_ORIGIN_BINDING)
                    .map(|value| String::from_utf8(value.bytes().to_vec()).unwrap());
                assert_eq!(nested.as_deref(), Some(key.as_str()));
                self.trace.origins.lock().unwrap().push((
                    nested.unwrap(),
                    outer.expect("mutating CICS nested request carries outer effect"),
                ));
            }
            let outcome = match effect.request {
                HostRequest::Dataset(request) => {
                    self.trace.requests.lock().unwrap().push(request.clone());
                    match request {
                        DatasetRequest::Read { key, .. } => {
                            let identity = key.unwrap_or_else(|| b"AA".to_vec());
                            Ok(HostResult::Dataset(DatasetResult::Records {
                                records: vec![b"AA11".to_vec()],
                                identities: vec![identity],
                                version: 1,
                            }))
                        }
                        DatasetRequest::StartBrowse { .. } => {
                            Ok(HostResult::Dataset(DatasetResult::Browse {
                                cursor: "CURSOR-1".into(),
                                record: None,
                                identity: None,
                                key: None,
                            }))
                        }
                        DatasetRequest::ReadNext { .. } => {
                            Ok(HostResult::Dataset(DatasetResult::Browse {
                                cursor: "CURSOR-1".into(),
                                record: Some(b"AA11".to_vec()),
                                identity: Some(b"AA".to_vec()),
                                key: Some(b"AA".to_vec()),
                            }))
                        }
                        DatasetRequest::EndBrowse { .. } => {
                            Ok(HostResult::Dataset(DatasetResult::Browse {
                                cursor: "CURSOR-1".into(),
                                record: None,
                                identity: None,
                                key: None,
                            }))
                        }
                        _ => Ok(HostResult::Dataset(DatasetResult::Mutated { version: 2 })),
                    }
                }
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for PersistedDataset {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Dataset(DatasetRequest::Attributes { .. }) => {
                    Ok(HostResult::Dataset(DatasetResult::Attributes {
                        attributes: DatasetAttributes {
                            organization: DatasetOrganization::KeySequenced,
                            record_format: RecordFormat::Variable,
                            logical_record_length: 8,
                            key_offset: Some(0),
                            key_length: Some(3),
                            ccsid: None,
                        },
                        version: 1,
                    }))
                }
                HostRequest::Dataset(DatasetRequest::Write { records, .. }) => {
                    if records.len() != 1 {
                        Err(HostProblem::Malformed)
                    } else {
                        *self.record.lock().unwrap() = records.first().cloned();
                        Ok(HostResult::Dataset(DatasetResult::Mutated { version: 2 }))
                    }
                }
                HostRequest::Dataset(DatasetRequest::Read { key, .. }) => {
                    let Some(record) = self.record.lock().unwrap().clone() else {
                        return EffectResult {
                            sequence: effect.sequence,
                            outcome: Err(HostProblem::NotFound),
                        };
                    };
                    Ok(HostResult::Dataset(DatasetResult::Records {
                        records: vec![record],
                        identities: vec![key.unwrap_or_else(|| b"KEY".to_vec())],
                        version: 2,
                    }))
                }
                HostRequest::Dataset(_) => Err(HostProblem::Unsupported),
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for CountedMutationProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            self.commits.fetch_add(1, Ordering::SeqCst);
            let outcome = match effect.request {
                HostRequest::Dataset(_) => {
                    Ok(HostResult::Dataset(DatasetResult::Mutated { version: 2 }))
                }
                HostRequest::Program(_) => {
                    Ok(HostResult::Program(bounded(b"CHILD".to_vec()).unwrap()))
                }
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for SyncpointOriginProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
            let nested = invocation
                .bindings
                .get(CICS_NESTED_EFFECT_ORIGIN_BINDING)
                .and_then(|value| String::from_utf8(value.bytes().to_vec()).ok());
            let outer = invocation
                .bindings
                .get(CICS_OUTER_EFFECT_ORIGIN_BINDING)
                .and_then(|value| String::from_utf8(value.bytes().to_vec()).ok());
            let key = effect.idempotency_key.as_ref().map(ToString::to_string);
            assert_eq!(nested, key);
            self.seen.lock().unwrap().push((
                self.descriptor.capability.as_str().into(),
                nested.unwrap(),
                outer.expect("syncpoint dispatch carries its exact outer effect"),
            ));
            let outcome = match effect.request {
                HostRequest::Db2(_) => Ok(HostResult::Db2(mainframe_env_host_api::Db2Result {
                    sqlcode: 0,
                    sqlstate: "00000".into(),
                    message: String::new(),
                    rows: Vec::new(),
                    affected_rows: 0,
                })),
                HostRequest::Ims(_) => Ok(HostResult::Ims(mainframe_env_host_api::ImsResult {
                    status: "  ".into(),
                    segments: Vec::new(),
                    checkpoint_id: None,
                    affected_segments: 0,
                })),
                HostRequest::Mq(_) => Ok(HostResult::Mq(mainframe_env_host_api::MqResult {
                    completion_code: 0,
                    reason_code: 0,
                    handle: None,
                    message: Vec::new(),
                    message_id: None,
                    correlation_id: None,
                    trigger_program: None,
                })),
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    fn descriptor(capability: &str) -> CapabilityDescriptor {
        let limits = InvocationLimits::default();
        CapabilityDescriptor {
            capability: CapabilityId::new(capability, limits).unwrap(),
            provider_id: format!("test-{capability}"),
            generation: "1".into(),
            request_schema: "request@1".into(),
            result_schema: "result@1".into(),
            max_request_bytes: 4 * 1024 * 1024,
            max_result_bytes: 4 * 1024 * 1024,
            ready: true,
        }
    }

    fn authorities() -> Arc<ScopedHostService> {
        let providers = [
            "host.security.authorize",
            "host.dataset.read",
            "host.dataset.write",
            "host.program.invoke",
            "host.clock",
        ]
        .into_iter()
        .map(|capability| {
            Arc::new(Authority {
                descriptor: descriptor(capability),
            }) as Arc<dyn HostProvider>
        })
        .collect();
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ))
    }

    fn command_authorities(deny_command: bool) -> (Arc<ScopedHostService>, CommandSecurityTrace) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(CommandSecurityAuthority {
            descriptor: descriptor("host.security.authorize"),
            deny_command,
            seen: seen.clone(),
        }) as Arc<dyn HostProvider>;
        (
            Arc::new(ScopedHostService::new(
                Arc::new(
                    RegistrySnapshot::new(1, vec![provider], InvocationLimits::default()).unwrap(),
                ),
                HostLimits::default(),
            )),
            seen,
        )
    }

    fn traced_authorities(trace: Arc<DatasetTrace>) -> Arc<ScopedHostService> {
        let mut providers = [
            "host.security.authorize",
            "host.program.invoke",
            "host.clock",
        ]
        .into_iter()
        .map(|capability| {
            Arc::new(Authority {
                descriptor: descriptor(capability),
            }) as Arc<dyn HostProvider>
        })
        .collect::<Vec<_>>();
        for capability in ["host.dataset.read", "host.dataset.write"] {
            providers.push(Arc::new(TracedDataset {
                descriptor: descriptor(capability),
                trace: trace.clone(),
            }));
        }
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ))
    }

    fn persisted_dataset_authorities(
        record: Arc<Mutex<Option<Vec<u8>>>>,
    ) -> Arc<ScopedHostService> {
        let mut providers = [
            "host.security.authorize",
            "host.program.invoke",
            "host.clock",
        ]
        .into_iter()
        .map(|capability| {
            Arc::new(Authority {
                descriptor: descriptor(capability),
            }) as Arc<dyn HostProvider>
        })
        .collect::<Vec<_>>();
        for capability in ["host.dataset.read", "host.dataset.write"] {
            providers.push(Arc::new(PersistedDataset {
                descriptor: descriptor(capability),
                record: record.clone(),
            }));
        }
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ))
    }

    fn syncpoint_origin_authorities(
        seen: Arc<Mutex<Vec<(String, String, String)>>>,
    ) -> Arc<ScopedHostService> {
        let mut providers = [
            "host.security.authorize",
            "host.dataset.read",
            "host.dataset.write",
            "host.program.invoke",
            "host.clock",
        ]
        .into_iter()
        .map(|capability| {
            Arc::new(Authority {
                descriptor: descriptor(capability),
            }) as Arc<dyn HostProvider>
        })
        .collect::<Vec<_>>();
        for capability in ["host.db2.write", "host.ims.write", "host.mq.write"] {
            providers.push(Arc::new(SyncpointOriginProvider {
                descriptor: descriptor(capability),
                seen: seen.clone(),
            }));
        }
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ))
    }

    #[allow(dead_code)]
    fn replay_authorities(
        dataset_commits: Arc<AtomicUsize>,
        program_commits: Arc<AtomicUsize>,
    ) -> Arc<ScopedHostService> {
        let mut providers = ["host.security.authorize", "host.dataset.read", "host.clock"]
            .into_iter()
            .map(|capability| {
                Arc::new(Authority {
                    descriptor: descriptor(capability),
                }) as Arc<dyn HostProvider>
            })
            .collect::<Vec<_>>();
        providers.push(Arc::new(CountedMutationProvider {
            descriptor: descriptor("host.dataset.write"),
            commits: dataset_commits,
        }));
        providers.push(Arc::new(CountedMutationProvider {
            descriptor: descriptor("host.program.invoke"),
            commits: program_commits,
        }));
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ))
    }

    fn invocation() -> Invocation {
        invocation_for("run", BTreeMap::new())
    }

    fn invocation_for(run: &str, bindings: BTreeMap<String, BoundedPayload>) -> Invocation {
        let limits = InvocationLimits::default();
        let grants = [
            "host.security.authorize",
            "host.dataset.read",
            "host.dataset.write",
            "host.program.invoke",
            "host.cics.execute",
            "host.clock",
            "host.db2.write",
            "host.ims.write",
            "host.mq.write",
        ]
        .into_iter()
        .map(|name| CapabilityId::new(name, limits).unwrap())
        .collect::<BTreeSet<_>>();
        Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("cics:MENU", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(PrincipalId::new("IBMUSER", limits).unwrap(), grants, limits).unwrap(),
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new("trace", limits).unwrap(),
            IdempotencyKey::new("invocation", limits).unwrap(),
            1,
            ResourceLimits::default(),
            bindings,
            limits,
        )
        .unwrap()
    }

    fn service(store: Arc<dyn ProviderStateStore>) -> Arc<CicsService> {
        CicsService::open(authorities(), store, CicsLimits::default()).unwrap()
    }

    fn registered(service: &CicsService) -> (Invocation, SessionId) {
        let invocation = invocation();
        let session = SessionId::new("session", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        (invocation, session)
    }

    fn argument(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.argument@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn cics_literal(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.literal@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn cics_option() -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.option@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn enqueue_identity(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.storage-identity@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn enqueue_value(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.storage-value@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn cics_decimal(value: i64) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.decimal@1",
            value.to_string().into_bytes(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn condition_list(names: &[&str]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.condition-list@1",
            names.join("\n").into_bytes(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn condition_handlers(entries: &[(&str, &str)]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.condition-handlers@1",
            entries
                .iter()
                .map(|(name, label)| format!("{name}\t{label}"))
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn aid_handlers(entries: &[(&str, &str)]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.aid-handlers@1",
            entries
                .iter()
                .map(|(name, label)| format!("{name}\t{label}"))
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn task_value(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.storage-value@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn storage_target(value: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.storage-target@1",
            value.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn enqueue_model(
        name: &str,
        enqueue_name: &str,
        enqueue_scope: Option<&str>,
        enabled: bool,
    ) -> CicsEnqueueModelDefinition {
        CicsEnqueueModelDefinition {
            name: name.into(),
            enqueue_name: enqueue_name.into(),
            enqueue_scope: enqueue_scope.map(str::to_string),
            enabled,
        }
    }

    fn request(
        operation: CicsOperation,
        arguments: BTreeMap<String, BoundedPayload>,
        sequence: u64,
    ) -> CicsRequest {
        CicsRequest {
            operation,
            arguments,
            condition_policy: CicsConditionPolicy::Default,
            mutation: operation.is_mutating().then(|| Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("outer-{sequence}"),
                    InvocationLimits::default(),
                )
                .unwrap(),
                transaction: Some("MENU".into()),
            }),
        }
    }

    fn effect(run: &RunUnitId, request: CicsRequest, sequence: u64) -> EffectRequest {
        EffectRequest {
            run_unit: run.clone(),
            sequence,
            deadline_tick: 100,
            idempotency_key: request
                .mutation
                .as_ref()
                .map(|mutation| mutation.idempotency_key.clone()),
            request: HostRequest::Cics(request),
        }
    }

    fn delay_work_id(request: &CicsRequest) -> String {
        let mutation = request.mutation.as_ref().unwrap();
        let digest = canonical_request_digest(&HostRequest::Cics(request.clone())).unwrap();
        let identity = format!(
            "{:x}",
            Sha256::digest([mutation.idempotency_key.as_str().as_bytes(), &digest].concat())
        );
        format!("cics-delay:{}", &identity[..32])
    }

    fn completed_cics_effect(
        invocation: &Invocation,
        request: &CicsRequest,
        response: &CicsResponse,
        sequence: u64,
    ) -> EffectRecord {
        let limits = InvocationLimits::default();
        EffectRecord {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence,
            key: request.mutation.as_ref().unwrap().idempotency_key.clone(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: canonical_request_digest(&HostRequest::Cics(request.clone())).unwrap(),
            intent: EffectIntentMetadata {
                owner: invocation.execution_id.clone(),
                attempt: 1,
                capability: Some(CapabilityId::new("host.cics.execute", limits).unwrap()),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 2,
                epoch: 1,
                recovery_lease: None,
            },
            state: EffectState::Completed,
            result_digest: Some(
                canonical_result_digest(&Ok(HostResult::Cics(response.clone()))).unwrap(),
            ),
            resolved_tick: Some(2),
        }
    }

    #[test]
    fn shared_catalog_recognizes_all_frozen_forms() {
        let cases = [
            ("ABEND", CicsOperation::Abend),
            ("ADDRESS SET", CicsOperation::AddressSet),
            ("ASKTIME", CicsOperation::AsktimeEib),
            ("ASKTIME ABSTIME(ABS-TIME)", CicsOperation::Asktime),
            ("ASSIGN", CicsOperation::Assign),
            ("PURGE MESSAGE", CicsOperation::PurgeMessage),
            ("CHANGE TASK", CicsOperation::ChangeTask),
            ("CANCEL", CicsOperation::Cancel),
            ("DEQ", CicsOperation::Deq),
            ("DELAY", CicsOperation::Delay),
            ("DELETE", CicsOperation::Delete),
            ("ENDBR", CicsOperation::EndBrowse),
            ("ENQ", CicsOperation::Enq),
            ("FORMATTIME", CicsOperation::FormatTime),
            ("HANDLE ABEND", CicsOperation::HandleAbend),
            ("HANDLE AID", CicsOperation::HandleAid),
            ("HANDLE CONDITION", CicsOperation::HandleCondition),
            ("IGNORE CONDITION ERROR", CicsOperation::IgnoreCondition),
            ("INQUIRE PROGRAM(PGM)", CicsOperation::Inquire),
            ("LINK", CicsOperation::Link),
            ("POP HANDLE", CicsOperation::PopHandle),
            ("PUSH HANDLE", CicsOperation::PushHandle),
            ("READ", CicsOperation::Read),
            ("READNEXT", CicsOperation::ReadNext),
            ("READPREV", CicsOperation::ReadPrev),
            ("RECEIVE MAP", CicsOperation::ReceiveMap),
            ("RETRIEVE", CicsOperation::Retrieve),
            ("RETURN", CicsOperation::Return),
            ("REWRITE", CicsOperation::Rewrite),
            ("SEND TEXT", CicsOperation::SendText),
            ("SEND MAP", CicsOperation::SendMap),
            (
                "SET ASSOCIATION USERCORRDATA(DATA-X)",
                CicsOperation::SetAssociationUserCorrData,
            ),
            ("START", CicsOperation::Start),
            ("STARTBR", CicsOperation::StartBrowse),
            ("SUSPEND", CicsOperation::Suspend),
            ("SYNCPOINT", CicsOperation::Syncpoint),
            ("WRITE", CicsOperation::Write),
            ("WRITEQ TD", CicsOperation::WriteTransientData),
            ("XCTL", CicsOperation::Xctl),
        ];
        for (source, expected) in cases {
            let tokens = format!("EXEC CICS {source} END-EXEC")
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            assert_eq!(CicsOperation::from_tokens(&tokens), Some(expected));
        }
    }

    #[test]
    fn generated_command_descriptors_are_total_and_family_routed() {
        assert_eq!(CICS_COMMAND_DESCRIPTORS.len(), 40);
        let mut operations = BTreeSet::new();
        let mut rows = BTreeSet::new();
        let mut families = BTreeSet::new();
        for descriptor in CICS_COMMAND_DESCRIPTORS {
            assert!(operations.insert(format!("{:?}", descriptor.operation)));
            assert!(rows.insert(descriptor.official_row));
            assert!(!descriptor.syntax.is_empty());
            assert_eq!(descriptor.mutating, descriptor.operation.is_mutating());
            assert_eq!(command_descriptor(descriptor.operation), descriptor);
            families.insert(format!("{:?}", descriptor.family));
        }
        assert_eq!(families.len(), 8);
        let asktime = command_descriptor(CicsOperation::Asktime);
        assert_eq!(asktime.syntax, "ASKTIME ABSTIME");
        assert_eq!(
            asktime.official_row,
            "ibm-cics-ts-6x-2026-08-31:api-commands:0010"
        );
        let bare_asktime = command_descriptor(CicsOperation::AsktimeEib);
        assert_eq!(bare_asktime.syntax, "ASKTIME");
        assert_eq!(
            bare_asktime.official_row,
            "ibm-cics-ts-6x-2026-08-31:api-commands:0009"
        );
    }

    #[test]
    fn interval_start_work_promotes_and_retrieve_consumes_exactly_once() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let work_store: Arc<dyn WorkStore> = store.clone();
        let service = CicsService::open_with_runtime(
            authorities(),
            provider_store,
            work_store,
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let (issuer, _) = registered(&service);
        let start = request(
            CicsOperation::Start,
            BTreeMap::from([
                ("TRANSID".into(), argument(b"NEXT")),
                ("REQID".into(), argument(b"REQ0001")),
                ("FROM".into(), argument(b"PAYLOAD")),
                ("LENGTH".into(), cics_decimal(4)),
                ("INTERVAL".into(), cics_decimal(0)),
                ("RTRANSID".into(), argument(b"BACK")),
                ("RTERMID".into(), argument(b"T001")),
                ("QUEUE".into(), argument(b"WORKQ")),
                ("OPTION.FMH".into(), cics_option()),
            ]),
            1,
        );
        let started = service
            .invoke(
                &effect(&issuer.run_unit_id, start.clone(), 1),
                start.clone(),
            )
            .unwrap();
        assert_eq!((started.response, started.response2), (0, 0));

        let work = store
            .claim("cics-worker", Some(CICS_START_WORK_GENERATION), 1_000, 100)
            .unwrap()
            .unwrap();
        assert_eq!(work.payload, b"REQ0001");
        service.promote_start_work(&work, 1_000).unwrap();
        store
            .complete(
                &work.work_id,
                work.lease_id.as_deref().unwrap(),
                work.lease_epoch,
                1_000,
            )
            .unwrap();

        let next = invocation_for("run-next", BTreeMap::new());
        let next_session = SessionId::new("next-session", 64).unwrap();
        service.create_session(&next_session, 24, 80).unwrap();
        service
            .register_run(next.clone(), &next_session, "NEXT", "MEAPPL", "MESYS")
            .unwrap();
        let mut retrieve = request(
            CicsOperation::Retrieve,
            BTreeMap::from([
                ("INTO".into(), argument(b"DATA-OUT")),
                ("LENGTH".into(), cics_decimal(16)),
                ("RTRANSID".into(), argument(b"RTRANS-X")),
                ("RTERMID".into(), argument(b"RTERM-X")),
                ("QUEUE".into(), argument(b"QUEUE-X")),
            ]),
            2,
        );
        retrieve.mutation.as_mut().unwrap().transaction = Some("NEXT".into());
        let retrieved = service
            .invoke(
                &effect(&next.run_unit_id, retrieve.clone(), 2),
                retrieve.clone(),
            )
            .unwrap();
        assert_eq!(retrieved.outputs["INTO"].bytes(), b"PAYL");
        assert_eq!(retrieved.outputs["LENGTH"].bytes(), b"4");
        assert_eq!(retrieved.outputs["RTRANSID"].bytes(), b"BACK");
        assert_eq!(retrieved.outputs["RTERMID"].bytes(), b"T001");
        assert_eq!(retrieved.outputs["QUEUE"].bytes(), b"WORKQ");
        assert_eq!(retrieved.outputs["EIBFMH"].bytes(), &[0xff]);
        assert_eq!(retrieved.payload.bytes(), b"PAYL");
        let replayed = service
            .invoke(
                &effect(&next.run_unit_id, retrieve.clone(), 2),
                retrieve.clone(),
            )
            .unwrap();
        assert_eq!(replayed, retrieved);

        let mut exhausted = request(
            CicsOperation::Retrieve,
            BTreeMap::from([
                ("INTO".into(), argument(b"DATA-OUT")),
                ("LENGTH".into(), cics_decimal(16)),
            ]),
            3,
        );
        exhausted.mutation.as_mut().unwrap().transaction = Some("NEXT".into());
        exhausted.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        };
        let exhausted = service
            .invoke(&effect(&next.run_unit_id, exhausted.clone(), 3), exhausted)
            .unwrap();
        assert_eq!(
            (
                exhausted.condition.as_str(),
                exhausted.response,
                exhausted.response2
            ),
            ("ENDDATA", 29, 0)
        );
    }

    #[test]
    fn interval_start_work_and_retrieve_survive_sqlite_reopen() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-start-retrieve-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let provider_store: Arc<dyn ProviderStateStore> = store.clone();
            let work_store: Arc<dyn WorkStore> = store;
            let service = CicsService::open_with_runtime(
                authorities(),
                provider_store,
                work_store,
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            let (issuer, _) = registered(&service);
            let start = request(
                CicsOperation::Start,
                BTreeMap::from([
                    ("TRANSID".into(), argument(b"NEXT")),
                    ("REQID".into(), argument(b"REQSQL01")),
                    ("FROM".into(), argument(b"RESTART")),
                    ("LENGTH".into(), cics_decimal(7)),
                    ("INTERVAL".into(), cics_decimal(0)),
                ]),
                1,
            );
            service
                .invoke(&effect(&issuer.run_unit_id, start.clone(), 1), start)
                .unwrap();
        }

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let provider_store: Arc<dyn ProviderStateStore> = store.clone();
            let work_store: Arc<dyn WorkStore> = store.clone();
            let service = CicsService::open_with_runtime(
                authorities(),
                provider_store,
                work_store,
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            let work = store
                .claim("cics-worker", Some(CICS_START_WORK_GENERATION), 1_000, 100)
                .unwrap()
                .unwrap();
            assert_eq!(work.payload, b"REQSQL01");
            service.promote_start_work(&work, 1_000).unwrap();
            store
                .complete(
                    &work.work_id,
                    work.lease_id.as_deref().unwrap(),
                    work.lease_epoch,
                    1_000,
                )
                .unwrap();

            let next = invocation_for("sqlite-run-next", BTreeMap::new());
            let next_session = SessionId::new("sqlite-next-session", 64).unwrap();
            service.create_session(&next_session, 24, 80).unwrap();
            service
                .register_run(next.clone(), &next_session, "NEXT", "MEAPPL", "MESYS")
                .unwrap();
            let mut retrieve = request(
                CicsOperation::Retrieve,
                BTreeMap::from([
                    ("INTO".into(), argument(b"DATA-OUT")),
                    ("LENGTH".into(), cics_decimal(16)),
                ]),
                2,
            );
            retrieve.mutation.as_mut().unwrap().transaction = Some("NEXT".into());
            let response = service
                .invoke(&effect(&next.run_unit_id, retrieve.clone(), 2), retrieve)
                .unwrap();
            assert_eq!(response.payload.bytes(), b"RESTART");
            assert_eq!(response.outputs["LENGTH"].bytes(), b"7");
            assert_eq!(response.outputs["EIBFMH"].bytes(), &[0x00]);
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interval_cancel_start_is_replay_safe_and_survives_sqlite_reopen() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-cancel-start-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store.clone(),
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            let (issuer, _) = registered(&service);
            let start = request(
                CicsOperation::Start,
                BTreeMap::from([
                    ("TRANSID".into(), argument(b"NEXT")),
                    ("REQID".into(), argument(b"CAN0001")),
                    ("FROM".into(), argument(b"CANCELME")),
                    ("INTERVAL".into(), cics_decimal(100)),
                ]),
                1,
            );
            service
                .invoke(&effect(&issuer.run_unit_id, start.clone(), 1), start)
                .unwrap();
            let cancel = request(
                CicsOperation::Cancel,
                BTreeMap::from([
                    ("REQID".into(), argument(b"CAN0001")),
                    ("TRANSID".into(), argument(b"NEXT")),
                ]),
                2,
            );
            let cancelled = service
                .invoke(
                    &effect(&issuer.run_unit_id, cancel.clone(), 2),
                    cancel.clone(),
                )
                .unwrap();
            assert_eq!((cancelled.response, cancelled.response2), (0, 0));
            assert_eq!(
                service
                    .invoke(&effect(&issuer.run_unit_id, cancel.clone(), 2), cancel)
                    .unwrap(),
                cancelled
            );
            let work = store.get_work("cics-start:CAN0001").unwrap().unwrap();
            assert_eq!(work.state, WorkState::Cancelled);
            assert!(work.cancellation_requested);
        }

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store.clone(),
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            let invocation = invocation_for("cancel-reopen", BTreeMap::new());
            let session = SessionId::new("cancel-reopen-session", 64).unwrap();
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let mut cancel = request(
                CicsOperation::Cancel,
                BTreeMap::from([("REQID".into(), argument(b"CAN0001"))]),
                3,
            );
            cancel.condition_policy = CicsConditionPolicy::Respond {
                response_field: "RESP".into(),
                response2_field: Some("RESP2".into()),
            };
            let response = service
                .invoke(&effect(&invocation.run_unit_id, cancel.clone(), 3), cancel)
                .unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                ("NOTFND", 13, 0)
            );
            assert!(
                store
                    .claim(
                        "cancel-worker",
                        Some(CICS_START_WORK_GENERATION),
                        100_000,
                        100
                    )
                    .unwrap()
                    .is_none()
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interval_cancel_fences_a_claimed_but_unhonored_start() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let (issuer, _) = registered(&service);
        let start = request(
            CicsOperation::Start,
            BTreeMap::from([
                ("TRANSID".into(), argument(b"NEXT")),
                ("REQID".into(), argument(b"CANRACE")),
                ("FROM".into(), argument(b"RACE")),
                ("INTERVAL".into(), cics_decimal(0)),
            ]),
            1,
        );
        service
            .invoke(&effect(&issuer.run_unit_id, start.clone(), 1), start)
            .unwrap();
        let work = store
            .claim("race-worker", Some(CICS_START_WORK_GENERATION), 1_000, 100)
            .unwrap()
            .unwrap();
        let cancel = request(
            CicsOperation::Cancel,
            BTreeMap::from([("REQID".into(), argument(b"CANRACE"))]),
            2,
        );
        service
            .invoke(&effect(&issuer.run_unit_id, cancel.clone(), 2), cancel)
            .unwrap();
        let cancelled = store.get_work(&work.work_id).unwrap().unwrap();
        assert_eq!(cancelled.state, WorkState::Claimed);
        assert!(cancelled.cancellation_requested);
        assert_eq!(
            service.promote_start_work(&work, 1_000),
            Err(HostProblem::InfrastructureFailure)
        );
        let released = store
            .release(
                &work.work_id,
                work.lease_id.as_deref().unwrap(),
                work.lease_epoch,
                1_000,
                1_001,
            )
            .unwrap();
        assert_eq!(released.state, WorkState::Cancelled);
    }

    #[test]
    fn interval_delay_zero_completes_without_timer_state() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        for (sequence, arguments) in [
            (1, BTreeMap::new()),
            (2, BTreeMap::from([("INTERVAL".into(), cics_decimal(0))])),
        ] {
            let request = request(CicsOperation::Delay, arguments, sequence);
            let response = service
                .invoke(
                    &effect(&invocation.run_unit_id, request.clone(), sequence),
                    request,
                )
                .unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                ("NORMAL", 0, 0)
            );
        }

        let mut invalid = request(
            CicsOperation::Delay,
            BTreeMap::from([("INTERVAL".into(), cics_decimal(1_000_000))]),
            3,
        );
        invalid.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        };
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, invalid.clone(), 3),
                invalid,
            )
            .unwrap();
        assert_eq!(
            (
                response.condition.as_str(),
                response.response,
                response.response2
            ),
            ("INVREQ", 16, 4)
        );
    }

    #[test]
    fn positive_interval_delay_suspends_promotes_replays_and_repeats() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:42", invocation.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
        ]);
        let first = request(CicsOperation::Delay, arguments.clone(), 1);
        let suspended = service
            .invoke(
                &effect(&invocation.run_unit_id, first.clone(), 1),
                first.clone(),
            )
            .unwrap();
        assert_eq!(suspended.disposition, CicsDisposition::Suspended);
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, first.clone(), 1), first)
                .unwrap(),
            suspended
        );
        assert!(
            store
                .claim("delay-worker", Some(CICS_DELAY_WORK_GENERATION), 1_999, 100)
                .unwrap()
                .is_none()
        );
        let work = store
            .claim("delay-worker", Some(CICS_DELAY_WORK_GENERATION), 2_000, 100)
            .unwrap()
            .unwrap();
        service.promote_delay_work(&work, 2_000).unwrap();
        store
            .complete(
                &work.work_id,
                work.lease_id.as_deref().unwrap(),
                work.lease_epoch,
                2_000,
            )
            .unwrap();
        let second = request(CicsOperation::Delay, arguments.clone(), 2);
        let completed = service
            .invoke(
                &effect(&invocation.run_unit_id, second.clone(), 2),
                second.clone(),
            )
            .unwrap();
        assert_eq!(completed.disposition, CicsDisposition::Complete);
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, second.clone(), 2), second)
                .unwrap(),
            completed
        );

        let repeated = request(CicsOperation::Delay, arguments, 3);
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, repeated.clone(), 3),
                repeated,
            )
            .unwrap();
        assert_eq!(response.disposition, CicsDisposition::Suspended);
        let next = store
            .claim(
                "next-delay-worker",
                Some(CICS_DELAY_WORK_GENERATION),
                2_000,
                100,
            )
            .unwrap()
            .unwrap();
        assert_ne!(next.work_id, work.work_id);
    }

    #[test]
    fn named_delay_cancel_is_other_task_only_and_returns_response2_23() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let (issuer, _) = registered(&service);
        let canceller = invocation_for("delay-canceller", BTreeMap::new());
        let canceller_session = SessionId::new("delay-canceller-session", 64).unwrap();
        service.create_session(&canceller_session, 24, 80).unwrap();
        service
            .register_run(
                canceller.clone(),
                &canceller_session,
                "MENU",
                "MEAPPL",
                "MESYS",
            )
            .unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:42", issuer.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            ("REQID".into(), cics_literal(b"WAIT0001")),
        ]);
        let first = request(CicsOperation::Delay, arguments.clone(), 1);
        let work_id = delay_work_id(&first);
        assert_eq!(
            service
                .invoke(&effect(&issuer.run_unit_id, first.clone(), 1), first)
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );

        let mut self_cancel = request(
            CicsOperation::Cancel,
            BTreeMap::from([("REQID".into(), argument(b"WAIT0001"))]),
            10,
        );
        self_cancel.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        };
        let response = service
            .invoke(
                &effect(&issuer.run_unit_id, self_cancel.clone(), 10),
                self_cancel,
            )
            .unwrap();
        assert_eq!(
            (response.condition.as_str(), response.response),
            ("NOTFND", 13)
        );

        let mut duplicate_arguments = arguments.clone();
        duplicate_arguments.insert(
            "DELAY.ID".into(),
            BoundedPayload::new(
                "mainframe-env.cics.delay-id@1",
                format!("{}:99", canceller.run_unit_id).into_bytes(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut duplicate = request(CicsOperation::Delay, duplicate_arguments, 9);
        duplicate.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        };
        let response = service
            .invoke(
                &effect(&canceller.run_unit_id, duplicate.clone(), 9),
                duplicate,
            )
            .unwrap();
        assert_eq!(
            (response.condition.as_str(), response.response),
            ("INVREQ", 16)
        );

        let cancel = request(
            CicsOperation::Cancel,
            BTreeMap::from([("REQID".into(), argument(b"WAIT0001"))]),
            11,
        );
        let cancelled = service
            .invoke(
                &effect(&canceller.run_unit_id, cancel.clone(), 11),
                cancel.clone(),
            )
            .unwrap();
        assert_eq!((cancelled.response, cancelled.response2), (0, 0));
        assert_eq!(
            service
                .invoke(&effect(&canceller.run_unit_id, cancel.clone(), 11), cancel)
                .unwrap(),
            cancelled
        );
        let work = store.get_work(&work_id).unwrap().unwrap();
        assert_eq!(work.state, WorkState::Cancelled);

        let resumed = request(CicsOperation::Delay, arguments, 2);
        let response = service
            .invoke(&effect(&issuer.run_unit_id, resumed.clone(), 2), resumed)
            .unwrap();
        assert_eq!(response.disposition, CicsDisposition::Complete);
        assert_eq!((response.response, response.response2), (0, 23));
    }

    #[test]
    fn named_delay_cancel_loses_to_expiration_boundary() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(TestCicsClock::fixed(1_000));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            clock.clone(),
        )
        .unwrap();
        let (issuer, _) = registered(&service);
        let canceller = invocation_for("expired-delay-canceller", BTreeMap::new());
        let session = SessionId::new("expired-delay-canceller-session", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(canceller.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:77", issuer.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            ("REQID".into(), cics_literal(b"EXPIRE01")),
        ]);
        let delay = request(CicsOperation::Delay, arguments.clone(), 1);
        service
            .invoke(&effect(&issuer.run_unit_id, delay.clone(), 1), delay)
            .unwrap();
        clock.tick.store(2_000, Ordering::SeqCst);

        let mut cancel = request(
            CicsOperation::Cancel,
            BTreeMap::from([("REQID".into(), argument(b"EXPIRE01"))]),
            10,
        );
        cancel.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP".into(),
            response2_field: Some("RESP2".into()),
        };
        let response = service
            .invoke(&effect(&canceller.run_unit_id, cancel.clone(), 10), cancel)
            .unwrap();
        assert_eq!(
            (response.condition.as_str(), response.response),
            ("NOTFND", 13)
        );

        let work = store
            .claim(
                "expired-delay-worker",
                Some(CICS_DELAY_WORK_GENERATION),
                2_000,
                100,
            )
            .unwrap()
            .unwrap();
        service.promote_delay_work(&work, 2_000).unwrap();
        store
            .complete(
                &work.work_id,
                work.lease_id.as_deref().unwrap(),
                work.lease_epoch,
                2_000,
            )
            .unwrap();
        let resumed = request(CicsOperation::Delay, arguments, 2);
        let response = service
            .invoke(&effect(&issuer.run_unit_id, resumed.clone(), 2), resumed)
            .unwrap();
        assert_eq!((response.response, response.response2), (0, 0));
    }

    #[test]
    fn terminal_task_cleanup_abandons_delay_and_cancels_work() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let issuer = invocation_for("delay-cleanup", BTreeMap::new());
        let session = SessionId::new("delay-cleanup-session", 64).unwrap();
        service
            .launch_terminal(
                issuer.clone(),
                &session,
                "MENU",
                24,
                80,
                "delay-cleanup-csrf",
                1_000,
                10_000,
            )
            .unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:9", issuer.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
        ]);
        let delay = request(CicsOperation::Delay, arguments.clone(), 1);
        let work_id = delay_work_id(&delay);
        service
            .invoke(&effect(&issuer.run_unit_id, delay.clone(), 1), delay)
            .unwrap();
        service
            .disconnect_terminal(&session, issuer.principal.id(), "delay-cleanup-csrf", 1_000)
            .unwrap();
        let work = store.get_work(&work_id).unwrap().unwrap();
        assert_eq!(work.state, WorkState::Cancelled);
        assert!(
            store
                .claim(
                    "abandoned-delay-worker",
                    Some(CICS_DELAY_WORK_GENERATION),
                    2_000,
                    100
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn terminal_timeout_abandons_delay_and_cancels_work() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_runtime(
            authorities(),
            store.clone(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(1_000)),
        )
        .unwrap();
        let issuer = invocation_for("delay-timeout", BTreeMap::new());
        let session = SessionId::new("delay-timeout-session", 64).unwrap();
        service
            .launch_terminal(
                issuer.clone(),
                &session,
                "MENU",
                24,
                80,
                "delay-timeout-csrf",
                1_000,
                1,
            )
            .unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:9", issuer.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
        ]);
        let delay = request(CicsOperation::Delay, arguments, 1);
        let work_id = delay_work_id(&delay);
        service
            .invoke(&effect(&issuer.run_unit_id, delay.clone(), 1), delay)
            .unwrap();
        assert_eq!(
            service.terminal_execution(&session, issuer.principal.id(), 1_001),
            Err(HostProblem::TimedOut)
        );
        let work = store.get_work(&work_id).unwrap().unwrap();
        assert_eq!(work.state, WorkState::Cancelled);
    }

    #[test]
    fn positive_interval_delay_survives_sqlite_reopen() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-delay-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let invocation = invocation_for("delay-reopen", BTreeMap::new());
        let session = SessionId::new("delay-reopen-session", 64).unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:7", invocation.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
        ]);

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store,
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let first = request(CicsOperation::Delay, arguments.clone(), 1);
            assert_eq!(
                service
                    .invoke(&effect(&invocation.run_unit_id, first.clone(), 1), first)
                    .unwrap()
                    .disposition,
                CicsDisposition::Suspended
            );
        }

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store.clone(),
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(2_000)),
            )
            .unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let work = store
                .claim(
                    "delay-reopen-worker",
                    Some(CICS_DELAY_WORK_GENERATION),
                    2_000,
                    100,
                )
                .unwrap()
                .unwrap();
            service.promote_delay_work(&work, 2_000).unwrap();
            store
                .complete(
                    &work.work_id,
                    work.lease_id.as_deref().unwrap(),
                    work.lease_epoch,
                    2_000,
                )
                .unwrap();
            let second = request(CicsOperation::Delay, arguments, 2);
            assert_eq!(
                service
                    .invoke(&effect(&invocation.run_unit_id, second.clone(), 2), second)
                    .unwrap()
                    .disposition,
                CicsDisposition::Complete
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn named_delay_cancel_survives_sqlite_reopen() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-delay-cancel-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let issuer = invocation_for("named-delay-reopen", BTreeMap::new());
        let issuer_session = SessionId::new("named-delay-reopen-session", 64).unwrap();
        let arguments = BTreeMap::from([
            ("INTERVAL".into(), cics_decimal(1)),
            (
                "DELAY.ID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.delay-id@1",
                    format!("{}:7", issuer.run_unit_id).into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            ("REQID".into(), cics_literal(b"WAITSQL1")),
        ]);

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store,
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            service.create_session(&issuer_session, 24, 80).unwrap();
            service
                .register_run(issuer.clone(), &issuer_session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let delay = request(CicsOperation::Delay, arguments.clone(), 1);
            assert_eq!(
                service
                    .invoke(&effect(&issuer.run_unit_id, delay.clone(), 1), delay)
                    .unwrap()
                    .disposition,
                CicsDisposition::Suspended
            );
        }

        {
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = CicsService::open_with_runtime(
                authorities(),
                store.clone(),
                store,
                CicsLimits::default(),
                Arc::new(TestCicsClock::fixed(1_000)),
            )
            .unwrap();
            service
                .register_run(issuer.clone(), &issuer_session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let canceller = invocation_for("named-delay-cancel-reopen", BTreeMap::new());
            let canceller_session = SessionId::new("named-delay-cancel-session", 64).unwrap();
            service.create_session(&canceller_session, 24, 80).unwrap();
            service
                .register_run(
                    canceller.clone(),
                    &canceller_session,
                    "MENU",
                    "MEAPPL",
                    "MESYS",
                )
                .unwrap();
            let cancel = request(
                CicsOperation::Cancel,
                BTreeMap::from([("REQID".into(), argument(b"WAITSQL1"))]),
                10,
            );
            service
                .invoke(&effect(&canceller.run_unit_id, cancel.clone(), 10), cancel)
                .unwrap();
            let resumed = request(CicsOperation::Delay, arguments, 2);
            let response = service
                .invoke(&effect(&issuer.run_unit_id, resumed.clone(), 2), resumed)
                .unwrap();
            assert_eq!((response.response, response.response2), (0, 23));
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn carddemo_residual_cics_forms_are_executable() {
        for source in [
            "ASKTIME ABSTIME(ABS-TIME)",
            "FORMATTIME",
            "INQUIRE PROGRAM(COCRDLIC)",
            "LINK PROGRAM(COPAUS2C)",
            "RETRIEVE INTO(MQTM)",
            "SYNCPOINT",
            "SYNCPOINT ROLLBACK",
        ] {
            let tokens = format!("EXEC CICS {source} END-EXEC")
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let operation = CicsOperation::from_tokens(&tokens)
                .unwrap_or_else(|| panic!("CardDemo CICS form is untyped: {source}"));
            assert!(
                operation.supported(),
                "CardDemo CICS form is unsupported: {source}"
            );
        }
    }

    #[test]
    fn carddemo_time_inquire_link_retrieve_and_assign_subforms_execute() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);

        let asktime = request(CicsOperation::Asktime, BTreeMap::new(), 1);
        let asked = service
            .invoke(
                &effect(&invocation.run_unit_id, asktime.clone(), 1),
                asktime,
            )
            .unwrap();
        let absolute =
            String::from_utf8(asked.outputs.get("ABSTIME").unwrap().bytes().to_vec()).unwrap();
        assert_eq!(absolute.parse::<i64>().unwrap(), 3_997_082_096_789);
        assert_eq!(asked.outputs["EIBDATE"].bytes(), b"126242");
        assert_eq!(asked.outputs["EIBTIME"].bytes(), b"123456");

        let bare_asktime = request(CicsOperation::AsktimeEib, BTreeMap::new(), 330);
        let bare_asktime = service
            .invoke(
                &effect(&invocation.run_unit_id, bare_asktime.clone(), 330),
                bare_asktime,
            )
            .unwrap();
        assert_eq!(bare_asktime.condition, "NORMAL");
        assert!(!bare_asktime.outputs.contains_key("ABSTIME"));
        assert_eq!(bare_asktime.outputs["EIBDATE"].bytes(), b"126242");
        assert_eq!(bare_asktime.outputs["EIBTIME"].bytes(), b"123456");
        let malformed = request(
            CicsOperation::AsktimeEib,
            BTreeMap::from([("ABSTIME".into(), argument(b"ABS-TIME"))]),
            331,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed.clone(), 331),
                malformed,
            ),
            Err(HostProblem::Malformed)
        );

        let format = request(
            CicsOperation::FormatTime,
            BTreeMap::from([
                ("ABSTIME".into(), argument(absolute.as_bytes())),
                ("YYYYMMDD".into(), argument(b"DATE-OUT")),
                ("TIME".into(), argument(b"TIME-OUT")),
                ("MILLISECONDS".into(), argument(b"MS-OUT")),
                ("DATESEP".into(), argument(b"-")),
                ("TIMESEP".into(), argument(b":")),
            ]),
            2,
        );
        let formatted = service
            .invoke(&effect(&invocation.run_unit_id, format.clone(), 2), format)
            .unwrap();
        assert_eq!(formatted.outputs["YYYYMMDD"].bytes(), b"2026-08-30");
        assert_eq!(formatted.outputs["TIME"].bytes(), b"12:34:56");
        assert_eq!(formatted.outputs["MILLISECONDS"].bytes(), b"789");
        assert_eq!(
            formatted.outputs["MILLISECONDS"].schema(),
            "mainframe-env.cics.decimal@1"
        );
        let negative = request(
            CicsOperation::FormatTime,
            BTreeMap::from([("ABSTIME".into(), argument(b"-1"))]),
            332,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, negative.clone(), 332),
                negative,
            ),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 1,
            })
        );
        let unsupported = request(
            CicsOperation::FormatTime,
            BTreeMap::from([
                ("ABSTIME".into(), argument(absolute.as_bytes())),
                ("DAYCOUNT".into(), argument(b"DAY-OUT")),
            ]),
            333,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, unsupported.clone(), 333),
                unsupported,
            ),
            Err(HostProblem::Malformed)
        );

        let assign = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLICATION".into(), argument(b"APPLICATION-OUT")),
                ("APPLID".into(), argument(b"APP-OUT")),
                ("CHANNEL".into(), argument(b"CHANNEL-OUT")),
                ("CWALENG".into(), argument(b"CWA-LENGTH-OUT")),
                ("MAJORVERSION".into(), argument(b"MAJOR-OUT")),
                ("MICROVERSION".into(), argument(b"MICRO-OUT")),
                ("MINORVERSION".into(), argument(b"MINOR-OUT")),
                ("NEXTTRANSID".into(), argument(b"NEXT-TRANS-OUT")),
                ("OPERATION".into(), argument(b"OPERATION-OUT")),
                ("OPERKEYS".into(), argument(b"OPERKEYS-OUT")),
                ("PLATFORM".into(), argument(b"PLATFORM-OUT")),
                ("RESTART".into(), argument(b"RESTART-OUT")),
                ("SYSID".into(), argument(b"SYS-OUT")),
                ("TASKPRIORITY".into(), argument(b"PRIORITY-OUT")),
                ("TWALENG".into(), argument(b"TWA-LENGTH-OUT")),
                ("USERID".into(), argument(b"USER-OUT")),
            ]),
            3,
        );
        let assigned = service
            .invoke(&effect(&invocation.run_unit_id, assign.clone(), 3), assign)
            .unwrap();
        assert_eq!(assigned.outputs["APPLICATION"].bytes(), &[b' '; 64]);
        assert_eq!(assigned.outputs["APPLID"].bytes(), b"MEAPPL");
        assert_eq!(assigned.outputs["CHANNEL"].bytes(), &[b' '; 16]);
        assert_eq!(assigned.outputs["CWALENG"].bytes(), b"0");
        for name in ["MAJORVERSION", "MICROVERSION", "MINORVERSION"] {
            assert_eq!(
                assigned.outputs[name].schema(),
                "mainframe-env.cics.decimal@1"
            );
            assert_eq!(assigned.outputs[name].bytes(), b"-1");
        }
        assert_eq!(assigned.outputs["NEXTTRANSID"].bytes(), &[b' '; 4]);
        assert_eq!(assigned.outputs["OPERATION"].bytes(), &[b' '; 64]);
        assert_eq!(assigned.outputs["OPERKEYS"].bytes(), &[0; 8]);
        assert_eq!(assigned.outputs["PLATFORM"].bytes(), &[b' '; 64]);
        assert_eq!(assigned.outputs["RESTART"].bytes(), &[0]);
        assert_eq!(assigned.outputs["SYSID"].bytes(), b"MESYS");
        assert_eq!(
            assigned.outputs["TASKPRIORITY"].schema(),
            "mainframe-env.cics.decimal@1"
        );
        assert_eq!(assigned.outputs["TASKPRIORITY"].bytes(), b"0");
        assert_eq!(assigned.outputs["TWALENG"].bytes(), b"0");
        assert_eq!(assigned.outputs["USERID"].bytes(), b"IBMUSER");

        let local_ccsid = request(
            CicsOperation::Assign,
            BTreeMap::from([("LOCALCCSID".into(), argument(b"LOCAL-CCSID-OUT"))]),
            324,
        );
        let local_ccsid = service
            .invoke(
                &effect(&invocation.run_unit_id, local_ccsid.clone(), 324),
                local_ccsid,
            )
            .unwrap();
        assert_eq!(local_ccsid.condition, "NORMAL");
        assert_eq!(
            local_ccsid.outputs["LOCALCCSID"].schema(),
            "mainframe-env.cics.decimal@1"
        );
        assert_eq!(local_ccsid.outputs["LOCALCCSID"].bytes(), b"37");

        let local_only = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("CMDSEC".into(), argument(b"CMDSEC-OUT")),
                ("OPSECURITY".into(), argument(b"OPSECURITY-OUT")),
                ("RESSEC".into(), argument(b"RESSEC-OUT")),
                ("TCTUALENG".into(), argument(b"TCTUA-LENGTH-OUT")),
            ]),
            29,
        );
        let local_only = service
            .invoke(
                &effect(&invocation.run_unit_id, local_only.clone(), 29),
                local_only,
            )
            .unwrap();
        assert_eq!(local_only.outputs["CMDSEC"].bytes(), b"X");
        assert_eq!(local_only.outputs["OPSECURITY"].bytes(), &[0; 3]);
        assert_eq!(local_only.outputs["RESSEC"].bytes(), b"X");
        assert_eq!(local_only.outputs["TCTUALENG"].bytes(), b"0");

        let initparm = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("BRIDGE".into(), argument(b"BRIDGE-OUT")),
                ("INITPARM".into(), argument(b"INITPARM-OUT")),
                ("INITPARMLEN".into(), argument(b"INITPARM-LENGTH-OUT")),
            ]),
            28,
        );
        let initparm = service
            .invoke(
                &effect(&invocation.run_unit_id, initparm.clone(), 28),
                initparm,
            )
            .unwrap();
        assert!(!initparm.outputs.contains_key("INITPARM"));
        assert_eq!(initparm.outputs["INITPARMLEN"].bytes(), b"0");
        assert_eq!(initparm.outputs["BRIDGE"].bytes(), &[b' '; 4]);

        let mut no_ati = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("QNAME".into(), argument(b"QNAME-OUT")),
            ]),
            320,
        );
        no_ati.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_ati = service
            .invoke(
                &effect(&invocation.run_unit_id, no_ati.clone(), 320),
                no_ati,
            )
            .unwrap();
        assert_eq!(
            (no_ati.condition.as_str(), no_ati.response, no_ati.response2),
            ("INVREQ", 16, 4)
        );
        assert_eq!(no_ati.outputs["APPLID"].bytes(), b"MEAPPL");
        assert!(!no_ati.outputs.contains_key("QNAME"));

        let mut no_bts = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("ACTIVITY".into(), argument(b"ACTIVITY-OUT")),
                ("ACTIVITYID".into(), argument(b"ACTIVITY-ID-OUT")),
                ("APPLID".into(), argument(b"APP-OUT")),
                ("PROCESS".into(), argument(b"PROCESS-OUT")),
                ("PROCESSTYPE".into(), argument(b"PROCESS-TYPE-OUT")),
            ]),
            321,
        );
        no_bts.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_bts = service
            .invoke(
                &effect(&invocation.run_unit_id, no_bts.clone(), 321),
                no_bts,
            )
            .unwrap();
        assert_eq!(
            (no_bts.condition.as_str(), no_bts.response, no_bts.response2),
            ("INVREQ", 16, 6)
        );
        assert_eq!(no_bts.outputs["APPLID"].bytes(), b"MEAPPL");
        for name in ["ACTIVITY", "ACTIVITYID", "PROCESS", "PROCESSTYPE"] {
            assert!(!no_bts.outputs.contains_key(name));
        }

        let mut no_bdi = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("DESTID".into(), argument(b"DESTINATION-OUT")),
                ("DESTIDLENG".into(), argument(b"DESTINATION-LENGTH-OUT")),
            ]),
            322,
        );
        no_bdi.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_bdi = service
            .invoke(
                &effect(&invocation.run_unit_id, no_bdi.clone(), 322),
                no_bdi,
            )
            .unwrap();
        assert_eq!(
            (no_bdi.condition.as_str(), no_bdi.response, no_bdi.response2),
            ("INVREQ", 16, 3)
        );
        assert_eq!(no_bdi.outputs["APPLID"].bytes(), b"MEAPPL");
        assert!(!no_bdi.outputs.contains_key("DESTID"));
        assert!(!no_bdi.outputs.contains_key("DESTIDLENG"));

        let mut no_intersystem_facility = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("PRINSYSID".into(), argument(b"PRINCIPAL-SYSTEM-OUT")),
            ]),
            323,
        );
        no_intersystem_facility.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_intersystem_facility = service
            .invoke(
                &effect(
                    &invocation.run_unit_id,
                    no_intersystem_facility.clone(),
                    323,
                ),
                no_intersystem_facility,
            )
            .unwrap();
        assert_eq!(
            (
                no_intersystem_facility.condition.as_str(),
                no_intersystem_facility.response,
                no_intersystem_facility.response2,
            ),
            ("INVREQ", 16, 5)
        );
        assert_eq!(no_intersystem_facility.outputs["APPLID"].bytes(), b"MEAPPL");
        assert!(!no_intersystem_facility.outputs.contains_key("PRINSYSID"));

        let diagnostics = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("ABCODE".into(), argument(b"ABCODE-OUT")),
                ("ABDUMP".into(), argument(b"ABDUMP-OUT")),
                ("ABOFFSET".into(), argument(b"ABOFFSET-OUT")),
                ("ABPROGRAM".into(), argument(b"ABPROGRAM-OUT")),
                ("ASRAINTRPT".into(), argument(b"ASRA-INTRPT-OUT")),
                ("ASRAPSW".into(), argument(b"ASRA-PSW-OUT")),
                ("ASRAPSW16".into(), argument(b"ASRA-PSW16-OUT")),
                ("ASRAREGS".into(), argument(b"ASRA-REGS-OUT")),
                ("ASRAREGS64".into(), argument(b"ASRA-REGS64-OUT")),
                ("ERRORMSG".into(), argument(b"ERROR-MSG-OUT")),
                ("ERRORMSGLEN".into(), argument(b"ERROR-MSG-LENGTH-OUT")),
                ("LINKLEVEL".into(), argument(b"LINK-LEVEL-OUT")),
                ("ORGABCODE".into(), argument(b"ORIGINAL-ABCODE-OUT")),
            ]),
            27,
        );
        let diagnostics = service
            .invoke(
                &effect(&invocation.run_unit_id, diagnostics.clone(), 27),
                diagnostics,
            )
            .unwrap();
        assert_eq!(diagnostics.outputs["ABCODE"].bytes(), b"    ");
        assert_eq!(diagnostics.outputs["ABDUMP"].bytes(), &[0]);
        assert_eq!(diagnostics.outputs["ABOFFSET"].bytes(), b"0");
        assert_eq!(diagnostics.outputs["ABPROGRAM"].bytes(), &[0; 8]);
        assert_eq!(diagnostics.outputs["ERRORMSG"].bytes(), &[0; 500]);
        assert_eq!(diagnostics.outputs["ERRORMSGLEN"].bytes(), b"0");
        assert_eq!(diagnostics.outputs["LINKLEVEL"].bytes(), b"1");
        assert_eq!(diagnostics.outputs["ORGABCODE"].bytes(), b"    ");
        for (name, length) in [
            ("ASRAINTRPT", 8),
            ("ASRAPSW", 8),
            ("ASRAPSW16", 16),
            ("ASRAREGS", 64),
            ("ASRAREGS64", 128),
        ] {
            assert_eq!(diagnostics.outputs[name].bytes(), vec![0; length]);
        }

        let mut no_terminal = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("ALTSCRNHT".into(), argument(b"ALTERNATE-HEIGHT-OUT")),
                ("ALTSCRNWD".into(), argument(b"ALTERNATE-WIDTH-OUT")),
                ("DEFSCRNHT".into(), argument(b"DEFAULT-HEIGHT-OUT")),
                ("DEFSCRNWD".into(), argument(b"DEFAULT-WIDTH-OUT")),
                ("DS3270".into(), argument(b"DS3270-OUT")),
                ("DSSCS".into(), argument(b"DSSCS-OUT")),
                ("FCI".into(), argument(b"FCI-OUT")),
                ("PARTNSET".into(), argument(b"PARTITION-SET-OUT")),
                ("SCRNHT".into(), argument(b"SCREEN-HEIGHT-OUT")),
                ("SCRNWD".into(), argument(b"SCREEN-WIDTH-OUT")),
                ("UNATTEND".into(), argument(b"UNATTEND-OUT")),
            ]),
            26,
        );
        no_terminal.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_terminal = service
            .invoke(
                &effect(&invocation.run_unit_id, no_terminal.clone(), 26),
                no_terminal,
            )
            .unwrap();
        assert_eq!(
            (
                no_terminal.condition.as_str(),
                no_terminal.response,
                no_terminal.response2
            ),
            ("INVREQ", 16, 5)
        );
        assert_eq!(no_terminal.outputs["APPLID"].bytes(), b"MEAPPL");
        assert_eq!(no_terminal.outputs["FCI"].bytes(), &[0]);
        for name in [
            "ALTSCRNHT",
            "ALTSCRNWD",
            "DEFSCRNHT",
            "DEFSCRNWD",
            "DS3270",
            "DSSCS",
            "PARTNSET",
            "SCRNHT",
            "SCRNWD",
            "UNATTEND",
        ] {
            assert!(!no_terminal.outputs.contains_key(name));
        }
        for (offset, name) in [
            "APLKYBD",
            "APLTEXT",
            "BTRANS",
            "COLOR",
            "EWASUPP",
            "EXTDS",
            "GMMI",
            "HILIGHT",
            "KATAKANA",
            "MSRCONTROL",
            "OUTLINE",
            "PARTNS",
            "PS",
            "SOSI",
            "TEXTKYBD",
            "TEXTPRINT",
            "UNATTEND",
            "VALIDATION",
        ]
        .into_iter()
        .enumerate()
        {
            let sequence = 100 + u64::try_from(offset).unwrap();
            let mut capability = request(
                CicsOperation::Assign,
                BTreeMap::from([
                    ("APPLID".into(), argument(b"APP-OUT")),
                    (name.into(), argument(b"CAPABILITY-OUT")),
                ]),
                sequence,
            );
            capability.condition_policy = CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            };
            let capability = service
                .invoke(
                    &effect(&invocation.run_unit_id, capability.clone(), sequence),
                    capability,
                )
                .unwrap();
            assert_eq!(
                (
                    capability.condition.as_str(),
                    capability.response,
                    capability.response2
                ),
                ("INVREQ", 16, 5),
                "{name}"
            );
            assert_eq!(capability.outputs["APPLID"].bytes(), b"MEAPPL");
            assert!(!capability.outputs.contains_key(name), "{name}");
        }

        let terminal_service = CicsService::open(
            authorities(),
            Arc::new(MemoryStore::new(Default::default())),
            CicsLimits::default(),
        )
        .unwrap();
        let terminal_invocation = invocation_for("assign-capabilities", BTreeMap::new());
        let terminal_session = SessionId::new("assign-capabilities", 64).unwrap();
        terminal_service
            .launch_terminal(
                terminal_invocation.clone(),
                &terminal_session,
                "MENU",
                24,
                80,
                "assign-capabilities-csrf",
                1,
                10_000,
            )
            .unwrap();
        let capability_names = [
            "APLKYBD",
            "APLTEXT",
            "BTRANS",
            "COLOR",
            "EWASUPP",
            "EXTDS",
            "GMMI",
            "HILIGHT",
            "KATAKANA",
            "MSRCONTROL",
            "OUTLINE",
            "PARTNS",
            "PS",
            "SOSI",
            "TEXTKYBD",
            "TEXTPRINT",
            "UNATTEND",
            "VALIDATION",
        ];
        for (offset, names) in capability_names.chunks(15).enumerate() {
            let sequence = 200 + u64::try_from(offset).unwrap();
            let capability = request(
                CicsOperation::Assign,
                names
                    .iter()
                    .map(|name| ((*name).into(), argument(b"CAPABILITY-OUT")))
                    .collect(),
                sequence,
            );
            let capability = terminal_service
                .invoke(
                    &effect(
                        &terminal_invocation.run_unit_id,
                        capability.clone(),
                        sequence,
                    ),
                    capability,
                )
                .unwrap();
            assert_eq!(capability.condition, "NORMAL");
            for name in names {
                assert_eq!(capability.outputs[*name].bytes(), &[0], "{name}");
            }
        }

        let mut no_positioned_map = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("MAPCOLUMN".into(), argument(b"MAP-COLUMN-OUT")),
                ("MAPHEIGHT".into(), argument(b"MAP-HEIGHT-OUT")),
                ("MAPLINE".into(), argument(b"MAP-LINE-OUT")),
                ("MAPWIDTH".into(), argument(b"MAP-WIDTH-OUT")),
            ]),
            205,
        );
        no_positioned_map.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_positioned_map = terminal_service
            .invoke(
                &effect(
                    &terminal_invocation.run_unit_id,
                    no_positioned_map.clone(),
                    205,
                ),
                no_positioned_map,
            )
            .unwrap();
        assert_eq!(
            (
                no_positioned_map.condition.as_str(),
                no_positioned_map.response,
                no_positioned_map.response2,
            ),
            ("INVREQ", 16, 2)
        );
        assert_eq!(no_positioned_map.outputs["APPLID"].bytes(), b"ME01");
        assert!(!no_positioned_map.outputs.contains_key("MAPCOLUMN"));
        assert!(!no_positioned_map.outputs.contains_key("MAPHEIGHT"));
        assert!(!no_positioned_map.outputs.contains_key("MAPLINE"));
        assert!(!no_positioned_map.outputs.contains_key("MAPWIDTH"));

        let missing_program = request(
            CicsOperation::Assign,
            BTreeMap::from([("PROGRAM".into(), argument(b"PROGRAM-OUT"))]),
            30,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, missing_program.clone(), 30),
                missing_program,
            ),
            Err(HostProblem::InfrastructureFailure)
        );

        let mut nested_invocation = invocation_for("assign-nested", BTreeMap::new());
        nested_invocation.parent_execution_id =
            Some(ExecutionId::new("parent-execution", InvocationLimits::default()).unwrap());
        let nested_session = SessionId::new("assign-nested", 64).unwrap();
        service.create_session(&nested_session, 24, 80).unwrap();
        service
            .register_run(
                nested_invocation.clone(),
                &nested_session,
                "MENU",
                "ME01",
                "S001",
            )
            .unwrap();
        let nested_level = request(
            CicsOperation::Assign,
            BTreeMap::from([("LINKLEVEL".into(), argument(b"LINK-LEVEL-OUT"))]),
            1,
        );
        assert_eq!(
            service.invoke(
                &effect(&nested_invocation.run_unit_id, nested_level.clone(), 1),
                nested_level,
            ),
            Err(HostProblem::InfrastructureFailure)
        );

        for (sequence, name, value) in [
            (31, "ASRAKEY", argument(b"ASRA-KEY-OUT")),
            (32, "USERID", cics_decimal(1)),
            (33, "OPTION.NOHANDLE", argument(b"")),
        ] {
            let malformed = request(
                CicsOperation::Assign,
                BTreeMap::from([(name.into(), value)]),
                sequence,
            );
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, malformed.clone(), sequence),
                    malformed,
                ),
                Err(HostProblem::Malformed)
            );
        }

        let too_many = request(
            CicsOperation::Assign,
            BTreeMap::from(
                [
                    "APPLICATION",
                    "APPLID",
                    "CHANNEL",
                    "CWALENG",
                    "MAJORVERSION",
                    "MICROVERSION",
                    "MINORVERSION",
                    "OPERATION",
                    "OPERKEYS",
                    "PLATFORM",
                    "RESP",
                    "RESP2",
                    "RESTART",
                    "SYSID",
                    "TASKPRIORITY",
                    "TWALENG",
                    "USERID",
                ]
                .map(|name| (name.into(), argument(b"OUT"))),
            ),
            33,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, too_many.clone(), 33),
                too_many,
            ),
            Err(HostProblem::Malformed)
        );

        let inquire = request(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"COCRDLIC"))]),
            4,
        );
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, inquire.clone(), 4),
                    inquire,
                )
                .unwrap()
                .condition,
            "NORMAL"
        );
        let link = request(
            CicsOperation::Link,
            BTreeMap::from([
                ("PROGRAM".into(), argument(b"COPAUS2C")),
                ("COMMAREA".into(), argument(b"PENDING")),
            ]),
            5,
        );
        let linked = service
            .invoke(&effect(&invocation.run_unit_id, link.clone(), 5), link)
            .unwrap();
        assert_eq!(linked.disposition, CicsDisposition::Complete);
        assert_eq!(linked.outputs["COMMAREA"].bytes(), b"CHILD");

        let retrieve_invocation = invocation_for(
            "retrieve-run",
            BTreeMap::from([("cics.retrieve".into(), argument(b"MQ-TRIGGER"))]),
        );
        let retrieve_session = SessionId::new("retrieve-session", 64).unwrap();
        service.create_session(&retrieve_session, 24, 80).unwrap();
        service
            .register_run(
                retrieve_invocation.clone(),
                &retrieve_session,
                "MENU",
                "MEAPPL",
                "MESYS",
            )
            .unwrap();
        let retrieve = request(CicsOperation::Retrieve, BTreeMap::new(), 1);
        assert_eq!(
            service
                .invoke(
                    &effect(&retrieve_invocation.run_unit_id, retrieve.clone(), 1),
                    retrieve,
                )
                .unwrap()
                .payload
                .bytes(),
            b"MQ-TRIGGER"
        );
    }

    #[test]
    fn existing_run_tracks_only_authenticated_program_frames() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        assert_eq!(
            service
                .lock()
                .unwrap()
                .runs
                .get(&invocation.run_unit_id)
                .unwrap()
                .current_program,
            None
        );

        let mut frame = invocation.clone();
        frame.selector = Selector::new("program:current", InvocationLimits::default()).unwrap();
        service.ensure_run(&frame).unwrap();
        assert_eq!(
            service
                .lock()
                .unwrap()
                .runs
                .get(&invocation.run_unit_id)
                .unwrap()
                .current_program
                .as_deref(),
            Some("CURRENT")
        );

        let mut foreign = frame;
        foreign.principal = Principal::new(
            PrincipalId::new("OTHER", InvocationLimits::default()).unwrap(),
            BTreeSet::new(),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(service.ensure_run(&foreign), Err(HostProblem::Unauthorized));
        assert_eq!(
            service
                .lock()
                .unwrap()
                .runs
                .get(&invocation.run_unit_id)
                .unwrap()
                .current_program
                .as_deref(),
            Some("CURRENT")
        );
    }

    #[test]
    fn assign_context_subset_is_available_in_dpl_context() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let context = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-without-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let mut invocation = invocation_for(
            "assign-dpl",
            BTreeMap::from([("cics.execution-context".into(), context)]),
        );
        invocation.selector = Selector::new("program:DPLPGM", InvocationLimits::default()).unwrap();
        let session = SessionId::new("assign-dpl", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "ME01", "S001")
            .unwrap();
        let assign = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLICATION".into(), argument(b"APPLICATION-OUT")),
                ("APPLID".into(), argument(b"APP-OUT")),
                ("CHANNEL".into(), argument(b"CHANNEL-OUT")),
                ("CWALENG".into(), argument(b"CWA-LENGTH-OUT")),
                ("MAJORVERSION".into(), argument(b"MAJOR-OUT")),
                ("MICROVERSION".into(), argument(b"MICRO-OUT")),
                ("MINORVERSION".into(), argument(b"MINOR-OUT")),
                ("OPERATION".into(), argument(b"OPERATION-OUT")),
                ("OPERKEYS".into(), argument(b"OPERKEYS-OUT")),
                ("PLATFORM".into(), argument(b"PLATFORM-OUT")),
                ("PROGRAM".into(), argument(b"PROGRAM-OUT")),
                ("RESTART".into(), argument(b"RESTART-OUT")),
                ("SYSID".into(), argument(b"SYS-OUT")),
                ("TASKPRIORITY".into(), argument(b"PRIORITY-OUT")),
                ("TWALENG".into(), argument(b"TWA-LENGTH-OUT")),
                ("USERID".into(), argument(b"USER-OUT")),
            ]),
            1,
        );
        let response = service
            .invoke(&effect(&invocation.run_unit_id, assign.clone(), 1), assign)
            .unwrap();
        assert_eq!(response.condition, "NORMAL");
        assert_eq!(response.outputs["APPLICATION"].bytes(), &[b' '; 64]);
        assert_eq!(response.outputs["APPLID"].bytes(), b"ME01");
        assert_eq!(response.outputs["CHANNEL"].bytes(), &[b' '; 16]);
        assert_eq!(response.outputs["CWALENG"].bytes(), b"0");
        for name in ["MAJORVERSION", "MICROVERSION", "MINORVERSION"] {
            assert_eq!(response.outputs[name].bytes(), b"-1");
        }
        assert_eq!(response.outputs["OPERATION"].bytes(), &[b' '; 64]);
        assert_eq!(response.outputs["OPERKEYS"].bytes(), &[0; 8]);
        assert_eq!(response.outputs["PLATFORM"].bytes(), &[b' '; 64]);
        assert_eq!(response.outputs["PROGRAM"].bytes(), b"DPLPGM");
        assert_eq!(response.outputs["RESTART"].bytes(), &[0]);
        assert_eq!(response.outputs["SYSID"].bytes(), b"S001");
        assert_eq!(response.outputs["TASKPRIORITY"].bytes(), b"0");
        assert_eq!(response.outputs["TWALENG"].bytes(), b"0");
        assert_eq!(response.outputs["USERID"].bytes(), b"IBMUSER");

        let local_ccsid = request(
            CicsOperation::Assign,
            BTreeMap::from([("LOCALCCSID".into(), argument(b"LOCAL-CCSID-OUT"))]),
            2,
        );
        let local_ccsid = service
            .invoke(
                &effect(&invocation.run_unit_id, local_ccsid.clone(), 2),
                local_ccsid,
            )
            .unwrap();
        assert_eq!(local_ccsid.condition, "NORMAL");
        assert_eq!(local_ccsid.outputs["LOCALCCSID"].bytes(), b"37");

        let initparm = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("BRIDGE".into(), argument(b"BRIDGE-OUT")),
                ("CMDSEC".into(), argument(b"CMDSEC-OUT")),
                ("INITPARM".into(), argument(b"INITPARM-OUT")),
                ("INITPARMLEN".into(), argument(b"INITPARM-LENGTH-OUT")),
                ("RESSEC".into(), argument(b"RESSEC-OUT")),
            ]),
            4,
        );
        let initparm = service
            .invoke(
                &effect(&invocation.run_unit_id, initparm.clone(), 4),
                initparm,
            )
            .unwrap();
        assert_eq!(initparm.condition, "NORMAL");
        assert!(!initparm.outputs.contains_key("INITPARM"));
        assert_eq!(initparm.outputs["INITPARMLEN"].bytes(), b"0");
        assert_eq!(initparm.outputs["BRIDGE"].bytes(), &[b' '; 4]);
        assert_eq!(initparm.outputs["CMDSEC"].bytes(), b"X");
        assert_eq!(initparm.outputs["RESSEC"].bytes(), b"X");

        let mut no_bts = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("ACTIVITY".into(), argument(b"ACTIVITY-OUT")),
                ("ACTIVITYID".into(), argument(b"ACTIVITY-ID-OUT")),
                ("APPLID".into(), argument(b"APP-OUT")),
                ("PROCESS".into(), argument(b"PROCESS-OUT")),
                ("PROCESSTYPE".into(), argument(b"PROCESS-TYPE-OUT")),
            ]),
            60,
        );
        no_bts.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_bts = service
            .invoke(&effect(&invocation.run_unit_id, no_bts.clone(), 60), no_bts)
            .unwrap();
        assert_eq!(
            (no_bts.condition.as_str(), no_bts.response, no_bts.response2),
            ("INVREQ", 16, 6)
        );
        assert_eq!(no_bts.outputs["APPLID"].bytes(), b"ME01");
        for name in ["ACTIVITY", "ACTIVITYID", "PROCESS", "PROCESSTYPE"] {
            assert!(!no_bts.outputs.contains_key(name));
        }

        let mut no_bdi = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("DESTID".into(), argument(b"DESTINATION-OUT")),
                ("DESTIDLENG".into(), argument(b"DESTINATION-LENGTH-OUT")),
            ]),
            61,
        );
        no_bdi.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_bdi = service
            .invoke(&effect(&invocation.run_unit_id, no_bdi.clone(), 61), no_bdi)
            .unwrap();
        assert_eq!(
            (no_bdi.condition.as_str(), no_bdi.response, no_bdi.response2),
            ("INVREQ", 16, 200)
        );
        assert_eq!(no_bdi.outputs["APPLID"].bytes(), b"ME01");
        assert!(!no_bdi.outputs.contains_key("DESTID"));
        assert!(!no_bdi.outputs.contains_key("DESTIDLENG"));

        let mut no_intersystem_facility = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("PRINSYSID".into(), argument(b"PRINCIPAL-SYSTEM-OUT")),
            ]),
            62,
        );
        no_intersystem_facility.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let no_intersystem_facility = service
            .invoke(
                &effect(&invocation.run_unit_id, no_intersystem_facility.clone(), 62),
                no_intersystem_facility,
            )
            .unwrap();
        assert_eq!(
            (
                no_intersystem_facility.condition.as_str(),
                no_intersystem_facility.response,
                no_intersystem_facility.response2,
            ),
            ("INVREQ", 16, 5)
        );
        assert_eq!(no_intersystem_facility.outputs["APPLID"].bytes(), b"ME01");
        assert!(!no_intersystem_facility.outputs.contains_key("PRINSYSID"));

        let mut prohibited_map_dimensions = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("MAPCOLUMN".into(), argument(b"MAP-COLUMN-OUT")),
                ("MAPHEIGHT".into(), argument(b"MAP-HEIGHT-OUT")),
                ("MAPLINE".into(), argument(b"MAP-LINE-OUT")),
                ("MAPWIDTH".into(), argument(b"MAP-WIDTH-OUT")),
            ]),
            63,
        );
        prohibited_map_dimensions.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let prohibited_map_dimensions = service
            .invoke(
                &effect(
                    &invocation.run_unit_id,
                    prohibited_map_dimensions.clone(),
                    63,
                ),
                prohibited_map_dimensions,
            )
            .unwrap();
        assert_eq!(
            (
                prohibited_map_dimensions.condition.as_str(),
                prohibited_map_dimensions.response,
                prohibited_map_dimensions.response2,
            ),
            ("INVREQ", 16, 200)
        );
        assert_eq!(prohibited_map_dimensions.outputs["APPLID"].bytes(), b"ME01");
        assert!(!prohibited_map_dimensions.outputs.contains_key("MAPCOLUMN"));
        assert!(!prohibited_map_dimensions.outputs.contains_key("MAPHEIGHT"));
        assert!(!prohibited_map_dimensions.outputs.contains_key("MAPLINE"));
        assert!(!prohibited_map_dimensions.outputs.contains_key("MAPWIDTH"));

        let diagnostics = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("ABCODE".into(), argument(b"ABCODE-OUT")),
                ("ABDUMP".into(), argument(b"ABDUMP-OUT")),
                ("ABOFFSET".into(), argument(b"ABOFFSET-OUT")),
                ("ABPROGRAM".into(), argument(b"ABPROGRAM-OUT")),
                ("ASRAINTRPT".into(), argument(b"ASRA-INTRPT-OUT")),
                ("ASRAPSW".into(), argument(b"ASRA-PSW-OUT")),
                ("ASRAPSW16".into(), argument(b"ASRA-PSW16-OUT")),
                ("ASRAREGS".into(), argument(b"ASRA-REGS-OUT")),
                ("ASRAREGS64".into(), argument(b"ASRA-REGS64-OUT")),
                ("ERRORMSG".into(), argument(b"ERROR-MSG-OUT")),
                ("ERRORMSGLEN".into(), argument(b"ERROR-MSG-LENGTH-OUT")),
                ("LINKLEVEL".into(), argument(b"LINK-LEVEL-OUT")),
                ("ORGABCODE".into(), argument(b"ORIGINAL-ABCODE-OUT")),
            ]),
            5,
        );
        let diagnostics = service
            .invoke(
                &effect(&invocation.run_unit_id, diagnostics.clone(), 5),
                diagnostics,
            )
            .unwrap();
        assert_eq!(diagnostics.condition, "NORMAL");
        assert_eq!(diagnostics.outputs["ABCODE"].bytes(), b"    ");
        assert_eq!(diagnostics.outputs["ABDUMP"].bytes(), &[0]);
        assert_eq!(diagnostics.outputs["ABOFFSET"].bytes(), b"0");
        assert_eq!(diagnostics.outputs["ABPROGRAM"].bytes(), &[0; 8]);
        assert_eq!(diagnostics.outputs["ERRORMSG"].bytes(), &[0; 500]);
        assert_eq!(diagnostics.outputs["ERRORMSGLEN"].bytes(), b"0");
        assert_eq!(diagnostics.outputs["LINKLEVEL"].bytes(), b"2");
        assert_eq!(diagnostics.outputs["ORGABCODE"].bytes(), b"    ");
        for (name, length) in [
            ("ASRAINTRPT", 8),
            ("ASRAPSW", 8),
            ("ASRAPSW16", 16),
            ("ASRAREGS", 64),
            ("ASRAREGS64", 128),
        ] {
            assert_eq!(diagnostics.outputs[name].bytes(), vec![0; length]);
        }

        let mut prohibited = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("ALTSCRNHT".into(), argument(b"ALTERNATE-HEIGHT-OUT")),
                ("ALTSCRNWD".into(), argument(b"ALTERNATE-WIDTH-OUT")),
                ("DEFSCRNHT".into(), argument(b"DEFAULT-HEIGHT-OUT")),
                ("DEFSCRNWD".into(), argument(b"DEFAULT-WIDTH-OUT")),
                ("DS3270".into(), argument(b"DS3270-OUT")),
                ("DSSCS".into(), argument(b"DSSCS-OUT")),
                ("FCI".into(), argument(b"FCI-OUT")),
                ("NEXTTRANSID".into(), argument(b"NEXT-TRANS-OUT")),
                ("OPSECURITY".into(), argument(b"OPSECURITY-OUT")),
                ("PARTNSET".into(), argument(b"PARTITION-SET-OUT")),
                ("QNAME".into(), argument(b"QNAME-OUT")),
                ("SCRNHT".into(), argument(b"SCREEN-HEIGHT-OUT")),
                ("SCRNWD".into(), argument(b"SCREEN-WIDTH-OUT")),
                ("TCTUALENG".into(), argument(b"TCTUA-LENGTH-OUT")),
                ("UNATTEND".into(), argument(b"UNATTEND-OUT")),
            ]),
            2,
        );
        prohibited.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let partial = service
            .invoke(
                &effect(&invocation.run_unit_id, prohibited.clone(), 2),
                prohibited,
            )
            .unwrap();
        assert_eq!(
            (
                partial.disposition,
                partial.condition.as_str(),
                partial.response,
                partial.response2,
            ),
            (CicsDisposition::Complete, "INVREQ", 16, 200)
        );
        assert_eq!(partial.outputs["APPLID"].bytes(), b"ME01");
        for name in [
            "ALTSCRNHT",
            "ALTSCRNWD",
            "DEFSCRNHT",
            "DEFSCRNWD",
            "DS3270",
            "DSSCS",
            "FCI",
            "NEXTTRANSID",
            "OPSECURITY",
            "PARTNSET",
            "QNAME",
            "SCRNHT",
            "SCRNWD",
            "TCTUALENG",
            "UNATTEND",
        ] {
            assert!(!partial.outputs.contains_key(name));
        }
        for (offset, name) in [
            "APLKYBD",
            "APLTEXT",
            "BTRANS",
            "COLOR",
            "EWASUPP",
            "EXTDS",
            "GMMI",
            "HILIGHT",
            "KATAKANA",
            "MSRCONTROL",
            "OUTLINE",
            "PARTNS",
            "PS",
            "SOSI",
            "TEXTKYBD",
            "TEXTPRINT",
            "UNATTEND",
            "VALIDATION",
        ]
        .into_iter()
        .enumerate()
        {
            let sequence = 100 + u64::try_from(offset).unwrap();
            let mut capability = request(
                CicsOperation::Assign,
                BTreeMap::from([
                    ("APPLID".into(), argument(b"APP-OUT")),
                    (name.into(), argument(b"CAPABILITY-OUT")),
                ]),
                sequence,
            );
            capability.condition_policy = CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            };
            let capability = service
                .invoke(
                    &effect(&invocation.run_unit_id, capability.clone(), sequence),
                    capability,
                )
                .unwrap();
            assert_eq!(
                (
                    capability.condition.as_str(),
                    capability.response,
                    capability.response2
                ),
                ("INVREQ", 16, 200),
                "{name}"
            );
            assert_eq!(capability.outputs["APPLID"].bytes(), b"ME01");
            assert!(!capability.outputs.contains_key(name), "{name}");
        }

        let prohibited = request(
            CicsOperation::Assign,
            BTreeMap::from([("OPSECURITY".into(), argument(b"OPSECURITY-OUT"))]),
            3,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, prohibited.clone(), 3),
                prohibited,
            ),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 200,
            })
        );
    }

    /// Issue #206: WRITEQ TD persists exactly the requested FROM-area prefix.
    #[test]
    fn transient_data_length_selects_prefix_and_rejects_excess() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let write = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"OUTQ")),
                ("FROM".into(), argument(b"ABCDEF")),
                ("LENGTH".into(), argument(b"3")),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, write.clone(), 1), write)
            .unwrap();
        assert_eq!(
            service.transient_records("OUTQ").unwrap(),
            [b"ABC".to_vec()]
        );

        let excessive = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"OUTQ")),
                ("FROM".into(), argument(b"ABCDEF")),
                ("LENGTH".into(), argument(b"7")),
            ]),
            2,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, excessive.clone(), 2),
                excessive,
            ),
            Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 0,
            })
        );
        assert_eq!(
            service.transient_records("OUTQ").unwrap(),
            [b"ABC".to_vec()]
        );
    }

    /// Issue #207: bare separators use slash/colon and compact forms keep compact widths.
    #[test]
    fn formattime_bare_and_absent_separators_return_documented_widths() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let separated = request(
            CicsOperation::FormatTime,
            BTreeMap::from([
                ("ABSTIME".into(), argument(b"3997082096789")),
                ("MMDDYY".into(), argument(b"DATE-OUT")),
                ("TIME".into(), argument(b"TIME-OUT")),
                ("OPTION.DATESEP".into(), cics_option()),
                ("OPTION.TIMESEP".into(), cics_option()),
            ]),
            1,
        );
        let separated = service
            .invoke(
                &effect(&invocation.run_unit_id, separated.clone(), 1),
                separated,
            )
            .unwrap();
        assert_eq!(separated.outputs["MMDDYY"].bytes(), b"08/30/26");
        assert_eq!(separated.outputs["TIME"].bytes(), b"12:34:56");

        let compact = request(
            CicsOperation::FormatTime,
            BTreeMap::from([
                ("ABSTIME".into(), argument(b"3997082096789")),
                ("MMDDYY".into(), argument(b"DATE-OUT")),
                ("TIME".into(), argument(b"TIME-OUT")),
            ]),
            2,
        );
        let compact = service
            .invoke(
                &effect(&invocation.run_unit_id, compact.clone(), 2),
                compact,
            )
            .unwrap();
        assert_eq!(compact.outputs["MMDDYY"].bytes(), b"083026");
        assert_eq!(compact.outputs["TIME"].bytes(), b"123456");
    }

    #[test]
    fn pseudo_conversation_tdq_and_syncpoint_survive_restart_and_replay() {
        let memory = Arc::new(MemoryStore::new(Default::default()));
        let store: Arc<dyn ProviderStateStore> = memory.clone();
        let initial = service(store.clone());
        let (invocation, session) = registered(&initial);

        let returned = request(
            CicsOperation::Return,
            BTreeMap::from([
                ("TRANSID".into(), argument(b"NEXT")),
                ("COMMAREA".into(), argument(b"STATE-1")),
            ]),
            1,
        );
        let response = initial
            .invoke(
                &effect(&invocation.run_unit_id, returned.clone(), 1),
                returned,
            )
            .unwrap();
        assert_eq!(response.next_transaction.as_deref(), Some("NEXT"));

        let writeq = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"JOBS")),
                ("FROM".into(), argument(b"//REPORT JOB")),
                ("LENGTH".into(), argument(b"8")),
            ]),
            2,
        );
        initial
            .invoke(
                &effect(&invocation.run_unit_id, writeq.clone(), 2),
                writeq.clone(),
            )
            .unwrap();
        initial
            .invoke(&effect(&invocation.run_unit_id, writeq.clone(), 2), writeq)
            .unwrap();
        let second_invocation = invocation_for("aaa-run", BTreeMap::new());
        let second_session = SessionId::new("second-session", 64).unwrap();
        initial.create_session(&second_session, 24, 80).unwrap();
        initial
            .register_run(
                second_invocation.clone(),
                &second_session,
                "MENU",
                "MEAPPL",
                "MESYS",
            )
            .unwrap();
        let mut second_write = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"JOBS")),
                ("FROM".into(), argument(b"//SECOND JOB")),
            ]),
            1,
        );
        second_write.mutation.as_mut().unwrap().idempotency_key =
            IdempotencyKey::new("outer-second-1", InvocationLimits::default()).unwrap();
        initial
            .invoke(
                &effect(&second_invocation.run_unit_id, second_write.clone(), 1),
                second_write,
            )
            .unwrap();
        assert_eq!(
            initial.transient_records("JOBS").unwrap(),
            [b"//REPORT".to_vec(), b"//SECOND JOB".to_vec()]
        );

        let commit = request(CicsOperation::Syncpoint, BTreeMap::new(), 3);
        let committed = initial
            .invoke(
                &effect(&invocation.run_unit_id, commit.clone(), 3),
                commit.clone(),
            )
            .unwrap();
        assert_eq!(
            committed.unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );
        assert_eq!(
            initial
                .invoke(&effect(&invocation.run_unit_id, commit.clone(), 3), commit,)
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );
        let rollback = request(
            CicsOperation::Syncpoint,
            BTreeMap::from([("OPTION.ROLLBACK".into(), argument(b""))]),
            4,
        );
        assert_eq!(
            initial
                .invoke(
                    &effect(&invocation.run_unit_id, rollback.clone(), 4),
                    rollback,
                )
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::RolledBack)
        );

        let restarted = service(store.clone());
        assert_eq!(
            restarted.transient_records("JOBS").unwrap(),
            [b"//REPORT".to_vec(), b"//SECOND JOB".to_vec()]
        );
        let resumed_invocation = invocation_for("resumed-run", BTreeMap::new());
        let continuation = restarted
            .claim_continuation(resumed_invocation, &session, "MEAPPL", "MESYS")
            .unwrap();
        assert_eq!(continuation.transaction, "NEXT");
        assert_eq!(continuation.commarea, b"STATE-1");

        let unknown_key = IdempotencyKey::new("outer-5", InvocationLimits::default()).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: unknown_key.as_str().into(),
                    version: 1,
                    payload: encode_uow(&UowRecord {
                        finalized: false,
                        outcome: CicsUnitOfWorkOutcome::Committed,
                        transaction: "MENU".into(),
                        metadata: None,
                    })
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        let unknown_invocation = invocation_for("unknown-run", BTreeMap::new());
        let unknown_session = SessionId::new("unknown-session", 64).unwrap();
        restarted.create_session(&unknown_session, 24, 80).unwrap();
        restarted
            .register_run(
                unknown_invocation.clone(),
                &unknown_session,
                "MENU",
                "MEAPPL",
                "MESYS",
            )
            .unwrap();
        let unknown = request(CicsOperation::Syncpoint, BTreeMap::new(), 5);
        assert_eq!(
            restarted.invoke(
                &effect(&unknown_invocation.run_unit_id, unknown.clone(), 5),
                unknown.clone(),
            ),
            Err(HostProblem::UnknownOutcome)
        );
        restarted
            .reconcile_unit_of_work(&unknown_key, CicsUnitOfWorkOutcome::Committed)
            .unwrap();
        assert_eq!(
            restarted
                .invoke(
                    &effect(&unknown_invocation.run_unit_id, unknown.clone(), 5),
                    unknown,
                )
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );
    }

    /// Issue #202: RETURN truncates to LENGTH and reports out-of-range as 22/11.
    #[test]
    fn return_length_selects_commarea_prefix_and_reports_lengerr_22_11() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        let excessive = request(
            CicsOperation::Return,
            BTreeMap::from([
                ("TRANSID".into(), argument(b"NEXT")),
                ("COMMAREA".into(), argument(b"STATE")),
                ("LENGTH".into(), cics_decimal(6)),
            ]),
            1,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, excessive.clone(), 1),
                excessive,
            ),
            Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 11,
            })
        );
        assert!(
            !service
                .lock()
                .unwrap()
                .continuations
                .contains_key(session.as_str())
        );

        let bounded = request(
            CicsOperation::Return,
            BTreeMap::from([
                ("TRANSID".into(), argument(b"NEXT")),
                ("COMMAREA".into(), argument(b"STATE")),
                ("LENGTH".into(), cics_decimal(3)),
            ]),
            2,
        );
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, bounded.clone(), 2),
                bounded,
            )
            .unwrap();
        assert_eq!(response.payload.bytes(), b"STA");
        assert_eq!(
            service
                .lock()
                .unwrap()
                .continuations
                .get(session.as_str())
                .unwrap()
                .commarea,
            b"STA"
        );
    }

    #[test]
    fn return_request_is_bounded_typed_and_local_only() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        for (sequence, arguments) in [
            (
                51,
                BTreeMap::from([("COMMAREA".into(), argument(b"STATE"))]),
            ),
            (
                52,
                BTreeMap::from([("TRANSID".into(), argument(b"TOOLONG"))]),
            ),
            (53, BTreeMap::from([("UNKNOWN".into(), argument(b"VALUE"))])),
            (
                54,
                BTreeMap::from([
                    ("TRANSID".into(), argument(b"NEXT")),
                    ("RESP".into(), task_value(b"RESP-X")),
                ]),
            ),
        ] {
            let invalid = request(CicsOperation::Return, arguments, sequence);
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, invalid.clone(), sequence),
                    invalid,
                ),
                Err(HostProblem::Malformed)
            );
        }
        assert!(
            !service
                .lock()
                .unwrap()
                .continuations
                .contains_key(session.as_str())
        );

        let context = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let dpl = invocation_for(
            "return-dpl",
            BTreeMap::from([("cics.execution-context".into(), context)]),
        );
        let dpl_session = SessionId::new("return-dpl", 64).unwrap();
        service.create_session(&dpl_session, 24, 80).unwrap();
        service
            .register_run(dpl.clone(), &dpl_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let mut denied = request(
            CicsOperation::Return,
            BTreeMap::from([("TRANSID".into(), argument(b"NEXT"))]),
            55,
        );
        denied.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = service
            .invoke(&effect(&dpl.run_unit_id, denied.clone(), 55), denied)
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "INVREQ", 16, 200)
        );
        assert!(
            !service
                .lock()
                .unwrap()
                .continuations
                .contains_key(dpl_session.as_str())
        );
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn file_queue_program_and_syncpoint_replay_once_across_process_crash() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-online-replay-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let dataset_commits = Arc::new(AtomicUsize::new(0));
        let program_commits = Arc::new(AtomicUsize::new(0));
        let first_store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let first = CicsService::open(
            replay_authorities(dataset_commits.clone(), program_commits.clone()),
            first_store.clone(),
            CicsLimits::default(),
        )
        .unwrap();
        let (invocation, session) = registered(&first);
        first
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();

        let cases = [
            request(
                CicsOperation::Write,
                BTreeMap::from([
                    ("FILE".into(), argument(b"ACCTDAT")),
                    ("FROM".into(), argument(b"AA11")),
                    ("RIDFLD".into(), argument(b"AA")),
                ]),
                1,
            ),
            request(
                CicsOperation::WriteTransientData,
                BTreeMap::from([
                    ("QUEUE".into(), argument(b"RECOVERY")),
                    ("FROM".into(), argument(b"ONE")),
                ]),
                2,
            ),
            request(
                CicsOperation::Link,
                BTreeMap::from([
                    ("PROGRAM".into(), argument(b"CHILD")),
                    ("COMMAREA".into(), argument(b"INPUT")),
                ]),
                3,
            ),
            request(CicsOperation::Syncpoint, BTreeMap::new(), 4),
        ];
        for (offset, request) in cases.iter().enumerate() {
            first.inject_mutation_fault_once(request.operation).unwrap();
            assert_eq!(
                first.invoke(
                    &effect(
                        &invocation.run_unit_id,
                        request.clone(),
                        u64::try_from(offset + 1).unwrap(),
                    ),
                    request.clone(),
                ),
                Err(HostProblem::UnknownOutcome),
                "{:?} did not preserve the post-mutation crash gap",
                request.operation
            );
        }
        assert_eq!(
            first.transient_records("RECOVERY").unwrap(),
            [b"ONE".to_vec()]
        );
        assert_eq!(dataset_commits.load(Ordering::SeqCst), 1);
        assert_eq!(program_commits.load(Ordering::SeqCst), 1);
        drop((first, first_store));

        let second_store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let second = CicsService::open(
            replay_authorities(dataset_commits.clone(), program_commits.clone()),
            second_store.clone(),
            CicsLimits::default(),
        )
        .unwrap();
        second
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        for (offset, request) in cases.into_iter().enumerate() {
            let key = request.mutation.as_ref().unwrap().idempotency_key.clone();
            let request_digest =
                canonical_request_digest(&HostRequest::Cics(request.clone())).unwrap();
            let reconciled = second
                .reconciled_effect_result_digest(&key, request_digest)
                .unwrap();
            let response = second
                .invoke(
                    &effect(
                        &invocation.run_unit_id,
                        request.clone(),
                        u64::try_from(offset + 1).unwrap(),
                    ),
                    request,
                )
                .unwrap();
            assert_eq!(
                reconciled,
                canonical_result_digest(&Ok(HostResult::Cics(response))).unwrap()
            );
        }
        assert_eq!(
            second.transient_records("RECOVERY").unwrap(),
            [b"ONE".to_vec()]
        );
        assert_eq!(dataset_commits.load(Ordering::SeqCst), 1);
        assert_eq!(program_commits.load(Ordering::SeqCst), 1);
        let replay_rows = second_store
            .list_provider_state("cics-effect-replay-v1", 16)
            .unwrap();
        assert_eq!(replay_rows.len(), 4);
        for row in &replay_rows {
            let replay = decode_cics_effect_replay(&row.payload, CicsLimits::default()).unwrap();
            assert_eq!(
                replay.owner_execution.as_deref(),
                Some(invocation.execution_id.as_str())
            );
            assert_eq!(replay.deadline_tick, Some(100));
        }
        let current = &replay_rows[0].payload;
        let replay = decode_cics_effect_replay(current, CicsLimits::default()).unwrap();
        let mut response_at = 8;
        for _ in 0..3 {
            let length = usize::try_from(u32::from_be_bytes(
                current[response_at..response_at + 4].try_into().unwrap(),
            ))
            .unwrap();
            response_at += 4 + length;
        }
        response_at += 3 * 8 + 3 * 32;
        let mut legacy = b"MECER001".to_vec();
        legacy.extend_from_slice(&replay.request_digest);
        legacy.extend_from_slice(&current[response_at..]);
        let legacy = decode_cics_effect_replay(&legacy, CicsLimits::default()).unwrap();
        assert_eq!((legacy.owner_execution, legacy.deadline_tick), (None, None));
        let syncpoint_key = IdempotencyKey::new("outer-4", InvocationLimits::default()).unwrap();
        assert_eq!(
            second_store
                .get_provider_state("cics-uow", syncpoint_key.as_str())
                .unwrap()
                .unwrap()
                .version,
            2
        );
        drop((second, second_store));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn carddemo_condition_abend_and_nohandle_routes_are_distinct() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let handle = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([("PGMIDERR".into(), argument(b"PGMIDERR-ERR-PARA"))]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, handle.clone(), 1), handle)
            .unwrap();
        let missing = request(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            2,
        );
        let handled = service
            .invoke(
                &effect(&invocation.run_unit_id, missing.clone(), 2),
                missing,
            )
            .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.condition, "PGMIDERR");
        assert_eq!(handled.target.as_deref(), Some("PGMIDERR-ERR-PARA"));

        let mut nohandle = request(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            3,
        );
        nohandle.condition_policy = CicsConditionPolicy::NoHandle;
        let condition = service
            .invoke(
                &effect(&invocation.run_unit_id, nohandle.clone(), 3),
                nohandle,
            )
            .unwrap();
        assert_eq!(condition.disposition, CicsDisposition::Complete);
        assert_eq!(
            (condition.condition.as_str(), condition.response),
            ("PGMIDERR", 27)
        );

        let handle_abend = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("LABEL".into(), argument(b"ABEND-ROUTINE"))]),
            4,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, handle_abend.clone(), 4),
                handle_abend,
            )
            .unwrap();
        let abend = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"9999"))]),
            5,
        );
        let handled = service
            .invoke(&effect(&invocation.run_unit_id, abend.clone(), 5), abend)
            .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.target.as_deref(), Some("ABEND-ROUTINE"));

        let reset = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("OPTION.RESET".into(), argument(b""))]),
            6,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, reset.clone(), 6), reset)
            .unwrap();
        let abend = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"8888"))]),
            7,
        );
        let handled_again = service
            .invoke(&effect(&invocation.run_unit_id, abend.clone(), 7), abend)
            .unwrap();
        assert_eq!(handled_again.disposition, CicsDisposition::Handler);
        assert_eq!(handled_again.target.as_deref(), Some("ABEND-ROUTINE"));

        let assign_codes = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("ABCODE".into(), argument(b"CURRENT-ABCODE-OUT")),
                ("ORGABCODE".into(), argument(b"ORIGINAL-ABCODE-OUT")),
            ]),
            70,
        );
        let assigned_codes = service
            .invoke(
                &effect(&invocation.run_unit_id, assign_codes.clone(), 70),
                assign_codes,
            )
            .unwrap();
        assert_eq!(assigned_codes.outputs["ABCODE"].bytes(), b"8888");
        assert_eq!(assigned_codes.outputs["ORGABCODE"].bytes(), b"9999");

        let reset = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("OPTION.RESET".into(), argument(b""))]),
            8,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, reset.clone(), 8), reset)
            .unwrap();
        let default_cancel = request(CicsOperation::HandleAbend, BTreeMap::new(), 9);
        service
            .invoke(
                &effect(&invocation.run_unit_id, default_cancel.clone(), 9),
                default_cancel,
            )
            .unwrap();
        let abend = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"9999"))]),
            10,
        );
        let dumped = service
            .invoke(&effect(&invocation.run_unit_id, abend.clone(), 10), abend)
            .unwrap();
        assert_eq!(dumped.disposition, CicsDisposition::Abended);
        assert_eq!(
            dumped.outputs["ABEND.DUMP"].schema(),
            "mainframe-env.cics.abend-dump@1"
        );
        assert_eq!(dumped.outputs["ABEND.DUMP"].bytes(), b"requested");

        let handle_abend = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("LABEL".into(), argument(b"SECOND-ABEND-ROUTINE"))]),
            11,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, handle_abend.clone(), 11),
                handle_abend,
            )
            .unwrap();
        let abend_cancel = request(
            CicsOperation::Abend,
            BTreeMap::from([
                ("ABCODE".into(), argument(b"9999")),
                ("OPTION.CANCEL".into(), argument(b"")),
            ]),
            12,
        );
        let cancelled = service
            .invoke(
                &effect(&invocation.run_unit_id, abend_cancel.clone(), 12),
                abend_cancel,
            )
            .unwrap();
        assert_eq!(cancelled.disposition, CicsDisposition::Abended);
        assert_eq!(cancelled.target, None);
        let abend = request(CicsOperation::Abend, BTreeMap::new(), 13);
        let default_no_dump = service
            .invoke(&effect(&invocation.run_unit_id, abend.clone(), 13), abend)
            .unwrap();
        assert_eq!(default_no_dump.disposition, CicsDisposition::Abended);
        assert_eq!(default_no_dump.outputs["ABEND.DUMP"].bytes(), b"suppressed");

        let no_dump = request(
            CicsOperation::Abend,
            BTreeMap::from([
                ("ABCODE".into(), argument(b"9999")),
                ("OPTION.NODUMP".into(), argument(b"")),
            ]),
            14,
        );
        let no_dump = service
            .invoke(
                &effect(&invocation.run_unit_id, no_dump.clone(), 14),
                no_dump,
            )
            .unwrap();
        assert_eq!(no_dump.outputs["ABEND.DUMP"].bytes(), b"suppressed");

        let reserved_code = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"A001"))]),
            15,
        );
        let reserved_code = service
            .invoke(
                &effect(&invocation.run_unit_id, reserved_code.clone(), 15),
                reserved_code,
            )
            .unwrap();
        assert_eq!(reserved_code.outputs["ABEND.DUMP"].bytes(), b"suppressed");

        let malformed = request(
            CicsOperation::Abend,
            BTreeMap::from([("OPTION.NODUMP".into(), argument(b"unexpected"))]),
            16,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed.clone(), 16),
                malformed,
            ),
            Err(HostProblem::Malformed)
        );

        let malformed_code = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), cics_decimal(9999))]),
            17,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed_code.clone(), 17),
                malformed_code,
            ),
            Err(HostProblem::Malformed)
        );

        let conflicting_handler = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([
                ("OPTION.CANCEL".into(), argument(b"")),
                ("OPTION.RESET".into(), argument(b"")),
            ]),
            18,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, conflicting_handler.clone(), 18,),
                conflicting_handler,
            ),
            Err(HostProblem::Malformed)
        );

        let malformed_handler = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("PROGRAM".into(), cics_decimal(1))]),
            19,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed_handler.clone(), 19),
                malformed_handler,
            ),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn handle_abend_program_checks_and_transfers_the_issuing_commarea() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        service
            .register_programs(&BTreeSet::from(["ABEXIT".into()]))
            .unwrap();
        let invocation = invocation_for(
            "handle-abend-program",
            BTreeMap::from([("cics.retrieve".into(), argument(b"ISSUER-COMMAREA"))]),
        );
        let session = SessionId::new("handle-abend-program", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let invoke = |request: CicsRequest, sequence| {
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };
        invoke(
            request(
                CicsOperation::HandleAbend,
                BTreeMap::from([("PROGRAM".into(), argument(b"abexit "))]),
                1,
            ),
            1,
        )
        .unwrap();
        let transferred = invoke(
            request(
                CicsOperation::Abend,
                BTreeMap::from([("ABCODE".into(), argument(b"B001"))]),
                2,
            ),
            2,
        )
        .unwrap();
        assert_eq!(
            (
                transferred.disposition,
                transferred.target.as_deref(),
                transferred.payload.bytes(),
            ),
            (
                CicsDisposition::Transfer,
                Some("ABEXIT"),
                b"ISSUER-COMMAREA".as_slice(),
            )
        );
        invoke(
            request(
                CicsOperation::HandleAbend,
                BTreeMap::from([("OPTION.RESET".into(), argument(b""))]),
                3,
            ),
            3,
        )
        .unwrap();
        let unmatched_pop =
            invoke(request(CicsOperation::PopHandle, BTreeMap::new(), 4), 4).unwrap();
        assert_eq!(
            (
                unmatched_pop.disposition,
                unmatched_pop.condition.as_str(),
                unmatched_pop.target.as_deref(),
                unmatched_pop.payload.bytes(),
            ),
            (
                CicsDisposition::Transfer,
                "INVREQ",
                Some("ABEXIT"),
                b"ISSUER-COMMAREA".as_slice(),
            )
        );

        let missing = self::service(Arc::new(MemoryStore::new(Default::default())));
        let (missing_invocation, _) = registered(&missing);
        let mut missing_request = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            1,
        );
        missing_request.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = missing
            .invoke(
                &effect(&missing_invocation.run_unit_id, missing_request.clone(), 1),
                missing_request,
            )
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "PGMIDERR", 27, 1)
        );
        assert_eq!(
            handlers::HandleState::from_run(
                missing
                    .lock()
                    .unwrap()
                    .runs
                    .get(&missing_invocation.run_unit_id)
                    .unwrap(),
            ),
            handlers::HandleState::default()
        );

        let (host, _) = command_authorities(true);
        let denied = CicsService::open(
            host,
            Arc::new(MemoryStore::new(Default::default())),
            CicsLimits::default(),
        )
        .unwrap();
        denied
            .register_programs(&BTreeSet::from(["ABEXIT".into()]))
            .unwrap();
        let (denied_invocation, _) = registered(&denied);
        let mut denied_request = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("PROGRAM".into(), argument(b"ABEXIT"))]),
            1,
        );
        denied_request.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = denied
            .invoke(
                &effect(&denied_invocation.run_unit_id, denied_request.clone(), 1),
                denied_request,
            )
            .unwrap();
        assert_eq!(
            (
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            ("NOTAUTH", 70, 0)
        );
        assert_eq!(
            handlers::HandleState::from_run(
                denied
                    .lock()
                    .unwrap()
                    .runs
                    .get(&denied_invocation.run_unit_id)
                    .unwrap(),
            ),
            handlers::HandleState::default()
        );

        let malformed = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("PROGRAM".into(), argument(b"TOO-LONG9"))]),
            5,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed.clone(), 5),
                malformed,
            ),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn handle_stack_nests_restores_and_reports_unmatched_pop_exactly() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let invoke = |operation, arguments, sequence| {
            let request = request(operation, arguments, sequence);
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };
        invoke(
            CicsOperation::HandleCondition,
            BTreeMap::from([("PGMIDERR".into(), argument(b"OUTER-COND"))]),
            1,
        )
        .unwrap();
        invoke(
            CicsOperation::HandleAbend,
            BTreeMap::from([("LABEL".into(), argument(b"OUTER-ABEND"))]),
            2,
        )
        .unwrap();
        invoke(CicsOperation::PushHandle, BTreeMap::new(), 3).unwrap();
        invoke(
            CicsOperation::HandleCondition,
            BTreeMap::from([("PGMIDERR".into(), argument(b"INNER-COND"))]),
            4,
        )
        .unwrap();
        invoke(
            CicsOperation::HandleAbend,
            BTreeMap::from([("LABEL".into(), argument(b"INNER-ABEND"))]),
            5,
        )
        .unwrap();
        invoke(CicsOperation::PushHandle, BTreeMap::new(), 6).unwrap();
        invoke(CicsOperation::PopHandle, BTreeMap::new(), 7).unwrap();
        let inner = invoke(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            8,
        )
        .unwrap();
        assert_eq!(inner.disposition, CicsDisposition::Handler);
        assert_eq!(inner.target.as_deref(), Some("INNER-COND"));
        invoke(CicsOperation::PopHandle, BTreeMap::new(), 9).unwrap();
        let outer = invoke(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            10,
        )
        .unwrap();
        assert_eq!(outer.disposition, CicsDisposition::Handler);
        assert_eq!(outer.target.as_deref(), Some("OUTER-COND"));
        let outer_abend = invoke(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"B001"))]),
            11,
        )
        .unwrap();
        assert_eq!(outer_abend.disposition, CicsDisposition::Handler);
        assert_eq!(outer_abend.target.as_deref(), Some("OUTER-ABEND"));

        let unmatched = self::service(Arc::new(MemoryStore::new(Default::default())));
        let (unmatched_invocation, _) = registered(&unmatched);
        let pop = request(CicsOperation::PopHandle, BTreeMap::new(), 1);
        let terminal = unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, pop.clone(), 1),
                pop,
            )
            .unwrap();
        assert_eq!(terminal.disposition, CicsDisposition::Abended);
        assert_eq!(
            (
                terminal.condition.as_str(),
                terminal.response,
                terminal.response2
            ),
            ("INVREQ", 16, 0)
        );
        let mut nohandle = request(CicsOperation::PopHandle, BTreeMap::new(), 2);
        nohandle.condition_policy = CicsConditionPolicy::NoHandle;
        let returned = unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, nohandle.clone(), 2),
                nohandle,
            )
            .unwrap();
        assert_eq!(returned.disposition, CicsDisposition::Complete);
        assert_eq!((returned.response, returned.response2), (16, 0));

        let mut responded = request(CicsOperation::PopHandle, BTreeMap::new(), 3);
        responded.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let returned = unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, responded.clone(), 3),
                responded,
            )
            .unwrap();
        assert_eq!(returned.disposition, CicsDisposition::Complete);
        assert_eq!((returned.response, returned.response2), (16, 0));

        let abend_handler = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("LABEL".into(), argument(b"POP-ABEND"))]),
            4,
        );
        unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, abend_handler.clone(), 4),
                abend_handler,
            )
            .unwrap();
        let pop = request(CicsOperation::PopHandle, BTreeMap::new(), 5);
        let handled = unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, pop.clone(), 5),
                pop,
            )
            .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.target.as_deref(), Some("POP-ABEND"));

        let condition_handler = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([("INVREQ".into(), argument(b"POP-INVREQ"))]),
            6,
        );
        unmatched
            .invoke(
                &effect(
                    &unmatched_invocation.run_unit_id,
                    condition_handler.clone(),
                    6,
                ),
                condition_handler,
            )
            .unwrap();
        let pop = request(CicsOperation::PopHandle, BTreeMap::new(), 7);
        let handled = unmatched
            .invoke(
                &effect(&unmatched_invocation.run_unit_id, pop.clone(), 7),
                pop,
            )
            .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.target.as_deref(), Some("POP-INVREQ"));

        let malformed = request(
            CicsOperation::PushHandle,
            BTreeMap::from([("OPTION.UNKNOWN".into(), argument(b""))]),
            8,
        );
        assert_eq!(
            unmatched.invoke(
                &effect(&unmatched_invocation.run_unit_id, malformed.clone(), 8),
                malformed,
            ),
            Err(HostProblem::Malformed)
        );

        let bounded = self::service(Arc::new(MemoryStore::new(Default::default())));
        let (bounded_invocation, _) = registered(&bounded);
        for sequence in 1..=64 {
            let push = request(CicsOperation::PushHandle, BTreeMap::new(), sequence);
            bounded
                .invoke(
                    &effect(&bounded_invocation.run_unit_id, push.clone(), sequence),
                    push,
                )
                .unwrap();
        }
        let overflow = request(CicsOperation::PushHandle, BTreeMap::new(), 65);
        assert_eq!(
            bounded.invoke(
                &effect(&bounded_invocation.run_unit_id, overflow.clone(), 65),
                overflow,
            ),
            Err(HostProblem::ResourceExhausted)
        );
    }

    #[test]
    fn ignore_condition_continues_overrides_and_restores_with_the_handle_stack() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let invoke = |operation, arguments, sequence| {
            let request = request(operation, arguments, sequence);
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };
        invoke(
            CicsOperation::IgnoreCondition,
            BTreeMap::from([("CONDITIONS".into(), condition_list(&["PGMIDERR", "INVREQ"]))]),
            1,
        )
        .unwrap();
        let ignored = invoke(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            2,
        )
        .unwrap();
        assert_eq!(ignored.disposition, CicsDisposition::Ignored);
        assert_eq!(
            (
                ignored.condition.as_str(),
                ignored.response,
                ignored.response2
            ),
            ("PGMIDERR", 27, 0)
        );

        invoke(
            CicsOperation::HandleCondition,
            BTreeMap::from([("PGMIDERR".into(), argument(b"ERROR-HANDLER"))]),
            3,
        )
        .unwrap();
        let handled = invoke(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            4,
        )
        .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.target.as_deref(), Some("ERROR-HANDLER"));

        invoke(
            CicsOperation::IgnoreCondition,
            BTreeMap::from([("CONDITIONS".into(), condition_list(&["PGMIDERR"]))]),
            5,
        )
        .unwrap();
        invoke(CicsOperation::PushHandle, BTreeMap::new(), 6).unwrap();
        invoke(
            CicsOperation::HandleCondition,
            BTreeMap::from([("PGMIDERR".into(), argument(b"INNER-HANDLER"))]),
            7,
        )
        .unwrap();
        invoke(CicsOperation::PopHandle, BTreeMap::new(), 8).unwrap();
        let restored = invoke(
            CicsOperation::Inquire,
            BTreeMap::from([("PROGRAM".into(), argument(b"MISSING"))]),
            9,
        )
        .unwrap();
        assert_eq!(restored.disposition, CicsDisposition::Ignored);

        let seventeen = crate::generated::CICS_CONDITION_NAMES[..17].join("\n");
        for (sequence, schema, value) in [
            (10, "mainframe-env.cics.condition-list@1", ""),
            (
                11,
                "mainframe-env.cics.condition-list@1",
                "PGMIDERR\nPGMIDERR",
            ),
            (12, "mainframe-env.cics.condition-list@1", "MADEUP"),
            (
                13,
                "mainframe-env.cics.condition-list@1",
                seventeen.as_str(),
            ),
            (14, "mainframe-env.cics.literal@1", "PGMIDERR"),
        ] {
            let malformed = request(
                CicsOperation::IgnoreCondition,
                BTreeMap::from([(
                    "CONDITIONS".into(),
                    BoundedPayload::new(
                        schema,
                        value.as_bytes().to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                )]),
                sequence,
            );
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, malformed.clone(), sequence),
                    malformed,
                ),
                Err(HostProblem::Malformed)
            );
        }
    }

    #[test]
    fn handle_condition_applies_multiple_handlers_deactivation_and_error_fallback_atomically() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let invoke = |operation, arguments, sequence| {
            let request = request(operation, arguments, sequence);
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };
        invoke(
            CicsOperation::IgnoreCondition,
            BTreeMap::from([("CONDITIONS".into(), condition_list(&["INVREQ", "LENGERR"]))]),
            1,
        )
        .unwrap();
        invoke(
            CicsOperation::HandleCondition,
            BTreeMap::from([(
                "CONDITIONS".into(),
                condition_handlers(&[
                    ("ERROR", "GENERAL-HANDLER"),
                    ("INVREQ", "SPECIFIC-HANDLER"),
                    ("LENGERR", ""),
                ]),
            )]),
            2,
        )
        .unwrap();

        let pop = invoke(CicsOperation::PopHandle, BTreeMap::new(), 3).unwrap();
        assert_eq!(
            (
                pop.disposition,
                pop.condition.as_str(),
                pop.target.as_deref()
            ),
            (CicsDisposition::Handler, "INVREQ", Some("SPECIFIC-HANDLER"))
        );

        let length_error = invoke(
            CicsOperation::Enq,
            BTreeMap::from([
                ("RESOURCE".into(), enqueue_value(b"LOCK")),
                ("LENGTH".into(), cics_decimal(0)),
            ]),
            4,
        )
        .unwrap();
        assert_eq!(
            (
                length_error.disposition,
                length_error.condition.as_str(),
                length_error.response,
                length_error.response2,
                length_error.target.as_deref(),
            ),
            (
                CicsDisposition::Handler,
                "LENGERR",
                22,
                1,
                Some("GENERAL-HANDLER"),
            )
        );

        let invalid_service = CicsService::open(
            authorities(),
            Arc::new(MemoryStore::new(Default::default())),
            CicsLimits::default(),
        )
        .unwrap();
        let (invalid_invocation, _) = registered(&invalid_service);
        let seventeen = crate::generated::CICS_CONDITION_NAMES[..17]
            .iter()
            .map(|name| format!("{name}\t"))
            .collect::<Vec<_>>()
            .join("\n");
        for (sequence, schema, value) in [
            (1, "mainframe-env.cics.condition-handlers@1", ""),
            (2, "mainframe-env.cics.condition-handlers@1", "ERROR"),
            (
                3,
                "mainframe-env.cics.condition-handlers@1",
                "ERROR\tONE\nERROR\tTWO",
            ),
            (
                4,
                "mainframe-env.cics.condition-handlers@1",
                "MADEUP\tHANDLER",
            ),
            (
                5,
                "mainframe-env.cics.condition-handlers@1",
                "LENGERR\tTWO\nERROR\tONE",
            ),
            (
                6,
                "mainframe-env.cics.condition-handlers@1",
                "ERROR\tlower-case",
            ),
            (7, "mainframe-env.cics.condition-handlers@1", &seventeen),
            (8, "mainframe-env.cics.literal@1", "ERROR\tHANDLER"),
        ] {
            let malformed = request(
                CicsOperation::HandleCondition,
                BTreeMap::from([(
                    "CONDITIONS".into(),
                    BoundedPayload::new(
                        schema,
                        value.as_bytes().to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                )]),
                sequence,
            );
            assert_eq!(
                invalid_service.invoke(
                    &effect(&invalid_invocation.run_unit_id, malformed.clone(), sequence,),
                    malformed,
                ),
                Err(HostProblem::Malformed)
            );
        }
        let duplicate_legacy = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([
                ("INVREQ".into(), argument(b"ONE")),
                ("invreq".into(), argument(b"TWO")),
            ]),
            9,
        );
        assert_eq!(
            invalid_service.invoke(
                &effect(&invalid_invocation.run_unit_id, duplicate_legacy.clone(), 9),
                duplicate_legacy,
            ),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn handle_aid_is_bounded_stack_scoped_and_rejects_dpl_before_state_change() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        let invoke = |operation, arguments, sequence| {
            let request = request(operation, arguments, sequence);
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };
        invoke(
            CicsOperation::HandleAid,
            BTreeMap::from([(
                "AIDS".into(),
                aid_handlers(&[
                    ("ANYKEY", "ANY-HANDLER"),
                    ("ENTER", "ENTER-HANDLER"),
                    ("PF10", ""),
                ]),
            )]),
            1,
        )
        .unwrap();
        {
            let state = service.lock().unwrap();
            let run = state.runs.get(&invocation.run_unit_id).unwrap();
            assert_eq!(run.aid_handlers["ANYKEY"], "ANY-HANDLER");
            assert_eq!(run.aid_handlers["ENTER"], "ENTER-HANDLER");
            assert_eq!(run.aid_handlers["PF10"], "");
        }
        invoke(CicsOperation::PushHandle, BTreeMap::new(), 2).unwrap();
        invoke(
            CicsOperation::HandleAid,
            BTreeMap::from([("AIDS".into(), aid_handlers(&[("PF1", "INNER")]))]),
            3,
        )
        .unwrap();
        invoke(CicsOperation::PopHandle, BTreeMap::new(), 4).unwrap();
        {
            let state = service.lock().unwrap();
            let run = state.runs.get(&invocation.run_unit_id).unwrap();
            assert_eq!(run.aid_handlers.len(), 3);
            assert!(!run.aid_handlers.contains_key("PF1"));
        }
        for (sequence, aid, disposition, target) in [
            (20, 0xf1, CicsDisposition::Handler, Some("ANY-HANDLER")),
            (21, 0x7a, CicsDisposition::Complete, None),
        ] {
            {
                let mut state = service.lock().unwrap();
                let terminal = state.sessions.get_mut(session.as_str()).unwrap();
                terminal.aid = aid;
                terminal.input = Some(Vec::new());
            }
            let response = invoke(CicsOperation::ReceiveMap, BTreeMap::new(), sequence).unwrap();
            assert_eq!(
                (
                    response.disposition,
                    response.aid,
                    response.target.as_deref()
                ),
                (disposition, aid, target)
            );
        }

        for (sequence, schema, value) in [
            (5, "mainframe-env.cics.aid-handlers@1", "PF25\tBAD"),
            (6, "mainframe-env.cics.aid-handlers@1", "PF2\tTWO\nPF1\tONE"),
            (7, "mainframe-env.cics.aid-handlers@1", "PF1\tlower"),
            (8, "mainframe-env.cics.literal@1", "PF1\tHANDLER"),
        ] {
            let malformed = request(
                CicsOperation::HandleAid,
                BTreeMap::from([(
                    "AIDS".into(),
                    BoundedPayload::new(
                        schema,
                        value.as_bytes().to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                )]),
                sequence,
            );
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, malformed.clone(), sequence),
                    malformed,
                ),
                Err(HostProblem::Malformed)
            );
        }
        let seventeen = crate::generated::CICS_AID_NAMES[..17]
            .iter()
            .map(|name| format!("{name}\t"))
            .collect::<Vec<_>>()
            .join("\n");
        let oversized = request(
            CicsOperation::HandleAid,
            BTreeMap::from([(
                "AIDS".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.aid-handlers@1",
                    seventeen.into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            )]),
            10,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, oversized.clone(), 10),
                oversized,
            ),
            Err(HostProblem::Malformed)
        );

        let context = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let dpl = invocation_for(
            "handle-aid-dpl",
            BTreeMap::from([("cics.execution-context".into(), context)]),
        );
        let dpl_session = SessionId::new("handle-aid-dpl", 64).unwrap();
        service.create_session(&dpl_session, 24, 80).unwrap();
        service
            .register_run(dpl.clone(), &dpl_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let mut denied = request(
            CicsOperation::HandleAid,
            BTreeMap::from([("AIDS".into(), aid_handlers(&[("ENTER", "BAD")]))]),
            9,
        );
        denied.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = service
            .invoke(&effect(&dpl.run_unit_id, denied.clone(), 9), denied)
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "INVREQ", 16, 200)
        );
        let state = service.lock().unwrap();
        assert!(state.runs[&dpl.run_unit_id].aid_handlers.is_empty());
    }

    #[test]
    fn handle_state_rolls_back_when_its_session_cas_fails() {
        let store = Arc::new(FailCicsReplayCasStore::new());
        let service = service(store.clone());
        let (invocation, session) = registered(&service);
        let before = {
            let state = service.lock().unwrap();
            handlers::HandleState::from_run(state.runs.get(&invocation.run_unit_id).unwrap())
        };
        let session_before = service.lock().unwrap().sessions[session.as_str()].clone();
        store.fail_session.store(true, Ordering::SeqCst);
        let handle = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([("INVREQ".into(), argument(b"SHOULD-ROLL-BACK"))]),
            1,
        );
        assert_eq!(
            service.invoke(&effect(&invocation.run_unit_id, handle.clone(), 1), handle,),
            Err(HostProblem::InfrastructureFailure)
        );
        let state = service.lock().unwrap();
        assert_eq!(
            handlers::HandleState::from_run(state.runs.get(&invocation.run_unit_id).unwrap()),
            before
        );
        assert_eq!(state.sessions[session.as_str()].handle_state, before);
        assert_eq!(
            state.sessions[session.as_str()].version,
            session_before.version
        );
        drop(state);
        assert_eq!(
            store
                .get_provider_state("cics-session", session.as_str())
                .unwrap()
                .unwrap()
                .version,
            session_before.version
        );
    }

    #[test]
    fn terminal_handoff_preserves_handle_state_and_terminal_outcomes_clear_it() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let session = SessionId::new("terminal-handle-lifecycle", 64).unwrap();
        let first = invocation_for("terminal-handle-first", BTreeMap::new());
        service
            .launch_terminal(
                first.clone(),
                &session,
                "MENU",
                24,
                80,
                "terminal-handle-csrf",
                1,
                10_000,
            )
            .unwrap();
        let handle = request(
            CicsOperation::HandleAid,
            BTreeMap::from([("AIDS".into(), aid_handlers(&[("PF1", "PRESERVED")]))]),
            1,
        );
        service
            .invoke(&effect(&first.run_unit_id, handle.clone(), 1), handle)
            .unwrap();
        let expected = service.lock().unwrap().sessions[session.as_str()]
            .handle_state
            .clone();
        service
            .suspend_terminal_run(&session, first.principal.id(), 2)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].handle_state,
            expected
        );

        let second = invocation_for("terminal-handle-second", BTreeMap::new());
        service
            .resume_terminal(second.clone(), &session, "terminal-handle-csrf", 3)
            .unwrap();
        assert_eq!(
            handlers::HandleState::from_run(
                service
                    .lock()
                    .unwrap()
                    .runs
                    .get(&second.run_unit_id)
                    .unwrap(),
            ),
            expected
        );
        service
            .complete_terminal_run(&session, second.principal.id(), 4)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].handle_state,
            handlers::HandleState::default()
        );

        let third = invocation_for("terminal-handle-third", BTreeMap::new());
        service
            .resume_terminal(third.clone(), &session, "terminal-handle-csrf", 5)
            .unwrap();
        let handle = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([("INVREQ".into(), argument(b"DISCARDED"))]),
            1,
        );
        service
            .invoke(&effect(&third.run_unit_id, handle.clone(), 1), handle)
            .unwrap();
        assert_ne!(
            service.lock().unwrap().sessions[session.as_str()].handle_state,
            handlers::HandleState::default()
        );
        service
            .discard_terminal_run_if_present(&session, third.principal.id(), 6)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].handle_state,
            handlers::HandleState::default()
        );
    }

    #[test]
    fn complete_handle_state_survives_sqlite_reopen_and_reseeds_a_run() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-handle-state-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cics.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let invocation = invocation_for("sqlite-handle-state", BTreeMap::new());
        let session = SessionId::new("sqlite-handle-state", 64).unwrap();
        let expected;
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            service
                .register_programs(&BTreeSet::from(["ABEXIT".into()]))
                .unwrap();
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let invoke = |operation, arguments, sequence| {
                let request = request(operation, arguments, sequence);
                service.invoke(
                    &effect(&invocation.run_unit_id, request.clone(), sequence),
                    request,
                )
            };
            invoke(
                CicsOperation::HandleCondition,
                BTreeMap::from([(
                    "CONDITIONS".into(),
                    condition_handlers(&[("INVREQ", "OUTER-COND")]),
                )]),
                1,
            )
            .unwrap();
            invoke(
                CicsOperation::HandleAid,
                BTreeMap::from([("AIDS".into(), aid_handlers(&[("PF1", "OUTER-AID")]))]),
                2,
            )
            .unwrap();
            invoke(
                CicsOperation::IgnoreCondition,
                BTreeMap::from([("CONDITIONS".into(), condition_list(&["LENGERR"]))]),
                3,
            )
            .unwrap();
            invoke(
                CicsOperation::HandleAbend,
                BTreeMap::from([("LABEL".into(), argument(b"OUTER-ABEND"))]),
                4,
            )
            .unwrap();
            invoke(CicsOperation::PushHandle, BTreeMap::new(), 5).unwrap();
            invoke(
                CicsOperation::HandleCondition,
                BTreeMap::from([(
                    "CONDITIONS".into(),
                    condition_handlers(&[("ERROR", "INNER-COND")]),
                )]),
                6,
            )
            .unwrap();
            invoke(
                CicsOperation::HandleAid,
                BTreeMap::from([("AIDS".into(), aid_handlers(&[("PF2", "INNER-AID")]))]),
                7,
            )
            .unwrap();
            invoke(
                CicsOperation::IgnoreCondition,
                BTreeMap::from([("CONDITIONS".into(), condition_list(&["PGMIDERR"]))]),
                8,
            )
            .unwrap();
            invoke(
                CicsOperation::HandleAbend,
                BTreeMap::from([("PROGRAM".into(), argument(b"ABEXIT"))]),
                9,
            )
            .unwrap();
            let handled = invoke(
                CicsOperation::Abend,
                BTreeMap::from([
                    ("ABCODE".into(), argument(b"B777")),
                    ("OPTION.NODUMP".into(), argument(b"")),
                ]),
                10,
            )
            .unwrap();
            assert_eq!(handled.disposition, CicsDisposition::Transfer);
            invoke(
                CicsOperation::HandleAbend,
                BTreeMap::from([("OPTION.RESET".into(), argument(b""))]),
                11,
            )
            .unwrap();
            let state = service.lock().unwrap();
            expected =
                handlers::HandleState::from_run(state.runs.get(&invocation.run_unit_id).unwrap());
            assert_eq!(state.sessions[session.as_str()].handle_state, expected);
            assert_eq!(expected.stack.len(), 1);
            assert_eq!(expected.handlers["ERROR"], "INNER-COND");
            assert_eq!(expected.aid_handlers["PF2"], "INNER-AID");
            assert!(expected.ignored_conditions.contains("PGMIDERR"));
            assert_eq!(
                expected
                    .abend_handler
                    .as_ref()
                    .map(handlers::AbendExit::target),
                Some("ABEXIT")
            );
            assert!(matches!(
                expected.abend_handler,
                Some(handlers::AbendExit::Program(_))
            ));
            let latest = expected.latest_abend.as_ref().unwrap();
            assert_eq!(latest.code, b"B777");
            assert_eq!(latest.original_code, b"B777");
            assert!(!latest.dump_requested);
            assert_eq!(latest.program, None);
            let mut legacy8 = handlers::encode_session(&state.sessions[session.as_str()]).unwrap();
            assert_eq!(&legacy8[..5], b"MECS9");
            legacy8.truncate(legacy8.len() - 8);
            legacy8[..5].copy_from_slice(b"MECS8");
            let decoded = decode_session(
                &legacy8,
                state.sessions[session.as_str()].version,
                CicsLimits::default(),
            )
            .unwrap();
            let legacy_abend = decoded.handle_state.latest_abend.unwrap();
            assert_eq!(legacy_abend.code, b"B777");
            assert_eq!(legacy_abend.original_code, b"B777");
            assert_eq!(expected.stack[0].handlers["INVREQ"], "OUTER-COND");
            assert_eq!(expected.stack[0].aid_handlers["PF1"], "OUTER-AID");
            assert!(expected.stack[0].ignored_conditions.contains("LENGERR"));
            assert_eq!(
                expected.stack[0]
                    .abend_handler
                    .as_ref()
                    .map(handlers::AbendExit::target),
                Some("OUTER-ABEND")
            );
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            assert_eq!(
                service.lock().unwrap().sessions[session.as_str()].handle_state,
                expected
            );
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            assert_eq!(
                handlers::HandleState::from_run(
                    service
                        .lock()
                        .unwrap()
                        .runs
                        .get(&invocation.run_unit_id)
                        .unwrap(),
                ),
                expected
            );
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn enqueue_nesting_contention_conditions_and_syncpoint_release_are_durable() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store);
        let (first, _) = registered(&service);
        let second = invocation_for("enqueue-second", BTreeMap::new());
        let second_session = SessionId::new("enqueue-second", 64).unwrap();
        service.create_session(&second_session, 24, 80).unwrap();
        service
            .register_run(second.clone(), &second_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let third = invocation_for("enqueue-third", BTreeMap::new());
        let third_session = SessionId::new("enqueue-third", 64).unwrap();
        service.create_session(&third_session, 24, 80).unwrap();
        service
            .register_run(third.clone(), &third_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();

        let address = b"artifact:1:LOCK-NAME";
        for sequence in [70, 71] {
            let enq = request(
                CicsOperation::Enq,
                BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
                sequence,
            );
            assert_eq!(
                service
                    .invoke(&effect(&first.run_unit_id, enq.clone(), sequence), enq)
                    .unwrap()
                    .condition,
                "NORMAL"
            );
        }

        let waiting = request(
            CicsOperation::Enq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
            72,
        );
        assert_eq!(
            service
                .invoke(
                    &effect(&second.run_unit_id, waiting.clone(), 72),
                    waiting.clone(),
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );

        let deq = request(
            CicsOperation::Deq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
            73,
        );
        service
            .invoke(&effect(&first.run_unit_id, deq.clone(), 73), deq)
            .unwrap();
        assert_eq!(
            service
                .invoke(
                    &effect(&second.run_unit_id, waiting.clone(), 72),
                    waiting.clone(),
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );
        let deq = request(
            CicsOperation::Deq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
            74,
        );
        service
            .invoke(&effect(&first.run_unit_id, deq.clone(), 74), deq)
            .unwrap();
        let resumed = request(
            CicsOperation::Enq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
            75,
        );
        assert_eq!(
            service
                .invoke(&effect(&second.run_unit_id, resumed.clone(), 75), resumed)
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );

        let busy = request(
            CicsOperation::Enq,
            BTreeMap::from([
                ("RESOURCE".into(), enqueue_identity(address)),
                ("OPTION.NOSUSPEND".into(), argument(b"")),
            ]),
            76,
        );
        let ignored = service
            .invoke(&effect(&third.run_unit_id, busy.clone(), 76), busy)
            .unwrap();
        assert_eq!(
            (
                ignored.disposition,
                ignored.condition.as_str(),
                ignored.response
            ),
            (CicsDisposition::Ignored, "ENQBUSY", 55)
        );

        let syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 77);
        service
            .invoke(
                &effect(&second.run_unit_id, syncpoint.clone(), 77),
                syncpoint,
            )
            .unwrap();
        let enq = request(
            CicsOperation::Enq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
            78,
        );
        assert_eq!(
            service
                .invoke(&effect(&third.run_unit_id, enq.clone(), 78), enq)
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );

        for (sequence, arguments, expected) in [
            (
                79,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(b"LOCK")),
                    ("LENGTH".into(), cics_decimal(0)),
                ]),
                ("LENGERR", 22, 1),
            ),
            (
                80,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_identity(b"OTHER")),
                    ("MAXLIFETIME".into(), cics_decimal(7)),
                ]),
                ("INVREQ", 16, 2),
            ),
        ] {
            let mut invalid = request(CicsOperation::Enq, arguments, sequence);
            invalid.condition_policy = CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            };
            let response = service
                .invoke(
                    &effect(&third.run_unit_id, invalid.clone(), sequence),
                    invalid,
                )
                .unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                expected
            );
        }

        for (sequence, operation, arguments) in [
            (
                81,
                CicsOperation::Deq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_identity(address)),
                    ("OPTION.NOSUSPEND".into(), argument(b"")),
                ]),
            ),
            (
                82,
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_identity(address)),
                    ("OPTION.TASK".into(), argument(b"valued")),
                ]),
            ),
            (
                83,
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_identity(address)),
                    ("LENGTH".into(), argument(b"4")),
                ]),
            ),
        ] {
            let invalid = request(operation, arguments, sequence);
            assert_eq!(
                service.invoke(
                    &effect(&third.run_unit_id, invalid.clone(), sequence),
                    invalid,
                ),
                Err(HostProblem::Malformed)
            );
        }
    }

    #[test]
    fn task_priority_change_and_suspend_yield_once_with_exact_conditions() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        for (sequence, priority) in [(320, None), (321, Some(-1))] {
            let arguments = priority.map_or_else(BTreeMap::new, |priority| {
                BTreeMap::from([("PRIORITY".into(), cics_decimal(priority))])
            });
            let change = request(CicsOperation::ChangeTask, arguments, sequence);
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, change.clone(), sequence),
                        change,
                    )
                    .unwrap()
                    .disposition,
                CicsDisposition::Complete
            );
        }

        let change = request(
            CicsOperation::ChangeTask,
            BTreeMap::from([("PRIORITY".into(), cics_decimal(200))]),
            322,
        );
        let changed = service
            .invoke(
                &effect(&invocation.run_unit_id, change.clone(), 322),
                change,
            )
            .unwrap();
        assert_eq!(changed.disposition, CicsDisposition::Suspended);
        assert_eq!(
            changed.outputs.get("TASK.PRIORITY").unwrap().bytes(),
            b"200"
        );
        assert_eq!(
            service
                .lock()
                .unwrap()
                .runs
                .get(&invocation.run_unit_id)
                .unwrap()
                .invocation
                .priority,
            200
        );

        let suspend = request(CicsOperation::Suspend, BTreeMap::new(), 323);
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, suspend.clone(), 323),
                    suspend,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );

        let mut invalid = request(
            CicsOperation::ChangeTask,
            BTreeMap::from([("PRIORITY".into(), cics_decimal(256))]),
            324,
        );
        invalid.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, invalid.clone(), 324),
                invalid,
            )
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "INVREQ", 16, 1)
        );

        for (sequence, operation, arguments) in [
            (
                325,
                CicsOperation::ChangeTask,
                BTreeMap::from([("PRIORITY".into(), argument(b"100"))]),
            ),
            (
                326,
                CicsOperation::Suspend,
                BTreeMap::from([("PRIORITY".into(), cics_decimal(100))]),
            ),
        ] {
            let malformed = request(operation, arguments, sequence);
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, malformed.clone(), sequence),
                    malformed,
                ),
                Err(HostProblem::Malformed)
            );
        }
    }

    #[test]
    fn address_set_accepts_only_two_checked_virtual_pointer_directions() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        for (sequence, arguments) in [
            (
                327,
                BTreeMap::from([
                    ("SET.POINTER".into(), storage_target(b"artifact:1:PTR-X")),
                    (
                        "USING.ADDRESS".into(),
                        enqueue_identity(b"artifact:2:DATA-X"),
                    ),
                ]),
            ),
            (
                328,
                BTreeMap::from([
                    ("SET.ADDRESS".into(), storage_target(b"artifact:2:LINK-X")),
                    ("USING.POINTER".into(), task_value(&[0, 0x10, 0, 0])),
                ]),
            ),
        ] {
            let request = request(CicsOperation::AddressSet, arguments, sequence);
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, request.clone(), sequence),
                        request,
                    )
                    .unwrap()
                    .disposition,
                CicsDisposition::Complete
            );
        }
        for (sequence, arguments) in [
            (
                329,
                BTreeMap::from([
                    ("SET.POINTER".into(), storage_target(b"artifact:1:PTR-X")),
                    ("USING.POINTER".into(), task_value(&[0, 0x10, 0, 0])),
                ]),
            ),
            (
                330,
                BTreeMap::from([
                    ("SET.POINTER".into(), argument(b"WRONG-SCHEMA")),
                    (
                        "USING.ADDRESS".into(),
                        enqueue_identity(b"artifact:2:DATA-X"),
                    ),
                ]),
            ),
            (
                331,
                BTreeMap::from([
                    ("SET.ADDRESS".into(), storage_target(b"artifact:2:LINK-X")),
                    ("USING.POINTER".into(), task_value(&[0, 1, 2])),
                ]),
            ),
        ] {
            let request = request(CicsOperation::AddressSet, arguments, sequence);
            assert_eq!(
                service.invoke(
                    &effect(&invocation.run_unit_id, request.clone(), sequence),
                    request,
                ),
                Err(HostProblem::Malformed)
            );
        }
    }

    #[test]
    fn task_association_is_bounded_replay_safe_origin_scoped_and_command_authorized() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let (invocation, session) = registered(&service);
        let set = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), task_value(&[b'A'; 80]))]),
            330,
        );
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, set.clone(), 330),
                    set.clone()
                )
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        let (version, value, effect_key) = {
            let state = service.lock().unwrap();
            let current = &state.sessions[session.as_str()];
            (
                current.version,
                current.user_corr_data.clone(),
                current.user_corr_effect_key.clone(),
            )
        };
        assert_eq!(value, vec![b'A'; 64]);
        assert_eq!(effect_key.as_deref(), Some("outer-330"));

        let replay = store
            .get_provider_state("cics-effect-replay-v1", "outer-330")
            .unwrap()
            .unwrap();
        store
            .delete_provider_state("cics-effect-replay-v1", "outer-330", replay.version)
            .unwrap();
        let mut conflicting_bytes = [b'A'; 80];
        conflicting_bytes[79] = b'B';
        let mut conflicting = set.clone();
        conflicting
            .arguments
            .insert("USERCORRDATA".into(), task_value(&conflicting_bytes));
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, conflicting.clone(), 330),
                conflicting,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        service
            .invoke(&effect(&invocation.run_unit_id, set.clone(), 330), set)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].version,
            version,
            "the session row itself fences a retry after the replay-row crash gap"
        );

        let malformed = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), argument(b"WRONG-SCHEMA"))]),
            331,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed.clone(), 331),
                malformed,
            ),
            Err(HostProblem::Malformed)
        );

        let origin = invocation_for("association-origin", BTreeMap::new());
        let foreign = invocation_for("association-foreign", BTreeMap::new());
        let public_session = SessionId::new("association-public", 64).unwrap();
        service
            .launch_terminal(
                origin,
                &public_session,
                "MENU",
                24,
                80,
                "association-csrf",
                1,
                100,
            )
            .unwrap();
        service
            .register_run(foreign.clone(), &public_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let mut mismatch = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), task_value(b"FOREIGN"))]),
            332,
        );
        mismatch.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let mismatch = service
            .invoke(
                &effect(&foreign.run_unit_id, mismatch.clone(), 332),
                mismatch,
            )
            .unwrap();
        assert_eq!(
            (
                mismatch.condition.as_str(),
                mismatch.response,
                mismatch.response2
            ),
            ("INVREQ", 16, 1)
        );
        assert!(
            service.lock().unwrap().sessions[public_session.as_str()]
                .user_corr_data
                .is_empty()
        );

        let (host, seen) = command_authorities(true);
        let denied_store = Arc::new(MemoryStore::new(Default::default()));
        let denied = CicsService::open(host, denied_store, CicsLimits::default()).unwrap();
        let (denied_invocation, denied_session) = registered(&denied);
        let mut denied_request = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), task_value(b"DENIED"))]),
            333,
        );
        denied_request.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let denied_response = denied
            .invoke(
                &effect(&denied_invocation.run_unit_id, denied_request.clone(), 333),
                denied_request,
            )
            .unwrap();
        assert_eq!(
            (
                denied_response.condition.as_str(),
                denied_response.response,
                denied_response.response2,
            ),
            ("NOTAUTH", 70, 100)
        );
        assert!(
            denied.lock().unwrap().sessions[denied_session.as_str()]
                .user_corr_data
                .is_empty()
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                ("TCICSTRN".into(), "CICS.MENU".into(), AccessIntent::Execute,),
                (
                    "FACILITY".into(),
                    "CICS.COMMAND.SET.ASSOCIATION.USERCORRDATA".into(),
                    AccessIntent::Alter,
                ),
            ]
        );
    }

    #[test]
    fn task_association_recovers_after_replay_journal_failure() {
        let store = Arc::new(FailCicsReplayCasStore::new());
        let service = service(store.clone());
        let (invocation, session) = registered(&service);
        let set = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), task_value(b"RECOVER"))]),
            335,
        );
        store.fail_insert.store(true, Ordering::SeqCst);
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, set.clone(), 335),
                set.clone()
            ),
            Err(HostProblem::UnknownOutcome)
        );
        let after_failure = service.lock().unwrap().sessions[session.as_str()].clone();
        assert_eq!(after_failure.user_corr_data, b"RECOVER");
        assert_eq!(
            after_failure.user_corr_effect_key.as_deref(),
            Some("outer-335")
        );
        assert!(
            store
                .get_provider_state("cics-effect-replay-v1", "outer-335")
                .unwrap()
                .is_none()
        );
        service
            .invoke(&effect(&invocation.run_unit_id, set.clone(), 335), set)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].version,
            after_failure.version
        );
        assert!(
            store
                .get_provider_state("cics-effect-replay-v1", "outer-335")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn task_association_session_codec_and_value_survive_sqlite_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-association-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cics.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let invocation = invocation_for("sqlite-association", BTreeMap::new());
        let session = SessionId::new("sqlite-association", 64).unwrap();
        let set = request(
            CicsOperation::SetAssociationUserCorrData,
            BTreeMap::from([("USERCORRDATA".into(), task_value(&[b'B'; 80]))]),
            334,
        );
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            service
                .invoke(
                    &effect(&invocation.run_unit_id, set.clone(), 334),
                    set.clone(),
                )
                .unwrap();
            let current = service.lock().unwrap().sessions[session.as_str()].clone();
            let encoded = handlers::encode_session(&current).unwrap();
            assert_eq!(&encoded[..5], b"MECS9");
            let mut corrupted = encoded.clone();
            let depth = corrupted.len() - 5;
            corrupted[depth..depth + 4].copy_from_slice(&65_u32.to_be_bytes());
            assert!(matches!(
                decode_session(&corrupted, current.version, CicsLimits::default()),
                Err(HostProblem::ResourceExhausted)
            ));
            let mut legacy8 = encoded;
            legacy8[..5].copy_from_slice(b"MECS8");
            let decoded = decode_session(&legacy8, current.version, CicsLimits::default()).unwrap();
            assert_eq!(decoded.handle_state, handlers::HandleState::default());
            let mut legacy7 = legacy8;
            legacy7.pop();
            legacy7[..5].copy_from_slice(b"MECS7");
            let decoded = decode_session(&legacy7, current.version, CicsLimits::default()).unwrap();
            assert_eq!(decoded.user_corr_data, current.user_corr_data);
            assert_eq!(decoded.user_corr_effect_key, current.user_corr_effect_key);
            assert_eq!(
                decoded.user_corr_request_digest,
                current.user_corr_request_digest
            );
            assert_eq!(decoded.handle_state, handlers::HandleState::default());
            let mut legacy6 = legacy7;
            legacy6.truncate(legacy6.len() - 18);
            legacy6.extend_from_slice(&[0; 24]);
            legacy6[..5].copy_from_slice(b"MECS6");
            let decoded = decode_session(&legacy6, current.version, CicsLimits::default()).unwrap();
            assert_eq!(decoded.user_corr_data, current.user_corr_data);
            assert_eq!(decoded.user_corr_effect_key, current.user_corr_effect_key);
            assert_eq!(
                decoded.user_corr_request_digest,
                current.user_corr_request_digest
            );
            assert_eq!(decoded.handle_state, handlers::HandleState::default());
            let mut legacy = legacy6;
            legacy.truncate(legacy.len() - 24);
            legacy[..5].copy_from_slice(b"MECS5");
            let decoded = decode_session(&legacy, current.version, CicsLimits::default()).unwrap();
            assert_eq!(decoded.user_corr_data, current.user_corr_data);
            assert_eq!(decoded.user_corr_effect_key, current.user_corr_effect_key);
            assert_eq!(
                decoded.user_corr_request_digest,
                current.user_corr_request_digest
            );
            assert_eq!(decoded.handle_state, handlers::HandleState::default());
            let suffix = 8
                + current.user_corr_data.len()
                + current.user_corr_effect_key.as_deref().unwrap().len()
                + 33;
            legacy.truncate(legacy.len() - suffix);
            legacy[..5].copy_from_slice(b"MECS4");
            let decoded = decode_session(&legacy, current.version, CicsLimits::default()).unwrap();
            assert!(decoded.user_corr_data.is_empty());
            assert_eq!(decoded.user_corr_effect_key, None);
            assert_eq!(decoded.user_corr_request_digest, None);
            assert_eq!(decoded.screen, current.screen);
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            let current = service.lock().unwrap().sessions[session.as_str()].clone();
            assert_eq!(current.user_corr_data, vec![b'B'; 64]);
            assert_eq!(current.user_corr_effect_key.as_deref(), Some("outer-334"));
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            service
                .invoke(&effect(&invocation.run_unit_id, set.clone(), 334), set)
                .unwrap();
            assert_eq!(
                service.lock().unwrap().sessions[session.as_str()].version,
                current.version
            );
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn concurrent_task_association_uses_postgres_cas_and_reopen() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit PostgreSQL test URL required");
        let suffix = std::process::id().to_string();
        let session = SessionId::new(format!("postgres-association-{suffix}"), 128).unwrap();
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            service(store).create_session(&session, 24, 80).unwrap();
        }
        let first = invocation_for(&format!("postgres-association-a-{suffix}"), BTreeMap::new());
        let second = invocation_for(&format!("postgres-association-b-{suffix}"), BTreeMap::new());
        let first_service = service(Arc::new(
            PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap(),
        ));
        let second_service = service(Arc::new(
            PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap(),
        ));
        for (service, invocation) in [(&first_service, &first), (&second_service, &second)] {
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let invoke = |service: Arc<CicsService>, invocation: Invocation, sequence, byte| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let request = request(
                    CicsOperation::SetAssociationUserCorrData,
                    BTreeMap::from([("USERCORRDATA".into(), task_value(&[byte; 80]))]),
                    sequence,
                );
                barrier.wait();
                (
                    byte,
                    service.invoke(
                        &effect(&invocation.run_unit_id, request.clone(), sequence),
                        request,
                    ),
                )
            })
        };
        let first_result = invoke(first_service, first, 340, b'A').join().unwrap();
        let second_result = invoke(second_service, second, 341, b'B').join().unwrap();
        let outcomes = [first_result, second_result];
        assert_eq!(
            outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        assert!(
            outcomes
                .iter()
                .any(|(_, result)| { matches!(result, Err(HostProblem::IdempotencyConflict)) })
        );
        let winning = outcomes
            .iter()
            .find_map(|(byte, result)| result.is_ok().then_some(*byte))
            .unwrap();
        let reopened = service(Arc::new(
            PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap(),
        ));
        let current = reopened.lock().unwrap().sessions[session.as_str()].clone();
        assert_eq!(current.user_corr_data, vec![winning; 64]);
        assert_eq!(current.version, 2);
    }

    #[test]
    fn enqueue_models_distinguish_region_local_global_address_and_disabled_resources() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        service
            .register_enqueue_models(&[
                enqueue_model("GLOBAL", "GLOBAL*", Some("G001"), true),
                enqueue_model("LOCAL", "LOCAL*", None, true),
            ])
            .unwrap();
        let first = invocation_for("enqueue-model-first", BTreeMap::new());
        let second = invocation_for("enqueue-model-second", BTreeMap::new());
        let first_session = SessionId::new("enqueue-model-first", 64).unwrap();
        let second_session = SessionId::new("enqueue-model-second", 64).unwrap();
        for (invocation, session, applid, sysid) in [
            (&first, &first_session, "AP01", "S001"),
            (&second, &second_session, "AP02", "S002"),
        ] {
            service.create_session(session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), session, "MENU", applid, sysid)
                .unwrap();
        }

        let content_request = |operation, resource: &[u8], sequence, nosuspend| {
            let mut arguments = BTreeMap::from([
                ("RESOURCE".into(), enqueue_value(resource)),
                (
                    "LENGTH".into(),
                    cics_decimal(i64::try_from(resource.len()).unwrap()),
                ),
            ]);
            if nosuspend {
                arguments.insert("OPTION.NOSUSPEND".into(), argument(b""));
            }
            request(operation, arguments, sequence)
        };
        let invoke = |invocation: &Invocation, request: CicsRequest, sequence| {
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), sequence),
                request,
            )
        };

        let global = b"GLOBAL-LOCK";
        assert_eq!(
            invoke(
                &first,
                content_request(CicsOperation::Enq, global, 300, false),
                300,
            )
            .unwrap()
            .disposition,
            CicsDisposition::Complete
        );
        let busy = invoke(
            &second,
            content_request(CicsOperation::Enq, global, 301, true),
            301,
        )
        .unwrap();
        assert_eq!(
            (busy.disposition, busy.condition.as_str(), busy.response),
            (CicsDisposition::Ignored, "ENQBUSY", 55)
        );
        invoke(
            &first,
            content_request(CicsOperation::Deq, global, 302, false),
            302,
        )
        .unwrap();

        for (ordinal, resource) in [b"LOCAL-LOCK".as_slice(), b"OTHER-LOCK".as_slice()]
            .into_iter()
            .enumerate()
        {
            let sequence = 303 + u64::try_from(ordinal).unwrap() * 2;
            for (invocation, sequence) in [(&first, sequence), (&second, sequence + 1)] {
                assert_eq!(
                    invoke(
                        invocation,
                        content_request(CicsOperation::Enq, resource, sequence, false),
                        sequence,
                    )
                    .unwrap()
                    .disposition,
                    CicsDisposition::Complete
                );
            }
        }

        let address = b"same-storage-address";
        for (invocation, sequence) in [(&first, 307), (&second, 308)] {
            let enqueue = request(
                CicsOperation::Enq,
                BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
                sequence,
            );
            assert_eq!(
                invoke(invocation, enqueue, sequence).unwrap().disposition,
                CicsDisposition::Complete
            );
        }
        for (invocation, sequence) in [(&first, 309), (&second, 310)] {
            let syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), sequence);
            invoke(invocation, syncpoint, sequence).unwrap();
        }
        assert!(
            store
                .list_provider_state("cics-enqueue-v1", 2)
                .unwrap()
                .is_empty()
        );

        let disabled_store = Arc::new(MemoryStore::new(Default::default()));
        let disabled = self::service(disabled_store.clone());
        disabled
            .register_enqueue_models(&[enqueue_model("DISABLED", "STOP*", Some("G002"), false)])
            .unwrap();
        let (invocation, _) = registered(&disabled);
        let enqueue = content_request(CicsOperation::Enq, b"STOP-NOW", 311, false);
        assert_eq!(
            disabled
                .invoke(
                    &effect(&invocation.run_unit_id, enqueue.clone(), 311),
                    enqueue
                )
                .unwrap()
                .disposition,
            CicsDisposition::Abended
        );
        assert!(
            disabled_store
                .list_provider_state("cics-enqueue-v1", 1)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn enqueue_model_installation_is_bounded_atomic_and_restart_validated() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        assert_eq!(
            service.register_enqueue_models(&[
                enqueue_model("ONE", "LOCK*", Some("G001"), true),
                enqueue_model("TWO", "LOCK-EXACT", Some("G002"), true),
            ]),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            service.register_enqueue_models(&[enqueue_model(
                "BAD",
                "LOCK",
                Some("TOO-LONG"),
                true,
            )]),
            Err(HostProblem::Malformed)
        );
        let models = [
            enqueue_model("one", "LOCK*", Some("g001"), true),
            enqueue_model("LOCAL", "REGION*", None, true),
        ];
        service.register_enqueue_models(&models).unwrap();
        service.register_enqueue_models(&models).unwrap();
        assert_eq!(
            service.register_enqueue_models(&[enqueue_model(
                "THREE",
                "OTHER*",
                Some("G003"),
                true,
            )]),
            Err(HostProblem::IdempotencyConflict)
        );
        drop(service);
        let reopened = self::service(store.clone());
        reopened.register_enqueue_models(&models).unwrap();
        drop(reopened);
        let catalog = store
            .get_provider_state("cics-enqueue-model-catalog-v1", "models")
            .unwrap()
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: 2,
                    payload: b"corrupt".to_vec(),
                    ..catalog
                },
                Some(1),
            )
            .unwrap();
        assert!(matches!(
            CicsService::open(authorities(), store, CicsLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        ));

        let race_store: Arc<dyn ProviderStateStore> =
            Arc::new(MemoryStore::new(Default::default()));
        let first = self::service(race_store.clone());
        let second = self::service(race_store.clone());
        let barrier = Arc::new(Barrier::new(2));
        let install = |service: Arc<CicsService>, model: CicsEnqueueModelDefinition| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.register_enqueue_models(&[model])
            })
        };
        let first = install(first, enqueue_model("FIRST", "FIRST*", Some("G001"), true));
        let second = install(
            second,
            enqueue_model("SECOND", "SECOND*", Some("G002"), true),
        );
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == Err(HostProblem::IdempotencyConflict))
                .count(),
            1
        );
        self::service(race_store);

        let stale_store: Arc<dyn ProviderStateStore> =
            Arc::new(MemoryStore::new(Default::default()));
        let stale = self::service(stale_store.clone());
        let stale_invocation = invocation_for("stale-model-reader", BTreeMap::new());
        let stale_session = SessionId::new("stale-model-reader", 64).unwrap();
        stale.create_session(&stale_session, 24, 80).unwrap();
        let installer = self::service(stale_store);
        installer
            .register_enqueue_models(&[enqueue_model("FRESH", "FRESH*", Some("G005"), true)])
            .unwrap();
        stale
            .register_run(
                stale_invocation.clone(),
                &stale_session,
                "MENU",
                "AP01",
                "S001",
            )
            .unwrap();
        let enqueue = request(
            CicsOperation::Enq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(b"stale"))]),
            313,
        );
        assert_eq!(
            stale.invoke(
                &effect(&stale_invocation.run_unit_id, enqueue.clone(), 313),
                enqueue,
            ),
            Err(HostProblem::InfrastructureFailure)
        );

        let limited = CicsService::open(
            authorities(),
            Arc::new(MemoryStore::new(Default::default())),
            CicsLimits {
                max_enqueue_models: 1,
                ..CicsLimits::default()
            },
        )
        .unwrap();
        assert_eq!(
            limited.register_enqueue_models(&models),
            Err(HostProblem::ResourceExhausted)
        );

        let active_store = Arc::new(MemoryStore::new(Default::default()));
        let active = self::service(active_store);
        let (invocation, _) = registered(&active);
        let enqueue = request(
            CicsOperation::Enq,
            BTreeMap::from([("RESOURCE".into(), enqueue_identity(b"active-lock"))]),
            312,
        );
        active
            .invoke(
                &effect(&invocation.run_unit_id, enqueue.clone(), 312),
                enqueue,
            )
            .unwrap();
        assert_eq!(
            active.register_enqueue_models(&[enqueue_model("LATE", "LOCK*", Some("G004"), true,)]),
            Err(HostProblem::IdempotencyConflict)
        );
    }

    #[test]
    fn enqueue_waiters_are_fifo_replay_bound_and_handler_policy_is_exact() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let (first, _) = registered(&service);
        let second = invocation_for("enqueue-fifo-second", BTreeMap::new());
        let second_session = SessionId::new("enqueue-fifo-second", 64).unwrap();
        service.create_session(&second_session, 24, 80).unwrap();
        service
            .register_run(second.clone(), &second_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let third = invocation_for("enqueue-fifo-third", BTreeMap::new());
        let third_session = SessionId::new("enqueue-fifo-third", 64).unwrap();
        service.create_session(&third_session, 24, 80).unwrap();
        service
            .register_run(third.clone(), &third_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let address = b"fifo-resource";
        let enqueue = |sequence| {
            request(
                CicsOperation::Enq,
                BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
                sequence,
            )
        };
        let dequeue = |sequence| {
            request(
                CicsOperation::Deq,
                BTreeMap::from([("RESOURCE".into(), enqueue_identity(address))]),
                sequence,
            )
        };

        let acquire = enqueue(100);
        assert_eq!(
            service
                .invoke(&effect(&first.run_unit_id, acquire.clone(), 100), acquire)
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        for (invocation, sequence) in [(&second, 101), (&third, 102)] {
            let waiting = enqueue(sequence);
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, waiting.clone(), sequence),
                        waiting,
                    )
                    .unwrap()
                    .disposition,
                CicsDisposition::Suspended
            );
        }
        let release = dequeue(103);
        service
            .invoke(&effect(&first.run_unit_id, release.clone(), 103), release)
            .unwrap();

        let third_early = enqueue(104);
        assert_eq!(
            service
                .invoke(
                    &effect(&third.run_unit_id, third_early.clone(), 104),
                    third_early,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );
        let second_resume = enqueue(105);
        assert_eq!(
            service
                .invoke(
                    &effect(&second.run_unit_id, second_resume.clone(), 105),
                    second_resume,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        let release = dequeue(106);
        service
            .invoke(&effect(&second.run_unit_id, release.clone(), 106), release)
            .unwrap();
        let third_resume = enqueue(107);
        assert_eq!(
            service
                .invoke(
                    &effect(&third.run_unit_id, third_resume.clone(), 107),
                    third_resume,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        let syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 108);
        service
            .invoke(
                &effect(&third.run_unit_id, syncpoint.clone(), 108),
                syncpoint,
            )
            .unwrap();
        assert!(
            store
                .list_provider_state("cics-enqueue-v1", 16)
                .unwrap()
                .is_empty()
        );
        let catalog = store
            .get_provider_state("cics-enqueue-catalog-v1", "locks")
            .unwrap()
            .expect("enqueue catalog");
        assert_eq!(&catalog.payload[8..], &0_u64.to_be_bytes());
        let replay_rows = store
            .list_provider_state("cics-effect-replay-v1", 32)
            .unwrap();
        assert_eq!(replay_rows.len(), 9);
        assert!(replay_rows.iter().all(|row| {
            crate::retention::describe_cics_replay_row(row, None, CicsLimits::default()).is_ok_and(
                |descriptor| {
                    descriptor.retention
                        == crate::retention::CicsReplayRetentionState::PendingProtected
                },
            )
        }));

        let fourth = invocation_for("enqueue-handler-fourth", BTreeMap::new());
        let fourth_session = SessionId::new("enqueue-handler-fourth", 64).unwrap();
        service.create_session(&fourth_session, 24, 80).unwrap();
        service
            .register_run(fourth.clone(), &fourth_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let reacquire = enqueue(109);
        service
            .invoke(
                &effect(&first.run_unit_id, reacquire.clone(), 109),
                reacquire,
            )
            .unwrap();
        let handle = request(
            CicsOperation::HandleCondition,
            BTreeMap::from([("ENQBUSY".into(), argument(b"BUSY-LABEL"))]),
            110,
        );
        service
            .invoke(&effect(&fourth.run_unit_id, handle.clone(), 110), handle)
            .unwrap();
        let handled = enqueue(111);
        let handled = service
            .invoke(&effect(&fourth.run_unit_id, handled.clone(), 111), handled)
            .unwrap();
        assert_eq!(handled.disposition, CicsDisposition::Handler);
        assert_eq!(handled.target.as_deref(), Some("BUSY-LABEL"));

        let mut nohandle = enqueue(112);
        nohandle.condition_policy = CicsConditionPolicy::NoHandle;
        assert_eq!(
            service
                .invoke(
                    &effect(&fourth.run_unit_id, nohandle.clone(), 112),
                    nohandle,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );
    }

    #[test]
    fn cancelling_one_side_of_an_enqueue_cycle_promotes_the_surviving_task() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let first = invocation_for("enqueue-cycle-first", BTreeMap::new());
        let second = invocation_for("enqueue-cycle-second", BTreeMap::new());
        let first_session = SessionId::new("enqueue-cycle-first", 64).unwrap();
        let second_session = SessionId::new("enqueue-cycle-second", 64).unwrap();
        service
            .launch_terminal(
                first.clone(),
                &first_session,
                "MENU",
                24,
                80,
                "enqueue-cycle-first-csrf",
                1,
                100,
            )
            .unwrap();
        service
            .launch_terminal(
                second.clone(),
                &second_session,
                "MENU",
                24,
                80,
                "enqueue-cycle-second-csrf",
                1,
                100,
            )
            .unwrap();
        let enqueue = |resource: &'static [u8], sequence| {
            request(
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(resource)),
                    (
                        "LENGTH".into(),
                        cics_decimal(i64::try_from(resource.len()).unwrap()),
                    ),
                    ("OPTION.TASK".into(), argument(b"")),
                ]),
                sequence,
            )
        };
        let first_a = enqueue(b"RESOURCE-A", 200);
        service
            .invoke(&effect(&first.run_unit_id, first_a.clone(), 200), first_a)
            .unwrap();
        let second_b = enqueue(b"RESOURCE-B", 201);
        service
            .invoke(
                &effect(&second.run_unit_id, second_b.clone(), 201),
                second_b,
            )
            .unwrap();
        let first_wait = enqueue(b"RESOURCE-B", 202);
        assert_eq!(
            service
                .invoke(
                    &effect(&first.run_unit_id, first_wait.clone(), 202),
                    first_wait,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );
        let second_wait = enqueue(b"RESOURCE-A", 203);
        assert_eq!(
            service
                .invoke(
                    &effect(&second.run_unit_id, second_wait.clone(), 203),
                    second_wait,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );

        service
            .discard_terminal_run_if_present(&second_session, second.principal.id(), 2)
            .unwrap();
        let first_resume = enqueue(b"RESOURCE-B", 204);
        assert_eq!(
            service
                .invoke(
                    &effect(&first.run_unit_id, first_resume.clone(), 204),
                    first_resume,
                )
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        service
            .complete_terminal_run(&first_session, first.principal.id(), 2)
            .unwrap();
        assert!(
            store
                .list_provider_state("cics-enqueue-v1", 4)
                .unwrap()
                .is_empty()
        );
        let catalog = store
            .get_provider_state("cics-enqueue-catalog-v1", "locks")
            .unwrap()
            .unwrap();
        assert_eq!(&catalog.payload[8..], &0_u64.to_be_bytes());
    }

    #[test]
    fn enqueue_lock_catalog_enforces_the_configured_resource_bound() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open(
            authorities(),
            store.clone(),
            CicsLimits {
                max_queue_records: 1,
                ..CicsLimits::default()
            },
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        for (sequence, resource, expected) in [
            (210, b"A".as_slice(), Ok(())),
            (211, b"B".as_slice(), Err(HostProblem::ResourceExhausted)),
        ] {
            let enqueue = request(
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(resource)),
                    ("LENGTH".into(), cics_decimal(1)),
                ]),
                sequence,
            );
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, enqueue.clone(), sequence),
                        enqueue,
                    )
                    .map(|_| ()),
                expected
            );
        }
        assert_eq!(
            store
                .list_provider_state("cics-enqueue-v1", 2)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn enqueue_wait_and_promotion_survive_sqlite_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-enqueue-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cics.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let first = invocation_for("sqlite-enqueue-first", BTreeMap::new());
        let second = invocation_for("sqlite-enqueue-second", BTreeMap::new());
        let first_session = SessionId::new("sqlite-enqueue-first", 64).unwrap();
        let second_session = SessionId::new("sqlite-enqueue-second", 64).unwrap();
        let address = b"sqlite-enqueue-resource";
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            service
                .register_enqueue_models(&[enqueue_model("SQLITE", "sqlite*", Some("Q001"), true)])
                .unwrap();
            for (invocation, session, applid, sysid) in [
                (&first, &first_session, "AP01", "S001"),
                (&second, &second_session, "AP02", "S002"),
            ] {
                service.create_session(session, 24, 80).unwrap();
                service
                    .register_run(invocation.clone(), session, "MENU", applid, sysid)
                    .unwrap();
            }
            let acquire = request(
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(address)),
                    (
                        "LENGTH".into(),
                        cics_decimal(i64::try_from(address.len()).unwrap()),
                    ),
                ]),
                120,
            );
            service
                .invoke(&effect(&first.run_unit_id, acquire.clone(), 120), acquire)
                .unwrap();
            let wait = request(
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(address)),
                    (
                        "LENGTH".into(),
                        cics_decimal(i64::try_from(address.len()).unwrap()),
                    ),
                ]),
                121,
            );
            assert_eq!(
                service
                    .invoke(&effect(&second.run_unit_id, wait.clone(), 121), wait)
                    .unwrap()
                    .disposition,
                CicsDisposition::Suspended
            );
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store.clone());
            for (invocation, session, applid, sysid) in [
                (&first, &first_session, "AP01", "S001"),
                (&second, &second_session, "AP02", "S002"),
            ] {
                service
                    .register_run(invocation.clone(), session, "MENU", applid, sysid)
                    .unwrap();
            }
            let release = request(
                CicsOperation::Deq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(address)),
                    (
                        "LENGTH".into(),
                        cics_decimal(i64::try_from(address.len()).unwrap()),
                    ),
                ]),
                122,
            );
            service
                .invoke(&effect(&first.run_unit_id, release.clone(), 122), release)
                .unwrap();
            let resume = request(
                CicsOperation::Enq,
                BTreeMap::from([
                    ("RESOURCE".into(), enqueue_value(address)),
                    (
                        "LENGTH".into(),
                        cics_decimal(i64::try_from(address.len()).unwrap()),
                    ),
                ]),
                123,
            );
            assert_eq!(
                service
                    .invoke(&effect(&second.run_unit_id, resume.clone(), 123), resume)
                    .unwrap()
                    .disposition,
                CicsDisposition::Complete
            );
            let syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 124);
            service
                .invoke(
                    &effect(&second.run_unit_id, syncpoint.clone(), 124),
                    syncpoint,
                )
                .unwrap();
            assert!(
                store
                    .list_provider_state("cics-enqueue-v1", 2)
                    .unwrap()
                    .is_empty()
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn concurrent_global_enqueue_uses_one_postgres_owner_and_fifo_promotion() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit PostgreSQL test URL required");
        let suffix = std::process::id().to_string();
        let first = invocation_for(&format!("postgres-enqueue-first-{suffix}"), BTreeMap::new());
        let second = invocation_for(
            &format!("postgres-enqueue-second-{suffix}"),
            BTreeMap::new(),
        );
        let first_session =
            SessionId::new(format!("postgres-enqueue-first-{suffix}"), 128).unwrap();
        let second_session =
            SessionId::new(format!("postgres-enqueue-second-{suffix}"), 128).unwrap();
        let initial_store: Arc<dyn ProviderStateStore> =
            Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let initial = service(initial_store);
        initial
            .register_enqueue_models(&[enqueue_model("POSTGRES", "postgres*", Some("P001"), true)])
            .unwrap();
        initial.create_session(&first_session, 24, 80).unwrap();
        initial.create_session(&second_session, 24, 80).unwrap();
        drop(initial);

        let first_store: Arc<dyn ProviderStateStore> =
            Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let second_store: Arc<dyn ProviderStateStore> =
            Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let first_service = service(first_store);
        let second_service = service(second_store);
        first_service
            .register_run(first.clone(), &first_session, "MENU", "AP01", "S001")
            .unwrap();
        second_service
            .register_run(second.clone(), &second_session, "MENU", "AP02", "S002")
            .unwrap();

        let address = format!("postgres-enqueue-resource-{suffix}").into_bytes();
        let barrier = Arc::new(Barrier::new(2));
        let invoke = |service: Arc<CicsService>, invocation: Invocation, ordinal: u64| {
            let address = address.clone();
            let barrier = barrier.clone();
            let suffix = suffix.clone();
            std::thread::spawn(move || {
                let mut enqueue = request(
                    CicsOperation::Enq,
                    BTreeMap::from([("RESOURCE".into(), enqueue_value(&address))]),
                    ordinal,
                );
                enqueue.arguments.insert(
                    "LENGTH".into(),
                    cics_decimal(i64::try_from(address.len()).unwrap()),
                );
                enqueue.mutation.as_mut().unwrap().idempotency_key = IdempotencyKey::new(
                    format!("postgres-enqueue-{suffix}-{ordinal}"),
                    InvocationLimits::default(),
                )
                .unwrap();
                barrier.wait();
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, enqueue.clone(), ordinal),
                        enqueue,
                    )
                    .unwrap()
                    .disposition
            })
        };
        let first_join = invoke(first_service.clone(), first.clone(), 130);
        let second_join = invoke(second_service.clone(), second.clone(), 131);
        let first_disposition = first_join.join().unwrap();
        let second_disposition = second_join.join().unwrap();
        assert_eq!(
            [first_disposition, second_disposition]
                .into_iter()
                .filter(|disposition| *disposition == CicsDisposition::Complete)
                .count(),
            1
        );
        assert_eq!(
            [first_disposition, second_disposition]
                .into_iter()
                .filter(|disposition| *disposition == CicsDisposition::Suspended)
                .count(),
            1
        );

        let (owner_service, owner, waiter_service, waiter) =
            if first_disposition == CicsDisposition::Complete {
                (&first_service, &first, &second_service, &second)
            } else {
                (&second_service, &second, &first_service, &first)
            };
        let mut release = request(
            CicsOperation::Deq,
            BTreeMap::from([
                ("RESOURCE".into(), enqueue_value(&address)),
                (
                    "LENGTH".into(),
                    cics_decimal(i64::try_from(address.len()).unwrap()),
                ),
            ]),
            132,
        );
        release.mutation.as_mut().unwrap().idempotency_key = IdempotencyKey::new(
            format!("postgres-enqueue-{suffix}-132"),
            InvocationLimits::default(),
        )
        .unwrap();
        owner_service
            .invoke(&effect(&owner.run_unit_id, release.clone(), 132), release)
            .unwrap();
        let mut resume = request(
            CicsOperation::Enq,
            BTreeMap::from([
                ("RESOURCE".into(), enqueue_value(&address)),
                (
                    "LENGTH".into(),
                    cics_decimal(i64::try_from(address.len()).unwrap()),
                ),
            ]),
            133,
        );
        resume.mutation.as_mut().unwrap().idempotency_key = IdempotencyKey::new(
            format!("postgres-enqueue-{suffix}-133"),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            waiter_service
                .invoke(&effect(&waiter.run_unit_id, resume.clone(), 133), resume)
                .unwrap()
                .disposition,
            CicsDisposition::Complete
        );
        let mut syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 134);
        syncpoint.mutation.as_mut().unwrap().idempotency_key = IdempotencyKey::new(
            format!("postgres-enqueue-{suffix}-134"),
            InvocationLimits::default(),
        )
        .unwrap();
        waiter_service
            .invoke(
                &effect(&waiter.run_unit_id, syncpoint.clone(), 134),
                syncpoint,
            )
            .unwrap();
        let reopened: Arc<dyn ProviderStateStore> =
            Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        service(reopened);
    }

    #[test]
    fn syncpoint_rejects_unowned_dpl_context_before_uow_mutation() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        for (sequence, context) in [
            (61, b"dpl-without-synconreturn".as_slice()),
            (62, b"dpl-executionset-subset".as_slice()),
        ] {
            let binding = BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                context.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap();
            let invocation = invocation_for(
                &format!("dpl-{sequence}"),
                BTreeMap::from([("cics.execution-context".into(), binding)]),
            );
            let session = SessionId::new(format!("dpl-session-{sequence}"), 64).unwrap();
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let mut request = request(CicsOperation::Syncpoint, BTreeMap::new(), sequence);
            request.condition_policy = CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            };
            let response = service
                .invoke(
                    &effect(&invocation.run_unit_id, request.clone(), sequence),
                    request,
                )
                .unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                ("INVREQ", 16, 200)
            );
            assert_eq!(response.unit_of_work, None);
            assert!(
                store
                    .get_provider_state("cics-uow", &format!("outer-{sequence}"))
                    .unwrap()
                    .is_none()
            );
        }

        let binding = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let invocation = invocation_for(
            "dpl-synconreturn",
            BTreeMap::from([("cics.execution-context".into(), binding)]),
        );
        let session = SessionId::new("dpl-synconreturn", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let allowed_request = request(CicsOperation::Syncpoint, BTreeMap::new(), 63);
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, allowed_request.clone(), 63),
                    allowed_request,
                )
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );

        let malformed = BoundedPayload::new(
            "mainframe-env.cics.argument@1",
            b"dpl-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let invocation = invocation_for(
            "dpl-malformed",
            BTreeMap::from([("cics.execution-context".into(), malformed)]),
        );
        let session = SessionId::new("dpl-malformed", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let malformed_request = request(CicsOperation::Syncpoint, BTreeMap::new(), 64);
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, malformed_request.clone(), 64),
                malformed_request,
            ),
            Err(HostProblem::Malformed)
        );
        assert!(
            store
                .get_provider_state("cics-uow", "outer-64")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn remote_commit_refusal_rolls_back_and_returns_rolledback_condition() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let limits = InvocationLimits::default();
        let context = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".to_vec(),
            limits,
        )
        .unwrap();
        let remote_outcome = BoundedPayload::new(
            "mainframe-env.cics.syncpoint.remote-outcome@1",
            b"unable-to-commit".to_vec(),
            limits,
        )
        .unwrap();
        let invocation = invocation_for(
            "dpl-remote-rollback",
            BTreeMap::from([
                ("cics.execution-context".into(), context),
                ("cics.syncpoint.remote-outcome".into(), remote_outcome),
            ]),
        );
        let session = SessionId::new("dpl-remote-rollback", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let mut syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 65);
        syncpoint.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, syncpoint.clone(), 65),
                syncpoint.clone(),
            )
            .unwrap();
        assert_eq!(
            (
                response.condition.as_str(),
                response.response,
                response.response2,
                response.unit_of_work
            ),
            ("ROLLEDBACK", 82, 0, None)
        );
        let record = store
            .get_provider_state("cics-uow", "outer-65")
            .unwrap()
            .unwrap();
        assert_eq!(record.version, 2);
        let uow = decode_uow(&record.payload).unwrap();
        assert!(uow.finalized);
        assert_eq!(uow.outcome, CicsUnitOfWorkOutcome::RolledBack);
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, syncpoint.clone(), 65),
                    syncpoint
                )
                .unwrap(),
            response
        );

        let remote_outcome = BoundedPayload::new(
            "mainframe-env.cics.syncpoint.remote-outcome@1",
            b"unable-to-commit".to_vec(),
            limits,
        )
        .unwrap();
        let local = invocation_for(
            "local-remote-outcome",
            BTreeMap::from([("cics.syncpoint.remote-outcome".into(), remote_outcome)]),
        );
        let local_session = SessionId::new("local-remote-outcome", 64).unwrap();
        service.create_session(&local_session, 24, 80).unwrap();
        service
            .register_run(local.clone(), &local_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let local_request = request(CicsOperation::Syncpoint, BTreeMap::new(), 66);
        assert_eq!(
            service.invoke(
                &effect(&local.run_unit_id, local_request.clone(), 66),
                local_request,
            ),
            Err(HostProblem::Malformed)
        );
        assert!(
            store
                .get_provider_state("cics-uow", "outer-66")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn remote_commit_refusal_replays_after_sqlite_restart() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-remote-rollback-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cics.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let limits = InvocationLimits::default();
        let invocation = invocation_for(
            "sqlite-remote-rollback",
            BTreeMap::from([
                (
                    "cics.execution-context".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.execution-context@1",
                        b"dpl-synconreturn".to_vec(),
                        limits,
                    )
                    .unwrap(),
                ),
                (
                    "cics.syncpoint.remote-outcome".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.syncpoint.remote-outcome@1",
                        b"unable-to-commit".to_vec(),
                        limits,
                    )
                    .unwrap(),
                ),
            ]),
        );
        let session = SessionId::new("sqlite-remote-rollback", 64).unwrap();
        let mut syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 67);
        syncpoint.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let expected = {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            service
                .invoke(
                    &effect(&invocation.run_unit_id, syncpoint.clone(), 67),
                    syncpoint.clone(),
                )
                .unwrap()
        };
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store.clone());
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, syncpoint.clone(), 67),
                        syncpoint,
                    )
                    .unwrap(),
                expected
            );
            let record = store
                .get_provider_state("cics-uow", "outer-67")
                .unwrap()
                .unwrap();
            assert_eq!(
                decode_uow(&record.payload).unwrap().outcome,
                CicsUnitOfWorkOutcome::RolledBack
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn remote_commit_refusal_replays_after_postgres_reopen() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit PostgreSQL test URL required");
        let suffix = std::process::id();
        let limits = InvocationLimits::default();
        let invocation = invocation_for(
            &format!("postgres-remote-rollback-{suffix}"),
            BTreeMap::from([
                (
                    "cics.execution-context".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.execution-context@1",
                        b"dpl-synconreturn".to_vec(),
                        limits,
                    )
                    .unwrap(),
                ),
                (
                    "cics.syncpoint.remote-outcome".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.syncpoint.remote-outcome@1",
                        b"unable-to-commit".to_vec(),
                        limits,
                    )
                    .unwrap(),
                ),
            ]),
        );
        let session = SessionId::new(format!("postgres-remote-rollback-{suffix}"), 64).unwrap();
        let mut syncpoint = request(CicsOperation::Syncpoint, BTreeMap::new(), 68);
        syncpoint.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let effect_key = IdempotencyKey::new(
            format!("postgres-remote-rollback-{suffix}"),
            InvocationLimits::default(),
        )
        .unwrap();
        syncpoint.mutation.as_mut().unwrap().idempotency_key = effect_key.clone();
        let expected = {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let service = service(store);
            service.create_session(&session, 24, 80).unwrap();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            service
                .invoke(
                    &effect(&invocation.run_unit_id, syncpoint.clone(), 68),
                    syncpoint.clone(),
                )
                .unwrap()
        };
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let service = service(store.clone());
        service
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, syncpoint.clone(), 68),
                    syncpoint,
                )
                .unwrap(),
            expected
        );
        let record = store
            .get_provider_state("cics-uow", effect_key.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_uow(&record.payload).unwrap().outcome,
            CicsUnitOfWorkOutcome::RolledBack
        );
    }

    #[test]
    fn terminal_suspends_without_worker_and_resumes_from_durable_input() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        let receive = request(CicsOperation::ReceiveMap, BTreeMap::new(), 1);
        let first = service
            .invoke(
                &effect(&invocation.run_unit_id, receive.clone(), 1),
                receive,
            )
            .unwrap();
        assert_eq!(first.disposition, CicsDisposition::Suspended);
        service
            .submit_input(
                &session,
                0x7d,
                &BTreeMap::from([("ACCOUNT".into(), b"00010000001".to_vec())]),
            )
            .unwrap();
        let receive = request(CicsOperation::ReceiveMap, BTreeMap::new(), 2);
        let resumed = service
            .invoke(
                &effect(&invocation.run_unit_id, receive.clone(), 2),
                receive,
            )
            .unwrap();
        assert_eq!(resumed.disposition, CicsDisposition::Complete);
        assert!(
            resumed
                .payload
                .bytes()
                .windows(7)
                .any(|value| value == b"ACCOUNT")
        );
    }

    #[test]
    fn carddemo_public_terminal_and_tn3270_share_durable_authority() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let initial = service(store.clone());
        initial
            .register_map(BmsMapDefinition {
                mapset: "COSGN00".into(),
                map: "COSGN0A".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: vec![
                    BmsFieldDefinition {
                        name: "USERID".into(),
                        row: 2,
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
                    },
                    BmsFieldDefinition {
                        name: "LOCKED".into(),
                        row: 3,
                        column: 1,
                        length: 8,
                        initial: b"PRIVATE".to_vec(),
                        color: None,
                        highlight: None,
                        protected: true,
                        secret: true,
                        fset: true,
                        justify_right: false,
                        fill_zero: false,
                        output_offset: None,
                        attribute_offset: None,
                    },
                ],
            })
            .unwrap();
        let invocation = invocation_for("public-run", BTreeMap::new());
        let session = SessionId::new("carddemo-public", 64).unwrap();
        let launched = initial
            .launch_terminal(
                invocation.clone(),
                &session,
                "CC00",
                24,
                80,
                "csrf-carddemo",
                10,
                100,
            )
            .unwrap();
        assert_eq!(launched.transaction, "CC00");
        let mut send = request(
            CicsOperation::SendMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"COSGN00")),
                ("MAP".into(), argument(b"COSGN0A")),
            ]),
            1,
        );
        send.mutation.as_mut().unwrap().transaction = Some("CC00".into());
        initial
            .invoke(&effect(&invocation.run_unit_id, send.clone(), 1), send)
            .unwrap();
        let principal = invocation.principal.id();
        let wire = initial.tn3270_screen(&session, principal, 11).unwrap();
        assert_eq!(wire[..2], [0xf5, 0xc3]);
        assert!(!wire.windows(7).any(|value| value == b"PRIVATE"));
        drop(initial);
        let initial = service(store.clone());
        initial
            .register_run(invocation.clone(), &session, "CC00", "MEAPPL", "MESYS")
            .unwrap();
        assert_eq!(
            initial.submit_terminal_input(
                &session,
                principal,
                "wrong-csrf",
                0x7d,
                &BTreeMap::new(),
                12,
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            initial.submit_terminal_input(
                &session,
                principal,
                "csrf-carddemo",
                0x7d,
                &BTreeMap::from([("LOCKED".into(), b"CHANGE".to_vec())]),
                12,
            ),
            Err(HostProblem::Unauthorized)
        );
        initial
            .submit_tn3270(
                &session,
                principal,
                "csrf-carddemo",
                &[0x7d, 0, 0, 0x11, 0, 80, b'U', b'S', b'E', b'R'],
                12,
            )
            .unwrap();
        let mut receive = request(
            CicsOperation::ReceiveMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"COSGN00")),
                ("MAP".into(), argument(b"COSGN0A")),
            ]),
            2,
        );
        receive.mutation.as_mut().unwrap().transaction = Some("CC00".into());
        let input = initial
            .invoke(
                &effect(&invocation.run_unit_id, receive.clone(), 2),
                receive,
            )
            .unwrap();
        assert!(
            input
                .payload
                .bytes()
                .windows(4)
                .any(|value| value == b"USER")
        );
        assert!(
            input
                .payload
                .bytes()
                .windows(7)
                .any(|value| value == b"PRIVATE")
        );
        assert_eq!(initial.active_worker_count(), 0);

        drop(initial);
        let restarted = service(store);
        assert_eq!(
            restarted
                .terminal_snapshot(&session, principal, 13)
                .unwrap()
                .transaction,
            "CC00"
        );
        assert_eq!(
            restarted.submit_tn3270(&session, principal, "csrf-carddemo", &[0x7d, 0], 14,),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            restarted.terminal_snapshot(
                &session,
                &PrincipalId::new("OTHER", InvocationLimits::default()).unwrap(),
                14,
            ),
            Err(HostProblem::Unauthorized)
        );
        restarted
            .resume_terminal(
                invocation_for("resumed-run", BTreeMap::new()),
                &session,
                "csrf-carddemo",
                15,
            )
            .unwrap();
        restarted
            .disconnect_terminal(&session, principal, "csrf-carddemo", 16)
            .unwrap();
        assert_eq!(
            restarted.terminal_snapshot(&session, principal, 17),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn carddemo_terminal_timeout_and_overload_fail_closed() {
        let limits = CicsLimits {
            max_sessions: 1,
            max_runs: 1,
            ..CicsLimits::default()
        };
        let service = CicsService::open(
            authorities(),
            Arc::new(MemoryStore::new(Default::default())),
            limits,
        )
        .unwrap();
        let first = invocation_for("first-terminal", BTreeMap::new());
        let first_session = SessionId::new("first-terminal", 64).unwrap();
        service
            .launch_terminal(
                first.clone(),
                &first_session,
                "CC00",
                24,
                80,
                "first-csrf",
                10,
                5,
            )
            .unwrap();
        assert_eq!(
            service.launch_terminal(
                invocation_for("second-terminal", BTreeMap::new()),
                &SessionId::new("second-terminal", 64).unwrap(),
                "CC00",
                24,
                80,
                "second-csrf",
                11,
                5,
            ),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            service.terminal_snapshot(&first_session, first.principal.id(), 15),
            Err(HostProblem::TimedOut)
        );
        assert_eq!(
            service.terminal_snapshot(&first_session, first.principal.id(), 16),
            Err(HostProblem::NotFound)
        );
        assert_eq!(service.active_worker_count(), 0);
    }

    #[test]
    fn dynamic_bms_protection_survives_session_restart() {
        let memory = Arc::new(MemoryStore::new(Default::default()));
        let store: Arc<dyn ProviderStateStore> = memory.clone();
        let initial = service(store.clone());
        initial
            .register_map(BmsMapDefinition {
                mapset: "DYNAMIC".into(),
                map: "DYNMAP".into(),
                line: 2,
                column: 3,
                rows: 23,
                columns: 78,
                fields: vec![BmsFieldDefinition {
                    name: "SELECT".into(),
                    row: 1,
                    column: 1,
                    length: 1,
                    initial: Vec::new(),
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
            })
            .unwrap();
        let invocation = invocation_for("dynamic-run", BTreeMap::new());
        let session = SessionId::new("dynamic-session", 64).unwrap();
        initial
            .launch_terminal(
                invocation.clone(),
                &session,
                "CC00",
                24,
                80,
                "dynamic-csrf",
                10,
                100,
            )
            .unwrap();
        let mut send = request(
            CicsOperation::SendMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"DYNAMIC")),
                ("MAP".into(), argument(b"DYNMAP")),
                ("FROM".into(), argument(&[b'A', 0xc1])),
            ]),
            1,
        );
        send.mutation.as_mut().unwrap().transaction = Some("CC00".into());
        initial
            .invoke(&effect(&invocation.run_unit_id, send.clone(), 1), send)
            .unwrap();
        let wire = initial
            .tn3270_screen(&session, invocation.principal.id(), 11)
            .unwrap();
        assert_eq!(&wire[..5], &[0xf5, 0xc3, 0x11, 0x00, 82]);
        let current_map = store
            .get_provider_state("cics-map", "DYNAMIC/DYNMAP")
            .unwrap()
            .unwrap();
        assert_eq!(&current_map.payload[..5], b"MECM6");
        let decoded = decode_map(&current_map.payload, CicsLimits::default()).unwrap();
        assert_eq!((decoded.line, decoded.column), (2, 3));
        let mut legacy5 = current_map.payload;
        let mut origin_at = 5usize;
        for _ in 0..2 {
            let length = usize::try_from(u32::from_be_bytes(
                legacy5[origin_at..origin_at + 4].try_into().unwrap(),
            ))
            .unwrap();
            origin_at += 4 + length;
        }
        legacy5.drain(origin_at..origin_at + 4);
        legacy5[..5].copy_from_slice(b"MECM5");
        let decoded = decode_map(&legacy5, CicsLimits::default()).unwrap();
        assert_eq!((decoded.line, decoded.column), (1, 1));
        drop(initial);
        let restarted = service(store);
        restarted
            .submit_terminal_input(
                &session,
                invocation.principal.id(),
                "dynamic-csrf",
                0x7d,
                &BTreeMap::from([("SELECT".into(), b"U".to_vec())]),
                11,
            )
            .unwrap();
    }

    /// Issue #208: runtime MAPSET values trim trailing blanks, then enforce 1-7 bytes.
    #[test]
    fn receive_map_runtime_mapset_trims_trailing_blanks_and_rejects_long_names() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        service
            .register_map(BmsMapDefinition {
                mapset: "MENUMS".into(),
                map: "MENU".into(),
                line: 1,
                column: 1,
                rows: 1,
                columns: 1,
                fields: Vec::new(),
            })
            .unwrap();
        let padded = request(
            CicsOperation::ReceiveMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"MENUMS  ")),
                ("MAP".into(), argument(b"MENU")),
            ]),
            1,
        );
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, padded.clone(), 1), padded)
                .unwrap()
                .disposition,
            CicsDisposition::Suspended
        );

        let too_long = request(
            CicsOperation::ReceiveMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"TOOLONG8")),
                ("MAP".into(), argument(b"MENU")),
            ]),
            2,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, too_long.clone(), 2),
                too_long,
            ),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn bms_send_file_read_and_program_transfer_are_typed() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        service
            .register_map(BmsMapDefinition {
                mapset: "MENUMS".into(),
                map: "MENU".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: vec![BmsFieldDefinition {
                    name: "TITLE".into(),
                    row: 1,
                    column: 1,
                    length: 8,
                    initial: b"CARDDEMO".to_vec(),
                    color: Some("BLUE".into()),
                    highlight: None,
                    protected: true,
                    secret: false,
                    fset: false,
                    justify_right: false,
                    fill_zero: false,
                    output_offset: None,
                    attribute_offset: None,
                }],
            })
            .unwrap();
        let send = request(
            CicsOperation::SendMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"MENUMS")),
                ("MAP".into(), argument(b"MENU")),
            ]),
            1,
        );
        assert!(
            service
                .invoke(&effect(&invocation.run_unit_id, send.clone(), 1), send)
                .unwrap()
                .payload
                .bytes()
                .windows(8)
                .any(|value| value == b"CARDDEMO")
        );
        let read = request(
            CicsOperation::Read,
            BTreeMap::from([("FILE".into(), argument(b"USER.CARD"))]),
            2,
        );
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, read.clone(), 2), read)
                .unwrap()
                .payload
                .bytes(),
            b"CARD0001"
        );
        let xctl = request(
            CicsOperation::Xctl,
            BTreeMap::from([("PROGRAM".into(), argument(b"COCRDLIC"))]),
            3,
        );
        let transferred = service
            .invoke(&effect(&invocation.run_unit_id, xctl.clone(), 3), xctl)
            .unwrap();
        assert_eq!(transferred.disposition, CicsDisposition::Transfer);
        assert_eq!(transferred.target.as_deref(), Some("COCRDLIC"));
    }

    #[test]
    fn purge_message_preserves_displayed_screen_and_rejects_dpl() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        let send = request(
            CicsOperation::SendText,
            BTreeMap::from([("FROM".into(), argument(b"DISPLAYED"))]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, send.clone(), 1), send)
            .unwrap();
        let before = {
            let state = service.lock().unwrap();
            let session = &state.sessions[session.as_str()];
            (
                session.screen.clone(),
                session.mapset.clone(),
                session.map.clone(),
                session.version,
            )
        };

        let purge = request(CicsOperation::PurgeMessage, BTreeMap::new(), 2);
        let response = service
            .invoke(&effect(&invocation.run_unit_id, purge.clone(), 2), purge)
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "NORMAL", 0, 0)
        );
        let after = {
            let state = service.lock().unwrap();
            let session = &state.sessions[session.as_str()];
            (
                session.screen.clone(),
                session.mapset.clone(),
                session.map.clone(),
                session.version,
            )
        };
        assert_eq!(after, before);

        let context = BoundedPayload::new(
            "mainframe-env.cics.execution-context@1",
            b"dpl-synconreturn".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let dpl = invocation_for(
            "purge-dpl",
            BTreeMap::from([("cics.execution-context".into(), context)]),
        );
        let dpl_session = SessionId::new("purge-dpl", 64).unwrap();
        service.create_session(&dpl_session, 24, 80).unwrap();
        service
            .register_run(dpl.clone(), &dpl_session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        let mut purge = request(CicsOperation::PurgeMessage, BTreeMap::new(), 3);
        purge.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let response = service
            .invoke(&effect(&dpl.run_unit_id, purge.clone(), 3), purge)
            .unwrap();
        assert_eq!(
            (
                response.disposition,
                response.condition.as_str(),
                response.response,
                response.response2,
            ),
            (CicsDisposition::Complete, "INVREQ", 16, 200)
        );
    }

    #[test]
    fn send_text_length_selects_the_requested_prefix() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, session) = registered(&service);
        let send = request(
            CicsOperation::SendText,
            BTreeMap::from([
                ("FROM".into(), argument(b"HELLOWORLD")),
                ("LENGTH".into(), cics_decimal(5)),
            ]),
            1,
        );
        let response = service
            .invoke(&effect(&invocation.run_unit_id, send.clone(), 1), send)
            .unwrap();
        assert_eq!(response.payload.bytes(), b"HELLO");
        assert_eq!(
            service.lock().unwrap().sessions[session.as_str()].screen,
            b"HELLO"
        );
    }

    #[test]
    fn write_length_persists_only_the_selected_record_prefix() {
        let persisted = Arc::new(Mutex::new(None));
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open(
            persisted_dataset_authorities(persisted.clone()),
            store,
            CicsLimits::default(),
        )
        .unwrap();
        service
            .register_file_aliases(&BTreeMap::from([(
                "TESTFILE".into(),
                DatasetName::new("IBMUSER.TESTFILE", 128).unwrap(),
            )]))
            .unwrap();
        let (invocation, _) = registered(&service);
        let write = request(
            CicsOperation::Write,
            BTreeMap::from([
                ("FILE".into(), argument(b"TESTFILE")),
                ("FROM".into(), argument(b"ABC12345")),
                ("RIDFLD".into(), argument(b"ABC")),
                ("LENGTH".into(), cics_decimal(5)),
                ("KEYLENGTH".into(), cics_decimal(3)),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, write.clone(), 1), write)
            .unwrap();
        assert_eq!(*persisted.lock().unwrap(), Some(b"ABC12".to_vec()));

        let read = request(
            CicsOperation::Read,
            BTreeMap::from([
                ("FILE".into(), argument(b"TESTFILE")),
                ("RIDFLD".into(), argument(b"ABC")),
                ("LENGTH".into(), cics_decimal(8)),
                ("KEYLENGTH".into(), cics_decimal(3)),
            ]),
            2,
        );
        let response = service
            .invoke(&effect(&invocation.run_unit_id, read.clone(), 2), read)
            .unwrap();
        assert_eq!(response.payload.bytes(), b"ABC12");
    }

    #[test]
    fn file_control_is_atomic_durable_and_enforced_by_online_io() {
        let memory = Arc::new(MemoryStore::new(Default::default()));
        let store: Arc<dyn ProviderStateStore> = memory.clone();
        let service = service(store.clone());
        service
            .register_file_aliases(&BTreeMap::from([
                (
                    "TRANSACT".into(),
                    DatasetName::new("IBMUSER.TRANSACT", 128).unwrap(),
                ),
                (
                    "ACCTDAT".into(),
                    DatasetName::new("IBMUSER.ACCTDAT", 128).unwrap(),
                ),
            ]))
            .unwrap();
        let (invocation, _) = registered(&service);
        let status = |value: &[u8]| {
            BoundedPayload::new(
                "mainframe-env.cics.file-status@1",
                value.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let invalid = request(
            CicsOperation::SetFileStatus,
            BTreeMap::from([
                ("TRANSACT".into(), status(b"CLOSED")),
                ("UNKNOWN".into(), status(b"CLOSED")),
            ]),
            1,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, invalid.clone(), 1),
                invalid
            ),
            Err(HostProblem::NotFound)
        );
        assert_eq!(service.file_status("TRANSACT"), Ok(CicsFileStatus::Open));

        let close = request(
            CicsOperation::SetFileStatus,
            BTreeMap::from([
                ("TRANSACT".into(), status(b"CLOSED")),
                ("ACCTDAT".into(), status(b"CLOSED")),
            ]),
            2,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, close.clone(), 2), close)
            .unwrap();
        assert_eq!(service.file_status("TRANSACT"), Ok(CicsFileStatus::Closed));
        let read = request(
            CicsOperation::Read,
            BTreeMap::from([("FILE".into(), argument(b"TRANSACT"))]),
            3,
        );
        assert_eq!(
            service.invoke(&effect(&invocation.run_unit_id, read.clone(), 3), read),
            Err(HostProblem::Condition {
                name: "NOTOPEN".into(),
                response: 19,
                response2: 60,
            })
        );

        let enable_closed = request(
            CicsOperation::SetFileStatus,
            BTreeMap::from([("TRANSACT".into(), status(b"CLOSED-ENABLED"))]),
            4,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, enable_closed.clone(), 4),
                enable_closed,
            )
            .unwrap();
        let auto_open_read = request(
            CicsOperation::Read,
            BTreeMap::from([("FILE".into(), argument(b"TRANSACT"))]),
            5,
        );
        assert_ne!(
            service.invoke(
                &effect(&invocation.run_unit_id, auto_open_read.clone(), 5),
                auto_open_read
            ),
            Err(HostProblem::Condition {
                name: "NOTOPEN".into(),
                response: 19,
                response2: 60,
            })
        );
        assert_eq!(service.file_status("TRANSACT"), Ok(CicsFileStatus::Open));

        let disable = request(
            CicsOperation::SetFileStatus,
            BTreeMap::from([("TRANSACT".into(), status(b"DISABLED"))]),
            6,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, disable.clone(), 6),
                disable,
            )
            .unwrap();
        let disabled_read = request(
            CicsOperation::Read,
            BTreeMap::from([("FILE".into(), argument(b"TRANSACT"))]),
            7,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, disabled_read.clone(), 7),
                disabled_read
            ),
            Err(HostProblem::Condition {
                name: "DISABLED".into(),
                response: 84,
                response2: 0,
            })
        );

        let close_again = request(
            CicsOperation::SetFileStatus,
            BTreeMap::from([("TRANSACT".into(), status(b"CLOSED-UNENABLED"))]),
            8,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, close_again.clone(), 8),
                close_again,
            )
            .unwrap();
        drop(service);
        let restarted = CicsService::open(authorities(), store, CicsLimits::default()).unwrap();
        assert_eq!(
            restarted.file_status("TRANSACT"),
            Ok(CicsFileStatus::Closed)
        );
    }

    /// Issue #204: typed GTEQ retains STARTBR's greater-or-equal positioning.
    #[test]
    fn carddemo_cics_browse_rewrite_and_delete_reuse_base_record_identity() {
        let trace = Arc::new(DatasetTrace::default());
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service =
            CicsService::open(traced_authorities(trace.clone()), store, Default::default())
                .unwrap();
        let (invocation, _) = registered(&service);
        service
            .register_file_aliases(&BTreeMap::from([(
                "CARDDAT".into(),
                DatasetName::new("CARDDEMO.CARDDAT", 128).unwrap(),
            )]))
            .unwrap();
        let start = request(
            CicsOperation::StartBrowse,
            BTreeMap::from([
                ("DATASET".into(), argument(b"CARDDAT")),
                ("RIDFLD".into(), argument(b"AA")),
                ("OPTION.GTEQ".into(), argument(b"")),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, start.clone(), 1), start)
            .unwrap();
        let next = request(
            CicsOperation::ReadNext,
            BTreeMap::from([
                ("DATASET".into(), argument(b"CARDDAT")),
                ("OPTION.UPDATE".into(), argument(b"")),
            ]),
            2,
        );
        let browsed = service
            .invoke(&effect(&invocation.run_unit_id, next.clone(), 2), next)
            .unwrap();
        assert_eq!(browsed.payload.bytes(), b"AA11");
        assert_eq!(browsed.outputs["RIDFLD"].bytes(), b"AA");
        let rewrite = request(
            CicsOperation::Rewrite,
            BTreeMap::from([
                ("DATASET".into(), argument(b"CARDDAT")),
                ("FROM".into(), argument(b"AA22")),
            ]),
            3,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, rewrite.clone(), 3),
                rewrite,
            )
            .unwrap();
        let delete = request(
            CicsOperation::Delete,
            BTreeMap::from([
                ("DATASET".into(), argument(b"CARDDAT")),
                ("RIDFLD".into(), argument(b"AA")),
            ]),
            4,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, delete.clone(), 4), delete)
            .unwrap();
        let end = request(
            CicsOperation::EndBrowse,
            BTreeMap::from([("DATASET".into(), argument(b"CARDDAT"))]),
            5,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, end.clone(), 5), end)
            .unwrap();

        let requests = trace.requests.lock().unwrap();
        assert!(matches!(
            &requests[0],
            DatasetRequest::StartBrowse {
                dataset,
                relation: mainframe_env_host_api::KeyRelation::GreaterOrEqual,
                ..
            } if dataset.as_str() == "CARDDEMO.CARDDAT"
        ));
        assert!(matches!(
            &requests[1],
            DatasetRequest::ReadNext { cursor, .. } if cursor == "CURSOR-1"
        ));
        assert!(matches!(
            &requests[2],
            DatasetRequest::RewriteRecord { key, record, .. }
                if key == b"AA" && record == b"AA22"
        ));
        assert!(matches!(
            &requests[3],
            DatasetRequest::DeleteRecord { key, .. } if key == b"AA"
        ));
        assert!(matches!(
            &requests[4],
            DatasetRequest::EndBrowse { cursor, .. } if cursor == "CURSOR-1"
        ));
        drop(requests);
        let origins = trace.origins.lock().unwrap();
        assert_eq!(origins.len(), 2);
        assert_eq!(origins[0].1, "outer-3");
        assert_eq!(origins[1].1, "outer-4");
    }

    /// Issue #205: current-record DELETE consumes exactly one READ UPDATE hold.
    #[test]
    fn delete_without_ridfld_uses_and_releases_latest_read_update_hold() {
        let trace = Arc::new(DatasetTrace::default());
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service =
            CicsService::open(traced_authorities(trace.clone()), store, Default::default())
                .unwrap();
        let (invocation, _) = registered(&service);
        service
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("CARDDEMO.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        let read = request(
            CicsOperation::Read,
            BTreeMap::from([
                ("FILE".into(), argument(b"ACCTDAT")),
                ("RIDFLD".into(), argument(b"AA")),
                ("OPTION.UPDATE".into(), argument(b"")),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, read.clone(), 1), read)
            .unwrap();
        let delete = request(
            CicsOperation::Delete,
            BTreeMap::from([("FILE".into(), argument(b"ACCTDAT"))]),
            2,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, delete.clone(), 2), delete)
            .unwrap();

        let second = request(
            CicsOperation::Delete,
            BTreeMap::from([("FILE".into(), argument(b"ACCTDAT"))]),
            3,
        );
        assert_eq!(
            service.invoke(&effect(&invocation.run_unit_id, second.clone(), 3), second),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 31,
            })
        );
        assert!(matches!(
            trace.requests.lock().unwrap().as_slice(),
            [
                DatasetRequest::Read { .. },
                DatasetRequest::DeleteRecord { key, .. }
            ] if key == b"AA"
        ));
    }

    #[test]
    fn syncpoint_rollback_compensates_reached_dataset_rewrite() {
        let trace = Arc::new(DatasetTrace::default());
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service =
            CicsService::open(traced_authorities(trace.clone()), store, Default::default())
                .unwrap();
        let (invocation, _) = registered(&service);
        service
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("CARDDEMO.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        let read = request(
            CicsOperation::Read,
            BTreeMap::from([
                ("DATASET".into(), argument(b"ACCTDAT")),
                ("RIDFLD".into(), argument(b"AA")),
                ("OPTION.UPDATE".into(), argument(b"")),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, read.clone(), 1), read)
            .unwrap();
        let rewrite = request(
            CicsOperation::Rewrite,
            BTreeMap::from([
                ("DATASET".into(), argument(b"ACCTDAT")),
                ("FROM".into(), argument(b"AA22")),
            ]),
            2,
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, rewrite.clone(), 2),
                rewrite,
            )
            .unwrap();
        let rollback = request(
            CicsOperation::Syncpoint,
            BTreeMap::from([("OPTION.ROLLBACK".into(), argument(b""))]),
            3,
        );
        assert_eq!(
            service
                .invoke(
                    &effect(&invocation.run_unit_id, rollback.clone(), 3),
                    rollback,
                )
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::RolledBack)
        );
        let requests = trace.requests.lock().unwrap();
        assert!(matches!(
            requests.as_slice(),
            [
                DatasetRequest::Read { .. },
                DatasetRequest::RewriteRecord { record, .. },
                DatasetRequest::RewriteRecord {
                    record: restored,
                    ..
                }
            ] if record == b"AA22" && restored == b"AA11"
        ));
    }

    #[test]
    fn plain_read_does_not_authorize_rewrite_update_context() {
        let trace = Arc::new(DatasetTrace::default());
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service =
            CicsService::open(traced_authorities(trace.clone()), store, Default::default())
                .unwrap();
        let (invocation, _) = registered(&service);
        service
            .register_file_aliases(&BTreeMap::from([(
                "ACCTDAT".into(),
                DatasetName::new("CARDDEMO.ACCTDAT", 128).unwrap(),
            )]))
            .unwrap();
        let read = request(
            CicsOperation::Read,
            BTreeMap::from([
                ("DATASET".into(), argument(b"ACCTDAT")),
                ("RIDFLD".into(), argument(b"AA")),
            ]),
            1,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, read.clone(), 1), read)
            .unwrap();
        let rewrite = request(
            CicsOperation::Rewrite,
            BTreeMap::from([
                ("DATASET".into(), argument(b"ACCTDAT")),
                ("FROM".into(), argument(b"AA22")),
            ]),
            2,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, rewrite.clone(), 2),
                rewrite,
            ),
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 30,
            })
        );
        assert!(matches!(
            trace.requests.lock().unwrap().as_slice(),
            [DatasetRequest::Read { .. }]
        ));
    }

    #[test]
    fn outer_registry_selects_cics_provider_for_clocked_operations() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let outer = ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![cics_provider(service, InvocationLimits::default())],
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            HostLimits::default(),
        );
        let assign = request(CicsOperation::Assign, BTreeMap::new(), 1);
        let (selected, _) = outer
            .invoke(
                &invocation,
                1,
                false,
                effect(&invocation.run_unit_id, assign, 1),
            )
            .into_transaction_parts();
        assert!(matches!(selected.outcome, Ok(HostResult::Cics(_))));
        let asktime = request(CicsOperation::Asktime, BTreeMap::new(), 2);
        assert!(matches!(
            outer
                .invoke(
                    &invocation,
                    1,
                    false,
                    effect(&invocation.run_unit_id, asktime, 2),
                )
                .into_transaction_parts()
                .0
                .outcome,
            Ok(HostResult::Cics(CicsResponse { outputs, .. }))
                if outputs.contains_key("ABSTIME")
        ));
        let bare_asktime = request(CicsOperation::AsktimeEib, BTreeMap::new(), 3);
        assert!(matches!(
            outer
                .invoke(
                    &invocation,
                    1,
                    false,
                    effect(&invocation.run_unit_id, bare_asktime, 3),
                )
                .into_transaction_parts()
                .0
                .outcome,
            Ok(HostResult::Cics(CicsResponse { outputs, .. }))
                if !outputs.contains_key("ABSTIME")
                    && outputs["EIBDATE"].bytes() == b"126242"
                    && outputs["EIBTIME"].bytes() == b"123456"
        ));
    }

    #[test]
    fn sqlite_reopen_retains_suspended_session() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cics.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let session = SessionId::new("session", 64).unwrap();
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65536).unwrap());
            let service = service(store);
            service.create_session(&session, 24, 80).unwrap();
            let invocation = invocation();
            service
                .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
            let receive = request(CicsOperation::ReceiveMap, BTreeMap::new(), 1);
            assert_eq!(
                service
                    .invoke(
                        &effect(&invocation.run_unit_id, receive.clone(), 1),
                        receive
                    )
                    .unwrap()
                    .disposition,
                CicsDisposition::Suspended
            );
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65536).unwrap());
            let service = service(store);
            service
                .register_run(invocation(), &session, "MENU", "MEAPPL", "MESYS")
                .unwrap();
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn transaction_mismatch_and_limits_fail_closed() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        let mut abend = request(CicsOperation::Abend, BTreeMap::new(), 1);
        abend.mutation.as_mut().unwrap().transaction = Some("OTHER".into());
        assert_eq!(
            service.invoke(&effect(&invocation.run_unit_id, abend.clone(), 1), abend),
            Err(HostProblem::IdempotencyConflict)
        );
        let invalid = BmsMapDefinition {
            mapset: "M".into(),
            map: "X".into(),
            line: 1,
            column: 1,
            rows: 1,
            columns: 1,
            fields: vec![BmsFieldDefinition {
                name: "F".into(),
                row: 2,
                column: 1,
                length: 1,
                initial: Vec::new(),
                color: None,
                highlight: None,
                protected: false,
                secret: true,
                fset: false,
                justify_right: false,
                fill_zero: false,
                output_offset: None,
                attribute_offset: None,
            }],
        };
        assert_eq!(service.register_map(invalid), Err(HostProblem::Malformed));

        service
            .register_map(BmsMapDefinition {
                mapset: "M".into(),
                map: "TOOBIG".into(),
                line: 2,
                column: 1,
                rows: 24,
                columns: 80,
                fields: Vec::new(),
            })
            .unwrap();
        let mut send = request(
            CicsOperation::SendMap,
            BTreeMap::from([
                ("MAPSET".into(), argument(b"M")),
                ("MAP".into(), argument(b"TOOBIG")),
            ]),
            2,
        );
        send.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let rejected = service
            .invoke(&effect(&invocation.run_unit_id, send.clone(), 2), send)
            .unwrap();
        assert_eq!(
            (
                rejected.condition.as_str(),
                rejected.response,
                rejected.response2
            ),
            ("INVMPSZ", 38, 0)
        );
    }

    #[test]
    fn racf_default_deny_is_enforced_at_cics_admission() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:ibmuser", b"PASSWORD".to_vec());
        let racf = RacfService::open(store.clone(), secrets, Default::default()).unwrap();
        racf.add_user(
            "IBMUSER",
            &SecretRef::new("secret:ibmuser", HostLimits::default()).unwrap(),
        )
        .unwrap();
        let inner = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    racf_providers(racf.clone(), InvocationLimits::default()),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            HostLimits::default(),
        ));
        let service = CicsService::open(inner, store, Default::default()).unwrap();
        let (invocation, _) = registered(&service);
        let assign = request(CicsOperation::Assign, BTreeMap::new(), 1);
        assert_eq!(
            service.invoke(&effect(&invocation.run_unit_id, assign.clone(), 1), assign),
            Err(HostProblem::Unauthorized)
        );
        racf.define_profile(
            "TCICSTRN",
            "CICS.MENU",
            "IBMUSER",
            Some(AccessIntent::Execute),
        )
        .unwrap();
        let assign = request(CicsOperation::Assign, BTreeMap::new(), 2);
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, assign.clone(), 2), assign)
                .unwrap()
                .condition,
            "NORMAL"
        );
    }

    #[test]
    fn uow_retention_descriptors_are_attributed_exact_and_restart_safe() {
        use crate::retention::{
            CicsUowCodecVersion, CicsUowDependencyState, CicsUowState, CicsUowValidationError,
            describe_cics_uow_row,
        };

        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let initial = service(store.clone());
        let (invocation, session) = registered(&initial);

        let commit = request(CicsOperation::Syncpoint, BTreeMap::new(), 71);
        assert_eq!(
            initial
                .invoke(
                    &effect(&invocation.run_unit_id, commit.clone(), 71),
                    commit.clone()
                )
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );
        let committed = store
            .get_provider_state("cics-uow", "outer-71")
            .unwrap()
            .unwrap();
        let descriptor = describe_cics_uow_row(&committed, None).unwrap();
        assert_eq!(descriptor.codec, CicsUowCodecVersion::RetentionV2);
        assert_eq!(descriptor.state, CicsUowState::Committed);
        assert_eq!(descriptor.owner_execution.as_deref(), Some("execution-run"));
        assert_eq!(descriptor.owner_run_unit.as_deref(), Some("run"));
        assert_eq!(descriptor.terminal_tick, None);
        assert_eq!(
            descriptor.dependency,
            CicsUowDependencyState::TerminalObservationRequired
        );

        let rollback = request(
            CicsOperation::Syncpoint,
            BTreeMap::from([("OPTION.ROLLBACK".into(), argument(b""))]),
            72,
        );
        initial
            .invoke(
                &effect(&invocation.run_unit_id, rollback.clone(), 72),
                rollback,
            )
            .unwrap();
        let rolled_back = store
            .get_provider_state("cics-uow", "outer-72")
            .unwrap()
            .unwrap();
        assert_eq!(
            describe_cics_uow_row(&rolled_back, None).unwrap().state,
            CicsUowState::RolledBack
        );

        drop(initial);
        let restarted = service(store.clone());
        restarted
            .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
            .unwrap();
        assert_eq!(
            restarted
                .invoke(&effect(&invocation.run_unit_id, commit.clone(), 71), commit)
                .unwrap()
                .unit_of_work,
            Some(CicsUnitOfWorkOutcome::Committed)
        );
        assert_eq!(
            describe_cics_uow_row(
                &store
                    .get_provider_state("cics-uow", "outer-71")
                    .unwrap()
                    .unwrap(),
                None,
            )
            .unwrap(),
            descriptor
        );

        let active = ProviderStateRecord {
            namespace: "cics-uow".into(),
            key: "active-key".into(),
            version: 1,
            payload: encode_uow(&UowRecord {
                finalized: false,
                outcome: CicsUnitOfWorkOutcome::Committed,
                transaction: "MENU".into(),
                metadata: Some(UowRetentionMetadata {
                    effect_key: "active-key".into(),
                    owner_execution: "execution-active".into(),
                    owner_run_unit: "run-active".into(),
                    deadline_tick: 200,
                    terminal_tick: None,
                }),
            })
            .unwrap(),
        };
        assert_eq!(
            describe_cics_uow_row(&active, None).unwrap().dependency,
            CicsUowDependencyState::Active
        );
        store.put_provider_state(active, None).unwrap();
        let active_key = IdempotencyKey::new("active-key", InvocationLimits::default()).unwrap();
        restarted
            .reconcile_unit_of_work(&active_key, CicsUnitOfWorkOutcome::Committed)
            .unwrap();
        let unobserved = store
            .get_provider_state("cics-uow", active_key.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            describe_cics_uow_row(&unobserved, None).unwrap().dependency,
            CicsUowDependencyState::TerminalObservationRequired
        );
        let finalized_with_undo = ProviderStateRecord {
            namespace: "cics-uow".into(),
            key: "undo-key".into(),
            version: 2,
            payload: encode_uow(&UowRecord {
                finalized: true,
                outcome: CicsUnitOfWorkOutcome::RolledBack,
                transaction: "MENU".into(),
                metadata: Some(UowRetentionMetadata {
                    effect_key: "undo-key".into(),
                    owner_execution: "execution-undo".into(),
                    owner_run_unit: "run-undo".into(),
                    deadline_tick: 300,
                    terminal_tick: Some(300),
                }),
            })
            .unwrap(),
        };
        let undo = ProviderStateRecord {
            namespace: "cics-uow-undo".into(),
            key: "run-undo".into(),
            version: 1,
            payload: encode_undo(&[DatasetUndo::Delete {
                dataset: DatasetName::new("USER.DATA", 128).unwrap(),
                key: b"AA".to_vec(),
            }])
            .unwrap(),
        };
        assert_eq!(
            describe_cics_uow_row(&finalized_with_undo, Some(&undo))
                .unwrap()
                .dependency,
            CicsUowDependencyState::UndoLogPresent
        );
        let mut mismatched_undo = undo.clone();
        mismatched_undo.key = "another-run".into();
        assert_eq!(
            describe_cics_uow_row(&finalized_with_undo, Some(&mismatched_undo)),
            Err(CicsUowValidationError::MismatchedUndo)
        );
        let mut wrong_version_undo = undo.clone();
        wrong_version_undo.version = 2;
        assert_eq!(
            crate::describe_cics_undo_row(&wrong_version_undo),
            Err(CicsUowValidationError::CorruptPayload)
        );

        let legacy = ProviderStateRecord {
            namespace: "cics-uow".into(),
            key: "legacy-key".into(),
            version: 2,
            payload: encode_uow(&UowRecord {
                finalized: true,
                outcome: CicsUnitOfWorkOutcome::Committed,
                transaction: "MENU".into(),
                metadata: None,
            })
            .unwrap(),
        };
        assert_eq!(
            describe_cics_uow_row(&legacy, None).unwrap().dependency,
            CicsUowDependencyState::LegacyTerminal
        );
        let mut unsupported_version = committed.clone();
        unsupported_version.version = 3;
        assert_eq!(
            describe_cics_uow_row(&unsupported_version, None),
            Err(CicsUowValidationError::CorruptPayload)
        );
        let mut corrupt = committed;
        corrupt.payload.push(0);
        assert_eq!(
            describe_cics_uow_row(&corrupt, None),
            Err(CicsUowValidationError::CorruptPayload)
        );
        let mut invalid_version = rolled_back;
        invalid_version.version = 1;
        assert_eq!(
            describe_cics_uow_row(&invalid_version, None),
            Err(CicsUowValidationError::CorruptPayload)
        );
    }

    #[test]
    fn outer_replay_clock_failure_recovers_once_without_sliding_and_codec_is_strict() {
        use crate::retention::{
            CicsReplayRetentionState, CicsReplayValidationError, describe_cics_replay_row,
        };

        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(TestCicsClock::fixed(250));
        clock.fail_next.store(true, Ordering::SeqCst);
        let service = CicsService::open_with_replay_clock(
            authorities(),
            store.clone(),
            CicsLimits::default(),
            clock.clone(),
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let request = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"CLOCK")),
                ("FROM".into(), argument(b"ONCE")),
            ]),
            81,
        );
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), 81),
                request.clone(),
            ),
            Err(HostProblem::UnknownOutcome)
        );
        let pending = store
            .get_provider_state("cics-effect-replay-v1", "outer-81")
            .unwrap()
            .unwrap();
        assert_eq!(
            describe_cics_replay_row(&pending, None, CicsLimits::default())
                .unwrap()
                .retention,
            CicsReplayRetentionState::PendingProtected
        );
        let response = service
            .invoke(
                &effect(&invocation.run_unit_id, request.clone(), 81),
                request.clone(),
            )
            .unwrap();
        let terminal = store
            .get_provider_state("cics-effect-replay-v1", "outer-81")
            .unwrap()
            .unwrap();
        let effect_record = completed_cics_effect(&invocation, &request, &response, 81);
        let descriptor =
            describe_cics_replay_row(&terminal, Some(&effect_record), CicsLimits::default())
                .unwrap();
        assert_eq!(descriptor.retention, CicsReplayRetentionState::Terminal);
        assert_eq!(descriptor.resolution_tick, Some(250));
        clock.tick.store(900, Ordering::SeqCst);
        service
            .invoke(
                &effect(&invocation.run_unit_id, request.clone(), 81),
                request.clone(),
            )
            .unwrap();
        assert_eq!(
            store
                .get_provider_state("cics-effect-replay-v1", "outer-81")
                .unwrap()
                .unwrap(),
            terminal
        );

        let mut copied = terminal.clone();
        copied.key = "outer-82".into();
        assert_eq!(
            describe_cics_replay_row(&copied, None, CicsLimits::default()),
            Err(CicsReplayValidationError::CorruptPayload)
        );
        let mut trailing = terminal.clone();
        trailing.payload.push(0);
        assert_eq!(
            describe_cics_replay_row(&trailing, None, CicsLimits::default()),
            Err(CicsReplayValidationError::CorruptPayload)
        );

        let mut replay =
            decode_cics_effect_replay(&terminal.payload, CicsLimits::default()).unwrap();
        replay.response.outputs.insert(
            "DUP".into(),
            BoundedPayload::new("test@1", b"value".to_vec(), InvocationLimits::default()).unwrap(),
        );
        replay.result_digest =
            Some(canonical_result_digest(&Ok(HostResult::Cics(replay.response.clone()))).unwrap());
        replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
        let mut duplicate = encode_cics_effect_replay(&replay).unwrap();
        let marker = [0, 0, 0, 3, b'D', b'U', b'P'];
        let field_at = duplicate
            .windows(marker.len())
            .position(|window| window == marker)
            .unwrap();
        let count_at = field_at - 4;
        duplicate[count_at..field_at].copy_from_slice(&2_u32.to_be_bytes());
        let entry = duplicate[field_at..duplicate.len() - 1].to_vec();
        let insert_at = duplicate.len() - 1;
        duplicate.splice(insert_at..insert_at, entry);
        let duplicate = ProviderStateRecord {
            payload: duplicate,
            ..terminal.clone()
        };
        assert_eq!(
            describe_cics_replay_row(&duplicate, None, CicsLimits::default()),
            Err(CicsReplayValidationError::CorruptPayload)
        );
        assert_eq!(
            describe_cics_replay_row(
                &ProviderStateRecord {
                    payload: encode_cics_effect_replay(&replay).unwrap(),
                    ..terminal
                },
                None,
                CicsLimits {
                    max_fields: 0,
                    ..CicsLimits::default()
                },
            ),
            Err(CicsReplayValidationError::CorruptPayload)
        );
    }

    #[test]
    fn outer_replay_metadata_cas_failure_is_unknown_and_retry_recovers() {
        let store = Arc::new(FailCicsReplayCasStore::new());
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let service = CicsService::open_with_replay_clock(
            authorities(),
            provider_store,
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(300)),
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let request = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"CAS")),
                ("FROM".into(), argument(b"ONCE")),
            ]),
            82,
        );
        store.fail_next.store(true, Ordering::SeqCst);
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), 82),
                request.clone(),
            ),
            Err(HostProblem::UnknownOutcome)
        );
        let pending = store
            .get_provider_state("cics-effect-replay-v1", "outer-82")
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::describe_cics_replay_row(&pending, None, CicsLimits::default())
                .unwrap()
                .retention,
            crate::CicsReplayRetentionState::PendingProtected
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, request.clone(), 82),
                request,
            )
            .unwrap();
        let terminal = store
            .get_provider_state("cics-effect-replay-v1", "outer-82")
            .unwrap()
            .unwrap();
        assert_eq!(terminal.version, 2);
    }

    #[test]
    fn durable_clock_ages_uow_after_nested_syncpoint_completion() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open_with_replay_clock(
            authorities(),
            store.clone(),
            CicsLimits::default(),
            Arc::new(TestCicsClock::fixed(500)),
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let request = request(CicsOperation::Syncpoint, BTreeMap::new(), 91);
        service
            .invoke(
                &effect(&invocation.run_unit_id, request.clone(), 91),
                request,
            )
            .unwrap();
        let descriptor = crate::describe_cics_uow_row(
            &store
                .get_provider_state("cics-uow", "outer-91")
                .unwrap()
                .unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(descriptor.terminal_tick, Some(500));
        assert_eq!(descriptor.dependency, crate::CicsUowDependencyState::Clear);
    }

    #[test]
    fn uow_clock_failure_stays_pending_until_exact_reconciliation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let clock = Arc::new(TestCicsClock::fixed(550));
        clock.fail_next.store(true, Ordering::SeqCst);
        let service = CicsService::open_with_replay_clock(
            authorities(),
            store.clone(),
            CicsLimits::default(),
            clock,
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let request = request(CicsOperation::Syncpoint, BTreeMap::new(), 93);
        assert_eq!(
            service.invoke(
                &effect(&invocation.run_unit_id, request.clone(), 93),
                request.clone(),
            ),
            Err(HostProblem::UnknownOutcome)
        );
        let key = IdempotencyKey::new("outer-93", InvocationLimits::default()).unwrap();
        let pending = store
            .get_provider_state("cics-uow", key.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(pending.version, 1);
        assert_eq!(
            crate::describe_cics_uow_row(&pending, None)
                .unwrap()
                .dependency,
            crate::CicsUowDependencyState::Active
        );
        service
            .invoke(
                &effect(&invocation.run_unit_id, request.clone(), 93),
                request,
            )
            .unwrap();
        let terminal = store
            .get_provider_state("cics-uow", key.as_str())
            .unwrap()
            .unwrap();
        let descriptor = crate::describe_cics_uow_row(&terminal, None).unwrap();
        assert_eq!(terminal.version, 2);
        assert_eq!(descriptor.terminal_tick, Some(550));
    }

    #[test]
    fn every_enterprise_syncpoint_dispatch_carries_nested_and_outer_attestation() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = CicsService::open(
            syncpoint_origin_authorities(seen.clone()),
            store,
            CicsLimits::default(),
        )
        .unwrap();
        let (invocation, _) = registered(&service);
        let first = request(CicsOperation::Syncpoint, BTreeMap::new(), 92);
        service
            .invoke(&effect(&invocation.run_unit_id, first.clone(), 92), first)
            .unwrap();
        let second = request(CicsOperation::Syncpoint, BTreeMap::new(), 94);
        service
            .invoke(&effect(&invocation.run_unit_id, second.clone(), 94), second)
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 6);
        assert_eq!(
            seen[0],
            (
                "host.db2.write".into(),
                "cics:run:2".into(),
                "outer-92".into()
            )
        );
        assert_eq!(
            seen[1],
            (
                "host.ims.write".into(),
                "cics:run:3".into(),
                "outer-92".into()
            )
        );
        assert_eq!(
            seen[2],
            (
                "host.mq.write".into(),
                "cics:run:4".into(),
                "outer-92".into()
            )
        );
        assert!(
            seen[3..]
                .iter()
                .all(|(_, nested, outer)| nested.starts_with("cics:run:") && outer == "outer-94")
        );
    }

    #[test]
    fn external_reserved_origins_are_rejected_and_internal_insertion_is_bounded() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store);
        let mut malicious = invocation();
        malicious.bindings.insert(
            CICS_NESTED_EFFECT_ORIGIN_BINDING.into(),
            BoundedPayload::new(
                CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                b"cics:run:1".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let session = SessionId::new("reserved-session", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        assert_eq!(
            service.register_run(malicious, &session, "MENU", "MEAPPL", "MESYS"),
            Err(HostProblem::Malformed)
        );

        let limits = InvocationLimits::default();
        let mut full = invocation();
        for index in 0..(limits.max_bindings - 1) {
            full.bindings.insert(
                format!("binding-{index}"),
                BoundedPayload::new("test@1", Vec::new(), limits).unwrap(),
            );
        }
        assert_eq!(
            invocation_with_nested_origin(
                &full,
                &IdempotencyKey::new("cics:run:1", limits).unwrap(),
                "outer-1",
            ),
            Err(HostProblem::ResourceExhausted)
        );
    }
}
