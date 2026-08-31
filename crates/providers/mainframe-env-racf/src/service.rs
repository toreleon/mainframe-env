use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    AccessIntent, AuditEvent, CapabilityDescriptor, EffectRequest, EffectResult, HostProblem,
    HostProvider, HostRequest, HostResult, ResourceName, SecretRef, SecurityDecision,
    SecurityRequest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem>;
}

pub struct ResolvedSecret(Vec<u8>);

impl ResolvedSecret {
    pub fn new(value: Vec<u8>) -> Result<Self, HostProblem> {
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

impl DerefMut for ResolvedSecret {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for ResolvedSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
#[derive(Default)]
pub struct MemorySecretResolver {
    values: Mutex<BTreeMap<String, Vec<u8>>>,
}
impl MemorySecretResolver {
    pub fn insert(&self, reference: &str, value: Vec<u8>) {
        self.values
            .lock()
            .expect("secret resolver mutex")
            .insert(reference.into(), value);
    }
    pub fn remove(&self, reference: &str) {
        if let Ok(mut values) = self.values.lock()
            && let Some(mut value) = values.remove(reference)
        {
            value.fill(0);
        }
    }
}
impl SecretResolver for MemorySecretResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem> {
        let value = self
            .values
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(reference.as_str())
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        ResolvedSecret::new(value)
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
impl Default for RacfLimits {
    fn default() -> Self {
        Self {
            max_users: 4096,
            max_groups: 4096,
            max_profiles: 65536,
            max_permissions: 1024,
            max_audits: 65536,
            max_name_bytes: 246,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct User {
    hash: String,
    expired: bool,
    revoked: bool,
    locked: bool,
    groups: BTreeSet<String>,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Profile {
    class: String,
    pattern: String,
    owner: String,
    uacc: Option<AccessIntent>,
    permissions: BTreeMap<String, AccessIntent>,
    version: u64,
}
struct State {
    users: BTreeMap<String, User>,
    groups: BTreeMap<String, u64>,
    profiles: BTreeMap<(String, String), Profile>,
    audits: Vec<AuditEvent>,
    next_audit: u64,
}
pub struct RacfService {
    store: Arc<dyn ProviderStateStore>,
    secrets: Arc<dyn SecretResolver>,
    limits: RacfLimits,
    state: Mutex<State>,
}
impl RacfService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        secrets: Arc<dyn SecretResolver>,
        limits: RacfLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let mut users = BTreeMap::new();
        for row in store
            .list_provider_state("racf-user", limits.max_users)
            .map_err(store_error)?
        {
            users.insert(row.key, decode_user(&row.payload, row.version)?);
        }
        let mut groups = BTreeMap::new();
        for row in store
            .list_provider_state("racf-group", limits.max_groups)
            .map_err(store_error)?
        {
            groups.insert(row.key, row.version);
        }
        let mut profiles = BTreeMap::new();
        for row in store
            .list_provider_state("racf-profile", limits.max_profiles)
            .map_err(store_error)?
        {
            let profile = decode_profile(&row.payload, row.version)?;
            profiles.insert((profile.class.clone(), profile.pattern.clone()), profile);
        }
        let mut audits = Vec::new();
        for row in store
            .list_provider_state("racf-audit", limits.max_audits)
            .map_err(store_error)?
        {
            audits.push(decode_audit(&row.payload, limits)?);
        }
        let next_audit = u64::try_from(audits.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        Ok(Arc::new(Self {
            store,
            secrets,
            limits,
            state: Mutex::new(State {
                users,
                groups,
                profiles,
                audits,
                next_audit,
            }),
        }))
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
            let mut secret = self.secrets.resolve(&definition.credential)?;
            let hash_result = Argon2::default()
                .hash_password_with_salt(&secret, format!("mainframe-env:{user}").as_bytes())
                .map(|hash| hash.to_string());
            secret.fill(0);
            let record = User {
                hash: hash_result.map_err(|_| HostProblem::ProviderFailure)?,
                expired: false,
                revoked: false,
                locked: false,
                groups: user_groups,
                version: 1,
            };
            if users.insert(user, record).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let principals = groups
            .iter()
            .chain(users.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut profiles = BTreeMap::new();
        for definition in &manifest.profiles {
            let class = normalize(&definition.class, 32)?;
            let pattern = normalize_pattern(&definition.pattern, self.limits.max_name_bytes)?;
            let owner = normalize(&definition.owner, 8)?;
            if !principals.contains(&owner)
                || definition.permissions.len() > self.limits.max_permissions
            {
                return Err(HostProblem::NotFound);
            }
            let mut permissions = BTreeMap::new();
            for (principal, access) in &definition.permissions {
                let principal = normalize(principal, 8)?;
                if !principals.contains(&principal)
                    || permissions.insert(principal, *access).is_some()
                {
                    return Err(HostProblem::NotFound);
                }
            }
            let profile = Profile {
                class: class.clone(),
                pattern: pattern.clone(),
                owner,
                uacc: definition.uacc,
                permissions,
                version: 1,
            };
            if profiles.insert((class, pattern), profile).is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if state
            .groups
            .len()
            .checked_add(
                groups
                    .iter()
                    .filter(|group| !state.groups.contains_key(*group))
                    .count(),
            )
            .is_none_or(|total| total > self.limits.max_groups)
            || state
                .users
                .len()
                .checked_add(
                    users
                        .keys()
                        .filter(|user| !state.users.contains_key(*user))
                        .count(),
                )
                .is_none_or(|total| total > self.limits.max_users)
            || state
                .profiles
                .len()
                .checked_add(
                    profiles
                        .keys()
                        .filter(|key| !state.profiles.contains_key(*key))
                        .count(),
                )
                .is_none_or(|total| total > self.limits.max_profiles)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for group in &groups {
            if state.groups.contains_key(group) {
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "racf-group".into(),
                    key: group.clone(),
                    version: 1,
                    payload: Vec::new(),
                },
                expected_version: None,
            });
        }
        for (name, user) in &users {
            if let Some(existing) = state.users.get(name) {
                if existing != user {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "racf-user".into(),
                    key: name.clone(),
                    version: 1,
                    payload: encode_user(user)?,
                },
                expected_version: None,
            });
        }
        for ((class, pattern), profile) in &profiles {
            if let Some(existing) = state.profiles.get(&(class.clone(), pattern.clone())) {
                if existing != profile {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "racf-profile".into(),
                    key: profile_key(class, pattern),
                    version: 1,
                    payload: encode_profile(profile)?,
                },
                expected_version: None,
            });
        }
        let replayed = writes.is_empty();
        if !replayed {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for group in &groups {
            state.groups.entry(group.clone()).or_insert(1);
        }
        state.users.extend(users);
        state.profiles.extend(profiles);
        Ok(RacfInstallReceipt {
            groups: groups.len(),
            users: manifest.users.len(),
            profiles: manifest.profiles.len(),
            permissions: manifest
                .profiles
                .iter()
                .map(|profile| profile.permissions.len())
                .sum(),
            replayed,
        })
    }
    pub fn add_group(&self, name: &str) -> Result<(), HostProblem> {
        let name = normalize(name, self.limits.max_name_bytes)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if state.groups.len() >= self.limits.max_groups {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.groups.contains_key(&name) {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-group".into(),
                    key: name.clone(),
                    version: 1,
                    payload: Vec::new(),
                },
                None,
            )
            .map_err(store_error)?;
        state.groups.insert(name, 1);
        Ok(())
    }
    pub fn add_user(&self, user: &str, credential: &SecretRef) -> Result<(), HostProblem> {
        let user = normalize(user, 8)?;
        let mut secret = self.secrets.resolve(credential)?;
        let hash_result = Argon2::default()
            .hash_password_with_salt(&secret, format!("mainframe-env:{user}").as_bytes())
            .map(|hash| hash.to_string());
        secret.fill(0);
        let hash = hash_result.map_err(|_| HostProblem::ProviderFailure)?;
        let record = User {
            hash,
            expired: false,
            revoked: false,
            locked: false,
            groups: BTreeSet::new(),
            version: 1,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if state.users.len() >= self.limits.max_users {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.users.contains_key(&user) {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-user".into(),
                    key: user.clone(),
                    version: 1,
                    payload: encode_user(&record)?,
                },
                None,
            )
            .map_err(store_error)?;
        state.users.insert(user, record);
        Ok(())
    }
    pub fn connect(&self, user: &str, group: &str) -> Result<(), HostProblem> {
        let user = normalize(user, 8)?;
        let group = normalize(group, 8)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if !state.groups.contains_key(&group) {
            return Err(HostProblem::NotFound);
        }
        let current = state
            .users
            .get(&user)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version += 1;
        next.groups.insert(group);
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-user".into(),
                    key: user.clone(),
                    version: next.version,
                    payload: encode_user(&next)?,
                },
                Some(current.version),
            )
            .map_err(store_error)?;
        state.users.insert(user, next);
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
        let profile = Profile {
            class: class.clone(),
            pattern: pattern.clone(),
            owner,
            uacc,
            permissions: BTreeMap::new(),
            version: 1,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if state.profiles.len() >= self.limits.max_profiles {
            return Err(HostProblem::ResourceExhausted);
        }
        let key = (class.clone(), pattern.clone());
        if state.profiles.contains_key(&key) {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-profile".into(),
                    key: profile_key(&class, &pattern),
                    version: 1,
                    payload: encode_profile(&profile)?,
                },
                None,
            )
            .map_err(store_error)?;
        state.profiles.insert(key, profile);
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let current = state
            .profiles
            .get(&(class.clone(), pattern.clone()))
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if current.permissions.len() >= self.limits.max_permissions
            && !current.permissions.contains_key(&principal)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut next = current.clone();
        next.version += 1;
        next.permissions.insert(principal, access);
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-profile".into(),
                    key: profile_key(&class, &pattern),
                    version: next.version,
                    payload: encode_profile(&next)?,
                },
                Some(current.version),
            )
            .map_err(store_error)?;
        state.profiles.insert((class, pattern), next);
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let current = state
            .users
            .get(&user)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version += 1;
        next.expired = expired;
        next.revoked = revoked;
        next.locked = locked;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-user".into(),
                    key: user.clone(),
                    version: next.version,
                    payload: encode_user(&next)?,
                },
                Some(current.version),
            )
            .map_err(store_error)?;
        state.users.insert(user, next);
        Ok(())
    }
    pub fn authenticate(
        &self,
        user: &PrincipalId,
        reference: &SecretRef,
    ) -> Result<SecurityDecision, HostProblem> {
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let Some(record) = state.users.get(user.as_str()) else {
            return Ok(SecurityDecision::InvalidCredentials);
        };
        if record.revoked {
            return Ok(SecurityDecision::Revoked);
        }
        if record.locked {
            return Ok(SecurityDecision::Locked);
        }
        if record.expired {
            return Ok(SecurityDecision::Expired);
        }
        let hash = record.hash.clone();
        drop(state);
        let mut secret = self.secrets.resolve(reference)?;
        let parsed = PasswordHash::new(&hash).map_err(|_| HostProblem::ProviderFailure)?;
        let valid = Argon2::default().verify_password(&secret, &parsed).is_ok();
        secret.fill(0);
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
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let user = state.users.get(principal.as_str());
        if user.is_none_or(|user| user.revoked || user.locked) {
            return Ok(SecurityDecision::Deny);
        }
        let selected = state
            .profiles
            .values()
            .filter(|profile| {
                profile.class == class && generic_match(&profile.pattern, resource.as_str())
            })
            .max_by_key(|profile| specificity(&profile.pattern));
        let Some(profile) = selected else {
            return Ok(SecurityDecision::Deny);
        };
        let mut granted = profile.permissions.get(principal.as_str()).copied();
        if granted.is_none() {
            for group in &user.expect("checked").groups {
                if let Some(access) = profile.permissions.get(group)
                    && granted.is_none_or(|current| rank(*access) > rank(current))
                {
                    granted = Some(*access);
                }
            }
        }
        let granted = granted.or(profile.uacc);
        Ok(
            if granted.is_some_and(|access| rank(access) >= rank(intent)) {
                SecurityDecision::Allow
            } else {
                SecurityDecision::Deny
            },
        )
    }
    fn audit(&self, event: AuditEvent) -> Result<SecurityDecision, HostProblem> {
        let event = redact_audit(event, self.limits)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if state.audits.len() >= self.limits.max_audits {
            return Err(HostProblem::ResourceExhausted);
        }
        let key = format!("{:020}", state.next_audit);
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-audit".into(),
                    key,
                    version: 1,
                    payload: encode_audit(&event)?,
                },
                None,
            )
            .map_err(store_error)?;
        state.next_audit = state
            .next_audit
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        state.audits.push(event);
        Ok(SecurityDecision::Allow)
    }
    pub fn record_audit(&self, event: AuditEvent) -> Result<SecurityDecision, HostProblem> {
        self.audit(event)
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
    #[must_use]
    pub fn audits(&self) -> Vec<AuditEvent> {
        self.state
            .lock()
            .map(|state| state.audits.clone())
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
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut profiles = Vec::new();
        let mut more = false;
        for profile in state.profiles.values().filter(|profile| {
            profile.class == class && start.is_none_or(|start| profile.pattern.as_str() > start)
        }) {
            if profiles.len() == max_items {
                more = true;
                break;
            }
            profiles.push(profile.pattern.clone());
        }
        Ok((profiles, more))
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
                    max_request_bytes: 65536,
                    max_result_bytes: 65536,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
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
fn rank(value: AccessIntent) -> u8 {
    match value {
        AccessIntent::Execute => 1,
        AccessIntent::Read => 2,
        AccessIntent::Update => 3,
        AccessIntent::Control => 4,
        AccessIntent::Alter => 5,
    }
}
fn generic_match(pattern: &str, value: &str) -> bool {
    let p: Vec<_> = pattern.split('.').collect();
    let v: Vec<_> = value.split('.').collect();
    match_parts(&p, &v)
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
            .all(|(p, v)| p == b'%' || p == v)
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
fn profile_key(class: &str, pattern: &str) -> String {
    format!("{class}:{pattern}")
}
fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        _ => HostProblem::InfrastructureFailure,
    }
}
fn encode_user(user: &User) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MERU1".to_vec();
    field(&mut out, user.hash.as_bytes())?;
    out.push(u8::from(user.expired));
    out.push(u8::from(user.revoked));
    out.push(u8::from(user.locked));
    u32v(
        &mut out,
        u32::try_from(user.groups.len()).map_err(|_| HostProblem::ResourceExhausted)?,
    );
    for group in &user.groups {
        field(&mut out, group.as_bytes())?;
    }
    Ok(out)
}
fn decode_user(bytes: &[u8], version: u64) -> Result<User, HostProblem> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"MERU1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let hash = String::from_utf8(r.field(4096)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let expired = r.flag()?;
    let revoked = r.flag()?;
    let locked = r.flag()?;
    let count = usize::try_from(r.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut groups = BTreeSet::new();
    for _ in 0..count {
        groups.insert(
            String::from_utf8(r.field(8)?).map_err(|_| HostProblem::InfrastructureFailure)?,
        );
    }
    if r.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(User {
        hash,
        expired,
        revoked,
        locked,
        groups,
        version,
    })
}
fn encode_profile(p: &Profile) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MERP1".to_vec();
    field(&mut out, p.class.as_bytes())?;
    field(&mut out, p.pattern.as_bytes())?;
    field(&mut out, p.owner.as_bytes())?;
    out.push(p.uacc.map(rank).unwrap_or(0));
    u32v(
        &mut out,
        u32::try_from(p.permissions.len()).map_err(|_| HostProblem::ResourceExhausted)?,
    );
    for (name, access) in &p.permissions {
        field(&mut out, name.as_bytes())?;
        out.push(rank(*access));
    }
    Ok(out)
}
fn decode_profile(bytes: &[u8], version: u64) -> Result<Profile, HostProblem> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"MERP1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let class = String::from_utf8(r.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let pattern =
        String::from_utf8(r.field(246)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let owner = String::from_utf8(r.field(8)?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let uacc = access(r.byte()?)?;
    let count = usize::try_from(r.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut permissions = BTreeMap::new();
    for _ in 0..count {
        let name =
            String::from_utf8(r.field(8)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = access(r.byte()?)?.ok_or(HostProblem::InfrastructureFailure)?;
        permissions.insert(name, value);
    }
    if r.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Profile {
        class,
        pattern,
        owner,
        uacc,
        permissions,
        version,
    })
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
fn encode_audit(event: &AuditEvent) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"MERA1".to_vec();
    field(&mut out, event.action.as_bytes())?;
    field(&mut out, event.resource_hash.as_bytes())?;
    field(&mut out, event.decision.as_bytes())?;
    u32v(
        &mut out,
        u32::try_from(event.fields.len()).map_err(|_| HostProblem::ResourceExhausted)?,
    );
    for (name, value) in &event.fields {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value.as_bytes())?;
    }
    Ok(out)
}
fn decode_audit(bytes: &[u8], limits: RacfLimits) -> Result<AuditEvent, HostProblem> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(5)? != b"MERA1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let action = String::from_utf8(reader.field(limits.max_name_bytes)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let resource_hash = String::from_utf8(reader.field(limits.max_name_bytes)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let decision = String::from_utf8(reader.field(limits.max_name_bytes)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    if count > limits.max_permissions {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let name = String::from_utf8(reader.field(limits.max_name_bytes)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = String::from_utf8(reader.field(limits.max_name_bytes)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if fields.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if reader.at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    redact_audit(
        AuditEvent {
            action,
            resource_hash,
            decision,
            fields,
        },
        limits,
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}
fn access(value: u8) -> Result<Option<AccessIntent>, HostProblem> {
    Ok(match value {
        0 => None,
        1 => Some(AccessIntent::Execute),
        2 => Some(AccessIntent::Read),
        3 => Some(AccessIntent::Update),
        4 => Some(AccessIntent::Control),
        5 => Some(AccessIntent::Alter),
        _ => return Err(HostProblem::InfrastructureFailure),
    })
}
fn field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    u32v(
        out,
        u32::try_from(value.len()).map_err(|_| HostProblem::ResourceExhausted)?,
    );
    out.extend_from_slice(value);
    Ok(())
}
fn u32v(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes())
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(HostProblem::InfrastructureFailure)?;
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
    fn field(&mut self, max: usize) -> Result<Vec<u8>, HostProblem> {
        let n = usize::try_from(self.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
        if n > max {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(n)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::SecretRef;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    fn setup() -> (Arc<RacfService>, Arc<MemorySecretResolver>) {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
        (service, resolver)
    }
    fn carddemo_manifest() -> RacfManifest {
        let profiles = [
            ("TCICSTRN", "CICS.CC00", "CARDUSR", AccessIntent::Execute),
            ("TCICSTRN", "CICS.CA00", "CARDADM", AccessIntent::Execute),
            (
                "FACILITY",
                "CICS.PROGRAM.COSGN00C",
                "CARDUSR",
                AccessIntent::Execute,
            ),
            (
                "FACILITY",
                "CICS.PROGRAM.**",
                "CARDADM",
                AccessIntent::Execute,
            ),
            (
                "DATASET",
                "AWS.M2.CARDDEMO.**",
                "CARDUSR",
                AccessIntent::Read,
            ),
            ("QUEUE", "CICS.TD.**", "CARDADM", AccessIntent::Update),
            ("DB2", "CARDDEMO.**", "CARDADM", AccessIntent::Update),
            ("IMS", "CARDDEMO.**", "CARDADM", AccessIntent::Update),
            ("JES", "CARDDEMO.**", "CARDADM", AccessIntent::Control),
            ("OPERCMDS", "CARDDEMO.**", "CARDADM", AccessIntent::Control),
        ]
        .into_iter()
        .map(
            |(class, pattern, principal, access)| RacfProfileDefinition {
                class: class.into(),
                pattern: pattern.into(),
                owner: "WEBADM".into(),
                uacc: None,
                permissions: BTreeMap::from([(principal.into(), access)]),
            },
        )
        .collect();
        RacfManifest {
            groups: ["CARDUSR".into(), "CARDADM".into()].into_iter().collect(),
            users: vec![
                RacfUserDefinition {
                    user: "WEBUSER".into(),
                    credential: SecretRef::new("secret:webuser", Default::default()).unwrap(),
                    groups: ["CARDUSR".into()].into_iter().collect(),
                },
                RacfUserDefinition {
                    user: "WEBADM".into(),
                    credential: SecretRef::new("secret:webadmin", Default::default()).unwrap(),
                    groups: ["CARDADM".into()].into_iter().collect(),
                },
            ],
            profiles,
        }
    }
    #[test]
    fn authentication_and_replay_resolution_do_not_expose_secret() {
        let (service, resolver) = setup();
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let reference = SecretRef::new("secret:user", Default::default()).unwrap();
        service.add_user("IBMUSER", &reference).unwrap();
        assert_eq!(
            service
                .authenticate(
                    &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    &reference
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        resolver.insert("secret:bad", b"WRONG".to_vec());
        assert_eq!(
            service
                .authenticate(
                    &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    &SecretRef::new("secret:bad", Default::default()).unwrap()
                )
                .unwrap(),
            SecurityDecision::InvalidCredentials
        );
        assert!(!format!("{:?}", service.audits()).contains("PASSWORD"));
    }
    #[test]
    fn generic_profiles_are_specific_and_default_deny() {
        let (service, resolver) = setup();
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let reference = SecretRef::new("secret:user", Default::default()).unwrap();
        service.add_user("IBMUSER", &reference).unwrap();
        service
            .define_profile("DATASET", "USER.**", "IBMUSER", None)
            .unwrap();
        service
            .permit("DATASET", "USER.**", "IBMUSER", AccessIntent::Read)
            .unwrap();
        let user = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("USER.DATA", 246).unwrap(),
                    AccessIntent::Read
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("USER.DATA", 246).unwrap(),
                    AccessIntent::Execute
                )
                .unwrap(),
            SecurityDecision::Allow
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("USER.DATA", 246).unwrap(),
                    AccessIntent::Update
                )
                .unwrap(),
            SecurityDecision::Deny
        );
        assert_eq!(
            service.list_profiles("DATASET", None, 1).unwrap(),
            (vec!["USER.**".to_string()], false)
        );
        assert_eq!(
            service
                .authorize(
                    &user,
                    "DATASET",
                    &ResourceName::new("SYS1.PARMLIB", 246).unwrap(),
                    AccessIntent::Read
                )
                .unwrap(),
            SecurityDecision::Deny
        );
    }

    #[test]
    fn carddemo_audit_redacts_application_and_transport_secrets() {
        let (service, _) = setup();
        service
            .audit(AuditEvent {
                action: "CARDDEMO.SIGNON".into(),
                resource_hash: "sha256:resource".into(),
                decision: "DENY".into(),
                fields: BTreeMap::from([
                    ("password".into(), "TRANSPORT-PASSWORD".into()),
                    ("card_number".into(), "0000000000000001".into()),
                    ("queue_payload".into(), "SECRET-MESSAGE".into()),
                    ("protected_field".into(), "PRIVATE".into()),
                    ("transaction".into(), "CC00".into()),
                ]),
            })
            .unwrap();
        let audits = service.audits();
        assert_eq!(audits[0].fields["transaction"], "CC00");
        for key in [
            "password",
            "card_number",
            "queue_payload",
            "protected_field",
        ] {
            assert_eq!(audits[0].fields[key], "<redacted>");
        }
        let shown = format!("{audits:?}");
        for secret in [
            "TRANSPORT-PASSWORD",
            "0000000000000001",
            "SECRET-MESSAGE",
            "PRIVATE",
        ] {
            assert!(!shown.contains(secret));
        }
    }

    #[test]
    fn carddemo_manifest_is_atomic_distinct_and_least_privilege() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:webuser", b"WEB-PASSWORD".to_vec());
        resolver.insert("secret:webadmin", b"ADMIN-PASSWORD".to_vec());
        resolver.insert("secret:appuser", b"APP-PASSWORD".to_vec());
        let service =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        let receipt = service.install_manifest(carddemo_manifest()).unwrap();
        assert_eq!(
            (receipt.groups, receipt.users, receipt.profiles),
            (2, 2, 10)
        );
        assert!(!receipt.replayed);
        assert!(
            service
                .install_manifest(carddemo_manifest())
                .unwrap()
                .replayed
        );
        let regular = PrincipalId::new("WEBUSER", InvocationLimits::default()).unwrap();
        let admin = PrincipalId::new("WEBADM", InvocationLimits::default()).unwrap();
        assert_eq!(
            service
                .authenticate(
                    &PrincipalId::new("APPUSER", InvocationLimits::default()).unwrap(),
                    &SecretRef::new("secret:appuser", Default::default()).unwrap(),
                )
                .unwrap(),
            SecurityDecision::InvalidCredentials
        );
        for (class, resource, intent) in [
            ("TCICSTRN", "CICS.CC00", AccessIntent::Execute),
            ("FACILITY", "CICS.PROGRAM.COSGN00C", AccessIntent::Execute),
            (
                "DATASET",
                "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
                AccessIntent::Read,
            ),
        ] {
            assert_eq!(
                service
                    .authorize(
                        &regular,
                        class,
                        &ResourceName::new(resource, 246).unwrap(),
                        intent,
                    )
                    .unwrap(),
                SecurityDecision::Allow
            );
        }
        for (class, resource, intent) in [
            ("TCICSTRN", "CICS.CA00", AccessIntent::Execute),
            ("QUEUE", "CICS.TD.JOBS", AccessIntent::Update),
            (
                "DATASET",
                "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
                AccessIntent::Update,
            ),
            ("DB2", "CARDDEMO.PENDING", AccessIntent::Read),
            ("IMS", "CARDDEMO.AUTH", AccessIntent::Read),
            ("JES", "CARDDEMO.REPORT", AccessIntent::Control),
            ("OPERCMDS", "CARDDEMO.CEMT", AccessIntent::Control),
        ] {
            assert_eq!(
                service
                    .authorize(
                        &regular,
                        class,
                        &ResourceName::new(resource, 246).unwrap(),
                        intent,
                    )
                    .unwrap(),
                SecurityDecision::Deny
            );
        }
        for (class, resource, intent) in [
            ("QUEUE", "CICS.TD.JOBS", AccessIntent::Update),
            ("DB2", "CARDDEMO.PENDING", AccessIntent::Update),
            ("IMS", "CARDDEMO.AUTH", AccessIntent::Update),
            ("JES", "CARDDEMO.REPORT", AccessIntent::Control),
            ("OPERCMDS", "CARDDEMO.CEMT", AccessIntent::Control),
        ] {
            assert_eq!(
                service
                    .authorize(
                        &admin,
                        class,
                        &ResourceName::new(resource, 246).unwrap(),
                        intent,
                    )
                    .unwrap(),
                SecurityDecision::Allow
            );
        }
        service
            .audit(AuditEvent {
                action: "CARDDEMO".into(),
                resource_hash: "hash".into(),
                decision: "ALLOW".into(),
                fields: BTreeMap::from([("password".into(), "WEB-PASSWORD".into())]),
            })
            .unwrap();
        let restarted = RacfService::open(store, resolver, Default::default()).unwrap();
        assert_eq!(restarted.audits()[0].fields["password"], "<redacted>");

        let bounded_store: Arc<dyn ProviderStateStore> =
            Arc::new(MemoryStore::new(Default::default()));
        let bounded_resolver = Arc::new(MemorySecretResolver::default());
        bounded_resolver.insert("secret:webuser", b"WEB-PASSWORD".to_vec());
        bounded_resolver.insert("secret:webadmin", b"ADMIN-PASSWORD".to_vec());
        let bounded = RacfService::open(
            bounded_store,
            bounded_resolver,
            RacfLimits {
                max_profiles: 1,
                ..RacfLimits::default()
            },
        )
        .unwrap();
        assert_eq!(
            bounded.install_manifest(carddemo_manifest()),
            Err(HostProblem::ResourceExhausted)
        );
    }
    #[test]
    fn revoked_and_locked_fail_closed() {
        let (service, resolver) = setup();
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        let reference = SecretRef::new("secret:user", Default::default()).unwrap();
        service.add_user("IBMUSER", &reference).unwrap();
        service
            .set_user_state("IBMUSER", true, false, false)
            .unwrap();
        assert_eq!(
            service
                .authenticate(
                    &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    &reference
                )
                .unwrap(),
            SecurityDecision::Expired
        );
        service
            .set_user_state("IBMUSER", false, true, false)
            .unwrap();
        assert_eq!(
            service
                .authenticate(
                    &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    &reference
                )
                .unwrap(),
            SecurityDecision::Revoked
        );
    }
    #[test]
    fn sqlite_restart_preserves_users_groups_and_profiles() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-{}-{:?}",
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
                Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap());
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
                Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap());
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
}
