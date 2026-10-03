use crate::{EffectRequest, EffectResult, HostProblem, SemanticNamespace, SemanticOperationId};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// Stable installed semantic-handler registry contract identity.
pub const SUBSYSTEM_HANDLER_REGISTRY_CONTRACT: &str = "mainframe-env.subsystem-handler-registry@1";

#[derive(Clone, Debug, Eq, PartialEq)]
/// Provider capability, schema and byte limits; readiness is declared separately from resource authorization.
pub struct CapabilityDescriptor {
    /// Exact unique capability key used for registry selection.
    pub capability: CapabilityId,
    /// Bounded nonempty provider identity, not a principal.
    pub provider_id: String,
    /// Bounded nonempty provider generation identity, distinct from registry generation.
    pub generation: String,
    /// Exact declared request schema identity, not a decoder or migration procedure.
    pub request_schema: String,
    /// Exact declared result schema identity, not proof that a reply was validated.
    pub result_schema: String,
    /// Positive maximum encoded request bytes declared by this provider.
    pub max_request_bytes: usize,
    /// Positive maximum encoded result bytes declared by this provider.
    pub max_result_bytes: usize,
    /// Explicit readiness declaration; selecting an unready implementation fails closed.
    pub ready: bool,
}

impl CapabilityDescriptor {
    /// Check bounded nonempty identities and positive byte limits; readiness and permission are not inferred.
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

/// Installed provider boundary. Implementations own execution; callers retain the original invocation/effect identity.
pub trait HostProvider: Send + Sync {
    /// Borrow the installed declaration used for selection; keep it consistent with the implementation lifetime.
    fn descriptor(&self) -> &CapabilityDescriptor;
    /// Handle the original request under the supplied invocation, retaining its sequence and exact failure/uncertainty result; this interface grants no permission by itself.
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult;

    /// Replay-only synchronous transport for the finite checked inquiry shape.
    /// Expectations/context are structural, not store/frame/SAF permission.
    /// The receiver must return the original retained result after current live
    /// checks; default refusal NEVER calls ordinary invoke or recomputes output.
    /// A future closed receiver must validate the concrete privately minted
    /// capture, same physical store and original frame independently.
    fn replay_retained(
        &self,
        _invocation: &Invocation,
        request: EffectRequest,
        _expected_result_digest: [u8; 32],
        _observed_tick: u64,
        _context: &(dyn std::any::Any + Send + Sync),
    ) -> EffectResult {
        EffectResult {
            sequence: request.sequence,
            outcome: Err(HostProblem::Unsupported),
        }
    }

    /// Transport a borrowed Rust-only context for an original Program request.
    /// Type erasure conveys no admission, JES, lifecycle, SAF or core authority;
    /// an implementing receiver must independently validate its closed owner type.
    /// The default refuses without calling `invoke`; old providers gain no route.
    fn invoke_program_context(
        &self,
        _invocation: &Invocation,
        request: EffectRequest,
        _context: &(dyn std::any::Any + Send + Sync),
    ) -> EffectResult {
        EffectResult {
            sequence: request.sequence,
            outcome: Err(HostProblem::Unsupported),
        }
    }
}

#[derive(Clone)]
/// Immutable generation of unique capability-to-provider bindings; held snapshots survive later publication.
pub struct RegistrySnapshot {
    generation: u64,
    providers: BTreeMap<CapabilityId, Arc<dyn HostProvider>>,
}

impl RegistrySnapshot {
    /// Build a positive bounded generation, validating descriptors and rejecting duplicate keys before publishing anything.
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
    /// Return this immutable registry publication generation, not an individual provider schema/revision.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Clone the exact installed binding; absent keys return Unsupported and unready declarations return ProviderFailure. No fallback or invocation occurs.
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
    /// Iterate exact capability keys in deterministic sorted order.
    pub fn capabilities(&self) -> impl ExactSizeIterator<Item = &CapabilityId> {
        self.providers.keys()
    }
}

/// Lock-protected publication of strictly increasing capability generations; existing snapshots remain valid observations.
pub struct RegistryPublisher {
    current: RwLock<Arc<RegistrySnapshot>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Semantic handler declaration checked against official subsystem identity when in the official namespace.
pub struct SubsystemHandlerDescriptor {
    /// Exact official/custom identity key selected by the registry.
    pub semantic_id: SemanticOperationId,
    /// Bounded subsystem label; official identity descriptors must agree exactly.
    pub subsystem: String,
    /// Bounded nonempty handler generation identity.
    pub generation: String,
    /// Exact declared request schema identity, not a decoder or migration procedure.
    pub request_schema: String,
    /// Exact declared result schema identity, not proof that a reply was validated.
    pub result_schema: String,
    /// Explicit readiness declaration; selecting an unready implementation fails closed.
    pub ready: bool,
}

impl SubsystemHandlerDescriptor {
    /// Check bounded nonempty descriptor identities; semantic official descriptors also require exact subsystem agreement. No handler is invoked.
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

/// Installed semantic handler; catalog identity alone does not install an implementation or grant permission.
pub trait SubsystemHandler: Send + Sync {
    /// Borrow the installed declaration used for selection; keep it consistent with the implementation lifetime.
    fn descriptor(&self) -> &SubsystemHandlerDescriptor;
    /// Handle the original request under the supplied invocation, retaining its sequence and exact failure/uncertainty result; this interface grants no permission by itself.
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult;
}

#[derive(Clone)]
/// Immutable unique semantic-handler generation, bounded by invocation capability count.
pub struct SubsystemHandlerRegistry {
    generation: u64,
    handlers: BTreeMap<SemanticOperationId, Arc<dyn SubsystemHandler>>,
}

impl SubsystemHandlerRegistry {
    /// Build a positive bounded generation, validating descriptors and rejecting duplicate keys before publishing anything.
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
    /// Return this immutable registry publication generation, not an individual provider schema/revision.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    /// Return the number of installed bindings, not the generated identity denominator.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    #[must_use]
    /// Report whether no handler binding is installed.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Clone the exact installed binding; absent keys return Unsupported and unready declarations return ProviderFailure. No fallback or invocation occurs.
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

    /// Iterate exact semantic keys in deterministic sorted order.
    pub fn identities(&self) -> impl ExactSizeIterator<Item = &SemanticOperationId> {
        self.handlers.keys()
    }
}

/// Monotonic semantic-handler publication with immutable retained snapshots and poison reporting.
pub struct SubsystemHandlerPublisher {
    current: RwLock<Arc<SubsystemHandlerRegistry>>,
}

impl SubsystemHandlerPublisher {
    #[must_use]
    /// Publish an already validated initial generation; later snapshots share it through Arc.
    pub fn new(initial: SubsystemHandlerRegistry) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    /// Clone the current immutable Arc under a read lock; poisoned state returns Poisoned instead of recovering silently.
    pub fn snapshot(&self) -> Result<Arc<SubsystemHandlerRegistry>, RegistryProblem> {
        self.current
            .read()
            .map(|registry| Arc::clone(&registry))
            .map_err(|_| RegistryProblem::Poisoned)
    }

    /// Replace the current Arc only with a strictly newer generation under one write lock; stale/poisoned failures preserve the current publication.
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
    /// Publish an already validated initial generation; later snapshots share it through Arc.
    pub fn new(initial: RegistrySnapshot) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    /// Clone the current immutable Arc under a read lock; poisoned state returns Poisoned instead of recovering silently.
    pub fn snapshot(&self) -> Result<Arc<RegistrySnapshot>, RegistryProblem> {
        self.current
            .read()
            .map(|snapshot| Arc::clone(&snapshot))
            .map_err(|_| RegistryProblem::Poisoned)
    }

    /// Replace the current Arc only with a strictly newer generation under one write lock; stale/poisoned failures preserve the current publication.
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
/// Registry construction/publication failure; rejected generations leave the current snapshot unchanged.
pub enum RegistryProblem {
    /// Descriptor identities or positive byte bounds are invalid.
    InvalidDescriptor,
    /// A capability key occurs more than once.
    DuplicateCapability,
    /// A semantic key occurs more than once.
    DuplicateSemanticIdentity,
    /// An official semantic identity disagrees with the declared subsystem.
    WrongSubsystem,
    /// Generation is zero or binding count exceeds its ceiling.
    LimitExceeded,
    /// Publication is not strictly newer than the current generation.
    StaleGeneration,
    /// The registry lock is poisoned; no silent recovery occurs.
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
