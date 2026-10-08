//! Existing SecretRef trust authority with explicit current and retained MAC profiles.

use crate::EnvironmentSecretResolver;
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use mainframe_env_application::{
    LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM, PACKAGE_AUTHENTICATION_ALGORITHM,
    PackageSignatureVerifier, verify_package_authentication,
};
use mainframe_env_host_api::{HostProblem, SecretRef};
use mainframe_env_racf::SecretResolver;
use ring::hmac;
use std::{collections::BTreeMap, sync::Arc};

pub struct HmacSha256PackageTrust {
    references: BTreeMap<String, SecretRef>,
    secrets: Arc<dyn SecretResolver>,
}

impl HmacSha256PackageTrust {
    pub fn new(
        references: BTreeMap<String, SecretRef>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, HostProblem> {
        if references.len() > 1_024
            || references.iter().any(|(key_id, _)| {
                key_id.is_empty() || key_id.len() > 128 || key_id.chars().any(char::is_control)
            })
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            references,
            secrets,
        })
    }

    pub fn from_environment(
        environment: &BTreeMap<String, String>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, HostProblem> {
        let Some(encoded) = environment.get("MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS") else {
            return Self::new(BTreeMap::new(), secrets);
        };
        let encoded: BTreeMap<String, String> =
            serde_json::from_str(encoded).map_err(|_| HostProblem::Malformed)?;
        let references = encoded
            .into_iter()
            .map(|(key_id, reference)| {
                EnvironmentSecretResolver::parse_reference(&reference)
                    .map(|reference| (key_id, reference))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Self::new(references, secrets)
    }

    fn verify_mac(&self, key_id: &str, data: &[u8], tag: &[u8]) -> bool {
        let Some(reference) = self.references.get(key_id) else {
            return false;
        };
        let Ok(key) = self.secrets.resolve(reference) else {
            return false;
        };
        if !(32..=4_096).contains(&key.len()) {
            return false;
        }
        hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, &key), data, tag).is_ok()
    }

    fn verify_retained_raw(&self, key_id: &str, identity: &str, value: &str) -> bool {
        if value.len() > 43
            || key_id.is_empty()
            || key_id.len() > 128
            || key_id.chars().any(char::is_control)
        {
            return false;
        }
        let mut tag = [0_u8; 32];
        if STANDARD_NO_PAD.decode_slice(value, &mut tag) != Ok(32) {
            return false;
        }
        self.verify_mac(key_id, identity.as_bytes(), &tag)
    }
}

impl PackageSignatureVerifier for HmacSha256PackageTrust {
    fn allows_fresh_algorithm(&self, algorithm: &str) -> bool {
        algorithm == PACKAGE_AUTHENTICATION_ALGORITHM
    }

    fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
        match algorithm {
            PACKAGE_AUTHENTICATION_ALGORITHM => verify_package_authentication(
                key_id,
                algorithm,
                identity,
                signature,
                |tag, data| self.verify_mac(key_id, data, tag),
            ),
            LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM => {
                self.verify_retained_raw(key_id, identity, signature)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
