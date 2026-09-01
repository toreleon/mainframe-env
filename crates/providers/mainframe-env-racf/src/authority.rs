use crate::database::SecurityDatabase;
use crate::model::{
    AccessControlEntry, AccessLevel, AuditFieldValue, AuditPolicy, ClassDescriptor,
    CredentialVerifier, DecisionOutcome, DecisionReason, GroupAuthority, GroupConnection,
    GroupProfile, PrincipalKind, PrincipalProfile, PrincipalState, ProfileTemplate,
    ResourceProfile, SafStatus, SecurityAuditRecord, SecurityDatabaseLimits, SegmentTemplate,
    connection_key, profile_key,
};
use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    AccessIntent, AuditEvent, CapabilityDescriptor, EffectRequest, EffectResult, HostProblem,
    HostProvider, HostRequest, HostResult, ResourceName, SecretRef, SecurityDecision,
    SecurityRequest,
};
use mainframe_env_store_api::ProviderStateStore;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem>;
}

pub struct ResolvedSecret(Zeroizing<Vec<u8>>);

impl ResolvedSecret {
    pub fn new(value: Vec<u8>) -> Result<Self, HostProblem> {
        Self::from_zeroizing(Zeroizing::new(value))
    }

    pub fn from_zeroizing(value: Zeroizing<Vec<u8>>) -> Result<Self, HostProblem> {
        if value.is_empty() || value.len() > 4_096 {
            Err(HostProblem::Malformed)
        } else {
            Ok(Self(value))
        }
    }
}

impl Deref for ResolvedSecret {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default)]
pub struct MemorySecretResolver {
    values: Mutex<BTreeMap<String, Zeroizing<Vec<u8>>>>,
}

impl MemorySecretResolver {
    pub fn insert(&self, reference: &str, value: Vec<u8>) {
        self.values
            .lock()
            .expect("secret resolver mutex")
            .insert(reference.into(), Zeroizing::new(value));
    }

    pub fn remove(&self, reference: &str) {
        if let Ok(mut values) = self.values.lock() {
            values.remove(reference);
        }
    }
}

impl SecretResolver for MemorySecretResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem> {
        let values = self
            .values
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = values
            .get(reference.as_str())
            .ok_or(HostProblem::NotFound)?;
        ResolvedSecret::new(value.as_slice().to_vec())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RacfLimits {
    pub max_users: usize,
    pub max_groups: usize,
    pub max_profiles: usize,
    pub max_permissions: usize,
    pub max_audits: usize,
    pub max_name_bytes: usize,
}

impl Default for RacfLimits {
    fn default() -> Self {
        Self {
            max_users: 4096,
            max_groups: 4096,
            max_profiles: 65_536,
            max_permissions: 1024,
            max_audits: 65_536,
            max_name_bytes: 246,
        }
    }
}

impl From<RacfLimits> for SecurityDatabaseLimits {
    fn from(value: RacfLimits) -> Self {
        Self {
            max_name_bytes: value.max_name_bytes,
            max_principals: value.max_users,
            max_groups: value.max_groups,
            max_profiles: value.max_profiles,
            max_access_entries: value.max_permissions,
            max_audits: value.max_audits,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RacfUserDefinition {
    pub user: String,
    pub credential: SecretRef,
    pub groups: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RacfProfileDefinition {
    pub class: String,
    pub pattern: String,
    pub owner: String,
    pub uacc: Option<AccessIntent>,
    pub permissions: BTreeMap<String, AccessIntent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RacfManifest {
    pub groups: BTreeSet<String>,
    pub users: Vec<RacfUserDefinition>,
    pub profiles: Vec<RacfProfileDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RacfInstallReceipt {
    pub groups: usize,
    pub users: usize,
    pub profiles: usize,
    pub permissions: usize,
    pub replayed: bool,
}

pub struct RacfService {
    database: Arc<SecurityDatabase>,
    secrets: Arc<dyn SecretResolver>,
    limits: RacfLimits,
}

impl RacfService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        secrets: Arc<dyn SecretResolver>,
        limits: RacfLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let database = SecurityDatabase::open(store, limits.into())?;
        Ok(Arc::new(Self {
            database,
            secrets,
            limits,
        }))
    }

    #[must_use]
    pub fn database(&self) -> &Arc<SecurityDatabase> {
        &self.database
    }

    pub fn install_manifest(
        &self,
        manifest: RacfManifest,
    ) -> Result<RacfInstallReceipt, HostProblem> {
        let groups = manifest
            .groups
            .iter()
            .map(|group| normalize(group, 8))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if groups.len() != manifest.groups.len() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut users = BTreeMap::new();
        for definition in &manifest.users {
            let user = normalize(&definition.user, 8)?;
            let user_groups = definition
                .groups
                .iter()
                .map(|group| normalize(group, 8))
                .collect::<Result<BTreeSet<_>, _>>()?;
            if !user_groups.is_subset(&groups) {
                return Err(HostProblem::NotFound);
            }
            let principal = self.password_principal(&user, &definition.credential)?;
            if users.insert(user, (principal, user_groups)).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let declared_principals = groups
            .iter()
            .chain(users.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut profiles = BTreeMap::new();
        for definition in &manifest.profiles {
            let class = normalize(&definition.class, 32)?;
            let pattern = normalize_pattern(&definition.pattern, self.limits.max_name_bytes)?;
            let owner = normalize(&definition.owner, 8)?;
            if !declared_principals.contains(&owner)
                || definition.permissions.len() > self.limits.max_permissions
            {
                return Err(HostProblem::NotFound);
            }
            let mut access_list = Vec::new();
            for (principal, access) in &definition.permissions {
                let principal = normalize(principal, 8)?;
                if !declared_principals.contains(&principal) {
                    return Err(HostProblem::NotFound);
                }
                access_list.push(AccessControlEntry {
                    principal,
                    access: (*access).into(),
                    when: None,
                    audit: AuditPolicy::None,
                });
            }
            access_list.sort_by(|left, right| left.principal.cmp(&right.principal));
            let profile = ResourceProfile {
                class: class.clone(),
                name: pattern.clone(),
                generic: contains_generic(&pattern),
                owner,
                uacc: definition.uacc.map_or(AccessLevel::None, Into::into),
                audit: AuditPolicy::None,
                security_level: 0,
                security_label: None,
                categories: BTreeSet::new(),
                access_list,
                segments: BTreeMap::new(),
                version: 1,
            };
            if profiles
                .insert(profile_key(&class, &pattern), profile)
                .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let permissions = profiles
            .values()
            .map(|profile| profile.access_list.len())
            .sum();
        let ((groups_count, users_count, profiles_count, replayed), _) =
            self.database.mutate_if_changed(|snapshot| {
                let mut changed = false;
                for group in &groups {
                    let profile = group_profile(group);
                    changed |= insert_exact(&mut snapshot.groups, group, profile)?;
                }
                for (user, (principal, user_groups)) in &users {
                    changed |= insert_exact(&mut snapshot.principals, user, principal.clone())?;
                    for group in user_groups {
                        let connection = GroupConnection {
                            user: user.clone(),
                            group: group.clone(),
                            authority: GroupAuthority::Use,
                            special: false,
                            operations: false,
                            auditor: false,
                            revoked: false,
                            version: 1,
                        };
                        let key = connection_key(user, group);
                        changed |= insert_exact(&mut snapshot.connections, &key, connection)?;
                    }
                }
                for profile in profiles.values() {
                    install_resource_schema(snapshot, &profile.class, self.limits.max_name_bytes)?;
                }
                for (key, profile) in &profiles {
                    changed |= insert_exact(&mut snapshot.profiles, key, profile.clone())?;
                }
                Ok((
                    (groups.len(), users.len(), profiles.len(), !changed),
                    changed,
                ))
            })?;
        Ok(RacfInstallReceipt {
            groups: groups_count,
            users: users_count,
            profiles: profiles_count,
            permissions,
            replayed,
        })
    }

    pub fn add_group(&self, name: &str) -> Result<(), HostProblem> {
        let name = normalize(name, 8)?;
        self.database.mutate(|snapshot| {
            if snapshot.groups.len() >= self.limits.max_groups {
                return Err(HostProblem::ResourceExhausted);
            }
            if snapshot.groups.contains_key(&name) {
                return Err(HostProblem::IdempotencyConflict);
            }
            snapshot.groups.insert(name.clone(), group_profile(&name));
            Ok(())
        })?;
        Ok(())
    }

    pub fn add_user(&self, user: &str, credential: &SecretRef) -> Result<(), HostProblem> {
        let user = normalize(user, 8)?;
        let principal = self.password_principal(&user, credential)?;
        self.database.mutate(|snapshot| {
            if snapshot.principals.len() >= self.limits.max_users {
                return Err(HostProblem::ResourceExhausted);
            }
            if snapshot.principals.contains_key(&user) {
                return Err(HostProblem::IdempotencyConflict);
            }
            snapshot.principals.insert(user, principal);
            Ok(())
        })?;
        Ok(())
    }

    pub fn connect(&self, user: &str, group: &str) -> Result<(), HostProblem> {
        let user = normalize(user, 8)?;
        let group = normalize(group, 8)?;
        self.database.mutate(|snapshot| {
            if !snapshot.principals.contains_key(&user) || !snapshot.groups.contains_key(&group) {
                return Err(HostProblem::NotFound);
            }
            let key = connection_key(&user, &group);
            if snapshot.connections.contains_key(&key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            snapshot.connections.insert(
                key,
                GroupConnection {
                    user,
                    group,
                    authority: GroupAuthority::Use,
                    special: false,
                    operations: false,
                    auditor: false,
                    revoked: false,
                    version: 1,
                },
            );
            Ok(())
        })?;
        Ok(())
    }

    pub fn define_profile(
        &self,
        class: &str,
        pattern: &str,
        owner: &str,
        uacc: Option<AccessIntent>,
    ) -> Result<(), HostProblem> {
        let class = normalize(class, 32)?;
        let pattern = normalize_pattern(pattern, self.limits.max_name_bytes)?;
        let owner = normalize(owner, 8)?;
        self.database.mutate(|snapshot| {
            if snapshot.profiles.len() >= self.limits.max_profiles {
                return Err(HostProblem::ResourceExhausted);
            }
            if !snapshot.principals.contains_key(&owner) && !snapshot.groups.contains_key(&owner) {
                return Err(HostProblem::NotFound);
            }
            install_resource_schema(snapshot, &class, self.limits.max_name_bytes)?;
            let key = profile_key(&class, &pattern);
            if snapshot.profiles.contains_key(&key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            snapshot.profiles.insert(
                key,
                ResourceProfile {
                    class,
                    name: pattern.clone(),
                    generic: contains_generic(&pattern),
                    owner,
                    uacc: uacc.map_or(AccessLevel::None, Into::into),
                    audit: AuditPolicy::None,
                    security_level: 0,
                    security_label: None,
                    categories: BTreeSet::new(),
                    access_list: Vec::new(),
                    segments: BTreeMap::new(),
                    version: 1,
                },
            );
            Ok(())
        })?;
        Ok(())
    }

    pub fn permit(
        &self,
        class: &str,
        pattern: &str,
        principal: &str,
        access: AccessIntent,
    ) -> Result<(), HostProblem> {
        let class = normalize(class, 32)?;
        let pattern = normalize_pattern(pattern, self.limits.max_name_bytes)?;
        let principal = normalize(principal, 8)?;
        self.database.mutate(|snapshot| {
            if !snapshot.principals.contains_key(&principal)
                && !snapshot.groups.contains_key(&principal)
            {
                return Err(HostProblem::NotFound);
            }
            let profile = snapshot
                .profiles
                .get_mut(&profile_key(&class, &pattern))
                .ok_or(HostProblem::NotFound)?;
            if let Some(entry) = profile
                .access_list
                .iter_mut()
                .find(|entry| entry.principal == principal && entry.when.is_none())
            {
                entry.access = access.into();
            } else {
                if profile.access_list.len() >= self.limits.max_permissions {
                    return Err(HostProblem::ResourceExhausted);
                }
                profile.access_list.push(AccessControlEntry {
                    principal,
                    access: access.into(),
                    when: None,
                    audit: AuditPolicy::None,
                });
                profile
                    .access_list
                    .sort_by(|left, right| left.principal.cmp(&right.principal));
            }
            profile.version = profile
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn set_user_state(
        &self,
        user: &str,
        expired: bool,
        revoked: bool,
        locked: bool,
    ) -> Result<(), HostProblem> {
        let user = normalize(user, 8)?;
        self.database.mutate(|snapshot| {
            let principal = snapshot
                .principals
                .get_mut(&user)
                .ok_or(HostProblem::NotFound)?;
            principal.state = if revoked {
                PrincipalState::Revoked
            } else if locked {
                PrincipalState::Locked
            } else if expired {
                PrincipalState::PasswordExpired
            } else {
                PrincipalState::Active
            };
            principal.version = principal
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn authenticate(
        &self,
        user: &PrincipalId,
        reference: &SecretRef,
    ) -> Result<SecurityDecision, HostProblem> {
        let snapshot = self.database.read()?;
        let Some(principal) = snapshot.principals.get(user.as_str()) else {
            return Ok(SecurityDecision::InvalidCredentials);
        };
        match principal.state {
            PrincipalState::Revoked | PrincipalState::Suspended => {
                return Ok(SecurityDecision::Revoked);
            }
            PrincipalState::Locked => return Ok(SecurityDecision::Locked),
            PrincipalState::PasswordExpired => return Ok(SecurityDecision::Expired),
            PrincipalState::Active => {}
        }
        let Some(credential) = principal.credential.as_ref() else {
            return Ok(SecurityDecision::InvalidCredentials);
        };
        if credential.algorithm != "argon2id" {
            return Err(HostProblem::ProviderFailure);
        }
        let secret = self.secrets.resolve(reference)?;
        let parsed = PasswordHash::new(&credential.encoded_verifier)
            .map_err(|_| HostProblem::ProviderFailure)?;
        let valid = Argon2::default().verify_password(&secret, &parsed).is_ok();
        drop(secret);
        Ok(if valid {
            SecurityDecision::Allow
        } else {
            SecurityDecision::InvalidCredentials
        })
    }

    pub fn authorize(
        &self,
        principal: &PrincipalId,
        class: &str,
        resource: &ResourceName,
        intent: AccessIntent,
    ) -> Result<SecurityDecision, HostProblem> {
        let class = normalize(class, 32)?;
        let snapshot = self.database.read()?;
        let Some(user) = snapshot.principals.get(principal.as_str()) else {
            return Ok(SecurityDecision::Deny);
        };
        if matches!(
            user.state,
            PrincipalState::Revoked | PrincipalState::Suspended | PrincipalState::Locked
        ) {
            return Ok(SecurityDecision::Deny);
        }
        let Some(class_record) = snapshot.classes.get(&class) else {
            return Ok(SecurityDecision::Deny);
        };
        if !class_record.active {
            return Ok(SecurityDecision::Deny);
        }
        let selected = snapshot
            .profiles
            .values()
            .filter(|profile| {
                profile.class == class
                    && if profile.generic {
                        generic_match(&profile.name, resource.as_str())
                    } else {
                        profile.name == resource.as_str()
                    }
            })
            .max_by_key(|profile| specificity(&profile.name));
        let Some(profile) = selected else {
            return Ok(SecurityDecision::Deny);
        };
        let requested: AccessLevel = intent.into();
        let direct = profile
            .access_list
            .iter()
            .filter(|entry| entry.principal == principal.as_str() && entry.when.is_none())
            .map(|entry| entry.access)
            .max();
        let groups = effective_groups(&snapshot.connections, principal.as_str());
        let group = profile
            .access_list
            .iter()
            .filter(|entry| groups.contains(&entry.principal) && entry.when.is_none())
            .map(|entry| entry.access)
            .max();
        let granted = direct.or(group).unwrap_or(profile.uacc);
        Ok(if granted.permits(requested) {
            SecurityDecision::Allow
        } else {
            SecurityDecision::Deny
        })
    }

    pub fn record_audit(&self, event: AuditEvent) -> Result<SecurityDecision, HostProblem> {
        self.audit(event)
    }

    #[must_use]
    pub fn audits(&self) -> Vec<AuditEvent> {
        self.database
            .read()
            .map(|snapshot| snapshot.audits.iter().map(audit_event).collect())
            .unwrap_or_default()
    }

    pub fn list_profiles(
        &self,
        class: &str,
        start: Option<&str>,
        max_items: usize,
    ) -> Result<(Vec<String>, bool), HostProblem> {
        let class = normalize(class, 32)?;
        if max_items == 0 || max_items > self.limits.max_profiles {
            return Err(HostProblem::ResourceExhausted);
        }
        let snapshot = self.database.read()?;
        let mut profiles = Vec::new();
        let mut more = false;
        for profile in snapshot.profiles.values().filter(|profile| {
            profile.class == class && start.is_none_or(|start| profile.name.as_str() > start)
        }) {
            if profiles.len() == max_items {
                more = true;
                break;
            }
            profiles.push(profile.name.clone());
        }
        Ok((profiles, more))
    }

    fn password_principal(
        &self,
        user: &str,
        credential: &SecretRef,
    ) -> Result<PrincipalProfile, HostProblem> {
        let secret = self.secrets.resolve(credential)?;
        let verifier = Argon2::default()
            .hash_password_with_salt(&secret, format!("mainframe-env:{user}").as_bytes())
            .map(|hash| hash.to_string())
            .map_err(|_| HostProblem::ProviderFailure)?;
        drop(secret);
        Ok(PrincipalProfile {
            id: user.into(),
            kind: PrincipalKind::User,
            owner: user.into(),
            default_group: None,
            state: PrincipalState::Active,
            credential: Some(CredentialVerifier {
                algorithm: "argon2id".into(),
                encoded_verifier: verifier,
                changed_tick: 0,
                history_digests: Vec::new(),
            }),
            profile_template: None,
            segments: BTreeMap::new(),
            security_level: 0,
            security_label: None,
            categories: BTreeSet::new(),
            attributes: BTreeSet::new(),
            version: 1,
        })
    }

    fn audit(&self, event: AuditEvent) -> Result<SecurityDecision, HostProblem> {
        let event = redact_audit(event, self.limits)?;
        self.database.mutate(|snapshot| {
            if snapshot.audits.len() >= self.limits.max_audits {
                return Err(HostProblem::ResourceExhausted);
            }
            let id = format!("AUDIT{:020}", snapshot.generation);
            snapshot.audits.push(SecurityAuditRecord {
                id,
                correlation: "LEGACY".into(),
                actor: "SYSTEM".into(),
                action: event.action,
                class: None,
                resource_digest: Some(event.resource_hash),
                decision: match event.decision.as_str() {
                    "ALLOW" | "SUCCESS" => DecisionOutcome::Allow,
                    "DENY" | "FAILURE" => DecisionOutcome::Deny,
                    _ => DecisionOutcome::NoDecision,
                },
                status: status(DecisionReason::Granted),
                fields: event
                    .fields
                    .into_iter()
                    .map(|(name, value)| {
                        let value = if value == "<redacted>" {
                            AuditFieldValue::Redacted
                        } else {
                            AuditFieldValue::Text(value)
                        };
                        (name, value)
                    })
                    .collect(),
                tick: 0,
            });
            Ok(())
        })?;
        Ok(SecurityDecision::Allow)
    }

    fn invoke(&self, request: SecurityRequest) -> Result<SecurityDecision, HostProblem> {
        match request {
            SecurityRequest::Authenticate {
                user,
                credential_reference,
            } => self.authenticate(&user, &credential_reference),
            SecurityRequest::Authorize {
                principal,
                class,
                resource,
                intent,
            } => self.authorize(&principal, &class, &resource, intent),
            SecurityRequest::Audit(event) => self.audit(event),
        }
    }
}

struct Provider {
    service: Arc<RacfService>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        let sequence = request.sequence;
        let outcome = match request.request {
            HostRequest::Security(request) => {
                self.service.invoke(request).map(HostResult::Security)
            }
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}

pub fn racf_providers(
    service: Arc<RacfService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.security.authorize", "host.audit"]
        .into_iter()
        .map(|capability| {
            Arc::new(Provider {
                service: Arc::clone(&service),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits).expect("static capability"),
                    provider_id: "mainframe-env-racf".into(),
                    generation: "1".into(),
                    request_schema: "mainframe-env.host.security-request@1".into(),
                    result_schema: "mainframe-env.host.security-result@1".into(),
                    max_request_bytes: 65_536,
                    max_result_bytes: 65_536,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

fn group_profile(name: &str) -> GroupProfile {
    GroupProfile {
        name: name.into(),
        owner: name.into(),
        superior_group: None,
        universal: false,
        profile_template: None,
        segments: BTreeMap::new(),
        version: 1,
    }
}

fn insert_exact<T: Eq>(
    values: &mut BTreeMap<String, T>,
    key: &str,
    value: T,
) -> Result<bool, HostProblem> {
    match values.get(key) {
        Some(current) if current == &value => Ok(false),
        Some(_) => Err(HostProblem::IdempotencyConflict),
        None => {
            values.insert(key.into(), value);
            Ok(true)
        }
    }
}

fn install_resource_schema(
    snapshot: &mut crate::model::SecurityDatabaseSnapshot,
    class: &str,
    max_name_bytes: usize,
) -> Result<(), HostProblem> {
    let template = ProfileTemplate {
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
    };
    insert_exact(&mut snapshot.templates, "RESOURCE", template)?;
    if !snapshot.classes.contains_key(class) {
        snapshot.classes.insert(
            class.into(),
            ClassDescriptor {
                name: class.into(),
                supplied: false,
                active: true,
                generic_allowed: true,
                discrete_allowed: true,
                raclist: false,
                default_uacc: AccessLevel::None,
                max_profile_name_bytes: max_name_bytes,
                posit: None,
                member_class: None,
                grouping_class: None,
                profile_template: "RESOURCE".into(),
                version: 1,
            },
        );
    }
    Ok(())
}

fn effective_groups(
    connections: &BTreeMap<String, GroupConnection>,
    principal: &str,
) -> BTreeSet<String> {
    connections
        .values()
        .filter(|connection| connection.user == principal && !connection.revoked)
        .map(|connection| connection.group.clone())
        .collect()
}

fn status(reason: DecisionReason) -> SafStatus {
    SafStatus {
        saf_return_code: 0,
        racf_return_code: 0,
        racf_reason_code: 0,
        reason,
    }
}

fn audit_event(record: &SecurityAuditRecord) -> AuditEvent {
    AuditEvent {
        action: record.action.clone(),
        resource_hash: record.resource_digest.clone().unwrap_or_default(),
        decision: match record.decision {
            DecisionOutcome::Allow => "ALLOW",
            DecisionOutcome::Deny => "DENY",
            DecisionOutcome::NoDecision => "NO-DECISION",
        }
        .into(),
        fields: record
            .fields
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    AuditFieldValue::Text(value)
                    | AuditFieldValue::Digest(value)
                    | AuditFieldValue::Reference(value) => value.clone(),
                    AuditFieldValue::Redacted => "<redacted>".into(),
                };
                (name.clone(), value)
            })
            .collect(),
    }
}

impl From<AccessIntent> for AccessLevel {
    fn from(value: AccessIntent) -> Self {
        match value {
            AccessIntent::Execute => Self::Execute,
            AccessIntent::Read => Self::Read,
            AccessIntent::Update => Self::Update,
            AccessIntent::Control => Self::Control,
            AccessIntent::Alter => Self::Alter,
        }
    }
}

fn normalize(value: &str, max: usize) -> Result<String, HostProblem> {
    let value = value.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > max
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value)
    }
}

fn normalize_pattern(value: &str, max: usize) -> Result<String, HostProblem> {
    let value = value.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > max
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$' | b'.' | b'*' | b'%')
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value)
    }
}

fn contains_generic(pattern: &str) -> bool {
    pattern.bytes().any(|byte| matches!(byte, b'*' | b'%'))
}

fn generic_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.split('.').collect::<Vec<_>>();
    let value = value.split('.').collect::<Vec<_>>();
    match_parts(&pattern, &value)
}

fn match_parts(pattern: &[&str], value: &[&str]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    if pattern[0] == "**" {
        return (0..=value.len()).any(|skip| match_parts(&pattern[1..], &value[skip..]));
    }
    if value.is_empty() {
        return false;
    }
    qualifier(pattern[0], value[0]) && match_parts(&pattern[1..], &value[1..])
}

fn qualifier(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    pattern.len() == value.len()
        && pattern
            .bytes()
            .zip(value.bytes())
            .all(|(pattern, value)| pattern == b'%' || pattern == value)
}

fn specificity(pattern: &str) -> (usize, usize) {
    (
        pattern
            .bytes()
            .filter(|byte| !matches!(byte, b'*' | b'%'))
            .count(),
        usize::MAX - pattern.matches("**").count(),
    )
}

fn redact_audit(mut event: AuditEvent, limits: RacfLimits) -> Result<AuditEvent, HostProblem> {
    if event.action.is_empty()
        || event.action.len() > limits.max_name_bytes
        || event.resource_hash.is_empty()
        || event.resource_hash.len() > limits.max_name_bytes
        || event.decision.is_empty()
        || event.decision.len() > limits.max_name_bytes
        || event.fields.len() > limits.max_permissions
    {
        return Err(HostProblem::ResourceExhausted);
    }
    for (name, value) in &mut event.fields {
        if name.is_empty() || name.len() > limits.max_name_bytes {
            return Err(HostProblem::Malformed);
        }
        let upper = name.to_ascii_uppercase();
        if [
            "PASSWORD",
            "CREDENTIAL",
            "SECRET",
            "TOKEN",
            "PRIVATE_KEY",
            "CERTIFICATE",
            "CARD",
            "QUEUE_PAYLOAD",
            "PROTECTED",
        ]
        .iter()
        .any(|marker| upper.contains(marker))
        {
            *value = "<redacted>".into();
        } else if value.len() > limits.max_name_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use static_assertions::assert_not_impl_any;

    assert_not_impl_any!(ResolvedSecret: Clone, std::fmt::Debug, std::fmt::Display, std::ops::DerefMut, serde::Serialize);

    fn setup() -> (Arc<RacfService>, Arc<MemorySecretResolver>) {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
        (service, resolver)
    }

    #[test]
    fn resolved_secret_has_no_clone_debug_display_or_serialization() {
        let secret = ResolvedSecret::new(b"bounded-secret".to_vec()).unwrap();
        assert_eq!(&*secret, b"bounded-secret");
    }

    #[test]
    fn authentication_profile_access_and_default_deny_use_v2_database() {
        let (service, resolver) = setup();
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let reference = SecretRef::new("secret:user", Default::default()).unwrap();
        service.add_group("OPER").unwrap();
        service.add_user("IBMUSER", &reference).unwrap();
        service.connect("IBMUSER", "OPER").unwrap();
        service
            .define_profile("DATASET", "USER.**", "IBMUSER", None)
            .unwrap();
        service
            .permit("DATASET", "USER.**", "OPER", AccessIntent::Update)
            .unwrap();
        let user = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        assert_eq!(
            service.authenticate(&user, &reference).unwrap(),
            SecurityDecision::Allow
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("USER.DATA", 246).unwrap(),
                    AccessIntent::Update,
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("SYS1.PARMLIB", 246).unwrap(),
                    AccessIntent::Read,
                )
                .unwrap(),
            SecurityDecision::Deny
        );
    }

    #[test]
    fn audit_redacts_before_durable_storage() {
        let (service, _) = setup();
        service
            .record_audit(AuditEvent {
                action: "SIGNON".into(),
                resource_hash: "sha256:resource".into(),
                decision: "DENY".into(),
                fields: BTreeMap::from([
                    ("password".into(), "TOP-SECRET".into()),
                    ("token_reference".into(), "secret:token".into()),
                    ("terminal".into(), "L7001".into()),
                ]),
            })
            .unwrap();
        let audits = service.audits();
        assert_eq!(audits[0].fields["password"], "<redacted>");
        assert_eq!(audits[0].fields["token_reference"], "<redacted>");
        assert_eq!(audits[0].fields["terminal"], "L7001");
        let shown = format!("{audits:?}");
        assert!(!shown.contains("TOP-SECRET"));
        assert!(!shown.contains("secret:token"));
    }

    #[test]
    fn sqlite_restart_preserves_v2_authority() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-v2-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("racf.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let reference = SecretRef::new("secret:user", Default::default()).unwrap();
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
            service.add_group("OPER").unwrap();
            service.add_user("IBMUSER", &reference).unwrap();
            service.connect("IBMUSER", "OPER").unwrap();
            service
                .define_profile("DATASET", "USER.**", "IBMUSER", None)
                .unwrap();
            service
                .permit("DATASET", "USER.**", "OPER", AccessIntent::Update)
                .unwrap();
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver, Default::default()).unwrap();
            assert_eq!(
                service
                    .authorize(
                        &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                        "DATASET",
                        &ResourceName::new("USER.DATA", 246).unwrap(),
                        AccessIntent::Update,
                    )
                    .unwrap(),
                SecurityDecision::Allow
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn manifest_is_atomic_replayable_and_bounded() {
        let (service, resolver) = setup();
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let manifest = RacfManifest {
            groups: BTreeSet::from(["OPER".into()]),
            users: vec![RacfUserDefinition {
                user: "IBMUSER".into(),
                credential: SecretRef::new("secret:user", Default::default()).unwrap(),
                groups: BTreeSet::from(["OPER".into()]),
            }],
            profiles: vec![RacfProfileDefinition {
                class: "DATASET".into(),
                pattern: "USER.**".into(),
                owner: "IBMUSER".into(),
                uacc: None,
                permissions: BTreeMap::from([("OPER".into(), AccessIntent::Read)]),
            }],
        };
        assert!(!service.install_manifest(manifest.clone()).unwrap().replayed);
        assert!(service.install_manifest(manifest).unwrap().replayed);
        assert_eq!(
            service.database().summary().unwrap().profiles,
            1,
            "manifest replay cannot duplicate profiles"
        );
    }
}
