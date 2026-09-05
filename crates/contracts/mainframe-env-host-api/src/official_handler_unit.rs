//! Catalog-scoped closure checks over the existing subsystem-handler registry.
//!
//! These checks establish registration and readiness, not command semantics or
//! conformance credit. They neither install nor invoke a handler. In particular,
//! the CICS application API is a different catalog unit from SPI and FEPI.

use crate::{
    RegistryProblem, SemanticIdentityDescriptor, SubsystemHandlerPublisher,
    SubsystemHandlerRegistry, official_semantic_identities,
};

/// A nonempty unit selected from the generated official catalog.
///
/// The catalog remains the only inventory authority. Callers cannot supply a
/// smaller expected count or replace official identities with custom handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfficialHandlerUnit {
    baseline: &'static str,
    subsystem: &'static str,
    unit: &'static str,
}

impl OfficialHandlerUnit {
    /// Select an exact baseline/subsystem/unit tuple. Unknown and empty units
    /// fail closed, rather than passing vacuously with an empty denominator.
    pub fn new(
        baseline: &str,
        subsystem: &str,
        unit: &str,
    ) -> Result<Self, OfficialHandlerClosureProblem> {
        let descriptor = official_semantic_identities()
            .iter()
            .find(|descriptor| {
                descriptor.baseline == baseline
                    && descriptor.subsystem == subsystem
                    && descriptor.unit == unit
            })
            .ok_or(OfficialHandlerClosureProblem::UnknownUnit)?;
        Ok(Self {
            baseline: descriptor.baseline,
            subsystem: descriptor.subsystem,
            unit: descriptor.unit,
        })
    }

    #[must_use]
    pub const fn baseline(self) -> &'static str {
        self.baseline
    }

    #[must_use]
    pub const fn subsystem(self) -> &'static str {
        self.subsystem
    }

    #[must_use]
    pub const fn unit(self) -> &'static str {
        self.unit
    }

    /// Borrow the generated descriptors without creating a second inventory.
    pub fn descriptors(self) -> impl Iterator<Item = &'static SemanticIdentityDescriptor> {
        official_semantic_identities()
            .iter()
            .filter(move |descriptor| {
                descriptor.baseline == self.baseline
                    && descriptor.subsystem == self.subsystem
                    && descriptor.unit == self.unit
            })
    }

    /// The catalog denominator, never a semantic completion numerator.
    #[must_use]
    pub fn identity_count(self) -> usize {
        self.descriptors().count()
    }

    /// Require an explicit, selectable handler for every identity in the unit.
    ///
    /// Both iterators are ordered by semantic identity. A merge scan permits
    /// unrelated units to coexist in the shared registry without counting them
    /// toward this unit, and avoids a quadratic scan or an allocated inventory.
    /// Failure reports the first missing or unready official identity in the
    /// generated catalog order; no unbounded caller input enters diagnostics.
    pub fn validate(
        self,
        registry: &SubsystemHandlerRegistry,
    ) -> Result<(), OfficialHandlerClosureProblem> {
        let mut registered = registry.identities().peekable();
        for expected in self.descriptors() {
            while registered
                .peek()
                .is_some_and(|identity| identity.as_str() < expected.id)
            {
                registered.next();
            }
            let identity = registered
                .next_if(|identity| identity.as_str() == expected.id)
                .ok_or(OfficialHandlerClosureProblem::MissingHandler {
                    identity: expected.id,
                })?;
            if registry.select(identity).is_err() {
                return Err(OfficialHandlerClosureProblem::UnreadyHandler {
                    identity: expected.id,
                });
            }
        }
        Ok(())
    }

    /// Validate before delegating publication to the existing monotonic owner.
    ///
    /// A failed closure check cannot replace the current snapshot. This is an
    /// opt-in admission check; it does not change compatibility-profile routing
    /// or turn an existing partial registry into a complete application runtime.
    pub fn publish(
        self,
        publisher: &SubsystemHandlerPublisher,
        next: SubsystemHandlerRegistry,
    ) -> Result<(), OfficialHandlerClosureProblem> {
        self.validate(&next)?;
        publisher
            .publish(next)
            .map_err(OfficialHandlerClosureProblem::Registry)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfficialHandlerClosureProblem {
    UnknownUnit,
    MissingHandler { identity: &'static str },
    UnreadyHandler { identity: &'static str },
    Registry(RegistryProblem),
}

impl std::fmt::Display for OfficialHandlerClosureProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "official handler closure failed: {self:?}")
    }
}

impl std::error::Error for OfficialHandlerClosureProblem {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Registry(problem) => Some(problem),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EffectRequest, EffectResult, HostProblem, SemanticOperationId, SubsystemHandler,
        SubsystemHandlerDescriptor,
    };
    use mainframe_env_execution_api::{Invocation, InvocationLimits};
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const BASELINE: &str = "ibm-cics-ts-6x-2026-08-31";

    fn application_unit() -> OfficialHandlerUnit {
        OfficialHandlerUnit::new(BASELINE, "cics", "api-commands").unwrap()
    }

    fn limits() -> InvocationLimits {
        InvocationLimits {
            max_capabilities: official_semantic_identities().len() + 1,
            ..InvocationLimits::default()
        }
    }

    // Internal registry fixtures only. They are never installed in a product
    // route and contribute no recognized/executed/differential verdicts.
    struct Handler {
        descriptor: SubsystemHandlerDescriptor,
        invocations: Arc<AtomicUsize>,
    }

    impl SubsystemHandler for Handler {
        fn descriptor(&self) -> &SubsystemHandlerDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            EffectResult {
                sequence: request.sequence,
                outcome: Err(HostProblem::Unsupported),
            }
        }
    }

    fn handler(
        id: &str,
        subsystem: &str,
        ready: bool,
        invocations: &Arc<AtomicUsize>,
    ) -> Arc<dyn SubsystemHandler> {
        Arc::new(Handler {
            descriptor: SubsystemHandlerDescriptor {
                semantic_id: SemanticOperationId::new(id, limits()).unwrap(),
                subsystem: subsystem.into(),
                generation: "closure-test@1".into(),
                request_schema: "mainframe-env.effect-request@1".into(),
                result_schema: "mainframe-env.effect-result@1".into(),
                ready,
            },
            invocations: Arc::clone(invocations),
        })
    }

    fn application_handlers(
        omitted: Option<&str>,
        unready: Option<&str>,
        invocations: &Arc<AtomicUsize>,
    ) -> Vec<Arc<dyn SubsystemHandler>> {
        application_unit()
            .descriptors()
            .filter(|descriptor| Some(descriptor.id) != omitted)
            .map(|descriptor| {
                handler(
                    descriptor.id,
                    descriptor.subsystem,
                    Some(descriptor.id) != unready,
                    invocations,
                )
            })
            .collect()
    }

    fn registry(
        generation: u64,
        omitted: Option<&str>,
        unready: Option<&str>,
    ) -> SubsystemHandlerRegistry {
        SubsystemHandlerRegistry::new(
            generation,
            application_handlers(omitted, unready, &Arc::new(AtomicUsize::new(0))),
            limits(),
        )
        .unwrap()
    }

    #[test]
    fn application_denominator_is_exact_and_excludes_other_units() {
        let unit = application_unit();
        assert_eq!(unit.baseline(), BASELINE);
        assert_eq!(unit.subsystem(), "cics");
        assert_eq!(unit.unit(), "api-commands");
        assert_eq!(unit.identity_count(), 263);
        let identities: BTreeSet<_> = unit.descriptors().map(|entry| entry.id).collect();
        assert_eq!(identities.len(), 263);
        assert!(official_semantic_identities().iter().any(|descriptor| {
            descriptor.subsystem == "cics" && descriptor.unit != "api-commands"
        }));
        for descriptor in official_semantic_identities() {
            if descriptor.subsystem != "cics" || descriptor.unit != "api-commands" {
                assert!(!identities.contains(descriptor.id));
            }
        }
    }

    #[test]
    fn unknown_or_empty_scope_never_passes_vacuously() {
        for (baseline, subsystem, unit) in [
            ("", "cics", "api-commands"),
            ("ibm-cics-unpinned", "cics", "api-commands"),
            (BASELINE, "", "api-commands"),
            (BASELINE, "db2", "api-commands"),
            (BASELINE, "cics", ""),
            (BASELINE, "cics", "api-command"),
            (BASELINE, "cics", "api-commands*"),
        ] {
            assert_eq!(
                OfficialHandlerUnit::new(baseline, subsystem, unit),
                Err(OfficialHandlerClosureProblem::UnknownUnit)
            );
        }
    }

    #[test]
    fn generated_presence_does_not_satisfy_empty_registry() {
        let registry = SubsystemHandlerRegistry::new(1, Vec::new(), limits()).unwrap();
        assert_eq!(
            application_unit().validate(&registry),
            Err(OfficialHandlerClosureProblem::MissingHandler {
                identity: application_unit().descriptors().next().unwrap().id,
            })
        );
    }

    #[test]
    fn complete_registration_is_not_execution() {
        let calls = Arc::new(AtomicUsize::new(0));
        let registry =
            SubsystemHandlerRegistry::new(1, application_handlers(None, None, &calls), limits())
                .unwrap();
        assert_eq!(application_unit().validate(&registry), Ok(()));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn every_single_omission_fails_with_exact_identity() {
        for descriptor in application_unit().descriptors() {
            assert_eq!(
                application_unit().validate(&registry(1, Some(descriptor.id), None)),
                Err(OfficialHandlerClosureProblem::MissingHandler {
                    identity: descriptor.id,
                })
            );
        }
    }

    #[test]
    fn every_single_unready_handler_fails_with_exact_identity() {
        for descriptor in application_unit().descriptors() {
            assert_eq!(
                application_unit().validate(&registry(1, None, Some(descriptor.id))),
                Err(OfficialHandlerClosureProblem::UnreadyHandler {
                    identity: descriptor.id,
                })
            );
        }
    }

    #[test]
    fn non_application_and_custom_handlers_cannot_fill_an_omission() {
        let first = application_unit().descriptors().next().unwrap().id;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handlers = application_handlers(Some(first), None, &calls);
        for descriptor in official_semantic_identities() {
            if descriptor.subsystem != "cics" || descriptor.unit != "api-commands" {
                handlers.push(handler(descriptor.id, descriptor.subsystem, true, &calls));
            }
        }
        handlers.push(handler("custom:cics.application@1", "cics", true, &calls));
        let registry = SubsystemHandlerRegistry::new(1, handlers, limits()).unwrap();
        assert!(registry.len() > 263);
        assert_eq!(
            application_unit().validate(&registry),
            Err(OfficialHandlerClosureProblem::MissingHandler { identity: first })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unrelated_unready_handlers_do_not_broaden_application_scope() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handlers = application_handlers(None, None, &calls);
        for descriptor in official_semantic_identities() {
            if descriptor.subsystem != "cics" || descriptor.unit != "api-commands" {
                handlers.push(handler(descriptor.id, descriptor.subsystem, false, &calls));
            }
        }
        handlers.push(handler("custom:cics.application@1", "cics", false, &calls));
        let registry = SubsystemHandlerRegistry::new(1, handlers, limits()).unwrap();
        assert_eq!(application_unit().validate(&registry), Ok(()));
        assert_eq!(application_unit().identity_count(), 263);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn input_order_does_not_change_deterministic_closure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handlers = application_handlers(None, None, &calls);
        handlers.reverse();
        let registry = SubsystemHandlerRegistry::new(1, handlers, limits()).unwrap();
        assert_eq!(application_unit().validate(&registry), Ok(()));
    }

    #[test]
    fn duplicate_registration_still_fails_in_shared_registry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handlers = application_handlers(None, None, &calls);
        handlers.push(Arc::clone(&handlers[0]));
        assert_eq!(
            SubsystemHandlerRegistry::new(1, handlers, limits()).err(),
            Some(RegistryProblem::DuplicateSemanticIdentity)
        );
    }

    #[test]
    fn missing_handler_cannot_replace_published_snapshot() {
        let unit = application_unit();
        let first = unit.descriptors().next().unwrap().id;
        let publisher = SubsystemHandlerPublisher::new(registry(1, None, None));
        let held = publisher.snapshot().unwrap();
        assert_eq!(
            unit.publish(&publisher, registry(2, Some(first), None)),
            Err(OfficialHandlerClosureProblem::MissingHandler { identity: first })
        );
        assert!(Arc::ptr_eq(&held, &publisher.snapshot().unwrap()));
        assert_eq!(unit.validate(&held), Ok(()));
    }

    #[test]
    fn unready_handler_cannot_replace_published_snapshot() {
        let unit = application_unit();
        let first = unit.descriptors().next().unwrap().id;
        let publisher = SubsystemHandlerPublisher::new(registry(1, None, None));
        let held = publisher.snapshot().unwrap();
        assert_eq!(
            unit.publish(&publisher, registry(2, None, Some(first))),
            Err(OfficialHandlerClosureProblem::UnreadyHandler { identity: first })
        );
        assert!(Arc::ptr_eq(&held, &publisher.snapshot().unwrap()));
    }

    #[test]
    fn valid_closure_cannot_bypass_monotonic_publication() {
        let unit = application_unit();
        let publisher = SubsystemHandlerPublisher::new(registry(2, None, None));
        let held = publisher.snapshot().unwrap();
        for generation in [1, 2] {
            assert_eq!(
                unit.publish(&publisher, registry(generation, None, None)),
                Err(OfficialHandlerClosureProblem::Registry(
                    RegistryProblem::StaleGeneration
                ))
            );
            assert!(Arc::ptr_eq(&held, &publisher.snapshot().unwrap()));
        }
    }

    #[test]
    fn complete_replacement_preserves_held_snapshot() {
        let unit = application_unit();
        let publisher = SubsystemHandlerPublisher::new(registry(1, None, None));
        let held = publisher.snapshot().unwrap();
        assert_eq!(unit.publish(&publisher, registry(2, None, None)), Ok(()));
        assert_eq!(held.generation(), 1);
        let current = publisher.snapshot().unwrap();
        assert_eq!(current.generation(), 2);
        assert!(!Arc::ptr_eq(&held, &current));
        assert_eq!(unit.validate(&held), Ok(()));
        assert_eq!(unit.validate(&current), Ok(()));
    }

    #[test]
    fn same_checker_extends_to_other_generated_units() {
        let mut units = BTreeSet::new();
        for descriptor in official_semantic_identities() {
            units.insert((descriptor.baseline, descriptor.subsystem, descriptor.unit));
        }
        let mut total = 0;
        for (baseline, subsystem, unit) in units {
            let scope = OfficialHandlerUnit::new(baseline, subsystem, unit).unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let handlers = scope
                .descriptors()
                .map(|descriptor| handler(descriptor.id, descriptor.subsystem, true, &calls))
                .collect();
            let registry = SubsystemHandlerRegistry::new(1, handlers, limits()).unwrap();
            assert_eq!(scope.validate(&registry), Ok(()));
            total += scope.identity_count();
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
        assert_eq!(total, official_semantic_identities().len());
    }
}
