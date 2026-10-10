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
        ("CD.J03", "restart from suspended menu"),
        ("CD.J03", "USRSEC keyed read"),
        ("CD.J04", "ACCTDAT and cross-reference reads"),
        ("CD.J02", "COSGN00 screen fields"),
        ("CD.J07", "bill-payment mutation"),
        ("CD.J07", "ASKTIME and FORMATTIME"),
        ("CD.J06", "record insert"),
        ("CD.J02", "NOTFND and wrong-password messages"),
        ("CD.J07", "WRITEQ TD job request"),
        ("CD.J07", "reported messages"),
        ("CD.J01", "dataset record counts and digests"),
        ("CD.J01", "package identity"),
        ("CD.J01", "program artifacts"),
        ("CD.J01", "resource catalog"),
        ("CD.J01", "map catalog"),
        ("CD.J01", "RACF profiles"),
        ("CD.J04", "BMS values"),
        ("CD.J04", "keyed rewrite"),
        ("CD.J04", "SYNCPOINT commit"),
        ("CD.J05", "keyed rewrite"),
        ("CD.J05", "CARDDAT detail"),
        ("CD.J05", "atomic base/index state"),
        ("CD.J08", "USRSEC browse"),
        ("CD.J08", "add"),
        ("CD.J08", "update"),
        ("CD.J08", "delete"),
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
    #[cfg(feature = "assurance-observers")]
    println!(
        "actual compiled account-detail READ storage control: three whole50/300/500 INTO postimages, exact rawCOMP4 RESP/RESP2 and six typed EIB cells; independently frozen external input; original request-chain/source/actor proof and completed wrapper cleanup; zero new acceptance tokens and no IBM numeric/full result-storage credit"
    );
    println!(
        "actual pinned-corpus base-online observation identities: {:?}; selected source-layout account49/customer49 rewrite and explicit PF3 commit; retained final state equality across reopen; selected pinned source-to-publisher-to-installed-artifact binding repeated after reopen, installed-index coherence after an unchanged-account-key rewrite and stored session/response independence; zero global journey/issue acceptance counts and zero licensed credit",
        observations.journeys(),
    );
}
