//! Invocation policy and retained equality controls using an explicitly injected verifier.

use super::*;
use crate::{
    LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM as RAW, PACKAGE_AUTHENTICATION_ALGORITHM as COSE,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct PolicyVerifier {
    calls: AtomicUsize,
    revoked: AtomicBool,
}

impl PackageSignatureVerifier for PolicyVerifier {
    fn allows_fresh_algorithm(&self, algorithm: &str) -> bool {
        algorithm == COSE
    }

    fn verify(&self, key: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        !self.revoked.load(Ordering::Relaxed)
            && matches!(key, "test-key" | "rotated-key")
            && matches!(algorithm, RAW | COSE)
            && signature == format!("signed:{identity}")
    }
}

fn candidate(generation: u64, domain: &str, algorithm: &str) -> ApplicationPackageV2 {
    let mut candidate = package(generation);
    candidate.sections.schema_version = domain.into();
    candidate.signature.algorithm = algorithm.into();
    resign(&mut candidate);
    candidate
}

#[test]
fn package_authentication_fresh_policy_refuses_raw_without_mutation_or_second_verification() {
    let verifier = Arc::new(PolicyVerifier::default());
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
    let before = installer.state_payload().unwrap();
    let raw = candidate(1, APPLICATION_PACKAGE_V3_CONTRACT, RAW);
    assert_eq!(installer.stage(&raw), Err(InstallProblem::InvalidSignature));
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 1);
    assert_eq!(installer.state_payload().unwrap(), before);
    let current = candidate(1, APPLICATION_PACKAGE_V3_CONTRACT, COSE);
    installer.stage(&current).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 2);
    installer.commit(&current).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 3);
    installer.install(&current).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 4);
}

#[test]
fn package_authentication_both_legacy_retry_directions_require_complete_equality() {
    for retained_algorithm in [RAW, COSE] {
        let verifier = Arc::new(PolicyVerifier::default());
        let installer =
            ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
        let original = candidate(1, APPLICATION_PACKAGE_V3_CONTRACT, retained_algorithm);
        installer
            .stage_package(&original, PackageAdmission::TrustedRecovery)
            .unwrap();
        installer.commit(&original).unwrap();
        let before = installer.state_payload().unwrap();
        let retry = installer.install(&original).unwrap();
        assert_eq!(retry.state, InstallState::Ready);
        assert_eq!(installer.state_payload().unwrap(), before);
        let mut changed = original.clone();
        changed.signature.algorithm = if retained_algorithm == RAW { COSE } else { RAW }.into();
        let calls = verifier.calls.load(Ordering::Relaxed);
        assert_eq!(
            installer.stage(&changed),
            Err(InstallProblem::IdentityConflict)
        );
        assert_eq!(verifier.calls.load(Ordering::Relaxed), calls + 1);
        assert_eq!(
            installer.commit(&changed),
            Err(InstallProblem::IdentityConflict)
        );
        assert_eq!(verifier.calls.load(Ordering::Relaxed), calls + 2);
        assert_eq!(installer.state_payload().unwrap(), before);
        if retained_algorithm == RAW {
            changed = original.clone();
            changed.signature.key_id = "rotated-key".into();
            assert_eq!(
                installer.stage(&changed),
                Err(InstallProblem::IdentityConflict)
            );
            assert_eq!(
                installer.commit(&changed),
                Err(InstallProblem::IdentityConflict)
            );
            assert_eq!(installer.state_payload().unwrap(), before);
        }
    }
}

#[test]
fn package_authentication_mixed_trusted_recovery_preserves_exact_bytes_and_selection() {
    let verifier = Arc::new(PolicyVerifier::default());
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
    for (generation, domain, algorithm) in [
        (1, APPLICATION_PACKAGE_V2_CONTRACT, RAW),
        (2, APPLICATION_PACKAGE_V3_CONTRACT, RAW),
        (3, APPLICATION_PACKAGE_V3_CONTRACT, COSE),
    ] {
        let retained = candidate(generation, domain, algorithm);
        installer
            .stage_package(&retained, PackageAdmission::TrustedRecovery)
            .unwrap();
        if generation < 3 {
            installer.commit(&retained).unwrap();
        }
    }
    let before = installer.state_payload().unwrap();
    let loaded = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        verifier.clone(),
        &before,
    )
    .unwrap();
    assert_eq!(loaded.state_payload().unwrap(), before);
    assert_eq!(
        loaded.selected_package("DEMO").unwrap().unwrap().generation,
        2
    );
    loaded.rollback("DEMO", 1).unwrap();
    let legacy = candidate(1, APPLICATION_PACKAGE_V2_CONTRACT, RAW);
    assert_eq!(loaded.install(&legacy).unwrap().state, InstallState::Ready);
    assert_eq!(
        loaded.selected_package("DEMO").unwrap().unwrap().as_ref(),
        &legacy
    );
    verifier.revoked.store(true, Ordering::Relaxed);
    assert!(matches!(
        ApplicationInstallerV2::from_state_payload(
            "0.2.0",
            PackageLimits::default(),
            verifier,
            &before,
        ),
        Err(InstallProblem::InvalidSignature)
    ));
    assert_eq!(installer.state_payload().unwrap(), before);
}

#[test]
fn package_authentication_null_selected_staged_raw_recovers_but_fresh_legacy_domain_refuses() {
    let verifier = Arc::new(PolicyVerifier::default());
    let installer =
        ApplicationInstallerV2::new("0.2.0", PackageLimits::default(), verifier.clone());
    let retained = candidate(1, APPLICATION_PACKAGE_V2_CONTRACT, RAW);
    assert_eq!(
        installer.stage(&retained),
        Err(InstallProblem::InvalidIdentity)
    );
    installer
        .stage_package(&retained, PackageAdmission::TrustedRecovery)
        .unwrap();
    let before = installer.state_payload().unwrap();
    let loaded = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        verifier,
        &before,
    )
    .unwrap();
    assert!(loaded.selected_package("DEMO").unwrap().is_none());
    assert_eq!(loaded.state_payload().unwrap(), before);
    assert_eq!(loaded.stage(&retained).unwrap().state, InstallState::Staged);
}
