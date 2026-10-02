#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Static dataset surface metadata generated from the contract inventory.
/// Authority and implementation labels describe the inventory; presence does not prove
/// provider availability or licensed execution coverage.
pub struct DatasetSurfaceDescriptor {
    /// Surface family grouping this descriptor.
    pub family: &'static str,
    /// Stable identity within the surface family.
    pub id: &'static str,
    /// Human-readable operation or surface label.
    pub label: &'static str,
    /// Declared authority classification from the surface inventory.
    pub authority: &'static str,
    /// Declared implementation classification from the surface inventory.
    pub implementation: &'static str,
    /// Effect classification carried by the inventory.
    pub effect: &'static str,
    /// Static operand names associated with this surface entry.
    pub operands: &'static [&'static str],
    /// Official catalog row identities associated with this entry.
    pub official_rows: &'static [&'static str],
}

include!("generated/dataset_programming_surface.rs");

#[must_use]
/// Borrow the generated surface table sorted by family and identity.
pub fn dataset_surface_descriptors() -> &'static [DatasetSurfaceDescriptor] {
    DATASET_SURFACE_DESCRIPTORS
}

#[must_use]
/// Look up an exact family/identity pair, or return `None` when absent.
pub fn dataset_surface_descriptor(
    family: &str,
    id: &str,
) -> Option<&'static DatasetSurfaceDescriptor> {
    DATASET_SURFACE_DESCRIPTORS
        .binary_search_by(|descriptor| (descriptor.family, descriptor.id).cmp(&(family, id)))
        .ok()
        .map(|index| &DATASET_SURFACE_DESCRIPTORS[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn generated_surface_is_sorted_unique_and_complete() {
        let descriptors = dataset_surface_descriptors();
        assert_eq!(descriptors.len(), 131);
        assert!(
            descriptors
                .windows(2)
                .all(|pair| { (pair[0].family, pair[0].id) < (pair[1].family, pair[1].id) })
        );
        assert_eq!(
            descriptors
                .iter()
                .map(|descriptor| descriptor.family)
                .collect::<BTreeSet<_>>()
                .len(),
            10
        );
        assert_eq!(
            descriptors
                .iter()
                .filter(|descriptor| descriptor.family == "ams-commands")
                .count(),
            31
        );
        for descriptor in descriptors {
            assert_eq!(
                dataset_surface_descriptor(descriptor.family, descriptor.id),
                Some(descriptor)
            );
        }
    }
}
