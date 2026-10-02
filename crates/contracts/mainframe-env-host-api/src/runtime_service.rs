use crate::{HostProblem, RuntimeServiceKind, RuntimeServiceName};
use std::collections::BTreeMap;

/// Versioned identity of the shared runtime-service registry contract.
pub const RUNTIME_SERVICE_REGISTRY_CONTRACT: &str = "mainframe-env.runtime-service-registry@1";

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned service identity and schema labels for one positive ABI version.
pub struct RuntimeServiceDescriptor {
    /// Service family used as part of the exact registry key.
    pub kind: RuntimeServiceKind,
    /// Validated service name used as part of the exact registry key.
    pub name: RuntimeServiceName,
    /// Positive ABI version; registry construction rejects zero.
    pub abi_version: u16,
    /// Nonempty owned request-schema label, bounded by UTF-8 byte length at construction.
    pub request_schema: String,
    /// Nonempty owned response-schema label, bounded by UTF-8 byte length at construction.
    pub response_schema: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Immutable descriptors keyed by service family, name and exact ABI version.
pub struct RuntimeServiceRegistry {
    entries: BTreeMap<(RuntimeServiceKind, RuntimeServiceName, u16), RuntimeServiceDescriptor>,
    max_entries: usize,
}

impl RuntimeServiceRegistry {
    /// Build a registry within positive entry-count and per-schema byte ceilings.
    /// Zero ceilings or duplicate keys are malformed; invalid descriptors or capacity
    /// overflow return resource exhaustion without publishing a partial registry.
    pub fn new(
        descriptors: impl IntoIterator<Item = RuntimeServiceDescriptor>,
        max_entries: usize,
        max_schema_bytes: usize,
    ) -> Result<Self, HostProblem> {
        if max_entries == 0 || max_schema_bytes == 0 {
            return Err(HostProblem::Malformed);
        }
        let mut entries = BTreeMap::new();
        for descriptor in descriptors {
            if entries.len() >= max_entries
                || descriptor.abi_version == 0
                || descriptor.request_schema.is_empty()
                || descriptor.response_schema.is_empty()
                || descriptor.request_schema.len() > max_schema_bytes
                || descriptor.response_schema.len() > max_schema_bytes
            {
                return Err(HostProblem::ResourceExhausted);
            }
            let key = (
                descriptor.kind,
                descriptor.name.clone(),
                descriptor.abi_version,
            );
            if entries.insert(key, descriptor).is_some() {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(Self {
            entries,
            max_entries,
        })
    }

    #[must_use]
    /// Borrow an exact descriptor, or return absence without version or family fallback.
    pub fn resolve(
        &self,
        kind: RuntimeServiceKind,
        name: &RuntimeServiceName,
        abi_version: u16,
    ) -> Option<&RuntimeServiceDescriptor> {
        self.entries.get(&(kind, name.clone(), abi_version))
    }

    #[must_use]
    /// Configured descriptor-count ceiling, independent of the current entry count.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_and_extensions_share_one_versioned_typed_registry() {
        let le = RuntimeServiceDescriptor {
            kind: RuntimeServiceKind::LanguageEnvironment,
            name: RuntimeServiceName::new("CEE-DATE", 128).unwrap(),
            abi_version: 1,
            request_schema: "mainframe-env.le.date.request@1".into(),
            response_schema: "mainframe-env.le.date.response@1".into(),
        };
        let extension = RuntimeServiceDescriptor {
            kind: RuntimeServiceKind::HostExtension,
            name: RuntimeServiceName::new("SITE-AUDIT", 128).unwrap(),
            abi_version: 2,
            request_schema: "example.site-audit.request@2".into(),
            response_schema: "example.site-audit.response@2".into(),
        };
        let registry =
            RuntimeServiceRegistry::new([le.clone(), extension.clone()], 8, 128).unwrap();
        assert_eq!(
            registry.resolve(le.kind, &le.name, le.abi_version),
            Some(&le)
        );
        assert_eq!(
            registry.resolve(extension.kind, &extension.name, extension.abi_version),
            Some(&extension)
        );
    }
}
