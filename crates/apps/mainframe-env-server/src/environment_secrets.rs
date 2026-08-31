use base64::Engine;
use mainframe_env_host_api::{HostProblem, SecretRef};
use mainframe_env_racf::{ResolvedSecret, SecretResolver};
use std::sync::Arc;
use zeroize::Zeroizing;

const ENV_SECRET_PREFIX: &str = "MAINFRAME_ENV_SECRET_";
const MAX_SECRET_BYTES: usize = 4_096;
const MAX_BASE64_BYTES: usize = MAX_SECRET_BYTES.div_ceil(3) * 4;

trait EncodedSecretSource: Send + Sync {
    fn read(&self, name: &str) -> Result<Zeroizing<String>, HostProblem>;
}

struct ProcessEnvironment;

impl EncodedSecretSource for ProcessEnvironment {
    fn read(&self, name: &str) -> Result<Zeroizing<String>, HostProblem> {
        std::env::var(name)
            .map(Zeroizing::new)
            .map_err(|_| HostProblem::NotFound)
    }
}

/// Resolves only explicitly referenced package-trust secrets from the process
/// environment. It never snapshots or retains the process environment.
pub struct EnvironmentSecretResolver {
    source: Arc<dyn EncodedSecretSource>,
}

impl EnvironmentSecretResolver {
    pub fn process() -> Self {
        Self {
            source: Arc::new(ProcessEnvironment),
        }
    }

    #[cfg(test)]
    fn with_source(source: Arc<dyn EncodedSecretSource>) -> Self {
        Self { source }
    }
}

impl SecretResolver for EnvironmentSecretResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, HostProblem> {
        let name = environment_name(reference)?;
        let encoded = self.source.read(name)?;
        if encoded.is_empty() || encoded.len() > MAX_BASE64_BYTES {
            return Err(HostProblem::Malformed);
        }
        let mut decoded = Zeroizing::new(Vec::with_capacity(
            encoded.len().saturating_mul(3).saturating_div(4),
        ));
        base64::engine::general_purpose::STANDARD
            .decode_vec(encoded.as_bytes(), &mut decoded)
            .map_err(|_| HostProblem::Malformed)?;
        ResolvedSecret::from_zeroizing(decoded)
    }
}

fn environment_name(reference: &SecretRef) -> Result<&str, HostProblem> {
    reference
        .as_str()
        .strip_prefix("env-base64:")
        .filter(|name| {
            name.starts_with(ENV_SECRET_PREFIX)
                && name.len() > ENV_SECRET_PREFIX.len()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .ok_or(HostProblem::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::HostLimits;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestEnvironment {
        values: Mutex<BTreeMap<String, Zeroizing<String>>>,
    }

    impl TestEnvironment {
        fn insert(&self, name: &str, value: String) {
            self.values
                .lock()
                .expect("test environment mutex")
                .insert(name.into(), Zeroizing::new(value));
        }

        fn remove(&self, name: &str) {
            self.values
                .lock()
                .expect("test environment mutex")
                .remove(name);
        }
    }

    impl EncodedSecretSource for TestEnvironment {
        fn read(&self, name: &str) -> Result<Zeroizing<String>, HostProblem> {
            self.values
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .get(name)
                .map(|value| Zeroizing::new(value.as_str().to_owned()))
                .ok_or(HostProblem::NotFound)
        }
    }

    fn reference(name: &str) -> SecretRef {
        SecretRef::new(format!("env-base64:{name}"), HostLimits::default()).unwrap()
    }

    #[test]
    fn production_resolver_handles_rotation_revocation_and_redacted_failures() {
        let source = Arc::new(TestEnvironment::default());
        let resolver = EnvironmentSecretResolver::with_source(source.clone());
        let name = "MAINFRAME_ENV_SECRET_PACKAGE_KEY";
        let first = b"first-package-verification-key-0001";
        source.insert(
            name,
            base64::engine::general_purpose::STANDARD.encode(first),
        );
        assert_eq!(&*resolver.resolve(&reference(name)).unwrap(), first);

        let second = b"second-package-verification-key-0002";
        source.insert(
            name,
            base64::engine::general_purpose::STANDARD.encode(second),
        );
        assert_eq!(&*resolver.resolve(&reference(name)).unwrap(), second);
        source.remove(name);
        assert_eq!(
            resolver.resolve(&reference(name)).err(),
            Some(HostProblem::NotFound)
        );
    }

    #[test]
    fn production_resolver_rejects_malformed_oversized_and_unsafe_inputs() {
        let source = Arc::new(TestEnvironment::default());
        let resolver = EnvironmentSecretResolver::with_source(source.clone());
        source.insert("MAINFRAME_ENV_SECRET_BAD", "not@base64".into());
        assert_eq!(
            resolver
                .resolve(&reference("MAINFRAME_ENV_SECRET_BAD"))
                .err(),
            Some(HostProblem::Malformed)
        );
        source.insert(
            "MAINFRAME_ENV_SECRET_LARGE",
            "A".repeat(MAX_BASE64_BYTES + 1),
        );
        assert_eq!(
            resolver
                .resolve(&reference("MAINFRAME_ENV_SECRET_LARGE"))
                .err(),
            Some(HostProblem::Malformed)
        );
        for invalid in [
            "PACKAGE_KEY",
            "MAINFRAME_ENV_SECRET_",
            "MAINFRAME_ENV_SECRET_lower",
            "MAINFRAME_ENV_SECRET_BAD-NAME",
        ] {
            assert_eq!(
                resolver.resolve(&reference(invalid)).err(),
                Some(HostProblem::Malformed)
            );
        }
        let shown = format!(
            "{:?}",
            resolver
                .resolve(&reference("MAINFRAME_ENV_SECRET_BAD"))
                .err()
        );
        assert!(!shown.contains("not@base64"));
    }
}
