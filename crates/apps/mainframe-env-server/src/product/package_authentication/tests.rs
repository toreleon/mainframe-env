//! Secret-resolution counts and frozen independent MAC controls.
use super::*;
use mainframe_env_host_api::HostLimits;
use mainframe_env_racf::{MemorySecretResolver, ResolvedSecret};
use std::sync::atomic::{AtomicUsize, Ordering};

const IDENTITY: &str = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const GOLDEN: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIInc9L64UV87fn2kbI1Vc4PWZ1JF0/G+Fe9J",
    "LfxR84E8",
);
const RAW_GOLDEN: &str = "tuU9Gcwv/rExWnfMdUM1XQT1BqCpQzVC0/RXqsD8WX0";
const WRONG_AAD: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIKIC8S1ZpJBiACpgm/npsk8bW6tF6TwS3hrS",
    "5WnsdXA+",
);
const KEY_WIRE_0: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIJlQKnGalAG21Sz+avloWwhUOzQCuCWvRT31",
    "gN02ew1/",
);
const KEY_WIRE_1: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIJlQKnGalAG21Sz+avloWwhUOzQCuCWvRT31",
    "gN02ew1/",
);
const KEY_WIRE_2: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIJlQKnGalAG21Sz+avloWwhUOzQCuCWvRT31",
    "gN02ew1/",
);
const KEY_WIRE_3: &str = concat!(
    "0YRPogEFBEpyZXZpZXcta2V5oFhHc2hhMjU2OmUzYjBjNDQyOThmYzFjMTQ5YWZiZjRjODk5NmZiOTI0",
    "MjdhZTQxZTQ2NDliOTM0Y2E0OTU5OTFiNzg1MmI4NTVYIPUWJDck4vSXS8yb6OrEOCr3Y3UKz2CDjuTN",
    "MzUOrgfI",
);

#[derive(Default)]
struct CountingResolver {
    inner: MemorySecretResolver,
    calls: AtomicUsize,
}
impl SecretResolver for CountingResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.resolve(reference)
    }
}
fn trust(kid: &str, key: Vec<u8>) -> (HmacSha256PackageTrust, Arc<CountingResolver>) {
    let resolver = Arc::new(CountingResolver::default());
    resolver.inner.insert("secret:counted-key", key);
    let reference = SecretRef::new("secret:counted-key", HostLimits::default()).unwrap();
    let trust =
        HmacSha256PackageTrust::new(BTreeMap::from([(kid.into(), reference)]), resolver.clone())
            .unwrap();
    (trust, resolver)
}
fn verify(trust: &HmacSha256PackageTrust, value: &str) -> bool {
    trust.verify(
        "review-key",
        PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        value,
    )
}

#[test]
fn package_trust_profile_resolution_once_rotation_and_explicit_legacy_controls() {
    let (trust, resolver) = trust("review-key", (0_u8..32).collect());
    assert!(trust.allows_fresh_algorithm(PACKAGE_AUTHENTICATION_ALGORITHM));
    assert!(!trust.allows_fresh_algorithm(LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 0);
    assert!(verify(&trust, GOLDEN));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    assert!(trust.verify(
        "review-key",
        LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        RAW_GOLDEN
    ));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 2);
    assert!(!verify(&trust, RAW_GOLDEN));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 2);
    assert!(!trust.verify(
        "review-key",
        LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        GOLDEN
    ));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 2);
    assert!(!verify(&trust, WRONG_AAD));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 3);
    resolver.inner.insert("secret:counted-key", vec![0; 32]);
    assert!(!verify(&trust, GOLDEN));
    assert!(verify(&trust, KEY_WIRE_1));
    resolver.inner.remove("secret:counted-key");
    assert!(!verify(&trust, KEY_WIRE_1));
    assert!(!trust.verify(
        "review-key",
        LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        RAW_GOLDEN
    ));
}

#[test]
fn package_trust_structural_refusals_never_resolve_a_secret() {
    let (trust, resolver) = trust("review-key", (0_u8..32).collect());
    let mut trailing = STANDARD_NO_PAD.decode(GOLDEN).unwrap();
    trailing.push(0);
    for value in [
        "A".repeat(257),
        "A".repeat(1024 * 1024),
        format!("{GOLDEN}="),
        STANDARD_NO_PAD.encode(trailing),
        STANDARD_NO_PAD.encode([0xd1, 0x84, 0x40, 0xa0, 0xf6, 0x40]),
    ] {
        assert!(!verify(&trust, &value));
    }
    assert!(!trust.verify(
        "mismatch",
        PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        GOLDEN
    ));
    assert!(!trust.verify("review-key", "unsupported@1", IDENTITY, GOLDEN));
    for bytes in [0, 31, 33, 1000] {
        assert!(!trust.verify(
            "review-key",
            LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
            IDENTITY,
            &STANDARD_NO_PAD.encode(vec![0; bytes])
        ));
    }
    assert!(!trust.verify(
        "review-key",
        LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        &format!("{RAW_GOLDEN}=")
    ));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn package_trust_preserves_existing_secret_lengths_and_configured_legacy_kids() {
    assert!(matches!(
        ResolvedSecret::new(vec![0; 4097]),
        Err(HostProblem::Malformed)
    ));
    for (length, value, expected) in [
        (31, KEY_WIRE_0, false),
        (32, KEY_WIRE_1, true),
        (33, KEY_WIRE_2, true),
        (4096, KEY_WIRE_3, true),
    ] {
        let (trust, resolver) = trust("review-key", vec![0; length]);
        assert_eq!(verify(&trust, value), expected);
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    }
    let kid = "K".repeat(128);
    let (trust, resolver) = trust(&kid, (0_u8..32).collect());
    assert!(trust.verify(
        &kid,
        LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM,
        IDENTITY,
        RAW_GOLDEN
    ));
    assert!(!trust.verify(&kid, PACKAGE_AUTHENTICATION_ALGORITHM, IDENTITY, GOLDEN));
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    let reference = SecretRef::new("secret:counted-key", HostLimits::default()).unwrap();
    assert!(matches!(
        HmacSha256PackageTrust::new(BTreeMap::from([("K".repeat(129), reference)]), resolver),
        Err(HostProblem::Malformed)
    ));
}

#[test]
fn package_trust_independent_rfc4231_primitive_does_not_admit_a_short_profile_key() {
    let tag = [
        0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b, 0xf1,
        0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c, 0x2e, 0x32,
        0xcf, 0xf7,
    ];
    assert!(
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, &[0x0b; 20]),
            b"Hi There",
            &tag
        )
        .is_ok()
    );
    let (trust, _) = trust("review-key", vec![0x0b; 20]);
    assert!(!verify(&trust, GOLDEN));
}
