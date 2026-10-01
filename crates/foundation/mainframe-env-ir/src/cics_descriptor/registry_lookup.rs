//! Registry lookup for one advertised host runtime operation.

use super::{CICS_APPLICATION_REGISTRY, CicsApplicationRegistryDescriptor};

/// Resolves the unique advertised application row for a host runtime operation.
///
/// Internal-only operations and the separate SPI compatibility route have no
/// application row and therefore return `None`.
#[must_use]
pub fn cics_application_registry_for_runtime_operation(
    runtime_operation: &str,
) -> Option<&'static CicsApplicationRegistryDescriptor> {
    let mut matches = CICS_APPLICATION_REGISTRY.iter().filter(|descriptor| {
        descriptor.advertised && descriptor.runtime_operation == Some(runtime_operation)
    });
    let descriptor = matches.next()?;
    matches.next().is_none().then_some(descriptor)
}
