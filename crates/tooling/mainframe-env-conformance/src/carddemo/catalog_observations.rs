//! Actual selected installed catalogs against frozen first-party/local fixture literals.
//! No IBM BMS/RACF parity, anonymous BMS fields, or full CSD installation is inferred.
use super::*;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};

const ORACLE: &str = include_str!("catalog_expected.json");
const POLICY_SOURCE: &str = include_str!("online_authorities.rs");

#[derive(Clone, Deserialize)]
struct ExpectedBytes {
    key: String,
    payload_base64: String,
}

#[derive(Clone, Deserialize)]
struct ExpectedProfile {
    class_name: String,
    resource: String,
    regular: bool,
    maximum: String,
}

#[derive(Clone, Deserialize)]
struct Oracle {
    schema_version: String,
    corpus_commit: String,
    source_pins: BTreeMap<String, String>,
    local_policy_sha256: String,
    maps: Vec<ExpectedBytes>,
    files: Vec<ExpectedBytes>,
    profiles: Vec<ExpectedProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstalledCatalog {
    maps: BTreeMap<String, ProviderStateRecord>,
    files: BTreeMap<String, ProviderStateRecord>,
    profiles: BTreeMap<String, serde_json::Value>,
}

// This proof is constructed only by an actual initial provider observation.
// It cannot bind until the same complete catalogs have been read after reopen.
pub(super) struct CatalogProof {
    oracle: Oracle,
    initial: InstalledCatalog,
    decisions: Vec<bool>,
    reopened: Option<InstalledCatalog>,
}

fn problem(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.installed_catalog", detail)
}

fn refuse(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value { Err(problem(detail)) } else { Ok(()) }
}

fn oracle() -> Result<Oracle, CorpusProblem> {
    let oracle: Oracle =
        serde_json::from_str(ORACLE).map_err(|error| problem(error.to_string()))?;
    refuse(
        oracle.schema_version != "mainframe-env.carddemo-selected-catalog-oracle@1"
            || oracle.corpus_commit != "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            || oracle.source_pins.len() != 35
            || oracle.maps.len() != 17
            || oracle.files.len() != 8
            || oracle.profiles.len() != 50
            || format!("{:x}", Sha256::digest(POLICY_SOURCE.as_bytes()))
                != oracle.local_policy_sha256,
        "frozen selected oracle or current local policy identity differs",
    )?;
    Ok(oracle)
}

pub(super) fn verify_pinned_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, digest) in oracle()?.source_pins {
        let bytes = read_corpus_file(corpus, &corpus.join(&path))?;
        refuse(
            format!("{:x}", Sha256::digest(&bytes)) != digest,
            &format!("catalog source differs: {path}"),
        )?;
    }
    Ok(())
}

fn expected_records(
    namespace: &str,
    expected: &[ExpectedBytes],
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let mut records = BTreeMap::new();
    for row in expected {
        let payload = base64::engine::general_purpose::STANDARD
            .decode(&row.payload_base64)
            .map_err(|error| problem(error.to_string()))?;
        refuse(
            payload.is_empty()
                || payload.len() > 16_384
                || records
                    .insert(
                        row.key.clone(),
                        ProviderStateRecord {
                            namespace: namespace.into(),
                            key: row.key.clone(),
                            version: 1,
                            payload,
                        },
                    )
                    .is_some(),
            "duplicate, empty or oversized frozen provider record",
        )?;
    }
    Ok(records)
}

fn observe_records(
    store: &dyn ProviderStateStore,
    namespace: &str,
    count: usize,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    // The extra row detects unoffered resources; never accept a truncated count.
    let rows = store
        .list_provider_state(namespace, count + 1)
        .map_err(|error| problem(error.to_string()))?;
    refuse(rows.len() != count, "installed provider inventory differs")?;
    let mut records = BTreeMap::new();
    for row in rows {
        refuse(
            row.namespace != namespace
                || row.version != 1
                || records.insert(row.key.clone(), row).is_some(),
            "installed namespace/version or unique key differs",
        )?;
    }
    Ok(records)
}

fn expected_profiles(
    oracle: &Oracle,
) -> Result<BTreeMap<String, serde_json::Value>, CorpusProblem> {
    let mut profiles = BTreeMap::new();
    for row in &oracle.profiles {
        let maximum = row.maximum.to_ascii_uppercase();
        refuse(
            !matches!(maximum.as_str(), "EXECUTE" | "UPDATE"),
            "unknown fixture access",
        )?;
        let mut entries = vec![serde_json::json!({
            "principal":"WEBADM", "access":maximum, "when":null, "audit":"none"
        })];
        if row.regular {
            entries.push(serde_json::json!({
                "principal":"WEBUSER", "access":maximum, "when":null, "audit":"none"
            }));
        }
        let key = format!("{}:{}", row.class_name, row.resource);
        let profile = serde_json::json!({
            "class":row.class_name, "name":row.resource, "generic":false,
            "owner":"WEBADM", "uacc":"NONE", "audit":"none", "security_level":0,
            "security_label":null, "categories":[], "access_list":entries, "segments":{},
            "version":if row.regular {3} else {2}
        });
        refuse(
            profiles.insert(key, profile).is_some(),
            "duplicate fixture profile",
        )?;
    }
    Ok(profiles)
}

fn observe_profiles(
    server: &ProductServer,
    store: &dyn ProviderStateStore,
    oracle: &Oracle,
) -> Result<BTreeMap<String, serde_json::Value>, CorpusProblem> {
    let expected = expected_profiles(oracle)?;
    for class in ["TCICSTRN", "FACILITY", "DATASET", "QUEUE"] {
        let names = oracle
            .profiles
            .iter()
            .filter(|row| row.class_name == class)
            .map(|row| row.resource.clone())
            .collect::<BTreeSet<_>>();
        let (actual, more) = server
            .racf_service()
            .list_profiles(class, None, names.len() + 1)
            .map_err(terminal_problem)?;
        refuse(
            more || actual.len() != names.len()
                || actual.into_iter().collect::<BTreeSet<_>>() != names,
            "complete profile-name inventory differs",
        )?;
    }
    let record = store
        .get_provider_state("racf-database-v2", "authority")
        .map_err(|error| problem(error.to_string()))?
        .ok_or_else(|| problem("actual RACF authority is absent"))?;
    refuse(
        record.namespace != "racf-database-v2"
            || record.key != "authority"
            || !record.payload.starts_with(b"MERACF2\0"),
        "RACF authority envelope differs",
    )?;
    let snapshot: serde_json::Value =
        serde_json::from_slice(&record.payload[8..]).map_err(|error| problem(error.to_string()))?;
    refuse(
        snapshot["generation"].as_u64() != Some(record.version),
        "RACF generation differs",
    )?;
    let profiles = snapshot["profiles"]
        .as_object()
        .ok_or_else(|| problem("actual RACF profiles are absent"))?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    // Retain only selected nonsecret profile metadata, never credentials/database bytes.
    refuse(
        profiles != expected,
        "complete selected profile owner/UACC/ACL/metadata differs",
    )?;
    Ok(profiles)
}

fn observe_catalog(
    server: &ProductServer,
    store: &dyn ProviderStateStore,
    oracle: &Oracle,
) -> Result<InstalledCatalog, CorpusProblem> {
    let actual = InstalledCatalog {
        maps: observe_records(store, "cics-map", 17)?,
        files: observe_records(store, "cics-file-alias", 8)?,
        profiles: observe_profiles(server, store, oracle)?,
    };
    refuse(
        actual.maps != expected_records("cics-map", &oracle.maps)?
            || actual.files != expected_records("cics-file-alias", &oracle.files)?,
        "full installed map or FILE binding bytes differ from independent literals",
    )?;
    Ok(actual)
}

fn expected_allow(row: &ExpectedProfile, principal: &str, intent: AccessIntent) -> bool {
    let offered = principal == "WEBADM" || principal == "WEBUSER" && row.regular;
    offered
        && match row.maximum.as_str() {
            "Execute" => intent == AccessIntent::Execute,
            "Update" => matches!(
                intent,
                AccessIntent::Execute | AccessIntent::Read | AccessIntent::Update
            ),
            _ => false,
        }
}

fn expected_decisions(oracle: &Oracle) -> Vec<bool> {
    let mut decisions = Vec::new();
    for row in &oracle.profiles {
        for principal in ["WEBADM", "WEBUSER", "NOBODY"] {
            for intent in [
                AccessIntent::Execute,
                AccessIntent::Read,
                AccessIntent::Update,
                AccessIntent::Control,
                AccessIntent::Alter,
            ] {
                decisions.push(expected_allow(row, principal, intent));
            }
        }
    }
    decisions.extend([false; 60]);
    decisions
}

fn observe_decisions(server: &ProductServer, oracle: &Oracle) -> Result<Vec<bool>, CorpusProblem> {
    let mut decisions = Vec::new();
    for row in &oracle.profiles {
        for principal in ["WEBADM", "WEBUSER", "NOBODY"] {
            for intent in [
                AccessIntent::Execute,
                AccessIntent::Read,
                AccessIntent::Update,
                AccessIntent::Control,
                AccessIntent::Alter,
            ] {
                let actual = server
                    .racf_service()
                    .authorize(
                        &PrincipalId::new(principal, InvocationLimits::default())
                            .expect("static principal"),
                        &row.class_name,
                        &ResourceName::new(&row.resource, 246)
                            .map_err(|_| problem("fixture resource invalid"))?,
                        intent,
                    )
                    .map_err(terminal_problem)?;
                let expected = if expected_allow(row, principal, intent) {
                    SecurityDecision::Allow
                } else {
                    SecurityDecision::Deny
                };
                refuse(
                    actual != expected,
                    "actual complete local intent matrix differs",
                )?;
                decisions.push(actual == SecurityDecision::Allow);
            }
        }
    }
    // Exact unoffered resources test default refusal without adding profiles.
    for (class, resource) in [
        ("TCICSTRN", "CICS.ZZ99"),
        ("FACILITY", "CICS.PROGRAM.UNOFFERED"),
        ("DATASET", "AWS.M2.CARDDEMO.UNOFFERED"),
        ("QUEUE", "CICS.TD.UNOFFERED"),
    ] {
        for principal in ["WEBADM", "WEBUSER", "NOBODY"] {
            for intent in [
                AccessIntent::Execute,
                AccessIntent::Read,
                AccessIntent::Update,
                AccessIntent::Control,
                AccessIntent::Alter,
            ] {
                let actual = server
                    .racf_service()
                    .authorize(
                        &PrincipalId::new(principal, InvocationLimits::default())
                            .expect("static principal"),
                        class,
                        &ResourceName::new(resource, 246).expect("static unoffered resource"),
                        intent,
                    )
                    .map_err(terminal_problem)?;
                refuse(
                    actual != SecurityDecision::Deny,
                    "unoffered resource received access",
                )?;
                decisions.push(false);
            }
        }
    }
    Ok(decisions)
}

pub(super) fn observe_initial(
    corpus: &Path,
    server: &ProductServer,
    store: &dyn ProviderStateStore,
) -> Result<CatalogProof, CorpusProblem> {
    verify_pinned_sources(corpus)?;
    let oracle = oracle()?;
    let initial = observe_catalog(server, store, &oracle)?;
    let decisions = observe_decisions(server, &oracle)?;
    // Authorize writes audit rows; selected profile/catalog bytes must remain exact.
    refuse(
        observe_catalog(server, store, &oracle)? != initial,
        "catalog changed while exercising local authorization matrix",
    )?;
    Ok(CatalogProof {
        oracle,
        initial,
        decisions,
        reopened: None,
    })
}

impl CatalogProof {
    pub(super) fn observe_reopened(
        &mut self,
        corpus: &Path,
        server: &ProductServer,
        store: &dyn ProviderStateStore,
    ) -> Result<(), CorpusProblem> {
        verify_pinned_sources(corpus)?;
        refuse(
            self.reopened.is_some(),
            "catalog reopen proof already supplied",
        )?;
        let reopened = observe_catalog(server, store, &self.oracle)?;
        refuse(
            reopened != self.initial,
            "complete reopened catalogs differ",
        )?;
        self.reopened = Some(reopened);
        Ok(())
    }

    fn require_complete(&self) -> Result<(), CorpusProblem> {
        refuse(
            self.reopened.as_ref() != Some(&self.initial)
                || self.decisions.len() != 810
                || self.decisions != expected_decisions(&self.oracle)
                || self.initial.maps != expected_records("cics-map", &self.oracle.maps)?
                || self.initial.files != expected_records("cics-file-alias", &self.oracle.files)?
                || self.initial.profiles != expected_profiles(&self.oracle)?,
            "actual complete initial matrix and reopened catalog proof are required",
        )
    }

    // Called after both checked drains and exact owned artifact cleanup, before
    // either this proof or the independently owned install proof is consumed.
    pub(super) fn bind_resource_catalog(
        &self,
        installation: &super::source_artifact_binding::InstallationProof,
        observations: &mut RouteObservations,
    ) -> Result<(), CorpusProblem> {
        self.require_complete()?;
        installation.require_resource_mappings()?;
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "resource catalog",
            false,
            || Ok(problem("resource catalog refused")),
        )
    }

    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        self.require_complete()?;
        for requirement in ["map catalog", "RACF profiles"] {
            observations.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J01",
                requirement,
                false,
                || Ok(problem("installed catalog refused")),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "catalog_observations_tests.rs"]
mod tests;
