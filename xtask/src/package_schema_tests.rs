//! Real serialized package DTO controls; structural shape is separate from trust.
use super::*;

fn check_cases(cases: &[(&str, bool)]) {
    check_schema_cases(
        "application-package-v2.schema.json",
        "application_packages",
        cases,
    );
}

fn check_schema_cases(schema_name: &str, fixture_family: &str, cases: &[(&str, bool)]) {
    let root = repository_root().expect("repository root");
    let schema_path = root
        .join("conformance/subsystems/coverage/schemas")
        .join(schema_name);
    let schema = json(&schema_path).expect("owned package schema");
    let validator = coverage_projection::compile_schema(&schema, &schema_path)
        .expect("native offline package schema compilation");
    let mut failures = Vec::new();
    for (name, expected) in cases {
        let path = root
            .join("xtask/tests/fixtures")
            .join(fixture_family)
            .join(name);
        let bytes = fs::read(&path).expect("mandatory serialized DTO fixture");
        assert!(bytes.len() <= 32_768, "bounded fixture: {name}");
        let instance: Value = serde_json::from_slice(&bytes).expect("fixture JSON");
        let valid = validator.is_valid(&instance);
        println!("package-schema case={name} expected={expected} actual={valid}");
        if valid != *expected {
            failures.push((
                *name,
                validator
                    .iter_errors(&instance)
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "package schema fixture mismatches: {failures:?}"
    );
}

#[test]
fn native_installer_schema_uses_owned_package_and_ims_reference_closure() {
    check_schema_cases(
        "application-installer-state.schema.json",
        "application_states",
        &[
            ("installer-empty-owner.json", true),
            ("installer-retained-mixed-null-ready.json", true),
            ("installer-generation-overflow.json", false),
            ("installer-selected-overflow.json", false),
            ("installer-package-arbitrary.json", false),
            ("installer-package-domain-unknown.json", false),
        ],
    );
}

#[test]
fn native_installer_schema_refuses_unregistered_external_references() {
    let root = repository_root().unwrap();
    let path = root
        .join("conformance/subsystems/coverage/schemas/application-installer-state.schema.json");
    let mut schema = json(&path).unwrap();
    schema["properties"]["applications"]["items"]["properties"]["generations"]["items"]["properties"]
        ["package"]["$ref"] = "https://mainframe-env.invalid/schemas/unregistered@1".into();
    assert!(coverage_projection::compile_schema(&schema, &path).is_err());
}

#[test]
fn native_installer_schema_accepts_owned_space_and_utf8_names() {
    use mainframe_env_application::{
        ApplicationInstallerV2, ApplicationPackageV2, PackageLimits, PackageSignature,
        PackageSignatureVerifier,
    };
    use std::sync::Arc;
    // An explicit synthetic trust port establishes owned serialization controls,
    // without asserting production signature authentication.
    struct StructuralTrust;
    impl PackageSignatureVerifier for StructuralTrust {
        fn verify(&self, _: &str, algorithm: &str, _: &str, signature: &str) -> bool {
            algorithm == "schema-control@1" && signature == "opaque-control"
        }
    }
    let root = repository_root().unwrap();
    let path = root
        .join("conformance/subsystems/coverage/schemas/application-installer-state.schema.json");
    let validator = coverage_projection::compile_schema(&json(&path).unwrap(), &path).unwrap();
    let input = fs::read(
        root.join("xtask/tests/fixtures/application_packages/positive-u64-generation.json"),
    )
    .unwrap();
    let mut refused = Vec::new();
    for name in ["schema control", "café demo"] {
        let mut package: ApplicationPackageV2 = serde_json::from_slice(&input).unwrap();
        package.base.manifest.name = name.into();
        package.signature = PackageSignature {
            algorithm: "schema-control@1".into(),
            key_id: "structural".into(),
            value: "opaque-control".into(),
        };
        let installer = ApplicationInstallerV2::new(
            "0.2.0",
            PackageLimits::default(),
            Arc::new(StructuralTrust),
        );
        installer
            .install(&package)
            .expect("owned kernel admits the control");
        let payload = installer.state_payload().unwrap();
        let owned: Value = serde_json::from_slice(&payload).unwrap();
        let valid = validator.is_valid(&owned);
        println!("installer owned name={name:?} native_valid={valid}");
        if !valid {
            refused.push(name);
        }
    }
    assert!(
        refused.is_empty(),
        "owned installer names rejected by projection: {refused:?}"
    );
}

#[test]
fn native_package_schema_accepts_actual_finite_dto_controls() {
    check_cases(&[
        ("positive-0-serialized.json", true),
        ("positive-0-explicit-null.json", true),
        ("positive-1-serialized.json", true),
        ("positive-1-explicit-null.json", true),
        ("positive-2-serialized.json", true),
        ("positive-3-serialized.json", true),
        ("positive-u64-generation.json", true),
    ]);
}

#[test]
fn native_package_schema_rejects_malformed_finite_dto_shapes() {
    check_cases(&[
        ("negative-unknown-domain.json", false),
        ("negative-second-discriminator.json", false),
        ("negative-missing-base.json", false),
        ("negative-slug-instead-of-dto-enum.json", false),
        ("negative-out-of-range-blob-byte.json", false),
        ("negative-unknown-mq-kind.json", false),
        ("negative-missing-sql-table.json", false),
        ("negative-unknown-ims-row-field.json", false),
        ("negative-zero-generation.json", false),
        ("negative-generation-overflow.json", false),
    ]);
}
