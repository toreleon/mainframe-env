//! Existing in-memory constructors, subsystem accessors and trace queries.
use super::*;

impl ProductServer {
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
