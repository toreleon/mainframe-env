use crate::catalog::{
    Db2CatalogGeneration, Db2ColumnDefinition, Db2ResultEncoding, Db2TableDefinition,
    input_for_column, normalize_identifier, value_for_column,
};
use mainframe_env_execution_api::{
    CapabilityId, IdempotencyKey, Invocation, InvocationLimits, ServiceClass,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, Db2HostVariable, Db2Operation, Db2Request, Db2Result,
    Db2Row, EffectRequest, EffectResult, EnterpriseAuthorizer, EnterpriseResource,
    EnterpriseResourceClass, HostProblem, HostProvider, HostRequest, HostResult,
    canonical_db2_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

const STATE_NAMESPACE: &str = "db2-state";
const STATE_KEY: &str = "catalog";
const ROW_STORE_SCHEMA: &str = "mainframe-env.db2-row-store@1";
const OBJECT_ROW_SCHEMA: &str = "mainframe-env.db2-object-row@1";
const TABLE_NAMESPACE: &str = "db2-v1-table";
const SCHEMA_NAMESPACE: &str = "db2-v1-schema";
const INSTALLATION_NAMESPACE: &str = "db2-v1-installation";
const GENERATION_NAMESPACE: &str = "db2-v1-catalog-generation";
const PROVENANCE_NAMESPACE: &str = "db2-v1-table-provenance";
const LEGACY_SNAPSHOT_NAMESPACE: &str = "db2-v1-legacy-snapshot";
const PENDING_NAMESPACE: &str = "db2-v1-unit-of-work";
const CURSOR_NAMESPACE: &str = "db2-v1-cursor";
const CURSOR_DECLARATION_NAMESPACE: &str = "db2-v1-cursor-declaration";
const REPLAY_NAMESPACE: &str = "db2-v1-replay";
const MAX_RETAINED_CATALOG_GENERATIONS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2Limits {
    pub max_tables: usize,
    pub max_rows_per_table: usize,
    pub max_columns: usize,
    pub max_column_bytes: usize,
    pub max_cursors: usize,
    pub max_replays: usize,
    pub max_state_bytes: usize,
    pub max_catalog_bytes: usize,
    pub max_primary_key_columns: usize,
    pub max_foreign_keys_per_table: usize,
    pub max_foreign_key_columns: usize,
    pub max_extract_fields: usize,
    pub max_catalog_text_bytes: usize,
    pub max_catalog_nested_items: usize,
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
            max_catalog_bytes: 8 * 1024 * 1024,
            max_primary_key_columns: 64,
            max_foreign_keys_per_table: 1_024,
            max_foreign_key_columns: 64,
            max_extract_fields: 1_024,
            max_catalog_text_bytes: 4 * 1024 * 1024,
            max_catalog_nested_items: 262_144,
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
    #[serde(default)]
    base_tables: BTreeMap<String, Arc<Table>>,
    tables: BTreeMap<String, Arc<Table>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Cursor {
    rows: Vec<Vec<Vec<u8>>>,
    index: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum ReplayDigestFormat {
    #[default]
    #[serde(rename = "legacy-debug@0")]
    LegacyDebugV0,
    #[serde(rename = "mainframe-env.provider-replay-canonical@1")]
    CanonicalHostV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RecordedResult {
    #[serde(default)]
    request_digest_format: ReplayDigestFormat,
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
            request_digest_format: ReplayDigestFormat::CanonicalHostV1,
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
    tables: BTreeMap<String, Arc<Table>>,
    #[serde(default)]
    schemas: BTreeMap<String, Db2TableDefinition>,
    #[serde(default)]
    installations: BTreeMap<String, CatalogInstallation>,
    #[serde(default)]
    catalog_generations: BTreeMap<String, BTreeMap<u64, Arc<CatalogGenerationSnapshot>>>,
    #[serde(default)]
    table_provenance: BTreeMap<String, TableProvenance>,
    #[serde(default)]
    legacy_snapshots: BTreeMap<String, Arc<LegacyTableSnapshot>>,
    pending: BTreeMap<String, Arc<PendingUnit>>,
    cursors: BTreeMap<String, Arc<Cursor>>,
    #[serde(default)]
    cursor_declarations: BTreeMap<String, String>,
    replay: BTreeMap<String, Arc<RecordedResult>>,
}

impl State {
    /// Fork a transaction by sharing immutable object payloads until touched.
    fn scoped_snapshot(&self) -> Self {
        self.clone()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CatalogInstallation {
    generation: u64,
    identity: String,
    tables: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CatalogGenerationSnapshot {
    generation: u64,
    identity: String,
    schemas: BTreeMap<String, Db2TableDefinition>,
    tables: BTreeMap<String, Arc<Table>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TableProvenance {
    Legacy,
    Application { owner: String, generation: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct LegacyTableSnapshot {
    schema: Db2TableDefinition,
    table: Arc<Table>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RowStoreManifest {
    schema_version: String,
    catalog_version: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObjectRow<T> {
    schema_version: String,
    object_key: String,
    value: T,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CatalogGenerationRow {
    application: String,
    snapshot: Arc<CatalogGenerationSnapshot>,
}

type RowVersions = BTreeMap<(String, String), u64>;

struct DurableState {
    versions: RowVersions,
    state: State,
}

pub struct Db2Service {
    store: Arc<dyn ProviderStateStore>,
    limits: Db2Limits,
    durable: Mutex<DurableState>,
    authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
}

impl Db2Service {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: Db2Limits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, None)
    }

    pub fn open_authorized(
        store: Arc<dyn ProviderStateStore>,
        limits: Db2Limits,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, Some(authorizer))
    }

    fn open_inner(
        store: Arc<dyn ProviderStateStore>,
        limits: Db2Limits,
        authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
    ) -> Result<Arc<Self>, HostProblem> {
        let (state, versions) = load_or_migrate(&*store, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState { versions, state }),
            authorizer,
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
        let mut next = durable.state.scoped_snapshot();
        apply_catalog_generation(&mut next, catalog, self.limits)?;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    pub fn rollback_catalog(&self, application: &str, generation: u64) -> Result<(), HostProblem> {
        let mut durable = self.lock()?;
        if !durable.state.pending.is_empty() || !durable.state.cursors.is_empty() {
            return Err(HostProblem::Condition {
                name: "DB2-CATALOG-BUSY".into(),
                response: -904,
                response2: 0,
            });
        }
        let application = application.to_ascii_uppercase();
        let target = durable
            .state
            .catalog_generations
            .get(&application)
            .and_then(|generations| generations.get(&generation))
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = durable.state.scoped_snapshot();
        snapshot_selected_catalog(&mut next, &application)?;
        let current_tables = next
            .installations
            .get(&application)
            .map(|installation| installation.tables.clone())
            .unwrap_or_default();
        let target_tables = target.schemas.keys().cloned().collect::<BTreeSet<_>>();
        for name in current_tables.difference(&target_tables) {
            match next.table_provenance.get(name).cloned() {
                Some(TableProvenance::Legacy) => {
                    let legacy = next
                        .legacy_snapshots
                        .get(name)
                        .cloned()
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    next.schemas.insert(name.clone(), legacy.schema.clone());
                    next.tables.insert(name.clone(), legacy.table.clone());
                }
                Some(TableProvenance::Application { owner, .. }) if owner == application => {
                    next.schemas.remove(name);
                    next.tables.remove(name);
                    next.table_provenance.remove(name);
                    next.legacy_snapshots.remove(name);
                }
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        }
        for (name, schema) in &target.schemas {
            next.schemas.insert(name.clone(), schema.clone());
            if !next.table_provenance.contains_key(name) {
                let provenance = if next.legacy_snapshots.contains_key(name) {
                    TableProvenance::Legacy
                } else {
                    TableProvenance::Application {
                        owner: application.clone(),
                        generation: target.generation,
                    }
                };
                next.table_provenance.insert(name.clone(), provenance);
            }
        }
        for (name, table) in &target.tables {
            next.tables.insert(name.clone(), table.clone());
        }
        next.installations.insert(
            application,
            CatalogInstallation {
                generation: target.generation,
                identity: target.identity.clone(),
                tables: target.schemas.keys().cloned().collect(),
            },
        );
        next.catalog_version = next
            .catalog_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        validate_foreign_keys(&next)?;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &Db2Request,
    ) -> Result<Db2Result, HostProblem> {
        let mut durable = self.lock()?;
        if let Some(authorizer) = &self.authorizer {
            for resource in db2_resources(&durable.state, invocation, request, self.limits)? {
                authorizer.authorize(invocation.principal.id(), &resource)?;
            }
        }
        let request_sha256 = request_digest(request)?;
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str());
        if let Some(key) = replay_key
            && let Some(recorded) = durable.state.replay.get(key)
        {
            return match recorded.request_digest_format {
                ReplayDigestFormat::LegacyDebugV0 => Err(HostProblem::UnknownOutcome),
                ReplayDigestFormat::CanonicalHostV1
                    if recorded.request_sha256 == request_sha256 =>
                {
                    Ok(recorded.result())
                }
                ReplayDigestFormat::CanonicalHostV1 => Err(HostProblem::IdempotencyConflict),
            };
        }
        let mut next = durable.state.scoped_snapshot();
        let result = apply_request(&mut next, invocation, request, self.limits)?;
        if request.operation.is_mutating() || request.operation == Db2Operation::DeclareCursor {
            if request.operation.is_mutating() {
                let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
                if next.replay.len() >= self.limits.max_replays {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut recorded = RecordedResult::from(&result);
                recorded.request_sha256 = request_sha256;
                next.replay.insert(key.to_string(), Arc::new(recorded));
            }
            validate_state(&next, self.limits)?;
            self.persist(&mut durable, next)?;
        }
        Ok(result)
    }

    /// Bind a retained pre-canonical replay receipt to a reviewed typed request.
    ///
    /// Legacy receipts are never replayed or redispatched implicitly. The caller
    /// must attest the exact retained digest before this metadata-only migration.
    pub fn reconcile_legacy_replay(
        &self,
        key: &IdempotencyKey,
        expected_legacy_digest: [u8; 32],
        request: &Db2Request,
    ) -> Result<(), HostProblem> {
        if !request.operation.is_mutating()
            || request
                .mutation
                .as_ref()
                .map(|mutation| &mutation.idempotency_key)
                != Some(key)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let canonical = request_digest(request)?;
        let mut durable = self.lock()?;
        let retained = durable
            .state
            .replay
            .get(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        match retained.request_digest_format {
            ReplayDigestFormat::CanonicalHostV1 => {
                return if retained.request_sha256 == canonical {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            ReplayDigestFormat::LegacyDebugV0
                if retained.request_sha256 != expected_legacy_digest =>
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            ReplayDigestFormat::LegacyDebugV0 => {}
        }
        let mut next = durable.state.scoped_snapshot();
        let retained = next
            .replay
            .get_mut(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        let retained = Arc::make_mut(retained);
        retained.request_digest_format = ReplayDigestFormat::CanonicalHostV1;
        retained.request_sha256 = canonical;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
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

    pub fn table_definition(&self, table: &str) -> Result<Db2TableDefinition, HostProblem> {
        self.lock()?
            .state
            .schemas
            .get(&table.to_ascii_uppercase())
            .cloned()
            .ok_or(HostProblem::NotFound)
    }

    pub fn pending_units(&self) -> Result<usize, HostProblem> {
        Ok(self.lock()?.state.pending.len())
    }

    fn persist(&self, durable: &mut DurableState, state: State) -> Result<(), HostProblem> {
        let changes = row_changes(
            &durable.state,
            &state,
            &durable.versions,
            self.limits,
            false,
        )?;
        commit_row_changes(&*self.store, changes, &mut durable.versions)?;
        durable.state = state;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, DurableState>, HostProblem> {
        self.durable
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}

struct RowChange {
    namespace: String,
    key: String,
    next_version: Option<u64>,
    mutation: ProviderStateMutation,
}

fn load_or_migrate(
    store: &dyn ProviderStateStore,
    limits: Db2Limits,
) -> Result<(State, RowVersions), HostProblem> {
    let Some(manifest_record) = store
        .get_provider_state(STATE_NAMESPACE, STATE_KEY)
        .map_err(store_error)?
    else {
        ensure_row_namespaces_empty(store)?;
        return Ok((State::default(), RowVersions::new()));
    };
    if manifest_record.namespace != STATE_NAMESPACE
        || manifest_record.key != STATE_KEY
        || manifest_record.version == 0
        || manifest_record.payload.len() > limits.max_state_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if let Ok(manifest) = serde_json::from_slice::<RowStoreManifest>(&manifest_record.payload) {
        if manifest.schema_version != ROW_STORE_SCHEMA {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut versions = RowVersions::from([(
            (STATE_NAMESPACE.into(), STATE_KEY.into()),
            manifest_record.version,
        )]);
        let state = State {
            catalog_version: manifest.catalog_version,
            tables: load_row_map(
                store,
                TABLE_NAMESPACE,
                limits.max_tables,
                limits,
                &mut versions,
            )?,
            schemas: load_row_map(
                store,
                SCHEMA_NAMESPACE,
                limits.max_tables,
                limits,
                &mut versions,
            )?,
            installations: load_row_map(
                store,
                INSTALLATION_NAMESPACE,
                limits.max_tables,
                limits,
                &mut versions,
            )?,
            catalog_generations: load_generation_rows(store, limits, &mut versions)?,
            table_provenance: load_row_map(
                store,
                PROVENANCE_NAMESPACE,
                limits.max_tables,
                limits,
                &mut versions,
            )?,
            legacy_snapshots: load_row_map(
                store,
                LEGACY_SNAPSHOT_NAMESPACE,
                limits.max_tables,
                limits,
                &mut versions,
            )?,
            pending: load_row_map(
                store,
                PENDING_NAMESPACE,
                limits.max_cursors,
                limits,
                &mut versions,
            )?,
            cursors: load_row_map(
                store,
                CURSOR_NAMESPACE,
                limits.max_cursors,
                limits,
                &mut versions,
            )?,
            cursor_declarations: load_row_map(
                store,
                CURSOR_DECLARATION_NAMESPACE,
                limits.max_cursors,
                limits,
                &mut versions,
            )?,
            replay: load_row_map(
                store,
                REPLAY_NAMESPACE,
                limits.max_replays,
                limits,
                &mut versions,
            )?,
        };
        validate_state(&state, limits)?;
        return Ok((state, versions));
    }

    let mut legacy: State = serde_json::from_slice(&manifest_record.payload)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    migrate_table_provenance(&mut legacy)?;
    rekey_state(&mut legacy)?;
    migrate_pending_units(&mut legacy)?;
    validate_state(&legacy, limits)?;
    ensure_row_namespaces_empty(store)?;
    let mut versions = RowVersions::from([(
        (STATE_NAMESPACE.into(), STATE_KEY.into()),
        manifest_record.version,
    )]);
    let changes = row_changes(&State::default(), &legacy, &versions, limits, true)?;
    commit_row_changes(store, changes, &mut versions)?;
    Ok((legacy, versions))
}

fn ensure_row_namespaces_empty(store: &dyn ProviderStateStore) -> Result<(), HostProblem> {
    for namespace in [
        TABLE_NAMESPACE,
        SCHEMA_NAMESPACE,
        INSTALLATION_NAMESPACE,
        GENERATION_NAMESPACE,
        PROVENANCE_NAMESPACE,
        LEGACY_SNAPSHOT_NAMESPACE,
        PENDING_NAMESPACE,
        CURSOR_NAMESPACE,
        CURSOR_DECLARATION_NAMESPACE,
        REPLAY_NAMESPACE,
    ] {
        if !store
            .list_provider_state(namespace, 1)
            .map_err(store_error)?
            .is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

fn load_row_map<T: DeserializeOwned>(
    store: &dyn ProviderStateStore,
    namespace: &str,
    max: usize,
    limits: Db2Limits,
    versions: &mut RowVersions,
) -> Result<BTreeMap<String, T>, HostProblem> {
    let fetch = max.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
    let records = store
        .list_provider_state(namespace, fetch)
        .map_err(store_error)?;
    if records.len() > max {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = BTreeMap::new();
    for record in records {
        if record.namespace != namespace
            || record.key.is_empty()
            || record.version == 0
            || record.payload.len() > limits.max_state_bytes
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let row: ObjectRow<T> = serde_json::from_slice(&record.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
            return Err(HostProblem::InfrastructureFailure);
        }
        versions.insert((namespace.into(), record.key.clone()), record.version);
        if values.insert(record.key, row.value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

fn load_generation_rows(
    store: &dyn ProviderStateStore,
    limits: Db2Limits,
    versions: &mut RowVersions,
) -> Result<BTreeMap<String, BTreeMap<u64, Arc<CatalogGenerationSnapshot>>>, HostProblem> {
    let max = limits
        .max_tables
        .checked_mul(MAX_RETAINED_CATALOG_GENERATIONS)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows: BTreeMap<String, CatalogGenerationRow> =
        load_row_map(store, GENERATION_NAMESPACE, max, limits, versions)?;
    let mut generations = BTreeMap::<String, BTreeMap<u64, Arc<CatalogGenerationSnapshot>>>::new();
    for (key, row) in rows {
        let expected_key = generation_key(&row.application, row.snapshot.generation);
        if key != expected_key
            || generations
                .entry(row.application)
                .or_default()
                .insert(row.snapshot.generation, row.snapshot)
                .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(generations)
}

fn generation_key(application: &str, generation: u64) -> String {
    format!("{application}|{generation:020}")
}

fn row_changes(
    current: &State,
    next: &State,
    versions: &RowVersions,
    limits: Db2Limits,
    force_manifest_write: bool,
) -> Result<Vec<RowChange>, HostProblem> {
    let mut changes = Vec::new();
    let fenced_tables = db2_table_fences(current, next);
    let no_forced_rows = BTreeSet::new();
    let current_manifest = RowStoreManifest {
        schema_version: ROW_STORE_SCHEMA.into(),
        catalog_version: current.catalog_version,
    };
    let next_manifest = RowStoreManifest {
        schema_version: ROW_STORE_SCHEMA.into(),
        catalog_version: next.catalog_version,
    };
    if force_manifest_write
        || current_manifest != next_manifest
        || !versions.contains_key(&(STATE_NAMESPACE.into(), STATE_KEY.into()))
    {
        let payload =
            serde_json::to_vec(&next_manifest).map_err(|_| HostProblem::InfrastructureFailure)?;
        changes.push(put_row_change(
            STATE_NAMESPACE,
            STATE_KEY,
            payload,
            versions,
            limits.max_state_bytes,
        )?);
    }
    map_arc_row_changes(
        TABLE_NAMESPACE,
        &current.tables,
        &next.tables,
        &fenced_tables,
        versions,
        limits,
        &mut changes,
    )?;
    map_row_changes(
        SCHEMA_NAMESPACE,
        &current.schemas,
        &next.schemas,
        &fenced_tables,
        versions,
        limits,
        &mut changes,
    )?;
    map_row_changes(
        INSTALLATION_NAMESPACE,
        &current.installations,
        &next.installations,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    generation_row_changes(
        &current.catalog_generations,
        &next.catalog_generations,
        versions,
        limits,
        &mut changes,
    )?;
    map_row_changes(
        PROVENANCE_NAMESPACE,
        &current.table_provenance,
        &next.table_provenance,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        LEGACY_SNAPSHOT_NAMESPACE,
        &current.legacy_snapshots,
        &next.legacy_snapshots,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        PENDING_NAMESPACE,
        &current.pending,
        &next.pending,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        CURSOR_NAMESPACE,
        &current.cursors,
        &next.cursors,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    map_row_changes(
        CURSOR_DECLARATION_NAMESPACE,
        &current.cursor_declarations,
        &next.cursor_declarations,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    map_arc_row_changes(
        REPLAY_NAMESPACE,
        &current.replay,
        &next.replay,
        &no_forced_rows,
        versions,
        limits,
        &mut changes,
    )?;
    Ok(changes)
}

fn db2_table_fences(current: &State, next: &State) -> BTreeSet<String> {
    let changed = current
        .tables
        .keys()
        .chain(next.tables.keys())
        .filter(
            |name| match (current.tables.get(*name), next.tables.get(*name)) {
                (Some(current), Some(next)) => !Arc::ptr_eq(current, next),
                (None, None) => false,
                _ => true,
            },
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    let schemas = current
        .schemas
        .iter()
        .chain(&next.schemas)
        .map(|(name, schema)| (name.clone(), schema))
        .collect::<BTreeMap<_, _>>();
    let mut fenced = changed.clone();
    for name in &changed {
        if let Some(schema) = schemas.get(name) {
            fenced.extend(
                schema
                    .foreign_keys
                    .iter()
                    .map(|foreign_key| foreign_key.referenced_table.to_ascii_uppercase()),
            );
        }
        fenced.extend(
            schemas
                .iter()
                .filter(|(_, schema)| {
                    schema.foreign_keys.iter().any(|foreign_key| {
                        foreign_key.referenced_table.to_ascii_uppercase() == *name
                    })
                })
                .map(|(candidate, _)| candidate.clone()),
        );
    }
    fenced
}

fn generation_row_changes(
    current: &BTreeMap<String, BTreeMap<u64, Arc<CatalogGenerationSnapshot>>>,
    next: &BTreeMap<String, BTreeMap<u64, Arc<CatalogGenerationSnapshot>>>,
    versions: &RowVersions,
    limits: Db2Limits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    for (application, generations) in next {
        for (generation, snapshot) in generations {
            if !current
                .get(application)
                .and_then(|current| current.get(generation))
                .is_some_and(|current| Arc::ptr_eq(current, snapshot))
            {
                let key = generation_key(application, *generation);
                changes.push(put_row_change(
                    GENERATION_NAMESPACE,
                    &key,
                    encode_object_row(
                        &key,
                        &CatalogGenerationRow {
                            application: application.clone(),
                            snapshot: snapshot.clone(),
                        },
                    )?,
                    versions,
                    limits.max_state_bytes,
                )?);
            }
        }
    }
    for (application, generations) in current {
        for generation in generations.keys().filter(|generation| {
            next.get(application)
                .and_then(|next| next.get(generation))
                .is_none()
        }) {
            let key = generation_key(application, *generation);
            changes.push(delete_row_change(GENERATION_NAMESPACE, &key, versions)?);
        }
    }
    Ok(())
}

fn map_row_changes<T: Serialize + PartialEq>(
    namespace: &str,
    current: &BTreeMap<String, T>,
    next: &BTreeMap<String, T>,
    forced: &BTreeSet<String>,
    versions: &RowVersions,
    limits: Db2Limits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    for (key, value) in next {
        if forced.contains(key) || current.get(key) != Some(value) {
            changes.push(put_row_change(
                namespace,
                key,
                encode_object_row(key, value)?,
                versions,
                limits.max_state_bytes,
            )?);
        }
    }
    for key in current.keys().filter(|key| !next.contains_key(*key)) {
        changes.push(delete_row_change(namespace, key, versions)?);
    }
    Ok(())
}

fn map_arc_row_changes<T: Serialize>(
    namespace: &str,
    current: &BTreeMap<String, Arc<T>>,
    next: &BTreeMap<String, Arc<T>>,
    forced: &BTreeSet<String>,
    versions: &RowVersions,
    limits: Db2Limits,
    changes: &mut Vec<RowChange>,
) -> Result<(), HostProblem> {
    for (key, value) in next {
        if forced.contains(key)
            || !current
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, value))
        {
            changes.push(put_row_change(
                namespace,
                key,
                encode_object_row(key, value)?,
                versions,
                limits.max_state_bytes,
            )?);
        }
    }
    for key in current.keys().filter(|key| !next.contains_key(*key)) {
        changes.push(delete_row_change(namespace, key, versions)?);
    }
    Ok(())
}

fn encode_object_row<T: Serialize>(key: &str, value: &T) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(&ObjectRow {
        schema_version: OBJECT_ROW_SCHEMA.into(),
        object_key: key.into(),
        value,
    })
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn put_row_change(
    namespace: &str,
    key: &str,
    payload: Vec<u8>,
    versions: &RowVersions,
    max_state_bytes: usize,
) -> Result<RowChange, HostProblem> {
    if payload.len() > max_state_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let current = versions.get(&(namespace.into(), key.into())).copied();
    let next = current
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(RowChange {
        namespace: namespace.into(),
        key: key.into(),
        next_version: Some(next),
        mutation: ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: next,
                payload,
            },
            expected_version: current,
        }),
    })
}

fn delete_row_change(
    namespace: &str,
    key: &str,
    versions: &RowVersions,
) -> Result<RowChange, HostProblem> {
    let version = versions
        .get(&(namespace.into(), key.into()))
        .copied()
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(RowChange {
        namespace: namespace.into(),
        key: key.into(),
        next_version: None,
        mutation: ProviderStateMutation::Delete {
            namespace: namespace.into(),
            key: key.into(),
            expected_version: version,
        },
    })
}

fn commit_row_changes(
    store: &dyn ProviderStateStore,
    changes: Vec<RowChange>,
    versions: &mut RowVersions,
) -> Result<(), HostProblem> {
    if changes.is_empty() {
        return Ok(());
    }
    let applied = changes
        .iter()
        .map(|change| {
            (
                (change.namespace.clone(), change.key.clone()),
                change.next_version,
            )
        })
        .collect::<Vec<_>>();
    store
        .mutate_provider_states_atomic(changes.into_iter().map(|change| change.mutation).collect())
        .map_err(store_error)?;
    for (key, version) in applied {
        match version {
            Some(version) => {
                versions.insert(key, version);
            }
            None => {
                versions.remove(&key);
            }
        }
    }
    Ok(())
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
    snapshot_selected_catalog(state, &application)?;
    let currently_owned = state
        .installations
        .get(&application)
        .map(|installation| installation.tables.clone())
        .unwrap_or_default();
    let retained = state
        .catalog_generations
        .get(&application)
        .map_or(0, BTreeMap::len);
    if !state
        .catalog_generations
        .get(&application)
        .is_some_and(|generations| generations.contains_key(&catalog.generation))
        && retained >= MAX_RETAINED_CATALOG_GENERATIONS
    {
        return Err(HostProblem::ResourceExhausted);
    }
    for definition in &catalog.tables {
        let name = definition.normalized_name();
        let existed_before_adoption =
            state.tables.contains_key(&name) && !currently_owned.contains(&name);
        if existed_before_adoption {
            if !matches!(
                state.table_provenance.get(&name),
                Some(TableProvenance::Legacy)
            ) {
                return Err(HostProblem::IdempotencyConflict);
            }
            if !state.legacy_snapshots.contains_key(&name) {
                let schema = state
                    .schemas
                    .get(&name)
                    .cloned()
                    .ok_or(HostProblem::Malformed)?;
                let table = state
                    .tables
                    .get(&name)
                    .cloned()
                    .ok_or(HostProblem::Malformed)?;
                state.legacy_snapshots.insert(
                    name.clone(),
                    Arc::new(LegacyTableSnapshot { schema, table }),
                );
            }
        } else if !state.tables.contains_key(&name) {
            state.table_provenance.insert(
                name.clone(),
                TableProvenance::Application {
                    owner: application.clone(),
                    generation: catalog.generation,
                },
            );
        }
        if let Some(existing) = state.schemas.get(&name) {
            if !schemas_compatible(existing, definition) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let table = state.tables.get(&name).ok_or(HostProblem::Malformed)?;
            if table
                .rows
                .values()
                .any(|row| validate_row(definition, row, limits).is_err())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        state.schemas.insert(name.clone(), definition.clone());
        state.tables.entry(name).or_insert_with(|| {
            Arc::new(Table {
                columns: definition.columns.len(),
                rows: BTreeMap::new(),
            })
        });
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
                    .or_else(|| column.default_value.clone())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let key = row_key(definition, &values)?;
        let table = state
            .tables
            .get_mut(&table_name)
            .ok_or(HostProblem::Malformed)?;
        let table = Arc::make_mut(table);
        if table.rows.len() >= limits.max_rows_per_table {
            return Err(HostProblem::ResourceExhausted);
        }
        match table.rows.get(&key) {
            Some(existing) if existing != &values => {
                return Err(HostProblem::IdempotencyConflict);
            }
            Some(_) => {}
            None => {
                table.rows.insert(key, values);
            }
        }
    }
    validate_foreign_keys(state)?;
    let snapshot = catalog_snapshot(state, catalog.generation, &catalog.identity, &table_names)?;
    state
        .catalog_generations
        .entry(application.clone())
        .or_default()
        .insert(catalog.generation, Arc::new(snapshot));
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

fn snapshot_selected_catalog(state: &mut State, application: &str) -> Result<(), HostProblem> {
    let Some(selected) = state.installations.get(application).cloned() else {
        return Ok(());
    };
    let snapshot = catalog_snapshot(
        state,
        selected.generation,
        &selected.identity,
        &selected.tables,
    )?;
    state
        .catalog_generations
        .entry(application.to_string())
        .or_default()
        .insert(selected.generation, Arc::new(snapshot));
    Ok(())
}

fn catalog_snapshot(
    state: &State,
    generation: u64,
    identity: &str,
    names: &BTreeSet<String>,
) -> Result<CatalogGenerationSnapshot, HostProblem> {
    let schemas = names
        .iter()
        .map(|name| {
            state
                .schemas
                .get(name)
                .cloned()
                .map(|schema| (name.clone(), schema))
                .ok_or(HostProblem::Malformed)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let tables = names
        .iter()
        .map(|name| {
            state
                .tables
                .get(name)
                .cloned()
                .map(|table| (name.clone(), table))
                .ok_or(HostProblem::Malformed)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(CatalogGenerationSnapshot {
        generation,
        identity: identity.into(),
        schemas,
        tables,
    })
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

fn db2_resources(
    state: &State,
    invocation: &Invocation,
    request: &Db2Request,
    limits: Db2Limits,
) -> Result<Vec<EnterpriseResource>, HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let intent = if request.operation.is_mutating() {
        AccessIntent::Update
    } else {
        AccessIntent::Read
    };
    let mut tables = BTreeSet::new();
    match request.operation {
        Db2Operation::ExecuteScript => {
            tables.extend(
                parse_create_tables(&request.statement, limits)?
                    .into_iter()
                    .map(|definition| definition.normalized_name()),
            );
            let words = sql_words(&request.statement);
            for pair in words.windows(2) {
                if pair[0].eq_ignore_ascii_case("INTO") {
                    tables.insert(pair[1].trim_matches('"').to_ascii_uppercase());
                }
            }
        }
        Db2Operation::FreePlans => {
            return Ok(vec![EnterpriseResource::new(
                EnterpriseResourceClass::Db2Plan,
                "ALL",
                AccessIntent::Control,
            )?]);
        }
        Db2Operation::Select | Db2Operation::Count | Db2Operation::Extract => {
            if request
                .statement
                .to_ascii_uppercase()
                .split_whitespace()
                .eq(["SELECT", "1"])
            {
                tables.insert("SYSIBM.SYSDUMMY1".into());
            } else {
                tables.insert(table_for_operation(&request.statement, "FROM")?);
            }
        }
        Db2Operation::Insert => {
            tables.insert(table_for_operation(&request.statement, "INTO")?);
        }
        Db2Operation::Update => {
            tables.insert(table_for_operation(&request.statement, "UPDATE")?);
        }
        Db2Operation::Delete => {
            tables.insert(table_for_operation(&request.statement, "FROM")?);
        }
        Db2Operation::DeclareCursor => {
            tables.insert(table_for_operation(&request.statement, "FROM")?);
        }
        Db2Operation::OpenCursor | Db2Operation::FetchCursor | Db2Operation::CloseCursor => {
            let statement = if request.statement.to_ascii_uppercase().contains(" FROM ") {
                Some(request.statement.as_str())
            } else {
                request.cursor.as_deref().and_then(|cursor| {
                    state
                        .cursor_declarations
                        .get(&cursor_key(run, cursor))
                        .map(String::as_str)
                })
            }
            .ok_or(HostProblem::Malformed)?;
            tables.insert(table_for_operation(statement, "FROM")?);
        }
        Db2Operation::Commit | Db2Operation::Rollback => {
            if let Some(pending) = state.pending.get(run) {
                tables.extend(pending.tables.keys().cloned());
            }
        }
    }
    if tables.is_empty() {
        return Ok(vec![EnterpriseResource::new(
            EnterpriseResourceClass::Db2UnitOfWork,
            "CURRENT",
            AccessIntent::Update,
        )?]);
    }
    tables
        .into_iter()
        .map(|table| EnterpriseResource::new(EnterpriseResourceClass::Db2Table, table, intent))
        .collect()
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
            if !ddl_redeclaration_compatible(installed, &definition) {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else {
            state.schemas.insert(name.clone(), definition.clone());
            changed = true;
        }
        state.tables.entry(name).or_insert_with(|| {
            Arc::new(Table {
                columns: definition.columns.len(),
                rows: BTreeMap::new(),
            })
        });
        state
            .table_provenance
            .entry(definition.normalized_name())
            .or_insert(TableProvenance::Legacy);
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
            let table = Arc::make_mut(table);
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
    let primary_key = definition
        .primary_key_indices()?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if assignments
        .iter()
        .any(|(index, _)| primary_key.contains(index))
    {
        return Ok(sql_condition(
            -798,
            "428C9",
            "PRIMARY KEY UPDATE IS NOT SUPPORTED",
        ));
    }
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
    let key_index = *definition
        .primary_key_indices()?
        .first()
        .ok_or(HostProblem::Malformed)?;
    let start = request
        .inputs
        .values()
        .next()
        .map(|value| predicate_operand(&definition.columns[key_index], &value.value))
        .transpose()?;
    let mut ordered = table.rows.values().collect::<Vec<_>>();
    ordered.sort_by(|left, right| compare_primary_key_rows(definition, left, right));
    let mut rows = ordered
        .into_iter()
        .filter(|row| {
            start.as_ref().is_none_or(|start| {
                start.bytes().is_empty()
                    || if backward {
                        row[key_index].as_slice() <= start.bytes()
                    } else {
                        row[key_index].as_slice() >= start.bytes()
                    }
            })
        })
        .map(|row| result_row(definition, row, &columns).map(|row| row.columns))
        .collect::<Result<Vec<_>, _>>()?;
    if backward {
        rows.reverse();
    }
    state.cursors.insert(
        cursor_key(run, cursor_name),
        Arc::new(Cursor { rows, index: 0 }),
    );
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
    let cursor = Arc::make_mut(cursor);
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
    if pending
        .base_tables
        .iter()
        .any(|(name, base)| state.tables.get(name).is_none_or(|current| current != base))
    {
        clear_run_cursors(state, run);
        return Ok(sql_condition(-911, "40001", "SERIALIZATION CONFLICT"));
    }
    for (name, table) in &pending.tables {
        state.tables.insert(name.clone(), table.clone());
    }
    validate_foreign_keys(state)?;
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
        .and_then(|pending| pending.tables.get(table))
        .or_else(|| state.tables.get(table))
        .map(Arc::as_ref)
        .ok_or(HostProblem::NotFound)
}

fn write_table<'a>(
    state: &'a mut State,
    run: &str,
    table: &str,
) -> Result<&'a mut Table, HostProblem> {
    let original = state
        .tables
        .get(table)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let pending = state.pending.entry(run.into()).or_insert_with(|| {
        Arc::new(PendingUnit {
            base_catalog_version: state.catalog_version,
            base_tables: BTreeMap::new(),
            tables: BTreeMap::new(),
        })
    });
    let pending = Arc::make_mut(pending);
    pending
        .base_tables
        .entry(table.into())
        .or_insert_with(|| original.clone());
    pending.tables.entry(table.into()).or_insert(original);
    pending
        .tables
        .get_mut(table)
        .map(Arc::make_mut)
        .ok_or(HostProblem::NotFound)
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
                .filter(|value| !value.is_empty())
                .map(|value| key_component(value))
                .ok_or(HostProblem::Malformed)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn key_component(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(16usize.saturating_add(value.len().saturating_mul(2)));
    encoded.push_str(&format!("{:016x}:", value.len()));
    for byte in value {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn rekey_state(state: &mut State) -> Result<(), HostProblem> {
    rekey_table_map(&mut state.tables, &state.schemas)?;
    for pending in state.pending.values_mut() {
        let pending = Arc::make_mut(pending);
        rekey_table_map(&mut pending.base_tables, &state.schemas)?;
        rekey_table_map(&mut pending.tables, &state.schemas)?;
    }
    for generations in state.catalog_generations.values_mut() {
        for snapshot in generations.values_mut() {
            let snapshot = Arc::make_mut(snapshot);
            rekey_table_map(&mut snapshot.tables, &snapshot.schemas)?;
        }
    }
    for snapshot in state.legacy_snapshots.values_mut() {
        let snapshot = Arc::make_mut(snapshot);
        let mut rows = BTreeMap::new();
        for row in snapshot.table.rows.values() {
            let key = row_key(&snapshot.schema, row)?;
            if rows.insert(key, row.clone()).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Arc::make_mut(&mut snapshot.table).rows = rows;
    }
    Ok(())
}

fn migrate_pending_units(state: &mut State) -> Result<(), HostProblem> {
    for pending in state.pending.values_mut() {
        let pending = Arc::make_mut(pending);
        pending.tables.retain(|name, table| {
            state
                .tables
                .get(name)
                .is_none_or(|committed| committed != table)
        });
        pending.base_tables = pending
            .tables
            .keys()
            .map(|name| {
                state
                    .tables
                    .get(name)
                    .cloned()
                    .map(|table| (name.clone(), table))
                    .ok_or(HostProblem::InfrastructureFailure)
            })
            .collect::<Result<_, _>>()?;
    }
    Ok(())
}

fn migrate_table_provenance(state: &mut State) -> Result<(), HostProblem> {
    let owners = state
        .installations
        .iter()
        .flat_map(|(owner, installation)| {
            installation.tables.iter().map(move |table| {
                (
                    table.clone(),
                    TableProvenance::Application {
                        owner: owner.clone(),
                        generation: installation.generation,
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    for name in state.tables.keys() {
        state
            .table_provenance
            .entry(name.clone())
            .or_insert_with(|| owners.get(name).cloned().unwrap_or(TableProvenance::Legacy));
    }
    if state
        .table_provenance
        .keys()
        .any(|name| !state.tables.contains_key(name))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn rekey_table_map(
    tables: &mut BTreeMap<String, Arc<Table>>,
    schemas: &BTreeMap<String, Db2TableDefinition>,
) -> Result<(), HostProblem> {
    for (name, table) in tables {
        let Some(schema) = schemas.get(name) else {
            continue;
        };
        let mut rows = BTreeMap::new();
        for row in table.rows.values() {
            let key = row_key(schema, row)?;
            if rows.insert(key, row.clone()).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Arc::make_mut(table).rows = rows;
    }
    Ok(())
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
    let decoded = match column.result_encoding {
        Db2ResultEncoding::Raw => value.to_vec(),
        Db2ResultEncoding::Varchar => {
            let declared =
                (value.len() >= 2).then(|| u16::from_be_bytes([value[0], value[1]]) as usize);
            if declared
                .is_some_and(|declared| declared <= value.len() - 2 && declared <= column.max_bytes)
            {
                let declared = declared.expect("checked above");
                let mut value = value[2..2 + declared].to_vec();
                while value.last() == Some(&b' ') {
                    value.pop();
                }
                value
            } else if value.first() != Some(&0) && value.len() <= column.max_bytes {
                value
                    .iter()
                    .rposition(|byte| *byte != b' ')
                    .map_or_else(Vec::new, |end| value[..=end].to_vec())
            } else {
                return Err(HostProblem::Malformed);
            }
        }
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
            let index = definition
                .column_index(column)
                .ok_or(HostProblem::Malformed)?;
            input_for_column(inputs, column)
                .map(|variable| decode_host_value(&definition.columns[index], &variable.value))
                .transpose()
        })
        .collect::<Result<Vec<_>, HostProblem>>()?;
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .filter(|parts| parts.iter().all(|part| !part.is_empty()))
        .map(|parts| {
            parts
                .iter()
                .map(|part| key_component(part))
                .collect::<Vec<_>>()
                .join("|")
        })
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
        let variable = inputs
            .iter()
            .find(|(name, _)| normalize_identifier(name).ends_with(&host))
            .map(|(_, variable)| variable)
            .ok_or(HostProblem::Malformed)?;
        let index = definition
            .column_index(column)
            .ok_or(HostProblem::Malformed)?;
        let value = decode_host_value(&definition.columns[index], &variable.value)?;
        if value.is_empty() {
            return Err(HostProblem::Malformed);
        }
        parts.push(key_component(&value));
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
    value: PredicateOperand,
}

impl SqlPredicate {
    fn matches(&self, row: &[Vec<u8>]) -> bool {
        let value = row.get(self.column).map(Vec::as_slice).unwrap_or_default();
        self.value.bytes().is_empty()
            || match self.kind {
                PredicateKind::Equal => value == self.value.bytes(),
                PredicateKind::Contains => value
                    .windows(self.value.bytes().len())
                    .any(|window| window == self.value.bytes()),
                PredicateKind::AtLeast => value >= self.value.bytes(),
                PredicateKind::AtMost => value <= self.value.bytes(),
            }
    }
}

enum PredicateOperand {
    Raw(Vec<u8>),
    Varchar(Vec<u8>),
}

impl PredicateOperand {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Raw(value) | Self::Varchar(value) => value,
        }
    }
}

fn predicate_operand(
    column: &Db2ColumnDefinition,
    value: &[u8],
) -> Result<PredicateOperand, HostProblem> {
    let value = decode_host_value(column, value)?;
    Ok(match column.result_encoding {
        Db2ResultEncoding::Raw => PredicateOperand::Raw(value),
        Db2ResultEncoding::Varchar => PredicateOperand::Varchar(value),
    })
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
        let column = definition
            .column_index(window[0].rsplit('.').next().unwrap_or(&window[0]))
            .ok_or(HostProblem::Malformed)?;
        if matches!(kind, PredicateKind::Contains)
            && definition.columns[column].result_encoding == Db2ResultEncoding::Raw
        {
            return Err(HostProblem::Malformed);
        }
        let mut value = predicate_operand(&definition.columns[column], &variable.value)?;
        if matches!(kind, PredicateKind::Contains)
            && let PredicateOperand::Varchar(bytes) = &mut value
        {
            if bytes.first() == Some(&b'%') {
                bytes.remove(0);
            }
            if bytes.last() == Some(&b'%') {
                bytes.pop();
            }
        }
        predicates.push(SqlPredicate {
            column,
            kind,
            value,
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
    normalize_identifier(&left.name) == normalize_identifier(&right.name)
        && left.columns.len() == right.columns.len()
        && left
            .columns
            .iter()
            .zip(&right.columns)
            .all(|(left, right)| {
                normalize_identifier(&left.name) == normalize_identifier(&right.name)
                    && left.nullable == right.nullable
                    && left.max_bytes == right.max_bytes
                    && left.result_encoding == right.result_encoding
                    && left.default_value == right.default_value
            })
        && left
            .primary_key
            .iter()
            .map(|value| normalize_identifier(value))
            .eq(right
                .primary_key
                .iter()
                .map(|value| normalize_identifier(value)))
        && left.foreign_keys.len() == right.foreign_keys.len()
        && left
            .foreign_keys
            .iter()
            .zip(&right.foreign_keys)
            .all(|(left, right)| {
                left.columns
                    .iter()
                    .map(|value| normalize_identifier(value))
                    .eq(right
                        .columns
                        .iter()
                        .map(|value| normalize_identifier(value)))
                    && normalize_identifier(&left.referenced_table)
                        == normalize_identifier(&right.referenced_table)
                    && left
                        .referenced_columns
                        .iter()
                        .map(|value| normalize_identifier(value))
                        .eq(right
                            .referenced_columns
                            .iter()
                            .map(|value| normalize_identifier(value)))
                    && left.delete_restrict == right.delete_restrict
            })
        && extract_layouts_compatible(left.extract.as_ref(), right.extract.as_ref())
}

fn extract_layouts_compatible(
    left: Option<&crate::Db2ExtractLayout>,
    right: Option<&crate::Db2ExtractLayout>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.trailer == right.trailer
                && left.fields.len() == right.fields.len()
                && left.fields.iter().zip(&right.fields).all(|(left, right)| {
                    normalize_identifier(&left.column) == normalize_identifier(&right.column)
                        && left.width == right.width
                })
        }
        _ => false,
    }
}

fn ddl_redeclaration_compatible(
    installed: &Db2TableDefinition,
    declared: &Db2TableDefinition,
) -> bool {
    let mut declared = declared.clone();
    // Static SQL DDL has no syntax for the package-owned extract projection.
    // A redeclaration may preserve that field, but every SQL-expressible field
    // must still be exactly compatible.
    declared.extract = installed.extract.clone();
    schemas_compatible(installed, &declared)
}

fn compare_primary_key_rows(
    definition: &Db2TableDefinition,
    left: &[Vec<u8>],
    right: &[Vec<u8>],
) -> Ordering {
    definition
        .primary_key_indices()
        .unwrap_or_default()
        .into_iter()
        .map(|index| left[index].cmp(&right[index]))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
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
    apply_alter_foreign_keys(statement, &mut definitions, limits)?;
    Ok(definitions)
}

fn apply_alter_foreign_keys(
    statement: &str,
    definitions: &mut [Db2TableDefinition],
    limits: Db2Limits,
) -> Result<(), HostProblem> {
    let upper = statement.to_ascii_uppercase();
    let mut offset = 0_usize;
    while let Some(relative) = upper[offset..].find("ALTER TABLE") {
        let start = offset + relative;
        let tail = &statement[start..];
        let end = tail.find(';').unwrap_or(tail.len());
        let clause = &tail[..end];
        let clause_upper = clause.to_ascii_uppercase();
        if clause_upper.contains("FOREIGN KEY") {
            let table = identifier_after(clause, "TABLE")
                .ok_or(HostProblem::Malformed)?
                .to_ascii_uppercase();
            let columns = constraint_columns(clause, "FOREIGN KEY")?;
            let referenced_table =
                identifier_after(clause, "REFERENCES").ok_or(HostProblem::Malformed)?;
            let references = clause_upper
                .find("REFERENCES")
                .ok_or(HostProblem::Malformed)?;
            let referenced_columns = constraint_columns(&clause[references..], "REFERENCES")?;
            let definition = definitions
                .iter_mut()
                .find(|definition| definition.normalized_name() == table)
                .ok_or(HostProblem::Malformed)?;
            if definition.foreign_keys.len() >= limits.max_foreign_keys_per_table {
                return Err(HostProblem::ResourceExhausted);
            }
            definition
                .foreign_keys
                .push(crate::Db2ForeignKeyDefinition {
                    columns,
                    referenced_table,
                    referenced_columns,
                    delete_restrict: !clause_upper.contains("ON DELETE CASCADE"),
                });
        }
        offset = start.saturating_add(end.max("ALTER TABLE".len()));
        if offset >= statement.len() {
            break;
        }
    }
    Ok(())
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

fn request_digest(request: &Db2Request) -> Result<[u8; 32], HostProblem> {
    canonical_db2_request_digest(request)
}

fn validate_state(state: &State, limits: Db2Limits) -> Result<(), HostProblem> {
    if state.tables.len() > limits.max_tables
        || state.schemas.len() > limits.max_tables
        || state.installations.len() > limits.max_tables
        || state.catalog_generations.len() > limits.max_tables
        || state.tables.keys().collect::<BTreeSet<_>>()
            != state.schemas.keys().collect::<BTreeSet<_>>()
        || state.pending.len() > limits.max_cursors
        || state.cursors.len() > limits.max_cursors
        || state.cursor_declarations.len() > limits.max_cursors
        || state.replay.len() > limits.max_replays
        || state.pending.keys().any(String::is_empty)
        || state.cursors.keys().any(String::is_empty)
        || state.cursor_declarations.keys().any(String::is_empty)
        || state
            .pending
            .values()
            .any(|pending| pending.base_catalog_version > state.catalog_version)
        || state
            .cursors
            .values()
            .any(|cursor| cursor.index > cursor.rows.len())
        || state.replay.keys().any(String::is_empty)
        || state.table_provenance.keys().collect::<BTreeSet<_>>()
            != state.tables.keys().collect::<BTreeSet<_>>()
        || state.legacy_snapshots.len() > limits.max_tables
        || state.legacy_snapshots.iter().any(|(name, snapshot)| {
            !matches!(
                state.table_provenance.get(name),
                Some(TableProvenance::Legacy)
            ) || snapshot.schema.normalized_name() != *name
                || !table_map_is_valid(
                    &BTreeMap::from([(name.clone(), snapshot.table.clone())]),
                    &BTreeMap::from([(name.clone(), snapshot.schema.clone())]),
                    limits,
                )
        })
        || !table_map_is_valid(&state.tables, &state.schemas, limits)
        || state.pending.values().any(|pending| {
            pending.base_tables.keys().collect::<BTreeSet<_>>()
                != pending.tables.keys().collect::<BTreeSet<_>>()
                || !table_map_is_valid(&pending.base_tables, &state.schemas, limits)
                || !table_map_is_valid(&pending.tables, &state.schemas, limits)
        })
        || state.installations.values().any(|installation| {
            installation.generation == 0
                || installation.identity.len() != 71
                || !installation.identity.starts_with("sha256:")
                || installation
                    .tables
                    .iter()
                    .any(|table| !state.schemas.contains_key(table))
        })
        || state.catalog_generations.values().any(|generations| {
            generations.len() > MAX_RETAINED_CATALOG_GENERATIONS
                || generations.iter().any(|(generation, snapshot)| {
                    *generation == 0
                        || *generation != snapshot.generation
                        || snapshot.identity.len() != 71
                        || !snapshot.identity.starts_with("sha256:")
                        || snapshot.schemas.keys().collect::<BTreeSet<_>>()
                            != snapshot.tables.keys().collect::<BTreeSet<_>>()
                        || !table_map_is_valid(&snapshot.tables, &snapshot.schemas, limits)
                })
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn table_map_is_valid(
    tables: &BTreeMap<String, Arc<Table>>,
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
            && table.rows.iter().all(|(key, row)| {
                row.len() == table.columns
                    && row
                        .iter()
                        .all(|column| column.len() <= limits.max_column_bytes)
                    && schemas
                        .get(name)
                        .is_none_or(|schema| row_key(schema, row).as_ref() == Ok(key))
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct DenyEnterprise {
        seen: Mutex<Vec<EnterpriseResource>>,
    }

    impl EnterpriseAuthorizer for DenyEnterprise {
        fn authorize(
            &self,
            _: &PrincipalId,
            resource: &EnterpriseResource,
        ) -> Result<(), HostProblem> {
            self.seen.lock().unwrap().push(resource.clone());
            Err(HostProblem::Unauthorized)
        }
    }

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

    #[test]
    fn db2_table_denial_precedes_mutation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(DenyEnterprise::default());
        let service =
            Db2Service::open_authorized(store, Db2Limits::default(), policy.clone()).unwrap();
        service.install_catalog(installed_catalog(1)).unwrap();
        let denied = service.execute(
            &invocation("deny-db2"),
            &request(
                Db2Operation::Insert,
                700,
                "INSERT INTO APP.CODE",
                BTreeMap::from([
                    ("CODE".into(), variable("70")),
                    ("DESCRIPTION".into(), varchar_variable("DENIED")),
                ]),
            ),
        );
        assert_eq!(denied, Err(HostProblem::Unauthorized));
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 1);
        assert_eq!(
            policy.seen.lock().unwrap().as_slice(),
            &[EnterpriseResource::new(
                EnterpriseResourceClass::Db2Table,
                "APP.CODE",
                AccessIntent::Update,
            )
            .unwrap()]
        );
    }

    fn retain_as_legacy_replay(
        store: &dyn ProviderStateStore,
        key: &IdempotencyKey,
        digest: [u8; 32],
    ) {
        let row = store
            .get_provider_state(REPLAY_NAMESPACE, key.as_str())
            .unwrap()
            .unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let replay = state["value"].as_object_mut().unwrap();
        replay.remove("request_digest_format");
        replay.insert("request_sha256".into(), serde_json::json!(digest));
        let version = row.version;
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: version + 1,
                    payload: serde_json::to_vec(&state).unwrap(),
                    ..row
                },
                Some(version),
            )
            .unwrap();
    }

    fn variable(value: &str) -> Db2HostVariable {
        Db2HostVariable {
            value: value.as_bytes().to_vec(),
            indicator: None,
        }
    }

    fn varchar_variable(value: &str) -> Db2HostVariable {
        let mut bytes = u16::try_from(value.len()).unwrap().to_be_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        Db2HostVariable {
            value: bytes,
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

    fn catalog_with_independent_note(identity_byte: u8) -> Db2CatalogGeneration {
        let mut catalog = installed_catalog(identity_byte);
        catalog.tables.push(Db2TableDefinition {
            name: "APP.NOTE".into(),
            columns: vec![
                Db2ColumnDefinition {
                    name: "ID".into(),
                    nullable: false,
                    max_bytes: 2,
                    result_encoding: Db2ResultEncoding::Raw,
                    default_value: None,
                },
                Db2ColumnDefinition {
                    name: "VALUE".into(),
                    nullable: false,
                    max_bytes: 8,
                    result_encoding: Db2ResultEncoding::Raw,
                    default_value: None,
                },
            ],
            primary_key: vec!["ID".into()],
            foreign_keys: Vec::new(),
            extract: None,
        });
        catalog
    }

    fn binary_catalog() -> Db2CatalogGeneration {
        Db2CatalogGeneration {
            application: "BINARY-FIXTURE".into(),
            generation: 1,
            identity: format!("sha256:{:064x}", 777),
            tables: vec![Db2TableDefinition {
                name: "APP.BINARY".into(),
                columns: vec![
                    Db2ColumnDefinition {
                        name: "KEY_BYTES".into(),
                        nullable: false,
                        max_bytes: 4,
                        result_encoding: Db2ResultEncoding::Raw,
                        default_value: None,
                    },
                    Db2ColumnDefinition {
                        name: "PAYLOAD".into(),
                        nullable: false,
                        max_bytes: 8,
                        result_encoding: Db2ResultEncoding::Raw,
                        default_value: Some(vec![b' ', 0xff, b' ']),
                    },
                    Db2ColumnDefinition {
                        name: "LABEL".into(),
                        nullable: false,
                        max_bytes: 8,
                        result_encoding: Db2ResultEncoding::Varchar,
                        default_value: Some(b" D ".to_vec()),
                    },
                ],
                primary_key: vec!["KEY_BYTES".into()],
                foreign_keys: Vec::new(),
                extract: None,
            }],
            rows: vec![crate::Db2SeedRow {
                table: "APP.BINARY".into(),
                values: BTreeMap::from([("KEY_BYTES".into(), vec![0xff, b' '])]),
            }],
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
    fn compatible_legacy_rows_survive_install_upgrade_restart_and_rollback() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let admin = invocation("legacy-catalog");
        let ddl = "CREATE TABLE APP.CODE (CODE CHAR(2) NOT NULL, DESCRIPTION VARCHAR(50) NOT NULL, PRIMARY KEY (CODE)); CREATE TABLE APP.CODE_DETAIL (CODE CHAR(2) NOT NULL, DETAIL CHAR(8) NOT NULL, PRIMARY KEY (CODE, DETAIL), FOREIGN KEY (CODE) REFERENCES APP.CODE (CODE)); INSERT INTO APP.CODE (CODE,DESCRIPTION) SELECT '99','LEGACY' FROM SYSIBM.SYSDUMMY1 COMMIT;";
        service
            .execute(
                &admin,
                &request(Db2Operation::ExecuteScript, 200, ddl, BTreeMap::new()),
            )
            .unwrap();
        let definitions = vec![
            service.table_definition("APP.CODE").unwrap(),
            service.table_definition("APP.CODE_DETAIL").unwrap(),
        ];
        let generation = |number, identity, extra: Option<(&str, &str)>| {
            let mut rows = vec![crate::Db2SeedRow {
                table: "APP.CODE".into(),
                values: BTreeMap::from([
                    ("CODE".into(), b"01".to_vec()),
                    ("DESCRIPTION".into(), b"GENERIC".to_vec()),
                ]),
            }];
            if let Some((code, description)) = extra {
                rows.push(crate::Db2SeedRow {
                    table: "APP.CODE".into(),
                    values: BTreeMap::from([
                        ("CODE".into(), code.as_bytes().to_vec()),
                        ("DESCRIPTION".into(), description.as_bytes().to_vec()),
                    ]),
                });
            }
            Db2CatalogGeneration {
                application: "GENERIC-FIXTURE".into(),
                generation: number,
                identity: format!("sha256:{identity:064x}"),
                tables: definitions.clone(),
                rows,
            }
        };
        service.install_catalog(generation(1, 1, None)).unwrap();
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 2);
        service
            .install_catalog(generation(2, 2, Some(("02", "UPGRADE"))))
            .unwrap();
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 3);
        drop(service);

        let restarted = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        assert_eq!(restarted.table_rows("APP.CODE").unwrap().len(), 3);
        restarted.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
        assert_eq!(restarted.table_rows("APP.CODE").unwrap().len(), 2);
        drop(restarted);

        let rolled_back = Db2Service::open(store, Db2Limits::default()).unwrap();
        let rows = rolled_back.table_rows("APP.CODE").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .any(|row| row[0] == b"99" && row[1] == b"LEGACY")
        );
        assert!(!rows.iter().any(|row| row[0] == b"02"));
    }

    #[test]
    fn primary_key_updates_are_rejected_without_rekey_or_duplicate_corruption() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store, Db2Limits::default()).unwrap();
        service.install_catalog(installed_catalog(1)).unwrap();
        let run = invocation("primary-key-update");
        service
            .execute(
                &run,
                &request(
                    Db2Operation::Insert,
                    210,
                    "INSERT INTO APP.CODE",
                    BTreeMap::from([
                        ("CODE".into(), variable("02")),
                        ("DESCRIPTION".into(), varchar_variable("SECOND")),
                    ]),
                ),
            )
            .unwrap();
        service
            .execute(
                &run,
                &request(Db2Operation::Commit, 211, "COMMIT", BTreeMap::new()),
            )
            .unwrap();

        let rejected = service
            .execute(
                &run,
                &request(
                    Db2Operation::Update,
                    212,
                    "UPDATE APP.CODE SET CODE = :NEW-CODE WHERE CODE = :OLD-CODE",
                    BTreeMap::from([
                        ("NEW-CODE".into(), variable("02")),
                        ("OLD-CODE".into(), variable("01")),
                    ]),
                ),
            )
            .unwrap();
        assert_eq!(
            (rejected.sqlcode, rejected.sqlstate.as_str()),
            (-798, "428C9")
        );
        assert_eq!(service.pending_units().unwrap(), 0);
        for code in ["01", "02"] {
            let selected = service
                .execute(
                    &run,
                    &request(
                        Db2Operation::Select,
                        213 + u64::from(code == "02"),
                        "SELECT CODE, DESCRIPTION FROM APP.CODE WHERE CODE = :LOOKUP-CODE",
                        BTreeMap::from([("LOOKUP-CODE".into(), variable(code))]),
                    ),
                )
                .unwrap();
            assert_eq!(selected.sqlcode, 0);
            assert_eq!(selected.rows[0].columns[0], code.as_bytes());
        }
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 2);
    }

    #[test]
    fn raw_host_bytes_and_catalog_defaults_survive_insert_update_read_and_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        service.install_catalog(binary_catalog()).unwrap();
        let run = invocation("binary-raw");

        let seeded = service
            .execute(
                &run,
                &request(
                    Db2Operation::Select,
                    220,
                    "SELECT KEY_BYTES, PAYLOAD, LABEL FROM APP.BINARY WHERE KEY_BYTES = :LOOKUP",
                    BTreeMap::from([(
                        "LOOKUP".into(),
                        Db2HostVariable {
                            value: vec![0xff, b' '],
                            indicator: None,
                        },
                    )]),
                ),
            )
            .unwrap();
        assert_eq!(seeded.rows[0].columns[0], vec![0xff, b' ']);
        assert_eq!(seeded.rows[0].columns[1], vec![b' ', 0xff, b' ']);
        assert_eq!(&seeded.rows[0].columns[2][..5], &[0, 3, b' ', b'D', b' ']);

        let key = vec![b' ', 0xfe, b' '];
        let inserted_payload = vec![0, 0xff, b' ', b'A', b' '];
        service
            .execute(
                &run,
                &request(
                    Db2Operation::Insert,
                    221,
                    "INSERT INTO APP.BINARY",
                    BTreeMap::from([
                        (
                            "KEY_BYTES".into(),
                            Db2HostVariable {
                                value: key.clone(),
                                indicator: None,
                            },
                        ),
                        (
                            "PAYLOAD".into(),
                            Db2HostVariable {
                                value: inserted_payload.clone(),
                                indicator: None,
                            },
                        ),
                        (
                            "LABEL".into(),
                            Db2HostVariable {
                                value: vec![0, 3, b'X', b' ', b'Y'],
                                indicator: None,
                            },
                        ),
                    ]),
                ),
            )
            .unwrap();
        service
            .execute(
                &run,
                &request(Db2Operation::Commit, 222, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        let selected = service
            .execute(
                &run,
                &request(
                    Db2Operation::Select,
                    223,
                    "SELECT KEY_BYTES, PAYLOAD FROM APP.BINARY WHERE KEY_BYTES = :LOOKUP",
                    BTreeMap::from([(
                        "LOOKUP".into(),
                        Db2HostVariable {
                            value: key.clone(),
                            indicator: None,
                        },
                    )]),
                ),
            )
            .unwrap();
        assert_eq!(
            selected.rows[0].columns,
            vec![key.clone(), inserted_payload]
        );

        let updated_payload = vec![b' ', 0x80, b' '];
        service
            .execute(
                &run,
                &request(
                    Db2Operation::Update,
                    224,
                    "UPDATE APP.BINARY SET PAYLOAD = :NEW_PAYLOAD WHERE KEY_BYTES = :LOOKUP",
                    BTreeMap::from([
                        (
                            "LOOKUP".into(),
                            Db2HostVariable {
                                value: key.clone(),
                                indicator: None,
                            },
                        ),
                        (
                            "NEW_PAYLOAD".into(),
                            Db2HostVariable {
                                value: updated_payload.clone(),
                                indicator: None,
                            },
                        ),
                    ]),
                ),
            )
            .unwrap();
        service
            .execute(
                &run,
                &request(Db2Operation::Commit, 225, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        drop(service);

        let restarted = Db2Service::open(store, Db2Limits::default()).unwrap();
        let selected = restarted
            .execute(
                &run,
                &request(
                    Db2Operation::Select,
                    226,
                    "SELECT KEY_BYTES, PAYLOAD FROM APP.BINARY WHERE KEY_BYTES = :LOOKUP",
                    BTreeMap::from([(
                        "LOOKUP".into(),
                        Db2HostVariable {
                            value: key.clone(),
                            indicator: None,
                        },
                    )]),
                ),
            )
            .unwrap();
        assert_eq!(selected.rows[0].columns, vec![key, updated_payload]);
    }

    #[test]
    fn rollback_removes_newer_owned_dependents_and_restart_remains_referentially_valid() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let mut first = installed_catalog(31);
        first.tables.truncate(1);
        first.rows.truncate(1);
        service.install_catalog(first).unwrap();

        let mut second = installed_catalog(32);
        second.generation = 2;
        second.rows.extend([
            crate::Db2SeedRow {
                table: "APP.CODE".into(),
                values: BTreeMap::from([
                    ("CODE".into(), b"02".to_vec()),
                    ("DESCRIPTION".into(), b"SECOND".to_vec()),
                ]),
            },
            crate::Db2SeedRow {
                table: "APP.CODE_DETAIL".into(),
                values: BTreeMap::from([
                    ("CODE".into(), b"02".to_vec()),
                    ("DETAIL".into(), b"D2".to_vec()),
                ]),
            },
        ]);
        service.install_catalog(second).unwrap();
        assert_eq!(service.table_rows("APP.CODE_DETAIL").unwrap().len(), 2);
        service.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
        assert_eq!(
            service.table_rows("APP.CODE_DETAIL"),
            Err(HostProblem::NotFound)
        );
        drop(service);

        let restarted = Db2Service::open(store, Db2Limits::default()).unwrap();
        assert_eq!(restarted.table_rows("APP.CODE").unwrap().len(), 1);
        assert_eq!(
            restarted.table_rows("APP.CODE_DETAIL"),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn rollback_restores_adopted_legacy_data_but_removes_new_owned_dependents() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let admin = invocation("legacy-owner");
        service
            .execute(
                &admin,
                &request(
                    Db2Operation::ExecuteScript,
                    300,
                    "CREATE TABLE LEGACY.PARENT (ID CHAR(2) NOT NULL, VALUE CHAR(8) NOT NULL, PRIMARY KEY (ID)); INSERT INTO LEGACY.PARENT (ID,VALUE) VALUES ('01','ORIGINAL');",
                    BTreeMap::new(),
                ),
            )
            .unwrap();
        let legacy_definition = service.table_definition("LEGACY.PARENT").unwrap();

        let mut first = installed_catalog(41);
        first.tables.truncate(1);
        first.rows.truncate(1);
        service.install_catalog(first.clone()).unwrap();

        let dependent = Db2TableDefinition {
            name: "APP.LEGACY_CHILD".into(),
            columns: vec![Db2ColumnDefinition {
                name: "PARENT_ID".into(),
                nullable: false,
                max_bytes: 2,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            }],
            primary_key: vec!["PARENT_ID".into()],
            foreign_keys: vec![crate::Db2ForeignKeyDefinition {
                columns: vec!["PARENT_ID".into()],
                referenced_table: "LEGACY.PARENT".into(),
                referenced_columns: vec!["ID".into()],
                delete_restrict: true,
            }],
            extract: None,
        };
        let mut second = first;
        second.generation = 2;
        second.identity = format!("sha256:{:064x}", 42);
        second.tables.extend([legacy_definition, dependent]);
        second.rows.push(crate::Db2SeedRow {
            table: "APP.LEGACY_CHILD".into(),
            values: BTreeMap::from([("PARENT_ID".into(), b"01".to_vec())]),
        });
        service.install_catalog(second).unwrap();

        service
            .execute(
                &admin,
                &request(
                    Db2Operation::Update,
                    301,
                    "UPDATE LEGACY.PARENT SET VALUE = :VALUE WHERE ID = :ID",
                    BTreeMap::from([
                        ("ID".into(), variable("01")),
                        ("VALUE".into(), variable("CHANGED")),
                    ]),
                ),
            )
            .unwrap();
        service
            .execute(
                &admin,
                &request(Db2Operation::Commit, 302, "COMMIT", BTreeMap::new()),
            )
            .unwrap();

        service.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
        assert_eq!(
            service.table_rows("LEGACY.PARENT").unwrap()[0][1],
            b"ORIGINAL"
        );
        assert_eq!(
            service.table_rows("APP.LEGACY_CHILD"),
            Err(HostProblem::NotFound)
        );
        drop(service);

        let restarted = Db2Service::open(store, Db2Limits::default()).unwrap();
        assert_eq!(
            restarted.table_rows("LEGACY.PARENT").unwrap()[0][1],
            b"ORIGINAL"
        );
        assert_eq!(
            restarted.table_rows("APP.LEGACY_CHILD"),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn raw_predicates_and_cursor_positions_are_byte_exact() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        let mut catalog = binary_catalog();
        catalog.rows = [vec![0xfe], vec![0xff], b"A".to_vec(), b" A ".to_vec()]
            .into_iter()
            .map(|key| crate::Db2SeedRow {
                table: "APP.BINARY".into(),
                values: BTreeMap::from([("KEY_BYTES".into(), key)]),
            })
            .collect();
        service.install_catalog(catalog).unwrap();
        drop(service);

        let service = Db2Service::open(store, Db2Limits::default()).unwrap();
        let run = invocation("raw-query");
        for key in [vec![0xfe], vec![0xff], b"A".to_vec(), b" A ".to_vec()] {
            let counted = service
                .execute(
                    &run,
                    &request(
                        Db2Operation::Count,
                        310 + u64::from(key[0]),
                        "SELECT COUNT(*) FROM APP.BINARY WHERE KEY_BYTES = :LOOKUP",
                        BTreeMap::from([(
                            "LOOKUP".into(),
                            Db2HostVariable {
                                value: key,
                                indicator: None,
                            },
                        )]),
                    ),
                )
                .unwrap();
            assert_eq!(counted.rows[0].columns[0], b"1");
        }
        for (operator, expected) in [(">=", b"2".as_slice()), ("<=", b"3".as_slice())] {
            let counted = service
                .execute(
                    &run,
                    &request(
                        Db2Operation::Count,
                        600 + u64::from(operator.as_bytes()[0]),
                        &format!(
                            "SELECT COUNT(*) FROM APP.BINARY WHERE KEY_BYTES {operator} :LOOKUP"
                        ),
                        BTreeMap::from([(
                            "LOOKUP".into(),
                            Db2HostVariable {
                                value: vec![0xfe],
                                indicator: None,
                            },
                        )]),
                    ),
                )
                .unwrap();
            assert_eq!(counted.rows[0].columns[0], expected);
        }
        assert_eq!(
            service.execute(
                &run,
                &request(
                    Db2Operation::Count,
                    699,
                    "SELECT COUNT(*) FROM APP.BINARY WHERE LABEL = :LABEL",
                    BTreeMap::from([(
                        "LABEL".into(),
                        Db2HostVariable {
                            value: b"malformed-varchar".to_vec(),
                            indicator: None,
                        },
                    )]),
                ),
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            service.execute(
                &run,
                &request(
                    Db2Operation::Count,
                    697,
                    "SELECT COUNT(*) FROM APP.BINARY WHERE LABEL = :LABEL",
                    BTreeMap::from([(
                        "LABEL".into(),
                        Db2HostVariable {
                            value: vec![0, 9, b'X'],
                            indicator: None,
                        },
                    )]),
                ),
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            service
                .execute(
                    &run,
                    &request(
                        Db2Operation::Count,
                        698,
                        "SELECT COUNT(*) FROM APP.BINARY WHERE LABEL = :LABEL",
                        BTreeMap::from([(
                            "LABEL".into(),
                            Db2HostVariable {
                                value: b"FIXED   ".to_vec(),
                                indicator: None,
                            },
                        )]),
                    ),
                )
                .unwrap()
                .rows[0]
                .columns[0],
            b"0"
        );

        let updated = service
            .execute(
                &run,
                &request(
                    Db2Operation::Update,
                    704,
                    "UPDATE APP.BINARY SET PAYLOAD = :PAYLOAD WHERE KEY_BYTES = :KEY_BYTES",
                    BTreeMap::from([
                        (
                            "KEY_BYTES".into(),
                            Db2HostVariable {
                                value: vec![0xfe],
                                indicator: None,
                            },
                        ),
                        (
                            "PAYLOAD".into(),
                            Db2HostVariable {
                                value: vec![b' ', 0xfe, b' '],
                                indicator: None,
                            },
                        ),
                    ]),
                ),
            )
            .unwrap();
        assert_eq!(updated.affected_rows, 1);
        service
            .execute(
                &run,
                &request(Db2Operation::Commit, 705, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        let selected = service
            .execute(
                &run,
                &request(
                    Db2Operation::Select,
                    706,
                    "SELECT PAYLOAD FROM APP.BINARY WHERE KEY_BYTES = :KEY_BYTES",
                    BTreeMap::from([(
                        "KEY_BYTES".into(),
                        Db2HostVariable {
                            value: vec![0xfe],
                            indicator: None,
                        },
                    )]),
                ),
            )
            .unwrap();
        assert_eq!(selected.rows[0].columns[0], vec![b' ', 0xfe, b' ']);

        let mut open = request(
            Db2Operation::OpenCursor,
            700,
            "SELECT KEY_BYTES FROM APP.BINARY ORDER BY KEY_BYTES",
            BTreeMap::from([(
                "START".into(),
                Db2HostVariable {
                    value: vec![0xff],
                    indicator: None,
                },
            )]),
        );
        open.cursor = Some("RAW-FORWARD".into());
        service.execute(&run, &open).unwrap();
        let mut fetch = request(Db2Operation::FetchCursor, 701, "FETCH", BTreeMap::new());
        fetch.cursor = Some("RAW-FORWARD".into());
        assert_eq!(
            service.execute(&run, &fetch).unwrap().rows[0].columns[0],
            vec![0xff]
        );

        let mut backward = request(
            Db2Operation::OpenCursor,
            702,
            "SELECT KEY_BYTES FROM APP.BINARY ORDER BY KEY_BYTES DESC",
            BTreeMap::from([(
                "START".into(),
                Db2HostVariable {
                    value: vec![0xfe],
                    indicator: None,
                },
            )]),
        );
        backward.cursor = Some("RAW-BACKWARD".into());
        service.execute(&run, &backward).unwrap();
        fetch = request(Db2Operation::FetchCursor, 703, "FETCH", BTreeMap::new());
        fetch.cursor = Some("RAW-BACKWARD".into());
        assert_eq!(
            service.execute(&run, &fetch).unwrap().rows[0].columns[0],
            vec![0xfe]
        );
    }

    #[test]
    fn schema_compatibility_covers_every_semantic_field() {
        let original = installed_catalog(51).tables;
        let assert_incompatible = |mut changed: Vec<Db2TableDefinition>| {
            assert!(!schemas_compatible(&original[0], &changed.remove(0)));
        };

        let mut changed = original.clone();
        changed[0].columns[0].name = "RENAMED".into();
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].columns[0].nullable = true;
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].columns[0].max_bytes += 1;
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].columns[1].result_encoding = Db2ResultEncoding::Raw;
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].columns[1].default_value = Some(b"DEFAULT".to_vec());
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].primary_key = vec!["DESCRIPTION".into()];
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].extract.as_mut().unwrap().fields[0].column = "DESCRIPTION".into();
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].extract.as_mut().unwrap().fields[0].width += 1;
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].extract.as_mut().unwrap().trailer.push(b'X');
        assert_incompatible(changed);
        let mut changed = original.clone();
        changed[0].extract = None;
        assert_incompatible(changed);

        let assert_child_incompatible = |changed: Db2TableDefinition| {
            assert!(!schemas_compatible(&original[1], &changed));
        };
        let mut changed = original[1].clone();
        changed.foreign_keys[0].columns = vec!["DETAIL".into()];
        assert_child_incompatible(changed);
        let mut changed = original[1].clone();
        changed.foreign_keys[0].referenced_table = "APP.CODE_DETAIL".into();
        assert_child_incompatible(changed);
        let mut changed = original[1].clone();
        changed.foreign_keys[0].referenced_columns = vec!["DESCRIPTION".into()];
        assert_child_incompatible(changed);
        let mut changed = original[1].clone();
        changed.foreign_keys[0].delete_restrict = false;
        assert_child_incompatible(changed);

        let mut normalized_case = original[0].clone();
        normalized_case.name = normalized_case.name.to_ascii_lowercase();
        normalized_case.columns[0].name = normalized_case.columns[0].name.to_ascii_lowercase();
        normalized_case.primary_key[0] = normalized_case.primary_key[0].to_ascii_lowercase();
        assert!(schemas_compatible(&original[0], &normalized_case));
    }

    #[test]
    fn ddl_redeclaration_preserves_extract_only_after_full_sql_semantic_match() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store, Db2Limits::default()).unwrap();
        service.install_catalog(installed_catalog(61)).unwrap();
        let admin = invocation("ddl-redeclaration");
        let exact = "CREATE TABLE APP.CODE (CODE CHAR(2) NOT NULL, DESCRIPTION VARCHAR(50) NOT NULL, PRIMARY KEY (CODE)); \
                     CREATE TABLE APP.CODE_DETAIL (CODE CHAR(2) NOT NULL, DETAIL CHAR(8) NOT NULL, PRIMARY KEY (CODE, DETAIL)); \
                     ALTER TABLE APP.CODE_DETAIL FOREIGN KEY (CODE) REFERENCES APP.CODE (CODE) ON DELETE RESTRICT;";
        assert_eq!(
            service
                .execute(
                    &admin,
                    &request(Db2Operation::ExecuteScript, 810, exact, BTreeMap::new())
                )
                .unwrap()
                .sqlcode,
            0
        );
        assert!(
            service
                .table_definition("APP.CODE")
                .unwrap()
                .extract
                .is_some()
        );

        let changed = exact.replace("DESCRIPTION VARCHAR(50) NOT NULL", "DESCRIPTION CHAR(50)");
        assert_eq!(
            service.execute(
                &admin,
                &request(Db2Operation::ExecuteScript, 811, &changed, BTreeMap::new())
            ),
            Err(HostProblem::IdempotencyConflict)
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
            ("DCL-TR-DESCRIPTION".into(), varchar_variable("TEMPORARY")),
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
            ("DCL-TR-DESCRIPTION".into(), varchar_variable("UPDATED")),
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

    #[test]
    fn legacy_replay_requires_attested_canonical_migration_without_redispatch() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        service.install_catalog(installed_catalog(71)).unwrap();
        let invocation = invocation("legacy-replay");
        let insert = request(
            Db2Operation::Insert,
            901,
            "INSERT INTO APP.CODE",
            BTreeMap::from([
                ("CODE".into(), variable("71")),
                ("DESCRIPTION".into(), varchar_variable("ONCE")),
            ]),
        );
        let key = insert.mutation.as_ref().unwrap().idempotency_key.clone();
        let original = service.execute(&invocation, &insert).unwrap();
        service
            .execute(
                &invocation,
                &request(Db2Operation::Commit, 902, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 2);
        drop(service);

        retain_as_legacy_replay(store.as_ref(), &key, [0x33; 32]);
        let reopened = Db2Service::open(store.clone(), Db2Limits::default()).unwrap();
        assert_eq!(
            reopened.execute(&invocation, &insert),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 2);
        assert_eq!(
            reopened.reconcile_legacy_replay(&key, [0x44; 32], &insert),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened
            .reconcile_legacy_replay(&key, [0x33; 32], &insert)
            .unwrap();
        assert_eq!(reopened.execute(&invocation, &insert), Ok(original.clone()));
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 2);
        drop(reopened);

        let restarted = Db2Service::open(store, Db2Limits::default()).unwrap();
        assert_eq!(restarted.execute(&invocation, &insert), Ok(original));
        assert_eq!(restarted.table_rows("APP.CODE").unwrap().len(), 2);
    }

    #[test]
    fn legacy_blob_migrates_atomically_to_versioned_scoped_rows_and_corruption_fails_closed() {
        let limits = Db2Limits::default();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let mut legacy = State::default();
        apply_catalog_generation(&mut legacy, catalog_with_independent_note(91), limits).unwrap();
        validate_state(&legacy, limits).unwrap();
        let legacy_payload = serde_json::to_vec(&legacy).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: legacy_payload.clone(),
                },
                None,
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: TABLE_NAMESPACE.into(),
                    key: "ORPHAN".into(),
                    version: 1,
                    payload: encode_object_row(
                        "ORPHAN",
                        &Table {
                            columns: 1,
                            rows: BTreeMap::new(),
                        },
                    )
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            Db2Service::open(store.clone(), limits),
            Err(HostProblem::InfrastructureFailure)
        ));
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap()
                .payload,
            legacy_payload
        );
        store
            .delete_provider_state(TABLE_NAMESPACE, "ORPHAN", 1)
            .unwrap();

        let service = Db2Service::open(store.clone(), limits).unwrap();
        let manifest_before = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let manifest: RowStoreManifest = serde_json::from_slice(&manifest_before.payload).unwrap();
        assert_eq!(manifest.schema_version, ROW_STORE_SCHEMA);
        assert_eq!(manifest_before.version, 2);
        assert_eq!(
            store.list_provider_state(TABLE_NAMESPACE, 4).unwrap().len(),
            3
        );
        assert_eq!(
            store
                .list_provider_state(GENERATION_NAMESPACE, 3)
                .unwrap()
                .len(),
            1
        );
        let note_before = store
            .get_provider_state(TABLE_NAMESPACE, "APP.NOTE")
            .unwrap()
            .unwrap();
        let code_version = store
            .get_provider_state(TABLE_NAMESPACE, "APP.CODE")
            .unwrap()
            .unwrap()
            .version;
        let invocation = invocation("row-scope");
        let insert = request(
            Db2Operation::Insert,
            501,
            "INSERT INTO APP.CODE",
            BTreeMap::from([
                ("CODE".into(), variable("91")),
                ("DESCRIPTION".into(), varchar_variable("SCOPED")),
            ]),
        );
        service.execute(&invocation, &insert).unwrap();
        assert!(
            store
                .get_provider_state(PENDING_NAMESPACE, invocation.run_unit_id.as_str())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest_before
        );
        service
            .execute(
                &invocation,
                &request(Db2Operation::Commit, 502, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        assert!(
            store
                .get_provider_state(PENDING_NAMESPACE, invocation.run_unit_id.as_str())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .get_provider_state(TABLE_NAMESPACE, "APP.NOTE")
                .unwrap()
                .unwrap(),
            note_before
        );
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest_before
        );
        assert_eq!(service.table_rows("APP.CODE").unwrap().len(), 2);
        assert_eq!(
            store
                .get_provider_state(TABLE_NAMESPACE, "APP.CODE")
                .unwrap()
                .unwrap()
                .version,
            code_version + 1
        );
        drop(service);

        let detail = store
            .get_provider_state(TABLE_NAMESPACE, "APP.CODE_DETAIL")
            .unwrap()
            .unwrap();
        let mut corrupt: serde_json::Value = serde_json::from_slice(&detail.payload).unwrap();
        corrupt["schema_version"] = serde_json::json!("mainframe-env.db2-object-row@999");
        let version = detail.version;
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: version + 1,
                    payload: serde_json::to_vec(&corrupt).unwrap(),
                    ..detail
                },
                Some(version),
            )
            .unwrap();
        assert!(matches!(
            Db2Service::open(store, limits),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn empty_legacy_blob_is_always_replaced_by_a_versioned_manifest() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: serde_json::to_vec(&State::default()).unwrap(),
                },
                None,
            )
            .unwrap();

        drop(Db2Service::open(store.clone(), Default::default()).unwrap());
        let manifest = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        assert_eq!(manifest.version, 2);
        assert_eq!(
            serde_json::from_slice::<RowStoreManifest>(&manifest.payload)
                .unwrap()
                .schema_version,
            ROW_STORE_SCHEMA
        );
    }

    #[test]
    fn in_flight_legacy_unit_gains_scoped_conflict_baselines_during_migration() {
        let limits = Db2Limits::default();
        let store = Arc::new(MemoryStore::new(Default::default()));
        let mut legacy = State::default();
        apply_catalog_generation(&mut legacy, installed_catalog(109), limits).unwrap();
        let invocation = invocation("legacy-pending");
        apply_request(
            &mut legacy,
            &invocation,
            &request(
                Db2Operation::Insert,
                971,
                "INSERT INTO APP.CODE",
                BTreeMap::from([
                    ("CODE".into(), variable("97")),
                    ("DESCRIPTION".into(), varchar_variable("MIGRATED")),
                ]),
            ),
            limits,
        )
        .unwrap();
        let mut legacy_payload = serde_json::to_value(&legacy).unwrap();
        legacy_payload["pending"][invocation.run_unit_id.as_str()]
            .as_object_mut()
            .unwrap()
            .remove("base_tables");
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: serde_json::to_vec(&legacy_payload).unwrap(),
                },
                None,
            )
            .unwrap();

        let service = Db2Service::open(store.clone(), limits).unwrap();
        assert_eq!(service.pending_units().unwrap(), 1);
        service
            .execute(
                &invocation,
                &request(Db2Operation::Commit, 972, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        drop(service);

        let reopened = Db2Service::open(store, limits).unwrap();
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 2);
        assert_eq!(reopened.pending_units().unwrap(), 0);
    }

    #[test]
    fn independent_table_rows_commit_from_separate_service_instances_without_global_cas() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let installer = Db2Service::open(store.clone(), Default::default()).unwrap();
        installer
            .install_catalog(catalog_with_independent_note(111))
            .unwrap();
        drop(installer);
        let manifest = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let left = Db2Service::open(store.clone(), Default::default()).unwrap();
        let right = Db2Service::open(store.clone(), Default::default()).unwrap();
        let left_worker = std::thread::spawn(move || {
            let invocation = invocation("left-table");
            left.execute(
                &invocation,
                &request(
                    Db2Operation::Insert,
                    931,
                    "INSERT INTO APP.CODE",
                    BTreeMap::from([
                        ("CODE".into(), variable("91")),
                        ("DESCRIPTION".into(), varchar_variable("LEFT")),
                    ]),
                ),
            )?;
            left.execute(
                &invocation,
                &request(Db2Operation::Commit, 932, "COMMIT", BTreeMap::new()),
            )
        });
        let right_worker = std::thread::spawn(move || {
            let invocation = invocation("right-table");
            right.execute(
                &invocation,
                &request(
                    Db2Operation::Insert,
                    941,
                    "INSERT INTO APP.NOTE",
                    BTreeMap::from([
                        ("ID".into(), variable("01")),
                        ("VALUE".into(), variable("RIGHT")),
                    ]),
                ),
            )?;
            right.execute(
                &invocation,
                &request(Db2Operation::Commit, 942, "COMMIT", BTreeMap::new()),
            )
        });
        left_worker.join().unwrap().unwrap();
        right_worker.join().unwrap().unwrap();

        let reopened = Db2Service::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 2);
        assert_eq!(reopened.table_rows("APP.NOTE").unwrap().len(), 1);
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest
        );
    }

    #[test]
    fn foreign_key_dependency_rows_fence_concurrent_related_commits() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let mut catalog = installed_catalog(112);
        catalog.rows.push(crate::Db2SeedRow {
            table: "APP.CODE".into(),
            values: BTreeMap::from([
                ("CODE".into(), b"02".to_vec()),
                ("DESCRIPTION".into(), b"SECOND".to_vec()),
            ]),
        });
        let installer = Db2Service::open(store.clone(), Default::default()).unwrap();
        installer.install_catalog(catalog).unwrap();
        drop(installer);

        let left = Db2Service::open(store.clone(), Default::default()).unwrap();
        let right = Db2Service::open(store.clone(), Default::default()).unwrap();
        let left_invocation = invocation("delete-parent");
        left.execute(
            &left_invocation,
            &request(
                Db2Operation::Delete,
                951,
                "DELETE FROM APP.CODE",
                BTreeMap::from([("CODE".into(), variable("02"))]),
            ),
        )
        .unwrap();
        let right_invocation = invocation("insert-child");
        right
            .execute(
                &right_invocation,
                &request(
                    Db2Operation::Insert,
                    961,
                    "INSERT INTO APP.CODE_DETAIL",
                    BTreeMap::from([
                        ("CODE".into(), variable("02")),
                        ("DETAIL".into(), variable("D2")),
                    ]),
                ),
            )
            .unwrap();
        left.execute(
            &left_invocation,
            &request(Db2Operation::Commit, 952, "COMMIT", BTreeMap::new()),
        )
        .unwrap();
        assert_eq!(
            right.execute(
                &right_invocation,
                &request(Db2Operation::Commit, 962, "COMMIT", BTreeMap::new()),
            ),
            Err(HostProblem::IdempotencyConflict)
        );

        let reopened = Db2Service::open(store, Default::default()).unwrap();
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 1);
        assert_eq!(reopened.table_rows("APP.CODE_DETAIL").unwrap().len(), 1);
    }

    #[test]
    fn sqlite_executes_and_reopens_the_versioned_row_layout() {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap());
        let mut legacy = State::default();
        apply_catalog_generation(&mut legacy, installed_catalog(101), Default::default()).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: serde_json::to_vec(&legacy).unwrap(),
                },
                None,
            )
            .unwrap();
        let service = Db2Service::open(store.clone(), Default::default()).unwrap();
        let invocation = invocation("sqlite-row");
        service
            .execute(
                &invocation,
                &request(
                    Db2Operation::Insert,
                    801,
                    "INSERT INTO APP.CODE",
                    BTreeMap::from([
                        ("CODE".into(), variable("81")),
                        ("DESCRIPTION".into(), varchar_variable("SQLITE")),
                    ]),
                ),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &request(Db2Operation::Commit, 802, "COMMIT", BTreeMap::new()),
            )
            .unwrap();
        drop(service);

        let reopened = Db2Service::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.table_rows("APP.CODE").unwrap().len(), 2);
        assert_eq!(
            store.list_provider_state(TABLE_NAMESPACE, 3).unwrap().len(),
            2
        );
        assert!(
            serde_json::from_slice::<RowStoreManifest>(
                &store
                    .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                    .unwrap()
                    .unwrap()
                    .payload
            )
            .is_ok()
        );
    }
}
