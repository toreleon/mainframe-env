use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
    RunUnitId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsConditionPolicy, CicsDisposition, CicsOperation,
    CicsRequest, CicsResponse, CicsUnitOfWorkOutcome, ClockRequest, DatasetName, DatasetRequest,
    DatasetResult, Db2Operation, Db2Request, EffectRequest, EffectResult, HostProblem,
    HostProvider, HostRequest, HostResult, ImsOperation, ImsRequest, MemberName, MqOperation,
    MqRequest, Mutation, ProgramName, ProgramRequest, ResourceName, ScopedHostService,
    SecurityDecision, SecurityRequest, SessionId,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsLimits {
    pub max_sessions: usize,
    pub max_runs: usize,
    pub max_maps: usize,
    pub max_programs: usize,
    pub max_file_aliases: usize,
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

#[derive(Clone, Debug)]
struct Session {
    rows: u16,
    columns: u16,
    principal: String,
    transaction: String,
    run_unit: String,
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
    session: String,
    transaction: String,
    applid: String,
    sysid: String,
    host_sequence: u64,
    handlers: BTreeMap<String, String>,
    abend_handler: Option<String>,
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
    continuations: BTreeMap<String, DurableContinuation>,
    transient: BTreeMap<String, TransientQueue>,
    transient_bytes: usize,
    #[cfg(feature = "fault-injection")]
    file_failure: Option<(CicsOperation, String)>,
    #[cfg(feature = "fault-injection")]
    file_fault: Option<(CicsOperation, String, CicsFileFaultPoint)>,
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
        Ok(Arc::new(Self {
            host,
            store,
            limits,
            state: Mutex::new(State {
                sessions,
                runs: BTreeMap::new(),
                maps,
                programs,
                file_aliases,
                file_statuses,
                continuations,
                transient,
                transient_bytes,
                #[cfg(feature = "fault-injection")]
                file_failure: None,
                #[cfg(feature = "fault-injection")]
                file_fault: None,
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
            version: 1,
        };
        let run = run_for(
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
        let run = state.runs.get(&run_id).ok_or(HostProblem::NotFound)?;
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

    pub fn complete_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut state = self.lock()?;
        let run = state.runs.get(&run_id).ok_or(HostProblem::NotFound)?;
        if run.session != session.as_str() || run.invocation.principal.id() != principal {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(current) = state.continuations.get(session.as_str()).cloned()
            && current.claimed_by.as_deref() == Some(run_id.as_str())
        {
            let mut released = current.clone();
            released.version = released
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            released.claimed_by = None;
            self.persist_continuation(session.as_str(), &released, Some(current.version))?;
            state
                .continuations
                .insert(session.as_str().into(), released);
        }
        state.runs.remove(&run_id);
        Ok(())
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
        if !valid_aid(aid) || fields.len() > self.limits.max_fields {
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
        state.runs.insert(
            invocation.run_unit_id.clone(),
            Run {
                invocation,
                session: session.as_str().into(),
                transaction: transaction.to_ascii_uppercase(),
                applid: applid.to_ascii_uppercase(),
                sysid: sysid.to_ascii_uppercase(),
                host_sequence: 0,
                handlers: BTreeMap::new(),
                abend_handler: None,
                retrieve,
                current_records: BTreeMap::new(),
                current_record_values: BTreeMap::new(),
                undo,
                undo_version,
                browses: BTreeMap::new(),
                trace: Vec::new(),
            },
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
        state.runs.insert(
            invocation.run_unit_id.clone(),
            Run {
                invocation,
                session: session.as_str().into(),
                transaction: next.transaction,
                applid: applid.to_ascii_uppercase(),
                sysid: sysid.to_ascii_uppercase(),
                host_sequence: 0,
                handlers: BTreeMap::new(),
                abend_handler: None,
                retrieve: Vec::new(),
                current_records: BTreeMap::new(),
                current_record_values: BTreeMap::new(),
                undo,
                undo_version,
                browses: BTreeMap::new(),
                trace: Vec::new(),
            },
        );
        Ok(continuation)
    }

    fn ensure_run(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        {
            let state = self.lock()?;
            if state.runs.contains_key(&invocation.run_unit_id) {
                return Ok(());
            }
        }
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

    pub fn file_status(&self, name: &str) -> Result<CicsFileStatus, HostProblem> {
        let name = normalize_terminal_name(name, 16)?;
        let state = self.lock()?;
        if !state.file_aliases.contains_key(&name) {
            return Err(HostProblem::NotFound);
        }
        Ok(state
            .file_statuses
            .get(&name)
            .map_or(CicsFileStatus::Open, |record| record.status))
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
        let mut run = self
            .lock()?
            .runs
            .remove(&effect.run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        let operation = request.operation;
        let result = self.invoke_run(&mut run, request);
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

    fn invoke_run(&self, run: &mut Run, request: CicsRequest) -> Result<CicsResponse, HostProblem> {
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
        match request.operation {
            CicsOperation::HandleCondition => {
                let (condition, label) = request
                    .arguments
                    .iter()
                    .find(|(name, _)| !name.starts_with("OPTION."))
                    .map(|(name, value)| {
                        (
                            name.to_ascii_uppercase(),
                            String::from_utf8_lossy(value.bytes()).into_owned(),
                        )
                    })
                    .ok_or(HostProblem::Malformed)?;
                run.handlers.insert(condition, label);
                self.response(
                    run,
                    CicsDisposition::Complete,
                    "NORMAL",
                    0,
                    0,
                    None,
                    None,
                    Vec::new(),
                )
            }
            CicsOperation::HandleAbend => {
                if request.arguments.contains_key("OPTION.CANCEL") {
                    run.abend_handler = None;
                } else {
                    run.abend_handler = Some(argument_text(&request, "LABEL")?);
                }
                self.response(
                    run,
                    CicsDisposition::Complete,
                    "NORMAL",
                    0,
                    0,
                    None,
                    None,
                    Vec::new(),
                )
            }
            CicsOperation::Assign => {
                let mut response = self.response(
                    run,
                    CicsDisposition::Complete,
                    "NORMAL",
                    0,
                    0,
                    None,
                    None,
                    Vec::new(),
                )?;
                for (name, value) in [
                    ("APPLID", run.applid.as_bytes()),
                    ("SYSID", run.sysid.as_bytes()),
                    ("TRANSID", run.transaction.as_bytes()),
                    (
                        "PRINCIPAL",
                        run.invocation.principal.id().as_str().as_bytes(),
                    ),
                ] {
                    if request.arguments.contains_key(name) {
                        response
                            .outputs
                            .insert(name.into(), bounded(value.to_vec())?);
                    }
                }
                Ok(response)
            }
            CicsOperation::Asktime => self.asktime(run),
            CicsOperation::FormatTime => self.format_time(run, &request),
            CicsOperation::Inquire => self.inquire(run, &request),
            CicsOperation::SendMap | CicsOperation::SendText => self.send(run, &request),
            CicsOperation::ReceiveMap => self.receive(run),
            CicsOperation::SetFileStatus => self.set_file_statuses(run, &request),
            CicsOperation::Retrieve => self.response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                run.retrieve.clone(),
            ),
            CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Rewrite
            | CicsOperation::Delete
            | CicsOperation::StartBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse => self.file(run, &request),
            CicsOperation::Link | CicsOperation::Xctl => self.transfer(run, &request),
            CicsOperation::Return => self.return_transaction(run, &request),
            CicsOperation::Abend => self.response(
                run,
                if run.abend_handler.is_some() {
                    CicsDisposition::Handler
                } else {
                    CicsDisposition::Abended
                },
                "ERROR",
                27,
                0,
                run.abend_handler.clone(),
                None,
                argument_bytes(&request, "ABCODE").unwrap_or_default(),
            ),
            CicsOperation::WriteTransientData => self.write_transient(run, &request),
            CicsOperation::Syncpoint => self.syncpoint(run, &request),
        }
        .or_else(|problem| self.condition(run, &request.condition_policy, problem))
    }

    fn send(&self, run: &Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let mut state = self.lock()?;
        let mut payload = argument_bytes(request, "FROM")
            .or_else(|| argument_bytes(request, "DATA"))
            .unwrap_or_default();
        let mut field_protection = None;
        let mut field_modified = None;
        let mut field_values = None;
        if request.operation == CicsOperation::SendMap {
            let mapset = argument_text(request, "MAPSET")?;
            let map = argument_text(request, "MAP")?;
            let definition = state
                .maps
                .get(&(mapset.to_ascii_uppercase(), map.to_ascii_uppercase()))
                .ok_or(HostProblem::NotFound)?;
            field_protection = Some(symbolic_map_protection(definition, &payload));
            field_modified = Some(symbolic_map_modified(definition, &payload));
            if payload.is_empty() {
                let mut values = BTreeMap::new();
                for item in &definition.fields {
                    field(&mut payload, item.name.as_bytes())?;
                    field(&mut payload, &item.initial)?;
                    values.insert(item.name.to_ascii_uppercase(), item.initial.clone());
                }
                field_values = Some(values);
            } else if definition
                .fields
                .iter()
                .all(|field| field.output_offset.is_some())
            {
                field_values = Some(symbolic_map_values(definition, &payload)?);
                payload = encode_symbolic_map_output(definition, &payload)?;
            } else if definition.fields.iter().any(|field| field.secret) {
                return Err(HostProblem::Unsupported);
            } else {
                field_values = Some(decode_map_payload(&payload, self.limits)?);
            }
        }
        if payload.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version += 1;
        next.screen = payload.clone();
        if request.operation == CicsOperation::SendMap {
            next.mapset = Some(argument_text(request, "MAPSET")?.to_ascii_uppercase());
            next.map = Some(argument_text(request, "MAP")?.to_ascii_uppercase());
            next.field_protection = field_protection.unwrap_or_default();
            next.field_modified = field_modified.unwrap_or_default();
            next.field_values = field_values.unwrap_or_default();
        }
        self.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
        self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            payload,
        )
    }

    fn receive(&self, run: &Run) -> Result<CicsResponse, HostProblem> {
        let mut state = self.lock()?;
        let current = state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version += 1;
        let (disposition, payload, fields) = if let Some(input) = next.input.take() {
            let fields = decode_map_payload(&input, self.limits)?;
            (CicsDisposition::Complete, input, fields)
        } else {
            next.suspended = true;
            (CicsDisposition::Suspended, Vec::new(), BTreeMap::new())
        };
        self.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
        let mut response = self.response(run, disposition, "NORMAL", 0, 0, None, None, payload)?;
        response.aid = current.aid;
        for (name, value) in fields {
            let input_length = value.len();
            let value = current
                .mapset
                .as_ref()
                .zip(current.map.as_ref())
                .and_then(|(mapset, map)| state.maps.get(&(mapset.clone(), map.clone())))
                .and_then(|map| {
                    map.fields
                        .iter()
                        .find(|field| field.name.eq_ignore_ascii_case(&name))
                })
                .map_or(value.clone(), |field| normalize_bms_input(field, &value));
            response
                .outputs
                .insert(format!("BMS.{name}"), bounded(value.clone())?);
            response.outputs.insert(
                format!("BMS.{name}.LENGTH"),
                decimal_payload(
                    i64::try_from(input_length).map_err(|_| HostProblem::ResourceExhausted)?,
                )?,
            );
        }
        Ok(response)
    }

    fn asktime(&self, run: &mut Run) -> Result<CicsResponse, HostProblem> {
        let timestamp = match self.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
            HostResult::Clock(value) => value,
            _ => return Err(HostProblem::ProviderFailure),
        };
        let instant = parse_clock_timestamp(&timestamp)?;
        let absolute = absolute_milliseconds(instant)?;
        let mut response = self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?;
        response
            .outputs
            .insert("ABSTIME".into(), decimal_payload(absolute)?);
        Ok(response)
    }

    fn format_time(&self, run: &Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let absolute = argument_text(request, "ABSTIME")?
            .trim()
            .parse::<i64>()
            .map_err(|_| HostProblem::Malformed)?;
        let instant = instant_from_absolute(absolute)?;
        let date_separator = separator(request, "DATESEP", b'/')?;
        let time_separator = separator(request, "TIMESEP", b':')?;
        let mut response = self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?;
        for key in ["YYYYMMDD", "YYMMDD", "MMDDYY", "MMDDYYYY", "YYDDD"] {
            if request.arguments.contains_key(key) {
                let value = format_date(instant, key, date_separator)?;
                response.outputs.insert(key.into(), bounded(value)?);
            }
        }
        if request.arguments.contains_key("TIME") {
            response.outputs.insert(
                "TIME".into(),
                bounded(format_time_value(instant, time_separator))?,
            );
        }
        if request.arguments.contains_key("MILLISECONDS") {
            response.outputs.insert(
                "MILLISECONDS".into(),
                bounded(format!("{:03}", instant.millisecond).into_bytes())?,
            );
        }
        Ok(response)
    }

    fn inquire(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let target = argument_text(request, "PROGRAM")?
            .trim()
            .to_ascii_uppercase();
        self.authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{target}"),
            AccessIntent::Execute,
        )?;
        if self.lock()?.programs.contains(&target) {
            return self.response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            );
        }
        let program = ProgramName::new(target, 128).map_err(|_| HostProblem::Malformed)?;
        let result = self.nested(
            run,
            HostRequest::Program(ProgramRequest::Inquire { program }),
        );
        match result {
            Err(HostProblem::NotFound) => Err(HostProblem::Condition {
                name: "PGMIDERR".into(),
                response: 27,
                response2: 0,
            }),
            Err(problem) => Err(problem),
            Ok(HostResult::Program(_)) => self.response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            ),
            Ok(_) => Err(HostProblem::ProviderFailure),
        }
    }

    fn return_transaction(
        &self,
        run: &Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        let mut state = self.lock()?;
        let next_transaction = argument_optional(request, "TRANSID")
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty());
        let commarea = argument_bytes(request, "COMMAREA").unwrap_or_default();
        if commarea.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        if let Some(transaction) = &next_transaction {
            if transaction.len() > 16
                || !transaction
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(HostProblem::Malformed);
            }
            let effect_key = request
                .mutation
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?
                .idempotency_key
                .as_str()
                .to_string();
            let current = state.continuations.get(&run.session).cloned();
            if let Some(current) = &current
                && current.effect_key == effect_key
            {
                if current.transaction != *transaction || current.commarea != commarea {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                let version = current.as_ref().map_or(Ok(1), |value| {
                    value
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)
                })?;
                let next = DurableContinuation {
                    transaction: transaction.clone(),
                    commarea: commarea.clone(),
                    claimed_by: None,
                    effect_key,
                    version,
                };
                self.persist_continuation(
                    &run.session,
                    &next,
                    current.as_ref().map(|value| value.version),
                )
                .map_err(mutation_problem)?;
                state.continuations.insert(run.session.clone(), next);
            }
        } else if let Some(current) = state.continuations.get(&run.session).cloned()
            && current.claimed_by.as_deref() == Some(run.invocation.run_unit_id.as_str())
        {
            self.store
                .delete_provider_state("cics-continuation", &run.session, current.version)
                .map_err(store_error)
                .map_err(mutation_problem)?;
            state.continuations.remove(&run.session);
        }
        self.response(
            run,
            CicsDisposition::Returned,
            "NORMAL",
            0,
            0,
            None,
            next_transaction,
            commarea,
        )
    }

    fn syncpoint(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let mutation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?;
        let outcome = if request.arguments.contains_key("OPTION.ROLLBACK") {
            CicsUnitOfWorkOutcome::RolledBack
        } else {
            CicsUnitOfWorkOutcome::Committed
        };
        let key = mutation.idempotency_key.as_str();
        if let Some(record) = self
            .store
            .get_provider_state("cics-uow", key)
            .map_err(store_error)?
        {
            let existing = decode_uow(&record.payload)?;
            if existing.transaction != run.transaction {
                return Err(HostProblem::IdempotencyConflict);
            }
            match (existing.finalized, existing.outcome) {
                (false, existing) if existing == outcome => {
                    return Err(HostProblem::UnknownOutcome);
                }
                (true, existing) if existing == outcome => {
                    return self.uow_response(run, outcome);
                }
                _ => return Err(HostProblem::IdempotencyConflict),
            }
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: key.into(),
                    version: 1,
                    payload: encode_uow(&UowRecord {
                        finalized: false,
                        outcome,
                        transaction: run.transaction.clone(),
                    })?,
                },
                None,
            )
            .map_err(store_error)?;
        self.syncpoint_db2(run, outcome)?;
        self.syncpoint_ims(run, outcome)?;
        self.syncpoint_mq(run, outcome)?;
        if outcome == CicsUnitOfWorkOutcome::RolledBack {
            self.rollback_run(run)?;
        } else {
            self.clear_undo(run)?;
        }
        // A syncpoint ends every no-token file update context regardless of
        // whether the unit of work commits or rolls back.
        run.current_records.clear();
        run.current_record_values.clear();
        if self
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: key.into(),
                    version: 2,
                    payload: encode_uow(&UowRecord {
                        finalized: true,
                        outcome,
                        transaction: run.transaction.clone(),
                    })?,
                },
                Some(1),
            )
            .is_err()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        self.uow_response(run, outcome)
    }

    fn syncpoint_db2(
        &self,
        run: &mut Run,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<(), HostProblem> {
        let capability = CapabilityId::new("host.db2.write", InvocationLimits::default())
            .expect("static Db2 capability");
        if !self.host.capability_ready(capability.as_str())
            || !run.invocation.principal.has_grant(&capability)
        {
            return Ok(());
        }
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = nested_key(run, run.host_sequence)?;
        let result = self.invoke_host(
            &run.invocation,
            run.invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: run.invocation.run_unit_id.clone(),
                sequence: run.host_sequence,
                deadline_tick: run.invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Db2(Db2Request {
                    operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                        Db2Operation::Rollback
                    } else {
                        Db2Operation::Commit
                    },
                    statement: String::new(),
                    cursor: None,
                    inputs: BTreeMap::new(),
                    outputs: Vec::new(),
                    max_rows: 0,
                    mutation: Some(Mutation {
                        sequence: run.host_sequence,
                        idempotency_key: key,
                        transaction: Some(run.transaction.clone()),
                    }),
                }),
            },
        );
        match result.outcome? {
            HostResult::Db2(result) if result.sqlcode == 0 => Ok(()),
            HostResult::Db2(result) => Err(HostProblem::Condition {
                name: format!("SQLCODE{}", result.sqlcode),
                response: result.sqlcode,
                response2: 0,
            }),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn syncpoint_ims(
        &self,
        run: &mut Run,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<(), HostProblem> {
        let capability = CapabilityId::new("host.ims.write", InvocationLimits::default())
            .expect("static IMS capability");
        if !self.host.capability_ready(capability.as_str())
            || !run.invocation.principal.has_grant(&capability)
        {
            return Ok(());
        }
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = nested_key(run, run.host_sequence)?;
        let result = self.invoke_host(
            &run.invocation,
            run.invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: run.invocation.run_unit_id.clone(),
                sequence: run.host_sequence,
                deadline_tick: run.invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Ims(ImsRequest {
                    operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                        ImsOperation::Rollback
                    } else {
                        ImsOperation::Commit
                    },
                    psb: None,
                    pcb: 1,
                    segments: Vec::new(),
                    data: Vec::new(),
                    qualifiers: Vec::new(),
                    checkpoint_id: None,
                    max_segments: 1,
                    mutation: Some(Mutation {
                        sequence: run.host_sequence,
                        idempotency_key: key,
                        transaction: Some(run.transaction.clone()),
                    }),
                }),
            },
        );
        match result.outcome? {
            HostResult::Ims(result) if result.status.trim().is_empty() => Ok(()),
            HostResult::Ims(result) => Err(HostProblem::Condition {
                name: format!("IMS{}", result.status.trim()),
                response: 1,
                response2: 0,
            }),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn syncpoint_mq(
        &self,
        run: &mut Run,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<(), HostProblem> {
        let capability = CapabilityId::new("host.mq.write", InvocationLimits::default())
            .expect("static MQ capability");
        if !self.host.capability_ready(capability.as_str())
            || !run.invocation.principal.has_grant(&capability)
        {
            return Ok(());
        }
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = nested_key(run, run.host_sequence)?;
        let result = self.invoke_host(
            &run.invocation,
            run.invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: run.invocation.run_unit_id.clone(),
                sequence: run.host_sequence,
                deadline_tick: run.invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Mq(MqRequest {
                    operation: if outcome == CicsUnitOfWorkOutcome::RolledBack {
                        MqOperation::Rollback
                    } else {
                        MqOperation::Commit
                    },
                    queue: None,
                    handle: None,
                    options: 0,
                    message: Vec::new(),
                    message_id: None,
                    correlation_id: None,
                    wait_ticks: 0,
                    max_message_bytes: 1,
                    mutation: Some(Mutation {
                        sequence: run.host_sequence,
                        idempotency_key: key,
                        transaction: Some(run.transaction.clone()),
                    }),
                }),
            },
        );
        match result.outcome? {
            HostResult::Mq(result) if result.completion_code == 0 => Ok(()),
            HostResult::Mq(result) => Err(HostProblem::Condition {
                name: format!("MQRC{}", result.reason_code),
                response: result.completion_code,
                response2: result.reason_code,
            }),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn rollback_run(&self, run: &mut Run) -> Result<(), HostProblem> {
        let undo = run.undo.clone();
        for operation in undo.into_iter().rev() {
            let sequence = run
                .host_sequence
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mutation = nested_mutation(run, sequence)?;
            let request = match operation {
                DatasetUndo::Restore {
                    dataset,
                    key,
                    record,
                } => DatasetRequest::RewriteRecord {
                    dataset,
                    key,
                    record,
                    expected_version: None,
                    mutation,
                },
                DatasetUndo::Delete { dataset, key } => DatasetRequest::DeleteRecord {
                    dataset,
                    key,
                    expected_version: None,
                    mutation,
                },
            };
            match self.nested(run, HostRequest::Dataset(request))? {
                HostResult::Dataset(DatasetResult::Mutated { .. }) => {}
                _ => return Err(HostProblem::ProviderFailure),
            }
        }
        run.current_records.clear();
        run.current_record_values.clear();
        self.clear_undo(run)
    }

    fn uow_response(
        &self,
        run: &Run,
        outcome: CicsUnitOfWorkOutcome,
    ) -> Result<CicsResponse, HostProblem> {
        let mut response = self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?;
        response.unit_of_work = Some(outcome);
        Ok(response)
    }

    pub fn reconcile_unit_of_work(
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
        match (existing.finalized, existing.outcome) {
            (true, existing_outcome) if existing_outcome == outcome => Ok(()),
            (false, existing_outcome) if existing_outcome == outcome => self
                .store
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
                        })?,
                    },
                    Some(record.version),
                )
                .map_err(store_error),
            _ => Err(HostProblem::IdempotencyConflict),
        }
    }

    fn set_file_statuses(
        &self,
        run: &mut Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        if request.arguments.is_empty() {
            return Err(HostProblem::Malformed);
        }
        let mut requested = BTreeMap::new();
        for (name, value) in &request.arguments {
            let normalized = normalize_terminal_name(name, 16)?;
            if value.schema() != "mainframe-env.cics.file-status@1" {
                return Err(HostProblem::Malformed);
            }
            let status = match value.bytes() {
                b"OPEN" => CicsFileStatus::Open,
                b"CLOSED-ENABLED" => CicsFileStatus::ClosedEnabled,
                b"CLOSED" | b"CLOSED-UNENABLED" => CicsFileStatus::Closed,
                b"DISABLED" => CicsFileStatus::Disabled,
                _ => return Err(HostProblem::Malformed),
            };
            if requested.insert(normalized, status).is_some() {
                return Err(HostProblem::Malformed);
            }
        }
        let mut state = self.lock()?;
        if requested
            .keys()
            .any(|name| !state.file_aliases.contains_key(name))
        {
            return Err(HostProblem::NotFound);
        }
        let mut writes = Vec::new();
        let mut changes = BTreeMap::new();
        for (name, status) in &requested {
            let current = state.file_statuses.get(name).copied();
            if current.is_some_and(|current| current.status == *status)
                || current.is_none() && *status == CicsFileStatus::Open
            {
                continue;
            }
            let version = current.map_or(Ok(1), |current| {
                current
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-file-status".into(),
                    key: name.clone(),
                    version,
                    payload: encode_file_status(*status),
                },
                expected_version: current.map(|current| current.version),
            });
            changes.insert(
                name.clone(),
                DurableFileStatus {
                    status: *status,
                    version,
                },
            );
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for (name, status) in changes {
            state.file_statuses.insert(name, status);
        }
        drop(state);
        self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )
    }

    fn file(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let logical_name = argument_text(request, "DATASET")
            .or_else(|_| argument_text(request, "FILE"))?
            .trim()
            .to_ascii_uppercase();
        #[cfg(feature = "fault-injection")]
        {
            let mut state = self.lock()?;
            if state
                .file_failure
                .as_ref()
                .is_some_and(|(operation, file)| {
                    *operation == request.operation && file == &logical_name
                })
            {
                state.file_failure = None;
                return Err(HostProblem::Condition {
                    name: "IOERR".into(),
                    response: 17,
                    response2: 1,
                });
            }
        }
        let definition = {
            let mut state = self.lock()?;
            match state.file_statuses.get(&logical_name).copied() {
                Some(DurableFileStatus {
                    status: CicsFileStatus::Closed,
                    ..
                }) => {
                    return Err(HostProblem::Condition {
                        name: "NOTOPEN".into(),
                        response: 19,
                        response2: 60,
                    });
                }
                Some(DurableFileStatus {
                    status: CicsFileStatus::Disabled,
                    ..
                }) => {
                    return Err(HostProblem::Condition {
                        name: "DISABLED".into(),
                        response: 84,
                        response2: 0,
                    });
                }
                Some(
                    current @ DurableFileStatus {
                        status: CicsFileStatus::ClosedEnabled,
                        ..
                    },
                ) => {
                    let version = current
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?;
                    self.store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "cics-file-status".into(),
                                key: logical_name.clone(),
                                version,
                                payload: encode_file_status(CicsFileStatus::Open),
                            },
                            Some(current.version),
                        )
                        .map_err(store_error)?;
                    state.file_statuses.insert(
                        logical_name.clone(),
                        DurableFileStatus {
                            status: CicsFileStatus::Open,
                            version,
                        },
                    );
                }
                Some(DurableFileStatus {
                    status: CicsFileStatus::Open,
                    ..
                })
                | None => {}
            }
            state.file_aliases.get(&logical_name).cloned()
        };
        let ccsid = definition.as_ref().and_then(|definition| definition.ccsid);
        let name = definition
            .map(|definition| definition.dataset.as_str().to_string())
            .unwrap_or_else(|| logical_name.clone());
        let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
        let dataset_key = dataset.as_str().to_string();
        self.authorize(
            run,
            "DATASET",
            dataset.as_str(),
            access_for(request.operation),
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 101,
            },
            other => other,
        })?;
        let member = argument_optional(request, "MEMBER")
            .map(|name| MemberName::new(name, 8).map_err(|_| HostProblem::Malformed))
            .transpose()?;
        let pending_undo = match request.operation {
            CicsOperation::Write => argument_bytes(request, "RIDFLD")
                .map(|key| encode_dataset_bytes(ccsid, &key))
                .transpose()?
                .map(|key| DatasetUndo::Delete {
                    dataset: dataset.clone(),
                    key,
                }),
            CicsOperation::Rewrite | CicsOperation::Delete => {
                let key = argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?
                    .or_else(|| run.current_records.get(&dataset_key).cloned());
                key.zip(run.current_record_values.get(&dataset_key).cloned())
                    .map(|(key, record)| DatasetUndo::Restore {
                        dataset: dataset.clone(),
                        key,
                        record,
                    })
            }
            _ => None,
        };
        let mutated_record = matches!(
            request.operation,
            CicsOperation::Write | CicsOperation::Rewrite
        )
        .then(|| encode_dataset_bytes(ccsid, &argument_bytes(request, "FROM").unwrap_or_default()))
        .transpose()?;
        let host_request = match request.operation {
            CicsOperation::Read => DatasetRequest::Read {
                dataset: dataset.clone(),
                member,
                key: argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?,
                max_records: 1,
                control: Default::default(),
            },
            CicsOperation::Write => {
                let sequence = run
                    .host_sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mutation = nested_mutation(run, sequence)?;
                DatasetRequest::Write {
                    dataset: dataset.clone(),
                    member,
                    records: vec![encode_dataset_bytes(
                        ccsid,
                        &argument_bytes(request, "FROM").unwrap_or_default(),
                    )?],
                    expected_version: argument_optional(request, "VERSION")
                        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                        .transpose()?,
                    mutation,
                }
            }
            CicsOperation::Rewrite => {
                let sequence = run
                    .host_sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mutation = nested_mutation(run, sequence)?;
                let key = argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?
                    .or_else(|| run.current_records.get(&dataset_key).cloned())
                    .ok_or_else(|| HostProblem::Condition {
                        name: "INVREQ".into(),
                        response: 16,
                        response2: 30,
                    })?;
                DatasetRequest::RewriteRecord {
                    dataset: dataset.clone(),
                    key,
                    record: encode_dataset_bytes(
                        ccsid,
                        &argument_bytes(request, "FROM").unwrap_or_default(),
                    )?,
                    expected_version: argument_optional(request, "VERSION")
                        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                        .transpose()?,
                    mutation,
                }
            }
            CicsOperation::Delete => {
                let sequence = run
                    .host_sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mutation = nested_mutation(run, sequence)?;
                let key = argument_bytes(request, "RIDFLD")
                    .map(|value| encode_dataset_bytes(ccsid, &value))
                    .transpose()?
                    .or_else(|| run.current_records.get(&dataset_key).cloned())
                    .ok_or_else(|| HostProblem::Condition {
                        name: "INVREQ".into(),
                        response: 16,
                        response2: 0,
                    })?;
                DatasetRequest::DeleteRecord {
                    dataset: dataset.clone(),
                    key,
                    expected_version: argument_optional(request, "VERSION")
                        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                        .transpose()?,
                    mutation,
                }
            }
            CicsOperation::StartBrowse => DatasetRequest::StartBrowse {
                dataset: dataset.clone(),
                key: encode_dataset_bytes(
                    ccsid,
                    &argument_bytes(request, "RIDFLD").unwrap_or_default(),
                )?,
                relation: mainframe_env_host_api::KeyRelation::GreaterOrEqual,
            },
            CicsOperation::ReadNext | CicsOperation::ReadPrev => DatasetRequest::ReadNext {
                dataset: dataset.clone(),
                cursor: argument_optional(request, "CURSOR")
                    .or_else(|| run.browses.get(&dataset_key).cloned())
                    .ok_or_else(|| HostProblem::Condition {
                        name: "INVREQ".into(),
                        response: 16,
                        response2: 0,
                    })?,
                reverse: request.operation == CicsOperation::ReadPrev,
                control: Default::default(),
            },
            CicsOperation::EndBrowse => DatasetRequest::EndBrowse {
                dataset: dataset.clone(),
                cursor: argument_optional(request, "CURSOR")
                    .or_else(|| run.browses.get(&dataset_key).cloned())
                    .ok_or_else(|| HostProblem::Condition {
                        name: "INVREQ".into(),
                        response: 16,
                        response2: 0,
                    })?,
            },
            _ => return Err(HostProblem::Malformed),
        };
        let operation = request.operation;
        #[cfg(feature = "fault-injection")]
        if self.consume_file_fault(operation, &logical_name, CicsFileFaultPoint::BeforeIntent)? {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some(undo) = pending_undo.clone() {
            self.append_undo(run, undo)?;
        }
        #[cfg(feature = "fault-injection")]
        if self.consume_file_fault(operation, &logical_name, CicsFileFaultPoint::AfterIntent)? {
            return Err(HostProblem::InfrastructureFailure);
        }
        let result = self
            .nested(run, HostRequest::Dataset(host_request))
            .map_err(|problem| match (operation, problem) {
                (CicsOperation::Read, HostProblem::NotFound) => HostProblem::Condition {
                    name: "NOTFND".into(),
                    response: 13,
                    response2: 80,
                },
                (
                    CicsOperation::Read,
                    HostProblem::Condition {
                        name, response: 13, ..
                    },
                ) if name == "NOTFND" => HostProblem::Condition {
                    name,
                    response: 13,
                    response2: 80,
                },
                (_, other) => other,
            })?;
        #[cfg(feature = "fault-injection")]
        if self.consume_file_fault(operation, &logical_name, CicsFileFaultPoint::AfterMutation)? {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut browse_key = None;
        let payload = match result {
            HostResult::Dataset(DatasetResult::Records {
                records,
                identities,
                ..
            }) => {
                if request.arguments.contains_key("OPTION.UPDATE")
                    && let Some(identity) = identities.first()
                {
                    run.current_records
                        .insert(dataset_key.clone(), identity.clone());
                }
                let record = records.into_iter().next().unwrap_or_default();
                if request.arguments.contains_key("OPTION.UPDATE") {
                    run.current_record_values
                        .insert(dataset_key.clone(), record.clone());
                }
                decode_dataset_bytes(ccsid, &record)?
            }
            HostResult::Dataset(DatasetResult::Browse {
                cursor,
                record,
                identity,
                key,
            }) => {
                if operation == CicsOperation::StartBrowse {
                    run.browses.insert(dataset_key.clone(), cursor);
                } else if operation == CicsOperation::EndBrowse {
                    run.browses.remove(&dataset_key);
                    run.current_records.remove(&dataset_key);
                }
                if request.arguments.contains_key("OPTION.UPDATE")
                    && let Some(identity) = identity
                {
                    run.current_records.insert(dataset_key.clone(), identity);
                }
                browse_key = key
                    .map(|key| decode_dataset_bytes(ccsid, &key))
                    .transpose()?;
                if matches!(operation, CicsOperation::ReadNext | CicsOperation::ReadPrev)
                    && record.is_none()
                {
                    return Err(HostProblem::Condition {
                        name: "ENDFILE".into(),
                        response: 20,
                        response2: 0,
                    });
                }
                let record = record.unwrap_or_default();
                if !record.is_empty() && request.arguments.contains_key("OPTION.UPDATE") {
                    run.current_record_values
                        .insert(dataset_key.clone(), record.clone());
                }
                decode_dataset_bytes(ccsid, &record)?
            }
            HostResult::Dataset(_) => Vec::new(),
            _ => return Err(HostProblem::ProviderFailure),
        };
        if matches!(operation, CicsOperation::Delete | CicsOperation::Rewrite) {
            run.current_records.remove(&dataset_key);
            run.current_record_values.remove(&dataset_key);
        } else if operation == CicsOperation::Write
            && let Some(identity) = argument_bytes(request, "RIDFLD")
        {
            run.current_records
                .insert(dataset_key.clone(), encode_dataset_bytes(ccsid, &identity)?);
        }
        if let Some(record) = mutated_record
            && operation != CicsOperation::Rewrite
        {
            run.current_record_values.insert(dataset_key, record);
        }
        let mut response = self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            payload,
        )?;
        if let Some(key) = browse_key {
            response.outputs.insert("RIDFLD".into(), bounded(key)?);
        }
        Ok(response)
    }

    fn transfer(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let target = argument_text(request, "PROGRAM")?
            .trim()
            .to_ascii_uppercase();
        self.authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{target}"),
            AccessIntent::Execute,
        )?;
        let program = ProgramName::new(target.clone(), 128).map_err(|_| HostProblem::Malformed)?;
        let payload = bounded(argument_bytes(request, "COMMAREA").unwrap_or_default())?;
        if request.operation == CicsOperation::Xctl && self.lock()?.programs.contains(&target) {
            return self.response(
                run,
                CicsDisposition::Transfer,
                "NORMAL",
                0,
                0,
                Some(target),
                None,
                payload.bytes().to_vec(),
            );
        }
        let host_request = if request.operation == CicsOperation::Link {
            HostRequest::Program(ProgramRequest::Link { program, payload })
        } else {
            HostRequest::Program(ProgramRequest::Xctl { program, payload })
        };
        let result = self.nested(run, host_request)?;
        let payload = match result {
            HostResult::Program(payload) => payload.bytes().to_vec(),
            _ => return Err(HostProblem::ProviderFailure),
        };
        let mut response = self.response(
            run,
            if request.operation == CicsOperation::Link {
                CicsDisposition::Complete
            } else {
                CicsDisposition::Transfer
            },
            "NORMAL",
            0,
            0,
            Some(target),
            None,
            payload.clone(),
        )?;
        if request.operation == CicsOperation::Link {
            response
                .outputs
                .insert("COMMAREA".into(), bounded(payload)?);
        }
        Ok(response)
    }

    fn write_transient(
        &self,
        run: &mut Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        let queue = argument_text(request, "QUEUE")
            .or_else(|_| argument_text(request, "TDQUEUE"))?
            .trim()
            .to_ascii_uppercase();
        if queue.is_empty()
            || queue.len() > 16
            || !queue
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(HostProblem::Malformed);
        }
        self.authorize(
            run,
            "QUEUE",
            &format!("CICS.TD.{queue}"),
            AccessIntent::Update,
        )?;
        let mut state = self.lock()?;
        let value = argument_bytes(request, "FROM").unwrap_or_default();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?;
        let effect_key = mutation.idempotency_key.as_str();
        let current = state.transient.get(&queue).cloned();
        if let Some((_, existing_value)) = current
            .as_ref()
            .and_then(|queue| queue.records.iter().find(|(key, _)| key == effect_key))
        {
            if existing_value != &value {
                return Err(HostProblem::IdempotencyConflict);
            }
            return self.response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            );
        }
        if state
            .transient
            .values()
            .map(|queue| queue.records.len())
            .sum::<usize>()
            >= self.limits.max_queue_records
            || state
                .transient_bytes
                .checked_add(value.len())
                .is_none_or(|bytes| bytes > self.limits.max_queue_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut next = current.unwrap_or(TransientQueue {
            records: Vec::new(),
            version: 0,
        });
        let expected = (next.version != 0).then_some(next.version);
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.records.push((effect_key.into(), value.clone()));
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-tdq".into(),
                    key: queue.clone(),
                    version: next.version,
                    payload: encode_transient(&next)?,
                },
                expected,
            )
            .map_err(store_error)
            .map_err(mutation_problem)?;
        state.transient.insert(queue, next);
        state.transient_bytes += value.len();
        self.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )
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
        let result = self.invoke_host(
            &run.invocation,
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

    fn condition(
        &self,
        run: &Run,
        policy: &CicsConditionPolicy,
        problem: HostProblem,
    ) -> Result<CicsResponse, HostProblem> {
        if matches!(
            problem,
            HostProblem::UnknownOutcome
                | HostProblem::InfrastructureFailure
                | HostProblem::ProviderFailure
                | HostProblem::TimedOut
                | HostProblem::Cancelled
        ) {
            return Err(problem);
        }
        let (name, response, response2) = condition_for(&problem);
        match policy {
            CicsConditionPolicy::NoHandle | CicsConditionPolicy::Respond { .. } => self.response(
                run,
                CicsDisposition::Complete,
                name,
                response,
                response2,
                None,
                None,
                Vec::new(),
            ),
            CicsConditionPolicy::Default => {
                if let Some(target) = run.handlers.get(name) {
                    self.response(
                        run,
                        CicsDisposition::Handler,
                        name,
                        response,
                        response2,
                        Some(target.clone()),
                        None,
                        Vec::new(),
                    )
                } else {
                    Err(problem)
                }
            }
        }
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
                    payload: encode_session(session)?,
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

fn run_for(
    invocation: Invocation,
    session: &str,
    transaction: &str,
    applid: &str,
    sysid: &str,
) -> Run {
    let retrieve = invocation
        .bindings
        .get("cics.retrieve")
        .map(|value| value.bytes().to_vec())
        .unwrap_or_default();
    Run {
        invocation,
        session: session.into(),
        transaction: transaction.to_ascii_uppercase(),
        applid: applid.to_ascii_uppercase(),
        sysid: sysid.to_ascii_uppercase(),
        host_sequence: 0,
        handlers: BTreeMap::new(),
        abend_handler: None,
        retrieve,
        current_records: BTreeMap::new(),
        current_record_values: BTreeMap::new(),
        undo: Vec::new(),
        undo_version: None,
        browses: BTreeMap::new(),
        trace: Vec::new(),
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

const fn valid_aid(aid: u8) -> bool {
    matches!(
        aid,
        0x6b..=0x6e | 0x7d | 0xc1..=0xc9 | 0x4a..=0x4c | 0xf1..=0xfc
    )
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
        let address = terminal_field_address(session, definition)?;
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
    if record.len() < 3 || !valid_aid(record[0]) || record.contains(&0xff) {
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
            .find(|field| terminal_field_address(session, field) == Ok(address))
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

fn terminal_field_address(
    session: &Session,
    field: &BmsFieldDefinition,
) -> Result<u16, HostProblem> {
    let row = field.row.checked_sub(1).ok_or(HostProblem::Malformed)?;
    let column = field.column.checked_sub(1).ok_or(HostProblem::Malformed)?;
    row.checked_mul(session.columns)
        .and_then(|value| value.checked_add(column))
        .filter(|value| *value < session.rows.saturating_mul(session.columns))
        .ok_or(HostProblem::Malformed)
}

fn encode_terminal_address(address: u16) -> Result<[u8; 2], HostProblem> {
    if address > 0x3fff {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok([(address >> 8) as u8, address as u8])
}

fn decode_terminal_address(first: u8, second: u8) -> Result<u16, HostProblem> {
    if first & 0xc0 != 0 {
        return Err(HostProblem::Unsupported);
    }
    Ok((u16::from(first) << 8) | u16::from(second))
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

fn validate_map(map: &BmsMapDefinition, limits: CicsLimits) -> Result<(), HostProblem> {
    if map.mapset.is_empty()
        || map.map.is_empty()
        || map.rows == 0
        || map.columns == 0
        || map.fields.len() > limits.max_fields
    {
        return Err(HostProblem::Malformed);
    }
    for field in &map.fields {
        if field.name.is_empty()
            || field.length == 0
            || field.row == 0
            || field.column == 0
            || field.row > map.rows
            || field.column > map.columns
            || field.initial.len() > usize::from(field.length)
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
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

fn condition_for(problem: &HostProblem) -> (&'static str, i32, i32) {
    match problem {
        HostProblem::NotFound => ("NOTFND", 13, 0),
        HostProblem::Unauthorized => ("NOTAUTH", 70, 0),
        HostProblem::ResourceExhausted => ("ERROR", 1, 101),
        HostProblem::Condition {
            name,
            response,
            response2,
        } => (condition_name(name), *response, *response2),
        _ => ("ERROR", 1, 0),
    }
}

fn condition_name(name: &str) -> &'static str {
    match name {
        "DUPREC" => "DUPREC",
        "INVREQ" => "INVREQ",
        "LENGERR" => "LENGERR",
        "ENDFILE" => "ENDFILE",
        "PGMIDERR" => "PGMIDERR",
        "NOTAUTH" => "NOTAUTH",
        "NOTFND" => "NOTFND",
        "NOTOPEN" => "NOTOPEN",
        "IOERR" => "IOERR",
        "LOCKED" => "LOCKED",
        "RECORDBUSY" => "RECORDBUSY",
        _ => "ERROR",
    }
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
}

fn encode_uow(record: &UowRecord) -> Result<Vec<u8>, HostProblem> {
    let state = match (record.finalized, record.outcome) {
        (false, CicsUnitOfWorkOutcome::Committed) => b'C',
        (false, CicsUnitOfWorkOutcome::RolledBack) => b'R',
        (true, CicsUnitOfWorkOutcome::Committed) => b'c',
        (true, CicsUnitOfWorkOutcome::RolledBack) => b'r',
    };
    let mut value = b"MECU1".to_vec();
    value.push(state);
    field(&mut value, record.transaction.as_bytes())?;
    Ok(value)
}

fn decode_uow(payload: &[u8]) -> Result<UowRecord, HostProblem> {
    let mut reader = Reader {
        bytes: payload,
        at: 0,
    };
    if reader.take(5)? != b"MECU1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let (finalized, outcome) = match reader.take(1)?[0] {
        b'C' => (false, CicsUnitOfWorkOutcome::Committed),
        b'R' => (false, CicsUnitOfWorkOutcome::RolledBack),
        b'c' => (true, CicsUnitOfWorkOutcome::Committed),
        b'r' => (true, CicsUnitOfWorkOutcome::RolledBack),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let transaction =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    if reader.at != payload.len() || transaction.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(UowRecord {
        finalized,
        outcome,
        transaction,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClockInstant {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

fn parse_clock_timestamp(value: &str) -> Result<ClockInstant, HostProblem> {
    if value.len() != 17 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HostProblem::ProviderFailure);
    }
    let number = |range: std::ops::Range<usize>| {
        value[range]
            .parse::<u32>()
            .map_err(|_| HostProblem::ProviderFailure)
    };
    let instant = ClockInstant {
        year: i64::from(number(0..4)?),
        month: number(4..6)?,
        day: number(6..8)?,
        hour: number(8..10)?,
        minute: number(10..12)?,
        second: number(12..14)?,
        millisecond: number(14..17)?,
    };
    validate_instant(instant)?;
    Ok(instant)
}

fn validate_instant(instant: ClockInstant) -> Result<(), HostProblem> {
    let days = days_from_civil(instant.year, instant.month, instant.day)
        .ok_or(HostProblem::ProviderFailure)?;
    let (year, month, day) = civil_from_days(days);
    if (year, month, day) != (instant.year, instant.month, instant.day)
        || instant.hour > 23
        || instant.minute > 59
        || instant.second > 59
        || instant.millisecond > 999
    {
        Err(HostProblem::ProviderFailure)
    } else {
        Ok(())
    }
}

fn absolute_milliseconds(instant: ClockInstant) -> Result<i64, HostProblem> {
    let epoch = days_from_civil(1900, 1, 1).ok_or(HostProblem::ProviderFailure)?;
    let days = days_from_civil(instant.year, instant.month, instant.day)
        .ok_or(HostProblem::ProviderFailure)?
        .checked_sub(epoch)
        .ok_or(HostProblem::ResourceExhausted)?;
    days.checked_mul(86_400_000)
        .and_then(|value| value.checked_add(i64::from(instant.hour) * 3_600_000))
        .and_then(|value| value.checked_add(i64::from(instant.minute) * 60_000))
        .and_then(|value| value.checked_add(i64::from(instant.second) * 1_000))
        .and_then(|value| value.checked_add(i64::from(instant.millisecond)))
        .ok_or(HostProblem::ResourceExhausted)
}

fn instant_from_absolute(value: i64) -> Result<ClockInstant, HostProblem> {
    if value < 0 {
        return Err(HostProblem::Malformed);
    }
    let epoch = days_from_civil(1900, 1, 1).ok_or(HostProblem::ProviderFailure)?;
    let days = value / 86_400_000;
    let rest = value % 86_400_000;
    let (year, month, day) = civil_from_days(
        epoch
            .checked_add(days)
            .ok_or(HostProblem::ResourceExhausted)?,
    );
    Ok(ClockInstant {
        year,
        month,
        day,
        hour: u32::try_from(rest / 3_600_000).map_err(|_| HostProblem::Malformed)?,
        minute: u32::try_from((rest / 60_000) % 60).map_err(|_| HostProblem::Malformed)?,
        second: u32::try_from((rest / 1_000) % 60).map_err(|_| HostProblem::Malformed)?,
        millisecond: u32::try_from(rest % 1_000).map_err(|_| HostProblem::Malformed)?,
    })
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
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
    (year, month as u32, day as u32)
}

fn separator(request: &CicsRequest, name: &str, default: u8) -> Result<Option<u8>, HostProblem> {
    if let Some(value) = request.arguments.get(name) {
        if value.bytes().len() != 1 {
            return Err(HostProblem::Malformed);
        }
        Ok(value.bytes().first().copied())
    } else if request.arguments.contains_key(&format!("OPTION.{name}")) {
        Ok(Some(default))
    } else {
        Ok(None)
    }
}

fn format_date(
    instant: ClockInstant,
    format: &str,
    separator: Option<u8>,
) -> Result<Vec<u8>, HostProblem> {
    let year = u32::try_from(instant.year).map_err(|_| HostProblem::Malformed)?;
    let parts = match format {
        "YYYYMMDD" => vec![
            format!("{year:04}"),
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
        ],
        "YYMMDD" => vec![
            format!("{:02}", year % 100),
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
        ],
        "MMDDYY" => vec![
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
            format!("{:02}", year % 100),
        ],
        "MMDDYYYY" => vec![
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
            format!("{year:04}"),
        ],
        "YYDDD" => {
            let jan1 = days_from_civil(instant.year, 1, 1).ok_or(HostProblem::Malformed)?;
            let current = days_from_civil(instant.year, instant.month, instant.day)
                .ok_or(HostProblem::Malformed)?;
            return Ok(format!("{:02}{:03}", year % 100, current - jan1 + 1).into_bytes());
        }
        _ => return Err(HostProblem::Malformed),
    };
    let joiner = separator.map_or_else(String::new, |value| char::from(value).to_string());
    Ok(parts.join(&joiner).into_bytes())
}

fn format_time_value(instant: ClockInstant, separator: Option<u8>) -> Vec<u8> {
    let parts = [
        format!("{:02}", instant.hour),
        format!("{:02}", instant.minute),
        format!("{:02}", instant.second),
    ];
    let joiner = separator.map_or_else(String::new, |value| char::from(value).to_string());
    parts.join(&joiner).into_bytes()
}

fn decimal_payload(value: i64) -> Result<BoundedPayload, HostProblem> {
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
    let mut out = b"MECM5".to_vec();
    field(&mut out, map.mapset.as_bytes())?;
    field(&mut out, map.map.as_bytes())?;
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
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let mapset =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let map =
        String::from_utf8(reader.field(16)?).map_err(|_| HostProblem::InfrastructureFailure)?;
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
        rows,
        columns,
        fields,
    };
    validate_map(&definition, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(definition)
}

fn encode_session(session: &Session) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MECS4".to_vec();
    out.extend_from_slice(&session.rows.to_be_bytes());
    out.extend_from_slice(&session.columns.to_be_bytes());
    field(&mut out, session.principal.as_bytes())?;
    field(&mut out, session.transaction.as_bytes())?;
    field(&mut out, session.run_unit.as_bytes())?;
    field(&mut out, session.csrf_sha256.as_bytes())?;
    out.extend_from_slice(&session.idle_timeout_ticks.to_be_bytes());
    out.extend_from_slice(&session.expires_at_tick.to_be_bytes());
    out.push(u8::from(session.connected));
    out.push(session.aid);
    out.push(u8::from(session.suspended));
    field(&mut out, session.mapset.as_deref().unwrap_or("").as_bytes())?;
    field(&mut out, session.map.as_deref().unwrap_or("").as_bytes())?;
    out.extend_from_slice(
        &u32::try_from(session.field_protection.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, protected) in &session.field_protection {
        field(&mut out, name.as_bytes())?;
        out.push(u8::from(*protected));
    }
    out.extend_from_slice(
        &u32::try_from(session.field_modified.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, modified) in &session.field_modified {
        field(&mut out, name.as_bytes())?;
        out.push(u8::from(*modified));
    }
    out.extend_from_slice(
        &u32::try_from(session.field_values.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &session.field_values {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value)?;
    }
    field(&mut out, &session.screen)?;
    match &session.input {
        Some(input) => {
            out.push(1);
            field(&mut out, input)?;
        }
        None => out.push(0),
    }
    Ok(out)
}

fn decode_session(bytes: &[u8], version: u64, limits: CicsLimits) -> Result<Session, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    let schema = reader.take(5)?;
    if !matches!(schema, b"MECS1" | b"MECS2" | b"MECS3" | b"MECS4") {
        return Err(HostProblem::InfrastructureFailure);
    }
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
    ) = if matches!(schema, b"MECS2" | b"MECS3" | b"MECS4") {
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
    let (mapset, map) = if matches!(schema, b"MECS2" | b"MECS3" | b"MECS4") {
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
    let field_protection = if matches!(schema, b"MECS3" | b"MECS4") {
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
    let field_modified = if schema == b"MECS4" {
        decode_session_flags(&mut reader, limits)?
    } else {
        BTreeMap::new()
    };
    let field_values = if schema == b"MECS4" {
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
    if reader.at != bytes.len() || rows == 0 || columns == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Session {
        rows,
        columns,
        principal,
        transaction,
        run_unit,
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
        version,
    })
}

fn decode_session_flags(
    reader: &mut Reader<'_>,
    limits: CicsLimits,
) -> Result<BTreeMap<String, bool>, HostProblem> {
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
        let name =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        if values.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

fn field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value);
    Ok(())
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
    use mainframe_env_execution_api::{
        ArtifactRef, ExecutionId, Principal, RequestId, ResourceLimits, Selector, ServiceClass,
        TraceId,
    };
    use mainframe_env_host_api::{
        DatasetAttributes, DatasetOrganization, HostLimits, RecordFormat, RegistrySnapshot,
        SecretRef,
    };
    use mainframe_env_racf::{MemorySecretResolver, RacfService, racf_providers};
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::collections::BTreeSet;

    struct Authority {
        descriptor: CapabilityDescriptor,
    }

    #[derive(Default)]
    struct DatasetTrace {
        requests: Mutex<Vec<DatasetRequest>>,
    }

    struct TracedDataset {
        descriptor: CapabilityDescriptor,
        trace: Arc<DatasetTrace>,
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

    impl HostProvider for TracedDataset {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
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

    #[test]
    fn shared_catalog_recognizes_all_frozen_forms() {
        let cases = [
            ("ABEND", CicsOperation::Abend),
            ("ASKTIME", CicsOperation::Asktime),
            ("ASSIGN", CicsOperation::Assign),
            ("DELETE", CicsOperation::Delete),
            ("ENDBR", CicsOperation::EndBrowse),
            ("FORMATTIME", CicsOperation::FormatTime),
            ("HANDLE ABEND", CicsOperation::HandleAbend),
            ("HANDLE CONDITION", CicsOperation::HandleCondition),
            ("INQUIRE", CicsOperation::Inquire),
            ("LINK", CicsOperation::Link),
            ("READ", CicsOperation::Read),
            ("READNEXT", CicsOperation::ReadNext),
            ("READPREV", CicsOperation::ReadPrev),
            ("RECEIVE MAP", CicsOperation::ReceiveMap),
            ("RETRIEVE", CicsOperation::Retrieve),
            ("RETURN", CicsOperation::Return),
            ("REWRITE", CicsOperation::Rewrite),
            ("SEND TEXT", CicsOperation::SendText),
            ("SEND MAP", CicsOperation::SendMap),
            ("STARTBR", CicsOperation::StartBrowse),
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
    fn carddemo_residual_cics_forms_are_executable() {
        for source in [
            "ASKTIME",
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
        assert_eq!(
            absolute.parse::<i64>().unwrap(),
            absolute_milliseconds(ClockInstant {
                year: 2026,
                month: 8,
                day: 30,
                hour: 12,
                minute: 34,
                second: 56,
                millisecond: 789,
            })
            .unwrap()
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

        let assign = request(
            CicsOperation::Assign,
            BTreeMap::from([
                ("APPLID".into(), argument(b"APP-OUT")),
                ("SYSID".into(), argument(b"SYS-OUT")),
            ]),
            3,
        );
        let assigned = service
            .invoke(&effect(&invocation.run_unit_id, assign.clone(), 3), assign)
            .unwrap();
        assert_eq!(assigned.outputs["APPLID"].bytes(), b"MEAPPL");
        assert_eq!(assigned.outputs["SYSID"].bytes(), b"MESYS");

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
        let second_write = request(
            CicsOperation::WriteTransientData,
            BTreeMap::from([
                ("QUEUE".into(), argument(b"JOBS")),
                ("FROM".into(), argument(b"//SECOND JOB")),
            ]),
            1,
        );
        initial
            .invoke(
                &effect(&second_invocation.run_unit_id, second_write.clone(), 1),
                second_write,
            )
            .unwrap();
        assert_eq!(
            initial.transient_records("JOBS").unwrap(),
            [b"//REPORT JOB".to_vec(), b"//SECOND JOB".to_vec()]
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
            [b"//REPORT JOB".to_vec(), b"//SECOND JOB".to_vec()]
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
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, abend.clone(), 5), abend)
                .unwrap()
                .disposition,
            CicsDisposition::Handler
        );
        let cancel = request(
            CicsOperation::HandleAbend,
            BTreeMap::from([("OPTION.CANCEL".into(), argument(b""))]),
            6,
        );
        service
            .invoke(&effect(&invocation.run_unit_id, cancel.clone(), 6), cancel)
            .unwrap();
        let abend = request(
            CicsOperation::Abend,
            BTreeMap::from([("ABCODE".into(), argument(b"9999"))]),
            7,
        );
        assert_eq!(
            service
                .invoke(&effect(&invocation.run_unit_id, abend.clone(), 7), abend)
                .unwrap()
                .disposition,
            CicsDisposition::Abended
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
        let mut receive = request(CicsOperation::ReceiveMap, BTreeMap::new(), 2);
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
                rows: 24,
                columns: 80,
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

    #[test]
    fn bms_send_file_read_and_program_transfer_are_typed() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let (invocation, _) = registered(&service);
        service
            .register_map(BmsMapDefinition {
                mapset: "MENUMS".into(),
                map: "MENU".into(),
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
            DatasetRequest::StartBrowse { dataset, .. }
                if dataset.as_str() == "CARDDEMO.CARDDAT"
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
}
