use crate::model::{
    ClassDescriptor, ProfileTemplate, SecurityDatabaseLimits, SecurityDatabaseSnapshot,
    SecuritySchemaProblem,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const DATABASE_NAMESPACE: &str = "racf-database-v2";
const DATABASE_KEY: &str = "authority";
const DATABASE_ENVELOPE: &[u8] = b"MERACF2\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityDatabaseSummary {
    pub generation: u64,
    pub principals: usize,
    pub groups: usize,
    pub connections: usize,
    pub classes: usize,
    pub templates: usize,
    pub profiles: usize,
    pub acees: usize,
    pub tokens: usize,
    pub certificates: usize,
    pub keys: usize,
    pub keyrings: usize,
    pub audits: usize,
    pub transactions: usize,
    pub recovery_records: usize,
    pub migrations: usize,
}

pub struct SecurityDatabase {
    store: Arc<dyn ProviderStateStore>,
    limits: SecurityDatabaseLimits,
    writer: Mutex<()>,
}

impl SecurityDatabase {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: SecurityDatabaseLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let database = Arc::new(Self {
            store,
            limits,
            writer: Mutex::new(()),
        });
        database.initialize()?;
        database.read()?;
        Ok(database)
    }

    pub fn summary(&self) -> Result<SecurityDatabaseSummary, HostProblem> {
        let snapshot = self.read()?;
        Ok(SecurityDatabaseSummary {
            generation: snapshot.generation,
            principals: snapshot.principals.len(),
            groups: snapshot.groups.len(),
            connections: snapshot.connections.len(),
            classes: snapshot.classes.len(),
            templates: snapshot.templates.len(),
            profiles: snapshot.profiles.len(),
            acees: snapshot.acees.len(),
            tokens: snapshot.tokens.len(),
            certificates: snapshot.certificates.len(),
            keys: snapshot.keys.len(),
            keyrings: snapshot.keyrings.len(),
            audits: snapshot.audits.len(),
            transactions: snapshot.transactions.len(),
            recovery_records: snapshot.recovery.len(),
            migrations: snapshot.migrations.len(),
        })
    }

    pub fn install_profile_schemas(
        &self,
        templates: Vec<ProfileTemplate>,
        classes: Vec<ClassDescriptor>,
    ) -> Result<u64, HostProblem> {
        let templates = unique(templates, |template| template.id.clone())?;
        let classes = unique(classes, |class| class.name.clone())?;
        self.mutate_if_changed(|snapshot| {
            let mut changed = false;
            for (id, template) in templates {
                match snapshot.templates.get(&id) {
                    Some(current) if current == &template => {}
                    Some(_) => return Err(HostProblem::IdempotencyConflict),
                    None => {
                        snapshot.templates.insert(id, template);
                        changed = true;
                    }
                }
            }
            for (name, class) in classes {
                match snapshot.classes.get(&name) {
                    Some(current) if current == &class => {}
                    Some(_) => return Err(HostProblem::IdempotencyConflict),
                    None => {
                        snapshot.classes.insert(name, class);
                        changed = true;
                    }
                }
            }
            Ok(((), changed))
        })
        .map(|(_, generation)| generation)
    }

    pub(crate) fn read(&self) -> Result<SecurityDatabaseSnapshot, HostProblem> {
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        decode_snapshot(&record, self.limits)
    }

    pub(crate) fn mutate<T>(
        &self,
        change: impl FnOnce(&mut SecurityDatabaseSnapshot) -> Result<T, HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        self.mutate_if_changed(|snapshot| change(snapshot).map(|result| (result, true)))
    }

    pub(crate) fn mutate_if_changed<T>(
        &self,
        change: impl FnOnce(&mut SecurityDatabaseSnapshot) -> Result<(T, bool), HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let mut snapshot = decode_snapshot(&record, self.limits)?;
        let (result, changed) = change(&mut snapshot)?;
        if !changed {
            return Ok((result, snapshot.generation));
        }
        snapshot.generation = snapshot
            .generation
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode_snapshot(&snapshot, self.limits)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: snapshot.generation,
                    payload,
                },
                Some(record.version),
            )
            .map_err(store_problem)?;
        Ok((result, snapshot.generation))
    }

    fn initialize(&self) -> Result<(), HostProblem> {
        if self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .is_some()
        {
            return Ok(());
        }
        let snapshot = SecurityDatabaseSnapshot::default();
        let record = ProviderStateRecord {
            namespace: DATABASE_NAMESPACE.into(),
            key: DATABASE_KEY.into(),
            version: snapshot.generation,
            payload: encode_snapshot(&snapshot, self.limits)?,
        };
        match self.store.put_provider_state(record, None) {
            Ok(()) | Err(StoreError::Conflict | StoreError::AlreadyExists) => Ok(()),
            Err(problem) => Err(store_problem(problem)),
        }
    }
}

fn unique<T>(
    values: Vec<T>,
    key: impl Fn(&T) -> String,
) -> Result<BTreeMap<String, T>, HostProblem> {
    let mut result = BTreeMap::new();
    for value in values {
        if result.insert(key(&value), value).is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok(result)
}

fn encode_snapshot(
    snapshot: &SecurityDatabaseSnapshot,
    limits: SecurityDatabaseLimits,
) -> Result<Vec<u8>, HostProblem> {
    snapshot.validate(limits).map_err(schema_problem)?;
    let body = serde_json::to_vec(snapshot).map_err(|_| HostProblem::InfrastructureFailure)?;
    let total = DATABASE_ENVELOPE
        .len()
        .checked_add(body.len())
        .ok_or(HostProblem::ResourceExhausted)?;
    if total > limits.max_database_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut payload = Vec::with_capacity(total);
    payload.extend_from_slice(DATABASE_ENVELOPE);
    payload.extend_from_slice(&body);
    Ok(payload)
}

fn decode_snapshot(
    record: &ProviderStateRecord,
    limits: SecurityDatabaseLimits,
) -> Result<SecurityDatabaseSnapshot, HostProblem> {
    if record.namespace != DATABASE_NAMESPACE
        || record.key != DATABASE_KEY
        || record.payload.len() > limits.max_database_bytes
        || !record.payload.starts_with(DATABASE_ENVELOPE)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let snapshot: SecurityDatabaseSnapshot =
        serde_json::from_slice(&record.payload[DATABASE_ENVELOPE.len()..])
            .map_err(|_| HostProblem::InfrastructureFailure)?;
    snapshot.validate(limits).map_err(schema_problem)?;
    if snapshot.generation != record.version {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(snapshot)
}

fn schema_problem(problem: SecuritySchemaProblem) -> HostProblem {
    match problem {
        SecuritySchemaProblem::LimitExceeded => HostProblem::ResourceExhausted,
        SecuritySchemaProblem::MissingReference => HostProblem::NotFound,
        SecuritySchemaProblem::Duplicate => HostProblem::IdempotencyConflict,
        SecuritySchemaProblem::Malformed | SecuritySchemaProblem::SecretMaterial => {
            HostProblem::Malformed
        }
        SecuritySchemaProblem::IncompatibleVersion | SecuritySchemaProblem::Cycle => {
            HostProblem::InfrastructureFailure
        }
    }
}

pub(crate) fn store_problem(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::NotFound => HostProblem::NotFound,
        _ => HostProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccessLevel, PrincipalKind, SegmentTemplate};
    use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
    use std::collections::BTreeSet;

    fn schema() -> (ProfileTemplate, ClassDescriptor) {
        (
            ProfileTemplate {
                id: "RESOURCE".into(),
                version: 1,
                profile_kind: PrincipalKind::Undefined,
                required_segments: BTreeSet::new(),
                segments: BTreeMap::from([(
                    "BASE".into(),
                    SegmentTemplate {
                        name: "BASE".into(),
                        version: 1,
                        fields: BTreeMap::new(),
                    },
                )]),
            },
            ClassDescriptor {
                name: "DATASET".into(),
                supplied: true,
                active: true,
                generic_allowed: true,
                discrete_allowed: true,
                raclist: false,
                default_uacc: AccessLevel::None,
                max_profile_name_bytes: 44,
                posit: None,
                member_class: None,
                grouping_class: None,
                profile_template: "RESOURCE".into(),
                version: 1,
            },
        )
    }

    #[test]
    fn schema_install_is_atomic_and_idempotent() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..StoreLimits::default()
        }));
        let database = SecurityDatabase::open(store, Default::default()).unwrap();
        let (template, class) = schema();
        let generation = database
            .install_profile_schemas(vec![template.clone()], vec![class.clone()])
            .unwrap();
        assert_eq!(generation, 2);
        let replay = database
            .install_profile_schemas(vec![template], vec![class])
            .unwrap();
        assert_eq!(replay, 2);
        let summary = database.summary().unwrap();
        assert_eq!((summary.templates, summary.classes), (1, 1));
    }

    #[test]
    fn sqlite_restart_preserves_exact_schema_generation() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-database-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("racf-schema.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let generation = {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let database = SecurityDatabase::open(store, Default::default()).unwrap();
            let (template, class) = schema();
            database
                .install_profile_schemas(vec![template], vec![class])
                .unwrap()
        };
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let database = SecurityDatabase::open(store, Default::default()).unwrap();
            assert_eq!(database.summary().unwrap().generation, generation);
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn corrupted_or_oversized_snapshot_fails_closed() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..StoreLimits::default()
        }));
        let database = SecurityDatabase::open(store.clone(), Default::default()).unwrap();
        let generation = database.summary().unwrap().generation;
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: generation + 1,
                    payload: b"not-a-security-database".to_vec(),
                },
                Some(generation),
            )
            .unwrap();
        assert_eq!(database.summary(), Err(HostProblem::InfrastructureFailure));
    }
}
