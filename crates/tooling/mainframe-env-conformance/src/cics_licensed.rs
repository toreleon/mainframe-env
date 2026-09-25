use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const CAPTURE_CONTRACT: &str = "mainframe-env.cics-oracle-capture@1";
const FAMILY_CAPTURE_CONTRACT: &str = "mainframe-env.cics-oracle-capture@2";
const TRUSTED_AUTHORITY: &str = "ibm-cics-protected-runner";
const CAPTURE_SCHEMA: &str =
    include_str!("../../../../conformance/0.9/schemas/cics-oracle-capture.schema.json");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsOracleObservation {
    pub scenario_id: String,
    pub application_output: String,
    pub record_hex: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CicsOracleOriginKind {
    LicensedIbm,
    Local,
    Model,
    Synthetic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsOracleOrigin {
    pub kind: CicsOracleOriginKind,
    pub authority: String,
    pub run_job_id: String,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsOracleCapture {
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_manifest_digest: Option<String>,
    pub candidate_digest: String,
    pub spec_digest: String,
    pub fixture_digest: String,
    pub source_review_digest: String,
    pub environment_manifest_digest: String,
    pub comparison_policy: String,
    pub raw_capture_digest: String,
    pub observations: Vec<CicsOracleObservation>,
    pub origin: CicsOracleOrigin,
}

pub struct CicsOracleExpectation<'a> {
    pub family_id: Option<&'a str>,
    pub family_manifest_digest: Option<&'a str>,
    pub candidate_digest: &'a str,
    pub spec_digest: &'a str,
    pub fixture_digest: &'a str,
    pub source_review_digest: &'a str,
    pub environment_manifest_digest: &'a str,
    pub comparison_policy: &'a str,
    pub required_scenarios: &'a BTreeSet<String>,
    pub required_order: Option<Vec<String>>,
    pub expected_observations: &'a BTreeMap<String, CicsOracleObservation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CicsOracleImport {
    AdapterContractValid,
    Licensed {
        authority: String,
        run_job_id: String,
        receipt_digest: String,
    },
}

impl CicsOracleImport {
    #[must_use]
    pub const fn licensed_credit(&self) -> u8 {
        match self {
            Self::AdapterContractValid => 0,
            Self::Licensed { .. } => 1,
        }
    }
}

pub fn import_cics_oracle_capture(
    bytes: &[u8],
    expected: &CicsOracleExpectation<'_>,
    trusted_public_key: Option<&[u8]>,
) -> Result<CicsOracleImport, String> {
    if bytes.is_empty() || bytes.len() > MAX_CAPTURE_BYTES {
        return Err("CICS oracle capture is empty or exceeds its byte limit".into());
    }
    let capture = parse_cics_oracle_capture(bytes)?;
    let family_capture = expected.family_id.is_some() && expected.family_manifest_digest.is_some();
    if capture.schema_version
        != if family_capture {
            FAMILY_CAPTURE_CONTRACT
        } else {
            CAPTURE_CONTRACT
        }
        || capture.family_id.as_deref() != expected.family_id
        || capture.family_manifest_digest.as_deref() != expected.family_manifest_digest
    {
        return Err("CICS oracle capture contract is stale".into());
    }
    if let Some(manifest_digest) = &capture.family_manifest_digest {
        validate_digest(manifest_digest)?;
    }
    for value in [
        &capture.candidate_digest,
        &capture.spec_digest,
        &capture.fixture_digest,
        &capture.source_review_digest,
        &capture.environment_manifest_digest,
        &capture.raw_capture_digest,
    ] {
        validate_digest(value)?;
    }
    if capture.candidate_digest != expected.candidate_digest
        || capture.spec_digest != expected.spec_digest
        || capture.fixture_digest != expected.fixture_digest
        || capture.source_review_digest != expected.source_review_digest
        || capture.environment_manifest_digest != expected.environment_manifest_digest
        || capture.comparison_policy != expected.comparison_policy
    {
        return Err("CICS oracle capture identity does not match the selected candidate".into());
    }
    let scenarios = capture
        .observations
        .iter()
        .map(|observation| observation.scenario_id.clone())
        .collect::<BTreeSet<_>>();
    if scenarios.len() != capture.observations.len() || &scenarios != expected.required_scenarios {
        return Err("CICS oracle scenario set is missing, duplicate, or unknown".into());
    }
    if expected.required_order.as_ref().is_some_and(|order| {
        capture
            .observations
            .iter()
            .map(|item| &item.scenario_id)
            .collect::<Vec<_>>()
            != order.iter().collect::<Vec<_>>()
    }) {
        return Err("CICS oracle family observation order differs from its sealed manifest".into());
    }
    if expected
        .expected_observations
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        != *expected.required_scenarios
    {
        return Err("CICS oracle reviewed expectation set is incomplete".into());
    }
    if capture.observations.iter().any(|observation| {
        observation.application_output.len() > 16 * 1024
            || observation.record_hex.len() > 16 * 1024
            || !observation.record_hex.len().is_multiple_of(2)
            || !observation
                .record_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err("CICS oracle observation is malformed or exceeds its limit".into());
    }
    let raw = serde_json::to_vec(&capture.observations).map_err(|error| error.to_string())?;
    if capture.raw_capture_digest != digest(&raw) {
        return Err("CICS oracle raw-capture digest does not match its observations".into());
    }
    for observation in &capture.observations {
        let reviewed = expected
            .expected_observations
            .get(&observation.scenario_id)
            .ok_or_else(|| "CICS oracle reviewed observation is missing".to_string())?;
        if observation != reviewed {
            return Err(format!(
                "CICS oracle observation {} does not match the reviewed comparison fixture",
                observation.scenario_id
            ));
        }
    }
    if capture.origin.run_job_id.is_empty() || capture.origin.run_job_id.len() > 256 {
        return Err("CICS oracle run/job identity is missing or too large".into());
    }
    if capture.origin.kind != CicsOracleOriginKind::LicensedIbm {
        if capture.origin.signature.is_some() {
            return Err(
                "non-IBM adapter fixture must not carry licensed signature metadata".into(),
            );
        }
        return Ok(CicsOracleImport::AdapterContractValid);
    }
    if capture.origin.authority != TRUSTED_AUTHORITY {
        return Err("CICS oracle licensed origin names an untrusted authority".into());
    }
    let key = trusted_public_key
        .ok_or_else(|| "CICS oracle protected public key is unavailable".to_string())?;
    let signature = capture
        .origin
        .signature
        .as_deref()
        .ok_or_else(|| "CICS oracle protected signature is missing".to_string())?;
    let signature = STANDARD
        .decode(signature)
        .map_err(|_| "CICS oracle protected signature is malformed".to_string())?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&signed_payload(&capture)?, &signature)
        .map_err(|_| "CICS oracle protected signature did not verify".to_string())?;
    Ok(CicsOracleImport::Licensed {
        authority: capture.origin.authority,
        run_job_id: capture.origin.run_job_id,
        receipt_digest: digest(bytes),
    })
}

fn parse_cics_oracle_capture(bytes: &[u8]) -> Result<CicsOracleCapture, String> {
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let value = StrictJson::deserialize(&mut parser)
        .map_err(|error| format!("CICS oracle capture: {error}"))?
        .0;
    parser
        .end()
        .map_err(|error| format!("CICS oracle capture: {error}"))?;
    let schema: serde_json::Value = serde_json::from_str(CAPTURE_SCHEMA)
        .map_err(|error| format!("CICS oracle capture schema: {error}"))?;
    let validator = jsonschema::draft202012::options()
        .offline()
        .build(&schema)
        .map_err(|error| format!("CICS oracle capture schema did not compile: {error}"))?;
    validator
        .validate(&value)
        .map_err(|error| format!("CICS oracle capture violates its schema: {error}"))?;
    serde_json::from_value(value).map_err(|error| format!("CICS oracle capture: {error}"))
}

// JSON duplicate members can make two parsers disagree about signed identity.
struct StrictJson(serde_json::Value);

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictJson;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object members")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .map(StrictJson)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson(serde_json::Value::Null))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictJson>()? {
                    values.push(value.0);
                }
                Ok(StrictJson(serde_json::Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictJson>()? {
                    if values.insert(key.clone(), value.0).is_some() {
                        return Err(de::Error::custom(format!("duplicate JSON member {key}")));
                    }
                }
                Ok(StrictJson(serde_json::Value::Object(values)))
            }
        }

        deserializer.deserialize_any(StrictVisitor)
    }
}

fn signed_payload(capture: &CicsOracleCapture) -> Result<Vec<u8>, String> {
    let mut payload = serde_json::json!({
        "schema_version": capture.schema_version,
        "candidate_digest": capture.candidate_digest,
        "spec_digest": capture.spec_digest,
        "fixture_digest": capture.fixture_digest,
        "source_review_digest": capture.source_review_digest,
        "environment_manifest_digest": capture.environment_manifest_digest,
        "comparison_policy": capture.comparison_policy,
        "raw_capture_digest": capture.raw_capture_digest,
        "authority": capture.origin.authority,
        "run_job_id": capture.origin.run_job_id,
    });
    if capture.schema_version == FAMILY_CAPTURE_CONTRACT {
        payload["family_id"] = serde_json::json!(capture.family_id);
        payload["family_manifest_digest"] = serde_json::json!(capture.family_manifest_digest);
    }
    serde_json::to_vec(&payload).map_err(|error| error.to_string())
}

fn validate_digest(value: &str) -> Result<(), String> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("CICS oracle identity is not a canonical sha256 digest".into())
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn scenarios() -> BTreeSet<String> {
        ["plain-read", "rollback"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn capture(kind: &str) -> CicsOracleCapture {
        let origin_kind = match kind {
            "licensed-ibm" => CicsOracleOriginKind::LicensedIbm,
            "local" => CicsOracleOriginKind::Local,
            "model" => CicsOracleOriginKind::Model,
            "synthetic" => CicsOracleOriginKind::Synthetic,
            _ => panic!("test origin kind must be part of the contract"),
        };
        let observations = vec![
            CicsOracleObservation {
                scenario_id: "plain-read".into(),
                application_output: "RESP=0".into(),
                record_hex: "c1c1f1f1".into(),
            },
            CicsOracleObservation {
                scenario_id: "rollback".into(),
                application_output: "RESP=0".into(),
                record_hex: "c1c1f2f2".into(),
            },
        ];
        let raw_capture_digest = digest(&serde_json::to_vec(&observations).unwrap());
        CicsOracleCapture {
            schema_version: CAPTURE_CONTRACT.into(),
            family_id: None,
            family_manifest_digest: None,
            candidate_digest: D.into(),
            spec_digest: D.into(),
            fixture_digest: D.into(),
            source_review_digest: D.into(),
            environment_manifest_digest: D.into(),
            comparison_policy: "policy@1".into(),
            raw_capture_digest,
            observations,
            origin: CicsOracleOrigin {
                kind: origin_kind,
                authority: if kind == "licensed-ibm" {
                    TRUSTED_AUTHORITY.into()
                } else {
                    "local-fixture".into()
                },
                run_job_id: "job-1".into(),
                signature: None,
            },
        }
    }

    fn expected_observations(
        capture: &CicsOracleCapture,
    ) -> BTreeMap<String, CicsOracleObservation> {
        capture
            .observations
            .iter()
            .map(|observation| (observation.scenario_id.clone(), observation.clone()))
            .collect()
    }

    fn expectation<'a>(
        required: &'a BTreeSet<String>,
        observations: &'a BTreeMap<String, CicsOracleObservation>,
    ) -> CicsOracleExpectation<'a> {
        CicsOracleExpectation {
            family_id: None,
            family_manifest_digest: None,
            candidate_digest: D,
            spec_digest: D,
            fixture_digest: D,
            source_review_digest: D,
            environment_manifest_digest: D,
            comparison_policy: "policy@1",
            required_scenarios: required,
            required_order: None,
            expected_observations: observations,
        }
    }

    #[test]
    fn local_model_and_synthetic_captures_validate_plumbing_with_zero_credit() {
        let required = scenarios();
        let reviewed = expected_observations(&capture("local"));
        for kind in ["local", "model", "synthetic"] {
            let bytes = serde_json::to_vec(&capture(kind)).unwrap();
            let outcome =
                import_cics_oracle_capture(&bytes, &expectation(&required, &reviewed), None)
                    .unwrap();
            assert_eq!(outcome.licensed_credit(), 0);
        }
    }

    #[test]
    fn malformed_missing_and_wrong_identity_captures_fail_closed() {
        let required = scenarios();
        let reviewed = expected_observations(&capture("local"));
        assert!(
            import_cics_oracle_capture(b"{", &expectation(&required, &reviewed), None).is_err()
        );
        let mut missing = capture("local");
        missing.observations.pop();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&missing).unwrap(),
                &expectation(&required, &reviewed),
                None,
            )
            .is_err()
        );
        let mut wrong = capture("local");
        wrong.environment_manifest_digest =
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&wrong).unwrap(),
                &expectation(&required, &reviewed),
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn capture_schema_and_typed_parser_have_exact_parity() {
        for kind in ["licensed-ibm", "local", "model", "synthetic"] {
            let capture = capture(kind);
            let bytes = serde_json::to_vec(&capture).unwrap();
            assert_eq!(parse_cics_oracle_capture(&bytes).unwrap(), capture);
        }

        let mut missing_signature = serde_json::to_value(capture("local")).unwrap();
        missing_signature["origin"]
            .as_object_mut()
            .unwrap()
            .remove("signature");
        assert!(
            parse_cics_oracle_capture(&serde_json::to_vec(&missing_signature).unwrap())
                .unwrap_err()
                .contains("violates its schema")
        );

        let mut unknown_kind = serde_json::to_value(capture("local")).unwrap();
        unknown_kind["origin"]["kind"] = serde_json::json!("future-unreviewed-kind");
        assert!(
            parse_cics_oracle_capture(&serde_json::to_vec(&unknown_kind).unwrap())
                .unwrap_err()
                .contains("violates its schema")
        );

        let mut extra = serde_json::to_value(capture("local")).unwrap();
        extra["origin"]["unreviewed"] = serde_json::json!(true);
        assert!(
            parse_cics_oracle_capture(&serde_json::to_vec(&extra).unwrap())
                .unwrap_err()
                .contains("violates its schema")
        );
    }

    #[test]
    fn valid_looking_forged_ibm_origin_cannot_grant_credit() {
        let required = scenarios();
        let forged = capture("licensed-ibm");
        let reviewed = expected_observations(&forged);
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&forged).unwrap(),
                &expectation(&required, &reviewed),
                None,
            )
            .unwrap_err()
            .contains("public key")
        );
    }

    #[test]
    fn protected_signature_binds_identity_digest_authority_and_run() {
        let required = scenarios();
        let random = SystemRandom::new();
        let document = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let mut signed = capture("licensed-ibm");
        let reviewed = expected_observations(&signed);
        signed.origin.signature =
            Some(STANDARD.encode(key.sign(&signed_payload(&signed).unwrap())));
        let bytes = serde_json::to_vec(&signed).unwrap();
        let outcome = import_cics_oracle_capture(
            &bytes,
            &expectation(&required, &reviewed),
            Some(key.public_key().as_ref()),
        )
        .unwrap();
        assert_eq!(outcome.licensed_credit(), 1);

        signed.origin.run_job_id = "forged-job".into();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&signed).unwrap(),
                &expectation(&required, &reviewed),
                Some(key.public_key().as_ref()),
            )
            .is_err()
        );
    }

    #[test]
    fn signed_or_local_capture_with_mismatched_behavior_cannot_grant_credit() {
        let required = scenarios();
        let mut actual = capture("local");
        let reviewed = expected_observations(&actual);
        actual.observations[0].record_hex = "c1c1f9f9".into();
        actual.raw_capture_digest = digest(&serde_json::to_vec(&actual.observations).unwrap());
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&actual).unwrap(),
                &expectation(&required, &reviewed),
                None,
            )
            .unwrap_err()
            .contains("reviewed comparison fixture")
        );
    }

    fn family_capture(kind: &str) -> CicsOracleCapture {
        let mut capture = capture(kind);
        capture.schema_version = FAMILY_CAPTURE_CONTRACT.into();
        capture.family_id = Some("bif-builtins-v1".into());
        capture.family_manifest_digest = Some(D.into());
        capture
    }

    fn family_expectation<'a>(
        required: &'a BTreeSet<String>,
        observations: &'a BTreeMap<String, CicsOracleObservation>,
    ) -> CicsOracleExpectation<'a> {
        let mut expected = expectation(required, observations);
        expected.family_id = Some("bif-builtins-v1");
        expected.family_manifest_digest = Some(D);
        expected.required_order = Some(vec!["plain-read".into(), "rollback".into()]);
        expected
    }

    #[test]
    fn family_capture_has_exact_closure_and_zero_credit_without_protected_origin() {
        let required = scenarios();
        let reviewed = expected_observations(&family_capture("local"));
        let expected = family_expectation(&required, &reviewed);
        for kind in ["local", "model", "synthetic"] {
            let actual = family_capture(kind);
            assert_eq!(
                import_cics_oracle_capture(&serde_json::to_vec(&actual).unwrap(), &expected, None)
                    .unwrap()
                    .licensed_credit(),
                0
            );
        }
        let mut missing = family_capture("local");
        missing.observations.pop();
        assert!(
            import_cics_oracle_capture(&serde_json::to_vec(&missing).unwrap(), &expected, None)
                .is_err()
        );
        let mut duplicate = family_capture("local");
        duplicate
            .observations
            .push(duplicate.observations[0].clone());
        assert!(
            import_cics_oracle_capture(&serde_json::to_vec(&duplicate).unwrap(), &expected, None)
                .is_err()
        );
        let mut unknown = family_capture("local");
        unknown.observations[0].scenario_id = "other-family".into();
        unknown.raw_capture_digest = digest(&serde_json::to_vec(&unknown.observations).unwrap());
        assert!(
            import_cics_oracle_capture(&serde_json::to_vec(&unknown).unwrap(), &expected, None)
                .is_err()
        );
        let mut reordered = family_capture("local");
        reordered.observations.reverse();
        reordered.raw_capture_digest =
            digest(&serde_json::to_vec(&reordered.observations).unwrap());
        assert!(
            import_cics_oracle_capture(&serde_json::to_vec(&reordered).unwrap(), &expected, None)
                .is_err()
        );
    }

    #[test]
    fn family_identity_and_protected_signature_reject_tampering() {
        let required = scenarios();
        let mut signed = family_capture("licensed-ibm");
        let reviewed = expected_observations(&signed);
        let expected = family_expectation(&required, &reviewed);
        let random = SystemRandom::new();
        let document = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        signed.origin.signature =
            Some(STANDARD.encode(key.sign(&signed_payload(&signed).unwrap())));
        let valid = serde_json::to_vec(&signed).unwrap();
        assert_eq!(
            import_cics_oracle_capture(&valid, &expected, Some(key.public_key().as_ref()))
                .unwrap()
                .licensed_credit(),
            1
        );
        for changed in [
            {
                let mut value = signed.clone();
                value.family_id = Some("other-family".into());
                value
            },
            {
                let mut value = signed.clone();
                value.family_manifest_digest = Some(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into(),
                );
                value
            },
            {
                let mut value = signed.clone();
                value.candidate_digest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into();
                value
            },
            {
                let mut value = signed.clone();
                value.spec_digest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into();
                value
            },
            {
                let mut value = signed.clone();
                value.fixture_digest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into();
                value
            },
            {
                let mut value = signed.clone();
                value.source_review_digest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into();
                value
            },
            {
                let mut value = signed.clone();
                value.environment_manifest_digest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into();
                value
            },
            {
                let mut value = signed.clone();
                value.comparison_policy = "policy@2".into();
                value
            },
        ] {
            assert!(
                import_cics_oracle_capture(
                    &serde_json::to_vec(&changed).unwrap(),
                    &expected,
                    Some(key.public_key().as_ref())
                )
                .is_err()
            );
        }
        let mut changed_run = signed.clone();
        changed_run.origin.run_job_id = "forged".into();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&changed_run).unwrap(),
                &expected,
                Some(key.public_key().as_ref())
            )
            .is_err()
        );
        let mut changed_authority = signed.clone();
        changed_authority.origin.authority = "forged-runner".into();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&changed_authority).unwrap(),
                &expected,
                Some(key.public_key().as_ref())
            )
            .is_err()
        );
        let mut changed_raw_digest = signed.clone();
        changed_raw_digest.raw_capture_digest = D.into();
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&changed_raw_digest).unwrap(),
                &expected,
                Some(key.public_key().as_ref())
            )
            .is_err()
        );
        let mut behavior = signed.clone();
        behavior.observations[0].record_hex = "c1".into();
        behavior.raw_capture_digest = digest(&serde_json::to_vec(&behavior.observations).unwrap());
        assert!(
            import_cics_oracle_capture(
                &serde_json::to_vec(&behavior).unwrap(),
                &expected,
                Some(key.public_key().as_ref())
            )
            .is_err()
        );
        assert!(import_cics_oracle_capture(&valid, &expected, None).is_err());
    }

    #[test]
    fn v1_and_v2_schema_disallow_cross_contract_identity_fields() {
        let mut v1 = serde_json::to_value(capture("local")).unwrap();
        v1["family_id"] = serde_json::json!("bif-builtins-v1");
        assert!(parse_cics_oracle_capture(&serde_json::to_vec(&v1).unwrap()).is_err());
        let mut v2 = serde_json::to_value(family_capture("local")).unwrap();
        v2.as_object_mut().unwrap().remove("family_manifest_digest");
        assert!(parse_cics_oracle_capture(&serde_json::to_vec(&v2).unwrap()).is_err());
        let duplicate = serde_json::to_string(&family_capture("local")).unwrap();
        let duplicate = duplicate.replacen(
            "\"family_id\":\"bif-builtins-v1\"",
            "\"family_id\":\"bif-builtins-v1\",\"family_id\":\"other-family\"",
            1,
        );
        assert!(
            parse_cics_oracle_capture(duplicate.as_bytes())
                .unwrap_err()
                .contains("duplicate JSON member")
        );
    }
}
