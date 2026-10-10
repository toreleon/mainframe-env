//! CV-202.package-state-schema. Private real-owner controls; no licensed credit.
//! Requires the accepted identity seal and manager-approved jsonschema dev edge.

use super::{
    APPLICATION_PUBLICATION_CONTRACT, ApplicationPublicationState, PublicationAction,
    PublicationSectionState, install_publication_state, rollback_publication_state,
};
use mainframe_env_application::{
    APPLICATION_PACKAGE_V2_CONTRACT, APPLICATION_PACKAGE_V3_CONTRACT, AbiLibrary, AbiMember,
    ApplicationInstallerV2, ApplicationManifest, ApplicationPackage, ApplicationPackageV2,
    ApplicationSections, BatchController, BatchControllerKind, EntryKind, HostSubsystem,
    ImsDefinition, ImsSeedRow, InstallProblem, InstallState, MqResource, MqResourceKind,
    PackageEntry, PackageLimits, PackageSignature, PackageSignatureVerifier, SecurityResource,
    SqlColumn, SqlSeedRow, SqlTable, package_generation_identity, package_v2_identity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const PACKAGE_ID: &str = "https://mainframe-env.invalid/schemas/application-package@2";
const INSTALLER_ID: &str = "https://mainframe-env.invalid/schemas/application-installer@1";
const PUBLICATION_ID: &str = "https://mainframe-env.invalid/schemas/application-publication@1";
const APPLICATION: &str = "SCHEMA-CONTROL";
const PRODUCT: &str = "0.2.0";
const RESOURCES: [&str; 4] = [
    "coverage/schemas/application-package-v2.schema.json",
    "coverage/schemas/application-installer-state.schema.json",
    "coverage/schemas/application-publication-state.schema.json",
    "ims/schemas/ims-metadata.schema.json",
];

struct RefuseRetrieval;

impl jsonschema::Retrieve for RefuseRetrieval {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("No external schema retrieval is permitted: {uri}").into())
    }
}

fn schema_documents() -> Vec<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/subsystems");
    RESOURCES
        .iter()
        .map(|relative| {
            let path = root.join(relative);
            let bytes = std::fs::read(&path).unwrap_or_else(|error| {
                panic!("Cannot read local schema {}: {error}", path.display())
            });
            let schema: Value = serde_json::from_slice(&bytes).unwrap();
            jsonschema::draft202012::meta::validate(&schema).unwrap();
            schema
        })
        .collect()
}

fn validator(id: &str) -> jsonschema::Validator {
    let documents = schema_documents();
    let mut registry = jsonschema::Registry::new().retriever(RefuseRetrieval);
    let mut ids = BTreeSet::new();
    for document in &documents {
        let resource_id = document["$id"].as_str().expect("Local schema needs $id");
        assert!(
            ids.insert(resource_id),
            "Duplicate schema resource: {resource_id}"
        );
        registry = registry.add(resource_id, document).unwrap();
    }
    let registry = registry.prepare().unwrap();
    let schema = documents
        .iter()
        .find(|document| document["$id"] == id)
        .unwrap();
    jsonschema::draft202012::options()
        .offline()
        .with_registry(&registry)
        .build(schema)
        .unwrap()
}

// Optional external payload emission happens before an assertion, retaining red
// cases as well. These are synthetic owned DTO outputs, not expected verdicts.
fn fixture(name: &str, value: &Value) {
    let Some(directory) = std::env::var_os("MAINFRAME_ENV_PACKAGE_STATE_FIXTURES") else {
        return;
    };
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    );
    let directory = PathBuf::from(directory);
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    assert!(
        !directory.starts_with(repository),
        "Fixtures must remain outside Git"
    );
    assert_ne!(directory, Path::new("/"));
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
}

fn accepts(validator: &jsonschema::Validator, name: &str, value: &Value) {
    fixture(name, value);
    if let Err(error) = validator.validate(value) {
        panic!("{name}: real owned DTO control rejected: {error}");
    }
}

fn refuses(validator: &jsonschema::Validator, name: &str, value: &Value) {
    fixture(name, value);
    assert!(
        !validator.is_valid(value),
        "{name}: malformed mutation admitted"
    );
}

#[derive(Default)]
struct ControlVerifier {
    calls: AtomicUsize,
    revoked: bool,
}

impl PackageSignatureVerifier for ControlVerifier {
    fn verify(&self, key_id: &str, algorithm: &str, identity: &str, signature: &str) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        !self.revoked
            && key_id == "schema-control-key"
            && algorithm == "schema-control-signature@1"
            && signature == format!("control:{identity}")
    }
}

fn control_package(generation: u64) -> ApplicationPackageV2 {
    let kinds = [
        EntryKind::Source,
        EntryKind::Resource,
        EntryKind::Program,
        EntryKind::Data,
        EntryKind::Profile,
        EntryKind::Migration,
    ];
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for (index, kind) in kinds.into_iter().enumerate() {
        let bytes = format!("owned-schema-control-{index}").into_bytes();
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        entries.push(PackageEntry {
            path: format!("entry/{index}"),
            kind,
            sha256: digest.clone(),
            bytes: bytes.len(),
            depends_on: if index == 0 {
                Vec::new()
            } else {
                vec!["entry/0".into()]
            },
        });
        blobs.insert(digest, bytes);
    }
    let member_blob = entries[0].sha256.clone();
    let mut package = ApplicationPackageV2 {
        base: ApplicationPackage {
            manifest: ApplicationManifest {
                name: APPLICATION.into(),
                version: "1.0.0".into(),
                target_product: PRODUCT.into(),
                entries,
            },
            blobs,
        },
        generation,
        sections: ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V3_CONTRACT.into(),
            host_abi_libraries: vec![AbiLibrary {
                id: "CONTROL-ABI".into(),
                subsystem: HostSubsystem::Cics,
                version: "1".into(),
                members: vec![AbiMember {
                    name: "CONTROL".into(),
                    blob_sha256: member_blob,
                }],
            }],
            sql_tables: vec![SqlTable {
                name: "CONTROL.TABLE".into(),
                columns: vec![SqlColumn {
                    name: "ID".into(),
                    nullable: false,
                }],
                primary_key: vec!["ID".into()],
            }],
            sql_rows: vec![SqlSeedRow {
                table: "CONTROL.TABLE".into(),
                values: BTreeMap::from([("ID".into(), "1".into())]),
            }],
            ims_definitions: vec![ImsDefinition {
                name: "CONTROLDB".into(),
                segments: BTreeSet::from(["ROOT".into()]),
            }],
            ims_rows: vec![ImsSeedRow {
                definition: "CONTROLDB".into(),
                segment: "ROOT".into(),
                values: BTreeMap::from([("ID".into(), "1".into())]),
            }],
            ims_metadata: None,
            ims_tm: None,
            mq_resources: vec![MqResource {
                name: "CONTROL.QUEUE".into(),
                kind: MqResourceKind::Queue,
                target: None,
                controller: Some("CONTROL-CONTROLLER".into()),
            }],
            batch_controllers: vec![BatchController {
                name: "CONTROL-CONTROLLER".into(),
                program: "entry/2".into(),
                kind: BatchControllerKind::CobolProgram,
                properties: BTreeMap::from([("commit".into(), "step".into())]),
            }],
            security_resources: vec![SecurityResource {
                class: "FACILITY".into(),
                profile: "CONTROL.EXECUTE".into(),
                owner: "CONTROL".into(),
            }],
        },
        signature: PackageSignature {
            algorithm: "schema-control-signature@1".into(),
            key_id: "schema-control-key".into(),
            value: "unassigned".into(),
        },
    };
    package.signature.value = format!("control:{}", package_generation_identity(&package).unwrap());
    package
}

fn installer() -> ApplicationInstallerV2 {
    ApplicationInstallerV2::new(
        PRODUCT,
        PackageLimits::default(),
        Arc::new(ControlVerifier::default()),
    )
}

fn export(installer: &ApplicationInstallerV2) -> Value {
    serde_json::from_slice(&installer.state_payload().unwrap()).unwrap()
}

fn reopen(
    value: &Value,
    verifier: Arc<ControlVerifier>,
) -> Result<ApplicationInstallerV2, InstallProblem> {
    ApplicationInstallerV2::from_state_payload(
        PRODUCT,
        PackageLimits::default(),
        verifier,
        &serde_json::to_vec(value).unwrap(),
    )
}

fn ready_state() -> Value {
    let owner = installer();
    assert_eq!(
        owner.install(&control_package(1)).unwrap().state,
        InstallState::Ready
    );
    export(&owner)
}

fn publication(generation: u64, rollback: bool) -> ApplicationPublicationState {
    let package = control_package(generation);
    let record = installer().stage(&package).unwrap();
    let state = if rollback {
        rollback_publication_state(&package, &record, true)
    } else {
        install_publication_state(&package, &record, true)
    };
    assert_eq!(state.schema_version, APPLICATION_PUBLICATION_CONTRACT);
    assert_eq!(state.generation, generation);
    assert_eq!(state.ims, PublicationSectionState::Pending);
    state
}

#[test]
fn schemas_compile_offline_with_local_reference_closure() {
    for id in [PACKAGE_ID, INSTALLER_ID, PUBLICATION_ID] {
        let _ = validator(id);
    }
    let unresolved = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": "https://mainframe-env.invalid/unregistered-schema",
    });
    assert!(
        jsonschema::draft202012::options()
            .offline()
            .build(&unresolved)
            .is_err()
    );
    assert!(
        jsonschema::Registry::new()
            .retriever(RefuseRetrieval)
            .add("https://mainframe-env.invalid/local-root", &unresolved)
            .unwrap()
            .prepare()
            .is_err()
    );
}

#[test]
fn publication_actual_install_and_rollback_states_include_ims() {
    let schema = validator(PUBLICATION_ID);
    for rollback in [false, true] {
        let state = publication(1, rollback);
        assert_eq!(
            state.action,
            if rollback {
                PublicationAction::Rollback
            } else {
                PublicationAction::Install
            }
        );
        let name = if rollback {
            "publication-rollback-ims"
        } else {
            "publication-install-ims"
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["ims"], "pending");
        assert_eq!(
            serde_json::from_value::<ApplicationPublicationState>(value.clone()).unwrap(),
            state
        );
        accepts(&schema, name, &value);
    }
}

#[test]
fn publication_historical_absent_ims_defaults_in_owned_reader() {
    let schema = validator(PUBLICATION_ID);
    let mut state = publication(1, false);
    state.controllers = PublicationSectionState::Applied;
    state.db2 = PublicationSectionState::NotApplicable;
    state.ims = PublicationSectionState::NotApplicable;
    state.complete = true;
    let mut historical = serde_json::to_value(&state).unwrap();
    historical.as_object_mut().unwrap().remove("ims");
    fixture("publication-historical-absent-ims", &historical);
    let decoded: ApplicationPublicationState = serde_json::from_value(historical.clone()).unwrap();
    assert_eq!(decoded.ims, PublicationSectionState::NotApplicable);
    assert_eq!(decoded, state);
    accepts(&schema, "publication-historical-absent-ims", &historical);
    let emitted = serde_json::to_value(&decoded).unwrap();
    assert_eq!(emitted["ims"], "not-applicable");
    accepts(&schema, "publication-historical-reader-reexport", &emitted);
}

#[test]
fn publication_completion_refuses_every_unsettled_section() {
    let schema = validator(PUBLICATION_ID);
    let mut state = publication(1, false);
    for settled in [
        PublicationSectionState::Applied,
        PublicationSectionState::NotApplicable,
    ] {
        state.controllers = settled;
        state.db2 = settled;
        state.ims = settled;
        state.complete = true;
        let name = if settled == PublicationSectionState::Applied {
            "publication-complete-applied"
        } else {
            "publication-complete-not-applicable"
        };
        accepts(&schema, name, &serde_json::to_value(&state).unwrap());
    }
    state.controllers = PublicationSectionState::Applied;
    state.db2 = PublicationSectionState::Applied;
    state.ims = PublicationSectionState::Applied;
    for section in ["controllers", "db2", "ims"] {
        for unsettled in ["pending", "applying", "failed"] {
            let mut invalid = serde_json::to_value(&state).unwrap();
            invalid[section] = json!(unsettled);
            // Serde knows the enum, but does not enforce completion consistency.
            serde_json::from_value::<ApplicationPublicationState>(invalid.clone()).unwrap();
            refuses(
                &schema,
                &format!("publication-complete-{section}-{unsettled}"),
                &invalid,
            );
            invalid["complete"] = json!(false);
            accepts(
                &schema,
                &format!("publication-partial-{section}-{unsettled}"),
                &invalid,
            );
        }
    }
}

#[test]
fn publication_unknown_and_missing_fields_refuse() {
    let schema = validator(PUBLICATION_ID);
    let valid = serde_json::to_value(publication(1, false)).unwrap();
    for section in ["controllers", "db2", "ims"] {
        let mut invalid = valid.clone();
        invalid[section] = json!("unknown-section-state");
        refuses(
            &schema,
            &format!("publication-unknown-{section}-state"),
            &invalid,
        );
        assert!(serde_json::from_value::<ApplicationPublicationState>(invalid).is_err());
    }
    for (field, replacement) in [
        ("action", json!("upgrade")),
        ("ims", Value::Null),
        (
            "schema_version",
            json!("mainframe-env.application-publication@999"),
        ),
        ("complete", json!("true")),
        ("identity", json!("sha256:invalid")),
        ("package", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = replacement;
        refuses(&schema, &format!("publication-invalid-{field}"), &invalid);
    }
    for field in ["mq", "unexpected"] {
        let mut invalid = valid.clone();
        invalid[field] = json!("applied");
        // The existing private DTO ignores extra keys; structural strictness is
        // a schema claim here, not a proposed runtime serde change.
        serde_json::from_value::<ApplicationPublicationState>(invalid.clone()).unwrap();
        refuses(
            &schema,
            &format!("publication-unknown-field-{field}"),
            &invalid,
        );
    }
    for field in [
        "schema_version",
        "package",
        "generation",
        "identity",
        "action",
        "controllers",
        "db2",
        "complete",
    ] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().unwrap().remove(field);
        refuses(&schema, &format!("publication-missing-{field}"), &invalid);
        assert!(serde_json::from_value::<ApplicationPublicationState>(invalid).is_err());
    }
}

#[test]
fn publication_generation_is_positive_u64() {
    let schema = validator(PUBLICATION_ID);
    for generation in [1, u64::MAX] {
        let state = publication(generation, false);
        let name = if generation == 1 {
            "publication-generation-min"
        } else {
            "publication-generation-max"
        };
        accepts(&schema, name, &serde_json::to_value(state).unwrap());
    }
    let valid = serde_json::to_value(publication(1, false)).unwrap();
    for (name, number) in [
        ("zero", json!(0)),
        ("negative", json!(-1)),
        ("fraction", json!(1.5)),
        ("text", json!("1")),
        ("null", Value::Null),
        (
            "overflow",
            serde_json::from_str::<Value>("18446744073709551616").unwrap(),
        ),
    ] {
        let mut invalid = valid.clone();
        invalid["generation"] = number;
        refuses(&schema, &format!("publication-generation-{name}"), &invalid);
    }
}

#[test]
fn installer_owned_export_ready_staged_empty_and_generation_boundary() {
    let schema = validator(INSTALLER_ID);
    accepts(&schema, "installer-empty-owner", &export(&installer()));
    accepts(&schema, "installer-ready-current", &ready_state());
    let staged = installer();
    staged.stage(&control_package(1)).unwrap();
    let value = export(&staged);
    assert!(value["applications"][0]["selected"].is_null());
    assert_eq!(
        value["applications"][0]["generations"][0]["state"],
        "Staged"
    );
    accepts(&schema, "installer-staged-null", &value);
    let maximum = installer();
    maximum.install(&control_package(u64::MAX)).unwrap();
    let value = export(&maximum);
    accepts(&schema, "installer-generation-max", &value);
    let reopened = reopen(&value, Arc::new(ControlVerifier::default())).unwrap();
    assert_eq!(export(&reopened), value);
}

#[test]
fn installer_real_legacy_and_mixed_retained_domains_reexport() {
    let schema = validator(INSTALLER_ID);
    let owner = installer();
    owner.install(&control_package(1)).unwrap();
    let mut historical = export(&owner);
    // Retain the real exported envelope. Construct/sign @2 through the owned DTO
    // and historical identity entry point; never invent a package JSON object.
    let mut legacy: ApplicationPackageV2 =
        serde_json::from_value(historical["applications"][0]["generations"][0]["package"].clone())
            .unwrap();
    legacy.sections.schema_version = APPLICATION_PACKAGE_V2_CONTRACT.into();
    legacy.signature.value = format!("control:{}", package_v2_identity(&legacy).unwrap());
    historical["applications"][0]["generations"][0]["package"] =
        serde_json::to_value(&legacy).unwrap();
    let retained = reopen(&historical, Arc::new(ControlVerifier::default())).unwrap();
    let legacy_export = export(&retained);
    assert_eq!(legacy_export, historical);
    assert_eq!(
        retained
            .selected_package(APPLICATION)
            .unwrap()
            .unwrap()
            .as_ref(),
        &legacy
    );
    accepts(&schema, "installer-retained-legacy-only", &legacy_export);
    retained.install(&control_package(2)).unwrap();
    retained.stage(&control_package(3)).unwrap();
    retained.rollback(APPLICATION, 1).unwrap();
    let mixed = export(&retained);
    let reopened = reopen(&mixed, Arc::new(ControlVerifier::default())).unwrap();
    assert_eq!(export(&reopened), mixed);
    assert_eq!(
        reopened.selected(APPLICATION).unwrap().unwrap().generation,
        1
    );
    accepts(&schema, "installer-retained-mixed-legacy-rollback", &mixed);
    reopened.rollback(APPLICATION, 2).unwrap();
    accepts(
        &schema,
        "installer-retained-mixed-current-selection",
        &export(&reopened),
    );
    let mut unselected = export(&reopened);
    unselected["applications"][0]["selected"] = Value::Null;
    let unselected_owner = reopen(&unselected, Arc::new(ControlVerifier::default())).unwrap();
    assert!(unselected_owner.selected(APPLICATION).unwrap().is_none());
    assert_eq!(export(&unselected_owner), unselected);
    accepts(
        &schema,
        "installer-retained-mixed-null-ready",
        &export(&unselected_owner),
    );
}

#[test]
fn installer_historical_null_with_ready_generations_remains_null() {
    let schema = validator(INSTALLER_ID);
    let mut retained = ready_state();
    retained["applications"][0]["selected"] = Value::Null;
    let owner = reopen(&retained, Arc::new(ControlVerifier::default())).unwrap();
    assert!(owner.selected(APPLICATION).unwrap().is_none());
    assert_eq!(export(&owner), retained);
    accepts(&schema, "installer-historical-null-ready", &export(&owner));
    owner.rollback(APPLICATION, 1).unwrap();
    accepts(
        &schema,
        "installer-null-ready-explicit-rollback",
        &export(&owner),
    );
}

#[test]
fn installer_arbitrary_packages_unknown_domains_fields_and_states_refuse() {
    let schema = validator(INSTALLER_ID);
    let valid = ready_state();
    for (name, replacement) in [
        ("empty", json!({})),
        ("arbitrary", json!({"arbitrary": true})),
        ("null", Value::Null),
        ("array", json!([])),
    ] {
        let mut invalid = valid.clone();
        invalid["applications"][0]["generations"][0]["package"] = replacement;
        refuses(&schema, &format!("installer-package-{name}"), &invalid);
    }
    for domain in [
        "mainframe-env.application-package@1",
        "mainframe-env.application-package@999",
    ] {
        let mut invalid = valid.clone();
        invalid["applications"][0]["generations"][0]["package"]["sections"]["schema_version"] =
            json!(domain);
        let suffix = if domain.ends_with("@1") {
            "v1"
        } else {
            "unknown"
        };
        refuses(
            &schema,
            &format!("installer-package-domain-{suffix}"),
            &invalid,
        );
        assert!(reopen(&invalid, Arc::new(ControlVerifier::default())).is_err());
    }
    for (name, pointer) in [
        ("root", ""),
        ("application", "/applications/0"),
        ("retained", "/applications/0/generations/0"),
        ("package", "/applications/0/generations/0/package"),
        ("sections", "/applications/0/generations/0/package/sections"),
        (
            "signature",
            "/applications/0/generations/0/package/signature",
        ),
    ] {
        let mut invalid = valid.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), json!(true));
        refuses(
            &schema,
            &format!("installer-unknown-{name}-field"),
            &invalid,
        );
    }
    for (name, pointer, replacement) in [
        (
            "contract",
            "/schema_version",
            json!("mainframe-env.application-installer@999"),
        ),
        (
            "state",
            "/applications/0/generations/0/state",
            json!("Broken"),
        ),
        (
            "subsystem",
            "/applications/0/generations/0/package/sections/host_abi_libraries/0/subsystem",
            json!("Other"),
        ),
        (
            "controller-kind",
            "/applications/0/generations/0/package/sections/batch_controllers/0/kind",
            json!("Other"),
        ),
        (
            "mq-kind",
            "/applications/0/generations/0/package/sections/mq_resources/0/kind",
            json!("Other"),
        ),
        (
            "entry-kind",
            "/applications/0/generations/0/package/base/manifest/entries/0/kind",
            json!("Other"),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        refuses(&schema, &format!("installer-unknown-{name}"), &invalid);
    }
    for field in ["base", "generation", "sections", "signature"] {
        let mut invalid = valid.clone();
        invalid["applications"][0]["generations"][0]["package"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        refuses(
            &schema,
            &format!("installer-package-missing-{field}"),
            &invalid,
        );
    }
    let mut invalid = valid;
    invalid["applications"][0]
        .as_object_mut()
        .unwrap()
        .remove("selected");
    refuses(&schema, "installer-missing-selected", &invalid);
    assert!(reopen(&invalid, Arc::new(ControlVerifier::default())).is_err());
}

#[test]
fn installer_selection_and_nested_generation_scalar_bounds_refuse() {
    let schema = validator(INSTALLER_ID);
    let valid = ready_state();
    for (field, pointer) in [
        ("selected", "/applications/0/selected"),
        (
            "generation",
            "/applications/0/generations/0/package/generation",
        ),
    ] {
        for (name, number) in [
            ("zero", json!(0)),
            ("negative", json!(-1)),
            ("fraction", json!(1.5)),
            ("text", json!("1")),
            (
                "overflow",
                serde_json::from_str::<Value>("18446744073709551616").unwrap(),
            ),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = number;
            refuses(&schema, &format!("installer-{field}-{name}"), &invalid);
        }
    }
}

#[test]
fn installer_schema_does_not_replace_typed_topology_reconstruction() {
    let schema = validator(INSTALLER_ID);
    let owner = installer();
    owner.install(&control_package(1)).unwrap();
    owner.stage(&control_package(2)).unwrap();
    let before = owner.state_payload().unwrap();
    let valid = export(&owner);
    for (name, case, expected) in [
        ("selected-missing", 0, InstallProblem::InvalidIdentity),
        ("selected-staged", 1, InstallProblem::InvalidIdentity),
        ("duplicate-generation", 2, InstallProblem::DuplicateEntry),
        ("application-mismatch", 3, InstallProblem::InvalidIdentity),
        ("duplicate-application", 4, InstallProblem::DuplicateEntry),
    ] {
        let mut invalid = valid.clone();
        match case {
            0 => invalid["applications"][0]["selected"] = json!(3),
            1 => invalid["applications"][0]["selected"] = json!(2),
            2 => {
                let duplicate = invalid["applications"][0]["generations"][0].clone();
                invalid["applications"][0]["generations"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            3 => invalid["applications"][0]["application"] = json!("OTHER"),
            4 => {
                let duplicate = invalid["applications"][0].clone();
                invalid["applications"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            _ => unreachable!(),
        }
        accepts(
            &schema,
            &format!("installer-structural-only-{name}"),
            &invalid,
        );
        let verifier = Arc::new(ControlVerifier::default());
        assert!(matches!(reopen(&invalid, verifier.clone()), Err(problem) if problem == expected));
        assert_eq!(
            verifier.calls.load(Ordering::Relaxed),
            0,
            "Topology must precede signatures"
        );
        assert_eq!(owner.state_payload().unwrap(), before);
    }
}

#[test]
fn installer_structural_success_does_not_authenticate_signature_or_trust() {
    let schema = validator(INSTALLER_ID);
    let valid = ready_state();
    let mut corrupt = valid.clone();
    corrupt["applications"][0]["generations"][0]["package"]["signature"]["value"] =
        json!("corrupt-control-signature");
    accepts(
        &schema,
        "installer-structural-only-corrupt-signature",
        &corrupt,
    );
    assert!(matches!(
        reopen(&corrupt, Arc::new(ControlVerifier::default())),
        Err(InstallProblem::InvalidSignature)
    ));
    accepts(&schema, "installer-structural-only-revoked-trust", &valid);
    let revoked = Arc::new(ControlVerifier {
        revoked: true,
        ..ControlVerifier::default()
    });
    assert!(matches!(
        reopen(&valid, revoked),
        Err(InstallProblem::InvalidSignature)
    ));
    let mut unresolved = valid.clone();
    let mut package: ApplicationPackageV2 =
        serde_json::from_value(unresolved["applications"][0]["generations"][0]["package"].clone())
            .unwrap();
    package.sections.batch_controllers[0].program = "entry/0".into();
    package.signature.value = format!("control:{}", package_generation_identity(&package).unwrap());
    unresolved["applications"][0]["generations"][0]["package"] =
        serde_json::to_value(package).unwrap();
    accepts(
        &schema,
        "installer-structural-only-unresolved-controller",
        &unresolved,
    );
    let verifier = Arc::new(ControlVerifier::default());
    assert!(matches!(
        reopen(&unresolved, verifier.clone()),
        Err(InstallProblem::MissingReference)
    ));
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    // Reopen is called only as the trusted host. No schema or local test
    // authenticates external snapshots, IBM sources, or an arbitrary backup root.
}
