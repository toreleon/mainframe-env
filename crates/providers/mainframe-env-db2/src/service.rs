use crate::catalog::{
    Db2CatalogGeneration, Db2ColumnDefinition, Db2ResultEncoding, Db2TableDefinition,
    input_for_column, normalize_identifier, value_for_column,
};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits, ServiceClass};
use mainframe_env_host_api::{
    CapabilityDescriptor, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row,
    EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest, HostResult,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

const STATE_NAMESPACE: &str = "db2-state";
const STATE_KEY: &str = "catalog";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2Limits {
    pub max_tables: usize,
    pub max_rows_per_table: usize,
    pub max_columns: usize,
    pub max_column_bytes: usize,
    pub max_cursors: usize,
    pub max_replays: usize,
    pub max_state_bytes: usize,
}

impl Default for Db2Limits {
    fn default() -> Self {
        Self {
            max_tables: 64,
            max_rows_per_table: 65_536,
            max_columns: 64,
            max_column_bytes: 4_096,
            max_cursors: 4_096,
            max_replays: 65_536,
            max_state_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Table {
    columns: usize,
    rows: BTreeMap<String, Vec<Vec<u8>>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PendingUnit {
    base_catalog_version: u64,
    tables: BTreeMap<String, Table>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Cursor {
    rows: Vec<Vec<Vec<u8>>>,
    index: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RecordedResult {
    request_sha256: [u8; 32],
    sqlcode: i32,
    sqlstate: String,
    message: String,
    rows: Vec<Vec<Vec<u8>>>,
    affected_rows: u64,
}

impl From<&Db2Result> for RecordedResult {
    fn from(result: &Db2Result) -> Self {
        Self {
            request_sha256: [0; 32],
            sqlcode: result.sqlcode,
            sqlstate: result.sqlstate.clone(),
            message: result.message.clone(),
            rows: result.rows.iter().map(|row| row.columns.clone()).collect(),
            affected_rows: result.affected_rows,
        }
    }
}

impl RecordedResult {
    fn result(&self) -> Db2Result {
        Db2Result {
            sqlcode: self.sqlcode,
            sqlstate: self.sqlstate.clone(),
            message: self.message.clone(),
            rows: self
                .rows
                .iter()
                .cloned()
                .map(|columns| Db2Row { columns })
                .collect(),
            affected_rows: self.affected_rows,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct State {
    catalog_version: u64,
    tables: BTreeMap<String, Table>,
    #[serde(default)]
    schemas: BTreeMap<String, Db2TableDefinition>,
    #[serde(default)]
    installations: BTreeMap<String, CatalogInstallation>,
    pending: BTreeMap<String, PendingUnit>,
    cursors: BTreeMap<String, Cursor>,
    #[serde(default)]
    cursor_declarations: BTreeMap<String, String>,
    replay: BTreeMap<String, RecordedResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CatalogInstallation {
    generation: u64,
    identity: String,
    tables: BTreeSet<String>,
}

struct DurableState {
    store_version: u64,
    state: State,
}

pub struct Db2Service {
    store: Arc<dyn ProviderStateStore>,
    limits: Db2Limits,
    durable: Mutex<DurableState>,
}

impl Db2Service {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: Db2Limits,
    ) -> Result<Arc<Self>, HostProblem> {
        let (store_version, state) = match store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => (
                record.version,
                serde_json::from_slice(&record.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            None => (0, State::default()),
        };
        validate_state(&state, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState {
                store_version,
                state,
            }),
        }))
    }

    pub fn install_catalog(&self, catalog: Db2CatalogGeneration) -> Result<(), HostProblem> {
        catalog.validate(self.limits)?;
        let mut durable = self.lock()?;
        if !durable.state.pending.is_empty() || !durable.state.cursors.is_empty() {
            return Err(HostProblem::Condition {
                name: "DB2-CATALOG-BUSY".into(),
                response: -904,
                response2: 0,
            });
        }
        let application = catalog.application.to_ascii_uppercase();
        if let Some(existing) = durable.state.installations.get(&application) {
            if existing.generation == catalog.generation {
                return if existing.identity == catalog.identity {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            if existing.generation > catalog.generation {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let mut next = durable.state.clone();
        apply_catalog_generation(&mut next, catalog, self.limits)?;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &Db2Request,
    ) -> Result<Db2Result, HostProblem> {
        let mut durable = self.lock()?;
        let request_sha256 = request_digest(request);
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str());
        if let Some(key) = replay_key
            && let Some(recorded) = durable.state.replay.get(key)
        {
            return if recorded.request_sha256 == request_sha256 {
                Ok(recorded.result())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let mut next = durable.state.clone();
        let result = apply_request(&mut next, invocation, request, self.limits)?;
        if request.operation.is_mutating() || request.operation == Db2Operation::DeclareCursor {
            if request.operation.is_mutating() {
                let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
                if next.replay.len() >= self.limits.max_replays {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut recorded = RecordedResult::from(&result);
                recorded.request_sha256 = request_sha256;
                next.replay.insert(key.to_string(), recorded);
            }
            validate_state(&next, self.limits)?;
            self.persist(&mut durable, next)?;
        }
        Ok(result)
    }

    pub fn table_rows(&self, table: &str) -> Result<Vec<Vec<Vec<u8>>>, HostProblem> {
        let durable = self.lock()?;
        let table = durable
            .state
            .tables
            .get(&table.to_ascii_uppercase())
            .ok_or(HostProblem::NotFound)?;
        Ok(table.rows.values().cloned().collect())
    }

    pub fn pending_units(&self) -> Result<usize, HostProblem> {
        Ok(self.lock()?.state.pending.len())
    }

    fn persist(&self, durable: &mut DurableState, state: State) -> Result<(), HostProblem> {
        let payload = serde_json::to_vec(&state).map_err(|_| HostProblem::ProviderFailure)?;
        if payload.len() > self.limits.max_state_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version,
                    payload,
                },
                (durable.store_version != 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        durable.state = state;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, DurableState>, HostProblem> {
        self.durable
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}

fn apply_catalog_generation(
    state: &mut State,
    catalog: Db2CatalogGeneration,
    limits: Db2Limits,
) -> Result<(), HostProblem> {
    let application = catalog.application.to_ascii_uppercase();
    let owned_elsewhere = state
        .installations
        .iter()
        .filter(|(owner, _)| *owner != &application)
        .flat_map(|(_, installation)| installation.tables.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let table_names = catalog
        .tables
        .iter()
        .map(Db2TableDefinition::normalized_name)
        .collect::<BTreeSet<_>>();
    if table_names
        .iter()
        .any(|table| owned_elsewhere.contains(table))
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    if let Some(previous) = state.installations.get(&application) {
        for table in &previous.tables {
            state.tables.remove(table);
            state.schemas.remove(table);
        }
    }
    for definition in &catalog.tables {
        let name = definition.normalized_name();
        state.schemas.insert(name.clone(), definition.clone());
        state.tables.insert(
            name,
            Table {
                columns: definition.columns.len(),
                rows: BTreeMap::new(),
            },
        );
    }
    for seed in &catalog.rows {
        let table_name = seed.table.to_ascii_uppercase();
        let definition = state
            .schemas
            .get(&table_name)
            .ok_or(HostProblem::Malformed)?;
        let values = definition
            .columns
            .iter()
            .map(|column| {
                value_for_column(&seed.values, &column.name)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let key = row_key(definition, &values)?;
        let table = state
            .tables
            .get_mut(&table_name)
            .ok_or(HostProblem::Malformed)?;
        if table.rows.len() >= limits.max_rows_per_table {
            return Err(HostProblem::ResourceExhausted);
        }
        if table.rows.insert(key, values).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    validate_foreign_keys(state)?;
    state.installations.insert(
        application,
        CatalogInstallation {
            generation: catalog.generation,
            identity: catalog.identity,
            tables: table_names,
        },
    );
    state.catalog_version = state
        .catalog_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(())
}

fn apply_request(
    state: &mut State,
    invocation: &Invocation,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    let run = invocation.run_unit_id.as_str();
    match request.operation {
        Db2Operation::ExecuteScript => execute_script(state, &request.statement, limits),
        Db2Operation::FreePlans => Ok(success(0, "PLANS FREED", Vec::new())),
        Db2Operation::DeclareCursor => declare_cursor(state, run, request, limits),
        Db2Operation::Select => select(state, run, request),
        Db2Operation::Count => count(state, run, request),
        Db2Operation::Insert => {
            let result = insert(state, run, request, limits)?;
            auto_commit_batch(state, invocation, &result)?;
            Ok(result)
        }
        Db2Operation::Update => {
            let result = update(state, run, request)?;
            auto_commit_batch(state, invocation, &result)?;
            Ok(result)
        }
        Db2Operation::Delete => {
            let result = delete(state, run, request)?;
            auto_commit_batch(state, invocation, &result)?;
            Ok(result)
        }
        Db2Operation::OpenCursor => open_cursor(state, run, request, limits),
        Db2Operation::FetchCursor => fetch_cursor(state, run, request),
        Db2Operation::CloseCursor => close_cursor(state, run, request),
        Db2Operation::Commit => commit(state, run),
        Db2Operation::Rollback => rollback(state, run),
        Db2Operation::Extract => extract(state, run, request, limits),
    }
}

fn execute_script(
    state: &mut State,
    statement: &str,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    let mut changed = false;
    for definition in parse_create_tables(statement, limits)? {
        let name = definition.normalized_name();
        if state.tables.len() >= limits.max_tables && !state.tables.contains_key(&name) {
            return Err(HostProblem::ResourceExhausted);
        }
        if let Some(installed) = state.schemas.get(&name) {
            if !schemas_compatible(installed, &definition) {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else {
            state.schemas.insert(name.clone(), definition.clone());
            changed = true;
        }
        state.tables.entry(name).or_insert_with(|| Table {
            columns: definition.columns.len(),
            rows: BTreeMap::new(),
        });
    }

    let mut affected = 0_u64;
    let upper = statement.to_ascii_uppercase();
    let mut offset = 0_usize;
    while let Some(relative) = upper[offset..].find("INSERT INTO") {
        let start = offset + relative;
        let tail = &statement[start..];
        let tail_upper = &upper[start..];
        let table_name = identifier_after(tail, "INTO")
            .ok_or(HostProblem::Malformed)?
            .to_ascii_uppercase();
        let definition = state
            .schemas
            .get(&table_name)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let columns = insert_columns(tail, &definition)?;
        let next_insert = tail_upper
            .get("INSERT INTO".len()..)
            .and_then(|value| value.find("INSERT INTO"))
            .map(|value| value + "INSERT INTO".len());
        let end = [tail_upper.find("COMMIT"), next_insert, tail_upper.find(';')]
            .into_iter()
            .flatten()
            .filter(|end| *end > "INSERT INTO".len())
            .min()
            .unwrap_or(tail.len());
        let literals = quoted_literals(&tail[..end])?;
        if columns.is_empty() || literals.len() % columns.len() != 0 {
            return Err(HostProblem::Malformed);
        }
        for values in literals.chunks(columns.len()) {
            let mut row = vec![Vec::new(); definition.columns.len()];
            for (column, value) in columns.iter().zip(values) {
                let index = definition
                    .column_index(column)
                    .ok_or(HostProblem::Malformed)?;
                row[index] = value.as_bytes().to_vec();
            }
            validate_row(&definition, &row, limits)?;
            let key = row_key(&definition, &row)?;
            let table = state
                .tables
                .get_mut(&table_name)
                .ok_or(HostProblem::NotFound)?;
            if table.rows.len() >= limits.max_rows_per_table && !table.rows.contains_key(&key) {
                return Err(HostProblem::ResourceExhausted);
            }
            match table.rows.get(&key) {
                Some(existing) if existing != &row => {
                    return Err(HostProblem::IdempotencyConflict);
                }
                Some(_) => {}
                None => {
                    table.rows.insert(key, row);
                    affected += 1;
                    changed = true;
                }
            }
        }
        offset = start.saturating_add(end.max("INSERT INTO".len()));
        if offset >= statement.len() {
            break;
        }
    }
    validate_foreign_keys(state)?;
    if changed {
        state.catalog_version = state
            .catalog_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    Ok(success(affected, "SCRIPT EXECUTED", Vec::new()))
}

fn select(state: &State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let upper = request.statement.to_ascii_uppercase();
    if upper.contains("SYSIBM.SYSDUMMY1")
        || upper.split_whitespace().collect::<Vec<_>>() == ["SELECT", "1"]
    {
        return Ok(success(
            0,
            "ROW",
            vec![Db2Row {
                columns: vec![b"1".to_vec()],
            }],
        ));
    }
    let (_, definition, table) = read_relation(state, run, &request.statement)?;
    let columns = selected_column_indices(&request.statement, definition)?;
    let key = key_from_statement_inputs(&request.statement, definition, &request.inputs)?;
    let selected = if let Some(key) = key {
        table.rows.get(&key).into_iter().collect::<Vec<_>>()
    } else {
        table
            .rows
            .values()
            .take(request.max_rows as usize)
            .collect()
    };
    if selected.is_empty() {
        return Ok(sql_condition(100, "02000", "ROW NOT FOUND"));
    }
    let rows = selected
        .into_iter()
        .map(|row| result_row(definition, row, &columns))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(success(0, "ROW", rows))
}

fn count(state: &State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let (_, definition, table) = read_relation(state, run, &request.statement)?;
    let predicates = sql_predicates(&request.statement, definition, &request.inputs)?;
    let count = table
        .rows
        .values()
        .filter(|row| predicates.iter().all(|predicate| predicate.matches(row)))
        .count();
    Ok(success(
        0,
        "COUNT",
        vec![Db2Row {
            columns: vec![count.to_string().into_bytes()],
        }],
    ))
}

fn insert(
    state: &mut State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    let name = table_for_operation(&request.statement, "INTO")?;
    let definition = state
        .schemas
        .get(&name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let row = row_from_insert(&request.statement, &definition, &request.inputs, limits)?;
    let key = row_key(&definition, &row)?;
    let table = write_table(state, run, &name)?;
    if table.rows.contains_key(&key) {
        return Ok(sql_condition(-803, "23505", "DUPLICATE KEY"));
    }
    if table.rows.len() >= limits.max_rows_per_table {
        return Err(HostProblem::ResourceExhausted);
    }
    table.rows.insert(key, row);
    Ok(success(1, "ROW INSERTED", Vec::new()))
}

fn update(state: &mut State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let name = table_for_operation(&request.statement, "UPDATE")?;
    let definition = state
        .schemas
        .get(&name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let key = key_from_statement_inputs(&request.statement, &definition, &request.inputs)?
        .ok_or(HostProblem::Malformed)?;
    let assignments = update_assignments(&request.statement, &definition, &request.inputs)?;
    let table = write_table(state, run, &name)?;
    let Some(row) = table.rows.get_mut(&key) else {
        return Ok(sql_condition(100, "02000", "ROW NOT FOUND"));
    };
    for (index, value) in assignments {
        row[index] = value;
    }
    Ok(success(1, "ROW UPDATED", Vec::new()))
}

fn delete(state: &mut State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let name = table_for_operation(&request.statement, "FROM")?;
    let definition = state
        .schemas
        .get(&name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let key = key_from_statement_inputs(&request.statement, &definition, &request.inputs)?
        .ok_or(HostProblem::Malformed)?;
    let table = read_table(state, run, &name)?;
    let Some(row) = table.rows.get(&key) else {
        return Ok(sql_condition(100, "02000", "ROW NOT FOUND"));
    };
    if delete_is_restricted(state, run, &name, &definition, row)? {
        return Ok(sql_condition(-532, "23504", "DELETE RESTRICTED"));
    }
    let table = write_table(state, run, &name)?;
    if table.rows.remove(&key).is_none() {
        Ok(sql_condition(100, "02000", "ROW NOT FOUND"))
    } else {
        Ok(success(1, "ROW DELETED", Vec::new()))
    }
}

fn declare_cursor(
    state: &mut State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    if state.cursor_declarations.len() >= limits.max_cursors {
        return Err(HostProblem::ResourceExhausted);
    }
    let name = request
        .cursor
        .clone()
        .or_else(|| identifier_after(&request.statement, "DECLARE"))
        .ok_or(HostProblem::Malformed)?;
    if !request.statement.to_ascii_uppercase().contains(" CURSOR ")
        || !request.statement.to_ascii_uppercase().contains(" FROM ")
    {
        return Err(HostProblem::Malformed);
    }
    state
        .cursor_declarations
        .insert(cursor_key(run, &name), request.statement.clone());
    Ok(success(0, "CURSOR DECLARED", Vec::new()))
}

fn open_cursor(
    state: &mut State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    if state.cursors.len() >= limits.max_cursors {
        return Err(HostProblem::ResourceExhausted);
    }
    let cursor_name = request.cursor.as_deref().ok_or(HostProblem::Malformed)?;
    let statement = if request.statement.to_ascii_uppercase().contains(" FROM ") {
        request.statement.clone()
    } else {
        state
            .cursor_declarations
            .get(&cursor_key(run, cursor_name))
            .cloned()
            .ok_or(HostProblem::NotFound)?
    };
    let (_, definition, table) = read_relation(state, run, &statement)?;
    let backward = statement.to_ascii_uppercase().contains(" DESC");
    let columns = selected_column_indices(&statement, definition)?;
    let start = request
        .inputs
        .values()
        .next()
        .map(|value| trimmed(&value.value));
    let key_index = *definition
        .primary_key_indices()?
        .first()
        .ok_or(HostProblem::Malformed)?;
    let mut rows = table
        .rows
        .values()
        .filter(|row| {
            start.as_ref().is_none_or(|start| {
                start.is_empty()
                    || if backward {
                        String::from_utf8_lossy(&row[key_index]).as_ref() <= start.as_str()
                    } else {
                        String::from_utf8_lossy(&row[key_index]).as_ref() >= start.as_str()
                    }
            })
        })
        .map(|row| result_row(definition, row, &columns).map(|row| row.columns))
        .collect::<Result<Vec<_>, _>>()?;
    if backward {
        rows.reverse();
    }
    state
        .cursors
        .insert(cursor_key(run, cursor_name), Cursor { rows, index: 0 });
    Ok(success(0, "CURSOR OPEN", Vec::new()))
}

fn fetch_cursor(
    state: &mut State,
    run: &str,
    request: &Db2Request,
) -> Result<Db2Result, HostProblem> {
    let cursor = state
        .cursors
        .get_mut(&cursor_key(
            run,
            request.cursor.as_deref().ok_or(HostProblem::Malformed)?,
        ))
        .ok_or(HostProblem::NotFound)?;
    let Some(row) = cursor.rows.get(cursor.index).cloned() else {
        return Ok(sql_condition(100, "02000", "END OF CURSOR"));
    };
    cursor.index += 1;
    Ok(success(0, "ROW", vec![Db2Row { columns: row }]))
}

fn close_cursor(
    state: &mut State,
    run: &str,
    request: &Db2Request,
) -> Result<Db2Result, HostProblem> {
    state.cursors.remove(&cursor_key(
        run,
        request.cursor.as_deref().ok_or(HostProblem::Malformed)?,
    ));
    Ok(success(0, "CURSOR CLOSED", Vec::new()))
}

fn commit(state: &mut State, run: &str) -> Result<Db2Result, HostProblem> {
    let Some(pending) = state.pending.remove(run) else {
        clear_run_cursors(state, run);
        return Ok(success(0, "NOTHING TO COMMIT", Vec::new()));
    };
    if pending.base_catalog_version != state.catalog_version {
        clear_run_cursors(state, run);
        return Ok(sql_condition(-911, "40001", "SERIALIZATION CONFLICT"));
    }
    state.tables = pending.tables;
    validate_foreign_keys(state)?;
    state.catalog_version = state
        .catalog_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    clear_run_cursors(state, run);
    Ok(success(0, "COMMIT", Vec::new()))
}

fn rollback(state: &mut State, run: &str) -> Result<Db2Result, HostProblem> {
    state.pending.remove(run);
    clear_run_cursors(state, run);
    Ok(success(0, "ROLLBACK", Vec::new()))
}

fn extract(
    state: &State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    let (_, definition, table) = read_relation(state, run, &request.statement)?;
    let layout = definition
        .extract
        .as_ref()
        .ok_or(HostProblem::Unsupported)?;
    if table.rows.len() > limits.max_rows_per_table {
        return Err(HostProblem::ResourceExhausted);
    }
    let rows = table
        .rows
        .values()
        .map(|row| {
            let mut record = Vec::new();
            for field in &layout.fields {
                let index = definition
                    .column_index(&field.column)
                    .ok_or(HostProblem::Malformed)?;
                record.extend(fixed(&row[index], field.width));
            }
            record.extend_from_slice(&layout.trailer);
            Ok(Db2Row {
                columns: vec![record],
            })
        })
        .collect::<Result<Vec<_>, HostProblem>>()?;
    Ok(success(0, "EXTRACT", rows))
}

fn auto_commit_batch(
    state: &mut State,
    invocation: &Invocation,
    result: &Db2Result,
) -> Result<(), HostProblem> {
    if invocation.service_class == ServiceClass::Batch && result.sqlcode == 0 {
        let committed = commit(state, invocation.run_unit_id.as_str())?;
        if committed.sqlcode != 0 {
            return Err(HostProblem::Condition {
                name: "SQL-CONFLICT".into(),
                response: committed.sqlcode,
                response2: 0,
            });
        }
    }
    Ok(())
}

fn read_table<'a>(state: &'a State, run: &str, table: &str) -> Result<&'a Table, HostProblem> {
    state
        .pending
        .get(run)
        .map(|pending| &pending.tables)
        .unwrap_or(&state.tables)
        .get(table)
        .ok_or(HostProblem::NotFound)
}

fn write_table<'a>(
    state: &'a mut State,
    run: &str,
    table: &str,
) -> Result<&'a mut Table, HostProblem> {
    let pending = state
        .pending
        .entry(run.into())
        .or_insert_with(|| PendingUnit {
            base_catalog_version: state.catalog_version,
            tables: state.tables.clone(),
        });
    pending.tables.get_mut(table).ok_or(HostProblem::NotFound)
}

fn clear_run_cursors(state: &mut State, run: &str) {
    let prefix = format!("{run}|");
    state.cursors.retain(|key, _| !key.starts_with(&prefix));
}

fn cursor_key(run: &str, cursor: &str) -> String {
    format!("{run}|{}", cursor.to_ascii_uppercase())
}

fn row_key(definition: &Db2TableDefinition, values: &[Vec<u8>]) -> Result<String, HostProblem> {
    definition
        .primary_key_indices()?
        .into_iter()
        .map(|index| {
            values
                .get(index)
                .map(|value| trimmed(value))
                .filter(|value| !value.is_empty())
                .ok_or(HostProblem::Malformed)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn read_relation<'a>(
    state: &'a State,
    run: &str,
    statement: &str,
) -> Result<(String, &'a Db2TableDefinition, &'a Table), HostProblem> {
    let name = table_for_operation(statement, "FROM")?;
    let definition = state.schemas.get(&name).ok_or(HostProblem::NotFound)?;
    let table = read_table(state, run, &name)?;
    Ok((name, definition, table))
}

fn table_for_operation(statement: &str, keyword: &str) -> Result<String, HostProblem> {
    identifier_after(statement, keyword)
        .map(|value| value.to_ascii_uppercase())
        .ok_or(HostProblem::Malformed)
}

fn identifier_after(statement: &str, keyword: &str) -> Option<String> {
    let words = sql_words(statement);
    words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(keyword))
        .and_then(|index| words.get(index + 1))
        .map(|value| value.trim_matches('"').to_string())
}

fn sql_words(statement: &str) -> Vec<String> {
    statement
        .split(|character: char| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '$' | '#' | '@' | '-' | ':')
                || matches!(character, '=' | '<' | '>'))
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn insert_columns(
    statement: &str,
    definition: &Db2TableDefinition,
) -> Result<Vec<String>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let into = upper.find("INTO").ok_or(HostProblem::Malformed)? + "INTO".len();
    let Some(open_relative) = statement[into..].find('(') else {
        return Ok(definition
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect());
    };
    let open = into + open_relative;
    if upper[into..open].contains("SELECT") {
        return Ok(definition
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect());
    }
    let close = matching_parenthesis(statement, open).ok_or(HostProblem::Malformed)?;
    let body = &statement[open + 1..close];
    if body.contains(',') {
        body.split(',')
            .map(|column| {
                let column = column.trim().trim_matches('"').to_string();
                definition
                    .column_index(&column)
                    .map(|_| column)
                    .ok_or(HostProblem::Malformed)
            })
            .collect()
    } else {
        let columns = sql_words(body)
            .into_iter()
            .filter(|column| definition.column_index(column).is_some())
            .collect::<Vec<_>>();
        if columns.is_empty() {
            Err(HostProblem::Malformed)
        } else {
            Ok(columns)
        }
    }
}

fn selected_column_indices(
    statement: &str,
    definition: &Db2TableDefinition,
) -> Result<Vec<usize>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let select = upper.find("SELECT").ok_or(HostProblem::Malformed)? + "SELECT".len();
    let from = upper[select..]
        .find("FROM")
        .map(|offset| select + offset)
        .ok_or(HostProblem::Malformed)?;
    let mut expression = statement[select..from].trim();
    if let Some(into) = expression.to_ascii_uppercase().find(" INTO ") {
        expression = expression[..into].trim();
    }
    expression = expression
        .strip_prefix("DISTINCT ")
        .or_else(|| expression.strip_prefix("distinct "))
        .unwrap_or(expression);
    if expression.is_empty() || expression == "*" {
        return Ok((0..definition.columns.len()).collect());
    }
    if expression.contains(',') {
        expression
            .split(',')
            .map(|column| {
                let column = column
                    .split_whitespace()
                    .last()
                    .unwrap_or(column)
                    .rsplit('.')
                    .next()
                    .unwrap_or(column)
                    .trim_matches('"');
                definition
                    .column_index(column)
                    .ok_or(HostProblem::Malformed)
            })
            .collect()
    } else {
        let mut columns = sql_words(expression)
            .into_iter()
            .filter_map(|column| definition.column_index(&column))
            .collect::<Vec<_>>();
        columns.dedup();
        if columns.is_empty() {
            Err(HostProblem::Malformed)
        } else {
            Ok(columns)
        }
    }
}

fn row_from_inputs(
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
    limits: Db2Limits,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let row = definition
        .columns
        .iter()
        .map(|column| match input_for_column(inputs, &column.name) {
            Some(variable) if variable.indicator.is_some_and(|indicator| indicator < 0) => {
                if column.nullable {
                    Ok(Vec::new())
                } else {
                    Err(HostProblem::Malformed)
                }
            }
            Some(variable) => decode_host_value(column, &variable.value),
            None if column.default_value.is_some() => {
                Ok(column.default_value.clone().unwrap_or_default())
            }
            None if column.nullable => Ok(Vec::new()),
            None => Err(HostProblem::Malformed),
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_row(definition, &row, limits)?;
    Ok(row)
}

fn row_from_insert(
    statement: &str,
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
    limits: Db2Limits,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let Some(values_offset) = upper.find("VALUES") else {
        return row_from_inputs(definition, inputs, limits);
    };
    let columns = insert_columns(statement, definition)?;
    let bindings = sql_words(&statement[values_offset + "VALUES".len()..])
        .into_iter()
        .filter_map(|word| word.strip_prefix(':').map(str::to_string))
        .collect::<Vec<_>>();
    if bindings.len() != columns.len() {
        return row_from_inputs(definition, inputs, limits);
    }
    let mut row = definition
        .columns
        .iter()
        .map(|column| column.default_value.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    for (column, binding) in columns.iter().zip(bindings) {
        let index = definition
            .column_index(column)
            .ok_or(HostProblem::Malformed)?;
        let binding = normalize_identifier(&binding);
        let variable = inputs
            .iter()
            .find(|(name, _)| normalize_identifier(name).ends_with(&binding))
            .map(|(_, variable)| variable)
            .ok_or(HostProblem::Malformed)?;
        row[index] = decode_host_value(&definition.columns[index], &variable.value)?;
    }
    validate_row(definition, &row, limits)?;
    Ok(row)
}

fn decode_host_value(column: &Db2ColumnDefinition, value: &[u8]) -> Result<Vec<u8>, HostProblem> {
    let decoded = if column.result_encoding == Db2ResultEncoding::Varchar && value.len() >= 2 {
        let declared = u16::from_be_bytes([value[0], value[1]]) as usize;
        if declared <= value.len() - 2 && declared <= column.max_bytes {
            let mut value = value[2..2 + declared].to_vec();
            while value.last() == Some(&b' ') {
                value.pop();
            }
            value
        } else {
            trimmed(value).into_bytes()
        }
    } else {
        trimmed(value).into_bytes()
    };
    if decoded.len() > column.max_bytes {
        Err(HostProblem::Malformed)
    } else {
        Ok(decoded)
    }
}

fn validate_row(
    definition: &Db2TableDefinition,
    row: &[Vec<u8>],
    limits: Db2Limits,
) -> Result<(), HostProblem> {
    if row.len() != definition.columns.len()
        || row.iter().zip(&definition.columns).any(|(value, column)| {
            value.len() > column.max_bytes
                || value.len() > limits.max_column_bytes
                || value.is_empty() && !column.nullable && column.default_value.is_none()
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn key_from_inputs(
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
) -> Result<Option<String>, HostProblem> {
    let values = definition
        .primary_key
        .iter()
        .map(|column| {
            input_for_column(inputs, column)
                .map(|variable| trimmed(&variable.value))
                .filter(|value| !value.is_empty())
        })
        .collect::<Vec<_>>();
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("|"))
        .map(Some)
        .ok_or(HostProblem::Malformed)
}

fn key_from_statement_inputs(
    statement: &str,
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
) -> Result<Option<String>, HostProblem> {
    let words = sql_words(statement);
    let mut parts = Vec::new();
    for column in &definition.primary_key {
        let normalized_column = normalize_identifier(column);
        let host = words.windows(3).find_map(|window| {
            (normalize_identifier(window[0].rsplit('.').next().unwrap_or(&window[0]))
                == normalized_column
                && window[1] == "=")
                .then(|| window[2].strip_prefix(':'))
                .flatten()
        });
        let Some(host) = host else {
            return key_from_inputs(definition, inputs);
        };
        let host = normalize_identifier(host);
        let value = inputs
            .iter()
            .find(|(name, _)| normalize_identifier(name).ends_with(&host))
            .map(|(_, variable)| trimmed(&variable.value))
            .filter(|value| !value.is_empty())
            .ok_or(HostProblem::Malformed)?;
        parts.push(value);
    }
    Ok(Some(parts.join("|")))
}

fn result_row(
    definition: &Db2TableDefinition,
    row: &[Vec<u8>],
    columns: &[usize],
) -> Result<Db2Row, HostProblem> {
    let columns = columns
        .iter()
        .map(|index| {
            let column = definition
                .columns
                .get(*index)
                .ok_or(HostProblem::Malformed)?;
            let value = row.get(*index).ok_or(HostProblem::Malformed)?;
            Ok(match column.result_encoding {
                Db2ResultEncoding::Raw => value.clone(),
                Db2ResultEncoding::Varchar => varchar(value, column.max_bytes),
            })
        })
        .collect::<Result<Vec<_>, HostProblem>>()?;
    Ok(Db2Row { columns })
}

fn update_assignments(
    statement: &str,
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
) -> Result<Vec<(usize, Vec<u8>)>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let mut assignments = Vec::new();
    if let Some(set) = upper.find(" SET ") {
        let start = set + " SET ".len();
        let end = upper[start..]
            .find(" WHERE ")
            .map_or(statement.len(), |offset| start + offset);
        for assignment in split_top_level(&statement[start..end], ',') {
            let (column, expression) = assignment.split_once('=').ok_or(HostProblem::Malformed)?;
            let index = definition
                .column_index(column.trim())
                .ok_or(HostProblem::Malformed)?;
            let value = if expression.trim().eq_ignore_ascii_case("CURRENT DATE") {
                b"CURRENT DATE".to_vec()
            } else {
                let host = expression
                    .split(':')
                    .nth(1)
                    .and_then(|tail| sql_words(tail).into_iter().next())
                    .and_then(|name| {
                        let name = normalize_identifier(&name);
                        inputs
                            .iter()
                            .find(|(candidate, _)| normalize_identifier(candidate).ends_with(&name))
                    })
                    .map(|(_, variable)| variable)
                    .or_else(|| input_for_column(inputs, &definition.columns[index].name))
                    .ok_or(HostProblem::Malformed)?;
                decode_host_value(&definition.columns[index], &host.value)?
            };
            assignments.push((index, value));
        }
    } else {
        let keys = definition
            .primary_key_indices()?
            .into_iter()
            .collect::<BTreeSet<_>>();
        for (index, column) in definition.columns.iter().enumerate() {
            if keys.contains(&index) {
                continue;
            }
            if let Some(variable) = input_for_column(inputs, &column.name) {
                assignments.push((index, decode_host_value(column, &variable.value)?));
            }
        }
    }
    if assignments.is_empty() {
        Err(HostProblem::Malformed)
    } else {
        Ok(assignments)
    }
}

#[derive(Clone, Copy)]
enum PredicateKind {
    Equal,
    Contains,
    AtLeast,
    AtMost,
}

struct SqlPredicate {
    column: usize,
    kind: PredicateKind,
    value: String,
}

impl SqlPredicate {
    fn matches(&self, row: &[Vec<u8>]) -> bool {
        let value = row
            .get(self.column)
            .map(|value| trimmed(value))
            .unwrap_or_default();
        self.value.is_empty()
            || match self.kind {
                PredicateKind::Equal => value == self.value,
                PredicateKind::Contains => value.contains(&self.value),
                PredicateKind::AtLeast => value >= self.value,
                PredicateKind::AtMost => value <= self.value,
            }
    }
}

fn sql_predicates(
    statement: &str,
    definition: &Db2TableDefinition,
    inputs: &BTreeMap<String, Db2HostVariable>,
) -> Result<Vec<SqlPredicate>, HostProblem> {
    let words = sql_words(statement);
    let mut predicates = Vec::new();
    for window in words.windows(3) {
        let kind = match window[1].to_ascii_uppercase().as_str() {
            "=" => PredicateKind::Equal,
            "LIKE" => PredicateKind::Contains,
            ">=" => PredicateKind::AtLeast,
            "<=" => PredicateKind::AtMost,
            _ => continue,
        };
        let Some(host) = window[2].strip_prefix(':') else {
            continue;
        };
        let host = normalize_identifier(host);
        let variable = inputs
            .iter()
            .find(|(name, _)| normalize_identifier(name).ends_with(&host))
            .map(|(_, variable)| variable)
            .ok_or(HostProblem::Malformed)?;
        predicates.push(SqlPredicate {
            column: definition
                .column_index(window[0].rsplit('.').next().unwrap_or(&window[0]))
                .ok_or(HostProblem::Malformed)?,
            kind,
            value: trimmed(&variable.value).trim_matches('%').to_string(),
        });
    }
    Ok(predicates)
}

fn delete_is_restricted(
    state: &State,
    run: &str,
    table_name: &str,
    target: &Db2TableDefinition,
    target_row: &[Vec<u8>],
) -> Result<bool, HostProblem> {
    for (source_name, source) in &state.schemas {
        let source_rows = read_table(state, run, source_name)?;
        for foreign_key in source.foreign_keys.iter().filter(|foreign_key| {
            foreign_key.delete_restrict
                && foreign_key
                    .referenced_table
                    .eq_ignore_ascii_case(table_name)
        }) {
            let source_columns = foreign_key
                .columns
                .iter()
                .map(|column| source.column_index(column).ok_or(HostProblem::Malformed))
                .collect::<Result<Vec<_>, _>>()?;
            let target_columns = foreign_key
                .referenced_columns
                .iter()
                .map(|column| target.column_index(column).ok_or(HostProblem::Malformed))
                .collect::<Result<Vec<_>, _>>()?;
            if source_rows.rows.values().any(|row| {
                source_columns
                    .iter()
                    .zip(&target_columns)
                    .all(|(source, target)| row[*source] == target_row[*target])
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn validate_foreign_keys(state: &State) -> Result<(), HostProblem> {
    for (source_name, source) in &state.schemas {
        let source_rows = state
            .tables
            .get(source_name)
            .ok_or(HostProblem::Malformed)?;
        for foreign_key in &source.foreign_keys {
            let target_name = foreign_key.referenced_table.to_ascii_uppercase();
            let target = state
                .schemas
                .get(&target_name)
                .ok_or(HostProblem::Malformed)?;
            let target_rows = state
                .tables
                .get(&target_name)
                .ok_or(HostProblem::Malformed)?;
            let source_columns = foreign_key
                .columns
                .iter()
                .map(|column| source.column_index(column).ok_or(HostProblem::Malformed))
                .collect::<Result<Vec<_>, _>>()?;
            let target_columns = foreign_key
                .referenced_columns
                .iter()
                .map(|column| target.column_index(column).ok_or(HostProblem::Malformed))
                .collect::<Result<Vec<_>, _>>()?;
            for row in source_rows.rows.values() {
                if source_columns.iter().all(|column| row[*column].is_empty()) {
                    continue;
                }
                if !target_rows.rows.values().any(|target_row| {
                    source_columns
                        .iter()
                        .zip(&target_columns)
                        .all(|(source, target)| row[*source] == target_row[*target])
                }) {
                    return Err(HostProblem::Malformed);
                }
            }
        }
    }
    Ok(())
}

fn schemas_compatible(left: &Db2TableDefinition, right: &Db2TableDefinition) -> bool {
    left.columns.len() == right.columns.len()
        && left
            .columns
            .iter()
            .zip(&right.columns)
            .all(|(left, right)| {
                normalize_identifier(&left.name) == normalize_identifier(&right.name)
            })
        && left
            .primary_key
            .iter()
            .map(|value| normalize_identifier(value))
            .eq(right
                .primary_key
                .iter()
                .map(|value| normalize_identifier(value)))
}

fn parse_create_tables(
    statement: &str,
    limits: Db2Limits,
) -> Result<Vec<Db2TableDefinition>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let mut offset = 0_usize;
    let mut definitions = Vec::new();
    while let Some(relative) = upper[offset..].find("CREATE TABLE") {
        let start = offset + relative;
        let boundary = start + "CREATE TABLE".len();
        if upper
            .as_bytes()
            .get(boundary)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            offset = boundary;
            continue;
        }
        let tail = &statement[start..];
        let name = identifier_after(tail, "TABLE").ok_or(HostProblem::Malformed)?;
        let open = tail.find('(').ok_or(HostProblem::Malformed)?;
        let close = matching_parenthesis(tail, open).ok_or(HostProblem::Malformed)?;
        let body = &tail[open + 1..close];
        let mut columns = Vec::new();
        let mut primary_key = Vec::new();
        let mut foreign_keys = Vec::new();
        for item in split_top_level(body, ',') {
            let item = item.trim();
            let item_upper = item.to_ascii_uppercase();
            if item_upper.contains("PRIMARY KEY") {
                primary_key = constraint_columns(item, "PRIMARY KEY")?;
                continue;
            }
            if item_upper.contains("FOREIGN KEY") {
                let columns = constraint_columns(item, "FOREIGN KEY")?;
                let referenced_table =
                    identifier_after(item, "REFERENCES").ok_or(HostProblem::Malformed)?;
                let references = item_upper
                    .find("REFERENCES")
                    .ok_or(HostProblem::Malformed)?;
                let referenced_columns = constraint_columns(&item[references..], "REFERENCES")?;
                foreign_keys.push(crate::Db2ForeignKeyDefinition {
                    columns,
                    referenced_table,
                    referenced_columns,
                    delete_restrict: !item_upper.contains("ON DELETE CASCADE"),
                });
                continue;
            }
            if item_upper.starts_with("CONSTRAINT ") {
                continue;
            }
            let words = item.split_whitespace().collect::<Vec<_>>();
            let name = words
                .first()
                .map(|value| value.trim_matches('"').to_string())
                .ok_or(HostProblem::Malformed)?;
            let max_bytes = sql_type_width(&item_upper).unwrap_or(limits.max_column_bytes.min(256));
            let result_encoding = if item_upper.contains("VARCHAR") {
                Db2ResultEncoding::Varchar
            } else {
                Db2ResultEncoding::Raw
            };
            columns.push(Db2ColumnDefinition {
                name,
                nullable: !item_upper.contains("NOT NULL"),
                max_bytes,
                result_encoding,
                default_value: item_upper.contains("DEFAULT").then(Vec::new),
            });
        }
        if columns.is_empty() || columns.len() > limits.max_columns {
            return Err(HostProblem::Malformed);
        }
        if primary_key.is_empty() {
            primary_key.push(columns[0].name.clone());
        }
        definitions.push(Db2TableDefinition {
            name,
            columns,
            primary_key,
            foreign_keys,
            extract: None,
        });
        offset = start + close + 1;
    }
    Ok(definitions)
}

fn constraint_columns(statement: &str, keyword: &str) -> Result<Vec<String>, HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let keyword = upper.find(keyword).ok_or(HostProblem::Malformed)? + keyword.len();
    let open = statement[keyword..]
        .find('(')
        .map(|offset| keyword + offset)
        .ok_or(HostProblem::Malformed)?;
    let close = matching_parenthesis(statement, open).ok_or(HostProblem::Malformed)?;
    Ok(statement[open + 1..close]
        .split(',')
        .map(|column| column.trim().trim_matches('"').to_string())
        .collect())
}

fn sql_type_width(definition: &str) -> Option<usize> {
    for type_name in ["VARCHAR", "CHARACTER", "CHAR", "DECIMAL"] {
        let Some(start) = definition.find(&format!("{type_name}(")) else {
            continue;
        };
        let start = start + type_name.len() + 1;
        let Some(end) = definition[start..].find(')').map(|end| start + end) else {
            continue;
        };
        if let Some(width) = definition[start..end]
            .split(',')
            .next()
            .and_then(|value| value.trim().parse().ok())
        {
            return Some(width);
        }
    }
    None
}

fn matching_parenthesis(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut quote = false;
    for (offset, character) in value[open..].char_indices() {
        match character {
            '\'' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(value: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0_usize;
    let mut depth = 0_usize;
    let mut quote = false;
    for (offset, character) in value.char_indices() {
        match character {
            '\'' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth = depth.saturating_sub(1),
            current if current == separator && !quote && depth == 0 => {
                parts.push(&value[start..offset]);
                start = offset + current.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

fn quoted_literals(statement: &str) -> Result<Vec<String>, HostProblem> {
    let bytes = statement.as_bytes();
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'\'' {
            index += 1;
            continue;
        }
        index += 1;
        let mut value = String::new();
        loop {
            let byte = *bytes.get(index).ok_or(HostProblem::Malformed)?;
            if byte == b'\'' {
                if bytes.get(index + 1) == Some(&b'\'') {
                    value.push('\'');
                    index += 2;
                    continue;
                }
                index += 1;
                break;
            }
            value.push(byte as char);
            index += 1;
        }
        values.push(value);
    }
    Ok(values)
}

fn trimmed(value: &[u8]) -> String {
    String::from_utf8_lossy(value).trim().to_string()
}

fn varchar(value: &[u8], max_bytes: usize) -> Vec<u8> {
    let width = max_bytes.min(u16::MAX as usize);
    let length = u16::try_from(value.len().min(width)).unwrap_or(u16::MAX);
    let mut output = Vec::with_capacity(width.saturating_add(2));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend(fixed(value, width));
    output
}

fn fixed(value: &[u8], length: usize) -> Vec<u8> {
    let mut output = value[..value.len().min(length)].to_vec();
    output.resize(length, b' ');
    output
}

fn success(affected_rows: u64, message: &str, rows: Vec<Db2Row>) -> Db2Result {
    Db2Result {
        sqlcode: 0,
        sqlstate: "00000".into(),
        message: message.into(),
        rows,
        affected_rows,
    }
}

fn sql_condition(sqlcode: i32, sqlstate: &str, message: &str) -> Db2Result {
    Db2Result {
        sqlcode,
        sqlstate: sqlstate.into(),
        message: message.into(),
        rows: Vec::new(),
        affected_rows: 0,
    }
}

fn request_digest(request: &Db2Request) -> [u8; 32] {
    Sha256::digest(format!("{request:?}").as_bytes()).into()
}

fn validate_state(state: &State, limits: Db2Limits) -> Result<(), HostProblem> {
    if state.tables.len() > limits.max_tables
        || state.schemas.len() > limits.max_tables
        || state.cursors.len() > limits.max_cursors
        || state.replay.len() > limits.max_replays
        || !table_map_is_valid(&state.tables, &state.schemas, limits)
        || state
            .pending
            .values()
            .any(|pending| !table_map_is_valid(&pending.tables, &state.schemas, limits))
        || state.installations.values().any(|installation| {
            installation.generation == 0
                || installation.identity.len() != 71
                || !installation.identity.starts_with("sha256:")
                || installation
                    .tables
                    .iter()
                    .any(|table| !state.schemas.contains_key(table))
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn table_map_is_valid(
    tables: &BTreeMap<String, Table>,
    schemas: &BTreeMap<String, Db2TableDefinition>,
    limits: Db2Limits,
) -> bool {
    tables.iter().all(|(name, table)| {
        table.columns > 0
            && table.columns <= limits.max_columns
            && table.rows.len() <= limits.max_rows_per_table
            && schemas
                .get(name)
                .is_none_or(|schema| schema.columns.len() == table.columns)
            && table.rows.values().all(|row| {
                row.len() == table.columns
                    && row
                        .iter()
                        .all(|column| column.len() <= limits.max_column_bytes)
            })
    })
}

fn store_error(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

struct Db2Provider {
    service: Arc<Db2Service>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for Db2Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let sequence = effect.sequence;
        let outcome = match effect.request {
            HostRequest::Db2(request) => self
                .service
                .execute(invocation, &request)
                .map(HostResult::Db2),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}

pub fn db2_providers(
    service: Arc<Db2Service>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.db2.read", "host.db2.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(Db2Provider {
                service: service.clone(),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits)
                        .expect("static Db2 capability"),
                    provider_id: "mainframe-env-db2".into(),
                    generation: "1".into(),
                    request_schema: "mainframe-env.db2-request@1".into(),
                    result_schema: "mainframe-env.db2-result@1".into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 16 * 1024 * 1024,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, TraceId,
    };
    use mainframe_env_host_api::Mutation;
    use mainframe_env_store::MemoryStore;
    use std::collections::BTreeSet;

    fn invocation(run: &str) -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("program:DB2TEST", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::from([
                    CapabilityId::new("host.db2.read", limits).unwrap(),
                    CapabilityId::new("host.db2.write", limits).unwrap(),
                ]),
                limits,
            )
            .unwrap(),
            ServiceClass::Interactive,
            0,
            10_000,
            TraceId::new(format!("trace-{run}"), limits).unwrap(),
            IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn request(
        operation: Db2Operation,
        sequence: u64,
        statement: &str,
        inputs: BTreeMap<String, Db2HostVariable>,
    ) -> Db2Request {
        let mutation = operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("db2-test-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("DB2TEST".into()),
        });
        Db2Request {
            operation,
            statement: statement.into(),
            cursor: None,
            inputs,
            outputs: Vec::new(),
            max_rows: 64,
            mutation,
        }
    }

    fn variable(value: &str) -> Db2HostVariable {
        Db2HostVariable {
            value: value.as_bytes().to_vec(),
            indicator: None,
        }
    }

    fn installed_catalog(identity_byte: u8) -> Db2CatalogGeneration {
        let parent = Db2TableDefinition {
            name: "APP.CODE".into(),
            columns: vec![
                Db2ColumnDefinition {
                    name: "CODE".into(),
                    nullable: false,
                    max_bytes: 2,
                    result_encoding: Db2ResultEncoding::Raw,
                    default_value: None,
                },
                Db2ColumnDefinition {
                    name: "DESCRIPTION".into(),
                    nullable: false,
                    max_bytes: 50,
                    result_encoding: Db2ResultEncoding::Varchar,
                    default_value: None,
                },
            ],
            primary_key: vec!["CODE".into()],
            foreign_keys: Vec::new(),
            extract: Some(crate::Db2ExtractLayout {
                fields: vec![
                    crate::Db2ExtractField {
                        column: "CODE".into(),
                        width: 2,
                    },
                    crate::Db2ExtractField {
                        column: "DESCRIPTION".into(),
                        width: 50,
                    },
                ],
                trailer: b"00000000".to_vec(),
            }),
        };
        let child = Db2TableDefinition {
            name: "APP.CODE_DETAIL".into(),
            columns: vec![
                Db2ColumnDefinition {
                    name: "CODE".into(),
                    nullable: false,
                    max_bytes: 2,
                    result_encoding: Db2ResultEncoding::Raw,
                    default_value: None,
                },
                Db2ColumnDefinition {
                    name: "DETAIL".into(),
                    nullable: false,
                    max_bytes: 8,
                    result_encoding: Db2ResultEncoding::Raw,
                    default_value: None,
                },
            ],
            primary_key: vec!["CODE".into(), "DETAIL".into()],
            foreign_keys: vec![crate::Db2ForeignKeyDefinition {
                columns: vec!["CODE".into()],
                referenced_table: "APP.CODE".into(),
                referenced_columns: vec!["CODE".into()],
                delete_restrict: true,
            }],
            extract: None,
        };
        Db2CatalogGeneration {
            application: "GENERIC-FIXTURE".into(),
            generation: 1,
            identity: format!("sha256:{identity_byte:064x}"),
            tables: vec![parent, child],
            rows: vec![
                crate::Db2SeedRow {
                    table: "APP.CODE".into(),
                    values: BTreeMap::from([
                        ("CODE".into(), b"01".to_vec()),
                        ("DESCRIPTION".into(), b"GENERIC".to_vec()),
                    ]),
                },
                crate::Db2SeedRow {
                    table: "APP.CODE_DETAIL".into(),
                    values: BTreeMap::from([
                        ("CODE".into(), b"01".to_vec()),
                        ("DETAIL".into(), b"D1".to_vec()),
                    ]),
                },
            ],
        }
    }

    #[test]
    fn selected_package_catalog_installs_generic_schema_rows_and_layout_atomically() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let catalog = installed_catalog(1);
        service.install_catalog(catalog.clone()).unwrap();
        service.install_catalog(catalog).unwrap();
        assert_eq!(service.table_rows("app.code").unwrap().len(), 1);
        let run = invocation("catalog");
        let extracted = service
            .execute(
                &run,
                &request(
                    Db2Operation::Extract,
                    100,
                    "SELECT CODE, DESCRIPTION FROM APP.CODE",
                    BTreeMap::new(),
                ),
            )
            .unwrap();
        assert_eq!(extracted.rows[0].columns[0].len(), 60);
        let restricted = service
            .execute(
                &run,
                &request(
                    Db2Operation::Delete,
                    101,
                    "DELETE FROM APP.CODE",
                    BTreeMap::from([("HOST-CODE".into(), variable("01"))]),
                ),
            )
            .unwrap();
        assert_eq!(restricted.sqlcode, -532);
        assert_eq!(
            service.install_catalog(installed_catalog(2)),
            Err(HostProblem::IdempotencyConflict)
        );
        drop(service);
        assert_eq!(
            Db2Service::open(store, Db2Limits::default())
                .unwrap()
                .table_rows("APP.CODE")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn ddl_crud_cursor_commit_rollback_conflict_and_restart_are_durable() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let admin = invocation("admin");
        let ddl = "CREATE TABLE CARDDEMO.TRANSACTION_TYPE (TR_TYPE CHAR(2) NOT NULL, TR_DESCRIPTION VARCHAR(50), PRIMARY KEY (TR_TYPE)); CREATE TABLE CARDDEMO.TRANSACTION_TYPE_CATEGORY (TRC_TYPE_CODE CHAR(2) NOT NULL, TRC_TYPE_CATEGORY CHAR(4) NOT NULL, TRC_CAT_DATA VARCHAR(50), PRIMARY KEY (TRC_TYPE_CODE, TRC_TYPE_CATEGORY), FOREIGN KEY (TRC_TYPE_CODE) REFERENCES CARDDEMO.TRANSACTION_TYPE (TR_TYPE)); INSERT INTO CARDDEMO.TRANSACTION_TYPE (TR_TYPE,TR_DESCRIPTION) SELECT '01','PURCHASE' FROM SYSIBM.SYSDUMMY1 UNION ALL SELECT '02','PAYMENT' FROM SYSIBM.SYSDUMMY1 COMMIT; INSERT INTO CARDDEMO.TRANSACTION_TYPE_CATEGORY (TRC_TYPE_CODE,TRC_TYPE_CATEGORY,TRC_CAT_DATA) SELECT '01','0001','SALES' FROM SYSIBM.SYSDUMMY1 COMMIT;";
        assert_eq!(
            service
                .execute(
                    &admin,
                    &request(Db2Operation::ExecuteScript, 1, ddl, BTreeMap::new())
                )
                .unwrap()
                .affected_rows,
            3
        );
        assert_eq!(
            service
                .table_rows("CARDDEMO.TRANSACTION_TYPE")
                .unwrap()
                .len(),
            2
        );

        let first = invocation("first");
        let values = BTreeMap::from([
            ("DCL-TR-TYPE".into(), variable("99")),
            ("DCL-TR-DESCRIPTION".into(), variable("TEMPORARY")),
        ]);
        assert_eq!(
            service
                .execute(
                    &first,
                    &request(
                        Db2Operation::Insert,
                        2,
                        "INSERT INTO CARDDEMO.TRANSACTION_TYPE",
                        values.clone(),
                    )
                )
                .unwrap()
                .sqlcode,
            0
        );
        assert_eq!(service.pending_units().unwrap(), 1);
        assert_eq!(
            service
                .execute(
                    &first,
                    &request(
                        Db2Operation::Select,
                        3,
                        "SELECT FROM CARDDEMO.TRANSACTION_TYPE",
                        BTreeMap::from([("DCL-TR-TYPE".into(), variable("99"))]),
                    )
                )
                .unwrap()
                .rows
                .len(),
            1
        );
        service
            .execute(
                &first,
                &request(Db2Operation::Rollback, 4, "ROLLBACK", BTreeMap::new()),
            )
            .unwrap();
        assert_eq!(
            service
                .execute(
                    &first,
                    &request(
                        Db2Operation::Select,
                        5,
                        "SELECT FROM CARDDEMO.TRANSACTION_TYPE",
                        BTreeMap::from([("DCL-TR-TYPE".into(), variable("99"))]),
                    )
                )
                .unwrap()
                .sqlcode,
            100
        );

        service
            .execute(
                &first,
                &request(
                    Db2Operation::Insert,
                    6,
                    "INSERT INTO CARDDEMO.TRANSACTION_TYPE",
                    values.clone(),
                ),
            )
            .unwrap();
        service
            .execute(
                &first,
                &request(Db2Operation::Commit, 7, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        drop(service);
        let restarted = Db2Service::open(store, Db2Limits::default()).unwrap();
        assert_eq!(
            restarted
                .table_rows("CARDDEMO.TRANSACTION_TYPE")
                .unwrap()
                .len(),
            3
        );

        let mut open = request(
            Db2Operation::OpenCursor,
            8,
            "SELECT TR_TYPE,TR_DESCRIPTION FROM CARDDEMO.TRANSACTION_TYPE ORDER BY TR_TYPE DESC",
            BTreeMap::from([("WS-START-KEY".into(), variable("99"))]),
        );
        open.cursor = Some("BACKWARD".into());
        restarted.execute(&first, &open).unwrap();
        let mut fetch = request(Db2Operation::FetchCursor, 9, "FETCH", BTreeMap::new());
        fetch.cursor = Some("BACKWARD".into());
        assert_eq!(
            restarted.execute(&first, &fetch).unwrap().rows[0].columns[0],
            b"99"
        );

        let left = invocation("left");
        let right = invocation("right");
        let update_values = BTreeMap::from([
            ("DCL-TR-TYPE".into(), variable("02")),
            ("DCL-TR-DESCRIPTION".into(), variable("UPDATED")),
        ]);
        restarted
            .execute(
                &left,
                &request(
                    Db2Operation::Update,
                    10,
                    "UPDATE CARDDEMO.TRANSACTION_TYPE SET TR_DESCRIPTION = :DCL-TR-DESCRIPTION",
                    update_values.clone(),
                ),
            )
            .unwrap();
        restarted
            .execute(
                &right,
                &request(
                    Db2Operation::Update,
                    11,
                    "UPDATE CARDDEMO.TRANSACTION_TYPE SET TR_DESCRIPTION = :DCL-TR-DESCRIPTION",
                    update_values,
                ),
            )
            .unwrap();
        assert_eq!(
            restarted
                .execute(
                    &left,
                    &request(Db2Operation::Commit, 12, "COMMIT", BTreeMap::new()),
                )
                .unwrap()
                .sqlcode,
            0
        );
        assert_eq!(
            restarted
                .execute(
                    &right,
                    &request(Db2Operation::Commit, 13, "COMMIT", BTreeMap::new()),
                )
                .unwrap()
                .sqlcode,
            -911
        );
        assert_eq!(
            restarted
                .execute(
                    &first,
                    &request(
                        Db2Operation::Delete,
                        14,
                        "DELETE FROM CARDDEMO.TRANSACTION_TYPE",
                        BTreeMap::from([("DCL-TR-TYPE".into(), variable("01"))]),
                    )
                )
                .unwrap()
                .sqlcode,
            -532
        );
    }
}
