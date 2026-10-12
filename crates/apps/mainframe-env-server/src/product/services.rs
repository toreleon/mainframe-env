//! Existing in-memory constructors, subsystem accessors and trace queries.
use super::*;

impl ProductServer {
    #[must_use]
    pub fn readiness(&self) -> ProductReadiness {
        let capacity = self
            .config
            .retention
            .policy()
            .ok()
            .and_then(|policy| self.store.retention_capacity_health(policy).ok());
        let retention_capacity = capacity
            .as_ref()
            .map_or(ProductCapacityStatus::Unavailable, |health| {
                health.saturation.into()
            });
        ProductReadiness {
            accepting: self.accepting.load(Ordering::SeqCst)
                && !self.local_composition_uncertain.load(Ordering::SeqCst),
            writable_store: capacity.is_some(),
            retention_capacity,
            retention_warning: retention_capacity.warning(),
            bootstrap_identity: self.bootstrap_principal_ready(),
            host_capabilities: [
                "host.cics.execute",
                "host.db2.read",
                "host.db2.write",
                "host.ims.read",
                "host.ims.write",
                "host.mq.read",
                "host.mq.write",
            ]
            .into_iter()
            .all(|capability| self.host.capability_ready(capability)),
            artifact_store: self.artifacts.is_ready(),
            jes_workers: self.jes_workers_ready(),
        }
    }

    pub(super) fn online_transaction(
        &self,
        transaction: &str,
    ) -> Result<Option<(String, ArtifactRef)>, GatewayProblem> {
        self.require_local_owner_known().map_err(gateway_problem)?;
        let transaction = normalize_online_name(transaction, 16).map_err(gateway_problem)?;
        let programs = self
            .online_programs
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        let transactions = self
            .online_transactions
            .lock()
            .map_err(|_| gateway_problem(HostProblem::InfrastructureFailure))?;
        // Check after acquiring both owners; an unknown commit cannot leak stale routes.
        self.require_local_owner_known().map_err(gateway_problem)?;
        let Some(program) = transactions.get(&transaction).cloned() else {
            return Ok(None);
        };
        let artifact = programs
            .get(&program)
            .cloned()
            .ok_or_else(|| gateway_problem(HostProblem::InfrastructureFailure))?;
        Ok(Some((program, artifact)))
    }

    pub fn memory(config: ServerConfig) -> Result<Arc<Self>, HostProblem> {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let program = default_program_router();
        Self::open(config, store, secrets, program)
    }

    pub fn memory_with_package_trust(
        config: ServerConfig,
        package_trust: Arc<dyn PackageSignatureVerifier>,
    ) -> Result<Arc<Self>, HostProblem> {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let program = default_program_router();
        Self::open_with_package_trust(config, store, secrets, program, package_trust)
    }

    #[must_use]
    pub fn application_installer(&self) -> ApplicationInstaller {
        self.applications.clone()
    }

    #[must_use]
    pub fn cics_service(&self) -> Arc<CicsService> {
        self.cics.clone()
    }

    #[must_use]
    pub fn batch_service(&self) -> Arc<BatchService> {
        self.batch.clone()
    }

    #[must_use]
    pub fn dataset_service(&self) -> Arc<DatasetService> {
        self.dataset.clone()
    }

    #[must_use]
    pub fn db2_service(&self) -> Arc<Db2Service> {
        self.db2.clone()
    }

    #[must_use]
    pub fn ims_service(&self) -> Arc<ImsService> {
        self.ims.clone()
    }

    #[must_use]
    pub fn mq_service(&self) -> Arc<MqService> {
        self.mq.clone()
    }

    #[must_use]
    pub fn racf_service(&self) -> Arc<RacfService> {
        self.racf.clone()
    }

    pub fn online_trace(&self, session: &str) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        Ok(self
            .online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(session)
            .cloned()
            .unwrap_or_default())
    }

    pub fn online_operation_count(&self, operation: CicsOperation) -> Result<usize, HostProblem> {
        Ok(self
            .online_traces
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .values()
            .flatten()
            .filter(|entry| entry.operation == operation)
            .count())
    }
}
