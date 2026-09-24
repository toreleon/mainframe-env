//! Durable APPC/MRO partner and profile resource definitions.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationSystemDefinition,
};
use crate::service::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};

const PARTNER_NAMESPACE: &str = "cics-conversation-partner-v1";
const PROFILE_NAMESPACE: &str = "cics-conversation-profile-v1";
const MAX_DEFINITION_BYTES: usize = 1024;
const MAX_REGISTRATION_RETRIES: usize = 32;

/// Installed PARTNER maps an eight-byte resource name to one system and
/// optional session-processing PROFILE/MODENAME.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationPartnerDefinition {
    pub name: String,
    pub sysid: String,
    pub profile: String,
}

impl ConversationPartnerDefinition {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if !valid_name(&self.name, 8)
            || !valid_name(&self.sysid, 4)
            || !valid_name(&self.profile, 8)
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

/// Installed profile affects bounded conversation data transfer. An APPC
/// basic definition names a mode group; mapped APPC/MRO use a PROFILE.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationProfileDefinition {
    pub name: String,
    pub kind: ConversationKind,
    pub maximum_data_bytes: u32,
}

impl ConversationProfileDefinition {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if !valid_name(&self.name, 8)
            || !(1..=1_048_576).contains(&self.maximum_data_bytes)
            || self.kind == ConversationKind::AppcBasic && self.name == "SNASVCMG"
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

fn valid_name(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
        })
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, HostProblem> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)?;
    if bytes.len() > MAX_DEFINITION_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}

fn write_immutable(
    store: &dyn ProviderStateStore,
    namespace: &str,
    name: &str,
    payload: Vec<u8>,
) -> Result<(), HostProblem> {
    if let Some(existing) = store
        .get_provider_state(namespace, name)
        .map_err(store_error)?
    {
        return if existing.version == 1 && existing.payload == payload {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    match store.put_provider_state(
        ProviderStateRecord {
            namespace: namespace.into(),
            key: name.into(),
            version: 1,
            payload,
        },
        None,
    ) {
        Ok(()) => Ok(()),
        Err(StoreError::Conflict) => Err(HostProblem::IdempotencyConflict),
        Err(error) => Err(store_error(error)),
    }
}

pub(super) fn load_partner(
    store: &dyn ProviderStateStore,
    name: &str,
) -> Result<Option<ConversationPartnerDefinition>, HostProblem> {
    let Some(row) = store
        .get_provider_state(PARTNER_NAMESPACE, name)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    let definition: ConversationPartnerDefinition =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if row.version != 1
        || row.key != definition.name
        || definition.validate().is_err()
        || encode(&definition)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Some(definition))
}

pub(super) fn load_profile(
    store: &dyn ProviderStateStore,
    name: &str,
    kind: ConversationKind,
) -> Result<Option<ConversationProfileDefinition>, HostProblem> {
    if name == "DFHCICSA" && kind != ConversationKind::AppcBasic
        || name == "DEFAULT" && kind == ConversationKind::AppcBasic
    {
        return Ok(Some(ConversationProfileDefinition {
            name: name.into(),
            kind,
            maximum_data_bytes: 32_767,
        }));
    }
    let Some(row) = store
        .get_provider_state(PROFILE_NAMESPACE, name)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    let definition: ConversationProfileDefinition =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if row.version != 1
        || row.key != definition.name
        || definition.validate().is_err()
        || encode(&definition)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if definition.kind != kind {
        return Ok(None);
    }
    Ok(Some(definition))
}

impl CicsService {
    /// Trusted ingress installs the principal APPC/MRO facility for one live
    /// task. Reissuing the same installation returns its durable token.
    pub fn install_conversation_principal_for_run(
        &self,
        run_unit: &RunUnitId,
        sysid: &str,
        kind: ConversationKind,
    ) -> Result<[u8; 4], HostProblem> {
        let state = self.lock()?;
        let run = state.runs.get(run_unit).ok_or(HostProblem::NotFound)?;
        let owner = ConversationOwner {
            execution: run.invocation.execution_id.as_str().into(),
            run_unit: run.invocation.run_unit_id.as_str().into(),
            lease_epoch: u64::from(run.invocation.attempt),
        };
        for _ in 0..MAX_REGISTRATION_RETRIES {
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            if let Some(existing) = current.conversations.values().find(|record| {
                record.owner.execution == owner.execution
                    && record.owner.run_unit == owner.run_unit
                    && record.principal_facility
                    && !record.released
            }) {
                return if existing.owner == owner
                    && existing.system == sysid
                    && existing.kind == kind
                {
                    Ok(existing.token)
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            let mut next = current.clone();
            let principal = next.install_principal(sysid, kind, owner.clone()).map_err(
                |problem| match problem {
                    ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
                    ConversationProblem::WrongState => HostProblem::IdempotencyConflict,
                    ConversationProblem::WrongKind => HostProblem::Unsupported,
                    _ => HostProblem::Malformed,
                },
            )?;
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(store_error)?
            {
                return Ok(principal.token);
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// Inspect one retained PARTNER definition through the same validated
    /// reader used by allocation commands.
    pub fn conversation_partner(
        &self,
        name: &str,
    ) -> Result<Option<ConversationPartnerDefinition>, HostProblem> {
        load_partner(self.store.as_ref(), name)
    }

    /// Inspect one retained PROFILE or MODENAME definition. The implicit
    /// source default is returned without creating a durable row.
    pub fn conversation_profile(
        &self,
        name: &str,
        kind: ConversationKind,
    ) -> Result<Option<ConversationProfileDefinition>, HostProblem> {
        load_profile(self.store.as_ref(), name, kind)
    }

    /// Install one bounded session group in the shared durable authority.
    pub fn register_conversation_system(
        &self,
        definition: ConversationSystemDefinition,
    ) -> Result<(), HostProblem> {
        for _ in 0..MAX_REGISTRATION_RETRIES {
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            if current.systems.get(&definition.sysid) == Some(&definition) {
                return Ok(());
            }
            let mut next = current.clone();
            next.register_system(definition.clone())
                .map_err(|problem| match problem {
                    ConversationProblem::Malformed => HostProblem::Malformed,
                    ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
                    _ => HostProblem::IdempotencyConflict,
                })?;
            if next == current
                || current
                    .persist(&mut next, self.store.as_ref())
                    .map_err(store_error)?
            {
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub fn register_conversation_partner(
        &self,
        definition: ConversationPartnerDefinition,
    ) -> Result<(), HostProblem> {
        definition.validate()?;
        let payload = encode(&definition)?;
        write_immutable(
            self.store.as_ref(),
            PARTNER_NAMESPACE,
            &definition.name,
            payload,
        )
    }

    pub fn register_conversation_profile(
        &self,
        definition: ConversationProfileDefinition,
    ) -> Result<(), HostProblem> {
        definition.validate()?;
        if matches!(definition.name.as_str(), "DFHCICSA" | "DEFAULT") {
            return Err(HostProblem::IdempotencyConflict);
        }
        let payload = encode(&definition)?;
        write_immutable(
            self.store.as_ref(),
            PROFILE_NAMESPACE,
            &definition.name,
            payload,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};

    #[test]
    fn partner_and_profile_are_immutable_and_restart_readable() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-conversation-definitions-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let first = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let partner = ConversationPartnerDefinition {
            name: "PARTNER1".into(),
            sysid: "SYS1".into(),
            profile: "FAST".into(),
        };
        let profile = ConversationProfileDefinition {
            name: "FAST".into(),
            kind: ConversationKind::AppcMapped,
            maximum_data_bytes: 1024,
        };
        partner.validate().unwrap();
        profile.validate().unwrap();
        write_immutable(
            &first,
            PARTNER_NAMESPACE,
            &partner.name,
            encode(&partner).unwrap(),
        )
        .unwrap();
        write_immutable(
            &first,
            PROFILE_NAMESPACE,
            &profile.name,
            encode(&profile).unwrap(),
        )
        .unwrap();
        drop(first);
        let reopened = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        assert_eq!(
            load_partner(&reopened, "PARTNER1"),
            Ok(Some(partner.clone()))
        );
        assert_eq!(
            load_profile(&reopened, "FAST", ConversationKind::AppcMapped),
            Ok(Some(profile.clone()))
        );
        assert_eq!(
            load_profile(&reopened, "FAST", ConversationKind::Mro),
            Ok(None)
        );
        assert_eq!(
            write_immutable(
                &reopened,
                PARTNER_NAMESPACE,
                &partner.name,
                encode(&partner).unwrap()
            ),
            Ok(())
        );
        let mut changed = partner;
        changed.profile = "SLOW".into();
        assert_eq!(
            write_immutable(
                &reopened,
                PARTNER_NAMESPACE,
                &changed.name,
                encode(&changed).unwrap()
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_names_and_reserved_mode_fail_before_storage() {
        let store = MemoryStore::new(Default::default());
        let invalid = ConversationPartnerDefinition {
            name: "lower".into(),
            sysid: "SYS1".into(),
            profile: "FAST".into(),
        };
        assert_eq!(invalid.validate(), Err(HostProblem::Malformed));
        assert!(load_partner(&store, "lower").unwrap().is_none());
        assert_eq!(
            ConversationProfileDefinition {
                name: "SNASVCMG".into(),
                kind: ConversationKind::AppcBasic,
                maximum_data_bytes: 1024,
            }
            .validate(),
            Err(HostProblem::Malformed)
        );
    }
}
