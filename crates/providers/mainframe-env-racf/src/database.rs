use crate::model::{
    AccessControlEntry, AccessLevel, AuditFieldValue, AuditPolicy, ClassDescriptor,
    CredentialVerifier, DecisionOutcome, DecisionReason, GroupAuthority, GroupConnection,
    GroupProfile, MigrationState, PrincipalKind, PrincipalProfile, PrincipalState, ProfileTemplate,
    RecoveryRecord, RecoveryState, ResourceProfile, SafStatus, SecurityAuditRecord,
    SecurityDatabaseLimits, SecurityDatabaseSnapshot, SecurityMigration, SecuritySchemaProblem,
    TransactionState, connection_key, profile_key,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const DATABASE_NAMESPACE: &str = "racf-database-v2";
const DATABASE_KEY: &str = "authority";
const DATABASE_ENVELOPE: &[u8] = b"MERACF2\0";
const LEGACY_MIGRATION_ID: &str = "RACF-V1-TO-V2";
const LEGACY_USER_NAMESPACE: &str = "racf-user";
const LEGACY_GROUP_NAMESPACE: &str = "racf-group";
const LEGACY_PROFILE_NAMESPACE: &str = "racf-profile";
const LEGACY_AUDIT_NAMESPACE: &str = "racf-audit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityDatabaseSummary {
    pub generation: u64,
    pub principals: usize,
    pub groups: usize,
    pub connections: usize,
    pub classes: usize,
    pub templates: usize,
    pub profiles: usize,
    pub raclist_caches: usize,
    pub acees: usize,
    pub tokens: usize,
    pub certificates: usize,
    pub keys: usize,
    pub keyrings: usize,
    pub mfa_factors: usize,
    pub identity_mappings: usize,
    pub user_associations: usize,
    pub rrsf_nodes: usize,
    pub signon_sessions: usize,
    pub audits: usize,
    pub transactions: usize,
    pub recovery_records: usize,
    pub migrations: usize,
    pub subsystem_running: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecuritySemanticProjection {
    pub domain_digests: BTreeMap<String, String>,
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
            raclist_caches: snapshot.raclist_caches.len(),
            acees: snapshot.acees.len(),
            tokens: snapshot.tokens.len(),
            certificates: snapshot.certificates.len(),
            keys: snapshot.keys.len(),
            keyrings: snapshot.keyrings.len(),
            mfa_factors: snapshot.mfa_factors.len(),
            identity_mappings: snapshot.identity_mappings.len(),
            user_associations: snapshot.user_associations.len(),
            rrsf_nodes: snapshot.rrsf_nodes.len(),
            signon_sessions: snapshot.signon_sessions.len(),
            audits: snapshot.audits.len(),
            transactions: snapshot.transactions.len(),
            recovery_records: snapshot.recovery.len(),
            migrations: snapshot.migrations.len(),
            subsystem_running: snapshot.subsystem.running,
        })
    }

    pub fn semantic_projection(&self) -> Result<SecuritySemanticProjection, HostProblem> {
        let snapshot = self.read()?;
        let mut domain_digests = BTreeMap::new();
        for (name, bytes) in [
            ("principals", semantic_bytes(&snapshot.principals)?),
            ("groups", semantic_bytes(&snapshot.groups)?),
            ("connections", semantic_bytes(&snapshot.connections)?),
            ("profiles", semantic_bytes(&snapshot.profiles)?),
            (
                "classes",
                semantic_bytes(&(&snapshot.classes, &snapshot.templates))?,
            ),
            ("caches", semantic_bytes(&snapshot.raclist_caches)?),
            (
                "policy",
                semantic_bytes(&(
                    &snapshot.policy,
                    &snapshot.database_status,
                    &snapshot.subsystem,
                ))?,
            ),
            ("acees", semantic_bytes(&snapshot.acees)?),
            ("tokens", semantic_bytes(&snapshot.tokens)?),
            ("certificates", semantic_bytes(&snapshot.certificates)?),
            ("keys", semantic_bytes(&snapshot.keys)?),
            ("keyrings", semantic_bytes(&snapshot.keyrings)?),
            ("mfa", semantic_bytes(&snapshot.mfa_factors)?),
            (
                "identity-mappings",
                semantic_bytes(&snapshot.identity_mappings)?,
            ),
            (
                "user-associations",
                semantic_bytes(&snapshot.user_associations)?,
            ),
            ("rrsf-nodes", semantic_bytes(&snapshot.rrsf_nodes)?),
            ("sessions", semantic_bytes(&snapshot.signon_sessions)?),
        ] {
            domain_digests.insert(name.into(), format!("sha256:{:x}", Sha256::digest(bytes)));
        }
        Ok(SecuritySemanticProjection { domain_digests })
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

    pub(crate) fn migrate_legacy_records(&self) -> Result<bool, HostProblem> {
        let legacy = LegacySnapshot::read(&*self.store, self.limits)?;
        if legacy.is_empty() {
            return Ok(false);
        }
        let source_digest = legacy.digest();
        self.mutate_if_changed(|snapshot| {
            if let Some(migration) = snapshot
                .migrations
                .iter()
                .find(|migration| migration.id == LEGACY_MIGRATION_ID)
            {
                return match migration.state {
                    MigrationState::Applied | MigrationState::RolledBack => Ok((false, false)),
                    MigrationState::Planned => Err(HostProblem::IdempotencyConflict),
                };
            }
            if !snapshot.principals.is_empty()
                || !snapshot.groups.is_empty()
                || !snapshot.connections.is_empty()
                || !snapshot.profiles.is_empty()
                || !snapshot.audits.is_empty()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            for group in legacy.groups.values() {
                snapshot.groups.insert(
                    group.name.clone(),
                    GroupProfile {
                        name: group.name.clone(),
                        owner: group.name.clone(),
                        superior_group: None,
                        universal: false,
                        profile_template: None,
                        segments: BTreeMap::new(),
                        version: group.version,
                    },
                );
            }
            for user in legacy.users.values() {
                let default_group = user.groups.iter().next().cloned();
                snapshot.principals.insert(
                    user.name.clone(),
                    PrincipalProfile {
                        id: user.name.clone(),
                        kind: PrincipalKind::User,
                        owner: user.name.clone(),
                        default_group,
                        state: user.state(),
                        credential: Some(CredentialVerifier {
                            algorithm: "argon2id".into(),
                            encoded_verifier: user.hash.clone(),
                            changed_tick: 0,
                            history_digests: Vec::new(),
                        }),
                        profile_template: None,
                        segments: BTreeMap::new(),
                        security_level: 0,
                        security_label: None,
                        categories: Default::default(),
                        attributes: Default::default(),
                        version: user.version,
                    },
                );
                for group in &user.groups {
                    snapshot.connections.insert(
                        connection_key(&user.name, group),
                        GroupConnection {
                            user: user.name.clone(),
                            group: group.clone(),
                            authority: GroupAuthority::Use,
                            special: false,
                            operations: false,
                            auditor: false,
                            revoked: false,
                            version: user.version,
                        },
                    );
                }
            }
            for profile in legacy.profiles.values() {
                crate::authority::install_resource_schema(
                    snapshot,
                    &profile.class,
                    self.limits.max_name_bytes,
                )?;
                snapshot.profiles.insert(
                    profile_key(&profile.class, &profile.name),
                    ResourceProfile {
                        class: profile.class.clone(),
                        name: profile.name.clone(),
                        generic: profile.name.bytes().any(|byte| matches!(byte, b'*' | b'%')),
                        owner: profile.owner.clone(),
                        uacc: profile.uacc,
                        audit: AuditPolicy::None,
                        security_level: 0,
                        security_label: None,
                        categories: Default::default(),
                        access_list: profile
                            .permissions
                            .iter()
                            .map(|(principal, access)| AccessControlEntry {
                                principal: principal.clone(),
                                access: *access,
                                when: None,
                                audit: AuditPolicy::None,
                            })
                            .collect(),
                        segments: BTreeMap::new(),
                        version: profile.version,
                    },
                );
            }
            for (index, audit) in legacy.audits.values().enumerate() {
                let reason = match audit.decision {
                    DecisionOutcome::Allow => DecisionReason::Granted,
                    DecisionOutcome::Deny | DecisionOutcome::NoDecision => {
                        DecisionReason::DefaultDeny
                    }
                };
                snapshot.audits.push(SecurityAuditRecord {
                    id: format!("MIGAUDIT{index:020}"),
                    correlation: "LEGACY-MIGRATION".into(),
                    actor: "SYSTEM".into(),
                    action: audit.action.clone(),
                    class: None,
                    resource_digest: Some(audit.resource.clone()),
                    decision: audit.decision,
                    status: legacy_status(reason),
                    fields: audit.fields.clone(),
                    tick: 0,
                });
            }
            let result_digest = snapshot_content_digest(snapshot)?;
            snapshot.migrations.push(SecurityMigration {
                id: LEGACY_MIGRATION_ID.into(),
                from_schema: "mainframe-env.racf-records@1".into(),
                to_schema: crate::SECURITY_DATABASE_SCHEMA.into(),
                state: MigrationState::Applied,
                source_digest: source_digest.clone(),
                result_digest: Some(result_digest),
            });
            Ok((true, true))
        })
        .map(|(migrated, _)| migrated)
    }

    pub fn rollback_legacy_migration(&self) -> Result<bool, HostProblem> {
        self.mutate_if_changed(|snapshot| {
            let migration_index = snapshot
                .migrations
                .iter()
                .position(|migration| migration.id == LEGACY_MIGRATION_ID)
                .ok_or(HostProblem::NotFound)?;
            if snapshot.migrations[migration_index].state == MigrationState::RolledBack {
                return Ok((false, false));
            }
            let current_digest = snapshot_content_digest(snapshot)?;
            if snapshot.migrations[migration_index].state != MigrationState::Applied
                || snapshot.migrations[migration_index]
                    .result_digest
                    .as_deref()
                    != Some(current_digest.as_str())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            snapshot.principals.clear();
            snapshot.groups.clear();
            snapshot.connections.clear();
            snapshot.profiles.clear();
            snapshot.audits.clear();
            snapshot.migrations[migration_index].state = MigrationState::RolledBack;
            Ok((true, true))
        })
        .map(|(rolled_back, _)| rolled_back)
    }

    pub(crate) fn reconcile_incomplete_transactions(&self) -> Result<usize, HostProblem> {
        self.mutate_if_changed(|snapshot| {
            let pending = snapshot
                .transactions
                .iter()
                .filter(|(_, transaction)| {
                    matches!(
                        transaction.state,
                        TransactionState::Intent | TransactionState::UnknownOutcome
                    )
                })
                .map(|(id, transaction)| {
                    (id.clone(), transaction.state, transaction.final_generation)
                })
                .collect::<Vec<_>>();
            for (transaction_id, state, final_generation) in &pending {
                let committed = *state == TransactionState::UnknownOutcome
                    && final_generation.is_some_and(|generation| generation <= snapshot.generation);
                snapshot
                    .transactions
                    .get_mut(transaction_id)
                    .ok_or(HostProblem::NotFound)?
                    .state = if committed {
                    TransactionState::Committed
                } else {
                    TransactionState::RolledBack
                };
                let recovery_id = recovery_id(transaction_id);
                let (attempt, version) =
                    snapshot
                        .recovery
                        .get(&recovery_id)
                        .map_or(Ok((1, 1)), |recovery| {
                            Ok((
                                recovery
                                    .attempt
                                    .checked_add(1)
                                    .ok_or(HostProblem::ResourceExhausted)?,
                                recovery
                                    .version
                                    .checked_add(1)
                                    .ok_or(HostProblem::ResourceExhausted)?,
                            ))
                        })?;
                snapshot.recovery.insert(
                    recovery_id.clone(),
                    RecoveryRecord {
                        id: recovery_id,
                        transaction_id: transaction_id.clone(),
                        state: RecoveryState::Reconciled,
                        attempt,
                        last_error: None,
                        version,
                    },
                );
            }
            let count = pending.len();
            Ok((count, count > 0))
        })
        .map(|(count, _)| count)
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

    pub(crate) fn mutate_retry<T>(
        &self,
        mut change: impl FnMut(&mut SecurityDatabaseSnapshot) -> Result<(T, bool), HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        const MAX_ATTEMPTS: usize = 4;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        for attempt in 0..MAX_ATTEMPTS {
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
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: snapshot.generation,
                    payload,
                },
                Some(record.version),
            ) {
                Ok(()) => return Ok((result, snapshot.generation)),
                Err(StoreError::Conflict) if attempt + 1 < MAX_ATTEMPTS => {}
                Err(problem) => return Err(store_problem(problem)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
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

struct LegacyUser {
    name: String,
    hash: String,
    expired: bool,
    revoked: bool,
    locked: bool,
    groups: std::collections::BTreeSet<String>,
    version: u64,
}

impl LegacyUser {
    const fn state(&self) -> PrincipalState {
        if self.locked {
            PrincipalState::Locked
        } else if self.revoked {
            PrincipalState::Revoked
        } else if self.expired {
            PrincipalState::PasswordExpired
        } else {
            PrincipalState::Active
        }
    }
}

struct LegacyGroup {
    name: String,
    version: u64,
}

struct LegacyProfile {
    class: String,
    name: String,
    owner: String,
    uacc: AccessLevel,
    permissions: BTreeMap<String, AccessLevel>,
    version: u64,
}

struct LegacyAudit {
    action: String,
    resource: String,
    decision: DecisionOutcome,
    fields: BTreeMap<String, AuditFieldValue>,
}

struct LegacySnapshot {
    rows: Vec<ProviderStateRecord>,
    users: BTreeMap<String, LegacyUser>,
    groups: BTreeMap<String, LegacyGroup>,
    profiles: BTreeMap<String, LegacyProfile>,
    audits: BTreeMap<String, LegacyAudit>,
}

impl LegacySnapshot {
    fn read(
        store: &dyn ProviderStateStore,
        limits: SecurityDatabaseLimits,
    ) -> Result<Self, HostProblem> {
        let mut rows = Vec::new();
        for (namespace, max) in [
            (LEGACY_USER_NAMESPACE, limits.max_principals),
            (LEGACY_GROUP_NAMESPACE, limits.max_groups),
            (LEGACY_PROFILE_NAMESPACE, limits.max_profiles),
            (LEGACY_AUDIT_NAMESPACE, limits.max_audits),
        ] {
            let probe = max.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
            let namespace_rows = match store.list_provider_state(namespace, probe) {
                Ok(rows) => rows,
                Err(StoreError::CapacityExceeded) => {
                    let rows = store
                        .list_provider_state(namespace, max)
                        .map_err(store_problem)?;
                    if rows.len() == max {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    rows
                }
                Err(problem) => return Err(store_problem(problem)),
            };
            if namespace_rows.len() > max {
                return Err(HostProblem::ResourceExhausted);
            }
            rows.extend(namespace_rows);
        }
        rows.sort_by(|left, right| {
            (&left.namespace, &left.key).cmp(&(&right.namespace, &right.key))
        });
        let mut users = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let mut profiles = BTreeMap::new();
        let mut audits = BTreeMap::new();
        for row in &rows {
            if row.version == 0 {
                return Err(HostProblem::InfrastructureFailure);
            }
            match row.namespace.as_str() {
                LEGACY_USER_NAMESPACE => {
                    let user = decode_legacy_user(row, limits)?;
                    if users.insert(user.name.clone(), user).is_some() {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                }
                LEGACY_GROUP_NAMESPACE => {
                    let group = LegacyGroup {
                        name: row.key.clone(),
                        version: row.version,
                    };
                    if groups.insert(group.name.clone(), group).is_some() {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                }
                LEGACY_PROFILE_NAMESPACE => {
                    let profile = decode_legacy_profile(row, limits)?;
                    let key = profile_key(&profile.class, &profile.name);
                    if key != row.key || profiles.insert(key, profile).is_some() {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                }
                LEGACY_AUDIT_NAMESPACE => {
                    let audit = decode_legacy_audit(row, limits)?;
                    if audits.insert(row.key.clone(), audit).is_some() {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                }
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        }
        Ok(Self {
            rows,
            users,
            groups,
            profiles,
            audits,
        })
    }

    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"mainframe-env.racf-legacy-records@1\0");
        for row in &self.rows {
            digest_part(&mut digest, row.namespace.as_bytes());
            digest_part(&mut digest, row.key.as_bytes());
            digest.update(row.version.to_be_bytes());
            digest_part(&mut digest, &row.payload);
        }
        format!("sha256:{:x}", digest.finalize())
    }
}

fn decode_legacy_user(
    record: &ProviderStateRecord,
    limits: SecurityDatabaseLimits,
) -> Result<LegacyUser, HostProblem> {
    let mut reader = LegacyReader::new(&record.payload, b"MERU1")?;
    let hash = reader.string(limits.max_value_bytes)?;
    let expired = reader.flag()?;
    let revoked = reader.flag()?;
    let locked = reader.flag()?;
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_groups {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut groups = std::collections::BTreeSet::new();
    for _ in 0..count {
        if !groups.insert(reader.string(8)?) {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    reader.finish()?;
    Ok(LegacyUser {
        name: record.key.clone(),
        hash,
        expired,
        revoked,
        locked,
        groups,
        version: record.version,
    })
}

fn decode_legacy_profile(
    record: &ProviderStateRecord,
    limits: SecurityDatabaseLimits,
) -> Result<LegacyProfile, HostProblem> {
    let mut reader = LegacyReader::new(&record.payload, b"MERP1")?;
    let class = reader.string(32)?;
    let name = reader.string(limits.max_name_bytes)?;
    let owner = reader.string(8)?;
    let uacc = legacy_access(reader.byte()?)?.unwrap_or(AccessLevel::None);
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_access_entries {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut permissions = BTreeMap::new();
    for _ in 0..count {
        let principal = reader.string(8)?;
        let access = legacy_access(reader.byte()?)?.ok_or(HostProblem::InfrastructureFailure)?;
        if permissions.insert(principal, access).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    reader.finish()?;
    Ok(LegacyProfile {
        class,
        name,
        owner,
        uacc,
        permissions,
        version: record.version,
    })
}

fn decode_legacy_audit(
    record: &ProviderStateRecord,
    limits: SecurityDatabaseLimits,
) -> Result<LegacyAudit, HostProblem> {
    let mut reader = LegacyReader::new(&record.payload, b"MERA1")?;
    let action = reader.string(limits.max_name_bytes)?;
    let resource = reader.string(limits.max_name_bytes)?;
    let decision_text = reader.string(limits.max_name_bytes)?;
    let decision = match decision_text.to_ascii_uppercase().as_str() {
        "ALLOW" | "SUCCESS" => DecisionOutcome::Allow,
        "DENY" | "FAILURE" => DecisionOutcome::Deny,
        _ => DecisionOutcome::NoDecision,
    };
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_fields_per_segment {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let name = reader.string(limits.max_name_bytes)?;
        let value = reader.string(limits.max_value_bytes)?;
        let value = if value == "<redacted>" {
            AuditFieldValue::Redacted
        } else {
            AuditFieldValue::Text(value)
        };
        if fields.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    reader.finish()?;
    Ok(LegacyAudit {
        action,
        resource,
        decision,
        fields: crate::audit::redact_fields(fields),
    })
}

struct LegacyReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> LegacyReader<'a> {
    fn new(bytes: &'a [u8], envelope: &[u8]) -> Result<Self, HostProblem> {
        if !bytes.starts_with(envelope) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Self {
            bytes,
            at: envelope.len(),
        })
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(HostProblem::ResourceExhausted)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    fn flag(&mut self) -> Result<bool, HostProblem> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn u32(&mut self) -> Result<u32, HostProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn string(&mut self, max: usize) -> Result<String, HostProblem> {
        let count = usize::try_from(self.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
        if count > max {
            return Err(HostProblem::ResourceExhausted);
        }
        String::from_utf8(self.take(count)?.to_vec())
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn finish(self) -> Result<(), HostProblem> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        }
    }
}

fn legacy_access(value: u8) -> Result<Option<AccessLevel>, HostProblem> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(AccessLevel::Execute)),
        2 => Ok(Some(AccessLevel::Read)),
        3 => Ok(Some(AccessLevel::Update)),
        4 => Ok(Some(AccessLevel::Control)),
        5 => Ok(Some(AccessLevel::Alter)),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn legacy_status(reason: DecisionReason) -> SafStatus {
    if reason == DecisionReason::Granted {
        SafStatus {
            saf_return_code: 0,
            racf_return_code: 0,
            racf_reason_code: 0,
            reason,
        }
    } else {
        SafStatus {
            saf_return_code: 8,
            racf_return_code: 8,
            racf_reason_code: 4,
            reason,
        }
    }
}

fn snapshot_content_digest(snapshot: &SecurityDatabaseSnapshot) -> Result<String, HostProblem> {
    let mut content = snapshot.clone();
    content.generation = 1;
    content.migrations.clear();
    let bytes = serde_json::to_vec(&content).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn semantic_bytes(value: &impl serde::Serialize) -> Result<Vec<u8>, HostProblem> {
    serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)
}

fn recovery_id(transaction_id: &str) -> String {
    format!("RECOVERY{:X}", Sha256::digest(transaction_id.as_bytes()))
}

fn digest_part(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
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
    use crate::{MemorySecretResolver, RacfService};
    use argon2::Argon2;
    use argon2::password_hash::PasswordHasher;
    use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
    use mainframe_env_host_api::{AccessIntent, ResourceName, SecretRef, SecurityDecision};
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
                generic_active: true,
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

    #[test]
    fn legacy_records_migrate_atomically_retain_source_and_support_rollback() {
        let store = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..StoreLimits::default()
        }));
        let hash = Argon2::default()
            .hash_password_with_salt(b"USER-PASSWORD", b"mainframe-env:USER1")
            .unwrap()
            .to_string();
        for record in [
            ProviderStateRecord {
                namespace: LEGACY_GROUP_NAMESPACE.into(),
                key: "GROUP1".into(),
                version: 1,
                payload: Vec::new(),
            },
            ProviderStateRecord {
                namespace: LEGACY_USER_NAMESPACE.into(),
                key: "USER1".into(),
                version: 1,
                payload: legacy_user_payload(&hash, &["GROUP1"]),
            },
            ProviderStateRecord {
                namespace: LEGACY_PROFILE_NAMESPACE.into(),
                key: "DATASET:USER1.**".into(),
                version: 1,
                payload: legacy_profile_payload("DATASET", "USER1.**", "USER1", 0, &[("USER1", 2)]),
            },
            ProviderStateRecord {
                namespace: LEGACY_AUDIT_NAMESPACE.into(),
                key: "00000000000000000001".into(),
                version: 1,
                payload: legacy_audit_payload(
                    "SIGNON",
                    "sha256:legacy-resource",
                    "DENY",
                    &[("PASSWORD", "NEVER-RETAIN")],
                ),
            },
        ] {
            store.put_provider_state(record, None).unwrap();
        }
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:user1", b"USER-PASSWORD".to_vec());
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let service = RacfService::open(provider_store, resolver, Default::default()).unwrap();
        let summary = service.database().summary().unwrap();
        assert_eq!(
            (summary.principals, summary.groups, summary.profiles),
            (1, 1, 1)
        );
        assert_eq!((summary.audits, summary.migrations), (1, 1));
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        assert_eq!(
            service
                .authenticate(
                    &user,
                    &SecretRef::new("secret:user1", Default::default()).unwrap(),
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("USER1.DATA", 246).unwrap(),
                    AccessIntent::Read,
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        let projected = service.smf_type80_records(0, 10).unwrap();
        assert_eq!(projected[0].record_type, 80);
        assert_eq!(projected[0].fields["PASSWORD"], AuditFieldValue::Redacted);
        assert!(service.database().rollback_legacy_migration().unwrap());
        assert!(!service.database().rollback_legacy_migration().unwrap());
        let rolled_back = service.database().summary().unwrap();
        assert_eq!(
            (
                rolled_back.principals,
                rolled_back.groups,
                rolled_back.profiles,
                rolled_back.audits,
                rolled_back.migrations,
            ),
            (0, 0, 0, 0, 1)
        );
        assert!(
            store
                .get_provider_state(LEGACY_USER_NAMESPACE, "USER1")
                .unwrap()
                .is_some()
        );
        let provider_store: Arc<dyn ProviderStateStore> = store;
        let reopened = RacfService::open(
            provider_store,
            Arc::new(MemorySecretResolver::default()),
            Default::default(),
        )
        .unwrap();
        assert_eq!(reopened.database().summary().unwrap().principals, 0);
    }

    #[test]
    fn malformed_legacy_record_fails_without_partial_migration() {
        let store = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..StoreLimits::default()
        }));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: LEGACY_GROUP_NAMESPACE.into(),
                    key: "GROUP1".into(),
                    version: 1,
                    payload: Vec::new(),
                },
                None,
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: LEGACY_USER_NAMESPACE.into(),
                    key: "USER1".into(),
                    version: 1,
                    payload: b"corrupt".to_vec(),
                },
                None,
            )
            .unwrap();
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        assert!(matches!(
            RacfService::open(
                provider_store,
                Arc::new(MemorySecretResolver::default()),
                Default::default(),
            ),
            Err(HostProblem::InfrastructureFailure)
        ));
        let provider_store: Arc<dyn ProviderStateStore> = store;
        let database = SecurityDatabase::open(provider_store, Default::default()).unwrap();
        let summary = database.summary().unwrap();
        assert_eq!(
            (summary.principals, summary.groups, summary.migrations),
            (0, 0, 0)
        );
    }

    #[test]
    fn every_legacy_namespace_rejects_truncation_and_retries_after_overflow_is_removed() {
        for namespace in [
            LEGACY_USER_NAMESPACE,
            LEGACY_GROUP_NAMESPACE,
            LEGACY_PROFILE_NAMESPACE,
            LEGACY_AUDIT_NAMESPACE,
        ] {
            let store = Arc::new(MemoryStore::new(StoreLimits {
                max_blob_bytes: 32 * 1024 * 1024,
                ..StoreLimits::default()
            }));
            let mut limits = SecurityDatabaseLimits::default();
            match namespace {
                LEGACY_USER_NAMESPACE => limits.max_principals = 2,
                LEGACY_GROUP_NAMESPACE => limits.max_groups = 2,
                LEGACY_PROFILE_NAMESPACE => limits.max_profiles = 2,
                LEGACY_AUDIT_NAMESPACE => limits.max_audits = 2,
                _ => unreachable!(),
            }
            for index in 1..=2 {
                store
                    .put_provider_state(legacy_record(namespace, index), None)
                    .unwrap();
            }
            assert_eq!(LegacySnapshot::read(&*store, limits).unwrap().rows.len(), 2);

            let overflow = legacy_record(namespace, 3);
            store.put_provider_state(overflow.clone(), None).unwrap();
            assert_eq!(
                LegacySnapshot::read(&*store, limits).map(|_| ()),
                Err(HostProblem::ResourceExhausted),
                "legacy namespace {namespace} was silently truncated"
            );
            let provider_store: Arc<dyn ProviderStateStore> = store.clone();
            let database = SecurityDatabase::open(provider_store, limits).unwrap();
            let summary = database.summary().unwrap();
            assert_eq!(
                (
                    summary.principals,
                    summary.groups,
                    summary.profiles,
                    summary.audits,
                    summary.migrations
                ),
                (0, 0, 0, 0, 0),
                "legacy namespace {namespace} published partial v2 state"
            );

            store
                .delete_provider_state(namespace, &overflow.key, overflow.version)
                .unwrap();
            assert_eq!(LegacySnapshot::read(&*store, limits).unwrap().rows.len(), 2);
        }
    }

    #[test]
    fn restart_reconciles_intent_and_unknown_outcome_once() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..StoreLimits::default()
        }));
        let database = SecurityDatabase::open(store.clone(), Default::default()).unwrap();
        database
            .mutate(|snapshot| {
                let base_generation = snapshot.generation;
                for (id, state) in [
                    ("PENDING-INTENT", TransactionState::Intent),
                    ("PENDING-UNKNOWN", TransactionState::UnknownOutcome),
                ] {
                    snapshot.transactions.insert(
                        id.into(),
                        crate::SecurityTransaction {
                            id: id.into(),
                            idempotency_key: id.into(),
                            actor: "SYSTEM".into(),
                            operation: "RECOVERY".into(),
                            request_digest: format!("sha256:{}", "a".repeat(64)),
                            state,
                            base_generation,
                            final_generation: Some(base_generation + 1),
                            status: legacy_status(DecisionReason::RecoveryRequired),
                            terminal_result: None,
                        },
                    );
                }
                Ok(())
            })
            .unwrap();
        drop(database);
        let service = RacfService::open(
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            Default::default(),
        )
        .unwrap();
        let snapshot = service.database.read().unwrap();
        assert_eq!(
            snapshot.transactions["PENDING-INTENT"].state,
            TransactionState::RolledBack
        );
        assert_eq!(
            snapshot.transactions["PENDING-UNKNOWN"].state,
            TransactionState::Committed
        );
        assert_eq!(snapshot.recovery.len(), 2);
        assert!(
            snapshot
                .recovery
                .values()
                .all(|record| record.state == RecoveryState::Reconciled && record.attempt == 1)
        );
        let generation = snapshot.generation;
        drop(service);
        let reopened = RacfService::open(
            store,
            Arc::new(MemorySecretResolver::default()),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            reopened.database().summary().unwrap().generation,
            generation
        );
    }

    fn legacy_record(namespace: &str, index: usize) -> ProviderStateRecord {
        let suffix = format!("{index:07}");
        let (key, payload) = match namespace {
            LEGACY_USER_NAMESPACE => (
                format!("U{suffix}"),
                legacy_user_payload("$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$dmVyaWZpZXI", &[]),
            ),
            LEGACY_GROUP_NAMESPACE => (format!("G{suffix}"), Vec::new()),
            LEGACY_PROFILE_NAMESPACE => {
                let name = format!("P{suffix}");
                (
                    format!("DATASET:{name}"),
                    legacy_profile_payload("DATASET", &name, "U0000001", 0, &[]),
                )
            }
            LEGACY_AUDIT_NAMESPACE => (
                format!("{index:020}"),
                legacy_audit_payload("SIGNON", "sha256:legacy", "DENY", &[]),
            ),
            _ => unreachable!(),
        };
        ProviderStateRecord {
            namespace: namespace.into(),
            key,
            version: 1,
            payload,
        }
    }

    fn legacy_user_payload(hash: &str, groups: &[&str]) -> Vec<u8> {
        let mut payload = b"MERU1".to_vec();
        legacy_field(&mut payload, hash.as_bytes());
        payload.extend_from_slice(&[0, 0, 0]);
        legacy_u32(&mut payload, groups.len());
        for group in groups {
            legacy_field(&mut payload, group.as_bytes());
        }
        payload
    }

    fn legacy_profile_payload(
        class: &str,
        name: &str,
        owner: &str,
        uacc: u8,
        permissions: &[(&str, u8)],
    ) -> Vec<u8> {
        let mut payload = b"MERP1".to_vec();
        for value in [class, name, owner] {
            legacy_field(&mut payload, value.as_bytes());
        }
        payload.push(uacc);
        legacy_u32(&mut payload, permissions.len());
        for (principal, access) in permissions {
            legacy_field(&mut payload, principal.as_bytes());
            payload.push(*access);
        }
        payload
    }

    fn legacy_audit_payload(
        action: &str,
        resource: &str,
        decision: &str,
        fields: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut payload = b"MERA1".to_vec();
        for value in [action, resource, decision] {
            legacy_field(&mut payload, value.as_bytes());
        }
        legacy_u32(&mut payload, fields.len());
        for (name, value) in fields {
            legacy_field(&mut payload, name.as_bytes());
            legacy_field(&mut payload, value.as_bytes());
        }
        payload
    }

    fn legacy_field(payload: &mut Vec<u8>, value: &[u8]) {
        legacy_u32(payload, value.len());
        payload.extend_from_slice(value);
    }

    fn legacy_u32(payload: &mut Vec<u8>, value: usize) {
        payload.extend_from_slice(&u32::try_from(value).unwrap().to_be_bytes());
    }
}
