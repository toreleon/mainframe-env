use mainframe_env_racf::{command_descriptors, racroute_descriptors};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const RACF_ORACLE_RELATIVE_PATH: &str = "conformance/0.5/racf/licensed-oracle.json";
const MAX_CAMPAIGN_BYTES: usize = 8 * 1024 * 1024;
const MAX_OBSERVATION_BYTES: usize = 65_536;
const BASELINE_ID: &str = "ibm-zos-3.2-racf-saf-2026";
const SOURCE_SHA256: &str =
    "sha256:f4c8860aeb4d00b78f9257b28b2d880bd7571d74e2e00b2b1424b203801d5a46";
const PRODUCT_IDENTITY: &str = "IBM z/OS 3.2 RACF/SAF";
const EVIDENCE_ORIGIN: &str = "external-licensed-zos-execution";
const CAMPAIGN_MODE: &str = "fresh-release-certify-campaign";
const ENVIRONMENT_PREFIX: &str = "licensed-zos:";
const ADAPTER_PREFIX: &str = "approved-licensed-zos-adapter:";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCampaign {
    schema_version: String,
    baseline_id: String,
    source_sha256: String,
    environment_identity: String,
    product_identity: String,
    adapter_identity: String,
    evidence_origin: String,
    campaign_mode: String,
    licensed: bool,
    cases: Vec<RawCase>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    row_id: String,
    surface: String,
    keyword: String,
    fixture_digest: String,
    observation: Value,
}

#[derive(Clone, Debug)]
pub struct RacfOracleCase {
    pub surface: String,
    pub keyword: String,
    pub fixture_digest: String,
    pub observation: Value,
}

#[derive(Clone, Debug)]
pub struct RacfOracleCampaign {
    digest: String,
    environment_identity: String,
    adapter_identity: String,
    cases: BTreeMap<String, RacfOracleCase>,
}

impl RacfOracleCampaign {
    pub fn load_optional(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join(RACF_ORACLE_RELATIVE_PATH);
        if !path.exists() {
            return Ok(None);
        }
        Self::load(&path).map(Some)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.is_empty() || bytes.len() > MAX_CAMPAIGN_BYTES {
            return Err("licensed RACF oracle campaign size is invalid".into());
        }
        let raw: RawCampaign = serde_json::from_slice(&bytes)
            .map_err(|error| format!("licensed RACF oracle campaign is malformed: {error}"))?;
        if raw.schema_version != "mainframe-env.racf-oracle-campaign@1"
            || raw.baseline_id != BASELINE_ID
            || raw.source_sha256 != SOURCE_SHA256
            || raw.product_identity != PRODUCT_IDENTITY
            || !valid_provenance(
                &raw.evidence_origin,
                &raw.campaign_mode,
                &raw.environment_identity,
                &raw.adapter_identity,
                raw.licensed,
            )
        {
            return Err("licensed RACF oracle campaign identity is invalid".into());
        }
        let expected = command_descriptors()
            .iter()
            .map(|descriptor| (descriptor.row_id(), ("command", descriptor.keyword())))
            .chain(
                racroute_descriptors()
                    .iter()
                    .map(|descriptor| (descriptor.row_id(), ("racroute", descriptor.keyword()))),
            )
            .collect::<BTreeMap<_, _>>();
        if raw.cases.len() != expected.len() {
            return Err("licensed RACF oracle campaign is incomplete".into());
        }
        let mut cases = BTreeMap::new();
        for case in raw.cases {
            let Some((surface, keyword)) = expected.get(case.row_id.as_str()) else {
                return Err("licensed RACF oracle campaign contains an unknown row".into());
            };
            if case.surface != *surface
                || case.keyword != *keyword
                || !sha256(&case.fixture_digest)
                || !case.observation.is_object()
                || !valid_observation_shape(&case.surface, &case.keyword, &case.observation)
                || serde_json::to_vec(&case.observation)
                    .map_err(|error| error.to_string())?
                    .len()
                    > MAX_OBSERVATION_BYTES
                || contains_sensitive_value(&case.observation, None)
            {
                return Err(format!(
                    "licensed RACF oracle case is invalid for {}",
                    case.row_id
                ));
            }
            let row_id = case.row_id;
            if cases
                .insert(
                    row_id.clone(),
                    RacfOracleCase {
                        surface: case.surface,
                        keyword: case.keyword,
                        fixture_digest: case.fixture_digest,
                        observation: case.observation,
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate licensed RACF oracle row {row_id}"));
            }
        }
        let actual = cases.keys().map(String::as_str).collect::<BTreeSet<_>>();
        let expected = expected.keys().copied().collect::<BTreeSet<_>>();
        if actual != expected {
            return Err("licensed RACF oracle campaign row set is incomplete".into());
        }
        Ok(Self {
            digest: format!("sha256:{:x}", Sha256::digest(bytes)),
            environment_identity: raw.environment_identity,
            adapter_identity: raw.adapter_identity,
            cases,
        })
    }

    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    #[must_use]
    pub fn environment_identity(&self) -> &str {
        &self.environment_identity
    }

    #[must_use]
    pub fn adapter_identity(&self) -> &str {
        &self.adapter_identity
    }

    #[must_use]
    pub fn case(&self, row_id: &str) -> Option<&RacfOracleCase> {
        self.cases.get(row_id)
    }
}

fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 246
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'-' | b'_'))
}

fn valid_provenance(
    evidence_origin: &str,
    campaign_mode: &str,
    environment_identity: &str,
    adapter_identity: &str,
    licensed: bool,
) -> bool {
    licensed
        && evidence_origin == EVIDENCE_ORIGIN
        && campaign_mode == CAMPAIGN_MODE
        && safe_identity(environment_identity)
        && environment_identity.starts_with(ENVIRONMENT_PREFIX)
        && safe_identity(adapter_identity)
        && adapter_identity.starts_with(ADAPTER_PREFIX)
}

fn sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn contains_sensitive_value(value: &Value, key: Option<&str>) -> bool {
    if key.is_some_and(sensitive_name) {
        return value.as_str() != Some("<redacted>");
    }
    match value {
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| contains_sensitive_value(value, Some(key))),
        Value::Array(values) => values
            .iter()
            .any(|value| contains_sensitive_value(value, None)),
        Value::String(value) => {
            let upper = value.to_ascii_uppercase();
            value.starts_with("secret:")
                || value.starts_with("vault:")
                || value.starts_with("keyring:")
                || value.starts_with("$argon2")
                || upper.contains("BEGIN PRIVATE KEY")
                || upper.contains("BEGIN CERTIFICATE")
        }
        _ => false,
    }
}

fn valid_observation_shape(surface: &str, keyword: &str, observation: &Value) -> bool {
    let Some(values) = observation.as_object() else {
        return false;
    };
    if values.get("surface").and_then(Value::as_str) != Some(surface)
        || values.get("keyword").and_then(Value::as_str) != Some(keyword)
        || !values.get("status").is_some_and(Value::is_object)
    {
        return false;
    }
    match surface {
        "command" => values.len() == 4 && values.get("records").is_some_and(Value::is_array),
        "racroute" => {
            values.len() == 5
                && values.get("states").is_some_and(Value::is_array)
                && values.contains_key("result")
        }
        _ => false,
    }
}

fn sensitive_name(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    [
        "PASSWORD",
        "PHRASE",
        "CREDENTIAL",
        "SECRET",
        "TOKEN_REFERENCE",
        "KEY_REFERENCE",
        "CERTIFICATE_REFERENCE",
        "PASSCODE",
        "MFA_PROOF",
        "ASSERTION",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_campaign_is_pending_and_never_synthetic() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-oracle-missing-{}",
            std::process::id()
        ));
        assert!(
            RacfOracleCampaign::load_optional(&directory)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn unlicensed_prohibited_origin_or_secret_bearing_campaign_is_rejected_before_credit() {
        assert!(!safe_identity("unsafe identity"));
        let licensed_environment = "licensed-zos:test-environment";
        let approved_adapter = "approved-licensed-zos-adapter:test-adapter";
        assert!(valid_provenance(
            EVIDENCE_ORIGIN,
            CAMPAIGN_MODE,
            licensed_environment,
            approved_adapter,
            true,
        ));
        for prohibited_origin in [
            "simulated-reference",
            "modeled-output",
            "documentation-derived",
            "current-product-output",
        ] {
            assert!(!valid_provenance(
                prohibited_origin,
                CAMPAIGN_MODE,
                licensed_environment,
                approved_adapter,
                true,
            ));
        }
        assert!(!valid_provenance(
            EVIDENCE_ORIGIN,
            "historical-receipt",
            licensed_environment,
            approved_adapter,
            true,
        ));
        assert!(!valid_provenance(
            EVIDENCE_ORIGIN,
            CAMPAIGN_MODE,
            "current-product",
            approved_adapter,
            true,
        ));
        assert!(!valid_provenance(
            EVIDENCE_ORIGIN,
            CAMPAIGN_MODE,
            licensed_environment,
            "local-observation-builder",
            true,
        ));
        assert!(!valid_provenance(
            EVIDENCE_ORIGIN,
            CAMPAIGN_MODE,
            licensed_environment,
            approved_adapter,
            false,
        ));
        assert!(contains_sensitive_value(
            &serde_json::json!({"note": "secret:must-not-retain"}),
            None
        ));
        assert!(contains_sensitive_value(
            &serde_json::json!({"password": "not-redacted"}),
            None
        ));
    }

    #[test]
    fn structurally_complete_unit_campaign_loads_but_is_not_conformance_evidence() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-oracle-structure-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("licensed-oracle.json");
        let cases = command_descriptors()
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                unit_case(
                    descriptor.row_id(),
                    "command",
                    descriptor.keyword(),
                    index + 1,
                )
            })
            .chain(
                racroute_descriptors()
                    .iter()
                    .enumerate()
                    .map(|(index, descriptor)| {
                        unit_case(
                            descriptor.row_id(),
                            "racroute",
                            descriptor.keyword(),
                            index + 1,
                        )
                    }),
            )
            .collect::<Vec<_>>();
        let document = serde_json::json!({
            "schema_version": "mainframe-env.racf-oracle-campaign@1",
            "baseline_id": BASELINE_ID,
            "source_sha256": SOURCE_SHA256,
            "product_identity": PRODUCT_IDENTITY,
            "adapter_identity": "approved-licensed-zos-adapter:unit-test-structural",
            "evidence_origin": EVIDENCE_ORIGIN,
            "campaign_mode": CAMPAIGN_MODE,
            "environment_identity": "licensed-zos:unit-test-not-evidence",
            "licensed": true,
            "cases": cases,
        });
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        let campaign = RacfOracleCampaign::load(&path).unwrap();
        assert_eq!(campaign.cases.len(), 48);
        assert!(sha256(campaign.digest()));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    fn unit_case(row_id: &str, surface: &str, keyword: &str, sequence: usize) -> Value {
        let fixture =
            format!("racf.{surface}.{sequence:04}.licensed-equivalence.differential.fixture");
        let observation = if surface == "command" {
            serde_json::json!({
                "surface": surface,
                "keyword": keyword,
                "status": {},
                "records": [],
            })
        } else {
            serde_json::json!({
                "surface": surface,
                "keyword": keyword,
                "status": {},
                "states": [],
                "result": null,
            })
        };
        serde_json::json!({
            "row_id": row_id,
            "surface": surface,
            "keyword": keyword,
            "fixture_digest": format!("sha256:{:x}", Sha256::digest(fixture.as_bytes())),
            "observation": observation,
        })
    }
}
