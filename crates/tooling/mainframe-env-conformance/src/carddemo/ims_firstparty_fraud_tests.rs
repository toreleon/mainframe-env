use super::reference::Reference;
use super::*;

#[test]
fn full_twenty_six_column_reference_refuses_any_field_order_width_or_date_substitution() {
    let reference = Reference::independent().unwrap();
    for action in ["F", "R"] {
        let expected = &reference.local_raw_rows[action];
        reference.row(action, expected).unwrap();
        for at in 0..26 {
            let mut changed = expected.clone();
            changed[at].push(0);
            assert!(reference.row(action, &changed).is_err());
            let mut changed = expected.clone();
            changed[at][0] ^= 1;
            assert!(reference.row(action, &changed).is_err());
        }
        let mut changed = expected.clone();
        changed.swap(0, 1);
        assert!(reference.row(action, &changed).is_err());
        let mut changed = expected.clone();
        changed.pop();
        assert!(reference.row(action, &changed).is_err());
        let mut changed = expected.clone();
        changed[23] = b"CURRENT DATE".to_vec();
        assert!(reference.row(action, &changed).is_err());
        changed[23].clear();
        assert!(reference.row(action, &changed).is_err());
    }
    assert!(
        reference
            .row("unknown", &reference.local_raw_rows["F"])
            .is_err()
    );
}

#[test]
fn complete_two_hundred_byte_child_and_neighbor_refuse_partial_or_out_of_band_rewrite() {
    let reference = Reference::independent().unwrap();
    for action in ["F", "R"] {
        let child = &reference.expected_children[action];
        reference
            .child_and_neighbor(action, child, &reference.unchanged_neighbor)
            .unwrap();
        for at in 0..200 {
            let mut changed = child.clone();
            changed[at] ^= 1;
            assert!(
                reference
                    .child_and_neighbor(action, &changed, &reference.unchanged_neighbor)
                    .is_err()
            );
            let mut neighbor = reference.unchanged_neighbor.clone();
            neighbor[at] ^= 1;
            assert!(
                reference
                    .child_and_neighbor(action, child, &neighbor)
                    .is_err()
            );
        }
        assert!(
            reference
                .child_and_neighbor(action, &child[..199], &reference.unchanged_neighbor)
                .is_err()
        );
        assert!(
            reference
                .child_and_neighbor(
                    action,
                    &reference.original_child,
                    &reference.unchanged_neighbor
                )
                .is_err()
        );
    }
}

#[test]
fn complete_link_commarea_refuses_account_action_status_message_date_or_filler_drift() {
    let reference = Reference::independent().unwrap();
    for action in ["F", "R"] {
        let area = &reference.commareas[action];
        reference.commarea(action, area).unwrap();
        for at in 0..272 {
            let mut changed = area.clone();
            changed[at] ^= 1;
            assert!(reference.commarea(action, &changed).is_err());
        }
        assert!(reference.commarea(action, &area[..271]).is_err());
        let other = if action == "F" { "R" } else { "F" };
        assert!(
            reference
                .commarea(action, &reference.commareas[other])
                .is_err()
        );
    }
}

#[test]
fn complete_named_detail_images_refuse_old_phase_missing_unknown_and_nul_space_normalization() {
    let reference = Reference::independent().unwrap();
    for (phase, expected) in &reference.fields {
        reference.screen(phase, expected).unwrap();
        for name in expected.keys() {
            let mut changed = expected.clone();
            changed.remove(name);
            assert!(reference.screen(phase, &changed).is_err());
            let mut changed = expected.clone();
            changed.get_mut(name).unwrap()[0] ^= 1;
            assert!(reference.screen(phase, &changed).is_err());
        }
        let mut changed = expected.clone();
        changed.insert("UNKNOWN".into(), vec![0]);
        assert!(reference.screen(phase, &changed).is_err());
    }
    assert!(
        reference
            .screen(
                "unheld-replace-rollback",
                &reference.fields["reported-space"]
            )
            .is_err()
    );
    assert!(
        reference
            .screen(
                "reported-space",
                &reference.fields["disabled-link-rollback"]
            )
            .is_err()
    );
}

#[test]
fn source_reference_cannot_label_current_hold_failure_or_local_date_artifacts_as_application_success()
 {
    let reference = Reference::independent().unwrap();
    reference.validate().unwrap();
    let mut changed = reference.clone();
    changed.successful_source_acceptance_authorized = true;
    assert!(changed.validate().is_err());
    let mut changed = reference.clone();
    changed.application_or_issue_credit = 1;
    assert!(changed.validate().is_err());
    let mut changed = reference.clone();
    changed.cics_reference_utc_date = "2026-10-17".into();
    assert!(changed.validate().is_err());
    let mut changed = reference.clone();
    changed.expected_children.get_mut("F").unwrap()[183] ^= 1;
    assert!(changed.validate().is_err());
    let mut changed = reference.clone();
    changed.commareas.get_mut("F").unwrap()[221] = b'F';
    assert!(changed.validate().is_err());
}

#[test]
#[ignore = "actual original COPAUS1C/COPAUS2C source compilation with exact source corpus and authorization DCL; no runtime or application observation"]
fn pinned_original_fraud_pair_compiles_with_original_authorization_dcl_and_distinct_artifacts() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    verify_carddemo_corpus_from_env(
        &repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .expect("original corpus admission required");
    let corpus =
        PathBuf::from(env::var_os(CORPUS_ENV).expect("explicit pinned corpus root required"));
    let prepared =
        Prepared::new(&corpus).expect("original pair and DCL must compile without source rewrite");
    assert_eq!(prepared.online.programs.len(), 3);
    assert_eq!(prepared.online.maps.len(), 2);
    assert_eq!(
        prepared.online.transactions.get("CPVD").map(String::as_str),
        Some("COPAUS1C")
    );
    assert_eq!(prepared.records.len(), 3);
    assert_eq!(prepared.source_ids.len(), 3);
    assert_ne!(
        prepared.records["COPAUS1C"].artifact,
        prepared.records["COPAUS2C"].artifact
    );
}

#[test]
#[ignore = "real original CPVS→COPAUS1C PF5 with deliberately unavailable COPAUS2C; exact source/seed/screens/replay/audits/checked teardown; zero application observations"]
fn installed_original_fraud_caller_rolls_back_an_unavailable_link_without_dirty_uow_claim() {
    super::owner::verify_unavailable_link_from_env().expect(
        "actual original missing-target LINK/ROLLBACK and unchanged whole postimages required",
    );
}
