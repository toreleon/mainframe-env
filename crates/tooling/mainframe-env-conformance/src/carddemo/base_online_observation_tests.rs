//! Actual pinned-corpus selected base-online observation owner.
use super::*;

#[test]
#[ignore = "requires the exact clean CARDDEMO_CORPUS_DIR first-party input"]
fn pinned_corpus_base_online_produces_selected_state_observations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    // The observed wrapper must complete its original route predicates, both
    // server drains and owned artifact cleanup before these tokens can escape.
    let (receipt, observations) =
        online_receipt::verify_carddemo_base_online_observed(&inventory).unwrap();
    assert_eq!(receipt.status, "pass");
    assert_eq!(
        receipt.corpus_commit,
        "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
    );
    for (id, requirement) in [
        ("CD.J04", "BMS values"),
        ("CD.J05", "keyed rewrite"),
        ("CD.J05", "CARDDAT detail"),
        ("CD.J05", "atomic base/index state"),
        ("CD.J08", "duplicate/not-found conditions"),
        ("CD.J08", "regular-user denial"),
        ("CD.J09", "concurrent session isolation"),
    ] {
        assert!(
            observations
                .journeys()
                .iter()
                .any(|(actual_id, requirements)| {
                    actual_id == id && requirements.iter().any(|actual| actual == requirement)
                }),
            "missing exercised observation {id}/{requirement}"
        );
    }
    println!(
        "actual pinned-corpus base-online observation identities: {:?}; selected installed-index coherence after an unchanged-account-key rewrite and stored session/response independence; zero global journey/issue acceptance counts and zero licensed credit",
        observations.journeys(),
    );
}
