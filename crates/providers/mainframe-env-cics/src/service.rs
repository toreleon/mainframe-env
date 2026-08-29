use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
    RunUnitId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsConditionPolicy, CicsDisposition, CicsOperation,
    CicsRequest, CicsResponse, CicsUnitOfWorkOutcome, ClockRequest, DatasetName, DatasetRequest,
    DatasetResult, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest, HostResult,
    MemberName, Mutation, ProgramName, ProgramRequest, ResourceName, ScopedHostService,
    SecurityDecision, SecurityRequest, SessionId,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsLimits {
    pub max_sessions: usize,
    pub max_runs: usize,
    pub max_maps: usize,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BmsMapDefinition {
    pub mapset: String,
    pub map: String,
    pub rows: u16,
    pub columns: u16,
    pub fields: Vec<BmsFieldDefinition>,
}

#[derive(Clone, Debug)]
struct Session {
    rows: u16,
    columns: u16,
    aid: u8,
    screen: Vec<u8>,
    input: Option<Vec<u8>>,
    suspended: bool,
    version: u64,
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
    browses: BTreeMap<String, String>,
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
    file_aliases: BTreeMap<String, DatasetName>,
    continuations: BTreeMap<String, DurableContinuation>,
    transient: BTreeMap<String, TransientQueue>,
    transient_bytes: usize,
}

pub struct CicsService {
    host: Arc<ScopedHostService>,
    store: Arc<dyn ProviderStateStore>,
    limits: CicsLimits,
    state: Mutex<State>,
}

impl CicsService {
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
        let mut file_aliases = BTreeMap::new();
        for row in store
            .list_provider_state("cics-file-alias", limits.max_file_aliases)
            .map_err(store_error)?
        {
            let target =
                String::from_utf8(row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
            let dataset =
                DatasetName::new(target, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            file_aliases.insert(row.key, dataset);
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
                maps: BTreeMap::new(),
                file_aliases,
                continuations,
                transient,
                transient_bytes,
            }),
        }))
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
            aid: 0,
            screen: Vec::new(),
            input: None,
            suspended: false,
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
                browses: BTreeMap::new(),
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
                browses: BTreeMap::new(),
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
        let mut state = self.lock()?;
        if state.maps.len() >= self.limits.max_maps {
            return Err(HostProblem::ResourceExhausted);
        }
        let key = (
            definition.mapset.to_ascii_uppercase(),
            definition.map.to_ascii_uppercase(),
        );
        if state.maps.insert(key, definition).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    pub fn register_file_aliases(
        &self,
        aliases: &BTreeMap<String, DatasetName>,
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
        for (name, dataset) in aliases {
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
                if existing != dataset {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-file-alias".into(),
                    key: name,
                    version: 1,
                    payload: dataset.as_str().as_bytes().to_vec(),
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for (name, dataset) in aliases {
            state
                .file_aliases
                .insert(name.trim().to_ascii_uppercase(), dataset.clone());
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
        let mut run = self
            .lock()?
            .runs
            .remove(&effect.run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        let result = self.invoke_run(&mut run, request);
        self.lock()?.runs.insert(effect.run_unit.clone(), run);
        result
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
        if request.operation == CicsOperation::SendMap {
            let mapset = argument_text(request, "MAPSET")?;
            let map = argument_text(request, "MAP")?;
            let definition = state
                .maps
                .get(&(mapset.to_ascii_uppercase(), map.to_ascii_uppercase()))
                .ok_or(HostProblem::NotFound)?;
            if payload.is_empty() {
                for item in &definition.fields {
                    field(&mut payload, item.name.as_bytes())?;
                    field(&mut payload, &item.initial)?;
                }
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
        let (disposition, payload) = if let Some(input) = next.input.take() {
            (CicsDisposition::Complete, input)
        } else {
            next.suspended = true;
            (CicsDisposition::Suspended, Vec::new())
        };
        self.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
        let mut response = self.response(run, disposition, "NORMAL", 0, 0, None, None, payload)?;
        response.aid = current.aid;
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

    fn syncpoint(&self, run: &Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
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

    fn file(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let logical_name = argument_text(request, "DATASET")
            .or_else(|_| argument_text(request, "FILE"))?
            .trim()
            .to_ascii_uppercase();
        let name = self
            .lock()?
            .file_aliases
            .get(&logical_name)
            .map(|dataset| dataset.as_str().to_string())
            .unwrap_or(logical_name);
        let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
        let dataset_key = dataset.as_str().to_string();
        self.authorize(
            run,
            "DATASET",
            dataset.as_str(),
            access_for(request.operation),
        )?;
        let member = argument_optional(request, "MEMBER")
            .map(|name| MemberName::new(name, 8).map_err(|_| HostProblem::Malformed))
            .transpose()?;
        let host_request = match request.operation {
            CicsOperation::Read => DatasetRequest::Read {
                dataset: dataset.clone(),
                member,
                key: argument_bytes(request, "RIDFLD"),
                max_records: 1,
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
                    records: vec![argument_bytes(request, "FROM").unwrap_or_default()],
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
                    .or_else(|| run.current_records.get(&dataset_key).cloned())
                    .ok_or_else(|| HostProblem::Condition {
                        name: "INVREQ".into(),
                        response: 16,
                        response2: 0,
                    })?;
                DatasetRequest::RewriteRecord {
                    dataset: dataset.clone(),
                    key,
                    record: argument_bytes(request, "FROM").unwrap_or_default(),
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
                key: argument_bytes(request, "RIDFLD").unwrap_or_default(),
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
        let result = self.nested(run, HostRequest::Dataset(host_request))?;
        let mut browse_key = None;
        let payload = match result {
            HostResult::Dataset(DatasetResult::Records {
                records,
                identities,
                ..
            }) => {
                if let Some(identity) = identities.first() {
                    run.current_records
                        .insert(dataset_key.clone(), identity.clone());
                }
                records.into_iter().next().unwrap_or_default()
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
                if let Some(identity) = identity {
                    run.current_records.insert(dataset_key.clone(), identity);
                }
                browse_key = key;
                if matches!(operation, CicsOperation::ReadNext | CicsOperation::ReadPrev)
                    && record.is_none()
                {
                    return Err(HostProblem::Condition {
                        name: "ENDFILE".into(),
                        response: 20,
                        response2: 0,
                    });
                }
                record.unwrap_or_default()
            }
            HostResult::Dataset(_) => Vec::new(),
            _ => return Err(HostProblem::ProviderFailure),
        };
        if operation == CicsOperation::Delete {
            run.current_records.remove(&dataset_key);
        } else if operation == CicsOperation::Write
            && let Some(identity) = argument_bytes(request, "RIDFLD")
        {
            run.current_records.insert(dataset_key, identity);
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
        let result = self.host.invoke(
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
        result.effect.outcome
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

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, HostProblem> {
        self.state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
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
        _ => "ERROR",
    }
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

fn encode_session(session: &Session) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MECS1".to_vec();
    out.extend_from_slice(&session.rows.to_be_bytes());
    out.extend_from_slice(&session.columns.to_be_bytes());
    out.push(session.aid);
    out.push(u8::from(session.suspended));
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
    if reader.take(5)? != b"MECS1" {
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
    let aid = reader.take(1)?[0];
    let suspended = match reader.take(1)?[0] {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
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
        aid,
        screen,
        input,
        suspended,
        version,
    })
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
            BTreeMap::from([("DATASET".into(), argument(b"CARDDAT"))]),
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
            BTreeMap::from([("DATASET".into(), argument(b"CARDDAT"))]),
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
        let selected = outer.invoke(
            &invocation,
            1,
            false,
            effect(&invocation.run_unit_id, assign, 1),
        );
        assert!(matches!(selected.effect.outcome, Ok(HostResult::Cics(_))));
        let asktime = request(CicsOperation::Asktime, BTreeMap::new(), 2);
        assert!(matches!(
            outer
                .invoke(
                    &invocation,
                    1,
                    false,
                    effect(&invocation.run_unit_id, asktime, 2),
                )
                .effect
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
