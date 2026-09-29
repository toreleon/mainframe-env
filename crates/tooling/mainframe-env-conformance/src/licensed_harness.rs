use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_REGISTRY_BYTES: usize = 1024 * 1024;
const MAX_RECEIPT_BYTES: usize = 4 * 1024 * 1024;
const ENVIRONMENT_SCHEMA: &str =
    include_str!("../../../../conformance/0.17/schemas/licensed-environment-manifest.schema.json");
const REGISTRY_SCHEMA: &str =
    include_str!("../../../../conformance/0.17/schemas/oracle-harness-registry.schema.json");
const RECEIPT_SCHEMA: &str =
    include_str!("../../../../conformance/0.17/schemas/oracle-harness-receipt.schema.json");
const COBOL_RECEIPT_SCHEMA: &str = include_str!(
    "../../../../conformance/spec/schemas/cobol-licensed-differential-receipt.schema.json"
);
const RACF_RECEIPT_SCHEMA: &str =
    include_str!("../../../../conformance/0.5/schemas/racf-oracle-campaign.schema.json");
const DATASET_RECEIPT_SCHEMA: &str =
    include_str!("../../../../conformance/0.6/schemas/dataset-oracle-receipt.schema.json");
const JES_RECEIPT_SCHEMA: &str = include_str!(
    "../../../../conformance/0.8/schemas/jes-licensed-differential-receipt.schema.json"
);
const CICS_RECEIPT_SCHEMA: &str =
    include_str!("../../../../conformance/0.9/schemas/cics-oracle-capture.schema.json");
const REQUIRED_SLOTS: [&str; 10] = [
    "cobol",
    "racf-saf",
    "dataset-vsam-ams",
    "jes2",
    "cics",
    "db2",
    "ims",
    "mq",
    "zosmf",
    "cross-resource",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleCandidateExpectation {
    pub slot_id: String,
    pub source_commit: String,
    pub source_tree_digest: String,
    pub artifacts: BTreeMap<String, String>,
    pub catalogs: BTreeMap<String, String>,
    pub conformance_spec_digest: String,
    pub fixture_digest: String,
    pub oracle_adapter_digest: String,
    pub normalization_policy_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleHarnessValidationKind {
    PlumbingOnly,
    CandidateBoundLicensedCapture,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleHarnessValidation {
    slot_id: String,
    receipt_digest: String,
    kind: OracleHarnessValidationKind,
}

impl OracleHarnessValidation {
    #[must_use]
    pub fn slot_id(&self) -> &str {
        &self.slot_id
    }

    #[must_use]
    pub fn receipt_digest(&self) -> &str {
        &self.receipt_digest
    }

    #[must_use]
    pub const fn kind(&self) -> OracleHarnessValidationKind {
        self.kind
    }

    #[must_use]
    pub const fn licensed_differential_credit(&self) -> u8 {
        0
    }
}

#[derive(Clone, Debug)]
pub struct OracleHarnessRegistry {
    digest: String,
    slots: BTreeMap<String, OracleHarnessSlot>,
}

impl OracleHarnessRegistry {
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    #[must_use]
    pub fn slot_ids(&self) -> impl Iterator<Item = &str> {
        self.slots.keys().map(String::as_str)
    }
}

#[derive(Clone, Debug)]
struct OracleHarnessSlot {
    required_baselines: BTreeSet<String>,
    fixture_digest: Option<String>,
    adapter_ready: bool,
    accepted_read_versions: BTreeSet<String>,
    validator_argv: Vec<String>,
    required_environment_variables: BTreeSet<String>,
    normalization_reviewed: bool,
    normalization_policy_id: String,
    normalization_policy_digest: Option<String>,
    expected_cases: Option<u64>,
    max_raw_capture_bytes: u64,
    protected_attestation_required: bool,
}

#[derive(Debug, Deserialize)]
struct RawRegistry {
    slots: Vec<RawSlot>,
}

#[derive(Debug, Deserialize)]
struct RawSlot {
    slot_id: String,
    required_baselines: Vec<String>,
    fixture: RawFixture,
    adapter: RawAdapter,
    capture: RawCapture,
    normalization: RawNormalization,
    bounds: RawBounds,
}

#[derive(Debug, Deserialize)]
struct RawFixture {
    state: String,
    path: Option<String>,
    digest: Option<String>,
    digest_rule: String,
}

#[derive(Debug, Deserialize)]
struct RawAdapter {
    state: String,
    policy_path: Option<String>,
    accepted_read_versions: Vec<String>,
    read_rule: String,
    validator_argv: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawCapture {
    protected_attestation_required: bool,
    required_environment_variables: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawNormalization {
    state: String,
    policy_id: String,
    policy_digest: Option<String>,
    rules: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct RawBounds {
    expected_cases: Option<u64>,
    max_raw_capture_bytes: u64,
}

#[derive(Clone, Debug)]
struct EnvironmentSummary {
    digest: String,
    class: String,
    licensed_execution: bool,
    source_baselines: BTreeSet<String>,
    available_slots: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct RawReceipt {
    slot_id: String,
    receipt_state: String,
    origin: RawOrigin,
    bindings: RawBindings,
    compatibility: RawCompatibility,
    execution: RawExecution,
    normalization: RawReceiptNormalization,
    replay: RawReplay,
}

#[derive(Debug, Deserialize)]
struct RawOrigin {
    kind: String,
    protected_attestation_digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawBindings {
    source_commit: String,
    source_tree_digest: String,
    artifacts: Vec<NamedDigest>,
    catalogs: Vec<NamedDigest>,
    conformance_spec_digest: String,
    fixture_digest: String,
    oracle_adapter_digest: String,
    normalization_policy_digest: String,
    environment_manifest_digest: String,
    harness_registry_digest: String,
}

#[derive(Debug, Deserialize)]
struct NamedDigest {
    id: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct RawCompatibility {
    legacy_schema_version: String,
    legacy_receipt_digest: String,
    read_rule: String,
    subsystem_validator_status: String,
}

#[derive(Debug, Deserialize)]
struct RawExecution {
    status: String,
    expected_cases: u64,
    observed_cases: u64,
}

#[derive(Debug, Deserialize)]
struct RawReceiptNormalization {
    policy_id: String,
}

#[derive(Debug, Deserialize)]
struct RawReplay {
    program: String,
    arguments: Vec<String>,
    required_environment_variables: Vec<String>,
    working_tree_commit: String,
}

pub fn validate_oracle_harness_registry(bytes: &[u8]) -> Result<OracleHarnessRegistry, String> {
    let value = validate_document(bytes, MAX_REGISTRY_BYTES, REGISTRY_SCHEMA, "registry")?;
    let raw: RawRegistry =
        serde_json::from_value(value).map_err(|error| format!("oracle registry: {error}"))?;
    let mut slots = BTreeMap::new();
    for entry in raw.slots {
        let required_baselines = unique_set(entry.required_baselines, "registry baseline")?;
        let accepted_read_versions =
            unique_set(entry.adapter.accepted_read_versions, "adapter read version")?;
        let required_environment_variables = unique_set(
            entry.capture.required_environment_variables,
            "adapter environment variable",
        )?;
        let adapter_ready = entry.adapter.state == "ready";
        let fixture_ready = entry.fixture.state == "ready";
        let normalization_reviewed = entry.normalization.state == "reviewed";
        if fixture_ready != (entry.fixture.path.is_some() && entry.fixture.digest.is_some()) {
            return Err(format!(
                "oracle registry fixture readiness is inconsistent for {}",
                entry.slot_id
            ));
        }
        if (fixture_ready && entry.fixture.digest_rule == "pending")
            || (!fixture_ready && entry.fixture.digest_rule != "pending")
        {
            return Err(format!(
                "oracle registry fixture digest rule is inconsistent for {}",
                entry.slot_id
            ));
        }
        if adapter_ready
            != (entry.adapter.policy_path.is_some()
                && !accepted_read_versions.is_empty()
                && entry.adapter.read_rule == "exact-v1-plus-cer1701-envelope"
                && !entry.adapter.validator_argv.is_empty())
        {
            return Err(format!(
                "oracle registry adapter readiness is inconsistent for {}",
                entry.slot_id
            ));
        }
        if normalization_reviewed
            != (entry.normalization.policy_digest.is_some()
                && !entry.normalization.rules.is_empty())
        {
            return Err(format!(
                "oracle registry normalization review is inconsistent for {}",
                entry.slot_id
            ));
        }
        if normalization_reviewed
            && entry.normalization.policy_digest.as_deref()
                != Some(
                    normalization_policy_digest(
                        &entry.normalization.policy_id,
                        &entry.normalization.rules,
                    )?
                    .as_str(),
                )
        {
            return Err(format!(
                "oracle registry normalization digest drifted for {}",
                entry.slot_id
            ));
        }
        let slot = OracleHarnessSlot {
            required_baselines,
            fixture_digest: entry.fixture.digest,
            adapter_ready,
            accepted_read_versions,
            validator_argv: entry.adapter.validator_argv,
            required_environment_variables,
            normalization_reviewed,
            normalization_policy_id: entry.normalization.policy_id,
            normalization_policy_digest: entry.normalization.policy_digest,
            expected_cases: entry.bounds.expected_cases,
            max_raw_capture_bytes: entry.bounds.max_raw_capture_bytes,
            protected_attestation_required: entry.capture.protected_attestation_required,
        };
        if slots.insert(entry.slot_id.clone(), slot).is_some() {
            return Err(format!("duplicate oracle registry slot {}", entry.slot_id));
        }
    }
    let actual = slots.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = REQUIRED_SLOTS.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("oracle registry slot set is incomplete or unknown".into());
    }
    for (slot, expected_cases) in [
        ("cobol", 153),
        ("racf-saf", 48),
        ("dataset-vsam-ams", 36),
        ("jes2", 16),
        ("cics", 12),
    ] {
        if slots.get(slot).and_then(|entry| entry.expected_cases) != Some(expected_cases) {
            return Err(format!("oracle registry changed the {slot} campaign bound"));
        }
    }
    Ok(OracleHarnessRegistry {
        digest: digest(bytes),
        slots,
    })
}

pub fn validate_oracle_harness_receipt(
    receipt_bytes: &[u8],
    environment_bytes: &[u8],
    registry_bytes: &[u8],
    legacy_receipt_bytes: &[u8],
    expected: &OracleCandidateExpectation,
) -> Result<OracleHarnessValidation, String> {
    let registry = validate_oracle_harness_registry(registry_bytes)?;
    let environment = validate_environment(environment_bytes)?;
    let value = validate_document(receipt_bytes, MAX_RECEIPT_BYTES, RECEIPT_SCHEMA, "receipt")?;
    let receipt: RawReceipt =
        serde_json::from_value(value).map_err(|error| format!("oracle receipt: {error}"))?;
    if receipt.slot_id != expected.slot_id {
        return Err("oracle receipt slot differs from the requested harness".into());
    }
    let slot = registry
        .slots
        .get(&receipt.slot_id)
        .ok_or("oracle receipt names an unknown harness slot")?;
    if !slot.adapter_ready {
        return Err("oracle harness adapter is still pending".into());
    }
    if slot.fixture_digest.is_none() {
        return Err("oracle harness independent fixture is still pending".into());
    }
    if !slot.normalization_reviewed {
        return Err("oracle harness normalization policy is still pending review".into());
    }
    if !environment.available_slots.contains(&receipt.slot_id) {
        return Err("oracle environment does not advertise the required slot capability".into());
    }
    if !slot
        .required_baselines
        .is_subset(&environment.source_baselines)
    {
        return Err("oracle environment omits a required source baseline".into());
    }
    if receipt.bindings.environment_manifest_digest != environment.digest
        || receipt.bindings.harness_registry_digest != registry.digest
    {
        return Err("oracle receipt environment or registry binding drifted".into());
    }
    validate_candidate_bindings(&receipt, expected)?;
    if !slot
        .accepted_read_versions
        .contains(&receipt.compatibility.legacy_schema_version)
        || receipt.compatibility.read_rule != "exact-v1-plus-cer1701-envelope"
    {
        return Err("oracle receipt uses an unsupported historical read version".into());
    }
    if legacy_receipt_bytes.is_empty()
        || u64::try_from(legacy_receipt_bytes.len()).unwrap_or(u64::MAX)
            > slot.max_raw_capture_bytes
        || receipt.compatibility.legacy_receipt_digest != digest(legacy_receipt_bytes)
    {
        return Err("oracle receipt legacy artifact binding is missing or drifted".into());
    }
    validate_legacy_receipt_schema(
        &receipt.compatibility.legacy_schema_version,
        legacy_receipt_bytes,
        usize::try_from(slot.max_raw_capture_bytes).unwrap_or(usize::MAX),
    )?;
    if receipt.bindings.fixture_digest != slot.fixture_digest.as_deref().unwrap_or_default()
        || receipt.normalization.policy_id != slot.normalization_policy_id
        || receipt.bindings.normalization_policy_digest
            != slot
                .normalization_policy_digest
                .as_deref()
                .unwrap_or_default()
    {
        return Err("oracle receipt fixture or normalization identity drifted".into());
    }
    if slot.expected_cases != Some(receipt.execution.expected_cases)
        || receipt.execution.observed_cases != receipt.execution.expected_cases
        || receipt.execution.status != "pass"
    {
        return Err("oracle receipt case closure is incomplete or non-passing".into());
    }
    validate_replay(&receipt, slot)?;
    let kind = match receipt.origin.kind.as_str() {
        "licensed-ibm" => {
            if environment.class != "licensed-ibm"
                || !environment.licensed_execution
                || (slot.protected_attestation_required
                    && receipt.origin.protected_attestation_digest.is_none())
                || receipt.receipt_state != "campaign-captured"
                || receipt.compatibility.subsystem_validator_status != "pass"
                || zero_bound_identity(&receipt)
            {
                return Err(
                    "licensed oracle receipt lacks protected exact-candidate identity".into(),
                );
            }
            OracleHarnessValidationKind::CandidateBoundLicensedCapture
        }
        "local" | "synthetic" => {
            if environment.class != "synthetic"
                || environment.licensed_execution
                || receipt.origin.protected_attestation_digest.is_some()
                || receipt.receipt_state != "plumbing-valid"
                || receipt.compatibility.subsystem_validator_status != "synthetic-fixture-pass"
            {
                return Err("local or synthetic receipt cannot claim a licensed origin".into());
            }
            OracleHarnessValidationKind::PlumbingOnly
        }
        _ => return Err("oracle receipt origin is not supported".into()),
    };
    Ok(OracleHarnessValidation {
        slot_id: receipt.slot_id,
        receipt_digest: digest(receipt_bytes),
        kind,
    })
}

fn validate_environment(bytes: &[u8]) -> Result<EnvironmentSummary, String> {
    let value = validate_document(
        bytes,
        MAX_MANIFEST_BYTES,
        ENVIRONMENT_SCHEMA,
        "environment manifest",
    )?;
    if contains_sensitive_value(&value, None) {
        return Err("licensed environment manifest contains secret-looking material".into());
    }
    let class = required_text(&value, "environment_class")?.to_string();
    let licensed_execution = value["licensed_execution"]
        .as_bool()
        .ok_or("licensed environment flag is missing")?;
    let source_baselines = unique_json_strings(
        value["source_evidence"]
            .as_array()
            .ok_or("licensed environment source evidence is missing")?
            .iter()
            .map(|entry| &entry["baseline_id"]),
        "licensed environment baseline",
    )?;
    let available_slots = unique_json_strings(
        value["capabilities"]
            .as_array()
            .ok_or("licensed environment capabilities are missing")?
            .iter()
            .filter(|entry| entry["status"] == "available")
            .map(|entry| &entry["slot_id"]),
        "licensed environment slot",
    )?;
    Ok(EnvironmentSummary {
        digest: digest(bytes),
        class,
        licensed_execution,
        source_baselines,
        available_slots,
    })
}

fn validate_candidate_bindings(
    receipt: &RawReceipt,
    expected: &OracleCandidateExpectation,
) -> Result<(), String> {
    let artifacts = named_digest_map(&receipt.bindings.artifacts, "artifact")?;
    let catalogs = named_digest_map(&receipt.bindings.catalogs, "catalog")?;
    if receipt.bindings.source_commit != expected.source_commit
        || receipt.bindings.source_tree_digest != expected.source_tree_digest
        || artifacts != expected.artifacts
        || catalogs != expected.catalogs
        || receipt.bindings.conformance_spec_digest != expected.conformance_spec_digest
        || receipt.bindings.fixture_digest != expected.fixture_digest
        || receipt.bindings.oracle_adapter_digest != expected.oracle_adapter_digest
        || receipt.bindings.normalization_policy_digest != expected.normalization_policy_digest
    {
        return Err("oracle receipt candidate/source/artifact binding drifted".into());
    }
    Ok(())
}

fn validate_replay(receipt: &RawReceipt, slot: &OracleHarnessSlot) -> Result<(), String> {
    if receipt.replay.working_tree_commit != receipt.bindings.source_commit
        || receipt
            .replay
            .required_environment_variables
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            != slot.required_environment_variables
    {
        return Err("oracle receipt replay identity drifted".into());
    }
    let mut receipt_argv = vec![receipt.replay.program.clone()];
    receipt_argv.extend(receipt.replay.arguments.clone());
    let stable_prefix = slot.validator_argv.iter().take(3).collect::<Vec<_>>();
    if receipt_argv.iter().take(3).collect::<Vec<_>>() != stable_prefix {
        return Err(
            "oracle receipt replay command does not select the registered validator".into(),
        );
    }
    Ok(())
}

fn validate_document(
    bytes: &[u8],
    max_bytes: usize,
    schema_text: &str,
    label: &str,
) -> Result<Value, String> {
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(format!("{label} is empty or exceeds its byte bound"));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("{label}: {error}"))?;
    let schema: Value =
        serde_json::from_str(schema_text).map_err(|error| format!("{label} schema: {error}"))?;
    jsonschema::draft202012::meta::validate(&schema)
        .map_err(|error| format!("{label} schema is invalid: {error}"))?;
    let validator = jsonschema::draft202012::options()
        .offline()
        .build(&schema)
        .map_err(|error| format!("{label} schema did not compile: {error}"))?;
    validator
        .validate(&value)
        .map_err(|error| format!("{label} violates its schema: {error}"))?;
    Ok(value)
}

fn validate_legacy_receipt_schema(
    version: &str,
    bytes: &[u8],
    max_bytes: usize,
) -> Result<(), String> {
    let schema = match version {
        "mainframe-env.cobol-licensed-differential-receipt@1" => COBOL_RECEIPT_SCHEMA,
        "mainframe-env.racf-oracle-campaign@1" => RACF_RECEIPT_SCHEMA,
        "mainframe-env.dataset-oracle-receipt@1" => DATASET_RECEIPT_SCHEMA,
        "mainframe-env.jes-licensed-differential-receipt@1" => JES_RECEIPT_SCHEMA,
        "mainframe-env.cics-oracle-capture@1" => CICS_RECEIPT_SCHEMA,
        _ => return Err("oracle receipt historical schema version has no shared reader".into()),
    };
    validate_document(bytes, max_bytes, schema, "historical oracle receipt")?;
    Ok(())
}

fn named_digest_map(
    values: &[NamedDigest],
    label: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for value in values {
        if result
            .insert(value.id.clone(), value.digest.clone())
            .is_some()
        {
            return Err(format!("duplicate oracle receipt {label} identity"));
        }
    }
    Ok(result)
}

fn unique_set(values: Vec<String>, label: &str) -> Result<BTreeSet<String>, String> {
    let count = values.len();
    let result = values.into_iter().collect::<BTreeSet<_>>();
    if result.len() != count {
        return Err(format!("duplicate {label}"));
    }
    Ok(result)
}

fn unique_json_strings<'a>(
    values: impl Iterator<Item = &'a Value>,
    label: &str,
) -> Result<BTreeSet<String>, String> {
    let mut result = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .ok_or_else(|| format!("{label} is not a string"))?;
        if !result.insert(value.to_string()) {
            return Err(format!("duplicate {label}"));
        }
    }
    Ok(result)
}

fn required_text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("licensed environment {field} is missing"))
}

fn zero_bound_identity(receipt: &RawReceipt) -> bool {
    receipt
        .bindings
        .source_commit
        .bytes()
        .all(|byte| byte == b'0')
        || receipt
            .bindings
            .source_tree_digest
            .strip_prefix("sha256:")
            .is_some_and(|value| value.bytes().all(|byte| byte == b'0'))
}

fn contains_sensitive_value(value: &Value, key: Option<&str>) -> bool {
    if key.is_some_and(|name| {
        let upper = name.to_ascii_uppercase();
        [
            "PASSWORD",
            "PASSPHRASE",
            "PRIVATE_KEY",
            "TOKEN",
            "CREDENTIAL",
        ]
        .iter()
        .any(|marker| upper.contains(marker))
    }) {
        return true;
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
            (!value.starts_with("secretref:")
                && (upper.starts_with("SECRET:") || upper.starts_with("VAULT:")))
                || upper.contains("BEGIN PRIVATE KEY")
        }
        _ => false,
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn normalization_policy_digest(policy_id: &str, rules: &[Value]) -> Result<String, String> {
    let rules = serde_json::to_vec(rules).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.oracle-normalization-policy@1\0");
    for value in [policy_id.as_bytes(), rules.as_slice()] {
        digest.update(
            u64::try_from(value.len())
                .map_err(|_| "normalization policy is too large".to_string())?
                .to_be_bytes(),
        );
        digest.update(value);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENVIRONMENT: &[u8] =
        include_bytes!("../../../../conformance/0.17/fixtures/synthetic-environment.json");
    const LEGACY_CAPTURE: &[u8] =
        include_bytes!("../../../../conformance/0.17/fixtures/synthetic-cics-capture.json");
    const RECEIPT: &[u8] =
        include_bytes!("../../../../conformance/0.17/fixtures/synthetic-receipt.json");
    const REGISTRY: &[u8] = include_bytes!("../../../../conformance/0.17/oracles/harnesses.json");

    fn expectation() -> OracleCandidateExpectation {
        OracleCandidateExpectation {
            slot_id: "cics".into(),
            source_commit: "5ab706b1dd069e26db7cb9a2b66e921c9001fc39".into(),
            source_tree_digest:
                "sha256:c4e5c4d7d40d6c5618ae4cde2e478d18a531f690f4eebacf44cf16959f70c984".into(),
            artifacts: BTreeMap::from([(
                "synthetic-candidate-artifact".into(),
                "sha256:6a1c9843c16282e72cc4acfd454948e3c2ca56898510d21e12fb0c52e5e34a04".into(),
            )]),
            catalogs: BTreeMap::from([(
                "ibm-cics-ts-6x-2026-08-31".into(),
                "sha256:fccd2a8e5cc24dd08aeb32754daf14ed80e9f1b20b5d9e762a1b0cfe429ceeba".into(),
            )]),
            conformance_spec_digest:
                "sha256:082744eadcb2536894602beb72781cd40836101fb3ae2a99bcd2646082bfbfe9".into(),
            fixture_digest:
                "sha256:5cce953c42c1ddeca1166750f5b7a1bffb2765c4db40f289c64d8af1567b5a5b".into(),
            oracle_adapter_digest:
                "sha256:7bde0441642fff59ab28af46e815c7c1ba4f65e47ca1066aad7536a910b9873d".into(),
            normalization_policy_digest:
                "sha256:111f3304ea7c6c9f3a7d380756ebb7cc5180283b59fa4e38dc8f12d358cd2ce9".into(),
        }
    }

    #[test]
    fn checked_in_registry_is_complete_and_zero_credit() {
        let bytes = include_bytes!("../../../../conformance/0.17/oracles/harnesses.json");
        let registry = validate_oracle_harness_registry(bytes).unwrap();
        assert_eq!(registry.slot_ids().count(), 10);
        assert!(registry.digest().starts_with("sha256:"));
    }

    #[test]
    fn registry_rejects_a_pending_slot_relabelled_as_ready() {
        let bytes = include_bytes!("../../../../conformance/0.17/oracles/harnesses.json");
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        let db2 = value["slots"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["slot_id"] == "db2")
            .unwrap();
        db2["adapter"]["state"] = Value::String("ready".into());
        assert!(validate_oracle_harness_registry(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn synthetic_envelope_and_legacy_cics_reader_validate_with_zero_credit() {
        let validation = validate_oracle_harness_receipt(
            RECEIPT,
            ENVIRONMENT,
            REGISTRY,
            LEGACY_CAPTURE,
            &expectation(),
        )
        .unwrap();
        assert_eq!(validation.kind(), OracleHarnessValidationKind::PlumbingOnly);
        assert_eq!(validation.licensed_differential_credit(), 0);

        let capture: crate::CicsOracleCapture = serde_json::from_slice(LEGACY_CAPTURE).unwrap();
        let required = capture
            .observations
            .iter()
            .map(|observation| observation.scenario_id.clone())
            .collect::<BTreeSet<_>>();
        let observations = capture
            .observations
            .iter()
            .map(|observation| (observation.scenario_id.clone(), observation.clone()))
            .collect::<BTreeMap<_, _>>();
        let expected = crate::CicsOracleExpectation {
            family_id: None,
            family_manifest_digest: None,
            candidate_digest: &capture.candidate_digest,
            spec_digest: &capture.spec_digest,
            fixture_digest: &capture.fixture_digest,
            source_review_digest: &capture.source_review_digest,
            environment_manifest_digest: &capture.environment_manifest_digest,
            comparison_policy: &capture.comparison_policy,
            required_scenarios: &required,
            required_order: None,
            expected_observations: &observations,
        };
        let legacy = crate::import_cics_oracle_capture(LEGACY_CAPTURE, &expected, None).unwrap();
        assert_eq!(legacy.licensed_credit(), 0);
    }

    #[test]
    fn candidate_environment_legacy_and_case_mutations_fail_closed() {
        let mut wrong_candidate = expectation();
        wrong_candidate.source_commit = "9999999999999999999999999999999999999999".into();
        assert!(
            validate_oracle_harness_receipt(
                RECEIPT,
                ENVIRONMENT,
                REGISTRY,
                LEGACY_CAPTURE,
                &wrong_candidate,
            )
            .unwrap_err()
            .contains("candidate/source/artifact")
        );

        let mut wrong_environment: Value = serde_json::from_slice(ENVIRONMENT).unwrap();
        wrong_environment["manifest_id"] = Value::String("cer1701.changed-environment@1".into());
        assert!(
            validate_oracle_harness_receipt(
                RECEIPT,
                &serde_json::to_vec(&wrong_environment).unwrap(),
                REGISTRY,
                LEGACY_CAPTURE,
                &expectation(),
            )
            .unwrap_err()
            .contains("environment or registry binding")
        );

        let mut wrong_legacy = LEGACY_CAPTURE.to_vec();
        wrong_legacy.push(b'\n');
        assert!(
            validate_oracle_harness_receipt(
                RECEIPT,
                ENVIRONMENT,
                REGISTRY,
                &wrong_legacy,
                &expectation(),
            )
            .unwrap_err()
            .contains("legacy artifact binding")
        );

        let mut missing_case: Value = serde_json::from_slice(RECEIPT).unwrap();
        missing_case["execution"]["observed_cases"] = Value::from(11);
        assert!(
            validate_oracle_harness_receipt(
                &serde_json::to_vec(&missing_case).unwrap(),
                ENVIRONMENT,
                REGISTRY,
                LEGACY_CAPTURE,
                &expectation(),
            )
            .unwrap_err()
            .contains("case closure")
        );
    }

    #[test]
    fn pending_and_self_declared_licensed_slots_cannot_be_promoted() {
        let mut pending: Value = serde_json::from_slice(RECEIPT).unwrap();
        pending["slot_id"] = Value::String("db2".into());
        let mut pending_expectation = expectation();
        pending_expectation.slot_id = "db2".into();
        assert!(
            validate_oracle_harness_receipt(
                &serde_json::to_vec(&pending).unwrap(),
                ENVIRONMENT,
                REGISTRY,
                LEGACY_CAPTURE,
                &pending_expectation,
            )
            .unwrap_err()
            .contains("still pending")
        );

        let mut forged: Value = serde_json::from_slice(RECEIPT).unwrap();
        forged["receipt_state"] = Value::String("campaign-captured".into());
        forged["origin"]["kind"] = Value::String("licensed-ibm".into());
        forged["origin"]["protected_attestation_digest"] = Value::String(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        );
        for field in [
            "fixture_independence",
            "environment",
            "normalizer",
            "receipt",
        ] {
            forged["review"][field] = Value::String("accepted".into());
        }
        forged["compatibility"]["subsystem_validator_status"] = Value::String("pass".into());
        assert!(
            validate_oracle_harness_receipt(
                &serde_json::to_vec(&forged).unwrap(),
                ENVIRONMENT,
                REGISTRY,
                LEGACY_CAPTURE,
                &expectation(),
            )
            .unwrap_err()
            .contains("protected exact-candidate")
        );
    }
}
