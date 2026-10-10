//! Synthetic guard/witness controls only; no application or corpus acceptance.
use super::*;
use crate::carddemo::{ContentReceipt, LicenseReceipt, RuntimeOracleReceipt};

fn contract() -> CorpusContract {
    serde_json::from_slice(INVENTORY).unwrap()
}
fn receipt() -> CardDemoCorpusReceipt {
    let contract = contract();
    CardDemoCorpusReceipt {
        schema_version: "mainframe-env.carddemo-corpus-receipt@1".into(),
        status: "pass".into(),
        repository: contract.repository,
        commit: contract.commit,
        tree: contract.tree,
        clean: true,
        license: LicenseReceipt {
            identifier: contract.license,
            sha256: contract.license_sha256,
        },
        content: ContentReceipt {
            algorithm: contract.executable_content_identity.algorithm,
            sha256: contract.executable_content_identity.sha256,
            tracked_files: 1,
        },
        runtime_oracles: contract
            .runtime_oracles
            .into_iter()
            .map(|row| RuntimeOracleReceipt {
                path: row.path,
                sha256: row.sha256,
            })
            .collect(),
        file_counts: contract
            .file_count_checks
            .into_iter()
            .map(|row| (row.id, row.expected))
            .collect(),
    }
}
fn witnessed(receipt: &CardDemoCorpusReceipt) -> GateChecks {
    let mut checks = GateChecks::default();
    checks.declare(&contract()).unwrap();
    for (check, actual) in [
        (FixedCheck::Cleanliness, ""),
        (FixedCheck::Commit, receipt.commit.as_str()),
        (FixedCheck::Tree, receipt.tree.as_str()),
        (FixedCheck::Repository, receipt.repository.as_str()),
        (FixedCheck::License, receipt.license.sha256.as_str()),
        (FixedCheck::Content, receipt.content.sha256.as_str()),
    ] {
        checks.fixed(check, actual, actual).unwrap();
    }
    for row in &receipt.runtime_oracles {
        checks.archive(&row.path, &row.sha256, &row.sha256).unwrap();
    }
    for (id, count) in &receipt.file_counts {
        checks.count(id, *count, *count).unwrap();
    }
    checks
}

#[test]
fn repository_paths_are_redacted_with_original_refusal_code() {
    let expected = "https://example.invalid/pinned.git";
    assert!(require_repository(expected, expected).is_ok());
    for actual in [
        "/private/carddemo",
        "C:\\private\\carddemo",
        "file:///private/carddemo",
        "../private",
    ] {
        let error = require_repository(expected, actual).unwrap_err();
        assert_eq!(error.code, "carddemo.corpus.repository_drift");
        assert!(!error.to_string().contains(actual));
        assert!(!error.to_string().contains(expected));
    }
}

#[test]
fn every_fixed_comparator_is_required_even_with_complete_receipt_counts() {
    let receipt = receipt();
    for omitted in FIXED_CHECKS {
        let mut checks = witnessed(&receipt);
        checks.fixed.remove(&omitted);
        assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
    }
}

#[test]
fn each_declared_archive_and_count_must_have_its_actual_comparison() {
    let receipt = receipt();
    for row in &receipt.runtime_oracles {
        let mut checks = witnessed(&receipt);
        checks.archives.remove(&row.path);
        assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
    }
    for id in receipt.file_counts.keys() {
        let mut checks = witnessed(&receipt);
        checks.counts.remove(id);
        assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
    }
}

#[test]
fn ignored_failed_or_duplicate_comparison_never_recovers_into_proof() {
    let receipt = receipt();
    let mut checks = witnessed(&receipt);
    assert!(
        checks
            .fixed(FixedCheck::Commit, &receipt.commit, &receipt.commit)
            .is_err()
    );
    assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
    let mut checks = witnessed(&receipt);
    assert!(
        checks
            .archive("undeclared", &"0".repeat(64), &"0".repeat(64))
            .is_err()
    );
    assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
    let mut checks = witnessed(&receipt);
    assert!(checks.count("undeclared", 1, 1).is_err());
    assert!(checks.finish(&std::env::temp_dir(), &receipt).is_err());
}

#[test]
fn compared_identity_values_cannot_be_replaced_by_another_pass_receipt() {
    let original = receipt();
    for field in 0..6 {
        let mut changed = original.clone();
        match field {
            0 => changed.commit = "0".repeat(40),
            1 => changed.tree = "0".repeat(40),
            2 => changed.content.sha256 = "0".repeat(64),
            3 => changed.license.sha256 = "0".repeat(64),
            4 => changed.runtime_oracles[0].sha256 = "0".repeat(64),
            _ => *changed.file_counts.values_mut().next().unwrap() += 1,
        }
        assert!(
            witnessed(&original)
                .finish(&std::env::temp_dir(), &changed)
                .is_err()
        );
    }
}

#[test]
fn archive_order_is_not_an_interchangeable_set() {
    let original = receipt();
    let mut changed = original.clone();
    changed.runtime_oracles.swap(0, 1);
    assert!(
        witnessed(&original)
            .finish(&std::env::temp_dir(), &changed)
            .is_err()
    );
}

#[test]
fn locator_admission_refuses_absolute_windows_traversal_empty_and_control_forms() {
    for bad in [
        "/private/archive.zip",
        "C:\\private\\archive.zip",
        "../archive.zip",
        "a//b",
        "a/./b",
        "a/../b",
        "a!../b",
        "a!b!c",
        "a\nb",
        "",
    ] {
        assert!(!runtime_locator(bad), "{bad:?}");
    }
    assert!(runtime_locator(
        "samples/m2/mf/CardDemo_runtime.zip!app_config.json"
    ));
    assert!(!atom("/private/count"));
    assert!(!atom("C:\\private"));
}

#[test]
fn declaration_bounds_duplicates_and_missing_groups_are_refused() {
    let mut value = contract();
    value.runtime_oracles.clear();
    assert!(GateChecks::default().declare(&value).is_err());
    let mut value = contract();
    value
        .file_count_checks
        .push(value.file_count_checks[0].clone());
    assert!(GateChecks::default().declare(&value).is_err());
    let mut value = contract();
    value.runtime_oracles.push(value.runtime_oracles[0].clone());
    assert!(GateChecks::default().declare(&value).is_err());
    let mut value = contract();
    while value.file_count_checks.len() + value.runtime_oracles.len() <= MAX_CHECK_IDENTITIES {
        let mut row = value.file_count_checks[0].clone();
        row.id = format!("additional-{}", value.file_count_checks.len());
        value.file_count_checks.push(row);
    }
    assert!(GateChecks::default().declare(&value).is_err());
}

#[test]
fn oversized_text_and_serialized_evidence_cannot_produce_observations() {
    let mut value = contract();
    value.file_count_checks[0].id = "x".repeat(MAX_TEXT_BYTES + 1);
    assert!(GateChecks::default().declare(&value).is_err());
    let mut value = receipt();
    value.runtime_oracles[0].path = "x".repeat(MAX_TEXT_BYTES + 1);
    assert!(require_relative_evidence(&std::env::temp_dir(), &value).is_err());
    let mut value = receipt();
    value.file_counts = (0..MAX_CHECK_IDENTITIES + 1)
        .map(|i| (format!("{i:02}{}", "x".repeat(MAX_TEXT_BYTES - 2)), 1))
        .collect();
    assert!(require_relative_evidence(&std::env::temp_dir(), &value).is_err());
}

#[test]
fn partial_witness_never_closes_all_four_issue_acceptances() {
    let receipt = receipt();
    let observed = witnessed(&receipt)
        .finish(&std::env::temp_dir(), &receipt)
        .unwrap();
    assert!(observed.journeys().is_empty());
    assert_eq!(
        observed.issues(),
        &[("CD-001".to_string(), vec![IDENTITIES.into(), PATHS.into()])]
    );
}
