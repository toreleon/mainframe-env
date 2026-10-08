//! Real serialized package DTO controls; structural shape is separate from trust.
use super::*;

fn check_cases(cases: &[(&str, bool)]) {
    let root = repository_root().expect("repository root");
    let schema_path =
        root.join("conformance/subsystems/coverage/schemas/application-package-v2.schema.json");
    let schema = json(&schema_path).expect("owned package schema");
    let validator = coverage_projection::compile_schema(&schema, &schema_path)
        .expect("native offline package schema compilation");
    let mut failures = Vec::new();
    for (name, expected) in cases {
        let path = root
            .join("xtask/tests/fixtures/application_packages")
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
fn native_package_schema_accepts_actual_finite_dto_controls() {
    check_cases(&[
        ("positive-0-serialized.json", true),
        ("positive-0-explicit-null.json", true),
        ("positive-1-serialized.json", true),
        ("positive-1-explicit-null.json", true),
        ("positive-2-serialized.json", true),
        ("positive-3-serialized.json", true),
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
    ]);
}
