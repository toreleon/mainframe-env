use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialVerifier {
    pub algorithm: String,
    pub encoded_verifier: String,
    pub changed_tick: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_phrase: bool,
    #[serde(default)]
    pub history_digests: Vec<String>,
    #[serde(default)]
    pub history_verifiers: Vec<String>,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) const fn default_password_minimum() -> usize {
    8
}

pub(super) const fn default_password_maximum() -> usize {
    100
}
