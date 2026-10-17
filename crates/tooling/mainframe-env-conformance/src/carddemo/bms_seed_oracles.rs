//! Bounded first-party selected-field comparisons, not full-map certification.
use super::*;

const ACCOUNT_INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COACTVWC.cbl",
        "4f1e55176f69edfb55b7af7a161aceb8c3bc3bd1b2924a704a76256e0e751b54",
    ),
    (
        "app/bms/COACTVW.bms",
        "e229c1b4c9d3f52a5b8ebbb5266c9e47502ac90577b46f5f146bdb34b05f7f46",
    ),
    (
        "app/cpy-bms/COACTVW.CPY",
        "f3bf70a636aecb5874518ccc53f9826bcc69070b8307bdc551b2c57b9dd61dce",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CVCUS01Y.cpy",
        "944fd9a8eb10a683d0db97b9a87f2151982f649970287eb74358826056af25f8",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS",
        "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS",
        "0435915cd35ea3077bad9d391fea1520bc7f30ec06d6e29fbe68290f3d6f27f3",
    ),
    (
        "app/data/ASCII/acctdata.txt",
        "c2a97b6a32dc4a87a7aafdf7f72e6712e560412d30b00c5526cca80fc9dfd260",
    ),
];

const CARD_INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COCRDSLC.cbl",
        "d5af307fb4b1a155f03df9eea14b402d866a332360e14a1e37dbefe59b73363b",
    ),
    (
        "app/cbl/COCRDUPC.cbl",
        "c245cf383a4a3f721773a13ca231eb76bd5ae75c1830d58c7aba898a554a0de9",
    ),
    (
        "app/bms/COCRDSL.bms",
        "93feec4d134e991a81320d8c2f19e1aba19937753a1972d7d07f8a24973c9af6",
    ),
    (
        "app/cpy-bms/COCRDSL.CPY",
        "3e099c4a1857986dd7fa5253d4419aeee0a2f27c76f34d9081ac5a724f45bfd1",
    ),
    (
        "app/cpy/CVACT02Y.cpy",
        "9f1c62ef31b9d541712f1ed28f79e011ea7338611f5e0f30f2e1cbf624181223",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDDATA.PS",
        "b5d968b6865bd48fb45928ef6cacce9b6aacdaec7b51cfcc9819b9fafcaed3e5",
    ),
];

const ACCOUNT_FIELDS: &[(&str, &[u8])] = &[
    ("ACCTSID", b"00000000050"),
    ("ACSTTUS", b"Y"),
    ("ADTOPEN", b"2011-04-22"),
    ("AEXPDT", b"2023-03-09"),
    ("AREISDT", b"2023-03-09"),
    ("AADDGRP", b"          "),
    ("ACSTNUM", b"000000050"),
    ("ACSFNAM", b"Aniya                    "),
    ("ACSMNAM", b"Alba                     "),
    ("ACSLNAM", b"Von                      "),
    (
        "ACSADL1",
        b"1588 Nienow Cape                                  ",
    ),
    (
        "ACSADL2",
        b"Suite 187                                         ",
    ),
    (
        "ACSCITY",
        b"New Aricchester                                   ",
    ),
    ("ACSSTTE", b"OR"),
    ("ACSCTRY", b"USA"),
    ("ACSZIPC", b"04257"),
    ("ACSPHN1", b"(325)301-0827"),
    ("ACSPHN2", b"(493)985-9283"),
    ("ACSTSSN", b"931-24-8469\0"),
    ("ACSGOVT", b"00000000000030387824"),
    ("ACSTDOB", b"1960-12-01"),
    ("ACSEFTC", b"0074883577"),
    ("ACSPFLG", b"Y"),
    ("ACSTFCO", b"623"),
    ("ACURBAL", b"+        492.00"),
    ("ACRDLIM", b"+      6,169.00"),
    ("ACSHLIM", b"+      4,587.00"),
    ("ACRCYCR", b"+           .00"),
    ("ACRCYDB", b"+           .00"),
];

const CARD_FIELDS: &[(&str, &[u8])] = &[
    ("ACCTSID", b"00000000050"),
    ("CARDSID", b"0500024453765740"),
    (
        "CRDNAME",
        b"CERTIFIED USER                                    ",
    ),
    ("CRDSTCD", b"Y"),
    ("EXPMON", b"03"),
    ("EXPYEAR", b"2023"),
];

fn check_inputs(corpus: &Path, expected: &[(&str, &str)]) -> Result<(), CorpusProblem> {
    for (path, digest) in expected {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *digest {
            return Err(CorpusProblem::new(
                "carddemo.online.bms_oracle_source_drift",
                *path,
            ));
        }
    }
    Ok(())
}

fn selected_values_match(
    terminal: &serde_json::Value,
    fields: &BTreeMap<String, Vec<u8>>,
    mapset: &str,
    map: &str,
    expected: &[(&str, &[u8])],
) -> bool {
    terminal["mapset"] == mapset
        && terminal["map"] == map
        && expected
            .iter()
            .all(|(name, bytes)| fields.get(*name).map(Vec::as_slice) == Some(*bytes))
}

pub(super) fn compare_account_view(
    corpus: &Path,
    terminal: &serde_json::Value,
    fields: &BTreeMap<String, Vec<u8>>,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    check_inputs(corpus, ACCOUNT_INPUTS)?;
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J04",
        "BMS values",
        !selected_values_match(terminal, fields, "COACTVW", "CACTVWA", ACCOUNT_FIELDS),
        || {
            Ok(CorpusProblem::new(
                "carddemo.online.account_bms_values",
                "selected fixed account/customer values differ from the pinned first-party oracle",
            ))
        },
    )
}

pub(super) fn compare_card_detail(
    corpus: &Path,
    terminal: &serde_json::Value,
    fields: &BTreeMap<String, Vec<u8>>,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    check_inputs(corpus, CARD_INPUTS)?;
    observations.compare(
        journey_closure::AuthorityKind::Journey, "CD.J05", "CARDDAT detail",
        !selected_values_match(terminal, fields, "COCRDSL", "CCRDSLA", CARD_FIELDS),
        || Ok(CorpusProblem::new("carddemo.online.card_detail_values", "all six card detail entity values differ from pinned seed plus the explicit prior CERTIFIED USER update")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_selected_field_missing_corrupt_or_wrong_width_is_refused() {
        for (mapset, map, expected) in [
            ("COACTVW", "CACTVWA", ACCOUNT_FIELDS),
            ("COCRDSL", "CCRDSLA", CARD_FIELDS),
        ] {
            let terminal = serde_json::json!({"mapset":mapset,"map":map});
            let original = expected
                .iter()
                .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
                .collect::<BTreeMap<_, _>>();
            assert!(selected_values_match(
                &terminal, &original, mapset, map, expected
            ));
            for (name, _) in expected {
                let mut missing = original.clone();
                missing.remove(*name);
                assert!(!selected_values_match(
                    &terminal, &missing, mapset, map, expected
                ));
                let mut corrupt = original.clone();
                corrupt.get_mut(*name).unwrap()[0] ^= 1;
                assert!(!selected_values_match(
                    &terminal, &corrupt, mapset, map, expected
                ));
                let mut oversized = original.clone();
                oversized.get_mut(*name).unwrap().push(b' ');
                assert!(!selected_values_match(
                    &terminal, &oversized, mapset, map, expected
                ));
            }
        }
    }

    #[test]
    fn wrong_map_or_missing_map_identity_does_not_match_valid_values() {
        for (mapset, map, expected) in [
            ("COACTVW", "CACTVWA", ACCOUNT_FIELDS),
            ("COCRDSL", "CCRDSLA", CARD_FIELDS),
        ] {
            let fields = expected
                .iter()
                .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
                .collect::<BTreeMap<_, _>>();
            for terminal in [
                serde_json::json!({"mapset":"WRONG","map":map}),
                serde_json::json!({"mapset":mapset,"map":"WRONG"}),
                serde_json::json!({"mapset":mapset}),
            ] {
                assert!(!selected_values_match(
                    &terminal, &fields, mapset, map, expected
                ));
            }
        }
    }

    #[test]
    fn amount_sign_digit_comma_zero_and_nul_corruptions_refuse_without_credit() {
        let terminal = serde_json::json!({"mapset":"COACTVW","map":"CACTVWA"});
        let original = ACCOUNT_FIELDS
            .iter()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .collect::<BTreeMap<_, _>>();
        for (name, position, changed) in [
            ("ACURBAL", 0, b'-'),
            ("ACURBAL", 9, b'9'),
            ("ACRDLIM", 8, b'.'),
            ("ACRCYCR", 13, b'1'),
            ("ACRCYDB", 14, 0),
        ] {
            let mut actual = original.clone();
            assert_ne!(actual[name][position], changed);
            actual.get_mut(name).unwrap()[position] = changed;
            let mut observed = RouteObservations::default();
            let result = observed.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J04",
                "BMS values",
                !selected_values_match(&terminal, &actual, "COACTVW", "CACTVWA", ACCOUNT_FIELDS),
                || {
                    Ok(CorpusProblem::new(
                        "carddemo.online.account_bms_values",
                        "controlled amount corruption",
                    ))
                },
            );
            assert!(result.is_err());
            assert!(observed.journeys().is_empty());
        }
    }

    #[test]
    fn card_identifier_name_status_and_expiry_corruptions_refuse_without_credit() {
        let terminal = serde_json::json!({"mapset":"COCRDSL","map":"CCRDSLA"});
        let original = CARD_FIELDS
            .iter()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .collect::<BTreeMap<_, _>>();
        for (name, position, changed) in [
            ("ACCTSID", 10, b'9'),
            ("CARDSID", 15, b'9'),
            ("CRDNAME", 0, b'A'),
            ("CRDSTCD", 0, b'N'),
            ("EXPMON", 1, b'4'),
            ("EXPYEAR", 3, b'4'),
        ] {
            let mut actual = original.clone();
            assert_ne!(actual[name][position], changed);
            actual.get_mut(name).unwrap()[position] = changed;
            let mut observed = RouteObservations::default();
            let result = observed.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J05",
                "CARDDAT detail",
                !selected_values_match(&terminal, &actual, "COCRDSL", "CCRDSLA", CARD_FIELDS),
                || {
                    Ok(CorpusProblem::new(
                        "carddemo.online.card_detail_values",
                        "controlled entity corruption",
                    ))
                },
            );
            assert!(result.is_err());
            assert!(observed.journeys().is_empty());
        }
    }
}
