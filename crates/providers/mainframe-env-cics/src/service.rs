use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
    RunUnitId,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, CicsConditionPolicy, CicsDisposition, CicsOperation,
    CicsRequest, CicsResponse, DatasetName, DatasetRequest, DatasetResult, EffectRequest,
    EffectResult, HostProblem, HostProvider, HostRequest, HostResult, MemberName, Mutation,
    ProgramName, ProgramRequest, ResourceName, ScopedHostService, SecurityDecision,
    SecurityRequest, SessionId,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsLimits {
    pub max_sessions: usize,
    pub max_runs: usize,
    pub max_maps: usize,
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
}

struct State {
    sessions: BTreeMap<String, Session>,
    runs: BTreeMap<RunUnitId, Run>,
    maps: BTreeMap<(String, String), BmsMapDefinition>,
    transient: BTreeMap<String, VecDeque<Vec<u8>>>,
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
        Ok(Arc::new(Self {
            host,
            store,
            limits,
            state: Mutex::new(State {
                sessions,
                runs: BTreeMap::new(),
                maps: BTreeMap::new(),
                transient: BTreeMap::new(),
                transient_bytes: 0,
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
            },
        );
        Ok(())
    }

    fn ensure_run(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        {
            let state = self.lock()?;
            if state.runs.contains_key(&invocation.run_unit_id) {
                return Ok(());
            }
        }
        let session = SessionId::new(
            format!("session-{}", invocation.run_unit_id),
            InvocationLimits::default().max_binding_bytes,
        )
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

    pub fn invoke(
        &self,
        effect: &EffectRequest,
        request: CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        if !request.operation.supported() {
            return Err(HostProblem::Unsupported);
        }
        let mut state = self.lock()?;
        let mut run = state
            .runs
            .remove(&effect.run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        let result = self.invoke_run(&mut state, &mut run, request);
        state.runs.insert(effect.run_unit.clone(), run);
        result
    }

    fn invoke_run(
        &self,
        state: &mut State,
        run: &mut Run,
        request: CicsRequest,
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
        match request.operation {
            CicsOperation::HandleCondition => {
                let condition = argument_text(&request, "CONDITION")?;
                let program = argument_text(&request, "PROGRAM")?;
                run.handlers.insert(condition, program);
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
                run.abend_handler = Some(argument_text(&request, "PROGRAM")?);
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
                let payload = format!(
                    "APPLID={}\nSYSID={}\nTRANSID={}\nPRINCIPAL={}\n",
                    run.applid,
                    run.sysid,
                    run.transaction,
                    run.invocation.principal.id()
                )
                .into_bytes();
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
            CicsOperation::SendMap | CicsOperation::SendText => self.send(state, run, &request),
            CicsOperation::ReceiveMap => self.receive(state, run),
            CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Rewrite
            | CicsOperation::Delete
            | CicsOperation::StartBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse => self.file(state, run, &request),
            CicsOperation::Xctl => self.transfer(run, &request),
            CicsOperation::Return => self.response(
                run,
                CicsDisposition::Returned,
                "NORMAL",
                0,
                0,
                None,
                argument_optional(&request, "TRANSID"),
                argument_bytes(&request, "COMMAREA").unwrap_or_default(),
            ),
            CicsOperation::Abend => self.response(
                run,
                CicsDisposition::Abended,
                "ERROR",
                27,
                0,
                run.abend_handler.clone(),
                None,
                argument_bytes(&request, "ABCODE").unwrap_or_default(),
            ),
            CicsOperation::WriteTransientData => self.write_transient(state, run, &request),
            CicsOperation::Asktime
            | CicsOperation::FormatTime
            | CicsOperation::Inquire
            | CicsOperation::Syncpoint => Err(HostProblem::Unsupported),
        }
        .or_else(|problem| self.condition(run, &request.condition_policy, problem))
    }

    fn send(
        &self,
        state: &mut State,
        run: &Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
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

    fn receive(&self, state: &mut State, run: &Run) -> Result<CicsResponse, HostProblem> {
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
        self.response(run, disposition, "NORMAL", 0, 0, None, None, payload)
    }

    fn file(
        &self,
        _state: &mut State,
        run: &mut Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        let name = argument_text(request, "DATASET").or_else(|_| argument_text(request, "FILE"))?;
        let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
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
                dataset,
                member,
                key: argument_bytes(request, "RIDFLD"),
                max_records: 1,
            },
            CicsOperation::Write | CicsOperation::Rewrite => {
                let sequence = run
                    .host_sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mutation = nested_mutation(run, sequence)?;
                DatasetRequest::Write {
                    dataset,
                    member,
                    records: vec![argument_bytes(request, "FROM").unwrap_or_default()],
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
                DatasetRequest::Delete {
                    dataset,
                    member,
                    expected_version: argument_optional(request, "VERSION")
                        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                        .transpose()?,
                    mutation,
                }
            }
            CicsOperation::StartBrowse => DatasetRequest::StartBrowse {
                dataset,
                key: argument_bytes(request, "RIDFLD").unwrap_or_default(),
            },
            CicsOperation::ReadNext | CicsOperation::ReadPrev => DatasetRequest::ReadNext {
                dataset,
                cursor: argument_text(request, "CURSOR")?,
                reverse: request.operation == CicsOperation::ReadPrev,
            },
            CicsOperation::EndBrowse => DatasetRequest::EndBrowse {
                dataset,
                cursor: argument_text(request, "CURSOR")?,
            },
            _ => return Err(HostProblem::Malformed),
        };
        let result = self.nested(run, HostRequest::Dataset(host_request))?;
        let payload = match result {
            HostResult::Dataset(DatasetResult::Records { records, .. }) => {
                records.into_iter().next().unwrap_or_default()
            }
            HostResult::Dataset(DatasetResult::Browse { cursor, record }) => {
                let mut value = cursor.into_bytes();
                if let Some(record) = record {
                    value.push(b'\n');
                    value.extend_from_slice(&record);
                }
                value
            }
            HostResult::Dataset(_) => Vec::new(),
            _ => return Err(HostProblem::ProviderFailure),
        };
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

    fn transfer(&self, run: &mut Run, request: &CicsRequest) -> Result<CicsResponse, HostProblem> {
        let target = argument_text(request, "PROGRAM")?;
        self.authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{target}"),
            AccessIntent::Execute,
        )?;
        let program = ProgramName::new(target.clone(), 128).map_err(|_| HostProblem::Malformed)?;
        let payload = bounded(argument_bytes(request, "COMMAREA").unwrap_or_default())?;
        let result = self.nested(
            run,
            HostRequest::Program(ProgramRequest::Xctl { program, payload }),
        )?;
        let payload = match result {
            HostResult::Program(payload) => payload.bytes().to_vec(),
            _ => return Err(HostProblem::ProviderFailure),
        };
        self.response(
            run,
            CicsDisposition::Transfer,
            "NORMAL",
            0,
            0,
            Some(target),
            None,
            payload,
        )
    }

    fn write_transient(
        &self,
        state: &mut State,
        run: &Run,
        request: &CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        let queue =
            argument_text(request, "QUEUE").or_else(|_| argument_text(request, "TDQUEUE"))?;
        let value = argument_bytes(request, "FROM").unwrap_or_default();
        if state.transient.values().map(VecDeque::len).sum::<usize>()
            >= self.limits.max_queue_records
            || state
                .transient_bytes
                .checked_add(value.len())
                .is_none_or(|bytes| bytes > self.limits.max_queue_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        state.transient.entry(queue).or_default().push_back(value);
        state.transient_bytes += argument_bytes(request, "FROM").map_or(0, |value| value.len());
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
                        CicsDisposition::Transfer,
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
            target,
            next_transaction,
            payload: bounded(payload)?,
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
                HostRequest::Program(_) => {
                    Ok(HostResult::Program(bounded(b"CHILD".to_vec()).unwrap()))
                }
                _ => Err(HostProblem::Unsupported),
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

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        let grants = [
            "host.security.authorize",
            "host.dataset.read",
            "host.dataset.write",
            "host.program.invoke",
            "host.cics.execute",
        ]
        .into_iter()
        .map(|name| CapabilityId::new(name, limits).unwrap())
        .collect::<BTreeSet<_>>();
        Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
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
            BTreeMap::new(),
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
            ("READ", CicsOperation::Read),
            ("READNEXT", CicsOperation::ReadNext),
            ("READPREV", CicsOperation::ReadPrev),
            ("RECEIVE MAP", CicsOperation::ReceiveMap),
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
    fn outer_registry_selects_cics_provider_and_unsupported_is_explicit() {
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
        let unsupported = request(CicsOperation::Asktime, BTreeMap::new(), 2);
        assert_eq!(
            outer
                .invoke(
                    &invocation,
                    1,
                    false,
                    effect(&invocation.run_unit_id, unsupported, 2),
                )
                .effect
                .outcome,
            Err(HostProblem::Unsupported)
        );
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
