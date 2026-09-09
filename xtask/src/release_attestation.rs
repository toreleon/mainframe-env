use crate::release_licenses::ProductionGraph;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::GzDecoder;
use in_toto_attestation::{
    predicates::provenance::v1::provenance::Provenance as TypedProvenance,
    v1::statement::Statement as TypedStatement, validator::MetadataValidator,
};
use protobuf_json_mapping::parse_from_str;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(crate) const DSSE_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
pub(crate) const SLSA_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";
const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
const POLICY_SCHEMA: &str = "mainframe-env.release-attestation-policy@1";
const MAX_SCHEMA_BYTES: usize = 512 * 1024;
const MAX_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PRIVATE_KEY_BYTES: u64 = 4 * 1024;
const CYCLONEDX_SCHEMA_SHA256: &str =
    "18f57f7482593bad9f21b4feed09084640cbeff419d62ad5090c5ceccca5b37d";
const CYCLONEDX_JSF_SHA256: &str =
    "8bae002c25e723db7ee1f26afde680ae1a2b1a8f6b4b4b0fd65dc3becb090aae";
const CYCLONEDX_SPDX_SHA256: &str =
    "c41917196639055e9f9670811bac23ef777732144f3ff5a2f39686f61580dbe6";
const CYCLONEDX_SCHEMA_B64: &str =
    include_str!("../../conformance/standards/cyclonedx/1.6/bom-1.6.schema.json.gz.b64");
const CYCLONEDX_JSF_B64: &str =
    include_str!("../../conformance/standards/cyclonedx/1.6/jsf-0.82.schema.json.gz.b64");
const CYCLONEDX_SPDX_B64: &str =
    include_str!("../../conformance/standards/cyclonedx/1.6/spdx.schema.json.gz.b64");

#[derive(Clone, Debug)]
pub(crate) struct ProvenanceMaterials<'a> {
    pub(crate) version: &'a str,
    pub(crate) target: &'a str,
    pub(crate) source_commit: &'a str,
    pub(crate) source_digest: &'a str,
    pub(crate) server_digest: &'a str,
    pub(crate) cli_digest: &'a str,
    pub(crate) manifest_digest: &'a str,
    pub(crate) sbom_digest: &'a str,
    pub(crate) build_inputs_digest: &'a str,
    pub(crate) licenses_digest: &'a str,
}

#[derive(Clone, Debug)]
struct TrustPolicy {
    build_type: String,
    builder_id: String,
    keyid: String,
    payload_type: String,
    predicate_type: String,
    public_key: Vec<u8>,
    statement_type: String,
}

pub(crate) fn validate_repository_policy(root: &Path) -> Result<(), String> {
    trust_policy(root)?;
    for (encoded, digest) in [
        (CYCLONEDX_SCHEMA_B64, CYCLONEDX_SCHEMA_SHA256),
        (CYCLONEDX_JSF_B64, CYCLONEDX_JSF_SHA256),
        (CYCLONEDX_SPDX_B64, CYCLONEDX_SPDX_SHA256),
    ] {
        decoded_schema(encoded, digest)?;
    }
    Ok(())
}

pub(crate) fn generate_sbom(
    graph: &ProductionGraph,
    target: &str,
    version: &str,
    phase_base: &str,
    lock_digest: &str,
) -> Result<Value, String> {
    let references = graph
        .packages
        .iter()
        .map(|(id, package)| (id.clone(), package_reference(package)))
        .collect::<BTreeMap<_, _>>();
    if references.values().collect::<BTreeSet<_>>().len() != references.len() {
        return Err("stable SBOM component references collide".into());
    }
    let mut components = graph
        .packages
        .iter()
        .map(|(id, package)| {
            let reference = references
                .get(id)
                .ok_or_else(|| format!("SBOM reference missing for {id}"))?;
            let mut component = json!({
                "type":if package.has_binary {"application"} else {"library"},
                "bom-ref":reference,
                "name":package.name,
                "version":package.version,
                "purl":format!("pkg:cargo/{}@{}", package.name, package.version),
                "properties":[
                    {"name":"mainframe-env:component-identity-sha256","value":component_identity(package)},
                    {"name":"mainframe-env:workspace-member","value":package.workspace.to_string()}
                ]
            });
            if let Some(license) = &package.license {
                component["licenses"] = json!([{"expression":license}]);
            }
            if let Some(source) = &package.source {
                component["properties"]
                    .as_array_mut()
                    .ok_or("SBOM component properties are missing")?
                    .push(json!({"name":"mainframe-env:cargo-source","value":source}));
            }
            Ok(component)
        })
        .collect::<Result<Vec<_>, String>>()?;
    components.sort_by(|left, right| left["bom-ref"].as_str().cmp(&right["bom-ref"].as_str()));
    let product_reference = format!("urn:mainframe-env:release:{version}:{target}");
    let mut root_references = graph
        .roots
        .iter()
        .map(|id| references[id].clone())
        .collect::<Vec<_>>();
    root_references.sort();
    let mut dependencies = vec![json!({
        "ref":product_reference,
        "dependsOn":root_references
    })];
    for (id, package) in &graph.packages {
        let mut dependency_references = package
            .dependencies
            .iter()
            .map(|id| references[id].clone())
            .collect::<Vec<_>>();
        dependency_references.sort();
        dependencies.push(json!({
            "ref":references[id],
            "dependsOn":dependency_references
        }));
    }
    dependencies.sort_by(|left, right| left["ref"].as_str().cmp(&right["ref"].as_str()));
    let sbom = json!({
        "$schema":"http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat":"CycloneDX",
        "specVersion":"1.6",
        "version":1,
        "metadata":{
            "lifecycles":[{"phase":"build"}],
            "component":{
                "type":"application",
                "bom-ref":product_reference,
                "name":"mainframe-env",
                "version":version
            },
            "properties":[
                {"name":"mainframe-env:phase-base","value":phase_base},
                {"name":"mainframe-env:cargo-lock-sha256","value":lock_digest},
                {"name":"mainframe-env:release-profile","value":"core-server"},
                {"name":"mainframe-env:target","value":target},
                {"name":"mainframe-env:dependency-scope","value":"target-filtered-normal"}
            ]
        },
        "components":components,
        "dependencies":dependencies
    });
    validate_sbom(&sbom, graph, target, version)?;
    Ok(sbom)
}

pub(crate) fn validate_sbom(
    sbom: &Value,
    graph: &ProductionGraph,
    target: &str,
    version: &str,
) -> Result<(), String> {
    validate_official_cyclonedx_schema(sbom)?;
    let components = sbom["components"]
        .as_array()
        .ok_or("CycloneDX components are missing")?;
    if components.len() != graph.packages.len() {
        return Err("CycloneDX component count differs from the production closure".into());
    }
    let expected_references = graph
        .packages
        .values()
        .map(package_reference)
        .collect::<BTreeSet<_>>();
    let actual_references = components
        .iter()
        .map(|component| {
            component["bom-ref"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "CycloneDX component has no bom-ref".to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual_references != expected_references {
        return Err("CycloneDX components differ from the production closure".into());
    }
    let dependencies = sbom["dependencies"]
        .as_array()
        .ok_or("CycloneDX dependency graph is missing")?;
    if dependencies.len() != graph.packages.len() + 1 {
        return Err("CycloneDX dependency graph is incomplete".into());
    }
    let actual_edges = dependencies
        .iter()
        .map(|dependency| {
            let reference = dependency["ref"]
                .as_str()
                .ok_or("CycloneDX dependency has no ref")?;
            let depends_on = dependency["dependsOn"]
                .as_array()
                .ok_or("CycloneDX dependency has no dependsOn")?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "CycloneDX dependency ref is not text".to_string())
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            Ok((reference.to_string(), depends_on))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let product_reference = format!("urn:mainframe-env:release:{version}:{target}");
    let expected_roots = graph
        .roots
        .iter()
        .map(|id| {
            graph
                .packages
                .get(id)
                .map(package_reference)
                .ok_or_else(|| format!("production graph omits root {id}"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual_edges.get(&product_reference) != Some(&expected_roots) {
        return Err("CycloneDX product roots differ from the production roots".into());
    }
    for package in graph.packages.values() {
        let expected = package
            .dependencies
            .iter()
            .map(|id| {
                graph
                    .packages
                    .get(id)
                    .map(package_reference)
                    .ok_or_else(|| format!("production graph omits dependency {id}"))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if actual_edges.get(&package_reference(package)) != Some(&expected) {
            return Err(format!(
                "CycloneDX dependency edges differ for {} {}",
                package.name, package.version
            ));
        }
    }
    if sbom["metadata"]["component"]["version"] != Value::String(version.into())
        || !sbom["metadata"]["properties"]
            .as_array()
            .is_some_and(|properties| {
                properties.iter().any(|property| {
                    property["name"] == Value::String("mainframe-env:target".into())
                        && property["value"] == Value::String(target.into())
                })
            })
    {
        return Err("CycloneDX target or product identity drifted".into());
    }
    Ok(())
}

fn component_identity(package: &crate::release_licenses::ProductionPackage) -> String {
    let source = package.source.as_deref().unwrap_or(if package.workspace {
        "mainframe-env-workspace"
    } else {
        "local-path"
    });
    sha256(format!("{}\0{}\0{source}", package.name, package.version).as_bytes())
}

fn package_reference(package: &crate::release_licenses::ProductionPackage) -> String {
    format!(
        "urn:mainframe-env:cargo:sha256:{}",
        component_identity(package)
    )
}

fn validate_official_cyclonedx_schema(instance: &Value) -> Result<(), String> {
    let schema = decoded_schema(CYCLONEDX_SCHEMA_B64, CYCLONEDX_SCHEMA_SHA256)?;
    let jsf = decoded_schema(CYCLONEDX_JSF_B64, CYCLONEDX_JSF_SHA256)?;
    let spdx = decoded_schema(CYCLONEDX_SPDX_B64, CYCLONEDX_SPDX_SHA256)?;
    let registry = jsonschema::Registry::new()
        .add("http://cyclonedx.org/schema/jsf-0.82.schema.json", &jsf)
        .map_err(|error| format!("register official CycloneDX JSF schema: {error}"))?
        .add("http://cyclonedx.org/schema/spdx.schema.json", &spdx)
        .map_err(|error| format!("register official CycloneDX SPDX schema: {error}"))?
        .prepare()
        .map_err(|error| format!("prepare official CycloneDX schema registry: {error}"))?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .with_registry(&registry)
        .should_validate_formats(true)
        .should_ignore_unknown_formats(true)
        .build(&schema)
        .map_err(|error| format!("compile official CycloneDX 1.6 schema: {error}"))?;
    validator
        .validate(instance)
        .map_err(|error| format!("release SBOM violates official CycloneDX 1.6 schema: {error}"))
}

fn decoded_schema(encoded: &str, expected_digest: &str) -> Result<Value, String> {
    let compact = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let compressed = BASE64
        .decode(compact)
        .map_err(|error| format!("decode retained official schema: {error}"))?;
    let mut bytes = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take((MAX_SCHEMA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("decompress retained official schema: {error}"))?;
    if bytes.len() > MAX_SCHEMA_BYTES || sha256(&bytes) != expected_digest {
        return Err("retained official schema digest or size differs".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("retained official schema JSON: {error}"))
}

pub(crate) fn release_invocation_id() -> Result<String, String> {
    let value = env::var("MAINFRAME_ENV_RELEASE_INVOCATION_ID")
        .or_else(|_| env::var("BUILD_URL"))
        .map_err(|_| {
            "release signing requires MAINFRAME_ENV_RELEASE_INVOCATION_ID or Jenkins BUILD_URL"
                .to_string()
        })?;
    validate_release_invocation_id_value(&value)?;
    Ok(value)
}

fn validate_release_invocation_id_value(value: &str) -> Result<(), String> {
    validate_type_uri(value, "release invocation ID")?;
    if value.contains("mainframe-env-v") && value.ends_with("-local") {
        return Err("release invocation ID is repeatable rather than run-unique".into());
    }
    Ok(())
}

pub(crate) fn provenance_statement(
    root: &Path,
    materials: &ProvenanceMaterials<'_>,
    invocation_id: &str,
) -> Result<Value, String> {
    validate_release_invocation_id_value(invocation_id)?;
    if !is_lower_hex(materials.source_commit, 40) {
        return Err("release provenance source commit is invalid".into());
    }
    for digest in [
        materials.source_digest,
        materials.server_digest,
        materials.cli_digest,
        materials.manifest_digest,
        materials.sbom_digest,
        materials.build_inputs_digest,
        materials.licenses_digest,
    ] {
        if !is_lower_hex(digest, 64) {
            return Err("release provenance input digest is invalid".into());
        }
    }
    let policy = trust_policy(root)?;
    let repository = "https://github.com/toreleon/mainframe-env";
    let blob = format!("{repository}/blob/{}", materials.source_commit);
    let source_uri = format!("git+{repository}@{}", materials.source_commit);
    let resource = |relative: &str| -> Result<Value, String> {
        Ok(json!({
            "uri":format!("{blob}/{relative}"),
            "digest":{"sha256":file_digest(&root.join(relative))?}
        }))
    };
    let jenkins_lock: Value = serde_json::from_slice(
        &fs::read(root.join("tools/jenkins/controller-plugins.lock.json"))
            .map_err(|error| format!("read Jenkins input lock: {error}"))?,
    )
    .map_err(|error| format!("Jenkins input lock JSON: {error}"))?;
    let jenkins_version = jenkins_lock["controller"]["version"]
        .as_str()
        .ok_or("Jenkins input lock omits controller version")?;
    let jenkins_digest = jenkins_lock["controller"]["sha256"]
        .as_str()
        .ok_or("Jenkins input lock omits controller digest")?;
    if !is_lower_hex(jenkins_digest, 64) {
        return Err("Jenkins controller digest is invalid".into());
    }
    let statement = json!({
        "_type":STATEMENT_TYPE,
        "subject":[
            {"name":"bin/mainframe-env-server","digest":{"sha256":materials.server_digest}},
            {"name":"bin/mainframe-env","digest":{"sha256":materials.cli_digest}}
        ],
        "predicateType":SLSA_PREDICATE_TYPE,
        "predicate":{
            "buildDefinition":{
                "buildType":policy.build_type,
                "externalParameters":{
                    "source":source_uri,
                    "version":materials.version,
                    "target":materials.target,
                    "profile":"release",
                    "allFeatures":true,
                    "locked":true
                },
                "internalParameters":{
                    "invocationUri":invocation_id,
                    "ciInputLockSha256":file_digest(&root.join("tools/ci-inputs.lock.json"))?
                },
                "resolvedDependencies":[
                    {"uri":source_uri,"digest":{"gitCommit":materials.source_commit,"sha256":materials.source_digest}},
                    resource("Cargo.lock")?,
                    resource("rust-toolchain.toml")?,
                    resource("config/mainframe-env.toml")?,
                    resource("crates/stores/mainframe-env-store/migrations/sqlite/0001-durable-state.sql")?,
                    resource("crates/stores/mainframe-env-store/migrations/postgres/0001-durable-state.sql")?,
                    resource("tools/build_release_binaries.sh")?,
                    resource("conformance/standards/cyclonedx/1.6/bom-1.6.schema.json.gz.b64")?,
                    resource("conformance/standards/cyclonedx/1.6/jsf-0.82.schema.json.gz.b64")?,
                    resource("conformance/standards/cyclonedx/1.6/spdx.schema.json.gz.b64")?
                ]
            },
            "runDetails":{
                "builder":{
                    "id":policy.builder_id,
                    "version":{"jenkins":jenkins_version,"mainframe-env-builder":"1"},
                    "builderDependencies":[
                        {"uri":jenkins_lock["controller"]["url"],"digest":{"sha256":jenkins_digest}},
                        resource("Jenkinsfile")?,
                        resource("tools/ci-inputs.lock.json")?,
                        resource("tools/jenkins/controller-plugins.lock.json")?,
                        resource("config/release-attestation-policy.json")?
                    ]
                },
                "metadata":{"invocationId":invocation_id},
                "byproducts":[
                    {"uri":format!("urn:mainframe-env:release:{}:{}:manifest",materials.version,materials.target),"digest":{"sha256":materials.manifest_digest}},
                    {"uri":format!("urn:mainframe-env:release:{}:{}:sbom",materials.version,materials.target),"digest":{"sha256":materials.sbom_digest},"mediaType":"application/vnd.cyclonedx+json; version=1.6"},
                    {"uri":format!("urn:mainframe-env:release:{}:{}:build-inputs",materials.version,materials.target),"digest":{"sha256":materials.build_inputs_digest}},
                    {"uri":format!("urn:mainframe-env:release:{}:{}:licenses",materials.version,materials.target),"digest":{"sha256":materials.licenses_digest}}
                ]
            }
        }
    });
    validate_provenance_statement(&statement, &policy)?;
    Ok(statement)
}

pub(crate) fn sign_statement(root: &Path, statement: &Value) -> Result<Vec<u8>, String> {
    let policy = trust_policy(root)?;
    validate_provenance_statement(statement, &policy)?;
    let private_path = env::var_os("MAINFRAME_ENV_RELEASE_SIGNING_KEY")
        .map(PathBuf::from)
        .ok_or("release generation requires MAINFRAME_ENV_RELEASE_SIGNING_KEY")?;
    if !private_path.is_absolute() {
        return Err("release signing key path must be absolute".into());
    }
    let metadata = fs::symlink_metadata(&private_path)
        .map_err(|error| format!("inspect release signing key: {error}"))?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_PRIVATE_KEY_BYTES
    {
        return Err("release signing key is not a bounded regular file".into());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err("release signing key permissions must exclude group and other access".into());
    }
    let pkcs8 =
        fs::read(&private_path).map_err(|error| format!("read release signing key: {error}"))?;
    sign_with_pkcs8(statement, &policy, &pkcs8)
}

fn sign_with_pkcs8(
    statement: &Value,
    policy: &TrustPolicy,
    pkcs8: &[u8],
) -> Result<Vec<u8>, String> {
    let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(pkcs8)
        .map_err(|_| "release signing key is not Ed25519 PKCS#8")?;
    if key_pair.public_key().as_ref() != policy.public_key {
        return Err("release signing key is not trusted by the repository policy".into());
    }
    let payload = pretty_json(statement)?;
    let signature = key_pair.sign(&pae(&policy.payload_type, &payload));
    pretty_json(&json!({
        "payloadType":policy.payload_type,
        "payload":BASE64.encode(&payload),
        "signatures":[{"keyid":policy.keyid,"sig":BASE64.encode(signature.as_ref())}]
    }))
}

pub(crate) fn verify_envelope(root: &Path, bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() || bytes.len() > MAX_ENVELOPE_BYTES {
        return Err("release DSSE envelope is empty or too large".into());
    }
    let envelope: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("release DSSE JSON: {error}"))?;
    let policy = trust_policy(root)?;
    verify_envelope_value(&envelope, &policy)
}

fn verify_envelope_value(envelope: &Value, policy: &TrustPolicy) -> Result<Value, String> {
    let object = envelope
        .as_object()
        .ok_or("release DSSE envelope is not an object")?;
    if object.keys().cloned().collect::<BTreeSet<_>>()
        != ["payload", "payloadType", "signatures"]
            .into_iter()
            .map(str::to_string)
            .collect()
    {
        return Err("release DSSE envelope fields differ".into());
    }
    let payload_type = envelope["payloadType"]
        .as_str()
        .ok_or("release DSSE payloadType is missing")?;
    if payload_type != policy.payload_type {
        return Err("release DSSE payloadType is not trusted".into());
    }
    let signatures = envelope["signatures"]
        .as_array()
        .ok_or("release DSSE signatures are missing")?;
    if signatures.len() != 1 || signatures[0]["keyid"].as_str() != Some(policy.keyid.as_str()) {
        return Err("release DSSE signer is not the trusted signer".into());
    }
    let signature_object = signatures[0]
        .as_object()
        .ok_or("release DSSE signature is not an object")?;
    if signature_object.keys().cloned().collect::<BTreeSet<_>>()
        != ["keyid", "sig"].into_iter().map(str::to_string).collect()
    {
        return Err("release DSSE signature fields differ".into());
    }
    let payload = BASE64
        .decode(
            envelope["payload"]
                .as_str()
                .ok_or("release DSSE payload is missing")?,
        )
        .map_err(|error| format!("release DSSE payload base64: {error}"))?;
    if payload.is_empty() || payload.len() > MAX_ENVELOPE_BYTES {
        return Err("release DSSE payload is empty or too large".into());
    }
    let signature = BASE64
        .decode(
            signatures[0]["sig"]
                .as_str()
                .ok_or("release DSSE signature is missing")?,
        )
        .map_err(|error| format!("release DSSE signature base64: {error}"))?;
    UnparsedPublicKey::new(&ED25519, &policy.public_key)
        .verify(&pae(payload_type, &payload), &signature)
        .map_err(|_| "release DSSE signature verification failed")?;
    let statement: Value = serde_json::from_slice(&payload)
        .map_err(|error| format!("signed provenance statement JSON: {error}"))?;
    validate_provenance_statement(&statement, policy)?;
    Ok(statement)
}

pub(crate) fn invocation_id(statement: &Value) -> Result<&str, String> {
    let value = statement["predicate"]["runDetails"]["metadata"]["invocationId"]
        .as_str()
        .ok_or("signed provenance omits invocationId")?;
    validate_release_invocation_id_value(value)?;
    Ok(value)
}

fn validate_provenance_statement(statement: &Value, policy: &TrustPolicy) -> Result<(), String> {
    exact_object_keys(
        statement,
        &["_type", "predicate", "predicateType", "subject"],
        "in-toto Statement",
    )?;
    exact_object_keys(
        &statement["predicate"],
        &["buildDefinition", "runDetails"],
        "SLSA predicate",
    )?;
    let build_value = &statement["predicate"]["buildDefinition"];
    let run_value = &statement["predicate"]["runDetails"];
    exact_object_keys(
        build_value,
        &[
            "buildType",
            "externalParameters",
            "internalParameters",
            "resolvedDependencies",
        ],
        "SLSA buildDefinition",
    )?;
    exact_object_keys(
        &build_value["externalParameters"],
        &[
            "allFeatures",
            "locked",
            "profile",
            "source",
            "target",
            "version",
        ],
        "SLSA externalParameters",
    )?;
    exact_object_keys(
        &build_value["internalParameters"],
        &["ciInputLockSha256", "invocationUri"],
        "SLSA internalParameters",
    )?;
    exact_object_keys(
        run_value,
        &["builder", "byproducts", "metadata"],
        "SLSA runDetails",
    )?;
    exact_object_keys(
        &run_value["builder"],
        &["builderDependencies", "id", "version"],
        "SLSA builder",
    )?;
    exact_object_keys(&run_value["metadata"], &["invocationId"], "SLSA metadata")?;
    let subjects = statement["subject"]
        .as_array()
        .ok_or("SLSA subjects are missing")?;
    let subject_names = subjects
        .iter()
        .filter_map(|subject| subject["name"].as_str())
        .collect::<BTreeSet<_>>();
    if subjects.len() != 2
        || subject_names
            != ["bin/mainframe-env", "bin/mainframe-env-server"]
                .into_iter()
                .collect()
        || build_value["resolvedDependencies"].as_array().map(Vec::len) != Some(10)
        || run_value["builder"]["builderDependencies"]
            .as_array()
            .map(Vec::len)
            != Some(5)
        || run_value["byproducts"].as_array().map(Vec::len) != Some(4)
        || build_value["externalParameters"]["profile"] != Value::String("release".into())
        || build_value["externalParameters"]["allFeatures"] != Value::Bool(true)
        || build_value["externalParameters"]["locked"] != Value::Bool(true)
    {
        return Err("signed provenance build interface or resource cardinality differs".into());
    }
    let byproduct_suffixes = run_value["byproducts"]
        .as_array()
        .expect("checked above")
        .iter()
        .filter_map(|descriptor| descriptor["uri"].as_str())
        .filter_map(|uri| uri.rsplit(':').next())
        .collect::<BTreeSet<_>>();
    if byproduct_suffixes
        != ["build-inputs", "licenses", "manifest", "sbom"]
            .into_iter()
            .collect()
    {
        return Err("signed provenance byproduct set differs".into());
    }
    let invocation_parameter = build_value["internalParameters"]["invocationUri"]
        .as_str()
        .ok_or("SLSA internal invocation URI is missing")?;
    let invocation_metadata = run_value["metadata"]["invocationId"]
        .as_str()
        .ok_or("SLSA invocationId is missing")?;
    if invocation_parameter != invocation_metadata
        || !is_lower_hex(
            build_value["internalParameters"]["ciInputLockSha256"]
                .as_str()
                .unwrap_or_default(),
            64,
        )
    {
        return Err("signed provenance invocation or CI lock identity differs".into());
    }
    let text = serde_json::to_string(statement).map_err(|error| error.to_string())?;
    let typed: TypedStatement = parse_from_str(&text)
        .map_err(|error| format!("official in-toto Statement schema: {error}"))?;
    typed
        .validate_fields()
        .map_err(|error| format!("official in-toto Statement validation: {error}"))?;
    let predicate_text =
        serde_json::to_string(&statement["predicate"]).map_err(|error| error.to_string())?;
    let provenance: TypedProvenance = parse_from_str(&predicate_text)
        .map_err(|error| format!("official SLSA provenance v1 schema: {error}"))?;
    let build = provenance
        .build_definition
        .as_ref()
        .ok_or("SLSA provenance omits buildDefinition")?;
    let run = provenance
        .run_details
        .as_ref()
        .ok_or("SLSA provenance omits runDetails")?;
    let builder = run
        .builder
        .as_ref()
        .ok_or("SLSA provenance omits builder")?;
    let metadata = run
        .metadata
        .as_ref()
        .ok_or("SLSA provenance omits metadata")?;
    if typed.type_ != policy.statement_type
        || typed.predicate_type != policy.predicate_type
        || build.build_type != policy.build_type
        || builder.id != policy.builder_id
        || build.external_parameters.is_none()
        || metadata.invocation_id.is_empty()
    {
        return Err("signed provenance differs from the trusted schema and identity policy".into());
    }
    validate_type_uri(&build.build_type, "SLSA buildType")?;
    validate_type_uri(&builder.id, "SLSA builder.id")?;
    validate_release_invocation_id_value(&metadata.invocation_id)?;
    for descriptor in typed
        .subject
        .iter()
        .chain(build.resolved_dependencies.iter())
        .chain(builder.builder_dependencies.iter())
        .chain(run.byproducts.iter())
    {
        descriptor
            .validate_fields()
            .map_err(|error| format!("official in-toto ResourceDescriptor validation: {error}"))?;
        if !descriptor.uri.is_empty() {
            validate_type_uri(&descriptor.uri, "in-toto resource URI")?;
        }
    }
    Ok(())
}

fn exact_object_keys(value: &Value, expected: &[&str], field: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{field} is not an object"))?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(format!("{field} fields differ"));
    }
    Ok(())
}

fn trust_policy(root: &Path) -> Result<TrustPolicy, String> {
    let path = root.join("config/release-attestation-policy.json");
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.is_empty() || bytes.len() > 16 * 1024 {
        return Err("release attestation policy is empty or too large".into());
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("release attestation policy JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or("release attestation policy is not an object")?;
    let expected = [
        "algorithm",
        "build_type",
        "builder_id",
        "keyid",
        "payload_type",
        "predicate_type",
        "public_key_base64",
        "schema_version",
        "slsa_build_level",
        "statement_type",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    if object.keys().cloned().collect::<BTreeSet<_>>() != expected
        || value["schema_version"] != Value::String(POLICY_SCHEMA.into())
        || value["algorithm"] != Value::String("Ed25519".into())
        || value["payload_type"] != Value::String(DSSE_PAYLOAD_TYPE.into())
        || value["predicate_type"] != Value::String(SLSA_PREDICATE_TYPE.into())
        || value["statement_type"] != Value::String(STATEMENT_TYPE.into())
        || value["slsa_build_level"].as_u64() != Some(1)
    {
        return Err("release attestation policy fields are invalid".into());
    }
    let text = |name: &str| {
        value[name]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("release attestation policy omits {name}"))
    };
    let public_key = BASE64
        .decode(text("public_key_base64")?)
        .map_err(|error| format!("release public key base64: {error}"))?;
    if public_key.len() != 32 {
        return Err("release Ed25519 public key must be 32 bytes".into());
    }
    let keyid = text("keyid")?;
    if keyid != format!("sha256:{}", sha256(&public_key)) {
        return Err("release keyid does not identify the trusted public key".into());
    }
    let build_type = text("build_type")?;
    let builder_id = text("builder_id")?;
    validate_type_uri(&build_type, "trusted buildType")?;
    validate_type_uri(&builder_id, "trusted builder.id")?;
    Ok(TrustPolicy {
        build_type,
        builder_id,
        keyid,
        payload_type: text("payload_type")?,
        predicate_type: text("predicate_type")?,
        public_key,
        statement_type: text("statement_type")?,
    })
}

fn validate_type_uri(value: &str, field: &str) -> Result<(), String> {
    let Some((scheme, remainder)) = value.split_once(':') else {
        return Err(format!("{field} is not an absolute URI"));
    };
    if value.len() > 2048
        || remainder.is_empty()
        || !scheme.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'A'..=b'Z' => true,
            b'0'..=b'9' | b'+' | b'-' | b'.' => index > 0,
            _ => false,
        })
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(format!("{field} is not a bounded absolute URI"));
    }
    Ok(())
}

fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    format!(
        "DSSEv1 {} {payload_type} {} ",
        payload_type.len(),
        payload.len()
    )
    .into_bytes()
    .into_iter()
    .chain(payload.iter().copied())
    .collect()
}

fn pretty_json(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn file_digest(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(sha256(&bytes))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    fn test_policy(pkcs8: &[u8]) -> TrustPolicy {
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8).expect("test key");
        let public_key = pair.public_key().as_ref().to_vec();
        TrustPolicy {
            build_type: "https://example.test/build/v1".into(),
            builder_id: "https://example.test/builder/v1".into(),
            keyid: format!("sha256:{}", sha256(&public_key)),
            payload_type: DSSE_PAYLOAD_TYPE.into(),
            predicate_type: SLSA_PREDICATE_TYPE.into(),
            public_key,
            statement_type: STATEMENT_TYPE.into(),
        }
    }

    fn test_statement(policy: &TrustPolicy) -> Value {
        let digest = || json!({"sha256":"a".repeat(64)});
        let resource = |uri: &str| json!({"uri":uri,"digest":digest()});
        json!({
            "_type":STATEMENT_TYPE,
            "subject":[
                {"name":"bin/mainframe-env","digest":digest()},
                {"name":"bin/mainframe-env-server","digest":digest()}
            ],
            "predicateType":SLSA_PREDICATE_TYPE,
            "predicate":{
                "buildDefinition":{
                    "buildType":policy.build_type,
                    "externalParameters":{
                        "source":"git+https://example.test/repository@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "version":"0.8.3",
                        "target":"test",
                        "profile":"release",
                        "allFeatures":true,
                        "locked":true
                    },
                    "internalParameters":{
                        "invocationUri":"urn:uuid:12345678-1234-4234-8234-123456789abc",
                        "ciInputLockSha256":"a".repeat(64)
                    },
                    "resolvedDependencies":[
                        resource("https://example.test/source"),
                        resource("https://example.test/lock"),
                        resource("https://example.test/toolchain"),
                        resource("https://example.test/config"),
                        resource("https://example.test/sqlite"),
                        resource("https://example.test/postgres"),
                        resource("https://example.test/build-script"),
                        resource("https://example.test/cyclonedx"),
                        resource("https://example.test/jsf"),
                        resource("https://example.test/spdx")
                    ]
                },
                "runDetails":{
                    "builder":{
                        "id":policy.builder_id,
                        "version":{"builder":"1"},
                        "builderDependencies":[
                            resource("https://example.test/jenkins"),
                            resource("https://example.test/jenkinsfile"),
                            resource("https://example.test/ci-lock"),
                            resource("https://example.test/plugin-lock"),
                            resource("https://example.test/trust-policy")
                        ]
                    },
                    "metadata":{"invocationId":"urn:uuid:12345678-1234-4234-8234-123456789abc"},
                    "byproducts":[
                        resource("urn:mainframe-env:release:0.8.3:test:manifest"),
                        resource("urn:mainframe-env:release:0.8.3:test:sbom"),
                        resource("urn:mainframe-env:release:0.8.3:test:build-inputs"),
                        resource("urn:mainframe-env:release:0.8.3:test:licenses")
                    ]
                }
            }
        })
    }

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }

    #[test]
    fn dsse_authenticates_payload_type_payload_and_signer_before_parsing() {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("generate key");
        let policy = test_policy(pkcs8.as_ref());
        let statement = test_statement(&policy);
        let envelope = sign_with_pkcs8(&statement, &policy, pkcs8.as_ref()).expect("sign");
        let other = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("other key");
        assert!(sign_with_pkcs8(&statement, &policy, other.as_ref()).is_err());
        let value: Value = serde_json::from_slice(&envelope).expect("envelope JSON");
        assert_eq!(verify_envelope_value(&value, &policy).unwrap(), statement);
        for field in ["payload", "payloadType", "signatures"] {
            let mut changed = value.clone();
            match field {
                "payload" => changed[field] = Value::String(BASE64.encode(b"{}")),
                "payloadType" => changed[field] = Value::String("application/json".into()),
                _ => changed[field][0]["sig"] = Value::String(BASE64.encode([0_u8; 64])),
            }
            assert!(
                verify_envelope_value(&changed, &policy).is_err(),
                "accepted changed {field}"
            );
        }
    }

    #[test]
    fn official_statement_validation_rejects_non_uri_builder_and_bad_digest() {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("generate key");
        let policy = test_policy(pkcs8.as_ref());
        let mut statement = test_statement(&policy);
        statement["subject"][0]["digest"]["sha256"] = Value::String("short".into());
        assert!(validate_provenance_statement(&statement, &policy).is_err());
        let mut bad_policy = policy.clone();
        bad_policy.builder_id = "local-builder".into();
        statement = test_statement(&bad_policy);
        assert!(validate_provenance_statement(&statement, &bad_policy).is_err());
        statement = test_statement(&policy);
        for pointer in [
            "/predicate/buildDefinition/internalParameters/invocationUri",
            "/predicate/runDetails/metadata/invocationId",
        ] {
            statement
                .pointer_mut(pointer)
                .unwrap()
                .clone_from(&Value::String("urn:mainframe-env-v0.8.3-local".into()));
        }
        assert!(validate_provenance_statement(&statement, &policy).is_err());
    }

    #[test]
    fn retained_cyclonedx_schemas_are_exact_and_reject_invalid_boms() {
        let schema = decoded_schema(CYCLONEDX_SCHEMA_B64, CYCLONEDX_SCHEMA_SHA256).unwrap();
        assert_eq!(
            schema["$id"],
            "http://cyclonedx.org/schema/bom-1.6.schema.json"
        );
        assert!(validate_official_cyclonedx_schema(&json!({"bomFormat":"not-cyclonedx"})).is_err());
    }

    #[test]
    fn target_sbom_is_the_exact_production_closure_and_graph() {
        let root = repository_root();
        let target = "aarch64-apple-darwin";
        let report = crate::release_licenses::generate(&root, target).expect("production graph");
        let sbom = generate_sbom(
            &report.sbom_graph,
            target,
            "0.8.3",
            &"a".repeat(40),
            &"b".repeat(64),
        )
        .expect("exact SBOM");
        assert_eq!(
            sbom["components"].as_array().unwrap().len(),
            report.sbom_graph.packages.len()
        );
        assert_eq!(
            sbom["dependencies"].as_array().unwrap().len(),
            report.sbom_graph.packages.len() + 1
        );
        assert!(report.sbom_graph.packages.len() < report.production_packages);
        for excluded in ["xtask", "mainframe-env-conformance", "proptest", "cc"] {
            assert!(
                !report
                    .sbom_graph
                    .packages
                    .values()
                    .any(|package| package.name == excluded)
            );
        }
        let mut missing_edge = sbom.clone();
        missing_edge["dependencies"].as_array_mut().unwrap().pop();
        assert!(validate_sbom(&missing_edge, &report.sbom_graph, target, "0.8.3").is_err());
        let mut extra_component = sbom.clone();
        extra_component["components"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"library","bom-ref":"urn:extra","name":"extra","version":"1"}));
        assert!(validate_sbom(&extra_component, &report.sbom_graph, target, "0.8.3").is_err());
    }

    #[test]
    fn repository_provenance_uses_policy_uris_unique_invocation_and_exact_resources() {
        let root = repository_root();
        let materials = ProvenanceMaterials {
            version: "0.8.3",
            target: "aarch64-apple-darwin",
            source_commit: &"a".repeat(40),
            source_digest: &"b".repeat(64),
            server_digest: &"c".repeat(64),
            cli_digest: &"d".repeat(64),
            manifest_digest: &"e".repeat(64),
            sbom_digest: &"f".repeat(64),
            build_inputs_digest: &"1".repeat(64),
            licenses_digest: &"2".repeat(64),
        };
        let invocation = "urn:uuid:12345678-1234-4234-8234-123456789abc";
        let statement = provenance_statement(&root, &materials, invocation).expect("provenance");
        assert_eq!(invocation_id(&statement).unwrap(), invocation);
        assert!(
            statement["predicate"]["buildDefinition"]["buildType"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert!(
            statement["predicate"]["runDetails"]["builder"]["id"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert!(validate_release_invocation_id_value("urn:mainframe-env-v0.8.3-local").is_err());
    }

    #[test]
    fn configured_release_key_matches_policy_when_supplied() {
        if env::var_os("MAINFRAME_ENV_RELEASE_SIGNING_KEY").is_none() {
            return;
        }
        let root = repository_root();
        let statement = test_statement(&trust_policy(&root).expect("repository trust policy"));
        let envelope = sign_statement(&root, &statement).expect("configured release signer");
        assert_eq!(verify_envelope(&root, &envelope).unwrap(), statement);
    }
}
