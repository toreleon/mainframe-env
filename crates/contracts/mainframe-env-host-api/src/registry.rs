use crate::{EffectRequest, EffectResult, HostProblem, SemanticNamespace, SemanticOperationId};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

pub const SUBSYSTEM_HANDLER_REGISTRY_CONTRACT: &str = "mainframe-env.subsystem-handler-registry@1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    pub capability: CapabilityId,
    pub provider_id: String,
    pub generation: String,
    pub request_schema: String,
    pub result_schema: String,
    pub max_request_bytes: usize,
    pub max_result_bytes: usize,
    pub ready: bool,
}

impl CapabilityDescriptor {
    pub fn validate(&self, limits: InvocationLimits) -> Result<(), RegistryProblem> {
        let values = [
            &self.provider_id,
            &self.generation,
            &self.request_schema,
            &self.result_schema,
        ];
        if values
            .iter()
            .any(|value| value.is_empty() || value.len() > limits.max_identity_bytes)
            || self.max_request_bytes == 0
            || self.max_result_bytes == 0
        {
            Err(RegistryProblem::InvalidDescriptor)
        } else {
            Ok(())
        }
    }
}

pub trait HostProvider: Send + Sync {
    fn descriptor(&self) -> &CapabilityDescriptor;
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult;
}

#[derive(Clone)]
pub struct RegistrySnapshot {
    generation: u64,
    providers: BTreeMap<CapabilityId, Arc<dyn HostProvider>>,
}

impl RegistrySnapshot {
    pub fn new(
        generation: u64,
        providers: Vec<Arc<dyn HostProvider>>,
        limits: InvocationLimits,
    ) -> Result<Self, RegistryProblem> {
        if generation == 0 || providers.len() > limits.max_capabilities {
            return Err(RegistryProblem::LimitExceeded);
        }
        let mut selected = BTreeMap::new();
        for provider in providers {
            provider.descriptor().validate(limits)?;
            let capability = provider.descriptor().capability.clone();
            if selected.insert(capability, provider).is_some() {
                return Err(RegistryProblem::DuplicateCapability);
            }
        }
        Ok(Self {
            generation,
            providers: selected,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn select(&self, capability: &CapabilityId) -> Result<Arc<dyn HostProvider>, HostProblem> {
        let provider = self
            .providers
            .get(capability)
            .ok_or(HostProblem::Unsupported)?;
        if !provider.descriptor().ready {
            return Err(HostProblem::ProviderFailure);
        }
        Ok(Arc::clone(provider))
    }

    #[must_use]
    pub fn capabilities(&self) -> impl ExactSizeIterator<Item = &CapabilityId> {
        self.providers.keys()
    }
}

pub struct RegistryPublisher {
    current: RwLock<Arc<RegistrySnapshot>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubsystemHandlerDescriptor {
    pub semantic_id: SemanticOperationId,
    pub subsystem: String,
    pub generation: String,
    pub request_schema: String,
    pub result_schema: String,
    pub ready: bool,
}

impl SubsystemHandlerDescriptor {
    pub fn validate(&self, limits: InvocationLimits) -> Result<(), RegistryProblem> {
        if [
            &self.subsystem,
            &self.generation,
            &self.request_schema,
            &self.result_schema,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > limits.max_identity_bytes)
        {
            return Err(RegistryProblem::InvalidDescriptor);
        }
        if self.semantic_id.namespace() == SemanticNamespace::Official
            && self
                .semantic_id
                .official_descriptor()
                .is_none_or(|identity| identity.subsystem != self.subsystem)
        {
            return Err(RegistryProblem::WrongSubsystem);
        }
        Ok(())
    }
}

pub trait SubsystemHandler: Send + Sync {
    fn descriptor(&self) -> &SubsystemHandlerDescriptor;
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult;
}

#[derive(Clone)]
pub struct SubsystemHandlerRegistry {
    generation: u64,
    handlers: BTreeMap<SemanticOperationId, Arc<dyn SubsystemHandler>>,
}

impl SubsystemHandlerRegistry {
    pub fn new(
        generation: u64,
        handlers: Vec<Arc<dyn SubsystemHandler>>,
        limits: InvocationLimits,
    ) -> Result<Self, RegistryProblem> {
        if generation == 0 || handlers.len() > limits.max_capabilities {
            return Err(RegistryProblem::LimitExceeded);
        }
        let mut selected = BTreeMap::new();
        for handler in handlers {
            handler.descriptor().validate(limits)?;
            let identity = handler.descriptor().semantic_id.clone();
            if selected.insert(identity, handler).is_some() {
                return Err(RegistryProblem::DuplicateSemanticIdentity);
            }
        }
        Ok(Self {
            generation,
            handlers: selected,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    pub fn select(
        &self,
        identity: &SemanticOperationId,
    ) -> Result<Arc<dyn SubsystemHandler>, HostProblem> {
        let handler = self
            .handlers
            .get(identity)
            .ok_or(HostProblem::Unsupported)?;
        if !handler.descriptor().ready {
            return Err(HostProblem::ProviderFailure);
        }
        Ok(Arc::clone(handler))
    }

    pub fn identities(&self) -> impl ExactSizeIterator<Item = &SemanticOperationId> {
        self.handlers.keys()
    }
}

pub struct SubsystemHandlerPublisher {
    current: RwLock<Arc<SubsystemHandlerRegistry>>,
}

impl SubsystemHandlerPublisher {
    #[must_use]
    pub fn new(initial: SubsystemHandlerRegistry) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    pub fn snapshot(&self) -> Result<Arc<SubsystemHandlerRegistry>, RegistryProblem> {
        self.current
            .read()
            .map(|registry| Arc::clone(&registry))
            .map_err(|_| RegistryProblem::Poisoned)
    }

    pub fn publish(&self, next: SubsystemHandlerRegistry) -> Result<(), RegistryProblem> {
        let mut current = self
            .current
            .write()
            .map_err(|_| RegistryProblem::Poisoned)?;
        if next.generation() <= current.generation() {
            return Err(RegistryProblem::StaleGeneration);
        }
        *current = Arc::new(next);
        Ok(())
    }
}

impl RegistryPublisher {
    #[must_use]
    pub fn new(initial: RegistrySnapshot) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    pub fn snapshot(&self) -> Result<Arc<RegistrySnapshot>, RegistryProblem> {
        self.current
            .read()
            .map(|snapshot| Arc::clone(&snapshot))
            .map_err(|_| RegistryProblem::Poisoned)
    }

    pub fn publish(&self, next: RegistrySnapshot) -> Result<(), RegistryProblem> {
        let mut current = self
            .current
            .write()
            .map_err(|_| RegistryProblem::Poisoned)?;
        if next.generation() <= current.generation() {
            return Err(RegistryProblem::StaleGeneration);
        }
        *current = Arc::new(next);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryProblem {
    InvalidDescriptor,
    DuplicateCapability,
    DuplicateSemanticIdentity,
    WrongSubsystem,
    LimitExceeded,
    StaleGeneration,
    Poisoned,
}
impl std::fmt::Display for RegistryProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "host registry failed: {self:?}")
    }
}
impl std::error::Error for RegistryProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::official_semantic_identities;

    struct Provider {
        descriptor: CapabilityDescriptor,
    }
    impl HostProvider for Provider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
            EffectResult {
                sequence: request.sequence,
                outcome: Err(HostProblem::Unsupported),
            }
        }
    }
    fn provider(ready: bool) -> Arc<dyn HostProvider> {
        let limits = InvocationLimits::default();
        Arc::new(Provider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.dataset.read", limits).unwrap(),
                provider_id: "dataset".into(),
                generation: "1".into(),
                request_schema: "request@1".into(),
                result_schema: "result@1".into(),
                max_request_bytes: 1024,
                max_result_bytes: 1024,
                ready,
            },
        })
    }

    struct Handler {
        descriptor: SubsystemHandlerDescriptor,
    }

    impl SubsystemHandler for Handler {
        fn descriptor(&self) -> &SubsystemHandlerDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
            EffectResult {
                sequence: request.sequence,
                outcome: Err(HostProblem::Unsupported),
            }
        }
    }

    fn handler(subsystem: &str, ready: bool) -> Arc<dyn SubsystemHandler> {
        let limits = InvocationLimits::default();
        Arc::new(Handler {
            descriptor: SubsystemHandlerDescriptor {
                semantic_id: SemanticOperationId::new(official_semantic_identities()[0].id, limits)
                    .unwrap(),
                subsystem: subsystem.into(),
                generation: "handler-generation@1".into(),
                request_schema: "mainframe-env.effect-request@1".into(),
                result_schema: "mainframe-env.effect-result@1".into(),
                ready,
            },
        })
    }

    #[test]
    fn snapshot_is_deterministic_and_duplicate_free() {
        let limits = InvocationLimits::default();
        assert!(RegistrySnapshot::new(1, vec![provider(true), provider(true)], limits).is_err());
        let snapshot = RegistrySnapshot::new(1, vec![provider(true)], limits).unwrap();
        let capability = CapabilityId::new("host.dataset.read", limits).unwrap();
        assert!(snapshot.select(&capability).is_ok());
    }

    #[test]
    fn unready_provider_fails_closed() {
        let limits = InvocationLimits::default();
        let snapshot = RegistrySnapshot::new(1, vec![provider(false)], limits).unwrap();
        let capability = CapabilityId::new("host.dataset.read", limits).unwrap();
        assert_eq!(
            snapshot.select(&capability).err(),
            Some(HostProblem::ProviderFailure)
        );
    }

    #[test]
    fn publication_is_monotonic_and_snapshots_remain_immutable() {
        let limits = InvocationLimits::default();
        let first = RegistrySnapshot::new(1, vec![provider(true)], limits).unwrap();
        let publisher = RegistryPublisher::new(first);
        let held = publisher.snapshot().unwrap();
        publisher
            .publish(RegistrySnapshot::new(2, Vec::new(), limits).unwrap())
            .unwrap();
        assert_eq!(held.generation(), 1);
        assert_eq!(publisher.snapshot().unwrap().generation(), 2);
        assert_eq!(
            publisher.publish(RegistrySnapshot::new(2, Vec::new(), limits).unwrap()),
            Err(RegistryProblem::StaleGeneration)
        );
    }

    #[test]
    fn generated_identity_presence_does_not_install_a_handler() {
        let limits = InvocationLimits::default();
        assert_eq!(official_semantic_identities().len(), 1_506);
        let registry = SubsystemHandlerRegistry::new(1, Vec::new(), limits).unwrap();
        assert!(registry.is_empty());
        let identity =
            SemanticOperationId::new(official_semantic_identities()[0].id, limits).unwrap();
        assert_eq!(
            registry.select(&identity).err(),
            Some(HostProblem::Unsupported)
        );
    }

    #[test]
    fn subsystem_handlers_are_explicit_unique_and_namespace_checked() {
        let limits = InvocationLimits::default();
        assert_eq!(
            SubsystemHandlerRegistry::new(1, vec![handler("db2", true)], limits).err(),
            Some(RegistryProblem::WrongSubsystem)
        );
        assert_eq!(
            SubsystemHandlerRegistry::new(
                1,
                vec![handler("cics", true), handler("cics", true)],
                limits,
            )
            .err(),
            Some(RegistryProblem::DuplicateSemanticIdentity)
        );
        let registry =
            SubsystemHandlerRegistry::new(1, vec![handler("cics", true)], limits).unwrap();
        let identity =
            SemanticOperationId::new(official_semantic_identities()[0].id, limits).unwrap();
        assert!(registry.select(&identity).is_ok());
        let unavailable =
            SubsystemHandlerRegistry::new(2, vec![handler("cics", false)], limits).unwrap();
        assert_eq!(
            unavailable.select(&identity).err(),
            Some(HostProblem::ProviderFailure)
        );
    }

    #[test]
    fn subsystem_handler_publication_is_monotonic() {
        let limits = InvocationLimits::default();
        let publisher = SubsystemHandlerPublisher::new(
            SubsystemHandlerRegistry::new(1, Vec::new(), limits).unwrap(),
        );
        publisher
            .publish(SubsystemHandlerRegistry::new(2, Vec::new(), limits).unwrap())
            .unwrap();
        assert_eq!(publisher.snapshot().unwrap().generation(), 2);
        assert_eq!(
            publisher.publish(SubsystemHandlerRegistry::new(2, Vec::new(), limits).unwrap()),
            Err(RegistryProblem::StaleGeneration)
        );
    }
}
