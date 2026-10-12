//! First-party finite profile declarations prepared under the real RACF writer.
use super::*;
use crate::model::{ClassDescriptor, ProfileTemplate};
use mainframe_env_store_api::{MAX_PROVIDER_COMPOSITION_BYTES, ProviderCompositionCommit};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::MutexGuard;

const PROFILE_CONTRACT: &str = "mainframe-env.application-local-racf-profile@1";

/// Explicit finite declaration to authenticate as a signed package Profile payload.
/// This does not create classes, templates, principals, credentials or vendor defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfProfileComposition {
    /// Existing normalized class name.
    pub class: String,
    /// Exact normalized generic or discrete profile name.
    pub name: String,
    /// Existing owner principal or group.
    pub owner: String,
    /// Explicit universal access level, including NONE.
    pub uacc: AccessLevel,
    /// Complete ordered unconditional principal access list, including explicit NONE entries.
    pub permissions: BTreeMap<String, AccessLevel>,
}

#[derive(Serialize)]
struct ProfilePayload<'a> {
    contract: &'static str,
    declaration: &'a RacfProfileComposition,
}

impl RacfProfileComposition {
    /// Exact first-party Profile manifest path for this class and profile.
    pub fn entry_path(&self) -> Result<String, HostProblem> {
        self.validate_declaration()?;
        Ok(format!("security-profile/{}/{}", self.class, self.name))
    }
    /// Canonical signed bytes; provider versions are assigned by the actual current snapshot.
    pub fn canonical_payload(&self) -> Result<Vec<u8>, HostProblem> {
        self.validate_declaration()?;
        let payload = serde_json::to_vec(&ProfilePayload {
            contract: PROFILE_CONTRACT,
            declaration: self,
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(payload)
    }
    fn validate_declaration(&self) -> Result<(), HostProblem> {
        fn token(value: &str, max: usize) -> bool {
            !value.is_empty()
                && value.len() <= max
                && value.bytes().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"@#$-_".contains(&byte)
                })
        }
        if !token(&self.class, 32)
            || !token(&self.owner, 8)
            || self.name.is_empty()
            || self.name.len() > 246
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"@#$.*%_".contains(&byte)
            })
            || self.permissions.len() > 1024
            || self.permissions.keys().any(|name| !token(name, 8))
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
    fn profile(&self, version: u64) -> ResourceProfile {
        ResourceProfile {
            class: self.class.clone(),
            name: self.name.clone(),
            owner: self.owner.clone(),
            generic: self.name.bytes().any(|byte| matches!(byte, b'*' | b'%')),
            uacc: self.uacc,
            audit: AuditPolicy::None,
            security_level: 0,
            security_label: None,
            categories: BTreeSet::new(),
            access_list: self
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
            version,
        }
    }
}

/// Exact selected profile and its existing class/template context, excluding runtime authority domains.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfProfileProjection {
    /// Actual complete persisted profile, with monotonically assigned provider version.
    pub profile: ResourceProfile,
    /// Actual existing class descriptor required by that profile.
    pub class: ClassDescriptor,
    /// Actual existing template required by that class.
    pub template: ProfileTemplate,
}
impl RacfProfileProjection {
    /// Canonical identity excluding the assigned profile version for retained rollback.
    /// The stored selection still compares the full actual version during readiness checks.
    pub fn content_payload(&self) -> Result<Vec<u8>, HostProblem> {
        let original = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if original.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut content = self.clone();
        content.profile.version = 1;
        serde_json::to_vec(&content).map_err(|_| HostProblem::InfrastructureFailure)
    }
    /// Exact normalized provider key of this profile.
    #[must_use]
    pub fn key(&self) -> String {
        profile_key(&self.profile.class, &self.profile.name)
    }
}

/// One real whole-authority write held through the genuine shared-store acknowledgement.
pub struct PreparedRacfProfiles<'a> {
    database: &'a SecurityDatabase,
    _writer: MutexGuard<'a, ()>,
    writes: Vec<ProviderStateWrite>,
    projections: Vec<RacfProfileProjection>,
}

impl SecurityDatabase {
    pub(crate) fn require_composition_known(&self) -> Result<(), HostProblem> {
        if self
            .composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            Err(HostProblem::UnknownOutcome)
        } else {
            Ok(())
        }
    }

    /// Prepare only explicit profiles under the existing database writer; no live row is changed.
    /// A full exact prior projection owns replacement; foreign selected keys refuse even on retry.
    pub fn prepare_profile_composition(
        &self,
        declarations: &[RacfProfileComposition],
        previous: &[RacfProfileProjection],
        foreign_selected: &BTreeSet<String>,
    ) -> Result<PreparedRacfProfiles<'_>, HostProblem> {
        self.require_composition_known()?;
        if declarations.is_empty() || declarations.len() > 4096 || previous.len() > 4096 {
            return Err(HostProblem::Malformed);
        }
        let mut declaration_bytes = 0usize;
        let mut keys = BTreeSet::new();
        for declaration in declarations {
            declaration_bytes = declaration_bytes
                .checked_add(declaration.canonical_payload()?.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            if declaration_bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
                return Err(HostProblem::ResourceExhausted);
            }
            let key = profile_key(&declaration.class, &declaration.name);
            if !keys.insert(key.clone()) || foreign_selected.contains(&key) {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        if !previous.is_empty()
            && previous
                .iter()
                .map(RacfProfileProjection::key)
                .collect::<BTreeSet<_>>()
                != keys
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_composition_known()?;
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if record.payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 4 {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut snapshot = decode_snapshot(&record, self.limits)?;
        verify_projections(&snapshot, previous)?;
        let mut changed = false;
        let mut projections = Vec::new();
        let mut projection_bytes = 0usize;
        for declaration in declarations {
            let key = profile_key(&declaration.class, &declaration.name);
            let class = snapshot
                .classes
                .get(&declaration.class)
                .ok_or(HostProblem::NotFound)?;
            let template = snapshot
                .templates
                .get(&class.profile_template)
                .ok_or(HostProblem::NotFound)?;
            // Charge each repeated context before cloning it into a selected projection.
            // A large shared template cannot be multiplied by the profile count unchecked.
            for encoded in [
                serde_json::to_vec(class),
                serde_json::to_vec(template),
                serde_json::to_vec(&declaration.profile(1)),
            ] {
                let encoded = encoded.map_err(|_| HostProblem::InfrastructureFailure)?;
                projection_bytes = projection_bytes
                    .checked_add(encoded.len())
                    .and_then(|bytes| bytes.checked_add(96))
                    .ok_or(HostProblem::ResourceExhausted)?;
                if projection_bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
                    return Err(HostProblem::ResourceExhausted);
                }
            }
            let class = class.clone();
            let template = template.clone();
            // A raclisted class would require its separately owned cache participant.
            if class.raclist || snapshot.raclist_caches.contains_key(&declaration.class) {
                return Err(HostProblem::Unsupported);
            }
            let current = snapshot.profiles.get(&key);
            if current.is_some() && !previous.iter().any(|projection| projection.key() == key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let candidate = declaration.profile(current.map_or(1, |profile| profile.version));
            let profile = if current == Some(&candidate) {
                candidate
            } else {
                changed = true;
                let version = current
                    .map_or(Some(1), |profile| profile.version.checked_add(1))
                    .ok_or(HostProblem::ResourceExhausted)?;
                declaration.profile(version)
            };
            snapshot.profiles.insert(key, profile.clone());
            projections.push(RacfProfileProjection {
                profile,
                class,
                template,
            });
        }
        projections.sort_by_key(RacfProfileProjection::key);
        snapshot.validate(self.limits).map_err(schema_problem)?;
        let writes = if changed {
            snapshot.generation = record
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let payload = encode_snapshot(&snapshot, self.limits)?;
            if record
                .payload
                .len()
                .checked_add(payload.len())
                .is_none_or(|bytes| bytes > MAX_PROVIDER_COMPOSITION_BYTES / 2)
            {
                return Err(HostProblem::ResourceExhausted);
            }
            vec![ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: snapshot.generation,
                    payload,
                },
                expected_version: Some(record.version),
            }]
        } else {
            Vec::new()
        };
        Ok(PreparedRacfProfiles {
            database: self,
            _writer: writer,
            writes,
            projections,
        })
    }

    /// Check full current participating profiles under the writer, allowing unrelated runtime domains to change.
    pub fn selected_profile_composition_matches(
        &self,
        projections: &[RacfProfileProjection],
    ) -> Result<(), HostProblem> {
        self.require_composition_known()?;
        if projections.is_empty() || projections.len() > 4096 {
            return Err(HostProblem::Malformed);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_composition_known()?;
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        verify_projections(&decode_snapshot(&record, self.limits)?, projections)
    }
}

fn verify_projections(
    snapshot: &SecurityDatabaseSnapshot,
    projections: &[RacfProfileProjection],
) -> Result<(), HostProblem> {
    let mut keys = BTreeSet::new();
    for projection in projections {
        if !keys.insert(projection.key())
            || projection.profile.version == 0
            || snapshot.profiles.get(&projection.key()) != Some(&projection.profile)
            || snapshot.classes.get(&projection.profile.class) != Some(&projection.class)
            || snapshot.templates.get(&projection.class.profile_template)
                != Some(&projection.template)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok(())
}

impl PreparedRacfProfiles<'_> {
    /// Complete real snapshot write, or none for an exact unchanged retry.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Complete current participating profiles retained in the local selection.
    pub fn projections(&self) -> &[RacfProfileProjection] {
        &self.projections
    }
    /// Check actual same-store acknowledged writes under the still-held writer.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.database.store, &self.writes)
    }
    /// Quarantine this live database after an unknown shared commit, until reopen.
    pub fn fence_unknown(&self) {
        self.database
            .composition_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Release the writer only after the exact acknowledged write is admitted.
    pub fn finish(self, commit: &ProviderCompositionCommit) -> Result<(), HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}
