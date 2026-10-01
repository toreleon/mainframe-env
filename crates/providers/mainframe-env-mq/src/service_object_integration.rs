//! Package topology installation and versioned catalog-row compatibility.

use super::*;
use crate::object_service::{catalog_limits, object_problem};

impl MqService {
    /// Install package-supplied typed MQ topology into the existing provider-row store.
    /// The legacy queue-only installer remains a compatibility entry point.
    pub fn install_object_catalog(
        &self,
        catalog: MqObjectCatalog,
    ) -> Result<MqInstallReceipt, HostProblem> {
        let encoded = catalog.encode().map_err(object_problem)?;
        let catalog = MqObjectCatalog::decode(&encoded, catalog_limits(self.limits))
            .map_err(object_problem)?;
        if catalog.model_instances().next().is_some() {
            return Err(HostProblem::ProviderFailure);
        }
        let identity = format!("sha256:{:x}", Sha256::digest(&encoded));
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.catalog {
            return if current.encode().map_err(object_problem)? == encoded {
                Ok(catalog_receipt(&catalog, identity, true))
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if durable.state.definitions.is_some() || !durable.state.queues.is_empty() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.state.scoped_snapshot();
        for (name, trigger_process) in local_definitions(&catalog) {
            next.queues.insert(
                name.as_str().into(),
                Arc::new(Queue {
                    trigger_program: trigger_process.as_ref().map(|name| name.as_str().into()),
                    messages: Vec::new(),
                }),
            );
        }
        next.catalog = Some(Arc::new(catalog.clone()));
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)?;
        Ok(catalog_receipt(&catalog, identity, false))
    }

    /// Read the single object authority reconstructed from versioned provider rows.
    pub fn object_catalog(&self) -> Result<Option<MqObjectCatalog>, HostProblem> {
        Ok(self.lock()?.state.catalog.as_deref().cloned())
    }
}

pub(super) fn load_catalog_row(
    store: &dyn ProviderStateStore,
    limits: MqLimits,
    versions: &mut RowVersions,
) -> Result<Option<Arc<MqObjectCatalog>>, HostProblem> {
    let rows: BTreeMap<String, String> =
        load_row_map(store, CATALOG_NAMESPACE, 1, limits, versions)?;
    match rows.into_iter().next() {
        None => Ok(None),
        Some((key, encoded)) if key == CATALOG_KEY => Ok(Some(Arc::new(
            MqObjectCatalog::decode(encoded.as_bytes(), catalog_limits(limits))
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))),
        Some(_) => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn encode_catalog_row(catalog: &MqObjectCatalog) -> Result<Vec<u8>, HostProblem> {
    let snapshot = catalog.encode().map_err(object_problem)?;
    let text = String::from_utf8(snapshot).map_err(|_| HostProblem::InfrastructureFailure)?;
    encode_object_row(CATALOG_KEY, &text)
}

fn catalog_receipt(
    catalog: &MqObjectCatalog,
    identity: String,
    replayed: bool,
) -> MqInstallReceipt {
    MqInstallReceipt {
        queues: local_definitions(catalog).count(),
        triggers: local_definitions(catalog)
            .filter(|(_, trigger)| trigger.is_some())
            .count(),
        identity,
        replayed,
    }
}
