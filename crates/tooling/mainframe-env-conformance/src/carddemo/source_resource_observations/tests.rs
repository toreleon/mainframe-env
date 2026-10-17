use super::*;
use mainframe_env_application::BmsField;

fn maps() -> BTreeMap<String, BmsMap> {
    oracle()
        .unwrap()
        .bms
        .into_iter()
        .map(|(path, row)| {
            (
                path,
                BmsMap {
                    mapset: row.mapset,
                    name: row.name,
                    line: row.line,
                    column: row.column,
                    size: row.size,
                    mapset_options: row.mapset_options,
                    map_options: row.map_options,
                    fields: row
                        .fields
                        .into_iter()
                        .map(|field| BmsField {
                            name: field.name,
                            length: field.length,
                            position: field.position,
                            attributes: field.attributes,
                            justify: field.justify,
                            initial: field.initial,
                            options: field.options,
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

fn csd() -> BTreeMap<String, Vec<CsdResource>> {
    oracle()
        .unwrap()
        .csd
        .into_iter()
        .map(|(path, rows)| {
            (
                path,
                rows.into_iter()
                    .map(|row| CsdResource {
                        kind: row.kind,
                        name: row.name,
                        properties: row.properties,
                    })
                    .collect(),
            )
        })
        .collect()
}

fn witnessed() -> SourceResourceWitness {
    let oracle = oracle().unwrap();
    SourceResourceWitness {
        oracle,
        bms_seen: BTreeSet::new(),
        csd_seen: BTreeSet::new(),
    }
}

fn completed_inputs() -> (
    SourceResourceWitness,
    BTreeMap<String, CsdResource>,
    CardDemoResourceReceipt,
) {
    let mut witness = witnessed();
    for (path, actual) in maps() {
        witness.observe_bms(&path, &actual).unwrap();
    }
    for (path, actual) in csd() {
        witness.observe_csd(&path, &actual).unwrap();
    }
    let resources = witness
        .oracle
        .merged_resources
        .iter()
        .map(|(key, row)| {
            (
                key.clone(),
                CsdResource {
                    kind: row.kind.clone(),
                    name: row.name.clone(),
                    properties: row.properties.clone(),
                },
            )
        })
        .collect();
    let count = |kind: &str| witness.oracle.counts[kind];
    let receipt = CardDemoResourceReceipt {
        schema_version: "mainframe-env.carddemo-resource-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: witness.oracle.corpus_commit.clone(),
        bms_sources: 21,
        mapsets: 21,
        maps: 21,
        fields: witness.oracle.fields,
        transactions: count("TRANSACTION"),
        programs: count("PROGRAM"),
        files: count("FILE"),
        tdqueues: count("TDQUEUE"),
        cross_references: 25,
        unresolved: vec!["CDV1->COCRDSEC".into()],
        resource_sha256: "not-an-independent-oracle".into(),
    };
    (witness, resources, receipt)
}

#[test]
fn full_bms_represented_metadata_refuses_each_missing_or_corrupt_field() {
    let expected = oracle().unwrap().bms;
    for (path, actual) in maps() {
        let expected = &expected[&path];
        assert!(map_matches(&actual, expected));
        for index in 0..actual.fields.len() {
            let mut missing = actual.clone();
            missing.fields.remove(index);
            assert!(!map_matches(&missing, expected));
            for changed in 0..7 {
                let mut corrupt = actual.clone();
                let field = &mut corrupt.fields[index];
                match changed {
                    0 => field.name = Some("WRONG".into()),
                    1 => field.length = Some(field.length.unwrap_or(0) + 1),
                    2 => field.position = Some((99, 99)),
                    3 => field.attributes.push("UNDECLARED".into()),
                    4 => field.justify.push("UNDECLARED".into()),
                    5 => field.initial = Some("UNDECLARED".into()),
                    _ => {
                        field.options.insert("UNDECLARED".into(), "WRONG".into());
                    }
                }
                assert!(!map_matches(&corrupt, expected));
            }
        }
    }
}

#[test]
fn full_bms_map_identity_geometry_and_duplicate_path_refuse() {
    for (path, actual) in maps() {
        for changed in 0..5 {
            let mut corrupt = actual.clone();
            match changed {
                0 => corrupt.mapset = "WRONG".into(),
                1 => corrupt.name = "WRONG".into(),
                2 => corrupt.line = Some(99),
                3 => corrupt.column = Some(99),
                _ => corrupt.size = Some((99, 99)),
            }
            assert!(witnessed().observe_bms(&path, &corrupt).is_err());
        }
        let mut witness = witnessed();
        witness.observe_bms(&path, &actual).unwrap();
        assert!(witness.observe_bms(&path, &actual).is_err());
        assert!(witnessed().observe_bms("unoffered.bms", &actual).is_err());
    }
}

#[test]
fn complete_declared_mapset_map_and_field_operands_refuse_missing_extra_or_changed_values() {
    let expected = oracle().unwrap().bms;
    for (path, actual) in maps() {
        let expected = &expected[&path];
        for mapset in [true, false] {
            let options = if mapset {
                &actual.mapset_options
            } else {
                &actual.map_options
            };
            for key in options.keys() {
                for missing in [true, false] {
                    let mut corrupt = actual.clone();
                    let options = if mapset {
                        &mut corrupt.mapset_options
                    } else {
                        &mut corrupt.map_options
                    };
                    if missing {
                        options.remove(key);
                    } else {
                        options.insert(key.clone(), "WRONG".into());
                    }
                    assert!(!map_matches(&corrupt, expected));
                }
            }
            let mut corrupt = actual.clone();
            let options = if mapset {
                &mut corrupt.mapset_options
            } else {
                &mut corrupt.map_options
            };
            options.insert("UNDECLARED".into(), "WRONG".into());
            assert!(!map_matches(&corrupt, expected));
        }
        for (index, field) in actual.fields.iter().enumerate() {
            for key in field.options.keys() {
                for missing in [true, false] {
                    let mut corrupt = actual.clone();
                    if missing {
                        corrupt.fields[index].options.remove(key);
                    } else {
                        corrupt.fields[index]
                            .options
                            .insert(key.clone(), "WRONG".into());
                    }
                    assert!(!map_matches(&corrupt, expected));
                }
            }
        }
    }
}

#[test]
fn full_csd_refuses_same_count_properties_order_kind_and_duplicate_path() {
    let expected = oracle().unwrap().csd;
    for (path, actual) in csd() {
        let expected = &expected[&path];
        assert!(csd_matches(&actual, expected));
        for index in 0..actual.len() {
            let mut missing = actual.clone();
            missing.remove(index);
            assert!(!csd_matches(&missing, expected));
            let mut changed = actual.clone();
            changed[index].kind = "WRONG".into();
            assert!(!csd_matches(&changed, expected));
            let mut changed = actual.clone();
            changed[index].name = "WRONG".into();
            assert!(!csd_matches(&changed, expected));
            for key in actual[index].properties.keys() {
                let mut changed = actual.clone();
                changed[index]
                    .properties
                    .insert(key.clone(), "WRONG".into());
                assert!(!csd_matches(&changed, expected));
            }
        }
        let mut reordered = actual.clone();
        reordered.reverse();
        assert!(!csd_matches(&reordered, expected));
        let mut witness = witnessed();
        witness.observe_csd(&path, &actual).unwrap();
        assert!(witness.observe_csd(&path, &actual).is_err());
    }
}

#[test]
fn complete_parse_source_and_merged_reference_inventory_is_required() {
    for changed in 0..6 {
        let (mut witness, mut resources, receipt) = completed_inputs();
        let mut paths = witness.oracle.source_paths.clone();
        let mut available = witness.oracle.available_programs.clone();
        match changed {
            0 => {
                witness.bms_seen.pop_first();
            }
            1 => {
                witness.csd_seen.pop_first();
            }
            2 => {
                paths.pop();
            }
            3 => {
                paths[0] = paths[1].clone();
            }
            4 => {
                available.pop_first();
            }
            _ => {
                resources.remove("TRANSACTION.CDV1");
            }
        }
        assert!(
            witness
                .finish(&paths, &available, &resources, &receipt)
                .is_err()
        );
    }
}

#[test]
fn pass_counts_digest_and_changed_orphan_cannot_bind() {
    for changed in 0..3 {
        let (witness, mut resources, mut receipt) = completed_inputs();
        let paths = witness.oracle.source_paths.clone();
        let available = witness.oracle.available_programs.clone();
        match changed {
            0 => {
                receipt.unresolved.clear();
            }
            1 => {
                receipt.unresolved = vec!["CDV1->COSGN00C".into()];
            }
            _ => {
                resources
                    .get_mut("TRANSACTION.CDV1")
                    .unwrap()
                    .properties
                    .insert("PROGRAM".into(), "COSGN00C".into());
            }
        }
        let mut observations = RouteObservations::default();
        if let Ok(proof) = witness.finish(&paths, &available, &resources, &receipt) {
            proof.bind(&mut observations).unwrap();
        }
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }
}

#[test]
fn represented_source_proof_binds_only_explicit_orphan_disposition() {
    let (witness, resources, receipt) = completed_inputs();
    let paths = witness.oracle.source_paths.clone();
    let available = witness.oracle.available_programs.clone();
    let proof = witness
        .finish(&paths, &available, &resources, &receipt)
        .unwrap();
    let mut observations = RouteObservations::default();
    proof.bind(&mut observations).unwrap();
    assert!(observations.journeys().is_empty());
    assert_eq!(
        observations.issues(),
        [("CD-010".into(), vec![DISPOSITION.into()])]
    );
}

#[test]
#[ignore = "requires the pinned external first-party corpus; source readback only"]
fn corpus_backed_complete_resource_source_inventory_and_orphan() {
    let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let (receipt, proof) =
        verify_from_env(&inventory).expect("actual full source inventory comparison");
    let mut observations = RouteObservations::default();
    proof.bind(&mut observations).unwrap();
    assert!(observations.journeys().is_empty());
    assert_eq!(
        observations.issues(),
        [("CD-010".into(), vec![DISPOSITION.into()])]
    );
    println!(
        "qualified first-party parsed-source receipt={}; installed-resource/app/IBM/license credit=0",
        serde_json::to_string(&receipt).unwrap()
    );
}
