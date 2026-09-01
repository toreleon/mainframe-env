#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatasetSurfaceDescriptor {
    pub family: &'static str,
    pub id: &'static str,
    pub label: &'static str,
    pub authority: &'static str,
    pub implementation: &'static str,
    pub effect: &'static str,
    pub operands: &'static [&'static str],
    pub official_rows: &'static [&'static str],
}

include!("generated/dataset_programming_surface.rs");

#[must_use]
pub fn dataset_surface_descriptors() -> &'static [DatasetSurfaceDescriptor] {
    DATASET_SURFACE_DESCRIPTORS
}

#[must_use]
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
        assert_eq!(descriptors.len(), 130);
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
