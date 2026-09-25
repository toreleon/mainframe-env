//! Installed BTS process types and local transaction definitions.

use super::*;
use crate::service::CicsService;

const PROCESS_TYPE_NAMESPACE: &str = "cics-bts-process-type-v1";
const TRANSACTION_NAMESPACE: &str = "cics-bts-transaction-v1";
const PROCESS_TYPE_SCHEMA: &str = "mainframe-env.cics.bts-process-type@1";
const TRANSACTION_SCHEMA: &str = "mainframe-env.cics.bts-transaction@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsProcessTypeDefinition {
    pub schema_version: String,
    pub name: String,
    pub repository_resource: String,
    pub enabled: bool,
}

impl BtsProcessTypeDefinition {
    pub fn new(name: &str, repository_resource: &str, enabled: bool) -> Result<Self, HostProblem> {
        let definition = Self {
            schema_version: PROCESS_TYPE_SCHEMA.into(),
            name: name.into(),
            repository_resource: repository_resource.into(),
            enabled,
        };
        definition.validate()?;
        Ok(definition)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != PROCESS_TYPE_SCHEMA
            || validate_name(&self.name, 8, true).is_err()
            || validate_identifier(&self.repository_resource, 44).is_err()
            || !self.repository_resource.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'@' | b'$' | b'#' | b'_')
            })
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsTransactionDefinition {
    pub schema_version: String,
    pub transid: String,
    pub program: String,
    pub enabled: bool,
    pub remote: bool,
}

impl BtsTransactionDefinition {
    pub fn new(
        transid: &str,
        program: &str,
        enabled: bool,
        remote: bool,
    ) -> Result<Self, HostProblem> {
        let definition = Self {
            schema_version: TRANSACTION_SCHEMA.into(),
            transid: transid.into(),
            program: program.into(),
            enabled,
            remote,
        };
        definition.validate()?;
        Ok(definition)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != TRANSACTION_SCHEMA
            || validate_identifier(&self.transid, 4).is_err()
            || validate_identifier(&self.program, 8).is_err()
            || ![self.transid.as_bytes(), self.program.as_bytes()]
                .into_iter()
                .flatten()
                .all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'@' | b'$' | b'#')
                })
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

impl CicsService {
    /// Install an immutable process-type/repository binding for BTS DEFINE.
    pub fn register_bts_process_type(
        &self,
        definition: BtsProcessTypeDefinition,
    ) -> Result<(), HostProblem> {
        BtsLifecycleStore::new(self.store.as_ref()).register_process_type(definition)
    }

    /// Install an immutable local transaction/program binding for BTS RUN.
    pub fn register_bts_transaction(
        &self,
        definition: BtsTransactionDefinition,
    ) -> Result<(), HostProblem> {
        BtsLifecycleStore::new(self.store.as_ref()).register_transaction(definition)
    }
}

impl<'a> BtsLifecycleStore<'a> {
    pub fn register_process_type(
        &self,
        definition: BtsProcessTypeDefinition,
    ) -> Result<(), HostProblem> {
        definition.validate()?;
        let payload =
            serde_json::to_vec(&definition).map_err(|_| HostProblem::ResourceExhausted)?;
        put_immutable(
            self.store,
            PROCESS_TYPE_NAMESPACE,
            &definition.name,
            payload,
        )
    }

    pub fn register_transaction(
        &self,
        definition: BtsTransactionDefinition,
    ) -> Result<(), HostProblem> {
        definition.validate()?;
        let payload =
            serde_json::to_vec(&definition).map_err(|_| HostProblem::ResourceExhausted)?;
        put_immutable(
            self.store,
            TRANSACTION_NAMESPACE,
            &definition.transid,
            payload,
        )
    }

    pub fn load_process_type(
        &self,
        name: &str,
    ) -> Result<Option<BtsProcessTypeDefinition>, HostProblem> {
        validate_name(name, 8, true)?;
        let Some(row) = self
            .store
            .get_provider_state(PROCESS_TYPE_NAMESPACE, name)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version != 1 || row.payload.len() > 512 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let definition: BtsProcessTypeDefinition =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        definition
            .validate()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if definition.name != name {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(definition))
    }

    pub fn load_transaction(
        &self,
        transid: &str,
    ) -> Result<Option<BtsTransactionDefinition>, HostProblem> {
        validate_identifier(transid, 4)?;
        let Some(row) = self
            .store
            .get_provider_state(TRANSACTION_NAMESPACE, transid)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version != 1 || row.payload.len() > 512 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let definition: BtsTransactionDefinition =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        definition
            .validate()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if definition.transid != transid {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(definition))
    }
}

fn put_immutable(
    store: &dyn ProviderStateStore,
    namespace: &str,
    key: &str,
    payload: Vec<u8>,
) -> Result<(), HostProblem> {
    if payload.len() > 512 {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(row) = store
        .get_provider_state(namespace, key)
        .map_err(store_error)?
    {
        return if row.version == 1 && row.payload == payload {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: 1,
                payload,
            },
            None,
        )
        .map_err(store_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::MemoryStore;

    #[test]
    fn installed_bts_catalog_is_durable_and_exact() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let process_type = BtsProcessTypeDefinition::new("ORDERS", "BTS.ORDER.REPO", true).unwrap();
        let transaction = BtsTransactionDefinition::new("BTS1", "MAIN", true, false).unwrap();
        authority
            .register_process_type(process_type.clone())
            .unwrap();
        authority.register_transaction(transaction.clone()).unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        assert_eq!(
            reopened.load_process_type("ORDERS").unwrap(),
            Some(process_type.clone())
        );
        assert_eq!(
            reopened.load_transaction("BTS1").unwrap(),
            Some(transaction)
        );
        assert!(reopened.register_process_type(process_type).is_ok());
        assert!(
            reopened
                .register_process_type(
                    BtsProcessTypeDefinition::new("ORDERS", "OTHER.REPO", true).unwrap()
                )
                .is_err()
        );
        assert!(reopened.load_process_type("UNKNOWN").unwrap().is_none());
    }
}
