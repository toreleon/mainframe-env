use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits, ServiceClass};
use mainframe_env_host_api::{
    CapabilityDescriptor, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row,
    EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest, HostResult,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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
    pending: BTreeMap<String, PendingUnit>,
    cursors: BTreeMap<String, Cursor>,
    replay: BTreeMap<String, RecordedResult>,
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
        if request.operation.is_mutating() {
            let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
            if next.replay.len() >= self.limits.max_replays {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut recorded = RecordedResult::from(&result);
            recorded.request_sha256 = request_sha256;
            next.replay.insert(key.to_string(), recorded);
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
        Db2Operation::DeclareCursor => Ok(success(0, "CURSOR DECLARED", Vec::new())),
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
    let upper = statement.to_ascii_uppercase();
    let mut affected = 0u64;
    for (name, columns) in [
        ("CARDDEMO.TRANSACTION_TYPE", 2usize),
        ("CARDDEMO.TRANSACTION_TYPE_CATEGORY", 3usize),
        ("CARDDEMO.AUTHFRDS", 26usize),
    ] {
        if upper.contains(&format!("CREATE TABLE {name}"))
            || upper.contains(&format!("CREATE TABLE  {name}"))
        {
            if state.tables.len() >= limits.max_tables && !state.tables.contains_key(name) {
                return Err(HostProblem::ResourceExhausted);
            }
            state.tables.entry(name.into()).or_insert(Table {
                columns,
                rows: BTreeMap::new(),
            });
        }
    }
    let mut rest = statement;
    while let Some(offset) = rest.to_ascii_uppercase().find("INSERT INTO") {
        rest = &rest[offset..];
        let upper_rest = rest.to_ascii_uppercase();
        let target = &upper_rest[..upper_rest.find('(').unwrap_or(upper_rest.len())];
        let (table_name, columns) = if target.contains("CARDDEMO.TRANSACTION_TYPE_CATEGORY") {
            ("CARDDEMO.TRANSACTION_TYPE_CATEGORY", 3usize)
        } else if target.contains("CARDDEMO.TRANSACTION_TYPE") {
            ("CARDDEMO.TRANSACTION_TYPE", 2usize)
        } else {
            return Err(HostProblem::Unsupported);
        };
        let end = upper_rest.find("COMMIT").unwrap_or(rest.len());
        let control = &rest[..end];
        let literals = quoted_literals(control)?;
        if literals.len() % columns != 0 {
            return Err(HostProblem::Malformed);
        }
        let table = state.tables.entry(table_name.into()).or_insert(Table {
            columns,
            rows: BTreeMap::new(),
        });
        for values in literals.chunks(columns) {
            if table.rows.len() >= limits.max_rows_per_table {
                return Err(HostProblem::ResourceExhausted);
            }
            let key = row_key(table_name, values)?;
            table.rows.insert(
                key,
                values
                    .iter()
                    .map(|value| value.as_bytes().to_vec())
                    .collect(),
            );
            affected += 1;
        }
        rest = rest.get(end..).unwrap_or_default();
        if end == 0 {
            break;
        }
    }
    state.catalog_version = state.catalog_version.saturating_add(1);
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
    let table = read_table(state, run, "CARDDEMO.TRANSACTION_TYPE")?;
    let key = key_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    match table.rows.get(&key) {
        Some(row) => Ok(success(
            0,
            "ROW",
            vec![Db2Row {
                columns: vec![row[0].clone(), varchar(&row[1])],
            }],
        )),
        None => Ok(sql_condition(100, "02000", "ROW NOT FOUND")),
    }
}

fn count(state: &State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let table = read_table(state, run, "CARDDEMO.TRANSACTION_TYPE")?;
    let type_filter = input_named(&request.inputs, "TYPE-CD-FILTER").map(trimmed);
    let desc_filter = input_named(&request.inputs, "TYPE-DESC-FILTER").map(trimmed);
    let count = table
        .rows
        .values()
        .filter(|row| {
            type_filter.as_ref().is_none_or(|filter| {
                filter.is_empty() || String::from_utf8_lossy(&row[0]).contains(filter)
            }) && desc_filter.as_ref().is_none_or(|filter| {
                filter.is_empty() || String::from_utf8_lossy(&row[1]).contains(filter)
            })
        })
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
    if request
        .statement
        .to_ascii_uppercase()
        .contains("CARDDEMO.AUTHFRDS")
    {
        return insert_authfrds(state, run, request, limits);
    }
    let key = key_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    let description = description_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    let table = write_table(state, run, "CARDDEMO.TRANSACTION_TYPE", 2)?;
    if table.rows.contains_key(&key) {
        return Ok(sql_condition(-803, "23505", "DUPLICATE KEY"));
    }
    if table.rows.len() >= limits.max_rows_per_table {
        return Err(HostProblem::ResourceExhausted);
    }
    table.rows.insert(
        key.clone(),
        vec![key.into_bytes(), description.into_bytes()],
    );
    Ok(success(1, "ROW INSERTED", Vec::new()))
}

fn update(state: &mut State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    if request
        .statement
        .to_ascii_uppercase()
        .contains("CARDDEMO.AUTHFRDS")
    {
        return update_authfrds(state, run, request);
    }
    let key = key_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    let description = description_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    let table = write_table(state, run, "CARDDEMO.TRANSACTION_TYPE", 2)?;
    let Some(row) = table.rows.get_mut(&key) else {
        return Ok(sql_condition(100, "02000", "ROW NOT FOUND"));
    };
    row[1] = description.into_bytes();
    Ok(success(1, "ROW UPDATED", Vec::new()))
}

const AUTHFRDS_COLUMNS: [&str; 26] = [
    "CARD-NUM",
    "AUTH-TS",
    "AUTH-TYPE",
    "CARD-EXPIRY-DATE",
    "MESSAGE-TYPE",
    "MESSAGE-SOURCE",
    "AUTH-ID-CODE",
    "AUTH-RESP-CODE",
    "AUTH-RESP-REASON",
    "PROCESSING-CODE",
    "TRANSACTION-AMT",
    "APPROVED-AMT",
    "MERCHANT-CATAGORY-CODE",
    "ACQR-COUNTRY-CODE",
    "POS-ENTRY-MODE",
    "MERCHANT-ID",
    "MERCHANT-NAME",
    "MERCHANT-CITY",
    "MERCHANT-STATE",
    "MERCHANT-ZIP",
    "TRANSACTION-ID",
    "MATCH-STATUS",
    "AUTH-FRAUD",
    "FRAUD-RPT-DATE",
    "ACCT-ID",
    "CUST-ID",
];

fn insert_authfrds(
    state: &mut State,
    run: &str,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Db2Result, HostProblem> {
    let card = input_named(&request.inputs, "CARD-NUM")
        .map(trimmed)
        .ok_or(HostProblem::Malformed)?;
    let timestamp = input_named(&request.inputs, "AUTH-TS")
        .map(trimmed)
        .ok_or(HostProblem::Malformed)?;
    let key = format!("{card}|{timestamp}");
    let table = write_table(state, run, "CARDDEMO.AUTHFRDS", AUTHFRDS_COLUMNS.len())?;
    if table.rows.contains_key(&key) {
        return Ok(sql_condition(-803, "23505", "DUPLICATE KEY"));
    }
    if table.rows.len() >= limits.max_rows_per_table {
        return Err(HostProblem::ResourceExhausted);
    }
    let row = AUTHFRDS_COLUMNS
        .iter()
        .map(|column| {
            input_named(&request.inputs, column)
                .map(ToOwned::to_owned)
                .unwrap_or_default()
        })
        .collect();
    table.rows.insert(key, row);
    Ok(success(1, "AUTHORIZATION ROW INSERTED", Vec::new()))
}

fn update_authfrds(
    state: &mut State,
    run: &str,
    request: &Db2Request,
) -> Result<Db2Result, HostProblem> {
    let card = input_named(&request.inputs, "CARD-NUM")
        .map(trimmed)
        .ok_or(HostProblem::Malformed)?;
    let timestamp = input_named(&request.inputs, "AUTH-TS")
        .map(trimmed)
        .ok_or(HostProblem::Malformed)?;
    let fraud = input_named(&request.inputs, "AUTH-FRAUD")
        .map(ToOwned::to_owned)
        .ok_or(HostProblem::Malformed)?;
    let table = write_table(state, run, "CARDDEMO.AUTHFRDS", AUTHFRDS_COLUMNS.len())?;
    let Some(row) = table.rows.get_mut(&format!("{card}|{timestamp}")) else {
        return Ok(sql_condition(100, "02000", "ROW NOT FOUND"));
    };
    row[22] = fraud;
    row[23] = b"CURRENT DATE".to_vec();
    Ok(success(1, "AUTHORIZATION FRAUD UPDATED", Vec::new()))
}

fn delete(state: &mut State, run: &str, request: &Db2Request) -> Result<Db2Result, HostProblem> {
    let key = key_input(&request.inputs).ok_or(HostProblem::Malformed)?;
    if read_table(state, run, "CARDDEMO.TRANSACTION_TYPE_CATEGORY").is_ok_and(|table| {
        table
            .rows
            .keys()
            .any(|category| category.starts_with(&format!("{key}|")))
    }) {
        return Ok(sql_condition(-532, "23504", "DELETE RESTRICTED"));
    }
    let table = write_table(state, run, "CARDDEMO.TRANSACTION_TYPE", 2)?;
    if table.rows.remove(&key).is_none() {
        Ok(sql_condition(100, "02000", "ROW NOT FOUND"))
    } else {
        Ok(success(1, "ROW DELETED", Vec::new()))
    }
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
    let table = read_table(state, run, "CARDDEMO.TRANSACTION_TYPE")?;
    let backward = request.statement.to_ascii_uppercase().contains(" DESC");
    let start = request
        .inputs
        .values()
        .next()
        .map(|value| trimmed(&value.value));
    let mut rows = table
        .rows
        .values()
        .filter(|row| {
            start.as_ref().is_none_or(|start| {
                start.is_empty()
                    || if backward {
                        String::from_utf8_lossy(&row[0]).as_ref() <= start.as_str()
                    } else {
                        String::from_utf8_lossy(&row[0]).as_ref() >= start.as_str()
                    }
            })
        })
        .map(|row| vec![row[0].clone(), varchar(&row[1])])
        .collect::<Vec<_>>();
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
    let category = request
        .statement
        .to_ascii_uppercase()
        .contains("TRANSACTION_TYPE_CATEGORY");
    let table_name = if category {
        "CARDDEMO.TRANSACTION_TYPE_CATEGORY"
    } else {
        "CARDDEMO.TRANSACTION_TYPE"
    };
    let table = read_table(state, run, table_name)?;
    if table.rows.len() > limits.max_rows_per_table {
        return Err(HostProblem::ResourceExhausted);
    }
    let rows = table
        .rows
        .values()
        .map(|row| {
            let mut record = Vec::with_capacity(60);
            for value in row {
                record.extend_from_slice(value);
                if category && record.len() == 6 {
                    continue;
                }
                if (!category && record.len() == 2) || (category && record.len() == 6) {
                    continue;
                }
            }
            if category {
                let mut exact = Vec::new();
                exact.extend(fixed(&row[0], 2));
                exact.extend(fixed(&row[1], 4));
                exact.extend(fixed(&row[2], 50));
                exact.extend_from_slice(b"0000");
                record = exact;
            } else {
                let mut exact = Vec::new();
                exact.extend(fixed(&row[0], 2));
                exact.extend(fixed(&row[1], 50));
                exact.extend_from_slice(b"00000000");
                record = exact;
            }
            Db2Row {
                columns: vec![record],
            }
        })
        .collect();
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
    columns: usize,
) -> Result<&'a mut Table, HostProblem> {
    let pending = state
        .pending
        .entry(run.into())
        .or_insert_with(|| PendingUnit {
            base_catalog_version: state.catalog_version,
            tables: state.tables.clone(),
        });
    Ok(pending.tables.entry(table.into()).or_insert(Table {
        columns,
        rows: BTreeMap::new(),
    }))
}

fn clear_run_cursors(state: &mut State, run: &str) {
    let prefix = format!("{run}|");
    state.cursors.retain(|key, _| !key.starts_with(&prefix));
}

fn cursor_key(run: &str, cursor: &str) -> String {
    format!("{run}|{}", cursor.to_ascii_uppercase())
}

fn row_key(table: &str, values: &[String]) -> Result<String, HostProblem> {
    match table {
        "CARDDEMO.TRANSACTION_TYPE" => values.first().cloned().ok_or(HostProblem::Malformed),
        "CARDDEMO.TRANSACTION_TYPE_CATEGORY" => Ok(format!(
            "{}|{}",
            values.first().ok_or(HostProblem::Malformed)?,
            values.get(1).ok_or(HostProblem::Malformed)?
        )),
        _ => Err(HostProblem::Unsupported),
    }
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

fn key_input(inputs: &BTreeMap<String, Db2HostVariable>) -> Option<String> {
    inputs
        .iter()
        .find(|(name, _)| {
            !name.contains("DESC")
                && (name.ends_with("TR-TYPE")
                    || name.contains("TR-TYPE-")
                    || name.contains("NUMBER"))
        })
        .or_else(|| inputs.iter().find(|(name, _)| !name.contains("DESC")))
        .map(|(_, variable)| trimmed(&variable.value))
}

fn description_input(inputs: &BTreeMap<String, Db2HostVariable>) -> Option<String> {
    inputs
        .iter()
        .find(|(name, _)| name.contains("DESC"))
        .map(|(_, variable)| {
            if variable.value.len() >= 52 {
                let length = u16::from_be_bytes([variable.value[0], variable.value[1]]) as usize;
                String::from_utf8_lossy(&variable.value[2..2 + length.min(50)])
                    .trim_end()
                    .to_string()
            } else {
                trimmed(&variable.value)
            }
        })
}

fn input_named<'a>(
    inputs: &'a BTreeMap<String, Db2HostVariable>,
    suffix: &str,
) -> Option<&'a [u8]> {
    inputs
        .iter()
        .find(|(name, _)| name.ends_with(suffix))
        .map(|(_, variable)| variable.value.as_slice())
}

fn trimmed(value: &[u8]) -> String {
    String::from_utf8_lossy(value).trim().to_string()
}

fn varchar(value: &[u8]) -> Vec<u8> {
    let length = u16::try_from(value.len().min(50)).unwrap_or(50);
    let mut output = Vec::with_capacity(52);
    output.extend_from_slice(&length.to_be_bytes());
    output.extend(fixed(value, 50));
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
        || state.cursors.len() > limits.max_cursors
        || state.replay.len() > limits.max_replays
        || state.tables.values().any(|table| {
            table.columns == 0
                || table.columns > limits.max_columns
                || table.rows.len() > limits.max_rows_per_table
                || table.rows.values().any(|row| {
                    row.len() != table.columns
                        || row
                            .iter()
                            .any(|column| column.len() > limits.max_column_bytes)
                })
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
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

    #[test]
    fn ddl_crud_cursor_commit_rollback_conflict_and_restart_are_durable() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let admin = invocation("admin");
        let ddl = "CREATE TABLE CARDDEMO.TRANSACTION_TYPE (TR_TYPE CHAR(2), TR_DESCRIPTION VARCHAR(50)); CREATE TABLE CARDDEMO.TRANSACTION_TYPE_CATEGORY (TRC_TYPE_CODE CHAR(2), TRC_TYPE_CATEGORY CHAR(4), TRC_CAT_DATA VARCHAR(50)); INSERT INTO CARDDEMO.TRANSACTION_TYPE (TR_TYPE,TR_DESCRIPTION) SELECT '01','PURCHASE' FROM SYSIBM.SYSDUMMY1 UNION ALL SELECT '02','PAYMENT' FROM SYSIBM.SYSDUMMY1 COMMIT; INSERT INTO CARDDEMO.TRANSACTION_TYPE_CATEGORY (TRC_TYPE_CODE,TRC_TYPE_CATEGORY,TRC_CAT_DATA) SELECT '01','0001','SALES' FROM SYSIBM.SYSDUMMY1 COMMIT;";
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
                    &request(Db2Operation::Insert, 2, "INSERT", values.clone())
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
                &request(Db2Operation::Insert, 6, "INSERT", values.clone()),
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
                &request(Db2Operation::Update, 10, "UPDATE", update_values.clone()),
            )
            .unwrap();
        restarted
            .execute(
                &right,
                &request(Db2Operation::Update, 11, "UPDATE", update_values),
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
                        "DELETE",
                        BTreeMap::from([("DCL-TR-TYPE".into(), variable("01"))]),
                    )
                )
                .unwrap()
                .sqlcode,
            -532
        );
    }
}
