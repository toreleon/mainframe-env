//! Synthetic comparator controls; zero runtime/application or acceptance credit.
use super::*;

fn catalog(oracle: &Oracle) -> InstalledCatalog {
    InstalledCatalog {
        maps: expected_records("cics-map", &oracle.maps).unwrap(),
        files: expected_records("cics-file-alias", &oracle.files).unwrap(),
        profiles: expected_profiles(oracle).unwrap(),
    }
}

fn proof() -> CatalogProof {
    let oracle = oracle().unwrap();
    let initial = catalog(&oracle);
    CatalogProof {
        decisions: expected_decisions(&oracle),
        reopened: Some(initial.clone()),
        initial,
        oracle,
    }
}

fn copy_proof(valid: &CatalogProof) -> CatalogProof {
    CatalogProof {
        oracle: valid.oracle.clone(),
        initial: valid.initial.clone(),
        decisions: valid.decisions.clone(),
        reopened: valid.reopened.clone(),
    }
}

#[test]
fn every_frozen_map_or_file_byte_and_metadata_change_is_refused() {
    let valid = proof();
    valid.require_complete().unwrap();
    for maps in [true, false] {
        let expected = if maps {
            &valid.initial.maps
        } else {
            &valid.initial.files
        };
        for (key, record) in expected {
            for offset in 0..record.payload.len() {
                let mut changed = copy_proof(&valid);
                let rows = if maps {
                    &mut changed.initial.maps
                } else {
                    &mut changed.initial.files
                };
                rows.get_mut(key).unwrap().payload[offset] ^= 1;
                changed.reopened = Some(changed.initial.clone());
                assert!(
                    changed.require_complete().is_err(),
                    "unbound catalog byte {key}:{offset}"
                );
            }
            for change in 0..5 {
                let mut changed = copy_proof(&valid);
                let rows = if maps {
                    &mut changed.initial.maps
                } else {
                    &mut changed.initial.files
                };
                match change {
                    0 => {
                        rows.remove(key);
                    }
                    1 => {
                        rows.get_mut(key).unwrap().payload.push(0);
                    }
                    2 => {
                        rows.get_mut(key).unwrap().version += 1;
                    }
                    3 => {
                        rows.get_mut(key).unwrap().namespace.push('X');
                    }
                    4 => {
                        rows.get_mut(key).unwrap().key.push('X');
                    }
                    _ => unreachable!(),
                }
                changed.reopened = Some(changed.initial.clone());
                assert!(
                    changed.require_complete().is_err(),
                    "unbound catalog metadata {key}:{change}"
                );
            }
        }
        let mut extra = copy_proof(&valid);
        let rows = if maps {
            &mut extra.initial.maps
        } else {
            &mut extra.initial.files
        };
        let mut row = rows.values().next().unwrap().clone();
        row.key = "UNOFFFERED".into();
        rows.insert(row.key.clone(), row);
        extra.reopened = Some(extra.initial.clone());
        assert!(extra.require_complete().is_err());
    }
}

#[test]
fn every_profile_missing_excess_owner_uacc_or_acl_change_is_refused() {
    let valid = proof();
    for key in valid.initial.profiles.keys() {
        for change in 0..8 {
            let mut changed = copy_proof(&valid);
            if change == 0 {
                changed.initial.profiles.remove(key);
            } else {
                let row = changed.initial.profiles.get_mut(key).unwrap();
                match change {
                    1 => row["owner"] = serde_json::json!("WEBUSER"),
                    2 => row["uacc"] = serde_json::json!("ALTER"),
                    3 => row["access_list"][0]["access"] = serde_json::json!("ALTER"),
                    4 => row["access_list"][0]["principal"] = serde_json::json!("NOBODY"),
                    5 => row["generic"] = serde_json::json!(true),
                    6 => row["version"] = serde_json::json!(0),
                    7 => row["access_list"].as_array_mut().unwrap().clear(),
                    _ => unreachable!(),
                }
            }
            changed.reopened = Some(changed.initial.clone());
            assert!(
                changed.require_complete().is_err(),
                "unbound profile metadata {key}:{change}"
            );
        }
    }
    let mut extra = copy_proof(&valid);
    extra
        .initial
        .profiles
        .insert("FACILITY:UNOFFFERED".into(), serde_json::json!({}));
    extra.reopened = Some(extra.initial.clone());
    assert!(extra.require_complete().is_err());
}

#[test]
fn every_authorization_matrix_flip_or_truncated_result_is_refused() {
    let valid = proof();
    assert_eq!(valid.decisions.len(), 810);
    for offset in 0..810 {
        let mut changed = copy_proof(&valid);
        changed.decisions[offset] = !changed.decisions[offset];
        assert!(
            changed.require_complete().is_err(),
            "unbound decision {offset}"
        );
    }
    let mut missing = copy_proof(&valid);
    missing.decisions.pop();
    assert!(missing.require_complete().is_err());
    let mut extra = copy_proof(&valid);
    extra.decisions.push(false);
    assert!(extra.require_complete().is_err());
}

#[test]
fn initial_inventory_alone_or_reopen_drift_cannot_emit_catalog_observations() {
    let mut incomplete = proof();
    incomplete.reopened = None;
    let mut observations = RouteObservations::default();
    assert!(incomplete.bind(&mut observations).is_err());
    assert!(observations.journeys().is_empty());
    let mut changed = proof();
    changed.reopened.as_mut().unwrap().files.clear();
    assert!(changed.bind(&mut observations).is_err());
    assert!(observations.journeys().is_empty());
}

#[test]
fn fixture_policy_separates_regular_admin_and_unknown_principals_at_every_intent() {
    let oracle = oracle().unwrap();
    for row in &oracle.profiles {
        for principal in ["WEBADM", "WEBUSER", "NOBODY"] {
            assert!(!expected_allow(row, principal, AccessIntent::Alter));
            assert!(!expected_allow(row, principal, AccessIntent::Control));
        }
        assert_eq!(
            expected_allow(row, "WEBUSER", AccessIntent::Execute),
            row.regular
        );
        assert!(!expected_allow(row, "NOBODY", AccessIntent::Execute));
        assert!(expected_allow(row, "WEBADM", AccessIntent::Execute));
    }
    let denied_transactions = oracle
        .profiles
        .iter()
        .filter(|row| row.class_name == "TCICSTRN" && !row.regular)
        .map(|row| row.resource.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        denied_transactions,
        [
            "CICS.CA00",
            "CICS.CU00",
            "CICS.CU01",
            "CICS.CU02",
            "CICS.CU03"
        ]
        .into_iter()
        .collect()
    );
    assert!(
        !oracle
            .profiles
            .iter()
            .any(|row| row.resource == "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH")
    );
}
