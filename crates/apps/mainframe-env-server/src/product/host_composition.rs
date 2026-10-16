//! Private host assembly. Only server unit builds can inject a wall-clock provider.
use super::*;

#[derive(Default)]
pub(super) struct RuntimeClocks {
    pub(super) jes: Option<Arc<dyn JesClock>>,
    #[cfg(test)]
    pub(super) host: Option<Arc<dyn HostProvider>>,
}

impl RuntimeClocks {
    pub(super) fn host_clock(
        &self,
        limits: InvocationLimits,
    ) -> Result<Arc<dyn HostProvider>, HostProblem> {
        #[cfg(test)]
        if let Some(clock) = &self.host {
            let descriptor = clock.descriptor();
            if descriptor.capability.as_str() != "host.clock"
                || descriptor.request_schema != "mainframe-env.clock.request@1"
                || descriptor.result_schema != "mainframe-env.clock.response@1"
                || descriptor.max_request_bytes != 256
                || descriptor.max_result_bytes != 256
                || !descriptor.ready
            {
                return Err(HostProblem::Malformed);
            }
            return Ok(clock.clone());
        }
        Ok(Arc::new(SystemClockProvider::new(limits)))
    }
}

// Preserve the private JES-clock constructor used by the existing server tests.
impl ProductServer {
    pub(super) fn open_configured(
        config: ServerConfig,
        store: Arc<dyn PlatformStore>,
        secrets: Arc<MemorySecretResolver>,
        program: Arc<DefaultProgramRouter>,
        package_trust: Arc<dyn PackageSignatureVerifier>,
        artifacts: Arc<ProductArtifactStore>,
        jes_clock: Option<Arc<dyn JesClock>>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_configured_with_clocks(
            config,
            store,
            secrets,
            program,
            package_trust,
            artifacts,
            RuntimeClocks {
                jes: jes_clock,
                #[cfg(test)]
                host: None,
            },
        )
    }
}

pub(super) fn scoped_host_with_clock(
    racf: &Arc<RacfService>,
    dataset: &Arc<DatasetService>,
    program: Arc<dyn HostProvider>,
    additional: Vec<Arc<dyn HostProvider>>,
    include_cics: bool,
    cics: Option<Arc<dyn HostProvider>>,
    clock: Arc<dyn HostProvider>,
) -> Result<Arc<ScopedHostService>, HostProblem> {
    let limits = InvocationLimits::default();
    let mut providers = dataset_providers(dataset.clone(), limits);
    providers.extend(racf_providers(racf.clone(), limits));
    providers.push(clock);
    providers.push(Arc::new(BatchTerminalProvider::new(limits)) as Arc<dyn HostProvider>);
    providers.push(program);
    providers.extend(additional);
    if include_cics {
        providers.push(cics.ok_or(HostProblem::InfrastructureFailure)?);
    }
    Ok(Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, providers, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ),
        HostLimits::default(),
    )))
}
