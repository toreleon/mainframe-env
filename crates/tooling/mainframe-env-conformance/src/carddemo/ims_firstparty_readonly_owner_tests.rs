//! Original separately ignored readonly owner; unchanged body and zero-credit scope.
use super::super::ims_firstparty_readonly::{Oracle, exercise, sources};
use super::*;

// Retain both original pure-control selector paths after the implementation move.
mod oracle {
    mod tests {
        #[test]
        fn complete_copy_images_refuse_filler_parent_order_width_and_eof_drift() {
            super::super::super::super::ims_firstparty_readonly::oracle::tests::complete_copy_images_refuse_filler_parent_order_width_and_eof_drift();
        }
        #[test]
        fn complete_named_map_refuses_unknown_missing_binary_and_width_drift() {
            super::super::super::super::ims_firstparty_readonly::oracle::tests::complete_named_map_refuses_unknown_missing_binary_and_width_drift();
        }
    }
}

#[test]
#[ignore = "real installed COPAUS0C readonly owner; exact pinned corpus/private oracle and fresh external evidence; no J15–18/vendor/hold/rawCALL credit"]
fn pinned_installed_copaus0_readonly_guest_compares_complete_seed_and_named_map() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let corpus = verify_carddemo_corpus_from_env(
        &repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    sources(&corpus_dir).unwrap();
    let offered_oracle = PathBuf::from(
        env::var_os("CARDDEMO_IMS_READONLY_ORACLE")
            .expect("exact external private oracle required"),
    );
    let oracle =
        Oracle::read(&offered_oracle).expect("independent private oracle admission refused");
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_IMS_READONLY_EVIDENCE").expect("fresh external evidence required"),
    );
    let parent = offered.parent().unwrap().canonicalize().unwrap();
    assert!(
        !parent.starts_with(repository.canonicalize().unwrap()),
        "evidence stays outside Git"
    );
    let evidence = parent.join(offered.file_name().unwrap());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .expect("exclusive evidence required");
    let metadata = fs::symlink_metadata(&evidence).unwrap();
    let identity = (metadata.dev(), metadata.ino());
    let artifacts = evidence.join("owned-artifacts");
    require_owned_directory(&evidence, identity).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .unwrap();
    let metadata = fs::symlink_metadata(&artifacts).unwrap();
    let artifact_identity = (metadata.dev(), metadata.ino());
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, artifact_identity).unwrap();
    let mut receipt = serde_json::json!({"schema":"external.carddemo-installed-copaus0-readonly-owner@1",
        "corpus_commit":corpus.commit,"backend":"memory","physical_store_restart":false,
        "source_program":"COPAUS0C","transaction":"CPVS","stage":"before-open",
        "first_drain":false,"second_drain":false,"reopen_compared":false,"cleanup_verified":false,
        "session_disconnect_checked":false,"actual_installed_guest_compared":false,"actual_ims_calls":0,
        "whole_seed_hierarchy_compared":false,"fixed_named_fields_compared":0,
        "root_seed_bytes":100,"child_seed_bytes":200,"child_seed_count":5,
        "whole_private_guest_storage_postimage_claimed":false,"raw_storage_retained":false,
        "original_j15_j16_j17_j18_criteria_unchanged":true,"application_credit":0,"vendor_credit":0,
        "acceptance_credit":0,"artifact_root":artifacts,"artifact_identity":artifact_identity,
        "controls":"default config/limits/router/date; original public HTTP and process bounds"});
    owned.save(&receipt).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(exercise(&corpus_dir, &oracle, &mut owned, &mut receipt));
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    owned.save(&receipt).unwrap();
    assert!(
        result.is_ok(),
        "real readonly owner refused; metadata retained at {}",
        evidence.display()
    );
    assert_eq!(receipt["cleanup_verified"], true);
}
