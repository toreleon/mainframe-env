//! Offline READACCT bundle gate and modernize-ai schema projection.

use super::{TaskResult, file_digest, validate_schema_instance};
use base64::Engine;
use mainframe_env_conformance::capture_carddemo_readacct_from_env;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const VENDOR: &str = "conformance/subsystems/jes/schemas/vendor/modernize-ai";
const OUTPUT: &str = "carddemo-readacct-bundle.json";

pub(super) fn run(root: &Path, _check: bool) -> TaskResult {
    let bundle = capture_carddemo_readacct_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|error| error.to_string())?;
    let schema =
        read_json(&root.join("conformance/subsystems/jes/schemas/run-bundle@1.schema.json"))?;
    validate_schema_instance(&schema, &bundle, &root.join(OUTPUT))?;
    require_replay_digest(&bundle)?;
    verify_vendor(root)?;
    let (manifest, observation) = project(&bundle)?;
    validate_vendor(root, "execution-manifest-1.0.0.schema.json", &manifest)?;
    validate_vendor(root, "observation-3.0.0.schema.json", &observation)?;
    println!(
        "READACCT rc={} outputs={} replay={} wall={}..{}",
        bundle["logical"]["job"]["return_code"],
        bundle["logical"]["outputs"].as_array().map_or(0, Vec::len),
        bundle["replay_digest"],
        bundle["wall_clock"]["started_epoch_micros"],
        bundle["wall_clock"]["finished_epoch_micros"]
    );
    Ok(())
}

fn read_json(path: &Path) -> TaskResult<Value> {
    serde_json::from_slice(&fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn digest_value(value: &Value) -> TaskResult<String> {
    Ok(digest(
        &serde_json::to_vec(value).map_err(|error| error.to_string())?,
    ))
}

fn require_replay_digest(bundle: &Value) -> TaskResult {
    if bundle["replay_digest"] == digest_value(&bundle["logical"])? {
        Ok(())
    } else {
        Err("READACCT replay digest does not bind the canonical logical projection".into())
    }
}

fn verify_vendor(root: &Path) -> TaskResult {
    let manifest = read_json(&root.join(VENDOR).join("manifest.json"))?;
    if manifest["source_commit"] != "1ebd5a975f1c92a9032c7d041e7bbc89327e25b9" {
        return Err("modernize-ai schema source commit differs".into());
    }
    let files = manifest["files"]
        .as_array()
        .ok_or("vendor manifest lacks files")?;
    if files.len() != 5 {
        return Err("vendor manifest must list exactly five schemas".into());
    }
    let expected = [
        (
            "execution-manifest-1.0.0.schema.json",
            "6dadc283c3646cf9cfbd62ae733780c29b49fbb80c3d27db328daa19e96f3532",
        ),
        (
            "observation-3.0.0.schema.json",
            "e09bc775d4b99432d2a53a307b802783b2a10361f4396d1b2eaecca4b9c30309",
        ),
        (
            "runner-result-manifest-1.0.0.schema.json",
            "c8622a77d598c0f892cd6ff0f4dbf4839e42899df98f46bd6ba9fbd23271953b",
        ),
        (
            "p5-common-1.0.0.schema.json",
            "d8eafa6002afb54a2557fa42691ca843026da8c416ef66aa205d365c39e93ead",
        ),
        (
            "p4-common-1.0.0.schema.json",
            "6894769dfc500c474248b126b5807f5b6993ad84afec68ca61b07a31ae215623",
        ),
    ];
    for (name, hash) in expected {
        let entry = files
            .iter()
            .find(|entry| {
                entry["source_path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(&format!("/{name}")))
            })
            .ok_or_else(|| format!("vendor manifest lacks {name}"))?;
        if entry["sha256"] != hash || file_digest(&root.join(VENDOR).join(name))? != hash {
            return Err(format!(
                "vendored modernize-ai schema {name} differs from pinned SHA-256"
            ));
        }
    }
    Ok(())
}

fn validate_vendor(root: &Path, name: &str, instance: &Value) -> TaskResult {
    vendor_validator(root, name)?
        .validate(instance)
        .map_err(|error| format!("{name} projection: {error}"))
}

pub(super) fn check_vendor_schemas(root: &Path) -> TaskResult {
    verify_vendor(root)?;
    let manifest = read_json(&root.join(VENDOR).join("manifest.json"))?;
    for entry in manifest["files"]
        .as_array()
        .ok_or("vendor manifest lacks files")?
    {
        let source = entry["source_path"]
            .as_str()
            .ok_or("vendor entry lacks source_path")?;
        let name = Path::new(source)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("vendor entry lacks schema filename")?;
        vendor_validator(root, name)?;
    }
    Ok(())
}

fn vendor_validator(root: &Path, name: &str) -> TaskResult<jsonschema::Validator> {
    let base = root.join(VENDOR);
    let p4 = read_json(&base.join("p4-common-1.0.0.schema.json"))?;
    let p5 = read_json(&base.join("p5-common-1.0.0.schema.json"))?;
    let registry = jsonschema::Registry::new()
        .add("https://schemas.modernize.ai/p4-common/1.0.0", &p4)
        .map_err(|error| error.to_string())?
        .add("https://schemas.modernize.ai/p5-common/1.0.0", &p5)
        .map_err(|error| error.to_string())?
        .prepare()
        .map_err(|error| error.to_string())?;
    let schema = read_json(&base.join(name))?;
    jsonschema::draft202012::meta::validate(&schema)
        .map_err(|error| format!("{name} is not Draft 2020-12: {error}"))?;
    jsonschema::draft202012::options()
        .offline()
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(&schema)
        .map_err(|error| format!("compile {name}: {error}"))
}

fn reference(label: &str, value: &Value) -> TaskResult<Value> {
    let hash = digest_value(value)?;
    Ok(json!({"artifactId": format!("art_sha256_{}", &hash[7..]),
        "artifactType": label, "schemaVersion": "1.0.0",
        "envelopeDigest": hash, "payloadDigest": hash}))
}

fn na(label: &str) -> TaskResult<Value> {
    reference("NotApplicableLocal", &json!({"not_applicable": label}))
}

fn digest_ref(id: &str, version: &str, hash: &Value) -> Value {
    json!({"id": id, "version": version, "digest": hash})
}

fn project(bundle: &Value) -> TaskResult<(Value, Value)> {
    let logical = &bundle["logical"];
    let source = &logical["sources"];
    let compiler = &logical["compiler"];
    let initial = &logical["initial_state"];
    let outputs = logical["outputs"]
        .as_array()
        .ok_or("bundle outputs missing")?;
    let executable = digest_ref(
        "CBACT01C",
        "1",
        &compiler["installed_artifacts"][0]["artifact"],
    );
    let runtime = digest_ref(
        "mainframe-env-server",
        env!("CARGO_PKG_VERSION"),
        &source["helper_sources"]["crates/apps/mainframe-env-server/src/cobol.rs"],
    );
    let implementation = digest_ref(
        "mainframe-env-interpreter",
        env!("CARGO_PKG_VERSION"),
        &source["helper_sources"]["crates/apps/mainframe-env-server/src/cobol/runtime.rs"],
    );
    let codec = digest_ref("CCSID-37", "1", &initial["records_digest"]);
    let comparator = digest_ref(
        "not-applicable-local-comparator",
        "1",
        &json!(digest(b"not-applicable-local-comparator")),
    );
    let policy = digest_ref(
        "not-applicable-local-policy",
        "1",
        &json!(digest(b"not-applicable-local-policy")),
    );
    let runtime_contract = na("runtime-contract")?;
    let pack = na("technology-pack")?;
    let packet = reference("LocalRunBundleComponent", source)?;
    let replay = reference("LocalRunBundleComponent", logical)?;
    let state = reference("LocalRunBundleComponent", initial)?;
    let build = reference("LocalRunBundleComponent", compiler)?;
    let shim = na("shim")?;
    let migration = na("migration-spec")?;
    let manifest = json!({
        "manifestVersion": "1.0.0",
        "logicalExecutionId": format!("execution_sha256_{}", &bundle["replay_digest"].as_str().ok_or("missing replay digest")?[7..]),
        "scenarioId": "carddemo.readacct", "runtimeContract": runtime_contract,
        "technologyPack": pack, "behaviorPacket": packet, "replayManifest": replay,
        "stateSnapshot": state, "codecProfile": codec, "migrationSpec": migration,
        "comparatorRegistry": comparator, "sensitivityPolicy": policy,
        "runner": {"role": "modern-candidate", "implementation": implementation, "build": build,
            "executable": executable, "runtime": runtime, "shim": shim},
        "deterministicSources": {"clock": "2022-07-06T00:00:00Z", "timezone": "UTC",
            "locale": "en-US", "randomSeed": 0},
        "jobParameters": {"accountLimit": initial["record_count"], "lookupKey": "00000000042",
            "expectedTermination": "success"},
        "initialState": {"namespaceDigest": digest_value(&initial["catalog"])?,
            "materializationDigest": initial["records_digest"], "cleanRequired": true},
        "limits": {"deadlineMillis": 120000, "maxStdoutBytes": 10485760,
            "maxStderrBytes": 10485760, "maxResultBytes": 10485760,
            "maxStateBytes": 104857600, "maxProcesses": 1},
        "authority": {"required": "development-only", "allowedLocal": "development-only"},
        "faultSchedule": null, "expectedOutputTypes": ["Observation", "RunnerResultManifest"],
    });
    let manifest_ref = reference("LocalRunBundleProjection", &manifest)?;
    let spool = logical["spool"].as_array().ok_or("bundle spool missing")?
        .iter().map(|stream| {
            let lines = stream["records"].as_array().ok_or("spool records missing")?
                .iter().enumerate().map(|(index, line)| {
                    let text = line.as_str().ok_or("spool text missing")?;
                    Ok(json!({"lineNumber": index, "text": text, "digest": digest(text.as_bytes())}))
                }).collect::<TaskResult<Vec<_>>>()?;
            Ok(json!({"streamName": stream["name"], "lines": lines, "ordering": "ordered"}))
        }).collect::<TaskResult<Vec<_>>>()?;
    let files = outputs.iter().map(|output| {
        let encoded = output["records_base64"].as_array().ok_or("output records missing")?;
        let records = encoded.iter().enumerate().map(|(index, record)| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(record.as_str()
                .ok_or("record base64 missing")?).map_err(|error| error.to_string())?;
            let hex = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            Ok(json!({"sequence": index, "entityKey": format!("record-{index}"),
                "layoutProfileId": "carddemo.readacct", "layoutProfileVersion": "1.0.0",
                "fields": [], "rawDigest": digest(&bytes), "rawBytesHex": hex}))
        }).collect::<TaskResult<Vec<_>>>()?;
        let size = encoded.iter().map(|record| base64::engine::general_purpose::STANDARD
            .decode(record.as_str().unwrap_or("")).map(|bytes| bytes.len())
            .map_err(|error| error.to_string())).collect::<TaskResult<Vec<_>>>()?
            .into_iter().sum::<usize>();
        Ok(json!({"logicalPath": output["name"], "mediaType": "application/x-mainframe-dataset",
            "sizeBytes": size, "digest": output["records_digest"],
            "records": records, "ordering": "ordered"}))
    }).collect::<TaskResult<Vec<_>>>()?;
    let observation = json!({
        "observationVersion": "3.0.0",
        "provenance": {"authority": "development-only", "executionManifestRef": manifest_ref,
            "runtimeContractRef": manifest["runtimeContract"], "runnerBuildRef": manifest["runner"]["build"],
            "runtimeRef": manifest["runner"]["runtime"], "shimRef": manifest["runner"]["shim"],
            "policyRef": na("policy")?, "comparatorRegistryRef": manifest["comparatorRegistry"],
            "codecProfileRef": manifest["codecProfile"], "packetRef": manifest["behaviorPacket"],
            "replayRef": manifest["replayManifest"], "stateSnapshotRef": manifest["stateSnapshot"],
            "graphRef": na("graph")?, "ruleCatalogRef": na("rule-catalog")?,
            "packRef": manifest["technologyPack"]},
        "fileOutputs": files, "dbMutations": [], "spoolStreams": spool, "eventPlaceholders": [],
        "termination": {"kind": "return-code", "code": 0, "reason": "READACCT completed",
            "abendCode": null, "failurePoint": null},
        "timing": {"deterministicDurationMicros": null,
            "rawWallDurationMicros": bundle["wall_clock"]["duration_micros"]},
        "resources": {"peakMemoryBytes": null, "cpuMillis": null},
        "runtimeTrace": [], "faultBoundary": null, "faultPhase": null,
        "runtimeChannels": {"jobSteps": "observed", "checkpoints": "not-observed",
            "transactions": "not-observed", "idempotency": "not-observed"},
    });
    Ok((manifest, observation))
}
