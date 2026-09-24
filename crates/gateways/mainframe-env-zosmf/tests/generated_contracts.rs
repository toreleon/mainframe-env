use mainframe_env_zosmf::{
    ZOSMF_FAMILY_BACKENDS, ZOSMF_LEGACY_ROUTE_OPERATIONS, ZOSMF_NEW_ADVERTISED_ROUTE_COUNT,
    ZOSMF_NORMALIZATION_CONTRACT, ZOSMF_NORMALIZATION_SHA256, ZOSMF_NORMALIZED_FAMILY_COUNT,
    ZOSMF_NORMALIZED_HEADING_COUNT, ZOSMF_NORMALIZED_OPERATION_COUNT,
    ZOSMF_NORMALIZED_ROUTE_VARIANT_COUNT, custom_route_ids, official_route_ids,
};
use std::collections::BTreeSet;

#[test]
fn generated_normalization_metadata_preserves_the_public_route_boundary() {
    assert_eq!(
        ZOSMF_NORMALIZATION_CONTRACT,
        "mainframe-env.zosmf-normalization@1"
    );
    assert!(ZOSMF_NORMALIZATION_SHA256.starts_with("sha256:"));
    assert_eq!(ZOSMF_NORMALIZED_FAMILY_COUNT, 27);
    assert_eq!(ZOSMF_NORMALIZED_HEADING_COUNT, 189);
    assert_eq!(ZOSMF_NORMALIZED_OPERATION_COUNT, 278);
    assert_eq!(ZOSMF_NORMALIZED_ROUTE_VARIANT_COUNT, 352);
    assert_eq!(ZOSMF_NEW_ADVERTISED_ROUTE_COUNT, 0);
    assert_eq!(ZOSMF_FAMILY_BACKENDS.len(), 27);
    assert_eq!(ZOSMF_LEGACY_ROUTE_OPERATIONS.len(), 23);
    assert_eq!(official_route_ids().len(), 23);
    assert_eq!(custom_route_ids().len(), 7);

    let generated = ZOSMF_LEGACY_ROUTE_OPERATIONS
        .iter()
        .map(|(route, _)| *route)
        .collect::<Vec<_>>();
    assert_eq!(generated, official_route_ids());
    assert_eq!(
        generated
            .iter()
            .filter(|route| route.ends_with("/{dsn}/search"))
            .count(),
        1
    );
    assert!(
        ZOSMF_LEGACY_ROUTE_OPERATIONS
            .iter()
            .find(|(route, _)| route.ends_with("/{dsn}/search"))
            .is_some_and(|(_, operations)| operations.is_empty())
    );

    let official = official_route_ids()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let custom = custom_route_ids().iter().copied().collect::<BTreeSet<_>>();
    assert!(official.is_disjoint(&custom));
    assert!(official.iter().all(|route| route.contains(" /zosmf/")));
    assert!(
        custom
            .iter()
            .all(|route| route.contains(" /mainframe-env/"))
    );
}
